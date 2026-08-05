#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests exercising Container and Cache code paths.
//!
//! Covers `container.rs` (impls for `()`, `Option<T>`, `Vec<T>`, `VecDeque<T>`,
//! `PhantomData<T>`) and `cache/` (blackhole, option cache).

mod common;

use std::collections::VecDeque;

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  emitter::{
    FullContainerEmitter, SeparatedEmitter, Silent, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingTokenOf, SeparatedError, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::CowStr,
};

use common::{TestLexer, Token};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ContainerTestError;

impl From<()> for ContainerTestError {
  fn from(_: ()) -> Self {
    ContainerTestError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for ContainerTestError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ContainerTestError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for ContainerTestError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    ContainerTestError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for ContainerTestError {
  fn from(_: TooFew<S, Lang>) -> Self {
    ContainerTestError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for ContainerTestError {
  fn from(_: TooMany<S, Lang>) -> Self {
    ContainerTestError
  }
}

impl From<UnexpectedEot> for ContainerTestError {
  fn from(_: UnexpectedEot) -> Self {
    ContainerTestError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for ContainerTestError
{
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    ContainerTestError
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for ContainerTestError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    ContainerTestError
  }
}

// ── Custom emitter ────────────────────────────────────────────────────────────

struct ContainerEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for ContainerEmitter {
  type Error = ContainerTestError;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }

  fn emit_error(
    &mut self,
    err: Spanned<ContainerTestError, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), ContainerTestError>
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

impl<'inp> SeparatedEmitter<'inp, TestLexer<'inp>> for ContainerEmitter {
  fn emit_missing_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }

  fn emit_missing_element(
    &mut self,
    _: MissingSyntaxOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for ContainerEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }
}

impl<'inp> TooFewEmitter<'inp, TestLexer<'inp>> for ContainerEmitter {
  fn emit_too_few(
    &mut self,
    _: TooFew<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }
}

impl<'inp> TooManyEmitter<'inp, TestLexer<'inp>> for ContainerEmitter {
  fn emit_too_many(
    &mut self,
    _: TooMany<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }
}

impl<'inp> UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>> for ContainerEmitter {
  fn emit_unexpected_leading_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }
}

impl<'inp> UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>> for ContainerEmitter {
  fn emit_unexpected_trailing_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), ContainerTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(ContainerTestError)
  }
}

// ── Context constructors ──────────────────────────────────────────────────────

fn container_ctx() -> ParserContext<'static, TestLexer<'static>, ContainerEmitter> {
  ParserContext::new(ContainerEmitter)
}

fn silent_ctx() -> ParserContext<'static, TestLexer<'static>, Silent<ContainerTestError>> {
  ParserContext::new(Silent::new())
}

// ── Element parsers ───────────────────────────────────────────────────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>,
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
// Container tests - Vec<T>
// ═══════════════════════════════════════════════════════════════════════════════

fn collect_into_vec<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num.separated_by_comma().collect().parse_input(inp)
}

#[test]
fn test_collect_into_vec_empty() {
  let r: Vec<i64> = Parser::with_context(container_ctx())
    .apply(collect_into_vec)
    .parse_str("")
    .unwrap();
  assert!(r.is_empty());
}

#[test]
fn test_collect_into_vec_single() {
  let r: Vec<i64> = Parser::with_context(container_ctx())
    .apply(collect_into_vec)
    .parse_str("42")
    .unwrap();
  assert_eq!(r, vec![42]);
}

#[test]
fn test_collect_into_vec_multiple() {
  let r: Vec<i64> = Parser::with_context(container_ctx())
    .apply(collect_into_vec)
    .parse_str("1,2,3,4,5")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4, 5]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Container tests - VecDeque<T>
// ═══════════════════════════════════════════════════════════════════════════════

fn collect_into_vecdeque<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<VecDeque<i64>, ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num.separated_by_comma().collect().parse_input(inp)
}

#[test]
fn test_collect_into_vecdeque_empty() {
  let r: VecDeque<i64> = Parser::with_context(container_ctx())
    .apply(collect_into_vecdeque)
    .parse_str("")
    .unwrap();
  assert!(r.is_empty());
}

#[test]
fn test_collect_into_vecdeque_multiple() {
  let r: VecDeque<i64> = Parser::with_context(container_ctx())
    .apply(collect_into_vecdeque)
    .parse_str("10,20,30")
    .unwrap();
  assert_eq!(r, VecDeque::from([10, 20, 30]));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Container tests - () blackhole container
// ═══════════════════════════════════════════════════════════════════════════════

fn collect_into_unit<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(), ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .collect_with(())
    .parse_input(inp)
}

#[test]
fn test_collect_into_unit_empty() {
  let r: () = Parser::with_context(container_ctx())
    .apply(collect_into_unit)
    .parse_str("")
    .unwrap();
  assert_eq!(r, ());
}

