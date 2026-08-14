#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Behavioral tests for `ParseChoice::dispatch_on_kind` — the kind-keyed dispatch
//! combinator whose committed failure carries the full static expected set.

mod common;

use common::{TestLexer, Token, TokenKind};
use tokora::{
  InputRef, Parse, ParseChoice, ParseContext, ParseInput, ParseTokenChoice, Parser, ParserContext,
  SimpleSpan, Token as TokenTrait, TryParseInput,
  emitter::Verbose,
  error::{UnexpectedEot, token::UnexpectedToken},
  parser::Any,
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::Expected,
};

// ── Error type that *preserves* the expected set for value assertions ──────────

#[derive(Debug, Clone, PartialEq)]
enum DispatchError {
  Unexpected {
    expected: Vec<TokenKind>,
    found: Option<TokenKind>,
  },
  // End of token stream now carries the expected set (the whole dispatch table), lifted from the
  // `UnexpectedEot`'s optional expected field.
  Eot {
    expected: Vec<TokenKind>,
  },
  Branch,
  Lexer,
}

impl From<()> for DispatchError {
  fn from(_: ()) -> Self {
    DispatchError::Lexer
  }
}

impl<'a, S, Lang: ?Sized> From<UnexpectedToken<'a, Token, TokenKind, S, Lang>> for DispatchError {
  fn from(err: UnexpectedToken<'a, Token, TokenKind, S, Lang>) -> Self {
    let (_span, found, expected) = err.into_components();
    let expected = match expected {
      Some(Expected::One(kind)) => vec![kind],
      Some(Expected::OneOf(one_of)) => one_of.as_slice().to_vec(),
      _ => Vec::new(),
    };
    DispatchError::Unexpected {
      expected,
      found: found.as_ref().map(|token| token.kind()),
    }
  }
}

// The kind-set EOT that `DispatchOnKind` raises at a committed end-of-input: extract the table.
impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang, TokenKind>> for DispatchError {
  fn from(err: UnexpectedEot<O, Lang, TokenKind>) -> Self {
    let expected = match err.expected() {
      Some(Expected::One(kind)) => vec![*kind],
      Some(Expected::OneOf(one_of)) => one_of.as_slice().to_vec(),
      _ => Vec::new(),
    };
    DispatchError::Eot { expected }
  }
}

// The plain EOT (default expected-set element type) that the branch parsers (`Any`) can raise —
// distinct source type from the kind-set form above, so both conversions coexist.
impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for DispatchError {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    DispatchError::Eot {
      expected: Vec::new(),
    }
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for DispatchError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    DispatchError::Eot {
      expected: Vec::new(),
    }
  }
}

// ── The parsers under test ─────────────────────────────────────────────────────

/// A three-alternative dispatch: number → B0, `+` → B1, `(` → B2. Each branch
/// consumes its token and reports which branch ran (its kind).
fn dispatch3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<TokenKind, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  (
    Any::new().map(|_| TokenKind::Num),
    Any::new().map(|_| TokenKind::Plus),
    Any::new().map(|_| TokenKind::LParen),
  )
    .dispatch_on_kind(&[TokenKind::Num, TokenKind::Plus, TokenKind::LParen])
    .parse_input(inp)
}

/// A single-alternative dispatch: number → B0.
fn dispatch1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<TokenKind, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  (Any::new().map(|_| TokenKind::Num),)
    .dispatch_on_kind(&[TokenKind::Num])
    .parse_input(inp)
}

fn verbose_ctx() -> ParserContext<'static, TestLexer<'static>, Verbose<DispatchError>> {
  ParserContext::new(Verbose::new())
}

// ── Tests ───────────────────────────────────────────────────────────────────────

#[test]
fn dispatch_failure_reports_full_expected_set_in_table_order() {
  // `;` (Semi) is not a viable first token; the failure reports the whole table,
  // in table order, alongside the found token.
  let r = Parser::new().apply(dispatch3).parse_str(";");
  assert_eq!(
    r,
    Err(DispatchError::Unexpected {
      expected: vec![TokenKind::Num, TokenKind::Plus, TokenKind::LParen],
      found: Some(TokenKind::Semi),
    })
  );
}

#[test]
fn dispatch_eof_reports_eot() {
  // End-of-input at the dispatch point: `UnexpectedEot` now carries the full expected set — the
  // whole table, in table order — exactly like the `Miss` arm's `UnexpectedToken`.
  let r = Parser::new().apply(dispatch3).parse_str("");
  assert_eq!(
    r,
    Err(DispatchError::Eot {
      expected: vec![TokenKind::Num, TokenKind::Plus, TokenKind::LParen],
    })
  );
}

