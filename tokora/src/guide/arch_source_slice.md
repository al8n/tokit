# Source, Slice, and storage backends

Every parser in this book has been generic over a lifetime `'inp` and a lexer `L`, and has run
over `&str` without ever saying why it *could* run over anything else. This chapter is the why.

Tokora's engine — the on-demand, parse-while-lexing machinery introduced in
[chapter 2](super::ch02_parsers), and the immutable-slice model of
[chapter 9](super::ch09_streaming) — never touches a `str` or a `[u8]` directly. It reads the
input through two small traits, [`Source`](crate::Source) and [`Slice`](crate::Slice). Everything
else — owned or borrowed, text or raw bytes, `std` or bare-metal — is a matter of which type you
hand it.

Like [the previous chapter](super::arch_parsing_engine), this one keeps Part III's register: less
*here is how to use it*, more *here is why it is shaped this way*.

## The problem: one engine, two axes of representation

A parser combinator library that hard-codes `&str` cannot lex a binary format; one that
hard-codes `&[u8]` throws away UTF-8's guarantees and has to re-check code-point boundaries by
hand. And neither can accept an *owned, reference-counted* buffer of the kind an async I/O stack
hands you — the `bytes::Bytes` your socket already filled — without a copy back down to a borrow.

The input a real program has on hand varies along two independent axes:

- **owned vs borrowed** — a `&str` you are lending the parser, versus a `bytes::Bytes` or a
  `HipStr` the parser can hold a cheap clone of;
- **text-shaped vs byte-shaped** — a source whose atom is a Unicode scalar value (`char`, with
  code-point boundaries to respect) versus one whose atom is a raw `u8` (any index is a valid cut).

Tokora refuses to pick for you. Instead it names the *seam* — the handful of operations the engine
actually needs from an input — as a trait, implements that trait once for `str` and once for
`[u8]`, and lets feature-gated backends add more. A parser written against the seam is
representation-agnostic for free; it never mentions a concrete source type at all.

## The seam is two traits

The engine needs to ask two different questions, so there are two traits:

- [`Source`](crate::Source) is implemented on the **input medium** — the whole thing being lexed
  (`str`, `[u8]`, `bytes::Bytes`, …). It answers *how long are you, and give me the sub-range
  `a..b`*.
- [`Slice`](crate::Slice) is implemented on **what a span of that medium looks like** — the value
  a lexer yields for one token (`&str`, `&[u8]`, a cheap `Bytes` clone, …). It answers *what are
  your characters, and how many*.

They are bound together by one associated type: `Source::Slice<'a>: Slice<'a>`. Slicing a
`Source` produces something that is itself a `Slice`. That is the whole contract, and it is why
[`SliceOf<'inp, L>`](crate::SliceOf) — the projection `<L::Source as Source>::Slice<'inp>` — is
the type generic parser code reaches for whenever it wants the raw text of a token.

`'a` is a validity requirement, not merely a label on that projection:
`Slice<'a>: 'a` means that both the slice value and the data it represents remain valid for at
least `'a`. The canonical `Slice` implementations therefore live on the representations
themselves — `str`, `[u8]`, `BStr`, and the optional backend values — rather than on a selected
set of reference spellings. In particular, the lifetime-carrying `HipStr<'data>` and
`HipByt<'data>` implement `Slice<'source>` when `'data: 'source`.

Shared references are forwarded uniformly. If `T: Slice<'source>` and a reference to `T` lives
for `'data: 'source`, then `&'data T: Slice<'source>`; the same law applies repeatedly to nested
references. That makes `&str`, `&&str`, `&[u8]`, and borrowed backend values retain the
representation, character type, and iterator behavior of their underlying `T`.

### `Source`: addressing the medium

```rust,ignore
pub trait Source<Cursor>: core::fmt::Debug {
    type Slice<'source>: Slice<'source> where Self: 'source;

    fn is_empty(&self) -> bool;
    fn len(&self) -> Cursor;
    fn as_slice(&self) -> Self::Slice<'_>;
    fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
    where R: RangeBounds<Cursor>;

    fn find_boundary(&self, index: Cursor) -> Cursor { index } // default
    fn is_boundary(&self, index: Cursor) -> bool;
}
```

Three things are worth reading closely.

