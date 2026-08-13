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

The precedence of an operator: an ordered level, and nothing else. The engine only ever
*compares* two powers, so the trait adds no methods of its own — anything
`Default + Clone + Ord` can be a ladder. tokora implements it for every standard integer type,
so a plain `i64` — the default `Power` throughout — works with no newtype. Implement it
yourself when you want *named* levels and a type-checked ladder.

```text
trait PrattPower: Default + Clone + Ord {}
```

```rust
use tokora::parser::PrattPower;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Prec(i32);
impl PrattPower for Prec {}

const PREC_SUM: Prec = Prec(1);
const PREC_PROD: Prec = Prec(2);
// Only the relative order carries meaning; the numbers themselves never get arithmetic done
// to them, so gaps in the ladder are free and its extremes are not special.
assert!(PREC_PROD > PREC_SUM);
assert_eq!(Prec::default(), Prec(0));
```

### Associativity is how strict the floor is

Associativity is not a special case in the loop — it is how strictly the engine compares the
next operator's power against the current one when it recurses into the right operand. This
table is the whole rule:

| Written | Right operand admits | Effect |
|---|---|---|
| [`PrattInfix::Left`](crate::parser::PrattInfix) | powers `> power` | equal-power operator to the right folds into the *outer* call → `a - b - c` = `(a - b) - c` |
| [`PrattInfix::Right`](crate::parser::PrattInfix) | powers `>= power` | equal-power operator to the right is consumed by the *inner* call → `a ^ b ^ c` = `a ^ (b ^ c)` |
| [`PrattInfix::Neither`](crate::parser::PrattInfix) | powers `> power`, then refuses a second **infix** operator of the same power | `a == b == c` fails with [`NonAssociativeChain`](crate::error::NonAssociativeChain) |

**What "refuses" means, exactly.** Both engines raise the *same* error and leave the second
operator on the input, unconsumed. The offset it carries is the **handback position**, and that is
one specific, checkable number: catch the error and
[`InputRef::span().end()`](crate::InputRef::span) is the offset. Nothing before it is still
available to you; everything from it onward is. That is what makes the error usable: the offset
names a real boundary in your own input, not a position derived from something near it.

It is **not** the second operator's own start, and you should not read it as a pointer at the
operator. Anything the handback also returned sits between the two: whitespace your lexer skipped,
trivia *tokens* a [`ParsePrattRHS`](crate::parser::ParsePrattRHS) would have skipped, the gap
inside a multi-token spelling (`not in`, `<>`), or a region a non-fatal lexer error was reported
over. On `1 ; 2 ; 3` the offset is 5 and the repeated `;` is at 6. If you are rendering a caret,
skip forward from the offset the way your own grammar would; if you are resuming a parse, start
exactly there. The AST-level driver could not report the operator's head even in principle:
finding it means running the classifier, and the driver has to decide the repeat before it may.
And
[`NonAssociativeChain`](crate::error::NonAssociativeChain) is **returned** — never emitted, so a
recording emitter cannot turn it back into a truncated success. It is *not* terminal, so a
grammar that wants the tolerant reading asks for it explicitly, by wrapping the pratt parser in
[`recover`](crate::ParseInput::recover) / [`skip_then_retry`](crate::ParseInput::skip_then_retry)
or by declaring the operator [`Left`](crate::parser::PrattInfix::Left). The latch is armed by a
`Neither` fold, cleared by folding an infix at a different power, and untouched by a postfix
fold — so `a == b! == c` still trips.

**The offset is where the handback left the input — not where a recovery combinator restarts.** Two
of the three roll back further before they run: [`recover`](crate::ParseInput::recover) and
[`skip_then_retry`](crate::ParseInput::skip_then_retry) speculate through
[`try_attempt`](crate::InputRef::try_attempt), whose failure path restores the pre-attempt
checkpoint, so what they hand a handler — or begin skipping from — is their own attempt origin. On
`1 ; 2 ; 3` with the whole pratt parser wrapped, the error carries offset **5** and:

| Path | The position it observes |
|---|---|
| catching the `Err` in your own grammar | **5** |
| [`inplace_recover`](crate::ParseInput::inplace_recover) | **5** — it never backtracks; the [`Cursor`](crate::input::Cursor) it is *also* handed names where the primary parser started, 0 |
| [`recover`](crate::ParseInput::recover) | **0** |
| [`skip_then_retry`](crate::ParseInput::skip_then_retry) | **0**, and it scans forward from there — on this input it synchronises on the *first* `;` at 2, behind the repeat, and its first skipped region is `0..1` |

