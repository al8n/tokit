#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests for `ParseInput` impls on `With<Expect, PhantomSpan/PhantomSliced/PhantomLocated>`.
//! These cover lines 142-216 in parser/expect.rs and parse_state.rs accessors.

mod common;

use common::{TestLexer, Token, TokenKind};
use tokora::{
  Emitter, InputRef, Lexer, Located, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan, Token as TokenTrait,
  error::{
    UnexpectedEot,
    token::{UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  parser::{With, expect},
  slice::Sliced,
  span::Spanned,
  utils::{
    Expected,
    marker::{PhantomLocated, PhantomSliced, PhantomSpan},
  },
};

type E = ();

// ── With<Expect, PhantomSpan> — ParseInput ──────────────────────────────────

#[test]
fn expect_parse_input_with_spanned_ok() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<Token, SimpleSpan>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser = expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    });
    let mut with_spanned: With<_, PhantomSpan> = With::new(expect_parser, PhantomSpan::phantom());
    with_spanned.parse_input(inp)
  }
  let r = Parser::new().apply(parse).parse_str("42");
  let spanned = r.unwrap();
  assert!(matches!(spanned.data(), Token::Num(42)));
  assert_eq!(spanned.span(), SimpleSpan::new(0, 2));
}

#[test]
fn expect_parse_input_with_spanned_err() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Spanned<Token, SimpleSpan>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser = expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    });
    let mut with_spanned: With<_, PhantomSpan> = With::new(expect_parser, PhantomSpan::phantom());
    with_spanned.parse_input(inp)
  }
  let r = Parser::new().apply(parse).parse_str("+");
  assert!(r.is_err());
}

// ── With<Expect, PhantomSliced> — ParseInput ────────────────────────────────

#[test]
fn expect_parse_input_with_sliced_ok() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Sliced<Token, &'inp str>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser = expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    });
    let mut with_sliced: With<_, PhantomSliced> =
      With::new(expect_parser, PhantomSliced::phantom());
    with_sliced.parse_input(inp)
  }
  let r = Parser::new().apply(parse).parse_str("42");
  let sliced = r.unwrap();
  assert!(matches!(sliced.data(), Token::Num(42)));
}

#[test]
fn expect_parse_input_with_sliced_err() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Sliced<Token, &'inp str>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser = expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    });
    let mut with_sliced: With<_, PhantomSliced> =
      With::new(expect_parser, PhantomSliced::phantom());
    with_sliced.parse_input(inp)
  }
  let r = Parser::new().apply(parse).parse_str("+");
  assert!(r.is_err());
}

// `With<Expect, PhantomSliced>` captures the current token via `slice()`, so the
// second token in a row must slice to its own text, not the whole consumed prefix.
#[test]
fn expect_with_sliced_slices_each_current_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(&'inp str, &'inp str), E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let mut first: With<_, PhantomSliced> = With::new(
      expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
        if matches!(t, Token::Num(_)) {
          Ok(())
        } else {
          Err(Expected::one(TokenKind::Num))
        }
      }),
      PhantomSliced::phantom(),
    );
    let a = first.parse_input(inp)?;
    let mut second: With<_, PhantomSliced> = With::new(
      expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
        if matches!(t, Token::Num(_)) {
          Ok(())
        } else {
          Err(Expected::one(TokenKind::Num))
        }
      }),
      PhantomSliced::phantom(),
    );
    let b = second.parse_input(inp)?;
    Ok((a.slice(), b.slice()))
  }
  let r = Parser::new().apply(parse).parse_str("12 34");
  assert_eq!(r.unwrap(), ("12", "34"));
}

// `With<Expect, PhantomLocated>` likewise must report the second token's own
// span and slice, not the whole consumed prefix.
#[test]
fn expect_with_located_reports_each_current_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(SimpleSpan, &'inp str), E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let mut first: With<_, PhantomLocated> = With::new(
      expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
        if matches!(t, Token::Num(_)) {
          Ok(())
        } else {
          Err(Expected::one(TokenKind::Num))
        }
      }),
      PhantomLocated::phantom(),
    );
    let _ = first.parse_input(inp)?;
    let mut second: With<_, PhantomLocated> = With::new(
      expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
        if matches!(t, Token::Num(_)) {
          Ok(())
        } else {
          Err(Expected::one(TokenKind::Num))
        }
      }),
      PhantomLocated::phantom(),
    );
    let b = second.parse_input(inp)?;
    Ok((b.span(), b.slice()))
  }
  let r = Parser::new().apply(parse).parse_str("12 34");
  assert_eq!(r.unwrap(), (SimpleSpan::new(3, 5), "34"));
}

// ── With<Expect, PhantomLocated> — ParseInput ───────────────────────────────

#[test]
fn expect_parse_input_with_located_ok() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Located<Token, SimpleSpan, &'inp str>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser = expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    });
    let mut with_located: With<_, PhantomLocated> =
      With::new(expect_parser, PhantomLocated::phantom());
    with_located.parse_input(inp)
  }
  let r = Parser::new().apply(parse).parse_str("42");
  let located = r.unwrap();
  assert!(matches!(located.data(), Token::Num(42)));
  assert_eq!(located.span(), SimpleSpan::new(0, 2));
}

