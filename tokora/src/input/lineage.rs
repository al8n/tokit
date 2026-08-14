//! The lineage memos an [`Input`](super::Input) carries so backtracking can rewind an
//! abandoned continuation exactly, gathered behind one guardian — and the **cell taxonomy** that
//! governs every other restorable cell an input owns.
//!
//! # Single-writer taxonomy
//!
//! Every cell an [`Input`](super::Input) or an [`InputRef`](super::InputRef) owns belongs to
//! exactly one of six classes. The class *is* the restore semantics: it decides what a
//! [`restore`](super::InputRef::restore) does to the cell, and it names the cell's single writer.
//! A cell whose behavior does not match its class is a bug, and this taxonomy exists so that
//! adding a cell without deciding which class it is in is not a thing that can happen quietly.
//!
//! - **Ground truth** — the live scanning position and regime the next token is lexed from: the
//!   lexer state, the last-consumed span, the token cache, and the emitter's emission log. The
//!   scan/consume paths are their sole writer. **Restore OVERWRITES them** with the saved values
//!   (the emission log by truncation to the saved mark). These live directly on
//!   [`Input`](super::Input); the layout keeps them packed ahead of this module's memos.
//! - **Lineage memos** — the bookkeeping gathered here, plus the two lineage facts that live on
//!   [`Input`](super::Input) for layout reasons (the poison boundary and the lexer-error dedup
//!   watermark): what a checkpoint must know to rewind an abandoned continuation exactly.
//!   Backtracking (save/restore/commit and the guards) is their sole writer. **Restore PURE-COPIES
//!   them** back to the saved value — with two structural exceptions that are still memos:
//!   the live-checkpoint stack (restore *pops through* the restored id rather than copying a
//!   snapshot) and the pin set (a restore never changes which guards are live).
//! - **Monotone id sources** — the counters that hand out never-reused ids: the checkpoint-id
//!   source and the savepoint sequence. They are memos in spirit but **restore must NOT touch
//!   them**: rewinding a counter would reissue a live id, and an id that can collide is worse than
//!   no id. Distinguished from a *restored* memo like the cache-push counter — which is a fact
//!   *about* the lineage, not an identity source — precisely because their restore semantics are
//!   opposite.
//!
//!   The general law behind that opposition, and the one to apply when adding a cell: **a witness
//!   must share the rollback semantics of the fact it witnesses.** `cache_pushes` is restored
//!   because a restore drops exactly the post-save entries it counts. A cell whose subject does
//!   not restore must not be restored either — and a cell whose subject *is* restored, like the
//!   front-report watermark whose subject is the emission log, must be.
//! - **World facts** — what the *caller* knows about the outside world, and the parse cannot: the
//!   [`Partial`](super::Partial) `is_final` bit. Its sole writer is the driver
//!   ([`Input::seal`](super::Input::seal), which takes `&mut Input`), it is **monotone** (a stream
//!   cannot un-end), and **restore does NOT touch it** — a rollback rewinds the *parse*, not the
//!   *world*. This class is not a loophole in the restore discipline; it is the one place the
//!   discipline must not reach, and it is enforced structurally rather than remembered: an
//!   [`InputRef`](super::InputRef) borrows the [`Input`](super::Input) for its whole life, so the
//!   cell is *unreachable* while any parser, guard, or speculative branch runs. It therefore cannot
//!   change during a handle's life, so no rollback can observe it change, so a
//!   [`Checkpoint`](super::Checkpoint) has nothing to save. Restoring it instead would be the
//!   mirror bug: a rollback across a legitimate seal would un-end an ended stream and the parser
//!   would wait forever for bytes that will never arrive.
//! - **Control-stack facts** — what the *live frames* know, and input progress does not: the
//!   descent depth of the recursion budget. Its sole writer is the [`Descent`](super::Descent)
//!   guard [`descend`](super::InputRef::descend) hands out, and **restore does NOT touch it**. A
//!   save and the restore returning to it sit at the same frame depth by construction, so the cell
//!   cannot be observed to change across the pair; and an unwind pops frames a state-restore knows
//!   nothing about, so the only witness that can be right is one that pops *with* the frame. That
//!   witness is the guard's destructor, which behaves the same in `std` and `no_std` — hence a
//!   [`Checkpoint`](super::Checkpoint) does not carry the cell at all. Sibling of the world fact
//!   above and distinct from it in the way that matters: the world fact is monotone and
//!   unreachable while a handle lives, while this one moves constantly, under a single writer
//!   whose lifetime *is* the frame it counts.
//! - **Witness / instrumentation** — cells that do not affect scanning at all, and are therefore
//!   never restored: the debug-only, process-unique cross-input identity a checkpoint is stamped
//!   with (see [`Witness`](super::Witness); its atomic id source keeps it behind the debug +
//!   `target_has_atomic = "ptr"` gate), and the `trace` nesting depth, whose events travel out of
//!   band (stderr) and so cannot be un-emitted by a rewind anyway.
//!
//! This module is the guardian of the lineage memos: the cells are private to it and reachable
//! only through the operations below, each of which carries the invariant it maintains. Every
//! memo but the cache-push counter is a live-checkpoint concept that only exists where an
//! allocator does, so all of them except [`cache_pushes`](Lineage::cache_pushes) sit behind the
//! allocator gate; an allocator-less build keeps only the counter and every stack operation
//! compiles out.
//!
//! # CELL_CENSUS — every mutable cell, and its class
//!
//! This is the contract, and it is greppable: `grep CELL_CENSUS` finds it from anywhere in the
//! tree. **A new mutable cell on [`Input`](super::Input), [`InputRef`](super::InputRef),
//! [`Session`](super::Session), or [`Lineage`] MUST be added to this table and classified
//! above.** Not merely a *scan-affecting* one, which is how the rule read while three of the rows
//! below — the finality bit, the identity witness, the `trace` depth — already described cells
//! that affect no scan: the qualifier read as an exemption and the table's own promise is
//! "every mutable cell". [`census`] is the tripwire that makes that structural rather than
//! advisory: it destructures both structs exhaustively — no `..` — so a new field is a **compile
//! error, right here, in the guardian**, at the table that asks what class it is in.
//!
//! **What that tripwire does and does not guard.** It guards the *rows*: a new cell cannot appear
//! without one. It cannot guard the *action column*, which is prose — a mechanism can be deleted
//! and leave the table confidently describing it, with every gate green. That happened: 0.8.0
//! removed `Cache::rewind` and this column went on naming it as the live action until a reader
//! traced it. The `taxonomy_census` test closes the cheap half of the gap by requiring every
//! `Type::method` path named in the action column to still exist in the crate; the rest of the
//! column is English and is reviewed by hand, so **a change that deletes or relocates a restore
//! mechanism must come here and re-read this column**, not merely keep the rows correct.
//!
//! # Why the push count is rewound, rather than left to run forward
//!
//! The count is the one memo whose purpose reads like bookkeeping, so the reason lives here and
//! not only at the restore site. A restore drops the cache entries pushed after the save,
//! computed as `cache_pushes - checkpoint.cache_pushes`. Rewinding the count on every restore is
//! what makes that subtraction describe *the current lineage only*, so nested, last-in-first-out
//! restores compose: an inner restore rewinds the count, and the outer one then computes zero
//! post-save pushes and correctly retains a token prefilled before either save.
//!
//! A never-rewound count reads stale-high there and over-drops that token — forcing a **re-lex
//! whose scan side effects belong to the abandoned lineage**: shared lexer and limit state advance
//! a second time, and a poison boundary can latch at a position the checkpoint predates. That is
//! the same harm that ruled out clearing the cache wholesale, measured on the committing path,
//! and it is why this design beats a clear.
//!
//! | Cell | Owner | Class | What restore does |
//! |---|---|---|---|
//! | `input` (the source slice) | `Input` | — (immutable borrow, fixed for the input's life) | nothing |
//! | `state` (the lexer regime) | `Input` | ground truth | overwrite from the checkpoint |
//! | `span` (last-consumed) | `Input` | ground truth | overwrite from the checkpoint |
//! | `cache` (the token cache) | `Input` | ground truth | the input layer rewinds the front against the restored cursor through `Cache::pop_front`/`Cache::clear`, then drops the post-save tail through `Cache::pop_back`, sized by the count below |
//! | `pending` (the parked front token) | `Input` | ground truth | **clear** it — a parked token is not a cache entry and is uncounted, so it is dropped rather than replayed and the region re-lexes it back under the `Lexer` determinism contract |
//! | `emitter` (the emission log) | `Input` (owned; borrowed through `InputRef`) | ground truth | truncate to the saved mark |
//! | `emitted_error_end` (dedup watermark) | `Input` | lineage memo | pure-copy the saved value |
//! | `front_reported_end` (front-report watermark) | `Input` | lineage memo | pure-copy the saved value — beside the emitter rewind, so the witness and the report it names move as one |
//! | `poison_boundary` (sticky terminal frontier) | `Input` | lineage memo | pure-copy the saved value |
//! | [`cache_pushes`](Lineage::cache_pushes) | `Lineage` | lineage memo | pure-copy the saved value — the copy is what makes nested restores compose; see below |
//! | [`live_ckpts`](Lineage) | `Lineage` | lineage memo | pop through the restored id |
//! | [`pinned`](Lineage) | `Lineage` | lineage memo | nothing (a restore does not change which guards are live) |
//! | `points` (open session points) | `Session` | lineage memo | settle the suffix below the restored id (the *checked* rewind is refused by the pin first) |
//! | [`next_ckp_id`](Lineage) | `Lineage` | monotone id source | **nothing** — rewinding would reissue a live id |
//! | [`savepoint_seq`](Lineage) | `Lineage` | monotone id source | **nothing** — same |
//! | `finality` (`is_final`) | `Input` (snapshot on `InputRef`) | **world fact** | **nothing** — and it cannot change while a handle lives |
//! | `recursion` (the descent budget) | `Input` (borrowed through `InputRef`) | **control-stack fact** | **nothing** — a checkpoint does not carry it; the `Descent` guard's drop balances it with the frame it counts |
//! | `token_budget` (the item ceiling) | `Input` (borrowed through `InputRef`) | **monotone session fact** | **nothing** — and here the reason is not that the cell cannot be observed to change, as it is for the two rows around it, but that a budget a rollback refunds is not a budget. An attempt that lexed a thousand items and declined performed a thousand items of scanning; giving the count back would hand an adversary who forces speculation free work without limit. Tested by `InputRef::lex_within_boundary` **before** it invokes the lexer and charged there, one per item a step produced, so the enforcement sits in front of the work rather than behind a stop a rollback can refund; the refusal records that stop through `InputRef::latch_scanner_trip`. No lowering writer exists, and there is no `token_budget_mut`. What it costs is that a re-lex is charged again, so the ceiling bounds produce-events rather than distinct document tokens |
//! | `resource_trips` (how many budget trips) | `Input` (borrowed through `InputRef`) | **monotone session fact** | **nothing** — it only counts up, and there is no writer that lowers it. A rollback rewinds *input progress*; it cannot un-exceed a budget, so restoring this cell would erase a stop that has already been decided. Sites that need a *per-attempt* answer snapshot it and compare rather than reading it absolutely |
//! | `scanner_trips` (how many scanner trips) | `Input` (borrowed through `InputRef`) | **monotone session fact** | **nothing** — the same class and the same argument as the cell above, for the other budget. It is the row that makes the `poison_boundary` row *safe*: a trip's **position** is a lineage memo and comes back with a restore, so no comparison of that memo can witness a trip across a rollback — at one level or at ten. The *fact* of the trip is recorded here instead, where nothing reaches it, which is why the recovery gate judges a scanner stop on this and reads the boundary only as the narrower standing-stop reading |
//! | `witness` (input identity) | `Input` | witness | nothing (identity is fixed for the input's life) |
//! | `depth` (trace nesting) | `Input` | instrumentation | nothing (trace events are out of band) |

