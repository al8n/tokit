#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The two pratt contracts that are not about precedence:
//!
//! * **R1 — the recursion budget.** Every pratt frame, in either engine, enters one level of the
//!   input's [`RecursionLimiter`] through `InputRef::descend`, and exceeding it fails the parse
//!   with the always-terminal `RecursionLimitReached`. On by default (depth 500), configurable
//!   through the context, shared by every parser on one input, and released on every exit
//!   including an unwind.
//! * **R2 — the non-associative contract.** A second same-power `PrattInfix::Neither` operator in
//!   one chain is a **syntax error** (`NonAssociativeChain`, non-terminal, operator left on the
//!   input), not a place to stop. Both engines, same trigger, same offset.
//!
//! # Termination defects do not fail; they hang or abort
//!
//! Every deep-recursion cell runs through [`on_a_deep_stack`], which gives the parse a stack far
//! larger than the harness's own *and* a hard wall-clock bound. Without the bigger stack a
//! pre-limit 600-deep chain aborts the whole test process with `has overflowed its stack`, which
//! is not a test result; without the timeout an un-decremented depth cell would hang instead of
//! failing. Both halves are load bearing.

mod common;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  emitter::{Fatal, PrattEmitter, Verbose},
  error::{
    MaybeTerminal, NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs,
    UnexpectedEot, token::UnexpectedTokenOf,
  },
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  state::recursion_tracker::RecursionLimiter,
  token::PrattToken,
};

use common::{Power, TestLexer, Token};

use tokora::utils::typenum::U4;

// ═══════════════════════════════════════════════════════════════════════════════
// The error type: it keeps what the engines report, so the assertions can be exact
// ═══════════════════════════════════════════════════════════════════════════════

/// A grammar error that **stores** both new payloads rather than collapsing them, so a test can
/// assert the offset the engine chose and so `is_terminal` can delegate — the migration posture
/// the terminal law asks of a consumer error type.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LimErr {
  Lex,
  Unexpected,
  Eot,
  EoLhs,
  EoRhs,
  /// A diagnostic a grammar hook emitted on purpose, so a Verbose log can be asserted whole.
  Note(usize),
  Limit {
    at: usize,
    depth: usize,
    limitation: usize,
  },
  NonAssoc {
    at: usize,
  },
}

/// Delegating, which is the point: a limit trip stays terminal after conversion, so recovery
/// re-raises it, and a non-associative repeat does not, so recovery may spend it.
impl MaybeTerminal for LimErr {
  fn is_terminal(&self) -> bool {
    matches!(self, LimErr::Limit { .. })
  }
}

impl From<()> for LimErr {
  fn from(_: ()) -> Self {
    LimErr::Lex
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, TestLexer<'inp>>> for LimErr {
  fn from(_: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Self {
    LimErr::Unexpected
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for LimErr {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    LimErr::Eot
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEoLhs<O, Lang, Set>> for LimErr {
  fn from(_: UnexpectedEoLhs<O, Lang, Set>) -> Self {
    LimErr::EoLhs
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEoRhs<O, Lang, Set>> for LimErr {
  fn from(_: UnexpectedEoRhs<O, Lang, Set>) -> Self {
    LimErr::EoRhs
  }
}
impl<Lang: ?Sized> From<RecursionLimitReached<usize, Lang>> for LimErr {
  fn from(e: RecursionLimitReached<usize, Lang>) -> Self {
    assert!(
      e.is_terminal(),
      "every `RecursionLimitReached` is terminal by construction"
    );
    LimErr::Limit {
      at: e.offset(),
      depth: e.depth(),
      limitation: e.limitation(),
    }
  }
}
impl<Lang: ?Sized> From<NonAssociativeChain<usize, Lang>> for LimErr {
  fn from(e: NonAssociativeChain<usize, Lang>) -> Self {
    assert!(
      !e.is_terminal(),
      "a non-associative repeat is malformed input, not a resource trip"
    );
    LimErr::NonAssoc { at: e.offset() }
  }
}
/// Never an incomplete: this fixture is complete-mode only. The empty impl is the documented
/// opt-in that `Recover` and friends require.
impl tokora::error::MaybeIncomplete for LimErr {}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for LimErr
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    LimErr::Unexpected
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// The ladder, shared by both engines
// ═══════════════════════════════════════════════════════════════════════════════
//
//   -  prefix          100   (the deep-recursion shape that costs one frame per operator)
//   *  infix Left        9   (binds tighter than the chain operators)
//   ;  infix Neither     5   (the chain constraint under test)
//   +  infix Left        5   (SAME power as `;`, different associativity)
//   /  postfix           5   (same power again, and postfix folds never touch the latch)
//   ,  infix Left        2   (below `;`: folding one clears the latch)
//   =  infix Right       1   (the enclosing operator that makes E2's misparse possible)

const P_PREFIX: Power = Power(100);
const P_HIGH: Power = Power(9);
const P_CHAIN: Power = Power(5);
const P_LOW: Power = Power(2);
const P_ASSIGN: Power = Power(1);

fn lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, Power>, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n.to_string())),
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new("-", P_PREFIX))),
      _ => Err(LimErr::Unexpected),
    },
    None => Err(LimErr::Eot),
  }
}

