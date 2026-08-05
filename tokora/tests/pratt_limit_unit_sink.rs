#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! What a **discarding error sink** does — and no longer does — to the two errors `pratt_limit.rs`
//! pins.
//!
//! `pratt_limit.rs` drives its ladder through `LimErr`, a grammar error that *stores* both payloads
//! and delegates `is_terminal`. This file drives the same shape through `()` — the sink every error
//! type under `tokora::error` carries a `From` into, `RecursionLimitReached` and
//! `NonAssociativeChain` included — and **measures** the difference rather than reading it off the
//! type signatures.
//!
//! The conversion is not new: fourteen of the sixteen `From<..> for ()` impls under
//! `tokora/src/error/` predate this ladder, `terminal.rs`'s own included. Two of the sixteen sit on
//! a **resource guard** rather than on a malformed-input report, and the answer for those two is
//! now the same as for a delegating error type:
//!
//! * [`NonAssociativeChain`](tokora::error::NonAssociativeChain) is non-terminal to begin with, so
//!   the sink changes nothing and never did. The cells below run the same recovery through both
//!   error types and get the same answer.
//! * [`RecursionLimitReached`](tokora::error::RecursionLimitReached) is terminal for every value,
//!   and **the terminality is not carried by the payload**. The trip counts on
//!   `Input::resource_trips`, a monotone cell on the input session, and `Recover`,
//!   `InplaceRecover` and `skip_then_retry` read that cell beside `MaybeTerminal::is_terminal` —
//!   each against a snapshot taken before its own attempt, so what re-raises is a trip *that
//!   attempt* caused. So the sink discards the *value* and cannot discard the *stop*.
//!
//! # This file is the regression witness for the erasure
//!
//! Every cell in section 1 asserted the opposite until issue #148: that `Recover` re-raised a trip
//! through a delegating error type and **spent** the very same trip through `()`, and that
//! `skip_then_retry` skipped and committed 68 bytes of input a delegating type never touched. They
//! were inverted rather than deleted, because a deleted test cannot notice the defect coming back.
//! Each names its counterpart in `pratt_limit.rs`, and the pair must keep agreeing: that agreement
//! *is* the invariance the fix delivers.
//!
//! # What the sink still does not cost
//!
//! The *stack safety* was never at risk and is still measured rather than argued. `Descent`'s
//! destructor releases its level on the unwind that carries the error out, so by the time anything
//! outside the engine sees the error, every frame the budget was protecting has already returned.
//! Both halves are read directly here: the depth cell through [`InputRef::recursion`], and the
//! **native** stack through the address of a local, which is the quantity the depth cell is only a
//! proxy for.
//!
//! # Termination defects do not fail; they hang
//!
//! A spent trip would be a retried trip, and a retry that makes no progress is an infinite loop
//! rather than a red test. Every cell here runs under [`bounded`], which puts a hard wall-clock
//! wall around the parse — the same reason `pratt_limit.rs`'s deep cells carry one. The wall stays
//! now that the trip re-raises: it is what would report a regression as a failure instead of as a
//! hung suite.

mod common;

use core::cell::Cell;
use tokora::EmitterView;

use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  TryParseInput,
  emitter::{Fatal, FullContainerEmitter, PrattEmitter},
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  state::recursion_tracker::RecursionLimiter,
  token::PrattToken,
  try_parse_input::ParseAttempt,
  utils::typenum::U4,
};

use common::{Power, TestLexer, Token};

// ═══════════════════════════════════════════════════════════════════════════════
// The ladder, over the discarding sink
// ═══════════════════════════════════════════════════════════════════════════════
//
// The shape `pratt_limit.rs` uses, with `()` where `LimErr` was. Only the operators the cells below
// need are kept: `-` prefix (one frame per level, the deep-recursion shape), `;` non-associative
// infix (the chain constraint, and a convenient sync point), and `=` right-associative infix (the
// second shape that descends one frame per operator).

const P_PREFIX: Power = Power(100);
const P_CHAIN: Power = Power(5);
const P_ASSIGN: Power = Power(1);

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
      Token::Eq => PrattRHS::Infix(Precedenced::new(PrattInfix::Right("="), P_ASSIGN)),
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

// ── The token engine over the same ladder ──────────────────────────────────────
//
// The issue's acceptance criteria ask for the two engines to behave identically under the sink,
// and identical behaviour is not inherited by reading the source: both engines take their level
// through `InputRef::descend`, which is one call site each, and a future engine could grow a
// second recursion site that forgets it. So the token engine is driven here too, against the same
// budget and the same input shape.

impl PrattToken<'_, i64, Power> for Token {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>> {
    match self {
      Token::Num(_) => Some(PrattLHS::Operand(())),
      Token::Minus => Some(PrattLHS::Prefix(Precedenced::new((), P_PREFIX))),
      _ => None,
    }
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>> {
    match self {
      Token::Semi => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        P_CHAIN,
      ))),
      Token::Eq => Some(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(()),
        P_ASSIGN,
      ))),
      _ => None,
    }
  }
}

type Tok = tokora::span::Spanned<Token, tokora::SimpleSpan>;

fn tok_fold_prefix<'inp, E>(
  _op: Tok,
  operand: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, ()> {
  Ok(operand)
}

fn tok_fold_postfix<'inp, E>(
  operand: Tok,
  _op: Tok,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, ()> {
  Ok(operand)
}

fn tok_fold_infix<'inp, E>(
  left: Tok,
  _right: Tok,
  _infix: tokora::span::Spanned<PrattInfix<Token, Token, Token>, tokora::SimpleSpan>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, E>,
) -> Result<Tok, ()> {
  Ok(left)
}

