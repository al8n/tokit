use core::marker::PhantomData;

use crate::span::{SimpleSpan, Span};

/// An error indicating too many elements were found.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TooMany<S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  nums: usize,
  limit: usize,
  _lang: PhantomData<Lang>,
}

impl<S> TooMany<S> {
  /// Creates a new `TooMany` error.
  #[inline]
  pub const fn new(span: S, nums: usize, maximum: usize) -> Self {
    Self::of(span, nums, maximum)
  }
}

impl<S, Lang: ?Sized> TooMany<S, Lang> {
  /// Creates a new `TooMany` error for the given language.
  #[inline]
  pub const fn of(span: S, nums: usize, maximum: usize) -> Self {
    Self::new_in(span, nums, maximum)
  }
}

impl<S, Lang: ?Sized> TooMany<S, Lang> {
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

  /// Returns the number of elements found: the **first count that exceeds
  /// [`limit()`](Self::limit)**, i.e. `limit() + 1`.
  ///
  /// It is not the construct's final element count. Every driver detects the violation at the
  /// same point — the element that first pushes the count past the limit, reported from the
  /// element-count hook `parser::many`'s admission runs — so `limit() + 1` is the value all of
  /// them report, and it is what makes one input history yield one diagnostic whichever builder
  /// produced it. Under a fail-fast emitter the true final count is unknowable in principle,
  /// since the parse aborts at the first violation; the construct's own output and returned span
  /// carry it when it is wanted.
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

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for () {
  #[inline]
  fn from(_: TooMany<S, Lang>) -> Self {}
}

impl<S, Lang: ?Sized> core::fmt::Display for TooMany<S, Lang>
where
  S: core::fmt::Display,
{
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(
      f,
      "too many elements: found {}, but maximum is {} at {}",
      self.nums, self.limit, self.span
    )
  }
}

impl<S, Lang: ?Sized> core::error::Error for TooMany<S, Lang>
where
  S: core::fmt::Display + core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
