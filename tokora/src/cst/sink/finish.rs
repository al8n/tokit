//! Materialization: the one place the buffered event stream becomes a rowan green tree —
//! losslessness and no-duplication enforced as **one function**, never panicking.
//!
//! The walk validates as it drives the builder: balance (an orphan finish or a leftover
//! open is a typed error — rowan's silent one-level absorb under the root wrapper is
//! unreachable, because the sink's own stack refuses first), close identity (a finish names
//! the kind it means to close, and closing a different one is refused), retro-wrap integrity
//! (stale `StartAt` targets, dangling `forward_parent` pointers — the journal's finish-time
//! canary), kind hygiene (the reserved tombstone band, and every kind the dialect's own
//! validator rejects), span discipline for tokens **and** for the diagnostic spans that
//! license gaps (monotone, non-overlapping, in-bounds, u32-fitting, char-aligned), the
//! **token-channel wall** (a balanced
//! stream that builds structure without one committed token over a nonempty source is
//! the half-forwarding-wrapper signature, refused instead of dressed up by tiling — see
//! [`FinishError::StructureWithoutTokens`]), and **gap tiling**: every source byte no
//! committed token covers becomes a `gap_kind` token, which is what makes
//! `tree.text() == source` structural for every input — poisoned, error-bearing, and
//! truncated parses included.
//!
//! # The gap-coverage law (and the one deliberate channel coupling)
//!
//! Gap tiling is not unconditional: a tiled byte must be *explained*. `finish` tiles a gap
//! only where a **recorded lexer-error diagnostic** covers it (the lexer saw bytes it could
//! not tokenize, said so, and committed no token there); a gap with no covering error and no
//! covering token is a **dropped committed token** — the partial-forwarding-wrapper signature
//! the zero-token wall cannot see — and is refused ([`FinishError::UncoveredGap`]). This is
//! the sink's one **deliberate coupling of the diagnostic and CST channels**: elsewhere they
//! are independent (a `Diag` slot is invisible to the tree), but at `finish` a lexer error's
//! *span* is what licenses its gap. The audit kept the channels separate; this law crosses
//! them on purpose, and only at materialization. [`finish_partial`](Sink::finish_partial)
//! is exempt — it tiles every gap, tolerating an incomplete parse the way it tolerates open
//! nodes (see its own boundary note on the fail-fast emitter case).

use std::vec::Vec;

use rowan::{GreenNode, GreenNodeBuilder, SyntaxKind};

use crate::{Lexer, span::Span};

use super::{
  super::{
    event::{Event, TOMBSTONE},
    profile::KindValidator,
    text::CstText,
  },
  Sink, TriviaPolicy,
};

