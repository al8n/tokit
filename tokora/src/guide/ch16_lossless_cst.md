# 16. Lossless CSTs with Rowan

Prerequisites: chapters 1 and 2. [Chapter 6](super::ch06_backtracking) explains the
backtracking this chapter gets for free, and [chapter 8](super::ch08_recovery) the recovery
machinery it reuses.

A **concrete** syntax tree keeps what an AST throws away: whitespace, comments, exact token
text, even the garbage inside a syntax error — every byte of the source, in order.
Formatters, linters, IDEs, and refactoring tools live on that property. Tokora's CST support
is **lossless by configuration**: you write one parser assembly, and the emitter you run it
with decides whether it also builds a tree.

Two crates share the work:

- **tokora** parses and records. Committed tokens flow to the emitter on their own
  ([`commit_token`](crate::Emitter::commit_token) fires once per settled token, everywhere —
  you never call it), and node structure is declared with the
  [`node`](crate::parser::node()) combinators. This half is rowan-free and compiles in every
  build.
- **[rowan](https://docs.rs/rowan)** stores the finished tree. Under the `rowan` feature,
  the [`Sink`](crate::cst::Sink) emitter buffers the parse as a flat event stream, and
  [`finish`](crate::cst::Sink::finish) materializes it once into a rowan green tree.

The event stream between the two is an implementation detail: you never construct, inspect,
or replay events. (The [`cst::event`](crate::cst::event) module documents the vocabulary and
its laws normatively, for the curious.) What matters is *where* the events live — in the
emitter's rewindable channel. The same checkpoint/rewind mark that unwinds diagnostics
unwinds tree events, so [`attempt`](crate::InputRef::attempt), the
[`Transaction`](crate::Transaction) guards, and pratt rollback rewind the tree for free.

If you read this chapter before 0.2: it taught a manual builder walkthrough — a recording
shim around every consume, a builder parameter threaded through every parser function — and
ended by warning that input rollback "cannot roll back external Rowan builder state". That
caveat is now the headline feature (tree recording participates in the one rollback
contract), and the manual threading is simply gone: no parser signature changes when a tree
is wanted.

## Enable Rowan

Rowan is an optional dependency and tokora does not re-export it, so a tree-building crate
names both:

```toml
[dependencies]
tokora = { version = "0.5", features = ["logos", "rowan"] }
rowan = "0.16"
```

The `rowan` feature implies `std` (rowan itself requires it); it does not imply `logos`.
Only the *materializing* half — [`Sink`](crate::cst::Sink) and the typed tree views —
lives behind the feature. The recording half ([`CstEmitter`](crate::emitter::CstEmitter),
the [`node`](crate::parser::node()) combinators, the marks) is unconditional, which is what
lets a grammar crate stay rowan-free while its tooling consumers opt in.

## One enum owns the kind space

Rowan trees are dynamically typed: every node and token carries a raw `u16` kind, and the
dialect gives those numbers meaning through a [`rowan::Language`] implementation. The
convention that keeps the numbering sane is: **one enum, one space** — node kinds and token
kinds live in the same `#[repr(u16)]` enum, declared in the dialect crate. Lexer tokens
enter the tree only as *images* under a mapper function you hand the sink, never as raw
lexer discriminants, so a collision between a token kind and a node kind is unrepresentable
rather than merely checked.

This chapter builds **Query**, a GraphQL-shaped slice: selection sets, fields with optional
aliases, and integer arguments. Its lossless lexer (hidden below, a logos derive like
chapter 1's — just without a `skip` rule, so whitespace, comments, and commas are real
tokens with [`is_trivia`](crate::Token::is_trivia) returning `true`) produces `Tok`. Because
the lexer surfaces every byte, its `Tok` declares `const SURFACES_TRIVIA = true`, and the
lossless [`Sink`](crate::cst::Sink) refuses **at compile time** to wrap a trivia-skipping
lexer — a skipped-whitespace gap is indistinguishable from a dropped committed token. The
unified kind space maps it like this:

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
use rowan::Language;

/// The dialect's whole u16 space: token images first, node kinds after, plus the three
/// bookkeeping kinds. One enum means one place to look and no way to collide. (One value
/// is reserved crate-wide: `u16::MAX`, the tombstone — never map anything to it.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
enum SyntaxKind {
  // Token images — committed tokens enter the tree only through `map_token` below.
  Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
  // Node kinds — the grammar's shapes, declared by the `node()` calls you will meet next.
  SelectionSet, Field, Alias, Arguments, Argument,
  // Bookkeeping: recovery holes, materialization gap tiles, and the synthetic root.
  Error, Gap, Root,
}
type K = SyntaxKind;

impl SyntaxKind {
  /// The raw value the event channel speaks.
  const fn raw(self) -> u16 {
    self as u16
  }
}

/// The sink-side mapper: one compiler-exhaustive match from lexer token to unified kind.
/// Add a token variant and this match — the whole cost of keeping the spaces aligned —
/// fails to compile until you place its image.
fn map_token(tok: &Tok) -> u16 {
  (match tok {
    Tok::Whitespace => K::Whitespace, Tok::Comment => K::Comment, Tok::Comma => K::Comma,
    Tok::Ident => K::Ident, Tok::Int => K::Int, Tok::LBrace => K::LBrace,
    Tok::RBrace => K::RBrace, Tok::LParen => K::LParen, Tok::RParen => K::RParen,
    Tok::Colon => K::Colon,
  }) as u16
}

/// Rowan's side of the bargain: raw ↔ typed kind conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum QueryLang {}

impl Language for QueryLang {
  type Kind = SyntaxKind;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
    // `#[repr(u16)]` with default discriminants: the raw value is the declaration index.
    const KINDS: [SyntaxKind; 18] = [
      K::Whitespace, K::Comment, K::Comma, K::Ident, K::Int, K::LBrace, K::RBrace,
      K::LParen, K::RParen, K::Colon, K::SelectionSet, K::Field, K::Alias, K::Arguments,
      K::Argument, K::Error, K::Gap, K::Root,
    ];
    KINDS[raw.0 as usize]
  }

  fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
    rowan::SyntaxKind(kind as u16)
  }
}

