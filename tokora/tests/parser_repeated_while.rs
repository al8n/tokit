#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
#![allow(warnings)]

//! Tests for the `RepeatedWhile` (condition-based repetition WITHOUT separator) combinator.
//!
//! Exercises `parser/many/repeated_while/` (unbounded, at_least, at_most, bounded),
//! `parser/many/delim/repeated_while.rs` via `.delimited()`, and
//! `parser/many/handler/bounded.rs` via `.at_least()`, `.at_most()`, `.bounded()`.
//!
//! # Sentinel token
//!
//! `RepeatedWhile::parse` calls `peek_with_emitter` to decide whether to continue.
//! At EOF there is nothing to peek, which triggers a debug_assert.  We therefore
//! append `+` (a non-Num token) to every test string so the condition always sees
//! a stop token instead of hitting EOF.  The trailing `+` is left unconsumed;
//! `parse_str` does not require all tokens to be consumed.

mod common;

use generic_arraydeque::typenum::U1;
use tokora::EmitterView;
use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait,
  cache::Peeked,
  emitter::{
    FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter, Verbose,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingTokenOf, SeparatedError, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  parser::Action,
  punct::Bracket,
  span::Spanned,
  utils::CowStr,
};

use common::{TestLexer, Token};

// -- Error type ---------------------------------------------------------------

#[derive(Debug)]
struct RWError;

impl From<()> for RWError {
  fn from(_: ()) -> Self {
    RWError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for RWError {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    RWError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for RWError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    RWError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for RWError {
  fn from(_: TooFew<S, Lang>) -> Self {
    RWError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for RWError {
  fn from(_: TooMany<S, Lang>) -> Self {
    RWError
  }
}

impl From<UnexpectedEot> for RWError {
  fn from(_: UnexpectedEot) -> Self {
    RWError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for RWError {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    RWError
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for RWError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    RWError
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for RWError {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    RWError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for RWError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    RWError
  }
}

// -- Custom emitter -----------------------------------------------------------

struct RWEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for RWEmitter {
  type Error = RWError;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RWError)
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RWError)
  }

  fn emit_error(
    &mut self,
    err: Spanned<RWError, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RWError>
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

impl<'inp> SeparatedEmitter<'inp, TestLexer<'inp>> for RWEmitter {
  fn emit_missing_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RWError)
  }

  fn emit_missing_element(
    &mut self,
    _: MissingSyntaxOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RWError)
  }
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for RWEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RWError)
  }
}

impl<'inp> UnclosedEmitter<'inp, TestLexer<'inp>> for RWEmitter {
  fn emit_unclosed<Delimiter>(
    &mut self,
    _: Unclosed<Delimiter, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
    RWError: tokora::emitter::FromUnclosed<'inp, TestLexer<'inp>>,
  {
    Err(RWError)
  }
}

impl<'inp> TooFewEmitter<'inp, TestLexer<'inp>> for RWEmitter {
  fn emit_too_few(
    &mut self,
    _: TooFew<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RWError)
  }
}

impl<'inp> TooManyEmitter<'inp, TestLexer<'inp>> for RWEmitter {
  fn emit_too_many(
    &mut self,
    _: TooMany<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), RWError>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(RWError)
  }
}

fn rw_ctx() -> ParserContext<'static, TestLexer<'static>, RWEmitter> {
  ParserContext::new(RWEmitter)
}

// -- Supertrait alias ---------------------------------------------------------

trait RWEmitterBound<'inp>:
  Emitter<'inp, TestLexer<'inp>, Error = RWError>
  + FullContainerEmitter<'inp, TestLexer<'inp>>
  + UnclosedEmitter<'inp, TestLexer<'inp>>
  + SeparatedEmitter<'inp, TestLexer<'inp>>
{
}

impl<'inp, E> RWEmitterBound<'inp> for E where
  E: Emitter<'inp, TestLexer<'inp>, Error = RWError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
{
}