/// Why a materialization was refused. Every variant names the offending **event index**
/// (the buffer position of the event that broke the law), so the failure is diagnosable
/// against the recorded stream without exposing the stream itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FinishError {
  /// A `FinishNode` arrived with no node open — the orphan-finish shape (a start rolled
  /// back apart from its finish). Under a plain rowan root wrapper this imbalance would
  /// be silently absorbed; here it is refused before the builder ever sees it.
  #[error(
    "finish event at index {index} closes no open node (its start was rolled back or never \
     emitted)"
  )]
  OrphanFinish {
    /// The buffer index of the orphan `FinishNode`.
    index: u64,
  },

  /// The walk ended with nodes still open — a fatal abort, an unguarded unwind, or a raw
  /// bracketing bug. `finish` refuses; `finish_partial` closes them instead (the explicit
  /// tooling opt-in).
  #[error("{open} node(s) still open at the end of the event stream")]
  UnclosedNodes {
    /// How many starts never saw their finish.
    open: u64,
  },

  /// A `StartAt`'s target is not a live tombstone — out of bounds, or the slot holds a
  /// different event. Unreachable through the validated emission surface (marks panic at
  /// spend time); refused here as the release backstop.
  #[error("retro-wrap at index {index} targets {target}, which is not a live tombstone")]
  StaleStartAt {
    /// The buffer index of the `StartAt` event.
    index: u64,
    /// The target it names.
    target: u64,
  },

  /// A tombstone's `forward_parent` pointer does not name a `StartAt` targeting it — the
  /// dangling-pointer shape of an abandoned wrap that escaped the undo journal. With the
  /// journal reverse-replayed on every rewind this is unreachable; it is checked anyway,
  /// because the silent alternative is a stolen start.
  #[error(
    "tombstone at index {index} carries a forward_parent that names no retro-wrap of it \
     (an abandoned wrap escaped the undo journal)"
  )]
  DanglingForwardParent {
    /// The buffer index of the corrupt tombstone.
    index: u64,
  },

  /// A finish would close a retro-wrap before the buffer position of the `StartAt` that
  /// declared it — the wrap crosses a node boundary instead of enclosing whole subtrees
  /// (a mark taken inside a node, wrapped after the node closed).
  #[error(
    "finish at index {finish} closes the retro-wrap declared at index {start_at} before \
     its declaration (the wrap crosses a node boundary)"
  )]
  ImproperWrap {
    /// The buffer index of the `StartAt` whose node was closed too early.
    start_at: u64,
    /// The buffer index of the offending finish.
    finish: u64,
  },

  /// An event carries the reserved tombstone kind (`u16::MAX`) where a real kind is
  /// required — the dialect mapper or a raw caller leaked the reserved band. The
  /// emission-time debug assert is the detect-at-cause form; this is the release wall.
  #[error("event at index {index} carries the reserved tombstone kind (u16::MAX)")]
  ReservedKind {
    /// The buffer index of the offending event.
    index: u64,
  },

  /// The dialect root kind itself is the reserved tombstone kind.
  #[error("the root kind is the reserved tombstone kind (u16::MAX)")]
  ReservedRootKind,

  /// The stream builds structure but carries **no committed token** over a nonempty
  /// source — the half-forwarding-wrapper signature. A wrapper emitter that forwards the
  /// [`CstEmitter`](crate::emitter::CstEmitter) structuring surface (satisfying the
  /// `node()` bound) but inherits the defaulted no-op
  /// [`Emitter::commit_token`](crate::emitter::Emitter::commit_token) produces exactly
  /// this shape: the parse succeeds, every structuring event flows, and every committed
  /// token silently vanishes between the input layer and the sink. Gap tiling would
  /// happily return a *plausible* tree (full text, empty nodes), so `finish` refuses
  /// instead — the loud failure the wrapper contract promises. A parse that legitimately
  /// consumed nothing builds either no structure (nothing to refuse) or a tree over an
  /// empty source (also not refused); a fatally-aborted parse inspected through
  /// [`finish_partial`](Sink::finish_partial) still has its open nodes as the abort
  /// witness and is likewise not refused.
  #[error(
    "the event stream builds structure but carries no committed token over a nonempty \
     source (a wrapper emitter forwarding the CstEmitter structuring surface without \
     Emitter::commit_token produces exactly this shape)"
  )]
  StructureWithoutTokens,

  /// A run of source bytes that **no committed token covers and no recorded lexer-error
  /// diagnostic explains** — an *unexplained gap*. Gap tiling makes `tree.text() == source`
  /// structural, but only bytes a lexer legitimately refused (a diagnostic was recorded, with
  /// this span, and no token settled there) may become `gap_kind` tokens. A byte no token and
  /// no error accounts for is a **dropped committed token** — the partial-forwarding-wrapper
  /// signature the zero-token wall ([`StructureWithoutTokens`](Self::StructureWithoutTokens))
  /// cannot see because a token *did* survive — or, under a fail-fast emitter, an unconsumed
  /// tail an abort left un-diagnosed. `finish` refuses it rather than tile a plausible-but-lossy
  /// tree; [`finish_partial`](Sink::finish_partial) tiles it (the tooling door that tolerates
  /// an incomplete parse). The first (leftmost) uncovered run is named.
  #[error(
    "source bytes {start}..{end} are covered by neither a committed token nor a recorded \
     lexer-error diagnostic (a dropped committed token, or an un-diagnosed unconsumed region)"
  )]
  UncoveredGap {
    /// The first uncovered byte offset.
    start: u32,
    /// The end (exclusive) of the first maximal uncovered run.
    end: u32,
  },

  /// A token span starts before the end of the previous token — a double emission or a
  /// non-monotone stream. Rejecting it here is what makes the no-duplication half of the
  /// round-trip law structural.
  #[error("token at index {index} overlaps the previous token's span")]
  OverlappingSpans {
    /// The buffer index of the offending token event.
    index: u64,
  },

  /// A token offset does not fit rowan's `u32` text size. Nothing is truncated; the
  /// materialization is refused whole.
  #[error("token at index {index} has an offset beyond u32::MAX (rowan text sizes are u32)")]
  OffsetOverflow {
    /// The buffer index of the offending token event.
    index: u64,
  },

  /// A token span does not slice the given source (beyond its end, or off a UTF-8
  /// boundary) — the events and the source disagree.
  #[error("token at index {index} does not slice the given source")]
  SpanOutOfBounds {
    /// The buffer index of the offending token event.
    index: u64,
  },

  /// A recorded lexer-error span is not a slice of the bound source — a `u32`-overflowing or
  /// reversed endpoint, an end past the source, or an endpoint off a UTF-8 boundary.
  ///
  /// A diagnostic span is **evidence**: it is the one thing that licenses a gap tile, and the
  /// tile slices the source at exactly its endpoints. So it earns the token-span discipline
  /// rather than a clamp — a clamped endpoint silently re-aims the evidence at bytes the lexer
  /// never refused, which is how a dropped committed token gets excused. Reversed spans are
  /// *unrepresentable* for [`SimpleSpan`](crate::SimpleSpan), whose constructor asserts, but
  /// perfectly representable for a `Range<usize>` span and through the unchecked `*_const`
  /// constructors; this wall refuses them at materialization whichever door produced them.
  /// Zero-width spans (`start == end`) stay legal — they cover nothing and license nothing.
  #[error(
    "the lexer-error diagnostic at index {index} carries a span that does not slice the source"
  )]
  InvalidDiagnosticSpan {
    /// The buffer index of the offending `Diag` slot.
    index: u64,
  },

  /// An event carries a kind the dialect's own [`KindValidator`](crate::cst::KindValidator)
  /// rejects — a kind outside the space the dialect declared at construction.
  ///
  /// Distinct from [`ReservedKind`](Self::ReservedKind), which names the reserved tombstone
  /// band specifically (the floor no validator can lift). Without this wall a stray kind
  /// reaches `rowan`, where the next authority is a `kind_from_raw` panic at query time —
  /// arbitrarily far from the parse that produced it, and not a typed error at all.
  #[error("event at index {index} carries kind {kind}, which the dialect's validator rejects")]
  InvalidDialectKind {
    /// The buffer index of the offending event.
    index: u64,
    /// The rejected kind.
    kind: u16,
  },

  /// The source the sink was constructed with is not valid UTF-8, so no green tree can be
  /// built from it — `rowan` stores text as `&str`.
  #[error("the bound source is not valid UTF-8 (first invalid sequence at byte {valid_up_to})")]
  NonUtf8Source {
    /// The number of leading bytes that *are* valid UTF-8
    /// ([`Utf8Error::valid_up_to`](core::str::Utf8Error::valid_up_to)).
    valid_up_to: u32,
  },

  /// A `FinishNode` names a different kind than the frame it would close — the leaked-finish
  /// shape, where the intended start was rolled back and the finish lands on an ancestor
  /// instead.
  ///
  /// Checked after [`OrphanFinish`](Self::OrphanFinish) and
  /// [`ImproperWrap`](Self::ImproperWrap), which describe *structural* impossibilities; this
  /// one describes a structurally-valid close of the wrong node. Identity by kind is
  /// deliberately weak — a leaked finish onto a same-kind ancestor still passes, and surfaces
  /// only as an imbalance at the end of the stream — see the
  /// [`CstEmitter`](crate::emitter::CstEmitter) contract for why a branded per-open token is
  /// not paid for.
  #[error(
    "finish at index {index} names kind {found} but closes a node of kind {expected} (its \
     own start was rolled back, or the pair is mis-bracketed)"
  )]
  MismatchedFinish {
    /// The buffer index of the offending `FinishNode`.
    index: u64,
    /// The kind of the frame the finish actually lands on.
    expected: u16,
    /// The kind the finish named.
    found: u16,
  },
}

