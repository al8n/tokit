use generic_arraydeque::{ArrayLength, GenericArrayDeque};

/// Trait for container types used in parsers.
///
/// # What the repetition drivers read
///
/// A `Container` is a **destination**, and the drivers treat it as one. They hand it parsed
/// elements and read back exactly two things: whether [`push`](Self::push) took an element, and
/// what [`max_capacity`](Self::max_capacity) says. Everything else a repetition construct
/// reports — how many elements it parsed, whether that count satisfies `at_least` / `at_most`,
/// and the count a [`FullContainer`](crate::error::syntax::FullContainer) names — is the
/// drivers' own bookkeeping, computed without consulting this trait. That is deliberate:
/// `Container` is implementable outside this crate, so an accounting law that rested on an
/// implementation behaving itself would be a law with no keeper.
///
/// The two obligations below are the ones that cannot be replaced by the drivers' own
/// bookkeeping, because they are what the two values *mean*. Nothing else an implementation
/// does is relied upon.
pub trait Container<T> {
  /// Takes `item`, or refuses it and hands it back unchanged.
  ///
  /// **Refuse an item only when the container cannot hold it.** `Err` is this crate's sole
  /// capacity channel: a refusal becomes a
  /// [`FullContainer`](crate::error::syntax::FullContainer) diagnostic naming
  /// [`max_capacity`](Self::max_capacity), so a refusal for any other reason reports a capacity
  /// the container never reached.
  ///
  /// A refusal is not a parse failure. The drivers drop the refused element, count it like any
  /// other parsed element, and carry on.
  fn push(&mut self, item: T) -> Result<(), T>;

  /// Returns the first item in the container, if any.
  fn first(&self) -> Option<&T>;

  /// Returns the last item in the container, if any.
  fn last(&self) -> Option<&T>;

  /// Returns the number of items in the container.
  fn len(&self) -> usize;

  /// Returns `true` if the container is empty.
  #[inline(always)]
  fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Returns the number of items this container can hold.
  ///
  /// `usize::MAX` when there is no fixed bound, e.g. for `Vec<T>`; the real bound otherwise.
  /// This is the capacity a [`FullContainer`](crate::error::syntax::FullContainer) diagnostic
  /// names, so it must be the bound [`push`](Self::push) refuses at.
  fn max_capacity(&self) -> usize;
}

