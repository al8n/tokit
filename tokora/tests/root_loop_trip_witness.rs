#![cfg(all(feature = "std", feature = "logos_0_16"))]

//! The input-side **terminality readings**, used the way a consumer outside this crate uses them.
//!
//! `InputRef::trip_snapshot` / `tripped_during_attempt` and their scanner twins
//! `scanner_trip_snapshot` / `scanner_tripped_during_attempt` were crate-private for as long as the
//! only sites judging an attempt were this crate's own. The descent pair is public now because a
//! consumer acquired the defect they exist for: a **hand-written document-root loop** that catches
//! a failed definition and has to decide whether that failure ends the document. The scanner pair
//! is still crate-internal, and the public scanner verdict is `InputRef::at_scanner_stop` — a
//! reading of the live stop rather than a baseline-and-compare, for the reason section 4 measures.
//!
//! Every other suite that touches these counters drives them through a *combinator* —
//! `tokora/tests/collection_resource_trip.rs` through the collection drivers,
//! `tokora/tests/collection_terminal_stop.rs` through the same four families under a scanner
//! limiter. None of them proves what publishing these readings is for, which is that a loop this
//! crate did not write can judge its own failure. That is what this file drives, and it drives it
//! from `tokora/tests/`, so nothing here can reach a `pub(crate)` item.
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
//! # The scanner half, and why it is not a counter — sections 3 and 4
//!
//! `try_expect` folds a terminal scanner stop into the same `Ok(None)` it uses for a genuine end of
//! input, so a root loop reaches its "the document ended" arm holding **no error**. The obvious
//! answer is the input-side scanner *counter*, and it is the wrong one: `set_state` / `state_mut`
//! re-key the forward-scanning facts and dropping the poison boundary there is the crate's
//! *documented* limit-recovery path, while the counter is monotone and never cleared. A loop that
//! recovers that way reads the whole document and the counter still says truncated. That pair is
//! therefore crate-internal, and `InputRef::scanner_trip_snapshot` records the measurement.
//!
//! [`InputRef::try_expect_or_stop`] is what a consumer reaches for **on the declining exits**, and
//! section 3 is the three cells that show it right in all three of those positions: truncated,
//! untruncated, and recovered.
//!
//! It is not the whole answer, and section 4 is the exit it cannot reach: a *rejecting* emitter's
//! trip is built and propagated from inside that very call, before it can raise a terminal stop, so
//! the caller gets an ordinary-looking error over an exhausted scanner and no delegation in its
//! `MaybeTerminal` can recover the fact. [`InputRef::at_scanner_stop`] is what a root loop reads
//! there — the **live** stop, at the committed cursor, taking no baseline. Section 4 measures both
//! directions of it: without the reading a re-keying root loop files one diagnostic per remaining
//! token at three document lengths, and with it the *documented* recovery still finishes the
//! document — the row a monotone counter answers wrongly, and the reason the counter pair is not
//! what was published.
//!
//! # Where the reading survives a rollback, and where it does not — section 5
//!
//! `at_scanner_stop` reads what is on record at the committed cursor, and a speculative wrapper —
//! `try_attempt`, `attempt_parse`, a rollback-on-drop `Transaction` — restores a checkpoint that
//! **predates** the trip: `None` boundary, original cursor, original lexer state. Section 5 drives
//! all three, and its point is that the resulting `false` is not one answer to grade. It reads
//! right or wrong according to where the scanner bound lives, and the three rows differ in nothing
//! else: a tally the state only points at is still spent (wrong), the crate's own `TokenLimiter`
//! held by value is refunded by the very same restore (right), and a `TokenBudget` on the input is
//! untouched by it (right, on both sides). The first two need opposite answers after the same
//! restore, which is why nothing carried across it can serve both — and the last cell measures
//! what reading it on the wrong side costs: a loop that files a report per turn and never advances
//! the cursor at all.
//!
//! [`InputRef::at_scanner_stop`]: tokora::InputRef::at_scanner_stop
//! [`InputRef::try_expect_or_stop`]: tokora::InputRef::try_expect_or_stop

mod common;

use core::cell::Cell;
use std::rc::Rc;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, Token as TokenTrait,
  emitter::{Fatal, Ignored, Silent},
  error::MaybeTerminal,
  input::TokenBudget,
  lexer::LogosLexer,
  logos::{self, Logos},
  state::{
    State,
    recursion_tracker::RecursionLimiter,
    token_tracker::{TokenLimitExceeded, TokenLimiter},
  },
  try_parse_input::ParseAttempt,
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
/// [`drive`]'s shape for a probe that returns a value rather than a verdict.
macro_rules! drive_probe {
  ($limit:expr, $root:ident, $src:expr) => {{
    let ctx: ParserContext<'_, TestLexer<'_>, Ignored> = ParserContext::new(Ignored::default());
    let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation($limit));
    Parser::with_context(ctx).apply($root).parse_str($src)
  }};
}

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

