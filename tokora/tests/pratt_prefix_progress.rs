#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The typed pratt driver refuses a `PrattLHS::Prefix` report that consumed nothing.
//!
//! The RHS report boundary was guarded first: the driver reads the watermark the instant
//! `parse_pratt_rhs` returns and refuses an admitted `Infix`/`Postfix` that consumed nothing,
//! before it recurses and before any fold. The LHS path is the same shape one site over — a
//! [`PrattLHS::Prefix`](tokora::parser::PrattLHS) report goes to `parse(..)` at the *same input
//! position*, then to `fold_prefix`, then to the CST wrap — and it carried no such check.
//!
//! Nothing declines a `Prefix`. `PrattLHS` has two variants and no floor applies to either;
//! unlike an RHS report, which the floor may legitimately reject having consumed nothing, a
//! `Prefix` report is acted on the moment it is made. "Reported" and "accepted" are the same
//! event here, so the guard holds every one of them and owes no exemption.
//!
//! Two payloads, both measured against the unguarded driver:
//!
//! * **Unbounded descent.** A grammar that reports `Prefix` after a peek recursed at the same
//!   position forever. Uncapped, on `1`, it died at roughly 700 frames:
//!   `thread has overflowed its stack / fatal runtime error: stack overflow, aborting`
//!   (SIGABRT). The fixture below caps itself so the suite can report the descent instead of
//!   aborting the test binary — the cap is the *fixture's*, never the driver's.
//! * **A phantom prefix fold.** Isolated from the recursion by a fixture that reports `Prefix`
//!   once and then parses an operand: `1` returned `Ok(-1)`, one prefix fold applied for an
//!   operator that appears nowhere in the source, no diagnostic on any channel.
//!
//! Both are now refused at the report itself: debug names the contract violation and panics,
//! release raises the terminal end-of-LHS error.

mod common;

use core::sync::atomic::{AtomicUsize, Ordering};

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  error::{UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot, token::UnexpectedTokenOf},
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
};

use common::{Power, TestLexer, Token};

#[derive(Debug)]
struct PrefixError;

impl From<()> for PrefixError {
  fn from(_: ()) -> Self {
    PrefixError
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, TestLexer<'inp>>> for PrefixError {
  fn from(_: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Self {
    PrefixError
  }
}
impl From<UnexpectedEoLhs> for PrefixError {
  fn from(_: UnexpectedEoLhs) -> Self {
    PrefixError
  }
}
impl From<UnexpectedEoRhs> for PrefixError {
  fn from(_: UnexpectedEoRhs) -> Self {
    PrefixError
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for PrefixError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    PrefixError
  }
}
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for PrefixError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    PrefixError
  }
}

// ── Shared channels: the RHS ends immediately, so only the LHS path is under test ────────

/// The expression never continues. Every cycle below is the LHS path and nothing else.
fn end_rhs<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  Ok(PrattRHS::End)
}

// Each test owns its counters: the suite runs them in parallel, and a shared tally makes one
// test's folds another's.
static DESCENT_FOLDS: AtomicUsize = AtomicUsize::new(0);
static ONCE_FOLDS: AtomicUsize = AtomicUsize::new(0);

fn descent_fold_prefix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  DESCENT_FOLDS.fetch_add(1, Ordering::Relaxed);
  Ok(-o)
}

fn once_fold_prefix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  ONCE_FOLDS.fetch_add(1, Ordering::Relaxed);
  Ok(-o)
}

fn plain_fold_prefix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  Ok(-o)
}

fn fold_infix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  Ok(l + r)
}

fn fold_postfix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  Ok(o)
}

// ── Payload one: the descent ─────────────────────────────────────────────────────────────

static DESCENTS: AtomicUsize = AtomicUsize::new(0);

/// The descent cap. Nothing in this grammar consumes before the recursive call, so the driver
/// never stops on its own; uncapped, the parse aborts the process with a stack overflow at
/// roughly 700 frames. The cap belongs to the fixture — it is what makes this test report the
/// descent rather than kill the test binary.
const DESCENT_CAP: usize = 64;

/// Reports a prefix operator after a **peek**, consuming nothing, until the fixture's own cap
/// releases it.
fn peeking_prefix_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  if DESCENTS.load(Ordering::Relaxed) >= DESCENT_CAP {
    return match inp.next()? {
      Some(tok) => match tok.into_data() {
        Token::Num(n) => Ok(PrattLHS::Operand(n)),
        _ => Err(PrefixError),
      },
      None => Err(PrefixError),
    };
  }
  // A peek moves the cache-front cursor across skipped trivia and commits nothing, which is
  // exactly the movement a progress metric must not read as consumption.
  if inp.peek_one()?.is_some() {
    DESCENTS.fetch_add(1, Ordering::Relaxed);
    Ok(PrattLHS::Prefix(Precedenced::new((), Power(0))))
  } else {
    Ok(PrattLHS::Operand(0))
  }
}

