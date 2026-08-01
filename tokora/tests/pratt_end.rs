#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! `PrattRHS::End`: the RHS channel's way of saying the expression stops here.
//!
//! What it buys that a below-minimum "sentinel" postfix cannot:
//!
//! * it is expressible over an **unsigned** `Power`, whose minimum representable value *is*
//!   the default floor, so no sentinel exists to write there at all;
//! * no floor admits it, so lowering `min_precedence` or recursing under a low-power operator
//!   cannot turn the end of an expression into a phantom fold that steals a token;
//! * it restores whatever the deciding read consumed, so a multi-token look at the position
//!   is safe.

mod common;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  emitter::PrattEmitter,
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
    token::UnexpectedTokenOf,
  },
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  token::PrattToken,
};

use common::{Power, TestLexer, Token};

#[derive(Debug)]
struct EndError;

impl From<()> for EndError {
  fn from(_: ()) -> Self {
    EndError
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, TestLexer<'inp>>> for EndError {
  fn from(_: UnexpectedTokenOf<'inp, TestLexer<'inp>>) -> Self {
    EndError
  }
}
impl From<UnexpectedEoLhs> for EndError {
  fn from(_: UnexpectedEoLhs) -> Self {
    EndError
  }
}
impl From<UnexpectedEoRhs> for EndError {
  fn from(_: UnexpectedEoRhs) -> Self {
    EndError
  }
}
impl From<RecursionLimitReached> for EndError {
  fn from(_: RecursionLimitReached) -> Self {
    EndError
  }
}
impl From<NonAssociativeChain> for EndError {
  fn from(_: NonAssociativeChain) -> Self {
    EndError
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for EndError {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    EndError
  }
}
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for EndError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    EndError
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unsigned powers at the top of the ladder
// ═══════════════════════════════════════════════════════════════════════════════
//
// These two grammars are only writable because `End` exists: their `Power` is `u8`, so
// `Power::default()` is the minimum representable value and there is nothing below the entry
// floor to spell "not an operator" with.

fn u8_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), u8>, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(EndError),
    },
    None => Err(EndError),
  }
}

fn u8_fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), u8>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(-operand)
}

fn u8_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), u8>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(operand)
}

// ── Left-associative at the very top of the ladder ───────────────────────────

fn max_sub_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Minus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        u8::MAX,
      ))),
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

fn max_sub_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, u8>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(left - right)
}

fn max_sub_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  pratt(
    u8_lhs,
    max_sub_rhs,
    u8_fold_prefix,
    max_sub_fold_infix,
    u8_fold_postfix,
  )
  .parse_input(inp)
}

/// Left-associativity survives the top of the ladder.
///
/// `-` is `Left(u8::MAX)`, so `10 - 3 - 2` must group as `(10 - 3) - 2` = 5. The floor used
/// to be `next(MAX)`, which saturates back to `MAX` and therefore admits the equal-power `-`
/// into the right operand: `10 - (3 - 2)` = 9, a right-associative parse of an operator
/// declared left-associative. An exclusive bound cannot saturate, because it never computes
/// a value in the first place.
#[test]
fn typed_left_assoc_at_the_top_of_the_ladder_stays_left_associative() {
  let r: i64 = Parser::new()
    .apply(max_sub_expr)
    .parse_str("10 - 3 - 2")
    .unwrap();
  assert_eq!(r, 5);
}

// ── Non-associative at the very top of the ladder ────────────────────────────

fn max_semi_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Semi => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        u8::MAX,
      ))),
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

/// Records the fold tree's shape in the value: one fold over `1 ; 2` reads `12`, a second
/// over `; 3` reads `123`.
fn digits_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, u8>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(left * 10 + right)
}

