# Reference: types & syntax building blocks

Two small, entirely **opt-in** modules round out tokora's public surface:
[`types`](crate::types) supplies reusable AST node shapes — identifiers, keywords, a family of
literals, an already-recovered wrapper — and [`syntax`](crate::syntax) supplies a pattern for
reporting *every* missing part of a multi-component construct in one error instead of stopping at
the first. Nothing elsewhere in tokora requires either module: the combinators taught from
[chapter 2](super::ch02_parsers) onward hand you raw tokens and spans, and what you build from
them is entirely up to you. These two modules exist so you don't have to reinvent "a name with a
span" or "a decimal literal" for every language you write a parser for.

This chapter catalogs the building blocks themselves. It does not repeat the combinator surface
([combinator reference](super::ref_combinators)), the error taxonomy or emitter capabilities
([errors, emitters & context reference](super::ref_errors_emitters_context)), or Pratt parsing
([Pratt reference](super::ref_pratt)) — reach for those chapters for everything *around* these
types.

## How to read this reference

- **Signatures** are trimmed (defaults, derives, and `Self: Sized` are elided) in `text` blocks;
  the compiling ` ```rust ` blocks show minimal, real uses.
- Almost everything here is a plain value: construct one, read it back, map it. None of this
  chapter's compiling examples need a running parser. Where a type *is* also produced by a
  combinator, the entry point is shown as a trimmed signature with a cross-link to a chapter that
  exercises it live, rather than repeating the parser scaffold here.
- The AST node types (`Ident`, `Keyword`, every `Lit*`, `IdentList`) carry a language marker
  `Lang: ?Sized = ()`, the same `Lang` convention from the
  [combinator reference](super::ref_combinators); the span/location wrappers (`Spanned`, `Sliced`,
  `Located`, `Recoverable`) do not. The examples below default `Lang` to `()` or fix it to one
  concrete marker type, whichever reads more clearly for that type.

## Span, offset & location primitives

Every type in this chapter carries a *span* — but tokora does not hardcode what a span is made of.
[`Span`](crate::Span) is a trait, implemented by the crate's own [`SimpleSpan`](crate::SimpleSpan)
and by `core::ops::Range<usize>`, so generic code can be written once against `S: Span` and used
with either (or a span type you write yourself).

```text
trait Span {
    type Offset: Ord + Clone + Hash;
    fn new(start: Self::Offset, end: Self::Offset) -> Self;
    fn start(&self) -> Self::Offset;        fn end(&self) -> Self::Offset;
    fn start_ref/end_ref(&self) -> &Self::Offset;
    fn start_mut/end_mut(&mut self) -> &mut Self::Offset;
    fn into_start/into_end(self) -> Self::Offset;
    fn into_range(self) -> Range<Self::Offset>;
    fn bump(&mut self, n: &Self::Offset);   // relocate: shift start AND end, length preserved
}
```

```rust
use tokora::{SimpleSpan, Span};

// Generic over any span representation — this is the whole point of the trait.
fn offsets<S: Span>(span: &S) -> (S::Offset, S::Offset) {
    (span.start(), span.end())
}

assert_eq!(offsets(&SimpleSpan::new(2, 7)), (2, 7));
assert_eq!(offsets(&(2usize..7)), (2, 7)); // `Range<usize>` implements `Span` too
```

[`SimpleSpan<Offset = usize>`](crate::SimpleSpan) is tokora's own span: two offsets, `Copy`,
`Ord`, `Hash`. Beyond the trait, it carries a fuller const-fn API of its own, where `bump`,
`bump_start`, and `bump_end` differ in what they move:

```text
SimpleSpan::new(start, end) -> Self          // panics if end < start
    .start() / .end() -> Offset (Copy)        .len() -> Offset       .is_empty() -> bool
    .bump(&n)         // relocate: start += n, end += n   (length preserved)
    .bump_start(n)    // grow from the left: start += n   (length shrinks)
    .bump_end(n)      // grow from the right: end += n    (length grows)
