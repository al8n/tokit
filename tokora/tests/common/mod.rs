#![cfg(all(feature = "std", feature = "logos"))]

/// Shared test infrastructure: lexer, token types, and trait impls.
///
/// All integration tests import this module via `mod common;`.
use tokora::{
  Token as TokenT,
  logos::{self, Logos},
  punct::{
    CloseAngle, CloseBrace, CloseBracket, CloseParen, Comma, OpenAngle, OpenBrace, OpenBracket,
    OpenParen, Semicolon,
  },
  token::PunctuatorToken,
};

// ── Token ─────────────────────────────────────────────────────────────────────

/// Test token with logos, default error type `()`.\
#[derive(Debug, Clone, Logos, PartialEq)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
pub enum Token {
  #[regex(r"-?[0-9]+", |lex| lex.slice().parse::<i64>().unwrap_or(0))]
  Num(i64),
  #[token(",")]
  Comma,
  #[token(";")]
  Semi,
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
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
  #[token("<")]
  LAngle,
  #[token(">")]
  RAngle,
  #[token("=")]
  Eq,
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
  Ident,
}

// ── TokenKind ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
  Num,
  Comma,
  Semi,
  Plus,
  Minus,
  Star,
  Slash,
  LParen,
  RParen,
  LBracket,
  RBracket,
  LBrace,
  RBrace,
  LAngle,
  RAngle,
  Eq,
  Ident,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokenKind::Num => write!(f, "number"),
      TokenKind::Comma => write!(f, ","),
      TokenKind::Semi => write!(f, ";"),
      TokenKind::Plus => write!(f, "+"),
      TokenKind::Minus => write!(f, "-"),
      TokenKind::Star => write!(f, "*"),
      TokenKind::Slash => write!(f, "/"),
      TokenKind::LParen => write!(f, "("),
      TokenKind::RParen => write!(f, ")"),
      TokenKind::LBracket => write!(f, "["),
      TokenKind::RBracket => write!(f, "]"),
      TokenKind::LBrace => write!(f, "{{"),
      TokenKind::RBrace => write!(f, "}}"),
      TokenKind::LAngle => write!(f, "<"),
      TokenKind::RAngle => write!(f, ">"),
      TokenKind::Eq => write!(f, "="),
      TokenKind::Ident => write!(f, "identifier"),
    }
  }
}

impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Token::Num(n) => write!(f, "{n}"),
      Token::Comma => write!(f, ","),
      Token::Semi => write!(f, ";"),
      Token::Plus => write!(f, "+"),
      Token::Minus => write!(f, "-"),
      Token::Star => write!(f, "*"),
      Token::Slash => write!(f, "/"),
      Token::LParen => write!(f, "("),
      Token::RParen => write!(f, ")"),
      Token::LBracket => write!(f, "["),
      Token::RBracket => write!(f, "]"),
      Token::LBrace => write!(f, "{{"),
      Token::RBrace => write!(f, "}}"),
      Token::LAngle => write!(f, "<"),
      Token::RAngle => write!(f, ">"),
      Token::Eq => write!(f, "="),
      Token::Ident => write!(f, "identifier"),
    }
  }
}

impl From<&Token> for TokenKind {
  fn from(t: &Token) -> Self {
    match t {
      Token::Num(_) => TokenKind::Num,
      Token::Comma => TokenKind::Comma,
      Token::Semi => TokenKind::Semi,
      Token::Plus => TokenKind::Plus,
      Token::Minus => TokenKind::Minus,
      Token::Star => TokenKind::Star,
      Token::Slash => TokenKind::Slash,
      Token::LParen => TokenKind::LParen,
      Token::RParen => TokenKind::RParen,
      Token::LBracket => TokenKind::LBracket,
      Token::RBracket => TokenKind::RBracket,
      Token::LBrace => TokenKind::LBrace,
      Token::RBrace => TokenKind::RBrace,
      Token::LAngle => TokenKind::LAngle,
      Token::RAngle => TokenKind::RAngle,
      Token::Eq => TokenKind::Eq,
      Token::Ident => TokenKind::Ident,
    }
  }
}

// ── Token<'_> trait ───────────────────────────────────────────────────────────

impl TokenT<'_> for Token {
  type Kind = TokenKind;
  type Error = ();

  fn kind(&self) -> TokenKind {
    TokenKind::from(self)
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

// ── PunctuatorToken<'_> ───────────────────────────────────────────────────────

impl PunctuatorToken<'_> for Token {
  fn comma() -> Option<Self::Kind> {
    Some(TokenKind::Comma)
  }

  fn semicolon() -> Option<Self::Kind> {
    Some(TokenKind::Semi)
  }

  fn open_paren() -> Option<Self::Kind> {
    Some(TokenKind::LParen)
  }

  fn close_paren() -> Option<Self::Kind> {
    Some(TokenKind::RParen)
  }

  fn open_bracket() -> Option<Self::Kind> {
    Some(TokenKind::LBracket)
  }

  fn close_bracket() -> Option<Self::Kind> {
    Some(TokenKind::RBracket)
  }

  fn open_brace() -> Option<Self::Kind> {
    Some(TokenKind::LBrace)
  }

  fn close_brace() -> Option<Self::Kind> {
    Some(TokenKind::RBrace)
  }

  fn open_angle() -> Option<Self::Kind> {
    Some(TokenKind::LAngle)
  }

  fn close_angle() -> Option<Self::Kind> {
    Some(TokenKind::RAngle)
  }

  fn minus() -> Option<Self::Kind> {
    Some(TokenKind::Minus)
  }

  fn asterisk() -> Option<Self::Kind> {
    Some(TokenKind::Star)
  }

  fn plus() -> Option<Self::Kind> {
    Some(TokenKind::Plus)
  }

  fn slash() -> Option<Self::Kind> {
    Some(TokenKind::Slash)
  }

  fn equal() -> Option<Self::Kind> {
    Some(TokenKind::Eq)
  }
}

