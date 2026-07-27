#![cfg(all(feature = "std", feature = "logos"))]

//! The built-in punctuator markers are usable as separators under a branded language.
//!
//! `define_separated_by!` hard-codes the bare marker — `Comma<(), (), ()>` — into every
//! generated `separated_by_*` method, so the whole fluent family is reachable only if the
//! marker's own language parameter is independent of the context language. These tests pin
//! that independence at `Lang = TestLang`; they do not compile if the two are tied together.
//!
//! The fixture error is local rather than `common::E` because `E`'s end-of-input conversion
//! is pinned to `Lang = ()`; every other fixture is shared.

mod common;

use common::{TestLexer, Token};

use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::{DefaultCache, Peeked},
  emitter::{
    FullContainerEmitter, SeparatedEmitter, Silent, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    TokenHint, Unclosed, UnexpectedEnd,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
  punct::{Comma, Punctuator},
  utils::typenum::U1,
};

/// A branded language marker, in the shape a real grammar declares one.
struct TestLang;

/// `common::E`, with its end-of-input conversion quantified over the language.
#[derive(Debug)]
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

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for BE {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    BE
  }
}

type BrandedCtx<'inp> = ParserContext<
  'inp,
  TestLexer<'inp>,
  Silent<BE, TestLang>,
  DefaultCache<'inp, TestLexer<'inp>>,
  TestLang,
>;

fn branded_ctx<'inp>() -> BrandedCtx<'inp> {
  ParserContext::of(Silent::new())
}

fn assert_punct<'inp, L, P, Lang>()
where
  L: Lexer<'inp>,
  P: Punctuator<'inp, L, Lang>,
  Lang: ?Sized,
{
}

/// The bare marker `define_separated_by!` emits, asked for at a branded language.
#[test]
fn bare_marker_is_a_punctuator_at_a_branded_lang() {
  assert_punct::<TestLexer<'_>, Comma, TestLang>();
  assert_punct::<TestLexer<'_>, Comma<(), (), TestLang>, TestLang>();
  assert_punct::<TestLexer<'_>, Comma, ()>();
}

fn parse_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, TestLang>,
) -> Result<i64, BE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, TestLang>,
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

fn decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: &mut Ctx::Emitter,
) -> Result<Action, BE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, TestLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, TestLang, Error = BE>,
{
  Ok(match peeked.pop_front() {
    None => Action::Stop,
    Some(tok) => {
      let tok = tok
        .as_maybe_ref()
        .map(|t| t.token().copied(), |t| t.token())
        .into_inner();
      if matches!(**tok.data(), Token::Num(_)) {
        Action::Continue
      } else {
        Action::Stop
      }
    }
  })
}

/// The fluent separator family, driven at a branded language.
fn branded_sep_while<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx, TestLang>,
) -> Result<Vec<i64>, BE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>, TestLang>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, TestLang, Error = BE>
    + SeparatedEmitter<'inp, TestLexer<'inp>, TestLang>
    + FullContainerEmitter<'inp, TestLexer<'inp>, TestLang>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>, TestLang>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>, TestLang>
    + TooManyEmitter<'inp, TestLexer<'inp>, TestLang>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

fn run_branded(src: &str) -> Result<Vec<i64>, BE> {
  Parser::with_context::<TestLexer<'_>, Vec<i64>, _>(branded_ctx())
    .apply::<_, TestLang>(branded_sep_while)
    .parse_str(src)
}

#[test]
fn fluent_separated_by_comma_runs_at_a_branded_lang() {
  assert_eq!(run_branded("1,2,3+").unwrap(), vec![1, 2, 3]);
}

#[test]
fn fluent_separated_by_comma_honours_allow_leading_at_a_branded_lang() {
  assert_eq!(run_branded(",1,2+").unwrap(), vec![1, 2]);
}
