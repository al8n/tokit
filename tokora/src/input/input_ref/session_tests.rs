//! Tests for [`InputRef`](super::InputRef) session points — the non-lexical form of speculation.
//!
//! [`begin_point`](super::InputRef::begin_point) saves a checkpoint onto the input's point stack
//! and pins it like a guard base; [`commit_point`](super::InputRef::commit_point) keeps the
//! progress and [`rollback_point`](super::InputRef::rollback_point) returns to it, newest-first.
//! Because a point is a value on the input rather than a borrow *of* it, the consume surface stays
//! callable while one is open — so unlike the guard suites these tests speculate over **real token
//! consumption**, and every rollback is watched to put the tokens back. The other facts a
//! checkpoint carries (lexer state, emission log, dedup watermark, poison boundary) ride along and
//! are asserted beside the cursor.

use crate::{
  Emitter, Token,
  cache::DefaultCache,
  emitter::{Silent, Verbose},
  error::token::UnexpectedToken,
  input::Input,
  lexer::LogosLexer,
  span::{SimpleSpan, Spanned},
  state::token_tracker::{TokenLimitExceeded, TokenLimiter},
};

// ── Fixture: a number lexer over a by-value token limiter (as in the guard tests) ──────────────
//
// The by-value `TokenLimiter` travels inside the lexer state, so a session point taken before a
// limit trip saves a clean count and rolling back to it un-trips.

#[derive(Debug, Clone, PartialEq)]
enum NumErr {
  Lex,
  Limit,
}

impl From<()> for NumErr {
  fn from(_: ()) -> Self {
    NumErr::Lex
  }
}

impl From<TokenLimitExceeded> for NumErr {
  fn from(_: TokenLimitExceeded) -> Self {
    NumErr::Limit
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for NumErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    NumErr::Lex
  }
}

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = TokenLimiter, skip r"[ \t\r\n]+")]
enum NumTok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); })]
  Num,
}

impl core::fmt::Display for NumTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NumKind {
  Num,
}

impl core::fmt::Display for NumKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl Token<'_> for NumTok {
  type Kind = NumKind;
  type Error = NumErr;

  const READ_FRONTIER_CLASS: crate::ReadFrontierClass = crate::ReadFrontierClass::Unbounded;

  fn kind(&self) -> NumKind {
    NumKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type NumLexer<'a> = LogosLexer<'a, NumTok>;
type NumCtx<'a> = (Silent<NumErr>, DefaultCache<'a, NumLexer<'a>>);
type NumVerboseCtx<'a> = (Verbose<NumErr>, DefaultCache<'a, NumLexer<'a>>);

/// The input reference the session runs on, over the verbose (collecting) context.
type VerboseIr<'inp, 'closure> =
  super::InputRef<'inp, 'closure, NumLexer<'inp>, NumVerboseCtx<'inp>>;

/// Builds a `Silent` input over `src` with a limit high enough never to trip.
fn silent_input(src: &str) -> Input<'_, NumLexer<'_>, NumCtx<'_>, ()> {
  let context = crate::input::InputContext::new(
    Silent::<NumErr>::new(),
    DefaultCache::<'_, NumLexer<'_>>::default(),
  );
  Input::<NumLexer<'_>, NumCtx<'_>, ()>::with_state_and_context(
    src,
    TokenLimiter::with_limitation(usize::MAX),
    context,
  )
}

/// Builds a `Verbose` input over `src` with a limit high enough never to trip.
fn verbose_input(src: &str) -> Input<'_, NumLexer<'_>, NumVerboseCtx<'_>, ()> {
  let context = crate::input::InputContext::new(
    Verbose::<NumErr>::new(),
    DefaultCache::<'_, NumLexer<'_>>::default(),
  );
  Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
    src,
    TokenLimiter::with_limitation(usize::MAX),
    context,
  )
}

/// Emits an application diagnostic through the input's emitter (the lexer type pins the blanket
/// `Verbose: Emitter<L>` impl, exactly as the emitter unit tests do).
fn emit(ir: &mut VerboseIr<'_, '_>, at: usize, err: NumErr) {
  let span = SimpleSpan::new(at, at + 1);
  <Verbose<NumErr> as Emitter<'_, NumLexer<'_>>>::emit_error(ir.emitter(), Spanned::new(span, err))
    .expect("Verbose is a non-fatal emitter");
}

/// The number of diagnostics currently retained by the input's emitter.
fn diag_count(ir: &mut VerboseIr<'_, '_>) -> usize {
  ir.emitter().errors().values().map(|g| g.len()).sum()
}

/// Consumes one token, asserting it is there, and returns its source text.
fn take<'inp>(ir: &mut VerboseIr<'inp, '_>) -> &'inp str {
  ir.next()
    .expect("complete + non-fatal")
    .expect("a token is there");
  ir.slice()
}

