#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The inference-pin battery (T1–T12, at crate level).
//!
//! Candidate F's contract: every 0.2.0 spelling infers **byte-for-byte** at a concrete
//! `Complete` drive — no turbofish, no annotations — because every builder-returned
//! adapter carries the `Cmpl` parameter and the terminal drive pins it by unification.
//! Each test here is one probe shape; a failure is an inference regression in the
//! completeness threading, not a behavior bug.

mod common;

use common::{TestLexer, Token, TokenKind};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  parser::{Any, expect, parens},
  utils::Expected,
};

type L<'a> = TestLexer<'a>;

fn num_tok<'inp>(t: &Token) -> Result<(), Expected<'inp, TokenKind>> {
  if matches!(t, Token::Num(_)) {
    Ok(())
  } else {
    Err(Expected::one(TokenKind::Num))
  }
}

fn as_num(t: Token) -> i64 {
  match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  }
}

// T1 — THE ORACLE: a 0.2.0 chain at a concrete Complete drive, annotation-free.
#[test]
fn t1_oracle_chain_annotation_free() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    expect(num_tok).map(as_num).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("42"), Ok(42));
}

// T2 — two adapters deep.
#[test]
fn t2_two_deep() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    expect(num_tok).map(as_num).map(|n| n * 2).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("21"), Ok(42));
}

// T3 — a two-parser adapter: both children share one Cmpl.
#[test]
fn t3_then_chain() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    expect(num_tok)
      .map(as_num)
      .then(expect(num_tok).map(as_num))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("1 2"), Ok((1, 2)));
}

// T4 — split statements: bind the chain, then drive it.
#[test]
fn t4_split_statement() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    let mut p = expect(num_tok).map(as_num);
    p.parse_input(inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("7"), Ok(7));
}

// T7 — the single-bound generic wrap fn: one `ParseInput<…, Cmpl>` bound suffices to
// keep building.
#[test]
fn t7_generic_wrap_single_bound() {
  use tokora::input::{Complete, SurfaceIncomplete};

  fn wrap<'inp, Ctx, Cmpl, P>(
    p: P,
    inp: &mut InputRef<'inp, '_, L<'inp>, Ctx, (), Cmpl>,
  ) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
    Cmpl: SurfaceIncomplete<'inp, L<'inp>, Ctx, ()>,
    P: ParseInput<'inp, L<'inp>, i64, Ctx, (), Cmpl>,
  {
    p.map(|n| n + 1).parse_input(inp)
  }
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    let _complete_is_the_default: core::marker::PhantomData<Complete> = core::marker::PhantomData;
    wrap(expect(num_tok).map(as_num), inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("41"), Ok(42));
}

// T8 — a closure-rooted chain: the Cmpl-generic closure blanket at the root.
#[test]
fn t8_closure_rooted() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    (|inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>| expect(num_tok).map(as_num).parse_input(inp))
      .map(|n| n - 1)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("43"), Ok(42));
}

// T9 — the WRITTEN-type default position: a 0.2.0 spelling that names every adapter
// parameter except `Cmpl` still names the same type (the struct default fills it).
#[test]
#[allow(clippy::type_complexity)] // the point IS the fully-written 0.2.0 type spelling
fn t9_written_type_default_position() {
  use tokora::{FatalContext, parser::Map};
  fn step(t: Token) -> i64 {
    as_num(t)
  }
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, L<'inp>, FatalContext<'inp, L<'inp>, ()>>,
  ) -> Result<i64, ()> {
    let mut m: Map<
      Any<L<'inp>, FatalContext<'inp, L<'inp>, ()>>,
      fn(Token) -> i64,
      L<'inp>,
      FatalContext<'inp, L<'inp>, ()>,
      Token,
      i64,
    > = Any::of().map(step as fn(Token) -> i64);
    m.parse_input(inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("5"), Ok(5));
}

