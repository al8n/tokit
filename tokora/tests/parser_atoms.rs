#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! The dialect-free atom surface: the `ComposableEmitter`/`ComposableParseContext` bundles and the
//! policy atoms promoted from smear-parser-next (W-MOVE).
//!
//! The cases are ported one-for-one from the smear-side suites (`combinator/tests.rs`
//! for the bundle gate, `combinator/shape/tests.rs` for the atoms), with the fixtures
//! adapted from smear's GraphQL lexers to the shared `common` logos fixture: smear's
//! three-source matrix (`str`/`[u8]`/`Bytes`) collapses to the fixture's `str`, its two
//! dialects collapse to `TestLexer` driven under two context shapes, and its `@`/`|`
//! tokens map to the fixture's `,`/`;` punctuators. Every assertion is otherwise the
//! same as the smear original it pins.

mod common;

use common::{E, TestLexer, Token, TokenKind};

use tokora::{
  ComposableParseContext, Emitter, ErrorOf, FatalContext, InputRef, Lexer, Parse, ParseInput,
  Parser, ParserContext, SimpleSpan,
  emitter::{
    Fatal, FullContainerEmitter, SeparatedEmitter, TooFewEmitter, UnclosedEmitter, Verbose,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::{
    angles, braces, brackets, delimited, list, opt, parens, peek_kind, separated1, try_angles,
    try_braces, try_brackets, try_delimited, try_parens,
  },
  punct::{Brace, Bracket, CloseBrace, Comma, Paren, Semicolon},
  span::Spanned,
  token::IdentifierToken,
  types::Ident,
  utils::Expected,
};

// The atoms' item parser is `Ident::parse`, which classifies through
// `IdentifierToken`; the shared fixture token only wires `PunctuatorToken`, so the
// identifier facet lives here with the one suite that needs it.
impl IdentifierToken<'_> for Token {
  fn is_identifier(&self) -> bool {
    matches!(self, Token::Ident)
  }
}

// ── The bundle gate (smear: combinator/tests.rs) ──────────────────────────────

fn assert_ctx<'inp, L, Ctx>()
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L>,
{
}

fn requires_separated<'inp, L, Em>()
where
  L: Lexer<'inp>,
  Em: SeparatedEmitter<'inp, L>,
{
}

fn requires_too_few<'inp, L, Em>()
where
  L: Lexer<'inp>,
  Em: TooFewEmitter<'inp, L>,
{
}

fn requires_full_container<'inp, L, Em>()
where
  L: Lexer<'inp>,
  Em: FullContainerEmitter<'inp, L>,
{
}

fn requires_unclosed<'inp, L, Em>()
where
  L: Lexer<'inp>,
  Em: UnclosedEmitter<'inp, L>,
{
}

fn elaborates<'inp, L, Ctx>()
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L>,
{
  requires_separated::<L, Ctx::Emitter>();
  requires_too_few::<L, Ctx::Emitter>();
  requires_full_container::<L, Ctx::Emitter>();
  requires_unclosed::<L, Ctx::Emitter>();
}

// Smear's `ctx_bundle_holds_for_both_dialects_and_sources`: two dialects over two
// sources there; here the fixture lexer under both a fail-fast and a collecting
// context, pinning that the bundle holds and elaborates for each.
#[test]
fn ctx_bundle_holds_for_fatal_and_verbose_contexts() {
  assert_ctx::<TestLexer<'_>, FatalContext<'_, TestLexer<'_>, E>>();
  assert_ctx::<TestLexer<'_>, ParserContext<'_, TestLexer<'_>, Verbose<E>>>();
  elaborates::<TestLexer<'_>, FatalContext<'_, TestLexer<'_>, E>>();
  elaborates::<TestLexer<'_>, ParserContext<'_, TestLexer<'_>, Verbose<E>>>();
}

// Smear's `trivial_parse_drives_over_str_and_slice`: a bundle-shaped closure drives an
// actual parse end to end (the fixture is `str`-only, so the slice arm collapses).
#[test]
fn trivial_parse_drives_through_the_bundle() {
  fn drive_fatal<'inp, O>(
    f: impl for<'c> FnMut(
      &mut InputRef<'inp, 'c, TestLexer<'inp>, FatalContext<'inp, TestLexer<'inp>, E>>,
    ) -> Result<O, E>,
    input: &'inp str,
  ) -> Result<O, E> {
    Parser::with_parser(f).parse_str(input)
  }

  let out = drive_fatal(|inp| inp.next().map(|t| t.is_some()), "query");
  assert!(matches!(out, Ok(true)));
}

// ── The atom harness (smear: shape/tests.rs) ─────────────────────────────────
//
// The port of smear's `drive_str`, the surviving arm of its `drive_all!` source
// matrix: one concrete lexer, still generic over the emitter, so the same closure
// runs under a fail-fast `Fatal` and a collecting `Verbose` context.

fn drive<'inp, O, Em>(
  emitter: Em,
  f: impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Em>>,
  ) -> Result<O, E>,
  input: &'inp str,
) -> Result<O, E>
where
  Em: Emitter<'inp, TestLexer<'inp>, Error = E> + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  let ctx: ParserContext<'inp, TestLexer<'inp>, Em> = ParserContext::new(emitter);
  Parser::with_parser_and_context(f, ctx).parse_str(input)
}

/// Continue-predicate for `separated1`: the next token is an identifier, so it starts
/// another item in a `,`-separated list of names.
fn starts_ident(tok: &Token) -> bool {
  matches!(tok, Token::Ident)
}

/// Stop-predicate for `list`: the next token is the closing brace that ends the
/// list, so the loop stops and leaves the `}` in place.
fn is_close_brace(tok: &Token) -> bool {
  matches!(tok, Token::RBrace)
}

// ── `separated1` (smear: shape/tests.rs) ─────────────────────────────────────
//
// One-or-more `,`-separated idents (smear drove `|`; the fixture's separator is the
// comma), leading separator allowed, trailing rejected, at least one required. The
// sources are spaced exactly like smear's, so every span assertion carries over
// unchanged.

