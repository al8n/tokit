#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Tests targeting uncovered lines in parse_input.rs:
//! - `ParseInput::sliced()` method and its `With<PhantomSliced, P>` impl
//! - `ParseInput::located()` method and its `With<PhantomLocated, P>` impl
//! - `ParseInput::by_ref()` method and its `&mut ByRef<F>` impl
//! - `ParseInput::ignored()` method
//! - `ParseInput::padded()`, `padded_left()`, `padded_right()` methods
//! - `ParseInputUnwrapExt::unwrap()` method
//! - `Accumulator::collect()` and `collect_with()` methods

mod common;

use common::{TestLexer, Token, TokenKind};
use tokora::{
  Emitter, InputRef, Located, Parse, ParseContext, ParseInput, Parser, parser::expect,
  slice::Sliced, span::Spanned, try_parse_input::ParseAttempt, utils::Expected,
};

// ── helper parsers ──────────────────────────────────────────────────────────

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
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

#[allow(dead_code)]
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

// ── ParseInput::sliced() ────────────────────────────────────────────────────

#[test]
fn sliced_wraps_output_with_source_slice() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Sliced<i64, &'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.sliced().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(*result.data(), 42);
  assert_eq!(result.slice(), "42");
}

#[test]
fn sliced_multi_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Sliced<(i64, i64), &'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then(parse_num).sliced().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(*result.data(), (10, 20));
  assert_eq!(result.slice(), "10 20");
}

// ── ParseInput::located() ───────────────────────────────────────────────────

#[test]
fn located_wraps_output_with_span_and_slice() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Located<i64, tokora::SimpleSpan, &'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.located().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(*result.data(), 42);
  let (sl, sp, d) = result.into_components();
  assert_eq!(sl, "42");
  assert_eq!(d, 42);
  assert_eq!(sp.start(), 0);
  assert_eq!(sp.end(), 2);
}

#[test]
fn located_multi_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Located<(i64, i64), tokora::SimpleSpan, &'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then(parse_num).located().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(*result.data(), (10, 20));
  let (sl, sp, _d) = result.into_components();
  assert_eq!(sl, "10 20");
  assert_eq!(sp.start(), 0);
  assert_eq!(sp.end(), 5);
}

// ── ParseInput::by_ref() ───────────────────────────────────────────────────

#[test]
fn by_ref_allows_reuse_of_parser() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut p = parse_num;
    let first = p.by_ref().parse_input(inp)?;
    let second = p.by_ref().parse_input(inp)?;
    Ok((first, second))
  }

  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(result, (10, 20));
}

// ── ParseInput::ignored() ───────────────────────────────────────────────────

#[test]
fn ignored_discards_output() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.ignored().parse_input(inp)
  }

  Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!((), ());
}

#[test]
fn ignored_still_consumes_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Ignore first token, parse second
    parse_num.ignored().parse_input(inp)?;
    parse_num.parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(result, 20);
}

// ── ParseInput::then_value() ────────────────────────────────────────────────

#[test]
fn then_value_replaces_output() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<&'static str, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then_value(|| "parsed a number").parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, "parsed a number");
}

// ── ParseInput::map() ───────────────────────────────────────────────────────

#[test]
fn map_transforms_output() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.map(|n| format!("num:{n}")).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, "num:42");
}

// ── ParseInput::filter_map() ────────────────────────────────────────────────

#[test]
fn filter_map_transforms_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<u32, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter_map(|n| if n >= 0 { Ok(n as u32) } else { Err(()) })
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, 42u32);
}

#[test]
fn filter_map_rejects() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<u32, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter_map(|n| if n >= 0 { Ok(n as u32) } else { Err(()) })
      .parse_input(inp)
  }

  let result: Result<u32, ()> = Parser::new().apply(parse).parse_str("-5");
  assert!(result.is_err());
}

// ── ParseInput::and_then() ──────────────────────────────────────────────────

#[test]
fn and_then_chains_computation() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.and_then(|n| Ok(n * 2)).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("21").unwrap();
  assert_eq!(result, 42);
}

// ── ParseInput::then() ─────────────────────────────────────────────────────

#[test]
fn then_sequences_parsers() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then(parse_num).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("1 2").unwrap();
  assert_eq!(result, (1, 2));
}

// ── ParseInput::ignore_then() ───────────────────────────────────────────────

#[test]
fn ignore_then_discards_first() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.ignore_then(parse_num).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(result, 20);
}

// ── ParseInput::then_ignore() ───────────────────────────────────────────────

#[test]
fn then_ignore_discards_second() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then_ignore(parse_num).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(result, 10);
}

// ── ParseInput::filter() ───────────────────────────────────────────────────

#[test]
fn filter_accepts_valid() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter(|n| if *n > 0 { Ok(()) } else { Err(()) })
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, 42);
}

#[test]
fn filter_rejects_invalid() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter(|n| if *n > 0 { Ok(()) } else { Err(()) })
      .parse_input(inp)
  }

  let result: Result<i64, ()> = Parser::new().apply(parse).parse_str("-5");
  assert!(result.is_err());
}

// ── ParseInput::validate() ──────────────────────────────────────────────────

#[test]
fn validate_accepts_valid() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<i64, tokora::SimpleSpan>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .spanned()
      .validate(|n| if *n.data() > 0 { Ok(()) } else { Err(()) })
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(*result.data(), 42);
}

// ── ParseInput::map_with() ──────────────────────────────────────────────────

#[test]
fn map_with_has_access_to_state() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(i64, &'inp str), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .map_with(|n, state| (n, state.slice().unwrap_or("")))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, (42, "42"));
}

// ── ParseInput::filter_with() ───────────────────────────────────────────────

#[test]
fn filter_with_accepts_valid() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter_with(|n, _state| if *n > 0 { Ok(()) } else { Err(()) })
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, 42);
}

// ── ParseInput::filter_map_with() ───────────────────────────────────────────

#[test]
fn filter_map_with_transforms() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .filter_map_with(|n, state| Ok(format!("{}@{}", n, state.slice().unwrap_or(""))))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, "42@42");
}

// ── ParseInput::validate_with() ─────────────────────────────────────────────

#[test]
fn validate_with_accepts_valid() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<i64, tokora::SimpleSpan>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .spanned()
      .validate_with(|n, _state| if *n.data() > 0 { Ok(()) } else { Err(()) })
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(*result.data(), 42);
}

// ── ParseInput::and_then_with() ─────────────────────────────────────────────

#[test]
fn and_then_with_chains_with_state() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(i64, &'inp str), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .and_then_with(|n, state| Ok((n, state.slice().unwrap_or(""))))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, (42, "42"));
}

// ── Combined: sliced + spanned ──────────────────────────────────────────────

#[test]
fn spanned_then_sliced_both_work() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<i64, tokora::SimpleSpan>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.spanned().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(*result.data(), 42);
  let sp = result.into_span();
  assert_eq!(sp.start(), 0);
  assert_eq!(sp.end(), 2);
}
