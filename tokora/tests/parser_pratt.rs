#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests for the Pratt parser API.
//!
//! Covers both the token-level API (`InputRef::pratt`) and the combinator
//! API (`pratt`).

mod common;

use tokora::EmitterView;
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext, SimpleSpan,
  emitter::{PrattEmitter, Verbose},
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
    token::UnexpectedTokenOf,
  },
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  span::Spanned,
  token::PrattToken,
};

use common::{Power, TestLexer, Token};

// ── Shared: error type and binding-power newtype ──────────────────────────────

#[derive(Debug)]
struct PrattError;

impl From<()> for PrattError {
  fn from(_: ()) -> Self {
    PrattError
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, TestLexer<'inp>>> for PrattError {
  fn from(_: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Self {
    PrattError
  }
}

impl From<UnexpectedEoLhs> for PrattError {
  fn from(_: UnexpectedEoLhs) -> Self {
    PrattError
  }
}

impl From<UnexpectedEoRhs> for PrattError {
  fn from(_: UnexpectedEoRhs) -> Self {
    PrattError
  }
}
impl From<RecursionLimitReached> for PrattError {
  fn from(_: RecursionLimitReached) -> Self {
    PrattError
  }
}
impl From<NonAssociativeChain> for PrattError {
  fn from(_: NonAssociativeChain) -> Self {
    PrattError
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for PrattError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    PrattError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for PrattError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    PrattError
  }
}

const PREC_PAREN: Power = Power(-1); // ( )
const PREC_SUM: Power = Power(1); //   + -
const PREC_PROD: Power = Power(2); //  * /
const PREC_NEG: Power = Power(3); //   unary -

// ── Token-level API (`InputRef::pratt`) ──────────────────────────────────────

/// Classify `Token` as an operand, prefix, or infix/postfix for the
/// token-level Pratt API.
impl PrattToken<'_, i64, Power> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    Some(match self {
      Token::Num(_) => PrattLHS::Operand(()),
      Token::Minus => PrattLHS::Prefix(Precedenced::new((), PREC_NEG)),
      // `(` triggers a nested pratt call; `)` will terminate it.
      Token::LParen => PrattLHS::Prefix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>> {
    Some(match self {
      Token::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
      Token::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
      Token::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
      Token::Slash => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
      // `)` is a postfix at PREC_PAREN; consumed only inside a `(` group.
      Token::RParen => PrattRHS::Postfix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }
}

type Tok = Spanned<Token, SimpleSpan>;

// Token-level fold functions are generic over `E` (the emitter type) so that
// the higher-rank lifetime bound is satisfied automatically.

fn tok_fold_prefix<'inp, E>(
  op: Tok,
  operand: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, PrattError> {
  match op.into_data() {
    Token::Minus => {
      let n = tok_num(operand);
      Ok(num_tok(-n))
    }
    Token::LParen => Ok(operand), // grouping: pass result through
    _ => unreachable!(),
  }
}

fn tok_fold_infix<'inp, E>(
  left: Tok,
  right: Tok,
  infix: Spanned<PrattInfix<Token, Token, Token>, SimpleSpan>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, PrattError> {
  let l = tok_num(left);
  let r = tok_num(right);
  let op = match infix.into_data() {
    PrattInfix::Left(t) | PrattInfix::Right(t) | PrattInfix::Neither(t) => t,
  };
  let result = match op {
    Token::Plus => l + r,
    Token::Minus => l - r,
    Token::Star => l * r,
    Token::Slash => l / r,
    _ => unreachable!(),
  };
  Ok(num_tok(result))
}

fn tok_fold_postfix<'inp, E>(
  operand: Tok,
  _op: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, PrattError> {
  Ok(operand) // `)` consumed; pass grouped result through
}

fn tok_num(tok: Tok) -> i64 {
  match tok.into_data() {
    Token::Num(n) => n,
    _ => unreachable!(),
  }
}

fn num_tok(n: i64) -> Tok {
  Spanned::new(SimpleSpan::new(0, 0), Token::Num(n))
}

/// Entry-point using the token-level `inp.pratt()` API.
fn calc_token<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = PrattError> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  let result = inp.pratt::<_, _, _, i64, Power>(
    tok_fold_prefix::<Ctx::Emitter>,
    tok_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
  )?;
  match result {
    Some(tok) => Ok(tok_num(tok)),
    None => Err(PrattError),
  }
}

// ── Recovery posture of the token driver (documented at the two exits) ────────
//
// The token-level driver has no node to withhold, so when a prefix operator's operand never
// arrives it reports the diagnostic and then returns **the operator token itself** as the
// expression. Under a recording emitter the parse therefore continues carrying a bare `-`
// where a negation should be. That is a deliberate posture, not an oversight — callers
// needing the stricter one use the typed driver — and it had no cell, which is how the
// behaviour stayed undocumented.
//
// Falsifier: delete the `emit_unexpected_end_of_lhs` call in `input_ref/pratt.rs` and the
// diagnostic assertion below fires (recorded count drops to 0) while the return stays
// `Some(Minus)` — the two halves are independent.
#[test]
fn token_driver_returns_the_prefix_operator_after_reporting_its_missing_operand() {
  fn probe<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<PrattError>>,
    >,
  ) -> Result<(Option<Token>, usize), PrattError> {
    let out = inp.pratt::<_, _, _, i64, Power>(
      tok_fold_prefix::<Verbose<PrattError>>,
      tok_fold_infix::<Verbose<PrattError>>,
      tok_fold_postfix::<Verbose<PrattError>>,
    )?;
    let recorded = inp.emitter_ref().errors().values().flatten().count();
    Ok((out.map(|t| t.data().clone()), recorded))
  }

  let (returned, recorded) = Parser::with_context(ParserContext::new(Verbose::<PrattError>::new()))
    .apply(probe)
    .parse_str("-")
    .expect("a recording emitter recovers from a prefix operator at end of input");

  assert_eq!(
    returned,
    Some(Token::Minus),
    "the prefix operator itself is returned as the expression when its operand is missing",
  );
  assert_eq!(
    recorded, 1,
    "the missing-operand diagnostic must be recorded before the operator is returned",
  );
}

#[test]
fn test_pratt_token_add() {
  let r: i64 = Parser::new().apply(calc_token).parse_str("1 + 2").unwrap();
  assert_eq!(r, 3);
}

#[test]
fn test_pratt_token_sub() {
  let r: i64 = Parser::new().apply(calc_token).parse_str("5 - 3").unwrap();
  assert_eq!(r, 2);
}

#[test]
fn test_pratt_token_mul() {
  let r: i64 = Parser::new().apply(calc_token).parse_str("3 * 4").unwrap();
  assert_eq!(r, 12);
}

#[test]
fn test_pratt_token_div() {
  let r: i64 = Parser::new().apply(calc_token).parse_str("10 / 2").unwrap();
  assert_eq!(r, 5);
}

#[test]
fn test_pratt_token_precedence_mul_over_add() {
  // 1 + 2 * 3 = 7 (not 9); * has higher precedence than +
  let r: i64 = Parser::new()
    .apply(calc_token)
    .parse_str("1 + 2 * 3")
    .unwrap();
  assert_eq!(r, 7);
}

#[test]
fn test_pratt_token_paren_overrides_precedence() {
  // (1 + 2) * 3 = 9
  let r: i64 = Parser::new()
    .apply(calc_token)
    .parse_str("(1 + 2) * 3")
    .unwrap();
  assert_eq!(r, 9);
}

#[test]
fn test_pratt_token_unary_minus() {
  let r: i64 = Parser::new().apply(calc_token).parse_str("-5").unwrap();
  assert_eq!(r, -5);
}

#[test]
fn test_pratt_token_left_assoc_sub() {
  // 10 - 3 - 2 = (10 - 3) - 2 = 5 (left-associative)
  let r: i64 = Parser::new()
    .apply(calc_token)
    .parse_str("10 - 3 - 2")
    .unwrap();
  assert_eq!(r, 5);
}

#[test]
fn test_pratt_token_single_num() {
  let r: i64 = Parser::new().apply(calc_token).parse_str("42").unwrap();
  assert_eq!(r, 42);
}

// ── Combinator API (`pratt`) ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum BinOp {
  Add,
  Sub,
  Mul,
  Div,
}