fn rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, Power>, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  Ok(match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left("*"), P_HIGH)),
      Token::Semi => PrattRHS::Infix(Precedenced::new(PrattInfix::Neither(";"), P_CHAIN)),
      Token::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left("+"), P_CHAIN)),
      Token::Slash => PrattRHS::Postfix(Precedenced::new("/", P_CHAIN)),
      Token::Comma => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(","), P_LOW)),
      Token::Eq => PrattRHS::Infix(Precedenced::new(PrattInfix::Right("="), P_ASSIGN)),
      _ => PrattRHS::End,
    },
    None => PrattRHS::End,
  })
}

fn fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, Power>,
) -> Result<String, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  Ok(format!("({}{operand})", op.into_data()))
}

fn fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: String,
  right: String,
  op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, Power>,
) -> Result<String, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  let (PrattInfix::Left(s) | PrattInfix::Right(s) | PrattInfix::Neither(s)) = op.into_data();
  Ok(format!("({left}{s}{right})"))
}

fn fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, Power>,
) -> Result<String, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  Ok(format!("({operand}{})", op.into_data()))
}

/// The outcome of one typed pratt parse plus the position the surrounding grammar is handed:
/// `Ok((tree, next_token_start))` or `Err(the engine's error)`.
type Outcome = Result<(String, Option<usize>), LimErr>;

/// Drives the typed engine and, whichever way it goes, reports where the input was left. The
/// pratt error is caught *inside* the grammar so the handback can be observed on the error path
/// too — the property that distinguishes "restored the deciding read" from "ate it".
fn typed_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Outcome, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  match pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp) {
    Ok(tree) => {
      let front = inp.next()?.map(|t| t.span().start());
      Ok(Ok((tree, front)))
    }
    Err(e) => Ok(Err(e)),
  }
}

/// [`typed_probe`] under a lower-power enclosing operator — the **nested** position, where a
/// truncating contract is unobservable because the enclosing frame folds the leftover operator
/// itself. Runs the same pratt parser at the entry floor, over a source that opens with `N = …`.
fn nested_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Outcome, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  typed_probe(inp)
}

/// The same as [`typed_probe`], with the lookahead window **prefilled** before the parse starts:
/// the trip offsets must not move with how much has been peeked.
fn prefilled_typed_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Outcome, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  // Fill the cache as far as it will go before a single token is consumed.
  let _ = inp.peek::<U4>()?;
  typed_probe(inp)
}

// ── The token engine over the same ladder ──────────────────────────────────────

impl PrattToken<'_, i64, Power> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    match self {
      Token::Num(_) => Some(PrattLHS::Operand(())),
      Token::Minus => Some(PrattLHS::Prefix(Precedenced::new((), P_PREFIX))),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>> {
    match self {
      Token::Star => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        P_HIGH,
      ))),
      Token::Semi => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        P_CHAIN,
      ))),
      Token::Plus => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        P_CHAIN,
      ))),
      Token::Slash => Some(PrattRHS::Postfix(Precedenced::new((), P_CHAIN))),
      Token::Comma => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        P_LOW,
      ))),
      Token::Eq => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(()),
        P_ASSIGN,
      ))),
      _ => None,
    }
  }
}

type Tok = tokora::span::Spanned<Token, tokora::SimpleSpan>;

fn tok_fold_prefix<E>(_op: Tok, operand: Tok, _: &mut E) -> Result<Tok, LimErr> {
  Ok(operand)
}

fn tok_fold_postfix<E>(operand: Tok, _op: Tok, _: &mut E) -> Result<Tok, LimErr> {
  Ok(operand)
}

/// Records the fold tree's shape in the value: each fold appends the right operand's digit, so
/// one fold over `1 ; 2` reads `12` and a second over `; 3` reads `123`.
fn tok_fold_infix<E>(
  left: Tok,
  right: Tok,
  infix: tokora::span::Spanned<PrattInfix<Token, Token, Token>, tokora::SimpleSpan>,
  _: &mut E,
) -> Result<Tok, LimErr> {
  let value = match (left.data(), right.data()) {
    (Token::Num(l), Token::Num(r)) => l * 10 + r,
    _ => return Err(LimErr::Unexpected),
  };
  Ok(tokora::span::Spanned::new(
    infix.into_span(),
    Token::Num(value),
  ))
}

/// The token engine's twin of [`typed_probe`]: value plus handback, or the engine's error.
type TokOutcome = Result<(i64, Option<usize>), LimErr>;

