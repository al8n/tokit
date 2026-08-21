#![cfg(all(feature = "std", feature = "logos_0_16"))]

//! The four **trip-witness** methods, used the way a consumer outside this crate uses them.
//!
//! `InputRef::trip_snapshot` / `tripped_during_attempt` and their scanner twins
//! `scanner_trip_snapshot` / `scanner_tripped_during_attempt` were crate-private for as long as the
//! only sites judging an attempt were this crate's own. They are public now because a consumer
//! acquired the defect they exist for: a **hand-written document-root loop** that catches a failed
//! definition and has to decide whether that failure ends the document.
//!
//! Every other suite that touches these counters drives them through a *combinator* —
//! `tokora/tests/collection_resource_trip.rs` through the collection drivers,
//! `tokora/tests/collection_terminal_stop.rs` through the same four families under a scanner
//! limiter. None of them proves what publishing the methods is for, which is that a loop this crate
//! did not write can take the baseline and read the verdict. That is what this file drives, and it
//! drives it from `tokora/tests/`, so nothing here can reach a `pub(crate)` item.
//!
//! # The shape, and the amplification it prevents
//!
//! The consumer loop is
//!
//! ```text
//! loop {
//!   let trips = inp.trip_snapshot();          // per DEFINITION — inside the loop
//!   match definition(inp) {
//!     Ok(true)  => {}                          // parsed one, keep going
//!     Ok(false) => return Ok(()),              // input ended
//!     Err(e) => {
//!       if e.is_terminal() || inp.tripped_during_attempt(trips) { return Err(e); }
//!       report(); // an ordinary syntax error: file it and carry on
//!     }
//!   }
//! }
//! ```
//!
//! and the term that matters is the second half of the disjunction. Without it — which is the loop
//! smear shipped before al8n/smear#169 — a nesting refusal is filed as an ordinary syntax error and
//! the loop carries on, re-reading the abandoned nest at document level and reporting once per
//! remaining unit. Section 1 measures that: the report count is 0 with the witness and equal to the
//! document's length without it, at three lengths, so what is pinned is the *growth* and not one
//! number.
//!
//! # The placement that compiles and is wrong — section 2
//!
//! The baseline is a value the caller places, and **where** it is placed is the whole verdict.
//! Taken once above the loop it is arithmetically a session-absolute read for every definition
//! after the first: the counter is monotone, so a refusal any earlier definition caught keeps
//! answering "tripped" for the rest of the document. Section 2 runs the hoisted loop beside the
//! per-definition one over a source whose first definition catches its own refusal and whose next
//! three fail ordinarily. The per-definition loop files all three; the hoisted one files none and
//! ends the document on the first. The widened-budget control is what makes the pair a
//! measurement: with nothing refusing, the two agree.
//!
//! The *other* wrong placement — the scanner baseline taken per element rather than per collection
//! — is pinned in-crate, beside the pair it belongs to, because that pair is not public. See
//! `input::input_ref::tests`.
//!
//! What is no longer testable here is the session-absolute reading itself. `trip_snapshot()`
//! returns an opaque `ResourceTripBaseline`, so `!= 0` does not compile, the difference of two
//! baselines does not compile, and a baseline cannot be stashed and carried into another handle
//! invocation. Those are compile-fail doctests on `InputRef::trip_snapshot`; what survives to be
//! measured at runtime is placement, which no type can decide for a caller.
//!
//! # The scanner half, and why this file does not use the scanner counter — section 3
//!
//! `try_expect` folds a terminal scanner stop into the same `Ok(None)` it uses for a genuine end of
//! input, so a root loop reaches its "the document ended" arm holding **no error**. The obvious
//! answer is the input-side scanner counter, and it is the wrong one: `set_state` re-keys the
//! forward-scanning facts and dropping the poison boundary there is the crate's *documented*
//! limit-recovery path, while the counter is monotone and never cleared. A loop that recovers that
//! way reads the whole document and the counter still says truncated. That pair is therefore
//! crate-internal — `InputRef::scanner_trip_snapshot` records the measurement — and the primitive
//! a consumer should reach for is [`InputRef::try_expect_or_stop`], whose contract is that a
//! terminal stop is an error and never a decline. Section 3 is the three cells that show it is
//! right in all three positions: truncated, untruncated, and recovered.
//!
//! [`InputRef::try_expect_or_stop`]: tokora::InputRef::try_expect_or_stop

