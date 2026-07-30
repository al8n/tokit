#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

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
//! `RepeatedHandler::on_element` (the hook with the defect) is consumed by exactly two sources,
//! pinned by `many/mod.rs`'s `MID_LOOP_PAIRING_CENSUS`: `many/repeated/mod.rs` (already correct —
//! parses first, since `try_parse_input`'s `Ok(Accept(item))` arm only matches a completed parse)
//! and `many/repeated_while/mod.rs` (the defect fixed here). No other driver calls
//! `RepeatedHandler::on_element` at all:
//!
//! - The delimited wrappers (`many/delim/repeated.rs`, `many/delim/repeated_while.rs`) have no
//!   mid-loop maximum hook whatsoever — their `on_stop` closures (`delim/repeated{,_while}/{at_most,
//!   bounded}.rs`) check `nums > max` exactly once, from the `FnOnce` closure invoked only at the
//!   driver's genuine end states (a found closer, a `Stop` decision with no closer, or the
//!   no-progress stall's close probe). A failed element parse propagates its `Err` immediately
//!   through `?` and never reaches that closure, so no premature `TooMany` is possible. This file's
//!   `delimited_at_most_boundary_continue_then_element_error_records_no_too_many` test exercises
//!   this directly.
//! - The separated drivers (`many/sep/parse/mod.rs`, `many/sep/delim/mod.rs`,
//!   `many/sep_while/parse/mod.rs`, `many/sep_while/delim/mod.rs`) enforce the maximum through
//!   `EndStateHandler` (`Maximum`/`Bounded`'s `handle_*_state`, which delegate to `Maximum::check`
//!   / `With<Minimum, Maximum>::check` in `parser/with.rs`), invoked only via each driver's
//!   `handle_end`/`parser.handle_end`, and only from genuine end states — never speculatively ahead
//!   of an unattempted element. `ContinueStateHandler::handle_start_state` (called before the
//!   element parse in `sep_while`'s `State::Start` arm) is a hard-coded no-op for both `Maximum`
//!   and `Bounded`, so even though its *call site* sits ahead of the parse, it can never emit
//!   `TooMany`; the maximum enforcement lives entirely in the end-state check.
//! - The `fold` family has no maximum/bounds concept at all (no `TooMany`, no `RepeatedHandler`,
//!   no `EndStateHandler`), so it is not applicable.
//!
//! An exhaustive `grep` for every `emit_too_many(TooMany::of(` call site in the crate turns up
//! exactly the eight sites `many/mod.rs`'s own `every_too_many_payload_exceeds_its_limit` census
//! names: the two end checks in `with.rs`, the two mid-loop `on_element` hooks (`maximum.rs`,
//! `bounded.rs`), and the four delimited `on_stop` closures — confirming there is no ninth,
//! unaccounted-for site.

mod common;

use common::{TestLexer, Token};
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
  _: &mut Ctx::Emitter,
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
  _: &mut Ctx::Emitter,
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

    let recorded: Vec<Diag> = inp.emitter().errors().values().flatten().cloned().collect();
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

    let recorded: Vec<Diag> = inp.emitter().errors().values().flatten().cloned().collect();
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

    let recorded: Vec<Diag> = inp.emitter().errors().values().flatten().cloned().collect();
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

    let recorded: Vec<Diag> = inp.emitter().errors().values().flatten().cloned().collect();
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
// Sibling confirmation — the delimited `repeated_while` named alongside the defect. It has no
// mid-loop maximum hook at all (its `on_stop` closure runs only at a genuine end state), so this
// is not a RED/GREEN pair: it must already pass, both before and after the fix, and stays here as
// executable proof rather than a claim resting on code reading alone.
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

    let recorded: Vec<Diag> = inp.emitter().errors().values().flatten().cloned().collect();
    assert!(
      !recorded.iter().any(|d| matches!(d, Diag::TooMany(..))),
      "the delimited driver has no mid-loop hook to misfire; unexpected TooMany: {recorded:?}"
    );
    Ok(())
  }

  Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("[x]")
    .unwrap();
}