fn token_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<TokOutcome, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = LimErr> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  match inp.pratt::<_, _, _, i64, Power>(
    tok_fold_prefix::<Ctx::Emitter>,
    tok_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
  ) {
    Ok(out) => {
      let value = match out.expect("the input opens with an operand").into_data() {
        Token::Num(n) => n,
        _ => return Err(LimErr::Unexpected),
      };
      let front = inp.next()?.map(|t| t.span().start());
      Ok(Ok((value, front)))
    }
    Err(e) => Ok(Err(e)),
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Harness
// ═══════════════════════════════════════════════════════════════════════════════

fn fatal_ctx<'inp>() -> ParserContext<'inp, TestLexer<'inp>, Fatal<LimErr>> {
  ParserContext::new(Fatal::new())
}

fn limited<'inp>(limit: usize) -> ParserContext<'inp, TestLexer<'inp>, Fatal<LimErr>> {
  fatal_ctx().with_recursion_limiter(RecursionLimiter::with_limitation(limit))
}

/// Runs `f` on a thread with a stack far larger than the harness's own and a hard wall-clock
/// bound. A recursion-limit defect does not fail an assertion — it hangs, or aborts the process
/// — so the bound has to live outside the code under test. The stack has to be bigger for the
/// **unlimited** cells, which by construction run deeper than the harness's 2 MiB allows.
fn on_a_deep_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
  let (tx, rx) = std::sync::mpsc::channel();
  let handle = std::thread::Builder::new()
    .stack_size(256 * 1024 * 1024)
    .spawn(move || {
      let _ = tx.send(f());
    })
    .expect("spawn the deep-stack worker");
  match rx.recv_timeout(std::time::Duration::from_secs(120)) {
    Ok(v) => {
      handle.join().expect("the deep-stack worker panicked");
      v
    }
    Err(e) => panic!("the parse did not terminate within 120s: {e:?}"),
  }
}

/// `- - - … 1`, one prefix operator per level.
fn prefix_chain(depth: usize) -> String {
  let mut src = String::with_capacity(depth * 2 + 1);
  for _ in 0..depth {
    src.push_str("- ");
  }
  src.push('1');
  src
}

/// `1 = 1 = … = 1`: right-associative, so the chain descends one frame per operator.
fn right_chain(depth: usize) -> String {
  let mut src = String::with_capacity(depth * 4 + 1);
  src.push('1');
  for _ in 0..depth {
    src.push_str(" = 1");
  }
  src
}

// ═══════════════════════════════════════════════════════════════════════════════
// R2 — the non-associative contract
// ═══════════════════════════════════════════════════════════════════════════════

/// The ladder-top case, typed: `1 ; 2 ; 3` folds once and then **fails** on the second `;`,
/// which is handed back unconsumed at its own offset.
///
/// Before this contract landed the same input returned `Ok(("(1;2)", 2 tokens left))`.
#[test]
fn typed_repeat_at_the_top_level_is_rejected_and_hands_the_operator_back() {
  let got: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(
    got,
    Err(LimErr::NonAssoc { at: 6 }),
    "the second `;` sits at offset 6 and is rejected there"
  );
}

/// **The E2 pin**, typed. `7 = 1 ; 2 ; 3` used to parse to completion as `((7=(1;2));3)` with an
/// empty remainder: the frame holding the latch broke, and the enclosing `=` frame — whose own
/// latch is `None` and whose floor admits power 5 — folded the second `;` itself. No EOI check
/// can see that, because nothing is left over. This is the case that makes truncation
/// unavailable as a contract, and it must stay an error forever.
#[test]
fn typed_repeat_under_an_enclosing_operator_is_rejected_not_reassociated() {
  let got: Outcome = Parser::with_context(fatal_ctx())
    .apply(nested_probe)
    .parse_str("7 = 1 ; 2 ; 3")
    .unwrap();
  assert_eq!(
    got,
    Err(LimErr::NonAssoc { at: 10 }),
    "the second `;` is at offset 10; re-associating it across the `=` is the misparse this pins"
  );
}

/// The token engine, same two inputs, same offsets: the parity the contract requires.
#[test]
fn token_repeat_is_rejected_at_the_same_offsets_as_the_typed_engine() {
  let top: TokOutcome = Parser::with_context(fatal_ctx())
    .apply(token_probe)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(top, Err(LimErr::NonAssoc { at: 6 }));

  let nested: TokOutcome = Parser::with_context(fatal_ctx())
    .apply(token_probe)
    .parse_str("7 = 1 ; 2 ; 3")
    .unwrap();
  assert_eq!(nested, Err(LimErr::NonAssoc { at: 10 }));
}

/// The trigger is the **power**, not the newcomer's associativity: a `Left` operator at the
/// chain's own power trips it too, because the constraint belongs to the chain.
#[test]
fn a_left_operator_at_the_same_power_trips_the_chain() {
  let typed: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 ; 2 + 3")
    .unwrap();
  assert_eq!(typed, Err(LimErr::NonAssoc { at: 6 }));

  let token: TokOutcome = Parser::with_context(fatal_ctx())
    .apply(token_probe)
    .parse_str("1 ; 2 + 3")
    .unwrap();
  assert_eq!(token, Err(LimErr::NonAssoc { at: 6 }));
}

