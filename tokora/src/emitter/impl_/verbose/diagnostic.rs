//! The read-side rendering bridge for [`Verbose`](super::Verbose): a borrowing
//! [`Diagnostic`] view, its [`DiagnosticKind`] payload tag, and the [`Diagnostics`] iterator
//! that replays every recorded channel — errors, warnings, and recovery holes — in emission
//! order.
//!
//! These types are deliberately renderer-agnostic — they borrow straight out of the emitter's
//! storage and carry no formatting policy — so a downstream adapter (ariadne, miette, a bespoke
//! reporter) can map each entry onto its own report kind without tokora taking on any dependency.

use core::iter::FusedIterator;
use std::{collections::BTreeMap, vec::Vec};

use derive_more::{IsVariant, TryUnwrap, Unwrap};

use super::{Channel, store::LogEntry};
use crate::emitter::Severity;

/// What one collected [`Diagnostic`] carries — the record kind and its payload.
///
/// The two payload channels carry a borrowed error value ([`Error`](Self::Error) /
/// [`Warning`](Self::Warning)); the recovery-hole channel carries only the skipped-token count
/// ([`SkippedRegion`](Self::SkippedRegion)), since a hole has no error value. The span and the
/// captured label snapshot are common to every kind and live on the outer [`Diagnostic`] rather
/// than being duplicated into each variant.
#[derive(Debug, IsVariant, Unwrap, TryUnwrap)]
pub enum DiagnosticKind<'a, E> {
  /// A hard error payload — the [`Severity::Error`] channel.
  Error(&'a E),
  /// A soft warning payload — the [`Severity::Warning`] channel.
  Warning(&'a E),
  /// A recovery hole recorded by [`emit_skipped_region`](crate::Emitter::emit_skipped_region):
  /// the count of tokens a balanced skip discarded. Payload-less — the span it covers is on the
  /// outer [`Diagnostic`].
  SkippedRegion(usize),
}

// Hand-written `Copy`/`Clone` (not derived): the variants hold only `&'a E` and `usize`, both of
// which are `Copy` for *every* `E`, so the type is unconditionally `Copy`. `#[derive(Copy,
// Clone)]` would instead emit a spurious `E: Copy`/`E: Clone` bound (the classic derive-adds-a-
// bound-it-does-not-need trap), which would in turn force that bound onto [`Diagnostic::kind`].
impl<E> Clone for DiagnosticKind<'_, E> {
  #[inline(always)]
  fn clone(&self) -> Self {
    *self
  }
}

impl<E> Copy for DiagnosticKind<'_, E> {}

/// A borrowing, read-side view of a single collected diagnostic.
///
/// Yielded by [`Verbose::diagnostics`](super::Verbose::diagnostics), a `Diagnostic` bundles the
/// facts a renderer needs about one entry — its source [`span`](Self::span), the *"while parsing
/// X"* [`labels`](Self::labels) that were open when it was emitted, and its
/// [`kind`](Self::kind) (the record kind plus payload) — all as borrows into the emitter, so
/// building the view allocates nothing. [`severity`](Self::severity) and
/// [`payload`](Self::payload) are convenience projections of [`kind`](Self::kind).
#[derive(Debug)]
pub struct Diagnostic<'a, S, E> {
  span: &'a S,
  labels: &'a [&'static str],
  kind: DiagnosticKind<'a, E>,
}

// Hand-written `Copy`/`Clone` for the same reason as [`DiagnosticKind`]: every field is `Copy`
// for all `S`/`E` (`&S`, `&[..]`, and the `Copy` `DiagnosticKind`), so deriving would attach
// spurious `S: Copy`/`E: Copy` bounds.
impl<S, E> Clone for Diagnostic<'_, S, E> {
  #[inline(always)]
  fn clone(&self) -> Self {
    *self
  }
}

impl<S, E> Copy for Diagnostic<'_, S, E> {}

impl<'a, S, E> Diagnostic<'a, S, E> {
  /// Bundles the borrowed facts of one collected diagnostic.
  #[inline(always)]
  pub(crate) const fn new(
    span: &'a S,
    labels: &'a [&'static str],
    kind: DiagnosticKind<'a, E>,
  ) -> Self {
    Self { span, labels, kind }
  }

  /// The source span of this diagnostic.
  #[inline(always)]
  pub const fn span(&self) -> &'a S {
    self.span
  }

  /// The open-label snapshot captured when this diagnostic was emitted, outermost
  /// [`labelled`](crate::labelled) context first. Empty when the emission was unlabelled.
  #[inline(always)]
  pub const fn labels(&self) -> &'a [&'static str] {
    self.labels
  }

  /// The record kind and payload — the primary way to tell an error, a warning, and a recovery
  /// hole apart and read each one's data.
  #[inline(always)]
  pub const fn kind(&self) -> DiagnosticKind<'a, E> {
    self.kind
  }

  /// The [`Severity`] tier of this diagnostic.
  ///
  /// An error reports [`Severity::Error`]; a warning reports [`Severity::Warning`]. A
  /// [`SkippedRegion`](DiagnosticKind::SkippedRegion) is a soft, non-fatal recovery event and so
  /// also reports [`Severity::Warning`] — use [`kind`](Self::kind) to distinguish a hole from a
  /// genuine warning payload.
  #[inline(always)]
  pub const fn severity(&self) -> Severity {
    match self.kind {
      DiagnosticKind::Error(_) => Severity::Error,
      DiagnosticKind::Warning(_) | DiagnosticKind::SkippedRegion(_) => Severity::Warning,
    }
  }

  /// The collected error/warning payload, or `None` for a
  /// [`SkippedRegion`](DiagnosticKind::SkippedRegion) hole (which carries a skipped-token count,
  /// not an error value — read it via [`kind`](Self::kind)).
  #[inline(always)]
  pub const fn payload(&self) -> Option<&'a E> {
    match self.kind {
      DiagnosticKind::Error(e) | DiagnosticKind::Warning(e) => Some(e),
      DiagnosticKind::SkippedRegion(_) => None,
    }
  }
}

