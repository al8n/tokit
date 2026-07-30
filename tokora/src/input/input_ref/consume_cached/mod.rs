use super::*;

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Consumes one token already lexed and waiting at the front of the stream, returning it if
  /// there is one; the cursor is advanced.
  #[inline(always)]
  pub fn consume_cached_one(&mut self) -> Option<Spanned<L::Token, L::Span>> {
    let tok = self.take_front()?;
    let (tok, extras): (Spanned<L::Token, L::Span>, _) = tok.into_components();
    self.commit_token(tok.data(), tok.span_ref(), extras);
    Some(tok)
  }

  /// Consumes tokens already lexed and waiting at the front of the stream until the predicate
  /// returns `true`.
  ///
  /// Advances the cursor to the end of the last consumed token.
  /// Returns the last consumed token.
  #[inline(always)]
  pub fn consume_cached_to<F>(&mut self, mut f: F) -> Option<Spanned<L::Token, L::Span>>
  where
    F: FnMut(CachedTokenRefOf<'_, 'inp, L>) -> bool,
  {
    let mut last = None;
    // pop from cache if not matching
    while let Some(tok) = self.take_front_if(|t| !f(t)) {
      let (tok, state) = tok.into_components();
      self.commit_token(tok.data(), tok.span_ref(), state);
      last = Some(tok);
    }

    last
  }

  /// Consumes tokens already lexed and waiting at the front of the stream while the predicate
  /// returns `true`.
  ///
  /// Advances the cursor to the end of the last consumed token.
  /// Returns the last consumed token.
  #[inline(always)]
  pub fn consume_cached_while<F>(&mut self, mut f: F) -> Option<Spanned<L::Token, L::Span>>
  where
    F: FnMut(CachedTokenRefOf<'_, 'inp, L>) -> bool,
  {
    self.consume_cached_to(|t| !f(t))
  }

  /// Consumes every token already lexed and waiting at the front of the stream — a parked one
  /// included.
  ///
  /// Advances the cursor to the end of the last of them.
  /// Returns the last consumed token.
  ///
  /// Drains **per token** through [`consume_cached_to`](Self::consume_cached_to) (with a
  /// never-matching predicate), so every retained token — not only the last — settles through
  /// the one commit primitive. The observable result is unchanged: the front empties, the
  /// cursor lands at the end of the last retained token with its state, and the last token is
  /// returned; but each token in the run commits individually, exactly as it would have had
  /// the caller consumed them one by one.
  #[inline(always)]
  pub fn consume_all_cached(&mut self) -> Option<Spanned<L::Token, L::Span>> {
    self.consume_cached_to(|_| false)
  }
}

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod tests;