mod common;

use core::cell::Cell;
use std::rc::Rc;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, Token as TokenTrait,
  emitter::{Ignored, Silent},
  error::MaybeTerminal,
  lexer::LogosLexer,
  logos::{self, Logos},
  state::{State, recursion_tracker::RecursionLimiter},
};

use common::{TestLexer, Token};

// ── Shared parameters ─────────────────────────────────────────────────────────

/// Levels a definition descends after committing its number. Past `TIGHT`, far short of `ROOMY`,
/// so the same definition refuses under one budget and returns under the other.
const LADDER: usize = 24;

/// The budget the ladder exceeds.
const TIGHT: usize = 8;

/// The budget it does not — the non-vacuity control's only difference from the cell above it.
const ROOMY: usize = 4_000;

/// The number a definition rejects as ordinary malformed input: no descent, no budget, no scanner
/// stop. The boring failure a root loop is supposed to file and carry on past.
const BAD: i64 = 9;

thread_local! {
  /// Diagnostics the root loop filed — one per ordinary failure it recovered from.
  ///
  /// A count rather than an emitter log because it is the loop's *own* decision being measured:
  /// what the emitter saw is not the question, what the loop concluded is.
  static REPORTS: Cell<usize> = const { Cell::new(0) };

  /// How many times the ladder actually refused, on this test's thread.
  ///
  /// The non-vacuity control for every cell below. A fixture that quietly stopped tripping
  /// satisfies "the witnessed loop filed nothing" by doing nothing at all, so each cell requires
  /// this to be nonzero on the tight run and zero on its widened control.
  static TRIPS: Cell<usize> = const { Cell::new(0) };
}

fn note_report() {
  REPORTS.with(|c| c.set(c.get() + 1));
}

fn note_trip() {
  TRIPS.with(|c| c.set(c.get() + 1));
}

/// Zeroes both counters and returns nothing — every cell opens with this, since libtest gives each
/// `#[test]` its own thread but a cell runs two parses on it.
fn reset() {
  REPORTS.with(|c| c.set(0));
  TRIPS.with(|c| c.set(0));
}

fn reports() -> usize {
  REPORTS.with(Cell::get)
}

fn trips() -> usize {
  TRIPS.with(Cell::get)
}

// ── Section 1 and 2: the descent pair, over the discarding `()` sink ──────────
//
// `()` is the sink whose `From` throws the trip's payload away, so `is_terminal()` answers `false`
// over a real refusal and the error value cannot carry the verdict. That is not an exotic choice —
// it is the cheapest error type a grammar can have — and it is why the witness lives on the input.

/// `left + 1` nested frames on the parse's shared depth budget, released on the way out.
///
/// The refusal is counted on the way out, exactly once per refusing call: the frames above it only
/// propagate.
fn ladder<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: usize,
) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  let mut frame = inp.descend().inspect_err(|_| note_trip())?;
  let inp = &mut *frame;
  match left {
    0 => Ok(()),
    n => ladder(inp, n - 1),
  }
}

/// One definition: commit a number, then descend. `Ok(false)` means the input ended.
///
/// The number is committed **first**, which is what makes the unwitnessed loop below an
/// amplification rather than a hang: every turn consumes a token whether or not the descent
/// refuses.
fn definition<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  let Some(tok) = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? else {
    return Ok(false);
  };
  let n = match tok.into_data() {
    Token::Num(n) => n,
    _ => unreachable!("the predicate accepted only `Num`"),
  };
  if n == BAD {
    return Err(());
  }
  ladder(inp, LADDER)?;
  Ok(true)
}

