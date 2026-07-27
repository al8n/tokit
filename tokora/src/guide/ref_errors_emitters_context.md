# Reference: errors, emitters & context

Three subsystems sit behind every parser signature in this book: the **error model** (what a
failure *is*), the **emitter** (what happens to a diagnostic once you have one), and the
**parse context** (the bundle that carries the emitter and its cache into every combinator).
The [combinator reference](super::ref_combinators) tabulated the taxonomy and feature matrix in
passing; this chapter is the catalog for the three, with the trait surface each one asks of your
code. The tutorial treatments are [chapter 7 (diagnostics)](super::ch07_diagnostics),
[chapter 8 (recovery)](super::ch08_recovery), and [chapter 9 (partial input)](super::ch09_streaming).

## How to read this reference

- **Signatures** are shown trimmed (the always-present `L: Lexer<'inp>` and `Self: Sized` bounds
  are elided) in `text` blocks; the compiling ` ```rust ` blocks show minimal real uses.
- The token-level examples share one hidden scaffold — a minimal hand-written
  [`Lexer`](crate::Lexer), `CharLexer`, over single-character tokens (`Digit`, `Ident`, and the
  punctuation `,` `;` `+` `*` `(` `)` `[` `]`) — identical to the combinator reference's. The
  first example makes the error type **visible** (it is the subject); later ones hide it.
- The `Lang: ?Sized = ()` language marker rides every type and trait here. The base spelling
  fixes `Lang = ()`; the `_of`/`…Of` spellings are generic over it. This chapter uses the base
  forms; see the [combinator reference](super::ref_combinators) for the convention.

---

## The error model

A parser never dictates a concrete error type. Each combinator raises the **leaf** error type
for its failure (all of them under [`crate::error`], each carrying a source span), and *your*
error type absorbs the ones it can encounter through `From`. The pre-built emitters are generic
over exactly that: give your enum the right `From` impls and `Fatal`/`Verbose`/`Silent` drive it
for free.

### Taxonomy by category

Every leaf has an `…Of<'inp, L, Lang>` alias that fixes its span/offset to the lexer's, and
[`ErrorOf<'inp, L, Ctx, Lang>`](crate::ErrorOf) names a context's error type. Full type list in
the [combinator reference](super::ref_combinators); here each category is paired with the `From`
impl that wires it in.

| Category | Leaf types (module) | Your error absorbs |
|----------|---------------------|--------------------|
| **Lexer** | your `Token::Error`, plus [`UnknownLexeme`](crate::error::UnknownLexeme) / [`Malformed`](crate::error::Malformed) / [`Invalid`](crate::error::Invalid) / escape errors | `From<<L::Token as Token>::Error>` |
| **Token** | [`UnexpectedToken`](crate::error::token::UnexpectedToken), [`MissingToken`](crate::error::token::MissingToken), [`SeparatedError`](crate::error::token::SeparatedError) | `From<UnexpectedToken>`, `From<MissingToken>`, `From<SeparatedError>` |
| **End of input** | [`UnexpectedEnd`](crate::error::UnexpectedEnd) (aliases [`UnexpectedEot`](crate::error::UnexpectedEot) / `UnexpectedEof` / `UnexpectedEos`) | `From<UnexpectedEot<O, Lang, Set>>` |
| **Syntax** | [`TooFew`](crate::error::syntax::TooFew), [`TooMany`](crate::error::syntax::TooMany), [`FullContainer`](crate::error::syntax::FullContainer), [`MissingSyntax`](crate::error::syntax::MissingSyntax) | `From<TooFew>`, `From<TooMany>`, `From<FullContainer>`, `From<MissingSyntax>` |
| **Delimiter** | [`Unclosed`](crate::error::Unclosed), [`Unopened`](crate::error::Unopened), [`Undelimited`](crate::error::Undelimited), [`Unterminated`](crate::error::Unterminated) | [`FromUnclosed`](crate::emitter::FromUnclosed) — **one** impl for every pair; the other three are raised by your own code, so they need a `From<…>` only if you raise them |
| **Incomplete** | [`Incomplete`](crate::error::Incomplete) — the never-recoverable partial-input signal | *no `From`*; `impl MaybeIncomplete` instead |

