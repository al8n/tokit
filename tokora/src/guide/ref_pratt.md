# Reference: Pratt (precedence) parsing

[Chapter 5](super::ch05_pratt) *teaches* Pratt parsing — one loop plus a precedence table in
place of the recursive-descent ladder — and works a calculator end to end. This chapter is the
**catalog**: every type, trait, method, and error in the Pratt surface, each with its real
signature and a compact compiling use. Reach here to look an item up; reach for
[chapter 5](super::ch05_pratt) (token-level) and [chapter 15](super::ch15_c_expression_example)
(AST-level) for the guided builds.

## Two surfaces, one engine

tokora exposes Pratt parsing at two altitudes. Both run the same precedence-climbing loop; they
differ only in the *currency* the folds trade in.

| | **Token-level** | **AST-level** |
|---|---|---|
| Entry | [`InputRef::pratt`](crate::InputRef::pratt) / [`pratt_with_min_precedence`](crate::InputRef::pratt_with_min_precedence) | [`pratt`](crate::parser::pratt) → [`Pratt`](crate::parser::Pratt) |
| Classifier | [`PrattToken`](crate::token::PrattToken) on the token type | `parse_lhs` / `parse_rhs` sub-parsers |
| Fold currency | `Spanned<Token, Span>` → `Spanned<Token, Span>` | your node type `O` → `O` |
| Result | `Option<Spanned<Token, Span>>` | `O` |
| Extra emitter capability | [`PrattEmitter`](crate::emitter::PrattEmitter) | none (folds hold the `InputRef`) |
| CST | unsupported (synthetic tokens) | [`with_cst_kinds`](crate::parser::Pratt::with_cst_kinds) |
| Worked example | [ch05](super::ch05_pratt) | [ch15](super::ch15_c_expression_example) |

Reach for the token-level API when an expression's value is itself a token — a calculator that
folds `1 + 2` into `Int(3)`. Reach for the AST-level API when the result is a tree over your own
node type.

## Folds are `fn` items, not closures

