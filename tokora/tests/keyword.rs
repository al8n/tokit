#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
#![allow(unused_imports)]

//! Tests for parser/keyword.rs -- Keyword::try_parse, try_parse_sliced,
//! try_parse_exact, try_parse_exact_sliced and their _of variants.

use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, Token as TokenT, TryParseInput,
  error::UnexpectedEot,
  logos::{self, Logos},
  token::{KeywordToken, PunctuatorToken},
  try_parse_input::ParseAttempt,
  types::{Components, Keyword},
};

// ── Token with keywords ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Logos, PartialEq)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
pub enum KwToken {
  #[token("let")]
  KwLet,
  #[token("if")]
  KwIf,
  #[token("return")]
  KwReturn,
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
  Ident,
  #[regex(r"-?[0-9]+", |lex| lex.slice().parse::<i64>().unwrap_or(0))]
  Num(i64),
  #[token("+")]
  Plus,
  #[token(",")]
  Comma,
  #[token(";")]
  Semi,
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
  #[token("[")]
  LBracket,
  #[token("]")]
  RBracket,
  #[token("{")]
  LBrace,
  #[token("}")]
  RBrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KwTokenKind {
  KwLet,
  KwIf,
  KwReturn,
  Ident,
  Num,
  Plus,
  Comma,
  Semi,
  LParen,
  RParen,
  LBracket,
  RBracket,
  LBrace,
  RBrace,
}

impl core::fmt::Display for KwTokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      KwTokenKind::KwLet => write!(f, "let"),
      KwTokenKind::KwIf => write!(f, "if"),
      KwTokenKind::KwReturn => write!(f, "return"),
      KwTokenKind::Ident => write!(f, "identifier"),
      KwTokenKind::Num => write!(f, "number"),
      KwTokenKind::Plus => write!(f, "+"),
      KwTokenKind::Comma => write!(f, ","),
      KwTokenKind::Semi => write!(f, ";"),
      KwTokenKind::LParen => write!(f, "("),
      KwTokenKind::RParen => write!(f, ")"),
      KwTokenKind::LBracket => write!(f, "["),
      KwTokenKind::RBracket => write!(f, "]"),
      KwTokenKind::LBrace => write!(f, "{{"),
      KwTokenKind::RBrace => write!(f, "}}"),
    }
  }
}

impl core::fmt::Display for KwToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      KwToken::KwLet => write!(f, "let"),
      KwToken::KwIf => write!(f, "if"),
      KwToken::KwReturn => write!(f, "return"),
      KwToken::Ident => write!(f, "identifier"),
      KwToken::Num(n) => write!(f, "{n}"),
      KwToken::Plus => write!(f, "+"),
      KwToken::Comma => write!(f, ","),
      KwToken::Semi => write!(f, ";"),
      KwToken::LParen => write!(f, "("),
      KwToken::RParen => write!(f, ")"),
      KwToken::LBracket => write!(f, "["),
      KwToken::RBracket => write!(f, "]"),
      KwToken::LBrace => write!(f, "{{"),
      KwToken::RBrace => write!(f, "}}"),
    }
  }
}

impl From<&KwToken> for KwTokenKind {
  fn from(t: &KwToken) -> Self {
    match t {
      KwToken::KwLet => KwTokenKind::KwLet,
      KwToken::KwIf => KwTokenKind::KwIf,
      KwToken::KwReturn => KwTokenKind::KwReturn,
      KwToken::Ident => KwTokenKind::Ident,
      KwToken::Num(_) => KwTokenKind::Num,
      KwToken::Plus => KwTokenKind::Plus,
      KwToken::Comma => KwTokenKind::Comma,
      KwToken::Semi => KwTokenKind::Semi,
      KwToken::LParen => KwTokenKind::LParen,
      KwToken::RParen => KwTokenKind::RParen,
      KwToken::LBracket => KwTokenKind::LBracket,
      KwToken::RBracket => KwTokenKind::RBracket,
      KwToken::LBrace => KwTokenKind::LBrace,
      KwToken::RBrace => KwTokenKind::RBrace,
    }
  }
}

