#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The two pratt contracts that are not about precedence:
//!
//! * **R1 — the recursion budget.** Every pratt frame, in either engine, enters one level of the
//!   input's [`RecursionLimiter`] through `InputRef::descend`, and exceeding it fails the parse
//!   with the always-terminal `RecursionLimitReached`. On by default (at
//!   `RecursionLimiter::PARSE_DEFAULT_DEPTH`), configurable
//!   through the context, shared by every parser on one input, and released on every exit
//!   including an unwind.
//! * **R2 — the non-associative contract.** A second same-power `PrattInfix::Neither` operator in
//!   one chain is a **syntax error** (`NonAssociativeChain`, non-terminal, operator left on the
//!   input), not a place to stop. Both engines, same trigger. The offset both report is the
//!   **handback position**, and that is one specific number: `InputRef::span().end()` after the
//!   error comes back — the committed frontier the input was restored (typed engine) or parked
//!   (token engine) to. Every offset cell here reads that frontier and asserts `at == frontier`,
//!   so the claim is a measured identity rather than a literal that happens to match.
//!
//!   It is deliberately **not** "the offending operator's head", and the two differ whenever
//!   anything the caller is also handed back sits between them: whitespace the lexer skips, trivia
//!   tokens a `ParsePrattRHS` skips, the gap inside a multi-token operator, or a region a
//!   non-fatal lexer error was reported over. This file has one fixture per shape, and each pins
//!   the identity against the position the operator actually starts at, so a future edit that
//!   "corrects" the offset to the head reds with a reason.
//!
//! # Termination defects do not fail; they hang or abort
//!
//! Every deep-recursion cell runs through [`on_a_deep_stack`], which gives the parse a stack far
//! larger than the harness's own *and* a hard wall-clock bound. Without the bigger stack a
//! pre-limit 600-deep chain aborts the whole test process with `has overflowed its stack`, which
//! is not a test result; without the timeout an un-decremented depth cell would hang instead of
//! failing. Both halves are load bearing.

mod common;

use tokora::EmitterView;
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext, Token as TokenT,
  emitter::{Fatal, PrattEmitter, Verbose},
  error::{
    MaybeTerminal, NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs,
    UnexpectedEot, token::UnexpectedToken,
  },
  logos::{self, Logos},
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  state::recursion_tracker::RecursionLimiter,
  token::PrattToken,
};

use common::{Power, TestLexer, Token, TokenKind};

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
  /// Kept whole rather than collapsed, because the **report-boundary stall** arrives through this
  /// channel and a cell that pins it has to be able to tell it from an ordinary end of RHS: the
  /// stall is marked terminal and names the position the offending report failed to advance past.
  EoRhs {
    at: usize,
    terminal: bool,
  },
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
    matches!(
      self,
      LimErr::Limit { .. } | LimErr::EoRhs { terminal: true, .. }
    )
  }
}

impl From<()> for LimErr {
  fn from(_: ()) -> Self {
    LimErr::Lex
  }
}
/// Spelled with concrete parameters rather than `UnexpectedTokenOf<'inp, TestLexer<'inp>>`, which
/// is the same type: coherence does not normalize the alias's `<L as Lexer<'inp>>::Token`
/// projection, so an impl written through the alias reads to the overlap check as a blanket impl
/// over `UnexpectedToken<'_, _, _, _>` and collides with the second lexer's impl further down.
/// Uses of the bound normalize normally, so every `Ctx::Emitter: Emitter<…, Error = LimErr>`
/// obligation is satisfied exactly as before.
impl<'inp> From<UnexpectedToken<'inp, Token, TokenKind>> for LimErr {
  fn from(_: UnexpectedToken<'inp, Token, TokenKind>) -> Self {
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
impl<Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEoRhs<usize, Lang, Set>> for LimErr {
  fn from(e: UnexpectedEoRhs<usize, Lang, Set>) -> Self {
    LimErr::EoRhs {
      at: e.offset(),
      terminal: e.is_terminal(),
    }
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
    // The rendered text of the value the ENGINE built, kept before the conversion discards it.
    // `LimErr` keeps the offset, so every cell in this file went on passing while `Display`
    // named that number something it is not — see
    // `the_render_names_the_handback_and_not_the_operator`.
    RENDERED_CHAIN.with(|c| *c.borrow_mut() = Some(e.to_string()));
    LimErr::NonAssoc { at: e.offset() }
  }
}

thread_local! {
  /// What `Display` produced for the last [`NonAssociativeChain`] the engines handed this
  /// fixture — the caller-visible surface, captured at the conversion boundary because the
  /// grammar's error type keeps the payload's offset rather than the payload itself.
  static RENDERED_CHAIN: core::cell::RefCell<Option<String>> =
    const { core::cell::RefCell::new(None) };
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

// ── A genuinely MULTI-TOKEN operator, and a zero-width one ─────────────────────
//
// The ladder above spells every operator with one token, and for a one-token operator "the start
// of the operator", "the start of the last token the classifier consumed" and "where the handback
// leaves the input" are the same number — so nothing above can tell a driver that names one of
// them from a driver that names another. These two classifiers separate them.

/// `< >` — one non-associative infix operator, spelled with **two** tokens and a gap between
/// them, at the chain's own power.
///
/// A `ParsePrattRHS` holds a whole `InputRef` and may consume as much as its operator needs;
/// `not in`, `is not` and `<>` are all real spellings. What that buys this suite is a case where
/// the operator's first and last tokens sit at different offsets, so the offset the trip reports
/// and the offset the surrounding grammar resumes at can be compared rather than assumed equal.
fn two_token_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, Power>, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::LAngle => match inp.next()? {
        Some(second) if matches!(second.data(), Token::RAngle) => Ok(PrattRHS::Infix(
          Precedenced::new(PrattInfix::Neither("<>"), P_CHAIN),
        )),
        _ => Err(LimErr::Unexpected),
      },
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

/// What every offset cell in this file measures, in the order it must be measured:
///
/// `(the offset the trip reported, the committed frontier the input was handed back at, the start
/// of the very next token the surrounding grammar reads)`.
///
/// The middle field is the contract. `NonAssociativeChain::offset` is *defined* as the position
/// the input was handed back at, so the claim a cell makes is `at == frontier` — and the frontier
/// is **read**, from `InputRef::span().end()`, never assumed. The third field is the observable
/// that kept drifting away from it: three review rounds derived the offset from something near the
/// first token a scan can produce, and trivia, a multi-token operator's gap and a skipped lexer
/// error each pushed that token past the position the caller was actually handed.
///
/// The frontier is read **before** the token, because reading a token moves both.
type Resumption = (usize, usize, Option<usize>);

fn two_token_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Resumption, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  let at = match pratt(lhs, two_token_rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp) {
    Ok(tree) => panic!("a repeated two-token `<>` must be refused; got Ok({tree})"),
    Err(LimErr::NonAssoc { at }) => at,
    Err(other) => panic!("expected the non-associative chain; got {other:?}"),
  };
  let frontier = inp.span().end();
  Ok((at, frontier, inp.next()?.map(|t| t.span().start())))
}

fn prefilled_two_token_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Resumption, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  let _ = inp.peek::<U4>()?;
  two_token_probe(inp)
}

/// The ladder's own [`rhs`] classifier, driven to a trip, reporting a [`Resumption`].
///
/// Used by the plain chain, by the lexer-error fixture — which differs only in its source and its
/// emitter — and by both engines' parity cell.
fn typed_resumption_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Resumption, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  let at = match pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp) {
    Ok(tree) => panic!("a repeated `;` must be refused; got Ok({tree})"),
    Err(LimErr::NonAssoc { at }) => at,
    Err(other) => panic!("expected the non-associative chain; got {other:?}"),
  };
  let frontier = inp.span().end();
  Ok((at, frontier, inp.next()?.map(|t| t.span().start())))
}

fn prefilled_typed_resumption_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Resumption, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  let _ = inp.peek::<U4>()?;
  typed_resumption_probe(inp)
}

/// **The fourth shape, covered rather than argued away.** A `ParsePrattRHS` that opens a
/// [session point](tokora::InputRef::begin_point) and **abandons** it — dropping the handle while
/// the point is still live, which `begin_point`'s contract permits — before reading its operator.
/// The point pins its base, so the probe's restore takes its reconciling path
/// (`abandon_points_above`, then the rewind) rather than the plain one.
///
/// That is the one remaining way for something other than a plain rewind to decide where the input
/// ends up, and therefore the last place the reported offset and the restore target could come
/// apart. `pratt_txn_retention.rs` already pins that this exit hands the operator back across such
/// a point; what this fixture adds is that the offset it *reports* is still the position it landed
/// on.
fn point_abandoning_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, Power>, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  // Opened and never settled: the handle is dropped while the point is still live, which pins its
  // base until an enclosing rollback reconciles it.
  let _ = inp.begin_point();
  rhs(inp)
}

fn point_abandoning_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Resumption, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  let at = match pratt(
    lhs,
    point_abandoning_rhs,
    fold_prefix,
    fold_infix,
    fold_postfix,
  )
  .parse_input(inp)
  {
    Ok(tree) => panic!("a repeated `;` must be refused; got Ok({tree})"),
    Err(LimErr::NonAssoc { at }) => at,
    Err(other) => panic!("expected the non-associative chain; got {other:?}"),
  };
  let frontier = inp.span().end();
  Ok((at, frontier, inp.next()?.map(|t| t.span().start())))
}