#[test]
fn separated1_three_idents() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let items = separated1::<Comma, _, _, _, _, _, _>(Ident::parse, starts_ident)(inp)?;
      assert_eq!(items.len(), 3);
      assert_eq!(*items[0].source_ref(), "A");
      assert_eq!(items[0].span(), SimpleSpan::new(0, 1));
      assert_eq!(*items[1].source_ref(), "B");
      assert_eq!(items[1].span(), SimpleSpan::new(4, 5));
      assert_eq!(*items[2].source_ref(), "C");
      assert_eq!(items[2].span(), SimpleSpan::new(8, 9));
      Ok::<_, E>(())
    },
    "A , B , C",
  );
  assert!(out.is_ok());
}

#[test]
fn separated1_allows_leading_separator() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      // The leading `,` is consumed and does not count as an item, so the two
      // names remain and their spans skip the leading separator.
      let items = separated1::<Comma, _, _, _, _, _, _>(Ident::parse, starts_ident)(inp)?;
      assert_eq!(items.len(), 2);
      assert_eq!(*items[0].source_ref(), "A");
      assert_eq!(items[0].span(), SimpleSpan::new(2, 3));
      assert_eq!(*items[1].source_ref(), "B");
      assert_eq!(items[1].span(), SimpleSpan::new(6, 7));
      Ok::<_, E>(())
    },
    ", A , B",
  );
  assert!(out.is_ok());
}

// A trailing separator is unexpected (no `allow_trailing`): the closing `,` with no
// item after it is an unexpected-trailing-separator emit. Under a fail-fast emitter it
// aborts to `Err`; under a collecting emitter it is recorded and the parse threads on
// to return the one item gathered before the trailing separator.
#[test]
fn separated1_trailing_separator_errors() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| separated1::<Comma, _, _, _, _, _, _>(Ident::parse, starts_ident)(inp).map(|_| ()),
    "A ,",
  );
  assert!(out.is_err());

  let out = drive(
    Verbose::<E>::new(),
    |inp| {
      let items = separated1::<Comma, _, _, _, _, _, _>(Ident::parse, starts_ident)(inp)?;
      assert_eq!(items.len(), 1);
      assert_eq!(*items[0].source_ref(), "A");
      assert_eq!(items[0].span(), SimpleSpan::new(0, 1));
      Ok::<_, E>(())
    },
    "A ,",
  );
  assert!(out.is_ok());
}

// At least one item is required: empty input is a too-few emit. Under a fail-fast
// emitter it aborts to `Err`; under a collecting emitter it is recorded and the parse
// returns the empty collection.
#[test]
fn separated1_empty_errors() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| separated1::<Comma, _, _, _, _, _, _>(Ident::parse, starts_ident)(inp).map(|_| ()),
    "",
  );
  assert!(out.is_err());

  let out = drive(
    Verbose::<E>::new(),
    |inp| {
      let items = separated1::<Comma, _, _, _, _, _, _>(Ident::parse, starts_ident)(inp)?;
      assert!(items.is_empty());
      Ok::<_, E>(())
    },
    "",
  );
  assert!(out.is_ok());
}

// ── `list` (smear: shape/tests.rs) ────────────────────────────────────────
//
// Zero-or-more idents, no separator, stopping at the `}` the stop predicate accepts.
// The accept path asserts each item's slice and span, then commits `CloseBrace` to
// prove `list` stopped before the `}` and left it in place.

#[test]
fn list_three_idents_until_brace() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let items = list(Ident::parse, is_close_brace)(inp)?;
      assert_eq!(items.len(), 3);
      assert_eq!(*items[0].source_ref(), "a");
      assert_eq!(items[0].span(), SimpleSpan::new(0, 1));
      assert_eq!(*items[1].source_ref(), "b");
      assert_eq!(items[1].span(), SimpleSpan::new(2, 3));
      assert_eq!(*items[2].source_ref(), "c");
      assert_eq!(items[2].span(), SimpleSpan::new(4, 5));
      // The `}` was the stop token, left in place, so the committed closer parses it.
      let close = CloseBrace::parse(inp)?;
      assert_eq!(close.span(), &SimpleSpan::new(6, 7));
      Ok::<_, E>(())
    },
    "a b c }",
  );
  assert!(out.is_ok());
}

// An immediate stop yields an empty list with the stop token left in place — the
// zero-or-more lower bound, with no diagnostic.
#[test]
fn list_empty_leaves_stop_token() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let items = list(Ident::parse, is_close_brace)(inp)?;
      assert!(items.is_empty());
      let close = CloseBrace::parse(inp)?;
      assert_eq!(close.span(), &SimpleSpan::new(0, 1));
      Ok::<_, E>(())
    },
    "}",
  );
  assert!(out.is_ok());
}

// The observation each differential row compares is `(drove, item count, diagnostic spans,
// tokens left unconsumed)`. That last part is the one a count and a diagnostic list cannot
// express: a delegate returning the right items while swallowing the stop token the free
// function leaves in place produced an identical tuple and passed.
//
// It is deliberately NOT `inp.offset()`, which was tried first and does not work. `offset()`
// tracks the LEX frontier, and the stop predicate peeks the token in order to decide — so the
// frontier has already advanced past it and reads the same whether or not the cached token is
// later consumed. Under a swallowing mutant both sides read `(true, 0, 1, [])` on the `"}"`
// row and the cell passed. Draining what a later parser could still see measures the thing
// itself rather than a number that moves for a different reason.
fn remaining<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Verbose<E>>>,
) -> Vec<(SimpleSpan, String)> {
  let mut out = Vec::new();
  loop {
    if !matches!(inp.peek_one(), Ok(Some(_))) {
      break;
    }
    match inp.consume_cached_one() {
      Some(tok) => out.push((*tok.span_ref(), format!("{:?}", tok.data()))),
      None => break,
    }
  }
  out
}

// Every diagnostic span the collecting emitter recorded, in span order — `errors` is a
// `BTreeMap`, so the keys come out sorted and the comparison is stable. Used to compare a
// fluent form against the free function it delegates to on inputs where the two could
// diverge in *which* diagnostic they raise rather than in how many items they return.
fn error_spans<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Verbose<E>>>,
) -> Vec<SimpleSpan> {
  inp.emitter_ref().errors().keys().copied().collect()
}

