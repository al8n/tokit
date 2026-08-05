#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests for `recover` and `inplace_recover` combinators.

mod common;

use common::{TestLexer, Token, TokenKind};
use tokora::input::Cursor;
use tokora::parser::expect;
use tokora::utils::Expected;
use tokora::{Emitter, InputRef, Parse, ParseContext, ParseInput, Parser};

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
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

#[allow(unused)]
fn parse_plus<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  expect(|t: &Token| {
    if matches!(t, Token::Plus) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Plus))
    }
  })
  .map(|_| ())
  .parse_input(inp)
}

// ── Recovery functions (must be named functions, not closures, due to HRTB) ──

fn recovery_neg1<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(-1)
}

fn recovery_zero<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(0)
}

fn recovery_fail<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Err(())
}

fn recovery_neg99<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(-99)
}

#[allow(unused)]
fn recovery_hundred<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(100)
}

// Wrapper that applies parse_num from within recovery (ignores error, tries parse_num again)
fn recovery_try_parse_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  parse_num(inp)
}

// ── Inplace recovery functions ─────────────────────────────────────────────

fn inplace_recovery_neg99<'inp, 'cx, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _cursor: Cursor<'inp, '_, TestLexer<'inp>>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(-99)
}

fn inplace_recovery_zero<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _cursor: Cursor<'inp, '_, TestLexer<'inp>>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(0)
}

fn inplace_recovery_fail<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _cursor: Cursor<'inp, '_, TestLexer<'inp>>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Err(())
}

fn inplace_recovery_42<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _cursor: Cursor<'inp, '_, TestLexer<'inp>>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(42)
}

fn inplace_recovery_hundred<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _cursor: Cursor<'inp, '_, TestLexer<'inp>>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(100)
}

/// A primary parser that consumes two numbers, so a failure on the second leaves the
/// input advanced past its start position.
fn parse_two_nums<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  let a = parse_num(inp)?;
  let b = parse_num(inp)?;
  Ok(a + b)
}

/// Asserts the handed cursor is the primary parser's start position — the position the
/// checkpoint used to convey — while the input itself is left at the advanced error
/// position (in-place recovery never backtracks).
fn inplace_recovery_check_start<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  cursor: Cursor<'inp, '_, TestLexer<'inp>>,
  _err: (),
) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  assert_eq!(
    *cursor.as_inner(),
    0,
    "the handed cursor is the start position"
  );
  assert!(
    *inp.cursor().as_inner() > 0,
    "the input stayed at the advanced error position (no backtracking)"
  );
  Ok(7)
}

// ── recover: primary succeeds ─────────────────────────────────────────────────

#[test]
fn recover_primary_succeeds_returns_primary_value() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.recover(recovery_neg1).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("42").unwrap(), 42);
}

#[test]
fn recover_fallback_used_on_failure() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.recover(recovery_neg1).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), -1);
}

#[test]
fn recover_fallback_returns_zero_on_empty() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.recover(recovery_zero).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("").unwrap(), 0);
}

#[test]
fn recover_fallback_parses_from_original_position() {
  // Primary fails on "+", checkpoint restores to "+", recovery also tries parse_num → also fails
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.recover(recovery_try_parse_num).parse_input(inp)
  }
  // Both primary and recovery fail on "+"
  assert!(Parser::new().apply(p).parse_str("+").is_err());
  // Primary succeeds on "42"
  assert_eq!(Parser::new().apply(p).parse_str("42").unwrap(), 42);
}

#[test]
fn recover_chained_multiple_recover() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .recover(recovery_try_parse_num)
      .recover(recovery_neg99)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), -99);
  assert_eq!(Parser::new().apply(p).parse_str("5").unwrap(), 5);
}

#[test]
fn recover_both_fail_propagates_recovery_error() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num.recover(recovery_fail).parse_input(inp)
  }
  assert!(Parser::new().apply(p).parse_str("+").is_err());
}

// ── inplace_recover: primary succeeds ────────────────────────────────────────

#[test]
fn inplace_recover_primary_succeeds_returns_primary_value() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .inplace_recover(inplace_recovery_neg99)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("7").unwrap(), 7);
}

// ── inplace_recover: primary fails, recovery called ──────────────────────────

#[test]
fn inplace_recover_fallback_used_on_failure() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .inplace_recover(inplace_recovery_neg99)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), -99);
}

#[test]
fn inplace_recover_fallback_on_empty() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .inplace_recover(inplace_recovery_zero)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("").unwrap(), 0);
}

#[test]
fn inplace_recover_recovery_also_fails() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .inplace_recover(inplace_recovery_fail)
      .parse_input(inp)
  }
  assert!(Parser::new().apply(p).parse_str("+").is_err());
}

#[test]
fn inplace_recover_receives_checkpoint() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .inplace_recover(inplace_recovery_42)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), 42);
}

// ── inplace_recover: handler receives the start-position cursor ───────────────

#[test]
fn recoverer_receives_cursor_position_matches() {
  // The in-place recoverer is handed a `Cursor` (not a `Checkpoint`) marking where the
  // primary parser began. For input "1 +" the two-number primary consumes `1`, then
  // fails on `+`; the handed cursor must equal the pre-parse start (offset 0), distinct
  // from the advanced error position the (non-backtracking) input is left at.
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_two_nums
      .inplace_recover(inplace_recovery_check_start)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("1 +").unwrap(), 7);
}

#[test]
fn inplace_recover_chained() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .inplace_recover(inplace_recovery_fail)
      .inplace_recover(inplace_recovery_hundred)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), 100);
}

// ── recover used in sequence ──────────────────────────────────────────────────

#[test]
fn recover_in_then_sequence() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let a = parse_num.parse_input(inp)?;
    let b = parse_num.recover(recovery_zero).parse_input(inp)?;
    Ok((a, b))
  }
  assert_eq!(Parser::new().apply(p).parse_str("3 5").unwrap(), (3, 5));
  assert_eq!(Parser::new().apply(p).parse_str("3 +").unwrap(), (3, 0));
}

#[test]
fn inplace_recover_in_then_sequence() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let a = parse_num.parse_input(inp)?;
    let b = parse_num
      .inplace_recover(inplace_recovery_neg99)
      .parse_input(inp)?;
    Ok((a, b))
  }
  assert_eq!(Parser::new().apply(p).parse_str("10 20").unwrap(), (10, 20));
  assert_eq!(Parser::new().apply(p).parse_str("10 +").unwrap(), (10, -99));
}
