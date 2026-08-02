#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The typed pratt driver refuses an RHS report the floor admitted and that consumed nothing.
//!
//! **Section one — the guard has teeth at all.** Both grammars model a **downstream** fixture
//! that spells "no operator here" as a below-minimum postfix and trusts the floor to reject
//! it; what keeps them terminating is the progress guard, not that trust. Each puts such a
//! report inside the entry floor:
//!
//! * an **unsigned** `Power`, where `Power::default()` is the minimum representable value, so
//!   the lowest expressible "sentinel" is `Postfix(0)` and the entry floor admits it — there
//!   is no below-minimum value to reach for;
//! * a **signed** `Power` under the documented `min_precedence` knob lowered beneath the
//!   sentinel, which is the supported way to embed an expression in a larger grammar.
//!
//! Each is run against both endings of the same token stream: flush against the end of the
//! buffer, and with one trailing space. The trailing-space form used to spin forever — the
//! trivia kept the scanner's frontier short of the buffer end, so the old position pre-gate
//! never fired and the phantom postfix was re-reported for as long as the process ran.
//!
//! **Section two — the guard is in the right place.** It sits at the report boundary, before
//! the recursion and before any fold. A stalled report whose *recursion* or *fold* then
//! advances the input satisfies an end-of-cycle watermark, so a guard placed there sees
//! progress and never fires: the driver folds phantom operators and, when the recursion
//! repeats the same zero-width report, descends without bound. Every grammar in that section
//! parses cleanly — `Ok`, no diagnostic on any channel — against an end-of-cycle guard alone.
//!
//! Deleting the guard's hunk from `parser/pratt/expr.rs` puts the four section-one tests back
//! into an unbounded loop, and turns every section-two test back into a clean `Ok` over a
//! phantom fold.

mod common;

use core::sync::atomic::{AtomicUsize, Ordering};

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
    token::UnexpectedTokenOf,
  },
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
};

use common::{Power, TestLexer, Token};

#[derive(Debug)]
struct GuardError;

impl From<()> for GuardError {
  fn from(_: ()) -> Self {
    GuardError
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, TestLexer<'inp>>> for GuardError {
  fn from(_: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Self {
    GuardError
  }
}
impl From<UnexpectedEoLhs> for GuardError {
  fn from(_: UnexpectedEoLhs) -> Self {
    GuardError
  }
}
impl From<UnexpectedEoRhs> for GuardError {
  fn from(_: UnexpectedEoRhs) -> Self {
    GuardError
  }
}
impl From<RecursionLimitReached> for GuardError {
  fn from(_: RecursionLimitReached) -> Self {
    GuardError
  }
}
impl From<NonAssociativeChain> for GuardError {
  fn from(_: NonAssociativeChain) -> Self {
    GuardError
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for GuardError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    GuardError
  }
}
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for GuardError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    GuardError
  }
}

// ── Unsigned power: no below-minimum value exists ────────────────────────────

fn u8_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), u8>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(GuardError),
    },
    None => Err(GuardError),
  }
}

fn u8_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  // The sentinel idiom, unsigned: the lowest power that exists is 0, and the entry floor
  // admits it.
  let sentinel = PrattRHS::Postfix(Precedenced::new((), 0u8));
  match inp.next()? {
    None => Ok(sentinel),
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), 5u8))),
      _ => Ok(sentinel),
    },
  }
}

fn u8_fold_prefix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), u8>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(-o)
}

fn u8_fold_infix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, u8>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(l + r)
}

fn u8_fold_postfix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), u8>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(o)
}

fn u8_expr<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  pratt(
    u8_lhs,
    u8_rhs,
    u8_fold_prefix,
    u8_fold_infix,
    u8_fold_postfix,
  )
  .parse_input(inp)
}

/// Unsigned power, flush against the end of the buffer.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn unsigned_sentinel_at_the_entry_floor_terminates() {
  let outcome: Result<i64, GuardError> = Parser::new().apply(u8_expr).parse_str("1 + 2");
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?}"
  );
}

