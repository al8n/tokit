#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]
mod common;

use common::{TestLexer, Token, TokenKind};
use generic_arraydeque::typenum::U1;
use tokora::{
  Branch, Emitter, InputRef, Lexer, Parse, ParseChoice, ParseContext, ParseInput, Parser,
  TryParseInput,
  cache::Peeked,
  parser::{Action, Any, expect, try_expect},
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
  .parse_input(inp)
  .map(|t| match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  })
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

// ── input_ref/try_expect.rs: punctuator expect methods ──────────────────────

#[test]
fn try_expect_comma_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_comma()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str(",").unwrap());
}

#[test]
fn try_expect_comma_no_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_comma()?.is_some())
  }

  assert!(!Parser::new().apply(parse).parse_str("+").unwrap());
}

#[test]
fn try_expect_comma_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_comma()?.is_some())
  }

  assert!(!Parser::new().apply(parse).parse_str("").unwrap());
}

#[test]
fn try_expect_semicolon_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_semicolon()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str(";").unwrap());
}

#[test]
fn try_expect_semicolon_no_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_semicolon()?.is_some())
  }

  assert!(!Parser::new().apply(parse).parse_str("+").unwrap());
}

#[test]
fn try_expect_open_paren_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_open_paren()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str("(").unwrap());
}

#[test]
fn try_expect_close_paren_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_close_paren()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str(")").unwrap());
}

#[test]
fn try_expect_open_bracket_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_open_bracket()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str("[").unwrap());
}

#[test]
fn try_expect_close_bracket_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_close_bracket()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str("]").unwrap());
}

#[test]
fn try_expect_open_brace_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_open_brace()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str("{").unwrap());
}

#[test]
fn try_expect_close_brace_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_close_brace()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str("}").unwrap());
}

#[test]
fn expect_comma_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_comma().map(|s| s.into_data())
  }

  let result = Parser::new().apply(parse).parse_str(",").unwrap();
  assert_eq!(result, Token::Comma);
}

#[test]
fn expect_comma_mismatch() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_comma().map(|s| s.into_data())
  }

  assert!(Parser::new().apply(parse).parse_str("+").is_err());
}

#[test]
fn expect_comma_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_comma().map(|s| s.into_data())
  }

  assert!(Parser::new().apply(parse).parse_str("").is_err());
}

#[test]
fn expect_semicolon_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_semicolon().map(|s| s.into_data())
  }

  let result = Parser::new().apply(parse).parse_str(";").unwrap();
  assert_eq!(result, Token::Semi);
}

#[test]
fn expect_open_paren_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_open_paren().map(|s| s.into_data())
  }

  let result = Parser::new().apply(parse).parse_str("(").unwrap();
  assert_eq!(result, Token::LParen);
}

#[test]
fn expect_close_paren_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_close_paren().map(|s| s.into_data())
  }

  let result = Parser::new().apply(parse).parse_str(")").unwrap();
  assert_eq!(result, Token::RParen);
}

// ── try_expect_map ──────────────────────────────────────────────────────────

#[test]
fn try_expect_map_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_map(|t| match t.data() {
        Token::Num(n) => Some(*n),
        _ => None,
      })
      .map(|opt| opt.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn try_expect_map_no_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_map(|t| match t.data() {
        Token::Num(n) => Some(*n),
        _ => None,
      })
      .map(|opt| opt.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

#[test]
fn try_expect_map_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_map(|t| match t.data() {
        Token::Num(n) => Some(*n),
        _ => None,
      })
      .map(|opt| opt.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, None);
}

// ── try_expect_and_then ─────────────────────────────────────────────────────

#[test]
fn try_expect_and_then_match_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) => Some(Ok(*n)),
        _ => None,
      })
      .map(|opt| opt.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn try_expect_and_then_no_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) => Some(Ok(*n)),
        _ => None,
      })
      .map(|opt| opt.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

#[test]
fn try_expect_and_then_match_err() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) if *n > 100 => Some(Err(())),
        Token::Num(n) => Some(Ok(*n)),
        _ => None,
      })
      .map(|opt| opt.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));

  assert!(Parser::new().apply(parse).parse_str("200").is_err());
}

#[test]
fn try_expect_and_then_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) => Some(Ok(*n)),
        _ => None,
      })
      .map(|opt| opt.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, None);
}

// ── input_ref/peek.rs ───────────────────────────────────────────────────────

#[test]
fn peek_one_returns_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.peek_one()?.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str("42").unwrap());
}

#[test]
fn peek_one_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.peek_one()?.is_some())
  }

  assert!(!Parser::new().apply(parse).parse_str("").unwrap());
}

#[test]
fn peek_does_not_consume() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(bool, bool), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let first = inp.peek_one()?.is_some();
    let second = inp.peek_one()?.is_some();
    Ok((first, second))
  }

  let (a, b) = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(a && b);
}

