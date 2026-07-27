#![cfg(all(feature = "std", feature = "logos"))]

// Additional tests targeting uncovered code paths in:
//   - parser/pratt/expr.rs (prefix/infix/postfix/min_precedence config, Neither assoc, Right assoc)
//   - parser/punct.rs (Comma/Semicolon/OpenParen/CloseParen ::parse and ::try_parse)
//   - parser/fold/ (try_fold_with, try_fold_while_with)
//   - input/input_ref/try_expect.rs (try_expect_map / try_expect_and_then with cached tokens)
//   - input/input_ref/sync_through.rs (sync_through with cached tokens)

mod common;

use common::{Power, TestLexer, Token, TokenKind};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext, TryParseInput,
  cache::{DefaultCache, Peeked},
  emitter::Ignored,
  error::{UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot, token::UnexpectedToken},
  parser::{Action, PrattInfix, PrattLHS, PrattRHS, Precedenced, expect, pratt},
  punct::{CloseParen, Comma, OpenParen, Semicolon},
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::Expected,
};

use generic_arraydeque::typenum::U1;

// ── Error types ─────────────────────────────────────────────────────────────

#[derive(Debug)]
struct TestError;

impl From<()> for TestError {
  fn from(_: ()) -> Self {
    TestError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for TestError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    TestError
  }
}

impl<S, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<S, Lang, Set>> for TestError {
  fn from(_: UnexpectedEot<S, Lang, Set>) -> Self {
    TestError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for TestError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    TestError
  }
}

impl From<UnexpectedEoLhs> for TestError {
  fn from(_: UnexpectedEoLhs) -> Self {
    TestError
  }
}

impl From<UnexpectedEoRhs> for TestError {
  fn from(_: UnexpectedEoRhs) -> Self {
    TestError
  }
}

// ── Ignored context helper ──────────────────────────────────────────────────

type IgnoredContext<'inp> =
  ParserContext<'inp, TestLexer<'inp>, Ignored, DefaultCache<'inp, TestLexer<'inp>>>;

macro_rules! ignored_parser {
  () => {
    Parser::with_context(IgnoredContext::new(Ignored::default()))
  };
}

const PREC_SUM: Power = Power(1);
const PREC_PROD: Power = Power(2);
const PREC_NEG: Power = Power(3);
const PREC_ASSIGN: Power = Power(0);

// ── Pratt helpers ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum BinOp {
  Add,
  Sub,
  Mul,
  Div,
  Assign,
}

// LHS parser for pratt combinator
fn pratt_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), Power>, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
{
  match inp.next()? {
    None => Err(TestError),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n)),
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new((), PREC_NEG))),
      Token::LParen => {
        let e = pratt_expr(inp)?;
        if inp
          .try_expect(|t| matches!(t.data(), Token::RParen))?
          .is_none()
        {
          return Err(TestError);
        }
        Ok(PrattLHS::Operand(e))
      }
      _ => Err(TestError),
    },
  }
}

// RHS parser with Right-associative and Neither-associative operators
fn pratt_rhs_with_right_neither<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<BinOp, BinOp, BinOp, (), Power>, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
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
      // '=' is right-associative
      Token::Eq => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(BinOp::Assign),
        PREC_ASSIGN,
      ))),
      // ';' is non-associative (Neither)
      Token::Semi => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(BinOp::Add),
        PREC_SUM,
      ))),
      _ => Ok(PrattRHS::End),
    },
  }
}

fn pratt_fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
{
  Ok(-operand)
}

fn pratt_fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  op: Precedenced<PrattInfix<BinOp, BinOp, BinOp>, Power>,
) -> Result<i64, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
{
  let bin_op = match op.into_data() {
    PrattInfix::Left(o) | PrattInfix::Right(o) | PrattInfix::Neither(o) => o,
  };
  Ok(match bin_op {
    BinOp::Add => left + right,
    BinOp::Sub => left - right,
    BinOp::Mul => left * right,
    BinOp::Div => left / right,
    BinOp::Assign => right, // right-assoc assignment: value is rhs
  })
}

fn pratt_fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), Power>,
) -> Result<i64, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
{
  Ok(operand)
}

// Basic pratt expression parser
fn pratt_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
{
  pratt(
    pratt_lhs,
    pratt_rhs_with_right_neither,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  )
  .parse_input(inp)
}

