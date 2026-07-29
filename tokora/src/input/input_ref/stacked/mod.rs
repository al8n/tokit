use core::{
  marker::PhantomData,
  ops::{Deref, DerefMut},
};

use super::super::SavepointStack;
use super::{
  Checkpoint, Complete, Completeness, InputRef, Lexer, ParseContext,
  drop_policy::{DropPolicy, Rollback},
};

/// An opaque handle to one savepoint inside a [`StackedTransaction`].
///
/// Returned by [`savepoint`](StackedTransaction::savepoint) and consumed by
/// [`rollback_to`](StackedTransaction::rollback_to) and
/// [`release`](StackedTransaction::release). It is a small `Copy` token that holds no
/// runtime borrow, so it can be stashed in a list of candidates or returned up the call
/// stack while the transaction stays open.
///
/// # Not a durable position token
///
/// A `SavepointId` is *branded* with the lifetime of the transaction that issued it, so
/// it cannot outlive that transaction or the input loan behind it — keeping one past the
/// parse is a compile error, not a dangling handle. For a position that must survive
/// beyond the transaction, capture a [`Cursor`](super::Cursor) or a span instead.
///
/// # How a misused id is caught
///
/// Identity is layered, from compile time down to a runtime check, with no global state
/// — no counter, no atomic — behind any of it:
///
/// - **Temporal misuse** — using an id after its transaction ended, or holding one across
///   the next [`begin_stacked`](InputRef::begin_stacked) on the same input — is a
///   **compile error**: the id's invariant lifetime brand keeps the input loan open while
///   the id is live, so the borrow checker rejects reopening it.
/// - **Cross-parser misuse** — an id from another, simultaneously-live transaction over a
///   *different* input — **panics in every build**: each id carries the address of an
///   Input-owned field, and two live inputs occupy distinct addresses.
/// - **Intra-transaction staleness** — an id destroyed by an earlier `rollback_to` /
///   `release` on the same transaction, or one whose checkpoint a raw restore below it
///   invalidated (see the mixing rules on [`StackedTransaction`]) — **panics in every
///   build** via a membership scan of the live savepoints and their lineage (see
///   [`rollback_to`](StackedTransaction::rollback_to)). State surgery is transactional and
///   does *not* invalidate a savepoint.
///
/// # Compile-time rejections
///
/// The temporal and nesting misuses never reach a runtime check — the borrow checker
/// rejects them. Each snippet below fails to compile.
///
/// Reusing an id after its transaction ended (here, across the next `begin_stacked` on the
/// same input) — the id keeps the first transaction's loan on `input` open, so the second
/// `begin_stacked` cannot re-borrow it:
///
/// ```compile_fail
/// use tokora::{InputRef, Lexer, ParseContext};
///
/// fn temporal_misuse<'inp, L, Ctx>(input: &mut InputRef<'inp, '_, L, Ctx>)
/// where
///   L: Lexer<'inp>,
///   L::State: Clone,
///   Ctx: ParseContext<'inp, L>,
/// {
///   let sp = {
///     let mut txn = input.begin_stacked();
///     let sp = txn.savepoint();
///     txn.commit();
///     sp
///   };
///   let mut txn2 = input.begin_stacked(); // error[E0499]: `input` is still borrowed by `sp`
///   txn2.rollback_to(sp);
/// }
/// ```
///
/// Storing an id past the parse (returning it out of the transaction) — the brand cannot
/// outlive the input loan:
///
/// ```compile_fail
/// use tokora::{InputRef, Lexer, ParseContext, SavepointId};
///
/// fn durable_misuse<'inp, 'closure, L, Ctx>(
///   input: &mut InputRef<'inp, 'closure, L, Ctx>,
/// ) -> SavepointId<'closure>
/// where
///   L: Lexer<'inp>,
///   L::State: Clone,
///   Ctx: ParseContext<'inp, L>,
/// {
///   let mut txn = input.begin_stacked();
///   let sp = txn.savepoint();
///   txn.commit();
///   sp // error: the id's brand does not outlive the transaction's loan
/// }
/// ```
///
/// Passing a parent savepoint into a nested child transaction — the parent's brand region
/// strictly contains the child's (the parent is used before the child exists), so the
/// invariant brands cannot unify:
///
/// ```compile_fail
/// use tokora::{InputRef, Lexer, ParseContext};
///
/// fn parent_id_in_child<'inp, L, Ctx>(input: &mut InputRef<'inp, '_, L, Ctx>)
/// where
///   L: Lexer<'inp>,
///   L::State: Clone,
///   Ctx: ParseContext<'inp, L>,
/// {
///   let mut parent = input.begin_stacked();
///   let sp_parent = parent.savepoint();
///   let mut child = parent.begin_stacked();
///   child.rollback_to(sp_parent); // error[E0597]: brands cannot unify parent with child
/// }
/// ```
///
/// The mirror — keeping a child savepoint to use in the parent after the child ends — also
/// fails: the child id keeps the child's borrow of the parent alive, so the parent cannot
/// be used:
///
/// ```compile_fail
/// use tokora::{InputRef, Lexer, ParseContext};
///
/// fn child_id_in_parent<'inp, L, Ctx>(input: &mut InputRef<'inp, '_, L, Ctx>)
/// where
///   L: Lexer<'inp>,
///   L::State: Clone,
///   Ctx: ParseContext<'inp, L>,
/// {
///   let mut parent = input.begin_stacked();
///   let sp_child = {
///     let mut child = parent.begin_stacked();
///     let s = child.savepoint();
///     child.commit();
///     s
///   };
///   parent.rollback_to(sp_child); // error: `parent` is still borrowed by the child id
/// }
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SavepointId<'t> {
  /// The savepoint's sequence number, drawn from an input-global counter that is never
  /// reset across the input's transactions. A seq is therefore unique for the whole life
  /// of the input, so an id that crosses transactions on one input (a nested or a
  /// sequential one) can never collide with a live savepoint's seq in another
  /// transaction's stack — the membership scan then panics deterministically as stale.
  seq: u64,
  /// The address of the issuing input's `poison_boundary` field, captured at
  /// [`begin_stacked`](InputRef::begin_stacked). Two simultaneously-live inputs are
  /// distinct structs at distinct addresses, so this separates their transactions. The
  /// lifetime brand — not this address — rules out the address-reuse case where a dropped
  /// input's slot is later reallocated, because a live id keeps its own input's loan open.
  nonce: usize,
  /// Invariant in `'t`, the transaction's borrow of the input. The fn-pointer form (not a
  /// bare reference, which would be covariant and defeat the brand) is what keeps the id
  /// from outliving its loan and makes a parent/child swap under nesting fail to unify.
  _brand: PhantomData<fn(&'t ()) -> &'t ()>,
}

