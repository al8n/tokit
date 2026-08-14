use super::*;

use crate::{
  ParserContext,
  error::token::UnexpectedTokenOf,
  input::Cursor,
  lexer::LogosLexer,
  logos::{self, Logos},
  span::Spanned,
  token::Token as TokenTrait,
};

#[derive(Debug, Clone, Logos, PartialEq)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
enum Token {
  #[token("...")]
  Spread,
  #[token("<<")]
  ShiftLeft,
  #[token("+=")]
  PlusEqual,
  #[regex(r"[0-9]+")]
  Num,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind {
  Spread,
  At,
  ShiftLeft,
  PlusEqual,
  Num,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokenKind::Spread => write!(f, "..."),
      TokenKind::At => write!(f, "@"),
      TokenKind::ShiftLeft => write!(f, "<<"),
      TokenKind::PlusEqual => write!(f, "+="),
      TokenKind::Num => write!(f, "number"),
    }
  }
}

impl TokenTrait<'_> for Token {
  type Kind = TokenKind;
  type Error = ();

  const READ_FRONTIER_CLASS: crate::ReadFrontierClass = crate::ReadFrontierClass::Unbounded;

  fn kind(&self) -> TokenKind {
    match self {
      Token::Spread => TokenKind::Spread,
      Token::ShiftLeft => TokenKind::ShiftLeft,
      Token::PlusEqual => TokenKind::PlusEqual,
      Token::Num => TokenKind::Num,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl PunctuatorToken<'_> for Token {
  fn spread() -> Option<Self::Kind> {
    Some(TokenKind::Spread)
  }

  fn shl() -> Option<Self::Kind> {
    Some(TokenKind::ShiftLeft)
  }

  fn plus_equal() -> Option<Self::Kind> {
    Some(TokenKind::PlusEqual)
  }
}

impl From<At<(), (), ()>> for TokenKind {
  fn from(_: At<(), (), ()>) -> Self {
    TokenKind::At
  }
}

impl From<Spread<(), (), ()>> for TokenKind {
  fn from(_: Spread<(), (), ()>) -> Self {
    TokenKind::Spread
  }
}

type TestLexer<'a> = LogosLexer<'a, Token>;

#[derive(Debug)]
struct E;

impl From<()> for E {
  fn from(_: ()) -> Self {
    E
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for E {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    E
  }
}

impl From<UnexpectedEot> for E {
  fn from(_: UnexpectedEot) -> Self {
    E
  }
}

struct TestEm;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for TestEm {
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

  fn emit_unexpected_token(&mut self, _: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Result<(), E>
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

fn ctx() -> ParserContext<'static, TestLexer<'static>, TestEm> {
  ParserContext::new(TestEm)
}

#[test]
fn spread_try_parse_accepts_spread_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(Spread::try_parse(inp)?.is_accept())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("...");
  assert!(r.unwrap());
}

#[test]
fn spread_try_parse_declines_non_spread_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(bool, bool), E> {
    let declined = Spread::try_parse(inp)?.is_decline();
    let next_is_num = inp
      .try_expect(|t| t.data.kind() == TokenKind::Num)?
      .is_some();
    Ok((declined, next_is_num))
  }
  let r: Result<(bool, bool), _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  let (declined, next_is_num) = r.unwrap();
  assert!(declined);
  assert!(next_is_num);
}

#[test]
fn shift_left_try_parse_accepts_shl_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(ShiftLeft::try_parse(inp)?.is_accept())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("<<");
  assert!(r.unwrap());
}

#[test]
fn shift_left_try_parse_declines_non_shl_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(bool, bool), E> {
    let declined = ShiftLeft::try_parse(inp)?.is_decline();
    let next_is_num = inp
      .try_expect(|t| t.data.kind() == TokenKind::Num)?
      .is_some();
    Ok((declined, next_is_num))
  }
  let r: Result<(bool, bool), _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  let (declined, next_is_num) = r.unwrap();
  assert!(declined);
  assert!(next_is_num);
}

#[test]
fn plus_equal_try_parse_accepts_plus_equal_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(PlusEqual::try_parse(inp)?.is_accept())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("+=");
  assert!(r.unwrap());
}

#[test]
fn plus_equal_try_parse_declines_non_plus_equal_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(bool, bool), E> {
    let declined = PlusEqual::try_parse(inp)?.is_decline();
    let next_is_num = inp
      .try_expect(|t| t.data.kind() == TokenKind::Num)?
      .is_some();
    Ok((declined, next_is_num))
  }
  let r: Result<(bool, bool), _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  let (declined, next_is_num) = r.unwrap();
  assert!(declined);
  assert!(next_is_num);
}

#[test]
fn punctuator_name_returns_screaming_snake() {
  assert_eq!(
    <At<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::name().as_str(),
    "AT"
  );
  assert_eq!(
    <Spread<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::name().as_str(),
    "SPREAD"
  );
}
