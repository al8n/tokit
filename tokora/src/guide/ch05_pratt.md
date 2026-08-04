# 5. Expressions: Pratt parsing

Calc's statements are done; its expressions are still bare integers. Expression grammars are
the one place where plain recursive descent gets ugly: the textbook shape is one function per
precedence level (`expr → term → factor → atom`), so every new operator level costs another
function and another layer of calls, and right-associativity and prefix operators need
hand-written special cases at each rung.

**Pratt parsing** (precedence climbing) replaces the ladder of functions with a single loop
plus a table: each operator carries a *binding power*, and the loop keeps consuming operators
while their power clears the current floor. One loop, any number of levels, and a new operator
is a new table row rather than new code.

tokora has two Pratt surfaces:

- **token-level** — [`InputRef::pratt`](crate::InputRef::pratt), used in this chapter. The
  *token type itself* classifies each token via [`PrattToken`](crate::token::PrattToken), and
  the folds map tokens to tokens. This is the shape to reach for when the expression's value
  is itself expressible as a token — a calculator that folds `1 + 2` into `Int(3)`.
- **AST-level** — the [`pratt`](crate::parser::pratt)
  combinator. You supply LHS/RHS sub-parsers and folds over *your own node type*, so the
  result is a tree. That is what a full Calc — the one whose expressions include variables,
  and which therefore cannot fold to a number during the parse — would use.

Both run the same engine; only the currency of the folds differs.

## The power ladder

| Syntax  | Position         | Associativity | Power |
|---------|------------------|---------------|-------|
| `( )`   | prefix + postfix | —             | `-1`  |
| `+` `-` | infix            | left          | `1`   |
| `*` `/` | infix            | left          | `2`   |
| `-`     | prefix           | —             | `3`   |
| `^`     | infix            | **right**     | `4`   |

Two of those rows carry the chapter's whole design.

**Associativity is how strict the floor is, not a special case.** After a left-associative
operator the engine recurses with a floor that admits only powers *strictly greater* than the
operator's own, so an equal-power operator to the right does *not* clear the inner floor and
folds into the outer call instead — `10 / 2 / 5` groups as `(10 / 2) / 5`. A right-associative
operator recurses with a floor that admits its own power too, so the equal-power operator
*does* clear it and is consumed by the inner call: `2 ^ 3 ^ 2` groups as `2 ^ (3 ^ 2)` = 512.
You write [`PrattInfix::Left`](crate::parser::PrattInfix) or `PrattInfix::Right`; picking the
strictness is the engine's job. Note what it never does: step to a neighbouring level. A
[`PrattPower`](crate::parser::PrattPower) is only ever compared, so the rule holds at the ends
of the ladder, where `power ± 1` would have run out of room and silently swapped the two
behaviours.

**Grouping is an operator pair below the floor.** `(` is a *prefix* operator at power `-1` and
`)` is a *postfix* operator at the same power. A top-level parse starts at the default floor
(`0`, for an integer power), so a stray `)` there is *below* the floor: the loop leaves it on
the input for the surrounding grammar. But the recursive call inside a `(` prefix runs with a
floor of `-1`, and there `)` clears the floor and is consumed — closing exactly its own group.
No bracket-matching code and no depth counter: the precedence rule already says it.

## Binding powers are plain integers

`Power` defaults to `i64`, and tokora implements [`PrattPower`](crate::parser::PrattPower) for
every standard integer type. Write `1`, `2`, `-1` and move on. A newtype is still welcome when
you want *named* levels and a type-checked ladder — the trait is public, and it asks for
nothing beyond `Default + Clone + Ord` — but nothing forces one on you.

## The folds must be named functions

The fold parameters are bound by `for<'lt> FnMut(…, &'lt mut Emitter)` — a higher-ranked
bound. A closure is monomorphic in its argument lifetimes and does **not** satisfy it; what
you get is a mismatched-types error mentioning a `for<'lt>` signature, which is baffling if
you do not know what you are looking at. Function *items* are generic over their lifetime
parameters and satisfy the bound for free. So: write the folds as `fn`s. Their shapes — mind
the argument order, the operator comes *last* for infix and postfix but *first* for prefix:

```text
fn fold_prefix (operator, operand,                  EmitterView) -> Result<Spanned<Tok, Span>, Error>
fn fold_infix  (left,     right,    infix_operator, EmitterView) -> Result<Spanned<Tok, Span>, Error>
fn fold_postfix(operand,  operator,                 EmitterView) -> Result<Spanned<Tok, Span>, Error>
```