// Pratt expression parser using .prefix(), .infix(), .postfix() config methods
fn pratt_expr_configured<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
{
  pratt(
    pratt_lhs,
    pratt_rhs_with_right_neither,
    pratt_fold_prefix::<Ctx>,
    pratt_fold_infix::<Ctx>,
    pratt_fold_postfix::<Ctx>,
  )
  .prefix(pratt_fold_prefix::<Ctx>)
  .infix(pratt_fold_infix::<Ctx>)
  .postfix(pratt_fold_postfix::<Ctx>)
  .parse_input(inp)
}

// Pratt expression parser with min_precedence set
fn pratt_expr_min_prec<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, TestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
{
  pratt(
    pratt_lhs,
    pratt_rhs_with_right_neither,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  )
  .min_precedence(PREC_PROD) // only parse PROD-level and above
  .parse_input(inp)
}

// ── Pratt tests: Right-associative ──────────────────────────────────────────

#[test]
fn pratt_right_associative_assign() {
  // a = b = 5 should parse as a = (b = 5) = 5 (right assoc, value is rhs)
  // With numbers: 1 = 2 = 3 -> 1 = (2 = 3) -> 1 = 3 -> 3
  let r: i64 = Parser::new()
    .apply(pratt_expr)
    .parse_str("1 = 2 = 3")
    .unwrap();
  assert_eq!(r, 3);
}

#[test]
fn pratt_right_assoc_single() {
  // 1 = 5 -> value is 5
  let r: i64 = Parser::new().apply(pratt_expr).parse_str("1 = 5").unwrap();
  assert_eq!(r, 5);
}

// ── Pratt tests: Neither-associative ────────────────────────────────────────

#[test]
fn pratt_neither_assoc_single() {
  // 1 ; 2 with Neither assoc at PREC_SUM -> parses as (1 ; 2)
  let r: i64 = Parser::new().apply(pratt_expr).parse_str("1 ; 2").unwrap();
  assert_eq!(r, 3); // treated as add
}

#[test]
fn pratt_neither_assoc_stops_chaining() {
  // 1 ; 2 ; 3 with Neither-assoc should stop after first ; since second ;
  // has same precedence and associativity is Neither
  // So it parses 1 ; 2 = 3, then stops. The "; 3" is leftover.
  // The pratt parser should return 3 (1+2)
  let r: i64 = Parser::new()
    .apply(pratt_expr)
    .parse_str("1 ; 2 ; 3")
    .unwrap();
  assert_eq!(r, 3);
}

// ── Pratt tests: prefix/infix/postfix/min_precedence config methods ─────────

#[test]
fn pratt_configured_prefix_infix_postfix() {
  let r: i64 = Parser::new()
    .apply(pratt_expr_configured)
    .parse_str("2 + 3 * 4")
    .unwrap();
  assert_eq!(r, 14);
}

#[test]
fn pratt_configured_unary_minus() {
  let r: i64 = Parser::new()
    .apply(pratt_expr_configured)
    .parse_str("-7")
    .unwrap();
  assert_eq!(r, -7);
}

#[test]
fn pratt_min_precedence_limits_parsing() {
  // With min_precedence = PREC_PROD (2), only * and / should bind.
  // "3 * 4" -> 12
  let r: i64 = Parser::new()
    .apply(pratt_expr_min_prec)
    .parse_str("3 * 4")
    .unwrap();
  assert_eq!(r, 12);
}

#[test]
fn pratt_min_precedence_stops_at_lower() {
  // "3" with min_prec = PROD: single number should still work
  let r: i64 = Parser::new()
    .apply(pratt_expr_min_prec)
    .parse_str("3")
    .unwrap();
  assert_eq!(r, 3);
}

// ── Punct parser tests ──────────────────────────────────────────────────────

#[test]
fn punct_comma_parse_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let _comma = Comma::parse(inp)?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str(",");
  assert!(r.unwrap());
}

#[test]
fn punct_comma_parse_failure() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let _comma = Comma::parse(inp)?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("+");
  assert!(r.is_err());
}

#[test]
fn punct_comma_parse_eot() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let _comma = Comma::parse(inp)?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("");
  assert!(r.is_err());
}

#[test]
fn punct_comma_try_parse_accept() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let result = Comma::try_parse(inp)?;
    Ok(matches!(result, ParseAttempt::Accept(_)))
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str(",");
  assert!(r.unwrap());
}

#[test]
fn punct_comma_try_parse_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let result = Comma::try_parse(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("+");
  assert!(r.unwrap());
}

#[test]
fn punct_semicolon_parse_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let _semi = Semicolon::parse(inp)?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str(";");
  assert!(r.unwrap());
}