thread_local! {
  /// Cycles [`zero_width_repeat_rhs`] has been asked about. Reset by the one cell that uses it.
  static ZERO_WIDTH_CALLS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// Reports the chain's `Neither` operator on **every** call, but consumes a token only on the
/// first — a `ParsePrattRHS` that breaks "consume what you report", which is a grammar bug and
/// not malformed input.
///
/// The first call folds a real `;` and arms the latch. The zero-width report that follows it is
/// therefore both a same-power repeat *and* a stalled report, which is the one input that can
/// tell the two guards' order apart.
fn zero_width_repeat_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, Power>, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  let call = ZERO_WIDTH_CALLS.with(|c| {
    let seen = c.get();
    c.set(seen + 1);
    seen
  });
  if call == 0 {
    return match inp.next()? {
      Some(tok) if matches!(tok.data(), Token::Semi) => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(";"),
        P_CHAIN,
      ))),
      _ => Err(LimErr::Unexpected),
    };
  }
  Ok(PrattRHS::Infix(Precedenced::new(
    PrattInfix::Neither(";"),
    P_CHAIN,
  )))
}

fn zero_width_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Outcome, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
{
  match pratt(
    lhs,
    zero_width_repeat_rhs,
    fold_prefix,
    fold_infix,
    fold_postfix,
  )
  .parse_input(inp)
  {
    Ok(tree) => {
      let front = inp.next()?.map(|t| t.span().start());
      Ok(Ok((tree, front)))
    }
    Err(e) => Ok(Err(e)),
  }
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

fn tok_fold_prefix<'inp, E>(
  _op: Tok,
  operand: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, LimErr> {
  Ok(operand)
}

fn tok_fold_postfix<'inp, E>(
  operand: Tok,
  _op: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, LimErr> {
  Ok(operand)
}

/// Records the fold tree's shape in the value: each fold appends the right operand's digit, so
/// one fold over `1 ; 2` reads `12` and a second over `; 3` reads `123`.
fn tok_fold_infix<'inp, E>(
  left: Tok,
  right: Tok,
  infix: tokora::span::Spanned<PrattInfix<Token, Token, Token>, tokora::SimpleSpan>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
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

/// The token engine's twin of [`typed_resumption_probe`]: it parks the operator instead of rolling
/// a probe back, so this is where the two engines' *definitions* of the handback position are
/// compared against each other rather than each against itself.
fn token_resumption_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Resumption, LimErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = LimErr> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  let at = match inp.pratt::<_, _, _, i64, Power>(
    tok_fold_prefix::<Ctx::Emitter>,
    tok_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
  ) {
    Ok(out) => panic!("a repeated `;` must be refused; got Ok({out:?})"),
    Err(LimErr::NonAssoc { at }) => at,
    Err(other) => panic!("expected the non-associative chain; got {other:?}"),
  };
  let frontier = inp.span().end();
  Ok((at, frontier, inp.next()?.map(|t| t.span().start())))
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

/// The wall-clock bound [`on_a_deep_stack`] enforces — one figure for a compiled build and a
/// different one once the build is interpreted, because those are two different machines.
///
/// Miri does not apply a flat per-step overhead here: `-Zmiri-tree-borrows` revalidates a growing
/// borrow tree on every access, so this file's deepest cell — the `unlimited` half of
/// `the_default_budget_refuses_a_deeper_chain_and_unlimited_restores_it`, a 1000-level
/// `unlimited()` chain — scales worse than linearly with depth, not by the roughly two orders of
/// magnitude a constant per-step slowdown would predict. A 120s native budget is not "a bit tight"
/// under Miri; it is calibrated for a different machine.
///
/// This is not the `frames_are_relocated` situation next door in `pratt_limit_unit_sink.rs`:
/// there, instrumentation makes the witness's own measurement meaningless, so the assertion is
/// skipped and the skip announces itself on real stderr — otherwise a build where the check never
/// ran would look identical to one where it passed. Here the witness — the bound not tripping
/// while the worker is still making progress — still runs, and still means the same thing under
/// Miri; only the number of seconds it is willing to wait has to change. Nothing is skipped, so
/// there is nothing to announce.
///
/// Measured directly with the exact command and flags `ci/miri_tb.sh` runs for the leg issue #148
/// saw this trip on (`-Zmiri-strict-provenance -Zmiri-disable-isolation
/// -Zmiri-symbolic-alignment-check -Zmiri-tree-borrows`), i.e. `cargo +nightly miri test --test
/// pratt_limit the_default_budget_refuses_a_deeper_chain_and_unlimited_restores_it`:
///
/// | run | aarch64-apple-darwin |
/// |---|---|
/// | 1 | 68.56s |
/// | 2 | 61.38s |
///
/// `ci`'s failing leg is `x86_64-unknown-linux-gnu` on a shared runner, a different host and
/// architecture than the one above, and it was already landing intermittently past the *native*
/// 120s bound — over 1.7× the slower reading here — before this fix, so a small multiple would
/// only relocate the same flake. ×10 over the slower reading (685.6s, rounded up to 700) absorbs
/// that cross-host gap and ordinary scheduler noise while staying a bounded timeout: a genuine
/// hang is still caught in about 12 minutes, comfortably inside CI's job-level timeout, instead of
/// being masked indefinitely.
///
/// **Cross-checked once the same defect turned up next door.** `pratt_limit_unit_sink.rs` carried
/// the identical bug — the fix above corrected this bound and did not sweep for siblings — and
/// its cell gives the cross-host factor a *measured* lower edge this one never had: 49.7s on this
/// machine against a trip of a 60s wall on the `x86_64-unknown-linux-gnu` runner, so that runner
/// is at least ×1.21 slower on an interpreted workload of this shape. A deadline that trips
/// reports only "over the budget", never by how much, so ×1.21 is a floor rather than an
/// estimate; ×10 over the reading above leaves the same order of headroom over it either way, and
/// 700 stands. Re-read under Miri with this refactor in place, on a quiet machine: 67.15s and
/// 66.65s, inside the range the table already records. The whole 37-cell file is 90s under Miri,
/// so this one cell is very nearly all of it — the ×10 buys margin on the only cell that needs
/// any.
const DEEP_STACK_BUDGET: common::WallClock = common::WallClock {
  native_secs: 120,
  interpreted_secs: 700,
};

/// Runs `f` on a thread with a stack far larger than the harness's own and a hard wall-clock
/// bound. A recursion-limit defect does not fail an assertion — it hangs, or aborts the process
/// — so the bound has to live outside the code under test. The stack has to be bigger for the
/// **unlimited** cells, which by construction run deeper than the harness's 2 MiB allows.
///
/// The wall itself is [`common::bounded_wait`], the tree's single bounded wait; this wrapper
/// supplies only the stack size and [`DEEP_STACK_BUDGET`].
fn on_a_deep_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
  common::bounded_wait(256 * 1024 * 1024, DEEP_STACK_BUDGET, f)
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

/// The ladder-top case, typed: `1 ; 2 ; 3` folds once and then **fails** on the second `;`, which
/// is handed back unconsumed.
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
    Err(LimErr::NonAssoc { at: 5 }),
    "the input is handed back at 5 — the end of the `2` the expression committed — and that is \
     what the offset names. The second `;` starts at 6, one skipped space further on"
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
    Err(LimErr::NonAssoc { at: 9 }),
    "the input is handed back at 9, the end of the committed `2`; re-associating the second `;` \
     across the `=` is the misparse this pins"
  );
}

/// The token engine, same two inputs, same offsets — and the parity now rests on both engines
/// answering the *same question* rather than on two mechanisms coinciding.
///
/// The two hand back by different means: the typed driver rolls a probe transaction back, the
/// token driver parks the classified token. Neither reports what it did — both report where the
/// input ended up, `InputRef::span().end()`, and that is the same number on the same input because
/// neither engine committed the operator. The cell reads that frontier out of both and asserts the
/// whole triple is equal, so a change to either mechanism that moved one engine's answer reds here
/// naming which column drifted.
///
/// **What the two engines' answers are NOT is the parked/rolled-back operator's own start.** On
/// `1 ; 2 ; 3` that is 6, and both report 5: `TestLexer` carries `skip r"[ \t\r\n]+"`, so the
/// space at 5 is lexed away and belongs to neither the committed expression nor the operator. This
/// is the same over-reach the trivia and lexer-error fixtures below measure at a larger scale, and
/// it is visible even here, on the plainest possible input.
///
/// **Scope, stated rather than assumed.** The engines cannot be compared on a *trivia-surfacing*
/// grammar at all: the token engine's classifier is a pure function of one token and ends the
/// expression on the first trivia token it sees, so there is no such input on which both reach a
/// trip. [`the_token_engine_ends_the_expression_at_the_first_trivia_token`] pins that half, and
/// [`a_trivia_skipping_classifier_is_named_at_the_handback_not_at_the_operator`] pins what the
/// typed engine reports where the token engine cannot follow.
#[test]
fn token_repeat_is_rejected_at_the_same_offsets_as_the_typed_engine() {
  for (src, want) in [
    ("1 ; 2 ; 3", (5usize, 5usize, Some(6usize))),
    ("7 = 1 ; 2 ; 3", (9, 9, Some(10))),
  ] {
    let typed: Resumption = Parser::with_context(fatal_ctx())
      .apply(typed_resumption_probe)
      .parse_str(src)
      .unwrap();
    let token: Resumption = Parser::with_context(fatal_ctx())
      .apply(token_resumption_probe)
      .parse_str(src)
      .unwrap();

    assert_eq!(
      typed, token,
      "the two engines must report the same offset AND land on the same position, on {src}"
    );
    assert_eq!(typed, want, "on {src}");

    let (at, frontier, next) = typed;
    assert_eq!(
      at, frontier,
      "THE CONTRACT: the offset is the position the input was handed back at, on {src}"
    );
    assert_ne!(
      Some(at),
      next,
      "and it is NOT the repeated operator's own start — the skipped space sits between them, on \
       {src}"
    );
  }
}