```

```rust
use tokora::SimpleSpan;

let mut span = SimpleSpan::new(5, 15);
assert_eq!(span.len(), 10);

span.bump(&3); // both ends move — same length
assert_eq!(span, SimpleSpan::new(8, 18));

span.bump_end(2); // only the end moves — grows
assert_eq!(span, SimpleSpan::new(8, 20));
```

[`AsSpan<Span>`](crate::span::AsSpan) pulls a span back out of anything that carries one —
`Ident`, `Keyword`, every `Lit*`, `IdentList`, `Spanned`, and `Located` all implement it (`Sliced`
has no span to give; `Recoverable`, further below, forwards it only when its payload has one).
[`IntoSpan<Span>`](crate::span::IntoSpan) is the consuming counterpart; currently only `Spanned`
implements it.

[`Spanned<D, S = SimpleSpan>`](crate::span::Spanned), [`Sliced<D, Src = ()>`](crate::slice::Sliced),
and [`Located<D, Sp = SimpleSpan, Sl = ()>`](crate::Located) are the three ready-made wrappers —
what [`.spanned()`/`.sliced()`/`.located()`](super::ref_combinators) hand you — pairing a value
with, respectively, its span, its captured source text, or both (`Spanned`'s fields are public;
`Sliced`/`Located` keep theirs private behind accessors):

```rust
use tokora::{Located, SimpleSpan, slice::Sliced, span::Spanned, utils::IntoComponents};

// `Spanned` — a value plus the span it came from.
let spanned = Spanned::new(SimpleSpan::new(10, 15), "hello");
assert_eq!(spanned.span(), SimpleSpan::new(10, 15));
assert_eq!(*spanned, "hello"); // Deref to the data

// `Sliced` — a value plus the source text/slice it came from.
let sliced = Sliced::new("config.toml", 42);
assert_eq!(sliced.slice(), "config.toml");

// `Located` — both at once: which source, and where in it.
let located = Located::new("main.rs", SimpleSpan::new(0, 5), "value");
assert_eq!((located.slice(), located.span()), ("main.rs", SimpleSpan::new(0, 5)));

// All three destructure via `IntoComponents`.
let (span, data) = spanned.into_components();
assert_eq!((span, data), (SimpleSpan::new(10, 15), "hello"));
```

## Identifiers & keywords

[`Ident<S, Span = SimpleSpan, Lang: ?Sized = ()>`](crate::types::Ident) and
[`Keyword<S, Span = SimpleSpan, Lang: ?Sized = ()>`](crate::types::Keyword) share a shape: a
source value `S` (a `&str` slice, an owned `String`, an interned symbol — anything), a span, and a
language marker. **Careful with the letter `S`**: here it names the *source*, and the span is the
second parameter, spelled `Span`. The literals further below flip this — their `S` is the span.
Read the parameter's *name*, not just its letter.

```text
impl<S, Span, Lang> Ident<S, Span, Lang> {
    const fn new(span: Span, source: S) -> Self;           // status: Valid
    const fn span(&self) -> Span where Span: Copy;         // + span_ref / span_mut
    const fn source(&self) -> S where S: Copy;             // + source_ref / source_mut
    fn bump(&mut self, by: &Span::Offset) -> &mut Self where Span: crate::Span;
    fn map<U>(self, f: impl FnOnce(S) -> U) -> Ident<U, Span, Lang>;
}

// Construction WITH a chosen status is a trait method too, and for the same reason: an inherent
// `with_status(.., Status)` captures a type-directed argument — an unchanged
// `unsafe { zeroed() }` infers as the consumer's status before the upgrade and as tokora's
// after, both compile, and a zero-valued rejection becomes Valid.
impl FromComponents for Ident<..> { fn from_components(c: Self::Components) -> Self; }
// Components is a NAMED STRUCT { span, payload, status }, not a 3-tuple: `let (_, .., v) = ..`
// binds the payload against a pair and the status against a triple, and both compile.

