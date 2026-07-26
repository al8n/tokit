#![cfg(all(feature = "std", feature = "logos"))]

//! Tests for delimited separator combinators.
//!
//! Covers `sep/delim/` (TryParseInput-based) and `sep_while/delim/`
//! (ParseInput + Decision-based) with all policy variants and count modifiers.

mod common;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  TryParseInput,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
  punct::Bracket,
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct DelimError;

impl From<()> for DelimError {
  fn from(_: ()) -> Self {
    DelimError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for DelimError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    DelimError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for DelimError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    DelimError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for DelimError {
  fn from(_: TooFew<S, Lang>) -> Self {
    DelimError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for DelimError {
  fn from(_: TooMany<S, Lang>) -> Self {
    DelimError
  }
}

// Required by the `From<UnexpectedEot<L::Offset, Lang>>` bound on delimited parsers.
impl From<UnexpectedEot> for DelimError {
  fn from(_: UnexpectedEot) -> Self {
    DelimError
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for DelimError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    DelimError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for DelimError
{
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    DelimError
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for DelimError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    DelimError
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for DelimError {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    DelimError
  }
}

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<DelimError>> {
  ParserContext::new(Fatal::new())
}

// ── Supertrait aliases ────────────────────────────────────────────────────────

trait DelimEmitter<'inp>:
  Emitter<'inp, TestLexer<'inp>, Error = DelimError>
  + SeparatedEmitter<'inp, TestLexer<'inp>>
  + FullContainerEmitter<'inp, TestLexer<'inp>>
  + UnclosedEmitter<'inp, TestLexer<'inp>>
  + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
  + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
{
}

impl<'inp, E> DelimEmitter<'inp> for E where
  E: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
{
}

// ── sep/delim element parser (TryParseInput) ──────────────────────────────────

fn try_num_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>,
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

// ── sep_while/delim element parser + condition (ParseInput) ───────────────────

fn parse_num_while_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>,
{
  match inp.next()? {
    None => Err(DelimError),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(DelimError),
    },
  }
}

fn decide_num_delim<'inp, Ctx>(
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

// ═══════════════════════════════════════════════════════════════════════════════
// sep/delim/ tests
// ═══════════════════════════════════════════════════════════════════════════════

// ── 1. Plain (unbounded) ──────────────────────────────────────────────────────

fn parse_plain_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: DelimEmitter<'inp>,
{
  try_num_delim
    .separated_by_comma()
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_plain_basic() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_plain_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_delim_plain_empty() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_plain_delim)
    .parse_str("[]")
    .unwrap();
  assert_eq!(r, vec![]);
}

#[test]
fn test_sep_delim_plain_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_plain_delim)
    .parse_str("[42]")
    .unwrap();
  assert_eq!(r, vec![42]);
}

// ── 2. Plain at_least ─────────────────────────────────────────────────────────

fn parse_plain_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: DelimEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_plain_delim_at_least)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// Note: at_least minimum is only enforced when the element parser declines mid-parse,
// not when the close delimiter is encountered. So [1] succeeds even with at_least(2).

// ── 3. Plain at_most ──────────────────────────────────────────────────────────

fn parse_plain_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: DelimEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .at_most(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_plain_delim_at_most)
    .parse_str("[1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn test_sep_delim_at_most_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_plain_delim_at_most)
    .parse_str("[7]")
    .unwrap();
  assert_eq!(r, vec![7]);
}

// ── 5. allow_trailing (unbounded) ─────────────────────────────────────────────

fn parse_allow_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_trailing_with_trailing() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_trailing_delim)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_delim_allow_trailing_without_trailing() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_trailing_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 6. allow_trailing at_least ────────────────────────────────────────────────

fn parse_allow_trailing_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_trailing_delim_at_least)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 7. allow_trailing at_most ─────────────────────────────────────────────────

fn parse_allow_trailing_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_trailing_delim_at_most)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 8. allow_trailing bounded ─────────────────────────────────────────────────

fn parse_allow_trailing_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .bounded(1, 3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_trailing_delim_bounded)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 9. allow_leading (unbounded) ──────────────────────────────────────────────

fn parse_allow_leading_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_with_leading() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_leading_delim)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_delim_allow_leading_without_leading() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_leading_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 10. allow_leading at_least ────────────────────────────────────────────────

fn parse_allow_leading_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_leading()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_leading_delim_at_least)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 11. allow_leading at_most ─────────────────────────────────────────────────

