#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
mod common;

use common::E;
use tokora::EmitterView;

// Comprehensive tests for `sep_while/parse` code paths (non-delimited separated_while)
// with all separator policies crossed against count modifiers (at_least, at_most, bounded).
//
// # Sentinel token
//
// Every test input ends with `+` so the condition closure always sees a stop
// token instead of hitting the debug_assert at EOF.

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  parser::Action,
};

use common::{TestLexer, Token};

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

// ── Element parser (ParseInput) ───────────────────────────────────────────────

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  match inp.next()? {
    None => Err(E),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(E),
    },
  }
}

// ── Decision function ─────────────────────────────────────────────────────────

fn decide<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
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

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Plain separated_while (no policy)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 1a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_plain_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least)
    .parse_str("1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn plain_at_least_exact_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least)
    .parse_str("1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn plain_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least)
    .parse_str("1+");
  assert!(r.is_err());
}

#[test]
fn plain_at_least_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least)
    .parse_str("+");
  assert!(r.is_err());
}

// ── 1b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_plain_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .at_most(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most)
    .parse_str("1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn plain_at_most_exact_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most)
    .parse_str("1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn plain_at_most_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most)
    .parse_str("1+")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn plain_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most)
    .parse_str("1,2,3,4+");
  assert!(r.is_err());
}

// ── 1c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_plain_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded)
    .parse_str("1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn plain_bounded_exact_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded)
    .parse_str("1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn plain_bounded_exact_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded)
    .parse_str("1,2,3,4+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn plain_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded)
    .parse_str("1+");
  assert!(r.is_err());
}

#[test]
fn plain_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded)
    .parse_str("1,2,3,4,5+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. allow_leading
// ═══════════════════════════════════════════════════════════════════════════════

// ── 2a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_allow_leading_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_leading()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_leading_at_least_ok_with_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_at_least)
    .parse_str(",1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_leading_at_least_ok_without_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_at_least)
    .parse_str("1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_leading_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_at_least)
    .parse_str(",1+");
  assert!(r.is_err());
}

#[test]
fn allow_leading_at_least_too_few_no_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_at_least)
    .parse_str("1+");
  assert!(r.is_err());
}

// ── 2b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_allow_leading_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_leading()
    .at_most(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_leading_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_at_most)
    .parse_str(",1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_leading_at_most_exact_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_at_most)
    .parse_str(",1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_leading_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_at_most)
    .parse_str(",1,2,3,4+");
  assert!(r.is_err());
}

// ── 2c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_allow_leading_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_leading()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_leading_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_bounded)
    .parse_str(",1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_leading_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_bounded)
    .parse_str(",1+");
  assert!(r.is_err());
}

#[test]
fn allow_leading_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_bounded)
    .parse_str(",1,2,3,4,5+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. allow_trailing
// ═══════════════════════════════════════════════════════════════════════════════

// ── 3a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_allow_trailing_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_trailing_at_least_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_at_least)
    .parse_str("1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_trailing_at_least_ok_without_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_at_least)
    .parse_str("1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_trailing_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_at_least)
    .parse_str("1,+");
  assert!(r.is_err());
}

#[test]
fn allow_trailing_at_least_too_few_no_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_at_least)
    .parse_str("1+");
  assert!(r.is_err());
}

// ── 3b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_allow_trailing_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_at_most)
    .parse_str("1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_trailing_at_most_exact_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_at_most)
    .parse_str("1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_trailing_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_at_most)
    .parse_str("1,2,3,4,5+");
  assert!(r.is_err());
}

// ── 3c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_allow_trailing_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_trailing_bounded_ok_no_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_trailing_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_bounded)
    .parse_str("1+");
  assert!(r.is_err());
}

#[test]
fn allow_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,2,3,4,5,+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. allow_surrounded (allow_trailing + allow_leading)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 4a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_allow_surrounded_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_surrounded_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_at_least)
    .parse_str(",1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_surrounded_at_least_ok_no_surround() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_at_least)
    .parse_str("1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_surrounded_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_at_least)
    .parse_str(",1,+");
  assert!(r.is_err());
}

// ── 4b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_allow_surrounded_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_surrounded_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_at_most)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_surrounded_at_most_exact_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_at_most)
    .parse_str(",1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_surrounded_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_at_most)
    .parse_str(",1,2,3,4,+");
  assert!(r.is_err());
}

// ── 4c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_allow_surrounded_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_surrounded_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_bounded)
    .parse_str(",1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_surrounded_bounded_exact_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_bounded)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_surrounded_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_bounded)
    .parse_str(",1,+");
  assert!(r.is_err());
}

#[test]
fn allow_surrounded_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_surrounded_bounded)
    .parse_str(",1,2,3,4,5,+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. require_leading
// ═══════════════════════════════════════════════════════════════════════════════

// ── 5a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_require_leading_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_leading()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn require_leading_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_at_least)
    .parse_str(",1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_leading_at_least_ok_three() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_at_least)
    .parse_str(",1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_leading_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_at_least)
    .parse_str(",1+");
  assert!(r.is_err());
}

#[test]
fn require_leading_at_least_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_at_least)
    .parse_str("1,2+");
  assert!(r.is_err());
}

// ── 5b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_require_leading_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_leading()
    .at_most(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn require_leading_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_at_most)
    .parse_str(",1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_leading_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_at_most)
    .parse_str("1,2,3+");
  assert!(r.is_err());
}

#[test]
fn require_leading_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_at_most)
    .parse_str(",1,2,3,4+");
  assert!(r.is_err());
}

