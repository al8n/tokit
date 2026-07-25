//! The write-chokepointed storage behind [`Verbose`](super::Verbose).
//!
//! # Why the storage is a child module
//!
//! Every one of `Verbose`'s emit channels must record on the shared emission log, because
//! that log is what [`checkpoint`](super::Emitter::checkpoint),
//! [`rewind`](super::Emitter::rewind) and [`diagnostics()`](super::Verbose::diagnostics) are
//! defined in terms of. A channel that appends to a payload map directly leaves the log
//! unmoved, and all three of those guarantees quietly stop holding for it — a defect that is
//! invisible at the write site and only shows up as a phantom diagnostic, an unmoved mark, or
//! a payload replayed against the wrong entry.
//!
//! Rust's own privacy rule makes that write unspellable rather than merely discouraged: a
//! private field is visible in its defining module and that module's *descendants*, so the
//! fields below are invisible to `store`'s siblings — the eleven channel files — and to its
//! parent. The only way in from a channel file is [`Store::record`] and its two mirrors, and
//! reaching for the raw map there is `error[E0616]`, not a review comment.

use std::{collections::BTreeMap, vec::Vec};

use crate::emitter::Severity;

use super::{Channel, Diagnostics};

/// Pops the newest entry of `span`'s group in a channel map, dropping the emptied group — the
/// shared per-channel step of [`rewind_to`](Store::rewind_to)'s newest-first unwind.
fn pop_group<S, T>(groups: &mut BTreeMap<S, Vec<T>>, span: &S)
where
  S: Ord,
{
  if let Some(group) = groups.get_mut(span) {
    group.pop();
    if group.is_empty() {
      groups.remove(span);
    }
  }
}

/// One emission-log entry: the channel tag, the span key, and the entry's slot within its
/// span's group in that channel at emit time.
///
/// The slot is what lets [`Diagnostics`] replay by direct index instead of rebuilding a
/// per-span cursor tree on every walk, and what [`rewind_to`](Store::rewind_to) debug-asserts
/// against: the log is popped newest-first, so a popped entry's slot always names its group's
/// current tail.
#[derive(Debug, Clone)]
pub(super) struct LogEntry<S> {
  pub(super) channel: Channel,
  pub(super) span: S,
  pub(super) slot: usize,
}

/// [`Verbose`](super::Verbose)'s collected diagnostics and the emission log that orders them.
///
/// Every field is private to this module by design (see the module docs): the write surface is
/// [`record`](Self::record) / [`record_warning`](Self::record_warning) /
/// [`record_hole`](Self::record_hole) and nothing else.
#[derive(Debug)]
pub(super) struct Store<Error, S> {
  errs: BTreeMap<S, Vec<Error>>,
  /// Parallel to `errs`: the open-label snapshot captured when each error was
  /// recorded, kept in lockstep with the error groups (same span keys, same
  /// per-span `Vec` lengths). `label_snapshots[span][i]` is the *"while parsing X"*
  /// context stack that was open when `errs[span][i]` was emitted. A separate map
  /// (rather than pairing the label into the error) keeps
  /// [`errors()`](super::Verbose::errors) returning exactly `&BTreeMap<S, Vec<Error>>`.
  label_snapshots: BTreeMap<S, Vec<Vec<&'static str>>>,
  /// The warning channel: mirrors `errs` in shape but is fed by
  /// [`emit_warning`](super::Emitter::emit_warning) rather than the error emit paths. Kept
  /// separate so [`errors()`](super::Verbose::errors) and
  /// [`warnings()`](super::Verbose::warnings) each return a clean `&BTreeMap<S, Vec<Error>>`
  /// for their own [`Severity`] tier.
  warns: BTreeMap<S, Vec<Error>>,
  /// Parallel to `warns`, exactly as `label_snapshots` is parallel to `errs`.
  warn_label_snapshots: BTreeMap<S, Vec<Vec<&'static str>>>,
  /// The skipped-region channel: one entry per recovery hole recorded by
  /// [`emit_skipped_region`](super::Emitter::emit_skipped_region), keyed by the hole span,
  /// each entry the skipped-token count. Payload-less (no `Error` value), which is why it is
  /// its own map rather than a third `Severity` tier.
  holes: BTreeMap<S, Vec<usize>>,
  /// Parallel to `holes`, exactly as `label_snapshots` is parallel to `errs`.
  hole_label_snapshots: BTreeMap<S, Vec<Vec<&'static str>>>,
  /// The `(channel, span)` of every emission, in emission order — the single ordering
  /// authority across *all* channels. An entry's index in this log is its monotonic sequence
  /// number; [`mark`](Self::mark) is the log length and [`rewind_to`](Self::rewind_to) unwinds
  /// the tail back to a mark, popping the matching record — and its label snapshot — off the
  /// channel named by the entry's [`Channel`] tag. This is what lets rewind drop a speculative
  /// zero-width diagnostic while keeping an earlier one at the same span, and what lets
  /// [`diagnostics()`](Self::diagnostics) reconstruct the true interleaving of the payload
  /// channels — a distinction span-ordered storage alone cannot make.
  log: Vec<LogEntry<S>>,
  /// The currently-open label stack, pushed by
  /// [`enter_label`](super::Emitter::enter_label) and popped by
  /// [`exit_label`](super::Emitter::exit_label). Snapshotted (cloned) into the recording
  /// channel at each emit; a push/pop never allocates and an empty snapshot clones for free.
  stack: Vec<&'static str>,
}

