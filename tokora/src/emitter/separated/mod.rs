use crate::{
  error::{syntax::MissingSyntaxOf, token::MissingTokenOf},
  utils::CowStr,
};

use super::*;

pub use missing_leading::*;
pub use missing_trailing::*;
pub use unexpected_leading::*;
pub use unexpected_trailing::*;

mod missing_leading;
mod missing_trailing;
mod unexpected_leading;
mod unexpected_trailing;

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod name_census;

/// An emitter that handles missing separator or repeated separators found during parsing.
#[diagnostic::on_unimplemented(
  message = "`{Self}` cannot report separator diagnostics for lexer `{L}`",
  label = "missing `SeparatedEmitter` — required by the separated drivers and by the `ComposableEmitter` bundle",
  note = "implement `SeparatedEmitter` (usually alongside the other members of the `ComposableEmitter` bundle)"
)]
pub trait SeparatedEmitter<'inp, L, Lang: ?Sized = ()>: Emitter<'inp, L, Lang> {
  /// Emits an error or warning for a missing separator found during parsing.
  fn emit_missing_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>;

  /// Emits an error or warning for a missing separator found during parsing.
  fn emit_missing_element(
    &mut self,
    err: MissingSyntaxOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>;
}

impl<'inp, L, U, Lang> SeparatedEmitter<'inp, L, Lang> for &mut U
where
  U: SeparatedEmitter<'inp, L, Lang>,
  Lang: ?Sized,
{
  #[inline(always)]
  fn emit_missing_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    (**self).emit_missing_separator(name, err)
  }

  #[inline(always)]
  fn emit_missing_element(&mut self, err: MissingSyntaxOf<'inp, L, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    (**self).emit_missing_element(err)
  }
}

/// A trait bound for converting separated-by emitter errors into emitter errors.
pub trait FromSeparatedError<'inp, L, Lang: ?Sized = ()>: FromEmitterError<'inp, L, Lang> {
  /// Creates an emitter error from a missing separator error.
  fn from_missing_separator(name: CowStr, err: MissingTokenOf<'inp, L, Lang>) -> Self
  where
    L: Lexer<'inp>;

  /// Creates an emitter error from a missing element error.
  fn from_missing_element(err: MissingSyntaxOf<'inp, L, Lang>) -> Self
  where
    L: Lexer<'inp>;
}

impl<'a, T, L, Lang: ?Sized> FromSeparatedError<'a, L, Lang> for T
where
  L: Lexer<'a>,
  T: FromEmitterError<'a, L, Lang>
    + From<MissingTokenOf<'a, L, Lang>>
    + From<MissingSyntaxOf<'a, L, Lang>>,
{
  #[inline(always)]
  fn from_missing_separator(name: CowStr, err: MissingTokenOf<'a, L, Lang>) -> Self
  where
    L: Lexer<'a>,
  {
    // The separator's name is data the driver had and the payload did not. Stamping it here is
    // the only place it can reach a downstream error type: this blanket captures every type
    // that implements `From<MissingTokenOf>` — which such a type must, to compose with the rest of
    // the conversion family — so coherence forbids it from writing its own impl to recover the
    // name.
    err.with_name(name).into()
  }

  #[inline(always)]
  fn from_missing_element(err: MissingSyntaxOf<'a, L, Lang>) -> Self
  where
    L: Lexer<'a>,
  {
    err.into()
  }
}
