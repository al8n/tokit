//! The rewindable event sink: [`Sink`] wraps any inner emitter, buffers the CST event
//! stream, and rewinds it under the **same** mark that rewinds diagnostics — one timeline,
//! every channel (see the [`event`](super::event) module for the vocabulary and its laws).
//!
//! # CELL_CENSUS — every mutable cell of the sink, and its class
//!
//! The input layer's cell taxonomy (see `input::lineage`) governs the sink's cells too: a
//! new cell lands here classified, or [`census`] fails to compile. The classes:
//!
//! | # | Cell | Class | Rewind/restore semantics |
//! |---|------|-------|--------------------------|
//! | E1 | [`Sink::events`] | **ground truth** (a second emission log) | append + suffix-truncate to the mark — the same two verbs as `Verbose`'s log (plus the one censused prefix-preserving splice of the hole wrap, entirely above every live mark, and the one censused interior write: `cst_start_at`'s journaled `forward_parent` link — see the law in [`event`](super::event)) |
//! | — | [`Sink::demotes`] | **latching hint** (an `Event::Demote` was appended at some point) | never restored, never cleared — a rewind may leave it set over a buffer with no `Demote` left, which costs one no-op canonicalization pass at materialization and can never skip a needed one |
//! | E2 | [`Sink::journal`] | **undo journal** (the Verbose-parallel-maps discipline lifted to events) | rewind pops entries written above the mark, reverse order, restoring each overwritten `forward_parent`; never grows on rewind |
//! | E3 | [`Sink::ledger`] | **monotone era source + truncation witness** | rewind APPENDS to it (a rewind *is* a truncation) and never removes; rewinding it would false-accept a stale mark |
//! | E4 | [`Sink::rows`] | **release stack + per-checkpoint depth ledger + inner reading** | push at `checkpoint()` (freezing the depth and the inner emitter's own checkpoint reading), pop at `release()` (kept) and `rewind()` (spent, the popped row's inner reading is the inner's rewind target); depth entries are frozen facts about prefixes, never live counters |
//! | E5 | [`Sink::depth`] | **live open-node depth** — the one value derived from the log that the sink maintains rather than recounts | every append charges the event's own `depth_delta`, through the ONE push helper the DEPTH_CENSUS pins; a truncating rewind **restores** it from the target row's frozen depth (or `0` at the origin), and a non-truncating one leaves it untouched |
//! | E6 | [`Sink::diag_tail`] | **live start of the pure-`Diag` tail** — the second value maintained rather than recounted, and restored by the identical rule | every append through the ONE push helper sets it to the new length for a non-`Diag` and leaves it for a `Diag`; the hole wrap's splice charges its own half; a truncating rewind **restores** it from the target row's frozen value (or `0` at the origin), and a non-truncating one leaves it untouched |
//! | E7 | [`Sink::opens`] (debug only) | **open-node chain** (a second append-only log, one entry per depth-increasing event, each carrying a parent link, its depth and a skew-binary skip link) | append + suffix-truncate, the log's own two verbs — a rewind drops the entries whose `start` reached the truncated region and touches no other, so it needs no journal; an entry is written once and never revisited, and a close never reclaims a slot (see [`OpenNode`]) |
//! | E8 | [`Sink::open_top`] (debug only) | **live chain head** — the third value maintained rather than recounted | the ONE push helper pushes at a `+1` and follows `parent` at a `-1`; a truncating rewind **restores** it from the target row's frozen value (or `None` at the origin), and a non-truncating one leaves it untouched |
//! | — | [`Sink::floor_mark`] | derived memo (the newest released row's mark) | reset to the surviving top row's mark when a rewind drops below it |
//! | — | [`Sink::base_inner`] | derived memo (the inner's construction-time reading) | primed at the first advancing touch (provably the construction reading), never restored (the exact no-row target at the origin only) |
//! | — | [`Sink::degraded`] | **latching poison** (a rewind the sink refused to perform) | set — never cleared — by the one rewind that must degrade instead of report (an unpaired settle detected mid-unwind); a rewind cannot un-refuse an earlier refusal, so it is deliberately outside the rewind timeline |
//! | — | `inner`, `source`, `profile`, `trivia` | configuration / the wrapped emitter | never touched by rewind (the inner rewinds through its own contract) |
//! | — | `witness` | sink identity (validated at every mark spend, every build) | never restored |
//!
//! # Open-node depth: maintained, and what pays for the restore rule
//!
//! Depth used to be **recounted** on every query — sum the `depth_delta`s above the nearest
//! frozen `(mark, depth)` fact — on the reasoning that a cached counter would need its own
//! restore rule while a derived one is restored by truncation for free. The restore rule turned
//! out to be free too, and the recount did not: the frozen facts are minted by
//! [`checkpoint`](Emitter::checkpoint)/[`release`](Emitter::release), and nothing in the CST
//! channel mints one. The blessed [`node`](crate::parser::node) bracket takes no emitter
//! checkpoint, so a predictive grammar emitting `n` flat sibling nodes left `rows` empty and
//! the floor at `0` and recounted the whole accumulated prefix once per close —
//! `3n(n − 1)/2 + 2n` event visits for `n` siblings, measured exactly (al8n/tokora#253).
//!
//! So [`depth`](Sink::depth) is a live scalar now, and the restore rule is the three lines the
//! rewind contract already forced into existence:
//!
//! - a rewind that truncates **to a captured row** takes that row's frozen `depth`, which is
//!   the depth of exactly the prefix that survives;
//! - a rewind that truncates **to the origin** takes `0` — an empty log has no open node;
//! - every other rewind truncates nothing ([`rewind`](Emitter::rewind)'s future-mark guard, its
//!   rewind-to-current arm, and its mid-unwind degradation), so the scalar is already right.
//!
//! The fourth case — a truncating rewind to a mid-log mark no row captured — has no restore
//! value, and needs none: it is an unpaired settle, refused by the preflight in every build for
//! the inner emitter's sake long before depth was a cell.
//!
//! What keeps the scalar exact is not discipline but arity: **one** helper appends to the log
//! ([`push_event`](Sink::push_event)) and charges the event's own `depth_delta`, and the one
//! non-append mutation (the hole wrap's splice) charges its two halves explicitly. DEPTH_CENSUS
//! — a source lock in this module's tests, in the shape of CST_FORWARD_CENSUS — fails the build
//! if a second `events.push` or `events.insert` appears.
//!
//! # The pure-`Diag` tail: the second value the same arity makes maintainable
//!
//! [`wrap_hole`](Sink::wrap_hole)'s backward scan steps over [`Event::Diag`] *unconditionally* —
//! that transparency is what lets a recovery hole wrap tokens that already carry diagnostics —
//! so a `Diag` contributes nothing to the scan's answer but one loop iteration. When a hole's
//! span matches no buffered token the wrap appends **no structural event**, and the diagnostic
//! run it leaves behind was then rescanned in full by the next call: `Θ(H²)` event visits for
//! `H` such holes, **in every profile including release**, measured at exactly `H(H − 1)/2`
//! (al8n/tokora#305). The ordinary case never had the problem, because a hole that *does* wrap
//! tokens appends a `StartNode`/`FinishNode` pair and those break the next scan.
//!
//! [`diag_tail`](Sink::diag_tail) is the bound that makes the empty case behave like the
//! ordinary one: the index at which the buffer's **pure-`Diag` tail** begins, so the scan enters
//! at that index instead of at the length. It skips exactly the iterations that would have
//! `continue`d and none that would not, so **no wrap is narrowed and no tree changes** — this is
//! a scan that stops re-deciding what it already decided, not a new boundary on the wrap's
//! structural authority. The event immediately below the entry point is a non-`Diag` by the
//! cell's own invariant, so an empty wrap breaks on its first iteration and every scan's run is
//! disjoint from every other's: `O(1)` per empty hole, `O(len)` over a parse.
//!
//! Its restore rule is [`depth`](Sink::depth)'s, verbatim and for the same three reasons — a
//! truncating rewind to a captured row takes the row's frozen value (the tail of exactly the
//! prefix that survives), one to the origin takes `0` (an empty log's tail starts at `0`), and
//! every other rewind truncates nothing. Rewind is where a watermark of this kind usually
//! breaks; this one inherits a rule the rewind contract had already forced into existence, and
//! the same one-append-door arity DEPTH_CENSUS pins keeps it from drifting by omission.
//!
//! # The open-node chain: what a scalar depth provably cannot answer
//!
//! [`cst_demote`](CstEmitter::cst_demote)'s debug wall asks whether the marked node is **still
//! open**, and used to answer it by recounting `events[index + 1..]` to the end of the buffer,
//! looking for a running sum that dips below zero — with **no early exit on the success path**.
//! For `d` nested failing brackets that is `Θ(d × len)`, measured at exactly `d(d − 1)` visits
//! (al8n/tokora#306).
//!
//! **The maintained [`depth`](Sink::depth) cannot supply what that scan wants, and neither can
//! any early exit.** The scan needs the *running minimum* of the suffix, and a scalar total is
//! blind to a dip that recovers: `Start(A) … Finish(A) … Start(C) …` ends at the same depth it
//! started, and the marked node is closed anyway. Nor can the walk stop short — the deltas are
//! `{-1, 0, +1}`, so from any position both a later dip and no later dip stay reachable with the
//! events that remain, in either direction of travel. What the *total* does decide, in `O(1)`,
//! is only the sub-case where the endpoint itself is low; the recovering dip is exactly the
//! residue, and skipping it would weaken the wall rather than accelerate it.
//!
//! So the structure is [`opens`](Sink::opens) + [`open_top`](Sink::open_top): the ordinary
//! bracket-matching stack, laid out as an append-only vector of parent links so that its
//! *contents* are a function of the event prefix and its *head* is one restorable scalar.
//!
//! **The equivalence.** Write `D(j)` for the depth after event `j`. The chain pushes at every
//! `+1` and follows `parent` at every `-1` (a `-1` with an empty chain is a no-op, which is the
//! negative depth the raw surface already admits). An entry pushed at `i` therefore leaves the
//! chain exactly when `D` first returns to `D(i) − 1` above `i`; and because every delta is in
//! `{-1, 0, +1}`, `D` cannot pass below `D(i) − 1` without landing on it. Hence
//!
//! > `i` is on the chain  ⟺  `min_{j > i} D(j) ≥ D(i)`  ⟺  the old scan finds no dip,
//!
//! and the wall's verdict is unchanged on every input, including the interleaved-closings shape
//! the scan deliberately refuses on the raw surface. The **diagnosis** still comes from the
//! scan: the failing path runs the old walk to name which of the three shapes it is, where its
//! cost is the panic's, not the parse's.
//!
//! Rewind, again, is the whole risk, and again the answer is the rule that already existed: the
//! vector is append-only and index-keyed, so the truncation that removes the events removes
//! their entries and renames none of the survivors, and the head is a frozen row fact copied
//! back exactly like the depth. Release pays nothing for either — the check, the chain and the
//! row field are all behind `debug_assertions`, and release's wall is the typed refusal both
//! finish doors already raise.
//!
//! ## The chain is *searched*, not walked — and why `O(depth)` was not the answer
//!
//! Membership was first answered by following `parent` one link at a time, which is `O(depth)`.
//! Through the blessed [`node`](crate::parser::node) combinator that is a bounded constant —
//! `d` is grammar nesting depth and `RecursionLimiter` caps it — but **the raw `CstEmitter`
//! surface is not bounded by that limiter at all**, and the query is reached from it directly.
//! A hand-rolled sequence that keeps the chain long and demotes near its *bottom* is enough:
//! open `n` same-kind nodes, then for `k = 0 … n − 2` demote the `k`-th and open a fresh one,
//! then close `n` times. Every mark is distinct and live, so every query succeeds and the
//! failing path's suffix scan never runs; the chain never gets shorter; and the `k`-th walk
//! passes `n − k` links. `Θ(n²)`, in the same profile and on the same wall the recount was —
//! i.e. the *shape* of #306 survived its first repair, on the surface #306 named as a
//! requirement. The measured shape hid it: closing innermost-first means every query hits the
//! chain's own head and reads exactly one entry, which is `O(1)` for a reason that has nothing
//! to do with the structure.
//!
//! The repair is that each entry also carries its `depth` and a **skip link**, laid down by
//! Myers' skew-binary rule (see [`skip_link_for`](Sink::skip_link_for)): hops of length
//! 1, 1, 3, 1, 1, 3, 7, … whose defining property is that the chain can be *descended* in
//! `O(log depth)` steps rather than walked in `O(depth)`. The query
//! ([`probe_open_chain`](Sink::probe_open_chain)) takes an entry's skip when the landing point
//! is still at or above the queried index and its `parent` otherwise; since `start` strictly
//! decreases along the chain and neither move can step past the target, the search meets the
//! index if the chain holds it and otherwise lands on the first entry below it. **`O(log
//! depth)`, and the verdict is the walk's on every input** — the ladder changes how the chain
//! is traversed, not what is on it.
//!
//! **It needs no restore rule of its own, and that is the point.** `depth` and `jump` are
//! written once at the push, from entries that are already frozen, and never revisited — a skip
//! link is always a *proper ancestor* on its own entry's chain (it is either the parent or a
//! composition of two ancestor hops). So a truncating rewind that restores
//! [`open_top`](Sink::open_top) from the target row restores that head's entire ladder along
//! with its parent chain: every entry the search can reach is an ancestor of the head, every
//! ancestor's `start` is below the mark, and the pop loop drops exactly the entries at or above
//! it. The alternative — a depth-indexed spine, which would answer in `O(1)` — was rejected
//! for exactly the property the ladder has: a spine is *overwritten* by later opens at the same
//! depth, so a rewind would have to rebuild it in `O(depth)`, moving the same cost from the
//! query onto every truncating rewind. Growth is unchanged and bounded by the log: one entry
//! per depth-increasing event in [`events`](Sink::events), dropped when that event is, so
//! `opens.len() ≤ events.len()` always, and the two new fields cost 8 bytes per entry in debug
//! and nothing in release.

use core::{marker::PhantomData, num::NonZeroU32};

use std::vec::Vec;

use crate::{
  Lexer,
  emitter::{
    CstEmitter, Emitter, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnclosedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
    ValueKeyedEmitter,
  },
  error::{
    syntax::{FullContainer, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingTokenOf, UnexpectedTokenOf},
  },
  input::Cursor,
  span::{Span, Spanned},
  token::Token,
  utils::CowStr,
};

// The sink forwards the pratt channel too, but only when that family exists.
#[cfg(feature = "pratt")]
use crate::{
  emitter::PrattEmitter,
  error::{UnexpectedEoLhs, UnexpectedEoRhs},
};

use super::{
  event::{Event, EventMark, TOMBSTONE, TruncationLedger},
  profile::CstProfile,
};

pub use finish::FinishError;

mod finish;

/// How the sink places trivia tokens at materialization.
///
/// The default — and, in this version, only — policy is the provable one:
/// **innermost-open-node-at-commit** (call-site placement). A committed trivia token
/// materializes into whichever node was open when it settled, which is deterministic (a
/// function of the event prefix), cache-transparent (the scanner is origin-blind), and
/// exactly what capturing padded atoms already encode. This is deliberately **not** the
/// Roslyn/Swift leading-attaches-forward policy; a token-attached view is a later
/// materialization-time extension, which is why the enum exists at all.
#[derive(
  Debug, Default, Clone, Copy, PartialEq, Eq, Hash, derive_more::IsVariant, derive_more::Display,
)]
#[display("{}", self.as_str())]
#[non_exhaustive]
pub enum TriviaPolicy {
  /// Trivia tokens land exactly where they were emitted: inside the innermost node open
  /// at their commit position.
  #[default]
  AsEmitted,
}

impl TriviaPolicy {
  /// The canonical name of this policy.
  #[inline(always)]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::AsEmitted => "as_emitted",
    }
  }
}