### The traits your error type implements

- **The `From` family.** The bound a generic parser actually checks is
  [`FromTokenErrors`](crate::emitter::FromTokenErrors) — the five token-level conversions as one
  name: both end-of-input instantiations (one `Set`-generic impl covers both), the unexpected
  token, the lexer's own `Token::Error`, and [`FromUnclosed`](crate::emitter::FromUnclosed). The
  entry path additionally names
  [`FromEmitterError`](crate::emitter::FromEmitterError) (`From<Token::Error> +
  From<UnexpectedTokenOf>`), and the collecting combinators layer more `From`s on top through
  their own blanket bounds (see [emitters](#emitters) below). You never implement either bundle
  by hand — you write the `From` impls, one `FromUnclosed` impl, and the blankets do the rest.
- [**`MaybeIncomplete`**](crate::error::MaybeIncomplete) — the discrimination hook for the
  [never-recoverable law](super::ch09_streaming): recovery re-raises an [`Incomplete`](crate::error::Incomplete)
  instead of fabricating a value from input that has not arrived. It has a blanket `false`
  default, so most error types opt in with an empty `impl MaybeIncomplete for MyError {}` and
  override [`is_incomplete`](crate::error::MaybeIncomplete::is_incomplete) only if the type can
  itself carry the signal. [`Recover`](crate::parser::Recover) requires this bound — and since
  0.3.0 partial mode as a whole does: `Partial`'s
  [`SurfaceIncomplete`](crate::SurfaceIncomplete) impl requires `MaybeIncomplete` alongside
  `From<Incomplete<L::Offset>>`, because the input layer *constructs* incompletes while the
  atom layer now also *recognizes* them (the resilient collection loops re-raise a frontier
  `Incomplete` untouched instead of spending it as a diagnostic).
- **The `Set` / `Expected` machinery.** A token mismatch does not just say "wrong" — it names
  what was wanted. [`UnexpectedToken`](crate::error::token::UnexpectedToken) carries an
  [`Expected<'a, Kind>`](crate::utils::Expected) (`One(kind)` or `OneOf(set)`); classifiers
  build it (`Expected::one(k)`, `Expected::one_of(&[…])`), and
  [`dispatch_on_kind`](crate::ParseChoice::dispatch_on_kind) turns its whole table into the
  expected set on a miss. The end-of-input errors are generic over a `Set` type (default
  `&'static str`); when the expected set is a token-kind table — as `dispatch_on_kind` builds —
  `Set` is your `Kind`, which is why the `From<UnexpectedEot>` impl below is generic over
  `Set: Clone + 'static`.

```text
trait MaybeIncomplete {
    fn is_incomplete(&self) -> bool { false }   // override only if the type can carry Incomplete
}
enum Expected<'a, T: Clone> { One(T), OneOf(OneOf<'a, T>) }   // the "expected set" on a mismatch
```

The example makes an error enum and wires the taxonomy into it, one variant per category, then
drives two of the paths:

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
#   punct::{Comma, OpenBracket, CloseBracket, OpenParen, CloseParen, Semicolon},
#   span::Span as _,
#   token::PunctuatorToken,
# };
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(u32), Ident(char), Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Ident, Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self {
#     Tok::Digit(_) => Kind::Digit, Tok::Ident(_) => Kind::Ident, Tok::Comma => Kind::Comma,
#     Tok::Semi => Kind::Semi, Tok::Plus => Kind::Plus, Tok::Star => Kind::Star,
#     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen,
#     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket } }
#   fn is_trivia(&self) -> bool { false }
# }
# impl PunctuatorToken<'_> for Tok {
#   fn comma() -> Option<Kind> { Some(Kind::Comma) }
#   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
#   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
#   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
#   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
#   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
# }
# impl From<Comma<(), (), ()>> for Kind { fn from(_: Comma<(), (), ()>) -> Self { Kind::Comma } }
# impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
# impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
# impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
# impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
# impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
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
#       '0'..='9' => Tok::Digit(c as u32 - '0' as u32),
#       ',' => Tok::Comma, ';' => Tok::Semi, '+' => Tok::Plus, '*' => Tok::Star,
#       '(' => Tok::LParen, ')' => Tok::RParen, '[' => Tok::LBracket, ']' => Tok::RBracket,
#       c => Tok::Ident(c),
#     }))
#   }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, ParseInput as _, error::MaybeIncomplete, parser::expect, utils::Expected};

// Your error type is an ordinary enum. It becomes a *tokora* error type by absorbing — through
// `From` — every leaf the combinators it drives can raise. Each impl wires one category to one
// variant. (The lexer error here is `Infallible`; a real lexer's is `<L::Token as Token>::Error`.)
#[derive(Debug, PartialEq)]
enum Error {
    Lex,        // the lexer's own error
    Unexpected, // a wrong token, or a stray separator
    Eot,        // input ended where a token was required
    Unclosed,   // an opener committed and its closer never arrived
    Missing,    // a required token or element was absent
    Count,      // a repetition/container bound: too few, too many, or full
}
impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error {
    fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error::Unexpected }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error {
    fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error::Unexpected }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error {
    fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error::Eot }
}
impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Error {
    fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error::Unclosed }
}
impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error {
    fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error::Missing }
}
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error {
    fn from(_: MissingSyntax<O, Lang>) -> Self { Error::Missing }
}
impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error::Count } }
impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error::Count } }
impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error::Count } }

