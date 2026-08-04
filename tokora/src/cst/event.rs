//! The event vocabulary of the rewindable CST channel: the flat log a
//! [`CstEmitter`] records into, and the branded marks that make
//! retro-parenting safe under backtracking.
//!
//! # The two verbs
//!
//! An event buffer changes in exactly two ways: **append** (the `cst_*` emission methods) and
//! **suffix-truncate** (an emitter [`rewind`](crate::emitter::Emitter::rewind)). No operation
//! rewrites the *kind* of an interior slot — retro-parenting is expressed by **appending** a
//! `StartAt` that names an earlier tombstone, never by completing the
//! tombstone in place. This is what makes rewind-by-truncation exact: the prefix below any
//! live mark is immutable, so truncating to the mark restores the buffer to precisely the
//! state it had when the mark was captured. (The one journaled exception is the
//! `forward_parent` *acceleration* field a `StartAt` writes back onto its
//! target tombstone; the sink's undo journal reverse-replays those writes on rewind, so the
//! law holds observationally — see `Sink` under the `rowan` feature.)
//!
//! # The depth model
//!
//! Balance is *derived* from the buffer, never cached beside it:
//!
//! - `StartNode` with a real kind opens a node: **+1**;
//! - `StartNode` with the [`TOMBSTONE`] kind is an inert mark: **0**;
//! - `StartAt` opens a retro-wrap (hoisted to its target at
//!   materialization): **+1** at its own index;
//! - `FinishNode` closes the innermost open node: **−1**;
//! - `Token` and `Diag` are neutral: **0**.
//!
//! Both verbs preserve "every prefix has depth ≥ 0 or is diagnosed at materialization": a
//! malformed buffer is representable (the raw `cst_*` surface is sharp), but it is
//! *unrepresentable as a successful materialization* — `finish` walks the log and returns a
//! typed error instead of building a wrong tree.
//!
//! # Marks carry eras
//!
//! Truncate-and-regrow is the normal backtracking rhythm, so "the index is in bounds" says
//! nothing about whether the event at that index is still the one a mark was issued for. An
//! [`EventMark`] therefore pairs its buffer index with the **era** of the truncation history
//! at issue time — and with the **identity** of the issuing sink, because eras are per-sink
//! histories and two sinks' `(index, era)` pairs coincide trivially. The recording sink keeps
//! a monotone era source and a truncation ledger (`TruncationLedger`) that restore never
//! rewinds, and validates every spend. A stale or foreign mark **panics in every build** —
//! the savepoint posture: both are parser bugs, not input-dependent conditions, and the
//! silent alternative is a wrong tree with no witness.

#[cfg(feature = "rowan")]
use core::num::NonZeroU32;

use crate::{Lexer, emitter::CstEmitter};

/// The reserved *initial* kind of a `StartNode` appended by
/// [`cst_mark`](crate::emitter::CstEmitter::cst_mark): an inert placeholder that materializes
/// into nothing unless a later `StartAt` names it as a retro-wrap target.
///
/// The value is `u16::MAX`, and the slot is **reserved across the whole shared kind space**:
/// a dialect's unified kind enum (node kinds and token images alike) must never map anything
/// to it. The recording sink asserts the reservation at emission time — unconditionally, in
/// every build — and rejects it with a typed error at materialization.
pub const TOMBSTONE: u16 = u16::MAX;