#[test]
fn test_collect_into_unit_multiple() {
  // Blackhole container just discards elements - useful for validation/counting
  let r: () = Parser::with_context(container_ctx())
    .apply(collect_into_unit)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, ());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Container tests - fold combinator on TryParseInput
// ═══════════════════════════════════════════════════════════════════════════════

fn fold_sum<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>,
{
  try_num.fold(|| 0i64, |acc, n| acc + n).parse_input(inp)
}

#[test]
fn test_fold_sum_empty() {
  let r: i64 = Parser::with_context(container_ctx())
    .apply(fold_sum)
    .parse_str("")
    .unwrap();
  assert_eq!(r, 0);
}

#[test]
fn test_fold_sum_values() {
  let r: i64 = Parser::with_context(container_ctx())
    .apply(fold_sum)
    .parse_str("10 20 30")
    .unwrap();
  assert_eq!(r, 60);
}

#[test]
fn test_fold_sum_single() {
  let r: i64 = Parser::with_context(container_ctx())
    .apply(fold_sum)
    .parse_str("42")
    .unwrap();
  assert_eq!(r, 42);
}

#[test]
fn test_fold_stops_on_non_num() {
  // fold stops when try_num declines (on "+"), returning sum of what was parsed
  let r: i64 = Parser::with_context(container_ctx())
    .apply(fold_sum)
    .parse_str("1 2 +")
    .unwrap();
  assert_eq!(r, 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Cache tests - exercises cache code paths via different parser constructions
// ═══════════════════════════════════════════════════════════════════════════════

// The default cache (DefaultCache = GenericArrayDeque<..., U3>) is exercised
// by all tests above. Here we test with an Option cache (single-slot).

fn option_cache_ctx() -> ParserContext<
  'static,
  TestLexer<'static>,
  ContainerEmitter,
  Option<tokora::cache::CachedTokenOf<'static, TestLexer<'static>>>,
> {
  ParserContext::new(ContainerEmitter)
}

fn parse_single_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>,
{
  use common::TokenKind;
  use tokora::parser::expect;
  use tokora::utils::Expected;
  expect(|t: &Token| {
    if matches!(t, Token::Num(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Num))
    }
  })
  .map(|t| match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  })
  .parse_input(inp)
}

#[test]
fn test_option_cache_single_token() {
  let r: i64 = Parser::with_context(option_cache_ctx())
    .apply(parse_single_num)
    .parse_str("99")
    .unwrap();
  assert_eq!(r, 99);
}

#[test]
fn test_option_cache_error() {
  let r: Result<i64, _> = Parser::with_context(option_cache_ctx())
    .apply(parse_single_num)
    .parse_str("+");
  assert!(r.is_err());
}

// ── Blackhole cache (no caching at all) ──────────────────────────────────────

fn blackhole_cache_ctx() -> ParserContext<'static, TestLexer<'static>, ContainerEmitter, ()> {
  ParserContext::new(ContainerEmitter)
}

#[test]
fn test_blackhole_cache_single_token() {
  let r: i64 = Parser::with_context(blackhole_cache_ctx())
    .apply(parse_single_num)
    .parse_str("77")
    .unwrap();
  assert_eq!(r, 77);
}

#[test]
fn test_blackhole_cache_error() {
  let r: Result<i64, _> = Parser::with_context(blackhole_cache_ctx())
    .apply(parse_single_num)
    .parse_str("+");
  assert!(r.is_err());
}

// ── Default cache with multiple tokens ───────────────────────────────────────

#[test]
fn test_default_cache_collect_vec() {
  // This exercises the GenericArrayDeque cache with multiple tokens
  let r: Vec<i64> = Parser::with_context(container_ctx())
    .apply(collect_into_vec)
    .parse_str("1,2,3,4,5,6,7,8,9,10")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Silent emitter + container combinations
// ═══════════════════════════════════════════════════════════════════════════════

fn silent_collect_vec<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(10)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_silent_collect_vec_too_few() {
  // Silent emitter ignores too_few error, so collect succeeds with fewer elements
  let r: Result<Vec<i64>, _> = Parser::with_context(silent_ctx())
    .apply(silent_collect_vec)
    .parse_str("1,2,3");
  assert!(r.is_ok());
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

fn silent_collect_vecdeque<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<VecDeque<i64>, ContainerTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ContainerTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num.separated_by_comma().collect().parse_input(inp)
}

#[test]
fn test_silent_collect_vecdeque() {
  let r: VecDeque<i64> = Parser::with_context(silent_ctx())
    .apply(silent_collect_vecdeque)
    .parse_str("5,10,15")
    .unwrap();
  assert_eq!(r, VecDeque::from([5, 10, 15]));
}
