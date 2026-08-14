use super::*;

use crate::{
  ParserContext, SimpleSpan,
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
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
  Ident,
  #[regex(r"[0-9]+")]
  Num,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind {
  Ident,
  Num,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokenKind::Ident => write!(f, "identifier"),
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
      Token::Ident => TokenKind::Ident,
      Token::Num => TokenKind::Num,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl IdentifierToken<'_> for Token {
  fn is_identifier(&self) -> bool {
    matches!(self, Token::Ident)
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

// Parsing two identifiers in a row must yield each identifier's own text.
// `slice()` returns the current token, not the accumulated consumed prefix.
#[test]
fn try_parse_twice_slices_each_current_ident() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(&'inp str, &'inp str), E> {
    let first = Ident::try_parse(inp)?.unwrap_accept();
    let second = Ident::try_parse(inp)?.unwrap_accept();
    Ok((first.source(), second.source()))
  }
  let r = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("foo bar");
  assert_eq!(r.unwrap(), ("foo", "bar"));
}

#[test]
fn parse_accepts_identifier_with_span() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Ident<&'inp str, SimpleSpan>, E> {
    Ident::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("foo");
  let ident = r.unwrap();
  assert_eq!(ident.source(), "foo");
  assert_eq!(ident.span(), SimpleSpan::new(0, 3));
}

#[test]
fn parse_errors_on_non_identifier() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Ident<&'inp str, SimpleSpan>, E> {
    Ident::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("123");
  assert_eq!(
    r.unwrap_err(),
    E::Unexpected {
      found: Some(TokenKind::Num)
    }
  );
}

#[test]
fn parse_errors_on_empty_input() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Ident<&'inp str, SimpleSpan>, E> {
    Ident::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("");
  assert_eq!(r.unwrap_err(), E::Eot);
}

#[test]
fn parse_accepts_identifier() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Ident<&'inp str, SimpleSpan>, E> {
    Ident::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("bar");
  assert_eq!(r.unwrap().source(), "bar");
}

// ── The exclusion pair ───────────────────────────────────────────────────────

#[test]
fn try_parse_except_declines_an_excluded_spelling_without_consuming_it() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(bool, &'inp str), E> {
    let attempt = Ident::try_parse_except(inp, &["on"])?;
    let declined = matches!(attempt, ParseAttempt::Decline);
    // The decline must be NON-consuming: the same token is still there for the
    // unrestricted parse. A body that ran the exclusion after taking the token would
    // read `after` here and this would be `"after"`.
    let follow = Ident::parse(inp)?;
    Ok((declined, follow.source()))
  }
  let r = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("on after");
  assert_eq!(r.unwrap(), (true, "on"));
}

#[test]
fn try_parse_except_accepts_a_spelling_outside_the_exclusion_set() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<&'inp str, E> {
    Ident::try_parse_except(inp, &["on"]).map(|a| match a {
      ParseAttempt::Accept(id) => id.source(),
      ParseAttempt::Decline => "<declined>",
    })
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("onto");
  assert_eq!(
    r.unwrap(),
    "onto",
    "the exclusion is by whole spelling, not by prefix"
  );
}

#[test]
fn try_parse_except_declines_a_non_identifier_too() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ident::try_parse_except(inp, &["on"]).map(|a| matches!(a, ParseAttempt::Decline))
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("123");
  assert!(r.unwrap());
}

#[test]
fn parse_except_errors_with_the_found_token_on_an_excluded_spelling() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<Ident<&'inp str, SimpleSpan>, E> {
    Ident::parse_except(inp, &["on"])
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("on");
  assert_eq!(
    r.unwrap_err(),
    E::Unexpected {
      found: Some(TokenKind::Ident)
    },
    "the committed form consumes and reports the token it rejected"
  );

  // And it accepts anything outside the set.
  let r = Parser::with_context(ctx()).apply(parse).parse_str("name");
  assert_eq!(r.unwrap().source(), "name");
}
