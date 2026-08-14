#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! The pratt recursion floor, and the RHS channel as the only end-of-expression authority.
//!
//! Three properties, one per section:
//!
//! * **The loop does not pre-gate on position.** Where an expression ends is what the RHS
//!   parser says at the consumer's own frontier — never what a lookahead did to the
//!   scanner's.
//! * **The floor is a bound, not a step.** A left- or non-associative operator's right
//!   operand admits powers strictly above it; a right-associative one's admits its own power
//!   too. Nothing computes a neighbouring level, so the rule holds at the ends of the ladder
//!   and on a ladder with gaps.
//! * **A report the floor admitted must have consumed.** The driver checks it at the report
//!   boundary — before the fold — and refuses the cycle outright; in debug it also names the
//!   contract violation.

mod common;

use core::sync::atomic::{AtomicUsize, Ordering};
use tokora::EmitterView;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, SimpleSpan,
  emitter::PrattEmitter,
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
    token::UnexpectedTokenOf,
  },
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  span::Spanned,
  token::PrattToken,
};

use common::{Power, TestLexer, Token};

#[derive(Debug)]
struct FloorError;

impl From<()> for FloorError {
  fn from(_: ()) -> Self {
    FloorError
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, TestLexer<'inp>>> for FloorError {
  fn from(_: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Self {
    FloorError
  }
}
impl From<UnexpectedEoLhs> for FloorError {
  fn from(_: UnexpectedEoLhs) -> Self {
    FloorError
  }
}
impl From<UnexpectedEoRhs> for FloorError {
  fn from(_: UnexpectedEoRhs) -> Self {
    FloorError
  }
}
impl From<RecursionLimitReached> for FloorError {
  fn from(_: RecursionLimitReached) -> Self {
    FloorError
  }
}
impl From<NonAssociativeChain> for FloorError {
  fn from(_: NonAssociativeChain) -> Self {
    FloorError
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for FloorError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    FloorError
  }
}
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for FloorError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    FloorError
  }
}

// The signed sentinel idiom, kept deliberately un-migrated in this file: a below-default
// postfix power standing for "not an operator here". It is what downstream grammars written
// against the pre-`End` vocabulary look like, and the floor alone must keep it out of every
// real operator's right operand.
const SENTINEL: Power = Power(-1);
const PREC_SUM: Power = Power(1);
const PREC_PROD: Power = Power(2);

// ── Typed flavor: shared LHS and folds ───────────────────────────────────────

fn num_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(FloorError),
    },
    None => Err(FloorError),
  }
}

fn fold_prefix_neg<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  Ok(-operand)
}

fn fold_postfix_identity<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  Ok(operand)
}

// ═══════════════════════════════════════════════════════════════════════════════
// The loop does not pre-gate on position
// ═══════════════════════════════════════════════════════════════════════════════

fn sum_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  let sentinel = PrattRHS::Postfix(Precedenced::new((), SENTINEL));
  match inp.next()? {
    None => Ok(sentinel),
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        PREC_SUM,
      ))),
      _ => Ok(sentinel),
    },
  }
}

fn fold_infix_add<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  Ok(left + right)
}

fn sum_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  pratt(
    num_lhs,
    sum_rhs,
    fold_prefix_neg,
    fold_infix_add,
    fold_postfix_identity,
  )
  .parse_input(inp)
}

fn sum_expr_preloaded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  use generic_arraydeque::typenum::U3;
  let _ = inp.peek::<U3>()?;
  sum_expr(inp)
}

/// The typed driver, like the token one, must be indifferent to a cache filled through the
/// end of input. `1 + 2` with a U3 window peeked first used to truncate to `1`: the loop's
/// pre-gate read the scanner's frontier, which the fill had already pushed to the buffer end
/// while `+ 2` sat cached in front of the consumer.
#[test]
fn typed_driver_does_not_truncate_when_the_cache_is_preloaded_through_eoi() {
  let control: i64 = Parser::new().apply(sum_expr).parse_str("1 + 2").unwrap();
  let preloaded: i64 = Parser::new()
    .apply(sum_expr_preloaded)
    .parse_str("1 + 2")
    .unwrap();
  assert_eq!((control, preloaded), (3, 3));
}