#[cfg(any(feature = "std", feature = "alloc"))]
use super::LineageStack;
use super::{Completeness, Input, InputRef};
use crate::{Lexer, ParseContext};

/// CELL_CENSUS — the structural tripwire behind the cell taxonomy in the [module docs](self).
///
/// It destructures [`Input`](super::Input), [`InputRef`](super::InputRef), and [`Lineage`]
/// **exhaustively** — no `..` — and binds every field to nothing. That is the entire point: adding a
/// field to any of them fails to compile *here*, in the guardian, at the taxonomy table that asks
/// which class the new cell is in and what a [`restore`](super::InputRef::restore) must do to it.
/// The one class of bug this module exists to prevent — a cell added without deciding its restore
/// semantics — has shipped twice (the cache-push counter, then the finality flag). Both times the
/// cell was added *next to* the guardian instead of *through* it. This is the wall.
///
/// The census also names the two cells the census cannot see from here, because their fields are
/// private to another module: [`Session`](super::Session)'s (`session::census`, same discipline) and
/// the guards' own (`ckp` / `base` / `saves` / `nonce`, which are guard-local — they die with the
/// guard and are never part of an input's restorable state).
///
/// **Costs nothing.** It is generic and never instantiated, so it is type-checked in every build and
/// monomorphized in none: it contributes zero bytes of code.
#[allow(dead_code)]
pub(crate) fn census<'inp, L, Ctx, Lang, Cmpl>(
  input: &Input<'inp, L, Ctx, Lang, Cmpl>,
  handle: &InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
) where
  L: Lexer<'inp>,
  Lang: ?Sized,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  let Input {
    // — immutable: the source slice is fixed for the input's life.
    input: _,
    // — ground truth: restore OVERWRITES from the checkpoint.
    state: _,
    span: _,
    cache: _,
    // — ground truth, and uncounted: the parked front token is not a cache entry, so restore
    //   CLEARS it rather than replaying it (the region re-lexes and, by the `Lexer` determinism
    //   contract, reproduces it).
    pending: _,
    // — WORLD FACT: monotone, driver-owned, restore does NOT touch it (and cannot: no handle may
    //   exist while `Input::seal` runs). See the module docs.
    finality: _,
    // — lineage memos: restore PURE-COPIES the saved value.
    emitted_error_end: _,
    // — lineage memo: pure-copied, and that is the point. Its subject is the emission log, which
    //   the same restore rewinds, so the witness shares the rollback semantics of the fact it
    //   witnesses. A witness that did not would be unable to see a rollback at all.
    front_reported_end: _,
    poison_boundary: _,
    // — CONTROL-STACK FACT: restore does NOT touch it, and must not. Depth is a property of the
    //   live frames, not of input progress: a save and the restore returning to it happen at the
    //   same depth by construction, so nothing observable changes across the pair; and an unwind
    //   pops frames a state-restore knows nothing about. Its witness is therefore the `Descent`
    //   guard's destructor, which pops with the frame in `std` and `no_std` alike — the one
    //   witness whose behaviour does not fork on the unwind edge. Not in `Checkpoint`.
    recursion: _,
    // — MONOTONE SESSION FACT: restore does NOT touch it, and must not — but for a reason the two
    //   neighbouring cells do not share. `finality` and `recursion` are outside the rollback set
    //   because nothing observable changes across a save/restore pair; this one is outside because
    //   a refunded budget is not a budget. The scanning an abandoned attempt performed was still
    //   performed, and a ceiling that forgets it bounds nothing against a grammar an adversary can
    //   make speculate. That is exactly what the refundable lexer-side `TokenLimiter` inside
    //   `state` cannot do, since a `Checkpoint` carries the state. Read as a gate by
    //   `InputRef::lex_within_boundary` BEFORE it invokes the lexer and charged there one per item
    //   produced — the counter being durable is only half a bound, and the other half is that the
    //   check runs in front of the work rather than behind the poison boundary, which a restore
    //   and a state re-key both clear. No lowering writer, no `token_budget_mut`, not in
    //   `Checkpoint`.
    token_budget: _,
    // — MONOTONE SESSION FACT: restore does NOT touch it, and must not. It is the sibling of the
    //   cell above and the opposite kind of fact from it: the *depth* is restored by the unwind
    //   because frames genuinely came back, while *that the budget was exceeded* is a thing that
    //   happened and cannot stop having happened. Rolling it back would hand recovery a session
    //   claiming a stop it has already been given. Counted up by `raise_level`'s trip arm; no
    //   lowering writer exists. Not in `Checkpoint`. The recovery combinators and the collection
    //   drivers read it as a per-attempt *difference*, never absolutely — see the field's doc.
    resource_trips: _,
    // — MONOTONE SESSION FACT: restore does NOT touch it, and must not. Same class, same argument,
    //   other budget. `poison_boundary` above records the same event as a lineage memo, and that
    //   is the point of having both: the memo says WHERE the stream stops and has to come back
    //   with a restore for the scanner to keep working; this says THAT a stop happened and must
    //   not, because an event a rollback can erase is an event no gate outside that rollback can
    //   see. Counted up by `latch_scanner_trip` — reached from the lexer's own limit probe inside
    //   `classify` and from the token budget's refusal inside `lex_within_boundary`; no lowering
    //   writer exists. Not in
    //   `Checkpoint`, and not in `ThroughEntry` either — the sync family's own positional rewind
    //   restores the boundary and deliberately leaves this alone.
    scanner_trips: _,
    // — GROUND TRUTH, and the aggregate's anchor: the emission log itself, bound at construction
    //   and owned for the input's whole life. Restore truncates it to the saved mark — the same
    //   action as when it was borrowed per handle. What changed is that the two watermarks above
    //   now describe THIS log by construction rather than by call-site discipline, so the live
    //   structure matches the grouping the rollback structure always had.
    emitter: _,
    lineage:
      Lineage {
        // — lineage memo: pure-copied (`restore_cache_pushes`).
        cache_pushes: _,
        // — monotone id source: restore must NOT touch it (a reissued id could collide).
        #[cfg(any(feature = "std", feature = "alloc"))]
          savepoint_seq: _,
        // — lineage memo: restore pops through the restored id.
        #[cfg(any(feature = "std", feature = "alloc"))]
          live_ckpts: _,
        // — monotone id source: restore must NOT touch it.
        #[cfg(any(feature = "std", feature = "alloc"))]
          next_ckp_id: _,
        // — lineage memo: restore does not change which guards are live.
        #[cfg(any(feature = "std", feature = "alloc"))]
          pinned: _,
      },
    // — instrumentation: out of band (stderr), never restored.
    #[cfg(feature = "trace")]
      depth: _,
    // — witness: the input's identity, fixed for its life, never restored.
    #[cfg(all(
      debug_assertions,
      any(feature = "std", feature = "alloc"),
      target_has_atomic = "ptr"
    ))]
      witness: _,
  } = input;

  let InputRef {
    // — borrows of the `Input` cells above; same classes.
    input: _,
    state: _,
    span: _,
    cache: _,
    pending: _,
    emitted_error_end: _,
    front_reported_end: _,
    poison_boundary: _,
    // — CONTROL-STACK FACT (borrowed): raised by `descend`, lowered by the `Descent` guard's
    //   drop, never restored. Same class as the `Input` field above it.
    recursion: _,
    // — MONOTONE SESSION FACT (borrowed): gated and charged one per produced item by
    //   `lex_within_boundary`, never lowered and never restored. Same class as the `Input` field
    //   above it.
    token_budget: _,
    // — MONOTONE SESSION FACT (borrowed): counted up by `raise_level`'s trip arm, never lowered
    //   and never restored. Same class as the `Input` field above it.
    resource_trips: _,
    // — MONOTONE SESSION FACT (borrowed): counted up by `latch_scanner_trip`, never lowered
    //   and never restored. Same class as the `Input` field above it.
    scanner_trips: _,
    // — WORLD FACT, as a read-only `Copy` snapshot: no mutator, and the handle's borrow of the
    //   input locks out the seal, so it is CONSTANT for this handle's life.
    finality: _,
    // — the lineage memos (borrowed) + the emitter borrow (ground truth: the emission log,
    //   rolled back by truncation to the saved mark) + the open session points (owned):
    //   censused in `session.rs`, which holds all three so its drop can settle an abandoned
    //   point's pin, lineage entry, and emitter mark together.
    session: _,
    // — instrumentation.
    #[cfg(feature = "trace")]
      depth: _,
    // — witness.
    #[cfg(all(
      debug_assertions,
      any(feature = "std", feature = "alloc"),
      target_has_atomic = "ptr"
    ))]
      witness: _,
    // — ZST.
    _marker: _,
  } = handle;
}

