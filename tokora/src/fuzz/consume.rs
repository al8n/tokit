//! The **consume-tree** driver: the core fuzzer over the complete-mode backtracking surface.
//!
//! A case is a recursively-generated *tree* of operations (the guard borrow structure is inherently
//! nested — a [`Transaction`](crate::Transaction) borrows the input for a lexical scope — so a
//! tree, not a flat list, is
//! the faithful model; every scope is well-bracketed by construction). The executor walks the tree
//! against a live [`InputRef`] over an **error-free** byte-per-token stream, so the shadow model is
//! closed-form and exact: one byte is one token spanning `[o, o+1)`, and the committed cursor
//! advances one token at a time. Lexer-error bytes (and the dedup watermark) are exercised by the
//! [`partial`](super::partial) driver's error-agnostic equivalence oracle instead, keeping these
//! offset/faithfulness oracles airtight.
//!
//! # Oracles checked after every operation
//!
//! - **Faithfulness** — a token *returned* by a consume equals the shadow's token at the pre-op
//!   cursor, and the cursor advances to its end (the committed stream equals the straight-lex
//!   stream).
//! - **Monotonicity** — the committed cursor never moves backward except across a rollback.
//! - **No-trace** — a declined `attempt`/`try_attempt`, a rolled-back guard, and a no-match
//!   `sync_through`/`sync_balanced` leave the cursor **and** the emission count exactly at their
//!   pre-op values (the documented "leaves no trace" law).
//! - **LIFO/pin** — a guard/savepoint rollback restores the cursor and emission count to the exact
//!   begin/savepoint state.
//! - **Termination & no-panic** — every script terminates (bounded generation), and a debug build
//!   reaching any assertion — including the machinery's own witness asserts — is a finding.

use std::{vec, vec::Vec};

use generic_arraydeque::typenum::U4;

use super::{
  fixtures::{FuzzCtx, FuzzError, FuzzKind, FuzzTok, ScriptLexer, kind_of},
  ops::{Coverage, Op},
  rng::Rng,
};
use crate::{
  Commit, InputRef, ParseInput, SimpleSpan, Token,
  cst::event::EventMark,
  emitter::CstEmitter,
  input::{Rollback, StackedTransaction},
  parser::node,
  span::Spanned,
};

/// The maximum nesting depth of generated speculation scopes.
const MAX_DEPTH: usize = 4;

/// The node kind the `Node` op brackets with, and the wrap kind `StartAt` spends — fixed
/// values well inside the u16 space (the recording twin's mapper owns the token kinds).
const K_NODE_KIND: u16 = 70;
const K_WRAP_KIND: u16 = 71;

/// The `InputRef` shape every consume operation drives.
type Ir<'inp, 'cl> = InputRef<'inp, 'cl, ScriptLexer<'inp>, FuzzCtx<'inp>, ()>;

// ── The shadow model ────────────────────────────────────────────────────────────────────────────

/// The closed-form reference: the source bytes and the committed cursor `o`. Over an error-free
/// stream, the token at offset `i` is `kind_of(src[i])` spanning `[i, i+1)`, so every operation's
/// outcome is a pure function of `(src, o)`.
#[derive(Debug, Clone)]
pub(crate) struct Model<'a> {
  src: &'a [u8],
  o: usize,
}

impl<'a> Model<'a> {
  fn new(src: &'a [u8]) -> Self {
    Self { src, o: 0 }
  }

  fn len(&self) -> usize {
    self.src.len()
  }

  /// The kind at the cursor, or `None` at end of input.
  fn kind_here(&self) -> Option<FuzzKind> {
    (self.o < self.src.len()).then(|| kind_of(self.src[self.o]))
  }
}

// ── The operation script (a tree) ────────────────────────────────────────────────────────────────

/// One node of the operation tree: an [`Op`] and, for a scope op, its nested body.
#[derive(Debug, Clone)]
pub(crate) struct Step {
  op: Op,
  body: Vec<Step>,
}

impl Step {
  fn leaf(op: Op) -> Self {
    Self {
      op,
      body: Vec::new(),
    }
  }

  fn scope(op: Op, body: Vec<Step>) -> Self {
    Self { op, body }
  }
}