// ── 1. Both verbs settle a point that CONSUMED TOKENS ──────────────────────────────────────────

#[test]
fn session_point_commit_keeps_consumed_tokens() {
  // Commit keeps the speculative work done through the point — including the tokens it consumed
  // across separate calls, which is the work a `ParseState` could never have done.
  let mut input = verbose_input("1 2 3 4");
  let mut ir = input.as_ref();

  let point = ir.begin_point();
  assert_eq!(ir.points(), 1, "the point is live");

  // Real parsing, through the open point, one separate call at a time.
  assert_eq!(take(&mut ir), "1");
  assert_eq!(take(&mut ir), "2");
  let after_two = *ir.cursor().as_inner();
  emit(&mut ir, 0, NumErr::Lex);

  ir.commit_point(point);
  assert_eq!(ir.points(), 0, "the point settled");
  assert_eq!(
    *ir.cursor().as_inner(),
    after_two,
    "commit keeps the consumed tokens: the cursor stays past `2`"
  );
  assert_eq!(
    diag_count(&mut ir),
    1,
    "commit keeps the emitted diagnostic"
  );
  // The stream resumes where the committed work left it.
  assert_eq!(take(&mut ir), "3", "the next token is the one after `2`");
}

#[test]
fn session_point_rollback_puts_the_tokens_back() {
  // THE capability: mark, consume several tokens across separate calls, emit, then roll back —
  // and the cursor, the token stream, and the emission log all return to the mark.
  let mut input = verbose_input("1 2 3 4");
  let mut ir = input.as_ref();

  assert_eq!(take(&mut ir), "1", "committed work before the session");
  let mark = *ir.cursor().as_inner();

  let point = ir.begin_point();
  assert_eq!(take(&mut ir), "2");
  emit(&mut ir, 2, NumErr::Lex);
  assert_eq!(take(&mut ir), "3");
  assert_eq!(diag_count(&mut ir), 1, "a speculative diagnostic");
  assert_ne!(
    *ir.cursor().as_inner(),
    mark,
    "the session moved the cursor"
  );

  ir.rollback_point(point);
  assert_eq!(ir.points(), 0, "the point settled");
  assert_eq!(
    *ir.cursor().as_inner(),
    mark,
    "rollback returned the cursor to the mark"
  );
  assert_eq!(
    diag_count(&mut ir),
    0,
    "rollback dropped the speculative diagnostic"
  );
  // The tokens are genuinely back on the stream: the abandoned `2` is next again.
  assert_eq!(take(&mut ir), "2", "the rewound token re-lexes");
  assert_eq!(take(&mut ir), "3");
  assert_eq!(take(&mut ir), "4");
  assert!(
    ir.next().expect("complete + non-fatal").is_none(),
    "and then the stream ends"
  );
}

#[test]
fn session_point_rollback_restores_state_and_poison() {
  // The non-cursor facts a checkpoint carries. The point is taken at a limit-tripped position
  // (poison latched, the limit diagnostic emitted, the watermark lifted); state surgery through
  // the point re-keys those forward-scanning facts away and a speculative diagnostic is emitted;
  // the rollback returns state, poison, watermark, position, and the emission log to the trip.
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    crate::input::InputContext::new(Verbose::<NumErr>::new(), cache),
  );
  {
    let mut ir = input.as_ref();

    // Trip the limiter: two tokens, then a poisoned `None`; the limit diagnostic emits once.
    assert!(ir.next().unwrap().is_some(), "1");
    assert!(ir.next().unwrap().is_some(), "2");
    assert!(ir.next().unwrap().is_none(), "the 3rd scan trips → None");
    let tripped = *ir.cursor().as_inner();

    let point = ir.begin_point(); // saves the tripped lineage: state, poison, watermark, mark
    assert_eq!(ir.points(), 1);

    // Speculative work through the point: re-key the regime (dropping poison, resetting the
    // watermark) and emit a fresh diagnostic on top of the trip's.
    *ir.state_mut() = TokenLimiter::with_limitation(usize::MAX);
    emit(&mut ir, 0, NumErr::Lex);
    assert_eq!(ir.state().limitation(), usize::MAX, "the re-key took");
    assert_eq!(
      diag_count(&mut ir),
      2,
      "the speculative diagnostic joined the trip's"
    );
    // The re-key lifted the poison, so the stream flows again — real work, rolled back below.
    assert!(
      ir.next().unwrap().is_some(),
      "the un-poisoned stream yields again"
    );

    ir.rollback_point(point);
    assert_eq!(ir.points(), 0, "the point settled");
    assert_eq!(
      ir.state().limitation(),
      2,
      "state: the pre-surgery regime returned"
    );
    assert_eq!(
      ir.state().tokens(),
      2,
      "state: the saved token count returned"
    );
    assert_eq!(
      diag_count(&mut ir),
      1,
      "diagnostics: the speculative one was rolled back, the trip's kept"
    );
    assert_eq!(
      *ir.cursor().as_inner(),
      tripped,
      "position rolled back to the trip"
    );
  }

  // The input now sits at the restored (tripped) lineage: the restored poison boundary stops the
  // stream, and the limit diagnostic is retained exactly once — never duplicated by the rollback.
  {
    let mut ir = input.as_ref();
    assert!(
      ir.next().unwrap().is_none(),
      "the restored poison boundary stops the stream"
    );
  }
  let total: usize = input.emitter().errors().values().map(|g| g.len()).sum();
  assert_eq!(
    total, 1,
    "the limit diagnostic is retained exactly once across the session"
  );
}

