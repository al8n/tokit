#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Basic parser combinator tests.
//!
//! Exercises `any`, `expect`, `map`, `filter_map`, `filter`, `validate`,
//! `then`, `then_ignore`, `ignore_then`, `then_value`, `and_then`, `opt`,
//! `fold`, `peek_then`, and friends.

mod common;

use common::E;

use tokora::{
  Accumulator, DefaultCache, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser,
  ParserContext, Token as TokenTrait, TryParseInput,
  emitter::{FullContainerEmitter, TooFewEmitter, TooManyEmitter},
  input::Cursor,
  parser::{Any, Empty, fail},
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::Expected,
};

use common::{TestLexer, Token, TokenKind};

// ── Element parsers ───────────────────────────────────────────────────────────

/// Parse a single `Num` token, return the i64 value.
fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  use tokora::parser::expect;
  expect(|t: &Token| {
    if matches!(t, Token::Num(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Num))
    }
  })
  .map(|t| match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  })
  .parse_input(inp)
}

/// Try to parse a `Num` token without consuming on decline.
fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  inp
    .try_expect(|t| matches!(t.data(), Token::Num(_)))
    .map(|opt| match opt {
      None => ParseAttempt::Decline,
      Some(tok) => ParseAttempt::Accept(match tok.into_data() {
        Token::Num(n) => n,
        _ => unreachable!(),
      }),
    })
}

// ── helper parsers ────────────────────────────────────────────────────────────

fn parse_any_token<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Token, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Any::new().parse_input(inp)
}

fn parse_empty<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Empty::new().parse_input(inp)
}

fn parse_fail<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  fail(|| ()).parse_input(inp)
}

// ── any ───────────────────────────────────────────────────────────────────────

#[test]
fn test_any_num() {
  let result: Token = Parser::new()
    .apply(parse_any_token)
    .parse_str("42")
    .unwrap();
  assert!(matches!(result, Token::Num(42)));
}

#[test]
fn test_any_plus() {
  let result: Token = Parser::new().apply(parse_any_token).parse_str("+").unwrap();
  assert!(matches!(result, Token::Plus));
}

#[test]
fn test_any_fails_on_empty() {
  let result: Result<Token, ()> = Parser::new().apply(parse_any_token).parse_str("");
  assert!(result.is_err());
}

// ── expect ────────────────────────────────────────────────────────────────────

#[test]
fn test_expect_num() {
  let result: i64 = Parser::new().apply(parse_num).parse_str("42").unwrap();
  assert_eq!(result, 42);
}

#[test]
fn test_expect_negative_num() {
  let result: i64 = Parser::new().apply(parse_num).parse_str("-7").unwrap();
  assert_eq!(result, -7);
}

#[test]
fn test_expect_num_fails_on_wrong_token() {
  let result: Result<i64, ()> = Parser::new().apply(parse_num).parse_str("+");
  assert!(result.is_err());
}

#[test]
fn test_expect_num_fails_on_empty() {
  let result: Result<i64, ()> = Parser::new().apply(parse_num).parse_str("");
  assert!(result.is_err());
}

// ── empty ─────────────────────────────────────────────────────────────────────

#[test]
fn test_empty() {
  let result: () = Parser::new().apply(parse_empty).parse_str("").unwrap();
  assert_eq!(result, ());
}

// ── fail ──────────────────────────────────────────────────────────────────────

#[test]
fn test_fail_always_fails() {
  let result: Result<i64, ()> = Parser::new().apply(parse_fail).parse_str("42");
  assert!(result.is_err());
}

// ── map ───────────────────────────────────────────────────────────────────────

#[test]
fn test_map_double() {
  fn parse_doubled<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.map(|n| n * 2).parse_input(inp)
  }

  let result: i64 = Parser::new().apply(parse_doubled).parse_str("7").unwrap();
  assert_eq!(result, 14);
}

// ── filter_map ────────────────────────────────────────────────────────────────

#[test]
fn test_filter_map_positive_num() {
  fn parse_positive<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter_map(|n| if n > 0 { Ok(n) } else { Err(()) })
      .parse_input(inp)
  }

  let result: i64 = Parser::new().apply(parse_positive).parse_str("5").unwrap();
  assert_eq!(result, 5);
}

#[test]
fn test_filter_map_rejects_negative() {
  fn parse_positive<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter_map(|n| if n > 0 { Ok(n) } else { Err(()) })
      .parse_input(inp)
  }

  let result: Result<i64, ()> = Parser::new().apply(parse_positive).parse_str("-3");
  assert!(result.is_err());
}

// ── filter ────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_positive() {
  fn parse_positive<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter(|&n| if n >= 0 { Ok(()) } else { Err(()) })
      .parse_input(inp)
  }

  assert_eq!(
    Parser::new().apply(parse_positive).parse_str("10").unwrap(),
    10
  );
  assert!(Parser::new().apply(parse_positive).parse_str("-1").is_err());
}

// ── validate ──────────────────────────────────────────────────────────────────

#[test]
fn test_validate() {
  fn parse_even<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .validate(|&n| if n % 2 == 0 { Ok(()) } else { Err(()) })
      .parse_input(inp)
  }

  assert_eq!(Parser::new().apply(parse_even).parse_str("4").unwrap(), 4);
  assert!(Parser::new().apply(parse_even).parse_str("3").is_err());
}

// ── then ──────────────────────────────────────────────────────────────────────

