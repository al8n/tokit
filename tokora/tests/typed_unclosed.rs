#![cfg(all(feature = "std", feature = "logos"))]

mod common;

use common::{TestLexer, Token, TokenKind};
use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, FatalContext, InputRef, Parse, ParseContext, ParseInput, Parser, SimpleSpan,
  cache::Peeked,
  delimiter::{Delimiter, DelimiterKind},
  emitter::FromUnclosed,
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::{Action, DelimitedBy, delimited},
  punct::{Brace, Bracket, CloseBrace, CloseBracket, OpenBrace, OpenBracket},
  utils::CowStr,
};

#[derive(Debug)]
enum CustomLang {}

/// A deliberately non-`Clone`, non-`Copy`, non-`Default` delimiter marker.
#[derive(Debug)]
struct CustomMarker;

impl<'inp> Delimiter<'inp, TestLexer<'inp>, CustomLang> for CustomMarker {
  const KIND: DelimiterKind = DelimiterKind::Custom("custom brackets");

  type Open = OpenBracket<(), (), CustomLang>;
  type Close = CloseBracket<(), (), CustomLang>;

  fn name() -> CowStr {
    CowStr::from_static("custom brackets")
  }
}

#[derive(Debug)]
enum TypedError {
  Bracket(Unclosed<Bracket, SimpleSpan, CustomLang>),
  Brace(Unclosed<Brace, SimpleSpan, CustomLang>),
  CustomMarker(Unclosed<CustomMarker, SimpleSpan, CustomLang>),
  Other,
}

impl From<()> for TypedError {
  fn from(_: ()) -> Self {
    Self::Other
  }
}

impl From<Unclosed<Bracket, SimpleSpan, CustomLang>> for TypedError {
  fn from(err: Unclosed<Bracket, SimpleSpan, CustomLang>) -> Self {
    Self::Bracket(err)
  }
}

impl From<Unclosed<Brace, SimpleSpan, CustomLang>> for TypedError {
  fn from(err: Unclosed<Brace, SimpleSpan, CustomLang>) -> Self {
    Self::Brace(err)
  }
}

impl From<Unclosed<CustomMarker, SimpleSpan, CustomLang>> for TypedError {
  fn from(err: Unclosed<CustomMarker, SimpleSpan, CustomLang>) -> Self {
    Self::CustomMarker(err)
  }
}

// The umbrella conversion the delimited drivers now demand. `from_unclosed` is generic over
// the pair, so the typed arm is selected by the runtime `DelimiterKind` the `Unclosed`
// carries; the catch-all is mandatory because no arm set over a generic `D` is exhaustive.
impl<'inp, L> tokora::emitter::FromUnclosed<'inp, L, CustomLang> for TypedError
where
  L: tokora::Lexer<'inp, Span = SimpleSpan>,
{
  fn from_unclosed<D>(err: Unclosed<D, SimpleSpan, CustomLang>) -> Self {
    let kind = err.kind();
    let (span, name) = err.into_components();
    match kind {
      DelimiterKind::Bracket { .. } => Self::Bracket(Unclosed::of(span, kind, name)),
      DelimiterKind::Brace { .. } => Self::Brace(Unclosed::of(span, kind, name)),
      DelimiterKind::Custom("custom brackets") => {
        Self::CustomMarker(Unclosed::of(span, kind, name))
      }
      _ => Self::Other,
    }
  }
}

impl<'inp, T, K: Clone, S> From<UnexpectedToken<'inp, T, K, S, CustomLang>> for TypedError {
  fn from(_: UnexpectedToken<'inp, T, K, S, CustomLang>) -> Self {
    Self::Other
  }
}

impl<O, Set: Clone + 'static> From<UnexpectedEot<O, CustomLang, Set>> for TypedError {
  fn from(_: UnexpectedEot<O, CustomLang, Set>) -> Self {
    Self::Other
  }
}

impl<S> From<FullContainer<S, CustomLang>> for TypedError {
  fn from(_: FullContainer<S, CustomLang>) -> Self {
    Self::Other
  }
}

impl<S> From<TooFew<S, CustomLang>> for TypedError {
  fn from(_: TooFew<S, CustomLang>) -> Self {
    Self::Other
  }
}