static TRIVIA_RHS_CALLS: AtomicUsize = AtomicUsize::new(0);

fn counted_sum_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  TRIVIA_RHS_CALLS.fetch_add(1, Ordering::Relaxed);
  sum_rhs(inp)
}

fn counted_sum_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  pratt(
    num_lhs,
    counted_sum_rhs,
    fold_prefix_neg,
    fold_infix_add,
    fold_postfix_identity,
  )
  .parse_input(inp)
}

/// How many times the RHS parser is invoked is a function of the token stream alone.
///
/// The pre-gate's "protection" of the RHS parser was history-dependent: with no trailing
/// trivia it fired and the RHS parser was never called at exhaustion (one call, for the
/// `+`); with a single trailing space the frontier stayed short of the buffer end and the
/// RHS parser *was* called at exhaustion, by both the inner recursion and the outer loop
/// (three calls) — same tokens, same grammar. Exhaustion is now a uniform RHS outcome, so
/// the two histories agree.
#[test]
fn rhs_invocation_count_does_not_depend_on_trailing_trivia() {
  TRIVIA_RHS_CALLS.store(0, Ordering::Relaxed);
  let flush: i64 = Parser::new()
    .apply(counted_sum_expr)
    .parse_str("1 + 2")
    .unwrap();
  let calls_flush = TRIVIA_RHS_CALLS.swap(0, Ordering::Relaxed);

  let trailing: i64 = Parser::new()
    .apply(counted_sum_expr)
    .parse_str("1 + 2 ")
    .unwrap();
  let calls_trailing = TRIVIA_RHS_CALLS.swap(0, Ordering::Relaxed);

  assert_eq!((flush, trailing), (3, 3));
  assert_eq!(
    (calls_flush, calls_trailing),
    (3, 3),
    "the RHS parser is consulted at exhaustion in both the inner recursion and the outer \
     loop, whichever way the input ends"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The floor is a bound, not a step — typed flavor
// ═══════════════════════════════════════════════════════════════════════════════

static DIV_POSTFIX_FOLDS: AtomicUsize = AtomicUsize::new(0);

fn div_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  let sentinel = PrattRHS::Postfix(Precedenced::new((), SENTINEL));
  match inp.next()? {
    None => Ok(sentinel),
    // Right-associative at the default floor: the adversarial shape. Its right operand's
    // bound is `Power(0)`, one step above the sentinel.
    Some(tok) => match tok.into_data() {
      Token::Slash => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(()),
        Power(0),
      ))),
      _ => Ok(sentinel),
    },
  }
}

fn div_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  Ok(if right == 0 { 0 } else { left / right })
}

fn div_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  DIV_POSTFIX_FOLDS.fetch_add(1, Ordering::Relaxed);
  Ok(operand)
}

fn div_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(usize, bool), FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  DIV_POSTFIX_FOLDS.store(0, Ordering::Relaxed);
  let _ = pratt(
    num_lhs,
    div_rhs,
    fold_prefix_neg,
    div_fold_infix,
    div_fold_postfix,
  )
  .parse_input(inp)?;
  let rparen_remains = inp
    .try_expect(|t| matches!(t.data(), Token::RParen))?
    .is_some();
  Ok((DIV_POSTFIX_FOLDS.load(Ordering::Relaxed), rparen_remains))
}

/// An un-migrated sentinel grammar keeps its closer: the floor alone is enough.
///
/// `/` is Right-associative at `Power(0)`, one dense level above the sentinel. The
/// right-associative floor used to descend to `prev(0)` = `-1` — exactly the sentinel's
/// power — so on `1 / 2 )` the sentinel *bound* as a phantom postfix and the `)`, which is
/// not part of this expression at all, was consumed by it. The floor now stops at the
/// operator's own power, so no recursion ever descends below a real operator and the closer
/// is left for the surrounding grammar.
///
/// This is the compatibility half of the repair: the fixture here is deliberately written
/// the pre-`End` way, so it pins that grammars that never migrate stop stealing tokens.
#[test]
fn legacy_sentinel_grammar_no_longer_binds_a_phantom_postfix() {
  let (postfix_folds, rparen_remains): (usize, bool) =
    Parser::new().apply(div_expr).parse_str("1 / 2 )").unwrap();
  assert_eq!(
    (postfix_folds, rparen_remains),
    (0, true),
    "no phantom postfix fold, and `)` is still on the input"
  );
}

