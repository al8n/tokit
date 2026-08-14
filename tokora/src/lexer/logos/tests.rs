use super::super::{Lexer, Token as TokenTrait};
use crate::span::Span;

// The version-neutral surfaces: `crate::logos` is the crate alias (newest enabled
// version wins) and `super::{FromLogos, LogosLexer}` are the adapter re-exports that
// follow the same precedence. This module therefore compiles unchanged on 0.14, 0.15
// and 0.16, which is what lets the per-version CI legs run it at all three.
use crate::logos;

#[derive(Debug, Clone, PartialEq, logos::Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
enum TestTok {
  #[token("+")]
  Plus,
  #[regex(r"[0-9]+")]
  Num,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestKind {
  Plus,
  Num,
}

impl core::fmt::Display for TestKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TestKind::Plus => write!(f, "+"),
      TestKind::Num => write!(f, "number"),
    }
  }
}

impl core::fmt::Display for TestTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TestTok::Plus => write!(f, "+"),
      TestTok::Num => write!(f, "number"),
    }
  }
}

impl TokenTrait<'_> for TestTok {
  type Kind = TestKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> TestKind {
    match self {
      TestTok::Plus => TestKind::Plus,
      TestTok::Num => TestKind::Num,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type TestLexer<'a> = super::LogosLexer<'a, TestTok>;

#[test]
fn logos_lexer_new() {
  let lexer = TestLexer::new("42 + 1");
  let _ = lexer;
}

#[test]
fn logos_lexer_with_state() {
  let lexer = TestLexer::with_state("42 + 1", ());
  let _ = lexer;
}

#[test]
fn logos_lexer_lex_tokens() {
  let mut lexer = TestLexer::new("42 + 1");
  let tok1 = lexer.lex().unwrap().unwrap();
  assert_eq!(tok1.kind(), TestKind::Num);
  let tok2 = lexer.lex().unwrap().unwrap();
  assert_eq!(tok2.kind(), TestKind::Plus);
  let tok3 = lexer.lex().unwrap().unwrap();
  assert_eq!(tok3.kind(), TestKind::Num);
  assert!(lexer.lex().is_none());
}

#[test]
fn logos_lexer_source() {
  let mut lexer = TestLexer::new("hello");
  // Need to lex at least once to have a valid source reference
  assert_eq!(lexer.source(), "hello");
}

#[test]
fn logos_lexer_state() {
  let lexer = TestLexer::new("42");
  let _state: &() = lexer.state();
}

#[test]
fn logos_lexer_state_mut() {
  let mut lexer = TestLexer::new("42");
  let _state: &mut () = lexer.state_mut();
}

#[test]
fn logos_lexer_into_state() {
  let lexer = TestLexer::new("42");
  let _state: () = lexer.into_state();
}

#[test]
fn logos_lexer_check() {
  let lexer = TestLexer::new("42");
  assert!(lexer.check().is_ok());
}

#[test]
fn logos_lexer_span() {
  let mut lexer = TestLexer::new("42 + 1");
  let _ = lexer.lex(); // consume "42"
  let span = lexer.span();
  assert_eq!(span.start(), 0);
  assert_eq!(span.end(), 2);
}

#[test]
fn logos_lexer_slice() {
  let mut lexer = TestLexer::new("42 + 1");
  let _ = lexer.lex(); // consume "42"
  assert_eq!(lexer.slice(), "42");
}

#[test]
fn logos_lexer_bump() {
  let mut lexer = TestLexer::new("42 + 1");
  lexer.bump(&1);
  let _ = lexer;
}

#[test]
fn logos_lexer_inner() {
  let lexer = TestLexer::new("42");
  let _inner = lexer.inner();
}

#[test]
fn logos_lexer_inner_mut() {
  let mut lexer = TestLexer::new("42");
  let _inner = lexer.inner_mut();
}

#[test]
fn logos_lexer_into_inner() {
  let lexer = TestLexer::new("42");
  let _inner = lexer.into_inner();
}

#[test]
fn logos_lexer_into_lexer_trait() {
  use super::super::IntoLexer;
  use crate::logos::Logos;
  let raw_lexer = TestTok::lexer("42");
  let _logos_lexer: TestLexer<'_> = raw_lexer.into_lexer();
}

#[test]
fn logos_lexer_from_logos_identity() {
  use super::FromLogos;
  let tok = TestTok::Plus;
  let converted = TestTok::from_logos(tok.clone());
  assert_eq!(converted, tok);
}

// ── Limit-error latching ─────────────────────────────────────────────────

use crate::state::token_tracker::{TokenLimitExceeded, TokenLimiter};

#[derive(Debug, Clone, PartialEq)]
enum LimitErr {
  Lex,
  Limit(TokenLimitExceeded),
}

impl From<()> for LimitErr {
  fn from(_: ()) -> Self {
    LimitErr::Lex
  }
}

impl From<TokenLimitExceeded> for LimitErr {
  fn from(e: TokenLimitExceeded) -> Self {
    LimitErr::Limit(e)
  }
}

#[derive(Debug, Clone, PartialEq, logos::Logos)]
#[logos(crate = logos, extras = TokenLimiter, skip r"[ \t\r\n]+")]
enum LimitedTok {
  // Each scanned token bumps the limiter; the over-limit condition is caught by
  // `LogosLexer::lex` via `check()`, not by the callback itself.
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); })]
  Num,
}

