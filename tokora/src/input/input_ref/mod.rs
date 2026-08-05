#![allow(clippy::type_complexity)]

use core::{
  marker::PhantomData,
  ops::{Range, RangeBounds},
};

use generic_arraydeque::{GenericArrayDeque, typenum::U1};
use mayber::{Maybe, MaybeRef};

use crate::{
  ParseContext, Token, Window,
  cache::{CachedToken, CachedTokenOf, CachedTokenRefOf, MaybeRefCachedTokenOf, Peeked},
  emitter::{Emitter, EmitterView},
  error::{UnexpectedEot, token::UnexpectedToken},
  span::Spanned,
  utils::Expected,
};

use super::{
  Cache, Checkpoint, Complete, Completeness, Cursor, Lexed, Lexer, Lineage, Source, Span,
  SurfaceIncomplete,
};

pub(crate) use session::Session;

#[cfg(any(feature = "std", feature = "alloc"))]
pub use session::SessionPointId;

mod consume_cached;
mod descent;
mod drop_policy;
mod emit;
mod fold;
mod peek;
#[cfg(feature = "pratt")]
mod pratt;
mod scan;
pub(crate) mod session;
mod skip_while;
#[cfg(any(feature = "std", feature = "alloc"))]
mod stacked;
mod sync_balanced;
mod sync_through;
mod sync_to;
#[cfg(feature = "trace")]
mod trace;
mod transaction;
mod try_expect;

pub use descent::Descent;
pub use drop_policy::{Commit, DropPolicy, Rollback};
pub use sync_balanced::{Balance, DelimClass, Hole};
pub use transaction::Transaction;

pub(crate) use try_expect::CloseStatus;
// `ClosePayload` is threaded through the delimited drivers without being named there (the
// `Close(payload)` arm passes it straight to `commit_probed`); only the tests name the origin,
// so the re-export is gated to exactly the cfg that compiles `partial_tests`.
#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
pub(crate) use try_expect::ClosePayload;

#[cfg(any(feature = "std", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
pub use stacked::{SavepointId, StackedTransaction};

/// SETTLE_CENSUS / RELEASE_CENSUS — source-census tests over `include_str!` snapshots.
/// The census greps the same source bytes in every configuration, so one allocator-enabled
/// run locks it for all of them; the string-building the checks use needs `format!`, which
/// the allocator-less build lacks.
#[cfg(all(test, any(feature = "std", feature = "alloc")))]
mod census_tests;

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod tests;

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod partial_tests;

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod capacity_tests;

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod session_tests;

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod fast_path_tests;

/// What holds a recursion level and what silently gives it back — the runtime half of the table on
/// [`Descent`]. Needs `std` for the unwind cell's `catch_unwind`.
#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod descent_tests;