/// A definition that **catches its own refusal** and carries on — section 2's subject.
///
/// Nothing about that is exotic: a production entitled to give up on one deep construct and keep
/// its document is the reason the session counter is compared against a baseline instead of read.
fn catching_definition<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<bool, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  let Some(tok) = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))? else {
    return Ok(false);
  };
  let n = match tok.into_data() {
    Token::Num(n) => n,
    _ => unreachable!("the predicate accepted only `Num`"),
  };
  if n == BAD {
    return Err(());
  }
  let _ = ladder(inp, LADDER);
  Ok(true)
}

/// The root loop **with** the witness — the repaired shape, and the one publishing these methods is
/// for.
///
/// The baseline is taken inside the loop, once per definition, which is the placement the descent
/// counter's documentation requires: hoisted above the loop it degrades into the session-absolute
/// read section 2 measures.
fn root_witnessed<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  loop {
    let trips = inp.trip_snapshot();
    match definition(inp) {
      Ok(true) => {}
      Ok(false) => return Ok(()),
      Err(e) => {
        if e.is_terminal() || inp.tripped_during_attempt(trips) {
          return Err(e);
        }
        note_report();
      }
    }
  }
}

/// The root loop **without** it: the error value is the whole decision, which is the loop
/// al8n/smear#169 was filed against.
fn root_unwitnessed<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  loop {
    match definition(inp) {
      Ok(true) => {}
      Ok(false) => return Ok(()),
      Err(e) => {
        if e.is_terminal() {
          return Err(e);
        }
        note_report();
      }
    }
  }
}

/// Section 2's two loops over [`catching_definition`]: attempt-relative, and session-absolute.
fn root_relative<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  loop {
    let trips = inp.trip_snapshot();
    match catching_definition(inp) {
      Ok(true) => {}
      Ok(false) => return Ok(()),
      Err(e) => {
        if e.is_terminal() || inp.tripped_during_attempt(trips) {
          return Err(e);
        }
        note_report();
      }
    }
  }
}

/// The placement the snapshot's own documentation refuses: **one baseline, above the loop**.
///
/// It compiles, because it is a legal baseline used with its own checker — no type can tell a
/// caller where to put it. Arithmetically it is a session-absolute read for every definition after
/// the first, and section 2 measures the difference.
fn root_hoisted<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  // THE DEFECT: hoisted out of the loop, so every definition is judged against the document's
  // start rather than against its own attempt.
  let trips = inp.trip_snapshot();
  loop {
    match catching_definition(inp) {
      Ok(true) => {}
      Ok(false) => return Ok(()),
      Err(e) => {
        if e.is_terminal() || inp.tripped_during_attempt(trips) {
          return Err(e);
        }
        note_report();
      }
    }
  }
}

/// Runs one root loop over `$src` under `$limit`, returning its verdict.
///
/// A macro and not a function, because a `fn`-pointer parameter over
/// `&mut InputRef<'_, '_, TestLexer<'_>, _>` elides into a higher-ranked bound that asks for
/// `Lexer<'a>` at *every* pair of lifetimes, which `LogosLexer<'a, Token>` cannot satisfy. Naming
/// the generic loop at the call site instantiates it at the one context type actually in play.
macro_rules! drive {
  ($limit:expr, $root:ident, $src:expr) => {{
    let ctx: ParserContext<'_, TestLexer<'_>, Ignored> = ParserContext::new(Ignored::default());
    let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation($limit));
    Parser::with_context(ctx).apply($root).parse_str($src)
  }};
}

/// `n` space-separated numbers, every one of them a definition that descends.
///
/// The numbers start above [`BAD`] so that none of them is the ordinary failure: section 1 counts
/// reports, and one ordinary failure mixed into the document would make the count agree with the
/// document's length for the wrong reason.
fn document(n: usize) -> String {
  (1..=n)
    .map(|i| (i + BAD as usize).to_string())
    .collect::<Vec<_>>()
    .join(" ")
}

// ── Section 1: one refusal is one stop, and without the witness it is one per unit ────

