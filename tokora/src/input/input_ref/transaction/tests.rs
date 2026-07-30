//! Tests for the [`Transaction`](super::Transaction) guard.
//!
//! `begin` saves one checkpoint and wraps the input; `commit` keeps the parsed
//! progress, `rollback` returns to the begin point, and dropping an undecided guard
//! rolls back. Nested guards borrow their parent, so the last-in, first-out discipline
//! holds at compile time (see the `compile_fail` doctest on the type).

use crate::{
  Commit, InputRef, Rollback, Token,
  cache::DefaultCache,
  emitter::{Fatal, Silent, Verbose},
  error::token::UnexpectedToken,
  input::Input,
  lexer::LogosLexer,
  span::SimpleSpan,
  state::token_tracker::{TokenLimitExceeded, TokenLimiter},
};

// ── Fixture: a number lexer over a by-value token limiter ──────────────────────
//
// A by-value `TokenLimiter` (checkpointed and restored with the lexer state) is what
// makes a rolled-back limit trip re-tripable: an overflow peek never writes its
// temporary lexer's counter back, so a checkpoint taken before the trip saves a clean
// count and the committed path re-lexes and re-trips from scratch. `@` matches no rule,
// so it is a plain lexer error between numbers.

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
type NumFatalCtx<'a> = (Fatal<NumErr>, DefaultCache<'a, NumLexer<'a>>);

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

// ── begin/commit/rollback ──────────────────────────────────────────────────────

#[test]
fn txn_commit_keeps_progress() {
  // begin, consume two tokens through the guard, commit: the progress sticks, so the
  // next token is the third.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let start = *inp.cursor().as_inner();
  let mut txn = inp.begin();
  let _ = txn.next().unwrap().expect("first token");
  let _ = txn.next().unwrap().expect("second token");
  txn.commit();

  assert!(
    *inp.cursor().as_inner() > start,
    "commit keeps progress — the cursor advanced past the consumed tokens"
  );
  assert_eq!(
    *inp.next().unwrap().expect("third token").span_ref(),
    SimpleSpan::new(4, 5)
  );
}

#[test]
fn txn_rollback_restores_everything() {
  // ── position, span, lexer state, emission log, and the dedup watermark ─────────
  // "1 @ 2": crossing the malformed `@` through the guard emits its lexer error and
  // lifts the watermark. `rollback` must return every one of those.
  {
    let cache = DefaultCache::<'_, NumLexer<'_>>::default();
    let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
      "1 @ 2",
      TokenLimiter::with_limitation(usize::MAX),
      crate::input::InputContext::new(Verbose::<NumErr>::new(), cache),
    );

    {
      let mut inp = input.as_ref();

      let cur0 = *inp.cursor().as_inner();
      let span0 = *inp.span();
      let tokens0 = inp.state().tokens();

      let mut txn = inp.begin();
      // Consume `1`, cross `@` (emits the lexer error, lifts the watermark), consume
      // `2`, then abandon the branch.
      while txn.next().unwrap().is_some() {}
      txn.rollback();

      assert_eq!(*inp.cursor().as_inner(), cur0, "position rolled back");
      assert_eq!(*inp.span(), span0, "last-consumed span rolled back");
      assert_eq!(inp.state().tokens(), tokens0, "lexer state rolled back");
    }

    // The emission log was truncated by the rollback: nothing the guard emitted survives.
    let after_rollback: usize = input.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(
      after_rollback, 0,
      "diagnostics emitted inside the transaction are rolled back (empty emission log)"
    );

    // The watermark rolled back too, so the committed path re-crosses `@` and the
    // rewound lexer error becomes re-emittable — exactly once.
    {
      let mut inp = input.as_ref();
      while inp.next().unwrap().is_some() {}
    }
    let at = SimpleSpan::new(2, 3);
    assert_eq!(
      input
        .emitter()
        .errors()
        .get(&at)
        .map(|g| g.len())
        .unwrap_or(0),
      1,
      "the rewound lexer error re-emits exactly once when re-reached"
    );
    let total: usize = input.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 1, "only the re-emitted lexer error is retained");
  }

  // ── the poison boundary, via a limit-trip variant ─────────────────────────────
  // An overflow peek inside the transaction trips the limiter (latching poison and
  // emitting the diagnostic); `rollback` un-latches it, and the committed path re-trips
  // — the diagnostic surviving exactly once, never a diagnostic-less latch.
  {
    use generic_arraydeque::typenum::U6;
    let cache = DefaultCache::<'_, NumLexer<'_>>::default();
    let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
      "1 2 3 4 5 6",
      TokenLimiter::with_limitation(5),
      crate::input::InputContext::new(Verbose::<NumErr>::new(), cache),
    );
    {
      let mut inp = input.as_ref();

      let mut txn = inp.begin();
      let _ = txn.peek::<U6>().unwrap(); // overflow trip: poison + diagnostic
      assert!(
        txn.is_poisoned(),
        "the overflow trip latches poison inside the guard"
      );
      txn.rollback();
      assert!(
        !inp.is_poisoned(),
        "the rollback un-latches the speculative poison boundary"
      );

      // The committed path re-reaches the trip and re-latches.
      while inp.next().unwrap().is_some() {}
      assert!(inp.is_poisoned(), "the committed re-lex re-latches poison");
    }
    let total: usize = input.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(
      total, 1,
      "the limit diagnostic is emitted exactly once in total"
    );
  }
}

