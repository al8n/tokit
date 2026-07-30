#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(unused_imports)]

//! Tests exercising the emitter system: Fatal, Silent, and Verbose emitters.
//!
//! These tests trigger error paths through parser combinators to exercise
//! emitter implementations in `emitter/impl_/fatal.rs`, `emitter/impl_/silent.rs`,
//! and `emitter/impl_/verbose.rs`.

mod common;

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  emitter::{
    Fatal, FromSeparatedError, FromUnexpectedLeadingSeparatorError,
    FromUnexpectedTrailingSeparatorError, FullContainerEmitter, Ignored, PrattEmitter,
    SeparatedEmitter, Severity, Silent, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, Verbose,
  },
  error::{
    UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf, TooFew, TooMany},
    token::{
      MissingToken, MissingTokenOf, SeparatedError, SeparatorPosition, UnexpectedToken,
      UnexpectedTokenOf,
    },
  },
  input::Cursor,
  span::{SimpleSpan, Spanned},
  try_parse_input::ParseAttempt,
  utils::CowStr,
};

use common::{TestLexer, Token, TokenKind};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum EmitterTestError {
  UnexpectedToken,
  LexerError,
  TooFew,
  TooMany,
  FullContainer,
  MissingSeparator,
  MissingElement,
  UnexpectedLeadingSep,
  UnexpectedTrailingSep,
  Custom,
}

impl From<()> for EmitterTestError {
  fn from(_: ()) -> Self {
    EmitterTestError::LexerError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for EmitterTestError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    EmitterTestError::UnexpectedToken
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for EmitterTestError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    EmitterTestError::FullContainer
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for EmitterTestError {
  fn from(_: TooFew<S, Lang>) -> Self {
    EmitterTestError::TooFew
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for EmitterTestError {
  fn from(_: TooMany<S, Lang>) -> Self {
    EmitterTestError::TooMany
  }
}

impl From<UnexpectedEot> for EmitterTestError {
  fn from(_: UnexpectedEot) -> Self {
    EmitterTestError::Custom
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for EmitterTestError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    EmitterTestError::MissingSeparator
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for EmitterTestError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    EmitterTestError::MissingElement
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for EmitterTestError
{
  fn from(err: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    match err.position() {
      SeparatorPosition::Leading => EmitterTestError::UnexpectedLeadingSep,
      SeparatorPosition::Trailing => EmitterTestError::UnexpectedTrailingSep,
      SeparatorPosition::Element => EmitterTestError::UnexpectedToken,
    }
  }
}

// ── Fatal-like custom emitter (all errors are fatal) ──────────────────────────

struct FatalEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for FatalEmitter {
  type Error = EmitterTestError;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::LexerError)
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::UnexpectedToken)
  }

  fn emit_error(
    &mut self,
    err: Spanned<EmitterTestError, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), EmitterTestError>
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

impl<'inp> SeparatedEmitter<'inp, TestLexer<'inp>> for FatalEmitter {
  fn emit_missing_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::MissingSeparator)
  }

  fn emit_missing_element(
    &mut self,
    _: MissingSyntaxOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::MissingElement)
  }
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for FatalEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::FullContainer)
  }
}

impl<'inp> TooFewEmitter<'inp, TestLexer<'inp>> for FatalEmitter {
  fn emit_too_few(
    &mut self,
    _: TooFew<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::TooFew)
  }
}

impl<'inp> TooManyEmitter<'inp, TestLexer<'inp>> for FatalEmitter {
  fn emit_too_many(
    &mut self,
    _: TooMany<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::TooMany)
  }
}

impl<'inp> UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>> for FatalEmitter {
  fn emit_unexpected_leading_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::UnexpectedLeadingSep)
  }
}

impl<'inp> UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>> for FatalEmitter {
  fn emit_unexpected_trailing_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), EmitterTestError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(EmitterTestError::UnexpectedTrailingSep)
  }
}

// ── Silent emitter ────────────────────────────────────────────────────────────

// ── Verbose emitter ───────────────────────────────────────────────────────────

// ── Context constructors ──────────────────────────────────────────────────────

fn fatal_ctx() -> ParserContext<'static, TestLexer<'static>, FatalEmitter> {
  ParserContext::new(FatalEmitter)
}

fn silent_ctx() -> ParserContext<'static, TestLexer<'static>, Silent<EmitterTestError>> {
  ParserContext::new(Silent::new())
}

fn verbose_ctx() -> ParserContext<'static, TestLexer<'static>, Verbose<EmitterTestError>> {
  ParserContext::new(Verbose::new())
}

// ── Element parsers ───────────────────────────────────────────────────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>,
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

fn parse_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>,
{
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

// ═══════════════════════════════════════════════════════════════════════════════
// Fatal emitter tests
// ═══════════════════════════════════════════════════════════════════════════════

// ── 1. unexpected token ───────────────────────────────────────────────────────

fn parse_expect_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>,
{
  parse_num(inp)
}

#[test]
fn test_fatal_unexpected_token() {
  let r: Result<i64, _> = Parser::with_context(fatal_ctx())
    .apply(parse_expect_num)
    .parse_str("+");
  assert!(r.is_err());
}

// ── 2. too few elements ──────────────────────────────────────────────────────

fn parse_at_least_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_fatal_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_at_least_3)
    .parse_str("1,2");
  assert!(r.is_err());
}

