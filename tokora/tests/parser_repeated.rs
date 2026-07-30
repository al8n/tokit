#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]

//! Tests for plain `Repeated` (TryParseInput::repeated()) WITHOUT delimiters.
//!
//! Covers `parser/many/repeated/` (unbounded, at_least, at_most, bounded)
//! and `parser/many/handler/` (maximum, minimum, bounded) via the
//! RepeatedHandler trait.

mod common;

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  emitter::{FullContainerEmitter, TooFewEmitter, TooManyEmitter},
  error::{
    UnexpectedEot,
    syntax::{FullContainer, TooFew, TooMany},
    token::{UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  span::Spanned,
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

// -- Error type ---------------------------------------------------------------

#[derive(Debug)]
struct RPError;

impl From<()> for RPError {
  fn from(_: ()) -> Self {
    RPError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for RPError {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    RPError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for RPError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    RPError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for RPError {
  fn from(_: TooFew<S, Lang>) -> Self {
    RPError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for RPError {
  fn from(_: TooMany<S, Lang>) -> Self {
    RPError
  }
}

impl From<UnexpectedEot> for RPError {
  fn from(_: UnexpectedEot) -> Self {
    RPError
  }
}

// -- Custom emitter -----------------------------------------------------------

struct RPEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for RPEmitter {
  type Error = RPError;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), RPError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RPError)
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), RPError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RPError)
  }

  fn emit_error(
    &mut self,
    err: Spanned<RPError, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RPError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(err.into_data())
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, _: u64)
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
  }
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for RPEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RPError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RPError)
  }
}

impl<'inp> TooFewEmitter<'inp, TestLexer<'inp>> for RPEmitter {
  fn emit_too_few(
    &mut self,
    _: TooFew<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RPError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RPError)
  }
}

impl<'inp> TooManyEmitter<'inp, TestLexer<'inp>> for RPEmitter {
  fn emit_too_many(
    &mut self,
    _: TooMany<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RPError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RPError)
  }
}

fn rp_ctx() -> ParserContext<'static, TestLexer<'static>, RPEmitter> {
  ParserContext::new(RPEmitter)
}

// -- Element parser (TryParseInput) -------------------------------------------

fn try_num_rp<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>,
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

// == 1. Plain repeated (unbounded) ============================================

fn parse_rp_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = RPError> + FullContainerEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().collect().parse_input(inp)
}

#[test]
fn test_repeated_unbounded_basic() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_list)
    .parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_unbounded_single() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_list)
    .parse_str("42");
  assert_eq!(r.unwrap(), vec![42]);
}

#[test]
fn test_repeated_unbounded_empty() {
  // No matching tokens: should produce empty vec
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_list)
    .parse_str("+");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_repeated_unbounded_five_elements() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_list)
    .parse_str("10 20 30 40 50");
  assert_eq!(r.unwrap(), vec![10, 20, 30, 40, 50]);
}

// == 2. at_least ==============================================================

fn parse_rp_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().at_least(2).collect().parse_input(inp)
}

#[test]
fn test_repeated_at_least_ok() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_2)
    .parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_at_least_exact_min() {
  // Exactly 2 elements with at_least(2) should succeed
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_2)
    .parse_str("1 2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_at_least_fail() {
  // Only 1 element with at_least(2) should fail
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_2)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn test_repeated_at_least_zero_elements_fail() {
  // 0 elements with at_least(2) should fail
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_2)
    .parse_str("+");
  assert!(r.is_err());
}

fn parse_rp_at_least_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().at_least(1).collect().parse_input(inp)
}

#[test]
fn test_repeated_at_least_1_single() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_1)
    .parse_str("99");
  assert_eq!(r.unwrap(), vec![99]);
}

#[test]
fn test_repeated_at_least_1_zero_fail() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_1)
    .parse_str("+");
  assert!(r.is_err());
}

// == 3. at_most ===============================================================

fn parse_rp_at_most_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().at_most(3).collect().parse_input(inp)
}

#[test]
fn test_repeated_at_most_ok() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_3)
    .parse_str("1 2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_at_most_exact_max() {
  // Exactly 3 elements with at_most(3) should succeed
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_3)
    .parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_at_most_exceeded() {
  // 4 elements with at_most(3) should trigger too_many error
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_3)
    .parse_str("1 2 3 4");
  assert!(r.is_err());
}

#[test]
fn test_repeated_at_most_single() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_3)
    .parse_str("5");
  assert_eq!(r.unwrap(), vec![5]);
}

#[test]
fn test_repeated_at_most_empty() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_3)
    .parse_str("+");
  assert_eq!(r.unwrap(), vec![]);
}

fn parse_rp_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().at_most(1).collect().parse_input(inp)
}