/// The trigger is the **power**, not the newcomer's associativity: a `Left` operator at the
/// chain's own power trips it too, because the constraint belongs to the chain.
#[test]
fn a_left_operator_at_the_same_power_trips_the_chain() {
  let typed: Outcome = Parser::with_context(fatal_ctx())
    .apply(typed_probe)
    .parse_str("1 ; 2 + 3")
    .unwrap();
  assert_eq!(typed, Err(LimErr::NonAssoc { at: 5 }));

  let token: TokOutcome = Parser::with_context(fatal_ctx())
    .apply(token_probe)
    .parse_str("1 ; 2 + 3")
    .unwrap();
  assert_eq!(token, Err(LimErr::NonAssoc { at: 5 }));
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
    Err(LimErr::NonAssoc { at: 7 }),
    "the postfix left the latch alone, so the `;` after it is still a repeat — handed back at 7, \
     the end of the `/` this expression did commit"
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
  assert_eq!(got, Err(LimErr::NonAssoc { at: 9 }));
}

/// A prefilled lookahead window changes nothing — not the outcome, not the offset, not the
/// position the input is handed back at.
#[test]
fn the_repeat_offset_is_the_same_with_a_prefilled_cache() {
  let empty: Resumption = Parser::with_context(fatal_ctx())
    .apply(typed_resumption_probe)
    .parse_str("7 = 1 ; 2 ; 3")
    .unwrap();
  let prefilled: Resumption = Parser::with_context(fatal_ctx())
    .apply(prefilled_typed_resumption_probe)
    .parse_str("7 = 1 ; 2 ; 3")
    .unwrap();
  assert_eq!(empty, prefilled);
  assert_eq!(prefilled, (9, 9, Some(10)));
  let (at, frontier, _) = prefilled;
  assert_eq!(at, frontier, "THE CONTRACT, under a prefilled cache too");
}

/// **Round 1's shape, re-verified against the settled contract: a multi-token operator.**
///
/// `1 < > 2 < > 3` spells one non-associative operator with two tokens:
///
/// ```text
///   1 < > 2 < > 3
///   0 2 4 6 8 10 12
///        ^ 7: where the handback leaves the input, and what the trip reports
/// ```
///
/// The second `<>` is the repeat: head at **8**, tail at **10**. Three numbers are therefore in
/// play on one input, and the fixture exists because a one-token operator collapses them into
/// one — which is why every cell above it stayed green through a driver that named the wrong one.
/// The tail (10) is what a position read *after* the classifier names, and no caller is ever
/// handed it. The head (8) is what a position read *before* the classifier named, and it is one
/// skipped space past where the input actually is.
///
/// What the trip reports is **7**: the end of the `2` this expression committed, which is where
/// the probe's rollback puts the input back. The cell asserts `at == frontier` — the identity that
/// is the contract — and `at != head`, so an edit that "corrects" the offset to the operator reds
/// with a reason rather than a bare literal mismatch.
///
/// Run twice, because a trip offset that moves with how much has been peeked has bitten this
/// project before: the empty-cache and prefilled-cache runs must be equal, not merely both
/// "reasonable".
#[test]
fn a_multi_token_repeat_is_named_where_the_handback_leaves_the_input() {
  const SRC: &str = "1 < > 2 < > 3";

  let empty: Resumption = Parser::with_context(fatal_ctx())
    .apply(two_token_probe)
    .parse_str(SRC)
    .unwrap();
  let prefilled: Resumption = Parser::with_context(fatal_ctx())
    .apply(prefilled_two_token_probe)
    .parse_str(SRC)
    .unwrap();

  assert_eq!(
    empty, prefilled,
    "neither the trip offset nor the handback may move with how much has been peeked"
  );
  assert_eq!(
    empty,
    (7, 7, Some(8)),
    "the input comes back at 7; the repeated `<>` starts at 8 and ends at 10, and neither is where \
     the caller was left"
  );

  let (at, frontier, head) = empty;
  assert_eq!(
    at, frontier,
    "THE CONTRACT: `NonAssociativeChain::offset` is `InputRef::span().end()` after the handback"
  );
  assert_ne!(
    Some(at),
    head,
    "and it is not the operator's head: naming 8 would skip the byte at 7 that the caller was \
     handed back, and naming the tail (10) would skip the operator's own first token"
  );
}

/// **Round 3's shape: a non-fatal lexer error between the fold and the repeat.**
///
/// This is the fixture that separates "the position the input was handed back at" from "the start
/// of the first token a scan can produce", on a grammar with no trivia tokens and a one-token
/// operator — the two shapes the earlier fixtures used to expose the gap. Nothing above it can:
/// trivia is a *token*, and a skipped lexer error is not.
///
/// ```text
///   1 ; 2 @ ; 3
///   0 2 4 6 8 10
///         ^ 6..7: not a token at all — a lexer error, emitted and stepped over
/// ```
///
/// The scan skips 6..7, emits the error, and produces the `;` at 8. So a driver deriving its
/// offset from that token reports **8** — three bytes past where its own rollback puts the input,
/// and past a region a recoverer resuming at 8 would silently swallow together with the
/// diagnostic that was just rewound off the log. Measured against the pre-fix driver, both cache
/// states reported `(8, 5, Some(8))`: `at` and the frontier disagreed by exactly the error's
/// region plus its surrounding spaces.
///
/// The emitter must be a **recording** one for the shape to exist at all — a `Fatal` emitter turns
/// the lexer error into the parse's error and no repeat is ever reached — and the log is asserted
/// too: the error is rewound with the probe and re-emitted exactly once when the caller reads
/// across it again, which is the dedup watermark travelling in the checkpoint.
#[test]
fn a_skipped_lexer_error_before_the_repeat_does_not_move_the_offset() {
  const SRC: &str = "1 ; 2 @ ; 3";

  let mut empty_emitter = Verbose::<LimErr>::new();
  let empty: Resumption = Parser::with_context((
    &mut empty_emitter,
    tokora::cache::DefaultCache::<TestLexer<'_>>::default(),
  ))
  .apply(typed_resumption_probe)
  .parse_str(SRC)
  .unwrap();

  let mut prefilled_emitter = Verbose::<LimErr>::new();
  let prefilled: Resumption = Parser::with_context((
    &mut prefilled_emitter,
    tokora::cache::DefaultCache::<TestLexer<'_>>::default(),
  ))
  .apply(prefilled_typed_resumption_probe)
  .parse_str(SRC)
  .unwrap();

  assert_eq!(
    empty, prefilled,
    "the prefill lexes across the error before the parse starts, so it also sets the dedup \
     watermark; neither the offset nor the handback may move with it"
  );
  assert_eq!(
    empty,
    (5, 5, Some(8)),
    "the input comes back at 5, the end of the committed `2`; the first token a scan can produce \
     from there is the `;` at 8, and the bytes between them include the lexer error"
  );

  let (at, frontier, next) = empty;
  assert_eq!(
    at, frontier,
    "THE CONTRACT: `NonAssociativeChain::offset` is `InputRef::span().end()` after the handback"
  );
  assert_ne!(
    Some(at),
    next,
    "and it is NOT the first token a scan produces — that token sits past a lexer error the \
     handback returned; reporting 8 would name a position the caller was never left at and would \
     invite a recoverer to skip the error's own region"
  );

  for (label, emitter) in [("empty", &empty_emitter), ("prefilled", &prefilled_emitter)] {
    let log: Vec<LimErr> = emitter.errors().values().flatten().cloned().collect();
    assert_eq!(
      log,
      std::vec![LimErr::Lex],
      "the lexer error is reported exactly once on the {label}-cache run: rewound with the \
       aborted probe, then re-emitted when the caller read across it again"
    );
  }
}

/// **The fourth shape: a classifier that abandons a session point.**
///
/// Every other fixture varies what sits *between* the restore target and the operator. This one
/// varies the restore itself: [`point_abandoning_rhs`] leaves a session point open, so the probe's
/// exit takes `rollback_abandoning_points`' reconciling path — abandon every point above the base,
/// then rewind — rather than a plain restore.
///
/// That is the last remaining way for something other than a straight rewind to decide where the
/// input ends up, and therefore the last place the reported offset could come apart from it. It
/// does not: the reconciliation drops the points and the rewind still installs the probe's own
/// checkpoint, so the frontier is the same number the plain fixture produces, and the offset is
/// still that frontier.
///
/// Beyond it the class is closed by construction rather than by enumeration — the offset is not
/// *derived from* the restore target, it **is** the value the checkpoint was built from, with no
/// statement between the two — which is the claim this round is really making.
#[test]
fn a_classifier_that_abandons_a_session_point_is_still_named_at_the_restore_target() {
  let got: Resumption = Parser::with_context(fatal_ctx())
    .apply(point_abandoning_probe)
    .parse_str("1 ; 2 ; 3")
    .unwrap();

  assert_eq!(
    got,
    (5, 5, Some(6)),
    "the reconciling rollback restores the same thing the plain one does, and reports it"
  );
  let (at, frontier, _) = got;
  assert_eq!(
    at, frontier,
    "THE CONTRACT, across the restore path a still-open session point forces"
  );
}