// -- Condition: continue iff next token is a Num ------------------------------

fn decide_num_rw<'inp, Ctx>(
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

// -- Element parser (ParseInput) ----------------------------------------------

fn parse_num_rw<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RWError>,
{
  match inp.next()? {
    None => Err(RWError),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(RWError),
    },
  }
}

// == 1. Plain repeated_while (unbounded) ======================================

fn parse_rw_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_basic() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_list)
    .parse_str("1 2 3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_single() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_list)
    .parse_str("42+");
  assert_eq!(r.unwrap(), vec![42]);
}

// == 2. at_least ==============================================================

fn parse_rw_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_least_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_2)
    .parse_str("1 2 3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_at_least_fail() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_2)
    .parse_str("1+");
  assert!(r.is_err());
}

// == 3. at_most ===============================================================

fn parse_rw_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_most(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_most_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_2)
    .parse_str("1 2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_while_at_most_single() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_2)
    .parse_str("7+");
  assert_eq!(r.unwrap(), vec![7]);
}

// == 4. bounded ===============================================================

fn parse_rw_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .bounded(2, 4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_bounded_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded)
    .parse_str("1 2 3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_bounded_too_few() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded)
    .parse_str("1+");
  assert!(r.is_err());
}

// == 5. Delimited repeated_while (unbounded) ==================================

fn parse_rw_delimited<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_delimited() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited)
    .parse_str("[1 2 3]+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_delimited_empty() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited)
    .parse_str("[]+");
  assert_eq!(r.unwrap(), vec![]);
}

// == 6. Delimited at_least ====================================================

fn parse_rw_delimited_at_least_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_delimited_at_least_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_least_2)
    .parse_str("[1 2 3]+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_delimited_at_least_fail() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_least_2)
    .parse_str("[1]+");
  assert!(r.is_err());
}

// == 7. Delimited at_most =====================================================

fn parse_rw_delimited_at_most_2<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_most(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_delimited_at_most_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_most_2)
    .parse_str("[1 2]+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

// == 8. Delimited bounded =====================================================

fn parse_rw_delimited_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .bounded(2, 4)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_delimited_bounded_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_bounded)
    .parse_str("[1 2 3]+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_delimited_bounded_too_few() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_bounded)
    .parse_str("[1]+");
  assert!(r.is_err());
}

// == 9. at_most exceeded (too many) ===========================================

#[test]
fn test_repeated_while_at_most_exceeded() {
  // 3 elements with at_most(2) should trigger too_many error
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_2)
    .parse_str("1 2 3+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_at_most_exact_max() {
  // Exactly 2 elements with at_most(2) should succeed
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_2)
    .parse_str("1 2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_while_at_most_empty() {
  // 0 elements with at_most(2) should succeed (zero is fine)
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_2)
    .parse_str("+");
  assert_eq!(r.unwrap(), vec![]);
}

// == 10. bounded too many =====================================================

#[test]
fn test_repeated_while_bounded_too_many() {
  // 5 elements with bounded(2, 4) should fail
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded)
    .parse_str("1 2 3 4 5+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_bounded_exact_min() {
  // Exactly 2 elements with bounded(2, 4) should succeed
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded)
    .parse_str("1 2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_while_bounded_exact_max() {
  // Exactly 4 elements with bounded(2, 4) should succeed
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded)
    .parse_str("1 2 3 4+");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn test_repeated_while_bounded_zero_fail() {
  // 0 elements with bounded(2, 4) should fail
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded)
    .parse_str("+");
  assert!(r.is_err());
}

// == 11. at_least exact boundary ==============================================

#[test]
fn test_repeated_while_at_least_exact_min() {
  // Exactly 2 elements with at_least(2) should succeed
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_2)
    .parse_str("1 2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_while_at_least_zero_fail() {
  // 0 elements with at_least(2) should fail
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_2)
    .parse_str("+");
  assert!(r.is_err());
}