assert_eq!(map_token(&Tok::Colon), SyntaxKind::Colon.raw());
assert_eq!(
  QueryLang::kind_from_raw(rowan::SyntaxKind(SyntaxKind::Field.raw())),
  SyntaxKind::Field,
);
```

That is the entire dialect setup. The sink-facing part is smaller still: the mapper plus two
kind choices at construction — `error_kind` (what wraps a recovery hole's skipped tokens)
and `gap_kind` (what tiles source bytes no committed token covered). Everything else is
rowan's ordinary price, paid once per dialect.

A note for real languages: keep **contextual keywords out of the token images**. GraphQL's
`query` lexes as an identifier and should map to `Ident` — let the typed layer classify by
text. Baking nineteen `*Kw` kinds into the image space forces the mapper to re-classify
identifiers on the hot path for no structural gain.

## The grammar declares the tree

Here is the heart of the chapter. [`node(kind, parser)`](crate::parser::node()) wraps a
parser so that, on success, **everything the sub-parse committed** — tokens, trivia, nested
nodes — becomes the children of one syntax node of that kind. Structure is declared exactly
where the grammar already is; nothing else about the parser changes. Compare these functions
with chapter 2's: the signatures are identical except for one bound —
[`CstEmitter`](crate::emitter::CstEmitter) where chapter 2 wrote
[`Emitter`](crate::Emitter) — and the bound appears only on functions that *declare tree
structure*. Helpers that merely consume keep the plain emitter bound.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
# type QueryLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, Clone, PartialEq)]
# enum QueryError { Lex, Unexpected }
# impl From<LexError> for QueryError { fn from(_: LexError) -> Self { QueryError::Lex } }
# impl<'a, T, Kd: Clone, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>> for QueryError {
#   fn from(_: tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>) -> Self { QueryError::Unexpected }
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# #[repr(u16)]
# enum SyntaxKind {
#   Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
#   SelectionSet, Field, Alias, Arguments, Argument,
#   Error, Gap, Root,
# }
# type K = SyntaxKind;
# impl SyntaxKind {
#   const fn raw(self) -> u16 { self as u16 }
# }
# fn map_token(tok: &Tok) -> u16 {
#   (match tok {
#     Tok::Whitespace => K::Whitespace, Tok::Comment => K::Comment, Tok::Comma => K::Comma,
#     Tok::Ident => K::Ident, Tok::Int => K::Int, Tok::LBrace => K::LBrace,
#     Tok::RBrace => K::RBrace, Tok::LParen => K::LParen, Tok::RParen => K::RParen,
#     Tok::Colon => K::Colon,
#   }) as u16
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# enum QueryLang {}
# impl rowan::Language for QueryLang {
#   type Kind = SyntaxKind;
#   fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
#     const KINDS: [SyntaxKind; 18] = [
#       K::Whitespace, K::Comment, K::Comma, K::Ident, K::Int, K::LBrace, K::RBrace,
#       K::LParen, K::RParen, K::Colon, K::SelectionSet, K::Field, K::Alias, K::Arguments,
#       K::Argument, K::Error, K::Gap, K::Root,
#     ];
#     KINDS[raw.0 as usize]
#   }
#   fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind { rowan::SyntaxKind(kind as u16) }
# }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
  cache::DefaultCache,
  cst::Sink,
  emitter::{CstEmitter, Fatal},
  parser::{node, node_at},
  try_parse_input::ParseAttempt,
};

/// Chapter shorthand for the input reference.
type QueryIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, QueryLexer<'inp>, Ctx>;

/// The typed result. The AST does not go away when a tree is wanted — the tree is a side
/// effect of consuming, and the parser still returns whatever it returned before.
#[derive(Debug, Clone, PartialEq)]
struct Field {
  alias: Option<String>,
  name: String,
  args: usize,
  children: Vec<Field>,
}

/// Commits any leading trivia, then reports the next token's kind without consuming it
/// (`None` at end of input). Committing trivia during a peek is safe over a lossless
/// stream: trivia belongs to the parse — and to the tree — no matter which branch wins.
fn sig_peek<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Option<Tok>, QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  let mut ahead = None;
  inp.try_expect(|t| {
    ahead = Some(t.data().kind());
    false
  })?;
  Ok(ahead)
}

# fn expect_tok<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>, want: Tok) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| t.data().kind() == want)? {
#     Some(_) => Ok(()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn ident<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<String, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| matches!(t.data().kind(), Tok::Ident))? {
#     Some(_) => Ok(inp.slice().to_string()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn try_colon<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<ParseAttempt<()>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   Ok(match inp.try_expect(|t| matches!(t.data().kind(), Tok::Colon))? {
#     Some(_) => ParseAttempt::Accept(()),
#     None => ParseAttempt::Decline,
#   })
# }
// (Hidden: `expect_tok` and `ident` — chapter 2's committed one-token parsers, with a
//  leading trivia skip; and `try_colon`, a declining attempt at a `:`.)

/// `selection_set := "{" field* "}"` — one `node()` bracket over the whole shape: the
/// braces, the trivia, and every child selection land inside the `SelectionSet` node.
fn selection_set<'inp, Ctx>(
  inp: &mut QueryIn<'inp, '_, Ctx>,
) -> Result<Vec<Field>, QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
    + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  node(K::SelectionSet.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
    expect_tok(inp, Tok::LBrace)?;
    let mut fields = Vec::new();
    loop {
      match sig_peek(inp)? {
        Some(Tok::Ident) => fields.push(field(inp)?),
        Some(Tok::RBrace) => {
          expect_tok(inp, Tok::RBrace)?;
          return Ok(fields);
        }
        _ => return Err(QueryError::Unexpected),
      }
    }
  })
  .parse_input(inp)
}

# fn field<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Field, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Field.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     let mark = inp.emitter().cst_mark();
#     let first = ident(inp)?;
#     let (alias, name) = match node_at(mark, K::Alias.raw(), try_colon).try_parse_input(inp)? {
#       ParseAttempt::Accept(()) => (Some(first), ident(inp)?),
#       _ => (None, first),
#     };
#     let args = opt_arguments(inp)?;
#     let children = match sig_peek(inp)? {
#       Some(Tok::LBrace) => selection_set(inp)?,
#       _ => Vec::new(),
#     };
#     Ok(Field { alias, name, args, children })
#   })
#   .parse_input(inp)
# }
// (Hidden: `field` — the next section builds it around the alias ambiguity.)

/// `arguments := "(" argument* ")"`, or nothing at all. Dispatch by PEEK, then let the
/// bracketed parser consume the `(` — so the parenthesis lands *inside* the `Arguments`
/// node. And when there are no arguments, no node is ever opened: an absent optional
/// shape must not leave an empty node behind.
fn opt_arguments<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<usize, QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
    + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  match sig_peek(inp)? {
    Some(Tok::LParen) => node(K::Arguments.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
      expect_tok(inp, Tok::LParen)?;
      let mut count = 0;
      loop {
        match sig_peek(inp)? {
          Some(Tok::Ident) => {
            argument(inp)?;
            count += 1;
          }
          Some(Tok::RParen) => {
            expect_tok(inp, Tok::RParen)?;
            return Ok(count);
          }
          _ => return Err(QueryError::Unexpected),
        }
      }
    })
    .parse_input(inp),
    _ => Ok(0),
  }
}

/// `argument := ident ":" int` — `Argument[Ident, Colon, Int]`, plus whatever trivia was
/// consumed along the way.
fn argument<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<(), QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
    + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  node(K::Argument.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
    ident(inp)?;
    expect_tok(inp, Tok::Colon)?;
    expect_tok(inp, Tok::Int)
  })
  .parse_input(inp)
}

let src = "{ user(id: 4) { name } }";

// The sink wraps an ordinary emitter (fail-fast `Fatal` here) and takes the dialect
// corner: the mapper and the two bookkeeping kinds. It stays OUTSIDE the parse — `&mut`
// in the context seat — because materialization needs it back afterwards.
let mut sink: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());

let fields = Parser::with_context((&mut sink, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(selection_set)
  .parse_str(src)
  .unwrap();

// The typed result, exactly as if no tree existed:
assert_eq!(fields.len(), 1);
let user = &fields[0];
assert_eq!(user.alias, None);
assert_eq!(user.name, "user");
assert_eq!(user.args, 1);
assert_eq!(user.children.len(), 1);
assert_eq!(user.children[0].name, "name");

// Materialize once. The sink is consumed; the inner emitter comes back with the tree,
// so collected diagnostics (chapter 7) survive materialization.
let (green, _emitter) = sink.finish(K::Root.raw(), src);
let tree = rowan::SyntaxNode::<QueryLang>::new_root(green.unwrap());

// The round-trip law — the reason to build a CST at all:
assert_eq!(tree.text().to_string(), src);

// And the structure is the grammar's:
//
//   Root
//   └─ SelectionSet
//      ├─ "{"  " "
//      ├─ Field
//      │  ├─ Ident "user"
//      │  ├─ Arguments ["(", Argument [Ident "id", ":", " ", Int "4"], ")"]
//      │  ├─ " "
//      │  └─ SelectionSet ["{", " ", Field [Ident "name", " "], "}"]
//      ├─ " "
//      └─ "}"
let sel = tree.first_child().unwrap();
assert_eq!(sel.kind(), SyntaxKind::SelectionSet);
let user_node = sel.first_child().unwrap();
assert_eq!(user_node.kind(), SyntaxKind::Field);
assert_eq!(
  user_node.children().map(|n| n.kind()).collect::<Vec<_>>(),
  [SyntaxKind::Arguments, SyntaxKind::SelectionSet],
);
assert_eq!(user_node.first_child().unwrap().text().to_string(), "(id: 4)");
```

### The bracket contract

`node()` is a *bracket*, and its exits are total:

- **Success** wraps precisely the region committed since entry.
- **A decline** (the inner parser is a `try_` parser that declined) records no node — not
  even an empty one. `opt_arguments` above leans on this;
  [`node_opt`](crate::parser::node_opt()) packages the same shape as an `Option`.
- **An error-path unwind** (`?` out of the inner parser) records no node and leaves no
  dangling half-open bracket: materialization stays balanced, whatever already committed
  stays in the tree, and gap tiling (below) keeps the round trip.

There is no "finish the node on every path" duty anywhere in the grammar — the bracket is
append-only under the hood (an inert mark at entry, spent only on success), which is why no
exit can leave the tree in a wrong state.

## `node_at`: wrap what you already parsed

Some shapes are only knowable in hindsight. A GraphQL field may open with an alias —
`author: user` — but when the parser reads the first identifier it cannot know whether that
identifier is the field's *name* or an *alias*: only a following `:` decides. Rewriting the
grammar to lookahead twice would contort it; wrapping too eagerly would put a wrong node in
the tree.

[`node_at`](crate::parser::node_at()) is the retro-wrap: take a
[mark](crate::emitter::CstEmitter::cst_mark) *before* the first identifier, parse it, and
spend the mark only when the colon shows up — the new node wraps everything recorded since
the mark, including tokens committed before the wrap was conceivable.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
# type QueryLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, Clone, PartialEq)]
# enum QueryError { Lex, Unexpected }
# impl From<LexError> for QueryError { fn from(_: LexError) -> Self { QueryError::Lex } }
# impl<'a, T, Kd: Clone, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>> for QueryError {
#   fn from(_: tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>) -> Self { QueryError::Unexpected }
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# #[repr(u16)]
# enum SyntaxKind {
#   Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
#   SelectionSet, Field, Alias, Arguments, Argument,
#   Error, Gap, Root,
# }
# type K = SyntaxKind;
# impl SyntaxKind {
#   const fn raw(self) -> u16 { self as u16 }
# }
# fn map_token(tok: &Tok) -> u16 {
#   (match tok {
#     Tok::Whitespace => K::Whitespace, Tok::Comment => K::Comment, Tok::Comma => K::Comma,
#     Tok::Ident => K::Ident, Tok::Int => K::Int, Tok::LBrace => K::LBrace,
#     Tok::RBrace => K::RBrace, Tok::LParen => K::LParen, Tok::RParen => K::RParen,
#     Tok::Colon => K::Colon,
#   }) as u16
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# enum QueryLang {}
# impl rowan::Language for QueryLang {
#   type Kind = SyntaxKind;
#   fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
#     const KINDS: [SyntaxKind; 18] = [
#       K::Whitespace, K::Comment, K::Comma, K::Ident, K::Int, K::LBrace, K::RBrace,
#       K::LParen, K::RParen, K::Colon, K::SelectionSet, K::Field, K::Alias, K::Arguments,
#       K::Argument, K::Error, K::Gap, K::Root,
#     ];
#     KINDS[raw.0 as usize]
#   }
#   fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind { rowan::SyntaxKind(kind as u16) }
# }
# use tokora::{
#   Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
#   cache::DefaultCache,
#   cst::Sink,
#   emitter::{CstEmitter, Fatal},
#   parser::{node, node_at},
#   try_parse_input::ParseAttempt,
# };
# type QueryIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, QueryLexer<'inp>, Ctx>;
# #[allow(dead_code)]
# #[derive(Debug, Clone, PartialEq)]
# struct Field {
#   alias: Option<String>,
#   name: String,
#   args: usize,
#   children: Vec<Field>,
# }
# fn sig_peek<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Option<Tok>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   let mut ahead = None;
#   inp.try_expect(|t| {
#     ahead = Some(t.data().kind());
#     false
#   })?;
#   Ok(ahead)
# }
# fn expect_tok<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>, want: Tok) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| t.data().kind() == want)? {
#     Some(_) => Ok(()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn ident<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<String, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| matches!(t.data().kind(), Tok::Ident))? {
#     Some(_) => Ok(inp.slice().to_string()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn try_colon<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<ParseAttempt<()>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   Ok(match inp.try_expect(|t| matches!(t.data().kind(), Tok::Colon))? {
#     Some(_) => ParseAttempt::Accept(()),
#     None => ParseAttempt::Decline,
#   })
# }
# fn selection_set<'inp, Ctx>(
#   inp: &mut QueryIn<'inp, '_, Ctx>,
# ) -> Result<Vec<Field>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::SelectionSet.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     expect_tok(inp, Tok::LBrace)?;
#     let mut fields = Vec::new();
#     loop {
#       match sig_peek(inp)? {
#         Some(Tok::Ident) => fields.push(field(inp)?),
#         Some(Tok::RBrace) => {
#           expect_tok(inp, Tok::RBrace)?;
#           return Ok(fields);
#         }
#         _ => return Err(QueryError::Unexpected),
#       }
#     }
#   })
#   .parse_input(inp)
# }
# fn opt_arguments<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<usize, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   match sig_peek(inp)? {
#     Some(Tok::LParen) => node(K::Arguments.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#       expect_tok(inp, Tok::LParen)?;
#       let mut count = 0;
#       loop {
#         match sig_peek(inp)? {
#           Some(Tok::Ident) => {
#             argument(inp)?;
#             count += 1;
#           }
#           Some(Tok::RParen) => {
#             expect_tok(inp, Tok::RParen)?;
#             return Ok(count);
#           }
#           _ => return Err(QueryError::Unexpected),
#         }
#       }
#     })
#     .parse_input(inp),
#     _ => Ok(0),
#   }
# }
# fn argument<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Argument.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     ident(inp)?;
#     expect_tok(inp, Tok::Colon)?;
#     expect_tok(inp, Tok::Int)
#   })
#   .parse_input(inp)
# }
/// `field := (ident ":")? ident arguments? selection_set?`
fn field<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Field, QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
    + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  node(K::Field.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
    // An inert mark: costs one buffer slot, promises nothing.
    let mark = inp.emitter().cst_mark();
    let first = ident(inp)?;
    let (alias, name) = match node_at(mark, K::Alias.raw(), try_colon).try_parse_input(inp)? {
      // The colon was there — `first` was an alias all along. `node_at` spent the mark:
      // the tree now holds `Alias[Ident, Colon]` wrapped around the identifier that was
      // parsed BEFORE the wrap was known. The real name follows.
      ParseAttempt::Accept(()) => (Some(first), ident(inp)?),
      // No colon: the attempt declined and the mark was left unspent. An unspent mark
      // materializes into nothing — `first` was the name, and no `Alias` node exists.
      _ => (None, first),
    };
    let args = opt_arguments(inp)?;
    let children = match sig_peek(inp)? {
      Some(Tok::LBrace) => selection_set(inp)?,
      _ => Vec::new(),
    };
    Ok(Field { alias, name, args, children })
  })
  .parse_input(inp)
}