/// Reports the pratt parser's own outcome plus, on the error path, where the input was left:
/// `Ok(value)` for a completed expression, `Err(at)` for a non-associative repeat whose
/// deciding operator is still the next token.
fn max_semi_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Result<i64, usize>, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  match pratt(
    u8_lhs,
    max_semi_rhs,
    u8_fold_prefix,
    digits_fold_infix,
    u8_fold_postfix,
  )
  .parse_input(inp)
  {
    Ok(value) => Ok(Ok(value)),
    Err(EndError) => {
      // The operator that tripped the chain constraint is handed back unconsumed: the very next
      // read sees it, at the offset the error reports.
      let semi = inp
        .try_expect(|t| matches!(t.data(), Token::Semi))?
        .expect("the repeated `;` must still be on the input");
      Ok(Err(semi.span().start()))
    }
  }
}

/// A non-associative chain is **rejected** at the top of the ladder, with the second operator
/// handed back.
///
/// `;` is `Neither(u8::MAX)`, so `1 ; 2 ; 3` is a declared-non-associative chain: the driver
/// folds `1 ; 2`, sees a second `;` at the same power, and fails the parse rather than ending
/// the expression. Ending it is not a conservative answer — an enclosing frame would fold the
/// leftover operator by its own rules and re-associate the chain across it with nothing left
/// over for any caller to reject.
///
/// This replaces `typed_neither_chain_at_the_top_of_the_ladder_stops_after_one_fold`, and it
/// still pins the regression that test was written for. Under the old saturating floor the
/// recursion bound at the ladder top stayed `MAX`, so the inner call consumed the second `;`
/// itself and *both* folds ran — which would show up here as `Ok(123)`. Reaching the repeat
/// guard at all proves the `Exclusive` floor handed the second `;` back to this frame, and the
/// error distinguishes the two failure modes strictly better than the old `(12, true)` did.
#[test]
fn typed_neither_chain_at_the_top_of_the_ladder_is_rejected() {
  let outcome: Result<i64, usize> = Parser::new()
    .apply(max_semi_expr)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(
    outcome,
    Err(6),
    "the second `;` (at offset 6) must be rejected, not folded and not left as a remainder"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// `End` at a floor that would have admitted a sentinel
// ═══════════════════════════════════════════════════════════════════════════════

fn num_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(EndError),
    },
    None => Err(EndError),
  }
}

fn signed_fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(-operand)
}

fn signed_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(operand)
}

fn div_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Slash => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(()),
        Power(0),
      ))),
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

fn div_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(if right == 0 { 0 } else { left / right })
}

fn div_expr_deep<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(i64, bool), EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  let value = pratt(
    num_lhs,
    div_rhs,
    signed_fold_prefix,
    div_fold_infix,
    signed_fold_postfix,
  )
  // Far below anything a sentinel could have been given, and below the `/` operator itself.
  .min_precedence(Power(-5))
  .parse_input(inp)?;
  let rparen_remains = inp
    .try_expect(|t| matches!(t.data(), Token::RParen))?
    .is_some();
  Ok((value, rparen_remains))
}

/// `End` outranks nothing, so no floor can admit it.
///
/// The same grammar and input as the un-migrated compatibility pin in `pratt_floor.rs`, but
/// run at `min_precedence(Power(-5))` — a floor that would admit *any* sentinel a signed
/// ladder could express. The expression still ends at `)` and leaves it on the input, which
/// is the property no choice of sentinel power can guarantee.
#[test]
fn end_is_admitted_by_no_floor_however_low() {
  let (value, rparen_remains): (i64, bool) = Parser::new()
    .apply(div_expr_deep)
    .parse_str("1 / 2 )")
    .unwrap();
  assert_eq!(
    (value, rparen_remains),
    (0, true),
    "`1 / 2` folded, and `)` is still on the input"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// `End` restores what the deciding read consumed
// ═══════════════════════════════════════════════════════════════════════════════

fn greedy_end_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Plus => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        Power(1),
      ))),
      // Deliberately greedy: read three more tokens before concluding. All four must come
      // back — a decision that ends the expression consumes nothing on the way out.
      _ => {
        let _ = inp.next()?;
        let _ = inp.next()?;
        let _ = inp.next()?;
        Ok(PrattRHS::End)
      }
    },
    None => Ok(PrattRHS::End),
  }
}

