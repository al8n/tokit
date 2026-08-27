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
//!
//! **Section four — a report variant chosen by lookahead** (tokora#202's optional-RHS shape).
//! `/` is infix when an operand follows and postfix when one does not; at the left edge it is a
//! prefix when an operand follows and a bare operand when one does not. All four reports consume
//! the `/` they name, so "consume what you report" holds in each, and the variant itself is the
//! thing the lookahead decides. This is r-a's `Restrictions`-gated `0..` / `..b` case, and it
//! needs nothing from the driver but the freedom `ParsePrattRHS` already documents.

mod common;

use core::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
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

/// What one run of a counted grammar did: how many times the LHS channel was entered, how many
/// folds ran, and — **unsettled** — what came back.
///
/// The counters are plain fields and the outcome is behind [`Run::settle`], deliberately: the
/// counts are what these cells are for, so they must be assertable *before* anything decides
/// whether the profile split was honoured. A runner that settled first would red on the profile
/// table where a plant should red on the number.
struct Run {
  caught: std::thread::Result<Result<String, AdjError>>,
  lhs_entries: usize,
  folds: usize,
}

/// What a settled [`Run`] produced.
struct Settled {
  /// `None` when the driver's own assertion unwound the run — the debug profile's half of the
  /// contract. `Some` is the value the release profile returns.
  outcome: Option<Result<String, AdjError>>,
  /// The refusal's own wording, in a debug build. **Both** adjacency refusals arrive as the same
  /// terminal `UnexpectedEoRhs` and at the same counts, so the error value alone cannot say which
  /// law stopped the parse; the assertion text can, and a cell that means one of them says so.
  refusal: Option<String>,
}

impl Run {
  /// The four-arm profile table, shared by the two counted fixtures below.
  ///
  /// The house idiom (`pratt_txn_retention.rs`'s `stall_outcome`), matched rather than asserted:
  /// `cfg!` is a constant, and an `assert!` over one is a lint. Only two of the arms are the
  /// contract, and the other two are named so a cell cannot pass by measuring the wrong profile
  /// twice.
  fn settle(self, expect_refusal: bool) -> Settled {
    let (outcome, refusal) = match (self.caught, expect_refusal, cfg!(debug_assertions)) {
      // Debug, refused: one of the driver's own adjacency assertions, raised in the wrapper once
      // the expression guard has settled. The payload is the wording, which names which one.
      (Err(payload), true, true) => (None, Some(panic_text(&*payload))),
      // Release, refused: no assertion exists, so the violation must arrive as the terminal error.
      (Ok(out), true, false) => (Some(out), None),
      (Ok(out), true, true) => panic!(
        "a debug build must reach the driver's own assertion — got {out:?}. Without it this cell \
         measures the release path twice and pins nothing about where that assertion fires"
      ),
      (Err(_), true, false) => panic!(
        "a release build has no assertion to raise on this exit, so an unwind out of it means a \
         settle refused rather than restored"
      ),
      // A served continuation must not panic in either profile.
      (Ok(out), false, _) => (Some(out), None),
      (Err(_), false, _) => panic!("a served continuation raised no assertion in either profile"),
    };
    Settled { outcome, refusal }
  }
}

/// A panic payload as text. `debug_assert!` with a bare literal message panics with a
/// `&'static str`; one whose message is formatted panics with a `String`. Both are read, so the
/// cells do not depend on which the compiler chose.
fn panic_text(payload: &(dyn core::any::Any + Send)) -> String {
  payload
    .downcast_ref::<String>()
    .cloned()
    .or_else(|| {
      payload
        .downcast_ref::<&'static str>()
        .map(|s| (*s).to_owned())
    })
    .unwrap_or_else(|| "<a panic payload that is neither String nor &str>".to_owned())
}

/// Runs the grammar under `catch_unwind`, so the counters can be read on **both** sides of the
/// profile split rather than only on the side that does not panic.
fn paid(src: &str) -> Run {
  let _guard = PAID_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  PAID_LHS_CALLS.store(0, Ordering::Relaxed);
  PAID_FOLDS.store(0, Ordering::Relaxed);
  let caught = std::panic::catch_unwind(|| {
    Parser::new()
      .apply(paid_expr)
      .parse_str(src)
      .expect("the outer parser never fails")
  });
  Run {
    caught,
    lhs_entries: PAID_LHS_CALLS.load(Ordering::Relaxed),
    folds: PAID_FOLDS.load(Ordering::Relaxed),
  }
}