#[test]
fn peek_window_larger_than_input() {
  use generic_arraydeque::typenum::U3;

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let peeked = inp.peek::<U3>()?;
    Ok(peeked.len())
  }

  // "42" is one token, peek<U3> should return 1
  let count = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(count, 1);
}

// ── input_ref/sync_through.rs ───────────────────────────────────────────────

#[test]
fn sync_through_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(|t| matches!(t.data(), Token::Semi), || None)?;
    Ok(result.is_some())
  }

  assert!(!Parser::new().apply(parse).parse_str("").unwrap());
}

#[test]
fn sync_through_first_token_matches() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(|t| matches!(t.data(), Token::Semi), || None)?;
    Ok(result.is_some())
  }

  // First token matches immediately, no unexpected tokens emitted
  assert!(Parser::new().apply(parse).parse_str(";").unwrap());
}

// ── parse_choice.rs ─────────────────────────────────────────────────────────

#[test]
fn parse_choice_tuple_b0() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices = (Any::new(), Any::new());
    choices.parse_choice(inp, &Branch::B0)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

#[test]
fn parse_choice_tuple_b1() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices = (Any::new(), Any::new());
    choices.parse_choice(inp, &Branch::B1)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

#[test]
fn parse_choice_tuple_try_some() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices = (Any::new(), Any::new());
    choices.try_parse_choice(inp, Some(&Branch::B0))
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(matches!(result, ParseAttempt::Accept(Token::Plus)));
}

#[test]
fn parse_choice_tuple_try_none() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices = (Any::new(), Any::new());
    choices.try_parse_choice(inp, None)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn parse_choice_array() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices = [Any::new(), Any::new()];
    // 1-based: the old 0-based `new(0)` is now `new(1)`.
    let id = deranged::RangedUsize::<1, 2>::new(1).unwrap();
    choices.parse_choice(inp, &id)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