#[test]
fn single_alternative_dispatch_reports_single_expectation() {
  // Miss over a one-entry table still reports that one expectation.
  let r = Parser::new().apply(dispatch1).parse_str(";");
  assert_eq!(
    r,
    Err(DispatchError::Unexpected {
      expected: vec![TokenKind::Num],
      found: Some(TokenKind::Semi),
    })
  );
  // ...and dispatches successfully on a hit.
  assert_eq!(
    Parser::new().apply(dispatch1).parse_str("7"),
    Ok(TokenKind::Num)
  );
}

#[test]
fn fatal_and_verbose_observe_the_same_payload() {
  let expected = Err(DispatchError::Unexpected {
    expected: vec![TokenKind::Num, TokenKind::Plus, TokenKind::LParen],
    found: Some(TokenKind::Semi),
  });

  // Fatal (fail-fast) and Verbose (error-collecting) surface the identical payload.
  let fatal = Parser::new().apply(dispatch3).parse_str(";");
  let verbose = Parser::with_context(verbose_ctx())
    .apply(dispatch3)
    .parse_str(";");

  assert_eq!(fatal, expected);
  assert_eq!(verbose, expected);
  assert_eq!(fatal, verbose);
}

#[test]
fn successful_dispatch_runs_the_selected_branch_unchanged() {
  // Scoping guard: on a hit there is no diagnostic and the correct branch runs,
  // identically under a fail-fast and an error-collecting emitter.
  for (src, want) in [
    ("42", TokenKind::Num),
    ("+", TokenKind::Plus),
    ("(", TokenKind::LParen),
  ] {
    assert_eq!(Parser::new().apply(dispatch3).parse_str(src), Ok(want));
    assert_eq!(
      Parser::with_context(verbose_ctx())
        .apply(dispatch3)
        .parse_str(src),
      Ok(want)
    );
  }
}

// ── Fused dispatch: the lex-once twin, observationally identical to the peek shape ──
//
// `FusedDispatchOnKind` lexes the decision token *once* and hands it to the winning arm
// (a `ParseTokenChoice` arm, `FnMut(head, inp) -> Result<O, E>`), where `DispatchOnKind`
// peeks it (staging a cache round trip) and lets an `Any` arm consume it back out. These
// tests pin that the two shapes are observationally identical for the same table and input:
// same hit result, same committed-failure error, same stream/cache state afterward, and
// identical rollback — the equivalence contract `FusedDispatchOnKind`'s docs promise.

const TABLE3: &[TokenKind] = &[TokenKind::Num, TokenKind::Plus, TokenKind::LParen];
const TABLE1: &[TokenKind] = &[TokenKind::Num];

/// The input matrix the equivalence tests share: a hit on each arm (with and without a
/// trailing token), committed misses, and both end-of-input shapes.
const CASES: &[&str] = &[
  "42",     // hit B0 (Num), nothing after
  "42 foo", // hit B0, Ident after
  "+ 3",    // hit B1 (Plus), Num after
  "( )",    // hit B2 (LParen), RParen after
  ";",      // miss (Semi), nothing after
  "; foo",  // miss, Ident after
  "] 3",    // miss (RBracket)
  "* (",    // miss (Star)
  "",       // end of input
  "   ",    // end of input (all trivia skipped)
];

/// Shared fused arm: report the branch that ran by returning the head token's kind. Because
/// `dispatch_on_kind` is 1:1 (table entry ↔ branch), a hit on branch `i` always hands this arm
/// a token of kind `TABLE[i]`, so the returned kind names the branch — matching the fixed
/// per-branch label the peek shape's `Any::new().map(|_| kind)` arms return.
fn kind_arm<'inp, Ctx>(
  head: Spanned<Token, SimpleSpan>,
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<TokenKind, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  Ok(head.data.kind())
}

fn rejecting_arm<'inp, Ctx>(
  _head: Spanned<Token, SimpleSpan>,
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<TokenKind, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  Err(DispatchError::Branch)
}

/// The fused twin of [`dispatch3`].
fn fused3<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<TokenKind, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  (kind_arm, kind_arm, kind_arm)
    .fused_dispatch_on_kind(TABLE3)
    .parse_input(inp)
}

/// The fused twin of [`dispatch1`].
fn fused1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<TokenKind, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  (kind_arm,).fused_dispatch_on_kind(TABLE1).parse_input(inp)
}

/// The tentative fused dispatcher. A table miss or EOT is a decline instead of a diagnostic.
fn fused3_try<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<TokenKind>, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  (kind_arm, kind_arm, kind_arm)
    .fused_dispatch_on_kind(TABLE3)
    .try_parse_input(inp)
}