// The recovery state is read through a trait, and ONLY through it — there is no inherent
// accessor of any name, because an inherent one can be displaced by a consumer's extension
// method whenever the two return types share a method (`x.status().is_valid()` typechecks
// either way). The trait has to be imported, which is what makes a clash loud.
impl RecoveryState for Ident<..> { fn status(&self) -> Status;
                                   fn is_valid/is_error/is_missing(&self) -> bool; }
// Cost: none of this is `const` — a trait method cannot be.

// Keyword and every `Lit*` type carry the same status and the same two doors to it, so
// converting a Keyword into an Ident via `From` carries the state across rather than declaring
// the result valid. `bump` is Ident's alone.
//
// `IdentList` keeps is_valid/is_error/is_missing as INHERENT methods and does not implement the
// trait: a list is an aggregate, and is_error and is_missing can both be true of one at once,
// which no single Status can say.
```

```rust
use tokora::{SimpleSpan, error::ErrorNode, types::{Ident, Keyword}, utils::IntoComponents};
// `RecoveryState` is NOT in `types::*` — a trait reached through a glob can be rebound by a
// second glob with only a warning, so it has to be named:
use tokora::types::{Components, FromComponents, RecoveryState};

struct MyLang;

let ident = Ident::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(5, 11), "my_var");
assert_eq!(ident.source_ref(), &"my_var");
assert!(ident.is_valid());

// `error`/`missing` build typed placeholders instead of failing outright — the source
// type's own `ErrorNode` impl supplies the text (`&str`'s is `"<error>"`/`"<missing>"`).
let bad = Ident::<&str, SimpleSpan, MyLang>::error(SimpleSpan::new(0, 3));
assert!(bad.is_error());
assert_eq!(bad.source_ref(), &"<error>");

// `Keyword` converts into `Ident` for free.
let kw = Keyword::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "let");
let as_ident: Ident<&str, SimpleSpan, MyLang> = kw.into();
assert_eq!(as_ident.source_ref(), &"let");

// Both destructure via `IntoComponents`, into span, payload AND status. The status is in the
// tuple because `FromComponents` is the inverse: rebuilding through `new` would declare a
// recovered node valid, which is the laundering the three-part decomposition exists to prevent.
let Components { span, payload, status } = ident.into_components();
let upper = Ident::<&str, SimpleSpan, MyLang>::from_components(Components { span, payload, status })
    .map(|s| s.to_uppercase());
assert_eq!(upper.source_ref(), "MY_VAR");
assert!(upper.is_valid());

let parts = bad.into_components();
assert!(Ident::<&str, SimpleSpan, MyLang>::from_components(parts).is_error());
```

Both also have real combinator entry points, not just bare constructors. Once the token type opts
in by implementing [`IdentifierToken`](crate::token::IdentifierToken) /
[`KeywordToken`](crate::token::KeywordToken) — the [custom-lexer recipe](super::recipe_custom_lexer)
implements both — `Ident::<(), ()>` and `Keyword::<(), ()>` host parsers that read the next token
and wrap it:

```text
Ident::<(), ()>::parse(inp)       -> Result<Ident<Slice, L::Span, Lang>, Error>        // errors on mismatch/EOI
Ident::<(), ()>::try_parse(inp)   -> Result<ParseAttempt<Ident<Slice, L::Span, Lang>>, Error>  // declines instead
Keyword::<(), ()>::parse(inp)     -> Result<Keyword<L::Token, L::Span, Lang>, Error>   // captures the WHOLE token
Keyword::<(), ()>::try_parse(inp) -> Result<ParseAttempt<Keyword<L::Token, L::Span, Lang>>, Error>
// one spelling each: `Lang` is read off `inp`, so `()` and a brand look identical at the call
```

[`IdentList<S, Span = SimpleSpan, Container = Vec<Ident<S, Span>>, Lang: ?Sized = ()>`](crate::types::IdentList)
aggregates already-parsed identifiers. It stores no status of its own — `is_valid`/`is_error`/
`is_missing` scan the elements on every call, so the list's answer is its segments':

```rust
use tokora::{SimpleSpan, error::ErrorNode, types::{Ident, IdentList}};

