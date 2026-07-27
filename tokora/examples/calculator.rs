//! A simple arithmetic expression evaluator using [`InputRef::pratt`](tokora::InputRef::pratt).
//!
//! Demonstrates the token-level Pratt API where the token type implements
//! [`PrattToken`](tokora::token::PrattToken) to classify itself as an operand, prefix,
//! or infix operator. Fold functions receive raw [`Spanned`](tokora::span::Spanned) tokens
//! and return computed results encoded back as [`Token::Num`].
//!
//! Operator table:
//!
//! | Syntax | Arity  | Assoc | Precedence |
//! |--------|--------|-------|------------|
//! | `+`    | infix  | left  | 1          |
//! | `-`    | infix  | left  | 1          |
//! | `*`    | infix  | left  | 2          |
//! | `/`    | infix  | left  | 2          |
//! | `-`    | prefix | —     | 3          |
//! | `^`    | infix  | right | 4          |

use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, SimpleSpan, Token as TokenT,
  emitter::PrattEmitter,
  error::{UnexpectedEoLhs, UnexpectedEoRhs},
  logos::{self, Logos},
  parser::{PrattInfix, PrattLHS, PrattPower, PrattRHS, Precedenced},
  span::Spanned,
  token::PrattToken,
};

// ── Lexer ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;

impl From<()> for LexError {
  fn from(_: ()) -> Self {
    LexError
  }
}

#[derive(Debug, Clone, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
enum Token {
  #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().parse::<f64>().map_err(|_| LexError))]
  Num(f64),
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("^")]
  Caret,
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
}

impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Token::Num(n) => write!(f, "{n}"),
      Token::Plus => write!(f, "+"),
      Token::Minus => write!(f, "-"),
      Token::Star => write!(f, "*"),
      Token::Slash => write!(f, "/"),
      Token::Caret => write!(f, "^"),
      Token::LParen => write!(f, "("),
      Token::RParen => write!(f, ")"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind {
  Num,
  Plus,
  Minus,
  Star,
  Slash,
  Caret,
  LParen,
  RParen,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokenKind::Num => write!(f, "number"),
      TokenKind::Plus => write!(f, "+"),
      TokenKind::Minus => write!(f, "-"),
      TokenKind::Star => write!(f, "*"),
      TokenKind::Slash => write!(f, "/"),
      TokenKind::Caret => write!(f, "^"),
      TokenKind::LParen => write!(f, "("),
      TokenKind::RParen => write!(f, ")"),
    }
  }
}

impl From<&Token> for TokenKind {
  fn from(t: &Token) -> Self {
    match t {
      Token::Num(_) => TokenKind::Num,
      Token::Plus => TokenKind::Plus,
      Token::Minus => TokenKind::Minus,
      Token::Star => TokenKind::Star,
      Token::Slash => TokenKind::Slash,
      Token::Caret => TokenKind::Caret,
      Token::LParen => TokenKind::LParen,
      Token::RParen => TokenKind::RParen,
    }
  }
}

impl TokenT<'_> for Token {
  type Kind = TokenKind;
  type Error = LexError;

  fn kind(&self) -> TokenKind {
    TokenKind::from(self)
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CalcError {
  Lex(LexError),
  UnexpectedToken,
  UnexpectedEot,
}

impl From<LexError> for CalcError {
  fn from(e: LexError) -> Self {
    CalcError::Lex(e)
  }
}

impl<'inp> From<tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError {
  fn from(_: tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self {
    CalcError::UnexpectedToken
  }
}

// The token-level bundle the entry points require: one `Set`-generic end-of-input impl covers
// both the default expected-set spelling and the `Kind`-table one the committed dispatch
// drivers raise, and one generic `FromUnclosed` covers every delimiter pair.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>>
  for CalcError
{
  fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self {
    CalcError::UnexpectedEot
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    CalcError::UnexpectedEot
  }
}

impl From<UnexpectedEoLhs> for CalcError {
  fn from(_: UnexpectedEoLhs) -> Self {
    CalcError::UnexpectedEot
  }
}

impl From<UnexpectedEoRhs> for CalcError {
  fn from(_: UnexpectedEoRhs) -> Self {
    CalcError::UnexpectedEot
  }
}

// ── Binding powers ────────────────────────────────────────────────────────────
//
// A newtype over i32 gives this expression grammar a named, domain-specific
// precedence ladder. Tokora already implements `PrattPower` for integer types.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Power(i32);

impl PrattPower for Power {}

// PREC_PAREN is below Power::default() (= Power(0)).
// When `(` is a prefix, pratt_in is called with min_power = Power(-1).
// `)` as a postfix with the same power satisfies power >= min_power inside
// that recursive call, so it is consumed there. In the outer call
// (min_power = Power(0)), Power(-1) < Power(0) so `)` is left alone.
const PREC_PAREN: Power = Power(-1); // ( )
const PREC_SUM: Power = Power(1); // + -
const PREC_PROD: Power = Power(2); // * /
const PREC_NEG: Power = Power(3); // unary -
const PREC_EXP: Power = Power(4); // ^

// ── PrattToken impl ────────────────────────────────────────────────────────────
//
// Classifies each token as an LHS (operand or prefix) or RHS (infix) participant.
// Returning `None` from either method tells the Pratt loop that the token is not
// part of the expression at that position, so it is left in the stream.

impl PrattToken<'_, f64, Power> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    Some(match self {
      Token::Num(_) => PrattLHS::Operand(()),
      Token::Minus => PrattLHS::Prefix(Precedenced::new((), PREC_NEG)),
      // `(` is a prefix with PREC_PAREN; the recursive pratt_in starts at
      // min_power = PREC_PAREN so that `)` (postfix with the same power) is
      // consumed inside that call rather than by the outer loop.
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
      Token::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), PREC_EXP)),
      // `)` is a postfix with PREC_PAREN; consumed only when min_power <= PREC_PAREN.
      Token::RParen => PrattRHS::Postfix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }
}

