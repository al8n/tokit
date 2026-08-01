#![cfg(feature = "std")]

//! What the typed pratt driver **retains** while it descends, and what it still gives back.
//!
//! The driver used to open its cycle's `Commit` transaction before the operator was classified
//! and keep it open across the recursive operand parse, the fold and the CST wrap. A transaction
//! is not two words: its checkpoint carries the cursor, the committed span, a **clone of
//! `L::State`**, three offsets and an emitter mark. So a right-associative chain of depth `d`
//! pinned `d` of them at once, and a lexer with a large state paid `d` copies of it for as long as
//! the expression took to parse. The probe is now committed the instant the operator is accepted,
//! before any of that, and the only guard live across the recursion is one **expression-scoped**
//! guard opened by `ParseInput::parse_input`.
//!
//! This suite measures that with a lexer state that counts its own clones and drops, and then
//! pins the rollback semantics the narrowing had to preserve — because narrowing a guard is only
//! sound if every exit that used to restore still restores, and if the two exits that can no
//! longer restore for themselves are restored by something **wider**.
//!
//! One thing the narrowing does *not* preserve, and this suite could not pin: the probe's **pin**
//! is released with it, so during classify → recursion → fold → wrap the only live pin is the
//! expression base, and a rewind to a target between the two is no longer refused at the cause.
//! See `the_crossing_rewind_this_suite_could_not_reach` at the foot of this file for why no test
//! here exercises it — the short version is that it cannot be written from outside the crate at
//! all, before the change or after it.
//!
//! * **Probes A and B** are the discriminators: retained clones at the deepest frame, and the
//!   peak concurrent live states, are constants rather than functions of `d`. Both were linear
//!   in `d` before the narrowing — `d + 1` and `d + 3`, measured by running these same cells
//!   against the pre-narrowing driver. Both are now asserted at their **exact** measured figures
//!   — `2` and `4`, at every depth in [`DEPTHS`] and in both profiles — and not as ceilings,
//!   because the claim the narrowing is justified by is a constant and a ceiling cannot pin one.
//! * **Probe C** is the time half, and is green on both sides on purpose: it pins that the space
//!   was not bought by re-saving. A "fix" that released a checkpoint early and re-took it per
//!   cycle would satisfy A and B and fail this. It stays an envelope for that reason — the
//!   narrowing moved its intercept, so an exact form could not be two-sided — but the envelope's
//!   slope is the measured one, which is what makes the sentence before this true.
//! * The **rollback cells** are the acceptance evidence. `unwind_restores_the_whole_expression`
//!   is red before the change and green after: the old per-cycle guards unwound only to the
//!   outermost live cycle's begin point, the expression guard unwinds to before the expression.
//!   `an_rhs_parser_panic_restores_the_whole_expression` is the same claim for the other unwind
//!   site — a panic raised inside `parse_pratt_rhs`, i.e. inside a *live probe guard* nested in
//!   an already-committed cycle — and `a_foot_of_cycle_refusal_restores_the_whole_expression`
//!   is the same claim for the non-panicking one. The `End`/floor/non-associative cells are
//!   keep-green: those exits still hold a live probe guard and must hand their deciding read
//!   straight back to the surrounding grammar.
//! * The **settle-shape cells** pin *how* every scope here is settled, not just that it is. The
//!   discriminator is a session point a hook opened and abandoned: it pins its base, and a
//!   *checked* `Transaction::rollback` refuses to cross a live pin — in release, *before* it
//!   restores anything — whereas `Transaction::rollback_abandoning_points` (and the rolling-back
//!   drop it names) abandons the point and then restores.
//!   `a_session_point_abandoned_in_a_fold_does_not_refuse_the_expression_rollback` pins that for
//!   the **expression** guard, and the five `..._across_an_abandoned_point` cells pin it for the
//!   five **cycle-scoped** probe exits, which called the checked verb until the point was
//!   made. `an_expression_rollback_takes_the_expressions_diagnostics_with_it` pins the other half
//!   of the settle — the emission log moves with the input, in both directions.
//! * The **profile cells** pin how many exits take an expression-scoped restore, which is the
//!   rest of that same contract sentence: *two, and in every build*. The guard is undecided for
//!   the whole of the driver's `parse`, so a `debug_assert` raised in there is a third one,
//!   visible only from a debug build — and the driver has three contract assertions to raise. The
//!   three `..._keeps_the_expression_in_both_profiles` cells assert the emission log and the
//!   handback with the same expected values on both sides of the profile split, and require the
//!   violation to arrive as a panic in debug and as the terminal error in release so neither side
//!   can pass vacuously.
//!
//! ## What this suite cannot reach: the `no_std` unwind edge
//!
//! The expression guard is `Rollback` policy rather than `Commit`, and the difference is
//! invisible from here. A `Commit` guard dropped mid-unwind rolls back too — but only in a
//! `std` build, where `Commit` reads `std::thread::panicking()`; under `no_std` the crate has no
//! panic fact to read, the constant is `false`, and a `Commit` guard *keeps* the torn
//! expression. That configuration cannot be exercised by any test in this file: observing a
//! caught unwind needs `catch_unwind`, which is `std`-only, so the very build in which the two
//! policies differ is the one in which the difference is unobservable. What is observable, and
//! is pinned above, is that the restore no longer depends on the unwind fact at all — the
//! foot-of-cycle and session-point cells reach the guard's rolling-back drop on paths where
//! nothing is panicking, which only a `ROLLBACK_ON_DROP` policy takes.
//!
//! The counters are process-global statics, so **every** test here — measuring or not — is
//! serialised behind [`MEASURE`], and takes the lock in a poison-tolerant way because one cell
//! panics by design.

use core::{
  cell::{Cell, RefCell},
  sync::atomic::{AtomicIsize, AtomicUsize, Ordering},
};
use std::sync::Mutex;

use tokora::{
  Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext, SimpleSpan,
  Token as TokenT,
  emitter::Fatal,
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
    token::UnexpectedTokenOf,
  },
  input::Cursor,
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  span::Spanned,
  state::{State, recursion_tracker::RecursionLimiter},
};

// ═══════════════════════════════════════════════════════════════════════════════
// The instrumented lexer state
// ═══════════════════════════════════════════════════════════════════════════════

/// Every `L::State` clone the input layer takes — a checkpoint capture is one — and every drop.
static CLONES: AtomicUsize = AtomicUsize::new(0);
/// Outstanding balance: states constructed or cloned, minus states dropped. Signed, because a
/// reset performed while a state is still live would otherwise underflow rather than read
/// negative, and a silent underflow is exactly the shape that makes a retention probe pass
/// vacuously.
static LIVE: AtomicIsize = AtomicIsize::new(0);
/// High-water mark of [`LIVE`].
static PEAK: AtomicIsize = AtomicIsize::new(0);

fn reset_counters() {
  CLONES.store(0, Ordering::Relaxed);
  LIVE.store(0, Ordering::Relaxed);
  PEAK.store(0, Ordering::Relaxed);
}

fn born() {
  let now = LIVE.fetch_add(1, Ordering::Relaxed) + 1;
  PEAK.fetch_max(now, Ordering::Relaxed);
}

/// A lexer state that is nothing but an accounting hook. It has no field: the whole point is that
/// what is being counted is the *number of live copies*, not their size — a real grammar's state
/// (a mode stack, an indentation stack, an interpolation depth) makes each copy expensive, and
/// this suite would rather fail on the count than on a benchmark.
#[derive(Debug)]
struct CountedState;

impl CountedState {
  fn new() -> Self {
    born();
    CountedState
  }
}

impl Default for CountedState {
  fn default() -> Self {
    Self::new()
  }
}

impl Clone for CountedState {
  fn clone(&self) -> Self {
    CLONES.fetch_add(1, Ordering::Relaxed);
    Self::new()
  }
}

impl Drop for CountedState {
  fn drop(&mut self) {
    LIVE.fetch_sub(1, Ordering::Relaxed);
  }
}

impl State for CountedState {
  type Error = ();

  fn check(&self) -> Result<(), ()> {
    Ok(())
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tokens and the one-character-per-token lexer
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
  Num,
  Caret,
  Rewind,
}

impl core::fmt::Display for Kind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Kind::Num => write!(f, "number"),
      Kind::Caret => write!(f, "^"),
      Kind::Rewind => write!(f, "@"),
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
  Num(i64),
  Caret,
  /// The foot-of-cycle fixture's token — see [`CountLexer::span`] for what makes it special and
  /// `a_fold_that_rewound_behind_the_operator_is_refused` for why nothing gentler reaches that
  /// check from outside the crate.
  Rewind,
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

impl TokenT<'_> for Tok {
  type Kind = Kind;
  type Error = ();

  fn kind(&self) -> Kind {
    match self {
      Tok::Num(_) => Kind::Num,
      Tok::Caret => Kind::Caret,
      Tok::Rewind => Kind::Rewind,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// One token per character over `1`-`9`, `^`, `@` and spaces.
struct CountLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  /// Set while the token just produced is [`Tok::Rewind`]; see [`CountLexer::span`].
  rewinding: bool,
  state: CountedState,
}

impl<'a> Lexer<'a> for CountLexer<'a> {
  type State = CountedState;
  type Source = str;
  type Token = Tok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self::with_state(src, CountedState::new())
  }