/// The lineage memos of one [`Input`](super::Input) — the class of cell backtracking owns (see
/// the [module docs](self) for the full taxonomy).
///
/// The fields are private: callers reach them only through the operations, so every invariant
/// that governs a cell lives on the one method that maintains it. In an allocator-less build
/// only [`cache_pushes`](Self::cache_pushes) exists; the live-checkpoint stack, the pin set, and
/// their counters all compile out with the backtracking machinery that needs them.
pub(crate) struct Lineage {
  /// Monotone count of tokens the cache has accepted over the input's life, bumped by every
  /// successful cache push (see [`record_cache_push`](Self::record_cache_push)). A
  /// [`Checkpoint`](super::Checkpoint) captures it at save time and
  /// [`restore`](super::InputRef::restore) uses the difference to drop the entries pushed on
  /// the abandoned continuation. It is correctness state in **every** build, so it is the one
  /// memo present without an allocator.
  cache_pushes: u64,
  /// Input-global savepoint sequence counter for [`StackedTransaction`](super::StackedTransaction),
  /// handed out monotonically by [`next_savepoint_seq`](Self::next_savepoint_seq) and never reset.
  /// It is monotone across every stacked transaction of this input, so a
  /// [`SavepointId`](super::SavepointId)'s `seq` is unique for the whole life of the input: an id
  /// that crosses transactions (nested or sequential) can never collide with a live savepoint's
  /// `seq` in another transaction's stack. There is no atomic and no process-wide state — the
  /// counter is per-input.
  #[cfg(any(feature = "std", feature = "alloc"))]
  savepoint_seq: u64,
  /// The live-checkpoint lineage stack: the ids of the checkpoints that have been saved and
  /// neither restored nor invalidated by restoring an older one, youngest last.
  /// [`open`](Self::open) pushes a fresh id, [`pop_through`](Self::pop_through) pops down through
  /// a restored id (invalidating it and every younger one), and a committed checkpoint is
  /// dropped by [`forget`](Self::forget). State surgery leaves it untouched — checkpoints survive
  /// state replacement, which is transactional.
  ///
  /// It is the single source of truth for lineage validity in **every** allocator build — no
  /// atomics, no interior mutability, just a stack — so [`StackedTransaction`](super::StackedTransaction)
  /// can reject a savepoint whose checkpoint a raw restore below it invalidated, on release and
  /// no-`target_has_atomic`-ptr targets alike. In debug + ptr builds the same stack also backs
  /// [`restore`](super::InputRef::restore)'s non-LIFO panic.
  #[cfg(any(feature = "std", feature = "alloc"))]
  live_ckpts: LineageStack,
  /// Monotone id source for [`live_ckpts`](Self::live_ckpts): each [`open`](Self::open) takes the
  /// current value and bumps it, so an id is never reused for the life of the input and a popped
  /// id can never be mistaken for a live one.
  #[cfg(any(feature = "std", feature = "alloc"))]
  next_ckp_id: u64,
  /// The pinned checkpoint ids: the begin-point checkpoint of every currently-live transaction
  /// guard, [`attempt`](super::InputRef::attempt)/[`try_attempt`](super::InputRef::try_attempt), and
  /// [session point](super::InputRef::begin_point).
  /// A guard/attempt logically borrows the timeline from its begin point forward, so a raw
  /// [`restore`](super::InputRef::restore) that would pop a pinned id off
  /// [`live_ckpts`](Self::live_ckpts) — tearing that begin point out from under a live guard —
  /// **panics at the restore** rather than silently invalidating it. Every guard/attempt
  /// constructor pins its held id on entry and every settle path unpins, so this holds exactly
  /// the live begin points and stays bounded across commit-heavy loops. Allocator-less builds
  /// maintain no pin set and fall back on the detect-at-use backstops.
  ///
  /// "Every settle path" includes one that is **not** a verb: an [`InputRef`](super::InputRef)
  /// dropped with session points still open releases their pins in its `Drop`. A guard cannot leak
  /// a pin (it borrows the handle, so it must settle before the handle can die), but a session point
  /// is a *value on* the handle while its pin lives out here on the longer-lived
  /// [`Input`](super::Input) — so the handle's death is a settle path, and unpinning there is what
  /// keeps "exactly the live begin points" true.
  #[cfg(any(feature = "std", feature = "alloc"))]
  pinned: LineageStack,
}

