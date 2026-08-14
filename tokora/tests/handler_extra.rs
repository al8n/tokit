#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
#![allow(warnings)]
mod common;

use common::E;

// Integration tests targeting additional handler/ modules for separated parsers.
// Exercises boundary conditions, error paths, and edge cases in:
//   - allow_trailing with at_most, bounded
//   - allow_leading + require_trailing with at_most, bounded
//   - require_surrounded (require_leading + require_trailing) with at_least, at_most, bounded
//   - require_leading + allow_trailing with at_most
//   - require_trailing unbounded
//   - require_leading unbounded
//   - maximum, bounded handler edge cases

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
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
// 1. allow_trailing + bounded — handler/allow_trailing/bounded.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_at_bounded_1_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .bounded(1, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn at_bounded_ok_exactly_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn at_bounded_ok_exactly_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn at_bounded_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn at_bounded_ok_max_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn at_bounded_too_few_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn at_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1,2,3,4");
  assert!(r.is_err());
}

#[test]
fn at_bounded_too_many_with_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str("1,2,3,4,");
  assert!(r.is_err());
}

#[test]
fn at_bounded_leading_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_bounded_1_3)
    .parse_str(",1,2");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. allow_trailing + at_most — handler/allow_trailing/at_most.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_at_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn at_at_most_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn at_at_most_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
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
fn at_at_most_ok_single_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn at_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_at_at_most_2)
    .parse_str("1,2,3");
  assert!(r.is_err());
}

#[test]
fn at_at_most_exceeded_trailing() {
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
    .parse_str(",1");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. require_trailing + unbounded — handler/require_trailing/unbounded.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rt_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
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
fn rt_unbounded_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_unbounded)
    .parse_str("1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rt_unbounded_ok_multiple() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_unbounded)
    .parse_str("1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rt_unbounded_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rt_unbounded)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn rt_unbounded_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_unbounded)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn rt_unbounded_missing_trailing_single() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_unbounded)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn rt_unbounded_leading_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_unbounded)
    .parse_str(",1,");
  assert!(r.is_err());
}

#[test]
fn rt_unbounded_only_comma() {
  // Leading separator with no element => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rt_unbounded)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. require_leading + unbounded — handler/require_leading/unbounded.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rl_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
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
fn rl_unbounded_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_unbounded)
    .parse_str(",1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rl_unbounded_ok_multiple() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_unbounded)
    .parse_str(",1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rl_unbounded_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rl_unbounded)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn rl_unbounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_unbounded)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn rl_unbounded_missing_leading_single() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_unbounded)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn rl_unbounded_trailing_sep_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_unbounded)
    .parse_str(",1,2,");
  assert!(r.is_err());
}

#[test]
fn rl_unbounded_leading_only() {
  // Leading separator with no element following => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rl_unbounded)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. require_surrounded (require_trailing + require_leading) + bounded
//    handler/require_surrounded/bounded.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rs_bounded_1_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
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
fn rs_bounded_ok_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rs_bounded_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rs_bounded_ok_mid() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rs_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_err());
}

#[test]
fn rs_bounded_empty_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn rs_bounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str("1,2,");
  assert!(r.is_err());
}

#[test]
fn rs_bounded_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn rs_bounded_missing_both() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_bounded_1_3)
    .parse_str("1,2");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. require_surrounded + at_least — handler/require_surrounded/at_least.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rs_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_least(2)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rs_at_least_ok_exact() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least_2)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rs_at_least_ok_more() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least_2)
    .parse_str(",1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rs_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least_2)
    .parse_str(",1,");
  assert!(r.is_err());
}

#[test]
fn rs_at_least_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least_2)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn rs_at_least_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least_2)
    .parse_str("1,2,");
  assert!(r.is_err());
}

#[test]
fn rs_at_least_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least_2)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn rs_at_least_leading_only() {
  // Just a comma => missing element after leading sep
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_least_2)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. require_surrounded + at_most — handler/require_surrounded/at_most.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rs_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
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
fn rs_at_most_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rs_at_most_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
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

#[test]
fn rs_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn rs_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str("1,2,");
  assert!(r.is_err());
}

