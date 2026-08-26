#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Regression coverage for the `RepeatedWhile::parse` maximum-hook ordering defect.
//!
//! Container accounting counts **parsed** elements (an element counts
//! only once it has actually, successfully parsed). `RepeatedWhile::parse`'s `Continue` arm broke
//! that convention: it called the `RepeatedHandler` maximum hook (`rh.on_element`, which fires
//! `TooMany` when the pre-element count equals `max`) *before* attempting
//! `self.f.parse_input(inp)`. If the decision says `Continue` at the boundary and the element
//! parser then fails, the hook has already fired for an element that was never parsed:
//! - under a fail-fast emitter, the resulting `TooMany` preempts the real element error, masking
//!   the actual syntax error the caller needed to see;
//! - under a recovering emitter, a `TooMany` is recorded even though only `max` elements were
//!   ever successfully parsed.
//!
//! The fix moves the hook to *after* a successful `self.f.parse_input(inp)` and before the push,
//! matching the try-driven `Repeated::parse`, which already calls `rh.on_element` only once
//! `try_parse_input` has returned `Ok(Accept(item))`.
//!
//! # Driver enumeration
//!
//! Every repetition driver now admits an element through `many::admit_element`, which runs the
//! count handler's `on_element` hook and *then* offers the element to the destination. So the
//! hook's placement is one function's, not each driver's, and there are exactly twelve admission
//! sites across the eight collection drivers — `many/mod.rs`'s `ELEMENT_ADMISSION_CENSUS` counts
//! them and pins that no driver spells `on_element` or `container.push` itself.
//!
//! That is what makes the defect fixed here unrepeatable rather than merely fixed: the hook can
//! no longer run for an element that was not parsed, because the only thing that runs it is the
//! admission of a parsed element. In this file's terms —
//!
//! - `many/repeated/mod.rs` reaches the admission from `try_parse_input`'s `Ok(Accept(item))`
//!   arm, which only matches a completed parse;
//! - `many/repeated_while/mod.rs` reaches it after `self.f.parse_input(inp)?`, which is the fix
//!   this file covers;
//! - the two delimited-repeated engines reach it from the same two shapes, and their cardinality
//!   wrappers hand the count handler down instead of re-checking `nums > max` from an end
//!   closure (that end check is what made this family's diagnostic order differ, #277). A failed
//!   element parse propagates its `Err` through `?` and never reaches the admission, so no
//!   premature `TooMany` is possible. This file's
//!   `delimited_at_most_boundary_continue_then_element_error_records_no_too_many` exercises that
//!   directly;
//! - the four separated drivers reach it from the two `handle_continue` bodies, in every state
//!   arm, always after the element exists. `ContinueStateHandler::handle_start_state` (called
//!   before the element parse in `sep_while`'s `State::Start` arm) is a hard-coded no-op for
//!   every cardinality, so even though its *call site* sits ahead of the parse it can never emit;
//! - the `fold` family has no cardinality concept at all (no `TooMany`, no container), so it is
//!   not applicable.
//!
//! `emit_too_many(TooMany::of(` now has exactly **two** call sites in the crate, both
//! `ElementCountHandler::on_element` (`handler/maximum.rs`, `handler/bounded.rs`);
//! `every_too_many_payload_exceeds_its_limit` pins that count and scans the six sources that used
//! to hold the others to prove none came back.

mod common;

use common::{TestLexer, Token};
use tokora::EmitterView;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::Peeked,
  emitter::{Fatal, Verbose},
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, TooFew, TooMany},
    token::UnexpectedToken,
  },
  parser::Action,
  punct::Bracket,
  utils::typenum::U1,
};

// ── The payload-preserving diagnostic ─────────────────────────────────────────
//
// A local error type that (unlike this file's alternative of a unit "any failure" error) keeps
// enough of each diagnostic's payload to tell "the real element error surfaced" apart from "a
// `TooMany` masked it" — the exact distinction this defect blurs.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Diag {
  TooMany(usize, usize),
  TooFew(usize, usize),
  Full(usize, usize),
  UnexpectedToken,
  Unclosed,
  Eot,
  /// The element parser's own failure: the next token is not a number. This is the diagnostic
  /// that must surface — never masked, never preceded by a spurious `TooMany`.
  Lex,
}

