#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(clippy::type_complexity)]
//! Regression suite for the delimited-SHAPE unclosed fix.
//!
//! The committed shape parsers (`delimited::<D>`/`parens`/`braces`/`brackets`/`angles` and
//! their `try_` twins) used to raise a plain unexpected-token / end-of-input error on a
//! missing closer. They now report the opener as [`Unclosed`] **through the emitter** — the
//! same four-way close-miss law the delimited many-builders followed since #73:
//!
//! - a fail-fast [`Fatal`] emitter converts the emission to `Err` (carrying the opener's span
//!   and the delimiter pair's name);
//! - a recovering [`Verbose`] emitter records the diagnostic and the shape recovers, yielding
//!   the construct with a closer synthesized at the insertion point;
//! - a wrong token where the closer belongs stays the unexpected-token (expected-close)
//!   diagnostic, **not** `Unclosed`;
//! - a terminated group is unaffected.
//!
//! The `try_` twins keep their decline law (absent opener ⇒ `Ok(None)`, zero consumption) and
//! inherit the close-miss law once the opener is committed (the #69/A7 opt-vs-committed
//! distinction): committed-then-unterminated reports `Unclosed`, it never silently declines.

mod common;

use common::{TestLexer, Token};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, SimpleSpan,
  emitter::{Fatal, Verbose},
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::{angles, braces, brackets, delimited, parens, try_parens},
  punct::Paren,
};

// ── A rich error type that preserves the `Unclosed` payload ────────────────────
//
// Unlike the shared unit `E`, this captures whether a diagnostic came from `Unclosed` (and
// the opener name + start offset it carries) so the assertions can prove *what* was emitted,
// not merely *that* something was.

#[derive(Debug, Clone, PartialEq)]
enum SE {
  Unclosed { name: String, start: usize },
  Other,
}

impl<D, Lang: ?Sized> From<Unclosed<D, SimpleSpan, Lang>> for SE {
  fn from(err: Unclosed<D, SimpleSpan, Lang>) -> Self {
    SE::Unclosed {
      name: err.name_ref().to_string(),
      start: err.span().start(),
    }
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for SE
where
  L: tokora::Lexer<'inp, Span = SimpleSpan>,
{
  fn from_unclosed<D>(err: Unclosed<D, SimpleSpan, Lang>) -> Self {
    SE::Unclosed {
      name: err.name_ref().to_string(),
      start: err.span().start(),
    }
  }
}

impl From<()> for SE {
  fn from(_: ()) -> Self {
    SE::Other
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for SE {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    SE::Other
  }
}
impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for SE {
  fn from(_: FullContainer<S, Lang>) -> Self {
    SE::Other
  }
}
impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for SE {
  fn from(_: TooFew<S, Lang>) -> Self {
    SE::Other
  }
}
impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for SE {
  fn from(_: TooMany<S, Lang>) -> Self {
    SE::Other
  }
}
impl<Set: Clone + 'static> From<UnexpectedEot<usize, (), Set>> for SE {
  fn from(_: UnexpectedEot<usize, (), Set>) -> Self {
    SE::Other
  }
}
impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for SE {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    SE::Other
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for SE {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    SE::Other
  }
}
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for SE {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    SE::Other
  }
}

type FatalCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Fatal<SE>>;
type VerboseCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<SE>>;
// The blackhole cache `()` — zero capacity, so `probe_close` cannot cache the wrong token and
// `inp.cursor()` never advances past it. Pins the recovery span's cache-independence.
type NoCacheVerboseCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<SE>, ()>;

/// Drives a shape parser under a fail-fast `Fatal<SE>` context. The concrete context in the
/// closure's `inp` type pins inference for the generic inner sub-parser.
fn drive_fatal<'inp, O>(
  f: impl for<'c> FnMut(&mut InputRef<'inp, 'c, TestLexer<'inp>, FatalCtx<'inp>>) -> Result<O, SE>,
  input: &'inp str,
) -> Result<O, SE> {
  let ctx: FatalCtx<'inp> = ParserContext::new(Fatal::new());
  Parser::with_parser_and_context(f, ctx).parse_str(input)
}

