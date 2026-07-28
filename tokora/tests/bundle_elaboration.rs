#![cfg(all(feature = "std", feature = "logos"))]

//! One `ComposableParseContext` bound is enough to run a leaf grammar.
//!
//! Before the bundle carried [`FromTokenErrors`](tokora::emitter::FromTokenErrors), a leaf atom
//! could not be reached from a context bound alone: `peek_kind`, `Ident::parse`,
//! `Ident::try_parse` and the delimited family each demanded their own `From<…>` line on the
//! error projection, so every production restated the ladder the bundle was supposed to have
//! bundled. The functions below carry **exactly one** context bound and nothing about the error
//! type. They do not compile if `ComposableParseContext` stops naming `Error: FromTokenErrors`
//! in its supertrait path.
//!
//! The elaboration is pinned three ways, deliberately:
//!
//! 1. **Fully generic in `Lang`** — the bundle must discharge the obligations without knowing
//!    the language brand, which is the only form a library production can be written in.
//! 2. **Driven at `Lang = ()`** through the shared `common::E`.
//! 3. **Driven at a branded `Lang`** through a local error fixture. `common::E`'s end-of-input
//!    conversion is pinned to `Lang = ()` on purpose, so a branded exercise needs its own
//!    fixture rather than a widening of the shared one — the pin stays a tripwire for every
//!    other suite.

mod common;

use common::{E, TestLexer, Token, TokenKind};

use tokora::{
  ComposableParseContext, FatalContext, InputRef, Lexer, Parse, ParseInput, Parser,
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  input::SurfaceIncomplete,
  parser::peek_kind,
  punct::Brace,
  token::IdentifierToken,
  try_parse_input::ParseAttempt,
  types::Ident,
};

// The shared fixture token wires only `PunctuatorToken`; `Ident::parse` classifies through
// `IdentifierToken`, so the identifier facet lives with the suite that needs it.
impl IdentifierToken<'_> for Token {
  fn is_identifier(&self) -> bool {
    matches!(self, Token::Ident)
  }
}

// ── The rail: one bound, four leaf conversions ─────────────────────────────────

/// The identifier a leaf atom yields — a projection chain, named once.
type IdentOf<'inp, L, Lang> = Ident<
  <<L as Lexer<'inp>>::Source as tokora::source::Source<<L as Lexer<'inp>>::Offset>>::Slice<'inp>,
  <L as Lexer<'inp>>::Span,
  Lang,
>;

/// `peek_kind`'s result, named once.
type KindOf<'inp, L, Ctx, Lang> = Result<
  Option<<<L as Lexer<'inp>>::Token as tokora::Token<'inp>>::Kind>,
  tokora::ErrorOf<'inp, L, Ctx, Lang>,
>;

/// `peek_kind` rides `try_expect_or_stop`, so it raises `UnexpectedEot<L::Offset, Lang>`.
/// Nothing here says so.
fn bundled_peek_kind<'inp, L, Ctx, Lang>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
) -> KindOf<'inp, L, Ctx, Lang>
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
{
  peek_kind(inp)
}

/// `Ident::parse` raises both the end-of-input and the unexpected-token conversion.
fn bundled_ident<'inp, L, Ctx, Lang, Cmpl>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
) -> Result<IdentOf<'inp, L, Lang>, tokora::ErrorOf<'inp, L, Ctx, Lang>>
where
  L: Lexer<'inp>,
  L::Token: IdentifierToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  Lang: ?Sized,
{
  Ident::parse(inp)
}

/// The attempt twin — same conversions, declining shape.
fn bundled_try_ident<'inp, L, Ctx, Lang, Cmpl>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
) -> Result<ParseAttempt<IdentOf<'inp, L, Lang>>, tokora::ErrorOf<'inp, L, Ctx, Lang>>
where
  L: Lexer<'inp>,
  L::Token: IdentifierToken<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  Lang: ?Sized,
{
  Ident::try_parse(inp)
}

/// The braced identifier the delimited production yields. The pair is named at the caller's
/// language: a `Brace` branded for a sibling grammar is not this grammar's pair.
type BracedIdentOf<'inp, L, Ctx, Lang> =
  tokora::parser::DelimitedOf<'inp, Brace<(), (), Lang>, L, Ctx, Lang, IdentOf<'inp, L, Lang>>;

/// The delimited family adds the fifth member: `FromUnclosed`, quantified over the pair.
/// This signature names neither `Brace` in a conversion nor `FromUnclosed` at all.
fn bundled_braced_ident<'inp, L, Ctx, Lang>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
) -> BracedIdentOf<'inp, L, Ctx, Lang>
where
  L: Lexer<'inp>,
  L::Token: IdentifierToken<'inp>,
  Brace<(), (), Lang>: tokora::delimiter::TypedDelimiter<'inp, L, Lang>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
{
  Ident::parse.delimited_by_braces()(inp)
}

