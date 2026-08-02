#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! What a **discarding error sink** does to the two errors `pratt_limit.rs` pins.
//!
//! `pratt_limit.rs` drives its ladder through `LimErr`, a grammar error that *stores* both
//! payloads and delegates `is_terminal`. This file drives the same shape through `()` — the sink
//! every error type under `tokora::error` carries a `From` into, `RecursionLimitReached` and
//! `NonAssociativeChain` included — and **measures** the difference rather than reading it off the
//! type signatures.
//!
//! The conversion is not new: fourteen of the sixteen `From<..> for ()` impls under
//! `tokora/src/error/` predate this ladder, `terminal.rs`'s own included. What is new is that two
//! of the sixteen now sit on a **resource guard** rather than on a malformed-input report, and the
//! two behave differently under the sink:
//!
//! * [`NonAssociativeChain`](tokora::error::NonAssociativeChain) is non-terminal to begin with, so
//!   the sink changes nothing. The cells below run the same recovery through both error types and
//!   get the same answer.
//! * [`RecursionLimitReached`](tokora::error::RecursionLimitReached) is terminal for every value
//!   and `()` is never terminal, so the sink **does** change the answer: `Recover` spends a trip
//!   it would otherwise re-raise.
//!
//! # What the sink does not cost
//!
//! The *stop* semantics are lost; the *stack safety* is not, and that half is worth measuring
//! rather than arguing. `Descent`'s destructor releases its level on the unwind that carries the
//! error out, so by the time any recoverer is handed the converted value every frame the budget
//! was protecting has already returned. Both halves are read directly here: the depth cell through
//! [`InputRef::recursion`], and the **native** stack through the address of a local, which is the
//! quantity the depth cell is only a proxy for.
//!
//! # Termination defects do not fail; they hang
//!
//! A spent trip is a retried trip, and a retry that makes no progress is an infinite loop rather
//! than a red test. Every cell here runs under [`bounded`], which puts a hard wall-clock wall
//! around the parse — the same reason `pratt_limit.rs`'s deep cells carry one.

mod common;

use core::cell::Cell;

use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  TryParseInput,
  emitter::{Fatal, FullContainerEmitter},
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  state::recursion_tracker::RecursionLimiter,
  try_parse_input::ParseAttempt,
};

use common::{Power, TestLexer, Token};

// ═══════════════════════════════════════════════════════════════════════════════
// The ladder, over the discarding sink
// ═══════════════════════════════════════════════════════════════════════════════
//
// The shape `pratt_limit.rs` uses, with `()` where `LimErr` was. Only the two operators the cells
// below need are kept: `-` prefix (one frame per level, the deep-recursion shape) and `;`
// non-associative infix (the chain constraint, and a convenient sync point).

const P_PREFIX: Power = Power(100);
const P_CHAIN: Power = Power(5);

fn lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattLHS<String, &'static str, Power>, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  // Every LHS call is one live pratt frame, so this is where the native stack is deepest.
  note_stack_mark();
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(PrattLHS::Operand(n.to_string())),
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new("-", P_PREFIX))),
      _ => Err(()),
    },
    None => Err(()),
  }
}

fn rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<PrattRHS<&'static str, &'static str, &'static str, &'static str, Power>, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Semi => PrattRHS::Infix(Precedenced::new(PrattInfix::Neither(";"), P_CHAIN)),
      _ => PrattRHS::End,
    },
    None => PrattRHS::End,
  })
}

fn fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, Power>,
) -> Result<String, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(std::format!("({}{operand})", op.into_data()))
}

fn fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: String,
  right: String,
  op: Precedenced<PrattInfix<&'static str, &'static str, &'static str>, Power>,
) -> Result<String, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  let (PrattInfix::Left(s) | PrattInfix::Right(s) | PrattInfix::Neither(s)) = op.into_data();
  Ok(std::format!("({left}{s}{right})"))
}

fn fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  operand: String,
  op: Precedenced<&'static str, Power>,
) -> Result<String, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  Ok(std::format!("({operand}{})", op.into_data()))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Harness
// ═══════════════════════════════════════════════════════════════════════════════

fn unit_ctx<'inp>() -> ParserContext<'inp, TestLexer<'inp>, Fatal<()>> {
  ParserContext::new(Fatal::new())
}

