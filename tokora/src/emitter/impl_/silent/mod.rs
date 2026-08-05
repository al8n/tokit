use core::marker::PhantomData;

use crate::{error::syntax::MissingSyntaxOf, span::Spanned};

use super::super::*;

mod full_container;
mod missing_leading_separator;
mod missing_trailing_separator;
#[cfg(feature = "pratt")]
mod pratt;
mod separator;
mod too_few;
mod too_many;
mod unclosed;
mod unexpected_leading_separator;
mod unexpected_trailing_separator;

/// A silent emitter that treats all errors as non-fatal, and ignores them.
///
/// Compared to [`Ignored`](super::ignored::Ignored) emitter, the error type is preserved.
///
/// `Silent` is a **complete implementation** of all atomic emitter traits, providing a pre-built bundle
/// for error-ignoring behavior. It implements all emitter traits ([`Emitter`](super::super::Emitter),
/// [`TooFewEmitter`](super::super::TooFewEmitter), [`TooManyEmitter`](super::super::TooManyEmitter),
/// etc.) with consistent silent behavior.
///
/// For custom error handling, you can implement only the atomic emitter traits you need rather than
/// using this pre-built bundle.
pub struct Silent<T: ?Sized, Lang: ?Sized = ()> {
  _marker: PhantomData<T>,
  _lang: PhantomData<Lang>,
}

impl<T: ?Sized, Lang: ?Sized> Silent<T, Lang> {
  /// Creates a new `Silent`.
  #[inline(always)]
  pub const fn new() -> Self {
    Self {
      _marker: PhantomData,
      _lang: PhantomData,
    }
  }
}

impl<T: ?Sized, Lang: ?Sized> Default for Silent<T, Lang> {
  #[inline(always)]
  fn default() -> Self {
    Self {
      _marker: PhantomData,
      _lang: PhantomData,
    }
  }
}

impl<T: ?Sized, Lang: ?Sized> core::fmt::Debug for Silent<T, Lang> {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Silent")
  }
}

impl<T: ?Sized, Lang: ?Sized> Clone for Silent<T, Lang> {
  #[inline(always)]
  fn clone(&self) -> Self {
    *self
  }
}

impl<T: ?Sized, Lang: ?Sized> Copy for Silent<T, Lang> {}

impl<'a, L, E, Lang: ?Sized> Emitter<'a, L, Lang> for Silent<E, Lang> {
  type Error = E;

  #[inline(always)]
  fn emit_lexer_error(
    &mut self,
    _: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  #[inline(always)]
  fn emit_error(&mut self, _: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  #[inline(always)]
  fn emit_unexpected_token(&mut self, _: UnexpectedTokenOf<'a, L, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  #[inline(always)]
  fn rewind(&mut self, cursor: &Cursor<'a, '_, L>, checkpoint: u64)
  where
    L: Lexer<'a>,
  {
    let _ = (cursor, checkpoint);
  }
}

#[cfg(test)]
const _: () = {
  use crate::lexer::DummyLexer;

  const fn assert_noop_separated_by_emitter<'a, L, Error, E>()
  where
    L: Lexer<'a>,
    E: SeparatedEmitter<'a, L, Error = Error>,
  {
  }

  assert_noop_separated_by_emitter::<'_, DummyLexer, (), Silent<()>>();
};

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