// ── 2. Nesting is last-in, first-out ─────────────────────────────────────────────────────────

#[test]
fn session_points_nest_lifo() {
  // Three nested points, each opened after consuming one more token. Roll back the newest, commit
  // the middle, roll back the oldest — the stream stays faithful: the middle commit keeps the
  // current position but does not disturb the oldest point's saved one, reached by the final
  // rollback.
  let mut input = verbose_input("1 2 3 4 5");
  let mut ir = input.as_ref();

  assert_eq!(take(&mut ir), "1");
  let at1 = *ir.cursor().as_inner();
  let p1 = ir.begin_point(); // P1 marks "after 1"

  assert_eq!(take(&mut ir), "2");
  let p2 = ir.begin_point(); // P2 marks "after 2"

  assert_eq!(take(&mut ir), "3");
  let at3 = *ir.cursor().as_inner();
  let p3 = ir.begin_point(); // P3 marks "after 3"

  assert_eq!(take(&mut ir), "4");
  assert_eq!(ir.points(), 3, "three live points");

  ir.rollback_point(p3); // newest: back to P3's mark
  assert_eq!(ir.points(), 2);
  assert_eq!(
    *ir.cursor().as_inner(),
    at3,
    "rolled back to the newest point's mark"
  );

  ir.commit_point(p2); // middle: keep the current position, release the point
  assert_eq!(ir.points(), 1);
  assert_eq!(
    *ir.cursor().as_inner(),
    at3,
    "commit keeps the current position"
  );

  ir.rollback_point(p1); // oldest: back to P1's mark, unaffected by the middle commit
  assert_eq!(ir.points(), 0);
  assert_eq!(
    *ir.cursor().as_inner(),
    at1,
    "rolled back to the oldest point's mark — a faithful stream"
  );
  assert_eq!(take(&mut ir), "2", "and the stream replays from there");
}

// ── 3. Misuse panics with the documented prefix ──────────────────────────────────────────────

#[test]
#[should_panic(expected = "no live session point")]
fn session_point_commit_misuse_panics() {
  let mut input = silent_input("1 2 3");
  let mut ir = input.as_ref();

  // Settling twice with the same id: the second call finds nothing open at all.
  let point = ir.begin_point();
  ir.commit_point(point);
  ir.commit_point(point); // zero live points → panic
}

#[test]
#[should_panic(expected = "no live session point")]
fn session_point_rollback_misuse_panics() {
  let mut input = silent_input("1 2 3");
  let mut ir = input.as_ref();

  let point = ir.begin_point();
  ir.rollback_point(point);
  ir.rollback_point(point); // zero live points → panic
}

// ── 3b. The id is a name, not a position ──────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "session point settled out of order")]
fn settling_an_older_point_while_a_younger_one_is_open_is_refused() {
  // Naming the outer point while the inner one is still open. Settling by position would have
  // taken the INNER point and silently applied the outer's intent to it — the shifted-target
  // class the id exists to close.
  let mut input = silent_input("1 2 3 4");
  let mut ir = input.as_ref();

  let outer = ir.begin_point();
  let _ = ir.next().unwrap().expect("1");
  let _inner = ir.begin_point();
  ir.rollback_point(outer);
}

#[test]
#[should_panic(expected = "stale session point")]
fn settling_an_already_settled_point_under_a_live_one_is_refused() {
  // The mirror: an id whose point is gone, while an unrelated point is open. A positional settle
  // would have spent the live one.
  let mut input = silent_input("1 2 3 4");
  let mut ir = input.as_ref();

  let first = ir.begin_point();
  ir.commit_point(first);
  let _second = ir.begin_point();
  ir.commit_point(first);
}

#[test]
fn a_reissued_point_never_reuses_a_settled_point_s_identity() {
  // Non-positional AND non-reused: a fresh point opened at the same depth, and even at the same
  // position, is a different point. The checkpoint-id source is monotone and never reset, so a
  // stale id can never be mistaken for the fresh one occupying its old slot.
  let mut input = silent_input("1 2 3 4");
  let mut ir = input.as_ref();

  let first = ir.begin_point();
  ir.commit_point(first);
  let second = ir.begin_point();
  assert_ne!(
    first, second,
    "a reopened point at the same depth is a distinct identity"
  );
  ir.commit_point(second);
  assert_eq!(ir.points(), 0);
}

