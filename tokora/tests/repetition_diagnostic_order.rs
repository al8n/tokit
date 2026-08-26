#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! LAW O — **an element's count verdict is reported before that element is offered to the
//! destination**, in every repetition driver.
//!
//! # The collision, and why the order is not a free choice
//!
//! One parsed element can be refused twice at once: by the repetition's own `at_most`/`bounded`
//! maximum, and by a destination with no room left. Which diagnostic is primary decides the
//! *public* answer under [`Fatal`](tokora::emitter::Fatal), which stops at the first one — so
//! before #277 the same input, the same limits and the same container returned `TooMany` under
//! `repeated` and `FullContainer` under `separated`, purely because the caller picked another
//! builder.
//!
//! The two candidates are **not symmetric**, and neither the input nor the limits pick between
//! them — the history does:
//!
//! * A same-element collision can only happen at element `max + 1`. It needs elements `1..=max`
//!   to have been *accepted* by the destination (a destination that refused earlier already
//!   spent this construct's one capacity report on that earlier element), so the destination is
//!   large enough for every element the grammar allows.
//! * Therefore **enlarging the destination never clears the parse** — element `max + 1` still
//!   violates the maximum — while **trimming the input to `max` elements clears both**. The
//!   maximum is the root; the refusal is a consequence of parsing an element the grammar had
//!   already rejected.
//! * And the two facts become true in that order. "This element exceeds the maximum" is settled
//!   by the driver's own parsed-element count the moment the element parses. "The destination
//!   refused it" cannot be settled until the driver *offers* it, which is strictly later. The
//!   maximum is decidable without asking anyone; the refusal is a caller-implemented
//!   [`Container`](tokora::container::Container)'s answer, and `many::admit_element` already
//!   holds that the container is untrusted input to the accounting rather than a participant in
//!   it.
//!
//! So `[TooMany, FullContainer]` for one element, everywhere. It is also the only order the two
//! rules this crate already states can both hold: `many::admit_element` requires the capacity
//! report to be made **at the refusal** (withholding it costs a rejecting emitter its documented
//! stop, a witnessed diagnostic, and a bounded amount of work — `capacity_report_timing.rs` pins
//! all three), and `end_state_parity.rs`'s INVARIANT E requires one diagnostic vector per logical
//! history whatever the builder. Fix the capacity report at the refusal and demand uniformity and
//! the maximum has exactly one place left to go: the same admission, one line earlier.
//!
//! # Cross-element order is untouched
//!
//! The law is about **one** element. When the destination fills at element 3 and the maximum is
//! first exceeded at element 5, the refusal really did happen first, and every driver reports
//! `[FullContainer, TooMany]` — before this change and after it. The control table below is that
//! history, and it is what separates "the collision was fixed" from "the order was flipped
//! globally".
//!
//! # Why a table
//!
//! [#259](https://github.com/al8n/tokora/issues/259) will rewrite `parser::many` into shared
//! engines, so this law has to survive a rewrite that has not been written yet. The rows are a
//! declared list and the assertion is written once, over the list — a driver added later is
//! covered by adding one line, and `many/mod.rs`'s `ELEMENT_ADMISSION_CENSUS` is what makes
//! *forgetting* that line visible: it pins the number of element-admission sites in the tree, so
//! a new driver cannot reach a container without moving a count this file also asserts.
//!
//! # Sentinel token
//!
//! The `*_while` drivers consult their condition through a lookahead window, and at EOF there is
//! nothing to peek. Their non-delimited inputs therefore end in `+`, the convention
//! `parser_repeated_while.rs` and `end_state_parity.rs` use. Delimited inputs need no sentinel —
//! the closer is the stop token.

mod common;