Every fold parameter — on both surfaces — is bound by a *higher-ranked* `FnMut` (the emitter
borrow on the token folds, the `InputRef`'s inner lifetime on the AST folds are each `for<'lt>`).
A closure is monomorphic in those lifetimes and does **not** satisfy the bound; the error is a
mismatched-types complaint mentioning a `for<'lt>` signature. Function *items* are generic over
their lifetime parameters and satisfy it for free. **Write the folds as named `fn`s.** Every
example below does.

---

## Binding power: `PrattPower`

The precedence of an operator. tokora implements it for every standard integer type (saturating
at the bounds, so [`prev`](crate::parser::PrattPower::prev) on the minimum cannot underflow), so
a plain `i64` — the default `Power` throughout — works with no newtype. Implement it yourself
when you want *named* levels and a type-checked ladder.

```text
trait PrattPower: Default + Clone + Ord {
    fn next(&self) -> Self;   // one level tighter
    fn prev(&self) -> Self;   // one level looser (use saturating subtraction)
}
```

```rust
use tokora::parser::PrattPower;

assert_eq!(3i64.next(), 4);
assert_eq!(3i64.prev(), 2);
// Saturating at the representable bounds — never wraps, never panics.
assert_eq!(u8::MAX.next(), u8::MAX);
assert_eq!(i8::MIN.prev(), i8::MIN);
```

### Associativity is a power adjustment

Associativity is not a special case in the loop — it is which neighbour of the operator's power
the engine recurses at. This table is the whole rule:

| Written | Recurses at | Effect |
|---|---|---|
| [`PrattInfix::Left`](crate::parser::PrattInfix) | `power.next()` | equal-power operator to the right folds into the *outer* call → `a - b - c` = `(a - b) - c` |
| [`PrattInfix::Right`](crate::parser::PrattInfix) | `power.prev()` | equal-power operator to the right is consumed by the *inner* call → `a ^ b ^ c` = `a ^ (b ^ c)` |
| [`PrattInfix::Neither`](crate::parser::PrattInfix) | `power.next()`, then refuses a second operator of the same power | `a == b == c` is rejected |

Two further knobs share the same mechanism:

- **A floor.** A parse runs against a *minimum* binding power (`Power::default()` at the top —
  `0` for an integer power). Operators below the floor are left on the input for the surrounding
  grammar. A non-operator token maps to a power below the floor so the loop stops there naturally.
- **Grouping is a pair below the floor.** `(` is a *prefix* operator and `)` a *postfix* operator
  at the same sub-floor power: `)` is invisible at the top level (below the floor, left for the
  caller) but consumable inside the recursive call a `(` prefix opens (whose floor is that same
  low power). No bracket-matching code — the precedence rule already says it.

---

## `Precedenced<T, Power>`

The carrier that pairs a value (an operand marker, an operator, or an associativity tag) with its
binding power. Every prefix/infix/postfix classification wraps its payload in one.

```text
Precedenced::new(token: T, precedence: Power) -> Precedenced<T, Power>
    .token_ref(&self) -> &T           .precedence(&self) -> &Power
    .into_data(self) -> T             .into_precedence(self) -> Power
    .into_components(self) -> (T, Power)
```

```rust
use tokora::parser::Precedenced;

let p = Precedenced::new("*", 2i64);
assert_eq!(*p.token_ref(), "*");
assert_eq!(*p.precedence(), 2);
let (tok, power) = p.into_components();
assert_eq!((tok, power), ("*", 2));
```

## Classifying operands & operators

Three enums describe what a token/parser contributes at a position. The unit type `()` fills the
payload slots you do not use (the token-level classifier uses `()` throughout; an AST classifier
carries your operator tags).

```text
enum PrattLHS<Op, Pre, Power = i64> {          // left edge of a (sub-)expression
    Operand(Op),                                //   a value
    Prefix(Precedenced<Pre, Power>),            //   a prefix operator + its power
}
enum PrattInfix<L, R, N> { Left(L), Right(R), Neither(N) }   // associativity + operator
enum PrattRHS<L, R, N, Post, Power = i64> {     // what follows an operand
    Infix(Precedenced<PrattInfix<L, R, N>, Power>),
    Postfix(Precedenced<Post, Power>),
}
```

`PrattLHS::try_pratt_lhs`-style classifiers returning `None` (token-level) or a below-floor
`Postfix` sentinel (AST-level) are how the loop learns a token is *not* part of the expression
here and stops.

---

## Token-level surface

### `PrattToken`

The token type classifies *itself*. The `Expr` marker disambiguates multiple grammars over one
token type; `Power` defaults to `i64`.

```text
trait PrattToken<'a, Expr: ?Sized, Power = i64>: Token<'a> {
    fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>>;
    fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>>;
}
```

### `InputRef::pratt` / `pratt_with_min_precedence`

The engine. `pratt` starts at `Power::default()`; `pratt_with_min_precedence` names the floor
(parse only what binds at least that tightly, leaving the rest to the caller — the same knob the
`(` prefix turns).

```text
InputRef::pratt::<FoldPrefix, FoldInfix, FoldPostfix, Expr, Power>(
    fold_prefix, fold_infix, fold_postfix,
) -> Result<Option<Spanned<Token, Span>>, Error>
InputRef::pratt_with_min_precedence(fold_prefix, fold_infix, fold_postfix, min_precedence: Power)
// where Token: PrattToken<'inp, Expr, Power>, Emitter: PrattEmitter, Power: PrattPower
```

`Ok(None)` means the cursor was not looking at an operand or prefix at all.

### The token folds

Named `fn`s (see above). Note the operator position: **first** for prefix, **last** for infix
and postfix; the emitter is always last.

```text
fn fold_prefix (operator, operand,                          &mut Emitter) -> Result<Spanned<Token, Span>, Error>
fn fold_infix  (left,     right,   Spanned<PrattInfix<…>>,  &mut Emitter) -> Result<Spanned<Token, Span>, Error>
fn fold_postfix(operand,  operator,                         &mut Emitter) -> Result<Spanned<Token, Span>, Error>
```

### `PrattEmitter`

The extra capability the token-level engine needs: it reports a prefix/infix operator that ran
out of operand. [`Fatal`](crate::emitter::Fatal)/[`Verbose`](crate::emitter::Verbose)/[`Silent`](crate::emitter::Silent)
all implement it, so a [`FatalContext`](crate::FatalContext) satisfies the bound with no extra work.

```text
trait PrattEmitter<'inp, L, Lang = ()>: Emitter<'inp, L, Lang> {
    fn emit_unexpected_end_of_lhs(&mut self, err: UnexpectedEoLhs<…>) -> Result<(), Self::Error>;
    fn emit_unexpected_end_of_rhs(&mut self, err: UnexpectedEoRhs<…>) -> Result<(), Self::Error>;
}
```

### End to end

A one-character-per-token arithmetic grammar. `+ -` bind loosest (left), `*` tighter (left), `^`
tightest (**right**), unary `-` is a prefix, and `( )` groups. The folds evaluate as they go,
re-encoding each result as a `Digit` token.

```rust
# use core::convert::Infallible;
# use tokora::{
#   FatalContext, InputRef, Lexer, Parse, Parser, SimpleSpan, Token,
#   emitter::Fatal,
#   error::{UnexpectedEnd, token::UnexpectedToken},
#   span::{Span as _, Spanned},
# };
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for Error { fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self { Error } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Error { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(i64), Ident(char), Plus, Minus, Star, Caret, LParen, RParen, Semi }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Ident, Plus, Minus, Star, Caret, LParen, RParen, Semi }
# impl core::fmt::Display for Kind { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self {
#     Tok::Digit(_) => Kind::Digit, Tok::Ident(_) => Kind::Ident, Tok::Plus => Kind::Plus,
#     Tok::Minus => Kind::Minus, Tok::Star => Kind::Star, Tok::Caret => Kind::Caret,
#     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen, Tok::Semi => Kind::Semi } }
#   fn is_trivia(&self) -> bool { false }
# }
# struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
# impl<'a> Lexer<'a> for CharLexer<'a> {
#   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
#   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
#   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
#   fn check(&self) -> Result<(), Infallible> { Ok(()) }
#   fn state(&self) -> &() { &self.state }
#   fn state_mut(&mut self) -> &mut () { &mut self.state }
#   fn into_state(self) -> Self::State {}
#   fn source(&self) -> &'a str { self.src }
#   fn span(&self) -> SimpleSpan { self.tok }
#   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
#   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
#     let bytes = self.src.as_bytes();
#     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
#     if self.pos >= bytes.len() { return None; }
#     let (start, c) = (self.pos, bytes[self.pos] as char);
#     self.pos += 1;
#     self.tok = SimpleSpan::new(start, self.pos);
#     Some(Ok(match c {
#       '0'..='9' => Tok::Digit(c as i64 - '0' as i64),
#       '+' => Tok::Plus, '-' => Tok::Minus, '*' => Tok::Star, '^' => Tok::Caret,
#       '(' => Tok::LParen, ')' => Tok::RParen, ';' => Tok::Semi,
#       c => Tok::Ident(c),
#     }))
#   }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced},
  token::PrattToken,
};

// The table: each token says what it is at each position. `None` = "not part of an
// expression here" — the loop leaves the token on the input and stops.
impl PrattToken<'_, (), i64> for Tok {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), i64>> {
    Some(match self {
      Tok::Digit(_) => PrattLHS::Operand(()),
      Tok::Minus => PrattLHS::Prefix(Precedenced::new((), 3)), //   unary minus
      Tok::LParen => PrattLHS::Prefix(Precedenced::new((), -1)), // `(` — a sub-floor prefix
      _ => return None,
    })
  }
  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), i64>> {
    Some(match self {
      Tok::Plus | Tok::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), 1)),
      Tok::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), 2)),
      Tok::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), 4)), // right-assoc
      Tok::RParen => PrattRHS::Postfix(Precedenced::new((), -1)), //                closes `(`
      _ => return None,
    })
  }
}

// The folds. Named `fn`s; token-level currency is `Spanned<Tok, Span>`.
fn fold_prefix(
  op: Spanned<Tok, SimpleSpan>,
  operand: Spanned<Tok, SimpleSpan>,
  _: &mut Fatal<Error>,
) -> Result<Spanned<Tok, SimpleSpan>, Error> {
  let (span, op) = op.into_components();
  Ok(match op {
    Tok::Minus => Spanned::new(span, Tok::Digit(-int(operand))),
    _ => operand, // `(` grouping: the inner value flows straight through
  })
}
fn fold_infix(
  left: Spanned<Tok, SimpleSpan>,
  right: Spanned<Tok, SimpleSpan>,
  op: Spanned<PrattInfix<Tok, Tok, Tok>, SimpleSpan>,
  _: &mut Fatal<Error>,
) -> Result<Spanned<Tok, SimpleSpan>, Error> {
  let span = left.span();
  let (a, b) = (int(left), int(right));
  // Associativity already did its job in the engine; the fold just wants the operator.
  let (PrattInfix::Left(o) | PrattInfix::Right(o) | PrattInfix::Neither(o)) = op.into_data();
  let v = match o {
    Tok::Plus => a + b,
    Tok::Minus => a - b,
    Tok::Star => a * b,
    Tok::Caret => a.pow(b as u32),
    _ => a,
  };
  Ok(Spanned::new(span, Tok::Digit(v)))
}
fn fold_postfix(
  operand: Spanned<Tok, SimpleSpan>,
  _close: Spanned<Tok, SimpleSpan>,
  _: &mut Fatal<Error>,
) -> Result<Spanned<Tok, SimpleSpan>, Error> {
  Ok(operand) // `)` closed its group; the value flows on
}
fn int(t: Spanned<Tok, SimpleSpan>) -> i64 {
  match t.into_data() {
    Tok::Digit(n) => n,
    _ => 0,
  }
}

// The entry point — one call, the whole expression grammar.
fn eval<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<i64, Error> {
  match inp.pratt::<_, _, _, (), i64>(fold_prefix, fold_infix, fold_postfix)? {
    Some(tok) => Ok(int(tok)),
    None => Err(Error),
  }
}

let eval = |src| Parser::with_parser(eval).parse_str(src);
assert_eq!(eval("1 + 2 * 3"), Ok(7)); //   `*` outranks `+`        → 1 + (2 * 3)
assert_eq!(eval("(1 + 2) * 3"), Ok(9)); // grouping overrides
assert_eq!(eval("2 ^ 3 ^ 2"), Ok(512)); // `^` is RIGHT-assoc      → 2 ^ (3 ^ 2)
assert_eq!(eval("-2 ^ 2"), Ok(-4)); //     `^` outranks unary `-`  → -(2 ^ 2)
```

---

## AST-level surface

### `pratt`

Build a [`Pratt`](crate::parser::Pratt) combinator from two sub-parsers and three folds. It is
generic over the language marker and reads it off the input the result is driven with — there is
one spelling for branded and unbranded grammars alike (the `Lang` convention runs through the
whole crate — see the [combinator reference](super::ref_combinators)). The result implements
[`ParseInput`](crate::ParseInput), so you drive it with `.parse_input(inp)`.

```text
pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix) -> Pratt<…, Lang>
// parse_lhs: any parser producing PrattLHS<O, PreOp, Power>
// parse_rhs: any parser producing PrattRHS<L, R, N, PostOp, Power>
```

`parse_lhs` and `parse_rhs` are ordinary parsers whose output is a classification: any
`ParseInput` yielding a `PrattLHS`/`PrattRHS` qualifies (via the blanket
[`ParsePrattLHS`](crate::parser::ParsePrattLHS) / [`ParsePrattRHS`](crate::parser::ParsePrattRHS)),
so a plain `fn(&mut InputRef) -> Result<PrattLHS<…>, Error>` is enough.

### The `Pratt` builder

Swap folds or set the floor after construction; every method returns a reconfigured `Pratt`.

```text
Pratt::prefix(self, folder)          Pratt::infix(self, folder)     Pratt::postfix(self, folder)
Pratt::min_precedence(self, p: Power)              // start above Power::default()
Pratt::with_cst_kinds(self, kinds: PrattCstKinds<…>) -> Pratt<…, WithCstKinds<…>>
```

### The AST folds

Named `fn`s. The `InputRef` comes **first** (a fold may consume further tokens — that is how a
postfix `[` reads an index and its `]`); the operator is a [`Precedenced`](crate::parser::Precedenced)
and comes **last**. Each returns your node type `O`.

```text
fn fold_prefix (&mut InputRef, operand: O,           operator: Precedenced<PreOp, Power>)              -> Result<O, Error>
fn fold_infix  (&mut InputRef, left: O,   right: O,  operator: Precedenced<PrattInfix<L,R,N>, Power>) -> Result<O, Error>
fn fold_postfix(&mut InputRef, operand: O,           operator: Precedenced<PostOp, Power>)            -> Result<O, Error>
```

### End to end

The same arithmetic, folded into a tree instead of evaluated. A non-operator token maps to a
below-floor `Postfix` sentinel: its power is under the floor, so the engine rolls back the token
it peeked and leaves it for the surrounding grammar. The last stanza adds
[`with_cst_kinds`](crate::parser::Pratt::with_cst_kinds).

```rust
# use core::convert::Infallible;
# use tokora::{
#   FatalContext, InputRef, Lexer, Parse, Parser, SimpleSpan, Token,
#   error::{UnexpectedEnd, token::UnexpectedToken},
#   span::Span as _,
# };
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for Error { fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self { Error } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Error { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(i64), Ident(char), Plus, Minus, Star, Caret, LParen, RParen, Semi }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Ident, Plus, Minus, Star, Caret, LParen, RParen, Semi }
# impl core::fmt::Display for Kind { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self {
#     Tok::Digit(_) => Kind::Digit, Tok::Ident(_) => Kind::Ident, Tok::Plus => Kind::Plus,
#     Tok::Minus => Kind::Minus, Tok::Star => Kind::Star, Tok::Caret => Kind::Caret,
#     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen, Tok::Semi => Kind::Semi } }
#   fn is_trivia(&self) -> bool { false }
# }
# struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
# impl<'a> Lexer<'a> for CharLexer<'a> {
#   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
#   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
#   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
#   fn check(&self) -> Result<(), Infallible> { Ok(()) }
#   fn state(&self) -> &() { &self.state }
#   fn state_mut(&mut self) -> &mut () { &mut self.state }
#   fn into_state(self) -> Self::State {}
#   fn source(&self) -> &'a str { self.src }
#   fn span(&self) -> SimpleSpan { self.tok }
#   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
#   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
#     let bytes = self.src.as_bytes();
#     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
#     if self.pos >= bytes.len() { return None; }
#     let (start, c) = (self.pos, bytes[self.pos] as char);
#     self.pos += 1;
#     self.tok = SimpleSpan::new(start, self.pos);
#     Some(Ok(match c {
#       '0'..='9' => Tok::Digit(c as i64 - '0' as i64),
#       '+' => Tok::Plus, '-' => Tok::Minus, '*' => Tok::Star, '^' => Tok::Caret,
#       '(' => Tok::LParen, ')' => Tok::RParen, ';' => Tok::Semi,
#       c => Tok::Ident(c),
#     }))
#   }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{
  ParseInput as _,
  parser::{PrattFoldOp, PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
};

#[derive(Debug, PartialEq)]
enum Expr {
  Num(i64),
  Neg(Box<Expr>),
  Bin(char, Box<Expr>, Box<Expr>),
}

const SUM: i64 = 1;
const PROD: i64 = 2;
const NEG: i64 = 3;
const EXP: i64 = 4;
const BELOW_FLOOR: i64 = -1; // a non-operator: power under the default floor (0)

// lhs — an operand, a prefix operator, or a parenthesised sub-expression.
fn parse_lhs<'a>(
  inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
) -> Result<PrattLHS<Expr, char, i64>, Error> {
  match inp.next()? {
    None => Err(Error),
    Some(tok) => match tok.into_data() {
      Tok::Digit(n) => Ok(PrattLHS::Operand(Expr::Num(n))),
      Tok::Minus => Ok(PrattLHS::Prefix(Precedenced::new('-', NEG))),
      Tok::LParen => {
        let inner = parse_expr(inp)?; // recurse; the inner call stops before `)`
        if inp.try_expect(|t| matches!(t.data, Tok::RParen))?.is_none() {
          return Err(Error);
        }
        Ok(PrattLHS::Operand(inner))
      }
      _ => Err(Error),
    },
  }
}

// rhs — an infix operator, else a below-floor sentinel the engine rolls back.
fn parse_rhs<'a>(
  inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
) -> Result<PrattRHS<char, char, char, char, i64>, Error> {
  let sentinel = PrattRHS::Postfix(Precedenced::new(' ', BELOW_FLOOR));
  match inp.next()? {
    None => Ok(sentinel),
    Some(tok) => Ok(match tok.into_data() {
      Tok::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left('+'), SUM)),
      Tok::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left('-'), SUM)),
      Tok::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left('*'), PROD)),
      Tok::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right('^'), EXP)),
      _ => sentinel,
    }),
  }
}

// The folds build tree nodes. Named `fn`s again; the `InputRef` comes first.
fn fold_prefix<'a>(
  _inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
  operand: Expr,
  _op: Precedenced<char, i64>,
) -> Result<Expr, Error> {
  Ok(Expr::Neg(Box::new(operand)))
}
fn fold_infix<'a>(
  _inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
  left: Expr,
  right: Expr,
  op: Precedenced<PrattInfix<char, char, char>, i64>,
) -> Result<Expr, Error> {
  let (PrattInfix::Left(c) | PrattInfix::Right(c) | PrattInfix::Neither(c)) = op.into_data();
  Ok(Expr::Bin(c, Box::new(left), Box::new(right)))
}
fn fold_postfix<'a>(
  _inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
  operand: Expr,
  _op: Precedenced<char, i64>,
) -> Result<Expr, Error> {
  Ok(operand)
}

fn parse_expr<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Expr, Error> {
  pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
}

let tree = Parser::with_parser(parse_expr).parse_str("1 + 2 * 3").unwrap();
assert_eq!(
  tree,
  Expr::Bin(
    '+',
    Box::new(Expr::Num(1)),
    Box::new(Expr::Bin('*', Box::new(Expr::Num(2)), Box::new(Expr::Num(3)))),
  ),
);

// `with_cst_kinds` wraps each fold in a CST node of the classifier's chosen kind. Over a
// `Fatal` emitter (a no-op `CstEmitter`) the wraps cost nothing and the value is unchanged;
// over a recording sink they build the lossless tree. The classifier is a plain `fn` pointer.
fn classify(op: PrattFoldOp<'_, char, char, char, char, char>) -> Option<u16> {
  match op {
    PrattFoldOp::Prefix(_) => Some(1),
    PrattFoldOp::Infix(_) => Some(2),
    PrattFoldOp::Postfix(_) => Some(3),
  }
}
fn parse_expr_cst<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Expr, Error> {
  pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix)
    .with_cst_kinds(classify)
    .parse_input(inp)
}
assert_eq!(Parser::with_parser(parse_expr_cst).parse_str("1 + 2 * 3").unwrap(), tree);
```

---

## Building a CST while you fold

Only the AST driver carries a CST seam. [`with_cst_kinds`](crate::parser::Pratt::with_cst_kinds)
takes a classifier mapping each fold's operator to a node kind (`None` records no node); the
driver mints one mark before the expression and spends it once per fold, so same-target wraps
nest inside-out and `1 + 2 * 3` materializes as `Bin[1, +, Bin[2, *, 3]]`. The fold hooks are
untouched — they never see the event channel.

```text
type PrattCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp> =
    fn(PrattFoldOp<'_, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>) -> Option<u16>;
enum PrattFoldOp<'op, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp> {
    Prefix(&'op PreOp), Infix(&'op PrattInfix<…>), Postfix(&'op PostOp),
}
```

Two implementation types back the seam (both re-exported, rarely named): the default
[`NoCst`](crate::parser::NoCst) — inert, zero-cost, no bound beyond the core emitter — and
[`WithCstKinds`](crate::parser::WithCstKinds), whose `ParseInput` impl carries
`Ctx::Emitter: CstEmitter`. That bound is a **structural gate**: a kinds-configured Pratt parser
over an emitter without the event channel is a compile error, never a silently tree-less parse.

The **token-level** API is CST-unsupported in this version: it folds into synthetic tokens with
no node-kind seam to classify. Build the tree with the typed driver instead. (See the lossless
CST chapter for the recording sink; it is behind the `rowan` feature and named here without a
link.)

## Expression-end errors

The two errors the token-level engine emits through [`PrattEmitter`](crate::emitter::PrattEmitter)
when an operator is missing its operand. Both are aliases of
[`UnexpectedEnd`](crate::error::UnexpectedEnd); the base constructor fixes `Lang = ()` and the
`_of` twin ([`eolhs_of`](crate::error::UnexpectedEnd::eolhs_of) /
[`eorhs_of`](crate::error::UnexpectedEnd::eorhs_of)) is language-generic.

```rust
use tokora::error::{UnexpectedEoLhs, UnexpectedEoRhs};

let lhs = UnexpectedEoLhs::eolhs(7usize);
assert_eq!(lhs.offset(), 7);
assert_eq!(lhs.name(), Some("expression (left hand side)"));

let rhs = UnexpectedEoRhs::eorhs(7usize);
assert_eq!(rhs.name(), Some("expression (right hand side)"));
```

## See also

- [Chapter 5 — Pratt parsing](super::ch05_pratt): the token-level engine taught with the full
  calculator ladder (grouping-below-floor, right-associativity, prefix operators).
- [Chapter 15 — C expressions](super::ch15_c_expression_example): the AST-level driver with folds
  that consume further input (index, call, ternary).
- [Combinator & atom reference](super::ref_combinators): the `Lang` convention and the
  broader parser surface the folds compose with.
- [Errors, emitters & context reference](super::ref_errors_emitters_context): the emitter
  capability model that [`PrattEmitter`](crate::emitter::PrattEmitter) extends.