impl TokenT<'_> for KwToken {
  type Kind = KwTokenKind;
  type Error = ();

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  fn kind(&self) -> KwTokenKind {
    KwTokenKind::from(self)
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl PunctuatorToken<'_> for KwToken {
  fn comma() -> Option<Self::Kind> {
    Some(KwTokenKind::Comma)
  }
  fn semicolon() -> Option<Self::Kind> {
    Some(KwTokenKind::Semi)
  }
  fn open_paren() -> Option<Self::Kind> {
    Some(KwTokenKind::LParen)
  }
  fn close_paren() -> Option<Self::Kind> {
    Some(KwTokenKind::RParen)
  }
  fn open_bracket() -> Option<Self::Kind> {
    Some(KwTokenKind::LBracket)
  }
  fn close_bracket() -> Option<Self::Kind> {
    Some(KwTokenKind::RBracket)
  }
  fn open_brace() -> Option<Self::Kind> {
    Some(KwTokenKind::LBrace)
  }
  fn close_brace() -> Option<Self::Kind> {
    Some(KwTokenKind::RBrace)
  }
}

impl KeywordToken<'_> for KwToken {
  fn keyword(&self) -> Option<&'static str> {
    match self {
      KwToken::KwLet => Some("let"),
      KwToken::KwIf => Some("if"),
      KwToken::KwReturn => Some("return"),
      _ => None,
    }
  }
}

// ── Punct From impls ────────────────────────────────────────────────────────

use tokora::punct::{
  CloseBrace, CloseBracket, CloseParen, Comma, OpenBrace, OpenBracket, OpenParen, Semicolon,
};

impl From<Comma<(), (), ()>> for KwTokenKind {
  fn from(_: Comma<(), (), ()>) -> Self {
    KwTokenKind::Comma
  }
}
impl From<Semicolon<(), (), ()>> for KwTokenKind {
  fn from(_: Semicolon<(), (), ()>) -> Self {
    KwTokenKind::Semi
  }
}
impl From<OpenParen<(), (), ()>> for KwTokenKind {
  fn from(_: OpenParen<(), (), ()>) -> Self {
    KwTokenKind::LParen
  }
}
impl From<CloseParen<(), (), ()>> for KwTokenKind {
  fn from(_: CloseParen<(), (), ()>) -> Self {
    KwTokenKind::RParen
  }
}
impl From<OpenBracket<(), (), ()>> for KwTokenKind {
  fn from(_: OpenBracket<(), (), ()>) -> Self {
    KwTokenKind::LBracket
  }
}
impl From<CloseBracket<(), (), ()>> for KwTokenKind {
  fn from(_: CloseBracket<(), (), ()>) -> Self {
    KwTokenKind::RBracket
  }
}
impl From<OpenBrace<(), (), ()>> for KwTokenKind {
  fn from(_: OpenBrace<(), (), ()>) -> Self {
    KwTokenKind::LBrace
  }
}
impl From<CloseBrace<(), (), ()>> for KwTokenKind {
  fn from(_: CloseBrace<(), (), ()>) -> Self {
    KwTokenKind::RBrace
  }
}

// ── Error type ──────────────────────────────────────────────────────────────

impl<S, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<S, Lang, Set>> for KwTestError {
  fn from(_: UnexpectedEot<S, Lang, Set>) -> Self {
    KwTestError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for KwTestError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    KwTestError
  }
}

use tokora::error::token::UnexpectedToken;

#[derive(Debug)]
struct KwTestError;

impl From<()> for KwTestError {
  fn from(_: ()) -> Self {
    KwTestError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for KwTestError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    KwTestError
  }
}

// ── Aliases ─────────────────────────────────────────────────────────────────

type KwLexer<'a> = tokora::lexer::LogosLexer<'a, KwToken>;

// ── Tests: Keyword::try_parse ───────────────────────────────────────────────

#[test]
fn keyword_try_parse_accept() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse(inp)?;
    Ok(matches!(result, ParseAttempt::Accept(_)))
  }

  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("let");
  assert!(r.unwrap());
}

