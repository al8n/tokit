#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! The behavioural matrix over every repetition driver — one fixture set, two layers of
//! assertion, and not one expected value written per variant.
//!
//! # Why this file is not the thirty-fourth suite
//!
//! `tokora/tests/` already holds tens of thousands of lines of behavioural tests for these
//! combinators, and `parser::many`'s censuses count the sites those combinators are spelled at.
//! [#259](https://github.com/al8n/tokora/issues/259) names the gap both of them leave, in its own
//! summary:
//!
//! > Those tests validate spelling, markers, counts, and source layout rather than the
//! > behavioural invariant itself. **A bug expressed uniformly across every counted site still
//! > satisfies the census.**
//!
//! The per-variant suites leave the same gap for a different reason. Each was written by looking
//! at what *that* variant does, so its expected values are a transcript of the behaviour it
//! found. A defect present in every variant is in every transcript, and every file stays green.
//!
//! So this file writes **no per-variant expected value at all**. Every assertion is either
//!
//! * a **relation between variants** (layer A) — the same fixture through two builders that must
//!   agree, so a variant that drifts from its siblings reds without anyone knowing what the right
//!   answer was; or
//! * an **absolute property** (layer B) — a statement checkable against an oracle *outside* the
//!   driver, so a defect all eight drivers share still reds.
//!
//! Both are verified by planting rather than asserted. Restoring the owning-collection residue
//! this crate repaired reds 202 of layer B's 480 residue cells across all eight drivers while
//! every one of the crate's 2 025 source censuses stays green; a drift confined to one driver's
//! end-of-construct pass reds layer A alone and leaves layer B untouched.
//!
//! Layer B is the half that answers #259. Layer A cannot see a uniformly wrong implementation:
//! the relations still hold when both sides are wrong the same way.
//!
//! # The oracle, and why it is not the driver
//!
//! Layer B needs to know what *should* be in the container without asking the thing that filled
//! it. The element parsers here are instrumented: every value they hand back is appended to a
//! thread-local log first (`accepted`). The log is written by the element parser, which is test
//! code, and read after the construct returns. Nothing in `parser::many` can move it.
//!
//! That is the whole of the independence claim, and it is worth stating what it does **not**
//! cover: an element the driver parses and then rolls back is in the log and legitimately not in
//! the container. So the conservation property is stated as a **prefix** relation rather than an
//! equality, and `no_element_is_dropped_after_the_last_one_kept` carries the bound on how much of
//! a suffix may legitimately be missing.
//!
//! # One fixture set, rendered per shape
//!
//! A fixture is an abstract element sequence — `1 2 3`, or `1 <fails> 2` — not a source
//! string. Each variant renders it with its own joiner and its own delimiters, so **the same
//! fixture reaches all forty-eight variants**, and a relation between two of them is a relation
//! over one logical input. Adding a variant is one line in `variants!`; it inherits every fixture
//! and every layer-B property without a file of its own. That inheritance is the acceptance
//! criterion #259 asks for.
//!
//! # Sentinel token
//!
//! The `*_while` drivers consult their condition through a lookahead window, and at EOF there is
//! nothing to peek. Non-delimited renderings therefore end in `+`, the convention
//! `parser_repeated_while.rs`, `end_state_parity.rs` and `repetition_diagnostic_order.rs` use.
//! The non-`while` drivers stop on it exactly as they stop on EOF — their element parser declines
//! it — which is what lets one rendering serve both halves of the `while` relation. Delimited
//! renderings need no sentinel: the closer is the stop token.
//!
//! # `Verbose` everywhere but the residue cells
//!
//! A fail-fast emitter turns the first diagnostic into `Err`, which truncates the observation to
//! the point of the first complaint — and most of this file's questions are about what the
//! construct did *after* that point. `Verbose` records and keeps going, so one run yields the
//! collection, the full diagnostic vector, the consumed region and the input's resting position
//! together. `repetition_diagnostic_order.rs` owns the fail-fast half of the diagnostic order.
//!
//! The residue cells are the exception, and the exception is forced: residue is only reachable
//! through an attempt that **fails after admitting elements**, and a recovering emitter is
//! precisely the thing that stops a construct failing. Under `Verbose` only the four `*_while`
//! families ever reach that state, so those cells run under `Fatal`, where a count bound, a
//! refused element or a missing closer takes any of the eight drivers there.

mod common;

use core::cell::RefCell;

use common::{TestLexer, Token, TokenKind};
use tokora::{
  Emitter, EmitterView, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  TryParseInput,
  cache::Peeked,
  emitter::{Fatal, FromUnclosed, Verbose},
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  logos::Logos,
  parser::{Action, Collect},
  punct::Bracket,
  span::SimpleSpan,
  try_parse_input::ParseAttempt,
  utils::typenum::U1,
};

// ══════════════════════════════════════════════════════════════════════════════
// The recorded diagnostic
// ══════════════════════════════════════════════════════════════════════════════

/// One recorded diagnostic, keeping the payloads layer B reads back against the container.
///
/// `TooFew`, `TooMany` and `FullContainer` carry a count the construct computed itself; the
/// container carries the same count computed by pushing. `the_counts_a_construct_reports_are_the
/// _counts_it_collected` is the identity between them, and it needs the numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Diag {
  TooMany(usize, usize),
  TooFew(usize, usize),
  Full(usize, usize),
  Other,
}

