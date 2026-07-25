use crate::{error::token::MissingTokenOf, utils::CowStr};

use super::*;

impl<'inp, L, S, E, Lang: ?Sized> MissingTrailingSeparatorEmitter<'inp, L, Lang>
  for Verbose<E, S, Lang>
where
  L: Lexer<'inp, Span = S, Offset = S::Offset>,
  E: FromMissingTrailingSeparatorError<'inp, L, Lang>,
  Verbose<E, S, Lang>: SeparatedEmitter<'inp, L, Lang, Error = E>,
  S: Span + Ord + Clone,
{
  #[inline(always)]
  fn emit_missing_trailing_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    // Record on the shared emission log (same path as `emit_error` / `emit_unclosed`) so the
    // diagnostic rewinds precisely with an abandoned speculative branch. The key is the
    // zero-width span at the offset where the trailing separator should have been.
    let off = err.offset_ref().clone();
    let span = S::new(off.clone(), off);
    self.record(span, E::from_missing_trailing_separator(name, err));
    Ok(())
  }
}