impl<T, U> Container<T> for &mut U
where
  U: Container<T>,
{
  #[inline(always)]
  fn max_capacity(&self) -> usize {
    U::max_capacity(self)
  }

  #[inline(always)]
  fn push(&mut self, item: T) -> Result<(), T> {
    U::push(self, item)
  }

  #[inline(always)]
  fn first(&self) -> Option<&T> {
    U::first(self)
  }

  #[inline(always)]
  fn last(&self) -> Option<&T> {
    U::last(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    U::len(self)
  }
}

macro_rules! blackhole {
  ($ty:ty) => {
    impl<T> Container<T> for $ty {
      #[inline(always)]
      fn max_capacity(&self) -> usize {
        usize::MAX
      }

      #[inline(always)]
      fn push(&mut self, _: T) -> Result<(), T> {
        Ok(())
      }

      #[inline(always)]
      fn first(&self) -> Option<&T> {
        None
      }

      #[inline(always)]
      fn last(&self) -> Option<&T> {
        None
      }

      #[inline(always)]
      fn len(&self) -> usize {
        0
      }
    }
  };
}

blackhole!(());
blackhole!(core::marker::PhantomData<T>);
blackhole!(crate::utils::marker::Ignored<T>);

impl<T> Container<T> for Option<T> {
  #[inline(always)]
  fn max_capacity(&self) -> usize {
    1
  }

  #[inline(always)]
  fn push(&mut self, item: T) -> Result<(), T> {
    if self.is_none() {
      *self = Some(item);
      Ok(())
    } else {
      Err(item)
    }
  }

  #[inline(always)]
  fn first(&self) -> Option<&T> {
    self.as_ref()
  }

  #[inline(always)]
  fn last(&self) -> Option<&T> {
    self.as_ref()
  }

  #[inline(always)]
  fn len(&self) -> usize {
    if self.is_some() { 1 } else { 0 }
  }
}

impl<T, N> Container<T> for GenericArrayDeque<T, N>
where
  N: ArrayLength,
{
  #[inline(always)]
  fn max_capacity(&self) -> usize {
    N::to_usize()
  }

  #[inline(always)]
  fn push(&mut self, item: T) -> Result<(), T> {
    match GenericArrayDeque::push_back(self, item) {
      None => Ok(()),
      Some(e) => Err(e),
    }
  }

  #[inline(always)]
  fn first(&self) -> Option<&T> {
    GenericArrayDeque::front(self)
  }

  #[inline(always)]
  fn last(&self) -> Option<&T> {
    GenericArrayDeque::back(self)
  }

  #[inline(always)]
  fn len(&self) -> usize {
    GenericArrayDeque::len(self)
  }
}

#[cfg(any(feature = "std", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
const _: () = {
  use std::{collections::VecDeque, vec::Vec};

  impl<T> Container<T> for Vec<T> {
    #[inline(always)]
    fn max_capacity(&self) -> usize {
      usize::MAX
    }

    #[inline(always)]
    fn push(&mut self, item: T) -> Result<(), T> {
      Vec::push(self, item);
      Ok(())
    }

    #[inline(always)]
    fn first(&self) -> Option<&T> {
      self.as_slice().first()
    }

    #[inline(always)]
    fn last(&self) -> Option<&T> {
      self.as_slice().last()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      Vec::len(self)
    }
  }

  impl<T> Container<T> for VecDeque<T> {
    #[inline(always)]
    fn max_capacity(&self) -> usize {
      usize::MAX
    }

    #[inline(always)]
    fn push(&mut self, item: T) -> Result<(), T> {
      VecDeque::push_back(self, item);
      Ok(())
    }

    #[inline(always)]
    fn first(&self) -> Option<&T> {
      self.front()
    }

    #[inline(always)]
    fn last(&self) -> Option<&T> {
      self.back()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      VecDeque::len(self)
    }
  }
};

#[cfg(feature = "smallvec_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "smallvec_1")))]
const _: () = {
  use smallvec_1::SmallVec;

  impl<A, T> Container<T> for SmallVec<A>
  where
    A: smallvec_1::Array<Item = T>,
  {
    #[inline(always)]
    fn max_capacity(&self) -> usize {
      usize::MAX
    }

    #[inline(always)]
    fn push(&mut self, item: T) -> Result<(), T> {
      SmallVec::push(self, item);
      Ok(())
    }

    #[inline(always)]
    fn first(&self) -> Option<&T> {
      self.as_slice().first()
    }

    #[inline(always)]
    fn last(&self) -> Option<&T> {
      self.as_slice().last()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      SmallVec::len(self)
    }
  }
};

#[cfg(feature = "tinyvec_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tinyvec_1")))]
const _: () = {
  use tinyvec_1::{Array, ArrayVec, SliceVec};

  impl<T> Container<T> for SliceVec<'_, T> {
    #[inline(always)]
    fn max_capacity(&self) -> usize {
      self.capacity()
    }

    #[inline(always)]
    fn push(&mut self, item: T) -> Result<(), T> {
      // `SliceVec::push` panics on capacity overflow and `tinyvec` publishes no fallible twin
      // of it, so the bound is tested here: the backing slice cannot grow, and an element count
      // the input chooses would otherwise unwind the parser instead of reaching the refusal
      // channel every other fixed-capacity adapter uses.
      if SliceVec::len(self) < self.capacity() {
        SliceVec::push(self, item);
        Ok(())
      } else {
        Err(item)
      }
    }

    #[inline(always)]
    fn first(&self) -> Option<&T> {
      self.as_slice().first()
    }

    #[inline(always)]
    fn last(&self) -> Option<&T> {
      self.as_slice().last()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      SliceVec::len(self)
    }
  }

  impl<A, T> Container<T> for ArrayVec<A>
  where
    A: Array<Item = T>,
  {
    #[inline(always)]
    fn max_capacity(&self) -> usize {
      A::CAPACITY
    }

    #[inline(always)]
    fn push(&mut self, item: T) -> Result<(), T> {
      match self.try_push(item) {
        Some(t) => Err(t),
        None => Ok(()),
      }
    }

    #[inline(always)]
    fn first(&self) -> Option<&T> {
      self.as_slice().first()
    }

    #[inline(always)]
    fn last(&self) -> Option<&T> {
      self.as_slice().last()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      ArrayVec::len(self)
    }
  }

  #[cfg(any(feature = "std", feature = "alloc"))]
  #[cfg_attr(docsrs, doc(cfg(any(feature = "std", feature = "alloc"))))]
  const _: () = {
    use tinyvec_1::TinyVec;

    impl<A, T> Container<T> for TinyVec<A>
    where
      A: Array<Item = T>,
    {
      #[inline(always)]
      fn max_capacity(&self) -> usize {
        usize::MAX
      }

      #[inline(always)]
      fn push(&mut self, item: T) -> Result<(), T> {
        TinyVec::push(self, item);
        Ok(())
      }

      #[inline(always)]
      fn first(&self) -> Option<&T> {
        self.as_slice().first()
      }

      #[inline(always)]
      fn last(&self) -> Option<&T> {
        self.as_slice().last()
      }

      #[inline(always)]
      fn len(&self) -> usize {
        TinyVec::len(self)
      }
    }
  };
};

#[cfg(feature = "heapless_0_9")]
#[cfg_attr(docsrs, doc(cfg(feature = "heapless_0_9")))]
const _: () = {
  use heapless_0_9::{Deque, LenType, Vec};

  impl<T, LenT, const N: usize> Container<T> for Vec<T, N, LenT>
  where
    LenT: LenType,
  {
    #[inline(always)]
    fn max_capacity(&self) -> usize {
      N
    }

    #[inline(always)]
    fn push(&mut self, item: T) -> Result<(), T> {
      Vec::push(self, item)
    }

    #[inline(always)]
    fn first(&self) -> Option<&T> {
      self.as_slice().first()
    }

    #[inline(always)]
    fn last(&self) -> Option<&T> {
      self.as_slice().last()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      self.as_slice().len()
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
      Vec::is_empty(self)
    }
  }

  impl<T, const N: usize> Container<T> for Deque<T, N> {
    #[inline(always)]
    fn max_capacity(&self) -> usize {
      N
    }

    #[inline(always)]
    fn push(&mut self, item: T) -> Result<(), T> {
      Deque::push_back(self, item)
    }

    #[inline(always)]
    fn first(&self) -> Option<&T> {
      self.front()
    }

    #[inline(always)]
    fn last(&self) -> Option<&T> {
      self.back()
    }

    #[inline(always)]
    fn len(&self) -> usize {
      Deque::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
      Deque::is_empty(self)
    }
  }
};

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