impl<'inp, Kind: Clone, O> From<MissingToken<'inp, Kind, O, CustomLang>> for TypedError {
  fn from(_: MissingToken<'inp, Kind, O, CustomLang>) -> Self {
    Self::Other
  }
}

impl<'inp, T, Kind: Clone, S> From<SeparatedError<'inp, T, Kind, S, CustomLang>> for TypedError {
  fn from(_: SeparatedError<'inp, T, Kind, S, CustomLang>) -> Self {
    Self::Other
  }
}

impl<O> From<MissingSyntax<O, CustomLang>> for TypedError {
  fn from(_: MissingSyntax<O, CustomLang>) -> Self {
    Self::Other
  }
}

impl From<OpenBracket<(), (), CustomLang>> for TokenKind {
  fn from(_: OpenBracket<(), (), CustomLang>) -> Self {
    Self::LBracket
  }
}

impl From<CloseBracket<(), (), CustomLang>> for TokenKind {
  fn from(_: CloseBracket<(), (), CustomLang>) -> Self {
    Self::RBracket
  }
}

impl From<OpenBrace<(), (), CustomLang>> for TokenKind {
  fn from(_: OpenBrace<(), (), CustomLang>) -> Self {
    Self::LBrace
  }
}

impl From<CloseBrace<(), (), CustomLang>> for TokenKind {
  fn from(_: CloseBrace<(), (), CustomLang>) -> Self {
    Self::RBrace
  }
}

type Ctx<'inp> = FatalContext<'inp, TestLexer<'inp>, TypedError, CustomLang>;

fn number<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, CustomLang>,
) -> Result<i64, TypedError> {
  match inp.next()? {
    Some(token) => match token.into_data() {
      Token::Num(number) => Ok(number),
      _ => Err(TypedError::Other),
    },
    None => Err(TypedError::Other),
  }
}

fn decide_number<'inp>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: &mut <Ctx<'inp> as ParseContext<'inp, TestLexer<'inp>, CustomLang>>::Emitter,
) -> Result<Action, TypedError> {
  Ok(match peeked.pop_front() {
    None => Action::Stop,
    Some(token) => {
      let token = token
        .as_maybe_ref()
        .map(|token| token.token().copied(), |token| token.token())
        .into_inner();
      if matches!(**token.data(), Token::Num(_)) {
        Action::Continue
      } else {
        Action::Stop
      }
    }
  })
}

fn bare_bracket_many<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, CustomLang>,
) -> Result<Vec<i64>, TypedError> {
  number
    .repeated_while::<_, U1>(decide_number)
    .delimited::<Bracket>()
    .collect()
    .parse_input(inp)
}

fn custom_marker_many<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, CustomLang>,
) -> Result<Vec<i64>, TypedError> {
  number
    .repeated_while::<_, U1>(decide_number)
    .delimited::<CustomMarker>()
    .collect()
    .parse_input(inp)
}

fn bare_bracket<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, CustomLang>,
) -> Result<i64, TypedError> {
  delimited::<Bracket, _, _, CustomLang, _, _, _>(number)(inp).map(|group| *group.data())
}

fn bare_brace<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, CustomLang>,
) -> Result<i64, TypedError> {
  delimited::<Brace, _, _, CustomLang, _, _, _>(number)(inp).map(|group| *group.data())
}

#[test]
fn bare_builtin_markers_preserve_typed_unclosed_conversions_under_custom_language() {
  let bracket =
    Parser::with_parser::<'_, TestLexer<'_>, i64, TypedError, _, CustomLang>(bare_bracket)
      .parse_str("[1");
  match bracket {
    Err(TypedError::Bracket(err)) => assert_eq!(err.name_ref(), "[]"),
    Err(other) => panic!("expected Unclosed<Bracket, _, CustomLang>, got {other:?}"),
    Ok(value) => panic!("expected unclosed bracket, parsed {value}"),
  }

  let brace = Parser::with_parser::<'_, TestLexer<'_>, i64, TypedError, _, CustomLang>(bare_brace)
    .parse_str("{1");
  match brace {
    Err(TypedError::Brace(err)) => assert_eq!(err.name_ref(), "{}"),
    Err(other) => panic!("expected Unclosed<Brace, _, CustomLang>, got {other:?}"),
    Ok(value) => panic!("expected unclosed brace, parsed {value}"),
  }
}

