use core::ops::RangeBounds;

use smol_bytes_0_1::{Utf8Bytes, compact, shared};

use super::Source;

impl Source<usize> for shared::Bytes {
  type Slice<'a>
    = shared::Bytes
  where
    Self: 'a;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <shared::Bytes>::is_empty(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    self.len()
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    self.clone()
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: RangeBounds<usize>,
  {
    use core::ops::Bound;

    let len = self.len();

    let begin = match range.start_bound() {
      Bound::Included(&n) => n,
      Bound::Excluded(&n) => n.checked_add(1)?,
      Bound::Unbounded => 0,
    };

    let end = match range.end_bound() {
      Bound::Included(&n) => n.checked_add(1)?,
      Bound::Excluded(&n) => n,
      Bound::Unbounded => len,
    };

    if begin > end || end > len {
      return None;
    }

    Some(shared::Bytes::slice(self, begin..end))
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    <[u8]>::is_boundary(self, index)
  }
}

impl Source<usize> for compact::Bytes {
  type Slice<'a>
    = compact::Bytes
  where
    Self: 'a;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <compact::Bytes>::is_empty(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    self.len()
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    self.clone()
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: RangeBounds<usize>,
  {
    use core::ops::Bound;

    let len = self.len();

    let begin = match range.start_bound() {
      Bound::Included(&n) => n,
      Bound::Excluded(&n) => n.checked_add(1)?,
      Bound::Unbounded => 0,
    };

    let end = match range.end_bound() {
      Bound::Included(&n) => n.checked_add(1)?,
      Bound::Excluded(&n) => n,
      Bound::Unbounded => len,
    };

    if begin > end || end > len {
      return None;
    }

    Some(compact::Bytes::slice(self, begin..end))
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    <[u8]>::is_boundary(self, index)
  }
}

impl Source<usize> for Utf8Bytes {
  type Slice<'a>
    = Utf8Bytes
  where
    Self: 'a;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <Utf8Bytes>::is_empty(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    <Utf8Bytes>::len(self)
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    self.clone()
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: RangeBounds<usize>,
  {
    self
      .try_slice((
        range.start_bound().map(|s| *s),
        range.end_bound().map(|s| *s),
      ))
      .ok()
  }

  /// Rounds `index` **down** to the nearest UTF-8 code point boundary.
  ///
  /// For an index **inside** the source the result is a valid slice position.
  /// Indices at or beyond the end are returned unchanged, matching the byte
  /// sources — and such a result is a valid slice position only when it is exactly `len`. The
  /// previous wording claimed the result is "always a valid slice position" *and* that
  /// out-of-range indices come back unchanged, which cannot both be true: `len + 1` comes back
  /// unchanged and is not a slice position.
  ///
  /// Callers that may hold an out-of-range index check it (`is_boundary`, or a bound against
  /// `len`) rather than relying on this to clamp.
  #[inline(always)]
  fn find_boundary(&self, index: usize) -> usize {
    if index >= <Utf8Bytes>::len(self) {
      return index;
    }
    let mut index = index;
    while !self.is_char_boundary(index) {
      index -= 1;
    }
    index
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    self.is_char_boundary(index)
  }
}

impl Source<usize> for compact::Utf8Bytes {
  type Slice<'a>
    = compact::Utf8Bytes
  where
    Self: 'a;

  #[inline(always)]
  fn is_empty(&self) -> bool {
    <compact::Utf8Bytes>::is_empty(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    <compact::Utf8Bytes>::len(self)
  }

  #[inline(always)]
  fn as_slice(&self) -> Self::Slice<'_> {
    self.clone()
  }

  #[inline(always)]
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: RangeBounds<usize>,
  {
    self
      .try_slice((
        range.start_bound().map(|s| *s),
        range.end_bound().map(|s| *s),
      ))
      .ok()
  }

  /// Rounds `index` **down** to the nearest UTF-8 code point boundary.
  ///
  /// For an index **inside** the source the result is a valid slice position.
  /// Indices at or beyond the end are returned unchanged, matching the byte
  /// sources — and such a result is a valid slice position only when it is exactly `len`. The
  /// previous wording claimed the result is "always a valid slice position" *and* that
  /// out-of-range indices come back unchanged, which cannot both be true: `len + 1` comes back
  /// unchanged and is not a slice position.
  ///
  /// Callers that may hold an out-of-range index check it (`is_boundary`, or a bound against
  /// `len`) rather than relying on this to clamp.
  #[inline(always)]
  fn find_boundary(&self, index: usize) -> usize {
    if index >= <compact::Utf8Bytes>::len(self) {
      return index;
    }
    let mut index = index;
    while !self.is_char_boundary(index) {
      index -= 1;
    }
    index
  }

  #[inline(always)]
  fn is_boundary(&self, index: usize) -> bool {
    self.is_char_boundary(index)
  }
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
#[allow(warnings)]
mod tests;