/// A reference to an `Input` instance.
pub struct InputRef<'inp, 'closure, L, Ctx, Lang: ?Sized = (), Cmpl = Complete>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  pub(super) input: &'closure &'inp L::Source,
  pub(super) state: &'closure mut L::State,
  pub(super) span: &'closure mut L::Span,
  pub(super) cache: &'closure mut Ctx::Cache,
  /// The parked front token — see [`Input`](super::Input)'s field of the same name for the law it
  /// exists to keep true. Reached only through the five `front` primitives.
  pub(super) pending: &'closure mut Option<CachedTokenOf<'inp, L>>,
  /// A **read-only snapshot** of the owning [`Input`]'s finality world cell (a ZST for
  /// [`Complete`], a `bool` for [`Partial`]), copied by value at
  /// [`as_ref`](super::Input::as_ref). The frontier rules read it only under
  /// [`Completeness::PARTIAL`].
  ///
  /// There is no mutator — see [`is_final`](Self::is_final) for the law. Taking this handle
  /// mutably borrows the input, which locks out the seal for the handle's whole life, so the
  /// snapshot cannot go stale: finality is *constant* while a handle lives, and therefore outside
  /// the rollback set by construction (a [`Checkpoint`] does not carry it, and does not need to).
  pub(super) finality: Cmpl::Finality,
  pub(super) emitted_error_end: &'closure mut L::Offset,
  /// The front-report watermark, borrowed from the owning [`Input`](super::Input) — see that
  /// field for the invariant and the three writers that keep it inductive.
  pub(super) front_reported_end: &'closure mut Option<L::Offset>,
  pub(super) poison_boundary: &'closure mut Option<L::Offset>,
  /// The **recursion budget**, borrowed from the owning [`Input`](super::Input) — see that field
  /// for why it is outside the rollback set. Read through [`recursion`](Self::recursion); its
  /// only writer is the [`Descent`] guard [`descend`](Self::descend) hands out.
  pub(super) recursion: &'closure mut crate::state::recursion_tracker::RecursionLimiter,
  /// The **resource-trip counter**, borrowed from the owning [`Input`](super::Input) — see that
  /// field for what is recorded, why it is the *fact* of a trip rather than the depth, and why it
  /// counts rather than latching a `bool`.
  ///
  /// Read through [`trip_snapshot`](Self::trip_snapshot) and compared through
  /// [`tripped_during_attempt`](Self::tripped_during_attempt); its only writer is
  /// [`raise_level`](Self::raise_level)'s trip arm, which is why grammar code cannot lower it:
  /// no handle method exposes a mutable route to the cell, and the recursion cell it guards is
  /// read-only too.
  pub(super) resource_trips: &'closure mut usize,
  /// The **scanner-trip counter**, borrowed from the owning [`Input`](super::Input) — see that
  /// field for why a monotone counter, and not the rollbackable poison boundary beside it, is what
  /// can witness a scanner stop across a nested speculation.
  ///
  /// Read through [`scanner_trip_snapshot`](Self::scanner_trip_snapshot) and compared through
  /// [`scanner_tripped_during_attempt`](Self::scanner_tripped_during_attempt); its only writer is
  /// [`latch_if_limit_tripped`](Self::latch_if_limit_tripped), the crate's terminal predicate, and
  /// no handle method exposes a mutable route to the cell.
  pub(super) scanner_trips: &'closure mut usize,
  /// The **session cell**: the input's lineage memos (the live-checkpoint stack, the pin set, and
  /// the cache-push/checkpoint-id/savepoint counters), the handle's **emitter borrow** (the
  /// ground-truth emission log, reached through [`emitter`](Self::emitter)), and the live
  /// [session points](Self::begin_point) opened on this handle.
  ///
  /// They are one cell because an abandoned session point has to release bookkeeping it does not
  /// own — the pin lives on the [`Input`](super::Input), which outlives this handle; the emitter
  /// mark lives in the borrowed emitter, which outlives it too; the point's [`Checkpoint`] lives
  /// here and dies with it. [`Session`]'s `Drop` reconciles all three (see its
  /// [module docs](session) for that, and for why the destructor lives on this cell rather than
  /// on the handle: a `Drop` on `InputRef` would escape *every* field to the destructor and cost
  /// the scanner its registers).
  pub(super) session: Session<'inp, 'closure, L, Ctx::Emitter, Lang>,
  /// Trace nesting depth, borrowed from the owning [`Input`] (the `trace` feature). Its sole
  /// mutators are [`traced`](crate::traced)'s enter/exit hooks; internal leaf events only read
  /// it for indentation.
  #[cfg(feature = "trace")]
  pub(super) depth: &'closure mut usize,
  /// Debug-only witness of the input identity, for `restore`'s foreign-input check.
  #[cfg(all(
    debug_assertions,
    any(feature = "std", feature = "alloc"),
    target_has_atomic = "ptr"
  ))]
  pub(super) witness: &'closure super::Witness,
  pub(super) _marker: PhantomData<Lang>,
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Returns a reference to the tokenizer's cache.
  ///
  /// The cache stores peeked tokens that have been lexed but not yet consumed.
  /// This can be useful for inspecting the cache state or implementing custom
  /// lookahead logic.
  #[inline(always)]
  pub const fn cache(&self) -> &Ctx::Cache {
    self.cache
  }

  /// Returns a mutable reference to the tokenizer's cache.
  #[inline(always)]
  const fn cache_mut(&mut self) -> &mut Ctx::Cache {
    self.cache
  }

  /// FRONT_CENSUS — the peek fill's **back** push: a token appended *behind* the tokens already
  /// retained, which is the one push in the crate that is genuinely not a put-back (see
  /// [`hold_front`](Self::hold_front) for those). Records the accepted push on the lineage memos
  /// ([`Lineage::record_cache_push`](super::Lineage::record_cache_push)) so [`save`](Self::save)
  /// can snapshot the count and [`restore`](Self::restore) drop exactly the entries pushed since.
  /// A full cache hands the token back and records nothing.
  #[inline(always)]
  fn cache_append(&mut self, tok: CachedTokenOf<'inp, L>) -> Result<(), CachedTokenOf<'inp, L>> {
    match self.cache.push_back(tok) {
      Ok(_) => {
        self.session.lineage.record_cache_push();
        Ok(())
      }
      Err(tok) => Err(tok),
    }
  }

  /// `true` only for a cache that can actually refuse a front push (capacity 0): the parked slot
  /// is written on exactly that refusal, so under a retaining cache
  /// ([`Cache::RETAINS_FRONT`](crate::cache::Cache::RETAINS_FRONT)) it is statically `None` and
  /// every probe of it below folds to the plain cache operation.
  #[inline(always)]
  fn can_park() -> bool {
    !<Ctx::Cache as Cache<'inp, L, Lang>>::RETAINS_FRONT
  }

  /// FRONT_CENSUS — the front of the consumer-visible token stream, read-only: the parked token if
  /// one is held, else the cache front. The stream order is `parked?` then the cache, so this
  /// consults the slot first.
  #[inline(always)]
  fn front(&self) -> Option<CachedTokenRefOf<'_, 'inp, L>> {
    if Self::can_park() && self.pending.is_some() {
      return Self::front_parked(self.pending);
    }
    self.cache().front()
  }

  /// Cold half of [`front`](Self::front): reached only with a token actually parked, kept out of
  /// line so the primitive stays small enough not to disturb its callers' inlining.
  #[cold]
  #[inline(never)]
  fn front_parked<'s>(
    pending: &'s Option<CachedTokenOf<'inp, L>>,
  ) -> Option<CachedTokenRefOf<'s, 'inp, L>> {
    pending.as_ref().map(|parked| parked.as_ref())
  }

  /// FRONT_CENSUS — whether a lexed token is already available without touching the lexer.
  #[inline(always)]
  fn has_front(&self) -> bool {
    (Self::can_park() && self.pending.is_some()) || !self.cache().is_empty()
  }

  /// FRONT_CENSUS — takes the front token out of the stream. Not a settle: the caller commits it
  /// through [`commit_token`](Self::commit_token), exactly as it did off a cache pop.
  #[inline(always)]
  fn take_front(&mut self) -> Option<CachedTokenOf<'inp, L>> {
    if Self::can_park() && self.pending.is_some() {
      return self.take_front_parked();
    }
    self.cache_mut().pop_front()
  }

  /// Cold half of [`take_front`](Self::take_front) — see [`front_parked`](Self::front_parked).
  #[cold]
  #[inline(never)]
  fn take_front_parked(&mut self) -> Option<CachedTokenOf<'inp, L>> {
    self.pending.take()
  }

  /// FRONT_CENSUS — whether a token is parked in the front slot. The scanner asks through this
  /// rather than reading the slot, so `scan.rs` keeps no direct reader of its own.
  #[inline(always)]
  pub(super) fn has_front_parked(&self) -> bool {
    self.pending.is_some()
  }

  /// Restores the five facts a rewinding scan's entry snapshot holds, then settles its mark by
  /// rewinding to it — the one body behind both `ScanMode::on_eof` and `ScanMode::on_incomplete`,
  /// so the normal no-match exit and the abandoning exits cannot drift.
  ///
  /// The span is **moved**, not borrowed: `set_span` clones a borrowed span, and cloning here is
  /// caller code inside a settle. The remaining fallible step is the emitter cursor's offset
  /// clone, which is why `ScanScope` keeps the mark beside the snapshot and releases it if this
  /// body unwinds part-way.
  #[inline]
  fn restore_entry(&mut self, entry: scan::ThroughEntry<L::Span, L::State, L::Offset>) {
    let (span, state, mark, error_end, rewind_to, poison_boundary) = entry.into_components();
    // The position goes in first and the values it replaced come back out, so their drops — caller
    // code — happen after the REST of the restore rather than in the middle of it. Interrupted
    // there, this function would otherwise return a restored position paired with a stale dedup
    // watermark, a stale poison latch and an un-rewound emitter.
    // SETTLE_CENSUS: this body runs NO caller code before its rewind — not a clone, not an
    // in-place assignment. `Emitter::rewind` needs the position the parse resumes from, and that
    // offset was cloned at capture (`ThroughEntry::rewind_to`), before the mark it settles even
    // existed. Reading it back off the input here instead — as this did — put an
    // `L::Offset::clone` between the position write and the rewind that completes it, and a panic
    // there skipped the rewind so `ScanScope::drop` RELEASED the mark: the abandoned scan's
    // diagnostics stayed in the log while the parser retried from the restored position.
    //
    // Hoisting that clone to the top of the body did NOT fix it, which is worth stating because
    // it looked like it should. Wherever in the body the clone sits, failing it still skips the
    // rewind — the measurement is `r9_restore_entry_is_atomic_at_every_offset_clone`, which stayed
    // green against the hoisted version and only went red once the operation left the body.
    let replaced = self.replace_position(span.into(), state);
    // `mem::replace`, not assignment, for the same reason as everywhere else in this family: a
    // plain write drops the value it displaced, and `L::Offset::drop` is caller code that would
    // land between two facts of one restore.
    let displaced = (
      core::mem::replace(self.emitted_error_end, error_end),
      // The front-report watermark is deliberately NOT among these, and the reason is an ordering
      // argument rather than a measurement, so it is written down: a scan captures its emitter mark
      // inside an element, which runs after the driver has already flagged and reported a wrong
      // opener. The flag's report therefore PREDATES this mark, the rewind below cannot truncate
      // it, and the watermark must stay armed — disarming it here would make the driver re-report a
      // token it has already reported.
      //
      // It is not asserted because it is not cheaply assertable: `Emitter::checkpoint` returns an
      // opaque `u64` with no ordering contract, so the crate cannot compare this mark against the
      // one the report was appended under. If that contract ever gains an ordering, this is the
      // site that should carry the assert.
      // The fifth fact. A limit trip latches the boundary before its diagnostic is emitted, so an
      // exit that abandons the scan must un-latch what the scan latched — otherwise the input stays
      // poisoned at a position no committed lineage ever reached. On the normal no-match exit this
      // is a no-op by construction (a trip leaves through the trip exit, which keeps its progress),
      // which is exactly why one body can serve both.
      core::mem::replace(self.poison_boundary, poison_boundary),
    );
    debug_assert!(
      !self.has_front_parked() && self.cache().is_empty(),
      "restore_entry reads the rewind cursor off the entry span, which is only the resumed \
       position while the stream is drained; a caller that reaches here holding tokens would \
       rewind the emitter to the wrong place",
    );
    self.emitter().rewind(Cursor::from_ref(&rewind_to), mark);
    // Last, with every fact restored: a `Drop` that unwinds here leaves a whole restore behind it
    // rather than a position without its watermark, latch and emitter rewind.
    drop((replaced, displaced));
  }

  /// Clears the whole retained stream — the parked slot and every cache entry — against a
  /// committed position that has not moved, so the region re-lexes deterministically from there.
  ///
  /// This exists for exactly one caller: the scanner's unwind edge under a **rewinding** mode,
  /// which restores the pre-call position and must not leave tokens in front of it that were
  /// popped out of a stream the restore is about to rewind past. The committing modes do not
  /// clear — their unwind edge keeps the diagnosed prefix, and clearing there was measured
  /// re-burning a shared limit budget and tripping a limiter that the honest run never trips.
  #[inline]
  pub(super) fn unwind_clear_stream(&mut self) {
    *self.pending = None;
    self.cache_mut().clear();
  }

  /// FRONT_CENSUS — takes the front token only if `pred` accepts it; a decline leaves it exactly
  /// where it was (parked slot or cache front), which is what makes a decline "definite absence"
  /// in every cache capacity.
  #[inline(always)]
  fn take_front_if<F>(&mut self, pred: F) -> Option<CachedTokenOf<'inp, L>>
  where
    F: FnOnce(CachedTokenRefOf<'_, 'inp, L>) -> bool,
  {
    if Self::can_park() && self.pending.is_some() {
      return self.take_front_if_parked(pred);
    }
    self.cache_mut().pop_front_if(pred)
  }

  /// Cold half of [`take_front_if`](Self::take_front_if) — see
  /// [`front_parked`](Self::front_parked).
  #[cold]
  #[inline(never)]
  fn take_front_if_parked<F>(&mut self, pred: F) -> Option<CachedTokenOf<'inp, L>>
  where
    F: FnOnce(CachedTokenRefOf<'_, 'inp, L>) -> bool,
  {
    match self.pending.as_ref() {
      Some(parked) => pred(parked.as_ref()).then(|| self.pending.take()).flatten(),
      None => None,
    }
  }

  /// FRONT_CENSUS — **the** put-back: puts a token the crate decided not to consume back at the
  /// front of the stream. The one home of the "an unconsumed token lives at the front" law, for
  /// the scanner's `to`-shaped stops and every `try_expect*`/`probe_close` decline alike.
  ///
  /// # Never a settle
  ///
  /// SETTLE_CENSUS lists `unconsume` among the non-settles; this is that put-back generalized, and
  /// it inherits the same posture: nothing is committed, so no [`Emitter::commit_token`] hook
  /// fires here.
  ///
  /// # Only the push history knows the origin
  ///
  /// A token **popped off the cache** goes back into the slot it left, so the cache is exactly what
  /// it was and its push count must not move. A token that was **lexed** by the caller, or
  /// **unparked** a moment ago, is a new cache entry if the cache takes it — precisely the entry a
  /// peek would have made — so its push is recorded and a checkpoint saved before this call drops
  /// it on restore. Getting that backwards over-drops a genuinely pre-save entry
  /// ([`restore`](Self::restore) drops the entries pushed since the save, from the back).
  ///
  /// A cache that refuses the push keeps the token anyway — parked, uncounted, outside the cache.
  #[inline(always)]
  fn hold_front(&mut self, tok: CachedTokenOf<'inp, L>, origin: Origin) {
    debug_assert!(
      self.pending.is_none(),
      "a second token was parked over a live one",
    );
    match self.cache_mut().push_front(tok) {
      Ok(_) => {
        if !matches!(origin, Origin::Cache) {
          self.session.lineage.record_cache_push();
        }
      }
      Err(tok) => self.park(tok),
    }
  }

  /// Cold half of [`hold_front`](Self::hold_front): the cache refused the put-back, so the token
  /// parks in the slot outside it. Out of line so the put-back sites stay small — see
  /// [`front_parked`](Self::front_parked).
  #[cold]
  #[inline(never)]
  fn park(&mut self, tok: CachedTokenOf<'inp, L>) {
    // A refusing cache under `RETAINS_FRONT` is lying about its contract: fail loudly at the cause
    // instead of silently losing the token (the parked-slot reads are compiled out under that
    // declaration, so the slot would never be consulted again).
    assert!(
      Self::can_park(),
      "a `Cache` declaring RETAINS_FRONT refused a front push into an empty cache",
    );
    debug_assert!(
      self.cache().is_empty(),
      "a `Cache` refused a front push into a non-empty cache — the parked token would sit \
       behind entries that are newer than it",
    );
    *self.pending = Some(tok);
  }

  /// Returns a reference to the underlying input source.
  ///
  /// This allows access to the raw source being tokenized, which is typically
  /// a `&str` or `&[u8]` depending on your Logos token definition.
  #[inline(always)]
  pub const fn source(&self) -> &'inp L::Source {
    self.input
  }

  /// Returns a reference to the current lexer state (extras).
  #[inline(always)]
  pub const fn state(&self) -> &L::State {
    self.state
  }

  /// Returns whether this input is **final** — the last chunk of a stream, or a
  /// [`Complete`](crate::input::Complete) input (always final).
  ///
  /// A [`Partial`](crate::input::Partial) input reports the flag the **driver** stated
  /// ([`parse_partial`](crate::parse_partial)'s `is_final` argument); a
  /// [`Complete`](crate::input::Complete) input is final by definition, so this returns `true` and
  /// the partial-input frontier rules are inert.
  ///
  /// # Read-only, and constant for this handle's life
  ///
  /// There is no `set_final` on an [`InputRef`], and that absence is a **law**, not an omission.
  /// `is_final` is a fact about the **world** — *the caller has told us no more bytes are coming* —
  /// and a parser combinator cannot possibly know it. Only the code that owns the byte buffer can.
  ///
  /// So the sole writer is the owning input's seal, which takes `&mut Input` — and this handle
  /// mutably borrows that input for its entire life. A parser therefore **cannot** end a stream, at
  /// any depth, inside any speculative branch. Nor can it un-end one: the seal is monotone and has
  /// no inverse anywhere in the crate.
  ///
  /// That is what keeps finality safely out of the rollback set. It cannot change while this handle
  /// lives, so no rollback can observe it change — a [`Checkpoint`] has nothing to save, and a
  /// restore has nothing to undo. The two laws this pins:
  ///
  /// - a failed speculative branch can never cost the frontier holdback (it could not have touched
  ///   finality to begin with);
  /// - a rollback can never un-end a stream the driver already ended (the hang that "roll finality
  ///   back too" would introduce).
  ///
  /// A parser reaching for the flag does not compile — through the handle, or through a guard's
  /// `DerefMut`:
  ///
  /// ```compile_fail
  /// use tokora::{InputRef, Lexer, ParseContext, Partial};
  ///
  /// fn end_the_stream<'inp, L, Ctx>(inp: &mut InputRef<'inp, '_, L, Ctx, (), Partial>)
  /// where
  ///   L: Lexer<'inp>,
  ///   L::State: Clone,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   inp.set_final(true); // error: no method named `set_final` — finality is the driver's
  /// }
  /// ```
  ///
  /// Enforcing tests (in `src/input/input_ref/partial_tests.rs`):
  /// `speculation_cannot_end_the_stream` and `rollback_cannot_un_end_a_sealed_stream`.
  #[inline(always)]
  pub fn is_final(&self) -> bool {
    Cmpl::is_final(&self.finality)
  }

  /// Returns a mutable reference to the current lexer state (extras).
  ///
  /// # State replacement re-keys the input's forward-scanning facts
  ///
  /// Mutating the state through the returned reference can change how the region ahead of
  /// the cursor lexes, so this call **eagerly** re-keys every offset-dependent fact that
  /// governs forward scanning: the token cache is cleared (its entries were lexed under the
  /// old state and those offsets may lex differently now), the poison boundary is dropped,
  /// and the lexer-error dedup watermark is reset to the current committed cursor. The
  /// re-key runs before this returns, so it applies whether or not the caller ends up
  /// mutating through the `&mut`.
  ///
  /// Speculative peek-ahead diagnostics emitted under the old state for the region beyond
  /// the cursor stay in the emitter log, and the watermark reset makes that same region
  /// re-reportable once it re-lexes under the new state: state surgery with outstanding
  /// speculative diagnostics may re-report the re-lexed region under the new regime, so
  /// callers should complete or roll back speculation before replacing state.
  ///
  /// # Transactional: checkpoints survive state surgery
  ///
  /// The re-key is itself **transactional**, not invalidating. A [`Checkpoint`] pure-copies
  /// every fact the re-key touches — regime, poison boundary, dedup watermark, cursor/span,
  /// and the cache-push counter — so restoring one saved *before* the surgery simply undoes
  /// it: the pre-surgery regime, boundary, watermark, and position all return, and the cache
  /// re-lexes under the restored regime. Outstanding checkpoints therefore **remain valid**
  /// across state surgery — a raw [`restore`](Self::restore), an [`attempt`](Self::attempt)
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " rollback, and a [`StackedTransaction`] savepoint taken before the surgery all roll back"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " rollback, and a `StackedTransaction` savepoint taken before the surgery all roll back"
  )]
  /// across it cleanly.
  #[inline(always)]
  pub fn state_mut(&mut self) -> &mut L::State {
    self.rekey_offset_facts();
    self.state
  }

  /// Manually sets the lexer state (for context-sensitive lexing).
  ///
  /// # State replacement re-keys the input's forward-scanning facts
  ///
  /// Replacing the state can change how the region ahead of the cursor lexes, so this call
  /// re-keys every offset-dependent fact that governs forward scanning: the token cache is
  /// cleared (its entries were lexed under the old state and those offsets may lex
  /// differently now), the poison boundary is dropped, and the lexer-error dedup watermark
  /// is reset to the current committed cursor. Dropping the poison boundary is the
  /// documented limit-recovery path — swap in a fresh or bigger-budget state and scanning
  /// resumes past the old boundary.
  ///
  /// Speculative peek-ahead diagnostics emitted under the old state for the region beyond
  /// the cursor stay in the emitter log, and the watermark reset makes that same region
  /// re-reportable once it re-lexes under the new state: state surgery with outstanding
  /// speculative diagnostics may re-report the re-lexed region under the new regime, so
  /// callers should complete or roll back speculation before replacing state.
  ///
  /// # Transactional: checkpoints survive state surgery
  ///
  /// The re-key is itself **transactional**, not invalidating. A [`Checkpoint`] pure-copies
  /// every fact the re-key touches — regime, poison boundary, dedup watermark, cursor/span,
  /// and the cache-push counter — so restoring one saved *before* the surgery simply undoes
  /// it: the pre-surgery regime, boundary, watermark, and position all return, and the cache
  /// re-lexes under the restored regime. Outstanding checkpoints therefore **remain valid**
  /// across state surgery — a raw [`restore`](Self::restore), an [`attempt`](Self::attempt)
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " rollback, and a [`StackedTransaction`] savepoint taken before the surgery all roll back"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " rollback, and a `StackedTransaction` savepoint taken before the surgery all roll back"
  )]
  /// across it cleanly.
  #[inline(always)]
  pub fn set_state(&mut self, state: L::State) {
    // SETTLE_CENSUS: the state goes in by `mem::replace` and BEFORE the re-key, so the displaced
    // state's `Drop` — caller code — runs with the surgery already whole. Assigning after the
    // re-key, as this did, put that `Drop` between the re-key and the return: a host that caught
    // it found every offset-dependent fact re-keyed for a state that was, at that instant, still
    // being installed. The re-key itself reads only `self.span`, so it does not care which state
    // is in place when it runs.
    // SETTLE_CENSUS: the re-key's one fallible step runs FIRST, above the state write.
    //
    // An earlier repair moved the state write ahead of the re-key so the displaced state's
    // `Drop` could not land mid-surgery. That was right and it is still here — but it left the
    // re-key's own
    // `L::Offset::clone` below the state write, so an unwind there carried the NEW lexer state
    // beside a cache, poison boundary and dedup watermark still keyed to the OLD regime. Hoisting
    // the clone costs nothing: it reads `self.span`, which neither the state write nor the
    // install touches.
    let committed = self.rekey_committed();
    let displaced = core::mem::replace(self.state, state);
    let settled = self.install_rekey(committed);
    drop((displaced, settled));
  }

  /// Re-keys every offset-dependent fact to the current committed cursor — the shared body
  /// of the public state-surgery APIs [`set_state`](Self::set_state) and
  /// [`state_mut`](Self::state_mut).
  ///
  /// Replacing the lexer state changes how the region ahead of the cursor lexes, so every
  /// fact keyed to the dead regime's offsets is discarded:
  ///
  /// - the **token cache** is cleared — its entries were lexed under the old state and those
  ///   offsets may lex differently now;
  /// - the **poison boundary** is dropped — a latched limit belonged to the old regime, and
  ///   replacing the state is the documented limit-recovery path (a caller swaps in a
  ///   fresh/bigger-budget state and scanning resumes);
  /// - the **lexer-error dedup watermark** is reset to the current committed cursor — not
  ///   zero: forward scanning never revisits the region behind the committed cursor (a
  ///   consume only advances), so its already-reported errors stay deduplicated, while the
  ///   region ahead must be re-evaluatable under the new regime.
  ///
  /// The cache is cleared first so the cursor reads the committed position (the end of the
  /// last consumed token), which is exactly where a re-lex now resumes. The cache-push
  /// counter is deliberately left untouched: future saves snapshot its current value.
  ///
  /// # Restoring across this re-key is consistent (state surgery is transactional)
  ///
  /// A [`Checkpoint`] saved before the surgery restores cleanly across it — walk each fact it
  /// carries against this re-key and [`restore_unchecked`](Self::restore_unchecked):
  ///
  /// - **cursor / span / state (the regime)**: pure-copied back; the cursor follows from the
  ///   restored span and the emptied cache.
  /// - **poison boundary** and **dedup watermark**: pure-copied back, overwriting the
  ///   surgery's `None` / committed-cursor reset with the saved values.
  /// - **cache-push counter**: the surgery cleared the cache (`len 0`) but left the counter,
  ///   so restore's tail-drop (`min(cache.len(), pushes − saved)`) drops nothing, and the
  ///   pure-copy re-anchors the counter to the saved value — future deltas stay exact.
  /// - **cache contents**: emptied by the surgery, so restore re-lexes the region on demand
  ///   under the RESTORED state — the old regime — which the restored state field makes
  ///   correct.
  ///
  /// Every fact therefore returns to its pre-surgery value: the surgery is simply undone,
  /// like any other post-save mutation, so outstanding checkpoints remain valid across it.
  ///
  /// This re-key is exclusive to the public state-surgery APIs. Internal state *threading* —
  /// [`restore`](Self::restore)'s copy-back, the scan/consume paths writing
  /// `*self.state = lexer.into_state()`, and the cached-consume state adoption — is
  /// lineage-consistent by construction and never routes through here.
  #[inline]
  fn rekey_offset_facts(&mut self) {
    let committed = self.rekey_committed();
    drop(self.install_rekey(committed));
  }

  /// The **fallible half** of a re-key: the committed position, cloned.
  ///
  /// `L::Offset::clone` is caller code, and it is the only step of a re-key that can fail. Split
  /// out so a caller that has its own fact to write — `set_state` replaces `L::State` — can run
  /// it BEFORE writing anything, instead of inheriting a fallible step in the middle of its own
  /// surgery. Same shape as `clamped_span` ahead of `install_position`; the crate has one pattern
  /// for this and this is it.
  ///
  /// Read straight off `self.span` rather than through `cursor()`: the two are the same value at
  /// a re-key, because the clears in `install_rekey` empty the parked slot and the cache that
  /// `cursor()` would otherwise report from.
  #[inline(always)]
  fn rekey_committed(&self) -> L::Offset {
    self.span.end_ref().clone()
  }

  /// The **infallible half**: every offset-dependent fact, moved into place.
  ///
  /// Nothing here can fail before the last fact lands. The three writes are `mem::replace`/`take`
  /// so no displaced value is dropped in place, and they are handed back for the caller to drop
  /// once its whole surgery is done. The cache clear trails them deliberately — a cache is a pure
  /// memo, so dropping any subset of it is unobservable, and it is the only caller code in the
  /// body precisely because it is the one thing that cannot tear anything.
  #[must_use]
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  fn install_rekey(
    &mut self,
    committed: L::Offset,
  ) -> (
    L::Offset,
    Option<L::Offset>,
    Option<L::Offset>,
    Option<CachedTokenOf<'inp, L>>,
  ) {
    // SETTLE_CENSUS — the front-report watermark is disarmed BEFORE anything that can unwind.
    //
    // This body destroys the front of the stream: it takes the parked token, then clears the
    // cache. `Cache::clear` is a trait method — caller code — and it drops every cache entry,
    // each owning an `L::Token` and an `L::Span`, so caller code runs *inside* the destruction.
    // The watermark is therefore taken in the settled block below, above that clear: an unwind
    // through it leaves the watermark already `None`, and a later close-miss arm emits rather
    // than suppresses. Conservative in one direction only — noisy, never silent.
    //
    // Disarming after the clear instead leaves a window in which the front is gone and the
    // watermark still claims a live report for it. Measured, not hypothesized: with the disarm
    // below the clear, `a_rekey_interrupted_by_caller_code_still_witnesses_itself` fails.
    let settled = (
      core::mem::replace(self.emitted_error_end, committed),
      // The front-report watermark dies with the regime that lexed its subject. Taken HERE,
      // before the cache clear, because that clear is caller code: an unwind through it leaves
      // the watermark already disarmed, so a later close-miss arm EMITS. Noisy, never silent.
      self.front_reported_end.take(),
      self.poison_boundary.take(),
      self.pending.take(),
    );
    self.cache_mut().clear();
    settled
  }

  /// Returns a mutable reference to the emitter (borrowed through the session cell — see
  /// `input_ref::session` for why the borrow lives there).
  ///
  /// **Crate-private, and that is the wall.** `&mut Ctx::Emitter` *is* an emitter, so a parser
  /// handed one can wrap it and install the wrapper as the context of a second parse over a
  /// different buffer — the wrong-source class the lossless drivers' pinned emitter slot closes
  /// at the entry, re-opened from inside. Callers reach the emitter's **operations** through the
  /// forwarding methods in [`emit`](self::emit) and read its state through
  /// [`emitter_ref`](Self::emitter_ref); neither yields a value an input can be built around.
  #[inline(always)]
  pub(crate) const fn emitter(&mut self) -> &mut Ctx::Emitter {
    self.session.emitter
  }

  /// Emits an unexpected-token report about the **unconsumed** token at the stream front and, on
  /// success, publishes the front-report watermark for it.
  ///
  /// The one writer of the set direction. Pairing the publish with the append in a single body is
  /// what makes "watermark implies a live report" inductive: there is no ordering in which the
  /// watermark is armed for a report that was never appended, because a failed append returns
  /// before the publish.
  // `many`'s delimited/separated drivers are the only callers of the one-junk-token-one-report
  // pair and of the positional scanner witness.
  #[cfg(feature = "many")]
  #[inline]
  pub(crate) fn emit_unexpected_front(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
    front_end: L::Offset,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.session.emitter.emit_unexpected_token(err)?;
    *self.front_reported_end = Some(front_end);
    Ok(())
  }

  /// Whether the emitter currently holds a live unexpected-token report naming the front token
  /// ending at `end`.
  ///
  /// The read side of *one junk token, one report*. Frame-independent by construction, which is
  /// why every close-miss arm in the delimited drivers can ask it with one identical line.
  // `many`'s delimited/separated drivers are the only callers of the one-junk-token-one-report
  // pair and of the positional scanner witness.
  #[cfg(feature = "many")]
  #[inline(always)]
  pub(crate) fn front_report_live(&self, end: &L::Offset) -> bool {
    self.front_reported_end.as_ref() == Some(end)
  }

  /// Emits a lexer error unless the same region has already been reported.
  ///
  /// Peeking a window larger than the cache lexes past the cached region and emits
  /// any lexer errors it finds right away, so a peek-and-stop caller never loses
  /// them. Consuming that region later re-lexes it; this dedup — keyed on the error
  /// span's end against a high-water mark — guarantees every lexer error is reported
  /// exactly once, whether it is peeked, consumed, or both.
  ///
  /// LEXER_ERROR_CENSUS — **the** site that raises the layer's own lexer errors, and the crate's
  /// only caller of [`Emitter::commit_lexer_error`]. The span it hands over is the lexer's, over
  /// bytes this layer just lexed and refused, which is what makes it evidence a recording sink
  /// may license a gap with. Every *caller*-facing spelling (`InputRef::emit_lexer_error`,
  /// `EmitterView::emit_lexer_error`, `ParseState::emit_lexer_error`) goes to
  /// [`Emitter::emit_lexer_error`] instead and licenses nothing.
  ///
  #[inline(always)]
  fn emit_lexer_error_deduped(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let end = err.span_ref().end_ref().clone();
    if end <= *self.emitted_error_end {
      return Ok(());
    }
    // ARM BEFORE APPEND, deliberately — and the reverse has now been tried twice, so the reason
    // lives here rather than only at `Emitter::emit_lexer_error`'s doc.
    //
    // Raising a suppressing watermark and *then* doing something that can fail looks like the
    // arm-then-act defect this crate removed elsewhere (`install_rekey` and friends), and the
    // instinct to flip it is a strong one. It does not apply, because those windows arm and then
    // **destroy** — caller code runs between the halves and the destruction cannot be taken back,
    // so a panic strands a witness describing a front that no longer exists. Here the second step
    // is an *emission*, and for a rejecting emitter its failure **is** the delivery: an `Err` from
    // `emit_lexer_error` propagates and becomes the parse's error, so the region has been
    // surfaced either into the log or onto the error channel. Nothing is lost by arming first.
    //
    // Flipping is not merely unnecessary, it regresses a fix: where a caller catches the `Err` and
    // carries on, an unraised watermark lets a later scan over the same bytes re-offer the same
    // lexer error — exactly the duplicate this ordering was chosen to close.
    *self.emitted_error_end = end;
    self.emitter().commit_lexer_error(err)
  }

  /// Returns `true` if the input is poisoned by a sticky limit error.
  ///
  /// True whenever a poison boundary is latched, regardless of the current lex
  /// position. The *positional* question a scanner asks ("has my lex position reached the
  /// boundary?") is [`reached_boundary`](Self::reached_boundary); a poisoned input can
  /// still lex strictly before its boundary (e.g. to replay a drained prefix).
  ///
  /// Test-support observability: gated to exactly the feature set of its callers (the
  /// `logos` + `std` guard test suites), so it exists precisely when they do and is never
  /// dead code under `--tests` with leaner feature combinations.
  #[cfg(all(
    test,
    any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
    feature = "std"
  ))]
  pub(super) fn is_poisoned(&self) -> bool {
    self.poison_boundary.is_some()
  }

  /// Returns `true` if `pos` — the offset a scan would lex its next token at — has
  /// reached the poison boundary (a smaller boundary is more poisoned). At or past
  /// it a scanner yields its poisoned outcome without rebuilding a lexer; strictly
  /// before it, lexing proceeds normally.
  #[inline(always)]
  fn reached_boundary(&self, pos: &L::Offset) -> bool {
    matches!(self.poison_boundary.as_ref(), Some(b) if pos >= b)
  }

  /// Returns whether a terminal scanner stop sits at or past the **lex offset** (the back of the
  /// cache).
  ///
  /// This is the witness for a program point with **nothing at the front** (nothing parked, cache
  /// empty) — a committed peek/dispatch that maps
  /// its `None` outcome (`try_expect*`'s empty-cache scan path, [`peek`](Self::peek), the fused
  /// dispatch EOT arm): there the lex offset equals the committed cursor, so reading the offset is
  /// exactly reading the cursor. It is *not* attempt-relative where a prefilled cache can advance
  /// the lex offset to or past the boundary while the committed cursor lags behind — a resilient
  /// collection loop, whose element may only replay cached tokens, must use
  /// [`at_committed_boundary`](Self::at_committed_boundary) instead.
  #[inline(always)]
  pub(crate) fn at_latched_boundary(&self) -> bool {
    self.reached_boundary(self.offset())
  }

  /// Returns whether a terminal scanner stop — a resource-limit trip, or the poison boundary it
  /// latches — sits at or before the **committed cursor**.
  ///
  /// The attempt-relative witness for the resilient collection loops. A trip latches at the cursor
  /// it scans from ([`AtCursor`]); an element that reaches the poison consumes up to it, so its
  /// committed cursor lands at or past the boundary. Reading the *committed cursor* — not the lex
  /// offset [`at_latched_boundary`](Self::at_latched_boundary) reads — is what makes this
  /// attempt-relative: a prior lookahead can leave the cache prefilled with the lex offset already
  /// at the boundary, but an element that fails *ordinarily* while only replaying cached tokens
  /// before it leaves the committed cursor short of the boundary, so this stays `false` and the
  /// ordinary failure is not mis-charged as terminal. Those loops re-raise the element's own error
  /// on it — the input-side twin of [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal),
  /// needing no terminal bound on the emitter error type. Kept on the failure arm, so a successful
  /// element does zero terminal work.
  ///
  /// **Scanner stops only.** A *descent* budget trip latches no boundary — it has a control stack
  /// rather than a position — so this reads `false` for one. The loops pair it with
  /// [`tripped_during_attempt`](Self::tripped_during_attempt), the session-counter witness for that
  /// half; neither witness subsumes the other and both ride the same guard. The two are
  /// attempt-relative by different means — this one positionally, that one against a snapshot —
  /// but to the same end: neither may charge an ordinary element failure with a stop it did not
  /// cause.
  // `many`'s delimited/separated drivers are the only callers of the one-junk-token-one-report
  // pair and of the positional scanner witness.
  #[cfg(feature = "many")]
  #[inline(always)]
  pub(crate) fn at_committed_boundary(&self) -> bool {
    self.reached_boundary(self.cursor().as_inner())
  }

  /// Snapshots the terminal-stop latch for an attempt-relative absence witness.
  ///
  /// The latch is not monotone: a trip raises it (keeping the most-poisoned, smallest boundary), a
  /// checkpoint restore returns it to whatever the checkpoint saved — raising it, or clearing it —
  /// and a `rekey_offset_facts` rebase clears it outright. What holds is
  /// that every one of those transitions lands the field on a *value*, so plain equality against this
  /// snapshot is exactly the question an absence exit needs answered: is the latch the same one this
  /// attempt started with, or a different one it produced along the way?
  ///
  /// Taken once per driver attempt (not per element), so it costs one offset clone per collection.
  #[inline(always)]
  pub(crate) fn latch_snapshot(&self) -> Option<L::Offset> {
    self.poison_boundary.clone()
  }

  /// Returns whether a terminal scanner stop is latched *and differs from the one this attempt
  /// started with* ([`latch_snapshot`](Self::latch_snapshot)).
  ///
  /// The witness for a driver's **absence** exits — the no-progress stall and the element-decline
  /// break — which conclude "no more elements" from what the element saw. An element's own lookahead
  /// ([`peek`](Self::peek), [`peek_one`](Self::peek_one), [`peek_with_emitter`](Self::peek_with_emitter))
  /// latches a trip and still returns `Ok` with a short window, so an element can decline, or accept
  /// consuming nothing, on evidence a terminal stop truncated.
  ///
  /// Presence-plus-change, deliberately **not** positional. A positional reading (is the boundary at
  /// or before some cursor or offset?) is not restore-stable: an element may latch a trip, open an
  /// [`attempt`](Self::attempt), consume the cached pre-trip tokens, and decline — the restore rewinds
  /// the cursor, the cache and the watermark *behind* the boundary, while the latch survives because
  /// the checkpoint saved it after the trip took it. Every positional witness reads clean there
  /// though the stop is live and already diagnosed. Comparing values instead sees it, and still keeps
  /// the witness attempt-relative: a boundary an enclosing lookahead latched before the driver
  /// started compares equal, so it is never mis-charged to this attempt — the misattribution
  /// [`at_committed_boundary`](Self::at_committed_boundary) had to rule out for the element-error
  /// gates.
  #[inline(always)]
  pub(crate) fn latched_during_attempt(&self, since: &Option<L::Offset>) -> bool {
    self.poison_boundary.is_some() && *self.poison_boundary != *since
  }

  /// Snapshots the session's **scanner-trip counter** — how many times the scanner has tripped a
  /// lexer resource limit in this input session — for a rollback-proof terminality witness.
  ///
  /// The scanner twin of [`trip_snapshot`](Self::trip_snapshot), used identically: take the
  /// baseline once per attempt, hand it back to
  /// [`scanner_tripped_during_attempt`](Self::scanner_tripped_during_attempt) when judging that
  /// attempt.
  ///
  /// **This, and not [`latch_snapshot`](Self::latch_snapshot), is what a recovery gate judges a
  /// scanner stop with.** The latch is a lineage memo: a [`Checkpoint`](crate::input::Checkpoint)
  /// carries it and a restore copies it back, so comparing it across a rollback compares a restored
  /// value against what it was restored to. Reading it inside the attempt fixes one level and the
  /// level below reopens it — grammar code that catches a stop inside an inner
  /// [`try_attempt`](Self::try_attempt) has that rollback erase the latch before an outer gate
  /// looks. This counter is outside the rollback set entirely and is therefore depth-independent.
  ///
  /// Costs one `usize` load per attempt: no scan, no lookahead fill and no token commit reads it.
  #[inline(always)]
  pub(crate) const fn scanner_trip_snapshot(&self) -> usize {
    *self.scanner_trips
  }

  /// Whether the **scanner tripped a resource limit during the attempt** that took `since` as its
  /// [`scanner_trip_snapshot`](Self::scanner_trip_snapshot) baseline.
  ///
  /// The depth-proof input-side witness for scanner terminality, read by the recovery gate beside
  /// [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal) and
  /// [`tripped_during_attempt`](Self::tripped_during_attempt). It answers where the error value
  /// cannot: a *rejecting* emitter reports a scanner trip by returning the value its
  /// `From<<L::Token as Token>::Error>` builds, and nothing on that path constructs an
  /// [`UnexpectedEnd`](crate::error::UnexpectedEnd) for
  /// [`into_terminal`](crate::error::UnexpectedEnd::into_terminal) to mark.
  ///
  /// **Attempt-relative, not session-absolute**, and a **count** rather than a flag — the same two
  /// disciplines [`tripped_during_attempt`](Self::tripped_during_attempt) documents at length, for
  /// the same two reasons. Its granularity floor is the same as well: this witnesses that *a* trip
  /// happened while the attempt ran, not that the `Err` in hand *is* that trip, so a unit that
  /// catches a trip and then fails ordinarily is re-raised. It fails closed, never open.
  ///
  /// Costs one `usize` load and a comparison, on the failure arm only.
  #[inline(always)]
  pub(crate) const fn scanner_tripped_during_attempt(&self, since: usize) -> bool {
    *self.scanner_trips != since
  }

  /// Lexes the next token unless doing so would cross the poison boundary.
  ///
  /// Once the position the next token would be lexed at (`lex_at`, threaded by the
  /// caller and advanced to each token's end) reaches the boundary, returns `None`
  /// so the caller's end-of-input handling produces the poisoned outcome — the
  /// tripping token and everything after it is never re-scanned. With no boundary
  /// (or strictly before it) this is exactly [`Lexed::lex_spanned`].
  #[inline(always)]
  fn lex_within_boundary(
    &self,
    lexer: &mut L,
    lex_at: &mut L::Offset,
  ) -> Option<Spanned<Lexed<'inp, L::Token>, L::Span>> {
    if self.reached_boundary(lex_at) {
      return None;
    }
    let lexed = Lexed::<L::Token>::lex_spanned(lexer)?;
    // Lexer contract: every lexed item has a nonempty span. The span wraps both the
    // `Token` and `Error` variants here, and this is the input layer's only lexing
    // site, so this one check guards every scanner and peek path. A zero-width span at
    // the poison boundary would be excluded by the positional gate yet advance nothing,
    // silently breaking replay and termination; catch it loudly in debug builds.
    debug_assert!(
      lexed.span_ref().end_ref() > lexed.span_ref().start_ref(),
      "lexer contract violation: zero-width token span {:?}",
      lexed.span_ref(),
    );
    *lex_at = lexed.span_ref().end_ref().clone();
    Some(lexed)
  }

  /// Latches the input-level poison boundary if `lexer`'s state has tripped a limit
  /// error, recording `boundary` — the durable frontier (the offset up to which the
  /// pre-trip tokens stay reproducible by re-lexing) — as the trip position.
  ///
  /// A limit-class error is sticky: it manifests as a failing
  /// [`check`](crate::Lexer::check) (the exact condition the lexer's own latch keys
  /// on). Because `InputRef` rebuilds a fresh lexer per operation, that per-lexer
  /// latch would be lost; recording the frontier here bounds the work a recovering
  /// caller can trigger by re-entering a scanner. Returns whether it latched. A
  /// plain (non-limit) lexer error leaves `check()` `Ok` and does not latch, so the
  /// caller keeps scanning for the next valid token.
  ///
  /// This is the crate's **terminal predicate**, and [`classify`](Self::classify) — the sole caller
  /// — asks it *first*, before the partial-input frontier holdback is even considered. That order is
  /// the law: a tripped limit is terminal, so it may never be withheld as an
  /// [`Incomplete`](crate::error::Incomplete) merely because the tripping token landed on a chunk
  /// boundary.
  ///
  /// # The one writer of the scanner-trip counter
  ///
  /// It is also where [`Input::scanner_trips`](super::Input) is counted up, and being the sole
  /// terminal probe is exactly why the count lives here rather than in a driver's `Verdict::Trip`
  /// arm. [`classify`](Self::classify) is reached by **both** lexing drivers — the scanner
  /// ([`scan_with`](Self::scan_with)) and the peek fill — so counting here makes "every scanner
  /// trip is counted" a property of the module. Counting in `scan_with`'s trip arm instead would
  /// miss every trip a **lookahead** takes, and a lookahead that trips latches the boundary and
  /// still returns `Ok` with a short window, so those are precisely the trips an attempt can be
  /// judged over.
  ///
  /// The bump runs **before** the boundary write and before the caller offers the diagnostic to
  /// the emitter — the same ordering [`raise_level`](Self::raise_level) uses for the descent
  /// counter, and for the same reason: a rejecting emitter reports the trip by *returning* `Err`,
  /// so a count taken after the emit would be skipped by the very path the counter exists for.
  /// `wrapping_add`, again for the sibling's reason: the reading is an inequality against a
  /// per-attempt baseline, and wrapping is the one overflow behaviour under which consecutive
  /// values always differ.
  #[inline(always)]
  fn latch_if_limit_tripped(&mut self, lexer: &L, boundary: L::Offset) -> bool {
    if lexer.check().is_err() {
      // COUNT, then record where. The count is the fact ("a scanner budget was spent inside this
      // attempt"); the boundary is the position, and it is a lineage memo a rollback puts back.
      // Counting every detected trip — including one that does not lower an already-latched
      // boundary — is deliberate: the reading is per attempt, and a second trip inside a later
      // attempt must not compare equal to that attempt's baseline just because the position it
      // latched is one an earlier trip had already reached.
      *self.scanner_trips = self.scanner_trips.wrapping_add(1);
      // A trip can only maintain or increase poison: clamp to the more-poisoned
      // (smaller) of any existing frontier and this one. In practice a live scan
      // never reaches a trip past an already-latched boundary (it stops at the
      // boundary first), so this only ever records the frontier or lowers it.
      match self.poison_boundary.as_ref() {
        Some(existing) if *existing <= boundary => {}
        _ => *self.poison_boundary = Some(boundary),
      }
      true
    } else {
      false
    }
  }

  /// Returns `true` if reached the end of input.
  ///
  /// # Prefer [`is_exhausted`](Self::is_exhausted) in a loop gate
  ///
  /// This is a **frontier** question — *has the scanner reached the end of the buffer?* — and it
  /// answers `true` the moment any lookahead lexes through the end, while the tokens that
  /// lookahead produced are still sitting unconsumed in front of the caller. A driver loop gated
  /// on it therefore stops early exactly when someone peeked far enough, which makes the parse a
  /// function of the caller's lookahead history rather than of the token stream.
  #[inline(always)]
  #[doc(alias = "is_eof")]
  #[doc(alias = "end_of_input")]
  pub fn is_eoi(&self) -> bool {
    self.offset().ge(&self.input.len())
  }

  /// Returns `true` if the input is exhausted **for a consumer**: no lexed token is waiting and
  /// the lexer frontier has reached the end of the buffer.
  ///
  /// This is the predicate a driver loop wants, and [`is_eoi`](Self::is_eoi) is not it — see its
  /// docs for why a frontier question makes a loop stop as a function of how deep someone peeked.
  ///
  /// It is also **independent of the cache implementation**: a capacity that retains a token
  /// answers `false` because that token is waiting; a capacity that retains nothing answers
  /// `false` because its lex frontier is still behind that token's start.
  ///
  /// # `false` does not promise a token
  ///
  /// The frontier this reads is the end of the newest item the input *committed* or retained, and
  /// a plain [`next`](Self::next) drain never commits past the last token's end — so over a source
  /// with trailing lexer-skipped bytes this stays `false` after the stream is fully drained, in
  /// every capacity. The scans that settle at exhaustion ([`skip_while`](Self::skip_while) and the
  /// `sync` family, and therefore the `padded` combinators) do commit the lexer's end and do reach
  /// `true`. So a consume's own outcome is the authoritative end-of-stream signal and this
  /// predicate is the *gate*: it never turns `true` early, and its residual `false` is broken by
  /// the loop's own handling of an empty consume.
  ///
  /// # Partial mode
  ///
  /// On a non-final [`Partial`](crate::input::Partial) input this is the end of the *buffer*, not
  /// the end of the *stream*: `true` here means a consume would surface an
  /// [`Incomplete`](crate::error::Incomplete), not `None`. A refill driver must treat it as
  /// "ask for more bytes", never as "the construct ended".
  ///
  /// # Fuzz coverage
  ///
  /// In the fuzz alphabet as `Op::IsExhausted`; see `OP_SURFACE_CENSUS` in `src/fuzz/ops.rs`.
  #[inline(always)]
  pub fn is_exhausted(&self) -> bool {
    !self.has_front() && self.is_eoi()
  }

  /// Creates a lexer resuming at the **lookahead frontier** — the end of the newest token the
  /// consumer has not yet consumed, under the state that produced it — or, with nothing
  /// retained, at the committed position under the committed state.
  ///
  /// # The pair is read from ONE value
  ///
  /// A retained token carries its own post-token state (a [`CachedToken`] is exactly that
  /// pair), and that is the state the byte after it must be lexed under. Reading the offset
  /// from the retained token and the state from the *committed* field would resume at the right
  /// byte under a state from before the retained run: a widening lookahead then lexes token
  /// `k + 1` under the state from before token 1, a by-value [`Lexer::State`] limiter
  /// under-counts by the whole retained run, and the same grammar over the same input parses
  /// differently depending on how deep the caller peeked.
  #[inline(always)]
  pub fn lexer(&self) -> L
  where
    L::State: Clone,
  {
    self.resume().into_lexer()
  }

  /// The lookahead-frontier resume: [`lexer`](Self::lexer) plus the offset it was bumped to, as
  /// one value.
  ///
  /// Each arm reads **both** halves of the pair from a single carrier, which is what makes the
  /// pairing impossible to get wrong; the three-way match is the same one — in the same order —
  /// that [`offset`](Self::offset) walks, so the two always agree by construction.
  /// RESUME_CENSUS — one of exactly two `Resume` constructors.
  #[inline(always)]
  fn resume(&self) -> Resume<L, L::Offset>
  where
    L::State: Clone,
  {
    match self.cache().back() {
      // The newest retained token: its post-token state, at its end. One value, both facts.
      Some(back) => self.resume_from(back.state.clone(), back.token.span.end_ref()),
      None => {
        if Self::can_park() && self.pending.is_some() {
          // Nothing cached, but a token is parked at the front: it is then the newest retained
          // token, and carries the same pair.
          return self.resume_parked();
        }
        // Nothing retained: the committed pair, which is what `offset()` reports here too.
        self.resume_from(self.state.clone(), self.span.end_ref())
      }
    }
  }

  /// Cold parked arm of [`resume`](Self::resume) — see [`front_parked`](Self::front_parked).
  /// RESUME_CENSUS — reads BOTH halves of the pair from the parked token, one value.
  #[cold]
  #[inline(never)]
  fn resume_parked(&self) -> Resume<L, L::Offset>
  where
    L::State: Clone,
  {
    let parked = self.pending.as_ref().expect("checked by the caller");
    self.resume_from(parked.state.clone(), parked.token.span.end_ref())
  }

  /// The scan's resume: an [`AtFrontier`] is already the bundled (span, state) pair a scan
  /// threads, so this reads both halves from it. RESUME_CENSUS — the second of two.
  #[inline(always)]
  fn resume_at_frontier(&self, frontier: &AtFrontier<L::Span, L::State>) -> Resume<L, L::Offset>
  where
    L::State: Clone,
  {
    self.resume_from(frontier.state.clone(), frontier.span.end_ref())
  }

  /// Shared body: build a fresh lexer under `state`, bump it to `at`, and keep `at` as the lex
  /// position. Private to the two constructors above (RESUME_CENSUS), which is what keeps the
  /// (state, offset) pair from being assembled anywhere else.
  #[inline(always)]
  fn resume_from(&self, state: L::State, at: &L::Offset) -> Resume<L, L::Offset> {
    let mut lexer = L::with_state(self.input, state);
    lexer.bump(at);
    Resume {
      lexer,
      at: at.clone(),
    }
  }

  /// The span a write would store, clamped to the source — **computed, not stored**.
  ///
  /// Split out because it is the fallible half: the comparison and, on the clamping branch, the
  /// offset clone and the `L::Span` construction are all caller code. Separating it lets a caller
  /// that must write span and state *together* get every fallible step out of the way before
  /// either half is written. See [`commit_position`](Self::commit_position).
  #[inline(always)]
  fn clamped_span(&self, new: MaybeRef<'_, L::Span>) -> (L::Span, Option<L::Offset>) {
    let end = self.input.len();
    if new.end_ref().le(&end) {
      // The common branch does not consume `end`, and dropping it HERE would put an
      // `L::Offset::drop` — caller code — inside the settle window, between the token leaving the
      // stream and the position being installed. So it is handed back and dies with the rest,
      // after the settle. The clamping branch below moves it into the new span, so there is
      // nothing left to return and no second clone is introduced to make the shapes match.
      (to_owned(new), Some(end))
    } else {
      (L::Span::new(new.start_ref().clone(), end), None)
    }
  }

  /// SETTLE_CENSUS — writes the committed position: the span **and** the lexer state that
  /// produced it, as one step.
  ///
  /// They are a pair, and a torn pair is worse than either half being stale. A span advanced past
  /// a state that has not moved describes a position no execution reaches: a host that catches a
  /// panic and resumes lexes from the new offset under the old state, which for a stateful lexer —
  /// a limiter, an indentation stack, a mode stack — is silent stream corruption, and silent
  /// exactly for the population least able to notice. The committing modes have no snapshot, so
  /// nothing can restore them afterwards; atomicity here is their only protection.
  ///
  /// So: **both halves arrive already computed**, the one fallible step (the clamp) runs before
  /// anything is written, the two writes are infallible moves, and the values they replace are
  /// dropped LAST. That last part is not tidiness — `Drop` is caller code, and assigning in place
  /// would run it between the two writes, which is the tear this exists to prevent.
  ///
  /// Callers therefore evaluate their own caller-code operands first. `SyncTo::on_eof` is the site
  /// that proves why: it reads `lexer.span()` and `lexer.into_state()`, and running the second
  /// between the writes left `span = (11, 11)` paired with the entry state.
  #[inline(always)]
  fn commit_position(&mut self, span: MaybeRef<'_, L::Span>, state: L::State) {
    drop(self.replace_position(span, state));
  }

  /// SETTLE_CENSUS — the position write itself, handing back the pair it replaced.
  ///
  /// Returning the old values rather than dropping them is what lets a caller that is doing MORE
  /// than a position write put the drops last. A `Drop` is caller code, so dropping in place makes
  /// the position write an unwind site in the middle of whatever else the caller is restoring —
  /// which is how `restore_entry` could install a position and then never reach the dedup
  /// watermark, the poison latch or the emitter rewind. `#[must_use]` is the enforcement: with
  /// `#![deny(warnings)]` a caller that forgets the returned pair does not compile.
  #[must_use = "drop the replaced position AFTER the rest of the restore: dropping it here puts                 caller code in the middle of a write that must not be interrupted part-way"]
  #[inline(always)]
  fn replace_position(
    &mut self,
    span: MaybeRef<'_, L::Span>,
    state: L::State,
  ) -> (L::Span, L::State, Option<L::Offset>) {
    let (span, spare) = self.clamped_span(span);
    self.install_position(span, spare, state)
  }

  /// The **infallible half** of a position write: the two moves, and nothing else.
  ///
  /// `replace_position` is a fallible step (`clamped_span` runs `Source::len`, and its clamping
  /// branch an `Offset::clone`) followed by two `mem::replace`s that cannot fail. A restore needs
  /// those halves apart: it clamps up front, while the input is still wholly on the branch it is
  /// abandoning, and installs down here, where a panic would tear the rollback instead. Splitting
  /// the body rather than duplicating it keeps the pair funnel singular — the two `mem::replace`s
  /// that move the committed span and state still live in exactly one place (SETTLE_CENSUS).
  #[inline(always)]
  fn install_position(
    &mut self,
    span: L::Span,
    spare: Option<L::Offset>,
    state: L::State,
  ) -> (L::Span, L::State, Option<L::Offset>) {
    // ── commit point: two infallible moves, nothing between them ──
    let replaced_span = core::mem::replace(self.span, span);
    let replaced_state = core::mem::replace(self.state, state);
    (replaced_span, replaced_state, spare)
  }

  /// SETTLE_CENSUS — **the** primitive that settles a committed token: one call per token, at
  /// the moment it commits, on every consume path.
  ///
  /// A token is *committed* the instant no continuation of the current lineage can yield it
  /// again — popped off the cache front by a consume, or accepted straight off the lexer. All
  /// fourteen 1:1 consume settles route through here (the census in `census_tests.rs` holds the
  /// list and fails on drift), so a side channel that must observe committed tokens exactly
  /// once has exactly one home on the consume surface — plus the scanner's skip settle
  /// ([`AtFrontier::adopt`], its own censused site) — instead of a dozen.
  ///
  /// The body is the settle the sites always performed — the span write that makes
  /// [`span`](Self::span)/[`slice`](Self::slice) report the consumed token — plus **the**
  /// side-channel hook: one [`Emitter::commit_token`] call, the auto-emission chokepoint
  /// that makes a recording CST sink see every consumed token exactly once (the scanner's
  /// skip settle beside [`AtFrontier::adopt`] is the surface's censused second member).
  /// The state write stays at each site — its value is a site-specific move (cached
  /// extras, or the live lexer's state), and no side channel needs it. Both references are
  /// borrowed straight from the site's own token, and the defaulted emitter hook is an
  /// empty inlined body, so a build with no observer computes nothing extra and the call
  /// inlines to exactly the pre-hook code (the `__text`-hash standard holds it).
  ///
  /// **Non-settles must never route here**: peeks and declines (nothing committed),
  /// [`unconsume`](Self::unconsume) (the stopper is examined, not consumed), `settle_fatal`
  /// (the span written is a rejected *error's*, with no token to observe), `SyncTo::on_eof`
  /// (exhaustion, not a token), `commit_at` (its tokens already settled behind the frontier
  /// via `adopt`), and the position surgeries (`set_state`, the restore paths).
  ///
  /// # Two entrances, one settle
  ///
  /// The body is a fallible clamp followed by an infallible
  /// [`settle_committed_token`](Self::settle_committed_token), and the split is not cosmetic:
  /// this entrance is handed a token its caller has *already* removed from the stream, so its
  /// clamp runs in the window where the token is out and the position has not moved.
  /// [`commit_front`](Self::commit_front) is the other entrance, for a caller that removes the
  /// token itself and can therefore clamp first — it closes that window rather than repairing it
  /// afterwards. Both reach the side channel through the one infallible half, so the emitter hook
  /// still has a single home.
  #[inline(always)]
  fn commit_token(&mut self, tok: &L::Token, span: &L::Span, state: L::State) {
    // The state travels WITH the span. It used to be the caller's job to write it on the next
    // line, at sixteen call sites, and a caller that forgot published half a position: a
    // committed span paired with the lexer state of somewhere else. Taking it here makes that
    // unrepresentable rather than censused.
    //
    // SETTLE_CENSUS: the clamp — the settle's ONE fallible step — runs here, and the two halves
    // below cannot fail. Split for the same reason `replace_position` is split from
    // `install_position`: a caller that removes the token from the stream itself needs the
    // fallible half to run BEFORE the removal, and calls `commit_front` instead.
    let (clamped, spare) = self.clamped_span(span.into());
    self.settle_committed_token(clamped, spare, tok, span, state);
  }

  /// SETTLE_CENSUS — the **infallible half** of a token settle: install the already-clamped
  /// position, notify the committed-token side channel, and only then let the replaced pair go.
  ///
  /// `install_position` rather than `replace_position`: the clamp has already happened, and
  /// keeping it out of this body is the whole point. Everything here is a move or a foreign call
  /// made *after* the input is whole again.
  ///
  /// # Publish first, then notify — and the order is the opposite of what it looks like it
  /// should be
  ///
  /// Every caller reaches this having ALREADY taken the token off the front stream: a cache hit
  /// popped it, a lexed one was never there. So by the time the observer runs, the token is gone
  /// from the stream whatever happens next. Notifying first therefore does not mean "a panicking
  /// observer publishes nothing" — it means the position is left behind a token the stream no
  /// longer holds, with the younger cache entries still resident in front of it, and
  /// [`cursor`](Self::cursor) reads straight past the gap. Measured: committed span (0, 0)
  /// against a stream starting at 3, with the token vanished.
  ///
  /// Publishing first closes it. A panicking observer then leaves a consistent input — the token
  /// consumed, the position accounting for it — and only the side-channel notification missing,
  /// which is a documented observer-contract violation rather than a lost token. The replaced
  /// pair is held across the notification for the same reason: `L::Span::drop` and
  /// `L::State::drop` are caller code, and a panicking drop between the publish and the notify
  /// would enter the same hole through the other door.
  #[inline(always)]
  fn settle_committed_token(
    &mut self,
    clamped: L::Span,
    spare: Option<L::Offset>,
    tok: &L::Token,
    span: &L::Span,
    state: L::State,
  ) {
    let replaced = self.install_position(clamped, spare, state);
    // The settle observed: the one home of the committed-token side channel on the
    // consume surface (SETTLE_CENSUS locks the emitter-hook sites too).
    self.session.emitter.commit_token(tok, span);
    drop(replaced);
  }

  /// SETTLE_CENSUS / FRONT_CENSUS — consumes the token at the front of the stream with its
  /// position **already clamped**, and returns it.
  ///
  /// This is [`commit_token`](Self::commit_token) for a caller that takes the token out of the
  /// stream itself, with the two steps in the order that leaves no window: the caller runs
  /// [`clamped_span`](Self::clamped_span) — the settle's only fallible step, and caller code
  /// three times over (`Source::len`, an `L::Offset` comparison, an `L::Span::clone`) — while the
  /// token is **still in the stream**, and hands the answer here. Everything from the removal to
  /// the publish is then a move.
  ///
  /// The ordinary settle cannot do that: `commit_token` is handed a token its caller has already
  /// popped, so its clamp necessarily runs with the token out of the stream and the position
  /// still behind it. That window is the crate's long-standing posture for the 1:1 consume
  /// settles and is left alone; the trivia skip's **resident** run crosses a *run* of tokens
  /// rather than one, so it takes this entrance and closes the window by construction rather than
  /// by a scope whose `Drop` repairs it afterwards. Its **lexing** run does not reach here at all
  /// — a token it just lexed was never in the stream, so there is no removal to clamp ahead of,
  /// and it settles through `commit_token` like every other 1:1 consume.
  ///
  /// # Panics
  ///
  /// If the front of the stream is empty. The clamp the caller passes in was necessarily read off
  /// a token it found there, and nothing runs in between that could take it away.
  #[inline(always)]
  fn commit_front(&mut self, clamped: (L::Span, Option<L::Offset>)) -> Spanned<L::Token, L::Span> {
    let (span, spare) = clamped;
    // ── nothing fallible from here to the publish: a take, a destructure, two moves ──
    let (tok, state) = self
      .take_front()
      .expect("the caller clamped a token it read at the front a moment ago")
      .into_components();
    self.settle_committed_token(span, spare, tok.data(), tok.span_ref(), state);
    tok
  }

  /// Commits a scan at its [`AtFrontier`] frontier — the end of the last token it settled there,
  /// with the lexer state that produced it.
  ///
  /// A scan that consumes tokens as it goes accumulates them behind the frontier and writes the
  /// input's position back only when it stops; every such stop — a limit trip, a fatal emitter
  /// exit, the poison short-circuit, and a `to`-shaped stop — commits through this one call, so
  /// the position a scan leaves behind is a function of the tokens it skipped and nothing else.
  #[inline(always)]
  fn commit_at(&mut self, frontier: AtFrontier<L::Span, L::State>) {
    let AtFrontier { span, state } = frontier;
    self.commit_position(span.into(), state);
  }
}

