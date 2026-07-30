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
