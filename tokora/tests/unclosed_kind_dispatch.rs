#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! An unclosed-delimiter conversion discriminates on [`DelimiterKind`], not on the display
//! name.
//!
//! `Delimiter::name` is documented as, and rendered as, a human-readable string; it carries no
//! uniqueness contract. Routing `FromUnclosed` on it made the correct display name and a
//! correct dispatch key mutually exclusive: `"[]"` is the right name for *any* bracket-shaped
//! pair, so a consumer who defined one and named it correctly had it silently absorbed by
//! tokora's built-in `Bracket` arm.
//!
//! These tests pin the fix and are honest about its edge. Two of them are genuine flips —
//! they fail if dispatch goes back to the name — two are keep-green pins that held before the
//! change as well and are here so a later refactor cannot quietly drop them, and two pin the
//! *accepted residue*: the routes by which a built-in kind still reaches a crate that is not
//! tokora. Those two assert the forgery **succeeds**, on purpose, so that closing a route
//! breaks a line here and the trade-off is re-decided rather than silently changed.
//!
//! This file is an integration test, so it is compiled as its own crate against tokora — the
//! privacy it exercises is a downstream crate's, not tokora's own. That is what makes the
//! `DelimiterKind::Paren { .. }` braces below load-bearing, and what makes the residue pins
//! evidence about the published API rather than about tokora's interior.

mod common;

use common::{TestLexer, Token};
use tokora::{
  FatalContext, InputRef, Lexer, Parse, Parser, SimpleSpan,
  delimiter::{Delimiter, DelimiterKind, TypedDelimiter},
  emitter::FromUnclosed,
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::{angles, braces, brackets, delimited, parens},
  punct::{Angle, Brace, Bracket, CloseAngle, CloseBrace, OpenAngle, OpenBrace, Paren},
  utils::CowStr,
};

#[derive(Debug)]
enum PLang {}

/// A legal custom pair over `<` `>` whose *display name* is the one `Bracket` also uses.
///
/// Nothing about this is a mistake: `"[]"` is what a bracket-shaped pair should print. Before
/// the kind existed, this alone routed it into the consumer's built-in `Bracket` arm.
#[derive(Debug)]
struct CollidingMarker;

impl<'inp> Delimiter<'inp, TestLexer<'inp>, PLang> for CollidingMarker {
  const KIND: DelimiterKind = DelimiterKind::Custom("colliding");

  type Open = OpenAngle<(), (), PLang>;
  type Close = CloseAngle<(), (), PLang>;

  fn name() -> CowStr {
    CowStr::from_static("[]")
  }
}

impl<'inp> TypedDelimiter<'inp, TestLexer<'inp>, PLang> for CollidingMarker {
  type OpenValue = OpenAngle<SimpleSpan, (), PLang>;
  type CloseValue = CloseAngle<SimpleSpan, (), PLang>;

  fn open_value(span: SimpleSpan) -> Self::OpenValue {
    OpenAngle::new(span).change_language()
  }

  fn close_value(span: SimpleSpan) -> Self::CloseValue {
    CloseAngle::new(span).change_language()
  }
}

/// The built-in bracket kind, obtained **from outside tokora** without naming the variant.
///
/// `DelimiterKind::Bracket` written here is `E0603`. This is not that: it projects the value
/// off the public associated const of tokora's own pair, which is an ordinary
/// `Copy` `DelimiterKind`. The line compiling at all is the residue
/// [`DelimiterKind`](tokora::delimiter::DelimiterKind) documents under *Not guaranteed*.
const PROJECTED_BRACKET: DelimiterKind =
  <Bracket<(), (), PLang> as Delimiter<'static, TestLexer<'static>, PLang>>::KIND;

/// A pair over `<` `>` that claims tokora's bracket identity by projection.
///
/// Its `name()` is honest — `"<>"` — so nothing but the kind can route it. That is the point:
/// the display-name fix does not reach this, and neither does the `#[non_exhaustive]` fence.
#[derive(Debug)]
struct ProjectingMarker;

