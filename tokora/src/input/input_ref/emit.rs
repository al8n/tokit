//! The emitter surface a parser reaches **through the handle**: forwarding methods rather than
//! the emitter itself.
//!
//! # Why the value is not handed out
//!
//! A parse's emitter is installed by the entry that started the parse. The lossless drivers
//! ([`parse_lossless`](crate::cst::parse_lossless)) pin that slot to the recording `Sink` *by
//! name*, so nothing of a caller's own can occupy it — but a pin on the entry is worth nothing
//! if the parse then hands the live emitter back out. `&mut Ctx::Emitter` **is** an emitter: a
//! parser holding one can wrap it in a type of its own and install that wrapper as the context
//! of a second parse over a **different** buffer, and the sink then records a foreign parse's
//! structure and materializes it over its own bytes — a tree whose shape came from one source
//! and whose text came from another, in an event log byte-identical to a legal one.
//!
//! So the handle exposes the emitter's **operations** and never the emitter. A method call is
//! not a value: `inp.cst_start(k)` does everything `inp.emitter().cst_start(k)` did, and there
//! is nothing to put in an emitter slot at the end of it.
//!
//! **The callback parameters got the same treatment.** `Decision::decide` — the condition of
//! every `*_while` combinator — the `peek_then` family's handlers, and the token-level pratt folds
//! (`PrattFoldTokenPrefix` / `Infix` / `Postfix`) used to take `&mut Ctx::Emitter` and hand it to
//! caller-written code. Each now receives an [`EmitterView`](crate::EmitterView): the same
//! operations, under the same names, in a value that implements no emitter trait and so cannot
//! occupy an emitter slot.
//!
//! [`emitter_ref`](InputRef::emitter_ref) remains public and hands back `&Ctx::Emitter` — the
//! door for reading a concrete emitter's own state mid-parse (a collecting emitter's diagnostics,
//! say). A **shared** reference cannot re-enter an emitter slot: every recording method on the
//! trait family takes `&mut self`, so a wrapper built around `&Sink` can forward none of them.
//!
//! # What is deliberately *not* forwarded
//!
//! Five `Emitter` members have exactly one lawful caller — the input layer itself — and the
//! `SETTLE_CENSUS` / `RELEASE_CENSUS` / `LEXER_ERROR_CENSUS` source censuses in `census_tests.rs`
//! exist to keep it that way:
//!
//! - [`Emitter::checkpoint`], [`Emitter::rewind`] and [`Emitter::release`] are the emitter half
//!   of the handle's own checkpoint lineage. A caller-driven call desynchronizes the emitter
//!   from the input that is supposed to own its marks; the disciplined doors are
//!   [`begin`](InputRef::begin), [`begin_stacked`](InputRef::begin_stacked),
//!   [`begin_point`](InputRef::begin_point), [`attempt`](InputRef::attempt) and the
//!   `unstable-raw` `save`/`restore`/`commit` triple, every one of which spends the mark it
//!   took.
//! - [`Emitter::commit_token`] is the auto-emission chokepoint, called exactly once per token
//!   that *settles*. Forwarding it would hand back the caller-chosen-span, no-consumption door
//!   that removing `CstEmitter::cst_token` closed: a span in the log that no committed token
//!   accounts for.
//! - [`Emitter::commit_lexer_error`] is the same door on the diagnostic channel: the input
//!   layer's own refusal, over bytes it lexed and could not tokenize. A recording sink treats its
//!   span as the licence to tile those bytes as a gap, so forwarding it would let a parser decide
//!   which source bytes need no token — which is the `cst_token` shape exactly.
//!   [`emit_lexer_error`](InputRef::emit_lexer_error) below is the caller's door onto the same
//!   report, and records no coverage span.
//!
//! Everything else is forwarded verbatim: the rest of [`Emitter`], the whole [`CstEmitter`]
//! structuring surface, and the twelve capability channels the collecting combinators emit
//! through (`TooFewEmitter`, `SeparatedEmitter`, `UnclosedEmitter`, `PrattEmitter`, …), each
//! under its own method-level bound so a diagnostics-only context is unaffected. The capability
//! was never the problem.
//!
//! # One surface, two exposures
//!
//! The bodies below are **delegations to [`EmitterView`](crate::EmitterView)**, the same value the
//! callback traits receive — not a second implementation of the same forwarding. The handle's
//! surface and the callback's surface are therefore the *same* surface, and a member added to or
//! withheld from one is added to or withheld from both. The two `&self` readers
//! ([`emitter_ref`](InputRef::emitter_ref) and
//! [`emitter_bound_source`](InputRef::emitter_bound_source)) are the exception, and only because a
//! view is built from a `&mut` borrow that a `&self` method does not have.