/// A scoped backtracking transaction that holds several live savepoints at once,
/// mirroring SQL savepoint semantics.
///
/// The lean [`Transaction`](super::Transaction) captures a single begin point;
/// `StackedTransaction` adds an internal last-in, first-out stack of savepoints so a
/// parser can keep **several fallback positions live simultaneously** and return to any
/// of them. Reach for it when a single alternative is not enough — best- or
/// longest-match selection (mark a savepoint after each parsed stage, score them, then
/// [`rollback_to`](Self::rollback_to) the winner), multi-segment speculation with
/// fallback to any earlier boundary, or recovery scans juggling several anchor
/// candidates. For a single speculative alternative, prefer
/// [`Transaction`](super::Transaction); for closure-shaped speculation,
/// [`attempt`](InputRef::attempt) / [`try_attempt`](InputRef::try_attempt).
///
/// # SQL savepoint semantics
///
/// The four operations map onto SQL exactly:
///
/// | this type | SQL | effect |
/// |---|---|---|
/// | [`savepoint`](Self::savepoint) | `SAVEPOINT` | mark the current position, return an id |
/// | [`rollback_to`](Self::rollback_to) | `ROLLBACK TO` | return to a mark, destroy the younger savepoints, keep the mark |
/// | [`release`](Self::release) | `RELEASE SAVEPOINT` | forget a mark and the younger ones, keep the parsed progress |
/// | [`commit`](Self::commit) | `COMMIT` | keep everything, forget all savepoints |
/// | [`rollback`](Self::rollback) | `ROLLBACK` | return to the begin point, discard everything |
///
/// Rolling back to an older savepoint always destroys every newer one, so out-of-order
/// revival is impossible by construction — the [`restore`](InputRef::restore) discipline
/// holds because the internal stack only ever shrinks from the top. A misused
/// [`SavepointId`] is rejected in layers: a temporally-misused id at compile time via its
/// lifetime brand, a foreign id from another live parser and a stale id both by a runtime
/// check in every build — see [`SavepointId`].
///
/// [`commit`](Self::commit) and [`rollback`](Self::rollback) consume the transaction and
/// are available whatever the drop policy. What an *undecided* transaction does on drop is
/// the compile-time [`DropPolicy`](super::DropPolicy) `P`: the default
/// [`Rollback`](super::Rollback) (from [`begin_stacked`](InputRef::begin_stacked)) rolls
/// back to the begin point, discarding all savepoints — the database default;
/// [`Commit`](super::Commit) (from [`begin_stacked_with`](InputRef::begin_stacked_with))
/// keeps the progress. Cost when unused is low: the transaction's own savepoint `Vec`
/// never allocates until the first [`savepoint`](Self::savepoint), and a begin captures one
/// field address and records its base checkpoint on the input's shared lineage stack (an
/// amortized `Vec` push) — no counter, no atomic.
///
/// # Mixing with raw save/restore, state surgery, and nested transactions
///
/// The guard deref-coerces to [`InputRef`], so raw [`save`](InputRef::save) /
/// [`restore`](InputRef::restore) and the nested backtracking tools are all reachable
/// through it. These rules govern how they interact with the live savepoints and the begin
/// point:
///
/// - **A raw restore below a savepoint (but above the base) invalidates the savepoint —
///   detect-at-use.** Savepoints are not pinned (only the base is), so this restore *succeeds*;
///   it rolls the lineage back past the savepoint's own checkpoint, so the savepoint is no
///   longer on a live lineage, and [`rollback_to`](Self::rollback_to) / [`release`](Self::release)
///   with it **panics as stale in every build** — release and no-`target_has_atomic`-ptr targets
///   included. Restoring the wrong lineage is never silently honored.
/// - **A raw restore below the begin point would tear out the whole transaction —
///   detect-at-cause.** Restoring a raw checkpoint taken *before*
///   [`begin_stacked`](InputRef::begin_stacked) would pop the **pinned** base off the live
///   lineage, so in allocator builds it **panics at the restore itself** (`restore would
///   invalidate a live transaction guard or attempt …`) — refused where it is caused, before any
///   commit/rollback decision. On allocator-less targets there is no pin set, so this is
///   unspecified-but-bounded rather than checked; in allocator builds the older detect-at-use
///   backstops (an explicit [`rollback`](Self::rollback) asserting a live base, a rolling-back
///   drop skipping a stale one) remain as defense in depth behind the pin check.
/// - **State surgery, nested `attempt` / `try_attempt` / [`Transaction`](super::Transaction),
///   and a LIFO-clean raw save/restore pair taken *above* the savepoints, are all legal and
///   do not disturb the savepoints.** [`set_state`](InputRef::set_state) /
///   [`state_mut`](InputRef::state_mut) re-key the forward-scanning facts but are
///   transactional — a savepoint taken before the surgery stays valid, and
///   [`rollback_to`](Self::rollback_to) it *undoes* the surgery (the regime, boundary,
///   watermark, and position all return). A nested speculation that saves and then restores
///   or commits its own younger checkpoint leaves every savepoint below it untouched.
///
/// The raw-restore rules above are reachable only with the `unstable-raw` feature. Without it,
/// raw [`save`](InputRef::save) / [`restore`](InputRef::restore) are crate-internal, so a
/// downstream crate cannot mix a raw restore into a live transaction at all — only the savepoint
/// operations and nested guards remain, and none of those can invalidate a savepoint.
///
/// ```ignore
/// // Best-match selection across three stages: keep a fallback after each, then return
/// // to the highest-scoring one and resume from exactly there.
/// let mut txn = input.begin_stacked();
///
/// let mut best = None;
/// let mut best_score = i32::MIN;
/// for _ in 0..3 {
///   let score = parse_one_stage(&mut txn);         // parse through the guard (DerefMut)
///   let sp = txn.savepoint();                       // fallback point after this stage
///   if score > best_score {
///     best_score = score;
///     best = Some(sp);
///   }
/// }
///
/// if let Some(sp) = best {
///   txn.rollback_to(sp);   // resume right after the best stage; younger savepoints die
/// }
/// txn.commit();            // keep the winning prefix
/// ```
pub struct StackedTransaction<
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
  /// The begin point. `Some` while the transaction is undecided; `None` once
  /// [`commit`](Self::commit) / [`rollback`](Self::rollback) (or a rolling-back drop)
  /// has consumed it. Routing the whole-transaction decision through this one
  /// `Option::take` is what keeps `commit`, `rollback`, and `Drop` from restoring the
  /// base twice — the same funnel the plain [`Transaction`](super::Transaction) uses.
  pub(super) base: Option<Checkpoint<'inp, 'closure, L>>,
  /// The live savepoints, youngest last. Each entry pairs a savepoint's `seq` with the
  /// checkpoint saved at that mark. `rollback_to` / `release` truncate this vector from
  /// the top, which is what makes destroy-younger structural rather than a runtime check.
  pub(super) saves: SavepointStack<'inp, 'closure, L>,
  /// The address of this input's `poison_boundary` field, stamped into every
  /// [`SavepointId`] this transaction issues. It separates this input's savepoints from
  /// those of another simultaneously-live input, which sits at a distinct address (see
  /// [`SavepointId`]).
  pub(super) nonce: usize,
  /// The drop policy — [`Rollback`](super::Rollback) or [`Commit`](super::Commit) —
  /// carried as a zero-sized typestate. It selects, at compile time and branch-free, what
  /// an undecided guard's `Drop` does: roll back to the begin point, or keep the progress.
  pub(super) _policy: PhantomData<P>,
}

