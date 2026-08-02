# 12. Walkthrough: calculator

Prerequisites: chapters 5, 10, and 11.

This walkthrough builds the maintained
[`calculator.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/calculator.rs)
end to end, inline. It is a token-level Pratt evaluator: the parser classifies tokens, then folds
directly to an `f64` rather than allocating an expression AST.

[Chapter 5](super::ch05_pratt) *taught* the token-level Pratt engine with a plain-`i64` ladder;
this chapter is the maintained instantiation of that same engine, with two deliberate differences
worth watching for as they appear below: a **named `Power` newtype** for the precedence ladder
(instead of a bare integer) and **`f64`** arithmetic (so `^` is `powf` and folds can produce
fractional results). Every part — lexer, self-classifying token, folds, and the one-call entry
point — is shown as a compiling doctest, so you can follow the whole calculator without leaving
the page.

| Maintained program | Symbols to follow |
| --- | --- |
| [`calculator.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/calculator.rs) | `Token`, `TokenKind`, `Power`, `PrattToken`, `fold_prefix`, `fold_infix`, `fold_postfix`, `calc_expr` |

## Define token, kind, lexer alias, and `CalcError`

The enum carries numeric payloads in `Token::Num(f64)` and leaves classification to a separate
`TokenKind` (a fieldless enum, so it can be `Copy + Eq + Hash` as the [`Token`](crate::Token)
trait requires of its `Kind`). The program derives the Logos lexer, aliases it as `CalcLexer`, and
has one `CalcError` family for lexical errors, an unexpected token, and an unexpected end. The
`From` conversions are what let a generic `Ctx::Emitter` return the application error — including
the two Pratt-specific expression-end errors, [`UnexpectedEoLhs`](crate::error::UnexpectedEoLhs)
and [`UnexpectedEoRhs`](crate::error::UnexpectedEoRhs), that the token-level engine reports through
the emitter when an operator runs out of operand.

```rust
use tokora::{Token as TokenT, logos::{self, Logos}};

#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;
impl From<()> for LexError { fn from(_: ()) -> Self { Self } }

#[derive(Debug, Clone, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
enum Token {
  #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().parse::<f64>().map_err(|_| LexError))]
  Num(f64),
  #[token("+")] Plus,
  #[token("-")] Minus,
  #[token("*")] Star,
  #[token("/")] Slash,
  #[token("^")] Caret,
  #[token("(")] LParen,
  #[token(")")] RParen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind { Num, Plus, Minus, Star, Slash, Caret, LParen, RParen }

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Num => "number", Self::Plus => "+", Self::Minus => "-", Self::Star => "*",
      Self::Slash => "/", Self::Caret => "^", Self::LParen => "(", Self::RParen => ")",
    })
  }
}
impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

// Classification lives on a separate type; `kind()` just projects into it.
impl From<&Token> for TokenKind {
  fn from(t: &Token) -> Self {
    match t {
      Token::Num(_) => Self::Num, Token::Plus => Self::Plus, Token::Minus => Self::Minus,
      Token::Star => Self::Star, Token::Slash => Self::Slash, Token::Caret => Self::Caret,
      Token::LParen => Self::LParen, Token::RParen => Self::RParen,
    }
  }
}
impl TokenT<'_> for Token {
  type Kind = TokenKind;
  type Error = LexError;
  fn kind(&self) -> TokenKind { TokenKind::from(self) }
  fn is_trivia(&self) -> bool { false }
}

type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

#[derive(Debug)]
enum CalcError { Lex(LexError), UnexpectedToken, UnexpectedEot }

impl From<LexError> for CalcError { fn from(e: LexError) -> Self { Self::Lex(e) } }
impl<'inp> From<tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError {
  fn from(_: tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self { Self::UnexpectedToken }
}
// Both Pratt expression-end errors collapse to the same "ran out of input" variant.
impl From<tokora::error::UnexpectedEot> for CalcError { fn from(_: tokora::error::UnexpectedEot) -> Self { Self::UnexpectedEot } }
impl From<tokora::error::UnexpectedEoLhs> for CalcError { fn from(_: tokora::error::UnexpectedEoLhs) -> Self { Self::UnexpectedEot } }
impl From<tokora::error::UnexpectedEoRhs> for CalcError { fn from(_: tokora::error::UnexpectedEoRhs) -> Self { Self::UnexpectedEot } }
impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CalcError { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Self::UnexpectedEot } }
impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CalcError { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Self::UnexpectedEot } }

assert_eq!(Token::Star.kind(), TokenKind::Star);
assert_eq!(Token::Num(1.5).kind(), TokenKind::Num);
```