/// The leaf operations, sampled uniformly by the generator.
const LEAVES: &[Op] = &[
  Op::Next,
  Op::PeekOne,
  Op::Peek,
  Op::TryExpectHit,
  Op::TryExpectMiss,
  Op::TryExpectMap,
  Op::TryExpectAndThen,
  Op::SkipWhile,
  Op::SyncTo,
  Op::SyncThrough,
  Op::SyncBalanced,
  Op::IsEoi,
  Op::IsExhausted,
  Op::Seal,
  Op::Mark,
  Op::StartAt,
  Op::RollbackAcrossMark,
];

/// The speculation-scope operations (each carries a nested body).
const SCOPES: &[Op] = &[
  Op::Node,
  Op::AttemptCommit,
  Op::AttemptDecline,
  Op::TryAttemptOk,
  Op::TryAttemptErr,
  Op::TxnCommit,
  Op::TxnRollback,
  Op::TxnRollbackAbandoningPoints,
  Op::TxnDropRollback,
  Op::TxnDropCommit,
  Op::StackedCommit,
  Op::StackedRollback,
  Op::StackedDropCommit,
];

/// Generates a top-level / attempt / transaction body: a bounded sequence of leaves and (below the
/// depth cap) speculation scopes.
pub(crate) fn gen_seq(rng: &mut Rng, depth: usize) -> Vec<Step> {
  let n = rng.below(5) + if depth == 0 { 2 } else { 1 };
  let mut steps = Vec::with_capacity(n);
  for _ in 0..n {
    // ~45% a scope when depth allows, else a leaf.
    if depth < MAX_DEPTH && rng.chance(45, 100) {
      let op = SCOPES[rng.below(SCOPES.len())];
      let body = if matches!(op, Op::StackedCommit | Op::StackedRollback) {
        gen_stacked_body(rng, depth + 1)
      } else {
        gen_seq(rng, depth + 1)
      };
      steps.push(Step::scope(op, body));
    } else {
      steps.push(Step::leaf(LEAVES[rng.below(LEAVES.len())]));
    }
  }
  steps
}

/// Generates a stacked-transaction body. It always opens with a [`Op::Savepoint`] so the
/// savepoint-consuming ops have a live target, then mixes leaves, savepoints, and savepoint
/// rollbacks/releases.
fn gen_stacked_body(rng: &mut Rng, _depth: usize) -> Vec<Step> {
  let mut steps = vec![Step::leaf(Op::Savepoint)];
  let n = rng.below(5) + 1;
  for _ in 0..n {
    let pick = rng.below(10);
    let op = match pick {
      0 | 1 => Op::Savepoint,
      2 | 3 => Op::RollbackToSavepoint,
      4 => Op::ReleaseSavepoint,
      _ => LEAVES[rng.below(LEAVES.len())],
    };
    steps.push(Step::leaf(op));
  }
  steps
}

// ── The executor ─────────────────────────────────────────────────────────────────────────────────

/// Runs a generated case against a fresh input over `src` (which must be error-free), threading the
/// shadow model and coverage. `exec_seed` seeds the executor-only randomness (which live savepoint
/// to target), keeping the whole case reproducible. Panics — a *finding* — on any oracle violation.
pub(crate) fn run(src: &[u8], script: &[Step], cov: &mut Coverage, exec_seed: u64) {
  let context = crate::input::InputContext::new(
    super::fixtures::CountEmitter::new(),
    super::fixtures::cache(),
  );
  let state = super::fixtures::initial_state(src);
  let mut input =
    crate::input::Input::<'_, ScriptLexer<'_>, FuzzCtx<'_>, ()>::with_state_and_context(
      src, state, context,
    );
  let mut ir = input.as_ref();
  let mut model = Model::new(src);
  let mut marks = Vec::new();
  run_seq(
    &mut ir,
    script,
    &mut model,
    cov,
    &mut Rng::new(exec_seed),
    &mut marks,
    0,
  );
}