macro_rules! other {
  ($($ty:ty),* $(,)?) => {$(
    impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<$ty> for Diag {
      fn from(_: $ty) -> Self { Diag::Other }
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

impl<Set: Clone + 'static> From<UnexpectedEot<usize, (), Set>> for Diag {
  fn from(_: UnexpectedEot<usize, (), Set>) -> Self {
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

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Diag {
  fn from(e: TooFew<S, Lang>) -> Self {
    Diag::TooFew(e.nums(), e.limit())
  }
}

type VCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<Diag>>;
type FCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Fatal<Diag>>;

thread_local! {
  /// The reuse runs' step-over plan: every token start offset of the joined source, and the
  /// index of the token the second construct begins at.
  ///
  /// It travels through a thread-local because the run function is a generic `fn` rather than a
  /// closure — `ParseInput` is implemented for `FnMut(&mut InputRef<'inp, ..>)`, and a closure
  /// annotated for one `'inp` is not the higher-ranked bound `parse_str` needs.
  static PLAN: RefCell<(Vec<usize>, usize)> = const { RefCell::new((Vec::new(), 0)) };
}

// ══════════════════════════════════════════════════════════════════════════════
// The oracle: instrumented element parsers
// ══════════════════════════════════════════════════════════════════════════════

thread_local! {
  /// Every value an element parser handed back on this thread since the last `take_accepts`.
  static ACCEPTS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
}

/// Records one successful element parse and returns it unchanged.
fn accepted(v: i64) -> i64 {
  ACCEPTS.with(|a| a.borrow_mut().push(v));
  v
}

/// Drains the log. Called immediately before and immediately after each construct, so what comes
/// back belongs to that construct and to nothing else.
fn take_accepts() -> Vec<i64> {
  ACCEPTS.with(|a| core::mem::take(&mut *a.borrow_mut()))
}

/// The value an element parser refuses. Rendered as `-9`, which the shared lexer's number
/// pattern
/// takes as a single `Num`, so it reaches the element parser as an element rather than as a token
/// the driver itself rejects — an **element failure**, which is the axis it exists for.
const BAD: i64 = -9;

/// The lookahead-bearing element parser: declines what it does not match without consuming, and
/// fails outright on `BAD`.
fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, Diag>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>,
{
  match inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? {
    None => Ok(ParseAttempt::Decline),
    Some(tok) => match tok.into_data() {
      Token::Num(BAD) => Err(Diag::Other),
      Token::Num(n) => Ok(ParseAttempt::Accept(accepted(n))),
      _ => unreachable!(),
    },
  }
}

/// The committed element parser the `*_while` drivers take: no lookahead of its own, so the
/// driver's condition decides where the collection stops.
fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, Diag>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = Diag>,
{
  match inp.next()? {
    None => Err(Diag::Other),
    Some(tok) => match tok.into_data() {
      Token::Num(BAD) => Err(Diag::Other),
      Token::Num(n) => Ok(accepted(n)),
      _ => Err(Diag::Other),
    },
  }
}

/// The `*_while` condition, written to encode **the same predicate** `try_num`'s decline encodes:
/// continue iff the next token is a number.
///
/// That equality of predicates is what makes the `while` half of layer A a relation rather than a
/// coincidence — the two drivers are asked the same question through two different mechanisms and
/// must answer it in the same place.
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

// ══════════════════════════════════════════════════════════════════════════════
// Fixtures: abstract element sequences, rendered per shape
// ══════════════════════════════════════════════════════════════════════════════

/// One cell of a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
  /// An element both element parsers accept.
  Ok(i64),
  /// An element both element parsers **fail** on, after consuming it.
  Bad,
  /// A token neither element parser matches: `try_num` declines it, `decide_num` stops on it.
  Junk,
}

/// How a variant lays its elements out in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Joiner {
  /// The plain families: elements sit next to each other.
  Space,
  /// The separated families: elements sit either side of a comma.
  Comma,
}

/// One abstract input, rendered into concrete source once per variant.
struct Fixture {
  name: &'static str,
  cells: &'static [Cell],
}

impl Fixture {
  /// Lays this fixture out as source for one variant: the elements joined by the variant's
  /// separator, inside its delimiters, followed by the stop token a non-delimited `*_while`
  /// driver needs something to peek at.
  fn render(&self, joiner: Joiner, delimited: bool) -> String {
    let sep = match joiner {
      Joiner::Space => " ",
      Joiner::Comma => ",",
    };
    let body = self
      .cells
      .iter()
      .map(|c| match c {
        Cell::Ok(n) => n.to_string(),
        Cell::Bad => BAD.to_string(),
        Cell::Junk => "*".to_string(),
      })
      .collect::<Vec<_>>()
      .join(sep);
    if delimited {
      format!("[{body}]")
    } else {
      // The `*_while` drivers need a token to peek at where the collection ends; the others
      // decline it exactly as they decline EOF.
      format!("{body} +")
    }
  }

  /// Whether this fixture contains a cell no element parser accepts, in any position.
  fn is_clean(&self) -> bool {
    self.cells.iter().all(|c| matches!(c, Cell::Ok(_)))
  }
}

const FIXTURES: &[Fixture] = &[
  Fixture {
    name: "empty",
    cells: &[],
  },
  Fixture {
    name: "one",
    cells: &[Cell::Ok(1)],
  },
  Fixture {
    name: "two",
    cells: &[Cell::Ok(1), Cell::Ok(2)],
  },
  Fixture {
    name: "three",
    cells: &[Cell::Ok(1), Cell::Ok(2), Cell::Ok(3)],
  },
  Fixture {
    name: "four",
    cells: &[Cell::Ok(1), Cell::Ok(2), Cell::Ok(3), Cell::Ok(4)],
  },
  Fixture {
    name: "five",
    cells: &[
      Cell::Ok(1),
      Cell::Ok(2),
      Cell::Ok(3),
      Cell::Ok(4),
      Cell::Ok(5),
    ],
  },
  Fixture {
    name: "bad_first",
    cells: &[Cell::Bad, Cell::Ok(2), Cell::Ok(3)],
  },
  Fixture {
    name: "bad_middle",
    cells: &[Cell::Ok(1), Cell::Bad, Cell::Ok(3)],
  },
  Fixture {
    name: "junk_first",
    cells: &[Cell::Junk, Cell::Ok(2)],
  },
  Fixture {
    name: "junk_middle",
    cells: &[Cell::Ok(1), Cell::Junk, Cell::Ok(3)],
  },
];

/// The second construct every reuse run ends with: two elements no fixture uses, so residue from
/// the first attempt shows up in the values as well as in the length. Two is the count every
/// cardinality in the table accepts, which is what lets the second attempt succeed however the
/// first one failed.
const TAIL: Fixture = Fixture {
  name: "tail",
  cells: &[Cell::Ok(7), Cell::Ok(8)],
};

/// The source's tokens, lexed **without** tokora: the independent side of the position property.
fn lexed(src: &str) -> Vec<(TokenKind, usize)> {
  let mut out = Vec::new();
  let mut lex = Token::lexer(src);
  while let Some(tok) = lex.next() {
    let span = lex.span();
    out.push((
      TokenKind::from(&tok.expect("the fixture renderings lex cleanly")),
      span.start,
    ));
  }
  out
}

// ══════════════════════════════════════════════════════════════════════════════
// The observation
// ══════════════════════════════════════════════════════════════════════════════

/// Everything one run of one variant over one fixture makes visible to a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Obs {
  /// What the construct returned, on the success arm.
  elements: Option<Vec<i64>>,
  /// Every diagnostic the construct recorded, in emission order.
  diags: Vec<Diag>,
  /// The region the construct consumed, measured by the **test** from a cursor taken before it
  /// ran — never read off the construct's own reported span, which is one of the things under
  /// test.
  consumed: (usize, usize),
  /// The tokens still in the input when the construct returned, drained afterwards.
  rest: Vec<TokenKind>,
  /// The element parser's own log for this run — the oracle.
  accepts: Vec<i64>,
}

/// The part of an observation two variants must agree on when a layer-A relation holds between
/// them.
type Agreed<'a> = (
  &'a Option<Vec<i64>>,
  &'a [Diag],
  (usize, usize),
  &'a [TokenKind],
);

/// What one reuse run reports: the second attempt's collection, and the second attempt's accept
/// log.
type Reuse = (Option<Vec<i64>>, Vec<i64>);

/// What one borrowed-destination run reports: the caller's container as it stands whatever the
/// construct returned, whether the construct failed, and this run's accept log.
type BorrowedRun = (Vec<i64>, bool, Vec<i64>);

impl Obs {
  /// The part two variants must agree on when a layer-A relation holds between them.
  ///
  /// The accept log is excluded deliberately: two drivers reaching the same answer are entitled
  /// to have asked the element parser a different number of times — `try_num` is offered the stop
  /// token and declines it, while `decide_num` sees it first and never calls `parse_num` at all.
  fn agreed(&self) -> Agreed<'_> {
    (&self.elements, &self.diags, self.consumed, &self.rest)
  }
}

/// One variant of the repetition matrix.
struct Variant {
  name: &'static str,
  joiner: Joiner,
  delimited: bool,
  /// The cardinality bound this row declares, as `(minimum, maximum)`. Read by the layer-A bound
  /// relations; layer B never consults it.
  bound: (Option<usize>, Option<usize>),
  /// One run over the source, through the owning destination — the mainstream `.collect()`
  /// surface, and the only one all eight drivers expose.
  run: fn(&str) -> Obs,
  /// One owning collector run over two constructs in turn — the fixture's rendering, then a
  /// second, always-satisfiable one. What comes back is the **second** attempt's collection
  /// beside the **second** attempt's accept log.
  reused: fn(&str, &str) -> Reuse,
}