fn verbose_ctx() -> VerboseCtx<'static> {
  ParserContext::new(Verbose::new())
}

fn no_cache_verbose_ctx() -> NoCacheVerboseCtx<'static> {
  ParserContext::new(Verbose::new())
}

/// Pulls the recorded `Unclosed` diagnostics (name, start) out of a `Verbose<SE>` sink, in
/// span order.
fn recorded_unclosed(em: &Verbose<SE>) -> Vec<(String, usize)> {
  em.errors()
    .values()
    .flatten()
    .filter_map(|e| match e {
      SE::Unclosed { name, start } => Some((name.clone(), *start)),
      SE::Other => None,
    })
    .collect()
}

/// The shapes' inner sub-parser: a single `Num`.
fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, SE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SE>,
{
  match inp.next()? {
    None => Err(SE::Other),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(SE::Other),
    },
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// The full pair × emitter matrix, generated per named shape. `$open`/`$close`/`$wrong`
// are single ASCII chars; the inner is one digit, so the terminated construct spans
// `[0,3)`, the unterminated one `[0,2)`, and the synthesized closer sits zero-width at
// offset 2 (the insertion point).
// ═══════════════════════════════════════════════════════════════════════════════

macro_rules! shape_matrix {
  ($shape:ident, $name:literal, $open:literal, $close:literal, $wrong:literal) => {
    mod $shape {
      use super::*;

      // Unterminated committed shape under a fail-fast emitter ⇒ Err carrying `Unclosed`
      // anchored at the opener (assert the VARIANT, not `is_err`).
      #[test]
      fn fatal_unterminated_is_unclosed() {
        let r = drive_fatal(|inp| $shape(parse_num)(inp), concat!($open, "1"));
        assert_eq!(
          r.map(|d| *d.data()),
          Err(SE::Unclosed {
            name: $name.to_string(),
            start: 0
          }),
          "fatal: unterminated shape must Err with Unclosed at the opener"
        );
      }

      // Unterminated committed shape under a recovering emitter ⇒ the diagnostic is recorded
      // and the shape recovers with the inner data and a synthesized closer.
      #[test]
      fn verbose_unterminated_records_and_recovers() {
        fn go<'inp>(
          inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
        ) -> Result<(i64, SimpleSpan, SimpleSpan, Vec<(String, usize)>), SE> {
          let d = $shape(parse_num)(inp)?;
          Ok((
            *d.data(),
            d.span(),
            *d.close_ref().span(),
            recorded_unclosed(inp.emitter_ref()),
          ))
        }
        let (data, span, close_span, diags) = Parser::with_context(verbose_ctx())
          .apply(go)
          .parse_str(concat!($open, "1"))
          .unwrap();
        assert_eq!(data, 1, "verbose: recovery yields the inner data");
        assert_eq!(
          span,
          SimpleSpan::new(0, 2),
          "the construct spans the opener + inner"
        );
        assert_eq!(
          close_span,
          SimpleSpan::new(2, 2),
          "the synthesized closer is zero-width at the insertion point"
        );
        assert_eq!(
          diags,
          vec![($name.to_string(), 0)],
          "verbose: records exactly one Unclosed at the opener"
        );
      }

      // A wrong token where the closer belongs is the unexpected-token (expected-close)
      // diagnostic, NOT Unclosed — unchanged from before the fix.
      #[test]
      fn wrong_close_is_unexpected_token_not_unclosed() {
        let r = drive_fatal(|inp| $shape(parse_num)(inp), concat!($open, "1", $wrong));
        assert_eq!(
          r.map(|d| *d.data()),
          Err(SE::Other),
          "wrong closer must be unexpected-token, not Unclosed"
        );
      }

      // A terminated construct is unaffected.
      #[test]
      fn terminated_is_ok() {
        let r = drive_fatal(|inp| $shape(parse_num)(inp), concat!($open, "1", $close));
        assert_eq!(r.map(|d| *d.data()), Ok(1));
      }

      // Recovery span invariant with trivia BEFORE a wrong closer: `probe_close` caches the
      // wrong token past the space, so the recovered shape's span ends at that token's start;
      // the synthesized closer must end there too — `close.span().end() == shape.span().end()`
      // — never at the stale pre-trivia offset.
      #[test]
      fn verbose_wrong_close_with_trivia_span_invariant() {
        fn go<'inp>(
          inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
        ) -> Result<(SimpleSpan, SimpleSpan), SE> {
          let d = $shape(parse_num)(inp)?;
          Ok((d.span(), *d.close_ref().span()))
        }
        let (span, close_span) = Parser::with_context(verbose_ctx())
          .apply(go)
          .parse_str(concat!($open, "1 ", $wrong))
          .unwrap();
        assert!(
          span.start() <= span.end(),
          "overall span is well-formed (not reversed)"
        );
        assert_eq!(
          close_span.end(),
          span.end(),
          "synthesized closer must end where the recovered shape ends"
        );
        assert_eq!(
          span.end(),
          3,
          "shape ends at the wrong token's start, past the trivia"
        );
        assert_eq!(
          close_span,
          SimpleSpan::new(3, 3),
          "zero-width closer at the insertion point"
        );
      }

      // The same wrong-closer-with-trivia case under the blackhole cache `()`. `probe_close`
      // leaves the wrong token at the FRONT OF THE STREAM whatever the capacity — the parked slot
      // when the cache refuses it — so `inp.cursor()` is that token's start (offset 3) here just
      // as it is under a retaining cache. The recovery spans the shape via `span_since(cursor)`,
      // so close.end() == shape.end() == the cursor, and the two capacities agree exactly.
      #[test]
      fn no_cache_verbose_wrong_close_span_invariant() {
        fn go<'inp>(
          inp: &mut InputRef<'inp, '_, TestLexer<'inp>, NoCacheVerboseCtx<'inp>>,
        ) -> Result<(SimpleSpan, SimpleSpan), SE> {
          let d = $shape(parse_num)(inp)?;
          Ok((d.span(), *d.close_ref().span()))
        }
        let (span, close_span) = Parser::with_context(no_cache_verbose_ctx())
          .apply(go)
          .parse_str(concat!($open, "1 ", $wrong))
          .unwrap();
        assert_eq!(
          close_span.end(),
          span.end(),
          "close ends where the shape ends"
        );
        assert_eq!(
          span.end(),
          3,
          "ends at the wrong token's start, past the trivia — the same as any other capacity"
        );
        assert_eq!(close_span, SimpleSpan::new(3, 3));
      }
    }
  };
}

