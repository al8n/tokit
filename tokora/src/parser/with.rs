// The cardinality checks below are `many`'s: `Minimum`/`Maximum` are its bound markers and the
// three `check` helpers are called only by its drivers. `With` itself is substrate.
#[cfg(feature = "many")]
use crate::{
  emitter::{SeparatedEmitter, TooFewEmitter, TooManyEmitter},
  error::syntax::{TooFew, TooMany},
  input::Cursor,
};

use super::*;

/// Combines two values in a type-safe way.
///
/// This type is used throughout the parser system for:
///
/// - Wrapping parser functions with base parsers: `With<F, Parser<()>>`
/// - Building configuration structures: `With<E, C>` for emitter + cache
/// - Nested configurations: `With<PhantomData<L>, With<E, C>>` for ParserOptions
///
/// # Type Parameters
///
/// - `P`: The primary value (typically a parser function or marker)
/// - `S`: The secondary value (typically configuration or a base parser)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct With<P, S, Cmpl = Complete> {
  pub(crate) primary: P,
  pub(crate) secondary: S,
  pub(crate) _cmpl: PhantomData<Cmpl>,
}

impl<P, S, Cmpl> With<P, S, Cmpl> {
  /// Create a new `With` combinator.
  #[inline(always)]
  pub const fn new(primary: P, secondary: S) -> Self {
    Self {
      primary,
      secondary,
      _cmpl: PhantomData,
    }
  }

  /// Returns a reference to the primary.
  #[inline(always)]
  pub const fn primary(&self) -> &P {
    &self.primary
  }

  /// Returns a reference to the secondary.
  #[inline(always)]
  pub const fn secondary(&self) -> &S {
    &self.secondary
  }

  /// Returns a mutable reference to the primary.
  #[inline(always)]
  pub const fn primary_mut(&mut self) -> &mut P {
    &mut self.primary
  }

  /// Returns a mutable reference to the secondary.
  #[inline(always)]
  pub const fn secondary_mut(&mut self) -> &mut S {
    &mut self.secondary
  }

  /// Maps the primary value using the given function.
  #[inline(always)]
  pub fn map_primary<U, F>(self, f: F) -> With<U, S, Cmpl>
  where
    F: FnOnce(P) -> U,
  {
    With {
      primary: f(self.primary),
      secondary: self.secondary,
      _cmpl: PhantomData,
    }
  }

  /// Maps the secondary value using the given function.
  #[inline(always)]
  pub fn map_secondary<U, F>(self, f: F) -> With<P, U, Cmpl>
  where
    F: FnOnce(S) -> U,
  {
    With {
      primary: self.primary,
      secondary: f(self.secondary),
      _cmpl: PhantomData,
    }
  }
}

#[cfg(feature = "many")]
impl With<Minimum, Maximum> {
  #[inline(always)]
  pub(crate) fn check<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    num_elems: usize,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
      + TooFewEmitter<'inp, L, Lang>
      + TooManyEmitter<'inp, L, Lang>,
  {
    let full_span = inp.span_since(anchor);
    let minimum = self.primary().get();
    let maximum = self.secondary().get();
    if num_elems < minimum {
      inp
        .emitter()
        .emit_too_few(TooFew::of(full_span.clone(), num_elems, minimum))?;
    }

    if num_elems > maximum {
      inp
        .emitter()
        .emit_too_many(TooMany::of(full_span.clone(), maximum + 1, maximum))?;
    }
    Ok(full_span)
  }
}

#[cfg(feature = "many")]
impl Minimum {
  #[inline(always)]
  pub(crate) fn check<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    num_elems: usize,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + TooFewEmitter<'inp, L, Lang>,
  {
    let full_span = inp.span_since(anchor);
    let minimum = self.get();
    if num_elems < minimum {
      inp
        .emitter()
        .emit_too_few(TooFew::of(full_span.clone(), num_elems, minimum))?;
    }
    Ok(full_span)
  }
}

#[cfg(feature = "many")]
impl Maximum {
  #[inline(always)]
  pub(crate) fn check<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    num_elems: usize,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + TooManyEmitter<'inp, L, Lang>,
  {
    let full_span = inp.span_since(anchor);
    let maximum = self.get();
    if num_elems > maximum {
      inp
        .emitter()
        .emit_too_many(TooMany::of(full_span.clone(), maximum + 1, maximum))?;
    }
    Ok(full_span)
  }
}
