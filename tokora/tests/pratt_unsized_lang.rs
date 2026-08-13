#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! tokora#276: the typed Pratt driver instantiated at a concrete **unsized** `Lang` brand.
//!
//! `pratt/`'s public constructor, [`pratt`](tokora::parser::pratt), was the one production in
//! the crate whose own generic parameter list forced `Lang: Sized` while every other
//! generalised production — 1,030 sites, including `Dialect::Lang: ?Sized` itself — accepts
//! `?Sized`. The bound was never load-bearing: the driver's only `Lang`-shaped field is
//! `PhantomData<Lang>` (`parser/pratt/expr.rs`), which needs no size; every trait the function
//! depends on ([`ParsePrattLHS`], [`ParsePrattRHS`], [`ParseContext`]) already spells `Lang:
//! ?Sized`. Relaxing `pratt`'s own list to match is therefore a four-character diff.
//!
//! A signature that says `?Sized` and is never instantiated at an unsized type is a claim, not
//! a capability. This file is the capability: a grammar branded `str` — the plainest concrete
//! unsized type the surrounding vocabulary admits, since `Lang` is never held as a value
//! anywhere in this driver, only phantom-carried — driven through the public `Parser`/`Parse`
//! API end to end, parsing real input and folding real operators.

mod common;

use tokora::{
  Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser,
  emitter::FromUnclosed,
  error::{
    NonAssociativeChain, RecursionLimitReached, Unclosed, UnexpectedEnd, token::UnexpectedToken,
  },
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
};

use common::{Power, TestLexer, Token};

// ── The unsized brand ────────────────────────────────────────────────────────────

/// The language brand this suite drives the driver at.
///
/// `Lang` is documented as a marker that is never a value (`Dialect::Lang`'s own words) — every
/// site in `pratt/` only ever holds it behind `PhantomData`, so nothing about it needs a size.
/// `str` is the plainest witness: no supporting trait impls to write, and it is exactly the
/// kind of type `?Sized` exists to admit (`Dialect::Lang`'s doc names "an extern type or a
/// trait object"; `str` is the unsized type already in scope everywhere else in this crate).
type UnsizedLang = str;

// ── A grammar-author error type, generalised over `Lang: ?Sized` ──────────────────
//
// The same recipe `tests/common::E` and `tests/parser_pratt.rs::PrattError` already use for
// most error families in this crate — `Lang: ?Sized` on the impl, not a concrete `()` —
// completed here for the two pratt-specific conversions the typed driver's `ParseInput` impl
// requires directly (`RecursionLimitReached`, `NonAssociativeChain`) and for the end-of-input
// family (`UnexpectedEnd<Hint, ..>` is generic over `Hint`, so one impl absorbs `UnexpectedEot`
// and the pratt-specific `UnexpectedEoLhs`/`UnexpectedEoRhs` aliases at once).

#[derive(Debug)]
struct PrattError;

impl From<()> for PrattError {
  fn from(_: ()) -> Self {
    PrattError
  }
}

impl<'inp, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'inp, T, Kind, S, Lang>>
  for PrattError
{
  fn from(_: UnexpectedToken<'inp, T, Kind, S, Lang>) -> Self {
    PrattError
  }
}

impl<Hint, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<Hint, O, Lang, Set>>
  for PrattError
{
  fn from(_: UnexpectedEnd<Hint, O, Lang, Set>) -> Self {
    PrattError
  }
}

impl<O, Lang: ?Sized> From<RecursionLimitReached<O, Lang>> for PrattError {
  fn from(_: RecursionLimitReached<O, Lang>) -> Self {
    PrattError
  }
}

impl<O, Lang: ?Sized> From<NonAssociativeChain<O, Lang>> for PrattError {
  fn from(_: NonAssociativeChain<O, Lang>) -> Self {
    PrattError
  }
}

impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for PrattError
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    PrattError
  }
}

// ── The grammar: a tiny calculator, branded `UnsizedLang` throughout ──────────────
//
// Same shape as `tests/parser_pratt.rs`'s `comb_*` family (the proven-correct combinator-API
// grammar), retyped so every `InputRef` names `UnsizedLang` instead of the default `()`. Any
// test failure here is therefore a fact about instantiating the driver at an unsized `Lang`,
// not about a freshly invented grammar.

