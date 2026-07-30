//! Tests for the [`StackedTransaction`](super::StackedTransaction) guard.
//!
//! `begin_stacked` saves a base checkpoint and wraps the input; `savepoint` marks a
//! position, `rollback_to` returns to a mark while destroying every younger one (SQL
//! `ROLLBACK TO`), `release` forgets marks while keeping progress (SQL `RELEASE`), and
//! `commit`/`rollback` decide the whole transaction. A foreign or destroyed
//! [`SavepointId`](super::SavepointId) panics in every build.

use crate::{
  Commit, InputRef, Token,
  cache::DefaultCache,
  emitter::{Fatal, Silent, Verbose},
  error::token::UnexpectedToken,
  input::Input,
  lexer::LogosLexer,
  span::SimpleSpan,
  state::token_tracker::{TokenLimitExceeded, TokenLimiter},
};

// ── Fixture: a number lexer over a by-value token limiter (mirrors the sibling) ──
//
// A by-value `TokenLimiter` (checkpointed and restored with the lexer state) is what
// makes a rolled-back limit trip re-tripable: an overflow peek never writes its
// temporary lexer's counter back, so a checkpoint taken before the trip saves a clean
// count and the committed path re-lexes and re-trips from scratch.

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
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  Input::<NumLexer<'_>, NumCtx<'_>, ()>::with_state_and_cache(
    src,
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  )
}

// ── rollback_to / savepoint ─────────────────────────────────────────────────────

#[test]
fn stacked_rollback_to_middle_destroys_younger_keeps_target() {
  // Three savepoints, then roll back to the middle one twice (SQL parity): the target
  // resumes the stream at exactly its position each time, and the younger savepoint is
  // destroyed structurally.
  let mut input = silent_input("1 2 3 4 5");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let mut txn = inp.begin_stacked();
  let _ = txn.next().unwrap().expect("1");
  let _sp1 = txn.savepoint();
  let _ = txn.next().unwrap().expect("2");
  let sp2 = txn.savepoint(); // fallback: the next token is "3"
  let _ = txn.next().unwrap().expect("3");
  let _sp3 = txn.savepoint();
  let _ = txn.next().unwrap().expect("4");

  txn.rollback_to(sp2);
  assert_eq!(
    *txn.next().unwrap().expect("resume at 3").span_ref(),
    SimpleSpan::new(4, 5),
    "rollback_to resumes exactly at the target savepoint"
  );

  // SQL parity: roll back to the same savepoint again after consuming past it.
  txn.rollback_to(sp2);
  assert_eq!(
    *txn.next().unwrap().expect("resume at 3 again").span_ref(),
    SimpleSpan::new(4, 5),
    "rolling back to the same savepoint twice works (SQL parity)"
  );

  assert_eq!(
    txn.saves.len(),
    2,
    "the first rollback_to destroyed the younger savepoint; only sp1 and sp2 remain"
  );
}

#[test]
fn stacked_release_keeps_progress_and_forgets() {
  // Release forgets the savepoint but keeps every parsed byte: the position does not move.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let mut txn = inp.begin_stacked();
  let _ = txn.next().unwrap().expect("1");
  let sp1 = txn.savepoint();
  let _ = txn.next().unwrap().expect("2");
  let _ = txn.next().unwrap().expect("3");

  txn.release(sp1);
  assert!(
    txn.saves.is_empty(),
    "release forgets the savepoint (and any younger)"
  );
  assert_eq!(
    *txn.next().unwrap().expect("4").span_ref(),
    SimpleSpan::new(6, 7),
    "release keeps the parsed progress: the position does not move"
  );
  txn.commit();
}

#[test]
#[should_panic(expected = "stacked transaction: savepoint is stale")]
fn stacked_stale_id_after_rollback_panics() {
  // Rolling back to an older savepoint destroys the younger one; using the younger id
  // afterwards panics as stale.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let mut txn = inp.begin_stacked();
  let _ = txn.next().unwrap().expect("1");
  let sp1 = txn.savepoint();
  let _ = txn.next().unwrap().expect("2");
  let sp2 = txn.savepoint();

  txn.rollback_to(sp1); // destroys sp2 (younger)
  txn.rollback_to(sp2); // sp2 is stale → panic
}