impl<'txn, 'inp, L, Ctx, Lang: ?Sized, P: DropPolicy, Cmpl>
  StackedTransaction<'txn, 'inp, '_, L, Ctx, Lang, P, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Marks the current position as a savepoint and returns its id (SQL `SAVEPOINT`).
  ///
  /// The returned [`SavepointId`] stays usable for [`rollback_to`](Self::rollback_to)
  /// and [`release`](Self::release) until an older savepoint destroys it or it is
  /// released. Its lifetime is branded to this transaction, so it cannot escape the
  /// transaction's scope.
  #[inline]
  pub fn savepoint(&mut self) -> SavepointId<'txn> {
    // CAPTURE_WINDOW — preflight this savepoint's slot before anything is captured.
    // `self.input.save()` below registers a lineage entry and an emitter mark that only the
    // `saves.push` hands an owner (a `Checkpoint` has no `Drop`: it cannot reach the input or the
    // emitter to settle itself). If anything between the capture and that push unwound — a
    // `capacity overflow` panic, or a panicking allocator — the capture would be dropped raw, its
    // lineage entry and emitter mark stranded with no rollback, commit, `forget_kept_checkpoint`,
    // or release able to find them; and with no `saves` entry pushed, none of the guard's own
    // life-ending paths could settle it either.
    //
    // The window holds exactly one allocation site — this `push` onto `saves`, a `Vec` by default
    // and a `SmallVec` that spills past its inline 2 under `smallvec_1`, whose `reserve` compares
    // against the *inline* capacity too and so spills here rather than in the push. A savepoint is
    // deliberately NOT pinned (only the transaction's base is, in `begin_stacked_with`), so unlike
    // `begin_point` this site has no second container to reserve. Everything else in the window is
    // allocation-free by inspection: `(seq, ckp)` is a tuple of a `Copy` scalar and a move.
    // (`seq` is taken before the capture, so the counter bump is outside the window; the
    // `SavepointId` literal is built after the push, likewise outside it.)
    self.saves.reserve(1);
    // Sequence numbers come from the input, not this transaction, so they never reset:
    // an id can only ever match the one live slot that pushed it (or none, if stale).
    let seq = self.input.next_savepoint_seq();
    let ckp = self.input.save();
    self.saves.push((seq, ckp));
    SavepointId {
      seq,
      nonce: self.nonce,
      _brand: PhantomData,
    }
  }

  /// Rolls back to `sp` (SQL `ROLLBACK TO`): returns the input to `sp`'s position —
  /// cursor, span, lexer state, emission log, dedup watermark, and poison boundary all
  /// restored — and destroys every savepoint created after it, while keeping `sp` itself
  /// valid for a later rollback.
  ///
  /// `Checkpoint` is single-use, so keeping `sp` reusable is done by restoring the stored
  /// checkpoint and immediately re-saving at the now-current position, swapping the fresh
  /// checkpoint into `sp`'s slot. This preserves the classic SQL loop of rolling back to
  /// the same savepoint any number of times; it costs one extra `O(1)` save per call on
  /// this cold path, plus one settle per destroyed savepoint (the same per-entry work
  /// [`release`](Self::release) and [`commit`](Self::commit) already pay).
  ///
  /// # Panics
  ///
  /// Panics if `sp` was issued by a different, simultaneously-live transaction
  /// (`stacked transaction: savepoint belongs to a different transaction`), was destroyed by
  /// an earlier `rollback_to` / [`release`](Self::release) (`stacked transaction: savepoint
  /// is stale (destroyed by an earlier rollback or release)`), or had its checkpoint
  /// invalidated by a raw restore below it through the transaction (`stacked transaction:
  /// savepoint is stale (invalidated by a raw restore below it)`). All three checks — an
  /// address compare and two short stack scans — run in every build. (Using an id after its
  /// transaction ended is a compile error, not a panic; see [`SavepointId`]. State surgery is
  /// transactional and does *not* invalidate a savepoint — one taken before it stays valid,
  /// and rolling back to it undoes the surgery.)
  #[inline]
  pub fn rollback_to(&mut self, sp: SavepointId<'txn>) {
    let idx = self.slot(sp);
    // Settle the younger savepoints one at a time, newest-first, BEFORE the single restore
    // below — never by truncating the vector, which would drop their checkpoints raw.
    // Dropping them raw is not enough because the restore forwards only the TARGET's emitter
    // mark, and a capture taken with no intervening events reads the very same mark: a rewind
    // spends what sits strictly above the mark plus the newest capture at it, so every older
    // capture at that same reading survives as mark-keyed bookkeeping nothing will ever
    // spend. Settling each one also keeps every removed id the live-stack top when it is
    // forgotten (the `O(1)` fast path), exactly as `release`/`commit` do.
    while self.saves.len() > idx + 1 {
      let (_, ckp) = self
        .saves
        .pop()
        .expect("len > idx + 1 implies a value to pop");
      self.input.forget_kept_checkpoint(ckp);
    }
    let (seq, ckp) = self
      .saves
      .pop()
      .expect("slot() returned a valid index into `saves`");
    // Restore consumes the stored checkpoint; re-save at the restored position and
    // reinstall it under the same `seq` so `sp` survives for repeated rollbacks.
    //
    // CAPTURE_WINDOW — this re-save needs no preflight, and must not grow one: the pops above
    // freed at least this very slot, so `len < capacity` holds and the `push` is a write into
    // spare capacity for `Vec` and `SmallVec` alike (a pop never shrinks either). Nothing between
    // the capture and the push allocates — the tuple is a `Copy` scalar and a move — and the
    // restore that precedes it is complete before the capture is taken.
    self.input.restore(ckp);
    let fresh = self.input.save();
    self.saves.push((seq, fresh));
  }

  /// Releases `sp` (SQL `RELEASE SAVEPOINT`): forgets `sp` and every savepoint created
  /// after it, **keeping the parsed progress**. The input position does not move.
  ///
  /// # Panics
  ///
  /// Same as [`rollback_to`](Self::rollback_to): a foreign or already-destroyed id panics.
  #[inline]
  pub fn release(&mut self, sp: SavepointId<'txn>) {
    let idx = self.slot(sp);
    // Forget from the youngest down to `sp` inclusive, so each removed id is the
    // live-stack top when it is forgotten (the `O(1)` fast path) and each emitter mark is
    // released newest-first (the kept-checkpoint funnel pairs the two). Progress is kept:
    // no checkpoint is restored.
    while self.saves.len() > idx {
      let (_, ckp) = self.saves.pop().expect("len > idx implies a value to pop");
      self.input.forget_kept_checkpoint(ckp);
    }
  }

  /// Validates `sp` and returns its index in `saves`, panicking on a foreign, a destroyed,
  /// or a lineage-invalidated id. An address compare plus two short scans, on a cold path.
  #[inline]
  fn slot(&self, sp: SavepointId<'txn>) -> usize {
    assert!(
      sp.nonce == self.nonce,
      "stacked transaction: savepoint belongs to a different transaction"
    );
    let idx = match self.saves.iter().position(|(seq, _)| *seq == sp.seq) {
      Some(idx) => idx,
      None => panic!(
        "stacked transaction: savepoint is stale (destroyed by an earlier rollback or release)"
      ),
    };
    // Lineage validity, in every build: the `seq` is still in `saves`, but the checkpoint
    // that slot marks must still be live on the input's lineage stack. A raw restore through
    // the transaction (via `DerefMut`) to a checkpoint older than this savepoint pops it off
    // that stack without touching `saves` — leaving a stale slot that the nonce + membership
    // check alone would honor, restoring the wrong lineage. (State surgery does NOT reach
    // here: it is transactional and leaves the lineage stack intact, so a savepoint taken
    // before it stays live and rolling back to it undoes the surgery.) This is a plain `Vec`
    // membership scan (no atomics), so it closes the hole on release and
    // no-`target_has_atomic`-ptr targets exactly as in a debug witness build.
    assert!(
      self.input.live_contains(self.saves[idx].1.ckp_id),
      "stacked transaction: savepoint is stale (invalidated by a raw restore below it)"
    );
    idx
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, P: DropPolicy, Cmpl>
  StackedTransaction<'_, 'inp, '_, L, Ctx, Lang, P, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Commits the whole transaction: keeps every parsed byte and forgets all savepoints
  /// and the begin point without restoring. Available whatever the drop policy.
  #[inline]
  pub fn commit(mut self) {
    trace_event!(self.input, "commit");
    // Forget youngest-first (each is the live-stack top when popped, and each emitter mark
    // is released newest-first through the kept-checkpoint funnel), then the base last (it
    // is the deepest, so it is the top once the savepoints are gone). Taking `base` leaves
    // the `Drop` guard nothing to restore.
    while let Some((_, ckp)) = self.saves.pop() {
      self.input.forget_kept_checkpoint(ckp);
    }
    if let Some(base) = self.base.take() {
      // Only the base was pinned (savepoints keep their detect-at-use rule): unpin it too.
      self.input.unpin_checkpoint(base.ckp_id);
      self.input.forget_kept_checkpoint(base);
    }
  }

  /// Rolls the whole transaction back to the begin point, discarding every savepoint and
  /// all parsed progress. Available whatever the drop policy (a [`Commit`](super::Commit)
  /// guard can still be rolled back explicitly).
  #[inline]
  pub fn rollback(mut self) {
    trace_event!(self.input, "rollback");
    // Settle every savepoint first, newest-first, for the reason `rollback_to` spells out: the
    // restore below forwards only the BASE's emitter mark, which cannot name captures that
    // read the same value. Restoring the base then pops the live stack down through it,
    // carrying off any id a settle already removed as a no-op.
    while let Some((_, ckp)) = self.saves.pop() {
      self.input.forget_kept_checkpoint(ckp);
    }
    if let Some(base) = self.base.take() {
      // Unpin the begin point FIRST so the checked restore does not see it as pinned — rolling
      // back to the guard's own base is legal. A raw restore *below* the base (through this
      // guard's `DerefMut`) would already have panicked at that restore (detect-at-cause), so
      // the stale assert here is now an unreachable backstop, kept for defense in depth. (A
      // rolling-back drop, which may run mid-unwind, quietly skips the restore instead.)
      self.input.unpin_checkpoint(base.ckp_id);
      assert!(
        self.input.live_contains(base.ckp_id),
        "transaction base is stale (invalidated by an earlier restore)"
      );
      self.input.restore(base);
    }
  }
}