impl Variant {
  fn render(&self, fx: &Fixture) -> String {
    fx.render(self.joiner, self.delimited)
  }

  /// The variant's stem — its driver — with the cardinality suffix removed.
  fn stem(&self) -> &'static str {
    self
      .name
      .split_once('_')
      .map(|(s, _)| s)
      .unwrap_or(self.name)
  }
}

/// Declares the variant table.
///
/// A row is one line: its name, its layout, its declared bound, and the builder expression. The
/// macro instantiates that expression twice — once for a single attempt and once for the reuse
/// pair — so a driver, cardinality or separator policy added to `parser::many` inherits every
/// fixture and every property in this file by gaining **one line here**, and by nothing else.
/// That inheritance is what #259's "generated behavioural matrix" acceptance criterion asks for,
/// and it is the reason the criterion is about inheritance rather than about coverage: the thirty
/// -odd per-variant suites already have the coverage.
macro_rules! variants {
  ($( $name:ident : $joiner:ident $delimited:literal ($min:expr, $max:expr) = $build:expr ; )*) => {
    $(
      mod $name {
        use super::*;

        pub fn run(src: &str) -> Obs {
          fn go<'inp>(
            inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
          ) -> Result<Obs, Diag> {
            type Ctx<'a> = VCtx<'a>;
            let _ = take_accepts();
            let anchor = inp.cursor().clone();
            let mut collect = Collect::new($build, Vec::<i64>::new());
            let out: Result<Vec<i64>, Diag> = collect.parse_input(inp);
            let accepts = take_accepts();
            let region = inp.span_since(&anchor);
            let consumed = (region.start(), region.end());
            let diags = inp
              .emitter_ref()
              .diagnostics()
              .filter_map(|d| d.payload().cloned())
              .collect();
            let mut rest = Vec::new();
            while let Some(tok) = inp.next()? {
              rest.push(tokora::Token::kind(tok.data()));
            }
            Ok(Obs { elements: out.ok(), diags, consumed, rest, accepts })
          }
          Parser::with_context(ParserContext::new(Verbose::new()))
            .apply(go)
            .parse_str(src)
            .expect("a Verbose emitter records rather than refuses")
        }

        /// One owning collector, two attempts, and only the **second** one's answer.
        ///
        /// The source is the fixture's rendering followed by `tail` — a two-element construct of
        /// the same shape, which every cardinality in the table accepts. So the first attempt is
        /// free to fail wherever the fixture makes it fail, and the second attempt still has a
        /// construct it can complete: the cell asks its question of every variant rather than
        /// only of the ones whose first attempt stops in a convenient place. Between the two, the
        /// input is walked to the start of `tail`, however far the first attempt got.
        ///
        /// It runs under `Fatal` rather than `Verbose`, and that is the point: residue is only
        /// reachable through an attempt that **fails after admitting elements**, and a recovering
        /// emitter is precisely the thing that stops a construct failing. Under `Fatal` a count
        /// bound, a refused element or a missing closer takes any of the eight drivers there.
        ///
        /// `tail`'s elements are 7 and 8, which no fixture uses, so residue is visible in the
        /// values and not only in the length.
        pub fn reused(src: &str, tail: &str) -> Reuse {
          fn go<'inp>(
            inp: &mut InputRef<'inp, '_, TestLexer<'inp>, FCtx<'inp>>,
          ) -> Result<Reuse, Diag> {
            type Ctx<'a> = FCtx<'a>;
            let mut collect = Collect::new($build, Vec::<i64>::new());
            let anchor = inp.cursor().clone();
            let _ = take_accepts();
            let first: Result<Vec<i64>, Diag> = collect.parse_input(inp);
            let _ = first;
            let _ = take_accepts();
            let end = inp.span_since(&anchor).end();
            let (starts, tail_at) = PLAN.with(|p| p.borrow().clone());
            let consumed = starts.iter().filter(|s| **s < end).count();
            for _ in consumed..tail_at {
              if inp.next()?.is_none() {
                break;
              }
            }
            let second: Result<Vec<i64>, Diag> = collect.parse_input(inp);
            let accepts = take_accepts();
            Ok((second.ok(), accepts))
          }
          let joined = format!("{src} {tail}");
          let starts: Vec<usize> = lexed(&joined).into_iter().map(|(_, s)| s).collect();
          PLAN.with(|p| *p.borrow_mut() = (starts, lexed(src).len()));
          Parser::with_context(ParserContext::new(Fatal::new()))
            .apply(go)
            .parse_str(&joined)
            .expect("the reuse run reports through its return value, never through the driver")
        }
      }
    )*

    const VARIANTS: &[Variant] = &[
      $( Variant {
        name: stringify!($name),
        joiner: Joiner::$joiner,
        delimited: $delimited,
        bound: ($min, $max),
        run: $name::run,
        reused: $name::reused,
      } ),*
    ];
  };
}

// ── The table ─────────────────────────────────────────────────────────────────
//
// Eight drivers — `repeated`, `repeated_while`, `separated`, `separated_while`, and the delimited
// form of each — crossed with the cardinalities each of them accepts. The `exact2_*` rows are one
// cardinality reached by three construction paths, which is what makes them a relation rather
// than a repetition; `Separated` and `SeparatedWhile` expose no `AtLeast -> AtMost` chaining, so
// those two drivers carry the `bounded(n, n)` path alone.