// Note: the same-input cross-transaction misuse (an id from one transaction on the same
// input used in a later one) is now a *compile* error — the id's lifetime brand keeps the
// input loan open, so the second `begin_stacked` cannot re-borrow it. It is pinned by a
// `compile_fail` doctest on `SavepointId`, so there is no runtime test for it here.

#[test]
#[should_panic(expected = "stacked transaction: savepoint belongs to a different transaction")]
fn stacked_foreign_input_savepoint_panics() {
  // Two simultaneously-live inputs. Both transactions are opened *before* either savepoint
  // is taken, so their brand regions coincide and the compiler unifies them — passing input
  // A's id to input B's transaction type-checks. (Opening B's input after A's savepoint
  // would instead be a compile error, the same brand-region mismatch the nesting doctests
  // pin.) Each id carries the address of its input's `poison_boundary` field, and the two
  // live inputs are distinct structs at distinct addresses, so the foreign id is caught at
  // runtime in every build.
  let mut input_a = silent_input("1 2 3 4");
  let mut emitter_a = Silent::<NumErr>::new();
  let mut input_b = silent_input("1 2 3 4");
  let mut emitter_b = Silent::<NumErr>::new();

  let mut inp_a = input_a.as_ref(&mut emitter_a);
  let mut inp_b = input_b.as_ref(&mut emitter_b);

  let mut txn_a = inp_a.begin_stacked();
  let mut txn_b = inp_b.begin_stacked();

  let sp_a = txn_a.savepoint();
  let _sp_b = txn_b.savepoint();

  // `sp_a` belongs to input A's transaction → foreign to `txn_b` → panic.
  txn_b.rollback_to(sp_a);
}

#[test]
fn stacked_drop_rolls_back_to_begin() {
  // An undecided stacked transaction rolls back to its begin point on drop, discarding
  // every savepoint.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let start = *inp.cursor().as_inner();
  {
    let mut txn = inp.begin_stacked();
    let _ = txn.next().unwrap().expect("1");
    let _sp = txn.savepoint();
    let _ = txn.next().unwrap().expect("2");
    // `txn` drops here undecided → rollback to begin.
  }
  assert_eq!(
    *inp.cursor().as_inner(),
    start,
    "dropping an undecided stacked transaction rolls back to the begin point"
  );
  assert_eq!(
    *inp.next().unwrap().expect("1 again").span_ref(),
    SimpleSpan::new(0, 1),
    "the consumed tokens replay after the drop-rollback"
  );
}

#[test]
fn stacked_savepoint_over_limit_trip_reemits_exactly_once() {
  // A savepoint taken before an overflow trip, rolled back to, then re-reached on the
  // committed path: the limit diagnostic is emitted exactly once in total, never zero.
  //   1 2 3 4 5 6   (limit 5 → the 6th scanned token trips; U6 window > U3 cache)
  use generic_arraydeque::typenum::U6;

  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut emitter = Verbose::<NumErr>::new();
  let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(5),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    let mut txn = inp.begin_stacked();
    let sp = txn.savepoint(); // before the trip
    let _ = txn.peek::<U6>().unwrap(); // overflow trip inside the guard
    assert!(
      txn.is_poisoned(),
      "the overflow trip latches poison inside the guard"
    );

    txn.rollback_to(sp);
    assert!(
      !txn.is_poisoned(),
      "rollback_to un-latches the speculative poison boundary"
    );

    // The committed path re-reaches the trip and re-latches.
    while txn.next().unwrap().is_some() {}
    assert!(txn.is_poisoned(), "the committed re-lex re-latches poison");
    txn.commit();
  }

  let total: usize = emitter.errors().values().map(|g| g.len()).sum();
  assert_eq!(
    total, 1,
    "the limit diagnostic is emitted exactly once in total"
  );
}

#[test]
fn stacked_best_match_selection() {
  // The motivating pattern end-to-end: parse several stages, keep a fallback savepoint
  // after each, score them, then roll back to the best-scoring one and resume from
  // exactly there — the younger fallbacks die with the rollback.
  let mut input = silent_input("1 2 3 4 5 6");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let mut txn = inp.begin_stacked();

  // Three stages; the middle one scores highest.
  let scores = [1i32, 5, 2];
  let mut candidates = Vec::new();
  for s in scores {
    let _ = txn.next().unwrap().expect("stage token");
    candidates.push((s, txn.savepoint()));
  }

  // Select the best-scoring savepoint (the middle stage, sitting after "2").
  let (_, best) = candidates
    .iter()
    .copied()
    .max_by_key(|(s, _)| *s)
    .expect("three candidates");

  txn.rollback_to(best);

  assert_eq!(
    *txn.next().unwrap().expect("resume at 3").span_ref(),
    SimpleSpan::new(4, 5),
    "the stream resumes exactly at the best-scoring savepoint"
  );
  assert_eq!(
    txn.saves.len(),
    2,
    "rolling back to the middle savepoint destroyed the younger one"
  );

  txn.commit();
}

