use crate::{error::syntax::MissingSyntaxOf, span::Spanned};

use super::super::{
  separated::{
    MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  *,
};

mod full_container;
mod missing_leading_separator;
mod missing_trailing_separator;
mod pratt;
mod separator;
mod too_few;
mod too_many;
mod unclosed;
mod unexpected_leading_separator;
mod unexpected_trailing_separator;

/// A fatal emitter that treats all errors as fatal.
///
/// This will make the parser stop at the first error encountered, so it is a fail-fast emitter,
/// suitable for scenarios where error recovery is not desired.
///
/// `Fatal` is a **complete implementation** of all atomic emitter traits, providing a pre-built bundle
/// for fail-fast parsing. It implements all emitter traits ([`Emitter`](super::super::Emitter),
/// [`TooFewEmitter`](super::super::TooFewEmitter), [`TooManyEmitter`](super::super::TooManyEmitter),
///
/// For custom error handling, you can implement only the atomic emitter traits you need rather than
/// using this pre-built bundle.
pub struct Fatal<T: ?Sized, Lang: ?Sized = ()> {
  _e: core::marker::PhantomData<T>,
  _lang: core::marker::PhantomData<Lang>,
}

impl<T: ?Sized, Lang: ?Sized> Clone for Fatal<T, Lang> {
  #[inline(always)]
  fn clone(&self) -> Self {
    *self
  }
}

impl<T: ?Sized, Lang: ?Sized> Copy for Fatal<T, Lang> {}

// `Lang`-generic, like `Clone`/`Copy` above and like `Silent`'s twins. These two were written
// `impl<T: ?Sized> ... for Fatal<T>`, which is `Fatal<T, ()>` only — so a branded
// `Fatal::<E, MyLang>::default()` did not resolve, and a branded `Fatal` could not be printed
// at all. Nothing about either body depends on the brand.
impl<T: ?Sized, Lang: ?Sized> Default for Fatal<T, Lang> {
  #[inline(always)]
  fn default() -> Self {
    Self::of()
  }
}

impl<T: ?Sized, Lang: ?Sized> core::fmt::Debug for Fatal<T, Lang> {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Fatal")
  }
}

impl<T: ?Sized> Fatal<T> {
  /// Creates a new `Fatal`.
  #[inline(always)]
  pub const fn new() -> Self {
    Self::of()
  }
}

impl<T: ?Sized, Lang: ?Sized> Fatal<T, Lang> {
  /// Creates a new `Fatal` for the given language.
  #[inline(always)]
  pub const fn of() -> Self {
    Self {
      _e: core::marker::PhantomData,
      _lang: core::marker::PhantomData,
    }
  }
}

impl<'a, L, E, Lang: ?Sized> Emitter<'a, L, Lang> for Fatal<E, Lang>
where
  E: FromEmitterError<'a, L, Lang>,
{
  type Error = E;

  /// Every method here converts and returns `Err`, which for this emitter is not a refusal to
  /// report — it **is** the report. The input layer advances its lexer-error dedup watermark
  /// before calling, so the diagnostic this `Err` carries is the only time it is offered; a
  /// re-lex of the same region will not produce it again. See
  /// [`Emitter::emit_lexer_error`](crate::Emitter::emit_lexer_error).
  #[inline(always)]
  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Err(E::from_lexer_error(err))
  }

  #[inline(always)]
  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Err(err.into_data())
  }

  #[inline(always)]
  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'a, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Err(E::from_unexpected_token(err))
  }

  #[inline(always)]
  fn rewind(&mut self, _: &Cursor<'a, '_, L>, _: u64)
  where
    L: Lexer<'a>,
  {
  }
}
