#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The `Dialect` anchor carries a lexer's equality block to every production.
//!
//! Without an anchor, a production that wants `*inp.offset()` to be a `usize` and its spans to
//! be `SimpleSpan` has to restate those equalities itself, once per production. With it, the
//! equalities are pinned once in a subtrait of `Dialect` and *elaborate* to each production, so
//! a signature carries two `where`-clause lines and says nothing about span, offset, source,
//! slice or the token's classification capabilities.
//!
//! This file is that boundary, not a claim about it. Every equality the productions below rely
//! on is stated in exactly one place — `PinnedDialect`'s supertrait — and nowhere else, so
//! removing any one of them stops this target compiling:
//!
//! | Remove from `PinnedDialect` | What stops compiling |
//! |---|---|
//! | `Span = SimpleSpan` | `SimpleSpan::new(…)`'s return type, and the `Ident`'s span parameter |
//! | `Offset = usize` | `let start: usize = *inp.offset()` |
//! | `Token: IdentifierToken<'inp>` | `Ident::parse` has no classifier |
//!
//! All three were removed one at a time and each produced a compile error; the productions were
//! then checked to compile with the block intact. A rail that only ever ran in its passing state
//! would prove nothing.
//!
//! **`PunctuatorToken` is deliberately *not* pinned here, and the reason is a finding.** A real
//! dialect pins it — the consumer's does, alongside `IdentifierToken` and `KeywordToken` — but
//! in *this* file it would not be load-bearing: the only production that needs a punctuator
//! classification is the delimited one, and it already carries `Brace<…>: TypedDelimiter<…>`, which
//! supplies the obligation on its own. Removing the pin was measured and the target still
//! compiled. Pinning it anyway would put a line in the table that cannot fail, which is the
//! defect class this file exists to avoid.

mod common;

use core::marker::PhantomData;

use common::{TestLexer, Token, TokenKind};

use tokora::{
  ComposableParseContext, Dialect, DialectErrorOf, DialectInput, DialectSlice, Lexer, LexerOf,
  Parse, ParseInput, Parser, SimpleSpan,
  delimiter::TypedDelimiter,
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  punct::Brace,
  token::IdentifierToken,
  types::Ident,
};

// The shared fixture token wires only `PunctuatorToken`; `Ident::parse` classifies through
// `IdentifierToken`, so the identifier facet lives with the suite that needs it.
impl IdentifierToken<'_> for Token {
  fn is_identifier(&self) -> bool {
    matches!(self, Token::Ident)
  }
}

// ── The anchor: one impl, and the only place the lexer is named ────────────────

/// The language brand. Deliberately **not** `()`: a dialect that only worked unbranded would
/// not be evidence for a branded consumer, which is every real one.
struct TestLang;

/// The dialect. A `PhantomData` carrier — an anchor is a type-level name, never a value.
struct TestDialect<'inp>(PhantomData<&'inp ()>);

impl<'inp> Dialect<'inp> for TestDialect<'inp> {
  type Lang = TestLang;
  type Lexer = TestLexer<'inp>;
}

/// The equality block, stated **once**. This is the line the whole design turns on: every
/// production below inherits `Span`, `Offset` and the identifier capability from here.
trait PinnedDialect<'inp>:
  Dialect<
    'inp,
    Lang = TestLang,
    Lexer: Lexer<'inp, Span = SimpleSpan, Offset = usize, Token: IdentifierToken<'inp>>,
  >
{
}

impl<'inp, D> PinnedDialect<'inp> for D where
  D: Dialect<
      'inp,
      Lang = TestLang,
      Lexer: Lexer<'inp, Span = SimpleSpan, Offset = usize, Token: IdentifierToken<'inp>>,
    >
{
}

// ── Two where-clause lines, and nothing restated ───────────────────────────────

/// The identifier this dialect's atoms yield. Spelled through the anchor: the slice comes from
/// [`DialectSlice`], the span from the pinned equality, the brand from the dialect.
type DialectIdent<'inp, D> = Ident<DialectSlice<'inp, D>, SimpleSpan, TestLang>;

/// The shape the anchor is for. Two `where`-clause lines. It reads an offset as a `usize`,
/// builds a `SimpleSpan` from it, and calls a leaf atom that raises three of the bundle's five
/// conversions — while naming no error type, no emitter capability, no `From`, no source, no
/// slice, and neither `Span` nor `Offset`.
fn spanned_ident<'inp, D, Ctx>(
  inp: &mut DialectInput<'inp, '_, D, Ctx>,
) -> Result<(DialectIdent<'inp, D>, SimpleSpan), DialectErrorOf<'inp, D, Ctx>>
where
  D: PinnedDialect<'inp>,
  Ctx: ComposableParseContext<'inp, LexerOf<'inp, D>, TestLang>,
{
  // `Offset = usize` had to elaborate for the annotation to hold…
  let start: usize = *inp.offset();
  let ident = Ident::parse(inp)?;
  let end: usize = *inp.offset();
  // …and `Span = SimpleSpan` for this to be the declared return type.
  Ok((ident, SimpleSpan::new(start, end)))
}

/// The three-line shape, stated honestly. A delimiter pair is not a fact about a dialect — the
/// grammar chooses it per production — so `Brace<…>: TypedDelimiter<…>` is a genuine third line
/// rather than something the anchor failed to carry. Everything else still rides the anchor,
/// including the `FromUnclosed` conversion the unclosed path needs.
///
/// The pair is named at the dialect's own language — `Brace<(), (), TestLang>` — because the
/// brand is not decoration: a `Brace` branded for a sibling dialect is not this one's pair.
/// Only the generic lexer forces the line to be written at all; at a concrete lexer the impl
/// resolves and the bound is inferred.
fn braced_ident<'inp, D, Ctx>(
  inp: &mut DialectInput<'inp, '_, D, Ctx>,
) -> Result<DialectIdent<'inp, D>, DialectErrorOf<'inp, D, Ctx>>
where
  D: PinnedDialect<'inp>,
  Ctx: ComposableParseContext<'inp, LexerOf<'inp, D>, TestLang>,
  Brace<(), (), TestLang>: TypedDelimiter<'inp, LexerOf<'inp, D>, TestLang>,
{
  Ident::parse.delimited_by_braces()(inp).map(|delimited| delimited.into_data())
}