shape_matrix!(parens, "()", "(", ")", "]");
shape_matrix!(braces, "{}", "{", "}", ")");
shape_matrix!(brackets, "[]", "[", "]", ")");
shape_matrix!(angles, "<>", "<", ">", ")");

// ═══════════════════════════════════════════════════════════════════════════════
// The generic `delimited::<Paren>` form — same law through the `TypedDelimiter` capability.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn generic_delimited_fatal_unterminated_is_unclosed() {
  let r = drive_fatal(
    |inp| delimited::<Paren, _, _, _, _, _, _>(parse_num)(inp),
    "(1",
  );
  assert_eq!(
    r.map(|d| *d.data()),
    Err(SE::Unclosed {
      name: "()".to_string(),
      start: 0
    })
  );
}

#[test]
fn generic_delimited_verbose_records_and_recovers() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<(i64, Vec<(String, usize)>), SE> {
    let d = delimited::<Paren, _, _, _, _, _, _>(parse_num)(inp)?;
    Ok((*d.data(), recorded_unclosed(inp.emitter_ref())))
  }
  let (data, diags) = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("(1")
    .unwrap();
  assert_eq!(data, 1);
  assert_eq!(diags, vec![("()".to_string(), 0)]);
}

#[test]
fn generic_delimited_wrong_close_is_unexpected_token() {
  let r = drive_fatal(
    |inp| delimited::<Paren, _, _, _, _, _, _>(parse_num)(inp),
    "(1]",
  );
  assert_eq!(r.map(|d| *d.data()), Err(SE::Other));
}