impl core::fmt::Display for LimitedTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LimitedKind {
  Num,
}

impl core::fmt::Display for LimitedKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl TokenTrait<'_> for LimitedTok {
  type Kind = LimitedKind;
  type Error = LimitErr;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> LimitedKind {
    LimitedKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type LimitedLexer<'a> = super::LogosLexer<'a, LimitedTok>;

#[test]
fn logos_lexer_latches_after_limit_error() {
  // Limit of 2: the third scanned token trips `check()`.
  let mut lexer = LimitedLexer::with_state("1 2 3 4 5 6", TokenLimiter::with_limitation(2));

  assert!(matches!(lexer.lex(), Some(Ok(_))), "first token");
  assert!(matches!(lexer.lex(), Some(Ok(_))), "second token");

  // Third token trips the limiter: exactly one limit error is returned.
  assert!(
    matches!(lexer.lex(), Some(Err(LimitErr::Limit(_)))),
    "limit error on the tripping token"
  );

  let tokens_at_trip = lexer.state().tokens();
  assert_eq!(
    tokens_at_trip, 3,
    "three tokens were scanned before latching"
  );

  // Latched: every subsequent `lex()` is `None` and NO further scanning happens
  // (the counting callback proves bounded work — the count never advances).
  for _ in 0..5 {
    assert!(lexer.lex().is_none(), "latched to EOF");
  }
  assert_eq!(
    lexer.state().tokens(),
    tokens_at_trip,
    "no further tokens scanned after the latch"
  );
}

#[test]
fn logos_lexer_latch_inherited_by_lex_spanned() {
  use super::super::Lexed;

  // The `lex_spanned`/iterator surface routes through `lex`, so it inherits the latch.
  let mut lexer = LimitedLexer::with_state("1 2 3 4 5", TokenLimiter::with_limitation(2));

  let mut errors = 0usize;
  let mut last_was_error = false;
  while let Some(spanned) = Lexed::lex_spanned(&mut lexer) {
    let (_, lexed) = spanned.into_components();
    last_was_error = lexed.is_error();
    if last_was_error {
      errors += 1;
    }
  }

  assert_eq!(
    errors, 1,
    "exactly one limit error surfaced via lex_spanned"
  );
  assert!(
    last_was_error,
    "iteration stopped right after the limit error"
  );
  assert_eq!(
    lexer.state().tokens(),
    3,
    "bounded work: scanning stopped at the trip point"
  );
}

// ── A logos error that COINCIDES with a state trip (#267) ────────────────
//
// The section above is the whole precedence question asked of a *token*: the item scanned
// cleanly, `check()` failed after it, and the state error replaces it. A logos callback is
// equally free to mutate `extras` and then return `Err` for the SAME matched item, and that
// item is a completed scan too — the tally moved. The defect this fixture pins is the
// asymmetry: a post-scan check placed inside the token arm treats a lexer error as if nothing
// had been scanned, so the two events happening at once report the *lexer* error, latch
// nothing, and let a recovery loop go on scanning past the configured limit.
//
// SIMULTANEITY IS THE DEFECT'S NAME. Each half alone is already covered — a clean token that
// trips is `logos_lexer_latches_after_limit_error`, and a lexer error that trips nothing is
// `logos_error_without_a_trip_stays_recoverable` below — and each half alone passes under the
// defect. Only the cell where both land on one item separates the two implementations.

/// The precedence fixture's error type. It is also the `logos(error = ...)` type, because the
/// witness needs a callback that can *return* one, and it keeps the two classes apart by value
/// so a test can say which of them was reported rather than only that something failed.
#[derive(Debug, Clone, Default, PartialEq)]
enum TripErr {
  #[default]
  Lex,
  Limit(TokenLimitExceeded),
  Incomplete,
}

impl From<TokenLimitExceeded> for TripErr {
  fn from(e: TokenLimitExceeded) -> Self {
    TripErr::Limit(e)
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized>
  From<crate::error::token::UnexpectedToken<'a, T, Kind, S, Lang>> for TripErr
{
  fn from(_: crate::error::token::UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    TripErr::Lex
  }
}

impl<O, Lang: ?Sized> From<crate::error::UnexpectedEot<O, Lang>> for TripErr {
  fn from(_: crate::error::UnexpectedEot<O, Lang>) -> Self {
    TripErr::Lex
  }
}

impl From<crate::error::Incomplete<usize>> for TripErr {
  fn from(_: crate::error::Incomplete<usize>) -> Self {
    TripErr::Incomplete
  }
}

impl crate::error::MaybeIncomplete for TripErr {
  fn is_incomplete(&self) -> bool {
    matches!(self, TripErr::Incomplete)
  }
}

#[derive(Debug, Clone, PartialEq, logos::Logos)]
#[logos(crate = logos, extras = TokenLimiter, error = TripErr, skip r"[ \t\r\n]+")]
enum TripTok {
  /// THE WITNESS. One legal callback that both bumps the limiter and fails for the same
  /// matched item, which is the coincidence the adapter has to rank.
  #[regex(r"x", |lex| { lex.extras.increase(); Err::<(), TripErr>(TripErr::Lex) })]
  X,

  /// A lexer error that leaves the state alone — the control for "a raw logos error whose
  /// `check()` is still `Ok` stays an ordinary, recoverable error".
  #[regex(r"e", |_| { Err::<(), TripErr>(TripErr::Lex) })]
  E,

  /// A clean item that only counts, for arranging a trip without a lexer error.
  #[regex(r"n", |lex| { lex.extras.increase(); })]
  N,
}

impl core::fmt::Display for TripTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{}", self.kind())
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TripKind {
  X,
  E,
  N,
}

impl core::fmt::Display for TripKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      TripKind::X => "x",
      TripKind::E => "e",
      TripKind::N => "n",
    })
  }
}