/// The `Debug` render of a baseline carries **no number at all**.
///
/// `trip_snapshot() != 0` does not compile — the session-absolute reading is the one the opaque
/// type exists to refuse — and a derived `Debug` would hand the same number straight back through
/// `{:?}`. It would also render the nonce, which is the address of an internal slot. So the impl
/// is hand-written, and this is what keeps it hand-written: one `#[derive]` and this cell reds.
///
/// The assertion is "no ASCII digit anywhere", not "does not contain the count": a consumer cannot
/// read the count to compare against, and a digit-free render is the stronger property and cannot
/// be satisfied by a leak that happens to print a value the test did not predict. The baseline is
/// taken **after** a real refusal, so the counter is off zero and a leak would have something to
/// leak.
#[test]
fn a_baseline_renders_without_any_number_in_it() {
  reset();

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // A real refusal first, so the counter this baseline snapshots is not zero.
    let _ = ladder(inp, LADDER);
    Ok(format!("{:?}", inp.trip_snapshot()))
  }

  let rendered = drive_probe!(TIGHT, probe, "1");
  assert!(
    trips() > 0,
    "the fixture must actually refuse, or there is no count to leak"
  );

  assert_eq!(
    rendered.as_deref(),
    Ok("ResourceTripBaseline(..)"),
    "the render is a fixed string: no fields, so nothing to leak and nothing to drift"
  );
  let rendered = rendered.expect("the probe returns the render");
  assert!(
    !rendered.chars().any(|c| c.is_ascii_digit()),
    "no digit may appear in the render — not the count, which is the reading the type refuses, \
     and not the nonce, which is an internal address. Rendered: {rendered}"
  );
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
  /// the truncation visible to an **accepting** emitter's caller.
  Eot,
  /// What the scanner's own limit trip converts to. A *rejecting* emitter returns this value
  /// instead of a stop, and nothing on that path marks it — see section 4.
  Limit,
}

impl From<()> for SErr {
  fn from((): ()) -> Self {
    SErr::Ordinary
  }
}

impl From<ScanLimitExceeded> for SErr {
  fn from(_: ScanLimitExceeded) -> Self {
    SErr::Limit
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized>
  From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for SErr
{
  fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    SErr::Ordinary
  }
}

/// Delegating as carefully as a grammar can: the terminal carrier answers `true`, everything else
/// `false`. Section 4 is that this is not enough, and that no care here could make it enough.
impl MaybeTerminal for SErr {
  fn is_terminal(&self) -> bool {
    matches!(self, SErr::Eot)
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

// ── Section 4: the exit the error value cannot answer, and the witness that does ────

/// A document of `units` numbers, so a report count can be measured as a *growth* rather than as
/// one number.
fn s_document(units: usize) -> String {
  (1..=units)
    .map(|n| n.to_string())
    .collect::<Vec<_>>()
    .join(" ")
}

/// The brake. The defect the cells below measure is an unbounded report stream, and a fixture that
/// hangs reports nothing — so the loop stops itself and the *count* is what fails the assertion.
const S_REPORT_CAP: usize = 256;

/// The root loop a context-sensitive grammar writes: an ordinary failure is met by **re-keying the
/// lexer regime** and retrying, which is [`InputRef::state_mut`]'s documented effect — the token
/// cache is dropped and the poison boundary with it.
///
/// `witness` is the whole variable. With it, the loop asks the input whether the scanner has
/// stopped before it decides the failure was ordinary; without it, the error value is all it has.
///
/// The re-key carries the **same** limiter — same shared counter, same limit. That is not a
/// contrived recovery: [`InputRef::state_mut`] hands out `&mut L::State` for a mode switch, and a
/// grammar that switches lexing modes on a syntax error has no reason to touch a resource tally it
/// did not know was spent. Widening the budget is the *other* recovery, and it is
/// [`s_root_widening`].
///
/// [`InputRef::state_mut`]: tokora::InputRef::state_mut
fn s_root_rekeying<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
  witness: bool,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let mut parsed = 0usize;
  loop {
    match inp.try_expect_or_stop(|_| true) {
      Ok(Some(_)) => parsed += 1,
      Ok(None) => return Ok(parsed),
      Err(e) => {
        // The root loop's one decision: does this failure end the document?
        if e.is_terminal() || (witness && inp.at_scanner_stop()) {
          return Err(e);
        }
        note_report();
        if reports() >= S_REPORT_CAP {
          return Ok(parsed);
        }
        // An ordinary syntax error, as far as this loop can tell: re-key and carry on. The
        // boundary goes with the regime, so the next turn re-lexes against a tally that is still
        // spent.
        inp.state_mut();
      }
    }
  }
}

/// [`s_root_rekeying`] reading the witness. A plain `fn` item, not a closure: the parser entry
/// point is higher-ranked in `'inp`, and only a `fn` item generalises there without annotation.
fn s_root_rekeying_witnessed<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  s_root_rekeying(inp, true)
}

/// [`s_root_rekeying`] reading only the error value — the loop that has nothing to read.
fn s_root_rekeying_unwitnessed<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  s_root_rekeying(inp, false)
}

