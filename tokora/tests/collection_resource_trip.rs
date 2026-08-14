#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! A **descent** budget trip inside a collection element re-raises; it is not spent as a diagnostic.
//!
//! The four try-driven collection families — `repeated`, `separated`, and their delimited forms —
//! swallow an element's `Err` by design: emit it and keep looping. Their gate re-raises what no
//! further input can clear, and until issue #148 it recognized only two of the three such
//! conditions. The third, a [`RecursionLimitReached`], latches no boundary — it has a control stack
//! rather than a position — so `inp.at_committed_boundary()` reads `false` for one and the trip fell
//! through to the swallow arm: emitted as an ordinary diagnostic, with the loop continuing to the
//! next element. The gate now also reads the session's **resource-trip counter**, which the trip arm
//! bumps before the grammar's `From` runs.
//!
//! # Attempt-relative, and why half this file is about that
//!
//! The counter is a **monotone session fact**: it says a budget was exceeded somewhere in this
//! parse, and it is never cleared, because nothing can un-exceed a budget. That is not the question
//! the gate has. The gate is judging **one element**, so it snapshots the counter before the element
//! runs (`inp.trip_snapshot()`) and re-raises only when the count moved *during* it
//! (`inp.tripped_during_attempt(..)`) — the same baseline-and-compare shape the sibling absence
//! witness already uses through `latch_snapshot`/`latched_during_attempt`.
//!
//! Read absolutely the two answers come apart exactly where grammar code catches a trip itself and
//! parses on: the session has tripped forever after, so every later element failure in every later
//! collection re-raises, ordinary syntax errors included. One deeply-nested expression early in a
//! document would then suppress every diagnostic after it — which is precisely wrong for the
//! editor and language-server consumers this crate is built for. Section 2 below is that regression;
//! section 3 pins that narrowing the witness did not put a hole in it.
//!
//! # The exit with no `Err` in it — section 4
//!
//! Everything above is about the `Err` an element hands back. An element that **answers the trip
//! itself** hands back `Ok`, and the gate never runs: the driver takes an *absence* exit — the
//! element declining, or a cycle that committed nothing — reads it as "no more elements", and the
//! collection **succeeds**. Same defect, same sink-independence, reached through the exit the
//! failure chokepoint does not cover; the resource budget stopped the parse and the parse reported
//! a complete construct. `parser::many::absence_after_element` is the second chokepoint, and
//! section 4 drives both absence exits of all four families through it. Its cells are the inverse
//! of section 2's: there a caught trip must change nothing, here it must change the verdict, and
//! the two sets of sources differ only in *where* the catch sits relative to the element the driver
//! is judging.
//!
//! # The exit that closes on a real token — section 5
//!
//! Section 4's delimited sources all end on a close MISS, so they never reach the arm where the
//! closer is genuinely present. There the two witnesses come apart. A `CloseStatus::Close` verdict
//! is cache-first, so it rests on a real pre-trip token: the construct ended **ahead of** any
//! boundary the element's lookahead went on to latch, which makes the *scanner* latch simply not
//! about it — gating on that would fail a parse a wider scan window completes to the identical
//! value. The *descent* trip is the other kind of fact, a counter event that already happened inside
//! the element attempt, and no later token unmakes it: an element that caught one, declined, and was
//! then followed by a real closer closed the collection **successfully** while a resource budget had
//! stopped the parse. `parser::many::close_after_element` is the third chokepoint, holding the
//! descent witness alone, and section 5 is section 4's delimited cells with the closer put back.
//!
//! # The exit that stays open, by design — section 6
//!
//! Sections 4 and 5 are both about an element that *answers* a trip and then concludes absence —
//! declining, or stalling — which is a conclusion the driver draws and therefore a conclusion the
//! driver has to guard. An element that answers a trip and reports `Accept` concludes nothing: it
//! hands the driver a value, and the driver collects it exactly as it would from an untripped
//! element. Neither `absence_after_element` nor `close_after_element` is reached on that path, on
//! purpose, and section 6 is **not** a residual — it is the permanent shape of the design, stated
//! by `parser::many`'s module docs and pinned here so it cannot drift silently in either direction.
//! Section 7 is the contrast that makes the distinction legible: the gap it closes *was* a residual,
//! recorded as one, and is now gone.
//!
//! # The other eight drivers — section 7
//!
//! Sections 4 and 5 cover the four try-driven families. The other eight guard-bearing sources — the
//! four `*_while` collection drivers and the four fold sources — had the same hole, for a reason
//! narrower than the four's rather than different: they never file an element's `Err`, so a trip an
//! element *hands back* propagates untouched and is terminal there with no gate at all. What they
//! could not see is the same `Ok` sections 4 and 5 are about. Closing it needed the per-element trip
//! baseline none of those loops took — which is why three of the folds stopped being `while let`
//! loops, a `while let` having nowhere to snapshot the counter before the element attempt its own
//! condition runs. Section 7 drives every one of the eight.
//!
//! **THIS SECTION PINS DELIBERATE, EXISTING BEHAVIOUR.** Every cell in it is expected to pass
//! against the code as it stands; a red here does not mean something is broken; it means the
//! boundary moved — either narrower (an `Accept` started being gated, which would strand a grammar
//! that deliberately recovers a value from a caught budget) or wider (a decline/stall/closer exit
//! sections 4 or 5 gate stopped being gated, and this section's own control failed to notice). If
//! this section goes red, the fix is almost never in this file — see `parser::many`'s module docs
//! for the reasoning the change would have to revisit.
//!
//! # Why every cell here runs twice, through two error types
//!
//! This is the half of #148 that was **never sink-dependent**. `Recover`, `InplaceRecover` and
//! `skip_then_retry` used to spend a trip only through a *discarding* error type — a delegating one
//! reported `is_terminal()` and was re-raised — which is why
//! `tokora/tests/pratt_limit_unit_sink.rs` pins those against their `LimErr` counterparts in
//! `tokora/tests/pratt_limit.rs`, one file per sink. Here the swallow arm consulted neither the
//! error's answer nor the input's, so it spent the trip from [`TripErr`] — which keeps the payload —
//! exactly as it did from `()`. Each family is therefore driven through both, in the same file, from
//! one macro body: the two must now agree, and the agreement is the property.
//!
//! [`TripErr`] deliberately does **not** implement
//! [`MaybeTerminal`](tokora::error::MaybeTerminal). That it compiles is itself an assertion: these
//! four gates witness terminality positionally and through the session, never through a trait bound
//! on the emitter's error type, and closing this hole did not change that.
//!
//! # Non-vacuity
//!
//! Through `()` a re-raised trip is `Err(())`, which carries no discriminant, so a fixture that
//! stopped reaching the ladder at all — a source that stopped lexing, an element that stopped being
//! called — reads exactly like the behaviour under test. Every cell therefore runs its probe a
//! second time with **only the recursion budget changed** and requires the same source to parse and
//! collect. `Verbose` is the emitter throughout rather than `Fatal`, for the same reason: a fatal
//! sink turns the swallow arm's own `emit_error` into an `Err` and would report a green cell over a
//! wide-open gate.
//!
//! # No scanner limiter anywhere in this file
//!
//! The lexer has no `State`, so no scan can trip and no poison boundary can ever be latched, which
//! pins `inp.at_committed_boundary()` to `false` for the whole run. The older witness therefore
//! cannot contribute to any verdict below: what these cells measure is the session latch alone.
//! `tokora/tests/collection_terminal_stop.rs` is the other side of that split — the same four
//! families under a *scanner* limiter and no descent at all.

mod common;

use core::cell::Cell;
use tokora::EmitterView;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  TryParseInput,
  cache::Peeked,
  emitter::{
    FromUnclosed, FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnclosedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
    Verbose,
  },
  error::{
    RecursionLimitReached, Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
  punct::Paren,
  state::recursion_tracker::RecursionLimiter,
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

// ── The delegating sink ───────────────────────────────────────────────────────

/// A grammar error that **keeps** the descent trip apart from everything else.
///
/// The counterpart of the `()` suite below: `()` erases the trip on conversion, this preserves it as
/// its own variant, and the point of the fix is that the two now behave identically at these four
/// gates. It is the shape a real grammar would write, minus the payload — the tests read the
/// discriminant, never the offset or the depth.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TripErr {
  /// Anything the drivers report as ordinary malformed input.
  Ordinary,
  /// The descent budget's trip.
  Depth,
}