#[test]
fn rs_at_most_missing_both() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn rs_at_most_only_sep() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rs_at_most_2)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. allow_leading + require_trailing + bounded
//    handler/allow_leading_require_trailing/bounded.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_alrt_bounded_1_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_trailing()
    .bounded(1, 3)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_bounded_ok_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str("1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn alrt_bounded_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str("1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn alrt_bounded_ok_with_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn alrt_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str("1,2,3,4,");
  assert!(r.is_err());
}

#[test]
fn alrt_bounded_too_few_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn alrt_bounded_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn alrt_bounded_leading_only_missing_elem() {
  // Leading separator with no element => error (missing element after trailing)
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_bounded_1_3)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. allow_leading + require_trailing + at_most
//    handler/allow_leading_require_trailing/at_most.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_alrt_at_most_2<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .require_trailing()
    .at_most(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn alrt_at_most_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn alrt_at_most_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn alrt_at_most_ok_with_leading() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str(",1,")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn alrt_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn alrt_at_most_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn alrt_at_most_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn alrt_at_most_leading_only() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_alrt_at_most_2)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. require_leading + allow_trailing + at_most
//     handler/require_leading_allow_trailing/at_most.rs
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rlat_at_most_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .allow_trailing()
    .at_most(3)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn rlat_at_most_ok_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str(",1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn rlat_at_most_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str(",1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn rlat_at_most_ok_with_trailing() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str(",1,2,")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn rlat_at_most_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str(",1,2,3,4");
  assert!(r.is_err());
}

#[test]
fn rlat_at_most_exceeded_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str(",1,2,3,4,");
  assert!(r.is_err());
}

#[test]
fn rlat_at_most_empty() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str("")
    .unwrap();
  assert_eq!(r, Vec::<i64>::new());
}

#[test]
fn rlat_at_most_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn rlat_at_most_leading_only() {
  // Leading separator with no element => error
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_rlat_at_most_3)
    .parse_str(",");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. maximum handler (plain at_most) — handler/maximum.rs
//     Additional edge cases for separator_state (trailing sep with at_most)
//     and leading_state paths
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_plain_at_most_3<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .at_most(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_at_most_3_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_3)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn plain_at_most_3_trailing_sep() {
  // trailing separator without policy => error (unexpected trailing)
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_3)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn plain_at_most_3_leading_sep() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_3)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn plain_at_most_3_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_at_most_3)
    .parse_str("1,2,3,4");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. bounded handler (plain bounded) — handler/bounded.rs
//     Additional edge cases for separator_state and leading_state
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_plain_bounded_2_4<'inp, Ctx>(
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
  try_num
    .separated_by_comma()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn plain_bounded_2_4_ok_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn plain_bounded_2_4_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1,2,3,4")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn plain_bounded_2_4_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn plain_bounded_2_4_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1,2,3,4,5");
  assert!(r.is_err());
}

#[test]
fn plain_bounded_2_4_trailing_sep() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn plain_bounded_2_4_leading_sep() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn plain_bounded_2_4_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_plain_bounded_2_4)
    .parse_str("");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 13. Repeated (non-separated) with maximum and bounded
//     handler/maximum.rs RepeatedHandler + handler/bounded.rs RepeatedHandler
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_repeated_at_most_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num.repeated().at_most(3).collect().parse_input(inp)
}

#[test]
fn repeated_at_most_3_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_most_3)
    .parse_str("1 2 3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn repeated_at_most_3_exceeded() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_most_3)
    .parse_str("1 2 3 4");
  assert!(r.is_err());
}

#[test]
fn repeated_at_most_3_single() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_at_most_3)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

fn parse_repeated_bounded_1_4<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .repeated()
    .at_most(4)
    .at_least(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn repeated_bounded_1_4_ok_min() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_1_4)
    .parse_str("1")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn repeated_bounded_1_4_ok_max() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_1_4)
    .parse_str("1 2 3 4")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn repeated_bounded_1_4_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_1_4)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn repeated_bounded_1_4_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_repeated_bounded_1_4)
    .parse_str("1 2 3 4 5");
  assert!(r.is_err());
}