/// One row of the sink's mark stack: an emitter checkpoint capture, with the derived
/// open-node depth frozen at capture time. Rows are spent by exactly one of
/// [`release`](Emitter::release) (the branch was kept) or [`rewind`](Emitter::rewind) (the
/// branch was abandoned) — the settle discipline the input layer's RELEASE_CENSUS locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkRow {
  /// The captured mark: the event-log length at capture time.
  mark: u64,
  /// The open-node depth at capture time — a frozen fact about the `mark`-length prefix,
  /// snapshotted from [`Sink::depth`] and never touched again.
  ///
  /// This is what makes the live scalar restorable: a truncating rewind to this row's mark
  /// leaves exactly the prefix this number describes, so the restore is a copy rather than a
  /// recount (see the depth section in the module docs).
  depth: i64,
  /// The start of the pure-`Diag` tail at capture time — a frozen fact about the `mark`-length
  /// prefix, snapshotted from [`Sink::diag_tail`] and never touched again.
  ///
  /// The [`depth`](Self::depth) field's twin, and restorable for the identical reason: a
  /// truncating rewind to this row's mark leaves exactly the prefix this number describes, so
  /// the restore is a copy rather than a backward rescan (see the `Diag`-tail section in the
  /// module docs).
  diag_tail: u64,
  /// The open-node chain's top slot at capture time — the third frozen fact about the
  /// `mark`-length prefix, and restored by the same rule as the two above.
  ///
  /// Debug-only with the chain it indexes: the only reader is
  /// [`validate_start_mark`](Sink::validate_start_mark)'s `debug_assertions` wall, and a release
  /// build must not pay a row field for a check it never runs.
  #[cfg(debug_assertions)]
  open_top: Option<u32>,
  /// The inner emitter's own checkpoint reading captured at this sink checkpoint. Handed
  /// back to `inner.rewind` when this row is the rewind target, restoring the inner to
  /// exactly its state when the mark was taken — every forwarded token AND diagnostic before
  /// the mark survives, every one after is undone. (Pre-fix the inner target came from the
  /// last surviving `Diag` slot, which missed tokens forwarded after the last diagnostic —
  /// the desync.)
  ///
  /// This is a plain `u64` reading, spent when the row is popped — there is no inner-side
  /// resource on the row, so `release` (which also pops the row) leaks no inner checkpoint.
  /// The mechanism assumes a **value-keyed** inner: `checkpoint` a pure monotone reading,
  /// `rewind` a drop-by-value, and `release` a no-op — the `Verbose`/token-tracking shape.
  /// (A table-keyed inner that allocated per `checkpoint` was already unsupported pre-fix:
  /// `forward_diag` then captured `inner.checkpoint()` per diagnostic with no matching
  /// release.) See the *Inner-emitter contract* section on [`Sink`].
  inner: u64,
}

/// The one rewind a sink can be asked for and be unable to perform: an **unpaired settle**
/// detected while a panic is already unwinding, where reporting it would abort the process
/// (see the `# Panics` section on [`rewind`](Emitter::rewind)). Recorded rather than merely
/// skipped, because the alternative is a sink whose later materialization is
/// indistinguishable from a clean one.
///
/// Carries the two numbers that name the violation, so the typed refusal
/// ([`FinishError::UnpairedSettle`]) is as diagnosable as the panic would have been.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DegradedRewind {
  /// The mark the rewind named — mid-log, and captured by no live row.
  mark: u64,
  /// The event-log length at the refused rewind.
  len: u64,
}

/// One link of the **open-node chain** — the debug-only structure that answers
/// [`cst_demote`](CstEmitter::cst_demote)'s "is the node this mark opened still open?" without
/// rescanning the event suffix (al8n/tokora#306).
///
/// One entry per depth-increasing event still in the log, appended in event order and dropped by
/// suffix truncation, so the chain is maintained with the log's own two verbs and needs no undo
/// journal. The **currently open** nodes are the entries reachable from [`Sink::open_top`]
/// through `parent`; the rest describe nodes that have since closed, and are kept because a
/// rewind can reopen one.
///
/// **Never reclaimed at a close, and that is load-bearing.** Popping an entry when its node
/// closes would hold the vector at exactly the live depth for a well-nested document, and it is
/// unsound: the next open reuses the freed slot, while the frozen [`MarkRow::open_top`] of a
/// checkpoint taken before the close still names it — a rewind would restore a top pointing at
/// an unrelated node. A slot is stable for the life of the event it describes, exactly as an
/// event index is.
///
/// **Every field is written once, at the push, and never again.** That immutability is what
/// makes the whole chain a *frozen fact about the event prefix*: restoring
/// [`Sink::open_top`] to a slot restores that slot's entire chain — links, depths and skips
/// alike — with no fix-up pass, which is the property the rewind contract needs and the reason
/// [`jump`](Self::jump) can be added without a second restore rule.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenNode {
  /// The event index of the depth-increasing event that opened this node.
  start: u64,
  /// The slot of the node that was innermost-open when this one opened — the chain's next
  /// link — or `None` when it opened at the log's outermost level.
  parent: Option<u32>,
  /// How many entries sit below this one on its chain: `0` at the log's outermost level, and
  /// `parent.depth + 1` otherwise. Bounded by [`Sink::opens`]'s length, which the push door
  /// already holds under `u32::MAX`.
  depth: u32,
  /// The **skip link**: a proper ancestor on this entry's own chain, or `None` for a skip that
  /// lands past the outermost entry.
  ///
  /// Chosen by Myers' skew-binary rule — `jump(parent)`'s own jump when the two hops it
  /// composes are the same length, and `parent` otherwise — which is what makes the chain
  /// searchable in `O(log depth)` instead of walked in `O(depth)` (al8n/tokora#306's residue;
  /// see the open-chain section in the module docs for why `O(depth)` was not enough).
  jump: Option<u32>,
}

/// One undo-journal entry: an in-place `forward_parent` write performed by
/// [`cst_start_at`](CstEmitter::cst_start_at) on its target tombstone, recorded so a rewind
/// can reverse it. Parse-time in-place event mutation is otherwise banned by law; this one
/// acceleration field is the single **journaled** exception, and it is legal *only because*
/// every write is journaled.
///
/// [`cst_demote`](CstEmitter::cst_demote) needs no entry here, and not because its write is
/// exempt: it **performs no parse-time write at all**. The failing exit appends an
/// `Event::Demote` naming its own start, exactly as `cst_start_at` appends a `StartAt` naming
/// its target, and materialization applies it. An appended event needs no undo — the
/// truncation *is* the undo. See the law in [`event`](super::event).
#[derive(Debug, Clone, Copy)]
struct JournalEntry {
  /// The event-log length immediately after the append that carried this write (the
  /// `StartAt`'s own index + 1). A rewind to any mark below it must reverse the write.
  at_len: u64,
  /// The absolute index of the mutated tombstone.
  index: u64,
  /// The `forward_parent` value the write overwrote.
  old_forward_parent: Option<NonZeroU32>,
}

/// The ceiling on the event-log capacity `Sink::new` reserves up front: **65 536 events**,
/// which is 2 MiB at the log's 32-byte element.
///
/// A cap is what makes sizing from the source length safe rather than merely usual. One event
/// per byte is the *measured* ratio for a lossless grammar (0.80–1.11 across this crate's
/// corpora, because a trivia-surfacing lexer — the only kind that can construct a sink — has
/// to cover every byte with a token), but it is a typical figure and not a law: a grammar
/// whose tokens are long, one string literal per kilobyte, would reserve gigabytes for a large
/// source without one.
///
/// Above the cap the reservation stops and the `Vec` resumes doubling from there, which costs
/// at most the handful of reallocations that remain — the many small ones, the expensive part,
/// are already gone. The over-reservation an atypical grammar still pays is address space, not
/// memory traffic: `Vec::with_capacity` does not write the pages it asks for, so a log that
/// never reaches the capacity never touches them.
const EVENT_CAPACITY_CAP: usize = 1 << 16;

/// The event-log capacity to reserve for a source of `len` bytes: **the capacity the `Vec`'s
/// own doubling would have arrived at**, taken in one step.
///
/// Rounding up to a power of two is not slack for its own sake — it is the whole of why the
/// reservation is free. A growing `Vec` doubles, so its *final* allocation is already a power
/// of two ≥ the event count; asking for that up front buys the identical block and skips every
/// intermediate copy. Reserving the raw length instead is the trap this exists to avoid:
/// measured at 1.11 events per byte, a 57.7 KB document would reserve 1.8 MiB, overrun it, and
/// then pay a 3.7 MiB reallocation on top — **slower than reserving nothing at all**, which is
/// how this function came to be written this way.
///
/// Zero stays zero: an empty source allocates no log, exactly as `Vec::new` did.
#[inline]
const fn event_capacity_for(len: usize) -> usize {
  if len == 0 {
    return 0;
  }
  let capped = if len > EVENT_CAPACITY_CAP {
    EVENT_CAPACITY_CAP
  } else {
    len
  };
  capped.next_power_of_two()
}

/// Mints a process-unique sink witness id (1-based; 0 is the inert mark's reserved id).
///
/// Unconditional on purpose: the witness is the **every-build** half of mark validation —
/// two sinks' `(index, era)` pairs coincide trivially (two fresh sinks both mint `(0, 0)`),
/// so a build without the identity check would let a foreign mark wrap an unrelated
/// history. A monotone counter rather than an address: sinks move, and a dead sink's
/// address can be reused, but a counter id is never reissued for the process's life —
/// **never** reissued: exhaustion aborts (see [`bump_witness`]), it does not wrap.
/// (`rowan` implies `std`, and the atomic is as available as the `Arc`s rowan itself uses.)
fn next_sink_witness() -> usize {
  use core::sync::atomic::AtomicUsize;
  static NEXT: AtomicUsize = AtomicUsize::new(1);
  bump_witness(&NEXT)
}

/// Allocates the next id from `next` **without ever wrapping**: a compare-exchange loop that
/// panics rather than roll `usize::MAX` over to `0`. A wrap would be doubly wrong — `0` is the
/// inert-mark id (a foreign inert mark would then validate), and every id after it reissues a
/// live one (a stale mark from an earlier sink would validate on a later one). So the counter
/// is never reissued in *any* build: the horizon is `usize::MAX` (2^64 on 64-bit targets,
/// where all mainstream `rowan`/`std` builds run) and its exhaustion is a loud abort, not a
/// silent reuse — the wrong-tree class the witness exists to kill.
fn bump_witness(next: &core::sync::atomic::AtomicUsize) -> usize {
  use core::sync::atomic::Ordering;
  let mut current = next.load(Ordering::Relaxed);
  loop {
    let bumped = current.checked_add(1).expect(
      "Sink witness counter exhausted: usize::MAX sinks were minted in one process. The \
       witness is never reissued (a wrap to 0 is the inert-mark id and would let a foreign \
       mark validate), so exhaustion aborts instead of rolling over.",
    );
    match next.compare_exchange_weak(current, bumped, Ordering::Relaxed, Ordering::Relaxed) {
      Ok(_) => return current,
      Err(actual) => current = actual,
    }
  }
}