let src = "{ author: user(id: 4) { name } }";
let mut sink: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
let fields = Parser::with_context((&mut sink, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(selection_set)
  .parse_str(src)
  .unwrap();

assert_eq!(fields[0].alias.as_deref(), Some("author"));
assert_eq!(fields[0].name, "user");

let (green, _emitter) = sink.finish(K::Root.raw(), src);
let tree = rowan::SyntaxNode::<QueryLang>::new_root(green.unwrap());
assert_eq!(tree.text().to_string(), src);

// The retro-wrap in the finished tree: Field's first child is the Alias node, spanning
// the identifier and the colon that revealed it.
let field_node = tree.first_child().unwrap().first_child().unwrap();
assert_eq!(field_node.kind(), SyntaxKind::Field);
let alias_node = field_node.first_child().unwrap();
assert_eq!(alias_node.kind(), SyntaxKind::Alias);
assert_eq!(alias_node.text().to_string(), "author:");
```

Two safety properties keep caller-held marks honest. A mark whose branch was rolled back is
**stale**, and spending it panics in every build — the alternative would be silently
wrapping whatever the retry parsed over the same buffer positions, a wrong tree nothing
downstream can detect. And for the common single-wrap decision tree, the
[`Marker`](crate::cst::event::Marker) typestate makes double-spends and
wrap-before-complete compile errors rather than conventions.

## Tokens reach the tree on their own

Notice what the grammar above never does: it never records a token. There is no
`builder.token(...)`, no recording wrapper around `next`, no per-atom plumbing. Every
**committed** token — consumed by `try_expect`, drained from the lookahead cache, or
settled by a scan like [`skip_while`](crate::InputRef::skip_while) — flows to the emitter
at the moment it settles, through one crate-internal chokepoint. Peeks, declines, and
rolled-back speculation record nothing, because nothing was committed.

That is why trivia handling costs zero code: the trivia skips sprinkled through the helpers
(`skip_while(|t| t.is_trivia())`, or the [`padded`](crate::ParseInput::padded) combinator,
which does the same) *commit* the trivia tokens they cross, so the whitespace lands in the
tree even though no grammar rule mentions it. A trivia token materializes into whichever
node was open where it committed (the [`TriviaPolicy::AsEmitted`](crate::cst::TriviaPolicy)
placement — deterministic, and exactly where the consuming code stood). Capturing trivia
wrappers that collect `Vec`s of trivia per node remain useful for consumers that want
formatting data *without* a tree in the dependency closure; under a sink they are redundant.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
# type QueryLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, Clone, PartialEq)]
# enum QueryError { Lex, Unexpected }
# impl From<LexError> for QueryError { fn from(_: LexError) -> Self { QueryError::Lex } }
# impl<'a, T, Kd: Clone, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>> for QueryError {
#   fn from(_: tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>) -> Self { QueryError::Unexpected }
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# #[repr(u16)]
# enum SyntaxKind {
#   Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
#   SelectionSet, Field, Alias, Arguments, Argument,
#   Error, Gap, Root,
# }
# type K = SyntaxKind;
# impl SyntaxKind {
#   const fn raw(self) -> u16 { self as u16 }
# }
# fn map_token(tok: &Tok) -> u16 {
#   (match tok {
#     Tok::Whitespace => K::Whitespace, Tok::Comment => K::Comment, Tok::Comma => K::Comma,
#     Tok::Ident => K::Ident, Tok::Int => K::Int, Tok::LBrace => K::LBrace,
#     Tok::RBrace => K::RBrace, Tok::LParen => K::LParen, Tok::RParen => K::RParen,
#     Tok::Colon => K::Colon,
#   }) as u16
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# enum QueryLang {}
# impl rowan::Language for QueryLang {
#   type Kind = SyntaxKind;
#   fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
#     const KINDS: [SyntaxKind; 18] = [
#       K::Whitespace, K::Comment, K::Comma, K::Ident, K::Int, K::LBrace, K::RBrace,
#       K::LParen, K::RParen, K::Colon, K::SelectionSet, K::Field, K::Alias, K::Arguments,
#       K::Argument, K::Error, K::Gap, K::Root,
#     ];
#     KINDS[raw.0 as usize]
#   }
#   fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind { rowan::SyntaxKind(kind as u16) }
# }
# use tokora::{
#   Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
#   cache::DefaultCache,
#   cst::Sink,
#   emitter::{CstEmitter, Fatal},
#   parser::{node, node_at},
#   try_parse_input::ParseAttempt,
# };
# type QueryIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, QueryLexer<'inp>, Ctx>;
# #[allow(dead_code)]
# #[derive(Debug, Clone, PartialEq)]
# struct Field {
#   alias: Option<String>,
#   name: String,
#   args: usize,
#   children: Vec<Field>,
# }
# fn sig_peek<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Option<Tok>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   let mut ahead = None;
#   inp.try_expect(|t| {
#     ahead = Some(t.data().kind());
#     false
#   })?;
#   Ok(ahead)
# }
# fn expect_tok<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>, want: Tok) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| t.data().kind() == want)? {
#     Some(_) => Ok(()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn ident<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<String, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| matches!(t.data().kind(), Tok::Ident))? {
#     Some(_) => Ok(inp.slice().to_string()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn try_colon<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<ParseAttempt<()>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   Ok(match inp.try_expect(|t| matches!(t.data().kind(), Tok::Colon))? {
#     Some(_) => ParseAttempt::Accept(()),
#     None => ParseAttempt::Decline,
#   })
# }
# fn selection_set<'inp, Ctx>(
#   inp: &mut QueryIn<'inp, '_, Ctx>,
# ) -> Result<Vec<Field>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::SelectionSet.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     expect_tok(inp, Tok::LBrace)?;
#     let mut fields = Vec::new();
#     loop {
#       match sig_peek(inp)? {
#         Some(Tok::Ident) => fields.push(field(inp)?),
#         Some(Tok::RBrace) => {
#           expect_tok(inp, Tok::RBrace)?;
#           return Ok(fields);
#         }
#         _ => return Err(QueryError::Unexpected),
#       }
#     }
#   })
#   .parse_input(inp)
# }
# fn field<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Field, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Field.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     let mark = inp.emitter().cst_mark();
#     let first = ident(inp)?;
#     let (alias, name) = match node_at(mark, K::Alias.raw(), try_colon).try_parse_input(inp)? {
#       ParseAttempt::Accept(()) => (Some(first), ident(inp)?),
#       _ => (None, first),
#     };
#     let args = opt_arguments(inp)?;
#     let children = match sig_peek(inp)? {
#       Some(Tok::LBrace) => selection_set(inp)?,
#       _ => Vec::new(),
#     };
#     Ok(Field { alias, name, args, children })
#   })
#   .parse_input(inp)
# }
# fn opt_arguments<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<usize, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   match sig_peek(inp)? {
#     Some(Tok::LParen) => node(K::Arguments.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#       expect_tok(inp, Tok::LParen)?;
#       let mut count = 0;
#       loop {
#         match sig_peek(inp)? {
#           Some(Tok::Ident) => {
#             argument(inp)?;
#             count += 1;
#           }
#           Some(Tok::RParen) => {
#             expect_tok(inp, Tok::RParen)?;
#             return Ok(count);
#           }
#           _ => return Err(QueryError::Unexpected),
#         }
#       }
#     })
#     .parse_input(inp),
#     _ => Ok(0),
#   }
# }
# fn argument<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Argument.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     ident(inp)?;
#     expect_tok(inp, Tok::Colon)?;
#     expect_tok(inp, Tok::Int)
#   })
#   .parse_input(inp)
# }
// Comments, newlines, commas: no grammar rule mentions them, all of them survive.
let src = "{ # every byte survives\n  a, b }";
let mut sink: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
Parser::with_context((&mut sink, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(selection_set)
  .parse_str(src)
  .unwrap();
let (green, _emitter) = sink.finish(K::Root.raw(), src);
let tree = rowan::SyntaxNode::<QueryLang>::new_root(green.unwrap());
assert_eq!(tree.text().to_string(), src);

let tokens: Vec<_> = tree
  .descendants_with_tokens()
  .filter_map(|el| el.into_token())
  .map(|t| (t.kind(), t.text().to_string()))
  .collect();
assert!(tokens.contains(&(SyntaxKind::Comment, "# every byte survives".to_string())));
assert!(tokens.contains(&(SyntaxKind::Comma, ",".to_string())));
// Nothing was gap-tiled: every byte was covered by a real committed token.
assert!(tokens.iter().all(|(kind, _)| *kind != SyntaxKind::Gap));

// And when bytes are NOT covered — here `%` is no token of the language, the lexer
// reports it and fail-fast `Fatal` aborts the parse before the tail is even lexed — that
// tail is un-diagnosed. Strict `finish` refuses it (an unexplained gap is, to `finish`,
// indistinguishable from a dropped token); the tooling door `finish_partial` tiles it, so
// an aborted parse still round-trips its text.
let src = "{ a % b }";
let mut sink: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
let res = Parser::with_context((&mut sink, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(selection_set)
  .parse_str(src);
assert_eq!(res, Err(QueryError::Lex));

let (green, _emitter) = sink.finish_partial(K::Root.raw(), src);
let tree = rowan::SyntaxNode::<QueryLang>::new_root(green.unwrap());
assert_eq!(tree.text().to_string(), src, "aborted parse, intact text");
assert!(
  tree
    .descendants_with_tokens()
    .filter_map(|el| el.into_token())
    .any(|t| t.kind() == SyntaxKind::Gap),
  "the unparsed region is a gap token, not a hole in the text"
);
```

## One assembly, two configurations

The grammar functions above bound their emitter as
[`CstEmitter`](crate::emitter::CstEmitter) — and every diagnostics-only emitter the crate
ships ([`Fatal`](crate::emitter::Fatal), [`Verbose`](crate::emitter::Verbose),
[`Silent`](crate::emitter::Silent), and [`Ignored`](crate::emitter::Ignored)) already
implements it, through defaulted no-op event methods. So the same functions run in a plain
fail-fast context with no sink anywhere in sight — no `rowan` feature, no tree, and no
cost: the no-op event calls take references, inline to empty bodies, and compile to
nothing.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
# type QueryLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, Clone, PartialEq)]
# enum QueryError { Lex, Unexpected }
# impl From<LexError> for QueryError { fn from(_: LexError) -> Self { QueryError::Lex } }
# impl<'a, T, Kd: Clone, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>> for QueryError {
#   fn from(_: tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>) -> Self { QueryError::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>> for QueryError { fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self { QueryError::Unexpected } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for QueryError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { QueryError::Unexpected } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# #[repr(u16)]
# enum SyntaxKind {
#   Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
#   SelectionSet, Field, Alias, Arguments, Argument,
#   Error, Gap, Root,
# }
# type K = SyntaxKind;
# impl SyntaxKind {
#   const fn raw(self) -> u16 { self as u16 }
# }
# use tokora::{
#   Emitter, InputRef, ParseContext, ParseInput, TryParseInput,
#   emitter::CstEmitter,
#   parser::{node, node_at},
#   try_parse_input::ParseAttempt,
# };
# type QueryIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, QueryLexer<'inp>, Ctx>;
# #[derive(Debug, Clone, PartialEq)]
# struct Field {
#   alias: Option<String>,
#   name: String,
#   args: usize,
#   children: Vec<Field>,
# }
# fn sig_peek<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Option<Tok>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   let mut ahead = None;
#   inp.try_expect(|t| {
#     ahead = Some(t.data().kind());
#     false
#   })?;
#   Ok(ahead)
# }
# fn expect_tok<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>, want: Tok) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| t.data().kind() == want)? {
#     Some(_) => Ok(()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn ident<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<String, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| matches!(t.data().kind(), Tok::Ident))? {
#     Some(_) => Ok(inp.slice().to_string()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn try_colon<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<ParseAttempt<()>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   Ok(match inp.try_expect(|t| matches!(t.data().kind(), Tok::Colon))? {
#     Some(_) => ParseAttempt::Accept(()),
#     None => ParseAttempt::Decline,
#   })
# }
# fn selection_set<'inp, Ctx>(
#   inp: &mut QueryIn<'inp, '_, Ctx>,
# ) -> Result<Vec<Field>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::SelectionSet.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     expect_tok(inp, Tok::LBrace)?;
#     let mut fields = Vec::new();
#     loop {
#       match sig_peek(inp)? {
#         Some(Tok::Ident) => fields.push(field(inp)?),
#         Some(Tok::RBrace) => {
#           expect_tok(inp, Tok::RBrace)?;
#           return Ok(fields);
#         }
#         _ => return Err(QueryError::Unexpected),
#       }
#     }
#   })
#   .parse_input(inp)
# }
# fn field<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Field, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Field.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     let mark = inp.emitter().cst_mark();
#     let first = ident(inp)?;
#     let (alias, name) = match node_at(mark, K::Alias.raw(), try_colon).try_parse_input(inp)? {
#       ParseAttempt::Accept(()) => (Some(first), ident(inp)?),
#       _ => (None, first),
#     };
#     let args = opt_arguments(inp)?;
#     let children = match sig_peek(inp)? {
#       Some(Tok::LBrace) => selection_set(inp)?,
#       _ => Vec::new(),
#     };
#     Ok(Field { alias, name, args, children })
#   })
#   .parse_input(inp)
# }
# fn opt_arguments<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<usize, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   match sig_peek(inp)? {
#     Some(Tok::LParen) => node(K::Arguments.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#       expect_tok(inp, Tok::LParen)?;
#       let mut count = 0;
#       loop {
#         match sig_peek(inp)? {
#           Some(Tok::Ident) => {
#             argument(inp)?;
#             count += 1;
#           }
#           Some(Tok::RParen) => {
#             expect_tok(inp, Tok::RParen)?;
#             return Ok(count);
#           }
#           _ => return Err(QueryError::Unexpected),
#         }
#       }
#     })
#     .parse_input(inp),
#     _ => Ok(0),
#   }
# }
# fn argument<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Argument.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     ident(inp)?;
#     expect_tok(inp, Tok::Colon)?;
#     expect_tok(inp, Tok::Int)
#   })
#   .parse_input(inp)
# }
use tokora::{Parse, Parser};

// The exact same `selection_set` — chapter 2's default fail-fast context, no sink,
// no tree. This code needs no `rowan` feature to compile.
let fields = Parser::new()
  .apply(selection_set)
  .parse_str("{ user(id: 4) { name } }")
  .unwrap();
assert_eq!(fields[0].name, "user");
assert_eq!(fields[0].children[0].name, "name");
```

Why a *subtrait* bound instead of more defaulted methods on `Emitter`? Because tree events
are load-bearing where diagnostics are advisory. A custom **wrapper** emitter that forwards
the diagnostic methods but forgot the event methods would produce a parse whose errors flow
perfectly and whose tree is silently empty. With the events on
[`CstEmitter`](crate::emitter::CstEmitter), a `node`-bearing parser refuses a non-forwarding
wrapper at **compile time** — the structural gate. (Wrapper authors: implement and forward
`CstEmitter` deliberately; the shipped diagnostics emitters already do.)

## Backtracking rewinds the tree

The sink's [`checkpoint`](crate::Emitter::checkpoint) mark covers the event buffer and the
wrapped emitter's diagnostics as one timeline. Every rollback shape from
[chapter 6](super::ch06_backtracking) — [`attempt`](crate::InputRef::attempt) /
[`try_attempt`](crate::InputRef::try_attempt), the [`Transaction`](crate::Transaction)
guards, session points — therefore rewinds the tree exactly as it rewinds position and
diagnostics. Speculation can consume tokens, wrap nodes, even recover from errors; if the
branch is abandoned, its events are truncated as if they never happened.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
# type QueryLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, Clone, PartialEq)]
# enum QueryError { Lex, Unexpected }
# impl From<LexError> for QueryError { fn from(_: LexError) -> Self { QueryError::Lex } }
# impl<'a, T, Kd: Clone, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>> for QueryError {
#   fn from(_: tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>) -> Self { QueryError::Unexpected }
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# #[repr(u16)]
# enum SyntaxKind {
#   Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
#   SelectionSet, Field, Alias, Arguments, Argument,
#   Error, Gap, Root,
# }
# type K = SyntaxKind;
# impl SyntaxKind {
#   const fn raw(self) -> u16 { self as u16 }
# }
# fn map_token(tok: &Tok) -> u16 {
#   (match tok {
#     Tok::Whitespace => K::Whitespace, Tok::Comment => K::Comment, Tok::Comma => K::Comma,
#     Tok::Ident => K::Ident, Tok::Int => K::Int, Tok::LBrace => K::LBrace,
#     Tok::RBrace => K::RBrace, Tok::LParen => K::LParen, Tok::RParen => K::RParen,
#     Tok::Colon => K::Colon,
#   }) as u16
# }
# use tokora::{
#   Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
#   cache::DefaultCache,
#   cst::Sink,
#   emitter::{CstEmitter, Fatal},
#   parser::{node, node_at},
#   try_parse_input::ParseAttempt,
# };
# type QueryIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, QueryLexer<'inp>, Ctx>;
# #[allow(dead_code)]
# #[derive(Debug, Clone, PartialEq)]
# struct Field {
#   alias: Option<String>,
#   name: String,
#   args: usize,
#   children: Vec<Field>,
# }
# fn sig_peek<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Option<Tok>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   let mut ahead = None;
#   inp.try_expect(|t| {
#     ahead = Some(t.data().kind());
#     false
#   })?;
#   Ok(ahead)
# }
# fn expect_tok<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>, want: Tok) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| t.data().kind() == want)? {
#     Some(_) => Ok(()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn ident<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<String, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| matches!(t.data().kind(), Tok::Ident))? {
#     Some(_) => Ok(inp.slice().to_string()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn try_colon<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<ParseAttempt<()>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   Ok(match inp.try_expect(|t| matches!(t.data().kind(), Tok::Colon))? {
#     Some(_) => ParseAttempt::Accept(()),
#     None => ParseAttempt::Decline,
#   })
# }
# fn selection_set<'inp, Ctx>(
#   inp: &mut QueryIn<'inp, '_, Ctx>,
# ) -> Result<Vec<Field>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::SelectionSet.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     expect_tok(inp, Tok::LBrace)?;
#     let mut fields = Vec::new();
#     loop {
#       match sig_peek(inp)? {
#         Some(Tok::Ident) => fields.push(field(inp)?),
#         Some(Tok::RBrace) => {
#           expect_tok(inp, Tok::RBrace)?;
#           return Ok(fields);
#         }
#         _ => return Err(QueryError::Unexpected),
#       }
#     }
#   })
#   .parse_input(inp)
# }
# fn field<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Field, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Field.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     let mark = inp.emitter().cst_mark();
#     let first = ident(inp)?;
#     let (alias, name) = match node_at(mark, K::Alias.raw(), try_colon).try_parse_input(inp)? {
#       ParseAttempt::Accept(()) => (Some(first), ident(inp)?),
#       _ => (None, first),
#     };
#     let args = opt_arguments(inp)?;
#     let children = match sig_peek(inp)? {
#       Some(Tok::LBrace) => selection_set(inp)?,
#       _ => Vec::new(),
#     };
#     Ok(Field { alias, name, args, children })
#   })
#   .parse_input(inp)
# }
# fn opt_arguments<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<usize, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   match sig_peek(inp)? {
#     Some(Tok::LParen) => node(K::Arguments.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#       expect_tok(inp, Tok::LParen)?;
#       let mut count = 0;
#       loop {
#         match sig_peek(inp)? {
#           Some(Tok::Ident) => {
#             argument(inp)?;
#             count += 1;
#           }
#           Some(Tok::RParen) => {
#             expect_tok(inp, Tok::RParen)?;
#             return Ok(count);
#           }
#           _ => return Err(QueryError::Unexpected),
#         }
#       }
#     })
#     .parse_input(inp),
#     _ => Ok(0),
#   }
# }
# fn argument<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Argument.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     ident(inp)?;
#     expect_tok(inp, Tok::Colon)?;
#     expect_tok(inp, Tok::Int)
#   })
#   .parse_input(inp)
# }
/// The speculative drive: parse the WHOLE selection set — tokens, trivia, nodes, all
/// recorded — then decline, truncating every event the branch buffered. Then parse it
/// again, for keeps.
fn decline_then_parse<'inp, Ctx>(
  inp: &mut QueryIn<'inp, '_, Ctx>,
) -> Result<Vec<Field>, QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
    + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  let declined: Option<()> = inp.attempt(|inp| {
    selection_set(inp).ok()?;
    None // the branch did real work; declining rewinds all of it
  });
  assert!(declined.is_none());
  selection_set(inp)
}

// Parse the same source twice: once straight, once through the declined speculation.
let src = "{ user(id: 4) { name } }";

let mut straight: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
Parser::with_context((&mut straight, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(selection_set)
  .parse_str(src)
  .unwrap();
let (green_straight, _) = straight.finish(K::Root.raw(), src);

let mut backtracked: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
Parser::with_context((&mut backtracked, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(decline_then_parse)
  .parse_str(src)
  .unwrap();
let (green_backtracked, _) = backtracked.finish(K::Root.raw(), src);

// One timeline survived — the trees are byte-identical.
assert_eq!(green_straight.unwrap(), green_backtracked.unwrap());
```

This equivalence — an attempt-and-decline drive materializes the exact green tree of the
straight drive — is a tested law of the sink, not an accident of this example.

## Recovery: holes become error nodes

[Chapter 8](super::ch08_recovery)'s recovery machinery needs nothing new to be
tree-correct. When [`sync_balanced`](crate::InputRef::sync_balanced) skips a garbage region,
the skipped tokens settle — so they flow to the sink like any committed token — and the
one-per-hole [`emit_skipped_region`](crate::Emitter::emit_skipped_region) wraps them in a
node of the `error_kind` you configured at construction. The tree keeps the **real tokens**,
not an opaque blob: syntax highlighting inside the broken region keeps working, IDE
completion sees the partial identifier, and a formatter reproduces the garbage verbatim.
(A sync that skips zero tokens makes no node, matching its no-diagnostic rule. A *failed*
scan — no sync point found — rewinds its speculative events entirely.)

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
# type QueryLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, Clone, PartialEq)]
# enum QueryError { Lex, Unexpected }
# impl From<LexError> for QueryError { fn from(_: LexError) -> Self { QueryError::Lex } }
# impl<'a, T, Kd: Clone, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>> for QueryError {
#   fn from(_: tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>) -> Self { QueryError::Unexpected }
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# #[repr(u16)]
# enum SyntaxKind {
#   Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
#   SelectionSet, Field, Alias, Arguments, Argument,
#   Error, Gap, Root,
# }
# type K = SyntaxKind;
# impl SyntaxKind {
#   const fn raw(self) -> u16 { self as u16 }
# }
# fn map_token(tok: &Tok) -> u16 {
#   (match tok {
#     Tok::Whitespace => K::Whitespace, Tok::Comment => K::Comment, Tok::Comma => K::Comma,
#     Tok::Ident => K::Ident, Tok::Int => K::Int, Tok::LBrace => K::LBrace,
#     Tok::RBrace => K::RBrace, Tok::LParen => K::LParen, Tok::RParen => K::RParen,
#     Tok::Colon => K::Colon,
#   }) as u16
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# enum QueryLang {}
# impl rowan::Language for QueryLang {
#   type Kind = SyntaxKind;
#   fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
#     const KINDS: [SyntaxKind; 18] = [
#       K::Whitespace, K::Comment, K::Comma, K::Ident, K::Int, K::LBrace, K::RBrace,
#       K::LParen, K::RParen, K::Colon, K::SelectionSet, K::Field, K::Alias, K::Arguments,
#       K::Argument, K::Error, K::Gap, K::Root,
#     ];
#     KINDS[raw.0 as usize]
#   }
#   fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind { rowan::SyntaxKind(kind as u16) }
# }
# use tokora::{
#   Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
#   cache::DefaultCache,
#   cst::Sink,
#   emitter::CstEmitter,
#   parser::{node, node_at},
#   try_parse_input::ParseAttempt,
# };
# type QueryIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, QueryLexer<'inp>, Ctx>;
# #[allow(dead_code)]
# #[derive(Debug, Clone, PartialEq)]
# struct Field {
#   alias: Option<String>,
#   name: String,
#   args: usize,
#   children: Vec<Field>,
# }
# fn sig_peek<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Option<Tok>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   let mut ahead = None;
#   inp.try_expect(|t| {
#     ahead = Some(t.data().kind());
#     false
#   })?;
#   Ok(ahead)
# }
# fn expect_tok<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>, want: Tok) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| t.data().kind() == want)? {
#     Some(_) => Ok(()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn ident<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<String, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| matches!(t.data().kind(), Tok::Ident))? {
#     Some(_) => Ok(inp.slice().to_string()),
#     None => Err(QueryError::Unexpected),
#   }
# }
# fn try_colon<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<ParseAttempt<()>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   Ok(match inp.try_expect(|t| matches!(t.data().kind(), Tok::Colon))? {
#     Some(_) => ParseAttempt::Accept(()),
#     None => ParseAttempt::Decline,
#   })
# }
# fn selection_set<'inp, Ctx>(
#   inp: &mut QueryIn<'inp, '_, Ctx>,
# ) -> Result<Vec<Field>, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::SelectionSet.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     expect_tok(inp, Tok::LBrace)?;
#     let mut fields = Vec::new();
#     loop {
#       match sig_peek(inp)? {
#         Some(Tok::Ident) => fields.push(field(inp)?),
#         Some(Tok::RBrace) => {
#           expect_tok(inp, Tok::RBrace)?;
#           return Ok(fields);
#         }
#         _ => return Err(QueryError::Unexpected),
#       }
#     }
#   })
#   .parse_input(inp)
# }
# fn field<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<Field, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Field.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     let mark = inp.emitter().cst_mark();
#     let first = ident(inp)?;
#     let (alias, name) = match node_at(mark, K::Alias.raw(), try_colon).try_parse_input(inp)? {
#       ParseAttempt::Accept(()) => (Some(first), ident(inp)?),
#       _ => (None, first),
#     };
#     let args = opt_arguments(inp)?;
#     let children = match sig_peek(inp)? {
#       Some(Tok::LBrace) => selection_set(inp)?,
#       _ => Vec::new(),
#     };
#     Ok(Field { alias, name, args, children })
#   })
#   .parse_input(inp)
# }
# fn opt_arguments<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<usize, QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   match sig_peek(inp)? {
#     Some(Tok::LParen) => node(K::Arguments.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#       expect_tok(inp, Tok::LParen)?;
#       let mut count = 0;
#       loop {
#         match sig_peek(inp)? {
#           Some(Tok::Ident) => {
#             argument(inp)?;
#             count += 1;
#           }
#           Some(Tok::RParen) => {
#             expect_tok(inp, Tok::RParen)?;
#             return Ok(count);
#           }
#           _ => return Err(QueryError::Unexpected),
#         }
#       }
#     })
#     .parse_input(inp),
#     _ => Ok(0),
#   }
# }
# fn argument<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
#     + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   node(K::Argument.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
#     ident(inp)?;
#     expect_tok(inp, Tok::Colon)?;
#     expect_tok(inp, Tok::Int)
#   })
#   .parse_input(inp)
# }
use tokora::{Balance, emitter::Verbose, span::Spanned};

/// The bracket classifier (chapter 8): the skip counts nesting so a sync point inside
/// brackets is never mistaken for a boundary.
fn brackets(kind: &Tok) -> Balance<()> {
  match kind {
    Tok::LBrace | Tok::LParen => Balance::Open(()),
    Tok::RBrace | Tok::RParen => Balance::Close(()),
    _ => Balance::Neutral,
  }
}

/// The recovering selection loop: a bad selection is reported, then skipped (at bracket
/// depth zero) to the next plausible field start or the closing brace.
fn selection_set_recovering<'inp, Ctx>(
  inp: &mut QueryIn<'inp, '_, Ctx>,
) -> Result<usize, QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
    + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  node(K::SelectionSet.raw(), |inp: &mut QueryIn<'inp, '_, Ctx>| {
    expect_tok(inp, Tok::LBrace)?;
    let mut salvaged = 0;
    loop {
      match sig_peek(inp)? {
        Some(Tok::Ident) => {
          field(inp)?;
          salvaged += 1;
        }
        Some(Tok::RBrace) => {
          expect_tok(inp, Tok::RBrace)?;
          return Ok(salvaged);
        }
        None => return Err(QueryError::Unexpected),
        Some(_) => {
          // Report, then skip. The hole reports itself through the emitter — and the
          // sink wraps the hole's REAL tokens in the configured error node. No
          // tree-building code appears anywhere in this recovery path.
          let at = *inp.span();
          inp.emitter().emit_error(Spanned::new(at, QueryError::Unexpected))?;
          inp.sync_balanced(brackets, |t| {
            matches!(t.data().kind(), Tok::Ident | Tok::RBrace)
          })?;
        }
      }
    }
  })
  .parse_input(inp)
}