The relevant public APIs are `Token`, [`token::PrattToken`](crate::token::PrattToken),
[`parser::PrattPower`](crate::parser::PrattPower),
[`parser::PrattLHS`](crate::parser::PrattLHS),
[`parser::PrattRHS`](crate::parser::PrattRHS),
[`parser::Precedenced`](crate::parser::Precedenced),
[`parser::PrattInfix`](crate::parser::PrattInfix), [`InputRef::pratt`](crate::InputRef::pratt),
`PrattEmitter`, `Spanned`, `Parser`, and `Parse::parse_str`.

## Define the precedence constants and grouping sentinel

`Power(i32)` names this language's ladder. It is useful for making the domain explicit, not for
orphan-rule reasons: Tokora implements [`PrattPower`](crate::parser::PrattPower) for the standard
integer types too, so a bare `i64` (chapter 5's choice) would also work. The grouping sentinel is
below the default floor so an opening parenthesis can recurse at that lower floor and consume its
matching closing parenthesis without exposing it to the outer expression.

```rust
use tokora::parser::PrattPower;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Power(i32);

// Nothing to implement: a binding power is only ever compared, never stepped.
impl PrattPower for Power {}

const PREC_PAREN: Power = Power(-1); // ( )       — below the floor
const PREC_SUM: Power = Power(1);    // + -
const PREC_PROD: Power = Power(2);   // * /
const PREC_NEG: Power = Power(3);    // unary -
const PREC_EXP: Power = Power(4);    // ^

// A top-level parse starts at the default floor.
assert_eq!(Power::default(), Power(0));
// `(` sits *below* that floor, so a stray `)` is invisible at the top level and left for the
// caller — but consumable inside the recursive call a `(` prefix opens (whose floor is PREC_PAREN).
assert!(PREC_PAREN < Power::default());
// Associativity is how strictly the engine compares against a level, not a move along the
// ladder: after a left-associative `+` the recursion admits only powers strictly above
// PREC_SUM, so `*` gets in and another `+` does not.
assert!(PREC_PROD > PREC_SUM);
assert!(PREC_NEG < PREC_EXP);
```

## Implement `try_pratt_lhs` and `try_pratt_rhs`

The [`PrattToken`](crate::token::PrattToken) implementation turns that ladder into the engine's
classifier: the token type describes *itself* at each position. `try_pratt_lhs` accepts a number
(an operand), a prefix minus, or an opening parenthesis; `try_pratt_rhs` accepts the infix
operators and the closing-parenthesis postfix sentinel. Returning `None` tells the engine that the
token is not part of this expression here, so it is left on the input and the loop stops. `^` is
the one [`Right`](crate::parser::PrattInfix)-associative row; `(`/`)` share `PREC_PAREN`.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Debug, Clone, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Token {
#   #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().parse::<f64>().map_err(|_| LexError))] Num(f64),
#   #[token("+")] Plus, #[token("-")] Minus, #[token("*")] Star, #[token("/")] Slash,
#   #[token("^")] Caret, #[token("(")] LParen, #[token(")")] RParen,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokenKind { Num, Plus, Minus, Star, Slash, Caret, LParen, RParen }
# impl core::fmt::Display for TokenKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Num => "number", Self::Plus => "+", Self::Minus => "-", Self::Star => "*", Self::Slash => "/", Self::Caret => "^", Self::LParen => "(", Self::RParen => ")" })
#   }
# }
# impl core::fmt::Display for Token { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) } }
# impl From<&Token> for TokenKind {
#   fn from(t: &Token) -> Self { match t { Token::Num(_) => Self::Num, Token::Plus => Self::Plus, Token::Minus => Self::Minus, Token::Star => Self::Star, Token::Slash => Self::Slash, Token::Caret => Self::Caret, Token::LParen => Self::LParen, Token::RParen => Self::RParen } }
# }
# impl TokenT<'_> for Token {
#   type Kind = TokenKind; type Error = LexError;
#   fn kind(&self) -> TokenKind { TokenKind::from(self) }
#   fn is_trivia(&self) -> bool { false }
# }
# use tokora::parser::PrattPower;
# #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
# struct Power(i32);
# impl PrattPower for Power {}
# const PREC_PAREN: Power = Power(-1);
# const PREC_SUM: Power = Power(1);
# const PREC_PROD: Power = Power(2);
# const PREC_NEG: Power = Power(3);
# const PREC_EXP: Power = Power(4);
use tokora::parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced};
use tokora::token::PrattToken;

impl PrattToken<'_, f64, Power> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    Some(match self {
      Token::Num(_) => PrattLHS::Operand(()),
      Token::Minus => PrattLHS::Prefix(Precedenced::new((), PREC_NEG)),
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
      // The one right-associative row: `2 ^ 3 ^ 2` groups as `2 ^ (3 ^ 2)`.
      Token::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), PREC_EXP)),
      // `)` is a postfix at PREC_PAREN, consumed only inside the group `(` opened.
      Token::RParen => PrattRHS::Postfix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }
}