/// The same loop, recovering the way the limit-recovery path is *documented*: swap in a state whose
/// budget is not spent.
///
/// The witness is read on every failure after the widening, and it must **not** fire — the recovery
/// is complete and the document is finished, not truncated. This is the row a monotone trip counter
/// gets wrong.
fn s_root_widening<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let mut parsed = 0usize;
  let mut widened = false;
  loop {
    match inp.try_expect_or_stop(|_| true) {
      Ok(Some(_)) => parsed += 1,
      // The end of the document, and the loop asks the witness whether it is a *truncated* one.
      // This is where a monotone trip counter answers wrongly and the live reading does not: the
      // counter still carries the trip the widening recovered from.
      Ok(None) => {
        return if inp.at_scanner_stop() {
          Err(SErr::Eot)
        } else {
          Ok(parsed)
        };
      }
      Err(e) => {
        if widened {
          if e.is_terminal() || inp.at_scanner_stop() {
            return Err(e);
          }
          note_report();
          return Ok(parsed);
        }
        inp.set_state(ScanLimiter::with_limit(S_ROOMY));
        widened = true;
      }
    }
  }
}

/// Reads the witness on the far side of a re-key, and hands the reading back as the parse's output.
///
/// Two budgets stop a scanner and they decay differently. The **lexer's** limit lives in `L::State`
/// and the boundary it latches is a per-regime memo, so a re-key really does recover it. The
/// **input's** [`TokenBudget`] refusal is recorded on the input's own tally, which no
/// [`Checkpoint`] carries, no re-key touches and no mutator lowers. A witness that read only the
/// boundary would answer clean on the far side of a re-key over an input that is stopped for good.
///
/// [`TokenBudget`]: tokora::input::TokenBudget
/// [`Checkpoint`]: tokora::input::Checkpoint
fn s_probe_after_rekey<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<bool, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  loop {
    match inp.try_expect_or_stop(|_| true) {
      Ok(Some(_)) => {}
      Ok(None) => return Ok(inp.at_scanner_stop()),
      Err(_) => {
        inp.state_mut();
        return Ok(inp.at_scanner_stop());
      }
    }
  }
}

/// Section 4's driver: the verdict, and how many items the lexer actually scanned, under the
/// **rejecting** emitter. `s_drive!`'s twin, and a macro for the same reason.
macro_rules! s_drive_fatal {
  ($limit:expr, $root:ident, $src:expr) => {{
    let limiter = ScanLimiter::with_limit($limit);
    let scanned = limiter.counter();
    let ctx: ParserContext<'_, SLexer<'_>, Fatal<SErr>> = ParserContext::new(Fatal::new());
    let out = Parser::with_parser_and_context($root, ctx).parse_str_with_state($src, limiter);
    (out, scanned.get())
  }};
}

/// A **rejecting** emitter hands a root loop a scanner stop with nothing on the error value to
/// read — and [`InputRef::at_scanner_stop`] is what the loop reads instead.
///
/// A rejecting (fail-fast) emitter reports a lexer-resource trip by **returning** the value its
/// `From<<L::Token as Token>::Error>` builds — that `Err` is the report, not a refusal to make one
/// — and `scan_with(..)?` propagates it from *inside* [`InputRef::try_expect_or_stop`], before the
/// call can reach the arm that raises a terminal end-of-input. So the caller receives an ordinary
/// grammar error over an exhausted scanner, and no care in the grammar's `MaybeTerminal` can fix
/// it: [`SErr`] delegates as carefully as a grammar can and still answers `false`, because there is
/// nothing terminal-marked anywhere on that path to delegate to. **That half is unchanged, and this
/// cell still measures it** — closing al8n/tokora#311 put nothing on the value.
///
/// What it changed is that the loop can read the stop off the *input*. The boundary is latched
/// inside the crate's terminal predicate, ahead of the diagnostic ever being offered to the
/// emitter, so it is already on record when the rejection arrives. The two emitters therefore now
/// agree about whether the document ended, which they did not before: the accepting one says so
/// through the error value, the rejecting one through the input.
///
/// [`InputRef::at_scanner_stop`]: tokora::InputRef::at_scanner_stop
/// [`InputRef::try_expect_or_stop`]: tokora::InputRef::try_expect_or_stop
#[test]
fn a_rejecting_emitter_hands_the_root_loop_an_unmarked_scanner_stop() {
  let limiter = ScanLimiter::with_limit(S_TIGHT);
  let scanned = limiter.counter();
  let ctx: ParserContext<'_, SLexer<'_>, Fatal<SErr>> = ParserContext::new(Fatal::new());
  let rejecting =
    Parser::with_parser_and_context(s_root_or_stop, ctx).parse_str_with_state(S_SRC, limiter);

  assert!(
    scanned.get() > S_TIGHT,
    "the fixture must actually trip the scan budget: scanned {}, limit {S_TIGHT}",
    scanned.get()
  );
  assert_eq!(
    rejecting,
    Err(SErr::Limit),
    "the trip arrives as the grammar's own conversion of the lexer error, built and propagated \
     inside `try_expect_or_stop` before it can raise a terminal stop"
  );
  assert!(
    !rejecting.unwrap_err().is_terminal(),
    "and it is still UNMARKED: nothing on that path constructs a terminal carrier for \
     `MaybeTerminal` to delegate to, so the fix could not be on the value"
  );

  // The control: the identical fixture under an ACCEPTING emitter, where the stop is marked. The
  // two differ only in the emitter, which is what made the gap a property of the channel.
  let (accepting, _) = s_drive!(S_TIGHT, s_root_or_stop, S_SRC);
  assert_eq!(
    accepting,
    Err(SErr::Eot),
    "accepting emitter, same source, same budget: the stop is terminal-marked and readable"
  );

  // And the reading that is the same on both channels: the loop asks the INPUT.
  reset();
  let (witnessed, scanned) = s_drive_fatal!(S_TIGHT, s_root_rekeying_witnessed, S_SRC);
  assert!(scanned > S_TIGHT, "the witnessed run must trip too");
  assert_eq!(
    witnessed,
    Err(SErr::Limit),
    "the witnessed loop ends the document holding the unmarked value, because `at_scanner_stop` \
     answered where `is_terminal` could not"
  );
  assert_eq!(
    reports(),
    0,
    "one stop is one stop: nothing was filed as an ordinary syntax error"
  );
}

