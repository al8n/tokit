#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! A dropping emitter pays nothing for the capability it drops.
//!
//! `UnclosedEmitter::emit_unclosed` used to carry `Self::Error: FromUnclosed` on the method, so
//! the conversion was charged to every *caller* of the capability — including the ones whose
//! implementation is `Ok(())`. The bound now sits on the two implementations that convert
//! (`Fatal`, `Verbose`), which is where something collects on it.
//!
//! The witness error type here, [`Opaque`], deliberately implements the *one* conversion the
//! delimited driver genuinely performs — `From<UnexpectedEot<..>>`, which the driver builds
//! itself — and no delimiter conversion at all. There is no `FromUnclosed` impl for `Opaque`
//! anywhere in this file, which is the whole point: everything below compiles without one.
//!
//! `Ignored` is not used as the no-op oracle. Its error type is `()`, and `()` has a blanket
//! `FromUnclosed` impl, so an `Ignored`-only test would pass under the old arrangement too.

mod common;

use common::{TestLexer, Token};

use tokora::{
  Accumulator, ComposableEmitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser,
  ParserContext, SimpleSpan, TryParseInput,
  delimiter::DelimiterKind,
  emitter::{Emitter, Fatal, FromUnclosed, FullContainerEmitter, UnclosedEmitter, Verbose},
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntaxOf, TooFew},
    token::{UnexpectedToken, UnexpectedTokenOf},
  },
  try_parse_input::ParseAttempt,
  utils::CowStr,
};

// ── The witness error: no delimiter conversion, anywhere ─────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Opaque;

impl From<()> for Opaque {
  fn from(_: ()) -> Self {
    Opaque
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for Opaque {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Opaque
  }
}

// The driver *constructs* this one, so it is a demand something collects on and it stays.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Opaque {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    Opaque
  }
}

type SilentOpaque = tokora::emitter::Silent<Opaque>;

fn silent_ctx() -> ParserContext<'static, TestLexer<'static>, SilentOpaque> {
  ParserContext::new(SilentOpaque::default())
}

fn unclosed_paren() -> Unclosed<char, SimpleSpan> {
  Unclosed::new(
    SimpleSpan::new(0usize, 1usize),
    DelimiterKind::Custom("paren"),
    CowStr::from_static("("),
  )
}

// ── 1. The capability is callable from the capability bound ──────────────────

#[test]
fn silent_over_an_opaque_error_calls_its_own_no_op() {
  let mut emitter = SilentOpaque::default();
  let out = <SilentOpaque as UnclosedEmitter<'_, TestLexer<'_>>>::emit_unclosed(
    &mut emitter,
    unclosed_paren(),
  );
  assert_eq!(out, Ok(()));
}

/// Generic over the capability rather than over the concrete emitter: the bound alone has to
/// be enough to call the method, which is the property `ComposableEmitter` advertises.
fn drive_unclosed<'inp, E>(emitter: &mut E) -> Result<(), E::Error>
where
  E: UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  emitter.emit_unclosed::<char>(unclosed_paren())
}

#[test]
fn the_capability_bound_alone_can_call_the_capability() {
  let mut emitter = SilentOpaque::default();
  assert_eq!(drive_unclosed::<SilentOpaque>(&mut emitter), Ok(()));
}

// ── 2. The bundle unlocks every member, with no extra conversion ─────────────

/// One bound, and every member method of the bundle is called through it. Before the repair
/// this function could not be written: `emit_unclosed` needed `E::Error: FromUnclosed` on top
/// of the bundle bound, which is precisely what "the bundle unlocks every member capability"
/// denies.
fn drive_every_bundle_member<'inp, E>(
  emitter: &mut E,
  token_err: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  missing: MissingSyntaxOf<'inp, TestLexer<'inp>, ()>,
) -> Result<(), E::Error>
where
  E: ComposableEmitter<'inp, TestLexer<'inp>>,
{
  let span = SimpleSpan::new(0usize, 1usize);
  emitter.emit_unexpected_token(token_err.clone())?;
  emitter.emit_full_container(FullContainer::new(span, 4, 4))?;
  emitter.emit_too_few(TooFew::new(span, 2, 1))?;
  emitter.emit_missing_element(missing)?;
  emitter.emit_unexpected_leading_separator(CowStr::from_static(","), token_err.clone())?;
  emitter.emit_unexpected_trailing_separator(CowStr::from_static(","), token_err)?;
  emitter.emit_unclosed::<char>(unclosed_paren())
}

// ── 3. A delimited driver instantiated with the dropping emitter ─────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, Opaque>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Opaque>,
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

fn bracketed_numbers<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, Opaque>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Opaque>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .repeated()
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)
}

#[test]
fn a_delimited_driver_runs_under_the_dropping_emitter() {
  let parsed: Result<Vec<i64>, Opaque> = Parser::with_context(silent_ctx())
    .apply(bracketed_numbers)
    .parse_str("[1 2 3]");
  assert_eq!(parsed, Ok(vec![1, 2, 3]));
}

/// The unterminated list is the arm that actually reaches `emit_unclosed`. The dropping
/// emitter discards the diagnostic, so the elements collected so far come back as `Ok`.
#[test]
fn the_dropping_emitter_absorbs_an_unclosed_list() {
  let parsed: Result<Vec<i64>, Opaque> = Parser::with_context(silent_ctx())
    .apply(bracketed_numbers)
    .parse_str("[1 2 3");
  assert_eq!(parsed, Ok(vec![1, 2, 3]));
}

#[test]
fn the_bundle_driver_runs_under_the_dropping_emitter() {
  let mut emitter = SilentOpaque::default();
  let span = SimpleSpan::new(0usize, 1usize);
  let token_err = UnexpectedToken::expected_one(span, common::TokenKind::Num);
  let missing = MissingSyntaxOf::<TestLexer<'_>, ()>::new(0usize);
  assert_eq!(
    drive_every_bundle_member::<SilentOpaque>(&mut emitter, token_err, missing),
    Ok(())
  );
}

// ── 4. The converting emitters still convert ─────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Converting;

impl From<()> for Converting {
  fn from(_: ()) -> Self {
    Converting
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for Converting
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Converting
  }
}

impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for Converting
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    Converting
  }
}

#[test]
fn fatal_over_a_converting_error_still_fails_fast() {
  let mut emitter = Fatal::<Converting>::default();
  let out = <Fatal<Converting> as UnclosedEmitter<'_, TestLexer<'_>>>::emit_unclosed(
    &mut emitter,
    unclosed_paren(),
  );
  assert_eq!(out, Err(Converting));
}

#[test]
fn verbose_over_a_converting_error_still_records() {
  let mut emitter = Verbose::<Converting, SimpleSpan>::new();
  let out = <Verbose<Converting, SimpleSpan> as UnclosedEmitter<'_, TestLexer<'_>>>::emit_unclosed(
    &mut emitter,
    unclosed_paren(),
  );
  assert_eq!(out, Ok(()));
  assert_eq!(emitter.errors().len(), 1);
}
