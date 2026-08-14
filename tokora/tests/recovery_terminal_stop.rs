#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The recovery combinators' **scanner** terminal-stop law: a resource-limit trip inside a
//! recovery attempt is re-raised, never retried — whichever emitter the parse was built with.
//!
//! # The defect these pin
//!
//! Terminality is a property of the **event**, and this crate stores it in carriers. A scanner
//! trip reaches an accepting emitter as a terminal-marked [`UnexpectedEot`] (so
//! [`MaybeTerminal::is_terminal`] answers `true`), but a **rejecting** emitter reports the very
//! same trip by returning `Err` from `emit_lexer_error` — and that value is built by
//! `FromEmitterError::from_lexer_error` out of the lexer's own `Token::Error`, which never passes
//! through `into_terminal()`. Same trip, same position, same committed leaf; one carrier marked,
//! one not. A rejecting emitter's `Err` is not a refusal to report — it **is** the report — so
//! this is every fail-fast sink, not a `Fatal` quirk.
//!
//! [`Recover`](tokora::parser::Recover), [`InplaceRecover`](tokora::parser::InplaceRecover) and
//! [`skip_then_retry`](tokora::ParseInput::skip_then_retry) keyed their terminal re-raise off the
//! error alone plus the *descent* counter, so through a rejecting emitter the recoverer RAN: it
//! re-entered the scanner and re-tripped the same limit. The input-side scanner witness
//! (`latch_snapshot`/`latched_during_attempt`) is what closes it — read **inside** the attempt,
//! because a speculating attempt's rollback restores the latch along with the position.
//!
//! # How each cell measures "did it retry?"
//!
//! Self-calibrating, so no absolute scan tally has to be maintained: the primary parser records
//! `scanned` at the instant its own scan tripped, and the cell asserts the counter has not moved
//! since. A recoverer that re-enters the scanner moves it — that is the whole observation.
//!
//! [`UnexpectedEot`]: tokora::error::UnexpectedEot
//! [`MaybeTerminal::is_terminal`]: tokora::error::MaybeTerminal::is_terminal

use core::cell::Cell;
use std::rc::Rc;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext, Token as TokenT,
  emitter::{Fatal, Silent},
  error::{
    MaybeIncomplete, MaybeTerminal, UnexpectedEot,
    token::{SeparatedError, UnexpectedToken},
  },
  input::{Balance, Cursor},
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::{InplaceRecoverInput, RecoverInput},
  span::{SimpleSpan, Spanned},
  state::State,
};

// ── A scan limiter whose counter is SHARED across every cloned lexer ──────────
//
// `InputRef` rebuilds a fresh lexer per operation by cloning the state, so only an
// `Rc<Cell<_>>`-shared counter makes every scan observable — and only a shared one stays tripped
// across the rollback a failed attempt performs.

#[derive(Debug, Clone, Default)]
struct ScanLimiter {
  scanned: Rc<Cell<usize>>,
  limit: usize,
}

impl ScanLimiter {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: Rc::new(Cell::new(0)),
      limit,
    }
  }

  /// A shared handle to observe the scan counter after moving the state in.
  fn counter(&self) -> Rc<Cell<usize>> {
    self.scanned.clone()
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }
}

#[derive(Debug, Clone, PartialEq)]
struct ScanLimitExceeded;

impl State for ScanLimiter {
  type Error = ScanLimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.scanned.get() > self.limit {
      Err(ScanLimitExceeded)
    } else {
      Ok(())
    }
  }
}

// ── The fixture error: the two carriers one trip can arrive in, kept apart ────

#[derive(Debug, Clone, PartialEq)]
enum RErr {
  /// The trip as a **rejecting** emitter reports it: built from the lexer's own `Token::Error`,
  /// so nothing on that path can mark it terminal. This is the carrier the defect rode.
  Limit,
  /// The trip as an **accepting** emitter surfaces it: a terminal-marked end-of-input.
  Terminal,
  /// Anything else — the ordinary syntax failure recovery is actually for.
  Other,
}

