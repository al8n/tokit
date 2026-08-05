use super::{input::Cursor, span::Spanned, *};

/// The view of a parse a state-carrying combinator callback is handed: the `*_with` family —
#[cfg_attr(feature = "map", doc = " [`map_with`](crate::ParseInput::map_with),")]
#[cfg_attr(not(feature = "map"), doc = " `map_with`,")]
#[cfg_attr(
  feature = "then",
  doc = " [`and_then_with`](crate::ParseInput::and_then_with),"
)]
#[cfg_attr(not(feature = "then"), doc = " `and_then_with`,")]
#[cfg_attr(
  feature = "validate",
  doc = " [`validate_with`](crate::ParseInput::validate_with),"
)]
#[cfg_attr(not(feature = "validate"), doc = " `validate_with`,")]
#[cfg_attr(
  feature = "fold",
  doc = " and [`try_fold_with`](crate::TryParseInput::try_fold_with)"
)]
#[cfg_attr(not(feature = "fold"), doc = " and `try_fold_with`")]
/// — each build one over the region their sub-parser just consumed and lend it for the call.
///
/// It answers the four questions a callback has about that region — its [`span`](Self::span), its
/// source [`slice`](Self::slice), the lexer [`state`](Self::state) it was read under, and the
/// emission surface to report against ([`emit_error`](Self::emit_error) and its neighbours,
/// forwarded from the emitter, plus [`emitter_ref`](Self::emitter_ref) to read one) — and nothing
/// else. In particular it does **not** consume tokens: the sub-parser already did, and a callback
/// that could parse more would be a parser, not a callback.
///
/// Speculation lives on the input handle, not here. Reach for the transaction guards
/// ([`InputRef::begin`](crate::InputRef::begin),
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " [`begin_stacked`](crate::InputRef::begin_stacked)) for lexical speculation and the"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " `begin_stacked`) for lexical speculation and the"
)]
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " [session points](crate::InputRef::begin_point) for the non-lexical kind a driver steps across"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " session points for the non-lexical kind a driver steps across"
)]
/// separate calls.
pub struct ParseState<'a, 'inp, 'closure, L, Ctx, Lang: ?Sized = (), Cmpl = Complete>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  inp: &'a mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  start: Cursor<'inp, 'closure, L>,
}