So a `.recover(…)` handler may render a caret at the offset it was handed, but must not assume the
input is positioned there. If you want a recovery that resumes at the offset, catch the `Err`
yourself or reach for [`inplace_recover`](crate::ParseInput::inplace_recover).

**Known limitation: the contract is per-operator, not whole-chain fixity resolution.** `a == b < c`
with `==` non-associative and `<` left-associative at the same power is **rejected**, while
`a < b == c` is **accepted** as `(a < b) == c`: the latch only exists once a `Neither` operator has
folded. Haskell and Rust reject both. Tightening tokora to match is a semantic expansion, not a
fix, and is deliberately out of scope for the table above.

Two further knobs share the same mechanism:

- **A floor.** A parse runs against a *minimum* binding power (`Power::default()` at the top —
  `0` for an integer power). Operators below the floor are left on the input for the surrounding
  grammar. A token that is not an operator at all is not a power: an AST-level classifier says
  [`PrattRHS::End`](crate::parser::PrattRHS) and a token-level one returns `None`.
- **Grouping is a pair below the floor.** `(` is a *prefix* operator and `)` a *postfix* operator
  at the same sub-floor power: `)` is invisible at the top level (below the floor, left for the
  caller) but consumable inside the recursive call a `(` prefix opens (whose floor is that same
  low power). No bracket-matching code — the precedence rule already says it.

---

## Recursion limits

