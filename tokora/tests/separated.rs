#![cfg(all(feature = "std", feature = "logos"))]

//! Tests for `separated_by` and `repeated` combinators.
//!
//! Covers every separator policy variant and count modifier in
//! `parser/many/sep/` and `parser/many/sep_while/`.

mod common;

use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
  emitter::{
    FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

// ── Local error type (satisfies orphan rule for separator traits) ─────────────

/// Error type for separated-parser tests.
#[derive(Debug)]
struct SepError;

impl From<()> for SepError {
  fn from(_: ()) -> Self {
    SepError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for SepError {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    SepError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for SepError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    SepError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for SepError {
  fn from(_: TooFew<S, Lang>) -> Self {
    SepError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for SepError {
  fn from(_: TooMany<S, Lang>) -> Self {
    SepError
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for SepError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    SepError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for SepError {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    SepError
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for SepError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    SepError
  }
}

// The separated driver's separator-slot decision gate surfaces a terminal scanner stop as this
// end-of-input error (the bound the delimited variants already required).
impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for SepError {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    SepError
  }
}

// ── Element parser helpers ────────────────────────────────────────────────────

/// Try to parse a `Num` token without consuming on decline; returns SepError.
fn try_num_sep<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepError>,
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

// ── Supertrait that bundles the emitter bounds used by most sep tests ─────────

trait SepEmitter<'inp>:
  Emitter<'inp, TestLexer<'inp>, Error = SepError>
  + SeparatedEmitter<'inp, TestLexer<'inp>>
  + FullContainerEmitter<'inp, TestLexer<'inp>>
  + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
  + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
{
}

impl<'inp, E> SepEmitter<'inp> for E where
  E: Emitter<'inp, TestLexer<'inp>, Error = SepError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
{
}

// ── 1. Plain separated_by_comma (unbounded) ───────────────────────────────────

fn parse_comma_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp>,
{
  try_num_sep.separated_by_comma().collect().parse_input(inp)
}

#[test]
fn test_separated_by_comma_basic() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_comma_list).parse_str("1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_separated_by_comma_single() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_comma_list).parse_str("42");
  assert_eq!(r.unwrap(), vec![42]);
}

#[test]
fn test_separated_by_comma_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_comma_list).parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

// ── 2. Plain separated_by_semicolon ───────────────────────────────────────────

fn parse_semi_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp>,
{
  try_num_sep
    .separated_by_semicolon()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_separated_by_semicolon() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_semi_list).parse_str("10;20;30");
  assert_eq!(r.unwrap(), vec![10, 20, 30]);
}

// ── 3. at_least ───────────────────────────────────────────────────────────────

fn parse_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_separated_at_least_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_least_2).parse_str("1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_separated_at_least_fail() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_least_2).parse_str("1");
  assert!(r.is_err());
}

// ── 4. at_most ────────────────────────────────────────────────────────────────

fn parse_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_separated_at_most_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_most_2).parse_str("1,2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_separated_at_most_single() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_most_2).parse_str("7");
  assert_eq!(r.unwrap(), vec![7]);
}

// ── 5. allow_trailing ─────────────────────────────────────────────────────────

fn parse_allow_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp>,
{
  try_num_sep
    .separated_by_comma()
    .allow_trailing()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_trailing_with_trailing() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing)
    .parse_str("1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_trailing_without_trailing() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_trailing).parse_str("1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

// ── 6. allow_leading ──────────────────────────────────────────────────────────

fn parse_allow_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp>,
{
  try_num_sep
    .separated_by_comma()
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_leading_with_leading() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_leading).parse_str(",1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_leading_without_leading() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_leading).parse_str("1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

// ── 7. allow_leading + allow_trailing ─────────────────────────────────────────

fn parse_allow_leading_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp>,
{
  try_num_sep
    .separated_by_comma()
    .allow_leading()
    .allow_trailing()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_leading_and_trailing() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str(",1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

// ── 8. allow_leading + at_least ───────────────────────────────────────────────

fn parse_allow_leading_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_leading()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_leading_at_least_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_least_2)
    .parse_str(",1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_leading_at_least_fail() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_least_2)
    .parse_str(",1");
  assert!(r.is_err());
}

// ── 9. allow_leading + at_most ────────────────────────────────────────────────

fn parse_allow_leading_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_leading()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_leading_at_most_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_most_2)
    .parse_str(",1,2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

// ── 10. allow_leading + allow_trailing + at_least ─────────────────────────────

fn parse_allow_leading_allow_trailing_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_trailing()
    .at_least(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_leading_allow_trailing_at_least_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_least_2)
    .parse_str(",1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

// ── 11. allow_leading + allow_trailing + at_most ──────────────────────────────

fn parse_allow_leading_allow_trailing_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_trailing()
    .at_most(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_leading_allow_trailing_at_most_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_most_2)
    .parse_str(",1,2,");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

// ── 12. allow_leading + allow_trailing + bounded ──────────────────────────────

fn parse_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    SepEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_trailing()
    .bounded(2, 4)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_bounded_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str(",1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_bounded_too_few() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str(",1,");
  assert!(r.is_err());
}

// ── 13. allow_trailing + at_least (no leading) ────────────────────────────────

fn parse_allow_trailing_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_trailing()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_trailing_at_least_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_least_2)
    .parse_str("1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_trailing_at_least_fail() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_least_2)
    .parse_str("1,");
  assert!(r.is_err());
}

// ── 14. allow_trailing + at_most (no leading) ─────────────────────────────────

fn parse_allow_trailing_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: SepEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_trailing()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_trailing_at_most_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_most_2)
    .parse_str("1,2,");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_allow_trailing_at_most_single() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_most_2)
    .parse_str("5,");
  assert_eq!(r.unwrap(), vec![5]);
}

// ── 15. allow_trailing + bounded (no leading) ─────────────────────────────────

fn parse_allow_trailing_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    SepEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_trailing()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_trailing_bounded_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_trailing_bounded_fail_too_few() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,");
  assert!(r.is_err());
}

