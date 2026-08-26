use super::*;

use crate::error::Unclosed;

impl<'a, L, Lang: ?Sized> UnclosedEmitter<'a, L, Lang> for Ignored
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
