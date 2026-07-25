use crate::{
  error::{syntax::MissingSyntaxOf, token::MissingTokenOf},
  utils::CowStr,
};

use super::*;

impl<'inp, L, S, E, Lang: ?Sized> SeparatedEmitter<'inp, L, Lang> for Verbose<E, S, Lang>
where
  L: Lexer<'inp, Span = S, Offset = S::Offset>,
  E: FromSeparatedError<'inp, L, Lang>,
  S: Span + Ord + Clone,
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
    // Record on the shared emission log (same path as `emit_error` / `emit_unclosed`) so the
    // diagnostic rewinds precisely with an abandoned speculative branch. The key is the
    // zero-width span at the offset where the separator should have been.
    let off = err.offset_ref().clone();
    let span = S::new(off.clone(), off);
    self.record(span, E::from_missing_separator(name, err));
    Ok(())
  }

  #[inline(always)]
  fn emit_missing_element(&mut self, err: MissingSyntaxOf<'inp, L, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    // Record on the shared emission log (same path as `emit_error` / `emit_unclosed`) so the
    // diagnostic rewinds precisely with an abandoned speculative branch. The key is the
    // zero-width span at the offset where the element should have been.
    let off = err.offset_ref().clone();
    let span = S::new(off.clone(), off);
    self.record(span, E::from_missing_element(err));
    Ok(())
  }
}