let idents = vec![
    Ident::<&str>::new(SimpleSpan::new(0, 3), "foo"),
    Ident::<&str>::error(SimpleSpan::new(4, 7)), // recovered from a malformed segment
];
let list = IdentList::<&str>::new(SimpleSpan::new(0, 7), idents);
assert_eq!(list.identifiers_slice().len(), 2);
assert!(!list.is_valid()); // false as soon as one element is
assert!(list.is_error());
```

Built by [`try_ident_list`](crate::parser::try_ident_list) in the
[combinator reference](super::ref_combinators) when every token is an `IdentifierToken`.

## Literals

One internal macro generates 17 near-identical literal types, covering the categories most
languages need:

| Type | `D` default | Example |
|------|:-----------:|---------|
| [`Lit`](crate::types::Lit) | — | any literal, undistinguished |
| [`LitDecimal`](crate::types::LitDecimal) | — | `42`, `1_000` |
| [`LitHex`](crate::types::LitHex) | — | `0xFF` |
| [`LitOctal`](crate::types::LitOctal) | — | `0o77` |
| [`LitBinary`](crate::types::LitBinary) | — | `0b1010` |
| [`LitFloat`](crate::types::LitFloat) | — | `3.14` |
| [`LitHexFloat`](crate::types::LitHexFloat) | — | `0x1.8p3` |
| [`LitString`](crate::types::LitString) | — | `"hello"` |
| [`LitMultilineString`](crate::types::LitMultilineString) | — | `"""..."""` |
| [`LitRawString`](crate::types::LitRawString) | — | `r"C:\path"` |
| [`LitChar`](crate::types::LitChar) | `char` | `'a'` |
| [`LitByte`](crate::types::LitByte) | `u8` | `b'a'` |
| [`LitByteString`](crate::types::LitByteString) | — | `b"bytes"` |
| [`LitBool`](crate::types::LitBool) | `bool` | `true` / `false` |
| [`LitTrue`](crate::types::LitTrue) | `()` | `true` |
| [`LitFalse`](crate::types::LitFalse) | `()` | `false` |
| [`LitNull`](crate::types::LitNull) | `()` | `null` / `nil` / `None` |

Unlike `Ident`/`Keyword`, **no combinator produces these** — every one is bring-your-own, typically
built inside a [`.map()`/`.map_with()`](super::ch03_combinators) over a raw token or a captured
slice.

```text
struct Name<D $(= default)?, S = SimpleSpan, Lang = ()> { .. }   // note: S is the SPAN here
impl<D, S, Lang> Name<D, S, Lang> {
    const fn new(span: S, data: D) -> Self;
    const fn span(&self) -> S where S: Copy;        // + span_ref / span_mut
    const fn data(&self) -> D where D: Copy;         // + data_ref / data_mut
    fn bump(&mut self, by: &S::Offset) -> &mut Self where S: crate::Span;
}
impl<D, S, Lang> ErrorNode<S> for Name<D, S, Lang> where D: ErrorNode<S>, S: Clone { .. }
```

The type-parameter order is the flip of `Ident`/`Keyword`: here `D` (the payload) comes first and
the **span** is the parameter named `S`. Same crate, two different things called `S` — the table
above and each parameter's own name are the only reliable guide, not the letter.

```rust
use tokora::{SimpleSpan, error::ErrorNode, types::{LitBool, LitChar, LitDecimal}};

struct MyLang;

