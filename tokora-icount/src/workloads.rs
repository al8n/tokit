//! The nine repetition axes, one driver and one deterministic source each, plus one control.
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
//! ## Why `scan_drain` is here
//!
//! Lexing, spanning and the peek cache are inside every driver below, and they are work the
//! repetition engine did not do. `scan_drain` runs a token stream through `try_expect` with no
//! repetition engine at all, so a delta in it is a scanner, lexer or input-layer change — which
//! is what makes it the first thing to read when all nine axes move at once, and the thing that
//! says a shared move is NOT the repetition families' doing.
//!
//! It is a CONTROL and not a tenth axis, and specifically it is **not a per-axis dilution
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

/// The group length for group `g`: `BOUND`, or one over it every `OVERFLOW_EVERY`th group.
fn group_len(g: usize) -> usize {
  if g.is_multiple_of(OVERFLOW_EVERY) {
    BOUND + 1
  } else {
    BOUND
  }
}

/// `1 2 ... 8 ; 9 10 ... ;` — whitespace-separated groups terminated by `;`.
fn space_groups() -> String {
  let mut s = String::with_capacity(ELEMENTS * 8);
  let mut i = 0usize;
  let mut g = 0usize;
  while i < ELEMENTS {
    for _ in 0..group_len(g) {
      let _ = write!(s, "{} ", nth(i));
      i += 1;
    }
    s.push_str("; ");
    g += 1;
  }
  s
}

/// `1,2,...,8;9,10,...;` — comma-separated groups terminated by `;`.
fn comma_groups(trailing: bool) -> String {
  let mut s = String::with_capacity(ELEMENTS * 8);
  let mut i = 0usize;
  let mut g = 0usize;
  while i < ELEMENTS {
    for k in 0..group_len(g) {
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
  "sep_while" => sep_while(comma_groups(true)),
  "at_most" => at_most(space_groups()),
  "separated_while" => separated_while(comma_run()),
  "exactly" => exactly(space_groups()),
  "repeated" => repeated(int_run()),
  "separated" => separated(comma_run()),
  "delimited" => delimited(paren_groups()),
  "repeated_while" => repeated_while(int_run()),
  "at_least" => at_least(space_groups()),
  "scan_drain" => scan_drain(int_run()),
}
