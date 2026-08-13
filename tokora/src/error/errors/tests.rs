use super::*;
use crate::error::ErrorContainer;
use generic_arraydeque::ConstGenericArrayDeque;

/// A container whose [`FromIterator`] keeps what fits and drops the rest — the only thing a
/// fixed-capacity `collect` can do, because `FromIterator` has no channel for reporting a value
/// it refused.
///
/// It wraps an inner [`ErrorContainer`] instead of reimplementing one so that the fixture carries
/// exactly the one behaviour under test and the capacity comes from the inner type. Nothing in
/// `Errors` reaches this `FromIterator` any more, and that is what the tests below check: put the
/// delegation back and it is reached again, and the flag goes quiet.
struct Truncating<C>(C);

impl<E, C> FromIterator<E> for Truncating<C>
where
  C: ErrorContainer<E>,
{
  fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
    let mut inner = C::new();
    for error in iter {
      if inner.try_push(error).is_err() {
        break;
      }
    }
    Self(inner)
  }
}

impl<E, C> ErrorContainer<E> for Truncating<C>
where
  C: ErrorContainer<E>,
{
  type IntoIter = C::IntoIter;
  type Iter<'a>
    = C::Iter<'a>
  where
    Self: 'a,
    E: 'a;

  fn new() -> Self {
    Self(C::new())
  }

  fn with_capacity(capacity: usize) -> Self {
    Self(C::with_capacity(capacity))
  }

  fn push(&mut self, error: E) {
    self.0.push(error);
  }

  fn try_push(&mut self, error: E) -> Result<(), E> {
    self.0.try_push(error)
  }

  fn pop(&mut self) -> Option<E> {
    self.0.pop()
  }

  fn clear(&mut self) {
    self.0.clear();
  }

  fn len(&self) -> usize {
    self.0.len()
  }

  fn iter(&self) -> Self::Iter<'_> {
    self.0.iter()
  }

  fn into_iter(self) -> Self::IntoIter {
    ErrorContainer::into_iter(self.0)
  }

  fn remaining_capacity(&self) -> Option<usize> {
    self.0.remaining_capacity()
  }
}

#[test]
fn test_new() {
  let _: Errors<&str> = Errors::new();
}

#[test]
fn test_push_and_len() {
  let mut errors = Errors::new();
  errors.push("Error 1");
  assert_eq!(errors.len(), 1);
  errors.push("Error 2");
  assert_eq!(errors.len(), 2);
}

#[test]
fn test_clear() {
  let mut errors = Errors::new();
  errors.push("Error");
  errors.clear();
  assert!(errors.is_empty());
}

#[test]
fn test_iteration() {
  let mut errors = Errors::new();
  errors.push(1);
  errors.push(2);

  let sum: i32 = errors.iter().sum();
  assert_eq!(sum, 3);
}

#[test]
fn test_overflow_tracking() {
  type SmallErrors<'a> = Errors<&'a str, ConstGenericArrayDeque<&'a str, 1>>;
  let mut errors: SmallErrors<'_> = Errors::from_container(ConstGenericArrayDeque::<_, 1>::new());

  assert!(!errors.overflowed());
  errors.push("first");
  assert_eq!(errors.len(), 1);
  assert_eq!(errors.remaining_capacity(), Some(0));
  assert!(errors.is_full());

  errors.push("second");
  assert!(errors.overflowed());
  assert_eq!(errors.len(), 1);
}

#[test]
fn test_try_push_reports_error() {
  type SmallErrors<'a> = Errors<&'a str, ConstGenericArrayDeque<&'a str, 1>>;
  let mut errors: SmallErrors<'_> = Errors::from_container(ConstGenericArrayDeque::<_, 1>::new());

  assert!(errors.try_push("first").is_ok());
  assert!(errors.try_push("second").is_err());
  assert!(errors.overflowed());
}

#[cfg(any(feature = "alloc", feature = "std"))]
#[test]
fn test_with_capacity() {
  let errors: Errors<&str> = Errors::with_capacity(10);
  assert_eq!(errors.capacity(), 10);
  assert!(errors.is_empty());
}

#[cfg(any(feature = "alloc", feature = "std"))]
#[test]
fn test_pop() {
  let mut errors = Errors::new();
  errors.push(1);
  errors.push(2);

  assert_eq!(errors.pop(), Some(1));
  assert_eq!(errors.pop(), Some(2));
  assert_eq!(errors.pop(), None);
}