impl<'inp> Delimiter<'inp, TestLexer<'inp>, PLang> for ProjectingMarker {
  const KIND: DelimiterKind = PROJECTED_BRACKET;

  type Open = OpenAngle<(), (), PLang>;
  type Close = CloseAngle<(), (), PLang>;

  fn name() -> CowStr {
    CowStr::from_static("<>")
  }
}

impl<'inp> TypedDelimiter<'inp, TestLexer<'inp>, PLang> for ProjectingMarker {
  type OpenValue = OpenAngle<SimpleSpan, (), PLang>;
  type CloseValue = CloseAngle<SimpleSpan, (), PLang>;

  fn open_value(span: SimpleSpan) -> Self::OpenValue {
    OpenAngle::new(span).change_language()
  }

  fn close_value(span: SimpleSpan) -> Self::CloseValue {
    CloseAngle::new(span).change_language()
  }
}

/// A legal custom pair with a unique name that the consumer's arm set simply forgot.
#[derive(Debug)]
struct ForgottenMarker;

impl<'inp> Delimiter<'inp, TestLexer<'inp>, PLang> for ForgottenMarker {
  const KIND: DelimiterKind = DelimiterKind::Custom("forgotten");

  type Open = OpenBrace<(), (), PLang>;
  type Close = CloseBrace<(), (), PLang>;

  fn name() -> CowStr {
    CowStr::from_static("<<>>")
  }
}

impl<'inp> TypedDelimiter<'inp, TestLexer<'inp>, PLang> for ForgottenMarker {
  type OpenValue = OpenBrace<SimpleSpan, (), PLang>;
  type CloseValue = CloseBrace<SimpleSpan, (), PLang>;

  fn open_value(span: SimpleSpan) -> Self::OpenValue {
    OpenBrace::new(span).change_language()
  }

  fn close_value(span: SimpleSpan) -> Self::CloseValue {
    CloseBrace::new(span).change_language()
  }
}

#[derive(Debug, PartialEq)]
enum PErr {
  UnclosedParen(SimpleSpan),
  UnclosedAngle(SimpleSpan),
  UnclosedBracket(SimpleSpan),
  UnclosedBrace(SimpleSpan),
  UnclosedColliding(SimpleSpan),
  /// Deliberately unreachable: no arm produces it. It exists so the forgotten-pair test can
  /// assert what the pair did *not* become.
  UnclosedForgotten(SimpleSpan),
  Other,
}

impl<'inp, L> FromUnclosed<'inp, L, PLang> for PErr
where
  L: Lexer<'inp, Span = SimpleSpan>,
{
  fn from_unclosed<D>(err: Unclosed<D, SimpleSpan, PLang>) -> Self {
    let kind = err.kind();
    let (span, _name) = err.into_components();
    match kind {
      // The braces are the fence showing through: the built-in variants are
      // `#[non_exhaustive]`, so this crate can match one but cannot write one.
      DelimiterKind::Paren { .. } => Self::UnclosedParen(span),
      DelimiterKind::Angle { .. } => Self::UnclosedAngle(span),
      DelimiterKind::Bracket { .. } => Self::UnclosedBracket(span),
      DelimiterKind::Brace { .. } => Self::UnclosedBrace(span),
      DelimiterKind::Custom("colliding") => Self::UnclosedColliding(span),
      // Deliberately absent: no arm for `Custom("forgotten")`.
      _ => Self::Other,
    }
  }
}

