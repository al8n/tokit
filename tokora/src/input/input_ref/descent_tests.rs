//! What actually holds a recursion level, measured rather than reasoned.
//!
//! Every cell here drives a hand-written recursive combinator — the case the budget exists for and
//! the case neither pratt engine covers — at a limitation of [`LIMIT`] through [`CALLS`] nested
//! calls, and reads the outcome plus the depth cell. The two shapes that hold the level stop at
//! `LIMIT + 1`; the five that release it early run all [`CALLS`] levels and report a depth of 0
//! or 1.
//!
//! **The five bypass cells are deliberate characterisation tests.** They pin a *hole*, not a
//! guarantee: the table in [`Descent`](super::Descent)'s documentation claims exactly these
//! results, and a change that closes one of them must change that table in the same commit. They
//! go red the moment the measured behaviour stops matching the prose — in either direction.
//!
//! What they do **not** measure is the native stack, because a stack overflow aborts the process
//! rather than failing a test. That half was measured out-of-band, one process per depth, and the
//! abort depths are recorded in the same documentation table.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{
  Token,
  cache::DefaultCache,
  emitter::Fatal,
  error::{RecursionLimitReached, token::UnexpectedToken},
  input::{Input, InputContext, InputRef},
  lexer::LogosLexer,
  state::recursion_tracker::RecursionLimiter,
};

/// The budget every cell runs against. Small enough that a bound frame trips almost immediately,
/// so an `Ok` can only mean the level was released early.
const LIMIT: usize = 8;

/// How deep each cell recurses. Far past `LIMIT`, and far short of the depth at which the native
/// stack would abort the harness — an aborting cell is not a test result.
const CALLS: usize = 200;

/// The depth a trip against `LIMIT` reports: the level is raised first and the check runs second,
/// so the failing frame is the ninth.
const TRIP_DEPTH: usize = LIMIT + 1;

// ── the fixture ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
enum Tok {
  #[regex(r"[0-9]+")]
  Num,
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokKind {
  Num,
}

impl core::fmt::Display for TokKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl Token<'_> for Tok {
  type Kind = TokKind;
  type Error = ();