**Both engines bound their own descent, and the bound is on by default.** Each pratt frame enters
one level of the input's shared
[`RecursionLimiter`](crate::state::recursion_tracker::RecursionLimiter) through
[`InputRef::descend`](crate::InputRef::descend), whose [`Descent`](crate::input::Descent) guard
releases the level on every exit — return, `?`, or unwind, identically in `std` and `no_std`.
Exceeding the limit fails the parse with
[`RecursionLimitReached`](crate::error::RecursionLimitReached). Your own recursive combinators
draw on the same budget through
[`InputRef::descending`](crate::InputRef::descending) — see
[Bounding your own recursion](#bounding-your-own-recursion).

- **Default 64**, so an unconfigured parse of a deeply nested expression fails cleanly instead of
  risking a native stack abort. Set your own with
  [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter) or
  [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter);
  spell "no limit" as
  [`RecursionLimiter::unlimited()`](crate::state::recursion_tracker::RecursionLimiter::unlimited).
  The default is sized against the **tightest** of the four measured configurations, on the
  **2 MiB** stack a spawned thread and a libtest harness thread get: release fits 3871 typed
  frames and 4247 token frames, debug fits 384 typed and **125 token**. 64 clears that last figure
  by about 1.9× and every other by 6× or more. It is deliberately conservative, because the two
  failure modes are not symmetric — too low returns a catchable error telling you to raise it, too
  high aborts the process with no diagnostic. See
  [`RecursionLimiter`](crate::state::recursion_tracker::RecursionLimiter#default-limit) for the
  full table. A grammar that parses deep untrusted input should still pick a limit against the
  stack it will actually run on rather than inherit this one.
- **One budget per input, not per parser.** Two pratt parsers composed into one grammar share the
  depth, because what the limit protects — the native stack — is shared too. The root expression
  counts as one level.
- **Terminal, for every grammar error type — the stop does not travel in the payload.** No amount
  of further input clears a depth budget, so [`recover`](crate::ParseInput::recover),
  [`InplaceRecover`](crate::parser::InplaceRecover) and
  [`skip_then_retry`](crate::ParseInput::skip_then_retry) re-raise a trip untouched rather than
  synthesizing a node. That holds for an error type that stores the value and delegates
  [`is_terminal`](crate::error::MaybeTerminal::is_terminal), and equally for a discarding sink such
  as `()`: the trip latches the **input session** before any conversion runs, and the three
  combinators read that latch beside `is_terminal()`. A discarding sink loses the offset, the depth
  and the limitation; it does not lose the stop.

  The **resilient collection loops** — `repeated`, `separated` and their delimited forms — read the
  same session cell on every exit that could otherwise spend a trip without ever raising it as the
  grammar's own error: an element's `Err` re-raises instead of filing among the collection's
  diagnostics, and so do the two exits that would otherwise conclude the construct ended with no
  `Err` in hand at all — the element declining (or a cycle making no progress), and a real closer
  committed just after either. None of that is a discarding-sink repair: those families spent a
  trip on all three exits, for *every* error type, until tokora 0.9, so an element that trips one of
  them now fails its collection where it used to truncate it.

  **One exit is not closed, and will not be.** An element that catches the trip itself and still
  answers `Accept` has produced a value rather than concluded absence, so the driver is faithfully
  collecting what it was handed instead of manufacturing a stop of its own — and that exit spends
  the trip regardless of error type, exactly as it always did. Refusing it would mean a
  value-producing element could never recover from a budget it deliberately caught, which is a
  broader contract than this crate makes for any other error a grammar is free to catch and answer.
  See `parser::many`'s module docs for the reasoning and the pinned boundary.

  Every one of those sites reads the cell **relative to the attempt it is judging** — the
  speculative parse, or the one element — by snapshotting it beforehand and asking whether it moved.
  What the cell records is a monotone session fact ("this parse tripped a budget"); what a site asks
  is a per-attempt one ("did *this* fail because of a trip"). They differ exactly where grammar code
  catches a trip and parses on, and answering the second with the first would have suppressed every
  later diagnostic in the document.

  The resolution of that per-attempt question is **one attempt** — one speculative parse, one
  `skip_then_retry` cycle, one element — and no finer. Inside that unit the witness proves that *a*
  trip happened, not that the error in hand is it, so grammar code that catches a trip itself and
  then fails **ordinarily before the same attempt ends** has the ordinary failure re-raised rather
  than recovered or filed. Move the catch one construct further out and it behaves exactly as an
  untripped parse. The floor fails closed — a real trip reaching one of these sites as an `Err`, or
  as an element concluding absence, is never recovered from and never filed as a diagnostic — and
  it cannot be lowered by inspecting the error, because the sink this whole design exists for has
  discarded it. That floor is about what a site does with a trip it is asked to judge; it says
  nothing about the one exit above that a caught trip never reaches at all.

  This was not true before tokora 0.9. A `()`-errored grammar used to get `is_terminal() == false`
  on the converted value and **spend** the trip: `recover` synthesized a node for a construct the
  budget forbade reading, and `skip_then_retry` handed the surrounding grammar back offset 68 — a
  32-deep chain and the sync token committed and gone — where a delegating error type re-raised
  before any skip and handed back offset 0. Same verdict, different input, decided by an unrelated
  error sink. Both now read offset 0.

  What the limiter's own job never depended on: by the time the error surfaces anywhere outside the
  engine, the native stack is fully unwound and the depth budget is back to what it was before the
  parse. Measured over a 200-level trip, the surfacing frame sits 48 bytes from the pre-parse
  baseline in a debug build and 0 in a release one, against a descent that reached about 1 MiB and
  97 KiB respectively. See
  [`RecursionLimitReached`](crate::error::RecursionLimitReached#the-stop-does-not-travel-in-the-payload-so-a-discarding-sink-cannot-drop-it)
  for the measurements in full and a compiling example of an error type that keeps the details.
- **What is latched, and what is not.** A *scanner* limit trip latches the poison boundary, because
  the lexer's tally is monotone in the input. A descent trip latches the fact that a budget was
  exceeded, for the same reason at one remove: that cannot be un-exceeded either. What is **not**
  latched is the *depth* — it is the opposite kind of fact and is fully restored by the unwind that
  carries the error out. So a scanner trip latches *where* and stops lexing; a descent trip latches
  *whether* and stops recovery — including the emit-and-continue kind a collection driver does.
  Because the second is a session fact rather than a per-error one, it is counted rather than
  flagged and consulted as a difference across one attempt: a failure is charged to the budget only
  where the count moved while that attempt ran.

### Bounding your own recursion

A hand-written recursive combinator can draw on the same budget, and the way to do it is
[`InputRef::descending`](crate::InputRef::descending) — the level is the closure:

```rust,ignore
fn nested(inp: &mut InputRef<'_, '_, L, Ctx>, remaining: usize) -> Result<usize, MyError> {
  inp.descending(|inp| match remaining {   // one level, for exactly this body
    0 => Ok(inp.recursion().depth()),
    n => nested(inp, n - 1),
  })
}
```

`f`'s error is returned untouched, so `?` inside the body composes with everything the frame
already returns, and the trip is built as the frame's own error type. Write the *whole* frame body
as the closure and its `return`s keep their meaning — the closure returns the same `Result` the
frame does. If the body panics, the level is released on the unwind.

**Why a closure and not just a guard.** [`descend`](crate::InputRef::descend) is also public and
hands the level back as an ordinary value, and then *where the level ends is your code*. The
correct spelling is a binding held for the whole frame:

```rust,ignore
let mut frame = inp.descend()?;   // one level, for as long as `frame` lives
let inp = &mut *frame;            // the body below is unchanged
```

and there are at least four spellings that end it one statement too early, of which the compiler
catches exactly one:

```rust,ignore
inp.descend()?;                                  // warns: `unused_must_use`
let _ = inp.descend()?;                          // silent
if inp.descend().is_ok() { recurse(inp, n - 1) } // silent
let d = inp.descend()?.recursion().depth();      // silent
drop(inp.descend()?);                            // silent
```

All five compile, and all five were measured: against a limit of 8, 200 recursive calls return
`Ok` with the depth cell reading 0 (or 1 for the chain), and by 4 000–5 000 levels each one aborts
a 2 MiB thread with `fatal runtime error: stack overflow` — the failure the budget exists to
delete. [`Descent`](crate::input::Descent) is `#[must_use]`, which is what catches the first
line, and `tests/ui/descent_dropped_early.rs` pins that it does; the other four are not a closed
list, because *any* expression that consumes the guard and lets it die before the recursion does
the same. Early release cannot be made unrepresentable — a frame that finishes recursing and then
keeps parsing shallower wants exactly it — so what closes the question for a given frame is
choosing a shape where the level's scope and the body are the same region. `descending` is that
shape without the discipline; the bound guard is that shape with it. Reach for `descend` when the
body cannot be a closure — a `return` out of an enclosing function, a `break` aimed at an outer
loop — and then bind it.

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
    End,                                        //   the expression stops here
}
```

A token-level classifier returning `None`, and an AST-level one returning `PrattRHS::End`, are
how the loop learns a token is *not* part of the expression here and stops — at exhaustion just
the same. Do **not** spell that as a below-floor `Postfix` "sentinel": a sentinel is a real
operator report, so whether it binds depends on the floor the loop happens to be running at, and
over an unsigned `Power` there is no value below the default floor to give it.

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
fn fold_prefix (operator, operand,                          EmitterView) -> Result<Spanned<Token, Span>, Error>
fn fold_infix  (left,     right,   Spanned<PrattInfix<…>>,  EmitterView) -> Result<Spanned<Token, Span>, Error>
fn fold_postfix(operand,  operator,                         EmitterView) -> Result<Spanned<Token, Span>, Error>
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
#   EmitterView, FatalContext, InputRef, Lexer, Parse, Parser, SimpleSpan, Token,
#   emitter::Fatal,
#   error::{UnexpectedEnd, token::UnexpectedToken},
#   span::{Span as _, Spanned},
# };
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for Error { fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self { Error } }
# impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for Error { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for Error { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Error } }
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
#   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
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
fn fold_prefix<'a>(
  op: Spanned<Tok, SimpleSpan>,
  operand: Spanned<Tok, SimpleSpan>,
  _: EmitterView<'_, 'a, CharLexer<'a>, Fatal<Error>>,
) -> Result<Spanned<Tok, SimpleSpan>, Error> {
  let (span, op) = op.into_components();
  Ok(match op {
    Tok::Minus => Spanned::new(span, Tok::Digit(-int(operand))),
    _ => operand, // `(` grouping: the inner value flows straight through
  })
}
fn fold_infix<'a>(
  left: Spanned<Tok, SimpleSpan>,
  right: Spanned<Tok, SimpleSpan>,
  op: Spanned<PrattInfix<Tok, Tok, Tok>, SimpleSpan>,
  _: EmitterView<'_, 'a, CharLexer<'a>, Fatal<Error>>,
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
fn fold_postfix<'a>(
  operand: Spanned<Tok, SimpleSpan>,
  _close: Spanned<Tok, SimpleSpan>,
  _: EmitterView<'_, 'a, CharLexer<'a>, Fatal<Error>>,
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

The same arithmetic, folded into a tree instead of evaluated. A non-operator token answers
`PrattRHS::End`, and the engine rolls back the token it read and leaves it for the surrounding
grammar. The last stanza adds
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
# impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for Error { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for Error { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Error } }
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
#   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
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

// rhs — an infix operator, else `End`; the engine rolls the token back.
fn parse_rhs<'a>(
  inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
) -> Result<PrattRHS<char, char, char, char, i64>, Error> {
  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => Ok(match tok.into_data() {
      Tok::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left('+'), SUM)),
      Tok::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left('-'), SUM)),
      Tok::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left('*'), PROD)),
      Tok::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right('^'), EXP)),
      _ => PrattRHS::End,
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
