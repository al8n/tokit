#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! `PrattRHS::Adjacent`: an infix continuation spelled with no token, and what stops it.
//!
//! **Section one — it is an operator, not a repetition.** The reason juxtaposition needs a
//! driver variant at all is that its right operand's floor has to be the *driver's*. A
//! production-level `while` loop, or the `Postfix`-carrying-the-right-operand workaround, both
//! choose that floor themselves and then sit outside the driver's precedence laws. So the pins
//! here are precedence pins: an adjacency at power 1 against `*` at 3 and against its spelled
//! siblings `,` (`Left`, 1) and `=` (`Neither`, 1). `1 2 * 3` grouping as `(1 · (2 * 3))` is the
//! whole requirement in one line.
//!
//! **Section two — what stops it.** A report that consumes nothing is a report the loop's own
//! progress guard cannot measure, so the driver moves the obligation onto the **right operand**
//! and charges it before the fold, the CST wrap and the next turn. The differential is one
//! grammar and two inputs: `1 2`, where the operand pays, and `1 ;`, where the LHS channel
//! answers with a zero-width operand and nothing pays. The second is refused — a `debug_assert`
//! naming the rule in a debug build, the terminal end-of-RHS report in a release one, which is
//! the same two-profile contract every other driver-raised violation carries.
//!
//! The **depth** half of the bound is pinned twice over. A zero-token continuation descends on
//! `> power` and not `>= power`, so an inner frame declines the same continuation rather than
//! re-reporting it over input nothing consumed: negatively, in the refusal's entry count, which
//! is two rather than the recursion budget; and positively, in a chain of adjacencies four times
//! longer than that budget parsing in **one frame**.
//!
//! **Section three — the token-level engine refuses the shape.** Its termination argument is that
//! acceptance *is* the commit of one nonzero-width token, and its infix fold is handed a real
//! token. Neither survives a zero-token operator, so `InputRef::pratt` diagnoses the report on
//! the RHS channel rather than parking it and ending the expression with a silent `Ok`.

mod common;

use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use tokora::{
  Emitter, EmitterView, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan,
  emitter::Verbose,
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
    token::UnexpectedTokenOf,
  },
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  span::Spanned,
  token::PrattToken,
};

use tokora::state::recursion_tracker::RecursionLimiter;

use common::{TestLexer, Token, TokenKind, WallClock, bounded_wait};

#[derive(Debug, PartialEq, Eq)]
enum AdjError {
  /// Anything the fixture does not care to distinguish.
  Other,
  /// The driver's terminal end-of-RHS report — what an unpaid adjacency raises.
  EoRhs,
  /// The driver's non-associative chain report.
  NonAssoc,
}

impl From<()> for AdjError {
  fn from(_: ()) -> Self {
    AdjError::Other
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, TestLexer<'inp>>> for AdjError {
  fn from(_: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Self {
    AdjError::Other
  }
}
impl From<UnexpectedEoLhs> for AdjError {
  fn from(_: UnexpectedEoLhs) -> Self {
    AdjError::Other
  }
}
impl From<UnexpectedEoRhs> for AdjError {
  fn from(_: UnexpectedEoRhs) -> Self {
    AdjError::EoRhs
  }
}
impl From<RecursionLimitReached> for AdjError {
  fn from(_: RecursionLimitReached) -> Self {
    AdjError::Other
  }
}
impl From<NonAssociativeChain> for AdjError {
  fn from(_: NonAssociativeChain) -> Self {
    AdjError::NonAssoc
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for AdjError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    AdjError::Other
  }
}
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for AdjError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    AdjError::Other
  }
}