/// Without the witness the same loop turns one spent budget into one diagnostic **per remaining
/// token**, and the count grows with the document.
///
/// This is the amplification shape of al8n/smear#169, reached here through the *other* public
/// contract: the loop reads the unmarked value, concludes "ordinary syntax error", re-keys — which
/// drops the poison boundary, because that is what a re-key does — and retries against a tally that
/// is still spent. The crate's own [`InputRef::try_expect_or_stop`] gate cannot stop it, since the
/// re-key removes the very latch that gate reads.
///
/// Three lengths, so what is pinned is the growth and not one number.
///
/// [`InputRef::try_expect_or_stop`]: tokora::InputRef::try_expect_or_stop
#[test]
fn without_the_witness_a_re_keying_root_loop_files_one_report_per_remaining_token() {
  for units in [4usize, 8, 16] {
    let src = s_document(units);

    reset();
    let (unwitnessed, scanned) = s_drive_fatal!(S_TIGHT, s_root_rekeying_unwitnessed, &src);
    assert!(
      scanned > S_TIGHT,
      "units={units}: the fixture must trip: scanned {scanned}, limit {S_TIGHT}"
    );
    assert_eq!(
      reports(),
      units - S_TIGHT,
      "units={units}: one spent budget filed {} diagnostics — the growth the witness removes. \
       Verdict was {unwitnessed:?}",
      units - S_TIGHT
    );
    assert_eq!(
      unwitnessed,
      Ok(S_TIGHT),
      "units={units}: and the run it finally reports is the truncated one — everything the \
       re-keys crossed was consumed by the trips that ate it"
    );

    reset();
    let (witnessed, scanned) = s_drive_fatal!(S_TIGHT, s_root_rekeying_witnessed, &src);
    assert!(
      scanned > S_TIGHT,
      "units={units}: the witnessed run must trip too"
    );
    assert_eq!(
      witnessed,
      Err(SErr::Limit),
      "units={units}: the witnessed loop ends the document at the stop"
    );
    assert_eq!(
      reports(),
      0,
      "units={units}: and files nothing, at every length"
    );
  }
}

/// Non-vacuity for the pair above: with a budget nothing reaches, the two loops agree.
///
/// Without this cell, "the witnessed loop files nothing" is satisfiable by a loop that does nothing
/// at all.
#[test]
fn with_scan_budget_to_spare_the_witness_costs_the_parse_nothing() {
  for units in [4usize, 8, 16] {
    let src = s_document(units);

    reset();
    let (out, scanned) = s_drive_fatal!(S_ROOMY, s_root_rekeying_unwitnessed, &src);
    assert!(
      scanned <= S_ROOMY,
      "units={units}: the control must not trip: scanned {scanned}"
    );
    assert_eq!(out, Ok(units), "units={units}: the whole document");
    assert_eq!(reports(), 0, "units={units}: nothing filed");

    reset();
    let (out, scanned) = s_drive_fatal!(S_ROOMY, s_root_rekeying_witnessed, &src);
    assert!(
      scanned <= S_ROOMY,
      "units={units}: the witnessed control must not trip either: scanned {scanned}"
    );
    assert_eq!(
      out,
      Ok(units),
      "units={units}: the whole document, with the witness read at every failure — and there are \
       none"
    );
    assert_eq!(reports(), 0, "units={units}: nothing filed");
  }
}

