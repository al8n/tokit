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

  /// Returns the number of elements found: the elements the **construct had parsed** when the
  /// container first refused a push, including the refused one. For any container that refuses
  /// only when full, that is [`capacity()`](Self::capacity)` + 1`.
  ///
  /// The count is the repetition driver's own. It is not read back from the container, so no
  /// property of [`len`](crate::container::Container::len) — that a refused push leaves it
  /// alone, that an accepted one moves it by exactly one, that it never passes
  /// [`max_capacity`](crate::container::Container::max_capacity) — is relied on to make this
  /// number true.
  ///
  /// The diagnostic is recorded once per construct, at the first refusal, and emitted after the
  /// construct's count bounds have been judged. Once per construct is a reporting policy: a
  /// per-dropped-element count would climb past the capacity it names, and nothing here predicts
  /// what a later push does.
  ///
  /// A custom container that refuses a push *below* its advertised
  /// [`max_capacity`](crate::container::Container::max_capacity) yields a `nums` that does not
  /// exceed `capacity()`. That reports what actually happened and is left as is.
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