// ── The dialect's error, in the one shape a consumer writes ────────────────────
//
// One `Set`-generic, `Lang`-generic end-of-input impl for both of the bundle's end-of-input
// members, and one `FromUnclosed` impl quantified over the delimiter pair. `common::E`'s
// end-of-input conversion is pinned to `Lang = ()` on purpose, so a branded dialect needs its
// own error fixture rather than a widening of the shared one.

#[derive(Debug)]
struct DialectErr;

impl From<()> for DialectErr {
  fn from(_: ()) -> Self {
    DialectErr
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for DialectErr
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    DialectErr
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for DialectErr {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    DialectErr
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for DialectErr {
  fn from(_: FullContainer<S, Lang>) -> Self {
    DialectErr
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for DialectErr {
  fn from(_: TooFew<S, Lang>) -> Self {
    DialectErr
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for DialectErr {
  fn from(_: TooMany<S, Lang>) -> Self {
    DialectErr
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for DialectErr {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    DialectErr
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for DialectErr
{
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    DialectErr
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for DialectErr {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    DialectErr
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for DialectErr
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    DialectErr
  }
}

// ── …and it runs ───────────────────────────────────────────────────────────────
//
// A rail that only type-checks is a rail that could be satisfied by a signature nobody can
// call. These drive the productions at the concrete dialect.

type Ctx<'inp> = tokora::FatalContext<'inp, TestLexer<'inp>, DialectErr, TestLang>;

// The drivers turbofish `D`, and that is a property of the *fixture*, not a wart in the anchor:
// `DialectInput` expands to an `InputRef` over `LexerOf<'inp, D>`, and a projection is not
// injective, so `D` is recoverable from the handle only when the dialect appears injectively in
// its own lexer — as `Graphql<Src>` does in `SyntacticLexer<'inp, Src>`. Here two dialects share
// one lexer on purpose (see `AliasDialect` below), so `D` genuinely is ambiguous and rustc is
// right to ask. Generic productions calling each other with `D` already in scope never hit it.

#[test]
fn the_anchor_drives_a_two_line_production() {
  fn drive<'inp>(
    inp: &mut DialectInput<'inp, '_, TestDialect<'inp>, Ctx<'inp>>,
  ) -> Result<SimpleSpan, DialectErr> {
    spanned_ident::<TestDialect<'inp>, _>(inp).map(|(_, span)| span)
  }

  let span = Parser::with_parser::<'_, TestLexer<'_>, SimpleSpan, DialectErr, _, TestLang>(drive)
    .parse_str("name")
    .unwrap();
  assert_eq!((span.start(), span.end()), (0, 4));

  // End of input reaches the bundle's end-of-input conversion, which the signature never named.
  assert!(
    Parser::with_parser::<'_, TestLexer<'_>, SimpleSpan, DialectErr, _, TestLang>(drive)
      .parse_str("")
      .is_err()
  );
}

#[test]
fn the_anchor_drives_the_delimited_production() {
  fn drive<'inp>(
    inp: &mut DialectInput<'inp, '_, TestDialect<'inp>, Ctx<'inp>>,
  ) -> Result<TokenKind, DialectErr> {
    braced_ident::<TestDialect<'inp>, _>(inp).map(|_| TokenKind::Ident)
  }

  assert_eq!(
    Parser::with_parser::<'_, TestLexer<'_>, TokenKind, DialectErr, _, TestLang>(drive)
      .parse_str("{name}")
      .unwrap(),
    TokenKind::Ident
  );

  // The opener commits and the closer never arrives: the diagnostic routes through
  // `FromUnclosed`, a conversion `braced_ident` never named.
  assert!(
    Parser::with_parser::<'_, TestLexer<'_>, TokenKind, DialectErr, _, TestLang>(drive)
      .parse_str("{name")
      .is_err()
  );
}

// ── The anchor is only an anchor: it must not gain associated items ────────────

/// A second dialect over the *same* lexer at the *same* brand, differing in nothing. It
/// compiles because `Dialect` has exactly two associated items and both are already fixed by
/// `PinnedDialect` — which is the property that lets a consumer instantiate one grammar over
/// several sources. An extra associated item would make this a second thing to spell.
struct AliasDialect<'inp>(PhantomData<&'inp ()>);

impl<'inp> Dialect<'inp> for AliasDialect<'inp> {
  type Lang = TestLang;
  type Lexer = TestLexer<'inp>;
}

fn assert_pinned<'inp, D: PinnedDialect<'inp>>() {}

#[test]
fn both_dialects_satisfy_the_pinned_subtrait() {
  assert_pinned::<TestDialect<'_>>();
  assert_pinned::<AliasDialect<'_>>();
}
