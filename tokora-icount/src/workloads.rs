//! The nine repetition axes, one driver and one deterministic source each, three of them read a
//! second time over short collections, plus one control.
//!
//! ## Why the axes are these nine
//!
//! `tokora/src/parser/many/` is a monomorphised matrix: 259 files and 338 `impl` blocks over
//! {family} x {plain, delimited} x {bound} x {leading/trailing policy}. The matrix exists
//! BECAUSE each combination is a separate specialisation, so any consolidation of it into
//! shared engines is exactly the shape that loses inlining. These nine reach one engine each,
//! chosen so that every family is represented and the three the rest of the suite cannot see
//! are covered first:
//!
//! | workload           | engine reached                                     |
//! |--------------------|----------------------------------------------------|
//! | `sep_while`        | `many/sep_while/parse/allow_trailing/at_most.rs`   |
//! | `at_most`          | `many/repeated/at_most.rs` + `options/at_most.rs`  |
//! | `separated_while`  | `many/sep_while/parse/unbounded.rs`                |
//! | `exactly`          | `many/repeated/bounded.rs`                         |
//! | `repeated`         | `many/repeated/unbounded.rs`                       |
//! | `separated`        | `many/sep/parse/unbounded.rs`                      |
//! | `delimited`        | `many/sep/delim/unbounded.rs`                      |
//! | `repeated_while`   | `many/repeated_while/unbounded.rs`                 |
//! | `at_least`         | `many/repeated/at_least.rs`                        |
//!
//! The first three are the ones with no other witness anywhere: `sep_while` is 91 of the 259
//! files and the most heavily monomorphised axis in the matrix, and neither it nor `at_most`
//! nor `separated_while` is reached by any smear end-to-end parse. The remaining six are
//! controls — a regression in them shows up downstream too, which is what makes them useful
//! here: a delta that moves all nine together is a substrate change, and a delta that moves
//! only `sep_while` is that family's own.
//!
//! Those same three are also read a SECOND time, over collections of `SHALLOW` elements instead
//! of the deep sources' 8 or 512. Same engine and the same combinator expression, character for
//! character; `separated_while` is the one whose driver has to add a drain, and that driver says
//! why. See "Deep and shallow" below for the failure the second reading is there for:
//!
//! | workload                  | reads the engine of | collections an iteration |
//! |---------------------------|---------------------|--------------------------|
//! | `sep_while_shallow`       | `sep_while`         | 256 (deep: 63)           |
//! | `at_most_shallow`         | `at_most`           | 256 (deep: 63)           |
//! | `separated_while_shallow` | `separated_while`   | 256 (deep: 1)            |
//!
//! There is no `exactly` combinator in the crate. "Exactly n" is `Bounded` with equal bounds —
//! `.at_least(n).at_most(n)` — which is its own engine file (`many/repeated/bounded.rs`),
//! distinct from both `at_least.rs` and `at_most.rs`. The workload keeps the name the shape is
//! usually called by; the engine it reaches is the one named in the table.
//!
//! ## Why the sources are small
//!
//! A wall-clock benchmark needs a large input so a full parse swamps the fixed per-parse setup
//! and the run-to-run spread. An instruction count has neither problem: `ci/icount/measure.py`
//! differences two readings of this binary at two iteration counts, which cancels every fixed
//! cost EXACTLY rather than diluting it, and
//! there is no spread to swamp. What is left is a straight trade against callgrind's 50-100x
//! slowdown, so the sources are sized at `ELEMENTS` = 512 elements — a few kilobytes, tens of
//! thousands of instructions an iteration, and a whole job that finishes in minutes.
//!
//! ## Deep and shallow: one engine, two ways to get more expensive
//!
//! A repetition engine can cost more per ELEMENT or more per COLLECTION, and one source cannot
//! read both. A workload that parses `n` elements in one collection divides a per-collection
//! cost by `n` before any threshold sees it — so the deeper the source, the more completely a
//! per-collection regression is amortised away.
//!
//! Both failures are real and they are not the same failure:
//!
//! * **Per element** is a handler inside the loop losing its inline. `#[inline(never)]` on
//!   `SeparatedWhile::handle_continue` — the per-element handler every `sep_while/parse/*`
//!   specialisation inlines today — reads +10.235% on `sep_while` and +10.332% on
//!   `separated_while`. Every workload above reads it loudly, because it is paid once per element
//!   and every source is 512 elements long.
//!
//! * **Per collection** is the engine AROUND the loop losing its inline. `#[inline(never)]` one
//!   level out, on `SeparatedWhile::parse`, is worth 44 instructions per collection — and 44
//!   instructions spread over an 8-element collection is +0.80%, which a +1% threshold reports
//!   as `ok`. Spread over a 512-element collection it is +0.02%, which is nothing at all.
//!
//! The second is the shape #259 is most likely to produce. It consolidates 259 files and 338
//! `impl` blocks into shared engines, and a shared engine is one a caller stops inlining once
//! per CALL, not once per element. A suite that only reads deep sources is close to blind to the
//! change it was built for.
//!
//! So the three axes with no other witness anywhere are read a second time over `SHALLOW`-element
//! collections: same driver, same combinator expression, same bound, same total element count,
//! spread over many short collections instead of few long ones. `separated_while` is the one
//! exception and its driver says why — unbounded over a run is ONE collection, so its shallow
//! twin needs the drain the bounded axes already have.
//!
//! ### What each row can see, exactly
//!
//! A cost of `k` instructions per collection moves a row by `C * k / D`, where `C` is the
//! collections one iteration parses and `D` is the row's Ir per iteration. That is measured, not
//! fitted: the `SeparatedWhile::parse` plant reads back as 44.0 instructions per collection on
//! all three of the sources that hold 4, 2 and 1 elements — 128, 256 and 512 collections an
//! iteration — 43.0 on the one that holds none, and 42.9 on the deep row, whose groups vary in
//! length. One constant across a fourfold range of `C` is what makes `0.01 * D / C` an exact
//! claim per row rather than an estimate: it is the smallest per-collection regression that
//! reaches a +1% ceiling.
//!
//! | workload                  | collections | Ir/iteration | +1% is a per-collection cost of |
//! |---------------------------|------------:|-------------:|--------------------------------:|
//! | `sep_while`               |          63 |      343,765 |                       55 instr. |
//! | `sep_while_shallow`       |         256 |      453,427 |                       18 instr. |
//! | `at_most`                 |          63 |      212,910 |                       34 instr. |
//! | `at_most_shallow`         |         256 |      316,211 |                       12 instr. |
//! | `separated_while`         |           1 |      277,520 |                    2,775 instr. |
//! | `separated_while_shallow` |         256 |      424,468 |                       17 instr. |
//!
//! The `separated_while` pair is the reason this is worth doing at all. Unbounded over a run is
//! one collection an iteration, so its deep row cannot see a per-collection regression smaller
//! than 2,775 instructions — a whole engine's worth. Its shallow twin sees 17.
//!
//! The deep rows STAY, and not out of caution: a per-element cost is amortised the OTHER way.
//! Both halves of a pair walk the same 512 elements, so the same cost per element is the same
//! number of instructions on each — divided, on the shallow row, by the larger per-iteration
//! total. Measured rather than argued: the per-element plant reads +10.235% on `sep_while` and
//! +8.187% on `sep_while_shallow`, +10.332% on `separated_while` and +6.695% on its twin. Each
//! granularity keeps the row that reads it best, and neither row reads both best.
//!
//! ### Why `SHALLOW` is 2
//!
//! Four candidate sources were measured against the `SeparatedWhile::parse` plant in one
//! comparison, at 4, 2, 1 and 0 elements per collection, holding the total at `ELEMENTS`:
//!
//! | elements/collection  | collections | the plant reads | implied cost per collection |
//! |---------------------:|------------:|----------------:|----------------------------:|
//! | 8 (the deep row)     |          63 |         +0.798% |                        42.9 |
//! | 4                    |         128 |         +1.541% |                        44.0 |
//! | 2                    |         256 |         +2.481% |                        44.0 |
//! | 1                    |         512 |         +3.568% |                        44.0 |
//! | 0                    |         512 |         +9.523% |                        43.0 |
//!
//! Those five are one sweep over candidate sources, so they are read against each other rather
//! than against the shipped rows; as shipped, the 2-element source reads the same plant at
//! +2.484% and the deep row at +0.798%.
//!
//! Sharper all the way down, so "most sensitive" does not decide it — the smallest collection
//! that is still a collection does:
//!
//! * **0** parses no element and crosses no separator. It measures the engine's entry and its
//!   exit and nothing in between, so a cost in the loop's prologue is invisible to it. Its
//!   checksum is also zero: `drain_groups!` folds element counts, a source of empty groups folds
//!   to nothing, and a zero checksum is exactly the reading `measure.py` refuses — it is what a
//!   workload that parsed nothing reports.
//! * **1** parses an element but crosses no separator, because a one-element list has no
//!   separator in it. Two of these three rows are the separator's own engines; buying five more
//!   instructions of resolution by never exercising the separator is the wrong trade.
//! * **2** is the smallest source that runs one whole cycle — element, separator, element,
//!   decision, exit — and it reads at +2.484% the plant that the deep row reports as `ok`.
//!
//! One thing a shallow row deliberately gives up: with `SHALLOW` under `BOUND`, no group reaches
//! its limit, so the `TooMany` short-circuit does not appear in a shallow row. It has not gone
//! anywhere — it is in the deep twin, which is the other reason both rows exist.
//!
//! ## Why `scan_drain` is here
//!
//! Lexing, spanning and the peek cache are inside every driver below, and they are work the
//! repetition engine did not do. `scan_drain` runs a token stream through `try_expect` with no
//! repetition engine at all, so a delta in it is a scanner, lexer or input-layer change — which
//! is what makes it the first thing to read when all nine axes move at once, and the thing that
//! says a shared move is NOT the repetition families' doing.
//!
//! It is a CONTROL and not another axis, and specifically it is **not a per-axis dilution
//! denominator**: it reads `int_run`, and the other sources carry different numbers of tokens
//! per element (a comma source has nearly two, the grouped sources add a terminator, the paren
//! source adds two). `scan_drain / axis` is therefore an order-of-magnitude reading of how much
//! of an axis is scanning, not a subtraction that leaves the engine behind. What this gate can
//! actually resolve is settled by planting a lost inline and reading the delta, not by
//! arithmetic over this number.

