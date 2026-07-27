#![cfg(all(feature = "std", feature = "logos"))]

//! Additional parser combinator tests targeting uncovered code paths.
//!
//! Exercises `expect` (error paths), `then` variants, `peek_then`,
//! `opt`, `and_then`, and various combinator chains.

mod common;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
  error::{UnexpectedEot, token::UnexpectedToken},
  parser::{Any, expect, try_expect},
  try_parse_input::ParseAttempt,
  utils::Expected,
};

use common::{TestLexer, Token, TokenKind};

// ── expect with error reporting ─────────────────────────────────────────────

/// Parse expecting a specific token kind, producing proper error on mismatch.
fn parse_expect_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Token, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  expect(|t: &Token| {
    if matches!(t, Token::Num(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Num))
    }
  })
  .parse_input(inp)
}

/// Parse expecting a Plus token.
fn parse_expect_plus<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Token, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  expect(|t: &Token| {
    if matches!(t, Token::Plus) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Plus))
    }
  })
  .parse_input(inp)
}

/// Try-expect a Num token (non-consuming on decline).
fn try_expect_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<Token>, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  try_expect(|t: &Token| matches!(t, Token::Num(_))).try_parse_input(inp)
}

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ExpError;

impl From<()> for ExpError {
  fn from(_: ()) -> Self {
    ExpError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for ExpError {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ExpError
  }
}

impl<S, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<S, Lang, Set>> for ExpError {
  fn from(_: UnexpectedEot<S, Lang, Set>) -> Self {
    ExpError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for ExpError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    ExpError
  }
}

// ── expect tests ────────────────────────────────────────────────────────────

#[test]
fn test_expect_match() {
  let r: Result<Token, ExpError> = Parser::new().apply(parse_expect_num).parse_str("42");
  assert!(matches!(r.unwrap(), Token::Num(42)));
}

#[test]
fn test_expect_mismatch() {
  // Expect Num but get Plus → UnexpectedToken error
  let r: Result<Token, ExpError> = Parser::new().apply(parse_expect_num).parse_str("+");
  assert!(r.is_err());
}

#[test]
fn test_expect_eot() {
  // Expect Num but input is empty → UnexpectedEot error
  let r: Result<Token, ExpError> = Parser::new().apply(parse_expect_num).parse_str("");
  assert!(r.is_err());
}

// ── try_expect tests ────────────────────────────────────────────────────────

#[test]
fn test_try_expect_accept() {
  let r: Result<ParseAttempt<Token>, ExpError> =
    Parser::new().apply(try_expect_num).parse_str("42");
  assert!(matches!(r.unwrap(), ParseAttempt::Accept(Token::Num(42))));
}

#[test]
fn test_try_expect_decline() {
  let r: Result<ParseAttempt<Token>, ExpError> = Parser::new().apply(try_expect_num).parse_str("+");
  assert!(matches!(r.unwrap(), ParseAttempt::Decline));
}

#[test]
fn test_try_expect_empty() {
  let r: Result<ParseAttempt<Token>, ExpError> = Parser::new().apply(try_expect_num).parse_str("");
  assert!(matches!(r.unwrap(), ParseAttempt::Decline));
}

// ── then combinator tests ───────────────────────────────────────────────────

/// Parse two nums: first then second.
fn parse_then_nums<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(Token, Token), ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_num.then(parse_expect_num).parse_input(inp)
}

#[test]
fn test_then_success() {
  let r: Result<(Token, Token), ExpError> = Parser::new().apply(parse_then_nums).parse_str("1 2");
  let (a, b) = r.unwrap();
  assert!(matches!(a, Token::Num(1)));
  assert!(matches!(b, Token::Num(2)));
}

#[test]
fn test_then_second_fails() {
  let r: Result<(Token, Token), ExpError> = Parser::new().apply(parse_then_nums).parse_str("1 +");
  assert!(r.is_err());
}

/// Parse num then ignore plus.
fn parse_then_ignore_plus<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Token, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_num
    .then_ignore(parse_expect_plus)
    .parse_input(inp)
}

#[test]
fn test_then_ignore_success() {
  let r: Result<Token, ExpError> = Parser::new()
    .apply(parse_then_ignore_plus)
    .parse_str("42 +");
  assert!(matches!(r.unwrap(), Token::Num(42)));
}

#[test]
fn test_then_ignore_fail() {
  let r: Result<Token, ExpError> = Parser::new()
    .apply(parse_then_ignore_plus)
    .parse_str("42 -");
  assert!(r.is_err());
}