/// One entry of the flat CST event log.
///
/// This is the buffer format of the `rowan`-gated `Sink` — a crate-internal type by the
/// second-consumer rule: the *vocabulary* is normative (documented here and in the module
/// docs), the *type* is not public API until a consumer outside the sink exists. It is
/// compiled exactly where its one consumer lives (the `rowan` feature); the public half of
/// the vocabulary — [`TOMBSTONE`], [`EventMark`], the [`Marker`] typestate, and the
/// [`CstEmitter`] transport — is unconditional.
///
/// The `S` parameter is the lexer's span type; only `Token` carries one.
#[cfg(feature = "rowan")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event<S> {
  /// Opens a node of `kind`, closed by the matching [`FinishNode`](Event::FinishNode) —
  /// unless `kind` is [`TOMBSTONE`], in which case the slot is an inert mark (see
  /// [`cst_mark`](crate::emitter::CstEmitter::cst_mark)) that pairs with no finish.
  ///
  /// `forward_parent` is a journaled link written by
  /// [`cst_start_at`](crate::emitter::CstEmitter::cst_start_at) onto its **target**
  /// tombstone: the relative forward offset of the newest [`StartAt`](Event::StartAt)
  /// naming this slot, and the **head of that target's wrap chain** —
  /// [`StartAt::prev`](Event::StartAt) carries the rest. Materialization follows the chain
  /// instead of rebuilding a keyed index of every wrap, so the pointer is load-bearing and
  /// must stay *consistent*: the sink's undo journal restores overwritten values on
  /// rewind, and `finish` validates both directions — a set pointer must still name a live
  /// `StartAt` of this target, and every `StartAt` must be reachable from its target's
  /// chain (the dangling `forward_parent` of an abandoned branch, and the unreachable wrap
  /// that would otherwise be structurally dropped, are the two silent corruptions the
  /// journal and the reachability check exist to kill).
  StartNode {
    /// The node kind, or [`TOMBSTONE`] while the slot is an inert mark.
    kind: u16,
    /// Relative forward offset to the newest [`StartAt`](Event::StartAt) targeting this
    /// tombstone, if any — the head of its wrap chain. Maintained under the sink's undo
    /// journal.
    forward_parent: Option<NonZeroU32>,
  },
  /// One committed token: its mapped kind and its source span. Appended exactly once per
  /// committed token, at the settle; peeks, declines, and unconsumed stoppers append
  /// nothing.
  Token {
    /// The dialect-mapped token kind (the sink-side mapper's output).
    kind: u16,
    /// The token's source span; materialization slices text from the source by it.
    span: S,
  },
  /// Closes the innermost open node (stack discipline), carrying the kind of the node the
  /// emitter *intended* to close.
  ///
  /// The kind is the leaked-finish detector: materialization compares it against the frame
  /// the finish actually lands on and refuses a mismatch
  /// ([`FinishError::MismatchedFinish`]), which is the one signal that separates a legal
  /// cross-checkpoint close from a finish whose start was rolled back out from under it —
  /// two histories that leave byte-identical event buffers.
  ///
  /// [`FinishError::MismatchedFinish`]: crate::cst::FinishError::MismatchedFinish
  FinishNode {
    /// The kind of the node this finish intends to close.
    kind: u16,
  },
  /// Retro-opens a node of `kind` at the buffer position of the tombstone at `target` —
  /// the **append-only** form of retro-parenting. Same-target `StartAt`s open in reverse
  /// buffer order at materialization (the later wrap is the outer node, because its
  /// finish is necessarily appended later). The in-place alternative (rewriting the
  /// tombstone's kind) is banned by law: an interior write below a live emitter mark
  /// survives the truncation that was supposed to erase the branch that made it.
  StartAt {
    /// The node kind of the retro-wrap.
    kind: u16,
    /// The absolute buffer index of the target tombstone; always `<` this event's own
    /// index, validated at emission (era-checked) and again at materialization.
    target: u64,
    /// The chain link: the `forward_parent` value this `StartAt` displaced on its target
    /// — i.e. the relative forward offset of the **previous newest** `StartAt` naming the
    /// same tombstone, or `None` when this is the target's first wrap.
    ///
    /// Written once, at this event's own append, from the value
    /// [`cst_start_at`](crate::emitter::CstEmitter::cst_start_at) already reads to journal
    /// (the append-only law holds: the field is never revisited). Materialization walks
    /// `tombstone.forward_parent → newest StartAt → prev → …` to recover a target's wrap
    /// list newest-first, which is the order the wraps open in — so the chain replaces the
    /// per-materialization keyed index that used to be rebuilt from scratch.
    ///
    /// Offsets are strictly decreasing along a chain (an older `StartAt` sits at a smaller
    /// index), which is what makes the walk terminating and fork-free. A wrap whose
    /// relative offset does not fit `u32` cannot be linked — the same limit the
    /// `forward_parent` acceleration has always carried — and materialization then
    /// *refuses* the stream ([`FinishError::DanglingForwardParent`]) rather than dropping
    /// the node; the buffer size that requires is far past any reachable event log.
    ///
    /// [`FinishError::DanglingForwardParent`]: crate::cst::FinishError::DanglingForwardParent
    prev: Option<NonZeroU32>,
  },
  /// A forwarded-diagnostic slot: a marker in the event log for one diagnostic forwarded to
  /// the wrapped emitter (on `Ok` and `Err` alike). Skipped at materialization — except that
  /// a **lexer-error** slot carries the offending source span in `error_span`, so `finish`
  /// can tell a byte a lexer legitimately refused (a covered gap, tile-able) from a byte a
  /// dropped `commit_token` lost (an unexplained gap, refused). Living in the event log means
  /// the span rewinds with the branch that saw it — an abandoned lexer error stops covering
  /// anything, for free.
  ///
  /// The inner emitter's rewind target is **not** kept here: it rides on the sink's
  /// mark-stack row, captured at [`checkpoint`](crate::emitter::Emitter::checkpoint) and
  /// handed back at [`rewind`](crate::emitter::Emitter::rewind), so one positional mark still
  /// governs both the event log and the diagnostic log.
  Diag {
    /// The source span of a **lexer error** that committed no token, or `None` for any
    /// other forwarded diagnostic. Only a lexer error names untokenized bytes; parser
    /// diagnostics (unexpected token, missing element, …) point at tokens that *did*
    /// settle or at zero-width absences, so they cover no gap and set `None`.
    error_span: Option<S>,
  },
}

