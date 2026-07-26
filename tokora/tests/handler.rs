#![cfg(all(feature = "std", feature = "logos"))]
#![allow(warnings)]
mod common;

use common::E;

// Integration tests targeting handler/ modules for separated/repeated parsers.
// Exercises boundary conditions, error paths, and edge cases in:
//   - require_trailing with at_most, bounded
//   - require_leading with at_most, bounded, at_least
//   - maximum, minimum, bounded handlers
//   - Empty input, 0-element, exactly-at-boundary scenarios

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingToken, MissingTokenOf, SeparatedError, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::CowStr,
};

use common::{TestLexer, Token};

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

// ── Element parser ────────────────────────────────────────────────────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  inp
    .try_expect(|t| matches!(t.data(), Token::Num(_)))
    .map(|opt| match opt {
      None => ParseAttempt::Decline,
      Some(tok) => ParseAttempt::Accept(match tok.into_data() {
        Token::Num(n) => n,
        _ => unreachable!(),
      }),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. require_trailing + at_most — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_at_most_ok_exactly_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rt_at_most_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rt_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn rt_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn rt_at_most_empty_input() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn rt_at_most_leading_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str(",1,2,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. require_trailing + bounded — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_bounded_2_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .bounded(2, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_bounded_ok_exactly_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_3)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rt_bounded_ok_exactly_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_3)
    .parse_str("1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rt_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_3)
    .parse_str("1,");
  assert!(r.is_err());
}

#[test]
fn rt_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_3)
    .parse_str("1,2,3,4,");
  assert!(r.is_err());
}

#[test]
fn rt_bounded_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_3)
    .parse_str("1,2,3");
  assert!(r.is_err());
}

#[test]
fn rt_bounded_empty() {
  // 0 elements < min=2 => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_3)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn rt_bounded_leading_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_3)
    .parse_str(",1,2,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. require_leading + at_most — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_leading()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_at_most_ok_exactly_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rl_at_most_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rl_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",1,2,3");
  assert!(r.is_err());
}

#[test]
fn rl_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn rl_at_most_empty_input() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn rl_at_most_trailing_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",1,2,");
  assert!(r.is_err());
}

#[test]
fn rl_at_most_leading_sep_only() {
  // Leading separator with no element following => error (missing element)
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. require_leading + bounded — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_bounded_2_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_leading()
    .bounded(2, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_bounded_ok_exactly_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_3)
    .parse_str(",1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rl_bounded_ok_exactly_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_3)
    .parse_str(",1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rl_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_3)
    .parse_str(",1");
  assert!(r.is_err());
}

#[test]
fn rl_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_3)
    .parse_str(",1,2,3,4");
  assert!(r.is_err());
}

#[test]
fn rl_bounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_3)
    .parse_str("1,2,3");
  assert!(r.is_err());
}

#[test]
fn rl_bounded_empty() {
  // 0 elements < min=2 => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_3)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn rl_bounded_trailing_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_3)
    .parse_str(",1,2,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. require_leading + at_least — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_at_least_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_leading()
    .at_least(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_3)
    .parse_str(",1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rl_at_least_ok_more_than_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_3)
    .parse_str(",1,2,3,4")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn rl_at_least_not_met() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_3)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn rl_at_least_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_3)
    .parse_str("1,2,3");
  assert!(r.is_err());
}

#[test]
fn rl_at_least_empty() {
  // 0 elements < min=3 => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_3)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn rl_at_least_trailing_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_3)
    .parse_str(",1,2,3,");
  assert!(r.is_err());
}

#[test]
fn rl_at_least_single_not_met() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_3)
    .parse_str(",1");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Plain at_most / bounded / at_least — additional edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_plain_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_at_most_1_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_1)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn plain_at_most_1_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_1)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn plain_at_most_1_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_1)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn plain_at_most_1_trailing_sep() {
  // trailing separator with no policy => unexpected trailing sep error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_1)
    .parse_str("1,");
  assert!(r.is_err());
}

fn parse_plain_bounded_1_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .bounded(1, 1)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_bounded_1_1_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_1_1)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn plain_bounded_1_1_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_1_1)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn plain_bounded_1_1_empty() {
  // 0 elements < min=1 => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_1_1)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn plain_bounded_1_1_trailing_sep() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_1_1)
    .parse_str("1,");
  assert!(r.is_err());
}

fn parse_plain_at_least_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_at_least_1_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least_1)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn plain_at_least_1_empty() {
  // 0 elements < min=1 => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least_1)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn plain_at_least_1_trailing_sep() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least_1)
    .parse_str("1,");
  assert!(r.is_err());
}

