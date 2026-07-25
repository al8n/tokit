use crate::{
  Lexer,
  emitter::Severity,
  span::{SimpleSpan, Span, Spanned},
};

pub use diagnostic::{Diagnostic, DiagnosticKind, Diagnostics};

use super::super::{
  separated::{
    MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  *,
};

use std::{collections::BTreeMap, vec::Vec};

use core::marker::PhantomData;

mod diagnostic;
mod full_container;
mod missing_leading_separator;
mod missing_trailing_separator;
mod pratt;
mod separator;
mod too_few;
mod too_many;
mod unclosed;
mod unexpected_leading_separator;
mod unexpected_trailing_separator;

/// Which channel one emission-log entry was recorded in: a payload-carrying diagnostic (an
/// error or a warning, tagged with its [`Severity`]) or a payload-less skipped-region record
/// (a recovery hole: span + skipped-token count). One tag per log entry is what lets
/// [`rewind`](Emitter::rewind) pop each entry off the map it was recorded in, and what keeps
/// the [`Diagnostics`] iterator's per-channel cursors exact when the record kinds interleave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channel {
  /// An error- or warning-channel record carrying an `Error` payload in the channel its
  /// [`Severity`] names.
  Diagnostic(Severity),
  /// An [`emit_skipped_region`](Emitter::emit_skipped_region) record: no payload value, just
  /// the skipped-token count keyed by the hole span (see
  /// [`skipped_regions`](Verbose::skipped_regions)).
  SkippedRegion,
}

/// Pops the newest entry of `span`'s group in a channel map, dropping the emptied group — the
/// shared per-channel step of [`rewind`](Emitter::rewind)'s newest-first unwind.
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