#[test]
fn expect_parse_input_with_located_err() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Located<Token, SimpleSpan, &'inp str>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser = expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    });
    let mut with_located: With<_, PhantomLocated> =
      With::new(expect_parser, PhantomLocated::phantom());
    with_located.parse_input(inp)
  }
  let r = Parser::new().apply(parse).parse_str("+");
  assert!(r.is_err());
}

// ── ParseState accessor coverage ────────────────────────────────────────────
// parse_state.rs: span(), emitter(), state(), state_mut(), slice()

#[test]
fn parse_state_accessors_via_map() {
  use tokora::ParseState;
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    })
    .map_with(
      |tok: Token, mut ps: ParseState<'_, 'inp, '_, TestLexer<'inp>, Ctx>| {
        // Exercise all parse_state accessors
        let _span = ps.span();
        let _emitter = ps.emitter_ref();
        let _state = ps.state();
        let _state_mut = ps.state_mut();
        let _slice = ps.slice();
        tok
      },
    )
    .parse_input(inp)
  }
  let r = Parser::new().apply(parse).parse_str("42");
  assert!(matches!(r.unwrap(), Token::Num(42)));
}

// ── try_expect coverage ──────────────────────────────────────────────────────

#[derive(Debug)]
struct TryExpectE;
impl From<()> for TryExpectE {
  fn from(_: ()) -> Self {
    TryExpectE
  }
}
impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for TryExpectE
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    TryExpectE
  }
}
impl From<UnexpectedEot> for TryExpectE {
  fn from(_: UnexpectedEot) -> Self {
    TryExpectE
  }
}

struct TestEm;
impl<'inp> Emitter<'inp, TestLexer<'inp>> for TestEm {
  type Error = TryExpectE;
  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), TryExpectE>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(TryExpectE)
  }
  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), TryExpectE>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(TryExpectE)
  }
  fn emit_error(
    &mut self,
    err: Spanned<TryExpectE, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), TryExpectE>
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

fn ctx() -> ParserContext<'static, TestLexer<'static>, TestEm> {
  ParserContext::new(TestEm)
}

// ── try_expect_<punct> methods ──────────────────────────────────────────────

#[test]
fn try_expect_comma_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_comma()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str(",");
  assert!(r.unwrap());
}

#[test]
fn try_expect_comma_decline() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_comma()?.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert!(r.unwrap());
}

#[test]
fn try_expect_semicolon() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_semicolon()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str(";");
  assert!(r.unwrap());
}

#[test]
fn try_expect_open_paren() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_open_paren()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("(");
  assert!(r.unwrap());
}

#[test]
fn try_expect_close_paren() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_close_paren()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str(")");
  assert!(r.unwrap());
}

#[test]
fn try_expect_open_bracket() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_open_bracket()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("[");
  assert!(r.unwrap());
}

#[test]
fn try_expect_close_bracket() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_close_bracket()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("]");
  assert!(r.unwrap());
}

#[test]
fn try_expect_open_brace() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_open_brace()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("{");
  assert!(r.unwrap());
}

#[test]
fn try_expect_close_brace() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    Ok(inp.try_expect_close_brace()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("}");
  assert!(r.unwrap());
}

// ── try_expect with empty cache ─────────────────────────────────────────────

#[test]
fn try_expect_empty_cache_match() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    let tok = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    Ok(tok.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert!(r.unwrap());
}

#[test]
fn try_expect_empty_cache_no_match() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    let tok = inp.try_expect(|t| matches!(t.data(), Token::Comma))?;
    Ok(tok.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert!(r.unwrap());
}

// ── try_expect_map with empty cache ─────────────────────────────────────────

#[test]
fn try_expect_map_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Option<i64>, TryExpectE> {
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _)| n))
  }
  let r: Result<Option<i64>, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), Some(42));
}

#[test]
fn try_expect_map_decline() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    let result = inp.try_expect_map::<i64, _>(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str(",");
  assert!(r.unwrap());
}

// ── try_expect_and_then ─────────────────────────────────────────────────────

#[test]
fn try_expect_and_then_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Option<i64>, TryExpectE> {
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.map(|(n, _)| n))
  }
  let r: Result<Option<i64>, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), Some(42));
}

#[test]
fn try_expect_and_then_decline() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    let result = inp.try_expect_and_then::<i64, _>(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str(",");
  assert!(r.unwrap());
}

#[test]
fn try_expect_and_then_cached_decline() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, TryExpectE> {
    let _ = inp.peek_one()?;
    let result = inp.try_expect_and_then::<i64, _>(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str(",");
  assert!(r.unwrap());
}

#[test]
fn try_expect_and_then_cached_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Option<i64>, TryExpectE> {
    let _ = inp.peek_one()?;
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.map(|(n, _)| n))
  }
  let r: Result<Option<i64>, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), Some(42));
}