#[test]
fn generic_delimited_verbose_wrong_close_with_trivia_span_invariant() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<(SimpleSpan, SimpleSpan), SE> {
    let d = delimited::<Paren, _, _, _, _, _, _>(parse_num)(inp)?;
    Ok((d.span(), *d.close_ref().span()))
  }
  let (span, close_span) = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("(1 ]")
    .unwrap();
  assert_eq!(
    close_span.end(),
    span.end(),
    "synthesized closer must end where the recovered shape ends"
  );
  assert_eq!(close_span, SimpleSpan::new(3, 3));
}

// The generic path under the blackhole cache `()`: the recovered span ends at the wrong token's
// start (offset 3), exactly as it does under a retaining cache.
#[test]
fn generic_delimited_no_cache_wrong_close_span_invariant() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, NoCacheVerboseCtx<'inp>>,
  ) -> Result<(SimpleSpan, SimpleSpan), SE> {
    let d = delimited::<Paren, _, _, _, _, _, _>(parse_num)(inp)?;
    Ok((d.span(), *d.close_ref().span()))
  }
  let (span, close_span) = Parser::with_context(no_cache_verbose_ctx())
    .apply(go)
    .parse_str("(1 ]")
    .unwrap();
  assert_eq!(close_span.end(), span.end());
  assert_eq!(close_span, SimpleSpan::new(3, 3));
}

// Eof arm cache-independence: `(1` at EOF under the blackhole cache
// recovers with a zero-width closer at the committed frontier (offset 2), same as any cache —
// nothing is cached at EOF, so there is no cursor staleness.
#[test]
fn parens_no_cache_eof_recovers_zero_width_close() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, NoCacheVerboseCtx<'inp>>,
  ) -> Result<(SimpleSpan, SimpleSpan), SE> {
    let d = parens(parse_num)(inp)?;
    Ok((d.span(), *d.close_ref().span()))
  }
  let (span, close_span) = Parser::with_context(no_cache_verbose_ctx())
    .apply(go)
    .parse_str("(1")
    .unwrap();
  assert_eq!(close_span.end(), span.end());
  assert_eq!(span, SimpleSpan::new(0, 2));
  assert_eq!(close_span, SimpleSpan::new(2, 2));
}

// The try-twin path: committed-then-wrong-closer recovers under the blackhole cache and the
// synthesized closer ends at the wrong token's start (offset 3), matching every other capacity.
#[test]
fn try_parens_no_cache_committed_wrong_close_span_invariant() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, NoCacheVerboseCtx<'inp>>,
  ) -> Result<(SimpleSpan, SimpleSpan), SE> {
    let d = try_parens(parse_num)(inp)?.expect("committed shape recovers, not a decline");
    Ok((d.span(), *d.close_ref().span()))
  }
  let (span, close_span) = Parser::with_context(no_cache_verbose_ctx())
    .apply(go)
    .parse_str("(1 ]")
    .unwrap();
  assert_eq!(close_span.end(), span.end());
  assert_eq!(close_span, SimpleSpan::new(3, 3));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Enclosing-parent containment: a recovered shape parsed INSIDE an enclosing parser
// under the blackhole cache `()` must never outrun the enclosing cursor. Both the child and the
// parent span via `span_since(cursor)`, so parent ⊇ child and close.end() == shape.end() == cursor
// for the generic, named, and try_ forms alike — and the cursor is the wrong token's start in
// every capacity, because a probe's `WrongToken` is retained at the front of the stream.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn generic_delimited_no_cache_enclosing_parent_contains_recovered_shape() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, NoCacheVerboseCtx<'inp>>,
  ) -> Result<(SimpleSpan, SimpleSpan, SimpleSpan), SE> {
    let outer = *inp.cursor();
    let d = delimited::<Paren, _, _, _, _, _, _>(parse_num)(inp)?;
    let child = d.span();
    let close = *d.close_ref().span();
    let parent = inp.span_since(&outer);
    Ok((child, close, parent))
  }
  let (child, close, parent) = Parser::with_context(no_cache_verbose_ctx())
    .apply(go)
    .parse_str("(1 ]")
    .unwrap();
  assert!(
    parent.end() >= child.end(),
    "enclosing parent must contain the recovered child shape"
  );
  assert_eq!(child.end(), parent.end(), "shape ends at the live cursor");
  assert_eq!(close.end(), child.end(), "close ends where the shape ends");
  assert_eq!(
    child.end(),
    3,
    "child ends at the wrong token's start — the same in every capacity"
  );
  assert_eq!(close, SimpleSpan::new(3, 3));
}

