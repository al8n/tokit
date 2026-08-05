#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]
mod common;

use common::E;

// Integration tests using a **recovering** emitter (returns Ok(()) for all errors)
// to exercise handler code paths that are unreachable with a fatal emitter.
// Covers: handle_leading_state, handle_separator_state, handle_element_state,
// handle_too_many_element, and handle_start_state across all handler combinations.

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  emitter::{
    Fatal, FromSeparatedError, FromUnexpectedLeadingSeparatorError,
    FromUnexpectedTrailingSeparatorError, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, PrattEmitter, SeparatedEmitter, Silent, TooFewEmitter,
    TooManyEmitter, UnclosedEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    NonAssociativeChain, RecursionLimitReached, Unclosed, UnexpectedEoLhs, UnexpectedEoRhs,
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingToken, MissingTokenOf, SeparatedError, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  span::{SimpleSpan, Spanned},
  try_parse_input::ParseAttempt,
  utils::{CowStr, GenericArrayDeque, marker::Ignored, typenum::U2},
};

use common::{TestLexer, Token, TokenKind};

fn recovering_ctx() -> ParserContext<'static, TestLexer<'static>, Silent<E>> {
  ParserContext::new(Silent::new())
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
// 1. allow_trailing + unbounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_at_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .allow_trailing()
    .collect()
    .parse_input(inp)
}

#[test]
fn at_unbounded_leading_sep_only() {
  // Input "," — hits handle_leading_state
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn at_unbounded_trailing_sep() {
  // Input "1," — hits handle_separator_state
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_unbounded)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn at_unbounded_leading_sep_recovery() {
  // Input ",1" — hits SeparatorStateHandler::handle_start_state with recovery
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_unbounded)
    .parse_str(",1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. allow_trailing + at_most
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
fn at_at_most_leading_sep_only() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_most_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn at_at_most_trailing_sep() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn at_at_most_overflow() {
  // Input "1, 2, 3" with at_most(2) — hits handle_too_many_element
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

#[test]
fn at_at_most_overflow_trailing() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,2,3,");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. allow_trailing + at_least
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_at_at_least_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn at_at_least_leading_sep_only() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_least_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn at_at_least_trailing_sep() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_least_2)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn at_at_least_too_few_recovery() {
  // Input "1" with at_least(2) — too few, but recovering
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_least_2)
    .parse_str("1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. allow_trailing + bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_at_bounded_1_3<'inp, Ctx>(
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
    .bounded(1, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn at_bounded_leading_sep_only() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn at_bounded_trailing_sep() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn at_bounded_overflow() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1,2,3,4");
  assert!(r.is_ok());
}

#[test]
fn at_bounded_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. allow_leading + at_most
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
fn al_at_most_trailing_sep() {
  // Trailing sep with allow_leading — hits handle_separator_state
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_at_most_2)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn al_at_most_overflow() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_at_most_2)
    .parse_str(",1,2,3");
  assert!(r.is_ok());
}

#[test]
fn al_at_most_overflow_no_leading() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_at_most_2)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. allow_leading + at_least
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_al_at_least_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_leading()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn al_at_least_trailing_sep() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_at_least_2)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

#[test]
fn al_at_least_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_at_least_2)
    .parse_str(",1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. allow_leading + bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_al_bounded_1_3<'inp, Ctx>(
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
    .bounded(1, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn al_bounded_trailing_sep() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_bounded_1_3)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn al_bounded_overflow() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_bounded_1_3)
    .parse_str(",1,2,3,4");
  assert!(r.is_ok());
}

#[test]
fn al_bounded_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_bounded_1_3)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. require_trailing + unbounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_unbounded_missing_trailing_recovery() {
  // Input "1" — hits handle_element_state (missing trailing sep)
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_unbounded)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn rt_unbounded_leading_sep_only_recovery() {
  // Input "," — hits handle_leading_state
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rt_unbounded_missing_trailing_multi_recovery() {
  // Input "1,2" — hits handle_element_state
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_unbounded)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn rt_unbounded_leading_sep_recovery() {
  // Input ",1," — hits SeparatorStateHandler::handle_start_state with recovery
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_unbounded)
    .parse_str(",1,");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. require_trailing + at_most
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
fn rt_at_most_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn rt_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1,2,3,");
  assert!(r.is_ok());
}

#[test]
fn rt_at_most_overflow_no_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. require_trailing + at_least
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
fn rt_at_least_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn rt_at_least_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn rt_at_least_too_few_no_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str("1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. require_trailing + bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_bounded_1_3<'inp, Ctx>(
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
    .bounded(1, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn rt_bounded_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_bounded_1_3)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn rt_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_bounded_1_3)
    .parse_str("1,2,3,4,");
  assert!(r.is_ok());
}

