use core::{
  marker::PhantomData,
  ops::{Deref, DerefMut},
};

use super::{
  Checkpoint, Complete, Completeness, InputRef, Lexer, ParseContext,
  drop_policy::{DropPolicy, Rollback},
};

/// A scoped backtracking transaction over an [`InputRef`].
///
/// Semantically identical to [`save`](InputRef::save)/[`restore`](InputRef::restore),
/// with the restore discipline enforced by the borrow checker: while a nested
/// transaction is alive, its parent is inaccessible, so out-of-order rollbacks — the
/// one contract violation [`restore`](InputRef::restore) documents — do not compile.
/// Nested transactions behave like database savepoints: rolling back a parent discards
/// everything its children committed.
///
/// [`commit`](Self::commit) and [`rollback`](Self::rollback) both consume the
/// transaction and are available whatever the policy. Zero-cost:
/// [`begin`](InputRef::begin) performs exactly one [`save`](InputRef::save), the guard
/// is two words, and deciding is one branch — there is no journaling, because the input
/// source is immutable and rewinding is a snapshot copy.
///
/// # Drop policy
///
/// The final type parameter `P` is a compile-time [`DropPolicy`](super::DropPolicy) that
/// fixes what an *undecided* guard does on drop:
///
/// - [`Rollback`](super::Rollback) (the default, from [`begin`](InputRef::begin)) — drop
///   restores to the begin point; uncommitted speculative work is discarded.
/// - [`Commit`](super::Commit) (from [`begin_with`](InputRef::begin_with)) — drop keeps
///   the progress, the dual used by commit-by-default loops.
///
/// # When to reach for it
///
/// Use `Transaction` for imperative flows with several exits (loops, `match` arms) —
/// [`begin`](InputRef::begin) for the speculative default, or
/// [`begin_with::<Commit>`](InputRef::begin_with) for a commit-by-default loop that keeps
/// progress on most exits and rolls back explicitly on the few that back out. Reach for
/// [`attempt`](InputRef::attempt)/[`try_attempt`](InputRef::try_attempt) for
/// single-closure speculation. Raw [`save`](InputRef::save)/[`restore`](InputRef::restore) sit
/// beneath all of these as the `unstable-raw` escape hatch, reachable only with that feature and
/// only where no guard shape fits.
///
/// The guards fit lexical scopes; for owned, externally-driven speculation — a driver that owns
/// its input and is stepped across separate calls — reach for
/// the [session points](crate::InputRef::begin_point); raw checkpoints sit beneath both.
///
/// # Compile-time last-in, first-out
///
/// A nested transaction mutably borrows its parent for as long as it is alive, so the
/// non-LIFO shape — deciding a parent while a child is still undecided — is a borrow
/// error, not a runtime panic:
///
/// ```compile_fail
/// use tokora::{InputRef, Lexer, ParseContext};
///
/// fn non_lifo<'inp, 'closure, L, Ctx>(input: &mut InputRef<'inp, 'closure, L, Ctx>)
/// where
///   L: Lexer<'inp>,
///   L::State: Clone,
///   Ctx: ParseContext<'inp, L>,
/// {
///   let mut outer = input.begin();
///   let mut inner = outer.begin();
///   outer.rollback(); // error: `outer` is mutably borrowed by `inner`
///   inner.commit();
/// }
/// ```
///
/// # Mixing with raw save/restore
///
/// The guard deref-coerces to [`InputRef`], so raw [`save`](InputRef::save) /
/// [`restore`](InputRef::restore) are reachable through it. A raw restore to a checkpoint saved
/// *before* the guard began would roll the lineage back past the guard's own begin-point
/// checkpoint, tearing out the region the guard borrows from its begin point forward. In
/// allocator builds the guard **pins** its begin point, so such a restore **panics at the
/// restore itself** (`restore would invalidate a live transaction guard or attempt …`) — the
/// violation is refused where it is caused, before any commit/rollback decision. A LIFO-clean
/// raw save/restore pair taken and released entirely *above* the begin point, and state surgery
/// (which is transactional), leave the guard's checkpoint intact and never trip the pin. Such a
/// raw checkpoint should itself end in [`restore`](InputRef::restore) or
/// [`commit`](InputRef::commit) — dropping it strands its lineage entry, exactly as in
/// standalone raw use.
///
/// On allocator-less targets there is no pin set and no lineage stack, so this mixing is
/// unspecified-but-bounded rather than checked. In allocator builds the older detect-at-use
/// behaviors remain as backstops behind the pin check — an explicit [`rollback`](Self::rollback)
/// still asserts a live base, a rolling-back drop still skips a stale one — defense in depth that
/// the pin check now makes unreachable in ordinary use.
///
/// This entire mixing surface exists only with the `unstable-raw` feature. Without it, raw
/// [`save`](InputRef::save) / [`restore`](InputRef::restore) are crate-internal, so a downstream
/// crate cannot express a raw restore beneath a live guard at all — the hazard is unrepresentable
/// there, and the guard is the whole story.
pub struct Transaction<
  'txn,
  'inp,
  'closure,
  L,
  Ctx,
  Lang: ?Sized = (),
  P: DropPolicy = Rollback,
  Cmpl = Complete,