impl From<()> for PErr {
  fn from(_: ()) -> Self {
    Self::Other
  }
}
impl<'inp, T, K: Clone, S> From<UnexpectedToken<'inp, T, K, S, PLang>> for PErr {
  fn from(_: UnexpectedToken<'inp, T, K, S, PLang>) -> Self {
    Self::Other
  }
}
impl<O, Set: Clone + 'static> From<UnexpectedEot<O, PLang, Set>> for PErr {
  fn from(_: UnexpectedEot<O, PLang, Set>) -> Self {
    Self::Other
  }
}
impl<S> From<FullContainer<S, PLang>> for PErr {
  fn from(_: FullContainer<S, PLang>) -> Self {
    Self::Other
  }
}
impl<S> From<TooFew<S, PLang>> for PErr {
  fn from(_: TooFew<S, PLang>) -> Self {
    Self::Other
  }
}
impl<'inp, K: Clone, O> From<MissingToken<'inp, K, O, PLang>> for PErr {
  fn from(_: MissingToken<'inp, K, O, PLang>) -> Self {
    Self::Other
  }
}
impl<'inp, T, K: Clone, S> From<SeparatedError<'inp, T, K, S, PLang>> for PErr {
  fn from(_: SeparatedError<'inp, T, K, S, PLang>) -> Self {
    Self::Other
  }
}
impl<O> From<MissingSyntax<O, PLang>> for PErr {
  fn from(_: MissingSyntax<O, PLang>) -> Self {
    Self::Other
  }
}

type Ctx<'inp> = FatalContext<'inp, TestLexer<'inp>, PErr, PLang>;

fn number<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, PLang>,
) -> Result<i64, PErr> {
  match inp.next()? {
    Some(token) => match token.into_data() {
      Token::Num(number) => Ok(number),
      _ => Err(PErr::Other),
    },
    None => Err(PErr::Other),
  }
}

macro_rules! pair_parser {
  ($fn_name:ident, $marker:ty) => {
    fn $fn_name<'inp>(
      inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, PLang>,
    ) -> Result<i64, PErr> {
      delimited::<$marker, _, _, PLang, _, _, _>(number)(inp).map(|group| *group.data())
    }
  };
}

pair_parser!(paren_pair, Paren<(), (), PLang>);
pair_parser!(angle_pair, Angle<(), (), PLang>);
pair_parser!(bracket_pair, Bracket<(), (), PLang>);
pair_parser!(brace_pair, Brace<(), (), PLang>);
pair_parser!(colliding_pair, CollidingMarker);
pair_parser!(forgotten_pair, ForgottenMarker);
pair_parser!(projecting_pair, ProjectingMarker);

/// The named shape parsers reach the same close-miss law by a different route: they do not
/// go through `Delimiter::KIND` at all, each naming an in-crate constant of its own.
macro_rules! shape_parser {
  ($fn_name:ident, $shape:ident) => {
    fn $fn_name<'inp>(
      inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>, PLang>,
    ) -> Result<i64, PErr> {
      $shape::<_, _, PLang, _, _, _>(number)(inp).map(|group| *group.data())
    }
  };
}

shape_parser!(paren_shape, parens);
shape_parser!(angle_shape, angles);
shape_parser!(bracket_shape, brackets);
shape_parser!(brace_shape, braces);

/// A `fn` item cannot be coerced to a pointer general enough for these higher-ranked
/// lifetimes, so the driver is a macro rather than a helper function.
macro_rules! run {
  ($parser:ident, $src:literal) => {
    Parser::with_parser::<'_, TestLexer<'_>, i64, PErr, _, PLang>($parser).parse_str($src)
  };
}

/// **Keep-green.** The four built-in pairs each report their own kind, end to end through a
/// real parse. This held before the change too — it is here so a regression in the kind
/// plumbing (a builder passing the wrong `Delim::KIND`) cannot pass unnoticed, since every
/// `Delim::KIND` site is exercised exactly once.
#[test]
fn each_builtin_pair_reports_its_own_kind() {
  let at0 = SimpleSpan::new(0, 1);
  assert_eq!(run!(paren_pair, "(1"), Err(PErr::UnclosedParen(at0)));
  assert_eq!(run!(angle_pair, "<1"), Err(PErr::UnclosedAngle(at0)));
  assert_eq!(run!(bracket_pair, "[1"), Err(PErr::UnclosedBracket(at0)));
  assert_eq!(run!(brace_pair, "{1"), Err(PErr::UnclosedBrace(at0)));
}