#[test]
fn rt_bounded_overflow_no_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_bounded_1_3)
    .parse_str("1,2,3,4");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. require_leading + unbounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_unbounded_missing_leading_recovery() {
  // Input "1" — hits ContinueStateHandler::handle_start_state (missing leading)
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_unbounded)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn rl_unbounded_trailing_sep_recovery() {
  // Input ",1," — hits handle_separator_state (unexpected trailing)
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_unbounded)
    .parse_str(",1,");
  assert!(r.is_ok());
}

#[test]
fn rl_unbounded_leading_only_recovery() {
  // Input "," — hits handle_leading_state
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rl_unbounded_missing_leading_multi_recovery() {
  // Input "1,2" — missing leading, recovery continues
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_unbounded)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. require_leading + at_most
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
fn rl_at_most_trailing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",1,");
  assert!(r.is_ok());
}

#[test]
fn rl_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",1,2,3");
  assert!(r.is_ok());
}

#[test]
fn rl_at_most_missing_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. require_leading + at_least
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_at_least_2<'inp, Ctx>(
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
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_at_least_missing_leading_recovery() {
  // Input "1,2" — missing leading, recovery
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_least_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn rl_at_least_trailing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_least_2)
    .parse_str(",1,2,");
  assert!(r.is_ok());
}

#[test]
fn rl_at_least_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_least_2)
    .parse_str(",1");
  assert!(r.is_ok());
}

#[test]
fn rl_at_least_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_least_2)
    .parse_str(",");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 15. require_leading + bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_bounded_1_3<'inp, Ctx>(
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
    .bounded(1, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn rl_bounded_trailing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_bounded_1_3)
    .parse_str(",1,");
  assert!(r.is_ok());
}

#[test]
fn rl_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_bounded_1_3)
    .parse_str(",1,2,3,4");
  assert!(r.is_ok());
}

#[test]
fn rl_bounded_missing_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_bounded_1_3)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 16. allow_leading_require_trailing + unbounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_alrt_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_unbounded_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_unbounded)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn alrt_unbounded_leading_sep_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn alrt_unbounded_missing_trailing_multi_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_unbounded)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 17. allow_leading_require_trailing + at_most
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
fn alrt_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1,2,3,");
  assert!(r.is_ok());
}

#[test]
fn alrt_at_most_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 18. allow_leading_require_trailing + at_least
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_alrt_at_least_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_least(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_at_least_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_least_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn alrt_at_least_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_least_2)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn alrt_at_least_leading_sep_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_least_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn alrt_at_least_missing_trailing_single_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_least_2)
    .parse_str("1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 19. allow_leading_require_trailing + bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_alrt_bounded_1_3<'inp, Ctx>(
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
    .bounded(1, 3)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_ok());
}

#[test]
fn alrt_bounded_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 20. require_leading_allow_trailing + unbounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rlat_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .allow_trailing()
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_unbounded_missing_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_unbounded)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn rlat_unbounded_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rlat_unbounded_missing_leading_multi_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_unbounded)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 21. require_leading_allow_trailing + at_most
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
fn rlat_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str(",1,2,3");
  assert!(r.is_ok());
}

#[test]
fn rlat_at_most_missing_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 22. require_leading_allow_trailing + at_least
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rlat_at_least_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .at_least(2)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_at_least_missing_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_least_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn rlat_at_least_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_least_2)
    .parse_str(",1");
  assert!(r.is_ok());
}

#[test]
fn rlat_at_least_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_least_2)
    .parse_str(",");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 23. require_leading_allow_trailing + bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rlat_bounded_1_3<'inp, Ctx>(
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
    .bounded(1, 3)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_bounded_1_3)
    .parse_str(",1,2,3,4");
  assert!(r.is_ok());
}

#[test]
fn rlat_bounded_missing_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_bounded_1_3)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn rlat_bounded_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_bounded_1_3)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 24. allow_surrounded + at_most (allow_trailing + allow_leading)
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
fn as_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_as_at_most_2)
    .parse_str(",1,2,3,");
  assert!(r.is_ok());
}

#[test]
fn as_at_most_overflow_no_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_as_at_most_2)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 25. allow_surrounded + bounded (allow_trailing + allow_leading)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_as_bounded_1_3<'inp, Ctx>(
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
    .bounded(1, 3)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn as_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_as_bounded_1_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_ok());
}

#[test]
fn as_bounded_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_as_bounded_1_3)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 26. require_surrounded + unbounded (require_trailing + require_leading)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rs_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_unbounded_missing_trailing_recovery() {
  // Input ",1" — missing trailing sep
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_unbounded)
    .parse_str(",1");
  assert!(r.is_ok());
}