/// The recording CST emitter: wraps an inner emitter `E`, forwards every diagnostic to it,
/// and buffers the event stream — one rewindable timeline for tree and diagnostics alike.
///
/// # One mark, every channel
///
/// [`checkpoint`](Emitter::checkpoint) is the event-log length: one positional mark over one
/// unified log, exactly `Verbose`'s architecture. Every diagnostic forwarded to the inner
/// emitter occupies a [`Diag`](super::event) slot *inside* the event buffer (appended by one
/// census-marked helper, on `Ok` and `Err` alike), so [`rewind`](Emitter::rewind) is:
/// truncate the buffer to the mark, reverse-replay the undo journal, and rewind the inner
/// emitter to the reading its mark-stack row captured at the checkpoint. The mark stack holds
/// exactly the live captures — the only per-checkpoint table — because every capture is spent
/// by exactly one of `rewind` (abandoned) or [`release`](Emitter::release) (kept), and it
/// carries the inner's own checkpoint reading so both channels rewind under the one mark.
///
/// # Composition
///
/// The sink forwards the **entire** emitter trait family — core [`Emitter`], the atomic
/// capability traits ([`TooFewEmitter`], [`TooManyEmitter`], [`FullContainerEmitter`],
/// [`SeparatedEmitter`] and its four leading/trailing refinements), and [`PrattEmitter`] —
/// so any context bound satisfied by `E` is satisfied by `Sink<E>`. It exposes the inner
/// emitter by shared reference only ([`inner_ref`](Self::inner_ref)); there is **no** `&mut`
/// accessor, because a caller who could rewind the inner emitter directly would shear the
/// event log from the diagnostic log with no witness. Materialization
/// ([`Cst::finish`](super::Cst::finish) / [`Cst::finish_partial`](super::Cst::finish_partial))
/// consumes the handle and returns the inner emitter with the tree.
///
/// # Inner-emitter contract: value-keyed checkpoint readings
///
/// The sink composes with its wrapped emitter through checkpoint *readings*, never mark
/// *resources*: [`checkpoint`](Emitter::checkpoint) captures `inner.checkpoint()` onto the
/// mark-stack row as a plain `u64` fact, [`rewind`](Emitter::rewind) hands a captured reading
/// back to `inner.rewind`, and [`release`](Emitter::release) pops the sink's own row **without
/// forwarding** — the inner is never told about kept branches. The wrapped emitter must
/// therefore be **value-keyed**, the trait's reference shape
/// ([`Verbose`](crate::emitter::Verbose), [`Fatal`](crate::emitter::Fatal),
/// [`Silent`](crate::emitter::Silent), [`Ignored`](crate::utils::marker::Ignored), and every
/// `Verbose`-shaped collector): `checkpoint` is a pure monotone reading of the emission state
/// (no per-call allocation), `rewind` restores by value — reclaiming everything above the
/// mark as a range — and `release` needs nothing forwarded, because a kept reading is just a
/// number going out of scope. The reading is a fact about the inner's one emission timeline,
/// whichever `Lang` instantiation reads it (the trait's one-timeline law). The sink hands
/// `inner.rewind` only readings it knows exactly — a row's capture, or the construction-time
/// base for a full unwind to the origin; a rewind that truncates nothing never touches the
/// inner, and a truncating rewind to a mid-log mark no row captured — an **unpaired settle**,
/// which is a parser bug — **panics in every build** (the sink never fabricates a reading).
/// The panic is raised by a preflight, before this method's first mutation, so a caught panic
/// leaves the sink exactly as it was rather than half-rewound. The sole exception is a panic
/// already unwinding, where reporting would abort the process instead: there the rewind
/// degrades to a total no-op and **latches**, and materialization then refuses through both
/// doors ([`FinishError::UnpairedSettle`]) rather than return the tree of a rollback that
/// never ran. See the `# Panics` section on [`rewind`](Emitter::rewind).
///
/// A table-keyed emitter — one that allocates per-`checkpoint` bookkeeping behind interior
/// mutability and reclaims it per-`release` — is **rejected as the inner at compile time**: the
/// sink re-spends its base reading across no-row **origin** rewinds, drops row readings above a
/// rewound target by value, and settles rows out of stack order under mixed raw use, all of which
/// presuppose readings — and equal-mark rows are interchangeable only while the reading is a
/// function of emission state. The requirement is therefore the [`ValueKeyedEmitter`] bound on
/// this type's [`Emitter`] impl and on `Sink::new`, not a convention; see that trait for
/// the exact promise and the roster of implementors. Such an emitter belongs at the input layer's
/// direct seam, where the settle discipline is 1:1 — the sink's own mark stack is exactly that
/// shape, and the input layer does release it. A sink is not a value-keyed emitter either, so a
/// sink cannot wrap a sink.
///
/// # Construction: minted by the parse, never beside it
///
/// A sink cannot be built by a caller. `Sink::new` is crate-private, and the public
/// doors are [`parse_lossless`](super::parse_lossless) and
/// [`parse_lossless_partial`](super::parse_lossless_partial), which take the source **once**
/// and hand that same argument to the sink they mint and to the input they drive.
///
/// That is what closes the *early* half of the wrong-source class. The late half was already
/// closed — materialization takes no source parameter, so it cannot be handed a different
/// buffer than construction named — but the early half, *that the buffer named at construction
/// is the one the parser reads*, was never a type-system fact: `'inp` proves both borrows
/// outlive the sink, not that they are the same bytes. Two independent names could differ;
/// one argument cannot differ from itself.
///
/// The drivers additionally pin `Ctx::Emitter` to `Sink` **by name**, so a forwarding wrapper
/// has no slot to occupy at the entry, and what they return is a [`Cst`](super::Cst) rather
/// than the sink — and a [`Cst`](super::Cst) is not an emitter, so the artefact cannot be
/// re-aimed at a second parse.
///
/// ## The runtime handshake, and what remains of it
///
/// The sink also overrides [`Emitter::bound_source`](crate::Emitter::bound_source) to report
/// its buffer's offset origin, and the point at which an emitter is attached to an input
/// compares that against the source the parse reads, **panicking** on a provable mismatch —
/// pinned by `sink_bound_to_a_foreign_source_is_refused`. With the drivers in place it is
/// redundant for a sink obtained the intended way, and no door hands one out any more:
/// `Sink::new` and `InputRef::emitter` are crate-private, `InputRef::emitter_ref` yields a
/// **shared** reference, which no emitter slot can take, and the public callback traits —
/// `Decision::decide`, the `peek_then` family, the token-level pratt folds — take an
/// [`EmitterView`](crate::EmitterView), which implements no emitter trait.
///
/// What keeps the check earning its place is the boundary the type system does not reach.
/// `EmitterView` is not an emitter *in this crate*; a downstream crate whose own lexer appears in
/// the parameter list may implement one for it under the orphan rules, and this handshake is what
/// stands on that route — for a wrapper that forwards `bound_source`. What such a wrapper cannot
/// do is **reach this sink's coverage machinery**, through either of its two doors: a token event
/// (only [`Emitter::commit_token`](crate::Emitter::commit_token) makes one, and it is not on the
/// view's surface — nor is `CstEmitter::cst_token`, deleted for being the second) or a
/// gap-licensing lexer-error span (only
/// [`Emitter::commit_lexer_error`](crate::Emitter::commit_lexer_error) makes one, and it is not on
/// the view's surface either). The view's own
/// [`emit_lexer_error`](crate::EmitterView::emit_lexer_error) forwards the *diagnostic* and
/// records no span, so a foreign parse driven through such a wrapper reports into this sink's log
/// and decides nothing about which of its bytes need a token.
///
/// **The refusal is conservative, and the residue is real.** It fires only where
/// [`Source::REFERENT_IS_BYTES`](crate::Source::REFERENT_IS_BYTES) says an unequal reference
/// *proves* an unequal source — the `?Sized` backings (`str`, `[u8]`, `BStr`), which every
/// lexer in this crate declares. For an **owned-handle** backing (`bytes::Bytes`, `HipStr`,
/// the `smol_bytes` family) the reference addresses a variable rather than the bytes, so two
/// clones of one buffer are two addresses and an inequality proves nothing; refusing them
/// would turn a correct program into a panic. Those backings keep the door open, unchanged
/// from before, and a third party closes it for their own by overriding one `const`. And a
/// wrapper that simply declines to forward `bound_source` reports `None`, which the seam reads
/// as "binds no source" — so the check sees nothing at all.
///
/// `Sink::new` takes the source buffer the parse runs over, the wrapped emitter, and
/// the dialect's [`CstProfile`] — the token mapper (`fn(&L::Token) -> u16` into the dialect's
/// unified kind space — no kind bound leaks into core), the [`KindValidator`] every recorded
/// kind is checked against, the `error_kind` used to wrap recovery holes, and the `gap_kind`
/// used to tile uncovered source bytes at materialization (what makes `tree.text() == source`
/// structural for every input, lexer errors included). Construction is **compile-time
/// restricted to trivia-surfacing lexers** ([`Lexer::SURFACES_TRIVIA`]): a syntactic lexer
/// that skips trivia cannot take the lossless door, because a skipped-whitespace gap is
/// indistinguishable from a dropped committed token.
///
/// ## Why the kind validator is data, not a type parameter
///
/// The validator rides in the profile as a plain predicate instead of being hung on the
/// dialect's brand or on `rowan::Language`, and that is forced rather than chosen:
/// [`Dialect::Lang`](crate::Dialect::Lang) is a **grammar brand** (`type Lang: ?Sized`, a
/// marker that keeps two grammars' vocabularies apart), while a kind authority would have to
/// be a `rowan::Language`; and `rowan`'s `kind_from_raw` has no fallible form, so even a
/// `Lang: rowan::Language` bound could not yield a *typed* rejection — only a panic at query
/// time. See [`KindValidator`] for the full statement.
///
/// [`KindValidator`]: super::KindValidator
pub struct Sink<'inp, L, E>
where
  L: Lexer<'inp>,
{
  /// The wrapped emitter every diagnostic forwards to.
  inner: E,
  /// E1 — the event buffer: the second emission log (ground truth).
  events: Vec<Event<L::Span>>,
  /// Whether an `Event::Demote` was ever appended to [`Sink::events`] — the guard that keeps
  /// materialization's canonicalization pass off a parse that never took a failing bracket
  /// exit, which is every parse of a predictive grammar.
  ///
  /// **Latching, and deliberately outside the rewind timeline**, in the [`Sink::degraded`]
  /// class rather than the memo class — and the direction of the imprecision is what makes
  /// that safe. Nothing ever writes `false` after construction, so the hint can only ever be
  /// *over*-set: a rewind that truncates the last surviving `Demote` leaves it standing, the
  /// pass then runs, finds nothing and returns. The opposite error — a clear over a surviving
  /// `Demote`, which would let an abandoned node materialize — is unrepresentable rather than
  /// merely unlikely. A `debug_assert!` at materialization holds the direction.
  ///
  /// It is a hint about the buffer and not a fact about a prefix, which is why it is not the
  /// #98 memo class: no reader derives anything from it, and the one reader that consults it
  /// gets the identical answer with the hint forced to `true`.
  demotes: bool,
  /// E2 — the undo journal for the `forward_parent` acceleration writes.
  journal: Vec<JournalEntry>,
  /// E4 — the mark stack: one row per live checkpoint capture, holding the frozen depth and
  /// the inner emitter's own checkpoint reading (the inner's rewind target).
  ///
  /// Plain storage, and that is load-bearing rather than incidental: this was a `RefCell`
  /// solely because [`Emitter::checkpoint`] used to take `&self`, and a cell reachable from a
  /// shared reference is a mutation door on every `&Sink` — which is what
  /// [`InputRef::emitter_ref`](crate::InputRef::emitter_ref) hands a parser. The receiver
  /// carries the capability now (al8n/tokora#257), so no `&Sink` can push a row, and the sink
  /// holds no cell for one to push through.
  rows: Vec<MarkRow>,
  /// E5 — the live open-node depth of the current buffer: pushes minus pops over every event
  /// in [`Sink::events`], maintained rather than recounted.
  ///
  /// Signed, and deliberately so. The buffer is a raw surface, so a caller *can* drive the
  /// count below zero (a `cst_finish` with nothing open anywhere); the whole point of the
  /// value is to see that, and an unsigned cell would wrap instead of showing it. See the
  /// depth section in the module docs for the restore rule and for the recount this replaced.
  depth: i64,
  /// E6 — the index at which the buffer's **pure-[`Diag`](Event::Diag) tail** begins:
  /// every event in `[diag_tail, events.len())` is a `Diag`, and `events[diag_tail - 1]` (when
  /// the tail is not the whole buffer) is not.
  ///
  /// The **ceiling** of the hole wrap's backward scan, as [`Sink::floor_mark`] is one term of
  /// its floor — and the bound that turns al8n/tokora#305's `Θ(H²)` release-profile rescan into
  /// `O(1)` per empty hole. `Diag` slots are transparent to that scan by design, so the tail
  /// contributes only loop iterations; entering at this index skips exactly those and nothing
  /// else, which is why the repair narrows no wrap and changes no tree.
  ///
  /// Maintained rather than recounted, in [`Sink::depth`]'s class and under the identical
  /// restore rule: the ONE append door writes it, the hole wrap's splice charges its own half,
  /// and a truncating rewind copies it back off the target row's frozen fact (or `0` at the
  /// origin). See the `Diag`-tail section in the module docs.
  diag_tail: u64,
  /// E7 — the open-node chain: one [`OpenNode`] per depth-increasing event still in
  /// [`Sink::events`], appended in event order.
  ///
  /// **Debug builds only.** Its one reader is
  /// [`validate_start_mark`](Self::validate_start_mark)'s `debug_assertions` wall, whose release
  /// backstop is a typed refusal at materialization through both finish doors, so a release
  /// build must pay neither the memory nor the append-path branch.
  ///
  /// Same two verbs as the log — append at a `+1` event, suffix-truncate at a rewind — which is
  /// why it needs no undo journal of its own. See [`OpenNode`] for why a close does not reclaim
  /// its slot.
  ///
  /// Growth is the log's: one entry per depth-increasing event that is still in `events`, and
  /// an entry dies with the event it names, so `opens.len() ≤ events.len()` holds at every
  /// moment. There is no second growth site and no charging ceiling of its own to state.
  #[cfg(debug_assertions)]
  opens: Vec<OpenNode>,
  /// E8 — the innermost open node's slot in [`Sink::opens`], or `None` when nothing is open.
  ///
  /// The chain's head, and [`Sink::depth`]'s structural companion: `depth` counts the open
  /// nodes, this one names them. Maintained by the same ONE append door (push at a `+1`, follow
  /// `parent` at a `-1`, and a `-1` with nothing open is the no-op the raw surface's negative
  /// depth already admits), and **restored at a truncating rewind from the target row's frozen
  /// value** — E5's rule, third time.
  ///
  /// What it buys is al8n/tokora#306: the demote wall used to recount `events[index + 1..]` to
  /// the end of the buffer with no early exit on the success path, `Θ(d × len)` visits for `d`
  /// nested failing brackets. The scan wanted a **running minimum** over that suffix, which the
  /// maintained `depth` cannot supply — a scalar total is blind to a dip that recovers — so the
  /// answer is this chain. See the open-chain section in the module docs for the equivalence.
  ///
  /// The query is a *search* of the chain from this head and not a walk of it: `O(log depth)`,
  /// over the skew-binary skip links each [`OpenNode`] carries. An `O(depth)` walk was the
  /// first repair and was not enough — the raw event surface has no depth cap, and the same
  /// `Θ(n²)` reappears there. That argument is in the module docs beside this one.
  #[cfg(debug_assertions)]
  open_top: Option<u32>,
  /// The newest *released* row's mark: the youngest committed positional fact the sink holds,
  /// and the floor of the hole wrap's backward scan.
  ///
  /// A `u64` and not a whole [`MarkRow`], because the depth half stopped having a reader when
  /// [`Sink::depth`] became live: the row's frozen depth is now consulted only where it is
  /// spent, at a rewind that lands on it. A floor that carried a `depth` nobody read would be
  /// state to keep exact for no one.
  floor_mark: u64,
  /// E3 — the monotone era source and truncation witness backing mark validation.
  ledger: TruncationLedger,
  /// The inner emitter's **construction-time** reading — the no-row **origin** rewind's exact
  /// inner target (an empty event log provably pairs with the construction reading; a mid-log
  /// no-row mark has no exact reading and is refused by the preflight instead of guessed at).
  /// Primed at the first inner-advancing touch (a forwarded diagnostic or a settled token; the
  /// rewind fallback reads it the same way), which provably equals the reading at
  /// `Sink::new`: the sink
  /// exposes no `&mut` path to the inner, so the inner cannot advance before the sink's own
  /// first advancing call — and every advancing surface primes this field before forwarding.
  /// (The capture is lazy only to keep the constructor free of emitter bounds: `Emitter` is
  /// `Lang`-parameterized and the built-in emitters implement it for exactly one `Lang`.)
  base_inner: Option<u64>,
  /// The latching poison: set by the one [`rewind`](Emitter::rewind) that detects an unpaired
  /// settle while it is forbidden to report one (a panic already unwinding), where it degrades
  /// to a total no-op instead. Materialization refuses through **both** doors afterwards
  /// ([`FinishError::UnpairedSettle`]), because the tree that door would return is the tree of a
  /// rollback that never happened.
  ///
  /// First-wins and never cleared: the first refusal is the cause, and no later rewind can undo
  /// it. It is therefore **not** part of the rewind timeline — restoring it at a rewind would
  /// let a subsequent rollback launder the refusal away.
  degraded: Option<DegradedRewind>,
  /// The buffer this parse runs over, bound at construction: materialization slices every
  /// token's text out of it, so the sink and the tree can never disagree about which source
  /// the spans belong to.
  source: &'inp L::Source,
  /// The dialect's kind space: token mapper, kind validator, and the two kinds the sink
  /// synthesizes on its own behalf (recovery-hole wrap, gap tile).
  profile: CstProfile<L::Token>,
  /// The materialization-time trivia placement policy.
  trivia: TriviaPolicy,
  /// The sink's identity, stamped into every mark it mints and validated at every spend —
  /// in **every** build (see `next_sink_witness`).
  witness: usize,
  _lexer: PhantomData<&'inp L>,
}

impl<'inp, L, E> core::fmt::Debug for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: core::fmt::Debug,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("Sink")
      .field("inner", &self.inner)
      .field("events", &self.events.len())
      .field("live_marks", &self.rows.len())
      .field("degraded", &self.degraded)
      .field("error_kind", &self.profile.error_kind())
      .field("gap_kind", &self.profile.gap_kind())
      .field("trivia", &self.trivia)
      .finish_non_exhaustive()
  }
}

/// CELL_CENSUS — the structural tripwire for the sink's cells, in the exact shape of the
/// input layer's guardian (`input::lineage::census`): it destructures [`Sink`]
/// exhaustively — no `..` — so adding a field fails to compile *here*, at the table that
/// asks which class the new cell is in and what a rewind must do to it. Generic and never
/// instantiated: type-checked in every build, monomorphized in none.
#[allow(dead_code)]
pub(crate) fn census<'inp, L, E>(sink: &Sink<'inp, L, E>)
where
  L: Lexer<'inp>,
{
  let Sink {
    // — the wrapped emitter: rewinds through its own contract, driven only by this sink.
    inner: _,
    // — E1, ground truth: append + suffix-truncate (+ the censused hole-wrap splice).
    events: _,
    // — latching hint: an `Event::Demote` was appended. Set once, NEVER cleared and never
    // rewound. Over-setting costs one no-op canonicalization pass; under-setting is
    // unrepresentable, which is the only direction that could matter.
    demotes: _,
    // — E2, undo journal: rewind reverse-replays and truncates by `at_len`.
    journal: _,
    // — E4, release stack + depth ledger: push at checkpoint, pop at release/rewind.
    rows: _,
    // — E5, the live open-node depth: charged by the ONE push helper on every append,
    // restored at a truncating rewind from the target row's frozen depth (or 0 at the
    // origin). A non-truncating rewind leaves it alone. DEPTH_CENSUS locks the arity.
    depth: _,
    // — E6, the live start of the pure-Diag tail: the hole wrap's scan ceiling. Written by the
    // SAME one push helper (to the new length for a non-Diag, left alone for a Diag) and by the
    // splice's own half, and restored at a truncating rewind from the target row's frozen value
    // (or 0 at the origin) — E5's rule verbatim, on the arity DEPTH_CENSUS already locks.
    diag_tail: _,
    // — E7, the debug-only open-node chain: append + suffix-truncate, the log's own two verbs.
    // A rewind drops the entries whose start reached the truncated region; a close reclaims
    // NOTHING, because a reused slot would alias a frozen row's open_top.
    #[cfg(debug_assertions)]
      opens: _,
    // — E8, the debug-only live chain head: pushed/followed by the SAME one push helper, and
    // restored at a truncating rewind from the target row's frozen value (or None at the
    // origin) — E5's rule again.
    #[cfg(debug_assertions)]
      open_top: _,
    // — derived memo: the newest released row's MARK; reset when a rewind drops below it.
    floor_mark: _,
    // — E3, monotone era source + truncation witness: NEVER rewound.
    ledger: _,
    // — derived memo: the inner's construction-time reading, primed at first advancing touch,
    // never restored.
    base_inner: _,
    // — latching poison: a rewind the sink refused to perform (an unpaired settle detected
    // mid-unwind). Set once, NEVER cleared and never rewound — a later rollback must not be
    // able to launder an earlier refusal away. Read only at materialization, which refuses.
    degraded: _,
    // — configuration: the construction-bound source buffer, fixed for the sink's life.
    source: _,
    // — configuration: the dialect's kind space, fixed for the sink's life.
    profile: _,
    // — configuration: fixed for the sink's life.
    trivia: _,
    // — witness: sink identity (every build), never restored.
    witness: _,
    _lexer: _,
  } = sink;
}