#[test]
fn keyword_try_parse_decline_on_non_keyword() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("foo");
  assert!(r.unwrap());
}

#[test]
fn keyword_try_parse_returns_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>,
  ) -> Result<Option<KwToken>, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse(inp)?;
    Ok(match result {
      ParseAttempt::Accept(kw) => {
        let Components { payload: tok, .. } = kw.into_components();
        Some(tok)
      }
      ParseAttempt::Decline => None,
    })
  }

  let r = Parser::new().apply(parse).parse_str("if").unwrap();
  assert_eq!(r, Some(KwToken::KwIf));
}

// ── Tests: Keyword::try_parse_sliced ────────────────────────────────────────

#[test]
fn keyword_try_parse_sliced_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>,
  ) -> Result<Option<&'inp str>, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_sliced(inp)?;
    Ok(match result {
      ParseAttempt::Accept(kw) => Some({
        let Components { payload: src, .. } = kw.into_components();
        src
      }),
      ParseAttempt::Decline => None,
    })
  }

  let r = Parser::new().apply(parse).parse_str("return").unwrap();
  assert_eq!(r, Some("return"));
}

#[test]
fn keyword_try_parse_sliced_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_sliced(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("42");
  assert!(r.unwrap());
}

// ── Tests: Keyword::try_parse_exact ─────────────────────────────────────────

#[test]
fn keyword_try_parse_exact_accept() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_exact(&"let").try_parse_input(inp)?;
    Ok(matches!(result, ParseAttempt::Accept(_)))
  }

  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("let");
  assert!(r.unwrap());
}

#[test]
fn keyword_try_parse_exact_decline_wrong_keyword() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_exact(&"let").try_parse_input(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  // "if" is a keyword, but not "let"
  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("if");
  assert!(r.unwrap());
}

#[test]
fn keyword_try_parse_exact_decline_non_keyword() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_exact(&"let").try_parse_input(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  // "foo" is not a keyword
  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("foo");
  assert!(r.unwrap());
}

#[test]
fn keyword_try_parse_exact_returns_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>,
  ) -> Result<Option<KwToken>, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_exact(&"return").try_parse_input(inp)?;
    Ok(match result {
      ParseAttempt::Accept(kw) => Some({
        let Components { payload: src, .. } = kw.into_components();
        src
      }),
      ParseAttempt::Decline => None,
    })
  }

  let r = Parser::new().apply(parse).parse_str("return").unwrap();
  assert_eq!(r, Some(KwToken::KwReturn));
}

// ── Tests: Keyword::try_parse_exact_sliced ──────────────────────────────────

#[test]
fn keyword_try_parse_exact_sliced_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>,
  ) -> Result<Option<&'inp str>, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_exact_sliced(&"if").try_parse_input(inp)?;
    Ok(match result {
      ParseAttempt::Accept(kw) => Some({
        let Components { payload: src, .. } = kw.into_components();
        src
      }),
      ParseAttempt::Decline => None,
    })
  }

  let r = Parser::new().apply(parse).parse_str("if").unwrap();
  assert_eq!(r, Some("if"));
}

#[test]
fn keyword_try_parse_exact_sliced_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_exact_sliced(&"if").try_parse_input(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("let");
  assert!(r.unwrap());
}

// ── Tests: Keyword::try_parse on empty input ────────────────────────────────

#[test]
fn keyword_try_parse_on_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("");
  assert!(r.unwrap());
}

#[test]
fn keyword_try_parse_sliced_on_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>) -> Result<bool, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let result = Keyword::try_parse_sliced(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  let r: Result<bool, KwTestError> = Parser::new().apply(parse).parse_str("");
  assert!(r.unwrap());
}

// ── Tests: multiple keywords in sequence ────────────────────────────────────