fn parse_allow_leading_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_leading()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_leading_delim_at_most)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 12. allow_leading bounded ─────────────────────────────────────────────────

fn parse_allow_leading_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_leading()
    .bounded(1, 3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_leading_delim_bounded)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 13. allow_surrounded (unbounded) ─────────────────────────────────────────

fn parse_allow_surrounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_surrounded_both() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_surrounded_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_delim_allow_surrounded_none() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_surrounded_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 14. allow_surrounded at_least ────────────────────────────────────────────
// Chain: .allow_trailing().at_least(2).allow_leading()
// Type:  AllowLeading<AllowTrailing<AtLeast<Separated<...>>>>

fn parse_allow_surrounded_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .at_least(2)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_surrounded_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_surrounded_delim_at_least)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 15. allow_surrounded at_most ─────────────────────────────────────────────

fn parse_allow_surrounded_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .at_most(3)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_surrounded_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_surrounded_delim_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 16. allow_surrounded bounded ─────────────────────────────────────────────

fn parse_allow_surrounded_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .bounded(1, 3)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_surrounded_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_allow_surrounded_delim_bounded)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 17. require_trailing (unbounded) ─────────────────────────────────────────

fn parse_require_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_trailing_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_delim)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// Note: require_trailing in sep/delim only enforces trailing separator when parsing
// ends prematurely (element parser declines), not at the close delimiter.

// ── 18. require_trailing at_least ────────────────────────────────────────────

fn parse_require_trailing_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_delim_at_least)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 19. require_leading (unbounded) ──────────────────────────────────────────

fn parse_require_leading_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_delim_require_leading_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim)
    .parse_str("[1,2,3]");
  assert!(r.is_err());
}

// ── 20. require_leading at_least ─────────────────────────────────────────────

fn parse_require_leading_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_leading()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim_at_least)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 21. allow_leading_require_trailing (unbounded) ───────────────────────────
// Chain: .require_trailing().allow_leading()
// Type:  AllowLeading<RequireTrailing<Separated<...>>>

fn parse_allow_leading_require_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_require_trailing_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 22. require_leading_allow_trailing (unbounded) ───────────────────────────
// Chain: .allow_trailing().require_leading()
// Type:  RequireLeading<AllowTrailing<Separated<...>>>

fn parse_require_leading_allow_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_allow_trailing_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 23. require_surrounded (unbounded) ───────────────────────────────────────
// Chain: .require_trailing().require_leading()
// Type:  RequireLeading<RequireTrailing<Separated<...>>>

fn parse_require_surrounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_surrounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 24. require_trailing at_most ─────────────────────────────────────────────
// Type: RequireTrailing<AtMost<Separated<...>>>

fn parse_require_trailing_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_delim_at_most)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 25. require_trailing bounded ─────────────────────────────────────────────
// Type: RequireTrailing<Bounded<Separated<...>>>

fn parse_require_trailing_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .bounded(1, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_delim_bounded)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 26. require_leading at_most ───────────────────────────────────────────────
// Type: RequireLeading<AtMost<Separated<...>>>

fn parse_require_leading_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_leading()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim_at_most)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 27. require_leading bounded ───────────────────────────────────────────────
// Type: RequireLeading<Bounded<Separated<...>>>

fn parse_require_leading_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_leading()
    .bounded(1, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim_bounded)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 28. allow_leading_require_trailing at_least ───────────────────────────────
// Chain: .require_trailing().at_least(N).allow_leading()
// Type:  AllowLeading<RequireTrailing<AtLeast<Separated<...>>>>

fn parse_allow_leading_require_trailing_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .at_least(2)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_require_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_delim_at_least)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 29. allow_leading_require_trailing at_most ────────────────────────────────
// Chain: .require_trailing().at_most(N).allow_leading()
// Type:  AllowLeading<RequireTrailing<AtMost<Separated<...>>>>

fn parse_allow_leading_require_trailing_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .at_most(3)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_require_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_delim_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 30. allow_leading_require_trailing bounded ────────────────────────────────
// Chain: .require_trailing().bounded(N,M).allow_leading()
// Type:  AllowLeading<RequireTrailing<Bounded<Separated<...>>>>

fn parse_allow_leading_require_trailing_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .bounded(1, 4)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_allow_leading_require_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_delim_bounded)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 31. require_leading_allow_trailing at_least ───────────────────────────────
// Chain: .allow_trailing().at_least(N).require_leading()
// Type:  RequireLeading<AllowTrailing<AtLeast<Separated<...>>>>