/// LHS parser: numbers, unary minus, and grouped `(expr)`.
fn comb_parse_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  match inp.next()? {
    None => Err(PrattError),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new((), PREC_NEG))),
      Token::LParen => {
        let e = comb_parse_expr(inp)?;
        if inp
          .try_expect(|t| matches!(t.data(), Token::RParen))?
          .is_none()
        {
          return Err(PrattError);
        }
        Ok(PrattLHS::Operand(e))
      }
      _ => Err(PrattError),
    },
  }
}

/// RHS parser: binary operators; anything else ends the expression.
fn comb_parse_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<BinOp, BinOp, BinOp, (), Power>, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Add),
        PREC_SUM,
      ))),
      Token::Minus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Sub),
        PREC_SUM,
      ))),
      Token::Star => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Mul),
        PREC_PROD,
      ))),
      Token::Slash => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Div),
        PREC_PROD,
      ))),
      _ => Ok(PrattRHS::End),
    },
  }
}

fn comb_fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  Ok(-operand)
}

fn comb_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  op: Precedenced<PrattInfix<BinOp, BinOp, BinOp>, Power>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  let bin_op = match op.into_data() {
    PrattInfix::Left(o) | PrattInfix::Right(o) | PrattInfix::Neither(o) => o,
  };
  Ok(match bin_op {
    BinOp::Add => left + right,
    BinOp::Sub => left - right,
    BinOp::Mul => left * right,
    BinOp::Div => left / right,
  })
}