/// A **lower-power** infix ends the chain: the latch frame folds it, clears the latch, and the
/// parse completes. The second `;` is folded inside that operator's right operand, at a floor
/// that admits it — which is exactly why it is not a repeat.
///
/// (Why the chain cannot be re-armed in the *same* frame instead: after a `Neither` fold at `p`
/// the recursion at `Exclusive(p)` has already consumed everything binding tighter than `p`, so
/// the only operators this frame can still see have power `<= p`. Equal trips; lower ends the
/// chain and takes the rest of the expression with it. That is the whole of why a one-step latch
/// is sufficient.)
#[test]
fn a_lower_power_infix_ends_the_chain() {
  let got: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 ; 2 , 3 ; 4")
    .unwrap();
  assert_eq!(
    got,
    Ok((String::from("((1;2),(3;4))"), None)),
    "`,` at a lower power ends the `;` chain rather than continuing it"
  );
}

/// The latch is armed by a `Neither` fold and by nothing else: a `Left` operator at the chain's
/// power leaves it clear, so a `Neither` at the same power right after it is a first link, not a
/// repeat. The mirror image of `a_left_operator_at_the_same_power_trips_the_chain`, and the pair
/// is what makes the contract per-operator rather than per-power.
#[test]
fn a_left_fold_does_not_arm_the_latch() {
  let got: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 + 2 ; 3")
    .unwrap();
  assert_eq!(got, Ok((String::from("((1+2);3)"), None)));

  let token: TokOutcome = Parser::with_context(fatal_ctx())
    .apply(token_probe)
    .parse_str("1 + 2 ; 3")
    .unwrap();
  assert_eq!(token, Ok((123, None)), "the token engine agrees");
}

/// A **postfix** fold does not touch the latch: it binds the already-folded left operand and
/// cannot re-associate the chain, so a same-power infix arriving after one still trips.
#[test]
fn the_latch_survives_a_postfix_fold() {
  let clean: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 ; 2 /")
    .unwrap();
  assert_eq!(
    clean,
    Ok((String::from("((1;2)/)"), None)),
    "the postfix folds; nothing about it is a repeat"
  );

  let tripped: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 ; 2 / ; 3")
    .unwrap();
  assert_eq!(
    tripped,
    Err(LimErr::NonAssoc { at: 8 }),
    "the postfix left the latch alone, so the `;` after it is still a repeat"
  );
}

/// A tighter operator between the two chain operators is invisible to the latch frame — it is
/// consumed inside the recursive right operand — so `a ; b * c ; d` trips exactly as `a ; b ; d`
/// does. The one-step latch is sufficient because the recursion at `Exclusive(p)` has already
/// eaten everything binding tighter than `p`.
#[test]
fn a_tighter_operator_inside_the_right_operand_does_not_clear_the_latch() {
  let got: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 ; 2 * 3 ; 4")
    .unwrap();
  assert_eq!(got, Err(LimErr::NonAssoc { at: 10 }));
}

/// A prefilled lookahead window changes nothing — not the outcome, not the offset.
#[test]
fn the_repeat_offset_is_the_same_with_a_prefilled_cache() {
  let empty: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("7 = 1 ; 2 ; 3")
    .unwrap();
  let prefilled: Outcome = Parser::with_context(fatal_ctx())
    .apply(prefilled_typed_probe)
    .parse_str("7 = 1 ; 2 ; 3")
    .unwrap();
  assert_eq!(empty, prefilled);
  assert_eq!(prefilled, Err(LimErr::NonAssoc { at: 10 }));
}

/// Under a **recording** emitter the repeat still surfaces as an `Err` — a recording emitter must
/// not be able to reproduce the truncation the contract bans — and the log holds exactly the
/// diagnostics that were emitted before the trip: none erased, none added.
#[test]
fn a_recording_emitter_sees_the_repeat_as_an_error_and_keeps_its_earlier_log() {
  /// Emits one note per operand parsed, so the log has content to preserve.
  fn noting_lhs<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<PrattLHS<String, &'static str, Power>, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let at = inp.span().end();
    let out = lhs(inp)?;
    if matches!(out, PrattLHS::Operand(_)) {
      let span = tokora::SimpleSpan::new(at, at);
      inp
        .emitter()
        .emit_error(tokora::span::Spanned::new(span, LimErr::Note(at)))?;
    }
    Ok(out)
  }

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Outcome, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    match pratt(noting_lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp) {
      Ok(tree) => Ok(Ok((tree, inp.next()?.map(|t| t.span().start())))),
      Err(e) => Ok(Err(e)),
    }
  }

  let mut emitter = Verbose::<LimErr>::new();
  let got: Outcome = Parser::with_context((
    &mut emitter,
    tokora::cache::DefaultCache::<TestLexer<'_>>::default(),
  ))
  .apply(probe)
  .parse_str("1 ; 2 ; 3")
  .unwrap();
  assert_eq!(
    got,
    Err(LimErr::NonAssoc { at: 6 }),
    "returned, never emitted: a recording emitter cannot turn the repeat into a success"
  );

  let notes: Vec<LimErr> = emitter.errors().values().flatten().cloned().collect();
  assert_eq!(
    notes,
    std::vec![LimErr::Note(0), LimErr::Note(3)],
    "the two operands parsed before the trip kept their diagnostics; the aborted cycle added \
     none and erased none"
  );
}