// ── Grouping and the entry floor (additive pins — green before this change too) ──

fn group_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  match inp.next()? {
    None => Err(FloorError),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      Token::LParen => {
        let inner = group_expr(inp)?;
        if inp
          .try_expect(|t| matches!(t.data(), Token::RParen))?
          .is_none()
        {
          return Err(FloorError);
        }
        Ok(PrattLHS::Operand(inner))
      }
      _ => Err(FloorError),
    },
  }
}

fn group_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  let sentinel = PrattRHS::Postfix(Precedenced::new((), SENTINEL));
  match inp.next()? {
    None => Ok(sentinel),
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        PREC_SUM,
      ))),
      Token::Star => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        PREC_PROD,
      ))),
      _ => Ok(sentinel),
    },
  }
}

fn group_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  Ok(if *op.precedence() == PREC_PROD {
    left * right
  } else {
    left + right
  })
}

fn group_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  pratt(
    group_lhs,
    group_rhs,
    fold_prefix_neg,
    group_fold_infix,
    fold_postfix_identity,
  )
  .parse_input(inp)
}

fn group_expr_from<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  floor: Power,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  pratt(
    group_lhs,
    group_rhs,
    fold_prefix_neg,
    group_fold_infix,
    fold_postfix_identity,
  )
  .min_precedence(floor)
  .parse_input(inp)
}

fn group_expr_at_sum<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  group_expr_from(inp, PREC_SUM)
}

fn group_expr_at_prod<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(i64, bool), FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  let v = group_expr_from(inp, PREC_PROD)?;
  let plus_remains = inp
    .try_expect(|t| matches!(t.data(), Token::Plus))?
    .is_some();
  Ok((v, plus_remains))
}

/// Additive pin (green before this change): a parenthesised group parses through the LHS,
/// and the operator after it still binds at the outer level.
#[test]
fn additive_grouping_still_folds_around_the_group() {
  let r: i64 = Parser::new()
    .apply(group_expr)
    .parse_str("( 1 + 2 ) * 3")
    .unwrap();
  assert_eq!(r, 9);
}

/// Additive pin (green before this change): `min_precedence` is *inclusive*. An operator at
/// exactly the configured floor is consumed; one below it is left on the input.
#[test]
fn additive_min_precedence_is_inclusive_at_exactly_the_floor() {
  let at_floor: i64 = Parser::new()
    .apply(group_expr_at_sum)
    .parse_str("1 + 2")
    .unwrap();
  assert_eq!(at_floor, 3, "`+` sits exactly at the floor and is consumed");

  let (below_floor, plus_remains): (i64, bool) = Parser::new()
    .apply(group_expr_at_prod)
    .parse_str("1 + 2")
    .unwrap();
  assert_eq!(
    (below_floor, plus_remains),
    (1, true),
    "`+` is one level below the floor: left on the input for the caller"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// A report that consumed nothing is refused at the report boundary
// ═══════════════════════════════════════════════════════════════════════════════

static STALL_POSTFIX_FOLDS: AtomicUsize = AtomicUsize::new(0);

fn stalling_rhs<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  // The contract violation, in its purest form: report an admitted operator, consume
  // nothing. Mid-input, so this is about consumption and not about exhaustion.
  Ok(PrattRHS::Postfix(Precedenced::new((), Power(0))))
}

fn stall_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  STALL_POSTFIX_FOLDS.fetch_add(1, Ordering::Relaxed);
  Ok(operand)
}

fn stalling_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<usize, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = FloorError>,
{
  STALL_POSTFIX_FOLDS.store(0, Ordering::Relaxed);
  let _ = pratt(
    num_lhs,
    stalling_rhs,
    fold_prefix_neg,
    fold_infix_add,
    stall_fold_postfix,
  )
  .parse_input(inp)?;
  Ok(STALL_POSTFIX_FOLDS.load(Ordering::Relaxed))
}