variants! {
  // ── repeated ────────────────────────────────────────────────────────────────
  rep_unb: Space false (None, None) = try_num::<Ctx<'_>>.repeated();
  rep_min2: Space false (Some(2), None) = try_num::<Ctx<'_>>.repeated().at_least(2);
  rep_max2: Space false (None, Some(2)) = try_num::<Ctx<'_>>.repeated().at_most(2);
  rep_bnd13: Space false (Some(1), Some(3)) = try_num::<Ctx<'_>>.repeated().bounded(1, 3);
  rep_exact2b: Space false (Some(2), Some(2)) = try_num::<Ctx<'_>>.repeated().bounded(2, 2);
  rep_exact2lm: Space false (Some(2), Some(2)) =
    try_num::<Ctx<'_>>.repeated().at_least(2).at_most(2);
  rep_exact2ml: Space false (Some(2), Some(2)) =
    try_num::<Ctx<'_>>.repeated().at_most(2).at_least(2);

  // ── repeated_while ──────────────────────────────────────────────────────────
  repw_unb: Space false (None, None) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>);
  repw_min2: Space false (Some(2), None) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2);
  repw_max2: Space false (None, Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2);
  repw_bnd13: Space false (Some(1), Some(3)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(1, 3);
  repw_exact2b: Space false (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(2, 2);
  repw_exact2lm: Space false (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2).at_most(2);
  repw_exact2ml: Space false (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2).at_least(2);

  // ── separated ───────────────────────────────────────────────────────────────
  sep_unb: Comma false (None, None) = try_num::<Ctx<'_>>.separated_by_comma();
  sep_min2: Comma false (Some(2), None) = try_num::<Ctx<'_>>.separated_by_comma().at_least(2);
  sep_max2: Comma false (None, Some(2)) = try_num::<Ctx<'_>>.separated_by_comma().at_most(2);
  sep_bnd13: Comma false (Some(1), Some(3)) =
    try_num::<Ctx<'_>>.separated_by_comma().bounded(1, 3);
  sep_exact2b: Comma false (Some(2), Some(2)) =
    try_num::<Ctx<'_>>.separated_by_comma().bounded(2, 2);

  // ── separated_while ─────────────────────────────────────────────────────────
  sepw_unb: Comma false (None, None) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>);
  sepw_min2: Comma false (Some(2), None) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2);
  sepw_max2: Comma false (None, Some(2)) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2);
  sepw_bnd13: Comma false (Some(1), Some(3)) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(1, 3);
  sepw_exact2b: Comma false (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(2, 2);

  // ── delimited repeated ──────────────────────────────────────────────────────
  drep_unb: Space true (None, None) =
    try_num::<Ctx<'_>>.repeated().delimited::<Bracket<(), (), ()>>();
  drep_min2: Space true (Some(2), None) =
    try_num::<Ctx<'_>>.repeated().at_least(2).delimited::<Bracket<(), (), ()>>();
  drep_max2: Space true (None, Some(2)) =
    try_num::<Ctx<'_>>.repeated().at_most(2).delimited::<Bracket<(), (), ()>>();
  drep_bnd13: Space true (Some(1), Some(3)) =
    try_num::<Ctx<'_>>.repeated().bounded(1, 3).delimited::<Bracket<(), (), ()>>();
  drep_exact2b: Space true (Some(2), Some(2)) =
    try_num::<Ctx<'_>>.repeated().bounded(2, 2).delimited::<Bracket<(), (), ()>>();
  drep_exact2lm: Space true (Some(2), Some(2)) =
    try_num::<Ctx<'_>>.repeated().at_least(2).at_most(2).delimited::<Bracket<(), (), ()>>();
  drep_exact2ml: Space true (Some(2), Some(2)) =
    try_num::<Ctx<'_>>.repeated().at_most(2).at_least(2).delimited::<Bracket<(), (), ()>>();

  // ── delimited repeated_while ────────────────────────────────────────────────
  drepw_unb: Space true (None, None) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>)
      .delimited::<Bracket<(), (), ()>>();
  drepw_min2: Space true (Some(2), None) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2)
      .delimited::<Bracket<(), (), ()>>();
  drepw_max2: Space true (None, Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2)
      .delimited::<Bracket<(), (), ()>>();
  drepw_bnd13: Space true (Some(1), Some(3)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(1, 3)
      .delimited::<Bracket<(), (), ()>>();
  drepw_exact2b: Space true (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(2, 2)
      .delimited::<Bracket<(), (), ()>>();
  drepw_exact2lm: Space true (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2).at_most(2)
      .delimited::<Bracket<(), (), ()>>();
  drepw_exact2ml: Space true (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2).at_least(2)
      .delimited::<Bracket<(), (), ()>>();

  // ── delimited separated ─────────────────────────────────────────────────────
  dsep_unb: Comma true (None, None) =
    try_num::<Ctx<'_>>.separated_by_comma().delimited::<Bracket<(), (), ()>>();
  dsep_min2: Comma true (Some(2), None) =
    try_num::<Ctx<'_>>.separated_by_comma().at_least(2).delimited::<Bracket<(), (), ()>>();
  dsep_max2: Comma true (None, Some(2)) =
    try_num::<Ctx<'_>>.separated_by_comma().at_most(2).delimited::<Bracket<(), (), ()>>();
  dsep_bnd13: Comma true (Some(1), Some(3)) =
    try_num::<Ctx<'_>>.separated_by_comma().bounded(1, 3).delimited::<Bracket<(), (), ()>>();
  dsep_exact2b: Comma true (Some(2), Some(2)) =
    try_num::<Ctx<'_>>.separated_by_comma().bounded(2, 2).delimited::<Bracket<(), (), ()>>();

  // ── delimited separated_while ───────────────────────────────────────────────
  dsepw_unb: Comma true (None, None) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>)
      .delimited::<Bracket<(), (), ()>>();
  dsepw_min2: Comma true (Some(2), None) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2)
      .delimited::<Bracket<(), (), ()>>();
  dsepw_max2: Comma true (None, Some(2)) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2)
      .delimited::<Bracket<(), (), ()>>();
  dsepw_bnd13: Comma true (Some(1), Some(3)) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(1, 3)
      .delimited::<Bracket<(), (), ()>>();
  dsepw_exact2b: Comma true (Some(2), Some(2)) =
    parse_num::<Ctx<'_>>.separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(2, 2)
      .delimited::<Bracket<(), (), ()>>();
}

// ── The borrowed destination ──────────────────────────────────────────────────
//
// `Collect<&mut _, &mut Container>` is the other ownership contract: the caller keeps the
// storage and can read it on the **failure** arm, which the owning form cannot expose. Six of the
// eight drivers implement it; `delim/repeated` and `delim/repeated_while` implement only the
// owning form, so they have no row here. That asymmetry is a fact about the crate rather than a
// gap in this file, and it is the reason the main table above is the owning one.

/// One row of the borrowed-destination table.
struct Borrowed {
  name: &'static str,
  joiner: Joiner,
  delimited: bool,
  /// The name of the main table's row this one is the borrowed twin of.
  twin: &'static str,
  /// Returns the container as the caller can see it — on **both** arms — beside whether the
  /// construct failed and this run's accept log.
  run: fn(&str) -> BorrowedRun,
}

/// Declares the borrowed-destination table.
///
/// The two spellings the crate uses are both reachable from one row: `repeated`'s borrowed impl
/// takes `&mut Repeated<F, ..>`, while the separated families' take `Separated<&mut F, ..>`, so a
/// row that needs the second writes `&mut $t` where the first writes `$t`. The binding names come
/// in as macro arguments because `macro_rules` hygiene would otherwise put the macro's `let` and
/// the row's mention of it in different scopes.
macro_rules! borrowed_variants {
  ($t:ident, $p:ident => {
    $( $name:ident : $joiner:ident $delimited:literal ~ $twin:ident = $build:expr ; )*
  }) => {
    $(
      mod $name {
        use super::*;

        pub fn run(src: &str) -> BorrowedRun {
          fn go<'inp>(
            inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
          ) -> Result<BorrowedRun, Diag> {
            type Ctx<'a> = VCtx<'a>;
            let _ = take_accepts();
            #[allow(unused_mut, unused_variables)]
            let mut $t = try_num::<Ctx<'_>>;
            #[allow(unused_mut, unused_variables)]
            let mut $p = parse_num::<Ctx<'_>>;
            let mut container = Vec::<i64>::new();
            let mut inner = $build;
            let out: Result<SimpleSpan, Diag> = {
              let mut collect = Collect::new(&mut inner, &mut container);
              collect.parse_input(inp)
            };
            Ok((container, out.is_err(), take_accepts()))
          }
          Parser::with_context(ParserContext::new(Verbose::new()))
            .apply(go)
            .parse_str(src)
            .expect("a Verbose emitter records rather than refuses")
        }
      }
    )*

    const BORROWED: &[Borrowed] = &[
      $( Borrowed {
        name: stringify!($name),
        joiner: Joiner::$joiner,
        delimited: $delimited,
        twin: stringify!($twin),
        run: $name::run,
      } ),*
    ];
  };
}

