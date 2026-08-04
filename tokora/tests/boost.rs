#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

// Coverage boost tests targeting uncovered lines across multiple modules.

mod common;

use common::{TestLexer, Token, TokenKind};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::DefaultCache,
  emitter::{Ignored, Silent},
  parser::expect,
  span::Spanned,
  utils::Expected,
};

// ── Type aliases ────────────────────────────────────────────────────────────

type IgnoredContext<'inp> =
  ParserContext<'inp, TestLexer<'inp>, Ignored, DefaultCache<'inp, TestLexer<'inp>>>;

type SilentContext<'inp> =
  ParserContext<'inp, TestLexer<'inp>, Silent<()>, DefaultCache<'inp, TestLexer<'inp>>>;

type BlackholeContext<'inp> = ParserContext<'inp, TestLexer<'inp>, Ignored, ()>;

macro_rules! ignored_parser {
  () => {
    Parser::with_context(IgnoredContext::new(Ignored::default()))
  };
}

macro_rules! silent_parser {
  () => {
    Parser::with_context(SilentContext::new(Silent::new()))
  };
}

macro_rules! blackhole_parser {
  () => {
    Parser::with_context(BlackholeContext::new(Ignored::default()))
  };
}

// ── InputRef::is_eoi ────────────────────────────────────────────────────────

#[test]
fn is_eoi_on_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.is_eoi())
  }
  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert!(result);
}

#[test]
fn is_eoi_on_non_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.is_eoi())
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(!result);
}

#[test]
fn is_eoi_after_consuming_all() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.next()?;
    Ok(inp.is_eoi())
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(result);
}

// ── InputRef::source ────────────────────────────────────────────────────────

#[test]
fn source_returns_input_str() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<&'inp str, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.source())
  }
  let result = Parser::new().apply(parse).parse_str("hello").unwrap();
  assert_eq!(result, "hello");
}

// ── InputRef::cache ─────────────────────────────────────────────────────────

#[test]
fn cache_is_accessible() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.cache();
    Ok(())
  }
  Parser::new().apply(parse).parse_str("42").unwrap();
}

// ── InputRef::emitter_ref ───────────────────────────────────────────────────

#[test]
fn emitter_is_accessible() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.emitter_ref();
    Ok(())
  }
  Parser::new().apply(parse).parse_str("42").unwrap();
}

// ── InputRef::attempt ───────────────────────────────────────────────────────

#[test]
fn attempt_success_preserves_state() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.attempt(|inp| {
      let tok = inp.next().ok()??;
      match tok.into_data() {
        Token::Num(n) => Some(n),
        _ => None,
      }
    });
    Ok(result.unwrap_or(-1))
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, 42);
}

#[test]
fn attempt_failure_rolls_back() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<i64>, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let attempt_result = inp.attempt(|inp| {
      let tok = inp.next().ok()??;
      match tok.data() {
        Token::Plus => Some(0i64),
        _ => None,
      }
    });
    let next = inp.next()?.map(|s| s.into_data());
    Ok((attempt_result, next))
  }
  let (attempt_result, next) = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(attempt_result.is_none());
  assert!(matches!(next, Some(Token::Num(42))));
}

// ── InputRef::save / restore ────────────────────────────────────────────────

#[cfg(feature = "unstable-raw")]
#[test]
fn save_and_restore() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Token, Token), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let ckp = inp.save();
    let first = inp.next()?.unwrap().into_data();
    inp.restore(ckp);
    let again = inp.next()?.unwrap().into_data();
    Ok((first, again))
  }
  let (first, again) = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(first, again);
}

// ── InputRef::cursor / offset ───────────────────────────────────────────────

#[test]
fn cursor_and_offset_advance() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let before = *inp.offset();
    let _ = inp.next()?;
    let after = *inp.offset();
    Ok((before, after))
  }
  let (before, after) = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(before, 0);
  assert!(after > before);
}

// ── InputRef::span_since / span_from / span_range ───────────────────────────

#[test]
fn span_since_covers_consumed_tokens() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let start = *inp.cursor();
    let _ = inp.next()?;
    let span = inp.span_since(&start);
    Ok((span.start(), span.end()))
  }
  let (start, end) = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(start, 0);
  assert_eq!(end, 2);
}

#[test]
fn span_from_cursor_to_end() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let start = *inp.cursor();
    let span = inp.span_from(&start);
    Ok((span.start(), span.end()))
  }
  let (start, end) = Parser::new().apply(parse).parse_str("42 55").unwrap();
  assert_eq!(start, 0);
  assert_eq!(end, 5);
}