fn limited<'inp>(limit: usize) -> ParserContext<'inp, TestLexer<'inp>, Fatal<()>> {
  unit_ctx().with_recursion_limiter(RecursionLimiter::with_limitation(limit))
}

/// `- - - … 1`, one prefix operator — and therefore one pratt frame — per level.
fn prefix_chain(depth: usize) -> String {
  let mut src = String::with_capacity(depth * 2 + 1);
  for _ in 0..depth {
    src.push_str("- ");
  }
  src.push('1');
  src
}

/// Runs `f` on a worker with a large stack and a **hard wall-clock bound**.
///
/// The bound is the load-bearing half here. Once the sink has erased terminality a recoverer runs,
/// and a recoverer that retries without progress does not fail an assertion — it spins. The wall
/// has to sit outside the code under test for a non-terminating parse to be reported as a failure
/// rather than as a hung suite.
///
/// The fresh thread is doing a second job: every observation below is a thread-local, so one
/// worker per cell is what keeps the cells from reading each other's marks under the harness's
/// parallel runner.
fn bounded<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
  let (tx, rx) = std::sync::mpsc::channel();
  let handle = std::thread::Builder::new()
    .stack_size(64 * 1024 * 1024)
    .spawn(move || {
      let _ = tx.send(f());
    })
    .expect("spawn the bounded worker");
  match rx.recv_timeout(std::time::Duration::from_secs(secs)) {
    Ok(v) => {
      handle.join().expect("the bounded worker panicked");
      v
    }
    Err(e) => panic!("the parse did not terminate within {secs}s: {e:?}"),
  }
}

// ── Observation ────────────────────────────────────────────────────────────────
//
// Thread-local rather than captured, because a recoverer written as a capturing closure cannot be
// spelled with elided lifetimes here: `Recover`'s blanket impl unifies the `InputRef`'s five
// parameters across the whole `apply`, and a closure's inferred signature does not. Every cell
// runs on its own `bounded` worker, so the cells stay independent.

thread_local! {
  /// How many times the recoverer ran.
  static RECOVERIES: Cell<usize> = const { Cell::new(0) };
  /// The stack address at the top of the probe, before the pratt parser runs.
  static BASE: Cell<usize> = const { Cell::new(0) };
  /// The largest distance from `BASE` any pratt frame reached.
  static DEEPEST: Cell<usize> = const { Cell::new(0) };
  /// The depth cell and the stack distance, as the recoverer read them.
  static AT_RECOVERY: Cell<(usize, usize)> = const { Cell::new((usize::MAX, 0)) };
}

/// An approximate stack pointer for the calling frame.
///
/// `#[inline(never)]` so the frame this measures is a real one, and the local goes through
/// `black_box` so it cannot be optimized into a register with no address.
#[inline(never)]
fn stack_mark() -> usize {
  let local = 0u8;
  core::hint::black_box(&local) as *const u8 as usize
}

fn set_base() {
  BASE.with(|b| b.set(stack_mark()));
  DEEPEST.with(|d| d.set(0));
}

/// Records how far from the baseline this frame sits, keeping the maximum.
///
/// The stack grows down on every platform this suite runs on, but the difference is taken
/// unsigned so a target that grows up reports a distance rather than an underflow.
fn note_stack_mark() {
  let base = BASE.with(Cell::get);
  if base == 0 {
    return;
  }
  let depth = base.abs_diff(stack_mark());
  DEEPEST.with(|d| {
    if depth > d.get() {
      d.set(depth);
    }
  });
}

fn distance_from_base() -> usize {
  BASE.with(Cell::get).abs_diff(stack_mark())
}