/// The row a **monotone** trip counter gets wrong, and this witness does not: a documented recovery
/// finishes the document, and the witness says so.
///
/// `set_state` drops the poison boundary — that *is* the documented limit-recovery path — while the
/// session's scanner-trip counter is monotone and never cleared. Measured on this exact fixture,
/// a counter-based verdict answers `true` here over a fully recovered parse; the live reading
/// answers `false`, because the regime that owned the stop is gone.
#[test]
fn a_documented_widening_leaves_the_witness_reporting_a_finished_document() {
  reset();
  let (out, scanned) = s_drive_fatal!(S_TIGHT, s_root_widening, S_SRC);
  assert!(
    scanned > S_TIGHT,
    "the fixture must actually trip before recovering: scanned {scanned}, limit {S_TIGHT}"
  );
  assert_eq!(
    out,
    Ok(S_UNITS - 1),
    "the recovery is documented and complete, so the document is finished and not truncated. One \
     token short of the source because the trip that provoked the recovery consumed the token it \
     tripped on — that loss is the trip's, not the witness's"
  );
  assert_eq!(
    reports(),
    0,
    "no failure after the widening: the loop reached a genuine end of input, and the witness — \
     read on every one of those turns — never fired"
  );
}

/// The **other** budget the witness answers for: the input-layer [`TokenBudget`], whose refusal no
/// re-key can clear.
///
/// The pair is the measurement. Same probe, same re-key, and the two stops answer differently on
/// the far side of it because they are recorded in different places — which is why the witness
/// reads both and not just the boundary.
///
/// [`TokenBudget`]: tokora::input::TokenBudget
#[test]
fn a_token_budget_refusal_outlives_the_re_key_that_clears_a_lexer_trip() {
  const BUDGET: usize = 3;
  let src = s_document(S_UNITS);

  // The lexer's own limit: the boundary is a per-regime memo, so the re-key really recovers it.
  let (lexer_side, scanned) = s_drive_fatal!(S_TIGHT, s_probe_after_rekey, &src);
  assert!(
    scanned > S_TIGHT,
    "the lexer-side cell must actually trip: scanned {scanned}, limit {S_TIGHT}"
  );
  assert_eq!(
    lexer_side,
    Ok(false),
    "a re-key drops the poison boundary — the documented limit-recovery path — so the witness \
     reads clean on the far side of it"
  );

  // The input's own budget: recorded on the tally, which the re-key does not touch.
  let ctx: ParserContext<'_, SLexer<'_>, Fatal<SErr>> = ParserContext::new(Fatal::new());
  let ctx = ctx.with_token_budget(TokenBudget::with_limitation(BUDGET));
  let budget_side = Parser::with_parser_and_context(s_probe_after_rekey, ctx)
    .parse_str_with_state(&src, ScanLimiter::with_limit(S_ROOMY));
  assert_eq!(
    budget_side,
    Ok(true),
    "the token budget's refusal is not a per-regime memo: no `Checkpoint` carries it, no re-key \
     touches it and no mutator lowers it, so the witness still reads stopped"
  );
}

/// The **descent** witness has no such gap, and that is why it is the half this branch publishes.
///
/// A resource-limit refusal from [`InputRef::descend`] is *returned*, never routed through the
/// emitter, so no emitter can unmark it and no emitter can convert it away from the counter that
/// already recorded it. Section 1's loop under a **rejecting** emitter behaves exactly as it does
/// under an accepting one: one refusal, one stop, nothing filed.
///
/// The error type is still `()`, whose `is_terminal()` is `false` for every value — so what is
/// measured here is the input-side witness alone, under the emitter that breaks the scanner half.
#[test]
fn the_descent_witness_holds_under_a_rejecting_emitter() {
  const UNITS: usize = 16;
  let src = document(UNITS);

  reset();
  let ctx: ParserContext<'_, TestLexer<'_>, Fatal<()>> = ParserContext::new(Fatal::new());
  let ctx = ctx.with_recursion_limiter(RecursionLimiter::with_limitation(TIGHT));
  let witnessed = Parser::with_context(ctx)
    .apply(root_witnessed)
    .parse_str(&src);

  assert_eq!(
    trips(),
    1,
    "the refusal ends the document on the first definition, exactly as under an accepting emitter"
  );
  assert_eq!(
    witnessed,
    Err(()),
    "and the loop stops: the descent trip never reaches the emitter, so the channel that unmarks \
     the scanner half cannot touch it"
  );
  assert_eq!(
    reports(),
    0,
    "nothing is filed, so one refusal is one diagnostic"
  );
}

// ── Section 5: a rollback erases the record, and where the bound lives decides ──

/// The lexer whose scanner bound is the crate's **own shipped** [`TokenLimiter`], held by value
/// in the state — the placement that type documents.
///
/// [`SLexer`]'s limiter is the other shape: an `Rc<Cell<_>>` the state only *points* at, so the
/// tally is reachable from every clone and no restore returns it. The two lexers are identical in
/// every other respect, which is what makes the pair below a measurement of the placement alone.
///
/// [`TokenLimiter`]: tokora::state::token_tracker::TokenLimiter
#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, extras = TokenLimiter, skip r"[ \t\r\n]+")]
enum CTok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); lex.slice().parse::<i64>().unwrap_or(0) })]
  Num(i64),
}

impl core::fmt::Display for CTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