fn greedy_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<i64, EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  Ok(left + right)
}

fn greedy_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(i64, usize), EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = EndError>,
{
  let value = pratt(
    num_lhs,
    greedy_end_rhs,
    signed_fold_prefix,
    greedy_fold_infix,
    signed_fold_postfix,
  )
  .parse_input(inp)?;
  let mut left_over = 0usize;
  while inp.next()?.is_some() {
    left_over += 1;
  }
  Ok((value, left_over))
}

/// Every token the deciding read consumed comes back.
///
/// The RHS parser here reads *four* tokens before answering `End`. `1 + 2 ; , ; ,` must
/// still fold to 3 and leave all four trailing tokens on the input for the surrounding
/// grammar: the driver rolls the whole decision back, not just its first token.
#[test]
fn end_restores_every_token_the_decision_consumed() {
  let (value, left_over): (i64, usize) = Parser::new()
    .apply(greedy_expr)
    .parse_str("1 + 2 ; , ; ,")
    .unwrap();
  assert_eq!((value, left_over), (3, 4));
}

// ═══════════════════════════════════════════════════════════════════════════════
// A token classifier may spell its decline as `End`
// ═══════════════════════════════════════════════════════════════════════════════

impl PrattToken<'_, i64, Power> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    match self {
      Token::Num(_) => Some(PrattLHS::Operand(())),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>> {
    match self {
      Token::Plus => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left(()),
        Power(1),
      ))),
      // `None` is this trait's own decline; `Some(End)` must mean exactly the same thing,
      // so a classifier shared with the typed driver needs no translation layer.
      Token::Semi => Some(PrattRHS::End),
      _ => None,
    }
  }
}

type Tok = tokora::span::Spanned<Token, tokora::SimpleSpan>;

fn tok_fold_prefix<E>(_op: Tok, operand: Tok, _: &mut E) -> Result<Tok, EndError> {
  Ok(operand)
}

fn tok_fold_postfix<E>(operand: Tok, _op: Tok, _: &mut E) -> Result<Tok, EndError> {
  Ok(operand)
}

fn tok_fold_infix<E>(
  left: Tok,
  right: Tok,
  infix: tokora::span::Spanned<PrattInfix<Token, Token, Token>, tokora::SimpleSpan>,
  _: &mut E,
) -> Result<Tok, EndError> {
  let value = match (left.data(), right.data()) {
    (Token::Num(l), Token::Num(r)) => l + r,
    _ => return Err(EndError),
  };
  Ok(tokora::span::Spanned::new(
    infix.into_span(),
    Token::Num(value),
  ))
}

fn token_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(i64, bool), EndError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TestLexer<'inp>, Error = EndError> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  let out = inp.pratt::<_, _, _, i64, Power>(
    tok_fold_prefix::<Ctx::Emitter>,
    tok_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
  )?;
  let value = match out.expect("the input opens with an operand").into_data() {
    Token::Num(n) => n,
    _ => return Err(EndError),
  };
  let semi_remains = inp
    .try_expect(|t| matches!(t.data(), Token::Semi))?
    .is_some();
  Ok((value, semi_remains))
}

/// A token classifier's `Some(End)` is its `None`.
///
/// `;` classifies as `End` rather than declining; the token driver must treat the two
/// identically — end the expression and leave the token in the stream.
#[test]
fn token_classifier_end_is_the_same_as_declining() {
  let (value, semi_remains): (i64, bool) = Parser::new()
    .apply(token_expr)
    .parse_str("1 + 2 ; 3")
    .unwrap();
  assert_eq!((value, semi_remains), (3, true));
}