/// A verbose emitter that collects all errors during parsing.
///
/// Unlike [`Fatal`](super::fatal::Fatal) which stops at the first error, or [`Silent`](super::silent::Silent)
/// which ignores errors silently, `Verbose` collects all errors encountered during parsing and
/// continues parsing where possible. This makes it ideal for compiler diagnostics, IDE integration,
/// and development scenarios where you need comprehensive error reporting.
///
/// `Verbose` is a **complete implementation** of all atomic emitter traits, providing a pre-built bundle
/// for comprehensive error collection. It implements:
/// - [`Emitter`](super::super::Emitter) - Core error handling
/// - [`TooFewEmitter`](super::super::TooFewEmitter) - "Too few elements" errors
/// - [`TooManyEmitter`](super::super::TooManyEmitter) - "Too many elements" errors
/// - [`SeparatedEmitter`](super::super::SeparatedEmitter) - Separator errors
/// - And other atomic traits for specific parsing scenarios
///
/// The errors are stored in a `BTreeMap` indexed by span, ensuring they are ordered by their
/// position in the source code. Multiple errors can share a single span (for example a
/// zero-width missing-element and missing-separator reported at the same offset), so each span
/// maps to a `Vec` of errors that accumulate in emission order rather than overwriting one
/// another. You can retrieve all collected errors via the [`errors()`](Self::errors) method.
///
/// # Examples
///
/// ```ignore
/// use tokora::emitter::Verbose;
///
/// // Create a verbose emitter
/// let emitter = Verbose::<MyError>::new();
///
/// // After parsing, retrieve all errors (each span may carry several).
/// for (span, errors) in emitter.errors() {
///     for error in errors {
///         println!("Error at {:?}: {}", span, error);
///     }
/// }
/// ```
///
/// # Use Cases
///
/// - **Compiler Diagnostics**: Collect all errors in a single pass to show users all issues at once
/// - **IDE Integration**: Provide comprehensive error highlighting and diagnostics
/// - **Development & Debugging**: Understand all parsing issues without having to fix them one at a time
/// - **Error Recovery**: Continue parsing after errors to provide better context and suggestions
///
/// # Comparison with Other Emitters
///
/// | Emitter | Behavior | Atomic Traits | Use Case |
/// |---------|----------|---------------|----------|
/// | [`Fatal`](super::fatal::Fatal) | Stop on first error | Implements all | Runtime, REPL, fail-fast scenarios |
/// | [`Silent`](super::silent::Silent) | Ignore all errors | Implements all | Error recovery, best-effort parsing |
/// | `Verbose` | Collect all errors | Implements all | Compilers, IDEs, comprehensive diagnostics |
/// | Custom | User-defined | Implement only what you need | Specialized use cases |
///
/// Thanks to Tokora's **atomically composable trait design**, you can implement only the emitter traits
/// your parser needs. `Verbose`, `Fatal`, and `Silent` are pre-built bundles that implement all atomic
/// traits with consistent behavior, but you're encouraged to create custom emitters by implementing just
/// the specific traits relevant to your parser.
///
/// # Diagnostic labels — context captured at emit time
///
/// [`labelled`](crate::labelled) opens a *"while parsing X"* context around a
/// sub-parse by pushing a `&'static str` onto this emitter's open-label stack
/// (via [`enter_label`](Emitter::enter_label)) and popping it as the scope closes.
/// Labels are captured **into the emission log at emit time**: each recorded
/// diagnostic carries a snapshot of the label stack that was open when it was
/// emitted, retrievable per-diagnostic through [`labels()`](Self::labels) in
/// lockstep with [`errors()`](Self::errors). Because the snapshot rides with the
/// entry, a [`rewind`](Emitter::rewind) that drops an entry drops its labels with
/// it, and a later re-emission re-derives its labels from the then-current stack;
/// the live stack itself follows the call structure of the wrapper scopes, so a
/// checkpoint restore needs no label handling at all.
///
/// # Two channels — errors and warnings
///
/// `Verbose` collects two parallel channels of diagnostics: hard [`errors()`](Self::errors)
/// (the [`Severity::Error`] tier, fed by the ordinary emit paths) and soft
/// [`warnings()`](Self::warnings) (the [`Severity::Warning`] tier, fed by
/// [`emit_warning`](Emitter::emit_warning)). Each channel keeps its own span-keyed groups and
/// its own parallel label snapshots ([`labels()`](Self::labels) /
/// [`warning_labels()`](Self::warning_labels)). A single emission `log` tags every entry with
/// its channel, so [`rewind`](Emitter::rewind) drops the abandoned branch's entries from the
/// correct channel and [`diagnostics()`](Self::diagnostics) can replay *both* channels
/// interleaved in true emission order.
///
/// # Skipped-region records — recovery holes
///
/// A third record kind rides the same log: [`emit_skipped_region`](Emitter::emit_skipped_region)
/// records the one-per-hole note of a balanced recovery skip
/// ([`sync_balanced`](crate::InputRef::sync_balanced)) — the hole's span and its skipped-token
/// count, read back through [`skipped_regions()`](Self::skipped_regions) with label snapshots
/// in [`skipped_region_labels()`](Self::skipped_region_labels). Sharing the log keeps rewind
/// exact — an abandoned branch's hole records unwind together with its diagnostics — and lets
/// [`diagnostics()`](Self::diagnostics) replay hole records interleaved with the payload
/// channels in emission order: each is yielded as a
/// [`DiagnosticKind::SkippedRegion`](crate::emitter::DiagnosticKind::SkippedRegion) carrying the
/// skipped-token count (its span and labels ride the [`Diagnostic`] as for any other record).
#[derive(Debug)]
pub struct Verbose<Error, S = SimpleSpan, Lang: ?Sized = ()> {
  errs: BTreeMap<S, Vec<Error>>,
  /// Parallel to `errs`: the open-label snapshot captured when each error was
  /// recorded, kept in lockstep with the error groups (same span keys, same
  /// per-span `Vec` lengths). `label_snapshots[span][i]` is the *"while parsing X"*
  /// context stack that was open when `errs[span][i]` was emitted. A separate map
  /// (rather than pairing the label into the error) keeps [`errors()`](Self::errors)
  /// returning exactly `&BTreeMap<S, Vec<Error>>`.
  label_snapshots: BTreeMap<S, Vec<Vec<&'static str>>>,
  /// The warning channel: mirrors `errs` in shape but is fed by
  /// [`emit_warning`](Emitter::emit_warning) rather than the error emit paths. Kept separate so
  /// [`errors()`](Self::errors) and [`warnings()`](Self::warnings) each return a clean
  /// `&BTreeMap<S, Vec<Error>>` for their own [`Severity`] tier.
  warns: BTreeMap<S, Vec<Error>>,
  /// Parallel to `warns`, exactly as `label_snapshots` is parallel to `errs`.
  warn_label_snapshots: BTreeMap<S, Vec<Vec<&'static str>>>,
  /// The skipped-region channel: one entry per recovery hole recorded by
  /// [`emit_skipped_region`](Emitter::emit_skipped_region), keyed by the hole span, each entry
  /// the skipped-token count. Payload-less (no `Error` value), which is why it is its own map
  /// rather than a third `Severity` tier.
  holes: BTreeMap<S, Vec<usize>>,
  /// Parallel to `holes`, exactly as `label_snapshots` is parallel to `errs`.
  hole_label_snapshots: BTreeMap<S, Vec<Vec<&'static str>>>,
  /// The `(channel, span)` of every emission, in emission order — the single ordering
  /// authority across *all* channels. An entry's index in this log is its monotonic sequence
  /// number; [`checkpoint`](Emitter::checkpoint) is the log length and
  /// [`rewind`](Emitter::rewind) unwinds the tail back to a mark, popping the matching record —
  /// and its label snapshot — off the channel named by the entry's [`Channel`] tag. This is
  /// what lets rewind drop a speculative zero-width diagnostic while keeping an earlier one at
  /// the same span, and what lets [`diagnostics()`](Self::diagnostics) reconstruct the true
  /// interleaving of the payload channels — a distinction span-ordered storage alone cannot
  /// make.
  log: Vec<(Channel, S)>,
  /// The currently-open label stack, pushed by [`enter_label`](Emitter::enter_label)
  /// and popped by [`exit_label`](Emitter::exit_label). Snapshotted (cloned) into
  /// the recording channel at each emit; a push/pop never allocates and an empty snapshot
  /// clones for free.
  stack: Vec<&'static str>,
  _lang: PhantomData<Lang>,
}