#[test]
fn span_range_between_cursors() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let start = *inp.cursor();
    let _ = inp.next()?;
    let mid = *inp.cursor();
    let span = inp.span_range(&start..&mid);
    Ok((span.start(), span.end()))
  }
  let (start, end) = Parser::new().apply(parse).parse_str("42 55").unwrap();
  assert_eq!(start, 0);
  assert!(end > 0);
}

// ── InputRef::slice_since / slice_from / slice_range ────────────────────────

#[test]
fn slice_since_returns_consumed_text() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<&'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let start = *inp.cursor();
    let _ = inp.next()?;
    Ok(inp.slice_since(&start))
  }
  let result = Parser::new().apply(parse).parse_str("42 55").unwrap();
  assert_eq!(result, Some("42"));
}

#[test]
fn slice_from_returns_remaining_text() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<&'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let start = inp.cursor();
    Ok(inp.slice_from(start))
  }
  let result = Parser::new().apply(parse).parse_str("42 55").unwrap();
  assert_eq!(result, Some("42 55"));
}

#[test]
fn slice_range_returns_span_text() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<&'inp str>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let start = *inp.cursor();
    let _ = inp.next()?;
    let mid = *inp.cursor();
    Ok(inp.slice_range(&start..&mid))
  }
  let result = Parser::new().apply(parse).parse_str("42 55").unwrap();
  assert_eq!(result, Some("42"));
}

// ── InputRef::state / state_mut / set_state ─────────────────────────────────

#[test]
fn state_and_state_mut_accessible() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _s = inp.state();
    let _sm = inp.state_mut();
    Ok(())
  }
  Parser::new().apply(parse).parse_str("42").unwrap();
}

#[test]
fn set_state_works() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.set_state(());
    Ok(())
  }
  Parser::new().apply(parse).parse_str("42").unwrap();
}

// ── InputRef::span ──────────────────────────────────────────────────────────

#[test]
fn span_returns_current_position() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let span = inp.span();
    Ok((span.start(), span.end()))
  }
  let (start, end) = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(start, 0);
  assert_eq!(end, 0);
}

// ── InputRef::lexer ─────────────────────────────────────────────────────────

#[test]
fn lexer_creates_positioned_lexer() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _lexer = inp.lexer();
    Ok(true)
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(result);
}

// ── InputRef::slice (current token) ─────────────────────────────────────────

#[test]
fn slice_returns_current_token_text() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<&'inp str, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.next()?;
    Ok(inp.slice())
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, "42");
}

// ── Expect parser error paths ───────────────────────────────────────────────

#[test]
fn expect_parser_unexpected_token_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
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
  }
  let result: Result<Token, ()> = Parser::new().apply(parse).parse_str("+");
  assert!(result.is_err());
}

#[test]
fn expect_parser_unexpected_eot_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
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
  }
  let result: Result<Token, ()> = Parser::new().apply(parse).parse_str("");
  assert!(result.is_err());
}

#[test]
fn expect_parser_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
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
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(matches!(result, Token::Num(42)));
}

// ── Expect parser with spanned ──────────────────────────────────────────────

#[test]
fn expect_parser_spanned_success() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<Token, tokora::SimpleSpan>, ()>
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
    .spanned()
    .parse_input(inp)
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(matches!(result.into_data(), Token::Num(42)));
}

#[test]
fn expect_parser_spanned_eot() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<Token, tokora::SimpleSpan>, ()>
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
    .spanned()
    .parse_input(inp)
  }
  let result: Result<_, ()> = Parser::new().apply(parse).parse_str("");
  assert!(result.is_err());
}

// ── Expect punctuator methods on InputRef ───────────────────────────────────

#[test]
fn expect_open_brace_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_open_brace().map(|s| s.into_data())
  }
  let result = Parser::new().apply(parse).parse_str("{").unwrap();
  assert!(matches!(result, Token::LBrace));
}

#[test]
fn expect_close_brace_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_close_brace().map(|s| s.into_data())
  }
  let result = Parser::new().apply(parse).parse_str("}").unwrap();
  assert!(matches!(result, Token::RBrace));
}

#[test]
fn expect_open_bracket_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_open_bracket().map(|s| s.into_data())
  }
  let result = Parser::new().apply(parse).parse_str("[").unwrap();
  assert!(matches!(result, Token::LBracket));
}

#[test]
fn expect_close_bracket_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_close_bracket().map(|s| s.into_data())
  }
  let result = Parser::new().apply(parse).parse_str("]").unwrap();
  assert!(matches!(result, Token::RBracket));
}