/// The token engine's frame prologue, wrapped as a `ParseInput` so it can carry the same
/// `.recover(..)` and `.skip_then_retry(..)` the typed cells use.
fn token_pratt<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()> + PrattEmitter<'inp, TestLexer<'inp>>,
{
  note_stack_mark();
  let out = inp.pratt::<_, _, _, i64, Power>(
    tok_fold_prefix::<Ctx::Emitter>,
    tok_fold_infix::<Ctx::Emitter>,
    tok_fold_postfix::<Ctx::Emitter>,
  )?;
  Ok(std::format!("{:?}", out.map(|t| t.into_data())))
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

/// `1 = 1 = … = 1`: right-associative, so the chain descends one frame per operator — the second
/// shape that reaches the budget, and it reaches it through the infix RHS rather than the prefix
/// operand.
fn right_chain(depth: usize) -> String {
  let mut src = String::with_capacity(depth * 4 + 1);
  src.push('1');
  for _ in 0..depth {
    src.push_str(" = 1");
  }
  src
}

/// The seconds [`bounded`] allows when the build is **interpreted**, replacing whatever native
/// figure the call site passed.
///
/// Every cell here calls `bounded(60, …)`, and 60 seconds is generous for a compiled build: this
/// file's slowest cell — the section 3 unwind cell, which descends 64 prefix levels under a 4,000
/// budget and unwinds a panic back out of them — runs in **9.8–10.4 ms** natively, and no other
/// cell reaches a millisecond. Under Miri that same cell is four orders of magnitude slower, and
/// 60 seconds stops being a generous bound and becomes a coin flip.
///
/// Measured directly with the exact command and flags `ci/miri_tb.sh` runs for the leg that went
/// red (`-Zmiri-strict-provenance -Zmiri-disable-isolation -Zmiri-symbolic-alignment-check
/// -Zmiri-tree-borrows`, `RUSTFLAGS=--cfg test_all_tests`), timing the interval `bounded` itself
/// bounds rather than the cargo invocation around it:
///
/// | run | aarch64-apple-darwin | slowdown vs native |
/// |---|---|---|
/// | 1, cell alone | 49.72s | ×4,800 |
/// | 2, cell alone | 48.61s | ×4,700 |
/// | 3, whole file | 49.32s | ×4,800 |
///
/// The whole file is 58.0s under Miri against 49.3s for this one cell, so the cell is essentially
/// the file and the other fifteen contribute under nine seconds between them — which is why CI
/// reported `15 passed; 1 failed` rather than a general slowness.
///
/// The borrow model is where the cost is, and that is measured too rather than asserted: the same
/// cell under the `-sb` leg's flags — `ci/miri_sb.sh`, identical but for dropping
/// `-Zmiri-tree-borrows` — is **6.94s**, a seventh of the tree-borrows figure. That is why only
/// the `-tb` leg went red, and why the single figure below has to be sized for the slower model
/// even though both legs read it.
///
/// **The margin, and why it is sized off CI rather than off the table.** CI's failing leg is
/// `x86_64-unknown-linux-gnu` on a shared runner: a different architecture, a slower core, and a
/// noisy neighbour. That gap is not a guess here, because the failure itself measures it — the
/// same cell that reads 49.7s above exceeded **60s** there, so the runner is at least ×1.21
/// slower on this exact workload, and the true figure is only bounded from below because a
/// tripped deadline reports "over the budget", never by how much. A shared x86 runner at ×3 of
/// this machine, on a bad draw, would put the cell near 150s.
///
/// 500 is ×10 over the slowest reading — the same multiple, from the same reasoning, that
/// `pratt_limit.rs`'s `DEEP_STACK_BUDGET` chose for the other bound — which leaves ×3.3 over that
/// pessimistic 150s, and ×8 over the one figure CI did establish, that the cell needs more than
/// 60 seconds there.
///
/// It is deliberately not larger. A budget is also a bill the job pays if a termination
/// regression ever does land, because every cell that hangs spends its own wall before the next
/// one starts: sixteen cells at 500s is 2h13, inside the six-hour job timeout, and that is the
/// unrealistic case where *every* cell hangs rather than the handful a real regression reaches.
/// Ten minutes per cell is still a bound; an hour per cell would be a way of never finding out.
const INTERPRETED_TIMEOUT_SECS: u64 = 500;

/// Runs `f` on a worker with a large stack and a **hard wall-clock bound**.
///
/// The bound is the load-bearing half here. Were the sink ever to erase terminality again a
/// recoverer would run, and a recoverer that retries without progress does not fail an assertion —
/// it spins. The wall has to sit outside the code under test for a non-terminating parse to be
/// reported as a failure rather than as a hung suite.
///
/// The fresh thread is doing a second job: every observation below is a thread-local, so one
/// worker per cell is what keeps the cells from reading each other's marks under the harness's
/// parallel runner.
///
/// Both jobs, and the choice between the two budgets, belong to [`common::bounded_wait`] — the
/// tree's single bounded wait. This wrapper only supplies this file's stack size and its pair of
/// allowances. `secs` is the native one, so the thirty-one call sites keep meaning exactly what
/// they say on a compiled build; [`INTERPRETED_TIMEOUT_SECS`] replaces it under Miri.
fn bounded<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
  common::bounded_wait(
    64 * 1024 * 1024,
    common::WallClock {
      native_secs: secs,
      interpreted_secs: INTERPRETED_TIMEOUT_SECS,
    },
    f,
  )
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
  /// How many times [`catch_a_trip`] actually tripped the budget and swallowed it.
  ///
  /// The non-vacuity control for the attempt-relative cells in section 5: they assert that a caught
  /// trip leaves the recovery combinators behaving as they do in an untripped parse, and a fixture
  /// that quietly stopped tripping would satisfy that by satisfying nothing.
  static CAUGHT: Cell<usize> = const { Cell::new(0) };
  /// The stack address at the top of the probe, before the pratt parser runs.
  static BASE: Cell<usize> = const { Cell::new(0) };
  /// The largest distance from `BASE` any pratt frame reached.
  static DEEPEST: Cell<usize> = const { Cell::new(0) };
  /// The depth cell and the stack distance, as the grammar read them where the trip surfaced.
  static AT_SURFACE: Cell<(usize, usize)> = const { Cell::new((usize::MAX, 0)) };
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
fn announce_skipped_stack_witness(why: &str, surface_frame: usize, deepest: usize) {
  use std::io::Write as _;

  let mut err = std::io::stderr().lock();
  let _ = writeln!(
    err,
    "\n\
     !!! SKIPPED ASSERTION — pratt_limit_unit_sink\n\
     !!!   every_level_and_every_frame_is_released_before_the_trip_surfaces\n\
     !!! The stack-address witness `surface_frame * 8 < deepest` did NOT run.\n\
     !!! Why: {why}.\n\
     !!! Read anyway, for the log: surface_frame={surface_frame}, deepest={deepest}.\n\
     !!! The two depth-cell assertions DID run and are unaffected: this build still proves the\n\
     !!! library released every level, it just does not corroborate that out-of-band.\n"
  );
  let _ = err.flush();
}

/// A budget far above any depth the fixtures below reach — the **non-vacuity control**.
///
/// Every trip cell asserts `Err(())`, and `()` carries no discriminant, so an unrelated failure —
/// a fixture whose source stopped reaching the ladder, a classifier arm that stopped matching, a
/// harness that stopped driving the engine at all — reads exactly like a re-raised trip. Each cell
/// therefore runs its own probe a second time with **only the budget changed** and requires the
/// parse to succeed. That is what makes `Err(())` attributable to the budget rather than merely
/// observed alongside it.
const ROOMY: usize = 4_000;

/// The recoverer every cell below shares: it records that it ran and synthesizes a value.
///
/// It is registered on the trip cells precisely so it can be asserted **not** to have run —
/// `RECOVERIES` at zero is a stronger statement than a `panic!` body, because it also proves the
/// counter was wired up (the `NonAssociativeChain` cells drive the same function to 1).
fn counting_recovery<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  _err: (),
) -> Result<String, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  RECOVERIES.with(|c| c.set(c.get() + 1));
  AT_SURFACE.with(|c| c.set((inp.recursion().depth(), distance_from_base())));
  Ok(String::from("<spent>"))
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1 — the sink does NOT erase the stop
// ═══════════════════════════════════════════════════════════════════════════════