// ── Fold functions ─────────────────────────────────────────────────────────────
//
// Named functions (not closures) are used here because closures do not satisfy
// the higher-rank lifetime bound `for<'lt> FnMut(..., &'lt mut Emitter)` that
// the `PrattFoldToken*` blanket impls require. Function items are polymorphic
// over their lifetime parameters and satisfy the bound automatically.
//
// Computed f64 values are encoded back into Token::Num so the Spanned<Token, Span>
// result type used by the token-level API carries the result.

fn fold_prefix<E>(
  op: Spanned<Token, SimpleSpan>,
  operand: Spanned<Token, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
  let (span, op_tok) = op.into_components();
  match op_tok {
    Token::Minus => {
      let n = match operand.into_data() {
        Token::Num(n) => n,
        _ => unreachable!(),
      };
      Ok(Spanned::new(span, Token::Num(-n)))
    }
    Token::LParen => Ok(operand), // grouping: pass the inner result through
    _ => unreachable!(),
  }
}

fn fold_infix<E>(
  left: Spanned<Token, SimpleSpan>,
  right: Spanned<Token, SimpleSpan>,
  infix: Spanned<PrattInfix<Token, Token, Token>, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
  let (span, left_tok) = left.into_components();
  let l = match left_tok {
    Token::Num(n) => n,
    _ => unreachable!(),
  };
  let r = match right.into_data() {
    Token::Num(n) => n,
    _ => unreachable!(),
  };
  let op = match infix.into_data() {
    PrattInfix::Left(t) | PrattInfix::Right(t) | PrattInfix::Neither(t) => t,
  };
  let result = match op {
    Token::Plus => l + r,
    Token::Minus => l - r,
    Token::Star => l * r,
    Token::Slash => l / r,
    Token::Caret => l.powf(r),
    _ => unreachable!(),
  };
  Ok(Spanned::new(span, Token::Num(result)))
}

fn fold_postfix<E>(
  operand: Spanned<Token, SimpleSpan>,
  _op: Spanned<Token, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
  Ok(operand) // `)` consumed; pass the grouped result through
}

// ── Expression entry point ────────────────────────────────────────────────────

fn calc_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<f64, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, CalcLexer<'inp>, Error = CalcError> + PrattEmitter<'inp, CalcLexer<'inp>>,
{
  let result = inp.pratt::<_, _, _, f64, Power>(
    fold_prefix::<Ctx::Emitter>,
    fold_infix::<Ctx::Emitter>,
    fold_postfix::<Ctx::Emitter>,
  )?;

  match result {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => unreachable!(),
    },
    None => Err(CalcError::UnexpectedEot),
  }
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
  let cases: &[(&str, f64)] = &[
    ("1 + 2 * 3", 7.0),   // precedence: * before +        → 1 + (2*3)
    ("(1 + 2) * 3", 9.0), // parentheses override          → (1+2) * 3
    ("2 ^ 3 ^ 2", 512.0), // right-associative ^           → 2^(3^2) = 2^9
    ("-2 ^ 2", -4.0),     // unary minus < ^ in precedence → -(2^2)
    ("10 / 2 / 5", 1.0),  // left-associative /            → (10/2)/5
  ];

  for (src, expected) in cases {
    let result: f64 = Parser::new().apply(calc_expr).parse_str(src).unwrap();
    println!("{src:20} = {result:>6}  (expected {expected})");
    assert_eq!(result, *expected, "mismatch for `{src}`");
  }

  println!("All assertions passed.");
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
