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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " See also [`foldrn`](Self::foldrn)."
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " See also `foldrn`."
  )]
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
        // `break`, never `return`: the reverse drain below is this fold's epilogue, and every
        // `Ok` exit runs it exactly once. `fold`/`foldn` return here because they have no
        // epilogue; copying their arm into a buffering fold consumed the accepted run into
        // `buf` and then dropped it with the buffer, returning the untouched initializer on
        // the `Ok` path. The sibling `foldrn` already breaks.
        None => break,
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

#[cfg(test)]
mod fold_census {
  //! BUFFERED_FOLD_CENSUS — a buffering fold leaves its loop by `break`, never by `return`.
  //!
  //! Two of the four folds here buffer their input and drain it in reverse on a single shared
  //! tail; that drain is their epilogue, and every `Ok` exit must run it exactly once. The
  //! other two accumulate in place and have no epilogue, so their loops end with
  //! `return Ok(output)`. Transplanting the epilogue-free arm into a buffering fold is what
  //! consumed an accepted run into the buffer and then dropped it, returning the untouched
  //! initializer on the `Ok` path. This census pins the two shapes apart by count so the
  //! transplant cannot recur.
  //!
  //! It lives here rather than in `census_tests.rs` so the fold family's rail travels with the
  //! file it guards.

  /// Counts occurrences of `needle` on lines that are not whole-line comments, so prose
  /// mentions of a counted form do not skew the tally.
  fn code_matches(src: &str, needle: &str) -> usize {
    src
      .lines()
      .filter(|line| !line.trim_start().starts_with("//"))
      .map(|line| line.matches(needle).count())
      .sum()
  }

  /// The four fold methods, i.e. this file with the census itself cut off the end — the
  /// needles below appear verbatim in the census's own source and would otherwise be counted.
  fn fold_methods() -> &'static str {
    let src = include_str!("fold.rs");
    let (methods, census) = src
      .split_once("#[cfg(test)]")
      .expect("the census marker must be present in its own source");
    assert!(
      census.contains("mod fold_census"),
      "the split must cut at the census module, not at some other cfg(test) item"
    );
    methods
  }

  #[test]
  fn buffering_folds_break_and_unbuffered_folds_return() {
    let src = fold_methods();

    // `fold`'s exhaustion arm, and `foldn`'s two (capacity reached, input exhausted). These
    // three accumulate in place, so returning straight out of the loop is correct.
    assert_eq!(
      code_matches(src, "return Ok(output)"),
      3,
      "only the two unbuffered folds may return out of their loop: `fold` once and `foldn` \
       twice. A fourth means a buffering fold acquired an exit that jumps over its drain"
    );

    // `foldr_within` and `foldrn` each leave their loop two ways — capacity reached (`break;`)
    // and input exhausted (`None => break,`) — and both converge on the one shared drain.
    assert_eq!(
      code_matches(src, "=> break,"),
      2,
      "each buffering fold ends its input-exhausted arm with `break`, so its drain runs"
    );
    assert_eq!(
      code_matches(src, "break;"),
      2,
      "each buffering fold ends its capacity arm with `break`, so its drain runs"
    );
  }
}