/// Unsigned power, one trailing space. This spun forever before the guard landed.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn unsigned_sentinel_at_the_entry_floor_terminates_under_trailing_trivia() {
  let outcome: Result<i64, GuardError> = Parser::new().apply(u8_expr).parse_str("1 + 2 ");
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?}"
  );
}

// ── Signed power under a lowered `min_precedence` ────────────────────────────

fn signed_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(GuardError),
    },
    None => Err(GuardError),
  }
}

fn signed_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  let sentinel = PrattRHS::Postfix(Precedenced::new((), Power(-1)));
  match inp.next()? {
    None => Ok(sentinel),
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        Power(5),
      ))),
      _ => Ok(sentinel),
    },
  }
}

fn signed_fold_prefix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(-o)
}

fn signed_fold_infix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(l + r)
}

fn signed_fold_postfix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  o: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(o)
}

fn signed_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  pratt(
    signed_lhs,
    signed_rhs,
    signed_fold_prefix,
    signed_fold_infix,
    signed_fold_postfix,
  )
  .min_precedence(Power(-5))
  .parse_input(inp)
}

/// Signed power with `min_precedence` lowered beneath the sentinel, flush EOF.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn signed_sentinel_under_a_lowered_min_precedence_terminates() {
  let outcome: Result<i64, GuardError> = Parser::new().apply(signed_expr).parse_str("1 + 2");
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?}"
  );
}

/// Signed power with `min_precedence` lowered beneath the sentinel, one trailing space.
/// This spun forever before the guard landed.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn signed_sentinel_under_a_lowered_min_precedence_terminates_under_trailing_trivia() {
  let outcome: Result<i64, GuardError> = Parser::new().apply(signed_expr).parse_str("1 + 2 ");
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The guard sits at the report boundary, not at the foot of the cycle
// ═══════════════════════════════════════════════════════════════════════════════
//
// Every grammar below reports an admitted operator having consumed nothing, and then lets
// something *else* — the recursive operand parse, or the fold — move the input. A watermark
// compared only at the end of the cycle reads that as progress, so it never fires: each of
// these parses `Ok` with a phantom fold committed, or descends without bound.
//
// Each RHS decides by `peek_one`, which is also what makes these the metric's pins: a peek
// moves the cache-front cursor (`InputRef::cursor`) across skipped trivia without committing
// anything, so a guard keyed on that cursor rather than on committed consumption
// (`span().end()`) reads every one of these zero-width reports as progress and misses them
// exactly as an end-of-cycle guard does.

/// "The expression stops here", in the same below-minimum spelling the first section uses:
/// a postfix one level under the entry floor, reported without consuming. The floor rejects
/// it, the driver rolls the deciding read back, and the report boundary is never reached — so
/// every grammar below can decline without tripping the guard it is written to trip.
fn decline() -> PrattRHS<(), (), (), (), Power> {
  PrattRHS::Postfix(Precedenced::new((), Power(-1)))
}

// ── Infix: the recursion is what advances ────────────────────────────────────

static INFIX_FOLDS: AtomicUsize = AtomicUsize::new(0);

/// Reports a left-associative infix at the entry floor whenever a token is still there —
/// and consumes nothing to do it. The operand the driver recurses for is the very token this
/// report left behind, so the recursion advances and an end-of-cycle watermark is satisfied.
fn peeking_infix_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  if inp.peek_one()?.is_some() {
    Ok(PrattRHS::Infix(Precedenced::new(
      PrattInfix::Left(()),
      Power(0),
    )))
  } else {
    Ok(decline())
  }
}

fn counting_fold_infix<'inp, Ctx>(
  _i: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  INFIX_FOLDS.fetch_add(1, Ordering::Relaxed);
  Ok(l + r)
}

fn peeking_infix_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  pratt(
    signed_lhs,
    peeking_infix_rhs,
    signed_fold_prefix,
    counting_fold_infix,
    signed_fold_postfix,
  )
  .parse_input(inp)
}