/// The wording of the charge below — the refusal that prices *this cycle's operand*, as against
/// the debt refusal that prices the frames a descent would build. Matched as a fragment of the
/// driver's own assertion, which is what keeps a cell from passing on the other law's exit.
const CHARGE_WORDING: &str = "right operand consumed nothing";

/// The debt refusal's wording, the same way.
const DEBT_WORDING: &str = "no input committed since the enclosing one";

/// The operand paid, so the continuation is served.
#[test]
fn a_continuation_whose_operand_consumed_is_served() {
  let run = paid("1 2");
  assert_eq!(
    run.lhs_entries, 2,
    "one entry for `1` and one for the operand the continuation descended onto — the ending is \
     the RHS channel's answer and costs no third entry"
  );
  assert_eq!(run.folds, 1);
  let served = run.settle(false);
  assert_eq!(served.outcome, Some(Ok("(1 · 2)".to_owned())));
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
  let run = paid("1 ;");
  assert_eq!(
    run.folds, 0,
    "the charge is read when the operand parse returns, before the fold and before the wrap"
  );
  assert_eq!(
    run.lhs_entries, 2,
    "`1`, then the hole the unpaid continuation descended onto — and nothing after it"
  );
  let refused = run.settle(true);
  if let Some(msg) = refused.refusal.as_deref() {
    assert!(
      msg.contains(CHARGE_WORDING),
      "debug: the law that stopped this is the CHARGE — the operand of a continuation the debt \
       test admitted. The debt refusal reaches the same counts and the same error, so a cell \
       that does not read the wording cannot tell the two apart: {msg}"
    );
  }
  if let Some(out) = refused.outcome {
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
  let run = paid("1 2 ;");
  assert_eq!(
    run.folds, 1,
    "the paid continuation folded and the unpaid one did not"
  );
  assert_eq!(run.lhs_entries, 3, "`1`, `2`, then the hole");
  let refused = run.settle(true);
  if let Some(msg) = refused.refusal.as_deref() {
    assert!(
      msg.contains(CHARGE_WORDING),
      "the charge, not the debt: {msg}"
    );
  }
  if let Some(out) = refused.outcome {
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
    // The wall is outside the parse because the failure it guards is not an assertion that
    // fires — it is a process that does not come back.
    //
    // Both figures are read rather than scaled. Native: the whole 17-cell file runs in 0.03s
    // under `cargo test -p tokora --all-features --test pratt_adjacent`, this cell being the
    // only one that is not instantaneous. Interpreted: **25.9s** for this cell alone, under
    //
    //     MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-disable-isolation \
    //       -Zmiri-symbolic-alignment-check -Zmiri-tree-borrows" \
    //     cargo +nightly miri test --target aarch64-apple-darwin --test pratt_adjacent \
    //       --features logos a_long_adjacency_chain
    //
    // on an M-series host. The interpreted allowance is `pratt_limit.rs`'s, which is ~27x that
    // reading and leaves room for a CI runner several times slower than the host measured on.
    // The margins are deliberately enormous on both sides: what this wall distinguishes is
    // "fast" from "never", not one duration from another, and a tight bound here would fail on
    // a loaded runner while pinning nothing extra.
    WallClock {
      native_secs: 120,
      interpreted_secs: 700,
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

// ── The escalation a frame-local charge cannot price ──────────────────────────
//
// The charge above is frame-local and RETROSPECTIVE: it runs when the recursive operand parse
// returns, so it prices this frame's own cycles and nothing the descent already built. The
// argument that used to close the gap was that what can still nest is a strictly increasing power
// chain, "the grammar's ladder, which the input does not choose" — and that is true only while the
// powers come from the grammar. A classifier is contract-valid with state in it, and this one puts
// the ladder under the input's control: a strictly increasing power at every level, zero-width
// operands all the way down, and the document's single byte arriving in the DEEPEST frame, where
// it is past the committed position every ancestor descended from and so passes every charge at
// once.
//
// What that buys, on a driver with the charge alone: `LADDER_RUNGS` native frames,
// `LADDER_RUNGS` folds and as many CST wraps, for one byte — and an `Ok` at the end of it. The
// counts are the differential; a terminal error is not, because the recursion limiter reaches one
// too, several hundred frames later and saying something else.

/// How far the classifier escalates. Derived from the recursion budget and **under** it, so this
/// input is one the escalation completes: what the cells below measure is the charge failing to
/// bound the descent, not the limiter absorbing it.
const LADDER_RUNGS: i64 = (RecursionLimiter::PARSE_DEFAULT_DEPTH * 3 / 4) as i64;

/// The rung the classifier has escalated to — the whole of its state, and the thing that makes
/// these powers a function of the parse rather than of the grammar.
static LADDER_RUNG: AtomicI64 = AtomicI64::new(0);

/// Whether the LHS channel pays for every rung or only for the deepest one. The single difference
/// between the escalation and its control.
static LADDER_PAYS_EVERY_RUNG: AtomicBool = AtomicBool::new(false);

static LADDER_LHS_CALLS: AtomicUsize = AtomicUsize::new(0);
static LADDER_FOLDS: AtomicUsize = AtomicUsize::new(0);
static LADDER_LOCK: Mutex<()> = Mutex::new(());

fn ladder_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  LADDER_LHS_CALLS.fetch_add(1, Ordering::Relaxed);
  let deepest = LADDER_RUNG.load(Ordering::Relaxed) >= LADDER_RUNGS;
  if !(deepest || LADDER_PAYS_EVERY_RUNG.load(Ordering::Relaxed))
    || !operand_can_begin(inp.peek_kind()?)
  {
    // A zero-width operand. Legal — `PrattLHS::Operand` is not held to "consume what you report"
    // — and the shape a recovery hole already uses.
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

fn ladder_rhs<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, (), i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  let rung = LADDER_RUNG.load(Ordering::Relaxed);
  if rung >= LADDER_RUNGS {
    return Ok(PrattRHS::End);
  }
  LADDER_RUNG.store(rung + 1, Ordering::Relaxed);
  // STRICTLY INCREASING, which is exactly what each inner frame's `Exclusive(p)` floor admits.
  // Nothing in the input chose this ladder and no line of the grammar wrote it down: the
  // classifier reads its own state, which is a thing `ParsePrattRHS` permits and this driver
  // cannot see.
  Ok(PrattRHS::Adjacent(Precedenced::new("·", rung + 1)))
}

fn ladder_fold_infix<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: String,
  right: String,
  op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, i64>,
) -> Result<String, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  LADDER_FOLDS.fetch_add(1, Ordering::Relaxed);
  juxt_fold_infix(inp, left, right, op)
}

fn ladder_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Result<String, AdjError>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(
    pratt(
      ladder_lhs,
      ladder_rhs,
      juxt_fold_prefix,
      ladder_fold_infix,
      juxt_fold_postfix,
    )
    .parse_input(inp),
  )
}

fn ladder(src: &str, pays_every_rung: bool) -> Run {
  let _guard = LADDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  LADDER_RUNG.store(0, Ordering::Relaxed);
  LADDER_PAYS_EVERY_RUNG.store(pays_every_rung, Ordering::Relaxed);
  LADDER_LHS_CALLS.store(0, Ordering::Relaxed);
  LADDER_FOLDS.store(0, Ordering::Relaxed);
  let caught = std::panic::catch_unwind(|| {
    Parser::new()
      .apply(ladder_expr)
      .parse_str(src)
      .expect("the outer parser never fails")
  });
  Run {
    caught,
    lhs_entries: LADDER_LHS_CALLS.load(Ordering::Relaxed),
    folds: LADDER_FOLDS.load(Ordering::Relaxed),
  }
}

/// **One byte may not buy a second zero-token frame, however the powers are spelled.**
///
/// `lhs_entries` is the frame count — the LHS channel is entered exactly once per frame — and
/// `folds` is the work. Two entries: the frame that took the outermost continuation, and the one
/// that reported the second and was refused before descending. Zero folds: the refusal is in
/// front of the work, as the charge is.
///
/// **The falsifier, and it is a count and not an error.** Delete the debt test in the `Adjacent`
/// arm, or pass `None` instead of `Some(&committed)` at its descent, and this cell reads
/// `LADDER_RUNGS + 1` entries and `LADDER_RUNGS` folds — with `Ok`, not an error, because the
/// document's one byte reaches the deepest frame and satisfies every ancestor's charge on the way
/// back up. That is the whole finding: a charge read on the way up cannot price what the way down
/// built. The rung count is under the recursion budget precisely so the cell reds on those
/// numbers rather than on `RecursionLimitReached`, which a driver with no bound at all would also
/// eventually reach.
#[test]
fn an_escalating_classifier_cannot_buy_frames_the_input_did_not() {
  let run = ladder("1", false);
  assert_eq!(
    run.lhs_entries, 2,
    "one frame took the outermost continuation and the next was refused before it descended — \
     an escalating classifier buys no frame the input has not paid for"
  );
  assert_eq!(
    run.folds, 0,
    "and the refusal is in front of the fold, the wrap and the next turn, exactly as the charge is"
  );
  let refused = run.settle(true);
  if let Some(msg) = refused.refusal.as_deref() {
    assert!(
      msg.contains(DEBT_WORDING),
      "debug: the law that stopped this is the DEBT — a continuation descending on input the \
       enclosing one was already paid with. The charge would name the operand instead: {msg}"
    );
  }
  if let Some(out) = refused.outcome {
    assert_eq!(
      out,
      Err(AdjError::EoRhs),
      "release: the terminal end-of-RHS report, the same posture every other unpaid continuation \
       takes"
    );
  }
}

/// **The control, and it is the half that says the bound is a price and not a prohibition.**
///
/// The same escalating classifier, over a document that pays a whole operand per rung. Every
/// level advances committed consumption past the position its parent descended from, so every
/// level is admitted: `LADDER_RUNGS + 1` frames and `LADDER_RUNGS` folds, right-deep because a
/// strictly increasing ladder is what makes the inner frame take the next continuation.
///
/// Without this cell the one above passes on a driver that refuses every nested adjacency
/// outright — which is a bound, and the wrong one.
#[test]
fn a_ladder_the_document_pays_for_rung_by_rung_still_nests() {
  let rungs = usize::try_from(LADDER_RUNGS).expect("the rung count is a small positive number");
  let src = (0..=rungs).map(|_| "1").collect::<Vec<_>>().join(" ");
  let run = ladder(&src, true);
  assert_eq!(
    run.lhs_entries,
    rungs + 1,
    "one frame per rung, plus the innermost operand's — every one of them paid for by an operand \
     of its own"
  );
  assert_eq!(run.folds, rungs, "and every rung folded");
  let out = run
    .settle(false)
    .outcome
    .expect("a paid ladder is served in both profiles")
    .expect("and it parses");
  assert_eq!(
    out.matches('·').count(),
    rungs,
    "every rung folded into the tree"
  );
  assert!(
    out.starts_with("(1 · (1 · "),
    "and the tree is right-deep, because a strictly increasing ladder is what puts the next \
     continuation inside the previous one's right operand: {}",
    &out[..out.len().min(96)]
  );
}

// ── Where the classifier's own bytes land ────────────────────────────────────
//
// Every cell above holds the classifier to the easy half of its exemption: it consumes nothing,
// so the position the cycle started from and the position the report crossed back at are one
// number and each of the arm's three measurements can be taken from either. The other half is
// legal too — a CST-shaped classifier skips the trivia between two adjacent operands before
// deciding they *are* adjacent — and it splits those two numbers apart. The driver reads
// `committed` at the top of the loop turn, so inside a turn it names the position BEFORE the
// classifier ran; `after_report` is the position it left.
//
// The rule these cells pin is one sentence: **classifier consumption pays the enclosing
// adjacency, never the continuation the classifier reports.** Bytes taken before the report
// discharge the debt the frames above are owed; bytes taken after it pay for this continuation.
// Every byte pays exactly one debt, and none pays two.
//
// Three measurements sit on that boundary, and there is one cell for each — so a driver that
// takes any single one of them from `committed` reds exactly one of these three and leaves the
// other two green:
//
//   the charge     `after_operand <= after_report`   the trivia is not the operand's payment
//   the debt test  `after_report <= *owed`           but it does discharge the outer debt
//   the descent    `Some(&after_report)`             and the frame below owes those bytes too
//
// The lexer skips whitespace, so "trivia" here is a real token the classifier consumes and does
// not report — which is what a CST-shaped grammar does with a comment or a line break it intends
// to attach to the node rather than to name as the operator.

/// What the scripted classifier below does at one rung.
#[derive(Clone, Copy)]
enum Rung {
  /// Consume the token sitting here and *then* report the continuation. The half of the exemption
  /// that moves `after_report` past the position the cycle started from.
  EatThenReport,
  /// Report the continuation having consumed nothing — the half every cell above uses.
  ReportOnly,
  /// End the expression here.
  Stop,
}

/// The script the classifier is running, and how far into it this parse has got. A rung is spent
/// per *report*, so the `End` the classifier answers at exhaustion costs none — which keeps the
/// powers below tied to the descent rather than to the call count.
static TRIVIA_SCRIPT: Mutex<&'static [Rung]> = Mutex::new(&[]);
static TRIVIA_RUNG: AtomicUsize = AtomicUsize::new(0);

/// The frame count and the work, read exactly as the two fixtures above read theirs. Separate
/// counters and a separate lock, because the harness runs cells in parallel and a reading has to
/// belong to the parse that produced it.
static TRIVIA_LHS_CALLS: AtomicUsize = AtomicUsize::new(0);
static TRIVIA_FOLDS: AtomicUsize = AtomicUsize::new(0);
static TRIVIA_LOCK: Mutex<()> = Mutex::new(());

fn trivia_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  TRIVIA_LHS_CALLS.fetch_add(1, Ordering::Relaxed);
  if !operand_can_begin(inp.peek_kind()?) {
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

fn trivia_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, (), i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  if inp.peek_kind()?.is_none() {
    return Ok(PrattRHS::End);
  }
  let rung = TRIVIA_RUNG.fetch_add(1, Ordering::Relaxed);
  let step = TRIVIA_SCRIPT
    .lock()
    .unwrap_or_else(|e| e.into_inner())
    .get(rung)
    .copied()
    .unwrap_or(Rung::Stop);
  // STRICTLY INCREASING, one rung per report, so an inner frame's `Exclusive(p)` floor admits the
  // next one and the descent is what the cells measure rather than a same-power chain iterating
  // in one frame.
  let power = i64::try_from(rung + 1).expect("the scripts here are a handful of rungs long");
  match step {
    Rung::Stop => Ok(PrattRHS::End),
    Rung::ReportOnly => Ok(PrattRHS::Adjacent(Precedenced::new("·", power))),
    Rung::EatThenReport => {
      inp.next()?;
      Ok(PrattRHS::Adjacent(Precedenced::new("·", power)))
    }
  }
}

fn trivia_fold_infix<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: String,
  right: String,
  op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, i64>,
) -> Result<String, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  TRIVIA_FOLDS.fetch_add(1, Ordering::Relaxed);
  juxt_fold_infix(inp, left, right, op)
}

