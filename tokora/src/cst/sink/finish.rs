//! Materialization: the one place the buffered event stream becomes a rowan green tree —
//! losslessness and no-duplication enforced as **one function**, never panicking.
//!
//! The walk validates as it drives the builder: balance (an orphan finish or a leftover
//! open is a typed error — rowan's silent one-level absorb under the root wrapper is
//! unreachable, because the sink's own stack refuses first), close identity (a finish names
//! the kind it means to close, and closing a different one is refused), retro-wrap integrity
//! (stale `StartAt` targets, dangling `forward_parent` pointers — the journal's finish-time
//! canary), kind hygiene (the reserved tombstone band and the root kind, refused
//! unconditionally in every build; every other kind, refused at the emission doors in every
//! build too, and re-checked here only as a debug-assertions-gated backstop — see
//! [`FinishError::InvalidDialectKind`]), span discipline for tokens **and** for the diagnostic
//! spans that license gaps (monotone, non-overlapping, in-bounds, u32-fitting, char-aligned),
//! the
//! **token-channel wall** (a balanced
//! stream that builds structure without one committed token over a nonempty source *no
//! lexer error explains* is the half-forwarding-wrapper signature, refused instead of
//! dressed up by tiling — see
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
//! *span* is what licenses its gap. The token-channel wall reads that **same** evidence — it
//! fires only where a byte is left unexplained — so the crossing stays one coupling with two
//! readers, not two couplings. The channels are otherwise separate; this law crosses
//! them on purpose, and only at materialization. [`Cst::finish_partial`](super::super::Cst::finish_partial)
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