use core::fmt::Write as _;
use std::hint::black_box;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, EmitterView, InputRef, Parse, ParseContext, ParseInput, Parser,
  TryParseInput,
  cache::{Peeked, PeekedTokenExt},
  emitter::{
    FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  parser::Action,
};

use crate::fixture::{Err0, Lex, Tok, int_elem, try_int};

/// Elements per source. See the module header for why this is small rather than large.
const ELEMENTS: usize = 512;

/// The bound the three bounded axes carry, and the group length every grouped source uses.
const BOUND: usize = 8;

/// The minimum the `at_least` axis carries. Every group satisfies it, so that axis measures the
/// accepting path of the minimum check rather than a `TooFew` trip.
const MINIMUM: usize = 4;

/// One group in `OVERFLOW_EVERY` is one element over `BOUND`, so a single pass measures both
/// the accepting path and the `TooMany` short-circuit of the bounded axes.
const OVERFLOW_EVERY: usize = 8;

/// Elements per collection in the SHALLOW sources — see the module header's second table for
/// why it is 2 and not 4, 1 or 0. Every other constant a shallow row meets is its deep twin's,
/// so the only thing that differs between a pair of rows is how many elements one collection
/// holds.
const SHALLOW: usize = 2;

// ── Deterministic sources ───────────────────────────────────────────────────────────────────
//
// Every source is generated from a counter — no randomness, no clock, no environment — so a
// fixture is byte-identical between runs, between machines, and between the two sides of a
// comparison. That is not a nicety here: the whole gate rests on the base and head binaries
// being handed the same bytes.

