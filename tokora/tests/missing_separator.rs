#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Tests that the shipped emitters implement `Missing{Leading,Trailing}SeparatorEmitter`,
//! making the `require_*` separator families usable without a hand-rolled emitter.

mod common;

use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  TryParseInput,
  emitter::{
    Fatal, FullContainerEmitter, MissingTrailingSeparatorEmitter, SeparatedEmitter,
    UnexpectedLeadingSeparatorEmitter, Verbose,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf},
    token::{MissingToken, MissingTokenOf, SeparatedError, UnexpectedToken},
  },
  span::SimpleSpan,
  try_parse_input::ParseAttempt,
  utils::CowStr,
};

use common::{TestLexer, Token};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ReqError;

impl From<()> for ReqError {
  fn from(_: ()) -> Self {
    ReqError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for ReqError {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ReqError
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for ReqError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    ReqError
  }
}

impl From<UnexpectedEot> for ReqError {
  fn from(_: UnexpectedEot) -> Self {
    ReqError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for ReqError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    ReqError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for ReqError {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    ReqError
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for ReqError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    ReqError
  }
}

// ── Context constructors ──────────────────────────────────────────────────────

fn fatal_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<ReqError>> {
  ParserContext::new(Fatal::new())
}

fn verbose_ctx() -> ParserContext<'static, TestLexer<'static>, Verbose<ReqError>> {
  ParserContext::new(Verbose::new())
}

// ── Element parser ────────────────────────────────────────────────────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, ReqError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ReqError>,
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

fn parse_require_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, ReqError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ReqError>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .collect()
    .parse_input(inp)
}

// ── Fatal: a missing required trailing separator is fatal ─────────────────────

#[test]
fn require_trailing_fatal_missing_is_err() {
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_require_trailing)
    .parse_str("1,2,3");
  assert!(r.is_err());
}

#[test]
fn require_trailing_fatal_present_is_ok() {
  let r: Vec<i64> = Parser::with_context(fatal_ctx())
    .apply(parse_require_trailing)
    .parse_str("1,2,3,")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── Verbose: a missing required trailing separator is recorded, not fatal ─────

#[test]
fn require_trailing_verbose_records_zero_width_span() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<ReqError>>,
    >,
  ) -> Result<Vec<i64>, ReqError> {
    let out = try_num
      .separated_by_comma()
      .require_trailing()
      .collect()
      .parse_input(inp)?;
    let errs = inp.emitter_ref().errors();
    assert_eq!(errs.len(), 1, "one missing-trailing-separator error");
    // "1,2,3" ends at offset 5; the error is recorded at the zero-width span there.
    assert!(
      errs.contains_key(&SimpleSpan::new(5usize, 5usize)),
      "missing trailing separator recorded at the zero-width span at end of input"
    );
    Ok(out)
  }

  let r: Vec<i64> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── Verbose: require_surrounded records both leading and trailing at zero-width ─

#[test]
fn require_surrounded_verbose_records_both_zero_width_spans() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<ReqError>>,
    >,
  ) -> Result<Vec<i64>, ReqError> {
    let out = try_num
      .separated_by_comma()
      .require_trailing()
      .require_leading()
      .collect()
      .parse_input(inp)?;
    let errs = inp.emitter_ref().errors();
    assert_eq!(errs.len(), 2, "missing leading and trailing separators");
    // Leading missing at the start (offset 0), trailing missing at the end (offset 5).
    assert!(errs.contains_key(&SimpleSpan::new(0usize, 0usize)));
    assert!(errs.contains_key(&SimpleSpan::new(5usize, 5usize)));
    Ok(out)
  }

  let r: Vec<i64> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(r, vec![1, 2, 3]);
}

// ── Verbose: same-offset missing-element + missing-separator both retained ─────

// The zero-width fixtures above sit at distinct offsets (0 and 5), so their span keys
// never collide. This exercises the collision directly: a missing separator and a
// missing element reported at the SAME zero-width offset must BOTH be retained under
// Verbose (span-keyed storage groups them instead of overwriting).
#[test]
fn verbose_missing_element_and_separator_same_offset_both_recorded() {
  let mut v = Verbose::<ReqError>::new();

  let miss_sep: MissingTokenOf<'_, TestLexer<'_>> = MissingToken::new(3usize);
  <Verbose<ReqError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
    &mut v,
    CowStr::from_static("comma"),
    miss_sep,
  )
  .unwrap();

  let miss_elem: MissingSyntaxOf<'_, TestLexer<'_>> = MissingSyntax::new(3usize);
  <Verbose<ReqError> as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_element(
    &mut v, miss_elem,
  )
  .unwrap();

  let errs = v.errors();
  assert_eq!(errs.len(), 1, "one zero-width span key at offset 3");
  assert_eq!(
    errs.get(&SimpleSpan::new(3usize, 3usize)).map(Vec::len),
    Some(2),
    "missing-element and missing-separator at the same offset are both retained"
  );
}

// ── The separator's name reaches the collected diagnostic ─────────────────────
//
// The driver knows the separator's name and hands it to the emitter; the emitter hands it to
// the conversion; the conversion is a blanket a downstream error type cannot override. This
// pins the whole pipeline rather than the conversion in isolation: the fixtures above discard
// the payload, so a name that never left the driver would look identical to one that arrived.

/// A downstream error type that keeps the separator name the conversions stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NamedError {
  MissingSeparator(Option<String>),
  Other,
}

impl From<()> for NamedError {
  fn from(_: ()) -> Self {
    NamedError::Other
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for NamedError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    NamedError::Other
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for NamedError
{
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    NamedError::Other
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for NamedError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    NamedError::Other
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for NamedError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    NamedError::Other
  }
}

impl From<UnexpectedEot> for NamedError {
  fn from(_: UnexpectedEot) -> Self {
    NamedError::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for NamedError {
  fn from(err: MissingToken<'a, Kind, O, Lang>) -> Self {
    NamedError::MissingSeparator(err.name().map(|n| n.as_str().to_string()))
  }
}

#[test]
fn driver_end_to_end_names_the_separator() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, Verbose<NamedError>>,
    >,
  ) -> Result<Vec<i64>, NamedError> {
    let out = try_named
      .separated_by_comma()
      .require_trailing()
      .collect()
      .parse_input(inp)?;

    let collected: Vec<&NamedError> = inp.emitter_ref().errors().values().flatten().collect();
    assert_eq!(
      collected,
      vec![&NamedError::MissingSeparator(Some("COMMA".to_string()))],
      "the separator's own name must reach the collected diagnostic"
    );
    Ok(out)
  }

  fn try_named<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, NamedError>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = NamedError>,
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

  let ctx: ParserContext<'static, TestLexer<'static>, Verbose<NamedError>> =
    ParserContext::new(Verbose::new());
  let out: Vec<i64> = Parser::with_context(ctx)
    .apply(parse)
    .parse_str("1,2,3")
    .unwrap();
  assert_eq!(out, vec![1, 2, 3]);
}