/// al8n/tokora#247. `Errors` derived `DerefMut` (and `AsMut<C>`) onto its container, so
/// `errors.push_back(..)` reached the deque's own insertion API: a bounded container rejected
/// the value, handed it back, and `overflowed()` never learned of it. Both doors are removed,
/// so that line no longer compiles and every path that can *offer* an error is a path that
/// accounts for it. This walks the surviving surface and requires the flag to be right at each
/// step.
#[test]
fn every_insertion_door_maintains_the_overflow_flag() {
  type SmallErrors<'a> = Errors<&'a str, ConstGenericArrayDeque<&'a str, 1>>;
  let mut errors: SmallErrors<'_> = Errors::from_container(ConstGenericArrayDeque::<_, 1>::new());

  // `try_push` — the door that reports.
  assert!(errors.try_push("first").is_ok());
  assert!(!errors.overflowed(), "a fitting error is not an overflow");
  assert_eq!(
    errors.try_push("dropped"),
    Err("dropped"),
    "the rejected value comes back to the caller"
  );
  assert!(errors.overflowed());
  assert_eq!(errors.len(), 1, "and it is not in the container");

  // `push` — the same gateway with the report discarded.
  let mut errors: SmallErrors<'_> = Errors::from_container(ConstGenericArrayDeque::<_, 1>::new());
  errors.push("first");
  assert!(!errors.overflowed());
  errors.push("dropped");
  assert!(errors.overflowed());
}

/// The flag is a fact about errors that never entered, so nothing that removes an error can
/// clear it — and a container with room again does not mean the dropped one came back.
#[test]
fn removal_and_reinsertion_leave_the_historical_overflow_flag_set() {
  type SmallErrors<'a> = Errors<&'a str, ConstGenericArrayDeque<&'a str, 1>>;
  let mut errors: SmallErrors<'_> = Errors::from_container(ConstGenericArrayDeque::<_, 1>::new());

  errors.push("first");
  errors.push("dropped");
  assert!(errors.overflowed());

  assert_eq!(errors.pop(), Some("first"));
  assert!(
    errors.overflowed(),
    "popping a held error does not un-drop the refused one"
  );

  errors.push("third");
  assert_eq!(errors.len(), 1);
  assert!(errors.overflowed(), "and neither does refilling the space");

  errors.clear();
  assert!(errors.is_empty());
  assert!(errors.overflowed(), "nor does clearing it");
}

/// al8n/tokora#284. `FromIterator` delegated to the container's own `FromIterator`, which for a
/// bounded container keeps what fits and cannot report the rest — so `collect()` truncated and
/// `overflowed()` said `false` over what was left. Nothing mutated: the container arrived already
/// short and the flag was initialised to a lie, which is why the mutation-door census of #247
/// could not see it.
#[test]
fn collecting_more_errors_than_fit_latches_the_flag() {
  type Small<'a> = Errors<&'a str, Truncating<ConstGenericArrayDeque<&'a str, 1>>>;

  let errors: Small<'_> = ["kept", "dropped", "also dropped"].into_iter().collect();
  assert_eq!(errors.len(), 1, "the container kept what fits");
  assert!(
    errors.overflowed(),
    "and the wrapper knows the other two were offered"
  );

  let errors: Small<'_> = ["kept"].into_iter().collect();
  assert_eq!(errors.len(), 1);
  assert!(!errors.overflowed(), "a collect that fits reports clean");

  let errors: Small<'_> = core::iter::empty::<&str>().collect();
  assert!(errors.is_empty());
  assert!(!errors.overflowed());
}

/// al8n/tokora#284, round 3. Funnelling the collect through `push` brought its own reservation,
/// and reading `size_hint` before the first `next` made a hint load-bearing: an empty iterator
/// claiming `usize::MAX` panicked in `with_capacity` where the delegated path had returned empty
/// without allocating at all.
///
/// Checked at both dispatch points of [`ErrorContainer::from_errors`], because round 4 moved the
/// default container off the funnel: `Errors<&str>` is `VecDeque`-backed and now delegates, while
/// `Truncating` inherits the default. A `Truncating<VecDeque<_>>` reserving `usize::MAX` is a
/// capacity-overflow panic, so surviving the collect *is* the observation that nothing was
/// reserved.
#[cfg(any(feature = "alloc", feature = "std"))]
#[test]
fn an_empty_iterator_that_overstates_its_lower_bound_reserves_nothing() {
  struct EmptyClaimingMax;

  impl Iterator for EmptyClaimingMax {
    type Item = &'static str;

    fn next(&mut self) -> Option<&'static str> {
      None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
      (usize::MAX, None)
    }
  }

  let errors: Errors<&str> = EmptyClaimingMax.collect();
  assert!(errors.is_empty());
  assert!(!errors.overflowed());
  assert_eq!(
    errors.capacity(),
    0,
    "nothing is reserved on the strength of a hint no item backed"
  );

  // The reservation is still made once an error has been observed, so the collect does not
  // become a sequence of growth reallocations.
  let errors: Errors<&str> = ["a", "b", "c"].into_iter().collect();
  assert_eq!(errors.len(), 3);
  assert!(errors.capacity() >= 3);

  // The same claim where the funnel itself is what runs.
  type Funnelled<'a> = Errors<&'a str, Truncating<std::collections::VecDeque<&'a str>>>;

  let errors: Funnelled<'_> = EmptyClaimingMax.collect();
  assert!(errors.is_empty());
  assert!(!errors.overflowed());

  let errors: Funnelled<'_> = ["a", "b", "c"].into_iter().collect();
  assert_eq!(errors.len(), 3);
}

