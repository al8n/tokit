use core::marker::PhantomData;

use crate::{TryParseInput, span::Span as _, try_parse_input::ParseAttempt};

use super::*;

pub use fold_while::*;

mod fold_while;

#[cfg(any(feature = "alloc", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
pub use rfold::*;

#[cfg(any(feature = "alloc", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
mod rfold;

#[cfg(any(feature = "alloc", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
pub use rfold_while::*;

#[cfg(any(feature = "alloc", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
mod rfold_while;

/// A fold parser combinator.
#[derive(Debug, Clone)]
pub struct Fold<P, Init, Acc, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  init: Init,
  acc: Acc,
  _output: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, Init, Acc, O, L, Ctx, Lang: ?Sized, Cmpl> Fold<P, Init, Acc, L, O, Ctx, Lang, Cmpl> {
  /// Creates a new fold parser combinator.
  pub(crate) fn new(parser: P, init: Init, acc: Acc) -> Self {
    Self {
      parser,
      init,
      acc,
      _output: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, Init, Acc, O, L, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Fold<P, Init, Acc, L, O, Ctx, Lang, Cmpl>
where
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Init: FnMut() -> O,
  Acc: FnMut(O, O) -> O,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
  // The absence-exit gate surfaces a terminal scanner stop as this end-of-input error.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut output = (self.init)();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exit below: comparing the live latch against it
    // keeps that witness attempt-relative. One offset clone per fold, off the per-element path.
    let latch = inp.latch_snapshot();
    while let ParseAttempt::Accept(value) = self.parser.try_parse_input(inp)? {
      output = (self.acc)(output, value);

      // A cycle that consumed nothing re-sees the same input and would accept forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill moves that across skipped trivia without consuming, reading a zero-width element as
      // false progress. `<=`, not `==`: the watermark cannot regress within a cycle, so anything not
      // strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break;
      }
      committed = new_committed;
    }
    // Both ways out of the loop — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements". An element's own lookahead can latch a terminal scanner
    // stop and still hand it back `Ok` with a short window, so that conclusion may rest on a
    // truncated view. Attempt-relative, so an inherited boundary is not mis-charged here.
    if inp.latched_during_attempt(&latch) {
      return Err(
        crate::error::UnexpectedEot::eot_of(inp.span().end())
          .into_terminal()
          .into(),
      );
    }
    Ok(output)
  }
}

/// A fold parser combinator that accepts a fallible accumulator.
#[derive(Debug, Clone)]
pub struct TryFold<P, Init, Acc, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  init: Init,
  acc: Acc,
  _output: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, Init, Acc, O, L, Ctx, Lang: ?Sized, Cmpl> TryFold<P, Init, Acc, L, O, Ctx, Lang, Cmpl> {
  /// Creates a new fold parser combinator.
  pub(crate) fn new(parser: P, init: Init, acc: Acc) -> Self {
    Self {
      parser,
      init,
      acc,
      _output: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, Init, Acc, O, L, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for TryFold<P, Init, Acc, L, O, Ctx, Lang, Cmpl>
where
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Init: FnMut() -> O,
  Acc: FnMut(O, O) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
  // The absence-exit gate surfaces a terminal scanner stop as this end-of-input error.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut output = (self.init)();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exit below: comparing the live latch against it
    // keeps that witness attempt-relative. One offset clone per fold, off the per-element path.
    let latch = inp.latch_snapshot();
    while let ParseAttempt::Accept(value) = self.parser.try_parse_input(inp)? {
      output = (self.acc)(output, value)?;

      // A cycle that consumed nothing re-sees the same input and would accept forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill moves that across skipped trivia without consuming, reading a zero-width element as
      // false progress. `<=`, not `==`: the watermark cannot regress within a cycle, so anything not
      // strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break;
      }
      committed = new_committed;
    }
    // Both ways out of the loop — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements". An element's own lookahead can latch a terminal scanner
    // stop and still hand it back `Ok` with a short window, so that conclusion may rest on a
    // truncated view. Attempt-relative, so an inherited boundary is not mis-charged here.
    if inp.latched_during_attempt(&latch) {
      return Err(
        crate::error::UnexpectedEot::eot_of(inp.span().end())
          .into_terminal()
          .into(),
      );
    }
    Ok(output)
  }
}

/// A fold parser combinator that accepts a fallible accumulator with access to parsing state.
#[derive(Debug, Clone)]
pub struct TryFoldWith<P, Init, Acc, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  init: Init,
  acc: Acc,
  _output: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, Init, Acc, O, L, Ctx, Lang: ?Sized, Cmpl> TryFoldWith<P, Init, Acc, L, O, Ctx, Lang, Cmpl> {
  /// Creates a new fold parser combinator.
  pub(crate) fn new(parser: P, init: Init, acc: Acc) -> Self {
    Self {
      parser,
      init,
      acc,
      _output: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, Init, Acc, O, L, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for TryFoldWith<P, Init, Acc, L, O, Ctx, Lang, Cmpl>
where
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Init: FnMut() -> O,
  Acc: FnMut(
    O,
    O,
    ParseState<'_, 'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
  // The absence-exit gate surfaces a terminal scanner stop as this end-of-input error.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<crate::error::UnexpectedEot<L::Offset, Lang>>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut output = (self.init)();
    // The terminal-latch baseline for the absence exit below: comparing the live latch against it
    // keeps that witness attempt-relative. One offset clone per fold, off the per-element path.
    let latch = inp.latch_snapshot();
    loop {
      let cursor = inp.cursor().clone();
      let entry_committed = inp.span().end();
      match self.parser.try_parse_input(inp)? {
        ParseAttempt::Accept(value) => {
          // The element already ran above; measure progress by committed consumption against the
          // pre-call watermark, before `cursor` is moved into `ParseState` below. Committed
          // consumption (`span().end()`), never the cache-front cursor — a lookahead fill moves that
          // across skipped trivia without consuming, reading a zero-width element as false progress.
          let advanced = inp.span().end() > entry_committed;
          output = (self.acc)(output, value, ParseState::new(inp, cursor))?;

          // A cycle that consumed nothing re-sees the same input and would accept forever; no
          // progress means no more elements — stop, same as a decline.
          if !advanced {
            break;
          }
        }
        ParseAttempt::Decline => break,
      }
    }
    // Both ways out of the loop — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements". An element's own lookahead can latch a terminal scanner
    // stop and still hand it back `Ok` with a short window, so that conclusion may rest on a
    // truncated view. Attempt-relative, so an inherited boundary is not mis-charged here.
    if inp.latched_during_attempt(&latch) {
      return Err(
        crate::error::UnexpectedEot::eot_of(inp.span().end())
          .into_terminal()
          .into(),
      );
    }
    Ok(output)
  }
}