impl From<RecursionLimitReached<usize, ()>> for TripErr {
  fn from(_: RecursionLimitReached<usize, ()>) -> Self {
    TripErr::Depth
  }
}

// The lexer's own error type is `()`; every other family below is ordinary malformed input.
impl From<()> for TripErr {
  fn from((): ()) -> Self {
    TripErr::Ordinary
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for TripErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<Set: Clone + 'static> From<UnexpectedEot<usize, (), Set>> for TripErr {
  fn from(_: UnexpectedEot<usize, (), Set>) -> Self {
    TripErr::Ordinary
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for TripErr {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for TripErr {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for TripErr {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for TripErr {
  fn from(_: FullContainer<S, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for TripErr {
  fn from(_: TooFew<S, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for TripErr {
  fn from(_: TooMany<S, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unclosed<Delimiter, S, Lang>> for TripErr {
  fn from(_: Unclosed<Delimiter, S, Lang>) -> Self {
    TripErr::Ordinary
  }
}

impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for TripErr
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<Delimiter>(_: Unclosed<Delimiter, L::Span, Lang>) -> Self {
    TripErr::Ordinary
  }
}

// ── Shared parameters ─────────────────────────────────────────────────────────

/// Levels the element descends after committing its token. Comfortably past `TIGHT` and far short
/// of `ROOMY`, so the same element trips under one budget and returns under the other.
const LADDER: usize = 24;

/// The budget the ladder exceeds.
const TIGHT: usize = 8;

/// The budget it does not — the non-vacuity control's only difference from the cell above it.
const ROOMY: usize = 4_000;

/// The number an ordinary element rejects as malformed input. Nothing terminal about it: no
/// descent, no budget, no scanner stop — the whole point is that it is the boring kind of failure a
/// collection is supposed to emit and loop past.
const BAD: i64 = 9;

/// The value section 4's stalling element accepts without consuming anything.
///
/// Nothing in any source lexes to it, so a container that ends with it says unambiguously that the
/// zero-width element ran and the driver's no-progress guard is what ended the collection — which
/// is the exit that cell is about.
const ZERO_WIDTH: i64 = -1;

thread_local! {
  /// How many times [`ladder`](trip_suite) actually tripped, on this test's thread.
  ///
  /// The non-vacuity control for every cell in sections 2 and 3: they assert that a *caught* trip
  /// leaves the collections behaving exactly as an untripped parse does, and a fixture that quietly
  /// stopped tripping would satisfy that by satisfying nothing. Each cell requires this to be
  /// nonzero on the tight-budget run and zero on its widened control, so "the two runs agree" is
  /// only ever asserted between a run that tripped and a run that did not.
  ///
  /// Thread-local rather than a `static`: libtest runs each `#[test]` on its own thread, so the
  /// two sink suites cannot see each other's count.
  static TRIPS: Cell<usize> = const { Cell::new(0) };
}

fn note_trip() {
  TRIPS.with(|c| c.set(c.get() + 1));
}

fn trips() -> usize {
  TRIPS.with(Cell::get)
}

fn reset_trips() {
  TRIPS.with(|c| c.set(0));
}

/// The diagnostics an emitter actually filed. Zero on every trip cell: the gate re-raises *before*
/// the swallow arm reaches `emit_error`, so a re-raised trip leaves the log untouched. This is the
/// assertion that distinguishes "re-raised" from "emitted, and then the parse failed for some other
/// reason".
fn filed<E>(log: &Verbose<E>) -> usize {
  log.errors().values().map(|group| group.len()).sum()
}

// ── The suite, instantiated once per error type ───────────────────────────────

/// One family cell: a trip run and its non-vacuity control.
///
/// The probe is named here rather than passed as a value on purpose. A `fn`-pointer parameter over
/// `&mut InputRef<'_, '_, TestLexer<'_>, ParserContext<'_, _, &mut Verbose<_>>>` elides into a
/// higher-ranked bound that asks for `Lexer<'a>` at *every* pair of lifetimes, which
/// `LogosLexer<'a, Token>` cannot satisfy; naming the generic probe at the call site instantiates it
/// at the one context type actually in play.
macro_rules! cell {
  ($name:ident, $err:ty, $trip:expr, $sink:literal, $probe:ident, $family:literal, $src:literal) => {
    /// A descent trip inside this family's element re-raises rather than being emitted and looped
    /// past, and the widened-budget control proves the budget is what did it.
    #[test]
    fn $name() {
      let mut log = Verbose::<$err>::new();
      let tripped = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(TIGHT));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      assert_eq!(
        tripped,
        Err($trip),
        concat!(
          $family,
          " over ",
          $sink,
          ": an element's descent trip must re-raise, not be spent as a diagnostic and looped \
           past. Before #148 this collected an empty container and filed the trip among its \
           diagnostics — for this sink exactly as for the other one"
        )
      );
      assert_eq!(
        filed(&log),
        0,
        concat!(
          $family,
          " over ",
          $sink,
          ": the gate re-raises before the swallow arm emits, so a tripped collection files nothing"
        )
      );

      // Non-vacuity: same probe, same source, only the recursion budget changed.
      let mut log = Verbose::<$err>::new();
      let roomy = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(ROOMY));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      assert_eq!(
        roomy,
        Ok(vec![1, 2, 3]),
        concat!(
          $family,
          " over ",
          $sink,
          ": the budget is what failed the run above — with room the same source parses and \
           collects"
        )
      );
      assert_eq!(
        filed(&log),
        0,
        concat!(
          $family,
          " over ",
          $sink,
          ": the control is a clean parse, so it files nothing either"
        )
      );
    }
  };
}

/// One attempt-relative cell: a **caught** trip, and the collection that runs after it.
///
/// The property is that the caught trip changes *nothing* — so the cell runs the identical probe
/// over the identical source twice, once under a budget the ladder exceeds and once under one it
/// does not, and requires the two runs to agree on both the value and the diagnostic count. That
/// formulation is what lets these cells cover the separated families without predicting how many
/// separator-state diagnostics a failed element leaves behind: whatever that number is, the trip may
/// not change it.
///
/// Non-vacuity has two halves here, because "the two runs agree" is satisfiable by two runs that
/// both did nothing:
///
/// * the tight run must actually have tripped and the control must actually not have
///   ([`TRIPS`]) — otherwise the cell is comparing two untripped parses;
/// * the control's value is asserted **absolutely**, so a fixture that stopped reaching the
///   collection, or one whose element stopped failing, fails here rather than agreeing quietly.
macro_rules! attempt_cell {
  (
    $name:ident, $err:ty, $sink:literal, $probe:ident, $what:literal, $src:literal,
    collects = $collects:expr, tripped = $tripped:expr
  ) => {
    #[doc = concat!("Attempt-relative: ", $what)]
    #[test]
    fn $name() {
      reset_trips();
      let mut tight_log = Verbose::<$err>::new();
      let tight = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut tight_log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(TIGHT));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      let tight_filed = filed(&tight_log);
      assert_eq!(
        trips(),
        $tripped,
        concat!(
          $what,
          " over ",
          $sink,
          ": the tight run must really have tripped the budget — without that this cell compares \
           two untripped parses and asserts nothing"
        )
      );

      reset_trips();
      let mut roomy_log = Verbose::<$err>::new();
      let roomy = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut roomy_log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(ROOMY));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      let roomy_filed = filed(&roomy_log);
      assert_eq!(
        trips(),
        0,
        concat!(
          $what,
          " over ",
          $sink,
          ": the control differs from the run above in one thing only — with room, nothing trips"
        )
      );
      assert_eq!(
        roomy,
        $collects,
        concat!(
          $what,
          " over ",
          $sink,
          ": the control pins the fixture absolutely — the collection is reached, the element fails \
           where it is meant to, and the driver emits and loops past it"
        )
      );

      assert_eq!(
        tight, roomy,
        concat!(
          $what,
          " over ",
          $sink,
          ": a trip the grammar caught and parsed past must change nothing here. The session \
           counter is monotone and stays raised for the rest of the parse; what the gate tests is \
           whether it moved during THIS element. Reading it absolutely re-raises every later \
           element failure in the document, ordinary syntax errors included"
        )
      );
      assert_eq!(
        tight_filed, roomy_filed,
        concat!(
          $what,
          " over ",
          $sink,
          ": and the diagnostics are the same ones — a caught trip must not silence the collection \
           either"
        )
      );
    }
  };
}

/// One **absence-exit** cell: an element that answers a trip itself and then reports *no more
/// elements*, over a source where the driver would otherwise end the collection cleanly.
///
/// The shape is [`attempt_cell`]'s with the verdict inverted. There the caught trip must change
/// nothing; here it must change everything, because the absence conclusion the driver is about to
/// draw is one the trip produced. So the two runs are required to **disagree** — and the same two
/// non-vacuity halves apply, for the same reason:
///
/// * the tight run must actually have tripped and the control must actually not have ([`TRIPS`]);
/// * the control's value is asserted **absolutely**, so a fixture that stopped reaching the exit —
///   an element that stopped being called, a source that stopped lexing — fails here rather than
///   producing an `Err` for the wrong reason and reading as a pass. Through `()` that matters twice
///   over: `Err(())` carries no discriminant, so *every* way of failing looks identical.
///
/// The filed counts are pinned on both runs rather than compared, because they need not agree: the
/// gate returns **before** the exit's own diagnostic, so wherever the control emits one (a close
/// miss, an `Unclosed`) the stopped run files strictly less. Section 5's cells close on a real token
/// and so emit nothing on either run, which the same absolute pins state rather than assume.
macro_rules! absence_cell {
  (
    $name:ident, $err:ty, $sink:literal, $probe:ident, $what:literal, $src:literal,
    stops = $stops:expr, tight_filed = $tight_filed:expr,
    collects = $collects:expr, roomy_filed = $roomy_filed:expr
  ) => {
    #[doc = concat!("An element that catches a trip and then reports absence: ", $what)]
    #[test]
    fn $name() {
      reset_trips();
      let mut tight_log = Verbose::<$err>::new();
      let tight = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut tight_log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(TIGHT));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      let tight_filed = filed(&tight_log);
      assert_eq!(
        trips(),
        1,
        concat!(
          $what,
          " over ",
          $sink,
          ": the tight run must really have tripped the budget, exactly once, in the element that \
           then reported absence — without that this cell measures nothing"
        )
      );

      reset_trips();
      let mut roomy_log = Verbose::<$err>::new();
      let roomy = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut roomy_log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(ROOMY));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      assert_eq!(
        trips(),
        0,
        concat!(
          $what,
          " over ",
          $sink,
          ": the control differs from the run above in one thing only — with room, nothing trips"
        )
      );
      assert_eq!(
        (&roomy, filed(&roomy_log)),
        (&$collects, $roomy_filed),
        concat!(
          $what,
          " over ",
          $sink,
          ": the control pins the fixture absolutely — the collection is reached, the element \
           reports absence where it is meant to, and the driver ends the construct and succeeds"
        )
      );

      assert_eq!(
        (&tight, tight_filed),
        (&Err($stops), $tight_filed),
        concat!(
          $what,
          " over ",
          $sink,
          ": a budget stop the element answered ITSELF must not be spendable as an accepted \
           absence. The element caught the trip and reported `no more elements`, so the driver \
           concluded the construct ended and returned `Ok` — a resource stop turned into a \
           success, which is the same defect the element-failure chokepoint closes, reached \
           through the exit it does not cover"
        )
      );
      assert_ne!(
        tight, roomy,
        concat!(
          $what,
          " over ",
          $sink,
          ": and the two runs must DISAGREE. The paired `a_caught_trip_leaves_*_emitting` cells are \
           the contrast: there the catch sits outside the element the driver is judging and the two \
           runs must agree exactly"
        )
      );
    }
  };
}

/// One **accept-exemption** cell: an element that catches a trip, consumes, and still answers
/// `Accept`, followed by a real closer.
///
/// PINS DELIBERATE, EXISTING BEHAVIOUR — see section 6's module note above. It is
/// [`absence_cell`]'s deliberate mirror, not its twin: there, a caught trip must flip the verdict,
/// because the driver was about to manufacture "no more elements" out of a stop the caller never
/// learns about, and the whole point of sections 4 and 5 is that it no longer may. Here the element
/// does not report absence — it hands the driver a value — so there is nothing for either
/// chokepoint to refuse: the value is what the grammar produced, and the driver is faithfully
/// collecting it. The very next cycle's own trip baseline is taken *after* the accepting cycle's
/// trip, so by the time a real closer reaches a gate, the trip the accepting element caught is
/// already outside the window being judged. So, unlike [`absence_cell`], the two runs here are
/// required to **agree**: the collection succeeds identically whether or not the budget actually
/// tripped, which is the same shape [`attempt_cell`] uses for a trip caught outside the element the
/// driver is judging — this is that shape, aimed at the one exit sections 4 and 5 deliberately
/// leave alone.
///
/// The non-vacuity halves are the same ones every cell in this file needs, for the same reasons:
///
/// * the tight run must actually have tripped, exactly once, in the accepting element
///   ([`TRIPS`]) — otherwise the cell is comparing two untripped parses;
/// * the control's value and filed count are asserted **absolutely**, so a fixture that stopped
///   reaching the accepting element, or one that stopped consuming, fails here rather than
///   producing a result that happens to match by accident.
macro_rules! accept_exemption_cell {
  (
    $name:ident, $err:ty, $sink:literal, $probe:ident, $what:literal, $src:literal,
    collects = $collects:expr
  ) => {
    #[doc = concat!(
      "PINS DELIBERATE, EXISTING BEHAVIOUR (not a regression witness — see section 6's module \
       note): ",
      $what
    )]
    #[test]
    fn $name() {
      reset_trips();
      let mut tight_log = Verbose::<$err>::new();
      let tight = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut tight_log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(TIGHT));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      let tight_filed = filed(&tight_log);
      assert_eq!(
        trips(),
        1,
        concat!(
          $what,
          " over ",
          $sink,
          ": the tight run must really have tripped the budget, exactly once, in the element that \
           accepted through it — without that this cell measures nothing"
        )
      );

      reset_trips();
      let mut roomy_log = Verbose::<$err>::new();
      let roomy = {
        let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
          ParserContext::new(&mut roomy_log);
        let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(ROOMY));
        Parser::with_context(ctx).apply($probe).parse_str($src)
      };
      let roomy_filed = filed(&roomy_log);
      assert_eq!(
        trips(),
        0,
        concat!(
          $what,
          " over ",
          $sink,
          ": the control differs from the run above in one thing only — with room, nothing trips"
        )
      );
      assert_eq!(
        (&roomy, roomy_filed),
        (&$collects, 0),
        concat!(
          $what,
          " over ",
          $sink,
          ": the control pins the fixture absolutely — the collection is reached, the element \
           accepts, and the real closer commits"
        )
      );

      assert_eq!(
        (&tight, tight_filed),
        (&$collects, 0),
        concat!(
          $what,
          " over ",
          $sink,
          ": DELIBERATE, per `parser::many`'s module docs — an element that catches a trip and \
           still answers `Accept` has produced a value, not concluded absence, so the driver keeps \
           it and closes on the real closer that follows exactly as the untripped control does. \
           Gating this would refuse a value the grammar legitimately produced, for every error a \
           grammar can catch and answer, not only this one. If this assertion ever fails, the \
           exemption moved — narrower if this now errors, wider if some decline/stall/closer exit \
           sections 4 or 5 gate stopped erroring and this cell's own tight/roomy disagreement (see \
           below) failed to catch it — and that is a design decision for a changelog entry, not a \
           bug this file should absorb silently"
        )
      );
      assert_eq!(
        tight, roomy,
        concat!(
          $what,
          " over ",
          $sink,
          ": and the two runs must AGREE — unlike `absence_cell`'s shapes, a trip an accepting \
           element answered changes nothing about whether the collection succeeds"
        )
      );
    }
  };
}

/// Generates the four family cells for one grammar error type.
///
/// A macro rather than a function generic over the error type: the drivers' bounds are stated per
/// family as `Error = ...` equality constraints, and spelling them once against a substituted `$err`
/// keeps each probe reading exactly like the grammar a user would write, with no higher-ranked
/// `From` bound collection standing between the test and what it measures.
///
/// `$eot` is what this sink's `From<UnexpectedEot<..>>` produces — the value section 4's absence
/// exits surface. It is a *separate* parameter from `$ordinary` even where the two are the same
/// value, because they are different claims: `$ordinary` is the grammar's own malformed-input
/// error, `$eot` is the terminal end-of-input the driver synthesizes when it refuses to conclude a
/// construct ended.
macro_rules! trip_suite {
  ($suite:ident, $err:ty, $trip:expr, $ordinary:expr, $eot:expr, $sink:literal) => {
    mod $suite {
      use super::*;

      /// The element: commit one `Num`, then descend `LADDER` levels on the shared budget.
      ///
      /// The token is committed **first**, so under `TIGHT` the pre-#148 loop had somewhere to go —
      /// each cycle consumed one number, emitted the trip, and passed the no-progress guard — and
      /// the swallow was a loop that ran to the end of the input rather than one that stalled
      /// immediately. It is also what makes the control meaningful: under `ROOMY` the very same
      /// element returns the number it consumed.
      fn deep_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        let n = match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => return Ok(ParseAttempt::Decline),
          Some(tok) => match tok.into_data() {
            Token::Num(n) => n,
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        };
        ladder(inp, LADDER)?;
        Ok(ParseAttempt::Accept(n))
      }

      /// `left + 1` nested frames on the parse's shared depth budget, released on the way out.
      ///
      /// The trip is counted on the way out — exactly once per tripping call, since the frames above
      /// it only propagate. [`TRIPS`] is what every attempt-relative cell's non-vacuity control
      /// reads; the cells in section 1 do not consult it, so counting here costs them nothing.
      fn ladder<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
        left: usize,
      ) -> Result<(), $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        let mut frame = inp.descend().inspect_err(|_| note_trip())?;
        let inp = &mut *frame;
        match left {
          0 => Ok(()),
          n => ladder(inp, n - 1),
        }
      }