// ── 3c. The id names its INPUT, not merely its point ──────────────────────────────────────────
//
// The `'closure` brand separates the handles one parser can hold, with a single residual: two
// inputs borrowed in one scope share that brand region, so the compiler unifies them and one
// input's id type-checks against the other's handle. A checkpoint id is unique only *within* an
// input — every input numbers from the same start — so the id also carries the address of its
// input's own poison-boundary slot, and a settle refuses a mismatch before it scans anything.
//
// That address is taken through the handle's borrow, so it names the slot inside the `Input`
// rather than the field inside the handle. The distinction is what keeps the refusal from firing
// on the handle's own reborrows and moves, which the two tests below hold it to.

#[test]
#[should_panic(expected = "foreign session point")]
fn settling_a_point_from_another_input_is_refused() {
  // Both handles are taken *before* either point opens, so the two brand regions coincide and
  // passing input A's id to input B type-checks. (Taking B's handle after A's point would instead
  // be a compile error — the brand's own half of the job.) Both inputs are fresh, so the two
  // points carry the *same* checkpoint id: the live-point scan alone would find a genuine match
  // and settle B's own point under A's intent.
  let mut input_a = silent_input("1 2 3 4");
  let mut input_b = silent_input("1 2 3 4");

  let mut ir_a = input_a.as_ref();
  let mut ir_b = input_b.as_ref();

  let point_a = ir_a.begin_point();
  let point_b = ir_b.begin_point();
  assert_eq!(
    point_a.ckp(),
    point_b.ckp(),
    "two fresh inputs number their checkpoints from the same start — the collision the nonce \
     exists to catch"
  );

  ir_b.commit_point(point_a);
}

/// Settles `point` behind a plain `&mut` reborrow of the handle — the shape every nested parser
/// call takes.
fn settle_behind_a_reborrow<'closure>(
  ir: &mut VerboseIr<'_, 'closure>,
  point: super::SessionPointId<'closure>,
) {
  ir.commit_point(point);
}

/// Settles `point` on a handle **moved** into this frame, and drops the handle here. The id
/// remembers a slot in the `Input`, which the move cannot relocate — the handle's own storage is
/// not what it names.
fn settle_on_a_moved_handle<'closure>(
  mut ir: VerboseIr<'_, 'closure>,
  point: super::SessionPointId<'closure>,
) {
  ir.commit_point(point);
}

#[test]
fn a_point_settles_across_the_handle_s_reborrows() {
  // A point is opened in one call and settled in another, so between the two the handle is passed
  // onward every way the crate allows. Each of these settles is legal and must be honored; an
  // input identity that changed with the handle would turn all three into false refusals.
  let mut input = verbose_input("1 2 3 4 5 6");
  let mut ir = input.as_ref();

  // (a) A `&mut` reborrow into a nested parser, with the id handed along beside it.
  let reborrowed = ir.begin_point();
  assert_eq!(take(&mut ir), "1");
  settle_behind_a_reborrow(&mut ir, reborrowed);
  assert_eq!(ir.points(), 0, "the settle behind a reborrow was honored");

  // (b) A guard's `DerefMut`: opened and settled through the guard, which reaches the same handle
  //     through a `&mut` of its own.
  {
    let mut guard = ir.begin();
    let through_guard = guard.begin_point();
    assert_eq!(take(&mut guard), "2");
    guard.commit_point(through_guard);
    guard.commit();
  }
  assert_eq!(ir.points(), 0, "the settle through a guard was honored");

  // (c) Held across an `attempt`, whose closure receives the handle reborrowed through the
  //     attempt's own guard — so the point outlives a reborrow it was not opened on.
  let across = ir.begin_point();
  let attempted: Option<()> = ir.attempt(|ir| {
    assert_eq!(take(ir), "3");
    Some(())
  });
  assert!(attempted.is_some(), "the attempt kept its progress");
  ir.commit_point(across);
  assert_eq!(ir.points(), 0, "the settle across an attempt was honored");
}

#[test]
fn a_point_settles_on_a_handle_that_moved() {
  // The handle is moved out of this frame with the point still open. Its fields travel with it,
  // the `Input`'s do not — so a settle after the move is legal, and the id must still resolve.
  let mut input = verbose_input("1 2 3");
  {
    let mut ir = input.as_ref();
    let point = ir.begin_point();
    assert_eq!(take(&mut ir), "1");
    settle_on_a_moved_handle(ir, point);
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "the commit through the moved handle released the point's pin"
  );
  assert_eq!(
    input.live_checkpoints_len(),
    0,
    "…and its live-checkpoint lineage entry"
  );
}