/// The amplification, measured at three document lengths.
///
/// The property is not the number but its **growth**: without the witness the report count tracks
/// the document's length, which is what made 66 nested selection sets return 67 diagnostics and 800
/// return 804. With it the count is 0 at every length and the refusal ends the document.
#[test]
fn the_witness_turns_one_refusal_per_unit_into_one_stop() {
  for units in [4usize, 16, 64] {
    let src = document(units);

    reset();
    let unwitnessed = drive!(TIGHT, root_unwitnessed, &src);
    let amplified = reports();
    assert_eq!(
      trips(),
      units,
      "{units} units: every definition must actually refuse — otherwise this cell compares two \
       clean parses"
    );
    assert_eq!(
      unwitnessed,
      Ok(()),
      "{units} units: reading only the error value, every refusal looks like an ordinary syntax \
       error, so the loop runs to the end of the document"
    );
    assert_eq!(
      amplified, units,
      "{units} units: one report per remaining unit — the count tracks the document's length, \
       which is al8n/smear#169"
    );

    reset();
    let witnessed = drive!(TIGHT, root_witnessed, &src);
    assert_eq!(
      trips(),
      1,
      "{units} units: the witnessed run refuses once and then stops reading — the count is the \
       document's length above and 1 here, which is the whole difference"
    );
    assert_eq!(
      witnessed,
      Err(()),
      "{units} units: the refusal ends the document — `tripped_during_attempt` answers where \
       `is_terminal()` on a discarding sink cannot"
    );
    assert_eq!(
      reports(),
      0,
      "{units} units: the loop files nothing, so the refusal is one diagnostic and not {units}"
    );
  }
}

/// The budget is what did it: the identical loop over the identical source, with room to descend.
#[test]
fn with_room_to_descend_the_same_loop_parses_the_whole_document() {
  let src = document(64);

  reset();
  assert_eq!(
    drive!(ROOMY, root_witnessed, &src),
    Ok(()),
    "widened budget: the same source and the same loop parse clean"
  );
  assert_eq!(trips(), 0, "widened budget: nothing refuses");
  assert_eq!(reports(), 0, "widened budget: nothing is filed either");
}

// ── Section 2: the absolute reading is available, and it is the wrong question ────

/// A caught refusal early in the document must not charge the ordinary failures after it.
///
/// Both loops read the same witness with the same checker. The only difference is **where** the
/// baseline is taken, and it decides the whole document.
///
/// `1 9 9 9`: the first definition descends, catches its own refusal and carries on; the three
/// after it fail ordinarily on [`BAD`] having descended nothing at all.
#[test]
fn a_hoisted_baseline_charges_every_later_failure_with_a_refusal_that_is_over() {
  const SRC: &str = "1 9 9 9";

  reset();
  let relative = drive!(TIGHT, root_relative, SRC);
  assert!(trips() > 0, "the fixture must actually refuse");
  assert_eq!(
    relative,
    Ok(()),
    "attempt-relative: the caught refusal belongs to the definition that caught it, so the \
     ordinary failures after it are ordinary"
  );
  assert_eq!(
    reports(),
    3,
    "attempt-relative: all three ordinary failures are filed"
  );

  reset();
  let hoisted = drive!(TIGHT, root_hoisted, SRC);
  assert!(trips() > 0, "the hoisted run must refuse too");
  assert_eq!(
    hoisted,
    Err(()),
    "hoisted: the counter is monotone, so a baseline taken above the loop keeps answering \
     `tripped` after the caught refusal and the next ordinary syntax error ends the document"
  );
  assert_eq!(
    reports(),
    0,
    "hoisted: one deep construct early in the document suppressed every diagnostic after it — the \
     failure the per-definition placement exists to prevent"
  );
}

/// Non-vacuity for the cell above: with nothing refusing, the two readings agree.
///
/// Without this, "the two loops disagree" is satisfiable by a loop that is simply broken.
#[test]
fn with_nothing_refusing_the_two_placements_agree() {
  const SRC: &str = "1 9 9 9";

  reset();
  assert_eq!(drive!(ROOMY, root_relative, SRC), Ok(()));
  assert_eq!(trips(), 0, "widened budget: nothing refuses");
  assert_eq!(
    reports(),
    3,
    "attempt-relative, no refusal: three ordinary failures"
  );

  reset();
  assert_eq!(drive!(ROOMY, root_hoisted, SRC), Ok(()));
  assert_eq!(trips(), 0, "widened budget: nothing refuses");
  assert_eq!(
    reports(),
    3,
    "hoisted, no refusal: the same three — the counter is what the two placements differ over, and \
     here it never moved"
  );
}