#[test]
fn txn_drop_without_commit_rolls_back() {
  // A guard dropped without deciding rolls back — uncommitted work is discarded.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let start = *inp.cursor().as_inner();
  {
    let mut txn = inp.begin();
    let _ = txn.next().unwrap().expect("first token");
    let _ = txn.next().unwrap().expect("second token");
    // `txn` drops here without commit/rollback → rollback on drop.
  }
  assert_eq!(
    *inp.cursor().as_inner(),
    start,
    "dropping an undecided guard rolls back to the begin point"
  );
  assert_eq!(
    *inp.next().unwrap().expect("token 1 again").span_ref(),
    SimpleSpan::new(0, 1),
    "the consumed tokens are replayable after the drop-rollback"
  );
}

#[test]
fn txn_nested_inner_commit_outer_rollback() {
  // A committed child's progress is discarded when its parent rolls back (savepoint
  // semantics: rolling back a parent discards everything its children committed).
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let start = *inp.cursor().as_inner();
  let mut outer = inp.begin();
  let _ = outer.next().unwrap().expect("outer consumes 1");

  let mut inner = outer.begin(); // borrows `outer` through DerefMut
  let _ = inner.next().unwrap().expect("inner consumes 2");
  inner.commit(); // keep the inner progress — within the still-open outer

  outer.rollback(); // discards everything, including the inner's committed 2

  assert_eq!(
    *inp.cursor().as_inner(),
    start,
    "the outer rollback discards the child's committed progress"
  );
  assert_eq!(
    *inp.next().unwrap().expect("token 1 again").span_ref(),
    SimpleSpan::new(0, 1)
  );
}

#[test]
fn txn_nested_inner_rollback_outer_commit() {
  // The mirror image: the inner rolls back its own work, the outer commits and keeps
  // only its own progress.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let mut outer = inp.begin();
  let _ = outer.next().unwrap().expect("outer consumes 1");
  let after_one = *outer.cursor().as_inner();

  let mut inner = outer.begin();
  let _ = inner.next().unwrap().expect("inner consumes 2");
  inner.rollback(); // back to just after token 1

  outer.commit(); // keep the outer progress: position stays just after token 1

  assert_eq!(
    *inp.cursor().as_inner(),
    after_one,
    "the inner rolled back; the outer kept its own progress"
  );
  assert_eq!(
    *inp.next().unwrap().expect("token 2").span_ref(),
    SimpleSpan::new(2, 3)
  );
}

#[test]
fn txn_over_limit_trip_rollback_reemits_exactly_once() {
  // Inside a transaction, an overflow peek trips the limiter (emitting the diagnostic);
  // rolling back un-emits it, and the committed path re-reaches the trip and re-emits —
  // exactly once in total, never zero.
  //   1 2 3 4 5 6   (limit 5 → the 6th scanned token trips; U6 window > U3 cache)
  use generic_arraydeque::typenum::U6;

  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(5),
    crate::input::InputContext::new(Verbose::<NumErr>::new(), cache),
  );
  {
    let mut inp = input.as_ref();

    let mut txn = inp.begin();
    let _ = txn.peek::<U6>().unwrap(); // overflow trip: emits the limit diagnostic
    txn.rollback();
    assert!(
      !inp.is_poisoned(),
      "the rollback un-poisons and un-emits the speculative diagnostic"
    );

    while inp.next().unwrap().is_some() {}
    assert!(inp.is_poisoned(), "the committed re-lex re-latches poison");
  }

  let errs: Vec<&NumErr> = input.emitter().errors().values().flatten().collect();
  assert_eq!(
    errs.len(),
    1,
    "the limit diagnostic is emitted exactly once in total"
  );
  assert_eq!(*errs[0], NumErr::Limit, "and it is the limit diagnostic");
}