/// The three monotone counters below are **witnesses**, not bookkeeping, and a wrap would be
/// silent. `next_ckp_id`'s ids are what make `live_ckpts` sorted-ascending, and the
/// `contains` / `pop_through` fast paths are binary searches *licensed by* that sortedness — a
/// wrapped id violates it without any observable error, so a shipped optimization would start
/// answering wrong. `savepoint_seq`'s uniqueness is what makes a stale savepoint panic
/// deterministically rather than collide with a live one. So each increment is checked and
/// loud at the wrap point, in every profile, following the sink's own witness counter.
///
/// 2^64 increments is not a practical horizon. The point is not that the panic will fire; it
/// is that the invariant a search relies on cannot break in silence.
#[cfg(any(feature = "std", feature = "alloc"))]
const CKP_ID_EXHAUSTED: &str = "checkpoint id space exhausted: 2^64 checkpoints were opened on \
                               one input. Ids are never reused, and a wrap would break the \
                               ascending order the live-checkpoint search depends on.";

#[cfg(any(feature = "std", feature = "alloc"))]
const SAVEPOINT_SEQ_EXHAUSTED: &str = "savepoint sequence exhausted: 2^64 savepoints were taken on one input. A wrap would let a \
   stale savepoint id collide with a live one instead of panicking.";