/// An admitted infix report that consumed nothing is refused **before** the recursion.
///
/// Against an end-of-cycle watermark this grammar parses `1 2 3` as `(1 + 2) + 3` — two
/// phantom infix folds over operands the report never took — and returns `Ok(6)` with no
/// diagnostic on any channel, because the recursive operand parse advanced the input inside
/// the very cycle the watermark was measuring.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn an_admitted_infix_report_that_consumed_nothing_is_refused_before_the_recursion() {
  INFIX_FOLDS.store(0, Ordering::Relaxed);
  let outcome: Result<i64, GuardError> = Parser::new().apply(peeking_infix_expr).parse_str("1 2 3");
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?} after {} fold(s)",
    INFIX_FOLDS.load(Ordering::Relaxed)
  );
  assert_eq!(
    INFIX_FOLDS.load(Ordering::Relaxed),
    0,
    "no phantom infix fold ran: the report is refused before the recursion and the fold"
  );
}

// ── Postfix: the fold is what advances ───────────────────────────────────────

static POSTFIX_FOLDS: AtomicUsize = AtomicUsize::new(0);

/// Reports a postfix at the entry floor whenever a token is still there, consuming nothing.
fn peeking_postfix_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  if inp.peek_one()?.is_some() {
    Ok(PrattRHS::Postfix(Precedenced::new((), Power(0))))
  } else {
    Ok(decline())
  }
}

/// The fold — not the report — is what moves the input here. A postfix report never
/// recurses, so this is the other half of the finding: a fold that advances also satisfies an
/// end-of-cycle watermark.
fn consuming_fold_postfix<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  POSTFIX_FOLDS.fetch_add(1, Ordering::Relaxed);
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(operand + n),
      _ => Ok(operand),
    },
    None => Ok(operand),
  }
}

fn peeking_postfix_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  pratt(
    signed_lhs,
    peeking_postfix_rhs,
    signed_fold_prefix,
    signed_fold_infix,
    consuming_fold_postfix,
  )
  .parse_input(inp)
}

/// An admitted postfix report that consumed nothing is refused **before** the fold.
///
/// Against an end-of-cycle watermark this grammar returns `Ok(6)` for `1 2 3`: two phantom
/// postfix folds, each of which ate the operand the report had declined to take.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn an_admitted_postfix_report_that_consumed_nothing_is_refused_before_the_fold() {
  POSTFIX_FOLDS.store(0, Ordering::Relaxed);
  let outcome: Result<i64, GuardError> =
    Parser::new().apply(peeking_postfix_expr).parse_str("1 2 3");
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?} after {} fold(s)",
    POSTFIX_FOLDS.load(Ordering::Relaxed)
  );
  assert_eq!(
    POSTFIX_FOLDS.load(Ordering::Relaxed),
    0,
    "no phantom postfix fold ran: the report is refused before the fold"
  );
}

// ── The recursion repeats the same zero-width report ─────────────────────────

static DESCENTS: AtomicUsize = AtomicUsize::new(0);

/// The descent cap. Nothing in the grammar below consumes *before* the recursive call, so
/// against an end-of-cycle watermark the descent is unbounded and ends in a stack overflow.
/// The cap is what makes this test fail fast instead of overflowing: the fixture, not the
/// driver, is what stops it.
const DESCENT_CAP: usize = 64;

/// An operand that consumes nothing at all.
fn zero_width_lhs<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(PrattLHS::Operand(0))
}

/// Reports the same **right-associative** infix at every depth, consuming nothing. Right
/// associativity is what makes the recursion admit its own power again, so the inner call
/// re-reports it and recurses in turn.
fn repeating_right_rhs<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  if DESCENTS.load(Ordering::Relaxed) >= DESCENT_CAP {
    return Ok(decline());
  }
  DESCENTS.fetch_add(1, Ordering::Relaxed);
  Ok(PrattRHS::Infix(Precedenced::new(
    PrattInfix::Right(()),
    Power(0),
  )))
}

/// Consumes one token per fold, so every level that *returns* satisfies an end-of-cycle
/// watermark on its way back up — which is why that watermark never fires on the way down.
fn consuming_fold_infix<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  let _ = inp.next()?;
  Ok(l + r)
}