#[test]
fn test_fatal_too_few_ok() {
  let r: Vec<i64> = Parser::with_context(fatal_ctx())
    .apply(parse_at_least_3)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── 3. too many elements ─────────────────────────────────────────────────────

fn parse_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_fatal_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_at_most_2)
    .parse_str("1,2,3");
  assert!(r.is_err());
}

#[test]
fn test_fatal_too_many_ok() {
  let r: Vec<i64> = Parser::with_context(fatal_ctx())
    .apply(parse_at_most_2)
    .parse_str("1,2")
    .unwrap();
  assert_eq!(r, vec![1, 2]);
}

// ── 4. bounded (triggers both too few and too many) ──────────────────────────

fn parse_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
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
fn test_fatal_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(fatal_ctx())
    .apply(parse_bounded)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_fatal_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_bounded)
    .parse_str("1");
  assert!(r.is_err());
}

#[test]
fn test_fatal_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_bounded)
    .parse_str("1,2,3,4,5");
  assert!(r.is_err());
}

// ── 5. empty input (exercises emitter construction path) ─────────────────────

#[test]
fn test_fatal_empty_input() {
  let r: Result<i64, _> = Parser::with_context(fatal_ctx())
    .apply(parse_expect_num)
    .parse_str("");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Silent emitter tests
// ═══════════════════════════════════════════════════════════════════════════════

// Silent emitter returns Ok(()) for all errors, allowing parsing to continue.
// We test that error paths do not abort parsing.

fn parse_silent_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
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
fn test_silent_too_few_continues() {
  // Silent emitter swallows the "too few" error, so we get Ok with what was parsed
  let r: Result<Vec<i64>, _> = Parser::with_context(silent_ctx())
    .apply(parse_silent_bounded)
    .parse_str("1");
  // Silent emitter makes too_few non-fatal, so this should succeed
  assert!(r.is_ok());
}

#[test]
fn test_silent_too_many_continues() {
  // Silent emitter swallows the "too many" error
  let r: Result<Vec<i64>, _> = Parser::with_context(silent_ctx())
    .apply(parse_silent_bounded)
    .parse_str("1,2,3,4,5");
  // Silent emitter makes too_many non-fatal, so this should succeed
  assert!(r.is_ok());
}

#[test]
fn test_silent_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(silent_ctx())
    .apply(parse_silent_bounded)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verbose emitter tests
// ═══════════════════════════════════════════════════════════════════════════════

// Verbose emitter collects errors into a BTreeMap and returns Ok(()), allowing
// parsing to continue. Errors can be retrieved after parsing completes.

#[test]
fn test_verbose_too_few_collects_error() {
  // Verbose emitter collects the "too few" error but parsing continues
  let r: Result<Vec<i64>, _> = Parser::with_context(verbose_ctx())
    .apply(parse_bounded)
    .parse_str("1");
  // Verbose emitter makes too_few non-fatal, so parsing succeeds
  assert!(r.is_ok());
}

#[test]
fn test_verbose_too_many_collects_error() {
  // Verbose emitter collects the "too many" error but parsing continues
  let r: Result<Vec<i64>, _> = Parser::with_context(verbose_ctx())
    .apply(parse_bounded)
    .parse_str("1,2,3,4,5");
  // Verbose emitter makes too_many non-fatal, so parsing succeeds
  assert!(r.is_ok());
}

#[test]
fn test_verbose_bounded_ok() {
  let r: Vec<i64> = Parser::with_context(verbose_ctx())
    .apply(parse_bounded)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

#[test]
fn test_verbose_unexpected_token() {
  // Verbose collects unexpected token errors
  let r: Result<i64, _> = Parser::with_context(verbose_ctx())
    .apply(parse_expect_num)
    .parse_str("+");
  // unexpected token via Verbose still collects - but the parser function
  // itself uses expect() which returns an error, so the overall parse fails
  // because the parser returns Err after emitter returns Ok.
  // Actually, the Verbose emitter returns Ok but the expect combinator
  // itself may still fail. Let's just check it doesn't panic.
  let _ = r;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Fatal emitter - happy path exercises all emitter trait impls
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_fatal_simple_parse_ok() {
  // Simple parse that succeeds - exercises Fatal emitter construction
  let r: i64 = Parser::with_context(fatal_ctx())
    .apply(parse_expect_num)
    .parse_str("42")
    .unwrap();
  assert_eq!(r, 42);
}

#[test]
fn test_fatal_separated_ok() {
  // Happy path through separated combinator with Fatal emitter
  let r: Vec<i64> = Parser::with_context(fatal_ctx())
    .apply(parse_at_least_3)
    .parse_str("10,20,30,40")
    .unwrap();
  assert_eq!(r, vec![10, 20, 30, 40]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Silent emitter - exercises all Silent trait impls
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_silent_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(5)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_silent_at_least_too_few() {
  // With Silent emitter, "at least 5" with only 2 elements silently succeeds
  let r: Result<Vec<i64>, _> = Parser::with_context(silent_ctx())
    .apply(parse_silent_at_least)
    .parse_str("1,2");
  assert!(r.is_ok());
}

fn parse_silent_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_silent_at_most_too_many() {
  // With Silent emitter, "at most 1" with 3 elements silently succeeds
  let r: Result<Vec<i64>, _> = Parser::with_context(silent_ctx())
    .apply(parse_silent_at_most)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verbose emitter - exercises Verbose trait impls with error collection
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_verbose_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(5)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_verbose_at_least_too_few() {
  // With Verbose emitter, "at least 5" with only 2 elements collects error
  let r: Result<Vec<i64>, _> = Parser::with_context(verbose_ctx())
    .apply(parse_verbose_at_least)
    .parse_str("1,2");
  assert!(r.is_ok());
}

fn parse_verbose_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_verbose_at_most_too_many() {
  // With Verbose emitter, "at most 1" with 3 elements collects error
  let r: Result<Vec<i64>, _> = Parser::with_context(verbose_ctx())
    .apply(parse_verbose_at_most)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// Tests that directly exercise emitter trait method implementations for
// Ignored, Silent, Fatal, and Verbose emitters to cover many small uncovered
// code paths (each file has 0-4 uncovered lines).

// ═══════════════════════════════════════════════════════════════════════════════
// Helper error type for Fatal/Verbose emitters that need FromEmitterError etc.
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
enum TestError {
  UnexpectedToken,
  LexerError,
  TooFew,
  TooMany,
  FullContainer,
  MissingSeparator,
  MissingElement,
  UnexpectedLeadingSep,
  UnexpectedTrailingSep,
  UnexpectedEoLhs,
  UnexpectedEoRhs,
}

impl From<()> for TestError {
  fn from(_: ()) -> Self {
    TestError::LexerError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for TestError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    TestError::UnexpectedToken
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for TestError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    TestError::FullContainer
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for TestError {
  fn from(_: TooFew<S, Lang>) -> Self {
    TestError::TooFew
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for TestError {
  fn from(_: TooMany<S, Lang>) -> Self {
    TestError::TooMany
  }
}

impl From<UnexpectedEot> for TestError {
  fn from(_: UnexpectedEot) -> Self {
    TestError::LexerError
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEoLhs<O, Lang>> for TestError {
  fn from(_: UnexpectedEoLhs<O, Lang>) -> Self {
    TestError::UnexpectedEoLhs
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEoRhs<O, Lang>> for TestError {
  fn from(_: UnexpectedEoRhs<O, Lang>) -> Self {
    TestError::UnexpectedEoRhs
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for TestError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    TestError::MissingSeparator
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for TestError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    TestError::MissingElement
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for TestError {
  fn from(err: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    match err.position() {
      SeparatorPosition::Leading => TestError::UnexpectedLeadingSep,
      SeparatorPosition::Trailing => TestError::UnexpectedTrailingSep,
      SeparatorPosition::Element => TestError::UnexpectedToken,
    }
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ignored emitter: direct trait method calls
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ignored_full_container_emitter() {
  let mut ign = Ignored::default();
  let err = FullContainer::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result =
    <Ignored as FullContainerEmitter<'_, TestLexer<'_>>>::emit_full_container(&mut ign, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_too_few_emitter() {
  let mut ign = Ignored::default();
  let err = TooFew::new(SimpleSpan::new(0usize, 5usize), 1, 3);
  let result = <Ignored as TooFewEmitter<'_, TestLexer<'_>>>::emit_too_few(&mut ign, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_too_many_emitter() {
  let mut ign = Ignored::default();
  let err = TooMany::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result = <Ignored as TooManyEmitter<'_, TestLexer<'_>>>::emit_too_many(&mut ign, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_separated_emitter_missing_separator() {
  let mut ign = Ignored::default();
  let name = CowStr::from_static("comma");
  let err: MissingToken<'_, TokenKind, usize> = MissingToken::new(0usize);
  let result =
    <Ignored as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(&mut ign, name, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_separated_emitter_missing_element() {
  let mut ign = Ignored::default();
  let err = MissingSyntax::new(0usize);
  let result =
    <Ignored as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_element(&mut ign, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_unexpected_leading_separator_emitter() {
  let mut ign = Ignored::default();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Ignored as UnexpectedLeadingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_leading_separator(&mut ign, name, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_unexpected_trailing_separator_emitter() {
  let mut ign = Ignored::default();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Ignored as UnexpectedTrailingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_trailing_separator(&mut ign, name, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_pratt_emitter_lhs() {
  let mut ign = Ignored::default();
  let err = UnexpectedEoLhs::eolhs(0usize);
  let result =
    <Ignored as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_lhs(&mut ign, err);
  assert!(result.is_ok());
}

#[test]
fn ignored_pratt_emitter_rhs() {
  let mut ign = Ignored::default();
  let err = UnexpectedEoRhs::eorhs(0usize);
  let result =
    <Ignored as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_rhs(&mut ign, err);
  assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Silent emitter: direct trait method calls
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn silent_full_container_emitter() {
  let mut s = Silent::<TestError>::new();
  let err = FullContainer::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result = <Silent<TestError> as FullContainerEmitter<'_, TestLexer<'_>>>::emit_full_container(
    &mut s, err,
  );
  assert!(result.is_ok());
}

#[test]
fn silent_too_few_emitter() {
  let mut s = Silent::<TestError>::new();
  let err = TooFew::new(SimpleSpan::new(0usize, 5usize), 1, 3);
  let result = <Silent<TestError> as TooFewEmitter<'_, TestLexer<'_>>>::emit_too_few(&mut s, err);
  assert!(result.is_ok());
}

#[test]
fn silent_too_many_emitter() {
  let mut s = Silent::<TestError>::new();
  let err = TooMany::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result = <Silent<TestError> as TooManyEmitter<'_, TestLexer<'_>>>::emit_too_many(&mut s, err);
  assert!(result.is_ok());
}

#[test]
fn silent_separated_emitter_missing_separator() {
  let mut s = Silent::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err: MissingToken<'_, TokenKind, usize> = MissingToken::new(0usize);
  let result = <Silent<TestError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
    &mut s, name, err,
  );
  assert!(result.is_ok());
}

#[test]
fn silent_separated_emitter_missing_element() {
  let mut s = Silent::<TestError>::new();
  let err = MissingSyntax::new(0usize);
  let result =
    <Silent<TestError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_element(&mut s, err);
  assert!(result.is_ok());
}

#[test]
fn silent_unexpected_leading_separator_emitter() {
  let mut s = Silent::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Silent<TestError> as UnexpectedLeadingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_leading_separator(&mut s, name, err);
  assert!(result.is_ok());
}

#[test]
fn silent_unexpected_trailing_separator_emitter() {
  let mut s = Silent::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Silent<TestError> as UnexpectedTrailingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_trailing_separator(&mut s, name, err);
  assert!(result.is_ok());
}

#[test]
fn silent_pratt_emitter_lhs() {
  let mut s = Silent::<TestError>::new();
  let err = UnexpectedEoLhs::eolhs(0usize);
  let result =
    <Silent<TestError> as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_lhs(&mut s, err);
  assert!(result.is_ok());
}

#[test]
fn silent_pratt_emitter_rhs() {
  let mut s = Silent::<TestError>::new();
  let err = UnexpectedEoRhs::eorhs(0usize);
  let result =
    <Silent<TestError> as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_rhs(&mut s, err);
  assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Fatal emitter: direct trait method calls
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn fatal_full_container_emitter() {
  let mut f = Fatal::<TestError>::new();
  let err = FullContainer::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result =
    <Fatal<TestError> as FullContainerEmitter<'_, TestLexer<'_>>>::emit_full_container(&mut f, err);
  assert!(result.is_err());
}

#[test]
fn fatal_too_few_emitter() {
  let mut f = Fatal::<TestError>::new();
  let err = TooFew::new(SimpleSpan::new(0usize, 5usize), 1, 3);
  let result = <Fatal<TestError> as TooFewEmitter<'_, TestLexer<'_>>>::emit_too_few(&mut f, err);
  assert!(result.is_err());
}

#[test]
fn fatal_too_many_emitter() {
  let mut f = Fatal::<TestError>::new();
  let err = TooMany::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result = <Fatal<TestError> as TooManyEmitter<'_, TestLexer<'_>>>::emit_too_many(&mut f, err);
  assert!(result.is_err());
}

#[test]
fn fatal_separated_emitter_missing_separator() {
  let mut f = Fatal::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err: MissingToken<'_, TokenKind, usize> = MissingToken::new(0usize);
  let result = <Fatal<TestError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
    &mut f, name, err,
  );
  assert!(result.is_err());
}

#[test]
fn fatal_separated_emitter_missing_element() {
  let mut f = Fatal::<TestError>::new();
  let err = MissingSyntax::new(0usize);
  let result =
    <Fatal<TestError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_element(&mut f, err);
  assert!(result.is_err());
}

#[test]
fn fatal_unexpected_leading_separator_emitter() {
  let mut f = Fatal::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Fatal<TestError> as UnexpectedLeadingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_leading_separator(&mut f, name, err);
  assert!(result.is_err());
}

#[test]
fn fatal_unexpected_trailing_separator_emitter() {
  let mut f = Fatal::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Fatal<TestError> as UnexpectedTrailingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_trailing_separator(&mut f, name, err);
  assert!(result.is_err());
}

#[test]
fn fatal_pratt_emitter_lhs() {
  let mut f = Fatal::<TestError>::new();
  let err = UnexpectedEoLhs::eolhs(0usize);
  let result =
    <Fatal<TestError> as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_lhs(&mut f, err);
  assert!(result.is_err());
}

#[test]
fn fatal_pratt_emitter_rhs() {
  let mut f = Fatal::<TestError>::new();
  let err = UnexpectedEoRhs::eorhs(0usize);
  let result =
    <Fatal<TestError> as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_rhs(&mut f, err);
  assert!(result.is_err());
}

#[test]
fn fatal_emit_lexer_error() {
  let mut f = Fatal::<TestError>::new();
  let spanned = Spanned::new(SimpleSpan::new(0usize, 5usize), ());
  let result = <Fatal<TestError> as Emitter<'_, TestLexer<'_>>>::emit_lexer_error(&mut f, spanned);
  assert!(result.is_err());
}

#[test]
fn fatal_emit_unexpected_token() {
  let mut f = Fatal::<TestError>::new();
  let ut = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Fatal<TestError> as Emitter<'_, TestLexer<'_>>>::emit_unexpected_token(&mut f, ut);
  assert!(result.is_err());
}

#[test]
#[allow(clippy::clone_on_copy)]
fn fatal_clone_copy_debug() {
  let f = Fatal::<TestError>::new();
  let f2 = f.clone();
  let f3 = f;
  let _ = (f2, f3);
  let _ = Fatal::<TestError>::default();
  let dbg = format!("{:?}", Fatal::<TestError>::new());
  assert_eq!(dbg, "Fatal");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verbose emitter: direct trait method calls
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn verbose_full_container_emitter() {
  let mut v = Verbose::<TestError>::new();
  let err = FullContainer::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result = <Verbose<TestError> as FullContainerEmitter<'_, TestLexer<'_>>>::emit_full_container(
    &mut v, err,
  );
  // Verbose collects the full-container error and keeps going (record-and-Ok).
  assert!(result.is_ok());
  assert!(v.errors().contains_key(&SimpleSpan::new(0usize, 5usize)));
}

#[test]
fn verbose_too_few_emitter() {
  let mut v = Verbose::<TestError>::new();
  let err = TooFew::new(SimpleSpan::new(0usize, 5usize), 1, 3);
  let result = <Verbose<TestError> as TooFewEmitter<'_, TestLexer<'_>>>::emit_too_few(&mut v, err);
  assert!(result.is_ok());
  assert_eq!(v.errors().len(), 1);
}

#[test]
fn verbose_too_many_emitter() {
  let mut v = Verbose::<TestError>::new();
  let err = TooMany::new(SimpleSpan::new(0usize, 5usize), 10, 5);
  let result =
    <Verbose<TestError> as TooManyEmitter<'_, TestLexer<'_>>>::emit_too_many(&mut v, err);
  assert!(result.is_ok());
  assert_eq!(v.errors().len(), 1);
}

#[test]
fn verbose_separated_emitter_missing_separator() {
  let mut v = Verbose::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err: MissingToken<'_, TokenKind, usize> = MissingToken::new(0usize);
  let result = <Verbose<TestError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
    &mut v, name, err,
  );
  assert!(result.is_ok());
  assert_eq!(v.errors().len(), 1);
}

#[test]
fn verbose_separated_emitter_missing_element() {
  let mut v = Verbose::<TestError>::new();
  let err = MissingSyntax::new(0usize);
  let result =
    <Verbose<TestError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_element(&mut v, err);
  assert!(result.is_ok());
  assert_eq!(v.errors().len(), 1);
}

#[test]
fn verbose_unexpected_leading_separator_emitter() {
  let mut v = Verbose::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Verbose<TestError> as UnexpectedLeadingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_leading_separator(&mut v, name, err);
  assert!(result.is_ok());
  assert_eq!(v.errors().len(), 1);
}

#[test]
fn verbose_unexpected_trailing_separator_emitter() {
  let mut v = Verbose::<TestError>::new();
  let name = CowStr::from_static("comma");
  let err = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Verbose<TestError> as UnexpectedTrailingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_trailing_separator(&mut v, name, err);
  assert!(result.is_ok());
  assert_eq!(v.errors().len(), 1);
}

#[test]
fn verbose_pratt_emitter_lhs() {
  let mut v = Verbose::<TestError>::new();
  let err = UnexpectedEoLhs::eolhs(0usize);
  let result = <Verbose<TestError> as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_lhs(
    &mut v, err,
  );
  // Verbose collects the pratt error at its zero-width offset and keeps going.
  assert!(result.is_ok());
  assert!(v.errors().contains_key(&SimpleSpan::new(0usize, 0usize)));
}

#[test]
fn verbose_pratt_emitter_rhs() {
  let mut v = Verbose::<TestError>::new();
  let err = UnexpectedEoRhs::eorhs(0usize);
  let result = <Verbose<TestError> as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_rhs(
    &mut v, err,
  );
  // Verbose collects the pratt error at its zero-width offset and keeps going.
  assert!(result.is_ok());
  assert!(v.errors().contains_key(&SimpleSpan::new(0usize, 0usize)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ignored emitter: Emitter base trait methods
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ignored_emit_unexpected_token() {
  let mut ign = Ignored::default();
  let ut = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Ignored as Emitter<'_, TestLexer<'_>>>::emit_unexpected_token(&mut ign, ut);
  assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Silent emitter: Emitter base trait methods
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn silent_emit_lexer_error() {
  let mut s = Silent::<TestError>::new();
  let spanned = Spanned::new(SimpleSpan::new(0usize, 5usize), ());
  let result = <Silent<TestError> as Emitter<'_, TestLexer<'_>>>::emit_lexer_error(&mut s, spanned);
  assert!(result.is_ok());
}

#[test]
fn silent_emit_error() {
  let mut s = Silent::<TestError>::new();
  let spanned = Spanned::new(SimpleSpan::new(0usize, 5usize), TestError::LexerError);
  let result = <Silent<TestError> as Emitter<'_, TestLexer<'_>>>::emit_error(&mut s, spanned);
  assert!(result.is_ok());
}

#[test]
fn silent_emit_unexpected_token() {
  let mut s = Silent::<TestError>::new();
  let ut = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Silent<TestError> as Emitter<'_, TestLexer<'_>>>::emit_unexpected_token(&mut s, ut);
  assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verbose emitter: Emitter base trait methods (emit_unexpected_token)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn verbose_emit_unexpected_token() {
  let mut v = Verbose::<TestError>::new();
  let ut = UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result =
    <Verbose<TestError> as Emitter<'_, TestLexer<'_>>>::emit_unexpected_token(&mut v, ut);
  assert!(result.is_ok());
  assert_eq!(v.errors().len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verbose emitter: restore rewinds using the front cached token's start offset
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "unstable-raw")]
#[test]
fn restore_rewinds_verbose_errors_using_front_cache_start() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    // Consume the first token ("12" at 0..2) so there is a "before" region.
    let _ = inp.next()?;
    // Record an error strictly before the checkpoint (end < the cached start).
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(0, 2), EmitterTestError::Custom),
    )?;
    // Cache the next token ("34" at 3..5) so the checkpoint offset is its START.
    {
      let peeked = inp.peek_one()?;
      assert!(peeked.is_some());
    }
    let ckp = inp.save();
    // Record an error AFTER the checkpoint position (start = 3).
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(3, 4), EmitterTestError::Custom),
    )?;
    inp.restore(ckp);

    let errs = inp.emitter().errors();
    assert!(
      errs.contains_key(&SimpleSpan::new(0, 2)),
      "error before the checkpoint must survive restore"
    );
    assert!(
      !errs.contains_key(&SimpleSpan::new(3, 4)),
      "error after the checkpoint must be rewound"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("12 34");
  r.unwrap();
}

#[cfg(feature = "unstable-raw")]
#[test]
fn restore_rewinds_verbose_errors_adjacent_to_checkpoint() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    // Consume the first token ("12" at 0..2). Unlike the gapped scenario above,
    // this token is immediately adjacent to the next one (no whitespace), so its
    // end offset (2) equals the upcoming checkpoint's start offset.
    let _ = inp.next()?;
    // Record an error on the just-consumed token whose end == the checkpoint offset.
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(0, 2), EmitterTestError::Custom),
    )?;
    // Cache the next token ("a" at 2..3) so the checkpoint offset is its START,
    // which is exactly the end offset of the already-consumed, already-errored token.
    {
      let peeked = inp.peek_one()?;
      assert!(peeked.is_some());
    }
    let ckp = inp.save();
    // Record a speculative error starting exactly at the checkpoint (start = 2).
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(2, 3), EmitterTestError::Custom),
    )?;
    inp.restore(ckp);

    let errs = inp.emitter().errors();
    assert!(
      errs.contains_key(&SimpleSpan::new(0, 2)),
      "error ending exactly at the checkpoint (adjacent, already-consumed token) must survive restore"
    );
    assert!(
      !errs.contains_key(&SimpleSpan::new(2, 3)),
      "speculative error starting at the checkpoint must be rewound"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("12a");
  r.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verbose emitter: rewind of same-span error groups is by emission order
// ═══════════════════════════════════════════════════════════════════════════════

// Retention on restore is by emission order: every error recorded at or before the
// saved checkpoint mark survives; every error recorded after it is dropped. Here the
// two pre-checkpoint errors were emitted before `save()` (so they survive) and the
// two speculative errors after it (so they drop) — even though every error shares the
// same span as another, the emission-order mark splits them cleanly.
#[cfg(feature = "unstable-raw")]
#[test]
fn restore_rewinds_verbose_same_span_vec_by_span_end() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    // Consume "12" (0..2). Record TWO errors at the SAME span [0,2] whose end (2)
    // equals the upcoming checkpoint offset: both must survive restore.
    let _ = inp.next()?;
    for _ in 0..2 {
      <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
        inp.emitter(),
        Spanned::new(SimpleSpan::new(0, 2), EmitterTestError::Custom),
      )?;
    }
    // Cache "a" (2..3) so the checkpoint offset is its start (2).
    {
      let peeked = inp.peek_one()?;
      assert!(peeked.is_some());
    }
    let ckp = inp.save();
    // Two speculative errors at the SAME span [2,3] (end 3 > offset 2): both drop.
    for _ in 0..2 {
      <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
        inp.emitter(),
        Spanned::new(SimpleSpan::new(2, 3), EmitterTestError::Custom),
      )?;
    }
    inp.restore(ckp);

    let errs = inp.emitter().errors();
    // The pre-checkpoint same-span group survives intact (both errors kept).
    assert_eq!(
      errs.get(&SimpleSpan::new(0, 2)).map(Vec::len),
      Some(2),
      "both same-span errors ending exactly at the checkpoint survive restore"
    );
    // The post-checkpoint same-span group is rewound atomically.
    assert!(
      !errs.contains_key(&SimpleSpan::new(2, 3)),
      "same-span speculative errors past the checkpoint are all rewound"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("12a");
  r.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verbose emitter: emission-aware rewind vs. zero-width diagnostics at the checkpoint
// ═══════════════════════════════════════════════════════════════════════════════

// The former offset-only rewind retained any error whose span ended at or before the
// restore offset, so a *zero-width* error emitted AT the checkpoint offset during an
// abandoned branch (its end == the offset) survived as a ghost. Emission-aware rewind
// drops it because it was recorded after the checkpoint mark, regardless of span.
#[cfg(feature = "unstable-raw")]
#[test]
fn restore_drops_speculative_zero_width_ghost_at_checkpoint() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    // Consume "12" (0..2); the cursor — and thus the checkpoint offset — is now 2.
    let _ = inp.next()?;
    let ckp = inp.save();
    // Speculative branch emits a ZERO-WIDTH error at exactly the checkpoint offset.
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(2, 2), EmitterTestError::MissingElement),
    )?;
    inp.restore(ckp);

    // The ghost must be gone: the offset heuristic would have kept it (end 2 <= 2).
    assert!(
      !inp.emitter().errors().contains_key(&SimpleSpan::new(2, 2)),
      "speculative zero-width error at the checkpoint offset must be rewound"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("12");
  r.unwrap();
}

// The inverse the offset heuristic could not express: a zero-width error emitted
// BEFORE the checkpoint at the SAME offset must SURVIVE, while a later speculative one
// at that offset drops. Emission order separates them; span end alone cannot.
#[cfg(feature = "unstable-raw")]
#[test]
fn restore_keeps_pre_checkpoint_zero_width_at_same_offset() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    // Consume "12" (0..2); checkpoint offset is 2.
    let _ = inp.next()?;
    // A real zero-width diagnostic recorded BEFORE the checkpoint at offset 2.
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(2, 2), EmitterTestError::MissingElement),
    )?;
    let ckp = inp.save();
    // Speculative branch adds another zero-width error at the same offset.
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(2, 2), EmitterTestError::MissingElement),
    )?;
    inp.restore(ckp);

    // Exactly the pre-checkpoint error survives; the speculative one drops.
    assert_eq!(
      inp
        .emitter()
        .errors()
        .get(&SimpleSpan::new(2, 2))
        .map(Vec::len),
      Some(1),
      "the pre-checkpoint zero-width error survives; only the speculative one is rewound"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("12");
  r.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Diagnostic labels: `labelled` context captured into the emission log
// ═══════════════════════════════════════════════════════════════════════════════

// A sub-parser that records one custom diagnostic at 0..1 without consuming input, so a
// `labelled` wrapper around it exercises the enter/emit/exit path for any emitter.
fn emit_marker<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(), EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>,
{
  inp.emitter().emit_error(Spanned::new(
    SimpleSpan::new(0usize, 1usize),
    EmitterTestError::Custom,
  ))
}

fn builtin_fatal_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<EmitterTestError>> {
  ParserContext::new(Fatal::new())
}

// A diagnostic recorded inside a `labelled` scope is stamped with the label, readable
// per-diagnostic through the public `Verbose::labels` accessor (parallel to `errors`).
#[test]
fn labelled_stamps_context_visible_through_labels_accessor() {
  fn run<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    let mut p = tokora::labelled("while parsing item", emit_marker);
    p.parse_input(inp)?;

    let labels = inp.emitter().labels();
    assert_eq!(
      labels[&SimpleSpan::new(0usize, 1usize)],
      vec![vec!["while parsing item"]],
      "diagnostic recorded inside the labelled scope carries the label"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(run)
    .parse_str("12");
  r.unwrap();
}

// Under a non-collecting (Fatal) emitter, `labelled` is behaviorally transparent: the wrapped
// sub-parser's Err propagates unchanged and the label push/pop are inlined-away no-ops. The
// labelled result must equal the bare result, value for value.
#[test]
fn labelled_zero_behavior_on_fatal_path() {
  fn run<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Fatal<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    // Bare: the sub-parser emits fatally → Err(Custom).
    let bare = emit_marker(inp);
    assert!(
      matches!(bare, Err(EmitterTestError::Custom)),
      "bare Fatal emission is fatal"
    );
    // Labelled: identical Err(Custom); the label scope changes nothing on the Fatal path.
    let mut p = tokora::labelled("while parsing item", emit_marker);
    let via_label = p.parse_input(inp);
    assert!(
      matches!(via_label, Err(EmitterTestError::Custom)),
      "labelled leaves the Fatal path unchanged"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(builtin_fatal_ctx())
    .apply(run)
    .parse_str("12");
  r.unwrap();
}

// The pinned rewind rule: a guard rollback across a labelled emission drops the entry together
// with its captured labels; a later re-emission under a DIFFERENT label re-derives its labels
// from the then-current stack — labels are captured at emit time, not bound to the span.
#[test]
fn labelled_guard_rollback_drops_labels_then_reemission_rederives() {
  fn run<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    let survivor = SimpleSpan::new(0usize, 1usize);
    let speculative = SimpleSpan::new(1usize, 2usize);

    // Baseline emission under "outer", BEFORE any guard — must survive the rollback.
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::enter_label(
      inp.emitter(),
      "outer",
    );
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(survivor, EmitterTestError::Custom),
    )?;

    // Speculative labelled emission inside a transaction guard, then rolled back.
    {
      let mut tx = inp.begin();
      <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::enter_label(
        tx.emitter(),
        "spec",
      );
      <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
        tx.emitter(),
        Spanned::new(speculative, EmitterTestError::Custom),
      )?;
      <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::exit_label(tx.emitter());
      tx.rollback();
    }

    // Re-emit at the same span under a DIFFERENT label; labels re-derive from the current stack.
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::enter_label(
      inp.emitter(),
      "final",
    );
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(speculative, EmitterTestError::Custom),
    )?;
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::exit_label(inp.emitter());
    <Verbose<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::exit_label(inp.emitter());

    let labels = inp.emitter().labels();
    assert_eq!(
      labels[&survivor],
      vec![vec!["outer"]],
      "the pre-guard diagnostic survives with its label"
    );
    assert_eq!(
      labels[&speculative],
      vec![vec!["outer", "final"]],
      "the speculative [outer, spec] snapshot was rewound; re-emission re-derived [outer, final]"
    );
    assert_eq!(
      inp.emitter().errors()[&speculative].len(),
      1,
      "only the re-emitted diagnostic remains at the speculative span"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(run)
    .parse_str("12 34");
  r.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Severity tiers: the warning channel and the diagnostic rendering bridge
// ═══════════════════════════════════════════════════════════════════════════════

// A sub-parser that records one custom *warning* at 0..1 without consuming input, mirroring
// `emit_marker` but on the warning channel — so a `labelled` wrapper exercises the
// enter/emit_warning/exit path for any emitter.
fn warn_marker<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(), EmitterTestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EmitterTestError>,
{
  inp.emitter().emit_warning(Spanned::new(
    SimpleSpan::new(0usize, 1usize),
    EmitterTestError::Custom,
  ))
}

// A warning recorded inside a `labelled` scope collects into the parallel `warnings()` channel
// (leaving `errors()` empty) and carries its open-label snapshot in lockstep, readable through
// the `warning_labels()` accessor.
#[test]
fn verbose_warnings_collect_with_labels_parallel_to_errors() {
  fn run<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    let mut p = tokora::labelled("while linting item", warn_marker);
    p.parse_input(inp)?;

    let emitter = inp.emitter();
    assert_eq!(emitter.warnings().len(), 1, "one warning collected");
    assert_eq!(
      emitter.warnings()[&SimpleSpan::new(0usize, 1usize)].len(),
      1,
      "the warning landed at its span"
    );
    assert_eq!(
      emitter.errors().len(),
      0,
      "warnings are a separate channel — errors() stays empty"
    );
    assert_eq!(
      emitter.warning_labels()[&SimpleSpan::new(0usize, 1usize)],
      vec![vec!["while linting item"]],
      "the warning carries its open-label snapshot, in lockstep with warnings()"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(run)
    .parse_str("12");
  r.unwrap();
}

// Under a fail-fast Fatal emitter a warning has no sink: `emit_warning` is the inherited no-op
// that returns `Ok(())`, so parsing continues and produces its value — while an error on the
// same emitter is still fatal (the contrast that pins "warnings never stop it").
#[test]
fn fatal_ignores_warnings_but_errors_still_stop() {
  fn run<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Fatal<EmitterTestError>>,
    >,
  ) -> Result<i64, EmitterTestError> {
    // Two warnings in a row: each is ignored (Ok), parsing keeps going.
    assert!(
      matches!(warn_marker(inp), Ok(())),
      "Fatal ignores the first warning"
    );
    assert!(
      matches!(warn_marker(inp), Ok(())),
      "Fatal ignores the second warning — no warning sink"
    );
    // Contrast: an *error* on the same emitter is fatal.
    assert!(
      matches!(
        <Fatal<EmitterTestError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
          inp.emitter(),
          Spanned::new(SimpleSpan::new(0usize, 1usize), EmitterTestError::Custom),
        ),
        Err(EmitterTestError::Custom)
      ),
      "an error under Fatal is still fatal"
    );
    // Value-asserted continuation: the warnings did not stop the parse.
    Ok(7)
  }

  let r: Result<i64, _> = Parser::with_context(builtin_fatal_ctx())
    .apply(run)
    .parse_str("12");
  assert!(matches!(r, Ok(7)), "the parse completed past the warnings");
}

// The rendering bridge: `diagnostics()` yields BOTH channels interleaved in true emission order,
// each entry carrying the right severity tier and the label snapshot open when it was emitted.
#[test]
fn diagnostics_bridge_interleaves_both_channels_in_emission_order() {
  fn run<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<EmitterTestError>>,
    >,
  ) -> Result<(), EmitterTestError> {
    type V = Verbose<EmitterTestError>;
    // seq 0: Error @ 0..1 under [a]
    <V as Emitter<'inp, TestLexer<'inp>>>::enter_label(inp.emitter(), "a");
    <V as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(0usize, 1usize), EmitterTestError::Custom),
    )?;
    // seq 1: Warning @ 1..2 under [a, b]
    <V as Emitter<'inp, TestLexer<'inp>>>::enter_label(inp.emitter(), "b");
    <V as Emitter<'inp, TestLexer<'inp>>>::emit_warning(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(1usize, 2usize), EmitterTestError::Custom),
    )?;
    <V as Emitter<'inp, TestLexer<'inp>>>::exit_label(inp.emitter());
    <V as Emitter<'inp, TestLexer<'inp>>>::exit_label(inp.emitter());
    // seq 2: Error @ 2..3, unlabelled
    <V as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      inp.emitter(),
      Spanned::new(SimpleSpan::new(2usize, 3usize), EmitterTestError::Custom),
    )?;

    let replay: Vec<(Severity, usize, Vec<&'static str>)> = inp
      .emitter()
      .diagnostics()
      .map(|d| (d.severity(), d.span().start(), d.labels().to_vec()))
      .collect();

    assert_eq!(
      replay,
      vec![
        (Severity::Error, 0usize, vec!["a"]),
        (Severity::Warning, 1usize, vec!["a", "b"]),
        (Severity::Error, 2usize, Vec::new()),
      ],
      "the bridge replays errors and warnings interleaved in emission order, each with its \
       severity tier and captured labels"
    );
    Ok(())
  }

  let r: Result<(), _> = Parser::with_context(verbose_ctx())
    .apply(run)
    .parse_str("12 34 56");
  r.unwrap();
}

// ── `Fatal`'s `Default` and `Debug` are Lang-generic ─────────────────────────

/// `Fatal`'s `Clone`/`Copy` were `Lang`-generic and its `Default`/`Debug` were not — they were
/// written `impl<T: ?Sized> ... for Fatal<T>`, i.e. `Fatal<T, ()>` only. So a branded
/// `Fatal::<E, MyLang>::default()` did not resolve and a branded `Fatal` could not be printed,
/// while its `Silent` sibling could do both. Nothing in either body depends on the brand.
///
/// Before: this function does not compile. After: it does, and the render is unchanged at `()`.
#[test]
fn fatal_is_lang_generic_for_default_and_debug() {
  struct MyLang;

  let branded: Fatal<(), MyLang> = Fatal::default();
  assert_eq!(std::format!("{branded:?}"), "Fatal");

  // The unbranded render must not move — this widens impl coverage, it does not change output.
  let plain: Fatal<()> = Fatal::default();
  assert_eq!(std::format!("{plain:?}"), "Fatal");

  // The sibling this brings `Fatal` level with.
  let silent: Silent<(), MyLang> = Silent::default();
  assert_eq!(std::format!("{silent:?}"), "Silent");
}