fn trivia_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Result<String, AdjError>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(
    pratt(
      trivia_lhs,
      trivia_rhs,
      juxt_fold_prefix,
      trivia_fold_infix,
      juxt_fold_postfix,
    )
    .parse_input(inp),
  )
}

fn trivia(src: &str, script: &'static [Rung]) -> Run {
  let _guard = TRIVIA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  *TRIVIA_SCRIPT.lock().unwrap_or_else(|e| e.into_inner()) = script;
  TRIVIA_RUNG.store(0, Ordering::Relaxed);
  TRIVIA_LHS_CALLS.store(0, Ordering::Relaxed);
  TRIVIA_FOLDS.store(0, Ordering::Relaxed);
  let caught = std::panic::catch_unwind(|| {
    Parser::new()
      .apply(trivia_expr)
      .parse_str(src)
      .expect("the outer parser never fails")
  });
  Run {
    caught,
    lhs_entries: TRIVIA_LHS_CALLS.load(Ordering::Relaxed),
    folds: TRIVIA_FOLDS.load(Ordering::Relaxed),
  }
}

/// **THE CHARGE. The trivia the classifier ate is not the right operand's payment.**
///
/// `1 , ;` with the classifier consuming the `,` before reporting the continuation, and the `;`
/// answered by the LHS channel with a hole. Committed consumption moved across this cycle — the
/// comma — but every byte of that movement is the *classifier's*, taken before the report, and
/// the operand the continuation descended onto consumed nothing at all. So the fold, the CST wrap
/// and the next turn have nothing paying for them, and `folds == 0` is what says the driver
/// refused them before buying any of it.
///
/// **The falsifier.** Charge against `committed` — the position the cycle started from, read
/// before the classifier ran — and the comma reads as the operand's payment: `folds == 1` and an
/// `Ok("(1 · <hole>)")` carrying a fold the document never bought, which is the reading this cell
/// replaces. Neither of the other two measurements can move it: the root frame inherits no
/// watermark, so the debt test's arm is `None`, and the frame the descent lands on ends without
/// reporting a continuation of its own, so the watermark it carried is never read.
#[test]
fn the_classifiers_own_trivia_does_not_pay_for_the_operand() {
  let run = trivia("1 , ;", &[Rung::EatThenReport, Rung::Stop]);
  assert_eq!(
    run.folds, 0,
    "the operand consumed nothing past where the classifier left the input, so the continuation \
     is refused in front of the fold, the wrap and the next turn"
  );
  assert_eq!(
    run.lhs_entries, 2,
    "`1`, then the hole the continuation descended onto — and nothing after it"
  );
  let refused = run.settle(true);
  if let Some(msg) = refused.refusal.as_deref() {
    assert!(
      msg.contains(CHARGE_WORDING),
      "debug: the law that stopped this is the CHARGE, read off the operand and not off the \
       cycle: {msg}"
    );
  }
  if let Some(out) = refused.outcome {
    assert_eq!(
      out,
      Err(AdjError::EoRhs),
      "release: the terminal end-of-RHS report, not an `Ok` carrying a fold the classifier's own \
       trivia paid for"
    );
  }
}

