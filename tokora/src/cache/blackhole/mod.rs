use super::{
  Cache, CachedTokenOf, CachedTokenRefOf, Checkpoint, GenericArrayDeque, Lexer,
  MaybeRefCachedTokenOf,
};

macro_rules! blackhole {
  ($ty:ty) => {
    impl<'a, L, Lang: ?Sized> Cache<'a, L, Lang> for $ty
    where
      L: Lexer<'a> + 'a,
    {
      type Options = ();

      // Retains nothing: every push refuses, so the parked-front machinery stays live.
      const RETAINS_FRONT: bool = false;

      #[inline(always)]
      fn new() -> Self {
        Default::default()
      }

      #[inline(always)]
      fn with_options(_: Self::Options) -> Self
      where
        Self: Sized,
      {
        Default::default()
      }

      #[inline(always)]
      fn len(&self) -> usize {
        0
      }

      #[inline(always)]
      fn remaining(&self) -> usize {
        0
      }

      #[inline(always)]
      fn rewind(&mut self, _: &Checkpoint<'a, '_, L>) {}

      #[inline(always)]
      fn push_front(
        &mut self,
        tok: CachedTokenOf<'a, L>,
      ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
        Err(tok)
      }

      #[inline(always)]
      fn push_back(
        &mut self,
        tok: CachedTokenOf<'a, L>,
      ) -> Result<CachedTokenRefOf<'_, 'a, L>, CachedTokenOf<'a, L>> {
        Err(tok)
      }

      #[inline(always)]
      fn pop_front(&mut self) -> Option<CachedTokenOf<'a, L>> {
        None
      }

      #[inline(always)]
      fn pop_back(&mut self) -> Option<CachedTokenOf<'a, L>> {
        None
      }

      #[inline(always)]
      fn clear(&mut self) {}

      #[inline(always)]
      fn peek<'p, W>(
        &'p self,
        _: &mut GenericArrayDeque<MaybeRefCachedTokenOf<'p, 'a, L>, W::CAPACITY>,
      ) where
        W: crate::Window,
      {
      }

      #[inline(always)]
      fn front(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
        None
      }

      #[inline(always)]
      fn back(&self) -> Option<CachedTokenRefOf<'_, 'a, L>> {
        None
      }
    }
  };
}

blackhole!(());

#[cfg(test)]
mod tests;
