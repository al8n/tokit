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
/// [`commit`](Self::commit), [`rollback`](Self::rollback) and
/// [`rollback_abandoning_points`](Self::rollback_abandoning_points) all consume the transaction
/// and are available whatever the policy. The two rollbacks differ in exactly one thing — what
/// they do about a session point opened *inside* the guard and left open — and each names the
/// other, so the choice is made once and in the open.
///
/// **What it actually costs, since "the guard is two words" was never true.**
/// [`begin`](InputRef::begin) performs exactly one [`save`](InputRef::save), deciding is one
/// branch, and there is no journaling — the input source is immutable and rewinding is a
/// snapshot copy. Those op-count claims hold. The *size* claim did not: the guard embeds a
/// whole [`Checkpoint`](crate::input::Checkpoint), which is a cursor, the span, `L::State`,
/// two `u64` marks and the poison boundary — 88 bytes with a near-stateless test lexer, and
/// `O(size_of::<L::State>())` in general. A lexer with a large state pays for it here.
///
/// The distinction matters because the two claims support different decisions: the op counts
/// are why a speculative parse is cheap to *run*, and the size is what a caller holding many
/// nested guards is actually paying.
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
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " the [session points](crate::InputRef::begin_point); raw checkpoints sit beneath both."
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " the session points; raw checkpoints sit beneath both."
)]
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
/// behaviors remain as backstops behind the pin check — both explicit rollbacks still assert a
/// live base, a rolling-back drop still skips a stale one — defense in depth that the pin check now
/// makes unreachable in ordinary use.
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
  ///
  /// This is the **checked** rollback, and it stays the one to reach for. A rewind that would
  /// cross a still-open session point younger than the begin point is *refused* — it panics at
  /// this call, in every allocator build, before anything is restored — because a point still
  /// open above the base means the code that opened it lost track of its own speculation, and
  /// this is where that is cheapest to find. When the scope spans foreign code that may
  /// legitimately open and abandon a point, that refusal is the wrong answer and
  /// [`rollback_abandoning_points`](Self::rollback_abandoning_points) is the verb; it says why.
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

  /// Rolls the transaction back the way a **rolling-back drop** of this guard does: restores to
  /// the begin point after *abandoning* every session point opened inside the guard and left
  /// open, rather than refusing to rewind across one. Available whatever the drop policy.
  ///
  /// Everything [`rollback`](Self::rollback) restores, this restores identically — position,
  /// span, lexer state, emission log, dedup watermark, poison boundary — and with no open point
  /// above the base the two are indistinguishable. What differs is the one case that separates
  /// them.
  ///
  /// # Which of the two rollbacks to reach for
  ///
  /// [`rollback`](Self::rollback) is the checked restore and the default. A session point still
  /// open above the begin point pins its base, and the pin makes that rewind **panic where it is
  /// requested** — in every allocator build, before anything is restored — exactly as a raw
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " [`restore`](InputRef::restore) below a live point is refused. Keep `rollback` wherever this"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " `restore` below a live point is refused. Keep `rollback` wherever this"
  )]
  /// scope owns every point opened inside it: there the refusal is a real bug detector, and
  /// giving it up costs more than it saves.
  ///
  /// Reach for `rollback_abandoning_points` when the guard's scope spans **foreign code that may
  /// legitimately open a point and abandon it** — a grammar hook, a caller-supplied closure, a
  /// parser you were handed. Abandoning a point is legal, and deliberately so
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " (see [`begin_point`](InputRef::begin_point)); refusing this rollback would turn someone"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " (see `begin_point`); refusing this rollback would turn someone"
  )]
  /// else's legal choice into *your* release panic — and into one raised before the restore, so
  /// the speculative progress this rollback exists to retract is still committed for any host
  /// that catches. That is strictly worse than the state the rollback was asking for.
  ///
  /// What this verb does instead is the input layer's own answer for a point an enclosing
  /// rollback reaches below: every point younger than the base is abandoned — unpinned, its
  /// lineage entry dropped, its emitter mark released — and then this restore subsumes its
  /// progress. It is not a new behaviour, it is the **reconciliation an undecided
  /// [`Rollback`](super::Rollback) guard's `Drop` already performs**, made sayable on the
  /// explicit path so a scope whose two exits are "drop" and "roll back here" can restore the
  /// same thing on both. An abandoned point is gone: settling it afterwards is refused by the
  /// session verb itself, which is the same outcome the drop path has always produced.
  ///
  /// # Where the crate itself reaches for it
  ///
  /// Every in-crate scope that hands a whole [`InputRef`] to code it did not write, and nowhere
  /// else: the restoring arms of [`attempt`](InputRef::attempt),
  /// [`try_attempt`](InputRef::try_attempt) and [`attempt_parse`](InputRef::attempt_parse) (a
  /// caller-supplied closure), and the typed pratt driver's expression guard and five
  /// cycle-scoped probe exits (grammar hooks). The test is not "could a point be open here" but
  /// "**who** would have opened it": in all of these it is someone the scope cannot hold to a
  /// settle discipline, and whose unwind edge through the same guard already reconciles. Scopes
  /// that own their own points — every other rollback in this crate — keep
  /// [`rollback`](Self::rollback) and its refusal.
  ///
  /// # Panics
  ///
  /// Only where [`rollback`](Self::rollback) does for a reason unrelated to points: a begin
  /// point invalidated by an earlier restore below it (`transaction base is stale`). Unlike a
  /// rolling-back `Drop` — which may run mid-unwind, where panicking is forbidden, and so stays
  /// silent and skips — this is an explicit call on a normal return path and reports that
  /// misuse.
  ///
  /// # Fuzz coverage
  ///
  /// In the fuzz alphabet as `Op::TxnRollbackAbandoningPoints`, whose executor opens a point
  /// inside the guard and abandons it so the corpus exercises the one input that separates this
  /// verb from [`rollback`](Self::rollback); see `OP_SURFACE_CENSUS` in `src/fuzz/ops.rs`.
  #[inline]
  pub fn rollback_abandoning_points(mut self) {
    trace_event!(self.input, "rollback");
    if let Some(ckp) = self.ckp.take() {
      // Unpin the begin point FIRST, exactly as `rollback` does: a guard rolling back to its own
      // base is legal, and the reconciling rewind below abandons points by id order, so the base
      // must not still be sitting in the pin set when it goes.
      #[cfg(any(feature = "std", feature = "alloc"))]
      {
        self.input.unpin_checkpoint(ckp.ckp_id);
        assert!(
          self.input.live_contains(ckp.ckp_id),
          "transaction base is stale (invalidated by an earlier restore)"
        );
      }
      // The reconciling rewind — the SAME body the rolling-back `Drop` reaches through
      // `restore_unchecked_if_live`, whose liveness guard is the assert above rather than a
      // silent skip. Deliberately NOT `restore`: its pin check is precisely the refusal this
      // verb exists not to raise, and the remaining checks it performs are either replaced here
      // (liveness) or unreachable (a guard's own begin point is never foreign to its own input).
      self.input.restore_unchecked(ckp);
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

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod tests;