#[test]
fn expect_close_paren_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_close_paren().map(|s| s.into_data())
  }
  let result = Parser::new().apply(parse).parse_str(")").unwrap();
  assert!(matches!(result, Token::RParen));
}

#[test]
fn expect_comma_eot_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_comma().map(|s| s.into_data())
  }
  let result: Result<Token, ()> = Parser::new().apply(parse).parse_str("");
  assert!(result.is_err());
}

#[test]
fn expect_semicolon_wrong_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_semicolon().map(|s| s.into_data())
  }
  let result: Result<Token, ()> = Parser::new().apply(parse).parse_str("+");
  assert!(result.is_err());
}

#[test]
fn expect_open_brace_eot() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_open_brace().map(|s| s.into_data())
  }
  let result: Result<Token, ()> = Parser::new().apply(parse).parse_str("");
  assert!(result.is_err());
}

#[test]
fn expect_close_bracket_wrong_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.expect_close_bracket().map(|s| s.into_data())
  }
  let result: Result<Token, ()> = Parser::new().apply(parse).parse_str("+");
  assert!(result.is_err());
}

// ── try_expect punctuators ──────────────────────────────────────────────────

#[test]
fn try_expect_open_brace_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_open_brace()?.is_some())
  }
  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(!result);
}

#[test]
fn try_expect_close_brace_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_close_brace()?.is_some())
  }
  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(!result);
}

#[test]
fn try_expect_open_bracket_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_open_bracket()?.is_some())
  }
  let result = Parser::new().apply(parse).parse_str("[").unwrap();
  assert!(result);
}

#[test]
fn try_expect_close_bracket_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_close_bracket()?.is_some())
  }
  let result = Parser::new().apply(parse).parse_str("]").unwrap();
  assert!(result);
}

#[test]
fn try_expect_open_paren_decline_on_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_open_paren()?.is_some())
  }
  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert!(!result);
}

#[test]
fn try_expect_close_paren_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.try_expect_close_paren()?.is_some())
  }
  let result = Parser::new().apply(parse).parse_str(")").unwrap();
  assert!(result);
}

// Note: try_expect_equal, try_expect_slash, try_expect_asterisk, try_expect_plus,
// try_expect_hyphen are not tested here because the test Token type does not
// implement those PunctuatorToken methods (they return None by default).

// ── Silent emitter runtime tests ────────────────────────────────────────────

#[test]
fn silent_emitter_new_and_default() {
  let s1: Silent<()> = Silent::new();
  let s2: Silent<()> = Silent::default();
  let s3 = s1;
  let s4 = s2;
  assert_eq!(format!("{:?}", s3), "Silent");
  assert_eq!(format!("{:?}", s4), "Silent");
}

#[test]
fn silent_emitter_allows_parsing_with_wrong_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
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
  }
  let result: Result<Token, ()> = silent_parser!().apply(parse).parse_str("+");
  assert!(result.is_err());
}

#[test]
fn silent_emitter_parse_multiple_tokens() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut tokens = Vec::new();
    while let Some(tok) = inp.next()? {
      tokens.push(tok.into_data());
    }
    Ok(tokens)
  }
  let result = silent_parser!().apply(parse).parse_str("1 + 2").unwrap();
  assert_eq!(result.len(), 3);
}

// ── Ignored emitter tests ───────────────────────────────────────────────────

#[test]
fn ignored_emitter_parse_next_tokens() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut tokens = Vec::new();
    while let Some(tok) = inp.next()? {
      tokens.push(tok.into_data());
    }
    Ok(tokens)
  }
  let result = ignored_parser!().apply(parse).parse_str("1 , ;").unwrap();
  assert_eq!(result.len(), 3);
}

#[test]
fn ignored_emitter_sync_through_skips_unexpected() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }
  let result = ignored_parser!().apply(parse).parse_str("1 + ;").unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

// ── Blackhole cache tests ───────────────────────────────────────────────────

#[test]
fn blackhole_cache_basic_parsing() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.next()?.map(|s| s.into_data()))
  }
  let result = blackhole_parser!().apply(parse).parse_str("42").unwrap();
  assert!(matches!(result, Some(Token::Num(42))));
}

#[test]
fn blackhole_cache_multiple_tokens() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut result = Vec::new();
    while let Some(tok) = inp.next()? {
      result.push(tok.into_data());
    }
    Ok(result)
  }
  let result = blackhole_parser!().apply(parse).parse_str("1 2 3").unwrap();
  assert_eq!(result.len(), 3);
}

#[test]
fn blackhole_cache_empty_input() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.next()?.map(|s| s.into_data()))
  }
  let result = blackhole_parser!().apply(parse).parse_str("").unwrap();
  assert!(result.is_none());
}