/// Can an operand begin at this kind? The one predicate every classifier below asks, and the
/// reason a zero-token continuation is decidable at all: adjacency is "an operand starts here
/// and no operator introduced it".
fn operand_can_begin(kind: Option<TokenKind>) -> bool {
  matches!(kind, Some(TokenKind::Num))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Section one — juxtaposition competing on the driver's own ladder
// ═══════════════════════════════════════════════════════════════════════════════
//
// Four spellings of connective, three of them written and one of them empty, which is the shape
// tokora#274 records for LogQL's `labelFilter labelFilter`:
//
//   *    Left,     3    binds tightest
//   ,    Left,     1    the adjacency's spelled sibling
//   =    Neither,  1    the same power, non-associative
//   ·    Adjacent, 1    no token at all

const P_TIGHT: i64 = 3;
const P_LOOSE: i64 = 1;

fn juxt_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n.to_string())),
      _ => Err(AdjError::Other),
    },
    None => Err(AdjError::Other),
  }
}

fn juxt_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, (), i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  // The adjacency decision is taken FIRST and without consuming: nothing introduces this
  // operator, so there is no token to read before deciding, and reading one would be the
  // `Postfix` workaround wearing a different name.
  if operand_can_begin(inp.peek_kind()?) {
    return Ok(PrattRHS::Adjacent(Precedenced::new("·", P_LOOSE)));
  }
  // Every other report consumes the operator it names; `End` hands the deciding read back.
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Star => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left("*"),
        P_TIGHT,
      ))),
      Token::Comma => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(","),
        P_LOOSE,
      ))),
      Token::Eq => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither("="),
        P_LOOSE,
      ))),
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

fn juxt_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: String,
  right: String,
  op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, i64>,
) -> Result<String, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  let (PrattInfix::Left(sym) | PrattInfix::Right(sym) | PrattInfix::Neither(sym)) = op.into_data();
  Ok(format!("({left} {sym} {right})"))
}

fn juxt_fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, i64>,
) -> Result<String, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(format!("({}{operand})", op.into_data()))
}

fn juxt_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: String,
  _op: Precedenced<(), i64>,
) -> Result<String, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(operand)
}

/// Reports the pratt parser's own outcome, so a refusal is a value this fixture can assert on
/// rather than a failure of the surrounding parse.
fn juxt_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Result<String, AdjError>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(
    pratt(
      juxt_lhs,
      juxt_rhs,
      juxt_fold_prefix,
      juxt_fold_infix,
      juxt_fold_postfix,
    )
    .parse_input(inp),
  )
}

fn juxt(src: &str) -> Result<String, AdjError> {
  Parser::new()
    .apply(juxt_expr)
    .parse_str(src)
    .expect("the outer parser never fails")
}

/// One operand is one operand: the adjacency channel is never consulted for a lone token, and a
/// lone token is not an adjacency chain of length one.
#[test]
fn a_single_operand_is_not_an_adjacency() {
  assert_eq!(juxt("1"), Ok("1".to_owned()));
}

/// Two adjacent operands fold as an infix application whose operator is the empty string.
#[test]
fn two_adjacent_operands_fold_as_one_infix_application() {
  assert_eq!(juxt("1 2"), Ok("(1 · 2)".to_owned()));
}

/// **Left-associative, and by the driver's floor rather than by the fold's shape.** The
/// continuation descends on `> power`, so the inner frame declines the second adjacency and the
/// outer frame folds it — the grouping a right-associative bound would invert.
#[test]
fn an_adjacency_chain_is_left_associative() {
  assert_eq!(juxt("1 2 3"), Ok("((1 · 2) · 3)".to_owned()));
  assert_eq!(juxt("1 2 3 4"), Ok("(((1 · 2) · 3) · 4)".to_owned()));
}

/// **The requirement tokora#274 states, in one input.** `*` binds at 3, the adjacency at 1, so
/// the tighter operator takes the right operand — and the floor that decided it is the driver's,
/// carried into the recursion from the adjacency's own power.
#[test]
fn a_tighter_spelled_operator_takes_the_adjacency_s_right_operand() {
  assert_eq!(juxt("1 2 * 3"), Ok("(1 · (2 * 3))".to_owned()));
}