#[test]
fn rs_unbounded_missing_leading_recovery() {
  // Input "1," — missing leading sep
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_unbounded)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn rs_unbounded_leading_only_recovery() {
  // Input "," — hits handle_leading_state
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rs_unbounded_missing_both_recovery() {
  // Input "1" — missing both leading and trailing
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_unbounded)
    .parse_str("1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 27. require_surrounded + at_most
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
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
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
fn rs_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,2,3,");
  assert!(r.is_ok());
}

#[test]
fn rs_at_most_missing_both_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn rs_at_most_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 28. require_surrounded + bounded
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rs_bounded_1_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .bounded(1, 3)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_bounded_missing_trailing_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

#[test]
fn rs_bounded_missing_leading_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

#[test]
fn rs_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_ok());
}

#[test]
fn rs_bounded_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rs_bounded_missing_both_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str("1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 29. Plain bounded (handler/bounded.rs)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_plain_bounded_2_4<'inp, Ctx>(
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
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_bounded_trailing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

#[test]
fn plain_bounded_leading_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

#[test]
fn plain_bounded_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn plain_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1,2,3,4,5");
  assert!(r.is_ok());
}

#[test]
fn plain_bounded_empty_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 30. Plain maximum (handler/maximum.rs)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_plain_at_most_2<'inp, Ctx>(
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
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_at_most_trailing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_most_2)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

#[test]
fn plain_at_most_leading_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_most_2)
    .parse_str(",1");
  assert!(r.is_ok());
}

#[test]
fn plain_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_most_2)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 31. Plain minimum (handler/minimum.rs)
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_plain_at_least_2<'inp, Ctx>(
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
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_at_least_trailing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_least_2)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

#[test]
fn plain_at_least_leading_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_least_2)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

#[test]
fn plain_at_least_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_least_2)
    .parse_str("1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 32. Repeated (non-separated) handler coverage
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
fn repeated_at_most_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_repeated_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

fn parse_repeated_bounded_2_4<'inp, Ctx>(
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
    .at_most(4)
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn repeated_bounded_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_repeated_bounded_2_4)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn repeated_bounded_overflow_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_repeated_bounded_2_4)
    .parse_str("1 2 3 4 5");
  assert!(r.is_ok());
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
fn repeated_at_least_too_few_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_repeated_at_least_2)
    .parse_str("1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 33. handler/mod.rs — SeparatorHandler coverage for blackhole impls
// ═══════════════════════════════════════════════════════════════════════════════

// The PhantomData and GenericArrayDeque SeparatorHandler impls (lines 47-49, 57-59, 75)
// are covered transitively via the separated parser collecting into Vec (which
// uses Vec's SeparatorHandler). These are no-op impls invoked during parsing.

// ═══════════════════════════════════════════════════════════════════════════════
// 34. Additional edge cases: leading-only for combinations with bounded checks
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_at_most_leading_sep_with_elem() {
  // ",1" with allow_trailing at_most — leading sep error then element
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_most_2)
    .parse_str(",1");
  assert!(r.is_ok());
}

#[test]
fn rt_at_most_leading_sep_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rt_bounded_leading_sep_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_bounded_1_3)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rt_at_least_leading_sep_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_least_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rs_at_most_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn al_at_most_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_at_most_2)
    .parse_str(",1");
  assert!(r.is_ok());
}

#[test]
fn al_bounded_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_bounded_1_3)
    .parse_str(",1");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 35. Missing element after leading sep for require_leading variants
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn rl_at_most_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rl_bounded_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_bounded_1_3)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rlat_at_most_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn rlat_bounded_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_bounded_1_3)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn alrt_at_most_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn alrt_bounded_leading_only_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str(",");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 36. Missing separator tests (consecutive elements without commas)
//     These reach the `State::Element` arm of `handle_continue`, which reports the missing
//     separator and keeps parsing. The count bound is NOT checked here: it is checked once,
//     by the end-state pass, on the true parsed count. The mid-loop `handle_too_many_element`
//     hook these rows once exercised was deleted — it duplicated the end check with an
//     understated payload, and its only callers were these malformed-input arms, so it was
//     never a resource guard. Its coverage is retired, not lost: the end check owns the
//     maximum, and `end_state_parity.rs` case A pins one `TooMany` per construct across all
//     eight drivers.
// ═══════════════════════════════════════════════════════════════════════════════

