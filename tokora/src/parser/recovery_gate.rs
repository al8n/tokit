//! The never-recoverable law's chokepoint for the recovery combinators — **the one place** a
//! recovery attempt's failure is judged, and the one place its baselines are taken.
//!
//! [`Recover`](super::Recover), [`InplaceRecover`](super::InplaceRecover) and
//! [`SkipThenRetry`](super::SkipThenRetry) run four attempts between them, and each one has to
//! answer the same question: *did something no further input clears happen inside this attempt?*
//! Four hand-written copies of that guard is how the scanner-side witnesses came to be consulted
//! zero times at all four sites while `parser::many`'s twelve drivers consulted one of them at
//! every one. Here there is no guard to spell: a combinator hands its attempt to
//! [`speculated_attempt`] or [`inplace_attempt`] and matches on an [`Attempted`], and `GATE_CENSUS`
//! pins that none of the four sources names a witness or a baseline of its own.
//!
//! [`SkipThenRetry`](super::SkipThenRetry) also does recovery work that is **not** a parse attempt
//! — the skip that finds a sync point and the advance that consumes a spent one — and that work
//! goes through [`recovery_step`] for the reason the [section below](#the-work-outside-an-attempt)
//! gives.
//!
//! # The five witnesses
//!
//! Two ride the error value and three ride the input, and none subsumes another *as a question*,
//! though the last two overlap heavily as answers:
//!
//! * `err.is_incomplete()` — the frontier [`Incomplete`](crate::error::Incomplete). Not a malformed
//!   construct, merely an unfinished one, so recovering past it fabricates output from input that
//!   has not finished arriving.
//! * `err.is_terminal()` — a terminal stop the grammar's error type **did** carry: the
//!   terminal-marked [`UnexpectedEot`](crate::error::UnexpectedEot) an accepting emitter's
//!   `Tripped` arm builds.
//! * `inp.tripped_during_attempt(trips)` — a **descent** budget trip. It latches no boundary (it
//!   has a control stack rather than a position), and the grammar's error type is free to discard
//!   [`RecursionLimitReached`](crate::error::RecursionLimitReached) on conversion — `()` does — so
//!   the session counter is what makes the verdict independent of the error sink.
//! * `inp.scanner_tripped_during_attempt(scans)` — a **scanner** limit trip, counted on the input
//!   session. This is the load-bearing scanner witness, and the
//!   [section below](#why-the-scanner-witness-is-a-counter-and-not-the-latch) is why it has to be a
//!   monotone counter rather than the boundary that trip latched.
//! * `inp.latched_during_attempt(latch)` — the scanner's **poison boundary**, compared against a
//!   per-attempt snapshot. The narrower of the two scanner readings: it answers *a different stop
//!   is standing at the end of this attempt*, where the counter answers *a stop happened inside
//!   it*. Every transition this crate can produce that moves the latch also moves the counter, so
//!   this term adds no verdict the counter does not already give; it is kept because it is the
//!   reading the boundary itself is the subject of, it costs one `Option<L::Offset>` clone on a
//!   path that already clones one, and a witness removed on a subsumption argument is a witness
//!   removed on an argument rather than on a measurement.
//!
//! # Why the error alone cannot answer, and why this is not a `Fatal` quirk
//!
//! One trip, one position, one committed leaf, two emitters:
//!
//! * an **accepting** emitter files the trip's diagnostic, `next_or_stop`/`try_expect_or_stop`
//!   reach their `Tripped` arm, and the caller gets an `UnexpectedEot` through
//!   [`into_terminal`](crate::error::UnexpectedEnd::into_terminal) — `is_terminal()` is `true`;
//! * a **rejecting** emitter reports the same trip by returning `Err` from
//!   [`emit_lexer_error`](crate::Emitter::emit_lexer_error) — that `Err` is not a refusal to
//!   report, it **is** the report — and the scanner propagates that value straight out of
//!   `scan_with`'s `Verdict::Trip` arm. It was built by
//!   [`FromEmitterError::from_lexer_error`](crate::emitter::FromEmitterError::from_lexer_error) out
//!   of the lexer's own `Token::Error`; no `UnexpectedEnd` exists on that path, so nothing ever
//!   calls `into_terminal` and `is_terminal()` is `false`.
//!
//! Every fail-fast emitter behaves that way, so this is a property of the *channel*, not of
//! [`Fatal`](crate::emitter::Fatal). Terminality is a fact about the **event**, this crate
//! enumerates **carriers** of it, and the two come apart in both directions — the same root cause
//! the descent counter was added for. The scanner's half of it is `Input::scanner_trips`, and
//! reading it here is what makes every gated site answer the same way for every sink.
//!
//! # Why the scanner witness is a COUNTER and not the latch
//!
//! The obvious scanner witness is the poison boundary the trip latched, and it is the wrong one at
//! every nesting depth. It is a **lineage memo**: a [`Checkpoint`](crate::input::Checkpoint)
//! carries it and [`restore`](crate::InputRef::restore) copies the saved value back verbatim.
//!
//! That defeats a comparison twice over, one level at a time.
//!
//! 1. A `latch_snapshot()` taken before [`try_attempt`](crate::InputRef::try_attempt) and compared
//!    after it compares a restored latch against the value it was restored *to*: always equal,
//!    always `false`, a dead term. Measured, not reasoned about: with the read placed after the
//!    attempt, `tokora/tests/recovery_terminal_stop.rs`'s in-place cell passed and all three
//!    speculating cells still failed with the scanner re-entered after the trip.
//! 2. Moving the read *inside* the attempt closes exactly that level. Grammar code is free to catch
//!    a scanner stop inside an **inner** attempt or transaction of its own, and that inner rollback
//!    restores the boundary before the outer read runs — so the outer verdict reads clean over a
//!    stop that is live, already diagnosed, and will re-trip on the next scan of the same prefix.
//!    Measured the same way: `recover_reraises_a_trip_caught_inside_a_nested_speculation` observed
//!    the recoverer running and the scan count moving 3 → 4 with the read placed inside the outer
//!    attempt.
//!
//! Relocating the read a third time would close level two and open level three. **A cell inside the
//! rollback set cannot witness an event across a rollback at any depth**, so the witness is a cell
//! no rollback reaches: `Input::scanner_trips`, a monotone counter bumped by
//! `InputRef::latch_if_limit_tripped` — the crate's sole terminal predicate, reached by both lexing
//! drivers — and outside the rollback set by the same taxonomy that puts the descent counter there.
//! This is the [descent side's](crate::InputRef::descend) answer applied to the scanner, and it is
//! the same root cause both times: this crate enumerates *carriers* of terminality, and a terminal
//! *event* can reach a caller through an unmarked or a rewound one.
//!
//! # The baselines are taken here, and the witnesses are read INSIDE the attempt
//!
//! With the counter in place the placement no longer decides the verdict — the counter reads the
//! same before or after any rollback — but [`judge`] still runs inside the closure the attempt
//! wraps, and the latch reading is why. Read outside, that term is the dead one described above;
//! read inside, it is at least the honest narrow reading it claims to be. One body, one place, and
//! the verdict rides out on the error channel beside the error.
//!
//! # The work outside an attempt
//!
//! A gate that wraps only the parse attempts says nothing about recovery work that never enters
//! one, and [`SkipThenRetry`](super::SkipThenRetry) does its actual recovering out there: the skip
//! to a sync point, and the advance over a sync point that did not admit a parse. Both are `Ok`-
//! returning primitives that fold a **terminal trip** into the same value as a genuine end of input
//! — `sync_balanced` gives `Ok(None)` for either, `next` gives `Ok(None)` for either — so a spent
//! scanner budget read as *"nothing left to skip to"* and the combinator surfaced its ordinary
//! trigger error over a stop.
//!
//! [`recovery_step`] is the same discipline for that shape: sample the input-side witnesses around
//! the operation and report a [`Stepped::Stopped`] when they move. It reads three witnesses rather
//! than five because the other two interrogate an error value and this shape has none — the step
//! returned `Ok`; an `Err` from it is already the emitter's own report and propagates untouched.
//!
//! # Granularity, and where the baselines belong
//!
//! One **attempt** is the unit, exactly as one *element* is `parser::many`'s: within it the verdict
//! is "something no further input clears happened here", and it fails closed. An ordinary failure
//! that shares its attempt with a trip the grammar caught and parsed past is re-raised rather than
//! recovered; the reverse never happens. See `parser::many`'s module docs for why the strong form —
//! "re-raise only when this very error *is* the trip" — is not implementable while the grammar's
//! error type may be `()`.
//!
//! Both baselines are per attempt, and taking them here is what makes that unspellable rather than
//! merely pinned. `SkipThenRetry`'s retry baseline in particular has to be **per retry cycle**: the
//! session counter is monotone and never cleared, so a baseline hoisted out of the retry loop
//! degrades into a session-absolute read and every retry after the parse's first trip re-raises,
//! ordinary syntax errors included. There is no longer a baseline at that call site to hoist.

