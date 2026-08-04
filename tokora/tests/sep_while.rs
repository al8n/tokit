#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests for the `SeparatedWhile` (condition-closure-based) combinator.
//!
//! Exercises `separated_by_comma_while` with plain, `at_least`, `at_most`,
//! `bounded`, `allow_trailing`, `allow_leading`, and combined variants.
//!
//! # Sentinel token
//!
//! `SeparatedWhile::parse` calls `peek_with_emitter` after every failed
//! separator attempt.  At EOF there is nothing to peek, which triggers a
//! debug_assert.  We therefore append `+` (a non-comma, non-Num token) to
//! every test string so the condition always sees a stop token instead of
//! hitting EOF.  The trailing `+` is left unconsumed; `parse_str` does not
//! require all tokens to be consumed.

mod common;

use generic_arraydeque::typenum::U1;
use tokora::EmitterView;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::Peeked,
  emitter::{
    FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, Verbose,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
};

use common::{TestLexer, Token};

// ── Local error type (satisfies orphan rule for separator traits) ─────────────

#[derive(Debug)]
struct WhileError;

impl From<()> for WhileError {
  fn from(_: ()) -> Self {
    WhileError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for WhileError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    WhileError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for WhileError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    WhileError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for WhileError {
  fn from(_: TooFew<S, Lang>) -> Self {
    WhileError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for WhileError {
  fn from(_: TooMany<S, Lang>) -> Self {
    WhileError
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for WhileError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    WhileError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for WhileError
{
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    WhileError
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for WhileError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    WhileError
  }
}

// The separated_while driver's separator-slot and decision-window gates surface a terminal scanner
// stop as this end-of-input error (the bound the delimited variants already required).
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for WhileError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    WhileError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for WhileError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    WhileError
  }
}

// ── Supertrait bundling common emitter bounds ─────────────────────────────────

trait WhileEmitter<'inp>:
  Emitter<'inp, TestLexer<'inp>, Error = WhileError>
  + SeparatedEmitter<'inp, TestLexer<'inp>>
  + FullContainerEmitter<'inp, TestLexer<'inp>>
  + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
  + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
{
}

impl<'inp, E> WhileEmitter<'inp> for E where
  E: Emitter<'inp, TestLexer<'inp>, Error = WhileError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
{
}

// ── Condition: continue iff the next token is a number (element start) ────────
//
// The `SeparatedWhile` loop first tries to consume a separator (comma).
// The condition is called only when the separator was NOT found; at that
// point the peek buffer contains whatever comes next.  Returning `Continue`
// means "this token starts another element"; `Stop` means "we are done."
//
// Using a sentinel `+` token in test inputs ensures the condition always
// receives a non-EOF token, avoiding the debug_assert in `peek_with_emitter`.

fn decide_num<'inp, Ctx>(
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

// ── Element parser (ParseInput, not TryParseInput) ────────────────────────────

fn parse_num_while<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = WhileError>,
{
  match inp.next()? {
    None => Err(WhileError),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(WhileError),
    },
  }
}

// ── 1. Plain separated_by_comma_while (unbounded) ────────────────────────────

fn parse_while_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_basic() {
  // "1,2,3+" — the trailing `+` acts as the stop sentinel.
  let r: Result<Vec<i64>, WhileError> = Parser::new().apply(parse_while_list).parse_str("1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_single() {
  let r: Result<Vec<i64>, WhileError> = Parser::new().apply(parse_while_list).parse_str("42+");
  assert_eq!(r.unwrap(), vec![42]);
}

// ── 2. at_least ───────────────────────────────────────────────────────────────

fn parse_while_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_at_least_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_at_least_2)
    .parse_str("1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_at_least_fail() {
  // Only 1 element; at_least(2) should fail.
  let r: Result<Vec<i64>, WhileError> = Parser::new().apply(parse_while_at_least_2).parse_str("1+");
  assert!(r.is_err());
}

// ── 3. at_most ────────────────────────────────────────────────────────────────

fn parse_while_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_at_most_ok() {
  let r: Result<Vec<i64>, WhileError> =
    Parser::new().apply(parse_while_at_most_2).parse_str("1,2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_sep_while_at_most_single() {
  let r: Result<Vec<i64>, WhileError> = Parser::new().apply(parse_while_at_most_2).parse_str("7+");
  assert_eq!(r.unwrap(), vec![7]);
}

// ── 4. bounded ────────────────────────────────────────────────────────────────

fn parse_while_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_bounded_ok() {
  let r: Result<Vec<i64>, WhileError> =
    Parser::new().apply(parse_while_bounded).parse_str("1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_bounded_too_few() {
  let r: Result<Vec<i64>, WhileError> = Parser::new().apply(parse_while_bounded).parse_str("1+");
  assert!(r.is_err());
}

// ── 5. allow_trailing ─────────────────────────────────────────────────────────

fn parse_while_allow_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_trailing_with_trailing() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_trailing)
    .parse_str("1,2,3,+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_allow_trailing_without_trailing() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_trailing)
    .parse_str("1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

// ── 6. allow_leading ──────────────────────────────────────────────────────────

fn parse_while_allow_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_leading_with_leading() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading)
    .parse_str(",1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_allow_leading_without_leading() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading)
    .parse_str("1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

// ── 7. allow_surrounding (leading + trailing) ─────────────────────────────────

fn parse_while_allow_surrounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_surrounded() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded)
    .parse_str(",1,2,3,+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

// ── 8. allow_trailing + at_least ──────────────────────────────────────────────

fn parse_while_allow_trailing_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_trailing_at_least_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_trailing_at_least_2)
    .parse_str("1,2,3,+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_allow_trailing_at_least_fail() {
  // Trailing comma, only 1 element; at_least(2) should fail.
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_trailing_at_least_2)
    .parse_str("1,+");
  assert!(r.is_err());
}

// ── 9. allow_surrounded + at_least ────────────────────────────────────────────

fn parse_while_allow_surrounded_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_surrounded_at_least_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded_at_least_2)
    .parse_str(",1,2,3,+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_allow_surrounded_at_least_fail() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded_at_least_2)
    .parse_str(",1,+");
  assert!(r.is_err());
}

// ── 10. allow_surrounded + at_most ────────────────────────────────────────────

fn parse_while_allow_surrounded_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .at_most(2)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_surrounded_at_most_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded_at_most_2)
    .parse_str(",1,2,+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

// ── 11. allow_surrounded + bounded ────────────────────────────────────────────

fn parse_while_allow_surrounded_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_surrounded_bounded_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded_bounded)
    .parse_str(",1,2,3,+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_allow_surrounded_bounded_fail() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded_bounded)
    .parse_str(",1,+");
  assert!(r.is_err());
}

// ── 12. allow_leading + at_least ──────────────────────────────────────────────

fn parse_while_allow_leading_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_leading_at_least_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading_at_least_2)
    .parse_str(",1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_allow_leading_at_least_fail() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading_at_least_2)
    .parse_str(",1+");
  assert!(r.is_err());
}

// ── 13. allow_leading + at_most ───────────────────────────────────────────────

fn parse_while_allow_leading_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_leading_at_most_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading_at_most_2)
    .parse_str(",1,2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

// ── 14. allow_leading + bounded ───────────────────────────────────────────────

fn parse_while_allow_leading_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_leading_bounded_ok() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading_bounded)
    .parse_str(",1,2,3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_sep_while_allow_leading_bounded_fail() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading_bounded)
    .parse_str(",1+");
  assert!(r.is_err());
}