/// One tick of [`W`](w) — a real increment under `cfg(test)`, an empty inlined body
/// otherwise, so no shipped build carries the instrument.
#[cfg(test)]
#[inline(always)]
fn w_tick() {
  w::tick();
}

/// The release form of [`w_tick`]: nothing.
#[cfg(not(test))]
#[inline(always)]
const fn w_tick() {}

/// Charges `n` units of [`W`](w) at once: the honest price of one bulk library call whose
/// iteration happens where the counter cannot see it (today, exactly one — the `sort_unstable`
/// over gathered diagnostic spans). Charging the element count is what makes a sort migrating
/// into the per-event body visible: it would then cost `k` per event instead of `k` per finish.
#[cfg(test)]
#[inline(always)]
fn w_tick_n(n: u64) {
  w::add(n);
}

/// The release form of [`w_tick_n`]: nothing.
#[cfg(not(test))]
#[inline(always)]
const fn w_tick_n(_n: u64) {}

/// One open node during the replay walk: a direct start, a hoisted retro-wrap (carrying
/// the buffer index of its `StartAt` declaration), or the synthetic dialect root.
///
/// Every real frame carries its kind, so a `FinishNode` can be checked against the node it
/// would actually close. The root carries none: a finish never closes it (the orphan wall
/// refuses first).
enum Frame {
  /// The dialect root wrapper.
  Root,
  /// A direct `StartNode`, and its kind.
  Start(u16),
  /// A retro-wrap hoisted to its target's position: the `StartAt`'s own buffer index (a
  /// finish may close it only at or after that position) and its kind.
  Wrap(u64, u16),
}

impl<'inp, L, E> Sink<'inp, L, E>
where
  L: Lexer<'inp>,
{
  /// Materializes the buffered events into a green tree wrapped in `root_kind`, returning
  /// the inner emitter either way — the sink is consumed exactly once, and the
  /// diagnostics survive the tree.
  ///
  /// The text comes from the source the sink was **constructed** with (read through
  /// [`CstText`](super::super::CstText)), so the spans and the bytes they slice cannot come
  /// from two different buffers; a byte-backed source that is not UTF-8 is refused once, here
  /// ([`FinishError::NonUtf8Source`]).
  ///
  /// The replay validates and builds in one walk: balance, close identity
  /// ([`FinishError::MismatchedFinish`]), retro-wrap integrity, kind hygiene (the reserved
  /// band, and every kind the dialect's own validator rejects —
  /// [`FinishError::InvalidDialectKind`]), span discipline for tokens and for the diagnostic
  /// spans that license gaps ([`FinishError::InvalidDiagnosticSpan`]), the token-channel wall
  /// ([`FinishError::StructureWithoutTokens`] — structure with zero committed tokens
  /// over a nonempty source is a severed `commit_token` channel, not a tree), the
  /// **gap-coverage law** (every uncovered byte tiles as a `gap_kind` token only where a
  /// recorded lexer error explains it; an unexplained gap is a dropped committed token —
  /// [`FinishError::UncoveredGap`]) — so on success `tree.text() == source` holds and
  /// every gap in it is a byte the lexer legitimately refused. On the first violation the
  /// half-built green state is dropped and a typed [`FinishError`] comes back instead;
  /// this method **never panics**.
  ///
  /// # Abort semantics
  ///
  /// - An `Incomplete` parse (needs more input) should not be materialized: keep the sink
  ///   — the buffered events *are* the resumable state.
  /// - A fatal abort leaves open nodes; `finish` refuses them
  ///   ([`FinishError::UnclosedNodes`]) — [`finish_partial`](Self::finish_partial) is
  ///   the explicit opt-in that closes them for tooling.
  ///
  /// # The fail-fast boundary (precise)
  ///
  /// The gap-coverage guarantee holds for a **collecting** inner emitter (the lossless
  /// case): every lexer error is recorded, with its span, before the parse moves on, so
  /// every refused byte is explained and `finish` succeeds. Under a **fail-fast** emitter
  /// ([`Fatal`](crate::emitter::Fatal)) the first lexer error aborts the parse, so the bytes
  /// past it are never lexed and no diagnostic covers them: `finish` then refuses (an
  /// [`UncoveredGap`](FinishError::UncoveredGap), or [`UnclosedNodes`](FinishError::UnclosedNodes)
  /// if the abort left a node open). That is by design — inspect such a partial parse through
  /// [`finish_partial`](Self::finish_partial), which tiles the un-diagnosed tail, or accept no
  /// tree. The guarantee is stated only for the collecting case for exactly this reason.
  pub fn finish(self, root_kind: u16) -> (Result<GreenNode, FinishError>, E)
  where
    L::Offset: TryInto<u32>,
    L::Source: CstText,
  {
    self.materialize(root_kind, false)
  }

  /// [`finish`](Self::finish), but the two **incompleteness** signals a partial parse leaves
  /// are tolerated instead of refused, so tooling can inspect a fatally-aborted or truncated
  /// parse: open nodes at the end of the stream are **closed** (not
  /// [`UnclosedNodes`](FinishError::UnclosedNodes)), and an uncovered gap is **tiled** (not
  /// [`UncoveredGap`](FinishError::UncoveredGap)) — the un-diagnosed tail of a fail-fast
  /// abort becomes one `gap_kind` run so `tree.text() == source` still holds. Every other law
  /// (balance underflow, close identity, wrap integrity, kind hygiene, span discipline —
  /// diagnostic spans included: a malformed one is corruption, not incompleteness, and is
  /// refused through **both** doors — and the token-channel wall for *balanced* streams, since
  /// the zero-token severance is corruption too) is enforced identically. The exemptions are
  /// the two ways an incomplete parse differs from a complete one; refusing them would defeat
  /// the door.
  pub fn finish_partial(self, root_kind: u16) -> (Result<GreenNode, FinishError>, E)
  where
    L::Offset: TryInto<u32>,
    L::Source: CstText,
  {
    self.materialize(root_kind, true)
  }

  /// The one replay walk behind [`finish`](Self::finish) and
  /// [`finish_partial`](Self::finish_partial).
  fn materialize(
    self,
    root_kind: u16,
    close_open_nodes: bool,
  ) -> (Result<GreenNode, FinishError>, E)
  where
    L::Offset: TryInto<u32>,
    L::Source: CstText,
  {
    let events = self.events;
    let profile = self.profile;
    let inner = self.inner;
    // TriviaPolicy::AsEmitted is the only variant today: the replay below IS that policy
    // (tokens land in whichever node is open at their buffer position).
    let TriviaPolicy::AsEmitted = self.trivia;

    // The source is the one the sink was constructed with, so the spans and the text they
    // slice can never come from different buffers. A byte-backed source validates here, once,
    // rather than per token.
    let result = match self.source.cst_text() {
      Ok(source) => replay(
        &events,
        root_kind,
        profile.gap_kind(),
        profile.validator(),
        source,
        close_open_nodes,
      ),
      Err(err) => Err(FinishError::NonUtf8Source {
        valid_up_to: u32::try_from(err.valid_up_to()).unwrap_or(u32::MAX),
      }),
    };
    (result, inner)
  }
}