#[test]
fn test_repeated_at_most_1_single() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_1)
    .parse_str("7");
  assert_eq!(r.unwrap(), vec![7]);
}

#[test]
fn test_repeated_at_most_1_exceeded() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_1)
    .parse_str("7 8");
  assert!(r.is_err());
}

// == 4. bounded ===============================================================

fn parse_rp_bounded_2_4<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp
    .repeated()
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_bounded_ok_middle() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_2_4)
    .parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_bounded_exact_min() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_2_4)
    .parse_str("1 2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_bounded_exact_max() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_2_4)
    .parse_str("1 2 3 4");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn test_repeated_bounded_too_few() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_2_4)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn test_repeated_bounded_too_many() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_2_4)
    .parse_str("1 2 3 4 5");
  assert!(r.is_err());
}

#[test]
fn test_repeated_bounded_zero_elements_fail() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_2_4)
    .parse_str("+");
  assert!(r.is_err());
}

// == 5. bounded via at_most().at_least() chain ================================

fn parse_rp_at_most_then_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp
    .repeated()
    .at_most(4)
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_at_most_then_at_least_ok() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_then_at_least)
    .parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_at_most_then_at_least_too_few() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_then_at_least)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn test_repeated_at_most_then_at_least_too_many() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_then_at_least)
    .parse_str("1 2 3 4 5");
  assert!(r.is_err());
}

// == 6. bounded via at_least().at_most() chain ================================

fn parse_rp_at_least_then_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp
    .repeated()
    .at_least(2)
    .at_most(4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_at_least_then_at_most_ok() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_then_at_most)
    .parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_at_least_then_at_most_too_few() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_then_at_most)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn test_repeated_at_least_then_at_most_too_many() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_then_at_most)
    .parse_str("1 2 3 4 5");
  assert!(r.is_err());
}

#[test]
fn test_repeated_at_least_then_at_most_exact_min() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_then_at_most)
    .parse_str("1 2");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_at_least_then_at_most_exact_max() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_then_at_most)
    .parse_str("1 2 3 4");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

// == 7. bounded with tight range (same min and max) ===========================

fn parse_rp_bounded_exact_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp
    .repeated()
    .bounded(3, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_bounded_exact_ok() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_exact_3)
    .parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_bounded_exact_too_few() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_exact_3)
    .parse_str("1 2");
  assert!(r.is_err());
}

#[test]
fn test_repeated_bounded_exact_too_many() {
  let r: Result<Vec<i64>, RPError> = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_exact_3)
    .parse_str("1 2 3 4");
  assert!(r.is_err());
}

// == 8. Spanned output variants ===============================================

fn parse_rp_bounded_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp
    .repeated()
    .bounded(1, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_bounded_spanned_ok() {
  let r = Parser::with_context(rp_ctx())
    .apply(parse_rp_bounded_spanned)
    .parse_str("10 20");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![10, 20]);
}

fn parse_rp_at_least_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().at_least(1).collect().parse_input(inp)
}

#[test]
fn test_repeated_at_least_spanned_ok() {
  let r = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_least_spanned)
    .parse_str("5 6 7");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![5, 6, 7]);
}

fn parse_rp_at_most_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RPError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().at_most(3).collect().parse_input(inp)
}

#[test]
fn test_repeated_at_most_spanned_ok() {
  let r = Parser::with_context(rp_ctx())
    .apply(parse_rp_at_most_spanned)
    .parse_str("8 9");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![8, 9]);
}

fn parse_rp_unbounded_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RPError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = RPError> + FullContainerEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rp.repeated().collect().parse_input(inp)
}

#[test]
fn test_repeated_unbounded_spanned_ok() {
  let r = Parser::with_context(rp_ctx())
    .apply(parse_rp_unbounded_spanned)
    .parse_str("1 2 3");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![1, 2, 3]);
}

#[test]
fn zero_width_peeking_element_with_leading_trivia_stops_after_one_push() {
  // Trivia-gap twin for the plain `repeated` no-progress guard: the element peeks the frontier
  // (moving the cache-front cursor across the leading trivia in `" 1"`) but consumes nothing, then
  // accepts. A cursor-keyed guard reads that jump as progress and pushes a phantom second element;
  // the committed-consumption metric stops after the first push.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Vec<i64>, RPError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = RPError> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    let elem =
      |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<i64>, RPError> {
        let _ = inp.peek_one()?;
        Ok(ParseAttempt::Accept(1))
      };
    elem.repeated().collect().parse_input(inp)
  }
  let r: Vec<i64> = Parser::with_context(rp_ctx())
    .apply(parse)
    .parse_str(" 1")
    .unwrap();
  assert_eq!(
    r.len(),
    1,
    "the committed-progress guard must stop after one push despite the leading trivia gap"
  );
}