// ── The fluent forms of the two list atoms ────────────────────────────────
//
// `list_until` and `separated1_by` are method forms delegating to `list` and
// `separated1`. They shipped with no cell of their own: nothing in the suite called
// either, so delegating to the wrong free function, or routing an argument wrongly,
// would not have shown up anywhere. Each cell asserts the method form produces what
// the free function it is named after produces, including where it stops.

#[test]
fn list_until_is_the_method_form_of_list() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let items = Ident::parse.list_until(is_close_brace)(inp)?;
      assert_eq!(items.len(), 3);
      assert_eq!(*items[0].source_ref(), "a");
      assert_eq!(items[0].span(), SimpleSpan::new(0, 1));
      assert_eq!(*items[2].source_ref(), "c");
      assert_eq!(items[2].span(), SimpleSpan::new(4, 5));
      // The stop token is left in place, exactly as the free function leaves it.
      let close = CloseBrace::parse(inp)?;
      assert_eq!(close.span(), &SimpleSpan::new(6, 7));
      Ok::<_, E>(())
    },
    "a b c }",
  );
  assert!(out.is_ok());

  // The same matrix the `separated1_by` cell carries, and for the same reason. This one's
  // single row does discriminate in both directions — a stop-predicate forced to `true`
  // yields no items and one forced to `false` never stops, and each fails the row above —
  // but that is a property of the two mutations that happened to be tried, not of the cell.
  // Comparing against the free function across inputs where stopping is and is not
  // reachable does not depend on guessing the mutation.
  for src in ["a b c }", "}", "a }", "a b", "", "a", "} a", "a } b"] {
    let by_method = drive(
      Verbose::<E>::new(),
      |inp| {
        let got = Ident::parse.list_until(is_close_brace)(inp);
        let ok = got.is_ok();
        let n = got.map(|v| v.len()).unwrap_or(0);
        Ok::<_, E>((ok, n, error_spans(inp), remaining(inp)))
      },
      src,
    );
    let by_free = drive(
      Verbose::<E>::new(),
      |inp| {
        let got = list(Ident::parse, is_close_brace)(inp);
        let ok = got.is_ok();
        let n = got.map(|v| v.len()).unwrap_or(0);
        Ok::<_, E>((ok, n, error_spans(inp), remaining(inp)))
      },
      src,
    );
    assert_eq!(
      by_method.is_ok(),
      by_free.is_ok(),
      "method and free must agree on whether {src:?} drives at all"
    );
    if let (Ok(m), Ok(f)) = (by_method, by_free) {
      assert_eq!(m, f, "method and free must agree on {src:?}");
    }
  }
}

#[test]
fn separated1_by_is_the_method_form_of_separated1() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let items = Ident::parse.separated1_by::<Comma, _>(starts_ident)(inp)?;
      assert_eq!(items.len(), 3);
      assert_eq!(*items[0].source_ref(), "A");
      assert_eq!(items[0].span(), SimpleSpan::new(0, 1));
      assert_eq!(*items[1].source_ref(), "B");
      assert_eq!(items[1].span(), SimpleSpan::new(4, 5));
      assert_eq!(*items[2].source_ref(), "C");
      assert_eq!(items[2].span(), SimpleSpan::new(8, 9));
      Ok::<_, E>(())
    },
    "A , B , C",
  );
  assert!(out.is_ok());

  // The row above cannot fail when `peek` is dropped, and that was measured, not supposed:
  // every separator in `A , B , C` is followed by an item, so a predicate that always
  // answers "another item follows" agrees with the real one everywhere on this input. The
  // continue-predicate is only load-bearing where it must DECLINE, and no row exercised
  // that. The matrix below adds the inputs where declining changes the outcome and compares
  // the method against the free function it delegates to, which is the claim the cell's name
  // makes.
  //
  // Its limit, stated as narrowly as it is actually true: a defect present in *both* the
  // method and the free function agrees on every row here and passes. What covers that is
  // only the absolute-assertion cells — the ones that name an expected value instead of
  // comparing two implementations — and they cover it *only for the inputs they name*.
  //
  // An earlier version of this comment said the free function's own rows above cover it.
  // That was false, and it was false in a way worth recording: mutating `list` itself to
  // drop the last item whenever input ends at EOF after items passed every cell in this
  // file and every one of the 6318 tests in the crate. The differential rows agreed because
  // both sides were wrong together, and no absolute cell named that input. The gap was the
  // sentence's, not the instrument's. `list_stops_at_end_of_input_keeping_every_item` below
  // now names it; the limit above is what remains after that.
  for src in [
    "A , B , C",
    "A ,",
    "A , ;",
    "A , , B",
    ", A , B",
    "A",
    "",
    ";",
    "A B",
  ] {
    let by_method = drive(
      Verbose::<E>::new(),
      |inp| {
        let got = Ident::parse.separated1_by::<Comma, _>(starts_ident)(inp);
        let ok = got.is_ok();
        let n = got.map(|v| v.len()).unwrap_or(0);
        Ok::<_, E>((ok, n, error_spans(inp), remaining(inp)))
      },
      src,
    );
    let by_free = drive(
      Verbose::<E>::new(),
      |inp| {
        let got = separated1::<Comma, _, _, _, _, _, _>(Ident::parse, starts_ident)(inp);
        let ok = got.is_ok();
        let n = got.map(|v| v.len()).unwrap_or(0);
        Ok::<_, E>((ok, n, error_spans(inp), remaining(inp)))
      },
      src,
    );
    assert_eq!(
      by_method.is_ok(),
      by_free.is_ok(),
      "method and free must agree on whether {src:?} drives at all"
    );
    if let (Ok(m), Ok(f)) = (by_method, by_free) {
      assert_eq!(m, f, "method and free must agree on {src:?}");
    }
  }
}

