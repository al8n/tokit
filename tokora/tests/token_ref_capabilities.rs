//! The token capability family, censused over the two representations `Token` admits.
//!
//! `&'a T` is a first-class token representation: `impl<'a, T: Token<'a>> Token<'a> for &'a T`
//! is in the crate, so a downstream lexer or adapter may legally hand out references. This
//! suite is the compile-time census that every *capability* crosses that borrow too, and it is
//! built to discriminate rather than merely to go red:
//!
//! * one assertion helper per capability, so a missing forwarding impl names the capability it
//!   is missing for rather than a bundle;
//! * a `Token` control beside every specialized assertion, so a base-trait failure cannot be
//!   misread as a capability failure;
//! * runtime parity over the punctuation constructors, because every one of them defaults to
//!   `None` — a *partial* forwarding impl compiles and answers `None` for the arms it forgot,
//!   which no compile assertion can see;
//! * two `PrattToken` instantiations, because that trait is generic in `Expr` and `Power` and a
//!   forwarding impl that pinned either would still satisfy a single-instantiation assertion.

use core::fmt;

use tokora::{
  Token,
  token::{IdentifierToken, KeywordToken, LitToken, PunctuatorToken, PunctuatorTokenExt},
};

#[cfg(feature = "pratt")]
use tokora::{
  parser::{PrattLHS, PrattRHS},
  token::PrattToken,
};

// ── A downstream token, written only against the public API ──────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
  Ident,
  If,
  Int,
  OpenParen,
  Comma,
  Dot,
  Arrow,
  LogicalAnd,
  ShlEqual,
  Newline,
  Other,
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{self:?}")
  }
}

#[derive(Debug, Clone)]
struct Tok {
  kind: Kind,
}

impl Tok {
  const fn new(kind: Kind) -> Self {
    Self { kind }
  }
}

impl Token<'_> for Tok {
  type Kind = Kind;
  type Error = ();

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  fn kind(&self) -> Self::Kind {
    self.kind
  }

  fn is_trivia(&self) -> bool {
    matches!(self.kind, Kind::Newline)
  }
}

impl IdentifierToken<'_> for Tok {
  fn is_identifier(&self) -> bool {
    matches!(self.kind, Kind::Ident)
  }
}

impl KeywordToken<'_> for Tok {
  fn keyword(&self) -> Option<&'static str> {
    match self.kind {
      Kind::If => Some("if"),
      _ => None,
    }
  }
}

impl LitToken<'_> for Tok {
  fn is_decimal_literal(&self) -> bool {
    matches!(self.kind, Kind::Int)
  }
}

// One override per declaration group of the punctuation macro — delimiters, ASCII
// punctuation, multi-character, equal-related, shift-assignment and trivia — so a forwarding
// impl generated from a truncated arm list is caught wherever the truncation falls.
impl PunctuatorToken<'_> for Tok {
  fn open_paren() -> Option<Self::Kind> {
    Some(Kind::OpenParen)
  }

  fn comma() -> Option<Self::Kind> {
    Some(Kind::Comma)
  }

  fn dot() -> Option<Self::Kind> {
    Some(Kind::Dot)
  }

  fn arrow() -> Option<Self::Kind> {
    Some(Kind::Arrow)
  }

  fn logical_and() -> Option<Self::Kind> {
    Some(Kind::LogicalAnd)
  }

  fn shl_equal() -> Option<Self::Kind> {
    Some(Kind::ShlEqual)
  }

  fn newline() -> Option<Self::Kind> {
    Some(Kind::Newline)
  }
}

// Two instantiations that differ in *both* type parameters, and disagree on the same token, so
// neither can stand in for the other.
#[cfg(feature = "pratt")]
impl PrattToken<'_, i64, i64> for Tok {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), i64>> {
    match self.kind {
      Kind::Int => Some(PrattLHS::Operand(())),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), i64>> {
    match self.kind {
      Kind::Dot => Some(PrattRHS::Postfix(tokora::parser::Precedenced::new((), 7))),
      _ => None,
    }
  }
}

#[cfg(feature = "pratt")]
impl PrattToken<'_, str, i8> for Tok {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), i8>> {
    match self.kind {
      Kind::Ident => Some(PrattLHS::Operand(())),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), i8>> {
    match self.kind {
      Kind::Comma => Some(PrattRHS::Postfix(tokora::parser::Precedenced::new((), 3))),
      _ => None,
    }
  }
}

// ── Compile-time census: one helper per capability ───────────────────────────
//
// Each block asserts the base `Token` capability for `&'static Tok` beside the specialized
// one, so a failure of the borrow representation *as such* is distinguishable from a failure
// of the one capability under test.

const fn assert_token<'a, T: Token<'a>>() {}
const fn assert_identifier<'a, T: IdentifierToken<'a>>() {}
const fn assert_keyword<'a, T: KeywordToken<'a>>() {}
const fn assert_lit<'a, T: LitToken<'a>>() {}
const fn assert_punctuator<'a, T: PunctuatorToken<'a>>() {}
const fn assert_punctuator_ext<'a, T: PunctuatorTokenExt<'a>>() {}

#[cfg(feature = "pratt")]
const fn assert_pratt<'a, Expr: ?Sized, Power, T: PrattToken<'a, Expr, Power>>() {}

const _: () = {
  assert_token::<Tok>();
  assert_token::<&'static Tok>();
};

const _: () = {
  assert_token::<&'static Tok>();
  assert_identifier::<Tok>();
  assert_identifier::<&'static Tok>();
};

