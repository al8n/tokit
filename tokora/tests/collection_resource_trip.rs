#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! A **descent** budget trip inside a collection element re-raises; it is not spent as a diagnostic.
//!
//! The four try-driven collection families — `repeated`, `separated`, and their delimited forms —
//! swallow an element's `Err` by design: emit it and keep looping. Their gate re-raises what no
//! further input can clear, and until issue #148 it recognized only two of the three such
//! conditions. The third, a [`RecursionLimitReached`], latches no boundary — it has a control stack
//! rather than a position — so `inp.at_committed_boundary()` reads `false` for one and the trip fell
//! through to the swallow arm: emitted as an ordinary diagnostic, with the loop continuing to the
//! next element. The gate now also reads `inp.resource_trip()`, the set-once session latch the trip
//! arm arms before the grammar's `From` runs.
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

use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  TryParseInput,
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

/// Generates the four family cells for one grammar error type.
///
/// A macro rather than a function generic over the error type: the drivers' bounds are stated per
/// family as `Error = ...` equality constraints, and spelling them once against a substituted `$err`
/// keeps each probe reading exactly like the grammar a user would write, with no higher-ranked
/// `From` bound collection standing between the test and what it measures.
macro_rules! trip_suite {
  ($suite:ident, $err:ty, $trip:expr, $sink:literal) => {
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
      fn ladder<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
        left: usize,
      ) -> Result<(), $err>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = $err>,
      {
        let mut frame = inp.descend()?;
        let inp = &mut *frame;
        match left {
          0 => Ok(()),
          n => ladder(inp, n - 1),
        }
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
    }
  };
}

trip_suite!(
  delegating_sink,
  TripErr,
  TripErr::Depth,
  "a delegating error type"
);
trip_suite!(discarding_sink, (), (), "the discarding `()` sink");