impl From<()> for Diag {
  fn from(_: ()) -> Self {
    Diag::Lex
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for Diag {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Diag::UnexpectedToken
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Diag {
  fn from(e: FullContainer<S, Lang>) -> Self {
    Diag::Full(e.nums(), e.capacity())
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Diag {
  fn from(e: TooFew<S, Lang>) -> Self {
    Diag::TooFew(e.nums(), e.limit())
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Diag {
  fn from(e: TooMany<S, Lang>) -> Self {
    Diag::TooMany(e.nums(), e.limit())
  }
}

impl From<UnexpectedEot> for Diag {
  fn from(_: UnexpectedEot) -> Self {
    Diag::Eot
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unclosed<Delimiter, S, Lang>> for Diag {
  fn from(_: Unclosed<Delimiter, S, Lang>) -> Self {
    Diag::Unclosed
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Diag
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<Delimiter>(_: Unclosed<Delimiter, L::Span, Lang>) -> Self {
    Diag::Unclosed
  }
}

// ── Fixtures ────────────────────────────────────────────────────────────────

type VCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<Diag>>;
type FCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Fatal<Diag>>;

fn verbose_ctx() -> VCtx<'static> {
  ParserContext::new(Verbose::new())
}

fn fatal_ctx() -> FCtx<'static> {
  ParserContext::new(Fatal::new())
}

/// Continues for *any* present token, regardless of kind — deliberately decoupled from the
/// element parser's own success. This is what lets the decision answer `Continue` at the boundary
/// while the element parser goes on to fail: with `decide_num` (below), the two never diverge,
/// since both keyed off "is the next token a number", so the defect could never be observed.
fn decide_any<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
) -> Result<Action, Diag>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>,
{
  Ok(match peeked.pop_front() {
    None => Action::Stop,
    Some(_) => Action::Continue,
  })
}

/// Continues only while the next token is a number — the convention every other
/// `repeated_while` test in this crate uses, so a trailing sentinel cleanly stops the list. Used
/// by the keep-green pins, where every element genuinely parses.
fn decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
) -> Result<Action, Diag>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>,
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

/// The element parser: succeeds on a number, fails with `Diag::Lex` on anything else. The
/// boundary token in the RED tests below is a non-number (`x`), so this is the "real element
/// error" that must surface undisturbed by the maximum hook.
fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, Diag>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>,
{
  match inp.next()? {
    None => Err(Diag::Eot),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(Diag::Lex),
    },
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// `at_most(0)` — `Continue` at the boundary, the element parser errors.
//
// `nums` starts at 0, so the very first iteration already sits at the boundary
// (`nums == max == 0`). The lone input token `x` is present (so `decide_any` answers
// `Continue`) but is not a number (so `parse_num` fails). This is exactly the finding's
// concrete repro: "`at_most(0)` with a `Continue` decision and a failing element parser
// reports `TooMany(1, 0)` before parsing any element."
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_most_boundary_continue_then_element_error_records_no_too_many() {
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<(), Diag> {
    let result: Result<Vec<i64>, Diag> = parse_num
      .repeated_while::<_, U1>(decide_any::<VCtx<'inp>>)
      .at_most(0)
      .collect()
      .parse_input(inp);

    assert_eq!(
      result,
      Err(Diag::Lex),
      "the real element error must surface — no element was ever successfully parsed"
    );

    let recorded: Vec<Diag> = inp
      .emitter_ref()
      .errors()
      .values()
      .flatten()
      .cloned()
      .collect();
    assert_eq!(
      recorded,
      Vec::<Diag>::new(),
      "a recovering emitter must not record `TooMany` for an element that was never parsed"
    );
    Ok(())
  }

  Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("x +")
    .unwrap();
}

#[test]
fn at_most_boundary_continue_then_element_error_fatal_surfaces_real_error() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>,
  ) -> Result<Vec<i64>, Diag> {
    parse_num
      .repeated_while::<_, U1>(decide_any::<FCtx<'inp>>)
      .at_most(0)
      .collect()
      .parse_input(inp)
  }

  let got = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str("x +");
  assert_eq!(
    got,
    Err(Diag::Lex),
    "a fail-fast emitter must not let a premature `TooMany` preempt the real element error"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// `bounded(0, 0)` — the same shape through the `Bounded` (`With<Minimum, Maximum>`) hook,
// which shares `RepeatedWhile::parse`'s single call site with plain `Maximum`.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn bounded_boundary_continue_then_element_error_records_no_too_many() {
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<(), Diag> {
    let result: Result<Vec<i64>, Diag> = parse_num
      .repeated_while::<_, U1>(decide_any::<VCtx<'inp>>)
      .bounded(0, 0)
      .collect()
      .parse_input(inp);

    assert_eq!(
      result,
      Err(Diag::Lex),
      "the real element error must surface — no element was ever successfully parsed"
    );

    let recorded: Vec<Diag> = inp
      .emitter_ref()
      .errors()
      .values()
      .flatten()
      .cloned()
      .collect();
    assert_eq!(
      recorded,
      Vec::<Diag>::new(),
      "a recovering emitter must not record `TooMany` for an element that was never parsed"
    );
    Ok(())
  }

  Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("x +")
    .unwrap();
}

#[test]
fn bounded_boundary_continue_then_element_error_fatal_surfaces_real_error() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>,
  ) -> Result<Vec<i64>, Diag> {
    parse_num
      .repeated_while::<_, U1>(decide_any::<FCtx<'inp>>)
      .bounded(0, 0)
      .collect()
      .parse_input(inp)
  }

  let got = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str("x +");
  assert_eq!(
    got,
    Err(Diag::Lex),
    "a fail-fast emitter must not let a premature `TooMany` preempt the real element error"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Keep-green pins — a genuine over-limit (every element actually parses) must still report
// `TooMany` exactly once, with a count that exceeds the limit. The fix reorders the hook around a
// *failing* parse; it must not touch the payload or cardinality of the diagnostic on a
// successful one.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_most_genuine_overflow_still_reports_too_many_once() {
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<(), Diag> {
    let result: Result<Vec<i64>, Diag> = parse_num
      .repeated_while::<_, U1>(decide_num::<VCtx<'inp>>)
      .at_most(2)
      .collect()
      .parse_input(inp);
    assert_eq!(result, Ok(vec![1, 2, 3, 4]));

    let recorded: Vec<Diag> = inp
      .emitter_ref()
      .errors()
      .values()
      .flatten()
      .cloned()
      .collect();
    assert_eq!(
      recorded,
      vec![Diag::TooMany(3, 2)],
      "a genuine over-limit must still report TooMany exactly once, naming a count past the limit"
    );
    Ok(())
  }

  Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("1 2 3 4 +")
    .unwrap();
}

#[test]
fn bounded_genuine_overflow_still_reports_too_many_once() {
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<(), Diag> {
    let result: Result<Vec<i64>, Diag> = parse_num
      .repeated_while::<_, U1>(decide_num::<VCtx<'inp>>)
      .bounded(1, 2)
      .collect()
      .parse_input(inp);
    assert_eq!(result, Ok(vec![1, 2, 3, 4]));

    let recorded: Vec<Diag> = inp
      .emitter_ref()
      .errors()
      .values()
      .flatten()
      .cloned()
      .collect();
    assert_eq!(
      recorded,
      vec![Diag::TooMany(3, 2)],
      "a genuine over-limit must still report TooMany exactly once, naming a count past the \
       limit, with no spurious TooFew alongside it"
    );
    Ok(())
  }

  Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("1 2 3 4 +")
    .unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sibling confirmation — the delimited `repeated_while` named alongside the defect. Its maximum
// is reached only through `many::admit_element`, which an element that failed to parse never
// reaches, so this is not a RED/GREEN pair: it must pass before the fix and after it, and stays
// here as executable proof rather than a claim resting on code reading alone. (It held for a
// different reason before #277 — the family had no per-element hook at all and checked `nums >
// max` from an end closure — so the row is load-bearing across both designs.)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn delimited_at_most_boundary_continue_then_element_error_records_no_too_many() {
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<(), Diag> {
    let result: Result<Vec<i64>, Diag> = parse_num
      .repeated_while::<_, U1>(decide_any::<VCtx<'inp>>)
      .at_most(0)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp);

    assert_eq!(
      result,
      Err(Diag::Lex),
      "the real element error must surface — no element was ever successfully parsed"
    );

    let recorded: Vec<Diag> = inp
      .emitter_ref()
      .errors()
      .values()
      .flatten()
      .cloned()
      .collect();
    assert!(
      !recorded.iter().any(|d| matches!(d, Diag::TooMany(..))),
      "an element that never parsed is never admitted, so its count verdict never runs; \
       unexpected TooMany: {recorded:?}"
    );
    Ok(())
  }

  Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("[x]")
    .unwrap();
}