// `Incomplete` is never recoverable (chapter 9). Opt in with the empty impl; override the
// method only if `Error` can itself represent an incomplete signal.
impl MaybeIncomplete for Error {}

// With those impls, the concrete `FatalContext<'_, CharLexer, Error>` (hidden as `Ctx`) drives
// the whole surface. `expect` produces two different leaves — exercise both:
fn a_plus<'a>(inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>) -> Result<Tok, Error> {
    expect(|t: &Tok| if matches!(t, Tok::Plus) { Ok(()) } else { Err(Expected::one(Kind::Plus)) })
        .parse_input(inp)
}
// a wrong token → `UnexpectedToken` → `Error::Unexpected`
assert_eq!(Parser::with_parser(a_plus).parse_str("*"), Err(Error::Unexpected));
// end of input where a token was required → `UnexpectedEot` → `Error::Eot`
assert_eq!(Parser::with_parser(a_plus).parse_str(""), Err(Error::Eot));
```

---

## Emitters

The emitter is the one replaceable object that decides *what happens to a diagnostic*. A parser
calls [`emit_error`](crate::Emitter::emit_error) (or `emit_warning`, or a combinator does so on
your behalf) and carries on with `?`; the emitter's return value is what the `?` sees. Same
parser code, opposite behavior, chosen by the context you hand in.

### The base trait

```text
trait Emitter<'a, L, Lang = ()> {
    type Error;                                                    // your error model
    fn emit_lexer_error(&mut self, Spanned<Token::Error, Span>)     -> Result<(), Error>;
    fn emit_unexpected_token(&mut self, UnexpectedTokenOf<'a,L,Lang>) -> Result<(), Error>;
    fn emit_error(&mut self, Spanned<Error, Span>)                  -> Result<(), Error>;
    // REQUIRED, no default — how does your state unwind? A speculative branch
    // unwinds position + diagnostics + CST events together (chapter 6); the
    // stateless emitters write the trivially empty body by hand.
    fn rewind(.., u64);
    // defaulted surface — override only what your emitter records:
    fn emit_warning(..)          // second, never-fatal channel
    fn emit_skipped_region(..)   // one note per recovery hole (chapter 8)
    fn checkpoint(&self) -> u64  //  ┐ the marks of that rewindable timeline: a reading
    fn release(&mut self, u64)   //  ┘ taken at a save, reclaimed when a branch is kept
    fn commit_token(.., ..)      // the auto-CST hook (see CstEmitter)
    fn enter_label / exit_label  // the "while parsing X" stack for `labelled`
}
```

`Ok(())` means non-fatal (parsing continues); `Err(Self::Error)` is fatal (the `?` stops the
parse). Four members are required: the three emit verbs, plus [`rewind`](crate::Emitter::rewind),
which deliberately has **no default body** — an emitter must say how its state unwinds, because a
recording emitter that inherited a no-op rewind would keep the diagnostics of abandoned branches
(the [atomic-emitter chapter](super::arch_atomic_emitter) develops why; `Fatal`/`Silent`/`Ignored`
each write the trivially empty body explicitly). Everything past those four has a blanket no-op
default, so a fail-fast emitter inherits empty bodies and the calls inline to nothing.

### Capability sub-traits

The collecting combinators (`separated`, `repeated`, the `many/` builder, `pratt`, the CST
nodes) need more than the base surface, so tokora splits each scenario into a focused sub-trait —
**implement only what you need**. Each rides a `From…Error` blanket, so implementing the named
`From` on your error type is all it takes.

| Sub-trait | Emits | Unlocked by | In [`ComposableEmitter`](crate::emitter::ComposableEmitter)? |
|-----------|-------|-------------|:--:|
| [`TooFewEmitter`](crate::emitter::TooFewEmitter) | [`TooFew`](crate::error::syntax::TooFew) | `From<TooFew>` | ✅ |
| [`TooManyEmitter`](crate::emitter::TooManyEmitter) | [`TooMany`](crate::error::syntax::TooMany) | `From<TooMany>` | — |
| [`FullContainerEmitter`](crate::emitter::FullContainerEmitter) | [`FullContainer`](crate::error::syntax::FullContainer) | `From<FullContainer>` | ✅ |
| [`SeparatedEmitter`](crate::emitter::SeparatedEmitter) | missing separator / element | `From<MissingTokenOf>` + `From<MissingSyntaxOf>` | ✅ |
| [`UnexpectedLeadingSeparatorEmitter`](crate::emitter::UnexpectedLeadingSeparatorEmitter) / [`…Trailing…`](crate::emitter::UnexpectedTrailingSeparatorEmitter) | a stray separator | `From<SeparatedErrorOf>` | ✅ |
| [`MissingLeadingSeparatorEmitter`](crate::emitter::MissingLeadingSeparatorEmitter) / [`…Trailing…`](crate::emitter::MissingTrailingSeparatorEmitter) | a required separator | `From<MissingTokenOf>` | — |
| [`PrattEmitter`](crate::emitter::PrattEmitter) | end-of-LHS / end-of-RHS ([chapter 5](super::ch05_pratt)) | `From<UnexpectedEoLhs>` + `From<UnexpectedEoRhs>` | — |
| [`CstEmitter`](crate::emitter::CstEmitter) | tree events (no error) | — (defaulted no-ops; the recording sink) | — |

[`ComposableEmitter`](crate::emitter::ComposableEmitter) is the six-trait bundle the
separated/repeated machinery needs, as one bound — blanket-implemented for every emitter that
satisfies the whole family, so `E: ComposableEmitter` stands in for the ladder. The four
capabilities outside it (`TooManyEmitter`, the missing-separator pair, `PrattEmitter`,
`CstEmitter`) are named on demand by the parsers that use them; the pre-built emitters implement
all of them anyway.

```text
trait ComposableEmitter<'inp, L, Lang = ()>:
    Emitter + FullContainerEmitter + SeparatedEmitter
    + UnexpectedLeadingSeparatorEmitter + UnexpectedTrailingSeparatorEmitter + TooFewEmitter {}