/// **Round 2's shape, re-verified: a classifier that skips trivia tokens.**
///
/// [`trivia_rhs`] skips whitespace *tokens* before it reads the `;` — the shape a CST-style
/// grammar has, and one `ParsePrattRHS` explicitly permits. On `1 ; 2   ; 3`:
///
/// ```text
///   1 ; 2   ; 3
///   0 2 4   8 10
///        ^^^ the whitespace the second `;`'s classifier would skip: 5..8
/// ```
///
/// The driver decides the repeat *before* the classifier runs — it has to, because running the
/// classifier is what the transaction rules forbid at that point — so it cannot know that 5..8
/// would be skipped, and reports **5**, the position its probe restores to. This grammar surfaces
/// whitespace as a token, so 5 is also where the next token starts, and the operator's own head is
/// at 8, one trivia skip further on.
///
/// This is the fixture on which the three columns pull furthest apart, and the one that makes the
/// contract's wording load bearing: `at == frontier` holds, `at == the next token's start` also
/// holds *here* but is the coincidence the lexer-error fixture below breaks, and `at != the
/// operator's head` is the claim a future "fix" would violate.
///
/// Both cache states, for the reason the multi-token cell runs both: an offset that moves with
/// how much has been peeked has bitten this project before.
#[test]
fn a_trivia_skipping_classifier_is_named_at_the_handback_not_at_the_operator() {
  const SRC: &str = "1 ; 2   ; 3";

  let empty: TriviaTrip = Parser::with_context(trivia_ctx())
    .apply(trivia_probe)
    .parse_str(SRC)
    .unwrap();
  let prefilled: TriviaTrip = Parser::with_context(trivia_ctx())
    .apply(prefilled_trivia_probe)
    .parse_str(SRC)
    .unwrap();

  assert_eq!(
    empty, prefilled,
    "neither the trip offset nor the handback may move with how much has been peeked"
  );
  assert_eq!(
    empty,
    (5, 5, Some(5), Some(8)),
    "the trip names 5 — the whitespace the handback returns — while the repeated `;` starts at 8"
  );

  let (at, frontier, next, operator_head) = empty;
  assert_eq!(
    at, frontier,
    "THE CONTRACT: `NonAssociativeChain::offset` is `InputRef::span().end()` after the handback"
  );
  assert_eq!(
    Some(at),
    next,
    "and on THIS grammar the next token starts there too, because the whitespace is a token; that \
     coincidence is what made the weaker definition look right for two rounds"
  );
  assert_ne!(
    Some(at),
    operator_head,
    "but it is NOT the operator's head — a driver that reported 8 here would be naming a byte the \
     caller was never handed back to"
  );
}

/// The **token** engine cannot express the grammar above at all, which is what scopes the
/// two-engine offset parity to trivia-less grammars.
///
/// `PrattToken::try_pratt_rhs` is a pure function of one token: there is no `InputRef` to skip
/// with. So the whitespace at 1..2 answers `None`, the expression ends there, and the chain's
/// second `;` is never reached — the parse returns the bare operand `1` with the whitespace left
/// at the front. No `NonAssociativeChain`, because no repeat was ever seen.
///
/// That is why [`token_repeat_is_rejected_at_the_same_offsets_as_the_typed_engine`] does not need
/// a trivia twin and could not have one: the two engines never report *different* offsets for one
/// trip, because there is no trivia-surfacing input on which the token engine reaches a trip at
/// all. Where it does reach one, its handback position and the operator's head are the same byte
/// by construction — acceptance is the commit, and the parked token is the operator.
#[test]
fn the_token_engine_ends_the_expression_at_the_first_trivia_token() {
  let got: TokOutcome = Parser::with_context(trivia_ctx())
    .apply(trivia_token_probe)
    .parse_str("1 ; 2   ; 3")
    .unwrap();
  assert_eq!(
    got,
    Ok((1, Some(1))),
    "the operand alone, and the whitespace at 1 left on the input: a token-level pratt grammar is \
     a trivia-less grammar by construction"
  );
}

/// **A contract violation outranks a diagnosis of the input.** A `ParsePrattRHS` that re-reports
/// the chain's own operator having consumed *nothing* is a grammar bug, and the driver says so
/// terminally instead of describing it as a non-associative chain.
///
/// Both conditions hold at once on `1 ; 2` with [`zero_width_repeat_rhs`]: the first call folds a
/// real `;` and arms the latch, and the second reports the same operator at the same power from a
/// zero-width read. The floor admits it, so exactly two guards can claim it — the report boundary
/// and the repeat — and only their **order** decides which.
///
/// The repeat is non-terminal and spendable by a recoverer, so ordering it first hands a broken
/// parser's bug to recovery as if it were the user's bad input; recovery then spends it and
/// re-enters a cycle that reports exactly the same thing. The stall is terminal and names the
/// channel that broke its contract. A parser that cannot advance cannot diagnose what it is
/// reading, so the stall wins.
///
/// Both profiles discriminate, and on different observables — the same shape
/// `pratt_txn_retention.rs`'s stall cells use. **Debug**: the wrapper's "consumed nothing"
/// assertion must fire, which the previous ordering never reached. **Release**: there is no
/// assertion, so the returned error must be the terminal end-of-RHS at the position the report
/// failed to advance past. Before the reorder this returned a non-terminal `NonAssoc` in both,
/// describing the grammar's bug as the user's bad input.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "consumed nothing"))]
fn a_zero_width_same_power_report_after_a_neither_fold_is_the_stall_not_the_chain() {
  ZERO_WIDTH_CALLS.with(|c| c.set(0));
  let got: Outcome = Parser::with_context(fatal_ctx())
    .apply(zero_width_probe)
    .parse_str("1 ; 2")
    .unwrap();
  assert_eq!(
    got,
    Err(LimErr::EoRhs {
      at: 5,
      terminal: true
    }),
    "release: the zero-width repeat is the RHS channel's contract violation — terminal, at the \
     committed frontier the report did not advance past — not a `NonAssociativeChain` a recoverer \
     may spend"
  );
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
      inp.emit_error(tokora::span::Spanned::new(span, LimErr::Note(at)))?;
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
    Err(LimErr::NonAssoc { at: 5 }),
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
      LimErr::NonAssoc { at: 5 },
      "the recoverer is handed the repeat itself, naming the position it may resume from"
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
// R2 — WHERE EACH RECOVERY PATH RESUMES, read off the input rather than assumed
// ═══════════════════════════════════════════════════════════════════════════════
//
// Every `Resumption` cell above pins the offset against the position the **handback** left the
// input at. That is a fact about the error's own exit, and it is not a statement about where a
// recovery combinator restarts — for two of the three paths those are different numbers:
//
//   * `Recover` and `skip_then_retry` speculate through `try_attempt`, whose failure path restores
//     the pre-attempt checkpoint *before* `recover_input` runs / before the skip loop's first sync.
//     Both therefore resume at their own attempt origin, which is at or before the offset.
//   * `InplaceRecover` never backtracks, and is the one combinator standing exactly where the
//     offset names.
//
// `an_explicit_recovery_may_spend_the_repeat` above asserts the payload the recoverer is handed and
// nothing about the input underneath it — which is exactly how "hand the error to `Recover` and the
// offset it reads is the offset the retry begins at" survived twelve review rounds. These three
// cells read the position.

thread_local! {
  /// What a recovery handler observed: the error's offset, `span().end()`, and `cursor()`.
  static RECOVERY_SITE: core::cell::Cell<Option<(usize, usize, usize)>> =
    const { core::cell::Cell::new(None) };
  /// Every offset `skip_then_retry`'s sync predicate was offered a token at, in source order.
  static SYNC_CANDIDATES: core::cell::RefCell<std::vec::Vec<usize>> =
    const { core::cell::RefCell::new(std::vec::Vec::new()) };
}

/// **`Recover` hands its handler the input at its own attempt origin, not at the reported offset.**
///
/// On `1 ; 2 ; 3` the error says 5 and the handler is standing on 0: `Recover` speculates through
/// [`try_attempt`], whose failure path restores the pre-parse checkpoint before `recover_input` is
/// called. Both numbers are right, and they answer different questions — this cell exists so the
/// contract cannot fuse them again.
///
/// What makes it red: `Recover` ceasing to roll back (the handler would read 5, and the two
/// `assert_ne!`-shaped claims below would both fail); the handback offset moving (the `at == 5`
/// pin, shared with `typed_repeat_at_the_top_level_is_rejected_and_hands_the_operator_back`); the
/// recoverer being handed anything other than the whole expression (the post-conditions: frontier
/// 0, next token 0).
#[test]
fn a_rollback_recovery_resumes_at_its_attempt_origin_not_at_the_offset() {
  fn recovery<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
    err: LimErr,
  ) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let LimErr::NonAssoc { at } = err else {
      panic!("expected the non-associative chain; got {err:?}");
    };
    // Read before anything else: a peek or a consume would move both.
    RECOVERY_SITE.with(|c| c.set(Some((at, inp.span().end(), *inp.cursor().as_inner()))));
    Ok(String::from("<recovered>"))
  }

  /// `(the recovered value, the frontier after the combinator, the next token's start)`.
  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(String, usize, Option<usize>), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(recovery)
      .parse_input(inp)?;
    let frontier = inp.span().end();
    Ok((out, frontier, inp.next()?.map(|t| t.span().start())))
  }

  RECOVERY_SITE.with(|c| c.set(None));
  let (got, frontier, next) = Parser::with_context(fatal_ctx())
    .apply(probe)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(got, "<recovered>");

  let (at, observed, cursor) = RECOVERY_SITE.with(|c| c.get()).expect("the recoverer ran");

  assert_eq!(at, 5, "the error still carries the handback position");
  assert_eq!(
    observed, 0,
    "`Recover` restores the pre-attempt checkpoint before the handler runs, so the handler is \
     standing on the position the WRAPPED PARSER started at — 0 here — not on the offset"
  );
  assert_eq!(
    cursor, 0,
    "the lookahead cursor is rolled back with it: nothing of the expression is still in hand"
  );
  assert!(
    observed < at,
    "the two are different numbers, and the rollback target is the earlier one: observed \
     {observed}, offset {at}"
  );
  assert_eq!(
    (frontier, next),
    (0, Some(0)),
    "and the whole expression is back in front of the caller — a recoverer that resumed at the \
     offset would find `; 3` here, not `1 ; 2 ; 3`"
  );
}

