#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]

//! Tests for `TryParseInput::repeated().delimited()` (repeated parser without
//! separator, wrapped in delimiters).
//!
//! Covers `parser/many/delim/repeated/` (unbounded, at_least, at_most, bounded).

mod common;

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  emitter::{FullContainerEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter, Verbose},
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, TooFew, TooMany},
    token::{UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  punct::Bracket,
  span::Spanned,
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

// -- Error type ---------------------------------------------------------------

#[derive(Debug)]
struct RDError;

impl From<()> for RDError {
  fn from(_: ()) -> Self {
    RDError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for RDError {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    RDError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for RDError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    RDError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for RDError {
  fn from(_: TooFew<S, Lang>) -> Self {
    RDError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for RDError {
  fn from(_: TooMany<S, Lang>) -> Self {
    RDError
  }
}

impl From<UnexpectedEot> for RDError {
  fn from(_: UnexpectedEot) -> Self {
    RDError
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for RDError {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    RDError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for RDError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    RDError
  }
}

// -- Custom emitter -----------------------------------------------------------

struct RDEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for RDEmitter {
  type Error = RDError;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), RDError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RDError)
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), RDError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RDError)
  }

  fn emit_error(
    &mut self,
    err: Spanned<RDError, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RDError>
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

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for RDEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RDError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RDError)
  }
}

impl<'inp> TooFewEmitter<'inp, TestLexer<'inp>> for RDEmitter {
  fn emit_too_few(
    &mut self,
    _: TooFew<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RDError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RDError)
  }
}

impl<'inp> TooManyEmitter<'inp, TestLexer<'inp>> for RDEmitter {
  fn emit_too_many(
    &mut self,
    _: TooMany<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RDError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RDError)
  }
}

impl<'inp> UnclosedEmitter<'inp, TestLexer<'inp>> for RDEmitter {
  fn emit_unclosed<Delimiter>(
    &mut self,
    _: Unclosed<Delimiter, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RDError>
  where
    TestLexer<'inp>: Lexer<'inp>,
    RDError: tokora::emitter::FromUnclosed<'inp, TestLexer<'inp>>,
  {
    Err(RDError)
  }
}

fn rd_ctx() -> ParserContext<'static, TestLexer<'static>, RDEmitter> {
  ParserContext::new(RDEmitter)
}

// -- Element parser (TryParseInput) -------------------------------------------

fn try_num_rd<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>,
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

// == 1. Plain repeated delimited (unbounded) ==================================

fn parse_rd_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_basic() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_list)
    .parse_str("[1 2 3]");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_delimited_empty() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_list)
    .parse_str("[]");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_repeated_delimited_single() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_list)
    .parse_str("[42]");
  assert_eq!(r.unwrap(), vec![42]);
}

// == 2. at_least ==============================================================

fn parse_rd_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .at_least(2)
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_at_least_ok() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_2)
    .parse_str("[1 2 3]");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_delimited_at_least_fail() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_2)
    .parse_str("[1]");
  assert!(r.is_err());
}

// == 3. at_most ===============================================================

fn parse_rd_at_most_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .at_most(3)
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_at_most_ok() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_3)
    .parse_str("[1 2]");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_delimited_at_most_single() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_3)
    .parse_str("[5]");
  assert_eq!(r.unwrap(), vec![5]);
}

// == 4. bounded ===============================================================

fn parse_rd_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .bounded(2, 4)
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_bounded_ok() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded)
    .parse_str("[1 2 3]");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_delimited_bounded_too_few() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded)
    .parse_str("[1]");
  assert!(r.is_err());
}

// == 5. Error: missing open bracket ==========================================

#[test]
fn test_repeated_delimited_missing_open() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_list)
    .parse_str("1 2 3]");
  assert!(r.is_err());
}

// == 6. Error: wrong close token (not a bracket) =============================

#[test]
fn test_repeated_delimited_wrong_close() {
  // After parsing elements, the parser expects `]` but sees `+` instead.
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_list)
    .parse_str("[1 2 3 +");
  assert!(r.is_err());
}

// == 7. at_most exceeded (too many) ===========================================

#[test]
fn test_repeated_delimited_at_most_exceeded() {
  // 4 elements with at_most(3) should trigger too_many
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_3)
    .parse_str("[1 2 3 4]");
  assert!(r.is_err());
}

#[test]
fn test_repeated_delimited_at_most_exact_max() {
  // Exactly 3 elements with at_most(3) should succeed
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_3)
    .parse_str("[1 2 3]");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_delimited_at_most_empty() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_3)
    .parse_str("[]");
  assert_eq!(r.unwrap(), vec![]);
}

// == 8. at_least exact boundary ===============================================

#[test]
fn test_repeated_delimited_at_least_exact_min() {
  // Exactly 2 with at_least(2) should succeed
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_2)
    .parse_str("[1 2]");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_delimited_at_least_zero_fail() {
  // 0 elements with at_least(2) should fail
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_2)
    .parse_str("[]");
  assert!(r.is_err());
}

// == 9. bounded too many ======================================================