impl<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Attempts to parse with the given function, rolling back on failure.
  ///
  /// A checkpoint is saved before `f` runs. If `f` returns `Some`, its progress is
  /// kept. If it returns `None`, the input rolls back to the checkpoint — position,
  /// lexer state, diagnostics emitted inside the attempt, the dedup watermark, and
  /// the poison boundary all return to their pre-attempt values.
  ///
  /// This is the recommended way to backtrack: the save/restore pair is scoped to the
  /// closure, so the last-in, first-out discipline documented on [`restore`](Self::restore)
  /// holds by construction, even under nesting.
  ///
  /// For a three-way flow — accept, decline, or a real error — reach for
  /// [`attempt_parse`](Self::attempt_parse), which speaks the crate's
  /// [`ParseAttempt`](crate::try_parse_input::ParseAttempt) vocabulary instead of making a
  /// decline borrow `Option`'s or `Result`'s.
  ///
  /// # Contract: the closure owns its span of the timeline
  ///
  /// The attempt saves at entry and settles at exit — commit-shaped on `Some`, restore-shaped
  /// on `None` — so the last-in, first-out law holds structurally and a declined attempt leaves
  /// **no trace** (the rewind story above: position, lexer state, emissions, watermark, poison
  /// boundary). One violation remains expressible, only under `unstable-raw`: a raw
  /// [`restore`](Self::restore) inside `f` to a checkpoint saved *before* the attempt began
  /// would tear out the attempt's own begin point (it pops it off the live lineage). Allocator
  /// builds pin that begin point, so such a restore **panics at the restore** — its message
  /// names a live transaction guard or attempt — rather than letting `f` continue on a torn
  /// foundation and detecting it only at the decline. A LIFO-clean raw save/restore pair taken
  /// and released entirely inside `f`, above the attempt's checkpoint, is unaffected.
  /// Allocator-less targets keep no pin set, so this mixing is unspecified-but-bounded there.
  /// Enforcing tests (in `src/input/input_ref/tests.rs`):
  /// `attempt_inner_raw_restore_below_checkpoint_panics_at_restore`,
  /// `attempt_inner_lifo_clean_raw_pair_is_legal`, and
  /// `attempt_backtrack_over_trip_reemits_diagnostic_exactly_once`.
  ///
  /// # A session point `f` opened and abandoned is legal, and the decline reconciles it
  ///
  /// `f` is **caller-supplied code holding a whole `InputRef`**, so it may open a
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " [session point](Self::begin_point) and leave it open. That is a liberty `begin_point`"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " session point and leave it open. That is a liberty `begin_point`"
  )]
  /// grants — an abandoned point keeps its progress and is released with the handle — not a bug
  /// this method may re-classify. So the decline settles through the **reconciling**
  /// [`rollback_abandoning_points`](Transaction::rollback_abandoning_points): every point younger
  /// than the attempt's base is abandoned (unpinned, lineage entry dropped, emitter mark
  /// released) and the rewind then subsumes its progress.
  ///
  /// It is not merely the kinder of two answers, it is the only **consistent** one. An abandoned
  /// point pins its base above the attempt's, and the checked
  /// [`rollback`](Transaction::rollback) refuses to rewind across a live pin — a release panic,
  /// in every allocator build, raised *before* anything is restored, leaving the speculative
  /// progress committed for a host that catches. The attempt's other settling exit for the same
  /// decision, an **unwind** out of `f`, has always reconciled instead: it is the guard's
  /// rolling-back `Drop`, and a `Drop` that may run mid-unwind can refuse nothing. Spelling the
  /// decline with the checked verb therefore made the same legal history panic when `f` returned
  /// and settle cleanly when `f` panicked. Both exits now restore the same thing.
  ///
  /// What this does **not** do is settle points for you. Only a rollback that reaches *below* a
  /// point abandons it, because only that destroys the lineage the point describes; an
  /// **accepted** attempt commits, and a point `f` left open is still open and still settleable
  /// afterwards — the non-lexical property session points exist for. Enforcing cells (in
  /// `src/input/input_ref/session_tests.rs`):
  /// `attempt_declining_across_an_abandoned_point_reconciles`,
  /// `attempt_panic_with_an_open_point_leaves_no_stranded_point`, and
  /// `attempt_accepting_leaves_the_closure_s_point_open_and_its_progress_kept`.
  ///
  /// # If the closure panics
  ///
  /// The begin point is *held* by a [`Transaction`] for the whole span of `f`, so an unwind out
  /// of `f` settles it exactly as a decline does — the guard's `Drop` rolls back to the begin
  /// point and releases its pin and its lineage id. A host that catches the unwind
  /// (`catch_unwind`: a test harness, a fuzzer, an editor server) is therefore handed an input
  /// that is still consistent and still usable, with nothing pinned on its behalf.
  ///
  /// For fallible closures that carry an error value, see
  /// [`try_attempt`](Self::try_attempt).
  pub fn attempt<F, R>(&mut self, f: F) -> Option<R>
  where
    F: FnOnce(&mut Self) -> Option<R>,
  {
    trace_event!(self, "attempt");
    // The begin point is *held* by a rollback-on-drop [`Transaction`], not by a bare local: `f`
    // is user code, and user code can unwind. A `Checkpoint` dropped by an unwind releases
    // neither its pin nor its lineage id, so a caught panic would strand a pinned begin point
    // that nothing can ever settle — a later restore reaching past it would then panic
    // spuriously, and the live stack would grow for the input's lifetime. The guard's `Drop` is
    // the crate's existing, silent, drop-safe settle, and it is what runs on that edge.
    let mut txn = self.guard_with::<Rollback>();

    match f(&mut txn) {
      // Progress kept: `commit` unpins the begin point and drops its lineage id, rather than
      // leaving either to grow the live stack. The now-decided guard's `Drop` is a no-op.
      Some(result) => {
        txn.commit();
        Some(result)
      }
      // Declined: the RECONCILING rollback, decided once here for the whole attempt family — see
      // the "a session point `f` opened and abandoned is legal" section above for why the checked
      // verb is the wrong one when the scope spans a caller-supplied closure. It unpins the begin
      // point FIRST (rolling back to it is
      // legal, so the restore must not see it pinned), then rewinds — position, lexer state,
      // emissions, dedup watermark, poison boundary — leaving no trace, after abandoning any
      // point `f` left open above the base. A raw restore *below* this checkpoint through `f`
      // would already have panicked at that restore (detect-at-cause), so the stale-base assert
      // inside the verb is an unreachable backstop, kept for defense in depth and for the
      // allocator-less path, which pins nothing.
      None => {
        txn.rollback_abandoning_points();
        None
      }
    }
  }

  /// Attempts to parse with a fallible function, rolling back on error.
  ///
  /// The `Result`-shaped sibling of [`attempt`](Self::attempt), for recovery- and
  /// pratt-style flows that need the failure value. A checkpoint is saved before `f`
  /// runs.
  ///
  /// - If `f` returns `Ok`, its progress is kept and the value is returned.
  /// - If `f` returns `Err`, the input rolls back to the checkpoint and the error is
  ///   returned to the caller. Everything the attempt touched returns to its
  ///   pre-attempt value: the position, the lexer state, the diagnostics emitted
  ///   inside the attempt, the dedup watermark, and the poison boundary.
  ///
  /// Like `attempt`, this is a structural way to backtrack: the save/restore pair is
  /// scoped to the closure, so the last-in, first-out discipline documented on
  /// [`restore`](Self::restore) holds by construction, even under nesting.
  ///
  /// # Contract: the closure owns its span of the timeline
  ///
  /// Exactly [`attempt`](Self::attempt)'s contract with `Err` as the declining shape: the
  /// last-in, first-out law holds structurally, a failed attempt leaves no trace, and the one
  /// remaining violation — a raw [`restore`](Self::restore) inside `f` to a checkpoint saved
  /// *before* the attempt (`unstable-raw` only) — **panics at the restore** in allocator
  /// builds, which pin the attempt's begin point, rather than letting `f` continue on a torn
  /// foundation. Allocator-less targets are unspecified-but-bounded there. Enforcing tests
  /// (in `src/input/input_ref/tests.rs`): `try_attempt_err_rolls_back_everything`,
  /// `try_attempt_nested_lifo`, and
  /// `try_attempt_inner_raw_restore_below_checkpoint_panics_at_restore`.
  ///
  /// The session-point clause carries over verbatim as well: `f` may open a point and abandon it,
  /// and the `Err` arm settles through the **reconciling**
  /// [`rollback_abandoning_points`](Transaction::rollback_abandoning_points) so that it restores
  /// exactly what an unwind out of `f` restores. See [`attempt`](Self::attempt)'s *"A session
  /// point `f` opened and abandoned is legal"* section for why the checked verb is the wrong one
  /// here. Enforcing cell (in
  /// `src/input/input_ref/session_tests.rs`):
  /// `try_attempt_erring_across_an_abandoned_point_reconciles`.
  ///
  /// # If the closure panics
  ///
  /// Exactly [`attempt`](Self::attempt)'s guarantee: the begin point rides in a [`Transaction`]
  /// for the whole span of `f`, so an unwind settles it like a decline — roll back, unpin,
  /// release the lineage id — and a host that catches the panic keeps a consistent input with
  /// nothing pinned on its behalf.
  pub fn try_attempt<F, T, E>(&mut self, f: F) -> Result<T, E>
  where
    F: FnOnce(&mut Self) -> Result<T, E>,
  {
    trace_event!(self, "try_attempt");
    // See `attempt`: the begin point rides in a rollback-on-drop [`Transaction`] for the whole
    // span of `f`, so an unwind out of user code settles it through the guard's `Drop` instead
    // of stranding a pin nobody can release.
    let mut txn = self.guard_with::<Rollback>();

    match f(&mut txn) {
      // Progress kept: `commit` unpins and drops the checkpoint's lineage id (see `attempt`).
      Ok(result) => {
        txn.commit();
        Ok(result)
      }
      // Declined: the reconciling rollback, for the reason `attempt` gives — this scope spans a
      // caller-supplied closure, so a point it opened and abandoned is a legal history and not a
      // rollback to refuse. Unpins FIRST (rolling back to the attempt's own base is legal) and
      // then rewinds; its stale-base assert is the same unreachable backstop `attempt` describes.
      Err(e) => {
        txn.rollback_abandoning_points();
        Err(e)
      }
    }
  }

  /// Speculation in the crate's own three-way vocabulary.
  ///
  /// - `Ok(Accept(v))` → progress kept (commit).
  /// - `Ok(Decline)` → rollback, **no trace** — a benign decline needs no fabricated
  ///   error, which is the whole reason this exists beside
  ///   [`try_attempt`](Self::try_attempt): a `try_*` production that wants to speculate
  ///   otherwise has to invent an error value to decline with and then unwrap it again.
  /// - `Err(e)` → rollback, and the error propagates untouched.
  ///
  /// The guard plumbing is exactly [`try_attempt`](Self::try_attempt)'s, so every one of
  /// its guarantees carries over verbatim: the last-in, first-out law holds structurally,
  /// a rolled-back attempt leaves no trace, the begin point rides a rollback-on-drop
  /// [`Transaction`] for the whole span of `f`, and **both** restoring arms settle through the
  /// reconciling [`rollback_abandoning_points`](Transaction::rollback_abandoning_points), so a
  /// session point `f` opened and abandoned is reconciled rather than refused — see
  /// [`attempt`](Self::attempt)'s *"A session point `f` opened and abandoned is legal"* section.
  /// Enforcing cells (in `src/input/input_ref/session_tests.rs`):
  /// `attempt_parse_declining_across_an_abandoned_point_reconciles` and
  /// `attempt_parse_erring_across_an_abandoned_point_reconciles`.
  ///
  /// # If the closure panics
  ///
  /// The unwind settles the transaction as a **decline** — roll back, unpin, release the
  /// lineage id — so a host that catches the panic keeps a consistent input with nothing
  /// pinned on its behalf.
  pub fn attempt_parse<F, T, E>(
    &mut self,
    f: F,
  ) -> Result<crate::try_parse_input::ParseAttempt<T>, E>
  where
    F: FnOnce(&mut Self) -> Result<crate::try_parse_input::ParseAttempt<T>, E>,
  {
    use crate::try_parse_input::ParseAttempt;
    trace_event!(self, "attempt_parse");
    let mut txn = self.guard_with::<Rollback>();
    match f(&mut txn) {
      Ok(ParseAttempt::Accept(v)) => {
        txn.commit();
        Ok(ParseAttempt::Accept(v))
      }
      // Both restoring arms take the reconciling rollback, for the reason `attempt` gives: the
      // scope spans a caller-supplied closure, and a point it opened and abandoned is a liberty
      // `begin_point` grants rather than a rollback to refuse.
      Ok(ParseAttempt::Decline) => {
        txn.rollback_abandoning_points();
        Ok(ParseAttempt::Decline)
      }
      Err(e) => {
        txn.rollback_abandoning_points();
        Err(e)
      }
    }
  }

  /// Closure-scoped span capture — the imperative twin of
  /// [`spanned`](crate::ParseInput::spanned), for hand-sequenced productions.
  ///
  /// Returns the span covering exactly what `f` consumed, alongside `f`'s value. The
  /// bracket is `spanned`'s own — [`cursor`](Self::cursor) before, `f`, then
  /// [`span_since`](Self::span_since) — so the two spellings cannot disagree about where
  /// a construct starts or ends.
  ///
  /// `f`'s error propagates unchanged and no span is produced: a failed production has no
  /// extent to report.
  pub fn spanning<F, T, E>(&mut self, f: F) -> Result<(L::Span, T), E>
  where
    F: FnOnce(&mut Self) -> Result<T, E>,
  {
    let cursor = self.cursor().clone();
    let value = f(self)?;
    Ok((self.span_since(&cursor), value))
  }

  /// Starts a transaction: a scoped, compile-time-safe form of [`save`](Self::save)
  /// and [`restore`](Self::restore).
  ///
  /// The returned [`Transaction`] guard mutably borrows this input; parse through the
  /// guard (it dereferences to `InputRef`), then decide with
  /// [`commit`](Transaction::commit) (keep the progress) or
  /// [`rollback`](Transaction::rollback) (return to the begin point). Dropping the
  /// guard without deciding rolls back — uncommitted speculative work is discarded, as
  /// in a database transaction. For a guard that instead *keeps* progress on drop
  /// (commit-by-default), use [`begin_with::<Commit>`](Self::begin_with).
  ///
  /// Prefer this for imperative flows with several exits (loops, `match` arms);
  /// [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt) for single-closure
  /// speculation; raw `save`/`restore` (feature `unstable-raw`) only where no guard shape fits.
  #[inline]
  pub fn begin(&mut self) -> Transaction<'_, 'inp, 'closure, L, Ctx, Lang, Rollback, Cmpl> {
    self.begin_with::<Rollback>()
  }

  /// Starts a transaction with an explicit [`DropPolicy`] — the canonical generic form of
  /// [`begin`](Self::begin).
  ///
  /// The type parameter `D` fixes what an *undecided* guard does on drop:
  ///
  /// - [`Rollback`] — restore to the begin point (the speculative default that
  ///   [`begin`](Self::begin) selects; drop discards the speculative work);
  /// - [`Commit`] — keep the progress (commit-by-default, the dual a Pratt-style operator
  ///   loop wants: keep progress on every success and every `?`-propagation, and roll back
  ///   explicitly only on the branches that back out).
  ///
  /// [`commit`](Transaction::commit) and [`rollback`](Transaction::rollback) are available
  /// on either flavour; only the *drop* behaviour differs.
  #[inline]
  pub fn begin_with<D: DropPolicy>(
    &mut self,
  ) -> Transaction<'_, 'inp, 'closure, L, Ctx, Lang, D, Cmpl> {
    trace_event!(self, "begin");
    self.guard_with::<D>()
  }

  /// The untraced core of [`begin_with`](Self::begin_with): saves the begin point, pins it, and
  /// hands both to a [`Transaction`], whose `Drop` owns their release from that moment on.
  ///
  /// Split out so [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt) can hold *their*
  /// begin point in the very same guard — the crate's one drop-safe answer to a pinned checkpoint
  /// that must outlive a call into user code — while still tracing under their own name.
  #[inline]
  fn guard_with<D: DropPolicy>(
    &mut self,
  ) -> Transaction<'_, 'inp, 'closure, L, Ctx, Lang, D, Cmpl> {
    // CAPTURE_WINDOW — preflight the pin set BEFORE the capture. `save` below registers a lineage
    // entry and an emitter mark that nothing can settle until the `Transaction` literal at the end
    // of this body exists, because `Checkpoint` has no `Drop` (it cannot reach the input or the
    // emitter). Between the two sits exactly one fallible step — the `pin_checkpoint` push onto the
    // pin set, a `Vec` by default and a `SmallVec` that spills past its inline capacity under
    // `smallvec_1`. If that growth unwound (`capacity overflow`, or a panicking allocator) the
    // capture would be dropped raw, and with no guard constructed nothing else would ever find its
    // pin, its lineage entry, or its mark. Reserving here means either this call panics with
    // nothing yet captured, or the push is a write into reserved capacity. Nothing in between
    // touches the pin set: `save` only pushes onto the *live-checkpoint* stack (whose own slot it
    // reserves for itself — see `save_checkpoint`).
    //
    // The rest of the window is allocation-free by inspection: `ckp.ckp_id` is a `u64` field read,
    // and the `Transaction` literal only moves (`Some(ckp)` is a move, `PhantomData` a ZST).
    #[cfg(any(feature = "std", feature = "alloc"))]
    self.reserve_pin_slot();
    let ckp = self.save();
    // Pin the begin point: a raw restore below it (through the guard's `DerefMut`) now panics at
    // the restore. Every settle path (commit, rollback, Drop — both policy flavors) unpins.
    #[cfg(any(feature = "std", feature = "alloc"))]
    self.pin_checkpoint(ckp.ckp_id);
    Transaction {
      input: self,
      ckp: Some(ckp),
      _policy: PhantomData,
    }
  }

  /// Starts a transaction that can hold several internal savepoints at once — the
  /// multi-fallback-point form of [`begin`](Self::begin).
  ///
  /// [`savepoint`](StackedTransaction::savepoint) marks a position;
  /// [`rollback_to`](StackedTransaction::rollback_to) returns to a mark, destroying every
  /// younger savepoint while the mark itself stays valid;
  /// [`release`](StackedTransaction::release) forgets savepoints while keeping the parsed
  /// progress; [`commit`](StackedTransaction::commit) /
  /// [`rollback`](StackedTransaction::rollback) decide the whole transaction. Savepoints
  /// follow SQL database semantics: rolling back to an older savepoint always destroys
  /// the newer ones — out-of-order revival is impossible by construction. A misused
  /// [`SavepointId`] is caught in layers: a temporally-misused id (kept past its
  /// transaction) at compile time via its lifetime brand, and a foreign or a stale id by a
  /// runtime check in every build; see [`SavepointId`].
  ///
  /// Raw [`save`](Self::save) / [`restore`](Self::restore), state replacement, and nested
  /// transactions are all reachable through the guard's deref; see the mixing rules on
  /// [`StackedTransaction`] for the one combination that invalidates a savepoint (a raw
  /// restore below it — it panics as stale in every build) and which are always legal (state
  /// surgery, nested speculation, and a LIFO-clean raw pair above the savepoints).
  ///
  /// Reach for the backtracking tools in order of shape:
  ///
  /// - [`begin`](Self::begin) / [`Transaction`] — a single speculative alternative with
  ///   several imperative exits (loops, `match` arms);
  /// - [`begin_stacked`](Self::begin_stacked) / [`StackedTransaction`] — **several live
  ///   fallback points at once** (best/longest-match selection: a savepoint after each
  ///   parsed stage, then `rollback_to` the best-scoring one);
  /// - [`attempt`](Self::attempt) / [`try_attempt`](Self::try_attempt) — closure-shaped
  ///   speculation;
  /// - [`begin_with::<Commit>`](Self::begin_with) — commit-by-default flows where progress
  ///   is kept on most exits;
  /// - [`begin_point`](Self::begin_point) session points — **non-lexical** speculation a driver
  ///   opens in one call and settles in a later one (the shape a borrowing guard cannot express).
  ///
  /// Raw [`save`](Self::save) / [`restore`](Self::restore) sit beneath all of these as the
  /// `unstable-raw` escape hatch — reachable only with that feature, for the rare shape no guard
  /// or session point fits.
  ///
  /// Dropping an undecided guard rolls back to the begin point; for a stacked guard that
  /// instead keeps its progress on drop, use
  /// [`begin_stacked_with::<Commit>`](Self::begin_stacked_with).
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  #[inline]
  pub fn begin_stacked(
    &mut self,
  ) -> StackedTransaction<'_, 'inp, 'closure, L, Ctx, Lang, Rollback, Cmpl> {
    self.begin_stacked_with::<Rollback>()
  }

  /// Starts a stacked transaction with an explicit [`DropPolicy`] — the canonical generic
  /// form of [`begin_stacked`](Self::begin_stacked) (see
  /// [`begin_with`](Self::begin_with) for the policy meanings).
  ///
  /// `D` fixes what an *undecided* guard does on drop: [`Rollback`] rolls back to the begin
  /// point, discarding all savepoints (the default [`begin_stacked`](Self::begin_stacked)
  /// selects); [`Commit`] keeps the parsed progress. The savepoint operations and
  /// `commit`/`rollback` are identical for either flavour.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  #[inline]
  pub fn begin_stacked_with<D: DropPolicy>(
    &mut self,
  ) -> StackedTransaction<'_, 'inp, 'closure, L, Ctx, Lang, D, Cmpl> {
    trace_event!(self, "begin_stacked");
    // CAPTURE_WINDOW — preflight the pin set BEFORE the capture, for exactly the reason
    // `guard_with` gives: `save` below registers a lineage entry and an emitter mark that only the
    // `StackedTransaction` literal at the end of this body can settle (a `Checkpoint` has no
    // `Drop`), and the `pin_checkpoint` push onto the pin set — `Vec` by default, an inline-spilling
    // `SmallVec` under `smallvec_1` — is the one allocation site inside that window. Reserving here
    // either panics with nothing yet captured or makes the push a write into reserved capacity;
    // nothing in between touches the pin set (`save` grows only the live-checkpoint stack, whose
    // slot it reserves for itself).
    //
    // The rest of the window is allocation-free by inspection: `base.ckp_id` is a `u64` field read,
    // and every field expression of the literal is infallible — `Some(base)` and `nonce` are moves,
    // `_policy` a ZST, and `saves: Default::default()` is `Vec::new`/`SmallVec::new`, neither of
    // which allocates. (The nonce is read before the capture, not after, so it is outside the
    // window either way.)
    self.reserve_pin_slot();
    // Nonce = the address of this Input's `poison_boundary` field, an Input-owned slot the
    // `InputRef` holds a `&mut` to. Two simultaneously-live Inputs are distinct structs at
    // distinct addresses (the field is never zero-sized), so their nonces differ and a
    // cross-parser id is caught at runtime; the `'txn` brand on `SavepointId` — not this
    // address — rules out the address-reuse case where a dropped Input's slot is later
    // reallocated. NOT the source pointer: two Inputs can share one `&str`.
    let nonce = core::ptr::from_ref(&*self.poison_boundary).addr();
    let base = self.save();
    // Pin the begin point (only the base — savepoints keep their detect-at-use staleness rule):
    // a raw restore below the base now panics at the restore. Every whole-transaction settle
    // path (commit, rollback, Drop) unpins the base.
    self.pin_checkpoint(base.ckp_id);
    StackedTransaction {
      input: self,
      base: Some(base),
      saves: Default::default(),
      nonce,
      _policy: PhantomData,
    }
  }

  /// Opens a **session point**: saves a checkpoint of the current position onto the input's
  /// internal point stack and **pins** its lineage id, exactly as a transaction guard pins its
  /// begin point. Returns nothing — and that is the whole feature.
  ///
  /// # The shape the guards cannot express
  ///
  /// Every guard ([`begin`](Self::begin), [`begin_stacked`](Self::begin_stacked)) and both
  /// attempts are **lexical**: the guard *is* a borrow of this input, so while one is alive the
  /// input is not, and the speculative scope can only end where the borrow does — inside one
  /// expression, one block, one call. A driver that is stepped across *separate method calls* — a
  /// REPL, an IDE that parses a fragment, speculates, and decides on a later call — cannot hold a
  /// guard beside the input it borrows: that value would be self-referential.
  ///
  /// A session point is a **value on the input**, not a borrow of it. `begin_point` takes
  /// `&mut self`, pushes, and returns; the borrow ends with the call, so the whole consume surface
  /// ([`next`](Self::next), [`peek`](Self::peek), [`try_expect`](Self::try_expect), any parser you
  /// hand this input to) stays callable *with the point still open*, in this call and in later
  /// ones:
  ///
  /// ```ignore
  /// let p = inp.begin_point();  // mark — nothing is borrowed afterwards
  /// let t = inp.next()?;        // parse, in this call or a later one
  /// let u = inp.next()?;        // …and again
  /// inp.rollback_point(p);      // unmark: cursor, span, state, cache, diagnostics all return
  /// ```
  ///
  /// Settle the point with [`commit_point`](Self::commit_point) (keep the progress) or
  /// [`rollback_point`](Self::rollback_point) (return to it), naming it by the
  /// [`SessionPointId`] this returns. The stack *is* the last-in, first-out order — points settle
  /// newest-first — so nesting stays structural; the id is what stops a settle from naming a
  /// *shifted* target, and what makes a stale one a refusal rather than a silent settle of
  /// whatever happens to be newest. [`points`](Self::points) is the live depth.
  ///
  /// # A point pins its base
  ///
  /// A session point is the base of a speculative scope, so it carries the same hazard a guard
  /// base does until it is settled: a rewind reaching *below* it would tear its foundation out.
  /// Two ways to reach it, both caller bugs, and each answered where it can be:
  ///
  /// - a **checked** rewind below the point — a raw [`restore`](Self::restore) (reachable only
  ///   under `unstable-raw`) or [`Transaction::rollback`](Transaction::rollback) — is refused
  ///   outright: the pin makes it **panic where it is requested** rather than corrupt the
  ///   timeline silently;
  /// - a **reconciling** rewind cannot refuse anything, so it abandons instead: every point
  ///   younger than its base is unpinned, its lineage entry dropped and its emitter mark
  ///   released — before the rewind, exactly as dropping the handle abandons a point still open.
  ///   The point's progress is not rolled back separately; the rewind subsumes it. Two shapes
  ///   reach it: a guard's or an attempt's **rollback on drop**, which *may not* refuse (a `Drop`
  ///   may run while already unwinding, where panicking is forbidden), and the explicit
  ///   [`Transaction::rollback_abandoning_points`](Transaction::rollback_abandoning_points),
  ///   which *chooses* not to.
  ///
  /// Those are the two *answers*, and which one a given rollback should be is a question about
  /// **who owns the points** — that verb's docs answer it. A scope that owns every point opened
  /// inside it keeps the refusal, because there a point still open above the base means the code
  /// that opened it lost track of its own speculation. A scope spanning foreign code does not:
  /// [`attempt`](Self::attempt), [`try_attempt`](Self::try_attempt) and
  /// [`attempt_parse`](Self::attempt_parse) hand a whole handle to a caller-supplied closure, and
  /// the typed pratt driver hands one to grammar hooks, so **all** of their explicit rollbacks
  /// reconcile — matching what their own unwind edge has always done. Abandoning is a liberty
  /// this method grants (see the drop section below); a rollback that answered it with a release
  /// panic would be re-classifying that liberty as a bug from the wrong side of the seam.
  ///
  /// Settle your points before the scope that opened them ends and neither arises. What never
  /// happens either way is the third outcome: a point left on the stack describing a lineage the
  /// rewind destroyed.
  ///
  /// # Contract: a point is scoped to *this handle*, and never outlives it
  ///
  /// A session point is non-lexical — it outlives the *call* that opened it — but it is **not**
  /// unbounded: it lives on this `InputRef` and dies with it. It cannot be carried to another
  /// handle, not even one taken from the same input, and this is a *law*, not a convention.
  ///
  /// The reason is what a [`Checkpoint`] carries. Among its facts is the **emitter's emission
  /// mark** — an index into the log of *the emitter this handle borrows*
  /// ([`Emitter::checkpoint`](crate::emitter::Emitter::checkpoint)), which
  /// [`rollback_point`](Self::rollback_point) replays into
  /// [`Emitter::rewind`](crate::emitter::Emitter::rewind). A point saved while emitter *A* was
  /// borrowed and settled while emitter *B* is would truncate *B*'s log at *A*'s mark: a diagnostic
  /// count from one timeline, applied to another. So a checkpoint is only meaningful within the one
  /// emitter borrow that produced it, and a session point — a checkpoint held across calls — must be
  /// scoped to that borrow.
  ///
  /// That scope *is* this handle: `as_ref` takes the emitter borrow, the handle holds it, and the
  /// borrow ends when the handle dies. The type system enforces it — the `'closure` brand on
  /// [`Checkpoint`] (and [`Cursor`]) is invariant in the emitter-borrow lifetime, so a checkpoint
  /// cannot even be *held* across the moment a second handle is taken from the same input; the
  /// attempt is a borrow error, not a runtime surprise. The point stack is therefore a field of the
  /// handle rather than of the input, on purpose.
  ///
  /// # Dropping the handle with points open: pins released, progress kept, nothing rewound
  ///
  /// Unlike a guard — whose drop rolls back (or, under [`Commit`], keeps) its undecided scope —
  /// dropping the handle with live session points performs **no rollback**. Their speculative work
  /// is *kept*: every token consumed, every diagnostic emitted, and every state change made through
  /// an open point stands, exactly as if each had been committed. A session ends *explicitly*;
  /// rolling an abandoned one back implicitly would silently paper over a driver that lost track of
  /// its own points — the deliberate opposite of a guard's drop policy — so the end is left explicit
  /// to surface that bug instead.
  ///
  /// # It is not merely the chosen policy — it is the only buildable one
  ///
  /// The deferral that carried this to the end of the campaign was framed as a *policy
  /// decision* between commit, rollback and a hybrid. Two of those three are not available from
  /// this drop site, which turns the decision into a structural fact plus a naming duty.
  ///
  /// `Session::drop` holds the lineage and the emitter, so it can release marks and pins. It
  /// does **not** hold the input itself: it cannot restore the span, the
  /// lexer state or the cursor. So "rollback on abandon" is not a posture this crate declined
  /// to take — it is unwritable from here. The one thing that *is* buildable, rewinding the
  /// emitter while keeping the position, is the **hybrid restore** the crate already refuses
  /// elsewhere, because a position without its matching emissions is the torn state the whole
  /// settle discipline exists to prevent.
  ///
  /// A `debug_assert!(points.is_empty())` on drop is also rejected, and for a reason worth
  /// stating rather than leaving as taste: abandonment via `?` through an enclosing rollback is
  /// a **legal** history, and an assert that fires on legal histories is exactly the class this
  /// crate has spent the campaign narrowing.
  ///
  /// So: an abandoned session point commits. Handle death keeps committed progress and releases
  /// the point's mark; a point you need rolled back must be settled by
  /// [`rollback_point`](Self::rollback_point) **before** the handle dies, because the drop site
  /// cannot reach the input to restore it. (Same argument family as "`Checkpoint` deliberately
  /// has no `Drop`".)
  ///
  /// What the drop *does* do is **release the bookkeeping**: each remaining point's pin and its
  /// live-checkpoint lineage entry are dropped from the input's lineage memos, and its emitter
  /// mark is [`release`](crate::emitter::Emitter::release)d (see the session cell's `Drop` in
  /// `input_ref::session`).
  /// It has to, precisely because the point is split across lifetimes — the [`Checkpoint`] dies
  /// with the handle, but the pin lives on the *input* and the mark-keyed bookkeeping in the
  /// *emitter*, and both outlive it. A pin left behind would stand for a point nobody can ever
  /// settle, so the pin set would no longer hold exactly the live begin points and would grow for
  /// the life of the input — and a mark never released would strand one row of an event sink's
  /// checkpoint stack per abandoned point, the same leak one layer up. Enforcing tests:
  /// `dropping_the_handle_releases_the_open_points`,
  /// `dropping_the_handle_keeps_the_progress_of_the_open_points`, and
  /// `a_second_handle_rewinds_across_an_abandoned_point` (in
  /// `src/input/input_ref/session_tests.rs`), and
  /// `abandoned_session_points_release_their_emitter_marks` (in `src/cst/sink/tests.rs`).
  ///
  /// The [`SessionPointId`] does not change this: an id merely dropped is not a signal, so a point
  /// whose id went out of scope is abandoned exactly like one whose driver forgot it, and its
  /// progress is still kept. The id makes a *settle* exact; keep-on-abandon is a separate, and
  /// deliberate, policy choice. `#[must_use]` is the one nudge available at the type level.
  ///
  /// # Fuzz coverage
  ///
  /// The abandon path is in the fuzz alphabet as `Op::SessionAbandon` (`session.abandon(drop)`);
  /// see `OP_SURFACE_CENSUS` in `src/fuzz/ops.rs`.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  #[inline]
  #[must_use = "a session point that is never settled is abandoned with the handle, keeping its \
                progress; hold the id to commit or roll it back"]
  pub fn begin_point(&mut self) -> SessionPointId<'closure> {
    trace_event!(self, "begin_point");
    // CAPTURE_WINDOW — preflight BOTH containers this point will land in, before anything is
    // captured. `save` below registers a lineage entry and an emitter mark that only the
    // `points.push` at the end of this body hands an owner (a `Checkpoint` has no `Drop`: it
    // cannot reach the input or the emitter to settle itself). If anything between the capture and
    // that push unwound — a `capacity overflow` panic, or a panicking allocator — the capture
    // would be dropped raw, its pin, lineage entry, and emitter mark stranded with no rollback,
    // commit, `forget_kept_checkpoint`, or release able to find them; and because no session point
    // was pushed, `Session::drop` could not release it either.
    //
    // The window holds exactly two allocation sites, in TWO DIFFERENT containers, so both are
    // reserved here:
    //
    // - `pin_checkpoint` pushes onto the lineage **pin set** (`Vec` by default, a `SmallVec` that
    //   spills past its inline capacity under `smallvec_1`);
    // - `points.push` pushes onto the session **point stack** (`Vec`).
    //
    // Reserving both first means either this preflight panics with nothing yet captured, or every
    // remaining step is a write into reserved capacity. Nothing in between consumes either
    // reservation: `save` grows only the live-checkpoint stack (whose own slot it reserves for
    // itself — see `save_checkpoint`), and neither it nor anything else here pushes onto the pin
    // set or the point stack.
    //
    // The rest of the window is allocation-free by inspection: `ckp.ckp_id` is a `u64` field read
    // and `SessionPointId::new` is a `const fn` over two `Copy` scalars plus `PhantomData`.
    self.session.points.reserve(1);
    self.reserve_pin_slot();
    // Nonce = the address of this Input's `poison_boundary` field, exactly as `begin_stacked`
    // derives it (see there for why this slot, and why the brand — not the address — answers
    // address reuse). Taken through `&*`, so it is the address of the *pointee*, which lives in
    // the `Input` this handle mutably borrows for its whole life: it therefore identifies the
    // input rather than the handle, and is the same value at the settle however the handle was
    // reborrowed — or moved — in between.
    let nonce = core::ptr::from_ref(&*self.poison_boundary).addr();
    let ckp = self.save();
    // The id names the point by its never-reused checkpoint id, so it stays exact however the
    // stack moves under it, and by the input's nonce, because that id is only unique *within* an
    // input.
    let id = SessionPointId::new(ckp.ckp_id, nonce);
    // Pin the base exactly like a guard: a rewind reaching below this point now panics at that
    // rewind instead of silently invalidating the session's foundation. Every settle path unpins —
    // `commit_point`, `rollback_point`, the enclosing rewind's reconciliation, and the handle's
    // `Drop` for a point abandoned outright.
    self.pin_checkpoint(ckp.ckp_id);
    self.session.points.push(ckp);
    id
  }

  /// Takes the newest open session point after checking that `point` names it.
  ///
  /// The whole runtime half of [`SessionPointId`]'s guarantee. Four refusals, each in every
  /// build, because settling the wrong point corrupts a timeline silently:
  ///
  /// - `point` belongs to a **different input** — the one misuse the `'closure` brand cannot
  ///   separate (two inputs borrowed in a single scope unify their brands). It is refused *first*,
  ///   before the checks below touch the stack, because a checkpoint id is unique only within one
  ///   input: every input numbers from the same start, so the scans would find a genuine match on
  ///   the wrong input and settle its point;
  /// - nothing open at all — the caller has lost track of its own points;
  /// - `point` is open but not the newest — settling it would collapse the ones above it, which
  ///   the newest-first law does not allow;
  /// - `point` is not open at all — already settled, or abandoned by an enclosing rollback that
  ///   reached below it. A positional settle would silently have taken whatever was newest.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  fn take_point(
    &mut self,
    point: SessionPointId<'closure>,
    verb: &str,
  ) -> Checkpoint<'inp, 'closure, L> {
    // The input's identity, derived exactly as `begin_point` stamped it. Stable across the
    // handle's reborrows because it addresses the `Input`-owned slot, not this handle's field.
    assert!(
      point.nonce() == core::ptr::from_ref(&*self.poison_boundary).addr(),
      "foreign session point: it belongs to a different input"
    );
    match self.session.points.last() {
      Some(newest) if newest.ckp_id == point.ckp() => {}
      Some(_) => {
        if self
          .session
          .points
          .iter()
          .any(|open| open.ckp_id == point.ckp())
        {
          panic!(
            "session point settled out of order: a younger point is still open (points settle \
             newest-first)"
          )
        }
        panic!("stale session point: it was already settled, or an enclosing rollback abandoned it")
      }
      None => panic!("no live session point to {verb}"),
    }
    self
      .session
      .points
      .pop()
      .expect("just observed as the newest open point")
  }

  /// Settles the session point `point` names by **committing** it: pops it off the internal stack,
  /// releases its pin, and keeps every bit of progress made since it opened — the consuming
  /// [`commit`](Self::commit) that releases the checkpoint's lineage entry.
  ///
  /// # Panics
  ///
  /// Panics with a message prefixed `no live session point` when nothing is open, and refuses a
  /// `point` that belongs to another input (`foreign session point`), is not the newest open one
  /// (`session point settled out of order`), or is no longer open at all (`stale session point`) —
  /// see [`SessionPointId`].
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  #[inline]
  pub fn commit_point(&mut self, point: SessionPointId<'closure>) {
    trace_event!(self, "commit_point");
    let ckp = self.take_point(point, "commit");
    // Kept, not restored: unpin the base, then the raw consuming commit keeps the progress and
    // releases the lineage entry.
    self.unpin_checkpoint(ckp.ckp_id);
    self.commit(ckp);
  }

  /// Settles the session point `point` names by **rolling back** to it: pops it off the internal
  /// stack, releases its pin **first** — so restoring to the point does not trip its own pin,
  /// mirroring the guards' settle ordering — then performs the checked [`restore`](Self::restore).
  /// Position, span, lexer state, token cache, emission log, dedup watermark, and poison boundary
  /// all return to where the point opened.
  ///
  /// # Panics
  ///
  /// Panics with a message prefixed `no live session point` when nothing is open, and refuses a
  /// `point` that belongs to another input (`foreign session point`), is not the newest open one
  /// (`session point settled out of order`), or is no longer open at all (`stale session point`) —
  /// see [`SessionPointId`].
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  #[inline]
  pub fn rollback_point(&mut self, point: SessionPointId<'closure>) {
    trace_event!(self, "rollback_point");
    let ckp = self.take_point(point, "roll back");
    // Unpin the base FIRST so the checked restore below does not see the point's own begin point
    // as pinned — rolling back to it is legal. A rewind *below* it would already have panicked at
    // that rewind (the pin's detect-at-cause check).
    self.unpin_checkpoint(ckp.ckp_id);
    self.restore(ckp);
  }

  /// The number of live session points — the depth of the speculation stack
  /// [`begin_point`](Self::begin_point) pushes onto, for a driver tracking where it sits in a
  /// nested speculation.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  #[inline(always)]
  pub fn points(&self) -> usize {
    self.session.points.len()
  }

  /// Hands out the next input-global savepoint sequence number; see
  /// [`Lineage::next_savepoint_seq`](super::Lineage::next_savepoint_seq) for the uniqueness
  /// invariant it maintains.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  pub(super) fn next_savepoint_seq(&mut self) -> u64 {
    self.session.lineage.next_savepoint_seq()
  }

  /// Drops `id` from the live-checkpoint lineage stack because its checkpoint was kept
  /// (committed) rather than restored. Lineage-only, and deliberately private to this module:
  /// the sole caller is [`forget_kept_checkpoint`](Self::forget_kept_checkpoint), which pairs
  /// this with the emitter-mark [`release`](Emitter::release) so the two cannot come apart
  /// (RELEASE_CENSUS). See [`Lineage::forget`](super::Lineage::forget) for the bounding
  /// invariant and its cost.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  fn forget_checkpoint(&mut self, id: u64) {
    self.session.lineage.forget(id);
  }

  /// RELEASE_CENSUS — **the** settle for a checkpoint whose branch was kept: drops its
  /// live-checkpoint lineage entry and [`release`](Emitter::release)s its emitter mark in one
  /// body, so lineage hygiene and emitter-bookkeeping hygiene cannot come apart.
  ///
  /// Every commit-shaped path routes through here — `commit_checkpoint` (the raw
  /// [`commit`](Self::commit) and the session [`commit_point`](Self::commit_point)), both
  /// [`Transaction`] commit arms (explicit and on-drop), and the [`StackedTransaction`]
  /// savepoint-release/commit/drop paths — because each holds the full [`Checkpoint`] at the
  /// exact moment its mark becomes unrewindable. The abandoning settle is the restore family,
  /// which *spends* the mark through [`Emitter::rewind`] instead; every checkpoint the crate
  /// takes ends in exactly one of the two (the census in `census_tests.rs` locks the sites).
  ///
  /// Assert-free and silent on purpose: the guards' commit-on-drop arms run inside `Drop`,
  /// possibly mid-unwind. The one keeper that does **not** route through this funnel is a
  /// session point abandoned with its handle: `Session::drop` performs the same
  /// unpin/forget/release settle itself, through the assert-free `Lineage` primitives (a
  /// mid-unwind drop must not assert) — the cell holds the emitter borrow precisely so it
  /// can. The census locks both homes.
  #[inline(always)]
  fn forget_kept_checkpoint(&mut self, checkpoint: Checkpoint<'inp, '_, L>) {
    #[cfg(any(feature = "std", feature = "alloc"))]
    self.forget_checkpoint(checkpoint.ckp_id);
    self.emitter().release(checkpoint.emitter_checkpoint);
  }

  /// Returns whether `id` is still live on the lineage stack; see
  /// [`Lineage::contains`](super::Lineage::contains).
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  pub(super) fn live_contains(&self, id: u64) -> bool {
    self.session.lineage.contains(id)
  }

  /// Pops the lineage stack down through `id` inclusive on restore; see
  /// [`Lineage::pop_through`](super::Lineage::pop_through).
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  fn live_pop_through(&mut self, id: u64) {
    self.session.lineage.pop_through(id);
  }

  /// Pins `id` — the begin-point checkpoint of a transaction guard, an
  /// [`attempt`](Self::attempt), or a [session point](Self::begin_point) — so a raw
  /// [`restore`](Self::restore) reaching below it panics at the restore. Every guard constructor
  /// ([`begin_with`](Self::begin_with), [`begin_stacked_with`](Self::begin_stacked_with)),
  /// [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt), and
  /// [`begin_point`](Self::begin_point) pins on entry; the matching
  /// [`unpin_checkpoint`](Self::unpin_checkpoint) runs on every settle path. See
  /// [`Lineage::pin`](super::Lineage::pin) for the borrowck-serialization argument (session points
  /// are serialized instead by their own last-in, first-out stack).
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  pub(crate) fn pin_checkpoint(&mut self, id: u64) {
    self.session.lineage.pin(id);
  }

  /// CAPTURE_WINDOW — reserves the pin set's slot so the
  /// [`pin_checkpoint`](Self::pin_checkpoint) that follows a capture cannot allocate.
  ///
  /// Every pinning site ([`guard_with`](Self::guard_with) — hence
  /// [`begin_with`](Self::begin_with) and both attempts —
  /// [`begin_stacked_with`](Self::begin_stacked_with), and [`begin_point`](Self::begin_point))
  /// takes its capture first and only then pins it, so the pin push sits *inside* the window
  /// where the capture has no owner. Call this **before** the capture; see
  /// [`Lineage::reserve_pin`](super::Lineage::reserve_pin) for the full argument.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  pub(crate) fn reserve_pin_slot(&mut self) {
    self.session.lineage.reserve_pin();
  }

  /// Removes `id` from the pin set when its guard, attempt, or session point settles; see
  /// [`Lineage::unpin`](super::Lineage::unpin). Called on **every** settle path (commit, explicit
  /// rollback, `Drop`, both closure arms of the attempts, and both session-point verbs). A session
  /// point abandoned with the handle settles through this handle's `Drop`, which reaches
  /// [`Lineage::unpin`](super::Lineage::unpin) directly — a `Drop` impl may not add the
  /// `L::State: Clone` bound this method's impl block carries.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  pub(crate) fn unpin_checkpoint(&mut self, id: u64) {
    self.session.lineage.unpin(id);
  }

  /// Panics if restoring to `target_id` would pop a **pinned** checkpoint off the live lineage —
  /// the detect-at-cause check that refuses a raw restore below a live guard/attempt begin point,
  /// in every allocator build. See
  /// [`Lineage::assert_restore_preserves_pins`](super::Lineage::assert_restore_preserves_pins)
  /// for why a guard's own settle, a savepoint `rollback_to`, and a dead target never trip it.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[inline]
  fn assert_restore_preserves_pins(&self, target_id: u64) {
    self
      .session
      .lineage
      .assert_restore_preserves_pins(target_id);
  }

  /// The number of live checkpoints — test-only observability for the no-growth
  /// guarantee that committing (and a success-path [`Recover`](crate::parser::Recover))
  /// gives the lineage stack (see [`Lineage::live_len`](super::Lineage::live_len)).
  ///
  /// The stack it measures is maintained in every allocator build, so this accessor is gated
  /// only to its callers — the `logos` + `std` guard and recover test suites — and *not* to
  /// `debug_assertions` or `target_has_atomic = "ptr"`, so the no-growth cases can run under the
  /// release profile too. Keeping the `logos` + `std` constraint (rather than the looser
  /// `any(std, alloc)`) keeps the method from being dead code under
  /// `cargo hack --each-feature --tests`, whose single-feature combinations never enable both
  /// `logos` and `std` and so compile neither this method nor its callers.
  #[cfg(all(
    test,
    any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
    feature = "std"
  ))]
  pub(crate) fn live_checkpoints_len(&self) -> usize {
    self.session.lineage.live_len()
  }

  /// Returns a slice of the current token from the input source.
  #[inline(always)]
  pub fn slice(&self) -> <L::Source as Source<L::Offset>>::Slice<'inp> {
    self
      .input
      .slice(self.span.start()..self.span.end())
      .expect("lexer should guarantee slice")
  }

  /// Returns a slice of the input source from the given cursor to the current position.
  #[inline(always)]
  pub fn slice_since(
    &self,
    cursor: &Cursor<'inp, 'closure, L>,
  ) -> Option<<L::Source as Source<L::Offset>>::Slice<'inp>> {
    let end = self.cursor();
    self
      .input
      .slice(cursor.as_inner().clone()..end.as_inner().clone())
  }

  /// Returns a slice of the input source from the given cursor to the end of the input.
  #[inline(always)]
  pub fn slice_from(
    &self,
    cursor: &Cursor<'inp, 'closure, L>,
  ) -> Option<<L::Source as Source<L::Offset>>::Slice<'inp>> {
    let start = cursor.as_inner().clone();
    self.input.slice(start..)
  }

  /// Returns a slice of the input source for the given cursor range.
  #[inline(always)]
  pub fn slice_range<'r, R>(
    &self,
    range: R,
  ) -> Option<<L::Source as Source<L::Offset>>::Slice<'inp>>
  where
    R: RangeBounds<&'r Cursor<'inp, 'closure, L>>,
    'closure: 'r,
  {
    let start = range.start_bound().map(|c| c.as_inner().clone());
    let end = range.end_bound().map(|c| c.as_inner().clone());
    // SAFETY: The range is guaranteed to be within bounds as both cursors are within input length and comes from the same input.
    self.input.slice((start, end))
  }

  /// Returns the span of the current position.
  #[inline(always)]
  pub const fn span(&self) -> &L::Span {
    self.span
  }

  /// Returns a span from the given cursor to the current position.
  #[inline(always)]
  pub fn span_since(&self, cursor: &Cursor<'inp, 'closure, L>) -> L::Span {
    Span::new(cursor.as_inner().clone(), self.cursor().as_inner().clone())
  }

  /// Returns a span from the given cursor to the end of the input.
  #[inline(always)]
  pub fn span_from(&self, cursor: &Cursor<'inp, 'closure, L>) -> L::Span {
    Span::new(cursor.as_inner().clone(), self.input.len())
  }

  /// Returns a span for the given cursor range.
  #[inline(always)]
  pub fn span_range(&self, range: Range<&Cursor<'inp, 'closure, L>>) -> L::Span {
    Span::new(range.start.as_inner().clone(), range.end.as_inner().clone())
  }

  /// Saves the current state as a [`Checkpoint`] for backtracking.
  ///
  /// # Unstable: feature-gated raw API
  ///
  /// `save` is one third of the raw checkpoint triple (`save` / [`restore`](Self::restore) /
  /// [`commit`](Self::commit)) and is public **only** under the `unstable-raw` feature; without
  /// it the method is crate-internal, so a [`Checkpoint`] can be neither obtained nor consumed
  /// from another crate. The supported backtracking surface is the transaction guards
  /// ([`begin`](Self::begin) / [`begin_stacked`](Self::begin_stacked)), the
  /// [session points](Self::begin_point), and
  /// [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt) — together these cover every
  /// legal backtracking shape. The last-in, first-out / lineage contract documented here and on
  /// [`restore`](Self::restore) governs the raw triple unchanged whenever the feature is on.
  ///
  /// The checkpoint captures the cursor, the last-consumed span, the lexer state, the
  /// emitter's emission mark, the lexer-error dedup watermark, and the poison
  /// boundary — everything [`restore`](Self::restore) needs to make this exact moment
  /// the live state again.
  ///
  /// Saving is amortized O(1): it clones the lexer state and a few offsets, and — in
  /// allocator builds — records the checkpoint's id on the input's live-checkpoint
  /// lineage stack (one `Vec` push) so restore ordering and savepoint validity can be
  /// tracked in every build; allocator-less builds allocate nothing. Saving never
  /// invalidates other checkpoints; only restoring does (see [`Checkpoint`]'s validity
  /// section).
  ///
  /// Every checkpoint `save` returns should end in exactly one of [`restore`](Self::restore)
  /// (abandon this branch and rewind) or [`commit`](Self::commit) (keep this branch's progress
  /// and release the checkpoint's lineage entry); a checkpoint merely dropped keeps its progress
  /// but strands that lineage entry until an older restore pops through it.
  ///
  /// Prefer [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt) when the
  /// save/restore pair brackets a single speculative computation — they enforce the
  /// restore discipline by construction.
  #[cfg(feature = "unstable-raw")]
  #[cfg_attr(docsrs, doc(cfg(feature = "unstable-raw")))]
  #[inline(always)]
  pub fn save(&mut self) -> Checkpoint<'inp, 'closure, L> {
    self.save_checkpoint()
  }

  /// The crate-internal raw `save`, used when the `unstable-raw` valve is off — the primitive the
  /// transaction guards, [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt), and the
  /// [session points](Self::begin_point) build on. Same body as the public flavor;
  /// only its visibility differs.
  #[cfg(not(feature = "unstable-raw"))]
  #[inline(always)]
  pub(crate) fn save(&mut self) -> Checkpoint<'inp, 'closure, L> {
    self.save_checkpoint()
  }

  /// Shared body of the [`save`](Self::save) twins.
  ///
  /// # CAPTURE_WINDOW — the two registrations come last, and cannot unwind
  ///
  /// A capture is two registrations a later settle has to find: the **lineage entry**
  /// ([`Lineage::open`](super::Lineage::open)) and the **emitter mark**
  /// ([`Emitter::checkpoint`]). Neither has an owner until the [`Checkpoint`] this returns
  /// exists, and `Checkpoint` deliberately has no `Drop` — a `Drop` could not reach the input or
  /// the borrowed emitter to settle itself — so an unwind after the first registration and before
  /// the finished value strands it outright: no restore spends it, no `forget_kept_checkpoint`
  /// releases it, nothing knows it is there. The body is therefore ordered so that **every**
  /// fallible step runs while nothing is captured:
  ///
  /// 1. the clones and reads. `Cache::front_span`, and the `Cursor`/`L::Span`/`L::State`/
  ///    `L::Offset` clones, are all caller-supplied code that may allocate or panic; here an
  ///    unwind strands nothing at all;
  /// 2. the live-checkpoint stack's slot is reserved — the last fallible step, still with nothing
  ///    captured (see [`Lineage::reserve_open`](super::Lineage::reserve_open));
  /// 3. the emitter mark is taken. Nothing is registered on *this* side yet, so an unwind out of
  ///    the emitter strands nothing here; the crate's own `Sink` is itself fail-atomic at this
  ///    point (its captured inner reading is a pure value whose `release` is a no-op, so a failed
  ///    mark-row push owes no settle);
  /// 4. `Lineage::open` records the entry into the slot reserved in (2) — it cannot allocate, so
  ///    it cannot unwind, so the mark taken in (3) cannot be orphaned by it;
  /// 5. `Checkpoint::new` is a `const fn` that only moves its arguments.
  #[inline(always)]
  fn save_checkpoint(&mut self) -> Checkpoint<'inp, 'closure, L> {
    // (1) Every fallible step first — caller-supplied `Clone`/`Cache` code, with nothing captured
    // yet for an unwind to strand.
    let cursor = self.cursor().clone();
    let span = self.span.clone();
    let state = self.state.clone();
    let emitted_error_end = self.emitted_error_end.clone();
    let front_reported_end = self.front_reported_end.clone();
    let poison_boundary = self.poison_boundary.clone();
    let cache_pushes = self.session.lineage.cache_pushes();
    #[cfg(all(
      debug_assertions,
      any(feature = "std", feature = "alloc"),
      target_has_atomic = "ptr"
    ))]
    let input_id = self.witness.input_id();
    // (2) The live-checkpoint stack's slot, so the `open` in (4) is a write into reserved
    // capacity. Nothing between here and there pushes onto that stack.
    #[cfg(any(feature = "std", feature = "alloc"))]
    self.session.lineage.reserve_open();
    // (3) The emitter mark: the first registration, and deliberately the one taken while the
    // lineage side is still clean.
    let emitter_checkpoint = self.session.emitter.checkpoint();
    // (4) Open a lineage entry (every allocator build): take a fresh id, record it on the
    // live-checkpoint stack, and stamp it into the checkpoint. `restore` pops the stack down
    // through that id, and a `StackedTransaction` checks the id is still present before honoring
    // a savepoint — the check that makes stale savepoints panic on release and no-ptr targets.
    #[cfg(any(feature = "std", feature = "alloc"))]
    let ckp_id = self.session.lineage.open();
    // (5) Moves only.
    Checkpoint::new(
      cursor,
      span,
      state,
      emitter_checkpoint,
      emitted_error_end,
      front_reported_end,
      poison_boundary,
      cache_pushes,
      #[cfg(all(
        debug_assertions,
        any(feature = "std", feature = "alloc"),
        target_has_atomic = "ptr"
      ))]
      input_id,
      #[cfg(any(feature = "std", feature = "alloc"))]
      ckp_id,
    )
  }

  /// Returns the current cursor position.
  ///
  /// If there are cached tokens, the cursor points to the start
  /// of the first cached token; otherwise, it points to the current position.
  ///
  /// This is the lookahead (cache-front) position: a peek or a scan decline moves it across skipped
  /// bytes without committing anything. It is **not** a progress metric — for committed progress
  /// compare [`span().end()`](Self::span).
  #[inline(always)]
  pub fn cursor(&self) -> &Cursor<'inp, 'closure, L> {
    // Stream order is `parked?` then the cache, so a parked token — when there is one — is where
    // the next consume starts. Reading the cache front first would report a token that is NEWER
    // than the parked one.
    if Self::can_park() {
      if let Some(parked) = self.pending.as_ref() {
        return Cursor::from_ref(parked.token.span.start_ref());
      }
    }
    Cursor::from_ref(
      self
        .cache()
        .front_span()
        .map(|span| span.start_ref())
        .unwrap_or_else(|| self.span.end_ref()),
    )
  }

  /// Returns the current offset of the tokenizer.
  ///
  /// This is the end of the last lexed token (cached or otherwise).
  #[inline(always)]
  pub fn offset(&self) -> &L::Offset {
    // The NEWEST retained token — the mirror of `cursor`'s oldest. The cache back is newer than a
    // parked front, so it wins; the parked token only answers when nothing is cached.
    match self.cache().back_span() {
      Some(span) => span.end_ref(),
      None => {
        if Self::can_park() {
          if let Some(parked) = self.pending.as_ref() {
            return parked.token.span.end_ref();
          }
        }
        self.span.end_ref()
      }
    }
  }

  /// Rewinds the input to `checkpoint`'s save point.
  ///
  /// # Unstable: feature-gated raw API
  ///
  /// `restore` is part of the raw checkpoint triple ([`save`](Self::save) / `restore` /
  /// [`commit`](Self::commit)) and is public **only** under the `unstable-raw` feature; without
  /// it the method is crate-internal. The supported backtracking surface is the transaction
  /// guards ([`begin`](Self::begin) / [`begin_stacked`](Self::begin_stacked)), the
  /// [session points](Self::begin_point), and
  /// [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt); each enforces the
  /// last-in, first-out discipline below by construction. That contract applies to the raw
  /// triple unchanged whenever the feature is on.
  ///
  /// After a restore, the input behaves exactly as it did the moment the checkpoint
  /// was taken:
  ///
  /// - the cursor, last-consumed span, and lexer state are restored; consuming
  ///   resumes from the saved position. Cached tokens appended after the save belong to
  ///   the abandoned continuation and are dropped so their region re-lexes (re-emitting
  ///   any lexer error it held); tokens cached before the save re-lex identically — this
  ///   includes a pre-save cached token the abandoned branch already consumed out of the
  ///   cache: it is re-lexed on demand after the restore. By the `Lexer` determinism
  ///   contract that replay is identical (the same token and span, its diagnostics
  ///   exactly once, an in-`State` limiter recounting the same), while scan-count
  ///   instrumentation held outside the lexer state will observe the additional scans;
  /// - diagnostics emitted after the save are rolled back — the emitter's emission
  ///   log is truncated to the saved mark (see
  ///   [`Emitter::rewind`](crate::emitter::Emitter::rewind));
  /// - the lexer-error dedup watermark returns to its saved value: an error whose
  ///   emission was just rolled back becomes re-emittable — exactly once — if the
  ///   resumed parse reaches it again, while errors retained from before the save
  ///   stay deduplicated;
  /// - the poison boundary returns to its saved value: an input unpoisoned at save
  ///   time is unpoisoned again (a rolled-back limit trip re-trips and re-diagnoses
  ///   if re-reached); an input poisoned at save time gets the saved boundary and its
  ///   retained diagnostic back, still paired.
  ///
  /// # A checkpoint restores only into the handle that saved it
  ///
  /// A [`Checkpoint`] is branded with the `'closure` lifetime of the handle that
  /// [`save`](Self::save)d it, and that brand is **invariant**, so `restore` (and
  /// [`commit`](Self::commit)) accept only a checkpoint carrying *this* handle's own brand. Every
  /// handle a parser receives arrives through the closure that produced it (`apply` hands it a
  /// `for<'closure>` borrow), so any two handles carry rigidly distinct brands that cannot unify.
  /// Restoring a checkpoint that a *different* handle produced — even a second handle over the same
  /// source, reached through a nested parse — is therefore a **compile error**, not a runtime
  /// check. A debug assert additionally re-checks input identity as a backstop (see [Debug
  /// builds](#debug-builds)).
  ///
  /// ```compile_fail
  /// use tokora::{InputRef, Lexer, ParseContext};
  ///
  /// // Two handles of the same input carry distinct, unrelated `'closure` brands, so a
  /// // checkpoint saved on `a` cannot be restored into `b`.
  /// fn foreign_restore<'inp, L, Ctx>(
  ///   a: &mut InputRef<'inp, '_, L, Ctx>,
  ///   b: &mut InputRef<'inp, '_, L, Ctx>,
  /// ) where
  ///   L: Lexer<'inp>,
  ///   L::State: Clone,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   let ckp = a.save();
  ///   b.restore(ckp); // error: the two handles' `'closure` brands cannot unify
  /// }
  /// ```
  ///
  /// # Contract: restores are last-in, first-out
  ///
  /// Restoring this checkpoint **invalidates every checkpoint saved after it**.
  /// Equivalently: with several live checkpoints, always restore the youngest one you
  /// intend to return to; never restore a checkpoint after restoring one older than
  /// it.
  ///
  /// Both of these are fine:
  ///
  /// ```ignore
  /// // Nested speculation — inner ended before outer (each ends in commit or restore):
  /// let outer = input.save();
  /// let inner = input.save();
  /// if try_variant_a(input) { input.commit(inner) } else { input.restore(inner) } // youngest first
  /// if try_variant_b(input) { input.commit(outer) } else { input.restore(outer) } // then the older
  ///
  /// // Retry loop — a fresh checkpoint per iteration:
  /// loop {
  ///   let ckp = input.save();
  ///   match try_parse(input) {
  ///     Ok(v) => { input.commit(ckp); break v }          // success: keep progress, release the id
  ///     Err(_) => input.restore(ckp),                    // failure: the youngest live one
  ///   }
  /// }
  /// ```
  ///
  /// This is a contract violation:
  ///
  /// ```ignore
  /// let a = input.save();
  /// let b = input.save();   // b is younger than a
  /// input.restore(a);       // rolls history back past b's save point:
  ///                         // b now refers to a lineage that no longer exists
  /// input.restore(b);       // ✗ contract violation
  /// ```
  ///
  /// The reason is structural, not stylistic: restoring `a` truncated the emission
  /// log below `b`'s mark and un-lexed the tokens `b`'s position depends on. A
  /// truncated log cannot be rebuilt, so there is *no correct state* the second
  /// restore could produce.
  ///
  /// # Debug builds
  ///
  /// Debug builds track live checkpoints exactly and **panic** on any out-of-order
  /// restore (message begins `non-LIFO checkpoint restore`). `cargo test` compiles with
  /// debug assertions by default, so exercising your parser's backtracking paths in
  /// tests surfaces violations immediately.
  ///
  /// A debug assert *also* re-checks that the checkpoint belongs to this input — a backstop for
  /// the one construction the `'closure` brand cannot catch: two inputs borrowed in a single scope,
  /// where the compiler is free to unify their brands. Through the public closure API the brand
  /// already makes every foreign restore a compile error, so this assert is defense in depth; it is
  /// compiled out entirely in release, where it costs nothing.
  ///
  /// # Release builds
  ///
  /// Release builds do not check. An out-of-order restore leaves the input in an
  /// **unspecified but bounded** state. Even then, all of the following still hold:
  /// no undefined behavior, no leak, no panic originating in the **input layer**, every scan
  /// terminates (the resource-limiter state travels inside the checkpoint, so a
  /// re-reached limit re-trips instead of rescanning without bound), and the input
  /// remains usable.
  ///
  /// What is **not** guaranteed after a violation: diagnostics may be missing or
  /// attributed to the wrong branch, and the replayed token stream may differ from
  /// what was visible at the save. The only well-specified use of a checkpoint is
  /// restoring it while it is still valid.
  ///
  /// **The attached emitter keeps its own posture, and one of them is louder than this.** The
  /// no-panic clause above is the input layer speaking for itself; a violation still reaches
  /// [`Emitter::rewind`], and an emitter that can *detect* the resulting unpaired settle is
  /// permitted to report it (see that method's mid-unwind contract). The recording CST
  /// `Sink` (the `rowan` feature) does: a stale restore that lands on a mid-log mark whose row
  /// was already spent panics there in **every** build, release included, rather than shear
  /// the event log from the diagnostic log. Every built-in diagnostics emitter (`Verbose`,
  /// `Fatal`, `Silent`) has nothing to detect and stays silent as before.
  ///
  /// **A restore the emitter refuses is not rolled back either.** The emitter's own state is
  /// left exactly as it was — the `Sink` decides before it mutates — but this method is not
  /// transactional across that panic: it raises from the middle of the rollback, so the
  /// lineage has already been popped through the target while the position and the
  /// error-reporting witnesses have not been restored. That is inside the
  /// "unspecified but bounded" envelope above and is the cost of being told at all; the input
  /// stays usable, and the only way to reach it is the violation being reported.
  #[cfg(feature = "unstable-raw")]
  #[cfg_attr(docsrs, doc(cfg(feature = "unstable-raw")))]
  #[doc(alias = "rewinds")]
  #[inline(always)]
  pub fn restore(&mut self, checkpoint: Checkpoint<'inp, 'closure, L>) {
    self.restore_checked(checkpoint)
  }

  /// The crate-internal raw `restore`, used when the `unstable-raw` valve is off. Same body as
  /// the public flavor; only its visibility differs. The transaction guards, the
  /// [session points](Self::begin_point), and
  /// [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt) rewind through it.
  #[cfg(not(feature = "unstable-raw"))]
  #[inline(always)]
  pub(crate) fn restore(&mut self, checkpoint: Checkpoint<'inp, 'closure, L>) {
    self.restore_checked(checkpoint)
  }

  /// Shared body of the [`restore`](Self::restore) twins: verifies the last-in, first-out and
  /// foreign-input discipline (debug + ptr builds) and refuses a restore that would tear out a
  /// pinned guard/attempt begin point (every allocator build), then rewinds.
  #[inline(always)]
  fn restore_checked(&mut self, checkpoint: Checkpoint<'inp, '_, L>) {
    // Verify the discipline exactly, before any mutation. Two debug + ptr checks: (1) the
    // checkpoint belongs to this input. The invariant `'closure` brand on this method's signature
    // makes a foreign restore a COMPILE error for any two handles with distinct brands — which is
    // every pair a downstream parser can hold, since each arrives through a `for<'closure>` closure
    // (`apply`) and their brands never unify. This assert still backstops the one construction the
    // brand cannot separate: two `Input`s borrowed in a single scope (crate-internal `as_ref`),
    // whose `'closure` regions the compiler is free to unify — see the `..._rejected_in_debug`
    // tests. (2) it is still live (restoring an older checkpoint invalidates every one saved after
    // it) — the LIFO witness the type system does NOT replace. Release and no-ptr builds omit both
    // panics; the lineage stack itself is still maintained in every allocator build inside
    // `restore_unchecked`.
    #[cfg(all(
      debug_assertions,
      any(feature = "std", feature = "alloc"),
      target_has_atomic = "ptr"
    ))]
    {
      assert!(
        checkpoint.input_id == self.witness.input_id(),
        "checkpoint restored into a foreign input: this checkpoint was created by a different input"
      );
      assert!(
        self.live_contains(checkpoint.ckp_id),
        "non-LIFO checkpoint restore: this checkpoint was invalidated by restoring an older one (restores must be last-in, first-out)"
      );
    }
    // Detect-at-cause, in EVERY allocator build (unlike the debug-only misuse panics above):
    // refuse a restore that would tear the begin point out from under a live transaction guard
    // or attempt — a raw restore below its pinned base. A guard's own settle unpins its held id
    // before reaching here, so this never trips a guard rolling back to its own base.
    #[cfg(any(feature = "std", feature = "alloc"))]
    self.assert_restore_preserves_pins(checkpoint.ckp_id);

    self.restore_unchecked(checkpoint);
  }

  /// Commits `checkpoint`: keeps every bit of progress made since its save and releases the
  /// checkpoint's lineage entry. This is the success-path counterpart to
  /// [`restore`](Self::restore) — the verb for a speculative branch that *worked out*.
  ///
  /// # Unstable: feature-gated raw API
  ///
  /// `commit` is part of the raw checkpoint triple ([`save`](Self::save) /
  /// [`restore`](Self::restore) / `commit`) and is public **only** under the `unstable-raw`
  /// feature; without it the method is crate-internal. The supported backtracking surface is the
  /// transaction guards ([`begin`](Self::begin) / [`begin_stacked`](Self::begin_stacked)), the
  /// [session points](Self::begin_point), and
  /// [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt); the lineage contract below
  /// applies to the raw triple unchanged whenever the feature is on.
  ///
  /// Like [`restore`](Self::restore), `commit` accepts only a checkpoint carrying *this* handle's
  /// own invariant `'closure` brand; committing one a different handle saved is a compile error.
  ///
  /// # Contract: end each checkpoint in exactly one of restore or commit
  ///
  /// A saved [`Checkpoint`] should end its life in exactly one of two ways: hand it to
  /// [`restore`](Self::restore) to abandon the branch and rewind, or hand it to `commit` to
  /// keep the branch's progress. A checkpoint that is merely **dropped** keeps the progress
  /// too — dropping rewinds nothing — but in allocator builds its id lingers on the input's
  /// live-checkpoint lineage stack until an older [`restore`](Self::restore) happens to pop
  /// through it. Repeated successful speculation that drops rather than commits therefore grows
  /// that stack for the life of the input; `commit` is what keeps it bounded. (The stranded ids
  /// are inert lineage bookkeeping, not unsafety: every restore still replays its lineage
  /// exactly.)
  ///
  /// A retry loop keeps its progress by committing the youngest live checkpoint on success:
  ///
  /// ```ignore
  /// loop {
  ///   let ckp = input.save();
  ///   match try_parse(input) {
  ///     Ok(v) => { input.commit(ckp); break v }   // success: keep progress, release the id
  ///     Err(_) => input.restore(ckp),             // failure: rewind to the save
  ///   }
  /// }
  /// ```
  ///
  /// Releasing is `O(1)` when `checkpoint` is the youngest live checkpoint — the common
  /// retry-loop case — and a linear removal otherwise (e.g. a younger raw checkpoint was dropped
  /// above it); the rest of the stack keeps its order either way, so an older restore still pops
  /// cleanly through the gap. Committing an already-invalidated checkpoint — one an older
  /// [`restore`](Self::restore) already popped off the lineage — is a harmless **no-op**: its id
  /// is simply absent, so nothing is released and no state changes (no panic, in any build).
  ///
  /// Allocator-less builds keep no lineage stack, so `commit` there merely drops the checkpoint;
  /// the growth it prevents cannot arise without a stack to grow.
  #[cfg(feature = "unstable-raw")]
  #[cfg_attr(docsrs, doc(cfg(feature = "unstable-raw")))]
  #[inline(always)]
  pub fn commit(&mut self, checkpoint: Checkpoint<'inp, 'closure, L>) {
    self.commit_checkpoint(checkpoint)
  }

  /// The crate-internal raw `commit`, used when the `unstable-raw` valve is off. Same body as the
  /// public flavor; only its visibility differs. Its sole in-crate caller is the allocator-gated
  /// [`commit_point`](Self::commit_point) (the guards release their kept
  /// begin points through unpin/forget directly), so in an allocator-less valve-off build it is
  /// deliberately uncalled — kept defined so the raw triple stays whole in every configuration.
  #[cfg(not(feature = "unstable-raw"))]
  #[cfg_attr(not(any(feature = "std", feature = "alloc")), allow(dead_code))]
  #[inline(always)]
  pub(crate) fn commit(&mut self, checkpoint: Checkpoint<'inp, 'closure, L>) {
    self.commit_checkpoint(checkpoint)
  }

  /// Shared body of the [`commit`](Self::commit) twins. Reachable only through them, so in the
  /// one configuration where both are uncalled (valve off, no allocator — see the twin above) it
  /// shares their deliberate-dead-code allowance.
  #[cfg_attr(
    all(
      not(feature = "unstable-raw"),
      not(any(feature = "std", feature = "alloc"))
    ),
    allow(dead_code)
  )]
  #[inline(always)]
  fn commit_checkpoint(&mut self, checkpoint: Checkpoint<'inp, '_, L>) {
    // Cheap sanity in debug + ptr builds, mirroring `restore`'s foreign-input guard: a
    // checkpoint may only be committed into the input that created it. The invariant `'closure`
    // brand on `commit` makes a foreign commit a compile error for handles with distinct brands
    // (every pair a downstream parser can hold); this assert backstops the crate-internal case the
    // brand cannot separate (two `Input`s in one scope). Presence is NOT asserted: committing a
    // dead checkpoint is the documented no-op handled below.
    #[cfg(all(
      debug_assertions,
      any(feature = "std", feature = "alloc"),
      target_has_atomic = "ptr"
    ))]
    assert!(
      checkpoint.input_id == self.witness.input_id(),
      "checkpoint committed into a foreign input: this checkpoint was created by a different input"
    );

    // Keep all progress; release ONLY the lineage entry (via the kept-checkpoint funnel, which
    // pairs it with the emitter-mark release), never the pin set. `forget_checkpoint` is `O(1)`
    // at the stack top and pops nothing for an already-invalidated id (the no-op case).
    //
    // No pin check is needed, and none could ever trip: a pinned id is the begin point of a live
    // transaction guard or `attempt`, which holds that begin-point `Checkpoint` internally and
    // never hands it out. A caller can only reach a checkpoint's id THROUGH a `Checkpoint` value,
    // and this method consumes one it was given — so the committed id is a raw, unpinned
    // checkpoint by construction. There is no reachable way to commit a guard's pinned base and
    // unpin-bypass it, and `forget_checkpoint` leaves `pinned` untouched regardless.
    self.forget_kept_checkpoint(checkpoint);
  }

  /// Rewinds to `checkpoint` without the debug raw-misuse panics, the shared primitive behind
  /// the checked [`restore`](Self::restore), the drop-path
  /// [`restore_unchecked_if_live`](Self::restore_unchecked_if_live), and the explicit reconciling
  /// [`rollback_abandoning_points`](Transaction::rollback_abandoning_points). A rolling-back
  /// `Drop` reaches it through the second and must stay silent: `Drop` may run while already
  /// unwinding, and `no_std` has no `thread::panicking()` to guard a drop-bomb, so a debug assert
  /// firing here would abort. It still maintains the lineage stack (popping through the restored
  /// id if present) and replays the saved lineage exactly, identically to
  /// [`restore`](Self::restore) in release. Its own base is usually the oldest live checkpoint,
  /// but a raw restore below it through the guard can invalidate it first — which is why the drop
  /// path consults liveness before calling in (skipping a dead base), while both explicit
  /// rollbacks report that stale case instead, since neither ever runs during an unwind:
  /// [`rollback`](Transaction::rollback) through the checked [`restore`](Self::restore)'s own
  /// assert, and the reconciling verb through one it raises before routing here.
  /// Settles every [session point](Self::begin_point) younger than `target_id` — the suffix a
  /// rewind to that checkpoint invalidates — the whole body of
  /// [`restore_unchecked`](Self::restore_unchecked)'s reconciliation, deliberately **outlined**.
  ///
  /// Reached only by a rewind that reaches below an open point, which no correct driver does on a
  /// hot path, so `#[cold]` + `#[inline(never)]` leaves the caller a single `is_empty` branch —
  /// the same reason the session cell outlines its abandoning drop, and it matters here for the
  /// same reason: `restore_unchecked` is `inline(always)` and sits on every rollback path.
  ///
  /// Newest-first, so [`Lineage::unpin`](super::Lineage::unpin) and the funnel's `forget` each
  /// take their `O(1)` stack-top path and each emitter mark is released newest-first. Keyed on
  /// the checkpoint id because it is the monotone order of the live-checkpoint stack: a point
  /// whose id is above the target's is exactly one the pop-through below invalidates.
  ///
  /// Silent because most of its callers are forbidden to speak: the guards' rolling-back `Drop`
  /// and the unwind paths under [`attempt`](Self::attempt)/[`try_attempt`](Self::try_attempt) may
  /// run while already unwinding. The one caller that is *not* mid-unwind — the explicit
  /// [`rollback_abandoning_points`](Transaction::rollback_abandoning_points), which asks for this
  /// reconciliation deliberately — raises its own base-liveness assert before routing in, so
  /// nothing is lost by this body staying quiet. What never reaches here with a younger point
  /// live is the *checked* [`restore`](Self::restore): the point's pin refuses that at the cause
  /// first, loudly, which is the whole difference between the two rollback verbs.
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cold]
  #[inline(never)]
  fn abandon_points_above(&mut self, target_id: u64) {
    while self
      .session
      .points
      .last()
      .is_some_and(|point| point.ckp_id > target_id)
    {
      let point = self
        .session
        .points
        .pop()
        .expect("guarded by the loop condition");
      // The same unpin-then-settle a `commit_point` performs, and like it the popped
      // `Checkpoint` is dropped WITHOUT restoring: the rewind this reconciliation precedes is
      // what moves the position, all the way down to its own target.
      self.unpin_checkpoint(point.ckp_id);
      self.forget_kept_checkpoint(point);
    }
  }

  #[inline(always)]
  pub(crate) fn restore_unchecked(&mut self, checkpoint: Checkpoint<'inp, '_, L>) {
    // SETTLE_CENSUS — the rollback in two phases, because staging every dying value is not
    // available here and ordering is.
    //
    // Every finding in this body had one shape: some owned value died while the rollback was
    // half-done. The first repair staged the values the body DISPLACES (the parked entry, the
    // watermark, the latch, the replaced pair) into one drop site. The next showed there is a
    // second source it cannot reach — the values the body EVICTS: `reconcile_cache_geometry`
    // drops cache entries through `clear`/`pop_front`, the tail prune through `pop_back`, and
    // `abandon_points_above` drops whole `Checkpoint`s (each owning `L::Span` AND `L::State`)
    // through `forget_kept_checkpoint`. None of those are values this body holds, so no drop site
    // can collect them, and a staging buffer is not an option: checkpoints work in
    // allocator-less builds, where there is nowhere to put an unbounded number of evicted
    // entries.
    //
    // So the fix is the order, not the buffer. Everything that runs caller code happens in
    // PHASE 1, while the input is still wholly on the branch being abandoned — a panic there
    // leaves it there, which is precisely the "unchanged" half of all-or-nothing. PHASE 2 then
    // installs every fact with no caller code among them at all. The cache evictions are safe to
    // hoist because a cache is a pure memo: dropping any subset of it is unobservable, entries
    // simply re-lex. The point abandonment was already first for its own reasons.
    #[cfg(any(feature = "std", feature = "alloc"))]
    let ckp_id = checkpoint.ckp_id;
    let Checkpoint {
      cursor,
      span,
      state,
      emitter_checkpoint,
      emitted_error_end,
      front_reported_end,
      poison_boundary,
      cache_pushes,
      ..
    } = checkpoint;

    // ── PHASE 1 — caller code runs here, and only here. Nothing is restored yet. ──

    // Clamping is the fallible half of the position write (`Source::len`, and an `Offset::clone`
    // on the clamping branch). Done up front, its failure costs nothing; done at the install, it
    // would leave the emitter and the facts rewound past a position that never moved.
    let (clamped, spare) = self.clamped_span(span.into());

    // Session points above the target: each pop hands a whole `Checkpoint` to
    // `forget_kept_checkpoint` BY VALUE, so its `L::Span` and `L::State` drop inside that call.
    #[cfg(any(feature = "std", feature = "alloc"))]
    {
      if !self.session.points.is_empty() {
        self.abandon_points_above(ckp_id);
      }
    }

    // Taken, not dropped: the parked slot is a value this body owns, so it can be staged.
    let displaced_park = self.pending.take();

    // Cache geometry and the post-save prune. Both discard owned `CachedToken`s — `clear`,
    // `pop_front` and a `pop_back()` whose result is thrown away are each a `Drop` call written
    // as a statement. Hoisted here, they run against the abandoned branch's own cache.
    reconcile_cache_geometry::<L, Ctx::Cache, Lang>(self.cache_mut(), cursor.as_inner());
    let post_save = self
      .session
      .lineage
      .cache_pushes()
      .saturating_sub(cache_pushes);
    let survivors = (self.cache.len() as u64).min(post_save);
    for _ in 0..survivors {
      self.cache.pop_back();
    }

    // ── CENSUS_PHASE_BOUNDARY — the restore. No caller code below this line except
    // `Emitter::rewind`.
    //
    // That admission used to rest on the trait contracting `rewind` as non-panicking here. It
    // no longer does: an emitter that can DETECT an unpaired settle may report it when no
    // unwind is in flight (see `Emitter::rewind`'s mid-unwind contract), and the recording CST
    // `Sink` takes that door. So the exception is now a known, measured hole rather than a
    // proof — a panic from the rewind below leaves the lineage popped through the target id
    // while the position, the three facts and the cache-push counter are never installed. That
    // is a torn restore, and reordering this phase cannot close it: phase 1 has already
    // abandoned the session points above the target and cleared the parked slot, neither of
    // which is recoverable either. It is reachable only through the same double-settle bug the
    // emitter is reporting, and only after that bug has already made the parse wrong.
    // `restore_unchecked_is_not_transactional_across_the_settle_wall` in `cst/sink/tests.rs`
    // pins both halves so the hole stays measured rather than assumed away. ──
    #[cfg(any(feature = "std", feature = "alloc"))]
    self.live_pop_through(ckp_id);
    self.emitter().rewind(&cursor, emitter_checkpoint);
    self.session.lineage.restore_cache_pushes(cache_pushes);
    let displaced_facts = (
      core::mem::replace(self.emitted_error_end, emitted_error_end),
      // Restored in the same no-caller-code block as the emitter rewind above: the watermark and
      // the log it describes move as one state. A rollback below the flag truncates the report and
      // disarms the watermark together; one above it keeps both, because the report predates the
      // mark. This is the pairing that makes the witness transactional.
      core::mem::replace(self.front_reported_end, front_reported_end),
      core::mem::replace(self.poison_boundary, poison_boundary),
    );
    let replaced = self.install_position(clamped, spare, state);

    // ── PHASE 3 — the one drop site, with the rollback whole. ──
    drop((displaced_park, displaced_facts, replaced));
  }

  /// Drop-path rewind that never resurrects a dead base. Used by the transaction guards'
  /// rolling-back [`Drop`], whose held begin-point checkpoint a raw restore below it (through
  /// the guard's `DerefMut`) may have popped off the live lineage.
  ///
  /// If the checkpoint is still live it rewinds exactly as
  /// [`restore_unchecked`](Self::restore_unchecked). If an earlier restore already invalidated
  /// it, the input already sits where that older restore left it, so this skips the rewind
  /// rather than copying the stale saved state back over it. It never panics: a `Drop` may run
  /// while already unwinding, so it must stay silent.
  ///
  /// # Now a backstop
  ///
  /// The guards pin their begin point, so in allocator builds a raw restore that would pop it
  /// off the lineage panics **at the restore** ([`restore`](Self::restore)'s pin check) — the
  /// base can no longer go stale while its guard is live, so the skip branch here is
  /// unreachable and this always rewinds. The skip is retained as **defense in depth** and for
  /// allocator-less builds, which keep no pin set and no lineage stack: there the rewind always
  /// proceeds regardless, unspecified-but-bounded on misuse as documented on the guards. Reads
  /// the lineage stack without popping — the pop-through happens only inside the rewind it
  /// forwards to.
  #[inline(always)]
  pub(crate) fn restore_unchecked_if_live(&mut self, checkpoint: Checkpoint<'inp, '_, L>) {
    #[cfg(any(feature = "std", feature = "alloc"))]
    {
      if !self.live_contains(checkpoint.ckp_id) {
        return;
      }
    }
    self.restore_unchecked(checkpoint);
  }

  /// Advances the cursor and returns the next valid token, emitting errors encountered on the way.
  ///
  /// Skips over lexer errors, emitting them through the provided emitter.
  /// Non-fatal errors are emitted and the method continues to the next token.
  ///
  /// # Partial-input frontier (`Partial`, non-final)
  ///
  /// On a [`Partial`](crate::input::Partial) input that is not yet final
  /// ([`is_final`](Self::is_final) `== false`), three conservative rules keep a construct that
  /// later input could still extend from being mistaken for a finished one — each surfaces an
  /// [`Incomplete`](crate::error::Incomplete) on the `Err` channel instead:
  ///
  /// 1. **Frontier holdback** — a token whose span **end touches the buffer end** is not yielded;
  ///    it may be a prefix of a longer token once more input arrives.
  /// 2. **Frontier error** — a **non-terminal** lexer error whose span **touches the buffer end** is
  ///    not emitted; it may be a truncation artifact.
  /// 3. **Non-final EOF** — lexer exhaustion is not treated as genuine end of input; more may come.
  ///
  /// # A terminal trip outranks all three
  ///
  /// Every rule above says *"more input may change this"* — so none of them may apply to a condition
  /// no input can change. A limit trip (and the poison boundary it latches) is exactly that: it
  /// emits its diagnostic and yields `Ok(None)` **even when the tripping token ends on the buffer
  /// end**, because a limiter's tally is monotone and no refill can un-trip it. Terminal beats
  /// incomplete, always — see the [law](crate::input#terminal-beats-incomplete-and-they-never-substitute),
  /// the dual of the crate's
  /// [never-recoverable law](crate::error::Incomplete#the-never-recoverable-law).
  ///
  /// With [`is_final`](Self::is_final) `== true`, or on a
  /// [`Complete`](crate::input::Complete) input, all three rules are off and `next` behaves
  /// identically to before this typestate existed (the checks are eliminated at monomorphization).
  /// The frontier holdback means the last token only becomes visible after more input arrives or
  /// the input is marked final — a **one-token latency** that is correct by construction. See the
  /// [`input`](crate::input) module docs for the Sans-I/O resumption loop.
  #[allow(clippy::should_implement_trait)]
  pub fn next(
    &mut self,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  {
    if let Some(cached_token) = self.take_front() {
      let (spanned_lexed, extras) = cached_token.into_components();
      let (span, lexed) = spanned_lexed.into_components();
      self.commit_token(&lexed, &span, extras);
      return Ok(Some(Spanned::new(span, lexed)));
    }

    // A sticky limit trip latches a poison boundary: once the cache is drained and
    // the cursor has reached the durable frontier, stop without rebuilding a lexer
    // or rescanning the tripping token. Strictly before it, `next()` re-lexes (e.g.
    // to replay a drained prefix after a restore).
    if self.reached_boundary(self.offset()) {
      return Ok(None);
    }

    // `next()` commits no progress before a poisoned or exhausted outcome, so it
    // latches at the cursor and yields `None` on both a trip and end of input.
    let mut resume = self.resume();
    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => {
        self.commit_token(tok.data(), tok.span_ref(), resume.into_lexer().into_state());
        Ok(Some(tok))
      }
      Scan::Tripped | Scan::Eof => Ok(None),
    }
  }

  /// Consumes the next valid token like [`next`](Self::next), except that a **terminal stop is an
  /// error, never a silent end of input** — the committed-consume sibling of
  /// [`try_expect_or_stop`](Self::try_expect_or_stop).
  ///
  /// [`next`](Self::next) folds three distinct outcomes into `Ok(None)`: a genuine end of input, a
  /// fresh resource-limit trip (a `Scan::Tripped`), and an already-latched poison boundary at the
  /// cursor. For a **committed** leaf — one that turns `next() == None` into an
  /// [`UnexpectedEot`](crate::error::UnexpectedEot) — that fold is a false negative: a fresh trip
  /// becomes a plain, *recoverable* end-of-input error, so [`Recover`](crate::parser::Recover)
  /// synthesizes a value and re-enters the scanner, re-tripping the same limit instead of re-raising
  /// it. No input ever clears a tripped limit, so a terminal stop must be re-raised untouched exactly
  /// as an [`Incomplete`](crate::error::Incomplete) is — see the [never-recoverable
  /// dual](crate::error::MaybeTerminal).
  ///
  /// So this draws the same split the attempt/decline primitives draw:
  ///
  /// - `Ok(Some(tok))` — a real consumed token;
  /// - `Ok(None)` — a **genuine** end of input; the caller builds its plain `UnexpectedEot`, exactly
  ///   as it did off `next() == None`;
  /// - `Err(..)` — a terminal stop (a fresh trip, or the poison boundary it latches), surfaced as the
  ///   committed form's end-of-input error already marked terminal via
  ///   [`into_terminal`](crate::error::UnexpectedEnd::into_terminal), so recovery re-raises it. A
  ///   fatal emitter's rejection of the trip diagnostic still propagates from the scan itself —
  ///   but as *that emitter's* value, converted from the lexer error, so it carries **no** terminal
  ///   mark: no `UnexpectedEnd` is built on that path for `into_terminal` to raise a flag on. The
  ///   arm of your error type holding a lexer error is what answers for it; see
  ///   [`MaybeTerminal`](crate::error::MaybeTerminal#where-the-set-stops-being-closed).
  ///
  /// # Zero-cost on the success path
  ///
  /// The terminal classification lives only on the cold exhaustion arms — the pre-latched-boundary
  /// short-circuit and the `Tripped`/`Eof` outcomes of the one scan. A cache hit and a
  /// `Scan::Token` return the token with no terminal work, so this is [`next`](Self::next) plus a
  /// single boundary compare on the end-of-input arm. The terminal signal rides inside the
  /// `UnexpectedEot` value, so no [`MaybeTerminal`](crate::error::MaybeTerminal) bound reaches the
  /// caller — the same boundary-witness discipline the resilient collection loops gate on.
  #[inline]
  pub fn next_or_stop(
    &mut self,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    if let Some(cached_token) = self.take_front() {
      // A retained token is a REAL token (nothing at the front is ever an error): never terminal —
      // the identical fast path `next` takes.
      let (spanned_lexed, extras) = cached_token.into_components();
      let (span, lexed) = spanned_lexed.into_components();
      self.commit_token(&lexed, &span, extras);
      return Ok(Some(Spanned::new(span, lexed)));
    }

    // A sticky limit trip latches a poison boundary at the cursor: a terminal stop, not genuine end
    // of input, so surface the terminal-marked end-of-input error where `next` returns a plain
    // `None`. Mirrors `try_expect_or_stop`'s E4.
    if self.reached_boundary(self.offset()) {
      return Err(
        UnexpectedEot::eot_of(self.span().end())
          .into_terminal()
          .into(),
      );
    }

    // `next` commits no progress before a poisoned or exhausted outcome, so it latches at the cursor
    // (`AtCursor`); the exhaustion is classified on the cold arms below.
    let mut resume = self.resume();
    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => {
        self.commit_token(tok.data(), tok.span_ref(), resume.into_lexer().into_state());
        Ok(Some(tok))
      }
      // A fresh trip whose diagnostic a recovering emitter accepted (a fatal emitter's rejection
      // already propagated out of `scan_with`), marked terminal so recovery re-raises it. Mirrors
      // `try_expect_or_stop`'s E3.
      Scan::Tripped => Err(
        UnexpectedEot::eot_of(self.span().end())
          .into_terminal()
          .into(),
      ),
      Scan::Eof => {
        // An exhaustion produced by refusing to cross a pre-latched boundary is terminal, not genuine
        // end of input (under `Partial` non-final, `scan_with` already surfaced `Incomplete` for every
        // non-boundary exhaustion, so `Eof` implies boundary there — this makes it explicit in
        // `Complete` mode too). One cold compare. Mirrors `try_expect_or_stop`'s E5.
        if self.reached_boundary(resume.at()) {
          Err(
            UnexpectedEot::eot_of(self.span().end())
              .into_terminal()
              .into(),
          )
        } else {
          Ok(None) // genuine end of input — the documented plain-EOT case.
        }
      }
    }
  }

  /// Asks the partial-input frontier holdback (rules 1 and 2) about one lexed item: in
  /// [`Partial`](crate::input::Partial) non-final mode, an item whose span END touches the buffer
  /// end may be a prefix of a longer construct once more input arrives, so it is neither yielded nor
  /// emitted.
  ///
  /// **It may only ever be asked about a NON-TERMINAL item.** The holdback's whole premise is that
  /// more input could change the answer; a terminal condition is precisely the one that no input can
  /// change, so it is ranked first and never reaches here. [`classify`](Self::classify) is the only
  /// caller, and it asks in that order — see its docs for the law.
  ///
  /// Const-gated: on a [`Complete`](crate::input::Complete) input `Cmpl::PARTIAL` is a `false`
  /// constant, so this is dead-code-eliminated and `is_final()` is never even evaluated.
  #[inline(always)]
  fn withhold_at_frontier(&self, span: &L::Span) -> bool {
    Cmpl::PARTIAL && !self.is_final() && span.end_ref() >= &self.input.len()
  }

  /// The fatal-emit exit every lexing driver shares: the emitter **rejected** a lexer error's
  /// diagnostic, so settle the input at the lexer — the rejected item's span, and the state that
  /// produced it — and hand the error back to be propagated.
  ///
  /// A trip's poison boundary is already latched by the time this can run
  /// ([`classify`](Self::classify) latches before the verdict is even returned), so a fatal exit
  /// records the trip for every later operation instead of losing it with the unwind.
  #[inline(always)]
  fn settle_fatal(
    &mut self,
    lexer: &L,
    e: <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  ) -> <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error {
    // The scanner's unwind edge leans on one invariant: the committed position moves inside the
    // loop only here, and here the token store is provably empty — this arm is reachable only
    // from the lexing branch, which runs only with the parked slot and the cache both drained.
    // A store-empty commit leaves the unwind settle nothing to reconcile. Guarded at the site
    // that could break it rather than asserted where it is read.
    debug_assert!(
      self.cache().is_empty() && self.pending.is_none(),
      "a mid-loop fatal commit requires an empty token store",
    );
    // Both halves computed before either is written, as everywhere else on this surface.
    let span = lexer.span();
    let state = lexer.state().clone();
    self.commit_position(span.into(), state);
    e
  }

  /// Ranks one freshly-lexed item into the [`Verdict`] its driver must act on — **the** choke point
  /// where a terminal condition meets the partial-input frontier, and the single place their
  /// precedence is decided.
  ///
  /// # The law: a terminal trip outranks an incomplete frontier
  ///
  /// Two verdicts stop a scan, and they mean **opposite** things:
  ///
  /// - [`Incomplete`](crate::error::Incomplete) means *"more input may fix this"* — refill and
  ///   retry;
  /// - a **terminal** condition (a limit trip; the poison boundary it latches) means *"no amount of
  ///   input will fix this"* — stop.
  ///
  /// They are mutually exclusive, and **terminal wins**. So the limit is probed — and latched —
  /// **before** the frontier holdback is even consulted, and only an item that is *not* terminal can
  /// be [`Withheld`](Verdict::Withheld). Ordering them the other way is not a cosmetic bug: the
  /// Logos backend reports a limit trip as a `Lexed::Error` carrying the *tripping token's* span, so
  /// a holdback that ran first would swallow every trip whose token happened to end on a chunk
  /// boundary — emitting no diagnostic, latching nothing, and telling a streaming caller to feed
  /// more bytes to a limit that had **already** been exceeded. An attacker who aligns a payload to
  /// that boundary would bypass the recursion/token limit outright.
  ///
  /// The asymmetry is not arbitrary, and it is what makes the ranking total. A frontier *item* is
  /// **provisional**: whether those bytes are a token or an error depends on bytes that have not
  /// arrived, so withholding it is the conservative answer. A limit trip is not about the item at
  /// all — it is a fact about the lexer's accumulated tally, which is **monotone**: re-lexing the
  /// same prefix re-trips, and appending bytes can only add to it. No refill can clear it, so
  /// reporting it as "incomplete" would be reporting a falsehood.
  ///
  /// This is the **dual** of the crate's [never-recoverable
  /// law](crate::error::Incomplete#the-never-recoverable-law) — recovery may not swallow an
  /// `Incomplete` — and the two halves are one rule: *an* `Incomplete` *and a terminal condition
  /// never substitute for each other, in either direction.*
  ///
  /// # Both lexing drivers rank here
  ///
  /// [`scan_with`](Self::scan_with) (every consume path) and the peek fill
  /// (`peek_with_emitter_inner`) are the crate's only two drivers of the single lexing site
  /// ([`lex_within_boundary`](Self::lex_within_boundary)), and both classify through this one
  /// method. The precedence therefore has exactly one home: a driver cannot re-derive it, and a
  /// third driver cannot get it wrong. `frontier` chooses where a trip latches — [`AtCursor`] for
  /// scans that commit no progress first (`next`, `try_expect*`, and the peek fill, which commits
  /// nothing and latches at the end of the newest RETAINED token), [`AtFrontier`] for scans that
  /// consume tokens as they go.
  ///
  /// The complete path is untouched: [`Verdict::Withheld`] is built only under `Cmpl::PARTIAL`, a
  /// `false` constant for [`Complete`](crate::input::Complete), so the holdback — and the whole
  /// incomplete arm of the ranking — is eliminated at monomorphization, leaving the terminal probe
  /// exactly where it has always been.
  #[inline(always)]
  fn classify<Fr>(
    &mut self,
    lexer: &L,
    frontier: &Fr,
    item: Spanned<Lexed<'inp, L::Token>, L::Span>,
  ) -> Verdict<'inp, L>
  where
    Fr: Frontier<'inp, L>,
  {
    let (span, lexed) = item.into_components();
    match lexed {
      Lexed::Error(err) => {
        // TERMINAL FIRST. The probe (and its latch) run before the frontier is consulted, so a trip
        // whose tripping token ends exactly on a non-final buffer end is reported as a trip — not
        // withheld as Incomplete. A plain lexer error leaves `check()` `Ok` and latches nothing, so
        // this costs the non-terminal path only the probe it already paid for.
        let boundary = frontier.boundary(self.offset());
        if self.latch_if_limit_tripped(lexer, boundary) {
          return Verdict::Trip(Spanned::new(span, err));
        }
        // Frontier error (rule 2), now asked only of a NON-terminal error: a truncated buffer really
        // can make a valid token look like a lex error, so this one is withheld — un-emitted — and
        // the caller refills. That rule is correct and survives the ranking intact.
        if self.withhold_at_frontier(&span) {
          return Verdict::Withheld(span.end_ref().clone());
        }
        Verdict::Error(Spanned::new(span, err))
      }
      // Frontier holdback (rule 1). A token is never terminal — the backend reports a trip as a
      // `Lexed::Error` on the tripping token (`check()` runs after each token; a failure *replaces*
      // it), so there is no terminal condition to outrank here and no `check()` on the token path.
      Lexed::Token(tok) => {
        if self.withhold_at_frontier(&span) {
          return Verdict::Withheld(span.end_ref().clone());
        }
        Verdict::Token(Spanned::new(span, tok))
      }
    }
  }

  /// Runs the shared scanner loop: lex within the poison boundary and handle
  /// every lexer error in one place — latch the durable frontier on a limit
  /// trip, deduplicate-and-emit the diagnostic, and take the identical fatal
  /// exit when the emitter rejects it.
  ///
  /// Returns to the caller only on an event it must decide: a valid
  /// [`Scan::Token`] (the caller applies its per-path policy and either commits
  /// or keeps scanning), a [`Scan::Tripped`] limit trip (already latched and
  /// emitted), or [`Scan::Eof`]. `frontier` chooses where a trip latches —
  /// [`AtCursor`] for scans that commit no progress first, [`AtFrontier`] for
  /// scans that consume tokens as they go — and advances over each error the
  /// loop skips on the way to the next event.
  ///
  /// # The partial-input frontier rules live here
  ///
  /// This is one of the two drivers of the single lexing site
  /// ([`lex_within_boundary`](Self::lex_within_boundary)) — and the only one every *consume* path
  /// goes through (`next`, `try_expect*`, `skip_while`, the `sync` family) — so the partial-input
  /// frontier rules are applied here once rather than scattered across them. In
  /// [`Partial`](crate::input::Partial) non-final mode they surface an
  /// [`Incomplete`](crate::error::Incomplete) on the `Err` channel, which every `scan_with(..)?`
  /// caller propagates unchanged:
  ///
  /// - **frontier holdback / frontier error** — a lexed item (token *or* error) whose span end
  ///   touches the buffer end is withheld, since more input could extend it — *unless it is
  ///   terminal*, which [`classify`](Self::classify) ranks first (a limit trip fires here even at
  ///   the frontier);
  /// - **non-final EOF** — lexer exhaustion that is *not* a poison-boundary trip surfaces
  ///   Incomplete, since more input may still arrive. A trip is exempt for the same reason it
  ///   outranks the holdback: it is terminal, and re-lexing the same prefix re-trips.
  ///
  /// All of it is written `if Cmpl::PARTIAL && …`; on a [`Complete`](crate::input::Complete)
  /// input `Cmpl::PARTIAL` is a `false` constant, so the whole block is eliminated at
  /// monomorphization and this compiles to the pre-typestate scanner byte for byte.
  #[inline]
  fn scan_with<Fr>(
    &mut self,
    parts: ResumeParts<'_, L, L::Offset>,
    frontier: &Fr,
  ) -> Result<Scan<'inp, L>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Fr: Frontier<'inp, L>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  {
    // The lex position runs the loop as a BY-VALUE local: the per-token `Lexer::lex` call is
    // opaque and reached through a pointer into the `Resume` allocation, so a position living in
    // that same allocation must be spilled and kept coherent around every call. A separate local
    // is provably unaliased by the call and stays in a register; the slot is written back once, at
    // the block's single exit below.
    let ResumeParts { lexer, at: at_slot } = parts;
    let mut lex_at = at_slot.clone();
    let result = 'scan: {
      while let Some(item) = self.lex_within_boundary(lexer, &mut lex_at) {
        match self.classify(lexer, frontier, item) {
          Verdict::Token(tok) => break 'scan Ok(Scan::Token(tok)),
          // A terminal trip: the poison boundary is already latched, so even the fatal exit below
          // keeps it. Emit the diagnostic and stop — this arm runs whether or not the tripping
          // token sits on the frontier, which is the whole of the law.
          Verdict::Trip(err) => {
            break 'scan match self.emit_lexer_error_deduped(err) {
              Ok(()) => Ok(Scan::Tripped),
              Err(e) => Err(self.settle_fatal(lexer, e)),
            };
          }
          Verdict::Error(err) => match self.emit_lexer_error_deduped(err) {
            Ok(()) => {
              // Non-limit error: skip over it and keep scanning for a token. The frontier does NOT
              // move — an error is not a token. `lex_at` already carries the scan past it, and the
              // token this loop goes on to find carries the post-error lexer state, so the
              // position is threaded by the same two things that thread it everywhere else.
              // Settling the error behind the frontier would put its span into `self.span` — which
              // every other path in this crate reserves for the last consumed TOKEN — and, worse,
              // would make that span depend on WHO crossed the error: a scan crosses it and would
              // move; a *peek* that lexed the same region into the cache never can. See
              // `AtFrontier`.
            }
            Err(e) => break 'scan Err(self.settle_fatal(lexer, e)),
          },
          // The holdback, reached only by a non-terminal item (see `classify`).
          Verdict::Withheld(at) => break 'scan Err(Cmpl::surface_incomplete(at)),
        }
      }

      // Non-final EOF (rule 3): the lexer is exhausted, but in partial non-final mode more input
      // may still arrive, so this is not genuine end of input — surface Incomplete. A
      // poison-boundary trip is exempt: it is a terminal limit outcome (re-lexing the same prefix
      // re-trips), so it stands as `Eof` — the same precedence `classify` applies to an item,
      // applied to exhaustion. Const-gated, so `Complete` never reaches this and yields `Eof` as
      // before.
      if Cmpl::PARTIAL && !self.is_final() && !self.reached_boundary(&lex_at) {
        // Report where the LEXER stopped, not where the last item it handed back ended. A driver
        // reads this offset to decide how much of its buffer was consumed, and the lexer's
        // position is that fact: the lex position only advances on an item, so every byte the
        // lexer SKIPS before exhaustion (trailing whitespace, a comment tail) would otherwise go
        // unreported — `"foo "` claiming 3 while the lexer stands at 4, `"   "` claiming 0 while
        // it stands at 3. `SyncTo::on_eof` already treats `lexer.span()` as the lexer's end for
        // exactly this reason (it commits there).
        //
        // The trait now SPECIFIES the post-exhaustion span — well-formed, ending at the lexer's
        // final position, within `[last item end, source len]` — and the conformance kit checks
        // it. The floor and the clamp stay as DEFENCE IN DEPTH against a non-conforming lexer,
        // not as the specification: the floor keeps one that reports a stale span from
        // *retracting* the frontier below the items it actually yielded, and the clamp keeps one
        // that over-reports from handing a refill driver an offset past its own buffer (the same
        // clamp `set_span` applies).
        let frontier = lexer.span().end().max(lex_at.clone()).min(self.input.len());
        break 'scan Err(Cmpl::surface_incomplete(frontier));
      }

      Ok(Scan::Eof)
    };
    // The one write-back: `Resume.at` observes the loop's final position on every exit path.
    *at_slot = lex_at;
    result
  }
}