let dec = LitDecimal::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 2), "42");
assert_eq!(dec.data_ref(), &"42");

// `D` need not be raw text — plug in an already-parsed value.
let flag = LitBool::<bool, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 4), true);
assert!(flag.data());

let ch = LitChar::<char, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), 'a'); // `char` is D's default
assert_eq!(ch.data(), 'a');

// Error recovery, same contract as `Ident`/`Keyword`.
let bad = LitDecimal::<&str, SimpleSpan, MyLang>::error(SimpleSpan::new(5, 8));
assert_eq!(bad.data_ref(), &"<error>");
```

## Error recovery: `ErrorNode` and `Recoverable`

Every type above implements [`ErrorNode<S = SimpleSpan>`](crate::error::ErrorNode) once its
payload does — the trait behind every `::error(span)`/`::missing(span)` call used so far:

```text
trait ErrorNode<S = SimpleSpan> {
    fn error(span: S) -> Self;     // malformed: content was there, but wrong
    fn missing(span: S) -> Self;   // absent: nothing was there at all
}
```

Built in for `&str`/`&[u8]` (→ `"<error>"`/`"<missing>"`, `b"<error>"`/`b"<missing>"`), plus
`bytes::Bytes` and `hipstr`'s `HipStr`/`HipByt` under their feature flags (the same backends the
[Source, Slice & storage backends](super::arch_source_slice) chapter catalogs). This is the
value-level half of recovery; the combinators that actually keep a parse going past a failure —
`recover`, `inplace_recover`, `sync_balanced` (all taught in [chapter 8](super::ch08_recovery)) —
are what call `error`/`missing` to manufacture the placeholder your AST needs instead of aborting.

[`Recoverable<T, S = SimpleSpan>`](crate::types::Recoverable) packages the same three outcomes as
one enum, for AST nodes that would rather match than ask `is_error()`/`is_missing()`:

```text
enum Recoverable<T, S = SimpleSpan> { Node(T), Error(S), Missing(S) }
// + is_node/is_error/is_missing (derived), try_unwrap_node -> Result<T, _>, unwrap_node -> T (panics),
//   From<T> for Recoverable<T>, ErrorNode for Recoverable<T> (span-only variants)
```

```rust
use tokora::{SimpleSpan, error::ErrorNode, types::Recoverable};

let ok: Recoverable<i32> = 42.into();
let bad: Recoverable<i32> = Recoverable::error(SimpleSpan::new(0, 3));
let gone: Recoverable<i32> = Recoverable::missing(SimpleSpan::new(3, 3));

