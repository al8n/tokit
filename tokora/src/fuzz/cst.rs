//! The **CST recording twin**: the fuzz alphabet's `node`/`mark`/`start_at`/
//! `rollback-across-mark` ops driven over a real `Sink`, with the two oracles the
//! event channel owes:
//!
//! - **Backtrack equivalence** — the full script (declines, rollbacks-across-marks and
//!   all) and its *pruned* twin (every declined branch dropped) materialize
//!   **byte-identical green trees**: a declined branch leaves no event trace, ever.
//! - **The append-only depth/count model** — the pruned run has no rollbacks, so every
//!   consume it makes is a committed settle; the tree's non-gap token count must equal
//!   that straight count exactly (a lost or doubled auto-emission cannot hide behind gap
//!   tiling), `finish` is the balance oracle (depth ends at zero — the only refusals a
//!   combinator-driven buffer may earn are the two coverage laws: the token-channel wall on
//!   the tokenless-structure signature, or an uncovered gap when the script left source bytes
//!   unconsumed; the twins must agree on either), and the sink's mark-stack must end empty
//!   (the release no-growth law).
//!
//! The sinkless halves of these ops run in every build through the consume tree (over
//! `CountEmitter`'s defaulted no-op event channel); this driver is the `rowan`-gated
//! recording twin.

use std::{string::String, vec::Vec};

use crate::{
  InputRef, ParseInput, Token,
  cache::DefaultCache,
  cst::{FinishError, event::EventMark},
  emitter::CstEmitter,
  input::Input,
  parser::node,
};

use super::{
  fixtures::{CountEmitter, FuzzError, FuzzKind, FuzzTok, ScriptLexer, initial_state},
  ops::{Coverage, Op},
  rng::Rng,
};

/// The dialect fixture: node/wrap kinds and the token-image mapper.
const K_ROOT: u16 = 1;
const K_NODE: u16 = 2;
const K_WRAP: u16 = 3;
const K_ERR: u16 = 90;
const K_GAP: u16 = 91;

/// The fixture's whole kind space: the structural kinds above, plus the token images
/// `map_tok` produces. A real range predicate, not `accept_all` — the fuzzer's trees have to
/// pass the same wall a dialect's do.
fn in_fixture_kind_space(kind: u16) -> bool {
  matches!(kind, K_ROOT..=K_WRAP | 20..=23 | K_ERR | K_GAP)
}

/// The fixture dialect's CST profile.
fn profile() -> crate::cst::CstProfile<FuzzTok> {
  crate::cst::CstProfile::new(
    map_tok,
    crate::cst::KindValidator::new(in_fixture_kind_space),
    K_ERR,
    K_GAP,
  )
}

fn map_tok(t: &FuzzTok) -> u16 {
  match t.kind() {
    FuzzKind::Open => 20,
    FuzzKind::Close => 21,
    FuzzKind::Semi => 22,
    FuzzKind::Word => 23,
  }
}

/// The generous live-mark cap; generation keeps well under it, the executor guard is a
/// never-hit safety.
const MAX_LIVE_MARKS: usize = 16;

/// The maximum nesting depth of generated scopes.
const MAX_DEPTH: usize = 3;

type Sink<'a> = crate::cst::Sink<'a, ScriptLexer<'a>, CountEmitter>;
type Ctx<'a> = (Sink<'a>, DefaultCache<'a, ScriptLexer<'a>>);
type Ir<'inp, 'c> = InputRef<'inp, 'c, ScriptLexer<'inp>, Ctx<'inp>, ()>;

/// One step of a CST script. Scripts are trees (scopes carry bodies), like the consume
/// tree, but over the event-channel alphabet.
#[derive(Debug, Clone)]
enum CStep {
  /// `InputRef::next` — one committed settle.
  Next,
  /// `InputRef::try_expect(|_| true)` — an accepting settle off either origin.
  TryHit,
  /// `InputRef::skip_while(Word)` — a run of skip settles (trivia-shaped consumption).
  SkipWhile,
  /// Mint a retro-wrap anchor into the frame's live set.
  Mark,
  /// Spend the newest frame-local anchor as a retro-wrap.
  StartAt,
  /// A declined attempt that minted a mark inside — the truncated-tombstone shape.
  RollbackAcrossMark,
  /// `node(kind, body)` — the CST bracket.
  Node(Vec<CStep>),
  /// `attempt(body → Some)` — kept speculation.
  AttemptCommit(Vec<CStep>),
  /// `attempt(body → None)` — declined speculation; the pruned twin drops it whole.
  AttemptDecline(Vec<CStep>),
}