// == 12. at_most via at_most().at_least() chain ===============================

fn parse_rw_at_most_then_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_most(4)
    .at_least(2)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_most_then_at_least_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_then_at_least)
    .parse_str("1 2 3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_at_most_then_at_least_too_few() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_then_at_least)
    .parse_str("1+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_at_most_then_at_least_too_many() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_then_at_least)
    .parse_str("1 2 3 4 5+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_at_most_then_at_least_exact_min() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_then_at_least)
    .parse_str("1 2+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_while_at_most_then_at_least_exact_max() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_then_at_least)
    .parse_str("1 2 3 4+");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

// == 13. at_least().at_most() chain ===========================================

fn parse_rw_at_least_then_at_most<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_least(2)
    .at_most(4)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_least_then_at_most_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_then_at_most)
    .parse_str("1 2 3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_at_least_then_at_most_too_few() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_then_at_most)
    .parse_str("1+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_at_least_then_at_most_too_many() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_then_at_most)
    .parse_str("1 2 3 4 5+");
  assert!(r.is_err());
}

// == 14. bounded with tight range (same min and max) ==========================

fn parse_rw_bounded_exact_3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .bounded(3, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_bounded_exact_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded_exact_3)
    .parse_str("1 2 3+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_bounded_exact_too_few() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded_exact_3)
    .parse_str("1 2+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_bounded_exact_too_many() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded_exact_3)
    .parse_str("1 2 3 4+");
  assert!(r.is_err());
}

// == 15. Delimited at_most exceeded ===========================================

#[test]
fn test_repeated_while_delimited_at_most_exceeded() {
  // 3 elements with at_most(2) inside brackets should trigger error
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_most_2)
    .parse_str("[1 2 3]+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_delimited_at_most_single() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_most_2)
    .parse_str("[7]+");
  assert_eq!(r.unwrap(), vec![7]);
}

#[test]
fn test_repeated_while_delimited_at_most_empty() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_most_2)
    .parse_str("[]+");
  assert_eq!(r.unwrap(), vec![]);
}

// == 16. Delimited bounded too many ===========================================

#[test]
fn test_repeated_while_delimited_bounded_too_many() {
  // 5 elements with bounded(2, 4) inside brackets should fail
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_bounded)
    .parse_str("[1 2 3 4 5]+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_delimited_bounded_exact_min() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_bounded)
    .parse_str("[1 2]+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

#[test]
fn test_repeated_while_delimited_bounded_exact_max() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_bounded)
    .parse_str("[1 2 3 4]+");
  assert_eq!(r.unwrap(), vec![1, 2, 3, 4]);
}

// == 17. Spanned output variants ==============================================

fn parse_rw_bounded_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .bounded(1, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_bounded_spanned_ok() {
  let r = Parser::with_context(rw_ctx())
    .apply(parse_rw_bounded_spanned)
    .parse_str("10 20+");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![10, 20]);
}

fn parse_rw_at_least_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_least(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_least_spanned_ok() {
  let r = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_spanned)
    .parse_str("5 6 7+");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![5, 6, 7]);
}

fn parse_rw_at_most_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_most(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_most_spanned_ok() {
  let r = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_spanned)
    .parse_str("8 9+");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![8, 9]);
}

fn parse_rw_unbounded_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, <TestLexer<'inp> as Lexer<'inp>>::Span>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_unbounded_spanned_ok() {
  let r = Parser::with_context(rw_ctx())
    .apply(parse_rw_unbounded_spanned)
    .parse_str("1 2 3+");
  let spanned = r.unwrap();
  assert_eq!(*spanned.data(), vec![1, 2, 3]);
}

// == 18. Unbounded edge cases =================================================

#[test]
fn test_repeated_while_unbounded_empty() {
  // No matching tokens -> empty vec (stop immediately)
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_list)
    .parse_str("+");
  assert_eq!(r.unwrap(), vec![]);
}