impl TokenTrait<'_> for TripTok {
  type Kind = TripKind;
  type Error = TripErr;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> TripKind {
    match self {
      TripTok::X => TripKind::X,
      TripTok::E => TripKind::E,
      TripTok::N => TripKind::N,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type TripLexer<'a> = super::LogosLexer<'a, TripTok>;

/// Drains a lexer to exhaustion (or to its latch), returning what each call answered and the
/// scan tally the callbacks left behind. `cap` bounds the drain so an unlatched lexer under a
/// tripped limit — the defective behaviour — reports its unbounded scanning as a long list
/// rather than hanging the suite.
fn drain(lexer: &mut TripLexer<'_>, cap: usize) -> (std::vec::Vec<Option<TripErr>>, usize) {
  let mut seen = std::vec::Vec::new();
  for _ in 0..cap {
    match lexer.lex() {
      None => {
        seen.push(None);
        break;
      }
      Some(Ok(_)) => seen.push(Some(TripErr::default())),
      Some(Err(e)) => seen.push(Some(e)),
    }
  }
  let tokens = lexer.state().tokens();
  (seen, tokens)
}

#[test]
fn logos_error_and_simultaneous_trip_returns_the_state_error_and_latches() {
  // The audit's own witness, spelled as it filed it: source `"xx"`, limit zero, a callback that
  // increments extras past the limit and returns a raw logos error for the same matched `x`.
  //
  // What it observed at the audited revision, and what this cell now refuses:
  //
  //   lex #1       -> Some(Err(TripErr::Lex))     <- the raw logos error, state never consulted
  //   state.check  -> Err(TokenLimitExceeded)     <- tripped, and nothing latched
  //   lex #2       -> Some(Err(TripErr::Lex))     <- scanning continues past the limit
  //   scan count   -> 2
  let mut lexer = TripLexer::with_state("xx", TokenLimiter::with_limitation(0));

  let first = lexer.lex();
  assert!(
    matches!(first, Some(Err(TripErr::Limit(_)))),
    "the tripped state outranks the raw logos error it arrived with, got {first:?}"
  );

  let tokens_at_trip = lexer.state().tokens();
  assert_eq!(tokens_at_trip, 1, "exactly one item was scanned");

  for _ in 0..5 {
    assert_eq!(
      lexer.lex(),
      None,
      "latched to EOF, exactly as the token arm latches"
    );
  }
  assert_eq!(
    lexer.state().tokens(),
    tokens_at_trip,
    "bounded work: no further scanning after the latch"
  );
}

#[test]
fn logos_error_without_a_trip_stays_recoverable() {
  // The other half of the ranking, and the reason the repair cannot be "latch on any error":
  // an `e` fails without touching the limiter, so `check()` is `Ok` and the raw logos error is
  // forwarded unchanged. A recovery caller sees every one of them and end of input after.
  let mut lexer = TripLexer::with_state("ee", TokenLimiter::with_limitation(usize::MAX));
  let (seen, tokens) = drain(&mut lexer, 8);

  assert_eq!(
    seen,
    std::vec![Some(TripErr::Lex), Some(TripErr::Lex), None],
    "two ordinary lexer errors, then a genuine end of input — no latch"
  );
  assert_eq!(tokens, 0, "neither error touched the limiter");
}

#[test]
fn a_pre_tripped_state_outranks_a_raw_logos_error() {
  // The trip need not happen ON the failing item: a state already over its limit when the scan
  // begins is the same terminal fact, so the first completed scan reports it whatever that scan
  // produced. Here the item is an `e`, whose callback never touches extras — so the ONLY thing
  // that can turn this into a limit error is a post-scan check that runs on the error arm.
  let mut lexer = TripLexer::with_state("ee", TokenLimiter::with_limitation(0));
  lexer.state_mut().increase();
  assert!(lexer.check().is_err(), "the fixture starts out tripped");

  let (seen, _) = drain(&mut lexer, 8);
  assert!(
    matches!(seen.as_slice(), [Some(TripErr::Limit(_)), None]),
    "the state error is reported once and the lexer latches, got {seen:?}"
  );
}

#[test]
fn a_clean_token_that_trips_is_unchanged() {
  // The existing control, re-run on THIS fixture so the two arms are compared over one lexer:
  // `n` scans cleanly and trips, and that has always reported the state error. A repair that
  // only moved the check must leave this exactly where it was.
  let mut lexer = TripLexer::with_state("nn", TokenLimiter::with_limitation(0));
  let (seen, tokens) = drain(&mut lexer, 8);

  assert!(
    matches!(seen.as_slice(), [Some(TripErr::Limit(_)), None]),
    "a token that trips reports the state error and latches, got {seen:?}"
  );
  assert_eq!(tokens, 1, "bounded work on the token arm too");
}

// ── The same precedence, seen through `InputRef` ─────────────────────────
//
// `InputRef` rebuilds a lexer per operation, so it never inherits the adapter's own latch: it
// asks `check()` itself and records a poison boundary. That half was already right. What it
// could not do was report the *value*, because it builds its trip verdict out of the error
// `lex` already returned — so a trip that arrived wearing a logos error was latched correctly
// and diagnosed wrongly. These two cells read the retained diagnostic rather than the boundary
// for exactly that reason.

type TripCtx<'a> = (
  crate::emitter::Verbose<TripErr>,
  crate::cache::DefaultCache<'a, TripLexer<'a>>,
);

fn tripping_input(
  src: &str,
  limit: usize,
) -> crate::input::Input<'_, TripLexer<'_>, TripCtx<'_>, ()> {
  crate::input::Input::with_state_and_context(
    src,
    TokenLimiter::with_limitation(limit),
    crate::input::InputContext::new(
      crate::emitter::Verbose::<TripErr>::new(),
      crate::cache::DefaultCache::<'_, TripLexer<'_>>::default(),
    ),
  )
}