/// Converts one span endpoint, mapping failures to the event's typed error.
fn offset_to_u32<O>(offset: O, index: u64) -> Result<u32, FinishError>
where
  O: TryInto<u32>,
{
  offset
    .try_into()
    .map_err(|_| FinishError::OffsetOverflow { index })
}

/// Converts one diagnostic-span endpoint. A failure is the evidence error, not the token one:
/// the offending event is a `Diag` slot, and conflating the two would send a reader looking for
/// a token at that index.
fn diag_offset_to_u32<O>(offset: O, index: u64) -> Result<u32, FinishError>
where
  O: TryInto<u32>,
{
  offset
    .try_into()
    .map_err(|_| FinishError::InvalidDiagnosticSpan { index })
}

/// Advances a **shared, monotone** cursor over the sorted, non-overlapping `cover` and reports
/// the first sub-range of `[lo, hi)` no interval spans, or `None` when `[lo, hi)` is fully
/// covered (or empty).
///
/// The cursor is the whole of D9's repair. Gaps reach this function in source order, so an
/// interval the walk has already passed can never be needed again; carrying `at` across calls
/// makes total coverage work `O(cover.len() + gaps)` instead of `O(gaps × cover.len())`. The
/// old shape restarted from index 0 on every gap — exact `n(n+1)/2` on error-dense input.
///
/// What is deliberately **not** changed: the answer. Coverage is still decided against the
/// merged *set* of recorded error spans, so it does not depend on the order the diagnostics
/// were emitted in — see [`replay`]'s note on why that is load-bearing rather than incidental.
fn first_uncovered(lo: u32, hi: u32, cover: &[(u32, u32)], at: &mut usize) -> Option<(u32, u32)> {
  if lo >= hi {
    return None;
  }
  let mut cursor = lo;
  while let Some(&(s, e)) = cover.get(*at) {
    w_tick(); // W row 4
    if e <= cursor {
      *at += 1; // entirely behind the walk: retired for good, never rescanned
      continue;
    }
    if s > cursor {
      return Some((cursor, s.min(hi))); // a hole precedes this interval
    }
    cursor = cursor.max(e);
    if cursor >= hi {
      return None; // fully covered, and this interval may still cover later bytes
    }
    *at += 1;
  }
  Some((cursor, hi))
}

/// `⌈log₂ k⌉`, the comparison depth a `k`-element sort pays — the multiplier that makes the
/// sort's charge to [`W`](w) its real cost rather than a flat element count.
#[inline]
fn ceil_log2(k: u64) -> u64 {
  if k < 2 {
    0
  } else {
    u64::from(k.next_power_of_two().trailing_zeros())
  }
}

