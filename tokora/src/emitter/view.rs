//! The emitter surface a **callback** is handed: forwarding methods rather than the emitter.
//!
//! # Why the value is not handed out
//!
//! A parse's emitter is installed by the entry that started the parse. The lossless drivers
//! ([`parse_lossless`](crate::cst::parse_lossless)) pin that slot to the recording
//! [`Sink`](crate::cst::Sink) *by name*, so nothing of a caller's own can occupy it — but a pin on
//! the entry is worth nothing if the parse then hands the live emitter back out. `&mut E` **is**
//! an emitter: code holding one can wrap it in a type of its own and install that wrapper as the
//! context of a second parse over a **different** buffer, and the sink then records a foreign
//! parse's structure and materializes it over its own bytes — a tree whose shape came from one
//! source and whose text came from another, in an event log byte-identical to a legal one.
//!
//! [`InputRef`](crate::InputRef) closed that door for the *handle*: its `emitter()` accessor is
//! crate-private and the operations are reached through forwarding methods instead. The handle was
//! never the only door. Several public callback traits take the emitter as a **parameter**, by
//! `&mut`, and hand it to code the consumer wrote:
//!
//! - [`Decision::decide`](crate::Decision::decide) — the condition of every `*_while` combinator;
//! - [`ParseInput::peek_then`](crate::ParseInput::peek_then) and
//!   [`ParseChoice::peek_then_choice`](crate::ParseChoice::peek_then_choice) — the peek-gate and
//!   dispatch handlers;
//! - [`PrattFoldTokenPrefix`](crate::parser::PrattFoldTokenPrefix),
//!   [`PrattFoldTokenInfix`](crate::parser::PrattFoldTokenInfix) and
//!   [`PrattFoldTokenPostfix`](crate::parser::PrattFoldTokenPostfix) — the token-level pratt folds.
//!
//! Every one of them now receives an [`EmitterView`] instead. A view **implements no emitter
//! trait**, does not [`Deref`](core::ops::Deref) to the emitter, and exposes no accessor yielding
//! `&mut E`. That non-implementation is the whole mechanism: there is nothing at the end of a
//! method call to put in an emitter slot, and the value itself cannot stand in one.
//!
//! # One surface, two exposures
//!
//! [`InputRef`](crate::InputRef)'s own `emit_*` / `cst_*` forwarders delegate to this type rather
//! than re-implementing it, so the handle's surface and the callback's surface are the *same*
//! surface and cannot drift apart. The method names are the **emitter's own**, so a body that used
//! to be handed `&mut E` keeps every statement it had; only the parameter's type changes.
//!
//! # What is deliberately *not* forwarded
//!
//! The same five [`Emitter`] members [`InputRef`](crate::InputRef) withholds, for the same
//! reasons, and the `SETTLE_CENSUS` / `RELEASE_CENSUS` / `LEXER_ERROR_CENSUS` source censuses in
//! `input_ref/census_tests.rs` exist to keep it that way:
//!
//! - [`Emitter::checkpoint`], [`Emitter::rewind`] and [`Emitter::release`] are the emitter half of
//!   the input's checkpoint lineage. A caller-driven call desynchronizes the emitter from the input
//!   that is supposed to own its marks; the disciplined doors are the transaction guards on the
//!   handle, every one of which spends the mark it took.
//! - [`Emitter::commit_token`] is the auto-emission chokepoint, called exactly once per token that
//!   *settles*. It is the **only** producer of token events in a recording sink — `cst_token` was
//!   deleted for being the second — so withholding it is what keeps a byte of one buffer from ever
//!   being paired with a span decided by another. It is also the caller-chosen-span,
//!   no-consumption door: a span in the log that no committed token accounts for.
//! - [`Emitter::commit_lexer_error`] is that same door on the **diagnostic** channel, and it was
//!   found open one round after `cst_token` was shut. A recording sink's materialization tiles a
//!   source byte no token covers only where a recorded lexer error covers it, so a recorded span
//!   is a *licence*, not a note — and a licence a caller picks is a caller deciding which bytes of
//!   the sink's buffer need no token. So the layer's own refusals come through this member, which
//!   is not here, and everyone else's come through [`emit_lexer_error`](Self::emit_lexer_error),
//!   which is: the report is delivered, and it licenses nothing.
//!
//! Everything else is forwarded verbatim: the rest of [`Emitter`] — the whole diagnostic surface,
//! [`emit_lexer_error`](Self::emit_lexer_error) included — the whole [`CstEmitter`] structuring
//! surface, and the capability channels the collecting combinators emit through, each under its own
//! method-level bound so a diagnostics-only context is unaffected. The capability was never the
//! problem: a callback must be able to say *"this input is malformed here"* inline, without
//! rewinding, which is the whole reason the `decide` family exists.