#[cfg(feature = "rowan")]
impl<S> Event<S> {
  /// This event's contribution to the derived open-node depth (the module-level depth
  /// model): `+1` for a real [`StartNode`](Self::StartNode) or a [`StartAt`](Self::StartAt),
  /// `-1` for a [`FinishNode`](Self::FinishNode), `0` for everything else (tombstones
  /// included).
  #[inline(always)]
  pub(crate) const fn depth_delta(&self) -> i64 {
    match self {
      Self::StartNode { kind, .. } => {
        if *kind == TOMBSTONE {
          0
        } else {
          1
        }
      }
      Self::StartAt { .. } => 1,
      Self::FinishNode { .. } => -1,
      Self::Token { .. } | Self::Diag { .. } => 0,
    }
  }

  /// Whether this event is a live tombstone: a [`StartNode`](Self::StartNode) still
  /// carrying the [`TOMBSTONE`] kind. The positional half of [`EventMark`] validation.
  #[inline(always)]
  pub(crate) const fn is_tombstone(&self) -> bool {
    matches!(
      self,
      Self::StartNode {
        kind: TOMBSTONE,
        ..
      }
    )
  }
}

/// A validated handle to a tombstone appended by
/// [`cst_mark`](crate::emitter::CstEmitter::cst_mark): the anchor a later
/// [`cst_start_at`](crate::emitter::CstEmitter::cst_start_at) retro-wraps from.
///
/// A mark is a **positional witness plus identity**: `index` names the tombstone's buffer
/// slot, `era` names the truncation history it was issued under, and `sink` names the one
/// recording sink that minted it. Index-in-bounds is *not* validity — a rewind can truncate
/// the tombstone away and unrelated events can regrow over the same index — and `(index,
/// era)` is *not* identity — two fresh sinks both mint `(0, 0)` — so the recording sink
/// checks all three at every spend and **panics in every build** on a stale *or foreign*
/// mark (the savepoint posture; see the [module docs](self)).
///
/// Marks are freely `Copy` and may legitimately outlive combinator frames (a pratt driver
/// holds one across arbitrarily many operator iterations, spending it once per fold). A
/// rewind *below* a live mark is legal — the wrap intent simply dies with the branch that
/// conceived it, and any later spend of the now-stale mark is the panic above. For the
/// single-use open → completed → abandoned discipline, wrap a mark in a [`Marker`].
///
/// Emitters without an event channel (the diagnostics-only implementations that opt into
/// [`CstEmitter`] via the defaulted no-ops) return an **inert**
/// mark; spending an inert mark on a recording sink panics deterministically (its reserved
/// witness id names no sink).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventMark {
  /// The buffer index of the tombstone this mark names.
  index: u64,
  /// The truncation era the mark was issued under (see `TruncationLedger`).
  era: u64,
  /// The identity of the recording sink that issued this mark, validated at every spend
  /// **in every build**: two sinks' `(index, era)` pairs coincide trivially — two fresh
  /// sinks both mint `(0, 0)` — so without the witness a foreign mark whose slot happens
  /// to hold a live tombstone would validate and wrap an unrelated history. That is the
  /// wrong-tree class the always-panic contract exists to kill, so the witness is carried
  /// unconditionally (the `SavepointId` nonce posture, not the debug-only input witness).
  sink: usize,
}