/// This build's reason that a raw address distance is **not** a measure of frame liveness, if it
/// has one.
///
/// [`stack_mark`] reads the address of a local, and the cell below compares two of them. That
/// comparison means something only while the addresses are native stack offsets. Two
/// instrumentations break that, in different ways, and neither can be tuned around:
///
/// * **miri** interprets the program. Its addresses are virtual allocation addresses handed out by
///   the interpreter, not offsets into any real stack. `cfg!(miri)` is set for the interpreted
///   build and is the stable spelling.
/// * **A sanitizer** may relocate a frame's locals onto a heap-allocated *fake stack* — ASan's
///   `detect_stack_use_after_return` machinery — so the address of a local is not where the frame
///   physically sits. There is no stable `cfg` to test: `cfg(sanitize = "address")` requires
///   `feature(cfg_sanitize)`, which a stable-compiled integration test cannot carry. So the leg
///   announces itself instead — `ci/sanitizer.sh` exports `TOKORA_SANITIZER` for **every**
///   sanitizer it runs, and this reads it at run time rather than through `option_env!`, so a
///   cached build cannot carry a stale answer into an uninstrumented run or the reverse.
fn frames_are_relocated() -> Option<String> {
  if cfg!(miri) {
    return Some(String::from(
      "miri: the interpreter hands out virtual allocation addresses, so the distance between two \
       of them is not a native stack depth",
    ));
  }
  match std::env::var("TOKORA_SANITIZER") {
    Ok(san) if !san.is_empty() => Some(std::format!(
      "TOKORA_SANITIZER={san}: a sanitizer-instrumented build may relocate a frame's locals onto a \
       fake stack, so the address of a local is not where its frame sits"
    )),
    _ => None,
  }
}

/// Says — on the process's **real** stderr — that the address-distance witness did not run.
///
/// Not `eprintln!`: libtest captures a passing test's output, so the one build where the assertion
/// is skipped would be the one build where nobody can see that it was. A skip that leaves no trace
/// is a check that passes by not looking, which is the same defect class one level up from the one
/// this cell measures. Writing through [`std::io::stderr`] bypasses the per-test capture buffer,
/// so the line lands in the CI log of a green run.
fn announce_skipped_stack_witness(why: &str, recovery_frame: usize, deepest: usize) {
  use std::io::Write as _;

  let mut err = std::io::stderr().lock();
  let _ = writeln!(
    err,
    "\n\
     !!! SKIPPED ASSERTION — pratt_limit_unit_sink\n\
     !!!   every_level_and_every_frame_is_released_before_recovery_sees_the_trip\n\
     !!! The stack-address witness `recovery_frame * 8 < deepest` did NOT run.\n\
     !!! Why: {why}.\n\
     !!! Read anyway, for the log: recovery_frame={recovery_frame}, deepest={deepest}.\n\
     !!! The two depth-cell assertions DID run and are unaffected: this build still proves the\n\
     !!! library released every level, it just does not corroborate that out-of-band.\n"
  );
  let _ = err.flush();
}

/// The recoverer every cell below shares: it records what it observed and synthesizes a value.
fn spending_recovery<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<String, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  RECOVERIES.with(|c| c.set(c.get() + 1));
  AT_RECOVERY.with(|c| c.set((inp.recursion().depth(), distance_from_base())));
  Ok(String::from("<spent>"))
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1 — the sink erases the stop
// ═══════════════════════════════════════════════════════════════════════════════

/// **The finding, confirmed.** `Recover` re-raises a trip through a delegating error type and
/// **spends** the very same trip through `()`.
///
/// `pratt_limit.rs::a_recoverer_re_raises_a_trip_instead_of_spending_it` is the other half of this
/// pair: identical ladder, identical limit, identical input, and a recoverer whose body is a
/// `panic!` because it must never run. Here the body runs and its value is returned as the parse's
/// result, so a `()`-errored grammar turns an exhausted depth budget into an ordinary recoverable
/// failure. That is the documented `MaybeTerminal` opt-out reaching a resource guard.
#[test]
fn a_discarding_sink_lets_recovery_spend_a_trip() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(spending_recovery)
      .parse_input(inp)
  }

  let (out, ran) = bounded(60, || {
    let src = prefix_chain(32);
    let out = Parser::with_context(limited(8))
      .apply(probe)
      .parse_str(&src);
    (out, RECOVERIES.with(Cell::get))
  });

  assert_eq!(
    ran, 1,
    "the recoverer must have been reached exactly once — through `LimErr` it is never reached at \
     all, which is the contrast this cell exists to pin"
  );
  assert_eq!(
    out,
    Ok(String::from("<spent>")),
    "and its synthesized value is the parse's result: the trip did not survive the sink as a stop"
  );
}