impl<Error, S> Default for Store<Error, S> {
  #[inline(always)]
  fn default() -> Self {
    Self::new()
  }
}

impl<Error, S> Clone for Store<Error, S>
where
  Error: Clone,
  S: Clone,
{
  #[inline(always)]
  fn clone(&self) -> Self {
    Self {
      errs: self.errs.clone(),
      label_snapshots: self.label_snapshots.clone(),
      warns: self.warns.clone(),
      warn_label_snapshots: self.warn_label_snapshots.clone(),
      holes: self.holes.clone(),
      hole_label_snapshots: self.hole_label_snapshots.clone(),
      log: self.log.clone(),
      stack: self.stack.clone(),
    }
  }
}

impl<Error, S> Store<Error, S> {
  /// An empty store: no diagnostics, no log, no open labels.
  #[inline(always)]
  pub(super) const fn new() -> Self {
    Self {
      errs: BTreeMap::new(),
      label_snapshots: BTreeMap::new(),
      warns: BTreeMap::new(),
      warn_label_snapshots: BTreeMap::new(),
      holes: BTreeMap::new(),
      hole_label_snapshots: BTreeMap::new(),
      log: Vec::new(),
      stack: Vec::new(),
    }
  }

  /// Records `err` in the **error** channel at `span`, appending it to the span's group and
  /// logging the emission (tagged [`Severity::Error`]) so a later
  /// [`rewind_to`](Self::rewind_to) can undo it precisely.
  ///
  /// A snapshot of the currently-open label stack is captured alongside the error,
  /// into `label_snapshots` at the same span/index — this is the *capture-at-emit*
  /// point for diagnostic labels. Cloning an empty stack does not allocate, so an
  /// unlabelled emission pays nothing beyond the parallel bookkeeping.
  ///
  /// The debug assert is the parallel-map tripwire: the payload group and its label
  /// snapshots are only ever grown here, one element each, so a span where they have drifted
  /// apart means a write that went round this chokepoint — which is exactly the class of
  /// defect that voids the mark, the unwind and the replay for the channel that did it.
  ///
  /// # Torn state under an unwinding panic
  ///
  /// This body is **not** atomic. The label snapshot is cloned before either map is touched,
  /// but each step after that can still panic — a panicking `S: Clone`, or a capacity
  /// overflow on a `Vec` grow (allocation *failure* aborts rather than unwinds, so it is not
  /// among these) — and the write is then left half-done. Logging last is chosen, not
  /// incidental, and the posture it buys is a *lesser* worst case rather than exactness:
  ///
  /// - The log entry is appended after both maps, so it can never name a slot that was never
  ///   filled. Under the reverse order an unwind mid-record left a dangling log entry, and
  ///   replay — which indexes by the logged slot — would panic on it.
  /// - What can survive instead is an **orphan payload**: an emission already appended to
  ///   the span's group, and so visible through the error accessors, with no entry in the
  ///   log. A [`rewind_to`](Self::rewind_to) with a mark taken before it will not remove it,
  ///   because rewinding walks the log. The diagnostic is stuck, not corrupt.
  /// - An unwind between the two `Vec::push`es leaves the narrower version of that: the group
  ///   grew and its label snapshots did not. That skew is only *deferred* — the next record
  ///   at the same span would log a slot one past the label group's end, and replay would
  ///   index out of bounds — so the assert above, not the ordering, is what makes it
  ///   detectable.
  ///
  /// No unwind guard is installed, so none of this promises that a record is atomic — only
  /// that the surviving state stays readable, and that replay can never be handed a slot that
  /// does not exist.
  #[inline(always)]
  pub(super) fn record(&mut self, span: S, err: Error)
  where
    S: Ord + Clone,
  {
    let snapshot = self.stack.clone();
    let group = self.errs.entry(span.clone()).or_default();
    let slot = group.len();
    group.push(err);
    let labels = self.label_snapshots.entry(span.clone()).or_default();
    debug_assert_eq!(
      labels.len(),
      slot,
      "the error group and its label snapshots must be parallel before a record: a span \
       where they have drifted apart was written outside this chokepoint"
    );
    labels.push(snapshot);
    self.log.push(LogEntry {
      channel: Channel::Diagnostic(Severity::Error),
      span,
      slot,
    });
  }