> where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  pub(super) input: &'txn mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  /// `Some` while the transaction is undecided; `None` once
  /// [`commit`](Self::commit)/[`rollback`](Self::rollback) (or a deciding drop) has
  /// consumed it. Routing every decision through this one `Option::take` is what keeps
  /// `commit`, `rollback`, and `Drop` from ever acting twice.
  pub(super) ckp: Option<Checkpoint<'inp, 'closure, L>>,
  /// The drop policy — [`Rollback`](super::Rollback) or [`Commit`](super::Commit) —
  /// carried as a zero-sized typestate. It selects, at compile time and branch-free, what
  /// an undecided guard's `Drop` does: restore to the begin point, or keep the progress.
  pub(super) _policy: PhantomData<P>,
}

impl<'inp, L, Ctx, Lang: ?Sized, P: DropPolicy, Cmpl>
  Transaction<'_, 'inp, '_, L, Ctx, Lang, P, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Commits the transaction: keeps the progress parsed through the guard and drops the
  /// begin-point checkpoint without restoring. Available whatever the drop policy.
  #[inline]
  pub fn commit(mut self) {
    trace_event!(self.input, "commit");
    // Take the checkpoint so the `Drop` guard below sees `None` and does not roll back.
    if let Some(ckp) = self.ckp.take() {
      // Kept, not restored: unpin the begin point, then the kept-checkpoint funnel drops its
      // lineage id and releases its emitter mark, so none of the three linger across
      // commit-heavy loops.
      #[cfg(any(feature = "std", feature = "alloc"))]
      self.input.unpin_checkpoint(ckp.ckp_id);
      self.input.forget_kept_checkpoint(ckp);
    }
  }

  /// The whole body of an *undecided* guard's [`Drop`], deliberately **outlined** — see the
  /// `Drop` impl for why that is load bearing rather than tidy.
  ///
  /// The policy is not the only input: an undecided guard dropped while the thread is
  /// **unwinding** rolls back whatever its policy says (std builds), because a panic aborts the
  /// region rather than completing it. See [`Commit`](super::Commit) for the posture and for
  /// the `no_std` divergence.
  #[cold]
  #[inline(never)]
  fn settle_undecided(&mut self) {
    let Some(ckp) = self.ckp.take() else { return };
    // A panic in flight overrides the policy: an unwind is an abort of the region, not its
    // completion, so it must not promote speculative progress. `Rollback` const-folds this to
    // `true || _` and keeps a byte-identical arm; `Commit` pays one TLS read, on the
    // undecided-drop path only — this body, which is already `#[cold] #[inline(never)]`.
    if P::ROLLBACK_ON_DROP || super::drop_policy::unwinding() {
      trace_event!(self.input, "rollback");
      // Unpin the begin point first — exception-safe, so it happens even though the rewind
      // below may be skipped (a `Drop` may run mid-unwind, where panicking is forbidden). The
      // pin check makes the base go-stale case unreachable in allocator builds, so this
      // normally just rewinds; the skip stays as a backstop. An explicit `rollback` reports a
      // stale base loudly; here we stay silent and truthful.
      #[cfg(any(feature = "std", feature = "alloc"))]
      self.input.unpin_checkpoint(ckp.ckp_id);
      self.input.restore_unchecked_if_live(ckp);
    } else {
      trace_event!(self.input, "commit");
      // Commit-on-drop: progress kept; unpin the begin point, then the kept-checkpoint funnel
      // forgets its lineage id and releases its emitter mark (as `commit` does). The funnel is
      // assert-free, so this arm stays silent even mid-unwind.
      #[cfg(any(feature = "std", feature = "alloc"))]
      self.input.unpin_checkpoint(ckp.ckp_id);
      self.input.forget_kept_checkpoint(ckp);
    }
  }

  /// Rolls the transaction back: returns the input to the begin point — position, span,
  /// lexer state, emission log, dedup watermark, and poison boundary all restored.
  /// Available whatever the drop policy (a [`Commit`](super::Commit) guard can still be
  /// rolled back explicitly).
  #[inline]
  pub fn rollback(mut self) {
    trace_event!(self.input, "rollback");
    if let Some(ckp) = self.ckp.take() {
      // Unpin the begin point FIRST so the checked restore below does not see it as pinned — a
      // guard rolling back to its own base is legal. A raw restore *below* the base (through
      // this guard's `DerefMut`) would already have panicked at that restore (detect-at-cause),
      // so the stale assert here is now an unreachable backstop, kept for defense in depth and
      // for allocator-less builds. (A rolling-back drop, which may run mid-unwind, quietly skips
      // the restore instead.)
      #[cfg(any(feature = "std", feature = "alloc"))]
      {
        self.input.unpin_checkpoint(ckp.ckp_id);
        assert!(
          self.input.live_contains(ckp.ckp_id),
          "transaction base is stale (invalidated by an earlier restore)"
        );
      }
      self.input.restore(ckp);
    }
  }
}

