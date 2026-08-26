use super::*;

use crate::error::Unclosed;

// Bound-free, like every other `Silent` impl in this family: the body discards the payload, so
// there is no conversion for a bound to describe. See `UnclosedEmitter::emit_unclosed` for why
// the trait method does not impose one either.
impl<'a, L, E, Lang: ?Sized> UnclosedEmitter<'a, L, Lang> for Silent<E, Lang>
where
  L: Lexer<'a>,
{
  #[inline(always)]
  fn emit_unclosed<Delimiter>(
    &mut self,
    _: Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }
}