- **`Cursor` is a type parameter, not `usize`.** A `Source` is addressed by whatever offset type
  its lexer uses; the core impls fix it to `usize`, but the trait does not. This is the same
  `Offset` a [`Lexer`](crate::Lexer) declares (`Lexer::Source: Source<Lexer::Offset>`), so the
  offset arithmetic the engine does and the addressing the source understands are the same type by
  construction.
- **`slice` is fallible and zero-copy.** It returns `Option<Self::Slice<'_>>` — `None` for an
  out-of-range or (for text) boundary-splitting range, mirroring `slice::get`. The `'_` selects
  the associated slice for that call. Borrowing sources such as `str` tie it to the borrow of
  `self`; explicit reference sources such as `&'data str` may preserve their longer carried
  lifetime instead. Either way, a token payload can remain a view into the source rather than a
  fresh allocation.
- **`find_boundary` and `is_boundary` are the entire text/byte distinction.** This is the design
  decision that keeps the rest of the engine shape-blind.

`Source` deliberately does **not** have a blanket implementation for `&T`. Such an implementation
can only select `T::Slice` using the lifetime of the outer borrow, which shortens source-carried
lifetimes: slicing a briefly borrowed `&'data str` would produce a slice tied to the brief borrow
instead of `'data`.

The borrowed core media therefore have explicit implementations: `&'data str`, `&'data [u8]`,
and (with `bstr_1`) `&'data BStr` all return slices that preserve `'data`. Owned backends such as
`Bytes`, `HipStr`, and the smol-bytes types implement `Source` on the owner itself. They can still
be borrowed normally to call source methods, but `&Bytes` is intentionally not a distinct source
type. This keeps the associated lifetime truthful rather than trading it for blanket convenience.

### The boundary discipline

`is_boundary(i)` asks *is `i` a legal place to cut?* `find_boundary(i)` asks *what is the nearest
legal cut at or below `i`?* The two core impls answer differently, and that difference is the only
place in tokora where "text" and "bytes" mean different things:

| | `str` (text) | `[u8]` (bytes) |
|---|---|---|
| `is_boundary(i)` | `self.is_char_boundary(i)` | `i <= self.len()` |
| `find_boundary(i)` | round **down** to a code-point boundary | `i` unchanged (the default) |

For bytes every in-range index is a boundary, so `find_boundary` is the identity and costs
nothing. For text, `find_boundary` walks *down* from `i` until it lands on a code-point boundary
(indices at or past the end are returned unchanged, matching the byte behavior). A lexer that
advances by a byte count it computed from a regex can therefore call `find_boundary` and be
guaranteed a slice position that will not split a multi-byte scalar — the same call is a no-op on
a byte source and a safety net on a text source. This is why [`Lexer::bump`](crate::Lexer) can
promise it never lands "in the middle of a UTF-8 code point (does not apply when lexing raw
`&[u8]`)": the promise is delegated to the `Source` impl, made once, and inherited by every
backend of the same shape.

### `Slice`: reading a span

```rust,ignore
pub trait Slice<'source>: PartialEq + Eq + core::fmt::Debug + 'source {
    type Char: Copy + core::fmt::Debug + PartialEq + Eq + core::hash::Hash;
    type Iter<'a>: Iterator<Item = Self::Char> where Self: 'a;
    type PositionedIter<'a>: Iterator<Item = (usize, Self::Char)> where Self: 'a;

    fn iter<'a>(&'a self) -> Self::Iter<'a> where Self: 'a;
    fn positioned_iter<'a>(&'a self) -> Self::PositionedIter<'a> where Self: 'a;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool { self.len() == 0 } // default
}
```

`Slice::Char` is the shape marker in the type system: `char` for a text slice, `u8` for a byte
slice, and it must match the character type the underlying lexer works in. The two iterators are
the whole reading interface — `iter` for the characters, `positioned_iter` for
`(offset, character)` pairs — and the canonical core impls forward to the standard-library
iterators that already do the right thing: `str::Chars` / `str::CharIndices` for `str`, and
`Copied<slice::Iter>` / `Enumerate<…>` for `[u8]`. Shared-reference forwarding supplies their
usual `&str` and `&[u8]` forms without separate implementations. There is no bespoke UTF-8
decoding in tokora; `Slice` is a thin, uniform face over machinery `core` already ships.