/// The constructor, in its own block: only the `*_with` callback combinators build a
/// `ParseState`, and every one of them is a gated family, so in a build with none of them on
/// there is nothing left here to compile.
#[cfg(any(
  feature = "fail",
  feature = "filter",
  feature = "fold",
  feature = "map",
  feature = "then",
  feature = "validate"
))]
impl<'a, 'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl>
  ParseState<'a, 'inp, 'closure, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Create a new `ParseState`.
  #[inline(always)]
  pub(super) const fn new(
    inp: &'a mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    start: Cursor<'inp, 'closure, L>,
  ) -> Self {
    Self { inp, start }
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Returns the span covering the output being parsed.
  #[inline(always)]
  pub fn span(&self) -> L::Span {
    self.inp.span_since(&self.start)
  }

  // `emitter(&mut self) -> &mut Ctx::Emitter` is **gone**, not narrowed. It was a pure
  // re-export of the handle's own accessor, and `&mut Ctx::Emitter` is itself an emitter: a
  // callback handed one can wrap it and install the wrapper as the context of a second parse
  // over a different buffer. Nothing in the crate used this door, so there is nothing left for
  // a `pub(crate)` version to serve — the operations below are the surface.

  /// The parse's emitter, by **shared** reference — the callback's door for reading a concrete
  /// emitter's own state (see [`InputRef::emitter_ref`](crate::InputRef::emitter_ref) for why a
  /// shared reference is not an emitter slot).
  #[inline(always)]
  pub const fn emitter_ref(&self) -> &Ctx::Emitter {
    self.inp.emitter_ref()
  }

  /// Emits a lexer error — [`Emitter::emit_lexer_error`](crate::Emitter::emit_lexer_error),
  /// forwarded through the handle.
  ///
  /// A *report*, not a licence: over a recording sink the span handed over here records no
  /// gap-coverage evidence, because the caller chose it and nothing was consumed for it. See
  /// [`InputRef::emit_lexer_error`](crate::InputRef::emit_lexer_error).
  #[inline(always)]
  pub fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.inp.emit_lexer_error(err)
  }

  /// Emits an unexpected-token report —
  /// [`Emitter::emit_unexpected_token`](crate::Emitter::emit_unexpected_token), forwarded.
  #[inline(always)]
  pub fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.inp.emit_unexpected_token(err)
  }

  /// Emits an application error — [`Emitter::emit_error`](crate::Emitter::emit_error),
  /// forwarded. The usual reason a validating callback exists.
  #[inline(always)]
  pub fn emit_error(
    &mut self,
    err: Spanned<<Ctx::Emitter as Emitter<'inp, L, Lang>>::Error, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.inp.emit_error(err)
  }

  /// Emits a warning — [`Emitter::emit_warning`](crate::Emitter::emit_warning), forwarded.
  #[inline(always)]
  pub fn emit_warning(
    &mut self,
    warning: Spanned<<Ctx::Emitter as Emitter<'inp, L, Lang>>::Error, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.inp.emit_warning(warning)
  }

  /// Emits a recovery-hole note —
  /// [`Emitter::emit_skipped_region`](crate::Emitter::emit_skipped_region), forwarded.
  #[inline(always)]
  pub fn emit_skipped_region(
    &mut self,
    span: L::Span,
    skipped: usize,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.inp.emit_skipped_region(span, skipped)
  }

  /// Pushes a diagnostic label — [`Emitter::enter_label`](crate::Emitter::enter_label),
  /// forwarded.
  #[inline(always)]
  pub fn enter_label(&mut self, label: &'static str) {
    self.inp.enter_label(label);
  }

  /// Pops the innermost diagnostic label — [`Emitter::exit_label`](crate::Emitter::exit_label),
  /// forwarded.
  #[inline(always)]
  pub fn exit_label(&mut self) {
    self.inp.exit_label();
  }

  /// The source the emitter is bound to, if any —
  /// [`Emitter::bound_source`](crate::Emitter::bound_source), forwarded.
  #[inline(always)]
  pub fn emitter_bound_source(&self) -> Option<crate::source::SourceIdentity> {
    self.inp.emitter_bound_source()
  }

  /// Opens a CST node of `kind` —
  /// [`CstEmitter::cst_start`](crate::emitter::CstEmitter::cst_start), forwarded.
  #[inline(always)]
  pub fn cst_start(&mut self, kind: u16)
  where
    Ctx::Emitter: crate::emitter::CstEmitter<'inp, L, Lang>,
  {
    self.inp.cst_start(kind);
  }

  /// Closes the innermost open CST node —
  /// [`CstEmitter::cst_finish`](crate::emitter::CstEmitter::cst_finish), forwarded.
  #[inline(always)]
  pub fn cst_finish(&mut self, kind: u16)
  where
    Ctx::Emitter: crate::emitter::CstEmitter<'inp, L, Lang>,
  {
    self.inp.cst_finish(kind);
  }

  /// Appends a retro-wrap anchor —
  /// [`CstEmitter::cst_mark`](crate::emitter::CstEmitter::cst_mark), forwarded.
  #[inline(always)]
  pub fn cst_mark(&mut self) -> crate::cst::event::EventMark
  where
    Ctx::Emitter: crate::emitter::CstEmitter<'inp, L, Lang>,
  {
    self.inp.cst_mark()
  }

  /// Retro-opens a node of `kind` at `mark` —
  /// [`CstEmitter::cst_start_at`](crate::emitter::CstEmitter::cst_start_at), forwarded.
  #[inline(always)]
  pub fn cst_start_at(&mut self, mark: crate::cst::event::EventMark, kind: u16)
  where
    Ctx::Emitter: crate::emitter::CstEmitter<'inp, L, Lang>,
  {
    self.inp.cst_start_at(mark, kind);
  }

  /// Returns the state of the lexer.
  #[inline(always)]
  pub const fn state(&self) -> &L::State {
    self.inp.state()
  }

  /// Returns a mutable reference to the state of the lexer.
  ///
  /// # State replacement re-keys the input's offset-dependent facts
  ///
  /// Delegates to [`InputRef::state_mut`](crate::InputRef::state_mut): taking the state
  /// mutably eagerly re-keys every offset-dependent fact the input tracks — the token cache
  /// is cleared, the poison boundary is dropped, and the lexer-error dedup watermark is
  /// reset to the current committed cursor. The re-key is itself transactional, not
  /// invalidating: checkpoints and savepoints saved before the state mutation remain
  /// valid, and restoring one afterwards simply undoes the surgery — the prior regime,
  /// boundary, watermark, and position all return.
  ///
  /// State surgery with outstanding speculative diagnostics may re-report the re-lexed
  /// region under the new regime, so callers should complete or roll back speculation
  /// before replacing state.
  #[inline(always)]
  pub fn state_mut(&mut self) -> &mut L::State {
    self.inp.state_mut()
  }

  /// Returns the source slice covering the output being parsed.
  #[inline(always)]
  pub fn slice(&self) -> Option<<L::Source as Source<L::Offset>>::Slice<'inp>> {
    self.inp.slice_since(&self.start)
  }
}
