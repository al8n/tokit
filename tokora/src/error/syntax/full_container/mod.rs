use core::marker::PhantomData;

use crate::span::{SimpleSpan, Span};

/// An error indicating a destination refused an element it could not hold.
///
/// It reports an event, not a count that overran a budget: [`nums()`](Self::nums) says **which**
/// of the construct's elements was refused and [`capacity()`](Self::capacity) says what the
/// destination holds in total. The two are not comparable — see `nums()` — because nothing here
/// knows what the destination held before the construct ran.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct FullContainer<S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  nums: usize,
  limit: usize,
  _lang: PhantomData<Lang>,
}

impl<S> FullContainer<S> {
  /// Creates a new `FullContainer` error.
  #[inline(always)]
  pub const fn new(span: S, nums: usize, maximum: usize) -> Self {
    Self::of(span, nums, maximum)
  }
}

impl<S, Lang: ?Sized> FullContainer<S, Lang> {
  /// Creates a new `FullContainer` error for the given language.
  #[inline(always)]
  pub const fn of(span: S, nums: usize, maximum: usize) -> Self {
    Self::new_in(span, nums, maximum)
  }
}

impl<S, Lang: ?Sized> FullContainer<S, Lang> {
  const fn new_in(span: S, nums: usize, limit: usize) -> Self {
    Self {
      span,
      nums,
      limit,
      _lang: PhantomData,
    }
  }

  /// Returns the span associated with this error.
  #[inline(always)]
  pub const fn span(&self) -> &S {
    &self.span
  }

  /// Returns **which element of the construct the destination refused**: a 1-based position
  /// among the elements the construct had parsed, so `1` means the very first one was refused.
  ///
  /// The count is the repetition driver's own. It is not read back from the container, so no
  /// property of [`len`](crate::container::Container::len) — that a refused push leaves it
  /// alone, that an accepted one moves it by exactly one, that it never passes
  /// [`max_capacity`](crate::container::Container::max_capacity) — is relied on to make this
  /// number true.
  ///
  /// # It is not comparable with [`capacity()`](Self::capacity)
  ///
  /// This counts what the *construct* parsed. `capacity()` describes everything the
  /// *destination* can ever hold, including whatever was in it before the construct ran. The
  /// driver never reads the destination's occupancy and has no way to — [`Container`] is
  /// caller-implemented, and the one obligation it owes, that a refusal means the destination
  /// cannot hold the item, says nothing about counts.
  ///
  /// So a destination handed to [`collect_with`](crate::Accumulator::collect_with) already
  /// holding elements refuses the construct's *first* element, and this is `1` however large
  /// the capacity is. Subtracting the two, or reading one as exceeding the other, is not a
  /// supported reading of this payload.
  ///
  /// The diagnostic is emitted once per construct, **at the first refusal** — not held for the
  /// construct's end, because the emitter's answer to it is also the decision whether the parse
  /// continues, and a fail-fast one is entitled to stop there. Once per construct is a
  /// reporting policy: nothing here predicts what a later push does.
  ///
  /// [`Container`]: crate::container::Container
  #[inline(always)]
  pub const fn nums(&self) -> usize {
    self.nums
  }

  /// Bumps the span by the given offset.
  #[inline(always)]
  pub fn bump(&mut self, by: &S::Offset) -> &mut Self
  where
    S: Span,
  {
    self.span.bump(by);
    self
  }

  /// Returns the destination's **total** capacity —
  /// [`Container::max_capacity`](crate::container::Container::max_capacity), read at the
  /// refusal.
  ///
  /// It is the destination's whole bound and not a budget for this construct: elements the
  /// construct never parsed count against it. It stays in the payload because it is the only
  /// thing the destination told the driver besides "no", and because it is the number a caller
  /// sizing a destination acts on. Read it beside [`nums()`](Self::nums), never against it.
  #[inline(always)]
  pub const fn capacity(&self) -> usize {
    self.limit
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for () {
  #[inline(always)]
  fn from(_: FullContainer<S, Lang>) -> Self {}
}

/// States the refusal that happened, and **relates neither number to the other**.
///
/// The text used to be *"found {nums} elements, which exceeds the maximum capacity of {limit}"*,
/// an arithmetic claim the producer of this error has no standing to make: a repetition driver
/// knows how many elements *it* parsed and that the destination refused one of them, not how
/// full the destination was when the construct started. A conforming `Option` handed to
/// `collect_with` already holding a value refused the first parsed element and rendered *"found
/// 1 elements, which exceeds the maximum capacity of 1"*.
///
/// A refusal at the Nth element of a construct, beside a destination capacity of M, is what is
/// actually known — less than the old sentence claimed, and true.
impl<S, Lang: ?Sized> core::fmt::Display for FullContainer<S, Lang> {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(
      f,
      "element {} of this construct was refused by a destination that holds at most {}",
      self.nums, self.limit
    )
  }
}

impl<S, Lang: ?Sized> core::error::Error for FullContainer<S, Lang>
where
  S: core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
