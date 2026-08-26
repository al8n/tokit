#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! When the destination's capacity report reaches the emitter — and the three things that
//! answer depends on.
//!
//! In a collection driver the emitter is not a log. It is the thing that decides whether the
//! parse continues: `Ok(())` means *recovered, keep going*, `Err` means *stop now*. So **when**
//! `FullContainer` is emitted is also **when** a rejecting emitter gets to stop, and a report
//! withheld until the construct's end — which is what buys a deterministic
//! grammar-before-capacity order for a *recovering* emitter — costs a rejecting one three
//! separate things. Each cell below is one of them, and each was red before the report moved
//! back to the refusal:
//!
//! 1. `Fatal` is documented to stop at the first error. Withheld, it ran to the end of the
//!    construct first (nine element attempts over `1 2 3 4 5 6 7 8`, not two).
//! 2. A refusal followed by any later `Err` exit lost the report entirely: the parse's error
//!    was the *later* failure and the witnessed refusal was never told.
//! 3. A destination that refuses at element 2 is an O(1) decision. Withheld, it became O(n)
//!    over trailing input the caller does not choose — 4099 element attempts against 4096
//!    trailing elements, versus 3 with none.
//!
//! `many::admit_element` states why the ordering the deferral bought cannot be bought back — and
//! how the ordering was reached instead, by settling the element's count bound one line ahead of
//! the push rather than by moving the capacity report. `end_state_parity.rs`'s case H and
//! `repetition_diagnostic_order.rs` record what both emitter classes see now.

mod common;