/// A plain `&mut InputRef` consumer: the guard must deref-coerce into it.
fn consume_all<'inp>(inp: &mut InputRef<'inp, '_, NumLexer<'inp>, NumCtx<'inp>>) -> usize {
  let mut n = 0;
  while inp.next().unwrap().is_some() {
    n += 1;
  }
  n
}

#[test]
fn txn_passes_as_input_ref() {
  // `&mut Transaction` coerces to `&mut InputRef` via `DerefMut`, so every combinator
  // and helper written against `InputRef` composes with a guard unchanged.
  let mut input = silent_input("1 2 3");
  let mut inp = input.as_ref();

  let mut txn = inp.begin();
  let consumed = consume_all(&mut txn); // deref coercion into fn(&mut InputRef)
  assert_eq!(consumed, 3, "the helper drove the input through the guard");
  txn.commit();

  assert!(inp.is_eoi(), "progress kept — every token was consumed");
}

// ── Commit drop policy (begin_with::<Commit>) ────────────────────────────────────
//
// The dual of the speculative default: an undecided `Commit`-policy guard KEEPS its
// progress on drop (like dropping a raw checkpoint). `commit`/`rollback` still work.

#[test]
fn txn_commit_policy_drop_keeps_progress() {
  // A `Commit`-policy guard dropped without deciding keeps its progress — the opposite of
  // the `Rollback` default, and the whole point of the policy.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let start = *inp.cursor().as_inner();
  {
    let mut txn = inp.begin_with::<Commit>();
    let _ = txn.next().unwrap().expect("first token");
    let _ = txn.next().unwrap().expect("second token");
    // `txn` drops here without commit/rollback → Commit policy keeps the progress.
  }
  assert!(
    *inp.cursor().as_inner() > start,
    "dropping an undecided Commit-policy guard keeps the consumed progress"
  );
  assert_eq!(
    *inp.next().unwrap().expect("third token").span_ref(),
    SimpleSpan::new(4, 5),
    "the input resumed past the kept tokens"
  );
}

#[test]
fn txn_commit_policy_explicit_commit_keeps() {
  // `commit` is available whatever the policy: on a Commit-policy guard it keeps progress,
  // just as on the default flavour.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let start = *inp.cursor().as_inner();
  let mut txn = inp.begin_with::<Commit>();
  let _ = txn.next().unwrap().expect("first token");
  txn.commit();

  assert!(
    *inp.cursor().as_inner() > start,
    "explicit commit on a Commit-policy guard keeps progress"
  );
}

#[test]
fn txn_commit_policy_explicit_rollback_restores() {
  // `rollback` is available whatever the policy: a Commit-policy guard can still be rolled
  // back explicitly, restoring the input to the begin point.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let start = *inp.cursor().as_inner();
  let mut txn = inp.begin_with::<Commit>();
  let _ = txn.next().unwrap().expect("first token");
  let _ = txn.next().unwrap().expect("second token");
  txn.rollback();

  assert_eq!(
    *inp.cursor().as_inner(),
    start,
    "explicit rollback on a Commit-policy guard restores to the begin point"
  );
  assert_eq!(
    *inp.next().unwrap().expect("token 1 again").span_ref(),
    SimpleSpan::new(0, 1),
    "the consumed tokens replay after the explicit rollback"
  );
}

#[test]
fn txn_commit_policy_keeps_progress_on_fatal_error() {
  // The Fatal-emitter case, mirroring the old raw pratt loop: an error propagating out of a
  // Commit-policy guard via `?` drops the still-undecided guard, which KEEPS the progress
  // consumed up to the error rather than rolling back. A fail-fast `Fatal` emitter turns the
  // malformed `@` into a propagating error.
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut input = Input::<NumLexer<'_>, NumFatalCtx<'_>, ()>::with_state_and_context(
    "1 @ 2",
    TokenLimiter::with_limitation(usize::MAX),
    crate::input::InputContext::new(Fatal::<NumErr>::new(), cache),
  );
  let mut inp = input.as_ref();

  // Drives a Commit-policy guard that propagates the first fail-fast error via `?`. When the
  // `@` lexer error fires, `next` commits the span up to it and returns `Err`; the `?` drops
  // the undecided guard, whose Commit policy keeps that progress.
  fn drive<'inp>(
    inp: &mut InputRef<'inp, '_, NumLexer<'inp>, NumFatalCtx<'inp>>,
  ) -> Result<(), NumErr> {
    let mut txn = inp.begin_with::<Commit>();
    let _ = txn.next()?; // consume "1"
    let _ = txn.next()?; // cross "@": Fatal emits Err → `?` drops the guard (Commit: keep)
    txn.commit();
    Ok(())
  }

  let start = *inp.cursor().as_inner();
  let result = drive(&mut inp);
  assert!(
    result.is_err(),
    "the fatal lexer error propagated out of the guard"
  );
  assert!(
    *inp.cursor().as_inner() > start,
    "the Commit-policy drop kept the progress consumed before the `?` (never rolled back)"
  );
  assert_eq!(
    *inp
      .next()
      .unwrap()
      .expect("resume past the kept progress")
      .span_ref(),
    SimpleSpan::new(4, 5),
    "the input resumed just past the consumed `@` — the guard kept its progress, as raw pratt did"
  );
}