/// An RHS report the floor admitted and that consumed nothing is refused where it is made.
///
/// Debug builds name the violation and panic — that message is the diagnostic a grammar
/// author needs. Release builds raise the terminal end-of-RHS error instead: the check sits at
/// the report boundary, so it runs before the fold, and no phantom operator is ever committed.
/// Ending the expression with an `Ok` here would hand the caller a truncated parse with no
/// diagnostic on any channel, which is the failure the position pre-gate used to produce.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "an Infix/Postfix report consumed nothing")
)]
fn a_report_that_consumed_nothing_is_refused_at_the_report_boundary() {
  let outcome: Result<usize, FloorError> = Parser::new().apply(stalling_expr).parse_str("1 2 3");
  assert!(
    outcome.is_err(),
    "release: the stalled report is refused, not folded — got {outcome:?}"
  );
  assert_eq!(
    STALL_POSTFIX_FOLDS.load(Ordering::Relaxed),
    0,
    "release: the report boundary runs before the fold, so no phantom fold ran"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Token flavor
// ═══════════════════════════════════════════════════════════════════════════════
//
// The folds evaluate into a `Token::Num` payload, so each oracle below is a plain number
// rather than a global counter — two grammars share this file and its tests run in
// parallel.

const DENSE_MUL: Power = Power(3); // `*` — Right-associative (the adversarial shape)
const DENSE_ADD: Power = Power(2); // `+` — Left-associative, one dense level below

impl PrattToken<'_, i64, Power> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    match self {
      Token::Num(_) => Some(PrattLHS::Operand(())),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>> {
    match self {
      Token::Star => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(()),
        DENSE_MUL,
      ))),
      Token::Plus => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        DENSE_ADD,
      ))),
      _ => None,
    }
  }
}

type Tok = Spanned<Token, SimpleSpan>;

fn num_of(tok: &Tok) -> i64 {
  match tok.data() {
    Token::Num(n) => *n,
    other => panic!("a fold operand must be a number token, got {other:?}"),
  }
}

fn tok_fold_prefix<'inp, E>(
  _op: Tok,
  operand: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, FloorError> {
  Ok(operand)
}

fn tok_fold_postfix<'inp, E>(
  operand: Tok,
  _op: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, FloorError> {
  Ok(operand)
}

/// Evaluates the fold: `*` multiplies, `+` adds. The result is a synthetic `Num` token
/// spanning the folded region, so the parse's shape is readable off the final value.
fn dense_fold_infix<'inp, E>(
  left: Tok,
  right: Tok,
  infix: Spanned<PrattInfix<Token, Token, Token>, SimpleSpan>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, FloorError> {
  let (span, op) = infix.into_components();
  let (l, r) = (num_of(&left), num_of(&right));
  let value = match op {
    PrattInfix::Right(_) => l * r,
    _ => l + r,
  };
  Ok(Spanned::new(span, Token::Num(value)))
}

fn dense_token_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = FloorError> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  let out = inp.pratt::<_, _, _, i64, Power>(
    tok_fold_prefix::<Ctx::Emitter>,
    dense_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
  )?;
  Ok(num_of(&out.expect("the input opens with an operand")))
}

fn dense_token_expr_at_mul<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(i64, bool), FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = FloorError> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  let out = inp.pratt_with_min_precedence::<_, _, _, i64, Power>(
    tok_fold_prefix::<Ctx::Emitter>,
    dense_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
    DENSE_MUL,
  )?;
  let value = num_of(&out.expect("the input opens with an operand"));
  let plus_remains = inp
    .try_expect(|t| matches!(t.data(), Token::Plus))?
    .is_some();
  Ok((value, plus_remains))
}

/// The token driver's right-associative floor, on the same dense ladder as the typed one.
///
/// `*` = Right(3), `+` = Left(2). The floor used to be `lpower.prev()` = 2, which admits
/// `+` into `*`'s right operand: `2 * 3 + 4` = `2 * (3 + 4)` = 14. Right-associativity
/// admits the operator's own power, not one below it, so the answer is `(2 * 3) + 4` = 10.
#[test]
fn token_dense_right_assoc_floor_keeps_lower_operator_out_of_the_rhs() {
  let r: i64 = Parser::new()
    .apply(dense_token_expr)
    .parse_str("2 * 3 + 4")
    .unwrap();
  assert_eq!(r, 10);
}