impl MaybeTerminal for RErr {
  fn is_terminal(&self) -> bool {
    matches!(self, RErr::Terminal)
  }
}

impl MaybeIncomplete for RErr {}

impl From<()> for RErr {
  fn from(_: ()) -> Self {
    RErr::Other
  }
}

impl From<ScanLimitExceeded> for RErr {
  fn from(_: ScanLimitExceeded) -> Self {
    RErr::Limit
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for RErr {
  fn from(e: UnexpectedEot<O, Lang, Set>) -> Self {
    if e.is_terminal() {
      RErr::Terminal
    } else {
      RErr::Other
    }
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for RErr {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    RErr::Other
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for RErr {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    RErr::Other
  }
}

// ── Token vocabulary: identifiers and a `;` sync token, every scan counted ────

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, extras = ScanLimiter, skip r"[ \t\r\n]+")]
enum Tok {
  #[regex(r"[a-z]+", |lex| { lex.extras.increase(); })]
  Ident,
  #[token(";", |lex| { lex.extras.increase(); })]
  Semi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
  Ident,
  Semi,
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

impl core::fmt::Display for Kind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Kind::Ident => "identifier",
      Kind::Semi => "`;`",
    })
  }
}

impl TokenT<'_> for Tok {
  type Kind = Kind;
  type Error = RErr;

  const READ_FRONTIER_CLASS: tokora::ReadFrontierClass = tokora::ReadFrontierClass::Unbounded;

  fn kind(&self) -> Kind {
    match self {
      Tok::Ident => Kind::Ident,
      Tok::Semi => Kind::Semi,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

// ── Harness ───────────────────────────────────────────────────────────────────

type TLexer<'a> = LogosLexer<'a, Tok>;

/// One concrete lexer, generic over the emitter, through the public `ParserContext` +
/// `parse_str_with_state` surface.
fn drive<'inp, O, Em>(
  emitter: Em,
  state: ScanLimiter,
  f: impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Em>>,
  ) -> Result<O, RErr>,
  input: &'inp str,
) -> Result<O, RErr>
where
  Em: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  let ctx: ParserContext<'inp, TLexer<'inp>, Em> = ParserContext::new(emitter);
  Parser::with_parser_and_context(f, ctx).parse_str_with_state(input, state)
}

/// The shared observation set for one recovery cell.
#[derive(Debug)]
struct Probe {
  /// How many times the primary parser was applied.
  calls: Cell<usize>,
  /// The scan count at the instant the primary's own scan tripped.
  at_trip: Cell<Option<usize>>,
  /// Whether the recovery handler ran.
  recovered: Cell<bool>,
  /// Whether the sync predicate ran (`skip_then_retry` only).
  synced: Cell<bool>,
  /// The lexer's shared scan counter.
  scanned: Rc<Cell<usize>>,
}

impl Probe {
  fn new(scanned: Rc<Cell<usize>>) -> Self {
    Self {
      calls: Cell::new(0),
      at_trip: Cell::new(None),
      recovered: Cell::new(false),
      synced: Cell::new(false),
      scanned,
    }
  }

  /// The assertion every trip cell shares: the trip's own carrier is re-raised, the recoverer
  /// never ran, and **no scan happened after the trip** — the retry the defect performed.
  fn assert_reraised_without_retry(&self, out: Result<(), RErr>, expected: RErr, calls: usize) {
    // The retry itself, asserted first: it is the observation the whole suite exists for, and it
    // is the one that survives every emitter — an in-place recovery keeps the latch and re-raises
    // from the boundary without re-scanning, while a speculating one rolls the latch back and
    // genuinely re-lexes.
    let at_trip = self
      .at_trip
      .get()
      .expect("the primary parser must have observed the trip");
    assert_eq!(
      self.scanned.get(),
      at_trip,
      "a terminal resource stop must not be retried: the scanner was re-entered after the trip \
       (scan count {at_trip} -> {})",
      self.scanned.get()
    );
    assert!(
      !self.recovered.get(),
      "the recovery handler must never run for a resource-limit trip"
    );
    assert_eq!(
      out,
      Err(expected),
      "the trip's own error is re-raised on the Err channel"
    );
    assert_eq!(
      self.calls.get(),
      calls,
      "the primary parser was applied {} time(s), expected {calls}",
      self.calls.get()
    );
  }
}