/// **`InplaceRecover` is the path that does resume at the offset**, and the `Cursor` it is *also*
/// handed is the other number — where the primary parser started.
///
/// This is the control for the cell above: same fixture, same error, no rollback. `span().end()`
/// in the handler **is** the offset, which is r15's checkable identity observed through a
/// combinator rather than by catching the `Err` in grammar code.
///
/// What makes it red: `InplaceRecover` gaining a rollback (the handler would read 0 and
/// `observed == at` would fail); the handback moving away from `span().end()`; the `Cursor`
/// argument ceasing to name the primary's start.
#[test]
fn an_in_place_recovery_resumes_at_the_offset_itself() {
  fn recovery<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
    cursor: tokora::input::Cursor<'inp, '_, TestLexer<'inp>>,
    err: LimErr,
  ) -> Result<String, LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let LimErr::NonAssoc { at } = err else {
      panic!("expected the non-associative chain; got {err:?}");
    };
    RECOVERY_SITE.with(|c| c.set(Some((at, inp.span().end(), *cursor.as_inner()))));
    Ok(String::from("<recovered>"))
  }

  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(String, usize, Option<usize>), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .inplace_recover(recovery)
      .parse_input(inp)?;
    let frontier = inp.span().end();
    Ok((out, frontier, inp.next()?.map(|t| t.span().start())))
  }

  RECOVERY_SITE.with(|c| c.set(None));
  let (got, frontier, next) = Parser::with_context(fatal_ctx())
    .apply(probe)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(got, "<recovered>");

  let (at, observed, cursor) = RECOVERY_SITE.with(|c| c.get()).expect("the recoverer ran");

  assert_eq!(at, 5, "the same handback position the rollback cell reads");
  assert_eq!(
    observed, at,
    "the in-place path does not backtrack, so `span().end()` in the handler IS the offset — the \
     one recovery path on which the two are the same number"
  );
  assert_eq!(
    cursor, 0,
    "and the `Cursor` it is handed is the OTHER number: where the primary parser started"
  );
  assert!(
    cursor < at,
    "the two arguments name different positions, which is the whole point of passing both: \
     cursor {cursor}, offset {at}"
  );
  assert_eq!(
    (frontier, next),
    (5, Some(6)),
    "the input is untouched by the handler, so the repeated `;` at 6 is still the next token"
  );
}

/// **`skip_then_retry` starts its skip loop at the attempt origin, so it scans from *behind* the
/// repeat.**
///
/// Its first attempt is a `try_attempt` too, so the rollback lands the loop on 0 and
/// `sync_balanced` scans from there. On `1 ; 2 ; 3` the first token its sync predicate is offered
/// is the `1` at 0 — three bytes behind the reported offset and four behind the repeated `;` at 6
/// — and its first skipped region is `0..1`, a stretch the parse had already accepted.
///
/// The offset is re-measured in this same cell by a bare run of the same parser over the same
/// source, so the comparison is against a read number and not against a literal.
///
/// What makes it red: the first attempt ceasing to roll back (the first candidate would be the
/// `;` at 6 and both ordering claims would fail); the sync scan starting anywhere other than the
/// attempt origin; the skipped regions changing shape.
#[test]
fn skip_then_retry_scans_from_the_attempt_origin_not_from_the_offset() {
  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Result<String, LimErr>, usize), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .skip_then_retry(
        |_: &common::TokenKind| tokora::input::Balance::<char>::Neutral,
        |tok: tokora::span::Spanned<&Token, &tokora::SimpleSpan>| {
          SYNC_CANDIDATES.with(|c| c.borrow_mut().push(tok.span().start()));
          matches!(tok.data(), Token::Semi)
        },
      )
      .parse_input(inp);
    let frontier = inp.span().end();
    Ok((out, frontier))
  }

  // The offset, read rather than assumed, from a bare run of the same parser over the same source.
  let (at, handback, _) = {
    let got: Resumption = Parser::with_context(fatal_ctx())
      .apply(typed_resumption_probe)
      .parse_str("1 ; 2 ; 3")
      .unwrap();
    got
  };
  assert_eq!(
    (at, handback),
    (5, 5),
    "the handback identity the retry is being compared against"
  );

  SYNC_CANDIDATES.with(|c| c.borrow_mut().clear());
  let mut emitter = Verbose::<LimErr>::new();
  let (out, frontier) = Parser::with_context((
    &mut emitter,
    tokora::cache::DefaultCache::<TestLexer<'_>>::default(),
  ))
  .apply(probe)
  .parse_str("1 ; 2 ; 3")
  .unwrap();

  let candidates = SYNC_CANDIDATES.with(|c| c.borrow().clone());
  let first = *candidates.first().expect("the skip loop ran a sync");
  assert_eq!(
    first, 0,
    "the first attempt is rolled back before the loop starts, so the scan begins at the attempt \
     origin — the `1` at 0 — and not at the offset the error reported"
  );
  assert!(
    first < at,
    "and it begins strictly BEHIND the reported offset: first candidate {first}, offset {at}"
  );
  assert_eq!(
    candidates,
    std::vec![0, 2, 4, 6, 8],
    "every cycle's scan, in source order: cycle 1 syncs on the FIRST `;` at 2 — the one the \
     expression had already folded — cycle 2 on the second at 6, and cycle 3 runs out of input"
  );

  assert_eq!(
    emitter
      .skipped_regions()
      .get(&tokora::SimpleSpan::new(0, 1)),
    Some(&std::vec![1usize]),
    "cycle 1's hole covers the `1` at 0..1, a region entirely before the reported offset and one \
     the parse had already accepted — a recovery that resumed at the offset could not have \
     skipped it"
  );
  let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
  assert_eq!(holes, 2, "one per cycle that skipped: 0..1 and 4..5");

  assert_eq!(
    out,
    Err(LimErr::Unexpected),
    "and no retry succeeds: every sync point lands the retry on a `;`, which is not an operand"
  );
  assert_eq!(
    frontier, 7,
    "the loop leaves the input past the second `;` — input the offset's own contract would have \
     preserved"
  );
}

/// **What a caller *reads* names the handback, not the repeated operator.**
///
/// Every other R2 cell asserts `offset()` off the converted payload, and `offset()` was right the
/// whole time — so nothing here noticed that `Display` rendered `non-associative operator at 5
/// cannot be chained at its own power` on the very fixture whose repeated `;` starts at **6**. The
/// payload was correct and the sentence around it was not, and a caller that logs, wraps or
/// snapshots the error only ever has the sentence.
///
/// So this cell reads `Display` on the value the **engine** built — captured in
/// [`RENDERED_CHAIN`] at the `From` boundary, because `LimErr` keeps the offset rather than the
/// payload — over a real `1 ; 2 ; 3` trip, and checks it against both measured numbers rather than
/// against literals.
///
/// The exact bytes are frozen separately, in `tests/render_freeze.rs`, on a hand-built value. That
/// suite says *this string moved and must be disclosed*; this cell says *the string agrees with
/// where the parse actually left the input*, which is the claim the old wording broke.
///
/// What makes it red: the render presenting the offset as the operator's own position again
/// (either negative pin); the render dropping the offset (the positive pin); the handback or the
/// operator's start moving (the two measured pins, shared with
/// [`typed_repeat_at_the_top_level_is_rejected_and_hands_the_operator_back`]).
#[test]
fn the_render_names_the_handback_and_not_the_operator() {
  RENDERED_CHAIN.with(|c| *c.borrow_mut() = None);

  let (at, handback, operator): Resumption = Parser::with_context(fatal_ctx())
    .apply(typed_resumption_probe)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(
    (at, handback),
    (5, 5),
    "the handback identity the render is being measured against"
  );
  let operator = operator.expect("the repeated `;` is still on the input");
  assert_eq!(
    operator, 6,
    "and the repeated operator's own start — the position the render must NOT be naming"
  );
  assert!(at < operator, "the two are different bytes on this fixture");

  let rendered = RENDERED_CHAIN
    .with(|c| c.borrow().clone())
    .expect("the engine built a `NonAssociativeChain`, and the conversion rendered it");

  assert!(
    rendered.contains(&std::format!("handed back at {at}")),
    "the render must name {at} for what it is — the position the input was handed back at; got \
     {rendered:?}"
  );
  assert!(
    !rendered.contains(&std::format!("operator at {at}")),
    "and must never present it as the operator's location: the repeated operator starts at \
     {operator}, so `operator at {at}` points at a byte it does not occupy; got {rendered:?}"
  );
  assert!(
    !rendered.starts_with("non-associative operator at "),
    "and not by re-adopting the opening this cell was written against; got {rendered:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// R2 under a TRIVIA-SURFACING grammar
// ═══════════════════════════════════════════════════════════════════════════════
//
// Every fixture above is lexed by `common::TestLexer`, whose logos declaration carries
// `skip r"[ \t\r\n]+"`: whitespace never becomes a token, so "the position the handback leaves
// the input at" and "the offending operator's first token" cannot differ there. This section
// supplies the grammar shape where they can — a lexer that emits whitespace as a token, and an
// RHS classifier that skips it before consuming the operator, which is what a CST-style tokora
// grammar looks like and what `ParsePrattRHS` explicitly permits.

/// A token vocabulary that **surfaces trivia**: whitespace is a token, not a lexer-level skip.
#[derive(Debug, Clone, Logos, PartialEq)]
#[logos(crate = logos)]
enum TriviaToken {
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().unwrap_or(0))]
  Num(i64),
  #[token(";")]
  Semi,
  #[regex(r"[ \t\r\n]+")]
  Ws,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TriviaKind {
  Num,
  Semi,
  Ws,
}

impl core::fmt::Display for TriviaKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TriviaKind::Num => write!(f, "number"),
      TriviaKind::Semi => write!(f, ";"),
      TriviaKind::Ws => write!(f, "whitespace"),
    }
  }
}