  fn with_state(src: &'a str, state: CountedState) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      rewinding: false,
      state,
    }
  }

  fn check(&self) -> Result<(), ()> {
    Ok(())
  }

  fn state(&self) -> &CountedState {
    &self.state
  }

  fn state_mut(&mut self) -> &mut CountedState {
    &mut self.state
  }

  fn into_state(self) -> CountedState {
    self.state
  }

  fn source(&self) -> &'a str {
    self.src
  }

  /// The span the input layer stores as committed consumption when this token settles.
  ///
  /// Honest for every token but [`Tok::Rewind`], which reports `(0, 1)` — the source's *first*
  /// character — wherever in the source it actually sits. That is a **deliberate lexer-contract
  /// violation**, and it is the only route this suite found that moves committed consumption
  /// *backwards* from inside a pratt fold, which is what the driver's foot-of-cycle refusal
  /// exists to catch. Every other rewind primitive is either lexically scoped to the hook call
  /// that opened it (`begin`, `attempt`, `begin_stacked`) or needs a token — a `Checkpoint`, a
  /// `SessionPointId` — that is invariantly branded with the input handle's `'closure` lifetime;
  /// and every pratt hook is higher-ranked over that lifetime, so no such token can be carried
  /// from the RHS parser into the fold. See the foot-of-cycle cell for the full argument.
  ///
  /// It is `(0, 1)` and not `(0, 0)` because the input layer holds lexers to a nonempty span
  /// (`lex_within_boundary`'s debug assertion) and that check is not the one under test here.
  fn span(&self) -> SimpleSpan {
    if self.rewinding {
      SimpleSpan::new(0, 1)
    } else {
      SimpleSpan::new(self.start, self.end)
    }
  }

  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }

  fn lex(&mut self) -> Option<Result<Tok, ()>> {
    let b = self.src.as_bytes();
    let mut i = self.end;
    while i < b.len() && b[i] == b' ' {
      i += 1;
    }
    self.rewinding = false;
    if i >= b.len() {
      self.start = i;
      self.end = i;
      return None;
    }
    self.start = i;
    let c = b[i];
    self.end = i + 1;
    Some(match c {
      b'^' => Ok(Tok::Caret),
      b'@' => {
        self.rewinding = true;
        Ok(Tok::Rewind)
      }
      d if d.is_ascii_digit() => Ok(Tok::Num(i64::from(d - b'0'))),
      _ => Err(()),
    })
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// The fixture error
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
struct RetErr;

impl From<()> for RetErr {
  fn from(_: ()) -> Self {
    RetErr
  }
}
impl<'inp> From<UnexpectedTokenOf<'inp, CountLexer<'inp>>> for RetErr {
  fn from(_: UnexpectedTokenOf<'inp, CountLexer<'inp>>) -> Self {
    RetErr
  }
}
impl From<UnexpectedEoLhs> for RetErr {
  fn from(_: UnexpectedEoLhs) -> Self {
    RetErr
  }
}
impl From<UnexpectedEoRhs> for RetErr {
  fn from(_: UnexpectedEoRhs) -> Self {
    RetErr
  }
}
impl From<RecursionLimitReached> for RetErr {
  fn from(_: RecursionLimitReached) -> Self {
    RetErr
  }
}
impl From<NonAssociativeChain> for RetErr {
  fn from(_: NonAssociativeChain) -> Self {
    RetErr
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for RetErr {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    RetErr
  }
}
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for RetErr
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    RetErr
  }
}

type Ir<'inp, 'c, Ctx> = InputRef<'inp, 'c, CountLexer<'inp>, Ctx>;

// ═══════════════════════════════════════════════════════════════════════════════
// The measured grammar: `1 ^ 1 ^ 1 …`, right-associative
// ═══════════════════════════════════════════════════════════════════════════════

/// The binding power `^` reports. One value: this suite is about retention, not precedence.
const CARET: u8 = 5;

/// How many LHS entries this parse will make — `d + 1` for depth `d`. Set before the parse.
static TARGET_LHS_ENTRY: AtomicUsize = AtomicUsize::new(0);
/// LHS entries so far. Frame *k*'s LHS is entry *k + 1*, in strictly increasing depth order,
/// which is what makes the counter a depth probe without a separate stack.
static LHS_ENTRIES: AtomicUsize = AtomicUsize::new(0);
/// [`LIVE`] sampled at the entry to the **deepest** frame's LHS parser — the one moment every
/// frame of the chain is on the stack at once. This is the number the narrowing changes.
static LIVE_AT_DEEPEST: AtomicIsize = AtomicIsize::new(-1);

fn measured_lhs<'inp, Ctx>(inp: &mut Ir<'inp, '_, Ctx>) -> Result<PrattLHS<i64, (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let entry = LHS_ENTRIES.fetch_add(1, Ordering::Relaxed) + 1;
  if entry == TARGET_LHS_ENTRY.load(Ordering::Relaxed) {
    // Sampled BEFORE this frame consumes anything, so what it counts is purely what the frames
    // above it are holding.
    LIVE_AT_DEEPEST.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
  }
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Tok::Num(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(RetErr),
    },
    None => Err(RetErr),
  }
}

/// Right-associative `^`, and [`PrattRHS::End`] for everything else — including exhaustion, which
/// is what keeps the sentinel idiom out of this file.
fn measured_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Tok::Caret => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Right(()),
        CARET,
      ))),
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

fn fold_prefix<'inp, Ctx>(
  _i: &mut Ir<'inp, '_, Ctx>,
  o: i64,
  _op: Precedenced<(), u8>,
) -> Result<i64, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  Ok(o)
}

fn fold_postfix<'inp, Ctx>(
  _i: &mut Ir<'inp, '_, Ctx>,
  o: i64,
  _op: Precedenced<(), u8>,
) -> Result<i64, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  Ok(o)
}

fn fold_infix<'inp, Ctx>(
  _i: &mut Ir<'inp, '_, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, u8>,
) -> Result<i64, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  Ok(l + r)
}

fn measured_expr<'inp, Ctx>(inp: &mut Ir<'inp, '_, Ctx>) -> Result<i64, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  pratt(
    measured_lhs,
    measured_rhs,
    fold_prefix,
    fold_infix,
    fold_postfix,
  )
  .parse_input(inp)
}

/// `"1 " + "^ 1 " * d` — `d` right-associative operators, so `d` nested driver frames.
fn chain(d: usize) -> String {
  let mut s = String::from("1 ");
  for _ in 0..d {
    s.push_str("^ 1 ");
  }
  s
}

/// One measurement: counters zeroed, the LHS sampler armed for depth `d`, one parse.
struct Measured {
  live_at_deepest: isize,
  peak: isize,
  clones: usize,
  value: i64,
}

/// The context the sweeping probes run under, and the one thing about it that is not incidental:
/// it sets its **own** recursion budget.
///
/// [`DEPTHS`] tops out at 64 operators, which is 65 live driver frames, and the input's *default*
/// budget is 64 — deliberately conservative, sized against the tightest measured native ceiling
/// rather than the most generous (see `RecursionLimiter`'s `Default Limit`). A retention probe
/// that inherited it would stop measuring retention and start measuring the budget, and it would
/// do so silently: `measure` would fail at its `expect`, at one depth, for a reason that has
/// nothing to do with checkpoints. What the default is, and that both engines honour it, is
/// `pratt_limit.rs`'s subject; this suite says so explicitly and looks away.
fn measuring_ctx<'inp>() -> ParserContext<'inp, CountLexer<'inp>, Fatal<RetErr>> {
  ParserContext::new(Fatal::new()).with_recursion_limiter(RecursionLimiter::unlimited())
}

fn measure(d: usize) -> Measured {
  let src = chain(d);
  LHS_ENTRIES.store(0, Ordering::Relaxed);
  TARGET_LHS_ENTRY.store(d + 1, Ordering::Relaxed);
  LIVE_AT_DEEPEST.store(-1, Ordering::Relaxed);
  reset_counters();
  let value: i64 = Parser::with_context(measuring_ctx())
    .apply(measured_expr)
    .parse_str(&src)
    .expect("the chain parses");
  let m = Measured {
    live_at_deepest: LIVE_AT_DEEPEST.load(Ordering::Relaxed),
    peak: PEAK.load(Ordering::Relaxed),
    clones: CLONES.load(Ordering::Relaxed),
    value,
  };
  assert_eq!(
    m.value,
    (d + 1) as i64,
    "the fixture must actually parse the whole chain at depth {d}, or the probe is measuring \
     nothing"
  );
  assert!(
    m.live_at_deepest >= 0,
    "the deepest LHS entry was never reached at depth {d}: the sampler is mis-armed and the \
     probe would pass vacuously"
  );
  m
}