use core::marker::PhantomData;

use crate::{
  Lexer, Token,
  cst::event::EventMark,
  emitter::{
    CstEmitter, Emitter, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnclosedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::token::UnexpectedTokenOf,
  span::Spanned,
};

// The two pratt forwarders below are the only members that name it.
#[cfg(feature = "pratt")]
use crate::emitter::PrattEmitter;

/// The emitter's **operations**, lent to a callback — never the emitter.
///
/// Handed by value to every public callback that used to receive `&mut Ctx::Emitter`:
/// [`Decision::decide`](crate::Decision::decide), the `peek_then` family's handlers, and the
/// token-level pratt folds. The methods carry the emitter's own names, so a body written against
/// `&mut E` keeps every statement it had.
///
/// # It is not an emitter, and that is the point
///
/// `EmitterView` implements no emitter trait, does not [`Deref`](core::ops::Deref), and hands back
/// no `&mut E`. A wrapper built around one can forward nothing, so it cannot occupy the emitter
/// slot of a second parse over a foreign buffer — the wrong-source class the lossless drivers'
/// pinned slot closes at the entry, and which a callback parameter used to re-open from inside.
/// See the module documentation for the class and for the four members deliberately withheld.
///
/// [`emitter_ref`](Self::emitter_ref) hands back `&E` — the door for *reading* a concrete
/// emitter's own state mid-parse (a collecting emitter's diagnostics, a counter, a label stack). A
/// **shared** reference cannot re-enter an emitter slot either: every recording method on the trait
/// family takes `&mut self`, so a wrapper built around `&E` can forward none of them.
///
/// # The bound of that claim
///
/// "Implements no emitter trait" is a fact about **this crate**, and the orphan rules do not make
/// it universal: a downstream crate whose own lexer type appears in the parameter list may write
/// `impl Emitter<'_, MyLexer<'_>> for EmitterView<'_, '_, MyLexer<'_>, Sink<..>>` — the trait and
/// this type are both foreign, but `MyLexer` is local and no uncovered type parameter precedes it
/// — and install *that* over a second buffer.
///
/// What such an implementation can forward is bounded by the surface below, and the two members
/// that decide what a source byte *is* are not on it:
///
/// - [`Emitter::commit_token`], the auto-emission chokepoint and — since `CstEmitter::cst_token`
///   was deleted — the only producer of token events. A token is what pairs a span with a byte, so
///   a second parse driven this way contributes no token, no span and no byte of its own buffer.
/// - [`Emitter::commit_lexer_error`], the only producer of **gap-coverage evidence**. Saying that
///   `commit_token` was the only door onto the sink's structural machinery was, for one round,
///   incomplete: a recorded lexer error's span licenses a gap tile, so a foreign parse whose input
///   layer reported *its own* refusals through such a wrapper was excusing bytes of this sink's
///   buffer that this parse never covered. The forwarded
///   [`emit_lexer_error`](Self::emit_lexer_error) records no coverage span, so that route now
///   delivers a diagnostic and nothing structural.
///
/// What is left is the diagnostic and node channels — which a callback body can already reach
/// directly through this same surface, by design — and the tree still materializes over the buffer
/// its sink was minted from, covered by exactly the bytes that parse's own tokens and its own
/// lexer's refusals account for.
pub struct EmitterView<'a, 'inp, L, E, Lang: ?Sized = ()>
where
  L: Lexer<'inp>,
{
  emitter: &'a mut E,
  // `fn() -> _`, not the types themselves: the view stores neither an `L` nor a `Lang`, so it must
  // not inherit their auto-traits — a lexer that is not `Send` must not un-`Send` a view over a
  // `Send` emitter. A fn-pointer phantom is `Send + Sync` unconditionally and keeps covariance.
  // Variance and auto-traits are public API the moment they ship.
  _marker: PhantomData<(&'inp (), fn() -> L, fn() -> *const Lang)>,
}

impl<'inp, L, E, Lang: ?Sized> core::fmt::Debug for EmitterView<'_, 'inp, L, E, Lang>
where
  L: Lexer<'inp>,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("EmitterView").finish_non_exhaustive()
  }
}