impl core::fmt::Display for TriviaToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TriviaToken::Num(n) => write!(f, "{n}"),
      TriviaToken::Semi => write!(f, ";"),
      TriviaToken::Ws => write!(f, "whitespace"),
    }
  }
}

impl From<&TriviaToken> for TriviaKind {
  fn from(t: &TriviaToken) -> Self {
    match t {
      TriviaToken::Num(_) => TriviaKind::Num,
      TriviaToken::Semi => TriviaKind::Semi,
      TriviaToken::Ws => TriviaKind::Ws,
    }
  }
}

impl TokenT<'_> for TriviaToken {
  type Kind = TriviaKind;
  type Error = ();

  fn kind(&self) -> TriviaKind {
    TriviaKind::from(self)
  }

  fn is_trivia(&self) -> bool {
    matches!(self, TriviaToken::Ws)
  }
}

type TriviaLexer<'a> = tokora::lexer::LogosLexer<'a, TriviaToken>;

/// Spelled with concrete parameters rather than `UnexpectedTokenOf<'inp, TriviaLexer<'inp>>`:
/// coherence does not normalize the alias's `<L as Lexer>::Token` projection when it checks this
/// against the `TestLexer` impl above, and reads the two as one blanket impl over
/// `UnexpectedToken<'_, _, _, _>`. The expanded form is the same type and the two are disjoint.
impl<'inp> From<UnexpectedToken<'inp, TriviaToken, TriviaKind>> for LimErr {
  fn from(_: UnexpectedToken<'inp, TriviaToken, TriviaKind>) -> Self {
    LimErr::Unexpected
  }
}

/// Skips leading trivia, then reads one operand — the ordinary shape for a trivia-surfacing
/// grammar's LHS channel.
fn trivia_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, Power>, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TriviaLexer<'inp>, Error = LimErr>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      TriviaToken::Num(n) => Ok(PrattLHS::Operand(n.to_string())),
      _ => Err(LimErr::Unexpected),
    },
    None => Err(LimErr::Eot),
  }
}

/// **Skips trivia before it reads the operator.** This is the classifier shape the finding is
/// about: the operator's first token is not the first token the probe sees, so a position read
/// ahead of this function names the whitespace rather than the `;`.
fn trivia_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, Power>, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TriviaLexer<'inp>, Error = LimErr>,
{
  inp.skip_while(|t| t.is_trivia())?;
  Ok(match inp.next()? {
    Some(tok) => match tok.into_data() {
      TriviaToken::Semi => PrattRHS::Infix(Precedenced::new(PrattInfix::Neither(";"), P_CHAIN)),
      _ => PrattRHS::End,
    },
    None => PrattRHS::End,
  })
}

fn trivia_fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, Power>,
) -> Result<String, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TriviaLexer<'inp>, Error = LimErr>,
{
  Ok(format!("({}{operand})", op.into_data()))
}

fn trivia_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
  left: String,
  right: String,
  op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, Power>,
) -> Result<String, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TriviaLexer<'inp>, Error = LimErr>,
{
  let (PrattInfix::Left(s) | PrattInfix::Right(s) | PrattInfix::Neither(s)) = op.into_data();
  Ok(format!("({left}{s}{right})"))
}

fn trivia_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, Power>,
) -> Result<String, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TriviaLexer<'inp>, Error = LimErr>,
{
  Ok(format!("({operand}{})", op.into_data()))
}

/// [`Resumption`] plus one column this grammar alone can supply: `(at, frontier, next token —
/// trivia included, the start of the next **non-trivia** token after it)`. The last is the
/// operator's own head, reached the way this grammar's own classifier would reach it.
type TriviaTrip = (usize, usize, Option<usize>, Option<usize>);

fn trivia_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
) -> Result<TriviaTrip, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TriviaLexer<'inp>, Error = LimErr>,
{
  let at = match pratt(
    trivia_lhs,
    trivia_rhs,
    trivia_fold_prefix,
    trivia_fold_infix,
    trivia_fold_postfix,
  )
  .parse_input(inp)
  {
    Ok(tree) => panic!("a repeated `;` must be refused; got Ok({tree})"),
    Err(LimErr::NonAssoc { at }) => at,
    Err(other) => panic!("expected the non-associative chain; got {other:?}"),
  };
  // The position the input was handed back at — read, not assumed.
  let frontier = inp.span().end();
  // The first token a caller reading the input verbatim receives.
  let handback = inp.next()?.map(|t| t.span().start());
  // The operator's own head, reached the way this grammar's own classifier would reach it.
  inp.skip_while(|t| t.is_trivia())?;
  let operator_head = inp.next()?.map(|t| t.span().start());
  Ok((at, frontier, handback, operator_head))
}

fn prefilled_trivia_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
) -> Result<TriviaTrip, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TriviaLexer<'inp>, Error = LimErr>,
{
  let _ = inp.peek::<U4>()?;
  trivia_probe(inp)
}

impl PrattToken<'_, i64, Power> for TriviaToken {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    match self {
      TriviaToken::Num(_) => Some(PrattLHS::Operand(())),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>> {
    match self {
      TriviaToken::Semi => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        P_CHAIN,
      ))),
      _ => None,
    }
  }
}

type TriviaTok = tokora::span::Spanned<TriviaToken, tokora::SimpleSpan>;

fn trivia_tok_fold_prefix<'inp, E>(
  _op: TriviaTok,
  operand: TriviaTok,
  _: EmitterView<'_, 'inp, TriviaLexer<'inp>, E>,
) -> Result<TriviaTok, LimErr> {
  Ok(operand)
}

fn trivia_tok_fold_postfix<'inp, E>(
  operand: TriviaTok,
  _op: TriviaTok,
  _: EmitterView<'_, 'inp, TriviaLexer<'inp>, E>,
) -> Result<TriviaTok, LimErr> {
  Ok(operand)
}

fn trivia_tok_fold_infix<'inp, E>(
  left: TriviaTok,
  right: TriviaTok,
  infix: tokora::span::Spanned<
    PrattInfix<TriviaToken, TriviaToken, TriviaToken>,
    tokora::SimpleSpan,
  >,
  _: EmitterView<'_, 'inp, TriviaLexer<'inp>, E>,
) -> Result<TriviaTok, LimErr> {
  let value = match (left.data(), right.data()) {
    (TriviaToken::Num(l), TriviaToken::Num(r)) => l * 10 + r,
    _ => return Err(LimErr::Unexpected),
  };
  Ok(tokora::span::Spanned::new(
    infix.into_span(),
    TriviaToken::Num(value),
  ))
}

/// The token engine over the same trivia-surfacing source: value plus handback, or its error.
fn trivia_token_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TriviaLexer<'inp>, Ctx>,
) -> Result<TokOutcome, LimErr>
where
  Ctx: ParseContext<'inp, TriviaLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TriviaLexer<'inp>, Error = LimErr> + PrattEmitter<'inp, TriviaLexer<'inp>>,
{
  match inp.pratt::<_, _, _, i64, Power>(
    trivia_tok_fold_prefix::<Ctx::Emitter>,
    trivia_tok_fold_infix::<Ctx::Emitter>,
    trivia_tok_fold_postfix::<Ctx::Emitter>,
  ) {
    Ok(out) => {
      let value = match out.expect("the input opens with an operand").into_data() {
        TriviaToken::Num(n) => n,
        _ => return Err(LimErr::Unexpected),
      };
      let front = inp.next()?.map(|t| t.span().start());
      Ok(Ok((value, front)))
    }
    Err(e) => Ok(Err(e)),
  }
}

fn trivia_ctx<'inp>() -> ParserContext<'inp, TriviaLexer<'inp>, Fatal<LimErr>> {
  ParserContext::new(Fatal::new())
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

