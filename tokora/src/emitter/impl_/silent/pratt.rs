use crate::error::{UnexpectedEoLhs, UnexpectedEoRhs};

use super::*;

// Bound-free, like [`Ignored`]'s twin, and for the same reason: both bodies discard their
// payloads. `Silent` used to demand `E: FromEmitterError + From<UnexpectedEoLhs> +
// From<UnexpectedEoRhs>` here — four conversions on the error type that nothing in the impl
// ever performs. That is a requirement derived from trait *surface* rather than from
// behaviour, and it made `Silent` representation-dependent where its three siblings are not.
//
// The typed pratt driver's own `From<UnexpectedEoLhs/EoRhs>` obligations are unaffected: those
// are the driver's terminal exits, stated at the driver's call sites. What this frees is the
// **token** driver and any direct `PrattEmitter` use. `Fatal` and `Verbose` keep their bounds —
// their bodies use the conversions.
impl<'a, L, E, Lang: ?Sized> PrattEmitter<'a, L, Lang> for Silent<E, Lang>
where
  L: Lexer<'a>,
  Silent<E, Lang>: Emitter<'a, L, Lang, Error = E>,
{
  #[inline(always)]
  fn emit_unexpected_end_of_lhs(
    &mut self,
    _: UnexpectedEoLhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  #[inline(always)]
  fn emit_unexpected_end_of_rhs(
    &mut self,
    _: UnexpectedEoRhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }
}