#[test]
fn keyword_try_parse_multiple() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, KwLexer<'inp>, Ctx>,
  ) -> Result<Vec<KwToken>, KwTestError>
  where
    Ctx: ParseContext<'inp, KwLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, KwLexer<'inp>, Error = KwTestError>,
  {
    let mut keywords = Vec::new();
    loop {
      let result = Keyword::try_parse(inp)?;
      match result {
        ParseAttempt::Accept(kw) => keywords.push({
          let Components { payload: src, .. } = kw.into_components();
          src
        }),
        ParseAttempt::Decline => break,
      }
    }
    Ok(keywords)
  }

  let r = Parser::new()
    .apply(parse)
    .parse_str("let if return foo")
    .unwrap();
  assert_eq!(r, vec![KwToken::KwLet, KwToken::KwIf, KwToken::KwReturn]);
}

// Integration tests covering all generated methods from the `keyword!` and `punctuator!` macros.

use std::borrow::Borrow;
use std::fmt::Write as _;

use tokora::__private::span::{AsSpan, IntoSpan};
use tokora::__private::utils::IntoComponents;
use tokora::__private::utils::human_display::DisplayHuman;
use tokora::__private::utils::sdl_display::{DisplayCompact, DisplayPretty};
use tokora::span::SimpleSpan;

// ── Define a keyword and punctuator for testing ──────────────────────────────

tokora::keyword! {
  (TestKw, "TEST_KW", "test_keyword"),
  (AnotherKw, "ANOTHER_KW", "another"),
  // A keyword whose literal contains brace characters. Under the old
  // `write!(f, $kw)` Display impl this failed to compile (the literal was
  // parsed as a format string); the literal must be written verbatim.
  (BraceKw, "BRACE_KW", "brace{tok}"),
}

tokora::punctuator! {
  (TestPunct, "TEST_PUNCT", "@@"),
  (AnotherPunct, "ANOTHER_PUNCT", "##"),
}

// ═══════════════════════════════════════════════════════════════════════════════
//  Keyword tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn keyword_new() {
  let span = SimpleSpan::new(0, 5);
  let kw = TestKw::new(span);
  assert_eq!(*kw.span(), span);
}

#[test]
fn keyword_with_braces_displays_verbatim() {
  let kw = BraceKw::new(SimpleSpan::new(0, 10));
  assert_eq!(format!("{kw}"), "brace{tok}");
  assert_eq!(BraceKw::<SimpleSpan>::raw(), "brace{tok}");
}

#[test]
fn keyword_with_content() {
  let span = SimpleSpan::new(1, 10);
  let kw = TestKw::with_content(span, "some_content");
  assert_eq!(*kw.span(), span);
  assert_eq!(*kw.content(), "some_content");
}

#[test]
fn keyword_raw() {
  assert_eq!(TestKw::<SimpleSpan>::raw(), "test_keyword");
  assert_eq!(AnotherKw::<SimpleSpan>::raw(), "another");
}

#[test]
fn keyword_as_ref() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let s: &str = kw.as_ref();
  assert_eq!(s, "test_keyword");
}

#[test]
fn keyword_borrow() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let s: &str = kw.borrow();
  assert_eq!(s, "test_keyword");
}

#[test]
fn keyword_display() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let s = format!("{}", kw);
  assert_eq!(s, "test_keyword");
}

#[test]
fn keyword_debug() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let dbg = format!("{:?}", kw);
  assert!(dbg.contains("TestKw"));
}

#[test]
fn keyword_clone_copy() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let kw2 = kw;
  let kw3 = kw;
  assert_eq!(kw2, kw3);
}

#[test]
fn keyword_eq_hash() {
  use std::collections::HashSet;
  let kw1 = TestKw::new(SimpleSpan::new(0, 1));
  let kw2 = TestKw::new(SimpleSpan::new(0, 1));
  assert_eq!(kw1, kw2);
  let mut set = HashSet::new();
  set.insert(kw1);
  assert!(set.contains(&kw2));
}

#[test]
fn keyword_as_span() {
  let span = SimpleSpan::new(5, 15);
  let kw = TestKw::new(span);
  assert_eq!(*AsSpan::as_span(&kw), span);
}

#[test]
fn keyword_into_span() {
  let span = SimpleSpan::new(5, 15);
  let kw = TestKw::new(span);
  let s: SimpleSpan = IntoSpan::into_span(kw);
  assert_eq!(s, span);
}