borrowed_variants! { te, pe => {
  b_rep_unb: Space false ~ rep_unb = te.repeated();
  b_rep_min2: Space false ~ rep_min2 = te.repeated().at_least(2);
  b_rep_max2: Space false ~ rep_max2 = te.repeated().at_most(2);
  b_rep_bnd13: Space false ~ rep_bnd13 = te.repeated().bounded(1, 3);

  b_repw_unb: Space false ~ repw_unb = pe.repeated_while::<_, U1>(decide_num::<Ctx<'_>>);
  b_repw_min2: Space false ~ repw_min2 =
    pe.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2);
  b_repw_max2: Space false ~ repw_max2 =
    pe.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2);
  b_repw_bnd13: Space false ~ repw_bnd13 =
    pe.repeated_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(1, 3);

  b_sep_unb: Comma false ~ sep_unb = (&mut te).separated_by_comma();
  b_sep_min2: Comma false ~ sep_min2 = (&mut te).separated_by_comma().at_least(2);
  b_sep_max2: Comma false ~ sep_max2 = (&mut te).separated_by_comma().at_most(2);
  b_sep_bnd13: Comma false ~ sep_bnd13 = (&mut te).separated_by_comma().bounded(1, 3);

  b_sepw_unb: Comma false ~ sepw_unb =
    (&mut pe).separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>);
  b_sepw_min2: Comma false ~ sepw_min2 =
    (&mut pe).separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2);
  b_sepw_max2: Comma false ~ sepw_max2 =
    (&mut pe).separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2);
  b_sepw_bnd13: Comma false ~ sepw_bnd13 =
    (&mut pe).separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).bounded(1, 3);

  b_dsep_unb: Comma true ~ dsep_unb =
    (&mut te).separated_by_comma().delimited::<Bracket<(), (), ()>>();
  b_dsep_min2: Comma true ~ dsep_min2 =
    (&mut te).separated_by_comma().at_least(2).delimited::<Bracket<(), (), ()>>();
  b_dsep_max2: Comma true ~ dsep_max2 =
    (&mut te).separated_by_comma().at_most(2).delimited::<Bracket<(), (), ()>>();

  b_dsepw_unb: Comma true ~ dsepw_unb =
    (&mut pe).separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>)
      .delimited::<Bracket<(), (), ()>>();
  b_dsepw_min2: Comma true ~ dsepw_min2 =
    (&mut pe).separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_least(2)
      .delimited::<Bracket<(), (), ()>>();
  b_dsepw_max2: Comma true ~ dsepw_max2 =
    (&mut pe).separated_by_comma_while::<_, U1>(decide_num::<Ctx<'_>>).at_most(2)
      .delimited::<Bracket<(), (), ()>>();
}}

// ══════════════════════════════════════════════════════════════════════════════
// The failure reporter
// ══════════════════════════════════════════════════════════════════════════════

/// Collects every disagreeing cell and reports them together.
///
/// A per-cell `assert!` stops at the first one, which is the wrong shape for this file: the
/// question a red matrix answers is *which* variants disagree, and an answer naming one cannot
/// tell "one driver drifted" from "all of them share a defect". That distinction is the whole
/// difference between the two layers.
struct Report {
  law: &'static str,
  cells: usize,
  bad: Vec<String>,
}

impl Report {
  fn new(law: &'static str) -> Self {
    Self {
      law,
      cells: 0,
      bad: Vec::new(),
    }
  }

  fn check(&mut self, ok: bool, cell: impl FnOnce() -> String) {
    self.cells += 1;
    if !ok {
      self.bad.push(format!("  {}", cell()));
    }
  }

  fn finish(self) {
    assert!(self.cells > 0, "{}: the matrix ran zero cells", self.law);
    assert!(
      self.bad.is_empty(),
      "{}\n{} of {} cells disagree:\n{}",
      self.law,
      self.bad.len(),
      self.cells,
      self.bad.join("\n"),
    );
  }
}

/// `needle` is a prefix of `hay`, element for element.
fn is_prefix(needle: &[i64], hay: &[i64]) -> bool {
  needle.len() <= hay.len() && needle == &hay[..needle.len()]
}

fn variant(name: &str) -> &'static Variant {
  VARIANTS
    .iter()
    .find(|v| v.name == name)
    .unwrap_or_else(|| panic!("no variant named {name}"))
}

// ══════════════════════════════════════════════════════════════════════════════
// LAYER B — absolute properties
//
// Each is true of every variant on its own and is checked against something outside
// `parser::many`, so it is still red when all forty-nine rows share one wrong behaviour. This is
// the layer #259 asks for, and the layer A below structurally cannot supply.
// ══════════════════════════════════════════════════════════════════════════════

/// **B1 — conservation.** A construct returns elements its element parser actually produced, in
/// the order it produced them, with nothing inserted, reordered, duplicated or carried in.
///
/// Stated as "a prefix of this run's accept log", which is the strongest form true of every
/// driver: a driver may parse an element speculatively and roll it back, and a full destination
/// refuses everything past its capacity, but both of those remove only a **suffix**. Anything
/// else — a value from an earlier attempt, a value the element parser never returned, two copies
/// of one element, a gap in the middle — stops the container being a prefix.
#[test]
fn a_construct_returns_a_prefix_of_what_its_element_parser_produced() {
  let mut r = Report::new(
    "LAYER B1 — conservation: a collection may hold only elements this attempt's element parser \
     produced, in that order, with no gaps and nothing carried in from anywhere else",
  );
  for v in VARIANTS {
    for fx in FIXTURES {
      let src = v.render(fx);
      let obs = (v.run)(&src);
      let Some(elements) = obs.elements.as_ref() else {
        continue;
      };
      r.check(is_prefix(elements, &obs.accepts), || {
        format!(
          "{}/{} on {src:?}: returned {elements:?}, which is not a prefix of the accept log {:?}",
          v.name, fx.name, obs.accepts
        )
      });
    }
  }
  r.finish();
}

/// **B2 — count identity.** Every count a construct reports about itself is the count of what it
/// collected.
///
/// The three payload-bearing diagnostics each carry a number the driver computed from its own
/// element counter; the returned collection carries the same number, computed by pushing. Two
/// independent derivations of one quantity:
///
/// * `TooFew(nums, min)` — the whole construct's final count, and the destination here is a `Vec`
///   that refuses nothing, so it is the collection's length. And `nums < min`, or the complaint
///   is about a minimum that was met.
/// * `TooMany(nums, limit)` — settled at the element that broke the bound, which is element
///   `limit + 1` and no other (`many::admit_element`: the hook fires iff some element saw the
///   pre-element count equal to the maximum, so it fires exactly once and at that element).
/// * `Full(nums, capacity)` — a `Vec` refuses nothing, so a capacity report here is a report of a
///   refusal that did not happen.
#[test]
fn the_counts_a_construct_reports_are_the_counts_it_collected() {
  let mut r = Report::new(
    "LAYER B2 — count identity: a construct's own count of its elements and the number of \
     elements it returned are two derivations of one quantity and must agree",
  );
  for v in VARIANTS {
    for fx in FIXTURES {
      let src = v.render(fx);
      let obs = (v.run)(&src);
      let Some(elements) = obs.elements.as_ref() else {
        continue;
      };
      for d in &obs.diags {
        match *d {
          Diag::TooFew(nums, min) => r.check(nums == elements.len() && nums < min, || {
            format!(
              "{}/{} on {src:?}: TooFew({nums}, {min}) against a returned collection of {} \
               elements {elements:?}",
              v.name,
              fx.name,
              elements.len()
            )
          }),
          Diag::TooMany(nums, limit) => r.check(nums == limit + 1, || {
            format!(
              "{}/{} on {src:?}: TooMany({nums}, {limit}) — a maximum is broken by element {} and \
               by no other",
              v.name,
              fx.name,
              limit + 1
            )
          }),
          Diag::Full(nums, cap) => r.check(false, || {
            format!(
              "{}/{} on {src:?}: Full({nums}, {cap}) from a `Vec` destination, which refuses \
               nothing",
              v.name, fx.name
            )
          }),
          Diag::Other => {}
        }
      }
    }
  }
  r.finish();
}

