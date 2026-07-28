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
//! | E1 | [`Sink::events`] | **ground truth** (a second emission log) | append + suffix-truncate to the mark — the same two verbs as `Verbose`'s log (plus the one censused prefix-preserving splice of the hole wrap, entirely above every live mark) |
//! | E2 | [`Sink::journal`] | **undo journal** (the Verbose-parallel-maps discipline lifted to events) | rewind pops entries written above the mark, reverse order, restoring each overwritten `forward_parent`; never grows on rewind |
//! | E3 | [`Sink::ledger`] | **monotone era source + truncation witness** | rewind APPENDS to it (a rewind *is* a truncation) and never removes; rewinding it would false-accept a stale mark |
//! | E4 | [`Sink::rows`] | **release stack + per-checkpoint depth ledger + inner reading** | push at `checkpoint()` (freezing the depth and the inner emitter's own checkpoint reading), pop at `release()` (kept) and `rewind()` (spent, the popped row's inner reading is the inner's rewind target); depth entries are frozen facts about prefixes, never live counters |
//! | — | [`Sink::floor`] | derived memo (the newest released row) | reset to the surviving top row when a rewind drops below it |
//! | — | [`Sink::base_inner`] | derived memo (the inner's construction-time reading) | primed at the first advancing touch (provably the construction reading), never restored (the exact no-row target at the origin only) |
//! | — | `inner`, `source`, `profile`, `trivia` | configuration / the wrapped emitter | never touched by rewind (the inner rewinds through its own contract) |
//! | — | `witness` | sink identity (validated at every mark spend, every build) | never restored |
//!
//! Open-node **depth is derived, never cached**: there is no live depth counter anywhere —
//! [`checkpoint`](Emitter::checkpoint) snapshots a frozen per-row depth, and every query
//! recounts the suffix above the nearest frozen fact. A cached counter would need its own
//! restore rule; a derived one is restored by truncation for free.

use core::{cell::RefCell, marker::PhantomData, num::NonZeroU32};

use std::vec::Vec;