impl EventMark {
  /// The reserved witness id of an [`inert`](Self::inert) mark (no recording sink).
  /// Recording sinks mint their witnesses from `1`, so an inert mark can never pass a
  /// sink's identity check.
  pub(crate) const INERT_SINK: usize = 0;

  /// Creates a mark naming the tombstone at `index`, issued under `era` by the sink
  /// witnessed as `sink`. Crate-private: only a recording sink mints live marks.
  #[cfg(feature = "rowan")]
  #[inline(always)]
  pub(crate) const fn new(index: u64, era: u64, sink: usize) -> Self {
    Self { index, era, sink }
  }

  /// The inert mark returned by the defaulted
  /// [`cst_mark`](crate::emitter::CstEmitter::cst_mark) of an emitter with no event
  /// channel. Its witness is the reserved [`INERT_SINK`](Self::INERT_SINK) id (which no
  /// recording sink ever carries) and its index is `u64::MAX` (which no buffer reaches),
  /// so a recording sink that is handed one panics at the identity wall rather than
  /// wrapping anything.
  #[inline(always)]
  pub(crate) const fn inert() -> Self {
    Self {
      index: u64::MAX,
      era: u64::MAX,
      sink: Self::INERT_SINK,
    }
  }

  /// The buffer index of the tombstone this mark names.
  #[inline(always)]
  pub const fn index(&self) -> u64 {
    self.index
  }

  /// The truncation era this mark was issued under.
  #[inline(always)]
  pub const fn era(&self) -> u64 {
    self.era
  }

  /// The issuing sink's witness id (validated at every spend, in every build).
  #[cfg(feature = "rowan")]
  #[inline(always)]
  pub(crate) const fn sink(&self) -> usize {
    self.sink
  }
}