impl<'inp, L, E> Sink<'inp, L, E>
where
  L: Lexer<'inp>,
{
  /// Creates a recording sink over `source`, around `inner`, configured by `profile`.
  ///
  /// **Crate-private: the drivers are the only minters.** A sink names the buffer its tree's
  /// text is sliced out of, and an input names the buffer the parse reads; while a caller could
  /// state those independently, one program could state them differently and obtain a tree whose
  /// structure came from one source and whose text came from another. Both names now come from
  /// the single `src` argument of [`parse_lossless`](super::parse_lossless) /
  /// [`parse_lossless_partial`](super::parse_lossless_partial), which is where the public door
  /// is and where every doctest for the two compile-time walls below now lives.
  ///
  /// - `source` is the buffer the parse runs over; every token's text is sliced out of it at
  ///   materialization, so [`Cst::finish`](super::Cst::finish) takes no source argument of its own;
  /// - `profile` is the dialect's kind space
  ///   ([`CstProfile`](super::CstProfile)): the token mapper into the unified u16 kind space,
  ///   the [`KindValidator`](super::KindValidator) every recorded kind is checked against, the
  ///   `error_kind` wrapped around a recovery hole's skipped tokens, and the `gap_kind` tiled
  ///   over source bytes no committed token covers (what makes `tree.text() == source`
  ///   structural for every input). The [`TOMBSTONE`] value is reserved throughout: no
  ///   validator admits it, emission asserts against it unconditionally in every build, and
  ///   materialization rejects it.
  ///
  /// Construction is restricted at **compile time** twice over, and both walls reach the public
  /// door through this one call:
  ///
  /// - to **trivia-surfacing lexers** ([`Lexer::SURFACES_TRIVIA`] `== true`): a syntactic lexer
  ///   that skips trivia cannot take this lossless door, because a skipped-whitespace gap is
  ///   indistinguishable from a dropped committed token. The wall is the inline-`const`
  ///   assertion in the body, so it fires at build/test/doc time (a post-monomorphization
  ///   `error[E0080]` reported at the offending *driver* call site, with a note naming the
  ///   instantiation) — **not** under `cargo check`, which never monomorphizes the call;
  /// - to **value-keyed inner emitters** ([`ValueKeyedEmitter`]) — see the *Inner-emitter
  ///   contract* on [`Sink`]. That one is an ordinary bound, restated on the drivers, so it
  ///   fires at type-check time.
  #[inline]
  pub(crate) fn new(source: &'inp L::Source, inner: E, profile: CstProfile<L::Token>) -> Self
  where
    E: ValueKeyedEmitter,
  {
    const {
      assert!(
        L::SURFACES_TRIVIA,
        "a lossless (gap_kind) Sink requires a trivia-surfacing lexer: every source \
         byte must reach the sink as a token or a reported lexer error, and a skipped \
         whitespace gap is indistinguishable from a dropped committed token. Declare \
         `const SURFACES_TRIVIA: bool = true` on the lexer's Token impl (or override it \
         on the Lexer impl) ONLY if the lexer really surfaces trivia as tokens."
      )
    };
    // The event log's one cheap predictor is the source it is about to describe: a
    // trivia-surfacing lexer covers every byte with a token, so the log grows with the byte
    // count. `event_capacity_for` turns that into the capacity the `Vec`'s own doubling would
    // have reached, so the same block is bought once instead of copied into six times.
    let capacity = {
      use crate::{slice::Slice as _, source::Source as _};
      event_capacity_for(source.as_slice().len())
    };
    Self {
      inner,
      events: Vec::with_capacity(capacity),
      demotes: false,
      journal: Vec::new(),
      rows: Vec::new(),
      depth: 0,
      diag_tail: 0,
      #[cfg(debug_assertions)]
      opens: Vec::new(),
      #[cfg(debug_assertions)]
      open_top: None,
      floor_mark: 0,
      ledger: TruncationLedger::new(),
      base_inner: None,
      degraded: None,
      source,
      profile,
      trivia: TriviaPolicy::AsEmitted,
      witness: next_sink_witness(),
      _lexer: PhantomData,
    }
  }

  /// Sets the materialization-time trivia policy (builder form).
  ///
  /// Crate-private with the constructor: the public builder is
  /// [`Cst::with_trivia_policy`](super::Cst::with_trivia_policy), at the door that reads the
  /// policy.
  #[inline(always)]
  #[must_use]
  pub(crate) fn with_trivia_policy(mut self, policy: TriviaPolicy) -> Self {
    self.trivia = policy;
    self
  }

  /// The wrapped emitter, by shared reference.
  ///
  /// Deliberately no `&mut` counterpart: a caller who could drive the inner emitter's
  /// `rewind` directly would shear the event log from the diagnostic log with no witness.
  /// The mutable path to the inner emitter is the sink's own trait surface; ownership
  /// comes back from [`Cst::finish`](super::Cst::finish) /
  /// [`Cst::finish_partial`](super::Cst::finish_partial).
  #[inline(always)]
  pub const fn inner_ref(&self) -> &E {
    &self.inner
  }

  /// The configured recovery-hole node kind.
  #[inline(always)]
  pub const fn error_kind(&self) -> u16 {
    self.profile.error_kind()
  }

  /// The configured gap-tile token kind.
  #[inline(always)]
  pub const fn gap_kind(&self) -> u16 {
    self.profile.gap_kind()
  }

  /// The configured trivia policy.
  #[inline(always)]
  pub const fn trivia_policy(&self) -> TriviaPolicy {
    self.trivia
  }

  /// DEPTH_CENSUS — the **one** append door of the event log, and the only place
  /// [`Sink::depth`] and [`Sink::diag_tail`] are charged for an event.
  ///
  /// Every `Event` the sink appends goes through here, so neither scalar can drift by
  /// omission: a new emission surface that forgets to maintain them is not a surface that
  /// forgets — it is a surface that does not compile against `events`, because the field is
  /// private and the source lock in this module's tests pins `self.events.push(` at exactly
  /// one occurrence. The one mutation that is *not* an append — the hole wrap's splice —
  /// charges its halves at its own site, and is pinned there by the same lock.
  ///
  /// The two charges are read off the event in the same breath, and both are total: `depth`
  /// takes the event's own `depth_delta`, and `diag_tail` moves to the new length for every
  /// event that is not a [`Diag`](Event::Diag) — the tail an appended `Diag` extends is the
  /// tail a non-`Diag` ends.
  #[inline]
  fn push_event(&mut self, event: Event<L::Span>) {
    let delta = event.depth_delta();
    self.depth += delta;
    let ends_the_diag_tail = !matches!(event, Event::Diag { .. });
    #[cfg(debug_assertions)]
    self.charge_open_chain(delta, self.events.len() as u64);
    self.events.push(event);
    if ends_the_diag_tail {
      self.diag_tail = self.events.len() as u64;
    }
  }

  /// E7/E8's half of a single event's charge: the open-node chain, moved by the event's own
  /// `depth_delta` and by nothing else.
  ///
  /// Called from the ONE append door and from the hole wrap's splice — the same two sites, and
  /// the same arity lock, that keep [`Sink::depth`] exact. `index` is the position the event is
  /// about to occupy.
  ///
  /// A `-1` with an empty chain is a deliberate no-op rather than an underflow: the buffer is a
  /// raw surface, `depth` is signed precisely so that a close with nothing open is *visible*
  /// rather than wrapped, and the chain says the same thing by having no entry to drop. The
  /// equivalence in the module docs holds through that case.
  ///
  /// The push writes all four fields of the new [`OpenNode`] and never revisits an older one, so
  /// every entry is a frozen fact about the prefix that produced it — the property the rewind
  /// restore leans on.
  #[cfg(debug_assertions)]
  #[inline]
  fn charge_open_chain(&mut self, delta: i64, index: u64) {
    match delta {
      1 => {
        let parent = self.open_top;
        let depth = parent.map_or(0, |slot| self.opens[slot as usize].depth + 1);
        let jump = self.skip_link_for(parent);
        self.opens.push(OpenNode {
          start: index,
          parent,
          depth,
          jump,
        });
        self.open_top = u32::try_from(self.opens.len() - 1).ok();
        debug_assert!(
          self.open_top.is_some(),
          "the open-node chain exceeded u32::MAX slots: one entry per depth-increasing event \
           in a log that is itself indexed by u64, so this is a 4-billion-node debug parse"
        );
      }
      -1 => {
        self.open_top = self
          .open_top
          .and_then(|slot| self.opens.get(slot as usize))
          .and_then(|node| node.parent);
      }
      // `depth_delta` is `{-1, 0, +1}`; a tombstone start, a token and a diag slot are all
      // neutral and open nothing.
      _ => {}
    }
  }

  /// A chain slot's depth, with the **virtual entry below the outermost one** at `-1`.
  ///
  /// `None` is a real position in this arithmetic, not an absence: it is where a skip link that
  /// runs off the bottom of the chain lands, and the skew-binary rule below has to be able to
  /// measure a hop that ends there.
  #[cfg(debug_assertions)]
  #[inline]
  fn chain_depth(&self, slot: Option<u32>) -> i64 {
    slot.map_or(-1, |s| i64::from(self.opens[s as usize].depth))
  }

  /// The skip link for a new entry whose parent is `parent` — **Myers' skew-binary rule**, in
  /// `O(1)` and reading nothing it will not also freeze.
  ///
  /// Take `p = parent`, `j = jump(p)` and `jj = jump(j)`. When the two hops `p → j` and
  /// `j → jj` are the *same length*, the new entry's skip composes them into one hop of roughly
  /// twice the length (`jj`); otherwise it starts a fresh unit hop (`p`). Applied at every push
  /// this lays down hops of length 1, 1, 3, 1, 1, 3, 7, … — the skew-binary ladder whose
  /// defining property is that following `jump` from any entry reaches the outermost one in
  /// `O(log depth)` steps, and that the greedy "skip when it does not overshoot, else step to
  /// the parent" search in [`probe_open_chain`](Self::probe_open_chain) is `O(log depth)` too.
  ///
  /// The rule reads only `parent` and its two skip targets, all of which are already frozen, so
  /// the link is decided once and — like every other field of an [`OpenNode`] — never rewritten.
  #[cfg(debug_assertions)]
  #[inline]
  fn skip_link_for(&self, parent: Option<u32>) -> Option<u32> {
    let p = parent?;
    let hop = self.opens[p as usize].jump;
    let double = hop.and_then(|j| self.opens[j as usize].jump);
    if self.chain_depth(Some(p)) - self.chain_depth(hop)
      == self.chain_depth(hop) - self.chain_depth(double)
    {
      double
    } else {
      Some(p)
    }
  }

  /// Whether the node opened by the event at `index` is **still open** — the `O(log depth)`
  /// answer that replaced [`validate_start_mark`](Self::validate_start_mark)'s `O(suffix)`
  /// recount (al8n/tokora#306).
  #[cfg(debug_assertions)]
  #[inline]
  fn node_is_open(&self, index: u64) -> bool {
    self.probe_open_chain(index).0
  }

  /// [`node_is_open`](Self::node_is_open)'s body, with the **work it did** returned beside the
  /// answer: the number of [`opens`](Self::opens) entries the search read.
  ///
  /// The search descends the chain from [`open_top`](Self::open_top), where `start` strictly
  /// decreases, taking the entry's skip link whenever that link lands at or above `index` and
  /// its `parent` otherwise. Neither move can step past the target — a skip is taken only when
  /// it does not, and a parent step moves exactly one link — so the walk meets `index` if the
  /// chain holds it, and otherwise lands on the first entry below it and says no. The
  /// skew-binary ladder [`skip_link_for`](Self::skip_link_for) builds bounds that at
  /// `O(log depth)` steps.
  ///
  /// The second half of the pair exists so a test can *measure* that bound rather than assert
  /// it in prose: the counter is the query's own, not a re-derivation of it, because the demote
  /// wall's query is literally this function with the count dropped.
  #[cfg(debug_assertions)]
  fn probe_open_chain(&self, index: u64) -> (bool, u64) {
    let mut reads = 0u64;
    let mut slot = self.open_top;
    while let Some(node) = slot.and_then(|s| self.opens.get(s as usize)) {
      reads += 1;
      if node.start == index {
        return (true, reads);
      }
      if node.start < index {
        // The chain descends past the queried index without meeting it.
        return (false, reads);
      }
      // `node.start > index`: take the longest hop that does not overshoot the target.
      slot = match node.jump {
        Some(j) => {
          reads += 1;
          if self.opens[j as usize].start >= index {
            Some(j)
          } else {
            node.parent
          }
        }
        None => node.parent,
      };
    }
    (false, reads)
  }

  /// The open-node depth of the current buffer, in O(1).
  ///
  /// A full recount over the events above the nearest frozen `(mark, depth)` fact until
  /// al8n/tokora#253: the CST channel mints no frozen facts of its own, so a grammar of flat
  /// sibling [`node`](crate::parser::node)s recounted the whole prefix on every close. See the
  /// depth section in the module docs.
  #[inline(always)]
  const fn depth(&self) -> i64 {
    self.depth
  }

  /// The full-recount oracle the maintained [`depth`](Self::depth) is checked against: the
  /// summed `depth_delta` of every event in the buffer, from zero.
  ///
  /// Test-only on purpose. Running it under `debug_assertions` would put the recount back in
  /// exactly the profile #253 is about — the equivalence is pinned by tests that walk every
  /// path that writes the scalar (checkpoint, release, the four rewind arms, retro-wraps,
  /// demotes, hole wraps, regrowth) instead.
  #[cfg(test)]
  pub(crate) fn recount_depth(&self) -> i64 {
    self.events.iter().map(Event::depth_delta).sum::<i64>()
  }

  /// The full-recount oracle for [`opens`](Self::opens)/[`open_top`](Self::open_top): the
  /// open-node stack replayed from an empty one over the whole buffer, outermost first.
  ///
  /// Test-only for the same reason as its two siblings, and it is what pins the equivalence the
  /// demote wall now rests on rather than restating it in prose.
  #[cfg(all(test, debug_assertions))]
  pub(crate) fn recount_open_chain(&self) -> std::vec::Vec<u64> {
    let mut stack = std::vec::Vec::new();
    for (index, ev) in self.events.iter().enumerate() {
      match ev.depth_delta() {
        1 => stack.push(index as u64),
        -1 => {
          stack.pop();
        }
        _ => {}
      }
    }
    stack
  }

  /// The maintained chain read out as start indices, outermost first — the shape
  /// [`recount_open_chain`](Self::recount_open_chain) produces.
  #[cfg(all(test, debug_assertions))]
  pub(crate) fn open_chain(&self) -> std::vec::Vec<u64> {
    let mut out = std::vec::Vec::new();
    let mut slot = self.open_top;
    while let Some(node) = slot.and_then(|s| self.opens.get(s as usize)) {
      out.push(node.start);
      slot = node.parent;
    }
    out.reverse();
    out
  }

  /// The **retired `O(depth)` walk**, kept as [`probe_open_chain`](Self::probe_open_chain)'s
  /// oracle: the same question answered by following `parent` one link at a time, with the
  /// entries it read counted the same way.
  ///
  /// Test-only for [`recount_depth`](Self::recount_depth)'s reason, and it earns its place
  /// twice. It is the *independent* answer the skew-binary search is checked against — the two
  /// share no code and only the slow one is obviously right — and it is the **before** side of
  /// the work counter, so the repair's numbers are measured against the walk they replaced
  /// rather than against a remembered figure from an uncommitted harness.
  #[cfg(all(test, debug_assertions))]
  pub(crate) fn probe_open_chain_via_parents(&self, index: u64) -> (bool, u64) {
    let mut reads = 0u64;
    let mut slot = self.open_top;
    while let Some(node) = slot.and_then(|s| self.opens.get(s as usize)) {
      reads += 1;
      if node.start == index {
        return (true, reads);
      }
      if node.start < index {
        return (false, reads);
      }
      slot = node.parent;
    }
    (false, reads)
  }

  /// The full-recount oracle for [`diag_tail`](Self::diag_tail): the length of the buffer with
  /// its trailing run of `Diag`s removed — which is exactly the index the cell claims.
  ///
  /// Test-only for [`recount_depth`](Self::recount_depth)'s reason: running it under
  /// `debug_assertions` would put back the backward walk over the `Diag` run that
  /// al8n/tokora#305 is about.
  #[cfg(test)]
  pub(crate) fn recount_diag_tail(&self) -> u64 {
    let kept = self
      .events
      .iter()
      .rev()
      .take_while(|ev| matches!(ev, Event::Diag { .. }))
      .count();
    (self.events.len() - kept) as u64
  }

  /// Validates a mark before a spend — the panic-in-every-build wall.
  ///
  /// A mark is live iff it was minted **by this sink**, its index is in bounds, the slot
  /// still holds a tombstone, and no truncation younger than the mark's era reached its
  /// index. Every check runs in every build — the identity check first, because the
  /// positional and era halves are only meaningful against the issuing sink's own history
  /// (two fresh sinks both mint `(index: 0, era: 0)`, so a foreign mark can look perfectly
  /// live). Anything else is a parser bug: the branch that conceived the wrap was rolled
  /// back (or belonged to another parse entirely), and silently wrapping whatever sits at
  /// that index is the wrong-tree class nothing downstream can detect.
  fn validate_mark(&self, mark: &EventMark) {
    assert!(
      mark.sink() == self.witness,
      "EventMark was minted by a different sink (or by a no-event emitter's defaulted \
       cst_mark): marks are only spendable on the sink that issued them"
    );
    let index = mark.index();
    let in_bounds = (index as usize) < self.events.len();
    let is_tombstone = in_bounds && self.events[index as usize].is_tombstone();
    let is_current = !self.ledger.is_stale(mark.era(), index);
    assert!(
      in_bounds && is_tombstone && is_current,
      "stale EventMark: this mark no longer names a live tombstone — {}. Spending it anyway \
       would wrap an unrelated region: the wrong-tree class nothing downstream can detect.",
      // Only evaluated on the failure path, so the extra discrimination costs the spend
      // nothing. The last arm is the wrong-provenance misuse, which is not staleness at all
      // and would otherwise be reported as a rewind that never happened.
      if !in_bounds {
        "a rewind truncated it away, so the wrap intent died with the branch that conceived it"
      } else if !is_current {
        "a rewind truncated it away and the buffer regrew over its index, so the wrap intent \
         died with the branch that conceived it"
      } else if matches!(self.events[index as usize], Event::StartNode { .. }) {
        "the slot holds a real-kind StartNode, so the mark came from `cst_start` and not from \
         `cst_mark`. An up-front start is closed by `cst_finish` or reverted by `cst_demote`; \
         it is never a retro-wrap target, because wrapping an already-open node would nest it \
         inside a wrap of itself"
      } else {
        "the slot holds no node start at all"
      },
    );
  }

  /// Validates a **start** mark before a demote — the panic-in-every-build wall for the
  /// up-front bracket's failing exit, and the mirror of [`validate_mark`](Self::validate_mark).
  ///
  /// The two walls take the same three positional checks (issued by this sink, index in
  /// bounds, no truncation younger than the mark's era reached the index) and then diverge on
  /// the **slot content**, which is what keeps the two mark provenances from crossing: a
  /// retro-wrap spend demands a live `TOMBSTONE`, a demote demands a live `StartNode` of
  /// exactly the kind its `cst_start` was given. So a `cst_mark` tombstone cannot be demoted
  /// (its kind is `TOMBSTONE`, which no `cst_start` is allowed to name) and a `cst_start` mark
  /// cannot be retro-wrapped (the tombstone wall refuses it).
  ///
  /// The provenance separation is only airtight because the **reserved kind is refused before
  /// the slot is read at all**. A content-only comparison would let `cst_demote(mark,
  /// TOMBSTONE)` satisfy itself against any live tombstone — a `cst_mark` slot, or one already
  /// carrying a live `forward_parent` from a retro wrap — and append a `Demote` naming a slot
  /// the caller never opened. One compare against an immediate closes it in every build,
  /// because no `cst_start` can ever have opened a `TOMBSTONE`-kind node.
  ///
  /// # What this wall cannot see, and what refuses it instead
  ///
  /// The demotion is an **appended event**, not a rewrite of the slot, so the slot is unchanged
  /// by it: neither a finish nor a prior demote leaves a trace the slot itself can witness.
  /// Two misuses therefore pass this wall in a release build, and no check that reads the slot
  /// alone can do better:
  ///
  /// - **demoting a start whose node was already finished** — the finish is appended *above*
  ///   the slot;
  /// - **demoting the same start twice** — the first `Demote` is appended above the slot too.
  ///
  /// Both are bounded and typed rather than silent, and both are caught at the two tiers this
  /// crate calibrates such checks to. A **debug** build catches each at the misuse site, on the
  /// exact suffix recount below: a surviving finish that closed the marked node dips the
  /// running sum, and so does a prior `Demote` naming it (both are −1 above the slot). A
  /// **release** build refuses at materialization, typed and through both doors — a
  /// finished-then-demoted start leaves pops exceeding pushes, so the replay walk underflows at
  /// the surplus finish ([`FinishError::OrphanFinish`](crate::cst::FinishError::OrphanFinish),
  /// or `MismatchedFinish` sooner when the kinds misalign first), and a double demote is
  /// refused by canonicalization
  /// ([`FinishError::StaleDemote`](crate::cst::FinishError::StaleDemote)), whose second pass
  /// over the same target finds a slot the first already tombstoned. Both public doors are the
  /// *same* walk over the *same* canonicalized buffer, and `close_open_nodes` relaxes only
  /// end-of-stream opens and gap tiling — never a balance underflow and never corruption — so
  /// [`finish`](Self::finish) and [`finish_partial`](Self::finish_partial) refuse alike.
  ///
  /// # One shape the debug scan refuses that release accepts
  ///
  /// The recount is positional, so it also fires on a **third** dip route that is not a misuse
  /// at all: a `Demote` naming an **enclosing** start, emitted while this node was still open —
  /// two brackets whose exits interleave instead of nesting. Release admits that order and
  /// materialises exactly the tree the innermost-first order builds (canonicalization tombstones
  /// both slots either way), so the scan is a **strictness choice on the raw surface**, not
  /// early detection of a release defect: debug enforces innermost-first closings; release
  /// admits the out-of-order shape. The blessed `node()` bracket cannot produce it — it mints
  /// and spends its mark inside one call frame — so the stricter reading costs no legal code.
  ///
  /// Every check but that scan runs in every build, identity first, for the reason
  /// [`validate_mark`](Self::validate_mark) gives: the positional and era halves are only
  /// meaningful against the issuing sink's own history.
  ///
  /// # Who can reach the stale arm
  ///
  /// Raw callers, and only raw callers: staling a start mark requires a rewind below the
  /// bracket's own `cst_start`, and the safe combinator surface cannot express one mid-frame —
  /// a session settle needs a `SessionPointId` whose invariant `'closure` brand cannot enter a
  /// universally quantified parser frame, so `node()`'s failing exit always spends a live mark.
  /// That is a type-system guarantee with **no runtime backstop**: the settle scan is
  /// newest-first, so a point opened before the bracket would pass it. See *A rollback below the
  /// start cannot happen mid-frame* in the [`node` module docs](crate::parser::node) for the
  /// theorem and the compile-fail cases that pin it.
  fn validate_start_mark(&self, mark: &EventMark, kind: u16) {
    assert!(
      mark.sink() == self.witness,
      "EventMark was minted by a different sink (or by a no-event emitter's defaulted \
       cst_start): marks are only spendable on the sink that issued them"
    );
    // Before the slot is read: the reserved kind names no node a `cst_start` could have
    // opened, so no start mark can legally be demoted with it. See the doc comment for the
    // two shapes this refuses that a content-only check admits.
    assert!(
      kind != TOMBSTONE,
      "cst_demote with the reserved TOMBSTONE kind: no cst_start can open a TOMBSTONE-kind \
       node (no validator admits it), so no start mark can legally be demoted with it — a \
       cst_mark tombstone is spent by cst_start_at, never by cst_demote"
    );
    let index = mark.index();
    let slot = if self.ledger.is_stale(mark.era(), index) {
      None
    } else {
      self.events.get(index as usize)
    };
    let matches_kind = matches!(slot, Some(Event::StartNode { kind: k, .. }) if *k == kind);
    assert!(
      matches_kind,
      "cst_demote on a mark that does not name a live open node of kind {kind}: the slot the \
       mark named holds no such start any more. Either a rewind truncated it (the frame that \
       opened the node was rolled back, and the demote belongs to a branch that no longer \
       exists), or it came from cst_mark, whose tombstones are spent by cst_start_at and never \
       demoted. Naming whatever else sits at that index would be the wrong-tree class nothing \
       downstream can detect."
    );

    // Detect-at-cause, debug-only — the calibration this surface already takes: `cst_finish`'s
    // global-underflow check is a `debug_assert!` for the same reason, because the release
    // backstop here is a TYPED refusal through both finish doors (see the doc comment) and not
    // a wrong tree. Paying an O(suffix) recount in release would tax the failure path of every
    // backtracking grammar to make a loud detection merely earlier.
    //
    // The predicate is EXACT, not heuristic, and — the #98 lesson — it consults no frozen
    // baseline and no memo. It recounts the LIVE suffix strictly above the mark's own
    // already-validated slot, so every truncation has already removed whatever it removed.
    //
    // WHAT A DIP MEANS, positionally. Every event above the slot that pairs WITHIN the suffix
    // nets zero and never dips on the way: a completed child (`Start` +1 … `Finish` −1), a
    // nested bracket that took its own failing exit (`Start` +1 … its own `Demote` −1), an
    // unspent tombstone and a token (0 outright), a `StartAt` (which sits above the slot in
    // EMISSION order whatever it hoists to, its +1 before its −1). So a dip below zero means
    // the suffix holds a CLOSING event whose opening event is not in the suffix — and since the
    // marked node's own start is at the slot and every enclosing start is below it, that
    // closing event closes the marked node or an ancestor of it. There are exactly three
    // shapes, and they are NOT all misuse:
    //
    //   1. a surviving `FinishNode` — this node already took its success exit (or a finish
    //      meant for an ancestor, which stack discipline lands on this node anyway);
    //   2. a prior `Demote` naming THIS slot — a double demote;
    //   3. a `Demote` naming an ENCLOSING start — an ancestor bracket emitted its failing exit
    //      while this node was still open, i.e. closings out of innermost-first order.
    //
    // (1) and (2) are misuse: the bracket owes exactly one closing exit and took one already.
    // (3) IS NOT. Materialization canonicalizes both slots and builds exactly the tree the
    // innermost-first order builds, and the two orders' trees are pinned equal
    // (`interleaved_and_innermost_first_demotes_materialize_the_same_tree`). So this assert is
    // a STRICTNESS CHOICE on the raw surface, not early detection of a release defect:
    // **debug enforces innermost-first closings; release admits the out-of-order shape and
    // materialises the same tree.**
    //
    // Firing on (3) is not the #98 false-positive class, because the blessed brackets cannot
    // reach it: `node()` mints and spends its mark inside one call frame, so nested brackets
    // close last-in-first-out structurally. Only a hand-rolled raw sequence can interleave two
    // brackets' exits, and on that surface the stricter reading is the useful one.
    //
    // The sum is kind-blind, so the hoist-order inversion that forbids an emit-site kind check
    // in `cst_finish` cannot reach it. The suffix total is deliberately NOT required to be 0:
    // an open child above the mark is a different bug, already walled at materialization as
    // `UnclosedNodes`.
    // The dipping event IS the diagnosis, so the report names which of the three it is rather
    // than enumerating all three at the reader. The previous message asserted one shape ("the
    // node was already closed") for a condition with three causes, and misdiagnosed the two it
    // did not name — the defect class this repo keeps closing.
    //
    // THE VERDICT IS NO LONGER THE SCAN'S, and the diagnosis still is. The walk below used to
    // run on EVERY demote, success included, with no early exit — `Theta(d x len)` for `d`
    // nested failing brackets, measured at exactly `d(d - 1)` visits (al8n/tokora#306). It now
    // runs only when the open-node chain has already said the node is closed, i.e. only on the
    // path that is about to panic, where its cost is the panic's. The chain's answer is the
    // scan's answer on every input — a chain entry survives exactly while the suffix holds no
    // dip below its own start, proved in the open-chain section of the module docs — so this is
    // a change of WHEN the suffix is read, not of WHAT is refused. Cases (1), (2) and (3) all
    // still fire, the interleaved strictness choice included.
    //
    // The chain is SEARCHED in `O(log depth)` over its skip links, not walked in `O(depth)`.
    // The walk was the first repair and left #306's own shape behind: the raw surface this
    // method is reachable from has no depth cap, so a sequence that demotes near the BOTTOM of
    // a long chain put the same `Theta(n^2)` back on this line. See the module docs.
    #[cfg(debug_assertions)]
    if !self.node_is_open(index) {
      let mut depth: i64 = 0;
      for ev in &self.events[index as usize + 1..] {
        depth += ev.depth_delta();
        if depth < 0 {
          let shape = match ev {
            Event::FinishNode { .. } => {
              "a surviving cst_finish closed this node — the bracket already took its SUCCESS \
               exit, and it owes exactly one closing exit. Misuse: a release build refuses the \
               residue typed at materialization (OrphanFinish, or MismatchedFinish when the \
               kinds misalign first), through both finish doors"
            }
            Event::Demote { target } if *target == index => {
              "a prior cst_demote already named THIS slot — a DOUBLE demote, and the bracket \
               owes exactly one closing exit. Misuse: a release build refuses it typed at \
               materialization (StaleDemote), through both finish doors"
            }
            Event::Demote { .. } => {
              "a cst_demote of an ENCLOSING start was emitted while this node was still open — \
               two raw brackets whose closings INTERLEAVE instead of nesting. This one is NOT a \
               release defect: materialization canonicalizes both slots and builds exactly the \
               tree the innermost-first order builds, so release admits it and debug does not. \
               Emit this bracket's demote before its ancestor's. The blessed node() bracket \
               cannot produce the shape at all, because it mints and spends its mark inside one \
               call frame"
            }
            // `depth_delta` is negative for exactly those two variants, so nothing else can
            // carry the sum below zero.
            _ => unreachable!("only FinishNode and Demote carry a negative depth delta"),
          };
          panic!("cst_demote below a closing event this mark's node did not open: {shape}.");
        }
      }
      // The chain said closed and the suffix holds no dip: the two disagree, which is a defect
      // in E7/E8 and not in the caller. Loud rather than silent — a chain that drifts towards
      // "closed" would otherwise turn this wall into a random panic on correct code, and one
      // that drifts the other way would turn it off.
      unreachable!(
        "the open-node chain and the suffix recount disagree at index {index}: the chain says \
         the marked node is closed and the recount finds no closing event above it. E7/E8 have \
         drifted from the log (see the open-chain section in the sink's module docs)."
      );
    }
  }

  /// Records one committed token. The token channel has exactly **one** door — the
  /// auto-emission hook ([`Emitter::commit_token`], fed by the input layer's settle primitive)
  /// — and this is its body. There is no raw transport beside it: a caller-chosen span that no
  /// settle accounts for is the wrong-tree class nothing downstream can detect, so a grammar
  /// cannot open this door at all.
  fn record_token(&mut self, tok: &L::Token, span: &L::Span) {
    let kind = (self.profile.mapper())(tok);
    // Emission-time mapper validity (detect-at-cause): rowan would defer a bad kind to a
    // query-time panic arbitrarily far from the parse; materialization keeps the release
    // backstop. The predicate is the dialect's own — the reserved tombstone is the floor no
    // validator can lift, and a kind outside the dialect's space is just as wrong.
    assert!(
      self.profile.validator().admits(kind),
      "the dialect mapper produced a kind outside the dialect's own kind space for a \
       committed token (the reserved tombstone kind, u16::MAX, is never admitted)"
    );
    self.push_event(Event::Token {
      kind,
      span: span.clone(),
    });
  }

  /// The hole wrap: brackets the already-buffered token events of a recovery hole in a
  /// `Start(error_kind) … Finish` pair at the recovery site.
  ///
  /// The wrap is a **prefix-preserving** splice — one insert at the first wrapped token, one
  /// appended finish — and it stays prefix-preserving because the backward scan is *floored*
  /// at the youngest positional fact the sink holds, not because the caller's span is trusted
  /// to sit above it. See the floor note in the body for what the floor is and why it is a
  /// bound rather than an assertion. Interleaved `Diag` slots (lexer errors crossed while
  /// skipping) ride inside the wrap unchanged — they are invisible to materialization. If no
  /// buffered token event lies inside the hole span at or above the floor (no auto-emission
  /// configured, a direct call, or a caller span that reaches only below the floor), there is
  /// nothing to wrap and no node is made.
  ///
  /// The floor bounds the wrap's **structural** authority only. A caller-chosen span wider
  /// than the caller's own transaction still reaches the diagnostic channel verbatim and
  /// unchanged; only the `error_kind` node it induces is narrowed, to the tokens that settled
  /// within the transaction reporting the hole.
  fn wrap_hole(&mut self, span: &L::Span) {
    // ── THE SCAN FLOOR — the wrap start is BOUNDED, never merely asserted ──────────────
    //
    // The splice below is an `insert`, so every index at or above it shifts by one, and the
    // sink's own bookkeeping is **positional**: a checkpoint mark IS an event-buffer length,
    // the released-floor memo is a mark, and the undo journal names indices. A splice below
    // any of them renames it. All three are made unreachable here rather than repaired
    // afterwards — the marks by construction (this floor), the journal by the theorem stated
    // at the splice — and for the marks that is forced rather than a preference: the splice
    // would put the `Start` below a mark and its `Finish` above it, so EVERY truncation point
    // between them tears the pair. No row arithmetic repairs that — a `+1` fixup just makes
    // the rewind keep an orphan `Start`. The start has to be bounded before the fact.
    //
    // `rows` is sorted non-decreasing (rows are pushed at the then-current length and every
    // truncation pops the rows above it), so `last()` is the youngest LIVE capture.
    // `self.floor_mark` is a COMMITTED row's mark that `release` promoted, and it can sit
    // **above** `rows.last()` — a guard opened above an older live capture and then committed
    // leaves exactly that shape — so the floor is the max of the two, not the row stack alone.
    //
    // In-crate this costs nothing. `sync_balanced` is the only in-crate producer of an
    // `emit_skipped_region` call, and its span covers exactly the tokens its own scan just
    // settled — those postdate every capture, live or released — so `at >= floor` already
    // held for it and the floor never narrows a recovery this crate drives. The floor governs
    // the PUBLIC door (`InputRef`/`EmitterView`/`ParseState::emit_skipped_region`), whose span
    // is the caller's own and is under no such discipline: a recoverer that widens its
    // reported span backward over already-committed tokens gets the wrap narrowed to its own
    // transaction, and the report itself forwarded untouched.
    //
    // `EventMark`s need no term of their own here — but ONLY because a mark *materializes* an
    // event and this scan breaks on any structural event, so `at` is always above every mark's
    // slot and every `StartAt` target. That holds for **both** mark provenances and for the
    // whole life of a marked slot: `cst_mark` appends a tombstone `StartNode`, `cst_start`
    // appends a real-kind `StartNode`, and nothing during the parse rewrites either — the
    // failing exit appends an `Event::Demote` naming the slot instead, and the canonicalization
    // that acts on it runs at materialization, after this sink is consumed. The slot is a
    // `StartNode` from its append to the end of the parse, so it never stops breaking the scan
    // and its index never moves. A `Demote` event breaks the scan too (it is neither a `Token`
    // nor a `Diag`), so one can never sit inside a wrapped token run and be renamed by the
    // splice, and its `target` — a `StartNode` index — is below the run for the same reason.
    // The up-front bracket therefore needs no floor term of its own. **If a future design ever
    // de-materialises marks — hands out an index with no event standing behind it — that
    // immunity is gone and the floor must be extended to cover them.**
    let floor = self
      .rows
      .last()
      .map_or(0, |row| row.mark)
      .max(self.floor_mark) as usize;

    // ── THE SCAN CEILING — the pure-`Diag` tail is skipped, not re-decided ─────────────
    //
    // `Diag` is transparent to this scan by design (that is what lets a hole wrap tokens that
    // already carry diagnostics), so every event in `[diag_tail, len)` — all of them `Diag`s,
    // by E6's invariant — would take the `continue` arm and change nothing. Entering at
    // `diag_tail` drops exactly those iterations and no others, so the wrap this computes is
    // the wrap the full walk computed: the repair is a scan that stops re-deciding what it
    // already decided, NOT a new bound on the wrap's structural authority (that is the floor's
    // job, above).
    //
    // What it buys is the al8n/tokora#305 quadratic. A hole whose span matches no buffered
    // token appends no structural event, so before E6 the diagnostic run left behind — which
    // grows by at least one per call, since `emit_skipped_region` forwards its own report —
    // was rescanned in full by every later hole: `H(H - 1)/2` visits for `H` holes, in EVERY
    // profile, release included. With the ceiling the first index examined is `diag_tail - 1`,
    // which is a non-`Diag` by the invariant, so an empty wrap breaks on its first iteration.
    // Every scan's run is then disjoint from every other's — exactly the property the ordinary
    // case already had from the `StartNode`/`FinishNode` pair it appends.
    //
    // Both ends are CLAMPED rather than asserted. `diag_tail <= len` and `floor <= len` both
    // hold (the cell is only ever set to a length or to a row's frozen value, and a rewind
    // resets the floor memo it drops below), but the old walk was total over any pair of
    // values — it iterated the whole log and broke on `idx < floor` — and a bound that is only
    // in range because of an argument elsewhere in the file is the wrong thing to hand a
    // slice. `lo <= hi` by construction, so the range is empty exactly where the floored walk
    // stopped immediately.
    let hi = (self.diag_tail as usize).min(self.events.len());
    let lo = floor.min(hi);
    let mut wrap_start: Option<usize> = None;
    for (offset, ev) in self.events[lo..hi].iter().enumerate().rev() {
      match ev {
        Event::Diag { .. } => continue,
        Event::Token { span: s, .. }
          if s.start_ref() >= span.start_ref() && s.end_ref() <= span.end_ref() =>
        {
          wrap_start = Some(lo + offset);
        }
        _ => break,
      }
    }
    // `at >= floor` by construction — the loop never looks below it.
    let Some(at) = wrap_start else {
      return;
    };

    // ── THE JOURNAL NEEDS NO FIX-UP — a theorem about the scan, not a habit of the producers ──
    //
    // The splice renames every index at or above `at`, and journal entries are indices, so the
    // obvious reading is that they need bumping. They do not, and this method used to bump them
    // anyway: `Theta(J)` reads and branches per hole that provably wrote nothing, `Theta(J x H)`
    // over a parse (al8n/tokora#250, measured at exactly `J x H`). What follows is why the loop
    // is not merely unnecessary today but unreachable, because deleting a fix-up on a wrong
    // proof does not cost time — it silently renames journal indices.
    //
    // ENTRY EXACTNESS. `cst_start_at` is the ONLY producer (one `journal.push` in the file). It
    // pushes `at_len = new_index + 1`, where `new_index` is the index its `StartAt` was appended
    // at, and `index = target`, the tombstone `validate_mark` just proved in bounds — so
    // `index < events.len() == new_index == at_len - 1`, structurally, from the in-bounds check
    // rather than by assumption. Every live entry therefore has a `StartAt` standing at
    // `at_len - 1` and a `StartNode` standing at `index`, and both keep standing:
    //
    //   - appends never move an existing index;
    //   - the two interior writes touch a `StartNode`'s `forward_parent` only — no index, no
    //     variant;
    //   - `rewind` truncates to `mark` and pops exactly the entries with `at_len > mark` (the
    //     journal is strictly increasing in `at_len`, pinned by a debug assert at the push), so
    //     every survivor has `at_len - 1 < mark`: its `StartAt` is below the cut;
    //   - this splice is the last mutation, and it is the induction step below.
    //
    // THE SCAN'S OWN CONCLUSION. The backward loop above breaks on the first event that is
    // neither a `Diag` nor an in-span `Token`, and records `at` as the LOWEST in-span `Token` it
    // reached. So every index in `[at, events.len())` holds a `Token` or a `Diag` — that is what
    // the loop computed, not something asserted about it. `Event::StartAt` is neither.
    //
    // Hence `at_len - 1 < at` for every live entry, i.e. `at_len <= at`, and `index < at_len - 1
    // < at`: both branch conditions of the deleted loop (`at_len > at`, `index >= at`) were
    // false for every entry, in every reachable state — including recovery inside a Pratt fold
    // (whose `cst_mark` tombstone and per-fold `StartAt` are both scan-breakers), a partial
    // session (which changes no cell this argument reads), and a rewound checkpoint (the pop
    // rule above). The splice moves only indices `>= at`, none of which any entry names, so the
    // entries stay exact and ENTRY EXACTNESS holds again for the next hole.
    //
    // The assert costs O(1), not O(J): `at_len` is strictly increasing, so the newest entry is
    // the only one that can be the first to cross `at`. That equivalence is what the push-side
    // monotonicity assert underwrites — read them as one pin, in debug builds only.
    debug_assert!(
      self
        .journal
        .last()
        .is_none_or(|newest| newest.at_len <= at as u64),
      "hole wrap at {at} with a journal entry at or above it: the backward scan admits only \
       Token and Diag events, and every entry's StartAt is neither, so this is a scan that \
       stopped breaking on a structural event or an entry whose StartAt no longer stands \
       where at_len names. Either way the splice is about to rename an index the journal \
       still points at."
    );

    let error_kind = self.profile.error_kind();
    // The wrap is depth-neutral, and its two halves are charged separately rather than netted
    // out: the `+1` here and the `-1` in the `push_event` below come from the events' own
    // `depth_delta`, so a change to either delta cannot silently unbalance E5. This `insert` is
    // the one log mutation that is not an append, and DEPTH_CENSUS pins it at exactly one site.
    let start = Event::StartNode {
      kind: error_kind,
      forward_parent: None,
    };
    self.depth += start.depth_delta();
    // E7/E8's half, from the same delta: the spliced start opens a node that the `push_event`
    // below immediately closes, so the chain ends where it began — but the two halves are
    // charged, not netted, so a change to either delta cannot silently unbalance them either.
    // The slot this appends is dead the moment the finish follows it, and stays (see
    // `OpenNode`). `at` is above every existing entry's `start`: the scan proved every index in
    // `[at, len)` holds a `Token` or a `Diag`, so no chain entry names one, which is also what
    // keeps the splice's index shift from renaming any of them.
    #[cfg(debug_assertions)]
    self.charge_open_chain(start.depth_delta(), at as u64);
    // E6's half of the same splice, charged here rather than netted out for the same reason.
    // The scan proved `events[at]` is a `Token`, so `diag_tail > at` held before the insert and
    // every event of the tail just moved up one slot. (The `push_event` below then overwrites
    // this with the final length — a non-`Diag` ends the tail — so the charge is invisible from
    // outside `wrap_hole`; it is written anyway, because a cell that is only right because the
    // next statement happens to overwrite it is a cell that breaks the day it does not.)
    self.diag_tail += 1;
    self.events.insert(at, start);
    self.push_event(Event::FinishNode { kind: error_kind });
  }
}

impl<'inp, L, E> Sink<'inp, L, E>
where
  L: Lexer<'inp>,
{
  /// The inner emitter's construction-time reading — the no-row rewind's inner target, used
  /// only for the full unwind to the **origin** — the one no-row case with an exact
  /// reconstruction (empty log ⟺ construction reading). Primed at the first **advancing** touch:
  /// `forward_diag` (emissions) and `commit_token` (settles) both prime before forwarding, and
  /// those two are the sink's only inner-advancing surfaces (labels are scope state that never
  /// moves a checkpoint reading, by the trait's label law). Whenever this value is read it
  /// therefore equals the reading `inner.checkpoint()` returned at construction; laziness
  /// exists only to keep the constructor free of emitter bounds, not to let the base drift.
  fn base_inner_mark<Lang>(&mut self) -> u64
  where
    Lang: ?Sized,
    E: Emitter<'inp, L, Lang>,
  {
    match self.base_inner {
      Some(mark) => mark,
      None => {
        let mark = <E as Emitter<'inp, L, Lang>>::checkpoint(&mut self.inner);
        self.base_inner = Some(mark);
        mark
      }
    }
  }

  /// CST_FORWARD_CENSUS — the ONE helper every forwarded diagnostic routes through: call
  /// the inner emitter, then append a `Diag` slot **regardless of the verdict**
  /// (record-then-propagate: transaction guards rewind during fatal unwinds, so a slot
  /// skipped on the `Err` edge would drop an `error_span` a later `finish` needs to cover).
  ///
  /// `error_span` is `Some` at exactly **one** call site —
  /// [`commit_lexer_error`](Emitter::commit_lexer_error), the input layer's own refusal over
  /// bytes it lexed and could not tokenize — and it is recorded into the slot so `finish`'s
  /// gap-coverage law can tell a legitimately-refused byte from a dropped committed token.
  /// Every other forwarded diagnostic passes `None`, **including the caller-facing
  /// [`emit_lexer_error`](Emitter::emit_lexer_error)**: that one carries a span the caller chose
  /// with nothing consumed for it, which is the `cst_token` shape on the diagnostic channel. The
  /// asymmetry is the whole point of having two doors, and COVERAGE_EVIDENCE_CENSUS pins the
  /// `Some(` site at exactly one.
  ///
  /// The inner emitter's rewind reading is captured on the mark-stack row at
  /// [`checkpoint`](Emitter::checkpoint), not here — a forwarded diagnostic advances the
  /// inner but records no rewind target of its own. This helper still primes `base_inner`
  /// before the first forwarded emission; `commit_token` does the same for settles — together
  /// the two advancing surfaces pin the base to the construction-time reading.
  ///
  /// Every `emit_*` of every implemented emitter trait calls this; none touches
  /// `self.inner` directly. The source census test locks the discipline.
  fn forward_diag<Lang, R>(
    &mut self,
    error_span: Option<L::Span>,
    forward: impl FnOnce(&mut E) -> R,
  ) -> R
  where
    Lang: ?Sized,
    E: Emitter<'inp, L, Lang>,
  {
    // The base must predate the first forwarded emission: it is the no-row origin-rewind target.
    let _ = self.base_inner_mark::<Lang>();
    let out = forward(&mut self.inner);
    self.push_event(Event::Diag { error_span });
    out
  }
}

impl<'inp, L, E, Lang> Emitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  // The *Inner-emitter contract* on the type, in the type system. This is where the requirement
  // belongs rather than on the struct: `checkpoint`, `rewind` and `release` — the three methods
  // whose exactness the promise underwrites — are all right here.
  E: Emitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  type Error = E::Error;

  /// The **diagnostic** door: a lexer-error report raised by a caller — a parser through
  /// [`InputRef::emit_lexer_error`](crate::InputRef::emit_lexer_error), a callback through
  /// [`EmitterView::emit_lexer_error`](crate::EmitterView::emit_lexer_error), a wrapper through
  /// either. It forwards the report and occupies a `Diag` slot like every other emission, and it
  /// records **no coverage span**: the caller chose that span, and nothing was consumed for it.
  ///
  /// This is the `cst_token` shape on the diagnostic channel, and it is closed the same way.
  /// A recorded lexer-error span is what *licenses* a gap tile at materialization, so accepting a
  /// caller's span here would let anyone who can reach this emitter excuse an uncovered byte of
  /// the sink's own buffer — including, on the documented orphan route, a **foreign** parse whose
  /// spans index a buffer this sink never saw. The evidence door is
  /// [`commit_lexer_error`](Emitter::commit_lexer_error), which only the input layer calls.
  ///
  /// The capability is not withheld, only its structural side effect: the report still reaches
  /// the inner emitter, so a caller can still say *"this input is malformed here"* inline, with
  /// no rewind — which is the whole reason the `decide` family exists.
  #[inline]
  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_lexer_error(err))
  }

  /// The **evidence** door: the input layer's own lexer error, over bytes it lexed and refused.
  ///
  /// This is the coverage channel's one and only producer — the refusal-side twin of
  /// [`commit_token`](Emitter::commit_token), reached from the input layer's single deduped
  /// reporting site and from nowhere a caller can stand. The span is recorded into the `Diag`
  /// slot so `finish`'s gap-coverage law can tell this legitimately-refused region from a dropped
  /// committed token.
  #[inline]
  fn commit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(Some(err.span_ref().clone()), |inner| {
      inner.commit_lexer_error(err)
    })
  }

  #[inline]
  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_unexpected_token(err))
  }

  #[inline]
  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_error(err))
  }

  #[inline]
  fn emit_warning(&mut self, warning: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_warning(warning))
  }

  /// The construction-bound source — the sink is the one emitter in this crate that has one.
  ///
  /// `finish` slices every token's text out of this buffer, so a parse driven over a
  /// *different* buffer produces a tree whose structure came from one source and whose text
  /// came from another. Answering here is what lets the parse entry refuse that pairing; see
  /// [`Emitter::bound_source`] for the law and for what an inequality is allowed to prove.
  #[inline(always)]
  fn bound_source(&self) -> Option<crate::source::SourceIdentity> {
    Some(crate::source::SourceIdentity::of(self.source))
  }

  /// Wraps the hole's already-buffered token events in an `error_kind` node at the
  /// recovery site (empty holes and token-less holes produce no node), then forwards the
  /// one-per-hole diagnostic to the inner emitter through the census helper.
  ///
  /// **Span semantics.** The wrap brackets only those hole tokens that settled *within the
  /// transaction reporting the hole* — the buffered tokens at or above the youngest live
  /// checkpoint. A `span` reaching back over tokens committed before that checkpoint reaches
  /// the diagnostic channel unchanged, but exerts no structural authority over them: the
  /// `error_kind` node stops at the transaction boundary. Checkpoint marks are event-buffer
  /// positions, and a node spliced beneath one would rename it, so `wrap_hole`'s backward scan
  /// is floored. This costs the crate's own recovery nothing (the scan's span never reaches
  /// below a live capture); it bounds a caller running its own recovery loop.
  fn emit_skipped_region(&mut self, span: L::Span, skipped: usize) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    if skipped > 0 {
      self.wrap_hole(&span);
    }
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_skipped_region(span, skipped))
  }

  /// One positional mark over one unified log: the event-buffer length. The capture also
  /// pushes a mark-stack row freezing the derived depth, so the depth recount — consulted
  /// by [`cst_finish`](CstEmitter::cst_finish)'s global-underflow assert, among others —
  /// has a short anchor to start from instead of rescanning the log from its start, and so
  /// [`release`](Emitter::release) has a row to reclaim.
  ///
  /// The row additionally captures the inner emitter's **own** checkpoint reading, handed
  /// back at the matching [`rewind`](Emitter::rewind) so the inner is restored to exactly
  /// its state at this mark — every forwarded token AND diagnostic before the mark survives,
  /// every one after is undone. This requires a value-keyed inner (a pure monotone
  /// `checkpoint`, a drop-by-value `rewind`, a no-op `release`); the `commit_token`-forwarding
  /// token-tracking inner the sink now supports is exactly that shape.
  fn checkpoint(&mut self) -> u64 {
    let mark = self.events.len() as u64;
    let depth = self.depth();
    let diag_tail = self.diag_tail;
    #[cfg(debug_assertions)]
    let open_top = self.open_top;
    let inner = self.inner.checkpoint();
    self.rows.push(MarkRow {
      mark,
      depth,
      diag_tail,
      #[cfg(debug_assertions)]
      open_top,
      inner,
    });
    mark
  }

  /// Truncate + reverse-replay + inner rewind: drop the events above the mark, undo the
  /// journaled `forward_parent` writes whose `StartAt`s died, record the truncation in
  /// the era ledger (marks into the dropped region are stale forever), and rewind the
  /// inner emitter to a reading the sink **knows exactly** — the target row's captured
  /// reading, or the construction-time base for a no-row unwind to the origin. An
  /// out-of-range FUTURE mark — `checkpoint` strictly above the current length — names a
  /// log position that does not exist yet: the sink ignores the call outright, a **total
  /// no-op on every channel** (events, mark stack, floor, journal, ledger, and the inner
  /// alike; no live row can sit above the length, so no settle is owed). Clamping it to
  /// the length instead — the pre-fix behavior — let a future mark spend the live row of
  /// a real checkpoint taken at the current length; `Verbose` may clamp only because it
  /// keeps no per-mark bookkeeping. A rewind to a mark exactly **at** the current length
  /// is the trait's rewind-to-current law — a no-op on every observable channel that
  /// still spends its capture's row.
  ///
  /// # Panics
  ///
  /// A **truncating rewind to a mid-log mark no live row captured** panics, in **every**
  /// build — release included. The mark was never returned by
  /// [`checkpoint`](Emitter::checkpoint), or its capture was already spent by an earlier
  /// `rewind`/`release`: an **unpaired settle**. That is a parser bug and never an
  /// input-dependent condition — a malformed document changes which branches run, not
  /// whether a branch settles its own capture exactly once — so hardening it cannot turn a
  /// bad document into a crash.
  ///
  /// It was a `debug_assertions`-only wall through 0.7.3, on the reasoning that the input
  /// layer's LIFO witness already rejected it upstream. That witness is *itself* debug-only
  /// (`InputRef::restore`, the `unstable-raw` valve), so a release build had no wall on
  /// either layer: the two logs silently sheared, the sink's own channels exact and the
  /// inner's stale. Bounded while materialization reads the whole log at once; a silently
  /// wrong tree the moment any of it is flushed incrementally.
  ///
  /// **The panic is raised before the first mutation, so it is transactional.** The condition
  /// is decided by a read-only preflight over the unchanged mark stack — ahead of the row
  /// spend, the truncation, the journal replay and the ledger write, all of which the
  /// violating call would otherwise have already performed by the time the wall fired. A wall
  /// placed after the damage only narrates it: a host that catches this panic would be left
  /// holding a sink whose event log had been rewound and whose inner had not, and
  /// [`Cst::finish_partial`](super::Cst::finish_partial) would then hand back that sheared state as a
  /// tree. Caught here, the sink is exactly as it was, on every channel.
  ///
  /// **The one exception is an unwind already in progress**, and it is a guard against a
  /// worse outcome rather than a softening. This method may run from a rolling-back guard's
  /// `Drop` (see the mid-unwind clause on [`Emitter::rewind`]), where a panic is a *double*
  /// panic and aborts the process — strictly worse than the violation it would report. When
  /// `std::thread::panicking` is true the report is therefore suppressed, and the call
  /// degrades to a **total no-op on every channel** — the same shape as the out-of-range
  /// future mark above, and for the same reason: the sink has no correct rewind to perform,
  /// so it performs none, rather than rewinding half of itself. The degradation is
  /// **latched**, and materialization refuses through both doors afterwards
  /// ([`FinishError::UnpairedSettle`]). Silence there is only permissible because it is
  /// recorded; a caller that catches the original panic must not be able to obtain a tree
  /// that looks like the product of a rollback that never happened.
  fn rewind(&mut self, cursor: &Cursor<'inp, '_, L>, checkpoint: u64)
  where
    L: Lexer<'inp>,
  {
    let len = self.events.len() as u64;
    if checkpoint > len {
      // root guard — an out-of-range FUTURE mark names a log position that does not
      // exist yet, so there is nothing to rewind on ANY channel: return before every
      // consumer of the mark (row pops, floor, truncation, journal replay, ledger, the
      // inner enumeration — and any consumer added later). The pre-fix
      // `checkpoint.min(len)` clamp instead dressed a future mark up as a
      // rewind-to-current, and the row lookup below then spent the live row of a REAL
      // checkpoint taken at the current length; that checkpoint's own later rewind found
      // no row (now the unconditional mid-log panic below). No live row
      // can sit above `len` (rows are pushed at the current length and truncation pops
      // them first), so no settle is owed here — the no-op is exact, not defensive.
      // `Verbose` may clamp only because it keeps no per-mark bookkeeping: clamp and
      // ignore coincide there; the sink's mark-keyed row stack makes them differ.
      // The boundary is strict: `checkpoint == len` IS the current position — a lawful
      // rewind-to-current that must still spend its capture's row below.
      return;
    }
    let mark = checkpoint;

    // ── PREFLIGHT — the unpaired-settle verdict, read off UNCHANGED state ──────────────
    //
    // Nothing above this point has written to any cell (the future-mark guard returns, and
    // `len`/`mark` are reads), so this is the last instant at which the sink is still exactly
    // as the caller left it. The wall has to be here, not at the inner-target match below: by
    // the time that match runs, this method has already spent the rows at and above the mark,
    // truncated the event log, reverse-replayed the journal and appended a truncation to the
    // ledger. Reported from there, a caught panic leaves the sink SHEARED — its own channels
    // rewound, the inner not — and `finish`/`finish_partial` will happily materialize that.
    // Reported from here, a caught panic leaves the sink untouched.
    //
    // The predicate is the exact read-only mirror of the spend below: the pop loop stops at
    // the newest row whose mark is at or below the target, and the target row is that row iff
    // its mark IS the target. `find` over the reverse iterator names the same row without
    // removing anything. (Rows are pushed at the then-current length and every truncation pops
    // the rows above it, so the stack is sorted non-decreasing by mark; the reverse scan
    // mirrors the loop whether or not that holds.) The two exempt no-row cases are exempt for
    // the reasons the match below states: `mark == len` truncates nothing, and `mark == 0` has
    // an exact reading in the construction-time base.
    let unpaired_settle = mark > 0
      && mark < len
      && !self
        .rows
        .iter()
        .rev()
        .copied()
        .find(|row| row.mark <= mark)
        .is_some_and(|row| row.mark == mark);
    if unpaired_settle {
      // `rowan` implies `std` (this whole module is behind that feature), so the
      // in-flight-panic query is as available as the `Arc`s rowan itself uses.
      if !std::thread::panicking() {
        panic!(
          "Sink rewind to a mid-log mark with no captured row: mark {mark} of a \
           {len}-event log was never returned by checkpoint(), or its capture was already \
           spent by an earlier rewind or release — no exact inner reading exists for it"
        );
      }
      // Mid-unwind, where a panic is a DOUBLE panic and aborts the process. Reporting is off,
      // so the rewind degrades — and it degrades to NOTHING, on every channel, exactly like
      // the out-of-range future mark above. The sink has no correct rewind to perform here (no
      // exact inner reading exists for this mark), and performing the half it *can* is what
      // shears the two logs. Leaving both un-rewound at least keeps them describing the same
      // history; it is wrong about the parser's intent, never wrong about itself.
      //
      // Latched, first-wins, and never rewound away: `finish`/`finish_partial` refuse
      // afterwards (`FinishError::UnpairedSettle`). Staying quiet is only defensible because
      // this record exists — otherwise a host that catches the original panic could still ask
      // for a tree and get one that silently reflects a rollback that never ran.
      if self.degraded.is_none() {
        self.degraded = Some(DegradedRewind { mark, len });
      }
      return;
    }

    // Spend the captures at or above the mark, capturing the target row's inner reading as
    // it is spent: everything strictly above dies with the branch; the newest capture at
    // exactly the mark is the one being rewound to, and it carries the exact inner reading to
    // hand back. A disciplined rewind (guards, attempt, the scan family, correct raw
    // save/restore) always finds that row live — a released mark is a committed mark, never
    // rewound to. `None` is the no-row case, resolved below by what the sink still knows —
    // and the preflight above has already removed the one no-row case with no answer.
    let target_row = {
      let rows = &mut self.rows;
      while rows.last().is_some_and(|row| row.mark > mark) {
        rows.pop();
      }
      let hit = if rows.last().map(|row| row.mark) == Some(mark) {
        rows.pop()
      } else {
        None
      };
      if self.floor_mark > mark {
        self.floor_mark = rows.last().map_or(0, |row| row.mark);
      }
      hit
    };

    if mark < len {
      self.events.truncate(mark as usize);
      // E5 restored, and the restore is a COPY rather than a recount: the spent row froze the
      // depth of exactly the prefix that just survived. `None` here is the origin unwind and
      // nothing else — the preflight above refused every mid-log no-row rewind, and `mark ==
      // len` never enters this block — so `0` is the depth of an empty log, not a fallback.
      // (The `mark == 0` half of that is asserted at the inner-target match below, which takes
      // the same two arms; one statement of it is enough.)
      self.depth = target_row.map_or(0, |row| row.depth);
      // E6 restored by the identical rule and from the identical fact: the spent row froze the
      // `Diag`-tail start of exactly the prefix that just survived, and `0` is where an empty
      // log's tail starts. Rewind is the failure mode a scan watermark normally has; this one
      // has no rule of its own to get wrong, because it reuses E5's.
      self.diag_tail = target_row.map_or(0, |row| row.diag_tail);
      // E7/E8, third and last use of the same rule. The chain is index-keyed and append-only,
      // so dropping the entries whose event just died is a suffix truncation like the log's own
      // and renames no survivor's slot; `opens` is sorted ascending in `start` (entries are
      // appended in event order, and the hole wrap's splice appends above every one of them),
      // so the pop loop removes exactly the entries at or above the mark. The head is a copy
      // off the frozen row — and every ancestor it reaches has a `start` below the mark, so the
      // restored head can never name a popped slot.
      //
      // THE SKIP LINKS RIDE THAT UNCHANGED, which is why they need nothing here. Each entry's
      // `jump` is a PROPER ANCESTOR on its own chain (it is either `parent`, or a composition
      // of two ancestor hops), so the set of slots the search can reach from the restored head
      // is a subset of the parent chain the sentence above already covers: below the mark,
      // therefore kept. And `depth`/`jump` are written once at the push and never revisited, so
      // no survivor's ladder was mutated by the events this rewind is undoing — there is
      // nothing to roll back, only entries to drop. A depth-indexed spine, the O(1) shape, is
      // exactly what this paragraph could NOT say: later opens overwrite its slots, so it would
      // need an O(depth) rebuild right here.
      #[cfg(debug_assertions)]
      {
        while self.opens.last().is_some_and(|node| node.start >= mark) {
          self.opens.pop();
        }
        self.open_top = target_row.and_then(|row| row.open_top);
      }
      // Reverse-replay the undo journal: every forward_parent write carried by a
      // truncated StartAt is reversed, newest first, restoring the overwritten value —
      // the Verbose parallel-maps pop discipline lifted to events. A write whose target
      // slot was itself truncated has nothing left to restore; its entry just pops.
      while self.journal.last().is_some_and(|entry| entry.at_len > mark) {
        let entry = self.journal.pop().expect("guarded by the loop condition");
        if let Some(Event::StartNode { forward_parent, .. }) =
          self.events.get_mut(entry.index as usize)
        {
          *forward_parent = entry.old_forward_parent;
        }
      }
      self.ledger.record_truncation(mark);
    }

    // The inner is rewound only to a reading the sink knows EXACTLY:
    //   - a spent row's captured reading (any mark) — every disciplined path lands here;
    //   - nothing at all when nothing was truncated (a no-row rewind at the current length;
    //     an out-of-range future mark never reaches this match — it early-returned above as
    //     a total no-op): the surviving events are the whole log, so every inner-side
    //     record they reference must survive with them — the trait's rewind-to-current
    //     no-op law, upheld on every channel (this arm wins the len == 0 overlap: with an
    //     empty log nothing ever advanced the inner, so skipping is exact there too);
    //   - the construction-time base for a no-row unwind to the ORIGIN, exact by the
    //     advancing-surfaces law: every inner advance appends an event and primes the base
    //     first, so an empty event log pairs with exactly the construction reading.
    // The third no-row case — a truncating rewind to a MID-LOG mark — has no exact reading
    // anywhere: the inner's reading is inner-specific (an emission-log length, a token count,
    // a constant) and was never captured at that mark. It cannot reach this match at all. The
    // PREFLIGHT at the top of the body decided it against unchanged state and either panicked
    // (the normal path) or returned as a total no-op after latching the poison (mid-unwind),
    // which is why the last arm below can take the `mark == 0` reading without a guard: the
    // only two no-row cases left are the two exact ones. Rewinding to `base` for a mid-log
    // mark — the pre-0.8 behavior — or to a neighboring row's reading would destroy committed
    // inner state the surviving log still carries; the sink hands the inner a reading it can
    // prove, or nothing at all.
    let inner_target = match target_row.map(|row| row.inner) {
      Some(reading) => Some(reading),
      None if mark == len => None,
      None => {
        debug_assert_eq!(
          mark, 0,
          "the preflight refuses every mid-log no-row rewind before this point"
        );
        Some(self.base_inner_mark::<Lang>())
      }
    };
    if let Some(reading) = inner_target {
      self.inner.rewind(cursor, reading);
    }
  }

  /// The auto-emission hook: the input layer settles every committed token through this
  /// one call — the consume settles via its `commit_token` primitive, the scan skips via
  /// `skip_and_report` — so the whole consume surface is tree-producing with zero per-atom
  /// code. It is the token channel's **only** door: the `Token` event exists because a token
  /// settled, never because a caller said so.
  #[inline]
  fn commit_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'inp>,
  {
    // The base reading must predate the first settle the inner observes — the settle-side
    // twin of forward_diag's prime. Whichever advancing surface fires first freezes the
    // inner's construction-time reading (see base_inner_mark).
    let _ = self.base_inner_mark::<Lang>();
    self.record_token(tok, span);
    self.inner.commit_token(tok, span);
  }

  /// Pops the kept capture's row off the mark stack — the eviction dual of
  /// [`checkpoint`](Self::checkpoint) that keeps the stack at exactly the live captures
  /// (commit-heavy loops would otherwise strand one dead row per committed guard, and a
  /// stale row is exactly the aliased-mark state the length-mark design must never
  /// consult). The popped row's mark becomes the sink's released-floor memo: the youngest
  /// committed positional fact it holds, and the floor of the hole wrap's backward scan.
  /// Marks arrive newest-first on the crate's paths (O(1) top pop); a mark already gone is a
  /// no-op, per the trait's advisory contract.
  ///
  /// The row's captured inner reading is deliberately **not** forwarded to `inner.release` —
  /// it is a plain value, not an inner-side resource (see *Inner-emitter contract* on the
  /// type); the forward census pins `self.inner.release` at zero so any future forwarding
  /// change must rewrite the contract deliberately.
  ///
  /// # Why the two non-top branches stay silent
  ///
  /// [`rewind`](Emitter::rewind) panics in every build on an unpaired settle. `release` does
  /// **not**, and the asymmetry is forced rather than an oversight: its two non-top outcomes
  /// are *specified behaviour*, not violations.
  ///
  /// - **Removing a non-innermost row** (`rposition` + `remove`) is the emitter-side mirror of
  ///   `InputRef::commit`'s documented cost model — "`O(1)` when
  ///   `checkpoint` is the youngest live checkpoint … and a linear removal otherwise (e.g. a
  ///   younger raw checkpoint was dropped above it); the rest of the stack keeps its order
  ///   either way". Settles are newest-first only *within* a family: the trait's own
  ///   [`release`](Emitter::release) contract states that guards and session points interleave,
  ///   so an out-of-stack-order release is lawful by construction.
  /// - **Finding no row at all** is the same doc's "harmless **no-op**: its id is simply
  ///   absent, so nothing is released and no state changes (no panic, in any build)".
  ///
  /// Hardening either would convert a documented guarantee into a crash. Neither can strand
  /// the mark stack in a shape that outlives the parse, either: a middle `remove` only fires
  /// when the top row's mark differs, so `rows` holds at least two entries and cannot be
  /// emptied by it, and equal-mark rows are interchangeable (same prefix ⟹ same depth and
  /// same inner reading, guaranteed by the [`ValueKeyedEmitter`] bound), so which of them is
  /// removed is unobservable. The residue a dropped-rather-committed raw checkpoint leaves is
  /// a *stale* row — the mark stack is a superset of the live captures, never a subset, which
  /// is the conservative direction for any consumer that keys on the oldest live mark. The one
  /// way a *live* capture loses its row is a double settle, and that surfaces where it should:
  /// at the later `rewind`, which panics.
  fn release(&mut self, checkpoint: u64) {
    let rows = &mut self.rows;
    let row = if rows.last().map(|row| row.mark) == Some(checkpoint) {
      rows.pop()
    } else {
      // Both non-top outcomes are lawful, not violations — see the contract note above.
      rows
        .iter()
        .rposition(|row| row.mark == checkpoint)
        .map(|pos| rows.remove(pos))
    };
    if let Some(row) = row
      && row.mark >= self.floor_mark
    {
      self.floor_mark = row.mark;
    }
  }

  #[inline]
  fn enter_label(&mut self, label: &'static str) {
    // Labels are not emissions: the inner's live stack follows the wrapper scopes, and
    // snapshots ride the inner's own entries — no order fact belongs in the event log.
    self.inner.enter_label(label);
  }

  #[inline]
  fn exit_label(&mut self) {
    self.inner.exit_label();
  }
}