fn comb_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  Ok(operand) // this grammar declares no postfix operator, so nothing folds here
}

/// Entry-point using the `pratt` combinator API.
fn comb_parse_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  pratt(
    comb_parse_lhs,
    comb_parse_rhs,
    comb_fold_prefix,
    comb_fold_infix,
    comb_fold_postfix,
  )
  .parse_input(inp)
}

#[test]
fn test_pratt_comb_add() {
  let r: i64 = Parser::new()
    .apply(comb_parse_expr)
    .parse_str("3 + 4")
    .unwrap();
  assert_eq!(r, 7);
}

#[test]
fn test_pratt_comb_mul() {
  let r: i64 = Parser::new()
    .apply(comb_parse_expr)
    .parse_str("3 * 4")
    .unwrap();
  assert_eq!(r, 12);
}

#[test]
fn test_pratt_comb_precedence() {
  // 2 + 3 * 4 = 14 (not 20)
  let r: i64 = Parser::new()
    .apply(comb_parse_expr)
    .parse_str("2 + 3 * 4")
    .unwrap();
  assert_eq!(r, 14);
}

#[test]
fn test_pratt_comb_paren() {
  // (2 + 3) * 4 = 20
  let r: i64 = Parser::new()
    .apply(comb_parse_expr)
    .parse_str("(2 + 3) * 4")
    .unwrap();
  assert_eq!(r, 20);
}

#[test]
fn test_pratt_comb_unary_minus() {
  let r: i64 = Parser::new()
    .apply(comb_parse_expr)
    .parse_str("-7")
    .unwrap();
  assert_eq!(r, -7);
}

#[test]
fn test_pratt_comb_left_assoc() {
  // 10 - 3 - 2 = 5
  let r: i64 = Parser::new()
    .apply(comb_parse_expr)
    .parse_str("10 - 3 - 2")
    .unwrap();
  assert_eq!(r, 5);
}