/// The truncation history of one event buffer: the monotone era source and the ledger of
/// suffix-truncations, which together decide whether an [`EventMark`] issued in the past
/// still names the tombstone it was minted for.
///
/// # Cell classes (CELL_CENSUS discipline)
///
/// Both cells are **monotone, never rewound**:
///
/// - the **era source** is the id-source class (`next_ckp_id` / `savepoint_seq` precedent):
///   rewinding it could reissue an era and let a dead mark validate;
/// - the **ledger** is a monotone *witness of truncations*: restore appends to it (a rewind
///   IS a truncation) and never removes from it — forgetting a truncation would false-accept
///   exactly the stale mark the record existed to kill.
///
/// The ledger stays small: a new truncation subsumes every recorded truncation at an equal
/// or higher low-water mark (any mark the older entry would invalidate, the newer one also
/// invalidates), so entries are merged on push and the stack is strictly increasing in both
/// era and low-water mark.
#[cfg(feature = "rowan")]
#[derive(Debug, Clone, Default)]
pub(crate) struct TruncationLedger {
  /// The monotone era source: the era stamped into marks issued *now*. Bumped by every
  /// recorded truncation, never rewound.
  era: u64,
  /// The merged truncation stack: `(era_after, low)` — after the bump to `era_after`, the
  /// buffer was truncated to length `low`. Strictly increasing in `era_after` (append
  /// order) and in `low` (the merge invariant).
  entries: std::vec::Vec<(u64, u64)>,
}

#[cfg(feature = "rowan")]
impl TruncationLedger {
  /// A fresh history: era 0, no truncations.
  #[inline(always)]
  pub(crate) const fn new() -> Self {
    Self {
      era: 0,
      entries: std::vec::Vec::new(),
    }
  }

  /// The era to stamp into marks issued now.
  #[inline(always)]
  pub(crate) const fn era(&self) -> u64 {
    self.era
  }

  /// Records a suffix-truncation of the buffer to length `low`: bumps the era and pushes
  /// the merged `(era, low)` entry. Entries whose low-water mark is `>= low` are subsumed
  /// (every index they invalidate for older marks, this entry invalidates too) and popped,
  /// keeping the stack strictly increasing in `low`.
  pub(crate) fn record_truncation(&mut self, low: u64) {
    self.era += 1;
    while matches!(self.entries.last(), Some((_, l)) if *l >= low) {
      self.entries.pop();
    }
    self.entries.push((self.era, low));
  }

  /// Whether a mark issued under `mark_era` for buffer index `index` has been invalidated
  /// by a later truncation: true iff some truncation younger than the mark reached the
  /// mark's index or below.
  ///
  /// The stack is strictly increasing in both fields, so the younger-than-`mark_era`
  /// entries are a suffix and the smallest low-water mark among them is the suffix's first
  /// entry: one binary search decides.
  pub(crate) fn is_stale(&self, mark_era: u64, index: u64) -> bool {
    let split = self.entries.partition_point(|(e, _)| *e <= mark_era);
    match self.entries.get(split) {
      Some((_, low)) => *low <= index,
      None => false,
    }
  }
}

/// An **open** retro-wrap intent: a single-use, typestate wrapper around an [`EventMark`]
/// enforcing the open → completed | abandoned lifecycle at compile time.
///
/// The raw mark is `Copy` and multi-spend (the pratt shape needs that); `Marker` is the
/// discipline for the common single-wrap case — the rust-analyzer `Marker` under tokora's
/// append-only encoding:
///
/// - [`complete`](Self::complete) spends the marker into a node of a decided kind
///   (append-only: a `StartAt` naming the tombstone plus its finish) and returns a
///   [`CompletedMarker`];
/// - [`abandon`](Self::abandon) consumes the marker; the tombstone stays in the buffer,
///   inert, and materializes into nothing;
/// - **`precede` exists only on [`CompletedMarker`]** — wrapping an abandoned or still-open
///   intent is unrepresentable, not merely checked.
///
/// ```ignore
/// let m = Marker::new(inp.emitter().cst_mark());
/// let name = ident(inp)?;                       // tokens under the mark
/// match try_colon(inp)? {
///   Accepted(_) => {
///     let alias = m.complete(inp.emitter(), K_ALIAS); // Alias[Ident, Colon]
///     let _ = alias.precede();                        // a further outer wrap, if wanted
///   }
///   Declined => m.abandon(),                          // tombstone stays inert
/// }
/// ```
///
/// # Misuse is a compile error
///
/// A marker cannot be spent twice — both verbs take `self`:
///
/// ```compile_fail,E0382
/// use tokora::cst::event::Marker;
///
/// fn misuse(m: Marker) {
///     m.abandon();
///     m.abandon(); // ERROR: use of moved value
/// }
/// ```
///
/// Preceding requires completion — an open (or abandoned) marker has no `precede`:
///
/// ```compile_fail,E0599
/// use tokora::cst::event::Marker;
///
/// fn misuse(m: Marker) {
///     let _ = m.precede(); // ERROR: no method `precede` on `Marker` — complete it first
/// }
/// ```
///
/// And a completed node cannot be un-decided — [`CompletedMarker`] has no `abandon`:
///
/// ```compile_fail,E0599
/// use tokora::cst::event::CompletedMarker;
///
/// fn misuse(c: CompletedMarker) {
///     c.abandon(); // ERROR: no method `abandon` on `CompletedMarker`
/// }
/// ```
#[derive(Debug)]
#[must_use = "an open marker must be completed or abandoned; dropping it silently leaves an inert tombstone"]
pub struct Marker {
  mark: EventMark,
}