/// **Keep-green, the other four production sites.** `parens`/`braces`/`brackets`/`angles`
/// never read `Delimiter::KIND`; each names one of the four in-crate constants that pair a
/// built-in kind with that pair's display name. The test above therefore does not reach them,
/// and `shape_unclosed` reads only the name — so a kind edited inside one of those constants
/// would move nothing any assertion looks at. This is the assertion that looks at it.
#[test]
fn each_builtin_shape_parser_reports_its_own_kind() {
  let at0 = SimpleSpan::new(0, 1);
  assert_eq!(run!(paren_shape, "(1"), Err(PErr::UnclosedParen(at0)));
  assert_eq!(run!(angle_shape, "<1"), Err(PErr::UnclosedAngle(at0)));
  assert_eq!(run!(bracket_shape, "[1"), Err(PErr::UnclosedBracket(at0)));
  assert_eq!(run!(brace_shape, "{1"), Err(PErr::UnclosedBrace(at0)));
}

/// **The flip.** A custom pair whose `name()` is `"[]"` — the correct display name for it —
/// reports as itself and cannot reach the built-in `Bracket` arm.
///
/// The source here contains no bracket at all; it is `<1`. Before the kind existed this
/// returned `UnclosedBracket`, naming a character the input never held.
#[test]
fn a_custom_pair_named_like_a_builtin_reports_itself() {
  let at0 = SimpleSpan::new(0, 1);
  let got = run!(colliding_pair, "<1");
  assert_eq!(got, Err(PErr::UnclosedColliding(at0)));
  assert_ne!(
    got,
    Err(PErr::UnclosedBracket(at0)),
    "a custom pair reached the built-in Bracket arm — dispatch is reading the display name"
  );
}

/// **Additive pin, and the honest edge.** A custom pair the arm set forgot still lands in the
/// catch-all. That is *not* fixed by the kind and cannot be: `from_unclosed` is generic over
/// `D`, so the compile-time obligation that used to catch a missing arm is gone for good.
///
/// What the kind buys here is that the landing is now *unambiguous*. The pair arrives labelled
/// `Custom("forgotten")` — a value the consumer can see, log and add an arm for — instead of
/// being aliased onto whichever built-in its name happened to match. This test held before the
/// change and holds after; it is here to keep the limit stated rather than to demonstrate a
/// fix.
#[test]
fn a_forgotten_custom_pair_lands_in_the_catch_all_unambiguously() {
  let at0 = SimpleSpan::new(0, 1);
  let got = run!(forgotten_pair, "{1");
  assert_eq!(got, Err(PErr::Other));
  assert_ne!(got, Err(PErr::UnclosedForgotten(at0)));
  // And specifically: it did not silently become the built-in pair over the same tokens.
  assert_ne!(
    got,
    Err(PErr::UnclosedBrace(at0)),
    "a forgotten custom pair was absorbed by the built-in arm for its opening token"
  );
}