/// **B3 — position: the input rests exactly at the boundary of what was consumed.**
///
/// Both sides are computed outside `parser::many`. The right-hand side is the fixture's source
/// re-lexed with `logos` directly and cut at the end of the consumed region; the left-hand side
/// is what the input still held, drained token by token after the construct returned. A driver
/// whose lookahead swallowed a token it did not commit, or that left a token behind the boundary
/// it claims to have reached, separates them.
///
/// The consumed region itself is measured by this file — a cursor taken before the construct and
/// `span_since` after it — rather than read off the construct's own reported span, which is one
/// of the things under test.
#[test]
fn the_input_rests_at_the_end_of_what_was_consumed() {
  let mut r = Report::new(
    "LAYER B3 — position: the tokens an attempt leaves behind are exactly the tokens of its \
     source that begin at or after the point it stopped consuming",
  );
  for v in VARIANTS {
    for fx in FIXTURES {
      let src = v.render(fx);
      let obs = (v.run)(&src);
      let (_, end) = obs.consumed;
      let want: Vec<TokenKind> = lexed(&src)
        .into_iter()
        .filter(|(_, start)| *start >= end)
        .map(|(k, _)| k)
        .collect();
      r.check(want == obs.rest, || {
        format!(
          "{}/{} on {src:?}: consumption ended at {end}, so {want:?} should remain — the input \
           held {:?}",
          v.name, fx.name, obs.rest
        )
      });
    }
  }
  r.finish();
}

/// **B4 — the documented resting position on success: just past the last element kept.**
///
/// The position B3 pins is *consistent*; this one says **where**, derived from the fixture rather
/// than from the driver. Over a fixture whose cells are all acceptable, a construct that returned
/// `k` elements left exactly the tokens that follow the `k`-th rendered element — nothing, for a
/// delimited variant, whose closer it consumed. A driver that reads one element too far, or that
/// eats the separator following its last element, changes what is left and reds here while B3
/// stays green.
///
/// # Why the statement is about tokens rather than about an offset
///
/// The two families do not stop at the same *byte*. After `repeated` returns, the input's
/// lookahead cursor sits at the end of the last token it consumed; after `separated` returns, it
/// sits at the start of the next token, past the whitespace between them, because the separator
/// slot was peeked. `InputRef::span_since` — which is what the drivers themselves build every
/// reported span from — reads that cursor, so the difference reaches the spans a caller sees.
/// It is a real cross-family difference and it belongs to #259's stage 2 verdict on which driver
/// differences are essential; it is **not** a difference about which tokens the construct took,
/// and that is the question this property asks.
///
/// Stated over clean fixtures only. On a fixture containing an element the parser refuses, where
/// a construct rests is a **policy** difference between the resilient and non-resilient families,
/// and `the_while_and_try_families_diverge_on_an_element_failure` is where that difference is
/// asserted instead of averaged over.
#[test]
fn a_successful_construct_rests_just_past_the_last_element_it_kept() {
  let mut r = Report::new(
    "LAYER B4 — a construct that returned k elements left exactly the tokens following the k-th \
     of them, having consumed no further",
  );
  for v in VARIANTS {
    for fx in FIXTURES.iter().filter(|f| f.is_clean()) {
      let src = v.render(fx);
      let obs = (v.run)(&src);
      let Some(elements) = obs.elements.as_ref() else {
        continue;
      };
      let toks = lexed(&src);
      // The rendering is `<e1><sep><e2>… +` or `[<e1><sep><e2>…]`, so element `i` sits at token
      // index `base + stride * i`, where `base` steps over a delimited variant's opener and
      // `stride` is 2 when a separator token stands between neighbours and 1 when they are only
      // whitespace apart.
      let base = usize::from(v.delimited);
      let stride = 1 + usize::from(v.joiner == Joiner::Comma);
      let want: Vec<TokenKind> = if v.delimited {
        // The closer is the construct's own last token, so a delimited variant that succeeded
        // leaves nothing.
        Vec::new()
      } else if elements.is_empty() {
        toks.iter().map(|(k, _)| *k).collect()
      } else {
        let after = base + stride * (elements.len() - 1) + 1;
        toks[after..].iter().map(|(k, _)| *k).collect()
      };
      r.check(want == obs.rest, || {
        format!(
          "{}/{} on {src:?}: kept {elements:?}, so {want:?} must remain — the input held {:?}",
          v.name, fx.name, obs.rest
        )
      });
    }
  }
  r.finish();
}

/// **B5 — a construct starts where it was entered.**
///
/// Every row here is driven from offset zero. A consumed region that begins anywhere else means
/// tokens were passed over before the construct's own accounting started — which is what an
/// anchor taken *after* the opening delimiter would produce in the four delimited rows.
#[test]
fn a_construct_consumes_from_where_it_was_entered() {
  let mut r = Report::new("LAYER B5 — a construct's consumed region begins at its first token");
  for v in VARIANTS {
    for fx in FIXTURES {
      let src = v.render(fx);
      let obs = (v.run)(&src);
      r.check(obs.consumed.0 == 0, || {
        format!(
          "{}/{} on {src:?}: entered at offset 0, consumed region starts at {}",
          v.name, fx.name, obs.consumed.0
        )
      });
    }
  }
  r.finish();
}

/// **B6 — no element is dropped after the last one kept.**
///
/// B1 permits the collection to be a *strict* prefix of the accept log, because a rolled-back
/// element is real. This closes that gap where the destination refuses nothing and every cell is
/// acceptable: the driver keeps everything it parsed **except at most one** — the single
/// speculative element a lookahead-of-one driver may parse before concluding the collection has
/// ended.
///
/// One is the bound these drivers' shape permits, not the number that happened to be measured: a
/// driver that discards two elements it successfully parsed has thrown away work no rollback of
/// its own can account for.
#[test]
fn no_element_is_dropped_after_the_last_one_kept() {
  let mut r = Report::new(
    "LAYER B6 — a driver may speculate one element ahead and roll it back; discarding more than \
     that loses a parse the element parser completed",
  );
  for v in VARIANTS {
    for fx in FIXTURES.iter().filter(|f| f.is_clean()) {
      let src = v.render(fx);
      let obs = (v.run)(&src);
      let Some(elements) = obs.elements.as_ref() else {
        continue;
      };
      r.check(obs.accepts.len() <= elements.len() + 1, || {
        format!(
          "{}/{} on {src:?}: the element parser produced {:?} and the construct returned \
           {elements:?}",
          v.name, fx.name, obs.accepts
        )
      });
    }
  }
  r.finish();
}