fn parse_require_leading_allow_trailing_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .at_least(2)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_allow_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_delim_at_least)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 32. require_leading_allow_trailing at_most ────────────────────────────────
// Chain: .allow_trailing().at_most(N).require_leading()
// Type:  RequireLeading<AllowTrailing<AtMost<Separated<...>>>>

fn parse_require_leading_allow_trailing_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .at_most(3)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_allow_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_delim_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 33. require_leading_allow_trailing bounded ────────────────────────────────
// Chain: .allow_trailing().bounded(N,M).require_leading()
// Type:  RequireLeading<AllowTrailing<Bounded<Separated<...>>>>

fn parse_require_leading_allow_trailing_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .allow_trailing()
    .bounded(1, 4)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_leading_allow_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_delim_bounded)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 34. require_surrounded at_least ──────────────────────────────────────────
// Chain: .require_trailing().at_least(N).require_leading()
// Type:  RequireLeading<RequireTrailing<AtLeast<Separated<...>>>>

fn parse_require_surrounded_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .at_least(2)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_surrounded_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim_at_least)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 35. require_surrounded at_most ───────────────────────────────────────────
// Chain: .require_trailing().at_most(N).require_leading()
// Type:  RequireLeading<RequireTrailing<AtMost<Separated<...>>>>

fn parse_require_surrounded_delim_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .at_most(3)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_surrounded_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim_at_most)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 36. require_surrounded bounded ───────────────────────────────────────────
// Chain: .require_trailing().bounded(N,M).require_leading()
// Type:  RequireLeading<RequireTrailing<Bounded<Separated<...>>>>

fn parse_require_surrounded_delim_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_delim
    .separated_by_comma()
    .require_trailing()
    .bounded(1, 4)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_delim_require_surrounded_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim_bounded)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// sep_while/delim/ tests
// ═══════════════════════════════════════════════════════════════════════════════
//
// sep_while uses ParseInput (not TryParseInput) + a Decision condition.
// Inputs need a non-number sentinel token before the closing `]` so the
// condition always sees a real token (not EOF) when stopping.
// We use `+` as the sentinel (left unconsumed).

// ── 24. Plain while delim (unbounded) ────────────────────────────────────────

fn parse_while_plain_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: DelimEmitter<'inp>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_plain_basic() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_plain_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_while_delim_plain_empty() {
  // Empty: no elements before `]`
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_plain_delim)
    .parse_str("[]")
    .unwrap();
  assert_eq!(r, vec![]);
}

#[test]
fn test_sep_while_delim_plain_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_plain_delim)
    .parse_str("[42]")
    .unwrap();
  assert_eq!(r, vec![42]);
}

// ── 25. While at_least ───────────────────────────────────────────────────────

fn parse_while_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: DelimEmitter<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_at_least_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_while_delim_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_at_least_delim)
    .parse_str("[1]");
  assert!(r.is_err());
}

// ── 26. While at_most ────────────────────────────────────────────────────────

fn parse_while_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: DelimEmitter<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .at_most(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_at_most_delim)
    .parse_str("[1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

#[test]
fn test_sep_while_delim_at_most_single() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_at_most_delim)
    .parse_str("[7]")
    .unwrap();
  assert_eq!(r, vec![7]);
}

// ── 27. While bounded ────────────────────────────────────────────────────────

fn parse_while_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: DelimEmitter<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .bounded(2, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_bounded_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}
#[test]
fn test_sep_while_delim_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_bounded_delim)
    .parse_str("[1,2,3,4,5]");
  assert!(r.is_err());
}

// Note: sep_while/delim bounded only enforces the maximum, not the minimum, at the close delimiter.

// ── 28. While allow_trailing delim ───────────────────────────────────────────

fn parse_while_allow_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_trailing_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing_delim)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_while_delim_allow_trailing_no_trailing() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing_delim)
    .parse_str("[1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 29. While allow_trailing at_least ────────────────────────────────────────

fn parse_while_allow_trailing_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing_at_least_delim)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 30. While allow_trailing at_most ─────────────────────────────────────────

fn parse_while_allow_trailing_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing_at_most_delim)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 31. While allow_trailing bounded ─────────────────────────────────────────

fn parse_while_allow_trailing_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .bounded(1, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_trailing_bounded_delim)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_allow_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_trailing_bounded_delim)
    .parse_str("[1,2,3,4,5,]");
  assert!(r.is_err());
}

