#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The built-in punctuator markers are usable as separators under a branded language.
//!
//! The marker's own brand and the context language are the **same** parameter: `Comma<(), (),
//! LangA>` is a punctuator of `LangA` and of nothing else. The fluent family is still reachable
//! at a branded grammar because `define_separated_by!` instantiates the separator at the
//! caller's language — `Comma<(), (), Lang>` — not because the impl is widened.
//!
//! These tests pin the positive half of that: equality at a branded language, equality at the
//! unbranded one, and the two fluent methods running end to end at `Lang = TestLang`. The
//! negative half — a marker branded for one grammar must not satisfy another — cannot be
//! written as a passing test, so it lives as the `compile_fail,E0277` doctest on
//! `tokora::punct::Punctuator`, paired there with a positive control differing in one token.
//!
//! The fixture error is local rather than `common::E` because `E`'s end-of-input conversion
//! is pinned to `Lang = ()`; every other fixture is shared.

mod common;

use common::{TestLexer, Token};
use tokora::EmitterView;

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

/// The marker `define_separated_by!` emits, asked for at the language it is branded with.
///
/// The bare `Comma` — i.e. `Comma<(), (), ()>` — is deliberately absent at `TestLang`: it is a
/// punctuator of `()` only. See the `compile_fail` doctest on `Punctuator` for that half.
#[test]
fn a_marker_is_a_punctuator_at_its_own_lang() {
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
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter, TestLang>,
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