/// The same tentative parser followed by one token read, exposing the position it leaves behind.
fn fused3_try_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(ParseAttempt<TokenKind>, Option<TokenKind>), DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  let outcome = (kind_arm, kind_arm, kind_arm)
    .fused_dispatch_on_kind(TABLE3)
    .try_parse_input(inp)?;
  let leftover = inp.next()?.map(|t| t.data.kind());
  Ok((outcome, leftover))
}

fn fused3_try_rejecting<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<TokenKind>, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  (rejecting_arm, rejecting_arm, rejecting_arm)
    .fused_dispatch_on_kind(TABLE3)
    .try_parse_input(inp)
}

/// Runs the peek-shape dispatcher, then reads the next token — the stream state the *following*
/// parser observes. Returning both lets one comparison cover the hit result, the committed-failure
/// error, and the post-dispatch cache/stream state at once (a miss leaves the missed token, a hit
/// leaves whatever follows the consumed head).
fn dispatch3_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(Result<TokenKind, DispatchError>, Option<TokenKind>), DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  let outcome = (
    Any::new().map(|_| TokenKind::Num),
    Any::new().map(|_| TokenKind::Plus),
    Any::new().map(|_| TokenKind::LParen),
  )
    .dispatch_on_kind(TABLE3)
    .parse_input(inp);
  let leftover = inp.next()?.map(|t| t.data.kind());
  Ok((outcome, leftover))
}

/// The fused twin of [`dispatch3_probe`].
fn fused3_probe<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<(Result<TokenKind, DispatchError>, Option<TokenKind>), DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  let outcome = (kind_arm, kind_arm, kind_arm)
    .fused_dispatch_on_kind(TABLE3)
    .parse_input(inp);
  let leftover = inp.next()?.map(|t| t.data.kind());
  Ok((outcome, leftover))
}

/// Runs the peek-shape dispatcher inside an attempt that *always* rolls back, then reads the next
/// token — which must be the original first token, proving the attempt unwound the dispatch (the
/// staged/consumed token and its lineage push alike) back to the pre-attempt state.
fn dispatch3_rollback<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Option<TokenKind>, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  let _: Option<()> = inp.attempt(|inp| {
    let _ = (
      Any::new().map(|_| TokenKind::Num),
      Any::new().map(|_| TokenKind::Plus),
      Any::new().map(|_| TokenKind::LParen),
    )
      .dispatch_on_kind(TABLE3)
      .parse_input(inp);
    None
  });
  Ok(inp.next()?.map(|t| t.data.kind()))
}

/// The fused twin of [`dispatch3_rollback`].
fn fused3_rollback<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Option<TokenKind>, DispatchError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: tokora::Emitter<'inp, TestLexer<'inp>, Error = DispatchError>,
{
  let _: Option<()> = inp.attempt(|inp| {
    let _ = (kind_arm, kind_arm, kind_arm)
      .fused_dispatch_on_kind(TABLE3)
      .parse_input(inp);
    None
  });
  Ok(inp.next()?.map(|t| t.data.kind()))
}

#[test]
fn fused_hit_runs_correct_arm_per_branch() {
  // Hit-path correctness: each first token selects its branch, and the fused arm receives the
  // already-lexed head — reporting its kind names the branch that ran.
  for (src, want) in [
    ("42", TokenKind::Num),
    ("+", TokenKind::Plus),
    ("(", TokenKind::LParen),
  ] {
    assert_eq!(Parser::new().apply(fused3).parse_str(src), Ok(want));
  }
}

#[test]
fn fused_miss_reports_full_expected_set_in_table_order() {
  // Byte-identical to `dispatch_failure_reports_full_expected_set_in_table_order`.
  assert_eq!(
    Parser::new().apply(fused3).parse_str(";"),
    Err(DispatchError::Unexpected {
      expected: vec![TokenKind::Num, TokenKind::Plus, TokenKind::LParen],
      found: Some(TokenKind::Semi),
    })
  );
}

#[test]
fn fused_eof_reports_eot_with_table() {
  // Byte-identical to `dispatch_eof_reports_eot` (the W2c end-of-input behavior).
  assert_eq!(
    Parser::new().apply(fused3).parse_str(""),
    Err(DispatchError::Eot {
      expected: vec![TokenKind::Num, TokenKind::Plus, TokenKind::LParen],
    })
  );
}