      // ── Section 2 and 3 material: a caught trip, and ordinary failures after it ──

      /// **Grammar code that catches the trip itself and keeps parsing** — the first of the two
      /// shapes the session counter's own documentation names, and the one this file's section 2
      /// is about. Under `ROOMY` the same call returns `Ok` and the session never trips at all,
      /// which is what makes the widened-budget run a control rather than a second copy.
      fn catch_a_trip<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>)
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        let _ = ladder(inp, LADDER);
      }

      /// An element that commits one `Num` and then fails **ordinarily** on [`BAD`]: no descent, no
      /// budget, no scanner stop. Committing first is what lets the swallow arm make progress, so
      /// the driver reaches the elements after it.
      fn ordinary_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        let n = match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => return Ok(ParseAttempt::Decline),
          Some(tok) => match tok.into_data() {
            Token::Num(n) => n,
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        };
        match n {
          BAD => Err($ordinary),
          n => Ok(ParseAttempt::Accept(n)),
        }
      }

      /// An element that catches a trip **itself** and then fails ordinarily — both inside the one
      /// element attempt the driver's gate is judging.
      ///
      /// The trip is taken only on the [`BAD`] element, so the parse's *only* trip and the ordinary
      /// failure the driver sees belong to the same element. That is what makes the paired cell
      /// below a statement about the granularity floor rather than about leakage from an earlier
      /// element, which the per-element baseline already rules out.
      fn caught_then_ordinary_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        let n = match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => return Ok(ParseAttempt::Decline),
          Some(tok) => match tok.into_data() {
            Token::Num(n) => n,
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        };
        if n == BAD {
          catch_a_trip(inp);
          return Err($ordinary);
        }
        Ok(ParseAttempt::Accept(n))
      }

      /// **An element that catches the trip itself and then reports `Decline`** — section 4's
      /// subject, and the shape the element-failure chokepoint cannot see: there is no `Err` for it
      /// to judge.
      ///
      /// The catch sits on the declining path only, so the parse's one trip and the absence
      /// conclusion the driver draws from it belong to the same element attempt. The numbers ahead
      /// of it parse normally, which is what makes the widened-budget control a *collection* rather
      /// than an empty one.
      fn caught_then_decline_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => {
            catch_a_trip(inp);
            Ok(ParseAttempt::Decline)
          }
          Some(tok) => match tok.into_data() {
            Token::Num(n) => Ok(ParseAttempt::Accept(n)),
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        }
      }

      /// **The same catch, on an element that CONSUMES the token at the slot and then declines** —
      /// section 5's `separated().delimited()` element.
      ///
      /// The delimited separated driver probes the separator-or-close slot *before* it attempts an
      /// element, so a closer sitting there is committed by that probe and the element never runs.
      /// The epilogue's close probe therefore sees a real closer only when the element attempt moved
      /// the front onto it — which means the element consumed. That is the shape
      /// `tokora/tests/probe_close_no_rescan.rs`'s `consume_then_decline` already uses to reach the
      /// same arm, for the same reason.
      ///
      /// The catch sits on the consuming path only, so the parse's one trip and the absence
      /// conclusion the driver draws from it belong to the same element attempt.
      fn caught_then_consuming_decline_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => {
            // Take the non-element token out of the way, so the closer is what the epilogue's
            // close probe classifies. Then catch the trip and report no more elements.
            let _ = inp.try_expect(|t| matches!(t.data(), Token::Plus))?;
            catch_a_trip(inp);
            Ok(ParseAttempt::Decline)
          }
          Some(tok) => match tok.into_data() {
            Token::Num(n) => Ok(ParseAttempt::Accept(n)),
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        }
      }

      /// The same catch, answered with a **zero-width `Accept`** instead of a decline — the driver's
      /// *other* absence exit, the no-progress stall.
      ///
      /// `Accept` is the arm no gate consults, deliberately: an element that catches a trip and
      /// still produces a value has answered it. What the driver may not do is read the *absence of
      /// progress* that follows as "no more elements" — that conclusion is the trip's, not the
      /// input's, and it reaches a different gate line in every driver than the decline does.
      fn caught_then_zero_width_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => {
            catch_a_trip(inp);
            Ok(ParseAttempt::Accept(ZERO_WIDTH))
          }
          Some(tok) => match tok.into_data() {
            Token::Num(n) => Ok(ParseAttempt::Accept(n)),
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        }
      }

      /// Section 7's element for the six drivers whose element is a plain
      /// [`ParseInput`](tokora::ParseInput): the same catch as
      /// [`caught_then_zero_width_num`], answered with a **value** rather than a
      /// [`ParseAttempt`].
      ///
      /// The `*_while` collection drivers and the `*_while` folds take their element through
      /// `parse_input`, which has no decline channel at all — the *only* way an element of theirs
      /// can report absence is by returning a value while consuming nothing, which the driver's
      /// no-progress guard then reads as "no more elements". So this is the one shape section 7 can
      /// drive those six with, and it is the shape the measurement in `parser::many`'s changelog
      /// entry used.
      fn caught_then_zero_width_plain<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => {
            catch_a_trip(inp);
            Ok(ZERO_WIDTH)
          }
          Some(tok) => match tok.into_data() {
            Token::Num(n) => Ok(n),
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        }
      }

      /// The decision every section 7 `*_while` probe drives its driver with: **always continue**.
      ///
      /// A condition that stops on a non-`Num` front would take the driver's *`Action::Stop`* exit,
      /// which sits at the TOP of a cycle — before that cycle's element has run — so the element
      /// that caught the trip is the *previous* cycle's, an accepting one, and the exemption the
      /// module docs state for `Accept` applies. That exit is therefore not where the defect lives
      /// and gating it would contradict section 6. Continuing unconditionally hands the cycle to the
      /// element, which catches the trip and consumes nothing, and the driver's **no-progress
      /// stall** — an absence conclusion drawn after that very attempt — is what ends the
      /// collection.
      fn always_continue<'inp, Ctx>(
        _peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
        _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
      ) -> Result<Action, <Ctx::Emitter as Emitter<'inp, TestLexer<'inp>>>::Error>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
      {
        Ok(Action::Continue)
      }

      /// Section 6's subject: the same catch, answered with a **consuming `Accept`** — the one arm
      /// no chokepoint in `parser::many` ever inspects, by design. When it finds a `Num` it catches
      /// the trip and still accepts, producing a value rather than concluding absence; when it does
      /// not, it consumes a stray `Plus` out of the way (a harmless no-op where none is there) and
      /// declines untouched by any catch, so the cycle that lets the driver conclude the construct
      /// ended is never the one that tripped. Reused across `repeated().delimited()` — where the
      /// closer sits directly at the next slot — and `separated_by_comma().delimited()`, where the
      /// consumed `Plus` is what routes the real closer to the epilogue's own gate
      /// (`parser::many::close_after_element`) rather than the mid-scan arm, which is a DIRECT
      /// closer the census exempts because only an accepting element can precede it — the same
      /// routing `caught_then_consuming_decline_num` above uses for section 5's separated cell.
      fn caught_then_consuming_accept_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
          None => {
            let _ = inp.try_expect(|t| matches!(t.data(), Token::Plus))?;
            Ok(ParseAttempt::Decline)
          }
          Some(tok) => match tok.into_data() {
            Token::Num(n) => {
              catch_a_trip(inp);
              Ok(ParseAttempt::Accept(n))
            }
            _ => unreachable!("the predicate accepted only `Num`"),
          },
        }
      }

      /// The inner collection's element: one `Plus`, then a descent that trips and is **caught**
      /// there. The inner collection therefore returns `Ok` with the session counter already raised
      /// — a trip that belongs to no element the *enclosing* driver is judging.
      fn catching_plus<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        if inp
          .try_expect(|t| matches!(t.data(), Token::Plus))?
          .is_none()
        {
          return Ok(ParseAttempt::Decline);
        }
        let _ = ladder(inp, LADDER);
        Ok(ParseAttempt::Accept(0))
      }

      /// The **enclosing** collection's element: an inner `repeated()` of [`catching_plus`], then
      /// one number that may fail ordinarily. On the first element the inner collection consumes the
      /// `+` and trips inside it; on every element after that the inner collection is empty, so the
      /// only thing that can fail is the number.
      fn plusses_then_num<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<ParseAttempt<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        let _inner: Vec<i64> = catching_plus.repeated().collect().parse_input(inp)?;
        ordinary_num(inp)
      }

      // ── The probes for sections 2 and 3 ─────────────────────────────────────

      fn caught_trip_then_repeated<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        catch_a_trip(inp);
        ordinary_num.repeated().collect().parse_input(inp)
      }

      fn caught_trip_then_delim_repeated<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>,
      {
        catch_a_trip(inp);
        ordinary_num
          .repeated()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      fn caught_trip_then_sep<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        catch_a_trip(inp);
        ordinary_num.separated_by_comma().collect().parse_input(inp)
      }

      fn caught_trip_then_sep_delim<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        catch_a_trip(inp);
        ordinary_num
          .separated_by_comma()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      /// Section 3: the protection this narrowing must not remove. The budget is tripped, caught and
      /// parsed past — and then a *second* trip happens, inside a collection element this time.
      fn caught_trip_then_a_second_one<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        catch_a_trip(inp);
        deep_num.repeated().collect().parse_input(inp)
      }

      fn caught_trip_inside_one_element<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_ordinary_num.repeated().collect().parse_input(inp)
      }

      /// Section 2, one nesting level up: the trip happens inside an *inner* collection, during the
      /// enclosing collection's first element, and is swallowed there.
      fn nested_collections<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        plusses_then_num.repeated().collect().parse_input(inp)
      }

      // ── The four probes ─────────────────────────────────────────────────────

      fn repeated_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        deep_num.repeated().collect().parse_input(inp)
      }

      fn delim_repeated_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>,
      {
        deep_num
          .repeated()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      fn sep_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        deep_num.separated_by_comma().collect().parse_input(inp)
      }

      fn sep_delim_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        deep_num
          .separated_by_comma()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      // ── Section 4's eight probes: the same four families, two absence exits each ────

      fn repeated_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_decline_num.repeated().collect().parse_input(inp)
      }

      fn repeated_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_num
          .repeated()
          .collect()
          .parse_input(inp)
      }

      fn delim_repeated_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_decline_num
          .repeated()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      fn delim_repeated_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_num
          .repeated()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      fn sep_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_decline_num
          .separated_by_comma()
          .collect()
          .parse_input(inp)
      }

      fn sep_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_num
          .separated_by_comma()
          .collect()
          .parse_input(inp)
      }

      fn sep_delim_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_decline_num
          .separated_by_comma()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      fn sep_delim_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_num
          .separated_by_comma()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      /// Section 5's `separated().delimited()` probe: the consuming decline, so the epilogue's close
      /// probe classifies a REAL closer rather than the token the element left behind.
      fn sep_delim_consuming_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_consuming_decline_num
          .separated_by_comma()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      /// Section 6's `repeated().delimited()` probe: the element accepts through a caught trip, and
      /// the very next cycle's own attempt is the plain decline that reaches the real closer.
      fn delim_repeated_accept_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_consuming_accept_num
          .repeated()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      /// Section 6's `separated_by_comma().delimited()` probe: the accepting cycle consumes the
      /// element, and the next cycle's consumed `Plus` routes the real closer to the epilogue's
      /// gate — the same routing `sep_delim_consuming_decline_probe` above uses.
      fn sep_delim_accept_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_consuming_accept_num
          .separated_by_comma()
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      // ── Section 7's probes: the four `*_while` collections and the eight folds ──

      fn repeated_while_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_plain
          .repeated_while::<_, U1>(always_continue::<Ctx>)
          .collect()
          .parse_input(inp)
      }

      fn delim_repeated_while_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_plain
          .repeated_while::<_, U1>(always_continue::<Ctx>)
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      fn sep_while_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_plain
          .separated_by_comma_while::<_, U1>(always_continue::<Ctx>)
          .collect()
          .parse_input(inp)
      }

      fn sep_while_delim_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        caught_then_zero_width_plain
          .separated_by_comma_while::<_, U1>(always_continue::<Ctx>)
          .delimited::<Paren<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      fn fold_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_decline_num
          .fold(|| 0i64, |acc, x| acc + x)
          .parse_input(inp)
      }

      fn fold_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_num
          .fold(|| 0i64, |acc, x| acc + x)
          .parse_input(inp)
      }

      fn try_fold_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_decline_num
          .try_fold(|| 0i64, |acc, x| Ok(acc + x))
          .parse_input(inp)
      }

      fn try_fold_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_num
          .try_fold(|| 0i64, |acc, x| Ok(acc + x))
          .parse_input(inp)
      }

      fn try_fold_with_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_decline_num
          .try_fold_with(|| 0i64, |acc, x, _state| Ok(acc + x))
          .parse_input(inp)
      }

      fn try_fold_with_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_num
          .try_fold_with(|| 0i64, |acc, x, _state| Ok(acc + x))
          .parse_input(inp)
      }

      fn rfold_decline_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_decline_num
          .rfold(|| 0i64, |acc, x| acc + x)
          .parse_input(inp)
      }

      fn rfold_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_num
          .rfold(|| 0i64, |acc, x| acc + x)
          .parse_input(inp)
      }

      fn fold_while_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_plain
          .fold_while::<_, _, _, U1>(always_continue::<Ctx>, || 0i64, |acc, x| acc + x)
          .parse_input(inp)
      }

      fn try_fold_while_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_plain
          .try_fold_while::<_, _, _, U1>(always_continue::<Ctx>, || 0i64, |acc, x| Ok(acc + x))
          .parse_input(inp)
      }

      fn try_fold_while_with_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_plain
          .try_fold_while_with::<_, _, _, U1>(
            always_continue::<Ctx>,
            || 0i64,
            |acc, x, _state| Ok(acc + x),
          )
          .parse_input(inp)
      }

      fn rfold_while_stall_probe<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<i64, $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        caught_then_zero_width_plain
          .rfold_while::<_, _, _, U1>(always_continue::<Ctx>, || 0i64, |acc, x| acc + x)
          .parse_input(inp)
      }

      // ── The four cells ──────────────────────────────────────────────────────

      cell!(
        repeated_re_raises_an_element_trip,
        $err,
        $trip,
        $sink,
        repeated_probe,
        "`repeated()`",
        "1 2 3"
      );

      cell!(
        delimited_repeated_re_raises_an_element_trip,
        $err,
        $trip,
        $sink,
        delim_repeated_probe,
        "`repeated().delimited()`",
        "( 1 2 3 )"
      );

      cell!(
        separated_re_raises_an_element_trip,
        $err,
        $trip,
        $sink,
        sep_probe,
        "`separated_by_comma()`",
        "1 , 2 , 3"
      );

      cell!(
        delimited_separated_re_raises_an_element_trip,
        $err,
        $trip,
        $sink,
        sep_delim_probe,
        "`separated_by_comma().delimited()`",
        "( 1 , 2 , 3 )"
      );

      // ── Section 2: a caught trip does not disable the collections after it ──
      //
      // Each cell parses one construct that trips and is caught by grammar code, and then drives one
      // family over an element that fails ORDINARILY. That failure is emitted and looped past, as it
      // is in a parse that never tripped — which is what the paired widened-budget run measures.
      //
      // These four are the regression witnesses for the attempt-relative comparison: against a
      // session-absolute reading each returns `Err` from the tight run while its control returns
      // `Ok`, and the equality below is what fails.

      attempt_cell!(
        a_caught_trip_leaves_repeated_emitting,
        $err,
        $sink,
        caught_trip_then_repeated,
        "`repeated()` after a caught trip",
        "1 9 3",
        collects = Ok(vec![1, 3]),
        tripped = 1
      );

      attempt_cell!(
        a_caught_trip_leaves_delimited_repeated_emitting,
        $err,
        $sink,
        caught_trip_then_delim_repeated,
        "`repeated().delimited()` after a caught trip",
        "( 1 9 3 )",
        collects = Ok(vec![1, 3]),
        tripped = 1
      );

      attempt_cell!(
        a_caught_trip_leaves_separated_emitting,
        $err,
        $sink,
        caught_trip_then_sep,
        "`separated_by_comma()` after a caught trip",
        "1 , 9 , 3",
        collects = Ok(vec![1, 3]),
        tripped = 1
      );

      attempt_cell!(
        a_caught_trip_leaves_delimited_separated_emitting,
        $err,
        $sink,
        caught_trip_then_sep_delim,
        "`separated_by_comma().delimited()` after a caught trip",
        "( 1 , 9 , 3 )",
        collects = Ok(vec![1, 3]),
        tripped = 1
      );

      /// One nesting level up: the trip happens inside an **inner** collection, during the enclosing
      /// collection's first element, and is swallowed there. The enclosing driver's later ordinary
      /// failure must not be charged with it.
      ///
      /// This is the cell that discriminates a per-ELEMENT baseline from a per-COLLECTION one. Both
      /// pass the four cells above, because there the trip happens before the driver starts and is
      /// therefore in either baseline; here it happens *inside the driver's own first element*, so a
      /// baseline taken once at the top of the driver has already been overtaken by the time the
      /// second element fails.
      #[allow(rustdoc::private_intra_doc_links)]
      mod nested {
        use super::*;

        attempt_cell!(
          an_inner_collections_trip_is_not_charged_to_the_enclosing_one,
          $err,
          $sink,
          nested_collections,
          "an enclosing `repeated()` over an inner `repeated()` that trips",
          "+ 1 9 3",
          collects = Ok(vec![1, 3]),
          tripped = 1
        );
      }

      // ── Section 2b: the granularity floor, pinned rather than closed ────────

      /// **A trip caught inside ONE element is charged to that element's ordinary failure.** The
      /// residual the four cells above do not reach, stated as behaviour instead of left to be
      /// discovered.
      ///
      /// `a_caught_trip_leaves_repeated_emitting` is this cell with one thing moved. There the
      /// catch happens *before* the collection starts, so the driver's per-element baseline already
      /// includes the trip and `"1 9 3"` collects `[1, 3]` with the `9` filed as a diagnostic. Here
      /// the catch happens *inside the element that then fails*, between the baseline this cycle
      /// took and the failure being judged — so the counter moved during the element and the
      /// ordinary error is re-raised. Same source, same element failure, same single trip: only
      /// where the catch sits.
      ///
      /// **The strong form — "re-raise only when this very error is the trip" — cannot be built
      /// here.** Deciding it means interrogating the error value, and the discarding `()` sink this
      /// file runs alongside has already thrown the trip away; an error type that discards it is
      /// the entire premise of #148 and the reason the witness lives on the input. So the floor is
      /// one element, and inside that unit the gate **fails closed**: an ordinary failure sharing
      /// its element with a caught trip is re-raised, never the reverse.
      ///
      /// Over the delegating sink the re-raised value is asserted to be the **ordinary** error and
      /// not the trip's, which is what makes "the wrong error was charged" the thing measured
      /// rather than merely "the parse failed". Over `()` the two are the same value, so that half
      /// of the assertion carries no information there — which is precisely why the file runs both.
      ///
      /// If the escape hatch (an explicit rebaseline by code that deliberately catches a trip) is
      /// ever built, this cell is re-blessed deliberately rather than drifting unobserved.
      #[allow(rustdoc::private_intra_doc_links)]
      mod same_element {
        use super::*;

        #[test]
        fn a_caught_trip_inside_one_element_re_raises_its_ordinary_failure() {
          reset_trips();
          let mut tight_log = Verbose::<$err>::new();
          let tight = {
            let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
              ParserContext::new(&mut tight_log);
            let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(TIGHT));
            Parser::with_context(ctx)
              .apply(caught_trip_inside_one_element)
              .parse_str("1 9 3")
          };
          let tight_filed = filed(&tight_log);
          let tight_trips = trips();

          reset_trips();
          let mut roomy_log = Verbose::<$err>::new();
          let roomy = {
            let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
              ParserContext::new(&mut roomy_log);
            let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(ROOMY));
            Parser::with_context(ctx)
              .apply(caught_trip_inside_one_element)
              .parse_str("1 9 3")
          };
          let roomy_filed = filed(&roomy_log);

          assert_eq!(
            tight_trips,
            1,
            concat!(
              "one element caught one trip over ",
              $sink,
              ": exactly one, and it is the failing element's — without that this cell is \
               comparing two untripped parses"
            )
          );
          assert_eq!(
            trips(),
            0,
            concat!(
              "one element caught one trip over ",
              $sink,
              ": the control differs in one thing only — with room, nothing trips"
            )
          );
          assert_eq!(
            (&roomy, roomy_filed),
            (&Ok(vec![1, 3]), 1),
            concat!(
              "one element caught one trip over ",
              $sink,
              ": the control pins the fixture absolutely — the element fails ordinarily on the \
               `9`, the driver files that one diagnostic and loops past it, and the collection \
               completes"
            )
          );

          assert_eq!(
            (&tight, tight_filed),
            (&Err($ordinary), 0),
            concat!(
              "one element caught one trip over ",
              $sink,
              ": the element's ORDINARY failure is re-raised and nothing is filed, because a trip \
               moved the counter during the same element. The witness proves that A trip happened \
               inside the element, not that THIS error is it — and it cannot prove the second \
               without reading a payload the discarding sink has thrown away. This is the \
               granularity floor: one element, failing closed"
            )
          );
          assert_ne!(
            tight,
            roomy,
            concat!(
              "one element caught one trip over ",
              $sink,
              ": and the two runs deliberately DISAGREE — the paired \
               `a_caught_trip_leaves_repeated_emitting` cell is the same fixture with the catch \
               moved outside the element, where they must agree. The contrast is the floor"
            )
          );
        }
      }

      // ── Section 3: and the protection is still there ────────────────────────

      /// Narrowing the witness must not put a hole in it: a trip in a *later* collection re-raises
      /// even though an earlier one already raised the session counter.
      ///
      /// This is the cell that requires the counter to be a **count**. A set-once `bool` snapshot
      /// compares equal to a baseline taken after the first, caught trip, so the second trip would
      /// read as "nothing happened during this element" and the collection would file it and loop
      /// past — the very behaviour section 1 closed, reintroduced by the fix for section 2.
      #[allow(rustdoc::private_intra_doc_links)]
      mod second_trip {
        use super::*;

        #[test]
        fn a_later_collections_own_trip_still_re_raises() {
          reset_trips();
          let mut log = Verbose::<$err>::new();
          let tripped = {
            let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
              ParserContext::new(&mut log);
            let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(TIGHT));
            Parser::with_context(ctx)
              .apply(caught_trip_then_a_second_one)
              .parse_str("1 2 3")
          };
          assert_eq!(
            tripped,
            Err($trip),
            concat!(
              "a second trip over ",
              $sink,
              ": the element's own trip re-raises, even though the session counter was already \
               raised by the caught one before the collection started"
            )
          );
          assert_eq!(
            filed(&log),
            0,
            concat!(
              "a second trip over ",
              $sink,
              ": the gate re-raises before the swallow arm emits, so nothing is filed"
            )
          );
          assert_eq!(
            trips(),
            2,
            concat!(
              "a second trip over ",
              $sink,
              ": both trips must really have happened — the caught one, then the element's own. \
               One would mean the fixture stopped exercising the second"
            )
          );

          // Non-vacuity: same probe, same source, only the recursion budget changed.
          reset_trips();
          let mut log = Verbose::<$err>::new();
          let roomy = {
            let ctx: ParserContext<'_, TestLexer<'_>, &mut Verbose<$err>> =
              ParserContext::new(&mut log);
            let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(ROOMY));
            Parser::with_context(ctx)
              .apply(caught_trip_then_a_second_one)
              .parse_str("1 2 3")
          };
          assert_eq!(
            roomy,
            Ok(vec![1, 2, 3]),
            concat!(
              "a second trip over ",
              $sink,
              ": the budget is what failed the run above — with room the same source parses and \
               collects"
            )
          );
          assert_eq!(
            trips(),
            0,
            concat!(
              "a second trip over ",
              $sink,
              ": and the control tripped nothing at all"
            )
          );
          assert_eq!(
            filed(&log),
            0,
            concat!(
              "a second trip over ",
              $sink,
              ": the control is a clean parse, so it files nothing either"
            )
          );
        }
      }

      // ── Section 4: a trip the element ANSWERS is not spendable as an absence ──
      //
      // Sections 1–3 are all about the `Err` the element hands back. An element that catches the
      // trip itself hands back `Ok` instead, and the driver's element-failure chokepoint never sees
      // it: the loop takes an absence exit — the element declining, or a cycle that committed
      // nothing — concludes "no more elements", and the collection SUCCEEDS. A resource-budget stop
      // becomes an accepted absence, for every sink, which is section 1's defect reached through
      // the exit its gate does not cover.
      //
      // Two exits per family, because they are different gate lines in every driver: the decline
      // and the no-progress stall. In the two delimited families both land in a close-probe arm, so
      // each source here is chosen to put a close MISS there — `WrongToken` or `Eof`. The real-closer
      // arm of the same probe is section 5's: it is a *different* gate, holding the descent witness
      // alone, because a committed pre-trip closer settles where the construct ended and settles
      // nothing about what the attempt before it cost.
      #[allow(rustdoc::private_intra_doc_links)]
      mod answered_trip {
        use super::*;

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_end_repeated,
          $err,
          $sink,
          repeated_decline_probe,
          "`repeated()`, element declines",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3]),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_repeated,
          $err,
          $sink,
          repeated_stall_probe,
          "`repeated()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 0
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_close_delimited_repeated,
          $err,
          $sink,
          delim_repeated_decline_probe,
          "`repeated().delimited()`, element declines at the unclosed end",
          "( 1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3]),
          roomy_filed = 1
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_close_delimited_repeated,
          $err,
          $sink,
          delim_repeated_stall_probe,
          "`repeated().delimited()`, element accepts consuming nothing at the unclosed end",
          "( 1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 1
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_end_separated,
          $err,
          $sink,
          sep_decline_probe,
          "`separated_by_comma()`, element declines on a token that is not an element",
          "1 , 2 , 3 +",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3]),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_separated,
          $err,
          $sink,
          sep_stall_probe,
          "`separated_by_comma()`, element accepts consuming nothing",
          "1 , 2 , 3 +",
          stops = $eot,
          tight_filed = 1,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 1
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_close_delimited_separated,
          $err,
          $sink,
          sep_delim_decline_probe,
          "`separated_by_comma().delimited()`, element declines where the closer should be",
          "( 1 , 2 , 3 +",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3]),
          roomy_filed = 1
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_close_delimited_separated,
          $err,
          $sink,
          sep_delim_stall_probe,
          "`separated_by_comma().delimited()`, element accepts consuming nothing",
          "( 1 , 2 , 3 +",
          stops = $eot,
          tight_filed = 1,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 2
        );
      }

      // ── Section 5: the closer is genuinely there, and the trip is still spent ──
      //
      // Section 4's delimited sources all end on a close MISS, which is why they never touched the
      // arm below. When the closer IS present the delimited drivers commit it and succeed — and for
      // one of the two never-recoverable witnesses that is exactly right:
      //
      // * the **scanner latch** is a fact about a token POSITION. A `CloseStatus::Close` verdict is
      //   cache-first, so it rests on a real pre-trip token: the construct ended ahead of whatever
      //   boundary the element's lookahead went on to latch, and that boundary is not about it.
      //   Gating a real close on the latch would fail a parse a wider scan window completes to the
      //   identical value, so the latch stays off this arm — `absence_terminal_stop.rs` is where
      //   that direction is pinned;
      // * the **descent trip** is a COUNTER EVENT inside the element attempt. A valid closer
      //   arriving afterwards does not unmake it. Before this, an element that caught a
      //   `RecursionLimitReached`, reported *no more elements*, and was then followed by a real
      //   closer produced a successfully closed collection that had silently spent a resource-limit
      //   stop — section 4's defect, through the arm section 4's gate deliberately does not cover.
      //
      // So these cells are section 4's with one character added to the source, and the arm they
      // reach carries the descent witness alone (`parser::many::close_after_element`).
      //
      // `separated().delimited()` gets ONE cell here, not two, and the missing one is a fact about
      // that driver rather than an omission. It probes the separator-or-close slot BEFORE it
      // attempts an element, so a closer sitting there is committed by that probe and the element
      // never runs; the epilogue sees a real closer only when the element attempt moved the front
      // onto it, which means the element CONSUMED. A consuming attempt cannot also be the
      // no-progress stall — the stall is defined by having committed nothing — so the stall exit of
      // that driver can only ever reach a close MISS, which section 4 already covers. The element
      // below therefore consumes and declines, the same shape `probe_close_no_rescan.rs` needs to
      // reach the same arm.
      #[allow(rustdoc::private_intra_doc_links)]
      mod answered_trip_at_a_real_closer {
        use super::*;

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_commit_the_repeated_closer,
          $err,
          $sink,
          delim_repeated_decline_probe,
          "`repeated().delimited()`, element declines with the closer at hand",
          "( 1 2 3 )",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3]),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_commit_the_repeated_closer,
          $err,
          $sink,
          delim_repeated_stall_probe,
          "`repeated().delimited()`, element accepts consuming nothing with the closer at hand",
          "( 1 2 3 )",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 0
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_commit_the_separated_closer,
          $err,
          $sink,
          sep_delim_consuming_decline_probe,
          "`separated_by_comma().delimited()`, element consumes and declines with the closer at hand",
          "( 1 , 2 , 3 + )",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3]),
          roomy_filed = 0
        );
      }

      // ── Section 6: the one exit no gate closes, because closing it is not this branch's job ──
      //
      // Sections 4 and 5 gate every exit that concludes the collection FROM an element's absence —
      // a decline, a stall, or a real closer arriving after either. An `Accept` concludes nothing:
      // the element produced a value, and the driver is faithfully collecting what it was handed,
      // not manufacturing "no more elements" out of a stop the caller never learns about. Gating
      // this exit would need the driver to refuse a value-producing return because of a trip the
      // grammar already caught — true of every error a grammar can catch and answer, not only this
      // one — which is a broader contract than #148 establishes.
      //
      // So an element that catches a trip, consumes, and still answers `Accept` leaves the counter
      // exactly where the trip left it, and the very next cycle's own baseline is taken AFTER
      // that — rebaselined, in the terms `parser::many`'s module docs use. When that next cycle is
      // the one that reaches a real closer (or, for `separated().delimited()`, consumes the
      // non-element token that puts a real closer in the epilogue's hands), neither
      // `close_after_element` nor anything else has a trip left to see. The collection closes and
      // succeeds, having produced the value the accepting element built while it was over budget.
      //
      // THIS SECTION PINS DELIBERATE, EXISTING BEHAVIOUR. It is not a regression witness and is not
      // expected to ever go red from correct code: it exists so a later change cannot silently
      // narrow the contract (start gating `Accept`, which would strand a grammar's own recovery
      // from a budget it deliberately caught) or silently widen it (stop re-raising the
      // decline/stall/closer exits sections 4 and 5 already gate). If this section starts failing,
      // the fix is almost never here — it means one of those two boundaries moved, and the change
      // that moved it owes a changelog entry, not a quiet edit to this file.
      #[allow(rustdoc::private_intra_doc_links)]
      mod answered_trip_through_a_value {
        use super::*;

        accept_exemption_cell!(
          an_accepting_element_that_answered_a_trip_still_commits_the_repeated_closer_by_design,
          $err,
          $sink,
          delim_repeated_accept_probe,
          "`repeated().delimited()`, element consumes and accepts through a caught trip, closer at \
           the next slot",
          "( 7 )",
          collects = Ok(vec![7])
        );

        accept_exemption_cell!(
          an_accepting_element_that_answered_a_trip_still_commits_the_separated_closer_by_design,
          $err,
          $sink,
          sep_delim_accept_probe,
          "`separated_by_comma().delimited()`, element consumes and accepts through a caught trip, \
           closer reached through the epilogue",
          "( 7 + )",
          collects = Ok(vec![7])
        );
      }

      // ── Section 7: the same absence exits in the `*_while` drivers and the folds ──
      //
      // Sections 4 and 5 close the hole for the four TRY-DRIVEN families. The other eight
      // guard-bearing sources — the four `*_while` collection drivers and the four fold sources —
      // had it too, and for a reason that is *narrower* than the try-driven four's rather than
      // different: they never file an element's `Err`, so a trip an element hands back propagates
      // untouched and IS terminal there. What they could not see is the same `Ok` sections 4 and 5
      // are about — the element answering the trip itself, and the driver then concluding *absence*
      // from an attempt a resource budget stopped.
      //
      // The element shape differs by family, because the two groups' elements differ:
      //
      // * the four plain folds take a [`TryParseInput`](tokora::TryParseInput) element, so both
      //   absence exits are reachable — the decline and the no-progress stall. Both are driven;
      // * the four `*_while` collections and the four `*_while` folds take a plain
      //   [`ParseInput`](tokora::ParseInput) element, which has no decline channel at all. Their
      //   only element-driven absence exit is the stall, and [`always_continue`] is what routes
      //   every cycle to the element so the stall is what ends the collection. Their *other* exit,
      //   the condition's `Action::Stop`, sits at the TOP of a cycle — the element that could have
      //   caught a trip is then the PREVIOUS cycle's accepting one, which section 6 exempts by
      //   design, so the descent witness there is a constant `false` and there is nothing for a cell
      //   to measure.
      //
      // Measured before it was fixed, with a throwaway probe and then with these cells: `fold` over
      // a declining element returned `Ok(6)` on `"1 2 3"` under a budget the element exceeded and
      // `Ok(6)` under one it did not, and `repeated_while` over the stalling element returned
      // `Ok([1, 2, 3, -1])` under both. Every cell below is that comparison, and every one of them
      // failed on `tight == roomy` before the per-element baseline was added.
      #[allow(rustdoc::private_intra_doc_links)]
      mod answered_trip_in_a_while_or_fold {
        use super::*;

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_repeated_while,
          $err,
          $sink,
          repeated_while_stall_probe,
          "`repeated_while()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_close_delimited_repeated_while,
          $err,
          $sink,
          delim_repeated_while_stall_probe,
          "`repeated_while().delimited()`, element accepts consuming nothing at the unclosed end",
          "( 1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 1
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_separated_while,
          $err,
          $sink,
          sep_while_stall_probe,
          "`separated_by_comma_while()`, element accepts consuming nothing",
          "1 , 2 , 3 +",
          stops = $eot,
          tight_filed = 1,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 1
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_close_delimited_separated_while,
          $err,
          $sink,
          sep_while_delim_stall_probe,
          "`separated_by_comma_while().delimited()`, element accepts consuming nothing",
          "( 1 , 2 , 3 +",
          stops = $eot,
          tight_filed = 1,
          collects = Ok(vec![1, 2, 3, ZERO_WIDTH]),
          roomy_filed = 2
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_end_fold,
          $err,
          $sink,
          fold_decline_probe,
          "`fold()`, element declines",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(6i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_fold,
          $err,
          $sink,
          fold_stall_probe,
          "`fold()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_end_try_fold,
          $err,
          $sink,
          try_fold_decline_probe,
          "`try_fold()`, element declines",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(6i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_try_fold,
          $err,
          $sink,
          try_fold_stall_probe,
          "`try_fold()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_end_try_fold_with,
          $err,
          $sink,
          try_fold_with_decline_probe,
          "`try_fold_with()`, element declines",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(6i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_try_fold_with,
          $err,
          $sink,
          try_fold_with_stall_probe,
          "`try_fold_with()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_declining_element_that_answered_a_trip_does_not_end_rfold,
          $err,
          $sink,
          rfold_decline_probe,
          "`rfold()`, element declines",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(6i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_rfold,
          $err,
          $sink,
          rfold_stall_probe,
          "`rfold()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_fold_while,
          $err,
          $sink,
          fold_while_stall_probe,
          "`fold_while()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_try_fold_while,
          $err,
          $sink,
          try_fold_while_stall_probe,
          "`try_fold_while()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_try_fold_while_with,
          $err,
          $sink,
          try_fold_while_with_stall_probe,
          "`try_fold_while_with()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );

        absence_cell!(
          a_stalling_element_that_answered_a_trip_does_not_end_rfold_while,
          $err,
          $sink,
          rfold_while_stall_probe,
          "`rfold_while()`, element accepts consuming nothing",
          "1 2 3",
          stops = $eot,
          tight_filed = 0,
          collects = Ok(5i64),
          roomy_filed = 0
        );
      }
    }
  };
}

trip_suite!(
  delegating_sink,
  TripErr,
  TripErr::Depth,
  TripErr::Ordinary,
  TripErr::Ordinary,
  "a delegating error type"
);
trip_suite!(discarding_sink, (), (), (), (), "the discarding `()` sink");