// ── Commit drop policy (begin_stacked_with::<Commit>) ────────────────────────────
//
// The dual of the speculative default for the stacked guard: an undecided
// `Commit`-policy transaction KEEPS its progress on drop (forgetting its savepoints and
// base). `savepoint`/`rollback_to`/`release` and `commit`/`rollback` all work as usual.

#[test]
fn stacked_commit_policy_drop_keeps_progress() {
  // An undecided Commit-policy stacked guard (with a live savepoint) keeps its progress on
  // drop — the opposite of the Rollback default.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let start = *inp.cursor().as_inner();
  {
    let mut txn = inp.begin_stacked_with::<Commit>();
    let _ = txn.next().unwrap().expect("1");
    let _sp = txn.savepoint();
    let _ = txn.next().unwrap().expect("2");
    // `txn` drops undecided → Commit policy keeps the progress, forgetting savepoint + base.
  }
  assert!(
    *inp.cursor().as_inner() > start,
    "dropping an undecided Commit-policy stacked guard keeps the consumed progress"
  );
  assert_eq!(
    *inp.next().unwrap().expect("3").span_ref(),
    SimpleSpan::new(4, 5),
    "the input resumed past the kept tokens"
  );
}

#[test]
fn stacked_commit_policy_explicit_commit_keeps() {
  // `commit` keeps progress whatever the policy.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let start = *inp.cursor().as_inner();
  let mut txn = inp.begin_stacked_with::<Commit>();
  let _ = txn.next().unwrap().expect("1");
  let _sp = txn.savepoint();
  let _ = txn.next().unwrap().expect("2");
  txn.commit();

  assert!(
    *inp.cursor().as_inner() > start,
    "explicit commit on a Commit-policy stacked guard keeps progress"
  );
}

#[test]
fn stacked_commit_policy_explicit_rollback_restores() {
  // `rollback` restores to the begin point whatever the policy: a Commit-policy stacked
  // guard can still be rolled back explicitly.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let start = *inp.cursor().as_inner();
  let mut txn = inp.begin_stacked_with::<Commit>();
  let _ = txn.next().unwrap().expect("1");
  let _sp = txn.savepoint();
  let _ = txn.next().unwrap().expect("2");
  txn.rollback();

  assert_eq!(
    *inp.cursor().as_inner(),
    start,
    "explicit rollback on a Commit-policy stacked guard restores to the begin point"
  );
  assert_eq!(
    *inp.next().unwrap().expect("1 again").span_ref(),
    SimpleSpan::new(0, 1),
    "the consumed tokens replay after the explicit rollback"
  );
}

#[test]
fn stacked_commit_policy_keeps_progress_on_fatal_error() {
  // The Fatal-emitter case for the stacked guard: an error propagating via `?` out of a
  // Commit-policy stacked guard drops it and KEEPS the progress consumed up to the error
  // (forgetting the live savepoint and base), never rolling back.
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut emitter = Fatal::<NumErr>::new();
  let mut input = Input::<NumLexer<'_>, NumFatalCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  fn drive<'inp>(
    inp: &mut InputRef<'inp, '_, NumLexer<'inp>, NumFatalCtx<'inp>>,
  ) -> Result<(), NumErr> {
    let mut txn = inp.begin_stacked_with::<Commit>();
    let _ = txn.next()?; // consume "1"
    let _sp = txn.savepoint();
    let _ = txn.next()?; // cross "@": Fatal emits Err → `?` drops the guard (Commit: keep)
    txn.commit();
    Ok(())
  }

  let start = *inp.cursor().as_inner();
  let result = drive(&mut inp);
  assert!(
    result.is_err(),
    "the fatal lexer error propagated out of the stacked guard"
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
    "the input resumed just past the consumed `@` — the stacked guard kept its progress"
  );
}