impl Marker {
  /// Wraps a freshly minted mark (from
  /// [`cst_mark`](crate::emitter::CstEmitter::cst_mark)) in the single-use typestate.
  #[inline(always)]
  pub const fn new(mark: EventMark) -> Self {
    Self { mark }
  }

  /// The underlying mark.
  #[inline(always)]
  pub const fn mark(&self) -> EventMark {
    self.mark
  }

  /// Completes the intent as a node of `kind`: appends the retro-wrap (`StartAt` at this
  /// marker's tombstone) and its finish, and moves to the completed state. The node spans
  /// everything recorded since the mark.
  ///
  /// Stale-mark spends panic in every build, at the emitter (see [`EventMark`]).
  #[inline(always)]
  pub fn complete<'a, L, Lang, E>(self, emitter: &mut E, kind: u16) -> CompletedMarker
  where
    L: Lexer<'a>,
    Lang: ?Sized,
    E: CstEmitter<'a, L, Lang> + ?Sized,
  {
    emitter.cst_start_at(self.mark, kind);
    emitter.cst_finish(kind);
    CompletedMarker { mark: self.mark }
  }

  /// Abandons the intent: no node is created, the tombstone stays in the buffer inert and
  /// materializes into nothing. Consumes the marker, so a later `precede`-shaped wrap of
  /// the abandoned region is unrepresentable.
  #[inline(always)]
  pub fn abandon(self) {}
}

/// A **completed** retro-wrap: the witness that a [`Marker`] was decided into a real node,
/// and the only state from which a further outer wrap may grow.
///
/// [`precede`](Self::precede) returns a fresh open [`Marker`] at the *same* tombstone:
/// completing it appends a later same-target `StartAt`, which materializes as the **outer**
/// node (same-target wraps open in reverse buffer order — the later wrap's finish is
/// necessarily appended later). `precede` borrows rather than consumes, because multiple
/// wraps of one completed region are legal — each precede-complete pair adds one more layer.
#[derive(Debug)]
pub struct CompletedMarker {
  mark: EventMark,
}

impl CompletedMarker {
  /// The underlying mark.
  #[inline(always)]
  pub const fn mark(&self) -> EventMark {
    self.mark
  }

  /// Opens a fresh wrap intent **around** this completed node (and everything recorded
  /// since its mark): the returned [`Marker`], when completed, becomes the outer node.
  #[inline(always)]
  pub const fn precede(&self) -> Marker {
    Marker { mark: self.mark }
  }
}

#[cfg(all(test, feature = "rowan"))]
mod tests {
  use super::*;