fn nth(i: usize) -> u32 {
  (i as u32).wrapping_mul(2654435761) % 100_000
}

/// `1 2 3 ...` — a whitespace-separated run for the repetition families.
fn int_run() -> String {
  let mut s = String::with_capacity(ELEMENTS * 8);
  for i in 0..ELEMENTS {
    let _ = write!(s, "{} ", nth(i));
  }
  s
}

/// `1,2,3,...` — a comma-separated run for the separator families.
fn comma_run() -> String {
  let mut s = String::with_capacity(ELEMENTS * 8);
  for i in 0..ELEMENTS {
    if i > 0 {
      s.push(',');
    }
    let _ = write!(s, "{}", nth(i));
  }
  s
}

/// A group-length policy: how many elements group `g` holds. The two below are the ONLY
/// difference between a deep source and its shallow twin — same alphabet, same separator, same
/// terminator, same total element count, different number of collections to spread it over.
type GroupLen = fn(usize) -> usize;

/// The deep policy: `BOUND`, or one over it every `OVERFLOW_EVERY`th group.
fn group_len(g: usize) -> usize {
  if g.is_multiple_of(OVERFLOW_EVERY) {
    BOUND + 1
  } else {
    BOUND
  }
}

/// The shallow policy: `SHALLOW` elements, every group. Nothing reaches `BOUND`, so the bounded
/// axes' `TooMany` short-circuit does not appear in a shallow row — it stays where it already
/// was, in the deep twin, and the shallow row is the accepting path alone.
fn shallow_len(_g: usize) -> usize {
  SHALLOW
}

