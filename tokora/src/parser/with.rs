// The cardinality checks below are `many`'s: `Minimum`/`Maximum` are its bound markers and the
// three `check` helpers are called only by its drivers. `With` itself is substrate.
#[cfg(feature = "many")]
use crate::{
  emitter::{SeparatedEmitter, TooFewEmitter},
  error::syntax::TooFew,
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
  /// The end-of-construct half of a `bounded` collection: the **minimum only**.
  ///
  /// The maximum is not re-checked here. It is settled at the element that broke it, inside
  /// `parser::many::admit_element`, which runs the count verdict before offering the element to
  /// the destination — see [`Maximum::check`] for the whole argument and
  /// `parser::many`'s `ELEMENT_ADMISSION_CENSUS` for what keeps it true.
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
    let minimum = self.primary().get();
    if num_elems < minimum {
      inp
        .emitter()
        .emit_too_few(TooFew::of(full_span.clone(), num_elems, minimum))?;
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
  /// The end-of-construct pass for an `at_most` collection: **nothing but the construct's span**.
  ///
  /// # Why a maximum is not an end-of-construct fact
  ///
  /// It used to be checked here, and that is where the separated drivers got the wrong answer to
  /// [#277](https://github.com/al8n/tokora/issues/277). A maximum is decided by the driver's own
  /// parsed-element count, so it is settled the moment the offending element parses — one whole
  /// construct before this pass runs. Reading it here cost three things:
  ///
  /// * an element that both exceeded the maximum and was refused by the destination reported the
  ///   **refusal first**, because the destination is offered the element mid-loop while this pass
  ///   waits for the end. Under [`Fatal`](crate::emitter::Fatal) that made the public error depend
  ///   on which repetition builder the caller picked, which
  ///   `tokora/tests/end_state_parity.rs`'s INVARIANT E forbids in one sentence;
  /// * a rejecting emitter did not stop at the element that broke the limit, so `at_most(1)` over
  ///   a thousand elements parsed all thousand before saying so;
  /// * any later `Err` exit — a lexer error, a delimiter miss, an element failure — propagated
  ///   past a violation that had already been witnessed and never reported it.
  ///
  /// So the check moved to [`ElementCountHandler::on_element`](crate::parser::many), which
  /// `parser::many::admit_element` runs immediately before the push. It fires exactly once per
  /// construct — a construct exceeds `max` iff some element saw the pre-element count equal to it
  /// — so nothing is lost by this pass staying silent, and `ELEMENT_ADMISSION_CENSUS` fails if a
  /// second `TooMany` site appears anywhere in the tree.
  ///
  /// The function stays because eighteen `EndStateHandler` impls call it and because this is the
  /// one place a reader looking for the maximum's end check will look.
  #[inline(always)]
  pub(crate) fn check<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    _num_elems: usize,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
  {
    Ok(inp.span_since(anchor))
  }
}