/// Ignore plus then parse num.
fn parse_ignore_then_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Token, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_plus
    .ignore_then(parse_expect_num)
    .parse_input(inp)
}

#[test]
fn test_ignore_then_success() {
  let r: Result<Token, ExpError> = Parser::new().apply(parse_ignore_then_num).parse_str("+ 42");
  assert!(matches!(r.unwrap(), Token::Num(42)));
}

#[test]
fn test_ignore_then_fail() {
  let r: Result<Token, ExpError> = Parser::new().apply(parse_ignore_then_num).parse_str("- 42");
  assert!(r.is_err());
}

// ── then_value tests ────────────────────────────────────────────────────────

fn parse_plus_value<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<&'static str, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_plus
    .then_value(|| "plus_found")
    .parse_input(inp)
}

#[test]
fn test_then_value() {
  let r: Result<&str, ExpError> = Parser::new().apply(parse_plus_value).parse_str("+");
  assert_eq!(r.unwrap(), "plus_found");
}

// ── and_then combinator ─────────────────────────────────────────────────────

fn parse_and_then_positive<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_num
    .and_then(|t| match t {
      Token::Num(n) if n > 0 => Ok(n),
      _ => Err(ExpError),
    })
    .parse_input(inp)
}

#[test]
fn test_and_then_pass() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_and_then_positive).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn test_and_then_reject() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_and_then_positive).parse_str("-5");
  assert!(r.is_err());
}

// ── map combinator ──────────────────────────────────────────────────────────

fn parse_num_doubled<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_num
    .map(|t| match t {
      Token::Num(n) => n * 2,
      _ => unreachable!(),
    })
    .parse_input(inp)
}

#[test]
fn test_map_transform() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_num_doubled).parse_str("21");
  assert_eq!(r.unwrap(), 42);
}

// ── Chained combinators ─────────────────────────────────────────────────────

/// Parse: num "+" num → sum
fn parse_addition<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_num
    .then_ignore(parse_expect_plus)
    .then(parse_expect_num)
    .map(|(a, b)| {
      let a = match a {
        Token::Num(n) => n,
        _ => unreachable!(),
      };
      let b = match b {
        Token::Num(n) => n,
        _ => unreachable!(),
      };
      a + b
    })
    .parse_input(inp)
}

#[test]
fn test_chained_addition() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_addition).parse_str("10 + 32");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn test_chained_addition_missing_op() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_addition).parse_str("10 32");
  assert!(r.is_err());
}

// ── filter combinator ───────────────────────────────────────────────────────

fn parse_filter_positive<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Token, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_num
    .filter(|t| {
      if matches!(t, Token::Num(n) if *n > 0) {
        Ok(())
      } else {
        Err(ExpError)
      }
    })
    .parse_input(inp)
}

#[test]
fn test_filter_pass() {
  let r: Result<Token, ExpError> = Parser::new().apply(parse_filter_positive).parse_str("42");
  assert!(matches!(r.unwrap(), Token::Num(42)));
}

#[test]
fn test_filter_reject() {
  let r: Result<Token, ExpError> = Parser::new().apply(parse_filter_positive).parse_str("-5");
  assert!(r.is_err());
}

// ── filter_map combinator ───────────────────────────────────────────────────

fn parse_filter_map_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  Any::new()
    .filter_map(|t| match t {
      Token::Num(n) => Ok(n),
      _ => Err(ExpError),
    })
    .parse_input(inp)
}

#[test]
fn test_filter_map_match() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_filter_map_num).parse_str("99");
  assert_eq!(r.unwrap(), 99);
}

#[test]
fn test_filter_map_no_match() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_filter_map_num).parse_str("+");
  assert!(r.is_err());
}

// ── validate combinator ─────────────────────────────────────────────────────

fn parse_validate_range<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, ExpError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ExpError>,
{
  parse_expect_num
    .map(|t| match t {
      Token::Num(n) => n,
      _ => unreachable!(),
    })
    .validate(|n| {
      if *n >= 0 && *n <= 100 {
        Ok(())
      } else {
        Err(ExpError)
      }
    })
    .parse_input(inp)
}

#[test]
fn test_validate_pass() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_validate_range).parse_str("50");
  assert_eq!(r.unwrap(), 50);
}

#[test]
fn test_validate_reject() {
  let r: Result<i64, ExpError> = Parser::new().apply(parse_validate_range).parse_str("200");
  assert!(r.is_err());
}