use crate::error::{MaybeIncomplete, MaybeTerminal};
use crate::input::{ResourceTripBaseline, ScannerTripBaseline};

use super::*;

/// The emitter error of a recovery attempt's context, spelled once.
type Failure<'inp, L, Ctx, Lang> =
  <<Ctx as ParseContext<'inp, L, Lang>>::Emitter as Emitter<'inp, L, Lang>>::Error;

/// What one recovery attempt yielded, already judged.
pub(super) enum Attempted<O, E> {
  /// The attempt succeeded, and its progress is kept.
  Done(O),
  /// The attempt failed with something no further input clears — an `Incomplete`, or a terminal
  /// stop in either of the places terminality is stored. Re-raise it **untouched**: recovery
  /// synthesizes progress over a *malformed* construct, and neither an unfinished construct nor a
  /// tripped budget is that.
  Reraise(E),
  /// The attempt failed with an ordinary error. Recovery may run.
  Recoverable(E),
}

/// Runs `f` as a **speculating** attempt — [`try_attempt`](InputRef::try_attempt), so a failure
/// rolls back to the pre-attempt state (position, lexer state, emissions, dedup watermark, poison
/// boundary) — and judges its failure before that rollback happens.
///
/// The shape [`Recover`](super::Recover) and both of
/// [`SkipThenRetry`](super::SkipThenRetry)'s attempts take.
#[inline(always)]
pub(super) fn speculated_attempt<'inp, 'closure, L, O, Ctx, Lang: ?Sized, Cmpl, F>(
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  f: F,
) -> Attempted<O, Failure<'inp, L, Ctx, Lang>>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
  Failure<'inp, L, Ctx, Lang>: MaybeIncomplete + MaybeTerminal,
  F: FnOnce(
    &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, Failure<'inp, L, Ctx, Lang>>,
{
  let trips = inp.trip_snapshot();
  let scans = inp.scanner_trip_snapshot();
  let latch = inp.latch_snapshot();
  settle(inp.try_attempt(|input| judge(input, f, &latch, trips, scans)))
}

/// Runs `f` **in place** — no checkpoint, no rollback, the input stays where the failure left it —
/// and judges its failure.
///
/// The shape [`InplaceRecover`](super::InplaceRecover) takes. Nothing rewinds the latch here, so
/// the read inside [`judge`] is the same read an after-the-fact one would be; it is inside for the
/// same reason both baselines are taken here, which is that there is then one body to check.
#[inline(always)]
pub(super) fn inplace_attempt<'inp, 'closure, L, O, Ctx, Lang: ?Sized, Cmpl, F>(
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  f: F,
) -> Attempted<O, Failure<'inp, L, Ctx, Lang>>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
  Failure<'inp, L, Ctx, Lang>: MaybeIncomplete + MaybeTerminal,
  F: FnOnce(
    &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, Failure<'inp, L, Ctx, Lang>>,
{
  let trips = inp.trip_snapshot();
  let scans = inp.scanner_trip_snapshot();
  let latch = inp.latch_snapshot();
  settle(judge(inp, f, &latch, trips, scans))
}

/// Runs one attempt and, on failure, reads all five never-recoverable witnesses **before returning
/// to whatever wraps the attempt** — which for a speculating one is the rollback that would erase
/// the scanner latch.
///
/// The verdict rides out on the error channel beside the error, so the wrapper needs no
/// out-parameter and no cell: a `try_attempt` whose `Err` type is `(E, bool)` rolls back exactly as
/// one whose `Err` type is `E`.
#[inline(always)]
fn judge<'inp, 'closure, L, O, Ctx, Lang: ?Sized, Cmpl, F>(
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  f: F,
  latch: &Option<L::Offset>,
  trips: ResourceTripBaseline<'closure>,
  scans: ScannerTripBaseline,
) -> Result<O, (Failure<'inp, L, Ctx, Lang>, bool)>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
  Failure<'inp, L, Ctx, Lang>: MaybeIncomplete + MaybeTerminal,
  F: FnOnce(
    &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, Failure<'inp, L, Ctx, Lang>>,
{
  match f(inp) {
    Ok(output) => Ok(output),
    Err(err) => {
      let never = err.is_incomplete()
        || err.is_terminal()
        || inp.tripped_during_attempt(trips)
        || inp.scanner_tripped_during_attempt(scans)
        || inp.latched_during_attempt(latch);
      Err((err, never))
    }
  }
}

/// What one recovery **side step** yielded — the skip that looks for a sync point, and the advance
/// over a sync point that did not admit a parse.
///
/// Neither is a parse attempt: nothing speculates, nothing rolls back, and an `Err` out of one is
/// the emitter's own report rather than a grammar failure to be judged. What they share with an
/// attempt is the hazard: each folds a **terminal stop** into the same `Ok` value it uses for a
/// genuine end of input.
pub(super) enum Stepped<T> {
  /// The step completed, and no terminal stop happened inside it. Its value means what it says.
  Went(T),
  /// A terminal stop happened **inside the step**. Whatever `Ok` it produced, it is not the end of
  /// input it looks like: the budget is spent, and no further skipping or advancing clears it.
  Stopped,
}

/// Runs one recovery side step and samples the three input-side witnesses across it.
///
/// The error-value witnesses have no subject here — the step returned `Ok`, and its `Err` is
/// propagated untouched by the `?` at the call site, being already a report rather than something
/// to judge.
///
/// Reading **after** the step, rather than inside it, is correct for this shape and for one reason:
/// a step performs no speculation of its own. `sync_balanced`'s trip exit deliberately *keeps* its
/// entry snapshot (the skipped prefix is committed progress), and `next` never rewinds. Even where
/// that were not so, the counter reading would survive it — which is precisely the property the
/// latch reading does not have, and why the latch is the last term here rather than the first.
#[inline(always)]
pub(super) fn recovery_step<'inp, 'closure, L, T, Ctx, Lang: ?Sized, Cmpl, F>(
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  f: F,
) -> Result<Stepped<T>, Failure<'inp, L, Ctx, Lang>>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
  F: FnOnce(
    &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  ) -> Result<T, Failure<'inp, L, Ctx, Lang>>,
{
  let trips = inp.trip_snapshot();
  let scans = inp.scanner_trip_snapshot();
  let latch = inp.latch_snapshot();
  let went = f(inp)?;
  if inp.tripped_during_attempt(trips)
    || inp.scanner_tripped_during_attempt(scans)
    || inp.latched_during_attempt(&latch)
  {
    return Ok(Stepped::Stopped);
  }
  Ok(Stepped::Went(went))
}

/// Turns [`judge`]'s carried verdict into the three-way answer the combinators match on.
#[inline(always)]
fn settle<O, E>(out: Result<O, (E, bool)>) -> Attempted<O, E> {
  match out {
    Ok(output) => Attempted::Done(output),
    Err((err, true)) => Attempted::Reraise(err),
    Err((err, false)) => Attempted::Recoverable(err),
  }
}

// ── RECOVERY_GATE_CENSUS_END — production above, tests below ──────────────────
//
// `RECOVERY_GATE_CENSUS` reads this file's source and counts needles that its own constants also
// spell. The split is this marker rather than the first `#[cfg(test)]`, because the recovery
// suites are feature-gated and their attribute is not that literal; the census `expect()`s the
// marker, so deleting it fails loudly instead of silently widening the scan over test code.

#[cfg(all(test, feature = "many"))]
mod gate_census {
  //! RECOVERY_GATE_CENSUS — the recovery combinators' never-recoverable gate, locked to one
  //! chokepoint.
  //!
  //! `parser::many`'s `GATE_CENSUS` reaches its twelve drivers and stops there, so for as long as
  //! this law had four hand-written copies in `recover.rs` and `skip_then_retry.rs` nothing read
  //! them at all. What that cost is measured, not supposed: all twelve drivers consulted the
  //! scanner latch at every guarded exit and the four recovery sites consulted it **zero** times,
  //! which is how a resource-limit trip reported by a rejecting emitter reached the recoverer and
  //! was retried. `tokora/tests/recovery_terminal_stop.rs` is the behaviour; this is the structure.
  //!
  //! # The shape that let a whole phase go unread, and what was done about it
  //!
  //! The first version of this census pinned what happens **inside** the chokepoint, and said
  //! nothing whatever about recovery work that never enters one. That is not a gap in its
  //! assertions; it is a gap in its *subject*. `SkipThenRetry`'s skip and advance are recovery — the
  //! skip is the entire recovery it performs — and both ran outside every attempt, folding a
  //! terminal trip into the same `Ok(None)` they use for genuine exhaustion. A census that
  //! enumerates the guarded sites can only ever be as complete as its list of what counts as a
  //! site, so the list now names the **phases**, not just the attempts, and
  //! [`every_recovery_step_routes_through_the_step_chokepoint`] is the half that reads outside an
  //! attempt.
  //!
  //! # What is pinned, and in which direction
  //!
  //! * [`every_recovery_attempt_routes_through_the_chokepoint`] — the spelling-independent half.
  //!   Each of the two sources carries the exact number of chokepoint calls it is classified with,
  //!   matches **all three** [`Attempted`](super::Attempted) outcomes at each one, and spells
  //!   **zero** witnesses, **zero** baselines and **zero** attempts of its own. There is nothing
  //!   left to balance: a site that judges its own failure has to name a witness, and every name it
  //!   could use is asserted absent — including the two positional readings a hand-written guard
  //!   would most plausibly reach for.
  //! * [`every_recovery_step_routes_through_the_step_chokepoint`] — the same, one level out. Each
  //!   source carries its classified number of [`recovery_step`](super::recovery_step) calls,
  //!   matches **both** [`Stepped`](super::Stepped) outcomes at each, and reaches **no** recovery
  //!   primitive on its own handle. That last is the load-bearing one and it works off a naming
  //!   convention the whole family already keeps: a combinator's handle is `inp` and the
  //!   chokepoint's closure parameter is `input`, so `inp.sync_balanced(` can only be a skip that
  //!   went around the gate.
  //! * [`the_chokepoint_reads_every_witness_before_its_verdict`] — the region scan, for both
  //!   judging bodies. The two error-carried witnesses appear exactly once in the module, inside
  //!   `judge`; the three input-side ones appear exactly twice, once inside `judge` and once inside
  //!   `recovery_step`, each ahead of the verdict its body carries out. Exact counts plus
  //!   present-in-region is what proves the runners read none, and what catches a witness quietly
  //!   dropped from one body while the other still has it — the shape a behaviour suite cannot see,
  //!   because the remaining witnesses answer the same way on every case it builds.
  //! * [`the_chokepoint_takes_its_baselines_per_attempt`] — one baseline triple per runner, taken in
  //!   the runner and not in the judging body, and `speculated_attempt` wrapping `judge` in the
  //!   attempt rather than judging outside it.
  //!
  //! # What a needle scan proves, and what it does not
  //!
  //! Presence, count and textual order. **Not** control-flow domination: a `judge` that read every
  //! witness into `let _ =` bindings and returned `false` would keep every test here green. What
  //! proves the gate gates is behaviour — `tokora/tests/recovery_terminal_stop.rs`'s trip cells,
  //! each of which goes red if a witness stops being consulted, and its scoping cells, which go red
  //! if the gate starts refusing ordinary failures.
  //!
  //! Watched failing rather than assumed, three times, each with the whole behaviour suite green:
  //!
  //! * replacing `inp.latched_during_attempt(latch)` in `judge` with the *positional*
  //!   `inp.at_committed_boundary()` — a real and subtle regression, since a rollback over the latch
  //!   rewinds the cursor behind a boundary that survives — reds two assertions here;
  //! * re-spelling `Recover`'s guard by hand, correctly, with its own baselines;
  //! * **deleting the scanner-counter witness from `recovery_step`** — the sync/advance phase's own
  //!   witness. The behaviour suite stays entirely green, because on every case it builds the
  //!   latch reading beside it answers the same way; the count for
  //!   `inp.scanner_tripped_during_attempt(` drops from two to one and both the module tally and the
  //!   `recovery_step` region assertion fire. That is the precise shape this census exists for: a
  //!   witness that is redundant *on the cases anyone thought to write* and load-bearing on the ones
  //!   they did not.
  //!
  //! # This census's frontier, stated rather than left to be discovered
  //!
  //! It reads a fixed list of **two** combinator sources plus the chokepoint. A fifth recovery
  //! attempt, or a third step, added to either of those two moves a count here; one added in a
  //! **new file** is not read, exactly as `GATE_CENSUS`'s swallow scan is bounded by
  //! `swallow_scan_sites`. The cheap half of that hole is closed by the same property the drivers
  //! rely on: any hand-written guard must name one of the needles below, and `GATE_CENSUS` already
  //! pins them absent across the twelve drivers, so the uncovered surface is *new recovery
  //! combinators only*. A new one belongs in [`recovery_sites`] before it can be trusted.
  //!
  //! The step half has a second frontier, and it is a list rather than a property:
  //! [`UNGATED_STEPS`] enumerates the primitives a recovery phase can reach for, and a phase built
  //! on one nobody listed is a phase this census does not see. The list is the sync family plus the
  //! plain consume, which is what "recovery work" means in this crate today.

  use crate::parser::many::end_state_census::{code_find, code_matches};

  /// The marker every scanned source ends its production section with.
  const MARKER: &str = "// ── RECOVERY_GATE_CENSUS_END";

  /// The two never-recoverable witnesses carried by the **error value**. Only a judging body that
  /// holds an error can read them, which is why `recovery_step` — whose subject returned `Ok` —
  /// reads neither.
  const ERROR_WITNESSES: [&str; 2] = ["is_incomplete()", "is_terminal()"];

  /// The three never-recoverable witnesses carried by the **input**. Every judging body reads all
  /// three, because every one of them is answering "did a stop happen inside this region".
  const INPUT_WITNESSES: [&str; 3] = [
    "inp.tripped_during_attempt(",
    "inp.scanner_tripped_during_attempt(",
    "inp.latched_during_attempt(",
  ];

  /// Every witness, for the assertion that a combinator source spells none of them.
  const WITNESSES: [&str; 5] = [
    "is_incomplete()",
    "is_terminal()",
    "inp.tripped_during_attempt(",
    "inp.scanner_tripped_during_attempt(",
    "inp.latched_during_attempt(",
  ];

  /// The three per-region baselines the input-side witnesses are read against.
  const BASELINES: [&str; 3] = [
    "inp.trip_snapshot()",
    "inp.scanner_trip_snapshot()",
    "inp.latch_snapshot()",
  ];

  /// The two **positional** scanner readings, which must appear nowhere in this family.
  ///
  /// Pinned as an absence rather than left to be noticed, for the reason
  /// `many::close_after_element`'s negative half is: a positional reading is not restore-stable. An
  /// inner attempt may latch a trip and then roll back — the cursor and the lex offset rewind
  /// *behind* a boundary that survives, because the checkpoint saved it after the trip took it —
  /// and every positional witness reads clean there while the stop is live and already diagnosed.
  /// Comparing the latch **value** against a per-attempt snapshot is what sees it. A substitution
  /// here is the kind of regression a behaviour suite that never builds that shape cannot catch,
  /// so it is caught by name.
  const POSITIONAL: [&str; 2] = ["at_committed_boundary()", "at_latched_boundary()"];

  /// The attempt itself. A combinator that opens its own is a combinator that can judge its own
  /// failure, so the count in the two sources must be zero: the attempt belongs to the chokepoint.
  const ATTEMPT: &str = "try_attempt(";

  /// The recovery-phase primitives, spelled **on the combinator's own handle**.
  ///
  /// Every one of them answers `Ok` with a value that means "nothing left" for two entirely
  /// different reasons — genuine exhaustion, and a terminal stop — so reaching one directly is
  /// reaching past the gate that tells those apart. The needles carry the `inp.` receiver on
  /// purpose: inside a step the handle is the chokepoint's closure parameter (`input`), so these
  /// spellings can only be a phase that went around it.
  ///
  /// This is a **list**, and therefore this census's second frontier: a recovery phase built on a
  /// primitive nobody added here is a phase nothing reads. It names the sync family and the plain
  /// consume, which is the whole of what a recovery phase reaches for today.
  const UNGATED_STEPS: [&str; 5] = [
    "inp.sync_balanced(",
    "inp.sync_to(",
    "inp.sync_through(",
    "inp.skip_until(",
    "inp.next(",
  ];

  /// The two attempt entry points, and the three outcomes every call must match on.
  const SPECULATED: &str = "speculated_attempt(";
  const INPLACE: &str = "inplace_attempt(";
  const OUTCOMES: [&str; 3] = [
    "Attempted::Done(",
    "Attempted::Reraise(",
    "Attempted::Recoverable(",
  ];

  /// The step entry point, and the two outcomes every call must match on.
  const STEP: &str = "recovery_step(";
  const STEP_OUTCOMES: [&str; 2] = ["Stepped::Stopped", "Stepped::Went("];

  /// The judging bodies, and the verdict that ends each one's guard region.
  const JUDGE_DEF: &str = "fn judge";
  const VERDICT: &str = "Err((err, never))";
  const STEP_DEF: &str = "fn recovery_step";
  const STEP_VERDICT: &str = "return Ok(Stepped::Stopped);";

  /// `speculated_attempt` judges **inside** the attempt, not after it.
  const WRAPPED: &str = "try_attempt(|input| judge(";

  /// A scanned source with its census-visible section only.
  fn production(name: &str, src: &'static str) -> &'static str {
    src.split_once(MARKER).unwrap_or_else(|| {
      panic!(
        "{name}: the `RECOVERY_GATE_CENSUS_END` marker is gone. Without it this census would scan \
         the test fixtures below it, which spell every needle it counts — re-cut the marker before \
         trusting a green run"
      )
    }).0
  }

  /// One classified combinator source.
  struct Site {
    /// The path, for the assertion messages.
    name: &'static str,
    /// The whole file; [`production`] cuts it at the marker.
    src: &'static str,
    /// Speculating attempts — [`speculated_attempt`](super::speculated_attempt).
    speculating: usize,
    /// In-place attempts — [`inplace_attempt`](super::inplace_attempt).
    inplace: usize,
    /// Side steps — [`recovery_step`](super::recovery_step).
    steps: usize,
  }

  /// The recovery combinator sources, with every gated site each one carries.
  ///
  /// `recover.rs` hosts `Recover` (one speculating attempt) and `InplaceRecover` (one in-place
  /// attempt), and recovers by handing the input to a recoverer, so it takes no step of its own.
  /// `skip_then_retry.rs` hosts `SkipThenRetry`'s first attempt and its retry — both speculating,
  /// the second deliberately **inside** the retry loop — and the two phases that are its actual
  /// recovery: the skip that finds a sync point and the advance over a spent one.
  fn recovery_sites() -> [Site; 2] {
    [
      Site {
        name: "parser/recover.rs",
        src: include_str!("recover.rs"),
        speculating: 1,
        inplace: 1,
        steps: 0,
      },
      Site {
        name: "parser/skip_then_retry.rs",
        src: include_str!("skip_then_retry.rs"),
        speculating: 2,
        inplace: 0,
        steps: 2,
      },
    ]
  }

  /// This module's own production section.
  fn gate_production() -> &'static str {
    production("parser/recovery_gate.rs", include_str!("recovery_gate.rs"))
  }

  /// Every recovery attempt is the chokepoint's, and no combinator judges its own failure.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_recovery_attempt_routes_through_the_chokepoint() {
    let mut attempts = 0;
    for site in recovery_sites() {
      let Site {
        name,
        speculating,
        inplace,
        ..
      } = site;
      let src = production(name, site.src);
      assert_eq!(
        code_matches(src, SPECULATED),
        speculating,
        "{name}: expected {speculating} speculating attempt(s) (`{SPECULATED}`). An attempt added \
         without one judges its own failure, which is how four sites came to read neither \
         input-side witness"
      );
      assert_eq!(
        code_matches(src, INPLACE),
        inplace,
        "{name}: expected {inplace} in-place attempt(s) (`{INPLACE}`)"
      );

      let calls = speculating + inplace;
      for outcome in OUTCOMES {
        assert_eq!(
          code_matches(src, outcome),
          calls,
          "{name}: every chokepoint call matches all three outcomes — `{outcome}` appears a \
           different number of times than the {calls} call(s). Folding `Reraise` together with \
           `Recoverable` is the whole defect, spelled in one arm"
        );
      }

      for needle in WITNESSES.iter().chain(&BASELINES).chain(&POSITIONAL) {
        assert_eq!(
          code_matches(src, needle),
          0,
          "{name}: a recovery combinator hands its attempt to the chokepoint — it never reads \
           `{needle}` itself, and never takes a baseline for one. A hand-spelled guard can consult \
           four of the five witnesses and look complete, which is exactly the state this file was \
           in"
        );
      }
      assert_eq!(
        code_matches(src, ATTEMPT),
        0,
        "{name}: the attempt belongs to the chokepoint (`{ATTEMPT}` must not appear here). A \
         combinator that opens its own speculation is one that can judge inside it, and a judgement \
         written outside a rollback reads a latch the rollback already erased"
      );
      attempts += calls;
    }
    assert_eq!(
      attempts, 4,
      "the recovery family runs exactly four attempts: `Recover`, `InplaceRecover`, and \
       `SkipThenRetry`'s first attempt and its retry. The total is pinned so an attempt cannot move \
       between the two sources unnoticed"
    );
  }

  /// Every recovery **phase** that is not an attempt is the step chokepoint's, and no combinator
  /// reaches a recovery primitive on its own handle.
  ///
  /// The half the first version of this census could not have had, because its subject was the
  /// inside of an attempt and this work never enters one. It is also the half with teeth on the
  /// cheapest possible mistake: writing the skip the way it was written for two releases.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_recovery_step_routes_through_the_step_chokepoint() {
    let mut steps = 0;
    for site in recovery_sites() {
      let Site {
        name, steps: want, ..
      } = site;
      let src = production(name, site.src);
      assert_eq!(
        code_matches(src, STEP),
        want,
        "{name}: expected {want} recovery step(s) (`{STEP}`). A skip or an advance written without \
         one reads a terminal trip as the end of input, because both primitives answer `Ok(None)` \
         for either"
      );
      for outcome in STEP_OUTCOMES {
        assert_eq!(
          code_matches(src, outcome),
          want,
          "{name}: every step matches BOTH outcomes — `{outcome}` appears a different number of \
           times than the {want} step(s). Handling only `Went` is the defect with the gate bolted \
           on: the stop is detected and then discarded"
        );
      }
      for needle in UNGATED_STEPS {
        assert_eq!(
          code_matches(src, needle),
          0,
          "{name}: a recovery phase goes through the step chokepoint — `{needle}` reaches the \
           primitive on the combinator's own handle, which is the spelling that bypasses it. \
           Inside a step the handle is the chokepoint's closure parameter, so a gated call cannot \
           produce this text"
        );
      }
      steps += want;
    }
    assert_eq!(
      steps, 2,
      "the recovery family runs exactly two side steps: `SkipThenRetry`'s skip to a sync point and \
       its advance over a spent one. Pinned as a total so a phase cannot move between the sources \
       unnoticed"
    );
  }

  /// The code ahead of `def`'s verdict, in the chokepoint's own production section.
  fn guard_region(def: &str, verdict: &str) -> &'static str {
    let prod = gate_production();
    let def_at = code_find(prod, def).unwrap_or_else(|| {
      panic!(
        "`parser/recovery_gate.rs`: `{def}` is gone. A judging body has been renamed or removed; \
         re-cut this census against whatever replaced it before trusting a green run"
      )
    });
    let body = &prod[def_at..];
    let verdict_at = code_find(body, verdict).unwrap_or_else(|| {
      panic!("`parser/recovery_gate.rs`: `{def}` no longer ends its guard with `{verdict}`")
    });
    &body[..verdict_at]
  }

  /// Every witness is consulted, in the judging body that can hold its subject, before that body's
  /// verdict leaves it.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn the_chokepoint_reads_every_witness_before_its_verdict() {
    let prod = gate_production();
    for needle in POSITIONAL {
      assert_eq!(
        code_matches(prod, needle),
        0,
        "`parser/recovery_gate.rs`: the recovery gate's scanner witnesses are a monotone session \
         counter and the latch VALUE against a per-region snapshot, never the positional \
         `{needle}`. A rollback over the latch rewinds the cursor and the lex offset behind a \
         boundary that survives, so a positional reading is clean there while the stop is live"
      );
    }

    let judged = guard_region(JUDGE_DEF, VERDICT);
    let stepped = guard_region(STEP_DEF, STEP_VERDICT);

    // The error-carried pair: one reading each, and only where an error exists to interrogate.
    for witness in ERROR_WITNESSES {
      assert_eq!(
        code_matches(prod, witness),
        1,
        "`parser/recovery_gate.rs`: `{witness}` is read exactly once in this module, in `judge`. A \
         second reading is a second judgement"
      );
      assert_eq!(
        code_matches(judged, witness),
        1,
        "`parser/recovery_gate.rs`: `judge` must consult `{witness}` before its verdict"
      );
      assert_eq!(
        code_matches(stepped, witness),
        0,
        "`parser/recovery_gate.rs`: `recovery_step` judges an `Ok` outcome — there is no error \
         value for `{witness}` to be asked of, and asking it of one that does not exist means the \
         step has grown a second job"
      );
    }

    // The input-carried triple: read by BOTH bodies, because both ask the same question of the
    // input — did a stop happen inside this region.
    for witness in INPUT_WITNESSES {
      assert_eq!(
        code_matches(prod, witness),
        2,
        "`parser/recovery_gate.rs`: `{witness}` is read exactly twice in this module — once in \
         `judge`, once in `recovery_step`. A count of one is a witness dropped from one of the two \
         bodies, which the behaviour suite cannot see: on the cases it builds the other witnesses \
         answer the same way, and the hole opens on the case nobody wrote"
      );
      for (body, region) in [("judge", judged), ("recovery_step", stepped)] {
        assert_eq!(
          code_matches(region, witness),
          1,
          "`parser/recovery_gate.rs`: `{body}` must consult all three input-side witnesses before \
           its verdict — `{witness}` is missing from the code ahead of it. Four of five is the \
           state the four recovery sites were in, and the missing one was the scanner"
        );
      }
    }
  }

  /// Both baselines are the chokepoint's, one pair per attempt, and the speculating runner judges
  /// inside its attempt.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn the_chokepoint_takes_its_baselines_per_attempt() {
    let prod = gate_production();
    let judged = guard_region(JUDGE_DEF, VERDICT);
    let stepped = guard_region(STEP_DEF, STEP_VERDICT);
    for baseline in BASELINES {
      assert_eq!(
        code_matches(prod, baseline),
        3,
        "`parser/recovery_gate.rs`: exactly one `{baseline}` per runner — the speculating attempt, \
         the in-place attempt and the side step. Every one is per REGION: both counters are \
         monotone session facts and the latch is not cleared by anything a caller controls, so a \
         baseline shared across regions refuses everything after the parse's first stop, ordinary \
         syntax errors included"
      );
      assert_eq!(
        code_matches(judged, baseline),
        0,
        "`parser/recovery_gate.rs`: `judge` receives its baselines, it does not take them — one \
         taken after the attempt has already started measures nothing"
      );
      assert_eq!(
        code_matches(stepped, baseline),
        1,
        "`parser/recovery_gate.rs`: `recovery_step` is its own runner — it has no wrapper to \
         receive `{baseline}` from, so it takes it, and takes it BEFORE the step runs"
      );
    }
    assert_eq!(
      code_matches(prod, WRAPPED),
      1,
      "`parser/recovery_gate.rs`: the speculating runner must judge INSIDE its attempt \
       (`{WRAPPED}`). `try_attempt`'s `Err` arm restores the poison boundary along with the \
       position, so a witness read after it compares a restored latch against the value it was \
       restored to — always equal, always false, a dead term. Measured: with the read moved after \
       the attempt, only the in-place cell of `recovery_terminal_stop.rs` passed"
    );
  }
}