/// The primary parser: drain the input through the **committed leaf** primitive
/// ([`next_or_stop`](InputRef::next_or_stop)), recording the scan count the moment a scan errors.
///
/// `next_or_stop` is the primitive whose whole job is to keep a terminal stop off the plain
/// end-of-input channel, so every trip cell drives this one body and the *only* difference between
/// the accepting and the rejecting emitter is which carrier the trip arrives in: a terminal-marked
/// `UnexpectedEot` from the `Tripped` arm, or the emitter's own rejection value propagated out of
/// the scan. With `decline_first`, the first application instead declines ordinarily without
/// consuming — the shape `skip_then_retry` needs to reach its retry loop at all.
struct Drain<'p> {
  probe: &'p Probe,
  decline_first: bool,
}

impl<'inp, Ctx> ParseInput<'inp, TLexer<'inp>, (), Ctx> for Drain<'_>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  fn parse_input(&mut self, inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<(), RErr> {
    let call = self.probe.calls.get() + 1;
    self.probe.calls.set(call);
    if self.decline_first && call == 1 {
      return Err(RErr::Other);
    }
    loop {
      match inp.next_or_stop() {
        Ok(Some(_)) => continue,
        Ok(None) => return Ok(()),
        Err(e) => {
          self.probe.at_trip.set(Some(self.probe.scanned.get()));
          return Err(e);
        }
      }
    }
  }
}

/// A recoverer that flags itself and then **re-enters the scanner** — the defect's retry, made
/// observable — before succeeding.
struct ScanningRecover<'p>(&'p Probe);

impl<'inp, Ctx> RecoverInput<'inp, TLexer<'inp>, (), Ctx> for ScanningRecover<'_>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  fn recover_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
    _err: RErr,
  ) -> Result<(), RErr> {
    self.0.recovered.set(true);
    let _ = inp.next();
    Ok(())
  }
}

impl<'inp, Ctx> InplaceRecoverInput<'inp, TLexer<'inp>, (), Ctx> for ScanningRecover<'_>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  fn inplace_recover_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
    _cursor: Cursor<'inp, '_, TLexer<'inp>>,
    _err: RErr,
  ) -> Result<(), RErr> {
    self.0.recovered.set(true);
    let _ = inp.next();
    Ok(())
  }
}

/// The primary of the **nested-speculation** cells: it drains through the committed leaf inside an
/// *inner* speculation of its own, CATCHES the scanner trip there, declines the inner attempt — so
/// the inner rollback restores the poison boundary — and then fails for an ordinary reason.
///
/// Grammar code is entitled to every one of those moves. What it must not be able to do is erase
/// the enclosing gate's evidence that a resource budget was spent, which a latch comparison at any
/// single nesting level cannot prevent: the read sits at one depth and the rollback that defeats it
/// sits one deeper.
struct CatchInsideSpeculation<'p>(&'p Probe);

impl<'inp, Ctx> ParseInput<'inp, TLexer<'inp>, (), Ctx> for CatchInsideSpeculation<'_>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  fn parse_input(&mut self, inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<(), RErr> {
    self.0.calls.set(self.0.calls.get() + 1);
    let probe = self.0;
    let _declined: Option<()> = inp.attempt(|inp| {
      loop {
        match inp.next_or_stop() {
          Ok(Some(_)) => continue,
          Ok(None) => return Some(()),
          Err(_caught) => {
            probe.at_trip.set(Some(probe.scanned.get()));
            // Decline: the inner rollback puts the position, the emissions AND the poison
            // boundary back. Everything a latch comparison could have read is now gone.
            return None;
          }
        }
      }
    });
    Err(RErr::Other)
  }
}

/// A primary that fails outright with a chosen error, consuming nothing.
struct FailWith(RErr);