#[test]
fn test_repeated_while_unbounded_five() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_list)
    .parse_str("10 20 30 40 50+");
  assert_eq!(r.unwrap(), vec![10, 20, 30, 40, 50]);
}

// == 19. at_least(1) boundary =================================================

fn parse_rw_at_least_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_least(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_least_1_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_1)
    .parse_str("99+");
  assert_eq!(r.unwrap(), vec![99]);
}

#[test]
fn test_repeated_while_at_least_1_zero_fail() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_least_1)
    .parse_str("+");
  assert!(r.is_err());
}

// == 20. at_most(1) boundary ==================================================

fn parse_rw_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp> + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_at_most_1_single() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_1)
    .parse_str("7+");
  assert_eq!(r.unwrap(), vec![7]);
}

#[test]
fn test_repeated_while_at_most_1_exceeded() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_at_most_1)
    .parse_str("7 8+");
  assert!(r.is_err());
}

// == 21. Delimited at_least exact boundary ====================================

#[test]
fn test_repeated_while_delimited_at_least_exact_min() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_least_2)
    .parse_str("[1 2]+");
  assert_eq!(r.unwrap(), vec![1, 2]);
}

// == 22. Delimited bounded via chains =========================================

fn parse_rw_delimited_at_most_then_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: RWEmitterBound<'inp>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num_rw
    .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
    .at_most(4)
    .at_least(2)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn test_repeated_while_delimited_at_most_then_at_least_ok() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_most_then_at_least)
    .parse_str("[1 2 3]+");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn test_repeated_while_delimited_at_most_then_at_least_too_few() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_most_then_at_least)
    .parse_str("[1]+");
  assert!(r.is_err());
}

#[test]
fn test_repeated_while_delimited_at_most_then_at_least_too_many() {
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse_rw_delimited_at_most_then_at_least)
    .parse_str("[1 2 3 4 5]+");
  assert!(r.is_err());
}

// == full container under Verbose: continues, records, and terminates =========

// With a capacity-1 container fed more elements than it can hold, the `repeated_while` loop
// reports the refusal ONCE — a container that refuses one push refuses every later one, so
// re-emitting per dropped element produced a count that climbed past the capacity it named.
// Under `Verbose` that call records the error and returns `Ok`, so the loop no longer
// short-circuits on the first overflow. It must still terminate: the loop's other exits (the
// condition returning `Stop`, or the element parser failing) are reached on any bounded input.
// The trailing `+` sentinel makes the condition return `Stop`, so the parse halts and returns.
//
// This pins the once-per-construct latch on the repeated wing; the payload it carries
// (`FullContainer(2, 1)`) is pinned across all eight drivers by `tests/end_state_parity.rs`
// case D, which this file's payload-free `RWError` cannot observe.
#[test]
fn test_repeated_while_full_container_verbose_records_and_terminates() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<RWError>>,
    >,
  ) -> Result<Option<i64>, RWError> {
    let out: Option<i64> = parse_num_rw
      .repeated_while::<_, U1>(
        decide_num_rw::<ParserContext<'inp, TestLexer<'inp>, Verbose<RWError>>>,
      )
      .collect_with(None::<i64>)
      .parse_input(inp)?;
    // Only the first element fit into the capacity-1 container.
    assert_eq!(out, Some(1));
    // The overflow was recorded rather than aborting the parse — and recorded exactly once,
    // however many later pushes the full container went on to refuse.
    let recorded: usize = inp.emitter_ref().errors().values().map(|v| v.len()).sum();
    assert_eq!(
      recorded, 1,
      "a full container reports its refusal once per construct, not once per dropped element"
    );
    Ok(out)
  }

  let ctx = ParserContext::new(Verbose::<RWError>::new());
  let r: Result<Option<i64>, RWError> = Parser::with_context(ctx).apply(parse).parse_str("1 2 3+");
  assert_eq!(r.unwrap(), Some(1));
}