impl<'a, 'inp, L, E, Lang: ?Sized> EmitterView<'a, 'inp, L, E, Lang>
where
  L: Lexer<'inp>,
{
  /// Lends `emitter`'s operations.
  ///
  /// # Why this is public, and grants nothing
  ///
  /// It takes a `&mut E` — so a caller who can build a view already held the very thing the view
  /// exists to withhold, and gains no reach by wrapping it. **Inside** a parse there is no `&mut
  /// Ctx::Emitter` to pass: the handle's accessor is crate-private and the callback traits hand
  /// out views, which is the wall. This constructor is for the other side of it: exercising a
  /// [`Decision`](crate::Decision) body, a `peek_then` handler, or a token-level pratt fold
  /// **directly**, over an emitter the test owns — `EmitterView::new(&mut Verbose::new())`, or
  /// `&mut ()` for a fold that ignores the parameter. Without it those bodies would only be
  /// reachable through a whole parse, which is a rewrite of every unit test they have.
  #[inline(always)]
  pub const fn new(emitter: &'a mut E) -> Self {
    Self {
      emitter,
      _marker: PhantomData,
    }
  }

  /// Reborrows this view for a shorter life.
  ///
  /// The view is not [`Copy`] — it holds a `&mut` — so a body that hands it to two helpers in
  /// sequence needs this, exactly as it would have needed `&mut *emitter` before.
  #[inline(always)]
  pub fn reborrow(&mut self) -> EmitterView<'_, 'inp, L, E, Lang> {
    EmitterView::new(self.emitter)
  }

  /// The emitter, by **shared** reference — for reading a concrete emitter's own state while the
  /// parse is running (a collecting emitter's recorded diagnostics, a counter, a label stack).
  ///
  /// Shared on purpose, and that is the whole of the difference: every method that *records*
  /// anything takes `&mut self`, so a wrapper type built around this reference can forward none of
  /// them and cannot stand in an emitter slot. To emit, call the forwarding methods on this view.
  #[inline(always)]
  pub const fn emitter_ref(&self) -> &E {
    self.emitter
  }
}