## Calc's expression engine

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# use tokora::EmitterView;
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, PartialEq, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
#   Int(i64),
#   #[token("let")] Let,
#   #[token("print")] Print,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[token("+")] Plus,
#   #[token("-")] Minus,
#   #[token("*")] Star,
#   #[token("/")] Slash,
#   #[token("^")] Caret,
#   #[token("=")] Assign,
#   #[token(";")] Semi,
#   #[token(",")] Comma,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokKind { Int, Let, Print, Ident, Plus, Minus, Star, Slash, Caret, Assign, Semi, Comma, LParen, RParen }
# impl core::fmt::Display for TokKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Self::Int => "integer", Self::Let => "`let`", Self::Print => "`print`",
#       Self::Ident => "identifier", Self::Plus => "`+`", Self::Minus => "`-`",
#       Self::Star => "`*`", Self::Slash => "`/`", Self::Caret => "`^`",
#       Self::Assign => "`=`", Self::Semi => "`;`", Self::Comma => "`,`",
#       Self::LParen => "`(`", Self::RParen => "`)`",
#     })
#   }
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     match self {
#       Tok::Int(n) => write!(f, "{n}"),
#       other => core::fmt::Display::fmt(&other.kind(), f),
#     }
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = TokKind;
#   type Error = LexError;
#   fn kind(&self) -> TokKind {
#     match self {
#       Tok::Int(_) => TokKind::Int, Tok::Let => TokKind::Let, Tok::Print => TokKind::Print,
#       Tok::Ident => TokKind::Ident, Tok::Plus => TokKind::Plus, Tok::Minus => TokKind::Minus,
#       Tok::Star => TokKind::Star, Tok::Slash => TokKind::Slash, Tok::Caret => TokKind::Caret,
#       Tok::Assign => TokKind::Assign, Tok::Semi => TokKind::Semi, Tok::Comma => TokKind::Comma,
#       Tok::LParen => TokKind::LParen, Tok::RParen => TokKind::RParen,
#     }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# use tokora::error::{UnexpectedEnd, token::UnexpectedToken};
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CalcError {
#   fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CalcError {
#   fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, SimpleSpan,
  emitter::PrattEmitter,
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced},
  span::Spanned,
  token::PrattToken,
};

// ── The ladder. Plain `i64`s — no newtype, because `PrattPower` is implemented for the
//    integers. The default floor is `i64::default()` = 0, and PREC_PAREN sits *below* it:
//    that is what makes `)` invisible at the top level and consumable inside a group.

const PREC_PAREN: i64 = -1; // ( )
const PREC_SUM: i64 = 1; //    + -
const PREC_PROD: i64 = 2; //   * /
const PREC_NEG: i64 = 3; //    unary -
const PREC_EXP: i64 = 4; //    ^

// ── The table, written as an impl on the token: each token says what it is at each
//    position. `None` means "not part of an expression here", so the token is left on the
//    input — which is exactly how the engine knows to stop at `;` or `,`.

impl PrattToken<'_, i64> for Tok {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), i64>> {
    Some(match self {
      Tok::Int(_) => PrattLHS::Operand(()),
      Tok::Minus => PrattLHS::Prefix(Precedenced::new((), PREC_NEG)),
      Tok::LParen => PrattLHS::Prefix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), i64>> {
    Some(match self {
      Tok::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
      Tok::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
      Tok::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
      Tok::Slash => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
      // The one right-associative row in the table.
      Tok::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), PREC_EXP)),
      Tok::RParen => PrattRHS::Postfix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }
}

// ── The folds. Named `fn`s, not closures. The token-level API's currency is
//    `Spanned<Tok, Span>`, so a computed value goes back in as a `Tok::Int`.

fn fold_prefix<'inp, E>(
  op: Spanned<Tok, SimpleSpan>,
  operand: Spanned<Tok, SimpleSpan>,
  _: EmitterView<'_, 'inp, CalcLexer<'inp>, E>,
) -> Result<Spanned<Tok, SimpleSpan>, CalcError> {
  let (span, op) = op.into_components();
  match op {
    Tok::Minus => Ok(Spanned::new(span, Tok::Int(-int(operand)?))),
    // Grouping: the `(` prefix's "operand" is the whole parenthesised expression, already
    // folded by the inner call (which also ate the `)`). Pass it through untouched.
    Tok::LParen => Ok(operand),
    _ => unreachable!("the LHS table admits only `-` and `(` as prefixes"),
  }
}

fn fold_infix<'inp, E>(
  left: Spanned<Tok, SimpleSpan>,
  right: Spanned<Tok, SimpleSpan>,
  infix: Spanned<PrattInfix<Tok, Tok, Tok>, SimpleSpan>,
  _: EmitterView<'_, 'inp, CalcLexer<'inp>, E>,
) -> Result<Spanned<Tok, SimpleSpan>, CalcError> {
  let span = left.span();
  let (l, r) = (int(left)?, int(right)?);
  // The associativity has already done its job in the engine; the fold just wants the token.
  let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) =
    infix.into_data();
  let value = match op {
    Tok::Plus => l + r,
    Tok::Minus => l - r,
    Tok::Star => l * r,
    // The folds are fallible on purpose. A grown-up Calc would add a `DivByZero` variant
    // rather than reuse `Unexpected`, but the shape is the same: an `Err` out of a fold
    // aborts the expression.
    Tok::Slash => l.checked_div(r).ok_or(CalcError::Unexpected)?,
    Tok::Caret => u32::try_from(r)
      .ok()
      .and_then(|e| l.checked_pow(e))
      .ok_or(CalcError::Unexpected)?,
    _ => unreachable!("the RHS table admits only the five arithmetic infixes"),
  };
  Ok(Spanned::new(span, Tok::Int(value)))
}