```

[`CstEmitter`](crate::emitter::CstEmitter) is the exception that *binds* rather than defaults:
its methods have no-op defaults (so `Fatal`/`Verbose`/`Silent` are `CstEmitter` for free and run
tree-less at zero cost), but a CST-producing parse path bounds `Ctx::Emitter: CstEmitter` so a
non-forwarding wrapper is a compile error rather than a silently empty tree. The recording
implementation is the `rowan`-gated `cst::Sink`; see [`crate::cst`] and the
[lossless-CST material](crate::parser::node).

### Built-in emitters

| Emitter | `Error` | Behavior | Reach for it when |
|---------|---------|----------|-------------------|
| [`Fatal<E, Lang=()>`](crate::emitter::Fatal) | `E` | returns the error, so `?` ends the parse; stores nothing, allocates nothing | the first error ends the job (config, query, protocol frame) |
| [`Verbose<E, S=SimpleSpan, Lang=()>`](crate::emitter::Verbose) | `E` | records every diagnostic, span-keyed, and continues | a human reads the output (compiler, IDE) — needs `std`/`alloc` |
| [`Silent<E, Lang=()>`](crate::emitter::Silent) | `E` | drops every diagnostic; keeps the error type | best-effort parse where diagnostics are unwanted |
| [`Ignored`](crate::emitter::Ignored) | `()` | drops everything; the error type collapses to `()` | you want the value, never the errors |
| `cst::Sink` (`rowan`) | inner's | a tree-building emitter: buffers CST events on the rewind timeline, forwarding diagnostics to an inner emitter | building a lossless syntax tree ([`crate::cst`]) |

`Fatal` and `Silent` are stateless (`Fatal::new()`, `Silent::new()`); `Verbose::new()` starts an
empty collection. A custom emitter implements the base [`Emitter`](crate::Emitter) plus whichever
capability sub-traits its parsers require — the [`FromEmitterError`](crate::emitter::FromEmitterError)
blanket means the base surface is often the only genuinely new code.

### Reading a collected harvest

[`Verbose`](crate::emitter::Verbose) exposes span-keyed channels — `errors()`, `warnings()`,
`labels()` (parallel to `errors()`), `skipped_regions()` — plus
[`diagnostics()`](crate::emitter::Verbose::diagnostics), which replays *every* channel
interleaved in true emission order as [`Diagnostic`](crate::emitter::Diagnostic) values a
renderer can consume.

| Read-side type | What it is |
|----------------|-----------|
| [`Severity`](crate::emitter::Severity) | the two tiers — `Error` / `Warning`; a classification, not a control-flow decision |
| [`Diagnostic<'a, S, E>`](crate::emitter::Diagnostic) | one borrowed record: `.span()`, `.labels()`, `.kind()`, `.severity()`, `.payload()` |
| [`DiagnosticKind<'a, E>`](crate::emitter::DiagnosticKind) | `Error(&E)` / `Warning(&E)` / `SkippedRegion(usize)` |
| [`Diagnostics<'a, S, E>`](crate::emitter::Diagnostics) | the emission-order iterator, from `diagnostics()` |