/// `(ordinary lexer diagnostics, limit diagnostics)` retained by the verbose emitter.
fn retained<'a, Cmpl>(
  input: &crate::input::Input<'a, TripLexer<'a>, TripCtx<'a>, (), Cmpl>,
) -> (usize, usize)
where
  Cmpl: crate::input::Completeness,
{
  let all = input.emitter().errors();
  let lex = all
    .values()
    .flatten()
    .filter(|e| matches!(e, TripErr::Lex))
    .count();
  let limit = all
    .values()
    .flatten()
    .filter(|e| matches!(e, TripErr::Limit(_)))
    .count();
  (lex, limit)
}

#[test]
fn a_complete_scan_diagnoses_the_trip_with_the_state_error() {
  let mut input = tripping_input("xx", 0);
  {
    let mut inp = input.as_ref();
    let before = inp.scanner_trip_snapshot();
    assert_eq!(
      inp.next(),
      Ok(None),
      "the trip is terminal: the scan ends rather than yielding a token"
    );
    assert!(
      inp.scanner_tripped_during_attempt(before),
      "and the terminal probe fired, so the poison boundary latched"
    );
  }
  assert_eq!(
    retained(&input),
    (0, 1),
    "the terminal diagnostic is the STATE error, not the logos error the item arrived with"
  );
}