const CACHE_PUSHES_EXHAUSTED: &str = "cache-push counter exhausted: 2^64 cache pushes on one input. The count is a monotone \
   witness; a wrap would make it read as a smaller history than actually occurred.";

impl Lineage {
  /// A fresh set of memos for a new input: an unadvanced cache-push counter and — in allocator
  /// builds — an empty live-checkpoint stack, an empty pin set, and zeroed counters.
  #[inline(always)]
  pub(crate) fn new() -> Self {
    Self {
      cache_pushes: 0,
      #[cfg(any(feature = "std", feature = "alloc"))]
      savepoint_seq: 0,
      #[cfg(any(feature = "std", feature = "alloc"))]
      live_ckpts: LineageStack::new(),
      #[cfg(any(feature = "std", feature = "alloc"))]
      next_ckp_id: 0,
      #[cfg(any(feature = "std", feature = "alloc"))]
      pinned: LineageStack::new(),
    }
  }

  /// The current cache-push count, snapshotted into a [`Checkpoint`](super::Checkpoint) at save
  /// time so a later restore can drop exactly the entries pushed since.
  #[inline(always)]
  pub(crate) fn cache_pushes(&self) -> u64 {
    self.cache_pushes
  }

  /// Records one accepted cache push. Every cache push flows through this on success — the peek
  /// fill and the `try_expect` put-backs — so the count tracks exactly the tokens the cache
  /// accepted: a full cache that hands the token back leaves the count unchanged, and a blackhole
  /// cache — which accepts no push — keeps it at 0.
  /// Seeds the three witness counters near `u64::MAX` so their exhaustion panics are
  /// reachable from a test. Without it the `checked_add`s would be a structural argument with
  /// no cell behind them — an assert nobody has shown can fire.
  #[cfg(test)]
  pub(crate) fn seed_counters_at_max_for_tests(&mut self) {
    // `cache_pushes` is correctness state in every build; the id sources exist only where an
    // allocator does, which is the same gate the fields themselves carry.
    self.cache_pushes = u64::MAX;
    #[cfg(any(feature = "std", feature = "alloc"))]
    {
      self.next_ckp_id = u64::MAX;
      self.savepoint_seq = u64::MAX;
    }
  }

  #[inline(always)]
  pub(crate) fn record_cache_push(&mut self) {
    self.cache_pushes = self
      .cache_pushes
      .checked_add(1)
      .expect(CACHE_PUSHES_EXHAUSTED);
  }