/// The mirror: an adjacency does not reach back inside a tighter operator's left operand either.
#[test]
fn a_tighter_spelled_operator_keeps_its_own_left_operand() {
  assert_eq!(juxt("1 * 2 3"), Ok("((1 * 2) · 3)".to_owned()));
}

/// An adjacency and its spelled sibling at the same power interleave as one left-associative
/// chain — which is what "competing for precedence with three spelled siblings" has to mean.
#[test]
fn an_adjacency_and_a_same_power_spelled_operator_form_one_chain() {
  assert_eq!(juxt("1 , 2 3"), Ok("((1 , 2) · 3)".to_owned()));
  assert_eq!(juxt("1 2 , 3"), Ok("((1 · 2) , 3)".to_owned()));
  assert_eq!(juxt("1 , 2 3 , 4"), Ok("(((1 , 2) · 3) , 4)".to_owned()));
}

/// A `Neither` operator at the adjacency's power latches the frame, and the adjacency is the
/// second link the latch refuses — the constraint is a property of the chain, not of the
/// newcomer's spelling.
#[test]
fn an_adjacency_after_a_same_power_neither_fold_is_a_chain_violation() {
  assert_eq!(juxt("1 = 2 3"), Err(AdjError::NonAssoc));
}

/// And the converse: an adjacency folds as `Left`, so it never *arms* that latch. `=` after it
/// is an ordinary same-power fold.
#[test]
fn an_adjacency_never_arms_the_non_associative_latch() {
  assert_eq!(juxt("1 2 = 3"), Ok("((1 · 2) = 3)".to_owned()));
}

/// The floor still owns the ending. Entering above the adjacency's power leaves the whole
/// juxtaposition to the surrounding grammar rather than folding it — the adjacency is declined
/// exactly as a spelled operator below the floor is, with the deciding read handed back.
#[test]
fn a_floor_above_the_adjacency_s_power_declines_it() {
  fn embedded<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(String, usize), AdjError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
  {
    let value = pratt(
      juxt_lhs,
      juxt_rhs,
      juxt_fold_prefix,
      juxt_fold_infix,
      juxt_fold_postfix,
    )
    .min_precedence(P_TIGHT)
    .parse_input(inp)?;
    let mut rest = 0usize;
    while inp.next()?.is_some() {
      rest += 1;
    }
    Ok((value, rest))
  }

  let (value, rest): (String, usize) = Parser::new().apply(embedded).parse_str("1 2 3").unwrap();
  assert_eq!(
    (value.as_str(), rest),
    ("1", 2),
    "at a floor above the adjacency's power the expression is the bare operand, and `2 3` is \
     left on the input untouched"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Section two — what stops it
// ═══════════════════════════════════════════════════════════════════════════════
//
// One grammar, two inputs, one difference: whether the right operand consumed anything. The LHS
// channel answers a **zero-width operand** for `;` — legal, and the shape a recovery grammar
// uses for a hole — so `a ;` is a continuation nothing paid for.

/// How many times the LHS channel below was entered. The charge's other half is a *depth*
/// statement, and depth is what this counts: with the exclusive descent the unpaid parse enters
/// it twice, with an inclusive one it enters it once per level of the recursion budget.
static PAID_LHS_CALLS: AtomicUsize = AtomicUsize::new(0);

/// How many times the fold below ran. This is the pin on *where* the charge sits: the work a
/// continuation buys is the fold, the CST wrap and another turn, and a charge read after the fold
/// prices work that has already been done — and then thrown away, together with the expression's
/// own diagnostics, by the wider restore the foot-of-cycle refusal takes.
///
/// Without it the cells below stay green on a driver with no charge at all, because the
/// foot-of-cycle guard reaches the same condition one fold later and raises the same terminal
/// error. Measured: with the charge deleted, every other assertion in this section still passed.
static PAID_FOLDS: AtomicUsize = AtomicUsize::new(0);

/// One counter, several cells, and the harness runs cells in parallel. The lock is taken for the
/// whole of `paid` so a reading belongs to the parse that produced it.
///
/// Poisoning is folded rather than propagated: two of the cells below `should_panic` in a debug
/// build, so a poisoned lock is this fixture working, not this fixture broken.
static PAID_LOCK: Mutex<()> = Mutex::new(());

fn paid_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  PAID_LHS_CALLS.fetch_add(1, Ordering::Relaxed);
  if !operand_can_begin(inp.peek_kind()?) {
    // A zero-width operand. `PrattLHS::Operand` is not held to "consume what you report", which
    // is what makes a recovery hole expressible — and what makes an unpaid adjacency reachable.
    return Ok(PrattLHS::Operand("<hole>".to_owned()));
  }
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n.to_string())),
      _ => Err(AdjError::Other),
    },
    None => Err(AdjError::Other),
  }
}

