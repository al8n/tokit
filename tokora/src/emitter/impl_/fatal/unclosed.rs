use super::*;

use crate::{emitter::FromUnclosed, error::Unclosed};

// The conversion is named here, on the impl that performs it, and not on the trait method —
// the same arrangement `fatal/pratt.rs` carries for `FromPrattError`. `Fatal` returns the
// converted diagnostic, so this bound is a demand something collects on.
impl<'a, L, E, Lang: ?Sized> UnclosedEmitter<'a, L, Lang> for Fatal<E, Lang>
where
  L: Lexer<'a>,
  E: FromUnclosed<'a, L, Lang>,
  Fatal<E, Lang>: Emitter<'a, L, Lang, Error = E>,
{
  #[inline(always)]
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Err(Self::Error::from_unclosed(err))
  }
}