/// **The #148 R1 regression witness.** `Recover` re-raises a trip through a delegating error type
/// and re-raises the very same trip through `()`.
///
/// Until #148 this cell asserted the opposite — that the recoverer ran once and its `"<spent>"`
/// became the parse's result — and it was correct about the code it measured. Terminality was
/// carried only by the error payload, so a `From` that discarded the payload discarded the stop
/// with it. It is stored on the input session now, so it cannot be discarded by an error type at
/// all.
///
/// `pratt_limit.rs::a_recoverer_re_raises_a_trip_instead_of_spending_it` is the other half of this
/// pair: identical ladder, identical limit, identical input, and a recoverer whose body is a
/// `panic!` because it must never run. Here the recoverer is a counter, and it must read zero. The
/// two cells now assert the same behaviour through two different error types, which is the whole
/// content of the fix.
#[test]
fn a_discarding_sink_cannot_let_recovery_spend_a_trip() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(counting_recovery)
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
    ran, 0,
    "the recoverer must never be reached — through `LimErr` it is never reached either, and that \
     agreement is the invariance this cell exists to pin"
  );
  assert_eq!(
    out,
    Err(()),
    "the trip survives the sink as a stop: the parse fails rather than returning a synthesized node"
  );

  // Non-vacuity: same probe, same source, only the budget changed.
  let roomy = bounded(60, || {
    let src = prefix_chain(32);
    Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str(&src)
  });
  assert!(
    roomy.is_ok(),
    "the budget is what failed the run above — with room this input parses; got {roomy:?}"
  );
}

/// The same invariance one layer down, and the shape where the erasure used to **cost** something:
/// `skip_then_retry` consults the same gate, so through `()` it now re-raises before any skip
/// rather than skipping and committing.
///
/// The outcome is `Err` either way, so a caller matching only on the discriminant sees no
/// difference — which is exactly why the cell measures the **input** instead. `pratt_limit.rs::
/// skip_then_retry_re_raises_a_trip_without_skipping` runs this exact source through `LimErr` and
/// asserts the surrounding grammar is handed offset **0**: the terminal law re-raised before any
/// skip, and `try_attempt` rolled the failed parse back. Through `()` it used to be offset **68** —
/// the whole 32-deep chain and the sync token consumed and not coming back. The two are the same
/// number now.
///
/// That number was the concrete price of the erasure. Skipping is how recovery buys progress over a
/// *malformed* construct, and a depth budget is not one — no amount of skipped input makes the next
/// descent shallower, so the cycles spent input for a result they could not change.
#[test]
fn a_discarding_sink_cannot_let_skip_then_retry_burn_input_on_a_trip() {
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
  // `…1 ; 1` with the `;` at 66 and the trailing `1` at 68 — the same source the `LimErr` cell
  // uses. The offsets are still read off the source so the cell keeps failing loudly if the
  // fixture changes shape, even though the assertion below no longer names 68.
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
    "the trip is re-raised before any skip, so the parse fails — as it does through `LimErr`"
  );
  assert_eq!(
    front,
    Some(0),
    "and nothing was skipped: `try_attempt` rolled the failed parse back to the start. This read \
     `Some({TAIL})` before #148 — the deep chain and the `;` consumed and committed — while \
     `LimErr` read `Some(0)`; the sink no longer decides it"
  );

  // Non-vacuity: same probe, same source, only the budget changed.
  let roomy = bounded(60, || {
    let src = std::format!("{} ; 1", prefix_chain(32));
    Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str(&src)
      .unwrap()
      .0
  });
  assert!(
    roomy.is_ok(),
    "the budget is what failed the run above — with room this input parses; got {roomy:?}"
  );
}