/// Runs a sequence of steps in order. `rng` is threaded only because a few executor decisions
/// (which live savepoint to target) are randomized; it does not regenerate the script.
///
/// `marks` is the live retro-wrap anchor stack and `floor` its frame boundary: `StartAt`
/// spends only marks minted in the current frame (above `floor`), because a wrap that
/// crossed a `node()` bracket's boundary would be the ImproperWrap shape materialization
/// refuses; every rollback truncates the stack to its entry length, mirroring the sink's
/// era staleness exactly (a rollback's truncation invalidates precisely the marks minted
/// after its capture).
fn run_seq(
  ir: &mut Ir<'_, '_>,
  steps: &[Step],
  model: &mut Model<'_>,
  cov: &mut Coverage,
  rng: &mut Rng,
  marks: &mut Vec<EventMark>,
  floor: usize,
) {
  for step in steps {
    run_step(ir, step, model, cov, rng, marks, floor);
  }
}

/// Executes one step (leaf or scope). The wildcard-free `match` is a compile-time exhaustiveness
/// prod: a new [`Op`] with no arm here fails to compile (OP_SURFACE_CENSUS).
fn run_step(
  ir: &mut Ir<'_, '_>,
  step: &Step,
  model: &mut Model<'_>,
  cov: &mut Coverage,
  rng: &mut Rng,
  marks: &mut Vec<EventMark>,
  floor: usize,
) {
  let op = step.op;
  match op {
    // ── leaves ──
    Op::Next
    | Op::PeekOne
    | Op::Peek
    | Op::TryExpectHit
    | Op::TryExpectMiss
    | Op::TryExpectMap
    | Op::TryExpectAndThen
    | Op::SkipWhile
    | Op::SyncTo
    | Op::SyncThrough
    | Op::SyncBalanced
    | Op::IsEoi
    | Op::IsExhausted
    | Op::Seal
    | Op::Mark
    | Op::StartAt
    | Op::RollbackAcrossMark => run_leaf(ir, op, model, cov, marks, floor),

    // ── closure attempts ──
    Op::AttemptCommit => run_attempt(
      ir, &step.body, model, cov, rng, marks, /*keep*/ true, /*fallible*/ false,
    ),
    Op::AttemptDecline => run_attempt(ir, &step.body, model, cov, rng, marks, false, false),
    Op::TryAttemptOk => run_attempt(ir, &step.body, model, cov, rng, marks, true, true),
    Op::TryAttemptErr => run_attempt(ir, &step.body, model, cov, rng, marks, false, true),

    // ── the CST bracket ──
    Op::Node => {
      cov.mark(op);
      let entry_marks = marks.len();
      let (o0, _c0, _saved) = snapshot(ir, model);
      let m = &mut *model;
      let cv = &mut *cov;
      let rg = &mut *rng;
      let mk = &mut *marks;
      let mut body = |ir2: &mut Ir<'_, '_>| -> Result<(), FuzzError> {
        // The body's frame floor is the node bracket: marks minted inside must not be
        // spendable outside (their wraps would cross the node's boundary).
        run_seq(ir2, &step.body, m, cv, rg, mk, entry_marks);
        Ok(())
      };
      node(K_NODE_KIND, &mut body)
        .parse_input(ir)
        .expect("the node body is infallible");
      marks.truncate(entry_marks);
      assert!(
        *ir.cursor().as_inner() >= o0,
        "node: the bracket itself must never move the cursor backwards"
      );
    }

    // ── transaction guards ──
    Op::TxnCommit => {
      cov.mark(op);
      let mut txn = ir.begin();
      run_seq(&mut txn, &step.body, model, cov, rng, marks, floor);
      txn.commit();
    }
    Op::TxnRollback => {
      cov.mark(op);
      let (o0, c0, saved) = snapshot(ir, model);
      let entry_marks = marks.len();
      {
        let mut txn = ir.begin();
        run_seq(&mut txn, &step.body, model, cov, rng, marks, floor);
        txn.rollback();
      }
      marks.truncate(entry_marks);
      restore_and_assert(ir, model, o0, c0, saved, "transaction.rollback");
    }
    // The reconciling explicit rollback, exercised on the one input that separates it from
    // `TxnRollback`: a session point opened inside the guard and never settled. Its base is
    // pinned and younger than the guard's, which is exactly what the checked `rollback` refuses
    // to cross — so this arm would panic if the verb ever stopped reconciling. Everything else is
    // `TxnRollback`'s oracle unchanged: cursor and emission count back at the begin point, with
    // the point gone from the stack rather than left describing a lineage the rewind destroyed.
    Op::TxnRollbackAbandoningPoints => {
      cov.mark(op);
      let (o0, c0, saved) = snapshot(ir, model);
      let entry_marks = marks.len();
      let points0 = ir.points();
      {
        let mut txn = ir.begin();
        let _abandoned = txn.begin_point();
        run_seq(&mut txn, &step.body, model, cov, rng, marks, floor);
        txn.rollback_abandoning_points();
      }
      marks.truncate(entry_marks);
      assert_eq!(
        ir.points(),
        points0,
        "transaction.rollback_abandoning_points: the abandoned point must be off the stack, not \
         left describing a lineage the rewind destroyed"
      );
      restore_and_assert(
        ir,
        model,
        o0,
        c0,
        saved,
        "transaction.rollback_abandoning_points",
      );
    }
    Op::TxnDropRollback => {
      cov.mark(op);
      let (o0, c0, saved) = snapshot(ir, model);
      let entry_marks = marks.len();
      {
        let mut txn = ir.begin(); // Rollback policy: drop rolls back
        run_seq(&mut txn, &step.body, model, cov, rng, marks, floor);
      }
      marks.truncate(entry_marks);
      restore_and_assert(ir, model, o0, c0, saved, "transaction.drop(rollback)");
    }
    Op::TxnDropCommit => {
      cov.mark(op);
      let mut txn = ir.begin_with::<Commit>(); // drop keeps progress
      run_seq(&mut txn, &step.body, model, cov, rng, marks, floor);
      drop(txn);
    }

    // ── stacked transactions ──
    Op::StackedCommit => {
      cov.mark(op);
      let mut txn = ir.begin_stacked();
      let mut sps = Vec::new();
      run_stacked_seq(
        &mut txn, &step.body, model, cov, rng, &mut sps, marks, floor,
      );
      txn.commit();
    }
    Op::StackedRollback => {
      cov.mark(op);
      let (o0, c0, saved) = snapshot(ir, model);
      let entry_marks = marks.len();
      {
        let mut txn = ir.begin_stacked();
        let mut sps = Vec::new();
        run_stacked_seq(
          &mut txn, &step.body, model, cov, rng, &mut sps, marks, floor,
        );
        txn.rollback();
      }
      marks.truncate(entry_marks);
      restore_and_assert(ir, model, o0, c0, saved, "stacked.rollback");
    }
    Op::StackedDropCommit => {
      cov.mark(op);
      let mut txn = ir.begin_stacked_with::<Commit>();
      run_seq(&mut txn, &step.body, model, cov, rng, marks, floor);
      drop(txn);
    }

    // ── handled by run_stacked_seq / the session driver, never reached here ──
    Op::Savepoint | Op::RollbackToSavepoint | Op::ReleaseSavepoint => {
      unreachable!("savepoint ops are only emitted inside a stacked body (run_stacked_seq)")
    }
    Op::SessionCommit | Op::SessionRollback | Op::SessionAbandon => {
      unreachable!("session ops are driven by the session case kind, not the consume tree")
    }
  }
}