// -- allow_trailing + at_most --
#[test]
fn at_at_most_missing_sep_recovery() {
  // "1 2 3" with at_most(2): parse 1 (Element), see 2 (no comma) → missing-separator report
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- allow_trailing + bounded --
#[test]
fn at_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- allow_leading + at_most --
#[test]
fn al_at_most_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- allow_leading + bounded --
#[test]
fn al_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_al_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- allow_leading_require_trailing + at_most --
#[test]
fn alrt_at_most_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- allow_leading_require_trailing + bounded --
#[test]
fn alrt_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- allow_surrounded + at_most --
#[test]
fn as_at_most_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_as_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- allow_surrounded + bounded --
#[test]
fn as_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_as_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- require_trailing + at_most --
#[test]
fn rt_at_most_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- require_trailing + bounded --
#[test]
fn rt_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rt_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- require_leading + at_most --
#[test]
fn rl_at_most_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- require_leading + bounded --
#[test]
fn rl_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rl_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- require_leading_allow_trailing + at_most --
#[test]
fn rlat_at_most_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- require_leading_allow_trailing + bounded --
#[test]
fn rlat_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- require_surrounded + at_most --
#[test]
fn rs_at_most_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// -- require_surrounded + bounded --
#[test]
fn rs_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str("1 2 3 4");
  assert!(r.is_ok());
}

// -- plain bounded (With<Minimum, Maximum>) --
#[test]
fn plain_bounded_missing_sep_recovery() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1 2 3 4 5");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 37. Plain bounded handle_leading_state — input "," with plain bounded
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn plain_bounded_leading_only_recovery() {
  // "," alone hits EndStateHandler::handle_leading_state for With<Minimum, Maximum>
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str(",");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 38. Plain maximum/minimum handle_leading_state — input "," alone
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn plain_at_most_leading_only_recovery() {
  // "," alone hits EndStateHandler::handle_leading_state for Maximum
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_most_2)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn plain_at_least_leading_only_recovery() {
  // "," alone hits EndStateHandler::handle_leading_state for Minimum
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_plain_at_least_2)
    .parse_str(",");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 39. Plain maximum/minimum handle_separator_state — input "1," (trailing sep)
// ═══════════════════════════════════════════════════════════════════════════════

// plain_at_most already has trailing sep test (plain_at_most_trailing_sep_recovery)
// plain_at_least already has trailing sep test (plain_at_least_trailing_sep_recovery)
// But we also need the separator-only-after-leading case to ensure full coverage.

// ═══════════════════════════════════════════════════════════════════════════════
// 40. EndStateHandler::handle_start_state for composite types (empty input)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn alrt_unbounded_empty_input_recovery() {
  // "" hits EndStateHandler::handle_start_state for AllowLeading<RequireTrailing<Unbounded>>
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_unbounded)
    .parse_str("");
  assert!(r.is_ok());
}

#[test]
fn alrt_at_least_empty_input_recovery() {
  // "" hits EndStateHandler::handle_start_state for AllowLeading<RequireTrailing<AtLeast>>
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_alrt_at_least_2)
    .parse_str("");
  assert!(r.is_ok());
}

#[test]
fn rlat_unbounded_empty_input_recovery() {
  // "" hits EndStateHandler::handle_start_state for RequireLeading<AllowTrailing<Unbounded>>
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_unbounded)
    .parse_str("");
  assert!(r.is_ok());
}

#[test]
fn rlat_at_least_empty_input_recovery() {
  // "" hits EndStateHandler::handle_start_state for RequireLeading<AllowTrailing<AtLeast>>
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_rlat_at_least_2)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Merged from handler_emitter_coverage.rs
//
// Coverage tests for:
// - `parser/many/handler/mod.rs` lines 47-49, 57-59, 75, 228-237, 245-254, 270, 277
//   — SeparatorHandler and DelimiterHandler impls on `()`, `PhantomData<T>`,
//     and `GenericArrayDeque` (exercised by running parsers that collect into those types)
// - `emitter/mod.rs` lines 170, 177, 181, 188, 192, 196, 200, 204
//   — `&mut U` delegation impl for `Emitter`
// - `emitter/pratt.rs` lines 30, 37, 41, 48
//   — `&mut U` delegation impl for `PrattEmitter`
// - `emitter/separated/` files
//   — `&mut U` delegation impls for separated emitter traits
// ═══════════════════════════════════════════════════════════════════════════════

// ── Error type (emitter coverage) ─────────────────────────────────────────────

#[derive(Debug)]
enum Err {
  Any,
}