/// The depths every probe sweeps. Two decades of `d`, so a retained-per-frame checkpoint cannot
/// hide inside a generous constant.
const DEPTHS: [usize; 6] = [2, 4, 8, 16, 32, 64];

/// The counters are process-global. Every cell in this file takes this, measuring or not.
static MEASURE: Mutex<()> = Mutex::new(());

fn measuring() -> std::sync::MutexGuard<'static, ()> {
  // One cell panics by design, so the lock is expected to be poisoned some of the time; a
  // poisoned counter mutex means "the previous test died", not "the counters are unusable".
  MEASURE.lock().unwrap_or_else(|e| e.into_inner())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Probe A — retained clones at the deepest frame
// ═══════════════════════════════════════════════════════════════════════════════

/// Retained lexer-state clones at the deepest frame: the exact figure, not a ceiling.
///
/// Measured on the fixed driver, identical at every depth in [`DEPTHS`] and in both profiles:
/// **2** — the input's own live `L::State`, plus the one clone held by the expression-scoped
/// guard `parse_input` opens. No operator probe is live at that instant: each frame commits its
/// probe *before* it recurses, which is the whole narrowing.
///
/// On the driver this branch replaced it was **`d + 1`** — 3, 5, 9, 17, 33, 65 across [`DEPTHS`],
/// measured by running this same cell against the pre-narrowing driver rather than reasoned from
/// the shape of the loop. That is the win, and it is why the number below is a constant.
///
/// Asserted exactly because the claim this branch is justified by is a *constant*, and a ceiling
/// cannot pin a constant. A ceiling of `4` — which is what this cell enforced until Codex round 5
/// pointed at the gap — passes a driver leaving one or two extra checkpoints live at every depth,
/// which is precisely the retained-memory regression the cell exists to catch. If a future change
/// genuinely needs a third live state, raise this number *and* say what holds it; do not widen it
/// back into a range.
const RETAINED_EXACT: isize = 2;

#[test]
fn retained_state_clones_do_not_grow_with_operator_depth() {
  let _g = measuring();
  let mut rows = Vec::new();
  for d in DEPTHS {
    let m = measure(d);
    rows.push((d, m.live_at_deepest));
  }
  for (d, live) in &rows {
    assert_eq!(
      *live, RETAINED_EXACT,
      "depth {d}: {live} live lexer-state clones at the deepest frame, expected exactly \
       {RETAINED_EXACT}. Every live checkpoint holds one — cursor, span, `L::State`, three \
       offsets and an emitter mark apiece — so a count that tracks the depth means the driver is \
       holding one guard per operator cycle across the recursion again, and a count that is \
       merely higher means something is retained that the narrowing said was not. Rows: {rows:?}"
    );
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Probe B — peak concurrent live states, and its independence from depth
// ═══════════════════════════════════════════════════════════════════════════════

/// Peak concurrent live lexer states over a whole parse: the exact figure, not a ceiling.
///
/// Measured on the fixed driver, identical at every depth in [`DEPTHS`] and in both profiles:
/// **4** — the two of [`RETAINED_EXACT`], plus the one operator probe that is live while the RHS
/// parser is deciding, plus the state the token cache carries beside a token it is holding.
///
/// On the pre-narrowing driver, measured the same way: **`d + 3`** — 5, 7, 11, 19, 35, 67 across
/// [`DEPTHS`].
///
/// Asserting it at *every* depth is the shape claim the old `ceiling + slack` pair made in two
/// weaker steps. A driver retaining one checkpoint per frame moves this number with `d`; so does
/// a driver that holds one extra everywhere. Both now fail, and they fail at the depth where they
/// first appear rather than only at the ends of the sweep.
const PEAK_EXACT: isize = 4;

#[test]
fn peak_live_states_are_independent_of_operator_depth() {
  let _g = measuring();
  let mut rows = Vec::new();
  for d in DEPTHS {
    let m = measure(d);
    rows.push((d, m.peak));
  }
  for (d, peak) in &rows {
    assert_eq!(
      *peak,
      PEAK_EXACT,
      "depth {d}: peak {peak} concurrent live lexer states, expected exactly {PEAK_EXACT} — the \
       same figure at every depth from {} to {}. A peak that moves with `d` means the driver is \
       holding a guard per operator cycle across the recursion again; a peak that is flat but \
       higher means one more state is live throughout than the narrowing accounts for. Rows: \
       {rows:?}",
      DEPTHS[0],
      DEPTHS[DEPTHS.len() - 1]
    );
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Probe C — the time half. Green before AND after.
// ═══════════════════════════════════════════════════════════════════════════════

/// The linear envelope on total state clones.
///
/// Measured on the fixed driver, in both profiles: exactly `5 * d + 4` clones at depth `d` — 14,
/// 24, 44, 84, 164, 324 across [`DEPTHS`]. Five per operator, because a cycle's probe is one
/// capture and the cache carries a state beside each token it holds; four fixed, for the
/// expression guard and the input's own construction and handover.
///
/// On the pre-narrowing driver, measured the same way: exactly `5 * d + 3` — 13, 23, 43, 83, 163,
/// 323. The slope is **the same five**; the narrowing moved the intercept by one, which is the
/// expression-scoped guard's own capture. That is the whole difference, and it is what makes this
/// cell a two-sided control rather than a discriminator.
///
/// Unlike Probes A and B this therefore stays an **envelope**: an exact assertion would be red on
/// the old driver — `5 * d + 3` is not `5 * d + 4` — and the cell would stop being two-sided,
/// which is the only property that makes it a control at all.
///
/// What the envelope keeps is the **slope**, at exactly the measured five, with headroom left
/// only on the constant. That is the tightening this cell needed: at a slope of six it admitted
/// the very thing its section header claims it forbids — a "fix" that released a checkpoint early
/// and re-took it per cycle, which is one extra capture per operator. `6 * d + 4` now breaks the
/// line at `d = 32` and `d = 64`, where before it fitted underneath at every depth swept; and
/// `5 * d + 3` still fits, which was checked by running this cell against the old driver.
fn clone_envelope(d: usize) -> usize {
  5 * d + 32
}

#[test]
fn total_state_clones_stay_inside_a_linear_envelope() {
  let _g = measuring();
  let mut rows = Vec::new();
  for d in DEPTHS {
    let m = measure(d);
    rows.push((d, m.clones));
  }
  for (d, clones) in &rows {
    assert!(
      *clones <= clone_envelope(*d),
      "depth {d}: {clones} total state clones, envelope {}. Checkpoint WORK must stay linear in \
       the number of operator probes — one save/settle pair per cycle plus one per expression. A \
       count above this line means a checkpoint is being re-taken rather than held, which trades \
       the space fix for time. Rows: {rows:?}",
      clone_envelope(*d)
    );
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// E10 / E11 — an unwind restores the whole expression
// ═══════════════════════════════════════════════════════════════════════════════

thread_local! {
  /// Armed once; the next `fold_infix` call panics and disarms itself so the unwind cannot meet a
  /// second panic.
  static FOLD_BOMB: Cell<bool> = const { Cell::new(false) };
}

fn panicking_fold_infix<'inp, Ctx>(
  _i: &mut Ir<'inp, '_, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, u8>,
) -> Result<i64, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  if FOLD_BOMB.with(|c| c.replace(false)) {
    panic!("the armed fold panics");
  }
  Ok(l + r)
}

/// A panic through a fold leaves the input where the expression started, not where the innermost
/// live cycle started.
///
/// This is the E10/E11 compensation, and it is the one cell in this file that is **red before the
/// narrowing**. `Commit`'s "a panic is not a decision" was written for this loop, and the state it
/// names — the operator consumed, the right-hand side absent — is what a half-applied cycle
/// leaves. With a guard per cycle, an unwind rolled back to the outermost *live cycle's* begin
/// point, which is after the expression's own left-hand side: the surrounding grammar resumed
/// mid-expression, seeing `^` where it had handed over `1`. With one expression-scoped guard the
/// unwind restores the whole expression, so the very next token is the one the expression began
/// with.
///
/// Strictly broader, never narrower — which is the only way a narrowing of the *live* scope can
/// be sound.
#[test]
fn unwind_restores_the_whole_expression() {
  let _g = measuring();
  let probe = |inp: &mut Ir<'static, '_, _>| -> Result<(bool, Option<(usize, usize)>), RetErr> {
    FOLD_BOMB.with(|c| c.set(true));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      pratt(
        measured_lhs,
        measured_rhs,
        fold_prefix,
        panicking_fold_infix,
        fold_postfix,
      )
      .parse_input(inp)
    }));
    // The bomb must actually have gone off, or this cell proves nothing.
    assert!(caught.is_err(), "the fixture needs the fold to panic");
    let front = inp.next()?.map(|t| (t.span().start(), t.span().end()));
    Ok((caught.is_err(), front))
  };

  LHS_ENTRIES.store(0, Ordering::Relaxed);
  TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
  let (panicked, front): (bool, Option<(usize, usize)>) = Parser::new()
    .apply(probe)
    .parse_str("1 ^ 1 ^ 1")
    .expect("the probe converts the outcome into a report");

  assert!(panicked);
  assert_eq!(
    front,
    Some((0, 1)),
    "after an unwind through a fold, the surrounding grammar must be handed back the token the \
     expression began with — offset 0. A per-cycle guard restores only to the innermost live \
     cycle's begin point and leaves the front at the first `^` (offset 2) or later, which is the \
     'operator consumed, right-hand side absent' state `Commit` documents"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// E2 / E3 — every exit that used to roll back still rolls back
// ═══════════════════════════════════════════════════════════════════════════════
//
// These three are keep-green. Each still holds a LIVE probe guard at the moment it decides — the
// narrowing's commit sits strictly behind all of them — so what the RHS parser consumed while
// deciding must come straight back to the surrounding grammar. The shape is
// `pratt_progress_guard.rs`'s `a_below_floor_report_that_consumed_nothing_still_ends_the_loop_cleanly`,
// generalised to the three exits and to a report that DID consume.

/// Consumes the `^` and then declares the expression over: the driver must hand the `^` back.
fn end_after_consuming_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.next()?;
  Ok(PrattRHS::End)
}

/// Consumes the `^` and reports a postfix one level under a raised entry floor.
fn below_floor_after_consuming_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.next()?;
  Ok(PrattRHS::Postfix(Precedenced::new((), CARET - 1)))
}

/// Reports the same **non-associative** `^` at every position, consuming it each time. The second
/// one in a row is the repeat the driver refuses.
fn neither_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Tok::Caret => Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Neither(()),
        CARET,
      ))),
      _ => Ok(PrattRHS::End),
    },
    None => Ok(PrattRHS::End),
  }
}