impl<Error, Span, Lang: ?Sized> Default for Verbose<Error, Span, Lang> {
  #[inline(always)]
  fn default() -> Self {
    Self {
      errs: BTreeMap::new(),
      label_snapshots: BTreeMap::new(),
      warns: BTreeMap::new(),
      warn_label_snapshots: BTreeMap::new(),
      holes: BTreeMap::new(),
      hole_label_snapshots: BTreeMap::new(),
      log: Vec::new(),
      stack: Vec::new(),
      _lang: PhantomData,
    }
  }
}

impl<Error, Span, Lang: ?Sized> Clone for Verbose<Error, Span, Lang>
where
  Error: Clone,
  Span: Clone,
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
      _lang: PhantomData,
    }
  }
}

impl<Error, S, Lang: ?Sized> Verbose<Error, S, Lang> {
  /// Creates a new `Verbose` emitter with an empty error collection.
  ///
  /// # Examples
  ///
  /// ```ignore
  /// use tokora::emitter::Verbose;
  ///
  /// let emitter = Verbose::<MyError>::new();
  /// assert_eq!(emitter.errors().len(), 0);
  /// ```
  #[inline(always)]
  pub const fn new() -> Self {
    Self {
      errs: BTreeMap::new(),
      label_snapshots: BTreeMap::new(),
      warns: BTreeMap::new(),
      warn_label_snapshots: BTreeMap::new(),
      holes: BTreeMap::new(),
      hole_label_snapshots: BTreeMap::new(),
      log: Vec::new(),
      stack: Vec::new(),
      _lang: PhantomData,
    }
  }

  /// Records `err` in the **error** channel at `span`, appending it to the span's group and
  /// logging the emission (tagged [`Severity::Error`]) so a later [`rewind`](Emitter::rewind)
  /// can undo it precisely.
  ///
  /// A snapshot of the currently-open label stack is captured alongside the error,
  /// into `label_snapshots` at the same span/index — this is the *capture-at-emit*
  /// point for diagnostic labels. Cloning an empty stack does not allocate, so an
  /// unlabelled emission pays nothing beyond the parallel bookkeeping.
  #[inline(always)]
  fn record(&mut self, span: S, err: Error)
  where
    S: Ord + Clone,
  {
    self
      .log
      .push((Channel::Diagnostic(Severity::Error), span.clone()));
    self
      .label_snapshots
      .entry(span.clone())
      .or_default()
      .push(self.stack.clone());
    self.errs.entry(span).or_default().push(err);
  }

