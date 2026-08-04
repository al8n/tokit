use crate::cst::event::EventMark;

use super::*;

/// The CST event channel: an [`Emitter`] subtrait whose methods record the flat event stream
/// a lossless syntax tree is derived from (see [`cst::event`](crate::cst::event) for the
/// vocabulary and its laws).
///
/// # Why a subtrait, and not more defaulted methods on [`Emitter`]
///
/// Every other capability here is a diagnostic: a wrapper emitter that forgets to forward
/// `emit_warning` loses a warning — annoying, visible, recoverable. Tree events are load
/// bearing: a wrapper that forwards the diagnostic methods but not these would produce a
/// parse whose diagnostics flow perfectly and whose **tree is silently empty**. So the event
/// methods live on this separate trait, and CST-producing parse paths bound
/// `Ctx::Emitter: CstEmitter` — a non-forwarding wrapper is then a **compile error**, never a
/// silent empty tree. CST is the first capability that *binds* rather than defaults.
///
/// One token-shaped residue is out of the bound's reach: the auto-emission hook is the
/// defaulted [`Emitter::commit_token`] on the **core** trait (the consume surface must stay
/// callable without a CST bound), so a wrapper can forward this whole structuring surface
/// and still inherit the core no-op — structure flows, tokens vanish. The type system
/// cannot see a provided-method override, so that shape is caught at the other end: a
/// recording sink's `finish` **refuses** a balanced stream that builds structure without a
/// single committed token over a nonempty source *no lexer error explains*
/// (`cst::FinishError::StructureWithoutTokens`) — a typed error, never a plausible
/// gap-tiled tree. A wrapper must forward
/// [`Emitter::commit_token`] alongside these methods.
///
/// # Defaulted no-ops: diagnostics-only emitters opt in trivially
///
/// Every method has an empty (or inert-value) default, so an emitter with no event channel
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " opts in with an empty `impl` — the crate does exactly that for [`Fatal`], [`Verbose`],"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " opts in with an empty `impl` — the crate does exactly that for [`Fatal`], `Verbose`,"
)]
/// [`Silent`], and [`Ignored`](crate::utils::marker::Ignored). That is what makes *one*
/// parser assembly serve both configurations: over a plain diagnostics emitter the event
/// calls compile to nothing (reference-taking signatures, empty inlined bodies — the
/// zero-cost bar is byte-identical machine code, held by the `__text`-hash standard); over a
/// recording sink the same calls buffer the tree.
///
/// The recording implementation is the `rowan`-gated `Sink`, which buffers events under
/// the one emitter checkpoint/rewind mark so backtracking rewinds the tree exactly as it
/// rewinds diagnostics.
///
/// # Contract: the raw surface is sharp
///
/// These are transport methods with the `enter_label`-class contract: route through the
/// blessed combinators (`node(kind, p)`-shaped bracketing,
/// [`Marker`](crate::cst::event::Marker) for retro-wraps) rather than calling the pair by
/// hand. A hand-rolled unbalanced [`cst_start`](Self::cst_start) /
/// [`cst_finish`](Self::cst_finish) is not detected at emit time — it is detected at the
/// sink's materialization, which refuses to build a wrong tree (typed error, never a panic).
/// Two shapes are worth naming:
///
/// - a **decline or error-unwind** between a start and its finish is safe *when the pair is
///   emitted by a both-exits bracket* (the `labelled` precedent); raw callers own that duty;
/// - a **session point rolled back across a finished node** truncates the finish but not the
///   start — legal, and reported by materialization as a leftover open, per the
///   `begin_point` settle-your-points clause.
///
/// # Identity by kind: what [`cst_finish`](Self::cst_finish)'s argument catches
///
/// [`cst_finish`](Self::cst_finish) takes the kind of the node it means to close, and a
/// recording sink checks that kind against the frame the finish actually lands on
/// (`cst::FinishError::MismatchedFinish`). The shape this exists for is the **leaked finish**:
/// `cst_start(A); checkpoint m; cst_start(B); rewind(m); <token settles>; cst_finish(B)`. B's
/// start died with the rewind, so the finish would otherwise close ancestor A silently, and the
/// resulting event buffer is byte-identical to the legal cross-checkpoint close
/// `cst_start(A); checkpoint m; <token settles>; cst_finish(A)` — no property of the buffer,
/// depth included, separates them. The kind argument does.
///
/// The residue is honest: identity **by kind** is weak. A leaked finish that closes a
/// same-kind ancestor still passes the check, and is then caught only if the stream ends
/// imbalanced (`cst::FinishError::UnclosedNodes`, or an
/// `OrphanFinish` further along). Closing that gap needs a *branded* per-open token — a value
/// minted at each node open and consumed at its finish — which costs a mark-like allocation
/// per node open on the hot emit path. The kind is the cheap 90%; the brand is not paid for.
pub trait CstEmitter<'a, L, Lang: ?Sized = ()>: Emitter<'a, L, Lang> {
  /// Opens a node of `kind`; the matching [`cst_finish`](Self::cst_finish) closes it.
  ///
  /// `kind` is a dialect u16 from the unified kind space; the [`TOMBSTONE`](crate::cst::event::TOMBSTONE)
  /// value is reserved and rejected by recording sinks.
  #[inline(always)]
  fn cst_start(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    let _ = kind;
  }