/// Snapshot of the pre-scope state a rollback must restore.
fn snapshot<'a>(ir: &mut Ir<'_, '_>, model: &Model<'a>) -> (usize, u64, Model<'a>) {
  (*ir.cursor().as_inner(), ir.emitter().count(), model.clone())
}

/// Assert a rollback restored the cursor and emission count exactly, and reset the model.
fn restore_and_assert<'a>(
  ir: &mut Ir<'_, '_>,
  model: &mut Model<'a>,
  o0: usize,
  c0: u64,
  saved: Model<'a>,
  what: &str,
) {
  let o = *ir.cursor().as_inner();
  let c = ir.emitter().count();
  assert_eq!(
    o, o0,
    "{what}: cursor not restored to the begin point (LIFO oracle)"
  );
  assert_eq!(
    c, c0,
    "{what}: emission count not restored to the begin point (no-trace oracle)"
  );
  *model = saved;
  let _ = model; // model.o now equals o0
}

/// Runs an `attempt` / `try_attempt` scope. `keep` chooses the succeed/decline path; `fallible`
/// chooses `attempt` vs `try_attempt`.
#[allow(clippy::too_many_arguments)]
fn run_attempt(
  ir: &mut Ir<'_, '_>,
  body: &[Step],
  model: &mut Model<'_>,
  cov: &mut Coverage,
  rng: &mut Rng,
  marks: &mut Vec<EventMark>,
  keep: bool,
  fallible: bool,
) {
  cov.mark(if keep {
    if fallible {
      Op::TryAttemptOk
    } else {
      Op::AttemptCommit
    }
  } else if fallible {
    Op::TryAttemptErr
  } else {
    Op::AttemptDecline
  });

  let (o0, c0, saved) = snapshot(ir, model);
  let entry_marks = marks.len();
  // Reborrow the through-state into the closure so the originals stay usable afterwards.
  let m = &mut *model;
  let cv = &mut *cov;
  let rg = &mut *rng;
  let mk = &mut *marks;
  let declined;
  if fallible {
    let r: Result<(), ()> = ir.try_attempt(move |ir2| {
      run_seq(ir2, body, m, cv, rg, mk, entry_marks);
      if keep { Ok(()) } else { Err(()) }
    });
    declined = r.is_err();
    assert_eq!(r.is_ok(), keep, "try_attempt returned the wrong branch");
  } else {
    let r: Option<()> = ir.attempt(move |ir2| {
      run_seq(ir2, body, m, cv, rg, mk, entry_marks);
      if keep { Some(()) } else { None }
    });
    declined = r.is_none();
    assert_eq!(r.is_some(), keep, "attempt returned the wrong branch");
  }

  if declined {
    // The rollback's truncation invalidates exactly the marks minted inside — the
    // append-only mark stack mirrors the sink's era staleness by suffix-truncation.
    marks.truncate(entry_marks);
    let o = *ir.cursor().as_inner();
    let c = ir.emitter().count();
    assert_eq!(
      o, o0,
      "declined attempt left a cursor trace (no-trace oracle)"
    );
    assert_eq!(
      c, c0,
      "declined attempt left an emission trace (no-trace oracle)"
    );
    *model = saved;
  }
  // On the kept path the body's committed progress stays in `model`.
}