// A number is an operand; `-` and `(` open the left edge; everything else declines.
assert!(matches!(Token::Num(1.0).try_pratt_lhs(), Some(PrattLHS::Operand(()))));
assert!(matches!(Token::Minus.try_pratt_lhs(), Some(PrattLHS::Prefix(_))));
assert!(Token::Plus.try_pratt_lhs().is_none());
// `^` is an infix, `)` a postfix; a bare number has no right-hand-side role.
assert!(matches!(Token::Caret.try_pratt_rhs(), Some(PrattRHS::Infix(_))));
assert!(matches!(Token::RParen.try_pratt_rhs(), Some(PrattRHS::Postfix(_))));
assert!(Token::Num(1.0).try_pratt_rhs().is_none());
```

## Implement the named prefix, infix, and postfix folds

Use named functions rather than closures because the token-level fold traits require a
higher-ranked lifetime bound on the emitter (`for<'lt> FnMut(…, &'lt mut Emitter)`); a closure is
monomorphic in that lifetime and does not satisfy it, while a `fn` item is generic over its
lifetimes and satisfies it for free. `fold_prefix` negates a number or passes a grouped value
through; `fold_infix` extracts the operator from [`PrattInfix`](crate::parser::PrattInfix) and
computes the next `f64` (here `^` is `powf`); `fold_postfix` acknowledges a closing parenthesis and
returns its operand. Each fold trades in [`Spanned`](crate::span::Spanned)`<Token>`, so the
evaluated value goes back in as a `Token::Num`. The emitter parameter is unused here, so the folds
can even be exercised directly with `E = ()`:

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Debug, Clone, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Token {
#   #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().parse::<f64>().map_err(|_| LexError))] Num(f64),
#   #[token("+")] Plus, #[token("-")] Minus, #[token("*")] Star, #[token("/")] Slash,
#   #[token("^")] Caret, #[token("(")] LParen, #[token(")")] RParen,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokenKind { Num, Plus, Minus, Star, Slash, Caret, LParen, RParen }
# impl core::fmt::Display for TokenKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Num => "number", Self::Plus => "+", Self::Minus => "-", Self::Star => "*", Self::Slash => "/", Self::Caret => "^", Self::LParen => "(", Self::RParen => ")" })
#   }
# }
# impl core::fmt::Display for Token { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) } }
# impl From<&Token> for TokenKind {
#   fn from(t: &Token) -> Self { match t { Token::Num(_) => Self::Num, Token::Plus => Self::Plus, Token::Minus => Self::Minus, Token::Star => Self::Star, Token::Slash => Self::Slash, Token::Caret => Self::Caret, Token::LParen => Self::LParen, Token::RParen => Self::RParen } }
# }
# impl TokenT<'_> for Token {
#   type Kind = TokenKind; type Error = LexError;
#   fn kind(&self) -> TokenKind { TokenKind::from(self) }
#   fn is_trivia(&self) -> bool { false }
# }
# type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;
# #[derive(Debug)]
# enum CalcError { Lex(LexError), UnexpectedToken, UnexpectedEot }
# impl From<LexError> for CalcError { fn from(e: LexError) -> Self { Self::Lex(e) } }
# impl<'inp> From<tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError { fn from(_: tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self { Self::UnexpectedToken } }
# impl From<tokora::error::UnexpectedEot> for CalcError { fn from(_: tokora::error::UnexpectedEot) -> Self { Self::UnexpectedEot } }
# impl From<tokora::error::UnexpectedEoLhs> for CalcError { fn from(_: tokora::error::UnexpectedEoLhs) -> Self { Self::UnexpectedEot } }
# impl From<tokora::error::UnexpectedEoRhs> for CalcError { fn from(_: tokora::error::UnexpectedEoRhs) -> Self { Self::UnexpectedEot } }
impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CalcError { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Self::UnexpectedEot } }
impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CalcError { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Self::UnexpectedEot } }
use tokora::{SimpleSpan, parser::PrattInfix, span::Spanned};

fn fold_prefix<E>(
  op: Spanned<Token, SimpleSpan>,
  operand: Spanned<Token, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
  let (span, op) = op.into_components();
  match op {
    Token::Minus => {
      let n = match operand.into_data() { Token::Num(n) => n, _ => unreachable!() };
      Ok(Spanned::new(span, Token::Num(-n)))
    }
    // Grouping: the `(` prefix's "operand" is the whole parenthesised expression, already folded
    // by the inner call (which also ate the `)`). Pass it through untouched.
    Token::LParen => Ok(operand),
    _ => unreachable!("the LHS table admits only `-` and `(` as prefixes"),
  }
}

fn fold_infix<E>(
  left: Spanned<Token, SimpleSpan>,
  right: Spanned<Token, SimpleSpan>,
  infix: Spanned<PrattInfix<Token, Token, Token>, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
  let (span, left_tok) = left.into_components();
  let l = match left_tok { Token::Num(n) => n, _ => unreachable!() };
  let r = match right.into_data() { Token::Num(n) => n, _ => unreachable!() };
  // Associativity has already done its job in the engine; the fold just wants the operator.
  let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) = infix.into_data();
  let value = match op {
    Token::Plus => l + r,
    Token::Minus => l - r,
    Token::Star => l * r,
    Token::Slash => l / r,
    Token::Caret => l.powf(r),
    _ => unreachable!("the RHS table admits only the five arithmetic infixes"),
  };
  Ok(Spanned::new(span, Token::Num(value)))
}

fn fold_postfix<E>(
  operand: Spanned<Token, SimpleSpan>,
  _close: Spanned<Token, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
  Ok(operand) // `)` closed its group; the value flows on
}

// The folds are pure arithmetic over `Spanned<Token>`, so they run with no parser context: pick
// `E = ()` and pass `&mut ()`.
let span = SimpleSpan::new(0, 0);
let sum = fold_infix::<()>(
  Spanned::new(span, Token::Num(2.0)),
  Spanned::new(span, Token::Num(3.0)),
  Spanned::new(span, PrattInfix::Left(Token::Star)),
  &mut (),
).unwrap();
assert!(matches!(sum.into_data(), Token::Num(n) if n == 6.0));

let neg = fold_prefix::<()>(
  Spanned::new(span, Token::Minus),
  Spanned::new(span, Token::Num(2.0)),
  &mut (),
).unwrap();
assert!(matches!(neg.into_data(), Token::Num(n) if n == -2.0));

let grouped = fold_postfix::<()>(
  Spanned::new(span, Token::Num(9.0)),
  Spanned::new(span, Token::RParen),
  &mut (),
).unwrap();
assert!(matches!(grouped.into_data(), Token::Num(n) if n == 9.0));
```

