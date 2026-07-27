use super::*;

use crate::{emitter::FromUnclosed, error::Unclosed};

impl<'a, L, S, E, Lang: ?Sized> UnclosedEmitter<'a, L, Lang> for Verbose<E, S, Lang>
where
  L: Lexer<'a, Span = S, Offset = S::Offset>,
  S: Span + Ord + Clone,
  Verbose<E, S, Lang>: Emitter<'a, L, Lang, Error = E>,
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
    // Record on the shared emission log (same path as `emit_error` /
    // `emit_unexpected_token`) so the diagnostic rewinds precisely with an abandoned
    // speculative branch. The span is the opener's, keyed at the opener's position.
    let span = err.span_ref().clone();
    self.record(span, Self::Error::from_unclosed(err));
    Ok(())
  }
}