#[cfg(all(
  debug_assertions,
  any(feature = "std", feature = "alloc"),
  target_has_atomic = "ptr"
))]
#[test]
fn stacked_commit_removes_all_ids_from_live_stack() {
  // Committing keeps the parsed progress but forgets the base and every savepoint id, so
  // the debug live-checkpoint stack does not grow across commit-heavy loops.
  let mut input = silent_input("1 2 3 4 5 6");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let baseline = inp.live_checkpoints_len();
  for _ in 0..100 {
    let mut txn = inp.begin_stacked();
    let _ = txn.next().unwrap();
    let _ = txn.savepoint();
    let _ = txn.next().unwrap();
    let _ = txn.savepoint();
    txn.commit();
  }
  assert_eq!(
    inp.live_checkpoints_len(),
    baseline,
    "commit forgets the base and every savepoint id — the live stack returns to baseline"
  );
}

// ── Savepoint lineage validity (all builds) ─────────────────────────────────────
//
// A `SavepointId` passing the nonce + `saves`-membership check is not enough: the
// checkpoint the slot holds must still be on a live lineage. A raw restore through the
// transaction (`DerefMut`) to a checkpoint older than a savepoint, or replacing the lexer
// state, invalidates the savepoint's checkpoint without removing its `saves` entry. Using
// it afterwards must panic as stale in *every* build (plain `Vec` membership, no atomics),
// making the docs' promise true on release and no-`target_has_atomic`-ptr targets alike.

#[test]
#[should_panic(expected = "stacked transaction: savepoint is stale")]
fn stacked_savepoint_after_raw_restore_panics_stale() {
  // A raw checkpoint saved *before* a savepoint, then restored through the transaction,
  // rolls the lineage back past the savepoint's checkpoint while leaving its `seq` in
  // `saves`. The nonce + membership check alone waves the stale id through; the lineage
  // check must reject it.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let mut txn = inp.begin_stacked();
  let _ = txn.next().unwrap().expect("1");
  let raw = txn.save(); // raw checkpoint, older than the savepoint
  let _ = txn.next().unwrap().expect("2");
  let sp = txn.savepoint(); // younger: its lineage sits above `raw`
  txn.restore(raw); // rolls back past the savepoint's checkpoint
  txn.rollback_to(sp); // `sp` is stale → panic in every build
}

#[test]
fn stacked_savepoint_survives_state_surgery() {
  // FLIPPED from the old `stacked_savepoint_after_set_state_panics_stale`. State surgery is
  // transactional, not invalidating (the pre-copy restore architecture pure-copies every
  // lineage fact), so a savepoint taken before it REMAINS VALID: `rollback_to` it undoes the
  // surgery. At HEAD `set_state` cleared the live lineage, so this rollback_to panicked as
  // stale. The savepoint's checkpoint captures a poisoned pre-surgery state; rolling back to
  // it restores the regime, the poison boundary, the dedup watermark, and the position, and
  // scanning resumes under the OLD regime.
  //   "1 @ 3 4": `@` is a plain lexer error; the limit-2 limiter trips on the 3rd number.
  use generic_arraydeque::typenum::U6;

  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut emitter = Verbose::<NumErr>::new();
  let mut input = Input::<NumLexer<'_>, NumVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 3 4",
    TokenLimiter::with_limitation(2),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);
    let mut txn = inp.begin_stacked();

    // A speculative peek seals `@`'s lexer error above the cursor (lifting the dedup
    // watermark) and trips the limiter (latching the poison boundary), without consuming.
    let _ = txn.peek::<U6>().unwrap();
    assert!(
      txn.is_poisoned(),
      "the peek trips the limiter and latches poison"
    );
    let sp = txn.savepoint(); // marks the poisoned pre-surgery state

    // State surgery through the guard's deref: re-keys the boundary away.
    txn.set_state(TokenLimiter::with_limitation(usize::MAX));
    assert!(!txn.is_poisoned(), "the surgery re-keys the boundary away");

    // At HEAD this panicked as stale; post-fix `sp` is still live and rolls back across it.
    txn.rollback_to(sp);

    // The surgery is undone: the pre-surgery regime, boundary, and position returned.
    assert!(
      txn.is_poisoned(),
      "boundary: the pre-surgery poison boundary returned"
    );
    assert_eq!(
      txn.state().limitation(),
      2,
      "regime: the pre-surgery limiter returned"
    );
    assert_eq!(
      txn.state().tokens(),
      0,
      "regime: the saved counter returned"
    );
    assert_eq!(
      *txn.cursor().as_inner(),
      0,
      "position: rolled back to the savepoint"
    );

    // Scanning resumes under the OLD regime: the prefix replays and the stream stops at the
    // restored boundary; the restored watermark keeps `@` deduplicated (no duplicate).
    let mut replayed = Vec::new();
    while let Some(tok) = txn.next().unwrap() {
      replayed.push(*tok.span_ref());
    }
    assert_eq!(
      replayed,
      vec![SimpleSpan::new(0, 1), SimpleSpan::new(4, 5)],
      "the prefix replays and the stream stops at the restored boundary"
    );
    txn.commit();
  }

  let total: usize = emitter.errors().values().map(|g| g.len()).sum();
  assert_eq!(
    total, 2,
    "watermark: `@` and the limit diagnostic each stay exactly once — no duplicate on replay"
  );
}

