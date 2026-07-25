use super::*;

/// An emitter that handles missing leading separator.
pub trait MissingLeadingSeparatorEmitter<'inp, L, Lang: ?Sized = ()>:
  SeparatedEmitter<'inp, L, Lang>
where
  L: Lexer<'inp>,
{
  /// Emits an error or warning for a missing a leading separator found during parsing.
  fn emit_missing_leading_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>;
}

impl<'inp, L, Lang, U> MissingLeadingSeparatorEmitter<'inp, L, Lang> for &mut U
where
  U: MissingLeadingSeparatorEmitter<'inp, L, Lang>,
  L: Lexer<'inp>,
  Lang: ?Sized,
{
  #[inline(always)]
  fn emit_missing_leading_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    (**self).emit_missing_leading_separator(name, err)
  }
}

/// A trait bound for creating emitter errors from missing leading separator errors.
pub trait FromMissingLeadingSeparatorError<'a, L, Lang: ?Sized = ()> {
  /// Creates an emitter error from a missing leading separator error.
  fn from_missing_leading_separator(name: CowStr, err: MissingTokenOf<'a, L, Lang>) -> Self
  where
    L: Lexer<'a>;
}

impl<'a, T, L, Lang: ?Sized> FromMissingLeadingSeparatorError<'a, L, Lang> for T
where
  L: Lexer<'a>,
  T: From<MissingTokenOf<'a, L, Lang>>,
{
  #[inline(always)]
  fn from_missing_leading_separator(name: CowStr, err: MissingTokenOf<'a, L, Lang>) -> Self
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
}
