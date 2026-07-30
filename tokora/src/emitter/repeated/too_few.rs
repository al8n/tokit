use super::*;

/// A trait bound for creating emitter errors from too few elements errors.
pub trait FromTooFewError<'a, L, Lang: ?Sized = ()> {
  /// Creates an emitter error from a too few elements error.
  fn from_too_few(err: TooFew<L::Span, Lang>) -> Self
  where
    L: Lexer<'a>;
}

impl<'a, T, L, Lang: ?Sized> FromTooFewError<'a, L, Lang> for T
where
  L: Lexer<'a>,
  T: From<TooFew<L::Span, Lang>>,
{
  #[inline(always)]
  fn from_too_few(err: TooFew<L::Span, Lang>) -> Self
  where
    L: Lexer<'a>,
  {
    err.into()
  }
}

/// An emitter that handles too few elements error for repeated parsers.
#[diagnostic::on_unimplemented(
  message = "`{Self}` cannot report an under-filled repetition for lexer `{L}`",
  label = "missing `TooFewEmitter` — a `ComposableEmitter` bundle member",
  note = "implement `TooFewEmitter` (usually alongside the other members of the `ComposableEmitter` bundle)"
)]
pub trait TooFewEmitter<'a, L, Lang: ?Sized = ()>: Emitter<'a, L, Lang> {
  /// Emits an error indicating that too few elements were found.
  fn emit_too_few(&mut self, err: TooFew<L::Span, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>;
}

impl<'a, L, Lang: ?Sized, U> TooFewEmitter<'a, L, Lang> for &mut U
where
  U: TooFewEmitter<'a, L, Lang>,
{
  #[inline(always)]
  fn emit_too_few(&mut self, err: TooFew<L::Span, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    (**self).emit_too_few(err)
  }
}