#[inline(always)]
fn to_owned<T>(maybe: MaybeRef<'_, T>) -> T
where
  T: Clone,
{
  match maybe {
    MaybeRef::Ref(r) => r.clone(),
    MaybeRef::Owned(o) => o,
  }
}

/// The event the shared scanner loop ([`InputRef::scan_with`]) stops on.
enum Scan<'inp, L>
where
  L: Lexer<'inp>,
{
  /// A valid token; the caller applies its per-path policy (commit, put back,
  /// consume-and-report, …) and either stops or keeps scanning.
  Token(Spanned<L::Token, L::Span>),
  /// A limit trip: the durable frontier is already latched and the diagnostic
  /// emitted. The caller yields its poisoned outcome.
  Tripped,
  /// The input is exhausted (or the boundary was already reached). The caller
  /// yields its end-of-input outcome.
  Eof,
}

/// What one freshly-lexed item *means*, as ranked by [`InputRef::classify`] — the crate's single
/// classification of a scan outcome, and therefore the single home of the rule that a **terminal**
/// condition outranks an **incomplete** frontier.
///
/// The variants are ordered by that precedence, and the ordering is the contract: [`Trip`](Self::Trip)
/// is decided *before* [`Withheld`](Self::Withheld) is even considered, so no terminal condition can
/// be disguised as an [`Incomplete`](crate::error::Incomplete). Both lexing drivers — the scanner
/// ([`scan_with`](InputRef::scan_with), behind every consume path) and the peek fill — act on this
/// one verdict, so neither can re-derive the ranking and get it wrong. See
/// [`classify`](InputRef::classify) for the law and why the two verdicts are mutually exclusive.
enum Verdict<'inp, L>
where
  L: Lexer<'inp>,
{
  /// A valid token, clear of the frontier: the scanner yields it, the peek fill caches it.
  Token(Spanned<L::Token, L::Span>),
  /// **Terminal.** This item tripped a resource limit: the poison boundary is *already latched* at
  /// the durable frontier, so even a fatal emitter cannot lose it. The driver emits the diagnostic
  /// (deduplicated) and stops. Reached whether or not the tripping item touches the buffer end.
  Trip(Spanned<<L::Token as Token<'inp>>::Error, L::Span>),
  /// A **non-terminal** lexer error, clear of the frontier: the driver emits it (deduplicated) and
  /// skips over it. Nothing is latched — the scan goes on looking for a token.
  Error(Spanned<<L::Token as Token<'inp>>::Error, L::Span>),
  /// The partial-input frontier holdback: a **non-terminal** item whose span end touches a non-final
  /// buffer end, so later input could still change what it is. Carries the frontier offset the
  /// [`Incomplete`](crate::error::Incomplete) reports. Built only under `Cmpl::PARTIAL`, so a
  /// [`Complete`](crate::input::Complete) input never constructs it and the arm compiles away.
  Withheld(L::Offset),
}

/// Where a scan latches the poison boundary on a limit trip: the **durable frontier**, the offset
/// up to which what the scan has already passed stays reproducible.
///
/// Two shapes cover every scanner path: a scan that commits no progress before its
/// poisoned/exhausted outcome latches at the cursor ([`AtCursor`]); a scan that consumes tokens as
/// it goes latches at — and later commits — the end of the last consumed token ([`AtFrontier`]).
trait Frontier<'inp, L: Lexer<'inp>> {
  /// The offset a trip latches as the durable frontier. `cursor` is the current
  /// scan position, used by scans that accumulate no progress of their own.
  fn boundary(&self, cursor: &L::Offset) -> L::Offset;
}

/// Frontier for scans that commit no progress before stopping (`next`,
/// `try_expect*`): a trip latches at the cursor, since nothing accumulates.
struct AtCursor;

impl<'inp, L: Lexer<'inp>> Frontier<'inp, L> for AtCursor {
  #[inline(always)]
  fn boundary(&self, cursor: &L::Offset) -> L::Offset {
    cursor.clone()
  }
}

/// Frontier for scans that consume tokens as they go (`skip_while` and the `sync` family, through
/// the shared [scanner](scan)): a trip latches at — and the scan commits — the end of the last
/// consumed token, tracked here as its span and the lexer state that produced it.
///
/// # It tracks TOKENS — a skipped lexer error is not one
///
/// The only thing that ever settles behind this frontier is a token the scan skipped
/// ([`adopt`](Self::adopt)). A lexer error the scan crosses on the way does **not** move it, and
/// that is a rule, not an omission: the frontier's span is what [`commit_at`](InputRef::commit_at)
/// writes into `self.span`, which every path in this crate reserves for the last consumed *token*
/// (`next` and `try_expect` set it from the token they consumed, never from an error they skipped
/// past, and a `peek` sets it from nothing at all).
///
/// Letting an error settle here also made `self.span` — and the boundary a later trip latched —
/// depend on **who crossed the error**. A scan that lexes across one would move; a *peek* that
/// lexed the very same region into the cache cannot, so the identical call committed a different
/// span, and latched a different durable frontier, purely as a function of how deep the caller had
/// peeked. The scan needs neither: `lex_at` already carries it past the error, and the next token
/// it finds arrives paired with the post-error lexer state, so both facts are threaded by the same
/// two carriers that thread them everywhere else.
struct AtFrontier<S, St> {
  span: S,
  state: St,
}

impl<S, St> AtFrontier<S, St> {
  /// Settles a token the scan skipped behind the frontier: its span, and the state that produced
  /// it.
  ///
  /// This is the frontier's **only** mutator. The token arrives carrying both facts — from the
  /// cache, or freshly lexed and paired with the lexer's state — so the two feeds write the same
  /// thing and the position a scan commits cannot depend on which fed it. See the type's docs for
  /// why a crossed lexer error is not among them.
  #[inline(always)]
  #[must_use = "drop the replaced pair AFTER the settle's notification: dropping it here puts                 caller code between adopting the token and telling the observer about it"]
  fn adopt(&mut self, span: S, state: St) -> (S, St) {
    // `self.span = span; self.state = state;` is a TEARING pair, and the reason is not obvious
    // enough to leave implicit: an assignment installs its new value and *then* drops the value it
    // replaced, so the replaced SPAN's `Drop` — caller code — runs between the two writes. If it
    // unwinds, the second assignment never executes and the frontier is published carrying one
    // token's span beside the previous token's state. A committing mode's unwind edge commits
    // that frontier, so the tear reaches the input. Measured: span (3, 5) paired with the tally
    // after token (0, 2).
    //
    // Same discipline as `InputRef::commit_position`: install both halves with no drop between
    // them, and let the replaced values go only once the pair is whole.
    // The replaced pair is HANDED BACK rather than dropped here, for the same reason
    // `InputRef::replace_position` hands its own back: these drops are caller code, and the
    // caller has a notification to make between adopting the token and letting them go.
    (
      core::mem::replace(&mut self.span, span),
      core::mem::replace(&mut self.state, state),
    )
  }
}

impl<'inp, L: Lexer<'inp>> Frontier<'inp, L> for AtFrontier<L::Span, L::State> {
  #[inline(always)]
  fn boundary(&self, _cursor: &L::Offset) -> L::Offset {
    self.span.end_ref().clone()
  }
}

/// The **cursor-keyed geometry** half of a restore: drop every resident cache entry that the
/// restored position has moved back past, keeping the suffix that still lies ahead of it.
///
/// This used to be `Cache::rewind`, and it could not be implemented correctly from the inputs a
/// cache was given. The method received only a `&Checkpoint`, whose public surface is the cursor
/// and the state — while the datum that distinguishes a pre-save entry from a post-save one, the
/// push generation, is crate-private. The documented sentence ("clear any tokens that were added
/// after the checkpoint was created") was therefore unimplementable by a third party, and an
/// implementation that tried over-dropped pre-save lookahead: the region re-lexes, so limit
/// budgets are re-burnt, instrumentation doubles, and poison can latch at a position the original
/// lineage never reached.
///
/// So the protocol has one owner. The input layer does the geometry here, through the trait's
/// own queue surface, and then does the generation-keyed tail-drop it always did — the half only
/// it can do. A cache implements a queue and nothing else.
///
/// The loop is exactly the fixpoint of the three built-in bodies it replaces: a cursor before the
/// resident window clears (first iteration); a cursor exactly at the front keeps the whole
/// suffix; a cursor at or past the back pops everything, which is the clear; and a mid-window
/// cursor pops the prefix and then either lands on an exact start (keep the suffix) or overshoots
/// into the "before the front" case (clear). The capacity-1 and capacity-0 caches are the
/// degenerate cases of the same loop.
#[inline]
fn reconcile_cache_geometry<'inp, L, C, Lang>(cache: &mut C, cursor: &L::Offset)
where
  L: Lexer<'inp> + 'inp,
  Lang: ?Sized,
  C: Cache<'inp, L, Lang> + ?Sized,
{
  while let Some(front_start) = cache.front_span().map(|span| span.start_ref()) {
    if cursor == front_start {
      // Exact front match: every resident entry lies at or after the restored position.
      return;
    }
    if cursor < front_start {
      // The restored position predates the resident window entirely.
      cache.clear();
      return;
    }
    // The cursor is past this token's start, so the restore has moved back past it: drop it and
    // re-examine the new front.
    cache.pop_front();
  }
}

/// Where a token being put back came from — the *only* thing [`InputRef::hold_front`] needs to
/// know, and only for the cache-push history.
#[derive(Clone, Copy)]
enum Origin {
  /// Popped off the cache front: putting it back is a no-op on the push history.
  Cache,
  /// Unparked from the front slot: never counted as a cache push, so a cache that now accepts it
  /// gains a genuinely new entry.
  Parked,
  /// Lexed by the caller once the front had run out: same as `Parked` for accounting.
  Lexer,
}

/// The one value a fresh lexer resumes from: the lexer, already bumped to its resume offset,
/// and that offset.
///
/// The pair is a *type* rather than two locals because the two facts are not independent — the
/// state **is** a function of the tokens lexed up to that offset — and a driver holding a lexer
/// built from one state beside an offset read from another silently resumes at the right byte
/// under the wrong state. Both fields are written once, by one of the two constructors on
/// [`InputRef`] ([`resume`](InputRef::resume), [`resume_at_frontier`](InputRef::resume_at_frontier)),
/// and each of those reads **one** bundled source: the newest [`CachedToken`], the committed
/// `(state, span)` pair, or an [`AtFrontier`]. There is no way to assemble a `Resume` from two
/// independently chosen facts, and [`scan_with`](InputRef::scan_with) accepts nothing but the
/// [`ResumeParts`] view of one, so a driver cannot re-derive the pairing and get it wrong.
/// [`lex_within_boundary`](InputRef::lex_within_boundary) — reached only from the two loops that
/// hold such a view — takes the halves as separate `&mut` for the codegen reason `ResumeParts`
/// documents; RESUME_CENSUS locks the constructor count and both caller lists.
struct Resume<L, Off> {
  lexer: L,
  at: Off,
}

impl<L, Off> Resume<L, Off> {
  /// The lexer, for the read-only questions a driver asks of it (its state, its span).
  #[inline(always)]
  const fn lexer(&self) -> &L {
    &self.lexer
  }

  /// The position the next token will be lexed at.
  #[inline(always)]
  const fn at(&self) -> &Off {
    &self.at
  }

  /// The two halves the single lexing site advances together. RESUME_CENSUS — the one
  /// mutation surface, and therefore the one place the pairing could drift apart. The parts are
  /// handed out as [`ResumeParts`] — two separate `&mut`, constructible only here — so the lexing
  /// entry points still accept nothing a driver could assemble from independently chosen facts.
  #[inline(always)]
  const fn parts_mut(&mut self) -> ResumeParts<'_, L, Off> {
    ResumeParts {
      lexer: &mut self.lexer,
      at: &mut self.at,
    }
  }

  /// Takes the lexer out, for the accept arms that adopt its state.
  #[inline(always)]
  fn into_lexer(self) -> L {
    self.lexer
  }
}

/// The mutable view of a [`Resume`] the lexing entry points operate on: the same bundled pair, as
/// **two separate `&mut`**.
///
/// Two independent borrows rather than one `&mut Resume` for a load-bearing codegen reason: the
/// per-token `Lexer::lex` call is opaque and reached through `lexer`, and when both halves live
/// behind one pointer the compiler must assume that call may read or write `at`, forcing `at` into
/// memory across every loop iteration. As two `&mut` params the aliasing is ruled out and the lex
/// position stays in a register. Constructible only by [`Resume::parts_mut`] (fields private to
/// this module tree), so a driver still cannot hand the lexing entry points an independently
/// assembled (lexer, offset) pair.
struct ResumeParts<'r, L, Off> {
  lexer: &'r mut L,
  at: &'r mut Off,
}