// ── Section 3: the scanner pair, at an exit that holds no error at all ───────

/// A scan limiter whose counter is shared across every cloned lexer.
///
/// `InputRef` rebuilds a fresh lexer per operation by cloning the state, so only an
/// `Rc<Cell<_>>`-shared counter makes every scan observable and the trip sticky.
#[derive(Debug, Clone, Default)]
struct ScanLimiter {
  scanned: Rc<Cell<usize>>,
  limit: usize,
}

impl ScanLimiter {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: Rc::new(Cell::new(0)),
      limit,
    }
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }

  /// A shared handle on the scan counter, readable after the state has been moved into the parse.
  fn counter(&self) -> Rc<Cell<usize>> {
    self.scanned.clone()
  }
}

#[derive(Debug, Clone, PartialEq)]
struct ScanLimitExceeded;

impl State for ScanLimiter {
  type Error = ScanLimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.scanned.get() > self.limit {
      Err(ScanLimitExceeded)
    } else {
      Ok(())
    }
  }
}

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, extras = ScanLimiter, skip r"[ \t\r\n]+")]
enum STok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); lex.slice().parse::<i64>().unwrap_or(0) })]
  Num(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SKind {
  Num,
}

impl core::fmt::Display for STok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

impl core::fmt::Display for SKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("number")
  }
}

/// The fixture error. It never has to distinguish anything the loop reads, because at the exit
/// section 3 is about there **is no error** to read.
#[derive(Debug, Clone, PartialEq)]
enum SErr {
  /// Whatever the lexer or the emitter produced.
  Ordinary,
  /// The terminal end-of-input `try_expect_or_stop` raises over a stop — the carrier that makes
  /// the truncation visible with no input-side witness at all.
  Eot,
}

impl From<()> for SErr {
  fn from((): ()) -> Self {
    SErr::Ordinary
  }
}

impl From<ScanLimitExceeded> for SErr {
  fn from(_: ScanLimitExceeded) -> Self {
    SErr::Ordinary
  }
}

impl<O, Lang: ?Sized> From<tokora::error::UnexpectedEot<O, Lang>> for SErr {
  fn from(_: tokora::error::UnexpectedEot<O, Lang>) -> Self {
    SErr::Eot
  }
}

impl TokenTrait<'_> for STok {
  type Kind = SKind;
  type Error = SErr;

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  fn kind(&self) -> SKind {
    SKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type SLexer<'a> = LogosLexer<'a, STok>;

/// The loop a consumer should write for the scanner half: the decision read is
/// [`InputRef::try_expect_or_stop`], whose `Ok(None)` means definite absence and whose terminal
/// stop is an error.
///
/// No input-side witness anywhere, and none needed. This is the whole of what section 3 argued the
/// scanner counter was for, met by a primitive that was already public — and met *better*, because
/// the counter is monotone while this reads the live boundary, so a documented `set_state`
/// recovery correctly stops it reporting a stop.
fn s_root_or_stop<'inp, Ctx>(inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let mut parsed = 0usize;
  while inp.try_expect_or_stop(|_| true)?.is_some() {
    parsed += 1;
  }
  Ok(parsed)
}

/// The same loop, recovering from the stop through the documented path — swap in a fresh state,
/// which drops the poison boundary and resumes scanning past it.
fn s_root_or_stop_recovering<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let mut parsed = 0usize;
  let mut recovered = false;
  loop {
    match inp.try_expect_or_stop(|_| true) {
      Ok(Some(_)) => parsed += 1,
      Ok(None) => return Ok(parsed),
      Err(_) if !recovered => {
        inp.set_state(ScanLimiter::with_limit(S_ROOMY));
        recovered = true;
      }
      Err(e) => return Err(e),
    }
  }
}

