use generic_arraydeque::{ArrayLength, GenericArrayDeque};

use crate::{
  Emitter, Lexer, ParseContext,
  input::{Cursor, InputRef},
  span::Spanned,
};

mod allow_leading;
mod allow_leading_require_trailing;
mod allow_surrounded;
mod allow_trailing;
mod bounded;
mod maximum;
mod minimum;
mod require_leading;
mod require_leading_allow_trailing;
mod require_surrounded;
mod require_trailing;
mod unbounded;

/// A handler for separator events during parsing.
///
/// # Delivery law
///
/// [`on_separator`](Self::on_separator) is called **exactly once for every separator token a
/// driver consumes, in source order** — the leading one, every one between elements, every
/// duplicate in a run, and the trailing one. Before this release the happy-path and trailing
/// separators were never delivered and only the anomalous ones were, i.e. the exact complement
/// of this law, so a container that recorded separators recorded the wrong set.
pub trait SeparatorHandler<'inp, L> {
  /// Whether this handler observes separators at all.
  ///
  /// A container that ignores them sets this to `false` and the drivers skip both the call and
  /// the clone that feeds it — the guard is a monomorphized constant, so delivery costs a
  /// non-observing container nothing. Defaults to `true` so an existing implementor keeps
  /// receiving every separator.
  const OBSERVES_SEPARATORS: bool = true;

  /// Called once for each separator token the driver consumes, in source order.
  fn on_separator(&mut self, sep: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>;

  /// Delivers `sep` iff this handler observes separators.
  ///
  /// Drivers call this, never [`on_separator`](Self::on_separator) directly: the clone the call
  /// needs lives inside the constant-guarded branch, so it folds away entirely for a
  /// non-observing container.
  #[inline(always)]
  fn observe_separator(&mut self, sep: &Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    if Self::OBSERVES_SEPARATORS {
      self.on_separator(sep.clone());
    }
  }
}

impl<'inp, L, T> SeparatorHandler<'inp, L> for &mut T
where
  T: ?Sized + SeparatorHandler<'inp, L>,
{
  // Load-bearing: without the forward every mut-ref container would fall back to the `true`
  // default and pay the clone on a path whose backing type does not observe separators.
  const OBSERVES_SEPARATORS: bool = T::OBSERVES_SEPARATORS;

  #[inline(always)]
  fn on_separator(&mut self, sep: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    (**self).on_separator(sep)
  }
}

macro_rules! blackhole_separator_handler {
  ($ty:ty) => {
    impl<'inp, L> SeparatorHandler<'inp, L> for $ty {
      const OBSERVES_SEPARATORS: bool = false;

      #[inline(always)]
      fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }
    }
  };
  (@generic $ty:ty) => {
    impl<'inp, L, T> SeparatorHandler<'inp, L> for $ty {
      const OBSERVES_SEPARATORS: bool = false;

      #[inline(always)]
      fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }
    }
  };
}

blackhole_separator_handler!(());
blackhole_separator_handler!(@generic core::marker::PhantomData<T>);
blackhole_separator_handler!(@generic crate::utils::marker::Ignored<T>);

impl<'inp, L, T, N> SeparatorHandler<'inp, L> for GenericArrayDeque<T, N>
where
  N: ArrayLength,
{
  const OBSERVES_SEPARATORS: bool = false;

  #[inline(always)]
  fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
  }
}