#[test]
fn bare_bracket_many_builder_preserves_typed_unclosed_under_custom_language() {
  let result = Parser::with_parser::<'_, TestLexer<'_>, Vec<i64>, TypedError, _, CustomLang>(
    bare_bracket_many,
  )
  .parse_str("[1 2");

  match result {
    Err(TypedError::Bracket(err)) => assert_eq!(err.name_ref(), "[]"),
    Err(other) => panic!("expected Unclosed<Bracket, _, CustomLang>, got {other:?}"),
    Ok(value) => panic!("expected unclosed bracket, parsed {value:?}"),
  }
}

#[test]
fn non_clone_custom_marker_preserves_typed_unclosed_in_delimited_many() {
  let result = Parser::with_parser::<'_, TestLexer<'_>, Vec<i64>, TypedError, _, CustomLang>(
    custom_marker_many,
  )
  .parse_str("[1 2");

  match result {
    Err(TypedError::CustomMarker(err)) => assert_eq!(err.name_ref(), "custom brackets"),
    Err(other) => panic!("expected Unclosed<CustomMarker, _, CustomLang>, got {other:?}"),
    Ok(value) => panic!("expected unclosed custom marker, parsed {value:?}"),
  }
}

#[test]
fn map_parser_mut_preserves_non_clone_custom_marker_type() {
  let mut delimited = DelimitedBy::<_, CustomMarker>::new(());
  let _: DelimitedBy<&mut (), CustomMarker> = delimited.map_parser_mut(|parser| parser);
}

/// The three per-pair `From<Unclosed<D, …>>` impls above coexist with the umbrella
/// `FromUnclosed` impl, and each is still directly selectable. `FromUnclosed`'s docs promise
/// exactly this ("a type that also wants type-level discrimination for a specific pair can
/// still write `impl From<Unclosed<Paren, …>>` alongside this impl; the two do not overlap"),
/// and the drivers now route through the umbrella — so without this the promise would be
/// unexercised and the three impls would read as dead code.
#[test]
fn per_pair_from_impls_coexist_with_the_umbrella() {
  let span = SimpleSpan::new(3, 4);

  // Type-level: the pair is chosen by the `Unclosed`'s type parameter. The two built-ins go
  // through their own constructors — `of` cannot take a directly spelled built-in variant
  // outside tokora, and these produce exactly the kind/name pair it used to be handed by
  // hand.
  let typed: TypedError = Unclosed::<Bracket, _, CustomLang>::bracket_of(span).into();
  assert!(matches!(typed, TypedError::Bracket(_)));
  let typed: TypedError = Unclosed::<Brace, _, CustomLang>::brace_of(span).into();
  assert!(matches!(typed, TypedError::Brace(_)));
  let typed: TypedError = Unclosed::<CustomMarker, _, CustomLang>::of(
    span,
    DelimiterKind::Custom("custom brackets"),
    "custom brackets".into(),
  )
  .into();
  assert!(matches!(typed, TypedError::CustomMarker(_)));

  // Umbrella: the same three pairs, chosen by the runtime `DelimiterKind`, plus the mandatory
  // catch-all for a pair the arm set does not name.
  let via = <TypedError as FromUnclosed<'_, TestLexer<'_>, CustomLang>>::from_unclosed(Unclosed::<
    Bracket,
    _,
    CustomLang,
  >::bracket_of(
    span
  ));
  assert!(matches!(via, TypedError::Bracket(_)));
  let via = <TypedError as FromUnclosed<'_, TestLexer<'_>, CustomLang>>::from_unclosed(Unclosed::<
    CustomMarker,
    _,
    CustomLang,
  >::of(
    span,
    DelimiterKind::Custom("custom brackets"),
    "custom brackets".into(),
  ));
  assert!(matches!(via, TypedError::CustomMarker(_)));
  let via = <TypedError as FromUnclosed<'_, TestLexer<'_>, CustomLang>>::from_unclosed(Unclosed::<
    Bracket,
    _,
    CustomLang,
  >::of(
    span,
    DelimiterKind::Custom("unnamed pair"),
    "unnamed pair".into(),
  ));
  assert!(matches!(via, TypedError::Other));
}