/// Generates one frame. `live` counts this frame's unspent marks so a generated
/// `StartAt` always has a frame-local anchor to spend.
fn gen_seq(rng: &mut Rng, depth: usize) -> Vec<CStep> {
  let n = rng.below(5) + if depth == 0 { 3 } else { 1 };
  let mut steps = Vec::with_capacity(n);
  let mut live = 0usize;
  for _ in 0..n {
    if depth < MAX_DEPTH && rng.chance(35, 100) {
      let body = gen_seq(rng, depth + 1);
      steps.push(match rng.below(3) {
        0 => CStep::Node(body),
        1 => CStep::AttemptCommit(body),
        _ => CStep::AttemptDecline(body),
      });
      continue;
    }
    let pick = rng.below(10);
    steps.push(match pick {
      0 | 1 => CStep::Next,
      2 => CStep::TryHit,
      3 => CStep::SkipWhile,
      4 | 5 => {
        if live < 3 {
          live += 1;
          CStep::Mark
        } else {
          CStep::Next
        }
      }
      6 | 7 => {
        if live > 0 {
          live -= 1;
          CStep::StartAt
        } else {
          live += 1;
          CStep::Mark
        }
      }
      _ => CStep::RollbackAcrossMark,
    });
  }
  steps
}

/// The pruned twin: every declined branch dropped whole. The full run's declines rewind
/// to their entry state, so the two scripts share one committed timeline by law — the
/// oracle is that their trees agree byte for byte.
fn prune(steps: &[CStep]) -> Vec<CStep> {
  steps
    .iter()
    .filter_map(|step| match step {
      CStep::AttemptDecline(_) | CStep::RollbackAcrossMark => None,
      CStep::Node(body) => Some(CStep::Node(prune(body))),
      CStep::AttemptCommit(body) => Some(CStep::AttemptCommit(prune(body))),
      other => Some(other.clone()),
    })
    .collect()
}