#[test]
fn a_partial_frontier_diagnoses_the_trip_with_the_state_error() {
  // The frontier case the terminal-first ranking exists for, now with the payload attached:
  // the tripping item ends exactly at a non-final buffer end, where the holdback would
  // otherwise withhold it as `Incomplete`. It is reported as a trip — carrying the state error.
  let mut input = crate::input::Input::<
    TripLexer<'_>,
    TripCtx<'_>,
    (),
    crate::input::Partial,
  >::with_state_and_context(
    "x",
    TokenLimiter::with_limitation(0),
    crate::input::InputContext::new(
      crate::emitter::Verbose::<TripErr>::new(),
      crate::cache::DefaultCache::<'_, TripLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();
    let before = inp.scanner_trip_snapshot();
    assert_eq!(
      inp.next(),
      Ok(None),
      "terminal beats incomplete: the trip is not withheld for more bytes"
    );
    assert!(
      inp.scanner_tripped_during_attempt(before),
      "and the terminal probe fired at the frontier"
    );
  }
  assert_eq!(
    retained(&input),
    (0, 1),
    "the frontier path reports the state payload too"
  );
}

// ── Provenance: `next()` is not a scan ───────────────────────────────────
//
// The value channel is read off `extras` after `inner.next()` returns, and the temptation is to
// treat "recorded during this call" as "recorded for this item". It is not. `logos` handles
// `Skip` INSIDE the same `next()` — `CallbackResult for Skip` does `lex.trivia(); T::lex(lex)`
// recursively on 0.14/0.15, and 0.16's `_take_action` does `$lex.trivia(); ... continue` — so one
// call can run several DFA scans and several callbacks before it returns an item.
//
// A trivia callback therefore records for a scan that produced NOTHING, and the scan that does
// produce the item may run no callback at all. Freshness cannot tell those apart; the trivia's
// value survives and is read as the item's. Because the skipped scan precedes the item, its
// offset is BELOW the item's real DFA frontier — the one direction that under-reports, which is
// the direction that lets an unstable item be committed.
//
// So a recorded value carries WHICH SCAN recorded it, and the adapter accepts it only for the
// item that scan produced. The key is the scan's start offset: inside a callback `lex.span()`
// is the current match, and `token_start` is reset by `trivia()` before each rescan, so a
// recorder has it for free — and after `next()` the adapter has the returned item's span.

/// Records `(scan start, probed-to)` exactly the way the doc tells a recorder to.
#[derive(Debug, Clone, Default, PartialEq)]
struct PeekRecorder {
  probe: Option<(usize, usize)>,
}

impl crate::State for PeekRecorder {
  type Error = TripErr;

  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }

  fn take_probe(&mut self) -> Option<crate::Probe> {
    self
      .probe
      .take()
      .map(|(from, to)| crate::Probe::new(from, to))
  }
}

