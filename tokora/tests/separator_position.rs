#![cfg(all(feature = "std", feature = "logos"))]

//! Regression tests for separator position carried as **data** (a
//! [`SeparatorPosition`] field on [`SeparatedError`]) rather than encoded in the
//! `Lang` type slot of `UnexpectedToken`.
//!
//! The headline test [`downstream_distinguishes_by_position`] builds a
//! downstream-style error enum from **only** core `From` impls — no
//! hand-written `FromSeparatedError` / `FromUnexpected{Leading,Trailing}SeparatorError`
//! impls — and receives leading / trailing / element separator errors
//! distinguished purely by the position field. This is the shape that was
//! impossible before the blanket `From` impls were restored.

mod common;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  cache::Peeked,
  emitter::{
    Fatal, FromMissingLeadingSeparatorError, FromMissingTrailingSeparatorError, FromSeparatedError,
    FromUnexpectedLeadingSeparatorError, FromUnexpectedTrailingSeparatorError,
    FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, Verbose,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, SeparatorPosition, UnexpectedToken},
  },
  parser::Action,
  span::SimpleSpan,
  utils::{CowStr, Expected},
};

use common::{TestLexer, Token, TokenKind};

// ── Downstream-style error enum — CORE `From` impls only ──────────────────────
//
// Note the absence of any `impl FromSeparatedError`, `impl
// FromUnexpectedLeadingSeparatorError`, or `impl
// FromUnexpectedTrailingSeparatorError`. Those are supplied by the restored
// blanket impls; the leading/trailing position rides in on `SeparatedError` and
// the element position on `MissingSyntax`.

#[derive(Debug, Clone, PartialEq, Eq)]
enum SepErr {
  /// A separator error, tagged with where it occurred.
  Sep(SeparatorPosition),
  /// Anything else (lexer error, plain unexpected token, missing separator...).
  Other,
}

impl From<()> for SepErr {
  fn from(_: ()) -> Self {
    SepErr::Other
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for SepErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    SepErr::Other
  }
}

// Leading / trailing separator errors arrive here — distinguished by position.
impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for SepErr {
  fn from(err: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    SepErr::Sep(err.position())
  }
}

// A missing element mid-list is the element position.
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for SepErr {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    SepErr::Sep(SeparatorPosition::Element)
  }
}