/// The third combinator on the same gate: [`inplace_recover`](tokora::ParseInput::inplace_recover)
/// re-raises a trip too.
///
/// It is here because it is the one of the three that does **not** backtrack — it hands the
/// recoverer the error position rather than a restored checkpoint — so "the recoverer never runs"
/// has to be established for it separately rather than inherited from `Recover`'s cell. There is no
/// `pratt_limit.rs` counterpart to agree with; the agreement asserted here is with the sibling
/// combinator directly above.
#[test]
fn a_discarding_sink_cannot_let_inplace_recovery_spend_a_trip() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .inplace_recover(
        |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
         _cursor: tokora::input::Cursor<'inp, '_, TestLexer<'inp>>,
         _err: ()| { counting_recovery(inp, ()) },
      )
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
    ran, 0,
    "the in-place recoverer must never be reached either"
  );
  assert_eq!(out, Err(()), "the trip is re-raised");

  // Non-vacuity: same probe, same source, only the budget changed.
  let roomy = bounded(60, || {
    let src = prefix_chain(32);
    Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str(&src)
  });
  assert!(
    roomy.is_ok(),
    "the budget is what failed the run above — with room this input parses; got {roomy:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2 — the two engines, the two chain shapes, and the two cache states
// ═══════════════════════════════════════════════════════════════════════════════

/// The **token** engine, same sink, same answer as the typed engine one section up.
///
/// Both engines take their level at the frame prologue through `InputRef::descend`, so they inherit
/// the latch from the same place — but "inherit it from the same place" is a claim about today's
/// source. This runs it.
#[test]
fn the_token_engine_re_raises_a_trip_through_the_sink_too() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()> + PrattEmitter<'inp, TestLexer<'inp>>,
  {
    token_pratt.recover(counting_recovery).parse_input(inp)
  }

  let (out, ran) = bounded(60, || {
    let src = prefix_chain(32);
    let out = Parser::with_context(limited(8))
      .apply(probe)
      .parse_str(&src);
    (out, RECOVERIES.with(Cell::get))
  });

  assert_eq!(ran, 0, "the token engine's trip is terminal too");
  assert_eq!(out, Err(()));

  // Non-vacuity: same probe, same source, only the budget changed.
  let roomy = bounded(60, || {
    let src = prefix_chain(32);
    Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str(&src)
  });
  assert!(
    roomy.is_ok(),
    "the budget is what failed the run above — with room this input parses; got {roomy:?}"
  );
}

/// A **right-associative** chain reaches the budget through the infix RHS rather than through the
/// prefix operand, and it is re-raised identically. Both engines, one cell, so a divergence between
/// them on this shape cannot hide in a cell that only runs one of them.
///
/// The limit is deliberately small (4): the criterion is "a small limit", because a small limit
/// trips inside the first handful of frames, where a bug in the prologue ordering is not masked by
/// dozens of frames that already worked.
#[test]
fn a_right_associative_chain_re_raises_its_trip_in_both_engines() {
  fn typed<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(counting_recovery)
      .parse_input(inp)
  }

  fn token<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()> + PrattEmitter<'inp, TestLexer<'inp>>,
  {
    token_pratt.recover(counting_recovery).parse_input(inp)
  }

  let (typed_out, typed_ran) = bounded(60, || {
    let src = right_chain(32);
    let out = Parser::with_context(limited(4))
      .apply(typed)
      .parse_str(&src);
    (out, RECOVERIES.with(Cell::get))
  });
  let (token_out, token_ran) = bounded(60, || {
    let src = right_chain(32);
    let out = Parser::with_context(limited(4))
      .apply(token)
      .parse_str(&src);
    (out, RECOVERIES.with(Cell::get))
  });

  assert_eq!(
    (typed_out, typed_ran),
    (Err(()), 0),
    "typed engine: the right-associative descent trips and the trip is re-raised"
  );
  assert_eq!(
    (token_out, token_ran),
    (Err(()), 0),
    "token engine: identically"
  );

  // Non-vacuity, both engines: same probes, same source, only the budget changed.
  let roomy_typed = bounded(60, || {
    let src = right_chain(32);
    Parser::with_context(limited(ROOMY))
      .apply(typed)
      .parse_str(&src)
  });
  let roomy_token = bounded(60, || {
    let src = right_chain(32);
    Parser::with_context(limited(ROOMY))
      .apply(token)
      .parse_str(&src)
  });
  assert!(
    roomy_typed.is_ok() && roomy_token.is_ok(),
    "the budget is what failed the runs above — with room this chain parses in both engines; got \
     typed={roomy_typed:?} token={roomy_token:?}"
  );
}

