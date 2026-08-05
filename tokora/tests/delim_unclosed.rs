#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(clippy::type_complexity)]
//! Regression suite for the unterminated-delimited-list fix.
//!
//! An unterminated delimited many-builder (`item…delimited::<D>().collect()`) used to accept
//! the input silently (returning `Ok` with the elements parsed so far). It now reports the
//! opener as [`Unclosed`] **through the emitter**:
//!
//! - a fail-fast [`Fatal`] emitter converts the emission to `Err` (carrying the opener's span
//!   and the delimiter pair's name);
//! - a recovering [`Verbose`] emitter records the diagnostic and the parse recovers, returning
//!   the elements collected so far.
//!
//! Both delimiter close-miss shapes are covered: (a) end-of-input with the opener still open
//! ⇒ `Unclosed`; (b) a wrong token where the closer belongs ⇒ the existing unexpected-token
//! (expected-close) vocabulary, **not** `Unclosed`.

mod common;

use common::{TestLexer, Token};
use generic_arraydeque::typenum::U1;
use tokora::EmitterView;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan, TryParseInput,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, SeparatedEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, Verbose,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
  punct::{Brace, Bracket, Paren},
  try_parse_input::ParseAttempt,
};

// ── A rich error type that preserves the `Unclosed` payload ────────────────────
//
// Unlike the shared unit `E`, this captures whether a diagnostic came from `Unclosed`
// (and the opener name + start offset it carries) so the assertions can prove *what*
// was emitted, not merely *that* something was.

#[derive(Debug, Clone, PartialEq)]
enum RE {
  Unclosed { name: String, start: usize },
  Other,
}

// The migration arm: delimited many-builders emit `Unclosed<Delim, …>` with the concrete
// delimiter marker, while the carried name remains available for diagnostics.
impl<D, Lang: ?Sized> From<Unclosed<D, SimpleSpan, Lang>> for RE {
  fn from(err: Unclosed<D, SimpleSpan, Lang>) -> Self {
    RE::Unclosed {
      name: err.name_ref().to_string(),
      start: err.span().start(),
    }
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for RE
where
  L: tokora::Lexer<'inp, Span = SimpleSpan>,
{
  fn from_unclosed<D>(err: Unclosed<D, SimpleSpan, Lang>) -> Self {
    RE::Unclosed {
      name: err.name_ref().to_string(),
      start: err.span().start(),
    }
  }
}

impl From<()> for RE {
  fn from(_: ()) -> Self {
    RE::Other
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for RE {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    RE::Other
  }
}
impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for RE {
  fn from(_: FullContainer<S, Lang>) -> Self {
    RE::Other
  }
}
impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for RE {
  fn from(_: TooFew<S, Lang>) -> Self {
    RE::Other
  }
}
impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for RE {
  fn from(_: TooMany<S, Lang>) -> Self {
    RE::Other
  }
}
impl From<UnexpectedEot> for RE {
  fn from(_: UnexpectedEot) -> Self {
    RE::Other
  }
}
impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for RE {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    RE::Other
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for RE {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    RE::Other
  }
}
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for RE {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    RE::Other
  }
}

type VerboseCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<RE>>;

fn fatal_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<RE>> {
  ParserContext::new(Fatal::new())
}
fn verbose_ctx() -> VerboseCtx<'static> {
  ParserContext::new(Verbose::new())
}

/// Pulls the recorded `Unclosed` diagnostics (name, start) out of a `Verbose<RE>` sink, in
/// span order.
fn recorded_unclosed(em: &Verbose<RE>) -> Vec<(String, usize)> {
  em.errors()
    .values()
    .flatten()
    .filter_map(|e| match e {
      RE::Unclosed { name, start } => Some((name.clone(), *start)),
      RE::Other => None,
    })
    .collect()
}

// ── Element parsers / stop condition ───────────────────────────────────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, RE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>,
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

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, RE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>,
{
  match inp.next()? {
    None => Err(RE::Other),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(RE::Other),
    },
  }
}