/// `1 2 ... 8 ; 9 10 ... ;` — whitespace-separated groups terminated by `;`.
fn space_groups(len: GroupLen) -> String {
  let mut s = String::with_capacity(ELEMENTS * 8);
  let mut i = 0usize;
  let mut g = 0usize;
  while i < ELEMENTS {
    for _ in 0..len(g) {
      let _ = write!(s, "{} ", nth(i));
      i += 1;
    }
    s.push_str("; ");
    g += 1;
  }
  s
}

/// `1,2,...,8;9,10,...;` — comma-separated groups terminated by `;`.
fn comma_groups(trailing: bool, len: GroupLen) -> String {
  let mut s = String::with_capacity(ELEMENTS * 8);
  let mut i = 0usize;
  let mut g = 0usize;
  while i < ELEMENTS {
    for k in 0..len(g) {
      if k > 0 {
        s.push(',');
      }
      let _ = write!(s, "{}", nth(i));
      i += 1;
    }
    if trailing {
      s.push(',');
    }
    s.push(';');
    g += 1;
  }
  s
}

/// `(1,2,...,8) (9,...) ...` — parenthesised comma lists, for the delimited arm.
fn paren_groups() -> String {
  let mut s = String::with_capacity(ELEMENTS * 8);
  let mut i = 0usize;
  let mut g = 0usize;
  while i < ELEMENTS {
    s.push('(');
    for k in 0..group_len(g) {
      if k > 0 {
        s.push(',');
      }
      let _ = write!(s, "{}", nth(i));
      i += 1;
    }
    s.push_str(") ");
    g += 1;
  }
  s
}

// ── The `*_while` families' decision ─────────────────────────────────────────────────────────

/// Continue while the upcoming element-position token is an integer; stop at anything else —
/// the group's trailing `;` in the grouped sources, end of input in the runs.
///
/// The condition is asked before every candidate element, INCLUDING the first, and is handed
/// the peek window positioned at that element's own leading token rather than at the separator.
/// That is the whole point of the `*_while` families: `separated_by_comma` already continues on
/// "a separator is there" with no callback, so a condition that only re-checked the separator
/// would measure indirection for a question the crate answers itself.
fn decide<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, Lex<'inp>, U1>,
  _emitter: EmitterView<'_, 'inp, Lex<'inp>, Ctx::Emitter>,
) -> Result<Action, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>,
{
  Ok(match peeked.pop_front() {
    Some(t) if matches!(t.token(), Tok::Int(_)) => Action::Continue,
    _ => Action::Stop,
  })
}

// ── The drain loop the grouped axes share ────────────────────────────────────────────────────

/// Runs `one` over each `;`-terminated group until the input is empty, folding the outcomes
/// into a checksum. An `Err` from `one` is a deliberate bound trip in every grouped source, so
/// it counts as one rather than aborting.
macro_rules! drain_groups {
  ($inp:expr, $one:expr) => {{
    let inp = $inp;
    let mut n = 0usize;
    loop {
      // Scope the peek borrow: an empty input ends the drain before a group is attempted that
      // is not there.
      let done = {
        let peeked = inp.peek_one()?;
        peeked.is_none()
      };
      if done {
        break;
      }
      match $one(inp) {
        Ok(items) => n += black_box(items).len(),
        Err(_) => n += 1,
      }
      // Resynchronise on the group terminator by CONSUMING UP TO the next `;` rather than
      // requiring one to be next. A group that trips its bound leaves the cursor somewhere
      // inside itself, and where exactly is a property of the family — `at_most` over a
      // whitespace run stops with the `;` next, `sep_while` over a trailing-comma group stops
      // with the comma next. Requiring the terminator immediately made the second break out of
      // the drain after ONE group: the workload reported a checksum of 1, ran a fraction of the
      // work the others ran, and would have gone on doing so silently. The resync is a handful
      // of `try_expect` calls per overflowing group, identical on both sides of a comparison.
      let mut terminated = false;
      while let Some(t) = inp.try_expect(|_| true)? {
        if matches!(t.data(), Tok::Semi) {
          terminated = true;
          break;
        }
      }
      if !terminated {
        break;
      }
    }
    Ok(n)
  }};
}

