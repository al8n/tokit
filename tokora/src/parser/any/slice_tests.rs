use super::*;

use crate::{
  ParseInput, ParserContext,
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
  #[regex(r"[0-9]+")]
  Num,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind {
  Num,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokenKind::Num => write!(f, "number"),
    }
  }
}

impl TokenTrait<'_> for Token {
  type Kind = TokenKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> TokenKind {
    match self {
      Token::Num => TokenKind::Num,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type TestLexer<'a> = LogosLexer<'a, Token>;

#[derive(Debug, PartialEq)]
enum E {
  Lex,
  Eot,
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

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for E {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    E::Lex
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
    Err(E::Lex)
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

// `Any::sliced()` captures the current token's text via `slice()`, so the
// second token in a row must slice to its own text, not the whole prefix.
#[test]
fn any_sliced_slices_each_current_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(&'inp str, &'inp str), E> {
    let first = Any::<TestLexer<'inp>, _>::sliced().parse_input(inp)?;
    let second = Any::<TestLexer<'inp>, _>::sliced().parse_input(inp)?;
    Ok((first.slice(), second.slice()))
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("12 34");
  assert_eq!(r.unwrap(), ("12", "34"));
}