/// A recoverer **may not** spend a trip: `Recover` consults `MaybeTerminal` *and* the input's
/// resource-trip latch and re-raises, so the recovery body never runs. The contrast with
/// `an_explicit_recovery_may_spend_the_repeat` is the whole point of the two classifications.
///
/// Paired with `pratt_limit_unit_sink.rs::a_discarding_sink_cannot_let_recovery_spend_a_trip`,
/// which runs this ladder, this limit and this input through `()`. The two must keep agreeing:
/// that agreement is the invariance issue #148 asked for, and until it landed they disagreed —
/// this cell re-raised and its `()` twin spent the trip.
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
///
/// Paired with
/// `pratt_limit_unit_sink.rs::a_discarding_sink_cannot_let_skip_then_retry_burn_input_on_a_trip`,
/// which runs this exact source through `()` and asserts the same `Some(0)` handback. That cell
/// read `Some(68)` before issue #148 — the whole chain and the sync token skipped and committed —
/// so the two files' agreement on this number is the observable the fix delivers.
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
    inp.emit_error(tokora::span::Spanned::new(span, LimErr::Note(at)))?;
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

/// **Protection is on by default.** An unconfigured `parse_str` over a chain deeper than the
/// default fails terminally instead of risking a native-stack abort — and `unlimited()` puts the
/// deep parse back, which proves the default is the thing doing the refusing.
///
/// The chain is deeper than the default but still well inside every measured native ceiling, so
/// what refuses it can only be the limiter. `on_a_deep_stack` stays on both halves anyway: the
/// `unlimited()` half runs 1000 levels deep, which no 2 MiB stack survives in a debug build.
///
/// **Every figure is read off `RecursionLimiter::PARSE_DEFAULT_DEPTH`, never written out.** What
/// this cell is about is the *relationship* — the frame after the budget is the one refused — and
/// a literal would state the budget a second time in a file that does not own it. The number
/// itself is written out once, in `src/state/recursion_tracker/tests.rs`, so that the suite does
/// not derive every one of its expectations from the subject.
#[test]
fn the_default_budget_refuses_a_deeper_chain_and_unlimited_restores_it() {
  let default_depth = RecursionLimiter::PARSE_DEFAULT_DEPTH;
  let past_it = default_depth + 16;

  let defaulted = on_a_deep_stack(move || {
    let src = prefix_chain(past_it);
    Parser::new()
      .apply(typed_probe)
      .parse_str(&src)
      .map_err(|e: LimErr| e)
      .unwrap()
  });
  // `at` is left out on purpose: it is committed consumption at the tripping frame's entry, which
  // moves with the default, and it is what the R1 offset cells above already pin.
  assert!(
    matches!(
      defaulted,
      Err(LimErr::Limit { depth, limitation, .. })
        if depth == default_depth + 1 && limitation == default_depth
    ),
    "the default budget is {default_depth} and the frame after it is refused; got {defaulted:?}"
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
  let token_defaulted = on_a_deep_stack(move || {
    let src = prefix_chain(past_it);
    Parser::new()
      .apply(token_probe)
      .parse_str(&src)
      .map_err(|e: LimErr| e)
      .unwrap()
  });
  assert!(
    matches!(
      token_defaulted,
      Err(LimErr::Limit { depth, limitation, .. })
        if depth == default_depth + 1 && limitation == default_depth
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
/// The limit is 32, half the default 64, on purpose — and the margin is chosen from a
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
/// The same table is why the shipped default stopped being the 500 this branch first carried.
/// 500 was sized against the *release* ceilings on the top row and cleared them by ~7.7×, but the
/// bottom row is the one an unconfigured parse meets in a test suite, and 500 is four times the
/// debug token engine's 125: the stack aborted before the limiter could return anything.
///
/// **The table above is tokora's own frames, and the shipped default is no longer derived from
/// it.** Its replacement, 64, cleared the tightest of these four by ~1.9× and was still too high,
/// because a consumer's grammar runs *inside* this recursion and pays for both frames: one
/// measured consumer aborts at **51**, below 64. `RecursionLimiter::PARSE_DEFAULT_DEPTH` carries
/// that derivation and is 16, in every build. This cell's own 32 is
/// unaffected — it is a *configured* budget chosen against the 125 in this table, which is the
/// right table for a cell that runs tokora's grammar and nobody else's.
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

// ═══════════════════════════════════════════════════════════════════════════════
// R1 — what the `stacker` feature moves, and what it does not
// ═══════════════════════════════════════════════════════════════════════════════

/// The thread every figure in `src/native_stack.rs`'s measurement table is stated on, and the one
/// `std::thread::spawn` hands out by default.
///
/// Written out rather than inherited from the harness: libtest's worker threads are 2 MiB today,
/// but that is its choice and not a contract, and a table stated in "the default thread size"
/// would silently re-base itself the day that changed.
const MEASUREMENT_STACK: usize = 2 * 1024 * 1024;

/// A chain past every native ceiling in the table: 8× the 125 the binding cell records for a 2 MiB
/// debug thread, 2.6× the 384 the typed driver holds there, and still two orders of magnitude
/// inside what a segmented stack costs to reach.
const FAR_PAST_THE_CEILING: usize = 1_000;

/// Runs `f` on an explicitly sized **2 MiB** thread — the measurement stack, not an enlarged one.
///
/// The opposite of [`on_a_deep_stack`] and used for the opposite reason: these cells are about
/// what happens when the stack is the ordinary one, so supplying a bigger one would make them pass
/// for a build in which nothing under test worked.
fn on_a_2mib_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
  common::bounded_wait(MEASUREMENT_STACK, DEEP_STACK_BUDGET, f)
}

/// **The claim that holds in both configurations, and the reason the budget is not optional.**
///
/// On a 2 MiB thread this chain is past every cell of the measurement table — without `stacker` by
/// 8× on the binding one — so the only reason this call returns at all is that the budget refused
/// it before the stack could. Deliberately **not** `on_a_deep_stack`: an enlarged stack is exactly
/// what must not be supplied, or the cell would pass for a build in which the limiter did nothing.
///
/// The depth is the **larger** of the two things it has to exceed, and it has to exceed both or it
/// tests nothing. Past the native ceiling, or an abort is not what it would have hit; past the
/// configured default, or nothing refuses it. The `max` is kept even though the shared default is
/// now 16 in every build: it costs nothing, and it is what stopped this cell going vacuous when
/// the default was briefly 1024 under `stacker` — which is exactly the failure it would have to
/// survive again if a future feature moved the number.
#[test]
fn a_chain_far_past_the_native_ceiling_comes_back_as_a_value_not_an_abort() {
  let depth = FAR_PAST_THE_CEILING.max(RecursionLimiter::PARSE_DEFAULT_DEPTH + 16);
  let got: Outcome = on_a_2mib_stack(move || {
    let src = prefix_chain(depth);
    Parser::new()
      .apply(typed_probe)
      .parse_str(&src)
      .map_err(|e: LimErr| e)
      .unwrap()
  });
  assert!(
    matches!(
      got,
      Err(LimErr::Limit { limitation, .. })
        if limitation == RecursionLimiter::PARSE_DEFAULT_DEPTH
    ),
    "the default budget must refuse a {depth}-deep chain on a 2 MiB thread, as a value; got {got:?}"
  );
}

/// **What the feature buys, and — in the same cell — what it does not.**
///
/// The budget is raised out of the way, the thread is the same ordinary 2 MiB, and the chain is 8×
/// the 125 that thread holds without segments. A build in which `native_stack::maybe_grow` were
/// the identity would not fail this line, it would *die* on it.
///
/// Then the second half, which is the one that must never be deleted: a budget one frame lower
/// still refuses, as an ordinary catchable value, at a depth the thread stack could never have
/// reached. **That pair is the whole claim** — the ceiling moved from hardware to policy, and the
/// thing enforcing the policy is still the limiter.
///
/// The limiter is `with_limitation` and never `unlimited()` here, deliberately. `unlimited()` at
/// this depth is the negative property, and the negative property cannot be a test: its outcome
/// is a dead process. It is measured by hand and recorded in `src/native_stack.rs`.
#[cfg(feature = "stacker")]
#[test]
fn with_stacker_the_same_chain_parses_past_the_thread_it_runs_on() {
  let carried: Outcome = on_a_2mib_stack(|| {
    let src = prefix_chain(FAR_PAST_THE_CEILING);
    Parser::with_context(limited(FAR_PAST_THE_CEILING + 1))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  let (tree, front) = carried.expect("segments must carry a chain the thread stack cannot");
  assert_eq!(front, None, "the whole chain was consumed");
  assert_eq!(
    tree.matches('-').count(),
    FAR_PAST_THE_CEILING,
    "every prefix operator folded"
  );

  let refused: Outcome = on_a_2mib_stack(|| {
    let src = prefix_chain(FAR_PAST_THE_CEILING);
    Parser::with_context(limited(FAR_PAST_THE_CEILING - 1))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  assert!(
    matches!(
      refused,
      Err(LimErr::Limit {
        limitation, ..
      }) if limitation == FAR_PAST_THE_CEILING - 1
    ),
    "the budget is what refuses, and it still does when segments are what carried the frames; \
     got {refused:?}"
  );

  // And the token engine, so "segments carry it" is not a typed-only claim: the two prologues are
  // separate lines of code and a fix that covered one and missed the other is exactly the shape
  // this file pins everywhere else.
  let token: TokOutcome = on_a_2mib_stack(|| {
    let src = prefix_chain(FAR_PAST_THE_CEILING);
    Parser::with_context(limited(FAR_PAST_THE_CEILING + 1))
      .apply(token_probe)
      .parse_str(&src)
      .unwrap()
  });
  assert!(
    token.is_ok(),
    "the token driver's prologue grows too; got {token:?}"
  );
}

/// A panic out of a frame that is running **on a segment** still unwinds, and the level it holds
/// is still released.
///
/// The extension of `descent_tests.rs`'s `descending_releases_the_level_when_the_body_unwinds` to
/// the case that file cannot reach: with `stacker` on, the frame body runs on a different stack
/// than the `Descent` guard that owns its level, because `maybe_grow` sits *inside* the guard's
/// scope. stacker catches the unwind on the segment and resumes it on the calling stack, so the
/// guard's destructor runs on the stack it was created on — but that is stacker's contract, not
/// tokora's, and a version bump could change it silently.
///
/// The depth is past the thread's ceiling on purpose: a shallow chain never leaves the original
/// stack, so a shallow version of this cell would pass without ever testing the crossing.
///
/// # What witnesses that the panic really started on a segment
///
/// Two readings, and the cell asserts both. The folds run on the way back *up*, so the one that
/// panics — the first at `FAR_PAST_THE_CEILING / 2` folded operators — is running in the frame at
/// native depth 500. That is past the **384** the table records for the typed driver on a 2 MiB
/// thread, and the parse had already gone to 1000 to get there: a build without segments does not
/// reach either number, it dies. So arriving at the panic at all is the witness, and it is the
/// same one `with_stacker_the_same_chain_parses_past_the_thread_it_runs_on` rests on.
#[cfg(feature = "stacker")]
#[test]
fn a_panic_on_a_segment_unwinds_and_releases_every_level() {
  use std::sync::atomic::{AtomicUsize, Ordering};

  /// The fold depth the panicking frame observed, recorded outside the `catch_unwind` because an
  /// assertion inside one is swallowed exactly as happily as the deliberate panic.
  static DEEPEST: AtomicUsize = AtomicUsize::new(0);
  /// The depth read after the unwind, from inside the grammar, where the live cell is visible.
  static AFTER: AtomicUsize = AtomicUsize::new(usize::MAX);

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      // A fold that panics once the chain is deep enough to be on a segment. The fold runs
      // inside the frame body, which is inside `maybe_grow`'s closure — so this panic starts on
      // a segment and has to cross back.
      let deep = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
                  operand: String,
                  op: Precedenced<&'static str, Power>|
       -> Result<String, LimErr> {
        let d = operand.matches('-').count();
        DEEPEST.fetch_max(d, Ordering::Relaxed);
        assert!(d < FAR_PAST_THE_CEILING / 2, "the fold panics on a segment");
        Ok(format!("({}{operand})", op.into_data()))
      };
      pratt(lhs, rhs, deep, fold_infix, fold_postfix).parse_input(inp)
    }));
    assert!(caught.is_err(), "the panic must reach this frame");
    AFTER.store(inp.recursion().depth(), Ordering::Relaxed);
    Ok(())
  }

  let previous = std::panic::take_hook();
  std::panic::set_hook(std::boxed::Box::new(|_| {}));
  let out = on_a_2mib_stack(|| {
    let src = prefix_chain(FAR_PAST_THE_CEILING);
    Parser::with_context(limited(FAR_PAST_THE_CEILING + 1))
      .apply(probe)
      .parse_str(&src)
  });
  std::panic::set_hook(previous);

  assert_eq!(out, Ok(()), "the unwind was caught and the parse returned");
  assert_eq!(
    DEEPEST.load(Ordering::Relaxed),
    FAR_PAST_THE_CEILING / 2,
    "the panic did not fire where this cell needs it to. Folds run on the way back up, so this \
     reading is also the native depth the panicking frame sat at — and it has to be past the 384 \
     the measurement table records for the typed driver on a 2 MiB thread, or nothing crossed a \
     stack boundary and the cell is testing the plain unwind `descent_tests.rs` already covers"
  );
  assert_eq!(
    AFTER.load(Ordering::Relaxed),
    0,
    "every level entered on a segment was released by the unwind that crossed back off it"
  );
}

/// **The negative property, and the reason it is a probe rather than an assertion.**
///
/// With the feature on and the budget set to [`RecursionLimiter::unlimited`], deepen the input and
/// nothing on the `Result` channel ever stops it. There is no depth at which this returns `Err`:
/// it returns `Ok`, using memory linear in the depth, until the machine's memory is gone — and
/// then the process is killed, or stacker's own `mmap` assertion fires. Neither is a value a
/// caller can match on, neither is [`MaybeTerminal`], and under `panic = "abort"` the second is
/// not even an unwind.
///
/// **That is why `InputRef::descend` stays and why deleting it because this feature exists would
/// restore the abort both defaults were written to delete.** `stacker` moved the ceiling from
/// hardware to policy; it did not remove it, and it is not a safety mechanism on its own.
///
/// A cell cannot assert this, because the outcome it is about is a dead process — and running it
/// to that point costs the whole machine, which is not something a test suite may do. So it is
/// `#[ignore]`d and takes its depth from the environment, exactly as `fuzz_deep_run` does, and the
/// run is recorded in `src/native_stack.rs`:
///
/// ```text
/// TOKORA_PROBE_DEPTH=200000 cargo test -p tokora --all-features --test pratt_limit \
///   -- --ignored --exact the_unlimited_budget_has_no_ceiling_to_refuse_at --nocapture
/// ```
///
/// Every line it prints is a survival, and the survivals are the point: the interesting outcome is
/// the run that prints nothing.
#[test]
#[ignore = "unbounded by construction — it ends when the machine's memory does; see the docs"]
fn the_unlimited_budget_has_no_ceiling_to_refuse_at() {
  let depth: usize = std::env::var("TOKORA_PROBE_DEPTH")
    .expect("set TOKORA_PROBE_DEPTH to the depth to probe")
    .parse()
    .expect("TOKORA_PROBE_DEPTH must be a depth");

  let got: Outcome = on_a_2mib_stack(move || {
    let src = prefix_chain(depth);
    Parser::with_context(fatal_ctx().with_recursion_limiter(RecursionLimiter::unlimited()))
      .apply(typed_probe)
      .parse_str(&src)
      .unwrap()
  });
  // Reported, never asserted. An assertion here would describe the shallow runs and say nothing
  // about the one that matters, and a cell that passes at every depth it survives is a cell that
  // passes by not reaching the property.
  match got {
    Ok((tree, _)) => println!(
      "depth={depth} SURVIVED ok, folded={} — no refusal, and none is coming",
      tree.matches('-').count()
    ),
    Err(e) => println!("depth={depth} SURVIVED err={e:?}"),
  }
}

/// A checkpoint whose save and restore **bracket a segment crossing** behaves exactly as one that
/// never leaves the original stack.
///
/// # Why this is the shape, and what it is really guarding
///
/// A frame cannot straddle a boundary: `maybe_grow` wraps the **whole** frame body, so a frame runs
/// entirely on the stack it started on and any save/restore pair inside one frame is same-segment
/// by construction. The pair that can span a crossing is therefore the outer one — saved above the
/// recursion, restored after it — which is what this cell writes.
///
/// The thing actually at risk is not the `Checkpoint`, which holds offsets, spans and marks and no
/// pointer into a stack. It is the input-identity **nonce** the transaction and session-point
/// machinery stamps: `core::ptr::from_ref(&*self.poison_boundary).addr()`, the one address-derived
/// value on this path. It is safe because it addresses a field of the `Input`, which is built by
/// the driver before any pratt frame exists and therefore never moves onto a segment — but "an
/// address that happens not to move" is exactly the claim a segmented stack invites someone to get
/// wrong, so it is pinned rather than reasoned about. The session point below is stamped on the
/// original stack and settled after 1000 levels of it have run somewhere else.
#[cfg(all(feature = "stacker", feature = "unstable-raw"))]
#[test]
fn a_checkpoint_bracketing_a_segment_crossing_restores_identically() {
  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize, usize), LimErr>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = LimErr>,
  {
    // Stamped here, on the original stack, before a single segment exists.
    let point = inp.begin_point();
    let saved = inp.save();
    let before = inp.span().end();

    // 1000 levels, most of them on segments the frame above never saw.
    let parsed = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)?;
    let after_parse = inp.span().end();
    assert!(
      after_parse > before,
      "the deep parse must have consumed something for the restore to have work to do"
    );
    assert_eq!(
      parsed.matches('-').count(),
      FAR_PAST_THE_CEILING,
      "the whole chain folded before the restore"
    );

    // Restored here, back on the original stack — and the settle re-derives the nonce and
    // compares it with the one stamped above. A moved `Input` panics on this line.
    inp.restore(saved);
    let after_restore = inp.span().end();
    inp.commit_point(point);
    Ok((before, after_parse, after_restore))
  }

  let (before, after_parse, after_restore) = on_a_2mib_stack(|| {
    let src = prefix_chain(FAR_PAST_THE_CEILING);
    Parser::with_context(limited(FAR_PAST_THE_CEILING + 1))
      .apply(probe)
      .parse_str(&src)
      .unwrap()
  });
  assert_eq!(
    after_restore, before,
    "the restore put the input back exactly where the save found it, across {} levels that ran \
     on other stacks (the parse had reached {after_parse})",
    FAR_PAST_THE_CEILING
  );
}