impl<'inp, L, E, Lang> CstEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  fn cst_start(&mut self, kind: u16) -> EventMark
  where
    L: Lexer<'inp>,
  {
    assert!(
      self.profile.validator().admits(kind),
      "node kind outside the dialect's own kind space (the tombstone kind, u16::MAX, is \
       reserved and no validator admits it)"
    );
    let index = self.events.len() as u64;
    self.push_event(Event::StartNode {
      kind,
      forward_parent: None,
    });
    EventMark::new(index, self.ledger.era(), self.witness)
  }

  fn cst_finish(&mut self, kind: u16)
  where
    L: Lexer<'inp>,
  {
    // Detect-at-cause for TRUE GLOBAL underflow only: a finish must close *some* node
    // that is open anywhere in the buffer (`depth() > 0`). This is the soundest
    // invariant a DEPTH-ONLY predicate can enforce; it is deliberately no stricter,
    // because depth cannot separate the two histories that reach this call with a node
    // still open:
    //
    //   - LEGAL cross-checkpoint close — `cst_start(A); checkpoint m; <token settles>;
    //     cst_finish(A)`: A was opened, never rolled back, and this finish closes it. Under
    //     commit/release both events survive balanced; under a rewind of `m` the finish
    //     truncates and A reopens (the truncate-and-reopen semantics the CstEmitter contract
    //     blesses, see `emitter/cst.rs`). The OLD assert compared depth against the innermost
    //     live capture's *frozen* baseline and panicked on this legal history — the defect
    //     this narrowing fixes (see issue #98).
    //
    //   - LEAKED-FINISH misuse — `cst_start(A); checkpoint m; cst_start(B); rewind(m);
    //     <token settles>; cst_finish(B)`: the finish was meant for B, but B's start died with the
    //     rewind, so it would silently close ancestor A instead. After the rewind the event
    //     buffer is IDENTICAL to the legal case above — depth cannot tell them apart, and
    //     neither can the whole buffer. The `kind` argument is what separates them: it
    //     travels into the `FinishNode` event and materialization compares it against the
    //     frame it would close (`FinishError::MismatchedFinish`).
    //
    // The kind comparison is NOT also made here, at the emit site, and the reason is
    // measured rather than conceded. An at-cause answer would have to name the frame
    // *materialization* would close, and materialization hoists every retro-wrap to its
    // target: same-target wraps open newest-first, so the first finish closes the
    // FIRST-declared wrap, and a wrap declared after a still-open direct start nests OUTSIDE
    // it. Both orders are inverted relative to the emission-order suffix a depth recount
    // sees, so a recount-derived assert would fire on correct code. Reproducing the true
    // order needs a live shadow stack keyed by materialization position — a middle-insert
    // structure with its own undo journal across rewinds, i.e. another censused cell and an
    // allocation on the hot emit path. The typed error at materialization is the wall; a rail
    // that fails on correct code would be worse than none.
    //
    // Debug-only, and it STAYS debug-only now that it is O(1). The tier is not a cost
    // concession that the maintained depth has made obsolete: release already refuses this
    // typed, through both finish doors (`OrphanFinish`/`MismatchedFinish`), and a typed refusal
    // a host can catch is a stronger wall for a library than a panic, not a weaker one. The
    // rewind preflight is an every-build panic precisely because release had NO wall there.
    debug_assert!(
      self.depth() > 0,
      "cst_finish with no open node anywhere in the buffer (global underflow): the \
       matching start was rolled back (or never emitted), and no enclosing node is open \
       to close instead"
    );
    self.push_event(Event::FinishNode { kind });
  }

  fn cst_demote(&mut self, mark: EventMark, kind: u16)
  where
    L: Lexer<'inp>,
  {
    self.validate_start_mark(&mark, kind);

    // APPEND, never rewrite — the mirror of `cst_start_at` below, and the whole of why the
    // bracket's two exits obey ONE rollback law. Rewriting the slot's kind here would survive a
    // rollback whose target sits between the slot and this call: the truncation keeps the slot
    // and keeps the rewrite, so the failing exit would silently drop a node that, by the
    // restore contract, is open again — while the success exit's appended `FinishNode`
    // truncates and the node correctly returns. Appending makes the failing exit acquire the
    // success exit's blessed truncate-and-reopen semantics exactly.
    //
    // Materialization applies it: one canonicalization pass sets `events[target].kind =
    // TOMBSTONE` per surviving `Demote`, at the one moment no live mark can exist. A `Demote`
    // can never orphan its target — `target` is strictly below this index and truncation is a
    // suffix operation, so the two survive and die together, in the same era.
    self.demotes = true;
    self.push_event(Event::Demote {
      target: mark.index(),
    });
  }

  fn cst_mark(&mut self) -> EventMark
  where
    L: Lexer<'inp>,
  {
    let index = self.events.len() as u64;
    self.push_event(Event::StartNode {
      kind: TOMBSTONE,
      forward_parent: None,
    });
    EventMark::new(index, self.ledger.era(), self.witness)
  }

  fn cst_start_at(&mut self, mark: EventMark, kind: u16)
  where
    L: Lexer<'inp>,
  {
    self.validate_mark(&mark);
    assert!(
      self.profile.validator().admits(kind),
      "retro-wrap kind outside the dialect's own kind space (the tombstone kind, u16::MAX, \
       is reserved and no validator admits it)"
    );
    let target = mark.index();
    let new_index = self.events.len() as u64;
    let relative = u32::try_from(new_index - target)
      .ok()
      .and_then(NonZeroU32::new);

    // The chain link, read BEFORE the append that displaces it: this wrap's `prev` is the
    // value it is about to overwrite on the target — the previous newest wrap of the same
    // tombstone. Written once, at this event's own append (the append-only law), from the
    // exact reading the journal below records.
    let prev = match self.events.get(target as usize) {
      Some(Event::StartNode { forward_parent, .. }) if relative.is_some() => *forward_parent,
      _ => None,
    };
    self.push_event(Event::StartAt { kind, target, prev });

    // The one journaled in-place write: point the tombstone's forward_parent at the newest
    // wrap, making it the head of the chain materialization walks (`prev` carries the
    // rest) and the integrity canary finish validates in both directions. The journal is
    // what keeps it honest across rewinds — restoring the overwritten value is the
    // pure-copy discipline, and it restores the chain head together with it.
    if let Some(relative) = relative
      && let Some(Event::StartNode { forward_parent, .. }) = self.events.get_mut(target as usize)
    {
      // The journal is STRICTLY INCREASING in `at_len`, and `wrap_hole`'s O(1) no-fix-up
      // assert rests on it: with the newest entry the largest, checking `last()` decides the
      // whole journal. Every push takes `events.len() + 1` after an append, and a rewind pops
      // every entry above its mark, so the order cannot invert — this pins that, in debug, at
      // the one site that could break it.
      debug_assert!(
        self
          .journal
          .last()
          .is_none_or(|prev_entry| prev_entry.at_len < new_index + 1),
        "undo-journal order inverted: entries must be strictly increasing in at_len, which is \
         what lets rewind's pop loop and wrap_hole's O(1) splice check read only the newest one"
      );
      self.journal.push(JournalEntry {
        at_len: new_index + 1,
        index: target,
        old_forward_parent: *forward_parent,
      });
      *forward_parent = Some(relative);
    }
  }
}