#[test]
fn stacked_base_drop_undoes_state_surgery() {
  // The base-drop twin: dropping an undecided stacked transaction after state surgery rolls
  // back to the begin point, UNDOING the surgery (it is transactional). The base checkpoint
  // captures a poisoned pre-surgery state; the drop restores it.
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut emitter = Silent::<NumErr>::new();
  let mut input = Input::<NumLexer<'_>, NumCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);
    // Trip the limiter before opening the transaction, so the base checkpoint is poisoned.
    assert!(inp.next().unwrap().is_some(), "1");
    assert!(inp.next().unwrap().is_some(), "2");
    assert!(inp.next().unwrap().is_none(), "the 3rd scan trips → None");
    assert!(inp.is_poisoned(), "the trip latched the poison boundary");
    let after_trip = *inp.cursor().as_inner();

    {
      let mut txn = inp.begin_stacked();
      let _sp = txn.savepoint();
      txn.set_state(TokenLimiter::with_limitation(usize::MAX));
      assert!(
        !txn.is_poisoned(),
        "the surgery re-keys the boundary away inside the guard"
      );
      // `txn` drops undecided → rollback to base, undoing the surgery.
    }

    assert!(
      inp.is_poisoned(),
      "the base-drop restored the pre-surgery poison boundary"
    );
    assert_eq!(
      *inp.cursor().as_inner(),
      after_trip,
      "the base-drop rolled back to the trip point"
    );
    assert_eq!(
      inp.state().limitation(),
      2,
      "the pre-surgery regime returned"
    );
    assert!(
      inp.next().unwrap().is_none(),
      "the restored boundary stops scanning (old regime)"
    );
  }
}

#[test]
fn stacked_savepoints_survive_nested_attempt_and_transaction() {
  // False-positive guard: legal nested speculation, and a LIFO-clean raw save/restore pair
  // entirely *above* the savepoints, must not disturb them. Two savepoints stay valid
  // across a failing attempt, a succeeding try_attempt, a nested begin/commit and a nested
  // begin/rollback, and a raw save/restore pair — and rollback_to each still resumes at
  // exactly its mark.
  let mut input = silent_input("1 2 3 4 5 6 7 8");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let mut txn = inp.begin_stacked();
  let _ = txn.next().unwrap().expect("1");
  let sp1 = txn.savepoint(); // after "1"
  let _ = txn.next().unwrap().expect("2");
  let sp2 = txn.savepoint(); // after "2"

  // A failing attempt consumes a token then rolls back.
  let attempted: Option<()> = txn.attempt(|inp| {
    let _ = inp.next().unwrap();
    None
  });
  assert!(attempted.is_none(), "the attempt failed and rolled back");

  // A succeeding try_attempt keeps its (empty) progress.
  let tried: Result<(), ()> = txn.try_attempt(|_inp| Ok(()));
  assert!(tried.is_ok(), "the try_attempt succeeded");

  // A nested Transaction that commits its progress, then one that rolls back.
  {
    let mut nested = txn.begin();
    let _ = nested.next().unwrap().expect("3");
    nested.commit();
  }
  {
    let mut nested = txn.begin();
    let _ = nested.next().unwrap().expect("4");
    nested.rollback();
  }

  // A LIFO-clean raw save/restore pair entirely above the savepoints.
  let raw = txn.save();
  let _ = txn.next().unwrap().expect("4 again");
  txn.restore(raw);

  // Both savepoints survived. Youngest first keeps the elder valid (SQL parity).
  txn.rollback_to(sp2);
  assert_eq!(
    *txn.next().unwrap().expect("resume at 3").span_ref(),
    SimpleSpan::new(4, 5),
    "sp2 survived the nested work and resumes exactly at its mark",
  );
  txn.rollback_to(sp1);
  assert_eq!(
    *txn.next().unwrap().expect("resume at 2").span_ref(),
    SimpleSpan::new(2, 3),
    "sp1 survived and resumes exactly at its mark",
  );
  txn.commit();
}

