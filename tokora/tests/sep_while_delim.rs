#![cfg(all(feature = "std", feature = "logos"))]

//! Integration tests for `separated_by_comma_while` combined with
//! `.delimited::<Bracket<(), (), ()>>()` across all 8 separator policies
//! and 3 count modifiers (at_least, at_most, bounded).
//!
//! The closing bracket `]` triggers `Action::Stop` from the condition,
//! so no sentinel token is needed outside the brackets.

mod common;

use common::E;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, Verbose,
  },
  parser::Action,
  punct::Bracket,
};

use common::{TestLexer, Token};

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

fn verbose_ctx() -> ParserContext<'static, TestLexer<'static>, Verbose<E>> {
  ParserContext::new(Verbose::new())
}

// ── Condition: continue iff the next token is a number ────────────────────────

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

// ── Element parser ────────────────────────────────────────────────────────────

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

// ═══════════════════════════════════════════════════════════════════════════════
// 1. allow_leading
// ═══════════════════════════════════════════════════════════════════════════════

// ── 1a. allow_leading + at_least ──────────────────────────────────────────────

fn parse_al_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn al_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_al_at_least)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn al_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_al_at_least).parse_str("[,1]");
  assert!(r.is_err());
}

// ── 1b. allow_leading + at_most ───────────────────────────────────────────────

fn parse_al_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn al_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_al_at_most)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn al_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_al_at_most)
    .parse_str("[,1,2,3,4]");
  assert!(r.is_err());
}

// ── 1c. allow_leading + bounded ───────────────────────────────────────────────

fn parse_al_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .bounded(2, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn al_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_al_bounded)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn al_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_al_bounded).parse_str("[,1]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. allow_trailing
// ═══════════════════════════════════════════════════════════════════════════════

// ── 2a. allow_trailing + at_least ─────────────────────────────────────────────

fn parse_at_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn at_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_at_at_least)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn at_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_at_at_least).parse_str("[1,]");
  assert!(r.is_err());
}

// ── 2b. allow_trailing + at_most ──────────────────────────────────────────────

fn parse_at_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn at_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_at_at_most)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn at_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_at_at_most)
    .parse_str("[1,2,3,4,]");
  assert!(r.is_err());
}

// ── 2c. allow_trailing + bounded ──────────────────────────────────────────────

fn parse_at_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn at_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_at_bounded)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn at_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_at_bounded).parse_str("[1,]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. allow_surrounded
// ═══════════════════════════════════════════════════════════════════════════════

// ── 3a. allow_surrounded + at_least ───────────────────────────────────────────

fn parse_as_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn as_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_as_at_least)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn as_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_as_at_least).parse_str("[,1,]");
  assert!(r.is_err());
}

// ── 3b. allow_surrounded + at_most ────────────────────────────────────────────

fn parse_as_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn as_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_as_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn as_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_as_at_most)
    .parse_str("[,1,2,3,4,]");
  assert!(r.is_err());
}

// ── 3c. allow_surrounded + bounded ────────────────────────────────────────────

fn parse_as_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn as_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_as_bounded)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn as_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_as_bounded).parse_str("[,1,]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. require_leading
// ═══════════════════════════════════════════════════════════════════════════════

// ── 4a. require_leading + at_least ────────────────────────────────────────────

fn parse_rl_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_leading()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rl_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least)
    .parse_str("[,1]");
  assert!(r.is_err());
}

// ── 4b. require_leading + at_most ─────────────────────────────────────────────

fn parse_rl_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_leading()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rl_at_most_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most)
    .parse_str("[,1,2,3,4]");
  assert!(r.is_err());
}

// ── 4c. require_leading + bounded ─────────────────────────────────────────────

fn parse_rl_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_leading()
    .bounded(2, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rl_bounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded)
    .parse_str("[1,2,3]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. require_trailing
// ═══════════════════════════════════════════════════════════════════════════════

// ── 5a. require_trailing + at_least ───────────────────────────────────────────

fn parse_rt_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_least)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rt_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_least)
    .parse_str("[1,]");
  assert!(r.is_err());
}

// ── 5b. require_trailing + at_most ────────────────────────────────────────────

fn parse_rt_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rt_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most)
    .parse_str("[1,2]");
  assert!(r.is_err());
}

