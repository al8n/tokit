use super::*;

/// A trait bound for emitters that handle separated-by syntax errors.
pub trait FromFullContainerError<'a, L, Lang: ?Sized = ()> {
  /// Creates an emitter error from a full container error.
  fn from_full_container(err: FullContainer<L::Span, Lang>) -> Self
  where
    L: Lexer<'a>;
}

impl<'a, T, L, Lang: ?Sized> FromFullContainerError<'a, L, Lang> for T
where
  L: Lexer<'a>,
  T: From<FullContainer<L::Span, Lang>>,
{
  #[inline(always)]
  fn from_full_container(err: FullContainer<L::Span, Lang>) -> Self
  where
    L: Lexer<'a>,
  {
    err.into()
  }
}

/// An emitter that handles errors related to containers do not have enough capacity for repeated parsers.
#[diagnostic::on_unimplemented(
  message = "`{Self}` cannot report a full container for lexer `{L}`",
  label = "missing `FullContainerEmitter` — required by the repetition drivers and by the `ComposableEmitter` bundle",
  note = "implement `FullContainerEmitter` (usually alongside the other members of the `ComposableEmitter` bundle)"
)]
pub trait FullContainerEmitter<'a, L, Lang: ?Sized = ()>: Emitter<'a, L, Lang> {
  /// Emits an error indicating that the given container is full, and cannot accept more elements.
  fn emit_full_container(&mut self, err: FullContainer<L::Span, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>;
}

impl<'a, L, U, Lang: ?Sized> FullContainerEmitter<'a, L, Lang> for &mut U
where
  U: FullContainerEmitter<'a, L, Lang>,
{
  #[inline(always)]
  fn emit_full_container(&mut self, err: FullContainer<L::Span, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    (**self).emit_full_container(err)
  }
}