#[test]
fn fused_single_alternative_dispatch() {
  assert_eq!(
    Parser::new().apply(fused1).parse_str("7"),
    Ok(TokenKind::Num)
  );
  assert_eq!(
    Parser::new().apply(fused1).parse_str(";"),
    Err(DispatchError::Unexpected {
      expected: vec![TokenKind::Num],
      found: Some(TokenKind::Semi),
    })
  );
}

#[test]
fn fused_try_hit_consumes_once_and_hands_head_to_the_arm() {
  // `kind_arm` reports its supplied head; the following read must therefore start after it.
  assert_eq!(
    Parser::new().apply(fused3_try_probe).parse_str("42 foo"),
    Ok((ParseAttempt::Accept(TokenKind::Num), Some(TokenKind::Ident)))
  );
}

#[test]
fn fused_try_miss_declines_and_leaves_the_token() {
  // A miss returns the tentative result and the next parser still sees the same head token.
  assert_eq!(
    Parser::new().apply(fused3_try_probe).parse_str("; foo"),
    Ok((ParseAttempt::Decline, Some(TokenKind::Semi)))
  );
}

#[test]
fn fused_try_eot_declines() {
  assert_eq!(
    Parser::new().apply(fused3_try).parse_str(""),
    Ok(ParseAttempt::Decline)
  );
}

#[test]
fn fused_try_propagates_selected_arm_error() {
  assert_eq!(
    Parser::new().apply(fused3_try_rejecting).parse_str("42"),
    Err(DispatchError::Branch)
  );
}

#[test]
fn fused_and_peek_return_identical_results() {
  // The headline equivalence: for the same table and input, both dispatch shapes return the exact
  // same `Result` — same hit value, same committed-failure `UnexpectedToken`/`UnexpectedEot`.
  for &src in CASES {
    assert_eq!(
      Parser::new().apply(fused3).parse_str(src),
      Parser::new().apply(dispatch3).parse_str(src),
      "fused vs peek result diverged on {src:?}"
    );
  }
}

#[test]
fn fused_miss_leaves_the_same_stream_state_as_peek() {
  // Same error AND same subsequent parse: the probe reads the token left after the dispatch, so a
  // divergence in the put-back (a missed token not restored, or restored differently) shows up as
  // a different leftover. Fatal emitter (fail-fast).
  for &src in CASES {
    assert_eq!(
      Parser::new().apply(fused3_probe).parse_str(src),
      Parser::new().apply(dispatch3_probe).parse_str(src),
      "fused vs peek stream state diverged on {src:?} (fatal)"
    );
  }
}

#[test]
fn fused_miss_leaves_the_same_stream_state_as_peek_verbose() {
  // Same as above under an error-collecting emitter: equal outcomes prove the emission path is
  // shared and neither shape routes the miss/EOT through the emitter (it rides the Err channel).
  for &src in CASES {
    let peek = Parser::with_context(verbose_ctx())
      .apply(dispatch3_probe)
      .parse_str(src);
    let fused = Parser::with_context(verbose_ctx())
      .apply(fused3_probe)
      .parse_str(src);
    assert_eq!(
      peek, fused,
      "fused vs peek stream state diverged on {src:?} (verbose)"
    );
  }
}

#[test]
fn fused_rollback_unwinds_identically_to_peek() {
  // Rollback-equivalence: run each shape inside an attempt that always declines, then read the
  // next token. Both must surface the original first token — the fused put-back's lineage push is
  // dropped by the restore exactly like the peek shape's staged token.
  for &src in CASES {
    assert_eq!(
      Parser::new().apply(fused3_rollback).parse_str(src),
      Parser::new().apply(dispatch3_rollback).parse_str(src),
      "fused vs peek rollback diverged on {src:?}"
    );
  }
}

#[test]
fn fused_matches_peek_on_lexer_errors() {
  // Lexer errors (`@`, `#` — unlexable by the test grammar) surface on the shared `scan_with`
  // path both shapes use. Under Fatal the first lexer error is fatal, so equal results prove it is
  // emitted at the same point; under Verbose both collect it and the dispatch then reaches the same
  // hit/miss — proving the deduplicated emission log is identical.
  for &src in &["@42", "@;", "@ @ 3", "# +", "@"] {
    assert_eq!(
      Parser::new().apply(fused3).parse_str(src),
      Parser::new().apply(dispatch3).parse_str(src),
      "lexer-error divergence on {src:?} (fatal)"
    );
    let peek = Parser::with_context(verbose_ctx())
      .apply(dispatch3)
      .parse_str(src);
    let fused = Parser::with_context(verbose_ctx())
      .apply(fused3)
      .parse_str(src);
    assert_eq!(peek, fused, "lexer-error divergence on {src:?} (verbose)");
  }
}