impl<'inp, Ctx> ParseInput<'inp, TLexer<'inp>, (), Ctx> for FailWith
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  fn parse_input(&mut self, _inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<(), RErr> {
    Err(self.0.clone())
  }
}

/// A primary that requires the next token to be `;`, counting its applications.
struct WantSemi<'p>(&'p Probe);

impl<'inp, Ctx> ParseInput<'inp, TLexer<'inp>, (), Ctx> for WantSemi<'_>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  fn parse_input(&mut self, inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<(), RErr> {
    self.0.calls.set(self.0.calls.get() + 1);
    match inp.next()? {
      Some(t) if matches!(t.data(), Tok::Semi) => Ok(()),
      _ => Err(RErr::Other),
    }
  }
}

/// A primary that always declines ordinarily without consuming, counting its applications — the
/// shape that keeps `skip_then_retry` cycling until one of its own phases stops it.
struct AlwaysDecline<'p>(&'p Probe);

impl<'inp, Ctx> ParseInput<'inp, TLexer<'inp>, (), Ctx> for AlwaysDecline<'_>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = RErr>,
{
  fn parse_input(&mut self, _inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<(), RErr> {
    self.0.calls.set(self.0.calls.get() + 1);
    Err(RErr::Other)
  }
}

/// No delimiter pairs in this grammar.
fn neutral(_: &Kind) -> Balance<()> {
  Balance::Neutral
}

/// The depth-0 sync predicate: the `;`.
fn is_semi(t: Spanned<&Tok, &SimpleSpan>) -> bool {
  matches!(t.data(), Tok::Semi)
}

// ── T1/T2: `Recover` re-raises a trip under BOTH emitters ─────────────────────
//
// The headline regression. `Silent` was already green — its carrier is terminal-marked — so the
// two halves together are the statement that the verdict no longer depends on the sink.

#[test]
fn recover_reraises_a_scanner_trip_under_a_rejecting_emitter() {
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Fatal::<RErr>::new(),
    limiter,
    |inp| {
      Drain {
        probe: &probe,
        decline_first: false,
      }
      .recover(ScanningRecover(&probe))
      .parse_input(inp)
    },
    "a b c d",
  );

  probe.assert_reraised_without_retry(out, RErr::Limit, 1);
}

#[test]
fn recover_reraises_a_scanner_trip_under_an_accepting_emitter() {
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Silent::<RErr>::new(),
    limiter,
    |inp| {
      Drain {
        probe: &probe,
        decline_first: false,
      }
      .recover(ScanningRecover(&probe))
      .parse_input(inp)
    },
    "a b c d",
  );

  probe.assert_reraised_without_retry(out, RErr::Terminal, 1);
}

// ── T3: `InplaceRecover` re-raises a trip under a rejecting emitter ───────────

#[test]
fn inplace_recover_reraises_a_scanner_trip_under_a_rejecting_emitter() {
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Fatal::<RErr>::new(),
    limiter,
    |inp| {
      Drain {
        probe: &probe,
        decline_first: false,
      }
      .inplace_recover(ScanningRecover(&probe))
      .parse_input(inp)
    },
    "a b c d",
  );

  probe.assert_reraised_without_retry(out, RErr::Limit, 1);
}

// ── T4: `skip_then_retry`'s FIRST attempt re-raises a trip ────────────────────

#[test]
fn skip_then_retry_reraises_a_first_attempt_trip_under_a_rejecting_emitter() {
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Fatal::<RErr>::new(),
    limiter,
    |inp| {
      Drain {
        probe: &probe,
        decline_first: false,
      }
      .skip_then_retry(neutral, |t: Spanned<&Tok, &SimpleSpan>| {
        probe.synced.set(true);
        is_semi(t)
      })
      .parse_input(inp)
    },
    "a b c d",
  );

  probe.assert_reraised_without_retry(out, RErr::Limit, 1);
  assert!(
    !probe.synced.get(),
    "the sync predicate never runs for a resource-limit trip: nothing is skipped"
  );
}