#[test]
fn punct_semicolon_parse_failure() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let _semi = Semicolon::parse(inp)?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("+");
  assert!(r.is_err());
}

#[test]
fn punct_open_paren_parse_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let _lp = OpenParen::parse(inp)?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("(");
  assert!(r.unwrap());
}

#[test]
fn punct_close_paren_parse_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let _rp = CloseParen::parse(inp)?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str(")");
  assert!(r.unwrap());
}

#[test]
fn punct_open_paren_try_parse_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let result = OpenParen::try_parse(inp)?;
    Ok(matches!(result, ParseAttempt::Decline))
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("+");
  assert!(r.unwrap());
}

#[test]
fn punct_semicolon_try_parse_accept() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    let result = Semicolon::try_parse(inp)?;
    Ok(matches!(result, ParseAttempt::Accept(_)))
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str(";");
  assert!(r.unwrap());
}

// ── Fold: try_fold_with ─────────────────────────────────────────────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  inp
    .try_expect(|t| matches!(t.data(), Token::Num(_)))
    .map(|opt| match opt {
      None => ParseAttempt::Decline,
      Some(tok) => ParseAttempt::Accept(match tok.into_data() {
        Token::Num(n) => n,
        _ => unreachable!(),
      }),
    })
}

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

#[test]
fn try_fold_with_sum_with_state() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .try_fold_with(|| 0i64, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("1 2 3").unwrap(), 6);
}

#[test]
fn try_fold_with_empty_returns_init() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .try_fold_with(|| 99i64, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("").unwrap(), 99);
}

#[test]
fn try_fold_with_error_propagates() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .try_fold_with(
        || 0i64,
        |_acc, x, _state| if x > 5 { Err(()) } else { Ok(x) },
      )
      .parse_input(inp)
  }
  assert!(Parser::new().apply(p).parse_str("3 10").is_err());
}

#[test]
fn try_fold_with_zero_width_element_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Accepts without ever touching `inp`, up to a budget: a driver missing the no-progress
    // guard loops until the budget runs out instead of the input running out.
    let mut budget = 5usize;
    let elem = move |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        ParseAttempt::Accept(1)
      } else {
        ParseAttempt::Decline
      })
    };
    elem
      .try_fold_with(|| 0usize, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(result, 1);
}

#[test]
fn try_fold_with_zero_width_peeking_element_stops_after_one_accumulation() {
  // Trivia-gap twin of the above for the `advanced` (`!=`-form) guard: the element peeks the
  // frontier (moving the cache-front cursor across the leading trivia in `" 1"`) but consumes
  // nothing. A cursor-keyed `advanced` reads that jump as progress and runs one extra cycle; the
  // committed-consumption metric stops after the first accumulation.
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let elem =
      |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
        let _ = inp.peek_one()?;
        Ok(ParseAttempt::Accept(1))
      };
    elem
      .try_fold_with(|| 0usize, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(result, 1);
}

// ── Fold: try_fold_while_with ───────────────────────────────────────────────

fn while_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _emitter: &mut Ctx::Emitter,
) -> Result<Action, <Ctx::Emitter as Emitter<'inp, TestLexer<'inp>>>::Error>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
{
  Ok(match peeked.pop_front() {
    None => Action::Stop,
    Some(tok) => {
      let tok = tok
        .as_maybe_ref()
        .map(|t| t.token().copied(), |t| t.token())
        .into_inner();
      if matches!(**tok.data(), Token::Num(_)) {
        Action::Continue
      } else {
        Action::Stop
      }
    }
  })
}

#[test]
fn try_fold_while_with_sum() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while_with::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("1 2 3 +").unwrap(), 6);
}

#[test]
fn try_fold_while_with_empty() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while_with::<_, _, _, U1>(while_num::<Ctx>, || 77i64, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), 77);
}

#[test]
fn try_fold_while_with_error() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while_with::<_, _, _, U1>(
        while_num::<Ctx>,
        || 0i64,
        |_acc, x, _state| if x > 5 { Err(()) } else { Ok(x) },
      )
      .parse_input(inp)
  }
  assert!(Parser::new().apply(p).parse_str("3 10 +").is_err());
}

#[test]
fn try_fold_while_with_zero_width_element_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Continues unconditionally, up to a budget, ignoring the peeked window entirely.
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: &mut Ctx::Emitter|
          -> Result<Action, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<usize, ()> { Ok(1) };
    elem
      .try_fold_while_with::<_, _, _, U1>(cond, || 0usize, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(result, 1);
}

// ── try_expect_map with cached tokens ───────────────────────────────────────
// When we peek first, the token goes into cache. Then try_expect_map uses the cache path.