  /// Records `warning` in the **warning** channel at `span` — the exact mirror of
  /// [`record`](Self::record), but into `warns`/`warn_label_snapshots` and logging the emission
  /// tagged [`Severity::Warning`]. The shared `log` keeps both channels on one emission
  /// timeline, so a [`rewind`](Emitter::rewind) unwinds warnings and errors together in reverse
  /// emission order.
  #[inline(always)]
  fn record_warning(&mut self, span: S, warning: Error)
  where
    S: Ord + Clone,
  {
    self
      .log
      .push((Channel::Diagnostic(Severity::Warning), span.clone()));
    self
      .warn_label_snapshots
      .entry(span.clone())
      .or_default()
      .push(self.stack.clone());
    self.warns.entry(span).or_default().push(warning);
  }

  /// Records a recovery hole in the **skipped-region** channel at `span` — the same shape as
  /// [`record_warning`](Self::record_warning), but the payload is the skipped-token count and
  /// the log entry is tagged [`Channel::SkippedRegion`]. The shared `log` keeps all record
  /// kinds on one emission timeline, so a [`rewind`](Emitter::rewind) unwinds hole records
  /// together with diagnostics in reverse emission order.
  #[inline(always)]
  fn record_hole(&mut self, span: S, skipped: usize)
  where
    S: Ord + Clone,
  {
    self.log.push((Channel::SkippedRegion, span.clone()));
    self
      .hole_label_snapshots
      .entry(span.clone())
      .or_default()
      .push(self.stack.clone());
    self.holes.entry(span).or_default().push(skipped);
  }

  /// Returns a reference to all collected errors.
  ///
  /// The errors are stored in a `BTreeMap` indexed by their span, which means they are
  /// automatically sorted by their position in the source code. Each span maps to a `Vec`
  /// of every error reported at that span, in emission order, so same-span errors are all
  /// retained rather than overwritten.
  ///
  /// # Examples
  ///
  /// ```ignore
  /// use tokora::emitter::Verbose;
  ///
  /// let mut emitter = Verbose::<MyError>::new();
  /// // ... perform parsing ...
  ///
  /// // Iterate through all errors in source order (flattening the per-span groups).
  /// for (span, error) in emitter.errors().iter().flat_map(|(s, es)| es.iter().map(move |e| (s, e))) {
  ///     println!("Error at position {}: {}", span.start(), error);
  /// }
  /// ```
  #[inline(always)]
  pub fn errors(&self) -> &BTreeMap<S, Vec<Error>> {
    &self.errs
  }