fn decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
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

// ═══════════════════════════════════════════════════════════════════════════════
// repeated_while + delimited — THE confirmed probe, full pair × emitter matrix.
// The three inputs "(1 2" / "[1 2" / "{1 2" are the exact regression inputs.
// ═══════════════════════════════════════════════════════════════════════════════

macro_rules! rw_matrix {
  ($fatal:ident, $verbose:ident, $delim:ty, $src:literal, $name:literal) => {
    #[test]
    fn $fatal() {
      fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, RE>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>,
      {
        parse_num
          .repeated_while::<_, U1>(decide_num::<Ctx>)
          .delimited::<$delim>()
          .collect()
          .parse_input(inp)
      }
      let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx()).apply(go).parse_str($src);
      assert_eq!(
        r,
        Err(RE::Unclosed {
          name: $name.to_string(),
          start: 0
        }),
        "fatal: unterminated {} must Err with Unclosed at the opener",
        $src
      );
    }

    #[test]
    fn $verbose() {
      fn go<'inp>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
      ) -> Result<(Vec<i64>, Vec<(String, usize)>), RE> {
        let items: Vec<i64> = parse_num
          .repeated_while::<_, U1>(decide_num::<VerboseCtx<'inp>>)
          .delimited::<$delim>()
          .collect()
          .parse_input(inp)?;
        let diags = recorded_unclosed(inp.emitter_ref());
        Ok((items, diags))
      }
      let (items, diags) = Parser::with_context(verbose_ctx())
        .apply(go)
        .parse_str($src)
        .unwrap();
      assert_eq!(
        items,
        vec![1, 2],
        "verbose: recovery yields the collected elements"
      );
      assert_eq!(
        diags,
        vec![($name.to_string(), 0)],
        "verbose: records exactly one Unclosed at the opener"
      );
    }
  };
}

rw_matrix!(rw_paren_fatal, rw_paren_verbose, Paren<(), (), ()>, "(1 2", "()");
rw_matrix!(rw_bracket_fatal, rw_bracket_verbose, Bracket<(), (), ()>, "[1 2", "[]");
rw_matrix!(rw_brace_fatal, rw_brace_verbose, Brace<(), (), ()>, "{1 2", "{}");

// Miss shape (b): a wrong token where the closer belongs is unexpected-token
// (expected-close), NOT Unclosed. `[1 2 )` stops at `)`, which is not `]`.
#[test]
fn rw_wrong_close_is_unexpected_token_not_unclosed() {
  fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, RE>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + SeparatedEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .repeated_while::<_, U1>(decide_num::<Ctx>)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("[1 2 )");
  assert_eq!(
    r,
    Err(RE::Other),
    "wrong closer must be unexpected-token, not Unclosed"
  );
}