/// The recorder body both callbacks below share: absolute offsets, keyed by the current match's
/// start, which is what `lex.span()` reports inside a callback.
///
/// `beyond` counts offsets **past the terminator**, not bytes of lookahead. `span.end` is the
/// first offset outside the half-open match, so `beyond == 0` records `span.end` — the
/// one-boundary-byte probe a maximal-munch scan makes — and `beyond == 1` records one offset
/// further. Reading it as a byte count is the off-by-one `Probe::new` now spells out.
fn record(lex: &mut logos::Lexer<'_, TriviaTok>, beyond: usize) {
  let span = lex.span();
  lex.extras.probe = Some((span.start, span.end + beyond));
}

#[derive(Debug, Clone, PartialEq, logos::Logos)]
#[logos(crate = logos, extras = PeekRecorder, error = TripErr)]
enum TriviaTok {
  /// **The witness.** Trivia with a callback: it records and returns `Skip`, so the scan that
  /// records is not the scan whose item `next()` returns. It records only its own end, which is
  /// strictly below the frontier of anything scanned after it.
  #[regex(r"[ \t]+", |lex| { record(lex, 0); logos::Skip })]
  Ws,

  /// The prefix-backtracking pair, both **callback-free**: deciding `Int` probes into the
  /// `Float` arm and rolls back to the accepting prefix, and nothing records that.
  #[regex(r"[0-9]+")]
  Int,
  #[regex(r"[0-9]+\.[0-9]+")]
  Float,
  #[token(".")]
  Dot,

  /// The accept path: a token whose OWN scan records. Provenance must not cost this precision.
  #[regex(r"[a-z]+", |lex| { record(lex, 1); })]
  Word,
}

impl core::fmt::Display for TriviaTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{}", self.kind())
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TriviaKind {
  Ws,
  Int,
  Float,
  Dot,
  Word,
}

impl core::fmt::Display for TriviaKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      TriviaKind::Ws => "whitespace",
      TriviaKind::Int => "int",
      TriviaKind::Float => "float",
      TriviaKind::Dot => "dot",
      TriviaKind::Word => "word",
    })
  }
}

impl TokenTrait<'_> for TriviaTok {
  type Kind = TriviaKind;
  type Error = TripErr;

  // `Unbounded` deliberately: it is the answer an item whose own scan recorded nothing must
  // fall back to, and every reject-path cell below reads it. There is no default to fall into
  // any more, so this line is the claim rather than the absence of one.
  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> TriviaKind {
    match self {
      TriviaTok::Ws => TriviaKind::Ws,
      TriviaTok::Int => TriviaKind::Int,
      TriviaTok::Float => TriviaKind::Float,
      TriviaTok::Dot => TriviaKind::Dot,
      TriviaTok::Word => TriviaKind::Word,
    }
  }

  fn is_trivia(&self) -> bool {
    matches!(self, TriviaTok::Ws)
  }
}

type TriviaLexer<'a> = super::LogosLexer<'a, TriviaTok>;

#[test]
fn a_skipped_scans_probe_does_not_answer_for_the_token_that_follows_it() {
  // `"  1."`. One `lex()` call runs two scans: the `Ws` callback records `(0, 2)` and skips,
  // then the DFA rescans from 2, probes into the `Float` arm, hits end of input at 4, and
  // backtracks to `Int@2..3`. That second scan ran no callback, so nothing was recorded FOR IT
  // and the honest answer is the vocabulary's class claim.
  //
  // Under the per-`next()` clear the trivia's `2` survives and is read as the integer's
  // frontier — below the item's real DFA frontier, which is the unsound direction.
  let mut lexer = TriviaLexer::new("  1.");
  assert_eq!(lexer.lex(), Some(Ok(TriviaTok::Int)));
  assert_eq!((lexer.span().start(), lexer.span().end()), (2, 3));
  assert_eq!(
    lexer.read_frontier(),
    crate::ReadFrontier::Unbounded,
    "the skipped trivia's probe belongs to the trivia, not to the integer scanned after it"
  );
}