fn fold_postfix<'inp, E>(
  operand: Spanned<Tok, SimpleSpan>,
  _close: Spanned<Tok, SimpleSpan>,
  _: EmitterView<'_, 'inp, CalcLexer<'inp>, E>,
) -> Result<Spanned<Tok, SimpleSpan>, CalcError> {
  Ok(operand) // `)` closed its group; the value flows on.
}

/// Unwrap a folded operand back to its integer.
fn int(tok: Spanned<Tok, SimpleSpan>) -> Result<i64, CalcError> {
  match tok.into_data() {
    Tok::Int(n) => Ok(n),
    _ => Err(CalcError::Unexpected),
  }
}

// ── The entry point: one call, the whole expression grammar.

fn calc_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<i64, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, CalcLexer<'inp>, Error = CalcError> + PrattEmitter<'inp, CalcLexer<'inp>>,
{
  // `Expr` = i64 (what an expression *means*); `Power` = i64 (how tightly things bind).
  let folded = inp.pratt::<_, _, _, i64, i64>(
    fold_prefix::<Ctx::Emitter>,
    fold_infix::<Ctx::Emitter>,
    fold_postfix::<Ctx::Emitter>,
  )?;
  // `Ok(None)` means the cursor was not looking at an expression at all.
  match folded {
    Some(tok) => int(tok),
    None => Err(CalcError::UnexpectedEnd),
  }
}

let eval = |src| Parser::new().apply(calc_expr).parse_str(src);

assert_eq!(eval("1 + 2 * 3"), Ok(7)); //     `*` outranks `+`       → 1 + (2 * 3)
assert_eq!(eval("(1 + 2) * 3"), Ok(9)); //   grouping overrides     → (1 + 2) * 3
assert_eq!(eval("10 / 2 / 5"), Ok(1)); //    `/` is left-assoc      → (10 / 2) / 5
assert_eq!(eval("2 ^ 3 ^ 2"), Ok(512)); //   `^` is RIGHT-assoc     → 2 ^ (3 ^ 2)
assert_eq!(eval("-(1 + 2)"), Ok(-3)); //     prefix over a group
assert_eq!(eval("-2 ^ 2"), Ok(-4)); //       `^` outranks unary `-` → -(2 ^ 2)

// Nothing here is expression-shaped: the engine consumes nothing and says so.
assert_eq!(eval(";"), Err(CalcError::UnexpectedEnd));

// ── And it slots straight into the statement grammar. ──
# fn expect_tok<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
#   want: fn(&Tok) -> bool,
# ) -> Result<(), CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   if inp.try_expect(|t| want(t.data()))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   Ok(())
# }
// (Hidden here: `expect_tok`, chapter 2's one-token helper.)

fn parse_let<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<(&'inp str, i64), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, CalcLexer<'inp>, Error = CalcError> + PrattEmitter<'inp, CalcLexer<'inp>>,
{
  expect_tok(inp, |t| matches!(t, Tok::Let))?;
  expect_tok(inp, |t| matches!(t, Tok::Ident))?;
  let name = inp.slice();
  expect_tok(inp, |t| matches!(t, Tok::Assign))?;
  // The engine stops at `;` by itself: `Semi` has no RHS table entry, so `try_pratt_rhs`
  // returns `None` and the operator loop ends with the token still on the input.
  let value = calc_expr(inp)?;
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok((name, value))
}

let binding = Parser::new()
  .apply(parse_let)
  .parse_str("let x = -2 + 3 * (4 + 1) ;")
  .unwrap();
assert_eq!(binding, ("x", 13));
```

## A floor of your own

[`pratt`](crate::InputRef::pratt) starts at `Power::default()`.
[`pratt_with_min_precedence`](crate::InputRef::pratt_with_min_precedence) lets you name the
floor instead — parse only what binds at least as tightly as some level and leave the rest to
the caller's loop. It is the same knob the `(` prefix turns; here you turn it by hand.

Calc parses and evaluates real expressions now. Everything so far has been *deterministic*:
one look at the next token decides everything, and no parser ever un-does work. The next
chapter is about the cases where you genuinely must try something and be able to take it back.
Next: [chapter 6](super::ch06_backtracking).