#[test]
fn txn_nested_cross_policy() {
  // The two policies are independent typestates: the child's policy governs the child, the
  // parent's governs the parent.

  // Case A: a Commit child inside a Rollback parent. The child's drop keeps its progress
  // (seen through the parent), but the parent's own drop then rolls everything back.
  {
    let mut input = silent_input("1 2 3 4");
    let mut inp = input.as_ref();
    let start = *inp.cursor().as_inner();
    {
      let mut parent = inp.begin_with::<Rollback>();
      let _ = parent.next().unwrap().expect("parent consumes 1");
      let after_one = *parent.cursor().as_inner();
      {
        let mut child = parent.begin_with::<Commit>();
        let _ = child.next().unwrap().expect("child consumes 2");
        // child drops (Commit) → keeps its progress
      }
      assert!(
        *parent.cursor().as_inner() > after_one,
        "the Commit child kept its progress on drop (child policy governs the child)"
      );
      // parent drops (Rollback) → restores to the begin point
    }
    assert_eq!(
      *inp.cursor().as_inner(),
      start,
      "the Rollback parent rolled everything back on drop, discarding the child's kept work"
    );
  }

  // Case B: a Rollback child inside a Commit parent. The child's drop rolls back its own
  // work; the parent's drop then keeps the parent's progress.
  {
    let mut input = silent_input("1 2 3 4");
    let mut inp = input.as_ref();
    let after_one;
    {
      let mut parent = inp.begin_with::<Commit>();
      let _ = parent.next().unwrap().expect("parent consumes 1");
      after_one = *parent.cursor().as_inner();
      {
        let mut child = parent.begin_with::<Rollback>();
        let _ = child.next().unwrap().expect("child consumes 2");
        // child drops (Rollback) → restores to `after_one`
      }
      assert_eq!(
        *parent.cursor().as_inner(),
        after_one,
        "the Rollback child rolled back its own work on drop (child policy governs the child)"
      );
      // parent drops (Commit) → keeps its progress
    }
    assert_eq!(
      *inp.cursor().as_inner(),
      after_one,
      "the Commit parent kept its progress on drop (parent policy governs the parent)"
    );
  }
}

#[cfg(all(
  debug_assertions,
  any(feature = "std", feature = "alloc"),
  target_has_atomic = "ptr"
))]
#[test]
fn txn_commit_removes_id_from_live_stack() {
  // Committing drops a checkpoint that was never restored; its debug-witness id must be
  // forgotten so the live stack does not grow across commit-heavy loops.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let baseline = inp.live_checkpoints_len();
  for _ in 0..100 {
    let txn = inp.begin();
    txn.commit();
  }
  assert_eq!(
    inp.live_checkpoints_len(),
    baseline,
    "each commit forgets its id — the live stack returns to its baseline length"
  );
}

// ── State surgery is transactional: checkpoints survive it ───────────────────────
//
// `Transaction` derefs to `InputRef`, so `set_state` / `state_mut` are reachable inside a
// guard. State surgery re-keys the FORWARD-scanning facts (cache, poison boundary, dedup
// watermark) but is itself transactional: a checkpoint saved before it — the guard's begin
// point — pure-copies every one of those facts, so rolling back across the surgery UNDOES
// it. Explicit `rollback` and an implicit drop-rollback therefore agree.

