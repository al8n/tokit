#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! INVARIANT E — the eight-driver end-state parity matrix.
//!
//! Every repetition driver runs its end-state pass exactly once on every `Ok` exit, so the
//! same logical history must produce the same diagnostic vector no matter which builder the
//! user picked. This file drives one history through all eight collection drivers
//! (`repeated`, `repeated_while`, their two delimited forms, and the four separated ones)
//! under a `Verbose` emitter and asserts the recorded vector is identical.
//!
//! The error type here deliberately keeps the payloads (`nums`/`limit`/`capacity`) that the
//! shared `common::E` collapses to a unit value: the payload *is* what several of these rows
//! pin.
//!
//! # Sentinel token
//!
//! The two non-delimited `*_while` drivers consult their condition through a lookahead
//! window; at EOF there is nothing to peek. Their inputs therefore end in `+`, the same
//! convention `parser_repeated_while.rs` and `sep_while_parse.rs` use. Delimited inputs need
//! no sentinel — the closer is the stop token.

mod common;

use common::{TestLexer, Token};
use tokora::EmitterView;
use tokora::{
  Accumulator, InputRef, Parse, ParseInput, Parser, ParserContext, TryParseInput,
  cache::Peeked,
  emitter::Verbose,
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
  punct::Bracket,
  try_parse_input::ParseAttempt,
  utils::{
    GenericArrayDeque,
    typenum::{U1, U2},
  },
};

// ── The payload-preserving diagnostic ─────────────────────────────────────────