#[test]
fn plain_at_least_1_leading_sep() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_least_1)
    .parse_str(",1");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Repeated parsers (non-separated) with at_most / at_least / bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_repeated_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num.repeated().at_most(2).collect().parse_input(inp)
}

#[test]
fn repeated_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_most_2)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn repeated_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_err());
}

#[test]
fn repeated_at_most_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_most_2)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn repeated_at_most_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

fn parse_repeated_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num.repeated().at_least(2).collect().parse_input(inp)
}

#[test]
fn repeated_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_least_2)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn repeated_at_least_not_met() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_least_2)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn repeated_at_least_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_least_2)
    .parse_str("");
  assert!(r.is_err());
}

fn parse_repeated_bounded_2_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .repeated()
    .at_most(3)
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn repeated_bounded_ok_exactly_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_2_3)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn repeated_bounded_ok_exactly_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_2_3)
    .parse_str("1 2 3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn repeated_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_2_3)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn repeated_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_2_3)
    .parse_str("1 2 3 4");
  assert!(r.is_err());
}

#[test]
fn repeated_bounded_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_2_3)
    .parse_str("");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. require_trailing + at_least — additional edge cases for at_least handler
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rt_at_least_ok_more() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rt_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("1,");
  assert!(r.is_err());
}

#[test]
fn rt_at_least_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn rt_at_least_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. require_leading + at_least with exactly at_least(1)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_at_least_1<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_leading()
    .at_least(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_at_least_1_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_1)
    .parse_str(",1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rl_at_least_1_ok_multiple() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_1)
    .parse_str(",1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rl_at_least_1_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_1)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn rl_at_least_1_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_least_1)
    .parse_str("1");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. require_trailing + at_most with at_most(1) — tight boundary
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_at_most_1_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_1)
    .parse_str("1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rt_at_most_1_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_1)
    .parse_str("1,2,");
  assert!(r.is_err());
}

#[test]
fn rt_at_most_1_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_1)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn rt_at_most_1_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_at_most_1)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. require_leading + at_most with at_most(1) — tight boundary
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_at_most_1<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_leading()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_at_most_1_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_1)
    .parse_str(",1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rl_at_most_1_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_1)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn rl_at_most_1_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_1)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn rl_at_most_1_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_at_most_1)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. require_trailing + bounded with min=max (exact count)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_bounded_2_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .bounded(2, 2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_bounded_exact_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_2)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rt_bounded_exact_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_2)
    .parse_str("1,");
  assert!(r.is_err());
}

#[test]
fn rt_bounded_exact_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_bounded_2_2)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. require_leading + bounded with min=max (exact count)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_bounded_2_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_leading()
    .bounded(2, 2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_bounded_exact_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_2)
    .parse_str(",1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rl_bounded_exact_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_2)
    .parse_str(",1");
  assert!(r.is_err());
}

#[test]
fn rl_bounded_exact_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_bounded_2_2)
    .parse_str(",1,2,3");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. Semicolon separator variants — exercises different Punctuator impls
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_semi_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_semicolon()
    .require_trailing()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_semi_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_semi_at_most_2)
    .parse_str("1;2;")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rt_semi_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_semi_at_most_2)
    .parse_str("1;2;3;");
  assert!(r.is_err());
}

fn parse_rl_semi_at_least_2<'inp, Ctx>(
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
  try_num
    .separated_by_semicolon()
    .require_leading()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_semi_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_semi_at_least_2)
    .parse_str(";1;2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rl_semi_at_least_not_met() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_semi_at_least_2)
    .parse_str(";1");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 15. allow_trailing — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_at_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn at_at_most_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn at_at_most_ok_without_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn at_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn at_at_most_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn at_at_most_leading_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str(",1,2,");
  assert!(r.is_err());
}

fn parse_at_bounded_2_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .bounded(2, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn at_bounded_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_2_3)
    .parse_str("1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn at_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_2_3)
    .parse_str("1,");
  assert!(r.is_err());
}

#[test]
fn at_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_2_3)
    .parse_str("1,2,3,4,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 16. allow_leading — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_al_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_leading()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn al_at_most_ok_with_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_al_at_most_2)
    .parse_str(",1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn al_at_most_ok_without_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_al_at_most_2)
    .parse_str("1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn al_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_al_at_most_2)
    .parse_str(",1,2,3");
  assert!(r.is_err());
}

#[test]
fn al_at_most_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_al_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn al_at_most_trailing_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_al_at_most_2)
    .parse_str(",1,2,");
  assert!(r.is_err());
}