fn repeating_right_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  pratt(
    zero_width_lhs,
    repeating_right_rhs,
    signed_fold_prefix,
    consuming_fold_infix,
    signed_fold_postfix,
  )
  .parse_input(inp)
}

/// A recursion that repeats the same zero-width report never reaches an end-of-cycle guard.
///
/// Each level reports, recurses, and only *then* consumes, so the watermark at the foot of a
/// cycle is evaluated `DESCENT_CAP` frames deep — after the damage. The report boundary is
/// reached on the first cycle instead, so exactly one report is ever made.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn a_recursion_repeating_a_zero_width_report_is_stopped_at_the_first_one() {
  DESCENTS.store(0, Ordering::Relaxed);
  // One token per level the fold can consume on the way back up, so nothing but the report
  // guard can end this parse.
  let src = "1 ".repeat(DESCENT_CAP);
  let outcome: Result<i64, GuardError> = Parser::new().apply(repeating_right_expr).parse_str(&src);
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?} after {} descent(s)",
    DESCENTS.load(Ordering::Relaxed)
  );
  assert_eq!(
    DESCENTS.load(Ordering::Relaxed),
    1,
    "the driver never descended: the first report is refused where it is made"
  );
}

// ── Keep-green: what a zero-consumption report is still allowed to be ────────
//
// There is no legitimate *accepted* zero-consumption report — "consume what you report" is
// the `ParsePrattRHS` contract, and an accepted `Infix`/`Postfix` that took nothing leaves
// the next cycle re-reading the same input. So this pin covers the two adjacent shapes that
// legitimately consume nothing and must stay green: a report the floor declines, and a
// report reached through a lookahead that then does consume.

/// Reports a postfix one level *below* the entry floor, having consumed nothing, at every
/// position including exhaustion.
fn below_floor_rhs<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  Ok(decline())
}

fn below_floor_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(i64, bool), GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  let v = pratt(
    signed_lhs,
    below_floor_rhs,
    signed_fold_prefix,
    signed_fold_infix,
    signed_fold_postfix,
  )
  .parse_input(inp)?;
  let leftover = matches!(inp.next()?.map(|t| t.into_data()), Some(Token::Num(_)));
  Ok((v, leftover))
}

/// A report the floor declines may consume nothing: it is not this expression's operator, and
/// the driver rolls the deciding read back and hands the token to the caller. The guard sits
/// behind the floor check, so this is not "reject anything that looks suspicious".
#[test]
fn a_below_floor_report_that_consumed_nothing_still_ends_the_loop_cleanly() {
  let (v, leftover): (i64, bool) = Parser::new()
    .apply(below_floor_expr)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(
    (v, leftover),
    (1, true),
    "the below-floor report ends the expression and leaves its token for the caller"
  );
}

/// Reports an infix only after a lookahead, and consumes the operator it reports.
fn lookahead_then_consume_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  // The lookahead moves the cache-front cursor onto the operator — across the space in
  // front of it — while committing nothing.
  if inp.peek_one()?.is_none() {
    return Ok(decline());
  }
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        Power(5),
      ))),
      _ => Ok(decline()),
    },
    None => Ok(decline()),
  }
}

fn lookahead_then_consume_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, GuardError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = GuardError>,
{
  pratt(
    signed_lhs,
    lookahead_then_consume_rhs,
    signed_fold_prefix,
    signed_fold_infix,
    signed_fold_postfix,
  )
  .parse_input(inp)
}

/// A report that looks ahead before consuming is a normal report, trailing trivia and all.
///
/// The lookahead is exactly the movement the guard's metric must ignore in the other
/// direction too: it advances the cache-front cursor without committing, and the report that
/// follows it commits the operator. Both endings of the stream, since a trailing space is
/// what separated the two positions historically.
#[test]
fn a_report_that_looks_ahead_before_consuming_parses_normally() {
  for src in ["1 + 2", "1 + 2 "] {
    let v: i64 = Parser::new()
      .apply(lookahead_then_consume_expr)
      .parse_str(src)
      .unwrap();
    assert_eq!(
      v, 3,
      "`{src}`: the lookahead is not the report's consumption"
    );
  }
}