// ── The forwarded capability family ─────────────────────────────────────────────
//
// Every atomic emitter trait the crate ships forwards through the ONE census helper, so
// `Sink<E>` satisfies every context bound `E` satisfies (the `ComposableEmitter`-shaped
// bundles downstream) and every forwarded diagnostic occupies a Diag slot in the unified
// log. CST_FORWARD_CENSUS locks the set.

/// The delimiter capability, routed like every other diagnostic: the event log records a
/// `Diag` slot and the inner emitter receives the payload.
///
/// It was **missing**, and its absence was not cosmetic: [`UnclosedEmitter`] is a member of
/// [`ComposableEmitter`](crate::emitter::ComposableEmitter), so without it a `Sink` could not
/// satisfy the one-bound collecting surface at all — a delimited CST parse could not name the
/// bundle its diagnostics-only sibling names. The forwarding census could not see the gap
/// either, because it hard-coded a capability list that omitted this door.
impl<'inp, L, E, Lang> UnclosedEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: UnclosedEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: crate::error::Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_unclosed(err))
  }
}

impl<'inp, L, E, Lang> TooFewEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: TooFewEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_too_few(&mut self, err: TooFew<L::Span, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_too_few(err))
  }
}

impl<'inp, L, E, Lang> TooManyEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: TooManyEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_too_many(&mut self, err: TooMany<L::Span, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_too_many(err))
  }
}

