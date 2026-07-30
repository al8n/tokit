use core::marker::PhantomData;

use super::{Cursor, Lexer};

/// A saved position in the token stream, together with everything needed to resume
/// from it: the cursor, the last-consumed span, the lexer state, the emitter's
/// emission mark, the lexer-error deduplication watermark, and the poison boundary.
///
/// # Unstable: obtainable only under `unstable-raw`
///
/// This type is public so it can be named (the guards, savepoints and session points hold one
/// internally), but its lifecycle is **sealed** to the `unstable-raw`
/// feature: [`save`](crate::InputRef::save) (the only way to obtain one),
/// [`restore`](crate::InputRef::restore), and [`commit`](crate::InputRef::commit) (the only ways
/// to consume one) are public only with that feature. Without it, a `Checkpoint` cannot be
/// created or spent from another crate, so the raw contract below is unrepresentable downstream —
/// the supported backtracking surface is the transaction guards
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " ([`begin`](crate::InputRef::begin) / [`begin_stacked`](crate::InputRef::begin_stacked)), the"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " ([`begin`](crate::InputRef::begin) / `begin_stacked`), the"
)]
/// [`InputRef`](crate::InputRef) session points
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " ([`begin_point`](crate::InputRef::begin_point)), and"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " (`begin_point`), and"
)]
/// [`attempt`](crate::InputRef::attempt)/[`try_attempt`](crate::InputRef::try_attempt). With the
/// feature on, the last-in, first-out / lineage contract that follows applies unchanged.
///
/// A checkpoint is a snapshot of one **lineage**: the concrete history of tokens
/// lexed and diagnostics emitted up to the moment of the save. Restoring makes that
/// history the live one again. Because diagnostics roll back by truncation (see
/// [`Emitter::rewind`](crate::emitter::Emitter::rewind)), only positions on the
/// *current* lineage can be returned to — which gives checkpoints a stack discipline.
///
/// # Validity
///
/// - A checkpoint is valid from the moment [`save`](crate::InputRef::save) returns it
///   until either **it** is restored ([`restore`](crate::InputRef::restore) consumes
///   it), or an **older** checkpoint is restored.
/// - Restoring a checkpoint **invalidates every checkpoint saved after it**: their
///   lineage — the diagnostics emitted and the tokens lexed after the older save —
///   has been rolled back, and a truncated emission log cannot be rebuilt. There is
///   no correct state such a restore could produce.
/// - Checkpoints are single-use, cannot be cloned, and must be restored into the same
///   input — indeed the same handle — that created them. The handle's invariant `'closure`
///   brand enforces this at compile time: a checkpoint one handle saved cannot be restored or
///   committed into another (see [`InputRef::restore`](crate::InputRef::restore)).
///
/// Every saved checkpoint should end in exactly one of two verbs:
/// [`restore`](crate::InputRef::restore) to abandon the branch and rewind, or
/// [`commit`](crate::InputRef::commit) to keep the branch's progress and release the
/// checkpoint's lineage entry. Merely dropping a checkpoint keeps its progress but, in
/// allocator builds, strands that lineage entry on the input's live-checkpoint stack until an
/// older restore pops through it — so speculation that drops rather than commits its
/// checkpoints grows that stack for the life of the input.
///
/// Restores that follow this discipline are exact: the token stream, the retained
/// diagnostics, the exactly-once lexer-error guarantee, and the poison boundary all
/// replay precisely as they stood at save time. Cache entries the abandoned branch
/// dropped or consumed before the restore are not memoized back; they re-lex on demand,
/// and by the `Lexer` determinism contract that replay reproduces them identically
/// (scan-count instrumentation held outside the lexer state does observe the extra scans).
///
/// In debug builds [`restore`](crate::InputRef::restore) verifies the discipline and
/// panics on violation; see its documentation for release behavior. Prefer
/// [`attempt`](crate::InputRef::attempt) (and
/// [`try_attempt`](crate::InputRef::try_attempt)), which manage the save/restore pair
/// structurally and cannot violate the discipline.
pub struct Checkpoint<'a, 'closure, L: Lexer<'a>> {
  /// `pub(crate)` like every other field here so a restore can take the whole checkpoint apart
  /// into owned locals before it starts mutating. Reaching through `&checkpoint.span` as it went
  /// inferred a borrowed `MaybeRef` and bought an `L::Span::clone` mid-restore.
  pub(crate) cursor: Cursor<'a, 'closure, L>,
  /// The actual `InputRef::span` at save time.
  ///
  /// This is the span of the last consumed token, which may differ from the
  /// cursor when the cache is non-empty.  Restoring with `self.span` (rather
  /// than the cursor's offset) ensures that the lexer position is placed *before*
  /// any cached tokens, so they can be re-lexed after a restore.
  pub(crate) span: L::Span,
  pub(crate) state: L::State,
  /// The emitter's emission mark at save time (see
  /// [`Emitter::checkpoint`](crate::emitter::Emitter::checkpoint)). Restoring
  /// replays it into [`Emitter::rewind`](crate::emitter::Emitter::rewind) so an
  /// emission-aware emitter drops exactly the diagnostics of the abandoned branch.
  pub(crate) emitter_checkpoint: u64,
  /// The lexer-error dedup high-water mark at save time.
  ///
  /// A speculative branch may seal (emit) a lexer error whose span end sits
  /// *above* the checkpoint cursor — e.g. a `peek` that scans past the cursor.
  /// [`Emitter::rewind`](crate::emitter::Emitter::rewind) keeps that error (it
  /// predates the emission checkpoint), so restoring the watermark to the cursor
  /// would drop it below the retained error and let a re-lex emit it a second
  /// time. Restoring *this* saved mark instead keeps the watermark above the
  /// retained error, preserving exactly-once emission; errors sealed *after* the
  /// checkpoint were unwound from the emitter, and this mark (predating them)
  /// correctly permits their re-emission if the committed path re-lexes them.
  pub(crate) emitted_error_end: L::Offset,
  /// The **front-report watermark** at save time (see
  /// [`Input::front_reported_end`](super::Input)).
  ///
  /// Restored verbatim beside the emitter mark, and that pairing is the whole point: a report
  /// predating the mark survives the rewind together with its watermark, and one postdating the
  /// mark is truncated together with its watermark. Neither can outlive the other.
  pub(crate) front_reported_end: Option<L::Offset>,
  /// The input-level sticky limit-error boundary at save time.
  ///
  /// `None` is unpoisoned; `Some(off)` is the durable frontier a trip latched (see
  /// [`Input::poison_boundary`](crate::input::Input)). It is a fact of one lineage,
  /// checkpointed alongside the emitter mark and the dedup watermark because the three
  /// move together: a speculative peek that trips the limit latches the frontier,
  /// emits the limit diagnostic, and lifts the watermark in one step, and
  /// [`restore`](crate::InputRef::restore) copies all three back to their saved values
  /// together. Under the last-in, first-out restore contract a saved `Some(off)`'s
  /// diagnostic predates the saved emitter mark, so it survives the emitter rewind and
  /// the restored frontier stays paired with it; a saved `None` re-lexes and re-trips
  /// if the limit is reached again.
  pub(crate) poison_boundary: Option<L::Offset>,
  /// The cache's monotone push count at save time.
  ///
  /// The cache memoizes token *values* but not the scan *side effects* of the region a
  /// token came from (a lexer error emitted while lexing across it). Entries pushed after
  /// this mark belong to a continuation a restore may abandon; leaving them in place would
  /// let a later drain jump over a rewound error instead of re-lexing — and re-emitting —
  /// its region. [`restore`](crate::InputRef::restore) drops exactly those post-save
  /// entries. It is correctness state in every build, not a debug-only witness.
  pub(crate) cache_pushes: u64,
  /// The identity of the input that produced this checkpoint (debug-only witness).
  #[cfg(all(
    debug_assertions,
    any(feature = "std", feature = "alloc"),
    target_has_atomic = "ptr"
  ))]
  pub(crate) input_id: usize,
  /// This checkpoint's id on its input's live-checkpoint lineage stack. Present in every
  /// allocator build (not just debug witness builds), so a
  /// [`StackedTransaction`](crate::StackedTransaction) can check that a savepoint's lineage
  /// is still live before honoring it, and so a restore can pop the lineage down through
  /// this checkpoint. See [`InputRef::save`](crate::InputRef::save).
  #[cfg(any(feature = "std", feature = "alloc"))]
  pub(crate) ckp_id: u64,
  _m: PhantomData<fn(&'closure ()) -> &'closure ()>,
}

impl<'a, 'closure, L: Lexer<'a>> Checkpoint<'a, 'closure, L> {
  /// Creates a new checkpoint.
  #[inline(always)]
  #[allow(clippy::too_many_arguments)]
  pub(super) const fn new(
    cursor: Cursor<'a, 'closure, L>,
    span: L::Span,
    state: L::State,
    emitter_checkpoint: u64,
    emitted_error_end: L::Offset,
    front_reported_end: Option<L::Offset>,
    poison_boundary: Option<L::Offset>,
    cache_pushes: u64,
    #[cfg(all(
      debug_assertions,
      any(feature = "std", feature = "alloc"),
      target_has_atomic = "ptr"
    ))]
    input_id: usize,
    #[cfg(any(feature = "std", feature = "alloc"))] ckp_id: u64,
  ) -> Self {
    Self {
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
      _m: PhantomData,
    }
  }

  /// Returns the cursor of the checkpoint.
  #[inline(always)]
  pub const fn cursor(&self) -> &Cursor<'a, 'closure, L> {
    &self.cursor
  }

  /// Returns the state of the checkpoint.
  #[inline(always)]
  pub const fn state(&self) -> &L::State {
    &self.state
  }
}