// ── 4. A session point pins its base ─────────────────────────────────────────────────────────

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn session_point_is_pinned() {
  // A rewind below a live session point's base tears the session's foundation out. `begin_point`
  // pins that base (like a guard), so the checked restore panics AT the restore with the existing
  // pin message — detect-at-cause, in every allocator build.
  let mut input = silent_input("1 2 3 4 5");
  let mut ir = input.as_ref();

  let a = ir.save(); // raw checkpoint, below the session point
  let _ = ir.next().unwrap().expect("consume 1"); // advance past A
  let _point = ir.begin_point(); // pins the base, above A
  ir.restore(a); // panics: restoring A would pop the still-pinned base off the lineage
}

// ── 4b. A guard that rolls back on DROP reconciles the points opened inside it ────────────────
//
// The pin makes a rewind below an open point panic where it is *requested* — but only on the
// checked `restore`. A guard's rolling-back drop rewinds through the unchecked path (it may run
// mid-unwind, where a panic is forbidden), so it cannot refuse: it has to reconcile instead,
// abandoning every point younger than its base before rewinding below them.

#[test]
fn guard_drop_rollback_reconciles_the_open_session_point() {
  // `Rollback` is `begin`'s default policy, so this is the shape every `?` early-return through
  // a guard scope takes. The point opened inside the guard describes a lineage the rollback
  // destroys; leaving it on the stack would let a later settle rewind to a timeline that no
  // longer exists.
  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _ = ir.next().unwrap().expect("committed work before the guard");
    let before = *ir.cursor().as_inner();
    let baseline = ir.live_checkpoints_len();

    {
      let mut guard = ir.begin();
      let _point = guard.begin_point();
      let _ = guard
        .next()
        .unwrap()
        .expect("speculative work through the point");
      // …and the guard is dropped undecided: rollback-on-drop, below the open point.
    }

    assert_eq!(
      ir.points(),
      0,
      "the rollback abandoned the point it invalidated"
    );
    assert_eq!(
      ir.live_checkpoints_len(),
      baseline,
      "…and released its lineage entry"
    );
    assert_eq!(
      *ir.cursor().as_inner(),
      before,
      "the guard still rolled back to its own base"
    );
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "…and its pin: the pin set holds exactly the live begin points"
  );
}

#[test]
#[should_panic(expected = "no live session point")]
fn settling_a_point_a_guard_drop_reconciled_is_refused() {
  // The flip side of the reconciliation: the point is *gone*, so settling afterwards is refused
  // by the session verb itself. Before the reconciliation the stale entry was still on the stack
  // and the settle rewound to a dead lineage — caught, in debug builds only, as a non-LIFO
  // restore at the settle rather than at the cause.
  let mut input = silent_input("1 2 3 4 5");
  let mut ir = input.as_ref();
  let point = {
    let mut guard = ir.begin();
    let point = guard.begin_point();
    let _ = guard.next().unwrap().expect("1");
    point
  };
  ir.rollback_point(point);
}

#[test]
fn attempt_panic_with_an_open_point_leaves_no_stranded_point() {
  // The unwind twin: `attempt` holds its begin point in a rolling-back guard precisely so a
  // panic out of user code settles it, and that settle must reconcile a point the closure left
  // open. A caught panic hands the host an input with nothing stranded on its behalf.
  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let baseline = ir.live_checkpoints_len();

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let _: Option<()> = ir.attempt(|ir| {
        let _point = ir.begin_point();
        let _ = ir.next().unwrap().expect("1");
        panic!("the attempt's closure unwinds")
      });
    }));
    assert!(caught.is_err(), "the panic unwound out of the attempt");

    assert_eq!(
      ir.points(),
      0,
      "the unwind's rollback reconciled the point opened inside it"
    );
    assert_eq!(
      ir.live_checkpoints_len(),
      baseline,
      "…and left no live lineage entry behind"
    );
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "…and nothing pinned on the caught panic's behalf"
  );
}

// ── 4c. The explicit reconciling rollback ────────────────────────────────────────────────────
//
// 4b's reconciliation was reachable only by *dropping* an undecided guard, which is a posture and
// not a statement. A scope whose exits are "fall out of the block" and "roll back right here"
// could express the first and not the second, so the second reached for `rollback()` — and turned
// a legal abandoned point into a release panic on an ordinary exit. `rollback_abandoning_points`
// is 4b's reconciliation on the explicit path. `rollback` keeps its refusal: the two are for
// different owners of the points, not for different tastes.