// ── Error path tests: at_most too many ──────────────────────────────────────

#[test]
fn test_sep_while_at_most_too_many() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_at_most_2)
    .parse_str("1,2,3+");
  assert!(r.is_err());
}

#[test]
fn test_sep_while_allow_trailing_at_most_too_many() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_trailing_at_least_2)
    .parse_str("1,+");
  // only 1 element, at_least(2) → err
  assert!(r.is_err());
}

#[test]
fn test_sep_while_allow_leading_at_most_too_many() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading_at_most_2)
    .parse_str(",1,2,3+");
  assert!(r.is_err());
}

#[test]
fn test_sep_while_allow_surrounded_at_most_too_many() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded_at_most_2)
    .parse_str(",1,2,3,+");
  assert!(r.is_err());
}

// ── Bounded too many tests ──────────────────────────────────────────────────

#[test]
fn test_sep_while_bounded_too_many() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_bounded)
    .parse_str("1,2,3,4,5+");
  assert!(r.is_err());
}

#[test]
fn test_sep_while_allow_surrounded_bounded_too_many() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_surrounded_bounded)
    .parse_str(",1,2,3,4,5,+");
  assert!(r.is_err());
}

#[test]
fn test_sep_while_allow_leading_bounded_too_many() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_allow_leading_bounded)
    .parse_str(",1,2,3,4,5+");
  assert!(r.is_err());
}

