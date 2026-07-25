//! The **session-point** driver: fuzzes [`InputRef`] session points — the non-lexical speculation
//! the transaction guards cannot express.
//!
//! A session point lives on the input handle itself, so — unlike a borrowing guard — it can be
//! opened, *parsed through*, and settled in unrelated steps. This driver exercises exactly that:
//! the generated sequence is a well-formed last-in, first-out stream of `begin_point` /
//! `commit_point` / `rollback_point` (never settling with no live point) interleaved with real
//! **token consumption**, diagnostic emission, and state re-keying, with a shadow stack asserting
//! the depth, the cursor, the emission count, and the state tag after every step.
//!
//! # Oracles
//!
//! - **LIFO depth** — `points()` always equals the shadow stack depth.
//! - **Commit keeps** — after `commit_point`, the current cursor, emission count, and state tag
//!   survive.
//! - **Rollback restores** — after `rollback_point`, the cursor, emission count, and state tag all
//!   return to the values captured when that point opened.
//! - **Abandon releases, and keeps** — a third of the cases end by **dropping the handle with points
//!   still open**, never settling them. The handle's `Drop` must release every remaining point's pin
//!   (the input's pin set is empty again — the memos live on the `Input`, which outlives the handle,
//!   so nothing else would), keep all the progress made through them (no implicit rollback), and
//!   leave a *second* handle over the same input free to speculate: a fresh point opened and rolled
//!   back over the abandoned region must not trip a pin left behind by a point nobody holds.

use std::vec::Vec;

use super::{
  fixtures::{CountEmitter, FuzzCtx, FuzzError, ScriptLexer, ScriptState, cache, initial_state},
  ops::{Coverage, Op},
  rng::Rng,
};
use crate::{
  InputRef,
  emitter::Emitter,
  input::SessionPointId,
  span::{SimpleSpan, Spanned},
};

/// The session handle this driver steps: the input reference the session points live on.
type Ir<'a, 'inp> = InputRef<'a, 'inp, ScriptLexer<'a>, FuzzCtx<'a>>;

/// A session sub-operation. Only `Commit`/`Rollback` map to census ops; `Begin`/`Consume`/`Emit`/
/// `Rekey` are the checkpoint-tracked "work" a point speculates over.
#[derive(Debug, Clone, Copy)]
enum SOp {
  Begin,
  Consume,
  Emit,
  Rekey(u64),
  Commit,
  Rollback,
}

/// Generates a well-formed session sub-op sequence: it never commits/rolls back with no live point.
///
/// `abandon` picks the ending. Normally the tail settles every open point, because a session ends
/// explicitly. When `abandon` is set the tail is left **open** instead — at least one point live —
/// so the caller can drop the handle on top of it and check the abandon oracles.
fn gen_session(rng: &mut Rng, abandon: bool) -> Vec<SOp> {
  let n = rng.below(12) + 4;
  let mut depth = 0usize;
  let mut out = Vec::with_capacity(n + 2);
  for _ in 0..n {
    if depth == 0 {
      match rng.below(4) {
        0 | 1 => {
          out.push(SOp::Begin);
          depth += 1;
        }
        2 => out.push(SOp::Consume),
        _ => out.push(SOp::Emit),
      }
    } else {
      match rng.below(7) {
        0 => {
          out.push(SOp::Begin);
          depth += 1;
        }
        1 => out.push(SOp::Emit),
        2 => out.push(SOp::Rekey(rng.next_u64())),
        3 | 4 => out.push(SOp::Consume),
        5 => {
          out.push(SOp::Commit);
          depth -= 1;
        }
        _ => {
          out.push(SOp::Rollback);
          depth -= 1;
        }
      }
    }
  }
  if abandon {
    // Leave the tail open — the handle is dropped on top of these. Guarantee at least one live
    // point, so an abandon case always really abandons something.
    if depth == 0 {
      out.push(SOp::Begin);
    }
    return out;
  }
  while depth > 0 {
    out.push(if rng.chance(1, 2) {
      SOp::Commit
    } else {
      SOp::Rollback
    });
    depth -= 1;
  }
  out
}

/// Records a diagnostic through the input's emitter (bumps the emission count).
fn emit<'inp>(ir: &mut Ir<'inp, '_>) {
  <CountEmitter as Emitter<'inp, ScriptLexer<'inp>>>::emit_error(
    ir.emitter(),
    Spanned::new(SimpleSpan::new(0, 1), FuzzError::Diagnostic),
  )
  .expect("CountEmitter is non-fatal");
}

/// The live emission count observed through the input's emitter.
fn count(ir: &mut Ir<'_, '_>) -> u64 {
  ir.emitter().count()
}

/// The live cursor offset — the fact only real token consumption moves.
fn at(ir: &Ir<'_, '_>) -> usize {
  *ir.cursor().as_inner()
}