/// **THE DEBT TEST. The same trivia does discharge the adjacency the frame above is inside.**
///
/// The mirror of the cell above, in the other direction. `1 , 2`: the outer continuation is
/// reported having consumed nothing and descends onto a zero-width hole, so the inner frame opens
/// at exactly the watermark it owes. Its classifier then eats the comma and reports at a higher
/// power — real committed consumption, past the position the enclosing continuation descended
/// from, and therefore the advancement the debt test asks for. The parse is legal and nests.
///
/// **The falsifier.** Test `committed` instead — the inner frame's position before its own
/// classifier ran, which is still the watermark — and the comma is invisible to the test: a legal
/// parse is refused, *terminally*, with `AdjacencyDebt` at `lhs_entries == 2` and `folds == 0`.
/// Neither of the other two measurements can move this cell: the outer classifier consumed
/// nothing, so the descent carries the same number either way, and both charges clear by a whole
/// operand.
#[test]
fn the_classifiers_own_trivia_discharges_the_enclosing_adjacency() {
  let run = trivia(
    "1 , 2",
    &[Rung::ReportOnly, Rung::EatThenReport, Rung::Stop],
  );
  assert_eq!(
    run.lhs_entries, 3,
    "`1`, the hole the outer continuation descended onto, and the `2` the inner one bought with \
     the comma its classifier committed"
  );
  assert_eq!(run.folds, 2, "and both continuations folded");
  let served = run.settle(false);
  assert_eq!(
    served.outcome,
    Some(Ok("(1 · (<hole> · 2))".to_owned())),
    "a continuation whose classifier committed input past the watermark is owed the descent, not \
     a terminal refusal"
  );
}

