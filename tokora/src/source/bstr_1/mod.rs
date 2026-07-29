use bstr_1::BStr;

use super::Source;

impl Source<usize> for BStr {
  // Unsized: `&BStr` addresses the bytes, so an unequal reference proves an unequal source.
  // The `&'data BStr` impl below is Sized and deliberately keeps the conservative default.
  const REFERENT_IS_BYTES: bool = true;

  type Slice<'a>
    = &'a BStr
  where
    Self: 'a;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <[u8]>::is_empty(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    <[u8]>::len(self)
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    self
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: core::ops::RangeBounds<usize>,
  {
    <[u8]>::slice(self, range).map(BStr::new)
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    <[u8]>::is_boundary(self, index)
  }
}

impl<'data> Source<usize> for &'data BStr {
  type Slice<'source>
    = &'data BStr
  where
    Self: 'source;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <BStr as Source<usize>>::is_empty(*self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    <BStr as Source<usize>>::len(*self)
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    <BStr as Source<usize>>::as_slice(*self)
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: core::ops::RangeBounds<usize>,
  {
    <BStr as Source<usize>>::slice(*self, range)
  }

  #[inline(always)]
  fn find_boundary(&self, index: usize) -> usize {
    <BStr as Source<usize>>::find_boundary(*self, index)
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    <BStr as Source<usize>>::is_boundary(*self, index)
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