/// An iterator over every collected diagnostic of a [`Verbose`](super::Verbose) emitter — the
/// error and warning channels **and** the recovery-hole channel — in true emission order.
///
/// Constructed by [`Verbose::diagnostics`](super::Verbose::diagnostics). It walks the emitter's
/// shared emission `log`; each log entry names a channel, a span, and the slot the record took
/// in that span's group when it was emitted, so the iterator hands back the matching payload and
/// label snapshot by direct index. The result is the three span-keyed maps *interleaved* on one
/// timeline — the ordering a renderer wants and that no single map can express alone.
///
/// Because the walk is one log entry per step with no per-span bookkeeping, the iterator is
/// **exact-size**: [`len`](ExactSizeIterator::len) and
/// [`size_hint`](Iterator::size_hint) report the true number of remaining diagnostics at every
/// point, including after partial consumption, and it is [`FusedIterator`] — once exhausted it
/// keeps returning `None`. Building one allocates nothing.
#[derive(Debug)]
pub struct Diagnostics<'a, S, E> {
  log: &'a [LogEntry<S>],
  errs: &'a BTreeMap<S, Vec<E>>,
  err_labels: &'a BTreeMap<S, Vec<Vec<&'static str>>>,
  warns: &'a BTreeMap<S, Vec<E>>,
  warn_labels: &'a BTreeMap<S, Vec<Vec<&'static str>>>,
  holes: &'a BTreeMap<S, Vec<usize>>,
  hole_labels: &'a BTreeMap<S, Vec<Vec<&'static str>>>,
  index: usize,
}

impl<'a, S, E> Diagnostics<'a, S, E> {
  /// Builds the iterator from the emitter's channels and shared log.
  ///
  /// Scoped to the `verbose` subtree: the sole caller is the store's own
  /// `diagnostics()`, and the parameters name the store's internal log
  /// representation, which no wider visibility should be able to spell.
  #[inline(always)]
  #[allow(clippy::too_many_arguments)]
  pub(super) const fn new(
    log: &'a [LogEntry<S>],
    errs: &'a BTreeMap<S, Vec<E>>,
    err_labels: &'a BTreeMap<S, Vec<Vec<&'static str>>>,
    warns: &'a BTreeMap<S, Vec<E>>,
    warn_labels: &'a BTreeMap<S, Vec<Vec<&'static str>>>,
    holes: &'a BTreeMap<S, Vec<usize>>,
    hole_labels: &'a BTreeMap<S, Vec<Vec<&'static str>>>,
  ) -> Self {
    Self {
      log,
      errs,
      err_labels,
      warns,
      warn_labels,
      holes,
      hole_labels,
      index: 0,
    }
  }
}

impl<'a, S, E> Iterator for Diagnostics<'a, S, E>
where
  S: Ord,
{
  type Item = Diagnostic<'a, S, E>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    let LogEntry {
      channel,
      span,
      slot,
    } = self.log.get(self.index)?;
    self.index += 1;
    let slot = *slot;

    // The `Channel` tag routes to the maps this entry was recorded in, and the slot recorded
    // at emit time indexes straight into this span's group — no per-walk cursor state, so the
    // three timelines interleave exactly as they were emitted for the cost of one lookup.
    match *channel {
      Channel::Diagnostic(severity) => {
        let (groups, labels) = match severity {
          Severity::Error => (self.errs, self.err_labels),
          Severity::Warning => (self.warns, self.warn_labels),
        };
        let payload = &groups[span][slot];
        let labels = labels[span][slot].as_slice();
        let kind = match severity {
          Severity::Error => DiagnosticKind::Error(payload),
          Severity::Warning => DiagnosticKind::Warning(payload),
        };
        Some(Diagnostic::new(span, labels, kind))
      }
      Channel::SkippedRegion => {
        let skipped = self.holes[span][slot];
        let labels = self.hole_labels[span][slot].as_slice();
        Some(Diagnostic::new(
          span,
          labels,
          DiagnosticKind::SkippedRegion(skipped),
        ))
      }
    }
  }

  /// Exact on every step: one log entry is one diagnostic, and `index` never passes the log's
  /// length.
  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    let rest = self.log.len() - self.index;
    (rest, Some(rest))
  }
}

impl<S, E> ExactSizeIterator for Diagnostics<'_, S, E> where S: Ord {}

impl<S, E> FusedIterator for Diagnostics<'_, S, E> where S: Ord {}