/// **THE DESCENT. The frame below owes the classifier's bytes too.**
///
/// `1 , ;` again, and the outer classifier eats the comma exactly as in the charge cell — but
/// here the inner frame reports a continuation of its own instead of ending. That inner report
/// consumes nothing, and nothing was committed between the two reports, so the inner one is
/// buying a second zero-token frame on input the first was already paid with. The debt test
/// refuses it, and can only refuse it if the watermark the descent carried is the position the
/// outer classifier *left* rather than the one its cycle started from.
///
/// **The falsifier.** Descend with `committed` and the comma is spent twice: once discharging
/// nothing, and once again admitting the inner continuation whose own report added no byte to it.
/// The refusal then lands one frame deeper — `lhs_entries == 3` — and that extra frame is exactly
/// the one the rule exists to refuse. Neither of the other two measurements can move this cell:
/// the inner classifier consumes nothing, so its own `committed` and `after_report` are the same
/// number and the debt test reads the same either way, and no charge in this parse ever runs,
/// because the refusal is in front of the descent and propagates through the outer frame's.
#[test]
fn the_descent_carries_the_position_the_classifier_left() {
  let run = trivia(
    "1 , ;",
    &[
      Rung::EatThenReport,
      Rung::ReportOnly,
      Rung::ReportOnly,
      Rung::ReportOnly,
      Rung::Stop,
    ],
  );
  assert_eq!(
    run.lhs_entries, 2,
    "`1`, then the hole the outer continuation descended onto — the inner continuation is refused \
     before it descends, so there is no third frame"
  );
  assert_eq!(
    run.folds, 0,
    "and the refusal is in front of the work, as both halves of the law are"
  );
  let refused = run.settle(true);
  if let Some(msg) = refused.refusal.as_deref() {
    assert!(
      msg.contains(DEBT_WORDING),
      "debug: the law that stopped this is the DEBT — the inner continuation descending on input \
       the outer one's own classifier was already paid with: {msg}"
    );
  }
  if let Some(out) = refused.outcome {
    assert_eq!(out, Err(AdjError::EoRhs), "release: the terminal report");
  }
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
      // `Neither` at the adjacency's own power, so a frame can be latched before the adjacency
      // arrives — the chain constraint is a property of the chain and not of the newcomer.
      Token::Eq => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        P_LOOSE,
      ))),
      // The report this engine cannot serve — but only once the floor has admitted it and the
      // chain constraint has let it through.
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

