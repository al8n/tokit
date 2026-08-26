use super::*;

use crate::{emitter::FromUnclosed, error::Unclosed};

/// The conversion is named here, on the impl that performs it, and not on the trait method:
/// `Verbose` records the converted diagnostic, so this bound is a demand something collects on.
/// A recording emitter over an error type that cannot absorb the diagnostic is therefore not an
/// [`UnclosedEmitter`] at all, where a dropping one is (see
/// [`UnclosedEmitter::emit_unclosed`] for that half and for why the bound is not on the method):
///
/// ```rust,compile_fail,E0277
/// use tokora::{Lexer, SimpleSpan, emitter::{Emitter, UnclosedEmitter, Verbose}};
///
/// struct Opaque;
///
/// fn has_capability<'inp, L: Lexer<'inp>, E: UnclosedEmitter<'inp, L>>() {}
///
/// fn probe<'inp, L: Lexer<'inp, Span = SimpleSpan, Offset = usize>>()
/// where
///   Verbose<Opaque, SimpleSpan>: Emitter<'inp, L>,
/// {
///   // `Opaque` has no `FromUnclosed` impl, and `Verbose`'s body needs one.
///   has_capability::<L, Verbose<Opaque, SimpleSpan>>();
/// }
/// ```
impl<'a, L, S, E, Lang: ?Sized> UnclosedEmitter<'a, L, Lang> for Verbose<E, S, Lang>
where
  L: Lexer<'a, Span = S, Offset = S::Offset>,
  S: Span + Ord + Clone,
  E: FromUnclosed<'a, L, Lang>,
  Verbose<E, S, Lang>: Emitter<'a, L, Lang, Error = E>,
{
  #[inline(always)]
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    // Record on the shared emission log (same path as `emit_error` /
    // `emit_unexpected_token`) so the diagnostic rewinds precisely with an abandoned
    // speculative branch. The span is the opener's, keyed at the opener's position.
    let span = err.span_ref().clone();
    self.record(span, Self::Error::from_unclosed(err));
    Ok(())
  }
}