/// The validating replay: **two passes — linear in events, plus one `k log k` ordering of
/// the recorded diagnostic spans**, one pass to gather and one to walk.
///
/// # Why two passes, and why the second one is not enough on its own
///
/// The obvious rewrite is a single forward pass that decides coverage from a monotone cursor
/// as it meets each `Diag`. It is wrong on this crate's own event streams, and the reason is
/// worth stating where the code is: **tokens are recorded at settle, lexer errors at lex**,
/// and the input layer's lookahead routinely settles a token *after* a diagnostic whose span
/// lies to its right. The combined stream is therefore not ordered by source position, and it
/// is not even stable — the same parse puts the same diagnostic at a different buffer index
/// depending on how far the caller peeked. (`input_ref`'s own cache-transparency oracle says
/// so in as many words: the token-event stream is exactly invariant under prefill, *"stronger
/// than diagnostics, which may hoist"*.) A cursor driven in emission order turns that skew
/// into an `OverlappingSpans` refusal on a legal parse, and makes the materialized tree a
/// function of the peek schedule.
///
/// So the gather pass stays. It is what makes the coverage verdict a function of the **set**
/// of recorded error spans rather than of their arrival order, which is the property that
/// absorbs the skew — by construction, not by assumption. What the gather pass does *not* get
/// to keep is the rescan: the merged cover is swept once by a shared cursor
/// ([`first_uncovered`]), not from index 0 per gap.
///
/// # Pass 1 — gather
///
/// Validates every kind against the dialect's own predicate, validates every retro-wrap
/// target, checks the `forward_parent` canary, and collects the recorded lexer-error spans —
/// each one held to the token-span discipline before it is allowed to license anything. Then
/// one `sort_unstable` over **diagnostics** (see the `W` inventory for how that cost is
/// charged and pinned) and one linear merge into sorted, non-overlapping intervals.
///
/// # Pass 2 — walk
///
/// Drives the builder, and recovers each target's retro-wraps from **the target's own chain**
/// (`forward_parent` → newest `StartAt` → `prev` → …), newest-first, which is the order they
/// open in. That replaces a `BTreeMap<target, Vec<…>>` rebuilt on every `finish`. A flat
/// reachability bitset keeps the integrity the unconditional rebuild used to give for free: a
/// `StartAt` no chain reaches is refused, never silently dropped.
///
/// The uncovered-gap latch is still consumed at the **end** of the walk, after the balance and
/// token-channel walls, so every error precedence the old shape had is preserved exactly.
fn replay<S>(
  events: &[Event<S>],
  root_kind: u16,
  gap_kind: u16,
  validator: KindValidator,
  source: &str,
  close_open_nodes: bool,
) -> Result<GreenNode, FinishError>
where
  S: Span,
  S::Offset: TryInto<u32>,
{
  // The root kind is validated in every build: it is one check per materialization, not per
  // event, so it costs nothing measurable.
  //
  // The PER-EVENT validator calls below are `cfg!(debug_assertions)`-gated, and that is a
  // measured trade rather than a preference. Every route by which a dialect kind reaches an
  // event goes through a door that validates it in every build — `cst_start`, `cst_start_at`,
  // and the one `record_token` body behind both token doors, plus `CstProfile::new` for the
  // two synthesized kinds — so for any caller reachable from outside this crate the wall is
  // already absolute, and earlier: it refuses at the emit site, at the cause, instead of one
  // materialization later. The single route with no door is `push_raw_event_for_tests`, which
  // is `pub(crate)`. Keeping the check here for that route, in every build, cost a measured
  // +8.3% on ordinary materialization (an unpredictable indirect call per event inside a tight
  // builder loop), which is the whole of it. So the honest trade, stated rather than hidden: a
  // RELEASE build whose event log was assembled by raw in-crate injection can materialize an
  // out-of-language kind. Every test run and every CI build refuses it.
  //
  // `ReservedKind` is NOT gated — the tombstone band is a plain comparison, it costs nothing,
  // and it was a release wall before this round.
  // One predicate for every kind in the stream, root included. `admits` ANDs in the tombstone
  // floor, so the reserved band is refused whatever the dialect's own predicate says — but the
  // reserved band keeps its own error identity, because "you used the reserved value" and
  // "that kind is not in your language" are different mistakes with different fixes.
  if root_kind == TOMBSTONE {
    return Err(FinishError::ReservedRootKind);
  }
  if !validator.admits(root_kind) {
    return Err(FinishError::InvalidDialectKind {
      index: 0,
      kind: root_kind,
    });
  }

  let source_len =
    u32::try_from(source.len()).map_err(|_| FinishError::OffsetOverflow { index: 0 })?;

  // ── Pass 1: gather ──────────────────────────────────────────────────────────
  let mut error_spans: Vec<(u32, u32)> = Vec::new();
  for (index, event) in events.iter().enumerate() {
    w_tick(); // W row 1
    let index = index as u64;
    match event {
      Event::StartAt { kind, target, .. } => {
        if *kind == TOMBSTONE {
          return Err(FinishError::ReservedKind { index });
        }
        if cfg!(debug_assertions) && !validator.admits(*kind) {
          return Err(FinishError::InvalidDialectKind { index, kind: *kind });
        }
        let live = *target < index && events[*target as usize].is_tombstone();
        if !live {
          return Err(FinishError::StaleStartAt {
            index,
            target: *target,
          });
        }
      }
      Event::StartNode {
        kind: TOMBSTONE,
        forward_parent: Some(relative),
      } => {
        // The journal-integrity canary: a set pointer must name a StartAt of this
        // tombstone. A dangling pointer is the un-journaled abandoned wrap, surfaced as a
        // typed error instead of a stolen start. Under the chain the pointer is also the
        // head of the wrap list, so this check now guards materialization, not just hygiene.
        let target = index;
        let named = target + u64::from(relative.get());
        let names_this = matches!(
          events.get(named as usize),
          Some(Event::StartAt { target: t, .. }) if *t == target
        );
        if !names_this {
          return Err(FinishError::DanglingForwardParent { index });
        }
      }
      Event::StartNode { kind, .. } if *kind == TOMBSTONE => {}
      Event::StartNode { kind, .. } | Event::Token { kind, .. } => {
        if *kind == TOMBSTONE {
          return Err(FinishError::ReservedKind { index });
        }
        if cfg!(debug_assertions) && !validator.admits(*kind) {
          return Err(FinishError::InvalidDialectKind { index, kind: *kind });
        }
      }
      Event::Diag {
        error_span: Some(span),
      } => {
        // A lexer error's span covers the bytes it refused — and covering a byte is what
        // *licenses* tiling it. That makes the span evidence, so it gets the token-span
        // discipline rather than the clamp it used to get: a clamped endpoint quietly re-aims
        // the evidence at a range the lexer never refused, excusing exactly the dropped
        // committed token `UncoveredGap` exists to catch. The gap tile slices the source at
        // these offsets, so they must also be char boundaries.
        let start = diag_offset_to_u32(span.start(), index)?;
        let end = diag_offset_to_u32(span.end(), index)?;
        if start > end
          || end > source_len
          || !source.is_char_boundary(start as usize)
          || !source.is_char_boundary(end as usize)
        {
          return Err(FinishError::InvalidDiagnosticSpan { index });
        }
        // A zero-width span covers nothing, so it licenses nothing: legal, and dropped.
        if start < end {
          error_spans.push((start, end));
        }
      }
      Event::FinishNode { .. } | Event::Diag { error_span: None } => {}
    }
  }

  // One sort, over DIAGNOSTICS: `O(k log k)` with `k ≤ events`. Its comparisons happen inside
  // `std`, where the counter cannot see them, so the cost is charged to `W` explicitly — and
  // charged at `k·⌈log₂ k⌉`, its REAL cost, not at a flat `k`.
  //
  // That distinction is the difference between a gate and a decoration. Charging `k` makes the
  // charged quantity linear by construction, so the growth cell would report 4.00× for a 4×
  // input no matter how the ordering behaved, and could never fail for the reason it exists.
  // With `k` diagnostics per token — the error-dense payload — this term is `Θ(n log n)`, and
  // the law below is stated in that shape rather than claiming a linearity the sort does not
  // provide.
  let k = error_spans.len() as u64;
  w_tick_n(k * ceil_log2(k)); // W row 2
  error_spans.sort_unstable();
  let mut error_cover: Vec<(u32, u32)> = Vec::with_capacity(error_spans.len());
  for (s, e) in error_spans {
    w_tick(); // W row 3
    match error_cover.last_mut() {
      Some((_, last_end)) if s <= *last_end => *last_end = (*last_end).max(e),
      _ => error_cover.push((s, e)),
    }
  }

  // ── Pass 2: walk ────────────────────────────────────────────────────────────
  let mut builder = GreenNodeBuilder::new();
  let mut stack: Vec<Frame> = Vec::new();
  builder.start_node(SyntaxKind(root_kind));
  stack.push(Frame::Root);

  // The tiling cursor: the end of the last covered source byte, in u32 space.
  let mut covered: u32 = 0;

  // The token-channel witnesses (see `StructureWithoutTokens`): whether any committed
  // token survived to materialization, and whether any real node did. Structure without a
  // single token over a nonempty source is the half-forwarding-wrapper signature — the
  // gap tiling below would otherwise dress it up as a plausible tree.
  let mut saw_token = false;
  let mut saw_structure = false;

  // The leftmost source run no committed token covers and no lexer error explains — the
  // dropped-committed-token signature (see `UncoveredGap`). Recorded during tiling and
  // refused at the end (by `finish`), so the zero-token wall keeps precedence for the
  // all-dropped case.
  let mut first_uncovered_gap: Option<(u32, u32)> = None;
  // The shared monotone cursor into `error_cover` — see `first_uncovered`.
  let mut cover_at: usize = 0;

  // The retro-wrap reachability set: one bit per event index, set for every `StartAt` the
  // walk opened by following a target's chain. Its purpose is the integrity the old
  // unconditional `wraps` materialization gave for free — a `StartAt` that no chain reaches
  // must be refused, not structurally dropped. Allocated lazily at the first chain hop
  // (a wrap-free stream never touches it) as ONE flat `alloc_zeroed` of
  // `ceil(events / 64)` words: no per-event iteration and no keyed structure, which is why
  // it is a *justified* row of the `W` inventory rather than a ticked one, and why it is
  // not the allocation class the chain exists to delete.
  let mut reached: Vec<u64> = Vec::new();

  for (index, event) in events.iter().enumerate() {
    w_tick(); // W row 5
    let index = index as u64;
    match event {
      Event::StartNode {
        kind: TOMBSTONE,
        forward_parent,
      } => {
        // An inert mark — unless retro-wraps target it. They open HERE, newest first (the
        // later wrap's finish comes later, so it is the outer node), which is exactly the
        // order the chain is threaded in.
        let mut link = *forward_parent;
        let mut previous: Option<u32> = None;
        while let Some(relative) = link {
          w_tick(); // W row 6 — one chain hop
          // Strictly decreasing offsets: an older `StartAt` sits at a smaller index. This
          // is what makes the chain terminating and fork-free, so a corrupt link is a
          // refusal rather than a hang.
          if previous.is_some_and(|p| relative.get() >= p) {
            return Err(FinishError::DanglingForwardParent { index });
          }
          previous = Some(relative.get());

          let at = index + u64::from(relative.get());
          let Some(Event::StartAt { kind, target, prev }) = events.get(at as usize) else {
            return Err(FinishError::DanglingForwardParent { index });
          };
          if *target != index {
            return Err(FinishError::DanglingForwardParent { index });
          }

          if reached.is_empty() {
            reached = std::vec![0u64; events.len().div_ceil(64)];
          }
          if let Some(word) = reached.get_mut((at / 64) as usize) {
            *word |= 1u64 << (at % 64);
          }

          builder.start_node(SyntaxKind(*kind));
          stack.push(Frame::Wrap(at, *kind));
          link = *prev;
        }
      }
      Event::StartNode { kind, .. } => {
        saw_structure = true;
        builder.start_node(SyntaxKind(*kind));
        stack.push(Frame::Start(*kind));
      }
      Event::Token { kind, span } => {
        saw_token = true;
        let start = offset_to_u32(span.start(), index)?;
        let end = offset_to_u32(span.end(), index)?;
        if start < covered || end < start {
          return Err(FinishError::OverlappingSpans { index });
        }
        if end > source_len {
          return Err(FinishError::SpanOutOfBounds { index });
        }
        // Tile the gap this token reveals: bytes no committed token covered (a skipped
        // lexer error, an undrained region) become one gap token in the currently open
        // node — losslessness by construction, not by lexer luck.
        if start > covered {
          let gap = source
            .get(covered as usize..start as usize)
            .ok_or(FinishError::SpanOutOfBounds { index })?;
          if first_uncovered_gap.is_none() {
            first_uncovered_gap = first_uncovered(covered, start, &error_cover, &mut cover_at);
          }
          builder.token(SyntaxKind(gap_kind), gap);
        }
        let text = source
          .get(start as usize..end as usize)
          .ok_or(FinishError::SpanOutOfBounds { index })?;
        builder.token(SyntaxKind(*kind), text);
        covered = end;
      }
      Event::FinishNode { kind } => {
        // Precedence is deliberate and unchanged at the top: the two STRUCTURAL walls first
        // (a finish with nothing to close; a wrap closed before it was declared), then the
        // identity check, which describes a structurally-valid close of the wrong node.
        let expected = match stack.last() {
          None | Some(Frame::Root) => {
            // The sink's own wall: rowan would silently absorb one level of imbalance
            // under the root wrapper; the walk refuses before the builder sees it.
            return Err(FinishError::OrphanFinish { index });
          }
          Some(Frame::Wrap(start_at, frame_kind)) => {
            // A hoisted wrap may only close at or after its declaration: closing
            // earlier means the wrap crosses a node boundary (the mark was taken
            // inside a node that closed before the wrap was declared).
            if *start_at > index {
              return Err(FinishError::ImproperWrap {
                start_at: *start_at,
                finish: index,
              });
            }
            *frame_kind
          }
          Some(Frame::Start(frame_kind)) => *frame_kind,
        };
        if expected != *kind {
          // The leaked finish: the start this call names died with a rewind, so the finish
          // lands on an ancestor. Nothing about the buffer's *shape* distinguishes that from
          // a legal cross-checkpoint close — only the named kind does.
          return Err(FinishError::MismatchedFinish {
            index,
            expected,
            found: *kind,
          });
        }
        stack.pop();
        builder.finish_node();
      }
      Event::StartAt { target, .. } => {
        // Its node was opened at the target's position (the hoist above); the declaration
        // slot itself is structural silence — except that this is the only place every
        // `StartAt` is guaranteed to be seen, so it is where reachability is asked.
        saw_structure = true;
        // The old shape materialized every `StartAt` from a keyed index, so an unreachable
        // one was impossible by construction. Following chains instead makes it
        // expressible, and a silently dropped node is the failure class this walk exists to
        // refuse — so the bit is checked rather than trusted.
        let unreachable = reached
          .get((index / 64) as usize)
          .is_none_or(|word| (word >> (index % 64)) & 1 == 0);
        if unreachable {
          return Err(FinishError::DanglingForwardParent { index: *target });
        }
      }
      Event::Diag { .. } => {
        // A diagnostic order-slot: invisible to the tree. Its span was gathered in pass 1,
        // where the coverage verdict is made order-independent.
      }
    }
  }

  // The trailing gap: bytes after the last covered token (an undrained tail, a poisoned
  // truncation) tile into the root.
  if covered < source_len {
    let gap = source
      .get(covered as usize..)
      .ok_or(FinishError::SpanOutOfBounds {
        index: events.len() as u64,
      })?;
    if first_uncovered_gap.is_none() {
      first_uncovered_gap = first_uncovered(covered, source_len, &error_cover, &mut cover_at);
    }
    builder.token(SyntaxKind(gap_kind), gap);
  }

  // Balance at the end: everything but the root must have closed. (The root frame is
  // always present here — the orphan wall above refuses every pop that could reach it —
  // but `finish` promises to never panic, so the arithmetic saturates instead of
  // assuming.)
  let open = (stack.len() as u64).saturating_sub(1);
  if open > 0 {
    if !close_open_nodes {
      return Err(FinishError::UnclosedNodes { open });
    }
    for _ in 0..open {
      w_tick(); // W row 7
      builder.finish_node();
    }
  } else if saw_structure && !saw_token && source_len > 0 {
    // The token-channel wall: a *balanced* stream that builds structure without one
    // committed token over a nonempty source is the half-forwarding-wrapper signature
    // (structuring forwarded, `Emitter::commit_token` inherited as the core no-op), and
    // gap tiling would return a plausible tree instead of a witness. Balanced-only on
    // purpose: a fatally-aborted parse inspected through `finish_partial` still has its
    // open nodes (the `open > 0` arm above), so the abort shape is never refused here.
    // Kept ahead of the gap-coverage law so the all-dropped case earns this precise
    // message rather than an `UncoveredGap` over the whole source.
    return Err(FinishError::StructureWithoutTokens);
  } else if !close_open_nodes {
    // The gap-coverage law, `finish`-only (the partial-drop generalization of the wall
    // above): an uncovered byte with no covering lexer error is a dropped committed token —
    // or, under a fail-fast emitter, an un-diagnosed unconsumed tail. `finish_partial` is
    // exempt and tiles the run, tolerating an incomplete parse as it tolerates open nodes.
    if let Some((start, end)) = first_uncovered_gap {
      return Err(FinishError::UncoveredGap { start, end });
    }
  }

  builder.finish_node();
  Ok(builder.finish())
}