The example runs **one** generic parser under two emitters — fail-fast, then collecting:

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{UnexpectedEot, syntax::{FullContainer, MissingSyntax, TooFew, TooMany}, token::{MissingToken, SeparatedError, UnexpectedToken}},
#   punct::{Comma, OpenBracket, CloseBracket, OpenParen, CloseParen, Semicolon},
#   span::Span as _,
#   token::PunctuatorToken,
# };
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for Error { fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { Error } }
# impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for Error { fn from(_: MissingToken<'a, K, O, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Error { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Error { fn from(_: MissingSyntax<O, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Error { fn from(_: FullContainer<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Error { fn from(_: TooFew<S, Lang>) -> Self { Error } }
# impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Error { fn from(_: TooMany<S, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(u32), Ident(char), Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Ident, Comma, Semi, Plus, Star, LParen, RParen, LBracket, RBracket }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self {
#     Tok::Digit(_) => Kind::Digit, Tok::Ident(_) => Kind::Ident, Tok::Comma => Kind::Comma,
#     Tok::Semi => Kind::Semi, Tok::Plus => Kind::Plus, Tok::Star => Kind::Star,
#     Tok::LParen => Kind::LParen, Tok::RParen => Kind::RParen,
#     Tok::LBracket => Kind::LBracket, Tok::RBracket => Kind::RBracket } }
#   fn is_trivia(&self) -> bool { false }
# }
# impl PunctuatorToken<'_> for Tok {
#   fn comma() -> Option<Kind> { Some(Kind::Comma) }
#   fn semicolon() -> Option<Kind> { Some(Kind::Semi) }
#   fn open_paren() -> Option<Kind> { Some(Kind::LParen) }
#   fn close_paren() -> Option<Kind> { Some(Kind::RParen) }
#   fn open_bracket() -> Option<Kind> { Some(Kind::LBracket) }
#   fn close_bracket() -> Option<Kind> { Some(Kind::RBracket) }
# }
# impl From<Comma<(), (), ()>> for Kind { fn from(_: Comma<(), (), ()>) -> Self { Kind::Comma } }
# impl From<Semicolon<(), (), ()>> for Kind { fn from(_: Semicolon<(), (), ()>) -> Self { Kind::Semi } }
# impl From<OpenParen<(), (), ()>> for Kind { fn from(_: OpenParen<(), (), ()>) -> Self { Kind::LParen } }
# impl From<CloseParen<(), (), ()>> for Kind { fn from(_: CloseParen<(), (), ()>) -> Self { Kind::RParen } }
# impl From<OpenBracket<(), (), ()>> for Kind { fn from(_: OpenBracket<(), (), ()>) -> Self { Kind::LBracket } }
# impl From<CloseBracket<(), (), ()>> for Kind { fn from(_: CloseBracket<(), (), ()>) -> Self { Kind::RBracket } }
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
#       '0'..='9' => Tok::Digit(c as u32 - '0' as u32),
#       ',' => Tok::Comma, ';' => Tok::Semi, '+' => Tok::Plus, '*' => Tok::Star,
#       '(' => Tok::LParen, ')' => Tok::RParen, '[' => Tok::LBracket, ']' => Tok::RBracket,
#       c => Tok::Ident(c),
#     }))
#   }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
use tokora::{
    Emitter, Parse, ParseContext, Parser,
    cache::DefaultCache,
    emitter::{Severity, Verbose},
    span::Spanned,
};

