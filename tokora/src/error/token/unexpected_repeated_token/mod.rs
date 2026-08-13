use core::marker::PhantomData;

use crate::span::{SimpleSpan, Span};

/// An error indicating that unexpected repeated tokens were found during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnexpectedRepeatedToken<T, S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  count: usize,
  _t: PhantomData<T>,
  _lang: PhantomData<Lang>,
}

impl<T, S> UnexpectedRepeatedToken<T, S> {
  /// Creates a new `UnexpectedRepeatedToken` error.
  #[inline]
  pub const fn new(span: S, count: usize) -> Self {
    Self::of(span, count)
  }
}

impl<T, S, Lang: ?Sized> UnexpectedRepeatedToken<T, S, Lang> {
  /// Creates a new `UnexpectedRepeatedToken` error for the given language.
  #[inline]
  pub const fn of(span: S, count: usize) -> Self {
    Self {
      span,
      count,
      _lang: PhantomData,
      _t: PhantomData,
    }
  }

  /// Returns the reference to the span covering the repeated tokens.
  #[inline]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns the mutable reference to the span covering the repeated tokens.
  #[inline]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Returns the span covering the repeated tokens.
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns the number of repeated tokens found.
  #[inline]
  pub const fn count(&self) -> usize {
    self.count
  }

  /// Expands the span covering the repeated tokens.
  ///
  /// The `span` parameter is used to extend the end of the current span,
  /// and the `count` parameter indicates how many additional repeated tokens were found.
  #[inline]
  pub fn expand(&mut self, span: S, count: usize) -> &mut Self
  where
    S: Span,
  {
    *self.span.end_mut() = span.end();
    self.count += count;
    self
  }
}

impl<T, S, Lang: ?Sized> From<UnexpectedRepeatedToken<T, S, Lang>> for () {
  #[inline]
  fn from(_: UnexpectedRepeatedToken<T, S, Lang>) -> Self {}
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(feature = "std")]
mod tests;