// `list` stopping at the end of input rather than at its stop token, asserted absolutely
// against named values for BOTH the free function and its method form.
//
// This is the cell whose absence made the both-forms limitation worse than documented. The
// differential matrices compare the two implementations against each other, so a defect in
// the shared free function agrees and passes; the pre-existing `list` cells drive `a b c }`
// and `}`, both of which stop at the token, so items-then-EOF was named by no absolute
// assertion anywhere. A `list` that dropped its last item at EOF passed all 6318 tests.
#[test]
fn list_stops_at_end_of_input_keeping_every_item() {
  // The free function.
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let items = list(Ident::parse, is_close_brace)(inp)?;
      assert_eq!(items.len(), 2, "both items must survive a stop at EOF");
      assert_eq!(*items[0].source_ref(), "a");
      assert_eq!(items[0].span(), SimpleSpan::new(0, 1));
      assert_eq!(*items[1].source_ref(), "b");
      assert_eq!(items[1].span(), SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "a b",
  );
  assert!(out.is_ok());

  // The method form, asserted against the same named values rather than against the free
  // function, so a defect shared by both is still caught.
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let items = Ident::parse.list_until(is_close_brace)(inp)?;
      assert_eq!(items.len(), 2, "both items must survive a stop at EOF");
      assert_eq!(*items[0].source_ref(), "a");
      assert_eq!(items[0].span(), SimpleSpan::new(0, 1));
      assert_eq!(*items[1].source_ref(), "b");
      assert_eq!(items[1].span(), SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "a b",
  );
  assert!(out.is_ok());
}

// ── `peek_kind` (smear: shape/tests.rs) ──────────────────────────────────────
//
// Reports the next kind without consuming, so the same peek repeats and a committed
// atom still parses the leftover; end of input peeks as `None`. Smear peeked an `@`;
// the fixture's stand-in punctuator is the comma.

#[test]
fn peek_kind_reports_comma_twice_then_comma_parses() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let first = peek_kind(inp)?;
      let second = peek_kind(inp)?;
      assert_eq!(first, Some(TokenKind::Comma));
      assert_eq!(first, second);
      // Peeking consumed nothing, so the committed `Comma` atom pulls the `,` straight
      // off the input; its span proves the leftover and that peeking left it in place.
      let comma = Comma::parse(inp)?;
      assert_eq!(comma.span(), &SimpleSpan::new(0, 1));
      Ok::<_, E>(())
    },
    ",x",
  );
  assert!(out.is_ok());
}

#[test]
fn peek_kind_none_on_empty_input() {
  let out = drive(Fatal::<E>::new(), peek_kind, "");
  assert!(matches!(out, Ok(None)));
}

// ── `opt` (smear: shape/tests.rs) ────────────────────────────────────────────
//
// An accepted `try_`-attempt becomes `Some`, a decline becomes `None` with the
// leftover left in place for the next atom. Smear lifted `try_at` over `@`/`:`; the
// fixture's stand-ins are the comma and the semicolon.

#[test]
fn opt_try_comma_accepts_comma() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let opt_comma = opt(Comma::try_parse)(inp)?;
      assert!(opt_comma.is_some());
      let comma = opt_comma.unwrap();
      assert_eq!(comma.span(), &SimpleSpan::new(0, 1));
      Ok::<_, E>(())
    },
    ",",
  );
  assert!(out.is_ok());
}

#[test]
fn opt_try_comma_declines_on_semicolon_and_leaves_it() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = opt(Comma::try_parse)(inp)?.is_none();
      // The `;` is untouched, so the committed `Semicolon` atom parses it; its span
      // proves the leftover's type and that the decline consumed nothing.
      let semi = Semicolon::parse(inp)?;
      assert_eq!(semi.span(), &SimpleSpan::new(0, 1));
      Ok::<_, E>(declined)
    },
    ";",
  );
  assert!(matches!(out, Ok(true)));
}

// ── Delimited shapes (smear: shape/tests.rs) ─────────────────────────────────
//
// `parens`/`braces`/`brackets`/`angles` commit the opener, run the inner sub-parser,
// commit the closer, and wrap the three in a `Delimited` spanning the whole construct;
// the generic `delimited::<D>` does the same through the `Delimiter` pair type. The
// accept paths assert the wrapped data (slice and span), each delimiter's span, and the
// construct span; the unterminated paths run under both emitter modes. Smear's `ident`
// inner becomes the fixture's `Ident::parse`.

/// An inner sub-parser that records one non-fatal diagnostic and then yields `()`. Under
/// a fail-fast emitter the emit is fatal and this errors; under a collecting emitter the
/// diagnostic is recorded, the emit recovers, and this returns `Ok` — so an enclosing
/// delimited atom threads on to commit its closer. It is a bare `fn` so the atom's
/// higher-order bound pins the lexer for the emitter call, the way the atom fns compose.
fn emit_then_recover<'inp, Em>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Em>>,
) -> Result<(), E>
where
  Em: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  inp.emit_error(Spanned::new(SimpleSpan::new(0, 0), E))?;
  Ok(())
}

#[test]
fn braces_wrap_ident() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let delimited = braces(Ident::parse)(inp)?;
      assert_eq!(*delimited.data().source_ref(), "x");
      assert_eq!(delimited.data().span(), SimpleSpan::new(1, 2));
      assert_eq!(delimited.span(), SimpleSpan::new(0, 3));
      assert_eq!(delimited.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(delimited.close_ref().span(), &SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "{x}",
  );
  assert!(out.is_ok());
}

#[test]
fn parens_wrap_ident() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let delimited = parens(Ident::parse)(inp)?;
      assert_eq!(*delimited.data().source_ref(), "x");
      assert_eq!(delimited.data().span(), SimpleSpan::new(1, 2));
      assert_eq!(delimited.span(), SimpleSpan::new(0, 3));
      assert_eq!(delimited.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(delimited.close_ref().span(), &SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "(x)",
  );
  assert!(out.is_ok());
}

#[test]
fn brackets_wrap_ident() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let delimited = brackets(Ident::parse)(inp)?;
      assert_eq!(*delimited.data().source_ref(), "x");
      assert_eq!(delimited.data().span(), SimpleSpan::new(1, 2));
      assert_eq!(delimited.span(), SimpleSpan::new(0, 3));
      assert_eq!(delimited.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(delimited.close_ref().span(), &SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "[x]",
  );
  assert!(out.is_ok());
}

// An empty group with an `opt(Ident::try_parse)` inner yields `None` inside, proving the
// inner runs between the delimiters and that a declining inner leaves the closer for the
// atom to commit.
#[test]
fn braces_empty_with_opt_inner_yields_none() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let delimited = braces(opt(Ident::try_parse))(inp)?;
      assert!(delimited.data().is_none());
      assert_eq!(delimited.span(), SimpleSpan::new(0, 2));
      assert_eq!(delimited.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(delimited.close_ref().span(), &SimpleSpan::new(1, 2));
      Ok::<_, E>(())
    },
    "{}",
  );
  assert!(out.is_ok());
}