impl<'inp, 'closure, L, Ctx, Lang: ?Sized, P: DropPolicy, Cmpl> Deref
  for Transaction<'_, 'inp, 'closure, L, Ctx, Lang, P, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  type Target = InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>;

  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    self.input
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, P: DropPolicy, Cmpl> DerefMut
  for Transaction<'_, 'inp, '_, L, Ctx, Lang, P, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.input
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, P: DropPolicy, Cmpl> Drop
  for Transaction<'_, 'inp, '_, L, Ctx, Lang, P, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Decides an undecided transaction according to its [`DropPolicy`](super::DropPolicy).
  /// After [`commit`](Self::commit)/[`rollback`](Self::rollback) the checkpoint is
  /// already taken, so this is a no-op whatever the policy.
  ///
  /// - [`Rollback`](super::Rollback): restore to the begin point (the database default,
  ///   uncommitted work discarded).
  /// - [`Commit`](super::Commit): keep the progress, only forgetting the checkpoint's
  ///   lineage id — identical to dropping a raw [`Checkpoint`], including during an error
  ///   `?`-propagation under a fail-fast emitter.
  ///
  /// `P::ROLLBACK_ON_DROP` is a compile-time constant, so each policy monomorphizes to
  /// one arm with the other eliminated. Either arm is silent (no debug raw-misuse panic):
  /// `Drop` may run while already unwinding, where `no_std` has no `thread::panicking()`
  /// to guard a drop-bomb. Both arms first unpin the begin point (exception-safe — it happens
  /// even on the rollback arm's skip). The pin check makes a raw restore below the begin point
  /// panic at that restore, so the base cannot go stale while the guard is live and the rollback
  /// arm normally just rewinds; the stale-base skip it still performs is a backstop (defense in
  /// depth, and the behavior for allocator-less builds, which pin nothing).
  ///
  /// **This body is one branch.** Everything it does when the branch is taken lives out of line
  /// in the crate-private `settle_undecided`, and that is load bearing rather than tidy: the
  /// lineage's `unpin`/`forget` are `inline(always)` stack scans and the drop-path rewind is an
  /// `inline(always)` full restore — and a destructor is emitted at *every unwind edge* of its
  /// owner. Inlined, all of that lands in the cleanup path of every hot loop that speculates
  /// through a guard, including [`attempt`](InputRef::attempt) /
  /// [`try_attempt`](InputRef::try_attempt), which hold their begin point in one of these across
  /// the call into user code. Measured on the `attempt_decline_per_token` benchmark: inlined,
  /// +30%; outlined, +6% — and the commit-by-default guard got 27% *faster*. It is the same
  /// shape, and the same reason, the session cell's `Drop` is outlined for.
  #[inline]
  fn drop(&mut self) {
    if self.ckp.is_some() {
      self.settle_undecided();
    }
  }
}

#[cfg(all(test, feature = "logos", feature = "std"))]
mod tests;