/// The repeat is **not terminal**, so an explicit recovery may spend it — which is how a grammar
/// asks for the tolerant reading the engine no longer applies silently.
#[test]
fn an_explicit_recovery_may_spend_the_repeat() {
  fn recovery<'inp, Ctx>(
    _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
    err: LimErr,
  ) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    assert_eq!(
      err,
      LimErr::NonAssoc { at: 6 },
      "the recoverer is handed the repeat itself"
    );
    Ok(String::from("<recovered>"))
  }

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(recovery)
      .parse_input(inp)
  }

  let got: String = Parser::with_context(fatal_ctx())
    .apply(probe)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(got, "<recovered>");
}

// ═══════════════════════════════════════════════════════════════════════════════
// R1 — the recursion budget
// ═══════════════════════════════════════════════════════════════════════════════

/// A configured limit stops a deep prefix chain at the frame after the limit, in **both**
/// engines, at the same depth, with the terminal marker intact.
///
/// Depth 9 against limit 8: the root frame counts as one level, so the ninth live frame is the
/// one that cannot be entered.
#[test]
fn a_prefix_chain_trips_the_configured_limit_in_both_engines() {
  let typed = on_a_deep_stack(|| {
    let src = prefix_chain(32);
    Parser::with_context(limited(8))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  // `at` is committed consumption at the refused frame's entry: eight `-` tokens are already
  // consumed by then, and in `"- - - … 1"` the eighth ends at offset 15.
  assert_eq!(
    typed,
    Err(LimErr::Limit {
      at: 15,
      depth: 9,
      limitation: 8
    }),
    "32 prefix operators against a limit of 8 trip at the ninth frame"
  );

  let token = on_a_deep_stack(|| {
    let src = prefix_chain(32);
    Parser::with_context(limited(8))
      .apply(token_probe)
      .parse_str(&src)
      .unwrap()
  });
  let (Err(t), Err(k)) = (&typed, &token) else {
    panic!("both engines must trip: typed={typed:?} token={token:?}");
  };
  assert_eq!(
    t, k,
    "both engines trip at the same depth and the same offset"
  );
}

/// The same, for the right-associative shape — the other native-stack surface.
#[test]
fn a_right_associative_chain_trips_the_configured_limit_in_both_engines() {
  let typed = on_a_deep_stack(|| {
    let src = right_chain(32);
    Parser::with_context(limited(8))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  let token = on_a_deep_stack(|| {
    let src = right_chain(32);
    Parser::with_context(limited(8))
      .apply(token_probe)
      .parse_str(&src)
      .unwrap()
  });
  let (Err(t), Err(k)) = (&typed, &token) else {
    panic!("both engines must trip: typed={typed:?} token={token:?}");
  };
  assert_eq!(t, k, "both engines trip at the same depth and offset");
  assert!(
    matches!(
      t,
      LimErr::Limit {
        depth: 9,
        limitation: 8,
        ..
      }
    ),
    "the ninth frame is the one refused; got {t:?}"
  );
}

/// The trip is **terminal** as the engine raises it, before any conversion — the property
/// `is_terminal` delegation must preserve.
#[test]
fn the_trip_value_is_terminal() {
  let got = on_a_deep_stack(|| {
    let src = prefix_chain(32);
    Parser::with_context(limited(8))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  match got {
    Err(e) => assert!(e.is_terminal(), "the stored trip stays terminal: {e:?}"),
    Ok(v) => panic!("expected a trip, got {v:?}"),
  }
}

/// A prefilled lookahead window does not move the trip: the offset is committed consumption, not
/// the cache front.
#[test]
fn the_trip_is_identical_with_an_empty_and_a_prefilled_cache() {
  let empty = on_a_deep_stack(|| {
    let src = prefix_chain(32);
    Parser::with_context(limited(8))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  let prefilled = on_a_deep_stack(|| {
    let src = prefix_chain(32);
    Parser::with_context(limited(8))
      .apply(prefilled_typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  assert_eq!(empty, prefilled);
}

/// A recoverer **may not** spend a trip: `Recover` consults `MaybeTerminal` and re-raises, so
/// the recovery body never runs. The contrast with
/// `an_explicit_recovery_may_spend_the_repeat` is the whole point of the two classifications.
#[test]
fn a_recoverer_re_raises_a_trip_instead_of_spending_it() {
  fn recovery<'inp, Ctx>(
    _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
    err: LimErr,
  ) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    panic!("a terminal trip must never reach a recoverer, but it did: {err:?}");
  }

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(recovery)
      .parse_input(inp)
  }

  let got = on_a_deep_stack(|| {
    let src = prefix_chain(32);
    Parser::with_context(limited(8))
      .apply(probe)
      .parse_str(&src)
  });
  assert!(
    matches!(got, Err(LimErr::Limit { depth: 9, .. })),
    "the trip is re-raised untouched; got {got:?}"
  );
}

/// `skip_then_retry` re-raises a trip too, and — the part worth pinning — it does so **before**
/// any skipping: no amount of input clears a depth budget, so a retry would only re-trip.
#[test]
fn skip_then_retry_re_raises_a_trip_without_skipping() {
  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Result<String, LimErr>, Option<usize>), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .skip_then_retry(
        |_: &common::TokenKind| tokora::input::Balance::<char>::Neutral,
        |tok: tokora::span::Spanned<&Token, &tokora::SimpleSpan>| matches!(tok.data(), Token::Semi),
      )
      .parse_input(inp);
    let front = inp.next()?.map(|t| tokora::span::Span::start(t.span_ref()));
    Ok((out, front))
  }

  let (out, front) = on_a_deep_stack(|| {
    // A `;` sync point sits after the deep chain, so a recoverable failure WOULD have skipped.
    let src = std::format!("{} ; 1", prefix_chain(32));
    Parser::with_context(limited(8))
      .apply(probe)
      .parse_str(&src)
      .unwrap()
  });
  assert!(
    matches!(out, Err(LimErr::Limit { depth: 9, .. })),
    "the trip is re-raised, not spent; got {out:?}"
  );
  assert_eq!(
    front,
    Some(0),
    "and nothing was skipped: `try_attempt` rolled the failed parse back to the start"
  );
}

/// Diagnostics emitted before the trip survive it: the trip is returned, not emitted, and it
/// rewinds nothing.
#[test]
fn a_pre_trip_diagnostic_survives_the_trip() {
  fn noting_lhs<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<PrattLHS<String, &'static str, Power>, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let at = inp.span().end();
    let span = tokora::SimpleSpan::new(at, at);
    inp
      .emitter()
      .emit_error(tokora::span::Spanned::new(span, LimErr::Note(at)))?;
    lhs(inp)
  }

  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Result<String, LimErr>, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    Ok(pratt(noting_lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp))
  }

  let mut emitter = Verbose::<LimErr>::new();
  let got = Parser::with_context(
    ParserContext::<'_, TestLexer<'_>, _>::new(&mut emitter)
      .with_recursion_limiter(RecursionLimiter::with_limitation(3)),
  )
  .apply(probe)
  .parse_str("- - - - - 1")
  .unwrap();
  assert!(
    matches!(got, Err(LimErr::Limit { depth: 4, .. })),
    "the fourth frame trips; got {got:?}"
  );
  let notes: Vec<LimErr> = emitter.errors().values().flatten().cloned().collect();
  assert_eq!(
    notes,
    std::vec![LimErr::Note(0), LimErr::Note(1), LimErr::Note(3)],
    "every note emitted before the trip is still in the log after it — one per frame entered, \
     keyed at that frame's committed consumption"
  );
}

/// Limit 0 refuses even the root frame — the documented degenerate configuration — and limit 1
/// admits an atom but nothing that recurses. Together they pin "the root counts as one level".
#[test]
fn the_degenerate_limits_pin_that_the_root_counts() {
  let zero: Outcome = Parser::with_context(limited(0))
    .apply(typed_probe)
    .parse_str("1")
    .unwrap();
  assert_eq!(
    zero,
    Err(LimErr::Limit {
      at: 0,
      depth: 1,
      limitation: 0
    }),
    "with no budget at all even the root expression cannot be entered"
  );

  let atom: Outcome = Parser::with_context(limited(1))
    .apply(typed_probe)
    .parse_str("1")
    .unwrap();
  assert_eq!(
    atom,
    Ok((String::from("1"), None)),
    "one level is exactly one bare expression"
  );

  let recursing: Outcome = Parser::with_context(limited(1))
    .apply(typed_probe)
    .parse_str("1 = 2")
    .unwrap();
  assert!(
    matches!(
      recursing,
      Err(LimErr::Limit {
        depth: 2,
        limitation: 1,
        ..
      })
    ),
    "the infix right operand is a second frame; got {recursing:?}"
  );
}

/// The budget belongs to the **input session**, not to a parser: an operand parser that runs a
/// whole nested pratt expression draws on the same depth as the expression containing it.
#[test]
fn two_composed_pratt_parsers_share_one_budget() {
  /// `( … )` runs a *second*, complete pratt parse for the parenthesized operand.
  fn nesting_lhs<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<PrattLHS<String, &'static str, Power>, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    if inp
      .try_expect(|t| matches!(t.data(), Token::LParen))?
      .is_some()
    {
      let inner =
        pratt(nesting_lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)?;
      inp
        .try_expect(|t| matches!(t.data(), Token::RParen))?
        .ok_or(LimErr::Unexpected)?;
      return Ok(PrattLHS::Operand(format!("[{inner}]")));
    }
    lhs(inp)
  }

  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Result<String, LimErr>, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    Ok(pratt(nesting_lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp))
  }

  // Three nested expressions: outer + two inner pratt parses = three frames.
  let three: Result<String, LimErr> = Parser::with_context(limited(3))
    .apply(probe)
    .parse_str("((1))")
    .unwrap();
  assert_eq!(
    three,
    Ok(String::from("[[1]]")),
    "three levels fit in three"
  );

  let too_deep: Result<String, LimErr> = Parser::with_context(limited(2))
    .apply(probe)
    .parse_str("((1))")
    .unwrap();
  assert!(
    matches!(
      too_deep,
      Err(LimErr::Limit {
        depth: 3,
        limitation: 2,
        ..
      })
    ),
    "the inner parsers do not each get their own budget; got {too_deep:?}"
  );
}

/// **Protection is on by default.** An unconfigured `parse_str` over a chain deeper than 500
/// fails terminally instead of risking a native-stack abort — and `unlimited()` puts the deep
/// parse back, which proves the default is the thing doing the refusing.
#[test]
fn the_default_budget_refuses_a_deeper_chain_and_unlimited_restores_it() {
  let defaulted = on_a_deep_stack(|| {
    let src = prefix_chain(600);
    Parser::new()
      .apply(typed_probe)
      .parse_str(&src)
      .map_err(|e: LimErr| e)
      .unwrap()
  });
  assert!(
    matches!(
      defaulted,
      Err(LimErr::Limit {
        depth: 501,
        limitation: 500,
        ..
      })
    ),
    "the default budget is 500 and the 501st frame is refused; got {defaulted:?}"
  );

  // Deliberately far below the measured native threshold: this cell proves `unlimited` removes
  // the *configured* bound, not that the machine has an infinite stack.
  let unlimited = on_a_deep_stack(|| {
    let src = prefix_chain(1_000);
    Parser::with_context(fatal_ctx().with_recursion_limiter(RecursionLimiter::unlimited()))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  let (tree, front) = unlimited.expect("`unlimited()` lets the deep parse through");
  assert_eq!(front, None, "the whole chain was consumed");
  assert_eq!(
    tree.matches('-').count(),
    1_000,
    "all 1000 prefix operators folded"
  );

  // And the same on the token engine, so "default on" is not a typed-only claim.
  let token_defaulted = on_a_deep_stack(|| {
    let src = prefix_chain(600);
    Parser::new()
      .apply(token_probe)
      .parse_str(&src)
      .map_err(|e: LimErr| e)
      .unwrap()
  });
  assert!(
    matches!(
      token_defaulted,
      Err(LimErr::Limit {
        depth: 501,
        limitation: 500,
        ..
      })
    ),
    "the token engine honours the same default; got {token_defaulted:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// R1 — the level is released on every exit
// ═══════════════════════════════════════════════════════════════════════════════

/// After a successful parse and after a failing one, the depth is back to where it started.
///
/// The reads happen inside the grammar, through `InputRef::recursion()`, which is the only place
/// the live cell is visible.
#[test]
fn the_depth_is_balanced_after_success_and_after_an_error() {
  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize, usize), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let before = inp.recursion().depth();
    let ok = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp);
    assert!(ok.is_ok(), "the first parse succeeds: {ok:?}");
    let after_ok = inp.recursion().depth();
    // A second parse over the same handle, this time one that fails inside a fold.
    let failed = pratt(lhs, rhs, failing_fold_infix, fold_infix, fold_postfix).parse_input(inp);
    assert!(failed.is_err(), "the second parse fails: {failed:?}");
    Ok((before, after_ok, inp.recursion().depth()))
  }

  /// A prefix fold that always fails, so the frame exits through `?` rather than a return.
  fn failing_fold_infix<'inp, Ctx>(
    _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
    _operand: String,
    _op: Precedenced<&'static str, Power>,
  ) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    Err(LimErr::Unexpected)
  }

  let (before, after_ok, after_err) = Parser::with_context(fatal_ctx())
    .apply(probe)
    .parse_str("1 = 2 - 3")
    .unwrap();
  assert_eq!(
    (before, after_ok, after_err),
    (0, 0, 0),
    "every level entered was left, on the success path and on the `?` path alike"
  );
}

/// An **unwind** through the driver releases every level it crossed. The panic is raised inside a
/// fold at depth 3 and caught by the surrounding grammar, which then reads the cell back.
#[test]
fn an_unwind_through_the_driver_releases_every_level() {
  /// Panics once the driver is three frames deep, which `1 = 1 = 1` reaches.
  fn panicking_fold_infix<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
    _left: String,
    _right: String,
    _op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, Power>,
  ) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    assert!(
      inp.recursion().depth() >= 2,
      "the fold runs with its frame still live"
    );
    panic!("pratt_limit: deliberate fold panic");
  }

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    assert_eq!(inp.recursion().depth(), 0, "nothing is live yet");
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      pratt(lhs, rhs, fold_prefix, panicking_fold_infix, fold_postfix).parse_input(inp)
    }));
    assert!(unwound.is_err(), "the fold's panic crossed the driver");
    Ok(inp.recursion().depth())
  }

  let previous = std::panic::take_hook();
  std::panic::set_hook(std::boxed::Box::new(|_| {}));
  let depth = Parser::with_context(fatal_ctx())
    .apply(probe)
    .parse_str("1 = 1 = 1")
    .unwrap();
  std::panic::set_hook(previous);
  assert_eq!(
    depth, 0,
    "every frame the unwind popped released its level: the guard's drop is not policy-dependent"
  );
}