  /// Rewinds the cache-push counter to a checkpoint's saved value on restore.
  ///
  /// The count is per-lineage state, exactly like the dedup watermark and the poison boundary:
  /// under the last-in, first-out contract a restore returns to the saved lineage exactly, so the
  /// counter copies back verbatim. It is re-anchored to the push history of the lineage now live
  /// (the restore's tail-drop has already consumed the pre-rewind value), so future
  /// `cache_pushes − saved` deltas stay exact. State surgery deliberately leaves the counter
  /// untouched — a re-key clears the cache but not the count, so a checkpoint saved before the
  /// surgery still restores an exact delta.
  #[inline(always)]
  pub(crate) fn restore_cache_pushes(&mut self, saved: u64) {
    self.cache_pushes = saved;
  }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl Lineage {
  /// Opens a checkpoint entry: takes a fresh, never-reused id, records it on the live-checkpoint
  /// lineage stack (youngest last), and returns it to be stamped into the
  /// [`Checkpoint`](super::Checkpoint). [`restore`](super::InputRef::restore) later pops the stack
  /// down through this id, and a [`StackedTransaction`](super::StackedTransaction) checks the id
  /// is still present before honoring a savepoint — the check that makes stale savepoints panic on
  /// release and no-ptr targets. Opening never invalidates another checkpoint; only restoring does.
  #[inline(always)]
  pub(crate) fn open(&mut self) -> u64 {
    let id = self.next_ckp_id;
    self.next_ckp_id = id.checked_add(1).expect(CKP_ID_EXHAUSTED);
    self.live_ckpts.push(id);
    id
  }

  /// CAPTURE_WINDOW — reserves the one slot [`open`](Self::open) is about to push into, so that
  /// push cannot allocate and therefore cannot unwind.
  ///
  /// [`open`](Self::open) *is* half of a capture (the other half is the emitter mark): it records
  /// a live entry that only a later settle — a restore, or [`forget`](Self::forget) through
  /// `forget_kept_checkpoint` — can remove. A [`Checkpoint`](super::Checkpoint) has no `Drop` (it
  /// cannot reach the input or the emitter to settle itself), so an unwind out of the push would
  /// leave the entry with no owner at all. Calling this first moves the only fallible step to a
  /// moment when nothing is captured yet: either it panics with nothing to strand, or the push is
  /// infallible. Amortization is unchanged: a reserve that finds room is a compare, and one that
  /// does not performs exactly the geometric growth the `push` would have performed. The guarantee
  /// holds for `Vec` and for `SmallVec` alike — the latter's `reserve` compares against the
  /// *inline* capacity when it has not spilled, so a stack sitting exactly at that capacity spills
  /// to the heap here rather than in the push.
  #[inline(always)]
  pub(crate) fn reserve_open(&mut self) {
    self.live_ckpts.reserve(1);
  }

  /// Returns whether `id` is still live on the lineage stack. Backs both the
  /// [`StackedTransaction`](super::StackedTransaction) savepoint-staleness check (every allocator
  /// build) and, in debug + ptr builds, [`restore`](super::InputRef::restore)'s non-LIFO panic.
  ///
  /// `O(1)` on the LIFO path — which is every guard settle — and `O(log d)` otherwise: ids are
  /// minted strictly increasing and every writer preserves order, so the stack is sorted and a
  /// binary search is licensed here, at the definition site where that invariant lives.
  #[inline(always)]
  pub(crate) fn contains(&self, id: u64) -> bool {
    if self.live_ckpts.last() == Some(&id) {
      scan_tick();
      return true;
    }
    self
      .live_ckpts
      .binary_search_by(|probe| {
        scan_tick();
        probe.cmp(&id)
      })
      .is_ok()
  }

  /// Pops the lineage stack down through `id` inclusive, invalidating it and every checkpoint
  /// saved after it. A no-op if `id` is already gone — a raw restore to a checkpoint an earlier
  /// restore already invalidated (release's unspecified-but-bounded posture; debug + ptr asserts
  /// presence in [`restore`](super::InputRef::restore) first).
  ///
  /// `O(1)` when `id` is the stack top — the LIFO common case, and every drop-rollback — and
  /// `O(log d)` otherwise, over the same sorted-stack invariant [`contains`](Self::contains)
  /// uses. An absent id still pops nothing.
  #[inline(always)]
  pub(crate) fn pop_through(&mut self, id: u64) {
    if self.live_ckpts.last() == Some(&id) {
      scan_tick();
      self.live_ckpts.truncate(self.live_ckpts.len() - 1);
      return;
    }
    if let Ok(pos) = self.live_ckpts.binary_search_by(|probe| {
      scan_tick();
      probe.cmp(&id)
    }) {
      self.live_ckpts.truncate(pos);
    }
  }

  /// Drops `id` from the live-checkpoint lineage stack because its checkpoint was **kept**
  /// (committed) rather than restored.
  ///
  /// A restored checkpoint is popped off the stack by [`pop_through`](Self::pop_through); a
  /// *committed* one never reaches a restore, so without this its id would linger and grow the
  /// stack across commit-heavy loops. Removing it keeps the stack exact and bounded. `O(1)` when
  /// `id` is the stack top (the common case for a committed checkpoint); a linear removal
  /// otherwise (e.g. a raw checkpoint saved above it was dropped without restoring) — the
  /// removal itself has to shift, so the search's order buys nothing here. Removing a non-top id
  /// keeps the rest of the stack in order, so an older restore still pops cleanly through it.
  /// Committing an already-invalidated id is a harmless no-op: it is simply absent.
  #[inline(always)]
  pub(crate) fn forget(&mut self, id: u64) {
    if self.live_ckpts.last() == Some(&id) {
      self.live_ckpts.pop();
    } else if let Some(pos) = self.live_ckpts.iter().position(|&x| x == id) {
      self.live_ckpts.remove(pos);
    }
  }

  /// Pins `id` — the begin-point checkpoint of a transaction guard or an
  /// [`attempt`](super::InputRef::attempt) — so a raw [`restore`](super::InputRef::restore) that
  /// would pop it off the lineage (a restore reaching *below* the guard's begin point) panics at
  /// the restore instead of silently tearing out the guard's foundation. Every guard constructor
  /// and attempt pins on entry; the matching [`unpin`](Self::unpin) runs on every settle path.
  ///
  /// Nested guards are borrowck-serialized: an inner guard mutably borrows its parent for its
  /// whole life, so the inner settles (and unpins) before the outer is usable again. An outer
  /// rollback therefore never finds a live inner pin sitting above its base — only its own
  /// (just-unpinned) begin point and any LIFO-clean raw checkpoints, none pinned.
  #[inline(always)]
  pub(crate) fn pin(&mut self, id: u64) {
    self.pinned.push(id);
  }

  /// CAPTURE_WINDOW — reserves the one slot [`pin`](Self::pin) is about to push into, so that push
  /// cannot allocate and therefore cannot unwind.
  ///
  /// Every pinning caller pins its begin point *after* taking the capture and *before* the capture
  /// has an owner (a [`Transaction`](super::Transaction), a
  /// [`StackedTransaction`](super::StackedTransaction), or the session-point stack), and this stack
  /// is a **second** container, distinct from the one the capture will land in: reserving the
  /// destination alone leaves the pin push as a live allocation site inside that window. An unwind
  /// there drops the capture raw — no rollback, no commit, no `forget_kept_checkpoint`, no emitter
  /// release — and, since the owner was never constructed, nothing else can settle it either. So
  /// the caller reserves here first, with nothing yet captured to strand. Same guarantee for `Vec`
  /// and for `SmallVec`: the latter's `reserve` compares against the *inline* capacity too, so a
  /// pin set sitting exactly at its inline capacity spills to the heap here rather than in the
  /// push.
  #[inline(always)]
  pub(crate) fn reserve_pin(&mut self) {
    self.pinned.reserve(1);
  }

  /// Removes `id` from the pin set when its guard, attempt, or session point settles. Mirrors
  /// [`forget`](Self::forget): `O(1)` when `id` is the top (the LIFO common case — guards and
  /// attempts are borrowck-serialized, so the settling one is innermost), a linear removal
  /// otherwise. Called on **every** settle path (commit, explicit rollback, `Drop`, both closure
  /// arms of the attempts, both session-point verbs, and the [`InputRef`](super::InputRef) `Drop`
  /// that releases session points abandoned with the handle), so the pin set stays bounded and holds
  /// exactly the begin points of the currently-live guards, attempts, and session points.
  #[inline(always)]
  pub(crate) fn unpin(&mut self, id: u64) {
    if self.pinned.last() == Some(&id) {
      self.pinned.pop();
    } else if let Some(pos) = self.pinned.iter().position(|&x| x == id) {
      self.pinned.remove(pos);
    }
  }

  /// Panics if restoring to `target_id` would pop a **pinned** checkpoint off the live lineage —
  /// i.e. if it would tear the begin point out from under a still-live transaction guard or
  /// attempt. This is the detect-at-cause check: a raw restore below a live guard/attempt begin
  /// point is refused right where it is requested, in every allocator build.
  ///
  /// A [`restore`](super::InputRef::restore) pops the target and every younger checkpoint. A
  /// guard's own settle unpins its held id **before** routing through the restore, so a guard
  /// rolling back to its own base never trips its own pin; only a restore reaching *below* a live
  /// begin point finds that begin point still pinned above the target. A stacked-transaction
  /// savepoint `rollback_to` restores a checkpoint *above* the base, so it can never reach the
  /// pinned base. A target that is not live pops nothing, so it cannot invalidate anything pinned.
  ///
  /// The search is ordered rather than nested-linear: both stacks are sorted ascending, so the
  /// target is found by binary search, and the suffix `live_ckpts[pos..]` is exactly "every live
  /// id `>= target_id`" — so only the pins at or above the target can be in it, and each is
  /// looked up in that suffix by binary search. `O(|pins >= target| · log d)` with `|pins|` tiny
  /// (live guards, attempts and session points).
  ///
  /// Deliberately **not** the `O(1)` `pinned.last() >= target_id` watermark: that form is exact
  /// only under "pinned is a subset of live", which is an invariant statement rather than a
  /// checked fact, and a false positive here is a release-active panic on a legal restore.
  #[inline(always)]
  pub(crate) fn assert_restore_preserves_pins(&self, target_id: u64) {
    let Ok(pos) = self.live_ckpts.binary_search(&target_id) else {
      // The target is already gone: the restore will pop nothing, so nothing pinned can be
      // invalidated (release's unspecified-but-bounded posture for an already-dead target).
      return;
    };
    // The restore truncates `live_ckpts` at `pos`, popping the target and every younger
    // checkpoint. If any of those is pinned, the restore would invalidate a live guard/attempt.
    let doomed = &self.live_ckpts[pos..];
    if self
      .pinned
      .iter()
      .rev()
      .take_while(|p| **p >= target_id)
      .any(|p| doomed.binary_search(p).is_ok())
    {
      panic!(
        "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
      );
    }
  }

  /// Hands out the next input-global savepoint sequence number, bumping the counter. Because the
  /// counter lives on the input (not on any one transaction) and is never reset, a
  /// [`SavepointId`](super::SavepointId)'s `seq` is unique for the whole life of the input: an id
  /// that crosses transactions can never collide with a live savepoint's `seq` in another
  /// transaction's stack, so the membership scan in `rollback_to`/`release` panics deterministically
  /// wherever the lifetime brand does not already reject the id.
  #[inline(always)]
  pub(crate) fn next_savepoint_seq(&mut self) -> u64 {
    let seq = self.savepoint_seq;
    self.savepoint_seq = seq.checked_add(1).expect(SAVEPOINT_SEQ_EXHAUSTED);
    seq
  }

  /// The number of live checkpoints — test-only observability for the no-growth guarantee that
  /// committing (and a success-path recover) gives the lineage stack.
  #[cfg(all(test, feature = "logos_0_16", feature = "std"))]
  pub(crate) fn live_len(&self) -> usize {
    self.live_ckpts.len()
  }

  /// The number of pinned checkpoints — observability for the law this set states about itself:
  /// it holds **exactly** the begin points of the currently-live guards, attempts, and session
  /// points, and is therefore empty whenever none is live. It backs the drop-path release the
  /// [`InputRef`](super::InputRef)'s `Drop` performs for abandoned session points (a pin whose
  /// point nobody can settle would otherwise sit here for the life of the input).
  ///
  /// Reachable from the owning [`Input`](super::Input), not just from a handle, because that is
  /// exactly where the question is asked: *after* the handle that opened the points is gone.
  /// Gated to its callers — the `logos` + `std` session tests and the `fuzz` harness's abandon
  /// oracle — so it is never dead code under `cargo hack --each-feature --tests`.
  #[cfg(any(
    all(test, feature = "logos_0_16", feature = "std"),
    all(feature = "fuzz", feature = "std")
  ))]
  pub(crate) fn pinned_len(&self) -> usize {
    self.pinned.len()
  }
}