#[test]
fn the_explicit_reconciling_rollback_abandons_the_open_session_point() {
  // `Commit` policy on purpose: this is the shape at the call site the verb was added for (a
  // commit-by-default probe that rolls back explicitly on its few decline exits), and it proves
  // the verb is available whatever the policy — the drop that reconciles is not.
  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _ = ir.next().unwrap().expect("committed work before the guard");
    let before = *ir.cursor().as_inner();
    let baseline = ir.live_checkpoints_len();

    {
      let mut guard = ir.begin_with::<super::Commit>();
      let _point = guard.begin_point();
      let _ = guard
        .next()
        .unwrap()
        .expect("speculative work through the point");
      guard.rollback_abandoning_points();
    }

    assert_eq!(
      ir.points(),
      0,
      "the explicit reconciling rollback abandoned the point it invalidated"
    );
    assert_eq!(
      ir.live_checkpoints_len(),
      baseline,
      "…and released its lineage entry"
    );
    assert_eq!(
      *ir.cursor().as_inner(),
      before,
      "…and still rolled back to the guard's own base, exactly as `rollback` would have"
    );
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "…and its pin: the pin set holds exactly the live begin points"
  );
}

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn the_checked_rollback_still_refuses_to_cross_an_open_session_point() {
  // The other half of the pair, and the reason the reconciling verb is a *third* settle rather
  // than a change to this one. A scope that owns every point opened inside it wants this refusal:
  // a point still open above the base means the code that opened it lost track of its own
  // speculation, and here is where that is cheapest to find. Detect-at-cause, every allocator
  // build, nothing restored.
  let mut input = silent_input("1 2 3 4 5");
  let mut ir = input.as_ref();
  let mut guard = ir.begin();
  let _point = guard.begin_point();
  let _ = guard.next().unwrap().expect("speculative work");
  guard.rollback();
}

#[test]
#[should_panic(expected = "no live session point")]
fn settling_a_point_the_reconciling_rollback_abandoned_is_refused() {
  // The flip side, and the same outcome `settling_a_point_a_guard_drop_reconciled_is_refused`
  // pins for the drop path: an abandoned point is *gone*, so a later settle is refused by the
  // session verb rather than rewinding to a timeline that no longer exists.
  let mut input = silent_input("1 2 3 4 5");
  let mut ir = input.as_ref();
  let point = {
    let mut guard = ir.begin();
    let point = guard.begin_point();
    let _ = guard.next().unwrap().expect("1");
    guard.rollback_abandoning_points();
    point
  };
  ir.rollback_point(point);
}

// ── 4d. The speculation closures settle like their own unwind does ───────────────────────────
//
// `attempt`, `try_attempt` and `attempt_parse` hand a whole `InputRef` to **caller-supplied
// code**, and a closure that opens a point and abandons it is exercising a documented liberty
// (`begin_point`: an abandoned point keeps its progress and is released with the handle). Each of
// these has two settling exits for the same decision — the closure declines, or the closure
// unwinds — and the unwind one has always reconciled, because it is the guard's rolling-back
// `Drop` and a `Drop` may not refuse anything. The decline one used to spell its restore with the
// *checked* verb, so the same legal history that unwinds cleanly panicked when it returned
// cleanly: a release panic, raised before anything was restored, blaming the attempt for someone
// else's legal choice.
//
// The five cells below are the four declining/erring arms plus the accepting one, and they say
// the same thing 4b's `attempt_panic_with_an_open_point_leaves_no_stranded_point` says about the
// unwind edge. The pair is the point: an attempt's two exits must reconcile identically, or the
// contract depends on how the closure chose to leave.

#[test]
fn attempt_declining_across_an_abandoned_point_reconciles() {
  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _ = ir
      .next()
      .unwrap()
      .expect("committed work before the attempt");
    let before = *ir.cursor().as_inner();
    let baseline = ir.live_checkpoints_len();

    let declined: Option<()> = ir.attempt(|ir| {
      let _point = ir.begin_point();
      let _ = ir
        .next()
        .unwrap()
        .expect("speculative work through the point");
      None
    });

    assert!(declined.is_none(), "the closure declined");
    assert_eq!(
      ir.points(),
      0,
      "the decline reconciled the point the closure abandoned, exactly as the unwind edge does"
    );
    assert_eq!(
      ir.live_checkpoints_len(),
      baseline,
      "…and released its lineage entry"
    );
    assert_eq!(
      *ir.cursor().as_inner(),
      before,
      "…and still rolled back to the attempt's own base — reconciling is not keeping"
    );
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "…and its pin: the pin set holds exactly the live begin points"
  );
}

#[test]
fn try_attempt_erring_across_an_abandoned_point_reconciles() {
  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _ = ir
      .next()
      .unwrap()
      .expect("committed work before the attempt");
    let before = *ir.cursor().as_inner();
    let baseline = ir.live_checkpoints_len();

    let erred: Result<(), NumErr> = ir.try_attempt(|ir| {
      let _point = ir.begin_point();
      let _ = ir
        .next()
        .unwrap()
        .expect("speculative work through the point");
      Err(NumErr::Lex)
    });

    assert_eq!(erred, Err(NumErr::Lex), "the error propagates untouched");
    assert_eq!(ir.points(), 0, "the error arm reconciled the open point");
    assert_eq!(
      ir.live_checkpoints_len(),
      baseline,
      "…and released its lineage entry"
    );
    assert_eq!(
      *ir.cursor().as_inner(),
      before,
      "…and still rolled back to the attempt's own base"
    );
  }
  assert_eq!(input.pinned_checkpoints_len(), 0, "…and its pin");
}

