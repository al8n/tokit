use super::*;

use crate::span::Span as _;

/// A fold parser that accumulates results while a condition is met, with a fallible accumulator.
///
/// # Completeness (0.3.0): Complete-only — the mode wall
///
/// The `*_while` family is decision-window class: its [`Decision`](crate::Decision) peeks
/// a `W`-token window, and a non-final [`Partial`](crate::Partial) frontier can silently
/// truncate that window, which the condition would misread as "construct ended". The
/// parser-trait impls are therefore pinned at [`Complete`](crate::Complete) in both
/// positions, and driving one at `Partial` fails to compile:
///
/// ```compile_fail,E0277
/// use generic_arraydeque::typenum::U1;
/// use tokora::{InputRef, Lexer, ParseContext, ParseInput, Partial, parser::FoldWhile};
///
/// fn wall<'inp, L, Ctx, P, C, I, A>(
///   parser: &mut FoldWhile<P, C, I, A, u8, U1, L, Ctx>,
///   inp: &mut InputRef<'inp, '_, L, Ctx, (), Partial>,
/// ) where
///   L: Lexer<'inp>,
///   Ctx: ParseContext<'inp, L>,
/// {
///   // ERROR: `FoldWhile<…>` implements `ParseInput<…>` only at `Complete`.
///   let _ = ParseInput::parse_input(parser, inp);
/// }
/// ```
#[derive(Clone, Debug, Copy)]
pub struct FoldWhile<F, Condition, Init, Acc, O, W, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  f: F,
  condition: Condition,
  init: Init,
  acc: Acc,
  _m: PhantomData<O>,
  _cap: PhantomData<W>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, Condition, Init, Acc, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  FoldWhile<F, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>
{
  /// Creates a new `FoldWhile` parser with the given container.
  #[inline(always)]
  pub(crate) const fn new(f: F, condition: Condition, init: Init, acc: Acc) -> Self {
    Self {
      f,
      condition,
      init,
      acc,
      _m: PhantomData,
      _cap: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

// STAYS COMPLETE-ONLY (0.3.0 — the decision-window class): the `Decision` peeks a
// `W`-window, and at a non-final Partial frontier the peek fill silently serves a SHORT
// window (the peek contract: short at the frontier, never an error). The condition would
// read that truncation as "construct ended" and return `Ok` early — breaking chunked
// equivalence with no error on any channel. Generalizing needs the deferred
// frontier-window rule (full-or-incomplete decision windows); until then the impls stay
// pinned at `Complete` in both positions, so a Partial drive is a compile-time wall.
impl<'inp, F, Condition, Init, Acc, O, W, L, Ctx, Lang> ParseInput<'inp, L, O, Ctx, Lang>
  for FoldWhile<F, Condition, Init, Acc, O, W, L, Ctx, Lang>
where
  F: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Init: FnMut() -> O,
  Acc: FnMut(O, O) -> O,
  W: Window,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut output = (self.init)();
    loop {
      // A short decision window can be a genuine end of input (Stop), but one truncated by a
      // terminal scanner stop is not: surface the committed end-of-input error rather than
      // reading the stop as a legitimate end of the fold.
      let end = inp.span().end();
      let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
      if terminal {
        return Err(
          crate::error::UnexpectedEot::eot_of(end)
            .into_terminal()
            .into(),
        );
      }
      match self.condition.decide(peeked, emitter)? {
        Action::Stop => break,
        Action::Continue => {
          output = (self.acc)(output, self.f.parse_input(inp)?);
        }
      }
    }

    Ok(output)
  }
}

/// A fold parser that accumulates results while a condition is met, with a fallible accumulator.
#[derive(Clone, Debug, Copy)]
pub struct TryFoldWhile<F, Condition, Init, Acc, O, W, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  f: F,
  condition: Condition,
  init: Init,
  acc: Acc,
  _m: PhantomData<O>,
  _cap: PhantomData<W>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, Condition, Init, Acc, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  TryFoldWhile<F, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>
{
  /// Creates a new `FoldWhile` parser with the given container.
  #[inline(always)]
  pub(crate) const fn new(f: F, condition: Condition, init: Init, acc: Acc) -> Self {
    Self {
      f,
      condition,
      init,
      acc,
      _m: PhantomData,
      _cap: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, F, Condition, Init, Acc, O, W, L, Ctx, Lang> ParseInput<'inp, L, O, Ctx, Lang>
  for TryFoldWhile<F, Condition, Init, Acc, O, W, L, Ctx, Lang>
where
  F: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Init: FnMut() -> O,
  Acc: FnMut(O, O) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  W: Window,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut output = (self.init)();
    loop {
      // A short decision window can be a genuine end of input (Stop), but one truncated by a
      // terminal scanner stop is not: surface the committed end-of-input error rather than
      // reading the stop as a legitimate end of the fold.
      let end = inp.span().end();
      let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
      if terminal {
        return Err(
          crate::error::UnexpectedEot::eot_of(end)
            .into_terminal()
            .into(),
        );
      }
      match self.condition.decide(peeked, emitter)? {
        Action::Stop => break,
        Action::Continue => {
          let new = self.f.parse_input(inp)?;
          output = (self.acc)(output, new)?;
        }
      }
    }

    Ok(output)
  }
}

/// A fold parser that accumulates results while a condition is met, with a fallible accumulator
/// and access to parsing state.
#[derive(Clone, Debug, Copy)]
pub struct TryFoldWhileWith<
  F,
  Condition,
  Init,
  Acc,
  O,
  W,
  L,
  Ctx,
  Lang: ?Sized = (),
  Cmpl = Complete,
> {
  f: F,
  condition: Condition,
  init: Init,
  acc: Acc,
  _m: PhantomData<O>,
  _cap: PhantomData<W>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<F, Condition, Init, Acc, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  TryFoldWhileWith<F, Condition, Init, Acc, O, W, L, Ctx, Lang, Cmpl>
{
  /// Creates a new `FoldWhile` parser with the given container.
  #[inline(always)]
  pub(crate) const fn new(f: F, condition: Condition, init: Init, acc: Acc) -> Self {
    Self {
      f,
      condition,
      init,
      acc,
      _m: PhantomData,
      _cap: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, F, Condition, Init, Acc, O, W, L, Ctx, Lang> ParseInput<'inp, L, O, Ctx, Lang>
  for TryFoldWhileWith<F, Condition, Init, Acc, O, W, L, Ctx, Lang>
where
  F: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Init: FnMut() -> O,
  Acc: FnMut(
    O,
    O,
    ParseState<'_, 'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  W: Window,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut output = (self.init)();
    loop {
      // A short decision window can be a genuine end of input (Stop), but one truncated by a
      // terminal scanner stop is not: surface the committed end-of-input error rather than
      // reading the stop as a legitimate end of the fold.
      let end = inp.span().end();
      let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
      if terminal {
        return Err(
          crate::error::UnexpectedEot::eot_of(end)
            .into_terminal()
            .into(),
        );
      }
      match self.condition.decide(peeked, emitter)? {
        Action::Stop => break,
        Action::Continue => {
          let cursor = inp.cursor().clone();
          let new = self.f.parse_input(inp)?;
          let state = ParseState::new(inp, cursor);
          output = (self.acc)(output, new, state)?;
        }
      }
    }

    Ok(output)
  }
}