/// The settle path's **scan odometer**, test-only.
///
/// [`contains`](Lineage::contains) and [`pop_through`](Lineage::pop_through) run on every guard
/// settle, so their cost as a function of nesting depth is the property worth pinning — and it is
/// invisible to a value-level assertion. Each element a search inspects ticks the odometer once,
/// which lets a cell assert the *law* (linear in depth) rather than a constant. Outside a
/// `std` test build this is an empty `#[inline(always)]` body and the odometer does not exist.
#[cfg(all(test, feature = "logos_0_16", feature = "std"))]
#[inline(always)]
fn scan_tick() {
  scan_probe::tick();
}

/// The no-op form: every build that is not a `std` test, **and** has a `Lineage` to tick for.
///
/// The second half of that gate is not decoration. `Lineage` — and with it `contains` and
/// `pop_through`, the only callers this has — is `#[cfg(any(feature = "std", feature = "alloc"))]`,
/// so at an allocator-free feature point the callers do not exist and an unconditional no-op is
/// `dead_code`, which is `-D warnings` under this crate's lint posture. It must be gated to
/// exactly the set its callers are compiled for; the odometer form above needs no such clause
/// because `feature = "std"` already implies it.
#[cfg(all(
  any(feature = "std", feature = "alloc"),
  not(all(test, feature = "logos_0_16", feature = "std"))
))]
#[inline(always)]
fn scan_tick() {}