// ── 1. repeated — `many/repeated/unbounded.rs` ───────────────────────────────────────────────

fn repeated<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0> + FullContainerEmitter<'inp, Lex<'inp>>,
{
  let items: Vec<i64> = try_int.repeated().collect().parse_input(inp)?;
  Ok(black_box(items).len())
}

// ── 2. repeated_while — `many/repeated_while/unbounded.rs` ───────────────────────────────────

fn repeated_while<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0> + FullContainerEmitter<'inp, Lex<'inp>>,
{
  let items: Vec<i64> = int_elem
    .repeated_while::<_, U1>(decide::<Ctx>)
    .collect()
    .parse_input(inp)?;
  Ok(black_box(items).len())
}

// ── 3. separated — `many/sep/parse/unbounded.rs` ─────────────────────────────────────────────

fn separated<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + SeparatedEmitter<'inp, Lex<'inp>>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, Lex<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, Lex<'inp>>,
{
  let items: Vec<i64> = try_int.separated_by_comma().collect().parse_input(inp)?;
  Ok(black_box(items).len())
}

// ── 4. separated_while — `many/sep_while/parse/unbounded.rs` ─────────────────────────────────

fn separated_while<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + SeparatedEmitter<'inp, Lex<'inp>>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, Lex<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, Lex<'inp>>,
{
  let items: Vec<i64> = int_elem
    .separated_by_comma_while::<_, U1>(decide::<Ctx>)
    .collect()
    .parse_input(inp)?;
  Ok(black_box(items).len())
}

/// The same engine as `separated_while` above, read one short collection at a time.
///
/// `separated_while` is unbounded, so over a run it is ONE collection and there is no second
/// call to amortise a per-collection cost against. Its shallow twin therefore needs the drain
/// the bounded axes already have: `decide` stops the collection at the group's `;`, the drain
/// steps over it, and the next `parse` starts. The combinator expression is the one above,
/// character for character.
fn separated_while_grouped<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>,
) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + SeparatedEmitter<'inp, Lex<'inp>>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, Lex<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, Lex<'inp>>,
{
  drain_groups!(inp, |inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>| {
    let items: Result<Vec<i64>, Err0> = int_elem
      .separated_by_comma_while::<_, U1>(decide::<Ctx>)
      .collect()
      .parse_input(inp);
    items
  })
}

// ── 5. at_most — `many/repeated/at_most.rs` ──────────────────────────────────────────────────

fn at_most<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + TooManyEmitter<'inp, Lex<'inp>>,
{
  drain_groups!(inp, |inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>| {
    let items: Result<Vec<i64>, Err0> =
      try_int.repeated().at_most(BOUND).collect().parse_input(inp);
    items
  })
}

// ── 6. at_least — `many/repeated/at_least.rs` ────────────────────────────────────────────────

fn at_least<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + TooFewEmitter<'inp, Lex<'inp>>,
{
  drain_groups!(inp, |inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>| {
    let items: Result<Vec<i64>, Err0> = try_int
      .repeated()
      .at_least(MINIMUM)
      .collect()
      .parse_input(inp);
    items
  })
}

// ── 7. exactly — `many/repeated/bounded.rs` ──────────────────────────────────────────────────

fn exactly<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + TooFewEmitter<'inp, Lex<'inp>>
    + TooManyEmitter<'inp, Lex<'inp>>,
{
  drain_groups!(inp, |inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>| {
    let items: Result<Vec<i64>, Err0> = try_int
      .repeated()
      .at_least(BOUND)
      .at_most(BOUND)
      .collect()
      .parse_input(inp);
    items
  })
}

// ── 8. delimited — `many/sep/delim/unbounded.rs` ─────────────────────────────────────────────