// No opener at all: nothing is unclosed. `1 2` (bracket expected) reports the wrong opener
// (unexpected-token), never Unclosed.
#[test]
fn rw_no_opener_is_not_unclosed() {
  fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, RE>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + SeparatedEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .repeated_while::<_, U1>(decide_num::<Ctx>)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx()).apply(go).parse_str("1 2");
  assert_eq!(
    r,
    Err(RE::Other),
    "no opener ⇒ unexpected-token, not Unclosed"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The other three delim drivers — one EOI case each, both emitters, bracket pair.
// ═══════════════════════════════════════════════════════════════════════════════

// repeated + delimited
#[test]
fn rd_bracket_fatal_errors_unclosed() {
  fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, RE>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    try_num
      .repeated()
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("[1 2");
  assert_eq!(
    r,
    Err(RE::Unclosed {
      name: "[]".to_string(),
      start: 0
    })
  );
}

#[test]
fn rd_bracket_verbose_records_and_recovers() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<(String, usize)>), RE> {
    let items: Vec<i64> = try_num
      .repeated()
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)?;
    Ok((items, recorded_unclosed(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("[1 2")
    .unwrap();
  assert_eq!(items, vec![1, 2]);
  assert_eq!(diags, vec![("[]".to_string(), 0)]);
}

// separated + delimited (this driver used to error with the WRONG vocabulary — a stale
// unexpected-token on the last element — rather than Unclosed; now it is Unclosed).
#[test]
fn sep_bracket_fatal_errors_unclosed() {
  fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, RE>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>
      + SeparatedEmitter<'inp, TestLexer<'inp>>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
      + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    try_num
      .separated_by_comma()
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("[1,2");
  assert_eq!(
    r,
    Err(RE::Unclosed {
      name: "[]".to_string(),
      start: 0
    })
  );
}

#[test]
fn sep_bracket_verbose_records_and_recovers() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<(String, usize)>), RE> {
    let items: Vec<i64> = try_num
      .separated_by_comma()
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)?;
    Ok((items, recorded_unclosed(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("[1,2")
    .unwrap();
  assert_eq!(items, vec![1, 2]);
  assert_eq!(diags, vec![("[]".to_string(), 0)]);
}

// separated_while + delimited
#[test]
fn sw_bracket_fatal_errors_unclosed() {
  fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, RE>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = RE>
      + SeparatedEmitter<'inp, TestLexer<'inp>>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
      + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("[1,2");
  assert_eq!(
    r,
    Err(RE::Unclosed {
      name: "[]".to_string(),
      start: 0
    })
  );
}

#[test]
fn sw_bracket_verbose_records_and_recovers() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<(String, usize)>), RE> {
    let items: Vec<i64> = parse_num
      .separated_by_comma_while::<_, U1>(decide_num::<VerboseCtx<'inp>>)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)?;
    Ok((items, recorded_unclosed(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("[1,2")
    .unwrap();
  assert_eq!(items, vec![1, 2]);
  assert_eq!(diags, vec![("[]".to_string(), 0)]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ordering: the close-status diagnostic is PRIMARY — emitted before the end-state
// secondaries (TooFew / trailing-separator), which under a fail-fast emitter would
// otherwise short-circuit first and bypass `Unclosed` entirely (the separated
// drivers used to run `handle_end` before the close-miss classification).
//
// `SeqE` stamps every `From` conversion with a process-wide sequence number at
// emission time (`Fatal` and `Verbose` both convert at the emit call), so a
// recovering run can assert primary-before-secondary order without reaching into
// the emitter's internals. Stamps are only compared within one parse (one thread),
// so cross-test interleaving of the shared counter is harmless.
// ═══════════════════════════════════════════════════════════════════════════════

use std::sync::atomic::{AtomicUsize, Ordering};

use tokora::emitter::TooFewEmitter;

static SEQ: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, PartialEq)]
enum SeqKind {
  Unclosed { name: String, start: usize },
  Secondary,
}

#[derive(Debug, Clone)]
struct SeqE {
  kind: SeqKind,
  seq: usize,
}

impl SeqE {
  fn secondary() -> Self {
    SeqE {
      kind: SeqKind::Secondary,
      seq: SEQ.fetch_add(1, Ordering::Relaxed),
    }
  }
}

// The migration arm, span-specific so the opener offset is captured for the asserts.
impl<D, Lang: ?Sized> From<Unclosed<D, SimpleSpan, Lang>> for SeqE {
  fn from(err: Unclosed<D, SimpleSpan, Lang>) -> Self {
    SeqE {
      kind: SeqKind::Unclosed {
        name: err.name_ref().to_string(),
        start: err.span().start(),
      },
      seq: SEQ.fetch_add(1, Ordering::Relaxed),
    }
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for SeqE
where
  L: tokora::Lexer<'inp, Span = SimpleSpan>,
{
  fn from_unclosed<D>(err: Unclosed<D, SimpleSpan, Lang>) -> Self {
    SeqE {
      kind: SeqKind::Unclosed {
        name: err.name_ref().to_string(),
        start: err.span().start(),
      },
      seq: SEQ.fetch_add(1, Ordering::Relaxed),
    }
  }
}

impl From<()> for SeqE {
  fn from(_: ()) -> Self {
    SeqE::secondary()
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for SeqE {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    SeqE::secondary()
  }
}
impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for SeqE {
  fn from(_: FullContainer<S, Lang>) -> Self {
    SeqE::secondary()
  }
}
impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for SeqE {
  fn from(_: TooFew<S, Lang>) -> Self {
    SeqE::secondary()
  }
}
impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for SeqE {
  fn from(_: TooMany<S, Lang>) -> Self {
    SeqE::secondary()
  }
}
impl From<UnexpectedEot> for SeqE {
  fn from(_: UnexpectedEot) -> Self {
    SeqE::secondary()
  }
}
impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for SeqE {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    SeqE::secondary()
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for SeqE {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    SeqE::secondary()
  }
}
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for SeqE {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    SeqE::secondary()
  }
}

type SeqVerboseCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<SeqE>>;

fn seq_fatal_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<SeqE>> {
  ParserContext::new(Fatal::new())
}
fn seq_verbose_ctx() -> SeqVerboseCtx<'static> {
  ParserContext::new(Verbose::new())
}

/// Flattened recorded diagnostics of a `Verbose<SeqE>` sink.
fn seq_recorded(em: &Verbose<SeqE>) -> Vec<SeqE> {
  em.errors().values().flatten().cloned().collect()
}

/// Asserts exactly one `Unclosed` (with `name`, at the opener) plus at least one
/// secondary were recorded, the `Unclosed` stamped strictly BEFORE every secondary.
fn assert_unclosed_first(diags: &[SeqE], name: &str) {
  let unclosed: Vec<&SeqE> = diags
    .iter()
    .filter(|e| matches!(&e.kind, SeqKind::Unclosed { .. }))
    .collect();
  let secondaries: Vec<&SeqE> = diags
    .iter()
    .filter(|e| e.kind == SeqKind::Secondary)
    .collect();
  assert_eq!(
    unclosed.len(),
    1,
    "exactly one Unclosed recorded: {diags:?}"
  );
  assert_eq!(
    unclosed[0].kind,
    SeqKind::Unclosed {
      name: name.to_string(),
      start: 0
    }
  );
  assert!(
    !secondaries.is_empty(),
    "the end-state secondary must still be recorded after recovery: {diags:?}"
  );
  for s in &secondaries {
    assert!(
      unclosed[0].seq < s.seq,
      "Unclosed (seq {}) must be emitted before the secondary (seq {}): {diags:?}",
      unclosed[0].seq,
      s.seq
    );
  }
}

// ── SeqE element parsers / condition ──────────────────────────────────────────

fn seq_try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, SeqE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>,
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

fn seq_parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, SeqE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>,
{
  match inp.next()? {
    None => Err(SeqE::secondary()),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(SeqE::secondary()),
    },
  }
}

fn seq_decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
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

// ── separated (`separated_by_comma`) ──────────────────────────────────────────

fn sep_at_least_zero<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SeqE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  seq_try_num
    .separated_by_comma()
    .at_least(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

fn sep_trailing_eof<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SeqE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  seq_try_num
    .separated_by_comma()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

// Fatal, zero elements under at_least: `[` at EOF ⇒ the error IS Unclosed (the
// primary), not the TooFew the end-state handler would raise.
#[test]
fn sep_at_least_zero_fatal_is_unclosed() {
  let r: Result<Vec<i64>, SeqE> = Parser::with_context(seq_fatal_ctx())
    .apply(sep_at_least_zero)
    .parse_str("[");
  let err = r.unwrap_err();
  assert_eq!(
    err.kind,
    SeqKind::Unclosed {
      name: "[]".to_string(),
      start: 0
    },
    "at_least/zero-element `[` at EOF must fail with Unclosed, got {err:?}"
  );
}

// Fatal, trailing separator at EOF: `[1,2,` ⇒ Unclosed (primary), not the
// trailing-separator diagnostic.
#[test]
fn sep_trailing_eof_fatal_is_unclosed() {
  let r: Result<Vec<i64>, SeqE> = Parser::with_context(seq_fatal_ctx())
    .apply(sep_trailing_eof)
    .parse_str("[1,2,");
  let err = r.unwrap_err();
  assert_eq!(
    err.kind,
    SeqKind::Unclosed {
      name: "[]".to_string(),
      start: 0
    },
    "trailing-separator `[1,2,` at EOF must fail with Unclosed, got {err:?}"
  );
}

// Recovering twins: Unclosed recorded FIRST, then the secondary, then the elements.
#[test]
fn sep_at_least_zero_verbose_unclosed_first() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, SeqVerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<SeqE>), SeqE> {
    let items = sep_at_least_zero(inp)?;
    Ok((items, seq_recorded(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(seq_verbose_ctx())
    .apply(go)
    .parse_str("[")
    .unwrap();
  assert_eq!(items, Vec::<i64>::new());
  assert_unclosed_first(&diags, "[]");
}

#[test]
fn sep_trailing_eof_verbose_unclosed_first() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, SeqVerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<SeqE>), SeqE> {
    let items = sep_trailing_eof(inp)?;
    Ok((items, seq_recorded(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(seq_verbose_ctx())
    .apply(go)
    .parse_str("[1,2,")
    .unwrap();
  assert_eq!(items, vec![1, 2]);
  assert_unclosed_first(&diags, "[]");
}

// ── separated_while (`separated_by_comma_while`) ──────────────────────────────

fn sw_at_least_zero<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SeqE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  seq_parse_num
    .separated_by_comma_while::<_, U1>(seq_decide_num::<Ctx>)
    .at_least(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

fn sw_trailing_eof<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, SeqE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  seq_parse_num
    .separated_by_comma_while::<_, U1>(seq_decide_num::<Ctx>)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_at_least_zero_fatal_is_unclosed() {
  let r: Result<Vec<i64>, SeqE> = Parser::with_context(seq_fatal_ctx())
    .apply(sw_at_least_zero)
    .parse_str("[");
  let err = r.unwrap_err();
  assert_eq!(
    err.kind,
    SeqKind::Unclosed {
      name: "[]".to_string(),
      start: 0
    },
    "at_least/zero-element `[` at EOF must fail with Unclosed, got {err:?}"
  );
}

#[test]
fn sw_trailing_eof_fatal_is_unclosed() {
  let r: Result<Vec<i64>, SeqE> = Parser::with_context(seq_fatal_ctx())
    .apply(sw_trailing_eof)
    .parse_str("[1,2,");
  let err = r.unwrap_err();
  assert_eq!(
    err.kind,
    SeqKind::Unclosed {
      name: "[]".to_string(),
      start: 0
    },
    "trailing-separator `[1,2,` at EOF must fail with Unclosed, got {err:?}"
  );
}

#[test]
fn sw_at_least_zero_verbose_unclosed_first() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, SeqVerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<SeqE>), SeqE> {
    let items = sw_at_least_zero(inp)?;
    Ok((items, seq_recorded(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(seq_verbose_ctx())
    .apply(go)
    .parse_str("[")
    .unwrap();
  assert_eq!(items, Vec::<i64>::new());
  assert_unclosed_first(&diags, "[]");
}

#[test]
fn sw_trailing_eof_verbose_unclosed_first() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, SeqVerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<SeqE>), SeqE> {
    let items = sw_trailing_eof(inp)?;
    Ok((items, seq_recorded(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(seq_verbose_ctx())
    .apply(go)
    .parse_str("[1,2,")
    .unwrap();
  assert_eq!(items, vec![1, 2]);
  assert_unclosed_first(&diags, "[]");
}

// ── plain drivers: ordering pins (already correct — Unclosed before on_stop) ──

#[test]
fn rd_at_least_zero_fatal_is_unclosed() {
  fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, SeqE>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    seq_try_num
      .repeated()
      .at_least(1)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, SeqE> = Parser::with_context(seq_fatal_ctx())
    .apply(go)
    .parse_str("[");
  let err = r.unwrap_err();
  assert_eq!(
    err.kind,
    SeqKind::Unclosed {
      name: "[]".to_string(),
      start: 0
    },
    "plain repeated: `[` + at_least must fail with Unclosed, got {err:?}"
  );
}

#[test]
fn rw_at_least_zero_fatal_is_unclosed() {
  fn go<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, SeqE>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SeqE>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + SeparatedEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    seq_parse_num
      .repeated_while::<_, U1>(seq_decide_num::<Ctx>)
      .at_least(1)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, SeqE> = Parser::with_context(seq_fatal_ctx())
    .apply(go)
    .parse_str("[");
  let err = r.unwrap_err();
  assert_eq!(
    err.kind,
    SeqKind::Unclosed {
      name: "[]".to_string(),
      start: 0
    },
    "plain repeated_while: `[` + at_least must fail with Unclosed, got {err:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Finding 2 — delimited `repeated_while`, `Action::Stop`, RECOVERING emitter.
//
// The delimited `repeated_while` driver used to emit the primary close diagnostic on
// the `Action::Stop` path and then return WITHOUT running the repeated end handler
// (`on_stop`). Under a fail-fast emitter the primary short-circuits, so the omission
// was invisible; under a RECOVERING emitter it silently dropped the secondary
// `TooFew`/bounds diagnostic (the plain, non-delimited `repeated_while` driver runs
// it). The fix runs `on_stop` after the primary — the same primary-then-secondary
// order the separated drivers established. `assert_unclosed_first` requires BOTH an
// `Unclosed` (first) AND at least one secondary, so it was red before the fix
// (no secondary) and is green after.
// ═══════════════════════════════════════════════════════════════════════════════

// at_least with ZERO elements: `[` at EOF ⇒ Unclosed FIRST, then TooFew(0, 1).
#[test]
fn rw_delim_at_least_zero_verbose_unclosed_then_toofew() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, SeqVerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<SeqE>), SeqE> {
    let items: Vec<i64> = seq_parse_num
      .repeated_while::<_, U1>(seq_decide_num::<SeqVerboseCtx<'inp>>)
      .at_least(1)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)?;
    Ok((items, seq_recorded(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(seq_verbose_ctx())
    .apply(go)
    .parse_str("[")
    .unwrap();
  assert_eq!(items, Vec::<i64>::new(), "zero elements collected");
  assert_unclosed_first(&diags, "[]");
}

// bounded, too-few elements: `[1` at EOF with a min of 2 ⇒ Unclosed FIRST, then
// TooFew(1, 2). `[1` reaches the same `Action::Stop` branch (one element, then EOF).
#[test]
fn rw_delim_bounded_too_few_verbose_unclosed_then_toofew() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, SeqVerboseCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<SeqE>), SeqE> {
    let items: Vec<i64> = seq_parse_num
      .repeated_while::<_, U1>(seq_decide_num::<SeqVerboseCtx<'inp>>)
      .bounded(2, 4)
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp)?;
    Ok((items, seq_recorded(inp.emitter_ref())))
  }
  let (items, diags) = Parser::with_context(seq_verbose_ctx())
    .apply(go)
    .parse_str("[1")
    .unwrap();
  assert_eq!(items, vec![1], "the one element before EOF is collected");
  assert_unclosed_first(&diags, "[]");
}
