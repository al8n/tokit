use smol_bytes_0_1::{Utf8Bytes, compact, shared};

use super::Slice;

impl Slice<'_> for shared::Bytes {
  type Char = u8;

  type Iter<'a>
    = core::iter::Copied<core::slice::Iter<'a, u8>>
  where
    Self: 'a;

  type PositionedIter<'a>
    = core::iter::Enumerate<Self::Iter<'a>>
  where
    Self: 'a;

  #[inline]
  fn iter<'a>(&'a self) -> Self::Iter<'a>
  where
    Self: 'a,
  {
    <[u8]>::iter(self).copied()
  }

  #[inline]
  fn positioned_iter<'a>(&'a self) -> Self::PositionedIter<'a>
  where
    Self: 'a,
  {
    self.iter().enumerate()
  }

  #[inline]
  fn len(&self) -> usize {
    <shared::Bytes>::len(self)
  }
}

impl Slice<'_> for compact::Bytes {
  type Char = u8;

  type Iter<'a>
    = core::iter::Copied<core::slice::Iter<'a, u8>>
  where
    Self: 'a;

  type PositionedIter<'a>
    = core::iter::Enumerate<Self::Iter<'a>>
  where
    Self: 'a;

  #[inline]
  fn iter<'a>(&'a self) -> Self::Iter<'a>
  where
    Self: 'a,
  {
    <[u8]>::iter(self).copied()
  }

  #[inline]
  fn positioned_iter<'a>(&'a self) -> Self::PositionedIter<'a>
  where
    Self: 'a,
  {
    self.iter().enumerate()
  }

  #[inline]
  fn len(&self) -> usize {
    <compact::Bytes>::len(self)
  }
}

impl Slice<'_> for Utf8Bytes {
  type Char = char;

  type Iter<'a>
    = core::str::Chars<'a>
  where
    Self: 'a;

  type PositionedIter<'a>
    = core::str::CharIndices<'a>
  where
    Self: 'a;

  #[inline]
  fn iter<'a>(&'a self) -> Self::Iter<'a>
  where
    Self: 'a,
  {
    self.chars()
  }

  #[inline]
  fn positioned_iter<'a>(&'a self) -> Self::PositionedIter<'a>
  where
    Self: 'a,
  {
    self.char_indices()
  }

  #[inline]
  fn len(&self) -> usize {
    <Utf8Bytes>::len(self)
  }
}

impl Slice<'_> for compact::Utf8Bytes {
  type Char = char;

  type Iter<'a>
    = core::str::Chars<'a>
  where
    Self: 'a;

  type PositionedIter<'a>
    = core::str::CharIndices<'a>
  where
    Self: 'a;

  #[inline]
  fn iter<'a>(&'a self) -> Self::Iter<'a>
  where
    Self: 'a,
  {
    self.chars()
  }

  #[inline]
  fn positioned_iter<'a>(&'a self) -> Self::PositionedIter<'a>
  where
    Self: 'a,
  {
    self.char_indices()
  }

  #[inline]
  fn len(&self) -> usize {
    <compact::Utf8Bytes>::len(self)
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