// ── T5: a trip inside a RETRY cycle re-raises too ─────────────────────────────
//
// The second baseline site — the one deliberately inside the retry loop. The primary declines
// ordinarily first (so the combinator legitimately syncs and retries), and the retry is what
// trips: `a` is skipped to reach the `;`, and draining from there spends the budget.

#[test]
fn skip_then_retry_reraises_a_retry_cycle_trip_under_a_rejecting_emitter() {
  let limiter = ScanLimiter::with_limit(3);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Fatal::<RErr>::new(),
    limiter,
    |inp| {
      Drain {
        probe: &probe,
        decline_first: true,
      }
      .skip_then_retry(neutral, is_semi)
      .parse_input(inp)
    },
    "a ; b c d",
  );

  probe.assert_reraised_without_retry(out, RErr::Limit, 2);
}

// ── T6–T8: the scoping guard — an ordinary failure still recovers ─────────────
//
// The fix must refuse a trip, not recovery. Under a roomy limit nothing latches, so each
// combinator behaves exactly as before.

#[test]
fn recover_still_recovers_an_ordinary_failure() {
  let limiter = ScanLimiter::with_limit(usize::MAX);
  let probe = Probe::new(limiter.counter());
  let out = drive(
    Silent::<RErr>::new(),
    limiter,
    |inp| {
      FailWith(RErr::Other)
        .recover(ScanningRecover(&probe))
        .parse_input(inp)
    },
    "a b c d",
  );
  assert_eq!(out, Ok(()), "an ordinary failure is still recovered");
  assert!(probe.recovered.get(), "the recoverer ran");
}

#[test]
fn inplace_recover_still_recovers_an_ordinary_failure() {
  let limiter = ScanLimiter::with_limit(usize::MAX);
  let probe = Probe::new(limiter.counter());
  let out = drive(
    Silent::<RErr>::new(),
    limiter,
    |inp| {
      FailWith(RErr::Other)
        .inplace_recover(ScanningRecover(&probe))
        .parse_input(inp)
    },
    "a b c d",
  );
  assert_eq!(out, Ok(()), "an ordinary failure is still recovered");
  assert!(probe.recovered.get(), "the in-place recoverer ran");
}

#[test]
fn skip_then_retry_still_retries_after_an_ordinary_failure() {
  let limiter = ScanLimiter::with_limit(usize::MAX);
  let probe = Probe::new(limiter.counter());
  let out = drive(
    Silent::<RErr>::new(),
    limiter,
    |inp| {
      WantSemi(&probe)
        .skip_then_retry(neutral, is_semi)
        .parse_input(inp)
    },
    "a ;",
  );
  assert_eq!(out, Ok(()), "the retry parses the sync token");
  assert_eq!(
    probe.calls.get(),
    2,
    "one failed attempt, one successful retry"
  );
}

// ── T9/T10: a trip the primary CAUGHT inside a NESTED speculation ─────────────
//
// The defect one level down, and the reason the scanner witness is a monotone counter rather than
// the boundary the trip latched. The boundary is a lineage memo: a checkpoint carries it and a
// restore copies it back. Reading it *inside* the outer attempt escapes the *outer* rollback and
// nothing else — grammar code that catches a stop inside an inner `attempt` has that inner rollback
// erase the boundary before the outer gate ever looks, and every reading of it says clean over a
// stop that is live, already diagnosed, and re-trips on the next scan of the same prefix.
//
// Both combinators are driven, because the two rollback postures are what would otherwise make this
// look like a property of the outer attempt: `Recover` speculates and `InplaceRecover` does not,
// and the *inner* rollback is what defeats the latch in either.

#[test]
fn recover_reraises_a_trip_caught_inside_a_nested_speculation() {
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Fatal::<RErr>::new(),
    limiter,
    |inp| {
      CatchInsideSpeculation(&probe)
        .recover(ScanningRecover(&probe))
        .parse_input(inp)
    },
    "a b c d",
  );

  // The primary's own error is ordinary — that is the point. The stop is witnessed on the input,
  // by a cell the inner rollback cannot reach.
  probe.assert_reraised_without_retry(out, RErr::Other, 1);
}

