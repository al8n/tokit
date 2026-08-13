use super::*;
use generic_arraydeque::ConstGenericArrayDeque;

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