/// Runs `rhs` over `src` at entry floor `floor` and reports what the surrounding grammar is handed
/// back afterwards.
macro_rules! handback_cell {
  ($name:ident, $rhs:expr, $floor:expr, $src:literal, $want:expr, $why:literal) => {
    #[test]
    fn $name() {
      let _g = measuring();
      let probe = |inp: &mut Ir<'static, '_, _>| -> Result<(i64, Option<(usize, usize)>), RetErr> {
        let v = pratt(measured_lhs, $rhs, fold_prefix, fold_infix, fold_postfix)
          .min_precedence($floor)
          .parse_input(inp)?;
        let front = inp.next()?.map(|t| (t.span().start(), t.span().end()));
        Ok((v, front))
      };
      LHS_ENTRIES.store(0, Ordering::Relaxed);
      TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
      let got: (i64, Option<(usize, usize)>) = Parser::new()
        .apply(probe)
        .parse_str($src)
        .expect("the probe converts the outcome into a report");
      assert_eq!(got, $want, $why);
    }
  };
}

/// The [`handback_cell!`] twin for the one probe exit that ends the **parse** rather than the
/// expression: the non-associative repeat, which restores the deciding read exactly as the other
/// four do and then returns `NonAssociativeChain`. The pratt parser's `Err` is caught inside the
/// probe so the same question can still be asked of it — *what is the surrounding grammar handed
/// back?* — which is the retention property these cells exist to pin, and which the repeat's
/// contract did not change.
macro_rules! handback_err_cell {
  ($name:ident, $rhs:expr, $floor:expr, $src:literal, $want:expr, $why:literal) => {
    #[test]
    fn $name() {
      let _g = measuring();
      let probe = |inp: &mut Ir<'static, '_, _>| -> Result<Option<(usize, usize)>, RetErr> {
        let outcome = pratt(measured_lhs, $rhs, fold_prefix, fold_infix, fold_postfix)
          .min_precedence($floor)
          .parse_input(inp);
        assert!(
          outcome.is_err(),
          "a second same-power non-associative operator must fail the parse"
        );
        Ok(inp.next()?.map(|t| (t.span().start(), t.span().end())))
      };
      LHS_ENTRIES.store(0, Ordering::Relaxed);
      TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
      let got: Option<(usize, usize)> = Parser::new()
        .apply(probe)
        .parse_str($src)
        .expect("the probe converts the outcome into a report");
      assert_eq!(got, $want, $why);
    }
  };
}

handback_cell!(
  a_consuming_end_report_is_rolled_back,
  end_after_consuming_rhs,
  0u8,
  "1 ^ 1",
  (1i64, Some((2usize, 3usize))),
  "PrattRHS::End restores whatever the decision consumed: the `^` at offset 2 must be sitting in \
   front of the surrounding grammar, not swallowed by the expression that declined it"
);

handback_cell!(
  a_floor_declined_report_is_rolled_back,
  below_floor_after_consuming_rhs,
  CARET,
  "1 ^ 1",
  (1i64, Some((2usize, 3usize))),
  "a report the floor declines is not this expression's operator: the deciding read is rolled \
   back and the `^` goes to the surrounding grammar"
);

handback_err_cell!(
  a_non_associative_repeat_is_rolled_back,
  neither_rhs,
  0u8,
  "1 ^ 1 ^ 1",
  Some((6usize, 7usize)),
  "the second non-associative operator at the same power is refused, and the read that reported \
   it is rolled back: the second `^` at offset 6 is parked in front of the surrounding grammar, \
   which is the position the returned `NonAssociativeChain` names. A driver that committed its \
   probe before this check would eat it"
);

// ═══════════════════════════════════════════════════════════════════════════════
// All five probe exits, with a session point the RHS parser abandoned
// ═══════════════════════════════════════════════════════════════════════════════
//
// The three cells above prove three of the probe exits *restore*. These five — the same three,
// plus both report-boundary stalls — prove that all five restore **the way the drop path does**:
// reconciling, not refusing. They are the red-before/green-after evidence for the third settle
// verb, and there is one per `rollback` call site the driver had.
//
// `parse_pratt_rhs` is grammar-author code holding a full `InputRef`, so it may open a
// [session point](tokora::InputRef::begin_point) and abandon it while deciding. That is legal:
// `begin_point`'s contract says an abandoned point keeps its progress and is released with the
// handle. What it also does is leave the point's base **pinned**, and a pin is what a *checked*
// restore refuses to cross — a release panic, in every allocator build, raised before anything is
// restored. So a probe exit spelled `Transaction::rollback` turns an ordinary handback into a
// panic, with the deciding read still consumed for any host that catches. Spelled
// `rollback_abandoning_points`, the point is abandoned and the restore proceeds, exactly as the
// expression guard's own rolling-back drop has always done for `Fault::Rewind`.
//
// Red baseline: all five panic at the pin against the tree at the *pre-fix* commit — the
// narrowing with its five probe exits still calling `txn.rollback()` — not against `main`, whose
// wide cycle guard this suite's other cells discriminate against. Green: the three handback cells
// assert the same handback their non-leaking twins do, byte for byte, and the two stall cells
// reach the contract violation they were written to report.

/// [`end_after_consuming_rhs`], having first opened a session point it never settles.
fn point_leaking_end_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.begin_point();
  end_after_consuming_rhs(inp)
}

/// [`below_floor_after_consuming_rhs`], having first opened a session point it never settles.
fn point_leaking_below_floor_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.begin_point();
  below_floor_after_consuming_rhs(inp)
}

/// [`neither_rhs`], having first opened a session point it never settles — on **every** call, so
/// the exits cross a point opened by an already-committed cycle as well as one opened by their
/// own.
fn point_leaking_neither_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.begin_point();
  neither_rhs(inp)
}

handback_cell!(
  a_consuming_end_report_is_rolled_back_across_an_abandoned_point,
  point_leaking_end_rhs,
  0u8,
  "1 ^ 1",
  (1i64, Some((2usize, 3usize))),
  "an abandoned session point pins its base; the `PrattRHS::End` exit must reconcile it and \
   restore, handing the `^` at offset 2 to the surrounding grammar. Spelled with the checked \
   `Transaction::rollback` this exit panics at the pin instead, with nothing restored"
);

handback_cell!(
  a_floor_declined_report_is_rolled_back_across_an_abandoned_point,
  point_leaking_below_floor_rhs,
  CARET,
  "1 ^ 1",
  (1i64, Some((2usize, 3usize))),
  "the floor-decline exit must reconcile the abandoned point and restore, not refuse to cross it: \
   a report the floor declines is an ordinary handback, never a panic"
);

handback_err_cell!(
  a_non_associative_repeat_is_rolled_back_across_an_abandoned_point,
  point_leaking_neither_rhs,
  0u8,
  "1 ^ 1 ^ 1",
  Some((6usize, 7usize)),
  "the non-associative repeat exit must reconcile the abandoned points and restore. This one also \
   crosses a point opened by a cycle that has since COMMITTED its probe, so the reconciliation \
   has to abandon exactly the points younger than the exiting probe's base and leave the older \
   one alone"
);

/// Opens a session point it never settles, then reports an admitted `Postfix` having consumed
/// nothing — the postfix report-boundary stall, with a pin above the probe base.
fn point_leaking_stalled_postfix_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.begin_point();
  Ok(PrattRHS::Postfix(Precedenced::new((), CARET)))
}