/// What one token-engine run did: the parser's own outcome, how many diagnostics it recorded,
/// and how many tokens it left on the input for the surrounding grammar.
struct TokRun {
  outcome: Result<Option<Token>, AdjError>,
  recorded: usize,
  left_on_input: usize,
}

/// Runs the token engine over `src` at a given floor, under a **recording** emitter.
///
/// The engine's own `Err` is captured rather than propagated, so a `NonAssociativeChain` is a
/// value this fixture asserts on instead of a failure of the surrounding parse — and the
/// diagnostic count and the handback are still read on that path.
fn token_engine_at<const FLOOR: i64>(src: &str) -> TokRun {
  fn probe<'inp, const FLOOR: i64>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<AdjError>>,
    >,
  ) -> Result<TokRun, AdjError> {
    let out = inp.pratt_with_min_precedence::<_, _, _, i64, i64>(
      tok_fold_unary::<Verbose<AdjError>>,
      tok_fold_infix::<Verbose<AdjError>>,
      tok_fold_unary::<Verbose<AdjError>>,
      FLOOR,
    );
    let recorded = inp.emitter_ref().errors().values().flatten().count();
    let mut left_on_input = 0usize;
    while inp.next()?.is_some() {
      left_on_input += 1;
    }
    Ok(TokRun {
      outcome: out.map(|o| o.map(|t| t.into_data())),
      recorded,
      left_on_input,
    })
  }

  Parser::with_context(ParserContext::new(Verbose::<AdjError>::new()))
    .apply(probe::<FLOOR>)
    .parse_str(src)
    .expect("a recording emitter carries the diagnostic instead of failing the parse")
}