#[test]
fn txn_drop_and_explicit_rollback_agree_after_state_surgery() {
  // The finding's divergence, resolved. State surgery inside a guard, then compare an
  // implicit drop-rollback against an explicit `rollback()`. At HEAD they DIVERGED:
  // `set_state` cleared the live-checkpoint lineage, so the drop path (`restore_unchecked`)
  // silently restored the pre-surgery snapshot while the explicit path (checked `restore`)
  // debug-panicked as non-LIFO. Post-fix both restore identically, undoing the surgery: the
  // pre-surgery regime, poison boundary, dedup watermark, and position all return.
  //   "1 @ 3 4": `@` is a plain lexer error; the limit-2 limiter trips on the 3rd number.
  use generic_arraydeque::typenum::U6;

  // Everything observable about the input after the enclosing transaction rolls back.
  #[derive(Debug, PartialEq)]
  struct Outcome {
    cursor: usize,
    limit: usize,
    tokens: usize,
    poisoned: bool,
    replayed: Vec<SimpleSpan>,
    diags: usize,
  }

  fn run(explicit: bool) -> Outcome {
    let cache = DefaultCache::<'_, NumLexer<'_>>::default();
    let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
      "1 @ 3 4",
      TokenLimiter::with_limitation(2),
      crate::input::InputContext::new(Verbose::<NumErr>::new(), cache),
    );
    let outcome;
    {
      let mut inp = input.as_ref();

      // A speculative peek seals `@`'s lexer error above the cursor (lifting the dedup
      // watermark past it) and trips the limiter (latching the poison boundary), all
      // without consuming — the cursor stays at 0.
      let _ = inp.peek::<U6>().unwrap();
      assert!(
        inp.is_poisoned(),
        "the peek trips the limiter and latches poison"
      );
      let pre_diags: usize = inp.emitter().errors().values().map(|g| g.len()).sum();
      assert_eq!(
        pre_diags, 2,
        "the peek emitted the `@` lexer error and the limit diagnostic"
      );

      let mut txn = inp.begin();
      // State surgery inside the transaction: re-keys the boundary away, resets the
      // watermark to the committed cursor, clears the cache, swaps the regime.
      txn.set_state(TokenLimiter::with_limitation(usize::MAX));
      assert!(
        !txn.is_poisoned(),
        "the surgery re-keys the boundary away inside the guard"
      );

      if explicit {
        txn.rollback(); // HEAD: checked restore debug-panics (lineage cleared); fix: restores
      } else {
        drop(txn); // HEAD: restore_unchecked silently restores; fix: restores identically
      }

      // The rollback undid the surgery: the pre-surgery facts are back.
      let cursor = *inp.cursor().as_inner();
      let limit = inp.state().limitation();
      let tokens = inp.state().tokens();
      let poisoned = inp.is_poisoned();

      // Drain to completion under the restored (old) regime: the prefix replays and the
      // stream stops at the restored boundary. The restored watermark keeps `@`
      // deduplicated, so no diagnostic is duplicated.
      let mut replayed = Vec::new();
      while let Some(tok) = inp.next().unwrap() {
        replayed.push(*tok.span_ref());
      }
      let diags = inp.emitter().errors().values().map(|g| g.len()).sum();

      outcome = Outcome {
        cursor,
        limit,
        tokens,
        poisoned,
        replayed,
        diags,
      };
    }
    outcome
  }

  let dropped = run(false);
  let explicit = run(true);

  // The finding, resolved: the two rollback paths produce identical state.
  assert_eq!(
    dropped, explicit,
    "drop-rollback and explicit rollback produce identical state after state surgery"
  );
  // Both undid the surgery — all four pre-surgery facts returned.
  assert_eq!(
    dropped.cursor, 0,
    "position: rolled back to the begin-point cursor"
  );
  assert_eq!(
    dropped.limit, 2,
    "regime: the pre-surgery limit-2 limiter returned, not the fresh one"
  );
  assert_eq!(
    dropped.tokens, 0,
    "regime: the saved counter returned (the peek's increments were never committed)"
  );
  assert!(
    dropped.poisoned,
    "boundary: the pre-surgery poison boundary returned"
  );
  assert_eq!(
    dropped.replayed,
    vec![SimpleSpan::new(0, 1), SimpleSpan::new(4, 5)],
    "position: the prefix replays from the begin point and stops at the restored boundary"
  );
  assert_eq!(
    dropped.diags, 2,
    "watermark: `@` stayed deduplicated on replay — the saved watermark returned, no duplicate"
  );
}