/// The odometer itself — see [`scan_tick`].
///
/// Gated to the exact feature set of the cells that read it — the `transaction` suite, which is
/// gated `all(test, feature = "logos_0_16", feature = "std")`; the attribute directly below this
/// doc comment is that predicate, spelled out. Gated any wider (it was `all(test, std)`) its
/// `reset`/`scanned` are `dead_code` at a point like `--no-default-features --features
/// conformance`, where `conformance` pulls in `std` but no logos version. The per-version logos
/// CI cells this was originally measured against (since retired along with logos 0.14/0.15)
/// reached it the other way, through `--no-default-features --features std,logos_0_14` — no job
/// compiled a test target in either configuration before, which is why this went unseen.
#[cfg(all(test, feature = "logos_0_16", feature = "std"))]
pub(crate) mod scan_probe {
  use core::cell::Cell;

  thread_local! {
    static SCANNED: Cell<usize> = const { Cell::new(0) };
  }

  /// Zeroes the odometer and opens a fresh reading window.
  #[cfg(feature = "logos_0_16")]
  pub(crate) fn reset() {
    SCANNED.with(|c| c.set(0));
  }

  /// Elements inspected since the last [`reset`].
  #[cfg(feature = "logos_0_16")]
  pub(crate) fn scanned() -> usize {
    SCANNED.with(Cell::get)
  }

  #[inline(always)]
  pub(crate) fn tick() {
    SCANNED.with(|c| c.set(c.get() + 1));
  }
}

/// CELL_CENSUS — the falsifiable half of the taxonomy's **action** column.
///
/// The exhaustive destructure in [`census`] guards the table's rows; nothing guarded the column
/// that says what a restore *does*, because it is prose. 0.8.0 deleted `Cache::rewind` and that
/// column kept naming it, with every gate green, until a reader traced the mechanism by hand.
///
/// This is the cheap half of the fix: every `Type::method` path the action column names must
/// still exist as a function in the crate. It cannot check that the English is *true* — that
/// stays a human review, and the preamble says so — but it does catch the specific failure that
/// occurred, which is a named mechanism outliving its deletion.
#[cfg(all(test, any(feature = "std", feature = "alloc")))]
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn taxonomy_census_action_column_names_only_live_mechanisms() {
  const LINEAGE: &str = include_str!("lineage.rs");
  const CACHE: &str = include_str!("../cache/mod.rs");
  const INPUT_REF: &str = include_str!("input_ref/mod.rs");
  const SESSION: &str = include_str!("input_ref/session.rs");

  let defined = std::format!("{LINEAGE}{CACHE}{INPUT_REF}{SESSION}");
  let mut checked = 0usize;

  for line in LINEAGE.lines() {
    let line = line.trim_start_matches("//!").trim();
    // Table rows only, and only the last column — the one that says what restore does.
    if !line.starts_with('|') || line.starts_with("|---") || line.starts_with("| Cell") {
      continue;
    }
    let Some(action) = line.trim_end_matches('|').rsplit('|').next() else {
      continue;
    };
    // `Type::method` paths, wherever they appear in that column.
    let mut rest = action;
    while let Some(at) = rest.find("::") {
      let before = &rest[..at];
      let after = &rest[at + 2..];
      let method: std::string::String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
      let is_type = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
      if is_type && !method.is_empty() && method.starts_with(|c: char| c.is_ascii_lowercase()) {
        checked += 1;
        // BOTH signature openers, as `census_tests::method_body` already matches: a mechanism that
        // grows a generic parameter of its own becomes `fn name<..>(`, and matching only
        // `fn name(` reports a live mechanism as deleted. A census that fails on the false side is
        // still a census the next change has to route around.
        // Written as two statements rather than one `||` chain because a continuation line
        // beginning with `||` is a line beginning with `|`, and the row filter above reads it as a
        // table row of this very table.
        let mut lives = defined.contains(&std::format!("fn {method}("));
        lives = lives || defined.contains(&std::format!("fn {method}<"));
        assert!(
          lives,
          "CELL_CENSUS drift: the restore taxonomy's action column names `::{method}`, which no \
           longer exists. A deleted or renamed restore mechanism must be removed from that \
           column in the same commit — the exhaustive destructure guards the table's ROWS, not \
           this column (grep CELL_CENSUS)."
        );
      }
      rest = &after[method.len()..];
    }
  }

  assert!(
    checked > 0,
    "CELL_CENSUS drift: the action column names no `Type::method` path at all, so this check \
     selected nothing. Either the table was rewritten or this parser stopped matching it — a \
     census that checks nothing is worse than none (grep CELL_CENSUS)."
  );
}

#[cfg(test)]
mod exhaustion_tests {
  use super::Lineage;

  /// The three witness counters are load-bearing for a **shipped optimization**, not just
  /// bookkeeping: the `contains` / `pop_through` fast paths are binary searches licensed by
  /// "ids monotone, `live_ckpts` sorted ascending", and a wrapped id violates that in silence.
  /// So each increment panics at the wrap point rather than rolling over.
  ///
  /// Falsifying output: no panic — the counter wrapped, and the search's precondition is now
  /// false with nothing to say so.
  #[test]
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[should_panic(expected = "checkpoint id space exhausted")]
  fn checkpoint_id_exhaustion_panics() {
    let mut lineage = Lineage::new();
    lineage.seed_counters_at_max_for_tests();
    let _ = lineage.open();
  }

  #[test]
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[should_panic(expected = "savepoint sequence exhausted")]
  fn savepoint_seq_exhaustion_panics() {
    let mut lineage = Lineage::new();
    lineage.seed_counters_at_max_for_tests();
    let _ = lineage.next_savepoint_seq();
  }

  #[test]
  #[should_panic(expected = "cache-push counter exhausted")]
  fn cache_push_exhaustion_panics() {
    let mut lineage = Lineage::new();
    lineage.seed_counters_at_max_for_tests();
    lineage.record_cache_push();
  }

  /// The keep-green direction: none of the three fires on an ordinary history.
  #[test]
  fn ordinary_use_never_trips_a_witness() {
    let mut lineage = Lineage::new();
    for _ in 0..1_000 {
      lineage.record_cache_push();
      #[cfg(any(feature = "std", feature = "alloc"))]
      let _ = lineage.next_savepoint_seq();
    }
    assert_eq!(lineage.cache_pushes(), 1_000);
  }
}