const _: () = {
  assert_token::<&'static Tok>();
  assert_keyword::<Tok>();
  assert_keyword::<&'static Tok>();
};

const _: () = {
  assert_token::<&'static Tok>();
  assert_lit::<Tok>();
  assert_lit::<&'static Tok>();
};

const _: () = {
  assert_token::<&'static Tok>();
  assert_punctuator::<Tok>();
  assert_punctuator::<&'static Tok>();
  assert_punctuator_ext::<Tok>();
  assert_punctuator_ext::<&'static Tok>();
};

#[cfg(feature = "pratt")]
const _: () = {
  assert_token::<&'static Tok>();
  assert_pratt::<i64, i64, Tok>();
  assert_pratt::<i64, i64, &'static Tok>();
};

// The second instantiation is a separate block: pinning `Power` in a forwarding impl leaves
// the block above green and reds only this one.
#[cfg(feature = "pratt")]
const _: () = {
  assert_token::<&'static Tok>();
  assert_pratt::<str, i8, Tok>();
  assert_pratt::<str, i8, &'static Tok>();
};

// ── Runtime discrimination ───────────────────────────────────────────────────
//
// `<&Tok as Trait>::method(&&tok)` forces dispatch through the `&'a T` impl rather than
// auto-derefing to `Tok`.

/// Every punctuation constructor is an associated function with a `None` default, so parity
/// with the referent is the only thing that separates a complete forwarding impl from one that
/// silently drops arms. The name in the message is the arm.
#[test]
fn ref_punctuator_constructors_match_the_referent() {
  macro_rules! parity {
    ($($ctor:ident),+ $(,)?) => {
      $(
        assert_eq!(
          <&Tok as PunctuatorToken>::$ctor(),
          <Tok as PunctuatorToken>::$ctor(),
          concat!("`", stringify!($ctor), "` does not forward across the borrow"),
        );
      )+
    };
  }

  // The seven the referent overrides…
  parity!(
    open_paren,
    comma,
    dot,
    arrow,
    logical_and,
    shl_equal,
    newline
  );
  // …and arms it does not, so "forwards" is not satisfied by a constant `None`.
  parity!(
    close_paren,
    semicolon,
    fat_arrow,
    logical_or,
    sar_equal,
    space
  );
}

#[test]
fn ref_punctuator_predicates_delegate() {
  let dot = Tok::new(Kind::Dot);
  assert!(<&Tok as PunctuatorTokenExt>::is_dot(&&dot));
  assert!(<&Tok as PunctuatorTokenExt>::is_punctuator(&&dot));

  let other = Tok::new(Kind::Other);
  assert!(!<&Tok as PunctuatorTokenExt>::is_dot(&&other));
  assert!(!<&Tok as PunctuatorTokenExt>::is_punctuator(&&other));
}

#[test]
fn ref_punctuator_aliases_delegate() {
  let arrow = Tok::new(Kind::Arrow);
  assert!(<&Tok as PunctuatorTokenExt>::is_thin_arrow(&&arrow));

  let shl = Tok::new(Kind::ShlEqual);
  assert!(<&Tok as PunctuatorTokenExt>::is_shl_assign(&&shl));
}

#[test]
fn ref_base_and_neighbour_capabilities_delegate() {
  let ident = Tok::new(Kind::Ident);
  assert_eq!(<&Tok as Token>::kind(&&ident), Kind::Ident);
  assert!(<&Tok as IdentifierToken>::is_identifier(&&ident));

  let kw = Tok::new(Kind::If);
  assert_eq!(<&Tok as KeywordToken>::keyword(&&kw), Some("if"));
  assert!(<&Tok as KeywordToken>::is_keyword(&&kw));

  let int = Tok::new(Kind::Int);
  assert!(<&Tok as LitToken>::is_decimal_literal(&&int));

  let nl = Tok::new(Kind::Newline);
  assert!(<&Tok as Token>::is_trivia(&&nl));
}

#[cfg(feature = "pratt")]
#[test]
fn ref_pratt_delegates_on_the_default_power() {
  let int = Tok::new(Kind::Int);
  assert!(matches!(
    <&Tok as PrattToken<i64, i64>>::try_pratt_lhs(&&int),
    Some(PrattLHS::Operand(()))
  ));
  assert!(<&Tok as PrattToken<i64, i64>>::try_pratt_rhs(&&int).is_none());

  let dot = Tok::new(Kind::Dot);
  assert!(<&Tok as PrattToken<i64, i64>>::try_pratt_lhs(&&dot).is_none());
  assert!(matches!(
    <&Tok as PrattToken<i64, i64>>::try_pratt_rhs(&&dot),
    Some(PrattRHS::Postfix(_))
  ));
}

/// The second instantiation classifies *different* tokens, so an impl that forwarded to the
/// wrong one would answer `None` here where the referent answers `Some`.
#[cfg(feature = "pratt")]
#[test]
fn ref_pratt_delegates_on_a_non_default_expr_and_power() {
  let ident = Tok::new(Kind::Ident);
  assert!(matches!(
    <&Tok as PrattToken<str, i8>>::try_pratt_lhs(&&ident),
    Some(PrattLHS::Operand(()))
  ));

  let comma = Tok::new(Kind::Comma);
  assert!(matches!(
    <&Tok as PrattToken<str, i8>>::try_pratt_rhs(&&comma),
    Some(PrattRHS::Postfix(_))
  ));
  assert!(<&Tok as PrattToken<str, i8>>::try_pratt_rhs(&&ident).is_none());
}