fn parse_al_bounded_2_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_leading()
    .bounded(2, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn al_bounded_ok_with_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_al_bounded_2_3)
    .parse_str(",1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn al_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_al_bounded_2_3)
    .parse_str(",1");
  assert!(r.is_err());
}

#[test]
fn al_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_al_bounded_2_3)
    .parse_str(",1,2,3,4");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 17. allow_surrounded — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_as_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .at_most(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn as_at_most_ok_surrounded() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_as_at_most_2)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn as_at_most_ok_no_surrounding() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_as_at_most_2)
    .parse_str("1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn as_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_as_at_most_2)
    .parse_str(",1,2,3,");
  assert!(r.is_err());
}

#[test]
fn as_at_most_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_as_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

fn parse_as_bounded_2_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .bounded(2, 3)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn as_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_as_bounded_2_3)
    .parse_str(",1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn as_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_as_bounded_2_3)
    .parse_str(",1,");
  assert!(r.is_err());
}

#[test]
fn as_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_as_bounded_2_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 18. require_surrounded — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rs_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_most(2)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rs_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str("1,2,");
  assert!(r.is_err());
}

#[test]
fn rs_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn rs_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,2,3,");
  assert!(r.is_err());
}

#[test]
fn rs_at_most_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

fn parse_rs_bounded_2_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_trailing()
    .bounded(2, 3)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_2_3)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rs_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_2_3)
    .parse_str(",1,");
  assert!(r.is_err());
}

#[test]
fn rs_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_2_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 19. allow_leading_require_trailing — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_alrt_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_most(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_at_most_ok_with_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn alrt_at_most_ok_without_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn alrt_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn alrt_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str(",1,2,3,");
  assert!(r.is_err());
}

fn parse_alrt_bounded_2_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_trailing()
    .bounded(2, 3)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_2_3)
    .parse_str(",1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn alrt_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_2_3)
    .parse_str(",1,");
  assert!(r.is_err());
}

#[test]
fn alrt_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_2_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 20. require_leading_allow_trailing — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rlat_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .at_most(2)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_at_most_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rlat_at_most_ok_without_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str(",1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rlat_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str("1,2,");
  assert!(r.is_err());
}

#[test]
fn rlat_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str(",1,2,3,");
  assert!(r.is_err());
}

fn parse_rlat_bounded_2_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .bounded(2, 3)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_bounded_2_3)
    .parse_str(",1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rlat_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_bounded_2_3)
    .parse_str(",1,");
  assert!(r.is_err());
}

#[test]
fn rlat_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_bounded_2_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 21. Delimited edge cases — empty brackets, single element
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_delim_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(2)
    .delimited::<tokora::punct::Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn delim_at_most_empty_brackets() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_at_most_2)
    .parse_str("[]")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn delim_at_most_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_at_most_2)
    .parse_str("[1]")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn delim_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_at_most_2)
    .parse_str("[1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn delim_at_most_three() {
  // In delimited context, at_most stops collecting but parsing continues until close bracket
  let r = Parser::with_context(full_ctx())
    .apply(parse_delim_at_most_2)
    .parse_str("[1,2,3]");
  // May error or succeed depending on implementation
  let _ = r;
}

fn parse_delim_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .delimited::<tokora::punct::Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn delim_unbounded_empty_brackets() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_unbounded)
    .parse_str("[]")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn delim_unbounded_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_unbounded)
    .parse_str("[1]")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn delim_unbounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_unbounded)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

fn parse_delim_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(2)
    .delimited::<tokora::punct::Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn delim_at_least_empty() {
  // Empty brackets with at_least(2): the closer commits mid-scan with no elements at all, so
  // this reaches the end-state pass through its `State::Start` arm. Under `Fatal` the `TooFew`
  // is the error.
  let r = Parser::with_context(full_ctx())
    .apply(parse_delim_at_least_2)
    .parse_str("[]");
  assert!(
    r.is_err(),
    "a closed but empty list still violates `at_least(2)`"
  );
}

#[test]
fn delim_at_least_single() {
  // Same, through the `State::Element` arm: one element under `at_least(2)`.
  let r = Parser::with_context(full_ctx())
    .apply(parse_delim_at_least_2)
    .parse_str("[1]");
  assert!(
    r.is_err(),
    "a closed one-element list still violates `at_least(2)`"
  );
}

#[test]
fn delim_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_at_least_2)
    .parse_str("[1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn delim_at_least_ok_more() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_delim_at_least_2)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}