// ── 5c. require_trailing + bounded ────────────────────────────────────────────

fn parse_rt_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .bounded(2, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rt_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded)
    .parse_str("[1,]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. require_surrounded (require_trailing + require_leading)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 6a. require_surrounded + at_least ─────────────────────────────────────────

fn parse_rs_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .at_least(2)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rs_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ── 6b. require_surrounded + at_most ──────────────────────────────────────────

fn parse_rs_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .at_most(3)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rs_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most)
    .parse_str("[1,2,]");
  assert!(r.is_err());
}

// ── 6c. require_surrounded + bounded ──────────────────────────────────────────

fn parse_rs_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .bounded(2, 4)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rs_bounded_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded)
    .parse_str("[,1,2,3]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. allow_leading_require_trailing (require_trailing + allow_leading)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 7a. allow_leading_require_trailing + at_least ─────────────────────────────

fn parse_alrt_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .at_least(2)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_least)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn alrt_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_least)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ── 7b. allow_leading_require_trailing + at_most ──────────────────────────────

fn parse_alrt_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .at_most(3)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn alrt_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most)
    .parse_str("[,1,2]");
  assert!(r.is_err());
}

// ── 7c. allow_leading_require_trailing + bounded ──────────────────────────────

fn parse_alrt_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .bounded(2, 4)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn alrt_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. require_leading_allow_trailing (allow_trailing + require_leading)
// ═══════════════════════════════════════════════════════════════════════════════

// ── 8a. require_leading_allow_trailing + at_least ─────────────────────────────

fn parse_rlat_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_least)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rlat_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_least)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ── 8b. require_leading_allow_trailing + at_most ──────────────────────────────

fn parse_rlat_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rlat_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most)
    .parse_str("[1,2,]");
  assert!(r.is_err());
}

// ── 8c. require_leading_allow_trailing + bounded ──────────────────────────────

fn parse_rlat_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_bounded)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rlat_bounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_bounded)
    .parse_str("[1,2,3,]");
  assert!(r.is_err());
}

// ── Zero-width element guard ──────────────────────────────────────────────────
//
// A `Verbose` (recording, non-fatal) emitter keeps the drive going past the missing-separator
// diagnostics a would-be livelock emits on every repeated cycle, so the oracle below can tell an
// unguarded loop (runs the element's full budget) apart from a guarded one (stops after the
// first accept). The opener is followed by one real token so the peek/decide cycle is reached at
// all — an immediate closer would exit before ever calling the condition. Unlike the undelimited
// form, stopping still leaves that real token unconsumed and un-closed, so one `Unclosed`-family
// diagnostic from the epilogue's own close-position probe is expected on both the guarded and
// unguarded runs — it is not the livelock signal, so it is not asserted on here.

#[test]
fn zero_width_element_stops_after_one_element() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Verbose<E>>>,
  ) -> Result<Vec<i64>, E> {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: &mut Verbose<E>|
          -> Result<Action, E> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<E>>,
    >|
     -> Result<i64, E> { Ok(0) };
    elem
      .separated_by_comma_while::<_, U1>(cond)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let r: Vec<i64> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("[1]")
    .unwrap();
  assert_eq!(r.len(), 1, "zero-width guard must stop after one element");
}

#[test]
fn zero_width_element_with_leading_trivia_stops_after_one_element() {
  // Trivia-gap twin of the above: the space after the opener in `"[ 1]"` is skipped trivia, so the
  // first decision peek jumps the cache-front cursor to the token start without consuming anything.
  // A cursor-keyed guard reads that as progress and runs one extra cycle, pushing a phantom second
  // element (and, in `State::Element`, a spurious missing separator). The committed-consumption
  // metric stops after the first element.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Verbose<E>>>,
  ) -> Result<Vec<i64>, E> {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: &mut Verbose<E>|
          -> Result<Action, E> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<E>>,
    >|
     -> Result<i64, E> { Ok(0) };
    elem
      .separated_by_comma_while::<_, U1>(cond)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let r: Vec<i64> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("[ 1]")
    .unwrap();
  assert_eq!(
    r.len(),
    1,
    "the committed-progress guard must stop after one element despite the leading trivia gap"
  );
}