// ── 5c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_require_leading_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_leading()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn require_leading_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_bounded)
    .parse_str(",1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_leading_bounded_exact_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_bounded)
    .parse_str(",1,2,3,4+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn require_leading_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_bounded)
    .parse_str(",1+");
  assert!(r.is_err());
}

#[test]
fn require_leading_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_bounded)
    .parse_str(",1,2,3,4,5+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. require_trailing
// ═══════════════════════════════════════════════════════════════════════════════

// ── 6a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_require_trailing_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn require_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_at_least)
    .parse_str("1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_trailing_at_least_ok_three() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_at_least)
    .parse_str("1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_trailing_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_at_least)
    .parse_str("1,+");
  assert!(r.is_err());
}

#[test]
fn require_trailing_at_least_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_at_least)
    .parse_str("1,2+");
  assert!(r.is_err());
}

// ── 6b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_require_trailing_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .at_most(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn require_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_at_most)
    .parse_str("1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_trailing_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_at_most)
    .parse_str("1,2,3+");
  assert!(r.is_err());
}

#[test]
fn require_trailing_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_at_most)
    .parse_str("1,2,3,4,+");
  assert!(r.is_err());
}

// ── 6c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_require_trailing_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn require_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_bounded)
    .parse_str("1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_trailing_bounded_exact_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_bounded)
    .parse_str("1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_trailing_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_bounded)
    .parse_str("1,+");
  assert!(r.is_err());
}

#[test]
fn require_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_bounded)
    .parse_str("1,2,3,4,5,+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. require_surrounded (require_trailing + require_leading)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 7a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_require_surrounded_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .at_least(2)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn require_surrounded_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_at_least)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_surrounded_at_least_ok_three() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_at_least)
    .parse_str(",1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_surrounded_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_at_least)
    .parse_str(",1,+");
  assert!(r.is_err());
}

#[test]
fn require_surrounded_at_least_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_at_least)
    .parse_str("1,2,+");
  assert!(r.is_err());
}

// ── 7b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_require_surrounded_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .at_most(3)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn require_surrounded_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_at_most)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_surrounded_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_at_most)
    .parse_str("1,2,3,+");
  assert!(r.is_err());
}

#[test]
fn require_surrounded_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_at_most)
    .parse_str(",1,2,3,4,+");
  assert!(r.is_err());
}

// ── 7c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_require_surrounded_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .bounded(2, 4)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn require_surrounded_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_bounded)
    .parse_str(",1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_surrounded_bounded_exact_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_bounded)
    .parse_str(",1,2,3,4,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn require_surrounded_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_bounded)
    .parse_str(",1,+");
  assert!(r.is_err());
}

#[test]
fn require_surrounded_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_bounded)
    .parse_str(",1,2,3,4,5,+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. allow_leading_require_trailing (require_trailing + allow_leading)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 8a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_allow_leading_require_trailing_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .at_least(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_leading_require_trailing_at_least_ok_with_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_at_least)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_leading_require_trailing_at_least_ok_without_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_at_least)
    .parse_str("1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_leading_require_trailing_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_at_least)
    .parse_str(",1,+");
  assert!(r.is_err());
}

#[test]
fn allow_leading_require_trailing_at_least_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_at_least)
    .parse_str(",1,2+");
  assert!(r.is_err());
}

// ── 8b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_allow_leading_require_trailing_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .at_most(3)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_leading_require_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_at_most)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_leading_require_trailing_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_at_most)
    .parse_str("1,2,3+");
  assert!(r.is_err());
}

#[test]
fn allow_leading_require_trailing_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_at_most)
    .parse_str(",1,2,3,4,+");
  assert!(r.is_err());
}

// ── 8c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_allow_leading_require_trailing_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .require_trailing()
    .bounded(2, 4)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn allow_leading_require_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_bounded)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn allow_leading_require_trailing_bounded_ok_no_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_bounded)
    .parse_str("1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn allow_leading_require_trailing_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_bounded)
    .parse_str(",1,+");
  assert!(r.is_err());
}

#[test]
fn allow_leading_require_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_bounded)
    .parse_str(",1,2,3,4,5,+");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. require_leading_allow_trailing (allow_trailing + require_leading)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 9a. at_least(2) ──────────────────────────────────────────────────────────

fn parse_require_leading_allow_trailing_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn require_leading_allow_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_least)
    .parse_str(",1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_leading_allow_trailing_at_least_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_least)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_leading_allow_trailing_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_least)
    .parse_str(",1+");
  assert!(r.is_err());
}

#[test]
fn require_leading_allow_trailing_at_least_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_least)
    .parse_str("1,2+");
  assert!(r.is_err());
}

// ── 9b. at_most(3) ──────────────────────────────────────────────────────────

fn parse_require_leading_allow_trailing_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn require_leading_allow_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_most)
    .parse_str(",1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_leading_allow_trailing_at_most_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_most)
    .parse_str(",1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn require_leading_allow_trailing_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_most)
    .parse_str("1,2,3+");
  assert!(r.is_err());
}

#[test]
fn require_leading_allow_trailing_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_at_most)
    .parse_str(",1,2,3,4,+");
  assert!(r.is_err());
}

// ── 9c. bounded(2, 4) ───────────────────────────────────────────────────────

fn parse_require_leading_allow_trailing_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn require_leading_allow_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_bounded)
    .parse_str(",1,2,3+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_leading_allow_trailing_bounded_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_bounded)
    .parse_str(",1,2,3,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn require_leading_allow_trailing_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_bounded)
    .parse_str(",1+");
  assert!(r.is_err());
}

#[test]
fn require_leading_allow_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_bounded)
    .parse_str(",1,2,3,4,5+");
  assert!(r.is_err());
}

#[test]
fn require_leading_allow_trailing_bounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_bounded)
    .parse_str("1,2,3+");
  assert!(r.is_err());
}
