use super::*;

use crate::{
  ParseInput, ParserContext, SimpleSpan,
  error::token::{UnexpectedToken, UnexpectedTokenOf},
  input::Cursor,
  lexer::LogosLexer,
  logos::{self, Logos},
  span::Spanned,
  token::Token as TokenTrait,
};

#[derive(Debug, Clone, Logos, PartialEq)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
enum Token {
  #[token("if")]
  If,
  #[token("else")]
  Else,
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
  Ident,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind {
  If,
  Else,
  Ident,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokenKind::If => write!(f, "if"),
      TokenKind::Else => write!(f, "else"),
      TokenKind::Ident => write!(f, "identifier"),
    }
  }
}

impl TokenTrait<'_> for Token {
  type Kind = TokenKind;
  type Error = ();

  fn kind(&self) -> TokenKind {
    match self {
      Token::If => TokenKind::If,
      Token::Else => TokenKind::Else,
      Token::Ident => TokenKind::Ident,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl KeywordToken<'_> for Token {
  fn keyword(&self) -> Option<&'static str> {
    match self {
      Token::If => Some("if"),
      Token::Else => Some("else"),
      Token::Ident => None,
    }
  }
}

type TestLexer<'a> = LogosLexer<'a, Token>;

#[derive(Debug, PartialEq)]
enum E {
  Lex,
  Eot,
  Unexpected { found: Option<TokenKind> },
}

impl From<()> for E {
  fn from(_: ()) -> Self {
    E::Lex
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for E {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    E::Eot
  }
}

impl<'a, S, Lang: ?Sized> From<UnexpectedToken<'a, Token, TokenKind, S, Lang>> for E {
  fn from(err: UnexpectedToken<'a, Token, TokenKind, S, Lang>) -> Self {
    let (_span, found, _expected) = err.into_components();
    E::Unexpected {
      found: found.map(|t| t.kind()),
    }
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
    Err(E::Lex)
  }

  fn emit_unexpected_token(&mut self, _: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(E::Unexpected { found: None })
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
fn parse_exact_errors_on_wrong_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<Token, SimpleSpan>, E> {
    Keyword::parse_exact(&"if").parse_input(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("else");
  assert_eq!(
    r.unwrap_err(),
    E::Unexpected {
      found: Some(TokenKind::Else)
    }
  );
}

#[test]
fn parse_exact_errors_on_empty_input() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<Token, SimpleSpan>, E> {
    Keyword::parse_exact(&"if").parse_input(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("");
  assert_eq!(r.unwrap_err(), E::Eot);
}

#[test]
fn parse_exact_accepts_matching_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<Token, SimpleSpan>, E> {
    Keyword::parse_exact(&"if").parse_input(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("if");
  let (span, tok) = r.unwrap().into_components();
  assert_eq!(tok, Token::If);
  assert_eq!(span, SimpleSpan::new(0, 2));
}

// The sliced keyword payload must be the current keyword's text, so parsing
// two keywords in a row yields each keyword's own slice, not the prefix.
#[test]
fn try_parse_sliced_slices_each_current_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(&'inp str, &'inp str), E> {
    let first = Keyword::try_parse_sliced(inp)?.unwrap_accept();
    let second = Keyword::try_parse_sliced(inp)?.unwrap_accept();
    Ok((first.source(), second.source()))
  }
  let r = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("if else");
  assert_eq!(r.unwrap(), ("if", "else"));
}

#[test]
fn parse_accepts_else_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<Token, SimpleSpan>, E> {
    Keyword::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("else");
  let (span, tok) = r.unwrap().into_components();
  assert_eq!(tok, Token::Else);
  assert_eq!(span, SimpleSpan::new(0, 4));
}

#[test]
fn parse_errors_on_non_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<Token, SimpleSpan>, E> {
    Keyword::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("foo");
  assert_eq!(
    r.unwrap_err(),
    E::Unexpected {
      found: Some(TokenKind::Ident)
    }
  );
}

#[test]
fn parse_errors_on_empty_input() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<Token, SimpleSpan>, E> {
    Keyword::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("");
  assert_eq!(r.unwrap_err(), E::Eot);
}

#[test]
fn parse_accepts_any_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<Token, SimpleSpan>, E> {
    Keyword::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("if");
  let (span, tok) = r.unwrap().into_components();
  assert_eq!(tok, Token::If);
  assert_eq!(span, SimpleSpan::new(0, 2));
}

#[test]
fn parse_sliced_accepts_else_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_sliced(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("else");
  let (span, source) = r.unwrap().into_components();
  assert_eq!(source, "else");
  assert_eq!(span, SimpleSpan::new(0, 4));
}

#[test]
fn parse_sliced_errors_on_non_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_sliced(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("foo");
  assert_eq!(
    r.unwrap_err(),
    E::Unexpected {
      found: Some(TokenKind::Ident)
    }
  );
}

#[test]
fn parse_sliced_errors_on_empty_input() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_sliced(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("");
  assert_eq!(r.unwrap_err(), E::Eot);
}

#[test]
fn parse_sliced_accepts_any_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_sliced(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("if");
  assert_eq!(r.unwrap().source(), "if");
}

#[test]
fn parse_exact_sliced_accepts_matching_keyword_with_span() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_exact_sliced(&"if").parse_input(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("if");
  let (span, source) = r.unwrap().into_components();
  assert_eq!(source, "if");
  assert_eq!(span, SimpleSpan::new(0, 2));
}

#[test]
fn parse_exact_sliced_errors_on_wrong_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_exact_sliced(&"if").parse_input(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("else");
  assert_eq!(
    r.unwrap_err(),
    E::Unexpected {
      found: Some(TokenKind::Else)
    }
  );
}

#[test]
fn parse_exact_sliced_errors_on_empty_input() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_exact_sliced(&"if").parse_input(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("");
  assert_eq!(r.unwrap_err(), E::Eot);
}

#[test]
fn parse_exact_sliced_accepts_matching_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Keyword<&'inp str, SimpleSpan>, E> {
    Keyword::parse_exact_sliced(&"if").parse_input(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("if");
  assert_eq!(r.unwrap().source(), "if");
}