/// The same, entered at the default floor.
fn token_engine(src: &str) -> (Option<Token>, usize) {
  let run = token_engine_at::<0>(src);
  (
    run
      .outcome
      .expect("the default-floor cells do not trip the chain constraint"),
    run.recorded,
  )
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

/// **An adjacency BELOW the floor is the surrounding grammar's, and is declined rather than
/// diagnosed.**
///
/// Parking the report the moment it is seen reads the payload as "adjacency" and never as
/// "adjacency *at a power*", so an embedded parse entered above the adjacency's power — the
/// ordinary way a Pratt expression is nested inside a larger grammar — is handed a diagnostic for
/// an operator that was never its own. The token stays on the input either way, which is what
/// makes the diagnostic count the whole differential: the engine's *value* is `1` on both sides.
///
/// Falsifier: drop the floor test from the `Adjacent` arm of `input_ref/pratt.rs` and `recorded`
/// goes to 1 while everything else in this cell is unchanged.
#[test]
fn an_adjacency_below_the_floor_is_declined_and_not_diagnosed() {
  let run = token_engine_at::<P_TIGHT>("1 , 2");
  assert_eq!(
    run.outcome,
    Ok(Some(Token::Num(1))),
    "at a floor above the adjacency's power the expression is the bare operand"
  );
  assert_eq!(
    run.recorded, 0,
    "and `,` belongs to whatever grammar wrapped this call, so this engine says nothing about it"
  );
  assert_eq!(
    run.left_on_input, 2,
    "`,` and `2` are handed back untouched"
  );
}

/// **A same-power adjacency after a `Neither` fold is a chain violation, and says so.**
///
/// `1 = 2 , 3`: the frame folds a non-associative `=` at the adjacency's power, and the `,` is the
/// second link. That is `NonAssociativeChain` — non-terminal, the token left on the input, a
/// recoverer's to spend — for exactly the reason it is one when the second link is spelled. Parked
/// unconditionally it became the unsupported-adjacency diagnostic instead: terminal in meaning,
/// wrong in kind, and returned as an `Ok` that a recoverer cannot act on.
///
/// The inner frame's decline is in this input too, and it is why `recorded` is 0 rather than 1:
/// `,` at power 1 is below `=`'s `Exclusive(1)` right-operand floor, so the inner frame hands it
/// back rather than diagnosing it.
#[test]
fn a_same_power_adjacency_after_a_neither_fold_is_a_chain_violation() {
  let run = token_engine_at::<0>("1 = 2 , 3");
  assert_eq!(
    run.outcome,
    Err(AdjError::NonAssoc),
    "the chain constraint outranks the engine's inability to serve the report — the second link \
     is malformed input, not an unsupported shape"
  );
  assert_eq!(
    run.recorded, 0,
    "the trip is returned, never emitted, and the inner frame's below-floor decline emits nothing \
     either"
  );
}

/// The control for both: an admitted, non-repeating adjacency still reaches the engine's
/// unsupported path. The two cells above narrow that path; they do not remove it.
#[test]
fn an_admitted_non_repeating_adjacency_still_reaches_the_refusal() {
  let run = token_engine_at::<P_LOOSE>("1 , 2");
  assert_eq!(run.outcome, Ok(Some(Token::Num(1))));
  assert_eq!(
    run.recorded, 1,
    "at the adjacency's own power, with no `Neither` latched, the report is this engine's and it \
     has no way to serve it"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Section four — a report variant chosen by lookahead (tokora#202, optional-RHS)
// ═══════════════════════════════════════════════════════════════════════════════
//
// `/` is r-a's `..`: infix in `1 / 2`, postfix in `1 /`, prefix in `/ 2`, and a bare operand in
// `/`. The driver contributes nothing to this and is not asked to — every one of the four reports
// consumes the `/` it names, so the report boundary holds in each. What the shape is here to
// demonstrate is that the *variant* may be decided by lookahead the channel takes for itself,
// which is the sub-case r-a gates with `Restrictions` and the one tokora#202 left unwitnessed.

const P_RANGE: i64 = 4;

fn range_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n.to_string())),
      // Both arms have already consumed the `/`; only the variant is still open, and one token
      // of lookahead closes it.
      Token::Slash => {
        if operand_can_begin(inp.peek_kind()?) {
          Ok(PrattLHS::Prefix(Precedenced::new("/", P_RANGE)))
        } else {
          Ok(PrattLHS::Operand("(/)".to_owned()))
        }
      }
      _ => Err(AdjError::Other),
    },
    None => Err(AdjError::Other),
  }
}