/// A **prefilled** lookahead window does not change the answer, in either engine.
///
/// The cache is the one piece of state that can put the lex offset ahead of the committed cursor,
/// and the trip's own offset is defined as committed consumption precisely so it cannot move with
/// it (`pratt_limit.rs::the_trip_is_identical_with_an_empty_and_a_prefilled_cache` pins that half).
/// What this cell pins is the *terminality* half: a trip reached with tokens already buffered is
/// still latched, and recovery still re-raises it.
#[test]
fn a_prefilled_cache_does_not_let_recovery_spend_a_trip() {
  fn typed_prefilled<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Fill the cache as far as it will go before a single token is consumed.
    let _ = inp.peek::<U4>()?;
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(counting_recovery)
      .parse_input(inp)
  }

  fn token_prefilled<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()> + PrattEmitter<'inp, TestLexer<'inp>>,
  {
    let _ = inp.peek::<U4>()?;
    token_pratt.recover(counting_recovery).parse_input(inp)
  }

  let typed = bounded(60, || {
    let src = prefix_chain(32);
    let out = Parser::with_context(limited(8))
      .apply(typed_prefilled)
      .parse_str(&src);
    (out, RECOVERIES.with(Cell::get))
  });
  let token = bounded(60, || {
    let src = prefix_chain(32);
    let out = Parser::with_context(limited(8))
      .apply(token_prefilled)
      .parse_str(&src);
    (out, RECOVERIES.with(Cell::get))
  });

  assert_eq!(
    typed,
    (Err(()), 0),
    "typed engine, prefilled window: the trip is still re-raised and the recoverer still never runs"
  );
  assert_eq!(token, (Err(()), 0), "token engine, prefilled window");

  // Non-vacuity, both engines: same probes, same source, only the budget changed. This one also
  // proves the prefill itself did not break the fixture — a `peek` that failed would give the same
  // `Err(())` as a trip.
  let roomy_typed = bounded(60, || {
    let src = prefix_chain(32);
    Parser::with_context(limited(ROOMY))
      .apply(typed_prefilled)
      .parse_str(&src)
  });
  let roomy_token = bounded(60, || {
    let src = prefix_chain(32);
    Parser::with_context(limited(ROOMY))
      .apply(token_prefilled)
      .parse_str(&src)
  });
  assert!(
    roomy_typed.is_ok() && roomy_token.is_ok(),
    "the budget is what failed the runs above, not the prefill; got typed={roomy_typed:?} \
     token={roomy_token:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3 — what the trip costs, and what it does not: the stack is already back
// ═══════════════════════════════════════════════════════════════════════════════

/// By the time anything outside the engine sees the error, **every level is released and every
/// frame has returned** — measured twice, once through the library's depth cell and once through
/// the machine stack the cell stands for.
///
/// This is the "recovery is still terminal after every Pratt frame and guard has unwound"
/// criterion, and the point of measuring it is that the two facts are independent: the re-raise
/// decision could in principle be taken while frames were still live (it is not; `Recover` sees the
/// error only after the engine has returned it), and the depth cell could in principle disagree
/// with the machine.
///
/// The observation moved out of the recoverer when the trip stopped reaching one. It is taken in
/// the grammar's own error arm instead, which sits **shallower** than the recoverer would have —
/// `Recover::parse_input` calls the recoverer one frame below where it returns the error to this
/// probe — so the margin the assertion checks is if anything conservative in the wrong direction,
/// and it still holds by three orders of magnitude.
#[test]
fn every_level_and_every_frame_is_released_before_the_trip_surfaces() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    set_base();
    let depth_before = inp.recursion().depth();
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(counting_recovery)
      .parse_input(inp);
    assert_eq!(
      out,
      Err(()),
      "this cell requires the trip to have been re-raised"
    );
    AT_SURFACE.with(|c| c.set((inp.recursion().depth(), distance_from_base())));
    Ok(depth_before)
  }

  let (depth_before, deepest, at_surface, ran) = bounded(60, || {
    // The limit is 200, not 8: a trip at depth 9 leaves a native footprint small enough that a
    // release build's could plausibly be confused with this frame's own. 200 frames of pratt
    // descent cannot be.
    let src = prefix_chain(400);
    let depth_before = Parser::with_context(limited(200))
      .apply(probe)
      .parse_str(&src)
      .unwrap();
    (
      depth_before,
      DEEPEST.with(Cell::get),
      AT_SURFACE.with(Cell::get),
      RECOVERIES.with(Cell::get),
    )
  });
  let (depth_at_surface, surface_frame) = at_surface;

  assert_eq!(ran, 0, "the recoverer must not have run");
  assert_eq!(
    (depth_before, depth_at_surface),
    (0, 0),
    "`Descent::drop` released every level on the way out, so the grammar reads the budget the parse \
     started with — a re-raised trip cannot hand a caller a permanently shallower budget"
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
  // | build                        | deepest   | per frame | surface   | holds? |
  // |------------------------------|-----------|-----------|-----------|--------|
  // | debug, aarch64 darwin        | 1_035_520 | ~5.2 KiB  |        48 | yes    |
  // | release, aarch64 darwin      |    99_728 | ~500 B    |         0 | yes    |
  // | miri SB/TB, x86_64 linux     | 2_002_360 | —         | 2_148_819 | NO     |
  // | miri SB/TB, aarch64 darwin   | 2_540_555 | —         | 3_040_618 | NO     |
  // | ASan, x86_64 linux           |    51_648 | —         |    52_480 | NO     |
  // | ASan, aarch64 darwin         | 1_737_952 | —         |       384 | yes    |
  //
  // The two native rows are re-measured for this frame (read out by forcing the skip path with
  // `TOKORA_SANITIZER=measure`, which prints both numbers). The four instrumented rows are the
  // pre-#148 readings taken in the recoverer, one frame deeper; the move only makes the observed
  // frame shallower, so they are quoted as the reason the assertion is gated rather than as
  // current readings. The assertion permits 0 because a release build returns to the baseline
  // exactly.
  //
  // Under an instrumented build the addresses are not native stack offsets at all — see
  // [`frames_are_relocated`] — and three of the four put the **observer farther from the baseline
  // than the descent it has already unwound past**. The operands are inverted, so there is no
  // threshold that turns the comparison back into a measurement: the quantity simply is not being
  // measured. A margin of 8 and a margin of 8 000 are equally meaningless there.
  //
  // The last row is the one that matters most for the decision. ASan on aarch64-darwin *passes*,
  // and that is not a reason to keep the assertion on for sanitizers — it is the reason to turn it
  // off for all of them. The relationship is not a property of the code under test; it is a
  // property of the host's instrumented frame layout, and it flips between two hosts running the
  // same source. A comparison whose answer changes with the runner is reporting the runner.
  //
  // The two assertions above are deliberately NOT gated. They are the library's own accounting and
  // hold under every build, so miri and the sanitizers still prove the levels came back — what they
  // cannot supply is the second, out-of-band witness. Assertion 1 and this one are independent on
  // purpose and are not to be collapsed into one counter: reading the same cell twice is not
  // corroboration.
  match frames_are_relocated() {
    None => assert!(
      surface_frame * 8 < deepest,
      "the trip must surface on an already-unwound stack: the grammar's error arm sits \
       {surface_frame} bytes from the pre-parse baseline while the descent reached {deepest} — if \
       the engine had raised the error with its deep frames still live these two would be \
       comparable"
    ),
    Some(why) => announce_skipped_stack_witness(&why, surface_frame, deepest),
  }
}