// Unterminated groups: the inner consumes the `a` ident, then the missing closer reports the
// opener as `Unclosed` through the emitter. A fail-fast emitter turns it into `Err`; a
// collecting emitter records it and recovers, yielding the group with a synthesized closer —
// so Fatal returns `Err` and Verbose returns `Ok`. (Before the shape-Unclosed fix both modes
// returned `Err`.)

#[test]
fn braces_unterminated_reports_unclosed() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| braces(Ident::parse)(inp).map(|_| ()),
    "{a",
  );
  assert!(out.is_err(), "fatal: Unclosed becomes Err");
  let out = drive(
    Verbose::<E>::new(),
    |inp| braces(Ident::parse)(inp).map(|_| ()),
    "{a",
  );
  assert!(out.is_ok(), "verbose: records Unclosed and recovers");
}

#[test]
fn parens_unterminated_reports_unclosed() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| parens(Ident::parse)(inp).map(|_| ()),
    "(a",
  );
  assert!(out.is_err(), "fatal: Unclosed becomes Err");
  let out = drive(
    Verbose::<E>::new(),
    |inp| parens(Ident::parse)(inp).map(|_| ()),
    "(a",
  );
  assert!(out.is_ok(), "verbose: records Unclosed and recovers");
}

#[test]
fn brackets_unterminated_reports_unclosed() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| brackets(Ident::parse)(inp).map(|_| ()),
    "[a",
  );
  assert!(out.is_err(), "fatal: Unclosed becomes Err");
  let out = drive(
    Verbose::<E>::new(),
    |inp| brackets(Ident::parse)(inp).map(|_| ()),
    "[a",
  );
  assert!(out.is_ok(), "verbose: records Unclosed and recovers");
}

// The collecting-mode recovery continuation: an inner sub-parser that records a non-fatal
// diagnostic and then yields. Under a fail-fast emitter the emit is fatal, so `braces`
// aborts before the closer and returns `Err`. Under a collecting emitter the emit is
// recorded and recovers, so the inner returns `Ok`, `braces` threads on to commit the `}`
// closer, and it returns the delimited construct spanning `{}`.
#[test]
fn braces_thread_collecting_mode_inner_emit() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| braces(emit_then_recover)(inp).map(|_| ()),
    "{}",
  );
  assert!(out.is_err());
  let out = drive(
    Verbose::<E>::new(),
    |inp| {
      let delimited = braces(emit_then_recover)(inp)?;
      assert_eq!(delimited.span(), SimpleSpan::new(0, 2));
      assert_eq!(delimited.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(delimited.close_ref().span(), &SimpleSpan::new(1, 2));
      Ok::<_, E>(())
    },
    "{}",
  );
  assert!(out.is_ok());
}

// New coverage: `angles` (the pair with no smear-side dialect vehicle) over the fixture,
// which wires both capability routes for `<`/`>`.
#[test]
fn angles_wrap_ident() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let delimited = angles(Ident::parse)(inp)?;
      assert_eq!(*delimited.data().source_ref(), "x");
      assert_eq!(delimited.data().span(), SimpleSpan::new(1, 2));
      assert_eq!(delimited.span(), SimpleSpan::new(0, 3));
      assert_eq!(delimited.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(delimited.close_ref().span(), &SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "<x>",
  );
  assert!(out.is_ok());
}

#[test]
fn angles_unterminated_reports_unclosed() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| angles(Ident::parse)(inp).map(|_| ()),
    "<a",
  );
  assert!(out.is_err(), "fatal: Unclosed becomes Err");
  let out = drive(
    Verbose::<E>::new(),
    |inp| angles(Ident::parse)(inp).map(|_| ()),
    "<a",
  );
  assert!(out.is_ok(), "verbose: records Unclosed and recovers");
}

// The `≡` proof: the generic `delimited::<Paren, …>` over `(x)` yields the same data and
// spans as `parens` (test `parens_wrap_ident`) — and, because the built-in pair's
// `OpenValue`/`CloseValue` normalize to the named alias inner types, the same result type.
#[test]
fn delimited_generic_paren_equals_parens() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let d = delimited::<Paren, _, _, _, _, _, _>(Ident::parse)(inp)?;
      assert_eq!(*d.data().source_ref(), "x");
      assert_eq!(d.data().span(), SimpleSpan::new(1, 2));
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      assert_eq!(d.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(d.close_ref().span(), &SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "(x)",
  );
  assert!(out.is_ok());
}

#[test]
fn delimited_method_wraps_ident() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let d = Ident::parse.delimited::<Paren>().parse_input(inp)?;
      assert_eq!(*d.data().source_ref(), "x");
      assert_eq!(d.data().span(), SimpleSpan::new(1, 2));
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(())
    },
    "(x)",
  );
  assert!(out.is_ok());
}

#[test]
fn delimited_convenience_methods_wrap_idents() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let parens = Ident::parse.delimited_by_parens().parse_input(inp)?;
      let braces = Ident::parse.delimited_by_braces().parse_input(inp)?;
      let brackets = Ident::parse.delimited_by_brackets().parse_input(inp)?;
      let angles = Ident::parse.delimited_by_angles().parse_input(inp)?;

      assert_eq!(parens.data().source_ref(), &"a");
      assert_eq!(braces.data().source_ref(), &"b");
      assert_eq!(brackets.data().source_ref(), &"c");
      assert_eq!(angles.data().source_ref(), &"d");
      Ok::<_, E>(())
    },
    "(a){b}[c]<d>",
  );
  assert!(out.is_ok());
}