/// One recorded diagnostic, with the payload fields this matrix asserts on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Diag {
  TooMany(usize, usize),
  TooFew(usize, usize),
  Full(usize, usize),
  /// An unexpected separator, tagged with its `SeparatorPosition`.
  UnexpectedSeparator(&'static str),
  /// A missing separator — leading, trailing, or between two elements. All three arrive as a
  /// `MissingToken`, so they are one variant here; no case below produces more than one kind.
  MissingSeparator,
  MissingElement,
  UnexpectedToken,
  Unclosed,
  Eot,
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

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for Diag {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    Diag::MissingSeparator
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for Diag {
  fn from(e: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    Diag::UnexpectedSeparator(e.position().as_str())
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Diag {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Diag::MissingElement
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

// ── Fixture ───────────────────────────────────────────────────────────────────

type VCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<Diag>>;

fn verbose_ctx() -> VCtx<'static> {
  ParserContext::new(Verbose::new())
}

type Cap1 = GenericArrayDeque<i64, U1>;
type Cap2 = GenericArrayDeque<i64, U2>;

fn try_num<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
) -> Result<ParseAttempt<i64>, Diag> {
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

fn parse_num<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<i64, Diag> {
  match inp.next()? {
    None => Err(Diag::Eot),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(Diag::Lex),
    },
  }
}

fn decide_num<'inp>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Verbose<Diag>>,
) -> Result<Action, Diag> {
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

/// Runs one driver over `src` and returns the diagnostics it recorded, flattened out of
/// `Verbose`'s span-keyed map into one source-ordered vector.
macro_rules! parity {
  ($name:ident, $out:ty, $src:literal, $expected:expr, $inp:ident => $build:expr) => {
    #[test]
    fn $name() {
      fn go<'inp>(
        $inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
      ) -> Result<Vec<Diag>, Diag> {
        let _out: $out = $build;
        Ok(
          $inp
            .emitter_ref()
            .errors()
            .values()
            .flatten()
            .cloned()
            .collect(),
        )
      }
      let got = Parser::with_context(verbose_ctx())
        .apply(go)
        .parse_str($src)
        .unwrap();
      let expected: Vec<Diag> = $expected;
      assert_eq!(
        got, expected,
        concat!(
          stringify!($name),
          " on ",
          $src,
          ": every driver must report the same \
                 diagnostic vector for the same history (INVARIANT E / LAW N)"
        )
      );
    }
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Case A — `at_most(2)` with 4 elements: exactly one `TooMany`, payload (3, 2).
//
// `nums` is the smallest count that exceeds the limit it names (LAW N), and the
// diagnostic is emitted once per construct, whichever driver detects it.
// ═══════════════════════════════════════════════════════════════════════════════

parity!(a_repeated, Vec<i64>, "1 2 3 4", vec![Diag::TooMany(3, 2)], inp =>
  try_num.repeated().at_most(2).collect().parse_input(inp)?);

parity!(a_repeated_while, Vec<i64>, "1 2 3 4 +", vec![Diag::TooMany(3, 2)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(2).collect().parse_input(inp)?);

parity!(a_delim_repeated, Vec<i64>, "[1 2 3 4]", vec![Diag::TooMany(3, 2)], inp =>
  try_num.repeated().at_most(2).delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(a_delim_repeated_while, Vec<i64>, "[1 2 3 4]", vec![Diag::TooMany(3, 2)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(a_sep, Vec<i64>, "1,2,3,4", vec![Diag::TooMany(3, 2)], inp =>
  try_num.separated_by_comma().at_most(2).collect().parse_input(inp)?);

parity!(a_sep_delim, Vec<i64>, "[1,2,3,4]", vec![Diag::TooMany(3, 2)], inp =>
  try_num.separated_by_comma().at_most(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(a_sep_while, Vec<i64>, "1,2,3,4+", vec![Diag::TooMany(3, 2)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(2).collect().parse_input(inp)?);

parity!(a_sep_while_delim, Vec<i64>, "[1,2,3,4]", vec![Diag::TooMany(3, 2)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// ═══════════════════════════════════════════════════════════════════════════════
// Case B — `at_most(2)` with exactly 2 elements: no diagnostic at all.
//
// The control row: the end-state pass now runs on paths it never reached before, so it must
// stay silent on conforming input, and the mid-loop `== max` hook must not fire one early.
// ═══════════════════════════════════════════════════════════════════════════════

parity!(b_repeated, Vec<i64>, "1 2", vec![], inp =>
  try_num.repeated().at_most(2).collect().parse_input(inp)?);

parity!(b_repeated_while, Vec<i64>, "1 2 +", vec![], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(2).collect().parse_input(inp)?);

parity!(b_delim_repeated, Vec<i64>, "[1 2]", vec![], inp =>
  try_num.repeated().at_most(2).delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(b_delim_repeated_while, Vec<i64>, "[1 2]", vec![], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(b_sep, Vec<i64>, "1,2", vec![], inp =>
  try_num.separated_by_comma().at_most(2).collect().parse_input(inp)?);

parity!(b_sep_delim, Vec<i64>, "[1,2]", vec![], inp =>
  try_num.separated_by_comma().at_most(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(b_sep_while, Vec<i64>, "1,2+", vec![], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(2).collect().parse_input(inp)?);

parity!(b_sep_while_delim, Vec<i64>, "[1,2]", vec![], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// ═══════════════════════════════════════════════════════════════════════════════
// Case C — `at_least(2)` with 1 element: exactly one `TooFew(1, 2)`.
//
// This is the closed-list row: on a properly closed `[1]` the `sep/delim` driver used to return
// through its mid-scan-closer arm without ever running the bound.
// ═══════════════════════════════════════════════════════════════════════════════

parity!(c_repeated, Vec<i64>, "1", vec![Diag::TooFew(1, 2)], inp =>
  try_num.repeated().at_least(2).collect().parse_input(inp)?);

parity!(c_repeated_while, Vec<i64>, "1 +", vec![Diag::TooFew(1, 2)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_least(2).collect().parse_input(inp)?);

parity!(c_delim_repeated, Vec<i64>, "[1]", vec![Diag::TooFew(1, 2)], inp =>
  try_num.repeated().at_least(2).delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(c_delim_repeated_while, Vec<i64>, "[1]", vec![Diag::TooFew(1, 2)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_least(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(c_sep, Vec<i64>, "1", vec![Diag::TooFew(1, 2)], inp =>
  try_num.separated_by_comma().at_least(2).collect().parse_input(inp)?);

parity!(c_sep_delim, Vec<i64>, "[1]", vec![Diag::TooFew(1, 2)], inp =>
  try_num.separated_by_comma().at_least(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(c_sep_while, Vec<i64>, "1+", vec![Diag::TooFew(1, 2)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_least(2).collect().parse_input(inp)?);

parity!(c_sep_while_delim, Vec<i64>, "[1]", vec![Diag::TooFew(1, 2)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_least(2)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// ═══════════════════════════════════════════════════════════════════════════════
// Case D — capacity-1 container, 3 elements, no count bounds: exactly one
// `FullContainer(2, 1)`.
//
// One report per construct: re-emitting per dropped element produced a count that climbed past
// the capacity it named. `nums` is the DRIVER's own parsed-element count at the first refusal,
// including the refused element, so "found 2 … exceeds … 1" is true (LAW N) without the
// container being asked to confirm it.
// ═══════════════════════════════════════════════════════════════════════════════

parity!(d_repeated, Cap1, "1 2 3", vec![Diag::Full(2, 1)], inp =>
  try_num.repeated().collect().parse_input(inp)?);

parity!(d_repeated_while, Cap1, "1 2 3 +", vec![Diag::Full(2, 1)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).collect().parse_input(inp)?);

parity!(d_delim_repeated, Cap1, "[1 2 3]", vec![Diag::Full(2, 1)], inp =>
  try_num.repeated().delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(d_delim_repeated_while, Cap1, "[1 2 3]", vec![Diag::Full(2, 1)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(d_sep, Cap1, "1,2,3", vec![Diag::Full(2, 1)], inp =>
  try_num.separated_by_comma().collect().parse_input(inp)?);

parity!(d_sep_delim, Cap1, "[1,2,3]", vec![Diag::Full(2, 1)], inp =>
  try_num.separated_by_comma().delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(d_sep_while, Cap1, "1,2,3+", vec![Diag::Full(2, 1)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).collect().parse_input(inp)?);

parity!(d_sep_while_delim, Cap1, "[1,2,3]", vec![Diag::Full(2, 1)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// ═══════════════════════════════════════════════════════════════════════════════
// Case E — capacity-2 container, `at_least(5)`, 5 elements: no `TooFew`, exactly one
// `FullContainer(3, 2)`.
//
// Count bounds judge the elements the driver PARSED, not the ones the container stored: the
// grammar fact "five elements are present" is satisfied, and the container's inadequacy is
// reported by the diagnostic named for it.
// ═══════════════════════════════════════════════════════════════════════════════

parity!(e_repeated, Cap2, "1 2 3 4 5", vec![Diag::Full(3, 2)], inp =>
  try_num.repeated().at_least(5).collect().parse_input(inp)?);

parity!(e_repeated_while, Cap2, "1 2 3 4 5 +", vec![Diag::Full(3, 2)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_least(5).collect().parse_input(inp)?);

parity!(e_delim_repeated, Cap2, "[1 2 3 4 5]", vec![Diag::Full(3, 2)], inp =>
  try_num.repeated().at_least(5).delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(e_delim_repeated_while, Cap2, "[1 2 3 4 5]", vec![Diag::Full(3, 2)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_least(5)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(e_sep, Cap2, "1,2,3,4,5", vec![Diag::Full(3, 2)], inp =>
  try_num.separated_by_comma().at_least(5).collect().parse_input(inp)?);

parity!(e_sep_delim, Cap2, "[1,2,3,4,5]", vec![Diag::Full(3, 2)], inp =>
  try_num.separated_by_comma().at_least(5)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(e_sep_while, Cap2, "1,2,3,4,5+", vec![Diag::Full(3, 2)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_least(5).collect().parse_input(inp)?);

parity!(e_sep_while_delim, Cap2, "[1,2,3,4,5]", vec![Diag::Full(3, 2)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_least(5)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// ═══════════════════════════════════════════════════════════════════════════════
// Case F — default policy, trailing separator (the four separated drivers): exactly one
// unexpected-trailing-separator.
// ═══════════════════════════════════════════════════════════════════════════════

parity!(f_sep, Vec<i64>, "1,", vec![Diag::UnexpectedSeparator("trailing")], inp =>
  try_num.separated_by_comma().collect().parse_input(inp)?);

parity!(f_sep_delim, Vec<i64>, "[1,]", vec![Diag::UnexpectedSeparator("trailing")], inp =>
  try_num.separated_by_comma().delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(f_sep_while, Vec<i64>, "1,+", vec![Diag::UnexpectedSeparator("trailing")], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).collect().parse_input(inp)?);

parity!(f_sep_while_delim, Vec<i64>, "[1,]", vec![Diag::UnexpectedSeparator("trailing")], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// ═══════════════════════════════════════════════════════════════════════════════
// Case G — `require_trailing` with no trailing separator (the four separated drivers):
// exactly one missing-trailing-separator.
// ═══════════════════════════════════════════════════════════════════════════════

parity!(g_sep, Vec<i64>, "1", vec![Diag::MissingSeparator], inp =>
  try_num.separated_by_comma().require_trailing().collect().parse_input(inp)?);

parity!(g_sep_delim, Vec<i64>, "[1]", vec![Diag::MissingSeparator], inp =>
  try_num.separated_by_comma().require_trailing()
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity!(g_sep_while, Vec<i64>, "1+", vec![Diag::MissingSeparator], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).require_trailing()
    .collect().parse_input(inp)?);

parity!(g_sep_while_delim, Vec<i64>, "[1]", vec![Diag::MissingSeparator], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).require_trailing()
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// ═══════════════════════════════════════════════════════════════════════════════
// Case H — the collision: `at_most(1)`, a capacity-1 container, and two parsed elements, so
// the SAME element both exceeds the count maximum and is refused by the destination.
//
// Every earlier case isolates one condition, which is why the matrix stayed green while the
// eight drivers disagreed about this one.
//
// **The order here is CHRONOLOGICAL, and it is not uniform across the drivers.** Each row
// asserts the order that driver's own detection points imply, and the two groups are:
//
// * `repeated` and `repeated_while` report a violated maximum from the mid-loop `on_element`
//   hook, which runs BEFORE the element is pushed — so `TooMany` precedes the refusal;
// * the four delimited and the four separated drivers detect it from an end callback or the
//   end-state pass, both of which run after the loop — so the refusal, reported when it
//   happens, precedes `TooMany`.
//
// This file used to assert `[TooMany, FullContainer]` in all twelve rows, bought by withholding
// the capacity report until each driver's end-state pass. That is not payable here. The emitter
// is not a log in this driver — it is what decides whether the parse continues — so withholding
// the report also withheld the decision: `Fatal`, documented to stop at the first error, ran on
// past the refusal to the end of the construct; any later `Err` exit propagated over the
// withheld report and discarded it; and a destination that refuses at element 2 stopped being an
// O(1) decision. `many::push_element` states why the ordering cannot be bought back, and
// `capacity_report_timing.rs` pins the three properties that were paying for it.
//
// So the rows below are a real pin in both directions: withholding the report again flips the
// nine that read `[Full, TooMany]`, and moving a maximum check flips one of the other three.
//
// `parity_ordered!` reads the emission log rather than the span-keyed map: the two payloads
// carry different spans in the delimited rows, so span order is not emission order there and
// only the log can witness which came first.
// ═══════════════════════════════════════════════════════════════════════════════

/// Like [`parity!`], but asserts the diagnostics in **emission** order.
macro_rules! parity_ordered {
  ($name:ident, $out:ty, $src:literal, $expected:expr, $inp:ident => $build:expr) => {
    #[test]
    fn $name() {
      fn go<'inp>(
        $inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
      ) -> Result<Vec<Diag>, Diag> {
        let _out: $out = $build;
        Ok(
          $inp
            .emitter_ref()
            .diagnostics()
            .filter_map(|d| d.payload().cloned())
            .collect(),
        )
      }
      let got = Parser::with_context(verbose_ctx())
        .apply(go)
        .parse_str($src)
        .unwrap();
      let expected: Vec<Diag> = $expected;
      assert_eq!(
        got, expected,
        concat!(
          stringify!($name),
          " on ",
          $src,
          ": each diagnostic is reported when its fact becomes true — the mid-loop maximum \
           before the push it precedes, the destination's refusal at the refusal (INVARIANT E \
           / LAW N)"
        )
      );
    }
  };
}

parity_ordered!(h_repeated, Cap1, "1 2", vec![Diag::TooMany(2, 1), Diag::Full(2, 1)], inp =>
  try_num.repeated().at_most(1).collect().parse_input(inp)?);

parity_ordered!(h_repeated_while, Cap1, "1 2 +", vec![Diag::TooMany(2, 1), Diag::Full(2, 1)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(1).collect().parse_input(inp)?);

parity_ordered!(h_delim_repeated, Cap1, "[1 2]", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  try_num.repeated().at_most(1).delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity_ordered!(h_delim_repeated_while, Cap1, "[1 2]", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(1)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity_ordered!(h_sep, Cap1, "1,2", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  try_num.separated_by_comma().at_most(1).collect().parse_input(inp)?);

parity_ordered!(h_sep_delim, Cap1, "[1,2]", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  try_num.separated_by_comma().at_most(1)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity_ordered!(h_sep_while, Cap1, "1,2+", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(1).collect().parse_input(inp)?);

parity_ordered!(h_sep_while_delim, Cap1, "[1,2]", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(1)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

// The same collision under `.bounded(0, 1)`, whose maximum arm rides the same hooks: the four
// plain rows would otherwise be the only proof that the mid-loop hook and the end pass agree.
parity_ordered!(h_bounded_repeated, Cap1, "1 2", vec![Diag::TooMany(2, 1), Diag::Full(2, 1)], inp =>
  try_num.repeated().bounded(0, 1).collect().parse_input(inp)?);

parity_ordered!(h_bounded_sep, Cap1, "1,2", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  try_num.separated_by_comma().bounded(0, 1).collect().parse_input(inp)?);

parity_ordered!(h_bounded_delim_repeated, Cap1, "[1 2]", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  try_num.repeated().bounded(0, 1).delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);

parity_ordered!(h_bounded_sep_delim, Cap1, "[1,2]", vec![Diag::Full(2, 1), Diag::TooMany(2, 1)], inp =>
  try_num.separated_by_comma().bounded(0, 1)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?);