#[test]
fn parens_no_cache_enclosing_parent_contains_recovered_shape() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, NoCacheVerboseCtx<'inp>>,
  ) -> Result<(SimpleSpan, SimpleSpan, SimpleSpan), SE> {
    let outer = *inp.cursor();
    let d = parens(parse_num)(inp)?;
    let child = d.span();
    let close = *d.close_ref().span();
    let parent = inp.span_since(&outer);
    Ok((child, close, parent))
  }
  let (child, close, parent) = Parser::with_context(no_cache_verbose_ctx())
    .apply(go)
    .parse_str("(1 ]")
    .unwrap();
  assert!(
    parent.end() >= child.end(),
    "enclosing parent must contain the recovered child shape"
  );
  assert_eq!(child.end(), parent.end(), "shape ends at the live cursor");
  assert_eq!(close.end(), child.end(), "close ends where the shape ends");
  assert_eq!(
    child.end(),
    3,
    "child ends at the wrong token's start — the same in every capacity"
  );
  assert_eq!(close, SimpleSpan::new(3, 3));
}

#[test]
fn try_parens_no_cache_enclosing_parent_contains_recovered_shape() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, NoCacheVerboseCtx<'inp>>,
  ) -> Result<(SimpleSpan, SimpleSpan, SimpleSpan), SE> {
    let outer = *inp.cursor();
    let d = try_parens(parse_num)(inp)?.expect("committed shape recovers, not a decline");
    let child = d.span();
    let close = *d.close_ref().span();
    let parent = inp.span_since(&outer);
    Ok((child, close, parent))
  }
  let (child, close, parent) = Parser::with_context(no_cache_verbose_ctx())
    .apply(go)
    .parse_str("(1 ]")
    .unwrap();
  assert!(
    parent.end() >= child.end(),
    "enclosing parent must contain the recovered child shape"
  );
  assert_eq!(child.end(), parent.end(), "shape ends at the live cursor");
  assert_eq!(close.end(), child.end(), "close ends where the shape ends");
  assert_eq!(
    child.end(),
    3,
    "child ends at the wrong token's start — the same in every capacity"
  );
  assert_eq!(close, SimpleSpan::new(3, 3));
}

// ═══════════════════════════════════════════════════════════════════════════════
// The `try_` twins: the decline law is unchanged (absent opener ⇒ `Ok(None)`, zero
// consumption); once the opener is committed they inherit the close-miss law
// (committed-then-unterminated ⇒ Unclosed, never a silent decline).
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn try_parens_declines_on_absent_opener_and_leaves_it() {
  // No `(` opener: decline, and the `1` is left for the next parse.
  let (declined, leftover) = drive_fatal(
    |inp| {
      let declined = try_parens(parse_num)(inp)?.is_none();
      let leftover = parse_num(inp)?;
      Ok((declined, leftover))
    },
    "1",
  )
  .unwrap();
  assert!(declined, "absent opener ⇒ decline");
  assert_eq!(leftover, 1, "the token stays unconsumed for the next parse");
}

#[test]
fn try_parens_committed_unterminated_fatal_is_unclosed() {
  let r = drive_fatal(|inp| try_parens(parse_num)(inp), "(1");
  assert_eq!(
    r.map(|o| o.map(|d| *d.data())),
    Err(SE::Unclosed {
      name: "()".to_string(),
      start: 0
    }),
    "committed-then-unterminated must Err with Unclosed, never decline"
  );
}

#[test]
fn try_parens_committed_unterminated_verbose_records_and_recovers() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<(Option<i64>, Vec<(String, usize)>), SE> {
    let d = try_parens(parse_num)(inp)?;
    Ok((d.map(|d| *d.data()), recorded_unclosed(inp.emitter_ref())))
  }
  let (data, diags) = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("(1")
    .unwrap();
  assert_eq!(
    data,
    Some(1),
    "verbose: committed shape recovers, not a decline"
  );
  assert_eq!(diags, vec![("()".to_string(), 0)]);
}
