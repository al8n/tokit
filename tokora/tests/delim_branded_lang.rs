#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The built-in delimiter pairs are usable under a branded language.
//!
//! The pair marker's own brand and the context language are the **same** parameter:
//! `Bracket<(), (), LangA>` is a delimiter of `LangA` and of nothing else. The fluent
//! `delimited_by_*` families are still reachable at a branded grammar because the macros that
//! generate them instantiate the pair at the caller's language, not because the impls are
//! widened.
//!
//! These tests pin the positive half. The negative half — a pair branded for one grammar must
//! not satisfy another — cannot be written as a passing test, so it lives as the two
//! `compile_fail,E0277` doctests on `tokora::delimiter::Delimiter`, paired there with a
//! positive control differing from each in one token.
//!
//! **The option-builder row is the one that would not have been noticed.** Nine of the eleven
//! many-builder surfaces carry `Lang` in their own type, so their `delimited_by_*` pin the pair
//! to it and a mismatch is a compile error at the method. The seven option builders are
//! `impl<P> AtLeast<P>` and friends — generic only over the parser they wrap, with no language
//! to name — so their four methods take the brand as a *method* parameter and rely on the
//! driving impl's `Delim: Delimiter<'inp, L, Lang>` obligation to unify it with the context
//! language. That inference is exercised at `Lang = ()` by several existing files and was
//! exercised at a branded language by none, which is what `branded_at_least` is for.
//!
//! The fixture error is local rather than `common::E` because `E`'s end-of-input conversion is
//! pinned to `Lang = ()`; every other fixture is shared.

mod common;

use common::{TestLexer, Token};

use tokora::{
  Accumulator, ComposableParseContext, Emitter, InputRef, Lexer, Parse, ParseInput, Parser,
  ParserContext, TryParseInput,
  cache::DefaultCache,
  delimiter::TypedDelimiter,
  emitter::Fatal,
  error::{
    TokenHint, Unclosed, UnexpectedEnd,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  punct::Bracket,
  try_parse_input::ParseAttempt,
};

/// A branded language marker, in the shape a real grammar declares one.
struct TestLang;

/// `common::E`, with every conversion quantified over the language.
#[derive(Debug, PartialEq)]
struct BE;

impl From<()> for BE {
  fn from(_: ()) -> Self {
    BE
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for BE {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    BE
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for BE {
  fn from(_: FullContainer<S, Lang>) -> Self {
    BE
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for BE {
  fn from(_: TooFew<S, Lang>) -> Self {
    BE
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for BE {
  fn from(_: TooMany<S, Lang>) -> Self {
    BE
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<TokenHint, O, Lang, Set>> for BE {
  fn from(_: UnexpectedEnd<TokenHint, O, Lang, Set>) -> Self {
    BE
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for BE {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    BE
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for BE {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    BE
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for BE {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    BE
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for BE
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    BE
  }
}

type BrandedCtx<'inp> = ParserContext<
  'inp,
  TestLexer<'inp>,
  Fatal<BE, TestLang>,
  DefaultCache<'inp, TestLexer<'inp>>,
  TestLang,
>;

fn branded_ctx<'inp>() -> BrandedCtx<'inp> {
  ParserContext::of(Fatal::<BE, TestLang>::of())
}

fn assert_delim<'inp, L, D, Lang>()
where
  L: Lexer<'inp>,
  D: TypedDelimiter<'inp, L, Lang>,
  Lang: ?Sized,
{
}

/// The pair the fluent families emit, asked for at the language it is branded with.
///
/// The bare `Bracket` — i.e. `Bracket<(), (), ()>` — is deliberately absent at `TestLang`: it
/// is a delimiter of `()` only. See the `compile_fail` doctests on `Delimiter` for that half.
#[test]
fn a_pair_is_a_delimiter_at_its_own_lang() {
  assert_delim::<TestLexer<'_>, Bracket<(), (), TestLang>, TestLang>();
  assert_delim::<TestLexer<'_>, Bracket, ()>();
}

// ── The fixtures the drivers run ──────────────────────────────────────────────

fn parse_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, TestLang>,
) -> Result<i64, BE>
where
  Ctx: ComposableParseContext<'inp, TestLexer<'inp>, TestLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, TestLang, Error = BE>,
{
  match inp.next()? {
    None => Err(BE),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(BE),
    },
  }
}

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, TestLang>,
) -> Result<ParseAttempt<i64>, BE>
where
  Ctx: ComposableParseContext<'inp, TestLexer<'inp>, TestLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, TestLang, Error = BE>,
{
  inp
    .try_expect(|t| matches!(t.data(), Token::Num(_)))
    .map(|opt| match opt {
      None => ParseAttempt::Decline,
      Some(tok) => ParseAttempt::Accept(match tok.into_data() {
        Token::Num(n) => n,
        _ => unreachable!(),
      }),
    })
}

// ── The four rows ─────────────────────────────────────────────────────────────

/// `ParseInput`'s fluent form — the pair is pinned by `define_delimited_by!`.
fn branded_parse_input<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, BrandedCtx<'inp>, TestLang>,
) -> Result<i64, BE> {
  parse_num.delimited_by_brackets()(inp).map(|delimited| *delimited.data())
}

/// The attempt twin — the pair is pinned by `define_try_delimited_by!`.
fn branded_try_parse_input<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, BrandedCtx<'inp>, TestLang>,
) -> Result<i64, BE> {
  match parse_num.try_delimited_by_brackets()(inp)? {
    Some(delimited) => Ok(*delimited.data()),
    None => Err(BE),
  }
}

/// A repetition driver's fluent form — `Repeated` carries `Lang`, so the pair is pinned to it.
fn branded_repeated<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, BrandedCtx<'inp>, TestLang>,
) -> Result<Vec<i64>, BE> {
  try_num
    .repeated()
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

/// An option builder's fluent form — `AtLeast<P>` has no language to name, so the brand is a
/// method parameter and the driving impl's obligation is what resolves it. Nothing else in the
/// suite drives this row at a branded language.
fn branded_at_least<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, BrandedCtx<'inp>, TestLang>,
) -> Result<Vec<i64>, BE> {
  try_num
    .repeated()
    .at_least(1)
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

macro_rules! run {
  ($parser:ident, $src:literal) => {
    Parser::with_context::<TestLexer<'_>, _, _>(branded_ctx())
      .apply::<_, TestLang>($parser)
      .parse_str($src)
  };
}

#[test]
fn fluent_delimited_by_brackets_runs_at_a_branded_lang() {
  assert_eq!(run!(branded_parse_input, "[1]").unwrap(), 1);
  // The closer never arrives, so the pair's `Unclosed` routes through `FromUnclosed`.
  assert_eq!(run!(branded_parse_input, "[1"), Err(BE));
}

#[test]
fn fluent_try_delimited_by_brackets_runs_at_a_branded_lang() {
  assert_eq!(run!(branded_try_parse_input, "[1]").unwrap(), 1);
  // The opener is absent, so the attempt declines and the fixture turns that into `BE`.
  assert_eq!(run!(branded_try_parse_input, "1"), Err(BE));
}

#[test]
fn fluent_repeated_delimited_by_brackets_runs_at_a_branded_lang() {
  assert_eq!(run!(branded_repeated, "[1 2 3]").unwrap(), vec![1, 2, 3]);
}

#[test]
fn fluent_at_least_delimited_by_brackets_runs_at_a_branded_lang() {
  assert_eq!(run!(branded_at_least, "[1 2]").unwrap(), vec![1, 2]);
}