use common::{TestLexer, Token};
use tokora::{
  Accumulator, Emitter, EmitterView, InputRef, Parse, ParseContext, ParseInput, Parser,
  ParserContext, TryParseInput,
  cache::Peeked,
  emitter::{
    Fatal, FromUnclosed, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnclosedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
    Verbose,
  },
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

/// One recorded diagnostic. `TooMany` and `Full` keep their payloads because the payload is half
/// of what the collision rows pin — an order assertion over two variants that carry no numbers
/// cannot tell a `TooMany` about element 5 from one about element 2.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Diag {
  TooMany(usize, usize),
  Full(usize, usize),
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

impl From<UnexpectedEot> for Diag {
  fn from(_: UnexpectedEot) -> Self {
    Diag::Other
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Diag {
  fn from(_: TooFew<S, Lang>) -> Self {
    Diag::Other
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Diag {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Diag::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for Diag {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    Diag::Other
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unclosed<Delimiter, S, Lang>> for Diag {
  fn from(_: Unclosed<Delimiter, S, Lang>) -> Self {
    Diag::Other
  }
}

impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for Diag
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<Delimiter>(_: Unclosed<Delimiter, L::Span, Lang>) -> Self {
    Diag::Other
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Diag {
  fn from(e: FullContainer<S, Lang>) -> Self {
    Diag::Full(e.nums(), e.capacity())
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Diag {
  fn from(e: TooMany<S, Lang>) -> Self {
    Diag::TooMany(e.nums(), e.limit())
  }
}

// ── Fixture ───────────────────────────────────────────────────────────────────

type VCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<Diag>>;
type FCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Fatal<Diag>>;

type Cap1 = GenericArrayDeque<i64, U1>;
type Cap2 = GenericArrayDeque<i64, U2>;

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, Diag>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>,
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

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, Diag>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>,
{
  match inp.next()? {
    None => Err(Diag::Other),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(Diag::Other),
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

/// One declared driver: its name, its input, and the two runs the law is read from.
type Row = (
  &'static str,
  &'static str,
  fn(&str) -> Vec<Diag>,
  fn(&str) -> Result<(), Diag>,
);

/// Declares a table of repetition drivers.
///
/// Each row is written once, as a builder expression generic over the parse context, and the
/// macro instantiates it twice: under `Verbose`, whose recorded diagnostics are read in
/// **emission** order (the collision's two payloads carry different spans in the delimited rows,
/// so the span-keyed map is not emission order there), and under `Fatal`, whose `Result` *is* the
/// public answer the builder choice used to change.
macro_rules! driver_table {
  ($tab:ident { $( $name:ident : $out:ty = $src:literal , $inp:ident => $build:expr ; )* }) => {
    mod $tab {
      use super::*;

      $(
        fn $name<'inp, Ctx>(
          $inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
        ) -> Result<(), Diag>
        where
          Ctx: ParseContext<'inp, TestLexer<'inp>>,
          Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>
            + SeparatedEmitter<'inp, TestLexer<'inp>>
            + FullContainerEmitter<'inp, TestLexer<'inp>>
            + UnclosedEmitter<'inp, TestLexer<'inp>>
            + TooFewEmitter<'inp, TestLexer<'inp>>
            + TooManyEmitter<'inp, TestLexer<'inp>>
            + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
            + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
            + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
            + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
        {
          let _out: $out = $build;
          Ok(())
        }
      )*

      /// The `Verbose` half: every diagnostic the construct recorded, in emission order.
      pub mod recorded {
        use super::*;

        $(
          pub fn $name(src: &str) -> Vec<Diag> {
            fn go<'inp>(
              inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
            ) -> Result<Vec<Diag>, Diag> {
              super::$name(inp)?;
              Ok(
                inp
                  .emitter_ref()
                  .diagnostics()
                  .filter_map(|d| d.payload().cloned())
                  .collect(),
              )
            }
            Parser::with_context(ParserContext::new(Verbose::new()))
              .apply(go)
              .parse_str(src)
              .expect("a Verbose emitter recovers from every diagnostic in this table")
          }
        )*
      }

      /// The `Fatal` half: the public parse result, which is the first diagnostic emitted.
      pub mod public {
        use super::*;

        $(
          pub fn $name(src: &str) -> Result<(), Diag> {
            fn go<'inp>(
              inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>,
            ) -> Result<(), Diag> {
              super::$name(inp)
            }
            Parser::with_context(ParserContext::new(Fatal::new()))
              .apply(go)
              .parse_str(src)
          }
        )*
      }

      pub const ROWS: &[Row] = &[
        $( (stringify!($name), $src, recorded::$name, public::$name) ),*
      ];
    }
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// The collision table — `at_most(1)`/`bounded(0, 1)`, a capacity-1 destination, and two parsed
// elements, so the SAME element both exceeds the maximum and is refused.
// ═══════════════════════════════════════════════════════════════════════════════

driver_table!(collision {
  repeated_at_most: Cap1 = "1 2", inp =>
    try_num.repeated().at_most(1).collect().parse_input(inp)?;
  repeated_bounded: Cap1 = "1 2", inp =>
    try_num.repeated().bounded(0, 1).collect().parse_input(inp)?;

  repeated_while_at_most: Cap1 = "1 2 +", inp =>
    parse_num.repeated_while::<_, U1>(decide_num::<Ctx>).at_most(1).collect().parse_input(inp)?;
  repeated_while_bounded: Cap1 = "1 2 +", inp =>
    parse_num.repeated_while::<_, U1>(decide_num::<Ctx>).bounded(0, 1).collect().parse_input(inp)?;

  delim_repeated_at_most: Cap1 = "[1 2]", inp =>
    try_num.repeated().at_most(1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;
  delim_repeated_bounded: Cap1 = "[1 2]", inp =>
    try_num.repeated().bounded(0, 1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;

  delim_repeated_while_at_most: Cap1 = "[1 2]", inp =>
    parse_num.repeated_while::<_, U1>(decide_num::<Ctx>).at_most(1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;
  delim_repeated_while_bounded: Cap1 = "[1 2]", inp =>
    parse_num.repeated_while::<_, U1>(decide_num::<Ctx>).bounded(0, 1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;

  sep_at_most: Cap1 = "1,2", inp =>
    try_num.separated_by_comma().at_most(1).collect().parse_input(inp)?;
  sep_bounded: Cap1 = "1,2", inp =>
    try_num.separated_by_comma().bounded(0, 1).collect().parse_input(inp)?;

  sep_delim_at_most: Cap1 = "[1,2]", inp =>
    try_num.separated_by_comma().at_most(1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;
  sep_delim_bounded: Cap1 = "[1,2]", inp =>
    try_num.separated_by_comma().bounded(0, 1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;

  sep_while_at_most: Cap1 = "1,2+", inp =>
    parse_num.separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
      .at_most(1).collect().parse_input(inp)?;
  sep_while_bounded: Cap1 = "1,2+", inp =>
    parse_num.separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
      .bounded(0, 1).collect().parse_input(inp)?;

  sep_while_delim_at_most: Cap1 = "[1,2]", inp =>
    parse_num.separated_by_comma_while::<_, U1>(decide_num::<Ctx>).at_most(1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;
  sep_while_delim_bounded: Cap1 = "[1,2]", inp =>
    parse_num.separated_by_comma_while::<_, U1>(decide_num::<Ctx>).bounded(0, 1)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;

  // The separator policies wrap the same cardinality handler the rows above use, so these rows
  // prove the wrappers forward the element hook rather than intercepting it. Each input carries
  // the separators its policy is named for, so the policy itself contributes no diagnostic.
  sep_allow_trailing_at_most: Cap1 = "1,2,", inp =>
    try_num.separated_by_comma().allow_trailing().at_most(1).collect().parse_input(inp)?;
  sep_allow_leading_at_most: Cap1 = ",1,2", inp =>
    try_num.separated_by_comma().allow_leading().at_most(1).collect().parse_input(inp)?;
  sep_require_trailing_bounded: Cap1 = "1,2,", inp =>
    try_num.separated_by_comma().require_trailing().bounded(0, 1).collect().parse_input(inp)?;
  sep_while_allow_trailing_at_most: Cap1 = "1,2,+", inp =>
    parse_num.separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
      .allow_trailing().at_most(1).collect().parse_input(inp)?;

  // The remaining five separator policies, on the `sep` shape. All nine are now represented, and
  // the four nested ones (`*_surrounded` and the two mixed pairs) are the rows that prove the
  // forward is recursive rather than one level deep.
  sep_require_leading_at_most: Cap1 = ",1,2", inp =>
    try_num.separated_by_comma().require_leading().at_most(1).collect().parse_input(inp)?;
  // The nested policies take the crate's own chain order — inner policy, cardinality, outer
  // policy — the one `sep_parse_extra.rs` uses.
  sep_allow_surrounded_bounded: Cap1 = ",1,2,", inp =>
    try_num.separated_by_comma().allow_trailing().bounded(0, 1).allow_leading()
      .collect().parse_input(inp)?;
  sep_require_surrounded_at_most: Cap1 = ",1,2,", inp =>
    try_num.separated_by_comma().require_trailing().at_most(1).require_leading()
      .collect().parse_input(inp)?;
  sep_allow_leading_require_trailing_at_most: Cap1 = ",1,2,", inp =>
    try_num.separated_by_comma().require_trailing().at_most(1).allow_leading()
      .collect().parse_input(inp)?;
  sep_require_leading_allow_trailing_bounded: Cap1 = ",1,2,", inp =>
    try_num.separated_by_comma().allow_trailing().bounded(0, 1).require_leading()
      .collect().parse_input(inp)?;
});

// ═══════════════════════════════════════════════════════════════════════════════
// The control table — a capacity-2 destination, `at_most(4)`/`bounded(0, 4)`, and six parsed
// elements. The refusal lands on element 3 and the maximum on element 5, so they are DIFFERENT
// elements and the refusal genuinely came first.
//
// Every row here reads `[Full(3, 2), TooMany(5, 4)]` before this change and after it. That is the
// point: it is the row a repair that simply flipped the order everywhere would break.
// ═══════════════════════════════════════════════════════════════════════════════

driver_table!(control {
  repeated_at_most: Cap2 = "1 2 3 4 5 6", inp =>
    try_num.repeated().at_most(4).collect().parse_input(inp)?;
  repeated_bounded: Cap2 = "1 2 3 4 5 6", inp =>
    try_num.repeated().bounded(0, 4).collect().parse_input(inp)?;

  repeated_while_at_most: Cap2 = "1 2 3 4 5 6 +", inp =>
    parse_num.repeated_while::<_, U1>(decide_num::<Ctx>).at_most(4).collect().parse_input(inp)?;

  delim_repeated_at_most: Cap2 = "[1 2 3 4 5 6]", inp =>
    try_num.repeated().at_most(4)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;
  delim_repeated_while_at_most: Cap2 = "[1 2 3 4 5 6]", inp =>
    parse_num.repeated_while::<_, U1>(decide_num::<Ctx>).at_most(4)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;

  sep_at_most: Cap2 = "1,2,3,4,5,6", inp =>
    try_num.separated_by_comma().at_most(4).collect().parse_input(inp)?;
  sep_bounded: Cap2 = "1,2,3,4,5,6", inp =>
    try_num.separated_by_comma().bounded(0, 4).collect().parse_input(inp)?;

  sep_delim_at_most: Cap2 = "[1,2,3,4,5,6]", inp =>
    try_num.separated_by_comma().at_most(4)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;

  sep_while_at_most: Cap2 = "1,2,3,4,5,6+", inp =>
    parse_num.separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
      .at_most(4).collect().parse_input(inp)?;

  sep_while_delim_at_most: Cap2 = "[1,2,3,4,5,6]", inp =>
    parse_num.separated_by_comma_while::<_, U1>(decide_num::<Ctx>).at_most(4)
      .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp)?;
});

// ═══════════════════════════════════════════════════════════════════════════════
// The law, asserted once over each table.
// ═══════════════════════════════════════════════════════════════════════════════

/// Runs one table and reports **every** row that disagrees, not the first.
///
/// A per-row `assert_eq!` stops at row one, which is exactly the wrong shape here: the question
/// this file answers is *which drivers disagree*, and an answer that names one of them cannot
/// distinguish "one driver is wrong" from "eight are".
fn check(table: &[Row], want_recorded: &[Diag], want_public: Result<(), Diag>, law: &str) {
  let mut bad: Vec<String> = Vec::new();

  for (name, src, recorded, public) in table {
    let got = recorded(src);
    if got != want_recorded {
      bad.push(format!(
        "  {name} on {src:?}: Verbose recorded {got:?}, want {want_recorded:?}"
      ));
    }
    let got = public(src);
    if got != want_public {
      bad.push(format!(
        "  {name} on {src:?}: Fatal returned {got:?}, want {want_public:?}"
      ));
    }
  }

  assert!(
    bad.is_empty(),
    "{law}\n{} of {} driver rows disagree:\n{}",
    bad.len(),
    table.len(),
    bad.join("\n"),
  );
}

#[test]
fn one_elements_maximum_is_reported_before_its_destination_refusal() {
  check(
    collision::ROWS,
    &[Diag::TooMany(2, 1), Diag::Full(2, 1)],
    Err(Diag::TooMany(2, 1)),
    "LAW O: an element that both exceeds the maximum and is refused by the destination reports \
     the maximum first, in EVERY driver — the builder the caller picked must not decide which \
     error a fail-fast parse returns (#277)",
  );
}

#[test]
fn a_refusal_on_an_earlier_element_still_comes_first() {
  check(
    control::ROWS,
    &[Diag::Full(3, 2), Diag::TooMany(5, 4)],
    Err(Diag::Full(3, 2)),
    "LAW O is about ONE element: a destination that filled at element 3 really did refuse before \
     the maximum was exceeded at element 5, and every driver must still say so",
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Where the count verdict sits among the separator diagnostics around it.
//
// The separated engines run the admission inside each `handle_continue` arm rather than once
// ahead of the match, and this is the row that says why: a diagnostic about the **separator slot
// preceding** the element became true earlier in the input than the element's own count verdict,
// and one about the slot **following** it became true later. Hoisting the admission to the top of
// the match would report the maximum before a missing separator that precedes it; leaving it in
// the end pass — which is what #277 fixed — reports it after a trailing separator that follows.
// ═══════════════════════════════════════════════════════════════════════════════

driver_table!(separator_before_the_element {
  missing_separator_precedes_the_element: Cap1 = "1 2", inp =>
    try_num.separated_by_comma().at_most(1).collect().parse_input(inp)?;
});

driver_table!(separator_after_the_element {
  trailing_separator: Cap1 = "1,2,", inp =>
    try_num.separated_by_comma().at_most(1).collect().parse_input(inp)?;
});

#[test]
fn the_count_verdict_sits_where_its_fact_became_true() {
  check(
    separator_before_the_element::ROWS,
    &[Diag::Other, Diag::TooMany(2, 1), Diag::Full(2, 1)],
    Err(Diag::Other),
    "the separator slot BEFORE an element is settled before that element parses, so its \
     diagnostic precedes the element's count verdict",
  );
  check(
    separator_after_the_element::ROWS,
    &[Diag::TooMany(2, 1), Diag::Full(2, 1), Diag::Other],
    Err(Diag::TooMany(2, 1)),
    "the separator slot AFTER the last element is settled after it, so its diagnostic follows \
     both of that element's refusals",
  );
}

/// The row count, pinned against `many/mod.rs`'s `ELEMENT_ADMISSION_CENSUS`.
///
/// That census pins how many places in `parser::many` admit an element into a container. The two
/// numbers are not the same quantity — one counts admission sites, the other counts builder
/// shapes — so this is a reminder rather than a derivation: a driver added to the tree moves the
/// census, and the census's failure message sends whoever moved it here.
///
/// The twenty-five collision rows are the eight driver families crossed with `at_most` and
/// `bounded` (sixteen, and the default separator policy is among them), plus the eight
/// non-default separator policies on the `sep` shape and one of them on `sep_while` (nine more).
/// Twenty-one of the twenty-five were red before #277; the four that were already right are the
/// two plain families, which had the hook in front of the push all along.
#[test]
fn the_table_covers_every_declared_repetition_family() {
  assert_eq!(
    collision::ROWS.len(),
    25,
    "the collision table changed size — every repetition builder that can reach both a maximum \
     and a container refusal for one element needs a row here, and `parser::many`'s \
     `ELEMENT_ADMISSION_CENSUS` pins the admission sites those builders reach"
  );
  assert_eq!(control::ROWS.len(), 10, "the control table changed size");
}