/// The four together, from one bound — the shape a real production has.
fn bundled_production<'inp, L, Ctx, Lang>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
) -> Result<usize, tokora::ErrorOf<'inp, L, Ctx, Lang>>
where
  L: Lexer<'inp>,
  L::Token: IdentifierToken<'inp>,
  Brace<(), (), Lang>: tokora::delimiter::TypedDelimiter<'inp, L, Lang>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
{
  let mut seen = 0usize;
  if bundled_peek_kind(inp)?.is_some() {
    seen += 1;
  }
  if let ParseAttempt::Accept(_) = bundled_try_ident(inp)? {
    seen += 1;
  }
  bundled_braced_ident(inp)?;
  seen += 1;
  Ok(seen)
}

// ── Arm 1: driven unbranded, through the shared fixture error ──────────────────

#[test]
fn bundle_drives_the_leaf_atoms_unbranded() {
  type Ctx<'inp> = FatalContext<'inp, TestLexer<'inp>, E>;

  fn drive<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<usize, E> {
    bundled_production(inp)
  }

  assert_eq!(Parser::with_parser(drive).parse_str("a {b}").unwrap(), 3);
  // A single identifier: the braced group is missing, so the atom errors — the point is that
  // the error is produced through the bundle, not that it parses.
  assert!(Parser::with_parser(drive).parse_str("a").is_err());
}

#[test]
fn bundle_reaches_ident_parse_unbranded() {
  type Ctx<'inp> = FatalContext<'inp, TestLexer<'inp>, E>;

  fn drive<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<TokenKind, E> {
    let ident = bundled_ident(inp)?;
    let _ = ident;
    Ok(TokenKind::Ident)
  }

  assert_eq!(
    Parser::with_parser(drive).parse_str("name").unwrap(),
    TokenKind::Ident
  );
  // End of input reaches the `UnexpectedEot` conversion the bundle supplies.
  assert!(Parser::with_parser(drive).parse_str("").is_err());
}

// ── Arm 2: driven branded ──────────────────────────────────────────────────────
//
// `common::E`'s end-of-input conversion is `Lang = ()`-pinned by design, so the branded arm
// carries its own error fixture. Its end-of-input impl is quantified over BOTH `Lang` and the
// expected set `Set` — one impl for both of the bundle's end-of-input members, which is exactly
// the single impl a consumer dialect writes.

/// A branded language marker, in the shape a real grammar declares one.
struct TestLang;

#[derive(Debug)]
struct BrandedErr;

impl From<()> for BrandedErr {
  fn from(_: ()) -> Self {
    BrandedErr
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for BrandedErr
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    BrandedErr
  }
}

/// The one impl that satisfies **both** end-of-input members of the bundle: generic in `Set`
/// (the `_or_stop` family's `&'static str` and the dispatch drivers' `Kind`) and in `Lang`.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for BrandedErr {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    BrandedErr
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for BrandedErr {
  fn from(_: FullContainer<S, Lang>) -> Self {
    BrandedErr
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for BrandedErr {
  fn from(_: TooFew<S, Lang>) -> Self {
    BrandedErr
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for BrandedErr {
  fn from(_: TooMany<S, Lang>) -> Self {
    BrandedErr
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for BrandedErr {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    BrandedErr
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for BrandedErr
{
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    BrandedErr
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for BrandedErr {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    BrandedErr
  }
}

/// The umbrella, quantified over the pair — the whole delimiter half in one impl.
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for BrandedErr
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    BrandedErr
  }
}

#[test]
fn bundle_drives_the_leaf_atoms_at_a_branded_lang() {
  type BrandedCtx<'inp> = FatalContext<'inp, TestLexer<'inp>, BrandedErr, TestLang>;

  fn drive<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, BrandedCtx<'inp>, TestLang>,
  ) -> Result<usize, BrandedErr> {
    bundled_production(inp)
  }

  let parsed = Parser::with_parser::<'_, TestLexer<'_>, usize, BrandedErr, _, TestLang>(drive)
    .parse_str("a {b}");
  assert_eq!(parsed.unwrap(), 3);

  // The unclosed path: the opener commits and the closer never arrives, so the diagnostic
  // routes through `FromUnclosed` — a conversion the production above never names.
  let unclosed = Parser::with_parser::<'_, TestLexer<'_>, usize, BrandedErr, _, TestLang>(drive)
    .parse_str("a {b");
  assert!(unclosed.is_err());
}

// ── The bundle holds for the built-in contexts, at both languages ──────────────

fn assert_bundle<'inp, L, Ctx, Lang>()
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
{
}

#[test]
fn bundle_holds_for_the_builtin_contexts() {
  assert_bundle::<TestLexer<'_>, FatalContext<'_, TestLexer<'_>, E>, ()>();
  assert_bundle::<TestLexer<'_>, FatalContext<'_, TestLexer<'_>, BrandedErr, TestLang>, TestLang>();
}