/// Executes one frame. `floor` is the frame boundary of the live-mark stack (see the
/// consume tree's discipline: wraps never cross a `node()` bracket); `consumed` counts
/// committed settles — kept speculation included, declined speculation excluded.
fn exec(
  ir: &mut Ir<'_, '_>,
  steps: &[CStep],
  marks: &mut Vec<EventMark>,
  floor: usize,
  consumed: &mut usize,
  cov: &mut Coverage,
) {
  for step in steps {
    match step {
      CStep::Next => {
        cov.mark(Op::Next);
        if ir.next().expect("non-fatal emitter").is_some() {
          *consumed += 1;
        }
      }
      CStep::TryHit => {
        cov.mark(Op::TryExpectHit);
        if ir.try_expect(|_| true).expect("non-fatal").is_some() {
          *consumed += 1;
        }
      }
      CStep::SkipWhile => {
        cov.mark(Op::SkipWhile);
        let before = *ir.cursor().as_inner();
        ir.skip_while(|t| t.data().kind() == FuzzKind::Word)
          .expect("non-fatal");
        // One byte per token: the cursor delta is the number of skip settles.
        *consumed += *ir.cursor().as_inner() - before;
      }
      CStep::Mark => {
        cov.mark(Op::Mark);
        if marks.len() < MAX_LIVE_MARKS {
          marks.push(CstEmitter::<ScriptLexer<'_>>::cst_mark(ir.emitter()));
        }
      }
      CStep::StartAt => {
        cov.mark(Op::StartAt);
        if marks.len() > floor {
          let mark = marks.pop().expect("guarded by the length check");
          let emitter = ir.emitter();
          CstEmitter::<ScriptLexer<'_>>::cst_start_at(emitter, mark, K_WRAP);
          CstEmitter::<ScriptLexer<'_>>::cst_finish(emitter, K_WRAP);
        }
      }
      CStep::RollbackAcrossMark => {
        cov.mark(Op::RollbackAcrossMark);
        let declined: Option<()> = ir.attempt(|ir2| {
          let _stale_to_be = CstEmitter::<ScriptLexer<'_>>::cst_mark(ir2.emitter());
          let _ = ir2.next();
          None
        });
        assert!(declined.is_none());
      }
      CStep::Node(body) => {
        cov.mark(Op::Node);
        let entry_marks = marks.len();
        let mk = &mut *marks;
        let cn = &mut *consumed;
        let cv = &mut *cov;
        let mut body_parser = |ir2: &mut Ir<'_, '_>| -> Result<(), FuzzError> {
          exec(ir2, body, mk, entry_marks, cn, cv);
          Ok(())
        };
        node(K_NODE, &mut body_parser)
          .parse_input(ir)
          .expect("the node body is infallible");
        marks.truncate(entry_marks);
      }
      CStep::AttemptCommit(body) => {
        cov.mark(Op::AttemptCommit);
        let entry_marks = marks.len();
        let mk = &mut *marks;
        let cn = &mut *consumed;
        let cv = &mut *cov;
        let kept: Option<()> = ir.attempt(|ir2| {
          exec(ir2, body, mk, entry_marks, cn, cv);
          Some(())
        });
        assert!(kept.is_some());
      }
      CStep::AttemptDecline(body) => {
        cov.mark(Op::AttemptDecline);
        let entry_marks = marks.len();
        let consumed_before = *consumed;
        {
          let mk = &mut *marks;
          let cn = &mut *consumed;
          let cv = &mut *cov;
          let declined: Option<()> = ir.attempt(|ir2| {
            exec(ir2, body, mk, entry_marks, cn, cv);
            None
          });
          assert!(declined.is_none());
        }
        // The decline unwinds the branch: its consumption never committed and its marks
        // died with the truncation.
        *consumed = consumed_before;
        marks.truncate(entry_marks);
      }
    }
  }
}

/// The u16-transparent language for reading fuzz trees back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum RawLang {}

impl rowan::Language for RawLang {
  type Kind = u16;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> u16 {
    raw.0
  }

  fn kind_to_raw(kind: u16) -> rowan::SyntaxKind {
    rowan::SyntaxKind(kind)
  }
}

/// Drives one script over a fresh recording sink; returns the materialization verdict and
/// the committed-settle count. The verdict stays a `Result` on purpose: a surviving timeline
/// that builds structure without one committed token (`StructureWithoutTokens`), or that
/// leaves source bytes no token and no lexer error covers (`UncoveredGap`, the unconsumed
/// tail), is *refused* by `finish` — and the twins must agree on that refusal exactly as they
/// agree on trees.
fn drive(
  src: &str,
  script: &[CStep],
  cov: &mut Coverage,
) -> (Result<rowan::GreenNode, FinishError>, usize) {
  let mut sink: Sink<'_> = crate::cst::Sink::new(src.as_bytes(), CountEmitter::new(), profile());
  let cache = DefaultCache::<'_, ScriptLexer<'_>>::default();
  let state = initial_state(src.as_bytes());
  let mut input =
    Input::<'_, ScriptLexer<'_>, Ctx<'_>, ()>::with_state_and_cache(src.as_bytes(), state, cache);
  let mut ir = input.as_ref(&mut sink);

  let mut marks = Vec::new();
  let mut consumed = 0usize;
  exec(&mut ir, script, &mut marks, 0, &mut consumed, cov);

  drop(ir);
  drop(input);
  assert_eq!(
    sink.rows_len(),
    0,
    "release no-growth: every capture must be settled once the script ends"
  );
  let (green, _emitter) = sink.finish(K_ROOT);
  (green, consumed)
}

/// Counts the tree's non-gap tokens: exactly the committed settles (gap tiles cover only
/// bytes no committed token claimed, so a lost auto-emission cannot hide behind them).
fn non_gap_tokens(green: &rowan::GreenNode) -> usize {
  let root = rowan::SyntaxNode::<RawLang>::new_root(green.clone());
  root
    .descendants_with_tokens()
    .filter(|el| el.as_token().is_some_and(|t| t.kind() != K_GAP))
    .count()
}

/// Runs one CST case: generate a script, drive the full and pruned twins, and hold the
/// equivalence + count + no-growth oracles.
pub(crate) fn run(src: &[u8], seed: u64, cov: &mut Coverage) {
  let src = match core::str::from_utf8(src) {
    Ok(s) => String::from(s),
    Err(_) => return, // the error-free palette is ASCII; non-UTF-8 cannot arise
  };
  let mut rng = Rng::new(seed ^ 0xC57_0C0DE);
  let script = gen_seq(&mut rng, 0);
  let pruned = prune(&script);

  let (full_tree, full_consumed) = drive(&src, &script, cov);
  let (pruned_tree, pruned_consumed) = drive(&src, &pruned, cov);

  assert_eq!(
    full_consumed, pruned_consumed,
    "committed consumption must match the pruned twin (declines leave no trace)"
  );
  match (full_tree, pruned_tree) {
    (Ok(full_tree), Ok(pruned_tree)) => {
      assert_eq!(
        full_tree, pruned_tree,
        "backtrack equivalence: the full and pruned scripts share one committed timeline, \
         so their green trees must be byte-identical"
      );
      assert_eq!(
        non_gap_tokens(&pruned_tree),
        pruned_consumed,
        "every committed settle appears in the tree exactly once (the auto-emission \
         exactly-once law; gap tiles cover only unconsumed bytes)"
      );
    }
    (Err(full_err), Err(pruned_err)) => {
      // The twins share one committed timeline, so they refuse identically (same variant,
      // same span). Balance is structural, so the only refusals a combinator-driven buffer
      // may earn are the two coverage laws: the token-channel wall (zero committed tokens
      // under structure) or an uncovered gap (the script left source bytes unconsumed, and
      // the error-free palette records no lexer error to explain them).
      assert_eq!(
        full_err, pruned_err,
        "the twins share one committed timeline and must refuse identically"
      );
      match full_err {
        FinishError::StructureWithoutTokens => assert_eq!(
          full_consumed, 0,
          "the wall fires only when no committed settle survived"
        ),
        FinishError::UncoveredGap { .. } => {}
        other => panic!(
          "only the token-channel wall or an uncovered gap may refuse a combinator-driven \
           buffer (balance is structural): {other:?}"
        ),
      }
      assert!(
        !src.is_empty(),
        "no refusal ever fires over an empty source (nothing to consume or leave uncovered)"
      );
    }
    (full, pruned) => panic!(
      "the twins share one committed timeline and must agree on the materialization \
       verdict: full {full:?} vs pruned {pruned:?}"
    ),
  }
}