// ── 16. allow_leading + bounded ───────────────────────────────────────────────

fn parse_allow_leading_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    SepEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_sep
    .separated_by_comma()
    .allow_leading()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_allow_leading_bounded_ok() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str(",1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_leading_bounded_fail_too_few() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str(",1");
  assert!(r.is_err());
}

// ── 17. TryParseInput::repeated — manual fold via loop ────────────────────────

fn parse_manual_fold<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, SepError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepError>,
{
  let mut sum = 0i64;
  while let ParseAttempt::Accept(n) = try_num_sep(inp)? {
    sum += n;
  }
  Ok(sum)
}

#[test]
fn test_manual_fold() {
  let r: Result<i64, SepError> = Parser::new()
    .apply(parse_manual_fold)
    .parse_str("1 2 3 4 5");
  assert_eq!(r.unwrap(), 15);
}

// ── Error path tests: at_most too many ──────────────────────────────────────

#[test]
fn test_separated_at_most_too_many() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_most_2).parse_str("1,2,3");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_at_most_too_many() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_most_2)
    .parse_str(",1,2,3");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_at_most_too_many() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_most_2)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_allow_trailing_at_most_too_many() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_most_2)
    .parse_str(",1,2,3,");
  assert!(r.is_err());
}

// ── Error path tests: bounded too many ──────────────────────────────────────

#[test]
fn test_bounded_too_many() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str(",1,2,3,4,5,");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,2,3,4,5,");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_bounded_too_many() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str(",1,2,3,4,5");
  assert!(r.is_err());
}

// ── Boundary tests: exactly at limit ────────────────────────────────────────

#[test]
fn test_separated_at_most_exactly_max() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_most_2).parse_str("1,2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_separated_at_least_exactly_min() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_least_2).parse_str("1,2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

// ── Empty input tests ───────────────────────────────────────────────────────

#[test]
fn test_separated_at_least_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_least_2).parse_str("");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_at_least_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_least_2)
    .parse_str("");
  assert!(r.is_err());
}

// ── Handler error path tests: plain separator policy ────────────────────────

#[test]
fn test_plain_trailing_separator_error() {
  // Triggers Unbounded EndStateHandler::handle_separator_state (unexpected trailing)
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_comma_list).parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn test_plain_leading_separator_error() {
  // Triggers Unbounded SeparatorStateHandler::handle_start_state (unexpected leading)
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_comma_list).parse_str(",1,2,3");
  assert!(r.is_err());
}

#[test]
fn test_plain_only_separator() {
  // Triggers SeparatorStateHandler::handle_start_state then EndStateHandler::handle_leading_state
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_comma_list).parse_str(",");
  assert!(r.is_err());
}

// ── Handler error path tests: allow_trailing with invalid leading ───────────

