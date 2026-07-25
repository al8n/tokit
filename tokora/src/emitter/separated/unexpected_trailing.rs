use crate::{
  error::token::{SeparatedError, SeparatedErrorOf, UnexpectedTokenOf},
  utils::CowStr,
};

use super::*;

/// An emitter that handles unexpected trailing separator.
pub trait UnexpectedTrailingSeparatorEmitter<'inp, L, Lang: ?Sized = ()>:
  SeparatedEmitter<'inp, L, Lang>
{
  /// Emits an error or warning for an unexpected trailing separator found during parsing.
  fn emit_unexpected_trailing_separator(
    &mut self,
    name: CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>;
}

impl<'inp, L, Lang, U> UnexpectedTrailingSeparatorEmitter<'inp, L, Lang> for &mut U
where
  U: UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>,
  L: Lexer<'inp>,
  Lang: ?Sized,
{
  #[inline(always)]
  fn emit_unexpected_trailing_separator(
    &mut self,
    name: CowStr,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    (**self).emit_unexpected_trailing_separator(name, err)
  }
}

/// A trait bound for creating emitter errors from unexpected trailing separator errors.
pub trait FromUnexpectedTrailingSeparatorError<'a, L, Lang: ?Sized = ()> {
  /// Creates an emitter error from an unexpected trailing separator error.
  fn from_unexpected_trailing_separator(name: CowStr, err: UnexpectedTokenOf<'a, L, Lang>) -> Self
  where
    L: Lexer<'a>;
}

impl<'a, T, L, Lang: ?Sized> FromUnexpectedTrailingSeparatorError<'a, L, Lang> for T
where
  L: Lexer<'a>,
  T: From<SeparatedErrorOf<'a, L, Lang>>,
{
  #[inline(always)]
  fn from_unexpected_trailing_separator(name: CowStr, err: UnexpectedTokenOf<'a, L, Lang>) -> Self
  where
    L: Lexer<'a>,
  {
    // The separator's name is data the driver had and the payload did not. Stamping it here is
    // the only place it can reach a downstream error type: this blanket captures every type
    // that implements `From<SeparatedErrorOf>` — which such a type must, to compose with the rest of
    // the conversion family — so coherence forbids it from writing its own impl to recover the
    // name.
    SeparatedError::trailing(err).with_name(name).into()
  }
}