impl<'inp, L, E, Lang> FullContainerEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: FullContainerEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_full_container(&mut self, err: FullContainer<L::Span, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_full_container(err))
  }
}

impl<'inp, L, E, Lang> SeparatedEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: SeparatedEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_missing_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_missing_separator(name, err))
  }

  #[inline]
  fn emit_missing_element(&mut self, err: MissingSyntaxOf<'inp, L, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_missing_element(err))
  }
}

impl<'inp, L, E, Lang> MissingLeadingSeparatorEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: MissingLeadingSeparatorEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_missing_leading_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| {
      inner.emit_missing_leading_separator(name, err)
    })
  }
}

impl<'inp, L, E, Lang> MissingTrailingSeparatorEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: MissingTrailingSeparatorEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_missing_trailing_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| {
      inner.emit_missing_trailing_separator(name, err)
    })
  }
}

impl<'inp, L, E, Lang> UnexpectedLeadingSeparatorEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: UnexpectedLeadingSeparatorEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_unexpected_leading_separator(
    &mut self,
    name: CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| {
      inner.emit_unexpected_leading_separator(name, err)
    })
  }
}

impl<'inp, L, E, Lang> UnexpectedTrailingSeparatorEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: UnexpectedTrailingSeparatorEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_unexpected_trailing_separator(
    &mut self,
    name: CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| {
      inner.emit_unexpected_trailing_separator(name, err)
    })
  }
}