/// The token flavor's `min_precedence` is inclusive — an operator at exactly the floor binds
/// — and the weaker operator after it stays on the input.
///
/// Not an additive pin: the second half of that is the right-floor repair. At
/// `min_precedence(DENSE_MUL)` on `2 * 3 + 4` this read `(14, false)` before — `*` bound at
/// the floor correctly, then its right operand's floor descended a level and swallowed the
/// `+` the caller had asked to keep.
#[test]
fn token_min_precedence_is_inclusive_and_keeps_the_weaker_operator() {
  let (value, plus_remains): (i64, bool) = Parser::new()
    .apply(dense_token_expr_at_mul)
    .parse_str("2 * 3 + 4")
    .unwrap();
  assert_eq!(
    (value, plus_remains),
    (6, true),
    "`*` sits exactly at the floor and binds; `+`, one level below, is left on the input"
  );
}

// ── Non-associative chains at the ladder's extreme ───────────────────────────

impl PrattToken<'_, i64, u8> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), u8>> {
    match self {
      Token::Num(_) => Some(PrattLHS::Operand(())),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), u8>> {
    match self {
      Token::Semi => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        u8::MAX,
      ))),
      Token::Comma => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        u8::MAX - 1,
      ))),
      _ => None,
    }
  }
}

/// Records the shape of the fold tree in the value: each fold appends its right operand's
/// digit, so one fold over `1 ; 2` reads `12` and a second over `; 3` reads `123`.
fn chain_fold_infix<'inp, E>(
  left: Tok,
  right: Tok,
  infix: Spanned<PrattInfix<Token, Token, Token>, SimpleSpan>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, FloorError> {
  let value = num_of(&left) * 10 + num_of(&right);
  Ok(Spanned::new(infix.into_span(), Token::Num(value)))
}

fn chain_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(i64, bool), FloorError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = FloorError> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  let out = inp.pratt::<_, _, _, i64, u8>(
    tok_fold_prefix::<Ctx::Emitter>,
    chain_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
  )?;
  let value = num_of(&out.expect("the input opens with an operand"));
  let second_op_remains = inp
    .try_expect(|t| matches!(t.data(), Token::Semi | Token::Comma))?
    .is_some();
  Ok((value, second_op_remains))
}

/// A non-associative chain is rejected, at the top of the ladder like anywhere else.
///
/// `;` is `Neither(u8::MAX)`. `1 ; 2 ; 3` folds once and then fails on the second `;`, which
/// stays parked on the input. This pins the same floor regression it always did: the floor used
/// to be `next(MAX)`, which saturates back to `MAX`, so the recursion itself admitted the second
/// `;` and both folds ran — an `Ok(123)` the repeat guard never got a chance to refuse. Reaching
/// the guard at all is what the assertion below proves; that it now *fails* rather than
/// truncating is the contract R2 settled.
#[test]
fn token_neither_chain_is_rejected_at_the_top_of_the_ladder() {
  let outcome: Result<(i64, bool), FloorError> =
    Parser::new().apply(chain_expr).parse_str("1 ; 2 ; 3");
  assert!(
    outcome.is_err(),
    "the second `;` must be rejected, not folded and not left as a remainder; got {outcome:?}"
  );
}

/// The non-associative repeat guard compares the operator's true power.
///
/// `;` is `Neither(MAX)` and `,` is `Neither(MAX - 1)` — different powers, so there is no
/// non-associative repeat and `,` must bind. The token driver used to hand the fold a
/// *transformed* power and recover the original by inverse arithmetic; under saturation
/// that reconstruction produced `MAX - 1`, the guard equated it with `,`'s real power, and
/// the loop stopped after one fold, silently abandoning `, 3`. The true power is now
/// carried through.
#[test]
fn token_neither_repeat_guard_compares_the_true_power() {
  let (value, second_remains): (i64, bool) = Parser::new()
    .apply(chain_expr)
    .parse_str("1 ; 2 , 3")
    .unwrap();
  assert_eq!(
    (value, second_remains),
    (123, false),
    "`,` is a different operator: both folds run and nothing is left behind"
  );
}