// == 12. Progress guard: a zero-consumption Continue cycle must terminate =====
//
// Regression tests for the `repeated_while` progress-guard parity: a condition
// that keeps answering `Continue` paired with an element parser that consumes nothing used to
// loop forever — `Repeated` had the cursor-compare guard, `RepeatedWhile` did not. The guard
// treats a no-progress cycle as end of elements, exactly as the `Repeated` family does.

/// "Parses" an element without consuming any input — the pathological pair for a `Continue`
/// condition, which sees the same lookahead on every cycle.
fn parse_consume_nothing_rw<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, RWError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RWError>,
{
  let _ = inp;
  Ok(0)
}

#[test]
fn test_repeated_while_zero_consumption_terminates() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Vec<i64>, RWError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: RWEmitterBound<'inp>,
  {
    parse_consume_nothing_rw
      .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  // The lookahead is a `Num`, so the condition answers `Continue`; the element parser consumes
  // nothing. Without the guard this loops forever; with it, the first no-progress cycle stops
  // the repetition after its single pushed element.
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse)
    .parse_str("1 2 3+");
  assert_eq!(
    r.unwrap(),
    vec![0],
    "the no-progress cycle stops after one element instead of looping"
  );
}

#[test]
fn test_repeated_while_delimited_zero_consumption_terminates() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Vec<i64>, RWError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: RWEmitterBound<'inp>,
  {
    parse_consume_nothing_rw
      .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  // Same pathological pair inside delimiters: the close-delimiter check keeps failing on `1`
  // and the condition keeps answering `Continue`. The guard breaks to the close-delimiter
  // epilogue, which reports the unclosed bracket through the fail-fast test emitter — the
  // parse terminates with an error instead of looping.
  let r: Result<Vec<i64>, RWError> = Parser::with_context(rw_ctx())
    .apply(parse)
    .parse_str("[1 2]+");
  assert!(
    r.is_err(),
    "the no-progress cycle terminates through the close-delimiter epilogue"
  );
}

// == 13. Committed-progress guard: leading-trivia twins =======================

#[test]
fn zero_width_element_with_leading_trivia_stops_after_one_element() {
  // The leading space in `" 1"` is skipped trivia, so the driver's decision peek jumps the
  // cache-front cursor to the token start without consuming anything. A cursor-keyed guard reads
  // that as progress and runs one extra cycle, pushing a phantom element; the committed-consumption
  // metric stops after the first.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Vec<i64>, RWError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = RWError> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    let elem =
      |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<i64, RWError> { Ok(0) };
    elem
      .repeated_while::<_, U1>(decide_num_rw::<Ctx>)
      .collect()
      .parse_input(inp)
  }
  let r: Vec<i64> = Parser::with_context(rw_ctx())
    .apply(parse)
    .parse_str(" 1")
    .unwrap();
  assert_eq!(
    r.len(),
    1,
    "the committed-progress guard must stop after one element despite the leading trivia gap"
  );
}

#[test]
fn delimited_zero_width_element_with_leading_trivia_stops_after_one_element() {
  // The delimited twin: the space after the opener in `"[ 1]"` is the trivia gap. A `Verbose`
  // emitter keeps the drive going past the epilogue's close-position diagnostic so the element
  // count is observable — a cursor-keyed guard pushes two, the committed metric one.
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<RWError>>,
    >,
  ) -> Result<Vec<i64>, RWError> {
    let elem = |_inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<RWError>>,
    >|
     -> Result<i64, RWError> { Ok(0) };
    elem
      .repeated_while::<_, U1>(
        decide_num_rw::<ParserContext<'inp, TestLexer<'inp>, Verbose<RWError>>>,
      )
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let ctx = ParserContext::new(Verbose::<RWError>::new());
  let r: Vec<i64> = Parser::with_context(ctx)
    .apply(parse)
    .parse_str("[ 1]")
    .unwrap();
  assert_eq!(
    r.len(),
    1,
    "the committed-progress guard must stop after one element inside delimiters despite the trivia gap"
  );
}