use super::*;

use crate::{
  cst::event::EventMark,
  emitter::{
    CstEmitter, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnclosedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::token::UnexpectedTokenOf,
};

// The two pratt forwarders below are the only members that name it.
#[cfg(feature = "pratt")]
use crate::emitter::PrattEmitter;

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// The parse's emitter, by **shared** reference — for reading a concrete emitter's own state
  /// while the parse is running (a collecting emitter's recorded diagnostics, a counter, a
  /// label stack).
  ///
  /// Shared on purpose, and that is the whole of the difference: every method that *records*
  /// anything — on [`Emitter`], on [`CstEmitter`], on the atomic emitter family — takes
  /// `&mut self`, so a wrapper type built around this reference can forward none of them and
  /// cannot stand in an emitter slot. To emit, call the forwarding methods on this handle.
  #[inline(always)]
  pub const fn emitter_ref(&self) -> &Ctx::Emitter {
    &*self.session.emitter
  }

  /// The parse's emitter **operations**, as the value every callback receives.
  ///
  /// Crate-private, and it does not need to be more: the forwarding methods below *are* this
  /// view's methods, reached one call shorter. It exists so the handle and the callback traits
  /// share one implementation of the forwarding rather than two that can drift.
  #[inline(always)]
  pub(crate) fn emitter_view(&mut self) -> EmitterView<'_, 'inp, L, Ctx::Emitter, Lang> {
    EmitterView::new(self.session.emitter)
  }

  /// Emits a lexer error — [`Emitter::emit_lexer_error`], forwarded.
  ///
  /// The input layer's own lexer-error reports are deduped against a watermark; a report raised
  /// here is not, so a caller re-reporting a region the layer already reported produces two
  /// diagnostics rather than one. Noisy, never silent.
  ///
  /// # It reports; it does not license
  ///
  #[cfg_attr(
    feature = "rowan",
    doc = " A recording [`Sink`](crate::cst::Sink) tiles a source byte no committed token covers only"
  )]
  #[cfg_attr(
    not(feature = "rowan"),
    doc = " A recording `Sink` tiles a source byte no committed token covers only"
  )]
  /// where a lexer error the *input layer* raised covers it. The span handed over here is the
  /// caller's, with no consumption behind it, so the sink records the diagnostic and no coverage
  /// span: an uncovered byte stays uncovered and `finish` still refuses it
  #[cfg_attr(
    feature = "rowan",
    doc = " ([`FinishError::UncoveredGap`](crate::cst::FinishError::UncoveredGap)). An unconsumed region"
  )]
  #[cfg_attr(
    not(feature = "rowan"),
    doc = " (`FinishError::UncoveredGap`). An unconsumed region"
  )]
  /// is materialized through
  #[cfg_attr(
    feature = "rowan",
    doc = " [`Cst::finish_partial`](crate::cst::Cst::finish_partial), which tiles it — not by reporting"
  )]
  #[cfg_attr(
    not(feature = "rowan"),
    doc = " `Cst::finish_partial`, which tiles it — not by reporting"
  )]
  /// a lexer error over it.
  #[inline(always)]
  pub fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.emitter_view().emit_lexer_error(err)
  }

  /// Emits an unexpected-token report — [`Emitter::emit_unexpected_token`], forwarded.
  ///
  /// This does **not** publish the front-report watermark the input layer maintains for the
  /// token at the stream front, so a later close-miss report about the same token is not
  /// suppressed by it. Same direction as above: an extra diagnostic, never a missing one.
  #[inline(always)]
  pub fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.emitter_view().emit_unexpected_token(err)
  }

  /// Emits an application error — [`Emitter::emit_error`], forwarded.
  #[inline(always)]
  pub fn emit_error(
    &mut self,
    err: Spanned<<Ctx::Emitter as Emitter<'inp, L, Lang>>::Error, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.emitter_view().emit_error(err)
  }

  /// Emits a warning — [`Emitter::emit_warning`], forwarded.
  #[inline(always)]
  pub fn emit_warning(
    &mut self,
    warning: Spanned<<Ctx::Emitter as Emitter<'inp, L, Lang>>::Error, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.emitter_view().emit_warning(warning)
  }

  /// Emits a recovery-hole note — [`Emitter::emit_skipped_region`], forwarded.
  ///
  /// [`sync_balanced`](InputRef::sync_balanced) raises exactly one of these per hole it skips;
  /// a caller running its own recovery loop is the reason this is reachable at all.
  ///
  /// **Span semantics.** Under a CST sink this call also has a structural effect: the hole's
  /// buffered tokens are bracketed in an error node. That bracket covers only the hole tokens
  /// that settled **within the transaction reporting the hole** — at or above the youngest
  /// live checkpoint. A recovery loop that widens `span` backward over tokens it already
  /// committed still gets the report forwarded verbatim, but the error node stops at the
  /// transaction boundary: checkpoint marks are event-log positions, and a node spliced
  /// beneath one would rename it. `sync_balanced`'s own spans postdate every live capture, so
  /// this bound never narrows the crate's own recovery.
  #[inline(always)]
  pub fn emit_skipped_region(
    &mut self,
    span: L::Span,
    skipped: usize,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.emitter_view().emit_skipped_region(span, skipped)
  }

  /// Pushes a diagnostic label — [`Emitter::enter_label`], forwarded.
  ///
  /// Pairs with [`exit_label`](Self::exit_label); prefer [`labelled`](crate::labelled), which
  /// pairs them through a drop guard.
  #[inline(always)]
  pub fn enter_label(&mut self, label: &'static str) {
    self.emitter_view().enter_label(label);
  }

  /// Pops the innermost diagnostic label — [`Emitter::exit_label`], forwarded.
  #[inline(always)]
  pub fn exit_label(&mut self) {
    self.emitter_view().exit_label();
  }

  /// The source the emitter is bound to, if any — [`Emitter::bound_source`], forwarded.
  ///
  /// A query, not an emission: it answers for anyone who can reach the emitter, which is why
  /// no sink-side witness built on it can encode *who* asked.
  ///
  /// One of the two `&self` readers that do **not** delegate to
  /// [`EmitterView`](crate::EmitterView): a view is built from a `&mut` borrow, which a shared
  /// method has not got. The view carries the same method under the emitter's own name,
  /// `bound_source`.
  #[inline(always)]
  pub fn emitter_bound_source(&self) -> Option<crate::source::SourceIdentity> {
    Emitter::<'inp, L, Lang>::bound_source(&*self.session.emitter)
  }

  /// Opens a CST node of `kind` — [`CstEmitter::cst_start`], forwarded.
  ///
  /// Raw transport: pair it with [`cst_finish`](Self::cst_finish) through a both-exits
  /// bracket, or use the `node`-shaped combinators.
  #[inline(always)]
  pub fn cst_start(&mut self, kind: u16)
  where
    Ctx::Emitter: CstEmitter<'inp, L, Lang>,
  {
    self.emitter_view().cst_start(kind);
  }

  /// Closes the innermost open CST node — [`CstEmitter::cst_finish`], forwarded.
  #[inline(always)]
  pub fn cst_finish(&mut self, kind: u16)
  where
    Ctx::Emitter: CstEmitter<'inp, L, Lang>,
  {
    self.emitter_view().cst_finish(kind);
  }

  /// Appends a retro-wrap anchor — [`CstEmitter::cst_mark`], forwarded.
  #[inline(always)]
  pub fn cst_mark(&mut self) -> EventMark
  where
    Ctx::Emitter: CstEmitter<'inp, L, Lang>,
  {
    self.emitter_view().cst_mark()
  }

  /// Retro-opens a node of `kind` at `mark` — [`CstEmitter::cst_start_at`], forwarded.
  #[inline(always)]
  pub fn cst_start_at(&mut self, mark: EventMark, kind: u16)
  where
    Ctx::Emitter: CstEmitter<'inp, L, Lang>,
  {
    self.emitter_view().cst_start_at(mark, kind);
  }

  /// [`TooFewEmitter::emit_too_few`], forwarded.
  #[inline(always)]
  pub fn emit_too_few(
    &mut self,
    err: crate::error::syntax::TooFew<L::Span, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: TooFewEmitter<'inp, L, Lang>,
  {
    self.emitter_view().emit_too_few(err)
  }

  /// [`TooManyEmitter::emit_too_many`], forwarded.
  #[inline(always)]
  pub fn emit_too_many(
    &mut self,
    err: crate::error::syntax::TooMany<L::Span, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: TooManyEmitter<'inp, L, Lang>,
  {
    self.emitter_view().emit_too_many(err)
  }

  /// [`FullContainerEmitter::emit_full_container`], forwarded.
  #[inline(always)]
  pub fn emit_full_container(
    &mut self,
    err: crate::error::syntax::FullContainer<L::Span, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>,
  {
    self.emitter_view().emit_full_container(err)
  }

  /// [`SeparatedEmitter::emit_missing_separator`], forwarded.
  #[inline(always)]
  pub fn emit_missing_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: crate::error::token::MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
  {
    self.emitter_view().emit_missing_separator(name, err)
  }

  /// [`SeparatedEmitter::emit_missing_element`], forwarded.
  #[inline(always)]
  pub fn emit_missing_element(
    &mut self,
    err: crate::error::syntax::MissingSyntaxOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
  {
    self.emitter_view().emit_missing_element(err)
  }

  /// [`MissingLeadingSeparatorEmitter::emit_missing_leading_separator`], forwarded.
  #[inline(always)]
  pub fn emit_missing_leading_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: crate::error::token::MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: MissingLeadingSeparatorEmitter<'inp, L, Lang>,
  {
    self
      .emitter_view()
      .emit_missing_leading_separator(name, err)
  }

  /// [`MissingTrailingSeparatorEmitter::emit_missing_trailing_separator`], forwarded.
  #[inline(always)]
  pub fn emit_missing_trailing_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: crate::error::token::MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: MissingTrailingSeparatorEmitter<'inp, L, Lang>,
  {
    self
      .emitter_view()
      .emit_missing_trailing_separator(name, err)
  }

  /// [`UnexpectedLeadingSeparatorEmitter::emit_unexpected_leading_separator`], forwarded.
  #[inline(always)]
  pub fn emit_unexpected_leading_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>,
  {
    self
      .emitter_view()
      .emit_unexpected_leading_separator(name, err)
  }

  /// [`UnexpectedTrailingSeparatorEmitter::emit_unexpected_trailing_separator`], forwarded.
  #[inline(always)]
  pub fn emit_unexpected_trailing_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>,
  {
    self
      .emitter_view()
      .emit_unexpected_trailing_separator(name, err)
  }

  /// [`UnclosedEmitter::emit_unclosed`], forwarded.
  #[inline(always)]
  pub fn emit_unclosed<Delimiter>(
    &mut self,
    err: crate::error::Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: UnclosedEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: crate::emitter::FromUnclosed<'inp, L, Lang>,
  {
    self.emitter_view().emit_unclosed(err)
  }

  /// [`PrattEmitter::emit_unexpected_end_of_lhs`], forwarded.
  #[cfg(feature = "pratt")]
  #[cfg_attr(docsrs, doc(cfg(feature = "pratt")))]
  #[inline(always)]
  pub fn emit_unexpected_end_of_lhs(
    &mut self,
    err: crate::error::UnexpectedEoLhs<L::Offset, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: PrattEmitter<'inp, L, Lang>,
  {
    self.emitter_view().emit_unexpected_end_of_lhs(err)
  }

  /// [`PrattEmitter::emit_unexpected_end_of_rhs`], forwarded.
  #[cfg(feature = "pratt")]
  #[cfg_attr(docsrs, doc(cfg(feature = "pratt")))]
  #[inline(always)]
  pub fn emit_unexpected_end_of_rhs(
    &mut self,
    err: crate::error::UnexpectedEoRhs<L::Offset, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Ctx::Emitter: PrattEmitter<'inp, L, Lang>,
  {
    self.emitter_view().emit_unexpected_end_of_rhs(err)
  }
}