#[test]
fn try_expect_map_from_cache_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    // Now try_expect_map should use the cache path
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn try_expect_map_from_cache_no_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

// ── try_expect_and_then with cached tokens ──────────────────────────────────

#[test]
fn try_expect_and_then_from_cache_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) if *n > 0 => Some(Ok(*n)),
      Token::Num(_) => Some(Err(())),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("5").unwrap();
  assert_eq!(result, Some(5));
}

#[test]
fn try_expect_and_then_from_cache_err() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) if *n > 0 => Some(Ok(*n)),
        Token::Num(_) => Some(Err(())),
        _ => None,
      })
      .map(|r| r.map(|(n, _)| n))
  }

  let result: Result<Option<i64>, ()> = Parser::new().apply(parse).parse_str("-3");
  assert!(result.is_err());
}

#[test]
fn try_expect_and_then_from_cache_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

// ── try_expect with cached tokens ───────────────────────────────────────────

#[test]
fn try_expect_from_cache_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    let result = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(matches!(result, Some(Token::Num(42))));
}

#[test]
fn try_expect_from_cache_no_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    let result = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(result.is_none());
}

// ── sync_through with cached tokens ─────────────────────────────────────────

#[test]
fn sync_through_with_cached_non_matching_token() {
  // When cache has a non-matching token, sync_through should pop it (emit unexpected)
  // and then continue scanning the remaining input.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache with "1"
    drop(inp.peek::<U1>()?);
    // sync_through: cache has Num(1) which doesn't match Semi
    // It should pop it from cache (emit unexpected), then scan input for ;
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

#[test]
fn sync_through_cache_miss_then_scan() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache with "1"
    drop(inp.peek::<U1>()?);
    // sync_through will check cache (Num(1) != Semi), emit unexpected, then scan input
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

#[test]
fn sync_through_cache_no_match_eof() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache with "1"
    drop(inp.peek::<U1>()?);
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }

  // No semicolon anywhere
  let result = ignored_parser!().apply(parse).parse_str("1 2").unwrap();
  assert!(result.is_none());
}

// ── InputRef punct convenience methods ──────────────────────────────────────

#[test]
fn expect_comma_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_comma()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str(",");
  assert!(r.unwrap());
}

#[test]
fn expect_comma_failure() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_comma()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("+");
  assert!(r.is_err());
}

#[test]
fn try_expect_comma_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_comma()?;
    Ok(result.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str(",").unwrap());
}

#[test]
fn try_expect_comma_decline() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_comma()?;
    Ok(result.is_some())
  }

  assert!(!Parser::new().apply(parse).parse_str("+").unwrap());
}

#[test]
fn try_expect_semicolon_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_semicolon()?;
    Ok(result.is_some())
  }

  assert!(Parser::new().apply(parse).parse_str(";").unwrap());
}

#[test]
fn expect_open_paren_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_open_paren()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("(");
  assert!(r.unwrap());
}

#[test]
fn expect_close_paren_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_close_paren()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str(")");
  assert!(r.unwrap());
}

#[test]
fn expect_open_bracket_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_open_bracket()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("[");
  assert!(r.unwrap());
}

#[test]
fn expect_close_bracket_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_close_bracket()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("]");
  assert!(r.unwrap());
}

#[test]
fn expect_open_brace_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_open_brace()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("{");
  assert!(r.unwrap());
}

#[test]
fn expect_close_brace_success() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_close_brace()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("}");
  assert!(r.unwrap());
}

#[test]
fn expect_semicolon_eot() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, TestError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = TestError>,
  {
    inp.expect_semicolon()?;
    Ok(true)
  }

  let r: Result<bool, TestError> = Parser::new().apply(parse).parse_str("");
  assert!(r.is_err());
}

// ── InputRef::fold and foldn with cached tokens ─────────────────────────────

fn extract_num<S>(tok: Spanned<Token, S>) -> i64 {
  match tok.into_data() {
    Token::Num(n) => n,
    _ => 0,
  }
}

#[test]
fn fold_with_cached_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    // fold should consume from cache first
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| acc + extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("1 2 3").unwrap();
  assert_eq!(result, 6);
}

#[test]
fn foldn_with_cached_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to populate cache, then drop the borrow
    drop(inp.peek::<U1>()?);
    inp.foldn(|| 0i64, |acc, tok| acc + extract_num(tok), 2)
  }

  let result = Parser::new().apply(parse).parse_str("10 20 30").unwrap();
  assert_eq!(result, 30);
}
