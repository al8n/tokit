use mayber::Maybe;

use crate::lexer::Lexer;

use super::{Cache, CachedToken, CachedTokenOf, CachedTokenRefOf, MaybeRefCachedTokenOf};

use generic_arraydeque::{ArrayLength, GenericArrayDeque};

impl<'a, L, Lang: ?Sized, N> Cache<'a, L, Lang>
  for GenericArrayDeque<CachedToken<L::Token, L::State, L::Span>, N>
where
  L: Lexer<'a>,
  N: ArrayLength,
{
  type Options = ();

  // A front push into an empty deque succeeds whenever there is any capacity at all.
  const RETAINS_FRONT: bool = N::USIZE != 0;

  #[inline(always)]
  fn new() -> Self {
    Self::new()
  }

  #[inline(always)]
  fn with_options(_options: ()) -> Self {
    Self::new()
  }

  #[inline(always)]
  fn len(&self) -> usize {
    self.len()
  }

  #[inline(always)]
  fn remaining(&self) -> usize {
    self.remaining_capacity()
  }

  #[inline(always)]
  fn push_front(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    match self.push_front_mut(tok) {
      Ok(tok) => Ok(tok.as_ref()),
      Err(tok) => Err(tok),
    }
  }

  #[inline(always)]
  fn push_back(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    match self.push_back_mut(tok) {
      Ok(tok) => Ok(tok.as_ref()),
      Err(tok) => Err(tok),
    }
  }

  #[inline(always)]
  fn pop_front(&mut self) -> Option<CachedTokenOf<'a, L>> {
    self.pop_front()
  }

  #[inline(always)]
  fn pop_back(&mut self) -> Option<CachedTokenOf<'a, L>> {
    self.pop_back()
  }

  #[inline(always)]
  fn pop_front_if<F>(&mut self, predicate: F) -> Option<CachedTokenOf<'a, L>>
  where
    F: FnOnce(CachedTokenRefOf<'_, 'a, L>) -> bool,
    L: Lexer<'a>,
  {
    self.pop_front_if(|tok| predicate(tok.as_ref()))
  }

  #[inline(always)]
  fn clear(&mut self) {
    self.clear();
  }

  #[inline(always)]
  fn peek<'p, W>(
    &'p self,
    buf: &mut GenericArrayDeque<MaybeRefCachedTokenOf<'p, 'a, L>, W::CAPACITY>,
  ) where
    W: crate::Window,
  {
    let fill = buf.remaining_capacity().min(self.len());
    for tok in self.iter().take(fill) {
      buf.push_back(Maybe::Ref(tok.as_ref()));
    }
  }

  #[inline(always)]
  fn front(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    self.front().map(|tok| tok.as_ref())
  }

  #[inline(always)]
  fn back(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    self.back().map(|tok| tok.as_ref())
  }
}

#[cfg(test)]
mod tests;