/// al8n/tokora#284, round 4. `FromIterator` asks the container to build itself, and the default
/// answer is the per-item funnel — so a container that leaves
/// [`ErrorContainer::from_errors`] alone accounts exactly as a loop of `try_push` would, and the
/// two containers that override it are the two that cannot refuse an error.
#[cfg(any(feature = "alloc", feature = "std"))]
#[test]
fn the_bulk_hook_defaults_to_the_funnel_and_the_unbounded_containers_override_it() {
  use std::{collections::VecDeque, vec::Vec};

  // Defaulted: a bounded container reports what it refused, through the funnel.
  let (container, overflowed) =
    <ConstGenericArrayDeque<&str, 1> as ErrorContainer<&str>>::from_errors(["kept", "dropped"]);
  assert_eq!(ErrorContainer::len(&container), 1);
  assert!(
    overflowed,
    "the default funnel offers every error and reports"
  );

  let (container, overflowed) =
    <ConstGenericArrayDeque<&str, 2> as ErrorContainer<&str>>::from_errors(["a", "b"]);
  assert_eq!(ErrorContainer::len(&container), 2);
  assert!(!overflowed);

  // Overridden: unbounded, so `false` is a property of the container rather than a reading.
  let (container, overflowed) = <Vec<u8> as ErrorContainer<u8>>::from_errors(std::vec![1u8, 2, 3]);
  assert_eq!(container, std::vec![1u8, 2, 3]);
  assert!(!overflowed);

  let (container, overflowed) =
    <VecDeque<u8> as ErrorContainer<u8>>::from_errors(std::vec![1u8, 2, 3]);
  assert_eq!(container, VecDeque::from(std::vec![1u8, 2, 3]));
  assert!(!overflowed);

  // And an empty iterator builds an empty container on both paths.
  let (container, overflowed) =
    <Vec<u8> as ErrorContainer<u8>>::from_errors(core::iter::empty::<u8>());
  assert!(container.is_empty());
  assert!(!overflowed);

  let (container, overflowed) =
    <ConstGenericArrayDeque<&str, 1> as ErrorContainer<&str>>::from_errors(
      core::iter::empty::<&str>(),
    );
  assert_eq!(ErrorContainer::len(&container), 0);
  assert!(!overflowed);
}

/// What a container that lies through the bulk hook can do, pinned next to what one that lies
/// through the per-item door can do: the same thing. `overflowed()` is only ever as honest as the
/// container's own report, and `from_errors` is trusted no further than `try_push` — the wrapper
/// neither re-derives the flag nor cross-checks it against `len()`, on either door.
#[test]
fn a_container_that_lies_through_the_bulk_hook_fails_exactly_as_one_that_lies_through_try_push() {
  /// Overrides the bulk hook to keep one error and report clean.
  struct LyingBulk(Option<&'static str>);

  /// Keeps one error and reports clean from the *per-item* door, which every `ErrorContainer`
  /// has always been able to do.
  struct LyingPush(Option<&'static str>);

  macro_rules! lying_container {
    ($ty:ident $(, $extra:item)?) => {
      impl ErrorContainer<&'static str> for $ty {
        type IntoIter = core::option::IntoIter<&'static str>;
        type Iter<'a> = core::option::Iter<'a, &'static str>;

        fn new() -> Self {
          Self(None)
        }

        fn push(&mut self, error: &'static str) {
          self.0.get_or_insert(error);
        }

        fn pop(&mut self) -> Option<&'static str> {
          self.0.take()
        }

        fn len(&self) -> usize {
          self.0.iter().count()
        }

        fn iter(&self) -> Self::Iter<'_> {
          self.0.iter()
        }

        fn into_iter(self) -> Self::IntoIter {
          IntoIterator::into_iter(self.0)
        }

        $($extra)?
      }
    };
  }

  lying_container!(
    LyingBulk,
    fn from_errors<I: IntoIterator<Item = &'static str>>(errors: I) -> (Self, bool) {
      let mut this = Self(None);
      for error in errors {
        this.push(error);
      }
      (this, false)
    }
  );
  // `try_push` is defaulted to an infallible `push`, so this one lies by omission — which is the
  // shape every container implemented before the bulk hook existed.
  lying_container!(LyingPush);

  let bulk: Errors<&str, LyingBulk> = ["kept", "dropped"].into_iter().collect();
  assert_eq!(bulk.len(), 1, "one of the two errors is gone");
  assert!(!bulk.overflowed(), "and the container said nothing was");

  let per_item: Errors<&str, LyingPush> = ["kept", "dropped"].into_iter().collect();
  assert_eq!(per_item.len(), 1);
  assert!(!per_item.overflowed());

  let mut per_item: Errors<&str, LyingPush> = Errors::new_in(ErrorContainer::new());
  per_item.push("kept");
  assert!(
    per_item.try_push("dropped").is_ok(),
    "the container accepted"
  );
  assert_eq!(per_item.len(), 1, "and then dropped it");
  assert!(!per_item.overflowed());
}