#[test]
fn keyword_into_components() {
  let span = SimpleSpan::new(2, 8);
  let kw = TestKw::with_content(span, 42u32);
  let (s, c) = kw.into_components();
  assert_eq!(s, span);
  assert_eq!(c, 42u32);
}

#[test]
fn keyword_display_human() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let mut buf = String::new();
  // Use the DisplayHuman trait via a manual Formatter invocation
  // The simplest way is through format_args + write
  struct HumanWrapper<'a, T: DisplayHuman>(&'a T);
  impl<T: DisplayHuman> std::fmt::Display for HumanWrapper<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      DisplayHuman::fmt(self.0, f)
    }
  }
  write!(buf, "{}", HumanWrapper(&kw)).unwrap();
  assert_eq!(buf, "test_keyword");
}

#[test]
fn keyword_display_compact() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let mut buf = String::new();
  struct CompactWrapper<'a, T: DisplayCompact<Options = ()>>(&'a T);
  impl<T: DisplayCompact<Options = ()>> std::fmt::Display for CompactWrapper<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      DisplayCompact::fmt(self.0, f, &())
    }
  }
  write!(buf, "{}", CompactWrapper(&kw)).unwrap();
  assert_eq!(buf, "test_keyword");
}

#[test]
fn keyword_display_pretty() {
  let kw = TestKw::new(SimpleSpan::new(0, 1));
  let mut buf = String::new();
  struct PrettyWrapper<'a, T: DisplayPretty<Options = ()>>(&'a T);
  impl<T: DisplayPretty<Options = ()>> std::fmt::Display for PrettyWrapper<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      DisplayPretty::fmt(self.0, f, &())
    }
  }
  write!(buf, "{}", PrettyWrapper(&kw)).unwrap();
  assert_eq!(buf, "test_keyword");
}

#[test]
fn keyword_another_variant() {
  let kw = AnotherKw::new(SimpleSpan::new(0, 7));
  assert_eq!(AnotherKw::<SimpleSpan>::raw(), "another");
  assert_eq!(format!("{}", kw), "another");
  let s: &str = kw.as_ref();
  assert_eq!(s, "another");
  let b: &str = kw.borrow();
  assert_eq!(b, "another");
}

// ═══════════════════════════════════════════════════════════════════════════════
//  Punctuator tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn punct_unit() {
  let p = TestPunct::unit();
  assert_eq!(p.as_str(), "@@");
  assert_eq!(std::mem::size_of::<TestPunct<()>>(), 0);
}

#[test]
fn punct_unit_const() {
  let _p = TestPunct::UNIT;
  assert_eq!(TestPunct::UNIT.as_str(), "@@");
}

#[test]
fn punct_new() {
  let span = SimpleSpan::new(0, 2);
  let p = TestPunct::<SimpleSpan>::new(span);
  assert_eq!(*p.span(), span);
  assert_eq!(p.as_str(), "@@");
}

#[test]
fn punct_with_content() {
  let span = SimpleSpan::new(3, 5);
  let p = TestPunct::<SimpleSpan, &str>::with_content(span, "content");
  assert_eq!(*p.span(), span);
  assert_eq!(*p.content(), "content");
}

#[test]
fn punct_raw() {
  assert_eq!(TestPunct::raw(), "@@");
  assert_eq!(AnotherPunct::raw(), "##");
}

#[test]
fn punct_as_str() {
  let p = TestPunct::unit();
  assert_eq!(p.as_str(), "@@");
}

#[test]
fn punct_as_ref() {
  let p = TestPunct::unit();
  let s: &str = p.as_ref();
  assert_eq!(s, "@@");
}

#[test]
fn punct_borrow() {
  let p = TestPunct::unit();
  let s: &str = p.borrow();
  assert_eq!(s, "@@");
}

#[test]
fn punct_display() {
  let p = TestPunct::unit();
  assert_eq!(format!("{}", p), "@@");
}

#[test]
fn punct_debug() {
  let p = TestPunct::unit();
  let dbg = format!("{:?}", p);
  assert!(dbg.contains("TestPunct"));
}