/// Why a materialization was refused. Every variant that names a *law the recorded stream
/// broke* names the offending **event index** (the buffer position of the event that broke
/// it), so the failure is diagnosable against the recorded stream without exposing the stream
/// itself. The few that describe the whole stream, its source, or a rewind the sink could not
/// perform name what they can instead.
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

  /// A `Demote`'s target is not a live, un-demoted `StartNode` — out of bounds, a slot
  /// holding some other event, a `cst_mark` tombstone, a slot a **prior** `Demote` already
  /// claimed, or a slot at or **above** the `Demote`'s own index.
  ///
  /// That last shape is raw-injected only: a `Demote` must name a slot strictly below itself,
  /// and the emission wall derives `target` from a validated live slot below the appended
  /// event, so no legal stream can carry one. Left unrefused it would tombstone a start the
  /// walk has not reached yet, erasing that node from a buffer that stays balanced — the
  /// silent class, so it is refused positionally before the slot is even read.
  ///
  /// The double demote is the one a caller can actually reach: `cst_demote` appends rather
  /// than rewrites, so the slot cannot witness an earlier demote of itself and the emission
  /// wall cannot refuse a **double demote** in a release build. It is refused here instead,
  /// by the canonicalization pass — the release half of the two-tier calibration whose
  /// at-cause half is `cst_demote`'s debug suffix scan (the prior `Demote`'s −1 dips the
  /// running sum). Both materialization doors refuse: canonicalization runs before the walk,
  /// and `finish_partial` tolerates incompleteness, not a corrupt stream.
  #[error(
    "demote event at index {index} targets {target}, which is not a live open node start (a \
     double demote, or a target that is not this bracket's own start)"
  )]
  StaleDemote {
    /// The buffer index of the `Demote` event.
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
  /// required — the dialect mapper or a raw caller leaked the reserved band.
  /// `record_token`, `cst_start`, and `cst_start_at` each `assert!` against it,
  /// unconditionally, in every build — the detect-at-cause form; this is the release wall.
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
  /// instead — the loud failure the wrapper contract promises.
  ///
  /// # What is NOT this shape
  ///
  /// The wall fires only where a byte is **unexplained** — the same evidence the
  /// [gap-coverage law](Self::UncoveredGap) uses, asked of the zero-token case so it can
  /// name it precisely. A parse that legitimately consumed nothing is therefore never
  /// refused here:
  ///
  /// - it builds no structure (nothing to refuse); or
  /// - its source is empty (nothing to consume); or
  /// - **every byte of a nonempty source is covered by a recorded lexer-error
  ///   diagnostic** — the source held nothing lexable, the lexer said so with its spans,
  ///   and the whole region tiles as `gap_kind`. This is an ordinary shape for a lossless
  ///   grammar, whose root production opens its document node before it can know whether
  ///   any token follows: `"unterminated` in an editor buffer reaches `finish` as exactly
  ///   one error span and one open-then-closed document. Refusing it made a half-typed
  ///   string a crash.
  ///
  /// A fatally-aborted parse inspected through [`Cst::finish_partial`](super::super::Cst::finish_partial)
  /// still has its open nodes as the abort witness and is likewise not refused.
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
  /// tree; [`Cst::finish_partial`](super::super::Cst::finish_partial) tiles it (the tooling door that tolerates
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
  ///
  /// # Which callers actually see this
  ///
  /// At `index: 0` this names the **root kind** `finish`/`finish_partial` were called with — a
  /// bare `u16` argument with no door in front of it, checked right here, unconditionally, in
  /// every build. That is the one construction site a caller outside this crate can reach: pass
  /// a root kind the dialect's own validator rejects and this is what comes back, in a release
  /// binary exactly as in a debug one.
  ///
  /// At every other index this names a node, token, or retro-wrap kind, and none of those ever
  /// arrive here from outside this crate. `cst_start`, `cst_start_at`, and the `record_token`
  /// body behind both token doors each assert the identical admission, unconditionally, in
  /// every build, before the event is ever buffered — so a bad kind panics there, at the emit
  /// site, and this walk never sees it. The per-event check downstream of those doors exists
  /// only to backstop the crate's own `pub(crate)` raw-injection test hook, the sole route that
  /// bypasses them, and it is `cfg!(debug_assertions)`-gated because that route is reachable
  /// only from this crate's own tests, never from a dependent crate, in either profile.
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

  /// The sink was asked for a rewind it had no correct way to perform and could not report:
  /// an **unpaired settle** (a truncating rewind to a mid-log mark no live row captured)
  /// detected while a panic was already unwinding, where panicking would have aborted the
  /// process. [`Sink::rewind`](crate::emitter::Emitter::rewind) panics on that condition in
  /// every build; mid-unwind it degrades to a total no-op and latches this instead.
  ///
  /// The refusal is the point. The events above the mark are still in the log and the inner
  /// emitter still holds their records, so the two channels do agree — with each other, and
  /// with a rollback that never happened. A tree built from them would be a *plausible* tree
  /// for a branch the parser abandoned, which is precisely the class of silent wrongness the
  /// unconditional wall exists to remove; a host that catches the original panic and asks for
  /// the tree anyway gets this error rather than that tree. Both doors refuse — this is
  /// corruption, not the incompleteness [`Cst::finish_partial`](super::super::Cst::finish_partial) tolerates.
  ///
  /// The fix is always at the call site: some capture is being settled twice, or a mark is
  /// being rewound to after it was released. See the `# Panics` section on
  /// [`Emitter::rewind`](crate::emitter::Emitter::rewind)'s `Sink` implementation.
  #[error(
    "a rewind to mid-log mark {mark} of a {len}-event log was refused mid-unwind (no live row \
     captured it, and reporting it would have aborted the process): the event log describes a \
     rollback that never happened"
  )]
  UnpairedSettle {
    /// The mark the refused rewind named.
    mark: u64,
    /// The event-log length at the refused rewind.
    len: u64,
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
#[inline]
fn w_tick() {
  w::tick();
}

/// The release form of [`w_tick`]: nothing.
#[cfg(not(test))]
#[inline]
const fn w_tick() {}

/// Charges `n` units of [`W`](w) at once: the honest price of one bulk library call whose
/// iteration happens where the counter cannot see it (today, exactly one — the `sort_unstable`
/// over gathered diagnostic spans). Charging the element count is what makes a sort migrating
/// into the per-event body visible: it would then cost `k` per event instead of `k` per finish.
#[cfg(test)]
#[inline]
fn w_tick_n(n: u64) {
  w::add(n);
}

/// The release form of [`w_tick_n`]: nothing.
#[cfg(not(test))]
#[inline]
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
  /// The body of [`Cst::finish`](super::super::Cst::finish), which is the public door; this
  /// twin is crate-private so the in-crate suites can materialize a sink they built directly.
  pub(crate) fn finish(self, root_kind: u16) -> (Result<GreenNode, FinishError>, E)
  where
    L::Offset: TryInto<u32>,
    L::Source: CstText,
  {
    self.materialize(root_kind, false)
  }

  /// The body of [`Cst::finish_partial`](super::super::Cst::finish_partial), which is the
  /// public door; crate-private for the same reason as [`finish`](Self::finish).
  pub(crate) fn finish_partial(self, root_kind: u16) -> (Result<GreenNode, FinishError>, E)
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
    // The poison, read before anything else: a sink that refused a rewind mid-unwind holds a
    // log describing a rollback that never happened, and every law below would pass over it
    // happily — the events are internally consistent, they are simply the wrong events. This
    // is the same posture the neighbouring emission-time walls take (a debug assert at cause,
    // a typed `FinishError` as the release backstop), except that here the at-cause report is
    // suppressed by the unwind rather than by the profile, which makes the backstop the ONLY
    // signal a caller can ever see. So it is checked through both doors: `finish_partial`
    // tolerates incompleteness, not corruption.
    if let Some(degraded) = self.degraded {
      return (
        Err(FinishError::UnpairedSettle {
          mark: degraded.mark,
          len: degraded.len,
        }),
        self.inner,
      );
    }

    let mut events = self.events;
    let has_demotes = self.demotes;
    let profile = self.profile;
    let inner = self.inner;
    // TriviaPolicy::AsEmitted is the only variant today: the replay below IS that policy
    // (tokens land in whichever node is open at their buffer position).
    let TriviaPolicy::AsEmitted = self.trivia;

    // The canonicalization pass, and the one place an interior kind write is unconditionally
    // safe: the sink has been consumed, so no `EventMark` and no checkpoint row can still be
    // live, and nothing can observe the buffer between here and the walk.
    //
    // The latch keeps the pass off a parse that never took a failing bracket exit — which is
    // EVERY parse of a predictive grammar, and is measured rather than assumed. The latch can
    // only ever be over-set (see `Sink::demotes`), so skipping here provably skips a pass with
    // nothing to do; the assert holds that direction rather than trusting it.
    debug_assert!(
      has_demotes || !events.iter().any(|ev| matches!(ev, Event::Demote { .. })),
      "the demote latch was clear over a buffer holding a Demote: canonicalization would be \
       skipped and an abandoned node would materialize"
    );
    if has_demotes && let Err(err) = canonicalize_demotes(&mut events) {
      return (Err(err), inner);
    }

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

/// Applies every surviving [`Event::Demote`] to its target, turning the abandoned node's
/// `StartNode` into the inert tombstone an unspent `cst_mark` would have left — the one pass
/// that makes the failing exit's *appended* event equivalent to the eager rewrite it replaced.
///
/// # Why here, and why it is not the banned interior write
///
/// The law forbids rewriting an earlier slot **during the parse**, because a truncation that
/// erases the deciding branch does not erase the slot the decision was written on. This pass
/// runs on the buffer `materialize` **owns**: the sink is consumed, every `EventMark` is
/// unspendable, every checkpoint row is gone, and no rewind can follow. It is the same bucket
/// the hole wrap's floored splice sits in — "provably beyond every live mark" — made absolute
/// rather than argued.
///
/// # What it refuses
///
/// A `Demote` whose target is not a live, un-demoted `StartNode`, **or does not sit strictly
/// below the `Demote` itself**. Through the validated emission surface exactly one shape
/// reaches that: a **double demote**, whose second event finds the slot its predecessor
/// already tombstoned. `cst_demote` cannot refuse it in a release build — appending leaves the
/// slot unchanged, so the emission wall has nothing to read — so this is the release half of
/// that misuse's two-tier calibration (the debug half is `cst_demote`'s own suffix scan, where
/// the prior `Demote`'s −1 dips the running sum). Every other shape — an out-of-range target,
/// a target holding a token, a `cst_mark` tombstone, a real-kind start carrying a
/// `forward_parent`, **a target at or above the `Demote`'s own index** — is unreachable
/// through the emission walls and refused here as the backstop for the raw in-crate injection
/// hook.
///
/// The positional shape is worth naming separately, because it is the only one whose residue
/// is *balanced*. A future target tombstones a start the walk has not opened yet, so the node
/// simply vanishes and no downstream check has anything to fire on; the others all leave a
/// stream that reads wrong somewhere. It is checked first, before the slot read, so the
/// narrowing to `usize` can never alias a distant target onto a nearby slot.
///
/// # Cost
///
/// One linear pass over the owned buffer with a single well-predicted branch per event, ahead
/// of a walk that reads the same events again. It is deliberately **not** ticked into the
/// [`W`](w) instrument: `W`'s obligation is over [`replay`] and its helpers, and folding an
/// out-of-walk pass into that counter would move its bounds for a reason unrelated to the
/// walk's own shape. It is linear by construction and stated here instead.
fn canonicalize_demotes<S>(events: &mut [Event<S>]) -> Result<(), FinishError> {
  for index in 0..events.len() {
    // `target` is a `Copy` scalar, so this read releases its borrow before the write below.
    let Event::Demote { target } = events[index] else {
      continue;
    };
    // The **positional** half of the documented invariant — `target` is strictly below the
    // `Demote`'s own index — mirroring the `StartAt` replay arm's `*target < index` term. The
    // content check below cannot stand in for it: a slot ahead of this event holds whatever the
    // stream put there, so a future-target demote would tombstone a start the walk has not
    // reached yet and erase its node silently, with a balanced buffer downstream and nothing
    // left to notice. Only raw injection can build that shape — the emission wall derives
    // `target` from a validated live slot strictly below the appended event — so no legal
    // stream is affected.
    //
    // Compared in `u64` space, ahead of the `as usize` narrowing below: on a 32-bit target that
    // cast truncates, so a `target` of, say, `2^32` would otherwise alias slot 0 and be
    // *accepted*. This compare refuses every such target before the narrowing can happen.
    if target >= index as u64 {
      return Err(FinishError::StaleDemote {
        index: index as u64,
        target,
      });
    }
    // A `forward_parent` on the target is refused with the rest: only `cst_start_at` writes
    // that field and its wall demands a tombstone, so a real-kind start can never carry one —
    // and a tombstoned target that does is a slot this demote does not own.
    match events.get_mut(target as usize) {
      Some(Event::StartNode {
        kind,
        forward_parent: None,
      }) if *kind != TOMBSTONE => *kind = TOMBSTONE,
      _ => {
        return Err(FinishError::StaleDemote {
          index: index as u64,
          target,
        });
      }
    }
  }
  Ok(())
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
/// The cursor is the whole of the repair. Gaps reach this function in source order, so an
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
    w_tick(); // W row 7
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

/// Tiles the uncovered run a committed token left open, at the last moment before a
/// **builder-visible** event would move the walk out of the node the token settled in.
///
/// This is the lookahead the second pass used to buy with a whole first pass. A run's far end
/// is the *next* token's start (or the source's end when no token follows), which the walk
/// cannot read at the trailing token — so the tile is deferred across the events that are
/// structurally silent (a `StartAt` declaration, a `Diag` slot, an unwrapped tombstone) and
/// forced here, before the first event that opens or closes a node. Nothing between the two
/// points touches the builder, so the run still lands in exactly the node that was open when
/// it opened — the placement `finish`'s *Where a gap lands* note states.
///
/// `probe` is a **monotone** cursor into `events`: it only ever moves forward, so the total
/// scanning over one materialization is bounded by the event count and no region is ever
/// examined twice (the same discipline as [`first_uncovered`]'s shared cover cursor). Regions
/// whose runs were resolved by a directly-following token are skipped outright — the cursor
/// starts at `from + 1` whenever that is further along than the cursor itself.
///
/// A run whose text does not slice the source (a following token span that starts off a
/// `char` boundary) is left untiled; the walk then refuses at that token's own index, exactly
/// as it did when the run list was precomputed.
///
/// `probe` and `covered` travel **by value, in and out** rather than behind `&mut`, and that
/// is deliberate: they are the walk's two hottest scalars, and letting their addresses escape
/// into a call inside the loop is enough to pin them to the stack for the whole
/// materialization. Measured on the 57.7 KB entry, the `&mut` shape gave back a third of what
/// deleting the gather pass won.
#[inline]
#[allow(clippy::too_many_arguments)]
fn tile_pending_run<S>(
  events: &[Event<S>],
  source: &str,
  source_len: u32,
  gap_kind: u16,
  from: usize,
  probe: usize,
  covered: u32,
  tiled: &mut Vec<(u32, u32)>,
  builder: &mut GreenNodeBuilder<'_>,
) -> (usize, u32)
where
  S: Span,
  S::Offset: TryInto<u32>,
{
  let mut at = probe.max(from + 1);
  let run_end = loop {
    w_tick(); // W row 2 — one monotone lookahead step
    match events.get(at) {
      // A span that does not fit `u32` supplies no end; the walk refuses at that token.
      Some(Event::Token { span, .. }) => break span.start().try_into().ok(),
      Some(_) => at += 1,
      // No token follows: the run ends where the source does.
      None => break Some(source_len),
    }
  };
  let Some(run_end) = run_end.filter(|&end| end > covered) else {
    return (at, covered);
  };
  let Some(gap) = source.get(covered as usize..run_end as usize) else {
    return (at, covered);
  };
  tiled.push((covered, run_end));
  builder.token(SyntaxKind(gap_kind), gap);
  (at, run_end)
}

/// The validating replay: **one pass — linear in events, plus one `k log k` ordering of
/// the recorded diagnostic spans**. Validates as it drives the builder.
///
/// # Why the coverage verdict is still order-independent
///
/// The obvious single pass is one that decides coverage from a monotone cursor as it meets
/// each `Diag`. It is wrong on this crate's own event streams, and the reason is worth
/// stating where the code is: **tokens are recorded at settle, lexer errors at lex**, and the
/// input layer's lookahead routinely settles a token *after* a diagnostic whose span lies to
/// its right. The combined stream is therefore not ordered by source position, and it is not
/// even stable — the same parse puts the same diagnostic at a different buffer index
/// depending on how far the caller peeked. (`input_ref`'s own cache-transparency oracle says
/// so in as many words: the token-event stream is exactly invariant under prefill, *"stronger
/// than diagnostics, which may hoist"*.) A cursor driven in emission order turns that skew
/// into an `OverlappingSpans` refusal on a legal parse, and makes the materialized tree a
/// function of the peek schedule.
///
/// So coverage is **not** decided during the walk. The walk *gathers* every recorded
/// lexer-error span (holding each to the token-span discipline before it is allowed to
/// license anything) and *records the runs it tiled*; the verdict is computed once, after the
/// walk, from the merged **set** of error spans against those runs. That is the property that
/// absorbs the skew — by construction, not by assumption — and it is exactly the property the
/// old gather pass supplied. Deferring it is free: the latch it produces
/// ([`FinishError::UncoveredGap`]) was already consumed only at the *end* of the walk, after
/// the balance and token-channel walls.
///
/// # What the gather pass used to own, and where each half went
///
/// - **the recorded error spans** and their validation: the `Diag` arm of the walk, at the
///   same index, feeding the post-walk `sort_unstable` + linear merge;
/// - **the uncovered runs**, read off the token spans: replaced by a *monotone lookahead*
///   ([`tile_pending_run`]), which resolves a run's far end only when a builder-visible event
///   is about to move the walk out of the node the run opened in. Never rescans, allocates
///   nothing per event, and skips outright every region whose run the next token resolves;
/// - **the integrity canaries** (`ReservedKind`, `StaleStartAt`, `DanglingForwardParent`, the
///   debug-gated dialect-kind check): every one of them is reachable in the walk at the same
///   index — the tombstone's `forward_parent` canary is literally the first hop of the chain
///   the walk already follows.
///
/// **The one behaviour that moved is error precedence on a malformed stream.** Two passes
/// meant every gather-class refusal outranked every walk-class refusal whatever their
/// indices; one pass reports the first violation *in buffer order*. Both orders are
/// arbitrary — each variant names its own offending index either way — and no stream that
/// was refused is now accepted, nor the reverse: the fused arms run each event's gather
/// checks ahead of its walk checks, so a single event's own precedence is unchanged.
///
/// # The walk
///
/// Drives the builder, and recovers each target's retro-wraps from **the target's own chain**
/// (`forward_parent` → newest `StartAt` → `prev` → …), newest-first, which is the order they
/// open in. That replaces a `BTreeMap<target, Vec<…>>` rebuilt on every `finish`. A flat
/// reachability bitset keeps the integrity the unconditional rebuild used to give for free: a
/// `StartAt` no chain reaches is refused, never silently dropped.
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
  // is `pub(crate)`. Keeping the check here for that route, in every build, costs a measured
  // +4.4% on ordinary materialization (an indirect call per event inside a tight builder
  // loop), which is the whole of it. So the honest trade, stated rather than hidden: a
  // RELEASE build whose event log was assembled by raw in-crate injection can materialize an
  // out-of-language kind. Debug-assertions test runs, and the CI profiles that run this cell,
  // refuse it; release-profile test runs do not exercise that wall.
  //
  // The +4.4% is this walk as it ships against the same tree with `cfg!(debug_assertions) &&`
  // deleted from exactly the three sites below: `finish_clean` (8 192 tokens), median of
  // sixteen interleaved paired rounds, positive in all sixteen, against a null control of two
  // byte-identical builds that spans -1.15%..+1.13%. It replaces a +8.3% taken on the two-pass
  // gather+walk shape this round superseded, where these three calls sat in the gather pass
  // rather than in the builder loop; the same deletion reads +1.1% there, a median inside that
  // control's own span. One code layout on one toolchain — low single digits, not a constant.
  //
  // `ReservedKind` is NOT gated — the tombstone band is a plain comparison, it costs nothing,
  // and it was a release wall.
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

  // The recorded lexer-error spans, gathered by the `Diag` arm below and merged into the
  // coverage set *after* the walk — the order-independence the two-pass shape used to buy
  // with a whole pass (see this function's own docs).
  let mut error_spans: Vec<(u32, u32)> = Vec::new();

  // Every uncovered run the walk actually tiled, in source order. The coverage verdict is
  // decided against this list once the walk is done; it is empty for a source every token
  // covers, which is the ordinary case — one `(u32, u32)` per gap, nothing per event.
  let mut tiled: Vec<(u32, u32)> = Vec::new();

  // The monotone lookahead cursor into `events` — see `tile_pending_run`.
  let mut probe: usize = 0;
  // Whether the last committed token may have opened an uncovered run whose far end is not
  // known yet. Resolved by the next token (for free, in the leading-run branch), or forced by
  // the next builder-visible event, or by the end of the walk.
  let mut pending_run = false;

  // ── The walk ────────────────────────────────────────────────────────────────
  let mut builder = GreenNodeBuilder::new();
  let mut stack: Vec<Frame> = Vec::new();
  builder.start_node(SyntaxKind(root_kind));
  stack.push(Frame::Root);

  // The tiling cursor: the end of the last covered source byte, in u32 space.
  let mut covered: u32 = 0;

  // The token-channel witnesses (see `StructureWithoutTokens`): whether any committed
  // token survived to materialization, and whether any real node did. Structure without a
  // single token over a nonempty source is the half-forwarding-wrapper signature — the
  // gap tiling below would otherwise dress it up as a plausible tree — but only where a
  // byte is left UNEXPLAINED; a token-free stream whose whole source a lexer error covers
  // is an honest parse of unlexable bytes, not a severed channel.
  let mut saw_token = false;
  let mut saw_structure = false;

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
    w_tick(); // W row 1
    let index = index as u64;
    match event {
      Event::StartNode {
        kind: TOMBSTONE,
        forward_parent,
      } => {
        // An inert mark — unless retro-wraps target it. They open HERE, newest first (the
        // later wrap's finish comes later, so it is the outer node), which is exactly the
        // order the chain is threaded in.
        //
        // A tombstone with no wrap is structurally silent, so it does not force a pending
        // run; one that opens wraps does — the run belongs to the node the trailing token
        // settled in, not inside the wrap about to open around it.
        if pending_run && forward_parent.is_some() {
          pending_run = false;
          (probe, covered) = tile_pending_run(
            events,
            source,
            source_len,
            gap_kind,
            index as usize,
            probe,
            covered,
            &mut tiled,
            &mut builder,
          );
        }
        let mut link = *forward_parent;
        let mut previous: Option<u32> = None;
        while let Some(relative) = link {
          w_tick(); // W row 3 — one chain hop
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
        if cfg!(debug_assertions) && !validator.admits(*kind) {
          return Err(FinishError::InvalidDialectKind { index, kind: *kind });
        }
        if pending_run {
          pending_run = false;
          (probe, covered) = tile_pending_run(
            events,
            source,
            source_len,
            gap_kind,
            index as usize,
            probe,
            covered,
            &mut tiled,
            &mut builder,
          );
        }
        saw_structure = true;
        builder.start_node(SyntaxKind(*kind));
        stack.push(Frame::Start(*kind));
      }
      Event::Token { kind, span } => {
        if *kind == TOMBSTONE {
          return Err(FinishError::ReservedKind { index });
        }
        if cfg!(debug_assertions) && !validator.admits(*kind) {
          return Err(FinishError::InvalidDialectKind { index, kind: *kind });
        }
        saw_token = true;
        let start = offset_to_u32(span.start(), index)?;
        let end = offset_to_u32(span.end(), index)?;
        if start < covered || end < start {
          return Err(FinishError::OverlappingSpans { index });
        }
        if end > source_len {
          return Err(FinishError::SpanOutOfBounds { index });
        }
        // Whatever run was pending is resolved right here, for free: this token's own start
        // is the far end the lookahead would have gone looking for, so the arm never consults
        // `pending_run` — it re-arms it below either way. The same branch tiles the **leading**
        // run — bytes before the first committed token, which no token trails, and which
        // therefore has no opening moment of its own.
        if start > covered {
          let gap = source
            .get(covered as usize..start as usize)
            .ok_or(FinishError::SpanOutOfBounds { index })?;
          tiled.push((covered, start));
          builder.token(SyntaxKind(gap_kind), gap);
        }
        let text = source
          .get(start as usize..end as usize)
          .ok_or(FinishError::SpanOutOfBounds { index })?;
        builder.token(SyntaxKind(*kind), text);
        covered = end;
        // This token may have OPENED a run — the bytes between it and whatever comes next that
        // no token will ever claim. Its far end is not knowable here, so the tile is armed and
        // forced at the last moment that still lands it in this node: the next builder-visible
        // event, or the end of the walk. Nothing between the two touches the builder, so the
        // run goes in the node the parse was in when it stopped covering the source, and no
        // later event can move it. The end of the stream is not a case — a run trailing the
        // last committed token is tiled before the finishes that close over it.
        pending_run = true;
      }
      Event::FinishNode { kind } => {
        if pending_run {
          pending_run = false;
          (probe, covered) = tile_pending_run(
            events,
            source,
            source_len,
            gap_kind,
            index as usize,
            probe,
            covered,
            &mut tiled,
            &mut builder,
          );
        }
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
        // A close decides nothing about gap placement, and that is deliberate. Two earlier
        // shapes of this rule read a trailing run's owner off *where the event log ends*
        // relative to this close — first "was it the last event to drive the builder", then
        // "was it the last event at all". Both made the answer a function of what happened
        // after the run was already determined, and each time a different ordering escaped.
        // The run is now tiled at the token it trails, before this close is even read.
        builder.finish_node();
      }
      Event::StartAt { kind, target, .. } => {
        // Its node was opened at the target's position (the hoist above); the declaration
        // slot itself is structural silence — except that this is the only place every
        // `StartAt` is guaranteed to be seen, so it is where its kind, its target and its
        // reachability are all asked.
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
      // Structural silence, exactly like an unwrapped tombstone or a `StartAt` declaration:
      // canonicalization has already applied this event to its target (and refused it if the
      // target was not the demote's own live start), so the slot it names is a tombstone the
      // walk will skip or has already skipped. It deliberately does NOT force a pending gap
      // tile — it touches the builder not at all, so the run still belongs to the node the
      // trailing token settled in, which is the placement the eager in-place demote produced
      // by emitting no event whatsoever. That equality is what keeps the corpus hashes exact.
      Event::Demote { .. } => {}
      Event::Diag {
        error_span: Some(span),
      } => {
        // A diagnostic order-slot is invisible to the tree, but a **lexer error's** span
        // covers the bytes it refused — and covering a byte is what *licenses* tiling it.
        // That makes the span evidence, so it gets the token-span discipline rather than a
        // clamp: a clamped endpoint quietly re-aims the evidence at a range the lexer never
        // refused, excusing exactly the dropped committed token `UncoveredGap` exists to
        // catch. The gap tile slices the source at these offsets, so they must also be char
        // boundaries.
        let start = diag_offset_to_u32(span.start(), index)?;
        let end = diag_offset_to_u32(span.end(), index)?;
        if start > end
          || end > source_len
          || !source.is_char_boundary(start as usize)
          || !source.is_char_boundary(end as usize)
        {
          return Err(FinishError::InvalidDiagnosticSpan { index });
        }
        // A zero-width span covers nothing, so it licenses nothing: legal, and dropped. The
        // rest are merged into the coverage set AFTER the walk, which is what keeps the
        // verdict a function of the set rather than of the arrival order.
        if start < end {
          error_spans.push((start, end));
        }
      }
      Event::Diag { error_span: None } => {}
    }
  }

  // The run that trails NO builder-visible event: the walk ran out before anything forced the
  // pending tile, so the node open here is still the node the run opened in. This also covers
  // the run that trails no token at all — a parse that committed nothing over a source with
  // bytes in it — which tiles into the ROOT under `finish` (a balanced stream has closed
  // everything else) and the INNERMOST OPEN node under `finish_partial` (this append
  // deliberately precedes the close loop below). Pinned by
  // `a_source_with_nothing_lexable_keeps_its_gap_at_the_root_either_way` and
  // `finish_partial_trailing_gap_tiles_into_the_innermost_open_node`.
  if covered < source_len {
    let gap = source
      .get(covered as usize..)
      .ok_or(FinishError::SpanOutOfBounds {
        index: events.len() as u64,
      })?;
    tiled.push((covered, source_len));
    builder.token(SyntaxKind(gap_kind), gap);
  }

  // ── The coverage verdict, once, after the walk ──────────────────────────────
  //
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
  w_tick_n(k * ceil_log2(k)); // W row 4
  error_spans.sort_unstable();
  let mut error_cover: Vec<(u32, u32)> = Vec::with_capacity(error_spans.len());
  for (s, e) in error_spans {
    w_tick(); // W row 5
    match error_cover.last_mut() {
      Some((_, last_end)) if s <= *last_end => *last_end = (*last_end).max(e),
      _ => error_cover.push((s, e)),
    }
  }

  // The leftmost source run no committed token covers and no lexer error explains — the
  // dropped-committed-token signature (see `UncoveredGap`). The tiled runs arrive in source
  // order, so one shared monotone cursor decides them all, and the FIRST answer is the one
  // the two-pass shape latched during the walk: the verdict is identical, only later.
  let mut cover_at: usize = 0;
  let mut first_uncovered_gap: Option<(u32, u32)> = None;
  for &(lo, hi) in &tiled {
    w_tick(); // W row 6
    first_uncovered_gap = first_uncovered(lo, hi, &error_cover, &mut cover_at);
    if first_uncovered_gap.is_some() {
      break;
    }
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
      w_tick(); // W row 8
      builder.finish_node();
    }
  } else if saw_structure && !saw_token && source_len > 0 && first_uncovered_gap.is_some() {
    // The token-channel wall: a *balanced* stream that builds structure without one
    // committed token over a nonempty source is the half-forwarding-wrapper signature
    // (structuring forwarded, `Emitter::commit_token` inherited as the core no-op), and
    // gap tiling would return a plausible tree instead of a witness. Balanced-only on
    // purpose: a fatally-aborted parse inspected through `finish_partial` still has its
    // open nodes (the `open > 0` arm above), so the abort shape is never refused here.
    // Kept ahead of the gap-coverage law so the all-dropped case earns this precise
    // message rather than an `UncoveredGap` over the whole source.
    //
    // `first_uncovered_gap.is_some()` is what keeps the wall from OVERFIRING, and it costs
    // the wall no detection power: a severed token channel records no lexer errors, so its
    // whole source is unexplained and the latch is always set — every stream this wall ever
    // caught, it still catches, `finish_partial`'s flavour included (the latch is recorded
    // during tiling on both doors). What the test removes is the one shape that shares the
    // wall's *symptom* and none of its cause: a token-free parse over a source the lexer
    // refused byte for byte and said so. That is the ordinary reading of `"unterminated` by
    // a lossless grammar whose root opens its document node before it can know whether any
    // token follows — refusing it turned a half-typed string into a crash.
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
/// | 1 | the walk over every event — the ONE pass | **ticked** (W row 1) |
/// | 2 | `tile_pending_run`'s lookahead for a run's far end | **ticked** (W row 2) — bounded by the event count in total, because the cursor is shared and monotone, and it starts at the forcing event whenever that is further along, so a region whose run the next token resolved is never examined at all |
/// | 3 | the retro-wrap chain hop | **ticked** (W row 3) |
/// | 4 | `error_spans.sort_unstable()` | **CHARGED AT COST** (W row 4): unticked inside `std`, so `k·⌈log₂ k⌉` — its real cost, not its element count — is added to `W`. One `O(k log k)` over **diagnostics**, `k ≤ events`, once per finish. Charging the element count instead would make the charged quantity linear by construction and leave the growth cell unable to fail |
/// | 5 | the cover-merge loop | **ticked** (W row 5) |
/// | 6 | the post-walk sweep of the tiled runs | **ticked** (W row 6) — one per tiled run, and it stops at the first uncovered one |
/// | 7 | `first_uncovered`'s sweep of the merged cover | **ticked** (W row 7) — and bounded by `cover.len() + gaps` in total, because the cursor is shared and monotone. This is the construct the defect made quadratic |
/// | 8 | the end-of-walk close `for _ in 0..open` | **ticked** (W row 8) — the loop the first instrument missed |
/// | 9 | reachability bitset `std::vec![0u64; events.len().div_ceil(64)]` | justified: one lazy `alloc_zeroed` of `ceil(events/64)` words, no per-event iteration, no keyed structure — allocated only when a chain is actually walked |
/// | 10 | `builder.start_node` / `token` / `finish_node`, `source.get`, `stack` push/pop | justified O(1) amortized per event |
/// | 11 | the tiled-run list `tiled` | justified: one `Vec::push` per *gap*, nothing per event, and the `Vec` is never allocated for a source every token covers |
///
/// Deleted by the single-pass rewrite: the gather loop over every event (its former W row 1),
/// the precomputed uncovered-run list and its cursor. Deleted by the chain rewrite before it:
/// the per-materialization `BTreeMap<target, Vec<(u64, u16)>>` — `O(log n)` per `StartAt` plus
/// one `Vec` allocation per target, and the last keyed structure in the walk. The one
/// superlinear term left is the sort, `O(k log k)` in the number of *diagnostics*, and it is
/// charged at that cost rather than at its element count — so the gate can observe it. With
/// one recorded lexer error per token, `k` is `Θ(events)` and the whole materialization is
/// `O(n log n)`; the law is stated in that shape rather than claiming a linearity the ordering
/// does not provide.
///
/// Measured before the quadratic fix, on the error-dense payload (n = 100 / 400 / 1600):
/// `W` = 5 550 / 82 200 / 1 288 800 against the bound 900 / 3 600 / 14 400, growing 14.81× /
/// 15.68× per 4× of input. Two-pass, after that fix: `7n − 1 + k·⌈log₂ k⌉`. Single-pass:
/// `6n − 1 + k·⌈log₂ k⌉` on the same payload — the gather pass gone, and the lookahead that
/// replaced it never fired, because every run there is resolved by the token that follows it.
#[cfg(test)]
pub(crate) mod w {
  use core::cell::Cell;

  thread_local! {
    static W: Cell<u64> = const { Cell::new(0) };
  }

  /// One unit of replay work.
  #[inline]
  pub(crate) fn tick() {
    W.with(|w| w.set(w.get().saturating_add(1)));
  }

  /// `n` units at once — see [`w_tick_n`](super::w_tick_n).
  #[inline]
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