fn paid_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, (), i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  // Continue on anything that is not the end of input — including `;`, which the LHS channel
  // above answers with a hole. That is the unpaid cycle.
  match inp.peek_kind()? {
    None => Ok(PrattRHS::End),
    Some(_) => Ok(PrattRHS::Adjacent(Precedenced::new("·", P_LOOSE))),
  }
}

fn paid_fold_infix<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: String,
  right: String,
  op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, i64>,
) -> Result<String, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  PAID_FOLDS.fetch_add(1, Ordering::Relaxed);
  juxt_fold_infix(inp, left, right, op)
}

fn paid_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Result<String, AdjError>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(
    pratt(
      paid_lhs,
      paid_rhs,
      juxt_fold_prefix,
      paid_fold_infix,
      juxt_fold_postfix,
    )
    .parse_input(inp),
  )
}

/// What one run of the grammar above did: its outcome, how many times the LHS channel was
/// entered, and how many folds ran.
struct Run {
  /// `None` when the driver's own assertion unwound the run — the debug profile's half of the
  /// contract. `Some` is the value the release profile returns.
  outcome: Option<Result<String, AdjError>>,
  lhs_entries: usize,
  folds: usize,
}

/// Runs the grammar under `catch_unwind`, so the counters can be read on **both** sides of the
/// profile split rather than only on the side that does not panic.
///
/// The four-arm table is the house idiom (`pratt_txn_retention.rs`'s `stall_outcome`) and it is
/// matched rather than asserted: `cfg!` is a constant, and an `assert!` over one is a lint. Only
/// two of the arms are the contract, and the other two are named so a cell cannot pass by
/// measuring the wrong profile twice.
fn paid(src: &str, expect_refusal: bool) -> Run {
  let _guard = PAID_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  PAID_LHS_CALLS.store(0, Ordering::Relaxed);
  PAID_FOLDS.store(0, Ordering::Relaxed);
  let caught = std::panic::catch_unwind(|| {
    Parser::new()
      .apply(paid_expr)
      .parse_str(src)
      .expect("the outer parser never fails")
  });
  let outcome = match (caught, expect_refusal, cfg!(debug_assertions)) {
    // Debug, refused: the driver's own charge assertion, raised in the wrapper once the
    // expression guard has settled.
    (Err(_), true, true) => None,
    // Release, refused: no assertion exists, so the violation must arrive as the terminal error.
    (Ok(out), true, false) => Some(out),
    (Ok(out), true, true) => panic!(
      "a debug build must reach the driver's own charge assertion — got {out:?}. Without it \
       this cell measures the release path twice and pins nothing about where that assertion \
       fires"
    ),
    (Err(_), true, false) => panic!(
      "a release build has no assertion to raise on this exit, so an unwind out of it means a \
       settle refused rather than restored"
    ),
    // A served continuation must not panic in either profile.
    (Ok(out), false, _) => Some(out),
    (Err(_), false, _) => panic!("a served continuation raised no assertion in either profile"),
  };
  Run {
    outcome,
    lhs_entries: PAID_LHS_CALLS.load(Ordering::Relaxed),
    folds: PAID_FOLDS.load(Ordering::Relaxed),
  }
}