#[test]
fn attempt_parse_declining_across_an_abandoned_point_reconciles() {
  use crate::try_parse_input::ParseAttempt;

  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _ = ir
      .next()
      .unwrap()
      .expect("committed work before the attempt");
    let before = *ir.cursor().as_inner();
    let baseline = ir.live_checkpoints_len();

    let declined: Result<ParseAttempt<()>, NumErr> = ir.attempt_parse(|ir| {
      let _point = ir.begin_point();
      let _ = ir
        .next()
        .unwrap()
        .expect("speculative work through the point");
      Ok(ParseAttempt::Decline)
    });

    assert!(
      matches!(declined, Ok(ParseAttempt::Decline)),
      "the decline is reported in the crate's own vocabulary"
    );
    assert_eq!(ir.points(), 0, "the decline arm reconciled the open point");
    assert_eq!(
      ir.live_checkpoints_len(),
      baseline,
      "…and released its lineage entry"
    );
    assert_eq!(
      *ir.cursor().as_inner(),
      before,
      "…and still rolled back to the attempt's own base"
    );
  }
  assert_eq!(input.pinned_checkpoints_len(), 0, "…and its pin");
}

#[test]
fn attempt_parse_erring_across_an_abandoned_point_reconciles() {
  use crate::try_parse_input::ParseAttempt;

  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _ = ir
      .next()
      .unwrap()
      .expect("committed work before the attempt");
    let before = *ir.cursor().as_inner();
    let baseline = ir.live_checkpoints_len();

    let erred: Result<ParseAttempt<()>, NumErr> = ir.attempt_parse(|ir| {
      let _point = ir.begin_point();
      let _ = ir
        .next()
        .unwrap()
        .expect("speculative work through the point");
      Err(NumErr::Lex)
    });

    assert_eq!(
      erred.err(),
      Some(NumErr::Lex),
      "the error propagates untouched"
    );
    assert_eq!(ir.points(), 0, "the error arm reconciled the open point");
    assert_eq!(
      ir.live_checkpoints_len(),
      baseline,
      "…and released its lineage entry"
    );
    assert_eq!(
      *ir.cursor().as_inner(),
      before,
      "…and still rolled back to the attempt's own base"
    );
  }
  assert_eq!(input.pinned_checkpoints_len(), 0, "…and its pin");
}

#[test]
fn attempt_accepting_leaves_the_closure_s_point_open_and_its_progress_kept() {
  // The arm that does NOT change, pinned so the reconciliation above cannot be mistaken for "an
  // attempt settles the closure's points". It does not: `commit` keeps progress and releases only
  // the attempt's own base, so a point the closure left open is still open afterwards — and still
  // settleable by the caller, which is the whole reason session points are non-lexical. Only a
  // *rollback below it* abandons it, because only that destroys the lineage it describes.
  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _ = ir
      .next()
      .unwrap()
      .expect("committed work before the attempt");

    let point = ir
      .attempt(|ir| {
        let point = ir.begin_point();
        let _ = ir.next().unwrap().expect("kept work through the point");
        Some(point)
      })
      .expect("the closure accepted");

    assert_eq!(
      ir.points(),
      1,
      "an accepted attempt settles its own base only; the closure's point is still open"
    );
    let after_accept = *ir.cursor().as_inner();
    ir.rollback_point(point);
    assert!(
      *ir.cursor().as_inner() < after_accept,
      "…and still settleable: rolling it back put the token consumed inside the attempt back"
    );
  }
  assert_eq!(input.pinned_checkpoints_len(), 0, "nothing left pinned");
}

// ── 5. The depth accessor through the lifecycle ──────────────────────────────────────────────

#[test]
fn session_depth_accessor() {
  let mut input = silent_input("1 2 3 4");
  let mut ir = input.as_ref();

  assert_eq!(ir.points(), 0, "a fresh reference has no points");
  let outer = ir.begin_point();
  assert_eq!(ir.points(), 1);
  let inner = ir.begin_point();
  assert_eq!(ir.points(), 2, "nesting deepens the stack");
  ir.commit_point(inner);
  assert_eq!(ir.points(), 1, "committing the newest lowers the depth");
  ir.rollback_point(outer);
  assert_eq!(ir.points(), 0, "rolling back the oldest empties the stack");
}

// ── 6. Settled points leave the lineage bounded ──────────────────────────────────────────────