use crate::{
  Lexer,
  emitter::{
    CstEmitter, Emitter, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, PrattEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, ValueKeyedEmitter,
  },
  error::{
    UnexpectedEoLhs, UnexpectedEoRhs,
    syntax::{FullContainer, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingTokenOf, UnexpectedTokenOf},
  },
  input::Cursor,
  span::{Span, Spanned},
  token::Token,
  utils::CowStr,
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
  /// The derived open-node depth at capture time — a frozen fact about the `mark`-length
  /// prefix, not a live counter.
  depth: i64,
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

impl MarkRow {
  /// The empty-buffer baseline: depth 0 at length 0, inner at its base reading 0.
  ///
  /// Floor sentinel only — the floor's `inner` is never a rewind target (the no-row **origin**
  /// fallback is `base_inner`), so the 0 here stays inert even for a reused inner whose
  /// construction reading is nonzero.
  const ZERO: Self = Self {
    mark: 0,
    depth: 0,
    inner: 0,
  };
}

/// One undo-journal entry: an in-place `forward_parent` write performed by
/// [`cst_start_at`](CstEmitter::cst_start_at) on its target tombstone, recorded so a rewind
/// can reverse it. In-place event mutation is otherwise banned by law; this one acceleration
/// field is the single exception, and it is legal *only because* every write is journaled.
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
/// ([`finish`](Self::finish) / [`finish_partial`](Self::finish_partial)) consumes the sink
/// and returns the inner emitter with the tree.
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
/// inner, and a truncating rewind to a mid-log mark no row captured is witnessed in debug and
/// leaves the inner untouched in release (the sink never fabricates a reading).
///
/// A table-keyed emitter — one that allocates per-`checkpoint` bookkeeping behind interior
/// mutability and reclaims it per-`release` — is **rejected as the inner at compile time**: the
/// sink re-spends its base reading across no-row **origin** rewinds, drops row readings above a
/// rewound target by value, and settles rows out of stack order under mixed raw use, all of which
/// presuppose readings — and equal-mark rows are interchangeable only while the reading is a
/// function of emission state. The requirement is therefore the [`ValueKeyedEmitter`] bound on
/// this type's [`Emitter`] impl and on [`new`](Self::new), not a convention; see that trait for
/// the exact promise and the roster of implementors. Such an emitter belongs at the input layer's
/// direct seam, where the settle discipline is 1:1 — the sink's own mark stack is exactly that
/// shape, and the input layer does release it. A sink is not a value-keyed emitter either, so a
/// sink cannot wrap a sink.
///
/// # Construction
///
/// The source is bound **here**, and `finish` has no source parameter, so a materialization
/// can no longer be handed a buffer different from the one construction named. What that
/// removes is the *late* half of the wrong-source class. It does not establish that the
/// buffer named here is the one the parser will read: `'inp` proves both borrows outlive the
/// sink, not that they are the same bytes, and nothing here compares them. A sink built over
/// one buffer and driven while parsing another of the same length still materializes a tree
/// whose text is this buffer and whose structure is the parser's — pinned, as an open hole,
/// by `sink_bound_to_a_foreign_source_is_not_yet_detected`. Closing it needs an identity the
/// sink can check, and no [`Emitter`] hook hands the sink the parser's buffer.
///
/// [`new`](Self::new) takes the source buffer the parse runs over, the wrapped emitter, and
/// the dialect's [`CstProfile`] — the token mapper (`fn(&L::Token) -> u16` into the dialect's
/// unified kind space — no kind bound leaks into core), the [`KindValidator`] every recorded
/// kind is checked against, the `error_kind` used to wrap recovery holes, and the `gap_kind`
/// used to tile uncovered source bytes at materialization (what makes `tree.text() == source`
/// structural for every input, lexer errors included). Binding the source here rather than at
/// [`finish`](Self::finish) removes the one way a caller could hand materialization a
/// different buffer than the spans were measured against. Construction is **compile-time
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
  /// E2 — the undo journal for the `forward_parent` acceleration writes.
  journal: Vec<JournalEntry>,
  /// E4 — the mark stack: one row per live checkpoint capture, holding the frozen depth and
  /// the inner emitter's own checkpoint reading (the inner's rewind target).
  /// Interior mutability because [`Emitter::checkpoint`] is `&self` by contract; every
  /// borrow is method-local and non-reentrant, and the `&mut` paths use `get_mut` (no
  /// runtime flag traffic).
  rows: RefCell<Vec<MarkRow>>,
  /// The newest *released* row: a frozen `(mark, depth)` fact that keeps depth derivation
  /// O(events-since-last-settle) instead of O(buffer) across commit-heavy loops.
  floor: MarkRow,
  /// E3 — the monotone era source and truncation witness backing mark validation.
  ledger: TruncationLedger,
  /// The inner emitter's **construction-time** reading — the no-row **origin** rewind's exact
  /// inner target (an empty event log provably pairs with the construction reading; mid-log
  /// no-row marks have no exact reading and never touch the inner). Primed at the first
  /// inner-advancing touch (a forwarded diagnostic or a settled token; the rewind fallback
  /// reads it the same way), which provably equals the reading at [`new`](Self::new): the sink
  /// exposes no `&mut` path to the inner, so the inner cannot advance before the sink's own
  /// first advancing call — and every advancing surface primes this field before forwarding.
  /// (The capture is lazy only to keep the constructor free of emitter bounds: `Emitter` is
  /// `Lang`-parameterized and the built-in emitters implement it for exactly one `Lang`.)
  base_inner: Option<u64>,
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
      .field("live_marks", &self.rows.borrow().len())
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
    // — E2, undo journal: rewind reverse-replays and truncates by `at_len`.
    journal: _,
    // — E4, release stack + depth ledger: push at checkpoint, pop at release/rewind.
    rows: _,
    // — derived memo: the newest released row; reset when a rewind drops below it.
    floor: _,
    // — E3, monotone era source + truncation witness: NEVER rewound.
    ledger: _,
    // — derived memo: the inner's construction-time reading, primed at first advancing touch,
    // never restored.
    base_inner: _,
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
  /// - `source` is the buffer the parse runs over; every token's text is sliced out of it at
  ///   materialization, so [`finish`](Self::finish) takes no source argument of its own;
  /// - `profile` is the dialect's kind space
  ///   ([`CstProfile`](super::CstProfile)): the token mapper into the unified u16 kind space,
  ///   the [`KindValidator`](super::KindValidator) every recorded kind is checked against, the
  ///   `error_kind` wrapped around a recovery hole's skipped tokens, and the `gap_kind` tiled
  ///   over source bytes no committed token covers (what makes `tree.text() == source`
  ///   structural for every input). The [`TOMBSTONE`] value is reserved throughout: no
  ///   validator admits it, emission debug-asserts against it, and materialization rejects it.
  ///
  /// Construction is restricted at **compile time** to trivia-surfacing lexers
  /// ([`Lexer::SURFACES_TRIVIA`] `== true`): a syntactic lexer that skips trivia cannot take
  /// this lossless door, because a skipped-whitespace gap is indistinguishable from a
  /// dropped committed token. The wall is an inline-`const` assertion, so it fires at
  /// build/test/doc time (a post-monomorphization `error[E0080]` at the offending call
  /// site) — **not** under `cargo check`, which never monomorphizes the call.
  ///
  /// # Compile-time wall: trivia-surfacing lexers only
  ///
  /// A lexer that surfaces trivia (declares [`Token::SURFACES_TRIVIA`] = `true`)
  /// constructs a sink:
  ///
  /// ```rust
  /// use tokora::{
  ///   Lexer, SimpleSpan, Token,
  ///   cst::{CstProfile, KindValidator, Sink},
  ///   emitter::Verbose,
  /// };
  ///
  /// #[derive(Debug, Clone, Copy)]
  /// struct STok;
  /// impl Token<'_> for STok {
  ///   type Kind = u8;
  ///   type Error = ();
  ///   const SURFACES_TRIVIA: bool = true; // ← the declaration under test
  ///   fn kind(&self) -> u8 { 0 }
  ///   fn is_trivia(&self) -> bool { false }
  /// }
  /// # struct Lossless<'a> { src: &'a str, state: () }
  /// # impl<'inp> Lexer<'inp> for Lossless<'inp> {
  /// #   type State = (); type Source = str; type Token = STok;
  /// #   type Span = SimpleSpan; type Offset = usize;
  /// #   fn new(src: &'inp str) -> Self { Self { src, state: () } }
  /// #   fn with_state(src: &'inp str, state: ()) -> Self { Self { src, state } }
  /// #   fn check(&self) -> Result<(), ()> { Ok(()) }
  /// #   fn state(&self) -> &() { &self.state }
  /// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
  /// #   fn into_state(self) -> () { self.state }
  /// #   fn source(&self) -> &'inp str { self.src }
  /// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(0, 0) }
  /// #   fn slice(&self) -> &'inp str { "" }
  /// #   fn lex(&mut self) -> Option<Result<STok, ()>> { None }
  /// #   fn bump(&mut self, _: &usize) {}
  /// # }
  /// let profile = CstProfile::new(|_| 0, KindValidator::new(|k| k <= 91), 90, 91);
  /// let _sink: Sink<'_, Lossless<'_>, Verbose<()>> = Sink::new("", Verbose::new(), profile);
  /// ```
  ///
  /// The same lexer without the declaration (the default, i.e. a syntactic lexer that
  /// skips trivia) is refused at compile time — the only difference from the example
  /// above is the missing `SURFACES_TRIVIA` line:
  ///
  /// ```compile_fail
  /// use tokora::{
  ///   Lexer, SimpleSpan, Token,
  ///   cst::{CstProfile, KindValidator, Sink},
  ///   emitter::Verbose,
  /// };
  ///
  /// #[derive(Debug, Clone, Copy)]
  /// struct STok;
  /// impl Token<'_> for STok {
  ///   type Kind = u8;
  ///   type Error = ();
  ///   // no SURFACES_TRIVIA: defaults to false (a skipping, syntactic grammar)
  ///   fn kind(&self) -> u8 { 0 }
  ///   fn is_trivia(&self) -> bool { false }
  /// }
  /// # struct Syntactic<'a> { src: &'a str, state: () }
  /// # impl<'inp> Lexer<'inp> for Syntactic<'inp> {
  /// #   type State = (); type Source = str; type Token = STok;
  /// #   type Span = SimpleSpan; type Offset = usize;
  /// #   fn new(src: &'inp str) -> Self { Self { src, state: () } }
  /// #   fn with_state(src: &'inp str, state: ()) -> Self { Self { src, state } }
  /// #   fn check(&self) -> Result<(), ()> { Ok(()) }
  /// #   fn state(&self) -> &() { &self.state }
  /// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
  /// #   fn into_state(self) -> () { self.state }
  /// #   fn source(&self) -> &'inp str { self.src }
  /// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(0, 0) }
  /// #   fn slice(&self) -> &'inp str { "" }
  /// #   fn lex(&mut self) -> Option<Result<STok, ()>> { None }
  /// #   fn bump(&mut self, _: &usize) {}
  /// # }
  /// let profile = CstProfile::new(|_| 0, KindValidator::new(|k| k <= 91), 90, 91);
  /// let _sink: Sink<'_, Syntactic<'_>, Verbose<()>> = Sink::new("", Verbose::new(), profile);
  /// ```
  ///
  /// # Compile-time wall: value-keyed inners only
  ///
  /// The *Inner-emitter contract* above is in the type system: the inner must be a
  /// [`ValueKeyedEmitter`]. A table-keyed emitter — one that allocates per-`checkpoint`
  /// bookkeeping behind interior mutability and reclaims it per-`release` — is refused here
  /// rather than accepted and then silently stranded (the sink's `release` forwards nothing, so
  /// such an inner would leak one row per committed capture):
  ///
  /// ```compile_fail
  /// use core::cell::RefCell;
  /// use tokora::{
  ///   Lexer, SimpleSpan, Token,
  ///   cst::{CstProfile, KindValidator, Sink},
  ///   emitter::Emitter,
  ///   input::Cursor,
  ///   span::Spanned,
  /// };
  ///
  /// /// A table-keyed emitter: `checkpoint` allocates a row and hands back its index.
  /// #[derive(Default)]
  /// struct TableKeyed {
  ///   rows: RefCell<Vec<u64>>,
  /// }
  ///
  /// impl<'a, L, Lang: ?Sized> Emitter<'a, L, Lang> for TableKeyed {
  ///   type Error = ();
  ///   fn emit_lexer_error(
  ///     &mut self,
  ///     _e: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ///   ) -> Result<(), ()> where L: Lexer<'a> { Ok(()) }
  ///   fn emit_unexpected_token(
  ///     &mut self,
  ///     _e: tokora::error::token::UnexpectedTokenOf<'a, L, Lang>,
  ///   ) -> Result<(), ()> where L: Lexer<'a> { Ok(()) }
  ///   fn emit_error(&mut self, _e: Spanned<(), L::Span>) -> Result<(), ()>
  ///   where L: Lexer<'a> { Ok(()) }
  ///   fn checkpoint(&self) -> u64 {
  ///     let mut rows = self.rows.borrow_mut();
  ///     rows.push(0);
  ///     rows.len() as u64 - 1 // a KEY, not a reading of the emission state
  ///   }
  ///   fn rewind(&mut self, _c: &Cursor<'a, '_, L>, _m: u64) where L: Lexer<'a> {}
  /// }
  ///
  /// # #[derive(Debug, Clone, Copy)]
  /// # struct STok;
  /// # impl Token<'_> for STok {
  /// #   type Kind = u8;
  /// #   type Error = ();
  /// #   const SURFACES_TRIVIA: bool = true;
  /// #   fn kind(&self) -> u8 { 0 }
  /// #   fn is_trivia(&self) -> bool { false }
  /// # }
  /// # struct Lossless<'a> { src: &'a str, state: () }
  /// # impl<'inp> Lexer<'inp> for Lossless<'inp> {
  /// #   type State = (); type Source = str; type Token = STok;
  /// #   type Span = SimpleSpan; type Offset = usize;
  /// #   fn new(src: &'inp str) -> Self { Self { src, state: () } }
  /// #   fn with_state(src: &'inp str, state: ()) -> Self { Self { src, state } }
  /// #   fn check(&self) -> Result<(), ()> { Ok(()) }
  /// #   fn state(&self) -> &() { &self.state }
  /// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
  /// #   fn into_state(self) -> () { self.state }
  /// #   fn source(&self) -> &'inp str { self.src }
  /// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(0, 0) }
  /// #   fn slice(&self) -> &'inp str { "" }
  /// #   fn lex(&mut self) -> Option<Result<STok, ()>> { None }
  /// #   fn bump(&mut self, _: &usize) {}
  /// # }
  /// let profile = CstProfile::new(|_| 0, KindValidator::new(|k| k <= 91), 90, 91);
  /// let _sink: Sink<'_, Lossless<'_>, TableKeyed> =
  ///   Sink::new("", TableKeyed::default(), profile);
  /// ```
  #[inline]
  pub fn new(source: &'inp L::Source, inner: E, profile: CstProfile<L::Token>) -> Self
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
    Self {
      inner,
      events: Vec::new(),
      journal: Vec::new(),
      rows: RefCell::new(Vec::new()),
      floor: MarkRow::ZERO,
      ledger: TruncationLedger::new(),
      base_inner: None,
      source,
      profile,
      trivia: TriviaPolicy::AsEmitted,
      witness: next_sink_witness(),
      _lexer: PhantomData,
    }
  }

  /// Sets the materialization-time trivia policy (builder form).
  #[inline(always)]
  #[must_use]
  pub fn with_trivia_policy(mut self, policy: TriviaPolicy) -> Self {
    self.trivia = policy;
    self
  }

  /// The wrapped emitter, by shared reference.
  ///
  /// Deliberately no `&mut` counterpart: a caller who could drive the inner emitter's
  /// `rewind` directly would shear the event log from the diagnostic log with no witness.
  /// The mutable path to the inner emitter is the sink's own trait surface; ownership
  /// comes back from [`finish`](Self::finish) / [`finish_partial`](Self::finish_partial).
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

  /// Derives the open-node depth of the current buffer: the nearest frozen `(mark, depth)`
  /// fact (the newest of the released floor and the innermost live row) plus the summed
  /// deltas of the events above it. Depth is **never** cached live — this recount is the
  /// restore rule (truncation restores it for free).
  fn derived_depth(&self) -> i64 {
    let top = self.rows.borrow().last().copied();
    let base = match top {
      Some(row) if row.mark >= self.floor.mark => row,
      _ => self.floor,
    };
    let from = (base.mark as usize).min(self.events.len());
    base.depth
      + self.events[from..]
        .iter()
        .map(Event::depth_delta)
        .sum::<i64>()
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
      "stale EventMark: the tombstone this mark named no longer exists on the live \
       timeline (a rewind truncated it{}). The wrap intent died with the branch that \
       conceived it; spending the mark anyway would wrap an unrelated region.",
      if in_bounds {
        " and the buffer regrew over its index"
      } else {
        ""
      },
    );
  }

  /// Records one committed token: the one body behind both doors of the token channel —
  /// the auto-emission hook ([`Emitter::commit_token`], fed by the input layer's settle
  /// primitive) and the raw transport ([`CstEmitter::cst_token`]).
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
    self.events.push(Event::Token {
      kind,
      span: span.clone(),
    });
  }

  /// The hole wrap: brackets the already-buffered token events of a recovery hole in a
  /// `Start(error_kind) … Finish` pair at the recovery site.
  ///
  /// The hole's tokens are the buffer's suffix by construction (they settled during the
  /// scan, after every live mark was captured, and the scanner runs no user code), so the
  /// wrap is a prefix-preserving splice: one insert at the first hole token, one appended
  /// finish. Interleaved `Diag` slots (lexer errors crossed while skipping) ride inside
  /// the wrap unchanged — they are invisible to materialization. If no buffered token
  /// event lies inside the hole span (no auto-emission configured, or a direct call),
  /// there is nothing to wrap and no node is made.
  fn wrap_hole(&mut self, span: &L::Span) {
    let mut wrap_start: Option<usize> = None;
    for (idx, ev) in self.events.iter().enumerate().rev() {
      match ev {
        Event::Diag { .. } => continue,
        Event::Token { span: s, .. }
          if s.start_ref() >= span.start_ref() && s.end_ref() <= span.end_ref() =>
        {
          wrap_start = Some(idx);
        }
        _ => break,
      }
    }
    let Some(at) = wrap_start else {
      return;
    };

    // The splice preserves every prefix a live mark can name: the first hole token
    // postdates every live capture (scan discipline), so nothing below `at` moves.
    debug_assert!(
      self
        .rows
        .borrow()
        .last()
        .is_none_or(|row| row.mark <= at as u64),
      "hole wrap would splice below a live checkpoint mark; the scan discipline \
       guarantees the hole's tokens postdate every live capture"
    );

    let error_kind = self.profile.error_kind();
    self.events.insert(
      at,
      Event::StartNode {
        kind: error_kind,
        forward_parent: None,
      },
    );
    self.events.push(Event::FinishNode { kind: error_kind });

    // Keep the undo journal exact across the splice: positions at or above the insert point
    // shift by one. (Journal entries cannot reference the spliced region — marks and their
    // tombstones all predate the scan — but the bump is exact anyway.)
    for entry in &mut self.journal {
      if entry.at_len > at as u64 {
        entry.at_len += 1;
      }
      if entry.index >= at as u64 {
        entry.index += 1;
      }
    }
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
        let mark = <E as Emitter<'inp, L, Lang>>::checkpoint(&self.inner);
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
  /// `error_span` is `Some` only for a **lexer error** (the one forwarded diagnostic that
  /// names untokenized source bytes); it is recorded into the slot so `finish`'s
  /// gap-coverage law can tell a legitimately-refused byte from a dropped committed token.
  /// Every other `emit_*` passes `None`.
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
    self.events.push(Event::Diag { error_span });
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

  #[inline]
  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    // The one diagnostic that names untokenized bytes: record its span so `finish` can tell
    // this legitimately-refused gap from a dropped committed token.
    self.forward_diag::<Lang, _>(Some(err.span_ref().clone()), |inner| {
      inner.emit_lexer_error(err)
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

  /// Wraps the hole's already-buffered token events in an `error_kind` node at the
  /// recovery site (empty holes and token-less holes produce no node), then forwards the
  /// one-per-hole diagnostic to the inner emitter through the census helper.
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
  fn checkpoint(&self) -> u64 {
    let mark = self.events.len() as u64;
    let depth = self.derived_depth();
    let inner = self.inner.checkpoint();
    self.rows.borrow_mut().push(MarkRow { mark, depth, inner });
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
  /// still spends its capture's row. A truncating rewind to a mid-log mark no live row
  /// captured has no exact inner reading anywhere: debug builds panic at cause; release
  /// builds keep the sink's own channels exact and leave the inner untouched
  /// (unspecified-but-bounded — the sink never guesses a reading; see the *Inner-emitter
  /// contract* on [`Sink`]).
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
      // no row (the mid-log witness in debug, the ghost-inner in release). No live row
      // can sit above `len` (rows are pushed at the current length and truncation pops
      // them first), so no settle is owed here — the no-op is exact, not defensive.
      // `Verbose` may clamp only because it keeps no per-mark bookkeeping: clamp and
      // ignore coincide there; the sink's mark-keyed row stack makes them differ.
      // The boundary is strict: `checkpoint == len` IS the current position — a lawful
      // rewind-to-current that must still spend its capture's row below.
      return;
    }
    let mark = checkpoint;

    // Spend the captures at or above the mark, capturing the target row's inner reading as
    // it is spent: everything strictly above dies with the branch; the newest capture at
    // exactly the mark is the one being rewound to, and it carries the exact inner reading to
    // hand back. A disciplined rewind (guards, attempt, the scan family, correct raw
    // save/restore) always finds that row live — a released mark is a committed mark, never
    // rewound to. `None` is the no-row case, resolved below by what the sink still knows.
    let target_inner = {
      let rows = self.rows.get_mut();
      while rows.last().is_some_and(|row| row.mark > mark) {
        rows.pop();
      }
      let hit = if rows.last().map(|row| row.mark) == Some(mark) {
        rows.pop().map(|row| row.inner)
      } else {
        None
      };
      if self.floor.mark > mark {
        self.floor = rows.last().copied().unwrap_or(MarkRow::ZERO);
      }
      hit
    };

    if mark < len {
      self.events.truncate(mark as usize);
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
    // A truncating no-row rewind to a MID-LOG mark has no exact reading anywhere: the
    // inner's reading is inner-specific (an emission-log length, a token count, a constant)
    // and was never captured at that mark — the mark was never returned by `checkpoint()`,
    // or its capture was already spent by an earlier rewind or release. That is
    // undisciplined raw use: debug builds panic at cause (the sink-level twin of the input
    // layer's LIFO witness, which already rejects it on every input-mediated path); release
    // builds keep the sink's own channels exact and REFUSE TO GUESS an inner reading — the
    // inner stays put, one-sided staleness that preserves every inner-side record the
    // surviving prefix still references. Rewinding to `base` here (the pre-fix behavior) or
    // to a neighboring row's reading would instead destroy committed inner state the
    // surviving log still carries.
    let inner_target = match target_inner {
      Some(reading) => Some(reading),
      None if mark == len => None,
      None if mark == 0 => Some(self.base_inner_mark::<Lang>()),
      None => {
        if cfg!(debug_assertions) {
          panic!(
            "Sink rewind to a mid-log mark with no captured row: mark {mark} of a \
             {len}-event log was never returned by checkpoint(), or its capture was already \
             spent by an earlier rewind or release — no exact inner reading exists for it"
          );
        }
        None
      }
    };
    if let Some(reading) = inner_target {
      self.inner.rewind(cursor, reading);
    }
  }

  /// The auto-emission hook: the input layer settles every committed token through this
  /// one call — the consume settles via its `commit_token` primitive, the scan skips via
  /// `skip_and_report` — so the whole consume surface is tree-producing with zero per-atom
  /// code. Records a `Token` event through the same body as the raw
  /// [`cst_token`](CstEmitter::cst_token) transport.
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
  /// consult). The popped row becomes the derived-depth floor: a frozen fact that keeps
  /// depth recounts short across commit-heavy loops. Marks arrive newest-first on the
  /// crate's paths (O(1) top pop); a mark already gone is a no-op, per the trait's
  /// advisory contract.
  ///
  /// The row's captured inner reading is deliberately **not** forwarded to `inner.release` —
  /// it is a plain value, not an inner-side resource (see *Inner-emitter contract* on the
  /// type); the forward census pins `self.inner.release` at zero so any future forwarding
  /// change must rewrite the contract deliberately.
  fn release(&mut self, checkpoint: u64) {
    let rows = self.rows.get_mut();
    let row = if rows.last().map(|row| row.mark) == Some(checkpoint) {
      rows.pop()
    } else {
      rows
        .iter()
        .rposition(|row| row.mark == checkpoint)
        .map(|pos| rows.remove(pos))
    };
    if let Some(row) = row {
      if row.mark >= self.floor.mark {
        self.floor = row;
      }
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
  fn cst_start(&mut self, kind: u16)
  where
    L: Lexer<'inp>,
  {
    assert!(
      self.profile.validator().admits(kind),
      "node kind outside the dialect's own kind space (the tombstone kind, u16::MAX, is \
       reserved and no validator admits it)"
    );
    self.events.push(Event::StartNode {
      kind,
      forward_parent: None,
    });
  }

  fn cst_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'inp>,
  {
    // Raw transport records the event only — a *settle* reaches the inner through
    // [`Emitter::commit_token`]; forwarding here would fabricate a settle the input layer
    // never made (exactly-once law).
    self.record_token(tok, span);
  }

  fn cst_finish(&mut self, kind: u16)
  where
    L: Lexer<'inp>,
  {
    // Detect-at-cause for TRUE GLOBAL underflow only: a finish must close *some* node
    // that is open anywhere in the buffer (`derived_depth() > 0`). This is the soundest
    // invariant a DEPTH-ONLY predicate can enforce; it is deliberately no stricter,
    // because depth cannot separate the two histories that reach this call with a node
    // still open:
    //
    //   - LEGAL cross-checkpoint close — `cst_start(A); checkpoint m; cst_token;
    //     cst_finish(A)`: A was opened, never rolled back, and this finish closes it. Under
    //     commit/release both events survive balanced; under a rewind of `m` the finish
    //     truncates and A reopens (the truncate-and-reopen semantics the CstEmitter contract
    //     blesses, see `emitter/cst.rs`). The OLD assert compared depth against the innermost
    //     live capture's *frozen* baseline and panicked on this legal history — the defect
    //     this narrowing fixes (see issue #98).
    //
    //   - LEAKED-FINISH misuse — `cst_start(A); checkpoint m; cst_start(B); rewind(m);
    //     cst_token; cst_finish(B)`: the finish was meant for B, but B's start died with the
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
    // Debug-only — the raw surface is sharp by contract.
    debug_assert!(
      self.derived_depth() > 0,
      "cst_finish with no open node anywhere in the buffer (global underflow): the \
       matching start was rolled back (or never emitted), and no enclosing node is open \
       to close instead"
    );
    self.events.push(Event::FinishNode { kind });
  }

  fn cst_mark(&mut self) -> EventMark
  where
    L: Lexer<'inp>,
  {
    let index = self.events.len() as u64;
    self.events.push(Event::StartNode {
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
    self.events.push(Event::StartAt { kind, target, prev });

    // The one journaled in-place write: point the tombstone's forward_parent at the newest
    // wrap, making it the head of the chain materialization walks (`prev` carries the
    // rest) and the integrity canary finish validates in both directions. The journal is
    // what keeps it honest across rewinds — restoring the overwritten value is the
    // pure-copy discipline, and it restores the chain head together with it.
    if let Some(relative) = relative {
      if let Some(Event::StartNode { forward_parent, .. }) = self.events.get_mut(target as usize) {
        self.journal.push(JournalEntry {
          at_len: new_index + 1,
          index: target,
          old_forward_parent: *forward_parent,
        });
        *forward_parent = Some(relative);
      }
    }
  }
}

// ── The forwarded capability family ─────────────────────────────────────────────
//
// Every atomic emitter trait the crate ships forwards through the ONE census helper, so
// `Sink<E>` satisfies every context bound `E` satisfies (the `ComposableEmitter`-shaped
// bundles downstream) and every forwarded diagnostic occupies a Diag slot in the unified
// log. CST_FORWARD_CENSUS locks the set.

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
    self.rows.borrow().len()
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

  /// Test-only raw event injection: the corruption shapes the emission-time debug asserts
  /// refuse (an orphan finish, a reserved kind, a stale wrap target) must still be
  /// constructible in debug builds, because the materialization walls they test are the
  /// **release** line of defense.
  pub(crate) fn push_raw_event_for_tests(&mut self, event: Event<L::Span>) {
    self.events.push(event);
  }
}

#[cfg(test)]
mod tests;