/// Runs one session case over `src`, checking the session-point oracles.
pub(crate) fn run(src: &[u8], seq_seed: u64, cov: &mut Coverage) {
  let cache = cache();
  let mut emitter = CountEmitter::new();
  let state = initial_state(src);
  let mut input = crate::input::Input::<'_, ScriptLexer<'_>, FuzzCtx<'_>, ()>::with_state_and_cache(
    src, state, cache,
  );
  let mut ir = input.as_ref(&mut emitter);

  // Optionally pre-consume a few tokens so the session opens at a non-trivial position.
  let mut rng = Rng::new(seq_seed);
  let pre = rng.below(4);
  for _ in 0..pre {
    if ir.next().expect("complete + non-fatal").is_none() {
      break;
    }
  }

  // Whether this case ends by ABANDONING its open points. Drawn from a derived stream rather than
  // `rng`, so the script generation below stays bit-for-bit what it was before this shape existed
  // (and the corpus keeps covering the settle verbs exactly as it did).
  let abandon = Rng::new(seq_seed ^ 0xABA4_D04E_5EED_5EED).chance(1, 3);
  let seq = gen_session(&mut rng, abandon);

  // Each live point remembers its id plus the emission count, state tag, and cursor captured when
  // it opened. Carrying the id is not bookkeeping for the oracle's sake: it is what a real driver
  // must do, since the settle verbs name the point rather than a position.
  let mut stack: Vec<(SessionPointId<'_>, u64, u64, usize)> = Vec::new();
  let mut cur_tag: u64 = ir.state().tag;

  for sop in seq {
    match sop {
      SOp::Begin => {
        let snap = (count(&mut ir), cur_tag, at(&ir));
        let point = ir.begin_point();
        stack.push((point, snap.0, snap.1, snap.2));
        assert_eq!(
          ir.points(),
          stack.len(),
          "session: points() diverged from the shadow depth"
        );
      }
      // Real parsing *through* an open point — the thing a borrowing guard cannot be held across.
      SOp::Consume => {
        let _ = ir.next().expect("complete + non-fatal");
      }
      SOp::Emit => emit(&mut ir),
      SOp::Rekey(t) => {
        *ir.state_mut() = ScriptState::with_tag(t);
        cur_tag = t;
      }
      SOp::Commit => {
        cov.mark(Op::SessionCommit);
        let before = at(&ir);
        let (point, ..) = stack.pop().expect("generator keeps the stream well-formed");
        ir.commit_point(point);
        assert_eq!(
          ir.points(),
          stack.len(),
          "session: commit_point left the wrong depth"
        );
        // Commit keeps: the progress made through the point stays exactly where it was.
        assert_eq!(
          at(&ir),
          before,
          "session: commit_point moved the cursor instead of keeping the progress"
        );
      }
      SOp::Rollback => {
        cov.mark(Op::SessionRollback);
        let (point, saved_count, saved_tag, saved_at) =
          stack.pop().expect("generator keeps the stream well-formed");
        ir.rollback_point(point);
        cur_tag = saved_tag;
        assert_eq!(
          ir.points(),
          stack.len(),
          "session: rollback_point left the wrong depth"
        );
        assert_eq!(
          at(&ir),
          saved_at,
          "session: rollback did not restore the cursor"
        );
        assert_eq!(
          count(&mut ir),
          saved_count,
          "session: rollback did not restore the emission count"
        );
        assert_eq!(
          ir.state().tag,
          saved_tag,
          "session: rollback did not restore the state tag"
        );
      }
    }
  }
  if !abandon {
    assert_eq!(
      ir.points(),
      0,
      "session: points left open at the end of the case"
    );
    return;
  }

  // ── The abandon shape: drop the handle with points still open ──────────────────────────────
  cov.mark(Op::SessionAbandon);
  let open = ir.points();
  assert!(open > 0, "session: an abandon case must leave a point open");
  // The committed facts at the moment of the drop. Everything the abandoned points speculated over
  // is part of them: abandoning keeps progress, it does not roll it back.
  let at_drop = at(&ir);
  let count_at_drop = count(&mut ir);
  let tag_at_drop = ir.state().tag;

  drop(ir);

  // Oracle: the handle's `Drop` released every open point. The pin set and the live-checkpoint
  // stack live on the `Input`, which outlives the handle — nothing else can release them, and a pin
  // for a point nobody holds would falsify the set's own invariant and grow for the input's life.
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "session: dropping the handle left {open} abandoned point(s) pinned on the input"
  );

  // Oracle: no implicit rollback. A second handle over the same input sees exactly the progress the
  // abandoned session made — cursor, emission log, and lexer state alike.
  let mut ir = input.as_ref(&mut emitter);
  assert_eq!(
    at(&ir),
    at_drop,
    "session: abandoning rolled the cursor back instead of keeping the progress"
  );
  assert_eq!(
    count(&mut ir),
    count_at_drop,
    "session: abandoning rolled back diagnostics instead of keeping them"
  );
  assert_eq!(
    ir.state().tag,
    tag_at_drop,
    "session: abandoning rolled the lexer state back instead of keeping it"
  );
  assert_eq!(
    ir.points(),
    0,
    "session: a fresh handle starts with an empty point stack"
  );

  // Oracle: the second handle speculates freely over the region the abandoned points covered — a
  // rewind here must not trip a pin left behind by a point nobody holds.
  let point = ir.begin_point();
  let _ = ir.next().expect("complete + non-fatal");
  ir.rollback_point(point);
  assert_eq!(
    at(&ir),
    at_drop,
    "session: a fresh point on the second handle did not roll back to its own base"
  );
  assert_eq!(ir.points(), 0, "session: the fresh point settled");
}