#[test]
fn test_allow_trailing_leading_separator_error() {
  // Triggers AllowTrailing<Unbounded> SeparatorStateHandler::handle_start_state (unexpected leading)
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing)
    .parse_str(",1,2,3");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_only_separator() {
  // Triggers SeparatorStateHandler::handle_start_state then handle_leading_state
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_trailing).parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_trailing).parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_allow_trailing_single() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_trailing).parse_str("42");
  assert_eq!(r.unwrap(), vec![42]);
}

#[test]
fn test_allow_trailing_single_with_trailing() {
  // Single item with trailing comma -- triggers handle_separator_state
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_trailing).parse_str("42,");
  assert_eq!(r.unwrap(), vec![42]);
}

// ── Handler error path tests: allow_leading with invalid trailing ────────────

#[test]
fn test_allow_leading_trailing_separator_error() {
  // Triggers AllowLeading<Unbounded> EndStateHandler::handle_separator_state (unexpected trailing)
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_leading).parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_only_separator() {
  // Leading sep consumed, then handle_leading_state fires (sep with no following element)
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_leading).parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_leading).parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_allow_leading_single() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_leading).parse_str("42");
  assert_eq!(r.unwrap(), vec![42]);
}

#[test]
fn test_allow_leading_single_with_leading() {
  // Single item with leading comma -- triggers SeparatorStateHandler::handle_start_state
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_allow_leading).parse_str(",42");
  assert_eq!(r.unwrap(), vec![42]);
}

// ── Handler error path tests: allow_surrounded (allow_leading + allow_trailing) ─

#[test]
fn test_allow_surrounded_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_allow_surrounded_single() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str("42");
  assert_eq!(r.unwrap(), vec![42]);
}

#[test]
fn test_allow_surrounded_only_leading() {
  // Triggers handle_leading_state on AllowLeading<AllowTrailing<Unbounded>>
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str(",1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_surrounded_only_trailing() {
  // Triggers handle_separator_state on AllowLeading<AllowTrailing<Unbounded>>
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str("1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_surrounded_only_separator() {
  // "," with both leading and trailing allowed -- triggers handle_leading_state (leading sep, no element follows)
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_surrounded_single_with_leading() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str(",42");
  assert_eq!(r.unwrap(), vec![42]);
}

#[test]
fn test_allow_surrounded_single_with_trailing() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str("42,");
  assert_eq!(r.unwrap(), vec![42]);
}

#[test]
fn test_allow_surrounded_single_with_both() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_trailing)
    .parse_str(",42,");
  assert_eq!(r.unwrap(), vec![42]);
}

// ── Handler error path tests: at_most with different policies ───────────────

#[test]
fn test_plain_at_most_empty() {
  // Triggers EndStateHandler::handle_start_state on Maximum (0 items, at_most ok)
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_at_most_2).parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_allow_trailing_at_most_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_most_2)
    .parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_allow_leading_at_most_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_most_2)
    .parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

// ── Handler error path tests: at_least with empty for different policies ────

#[test]
fn test_allow_leading_at_least_empty() {
  // Triggers EndStateHandler::handle_start_state on AllowLeading<Minimum> (too few)
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_least_2)
    .parse_str("");
  assert!(r.is_err());
}

// ── Handler error path tests: bounded edge cases ────────────────────────────

