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
#[cfg(feature = "pratt")]
mod pratt;
mod separator;
mod store;
mod too_few;
mod too_many;
mod unclosed;
mod unexpected_leading_separator;
mod unexpected_trailing_separator;

use store::Store;

/// Which channel one emission-log entry was recorded in: a payload-carrying diagnostic (an
/// error or a warning, tagged with its [`Severity`]) or a payload-less skipped-region record
/// (a recovery hole: span + skipped-token count). One tag per log entry is what lets
/// [`rewind`](Emitter::rewind) pop each entry off the map it was recorded in, and what routes
/// the [`Diagnostics`] iterator to the right channel when the record kinds interleave.
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
///
/// # Where the storage lives
///
/// All of it sits behind a write-chokepointed private `store` submodule: the maps, the log
/// and the label stack are fields of a type declared in a *child* module, so Rust's own
/// privacy rule makes them unreachable from the per-channel emit files. Every emission
/// therefore has exactly one way in — one private `record` method per channel family — which
/// is what keeps the mark, the unwind and the replay above true for **every** channel rather
/// than for whichever ones remembered to log.
#[derive(Debug)]
pub struct Verbose<Error, S = SimpleSpan, Lang: ?Sized = ()> {
  store: Store<Error, S>,
  _lang: PhantomData<Lang>,
}

impl<Error, Span, Lang: ?Sized> Default for Verbose<Error, Span, Lang> {
  #[inline(always)]
  fn default() -> Self {
    Self::new()
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
      store: self.store.clone(),
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
      store: Store::new(),
      _lang: PhantomData,
    }
  }

  /// Records `err` in the **error** channel at `span` — the error channel's one write.
  ///
  /// This is the spelling the per-channel emit files call; the work is
  /// [`Store::record`](store::Store::record), which appends to the span's group, captures the
  /// open-label snapshot beside it, and logs the emission so a later
  /// [`rewind`](Emitter::rewind) can undo it precisely.
  #[inline(always)]
  fn record(&mut self, span: S, err: Error)
  where
    S: Ord + Clone,
  {
    self.store.record(span, err);
  }

  /// Records `warning` in the **warning** channel at `span` — the exact mirror of
  /// [`record`](Self::record). The shared log keeps both channels on one emission timeline, so
  /// a [`rewind`](Emitter::rewind) unwinds warnings and errors together in reverse emission
  /// order.
  #[inline(always)]
  fn record_warning(&mut self, span: S, warning: Error)
  where
    S: Ord + Clone,
  {
    self.store.record_warning(span, warning);
  }

  /// Records a recovery hole in the **skipped-region** channel at `span` — the same shape as
  /// [`record_warning`](Self::record_warning), but the payload is the skipped-token count.
  #[inline(always)]
  fn record_hole(&mut self, span: S, skipped: usize)
  where
    S: Ord + Clone,
  {
    self.store.record_hole(span, skipped);
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
  pub const fn errors(&self) -> &BTreeMap<S, Vec<Error>> {
    self.store.errors()
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
  pub const fn labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    self.store.labels()
  }

  /// Returns a reference to all collected **warnings**, parallel to [`errors()`](Self::errors).
  ///
  /// Warnings are the [`Severity::Warning`] tier, recorded via
  /// [`emit_warning`](Emitter::emit_warning). The map has the same span-keyed,
  /// group-per-span shape as [`errors()`](Self::errors); the two channels are independent, so a
  /// span may carry warnings, errors, or both.
  #[inline(always)]
  pub const fn warnings(&self) -> &BTreeMap<S, Vec<Error>> {
    self.store.warnings()
  }

  /// Returns the per-warning label snapshots, parallel to [`warnings()`](Self::warnings)
  /// exactly as [`labels()`](Self::labels) is parallel to [`errors()`](Self::errors).
  #[inline(always)]
  pub const fn warning_labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    self.store.warning_labels()
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
  pub const fn skipped_regions(&self) -> &BTreeMap<S, Vec<usize>> {
    self.store.skipped_regions()
  }

  /// Returns the per-hole label snapshots, parallel to
  /// [`skipped_regions()`](Self::skipped_regions) exactly as [`labels()`](Self::labels) is
  /// parallel to [`errors()`](Self::errors).
  #[inline(always)]
  pub const fn skipped_region_labels(&self) -> &BTreeMap<S, Vec<Vec<&'static str>>> {
    self.store.skipped_region_labels()
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
  /// The iterator is exact-size and fused: each log entry carries the slot its record took in
  /// its span's group at emit time, so the walk needs no per-span bookkeeping and its
  /// `len()`/`size_hint()` are the true remaining count at every step. Building one allocates
  /// nothing, so a renderer can size its own buffer up front.
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
    self.store.diagnostics()
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
  fn checkpoint(&mut self) -> u64 {
    self.store.mark()
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
    self.store.rewind_to(checkpoint);
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
    self.store.enter_label(label);
  }

  /// Pops the innermost open label as its [`labelled`](crate::labelled) scope closes.
  #[inline(always)]
  fn exit_label(&mut self) {
    self.store.exit_label();
  }
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod record_census;

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