assert!(ok.is_node());
assert!(bad.is_error());
assert!(gone.is_missing());
assert_eq!(ok.try_unwrap_node(), Ok(42));
```

When `T: Syntax` (next section) or `T: AsSpan<S>`, `Recoverable<T, S>` forwards the impl — so a
`Recoverable<IfExpr>` is itself a `Syntax`, and its span comes from whichever variant is active.

## Collecting every missing part: `Syntax`, `AstNode`, `Language`

A construct with several required parts — an `if` needs a condition and a body, a `let` needs a
name, an `=`, and an initializer — reports better diagnostics by naming *every* part that turned
out missing in one error, instead of stopping at the first. `syntax` is the trait pattern for
that; [`error::IncompleteSyntax`](crate::error::IncompleteSyntax) is the error type that
accumulates the result.

[`Language`](crate::syntax::Language) comes first — `Syntax` is generic over it:

```text
trait Language: Sized + Copy + Debug + Eq + Ord + Hash {
    type SyntaxKind: Sized + Copy + Debug + Eq + Ord + Hash;
}
```

Implement it once per language or dialect; `SyntaxKind` is usually the same node-kind enum a
lossless CST would use (a `rowan::Language` implementor gets this for free when the `rowan`
feature is on — the blanket impl is not shown here since this chapter does not depend on that
feature).

```text
trait Syntax {
    type Lang: Language;
    const KIND: <Self::Lang as Language>::SyntaxKind;
    type Component: Display + Debug + Clone + PartialEq + Eq + Hash;   // usually an enum
    type COMPONENTS: ArrayLength;   // type-level component count (typenum, via generic-arraydeque)
    type REQUIRED: ArrayLength;     // type-level count of the required subset
    fn possible_components() -> &'static GenericArrayDeque<Self::Component, Self::COMPONENTS>;
    fn required_components() -> &'static GenericArrayDeque<Self::Component, Self::REQUIRED>;
}
trait AstNode<Lang> { type Syntax: Syntax<Lang = Lang>; }  // bridge: AST node type -> its Syntax
```

[`AstNode`](crate::syntax::AstNode) is a thin bridge, not a requirement: implement it so generic
code can go from an AST node type `T` to `T::Syntax` (and from there to
`IncompleteSyntax<T::Syntax>`) without matching on concrete node types.

```rust
use core::fmt;
use tokora::{
    SimpleSpan,
    error::IncompleteSyntax,
    syntax::{Language, Syntax},
    utils::{GenericArrayDeque, typenum::U2},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MyLang;
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Kind { IfExpr }
impl Language for MyLang {
    type SyntaxKind = Kind;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum IfComponent { Condition, ThenBranch }
impl fmt::Display for IfComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Condition => "condition",
            Self::ThenBranch => "then-branch",
        })
    }
}

struct IfExpr;
impl Syntax for IfExpr {
    type Lang = MyLang;
    const KIND: Kind = Kind::IfExpr;
    type Component = IfComponent;
    type COMPONENTS = U2;
    type REQUIRED = U2;

    fn possible_components() -> &'static GenericArrayDeque<IfComponent, U2> {
        const ALL: &GenericArrayDeque<IfComponent, U2> =
            &GenericArrayDeque::from_array([IfComponent::Condition, IfComponent::ThenBranch]);
        ALL
    }
    fn required_components() -> &'static GenericArrayDeque<IfComponent, U2> {
        Self::possible_components()
    }
}

// Parsed an `if` with a missing then-branch: record it and keep going instead of aborting.
let mut error = IncompleteSyntax::<IfExpr>::new(SimpleSpan::new(0, 8), IfComponent::ThenBranch);
assert_eq!(error.len(), 1);
assert!(!error.is_full());
assert_eq!(error.to_string(), "incomplete syntax: component then-branch is missing");

// A second pass finds the condition missing too — same error, one more component.
error.push(IfComponent::Condition);
assert_eq!(error.len(), 2);
assert!(error.is_full()); // == IfExpr::COMPONENTS::USIZE
```

[`IncompleteSyntax::new`](crate::error::IncompleteSyntax::new) always starts with one component;
[`push`](crate::error::IncompleteSyntax::push) records another (a duplicate is a no-op; pushing
past capacity panics), and its `Display` renders "component X is missing" or "components X, Y, …
are missing" depending on how many accumulated.

## See also

- [Combinator & atom reference](super::ref_combinators): the `Lang` convention these types
  share, and `try_ident_list` — the one combinator that builds an `IdentList` for you.
- [Errors, emitters & context reference](super::ref_errors_emitters_context): the error taxonomy
  and emitter capabilities that `ErrorNode` placeholders eventually flow into.
- [Recovery](super::ch08_recovery): the `recover`/`inplace_recover`/`sync_balanced` combinators
  that call `ErrorNode::error`/`::missing` to keep a parse going.
- [Recipe: writing a custom lexer](super::recipe_custom_lexer): a token implementing
  `IdentifierToken`/`KeywordToken`, the traits `Ident`'s and `Keyword`'s combinator entry points
  need.
- [Source, Slice & storage backends](super::arch_source_slice): the `bytes_1`/`hipstr_0_8`
  backends behind two of `ErrorNode`'s built-in implementations.
