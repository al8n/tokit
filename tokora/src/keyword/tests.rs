use crate::{
  Emitter, InputRef, Lexer, Parse, Parser, ParserContext, SimpleSpan, Token as TokenTrait,
  error::{
    UnexpectedEot,
    token::{UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  lexer::LogosLexer,
  logos::{self, Logos},
  span::Spanned,
  token::KeywordToken,
};

// A test-local invocation of the `keyword!` macro under test.
keyword! {
  (If, "IF_KW", "if"),
  (Else, "ELSE_KW", "else"),
}

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

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

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

// ── typed parsers: try_parse ────────────────────────────────────────────

#[test]
fn if_try_parse_accepts_if_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(If::try_parse(inp)?.is_accept())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("if");
  assert!(r.unwrap());
}

#[test]
fn if_try_parse_declines_else_without_consuming() {
  // Declining on `else` must not consume it: a following `Else::try_parse`
  // still accepts the same token.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<(bool, bool), E> {
    let declined = If::try_parse(inp)?.is_decline();
    let else_still_there = Else::try_parse(inp)?.is_accept();
    Ok((declined, else_still_there))
  }
  let r: Result<(bool, bool), _> = Parser::with_context(ctx()).apply(parse).parse_str("else");
  assert_eq!(r.unwrap(), (true, true));
}

#[test]
fn if_try_parse_declines_non_keyword() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(If::try_parse(inp)?.is_decline())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("foo");
  assert!(r.unwrap());
}

// ── typed parsers: parse (committed) ────────────────────────────────────

#[test]
fn if_parse_accepts_if_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<If<SimpleSpan>, E> {
    If::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("if");
  assert_eq!(*r.unwrap().span(), SimpleSpan::new(0, 2));
}

#[test]
fn if_parse_errors_on_else_carrying_found_token() {
  // The committed parser falls back to `UnexpectedToken`, which carries the
  // found token (`else`); the emitter error records it.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<If<SimpleSpan>, E> {
    If::parse(inp)
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
fn if_parse_errors_on_empty_input() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<If<SimpleSpan>, E> {
    If::parse(inp)
  }
  let r = Parser::with_context(ctx()).apply(parse).parse_str("");
  assert_eq!(r.unwrap_err(), E::Eot);
}

// ── Lang-defaulted type name compiles bare ──────────────────────────────

#[test]
fn lang_defaulted_type_name_compiles() {
  let kw: If<SimpleSpan> = If::new(SimpleSpan::new(0, 2));
  assert_eq!(kw.as_str(), "if");
}

// ── as_str / raw consistency + PartialEq<str> both directions ───────────

#[test]
fn as_str_and_raw_are_consistent() {
  let kw = If::new(SimpleSpan::new(0, 2));
  assert_eq!(kw.as_str(), "if");
  assert_eq!(If::<SimpleSpan>::raw(), "if");
  assert_eq!(kw.as_str(), If::<SimpleSpan>::raw());
}

#[test]
fn partial_eq_str_round_trip() {
  let kw = If::new(SimpleSpan::new(0, 2));
  assert!(kw == *"if");
  assert!(*"if" == kw);
  assert!(kw != *"else");
  assert!(*"else" != kw);
}

// ── UNIT / unit / change_language ───────────────────────────────────────

#[test]
fn unit_is_zero_sized_and_matches_literal() {
  assert_eq!(core::mem::size_of::<If<()>>(), 0);
  let kw = If::<()>::unit();
  assert_eq!(kw.as_str(), "if");
  assert_eq!(If::<()>::UNIT.as_str(), "if");
}

#[test]
fn change_language_preserves_literal() {
  struct LangA;
  struct LangB;
  let kw: If<SimpleSpan, (), LangA> = If::new(SimpleSpan::new(0, 2)).change_language();
  let kw2: If<SimpleSpan, (), LangB> = kw.change_language();
  assert_eq!(kw2.as_str(), "if");
}

// ── Check impl against KeywordToken ─────────────────────────────────────

#[test]
fn check_matches_only_its_own_keyword() {
  use crate::Check;
  let if_kw = If::new(SimpleSpan::new(0, 2));
  assert!(if_kw.check(&Token::If));
  assert!(!if_kw.check(&Token::Else));
  assert!(!if_kw.check(&Token::Ident));
}