  /// Returns the per-diagnostic label snapshots, parallel to [`errors()`](Self::errors).
  ///
  /// The returned map mirrors [`errors()`](Self::errors) span-for-span and
  /// index-for-index: `labels()[span][i]` is the open-label stack — outermost
  /// [`labelled`](crate::labelled) context first — that was captured when
  /// `errors()[span][i]` was emitted. An unlabelled diagnostic maps to an empty
  /// stack. The two accessors are meant to be read together, e.g. by zipping each
  /// span's error group with its label group.
  ///
  /// ```ignore
  /// for (span, errs) in emitter.errors() {
  ///     let labels = &emitter.labels()[span];
  ///     for (err, ctx) in errs.iter().zip(labels) {
  ///         println!("{err} at {span:?} (while parsing {ctx:?})");
  ///     }
  /// }
  /// ```
  #[inline(always)]
  pub fn labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    &self.label_snapshots
  }

  /// Returns a reference to all collected **warnings**, parallel to [`errors()`](Self::errors).
  ///
  /// Warnings are the [`Severity::Warning`] tier, recorded via
  /// [`emit_warning`](Emitter::emit_warning). The map has the same span-keyed,
  /// group-per-span shape as [`errors()`](Self::errors); the two channels are independent, so a
  /// span may carry warnings, errors, or both.
  #[inline(always)]
  pub fn warnings(&self) -> &BTreeMap<S, Vec<Error>> {
    &self.warns
  }

  /// Returns the per-warning label snapshots, parallel to [`warnings()`](Self::warnings)
  /// exactly as [`labels()`](Self::labels) is parallel to [`errors()`](Self::errors).
  #[inline(always)]
  pub fn warning_labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    &self.warn_label_snapshots
  }

  /// Returns every recorded **skipped region** (recovery hole), keyed by the hole span; each
  /// entry is the skipped-token count recorded via
  /// [`emit_skipped_region`](Emitter::emit_skipped_region).
  ///
  /// Hole records ride the same emission log as the diagnostics, so a
  /// [`rewind`](Emitter::rewind) unwinds an abandoned branch's holes together with its errors
  /// and warnings. This accessor returns them in span order; to see them interleaved with the
  /// error and warning channels in emission order, walk [`diagnostics()`](Self::diagnostics),
  /// where each hole surfaces as a
  /// [`DiagnosticKind::SkippedRegion`](crate::emitter::DiagnosticKind::SkippedRegion).
  #[inline(always)]
  pub fn skipped_regions(&self) -> &BTreeMap<S, Vec<usize>> {
    &self.holes
  }

  /// Returns the per-hole label snapshots, parallel to
  /// [`skipped_regions()`](Self::skipped_regions) exactly as [`labels()`](Self::labels) is
  /// parallel to [`errors()`](Self::errors).
  #[inline(always)]
  pub fn skipped_region_labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    &self.hole_label_snapshots
  }

  /// Returns an iterator over every collected diagnostic — errors, warnings, **and** recovery
  /// holes — in true emission order.
  ///
  /// Each item is a borrowing [`Diagnostic`] view carrying the entry's span, its captured label
  /// snapshot, and its [`DiagnosticKind`] (the record kind plus payload). The order is the
  /// emission order recorded in the shared `log`, so a record of any kind appears in the exact
  /// position it was emitted — the interleaving the span-keyed maps cannot express on their own.
  /// This is the read-side bridge a downstream renderer (ariadne, miette, a bespoke reporter)
  /// consumes; tokora takes on no dependency on any of them.
  ///
  /// ```ignore
  /// // Sketch of an ariadne adapter (tokora does not depend on ariadne):
  /// use tokora::emitter::DiagnosticKind;
  /// for diag in emitter.diagnostics() {
  ///     let mut report = ariadne::Report::build(
  ///         match diag.severity() {
  ///             tokora::emitter::Severity::Error => ariadne::ReportKind::Error,
  ///             tokora::emitter::Severity::Warning => ariadne::ReportKind::Warning,
  ///         },
  ///         (),
  ///         diag.span().start(),
  ///     );
  ///     // Each open label is a "while parsing X" context note.
  ///     for ctx in diag.labels() {
  ///         report = report.with_note(format!("while parsing {ctx}"));
  ///     }
  ///     let report = match diag.kind() {
  ///         DiagnosticKind::Error(e) | DiagnosticKind::Warning(e) => report.with_message(e.to_string()),
  ///         DiagnosticKind::SkippedRegion(skipped) => {
  ///             report.with_message(format!("recovered by skipping {skipped} tokens"))
  ///         }
  ///     };
  ///     report.finish();
  /// }
  /// ```
  #[inline(always)]
  pub fn diagnostics(&self) -> Diagnostics<'_, S, Error> {
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