impl<'inp, 'closure, L, Ctx, Lang: ?Sized, P: DropPolicy, Cmpl> Deref
  for StackedTransaction<'_, 'inp, 'closure, L, Ctx, Lang, P, Cmpl>
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
  for StackedTransaction<'_, 'inp, '_, L, Ctx, Lang, P, Cmpl>
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
  for StackedTransaction<'_, 'inp, '_, L, Ctx, Lang, P, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Decides an undecided transaction according to its [`DropPolicy`](super::DropPolicy).
  /// After [`commit`](Self::commit) / [`rollback`](Self::rollback) the base and savepoints
  /// are already taken, so this is a no-op whatever the policy.
  ///
  /// - [`Rollback`](super::Rollback): roll back to the begin point (the database default,
  ///   all savepoints and progress discarded). Every savepoint is settled first, youngest
  ///   first, then the base is restored — a savepoint's emitter mark can read the same value
  ///   as the base's, so the one rewind cannot settle it (see
  ///   [`rollback_to`](Self::rollback_to)).
  /// - [`Commit`](super::Commit): keep the progress, forgetting every savepoint id
  ///   (youngest first) then the base — the same lineage-id hygiene as
  ///   [`commit`](Self::commit).
  ///
  /// An undecided guard dropped while the thread is **unwinding** takes the rollback arm
  /// whatever its policy (std builds) — a panic aborts the region rather than completing it,
  /// and the base and every savepoint settle through the same funnel either way. See
  /// [`Commit`](super::Commit) for the posture and the `no_std` divergence.
  ///
  /// `P::ROLLBACK_ON_DROP` is a compile-time constant, so the `Rollback` policy monomorphizes
  /// to one arm with the other eliminated; `Commit` reads the unwind fact once per undecided
  /// drop. The rollback arm is silent (unchecked): `Drop` may run
  /// while already unwinding, where `no_std` has no `thread::panicking()` to guard a
  /// drop-bomb. Both arms first unpin the base (exception-safe). The pin check makes a raw
  /// restore below the base panic at that restore, so the base cannot go stale while the guard
  /// is live and the rollback arm normally just rewinds; the stale-base skip it still performs
  /// is a backstop (defense in depth, and the behavior for allocator-less builds).
  #[inline]
  fn drop(&mut self) {
    // The unwind fact overrides the policy, exactly as in the sibling guard: a panic aborts the
    // region, so an undecided `Commit` stacked guard rolls back to its base instead of keeping
    // half an iteration. `Rollback` const-folds to the same arm it always had.
    if P::ROLLBACK_ON_DROP || crate::input::input_ref::drop_policy::unwinding() {
      // Settle every savepoint before the base's rewind, newest-first — the same per-entry
      // discipline the commit arm below applies, and required for the same reason
      // `rollback_to` spells out: the rewind forwards only the base's emitter mark and cannot
      // name captures that read the same value. Unconditional: when the stale-base backstop
      // skips the rewind, these settles are simply the whole reclamation. The funnel is
      // assert-free, so this stays silent even mid-unwind.
      while let Some((_, ckp)) = self.saves.pop() {
        self.input.forget_kept_checkpoint(ckp);
      }
      if let Some(base) = self.base.take() {
        // Unpin the begin point first — exception-safe, so it happens even though the rewind
        // below may be skipped (a `Drop` may run mid-unwind, where panicking is forbidden). The
        // pin check makes the base go-stale case unreachable in allocator builds, so this
        // normally rewinds — popping the live stack down through the base; the skip stays as a
        // backstop. An explicit `rollback` reports a stale base loudly instead.
        self.input.unpin_checkpoint(base.ckp_id);
        self.input.restore_unchecked_if_live(base);
      }
    } else {
      // Commit-on-drop: progress kept; forget every savepoint id (youngest first, each the
      // live-stack top when popped, its emitter mark released through the kept-checkpoint
      // funnel) then unpin and settle the base the same way, so nothing lingers on the
      // live/pin stacks or in mark-keyed emitter bookkeeping. The funnel is assert-free, so
      // this arm stays silent even mid-unwind.
      while let Some((_, ckp)) = self.saves.pop() {
        self.input.forget_kept_checkpoint(ckp);
      }
      if let Some(base) = self.base.take() {
        self.input.unpin_checkpoint(base.ckp_id);
        self.input.forget_kept_checkpoint(base);
      }
    }
  }
}

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod tests;