/// The infix twin of [`point_leaking_stalled_postfix_rhs`] — the other report-boundary stall, and
/// the other `rollback` call site in the `Infix` arm.
fn point_leaking_stalled_infix_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.begin_point();
  Ok(PrattRHS::Infix(Precedenced::new(
    PrattInfix::Right(()),
    CARET,
  )))
}

/// The two report-boundary stalls, which roll their cycle back and *then* assert.
///
/// Both profiles discriminate, and on different observables. **Debug**: the exit must reach its
/// own `debug_assert` — the "consumed nothing" contract violation it exists to report — so a
/// settle that refused at the pin first fails the `expected` match. **Release**: there is no
/// assertion, so the exit must return the terminal error; a settle that refused unwinds out of
/// the parse instead and fails the test.
macro_rules! stalled_point_cell {
  ($name:ident, $rhs:expr, $why:literal) => {
    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "consumed nothing"))]
    fn $name() {
      let _g = measuring();
      LHS_ENTRIES.store(0, Ordering::Relaxed);
      TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
      let outcome: Result<i64, RetErr> = Parser::new()
        .apply(|inp: &mut Ir<'static, '_, _>| -> Result<i64, RetErr> {
          pratt(measured_lhs, $rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
        })
        .parse_str("1 ^ 1");
      assert!(outcome.is_err(), $why);
    }
  };
}

stalled_point_cell!(
  a_stalled_postfix_report_is_rolled_back_across_an_abandoned_point,
  point_leaking_stalled_postfix_rhs,
  "release: the postfix report-boundary stall is a terminal error, not a panic — the rollback \
   ahead of the assertion must reconcile the abandoned point rather than refuse at its pin"
);

stalled_point_cell!(
  a_stalled_infix_report_is_rolled_back_across_an_abandoned_point,
  point_leaking_stalled_infix_rhs,
  "release: the infix report-boundary stall is a terminal error, not a panic — the rollback ahead \
   of the assertion must reconcile the abandoned point rather than refuse at its pin"
);

// ═══════════════════════════════════════════════════════════════════════════════
// E9 — the foot-of-cycle refusal
// ═══════════════════════════════════════════════════════════════════════════════

/// Consumes the `@`, whose settle writes committed consumption back to offset 0 — behind the
/// operator this fold was handed. See [`CountLexer::span`].
fn rewinding_fold_infix<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, u8>,
) -> Result<i64, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  let _ = inp.next()?;
  Ok(l + r)
}

/// A fold that rewound behind its operator is refused, and the refusal restores before it reports.
///
/// The driver's probe transaction is committed the moment the operator is accepted, so by the time
/// this is detected the frame has no guard of its own left: the refusal travels out as the
/// posture (`Fault::Rewind`) and the expression-scoped guard performs the restore, then the
/// assertion fires. The ordering is kept, but no longer load bearing: the expression guard is
/// `Rollback` policy, so an assertion raised ahead of the restore would panic past it and the
/// unwinding drop would restore anyway. (It *was* load bearing under a `Commit` guard, and only
/// on a `no_std` build even then — a `std` `Commit` guard reads `thread::panicking()` and rolls
/// back on the unwind edge too.) What this cell pins is the report; the restore it happens
/// *from* is pinned by `a_foot_of_cycle_refusal_restores_the_whole_expression` below.
///
/// **On the fixture.** The rewind is performed by a lexer that reports the span `(0, 1)` for `@`
/// wherever `@` actually sits. That is a contract violation on the lexer's part, and it is used
/// because it is the only route this suite could find; see
/// `the_crossing_rewind_this_suite_could_not_reach` for why the design's preferred route — a
/// session point opened in the RHS parser and rolled back in the fold — cannot be written at all.
/// Committed consumption is otherwise monotone, so short of a lying lexer this check is
/// unreachable from outside the crate. That is a property of the *check*, and it held identically
/// before the narrowing.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "rewound the input behind the operator")
)]
fn a_fold_that_rewound_behind_the_operator_is_refused() {
  let _g = measuring();
  LHS_ENTRIES.store(0, Ordering::Relaxed);
  TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
  let outcome: Result<i64, RetErr> = Parser::new()
    .apply(|inp: &mut Ir<'static, '_, _>| -> Result<i64, RetErr> {
      pratt(
        measured_lhs,
        measured_rhs,
        fold_prefix,
        rewinding_fold_infix,
        fold_postfix,
      )
      .parse_input(inp)
    })
    .parse_str("1 ^ 1 @");
  assert!(
    outcome.is_err(),
    "release: the refusal is a terminal error, not a truncated `Ok` — got {outcome:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The expression-scoped settle: what it restores, and how it is settled
// ═══════════════════════════════════════════════════════════════════════════════
//
// The E9 cell above proves the foot-of-cycle refusal *reports*. These prove it **restores**, and
// that the guard's own settle survives the two things that can make a restore refuse or diverge:
// a live pin above the base, and a build with no panic fact to read.

/// Runs `body` under `catch_unwind` and reports the token the surrounding grammar is handed
/// afterwards.
///
/// Both profiles are covered by one shape on purpose. Every cell below drives the driver onto an
/// exit that raises a `debug_assert` *after* the guard has already been settled, so a debug build
/// unwinds out of `parse_input` with the restore complete and a release build returns the terminal
/// error with the restore equally complete. Reading the front token after catching is therefore
/// the same measurement in both — and it is the measurement that matters, because a settle that
/// panicked *before* restoring also unwinds, and is caught here just the same, with the input left
/// where the broken hook put it.
fn front_after_settle<'inp, Ctx, F>(inp: &mut Ir<'inp, '_, Ctx>, body: F) -> Option<(usize, usize)>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
  F: FnOnce(&mut Ir<'inp, '_, Ctx>) -> Result<i64, RetErr>,
{
  settle(inp, body);
  front(inp)
}

/// The settle half of [`front_after_settle`], split out because the diagnostics cell has to read
/// the emission log **between** the settle and the front read — reading the front is itself a
/// token settle, and a token settle lands in the very channel that cell measures.
fn settle<'inp, Ctx, F>(inp: &mut Ir<'inp, '_, Ctx>, body: F)
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
  F: FnOnce(&mut Ir<'inp, '_, Ctx>) -> Result<i64, RetErr>,
{
  LHS_ENTRIES.store(0, Ordering::Relaxed);
  TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
  let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(inp)));
  match &outcome {
    // Release: the refusal is the terminal error, never a truncated `Ok`.
    Ok(r) => assert!(
      r.is_err(),
      "the fixture must reach the refusal it is written for — got {r:?}"
    ),
    // A *release* build that unwinds out of here has had a settle refuse rather than restore —
    // the defect these cells exist to catch — so it fails loudly instead of passing quietly.
    Err(_) if !cfg!(debug_assertions) => panic!(
      "a release build must not panic out of this exit: the guard's settle is supposed to \
       restore, not refuse"
    ),
    // Debug: the assertion the refusal raises once the guard has settled.
    Err(_) => {}
  }
}

/// The front-read half of [`front_after_settle`]: the token the surrounding grammar is handed.
fn front<'inp, Ctx>(inp: &mut Ir<'inp, '_, Ctx>) -> Option<(usize, usize)>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  inp
    .next()
    .ok()
    .flatten()
    .map(|t| (t.span().start(), t.span().end()))
}

/// The foot-of-cycle refusal restores the **whole expression**, not the cycle that misbehaved.
///
/// The E9 cell asserts the refusal is reported. This asserts what it reports *from*: the probe was
/// committed the instant the operator was accepted, so the restore can only come from the
/// expression-scoped guard, and an expression-scoped restore puts the front back at the token the
/// expression began with. A per-cycle guard restores to the cycle base — after this expression's
/// left-hand side — and hands the surrounding grammar the `^` instead.
#[test]
fn a_foot_of_cycle_refusal_restores_the_whole_expression() {
  let _g = measuring();
  let front: Option<(usize, usize)> = Parser::new()
    .apply(|inp: &mut Ir<'static, '_, _>| {
      Ok(front_after_settle(inp, |inp| {
        pratt(
          measured_lhs,
          measured_rhs,
          fold_prefix,
          rewinding_fold_infix,
          fold_postfix,
        )
        .parse_input(inp)
      }))
    })
    .parse_str("1 ^ 1 @")
    .expect("the probe converts the outcome into a report");

  assert_eq!(
    front,
    Some((0, 1)),
    "the foot-of-cycle refusal must restore to before the whole expression — offset 0. A \
     cycle-scoped restore leaves the front at the `^` (offset 2), and a settle that refused \
     before restoring leaves it wherever the rewinding fold put it"
  );
}

/// Consumes the `@` exactly as [`rewinding_fold_infix`] does, and first opens a
/// [session point](tokora::InputRef::begin_point) that it never settles.
///
/// Abandoning a point is legal and its contract is explicit: the progress is kept and the handle
/// releases the bookkeeping when it dies. What it also does, until then, is leave the point's base
/// **pinned** — and a pin is what a *checked* restore refuses to cross.
fn point_leaking_rewinding_fold_infix<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
  l: i64,
  r: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, u8>,
) -> Result<i64, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  // Opened here and never settled: the point stays on the stack, and its base stays pinned, for
  // the rest of the parse — including the moment `parse_input` settles the expression guard.
  let _ = inp.begin_point();
  let _ = inp.next()?;
  Ok(l + r)
}