  /// The F-A5 core, at the vocabulary level: a truncation at or below a mark's index,
  /// recorded after the mark's era, makes the mark stale; a truncation strictly above it
  /// does not; and marks issued after a truncation are untouched by it.
  #[test]
  fn ledger_staleness_is_truncate_at_or_below_after_issue() {
    let mut ledger = TruncationLedger::new();
    let mark_era = ledger.era();

    // No truncations yet: nothing is stale.
    assert!(!ledger.is_stale(mark_era, 3));

    // Truncation strictly above the mark's index leaves it live.
    ledger.record_truncation(4);
    assert!(!ledger.is_stale(mark_era, 3));

    // Truncation reaching the index kills it — the tombstone at 3 was dropped, whatever
    // regrew at index 3 later is not it.
    ledger.record_truncation(3);
    assert!(ledger.is_stale(mark_era, 3));

    // A mark issued NOW (after both truncations) is untouched by them.
    let fresh_era = ledger.era();
    assert!(!ledger.is_stale(fresh_era, 3));
  }

  /// The merge invariant: a deeper truncation subsumes shallower ones recorded before it,
  /// so the stack stays strictly increasing in `low` — and staleness answers stay exact
  /// across the merge.
  #[test]
  fn ledger_merges_subsumed_truncations() {
    let mut ledger = TruncationLedger::new();
    let old = ledger.era();
    ledger.record_truncation(10);
    ledger.record_truncation(7);
    ledger.record_truncation(3);
    // All three collapse to the deepest record; answers are unchanged by the merge.
    assert_eq!(ledger.entries.len(), 1);
    assert!(ledger.is_stale(old, 3));
    assert!(ledger.is_stale(old, 9));
    assert!(ledger.is_stale(old, 100));
    assert!(!ledger.is_stale(old, 2));

    // Regrow-then-truncate-shallow keeps both records: they invalidate different ranges.
    let mid = ledger.era();
    ledger.record_truncation(8);
    assert_eq!(ledger.entries.len(), 2);
    assert!(ledger.is_stale(mid, 8), "the new truncation reaches 8");
    assert!(!ledger.is_stale(mid, 7), "below the new low-water mark");
    assert!(
      ledger.is_stale(old, 5),
      "the old, deeper truncation still applies"
    );
  }

  /// The era source is monotone and bumps exactly once per recorded truncation.
  #[test]
  fn era_source_is_monotone_per_truncation() {
    let mut ledger = TruncationLedger::new();
    assert_eq!(ledger.era(), 0);
    ledger.record_truncation(5);
    assert_eq!(ledger.era(), 1);
    ledger.record_truncation(5);
    assert_eq!(ledger.era(), 2);
  }

  /// The depth model, event by event: tombstones are neutral, wraps count at their own
  /// index, tokens and diag slots are invisible to balance.
  #[test]
  fn depth_deltas_follow_the_model() {
    let start: Event<()> = Event::StartNode {
      kind: 7,
      forward_parent: None,
    };
    let tomb: Event<()> = Event::StartNode {
      kind: TOMBSTONE,
      forward_parent: None,
    };
    let wrap: Event<()> = Event::StartAt {
      kind: 7,
      target: 0,
      prev: None,
    };
    let fin: Event<()> = Event::FinishNode { kind: 7 };
    let tok: Event<()> = Event::Token { kind: 7, span: () };
    let diag: Event<()> = Event::Diag { error_span: None };

    assert_eq!(start.depth_delta(), 1);
    assert_eq!(tomb.depth_delta(), 0);
    assert_eq!(wrap.depth_delta(), 1);
    assert_eq!(fin.depth_delta(), -1);
    assert_eq!(tok.depth_delta(), 0);
    assert_eq!(diag.depth_delta(), 0);

    assert!(tomb.is_tombstone());
    assert!(!start.is_tombstone());
  }

  /// An inert mark can never name a real slot: its index is `u64::MAX`, above any
  /// reachable buffer length, so the bounds half of validation rejects it before the era
  /// half is consulted.
  #[test]
  fn inert_mark_is_out_of_every_buffer() {
    let m = EventMark::inert();
    assert_eq!(m.index(), u64::MAX);
    assert_eq!(m.era(), u64::MAX);
  }
}