/// **Panic/unwind returns the recursion counter to its exact entry value** — and *entry* is
/// deliberately not zero here.
///
/// The nesting is what makes the cell say something a top-level run cannot: a counter that is reset
/// rather than balanced would also read 0 at the top, so this enters three levels by hand first and
/// requires the cell to come back to **3**. The `Descent` destructor is the only witness that gets
/// that right on an unwind, which is why the guard exists in the first place.
///
/// The second half is that a **panic is not a trip**. The unwind carries no
/// `RecursionLimitReached`, so nothing latches, and the proof is behavioural rather than a peek at
/// a private cell: an ordinary recoverable failure afterwards still reaches the recoverer. Without
/// that half the cell could pass with a latch that armed on any unwind, which would quietly disable
/// recovery for the rest of any parse a `catch_unwind` grammar caught.
#[test]
fn a_panicking_frame_returns_the_counter_to_its_entry_value_and_latches_nothing() {
  /// Panics five levels below whatever depth the pratt parse started at.
  fn panicking_lhs<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<PrattLHS<String, &'static str, Power>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    assert!(
      inp.recursion().depth() < 8,
      "deliberate panic from inside a live pratt frame"
    );
    lhs(inp)
  }

  /// Fails without consuming, and without tripping anything: the control failure.
  fn always_fails<'inp, Ctx>(
    _inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Err(())
  }

  fn probe<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, usize, bool), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.descending(|inp| {
      inp.descending(|inp| {
        inp.descending(|inp| {
          let entry = inp.recursion().depth();
          let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pratt(panicking_lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
          }));
          let after = inp.recursion().depth();
          // The control: an ordinary failure, recovered, AFTER the unwind. It reaches the
          // recoverer iff nothing was latched by the panic.
          let recovered = always_fails.recover(counting_recovery).parse_input(inp);
          Ok((
            entry,
            after,
            caught.is_err() && recovered == Ok(String::from("<spent>")),
          ))
        })
      })
    })
  }

  let (entry, after, unwound_and_recovered, ran) = bounded(60, || {
    let src = prefix_chain(64);
    // A budget far above the depth the panic fires at, so the unwind is a panic and never a trip.
    let (entry, after, ok) = Parser::with_context(limited(4_000))
      .apply(probe)
      .parse_str(&src)
      .unwrap();
    (entry, after, ok, RECOVERIES.with(Cell::get))
  });

  assert_eq!(entry, 3, "the three hand-entered levels are live at entry");
  assert_eq!(
    after, entry,
    "the unwind released every level the pratt frames raised and not one more: the counter is back \
     at its EXACT entry value, not merely at zero"
  );
  assert!(
    unwound_and_recovered,
    "the panic must have unwound to the catch, and the ordinary failure after it must still have \
     been recovered — a panic is not a resource trip and latches nothing"
  );
  assert_eq!(ran, 1, "the control recovery ran exactly once");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4 — the retry and repetition shapes, which used to be the dangerous ones
// ═══════════════════════════════════════════════════════════════════════════════

/// `skip_then_retry` over an input whose **every** segment trips: it re-raises at the first one and
/// never enters a cycle.
///
/// Eight deep segments separated by `;`. Through a delegating error type this is one `Err` and zero
/// cycles, and it is now one `Err` and zero cycles through `()` as well. The cell is kept from the
/// erasure era because its failure mode is the one that does not show up as a wrong value: were the
/// stop ever erased again the loop would drive a trip per cycle, and a cycle that failed to consume
/// its sync token would hang rather than fail — inside the `bounded` wall, which is why the wall
/// stays.
#[test]
fn a_trip_at_every_sync_point_re_raises_at_the_first_one() {
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
    "the first trip ends it: no cycle runs, and the parse fails"
  );
}