// ── Empty/single element edge cases ─────────────────────────────────────────

#[test]
fn test_sep_while_at_least_empty_input() {
  let r: Result<Vec<i64>, WhileError> = Parser::new().apply(parse_while_at_least_2).parse_str("+");
  assert!(r.is_err());
}

#[test]
fn test_sep_while_bounded_exactly_min() {
  let r: Result<Vec<i64>, WhileError> = Parser::new().apply(parse_while_bounded).parse_str("1,2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_sep_while_bounded_exactly_max() {
  let r: Result<Vec<i64>, WhileError> = Parser::new()
    .apply(parse_while_bounded)
    .parse_str("1,2,3,4+");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

// ── Handler error path: trailing separator with count modifiers ──────────

#[test]
fn test_sep_while_allow_trailing_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing)
    .parse_str("42+")
    .unwrap();
  assert_eq!(r, vec![42]);
}

#[test]
fn test_sep_while_allow_trailing_single_with_trailing() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing)
    .parse_str("42,+")
    .unwrap();
  assert_eq!(r, vec![42]);
}

#[test]
fn test_sep_while_allow_trailing_empty() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing)
    .parse_str("+")
    .unwrap();
  assert_eq!(r, vec![]);
}

#[test]
fn test_sep_while_allow_trailing_only_comma() {
  // Leading comma with allow_trailing = error (unexpected leading sep)
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_trailing)
    .parse_str(",+");
  assert!(r.is_err());
}

#[test]
fn test_sep_while_allow_leading_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading)
    .parse_str("42+")
    .unwrap();
  assert_eq!(r, vec![42]);
}

#[test]
fn test_sep_while_allow_leading_single_with_leading() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading)
    .parse_str(",42+")
    .unwrap();
  assert_eq!(r, vec![42]);
}

#[test]
fn test_sep_while_allow_leading_empty() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading)
    .parse_str("+")
    .unwrap();
  assert_eq!(r, vec![]);
}

#[test]
fn test_sep_while_allow_leading_only_comma() {
  // Trailing comma with allow_leading = emit trailing sep error
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_leading)
    .parse_str(",+");
  // This triggers handle_leading_state → emit_missing_element
  assert!(r.is_err());
}

#[test]
fn test_sep_while_allow_surrounded_empty() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded)
    .parse_str("+")
    .unwrap();
  assert_eq!(r, vec![]);
}

#[test]
fn test_sep_while_allow_surrounded_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded)
    .parse_str("1+")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn test_sep_while_allow_surrounded_only_leading() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded)
    .parse_str(",1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn test_sep_while_allow_surrounded_only_trailing() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded)
    .parse_str("1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn test_sep_while_allow_surrounded_single_with_both() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded)
    .parse_str(",1,+")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn test_sep_while_allow_surrounded_only_comma() {
  // Just a comma, no items: leading handled, no item follows
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_surrounded)
    .parse_str(",+");
  // allow_surrounded with just comma: triggers handle_leading_state
  assert!(r.is_err());
}

// ── Handler paths with at_least/at_most/bounded + trailing/leading ───────

fn parse_while_allow_trailing_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

fn parse_while_plain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WhileError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: WhileEmitter<'inp>,
{
  parse_num_while
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_allow_trailing_bounded_trailing_comma() {
  // "1,2," with allow_trailing bounded(2,4) → ok, trailing sep handled
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing_bounded)
    .parse_str("1,2,+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn test_sep_while_allow_trailing_bounded_max_with_trailing() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing_bounded)
    .parse_str("1,2,3,4,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn test_sep_while_allow_leading_bounded_leading_comma() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading_bounded)
    .parse_str(",1,2+")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn test_sep_while_allow_leading_at_most_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading_at_most_2)
    .parse_str(",1+")
    .unwrap();
  assert_eq!(r, vec![1]);
}

#[test]
fn test_sep_while_allow_surrounded_bounded_max_with_both() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded_bounded)
    .parse_str(",1,2,3,4,+")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4]);
}

