use crate::utils::CowStr;

use super::*;

impl<'inp, L, S, E, Lang: ?Sized> UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>
  for Verbose<E, S, Lang>
where
  L: Lexer<'inp, Span = S, Offset = S::Offset>,
  E: FromUnexpectedTrailingSeparatorError<'inp, L, Lang>,
  Verbose<E, S, Lang>: SeparatedEmitter<'inp, L, Lang, Error = E>,
  S: Span + Ord + Clone,
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
    // Record on the shared emission log (same path as `emit_error` / `emit_unclosed`) so the
    // diagnostic rewinds precisely with an abandoned speculative branch. The key is the
    // offending separator token's own span.
    let span = err.span_ref().clone();
    self.record(span, E::from_unexpected_trailing_separator(name, err));
    Ok(())
  }
}