impl From<()> for Err {
  fn from(_: ()) -> Self {
    Err::Any
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Err {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    Err::Any
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Err
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    Err::Any
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for Err {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Err::Any
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Err {
  fn from(_: TooFew<S, Lang>) -> Self {
    Err::Any
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Err {
  fn from(_: TooMany<S, Lang>) -> Self {
    Err::Any
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Err {
  fn from(_: FullContainer<S, Lang>) -> Self {
    Err::Any
  }
}

impl From<UnexpectedEot> for Err {
  fn from(_: UnexpectedEot) -> Self {
    Err::Any
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEoLhs<O, Lang>> for Err {
  fn from(_: UnexpectedEoLhs<O, Lang>) -> Self {
    Err::Any
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEoRhs<O, Lang>> for Err {
  fn from(_: UnexpectedEoRhs<O, Lang>) -> Self {
    Err::Any
  }
}

impl<O, Lang: ?Sized> From<RecursionLimitReached<O, Lang>> for Err {
  fn from(_: RecursionLimitReached<O, Lang>) -> Self {
    Err::Any
  }
}

impl<O, Lang: ?Sized> From<NonAssociativeChain<O, Lang>> for Err {
  fn from(_: NonAssociativeChain<O, Lang>) -> Self {
    Err::Any
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for Err {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    Err::Any
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Err {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Err::Any
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for Err {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    Err::Any
  }
}

// ── Minimal tracking emitter ─────────────────────────────────────────────────

/// An emitter that tracks calls made through it, used to verify `&mut U`
/// delegation actually reaches the inner emitter.
struct TrackingEmitter {
  calls: usize,
  /// Counted separately from `calls`: `release` fires on every commit/forget path of the
  /// input layer, so folding it into the shared counter would shift every existing tally.
  releases: usize,
  /// Counted separately from `calls`: CST events are a channel, not emissions — a
  /// forwarding gap here drops the tree while diagnostics flow, so the tally must not be
  /// maskable by diagnostic traffic.
  cst_events: usize,
}

impl TrackingEmitter {
  fn new() -> Self {
    Self {
      calls: 0,
      releases: 0,
      cst_events: 0,
    }
  }
}

impl<'inp> Emitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  type Error = Err;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }

  fn emit_error(
    &mut self,
    _: Spanned<Err, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, _: u64)
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
  }

  fn release(&mut self, _: u64) {
    self.releases += 1;
  }

  /// The token channel. It lives on the CORE trait rather than on `CstEmitter` — a settle is
  /// the only way a token event is born — so its `&mut U` forwarding gap is the same W3 class
  /// as the structuring methods below, and is counted with them.
  fn commit_token(
    &mut self,
    _: &<TestLexer<'inp> as Lexer<'inp>>::Token,
    _: &<TestLexer<'inp> as Lexer<'inp>>::Span,
  ) where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.cst_events += 1;
  }
}

impl<'inp> PrattEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_unexpected_end_of_lhs(
    &mut self,
    _: UnexpectedEoLhs<<TestLexer<'inp> as Lexer<'inp>>::Offset>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }

  fn emit_unexpected_end_of_rhs(
    &mut self,
    _: UnexpectedEoRhs<<TestLexer<'inp> as Lexer<'inp>>::Offset>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> SeparatedEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_missing_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }

  fn emit_missing_element(&mut self, _: MissingSyntaxOf<'inp, TestLexer<'inp>>) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_unexpected_leading_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_unexpected_trailing_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_missing_leading_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_missing_trailing_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> UnclosedEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_unclosed<Delimiter>(
    &mut self,
    _: Unclosed<Delimiter, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
    Err: tokora::emitter::FromUnclosed<'inp, TestLexer<'inp>>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> TooFewEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_too_few(&mut self, _: TooFew<<TestLexer<'inp> as Lexer<'inp>>::Span>) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

impl<'inp> TooManyEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn emit_too_many(&mut self, _: TooMany<<TestLexer<'inp> as Lexer<'inp>>::Span>) -> Result<(), Err>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    self.calls += 1;
    Ok(())
  }
}

fn tracking_ctx() -> ParserContext<'static, TestLexer<'static>, TrackingEmitter> {
  ParserContext::new(TrackingEmitter::new())
}

fn silent_ctx() -> ParserContext<'static, TestLexer<'static>, Silent<Err>> {
  ParserContext::new(Silent::new())
}

// ── Element parser helper (emitter coverage) ──────────────────────────────────

fn try_num_emitter<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>,
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
// emitter/mod.rs — &mut U delegation for Emitter (lines 170, 177, 181, 188,
//                   192, 196, 200, 204)
// Calling a method on `&mut emitter` explicitly invokes the &mut U forwarding
// impl rather than the concrete impl.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn emitter_mut_ref_emit_lexer_error() {
  let mut emitter = TrackingEmitter::new();
  // r is &mut TrackingEmitter; calling through &mut r invokes the &mut U impl
  let mut r: &mut TrackingEmitter = &mut emitter;
  let spanned = Spanned::new(SimpleSpan::new(0usize, 1usize), ());
  <&mut TrackingEmitter as Emitter<'_, TestLexer<'_>>>::emit_lexer_error(&mut r, spanned).unwrap();
  assert_eq!(emitter.calls, 1);
}

#[test]
fn emitter_mut_ref_emit_unexpected_token() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let ut = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  <&mut TrackingEmitter as Emitter<'_, TestLexer<'_>>>::emit_unexpected_token(&mut r, ut).unwrap();
  assert_eq!(emitter.calls, 1);
}

#[test]
fn emitter_mut_ref_emit_error() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let spanned = Spanned::new(SimpleSpan::new(0usize, 1usize), Err::Any);
  <&mut TrackingEmitter as Emitter<'_, TestLexer<'_>>>::emit_error(&mut r, spanned).unwrap();
  assert_eq!(emitter.calls, 1);
}

#[test]
fn emitter_mut_ref_release() {
  // The W3 forwarding-gap class: a defaulted trait method the `&mut U` blanket impl fails to
  // forward resolves to the *default no-op* on `&mut E`, silently dropping the capability while
  // everything else flows. Drive `release` through the blanket impl and assert it reaches the
  // inner emitter's override.
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  <&mut TrackingEmitter as Emitter<'_, TestLexer<'_>>>::release(&mut r, 0);
  assert_eq!(emitter.releases, 1);
  assert_eq!(
    emitter.calls, 0,
    "release must not masquerade as an emission"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// emitter/pratt.rs — &mut U delegation for PrattEmitter (lines 30, 37, 41, 48)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn pratt_emitter_mut_ref_emit_lhs() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let err = UnexpectedEoLhs::eolhs(0usize);
  <&mut TrackingEmitter as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_lhs(
    &mut r, err,
  )
  .unwrap();
  assert_eq!(emitter.calls, 1);
}

#[test]
fn pratt_emitter_mut_ref_emit_rhs() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let err = UnexpectedEoRhs::eorhs(0usize);
  <&mut TrackingEmitter as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_rhs(
    &mut r, err,
  )
  .unwrap();
  assert_eq!(emitter.calls, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// emitter/separated/mod.rs — &mut U delegation for SeparatedEmitter
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn separated_emitter_mut_ref_missing_separator() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let name = CowStr::from_static("comma");
  let err: MissingToken<'_, TokenKind, usize> = MissingToken::new(0usize);
  <&mut TrackingEmitter as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
    &mut r, name, err,
  )
  .unwrap();
  assert_eq!(emitter.calls, 1);
}

#[test]
fn separated_emitter_mut_ref_missing_element() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let err = MissingSyntax::new(0usize);
  <&mut TrackingEmitter as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_element(&mut r, err)
    .unwrap();
  assert_eq!(emitter.calls, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// emitter/separated/unexpected_leading.rs — &mut U delegation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn unexpected_leading_separator_emitter_mut_ref() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  <&mut TrackingEmitter as UnexpectedLeadingSeparatorEmitter<
    '_,
    TestLexer<'_>,
  >>::emit_unexpected_leading_separator(&mut r, name, err)
  .unwrap();
  assert_eq!(emitter.calls, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// emitter/separated/unexpected_trailing.rs — &mut U delegation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn unexpected_trailing_separator_emitter_mut_ref() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  <&mut TrackingEmitter as UnexpectedTrailingSeparatorEmitter<
    '_,
    TestLexer<'_>,
  >>::emit_unexpected_trailing_separator(&mut r, name, err)
  .unwrap();
  assert_eq!(emitter.calls, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// emitter/separated/missing_leading.rs — &mut U delegation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn missing_leading_separator_emitter_mut_ref() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let name = CowStr::from_static("comma");
  let err: MissingToken<'_, TokenKind, usize> = MissingToken::new(0usize);
  <&mut TrackingEmitter as MissingLeadingSeparatorEmitter<
    '_,
    TestLexer<'_>,
  >>::emit_missing_leading_separator(&mut r, name, err)
  .unwrap();
  assert_eq!(emitter.calls, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// emitter/separated/missing_trailing.rs — &mut U delegation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn missing_trailing_separator_emitter_mut_ref() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let name = CowStr::from_static("comma");
  let err: MissingToken<'_, TokenKind, usize> = MissingToken::new(0usize);
  <&mut TrackingEmitter as MissingTrailingSeparatorEmitter<
    '_,
    TestLexer<'_>,
  >>::emit_missing_trailing_separator(&mut r, name, err)
  .unwrap();
  assert_eq!(emitter.calls, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// parser/many/handler/mod.rs — SeparatorHandler impl for ()
// (lines 47-49)
//
// To exercise on_separator for (), we run a separated parser that collects
// into () — the library calls container.on_separator() during parsing.
// ═══════════════════════════════════════════════════════════════════════════════

fn sep_into_unit<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_emitter
    .separated_by_comma()
    .collect()
    .parse_input(inp)
}

#[test]
fn separator_handler_unit_via_parser() {
  // Parsing "1,2,3" collecting into () triggers on_separator on ()
  let r: Result<(), _> = Parser::with_context(tracking_ctx())
    .apply(sep_into_unit)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// parser/many/handler/mod.rs — SeparatorHandler impl for PhantomData<T>
// (lines 57-59)
//
// Collect into PhantomData<i64> — triggers on_separator for PhantomData.
// ═══════════════════════════════════════════════════════════════════════════════

fn sep_into_phantom<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<core::marker::PhantomData<i64>, Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_emitter
    .separated_by_comma()
    .collect()
    .parse_input(inp)
}

#[test]
fn separator_handler_phantom_data_via_parser() {
  let r: Result<core::marker::PhantomData<i64>, _> = Parser::with_context(tracking_ctx())
    .apply(sep_into_phantom)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// parser/many/handler/mod.rs — SeparatorHandler impl for GenericArrayDeque
// (line 75)
//
// Collect into GenericArrayDeque<i64, U2>.
// ═══════════════════════════════════════════════════════════════════════════════

fn sep_into_gad<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<GenericArrayDeque<i64, U2>, Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_emitter
    .separated_by_comma()
    .collect()
    .parse_input(inp)
}

#[test]
fn separator_handler_gad_via_parser() {
  // Parse 2 elements (capacity of U2 GAD)
  let r: Result<GenericArrayDeque<i64, U2>, _> = Parser::with_context(tracking_ctx())
    .apply(sep_into_gad)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// parser/many/handler/mod.rs — SeparatorHandler impl for Ignored<T>
// (line 68 via @generic macro)
//
// Collect into Ignored<i64> — triggers on_separator for Ignored.
// ═══════════════════════════════════════════════════════════════════════════════

fn sep_into_ignored<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Ignored<i64>, Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_emitter
    .separated_by_comma()
    .collect()
    .parse_input(inp)
}

#[test]
fn separator_handler_ignored_via_parser() {
  let r: Result<Ignored<i64>, _> = Parser::with_context(tracking_ctx())
    .apply(sep_into_ignored)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// parser/many/handler/mod.rs — DelimiterHandler impls on (), PhantomData, GAD
// (lines 228-237, 245-254, 270, 277)
//
// Delimited parsers call on_open_delimiter and on_close_delimiter on the
// container type. Using bracket-delimited parsers with those container types
// exercises those impls.
// ═══════════════════════════════════════════════════════════════════════════════

fn delim_into_unit<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  use tokora::{parser::With, punct::Bracket};
  try_num_emitter
    .separated_by_comma()
    .delimited::<Bracket>()
    .collect()
    .parse_input(inp)
}

#[test]
fn delimiter_handler_unit_via_parser() {
  // "[1,2,3]" exercises on_open_delimiter and on_close_delimiter for ()
  let r: Result<(), _> = Parser::with_context(tracking_ctx())
    .apply(delim_into_unit)
    .parse_str("[1,2,3]");
  assert!(r.is_ok());
}

fn delim_into_phantom<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<core::marker::PhantomData<i64>, Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  use tokora::{parser::With, punct::Bracket};
  try_num_emitter
    .separated_by_comma()
    .delimited::<Bracket>()
    .collect()
    .parse_input(inp)
}

#[test]
fn delimiter_handler_phantom_data_via_parser() {
  let r: Result<core::marker::PhantomData<i64>, _> = Parser::with_context(tracking_ctx())
    .apply(delim_into_phantom)
    .parse_str("[1,2]");
  assert!(r.is_ok());
}

fn delim_into_gad<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<GenericArrayDeque<i64, U2>, Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  use tokora::{parser::With, punct::Bracket};
  try_num_emitter
    .separated_by_comma()
    .delimited::<Bracket>()
    .collect()
    .parse_input(inp)
}

#[test]
fn delimiter_handler_gad_via_parser() {
  let r: Result<GenericArrayDeque<i64, U2>, _> = Parser::with_context(tracking_ctx())
    .apply(delim_into_gad)
    .parse_str("[1,2]");
  assert!(r.is_ok());
}

fn delim_into_ignored<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Ignored<i64>, Err>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Err>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  use tokora::{parser::With, punct::Bracket};
  try_num_emitter
    .separated_by_comma()
    .delimited::<Bracket>()
    .collect()
    .parse_input(inp)
}

#[test]
fn delimiter_handler_ignored_via_parser() {
  let r: Result<Ignored<i64>, _> = Parser::with_context(tracking_ctx())
    .apply(delim_into_ignored)
    .parse_str("[1,2,3]");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// emitter/cst.rs — &mut U delegation for CstEmitter (the W3 forwarding-gap class)
// A defaulted CST method the `&mut U` blanket impl failed to forward would resolve
// to the *default no-op* on `&mut E` — the exact configuration parse_partial
// round-threading uses — silently dropping the whole tree while diagnostics flow.
// Drive every method through the blanket impl and assert it reaches the inner
// emitter's override, without touching the diagnostic tally.
// ═══════════════════════════════════════════════════════════════════════════════

use tokora::{cst::event::EventMark, emitter::CstEmitter};

impl<'inp> CstEmitter<'inp, TestLexer<'inp>> for TrackingEmitter {
  fn cst_start(&mut self, _: u16) {
    self.cst_events += 1;
  }

  fn cst_finish(&mut self, _: u16) {
    self.cst_events += 1;
  }

  fn cst_mark(&mut self) -> EventMark {
    self.cst_events += 1;
    // No public EventMark constructor exists (only a recording sink mints live marks), so
    // the tracking override hands back the defaulted inert mark of a diagnostics-only
    // emitter; the forwarding proof is the counter, not the value.
    <Fatal<E> as CstEmitter<'inp, TestLexer<'inp>>>::cst_mark(&mut Fatal::new())
  }

  fn cst_start_at(&mut self, _: EventMark, _: u16) {
    self.cst_events += 1;
  }
}

#[test]
fn cst_emitter_mut_ref_start() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  <&mut TrackingEmitter as CstEmitter<'_, TestLexer<'_>>>::cst_start(&mut r, 1);
  assert_eq!(emitter.cst_events, 1);
  assert_eq!(
    emitter.calls, 0,
    "an event must not masquerade as an emission"
  );
}

/// `CstEmitter` has no `cst_token` — a token event is born from a **settle**, and the settle
/// hook is [`Emitter::commit_token`] on the core trait. Its `&mut U` forward carries the same
/// W3 hazard as the structuring methods above (forward the structure, inherit the token no-op,
/// get a tree with no leaves), so it is driven through the blanket here beside them.
#[test]
fn emitter_mut_ref_commit_token() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let tok = Token::Num(7);
  let span = SimpleSpan::new(0usize, 1usize);
  <&mut TrackingEmitter as Emitter<'_, TestLexer<'_>>>::commit_token(&mut r, &tok, &span);
  assert_eq!(emitter.cst_events, 1);
  assert_eq!(
    emitter.calls, 0,
    "a settle must not masquerade as an emission"
  );
}

#[test]
fn cst_emitter_mut_ref_finish() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  <&mut TrackingEmitter as CstEmitter<'_, TestLexer<'_>>>::cst_finish(&mut r, 1);
  assert_eq!(emitter.cst_events, 1);
  assert_eq!(emitter.calls, 0);
}

#[test]
fn cst_emitter_mut_ref_mark_and_start_at() {
  let mut emitter = TrackingEmitter::new();
  let mut r: &mut TrackingEmitter = &mut emitter;
  let m = <&mut TrackingEmitter as CstEmitter<'_, TestLexer<'_>>>::cst_mark(&mut r);
  <&mut TrackingEmitter as CstEmitter<'_, TestLexer<'_>>>::cst_start_at(&mut r, m, 3);
  assert_eq!(emitter.cst_events, 2);
  assert_eq!(emitter.calls, 0);
}

/// The diagnostics-only bundles opt into the event channel via the defaulted no-ops, so a
/// parse path bounding `Ctx::Emitter: CstEmitter` accepts them unchanged (tree-less
/// configuration) — while a wrapper emitter gets no blanket opt-in and must implement the
/// trait deliberately. The `&mut` forms hold through the blanket impl.
#[test]
fn cst_emitter_bound_accepts_diagnostics_only_bundles() {
  fn requires_cst_emitter<'inp, T>(_: &T)
  where
    T: CstEmitter<'inp, TestLexer<'inp>>,
  {
  }

  let mut fatal = Fatal::<E>::new();
  let mut silent = Silent::<E>::new();
  let mut verbose = tokora::emitter::Verbose::<E>::new();
  let ignored = Ignored::<()>::default();
  requires_cst_emitter(&fatal);
  requires_cst_emitter(&silent);
  requires_cst_emitter(&verbose);
  requires_cst_emitter(&ignored);
  requires_cst_emitter(&&mut fatal);
  requires_cst_emitter(&&mut silent);
  requires_cst_emitter(&&mut verbose);

  // The inert mark a defaulted cst_mark returns can never name a live slot.
  let m = <Fatal<E> as CstEmitter<'_, TestLexer<'_>>>::cst_mark(&mut fatal);
  assert_eq!(m.index(), u64::MAX);
}