// One parser, written generic over the context, so the *same* code runs fail-fast or collecting.
// The `Ctx::Emitter` bound pins the error type your `From` impls target.
fn line<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CharLexer<'inp>, Ctx>) -> Result<u32, Error>
where
    Ctx: ParseContext<'inp, CharLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, CharLexer<'inp>, Error = Error>,
{
    let at = *inp.span();
    match inp.try_expect(|t| matches!(t.data(), Tok::Digit(_)))? {
        Some(sp) => Ok(match sp.into_data() { Tok::Digit(n) => n, _ => unreachable!() }),
        None => {
            // The one line the emitter reinterprets: under `Fatal` this `?` ends the parse;
            // under `Verbose` the diagnostic is filed and we return a recovered value.
            inp.emitter().emit_error(Spanned::new(at, Error))?;
            Ok(0)
        }
    }
}

// `Fatal` — what `Parser::new()` installs: the first diagnostic is the `Err` you already handle.
assert_eq!(Parser::new().apply(line).parse_str("*"), Err(Error));

// `Verbose` — the very same `line`, run to completion; read the harvest afterwards.
let mut errors = Verbose::<Error>::new();
let cache = DefaultCache::<'_, CharLexer<'_>>::default();
let value = Parser::with_context((&mut errors, cache)).apply(line).parse_str("*").unwrap();
assert_eq!(value, 0); // the recovered value; the parse did not fail
assert_eq!(errors.errors().values().flatten().count(), 1);
let tiers: Vec<Severity> = errors.diagnostics().map(|d| d.severity()).collect();
assert_eq!(tiers, [Severity::Error]);
```

---

## ParseContext / ComposableParseContext

Every parser signature in this book carries a `Ctx` type parameter, yet the tutorials never say
what it is. A **parse context** is the bundle that supplies the two things a parse needs beyond
the lexer: the **emitter** (above) and the lookahead **cache**. Signatures are generic over it so
one parser can run under any emitter/cache pairing; `provide()` hands the pair to the input layer.

```text
trait ParseContext<'inp, L, Lang = ()> {
    type Emitter: Emitter<'inp, L, Lang>;    // the diagnostic policy
    type Cache:   Cache<'inp, L, Lang>;      // the lookahead buffer
    fn provide(self) -> InputContext<Self::Emitter, Self::Cache>;
}
```

Two blanket impls cover the common cases, and one concrete carrier holds a custom pairing:

```text
()      ...............  Fatal<Error> + DefaultCache      // the zero-config default
(E, C)  ...............  your emitter E + your cache C     // an ad-hoc pair (as `with_context` takes)

struct ParserContext<'inp, L, E, C = DefaultCache<'inp, L>, Lang = ()>;   // the concrete carrier
    ParserContext::new(emitter)                     // default cache
    ParserContext::with_cache_options(emitter, o)   // tuned cache

type FatalContext<'inp, L, Error, Lang = ()>
      = ParserContext<'inp, L, Fatal<Error, Lang>, DefaultCache<'inp, L>, Lang>;   // the common alias
