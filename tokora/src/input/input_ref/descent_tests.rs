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
//! One further cell, `a_leaked_guard_holds_its_level_for_the_rest_of_the_parse`, pins the
//! *opposite* failure and the reason the balance claim is scoped to exit paths: a guard that is
//! leaked rather than dropped holds its level permanently.
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

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

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

/// **The one way the balance claim can be broken, and it is Rust's universal one.** Every *exit
/// path* out of the guard's scope runs its destructor, which is what makes "every level entered is
/// a level left" a property of the type rather than of caller discipline. Leaking the guard —
/// `mem::forget`, `ManuallyDrop`, `Box::leak` — is not an exit path, and it holds the level for the
/// rest of the parse: the cell never comes back down and the input stays usable at the raised
/// depth.
///
/// This is the *opposite* failure from the five bypass cells above, and the milder one: it tightens
/// the budget rather than removing it, so the worst outcome is a spurious
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) and never a native abort. It is
/// pinned because [`Descent`](super::Descent)'s documentation states the balance claim *scoped to
/// exit paths* — a change that made the claim unconditional, or that closed this hole, must change
/// that prose in the same commit.
#[test]
fn a_leaked_guard_holds_its_level_for_the_rest_of_the_parse() {
  with_budget(|inp| {
    assert_eq!(inp.recursion().depth(), 0, "nothing is descended yet");

    {
      let frame = inp.descend().expect("one level fits under the budget");
      core::mem::forget(frame);
    }

    assert_eq!(
      inp.recursion().depth(),
      1,
      "the level survives the scope it was taken in: no destructor ran, so nothing released it"
    );

    // And the budget is genuinely one level tighter for everything that follows: a chain that
    // would otherwise trip at `LIMIT + 1` now trips one frame earlier.
    assert_eq!(
      scoped(inp, CALLS),
      Err(ProbeErr::Trip {
        at: 0,
        depth: TRIP_DEPTH,
        limitation: LIMIT
      }),
      "the parse continues normally — the leaked level is spent budget, not a broken input — and \
       trips having descended one frame fewer than a clean parse would"
    );

    assert_eq!(
      inp.recursion().depth(),
      1,
      "and it is still held after the trip unwinds every level the trip itself raised"
    );
  });
}

// ══════════════════════════════════════════════════════════════════════════════
// The shared budget against the stack it actually runs on
// ══════════════════════════════════════════════════════════════════════════════

/// The thread every figure in the measurement table is stated on, and the one
/// `std::thread::spawn` hands out.
///
/// Written out rather than read from `measured::STACK`: this is the stack the cell *asks for*, and
/// a cell that requested "whatever the table says" would silently re-base itself if the table were
/// ever restated on a different thread — which is precisely the drift the assertions in
/// `state::recursion_tracker` exist to catch, arriving through the test suite instead.
const MEASUREMENT_STACK: usize = 2 * 1024 * 1024;

/// Bytes of native stack each level of [`heavy`] spends, set to the heaviest per-level cost
/// anybody has measured: the consumer row, ~41 KiB.
///
/// This is what makes the cell below a statement about a *stack* rather than about a counter. A
/// frame of a handful of bytes would let any budget whatever fit in 2 MiB, and the cell would pass
/// for a build in which the shared default was 1024.
const HEAVY_LEVEL: usize =
  crate::state::recursion_tracker::measured::CONSUMER_SYNTACTIC_BYTES_PER_LEVEL;

/// How deep [`heavy`] is willing to go before giving up and returning `Ok`.
///
/// **It exists so that a defect reds this cell instead of killing the process.** The budget is
/// what should stop the recursion; if it does not, something has to, and an assertion failure is a
/// test result while `fatal runtime error: stack overflow` is not. 24 levels at ~41 KiB is ≈0.94
/// MiB — comfortably inside the 2 MiB thread — and it is above the shipped default, which is the
/// other half of what it has to be or the cell would never reach the budget at all.
const HEAVY_CAP: usize = 24;

