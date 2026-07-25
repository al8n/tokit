use crate::error::{UnexpectedEoLhs, UnexpectedEoRhs};

use super::*;

impl<'a, L, S, E, Lang: ?Sized> PrattEmitter<'a, L, Lang> for Verbose<E, S, Lang>
where
  L: Lexer<'a, Span = S, Offset = S::Offset>,
  E: FromPrattError<'a, L, Lang>,
  S: Span + Ord + Clone,
  Verbose<E, S, Lang>: Emitter<'a, L, Lang, Error = E>,
{
  #[inline(always)]
  fn emit_unexpected_end_of_lhs(
    &mut self,
    err: UnexpectedEoLhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    // Record on the shared emission log (same path as `emit_error` / `emit_unclosed`) so the
    // diagnostic rewinds precisely with an abandoned speculative branch. The key is the
    // zero-width span at the offset the operand ran out.
    let off = err.offset_ref().clone();
    let span = S::new(off.clone(), off);
    self.record(span, E::from_unexpected_end_of_lhs(err));
    Ok(())
  }

  #[inline(always)]
  fn emit_unexpected_end_of_rhs(
    &mut self,
    err: UnexpectedEoRhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    // Record on the shared emission log (same path as `emit_error` / `emit_unclosed`) so the
    // diagnostic rewinds precisely with an abandoned speculative branch. The key is the
    // zero-width span at the offset the operand ran out.
    let off = err.offset_ref().clone();
    let span = S::new(off.clone(), off);
    self.record(span, E::from_unexpected_end_of_rhs(err));
    Ok(())
  }
}