#[derive(Debug, PartialEq)]
enum Nested<'inp> {
  Name(&'inp str),
  List(Box<Nested<'inp>>),
}

fn nested<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Nested<'inp>, ErrorOf<'inp, TestLexer<'inp>, Ctx, ()>>
where
  Ctx: ComposableParseContext<'inp, TestLexer<'inp>>,
{
  match peek_kind(inp)? {
    Some(TokenKind::LBracket) => nested
      .delimited::<Bracket>()
      .parse_input(inp)
      .map(|d| Nested::List(Box::new(d.into_data()))),
    _ => Ident::parse(inp).map(|ident| Nested::Name(ident.source_ref())),
  }
}

#[test]
fn delimited_by_brackets_supports_recursive_generic_parsers() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      nested
        .delimited_by_brackets()
        .parse_input(inp)
        .map(|d| Nested::List(Box::new(d.into_data())))
    },
    "[[x]]",
  )
  .unwrap();
  assert_eq!(
    out,
    Nested::List(Box::new(Nested::List(Box::new(Nested::Name("x")))))
  );
}

#[test]
fn delimited_method_eof_reports_typed_unclosed() {
  fn unterminated<'a>(
    inp: &mut InputRef<'a, '_, TestLexer<'a>, FatalContext<'a, TestLexer<'a>, ShapeError>>,
  ) -> Result<(), ShapeError> {
    Ident::parse
      .delimited::<Paren>()
      .parse_input(inp)
      .map(|_| ())
  }

  let err = Parser::with_parser(unterminated)
    .parse_str("(a")
    .unwrap_err();
  assert!(matches!(
    err,
    ShapeError::Unclosed { ref name, start: 0 } if name == "()"
  ));
}

// A wrong closer is a hard error carrying the expected close kind. `ShapeError` (local,
// discriminating — the shared `E` is unit) pins that the error IS an `UnexpectedToken`
// naming the close-brace kind, not a fabricated delimiter.
#[test]
fn delimited_wrong_close_reports_unexpected_token() {
  fn wrong_close<'a>(
    inp: &mut InputRef<'a, '_, TestLexer<'a>, FatalContext<'a, TestLexer<'a>, ShapeError>>,
  ) -> Result<(), ShapeError> {
    delimited::<Brace, _, _, _, _, _, _>(Ident::parse)(inp).map(|_| ())
  }
  let err = Parser::with_parser(wrong_close)
    .parse_str("{x)")
    .unwrap_err();
  assert!(matches!(
    err,
    ShapeError::Unexpected(Some(TokenKind::RBrace))
  ));
}

// A missing closer at end of input reports the opener as `Unclosed` through the emitter (a
// fail-fast context turns it into `Err`), pinned distinct from the wrong-closer
// unexpected-token path by the same local discriminating error. (Before the shape-Unclosed
// fix this was an `UnexpectedEot`.)
#[test]
fn delimited_eof_reports_unclosed() {
  fn unterminated<'a>(
    inp: &mut InputRef<'a, '_, TestLexer<'a>, FatalContext<'a, TestLexer<'a>, ShapeError>>,
  ) -> Result<(), ShapeError> {
    delimited::<Paren, _, _, _, _, _, _>(Ident::parse)(inp).map(|_| ())
  }
  let err = Parser::with_parser(unterminated)
    .parse_str("(a")
    .unwrap_err();
  match err {
    ShapeError::Unclosed { name, start } => {
      assert_eq!(name, "()", "the opener's delimiter-pair name");
      assert_eq!(start, 0, "anchored at the opener");
    }
    other => panic!("missing closer at EOF must be Unclosed, got {other:?}"),
  }
}

// Nesting composes: `braces(brackets(ident))` over `{[x]}` wraps the bracket construct as
// the brace construct's data, each carrying its own delimiter and construct spans.
#[test]
fn nested_delimiters_compose() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let delimited = braces(brackets(Ident::parse))(inp)?;
      assert_eq!(delimited.span(), SimpleSpan::new(0, 5));
      assert_eq!(delimited.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(delimited.close_ref().span(), &SimpleSpan::new(4, 5));
      let inner = delimited.data();
      assert_eq!(inner.span(), SimpleSpan::new(1, 4));
      assert_eq!(inner.open_ref().span(), &SimpleSpan::new(1, 2));
      assert_eq!(inner.close_ref().span(), &SimpleSpan::new(3, 4));
      assert_eq!(*inner.data().source_ref(), "x");
      Ok::<_, E>(())
    },
    "{[x]}",
  );
  assert!(out.is_ok());
}

// ── The try_* attempt twins ──────────────────────────────────────────────────
//
// The law: the attempt form declines — `Ok(None)`, zero consumption — iff the opener is
// absent (wrong token or end of input at entry). The moment the opener is consumed the
// parse is committed: inner and closer errors propagate exactly as the committed form's,
// so an unterminated group is an `Err`, never a silent decline. Each shape pins (a) the
// decline-and-leave path — the next committed parse still sees the token — plus the
// EOT-at-entry decline, and (b) the law path: opener present + unterminated → `Err`,
// NOT `None`.

#[test]
fn try_parens_declines_on_wrong_opener_and_leaves_it() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = try_parens(Ident::parse)(inp)?.is_none();
      // The `{` group is untouched: the committed brace shape parses it whole.
      let d = braces(Ident::parse)(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "{x}",
  );
  assert!(matches!(out, Ok(true)));
  // End of input at entry declines too (the `try_` atoms' EOT convention).
  let out = drive(Fatal::<E>::new(), |inp| try_parens(Ident::parse)(inp), "");
  assert!(matches!(out, Ok(None)));
}

#[test]
fn try_parens_after_opener_is_committed_unterminated_never_declines() {
  // Fatal: committed-then-unterminated reports Unclosed as an `Err`.
  let out = drive(Fatal::<E>::new(), |inp| try_parens(Ident::parse)(inp), "(a");
  assert!(out.is_err());
  // Verbose: it records the Unclosed and recovers with the construct — NOT a silent decline.
  let out = drive(
    Verbose::<E>::new(),
    |inp| try_parens(Ident::parse)(inp),
    "(a",
  );
  assert!(
    matches!(out, Ok(Some(_))),
    "committed shape recovers under a recovering emitter, never Ok(None)"
  );
}

#[test]
fn try_braces_declines_on_wrong_opener_and_leaves_it() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = try_braces(Ident::parse)(inp)?.is_none();
      // The `(` group is untouched: the committed paren shape parses it whole.
      let d = parens(Ident::parse)(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "(x)",
  );
  assert!(matches!(out, Ok(true)));
  let out = drive(Fatal::<E>::new(), |inp| try_braces(Ident::parse)(inp), "");
  assert!(matches!(out, Ok(None)));
}