#[test]
fn punct_clone_copy() {
  let p = TestPunct::unit();
  let p2 = p;
  let p3 = p;
  assert_eq!(p2, p3);
}

#[test]
fn punct_eq_hash() {
  use std::collections::HashSet;
  let p1 = TestPunct::unit();
  let p2 = TestPunct::unit();
  assert_eq!(p1, p2);
  let mut set = HashSet::new();
  set.insert(p1);
  assert!(set.contains(&p2));
}

#[test]
fn punct_partial_eq_str() {
  let p = TestPunct::unit();
  assert!(p == *"@@");
  assert!(!(p == *"##"));
}

#[test]
fn str_partial_eq_punct() {
  let p = TestPunct::unit();
  assert!(*"@@" == p);
  assert!(*"##" != p);
}

#[test]
fn punct_partial_ord_str() {
  let p = TestPunct::unit();
  assert_eq!(p.partial_cmp("@@"), Some(std::cmp::Ordering::Equal));
}

#[test]
fn str_partial_ord_punct() {
  let p = TestPunct::unit();
  assert_eq!("@@".partial_cmp(&p), Some(std::cmp::Ordering::Equal));
}

#[test]
fn punct_as_span() {
  let span = SimpleSpan::new(10, 20);
  let p = TestPunct::<SimpleSpan>::new(span);
  assert_eq!(*AsSpan::as_span(&p), span);
}

#[test]
fn punct_into_span() {
  let span = SimpleSpan::new(10, 20);
  let p = TestPunct::<SimpleSpan>::new(span);
  let s: SimpleSpan = IntoSpan::into_span(p);
  assert_eq!(s, span);
}

#[test]
fn punct_into_components() {
  let span = SimpleSpan::new(1, 3);
  let p = TestPunct::<SimpleSpan, i32>::with_content(span, 99);
  let (s, c) = p.into_components();
  assert_eq!(s, span);
  assert_eq!(c, 99);
}

#[test]
fn punct_change_language() {
  struct LangA;
  struct LangB;
  let p: TestPunct<(), (), LangA> = TestPunct::new(()).change_language();
  let p2: TestPunct<(), (), LangB> = p.change_language();
  assert_eq!(p2.as_str(), "@@");
}

#[test]
fn punct_change_language_const() {
  struct LangA;
  struct LangB;
  let p: TestPunct<(), (), LangA> = TestPunct::new(()).change_language_const();
  let p2: TestPunct<(), (), LangB> = p.change_language_const();
  assert_eq!(p2.as_str(), "@@");
}

#[test]
fn punct_display_human() {
  let p = TestPunct::unit();
  struct HumanWrapper<'a, T: DisplayHuman>(&'a T);
  impl<T: DisplayHuman> std::fmt::Display for HumanWrapper<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      DisplayHuman::fmt(self.0, f)
    }
  }
  let s = format!("{}", HumanWrapper(&p));
  assert_eq!(s, "@@");
}

#[test]
fn punct_display_compact() {
  let p = TestPunct::unit();
  struct CompactWrapper<'a, T: DisplayCompact<Options = ()>>(&'a T);
  impl<T: DisplayCompact<Options = ()>> std::fmt::Display for CompactWrapper<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      DisplayCompact::fmt(self.0, f, &())
    }
  }
  let s = format!("{}", CompactWrapper(&p));
  assert_eq!(s, "@@");
}

#[test]
fn punct_display_pretty() {
  let p = TestPunct::unit();
  struct PrettyWrapper<'a, T: DisplayPretty<Options = ()>>(&'a T);
  impl<T: DisplayPretty<Options = ()>> std::fmt::Display for PrettyWrapper<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      DisplayPretty::fmt(self.0, f, &())
    }
  }
  let s = format!("{}", PrettyWrapper(&p));
  assert_eq!(s, "@@");
}

#[test]
fn punct_another_variant() {
  let p = AnotherPunct::unit();
  assert_eq!(AnotherPunct::raw(), "##");
  assert_eq!(format!("{}", p), "##");
  let s: &str = p.as_ref();
  assert_eq!(s, "##");
  let b: &str = p.borrow();
  assert_eq!(b, "##");
}