## Build `calc_expr`

`calc_expr` calls [`InputRef::pratt`](crate::InputRef::pratt) with the three folds, then unwraps
the final `Token::Num`. The turbofish fixes the two type parameters the engine cannot infer:
`Expr = f64` (what an expression *means*) and `Power` (how tightly things bind). Its `Ctx` bounds
add [`PrattEmitter`](crate::emitter::PrattEmitter) to the ordinary
[`Emitter`](crate::Emitter) bound because Pratt-specific diagnostics travel through the emitter
too; a [`FatalContext`](crate::FatalContext) satisfies both with no extra work. The five
assertions below are the maintained evaluator's behavior contract, now executable inline:

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Debug, Clone, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Token {
#   #[regex(r"[0-9]+(\.[0-9]+)?", |lex| lex.slice().parse::<f64>().map_err(|_| LexError))] Num(f64),
#   #[token("+")] Plus, #[token("-")] Minus, #[token("*")] Star, #[token("/")] Slash,
#   #[token("^")] Caret, #[token("(")] LParen, #[token(")")] RParen,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokenKind { Num, Plus, Minus, Star, Slash, Caret, LParen, RParen }
# impl core::fmt::Display for TokenKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Num => "number", Self::Plus => "+", Self::Minus => "-", Self::Star => "*", Self::Slash => "/", Self::Caret => "^", Self::LParen => "(", Self::RParen => ")" })
#   }
# }
# impl core::fmt::Display for Token { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) } }
# impl From<&Token> for TokenKind {
#   fn from(t: &Token) -> Self { match t { Token::Num(_) => Self::Num, Token::Plus => Self::Plus, Token::Minus => Self::Minus, Token::Star => Self::Star, Token::Slash => Self::Slash, Token::Caret => Self::Caret, Token::LParen => Self::LParen, Token::RParen => Self::RParen } }
# }
# impl TokenT<'_> for Token {
#   type Kind = TokenKind; type Error = LexError;
#   fn kind(&self) -> TokenKind { TokenKind::from(self) }
#   fn is_trivia(&self) -> bool { false }
# }
# type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;
# #[derive(Debug)]
# enum CalcError { Lex(LexError), UnexpectedToken, UnexpectedEot }
# impl From<LexError> for CalcError { fn from(e: LexError) -> Self { Self::Lex(e) } }
# impl<'inp> From<tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError { fn from(_: tokora::error::token::UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self { Self::UnexpectedToken } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>> for CalcError { fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::UnexpectedEot } }
# impl From<tokora::error::UnexpectedEoLhs> for CalcError { fn from(_: tokora::error::UnexpectedEoLhs) -> Self { Self::UnexpectedEot } }
# impl From<tokora::error::UnexpectedEoRhs> for CalcError { fn from(_: tokora::error::UnexpectedEoRhs) -> Self { Self::UnexpectedEot } }
impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CalcError { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Self::UnexpectedEot } }
impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CalcError { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Self::UnexpectedEot } }
# use tokora::parser::PrattPower;
# #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
# struct Power(i32);
# impl PrattPower for Power {}
# const PREC_PAREN: Power = Power(-1);
# const PREC_SUM: Power = Power(1);
# const PREC_PROD: Power = Power(2);
# const PREC_NEG: Power = Power(3);
# const PREC_EXP: Power = Power(4);
# use tokora::parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced};
# use tokora::token::PrattToken;
# impl PrattToken<'_, f64, Power> for Token {
#   fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
#     Some(match self {
#       Token::Num(_) => PrattLHS::Operand(()),
#       Token::Minus => PrattLHS::Prefix(Precedenced::new((), PREC_NEG)),
#       Token::LParen => PrattLHS::Prefix(Precedenced::new((), PREC_PAREN)),
#       _ => return None,
#     })
#   }
#   fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>> {
#     Some(match self {
#       Token::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
#       Token::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
#       Token::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
#       Token::Slash => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
#       Token::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), PREC_EXP)),
#       Token::RParen => PrattRHS::Postfix(Precedenced::new((), PREC_PAREN)),
#       _ => return None,
#     })
#   }
# }
# use tokora::{SimpleSpan, span::Spanned};
# fn fold_prefix<E>(op: Spanned<Token, SimpleSpan>, operand: Spanned<Token, SimpleSpan>, _: &mut E) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
#   let (span, op) = op.into_components();
#   match op {
#     Token::Minus => { let n = match operand.into_data() { Token::Num(n) => n, _ => unreachable!() }; Ok(Spanned::new(span, Token::Num(-n))) }
#     Token::LParen => Ok(operand),
#     _ => unreachable!(),
#   }
# }
# fn fold_infix<E>(left: Spanned<Token, SimpleSpan>, right: Spanned<Token, SimpleSpan>, infix: Spanned<PrattInfix<Token, Token, Token>, SimpleSpan>, _: &mut E) -> Result<Spanned<Token, SimpleSpan>, CalcError> {
#   let (span, left_tok) = left.into_components();
#   let l = match left_tok { Token::Num(n) => n, _ => unreachable!() };
#   let r = match right.into_data() { Token::Num(n) => n, _ => unreachable!() };
#   let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) = infix.into_data();
#   let value = match op { Token::Plus => l + r, Token::Minus => l - r, Token::Star => l * r, Token::Slash => l / r, Token::Caret => l.powf(r), _ => unreachable!() };
#   Ok(Spanned::new(span, Token::Num(value)))
# }
# fn fold_postfix<E>(operand: Spanned<Token, SimpleSpan>, _close: Spanned<Token, SimpleSpan>, _: &mut E) -> Result<Spanned<Token, SimpleSpan>, CalcError> { Ok(operand) }
use tokora::{Emitter, InputRef, Parse, ParseContext, Parser, emitter::PrattEmitter};