// ── Raw restore below a live base panics AT THE RESTORE (detect-at-cause) ────────
//
// The base-checkpoint twin of the detect-at-cause rule. A raw restore below the base (a
// checkpoint saved BEFORE `begin_stacked`) would pop the pinned base off the live lineage.
// The checked `restore` path refuses it loudly, right where it is requested, in every
// allocator build. The savepoint staleness rule below still detects-at-use: savepoints are
// not pinned (only the base is), so a raw restore that invalidates only a savepoint — above
// the base — succeeds, and its `rollback_to` panics as stale.

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn stacked_raw_restore_below_base_panics_at_the_restore() {
  // The stacked twin of the scenario converted from `stacked_base_drop_after_raw_restore_
  // below_base_does_not_resurrect`. save A, begin_stacked (base above A), savepoint, consume,
  // then raw-restore to A through the guard. A predates the guard's begin point, so restoring
  // it would invalidate the pinned base (and every savepoint above it) — the pin check panics
  // AT THE RESTORE. At HEAD the restore succeeded and the rolling-back drop silently committed
  // the abandoned work.
  let mut input = silent_input("1 2 3 4 5");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save(); // raw checkpoint, below the guard's begin point
  let _ = inp.next().unwrap().expect("consume 1"); // advance past A before begin

  let mut txn = inp.begin_stacked(); // base checkpoint, above A
  let _ = txn.next().unwrap().expect("consume 2");
  let _sp = txn.savepoint(); // a live savepoint above the base
  let _ = txn.next().unwrap().expect("consume 3");
  txn.restore(a); // POST-FIX: panics — restoring A would pop the pinned base off the lineage
}

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn stacked_explicit_rollback_after_raw_restore_below_base_panics_at_restore() {
  // Converted from `stacked_explicit_rollback_after_raw_restore_below_base_panics_stale`. The
  // panic now fires AT THE RAW RESTORE, before the explicit `rollback`. At HEAD the raw restore
  // succeeded and `rollback` panicked as stale ("transaction base is stale"); post-fix the
  // pinned restore panics first.
  let mut input = silent_input("1 2 3 4 5");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save();
  let _ = inp.next().unwrap().expect("consume 1");

  let mut txn = inp.begin_stacked();
  let _ = txn.next().unwrap().expect("consume 2");
  txn.restore(a); // POST-FIX: panics here (was: succeeded, then `rollback` panicked as stale)
  txn.rollback(); // unreachable post-fix
}

#[test]
fn stacked_rollback_to_savepoint_with_live_base_is_legal() {
  // Negative control: `rollback_to` restores a savepoint's checkpoint, which sits ABOVE the
  // pinned base — so it pops only savepoints, never the base. The base stays pinned and the
  // savepoint stays reusable; the pin check must not fire on this ordinary savepoint rollback.
  let mut input = silent_input("1 2 3 4 5");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let mut txn = inp.begin_stacked(); // base pinned
  let _ = txn.next().unwrap().expect("consume 1");
  let sp = txn.savepoint(); // above the base
  let at_sp = *txn.cursor().as_inner();
  let _ = txn.next().unwrap().expect("consume 2");
  let _ = txn.next().unwrap().expect("consume 3");
  txn.rollback_to(sp); // legal: restores the savepoint (above the base), base untouched
  assert_eq!(
    *txn.cursor().as_inner(),
    at_sp,
    "rolled back to the savepoint"
  );
  txn.rollback_to(sp); // still reusable after the first rollback_to
  assert_eq!(
    *txn.cursor().as_inner(),
    at_sp,
    "the savepoint is reusable — the pinned base was never disturbed"
  );
  txn.commit();
  assert_eq!(
    *inp.cursor().as_inner(),
    at_sp,
    "committed at the savepoint position"
  );
}