// A terminal scanner stop at the separator slot surfaces as this end-of-input error — not a
// separator-position diagnostic.
impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for SepErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    SepErr::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for SepErr {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    SepErr::Other
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for SepErr {
  fn from(_: FullContainer<S, Lang>) -> Self {
    SepErr::Other
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for SepErr {
  fn from(_: TooFew<S, Lang>) -> Self {
    SepErr::Other
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for SepErr {
  fn from(_: TooMany<S, Lang>) -> Self {
    SepErr::Other
  }
}

// ── Parser harness ────────────────────────────────────────────────────────────

fn decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: &mut Ctx::Emitter,
) -> Result<Action, <Ctx::Emitter as Emitter<'inp, TestLexer<'inp>>>::Error>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
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

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>,
{
  match inp.next()? {
    None => Err(SepErr::Other),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(SepErr::Other),
    },
  }
}

fn parse_allow_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .collect()
    .parse_input(inp)
}

fn parse_allow_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

fn parse_plain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SepErr>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SepErr>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .collect()
    .parse_input(inp)
}

// ── The previously-impossible test ────────────────────────────────────────────

#[test]
fn downstream_distinguishes_by_position() {
  // Leading separator under allow_trailing → position = Leading.
  let leading: Result<Vec<i64>, SepErr> =
    Parser::new().apply(parse_allow_trailing).parse_str(",1+");
  assert_eq!(leading, Err(SepErr::Sep(SeparatorPosition::Leading)));

  // Trailing separator under allow_leading → position = Trailing.
  let trailing: Result<Vec<i64>, SepErr> =
    Parser::new().apply(parse_allow_leading).parse_str("1,+");
  assert_eq!(trailing, Err(SepErr::Sep(SeparatorPosition::Trailing)));

  // Consecutive separators mid-list → missing element → position = Element.
  let element: Result<Vec<i64>, SepErr> = Parser::new().apply(parse_plain).parse_str("1,,2+");
  assert_eq!(element, Err(SepErr::Sep(SeparatorPosition::Element)));
}

// ── Unit coverage of the new data types ───────────────────────────────────────

#[test]
fn separator_position_as_str_and_predicates() {
  assert_eq!(SeparatorPosition::Element.as_str(), "element");
  assert_eq!(SeparatorPosition::Leading.as_str(), "leading");
  assert_eq!(SeparatorPosition::Trailing.as_str(), "trailing");

  assert!(SeparatorPosition::Element.is_element());
  assert!(SeparatorPosition::Leading.is_leading());
  assert!(SeparatorPosition::Trailing.is_trailing());
  assert!(!SeparatorPosition::Element.is_leading());

  assert_eq!(SeparatorPosition::Trailing.to_string(), "trailing");
}

#[test]
fn separated_error_constructors_and_accessors() {
  let ut: UnexpectedToken<'_, &str, &str, tokora::SimpleSpan> =
    UnexpectedToken::expected_one_with_found(tokora::SimpleSpan::new(1, 2), ",", ";");

  let leading = SeparatedError::leading(ut.clone());
  assert_eq!(leading.position(), SeparatorPosition::Leading);
  assert_eq!(leading.inner_ref().found(), Some(&","));

  let trailing = SeparatedError::trailing(ut.clone());
  assert_eq!(trailing.position(), SeparatorPosition::Trailing);

  let element = SeparatedError::element(ut.clone());
  assert_eq!(element.position(), SeparatorPosition::Element);

  let explicit = SeparatedError::new(SeparatorPosition::Leading, ut.clone());
  // SANCTIONED TEST UPDATE: `into_components` now also yields the stamped separator name, so
  // the destructuring seam of the type that carries the separator's identity cannot silently
  // drop it. This fixture constructs the error directly and never stamps a name, so `None` is
  // the correct expectation here.
  let (pos, name, inner) = explicit.into_components();
  assert_eq!(pos, SeparatorPosition::Leading);
  assert_eq!(name, None);
  assert_eq!(inner.found(), Some(&","));
}

/// `into_inner` is the deliberately narrow sibling of `into_components`: it hands back the
/// token alone and so drops the position and the name. Pinning both keeps the pair legible —
/// one seam lossless by contract, the other lossy by contract — so a later reader does not
/// mistake the lossy one for the defect the name channel was added to close.
#[test]
fn separated_error_into_inner_is_the_lossy_sibling() {
  let ut: UnexpectedToken<'_, &str, &str, tokora::SimpleSpan> =
    UnexpectedToken::expected_one_with_found(tokora::SimpleSpan::new(1, 2), ",", ";");

  let stamped = SeparatedError::leading(ut).with_name(sep_name());
  assert_eq!(stamped.name().map(|n| n.as_str()), Some("comma"));
  assert_eq!(stamped.position(), SeparatorPosition::Leading);

  let inner = stamped.into_inner();
  assert_eq!(inner.found(), Some(&","));
}

// ═══════════════════════════════════════════════════════════════════════════════
// The separator name reaches the payload
// ═══════════════════════════════════════════════════════════════════════════════
//
// Every separator emitter is handed the separator's name, and every conversion from that
// emission into a downstream error type goes through a blanket impl. A downstream type cannot
// override those blankets — it implements `From<MissingTokenOf>` / `From<SeparatedErrorOf>` to
// compose with the rest of the family, which is exactly the bound the blankets capture, so its
// own impl is a coherence error. So if the blanket drops the name, the name is unreachable for
// every user of the family, and the diagnostic can only say *a* separator is missing.
//
// The repair is therefore payload enrichment inside the blanket, and these are its pins: two
// payload-preserving local error types that keep what the shipped fixtures throw away.

/// Keeps a `MissingToken`'s optional channels, which the other fixtures in this file discard.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Keep {
  Missing {
    name: Option<String>,
    message: Option<String>,
    has_expected: bool,
  },
  Other,
}