/// The same for an unwind out of the **RHS classifier**, which runs inside the probe guard rather
/// than after it — a different unwind edge, same obligation.
#[test]
fn an_unwind_out_of_the_rhs_classifier_releases_every_level() {
  fn panicking_rhs<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, Power>, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    if inp.recursion().depth() >= 2 {
      panic!("pratt_limit: deliberate classifier panic");
    }
    rhs(inp)
  }

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      pratt(lhs, panicking_rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
    }));
    assert!(
      unwound.is_err(),
      "the classifier's panic crossed the driver"
    );
    Ok(inp.recursion().depth())
  }

  let previous = std::panic::take_hook();
  std::panic::set_hook(std::boxed::Box::new(|_| {}));
  let depth = Parser::with_context(fatal_ctx())
    .apply(probe)
    .parse_str("1 = 1 = 1")
    .unwrap();
  std::panic::set_hook(previous);
  assert_eq!(depth, 0);
}

/// And out of the **LHS parser**, the third channel.
#[test]
fn an_unwind_out_of_the_lhs_parser_releases_every_level() {
  fn panicking_lhs<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<PrattLHS<String, &'static str, Power>, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    if inp.recursion().depth() >= 3 {
      panic!("pratt_limit: deliberate LHS panic");
    }
    lhs(inp)
  }

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      pratt(panicking_lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
    }));
    assert!(
      unwound.is_err(),
      "the LHS parser's panic crossed the driver"
    );
    Ok(inp.recursion().depth())
  }

  let previous = std::panic::take_hook();
  std::panic::set_hook(std::boxed::Box::new(|_| {}));
  let depth = Parser::with_context(fatal_ctx())
    .apply(probe)
    .parse_str("1 = 1 = 1 = 1")
    .unwrap();
  std::panic::set_hook(previous);
  assert_eq!(depth, 0);
}