impl From<TokenLimitExceeded> for SErr {
  fn from(_: TokenLimitExceeded) -> Self {
    SErr::Limit
  }
}

impl TokenTrait<'_> for CTok {
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

type CLexer<'a> = LogosLexer<'a, CTok>;

thread_local! {
  /// [`InputRef::at_scanner_stop`] read **where the failure was produced** — inside the
  /// speculative wrapper, before it decides.
  ///
  /// A thread-local rather than a return value because the reading has to be taken from inside a
  /// closure whose error type is the grammar's, and the whole point of section 4 is that the
  /// grammar's error type carries nothing.
  ///
  /// [`InputRef::at_scanner_stop`]: tokora::InputRef::at_scanner_stop
  static INSIDE: Cell<bool> = const { Cell::new(false) };
}

fn note_inside(reading: bool) {
  INSIDE.with(|c| c.set(reading));
}

fn inside() -> bool {
  INSIDE.with(Cell::get)
}

/// What the input answers **for itself** on the far side of the rollback — the control that says
/// whether the reading beside it was right or wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NextAnswer {
  /// A scan from the restored state yielded a token: the stop really is gone.
  Token,
  /// A genuine end of input.
  EndOfInput,
  /// The scan tripped again: the stop is still in force.
  Stopped,
}

/// The three readings a speculative wrapper's rollback is judged by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AcrossRollback {
  /// `at_scanner_stop()` inside the wrapper, at the failure.
  inside: bool,
  /// `at_scanner_stop()` after the wrapper restored its pre-trip checkpoint.
  after: bool,
  /// What the input's own next call answers from that restored state.
  next: NextAnswer,
}

/// Consumes until the scanner stops, recording the verdict **where the failure is produced**.
fn drain_recording<'inp, L, Ctx>(inp: &mut InputRef<'inp, '_, L, Ctx>) -> Result<(), SErr>
where
  L: tokora::Lexer<'inp>,
  Ctx: ParseContext<'inp, L>,
  Ctx::Emitter: Emitter<'inp, L, Error = SErr>,
{
  loop {
    match inp.try_expect_or_stop(|_| true) {
      Ok(Some(_)) => {}
      Ok(None) => return Ok(()),
      Err(e) => {
        note_inside(inp.at_scanner_stop());
        return Err(e);
      }
    }
  }
}

/// The reading after the wrapper has restored, beside what the input itself does next.
fn after_rollback<'inp, L, Ctx>(inp: &mut InputRef<'inp, '_, L, Ctx>) -> AcrossRollback
where
  L: tokora::Lexer<'inp>,
  Ctx: ParseContext<'inp, L>,
  Ctx::Emitter: Emitter<'inp, L, Error = SErr>,
{
  let after = inp.at_scanner_stop();
  let next = match inp.try_expect_or_stop(|_| true) {
    Ok(Some(_)) => NextAnswer::Token,
    Ok(None) => NextAnswer::EndOfInput,
    Err(_) => NextAnswer::Stopped,
  };
  AcrossRollback {
    inside: inside(),
    after,
    next,
  }
}

/// The three public wrappers the finding names, driven identically.
macro_rules! probe_bodies {
  ($lexer:ident, $try_attempt:ident, $attempt_parse:ident, $transaction:ident) => {
    /// `try_attempt`: the closure's `Err` rolls back and then propagates.
    fn $try_attempt<'inp, Ctx>(
      inp: &mut InputRef<'inp, '_, $lexer<'inp>, Ctx>,
    ) -> Result<AcrossRollback, SErr>
    where
      Ctx: ParseContext<'inp, $lexer<'inp>>,
      Ctx::Emitter: Emitter<'inp, $lexer<'inp>, Error = SErr>,
    {
      note_inside(false);
      let _ = inp.try_attempt(drain_recording);
      Ok(after_rollback(inp))
    }

    /// `attempt_parse`: the same rollback, reached through the three-way vocabulary.
    fn $attempt_parse<'inp, Ctx>(
      inp: &mut InputRef<'inp, '_, $lexer<'inp>, Ctx>,
    ) -> Result<AcrossRollback, SErr>
    where
      Ctx: ParseContext<'inp, $lexer<'inp>>,
      Ctx::Emitter: Emitter<'inp, $lexer<'inp>, Error = SErr>,
    {
      note_inside(false);
      let _ = inp.attempt_parse(|inp| drain_recording(inp).map(ParseAttempt::Accept));
      Ok(after_rollback(inp))
    }

    /// A rollback-on-drop [`Transaction`]: no verb at all, just the guard going out of scope.
    ///
    /// [`Transaction`]: tokora::input::Transaction
    fn $transaction<'inp, Ctx>(
      inp: &mut InputRef<'inp, '_, $lexer<'inp>, Ctx>,
    ) -> Result<AcrossRollback, SErr>
    where
      Ctx: ParseContext<'inp, $lexer<'inp>>,
      Ctx::Emitter: Emitter<'inp, $lexer<'inp>, Error = SErr>,
    {
      note_inside(false);
      {
        let mut txn = inp.begin();
        let _ = drain_recording(&mut txn);
      }
      Ok(after_rollback(inp))
    }
  };
}