use common::{TestLexer, Token};
use std::cell::Cell;
use tokora::{
  Accumulator, InputRef, Parse, ParseInput, Parser, ParserContext, TryParseInput,
  emitter::Fatal,
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  try_parse_input::ParseAttempt,
  utils::{GenericArrayDeque, typenum::U1},
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Diag {
  Full(usize, usize),
  TooMany(usize, usize),
  Element,
  Other,
}

macro_rules! other {
  ($($ty:ty),* $(,)?) => {$(
    impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<$ty> for Diag {
      fn from(_: $ty) -> Self {
        Diag::Other
      }
    }
  )*};
}

other!(
  UnexpectedToken<'a, T, Kind, S, Lang>,
  SeparatedError<'a, T, Kind, S, Lang>,
);

impl From<()> for Diag {
  fn from(_: ()) -> Self {
    Diag::Other
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Diag {
  fn from(e: FullContainer<S, Lang>) -> Self {
    Diag::Full(e.nums(), e.capacity())
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Diag {
  fn from(_: TooFew<S, Lang>) -> Self {
    Diag::Other
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Diag {
  fn from(e: TooMany<S, Lang>) -> Self {
    Diag::TooMany(e.nums(), e.limit())
  }
}

impl From<UnexpectedEot> for Diag {
  fn from(_: UnexpectedEot) -> Self {
    Diag::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for Diag {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    Diag::Other
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Diag {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Diag::Other
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unclosed<Delimiter, S, Lang>> for Diag {
  fn from(_: Unclosed<Delimiter, S, Lang>) -> Self {
    Diag::Other
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Diag
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<Delimiter>(_: Unclosed<Delimiter, L::Span, Lang>) -> Self {
    Diag::Other
  }
}

type FCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Fatal<Diag>>;

/// Generic in `'inp` rather than fixed at `'static`: the amplification fixture below builds its
/// inputs at run time, and a `'static` context would force those `String`s to be leaked to be
/// borrowed from. This repository runs Miri with leak checking on, so a leaked fixture is a red
/// cell — the context holds no input, so nothing here needs `'static`.
fn fatal_ctx<'inp>() -> FCtx<'inp> {
  ParserContext::new(Fatal::new())
}

type Cap1 = GenericArrayDeque<i64, U1>;

thread_local! {
  /// How many times the element parser was entered in the current parse.
  static ATTEMPTS: Cell<usize> = const { Cell::new(0) };
}

fn attempts() -> usize {
  ATTEMPTS.with(|c| c.get())
}

fn reset() {
  ATTEMPTS.with(|c| c.set(0));
}

/// Accepts a `Num`, declines anything else — and counts every entry.
fn try_num<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>,
) -> Result<ParseAttempt<i64>, Diag> {
  ATTEMPTS.with(|c| c.set(c.get() + 1));
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

/// Accepts a `Num`, **fails** on `Ident`, declines anything else.
fn try_num_or_fail<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>,
) -> Result<ParseAttempt<i64>, Diag> {
  ATTEMPTS.with(|c| c.set(c.get() + 1));
  match inp.try_expect(|t| matches!(t.data(), Token::Num(_) | Token::Ident))? {
    None => Ok(ParseAttempt::Decline),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(ParseAttempt::Accept(n)),
      _ => Err(Diag::Element),
    },
  }
}

fn go_plain<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>) -> Result<Cap1, Diag> {
  try_num.repeated().collect().parse_input(inp)
}

fn go_failing<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>,
) -> Result<Cap1, Diag> {
  try_num_or_fail.repeated().collect().parse_input(inp)
}

fn run_plain(src: &str) -> (Result<Cap1, Diag>, usize) {
  reset();
  let out = Parser::with_context(fatal_ctx())
    .apply(go_plain)
    .parse_str(src);
  (out, attempts())
}

// ── 1. FAIL-FAST: the refusal must stop the parse ─────────────────────────────

/// The count is the load-bearing half. `Err(Full(2, 1))` was already the answer while the report
/// was withheld — it just arrived after the whole construct had been parsed, which is the thing
/// a fail-fast contract exists to prevent.
#[test]
fn a_rejected_refusal_stops_the_parse_at_the_refusal() {
  // Capacity 1: element 1 is stored, element 2 is REFUSED. Six more elements follow.
  let (out, attempts) = run_plain("1 2 3 4 5 6 7 8");
  assert_eq!(
    out,
    Err(Diag::Full(2, 1)),
    "under `Fatal` the refusal is the parse's error"
  );
  assert_eq!(
    attempts, 2,
    "under `Fatal` the driver stops AT the refusal: two element attempts, not nine"
  );
}

// ── 2. THE DROP: a later `Err` must not discard the report ────────────────────

/// `1 2 oops`: the destination refuses element 2, then element 3 fails to parse. A report held
/// for a later exit is discarded by the `Err` that reaches that exit first, and the caller is
/// told about the element failure instead of the refusal that preceded it.
#[test]
fn a_later_error_does_not_discard_the_witnessed_refusal() {
  reset();
  let out = Parser::with_context(fatal_ctx())
    .apply(go_failing)
    .parse_str("1 2 oops");
  assert_eq!(
    out,
    Err(Diag::Full(2, 1)),
    "the refusal at element 2 is the parse's error — not the element-3 failure that followed it"
  );
}

// ── 3. AMPLIFICATION: a prefix failure is constant work ───────────────────────

/// `1 2` followed by `n` more elements. The refusal is decided at element 2 in every row, so the
/// element-attempt count must not move with `n`.
fn trailing(n: usize) -> String {
  let mut s = String::from("1 2");
  for i in 0..n {
    s.push(' ');
    s.push_str(&(i as i64 + 3).to_string());
  }
  s
}

/// The row that makes this a `high` rather than a wording defect: `n` is attacker-controlled.
/// Withheld, the counts ran 3 / 67 / 515 / 4099 — a 1366x amplification at the widest row over a
/// decision the driver had already made at element 2.
#[test]
fn a_prefix_refusal_costs_the_same_whatever_follows_it() {
  let counts: Vec<(usize, usize)> = [0usize, 64, 512, 4096]
    .into_iter()
    .map(|n| {
      // Owned, not `Box::leak`ed: the parse and the assertion both finish inside this closure,
      // so the row's input is dropped with it.
      let src = trailing(n);
      let (out, attempts) = run_plain(&src);
      assert_eq!(out, Err(Diag::Full(2, 1)), "trailing={n}: same verdict");
      (n, attempts)
    })
    .collect();
  let base = counts[0].1;
  assert!(
    counts.iter().all(|&(_, attempts)| attempts == base),
    "a prefix that fails at element 2 must cost the same whatever follows it: {counts:?}"
  );
}