/// A live savepoint: its id plus the cursor/emission/mark-stack facts it must restore to.
struct Sp<'txn> {
  id: crate::SavepointId<'txn>,
  o: usize,
  count: u64,
  marks: usize,
}

/// Runs a stacked-transaction body, handling the savepoint ops on the guard and every other op
/// through the guard's deref (`&mut **txn`).
#[allow(clippy::too_many_arguments)]
fn run_stacked_seq<'txn, 'inp>(
  txn: &mut StackedTransaction<'txn, 'inp, '_, ScriptLexer<'inp>, FuzzCtx<'inp>, (), Rollback>,
  steps: &[Step],
  model: &mut Model<'_>,
  cov: &mut Coverage,
  rng: &mut Rng,
  sps: &mut Vec<Sp<'txn>>,
  marks: &mut Vec<EventMark>,
  floor: usize,
) {
  for step in steps {
    match step.op {
      Op::Savepoint => {
        cov.mark(Op::Savepoint);
        let id = txn.savepoint();
        sps.push(Sp {
          id,
          o: *txn.cursor().as_inner(),
          count: txn.emitter().count(),
          marks: marks.len(),
        });
      }
      Op::RollbackToSavepoint => {
        if sps.is_empty() {
          continue;
        }
        cov.mark(Op::RollbackToSavepoint);
        let i = rng.below(sps.len());
        let target_o = sps[i].o;
        let target_c = sps[i].count;
        let target_marks = sps[i].marks;
        txn.rollback_to(sps[i].id);
        // rollback_to destroys the younger savepoints and keeps the target valid; the
        // marks minted after the savepoint die with its truncation.
        sps.truncate(i + 1);
        marks.truncate(target_marks);
        let o = *txn.cursor().as_inner();
        let c = txn.emitter().count();
        assert_eq!(
          o, target_o,
          "rollback_to: cursor not restored to the savepoint (LIFO oracle)"
        );
        assert_eq!(
          c, target_c,
          "rollback_to: emission not restored to the savepoint (no-trace oracle)"
        );
        model.o = target_o;
      }
      Op::ReleaseSavepoint => {
        if sps.is_empty() {
          continue;
        }
        cov.mark(Op::ReleaseSavepoint);
        let i = rng.below(sps.len());
        let o_before = *txn.cursor().as_inner();
        txn.release(sps[i].id);
        sps.truncate(i);
        let o = *txn.cursor().as_inner();
        assert_eq!(
          o, o_before,
          "release: kept progress but moved the cursor (release keeps position)"
        );
      }
      _ => run_step(txn, step, model, cov, rng, marks, floor),
    }
  }
}