/// A session point abandoned inside the expression must not turn the expression-scoped rollback
/// into a refusal.
///
/// This is the cell that discriminates *how* `Fault::Rewind` is settled. `Transaction::rollback`
/// is a **checked** restore: in every allocator build it panics — in release — when the rewind
/// would tear out a live pin, and it panics *before* restoring anything, so the damaged state is
/// still committed for any host that catches. `Transaction::rollback_abandoning_points` takes the
/// reconciling path instead: every point younger than the base is abandoned, and the restore then
/// subsumes it. That is the same path a rolling-back drop of this guard takes, which is why this
/// cell was already green when the settle was spelled `drop(txn)`.
///
/// The two are indistinguishable with no point live, which is why the fixture opens one. Red
/// against a settle that calls `rollback()` (the panic arrives with the wrong message, and the
/// front is wherever the rewinding fold left it).
#[test]
fn a_session_point_abandoned_in_a_fold_does_not_refuse_the_expression_rollback() {
  let _g = measuring();
  let front: Option<(usize, usize)> = Parser::new()
    .apply(|inp: &mut Ir<'static, '_, _>| {
      Ok(front_after_settle(inp, |inp| {
        pratt(
          measured_lhs,
          measured_rhs,
          fold_prefix,
          point_leaking_rewinding_fold_infix,
          fold_postfix,
        )
        .parse_input(inp)
      }))
    })
    .parse_str("1 ^ 1 @")
    .expect("the probe converts the outcome into a report");

  assert_eq!(
    front,
    Some((0, 1)),
    "an abandoned session point pins its base; the expression-scoped settle must reconcile it \
     and restore, not refuse to cross it. A settle through the checked `Transaction::rollback` \
     panics at the pin with nothing restored, leaving the front where the rewinding fold put it"
  );
}

thread_local! {
  /// Calls to let through before the armed RHS parser panics; `None` once it has fired, so the
  /// unwind cannot meet a second panic.
  static RHS_BOMB: Cell<Option<usize>> = const { Cell::new(None) };
}

/// [`measured_rhs`], with a bomb on the *n*-th call.
fn panicking_measured_rhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  match RHS_BOMB.with(|c| c.get()) {
    Some(0) => {
      RHS_BOMB.with(|c| c.set(None));
      panic!("the armed RHS parser panics");
    }
    Some(n) => RHS_BOMB.with(|c| c.set(Some(n - 1))),
    None => {}
  }
  measured_rhs(inp)
}

/// A panic raised inside `parse_pratt_rhs` restores the whole expression too.
///
/// The companion to `unwind_restores_the_whole_expression`, and the site that cell could not
/// reach: a fold panics with **no** guard of its own live, while the RHS parser panics inside the
/// operator *probe* guard, nested in a cycle the driver already committed. The unwind therefore
/// crosses two guards, and the answer must still be the outer one's — the probe's own restore
/// only reaches the top of this cycle.
///
/// The bomb is armed for the **second** RHS call, so the first `^` has been accepted, its probe
/// committed and its recursion entered before anything panics. A per-cycle driver restores to the
/// outermost live cycle's base, which is after the expression's left-hand side.
#[test]
fn an_rhs_parser_panic_restores_the_whole_expression() {
  let _g = measuring();
  let probe = |inp: &mut Ir<'static, '_, _>| -> Result<(bool, Option<(usize, usize)>), RetErr> {
    RHS_BOMB.with(|c| c.set(Some(1)));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      pratt(
        measured_lhs,
        panicking_measured_rhs,
        fold_prefix,
        fold_infix,
        fold_postfix,
      )
      .parse_input(inp)
    }));
    assert!(caught.is_err(), "the fixture needs the RHS parser to panic");
    let front = inp.next()?.map(|t| (t.span().start(), t.span().end()));
    Ok((caught.is_err(), front))
  };

  LHS_ENTRIES.store(0, Ordering::Relaxed);
  TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
  let (panicked, front): (bool, Option<(usize, usize)>) = Parser::new()
    .apply(probe)
    .parse_str("1 ^ 1 ^ 1")
    .expect("the probe converts the outcome into a report");

  assert!(panicked);
  assert_eq!(
    front,
    Some((0, 1)),
    "after an unwind through the RHS parser, the surrounding grammar must be handed back the \
     token the expression began with — offset 0. The probe guard alone restores only this \
     cycle's deciding read; the expression guard is what takes back the committed cycle in \
     front of it"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The emission log moves with the input — in both directions
// ═══════════════════════════════════════════════════════════════════════════════

/// One entry of [`CollectingEm`]'s unified log, tagged by the channel it arrived on.
///
/// Two channels, **one** log, because that is the shape the [`Emitter`] contract mandates and the
/// shape the contract on `Pratt` turns on: `Emitter::checkpoint` returns a single mark and
/// `Emitter::rewind` is required to restore *every* channel to it — diagnostics and the token
/// settles the CST channel is built from alike. An emitter that gave the two channels separate
/// marks would not be a compliant one, which is exactly why the driver cannot rewind one without
/// the other. The crate's own recording sink is this shape (a single event buffer, truncated to
/// the mark); this is that shape at fixture scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rec {
  /// A diagnostic, by its span start.
  Diag(usize),
  /// A committed token, by its span start — `Emitter::commit_token`, the auto-emission hook.
  Settle(usize),
}

thread_local! {
  /// Everything [`CollectingEm`] has recorded, in arrival order across both channels.
  ///
  /// Outside the emitter on purpose: the emitter is moved into the `ParserContext` and dies with
  /// the parse, and one half of the cell below reads the log *after* a debug assertion has
  /// unwound out of that parse.
  static LOG: RefCell<Vec<Rec>> = const { RefCell::new(Vec::new()) };
}

/// The diagnostic channel of the log, as span starts.
fn diags() -> Vec<usize> {
  LOG.with(|d| {
    d.borrow()
      .iter()
      .filter_map(|r| match r {
        Rec::Diag(at) => Some(*at),
        Rec::Settle(_) => None,
      })
      .collect()
  })
}

/// The token-settle channel of the log, as span starts — what a recording sink would have built
/// its tree from.
fn settles() -> Vec<usize> {
  LOG.with(|d| {
    d.borrow()
      .iter()
      .filter_map(|r| match r {
        Rec::Settle(at) => Some(*at),
        Rec::Diag(_) => None,
      })
      .collect()
  })
}

/// The crate's own collecting shape: the mark **is** the log length, and rewinding truncates to
/// it. Nothing clever — the point of the cell below is what the *driver* asks this emitter to do,
/// not what the emitter does with the request.
struct CollectingEm;

/// The lexer-error payload, the token and the span type, spelled as the trait spells them.
type LexErr<'inp> = <<CountLexer<'inp> as Lexer<'inp>>::Token as TokenT<'inp>>::Error;
type LexTok<'inp> = <CountLexer<'inp> as Lexer<'inp>>::Token;
type LexSpan<'inp> = <CountLexer<'inp> as Lexer<'inp>>::Span;

impl<'inp> Emitter<'inp, CountLexer<'inp>> for CollectingEm {
  type Error = RetErr;

  fn emit_lexer_error(&mut self, err: Spanned<LexErr<'inp>, LexSpan<'inp>>) -> Result<(), RetErr> {
    LOG.with(|d| d.borrow_mut().push(Rec::Diag(err.span().start())));
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, CountLexer<'inp>>,
  ) -> Result<(), RetErr> {
    Ok(())
  }

  fn emit_error(&mut self, err: Spanned<RetErr, LexSpan<'inp>>) -> Result<(), RetErr> {
    LOG.with(|d| d.borrow_mut().push(Rec::Diag(err.span().start())));
    Ok(())
  }

  fn commit_token(&mut self, _: &LexTok<'inp>, span: &LexSpan<'inp>) {
    LOG.with(|d| d.borrow_mut().push(Rec::Settle(span.start())));
  }

  fn checkpoint(&self) -> u64 {
    LOG.with(|d| d.borrow().len() as u64)
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, CountLexer<'inp>>, mark: u64) {
    LOG.with(|d| d.borrow_mut().truncate(mark as usize));
  }
}

thread_local! {
  /// Armed per parse: only the outermost left-hand side records, so the log below is a fact about
  /// the expression's own LHS and not about how many frames the chain has.
  static LHS_DIAG: Cell<bool> = const { Cell::new(false) };
}

/// [`measured_lhs`], plus one recoverable diagnostic from the outermost left-hand side — a real
/// complaint about the user's input, of the kind an LHS legitimately makes and the misbehaving
/// fold has nothing to do with.
fn diagnosing_lhs<'inp, Ctx>(inp: &mut Ir<'inp, '_, Ctx>) -> Result<PrattLHS<i64, (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  if LHS_DIAG.with(|c| c.replace(false)) {
    inp
      .emitter()
      .emit_error(Spanned::new(SimpleSpan::new(0, 1), RetErr))?;
  }
  measured_lhs(inp)
}

