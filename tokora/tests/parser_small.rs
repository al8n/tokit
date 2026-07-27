#![cfg(all(feature = "std", feature = "logos"))]

//! Coverage tests for small parser source files:
//! accepted, todo, unwrapped, fail, map, then_value, and_then_with,
//! peek_then, expect, fold_while.

mod common;

use common::{TestLexer, Token, TokenKind};
use generic_arraydeque::typenum::U1;
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, ParseInputUnwrapExt, Parser, TryParseInput,
  cache::Peeked,
  parser::{Action, Any, Empty, Todo, expect, fail, fail_with, try_expect},
  try_parse_input::ParseAttempt,
  utils::Expected,
};

// ── Helpers ─────────────────────────────────────────────────────────────────

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

fn while_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _emitter: &mut Ctx::Emitter,
) -> Result<Action, <Ctx::Emitter as Emitter<'inp, TestLexer<'inp>>>::Error>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
{
  Ok(match peeked.pop_front() {
    None => Action::Stop,
    Some(tok) => {
      let tok = tok
        .as_maybe_ref()
        .map(|t| t.token().copied(), |t| t.token())
        .into_inner();
      if matches!(**tok.data(), Token::Num(_)) {
        Action::Continue
      } else {
        Action::Stop
      }
    }
  })
}

// ── accepted.rs ─────────────────────────────────────────────────────────────

#[test]
fn accepted_parse_attempt_output() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.accepted().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, ParseAttempt::Accept(42));
}

#[test]
fn accepted_decline_on_mismatch() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.accepted().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn accepted_option_output() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.accepted().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));

  let result2 = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result2, None);
}

#[test]
fn accepted_try_parse_input() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.accepted().try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, ParseAttempt::Accept(42));

  let result2 = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result2, ParseAttempt::Decline);
}

// ── todo.rs ─────────────────────────────────────────────────────────────────

#[test]
#[should_panic]
fn todo_parser_panics() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Todo::<Token>::new().parse_input(inp)
  }

  let _ = Parser::new().apply(parse).parse_str("42");
}

// ── unwrapped.rs ────────────────────────────────────────────────────────────

#[test]
fn unwrapped_some_value() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.map(Some).unwrap().parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, 42);
}

#[test]
#[should_panic]
fn unwrapped_none_panics() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Empty::new()
      .map(|()| -> Option<i64> { None })
      .unwrap()
      .parse_input(inp)
  }

  let _ = Parser::new().apply(parse).parse_str("42");
}

// ── fail.rs ─────────────────────────────────────────────────────────────────

#[test]
fn fail_parse_input_returns_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    fail(|| ()).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("42").is_err());
}

#[test]
fn fail_try_parse_input_returns_error() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    fail::<_, _, i64, _, _>(|| ()).try_parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("42").is_err());
}

#[test]
fn fail_with_parse_input_returns_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    fail_with(|_state| ()).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("42").is_err());
}

#[test]
fn fail_with_try_parse_input_returns_error() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    fail_with::<_, _, i64, _, _>(|_state| ()).try_parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("42").is_err());
}

#[test]
fn fail_on_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    fail(|| ()).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("").is_err());
}

#[test]
fn fail_with_on_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    fail_with(|_state| ()).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("").is_err());
}

// ── map.rs ──────────────────────────────────────────────────────────────────

#[test]
fn map_parse_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.map(|n| n.to_string()).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, "42");
}

#[test]
fn map_error_propagation() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.map(|n| n.to_string()).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("+").is_err());
}

#[test]
fn map_with_parse_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.map_with(|n, _state| n * 2).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("5").unwrap();
  assert_eq!(result, 10);
}

#[test]
fn map_with_error_propagation() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.map_with(|n, _state| n * 2).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("+").is_err());
}

#[test]
fn map_chain_multiple() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .map(|n| n * 2)
      .map(|n| n.to_string())
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("21").unwrap();
  assert_eq!(result, "42");
}

// ── then_value.rs ───────────────────────────────────────────────────────────

#[test]
fn then_value_parse_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then_value(|| true).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("42").unwrap());
}

#[test]
fn then_value_parse_input_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then_value(|| true).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("+").is_err());
}

#[test]
fn then_value_string_output() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.then_value(|| "done".to_string()).parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, "done");
}

// ── and_then_with.rs ────────────────────────────────────────────────────────

#[test]
fn and_then_with_parse_input_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .and_then_with(|n, _state| Ok(n * 2))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("5").unwrap();
  assert_eq!(result, 10);
}

#[test]
fn and_then_with_parse_input_error_from_inner() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .and_then_with(|n, _state| Ok(n * 2))
      .parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("+").is_err());
}

