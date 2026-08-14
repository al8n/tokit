#![cfg(all(feature = "rowan", feature = "std", feature = "combinators"))]

//! **A lossless consumer can ask whether the recursion budget tripped** — `Cst::resource_trips`,
//! from both drivers, in both directions.
//!
//! # The gap this closes, restated as the measurement that found it
//!
//! A descent trip reaches the caller on exactly one channel: the driver's `Result`. It is
//! deliberately **not** an emitter event — a recording emitter that could absorb a resource trip
//! would be worse than one that is hard to observe, so `RecursionLimitReached` has no `emit_*`
//! counterpart and the engines *return* it. Correct, and until this change it left nothing else
//! offered. A `parse_lossless` run whose parser trips the budget produced:
//!
//! ```text
//! driver Result            = Err(..)
//! diagnostics on the sink  = 0
//! tree materialized, text == source: true
//! ```
//!
//! Zero diagnostics, a tree that round-trips perfectly, and nothing saying why it is shaped that
//! way. The realistic lossless consumer discards the `Result` — its product is the tree plus its
//! diagnostics — so for that consumer a descent trip was completely silent.
//! [`the_only_witness`](tripping_parse_is_otherwise_silent_and_the_count_is_the_only_witness)
//! below reproduces every line of that measurement and then asks the new question over it.
//!
//! # Why the count could not simply be fetched later
//!
//! It lived on the `Input`, which the driver drops on the same line that mints the handle.
//! `InputRef::recursion()` is public but *live-only* — it reports depth and limitation, never
//! "a trip happened", and the trip path releases the frame before building the error, so the
//! budget reads clean the instant the error exists. The counter had to be carried onto the
//! handle at the one moment it still existed, which is what
//! `Cst::from_sink(sink, input.resource_trips())` does in both drivers.
//!
//! # Session-absolute here, attempt-relative in there
//!
//! The cell is monotone: it counts up and nothing lowers it. Every site **inside** a parse reads
//! it as a difference across the one attempt it is judging, because an absolute reading there
//! would let one deep construct early in a document re-raise every later failure and suppress
//! every later diagnostic (`tests/collection_resource_trip.rs` is that argument in full, and
//! section 2 of it pins that a caught trip does *not* disable the collections after it). This
//! accessor takes the absolute reading, and it is sound here for the reason it is unsound there:
//! the parse is over, so there is no later attempt to poison.
//!
//! # The boundary is a relation, never the literal
//!
//! Frames admitted equals the limiter's `limitation` **exactly** — the `limitation + 1`th descent
//! is the one refused. The pratt driver takes its frame at the frame prologue
//! (`parser/pratt/expr.rs`), so the root expression spends the first one and the deepest *nested*
//! group that parses clean is `limitation - 1`. Every cell below derives `limitation` from the
//! parse itself ([`limitation`]) rather than naming 64, so a moved default re-points nothing.

use core::fmt;
use std::cell::Cell;

use tokora::{
  InputRef, Lexer, ParseInput, SimpleSpan, Token,
  cache::DefaultCache,
  cst::{CstProfile, KindValidator, parse_lossless, parse_lossless_partial},
  emitter::Verbose,
  error::token::UnexpectedToken,
  input::{Complete, Completeness, SurfaceIncomplete},
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
};

// ── A tiny real lexer: one byte per token ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tok(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LexErr;

impl Token<'_> for Tok {
  type Kind = u8;
  type Error = LexErr;

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  // Honest: byte-per-token, never skips a byte.
  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> u8 {
    self.0
  }

  fn is_trivia(&self) -> bool {
    self.0 == b' '
  }
}

struct ByteLexer<'inp> {
  src: &'inp str,
  tok_start: usize,
  pos: usize,
  state: (),
}

