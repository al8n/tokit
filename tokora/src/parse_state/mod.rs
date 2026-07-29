use super::{input::Cursor, *};

/// The view of a parse a state-carrying combinator callback is handed:
/// [`map_with`](crate::ParseInput::map_with),
/// [`and_then_with`](crate::ParseInput::and_then_with),
/// [`validate_with`](crate::ParseInput::validate_with), and [`fold`](crate::TryParseInput::fold)
/// each build one over the region their sub-parser just consumed and lend it for the call.
///
/// It answers the four questions a callback has about that region — its [`span`](Self::span), its
/// source [`slice`](Self::slice), the lexer [`state`](Self::state) it was read under, and the
/// [`emitter`](Self::emitter) to report against — and nothing else. In particular it does **not**
/// consume tokens: the sub-parser already did, and a callback that could parse more would be a
/// parser, not a callback.
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

  /// Returns the span covering the output being parsed.
  #[inline(always)]
  pub fn span(&self) -> L::Span {
    self.inp.span_since(&self.start)
  }

  /// Returns a mutable reference to an emitter.
  #[inline(always)]
  pub const fn emitter(&mut self) -> &mut Ctx::Emitter {
    self.inp.emitter()
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