// The garbage between the two fields is not valid Query syntax.
let src = "{ user(id: 4) 4 5 name }";
let mut sink: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Verbose::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
let salvaged = Parser::with_context((&mut sink, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(selection_set_recovering)
  .parse_str(src)
  .unwrap();
assert_eq!(salvaged, 2, "`user` and `name` both survive the garbage between them");

let (green, emitter) = sink.finish(K::Root.raw(), src);
let tree = rowan::SyntaxNode::<QueryLang>::new_root(green.unwrap());
assert_eq!(tree.text().to_string(), src, "recovery does not break the round trip");

// One hole, one error node — holding the real skipped tokens.
let sel = tree.first_child().unwrap();
let error = sel
  .children()
  .find(|n| n.kind() == SyntaxKind::Error)
  .unwrap();
assert_eq!(error.text().to_string(), "4 5 ");
let kinds: Vec<_> = error
  .children_with_tokens()
  .filter_map(|el| el.into_token().map(|t| t.kind()))
  .collect();
assert_eq!(
  kinds,
  [SyntaxKind::Int, SyntaxKind::Whitespace, SyntaxKind::Int, SyntaxKind::Whitespace],
);

// The diagnostics side of the same timeline saw the same single hole.
assert_eq!(emitter.skipped_regions().values().flatten().count(), 1);
assert_eq!(emitter.errors().values().flatten().count(), 1);
```

## Materialization is a typed wall

[`finish(root_kind, source)`](crate::cst::Sink::finish) consumes the sink, validates the
recorded stream, and builds the green tree — returning the inner emitter either way, so
collected diagnostics survive materialization. It **never panics**: a stream that cannot
become a correct tree comes back as a typed [`FinishError`](crate::cst::FinishError)
naming the offending event, and no wrong tree is ever built. Under the blessed combinators
you will not meet these errors — the brackets are total — but the raw
[`CstEmitter`](crate::emitter::CstEmitter) transport is sharp on purpose, and `finish` is
the wall that keeps a hand-rolled mistake loud:

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
# #[logos(crate = logos, error = LexError)]
# enum Tok {
#   #[regex(r"[ \t\r\n]+")] Whitespace,
#   #[regex(r"#[^\r\n]*", allow_greedy = true)] Comment,
#   #[token(",")] Comma,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[regex(r"-?[0-9]+")] Int,
#   #[token("{")] LBrace,
#   #[token("}")] RBrace,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
#   #[token(":")] Colon,
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Tok::Whitespace => "whitespace", Tok::Comment => "comment", Tok::Comma => "`,`",
#       Tok::Ident => "identifier", Tok::Int => "integer", Tok::LBrace => "`{`",
#       Tok::RBrace => "`}`", Tok::LParen => "`(`", Tok::RParen => "`)`", Tok::Colon => "`:`",
#     })
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = Tok;
#   type Error = LexError;
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Tok { *self }
#   fn is_trivia(&self) -> bool { matches!(self, Tok::Whitespace | Tok::Comment | Tok::Comma) }
# }
# type QueryLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# #[derive(Debug, Clone, PartialEq)]
# enum QueryError { Lex, Unexpected }
# impl From<LexError> for QueryError { fn from(_: LexError) -> Self { QueryError::Lex } }
# impl<'a, T, Kd: Clone, S, Lang: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>> for QueryError {
#   fn from(_: tokora::error::token::UnexpectedToken<'a, T, Kd, S, Lang>) -> Self { QueryError::Unexpected }
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# #[repr(u16)]
# enum SyntaxKind {
#   Whitespace, Comment, Comma, Ident, Int, LBrace, RBrace, LParen, RParen, Colon,
#   SelectionSet, Field, Alias, Arguments, Argument,
#   Error, Gap, Root,
# }
# type K = SyntaxKind;
# impl SyntaxKind {
#   const fn raw(self) -> u16 { self as u16 }
# }
# fn map_token(tok: &Tok) -> u16 {
#   (match tok {
#     Tok::Whitespace => K::Whitespace, Tok::Comment => K::Comment, Tok::Comma => K::Comma,
#     Tok::Ident => K::Ident, Tok::Int => K::Int, Tok::LBrace => K::LBrace,
#     Tok::RBrace => K::RBrace, Tok::LParen => K::LParen, Tok::RParen => K::RParen,
#     Tok::Colon => K::Colon,
#   }) as u16
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
# enum QueryLang {}
# impl rowan::Language for QueryLang {
#   type Kind = SyntaxKind;
#   fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
#     const KINDS: [SyntaxKind; 18] = [
#       K::Whitespace, K::Comment, K::Comma, K::Ident, K::Int, K::LBrace, K::RBrace,
#       K::LParen, K::RParen, K::Colon, K::SelectionSet, K::Field, K::Alias, K::Arguments,
#       K::Argument, K::Error, K::Gap, K::Root,
#     ];
#     KINDS[raw.0 as usize]
#   }
#   fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind { rowan::SyntaxKind(kind as u16) }
# }
# use tokora::{
#   Emitter, InputRef, Parse, ParseContext, Parser,
#   cache::DefaultCache,
#   cst::Sink,
#   emitter::{CstEmitter, Fatal},
# };
# type QueryIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, QueryLexer<'inp>, Ctx>;
# fn expect_tok<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>, want: Tok) -> Result<(), QueryError>
# where
#   Ctx: ParseContext<'inp, QueryLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
# {
#   inp.skip_while(|t| t.is_trivia())?;
#   match inp.try_expect(|t| t.data().kind() == want)? {
#     Some(_) => Ok(()),
#     None => Err(QueryError::Unexpected),
#   }
# }
use tokora::cst::FinishError;

/// The raw transport, deliberately skipping the `node()` bracket. Don't write this —
/// this is what the bracket exists to make unnecessary.
fn unfinished<'inp, Ctx>(inp: &mut QueryIn<'inp, '_, Ctx>) -> Result<(), QueryError>
where
  Ctx: ParseContext<'inp, QueryLexer<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, QueryLexer<'inp>>
    + Emitter<'inp, QueryLexer<'inp>, Error = QueryError>,
{
  inp.emitter().cst_start(K::SelectionSet.raw());
  expect_tok(inp, Tok::LBrace)?;
  Err(QueryError::Unexpected) // abort with the node still open
}

let src = "{ user }";
let mut sink: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
let _ = Parser::with_context((&mut sink, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(unfinished)
  .parse_str(src);

// `finish` refuses to guess what the dangling node meant:
let (green, _emitter) = sink.finish(K::Root.raw(), src);
assert!(matches!(green, Err(FinishError::UnclosedNodes { open: 1 })));

// `finish_partial` is the explicit tooling opt-in: close whatever the abort left open
// and hand back an inspectable partial tree — the round-trip law holds on it too.
let mut sink: Sink<'_, QueryLexer<'_>, _> =
  Sink::new(Fatal::<QueryError>::new(), map_token, K::Error.raw(), K::Gap.raw());
let _ = Parser::with_context((&mut sink, DefaultCache::<QueryLexer<'_>>::default()))
  .apply(unfinished)
  .parse_str(src);
let (green, _emitter) = sink.finish_partial(K::Root.raw(), src);
let tree = rowan::SyntaxNode::<QueryLang>::new_root(green.unwrap());
assert_eq!(tree.text().to_string(), src);
assert_eq!(tree.first_child().unwrap().kind(), SyntaxKind::SelectionSet);
```

Note the two incompletenesses `finish_partial` forgives. A **fatal abort through the blessed
combinators** leaves no dangling start — the brackets never desync — so it never earns
`UnclosedNodes`; but the tail it never reached is un-diagnosed, and strict `finish` refuses
that as an [`UncoveredGap`](crate::cst::FinishError::UncoveredGap) (a dropped committed
token and an abandoned tail are indistinguishable to it). `finish_partial` closes any open
node *and* tiles the un-diagnosed tail — the aborted-parse example in the trivia section used
exactly that door. And an [`Incomplete`](crate::Completeness) verdict from a
[partial-input parse](super::ch09_streaming) should not be materialized at all: keep the
sink — the buffered events *are* the resumable state — and `finish` once the parse
completes.

## Pratt expressions

The typed pratt driver of [chapter 5](super::ch05_pratt) carries an additive CST hook:
[`with_cst_kinds`](crate::parser::Pratt::with_cst_kinds) takes a classifier from fold
operators to node kinds, and the driver wraps each folded region itself — the driver holds
the mark, spends it once per fold, and your fold hooks keep their exact signatures. `1 + 2 *
3` materializes as `Bin[1, +, Bin[2, *, 3]]` with the folds computing the same values they
always did; an unconfigured driver records no nodes at all. The token-level pratt API
([`InputRef::pratt`](crate::InputRef::pratt)) folds into synthetic tokens, has no kind seam,
and is documented CST-unsupported.

## Where to go next

- **Typed access** over the finished tree: [`Element`](crate::cst::Element),
  [`Node`](crate::cst::Node), [`Token`](crate::cst::Token), and
  [`cst::cast`](crate::cst::cast) wrap and cast rowan elements without changing the
  losslessness story.
- **The event vocabulary**, its depth model, and the era-branded mark validation are
  specified in [`cst::event`](crate::cst::event) — the normative reference behind
  everything this chapter demonstrated.
- [`SyntaxTreeBuilder`](crate::cst::SyntaxTreeBuilder) remains as the low-level, append-only
  escape hatch over rowan's builder for code that constructs trees outside a parse. Inside a
  parse, prefer the sink: the builder knows nothing about rollback.
- Keep the **round-trip oracle** in your dialect's test suite:
  `tree.text() == source` over your whole corpus — including inputs with lexer errors and
  recovery holes — is the one assertion that catches a skipped token, a double emission, or
  a span drift, and this chapter showed it holding by construction.