impl<'inp> Lexer<'inp> for ByteLexer<'inp> {
  type State = ();
  type Source = str;
  type Token = Tok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp str) -> Self {
    Self {
      src,
      tok_start: 0,
      pos: 0,
      state: (),
    }
  }

  fn with_state(src: &'inp str, state: ()) -> Self {
    Self {
      src,
      tok_start: 0,
      pos: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), LexErr> {
    Ok(())
  }

  fn state(&self) -> &Self::State {
    &self.state
  }

  fn state_mut(&mut self) -> &mut Self::State {
    &mut self.state
  }

  fn into_state(self) -> Self::State {
    self.state
  }

  fn source(&self) -> &'inp str {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.tok_start, self.pos)
  }

  fn slice(&self) -> &'inp str {
    &self.src[self.tok_start..self.pos]
  }

  fn lex(&mut self) -> Option<Result<Tok, LexErr>> {
    let byte = *self.src.as_bytes().get(self.pos)?;
    self.tok_start = self.pos;
    self.pos += 1;
    Some(Ok(Tok(byte)))
  }

  fn read_frontier(&self) -> tokora::ReadFrontier<usize> {
    tokora::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.pos += *n;
    self.tok_start = self.pos;
  }
}

// ── Error plumbing ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestErr {
  Lex,
  Unexpected,
  /// The one the budget raises. Kept as its own variant so a cell can tell the terminal stop
  /// from an ordinary syntax failure — which is exactly what a consumer whose error type is `()`
  /// could not do, and the reason the witness lives on the session rather than in the payload.
  Depth,
}

impl fmt::Display for TestErr {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{self:?}")
  }
}