  /// Records `warning` in the **warning** channel at `span` — the exact mirror of
  /// [`record`](Self::record), but into `warns`/`warn_label_snapshots` and logging the
  /// emission tagged [`Severity::Warning`]. The shared `log` keeps both channels on one
  /// emission timeline, so a [`rewind_to`](Self::rewind_to) unwinds warnings and errors
  /// together in reverse emission order.
  #[inline(always)]
  pub(super) fn record_warning(&mut self, span: S, warning: Error)
  where
    S: Ord + Clone,
  {
    let snapshot = self.stack.clone();
    let group = self.warns.entry(span.clone()).or_default();
    let slot = group.len();
    group.push(warning);
    let labels = self.warn_label_snapshots.entry(span.clone()).or_default();
    debug_assert_eq!(
      labels.len(),
      slot,
      "the warning group and its label snapshots must be parallel before a record: a span \
       where they have drifted apart was written outside this chokepoint"
    );
    labels.push(snapshot);
    self.log.push(LogEntry {
      channel: Channel::Diagnostic(Severity::Warning),
      span,
      slot,
    });
  }

  /// Records a recovery hole in the **skipped-region** channel at `span` — the same shape as
  /// [`record_warning`](Self::record_warning), but the payload is the skipped-token count and
  /// the log entry is tagged [`Channel::SkippedRegion`]. The shared `log` keeps all record
  /// kinds on one emission timeline, so a [`rewind_to`](Self::rewind_to) unwinds hole records
  /// together with diagnostics in reverse emission order.
  #[inline(always)]
  pub(super) fn record_hole(&mut self, span: S, skipped: usize)
  where
    S: Ord + Clone,
  {
    let snapshot = self.stack.clone();
    let group = self.holes.entry(span.clone()).or_default();
    let slot = group.len();
    group.push(skipped);
    let labels = self.hole_label_snapshots.entry(span.clone()).or_default();
    debug_assert_eq!(
      labels.len(),
      slot,
      "the hole group and its label snapshots must be parallel before a record: a span \
       where they have drifted apart was written outside this chokepoint"
    );
    labels.push(snapshot);
    self.log.push(LogEntry {
      channel: Channel::SkippedRegion,
      span,
      slot,
    });
  }

  /// The emission mark: the log's length, an opaque monotone counter that advances by exactly
  /// one per recorded emission on every channel.
  #[inline(always)]
  pub(super) fn mark(&self) -> u64 {
    self.log.len() as u64
  }