#[test]
fn and_then_with_parse_input_error_from_then() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .and_then_with(|n, _state| if n > 0 { Ok(n) } else { Err(()) })
      .parse_input(inp)
  }

  assert_eq!(Parser::new().apply(parse).parse_str("5").unwrap(), 5);
  assert!(Parser::new().apply(parse).parse_str("-3").is_err());
}

#[test]
fn and_then_with_on_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.and_then_with(|n, _state| Ok(n)).parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("").is_err());
}

// ── peek_then.rs ────────────────────────────────────────────────────────────

#[test]
fn peek_then_parse_input_accept() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .peek_then::<_, U1>(|_peeked: Peeked<'_, '_, TestLexer<'_>, U1>, _emitter| Ok(()))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, 42);
}

#[test]
fn peek_then_parse_input_reject() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .peek_then::<_, U1>(|_peeked: Peeked<'_, '_, TestLexer<'_>, U1>, _emitter| Err(()))
      .parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("42").is_err());
}

#[test]
fn peek_then_parse_input_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Any::new()
      .peek_then::<_, U1>(|_peeked: Peeked<'_, '_, TestLexer<'_>, U1>, _emitter| Err(()))
      .parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("").is_err());
}

#[test]
fn peek_then_parse_input_chained() {
  // Test parse_input path with actual token inspection
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .peek_then::<_, U1>(|_peeked: Peeked<'_, '_, TestLexer<'_>, U1>, _emitter| Ok(()))
      .map(|n| n + 1)
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("10").unwrap();
  assert_eq!(result, 11);
}

// ── expect.rs ───────────────────────────────────────────────────────────────

#[test]
fn expect_parse_input_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
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

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

#[test]
fn expect_parse_input_mismatch() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
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

  assert!(Parser::new().apply(parse).parse_str("42").is_err());
}

#[test]
fn expect_parse_input_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
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

  assert!(Parser::new().apply(parse).parse_str("").is_err());
}

#[test]
fn expect_spanned_output() {
  use tokora::span::Spanned;

  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<Token, tokora::SimpleSpan>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    expect(|t: &Token| {
      if matches!(t, Token::Plus) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Plus))
      }
    })
    .spanned()
    .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(*result.data(), Token::Plus);
}

#[test]
fn expect_sliced_output() {
  use tokora::slice::Sliced;

  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Sliced<Token, &'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    expect(|t: &Token| {
      if matches!(t, Token::Plus) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Plus))
      }
    })
    .sliced()
    .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(*result.data(), Token::Plus);
  assert_eq!(*result.slice_ref(), "+");
}

#[test]
fn expect_located_output() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<tokora::Located<Token, tokora::SimpleSpan, &'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    expect(|t: &Token| {
      if matches!(t, Token::Plus) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Plus))
      }
    })
    .located()
    .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  // Located derefs to the data
  assert_eq!(*result, Token::Plus);
}

#[test]
fn try_expect_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_expect(|t: &Token| matches!(t, Token::Plus)).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(matches!(result, ParseAttempt::Accept(Token::Plus)));
}

#[test]
fn try_expect_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_expect(|t: &Token| matches!(t, Token::Plus)).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn try_expect_empty_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_expect(|t: &Token| matches!(t, Token::Plus)).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

// ── fold_while.rs ───────────────────────────────────────────────────────────

#[test]
fn fold_while_basic_sum() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, n| acc + n)
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("1 2 3").unwrap();
  assert_eq!(result, 6);
}

#[test]
fn fold_while_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, n| acc + n)
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, 0);
}

#[test]
fn try_fold_while_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, n| Ok(acc + n))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(result, 30);
}

#[test]
fn try_fold_while_acc_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while::<_, _, _, U1>(
        while_num::<Ctx>,
        || 0i64,
        |acc, n| {
          if acc + n > 50 { Err(()) } else { Ok(acc + n) }
        },
      )
      .parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("30 30").is_err());
}

#[test]
fn try_fold_while_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while::<_, _, _, U1>(while_num::<Ctx>, || 99i64, |acc, n| Ok(acc + n))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, 99);
}

#[test]
fn try_fold_while_with_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while_with::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, n, _state| Ok(acc + n))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("5 10 15").unwrap();
  assert_eq!(result, 30);
}

#[test]
fn try_fold_while_with_acc_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while_with::<_, _, _, U1>(
        while_num::<Ctx>,
        || 0i64,
        |acc, n, _state| {
          if acc + n > 10 { Err(()) } else { Ok(acc + n) }
        },
      )
      .parse_input(inp)
  }

  assert!(Parser::new().apply(parse).parse_str("8 8").is_err());
}

#[test]
fn try_fold_while_with_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while_with::<_, _, _, U1>(while_num::<Ctx>, || 42i64, |acc, n, _state| Ok(acc + n))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, 42);
}