/// A trip leaves the depth exactly as it found it, so a caller that catches one and parses
/// something else is not handed a permanently shallower budget.
#[test]
fn a_trip_leaves_the_depth_unchanged() {
  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let before = inp.recursion().depth();
    let tripped = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp);
    assert!(
      matches!(tripped, Err(LimErr::Limit { .. })),
      "the parse must trip: {tripped:?}"
    );
    Ok((before, inp.recursion().depth()))
  }

  let (before, after) = Parser::with_context(limited(2))
    .apply(probe)
    .parse_str("1 = 2 = 3")
    .unwrap();
  assert_eq!(
    (before, after),
    (0, 0),
    "`descend` decrements before it builds the error, so the trip is net zero"
  );
}

/// A **configured** budget protects an ordinary thread stack: this cell runs on the harness's own
/// 2 MiB thread, with no deep-stack helper, over an input a thousand levels deep. Before the
/// budget existed the same input killed the whole test process with `has overflowed its stack` —
/// which is why this cell exists and why it must never be moved onto `on_a_deep_stack`.
///
/// The limit is 32, far below the default 500, on purpose — and the margin is chosen from a
/// measurement rather than guessed. Bisected on this tree, one frame per level, on a 2 MiB
/// thread, taking the last depth that completes before the process aborts:
///
/// | build | typed | token |
/// |---|---|---|
/// | release | 3871 | 4247 |
/// | debug | 384 | **125** |
///
/// This cell runs in whatever profile the suite is built in, so it has to fit the **smallest** of
/// those four numbers with room to spare: 32 clears the debug token engine by ~3.9×. An earlier
/// 100 fit too, but only by 1.25× — close enough that a codegen change on another platform could
/// turn this cell from a failure into a process abort, which is not a test result.
///
/// The same table is why the shipped default is 500: on the 2 MiB stack a spawned thread gets, a
/// **release** build clears 500 by ~7.7×, so the limiter fires long before the stack does. A
/// debug build does not, which is exactly why every cell above that exercises the default runs on
/// `on_a_deep_stack` instead.
#[test]
fn a_configured_budget_holds_on_an_ordinary_thread_stack() {
  let src = prefix_chain(1_000);
  let got: Outcome = Parser::with_context(limited(32))
    .apply(typed_probe)
    .parse_str(&src)
    .unwrap();
  assert!(
    matches!(
      got,
      Err(LimErr::Limit {
        depth: 33,
        limitation: 32,
        ..
      })
    ),
    "the trip happens long before the stack does; got {got:?}"
  );

  let token: TokOutcome = Parser::with_context(limited(32))
    .apply(token_probe)
    .parse_str(&src)
    .unwrap();
  assert!(
    matches!(
      token,
      Err(LimErr::Limit {
        depth: 33,
        limitation: 32,
        ..
      })
    ),
    "the token engine too; got {token:?}"
  );
}