(Do not confuse [`Slice`](crate::Slice) with the [`Sliced<D, Src>`](crate::slice::Sliced) struct
that lives in the same module. `Slice` is the *span of a source*; `Sliced` is an unrelated
provenance wrapper — a value paired with *which* source it came from, e.g. a file name — the
counterpart to [`Spanned`](crate::span::Spanned)'s *where within* a source.)

## The backends

Beyond the two always-present core impls, four optional crates each add `Source`/`Slice` impls for
their buffer types. Every one reuses the same iterator machinery as the core — the byte-shaped
backends borrow `<[u8]>::iter().copied()`, the text-shaped ones borrow `str::Chars` — so a backend
is really just *an owner type plus its slicing and its boundary rule*. Byte-shaped backends
delegate `is_boundary` straight to the `[u8]` rule (`i <= len`); the text-shaped ones
(`HipStr`, `Utf8Bytes`) replicate the `str` char-boundary logic verbatim.

| Backend | Feature | Type(s) | Owned / borrowed | Shape (`Char`) | `Slice<'a>` |
|---|---|---|---|---|---|
| core text | *(always on)* | `str` | borrowed | text — `char` | `&str` |
| core bytes | *(always on)* | `[u8]` | borrowed | bytes — `u8` | `&[u8]` |
| [`bytes`](https://docs.rs/bytes) | `bytes_1` | `Bytes` | owned (ref-counted) | bytes — `u8` | `Bytes` |
| [`bstr`](https://docs.rs/bstr) | `bstr_1` | `BStr` | borrowed | bytes — `u8` | `&'a BStr` |
| [`hipstr`](https://docs.rs/hipstr) | `hipstr_0_8` | `HipStr<'_>` | owned *or* borrowed | text — `char` | `HipStr<'_>` |
| [`hipstr`](https://docs.rs/hipstr) | `hipstr_0_8` | `HipByt<'_>` | owned *or* borrowed | bytes — `u8` | `HipByt<'_>` |
| [`smol-bytes`](https://docs.rs/smol-bytes) | `smol_bytes_0_1` | `shared::Bytes` | owned (ref-counted, ≤62 B inline) | bytes — `u8` | `shared::Bytes` |
| [`smol-bytes`](https://docs.rs/smol-bytes) | `smol_bytes_0_1` | `compact::Bytes` | owned (≤62 B inline) | bytes — `u8` | `compact::Bytes` |
| [`smol-bytes`](https://docs.rs/smol-bytes) | `smol_bytes_0_1` | `Utf8Bytes` | owned (ref-counted, ≤62 B inline) | text — `char` | `Utf8Bytes` |

The single most important column is **`Slice<'a>`**. For the borrowed backends it is a plain
reference, as you would expect. But for the *owned* backends it is the owner type again, not a
`&`-borrow — `bytes::Bytes::Slice = Bytes`, `HipStr::Slice = HipStr`, and so on. That is not a
copy: these types slice in O(1) by bumping a reference count (`bytes`, `smol-bytes` `shared`) or,
for a small enough span, by inlining ≤62 bytes into the handle (`smol-bytes`). An owned source is
therefore *still* zero-copy to slice — tokora does not force borrowing to get the property; it
lets each representation express its own cheapest slice, and refcounted buffers happen to have a
very cheap one.

A quick tour of what each backend is *for*:

- **`bytes_1`** exposes [`bytes::Bytes`](https://docs.rs/bytes) — the de-facto owned, cheaply
  cloneable byte buffer of the async ecosystem. Reach for it when the bytes you want to parse
  already arrived as a `Bytes` and you want to keep token slices alive past the parse without
  copying.
- **`bstr_1`** exposes [`bstr::BStr`](https://docs.rs/bstr), a borrowed, byte-shaped view whose
  slices are `&BStr`. It is the "bytes that are *conventionally* text but not guaranteed UTF-8"
  case; it retains that byte-string representation while forwarding its byte iteration and
  boundary behavior to the same underlying rules as `[u8]`.
- **`hipstr_0_8`** exposes the [`hipstr`](https://docs.rs/hipstr) inline-or-shared-or-borrowed
  hybrids in *both* shapes: `HipStr` (text) and `HipByt` (bytes). A `HipStr` may be a small string
  stored inline, a reference-counted heap share, or a borrow — and slicing it preserves whichever
  it is, so it is the flexible choice when you do not know in advance whether inputs are tiny or
  huge.
- **`smol_bytes_0_1`** exposes three impls from [`smol-bytes`](https://docs.rs/smol-bytes): the
  byte-shaped `shared::Bytes` (the default; ref-counted heap with a 62-byte inline
  small-buffer optimization, and zero-copy convertible with `bytes::Bytes`), the byte-shaped
  `compact::Bytes` (same inline threshold, but it re-inlines a shrinking view to *release* its
  allocation), and the text-shaped `Utf8Bytes` (a UTF-8 wrapper over the shared strategy — the
  `char`-shaped counterpart to `HipStr`). All three are owned and all three slice cheaply.

### Why the feature names carry versions

The feature that turns a backend on is `bytes_1`, not `bytes`; `hipstr_0_8`, not `hipstr`. Each
versioned feature enables a `package`-renamed optional dependency pinned to one SemVer-major line
of the upstream crate (`bytes_1 = { package = "bytes", version = "1", … }`), and a bare alias
forwards to the current one (`bytes = ["bytes_1"]`). This is the same discipline the crate uses
for the logos adapter (`logos_0_16`, the only supported major — 0.14 and 0.15 have been
retired): the version in the name is what lets a future major be added as a purely additive
feature, no breaking change to anyone pinned to the current one.

## `no_std` posture

The category list in `Cargo.toml` includes `no-std::no-alloc`, and that is a load-bearing claim,
not an aspiration. The two core impls — `Source`/`Slice` for `str` and `[u8]` — carry **no**
feature gate at all. They are `core`-only: no `std`, no `alloc`, no allocator. A parser whose
lexer sources from `&str` or `&[u8]` compiles and runs on bare metal, and that is the baseline the
whole abstraction rests on.

The feature graph layers up from there:

- **no feature** → `core` only. Core `str` / `[u8]` sources, the parser itself, and its
  stack-buffered lookahead machinery (peek windows of 1-32 tokens over a small inline
  cache) — no allocator in sight.

  Stack-buffered means the lookahead is on **your** stack, so the bound is worth stating.
  A `peek::<W>()` reserves **one** window and its worst case is the whole array live at
  once:

  ```text
  W::CAPACITY × size_of::<Maybe<CachedToken<&Token, &State, &Span>,
                                CachedToken<Token, State, Span>>>()
  ```

  which for every realistic type is `W::CAPACITY × (size_of::<Token>() + size_of::<State>() +
  size_of::<Span>())` plus per-entry padding and a discriminant. `W::CAPACITY` is at most
  **32** — `U32` is the widest window the crate offers — so that expression, evaluated at
  32, is the maximum a single peek can cost. Everything else on the frame is O(1) in the
  window width: single-entry temporaries, one clone of the lexer (`size_of::<L>()`, which
  contains `State`), and a small fixed part.

  The coefficient is **one**, not two, and that is recent: through 0.7.3 a peek that looked
  past the cache staged the extra tokens in a *second* `W::CAPACITY`-slot array, so a cache
  miss cost twice the figure above. At `U32` over a 1 KiB token beside a 1 KiB lexer state —
  2,072 bytes an entry — that second array was 66,312 bytes of stack nobody asked for, and the
  peek's owned storage was 132,632 bytes rather than 66,320. Those tokens are now staged in
  the window the caller already owns.

  `Token`, `State` and `Span` are yours and unconstrained in size, so this is the one place
  an embedded target has to do the arithmetic itself: pick the narrowest window that decides
  the production, and use `peek_kind` / `head_satisfies` — which run at `U1` — for a head
  test. Nothing here is heap-allocated and nothing scales with the length of the input.
- **`alloc`** → adds the allocator-backed pieces (growable containers, the session stack) while
  staying `no_std`.
- **`std`** (default) → everything, plus it turns on the upstream backends' own default features.

The backend features themselves are uniform about `std`: none implies it.

- `bytes_1`, `bstr_1`, `hipstr_0_8`, and `smol_bytes_0_1` all bring their crates in with
  `default-features = false`, so enabling a backend does **not** drag in `std`. Every backend
  compiles with neither `std` nor `alloc` turned on in tokora (the owned buffers still need a
  global allocator to *link* into a final binary, but tokora's feature graph leaves that choice
  to you rather than forcing it).
- `smol_bytes_0_1` additionally turns on smol-bytes' own `alloc` feature — the tier its buffer
  types live on — so it requires smol-bytes ≥ 0.1.2, the first `rlib`-only release with an
  `alloc` tier. (Earlier 0.1.x built a `cdylib` alongside the `rlib`, which needed `std`'s
  global allocator and panic handler even to *check*; that is why this feature once implied
  `std`.)

However far down you turn the graph, the parser you write does not change. It is generic over
`L: Lexer`, and `L::Source` is *some* `Source`; the concrete choice of representation lives at the
call site, not in the grammar.

### The entry-point family

The [`Parse`](crate::Parse) trait's methods are the ergonomic front door to all of this.
[`parse`](crate::Parse::parse) / [`parse_with_state`](crate::Parse::parse_with_state) are the
general form — they take `&L::Source` for whatever source your lexer declared, so a lexer whose
`Source` is `bytes::Bytes` or `smol_bytes::compact::Bytes` is driven through exactly these. On top
of them sit conveniences: [`parse_str`](crate::Parse::parse_str) and
[`parse_slice`](crate::Parse::parse_slice) for the core `str` / `[u8]` shapes, and — behind their
respective backend features — `parse_bytes`, `parse_bstr`, and `parse_hipstr`.

There is a subtlety worth naming, because it explains why those last three exist. `parse_bytes`,
`parse_bstr`, and `parse_hipstr` are *convenience over a core-sourced lexer*: each requires
`L::Source` to be `[u8]` (or `str`) and simply borrows your owned buffer down to `&[u8]` / `&str`
before parsing. They are for "I am holding a `bytes::Bytes` but my lexer reads `[u8]`." The
owned-type `Source` impls in the table above are the *other* path — they are what a lexer uses
when its `Source` associated type genuinely *is* the owned type, and it is that path that gives
you owned, refcount-sliced tokens.

## The abstraction, exercised

Nothing above needs a lexer to demonstrate — the `Source` and `Slice` traits stand on their own.
The behavior starts with canonical `str` and `[u8]` implementations. `Slice` behavior is inherited
through its shared-reference forwarding law, while `Source` additionally has explicit `&str` and
`&[u8]` implementations that preserve the input reference's lifetime. The distinction is
intentional: `Slice` only reads an already-produced span, while `Source::Slice<'a>` must accurately
describe how long a newly produced span remains valid.

Here is one function generic over `Source` and one generic over `Slice`, each run over both a
text source and a byte source, with the boundary discipline doing its job:

```rust
use tokora::{Slice, Source};

// Representation-agnostic: take the leading `n` cursor units of any source,
// snapping `n` to a valid boundary so the returned slice is always well-formed.
// For `str` that means a UTF-8 code-point boundary; for `[u8]` every index is a
// boundary, so nothing moves.
fn head<S>(src: &S, n: usize) -> Option<S::Slice<'_>>
where
    S: Source<usize> + ?Sized,
{
    let end = src.find_boundary(n.min(src.len()));
    src.slice(..end)
}

// Slice-level: how many *elements* does this span iterate? The element type is
// the slice's `Char` — `char` for text, `u8` for bytes.
fn elements<'s, S: Slice<'s>>(span: &S) -> usize {
    span.iter().count()
}

// "héllo": 'é' is a two-byte code point, so the text is 6 bytes long.
let text: &str = "héllo";
assert_eq!(text.len(), 6);

// Asking for 2 bytes lands *inside* 'é'. The str source snaps down to a
// boundary, so `head` yields "h" — never a panic, never a split scalar.
assert_eq!(head(text, 2), Some("h"));

// The exact same code over bytes keeps both bytes: every index is valid there.
let bytes: &[u8] = b"h\xC3\xA9llo"; // the UTF-8 encoding of "héllo"
assert_eq!(head(bytes, 2), Some(b"h\xC3".as_slice()));

// One text, two shapes: `str` iterates 5 scalar values; `[u8]` iterates 6 bytes.
assert_eq!(elements(&text), 5);
assert_eq!(elements(&text.as_bytes()), 6);
```

Note the asymmetry the two signatures reveal: `head` is generic over `S = str` / `[u8]` — the
*medium*, which is what implements `Source` — while `elements` is generic over `S = &str` /
`&[u8]` — the *span*, which is what implements `Slice`. `Source::Slice<'a>: Slice<'a>` is the hinge
that connects them, and it is the only line of glue the engine needs to be blind to representation.

The same `head` would compile unchanged for a lexer sourced from `bytes::Bytes` or `HipStr`; the
only thing that changes is the concrete `S::Slice<'_>` it returns — a refcount bump instead of a
borrow. That is the payoff of naming the seam: the grammar is written once, and the choice of how
the input is stored is somebody else's, made later, at the edge.
