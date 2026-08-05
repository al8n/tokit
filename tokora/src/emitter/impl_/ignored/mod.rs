use crate::{error::syntax::MissingSyntaxOf, span::Spanned};

use super::super::*;

mod full_container;
mod missing_leading_separator;
mod missing_trailing_separator;
#[cfg(feature = "pratt")]
mod pratt;
mod separator;
mod too_few;
mod too_many;
mod unclosed;
mod unexpected_leading_separator;
mod unexpected_trailing_separator;

/// An emitter that ignores all errors, and the error type is `()`.
///
/// If you want to preserve the error type, use [`Silent`](super::silent::Silent) emitter instead.
pub type Ignored = crate::utils::marker::Ignored<()>;

impl<'a, L, Lang: ?Sized> Emitter<'a, L, Lang> for Ignored {
  type Error = ();

  #[inline(always)]
  fn emit_lexer_error(
    &mut self,
    _: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  #[inline(always)]
  fn emit_error(&mut self, _: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  #[inline(always)]
  fn emit_unexpected_token(&mut self, _: UnexpectedTokenOf<'a, L, Lang>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  #[inline(always)]
  fn rewind(&mut self, _: &Cursor<'a, '_, L>, _: u64)
  where
    L: Lexer<'a>,
  {
  }
}

// `SeparatedEmitter` is `<'inp, L, Lang>`. This assert used to instantiate it as
// `SeparatedEmitter<'a, Sep, L>` — the *separator* in the lexer slot and the *lexer* in the
// language slot — and it compiled anyway, because `Ignored` implements the trait for any
// parameters at all. A transposed assert over an unconstrained type pins nothing: it type-checks
// whatever you hand it, which is the shape of a cell whose payload cannot fail.
//
// Written at the real signature it does pin something: that `Ignored` really is a no-op
// `SeparatedEmitter` for a concrete lexer at both an unbranded and a branded language. Its
// teeth are recorded by mutation rather than asserted — narrowing `Ignored`'s impl fails this
// block, and transposing the parameters back fails it too, now that the slots carry types that
// cannot stand in for each other.
#[cfg(test)]
const _: () = {
  use crate::lexer::DummyLexer;

  struct DummyLang;

  const fn assert_noop_separated_emitter<'a, L, Lang, Error, E>()
  where
    L: Lexer<'a>,
    Lang: ?Sized,
    E: SeparatedEmitter<'a, L, Lang, Error = Error>,
  {
  }

  assert_noop_separated_emitter::<'_, DummyLexer, (), (), Ignored>();
  assert_noop_separated_emitter::<'_, DummyLexer, DummyLang, (), Ignored>();
};

#[cfg(test)]
mod tests;