fn delimited<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + SeparatedEmitter<'inp, Lex<'inp>>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + UnclosedEmitter<'inp, Lex<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, Lex<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, Lex<'inp>>,
{
  let mut n = 0usize;
  loop {
    let done = {
      let peeked = inp.peek_one()?;
      peeked.is_none()
    };
    if done {
      break;
    }
    let items: Vec<i64> = try_int
      .separated_by_comma()
      .delimited_by_parens()
      .collect()
      .parse_input(inp)?;
    n += black_box(items).len();
  }
  Ok(n)
}

// ── 9. sep_while — `many/sep_while/parse/allow_trailing/at_most.rs` ──────────────────────────

fn sep_while<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>
    + SeparatedEmitter<'inp, Lex<'inp>>
    + FullContainerEmitter<'inp, Lex<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, Lex<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, Lex<'inp>>
    + TooManyEmitter<'inp, Lex<'inp>>,
{
  drain_groups!(inp, |inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>| {
    let items: Result<Vec<i64>, Err0> = int_elem
      .separated_by_comma_while::<_, U1>(decide::<Ctx>)
      .allow_trailing()
      .at_most(BOUND)
      .collect()
      .parse_input(inp);
    items
  })
}

// ── Control. Not an axis: names nothing under `parser/many/`. ────────────────────────────────

fn scan_drain<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx>) -> Result<usize, Err0>
where
  Ctx: ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: Emitter<'inp, Lex<'inp>, Error = Err0>,
{
  let mut n = 0usize;
  while inp.try_expect(|_| true)?.is_some() {
    n += 1;
  }
  Ok(n)
}

// ── The registry ────────────────────────────────────────────────────────────────────────────

/// The one place a workload name is written.
///
/// It generates both `NAMES` — which `--list` prints and `ci/icount/measure.py` iterates — and
/// the `match` that runs one. A workload the macro does not carry is a workload the gate does not
/// measure, and there is no second list for the first to drift from.
///
/// The arm calls its driver DIRECTLY rather than through a `fn` pointer, and that is not
/// stylistic. A table of `fn` pointers puts an un-inlinable boundary between the loop and the
/// driver; on a small kernel that boundary is the measurement, and this instrument's whole
/// subject is inlining. Here the compiler sees a direct call to a monomorphic function and the
/// only thing that stops it hoisting the parse out of the loop is `black_box` on the source.
///
/// The source is built INSIDE the arm and OUTSIDE the loop, so its cost is a fixed per-process
/// cost — which `ci/icount/measure.py` cancels exactly by differencing two readings at two
/// iteration counts rather than diluting it with a large input.
macro_rules! workloads {
  ($($name:literal => $driver:ident($src:expr)),* $(,)?) => {
    /// Every workload, in the order the gate reports them: the three with no other witness
    /// anywhere first, then the six controls, then `scan_drain`.
    pub const NAMES: &[&str] = &[$($name),*];

    /// Runs `iters` iterations of `name`, returning a checksum folded over every iteration —
    /// so nothing is optimised away, and a fixture that stops parsing what it used to parse
    /// changes the number rather than passing quietly.
    ///
    /// `None` means the name is not in the table.
    pub fn run(name: &str, iters: u64) -> Option<usize> {
      match name {
        $(
          $name => {
            let src = $src;
            let src: &str = src.as_str();
            let mut acc = 0usize;
            for _ in 0..iters {
              let n = Parser::new()
                .apply($driver)
                .parse_str(black_box(src))
                .expect(concat!($name, ": the fixture no longer parses"));
              acc = acc.wrapping_add(black_box(n));
            }
            Some(acc)
          }
        )*
        _ => None,
      }
    }
  };
}

workloads! {
  "sep_while" => sep_while(comma_groups(true, group_len)),
  "sep_while_shallow" => sep_while(comma_groups(true, shallow_len)),
  "at_most" => at_most(space_groups(group_len)),
  "at_most_shallow" => at_most(space_groups(shallow_len)),
  "separated_while" => separated_while(comma_run()),
  "separated_while_shallow" => separated_while_grouped(comma_groups(false, shallow_len)),
  "exactly" => exactly(space_groups(group_len)),
  "repeated" => repeated(int_run()),
  "separated" => separated(comma_run()),
  "delimited" => delimited(paren_groups()),
  "repeated_while" => repeated_while(int_run()),
  "at_least" => at_least(space_groups(group_len)),
  "scan_drain" => scan_drain(int_run()),
}