// T10 — an S-class (Complete-only) adapter atop a generic sub-chain: back-propagation
// flows through the pinned layer at a Complete drive.
#[test]
fn t10_s_class_atop_generic_subchain() {
  use generic_arraydeque::typenum::U1;
  use tokora::parser::Action;
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    expect(num_tok)
      .map(as_num)
      .peek_then::<_, U1>(|_peeked, _emitter| Ok(()))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(parse).parse_str("42"), Ok(42));
  let _ = Action::Continue;
}

// T11 — an opaque closure-returning constructor driven without any turbofish.
#[derive(Debug, PartialEq)]
enum PinErr {
  Other,
}
impl From<()> for PinErr {
  fn from(_: ()) -> Self {
    PinErr::Other
  }
}
impl<D, S, Lang: ?Sized> From<tokora::error::Unclosed<D, S, Lang>> for PinErr {
  fn from(_: tokora::error::Unclosed<D, S, Lang>) -> Self {
    PinErr::Other
  }
}
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for PinErr
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    PinErr::Other
  }
}
impl<'a, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, Token, TokenKind, S, Lang>>
  for PinErr
{
  fn from(_: tokora::error::token::UnexpectedToken<'a, Token, TokenKind, S, Lang>) -> Self {
    PinErr::Other
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>>
  for PinErr
{
  fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self {
    PinErr::Other
  }
}
impl<'a, S, Lang: ?Sized> From<tokora::error::token::SeparatedError<'a, Token, TokenKind, S, Lang>>
  for PinErr
{
  fn from(_: tokora::error::token::SeparatedError<'a, Token, TokenKind, S, Lang>) -> Self {
    PinErr::Other
  }
}
impl<'a, O, Lang: ?Sized> From<tokora::error::token::MissingToken<'a, TokenKind, O, Lang>>
  for PinErr
{
  fn from(_: tokora::error::token::MissingToken<'a, TokenKind, O, Lang>) -> Self {
    PinErr::Other
  }
}
impl<O, Lang: ?Sized> From<tokora::error::syntax::MissingSyntax<O, Lang>> for PinErr {
  fn from(_: tokora::error::syntax::MissingSyntax<O, Lang>) -> Self {
    PinErr::Other
  }
}
impl<S, Lang: ?Sized> From<tokora::error::syntax::FullContainer<S, Lang>> for PinErr {
  fn from(_: tokora::error::syntax::FullContainer<S, Lang>) -> Self {
    PinErr::Other
  }
}
impl<S, Lang: ?Sized> From<tokora::error::syntax::TooFew<S, Lang>> for PinErr {
  fn from(_: tokora::error::syntax::TooFew<S, Lang>) -> Self {
    PinErr::Other
  }
}
impl<S, Lang: ?Sized> From<tokora::error::syntax::TooMany<S, Lang>> for PinErr {
  fn from(_: tokora::error::syntax::TooMany<S, Lang>) -> Self {
    PinErr::Other
  }
}

#[test]
fn t11_delimited_constructor_no_turbofish() {
  // The corpus's delimited-drive shape: a CONCRETE context (the Fatal wiring implements
  // the shape's emitter families through the `From` impls above).
  type PinCtx<'a> = tokora::FatalContext<'a, L<'a>, PinErr>;
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, L<'inp>, PinCtx<'inp>>) -> Result<i64, PinErr> {
    fn digit<'inp>(inp: &mut InputRef<'inp, '_, L<'inp>, PinCtx<'inp>>) -> Result<i64, PinErr> {
      expect(num_tok).map(as_num).parse_input(inp)
    }
    parens(digit)(inp).map(|d| *d.data())
  }
  assert_eq!(Parser::with_parser(parse).parse_str("(7)"), Ok(7));
}

// T12 — `.by_ref()` reuse of one parser across two Complete drives.
#[test]
fn t12_by_ref_reuse() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, L<'inp>, Ctx>) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, L<'inp>>,
    Ctx::Emitter: Emitter<'inp, L<'inp>, Error = ()>,
  {
    let mut p = expect(num_tok).map(as_num);
    let a = p.by_ref().parse_input(inp)?;
    let b = p.by_ref().parse_input(inp)?;
    Ok((a, b))
  }
  assert_eq!(Parser::new().apply(parse).parse_str("3 4"), Ok((3, 4)));
}