```

### `ComposableParseContext` — the one bound a grammar function needs

A parser needs two orthogonal things of its context: an emitter that can **route** every
diagnostic, and an error type that can **absorb** one. Spelled out, that is the collecting-emitter
ladder the [emitters](#emitters) section listed *plus* the five `From`s from
[the error model](#the-error-model) — at every generic parser.
[`ComposableParseContext`](crate::ComposableParseContext) is both halves as one bound: it rides
[`ComposableEmitter`](crate::emitter::ComposableEmitter) on the context's emitter and
[`FromTokenErrors`](crate::emitter::FromTokenErrors) on that emitter's `Error`. It is
blanket-implemented for every qualifying [`ParseContext`](crate::ParseContext) (the one extra
requirement, `SliceOf<'inp, L>: Clone`, lives on the blanket impl).

```text
trait ComposableParseContext<'inp, L, Lang = ()>:
    ParseContext<'inp, L, Lang,
        Emitter: ComposableEmitter<'inp, L, Lang,
            Error: FromTokenErrors<'inp, L, Lang>>> {}
```

Both halves are **nested associated-type bounds**, not a free-standing
`where ErrorOf<…>: …` clause, and that is load-bearing: rustc does not elaborate a where-clause
predicate whose self type is a projection, so a free clause would make the obligation reappear at
every use site — which is exactly the restatement this bundle exists to remove.

A context whose error cannot absorb, say, an end-of-input **stops qualifying**. That is the
intended break: such a context could never have run a real grammar, and it now fails at the bound
rather than hundreds of frames deep at the first leaf atom that raises one.

### Aliases

- [`ErrorOf<'inp, L, Ctx, Lang>`](crate::ErrorOf) — the context emitter's `Error`, i.e.
  `<Ctx::Emitter as Emitter<'inp, L, Lang>>::Error`, so a return type stays
  `Result<T, ErrorOf<'inp, L, Ctx, ()>>` instead of the nested projection.
- [`SliceOf<'inp, L>`](crate::lexer::SliceOf) — the lexer's borrowed source-slice type (`&str`,
  `&[u8]`, …); the `Clone` bound `ComposableParseContext` needs.

### Naming it in your own parser fn

| Idiom | Signature shape | Use when |
|-------|-----------------|----------|
| **Concrete** | `InputRef<'a, '_, MyLexer<'a>, FatalContext<'a, MyLexer<'a>, MyError>>` | one fixed emitter (the error-model example above) |
| **Generic context** | `where Ctx: ParseContext<'inp, L>, Ctx::Emitter: Emitter<'inp, L, Error = MyError>` | reusable across emitters (the emitter example above; [chapter 7](super::ch07_diagnostics)) |
| **+ the whole leaf surface** | add `Ctx: ComposableParseContext<'inp, L>` | you drive `separated` / `repeated` / the `many/` builder, a delimited shape, or any leaf atom — it supplies the emitter family *and* the five error conversions, so nothing is restated |

The two aliases and the `ComposableParseContext` elaboration, on their own — no lexer scaffold needed:

```rust
use tokora::{Emitter, ErrorOf, Lexer, ParseContext, ComposableParseContext};
use tokora::emitter::{SeparatedEmitter, TooFewEmitter};

// `ErrorOf` is definitionally the context emitter's `Error` — this identity typechecks
// precisely because the two spellings are the same type.
fn error_of<'inp, L, Ctx>(
    e: <Ctx::Emitter as Emitter<'inp, L>>::Error,
) -> ErrorOf<'inp, L, Ctx, ()>
where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L>,
{
    e
}

// A single `Ctx: ComposableParseContext` elaborates to the whole collecting-emitter family: this body
// calls into code that demands individual capabilities of `Ctx::Emitter`.
fn collecting<'inp, L, Ctx>()
where
    L: Lexer<'inp>,
    Ctx: ComposableParseContext<'inp, L>,
{
    fn needs_family<'inp, L, E>()
    where
        L: Lexer<'inp>,
        E: SeparatedEmitter<'inp, L> + TooFewEmitter<'inp, L>,
    {
    }
    needs_family::<L, Ctx::Emitter>();
}
```