/// **The flip, at the conversion boundary.** The kind decides even when the display name
/// contradicts it, in both directions.
#[test]
fn the_kind_decides_and_the_display_name_does_not() {
  let span = SimpleSpan::new(3, 4);

  // A real `Bracket` carrying a name that says otherwise still routes to the bracket arm.
  //
  // This crate cannot *write* `DelimiterKind::Bracket` — the variant is `#[non_exhaustive]`,
  // which is the point — so the contradicting pair is assembled from a kind copied off an
  // `Unclosed` tokora itself built. That copy is the residue `DelimiterKind`'s docs name:
  // unwritable is not uncopyable. Should a later change close it too, this line stops
  // compiling and the trade-off is re-decided rather than quietly lost.
  let from_tokoras_own_pair = Unclosed::<Bracket<(), (), PLang>, _, PLang>::bracket_of(span).kind();
  let via = <PErr as FromUnclosed<'_, TestLexer<'_>, PLang>>::from_unclosed(Unclosed::<
    Bracket<(), (), PLang>,
    _,
    PLang,
  >::of(
    span,
    from_tokoras_own_pair,
    "renamed".into(),
  ));
  assert_eq!(
    via,
    PErr::UnclosedBracket(span),
    "a misleading display name overrode a correct kind"
  );

  // A custom pair carrying the built-in's name does not reach the built-in arm.
  let via = <PErr as FromUnclosed<'_, TestLexer<'_>, PLang>>::from_unclosed(Unclosed::<
    CollidingMarker,
    _,
    PLang,
  >::of(
    span,
    DelimiterKind::Custom("colliding"),
    "[]".into(),
  ));
  assert_eq!(
    via,
    PErr::UnclosedColliding(span),
    "a display name matching a built-in reached the built-in arm"
  );
}

/// **Accepted residue, pinned green: the associated-const projection.**
///
/// A pair defined outside tokora, over `<` `>`, with the honest name `"<>"`, reports as
/// tokora's built-in bracket — because its `KIND` was projected off
/// `<tokora::punct::Bracket<…> as Delimiter<…>>::KIND` rather than spelled. The
/// `#[non_exhaustive]` fence stops the *spelling*; it does not make the value unreachable, and
/// this test is the standing record of that.
///
/// **What this establishes:** the projection is live in the published API, from a crate that
/// is not tokora, and it lands in the consumer's built-in arm — so a `Bracket { .. }` arm
/// firing is not proof of provenance.
///
/// **What it does not establish:** that the route is harmless. It is a deliberate act that has
/// to name `tokora::punct::Bracket` in the author's own source; what the fence removed was the
/// accident, and that removal is what the other tests here hold.
///
/// **If it ever fails,** the route was closed. Read `DelimiterKind`'s *Not guaranteed* section
/// and the CHANGELOG before deleting this test: both state the residue as accepted, and both
/// become wrong the moment this line stops compiling.
#[test]
fn a_projected_builtin_kind_reaches_the_builtin_arm() {
  let at0 = SimpleSpan::new(0, 1);
  assert_eq!(
    run!(projecting_pair, "<1"),
    Err(PErr::UnclosedBracket(at0)),
    "the projected built-in kind stopped routing to the built-in arm"
  );
}

/// **Accepted residue, pinned green: the typed constructors.**
///
/// Shorter than the projection above and needing no `Delimiter` impl at all —
/// [`Unclosed::bracket_of`] is a public constructor that mints a fully-formed built-in-kinded
/// error, with tokora's own name, from any crate, over any span. Closing the projection route
/// alone would leave this one open, which is the reason the projection route is not closed.
///
/// **What this establishes:** two independent public doors hand out the identical built-in
/// value to a crate that is not tokora. **What it does not establish:** anything about
/// *accidental* misuse — reaching either door requires naming tokora's own bracket.
#[test]
fn a_builtin_kind_is_mintable_outside_tokora_in_one_call() {
  let span = SimpleSpan::new(7, 8);

  let minted = Unclosed::<Bracket<(), (), PLang>, _, PLang>::bracket_of(span);
  assert_eq!(
    minted.kind(),
    PROJECTED_BRACKET,
    "the constructor and the projection stopped agreeing on the built-in bracket value"
  );

  let via = <PErr as FromUnclosed<'_, TestLexer<'_>, PLang>>::from_unclosed(minted);
  assert_eq!(
    via,
    PErr::UnclosedBracket(span),
    "an externally minted built-in kind stopped routing to the built-in arm"
  );
}