#[cfg(feature = "pratt")]
impl<'inp, L, E, Lang> PrattEmitter<'inp, L, Lang> for Sink<'inp, L, E>
where
  L: Lexer<'inp>,
  E: PrattEmitter<'inp, L, Lang> + ValueKeyedEmitter,
  Lang: ?Sized,
{
  #[inline]
  fn emit_unexpected_end_of_lhs(
    &mut self,
    err: UnexpectedEoLhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_unexpected_end_of_lhs(err))
  }

  #[inline]
  fn emit_unexpected_end_of_rhs(
    &mut self,
    err: UnexpectedEoRhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    self.forward_diag::<Lang, _>(None, |inner| inner.emit_unexpected_end_of_rhs(err))
  }
}

// ── Test observability ──────────────────────────────────────────────────────────

/// The release no-growth observable, shared with the public fuzz harness
/// (`feature = "fuzz"`): its recording-twin driver asserts through this that every
/// checkpoint capture was settled once a script ends.
#[cfg(any(test, feature = "fuzz"))]
impl<'inp, L, E> Sink<'inp, L, E>
where
  L: Lexer<'inp>,
{
  /// The number of live mark-stack rows (the release no-growth oracle).
  pub(crate) fn rows_len(&self) -> usize {
    self.rows.len()
  }
}

#[cfg(test)]
impl<'inp, L, E> Sink<'inp, L, E>
where
  L: Lexer<'inp>,
{
  /// The event-buffer view, for shape assertions.
  pub(crate) fn events(&self) -> &[Event<L::Span>] {
    &self.events
  }

  /// The event buffer's allocated capacity — the construction-time reservation, before
  /// anything has been appended.
  pub(crate) fn events_capacity(&self) -> usize {
    self.events.capacity()
  }

  /// The number of live undo-journal entries.
  pub(crate) fn journal_len(&self) -> usize {
    self.journal.len()
  }

  /// The tombstone's forward_parent pointer at `index`, if that slot is a start.
  pub(crate) fn forward_parent_at(&self, index: usize) -> Option<NonZeroU32> {
    match self.events.get(index) {
      Some(Event::StartNode { forward_parent, .. }) => *forward_parent,
      _ => None,
    }
  }

  /// Test-only raw event injection: constructs the corrupt shapes the emission-time checks
  /// refuse, so the materialization walls that backstop them — the **release** line of
  /// defense — can be exercised at all. The three checks do not share a build gate: an
  /// orphan finish (`cst_finish`'s `debug_assert!`) is debug-only, while a reserved kind and
  /// a stale wrap target (`record_token`/`cst_start`/`cst_start_at`'s and `validate_mark`'s
  /// plain `assert!`s) are unconditional — refused in every build — so this hook is their
  /// only bypass at all, not merely their debug-build bypass.
  pub(crate) fn push_raw_event_for_tests(&mut self, event: Event<L::Span>) {
    // The latch's invariant is "set whenever a Demote is in the buffer", and this hook is the
    // one route that appends an event without going through an emission door, so it maintains
    // it here rather than leaving the canonicalization pass to be skipped over an injected
    // demote.
    self.demotes |= matches!(event, Event::Demote { .. });
    // Through the one append door like every other emission, so an injected event charges E5
    // exactly as the door that would have produced it does: the raw shapes this hook exists to
    // build are the ones the depth oracle most needs to see, and a bypass here would make the
    // scalar and the recount disagree for a reason that is the harness's fault, not the sink's.
    self.push_event(event);
  }
}

#[cfg(test)]
mod tests;