impl<'inp, L, E, Lang: ?Sized> EmitterView<'_, 'inp, L, E, Lang>
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L, Lang>,
{
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
  /// where a lexer error **the input layer raised** covers it. A report raised here carries a span
  /// the *caller* chose, with nothing consumed for it, so the sink records the diagnostic and no
  /// coverage span: an uncovered byte stays uncovered and `finish` still refuses it
  #[cfg_attr(
    feature = "rowan",
    doc = " ([`FinishError::UncoveredGap`](crate::cst::FinishError::UncoveredGap)). The evidence door is"
  )]
  #[cfg_attr(
    not(feature = "rowan"),
    doc = " (`FinishError::UncoveredGap`). The evidence door is"
  )]
  /// [`Emitter::commit_lexer_error`], which is deliberately not on this surface.
  #[inline(always)]
  pub fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), E::Error> {
    self.emitter.emit_lexer_error(err)
  }

  /// Emits an unexpected-token report — [`Emitter::emit_unexpected_token`], forwarded.
  ///
  /// This does **not** publish the front-report watermark the input layer maintains for the token
  /// at the stream front, so a later close-miss report about the same token is not suppressed by
  /// it. Same direction as above: an extra diagnostic, never a missing one.
  #[inline(always)]
  pub fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), E::Error> {
    self.emitter.emit_unexpected_token(err)
  }

  /// Emits an application error — [`Emitter::emit_error`], forwarded.
  #[inline(always)]
  pub fn emit_error(&mut self, err: Spanned<E::Error, L::Span>) -> Result<(), E::Error> {
    self.emitter.emit_error(err)
  }

  /// Emits a warning — [`Emitter::emit_warning`], forwarded.
  #[inline(always)]
  pub fn emit_warning(&mut self, warning: Spanned<E::Error, L::Span>) -> Result<(), E::Error> {
    self.emitter.emit_warning(warning)
  }

  /// Emits a recovery-hole note — [`Emitter::emit_skipped_region`], forwarded.
  ///
  /// **Span semantics.** Under a CST sink this call also has a structural effect: the hole's
  /// buffered tokens are bracketed in an error node, and that bracket covers only the hole
  /// tokens that settled **within the transaction reporting the hole** — at or above the
  /// youngest live checkpoint. A wider `span` still reaches the diagnostic channel verbatim;
  /// it exerts no structural authority over tokens committed before that checkpoint, because
  /// checkpoint marks are event-log positions and a node spliced beneath one would rename it.
  #[inline(always)]
  pub fn emit_skipped_region(&mut self, span: L::Span, skipped: usize) -> Result<(), E::Error> {
    self.emitter.emit_skipped_region(span, skipped)
  }

  /// Pushes a diagnostic label — [`Emitter::enter_label`], forwarded.
  ///
  /// Pairs with [`exit_label`](Self::exit_label); prefer [`labelled`](crate::labelled), which
  /// pairs them through a drop guard.
  #[inline(always)]
  pub fn enter_label(&mut self, label: &'static str) {
    self.emitter.enter_label(label);
  }

  /// Pops the innermost diagnostic label — [`Emitter::exit_label`], forwarded.
  #[inline(always)]
  pub fn exit_label(&mut self) {
    self.emitter.exit_label();
  }

  /// The source the emitter is bound to, if any — [`Emitter::bound_source`], forwarded.
  ///
  /// A query, not an emission: it answers for anyone who can reach the emitter, which is why no
  /// sink-side witness built on it can encode *who* asked.
  #[inline(always)]
  pub fn bound_source(&self) -> Option<crate::source::SourceIdentity> {
    Emitter::<'inp, L, Lang>::bound_source(&*self.emitter)
  }

  /// Opens a CST node of `kind` — [`CstEmitter::cst_start`], forwarded.
  ///
  /// Raw transport: pair it with [`cst_finish`](Self::cst_finish) through a both-exits bracket, or
  /// use the `node`-shaped combinators. The returned mark is the failing exit's handle — spend
  /// it on [`cst_demote`](Self::cst_demote).
  #[inline(always)]
  pub fn cst_start(&mut self, kind: u16) -> EventMark
  where
    E: CstEmitter<'inp, L, Lang>,
  {
    self.emitter.cst_start(kind)
  }

  /// Closes the innermost open CST node — [`CstEmitter::cst_finish`], forwarded.
  #[inline(always)]
  pub fn cst_finish(&mut self, kind: u16)
  where
    E: CstEmitter<'inp, L, Lang>,
  {
    self.emitter.cst_finish(kind);
  }

  /// Un-opens the node started at `mark` — [`CstEmitter::cst_demote`], forwarded.
  #[inline(always)]
  pub fn cst_demote(&mut self, mark: EventMark, kind: u16)
  where
    E: CstEmitter<'inp, L, Lang>,
  {
    self.emitter.cst_demote(mark, kind);
  }

  /// Appends a retro-wrap anchor — [`CstEmitter::cst_mark`], forwarded.
  #[inline(always)]
  pub fn cst_mark(&mut self) -> EventMark
  where
    E: CstEmitter<'inp, L, Lang>,
  {
    self.emitter.cst_mark()
  }

  /// Retro-opens a node of `kind` at `mark` — [`CstEmitter::cst_start_at`], forwarded.
  #[inline(always)]
  pub fn cst_start_at(&mut self, mark: EventMark, kind: u16)
  where
    E: CstEmitter<'inp, L, Lang>,
  {
    self.emitter.cst_start_at(mark, kind);
  }

  /// [`TooFewEmitter::emit_too_few`], forwarded.
  #[inline(always)]
  pub fn emit_too_few(
    &mut self,
    err: crate::error::syntax::TooFew<L::Span, Lang>,
  ) -> Result<(), E::Error>
  where
    E: TooFewEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_too_few(err)
  }

  /// [`TooManyEmitter::emit_too_many`], forwarded.
  #[inline(always)]
  pub fn emit_too_many(
    &mut self,
    err: crate::error::syntax::TooMany<L::Span, Lang>,
  ) -> Result<(), E::Error>
  where
    E: TooManyEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_too_many(err)
  }

  /// [`FullContainerEmitter::emit_full_container`], forwarded.
  #[inline(always)]
  pub fn emit_full_container(
    &mut self,
    err: crate::error::syntax::FullContainer<L::Span, Lang>,
  ) -> Result<(), E::Error>
  where
    E: FullContainerEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_full_container(err)
  }

  /// [`SeparatedEmitter::emit_missing_separator`], forwarded.
  #[inline(always)]
  pub fn emit_missing_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: crate::error::token::MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), E::Error>
  where
    E: SeparatedEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_missing_separator(name, err)
  }

  /// [`SeparatedEmitter::emit_missing_element`], forwarded.
  #[inline(always)]
  pub fn emit_missing_element(
    &mut self,
    err: crate::error::syntax::MissingSyntaxOf<'inp, L, Lang>,
  ) -> Result<(), E::Error>
  where
    E: SeparatedEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_missing_element(err)
  }

  /// [`MissingLeadingSeparatorEmitter::emit_missing_leading_separator`], forwarded.
  #[inline(always)]
  pub fn emit_missing_leading_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: crate::error::token::MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), E::Error>
  where
    E: MissingLeadingSeparatorEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_missing_leading_separator(name, err)
  }

  /// [`MissingTrailingSeparatorEmitter::emit_missing_trailing_separator`], forwarded.
  #[inline(always)]
  pub fn emit_missing_trailing_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: crate::error::token::MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), E::Error>
  where
    E: MissingTrailingSeparatorEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_missing_trailing_separator(name, err)
  }

  /// [`UnexpectedLeadingSeparatorEmitter::emit_unexpected_leading_separator`], forwarded.
  #[inline(always)]
  pub fn emit_unexpected_leading_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), E::Error>
  where
    E: UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_unexpected_leading_separator(name, err)
  }

  /// [`UnexpectedTrailingSeparatorEmitter::emit_unexpected_trailing_separator`], forwarded.
  #[inline(always)]
  pub fn emit_unexpected_trailing_separator(
    &mut self,
    name: crate::utils::CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), E::Error>
  where
    E: UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_unexpected_trailing_separator(name, err)
  }

  /// [`UnclosedEmitter::emit_unclosed`], forwarded.
  #[inline(always)]
  pub fn emit_unclosed<Delimiter>(
    &mut self,
    err: crate::error::Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), E::Error>
  where
    E: UnclosedEmitter<'inp, L, Lang>,
    E::Error: crate::emitter::FromUnclosed<'inp, L, Lang>,
  {
    self.emitter.emit_unclosed(err)
  }

  /// [`PrattEmitter::emit_unexpected_end_of_lhs`], forwarded.
  #[cfg(feature = "pratt")]
  #[cfg_attr(docsrs, doc(cfg(feature = "pratt")))]
  #[inline(always)]
  pub fn emit_unexpected_end_of_lhs(
    &mut self,
    err: crate::error::UnexpectedEoLhs<L::Offset, Lang>,
  ) -> Result<(), E::Error>
  where
    E: PrattEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_unexpected_end_of_lhs(err)
  }

  /// [`PrattEmitter::emit_unexpected_end_of_rhs`], forwarded.
  #[cfg(feature = "pratt")]
  #[cfg_attr(docsrs, doc(cfg(feature = "pratt")))]
  #[inline(always)]
  pub fn emit_unexpected_end_of_rhs(
    &mut self,
    err: crate::error::UnexpectedEoRhs<L::Offset, Lang>,
  ) -> Result<(), E::Error>
  where
    E: PrattEmitter<'inp, L, Lang>,
  {
    self.emitter.emit_unexpected_end_of_rhs(err)
  }
}