#[cfg(feature = "heapless_0_9")]
#[cfg_attr(docsrs, doc(cfg(feature = "heapless_0_9")))]
const _: () = {
  use heapless_0_9::{Deque, Vec};

  impl<'inp, L, T, const N: usize, LenT> SeparatorHandler<'inp, L> for Vec<T, N, LenT>
  where
    LenT: heapless_0_9::LenType,
  {
    const OBSERVES_SEPARATORS: bool = false;

    #[inline(always)]
    fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  impl<'inp, L, T, const N: usize> SeparatorHandler<'inp, L> for Deque<T, N> {
    const OBSERVES_SEPARATORS: bool = false;

    #[inline(always)]
    fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }
};

#[cfg(any(feature = "alloc", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
const _: () = {
  use std::{collections::vec_deque::VecDeque, vec::Vec};

  impl<'inp, L, T> SeparatorHandler<'inp, L> for Vec<T> {
    const OBSERVES_SEPARATORS: bool = false;

    #[inline(always)]
    fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  impl<'inp, L, T> SeparatorHandler<'inp, L> for VecDeque<T> {
    const OBSERVES_SEPARATORS: bool = false;

    #[inline(always)]
    fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  #[cfg(feature = "smallvec_1")]
  #[cfg_attr(docsrs, doc(cfg(feature = "smallvec_1")))]
  impl<'inp, L, T, N> SeparatorHandler<'inp, L> for smallvec_1::SmallVec<N>
  where
    N: smallvec_1::Array<Item = T>,
  {
    const OBSERVES_SEPARATORS: bool = false;

    #[inline(always)]
    fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }
};

#[cfg(feature = "tinyvec_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tinyvec_1")))]
const _: () = {
  use tinyvec_1::{Array, ArrayVec, SliceVec};

  impl<'inp, L, T, N> SeparatorHandler<'inp, L> for ArrayVec<N>
  where
    N: Array<Item = T>,
  {
    const OBSERVES_SEPARATORS: bool = false;

    #[inline(always)]
    fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  impl<'inp, L, T> SeparatorHandler<'inp, L> for SliceVec<'_, T> {
    const OBSERVES_SEPARATORS: bool = false;

    #[inline(always)]
    fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  #[cfg(any(feature = "alloc", feature = "std"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
  const _: () = {
    impl<'inp, L, T, A> SeparatorHandler<'inp, L> for tinyvec_1::TinyVec<A>
    where
      A: Array<Item = T>,
    {
      const OBSERVES_SEPARATORS: bool = false;

      #[inline(always)]
      fn on_separator(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }
    }
  };
};

/// A handler for delimiter events during parsing.
pub trait DelimiterHandler<'inp, L> {
  /// Called when a delimiter is encountered.
  fn on_open_delimiter(&mut self, open: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>;

  /// Called when a closing delimiter is encountered.
  fn on_close_delimiter(&mut self, close: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>;
}

impl<'inp, L, T> DelimiterHandler<'inp, L> for &mut T
where
  T: ?Sized + DelimiterHandler<'inp, L>,
{
  #[inline(always)]
  fn on_open_delimiter(&mut self, open: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    (**self).on_open_delimiter(open);
  }

  #[inline(always)]
  fn on_close_delimiter(&mut self, close: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    (**self).on_close_delimiter(close);
  }
}

macro_rules! blackhole_delimiter_handler {
  ($ty:ty) => {
    impl<'inp, L> DelimiterHandler<'inp, L> for $ty {
      #[inline(always)]
      fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }

      #[inline(always)]
      fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }
    }
  };
  (@generic $ty:ty) => {
    impl<'inp, L, T> DelimiterHandler<'inp, L> for $ty {
      #[inline(always)]
      fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }

      #[inline(always)]
      fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }
    }
  };
}

blackhole_delimiter_handler!(());
blackhole_delimiter_handler!(@generic core::marker::PhantomData<T>);
blackhole_delimiter_handler!(@generic crate::utils::marker::Ignored<T>);

impl<'inp, L, T, N> DelimiterHandler<'inp, L> for GenericArrayDeque<T, N>
where
  N: ArrayLength,
{
  #[inline(always)]
  fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
  }

  #[inline(always)]
  fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
  }
}

#[cfg(any(feature = "alloc", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
const _: () = {
  use std::{collections::vec_deque::VecDeque, vec::Vec};

  impl<'inp, L, T> DelimiterHandler<'inp, L> for Vec<T> {
    #[inline(always)]
    fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }

    #[inline(always)]
    fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  impl<'inp, L, T> DelimiterHandler<'inp, L> for VecDeque<T> {
    #[inline(always)]
    fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }

    #[inline(always)]
    fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  #[cfg(feature = "smallvec_1")]
  #[cfg_attr(docsrs, doc(cfg(feature = "smallvec_1")))]
  impl<'inp, L, T, N> DelimiterHandler<'inp, L> for smallvec_1::SmallVec<N>
  where
    N: smallvec_1::Array<Item = T>,
  {
    #[inline(always)]
    fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }

    #[inline(always)]
    fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }
};

#[cfg(feature = "tinyvec_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tinyvec_1")))]
const _: () = {
  use tinyvec_1::{Array, ArrayVec, SliceVec};

  impl<'inp, L, T, N> DelimiterHandler<'inp, L> for ArrayVec<N>
  where
    N: Array<Item = T>,
  {
    #[inline(always)]
    fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }

    #[inline(always)]
    fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  impl<'inp, L, T> DelimiterHandler<'inp, L> for SliceVec<'_, T> {
    #[inline(always)]
    fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }

    #[inline(always)]
    fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }
  #[cfg(any(feature = "alloc", feature = "std"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "std"))))]
  const _: () = {
    impl<'inp, L, T, A> DelimiterHandler<'inp, L> for tinyvec_1::TinyVec<A>
    where
      A: Array<Item = T>,
    {
      #[inline(always)]
      fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }

      #[inline(always)]
      fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
      where
        L: Lexer<'inp>,
      {
      }
    }
  };
};

