use core::marker::PhantomData;

use crate::span::{SimpleSpan, Span};

/// An error indicating too many elements were found.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct FullContainer<S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  nums: usize,
  limit: usize,
  _lang: PhantomData<Lang>,
}

impl<S> FullContainer<S> {
  /// Creates a new `FullContainer` error.
  #[inline(always)]
  pub const fn new(span: S, nums: usize, maximum: usize) -> Self {
    Self::of(span, nums, maximum)
  }
}

impl<S, Lang: ?Sized> FullContainer<S, Lang> {
  /// Creates a new `FullContainer` error for the given language.
  #[inline(always)]
  pub const fn of(span: S, nums: usize, maximum: usize) -> Self {
    Self::new_in(span, nums, maximum)
  }
}

impl<S, Lang: ?Sized> FullContainer<S, Lang> {
  const fn new_in(span: S, nums: usize, limit: usize) -> Self {
    Self {
      span,
      nums,
      limit,
      _lang: PhantomData,
    }
  }

  /// Returns the span associated with this error.
  #[inline(always)]
  pub const fn span(&self) -> &S {
    &self.span
  }

  /// Returns the number of elements found: the elements the container held when it refused a
  /// push, **plus the refused one**. For any container that refuses only when full, that is
  /// [`capacity()`](Self::capacity)` + 1`.
  ///
  /// The diagnostic is emitted once per construct, at the first refusal — a container that
  /// refuses one push refuses every later one, so a per-dropped-element count would climb past
  /// the capacity it names.
  ///
  /// A custom container that refuses a push *below* its advertised
  /// [`Container::max_capacity`](crate::container::Container::max_capacity) yields a `nums`
  /// that does not exceed `capacity()`. That reports what actually happened and is left as is.
  #[inline(always)]
  pub const fn nums(&self) -> usize {
    self.nums
  }

  /// Bumps the span by the given offset.
  #[inline(always)]
  pub fn bump(&mut self, by: &S::Offset) -> &mut Self
  where
    S: Span,
  {
    self.span.bump(by);
    self
  }

  /// Returns the maximum capacity of the container.
  #[inline(always)]
  pub const fn capacity(&self) -> usize {
    self.limit
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for () {
  #[inline(always)]
  fn from(_: FullContainer<S, Lang>) -> Self {}
}

impl<S, Lang: ?Sized> core::fmt::Display for FullContainer<S, Lang> {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(
      f,
      "found {} elements, which exceeds the maximum capacity of {}",
      self.nums, self.limit
    )
  }
}

impl<S, Lang: ?Sized> core::error::Error for FullContainer<S, Lang>
where
  S: core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