fn collecting_ctx() -> ParserContext<
  'static,
  CountLexer<'static>,
  CollectingEm,
  tokora::cache::DefaultCache<'static, CountLexer<'static>>,
> {
  ParserContext::new(CollectingEm)
}

/// The marker span start of the diagnostic the erasing half emits **before** entering the pratt
/// parser.
///
/// Deliberately not a position in this source: the contract's dividing line is *temporal* —
/// emitted before the parser was entered, or after — and a marker that cannot be confused with the
/// left-hand side's own diagnostic at offset 0 is what makes the two sides of that line separable
/// in one log.
const PRE_EXPR_DIAG: usize = 100;

/// Emits [`PRE_EXPR_DIAG`] through the input's emitter, before the pratt parser is entered.
///
/// A named helper rather than an inline call for the same reason [`diagnosing_lhs`] is one: the
/// `Ctx::Emitter` bound is what makes `emit_error` resolve to the one impl.
fn emit_pre_expression_diag<'inp, Ctx>(inp: &mut Ir<'inp, '_, Ctx>) -> Result<(), RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  inp.emitter().emit_error(Spanned::new(
    SimpleSpan::new(PRE_EXPR_DIAG, PRE_EXPR_DIAG + 1),
    RetErr,
  ))
}

/// The published contract on `Pratt`, under test on all three of its edges: **everything emitted
/// before the parser was entered survives, everything it emitted after does not, on the exits that
/// restore the whole expression — and every other exit keeps the lot.**
///
/// The expression-scoped restore rewinds the emitter to the mark `parse_input` took, so the
/// left-hand side's diagnostic and every token this expression settled go with the input they
/// described. That is deliberate and it is documented as a contract rather than a footnote, for
/// the reason this fixture's emitter demonstrates: `Emitter::checkpoint` hands out **one** mark
/// and `Emitter::rewind` is required to restore *every* channel to it, so the diagnostics and the
/// token settles the CST channel is built from cannot be rewound independently — and the mark
/// itself travels inside the same `Checkpoint` as the position, the lexer state and the dedup
/// watermarks. Emission scope is a function of rollback scope; the only way to discard less is to
/// retract less, and retracting less is the per-depth checkpoint retention this driver gave up.
///
/// Three halves, and each is load bearing:
///
/// * **kept** — the identical fixture on a *committing* exit. Without it an emitter that simply
///   never recorded anything would pass the erasing half;
/// * **erased** — both channels, because "the diagnostics went" and "the whole log was cleared"
///   are different facts and only the first is the contract;
/// * **bounded** — a diagnostic emitted before `parse_input` was ever called survives the same
///   restore. Without it the erasing half is equally consistent with an emitter the driver simply
///   truncates to zero, which would be a far larger and quite different loss.
#[test]
fn an_expression_rollback_takes_the_expressions_diagnostics_with_it() {
  let _g = measuring();

  // ── Kept: the expression commits, so everything it emitted commits with it. ──
  LOG.with(|d| d.borrow_mut().clear());
  LHS_DIAG.with(|c| c.set(true));
  LHS_ENTRIES.store(0, Ordering::Relaxed);
  TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
  let ok: Result<i64, RetErr> = Parser::with_context(collecting_ctx())
    .apply(|inp: &mut Ir<'static, '_, _>| {
      pratt(
        diagnosing_lhs,
        measured_rhs,
        fold_prefix,
        fold_infix,
        fold_postfix,
      )
      .parse_input(inp)
    })
    .parse_str("1 ^ 1");
  assert_eq!(
    ok.expect("the control fixture parses"),
    2,
    "the keeping half must actually parse, or it pins nothing"
  );
  assert_eq!(
    diags(),
    vec![0],
    "an expression that commits keeps every diagnostic it collected: the guard's commit releases \
     the emitter mark, it does not spend it"
  );
  assert_eq!(
    settles(),
    vec![0, 2, 4],
    "…and every token it settled, on the same mark and by the same release"
  );

  // ── Erased, and bounded: the expression is rolled back, so everything it emitted goes with it
  //    — and nothing that was already in the log when it started does. ──
  LOG.with(|d| d.borrow_mut().clear());
  LHS_DIAG.with(|c| c.set(true));
  let (log_at_settle, front) = Parser::with_context(collecting_ctx())
    .apply(|inp: &mut Ir<'static, '_, _>| {
      // Before the parser is entered: the surrounding grammar's own complaint, which this restore
      // has no business touching. It is what separates "rewound to the expression's mark" from
      // "cleared".
      emit_pre_expression_diag(inp).expect("the collecting emitter accepts");

      settle(inp, |inp| {
        pratt(
          diagnosing_lhs,
          measured_rhs,
          fold_prefix,
          rewinding_fold_infix,
          fold_postfix,
        )
        .parse_input(inp)
      });
      // Read the log BEFORE the front token — reading it is itself a token settle, and would land
      // in the very channel measured below.
      let log = LOG.with(|d| d.borrow().clone());
      Ok((log, front(inp)))
    })
    .parse_str("1 ^ 1 @")
    .expect("the probe converts the outcome into a report");

  assert_eq!(
    front,
    Some((0, 1)),
    "the erasing half must reach the same expression-scoped restore the cells above pin, or it \
     is measuring a different exit"
  );
  assert_eq!(
    log_at_settle,
    vec![Rec::Diag(PRE_EXPR_DIAG)],
    "the expression-scoped restore rewinds the emitter to the mark `parse_input` took: everything \
     the expression emitted — the left-hand side's diagnostic AND the tokens it settled — goes \
     with the input it described, and the diagnostic that was already in the log when the parser \
     was entered stays. Preserving the expression's own emissions instead is not a smaller loss: \
     the dedup watermarks travel in the same checkpoint, so a re-parse of the retracted region \
     would emit its diagnostics a second time on top of the preserved copies — and the token \
     settles cannot be held back at all, since one emitter mark covers both channels. Got {:?}",
    log_at_settle
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// …and it takes them on TWO exits, in EVERY build — the third rollback that must not exist
// ═══════════════════════════════════════════════════════════════════════════════
//
// The cell above pins what an expression-scoped restore takes. These three pin how many exits
// take it, which is the other half of the same sentence: "on the two exits that restore the whole
// expression, and on those two only".
//
// WHAT WOULD BREAK IT. The expression guard is `Rollback` policy and is undecided for the whole of
// the driver's own `parse`, so ANY panic raised in there — including one the driver raises itself —
// is an expression-scoped restore. Three exits report a "consume what you report" violation and
// each of them has a `debug_assert` attached: the prefix stall, and the report-boundary stall in
// the `Postfix` and `Infix` arms. Raise those assertions where the violation is detected and a
// debug build unwinds through the undecided guard and erases the whole expression, on the exact
// inputs where a release build keeps it and returns the terminal error — a third whole-expression
// rollback, in one profile only, undocumented because it is invisible from the release side.
//
// The same is true of the terminal error itself, one step quieter and in BOTH profiles: building it
// clones an `L::Offset` and runs the grammar's `From`, and a panicking `Offset::clone` is caller
// code this repository treats as reachable. So the driver carries the offsets and the channel out
// in its `Fault` — data only, no error — and the wrapper commits, then asserts, then builds. These
// cells cannot reach the clone edge (no fixture can make a `usize` clone panic), so what they pin
// is the settle each posture takes; the construction ordering is pinned by `Fault` carrying no `E`
// and `parse` carrying no `From<UnexpectedEo…>` bound, which is a compile-time fact.
//
// WHAT THESE CELLS ASSERT. The observables the contract is written in — the emission log and the
// token the surrounding grammar is handed next — with the SAME expected values on both sides of
// the profile split. The split is confined to how the violation arrives (a panic in debug, the
// terminal error in release) and each side is required to arrive that way, so neither can pass
// vacuously. Red against the assertion moved back inside `parse`: the debug run reports the log as
// `[Diag(PRE_EXPR_DIAG)]` — the left-hand side's own diagnostic and its token settle erased — while
// the release run reports all three entries, which is precisely the divergence.

/// Reports a `Prefix` having consumed nothing — the LHS report-boundary stall — after emitting one
/// diagnostic of its own.
///
/// The diagnostic is what makes the cell discriminate at all. The stall's own precondition is that
/// committed consumption did not advance, so the input position is the same whether the expression
/// is kept or rolled back; the emission log is not.
fn diagnosing_stalled_prefix_lhs<'inp, Ctx>(
  inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattLHS<i64, (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  if LHS_DIAG.with(|c| c.replace(false)) {
    inp
      .emitter()
      .emit_error(Spanned::new(SimpleSpan::new(0, 1), RetErr))?;
  }
  Ok(PrattLHS::Prefix(Precedenced::new((), CARET)))
}

/// Reports an admitted `Postfix` having consumed nothing — the report-boundary stall in the
/// `Postfix` arm. No session point: [`point_leaking_stalled_postfix_rhs`] is the settle-verb
/// fixture, this one is the profile-divergence fixture.
fn stalled_postfix_rhs<'inp, Ctx>(
  _inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  Ok(PrattRHS::Postfix(Precedenced::new((), CARET)))
}

/// The `Infix` twin of [`stalled_postfix_rhs`] — the other report-boundary stall, and the other
/// site the assertion had to be lifted out of.
fn stalled_infix_rhs<'inp, Ctx>(
  _inp: &mut Ir<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), u8>, RetErr>
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
{
  Ok(PrattRHS::Infix(Precedenced::new(
    PrattInfix::Right(()),
    CARET,
  )))
}