probe_bodies!(SLexer, s_try_attempt, s_attempt_parse, s_transaction);
probe_bodies!(CLexer, c_try_attempt, c_attempt_parse, c_transaction);

/// Section 5's driver for the **state-side** bound whose tally is shared out of band.
macro_rules! shared_state_bound {
  ($probe:ident) => {{
    let limiter = ScanLimiter::with_limit(S_TIGHT);
    let scanned = limiter.counter();
    let ctx: ParserContext<'_, SLexer<'_>, Fatal<SErr>> = ParserContext::new(Fatal::new());
    let out = Parser::with_parser_and_context($probe, ctx).parse_str_with_state(S_SRC, limiter);
    (out, scanned.get())
  }};
}

/// The same bound, held **by value** in the state — the crate's own [`TokenLimiter`].
///
/// [`TokenLimiter`]: tokora::state::token_tracker::TokenLimiter
macro_rules! by_value_state_bound {
  ($probe:ident) => {{
    let ctx: ParserContext<'_, CLexer<'_>, Fatal<SErr>> = ParserContext::new(Fatal::new());
    Parser::with_parser_and_context($probe, ctx)
      .parse_str_with_state(S_SRC, TokenLimiter::with_limitation(S_TIGHT))
  }};
}

/// The bound on the **input** — [`TokenBudget`] — with the lexer's own limit out of reach.
///
/// [`TokenBudget`]: tokora::input::TokenBudget
macro_rules! input_bound {
  ($probe:ident) => {{
    let ctx: ParserContext<'_, SLexer<'_>, Fatal<SErr>> = ParserContext::new(Fatal::new());
    let ctx = ctx.with_token_budget(TokenBudget::with_limitation(S_TIGHT));
    Parser::with_parser_and_context($probe, ctx)
      .parse_str_with_state(S_SRC, ScanLimiter::with_limit(S_ROOMY))
  }};
}

/// A rollback erases the **record** of a scanner stop, so
/// [`InputRef::at_scanner_stop`] reads clean on its far side — through all three public
/// speculative wrappers.
///
/// The wrapper restores the checkpoint it saved *before* the trip: `None` boundary, original
/// cursor, original state. The reading is a reading of what is on record at the committed cursor,
/// and after that restore there is no record and the cursor is not at the frontier. So it answers
/// `false`, where the identical loop **without** the wrapper answers `true` — measured in
/// [`a_rejecting_emitter_hands_the_root_loop_an_unmarked_scanner_stop`].
///
/// `inside` is the same reading taken where the failure is produced, before the wrapper decides.
/// It is `true` in every row, which is the whole of the contract's answer: the record exists, and
/// it exists until the rollback discards it along with everything else the attempt did.
///
/// [`InputRef::at_scanner_stop`]: tokora::InputRef::at_scanner_stop
#[test]
fn every_speculative_wrapper_restores_a_checkpoint_that_predates_the_trip() {
  for (name, (out, scanned)) in [
    ("try_attempt", shared_state_bound!(s_try_attempt)),
    ("attempt_parse", shared_state_bound!(s_attempt_parse)),
    ("transaction drop", shared_state_bound!(s_transaction)),
  ] {
    assert!(
      scanned > S_TIGHT,
      "{name}: the fixture must actually trip the scan budget: scanned {scanned}, limit {S_TIGHT}"
    );
    assert_eq!(
      out,
      Ok(AcrossRollback {
        inside: true,
        after: false,
        next: NextAnswer::Stopped,
      }),
      "{name}: the stop is on record where the failure is produced, gone on the far side of the \
       rollback, and STILL IN FORCE — the input's own next call trips again over a tally no \
       restore returned"
    );
  }
}

/// The row above is a false negative **because of where that bound lives**, and this is the
/// control that says so: the crate's own [`TokenLimiter`], held by value in the state, makes the
/// identical `false` the **right** answer.
///
/// `TokenLimiter` documents it outright — *"a rollback reinstalls the state a checkpoint saved, so
/// the baseline becomes that saved count and everything spent after it is given back"*, and *"an
/// abandoned speculation's tokens are not in it, so a refund is the correct answer"*. The refund
/// is what the third column reads: a scan from the restored state yields a token, so no stop is in
/// force and `at_scanner_stop` saying so is correct.
///
/// The two rows differ in nothing but the tally's placement, and they need **opposite** answers
/// after the same rollback. That is what no carrier riding the rollback can supply: the fact that
/// separates them lives in `L::State`, which is precisely the thing a checkpoint restores.
///
/// [`TokenLimiter`]: tokora::state::token_tracker::TokenLimiter
#[test]
fn a_by_value_state_bound_makes_the_same_false_the_right_answer() {
  for (name, out) in [
    ("try_attempt", by_value_state_bound!(c_try_attempt)),
    ("attempt_parse", by_value_state_bound!(c_attempt_parse)),
    ("transaction drop", by_value_state_bound!(c_transaction)),
  ] {
    assert_eq!(
      out,
      Ok(AcrossRollback {
        inside: true,
        after: false,
        next: NextAnswer::Token,
      }),
      "{name}: the rollback refunded the tally — which `TokenLimiter` documents as the correct \
       answer — so the stop really is gone and reading `false` is right"
    );
  }
}