// ── 32. While allow_leading delim ────────────────────────────────────────────

fn parse_while_allow_leading_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading_delim)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 33. While allow_leading at_least ─────────────────────────────────────────

fn parse_while_allow_leading_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_leading()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading_at_least_delim)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 34. While allow_leading at_most ──────────────────────────────────────────

fn parse_while_allow_leading_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_leading()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading_at_most_delim)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 35. While allow_leading bounded ──────────────────────────────────────────

fn parse_while_allow_leading_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_leading()
    .bounded(1, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_leading_bounded_delim)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_allow_leading_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_leading_bounded_delim)
    .parse_str("[,1,2,3,4,5]");
  assert!(r.is_err());
}

// ── 36. While allow_surrounded delim ─────────────────────────────────────────

fn parse_while_allow_surrounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_surrounded_both() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 37. While allow_surrounded at_least ──────────────────────────────────────

fn parse_while_allow_surrounded_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_surrounded_at_least_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded_at_least_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 38. While allow_surrounded at_most ───────────────────────────────────────

fn parse_while_allow_surrounded_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_surrounded_at_most_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded_at_most_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 39. While allow_surrounded bounded ───────────────────────────────────────

fn parse_while_allow_surrounded_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .bounded(1, 4)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_surrounded_bounded_ok() {
  let r: Vec<i64> = Parser::new()
    .apply(parse_while_allow_surrounded_bounded_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_allow_surrounded_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_surrounded_bounded_delim)
    .parse_str("[,1,2,3,4,5,]");
  assert!(r.is_err());
}

// ── 40. While require_trailing delim ─────────────────────────────────────────

fn parse_while_require_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_trailing_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_delim)
    .parse_str("[1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_sep_while_delim_require_trailing_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_delim)
    .parse_str("[1,2,3]");
  assert!(r.is_err());
}

// ── 41. While require_trailing at_least ──────────────────────────────────────

fn parse_while_require_trailing_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_at_least_delim)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 42. While require_trailing at_most ───────────────────────────────────────

fn parse_while_require_trailing_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_at_most_delim)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 43. While require_trailing bounded ───────────────────────────────────────

fn parse_while_require_trailing_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .bounded(1, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_bounded_delim)
    .parse_str("[1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_require_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_bounded_delim)
    .parse_str("[1,2,3,4,5,]");
  assert!(r.is_err());
}

// ── 44. While require_leading delim ──────────────────────────────────────────

fn parse_while_require_leading_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_delim)
    .parse_str("[,1,2,3]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 45. While require_leading at_least ───────────────────────────────────────

fn parse_while_require_leading_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_leading()
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_at_least_delim)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 46. While require_leading at_most ────────────────────────────────────────

fn parse_while_require_leading_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_leading()
    .at_most(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_at_most_delim)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 47. While require_leading bounded ────────────────────────────────────────

fn parse_while_require_leading_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_leading()
    .bounded(1, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_bounded_delim)
    .parse_str("[,1,2]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_require_leading_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_bounded_delim)
    .parse_str("[,1,2,3,4,5]");
  assert!(r.is_err());
}

// ── 48. While allow_leading_require_trailing delim ────────────────────────────
// Chain: .require_trailing().allow_leading()

fn parse_while_allow_leading_require_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 49. While allow_leading_require_trailing at_least ─────────────────────────

fn parse_while_allow_leading_require_trailing_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .at_least(2)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_at_least_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 50. While allow_leading_require_trailing at_most ──────────────────────────

fn parse_while_allow_leading_require_trailing_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .at_most(3)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_at_most_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 51. While allow_leading_require_trailing bounded ──────────────────────────

fn parse_while_allow_leading_require_trailing_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .bounded(1, 4)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_bounded_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_allow_leading_require_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_bounded_delim)
    .parse_str("[1,2,3,4,5,]");
  assert!(r.is_err());
}

// ── 52. While require_leading_allow_trailing delim ────────────────────────────
// Chain: .allow_trailing().require_leading()

fn parse_while_require_leading_allow_trailing_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 53. While require_leading_allow_trailing at_least ─────────────────────────

fn parse_while_require_leading_allow_trailing_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .at_least(2)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_at_least_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 54. While require_leading_allow_trailing at_most ──────────────────────────