#[test]
fn txn_commit_keeps_state_surgery() {
  // The dual of the rollback tests: state surgery inside a COMMITTED transaction PERSISTS.
  // Commit keeps the progress and the re-keyed forward-scanning facts (fresh regime, dropped
  // boundary, reset watermark). Only rolling back across the surgery undoes it.
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    crate::input::InputContext::new(Verbose::<NumErr>::new(), cache),
  );
  {
    let mut inp = input.as_ref();

    // Trip the limiter via `next`, latching the poison boundary.
    assert!(inp.next().unwrap().is_some(), "1");
    assert!(inp.next().unwrap().is_some(), "2");
    assert!(inp.next().unwrap().is_none(), "the 3rd scan trips → None");
    assert!(inp.is_poisoned(), "the trip latched the poison boundary");

    let mut txn = inp.begin();
    // Surgery inside the guard, then COMMIT: the re-key must survive the commit.
    txn.set_state(TokenLimiter::with_limitation(usize::MAX));
    assert!(!txn.is_poisoned(), "the surgery re-keys the boundary away");
    txn.commit();

    // Commit kept the surgery: the input stays un-poisoned and scanning resumes past the
    // old boundary under the fresh regime.
    assert!(
      !inp.is_poisoned(),
      "commit kept the surgery — the boundary stays dropped"
    );
    assert_eq!(
      inp.state().limitation(),
      usize::MAX,
      "commit kept the fresh regime"
    );

    let mut resumed = 0usize;
    while inp.next().unwrap().is_some() {
      resumed += 1;
    }
    assert_eq!(
      resumed, 4,
      "scanning resumed past the old boundary: 3, 4, 5, 6"
    );
  }
}

#[test]
fn txn_rollback_after_state_surgery_restores_poison_and_diagnostic() {
  // The poison-focused twin of the divergence test. Trip the limiter (latching the boundary
  // and emitting the limit diagnostic exactly once), begin a transaction, do state surgery
  // (the re-key drops the boundary — forward semantics), then EXPLICITLY roll back. The
  // rollback undoes the surgery: the original poison boundary returns paired with its single
  // retained diagnostic — no duplicate, no diagnostic-less latch. At HEAD the explicit
  // rollback debug-panicked (the surgery cleared the lineage).
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_context(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    crate::input::InputContext::new(Verbose::<NumErr>::new(), cache),
  );
  {
    let mut inp = input.as_ref();

    assert!(inp.next().unwrap().is_some(), "1");
    assert!(inp.next().unwrap().is_some(), "2");
    assert!(inp.next().unwrap().is_none(), "the 3rd scan trips → None");
    assert!(inp.is_poisoned(), "the trip latched the poison boundary");
    let after_trip = *inp.cursor().as_inner();
    let diags_after_trip: usize = inp.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(
      diags_after_trip, 1,
      "the trip emitted the limit diagnostic once"
    );

    let mut txn = inp.begin();
    txn.set_state(TokenLimiter::with_limitation(usize::MAX));
    assert!(
      !txn.is_poisoned(),
      "the surgery re-keys the boundary away inside the guard"
    );
    txn.rollback(); // HEAD: debug-panics; post-fix: restores the pre-surgery trip state

    // The original poison boundary and its single diagnostic returned, still paired.
    assert!(
      inp.is_poisoned(),
      "the pre-surgery poison boundary returned after rollback"
    );
    assert_eq!(
      *inp.cursor().as_inner(),
      after_trip,
      "position rolled back to the trip point"
    );
    assert_eq!(
      inp.state().limitation(),
      2,
      "the pre-surgery regime returned"
    );
    let total: usize = inp.emitter().errors().values().map(|g| g.len()).sum();
    assert_eq!(
      total, 1,
      "the limit diagnostic is retained exactly once — never duplicated"
    );

    // Scanning resumes under the OLD (tripped) regime: the boundary stops the stream.
    assert!(
      inp.next().unwrap().is_none(),
      "the restored boundary stops scanning (old regime)"
    );
  }
}