#[test]
fn test_sep_while_allow_surrounded_at_most_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded_at_most_2)
    .parse_str(",1,+")
    .unwrap();
  assert_eq!(r, vec![1]);
}

// ── Plain parser edge cases ─────────────────────────────────────────────

#[test]
fn test_sep_while_plain_empty() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_plain)
    .parse_str("+")
    .unwrap();
  assert_eq!(r, vec![]);
}

#[test]
fn test_sep_while_plain_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_plain)
    .parse_str("42+")
    .unwrap();
  assert_eq!(r, vec![42]);
}

#[test]
fn test_sep_while_plain_trailing_comma_err() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_while_plain).parse_str("1,2,+");
  // Plain parser: trailing comma triggers emit_missing_element
  assert!(r.is_err());
}

#[test]
fn test_sep_while_plain_leading_comma_err() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_while_plain).parse_str(",1+");
  // Plain: leading comma triggers emit_unexpected_leading_separator
  assert!(r.is_err());
}

// ── Zero-width element guard ──────────────────────────────────────────────────
//
// A `Verbose` (recording, non-fatal) emitter keeps the drive going past the missing-separator
// diagnostics a would-be livelock emits on every repeated cycle, so the oracles below can tell
// an unguarded loop (runs the element's full budget, re-entering the missing-separator path on
// every cycle past the first) apart from a guarded one (stops after the first accept, never
// re-entering it).

fn verbose_while_ctx() -> ParserContext<'static, TestLexer<'static>, Verbose<WhileError>> {
  ParserContext::new(Verbose::new())
}

#[test]
fn test_sep_while_zero_width_element_stops_after_one_element() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<WhileError>>,
    >,
  ) -> Result<Vec<i64>, WhileError> {
    // Continues unconditionally, up to a budget, ignoring the peeked window entirely: the
    // element it drives never consumes, so an unguarded loop would otherwise run forever.
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Verbose<WhileError>>|
          -> Result<Action, WhileError> {
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
      ParserContext<'inp, TestLexer<'inp>, Verbose<WhileError>>,
    >|
     -> Result<i64, WhileError> { Ok(0) };
    let out = elem
      .separated_by_comma_while::<_, U1>(cond)
      .collect()
      .parse_input(inp)?;
    assert_eq!(
      inp.emitter_ref().errors().len(),
      0,
      "a single accepted element must not re-enter the missing-separator path"
    );
    Ok(out)
  }

  let r: Vec<i64> = Parser::with_context(verbose_while_ctx())
    .apply(parse)
    .parse_str("1+")
    .unwrap();
  assert_eq!(r.len(), 1, "zero-width guard must stop after one element");
}

#[test]
fn test_sep_while_zero_width_element_with_leading_trivia_stops_after_one_element() {
  // Trivia-gap twin of the above: the leading space in `" 1+"` is skipped trivia, so the driver's
  // first decision peek jumps the cache-front cursor from the committed start to the token start
  // without consuming anything. A cursor-keyed guard reads that as progress and runs one extra
  // cycle — pushing a phantom element and, in `State::Element`, emitting a spurious missing
  // separator. The committed-consumption metric stops after the first element with no diagnostic.
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<WhileError>>,
    >,
  ) -> Result<Vec<i64>, WhileError> {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Verbose<WhileError>>|
          -> Result<Action, WhileError> {
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
      ParserContext<'inp, TestLexer<'inp>, Verbose<WhileError>>,
    >|
     -> Result<i64, WhileError> { Ok(0) };
    let out = elem
      .separated_by_comma_while::<_, U1>(cond)
      .collect()
      .parse_input(inp)?;
    assert_eq!(
      inp.emitter_ref().errors().len(),
      0,
      "a single accepted element behind leading trivia must not re-enter the missing-separator path"
    );
    Ok(out)
  }

  let r: Vec<i64> = Parser::with_context(verbose_while_ctx())
    .apply(parse)
    .parse_str(" 1+")
    .unwrap();
  assert_eq!(
    r.len(),
    1,
    "the committed-progress guard must stop after one element despite the leading trivia gap"
  );
}