/// **B7 — no residue.** A reused owning collector never starts from the previous attempt's
/// leftovers.
///
/// This is the defect #259 cites as its evidence — "the owning collection residue bug found in
/// this audit is direct evidence of the multiplication risk" — and the one a census structurally
/// cannot see, because it is a property of the collector *between* two attempts and no source
/// text is wrong at any counted site. The transfer that leaves it is one function, so the defect
/// is **uniform across every row at once**: exactly the shape a per-variant suite's expected
/// values absorb without noticing, since each of those values was read off the behaviour it
/// found.
///
/// The assertion needs no expected value. The second attempt's collection has to be a prefix of
/// the **second attempt's** accept log; residue makes it begin with values only the first attempt
/// ever produced.
#[test]
fn a_reused_collector_carries_nothing_from_the_attempt_before() {
  let mut r = Report::new(
    "LAYER B7 — no residue: a collector reused after an earlier attempt must return only what \
     the NEW attempt's element parser produced (#259's cited evidence)",
  );
  for v in VARIANTS {
    for fx in FIXTURES {
      let src = v.render(fx);
      let tail = v.render(&TAIL);
      let (second, accepts) = (v.reused)(&src, &tail);
      let Some(second) = second else { continue };
      r.check(is_prefix(&second, &accepts), || {
        format!(
          "{}/{} on {src:?}: the second attempt returned {second:?} while its element parser \
           produced only {accepts:?} — the difference came from the attempt before it",
          v.name, fx.name
        )
      });
    }
  }
  r.finish();
}

/// **B8 — the borrowed destination holds no more than it was given, on the failure arm too.**
///
/// The owning contract hides the failure arm: a caller that gets `Err` gets no container, so
/// nothing can be observed there. The borrowed contract is the one where the caller keeps the
/// storage and can look at it whatever the construct returned — and it is therefore the only
/// place a partial collection left behind by a failed attempt is visible at all.
///
/// The statement is B1's, applied to that arm: whatever the container holds when the construct
/// returns `Err` is still a prefix of what this attempt's element parser produced.
#[test]
fn a_failed_borrowed_attempt_holds_only_what_it_parsed() {
  let mut r = Report::new(
    "LAYER B8 — the caller-visible container on the FAILURE arm holds a prefix of this attempt's \
     elements and nothing else",
  );
  for b in BORROWED {
    for fx in FIXTURES {
      let src = fx.render(b.joiner, b.delimited);
      let (container, _errored, accepts) = (b.run)(&src);
      r.check(is_prefix(&container, &accepts), || {
        format!(
          "{}/{} on {src:?}: the container held {container:?} against an accept log of {accepts:?}",
          b.name, fx.name
        )
      });
    }
  }
  r.finish();
}

// ══════════════════════════════════════════════════════════════════════════════
// LAYER A — cross-variant relations
//
// The same fixture through two builders that must agree. These catch a variant that has drifted
// from its siblings, and they catch nothing that all of them do together — which is why layer B
// above exists and is the half that answers #259.
// ══════════════════════════════════════════════════════════════════════════════

/// Runs one relation over every fixture, for each declared pair of variant names.
fn relate(law: &'static str, pairs: &[(&str, &str)], only_clean: bool) {
  let mut r = Report::new(law);
  for (a, b) in pairs {
    let va = variant(a);
    let vb = variant(b);
    for fx in FIXTURES {
      if only_clean && !fx.is_clean() {
        continue;
      }
      let sa = va.render(fx);
      let sb = vb.render(fx);
      assert_eq!(
        sa, sb,
        "{law}: {a} and {b} must render one fixture identically or they are not being asked the \
         same question"
      );
      let oa = (va.run)(&sa);
      let ob = (vb.run)(&sb);
      r.check(oa.agreed() == ob.agreed(), || {
        format!(
          "{a} vs {b} on {sa:?} ({}):\n      {a} = {oa:?}\n      {b} = {ob:?}",
          fx.name
        )
      });
    }
  }
  r.finish();
}

/// Every `while` driver paired with its non-`while` sibling, one pair per cardinality they share.
const WHILE_PAIRS: &[(&str, &str)] = &[
  ("rep_unb", "repw_unb"),
  ("rep_min2", "repw_min2"),
  ("rep_max2", "repw_max2"),
  ("rep_bnd13", "repw_bnd13"),
  ("rep_exact2b", "repw_exact2b"),
  ("sep_unb", "sepw_unb"),
  ("sep_min2", "sepw_min2"),
  ("sep_max2", "sepw_max2"),
  ("sep_bnd13", "sepw_bnd13"),
  ("sep_exact2b", "sepw_exact2b"),
  ("drep_unb", "drepw_unb"),
  ("drep_min2", "drepw_min2"),
  ("drep_max2", "drepw_max2"),
  ("drep_bnd13", "drepw_bnd13"),
  ("drep_exact2b", "drepw_exact2b"),
  ("dsep_unb", "dsepw_unb"),
  ("dsep_min2", "dsepw_min2"),
  ("dsep_max2", "dsepw_max2"),
  ("dsep_bnd13", "dsepw_bnd13"),
  ("dsep_exact2b", "dsepw_exact2b"),
];

/// **A1 — the `while` axis is a mechanism, not a behaviour.**
///
/// `try_num`'s decline and `decide_num`'s `Stop` encode one predicate: *the next token is a
/// number*. A `*_while` driver was asked that predicate through its condition and its non-`while`
/// sibling through the element parser's own lookahead, so the two must keep the same elements,
/// record the same diagnostics and leave the input at the same token.
///
/// Restricted to fixtures with no failing element, and the restriction is a **declared difference
/// rather than a skipped case**: the try-driven families file an element's `Err` as a diagnostic
/// and carry on, and the `*_while` families never file one at all.
/// `the_while_and_try_families_diverge_on_an_element_failure` asserts that difference.
#[test]
fn a_while_driver_stops_where_its_try_driven_sibling_stops() {
  relate(
    "LAYER A1 — a `*_while` driver and its non-`while` sibling, asked the same predicate through \
     two mechanisms, must reach the same collection, the same diagnostics and the same resting \
     position",
    WHILE_PAIRS,
    true,
  );
}

/// **A2 — three construction paths, one cardinality.**
///
/// `bounded(2, 2)`, `at_least(2).at_most(2)` and `at_most(2).at_least(2)` reach one `Bounded`
/// through three different `Apply` impls, each placing the two numbers into the constructor
/// itself. A transposition in any one of them is invisible while the two numbers are equal in the
/// *builder* — and this relation makes it visible, by demanding the three agree over every
/// fixture including the ones where a swapped pair changes the answer.
///
/// `Separated` and `SeparatedWhile` expose no `AtLeast`→`AtMost` chaining, so only the four
/// `repeated`-family drivers have three paths to relate.
#[test]
fn the_three_ways_to_spell_one_bound_agree() {
  let pairs: &[(&str, &str)] = &[
    ("rep_exact2b", "rep_exact2lm"),
    ("rep_exact2b", "rep_exact2ml"),
    ("repw_exact2b", "repw_exact2lm"),
    ("repw_exact2b", "repw_exact2ml"),
    ("drep_exact2b", "drep_exact2lm"),
    ("drep_exact2b", "drep_exact2ml"),
    ("drepw_exact2b", "drepw_exact2lm"),
    ("drepw_exact2b", "drepw_exact2ml"),
  ];
  relate(
    "LAYER A2 — `bounded(n, n)`, `at_least(n).at_most(n)` and `at_most(n).at_least(n)` are three \
     construction paths to one cardinality and cannot behave differently",
    pairs,
    false,
  );
}