/// And the bound the crate ships for *work performed* — [`TokenBudget`], on the input — survives
/// every one of the three rollbacks, so the reading survives with it.
///
/// No `Checkpoint` carries the input's tally, no re-key touches it and no mutator lowers it. That
/// is not a property a witness had to be given: it is the placement, and
/// [`InputRef::at_scanner_stop`] already reads it. A consumer that speculates over a scanner bound
/// gets a rollback-proof verdict by putting the bound here — which is the same sentence
/// `TokenLimiter`'s own documentation ends on.
///
/// [`TokenBudget`]: tokora::input::TokenBudget
/// [`InputRef::at_scanner_stop`]: tokora::InputRef::at_scanner_stop
#[test]
fn an_input_side_bound_outlives_every_one_of_the_three_rollbacks() {
  for (name, out) in [
    ("try_attempt", input_bound!(s_try_attempt)),
    ("attempt_parse", input_bound!(s_attempt_parse)),
    ("transaction drop", input_bound!(s_transaction)),
  ] {
    assert_eq!(
      out,
      Ok(AcrossRollback {
        inside: true,
        after: true,
        next: NextAnswer::Stopped,
      }),
      "{name}: the refusal is recorded on the input's own tally, which no rollback reaches, so \
       the verdict is the same on both sides of the restore"
    );
  }
}

/// The root loop of section 4 with its element **speculating**, and the amplification that opens
/// when the verdict is read on the wrong side of the rollback.
///
/// `read_inside` is the whole variable. Reading where the failure is produced ends the document at
/// the stop; reading after the wrapper has restored reads clean, so the loop files the failure as
/// an ordinary syntax error, re-keys, and retries — from a state identical to the one it just
/// failed in. There is **no progress at all**: the element consumes nothing, so the loop neither
/// advances nor terminates, and only [`S_REPORT_CAP`] stops the fixture.
///
/// That is strictly worse than section 4's unwrapped loop, which files one report per *remaining
/// token* and does end. It is also the case section 4's named residue did not cover: the residue's
/// bound was "the region behind the frontier is replayable, so the loop makes real progress and
/// re-reaches the stop", and a wrapper that rolls back keeps none of what it replayed.
fn s_root_speculating<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
  read_inside: bool,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  let mut parsed = 0usize;
  loop {
    let mut stopped = false;
    let element = inp.try_attempt(|inp| match inp.try_expect_or_stop(|_| true) {
      Ok(Some(_)) => Ok(true),
      Ok(None) => Ok(false),
      Err(e) => {
        stopped = inp.at_scanner_stop();
        Err(e)
      }
    });
    match element {
      Ok(true) => parsed += 1,
      Ok(false) => return Ok(parsed),
      Err(e) => {
        let verdict = if read_inside {
          stopped
        } else {
          inp.at_scanner_stop()
        };
        if e.is_terminal() || verdict {
          return Err(e);
        }
        note_report();
        if reports() >= S_REPORT_CAP {
          return Ok(parsed);
        }
        inp.state_mut();
      }
    }
  }
}

fn s_root_speculating_inside<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  s_root_speculating(inp, true)
}

fn s_root_speculating_after<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SLexer<'inp>, Ctx>,
) -> Result<usize, SErr>
where
  Ctx: ParseContext<'inp, SLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SLexer<'inp>, Error = SErr>,
{
  s_root_speculating(inp, false)
}

/// Reading the verdict on the far side of a rollback costs the loop its termination; reading it
/// where the failure is produced does not.
///
/// The pair is the measurement, and the placement is its only variable — same loop, same wrapper,
/// same source, same budget.
#[test]
fn the_verdict_read_after_the_rollback_leaves_the_loop_without_progress() {
  reset();
  let (after, scanned) = shared_state_bound!(s_root_speculating_after);
  assert!(
    scanned > S_TIGHT,
    "the fixture must actually trip: scanned {scanned}, limit {S_TIGHT}"
  );
  assert_eq!(
    reports(),
    S_REPORT_CAP,
    "the loop never stops on its own: every turn rolls back to the state the last one failed in, \
     so only the fixture's brake ends it. Verdict was {after:?}"
  );
  assert_eq!(
    after,
    Ok(S_TIGHT),
    "and it has parsed exactly what the pre-trip prefix holds — no turn after the first stop \
     advanced the cursor by a single token"
  );

  reset();
  let (inside, scanned) = shared_state_bound!(s_root_speculating_inside);
  assert!(scanned > S_TIGHT, "the inside-read run must trip too");
  assert_eq!(
    inside,
    Err(SErr::Limit),
    "read where the failure is produced, the same loop ends the document at the stop"
  );
  assert_eq!(
    reports(),
    0,
    "one stop is one stop: nothing was filed as an ordinary syntax error"
  );
}