#[test]
fn blackhole_cache_is_eoi() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(bool, bool), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let before = inp.is_eoi();
    let _ = inp.next()?;
    let after = inp.is_eoi();
    Ok((before, after))
  }
  let (before, after) = blackhole_parser!().apply(parse).parse_str("42").unwrap();
  assert!(!before);
  assert!(after);
}

#[test]
fn blackhole_cache_try_expect() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let tok = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    Ok(tok.map(|s| s.into_data()))
  }
  let result = blackhole_parser!().apply(parse).parse_str("42").unwrap();
  assert!(matches!(result, Some(Token::Num(42))));
}

#[test]
fn blackhole_cache_try_expect_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let tok = inp.try_expect(|t| matches!(t.data(), Token::Semi))?;
    Ok(tok.map(|s| s.into_data()))
  }
  let result = blackhole_parser!().apply(parse).parse_str("+").unwrap();
  assert!(result.is_none());
}

// ── Errors collection ───────────────────────────────────────────────────────

#[test]
fn errors_display_empty() {
  use tokora::error::Errors;
  let errors: Errors<&str, Vec<&str>> = Errors::from_container(vec![]);
  let display = format!("{}", errors);
  assert_eq!(display, "");
}

#[test]
fn errors_display_single() {
  use tokora::error::Errors;
  let errors: Errors<&str, Vec<&str>> = Errors::from_container(vec!["something went wrong"]);
  let display = format!("{}", errors);
  assert_eq!(display, "something went wrong");
}

#[test]
fn errors_display_multiple() {
  use tokora::error::Errors;
  let errors: Errors<&str, Vec<&str>> =
    Errors::from_container(vec!["error one", "error two", "error three"]);
  let display = format!("{}", errors);
  assert!(display.contains("3 errors:"));
  assert!(display.contains("1. error one"));
  assert!(display.contains("2. error two"));
  assert!(display.contains("3. error three"));
}

#[test]
fn errors_from_single_error() {
  use tokora::error::Errors;
  let errors: Errors<&str> = Errors::from("hello");
  assert_eq!(errors.len(), 1);
}

#[test]
fn errors_from_iterator() {
  use tokora::error::Errors;
  let errors: Errors<i32> = vec![1, 2, 3].into_iter().collect();
  assert_eq!(errors.len(), 3);
}

#[test]
fn errors_into_iterator() {
  use tokora::error::Errors;
  let mut errors: Errors<i32> = Errors::new();
  errors.push(10);
  errors.push(20);
  let sum: i32 = errors.into_iter().sum();
  assert_eq!(sum, 30);
}

#[test]
fn errors_ref_into_iterator() {
  use tokora::error::Errors;
  let mut errors: Errors<i32> = Errors::new();
  errors.push(10);
  errors.push(20);
  let sum: i32 = (&errors).into_iter().sum();
  assert_eq!(sum, 30);
}

#[test]
fn errors_default() {
  use tokora::error::Errors;
  let errors: Errors<&str> = Errors::default();
  assert!(errors.is_empty());
  assert!(!errors.overflowed());
}

#[test]
fn errors_reserve_and_capacity() {
  use tokora::error::Errors;
  let mut errors: Errors<&str> = Errors::new();
  errors.reserve(100);
  assert!(errors.capacity() >= 100);
}

#[test]
fn errors_remaining_capacity_unbounded() {
  use tokora::error::Errors;
  let errors: Errors<&str> = Errors::new();
  assert!(errors.remaining_capacity().is_none());
  assert!(!errors.is_full());
}

#[test]
fn errors_from_container() {
  use std::collections::VecDeque;
  use tokora::error::Errors;
  let mut container = VecDeque::new();
  container.push_back("a");
  container.push_back("b");
  let errors = Errors::<&str>::from_container(container);
  assert_eq!(errors.len(), 2);
}

#[test]
fn errors_debug() {
  use tokora::error::Errors;
  let mut errors: Errors<i32> = Errors::new();
  errors.push(42);
  let debug = format!("{:?}", errors);
  assert!(debug.contains("42"));
}

#[test]
fn errors_clone_and_eq() {
  use tokora::error::Errors;
  let mut errors: Errors<i32> = Errors::new();
  errors.push(1);
  let cloned = errors.clone();
  assert_eq!(errors, cloned);
}

#[test]
fn errors_with_capacity() {
  use tokora::error::Errors;
  let errors: Errors<&str> = Errors::with_capacity(10);
  assert!(errors.capacity() >= 10);
  assert!(errors.is_empty());
}