#[test]
fn parse_choice_slice() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices: [Any<TestLexer<'inp>, Ctx>; 2] = [Any::new(), Any::new()];
    let slice: &mut [Any<TestLexer<'inp>, Ctx>] = &mut choices;
    slice.parse_choice(inp, &0usize)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

// ── utils/human_display.rs ──────────────────────────────────────────────────

#[test]
fn human_display_u8_ascii() {
  use tokora::utils::human_display::DisplayHuman;
  let byte: u8 = b'A';
  assert_eq!(format!("{}", byte.display()), "A");
}

#[test]
fn human_display_u8_non_ascii() {
  use tokora::utils::human_display::DisplayHuman;
  let byte: u8 = 200;
  assert_eq!(format!("{}", byte.display()), "200");
}

#[test]
fn human_display_byte_slice_valid_utf8() {
  use tokora::utils::human_display::DisplayHuman;
  let bytes = b"hello";
  assert_eq!(format!("{}", bytes.display()), "hello");
}

#[test]
fn human_display_byte_slice_invalid_utf8() {
  use tokora::utils::human_display::DisplayHuman;
  let bytes: &[u8] = &[0xFF, 0xFE];
  let display = format!("{}", bytes.display());
  // Should show debug format for non-UTF-8
  assert!(!display.is_empty());
}

#[test]
fn human_display_char_slice() {
  use tokora::utils::human_display::DisplayHuman;
  let chars = ['h', 'e', 'l', 'l', 'o'];
  assert_eq!(format!("{}", chars.display()), "hello");
}

#[test]
fn human_display_char_array() {
  use tokora::utils::human_display::DisplayHuman;
  let chars = ['a', 'b'];
  assert_eq!(format!("{}", chars.display()), "ab");
}

#[test]
fn human_display_byte_array() {
  use tokora::utils::human_display::DisplayHuman;
  let bytes = [b'x', b'y'];
  assert_eq!(format!("{}", bytes.display()), "xy");
}

#[test]
fn human_display_unit() {
  use tokora::utils::human_display::DisplayHuman;
  assert_eq!(format!("{}", ().display()), "");
}

#[test]
fn human_display_str() {
  use tokora::utils::human_display::DisplayHuman;
  let s = "test";
  assert_eq!(format!("{}", s.display()), "test");
}

#[test]
fn human_display_char() {
  use tokora::utils::human_display::DisplayHuman;
  let c = 'z';
  assert_eq!(format!("{}", c.display()), "z");
}

#[test]
fn human_display_integers() {
  use tokora::utils::human_display::DisplayHuman;
  assert_eq!(format!("{}", 42i32.display()), "42");
  assert_eq!(format!("{}", 42u64.display()), "42");
  assert_eq!(format!("{}", (-5i64).display()), "-5");
}

#[test]
fn human_display_positioned_char() {
  use tokora::utils::{PositionedChar, human_display::DisplayHuman};
  let pc = PositionedChar::with_position('x', 42usize);
  assert_eq!(format!("{}", pc.display()), "x");
}

#[test]
fn human_display_ref() {
  use tokora::utils::human_display::DisplayHuman;
  let val = 42i32;
  let r = &val;
  assert_eq!(format!("{}", r.display()), "42");
}

// ── punct.rs ────────────────────────────────────────────────────────────────

#[test]
fn punct_comma_display_human() {
  use tokora::punct::Comma;
  use tokora::utils::human_display::DisplayHuman;
  let c = Comma::unit();
  assert_eq!(format!("{}", c.display()), ",");
}

#[test]
fn punct_semicolon_display_human() {
  use tokora::punct::Semicolon;
  use tokora::utils::human_display::DisplayHuman;
  let s = Semicolon::unit();
  assert_eq!(format!("{}", s.display()), ";");
}

#[test]
fn punct_comma_partial_eq() {
  use tokora::punct::Comma;
  let c1 = Comma::unit();
  let c2 = Comma::unit();
  assert_eq!(c1, c2);
  assert!(c1 == *",");
  assert!(*"," == c1);
}

#[test]
fn punct_comma_partial_ord() {
  use tokora::punct::Comma;
  let c = Comma::unit();
  assert_eq!(c.partial_cmp(","), Some(core::cmp::Ordering::Equal));
  assert_eq!(",".partial_cmp(&c), Some(core::cmp::Ordering::Equal));
}

#[test]
fn punct_various_raw() {
  use tokora::punct::*;
  assert_eq!(Plus::raw(), "+");
  assert_eq!(Hyphen::raw(), "-");
  assert_eq!(Asterisk::raw(), "*");
  assert_eq!(Slash::raw(), "/");
  assert_eq!(Equal::raw(), "=");
  assert_eq!(Exclamation::raw(), "!");
  assert_eq!(Question::raw(), "?");
  assert_eq!(Colon::raw(), ":");
  assert_eq!(Dot::raw(), ".");
  assert_eq!(Hash::raw(), "#");
  assert_eq!(Percent::raw(), "%");
  assert_eq!(Ampersand::raw(), "&");
  assert_eq!(Pipe::raw(), "|");
  assert_eq!(Caret::raw(), "^");
  assert_eq!(Tilde::raw(), "~");
  assert_eq!(Dollar::raw(), "$");
  assert_eq!(At::raw(), "@");
  assert_eq!(Backtick::raw(), "`");
  assert_eq!(Backslash::raw(), "\\");
  assert_eq!(Underscore::raw(), "_");
  assert_eq!(Apostrophe::raw(), "'");
  assert_eq!(DoubleQuote::raw(), "\"");
}

#[test]
fn punct_multi_char_raw() {
  use tokora::punct::*;
  assert_eq!(Arrow::raw(), "->");
  assert_eq!(FatArrow::raw(), "=>");
  assert_eq!(PipeArrow::raw(), "|>");
  assert_eq!(ColonEqual::raw(), ":=");
  assert_eq!(LogicalEqual::raw(), "==");
  assert_eq!(LogicalNotEqual::raw(), "!=");
  assert_eq!(StrictEqual::raw(), "===");
  assert_eq!(StrictNotEqual::raw(), "!==");
  assert_eq!(LessThanOrEqual::raw(), "<=");
  assert_eq!(GreaterThanOrEqual::raw(), ">=");
  assert_eq!(Increment::raw(), "++");
  assert_eq!(Decrement::raw(), "--");
  assert_eq!(Exponentiation::raw(), "**");
  assert_eq!(LogicalAnd::raw(), "&&");
  assert_eq!(LogicalOr::raw(), "||");
  assert_eq!(DoubleColon::raw(), "::");
  assert_eq!(Spread::raw(), "...");
  assert_eq!(NullCoalesce::raw(), "??");
  assert_eq!(OptionalChain::raw(), "?.");
}

#[test]
fn punct_with_content_and_span() {
  use tokora::punct::Comma;
  let c = Comma::<usize, &str>::with_content(10, "src");
  assert_eq!(*c.span(), 10);
  assert_eq!(*c.content(), "src");
  assert_eq!(c.as_str(), ",");
}

#[test]
fn punct_borrow_and_as_ref() {
  use core::borrow::Borrow;
  use tokora::punct::Comma;
  let c = Comma::unit();
  let b: &str = c.borrow();
  assert_eq!(b, ",");
  let r: &str = c.as_ref();
  assert_eq!(r, ",");
}

#[test]
fn punct_into_components() {
  use tokora::punct::Comma;
  use tokora::utils::IntoComponents;
  let c = Comma::<usize, &str>::with_content(42, "test");
  let (span, content) = c.into_components();
  assert_eq!(span, 42);
  assert_eq!(content, "test");
}

#[test]
fn punct_into_span() {
  use tokora::punct::Comma;
  use tokora::span::IntoSpan;
  let c = Comma::<usize>::new(99);
  assert_eq!(c.into_span(), 99);
}

#[test]
fn punct_as_span() {
  use tokora::punct::Comma;
  use tokora::span::AsSpan;
  let c = Comma::<usize>::new(77);
  assert_eq!(*c.as_span(), 77);
}

#[test]
fn punct_change_language() {
  use tokora::punct::Comma;
  struct LangA;
  struct LangB;
  // Comma::unit() creates Comma<(), (), ()>, change to LangA then LangB
  let c: Comma<(), (), LangA> = Comma::unit().change_language();
  let c2: Comma<(), (), LangB> = c.change_language();
  assert_eq!(c2.as_str(), ",");
}

// ── Branch ──────────────────────────────────────────────────────────────────

#[test]
fn branch_id() {
  let b: Branch<2> = Branch::B0;
  assert_eq!(b.id(), 0);
  let b1: Branch<2> = Branch::B1;
  assert_eq!(b1.id(), 1);
}

// ── The array choice's id space is a bijection onto its indices ──────────────

/// **Bound to tokora's own declaration, not to `deranged`'s.** Spelling the bound directly
/// (`RangedUsize::<1, 2>::new(3)`) would test a dependency's property and would keep passing
/// if this crate reverted `Id` — so the id type is projected off the very `ParseChoice` impl
/// the dispatch below uses, through a helper that takes the array by reference and never names
/// `RangedUsize` at all.
///
/// Before: `Id = RangedUsize<0, 2>`, so `new(0)` was `Some` (a legal id) and `new(2)` was
/// `Some` — the over-admission, one past the last index, and a panic on dispatch.
/// After: `Id = RangedUsize<1, 2>`, so `new(0)` and `new(3)` are both `None` and every value
/// that exists is a valid index.
#[test]
fn array_choice_id_rejects_the_boundary() {
  /// Projects the array impl's `Id` and reports which of the four boundary values construct.
  fn id_admits<'inp, L, O, Ctx, Lang, P, const N: usize>(_witness: &[P; N]) -> [bool; 4]
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Lang: ?Sized,
    P: ParseInput<'inp, L, O, Ctx, Lang>,
  {
    type Id<'inp, L, O, Ctx, Lang, P, const N: usize> =
      <[P; N] as ParseChoice<'inp, L, O, Ctx, Lang>>::Id;
    [
      Id::<'inp, L, O, Ctx, Lang, P, N>::new(0).is_some(),
      Id::<'inp, L, O, Ctx, Lang, P, N>::new(1).is_some(),
      Id::<'inp, L, O, Ctx, Lang, P, N>::new(2).is_some(),
      Id::<'inp, L, O, Ctx, Lang, P, N>::new(3).is_some(),
    ]
  }

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Deliberately no `RangedUsize` literal anywhere in this cell: naming one would make the
    // RED a type error rather than the boundary payload. The only thing asserted is which of
    // the four values the *declared* `Id` admits.
    let choices = [Any::new(), Any::new()];
    let admits = id_admits::<TestLexer<'inp>, Token, Ctx, (), _, 2>(&choices);
    assert_eq!(
      admits,
      [false, true, true, false],
      "array-choice ids are 1-based and bijective: 0 and 3 must not construct, 1 and 2 must"
    );
    let _ = inp;
    Ok(Token::Plus)
  }

  assert_eq!(
    Parser::new().apply(parse).parse_str("+").unwrap(),
    Token::Plus
  );
}

/// The boundary id used to panic on dispatch; it now selects the last element through the
/// public surface.
#[test]
fn array_choice_dispatch_reaches_the_last_element() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices = [Any::new(), Any::new()];
    let last = deranged::RangedUsize::<1, 2>::new(2).unwrap();
    choices.parse_choice(inp, &last)
  }

  assert_eq!(
    Parser::new().apply(parse).parse_str("+").unwrap(),
    Token::Plus
  );
}

/// The slice siblings have no compile-time repair — length is a runtime fact — so the repair
/// is a **named** refusal. Before: the raw `index out of bounds` message, which says nothing
/// about the contract. After: a message that names it.
#[test]
#[should_panic(expected = "choice id")]
fn slice_choice_out_of_range_id_names_the_contract() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut choices = [Any::new(), Any::new()];
    let slice: &mut [_] = &mut choices;
    ParseChoice::parse_choice(&mut { slice }, inp, &7usize)
  }

  let _ = Parser::new().apply(parse).parse_str("+");
}
