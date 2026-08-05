use core::marker::PhantomData;

use crate::{
  TryParseInput, parse_state::ParseState, span::Span as _, try_parse_input::ParseAttempt,
};

use super::*;
// The absence chokepoint the `many` drivers route their "no more elements" exits through. The folds
// are the same shape of driver and take the same gate, rather than each spelling a witness pair of
// its own — `many::absence_after_element` is where the reasoning for both witnesses lives, and
// `many`'s `GATE_CENSUS` requires every guard-bearing source, these four included, to hand its
// absence exits to it.
use super::many::absence_after_element;

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
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();
    // The trip baseline of the LAST element attempt, carried out by whichever break concluded
    // absence — see `many::absence_after_element` for what the gate below does with it. Taking it
    // per element is why this is a `loop` rather than the `while let` it used to be: the condition
    // of a `while let` runs the element attempt, leaving nowhere to snapshot the counter before it.
    let elem_trips = loop {
      // The descent witness's baseline, taken once per ELEMENT — the attempt the absence gate after
      // the loop judges. `many::file_element_failure` says why it belongs here and not out beside
      // `latch`.
      let trips = inp.trip_snapshot();
      let value = match self.parser.try_parse_input(inp)? {
        ParseAttempt::Accept(value) => value,
        ParseAttempt::Decline => break trips,
      };
      output = (self.acc)(output, value);

      // A cycle that consumed nothing re-sees the same input and would accept forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill moves that across skipped trivia without consuming, reading a zero-width element as
      // false progress. `<=`, not `==`: the watermark cannot regress within a cycle, so anything not
      // strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break trips;
      }
      committed = new_committed;
    };
    // Both ways out of the loop — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements", on the strength of what the last element attempt did.
    // That attempt can hit either never-recoverable stop and still hand back `Ok`: a terminal
    // scanner stop its own lookahead latched, leaving a short window, or a descent budget trip it
    // caught itself. `many::absence_after_element` holds both and says why each baseline is the
    // granularity it is.
    absence_after_element(inp, &latch, scans, elem_trips)?;
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
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();
    // The trip baseline of the LAST element attempt, carried out by whichever break concluded
    // absence. Taking it per element is why this is a `loop` rather than the `while let` it used to
    // be — see [`Fold`]'s body above.
    let elem_trips = loop {
      // The descent witness's baseline, taken once per ELEMENT — the attempt the absence gate after
      // the loop judges. See `many::file_element_failure`.
      let trips = inp.trip_snapshot();
      let value = match self.parser.try_parse_input(inp)? {
        ParseAttempt::Accept(value) => value,
        ParseAttempt::Decline => break trips,
      };
      output = (self.acc)(output, value)?;

      // A cycle that consumed nothing re-sees the same input and would accept forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill moves that across skipped trivia without consuming, reading a zero-width element as
      // false progress. `<=`, not `==`: the watermark cannot regress within a cycle, so anything not
      // strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break trips;
      }
      committed = new_committed;
    };
    // Both ways out of the loop — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements", on the strength of what the last element attempt did.
    // A terminal scanner stop its own lookahead latched, and a descent budget trip it caught itself,
    // both leave it returning `Ok`; `many::absence_after_element` holds both witnesses.
    absence_after_element(inp, &latch, scans, elem_trips)?;
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
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();
    // The trip baseline of the LAST element attempt, carried out by whichever break concluded
    // absence. See `many::absence_after_element`.
    let elem_trips = loop {
      // The descent witness's baseline, taken once per ELEMENT — the attempt the absence gate after
      // the loop judges. See `many::file_element_failure`.
      let trips = inp.trip_snapshot();
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
            break trips;
          }
        }
        ParseAttempt::Decline => break trips,
      }
    };
    // Both ways out of the loop — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements", on the strength of what the last element attempt did.
    // A terminal scanner stop its own lookahead latched, and a descent budget trip it caught itself,
    // both leave it returning `Ok`; `many::absence_after_element` holds both witnesses.
    absence_after_element(inp, &latch, scans, elem_trips)?;
    Ok(output)
  }
}