#[test]
fn test_pratt_comb_div() {
  let r: i64 = Parser::new()
    .apply(comb_parse_expr)
    .parse_str("20 / 4")
    .unwrap();
  assert_eq!(r, 5);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Lookahead-equivalence witness (issue #87) — an empty cache vs a cache preloaded
// through EOI diverge on the identical input and grammar
// ═══════════════════════════════════════════════════════════════════════════════

/// Entry-point identical to `calc_token`, but first preloads the cache through EOI via
/// `peek::<U3>()` — the dispatcher-peek shape that exposes the divergence: both pratt
/// loops gate on `is_eoi()`, which reads the SCANNER's frontier (has the
/// lexer reached the end of the source?), not the CONSUMER's (are there still cached,
/// unconsumed tokens to fold?). A peek that reaches end-of-input makes the loop believe
/// the expression is already over even though the operator and RHS still sit in the
/// cache, unconsumed.
fn calc_token_cache_preloaded_through_eoi<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = PrattError> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  use generic_arraydeque::typenum::U3;
  // "1 + 2" is exactly 3 tokens: a U3 fill caches all of them, hitting EOI.
  let _ = inp.peek::<U3>()?;
  calc_token(inp)
}

/// A parse result is a function of the token stream, not of lookahead history (issue #87).
///
/// The SAME input and grammar parsed two ways: `control` never peeks before delegating
/// to pratt (empty cache, the equivalence oracle); `preloaded` peeks a U3 window first
/// (every token of `"1 + 2"` cached, hitting EOI). These used to diverge — the preloaded
/// run truncated to its LHS operand alone, `Ok`-shaped and with no diagnostic on any
/// channel — because the RHS loop pre-gated on the scanner's frontier, which the peek had
/// already moved to the end of the buffer while `+ 2` sat unconsumed in the cache. The
/// loop now asks the RHS parser, which is the only thing that knows.
#[test]
fn lookahead_equivalence_holds_when_the_cache_is_preloaded_through_eoi() {
  let control: i64 = Parser::new().apply(calc_token).parse_str("1 + 2").unwrap();
  let preloaded: i64 = Parser::new()
    .apply(calc_token_cache_preloaded_through_eoi)
    .parse_str("1 + 2")
    .unwrap();

  assert_eq!(
    control, 3,
    "the un-peeked control parses the whole expression"
  );
  assert_eq!(
    preloaded, 3,
    "the preloaded run parses the same expression: a lookahead that reached the end of the \
     buffer says nothing about whether this consumer still has operators to fold"
  );
  assert_eq!(
    control, preloaded,
    "same input, same grammar, different lookahead history — the equivalence must hold"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Dense-level associativity witness (issue #93; shares the root cause tracked in #87):
// a strictly-lower-precedence operator binds INTO the right operand of an adjacent
// right-associative level
// ═══════════════════════════════════════════════════════════════════════════════

const DENSE_MUL: Power = Power(3); // `*` — Right-assoc (deliberately unusual: the adversarial shape)
const DENSE_ADD: Power = Power(2); // `+` — Left-assoc, one dense level directly below `*`

/// LHS parser for the dense-level grammar: bare numbers only (no prefix operators — not
/// needed to reproduce the adversarial history).
fn dense_parse_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(PrattError),
    },
    None => Err(PrattError),
  }
}

/// RHS parser for the dense-level grammar: `*` is RIGHT-associative at power 3, `+` is
/// LEFT-associative at power 2 — dense, adjacent levels (the Python/JS `**`-above-`*`
/// shape, applied to `*` itself for a minimal reproduction).
fn dense_parse_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<BinOp, BinOp, BinOp, (), Power>, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => match tok.into_data() {
      Token::Star => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(BinOp::Mul),
        DENSE_MUL,
      ))),
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(BinOp::Add),
        DENSE_ADD,
      ))),
      _ => Ok(PrattRHS::End),
    },
  }
}

/// Entry-point for the dense-level grammar, reusing the calculator's fold functions
/// (`comb_fold_prefix` is never invoked: `dense_parse_lhs` never returns `Prefix`).
fn dense_parse_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, PrattError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = PrattError>,
{
  pratt(
    dense_parse_lhs,
    dense_parse_rhs,
    comb_fold_prefix,
    comb_fold_infix,
    comb_fold_postfix,
  )
  .parse_input(inp)
}

/// A right-associative operator's right operand stops at that operator's own power — no
/// weaker operator gets in (issue #93, shares the root cause tracked in #87).
///
/// `*` = Right(3), `+` = Left(2) — dense adjacent levels. The right-associative recursion
/// floor used to be computed as `lpower.prev()`, which admits `power - 1`, so after
/// binding `*` the recursive RHS call admitted `+` — one whole level lower — INTO the
/// right operand and `2 * 3 + 4` parsed as `2 * (3 + 4)` = 14. Right-associativity means
/// the floor admits the operator's *own* power (so a second `*` binds inward), not one
/// below it.
#[test]
fn dense_level_right_assoc_floor_keeps_lower_operator_out_of_the_rhs() {
  let r: i64 = Parser::new()
    .apply(dense_parse_expr)
    .parse_str("2 * 3 + 4")
    .unwrap();
  assert_eq!(
    r, 10,
    "`+` is one level below `*`, so it must fold the whole `2 * 3` — (2*3)+4, not 2*(3+4)"
  );
}