// ── InputRef::try_expect_map with cache ─────────────────────────────────────

#[test]
fn try_expect_map_after_peek_uses_cache() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.try_expect(|t| matches!(t.data(), Token::Plus))?;
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn try_expect_map_decline_with_cache() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.try_expect(|t| matches!(t.data(), Token::Semi))?;
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

// ── InputRef::try_expect_and_then with cache ────────────────────────────────

#[test]
fn try_expect_and_then_ok_with_cache() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.try_expect(|t| matches!(t.data(), Token::Plus))?;
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) if *n > 0 => Some(Ok(*n)),
      Token::Num(_) => Some(Err(())),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn try_expect_and_then_err_with_cache() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.try_expect(|t| matches!(t.data(), Token::Plus))?;
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) if *n > 100 => Some(Ok(*n)),
        Token::Num(_) => Some(Err(())),
        _ => None,
      })
      .map(|r| r.map(|(n, _)| n))
  }
  let result: Result<Option<i64>, ()> = Parser::new().apply(parse).parse_str("42");
  assert!(result.is_err());
}

#[test]
fn try_expect_and_then_decline_with_cache() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.try_expect(|t| matches!(t.data(), Token::Semi))?;
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

// ── Sequential parsing tests ────────────────────────────────────────────────

#[test]
fn sequential_try_expects() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut tokens = Vec::new();
    while let Some(tok) = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
      tokens.push(tok.into_data());
    }
    Ok(tokens)
  }
  let result = Parser::new().apply(parse).parse_str("1 2 3 +").unwrap();
  assert_eq!(result.len(), 3);
}

#[test]
fn multiple_expects_in_sequence() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Token, Token, Token), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let a = inp.next()?.unwrap().into_data();
    let b = inp.next()?.unwrap().into_data();
    let c = inp.next()?.unwrap().into_data();
    Ok((a, b, c))
  }
  let (a, b, c) = Parser::new().apply(parse).parse_str("1 + 2").unwrap();
  assert!(matches!(a, Token::Num(1)));
  assert!(matches!(b, Token::Plus));
  assert!(matches!(c, Token::Num(2)));
}

// ── Blackhole cache with try_expect_map ─────────────────────────────────────

#[test]
fn blackhole_try_expect_map() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = blackhole_parser!().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn blackhole_try_expect_and_then() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) if *n > 0 => Some(Ok(*n)),
      Token::Num(_) => Some(Err(())),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = blackhole_parser!().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn blackhole_try_expect_and_then_err() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) if *n > 100 => Some(Ok(*n)),
        Token::Num(_) => Some(Err(())),
        _ => None,
      })
      .map(|r| r.map(|(n, _)| n))
  }
  let result: Result<Option<i64>, ()> = blackhole_parser!().apply(parse).parse_str("42");
  assert!(result.is_err());
}

#[test]
fn blackhole_try_expect_and_then_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = blackhole_parser!().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

#[test]
fn blackhole_try_expect_map_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }
  let result = blackhole_parser!().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

// ── Blackhole cache with sync_through ───────────────────────────────────────

#[test]
fn blackhole_sync_through() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }
  let result = blackhole_parser!().apply(parse).parse_str("1 + ;").unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

// ── sync_through edge cases ──────────────────────────────────────────────────

#[test]
fn sync_through_no_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }
  // No semicolon in input - returns None
  let result = ignored_parser!().apply(parse).parse_str("1 + 2").unwrap();
  assert!(result.is_none());
}

#[test]
fn sync_through_empty_input() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }
  let result = ignored_parser!().apply(parse).parse_str("").unwrap();
  assert!(result.is_none());
}

#[test]
fn sync_through_first_token_matches() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }
  let result = Parser::new().apply(parse).parse_str(";").unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

#[test]
fn sync_through_skips_unexpected_to_find_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }
  // Skips multiple unexpected tokens to find semicolon
  let result = ignored_parser!()
    .apply(parse)
    .parse_str("1 + 2 , ;")
    .unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

// ── Expect parser with Expected::one_of ─────────────────────────────────────

#[test]
fn expect_with_multiple_expected() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    expect(|t: &Token| {
      if matches!(t, Token::Num(_) | Token::Plus) {
        Ok(())
      } else {
        Err(Expected::one_of(&[TokenKind::Num, TokenKind::Plus]))
      }
    })
    .parse_input(inp)
  }
  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(matches!(result, Token::Plus));

  let result: Result<Token, ()> = Parser::new().apply(parse).parse_str(",");
  assert!(result.is_err());
}
