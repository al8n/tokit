use super::*;

use crate::{emitter::FromUnclosed, error::Unclosed};

impl<'a, L, E, Lang: ?Sized> UnclosedEmitter<'a, L, Lang> for Fatal<E, Lang>
where
  L: Lexer<'a>,
  Fatal<E, Lang>: Emitter<'a, L, Lang, Error = E>,
{
  #[inline(always)]
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
    Self::Error: FromUnclosed<'a, L, Lang>,
  {
    Err(Self::Error::from_unclosed(err))
  }
}