/// The operand paid, so the continuation is served.
#[test]
fn a_continuation_whose_operand_consumed_is_served() {
  let run = paid("1 2", false);
  assert_eq!(run.outcome, Some(Ok("(1 · 2)".to_owned())));
  assert_eq!(
    run.lhs_entries, 2,
    "one entry for `1` and one for the operand the continuation descended onto — the ending is \
     the RHS channel's answer and costs no third entry"
  );
  assert_eq!(run.folds, 1);
}

/// **The charge, and the fact that it sits in front of the work.**
///
/// The adjacency consumed nothing and its operand consumed nothing, so the cycle would buy a
/// fold, a CST wrap and another turn for no input at all. `folds == 0` is what says the driver
/// refused it *before* paying for any of that — and it is the assertion that fails on a driver
/// with no charge, where the foot-of-cycle guard reaches the same condition one fold later,
/// raises the same terminal error, and additionally takes the whole expression and its
/// diagnostics back with it. Every other assertion in this section is green on that driver.
///
/// `lhs_entries == 2` is the **depth** half. A continuation that descended on `>= power` would be
/// re-reported by the inner frame over the same hole and descend again, so this number would be
/// the recursion budget and the error would be `RecursionLimitReached`.
#[test]
fn a_continuation_nothing_paid_for_is_refused_before_it_folds() {
  let run = paid("1 ;", true);
  assert_eq!(
    run.folds, 0,
    "the charge is read when the operand parse returns, before the fold and before the wrap"
  );
  assert_eq!(
    run.lhs_entries, 2,
    "`1`, then the hole the unpaid continuation descended onto — and nothing after it"
  );
  if let Some(out) = run.outcome {
    assert_eq!(
      out,
      Err(AdjError::EoRhs),
      "release: a zero-token continuation over a zero-width operand is the terminal end-of-RHS \
       report, not an `Ok` carrying a phantom fold"
    );
  }
}

/// The same refusal at a cycle that has already folded: the charge is per continuation, not a
/// property of the first one, and the paid fold ahead of it still happened.
#[test]
fn the_charge_is_paid_once_per_continuation() {
  let run = paid("1 2 ;", true);
  assert_eq!(
    run.folds, 1,
    "the paid continuation folded and the unpaid one did not"
  );
  assert_eq!(run.lhs_entries, 3, "`1`, `2`, then the hole");
  if let Some(out) = run.outcome {
    assert_eq!(out, Err(AdjError::EoRhs));
  }
}

/// **The other half of the bound, pinned positively.** A chain of adjacencies far longer than the
/// recursion budget parses in **one frame**: the exclusive descent makes the inner frame decline
/// the next continuation, so the chain iterates in the loop where committed consumption bounds it.
///
/// Under an inclusive descent this same input is one native frame per operand and fails with
/// `RecursionLimitReached` long before the end. The length is derived from the budget rather than
/// written down, so raising the default cannot quietly make the pin vacuous.
#[test]
fn a_long_adjacency_chain_iterates_in_one_frame() {
  let operands = RecursionLimiter::PARSE_DEFAULT_DEPTH * 4 + 16;
  let src = (1..=operands).map(|_| "1").collect::<Vec<_>>().join(" ");

  let out = bounded_wait(
    2 * 1024 * 1024,
    // The wall is outside the parse because the failure it guards is not an assertion that fires
    // — it is a process that does not come back. Measured on this fixture: under a millisecond
    // compiled, a few seconds interpreted. Both allowances are far over their readings, because
    // what is being distinguished is "fast" from "never".
    WallClock {
      native_secs: 30,
      interpreted_secs: 300,
    },
    move || juxt(&src),
  )
  .expect("the chain parses");

  assert_eq!(
    out.matches('·').count(),
    operands - 1,
    "every adjacent pair folded"
  );
  let left_deep = format!("{}1 · 1)", "(".repeat(operands - 1));
  assert!(
    out.starts_with(&left_deep),
    "and the tree is left-deep — every fold nests inside the next, so the whole chain opens \
     with {} parentheses before the first operand: {}",
    operands - 1,
    &out[..out.len().min(96)]
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Section three — the token-level engine refuses the shape
// ═══════════════════════════════════════════════════════════════════════════════

/// The token-level classifier: `+` is an ordinary infix, `,` is the zero-token continuation this
/// engine has no token to commit for and no token to hand its infix fold.
impl PrattToken<'_, i64, i64> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), i64>> {
    match self {
      Token::Num(_) => Some(PrattLHS::Operand(())),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), i64>> {
    match self {
      Token::Plus => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        P_LOOSE,
      ))),
      // The report this engine cannot serve.
      Token::Comma => Some(PrattRHS::Adjacent(Precedenced::new((), P_LOOSE))),
      _ => None,
    }
  }
}