fn parse_while_require_leading_allow_trailing_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .at_most(3)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_at_most_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 55. While require_leading_allow_trailing bounded ──────────────────────────

fn parse_while_require_leading_allow_trailing_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .allow_trailing()
    .bounded(1, 4)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_bounded_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_require_leading_allow_trailing_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_bounded_delim)
    .parse_str("[,1,2,3,4,5,]");
  assert!(r.is_err());
}

// ── 56. While require_surrounded delim ───────────────────────────────────────

fn parse_while_require_surrounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_surrounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_delim)
    .parse_str("[,1,2,3,]")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 57. While require_surrounded at_least ────────────────────────────────────

fn parse_while_require_surrounded_at_least_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .at_least(2)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_surrounded_at_least_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_at_least_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 58. While require_surrounded at_most ─────────────────────────────────────

fn parse_while_require_surrounded_at_most_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .at_most(3)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_surrounded_at_most_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_at_most_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 59. While require_surrounded bounded ─────────────────────────────────────

fn parse_while_require_surrounded_bounded_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, DelimError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = DelimError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_while_delim
    .separated_by_comma_while::<_, U1>(decide_num_delim::<Ctx>)
    .require_trailing()
    .bounded(1, 4)
    .require_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_sep_while_delim_require_surrounded_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_bounded_delim)
    .parse_str("[,1,2,]")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}
#[test]
fn test_sep_while_delim_require_surrounded_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_bounded_delim)
    .parse_str("[,1,2,3,4,5,]");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Error-path tests: sep/delim/
// ═══════════════════════════════════════════════════════════════════════════════
//
// Note: sep/delim (TryParseInput) leaves its element loop two ways — the element parser
// declining, and the close delimiter committing mid-scan — and BOTH run the end-state pass,
// so at_most/at_least/bounded and the separator policy are enforced on a properly closed list
// exactly as they are on a truncated one (INVARIANT E: every `Ok`-returning exit runs the
// end-state pass exactly once). This section covers the error shapes that reach the loop's
// other exits: require_leading (missing leading comma), missing delimiters, empty input, etc.
// The count-bound and trailing-policy violations on a closed list are pinned in
// `sep_delim_extra.rs` and, across all eight drivers, in `end_state_parity.rs`.

// ── require_surrounded fail (missing leading) ────────────────────────────────

#[test]
fn test_sep_delim_require_surrounded_fail_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim)
    .parse_str("[1,2,3,]");
  assert!(r.is_err());
}

// ── require_surrounded fail (missing both) ───────────────────────────────────

#[test]
fn test_sep_delim_require_surrounded_fail_missing_both() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim)
    .parse_str("[1,2,3]");
  assert!(r.is_err());
}

// ── require_leading_allow_trailing fail (missing leading) ────────────────────

#[test]
fn test_sep_delim_require_leading_allow_trailing_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_delim)
    .parse_str("[1,2,3,]");
  assert!(r.is_err());
}

// ── Missing close delimiter ──────────────────────────────────────────────────

#[test]
fn test_sep_delim_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_plain_delim).parse_str("[1,2");
  assert!(r.is_err());
}

// ── Missing open delimiter ───────────────────────────────────────────────────

#[test]
fn test_sep_delim_missing_open() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_plain_delim).parse_str("1,2]");
  assert!(r.is_err());
}

// ── Empty input ──────────────────────────────────────────────────────────────

#[test]
fn test_sep_delim_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_plain_delim).parse_str("");
  assert!(r.is_err());
}

// ── allow_trailing missing close ─────────────────────────────────────────────

#[test]
fn test_sep_delim_allow_trailing_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_allow_trailing_delim)
    .parse_str("[1,2,3,");
  assert!(r.is_err());
}

// ── allow_leading missing close ──────────────────────────────────────────────

#[test]
fn test_sep_delim_allow_leading_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_allow_leading_delim)
    .parse_str("[,1,2");
  assert!(r.is_err());
}

// ── require_trailing missing close ───────────────────────────────────────────

#[test]
fn test_sep_delim_require_trailing_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_delim)
    .parse_str("[1,2,3,");
  assert!(r.is_err());
}

// ── require_leading missing close ────────────────────────────────────────────

#[test]
fn test_sep_delim_require_leading_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim)
    .parse_str("[,1,2");
  assert!(r.is_err());
}

// ── require_surrounded missing close ─────────────────────────────────────────

#[test]
fn test_sep_delim_require_surrounded_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim)
    .parse_str("[,1,2,");
  assert!(r.is_err());
}