#[test]
fn test_bounded_empty() {
  // Triggers handle_start_state on AllowLeading<AllowTrailing<With<Min,Max>>> (too few)
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str("");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_bounded_empty() {
  // Triggers handle_start_state on AllowTrailing<With<Min,Max>>
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_bounded_empty() {
  // Triggers handle_start_state on AllowLeading<With<Min,Max>>
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn test_bounded_exactly_min() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str(",1,2,");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_bounded_exactly_max() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str(",1,2,3,4,");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn test_allow_trailing_bounded_exactly_min() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,2,");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_allow_trailing_bounded_exactly_max() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str("1,2,3,4,");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn test_allow_leading_bounded_exactly_min() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str(",1,2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_allow_leading_bounded_exactly_max() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str(",1,2,3,4");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

// ── Handler error path tests: allow_trailing at_least/at_most with leading sep ──

#[test]
fn test_allow_trailing_at_least_leading_sep_error() {
  // Triggers AllowTrailing<Minimum> SeparatorStateHandler::handle_start_state
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_least_2)
    .parse_str(",1,2,3");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_at_most_leading_sep_error() {
  // Triggers AllowTrailing<Maximum> SeparatorStateHandler::handle_start_state
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_most_2)
    .parse_str(",1,2");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_bounded_leading_sep_error() {
  // Triggers AllowTrailing<With<Min,Max>> SeparatorStateHandler::handle_start_state
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str(",1,2,3");
  assert!(r.is_err());
}

// ── Handler error path tests: allow_leading at_least/at_most with trailing sep ──

#[test]
fn test_allow_leading_at_least_trailing_sep_error() {
  // Triggers AllowLeading<Minimum> EndStateHandler::handle_separator_state (unexpected trailing)
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_least_2)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_at_most_trailing_sep_error() {
  // Triggers AllowLeading<Maximum> EndStateHandler::handle_separator_state (unexpected trailing)
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_most_2)
    .parse_str("1,2,");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_bounded_trailing_sep_error() {
  // Triggers AllowLeading<With<Min,Max>> EndStateHandler::handle_separator_state (unexpected trailing)
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str("1,2,3,");
  assert!(r.is_err());
}

// ── Handler error path tests: allow_leading with only separator ─────────────

#[test]
fn test_allow_leading_at_least_only_separator() {
  // "," with allow_leading at_least(2) -- leading sep accepted, no element, handle_leading_state
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_least_2)
    .parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_at_most_only_separator() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_most_2)
    .parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_leading_bounded_only_separator() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_bounded)
    .parse_str(",");
  assert!(r.is_err());
}

// ── Handler error path tests: allow_trailing with only separator ────────────

#[test]
fn test_allow_trailing_at_least_only_separator() {
  // "," with allow_trailing at_least(2) -- starts with sep, triggers leading error
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_least_2)
    .parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_at_most_only_separator() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_most_2)
    .parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_trailing_bounded_only_separator() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_bounded)
    .parse_str(",");
  assert!(r.is_err());
}

// ── Handler error path tests: allow_surrounded bounded edge cases ───────────

#[test]
fn test_allow_surrounded_at_least_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_least_2)
    .parse_str("");
  assert!(r.is_err());
}

#[test]
fn test_allow_surrounded_at_least_too_few() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_least_2)
    .parse_str(",1,");
  assert!(r.is_err());
}

#[test]
fn test_allow_surrounded_at_least_only_separator() {
  // Triggers handle_leading_state on AllowLeading<AllowTrailing<Minimum>>
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_least_2)
    .parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_surrounded_at_most_empty() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_most_2)
    .parse_str("");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_allow_surrounded_at_most_only_separator() {
  // Triggers handle_leading_state on AllowLeading<AllowTrailing<Maximum>>
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_most_2)
    .parse_str(",");
  assert!(r.is_err());
}

#[test]
fn test_allow_surrounded_bounded_only_separator() {
  // Triggers handle_leading_state on AllowLeading<AllowTrailing<With<Min,Max>>>
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str(",");
  assert!(r.is_err());
}

// ── Handler error path tests: trailing sep + too few interaction ────────────

#[test]
fn test_allow_trailing_at_least_single_with_trailing() {
  // "1," -- trailing is allowed, but only 1 item, at_least(2) fails
  // Triggers handle_separator_state then too_few check
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_trailing_at_least_2)
    .parse_str("1,");
  assert!(r.is_err());
}

// ── Handler error path tests: leading sep + too few interaction ─────────────

#[test]
fn test_allow_leading_at_least_single_with_leading() {
  // ",1" -- leading is allowed, but only 1 item, at_least(2) fails
  // Triggers handle_element_state then too_few check
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_at_least_2)
    .parse_str(",1");
  assert!(r.is_err());
}

// ── Handler error path tests: surrounded + trailing/leading sep combos ──────

#[test]
fn test_allow_surrounded_at_least_leading_only() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_least_2)
    .parse_str(",1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_surrounded_at_least_trailing_only() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_least_2)
    .parse_str("1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_surrounded_at_most_leading_only() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_most_2)
    .parse_str(",1,2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_allow_surrounded_at_most_trailing_only() {
  let r: Result<Vec<i64>, SepError> = Parser::new()
    .apply(parse_allow_leading_allow_trailing_at_most_2)
    .parse_str("1,2,");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_allow_surrounded_bounded_leading_only() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str(",1,2,3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_allow_surrounded_bounded_trailing_only() {
  let r: Result<Vec<i64>, SepError> = Parser::new().apply(parse_bounded).parse_str("1,2,3,");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}