#[test]
fn a_skipped_scans_probe_does_not_answer_for_the_error_that_follows_it() {
  // The error arm, for the reason the post-scan `check()` covers both arms: a callback can
  // mutate `extras` and the item can still arrive as an `Err`. Here the recording callback runs
  // on the trivia and the item is a raw logos error for `#`, whose scan ran no callback at all.
  let mut lexer = TriviaLexer::new("  #");
  assert_eq!(lexer.lex(), Some(Err(TripErr::Lex)));
  assert_eq!((lexer.span().start(), lexer.span().end()), (2, 3));
  assert_eq!(
    lexer.read_frontier(),
    crate::ReadFrontier::Unbounded,
    "an item that arrives as an error is a completed scan too, and it did not record this"
  );
}

#[test]
fn a_probe_the_items_own_scan_recorded_still_answers_for_it() {
  // The accept path, and the control that keeps the two cells above from passing because the
  // adapter simply stopped reading the channel. Same recording trivia in front, but the `Word`
  // scan records for ITSELF — start 2, matching the item's span start — so the value answers.
  let mut lexer = TriviaLexer::new("  ab");
  assert_eq!(lexer.lex(), Some(Ok(TriviaTok::Word)));
  assert_eq!((lexer.span().start(), lexer.span().end()), (2, 4));
  assert_eq!(
    lexer.read_frontier(),
    crate::ReadFrontier::ReadTo(5),
    "a value recorded by the scan that produced the item is exactly what the channel is for"
  );
}

type TriviaCtx<'a> = (
  crate::emitter::Verbose<TripErr>,
  crate::cache::DefaultCache<'a, TriviaLexer<'a>>,
);

/// Drains a partial input over `src` with the trivia vocabulary, returning the kinds the driver
/// actually YIELDED beside the result that stopped it.
fn run_trivia_partial(
  src: &str,
  is_final: bool,
) -> (std::vec::Vec<TriviaKind>, Result<Option<()>, TripErr>) {
  run_trivia_partial_from(src, PeekRecorder::default(), is_final)
}

/// [`run_trivia_partial`] starting from a state that already carries a value — the shape
/// `InputRef::resume_from` produces, where a recording made by some earlier scan travels in the
/// state into a lexer that never ran it.
fn run_trivia_partial_from(
  src: &str,
  state: PeekRecorder,
  is_final: bool,
) -> (std::vec::Vec<TriviaKind>, Result<Option<()>, TripErr>) {
  let mut input = crate::input::Input::<
    TriviaLexer<'_>,
    TriviaCtx<'_>,
    (),
    crate::input::Partial,
  >::with_state_and_context(
    src,
    state,
    crate::input::InputContext::new(
      crate::emitter::Verbose::<TripErr>::new(),
      crate::cache::DefaultCache::<'_, TriviaLexer<'_>>::default(),
    ),
  );
  if is_final {
    input.seal();
  }
  let mut inp = input.as_ref();
  let mut kinds = std::vec::Vec::new();
  let result = loop {
    match inp.next() {
      Ok(Some(t)) => kinds.push(t.data().kind()),
      Ok(None) => break Ok(None),
      Err(e) => break Err(e),
    }
  };
  (kinds, result)
}

#[test]
fn the_driver_withholds_the_item_a_skipped_scans_probe_would_have_released() {
  // The same defect one level up, where it is an unsoundness rather than a wrong answer.
  //
  // Non-final `"  1."`, buffer end 4. `Int@2..3` reports the trivia's stale `2`; the driver
  // floors that at the item's own span end, 3, and `3 < 4` COMMITS it. Append `5` and the same
  // bytes are one `Float@2..5` — chunked equivalence broken, on exactly the trivia-skipping
  // path the protocol claims to cover.
  assert_eq!(
    run_trivia_partial("  1.", false),
    (std::vec![], Err(TripErr::Incomplete)),
    "nothing may be committed out of a buffer whose last bytes are still being read"
  );

  // The append that proves the withholding was right and not merely conservative.
  assert_eq!(
    run_trivia_partial("  1.5", true).0,
    std::vec![TriviaKind::Float],
    "one byte more and the committed integer was never an integer"
  );
}