impl<'inp, L, S, Error, Lang: ?Sized> Emitter<'inp, L, Lang> for Verbose<Error, S, Lang>
where
  L: Lexer<'inp, Span = S, Offset = S::Offset>,
  Error: FromEmitterError<'inp, L, Lang>,
  S: Span + Ord + Clone,
{
  type Error = Error;

  #[inline(always)]
  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), Self::Error> {
    let (span, err) = err.into_components();
    let err = Error::from_lexer_error(Spanned::new(span.clone(), err));
    self.record(span, err);
    Ok(())
  }

  #[inline(always)]
  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error> {
    let (span, err) = err.into_components();
    self.record(span, err);
    Ok(())
  }

  /// Records the warning into the parallel warning channel (never fatal), capturing the same
  /// label snapshot the error paths capture. See [`emit_warning`](Emitter::emit_warning).
  #[inline(always)]
  fn emit_warning(&mut self, warning: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error> {
    let (span, warning) = warning.into_components();
    self.record_warning(span, warning);
    Ok(())
  }

  /// Records the recovery hole into the skipped-region channel (never fatal), on the shared
  /// emission log so a rewind unwinds it in order. See
  /// [`emit_skipped_region`](Emitter::emit_skipped_region) and
  /// [`skipped_regions`](Self::skipped_regions).
  #[inline(always)]
  fn emit_skipped_region(&mut self, span: L::Span, skipped: usize) -> Result<(), Self::Error> {
    self.record_hole(span, skipped);
    Ok(())
  }

  #[inline(always)]
  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    let span = err.span_ref().clone();
    self.record(span, Error::from_unexpected_token(err));
    Ok(())
  }

  #[inline(always)]
  fn checkpoint(&self) -> u64 {
    self.log.len() as u64
  }

  /// Rewind the error state to a checkpoint, emission-aware.
  ///
  /// `checkpoint` is the [`checkpoint`](Emitter::checkpoint) mark captured at the
  /// save point: the emission-log length at that instant. Every diagnostic
  /// recorded *after* it — exactly the emissions of the abandoned branch — is
  /// dropped, newest first, by popping the matching entry off its span's group;
  /// everything recorded before survives. The decision is purely by emission
  /// order, so a zero-width error emitted during a speculative branch is removed
  /// while an earlier zero-width error at the *same* offset is kept — a
  /// distinction the former span-end offset heuristic could not make. `cursor`
  /// is unused.
  #[inline(always)]
  fn rewind(&mut self, cursor: &Cursor<'inp, '_, L>, checkpoint: u64)
  where
    L: Lexer<'inp>,
  {
    let _ = cursor;
    let mark = (checkpoint as usize).min(self.log.len());
    while self.log.len() > mark {
      // Unwind newest-first: each span's `Vec` grows in emission order, so the matching entry
      // to drop is always its last one. The `Channel` tag names the maps it was recorded in,
      // so the pop lands in the right channel. The dropped entry takes its label snapshot with
      // it — labels captured into an entry are rewound together with it, and any later
      // re-emission re-derives labels from the then-current stack.
      let (channel, span) = self.log.pop().expect("log length exceeds the mark");
      match channel {
        Channel::Diagnostic(severity) => {
          let (groups, labels) = match severity {
            Severity::Error => (&mut self.errs, &mut self.label_snapshots),
            Severity::Warning => (&mut self.warns, &mut self.warn_label_snapshots),
          };
          pop_group(groups, &span);
          pop_group(labels, &span);
        }
        Channel::SkippedRegion => {
          pop_group(&mut self.holes, &span);
          pop_group(&mut self.hole_label_snapshots, &span);
        }
      }
    }
  }

  /// Releasing a kept checkpoint is a **deliberate no-op** for `Verbose`, and that is the
  /// reference posture for value-keyed emitters.
  ///
  /// `Verbose` keeps no per-checkpoint table to evict: its mark is nothing but the emission
  /// log's length ([`checkpoint`](Emitter::checkpoint) above), and its rollback state lives
  /// *in the emission values themselves* — [`rewind`](Emitter::rewind) pops log entries and
  /// their parallel-map groups by value, never consulting a mark-keyed row. A kept branch
  /// therefore leaves nothing behind that a release could reclaim; the default empty body is
  /// the whole implementation, spelled out here so the choice is legible rather than
  /// incidental. That property is exactly what makes `Verbose` a
  /// [`ValueKeyedEmitter`](crate::emitter::ValueKeyedEmitter), and therefore admissible inside
  /// the recording CST sink. An emitter that *does* key bookkeeping on marks overrides this to
  /// pop the kept row — see the advisory contract on [`release`](Emitter::release) for where such
  /// an emitter belongs (the input layer's own seam, where the settle discipline is 1:1, not
  /// inside a wrapper).
  #[inline(always)]
  fn release(&mut self, checkpoint: u64) {
    let _ = checkpoint;
  }

  /// Pushes a *"while parsing X"* label onto the open-label stack; the next
  /// recorded diagnostic snapshots it into the entry it emits.
  #[inline(always)]
  fn enter_label(&mut self, label: &'static str) {
    self.stack.push(label);
  }

  /// Pops the innermost open label as its [`labelled`](crate::labelled) scope closes.
  #[inline(always)]
  fn exit_label(&mut self) {
    self.stack.pop();
  }
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