#[test]
fn try_braces_after_opener_is_committed_unterminated_never_declines() {
  let out = drive(Fatal::<E>::new(), |inp| try_braces(Ident::parse)(inp), "{a");
  assert!(out.is_err());
  let out = drive(
    Verbose::<E>::new(),
    |inp| try_braces(Ident::parse)(inp),
    "{a",
  );
  assert!(
    matches!(out, Ok(Some(_))),
    "committed shape recovers under a recovering emitter, never Ok(None)"
  );
}

#[test]
fn try_brackets_declines_on_wrong_opener_and_leaves_it() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = try_brackets(Ident::parse)(inp)?.is_none();
      // The `{` group is untouched: the committed brace shape parses it whole.
      let d = braces(Ident::parse)(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "{x}",
  );
  assert!(matches!(out, Ok(true)));
  let out = drive(Fatal::<E>::new(), |inp| try_brackets(Ident::parse)(inp), "");
  assert!(matches!(out, Ok(None)));
}

#[test]
fn try_brackets_after_opener_is_committed_unterminated_never_declines() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| try_brackets(Ident::parse)(inp),
    "[a",
  );
  assert!(out.is_err());
  let out = drive(
    Verbose::<E>::new(),
    |inp| try_brackets(Ident::parse)(inp),
    "[a",
  );
  assert!(
    matches!(out, Ok(Some(_))),
    "committed shape recovers under a recovering emitter, never Ok(None)"
  );
}

#[test]
fn try_angles_declines_on_wrong_opener_and_leaves_it() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = try_angles(Ident::parse)(inp)?.is_none();
      // The `(` group is untouched: the committed paren shape parses it whole.
      let d = parens(Ident::parse)(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "(x)",
  );
  assert!(matches!(out, Ok(true)));
  let out = drive(Fatal::<E>::new(), |inp| try_angles(Ident::parse)(inp), "");
  assert!(matches!(out, Ok(None)));
}

#[test]
fn try_angles_after_opener_is_committed_unterminated_never_declines() {
  let out = drive(Fatal::<E>::new(), |inp| try_angles(Ident::parse)(inp), "<a");
  assert!(out.is_err());
  let out = drive(
    Verbose::<E>::new(),
    |inp| try_angles(Ident::parse)(inp),
    "<a",
  );
  assert!(
    matches!(out, Ok(Some(_))),
    "committed shape recovers under a recovering emitter, never Ok(None)"
  );
}

#[test]
fn try_delimited_generic_declines_on_wrong_opener_and_leaves_it() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = try_delimited::<Brace, _, _, _, _, _, _>(Ident::parse)(inp)?.is_none();
      // The `(` group is untouched: the committed paren shape parses it whole.
      let d = parens(Ident::parse)(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "(x)",
  );
  assert!(matches!(out, Ok(true)));
  let out = drive(
    Fatal::<E>::new(),
    |inp| try_delimited::<Brace, _, _, _, _, _, _>(Ident::parse)(inp),
    "",
  );
  assert!(matches!(out, Ok(None)));
}

#[test]
fn try_delimited_generic_after_opener_is_committed_unterminated_never_declines() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| try_delimited::<Paren, _, _, _, _, _, _>(Ident::parse)(inp),
    "(a",
  );
  assert!(out.is_err());
  let out = drive(
    Verbose::<E>::new(),
    |inp| try_delimited::<Paren, _, _, _, _, _, _>(Ident::parse)(inp),
    "(a",
  );
  assert!(
    matches!(out, Ok(Some(_))),
    "committed shape recovers under a recovering emitter, never Ok(None)"
  );
}

// ── #115: the fluent attempt twins on `ParseInput` ───────────────────────────
//
// `try_delimited::<D>()` plus the four named methods. Each is a delegation and holds no
// state machine of its own, so these re-drive the free-function cases above through a
// receiver: the decline-and-leave path, the EOT-at-entry decline, and the law that an
// opener commits. The last case runs both surfaces over the same four inputs and asserts
// they agree, which is what makes "it delegates" observable rather than merely documented.

#[test]
fn try_delimited_by_parens_declines_on_wrong_opener_and_leaves_it() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = Ident::parse.try_delimited_by_parens()(inp)?.is_none();
      // The `{` group is untouched: the committed brace shape parses it whole.
      let d = Ident::parse.delimited_by_braces().parse_input(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "{x}",
  );
  assert!(matches!(out, Ok(true)));
  // End of input at entry declines too (the `try_` atoms' EOT convention).
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_parens()(inp),
    "",
  );
  assert!(matches!(out, Ok(None)));
}

#[test]
fn try_delimited_by_parens_accepts_and_spans_the_whole_group() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let d = Ident::parse.try_delimited_by_parens()(inp)?
        .expect("the opener is present, so the attempt accepts");
      assert_eq!(*d.data().source_ref(), "x");
      assert_eq!(d.data().span(), SimpleSpan::new(1, 2));
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      assert_eq!(d.open_ref().span(), &SimpleSpan::new(0, 1));
      assert_eq!(d.close_ref().span(), &SimpleSpan::new(2, 3));
      Ok::<_, E>(())
    },
    "(x)",
  );
  assert!(out.is_ok());
}

#[test]
fn try_delimited_by_parens_after_opener_is_committed_unterminated_never_declines() {
  // Fatal: committed-then-unterminated reports Unclosed as an `Err`.
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_parens()(inp),
    "(a",
  );
  assert!(out.is_err());
  // Verbose: it records the Unclosed and recovers with the construct — NOT a silent decline.
  let out = drive(
    Verbose::<E>::new(),
    |inp| Ident::parse.try_delimited_by_parens()(inp),
    "(a",
  );
  assert!(
    matches!(out, Ok(Some(_))),
    "committed shape recovers under a recovering emitter, never Ok(None)"
  );
}

#[test]
fn try_delimited_by_braces_declines_and_commits_like_its_free_twin() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = Ident::parse.try_delimited_by_braces()(inp)?.is_none();
      let d = Ident::parse.delimited_by_parens().parse_input(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "(x)",
  );
  assert!(matches!(out, Ok(true)));
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_braces()(inp),
    "",
  );
  assert!(matches!(out, Ok(None)));
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_braces()(inp),
    "{a",
  );
  assert!(out.is_err());
}