fn peeking_prefix_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  pratt(
    peeking_prefix_lhs,
    end_rhs,
    descent_fold_prefix,
    fold_infix,
    fold_postfix,
  )
  .parse_input(inp)
}

/// A zero-width `Prefix` report is refused **before** the recursion.
///
/// Without the guard the driver descends once per report and folds once per level on the way
/// back up, stopped only by the fixture's cap — and uncapped, by a stack overflow. With it,
/// the very first report is refused where it is made: one report, no descent, no fold.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "a Prefix report consumed nothing")
)]
fn a_zero_width_prefix_report_is_refused_before_the_recursion() {
  DESCENTS.store(0, Ordering::Relaxed);
  DESCENT_FOLDS.store(0, Ordering::Relaxed);
  let outcome: Result<i64, PrefixError> = Parser::new().apply(peeking_prefix_expr).parse_str("1");
  assert!(
    outcome.is_err(),
    "release: the stalled prefix report is refused, not folded — got {outcome:?} after {} \
     descent(s)",
    DESCENTS.load(Ordering::Relaxed)
  );
  assert_eq!(
    DESCENTS.load(Ordering::Relaxed),
    1,
    "the driver never descended: the first report is refused where it is made"
  );
  assert_eq!(
    DESCENT_FOLDS.load(Ordering::Relaxed),
    0,
    "no phantom prefix fold ran"
  );
}

// ── Payload two: the phantom fold, isolated from the descent ─────────────────────────────

static CALLS: AtomicUsize = AtomicUsize::new(0);

/// Reports `Prefix` after a peek on its **first** call, then parses operands normally. The
/// recursion terminates, so what is left is the fold alone.
fn once_prefix_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  if CALLS.fetch_add(1, Ordering::Relaxed) == 0 {
    inp.peek_one()?;
    return Ok(PrattLHS::Prefix(Precedenced::new((), Power(0))));
  }
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(PrefixError),
    },
    None => Err(PrefixError),
  }
}

fn once_prefix_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  pratt(
    once_prefix_lhs,
    end_rhs,
    once_fold_prefix,
    fold_infix,
    fold_postfix,
  )
  .parse_input(inp)
}

/// A zero-width `Prefix` report is refused **before** the fold.
///
/// The recursion terminates in this grammar, so the descent is not what is under test: the
/// fold is. Without the guard the source `1` — one number, no operator — comes back `Ok(-1)`,
/// negated by an operator that is not in it, with no diagnostic on any channel. That is the
/// silent-wrong-answer shape the RHS guard exists to delete, and it is refused here for the
/// same reason.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "a Prefix report consumed nothing")
)]
fn a_zero_width_prefix_report_is_refused_before_the_fold() {
  CALLS.store(0, Ordering::Relaxed);
  ONCE_FOLDS.store(0, Ordering::Relaxed);
  let outcome: Result<i64, PrefixError> = Parser::new().apply(once_prefix_expr).parse_str("1");
  assert!(
    outcome.is_err(),
    "release: the stalled prefix report is refused, not folded — got {outcome:?}"
  );
  assert_eq!(
    ONCE_FOLDS.load(Ordering::Relaxed),
    0,
    "no phantom prefix fold ran: the report is refused before the fold"
  );
}

// ── Keep-green: what a prefix report is allowed to be ────────────────────────────────────

/// A real prefix operator: consumed by the report that names it.
fn consuming_prefix_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  // The lookahead first, so the operator this fixture reports is reached through exactly the
  // cursor movement the guard's metric must ignore.
  if inp.peek_one()?.is_none() {
    return Err(PrefixError);
  }
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new((), Power(9)))),
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(PrefixError),
    },
    None => Err(PrefixError),
  }
}

fn consuming_prefix_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, PrefixError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrefixError>,
{
  pratt(
    consuming_prefix_lhs,
    end_rhs,
    plain_fold_prefix,
    fold_infix,
    fold_postfix,
  )
  .parse_input(inp)
}

/// A prefix report that consumed its operator is a normal report, lookahead and trailing
/// trivia included. This pin stays green across the guard.
#[test]
fn a_prefix_report_that_consumed_its_operator_parses_normally() {
  for (src, want) in [("- 1", -1i64), ("- 1 ", -1), ("1", 1), ("- - 1", 1)] {
    let got: i64 = Parser::new()
      .apply(consuming_prefix_expr)
      .parse_str(src)
      .unwrap_or_else(|e| panic!("`{src}` should parse — got {e:?}"));
    assert_eq!(got, want, "`{src}`");
  }
}