/// **A3 — a bound the input satisfies is not a bound.**
///
/// Over a fixture whose acceptable prefix is `k` elements long, `at_most(n)` for `k <= n` and
/// `at_least(m)` for `k >= m` must be indistinguishable from the unbounded form: same elements,
/// same diagnostics, same resting position. A cardinality wrapper that changes the collection
/// while its bound is satisfied is doing something the bound does not license — this is the
/// relation that catches a wrapper intercepting the element loop instead of forwarding it, which
/// is the shape #259's "policy/cardinality wrappers contain no independent mutable collection
/// lifecycle" criterion is about.
#[test]
fn a_bound_that_is_satisfied_changes_nothing() {
  let mut r = Report::new(
    "LAYER A3 — a cardinality bound the input satisfies must leave the collection identical to \
     the unbounded form of the same driver",
  );
  for fx in FIXTURES {
    let kept = fx
      .cells
      .iter()
      .take_while(|c| matches!(c, Cell::Ok(_)))
      .count();
    for v in VARIANTS {
      let (min, max) = v.bound;
      if min.is_none() && max.is_none() {
        continue;
      }
      if min.is_some_and(|m| kept < m) || max.is_some_and(|m| kept > m) {
        continue;
      }
      let base = variant(&format!("{}_unb", v.stem()));
      let src = v.render(fx);
      let got = (v.run)(&src);
      let want = (base.run)(&src);
      r.check(got.agreed() == want.agreed(), || {
        format!(
          "{}/{} on {src:?}: {kept} elements satisfy the bound {:?}, so this must match {}:\n   \
           bounded   = {got:?}\n      unbounded = {want:?}",
          v.name, fx.name, v.bound, base.name
        )
      });
    }
  }
  r.finish();
}

/// **A4 — the delimiters do not reach inside.**
///
/// A delimited construct is its undelimited sibling run over the region between the brackets, so
/// the elements must be identical and the delimited construct must consume at least one bracket
/// more on each side. A delimited driver that collects differently — because it carries its own
/// element loop rather than delegating to the shared one — separates them, which is the "delimited
/// helper engines" line of #259's evidence expressed as behaviour.
#[test]
fn a_delimiter_does_not_change_what_is_collected_inside_it() {
  let mut r = Report::new(
    "LAYER A4 — a delimited construct collects exactly what its undelimited sibling collects over \
     the same elements",
  );
  for fx in FIXTURES {
    for (plain, delim) in [
      ("rep_unb", "drep_unb"),
      ("rep_min2", "drep_min2"),
      ("rep_max2", "drep_max2"),
      ("rep_bnd13", "drep_bnd13"),
      ("repw_unb", "drepw_unb"),
      ("repw_min2", "drepw_min2"),
      ("repw_max2", "drepw_max2"),
      ("sep_unb", "dsep_unb"),
      ("sep_min2", "dsep_min2"),
      ("sep_max2", "dsep_max2"),
      ("sepw_unb", "dsepw_unb"),
      ("sepw_min2", "dsepw_min2"),
      ("sepw_max2", "dsepw_max2"),
    ] {
      let vp = variant(plain);
      let vd = variant(delim);
      let sp = vp.render(fx);
      let sd = vd.render(fx);
      let op = (vp.run)(&sp);
      let od = (vd.run)(&sd);
      r.check(op.elements == od.elements, || {
        format!(
          "{plain} vs {delim} on {sp:?} / {sd:?} ({}): inner {:?} against delimited {:?}",
          fx.name, op.elements, od.elements
        )
      });
    }
  }
  r.finish();
}

/// **A5 — the two ownership contracts collect the same elements.**
///
/// `Collect<_, Container>` and `Collect<&mut _, &mut Container>` differ in who owns the storage
/// and in when the caller may look. They must not differ in what the driver puts there. This is
/// the relation that catches a family whose owning path and borrowed path are separate element
/// loops that have drifted — the multiplication #259's evidence section describes, in the one
/// place the crate genuinely writes a driver twice.
///
/// Only the six drivers that implement both contracts have a row; `delim/repeated` and
/// `delim/repeated_while` implement the owning form alone.
#[test]
fn the_owning_and_borrowed_contracts_collect_the_same_elements() {
  let mut r = Report::new(
    "LAYER A5 — the owning and borrowed destinations are one collection through two ownership \
     contracts and must hold the same elements",
  );
  for b in BORROWED {
    let twin = variant(b.twin);
    assert_eq!(
      (b.joiner, b.delimited),
      (twin.joiner, twin.delimited),
      "LAYER A5: {} and its twin {} must render a fixture identically",
      b.name,
      b.twin
    );
    for fx in FIXTURES {
      let src = fx.render(b.joiner, b.delimited);
      let (container, errored, _) = (b.run)(&src);
      let owned = (twin.run)(&src);
      match owned.elements.as_ref() {
        Some(elements) => r.check(!errored && *elements == container, || {
          format!(
            "{}/{} on {src:?}: owning returned {elements:?}, borrowed held {container:?} \
             (borrowed errored: {errored})",
            b.name, fx.name
          )
        }),
        None => r.check(errored, || {
          format!(
            "{}/{} on {src:?}: the owning contract failed while the borrowed one succeeded with \
             {container:?}",
            b.name, fx.name
          )
        }),
      }
    }
  }
  r.finish();
}

/// **A6 — the declared difference between the try-driven and `*_while` families.**
///
/// A1 is restricted to fixtures with no failing element. This is that restriction turned into an
/// assertion: the four try-driven families file an element's `Err` as a diagnostic and go on to
/// the next element, and the four `*_while` families never file one, so the same failing element
/// ends the collection there. The restriction is a documented exception with a consequence, not a
/// fixture the matrix quietly steps around — and this cell is what would red if either family
/// changed its relationship to a failing element.
#[test]
fn the_while_and_try_families_diverge_on_an_element_failure() {
  let mut r = Report::new(
    "LAYER A6 — resilience is the try-driven families' documented difference from the `*_while` \
     ones: an element failure is filed and stepped over on one side and ends the collection on \
     the other",
  );
  let bad = FIXTURES
    .iter()
    .find(|f| f.name == "bad_middle")
    .expect("the bad_middle fixture");
  for (try_driven, while_driven) in WHILE_PAIRS {
    let vt = variant(try_driven);
    let vw = variant(while_driven);
    let src = vt.render(bad);
    let ot = (vt.run)(&src);
    let ow = (vw.run)(&src);
    r.check(ot.accepts.len() > ow.accepts.len(), || {
      format!(
        "{try_driven} vs {while_driven} on {src:?}: the resilient family parsed {:?} and the \
         `*_while` one parsed {:?} — equal counts mean one of the two has changed its \
         relationship to a failing element",
        ot.accepts, ow.accepts
      )
    });
  }
  r.finish();
}

// ══════════════════════════════════════════════════════════════════════════════
// The table itself
// ══════════════════════════════════════════════════════════════════════════════

/// The matrix's size, pinned so that a driver added to `parser::many` without a row here is
/// visible rather than silently uncovered.
///
/// Eight drivers — `repeated`, `repeated_while`, `separated`, `separated_while` and the delimited
/// form of each — times the cardinalities each accepts: seven for the four `repeated`-family
/// drivers, five for the four separated ones, whose builders expose no `AtLeast`→`AtMost`
/// chaining.
#[test]
fn every_declared_driver_has_a_row() {
  assert_eq!(
    VARIANTS.len(),
    48,
    "the variant table changed size; a driver, cardinality or policy added to `parser::many` \
     needs one line in `variants!`, and inherits every fixture and every property from it"
  );
  assert_eq!(
    BORROWED.len(),
    22,
    "the borrowed-destination table changed size; every row here must name a main-table twin"
  );
  assert_eq!(FIXTURES.len(), 10, "the fixture set changed size");
  for b in BORROWED {
    variant(b.twin);
  }
}