  // There is deliberately **no** `cst_token` here. Tokens reach the event stream through
  // exactly one door — the defaulted [`Emitter::commit_token`] on the core trait, which the
  // input layer calls once per token that *settles* (consumed, or skipped behind a scan
  // frontier). A second, raw door carrying a caller-chosen span and consuming nothing let a
  // grammar decide which source bytes the tree shows without any settle having happened, and
  // nothing downstream could tell the difference: the resulting log is byte-identical to one a
  // legal parse could produce. Recording is the input layer's to trigger, not the grammar's.

  /// Closes the innermost open node (stack discipline; see the module docs of
  /// [`cst::event`](crate::cst::event) for the derived depth model).
  ///
  /// `kind` is the kind of the node this call *intends* to close — the same value passed to
  /// the matching [`cst_start`](Self::cst_start) or [`cst_start_at`](Self::cst_start_at). A
  /// recording sink carries it into the event log and compares it against the frame the
  /// finish actually lands on; see the *identity by kind* section on the trait for what that
  /// catches and what it does not.
  #[inline(always)]
  fn cst_finish(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    let _ = kind;
  }

  /// Appends an inert tombstone and returns the [`EventMark`] naming it — the anchor for a
  /// later retro-wrap ([`cst_start_at`](Self::cst_start_at)). An unspent mark costs
  /// nothing: an unwrapped tombstone materializes into nothing.
  ///
  /// The default returns an **inert** mark (no event channel to anchor into); spending an
  /// inert mark on a recording sink panics deterministically, at the sink-identity wall
  /// that rejects every foreign mark in every build. Prefer wrapping
  /// the result in a [`Marker`](crate::cst::event::Marker) for the single-use
  /// open/completed/abandoned discipline.
  #[inline(always)]
  fn cst_mark(&mut self) -> EventMark
  where
    L: Lexer<'a>,
  {
    EventMark::inert()
  }

  /// Retro-opens a node of `kind` at `mark`'s tombstone, wrapping everything recorded
  /// since the mark once the matching [`cst_finish`](Self::cst_finish) lands. Append-only
  /// by law: the wrap is a new event naming the tombstone, never an in-place completion
  /// of it. Same-target wraps nest outward: the latest wrap is the outermost node.
  ///
  /// # Panics
  ///
  /// A recording sink validates the mark — in bounds, still a tombstone, era not
  /// invalidated by any later truncation, issued by this sink — and **panics in every
  /// build** on staleness: a stale mark is a parser bug (the branch that conceived the
  /// wrap was rolled back), and silently wrapping whatever regrew at that index is the
  /// wrong-tree class nothing downstream can detect.
  #[inline(always)]
  fn cst_start_at(&mut self, mark: EventMark, kind: u16)
  where
    L: Lexer<'a>,
  {
    let _ = (mark, kind);
  }
}

impl<'a, L, U, Lang: ?Sized> CstEmitter<'a, L, Lang> for &mut U
where
  U: CstEmitter<'a, L, Lang>,
{
  #[inline(always)]
  fn cst_start(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    (**self).cst_start(kind)
  }

  #[inline(always)]
  fn cst_finish(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    (**self).cst_finish(kind)
  }

  #[inline(always)]
  fn cst_mark(&mut self) -> EventMark
  where
    L: Lexer<'a>,
  {
    (**self).cst_mark()
  }

  #[inline(always)]
  fn cst_start_at(&mut self, mark: EventMark, kind: u16)
  where
    L: Lexer<'a>,
  {
    (**self).cst_start_at(mark, kind)
  }
}

// The shipped diagnostics-only emitters opt into the event channel with the defaulted
// no-ops: one parser assembly can then bound `Ctx::Emitter: CstEmitter` and run tree-less
// (today's behavior, zero cost) or tree-building (a `Sink`) by configuration alone. The
// wrapper-emitter trap this trait exists to close is untouched: a *wrapper* type gets no
// blanket opt-in and must implement (and forward) the trait deliberately.

impl<'a, L, E, Lang: ?Sized> CstEmitter<'a, L, Lang> for Fatal<E, Lang> where
  Self: Emitter<'a, L, Lang>
{
}

impl<'a, L, E, Lang: ?Sized> CstEmitter<'a, L, Lang> for Silent<E, Lang> where
  Self: Emitter<'a, L, Lang>
{
}

impl<'a, L, Lang: ?Sized> CstEmitter<'a, L, Lang> for Ignored where Self: Emitter<'a, L, Lang> {}

#[cfg(any(feature = "std", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
impl<'a, L, Error, S, Lang: ?Sized> CstEmitter<'a, L, Lang> for Verbose<Error, S, Lang> where
  Self: Emitter<'a, L, Lang>
{
}
