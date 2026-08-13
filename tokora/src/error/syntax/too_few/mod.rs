use core::marker::PhantomData;

use crate::span::{SimpleSpan, Span};

/// An error indicating too few elements were found.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TooFew<S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  nums: usize,
  limit: usize,
  _lang: PhantomData<Lang>,
}

impl<S> TooFew<S> {
  /// Creates a new `TooFew` error.
  #[inline]
  pub const fn new(span: S, nums: usize, minimum: usize) -> Self {
    Self::of(span, nums, minimum)
  }
}

impl<S, Lang: ?Sized> TooFew<S, Lang> {
  /// Creates a new `TooFew` error for the given language.
  #[inline]
  pub const fn of(span: S, nums: usize, minimum: usize) -> Self {
    Self::new_in(span, nums, minimum)
  }
}

impl<S, Lang: ?Sized> TooFew<S, Lang> {
  const fn new_in(span: S, nums: usize, limit: usize) -> Self {
    Self {
      span,
      nums,
      limit,
      _lang: PhantomData,
    }
  }

  /// Returns the span associated with this error.
  #[inline]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns the span associated with this error.
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns the mutable reference to the span associated with this error.
  #[inline]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Bumps the span by n offsets.
  #[inline]
  pub fn bump(&mut self, by: &S::Offset) -> &mut Self
  where
    S: Span,
  {
    self.span.bump(by);
    self
  }

  /// Returns the number of elements found.
  #[inline]
  pub const fn nums(&self) -> usize {
    self.nums
  }

  /// Returns the limit that was violated.
  #[inline]
  pub const fn limit(&self) -> usize {
    self.limit
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for () {
  #[inline]
  fn from(_: TooFew<S, Lang>) -> Self {}
}

impl<S, Lang: ?Sized> core::fmt::Display for TooFew<S, Lang>
where
  S: core::fmt::Display,
{
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(
      f,
      "too few elements: found {}, but minimum is {} at {}",
      self.nums, self.limit, self.span
    )
  }
}

impl<S, Lang: ?Sized> core::error::Error for TooFew<S, Lang>
where
  S: core::fmt::Display + core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
