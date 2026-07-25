use generic_arraydeque::typenum::Unsigned;

use super::*;

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  /// Folds over the input tokens using the provided accumulator function.
  pub fn fold<O, Pred, Init, Op>(
    &mut self,
    mut pred: Pred,
    init: Init,
    mut op: Op,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Init: FnOnce() -> O,
    Op: FnMut(O, Spanned<L::Token, L::Span>) -> O,
    Pred: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    let mut output = init();

    loop {
      match self.try_expect(&mut pred)? {
        Some(spanned) => {
          output = op(output, spanned);
        }
        None => return Ok(output),
      }
    }
  }

  /// Folds at most n tokens over the input using the provided accumulator function.
  pub fn foldn<O, Init, Op>(
    &mut self,
    init: Init,
    mut op: Op,
    num: usize,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Init: FnOnce() -> O,
    Op: FnMut(O, Spanned<L::Token, L::Span>) -> O,
  {
    let mut output = init();

    let mut n = 0;

    loop {
      if n >= num {
        return Ok(output);
      }

      match self.next()? {
        Some(spanned) => {
          output = op(output, spanned);
          n += 1;
        }
        None => return Ok(output),
      }
    }
  }

  /// Right-folds over the input tokens using the provided accumulator function.
  ///
  /// The maximum number of tokens folded is determined by the capacity of the specified `W`.
  ///
  /// See also [`foldrn`](Self::foldrn).
  pub fn foldr_within<O, W, Pred, Init, Op>(
    &mut self,
    mut pred: Pred,
    init: Init,
    mut op: Op,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Init: FnOnce() -> O,
    Op: FnMut(O, Spanned<L::Token, L::Span>) -> O,
    W: Window,
    Pred: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    let mut output = init();
    let mut buf = GenericArrayDeque::<_, W::CAPACITY>::new();

    loop {
      if buf.len() >= <W::CAPACITY as Unsigned>::USIZE {
        break;
      }

      match self.try_expect(&mut pred)? {
        Some(spanned) => {
          buf.push_back(spanned);
        }
        None => return Ok(output),
      }
    }

    while let Some(spanned) = buf.pop_back() {
      output = op(output, spanned);
    }

    Ok(output)
  }

  /// Right-folds over the input tokens using the provided accumulator function.
  ///
  /// This method folds up to `num` tokens, and this will lead to implicit allocation.
  ///
  /// See also [`foldr_within`](Self::foldr_within).
  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  pub fn foldrn<O, Init, Op>(
    &mut self,
    init: Init,
    mut op: Op,
    num: usize,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Init: FnOnce() -> O,
    Op: FnMut(O, Spanned<L::Token, L::Span>) -> O,
  {
    let mut output = init();
    // `num` is an upper bound the caller may set to `usize::MAX` to mean "unbounded"; reserving
    // it up front would try to allocate that many slots before a single token is read. Let the
    // buffer grow with actual consumption instead.
    let mut buf = std::vec::Vec::new();

    let mut n = 0;
    loop {
      if n >= num {
        break;
      }

      match self.next()? {
        Some(spanned) => {
          buf.push(spanned);
          n += 1;
        }
        None => break,
      }
    }

    while let Some(spanned) = buf.pop() {
      output = op(output, spanned);
    }

    Ok(output)
  }
}