// ── Leaf operations + their oracles ──────────────────────────────────────────────────────────────

/// A predicate over the fuzz token.
#[inline]
fn kind_is(target: FuzzKind) -> impl FnMut(Spanned<&FuzzTok, &SimpleSpan>) -> bool {
  move |t| t.data().kind() == target
}

/// A kind guaranteed to differ from `k` (for the miss paths).
#[inline]
fn other_kind(k: FuzzKind) -> FuzzKind {
  if k == FuzzKind::Word {
    FuzzKind::Open
  } else {
    FuzzKind::Word
  }
}

/// Executes one leaf operation and checks its oracle against the shadow model.
fn run_leaf(
  ir: &mut Ir<'_, '_>,
  op: Op,
  model: &mut Model<'_>,
  cov: &mut Coverage,
  marks: &mut Vec<EventMark>,
  floor: usize,
) {
  cov.mark(op);
  match op {
    Op::Next => {
      let got = ir.next().expect("complete + non-fatal emitter never errs");
      match model.kind_here() {
        Some(kind) => {
          let (span, tok) = got.expect("a token remained").into_components();
          assert_eq!(
            tok.kind(),
            kind,
            "next: token kind diverged from the straight-lex reference"
          );
          assert_eq!(
            span,
            SimpleSpan::new(model.o, model.o + 1),
            "next: span diverged from the reference"
          );
          model.o += 1;
        }
        None => assert!(got.is_none(), "next: yielded a token past end of input"),
      }
    }
    Op::PeekOne => {
      let _ = ir
        .peek_one()
        .expect("complete + non-fatal emitter never errs");
      assert_cursor(ir, model.o, "peek_one moved the committed cursor");
    }
    Op::Peek => {
      let _ = ir
        .peek::<U4>()
        .expect("complete + non-fatal emitter never errs");
      assert_cursor(ir, model.o, "peek moved the committed cursor");
    }
    Op::TryExpectHit => {
      let target = model.kind_here().unwrap_or(FuzzKind::Word);
      let got = ir.try_expect(kind_is(target)).expect("non-fatal");
      match model.kind_here() {
        Some(_) => {
          let (span, tok) = got
            .expect("try_expect(hit) consumed the matching token")
            .into_components();
          assert_eq!(tok.kind(), target, "try_expect(hit): wrong kind");
          assert_eq!(
            span,
            SimpleSpan::new(model.o, model.o + 1),
            "try_expect(hit): wrong span"
          );
          model.o += 1;
        }
        None => assert!(got.is_none(), "try_expect(hit): matched past end of input"),
      }
    }
    Op::TryExpectMiss => {
      let target = model.kind_here().map(other_kind).unwrap_or(FuzzKind::Word);
      let got = ir.try_expect(kind_is(target)).expect("non-fatal");
      assert!(
        got.is_none(),
        "try_expect(miss): consumed a non-matching token"
      );
      // The (non-matching) token stays cached; the cursor does not advance (error-free stream).
      assert_cursor(ir, model.o, "try_expect(miss) advanced the cursor");
    }
    Op::TryExpectMap => {
      let target = model.kind_here().unwrap_or(FuzzKind::Word);
      let got = ir
        .try_expect_map(|t| (t.data().kind() == target).then_some(()))
        .expect("non-fatal");
      match model.kind_here() {
        Some(_) => {
          let (_out, spanned) = got.expect("try_expect_map consumed the matching token");
          assert_eq!(
            spanned.into_components().0,
            SimpleSpan::new(model.o, model.o + 1),
            "try_expect_map: wrong span"
          );
          model.o += 1;
        }
        None => assert!(got.is_none(), "try_expect_map: matched past end of input"),
      }
    }
    Op::TryExpectAndThen => {
      let target = model.kind_here().unwrap_or(FuzzKind::Word);
      let got = ir
        .try_expect_and_then(|t| (t.data().kind() == target).then_some(Ok::<(), FuzzError>(())))
        .expect("non-fatal");
      match model.kind_here() {
        Some(_) => {
          let (_out, spanned) = got.expect("try_expect_and_then consumed the matching token");
          assert_eq!(
            spanned.into_components().0,
            SimpleSpan::new(model.o, model.o + 1),
            "try_expect_and_then: wrong span"
          );
          model.o += 1;
        }
        None => assert!(
          got.is_none(),
          "try_expect_and_then: matched past end of input"
        ),
      }
    }
    Op::SkipWhile => {
      // Skip a run of `Word` tokens; stop before the first non-`Word` token or at end of input.
      ir.skip_while(kind_is(FuzzKind::Word)).expect("non-fatal");
      let mut j = model.o;
      while j < model.len() && kind_of(model.src[j]) == FuzzKind::Word {
        j += 1;
      }
      assert_cursor(ir, j, "skip_while stopped at the wrong offset");
      model.o = j;
    }
    Op::SyncTo => {
      // `sync_to` returns a token that borrows `ir`; take the boolean before touching `ir` again.
      let found = ir
        .sync_to(kind_is(FuzzKind::Semi), || None)
        .expect("non-fatal")
        .is_some();
      // Stop *before* the first `Semi`; if none, commit at end of input.
      let stop = first_kind(model, FuzzKind::Semi).unwrap_or(model.len());
      assert_cursor(ir, stop, "sync_to stopped at the wrong offset");
      assert_eq!(
        found,
        stop < model.len(),
        "sync_to: match presence diverged"
      );
      model.o = stop;
    }
    Op::SyncThrough => {
      let (o0, c0, _) = snapshot(ir, model);
      let got = ir
        .sync_through(kind_is(FuzzKind::Semi), || None)
        .expect("non-fatal");
      match first_kind(model, FuzzKind::Semi) {
        Some(j) => {
          let (span, tok) = got
            .expect("sync_through consumed the sync token")
            .into_components();
          assert_eq!(tok.kind(), FuzzKind::Semi, "sync_through: wrong sync kind");
          assert_eq!(
            span,
            SimpleSpan::new(j, j + 1),
            "sync_through: wrong sync span"
          );
          assert_cursor(ir, j + 1, "sync_through: cursor not past the sync token");
          model.o = j + 1;
        }
        None => {
          // No-match run to end of input leaves no trace.
          assert!(got.is_none(), "sync_through: found a phantom sync token");
          assert_cursor(
            ir,
            o0,
            "sync_through(no-match) left a cursor trace (no-trace oracle)",
          );
          assert_eq!(
            ir.emitter().count(),
            c0,
            "sync_through(no-match) left an emission trace (no-trace oracle)"
          );
        }
      }
    }
    Op::SyncBalanced => {
      let (o0, c0, _) = snapshot(ir, model);
      let classify = |k: &FuzzKind| match k {
        FuzzKind::Open => crate::input::Balance::Open(()),
        FuzzKind::Close => crate::input::Balance::Close(()),
        _ => crate::input::Balance::Neutral,
      };
      let got = ir
        .sync_balanced(classify, kind_is(FuzzKind::Semi))
        .expect("non-fatal");
      match balanced_stop(model) {
        Some((stop, skipped)) => {
          let hole = got.expect("sync_balanced produced a hole at the sync point");
          assert_eq!(
            hole.skipped(),
            skipped,
            "sync_balanced: skipped-token count diverged"
          );
          assert_cursor(ir, stop, "sync_balanced: cursor not before the sync token");
          model.o = stop;
        }
        None => {
          assert!(
            got.is_none(),
            "sync_balanced: phantom hole on a no-match run"
          );
          assert_cursor(
            ir,
            o0,
            "sync_balanced(no-match) left a cursor trace (no-trace oracle)",
          );
          assert_eq!(
            ir.emitter().count(),
            c0,
            "sync_balanced(no-match) left an emission trace (no-trace oracle)"
          );
        }
      }
    }
    Op::IsEoi => {
      let eoi = ir.is_eoi();
      // Sound one-directional check: at the modelled end of input, `is_eoi` must be true. (Below
      // the end it may still be true if an earlier peek lexed to the buffer end, so the converse is
      // not asserted.)
      if model.o >= model.len() {
        assert!(eoi, "is_eoi: false at end of input");
      }
    }
    Op::IsExhausted => {
      // Sound one-directional check: exhausted implies the model is at the end of input. The
      // converse does not hold — a drain that ends on lexer-skipped bytes leaves the committed
      // frontier short of the buffer end, so the gate stays false with the stream already empty.
      // That direction is the safe one: the gate never turns true while a token is still
      // consumable.
      if ir.is_exhausted() {
        assert!(
          model.o >= model.len(),
          "is_exhausted: true with input left to consume"
        );
      }
    }
    Op::Seal => {
      // Complete mode: the input is final by definition, and a handle has no way to say otherwise —
      // `is_final` is read-only, and the seal lives on the owning `Input`, which this handle borrows.
      assert!(
        ir.is_final(),
        "a Complete input must report is_final == true"
      );
      assert_cursor(ir, model.o, "is_final moved the cursor");
    }
    Op::Mark => {
      // Mint a retro-wrap anchor. Bounded so a mark-heavy script cannot grow the live
      // stack without spends; the anchor observes nothing about the input.
      if marks.len() < 8 {
        marks.push(CstEmitter::<ScriptLexer<'_>>::cst_mark(ir.emitter()));
      }
      assert_cursor(ir, model.o, "cst_mark moved the committed cursor");
    }
    Op::StartAt => {
      // Spend the newest frame-local live mark as a retro-wrap. Marks below the frame
      // floor stay unspent: a wrap crossing a `node()` bracket's boundary is the
      // ImproperWrap shape materialization refuses by design.
      if marks.len() > floor {
        let mark = marks.pop().expect("guarded by the length check");
        let emitter = ir.emitter();
        CstEmitter::<ScriptLexer<'_>>::cst_start_at(emitter, mark, K_WRAP_KIND);
        CstEmitter::<ScriptLexer<'_>>::cst_finish(emitter, K_WRAP_KIND);
      }
      assert_cursor(ir, model.o, "a retro-wrap moved the committed cursor");
    }
    Op::RollbackAcrossMark => {
      // The truncated-tombstone shape: a declined attempt minted a mark inside, so the
      // rollback truncates the tombstone out of the live timeline. The mark is dropped
      // with the branch (never pushed to the live stack), exactly as the era ledger
      // makes it unspendable on a recording sink.
      let o0 = *ir.cursor().as_inner();
      let c0 = ir.emitter().count();
      let declined: Option<()> = ir.attempt(|ir2| {
        let _stale_to_be = CstEmitter::<ScriptLexer<'_>>::cst_mark(ir2.emitter());
        let _ = ir2.next();
        None
      });
      assert!(
        declined.is_none(),
        "the rollback-across-mark attempt declines"
      );
      assert_cursor(
        ir,
        o0,
        "rollback across a mark left a cursor trace (no-trace oracle)",
      );
      assert_eq!(
        ir.emitter().count(),
        c0,
        "rollback across a mark left an emission trace (no-trace oracle)"
      );
    }
    other => unreachable!("run_leaf received a non-leaf op: {}", other.label()),
  }
}

/// The offset of the first token of `kind` at or after the cursor.
fn first_kind(model: &Model<'_>, kind: FuzzKind) -> Option<usize> {
  (model.o..model.len()).find(|&i| kind_of(model.src[i]) == kind)
}

/// Mirrors `sync_balanced`'s decision loop: scan from the cursor, sync on a `Semi` at depth zero;
/// returns `(stop_offset, skipped_token_count)` on a match, `None` on a no-match run to EOI.
fn balanced_stop(model: &Model<'_>) -> Option<(usize, usize)> {
  let mut depth = 0usize;
  let mut skipped = 0usize;
  let mut j = model.o;
  while j < model.len() {
    let k = kind_of(model.src[j]);
    if depth == 0 && k == FuzzKind::Semi {
      return Some((j, skipped));
    }
    match k {
      FuzzKind::Open => depth += 1,
      FuzzKind::Close => depth = depth.saturating_sub(1),
      _ => {}
    }
    skipped += 1;
    j += 1;
  }
  None
}

/// Asserts the committed cursor equals `expected`.
fn assert_cursor(ir: &mut Ir<'_, '_>, expected: usize, what: &str) {
  assert_eq!(*ir.cursor().as_inner(), expected, "{what}");
}