/// `From<E>` inherited the same silence, and stays infallible: a capacity floor is not expressible
/// on [`ErrorContainer`], so the zero-capacity case is answered by the flag rather than by a
/// `Result`.
#[test]
fn a_single_error_into_a_zero_capacity_container_latches_the_flag() {
  let errors: Errors<&str, Truncating<ConstGenericArrayDeque<&str, 0>>> = Errors::from("dropped");
  assert_eq!(errors.len(), 0, "the sole error did not fit");
  assert!(errors.overflowed(), "and it is not silently gone");

  let errors: Errors<&str, Truncating<ConstGenericArrayDeque<&str, 1>>> = Errors::from("kept");
  assert_eq!(errors.len(), 1);
  assert!(!errors.overflowed());
}

/// The bound is a swap, not an addition, and this is the side it pays back: the containers that
/// gained `collect()` are exactly this crate's bounded ones, which had no `FromIterator` of their
/// own to delegate to and so had no `collect()` at all.
#[test]
fn the_bounded_containers_that_had_no_from_iterator_can_now_be_collected_into() {
  let errors: Errors<&str, Option<&str>> = ["kept", "dropped"].into_iter().collect();
  assert_eq!(errors.len(), 1);
  assert!(errors.overflowed());

  let errors: Errors<&str, ConstGenericArrayDeque<&str, 2>> =
    ["a", "b", "dropped"].into_iter().collect();
  assert_eq!(errors.len(), 2);
  assert!(errors.overflowed());
}

/// [`Errors::from_container`] adopts a container the caller built, so the flag starts `false` and
/// covers only what is offered afterwards — the wrapper cannot read a history the container does
/// not keep. This pins the difference from the conversions above, which are handed *errors* and
/// therefore owe the accounting.
#[test]
fn from_container_adopts_and_starts_the_accounting_from_there() {
  type Small<'a> = Errors<&'a str, ConstGenericArrayDeque<&'a str, 1>>;

  let mut prefilled = ConstGenericArrayDeque::<&str, 1>::new();
  assert!(prefilled.push_back("already here").is_none());

  let mut errors: Small<'_> = Errors::from_container(prefilled);
  assert!(!errors.overflowed(), "nothing has been offered yet");

  errors.push("offered and refused");
  assert!(errors.overflowed());
}

/// What the removed doors took with them, and what replaced it: reading, iterating, mutating an
/// element in place, and removing are all still reachable — only *inserting* is now funnelled.
#[cfg(any(feature = "alloc", feature = "std"))]
#[test]
fn reading_element_mutation_and_removal_survive_the_removed_mutable_door() {
  let mut errors: Errors<i32> = Errors::new();
  errors.push(1);
  errors.push(2);

  // Shared views, through `Deref`.
  assert_eq!(errors.len(), 2);
  assert_eq!(errors.front(), Some(&1));
  assert_eq!(errors.iter().sum::<i32>(), 3);

  // Element mutation: `&mut E` per element, and no way to change how many there are.
  for e in errors.iter_mut() {
    *e *= 10;
  }
  assert_eq!(
    errors.iter().copied().collect::<std::vec::Vec<_>>(),
    std::vec![10, 20]
  );

  // Removal.
  assert_eq!(errors.pop(), Some(10));
  errors.clear();
  assert!(errors.is_empty());
}