// ── at_least missing close ───────────────────────────────────────────────────

#[test]
fn test_sep_delim_at_least_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_plain_delim_at_least)
    .parse_str("[1,2");
  assert!(r.is_err());
}

// ── at_most missing close ────────────────────────────────────────────────────

#[test]
fn test_sep_delim_at_most_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_plain_delim_at_most)
    .parse_str("[1,2");
  assert!(r.is_err());
}

// ── require_leading missing open ─────────────────────────────────────────────

#[test]
fn test_sep_delim_require_leading_missing_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim)
    .parse_str(",1,2]");
  assert!(r.is_err());
}

// ── require_trailing missing open ────────────────────────────────────────────

#[test]
fn test_sep_delim_require_trailing_missing_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_delim)
    .parse_str("1,2,]");
  assert!(r.is_err());
}

// ── allow_trailing empty input ───────────────────────────────────────────────

#[test]
fn test_sep_delim_allow_trailing_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_allow_trailing_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── allow_leading empty input ────────────────────────────────────────────────

#[test]
fn test_sep_delim_allow_leading_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_allow_leading_delim).parse_str("");
  assert!(r.is_err());
}

// ── require_trailing empty input ─────────────────────────────────────────────

#[test]
fn test_sep_delim_require_trailing_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_trailing_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── require_leading empty input ──────────────────────────────────────────────

#[test]
fn test_sep_delim_require_leading_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── require_surrounded empty input ───────────────────────────────────────────

#[test]
fn test_sep_delim_require_surrounded_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_surrounded_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── allow_leading_require_trailing empty input ───────────────────────────────

#[test]
fn test_sep_delim_allow_leading_require_trailing_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_allow_leading_require_trailing_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── require_leading_allow_trailing empty input ───────────────────────────────

#[test]
fn test_sep_delim_require_leading_allow_trailing_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_require_leading_allow_trailing_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── allow_surrounded missing open ────────────────────────────────────────────

#[test]
fn test_sep_delim_allow_surrounded_missing_open() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_allow_surrounded_delim)
    .parse_str(",1,2,3,]");
  assert!(r.is_err());
}

// ── allow_surrounded missing close ───────────────────────────────────────────

#[test]
fn test_sep_delim_allow_surrounded_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_allow_surrounded_delim)
    .parse_str("[,1,2,3,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Error-path tests: sep_while/delim/
// ═══════════════════════════════════════════════════════════════════════════════
//
// sep_while/delim (ParseInput + Decision) enforces at_most/too_many at the
// close delimiter. at_least/too_few is also enforced via the decision function.

// ── at_most too many ─────────────────────────────────────────────────────────

#[test]
fn test_sep_while_delim_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_at_most_delim)
    .parse_str("[1,2,3]");
  assert!(r.is_err());
}

// ── allow_trailing at_least fail ─────────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_trailing_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_trailing_at_least_delim)
    .parse_str("[1,]");
  assert!(r.is_err());
}

// ── allow_trailing at_most fail ──────────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_trailing_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_trailing_at_most_delim)
    .parse_str("[1,2,3,4,]");
  assert!(r.is_err());
}

// ── allow_leading at_least fail ──────────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_leading_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_leading_at_least_delim)
    .parse_str("[,1]");
  assert!(r.is_err());
}

// ── allow_leading at_most fail ───────────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_leading_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_leading_at_most_delim)
    .parse_str("[,1,2,3,4]");
  assert!(r.is_err());
}

// ── allow_surrounded at_least fail ───────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_surrounded_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_surrounded_at_least_delim)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ── allow_surrounded at_most fail ────────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_surrounded_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_surrounded_at_most_delim)
    .parse_str("[,1,2,3,4,]");
  assert!(r.is_err());
}

// ── allow_surrounded bounded too few ─────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_surrounded_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_surrounded_bounded_delim)
    .parse_str("[,]");
  assert!(r.is_err());
}

// ── require_trailing at_least fail ───────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_trailing_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_at_least_delim)
    .parse_str("[1,]");
  assert!(r.is_err());
}

// ── require_trailing at_most fail ────────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_trailing_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_at_most_delim)
    .parse_str("[1,2,3,4,]");
  assert!(r.is_err());
}

// ── require_leading fail (missing leading) ───────────────────────────────────

#[test]
fn test_sep_while_delim_require_leading_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_delim)
    .parse_str("[1,2,3]");
  assert!(r.is_err());
}