type Tok = Spanned<Token, SimpleSpan>;

/// Folds to the **right** operand, so a served infix and a refused continuation are told apart by
/// the value as well as by the diagnostic count.
fn tok_fold_infix<'inp, E>(
  _left: Tok,
  right: Tok,
  _infix: Spanned<PrattInfix<Token, Token, Token>, SimpleSpan>,
  _em: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, AdjError> {
  Ok(right)
}

fn tok_fold_unary<'inp, E>(
  a: Tok,
  _b: Tok,
  _em: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, AdjError> {
  Ok(a)
}

/// Runs the token engine over `src` under a **recording** emitter, returning the token it ended
/// with and how many diagnostics it recorded.
fn token_engine(src: &str) -> (Option<Token>, usize) {
  fn probe<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<AdjError>>,
    >,
  ) -> Result<(Option<Token>, usize), AdjError> {
    let out = inp.pratt::<_, _, _, i64, i64>(
      tok_fold_unary::<Verbose<AdjError>>,
      tok_fold_infix::<Verbose<AdjError>>,
      tok_fold_unary::<Verbose<AdjError>>,
    )?;
    let recorded = inp.emitter_ref().errors().values().flatten().count();
    Ok((out.map(|t| t.into_data()), recorded))
  }

  Parser::with_context(ParserContext::new(Verbose::<AdjError>::new()))
    .apply(probe)
    .parse_str(src)
    .expect("a recording emitter carries the diagnostic instead of failing the parse")
}

/// The token engine **diagnoses** the report instead of parking it and ending the expression.
///
/// A silent ending is the failure `PrattRHS::End`'s own documentation refuses for a below-floor
/// sentinel: the expression truncates with an `Ok` and no diagnostic on any channel while the
/// grammar believes it asked for a continuation. Here the expression still ends at `1` — this
/// engine has nothing to fold — but it says so on the RHS channel.
///
/// Falsifier: answer `None` in the `Adjacent` arm of `input_ref/pratt.rs` instead of recording a
/// park reason, and the returned token is unchanged while the count drops to 0. The two halves
/// are independent, and the count is the half that matters.
#[test]
fn the_token_engine_diagnoses_a_zero_token_continuation() {
  let (returned, recorded) = token_engine("1 , 2");
  assert_eq!(
    returned,
    Some(Token::Num(1)),
    "the engine has no zero-token operator to fold, so the expression is the left operand"
  );
  assert_eq!(
    recorded, 1,
    "and it is an end-of-RHS diagnostic rather than a quiet truncation"
  );
}

/// The control: the same engine, the same fixture, a spelled operator at the same power. The
/// refusal above is about the report and not about the harness.
#[test]
fn the_token_engine_still_serves_a_spelled_operator() {
  let (returned, recorded) = token_engine("1 + 2");
  assert_eq!(returned, Some(Token::Num(2)), "folded to the right operand");
  assert_eq!(recorded, 0);
}