/// The same erasure one layer down, and the shape where it **costs** something: `skip_then_retry`
/// consults the same gate, so through `()` it skips and retries rather than re-raising before any
/// skip — and the skipping is committed.
///
/// The outcome is `Err` either way, so a caller matching only on the discriminant sees no
/// difference. The difference is in the input: `pratt_limit.rs::
/// skip_then_retry_re_raises_a_trip_without_skipping` runs this exact source through `LimErr` and
/// asserts the surrounding grammar is handed offset **0** — the terminal law re-raised before any
/// skip, and `try_attempt` rolled the failed parse back. Through `()` the same source leaves the
/// grammar at offset **68**: the whole 32-deep chain and the sync token were consumed and are not
/// coming back.
///
/// That is the concrete price of the erasure. Skipping is how recovery buys progress over a
/// *malformed* construct, and a depth budget is not one — no amount of skipped input makes the
/// next descent shallower, so the cycles spend input for a result they cannot change.
#[test]
fn a_discarding_sink_lets_skip_then_retry_burn_input_on_a_trip() {
  type Probed = (Result<String, ()>, Option<usize>);

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Probed, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .skip_then_retry(
        |_: &common::TokenKind| tokora::input::Balance::<char>::Neutral,
        |tok: tokora::span::Spanned<&Token, &tokora::SimpleSpan>| matches!(tok.data(), Token::Semi),
      )
      .parse_input(inp);
    let front = inp.next()?.map(|t| tokora::span::Span::start(t.span_ref()));
    Ok((out, front))
  }

  // `prefix_chain(32)` is 32 × `"- "` then `1`, so it ends at offset 64 and the source is
  // `…1 ; 1` with the `;` at 66 and the trailing `1` at 68 — the same source the `LimErr` cell uses.
  const SEMI: usize = 66;
  const TAIL: usize = 68;

  let (out, front) = bounded(60, || {
    let src = std::format!("{} ; 1", prefix_chain(32));
    assert_eq!(
      (src.find(';'), src.rfind('1')),
      (Some(SEMI), Some(TAIL)),
      "the offsets below are read off this source"
    );
    Parser::with_context(limited(8))
      .apply(probe)
      .parse_str(&src)
      .unwrap()
  });

  assert_eq!(
    out,
    Err(()),
    "the retry re-trips and the loop runs out of sync points, so the parse still fails — the \
     erasure does not turn this input into a success"
  );
  assert_eq!(
    front,
    Some(TAIL),
    "but it is not the same failure: the cycles consumed the deep chain and then the `;` itself, \
     leaving the grammar at the trailing `1`. Through `LimErr` this is `Some(0)` — nothing skipped \
     at all — so the sink's cost here is committed input, not a changed verdict"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2 — what the sink does NOT erase: the stack is already back
// ═══════════════════════════════════════════════════════════════════════════════

/// The guard's safety purpose survives the sink. By the time the recoverer sees the converted
/// value, **every level is released and every frame has returned** — measured twice, once through
/// the library's depth cell and once through the machine stack the cell stands for.
///
/// This is why the erasure is a lost *stop* rather than a lost *bound*: a recoverer that spends the
/// trip and re-descends starts from the depth it started at the first time and meets the same
/// limit, on a stack that has already unwound.
#[test]
fn every_level_and_every_frame_is_released_before_recovery_sees_the_trip() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    set_base();
    let depth_before = inp.recursion().depth();
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(spending_recovery)
      .parse_input(inp)?;
    assert_eq!(
      out, "<spent>",
      "this cell requires the trip to have been spent"
    );
    Ok(depth_before)
  }

  let (depth_before, deepest, at_recovery) = bounded(60, || {
    // The limit is 200, not 8: a trip at depth 9 leaves a native footprint small enough that a
    // release build's could plausibly be confused with the recoverer's own frame. 200 frames of
    // pratt descent cannot be.
    let src = prefix_chain(400);
    let depth_before = Parser::with_context(limited(200))
      .apply(probe)
      .parse_str(&src)
      .unwrap();
    (
      depth_before,
      DEEPEST.with(Cell::get),
      AT_RECOVERY.with(Cell::get),
    )
  });
  let (depth_at_recovery, recovery_frame) = at_recovery;

  assert_eq!(
    (depth_before, depth_at_recovery),
    (0, 0),
    "`Descent::drop` released every level on the way out, so the recoverer reads the budget the \
     parse started with — spending the trip cannot hand a caller a permanently shallower budget"
  );

  // The depth cell is the library's accounting; this is the machine.
  assert!(
    deepest > 0,
    "the descent's stack marks must have been taken; got {deepest}"
  );
  // ── The machine half runs in NATIVE builds only ──────────────────────────────────────────
  //
  // Measured on this tree over the 200 frames below. In a native build the ×8 margin is nowhere
  // near the edge:
  //
  // | build                        | deepest   | per frame | recoverer | holds? |
  // |------------------------------|-----------|-----------|-----------|--------|
  // | debug                        | 1_029_120 | ~5.1 KiB  |       160 | yes    |
  // | release                      |    99_760 | ~500 B    |         0 | yes    |
  // | miri SB/TB, x86_64 linux     | 2_002_360 | —         | 2_148_819 | NO     |
  // | miri SB/TB, aarch64 darwin   | 2_540_555 | —         | 3_040_618 | NO     |
  // | ASan, x86_64 linux           |    51_648 | —         |    52_480 | NO     |
  // | ASan, aarch64 darwin         | 1_737_952 | —         |       384 | yes    |
  //
  // Release returns the recoverer to the baseline exactly, which is why the assertion permits 0.
  // The debug per-frame figure agrees with the 4–6 KiB `RecursionLimiter`'s own docs record, so
  // the two independent measurements of this engine's frame cost corroborate each other.
  //
  // The four instrumented rows are why this one assertion is gated rather than widened. Under an
  // instrumented build the addresses are not native stack offsets at all — see
  // [`frames_are_relocated`] — and three of the four put the **recoverer farther from the
  // baseline than the descent it has already unwound past**. The operands are inverted, so there
  // is no threshold that turns the comparison back into a measurement: the quantity simply is not
  // being measured. A margin of 8 and a margin of 8 000 are equally meaningless there.
  //
  // The last row is the one that matters most for the decision. ASan on aarch64-darwin *passes*,
  // and that is not a reason to keep the assertion on for sanitizers — it is the reason to turn
  // it off for all of them. The relationship is not a property of the code under test; it is a
  // property of the host's instrumented frame layout, and it flips between two hosts running the
  // same source. A comparison whose answer changes with the runner is reporting the runner.
  //
  // The two assertions above are deliberately NOT gated. They are the library's own accounting
  // and hold under every build, so miri and the sanitizers still prove the levels came back — what
  // they cannot supply is the second, out-of-band witness. Assertion 1 and this one are
  // independent on purpose and are not to be collapsed into one counter: reading the same cell
  // twice is not corroboration.
  match frames_are_relocated() {
    None => assert!(
      recovery_frame * 8 < deepest,
      "the recoverer must run on an already-unwound stack: it sits {recovery_frame} bytes from \
       the pre-parse baseline while the descent reached {deepest} — if the sink had let recovery \
       run with the deep frames still live these two would be comparable"
    ),
    Some(why) => announce_skipped_stack_witness(&why, recovery_frame, deepest),
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3 — a spent trip is a retried trip, and the retries terminate
// ═══════════════════════════════════════════════════════════════════════════════

/// `skip_then_retry` over an input whose **every** segment trips: each retry re-trips, so the loop
/// is driven entirely by its progress guard. It terminates, and it terminates by consuming.
///
/// Eight deep segments separated by `;`. Through a delegating error type this is one `Err` and zero
/// cycles; through `()` it is a trip per cycle, and the cell fails by hanging — inside the
/// `bounded` wall — if a cycle ever fails to consume its sync token.
#[test]
fn a_sink_erased_trip_retried_at_every_sync_point_still_terminates() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .skip_then_retry(
        |_: &common::TokenKind| tokora::input::Balance::<char>::Neutral,
        |tok: tokora::span::Spanned<&Token, &tokora::SimpleSpan>| matches!(tok.data(), Token::Semi),
      )
      .parse_input(inp)
  }

  let out = bounded(60, || {
    let segment = prefix_chain(24);
    let src = std::iter::repeat_n(segment.as_str(), 8)
      .collect::<std::vec::Vec<_>>()
      .join(" ; ");
    Parser::with_context(limited(8))
      .apply(probe)
      .parse_str(&src)
  });

  assert_eq!(
    out,
    Err(()),
    "every retry re-trips and the loop runs out of sync points rather than spinning: the last \
     trigger error is surfaced"
  );
}