const PREC_SUM: Power = Power(1); //  + -
const PREC_PROD: Power = Power(2); // * /
const PREC_NEG: Power = Power(3); //  unary -

#[derive(Debug, Clone, Copy)]
enum BinOp {
  Add,
  Sub,
  Mul,
  Div,
}

/// LHS parser: numbers, unary minus, and grouped `(expr)`.
fn lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, UnsizedLang>,
) -> Result<PrattLHS<i64, (), Power>, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, UnsizedLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, UnsizedLang, Error = PrattError>,
{
  match inp.next()? {
    None => Err(PrattError),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new((), PREC_NEG))),
      Token::LParen => {
        let e = expr(inp)?;
        if inp
          .try_expect(|t| matches!(t.data(), Token::RParen))?
          .is_none()
        {
          return Err(PrattError);
        }
        Ok(PrattLHS::Operand(e))
      }
      _ => Err(PrattError),
    },
  }
}

/// RHS parser: binary operators; anything else (including `)`, left for the LParen handler
/// above) ends the expression.
fn rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, UnsizedLang>,
) -> Result<PrattRHS<BinOp, BinOp, BinOp, (), Power>, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, UnsizedLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, UnsizedLang, Error = PrattError>,
{
  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Add),
        PREC_SUM,
      ))),
      Token::Minus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Sub),
        PREC_SUM,
      ))),
      Token::Star => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Mul),
        PREC_PROD,
      ))),
      Token::Slash => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Div),
        PREC_PROD,
      ))),
      _ => Ok(PrattRHS::End),
    },
  }
}

fn fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, UnsizedLang>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, UnsizedLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, UnsizedLang, Error = PrattError>,
{
  Ok(-operand)
}

fn fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, UnsizedLang>,
  left: i64,
  right: i64,
  op: Precedenced<PrattInfix<BinOp, BinOp, BinOp>, Power>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, UnsizedLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, UnsizedLang, Error = PrattError>,
{
  let bin_op = match op.into_data() {
    PrattInfix::Left(o) | PrattInfix::Right(o) | PrattInfix::Neither(o) => o,
  };
  Ok(match bin_op {
    BinOp::Add => left + right,
    BinOp::Sub => left - right,
    BinOp::Mul => left * right,
    BinOp::Div => left / right,
  })
}

fn fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, UnsizedLang>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, UnsizedLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, UnsizedLang, Error = PrattError>,
{
  Ok(operand) // this grammar declares no postfix operator, so nothing folds here
}

/// Entry point: the `pratt` combinator constructor, instantiated with `Lang = str` — the
/// concrete unsized brand — via every `InputRef` parameter above.
fn expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, UnsizedLang>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, UnsizedLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, UnsizedLang, Error = PrattError>,
{
  pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
}

// ── The capability, exercised ──────────────────────────────────────────────────

#[test]
fn pratt_parses_precedence_at_an_unsized_lang() {
  // 2 + 3 * 4 = 14 (not 20); `*` must still bind tighter than `+` with `Lang` unsized.
  let r: i64 = Parser::new().apply(expr).parse_str("2 + 3 * 4").unwrap();
  assert_eq!(r, 14);
}

#[test]
fn pratt_parses_parens_at_an_unsized_lang() {
  // (2 + 3) * 4 = 20; exercises the LHS grouping recursion (a nested `expr` call at the same
  // unsized `Lang`) and the driver's own expression-scoped guard nesting one level deep.
  let r: i64 = Parser::new().apply(expr).parse_str("(2 + 3) * 4").unwrap();
  assert_eq!(r, 20);
}

#[test]
fn pratt_parses_unary_minus_at_an_unsized_lang() {
  // -7; exercises the Prefix LHS channel and `fold_prefix`.
  let r: i64 = Parser::new().apply(expr).parse_str("-7").unwrap();
  assert_eq!(r, -7);
}

#[test]
fn pratt_parses_left_associativity_at_an_unsized_lang() {
  // 10 - 3 - 2 = 5, not 9; left-associative fold order with `Lang` unsized.
  let r: i64 = Parser::new().apply(expr).parse_str("10 - 3 - 2").unwrap();
  assert_eq!(r, 5);
}