#[test]
fn settled_points_do_not_grow_the_lineage() {
  // Every settle path — commit and rollback alike — releases the point's lineage entry, so a
  // driver that speculates in a loop does not grow the input's live-checkpoint stack.
  let mut input = silent_input("1 2 3 4 5 6 7 8");
  let mut ir = input.as_ref();

  for i in 0..4 {
    let point = ir.begin_point();
    let _ = ir.next().unwrap();
    if i % 2 == 0 {
      ir.commit_point(point);
    } else {
      ir.rollback_point(point);
    }
    assert_eq!(
      ir.live_checkpoints_len(),
      0,
      "a settled session point releases its lineage entry"
    );
  }
  assert_eq!(ir.points(), 0);
}

// ── 7. A point does not survive the handle: dropping releases it ─────────────────────────────
//
// The pin and the lineage entry of a session point live in the `Lineage` memos on the *owning*
// `Input`, which outlives the handle; the point's `Checkpoint` lives in the handle's own stack.
// `InputRef`'s `Drop` is what keeps the two in step — an abandoned point releases both, keeps its
// progress, and rewinds nothing.

#[test]
fn dropping_the_handle_releases_the_open_points() {
  // Two points opened and NEITHER settled, then the handle dies. The input's pin set must hold
  // exactly the live begin points — and with no handle alive there are none.
  let mut input = silent_input("1 2 3 4 5");
  {
    let mut ir = input.as_ref();
    let _outer = ir.begin_point();
    let _ = ir.next().unwrap().expect("1");
    let _inner = ir.begin_point();
    let _ = ir.next().unwrap().expect("2");
    assert_eq!(ir.points(), 2, "two points are open");
    assert_eq!(
      ir.live_checkpoints_len(),
      2,
      "each open point holds a live lineage entry"
    );
    // …and the handle is dropped here, with both points still open.
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "an abandoned point releases its pin: the pin set holds exactly the live begin points, and \
     with the handle gone there are none"
  );
  assert_eq!(
    input.live_checkpoints_len(),
    0,
    "…and its lineage entry, exactly as a `commit_point` would — nobody can ever settle it"
  );
}

#[test]
fn dropping_the_handle_keeps_the_progress_of_the_open_points() {
  // The no-rollback-on-drop law. Abandoning a point releases its bookkeeping but rewinds nothing:
  // the tokens consumed through it stay consumed, and a later handle resumes where they left off.
  let mut input = verbose_input("1 2 3 4");
  let after_three;
  {
    let mut ir = input.as_ref();
    assert_eq!(take(&mut ir), "1", "committed work before the session");
    let _point = ir.begin_point();
    assert_eq!(take(&mut ir), "2", "speculative work through the point");
    assert_eq!(take(&mut ir), "3");
    emit(&mut ir, 0, NumErr::Lex);
    after_three = *ir.cursor().as_inner();
    assert_eq!(ir.points(), 1, "the point is still open at the drop");
  }
  {
    let mut ir = input.as_ref();
    assert_eq!(
      *ir.cursor().as_inner(),
      after_three,
      "drop kept the progress: the cursor did NOT rewind to the abandoned point"
    );
    assert_eq!(
      diag_count(&mut ir),
      1,
      "and kept the diagnostic emitted through it — drop is not a rollback"
    );
    assert_eq!(take(&mut ir), "4", "the stream resumes past the kept work");
    assert_eq!(ir.points(), 0, "a fresh handle starts with no points");
  }
}

#[test]
fn a_second_handle_rewinds_across_an_abandoned_point() {
  // End-to-end: the scenario a leaked pin would poison. From the SAME input, a first handle opens
  // a point and dies without settling it; a second handle then saves and restores over the region
  // that point covered. Every rewind the second handle can express — a raw `restore`, a fresh
  // session point, an `attempt` — must run to completion, never tripping a pin left behind by a
  // point nobody holds.
  let mut input = silent_input("1 2 3 4 5 6");
  {
    let mut ir = input.as_ref();
    let _ = ir.next().unwrap().expect("1");
    let _point = ir.begin_point(); // opened, pinned — and never settled
    let _ = ir.next().unwrap().expect("2");
  }
  let mut ir = input.as_ref();
  let at = *ir.cursor().as_inner();

  // A raw save/restore pair over the abandoned point's region.
  let ckp = ir.save();
  let _ = ir.next().unwrap().expect("3");
  ir.restore(ckp);
  assert_eq!(*ir.cursor().as_inner(), at, "the raw restore rewound");

  // A fresh session point, and an attempt that declines — both rewind through the same checked
  // `restore`, and neither may see a stale pin.
  let point = ir.begin_point();
  let _ = ir.next().unwrap().expect("3");
  ir.rollback_point(point);
  assert_eq!(*ir.cursor().as_inner(), at, "the new point rolled back");

  assert!(
    ir.attempt(|ir| {
      let _ = ir.next().unwrap();
      None::<()>
    })
    .is_none()
  );
  assert_eq!(*ir.cursor().as_inner(), at, "the declined attempt rewound");
}
