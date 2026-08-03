use super::*;

/// A parser that repeatedly applies another parser, buffers outputs, and folds them in reverse order.
#[derive(Debug, Clone, Copy)]
pub struct RFold<P, Init, Acc, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  init: Init,
  acc: Acc,
  _output: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, Init, Acc, O, L, Ctx, Lang: ?Sized, Cmpl> RFold<P, Init, Acc, L, O, Ctx, Lang, Cmpl> {
  /// Creates a new `RFold` parser combinator.
  #[inline(always)]
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
  for RFold<P, Init, Acc, L, O, Ctx, Lang, Cmpl>
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
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut buf = std::vec::Vec::new();
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
    // be: the condition of a `while let` runs the element attempt, leaving nowhere to snapshot the
    // counter before it. See `many::absence_after_element`.
    let elem_trips = loop {
      // The descent witness's baseline, taken once per ELEMENT — the attempt the absence gate after
      // the loop judges. See `many::file_element_failure`.
      let trips = inp.trip_snapshot();
      let value = match self.parser.try_parse_input(inp)? {
        ParseAttempt::Accept(value) => value,
        ParseAttempt::Decline => break trips,
      };
      buf.push(value);

      // A cycle that consumed nothing re-sees the same input and would buffer a phantom output
      // forever. The progress metric is committed consumption (`span().end()`), never the cache-front
      // cursor — a lookahead fill moves that across skipped trivia without consuming, reading a
      // zero-width element as false progress. `<=`, not `==`: the watermark cannot regress within a
      // cycle, so anything not strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break trips;
      }
      committed = new_committed;
    };

    // Both ways out of the loop — the element declining, and a cycle that committed nothing —
    // conclude *absence*: "no more elements", on the strength of what the last element attempt did.
    // A terminal scanner stop its own lookahead latched, and a descent budget trip it caught itself,
    // both leave it returning `Ok`; `many::absence_after_element` holds both witnesses. Checked here,
    // ahead of the buffered reverse fold — the loop's only successor and the single success path, so
    // neither break can bypass it.
    absence_after_element(inp, &latch, scans, elem_trips)?;

    Ok(buf.into_iter().rfold((self.init)(), &mut self.acc))
  }
}