#[test]
fn try_delimited_by_brackets_declines_and_commits_like_its_free_twin() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = Ident::parse.try_delimited_by_brackets()(inp)?.is_none();
      let d = Ident::parse.delimited_by_braces().parse_input(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "{x}",
  );
  assert!(matches!(out, Ok(true)));
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_brackets()(inp),
    "",
  );
  assert!(matches!(out, Ok(None)));
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_brackets()(inp),
    "[a",
  );
  assert!(out.is_err());
}

#[test]
fn try_delimited_by_angles_declines_and_commits_like_its_free_twin() {
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = Ident::parse.try_delimited_by_angles()(inp)?.is_none();
      let d = Ident::parse.delimited_by_parens().parse_input(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "(x)",
  );
  assert!(matches!(out, Ok(true)));
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_angles()(inp),
    "",
  );
  assert!(matches!(out, Ok(None)));
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited_by_angles()(inp),
    "<a",
  );
  assert!(out.is_err());
}

#[test]
fn try_delimited_method_generic_declines_commits_and_nests() {
  // Wrong opener: decline, leave it, and let the committed shape take the group.
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      let declined = Ident::parse
        .try_delimited::<Brace>()
        .parse_input(inp)?
        .is_none();
      let d = parens(Ident::parse)(inp)?;
      assert_eq!(d.span(), SimpleSpan::new(0, 3));
      Ok::<_, E>(declined)
    },
    "(x)",
  );
  assert!(matches!(out, Ok(true)));
  // Committed after the opener.
  let out = drive(
    Fatal::<E>::new(),
    |inp| Ident::parse.try_delimited::<Paren>().parse_input(inp),
    "(a",
  );
  assert!(out.is_err());
  // The receiver may itself be a recursive generic parser, as the committed twin's is.
  let out = drive(
    Fatal::<E>::new(),
    |inp| {
      nested
        .try_delimited::<Bracket>()
        .parse_input(inp)
        .map(|d| d.map(|d| Nested::List(Box::new(d.into_data()))))
    },
    "[[x]]",
  )
  .unwrap();
  assert_eq!(
    out,
    Some(Nested::List(Box::new(Nested::List(Box::new(
      Nested::Name("x")
    )))))
  );
}

#[test]
fn try_delimited_by_parens_agrees_with_the_free_function_it_delegates_to() {
  // The delegation target is the GENERIC free `try_delimited::<Paren>`, not the named
  // `try_parens` — the named free fn carries its own opener body, so only the
  // generic one is the same code path. Four inputs cover accept, decline-on-wrong-opener,
  // decline-at-EOT, and the committed-unterminated law.
  for src in ["(x)", "{x}", "", "(a"] {
    let via_method = drive(
      Fatal::<E>::new(),
      |inp| {
        Ok::<_, E>(
          Ident::parse.try_delimited_by_parens()(inp)?.map(|d| (d.span(), *d.data().source_ref())),
        )
      },
      src,
    );
    let via_free = drive(
      Fatal::<E>::new(),
      |inp| {
        Ok::<_, E>(
          try_delimited::<Paren, _, _, _, _, _, _>(Ident::parse)(inp)?
            .map(|d| (d.span(), *d.data().source_ref())),
        )
      },
      src,
    );
    match (via_method, via_free) {
      (Ok(m), Ok(f)) => assert_eq!(m, f, "method and free form disagreed on {src:?}"),
      (Err(_), Err(_)) => {}
      (m, f) => panic!("method/free disagreed on {src:?}: {m:?} vs {f:?}"),
    }
  }
}

// ── Local discriminating error for the generic error-path tests ──────────────
//
// `ShapeError` mirrors the shared `E`'s absorb-everything `From` family (so it is a
// `ComposableEmitter` error and backs a `FatalContext`), but keeps the two token-level
// families the shapes commit through as distinct variants, so a test can pin WHICH one a
// missing/wrong closer produced. The shared `E` is unit and cannot.
#[derive(Debug)]
enum ShapeError {
  /// An unexpected token, carrying the single expected kind (the delimiter).
  Unexpected(Option<TokenKind>),
  /// End of input where a delimiter was required.
  Eot,
  /// An unclosed delimiter, carrying the opener's name and start offset.
  Unclosed { name: String, start: usize },
  /// Any other diagnostic family (unexercised by these tests).
  Other,
}

impl From<()> for ShapeError {
  fn from(_: ()) -> Self {
    ShapeError::Other
  }
}

impl<D, Lang: ?Sized> From<Unclosed<D, SimpleSpan, Lang>> for ShapeError {
  fn from(e: Unclosed<D, SimpleSpan, Lang>) -> Self {
    ShapeError::Unclosed {
      name: e.name_ref().to_string(),
      start: e.span().start(),
    }
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for ShapeError
where
  L: tokora::Lexer<'inp, Span = SimpleSpan>,
{
  fn from_unclosed<D>(e: Unclosed<D, SimpleSpan, Lang>) -> Self {
    ShapeError::Unclosed {
      name: e.name_ref().to_string(),
      start: e.span().start(),
    }
  }
}

impl<'a, S, Lang: ?Sized> From<UnexpectedToken<'a, Token, TokenKind, S, Lang>> for ShapeError {
  fn from(e: UnexpectedToken<'a, Token, TokenKind, S, Lang>) -> Self {
    let kind = match e.expected() {
      Some(Expected::One(k)) => Some(*k),
      _ => None,
    };
    ShapeError::Unexpected(kind)
  }
}

impl<Set: Clone + 'static> From<UnexpectedEot<usize, (), Set>> for ShapeError {
  fn from(_: UnexpectedEot<usize, (), Set>) -> Self {
    ShapeError::Eot
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for ShapeError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    ShapeError::Other
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for ShapeError {
  fn from(_: TooFew<S, Lang>) -> Self {
    ShapeError::Other
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for ShapeError {
  fn from(_: TooMany<S, Lang>) -> Self {
    ShapeError::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for ShapeError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    ShapeError::Other
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>>
  for ShapeError
{
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    ShapeError::Other
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for ShapeError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    ShapeError::Other
  }
}