/// The loop that reads `try_expect`, whose `Ok(None)` covers a terminal stop — the shape the
/// scanner witness would have had to rescue.
fn s_root_try_expect<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let mut parsed = 0usize;
  while inp.try_expect(|_| true)?.is_some() {
    parsed += 1;
  }
  Ok(parsed)
}

/// Section 3's driver: the verdict, and how many items the lexer actually scanned.
///
/// A macro for the reason `drive!` is one.
macro_rules! s_drive {
  ($limit:expr, $root:ident, $src:expr) => {{
    let limiter = ScanLimiter::with_limit($limit);
    let scanned = limiter.counter();
    let ctx: ParserContext<'_, SLexer<'_>, Silent<SErr>> = ParserContext::new(Silent::new());
    let out = Parser::with_parser_and_context($root, ctx).parse_str_with_state($src, limiter);
    (out, scanned.get())
  }};
}

/// The source for section 3: eight definitions, a budget that stops the scanner part-way.
const S_SRC: &str = "1 2 3 4 5 6 7 8";
const S_UNITS: usize = 8;
const S_TIGHT: usize = 3;
const S_ROOMY: usize = 1_000;

/// `try_expect` folds a spent scan budget into the same `Ok(None)` a finished document produces,
/// and `try_expect_or_stop` does not.
///
/// The two loops differ in exactly one call. One reports a complete document over a truncated
/// stream; the other raises the terminal end-of-input error, with no input-side witness in either.
#[test]
fn try_expect_or_stop_surfaces_the_scanner_stop_that_try_expect_hides() {
  let (hidden, scanned) = s_drive!(S_TIGHT, s_root_try_expect, S_SRC);
  assert!(
    scanned > S_TIGHT,
    "the fixture must actually trip the scan budget: scanned {scanned}, limit {S_TIGHT}"
  );
  let parsed = hidden.expect("`try_expect` reports a finished document over the spent budget");
  assert!(
    parsed < S_UNITS,
    "the run must really be truncated: {parsed} of {S_UNITS} definitions parsed"
  );

  let (surfaced, scanned) = s_drive!(S_TIGHT, s_root_or_stop, S_SRC);
  assert!(scanned > S_TIGHT, "the or_stop run must trip too");
  assert_eq!(
    surfaced,
    Err(SErr::Eot),
    "`try_expect_or_stop` raises the terminal end-of-input error, which is the whole answer — no \
     scanner counter, no baseline, no placement to get wrong"
  );
}

/// Non-vacuity: with a budget nothing reaches, the safe primitive costs the parse nothing.
#[test]
fn with_scan_budget_to_spare_or_stop_reads_the_whole_document() {
  let (out, scanned) = s_drive!(S_ROOMY, s_root_or_stop, S_SRC);
  assert_eq!(out, Ok(S_UNITS));
  assert!(
    scanned <= S_ROOMY,
    "the control must not trip: scanned {scanned}"
  );
}

/// The position the crate-internal scanner counter gets **wrong**, and this primitive gets right.
///
/// `set_state` drops the poison boundary — the documented limit-recovery path — while the scanner
/// counter is monotone and never cleared. A loop that recovers that way reads the whole document,
/// and a counter-based verdict taken once above the loop still answers "tripped": measured at
/// `(8, true)` for this exact fixture, which is why `InputRef::scanner_trip_snapshot` is not
/// public. The live-boundary read has no such residue.
#[test]
fn a_documented_state_recovery_leaves_or_stop_reporting_a_finished_document() {
  let (out, scanned) = s_drive!(S_TIGHT, s_root_or_stop_recovering, S_SRC);
  assert!(
    scanned > S_TIGHT,
    "the fixture must actually trip before recovering: scanned {scanned}, limit {S_TIGHT}"
  );
  assert_eq!(
    out,
    Ok(S_UNITS),
    "the recovery is documented and complete, so the document is finished and not truncated — the \
     verdict a monotone counter cannot reach"
  );
}