#[test]
fn inplace_recover_reraises_a_trip_caught_inside_a_nested_speculation() {
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Fatal::<RErr>::new(),
    limiter,
    |inp| {
      CatchInsideSpeculation(&probe)
        .inplace_recover(ScanningRecover(&probe))
        .parse_input(inp)
    },
    "a b c d",
  );

  probe.assert_reraised_without_retry(out, RErr::Other, 1);
}

// ── T11–T13: the SKIP and ADVANCE phases, which the gate did not wrap ─────────
//
// `SkipThenRetry`'s recovery work is not only its retries: it is the skip to a sync point and the
// advance over a spent one, and both ran outside the gate. Both fold a terminal trip into the same
// `Ok(None)` they use for genuine exhaustion, so a spent scanner budget read as "nothing left to
// skip to" and the combinator surfaced its ordinary trigger error for it.
//
// An ACCEPTING emitter is the whole point of these cells: through a rejecting one the trip's
// rejection propagates as an `Err` straight out of the skip, and the fold was never visible.

#[test]
fn skip_then_retry_does_not_read_a_sync_phase_trip_as_end_of_input() {
  // No `;` anywhere, so the skip scans until something stops it — and the limit stops it first.
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Silent::<RErr>::new(),
    limiter,
    |inp| {
      FailWith(RErr::Other)
        .skip_then_retry(neutral, |t: Spanned<&Tok, &SimpleSpan>| {
          probe.synced.set(true);
          is_semi(t)
        })
        .parse_input(inp)
    },
    "a b c d e f",
  );

  assert_eq!(
    out,
    Err(RErr::Terminal),
    "a scanner trip during the SKIP is a terminal stop, not `nowhere to sync to`: it must not be \
     reported as the ordinary error that triggered the recovery"
  );
  assert!(
    probe.synced.get(),
    "the skip really ran — this cell is about how it ENDED, so a sync that never started would \
     make the assertion above vacuous"
  );
}

#[test]
fn skip_then_retry_still_reads_a_genuine_sync_exhaustion_as_end_of_input() {
  // The other half of the pair, and the one that keeps the cell above from passing for the wrong
  // reason: same grammar, same absent `;`, a limit nothing can reach. The skip runs out of input
  // rather than out of budget, and THAT is the trigger error's case.
  let limiter = ScanLimiter::with_limit(usize::MAX);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Silent::<RErr>::new(),
    limiter,
    |inp| {
      FailWith(RErr::Other)
        .skip_then_retry(neutral, |t: Spanned<&Tok, &SimpleSpan>| {
          probe.synced.set(true);
          is_semi(t)
        })
        .parse_input(inp)
    },
    "a b c d e f",
  );

  assert_eq!(
    out,
    Err(RErr::Other),
    "a genuine end of input still surfaces the error that triggered the recovery — the fix must \
     narrow the EOF reading, not replace it"
  );
  assert!(probe.synced.get(), "the skip ran");
}

#[test]
fn skip_then_retry_does_not_read_an_advance_phase_trip_as_end_of_input() {
  //   a ; b c d
  // The primary declines ordinarily; cycle 1 skips `a` and syncs at `;`; the retry declines
  // ordinarily again; and the ADVANCE that consumes the spent `;` is what spends the last of the
  // budget. `next` folds that trip into the same `Ok(None)` it uses for a consumed-out stream.
  let limiter = ScanLimiter::with_limit(2);
  let probe = Probe::new(limiter.counter());

  let out = drive(
    Silent::<RErr>::new(),
    limiter,
    |inp| {
      AlwaysDecline(&probe)
        .skip_then_retry(neutral, is_semi)
        .parse_input(inp)
    },
    "a ; b c d",
  );

  assert_eq!(
    out,
    Err(RErr::Terminal),
    "a scanner trip during the ADVANCE over a spent sync point is a terminal stop, not `nothing \
     left to consume`"
  );
  assert!(
    probe.calls.get() >= 2,
    "the combinator reached its retry loop: {} application(s)",
    probe.calls.get()
  );
}