impl From<()> for Keep {
  fn from(_: ()) -> Self {
    Keep::Other
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for Keep {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Keep::Other
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Keep {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Keep::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for Keep {
  fn from(err: MissingToken<'a, Kind, O, Lang>) -> Self {
    Keep::Missing {
      name: err.name().map(|n| n.as_str().to_string()),
      message: err.message().map(|m| m.as_str().to_string()),
      has_expected: err.expected().is_some(),
    }
  }
}

/// Keeps a `SeparatedError`'s position and name, and whether the wrapped token survived.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeepSep {
  position: SeparatorPosition,
  name: Option<String>,
  has_found: bool,
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for KeepSep {
  fn from(err: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    KeepSep {
      position: err.position(),
      name: err.name().map(|n| n.as_str().to_string()),
      has_found: err.inner_ref().found().is_some(),
    }
  }
}

fn sep_name() -> CowStr {
  CowStr::from_static("comma")
}

/// The three `MissingToken`-carrying conversions stamp the separator name into the payload's
/// own name channel, leaving `expected` and `message` untouched.
#[test]
fn e2_flipped_missing_separator_conversions_carry_the_name() {
  let converted: Vec<Keep> = vec![
    <Keep as FromSeparatedError<'_, TestLexer<'_>>>::from_missing_separator(
      sep_name(),
      MissingToken::of(7usize),
    ),
    <Keep as FromMissingLeadingSeparatorError<'_, TestLexer<'_>>>::from_missing_leading_separator(
      sep_name(),
      MissingToken::of(7usize),
    ),
    <Keep as FromMissingTrailingSeparatorError<'_, TestLexer<'_>>>::from_missing_trailing_separator(
      sep_name(),
      MissingToken::of(7usize),
    ),
  ];

  for (i, got) in converted.iter().enumerate() {
    assert_eq!(
      got,
      &Keep::Missing {
        name: Some("comma".to_string()),
        message: None,
        has_expected: false,
      },
      "conversion {i} must carry the separator name into the payload"
    );
  }
}

/// The two `UnexpectedToken`-carrying conversions stamp the name onto the `SeparatedError`
/// wrapper, alongside the position it already carried, and leave the wrapped token alone.
#[test]
fn e2_flipped_unexpected_separator_conversions_carry_the_name() {
  let tok: UnexpectedToken<'_, Token, TokenKind, SimpleSpan> =
    UnexpectedToken::new(SimpleSpan::new(3usize, 4usize)).with_found(Token::Comma);

  let leading =
    <KeepSep as FromUnexpectedLeadingSeparatorError<'_, TestLexer<'_>>>::from_unexpected_leading_separator(
      sep_name(),
      tok.clone(),
    );
  assert_eq!(
    leading,
    KeepSep {
      position: SeparatorPosition::Leading,
      name: Some("comma".to_string()),
      has_found: true,
    }
  );

  let trailing =
    <KeepSep as FromUnexpectedTrailingSeparatorError<'_, TestLexer<'_>>>::from_unexpected_trailing_separator(
      sep_name(),
      tok,
    );
  assert_eq!(
    trailing,
    KeepSep {
      position: SeparatorPosition::Trailing,
      name: Some("comma".to_string()),
      has_found: true,
    }
  );
}

/// The name has a channel of its own, so the stamp neither overwrites a caller's message nor
/// has to decline to stamp when one is present.
#[test]
fn stamp_leaves_the_message_channel_alone() {
  let pre_set = MissingToken::of(7usize).with_message(CowStr::from_static("custom"));
  let got =
    <Keep as FromSeparatedError<'_, TestLexer<'_>>>::from_missing_separator(sep_name(), pre_set);
  assert_eq!(
    got,
    Keep::Missing {
      name: Some("comma".to_string()),
      message: Some("custom".to_string()),
      has_expected: false,
    },
    "the caller's message survives and the name arrives beside it"
  );
}

/// `with_expected` clears the message channel — which is why the name does not live there.
#[test]
fn with_expected_preserves_the_stamped_name() {
  let stamped = MissingToken::<'_, TokenKind, usize>::of(7usize)
    .with_name(sep_name())
    .with_message(CowStr::from_static("custom"))
    .with_expected(Expected::one(TokenKind::Comma));

  assert_eq!(
    stamped.name().map(|n| n.as_str()),
    Some("comma"),
    "enriching a stamped error with an expectation must not discard the separator name"
  );
  assert!(
    stamped.message().is_none(),
    "with_expected clears the message channel, as it always has"
  );
}

/// The stamp lives in the shared conversion, so `Fatal`'s propagated error carries exactly what
/// `Verbose` collects.
#[test]
fn fatal_and_verbose_share_the_stamped_name() {
  let expected = Keep::Missing {
    name: Some("comma".to_string()),
    message: None,
    has_expected: false,
  };

  let mut fatal = Fatal::<Keep>::new();
  let propagated = <Fatal<Keep> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
    &mut fatal,
    sep_name(),
    MissingToken::of(7usize),
  );
  assert_eq!(
    propagated.unwrap_err(),
    expected,
    "Fatal propagates the stamped payload"
  );

  let mut verbose = Verbose::<Keep>::new();
  <Verbose<Keep> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
    &mut verbose,
    sep_name(),
    MissingToken::of(7usize),
  )
  .expect("Verbose collects rather than propagates");
  let collected: Vec<&Keep> = verbose.errors().values().flatten().collect();
  assert_eq!(
    collected,
    vec![&expected],
    "Verbose collects the same stamped payload"
  );
}