#[test]
fn test_repeated_delimited_bounded_too_many() {
  // 5 elements with bounded(2, 4) should fail
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded)
    .parse_str("[1 2 3 4 5]");
  assert!(r.is_err());
}

#[test]
fn test_repeated_delimited_bounded_exact_min() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded)
    .parse_str("[1 2]");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_delimited_bounded_exact_max() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded)
    .parse_str("[1 2 3 4]");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn test_repeated_delimited_bounded_zero_fail() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded)
    .parse_str("[]");
  assert!(r.is_err());
}

// == 10. bounded with tight range (same min and max) ==========================

fn parse_rd_bounded_exact_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .bounded(3, 3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_bounded_exact_ok() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded_exact_3)
    .parse_str("[1 2 3]");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_delimited_bounded_exact_too_few() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded_exact_3)
    .parse_str("[1 2]");
  assert!(r.is_err());
}

#[test]
fn test_repeated_delimited_bounded_exact_too_many() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_bounded_exact_3)
    .parse_str("[1 2 3 4]");
  assert!(r.is_err());
}

// == 11. bounded via at_most().at_least() chain ===============================

fn parse_rd_at_most_then_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .at_most(4)
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_at_most_then_at_least_ok() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_then_at_least)
    .parse_str("[1 2 3]");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_delimited_at_most_then_at_least_too_few() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_then_at_least)
    .parse_str("[1]");
  assert!(r.is_err());
}

#[test]
fn test_repeated_delimited_at_most_then_at_least_too_many() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_then_at_least)
    .parse_str("[1 2 3 4 5]");
  assert!(r.is_err());
}

// == 12. bounded via at_least().at_most() chain ===============================

fn parse_rd_at_least_then_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .at_least(2)
    .at_most(4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_at_least_then_at_most_ok() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_then_at_most)
    .parse_str("[1 2 3]");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_delimited_at_least_then_at_most_too_few() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_then_at_most)
    .parse_str("[1]");
  assert!(r.is_err());
}

#[test]
fn test_repeated_delimited_at_least_then_at_most_too_many() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_then_at_most)
    .parse_str("[1 2 3 4 5]");
  assert!(r.is_err());
}

#[test]
fn test_repeated_delimited_at_least_then_at_most_exact_min() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_then_at_most)
    .parse_str("[1 2]");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_delimited_at_least_then_at_most_exact_max() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_then_at_most)
    .parse_str("[1 2 3 4]");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

// == 13. at_most(1) boundary ==================================================

fn parse_rd_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .at_most(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_at_most_1_single() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_1)
    .parse_str("[7]");
  assert_eq!(r.unwrap(), vec![7]);
}

#[test]
fn test_repeated_delimited_at_most_1_exceeded() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_1)
    .parse_str("[7 8]");
  assert!(r.is_err());
}

#[test]
fn test_repeated_delimited_at_most_1_empty() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_most_1)
    .parse_str("[]");
  assert_eq!(r.unwrap(), vec![]);
}

// == 14. at_least(1) boundary =================================================

fn parse_rd_at_least_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RDError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RDError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num_rd
    .repeated()
    .at_least(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_delimited_at_least_1_ok() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_1)
    .parse_str("[99]");
  assert_eq!(r.unwrap(), vec![99]);
}

#[test]
fn test_repeated_delimited_at_least_1_zero_fail() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_at_least_1)
    .parse_str("[]");
  assert!(r.is_err());
}

// == 15. Five elements unbounded ==============================================

#[test]
fn test_repeated_delimited_five_elements() {
  let r: Result<Vec<i64>, RDError> = Parser::with_context(rd_ctx())
    .apply(parse_rd_list)
    .parse_str("[10 20 30 40 50]");
  assert_eq!(r.unwrap(), vec![10, 20, 30, 40, 50]);
}

// == 16. Committed-progress guard: leading-trivia twin ========================

#[test]
fn zero_width_peeking_element_with_leading_trivia_stops_after_one_push() {
  // The space after the opener in `"[ 1]"` is the trivia gap. The element peeks the frontier
  // (moving the cache-front cursor across it) but consumes nothing, then accepts. A cursor-keyed
  // guard reads that jump as progress and pushes a phantom second element; the committed metric
  // stops after the first. A `Verbose` emitter keeps the drive past the epilogue's close-position
  // diagnostic so the element count is observable.
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<RDError>>,
    >,
  ) -> Result<Vec<i64>, RDError> {
    let elem = |inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<RDError>>,
    >|
     -> Result<ParseAttempt<i64>, RDError> {
      let _ = inp.peek_one()?;
      Ok(ParseAttempt::Accept(1))
    };
    elem
      .repeated()
      .delimited_by_brackets()
      .collect()
      .parse_input(inp)
  }
  let ctx = ParserContext::new(Verbose::<RDError>::new());
  let r: Vec<i64> = Parser::with_context(ctx)
    .apply(parse)
    .parse_str("[ 1]")
    .unwrap();
  assert_eq!(
    r.len(),
    1,
    "the committed-progress guard must stop after one push inside delimiters despite the trivia gap"
  );
}