/// The shape where a spent trip was most dangerous: a **repetition** whose element recovers.
///
/// `Recover` restores the input to the pre-element position before it runs the recoverer, so a
/// recoverer that returns `Ok` without consuming makes the repetition's element succeed while
/// consuming nothing. The budget trip is what makes the element fail every single time, so this
/// used to be that loop's worst input: the element could never succeed on its own and never
/// declined either, and only `repeated`'s no-progress stall ended it — with one synthesized element
/// in the output, fabricated from a construct the parse was forbidden to read.
///
/// Now the element's trip re-raises through the recoverer and out of the repetition, so the
/// repetition fails instead of yielding `["<spent>"]`. The no-progress guard is still there and
/// still correct; it simply is no longer the only thing standing between a resource trip and a
/// synthesized tree.
///
/// The element has to be written out as a `TryParseInput` closure rather than spelled
/// `.recover(..).repeated()`, because `repeated` is a `TryParseInput` method and `Recover` is not
/// one: a recoverer either produces a value or fails, and can never *decline*.
#[test]
fn a_repetition_over_a_trip_cannot_synthesize_an_element() {
  fn recovering_element<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<String>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(counting_recovery)
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
    Err(()),
    "the element's trip leaves the repetition on the `Err` channel; before #148 this was \
     `Ok([\"<spent>\"])` — a node synthesized for a construct the budget forbade reading"
  );
  assert_eq!(
    ran, 0,
    "and the recoverer never ran: the stop is taken before recovery, not by a progress guard after \
     it"
  );

  // Non-vacuity: same probe, same source, only the budget changed.
  let roomy = bounded(60, || {
    let src = prefix_chain(64);
    Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str(&src)
  });
  assert!(
    roomy.is_ok(),
    "the budget is what failed the run above — with room the element parses and the repetition \
     collects it; got {roomy:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5 — the other new conversion: the sink is inert for `NonAssociativeChain`
// ═══════════════════════════════════════════════════════════════════════════════

/// `NonAssociativeChain` is malformed input, so it is **already** non-terminal: the blanket `false`
/// is its real classification, the `From<..> for ()` impl takes nothing away, and the input-side
/// latch never arms because nothing tripped.
///
/// The pairing is the point, and it is the control for the whole file. `pratt_limit.rs::
/// an_explicit_recovery_may_spend_the_repeat` runs this same recovery through `LimErr`, which
/// stores the payload and reports `is_terminal() == false`, and gets the same recovered value. Two
/// error types, one answer — and, now, one answer for the trip as well, but a *different* one. If
/// the latch were coarser than "a resource budget was exceeded" this cell would be the one that
/// went red.
#[test]
fn a_discarding_sink_does_not_change_the_repeat_which_was_always_recoverable() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(counting_recovery)
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

// ═══════════════════════════════════════════════════════════════════════════════
// 6 — the stop belongs to the attempt that caused it, not to the session
// ═══════════════════════════════════════════════════════════════════════════════
//
// Everything above measures what happens to the trip *itself*. This section measures what happens
// to everything **after** it, once grammar code has caught a trip and gone on parsing — which is
// the other of the two shapes the session counter's documentation names, and the one that decides
// whether an editor still gets diagnostics for the rest of the file.
//
// The counter these combinators read is a monotone session fact and is never cleared, so an
// absolute reading answers "has this parse ever tripped" — true forever after the first trip. Each
// site therefore snapshots the counter before the attempt it is judging and re-raises only when it
// moved *during* that attempt. Section 1's cells pin that a real trip is still re-raised; these pin
// that an unrelated failure afterwards is not charged with it.
//
// Each cell is the same probe over the same source, run twice with only the budget changed, and
// requires the two runs to agree — a caught trip must change nothing. `CAUGHT` is what keeps that
// from being an agreement between two parses that both did nothing.

/// `left + 1` nested frames on the parse's shared depth budget, counting the trip on the way out.
fn caught_ladder<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  left: usize,
) -> Result<(), ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  let mut frame = inp
    .descend()
    .inspect_err(|_| CAUGHT.with(|c| c.set(c.get() + 1)))?;
  let inp = &mut *frame;
  match left {
    0 => Ok(()),
    n => caught_ladder(inp, n - 1),
  }
}

/// **Grammar code that catches a trip itself and keeps parsing.** Consumes no input, so everything
/// after it sees exactly the source an untripped parse would.
fn catch_a_trip<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>)
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  let _ = caught_ladder(inp, 32);
}

/// `Recover` still recovers an **ordinary** failure that happens after a caught trip.
///
/// The regression witness for the attempt-relative comparison on `Recover`'s arm: read absolutely,
/// the session counter refuses this recovery — and every recovery after it, for the rest of the
/// parse — because something unrelated tripped earlier. The source here (`"; 1"`) cannot trip
/// anything: `lhs` refuses `;` on the first token, one frame deep, which is the boring malformed
/// input recovery exists for.
#[test]
fn a_caught_trip_does_not_disable_a_later_recovery() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    catch_a_trip(inp);
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .recover(counting_recovery)
      .parse_input(inp)
  }

  let (tight, ran, caught) = bounded(60, || {
    let out = Parser::with_context(limited(8))
      .apply(probe)
      .parse_str("; 1");
    (out, RECOVERIES.with(Cell::get), CAUGHT.with(Cell::get))
  });
  assert_eq!(
    caught, 1,
    "the tight run must really have tripped and swallowed it — otherwise this cell compares two \
     untripped parses"
  );

  let (roomy, roomy_ran, roomy_caught) = bounded(60, || {
    let out = Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str("; 1");
    (out, RECOVERIES.with(Cell::get), CAUGHT.with(Cell::get))
  });
  assert_eq!(
    roomy_caught, 0,
    "the control differs in one thing only — with room, nothing trips"
  );
  assert_eq!(
    (&roomy, roomy_ran),
    (&Ok(String::from("<spent>")), 1),
    "the control pins the fixture absolutely: the primary fails ordinarily and the recoverer runs"
  );

  assert_eq!(
    (tight, ran),
    (roomy, roomy_ran),
    "a trip the grammar caught and parsed past must not refuse the recoveries after it. The \
     counter is monotone and stays raised for the rest of the parse; what the arm tests is whether \
     it moved during THIS attempt"
  );
}