/// A **non-Pratt** recursive production with an ordinary, expensive native frame.
///
/// `descending` and a stack-resident array, which is all a consumer's own recursive combinator is.
/// The `black_box` pair is what keeps the array live *across* the recursive call rather than
/// letting LLVM shrink it to the two instants it is touched.
fn heavy(inp: &mut Frame<'_, '_>, depth: usize) -> Result<usize, ProbeErr> {
  let mut pad = [0u8; HEAVY_LEVEL];
  core::hint::black_box(&mut pad);
  let out = inp.descending(|inp| match depth {
    // Not `Err`: the cell distinguishes "the budget refused" from "this ran out of patience", and
    // conflating them would let a missing refusal read as a refusal.
    HEAVY_CAP => Ok(inp.recursion().depth()),
    n => heavy(inp, n + 1),
  });
  core::hint::black_box(&mut pad);
  out
}

/// Builds an input at the **shipped default** budget — not [`LIMIT`] — and hands the handle to `f`.
fn with_default_budget<O>(f: impl FnOnce(&mut Frame<'_, '_>) -> O) -> O {
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_context(
    "1",
    (),
    InputContext::new(
      Fatal::<ProbeErr>::new(),
      DefaultCache::<'_, ProbeLexer<'_>>::default(),
    ),
  );
  let mut inp = input.as_ref();
  f(&mut inp)
}

/// **The shared budget must not authorise a depth the native stack cannot hold** — measured on a
/// 2 MiB thread, through the path `stacker` never touches.
///
/// # The defect this is the regression for
///
/// `PARSE_DEFAULT_DEPTH` was `if cfg!(feature = "stacker") { 1024 } else { 16 }`. But
/// `native_stack::maybe_grow` is called from the two Pratt frame prologues and from nowhere else,
/// while that constant seeds **every** `InputContext` — including the budget a consumer's own
/// `descending` production draws on, which is what this cell recurses through. So a `stacker`
/// build authorised 1024 ordinary unsegmented frames, and 1024 × ~41 KiB is ≈41 MiB against a
/// 2 MiB thread: the native abort the whole recursion budget exists to delete, reintroduced by the
/// feature that was supposed to remove it.
///
/// It is deliberately **not** `#[cfg(feature = "stacker")]`. The property is that the shared
/// budget is safe for unsegmented frames in *every* configuration, and a cell that only ran with
/// the feature on would say nothing about the build that does not have it.
///
/// # Why it cannot simply recurse until something stops it
///
/// Because the failure mode under test *is* a dead process, and a dead process is not a test
/// result. [`heavy`] therefore caps itself at [`HEAVY_CAP`] and returns `Ok`, so a build whose
/// budget authorised too much comes back as a failed assertion on this line rather than as
/// `fatal runtime error: stack overflow` in the harness. That is also what makes the plant
/// legible: restore the `cfg!` arm and this cell reds, rather than taking the suite with it.
#[test]
fn the_shared_budget_refuses_unsegmented_descent_within_a_2mib_thread() {
  let default = RecursionLimiter::PARSE_DEFAULT_DEPTH;

  // FIRST, THE ARITHMETIC, so that a budget too large to probe safely is reported rather than
  // executed. This is the whole claim in one line, and the runtime half below is its witness.
  assert!(
    default * HEAVY_LEVEL < MEASUREMENT_STACK,
    "the shared recursion budget is {default}, which at the heaviest measured per-level cost of \
     {HEAVY_LEVEL} bytes authorises {} bytes of native stack against a {MEASUREMENT_STACK}-byte \
     thread. A budget only a segmented path could survive does not belong on the cell every \
     unsegmented `descend` reads.",
    default * HEAVY_LEVEL
  );
  assert!(
    default < HEAVY_CAP,
    "this cell recurses at most {HEAVY_CAP} levels, so a budget of {default} would never be \
     reached and the pin below would be vacuous"
  );

  let out = std::thread::Builder::new()
    .stack_size(MEASUREMENT_STACK)
    .spawn(move || with_default_budget(|inp| heavy(inp, 0)))
    .expect("spawning the measurement thread")
    .join()
    .expect("the budget must refuse this as a VALUE — a panic or an abort here is the defect");

  assert_eq!(
    out,
    Err(ProbeErr::Trip {
      at: 0,
      depth: default + 1,
      limitation: default
    }),
    "the shared budget must refuse the frame after the default, as an ordinary catchable value. \
     An `Ok` here means the recursion reached {HEAVY_CAP} levels unrefused, which is the shared \
     budget authorising more unsegmented native stack than it can account for"
  );
}
