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
//! # The reading that is available and wrong — section 2
//!
//! Publishing the snapshot publishes `trip_snapshot() != 0`, which is a true statement about the
//! session and not the question the loop has: it stays true forever once grammar code catches a
//! trip and parses on. Section 2 is that misreading, run beside the attempt-relative one over a
//! source whose first definition catches its own refusal. The two answer differently, and the
//! absolute one suppresses every diagnostic after the deep construct — exactly the failure the
//! witness was narrowed to avoid. Its widened-budget control is what makes the pair a measurement:
//! with nothing tripping, the two loops agree.
//!
//! # The scanner half, where there is no error value at all — section 3
//!
//! `try_expect` folds a terminal scanner stop into the same `Ok(None)` it uses for a genuine end of
//! input, so a root loop reaches its "the document ended" arm holding **no error**. No reading of an
//! error value can answer there, for any error type, however carefully the grammar delegates
//! `MaybeTerminal`. `scanner_tripped_during_attempt` against a baseline taken once above the loop is
//! the whole of what tells a finished document from a truncated one, and section 3 drives both
//! answers.

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

/// The misreading the snapshot's own documentation refuses: `trip_snapshot() != 0`, which asks
/// whether *the parse* ever refused rather than whether *this definition* did.
fn root_absolute<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  loop {
    match catching_definition(inp) {
      Ok(true) => {}
      Ok(false) => return Ok(()),
      Err(e) => {
        if e.is_terminal() || inp.trip_snapshot() != 0 {
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
/// `1 9 9 9`: the first definition descends, catches its own refusal and carries on; the three
/// after it fail ordinarily on [`BAD`] having descended nothing at all.
#[test]
fn the_absolute_reading_charges_every_later_failure_with_a_refusal_that_is_over() {
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
  let absolute = drive!(TIGHT, root_absolute, SRC);
  assert!(trips() > 0, "the absolute run must refuse too");
  assert_eq!(
    absolute,
    Err(()),
    "session-absolute: `trip_snapshot() != 0` is true forever after the caught refusal, so the \
     next ordinary syntax error ends the document"
  );
  assert_eq!(
    reports(),
    0,
    "session-absolute: one deep construct early in the document suppressed every diagnostic after \
     it — the failure the baseline exists to prevent"
  );
}

/// Non-vacuity for the cell above: with nothing refusing, the two readings agree.
///
/// Without this, "the two loops disagree" is satisfiable by a loop that is simply broken.
#[test]
fn with_nothing_refusing_the_two_readings_agree() {
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
  assert_eq!(drive!(ROOMY, root_absolute, SRC), Ok(()));
  assert_eq!(trips(), 0, "widened budget: nothing refuses");
  assert_eq!(
    reports(),
    3,
    "session-absolute, no refusal: the same three — the counter is what the two readings differ \
     over, and here it never moved"
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
  /// What the witnessed loop returns when it refuses to call a truncated document finished.
  Truncated,
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

/// One definition over the limited stream: commit a number. `Ok(false)` means "nothing more to
/// read" — which is the claim section 3 is about, because a spent scan budget produces it too.
fn s_definition<'inp, Ctx>(inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>) -> Result<bool, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  Ok(inp.try_expect(|_| true)?.is_some())
}

/// The root loop **with** the scanner witness. The baseline is taken once, above the loop: a spent
/// scan budget stays true of the input, so unlike the descent one it is not per element.
fn s_root_witnessed<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let scans = inp.scanner_trip_snapshot();
  let mut parsed = 0usize;
  loop {
    if s_definition(inp)? {
      parsed += 1;
      continue;
    }
    if inp.scanner_tripped_during_attempt(scans) {
      return Err(SErr::Truncated);
    }
    return Ok(parsed);
  }
}

/// The root loop **without** it: `Ok(None)` is read as the end of the document, whatever produced
/// it.
fn s_root_unwitnessed<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let mut parsed = 0usize;
  while s_definition(inp)? {
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

/// A spent scan budget reaches the loop as `Ok(None)`, and only the input can say so.
#[test]
fn a_truncated_document_is_indistinguishable_from_a_finished_one_without_the_scanner_witness() {
  let (unwitnessed, scanned) = s_drive!(S_TIGHT, s_root_unwitnessed, S_SRC);
  assert!(
    scanned > S_TIGHT,
    "the fixture must actually trip the scan budget: scanned {scanned}, limit {S_TIGHT}"
  );
  let parsed = unwitnessed.expect(
    "without the witness the loop reports a finished document — there is no error value at this \
     exit for any reading of one to judge",
  );
  assert!(
    parsed < S_UNITS,
    "the run must really be truncated: {parsed} of {S_UNITS} definitions parsed"
  );

  let (witnessed, scanned) = s_drive!(S_TIGHT, s_root_witnessed, S_SRC);
  assert!(scanned > S_TIGHT, "the witnessed run must trip too");
  assert_eq!(
    witnessed,
    Err(SErr::Truncated),
    "`scanner_tripped_during_attempt` against a baseline taken above the loop refuses to call a \
     truncated document finished"
  );
}

/// Non-vacuity: with a budget nothing reaches, both loops read the whole document and agree.
#[test]
fn with_scan_budget_to_spare_both_loops_read_the_whole_document() {
  let (unwitnessed, scanned) = s_drive!(S_ROOMY, s_root_unwitnessed, S_SRC);
  assert_eq!(unwitnessed, Ok(S_UNITS));
  assert!(
    scanned <= S_ROOMY,
    "the control must not trip: scanned {scanned}"
  );

  let (witnessed, _) = s_drive!(S_ROOMY, s_root_witnessed, S_SRC);
  assert_eq!(
    witnessed,
    Ok(S_UNITS),
    "the witness costs an untripped parse nothing: same count, same verdict"
  );
}