/// **The granularity floor, pinned rather than closed.** A trip caught *inside* the attempt is
/// charged to that attempt's ordinary failure, and the recovery does not run.
///
/// The cell directly above is the same probe with one thing moved: there `catch_a_trip` runs
/// **before** `Recover`'s attempt begins, so the baseline already includes the trip and the later
/// ordinary failure recovers. Here it runs **inside** the primary, between the attempt's baseline
/// and the failure being judged, so the counter moved during the attempt and the ordinary failure
/// is re-raised. Same source, same recoverer, same one trip — only where the catch sits.
///
/// This is a real residual and it is documented as one, on `InputRef::tripped_during_attempt`, on
/// `Input::resource_trips` and in `parser::many`'s module docs. **The strong form is not
/// implementable**: proving that the `Err` in hand *is* the trip means interrogating the error
/// value, and this file's whole subject is a sink that discards it — `()` keeps no discriminant to
/// interrogate. So the witness resolves to one *attempt*, and inside that unit it fails closed: an
/// ordinary failure sharing an attempt with a caught trip is re-raised, never the reverse.
///
/// The cell asserts what the code does today. If the escape hatch — an explicit rebaseline by
/// grammar code that deliberately catches a trip — is ever built, this is the cell that has to be
/// re-blessed, deliberately, rather than a behaviour that drifts unobserved.
#[test]
fn a_caught_trip_inside_one_attempt_re_raises_that_attempts_ordinary_failure() {
  /// The primary: catches a trip itself, carries on, and then fails **ordinarily** — the whole of
  /// it inside the one `Recover` attempt. `catch_a_trip` consumes nothing, so the pratt parse
  /// below sees exactly the source it would in an untripped run.
  fn caught_then_ordinary<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    catch_a_trip(inp);
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
  }

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    caught_then_ordinary
      .recover(counting_recovery)
      .parse_input(inp)
  }

  let (tight, ran, caught) = bounded(60, || {
    let out = Parser::with_context(limited(8))
      .apply(probe)
      .parse_str("; 1");
    (out, RECOVERIES.with(Cell::get), CAUGHT.with(Cell::get))
  });
  assert_eq!(
    caught, 1,
    "the tight run must really have tripped and swallowed it inside the attempt — otherwise this \
     cell measures nothing"
  );

  let (roomy, roomy_ran, roomy_caught) = bounded(60, || {
    let out = Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str("; 1");
    (out, RECOVERIES.with(Cell::get), CAUGHT.with(Cell::get))
  });
  assert_eq!(
    roomy_caught, 0,
    "the control differs in one thing only — with room, nothing trips"
  );
  assert_eq!(
    (&roomy, roomy_ran),
    (&Ok(String::from("<spent>")), 1),
    "the control pins the fixture absolutely: the failure is an ORDINARY one — `lhs` refuses `;` \
     one frame deep — and on its own it is recoverable"
  );

  assert_eq!(
    (tight, ran),
    (Err(()), 0),
    "the ordinary failure is re-raised and the recoverer never runs, because a trip moved the \
     counter during the same attempt. This is the residual: the witness proves that A trip \
     happened inside the attempt, not that THIS error is it — and it cannot prove the second \
     without reading a payload a discarding sink has already thrown away"
  );
}

/// The same, for the non-backtracking sibling: [`inplace_recover`](tokora::ParseInput::inplace_recover)
/// reads the same cell on the same terms and must narrow the same way.
#[test]
fn a_caught_trip_does_not_disable_a_later_inplace_recovery() {
  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<String, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    catch_a_trip(inp);
    pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .inplace_recover(
        |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
         _cursor: tokora::input::Cursor<'inp, '_, TestLexer<'inp>>,
         _err: ()| { counting_recovery(inp, ()) },
      )
      .parse_input(inp)
  }

  let (tight, ran, caught) = bounded(60, || {
    let out = Parser::with_context(limited(8))
      .apply(probe)
      .parse_str("; 1");
    (out, RECOVERIES.with(Cell::get), CAUGHT.with(Cell::get))
  });
  assert_eq!(caught, 1, "the tight run must really have tripped");

  let (roomy, roomy_ran, roomy_caught) = bounded(60, || {
    let out = Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str("; 1");
    (out, RECOVERIES.with(Cell::get), CAUGHT.with(Cell::get))
  });
  assert_eq!(roomy_caught, 0, "and the control must really not have");
  assert_eq!(
    (&roomy, roomy_ran),
    (&Ok(String::from("<spent>")), 1),
    "the control pins the fixture absolutely: the primary fails ordinarily and the recoverer runs"
  );

  assert_eq!(
    (tight, ran),
    (roomy, roomy_ran),
    "an earlier caught trip must not refuse a later in-place recovery either"
  );
}

/// And `skip_then_retry` still skips for an ordinary failure after a caught trip.
///
/// Measured on the **input** rather than on the outcome, exactly as
/// `a_discarding_sink_cannot_let_skip_then_retry_burn_input_on_a_trip` is: both runs end on the
/// `Err` channel here — nothing in `"+ ; 1"` parses — so the discriminating observation is how far
/// the sync cycles got. Re-raising on the session counter would return before any skip, leaving the
/// input where it started; the two runs would then disagree.
#[test]
fn a_caught_trip_does_not_disable_a_later_skip_then_retry() {
  type Probed = (Result<String, ()>, Option<usize>);

  fn probe<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Probed, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    catch_a_trip(inp);
    let out = pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)
      .skip_then_retry(
        |_: &common::TokenKind| tokora::input::Balance::<char>::Neutral,
        |tok: tokora::span::Spanned<&Token, &tokora::SimpleSpan>| matches!(tok.data(), Token::Semi),
      )
      .parse_input(inp);
    let front = inp.next()?.map(|t| tokora::span::Span::start(t.span_ref()));
    Ok((out, front))
  }

  let (tight, caught) = bounded(60, || {
    let out = Parser::with_context(limited(8))
      .apply(probe)
      .parse_str("+ ; 1")
      .unwrap();
    (out, CAUGHT.with(Cell::get))
  });
  assert_eq!(caught, 1, "the tight run must really have tripped");

  let (roomy, roomy_caught) = bounded(60, || {
    let out = Parser::with_context(limited(ROOMY))
      .apply(probe)
      .parse_str("+ ; 1")
      .unwrap();
    (out, CAUGHT.with(Cell::get))
  });
  assert_eq!(roomy_caught, 0, "and the control must really not have");
  assert_ne!(
    roomy.1,
    Some(0),
    "the control pins the fixture absolutely: an ordinary failure makes the sync cycles run and \
     commit skipped input, so the front is no longer the token the parse started on"
  );

  assert_eq!(
    tight, roomy,
    "an earlier caught trip must not turn a later ordinary failure into an immediate re-raise: \
     both the outcome and the input the surrounding grammar is handed back must be the ones an \
     untripped parse produces"
  );
}