fn range_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, i64>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Slash => {
        if operand_can_begin(inp.peek_kind()?) {
          Ok(PrattRHS::Infix(Precedenced::new(
            PrattInfix::Left("/"),
            P_RANGE,
          )))
        } else {
          Ok(PrattRHS::Postfix(Precedenced::new("/", P_RANGE)))
        }
      }
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left("+"),
        P_LOOSE,
      ))),
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

fn range_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, i64>,
) -> Result<String, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(format!("({operand}{})", op.into_data()))
}

fn range_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Result<String, AdjError>, AdjError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = AdjError>,
{
  Ok(
    pratt(
      range_lhs,
      range_rhs,
      juxt_fold_prefix,
      juxt_fold_infix,
      range_fold_postfix,
    )
    .parse_input(inp),
  )
}

fn range(src: &str) -> Result<String, AdjError> {
  Parser::new()
    .apply(range_expr)
    .parse_str(src)
    .expect("the outer parser never fails")
}

/// All four variants of one operator, decided by one token of lookahead each.
#[test]
fn one_operator_reports_four_variants_by_lookahead() {
  assert_eq!(range("1 / 2"), Ok("(1 / 2)".to_owned()), "infix");
  assert_eq!(range("1 /"), Ok("(1/)".to_owned()), "postfix");
  assert_eq!(range("/ 2"), Ok("(/2)".to_owned()), "prefix");
  assert_eq!(range("/"), Ok("(/)".to_owned()), "bare operand");
}

/// The lookahead-chosen variant still obeys the ladder: `/` at 4 against `+` at 1.
#[test]
fn a_lookahead_chosen_variant_still_obeys_the_floor() {
  assert_eq!(range("1 + 2 / 3"), Ok("(1 + (2 / 3))".to_owned()));
  assert_eq!(range("1 / 2 + 3"), Ok("((1 / 2) + 3)".to_owned()));
  // The postfix reading nested under the looser operator: `2 /` closes, then `+` folds.
  assert_eq!(range("1 + 2 /"), Ok("(1 + (2/))".to_owned()));
}