#[cfg(feature = "heapless_0_9")]
#[cfg_attr(docsrs, doc(cfg(feature = "heapless_0_9")))]
const _: () = {
  use heapless_0_9::{Deque, Vec};

  impl<'inp, L, T, const N: usize, LenT> DelimiterHandler<'inp, L> for Vec<T, N, LenT>
  where
    LenT: heapless_0_9::LenType,
  {
    #[inline(always)]
    fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }

    #[inline(always)]
    fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }

  impl<'inp, L, T, const N: usize> DelimiterHandler<'inp, L> for Deque<T, N> {
    #[inline(always)]
    fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }

    #[inline(always)]
    fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
    where
      L: Lexer<'inp>,
    {
    }
  }
};

pub(super) trait EndStateHandler<
  'inp,
  'closure,
  Sep,
  O,
  L,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::Completeness = crate::input::Complete,
>
{
  fn handle_start_state(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;

  fn handle_element_state(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;

  fn handle_leading_state(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    leading_sep: Spanned<L::Token, L::Span>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;

  fn handle_separator_state(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    sep: Spanned<L::Token, L::Span>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}

pub(super) trait ContinueStateHandler<
  'inp,
  'closure,
  Sep,
  O,
  L,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::Completeness = crate::input::Complete,
>: ElementCountHandler<'inp, 'closure, L, Ctx, Lang, Cmpl>
{
  fn handle_start_state(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    off: L::Offset,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}

pub(super) trait SeparatorStateHandler<
  'inp,
  'closure,
  Sep,
  O,
  L,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::Completeness = crate::input::Complete,
>
{
  fn handle_start_state(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    sep_tok: &Spanned<L::Token, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}

/// The count-bound verdict on **one parsed element**, run before that element is offered to the
/// destination.
///
/// # Why this is its own trait, and why it names no `Sep` or `O`
///
/// It is the half of a cardinality handler that [`admit_element`](super::admit_element) needs, and
/// `admit_element` is reached from every repetition driver — the two plain ones through
/// [`RepeatedHandler`], the four separated ones through [`ContinueStateHandler`], and the two
/// delimited-repeated engines through neither, since those carry their end pass in a closure. One
/// trait naming only what the hook itself uses is what lets all three routes hand the same value
/// to the same chokepoint.
///
/// It is a **supertrait** of the other two rather than a fourth handler threaded beside them, so a
/// driver already generic over `RepeatedHandler` or `ContinueStateHandler` gains the hook without a
/// new where-clause and no `ParseInput` impl in `many/macros.rs` changes.
///
/// # The law it exists to keep
///
/// A construct exceeds its maximum **iff** some element saw the pre-element count equal to it, so
/// reporting at that element reports the violation exactly once — and reports it *before* the
/// element reaches the container, which is what fixes the order of the two refusals one element
/// can collect. [`admit_element`](super::admit_element) carries the argument;
/// `tokora/tests/repetition_diagnostic_order.rs` is the runtime pin and `ELEMENT_ADMISSION_CENSUS`
/// the structural one.
// `pub(in crate::parser)` rather than the `pub(super)` its two subtraits carry: it appears in
// `many::admit_element`'s signature, which the drivers reach at that visibility, and a supertrait
// narrower than the function naming it is a `private_interfaces` error.
pub(in crate::parser) trait ElementCountHandler<
  'inp,
  'closure,
  L,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::Completeness = crate::input::Complete,
>
{
  fn on_element(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}

/// Forwards the element hook through a separator-policy wrapper.
///
/// The policy newtypes nest — `AllowSurrounded` *is* `AllowLeading<AllowTrailing<_>>`, and the two
/// combined policies are the same shape — so four blanket forwards cover all thirty-two
/// policy-by-cardinality end-state handlers. Written per handler file instead, thirty-two of them
/// would each be a place to forget the hook; here a wrapper that does not forward does not compile.
macro_rules! forward_element_count {
  ($($policy:ident),* $(,)?) => {$(
    impl<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness, P>
      ElementCountHandler<'inp, 'closure, L, Ctx, Lang, Cmpl> for crate::parser::$policy<P>
    where
      L: Lexer<'inp>,
      Ctx: ParseContext<'inp, L, Lang>,
      P: ElementCountHandler<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    {
      #[inline(always)]
      fn on_element(
        &self,
        num_elems: usize,
        inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
        anchor: &Cursor<'inp, 'closure, L>,
      ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
      where
        L: Lexer<'inp>,
        Ctx: ParseContext<'inp, L, Lang>,
      {
        self.parser.on_element(num_elems, inp, anchor)
      }
    }
  )*};
}

forward_element_count!(AllowLeading, AllowTrailing, RequireLeading, RequireTrailing);

pub(super) trait RepeatedHandler<
  'inp,
  'closure,
  O,
  L,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::Completeness = crate::input::Complete,
>: ElementCountHandler<'inp, 'closure, L, Ctx, Lang, Cmpl>
{
  fn on_stop(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}