// ── Raw restore below a live guard base panics AT THE RESTORE (detect-at-cause) ──
//
// A raw restore through the guard (`DerefMut`) to a checkpoint saved BEFORE the guard began
// would tear out the guard's begin point from under it. The guard PINS its base id on entry,
// so the checked `restore` path refuses the invalidating restore loudly, right where it is
// requested, in every allocator build — instead of letting it succeed and detecting the
// wreckage later at the guard's settle. This is the third strengthening of this area: silent
// commit → detect-at-use (stale panic at `rollback` / skip at drop) → detect-at-cause (panic
// at the raw restore). The detect-at-use backstops remain as defense in depth (and for
// no-alloc, which maintains no pin set).

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn txn_raw_restore_below_base_panics_at_the_restore() {
  // The exact scenario converted from `txn_drop_after_raw_restore_below_base_does_not_
  // resurrect`. save A, begin (rollback policy), consume, then raw-restore to A through the
  // guard. A predates the guard's begin point, so restoring it would invalidate the guard's
  // pinned base — the pin check panics AT THE RESTORE. At HEAD the restore instead succeeded
  // and, with further parsing before the drop, the rolling-back drop silently committed the
  // abandoned work.
  let mut input = silent_input("1 2 3 4 5");
  let mut inp = input.as_ref();

  let a = inp.save(); // raw checkpoint, below the guard's begin point
  let _ = inp.next().unwrap().expect("consume 1"); // advance past A before begin

  let mut txn = inp.begin(); // base checkpoint, above A
  let _ = txn.next().unwrap().expect("consume 2"); // advance past the base
  txn.restore(a); // POST-FIX: panics right here — the target predates the guard's begin point
}

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn txn_explicit_rollback_after_raw_restore_below_base_panics_at_restore() {
  // Converted from `txn_explicit_rollback_after_raw_restore_below_base_panics_stale`. The panic
  // now fires AT THE RAW RESTORE (detect-at-cause), before the explicit `rollback` is ever
  // reached. At HEAD the raw restore succeeded and the later `rollback` panicked as stale
  // ("transaction base is stale"); post-fix the pinned restore panics first.
  let mut input = silent_input("1 2 3 4 5");
  let mut inp = input.as_ref();

  let a = inp.save();
  let _ = inp.next().unwrap().expect("consume 1");

  let mut txn = inp.begin();
  let _ = txn.next().unwrap().expect("consume 2");
  txn.restore(a); // POST-FIX: panics here (was: succeeded, then `rollback` panicked as stale)
  txn.rollback(); // unreachable post-fix
}

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn txn_commit_policy_raw_restore_below_base_panics_at_restore() {
  // Converted from `txn_commit_policy_drop_after_raw_restore_below_base_is_noop`. A Commit-
  // policy guard pins its base on entry exactly like a Rollback-policy one — it still logically
  // owns the region from its begin point forward — so a raw restore below the base panics AT
  // THE RESTORE too. At HEAD the restore succeeded and the Commit-policy drop was a harmless
  // no-op (it only forgets the already-absent id); post-fix the restore never lands.
  let mut input = silent_input("1 2 3 4 5");
  let mut inp = input.as_ref();

  let a = inp.save();
  let _ = inp.next().unwrap().expect("consume 1");

  let mut txn = inp.begin_with::<Commit>();
  let _ = txn.next().unwrap().expect("consume 2");
  txn.restore(a); // POST-FIX: panics here (a Commit-policy guard pins its base too)
}

// ── Negative controls: legal mixing must NOT trip the pin ───────────────────────

#[test]
fn txn_lifo_clean_raw_pair_above_base_is_legal() {
  // A raw save/restore pair taken and released entirely ABOVE the guard's begin point is
  // LIFO-legal and must NOT trip the pin — the pinned base sits BELOW the raw checkpoint, so
  // restoring the raw one pops only itself, never the base. The guard commits normally after.
  let mut input = silent_input("1 2 3 4 5");
  let mut inp = input.as_ref();

  let mut txn = inp.begin(); // base pinned
  let _ = txn.next().unwrap().expect("consume 1"); // advance past the base
  let c = txn.save(); // raw checkpoint ABOVE the base
  let mid = *txn.cursor().as_inner();
  let _ = txn.next().unwrap().expect("consume 2");
  txn.restore(c); // legal (LIFO): pops only c — the base stays live and pinned
  assert_eq!(
    *txn.cursor().as_inner(),
    mid,
    "the legal raw restore returned to c"
  );
  txn.commit(); // commits at c's position

  assert_eq!(
    *inp.cursor().as_inner(),
    mid,
    "the guard committed the progress up to the legal raw restore point"
  );
}

#[test]
fn txn_nested_attempt_is_legal() {
  // An `attempt` nested inside a live transaction pins and unpins entirely within the
  // closure's extent. A declining attempt rolls back to ITS OWN checkpoint (above the guard's
  // base), never below the base, so it never trips the guard's pin; the guard is usable after.
  let mut input = silent_input("1 2 3 4 5");
  let mut inp = input.as_ref();

  let mut txn = inp.begin();
  let _ = txn.next().unwrap().expect("consume 1");
  let before = *txn.cursor().as_inner();
  let out: Option<()> = txn.attempt(|inp| {
    let _ = inp.next().unwrap().expect("consume 2 inside attempt");
    None // decline → rolls back to the attempt's own checkpoint (above the base)
  });
  assert!(out.is_none(), "the attempt declined");
  assert_eq!(
    *txn.cursor().as_inner(),
    before,
    "the declined attempt rolled back to its own checkpoint, not below the base"
  );
  txn.commit();
  assert_eq!(
    *inp.cursor().as_inner(),
    before,
    "the guard committed at the post-attempt position"
  );
}

// ── The unwind posture of an undecided Commit-policy guard ───────────────────────
//
// A panic inside a `begin_with::<Commit>` region that a host catches used to leave the
// input in a state no non-panicking execution can produce: the operator consumed, the
// right-hand side absent. The guard's `Drop` ran its commit arm with no unwind gate.