/// The shape where a spent trip is most dangerous: a **repetition** whose element recovers.
///
/// `Recover` restores the input to the pre-element position before it runs the recoverer, so a
/// recoverer that returns `Ok` without consuming makes the repetition's element succeed while
/// consuming nothing — an unbounded loop unless the repetition guards on committed progress. The
/// budget trip is what makes the element fail every single time, so this is that loop's worst
/// input: the element can *never* succeed on its own, and the element below never declines either,
/// so nothing but the guard can end the repetition.
///
/// It terminates. `repeated`'s no-progress stall sees the zero-consumption element and ends the
/// repetition, which is the guard doing exactly the job the `many` family documents.
///
/// The element has to be written out as a `TryParseInput` closure rather than spelled
/// `.recover(..).repeated()`, because `repeated` is a `TryParseInput` method and `Recover` is not
/// one: a recoverer either produces a value or fails, and can never *decline*. That is a small
/// structural obstacle in front of this hazard — the loop cannot be built by combinator chaining
/// alone — but it is not a guarantee, which is why the cell builds it by hand.
#[test]
fn a_repetition_over_a_sink_erased_trip_is_bounded_by_the_no_progress_guard() {
  /// Always accepts: on a trip the recoverer synthesizes a value from the position `Recover`
  /// rolled back to, so the accept commits nothing.
  fn recovering_element<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<String>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(spending_recovery)
      .parse_input(inp)
      .map(ParseAttempt::Accept)
  }

  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<std::vec::Vec<String>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = ()> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    recovering_element.repeated().collect().parse_input(inp)
  }

  let (out, ran) = bounded(60, || {
    let src = prefix_chain(64);
    let out = Parser::with_context(limited(4))
      .apply(probe)
      .parse_str(&src);
    (out, RECOVERIES.with(Cell::get))
  });

  assert_eq!(
    out,
    Ok(std::vec![String::from("<spent>")]),
    "one zero-consumption element, then the no-progress stall — the repetition does not spin on an \
     element that fails identically forever"
  );
  assert_eq!(
    ran, 1,
    "and the recoverer ran once, not unboundedly: the guard cut the loop at the first element that \
     committed nothing"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4 — the other new conversion: the sink is inert for `NonAssociativeChain`
// ═══════════════════════════════════════════════════════════════════════════════

/// `NonAssociativeChain` is malformed input, so it is **already** non-terminal: the blanket `false`
/// is its real classification and the `From<..> for ()` impl takes nothing away.
///
/// The pairing is the point. `pratt_limit.rs::an_explicit_recovery_may_spend_the_repeat` runs this
/// same recovery through `LimErr`, which stores the payload and reports `is_terminal() == false`,
/// and gets the same recovered value. Two error types, one answer — so unlike the trip above, the
/// sink is not what decided it.
#[test]
fn a_discarding_sink_does_not_change_the_repeat_which_was_always_recoverable() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(spending_recovery)
      .parse_input(inp)
  }

  let (got, ran) = bounded(60, || {
    let got = Parser::with_context(unit_ctx())
      .apply(probe)
      .parse_str("1 ; 2 ; 3");
    (got, RECOVERIES.with(Cell::get))
  });

  assert_eq!(got, Ok(String::from("<spent>")));
  assert_eq!(
    ran, 1,
    "the recoverer runs, exactly as it does through a delegating error type that stores the repeat"
  );
}

/// And the repeat's own reporting is unchanged by the sink: a chain shallow enough not to trip is
/// still rejected at the repeat, so the two errors do not mask one another.
#[test]
fn the_repeat_is_still_reported_under_a_budget_that_does_not_trip() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
  }

  let out = bounded(60, || {
    Parser::with_context(limited(64))
      .apply(probe)
      .parse_str("1 ; 2 ; 3")
  });

  assert_eq!(
    out,
    Err(()),
    "the second `;` is refused; through `()` the value is gone but the `Err` channel is not"
  );
}
