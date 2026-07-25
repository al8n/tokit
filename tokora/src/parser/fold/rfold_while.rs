use super::*;

/// A parser that repeatedly applies another parser, buffers outputs, and folds them in reverse
/// order while a condition is met.
#[derive(Debug, Clone, Copy)]
pub struct RFoldWhile<P, Condition, Init, Acc, L, O, W, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  condition: Condition,
  init: Init,
  acc: Acc,
  _output: PhantomData<O>,
  _window: PhantomData<W>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, Condition, Init, Acc, O, W, L, Ctx, Lang: ?Sized, Cmpl>
  RFoldWhile<P, Condition, Init, Acc, L, O, W, Ctx, Lang, Cmpl>
{
  /// Creates a new `RFoldWhile` parser combinator.
  #[inline(always)]
  pub(crate) fn new(parser: P, condition: Condition, init: Init, acc: Acc) -> Self {
    Self {
      parser,
      condition,
      init,
      acc,
      _output: PhantomData,
      _window: PhantomData,
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
impl<'inp, P, Condition, Init, Acc, O, W, L, Ctx, Lang> ParseInput<'inp, L, O, Ctx, Lang>
  for RFoldWhile<P, Condition, Init, Acc, L, O, W, Ctx, Lang>
where
  P: ParseInput<'inp, L, O, Ctx, Lang>,
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
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut buf = std::vec::Vec::new();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence check after the loop: comparing the live latch
    // against it keeps that witness attempt-relative. One offset clone per fold.
    let latch = inp.latch_snapshot();

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
          buf.push(self.parser.parse_input(inp)?);
        }
      }

      // A `Continue` cycle that consumed nothing re-sees the same lookahead and would buffer a
      // phantom output forever. The progress metric is committed consumption (`span().end()`), never
      // the cache-front cursor — a lookahead fill moves that across skipped trivia without consuming,
      // reading a zero-width element as false progress. `<=`, not `==`: the watermark cannot regress
      // within a cycle, so anything not strictly ahead is a stall.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break;
      }
      committed = new_committed;
    }

    // Both ways out of the loop — the condition's `Stop` and the no-progress stall — conclude
    // *absence*: "no more elements". The element's own lookahead can latch a terminal scanner stop
    // and still return `Ok` with a short window, leaving the pre-trip tokens cached, so a stall may
    // rest on a truncated view; and the next decision window is then served whole from that cache,
    // with no terminal flag for the gate above to see, so a `Stop` on it may too. Checked here, ahead
    // of the buffered reverse fold — the loop's only successor and the single success path, so
    // neither break can bypass it. Attempt-relative, so an inherited boundary is not mis-charged
    // here.
    if inp.latched_during_attempt(&latch) {
      return Err(
        crate::error::UnexpectedEot::eot_of(inp.span().end())
          .into_terminal()
          .into(),
      );
    }

    let output = buf.into_iter().rfold((self.init)(), &mut self.acc);
    Ok(output)
  }
}
