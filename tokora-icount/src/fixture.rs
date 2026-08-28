//! The one grammar every workload is built on: integers, commas, semicolons, parentheses.
//!
//! It is deliberately the smallest thing that can drive all nine repetition families. Every
//! token is one or two bytes and the lexer skips whitespace rather than tokenizing it, because
//! anything the lexer does is work the repetition engines did not do — see `workloads.rs`'s
//! `scan_drain` control, which measures exactly that share.

use tokora::{
  Emitter, InputRef, ParseContext, ScanLookahead, Token,
  error::{
    MaybeIncomplete, MaybeTerminal, UnexpectedEnd,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  lexer::LogosLexer,
  logos::{self, Logos},
  punct::{CloseParen, Comma, OpenParen},
  try_parse_input::ParseAttempt,
};

/// Dense discriminants, so a kind match beside a dispatch table compiles to a jump table.
#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
pub enum Tok {
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
  Int(i64),
  #[token(",")]
  Comma,
  #[token(";")]
  Semi,
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
  Int,
  Comma,
  Semi,
  LParen,
  RParen,
}

impl core::fmt::Display for Kind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Kind::Int => "integer",
      Kind::Comma => "','",
      Kind::Semi => "';'",
      Kind::LParen => "'('",
      Kind::RParen => "')'",
    })
  }
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Tok::Int(n) => write!(f, "{n}"),
      other => core::fmt::Display::fmt(&other.kind(), f),
    }
  }
}

impl Token<'_> for Tok {
  type Kind = Kind;
  type Error = ();

  const SCAN_LOOKAHEAD: ScanLookahead = ScanLookahead::Unbounded;

  fn kind(&self) -> Kind {
    match self {
      Tok::Int(_) => Kind::Int,
      Tok::Comma => Kind::Comma,
      Tok::Semi => Kind::Semi,
      Tok::LParen => Kind::LParen,
      Tok::RParen => Kind::RParen,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// The three wired punctuators. `comma` is what the `separated_by_comma` and
/// `separated_by_comma_while` drivers look the separator up through; the paren pair is what
/// `Separated::delimited_by_parens` looks the `Paren` delimiter up through. Each needs BOTH
/// halves of its wiring — the `Option<Kind>` here, which says the vocabulary word exists in
/// this language, and the `From` below, which is how the generic punctuator names the concrete
/// kind. A punctuator with only one half compiles until something asks for it.
impl tokora::token::PunctuatorToken<'_> for Tok {
  fn comma() -> Option<Kind> {
    Some(Kind::Comma)
  }

  fn open_paren() -> Option<Kind> {
    Some(Kind::LParen)
  }

  fn close_paren() -> Option<Kind> {
    Some(Kind::RParen)
  }
}

impl From<Comma<(), (), ()>> for Kind {
  fn from(_: Comma<(), (), ()>) -> Self {
    Kind::Comma
  }
}

impl From<OpenParen<(), (), ()>> for Kind {
  fn from(_: OpenParen<(), (), ()>) -> Self {
    Kind::LParen
  }
}

impl From<CloseParen<(), (), ()>> for Kind {
  fn from(_: CloseParen<(), (), ()>) -> Self {
    Kind::RParen
  }
}

pub type Lex<'a> = LogosLexer<'a, Tok>;

// ── The error absorber ──────────────────────────────────────────────────────────────────────
//
// Every source in `workloads.rs` is well-formed against the driver that reads it, so most of
// these `From`s are never constructed at runtime. They exist to satisfy the `FromEmitterError`
// bounds the repetition drivers require — and constructing none of them is what keeps the
// instruction count a measurement of the accepting path plus the deliberate bound trips, and
// not of diagnostic formatting.

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Err0;

impl From<()> for Err0 {
  fn from(_: ()) -> Self {
    Err0
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Err0 {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    Err0
  }
}

/// Covers `UnexpectedEot` and the end-of-input hints in one impl — they are all aliases of
/// `UnexpectedEnd` with different hint parameters.
impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for Err0 {
  fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self {
    Err0
  }
}

/// The delimiter half of the bundle: one generic impl for every pair, which is what the
/// `delim` arm of each family needs.
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Err0
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    Err0
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Err0 {
  fn from(_: FullContainer<S, Lang>) -> Self {
    Err0
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Err0 {
  fn from(_: TooFew<S, Lang>) -> Self {
    Err0
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Err0 {
  fn from(_: TooMany<S, Lang>) -> Self {
    Err0
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Err0 {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Err0
  }
}

impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Err0 {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    Err0
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Err0 {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    Err0
  }
}

impl MaybeIncomplete for Err0 {}
impl MaybeTerminal for Err0 {}

// ── Elements ────────────────────────────────────────────────────────────────────────────────

/// The try-shaped integer element the lookahead-owning families are built on: a non-integer
/// is put back and the element declines, which is how `repeated` and `separated` stop.
pub fn try_int<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>,
{
  Ok(match inp.try_expect(|t| matches!(t.data(), Tok::Int(_)))? {
    Some(tok) => match tok.into_data() {
      Tok::Int(n) => ParseAttempt::Accept(n),
      _ => unreachable!("the predicate admits only integers"),
    },
    None => ParseAttempt::Decline,
  })
}

/// The plain-shaped integer element the `*_while` families need: their element bound is
/// `ParseInput`, not `TryParseInput`, because the caller's `Decision` has already answered the
/// question a decline would have answered. A missing integer here is a hard failure.
pub fn int_elem<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<i64, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>,
{
  match inp.try_expect(|t| matches!(t.data(), Tok::Int(_)))? {
    Some(tok) => match tok.into_data() {
      Tok::Int(n) => Ok(n),
      _ => unreachable!("the predicate admits only integers"),
    },
    None => Err(Err0),
  }
}