// ── The unwind posture of an undecided Commit-policy stacked guard ───────────────
//
// `StackedTransaction::drop` carries the same policy branch and the same unwind-blind
// commit arm as its sibling. This is the sibling's own witness, not a copy of it: it
// additionally checks that the savepoint's emitter mark was settled, which needs an
// emitter whose marks are TABLE-keyed (a live row per capture) rather than value-keyed.

/// A mark-keyed emitter: one live row per outstanding capture, settled by `rewind`
/// (the capture and every younger one) or `release` (that capture alone). `Verbose`
/// cannot show this — its mark is a log length, so a stranded mark is invisible.
#[derive(Debug, Default)]
struct MarkLedger {
  next: core::cell::Cell<u64>,
  live: core::cell::RefCell<std::vec::Vec<u64>>,
}

impl MarkLedger {
  fn live_rows(&self) -> usize {
    self.live.borrow().len()
  }
}

impl<'inp, L, Lang: ?Sized> crate::Emitter<'inp, L, Lang> for MarkLedger
where
  L: crate::Lexer<'inp>,
  <L::Token as Token<'inp>>::Error: Into<NumErr>,
{
  type Error = NumErr;

  fn emit_lexer_error(
    &mut self,
    err: crate::span::Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), NumErr> {
    let _ = err;
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), NumErr> {
    let _ = err;
    Ok(())
  }

  fn emit_error(&mut self, err: crate::span::Spanned<NumErr, L::Span>) -> Result<(), NumErr> {
    let _ = err;
    Ok(())
  }

  fn checkpoint(&self) -> u64 {
    let id = self.next.get() + 1;
    self.next.set(id);
    self.live.borrow_mut().push(id);
    id
  }

  fn rewind(&mut self, _cursor: &crate::input::Cursor<'inp, '_, L>, checkpoint: u64) {
    self.live.borrow_mut().retain(|m| *m < checkpoint);
  }

  fn release(&mut self, checkpoint: u64) {
    let mut live = self.live.borrow_mut();
    if let Some(pos) = live.iter().rposition(|m| *m == checkpoint) {
      live.remove(pos);
    }
  }
}

type NumLedgerCtx<'a> = (MarkLedger, DefaultCache<'a, NumLexer<'a>>);

#[test]
fn stacked_commit_guard_dropped_mid_unwind_rolls_back() {
  let cache = DefaultCache::<'_, NumLexer<'_>>::default();
  let mut emitter = MarkLedger::default();
  let mut input = Input::<NumLexer<'_>, NumLedgerCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let mut txn = inp.begin_stacked_with::<Commit>();
    let _ = txn
      .next()
      .unwrap()
      .expect("the half-iteration consumes `1`");
    let _sp = txn.savepoint();
    let _ = txn.next().unwrap().expect("and `2`");
    panic!("D27: the stacked region panics with the guard undecided");
  }));
  assert!(caught.is_err(), "the panic was caught by this host");

  assert_eq!(
    *inp
      .next()
      .unwrap()
      .expect("a token after the caught panic")
      .span_ref(),
    SimpleSpan::new(0, 1),
    "an undecided Commit stacked guard dropped mid-unwind rolls back to its base"
  );
  assert_eq!(
    inp.emitter().live_rows(),
    0,
    "every mark the guard held — base and savepoint — was settled by the unwinding drop"
  );
}

#[test]
fn stacked_commit_guard_dropped_without_unwind_still_commits() {
  // The control: an ordinary undecided stacked `Commit` drop keeps its progress.
  let mut input = silent_input("1 2 3 4");
  let mut emitter = Silent::<NumErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  {
    let mut txn = inp.begin_stacked_with::<Commit>();
    let _ = txn.next().unwrap().expect("consume `1`");
    let _sp = txn.savepoint();
    let _ = txn.next().unwrap().expect("consume `2`");
  }

  assert_eq!(
    *inp
      .next()
      .unwrap()
      .expect("a token after the ordinary drop")
      .span_ref(),
    SimpleSpan::new(4, 5),
    "an ordinary undecided stacked Commit drop still commits"
  );
}