/// Drives `body` onto a report-boundary stall and reports the emission log and the front token,
/// having first required the violation to arrive the way the profile says it should.
///
/// The sibling of [`settle`], and deliberately stricter in the one place it matters. `settle`
/// *tolerates* a debug panic because the exits it serves restore either way; here the panic is the
/// thing under test, so a debug build that does not panic has lost the assertion entirely and the
/// cell would then pin nothing about where it fires. A release build that panics has had a settle
/// refuse instead of restore, which is the failure mode the sibling names.
///
/// The log is read **before** the front token, for the reason the diagnostics cell reads it there:
/// a front read is itself a token settle and would land in the channel being measured.
fn stall_outcome<'inp, Ctx, F>(
  inp: &mut Ir<'inp, '_, Ctx>,
  body: F,
) -> (Vec<Rec>, Option<(usize, usize)>)
where
  Ctx: ParseContext<'inp, CountLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CountLexer<'inp>, Error = RetErr>,
  F: FnOnce(&mut Ir<'inp, '_, Ctx>) -> Result<i64, RetErr>,
{
  LHS_ENTRIES.store(0, Ordering::Relaxed);
  TARGET_LHS_ENTRY.store(0, Ordering::Relaxed);
  let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(inp)));
  // Matched against the profile rather than asserted about it: `cfg!` is a constant, and an
  // `assert!` over one is a lint. The four arms are the whole truth table, and only two of them
  // are the contract.
  match (&outcome, cfg!(debug_assertions)) {
    // Debug: the driver's own "consumed nothing" assertion, raised in the wrapper once the
    // expression guard has committed.
    (Err(_), true) => {}
    // Release: no assertion exists, so the violation must arrive as the terminal error.
    (Ok(r), false) => assert!(
      r.is_err(),
      "release: a report-boundary stall is the terminal error, never a truncated `Ok` — got {r:?}"
    ),
    (Ok(r), true) => panic!(
      "a debug build must reach the driver's own 'consumed nothing' assertion — got {r:?}. \
       Without it this cell measures the release path twice and pins nothing about where that \
       assertion fires"
    ),
    (Err(_), false) => panic!(
      "a release build has no assertion to raise on this exit, so an unwind out of it means a \
       settle refused rather than restored"
    ),
  }
  let log = LOG.with(|d| d.borrow().clone());
  (log, front(inp))
}

/// One report-boundary stall, measured on both channels of the log and on the handback, with the
/// expectations outside the profile split.
macro_rules! stall_keeps_the_expression_cell {
  ($name:ident, $lhs:expr, $rhs:expr, $log:expr, $front:expr, $why:literal) => {
    #[test]
    fn $name() {
      let _g = measuring();
      LOG.with(|d| d.borrow_mut().clear());
      LHS_DIAG.with(|c| c.set(true));
      let (log, front) = Parser::with_context(collecting_ctx())
        .apply(|inp: &mut Ir<'static, '_, _>| {
          // The surrounding grammar's own complaint, emitted before the parser is entered: the
          // bound that separates "kept the expression" from "cleared the log".
          emit_pre_expression_diag(inp).expect("the collecting emitter accepts");
          Ok(stall_outcome(inp, |inp| {
            pratt($lhs, $rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
          }))
        })
        .parse_str("1 ^ 1")
        .expect("the probe converts the outcome into a report");

      assert_eq!(log, $log, "{}. Got {:?}", $why, log);
      assert_eq!(
        front, $front,
        "{}. The handback must be the committed one in both profiles too — got {:?}",
        $why, front
      );
    }
  };
}

stall_keeps_the_expression_cell!(
  a_stalled_prefix_report_keeps_the_expression_in_both_profiles,
  diagnosing_stalled_prefix_lhs,
  measured_rhs,
  vec![Rec::Diag(PRE_EXPR_DIAG), Rec::Diag(0)],
  Some((0, 1)),
  "the prefix stall precedes the recursion, the fold and the wrap, and by its own precondition \
   consumed nothing — so it restores nothing and the driver has no business erasing what the LHS \
   parser emitted on its way to the violation. A `debug_assert` raised at the detection site \
   erases exactly that diagnostic, in debug only"
);

stall_keeps_the_expression_cell!(
  a_stalled_postfix_report_keeps_the_expression_in_both_profiles,
  diagnosing_lhs,
  stalled_postfix_rhs,
  vec![Rec::Diag(PRE_EXPR_DIAG), Rec::Diag(0), Rec::Settle(0)],
  Some((2, 3)),
  "the postfix report-boundary stall rolls back THE CYCLE — the deciding read, nothing else — so \
   the expression's left-hand side, its diagnostic and its token settle all survive, and the `^` \
   the stalled report declined to consume is handed on. A `debug_assert` raised before the return \
   unwinds through the expression guard instead and takes all three, in debug only"
);

stall_keeps_the_expression_cell!(
  a_stalled_infix_report_keeps_the_expression_in_both_profiles,
  diagnosing_lhs,
  stalled_infix_rhs,
  vec![Rec::Diag(PRE_EXPR_DIAG), Rec::Diag(0), Rec::Settle(0)],
  Some((2, 3)),
  "the infix report-boundary stall is the same exit one arm over, and the same assertion had to \
   be lifted out of it: cycle-scoped restore, expression kept, `^` handed on, in both profiles"
);

// ═══════════════════════════════════════════════════════════════════════════════
// The cell that is missing, and why — the crossing rewind
// ═══════════════════════════════════════════════════════════════════════════════
//
// `the_crossing_rewind_this_suite_could_not_reach`. There is no `#[test]` under this heading, and
// that is deliberate: a fabricated assertion here would be worse than the gap.
//
// WHAT WOULD BE PINNED. A transaction guard pins its begin point, and the pin check is
// release-active in every allocator build: a checked restore to a target below a live pin panics
// at the restore that caused it. The old cycle guard pinned the cycle base across classify, the
// recursion, the fold and the CST wrap, so a hook that rewound to a target between the expression
// base and the cycle base was refused loudly. The narrowing commits the probe before that window
// opens, so within it that rewind now succeeds silently — the one dimension in which the change is
// *narrower* rather than wider, written up on the `Pratt` type doc. A cell pinning the new
// behaviour, and a second cell pinning the conditional `EventMark`-era backstop under
// `with_cst_kinds` plus a recording sink, both belong here.
//
// WHY NEITHER IS WRITTEN. A crossing rewind cannot be expressed from outside the crate, before the
// change or after it. Every primitive that moves committed consumption backwards is one of two
// shapes:
//
//   * lexically scoped to the call that opened it — `begin`, `begin_with`, `attempt`,
//     `try_attempt`, `begin_stacked` and its savepoints. The furthest back any of these reaches is
//     the entry of the hook that opened it, and every pratt hook's entry is at or above the
//     current cycle's base. So none of them can cross.
//   * named by a token branded with the input handle's `'closure` lifetime — `SessionPointId` for
//     `begin_point`/`rollback_point`, `Checkpoint` for `save`/`restore` under `unstable-raw`. The
//     brand is *invariant* (a `PhantomData<fn(&'closure ()) -> &'closure ()>`), and every pratt
//     hook reaches the driver through `ParseInput`, whose method takes
//     `&mut InputRef<'inp, '_, …>` — an elided lifetime in `FnMut(…)` sugar, i.e. **higher-ranked**.
//     So a hook is instantiated at every `'closure`, and no value it can hold across calls may
//     mention one.
//
// Both were checked against the compiler rather than reasoned about. A `'static` thread-local
// holding a `SessionPointId` fails with `E0521: borrowed data escapes outside of function …
// argument requires that `'inp` must outlive `'static``. Closures sharing a
// `Cell<Option<SessionPointId<'c>>>` for a named `'c` — the shape that would otherwise work —
// compile individually and then fail at the call site with `E0599: no method named `parse_input`
// found for struct `Pratt<…>``, because a closure fixed at one `'c` does not satisfy the
// higher-ranked bound.
//
// CONSEQUENCE. The pin narrowing is real as a code-shape fact and is documented as one, but this
// suite cannot demonstrate it, and by the same argument neither can any downstream grammar trigger
// it. If the input layer ever grows a rewind verb that is neither lexically scoped nor branded —
// or if the pratt hooks stop being higher-ranked over `'closure` — the two cells described above
// become writable and should be written in the same commit.
