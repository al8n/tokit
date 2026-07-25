#![cfg(all(feature = "std", feature = "logos"))]

//! Regression tests for separator position carried as **data** (a
//! [`SeparatorPosition`] field on [`SeparatedError`]) rather than encoded in the
//! `Lang` type slot of `UnexpectedToken`.
//!
//! The headline test [`downstream_distinguishes_by_position`] builds a
//! downstream-style error enum from **only** core `From` impls — no
//! hand-written `FromSeparatedError` / `FromUnexpected{Leading,Trailing}SeparatorError`
//! impls — and receives leading / trailing / element separator errors
//! distinguished purely by the position field. This is the shape that was
//! impossible before the blanket `From` impls were restored.

mod common;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  cache::Peeked,
  emitter::{
    FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, SeparatorPosition, UnexpectedToken},
  },
  parser::Action,
};

use common::{TestLexer, Token};

// ── Downstream-style error enum — CORE `From` impls only ──────────────────────
//
// Note the absence of any `impl FromSeparatedError`, `impl
// FromUnexpectedLeadingSeparatorError`, or `impl
// FromUnexpectedTrailingSeparatorError`. Those are supplied by the restored
// blanket impls; the leading/trailing position rides in on `SeparatedError` and
// the element position on `MissingSyntax`.

#[derive(Debug, Clone, PartialEq, Eq)]
enum SepErr {
  /// A separator error, tagged with where it occurred.
  Sep(SeparatorPosition),
  /// Anything else (lexer error, plain unexpected token, missing separator...).
  Other,
}

impl From<()> for SepErr {
  fn from(_: ()) -> Self {
    SepErr::Other
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for SepErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    SepErr::Other
  }
}

// Leading / trailing separator errors arrive here — distinguished by position.
impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for SepErr {
  fn from(err: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    SepErr::Sep(err.position())
  }
}

// A missing element mid-list is the element position.
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for SepErr {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    SepErr::Sep(SeparatorPosition::Element)
  }
}

// A terminal scanner stop at the separator slot surfaces as this end-of-input error — not a
// separator-position diagnostic.
impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for SepErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    SepErr::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for SepErr {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    SepErr::Other
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for SepErr {
  fn from(_: FullContainer<S, Lang>) -> Self {
    SepErr::Other
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for SepErr {
  fn from(_: TooFew<S, Lang>) -> Self {
    SepErr::Other
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for SepErr {
  fn from(_: TooMany<S, Lang>) -> Self {
    SepErr::Other
  }
}

// ── Parser harness ────────────────────────────────────────────────────────────

fn decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: &mut Ctx::Emitter,
) -> Result<Action, <Ctx::Emitter as Emitter<'inp, TestLexer<'inp>>>::Error>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
{
  Ok(match peeked.pop_front() {
    None => Action::Stop,
    Some(tok) => {
      let tok = tok
        .as_maybe_ref()
        .map(|t| t.token().copied(), |t| t.token())
        .into_inner();
      if matches!(**tok.data(), Token::Num(_)) {
        Action::Continue
      } else {
        Action::Stop
      }
    }
  })
}

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>,
{
  match inp.next()? {
    None => Err(SepErr::Other),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(SepErr::Other),
    },
  }
}

fn parse_allow_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .collect()
    .parse_input(inp)
}

fn parse_allow_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

fn parse_plain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .collect()
    .parse_input(inp)
}

// ── The previously-impossible test ────────────────────────────────────────────

#[test]
fn downstream_distinguishes_by_position() {
  // Leading separator under allow_trailing → position = Leading.
  let leading: Result<Vec<i64>, SepErr> =
    Parser::new().apply(parse_allow_trailing).parse_str(",1+");
  assert_eq!(leading, Err(SepErr::Sep(SeparatorPosition::Leading)));

  // Trailing separator under allow_leading → position = Trailing.
  let trailing: Result<Vec<i64>, SepErr> =
    Parser::new().apply(parse_allow_leading).parse_str("1,+");
  assert_eq!(trailing, Err(SepErr::Sep(SeparatorPosition::Trailing)));

  // Consecutive separators mid-list → missing element → position = Element.
  let element: Result<Vec<i64>, SepErr> = Parser::new().apply(parse_plain).parse_str("1,,2+");
  assert_eq!(element, Err(SepErr::Sep(SeparatorPosition::Element)));
}

// ── Unit coverage of the new data types ───────────────────────────────────────

#[test]
fn separator_position_as_str_and_predicates() {
  assert_eq!(SeparatorPosition::Element.as_str(), "element");
  assert_eq!(SeparatorPosition::Leading.as_str(), "leading");
  assert_eq!(SeparatorPosition::Trailing.as_str(), "trailing");

  assert!(SeparatorPosition::Element.is_element());
  assert!(SeparatorPosition::Leading.is_leading());
  assert!(SeparatorPosition::Trailing.is_trailing());
  assert!(!SeparatorPosition::Element.is_leading());

  assert_eq!(SeparatorPosition::Trailing.to_string(), "trailing");
}

#[test]
fn separated_error_constructors_and_accessors() {
  let ut: UnexpectedToken<'_, &str, &str, tokora::SimpleSpan> =
    UnexpectedToken::expected_one_with_found(tokora::SimpleSpan::new(1, 2), ",", ";");

  let leading = SeparatedError::leading(ut.clone());
  assert_eq!(leading.position(), SeparatorPosition::Leading);
  assert_eq!(leading.inner_ref().found(), Some(&","));

  let trailing = SeparatedError::trailing(ut.clone());
  assert_eq!(trailing.position(), SeparatorPosition::Trailing);

  let element = SeparatedError::element(ut.clone());
  assert_eq!(element.position(), SeparatorPosition::Element);

  let explicit = SeparatedError::new(SeparatorPosition::Leading, ut.clone());
  let (pos, inner) = explicit.into_components();
  assert_eq!(pos, SeparatorPosition::Leading);
  assert_eq!(inner.found(), Some(&","));
}