// ── From<Punct> for TokenKind ─────────────────────────────────────────────────
//
// Required by `Punctuator<'inp, L>` impl in parser/punct.rs which needs
// `<L::Token as Token<'inp>>::Kind: From<$name<(), (), ()>>`.

impl From<Comma<(), (), ()>> for TokenKind {
  fn from(_: Comma<(), (), ()>) -> Self {
    TokenKind::Comma
  }
}

impl From<Semicolon<(), (), ()>> for TokenKind {
  fn from(_: Semicolon<(), (), ()>) -> Self {
    TokenKind::Semi
  }
}

impl From<OpenBracket<(), (), ()>> for TokenKind {
  fn from(_: OpenBracket<(), (), ()>) -> Self {
    TokenKind::LBracket
  }
}

impl From<CloseBracket<(), (), ()>> for TokenKind {
  fn from(_: CloseBracket<(), (), ()>) -> Self {
    TokenKind::RBracket
  }
}

impl From<OpenBrace<(), (), ()>> for TokenKind {
  fn from(_: OpenBrace<(), (), ()>) -> Self {
    TokenKind::LBrace
  }
}

impl From<CloseBrace<(), (), ()>> for TokenKind {
  fn from(_: CloseBrace<(), (), ()>) -> Self {
    TokenKind::RBrace
  }
}

impl From<OpenParen<(), (), ()>> for TokenKind {
  fn from(_: OpenParen<(), (), ()>) -> Self {
    TokenKind::LParen
  }
}

impl From<CloseParen<(), (), ()>> for TokenKind {
  fn from(_: CloseParen<(), (), ()>) -> Self {
    TokenKind::RParen
  }
}

impl From<OpenAngle<(), (), ()>> for TokenKind {
  fn from(_: OpenAngle<(), (), ()>) -> Self {
    TokenKind::LAngle
  }
}

impl From<CloseAngle<(), (), ()>> for TokenKind {
  fn from(_: CloseAngle<(), (), ()>) -> Self {
    TokenKind::RAngle
  }
}

// ── TestLexer ─────────────────────────────────────────────────────────────────

pub type TestLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Shared fixture error type `E` ────────────────────────────────────────────
//
// The unit "any failure" error the separated/handler/state-machine suites share:
// one `From` impl per error family their drivers can surface. Moved here from 14
// integration files that declared byte-identical copies (W7 harness consolidation);
// not every integration binary constructs it, hence the `allow(dead_code)`.

use tokora::error::{
  Unclosed, UnexpectedEot,
  syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
  token::{MissingToken, SeparatedError, UnexpectedToken},
};

#[derive(Debug)]
#[allow(dead_code)]
pub struct E;

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

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for E {
  fn from(_: FullContainer<S, Lang>) -> Self {
    E
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for E {
  fn from(_: TooFew<S, Lang>) -> Self {
    E
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for E {
  fn from(_: TooMany<S, Lang>) -> Self {
    E
  }
}

impl<Set: Clone + 'static> From<UnexpectedEot<usize, (), Set>> for E {
  fn from(_: UnexpectedEot<usize, (), Set>) -> Self {
    E
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for E {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    E
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for E {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    E
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for E {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    E
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unclosed<Delimiter, S, Lang>> for E {
  fn from(_: Unclosed<Delimiter, S, Lang>) -> Self {
    E
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for E
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<Delimiter>(_: Unclosed<Delimiter, L::Span, Lang>) -> Self {
    E
  }
}

// ── Shared emitter `TestEm` + its context constructor ─────────────────────────
//
// A "fail on everything" emitter (`Error = E`) with the Fatal-free context built
// from it. Byte-identical copies previously lived in the punct and input_ref
// suites (W7 harness consolidation); not every binary that imports common
// exercises them, hence the `allow(dead_code)`.

use tokora::{
  Emitter, Lexer, ParserContext, error::token::UnexpectedTokenOf, input::Cursor, span::Spanned,
};

#[allow(dead_code)]
pub struct TestEm;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for TestEm {
  type Error = E;
  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenT<'inp>>::Error,
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

#[allow(dead_code)]
pub fn ctx() -> ParserContext<'static, TestLexer<'static>, TestEm> {
  ParserContext::new(TestEm)
}

// ── Pratt binding-power newtype `Power` ──────────────────────────────────────
//
// A user newtype implementing `PrattPower` (the orphan rule forbids
// `impl PrattPower for i32`), shared by the pratt and misc suites to pin the
// user-newtype path distinct from the built-in integer impls. The field is
// public so each suite's local `PREC_*` constants can construct it; not every
// binary that imports common uses it, hence `allow(dead_code)`.

use tokora::parser::PrattPower;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub struct Power(pub i32);

impl PrattPower for Power {
  fn next(&self) -> Self {
    Power(self.0 + 1)
  }
  fn prev(&self) -> Self {
    Power(self.0 - 1)
  }
}
