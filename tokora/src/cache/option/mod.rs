use generic_arraydeque::GenericArrayDeque;
use mayber::Maybe;

use crate::lexer::Lexer;

use super::{Cache, CachedToken, CachedTokenOf, CachedTokenRefOf, MaybeRefCachedTokenOf};

impl<'a, L, Lang: ?Sized> Cache<'a, L, Lang> for Option<CachedToken<L::Token, L::State, L::Span>>
where
  L: Lexer<'a>,
{
  type Options = ();

  // Capacity 1: a front push into an empty `Option` always succeeds.
  const RETAINS_FRONT: bool = true;

  #[inline(always)]
  fn new() -> Self {
    None
  }

  #[inline(always)]
  fn with_options(_options: ()) -> Self {
    None
  }

  #[inline(always)]
  fn len(&self) -> usize {
    self.as_ref().map(|_| 1).unwrap_or(0)
  }

  #[inline(always)]
  fn remaining(&self) -> usize {
    if self.is_none() { 1 } else { 0 }
  }

  #[inline(always)]
  fn push_front(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    match self {
      Some(_) => Err(tok),
      None => {
        *self = Some(tok);
        Ok(self.as_ref().expect("there must be a token").as_ref())
      }
    }
  }

  #[inline(always)]
  fn push_back(
    &mut self,
    tok: CachedTokenOf<'a, L>,
  ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
    match self {
      Some(_) => Err(tok),
      None => {
        *self = Some(tok);
        Ok(self.as_ref().expect("there must be a token").as_ref())
      }
    }
  }

  #[inline(always)]
  fn pop_front(&mut self) -> Option<CachedTokenOf<'a, L>> {
    self.take()
  }

  #[inline(always)]
  fn pop_back(&mut self) -> Option<CachedTokenOf<'a, L>> {
    self.take()
  }

  #[inline(always)]
  fn clear(&mut self) {
    *self = None;
  }

  #[inline(always)]
  fn peek<'p, W>(
    &'p self,
    buf: &mut GenericArrayDeque<MaybeRefCachedTokenOf<'p, 'a, L>, W::CAPACITY>,
  ) where
    W: crate::Window,
  {
    if let Some(tok) = self.as_ref() {
      buf.push_back(Maybe::Ref(tok.as_ref()));
    }
  }

  #[inline(always)]
  fn front(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    self.as_ref().map(|tok| tok.as_ref())
  }

  #[inline(always)]
  fn back(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
    self.as_ref().map(|tok| tok.as_ref())
  }
}

#[cfg(test)]
mod tests;
