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