impl From<LexErr> for TestErr {
  fn from(_: LexErr) -> Self {
    Self::Lex
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for TestErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Self::Unexpected
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>>
  for TestErr
{
  fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self {
    Self::Unexpected
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoLhs<O, Lang, Set>>
  for TestErr
{
  fn from(_: tokora::error::UnexpectedEoLhs<O, Lang, Set>) -> Self {
    Self::Unexpected
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoRhs<O, Lang, Set>>
  for TestErr
{
  fn from(_: tokora::error::UnexpectedEoRhs<O, Lang, Set>) -> Self {
    Self::Unexpected
  }
}

impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for TestErr {
  fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self {
    Self::Depth
  }
}

impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for TestErr {
  fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self {
    Self::Unexpected
  }
}

impl<O> From<tokora::error::Incomplete<O>> for TestErr {
  fn from(_: tokora::error::Incomplete<O>) -> Self {
    Self::Unexpected
  }
}

/// The partial door's bound. This fixture never surfaces an `Incomplete` — every cell seals the
/// stream — but a grammar error type driven through `parse_lossless_partial` has to answer the
/// question all the same.
impl tokora::error::MaybeIncomplete for TestErr {}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for TestErr
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    Self::Unexpected
  }
}

// ── Dialect fixture ─────────────────────────────────────────────────────────────

const K_ROOT: u16 = 1;
const K_ERR: u16 = 90;
const K_GAP: u16 = 91;

/// Token images: `100 + byte`, so no token kind can collide with a node kind.
fn map_tok(t: &Tok) -> u16 {
  100 + t.0 as u16
}

fn in_kind_space(kind: u16) -> bool {
  matches!(kind, K_ROOT | K_ERR | K_GAP) || kind >= 100
}

fn profile() -> CstProfile<Tok> {
  CstProfile::new(map_tok, KindValidator::new(in_kind_space), K_ERR, K_GAP)
}

type Sink<'inp> = tokora::cst::Sink<'inp, ByteLexer<'inp>, Verbose<TestErr>>;
type Ctx<'inp> = (Sink<'inp>, DefaultCache<'inp, ByteLexer<'inp>>);
type Ir<'inp, 'c, Cmpl = Complete> = InputRef<'inp, 'c, ByteLexer<'inp>, Ctx<'inp>, (), Cmpl>;

// ── The grammar: nested groups, by both routes into the descent ─────────────────
//
// The same source shape parsed two ways, because one route cannot cover both doors and because
// pinning the boundary through one of them alone would leave the `- 1` looking like a quirk of
// that route.
//
//   A. the pratt driver, which takes its frame at the frame prologue. The root expression spends
//      the first; each `(` recurses through the driver again and spends one more. `Complete`-only.
//   B. a hand-written `InputRef::descending` recursion, mode-generic, so the partial door has a
//      grammar at all.
//
// Frame accounting is the same under both: `nest(n)` asks for `n + 1` frames, because the
// innermost level — the one that reads the bare operand — spends one too.

const PREC_SUM: i64 = 1;

/// The bound both doors' grammar carries: one `Completeness` mode, and whatever that mode says
/// about surfacing `Incomplete`. Written once here so the productions below read as a grammar.
trait Mode<'inp>: Completeness + SurfaceIncomplete<'inp, ByteLexer<'inp>, Ctx<'inp>, ()> {}

impl<'inp, T> Mode<'inp> for T where
  T: Completeness + SurfaceIncomplete<'inp, ByteLexer<'inp>, Ctx<'inp>, ()>
{
}

/// **Grammar A — the pratt driver.** Complete-only: `Pratt`'s `ParseInput` impl leaves the mode
/// parameter at its `Complete` default, so this shape cannot be driven through
/// `parse_lossless_partial` at all. That is why grammar B exists below rather than the partial
/// cells re-using this one.
fn expr(inp: &mut Ir<'_, '_>) -> Result<i64, TestErr> {
  pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
}

fn lhs(inp: &mut Ir<'_, '_>) -> Result<PrattLHS<i64, (), i64>, TestErr> {
  match inp.next()? {
    Some(tok) if tok.data().0 == b'(' => {
      // The recursion: one more pratt frame, then the closer.
      let inner = expr(inp)?;
      match inp.next()? {
        Some(t) if t.data().0 == b')' => Ok(PrattLHS::Operand(inner)),
        _ => Err(TestErr::Unexpected),
      }
    }
    Some(tok) if tok.data().0.is_ascii_digit() => {
      Ok(PrattLHS::Operand(i64::from(tok.data().0 - b'0')))
    }
    _ => Err(TestErr::Unexpected),
  }
}

fn rhs(inp: &mut Ir<'_, '_>) -> Result<PrattRHS<u8, u8, u8, (), i64>, TestErr> {
  Ok(match inp.next()? {
    Some(tok) if tok.data().0 == b'+' => {
      PrattRHS::Infix(Precedenced::new(PrattInfix::Left(b'+'), PREC_SUM))
    }
    _ => PrattRHS::End,
  })
}

fn fold_prefix(_: &mut Ir<'_, '_>, operand: i64, _: Precedenced<(), i64>) -> Result<i64, TestErr> {
  Ok(operand)
}

fn fold_infix(
  _: &mut Ir<'_, '_>,
  left: i64,
  right: i64,
  _: Precedenced<PrattInfix<u8, u8, u8>, i64>,
) -> Result<i64, TestErr> {
  Ok(left + right)
}

fn fold_postfix(_: &mut Ir<'_, '_>, operand: i64, _: Precedenced<(), i64>) -> Result<i64, TestErr> {
  Ok(operand)
}

/// **Grammar B — the same nesting, hand-written over [`InputRef::descending`].**
///
/// Mode-generic, so it is the grammar the partial cells drive; and driven through the complete
/// door too, in [`the_boundary_is_limitation_minus_one_nested_constructs`], so the two routes into
/// the descent are pinned against one budget rather than one route standing for both.
///
/// Frame accounting is identical to grammar A's: one frame per level, and the innermost level —
/// the one that reads the bare operand — spends one, exactly as the pratt root expression does.
/// So `nest(n)` asks for `n + 1` frames under either grammar.
fn nested<'inp, Cmpl: Mode<'inp>>(inp: &mut Ir<'inp, '_, Cmpl>) -> Result<usize, TestErr> {
  inp.descending(|inp| match inp.next()? {
    Some(tok) if tok.data().0 == b'(' => {
      let inner = nested(inp)?;
      match inp.next()? {
        Some(t) if t.data().0 == b')' => Ok(inner + 1),
        _ => Err(TestErr::Unexpected),
      }
    }
    Some(tok) if tok.data().0.is_ascii_digit() => Ok(0),
    _ => Err(TestErr::Unexpected),
  })
}

// ── The limit, read off the parse rather than written down ──────────────────────

/// The recursion budget this fixture's lossless parses actually run under.
///
/// **Derived, never named.** `parse_lossless` builds its own context, so the number is a library
/// default this file does not choose — and pinning the literal is how a moved default silently
/// re-points a boundary test. `InputRef::recursion()` is public and reports the limitation, so
/// one throwaway parse asks the parse itself.
fn limitation() -> usize {
  let seen = Cell::new(0usize);
  let (_cst, res) = parse_lossless(
    "1",
    (),
    Verbose::<TestErr>::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    |inp: &mut Ir<'_, '_>| {
      // Read before any frame is entered: this is the configured ceiling, not a live depth.
      seen.set(inp.recursion().limitation());
      let _ = inp.next()?;
      Ok::<_, TestErr>(())
    },
  );
  assert_eq!(res, Ok(()), "the probe parse itself must succeed");
  let lim = seen.get();
  assert!(
    lim > 2,
    "a degenerate ceiling would make every cell vacuous"
  );
  lim
}

/// `n` nested groups around a single operand — **`n + 1` frames** under either grammar: one per
/// group, plus the innermost level that reads the operand.
fn nest(n: usize) -> String {
  format!("{}1{}", "(".repeat(n), ")".repeat(n))
}

/// One complete lossless parse of `src`, reported as (trip count, driver result, diagnostic
/// count, materialized text) — every channel a consumer has, so a cell can state what each one
/// says rather than only the one it is about.
fn run_complete(src: &str, partial_door: bool) -> (usize, Result<i64, TestErr>, usize, String) {
  let (cst, res) = parse_lossless(
    src,
    (),
    Verbose::<TestErr>::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    expr,
  );
  // Read off the handle, BEFORE materialization consumes it. This is the sequencing the fix has
  // to support: the consumer holds one value, and gets the tree, the diagnostics and the trip
  // count out of it.
  let trips = cst.resource_trips();
  let (green, emitter) = if partial_door {
    cst.finish_partial(K_ROOT)
  } else {
    cst.finish(K_ROOT)
  };
  let text = rowan::SyntaxNode::<RawLang>::new_root(green.expect("the tree must materialize"))
    .text()
    .to_string();
  (trips, res, emitter.diagnostics().len(), text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum RawLang {}

impl rowan::Language for RawLang {
  type Kind = u16;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> u16 {
    raw.0
  }

  fn kind_to_raw(kind: u16) -> rowan::SyntaxKind {
    rowan::SyntaxKind(kind)
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Direction 1: a parse that does not trip reports none
// ═══════════════════════════════════════════════════════════════════════════════

/// A parse well inside the budget, and one exactly at the deepest clean nesting, both report
/// **zero**.
///
/// The direction an accessor that returned a constant would pass — which is why it is a cell of
/// its own and why it is driven at the edge as well as in the middle.
#[test]
fn a_parse_inside_the_budget_reports_no_trip() {
  let lim = limitation();

  for src in ["1", "1+1", &nest(1), &nest(4), &nest(lim - 1)] {
    let (trips, res, diags, text) = run_complete(src, false);
    assert_eq!(trips, 0, "{src:.24?} stays inside the budget");
    assert!(res.is_ok(), "{src:.24?} parses: {res:?}");
    assert_eq!(diags, 0, "{src:.24?} is clean");
    assert_eq!(text, src, "{src:.24?} round-trips");
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Direction 2: a parse that trips reports the trip — and nothing else does
// ═══════════════════════════════════════════════════════════════════════════════

/// **The measurement from the issue, reproduced, and then answered.**
///
/// The tripping parse's other three channels say exactly what they said before this change: the
/// diagnostics are empty, the tree round-trips byte for byte, and only the `Result` — the one a
/// lossless consumer discards, because its product is the tree plus the diagnostics — carries
/// anything at all. `resource_trips()` is the channel that survives on the value the consumer
/// keeps.
#[test]
fn tripping_parse_is_otherwise_silent_and_the_count_is_the_only_witness() {
  let lim = limitation();
  let src = nest(lim);

  let (trips, res, diags, text) = run_complete(&src, true);

  // The three channels that were already there, unchanged — this is the silence.
  assert_eq!(
    diags, 0,
    "a descent trip is deliberately not an emitter event"
  );
  assert_eq!(
    text, src,
    "the tree round-trips exactly as an untripped one does"
  );
  assert_eq!(
    res,
    Err(TestErr::Depth),
    "only the discarded Result said so"
  );

  // The channel this change adds.
  assert_eq!(trips, 1, "and the handle now says so too");
}

/// A trip well past the boundary is still reported, and still as a count of one: the budget is a
/// floor, not a one-off at the edge, and the stop is terminal rather than retried.
#[test]
fn a_trip_far_past_the_boundary_is_reported_once() {
  let lim = limitation();
  let src = nest(lim + 64);
  let (trips, res, _diags, text) = run_complete(&src, true);
  assert_eq!(trips, 1, "terminal: the stop is taken once and propagates");
  assert_eq!(res, Err(TestErr::Depth));
  assert_eq!(text, src, "and it still round-trips");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Direction 3: the value survives materialization, through both doors
// ═══════════════════════════════════════════════════════════════════════════════

/// The count is a property of the **parse**, not of the materialization door.
///
/// Materialization is where the handle is spent, and — before this change — where the last trace
/// of the input had already been gone for a line. Both doors are driven over the identical
/// source and must agree, and each must hand back its tree as well: the accessor is on the value
/// the consumer materializes, not on a value it has to keep instead.
#[test]
fn the_count_survives_both_materialization_doors() {
  let lim = limitation();

  // A tripped parse: `finish` refuses it (the un-parsed tail is an uncovered gap), so the
  // agreement is stated over the door that tolerates it, against the same parse's `finish`
  // refusal — which must still report the trip.
  let tripping = nest(lim);
  let (trips_partial, _res, _diags, text) = run_complete(&tripping, true);
  assert_eq!(trips_partial, 1);
  assert_eq!(text, tripping);

  let (cst, res) = parse_lossless(
    tripping.as_str(),
    (),
    Verbose::<TestErr>::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    expr,
  );
  assert_eq!(res, Err(TestErr::Depth));
  let trips_before = cst.resource_trips();
  let (green, _emitter) = cst.finish(K_ROOT);
  assert!(
    green.is_err(),
    "a terminated parse leaves an uncovered tail; `finish` refuses it, by design"
  );
  assert_eq!(
    trips_before, trips_partial,
    "the count is the parse's, and does not depend on which door the tree comes out of — \
     including the door that refuses to produce one"
  );

  // A clean parse, through both doors, still zero.
  let clean = nest(lim - 1);
  assert_eq!(run_complete(&clean, false).0, 0);
  assert_eq!(run_complete(&clean, true).0, 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// The boundary, as a relation
// ═══════════════════════════════════════════════════════════════════════════════

/// **Frames admitted equals `limitation` exactly** — the `limitation + 1`th descent is the one
/// refused — so the deepest clean *nested-group* count is `limitation - 1`.
///
/// **Keyed off the limiter, never off 64.** A ladder rather than a single point, so a boundary
/// that moved by one frame in either direction reddens this instead of sliding. Driven through
/// **both** routes into the descent — the pratt driver's frame prologue and a hand-written
/// `descending` — because a change could plausibly move one and not the other, and because the
/// `- 1` is otherwise easy to read as a pratt quirk rather than as "the innermost level costs a
/// frame too".
#[test]
fn the_boundary_is_limitation_minus_one_nested_constructs() {
  let lim = limitation();

  // Grammar A: the pratt driver.
  for n in [lim - 2, lim - 1] {
    let (trips, res, _, _) = run_complete(&nest(n), false);
    assert_eq!(
      trips,
      0,
      "{n} nested groups asks for {} frames, within a limitation of {lim}",
      n + 1
    );
    assert!(res.is_ok(), "{n} nested groups must parse");
  }

  for n in [lim, lim + 1] {
    let (trips, res, _, _) = run_complete(&nest(n), true);
    assert_eq!(
      trips,
      1,
      "{n} nested groups asks for {} frames, past a limitation of {lim}",
      n + 1
    );
    assert_eq!(res, Err(TestErr::Depth), "{n} nested groups must trip");
  }

  // Grammar B: the hand-written descent, over the same budget, at the same two points.
  let run_nested = |n: usize, partial_door: bool| {
    let src = nest(n);
    let (cst, res) = parse_lossless(
      src.as_str(),
      (),
      Verbose::<TestErr>::new(),
      profile(),
      DefaultCache::<ByteLexer<'_>>::default(),
      nested,
    );
    let trips = cst.resource_trips();
    let (green, _emitter) = if partial_door {
      cst.finish_partial(K_ROOT)
    } else {
      cst.finish(K_ROOT)
    };
    assert!(green.is_ok(), "{n} groups must materialize");
    (trips, res)
  };

  let (trips, res) = run_nested(lim - 1, false);
  assert_eq!(
    trips, 0,
    "the hand-written route admits `limitation` frames too"
  );
  assert_eq!(res, Ok(lim - 1), "and parsed every group");

  let (trips, res) = run_nested(lim, true);
  assert_eq!(
    trips, 1,
    "and refuses the `limitation + 1`th, exactly as the driver does"
  );
  assert_eq!(res, Err(TestErr::Depth));
}

// ═══════════════════════════════════════════════════════════════════════════════
// The second driver — the one that is easy to miss
// ═══════════════════════════════════════════════════════════════════════════════

/// `parse_lossless_partial` carries the count too, in both directions.
///
/// Its construction differs from the complete driver's only by `seal`, which is exactly why it
/// is the line a fix forgets. Driven `is_final = true` so the grammar sees the same world the
/// complete driver gives it and the two are comparable frame for frame.
#[test]
fn the_partial_driver_carries_the_count_too() {
  let lim = limitation();

  let run = |src: &str, partial_door: bool| {
    let (cst, res) = parse_lossless_partial(
      src,
      (),
      Verbose::<TestErr>::new(),
      profile(),
      DefaultCache::<ByteLexer<'_>>::default(),
      true,
      nested,
    );
    let trips = cst.resource_trips();
    let (green, emitter) = if partial_door {
      cst.finish_partial(K_ROOT)
    } else {
      cst.finish(K_ROOT)
    };
    let text = rowan::SyntaxNode::<RawLang>::new_root(green.expect("the tree must materialize"))
      .text()
      .to_string();
    (trips, res, emitter.diagnostics().len(), text)
  };

  // Does not trip.
  let clean = nest(lim - 1);
  let (trips, res, diags, text) = run(&clean, false);
  assert_eq!(
    trips, 0,
    "the partial driver reports none when none happened"
  );
  assert_eq!(res, Ok(lim - 1), "and parsed every group: {res:?}");
  assert_eq!(diags, 0);
  assert_eq!(text, clean);

  // Trips.
  let tripping = nest(lim);
  let (trips, res, diags, text) = run(&tripping, true);
  assert_eq!(trips, 1, "the partial driver reports the trip");
  assert_eq!(res, Err(TestErr::Depth));
  assert_eq!(
    diags, 0,
    "silent on every other channel, exactly as the complete driver is"
  );
  assert_eq!(text, tripping);
}

/// A partial attempt's count is **that attempt's**, because the attempt builds a fresh input and
/// therefore a fresh counter.
///
/// The property a refill loop depends on and could not otherwise learn: two drives over the same
/// buffer each report one trip, rather than the second inheriting the first's. Stated here so a
/// future change that hoisted the counter to something longer-lived than one input session would
/// have to come through this cell.
#[test]
fn each_partial_attempt_counts_its_own_session() {
  let lim = limitation();
  let src = nest(lim);

  for attempt in 0..3 {
    let (cst, res) = parse_lossless_partial(
      src.as_str(),
      (),
      Verbose::<TestErr>::new(),
      profile(),
      DefaultCache::<ByteLexer<'_>>::default(),
      true,
      nested,
    );
    assert_eq!(res, Err(TestErr::Depth));
    assert_eq!(
      cst.resource_trips(),
      1,
      "attempt {attempt} reports its own trip, not the running total"
    );
  }
}