  fn kind(&self) -> TokKind {
    TokKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// A grammar error that **keeps** the trip's payload, so a cell can assert the numbers rather than
/// just the discriminant.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProbeErr {
  Lex,
  Trip {
    at: usize,
    depth: usize,
    limitation: usize,
  },
}

impl From<()> for ProbeErr {
  fn from((): ()) -> Self {
    ProbeErr::Lex
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for ProbeErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ProbeErr::Lex
  }
}

impl<Lang: ?Sized> From<RecursionLimitReached<usize, Lang>> for ProbeErr {
  fn from(e: RecursionLimitReached<usize, Lang>) -> Self {
    ProbeErr::Trip {
      at: e.offset(),
      depth: e.depth(),
      limitation: e.limitation(),
    }
  }
}

/// A second error type, unrelated to the emitter's, for the cell that pins `descending`'s error
/// threading: the trip is *returned*, so it is built as whatever the frame's own `Result` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameErr(usize);

impl<Lang: ?Sized> From<RecursionLimitReached<usize, Lang>> for FrameErr {
  fn from(e: RecursionLimitReached<usize, Lang>) -> Self {
    FrameErr(e.depth())
  }
}

type ProbeLexer<'a> = LogosLexer<'a, Tok>;
type ProbeCtx<'a> = (Fatal<ProbeErr>, DefaultCache<'a, ProbeLexer<'a>>);
type Frame<'inp, 'c> = InputRef<'inp, 'c, ProbeLexer<'inp>, ProbeCtx<'inp>, ()>;

/// Builds an input whose recursion budget is `LIMIT` and hands the handle to `f`.
fn with_budget<O>(f: impl FnOnce(&mut Frame<'_, '_>) -> O) -> O {
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_context(
    "1",
    (),
    InputContext::new(
      Fatal::<ProbeErr>::new(),
      DefaultCache::<'_, ProbeLexer<'_>>::default(),
    )
    .with_recursion_limiter(RecursionLimiter::with_limitation(LIMIT)),
  );
  let mut inp = input.as_ref();
  f(&mut inp)
}

// ── the two shapes that hold the level ────────────────────────────────────────

/// The scoped form. The level is this call's, for exactly the closure's extent.
fn scoped(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, ProbeErr> {
  inp.descending(|inp| match remaining {
    0 => Ok(inp.recursion().depth()),
    n => scoped(inp, n - 1),
  })
}

/// The guard form, written correctly: bound, then shadowed, so the borrow checker holds the guard
/// to the last use of `inp`.
fn bound_guard(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, ProbeErr> {
  let mut frame = inp.descend()?;
  let inp = &mut *frame;
  match remaining {
    0 => Ok(inp.recursion().depth()),
    n => bound_guard(inp, n - 1),
  }
}

// ── the five shapes that do not ───────────────────────────────────────────────
//
// Each of these is a defect. They are compiled here on purpose, and the `expect` below is the
// only place in the crate that silences the lint that catches one of them — deliberately narrow,
// and it would itself warn (`unfulfilled_lint_expectations`) if the attribute stopped firing.

/// The bare expression statement — the **one** shape `#[must_use]` catches.
fn bare_statement(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, ProbeErr> {
  #[expect(
    unused_must_use,
    reason = "the discarded guard IS the subject of this cell; tests/ui/descent_dropped_early.rs \
              is where the lint's own firing is pinned"
  )]
  inp.descend()?;
  match remaining {
    0 => Ok(inp.recursion().depth()),
    n => bare_statement(inp, n - 1),
  }
}

/// `let _` — the spelling rustc's own `help:` suggests for the row above, and silent.
fn let_underscore(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, ProbeErr> {
  let _ = inp.descend()?;
  match remaining {
    0 => Ok(inp.recursion().depth()),
    n => let_underscore(inp, n - 1),
  }
}

/// A `Result` combinator. The `Result` is *used*, so no lint fires; the guard inside it dies at
/// the end of the condition, which is a temporary scope, so the body below runs released.
fn result_combinator(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, ProbeErr> {
  if inp.descend().is_ok() {
    match remaining {
      0 => Ok(inp.recursion().depth()),
      n => result_combinator(inp, n - 1),
    }
  } else {
    // Unreachable at this limitation precisely *because* the shape is broken: the depth never
    // climbs past 1, so the check never trips. Reaching this arm would itself be the news.
    Ok(usize::MAX)
  }
}

/// A method chain over the guard. The guard is used — that is what defeats the lint — and then
/// dies at the semicolon, one statement before the recursion.
fn method_chain(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, ProbeErr> {
  let depth = inp.descend()?.recursion().depth();
  match remaining {
    0 => Ok(depth),
    n => method_chain(inp, n - 1),
  }
}

/// An explicit `drop`. The guard is moved into a call, so it is used, and the level ends there.
fn explicit_drop(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, ProbeErr> {
  drop(inp.descend()?);
  match remaining {
    0 => Ok(inp.recursion().depth()),
    n => explicit_drop(inp, n - 1),
  }
}

// ── the cells ─────────────────────────────────────────────────────────────────

/// The rail for [`InputRef::descending`](super::InputRef::descending): a frame whose body is the
/// closure cannot release its own level, so 200 calls against a budget of 8 stop at 9.
///
/// Red if the helper ever stops owning the level for the whole body — the failure is `Ok`.
#[test]
fn descending_holds_the_bound_through_two_hundred_calls() {
  let out = with_budget(|inp| scoped(inp, CALLS));
  assert_eq!(
    out,
    Err(ProbeErr::Trip {
      at: 0,
      depth: TRIP_DEPTH,
      limitation: LIMIT
    }),
    "the scoped form must stop at the budget; an `Ok` here means the level was released early"
  );
}

/// The same for the guard form written correctly — the low-level escape hatch is not broken, it is
/// only easy to write wrong.
#[test]
fn the_bound_guard_form_holds_the_bound_too() {
  let out = with_budget(|inp| bound_guard(inp, CALLS));
  assert_eq!(
    out,
    Err(ProbeErr::Trip {
      at: 0,
      depth: TRIP_DEPTH,
      limitation: LIMIT
    })
  );
}

/// `inp.descend()?;` — bypasses the bound. Caught by `#[must_use]`, and by nothing else.
#[test]
fn bare_statement_bypasses_the_bound() {
  assert_eq!(
    with_budget(|inp| bare_statement(inp, CALLS)),
    Ok(0),
    "200 frames deep with the cell reading zero — the documented behaviour of this shape"
  );
}

/// `let _ = inp.descend()?;` — bypasses the bound, silently.
#[test]
fn let_underscore_bypasses_the_bound() {
  assert_eq!(with_budget(|inp| let_underscore(inp, CALLS)), Ok(0));
}

/// `if inp.descend().is_ok() { … }` — bypasses the bound, silently.
#[test]
fn result_combinator_bypasses_the_bound() {
  assert_eq!(with_budget(|inp| result_combinator(inp, CALLS)), Ok(0));
}

/// `let d = inp.descend()?.recursion().depth();` — bypasses the bound, silently. Reads **1**, not
/// 0, because the depth is sampled while the temporary is still alive; it never reads 2.
#[test]
fn method_chain_temporary_bypasses_the_bound() {
  assert_eq!(with_budget(|inp| method_chain(inp, CALLS)), Ok(1));
}

/// `drop(inp.descend()?);` — bypasses the bound, silently.
#[test]
fn explicit_drop_bypasses_the_bound() {
  assert_eq!(with_budget(|inp| explicit_drop(inp, CALLS)), Ok(0));
}

/// The level `descending` raises is released on an unwind out of the body, not only on a return.
///
/// Red if the guard is ever replaced by an explicit release at the `Ok` exits: the depth would
/// stay raised and this reads a non-zero cell.
#[test]
fn descending_releases_the_level_when_the_body_unwinds() {
  // Recorded outside the closure, never asserted inside it: `catch_unwind` swallows a failing
  // assertion just as happily as the deliberate panic, so an inner `assert_eq!` would make this
  // cell pass no matter what the depth was.
  static INSIDE: AtomicUsize = AtomicUsize::new(usize::MAX);

  with_budget(|inp| {
    assert_eq!(inp.recursion().depth(), 0);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      inp.descending(|inp| -> Result<(), ProbeErr> {
        INSIDE.store(inp.recursion().depth(), Ordering::Relaxed);
        panic!("the body unwinds");
      })
    }));
    assert!(caught.is_err(), "the panic must reach the caller");
    assert_eq!(
      INSIDE.load(Ordering::Relaxed),
      1,
      "the level must be live for the body that unwound"
    );
    assert_eq!(
      inp.recursion().depth(),
      0,
      "the unwind released the level the closure ran in"
    );
  });
}

/// One level per live frame, and the cell returns to zero. This is the balance property, read
/// through the scoped form rather than the guard.
#[test]
fn descending_nests_one_level_per_frame_and_returns_to_zero() {
  static SEEN: AtomicUsize = AtomicUsize::new(0);

  fn ladder(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<(), ProbeErr> {
    inp.descending(|inp| {
      let depth = inp.recursion().depth();
      SEEN.fetch_max(depth, Ordering::Relaxed);
      match remaining {
        0 => Ok(()),
        n => {
          ladder(inp, n - 1)?;
          // After the whole subtree has unwound back to here, THIS frame's level is still the one
          // it entered with — the closure cannot shorten it.
          assert_eq!(
            inp.recursion().depth(),
            depth,
            "the level outlived the recursion below it"
          );
          Ok(())
        }
      }
    })
  }

  with_budget(|inp| {
    assert_eq!(ladder(inp, 4), Ok(()));
    assert_eq!(inp.recursion().depth(), 0, "every level entered was left");
  });
  assert_eq!(SEEN.load(Ordering::Relaxed), 5, "five frames, five levels");
}

/// `descending` and `descend` report the **same** trip — same offset, same depth, same limitation.
/// They share one RAISE/CHECK/ARM sequence, and this is what keeps that true.
#[test]
fn descending_and_descend_report_the_same_trip() {
  fn trip_through_descending(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<(), ProbeErr> {
    inp.descending(|inp| match remaining {
      0 => Ok(()),
      n => trip_through_descending(inp, n - 1),
    })
  }
  fn trip_through_descend(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<(), ProbeErr> {
    let mut frame = inp.descend()?;
    let inp = &mut *frame;
    match remaining {
      0 => Ok(()),
      n => trip_through_descend(inp, n - 1),
    }
  }

  let a = with_budget(|inp| trip_through_descending(inp, CALLS));
  let b = with_budget(|inp| trip_through_descend(inp, CALLS));
  assert_eq!(a, b, "the two spellings must not drift apart");
  assert!(matches!(a, Err(ProbeErr::Trip { .. })));
}

/// The frame's error type is threaded through untouched: `descending` builds the trip as whatever
/// the closure returns, which need not be the emitter's error type at all.
#[test]
fn descending_builds_the_trip_as_the_frames_own_error() {
  fn frame(inp: &mut Frame<'_, '_>, remaining: usize) -> Result<usize, FrameErr> {
    inp.descending(|inp| match remaining {
      0 => Ok(inp.recursion().depth()),
      n => frame(inp, n - 1),
    })
  }

  assert_eq!(
    with_budget(|inp| frame(inp, CALLS)),
    Err(FrameErr(TRIP_DEPTH)),
    "the trip is returned, not emitted, so it is built as the frame's own error"
  );
}