/// The replay-work counter `W`: **one tick at the top of every loop body anywhere inside
/// [`replay`] and its helpers**, so the quantity exists identically before and after a
/// rewrite of the walk and before/after is one comparison rather than two instruments.
///
/// It is deliberately **not** a needle counter (one tick per `Diag`, one per tile): such a
/// counter restates the payload's own shape and reads its required GREEN value on unfixed
/// code — measured, and the reason this instrument replaced it.
///
/// # The law (two-sided; the lower bound is what keeps the instrument honest)
///
/// `events ≤ W ≤ 3 × (events + gap_tiles) + k·⌈log₂ k⌉`, **and** the per-4×n growth ratio of
/// `W` ≤ 4.7 (linear would be 4.00; the sort puts it at ~4.5–4.6; a rescan reads ~15).
/// The upper bound catches a reintroduced rescan; the **lower** bound (`W ≥ events`, since
/// the main walk ticks once per event) catches a deleted or misplaced tick — the failure
/// mode an instrument swap is most exposed to; the growth clause catches superlinearity
/// where the absolute bound still has slack (linear ⇒ 4.0, quadratic ⇒ ≈ 16).
///
/// # The iteration-construct inventory (every row ticked, or justified O(1) per event)
///
/// The counter is blind to work that iterates without a `for` of ours, so the obligation
/// covers every iteration *construct*, not every loop: explicit loops; recursion; iterator
/// consumers (`any`/`all`/`position`/`find`/`fold`/`count`/`filter`-chains); `sort*`,
/// `binary_search*`, `contains`, `retain`; every linear-cost `Vec`/slice mutation
/// (`insert`, `remove`, `drain`, `dedup`, `copy_within`, `extend_from_slice`, `clone`,
/// `to_vec`); and every ordered/associative-container operation.
///
/// | # | Construct in `replay` + helpers | Disposition |
/// |---|---|---|
/// | 1 | pass 1, the gather loop over every event | **ticked** (W row 1) |
/// | 2 | `error_spans.sort_unstable()` | **CHARGED AT COST** (W row 2): unticked inside `std`, so `k·⌈log₂ k⌉` — its real cost, not its element count — is added to `W`. One `O(k log k)` over **diagnostics**, `k ≤ events`, once per finish. Charging the element count instead would make the charged quantity linear by construction and leave the growth cell unable to fail |
/// | 3 | the cover-merge loop | **ticked** (W row 3) |
/// | 4 | `first_uncovered`'s sweep of the merged cover | **ticked** (W row 4) — and bounded by `cover.len() + gaps` in total, because the cursor is shared and monotone. This is the construct D9 made quadratic |
/// | 5 | pass 2, the walk over every event | **ticked** (W row 5) |
/// | 6 | the retro-wrap chain hop | **ticked** (W row 6) |
/// | 7 | the end-of-walk close `for _ in 0..open` | **ticked** (W row 7) — the loop the first instrument missed |
/// | 8 | reachability bitset `std::vec![0u64; events.len().div_ceil(64)]` | justified: one lazy `alloc_zeroed` of `ceil(events/64)` words, no per-event iteration, no keyed structure — allocated only when a chain is actually walked |
/// | 9 | `builder.start_node` / `token` / `finish_node`, `source.get`, `stack` push/pop | justified O(1) amortized per event |
///
/// Deleted by the rewrite: the per-materialization `BTreeMap<target, Vec<(u64, u16)>>` —
/// `O(log n)` per `StartAt` plus one `Vec` allocation per target, and the last keyed structure
/// in the walk. The one superlinear term left is the sort, `O(k log k)` in the number of
/// *diagnostics*, and it is charged at that cost rather than at its element count — so the
/// gate can observe it. With one recorded lexer error per token, `k` is `Θ(events)` and the
/// whole materialization is `O(n log n)`; the law is stated in that shape rather than claiming
/// a linearity the ordering does not provide.
///
/// Measured before the rewrite, on the error-dense payload (n = 100 / 400 / 1600):
/// `W` = 5 550 / 82 200 / 1 288 800 against the bound 900 / 3 600 / 14 400, growing 14.81× /
/// 15.68× per 4× of input. After: `8n − 1`, growing 4.00×.
#[cfg(test)]
pub(crate) mod w {
  use core::cell::Cell;

  thread_local! {
    static W: Cell<u64> = const { Cell::new(0) };
  }

  /// One unit of replay work.
  #[inline(always)]
  pub(crate) fn tick() {
    W.with(|w| w.set(w.get().saturating_add(1)));
  }

  /// `n` units at once — see [`w_tick_n`](super::w_tick_n).
  #[inline(always)]
  pub(crate) fn add(n: u64) {
    W.with(|w| w.set(w.get().saturating_add(n)));
  }

  /// Zeroes the counter; call immediately before the measured `finish`.
  pub(crate) fn reset() {
    W.with(|w| w.set(0));
  }

  /// The accumulated tick count since the last [`reset`].
  pub(crate) fn read() -> u64 {
    W.with(Cell::get)
  }
}