fn calc_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<f64, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, CalcLexer<'inp>, Error = CalcError> + PrattEmitter<'inp, CalcLexer<'inp>>,
{
  let folded = inp.pratt::<_, _, _, f64, Power>(
    fold_prefix::<Ctx::Emitter>,
    fold_infix::<Ctx::Emitter>,
    fold_postfix::<Ctx::Emitter>,
  )?;
  // `Ok(None)` means the cursor was not looking at an expression at all.
  match folded {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => unreachable!(),
    },
    None => Err(CalcError::UnexpectedEot),
  }
}

let eval = |src| Parser::new().apply(calc_expr).parse_str(src);

# // `^` folds through `f64::powf`, and Miri intentionally perturbs transcendental floats to catch
# // code that assumes bit-exact results across platforms — so this table compares with a small
# // epsilon instead of `assert_eq!`. The taught values themselves are unchanged.
# fn assert_close(got: f64, want: f64) {
#   assert!((got - want).abs() < 1e-9, "{got} != {want}");
# }
assert_close(eval("1 + 2 * 3").unwrap(), 7.0);   // `*` binds tighter than `+`  → 1 + (2 * 3)
assert_close(eval("(1 + 2) * 3").unwrap(), 9.0); // grouping overrides          → (1 + 2) * 3
assert_close(eval("2 ^ 3 ^ 2").unwrap(), 512.0); // `^` is RIGHT-assoc          → 2 ^ (3 ^ 2)
assert_close(eval("-2 ^ 2").unwrap(), -4.0);     // `^` outranks unary `-`      → -(2 ^ 2)
assert_close(eval("10 / 2 / 5").unwrap(), 1.0);  // `/` is left-assoc           → (10 / 2) / 5
```

## Reproduce the maintained assertion table

The assertions above *are* the maintained binary's assertion table: precedence (`1 + 2 * 3`),
parentheses, right-associative `2 ^ 3 ^ 2`, unary minus versus exponentiation, and
left-associative division. They are the behavior contract for the evaluator. For the full runnable
program — the same code driven from a `main` that prints each result — run:

```sh
cargo run -p tokora --example calculator --features logos
```

You have now followed the complete calculator inline: a Logos lexer, a self-classifying
`PrattToken`, three named folds, and a one-call `calc_expr`, evaluating real expressions to `f64`
with no AST allocated. Next: [chapter 13](super::ch13_s_expression_example).