  /// Unwinds every emission recorded after `mark`, newest first.
  ///
  /// Each span's group grows in emission order, so the matching entry to drop is always its
  /// last one. The [`Channel`] tag names the maps the entry was recorded in, so the pop lands
  /// in the right channel. The dropped entry takes its label snapshot with it — labels
  /// captured into an entry are rewound together with it, and any later re-emission re-derives
  /// labels from the then-current stack. A mark beyond the current length is clamped: `Verbose`
  /// keeps no per-mark bookkeeping, so a stale mark can only name a suffix that is already gone.
  #[inline(always)]
  pub(super) fn rewind_to(&mut self, mark: u64)
  where
    S: Ord,
  {
    let mark = (mark as usize).min(self.log.len());
    while self.log.len() > mark {
      let LogEntry {
        channel,
        span,
        slot,
      } = self.log.pop().expect("log length exceeds the mark");
      match channel {
        Channel::Diagnostic(severity) => {
          let (groups, labels) = match severity {
            Severity::Error => (&mut self.errs, &mut self.label_snapshots),
            Severity::Warning => (&mut self.warns, &mut self.warn_label_snapshots),
          };
          debug_assert_eq!(
            groups.get(&span).map(Vec::len),
            Some(slot + 1),
            "the newest-first unwind must meet the emit-time slot"
          );
          debug_assert_eq!(
            labels.get(&span).map(Vec::len),
            Some(slot + 1),
            "the label group unwinds in lockstep with its payload group"
          );
          pop_group(groups, &span);
          pop_group(labels, &span);
        }
        Channel::SkippedRegion => {
          debug_assert_eq!(
            self.holes.get(&span).map(Vec::len),
            Some(slot + 1),
            "the newest-first unwind must meet the emit-time slot"
          );
          debug_assert_eq!(
            self.hole_label_snapshots.get(&span).map(Vec::len),
            Some(slot + 1),
            "the label group unwinds in lockstep with its payload group"
          );
          pop_group(&mut self.holes, &span);
          pop_group(&mut self.hole_label_snapshots, &span);
        }
      }
    }
  }

  /// Pushes a *"while parsing X"* label onto the open-label stack; the next recorded
  /// diagnostic snapshots it into the entry it emits.
  #[inline(always)]
  pub(super) fn enter_label(&mut self, label: &'static str) {
    self.stack.push(label);
  }

  /// Pops the innermost open label as its [`labelled`](crate::labelled) scope closes.
  #[inline(always)]
  pub(super) fn exit_label(&mut self) {
    self.stack.pop();
  }

  /// The error channel's span-keyed groups.
  #[inline(always)]
  pub(super) const fn errors(&self) -> &BTreeMap<S, Vec<Error>> {
    &self.errs
  }

  /// The error channel's per-diagnostic label snapshots.
  #[inline(always)]
  pub(super) const fn labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    &self.label_snapshots
  }

  /// The warning channel's span-keyed groups.
  #[inline(always)]
  pub(super) const fn warnings(&self) -> &BTreeMap<S, Vec<Error>> {
    &self.warns
  }

  /// The warning channel's per-diagnostic label snapshots.
  #[inline(always)]
  pub(super) const fn warning_labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    &self.warn_label_snapshots
  }

  /// The skipped-region channel's span-keyed skipped-token counts.
  #[inline(always)]
  pub(super) const fn skipped_regions(&self) -> &BTreeMap<S, Vec<usize>> {
    &self.holes
  }

  /// The skipped-region channel's per-hole label snapshots.
  #[inline(always)]
  pub(super) const fn skipped_region_labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    &self.hole_label_snapshots
  }

  /// Replays every recorded diagnostic — errors, warnings and holes — in emission order.
  #[inline(always)]
  pub(super) fn diagnostics(&self) -> Diagnostics<'_, S, Error> {
    Diagnostics::new(
      &self.log,
      &self.errs,
      &self.label_snapshots,
      &self.warns,
      &self.warn_label_snapshots,
      &self.holes,
      &self.hole_label_snapshots,
    )
  }
}