#[test]
fn test_then_two_nums() {
  fn parse_pair<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then(parse_num).parse_input(inp)
  }

  let result: (i64, i64) = Parser::new().apply(parse_pair).parse_str("3 7").unwrap();
  assert_eq!(result, (3, 7));
}

// ── then_ignore ───────────────────────────────────────────────────────────────

#[test]
fn test_then_ignore_second() {
  fn parse_first_ignore_second<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then_ignore(parse_num).parse_input(inp)
  }

  let result: i64 = Parser::new()
    .apply(parse_first_ignore_second)
    .parse_str("3 7")
    .unwrap();
  assert_eq!(result, 3);
}

// ── ignore_then ───────────────────────────────────────────────────────────────

#[test]
fn test_ignore_then_second() {
  fn parse_ignore_first_get_second<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.ignore_then(parse_num).parse_input(inp)
  }

  let result: i64 = Parser::new()
    .apply(parse_ignore_first_get_second)
    .parse_str("1 99")
    .unwrap();
  assert_eq!(result, 99);
}

// ── then_value ────────────────────────────────────────────────────────────────

#[test]
fn test_then_value() {
  fn parse_num_then_true<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then_value(|| true).parse_input(inp)
  }

  let result: bool = Parser::new()
    .apply(parse_num_then_true)
    .parse_str("42")
    .unwrap();
  assert!(result);
}

// ── and_then ──────────────────────────────────────────────────────────────────

#[test]
fn test_and_then_double_if_positive() {
  fn parse_and_transform<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .and_then(|n| if n > 0 { Ok(n * 2) } else { Err(()) })
      .parse_input(inp)
  }

  assert_eq!(
    Parser::new()
      .apply(parse_and_transform)
      .parse_str("5")
      .unwrap(),
    10
  );
  assert!(
    Parser::new()
      .apply(parse_and_transform)
      .parse_str("-1")
      .is_err()
  );
}

// ── manual fold (via try_parse_input loop) ────────────────────────────────────

#[test]
fn test_fold_sum() {
  fn parse_sum<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut sum = 0i64;
    while let ParseAttempt::Accept(n) = try_num(inp)? {
      sum += n;
    }
    Ok(sum)
  }

  assert_eq!(
    Parser::new()
      .apply(parse_sum)
      .parse_str("1 2 3 4 5")
      .unwrap(),
    15
  );
  assert_eq!(Parser::new().apply(parse_sum).parse_str("").unwrap(), 0);
}

// ── repeated (via TryParseInput::repeated) ────────────────────────────────────

#[test]
fn test_repeated_collect() {
  fn parse_nums<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = ()> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    try_num.repeated().collect().parse_input(inp)
  }

  assert_eq!(
    Parser::new().apply(parse_nums).parse_str("1 2 3").unwrap(),
    vec![1, 2, 3]
  );
  assert_eq!(
    Parser::new().apply(parse_nums).parse_str("").unwrap(),
    vec![]
  );
}

#[test]
fn test_repeated_at_least() {
  fn parse_nums<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>,
  {
    try_num.repeated().at_least(2).collect().parse_input(inp)
  }

  assert_eq!(
    Parser::new()
      .apply(parse_nums)
      .parse_str("10 20 30")
      .unwrap(),
    vec![10, 20, 30]
  );
  // fewer than 2 should fail
  assert!(Parser::new().apply(parse_nums).parse_str("10").is_err());
}

#[test]
fn test_repeated_at_most() {
  fn parse_nums<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>,
  {
    try_num.repeated().at_most(2).collect().parse_input(inp)
  }

  assert_eq!(
    Parser::new().apply(parse_nums).parse_str("10 20").unwrap(),
    vec![10, 20]
  );
  assert_eq!(
    Parser::new().apply(parse_nums).parse_str("10").unwrap(),
    vec![10]
  );
}

// ── Parser construction ───────────────────────────────────────────────────────
// (merged from parser_construction.rs)

struct TestEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for TestEmitter {
  type Error = E;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(E)
  }

  fn emit_unexpected_token(
    &mut self,
    _: tokora::error::token::UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(E)
  }

  fn emit_error(&mut self, err: Spanned<E, <TestLexer<'inp> as Lexer<'inp>>::Span>) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(err.into_data())
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, _: u64)
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
  }
}

fn parse_first_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(E),
    },
    None => Err(E),
  }
}

#[test]
fn parser_new_and_apply() {
  let r: Result<i64, _> = Parser::new().apply(parse_first_num).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn parser_with_context() {
  let ctx: ParserContext<'_, TestLexer<'_>, TestEmitter, DefaultCache<'_, TestLexer<'_>>> =
    ParserContext::new(TestEmitter);
  let r: Result<i64, _> = Parser::with_context(ctx)
    .apply(parse_first_num)
    .parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn parser_with_parser() {
  let r: Result<i64, _> = Parser::with_parser(parse_first_num).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn parser_with_parser_and_context() {
  let ctx: ParserContext<'_, TestLexer<'_>, TestEmitter, DefaultCache<'_, TestLexer<'_>>> =
    ParserContext::new(TestEmitter);
  let r: Result<i64, _> = Parser::with_parser_and_context(parse_first_num, ctx).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn parser_deref() {
  let p = Parser::with_parser(parse_first_num);
  let _: &_ = &*p;
}

#[test]
fn parser_deref_mut() {
  let mut p = Parser::with_parser(parse_first_num);
  let _: &mut _ = &mut *p;
}