#[test]
fn a_probe_left_by_a_scan_that_produced_no_item_does_not_survive_the_next_call() {
  // `"  "` is all trivia: the callback records `(0, 2)`, the rescan hits end of input, and
  // `lex()` returns `None` with a value still sitting in the state. No item was produced, so
  // nothing asks `read_frontier` about it — but the leftover must not be able to reach a later
  // item either. The `take_probe` before each scan is what guarantees that, and it is the guard
  // provenance cannot supply: a rebuilt lexer positioned by `bump` can start an item at exactly
  // the offset a stale value was keyed to.
  let mut lexer = TriviaLexer::new("  ");
  assert_eq!(lexer.lex(), None);
  assert_eq!(lexer.state().probe, Some((0, 2)), "the trivia did record");
  assert_eq!(lexer.lex(), None);
  assert_eq!(
    lexer.state().probe,
    None,
    "the next scan cleared it before running, so no later item can inherit it"
  );
}

#[test]
fn a_probe_restored_with_the_state_cannot_answer_for_a_rebuilt_lexers_first_item() {
  // Why the pre-scan take is kept rather than retired as redundant, spelled as a falsifier.
  //
  // The input layer rebuilds a lexer through `with_state` + `bump`
  // (`InputRef::resume_from`), so a value recorded by some earlier scan — in another lexer, at
  // another buffer length — travels in the state. An equal start is only *evidence* of
  // provenance, and here it is misleading evidence: the state claims a scan from 0 probed to 1,
  // and the rebuilt lexer's first item really does begin at 0, so the check would accept it.
  //
  // `Int@0..1` in `"1."` is decided by probing offset 2 and finding end of input, so accepting
  // the restored `1` is the same under-report the skipped-trivia cells above pin. Taking the
  // value before the scan removes it before any start can be matched against it.
  let mut lexer = TriviaLexer::with_state(
    "1.",
    PeekRecorder {
      probe: Some((0, 1)),
    },
  );
  assert_eq!(lexer.lex(), Some(Ok(TriviaTok::Int)));
  assert_eq!((lexer.span().start(), lexer.span().end()), (0, 1));
  assert_eq!(
    lexer.read_frontier(),
    crate::ReadFrontier::Unbounded,
    "the scan that produced this item recorded nothing — the value arrived with the state"
  );
}

#[test]
fn a_probe_restored_with_the_state_cannot_release_an_item_out_of_a_growing_buffer() {
  // The cell above one level up, where the same accepted value is an unsoundness rather than a
  // wrong answer — and the reason the channel is ONE consuming operation rather than a reader
  // beside a reset.
  //
  // While those were two independently defaulted members, a `State` could override the reader and
  // silently inherit the empty reset. The adapter then called a reset that did nothing, the scan
  // recorded nothing, and provenance ACCEPTED the restored value because the starts matched. A
  // recorded value answers the frontier contract outright, so the vocabulary's honest `Unbounded`
  // was never consulted: `read_frontier` returned `ReadTo(1)`, the driver floored it at the item's
  // own span end — still 1 — and `1 < 2` committed `Int` out of a buffer still being read. One
  // appended byte and the same offsets are `Float@0..3`.
  //
  // `State::take_probe` consumes, so the pre-scan take IS the reset and there is no sibling to
  // inherit. This cell is what fails if that take is ever dropped from `lex`.
  assert_eq!(
    run_trivia_partial_from(
      "1.",
      PeekRecorder {
        probe: Some((0, 1)),
      },
      false,
    ),
    (std::vec![], Err(TripErr::Incomplete)),
    "a value recorded before this lexer existed may not release an item out of a growing buffer"
  );

  // The append that proves the withholding was right and not merely conservative.
  assert_eq!(
    run_trivia_partial_from(
      "1.5",
      PeekRecorder {
        probe: Some((0, 1)),
      },
      true,
    )
    .0,
    std::vec![TriviaKind::Float],
    "one byte more and the committed integer was never an integer"
  );
}