// ── require_leading at_least fail ────────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_leading_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_at_least_delim)
    .parse_str("[,1]");
  assert!(r.is_err());
}

// ── require_leading at_most fail ─────────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_leading_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_at_most_delim)
    .parse_str("[,1,2,3,4]");
  assert!(r.is_err());
}

// ── require_leading bounded too few ──────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_leading_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_bounded_delim)
    .parse_str("[,]");
  assert!(r.is_err());
}

// ── require_surrounded fail (missing leading) ────────────────────────────────

#[test]
fn test_sep_while_delim_require_surrounded_fail_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_delim)
    .parse_str("[1,2,3,]");
  assert!(r.is_err());
}

// ── require_surrounded fail (missing trailing) ───────────────────────────────

#[test]
fn test_sep_while_delim_require_surrounded_fail_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_delim)
    .parse_str("[,1,2,3]");
  assert!(r.is_err());
}

// ── require_surrounded fail (missing both) ───────────────────────────────────

#[test]
fn test_sep_while_delim_require_surrounded_fail_missing_both() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_delim)
    .parse_str("[1,2,3]");
  assert!(r.is_err());
}

// ── require_surrounded at_least fail ─────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_surrounded_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_at_least_delim)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ── require_surrounded at_most fail ──────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_surrounded_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_at_most_delim)
    .parse_str("[,1,2,3,4,]");
  assert!(r.is_err());
}

// ── require_surrounded bounded too few ───────────────────────────────────────

#[test]
fn test_sep_while_delim_require_surrounded_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_surrounded_bounded_delim)
    .parse_str("[,,]");
  assert!(r.is_err());
}

// ── allow_leading_require_trailing fail (missing trailing) ───────────────────

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_delim)
    .parse_str("[,1,2,3]");
  assert!(r.is_err());
}

// ── allow_leading_require_trailing at_least fail ─────────────────────────────

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_at_least_delim)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ── allow_leading_require_trailing at_most fail ──────────────────────────────

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_at_most_delim)
    .parse_str("[,1,2,3,4,]");
  assert!(r.is_err());
}

// ── allow_leading_require_trailing bounded too few ───────────────────────────

#[test]
fn test_sep_while_delim_allow_leading_require_trailing_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_allow_leading_require_trailing_bounded_delim)
    .parse_str("[,]");
  assert!(r.is_err());
}

// ── require_leading_allow_trailing fail (missing leading) ────────────────────

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_delim)
    .parse_str("[1,2,3,]");
  assert!(r.is_err());
}

// ── require_leading_allow_trailing at_least fail ─────────────────────────────

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_at_least_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_at_least_delim)
    .parse_str("[,1,]");
  assert!(r.is_err());
}

// ── require_leading_allow_trailing at_most fail ──────────────────────────────

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_at_most_fail() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_at_most_delim)
    .parse_str("[,1,2,3,4,]");
  assert!(r.is_err());
}

// ── require_leading_allow_trailing bounded too few ───────────────────────────

#[test]
fn test_sep_while_delim_require_leading_allow_trailing_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_allow_trailing_bounded_delim)
    .parse_str("[,]");
  assert!(r.is_err());
}

// ── Missing open delimiter (sep_while) ───────────────────────────────────────

#[test]
fn test_sep_while_delim_missing_open() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_plain_delim)
    .parse_str("1,2]");
  assert!(r.is_err());
}

// ── Empty input (sep_while) ──────────────────────────────────────────────────

#[test]
fn test_sep_while_delim_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::new().apply(parse_while_plain_delim).parse_str("");
  assert!(r.is_err());
}

// ── require_trailing empty bracket ───────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_trailing_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_trailing_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── require_leading empty bracket ────────────────────────────────────────────

#[test]
fn test_sep_while_delim_require_leading_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(full_ctx())
    .apply(parse_while_require_leading_delim)
    .parse_str("");
  assert!(r.is_err());
}

// ── allow_trailing missing open ──────────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_trailing_missing_open() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_trailing_delim)
    .parse_str("1,2,]");
  assert!(r.is_err());
}

// ── allow_leading missing open ───────────────────────────────────────────────

#[test]
fn test_sep_while_delim_allow_leading_missing_open() {
  let r: Result<Vec<i64>, _> = Parser::new()
    .apply(parse_while_allow_leading_delim)
    .parse_str(",1,2]");
  assert!(r.is_err());
}