#[test]
fn commit_guard_dropped_mid_unwind_rolls_back() {
  // Consume one token inside a `Commit`-policy region, then panic; the host catches. The
  // guard is undecided, so its `Drop` decides — and an unwind must not promote speculative
  // progress.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let mut txn = inp.begin_with::<Commit>();
    let _ = txn
      .next()
      .unwrap()
      .expect("the half-iteration consumes `1`");
    panic!("D27: the region panics with the guard undecided");
  }));
  assert!(caught.is_err(), "the panic was caught by this host");

  assert_eq!(
    *inp
      .next()
      .unwrap()
      .expect("a token after the caught panic")
      .span_ref(),
    SimpleSpan::new(0, 1),
    "an undecided Commit guard dropped mid-unwind rolls back: the next token is `1`, not `2`"
  );
}

#[test]
fn commit_guard_dropped_without_unwind_still_commits() {
  // The control that pins WHAT the gate reads: the unwind fact, not the drop itself. A
  // non-panicking undecided `Commit` drop keeps its progress exactly as before.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  {
    let mut txn = inp.begin_with::<Commit>();
    let _ = txn.next().unwrap().expect("consume `1`");
  }

  assert_eq!(
    *inp
      .next()
      .unwrap()
      .expect("a token after the ordinary drop")
      .span_ref(),
    SimpleSpan::new(2, 3),
    "an ordinary undecided Commit drop still commits: the next token is `2`"
  );
}

#[test]
fn rollback_guard_dropped_mid_unwind_still_rolls_back() {
  // p87_12b: the default `begin()` flavour is untouched by the flip.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let mut txn = inp.begin();
    let _ = txn.next().unwrap().expect("consume `1`");
    panic!("D27 control: the Rollback-policy region panics");
  }));
  assert!(caught.is_err(), "the panic was caught by this host");

  assert_eq!(
    *inp
      .next()
      .unwrap()
      .expect("a token after the caught panic")
      .span_ref(),
    SimpleSpan::new(0, 1),
    "the Rollback default rolls back mid-unwind exactly as it always did"
  );
}

// ── The settle path's cost as a function of nesting depth ────────────────────────
//
// `Lineage::contains` and `pop_through` back every guard settle. Both scanned front-to-back
// with no last-element fast path, while `forget`/`unpin` had one — so `d` nested guards
// unwound by drop scanned exactly `d(d+1)` elements. The odometer in `lineage::scan_probe`
// makes that law assertable; the bound is deliberately generous (`8d`) so the cell pins the
// asymptotics, not a constant.

/// Opens `depth` nested default (`Rollback`) guards and lets every one settle by drop.
fn nest_and_drop<'inp>(inp: &mut InputRef<'inp, '_, NumLexer<'inp>, NumCtx<'inp>>, depth: usize) {
  if depth == 0 {
    return;
  }
  let mut txn = inp.begin();
  nest_and_drop(&mut txn, depth - 1);
  // `txn` drops here, undecided → the Rollback default settles it.
}

/// Opens `depth` nested default guards and settles every one by an explicit `commit`.
fn nest_and_commit<'inp>(inp: &mut InputRef<'inp, '_, NumLexer<'inp>, NumCtx<'inp>>, depth: usize) {
  if depth == 0 {
    return;
  }
  let mut txn = inp.begin();
  nest_and_commit(&mut txn, depth - 1);
  txn.commit();
}

#[test]
fn nested_drop_rollback_scans_linearly() {
  for depth in [100usize, 400, 1600] {
    let mut input = silent_input("1 2 3 4");
    let mut inp = input.as_ref();

    crate::input::lineage::scan_probe::reset();
    nest_and_drop(&mut inp, depth);
    let scanned = crate::input::lineage::scan_probe::scanned();

    assert!(
      scanned <= 8 * depth,
      "nested rollback-on-drop is quadratic in lineage depth: {depth} nested guards scanned \
       {scanned} live-lineage elements (linear bound: {}); d(d+1) = {}",
      8 * depth,
      depth * (depth + 1)
    );
  }
}

#[test]
fn nested_commit_scans_nothing() {
  // The contrast the audit measured at zero: `forget`/`unpin` already have their fast path, so
  // a committing settle inspects no live-lineage element through the two scanning primitives.
  let mut input = silent_input("1 2 3 4");
  let mut inp = input.as_ref();

  crate::input::lineage::scan_probe::reset();
  nest_and_commit(&mut inp, 400);
  assert_eq!(
    crate::input::lineage::scan_probe::scanned(),
    0,
    "the committing settle path scans no lineage elements"
  );
}
