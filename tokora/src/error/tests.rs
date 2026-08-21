use super::*;
use std::{vec, vec::Vec};

// --- ErrorNode for &str ---

#[test]
fn error_node_str_error() {
  let node = <&str as ErrorNode>::error(SimpleSpan::new(0, 5));
  assert_eq!(node, "<error>");
}

#[test]
fn error_node_str_missing() {
  let node = <&str as ErrorNode>::missing(SimpleSpan::new(0, 5));
  assert_eq!(node, "<missing>");
}

// --- ErrorNode for &[u8] ---

#[test]
fn error_node_bytes_error() {
  let node = <&[u8] as ErrorNode>::error(SimpleSpan::new(0, 5));
  assert_eq!(node, b"<error>");
}

#[test]
fn error_node_bytes_missing() {
  let node = <&[u8] as ErrorNode>::missing(SimpleSpan::new(0, 5));
  assert_eq!(node, b"<missing>");
}

// --- ErrorContainer for Option<E> ---

#[test]
fn option_error_container_new() {
  let c: Option<i32> = ErrorContainer::new();
  assert!(c.is_none());
}

#[test]
fn option_error_container_push_and_len() {
  let mut c: Option<i32> = ErrorContainer::new();
  assert_eq!(ErrorContainer::len(&c), 0);
  assert!(ErrorContainer::is_empty(&c));
  ErrorContainer::push(&mut c, 42);
  assert_eq!(ErrorContainer::len(&c), 1);
  assert!(!ErrorContainer::is_empty(&c));
}

#[test]
fn option_error_container_push_keeps_first() {
  let mut c: Option<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 1);
  ErrorContainer::push(&mut c, 2);
  assert_eq!(c, Some(1)); // get_or_insert keeps the first
}

#[test]
fn option_error_container_try_push() {
  let mut c: Option<i32> = ErrorContainer::new();
  assert!(ErrorContainer::try_push(&mut c, 1).is_ok());
  assert!(ErrorContainer::try_push(&mut c, 2).is_err());
}

#[test]
fn option_error_container_pop() {
  let mut c: Option<i32> = ErrorContainer::new();
  assert_eq!(ErrorContainer::pop(&mut c), None);
  ErrorContainer::push(&mut c, 10);
  assert_eq!(ErrorContainer::pop(&mut c), Some(10));
  assert_eq!(ErrorContainer::pop(&mut c), None);
}

#[test]
fn option_error_container_iter() {
  let mut c: Option<i32> = ErrorContainer::new();
  assert_eq!(ErrorContainer::iter(&c).count(), 0);
  ErrorContainer::push(&mut c, 5);
  let items: Vec<_> = ErrorContainer::iter(&c).collect();
  assert_eq!(items, vec![&5]);
}

#[test]
fn option_error_container_into_iter() {
  let mut c: Option<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 99);
  let items: Vec<_> = ErrorContainer::into_iter(c).collect();
  assert_eq!(items, vec![99]);
}

#[test]
fn option_error_container_remaining_capacity() {
  let mut c: Option<i32> = ErrorContainer::new();
  assert_eq!(ErrorContainer::remaining_capacity(&c), Some(1));
  ErrorContainer::push(&mut c, 1);
  assert_eq!(ErrorContainer::remaining_capacity(&c), Some(0));
}

// --- ErrorContainer for Vec<E> ---

#[test]
fn vec_error_container_new() {
  let c: Vec<i32> = ErrorContainer::new();
  assert!(c.is_empty());
}

#[test]
fn vec_error_container_push_and_len() {
  let mut c: Vec<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 1);
  ErrorContainer::push(&mut c, 2);
  assert_eq!(ErrorContainer::len(&c), 2);
}

#[test]
fn vec_error_container_pop() {
  let mut c: Vec<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 10);
  ErrorContainer::push(&mut c, 20);
  assert_eq!(ErrorContainer::pop(&mut c), Some(10));
  assert_eq!(ErrorContainer::pop(&mut c), Some(20));
  assert_eq!(ErrorContainer::pop(&mut c), None);
}

#[test]
fn vec_error_container_with_capacity() {
  let c: Vec<i32> = ErrorContainer::with_capacity(10);
  assert!(c.is_empty());
}

#[test]
fn vec_error_container_remaining_capacity_is_none() {
  let c: Vec<i32> = ErrorContainer::new();
  assert_eq!(ErrorContainer::remaining_capacity(&c), None);
}

#[test]
fn vec_error_container_iter() {
  let mut c: Vec<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 1);
  ErrorContainer::push(&mut c, 2);
  let items: Vec<_> = ErrorContainer::iter(&c).collect();
  assert_eq!(items, vec![&1, &2]);
}

#[test]
fn vec_error_container_into_iter() {
  let mut c: Vec<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 10);
  ErrorContainer::push(&mut c, 20);
  let items: Vec<_> = ErrorContainer::into_iter(c).collect();
  assert_eq!(items, vec![10, 20]);
}

#[test]
fn vec_error_container_is_empty() {
  let c: Vec<i32> = ErrorContainer::new();
  assert!(ErrorContainer::is_empty(&c));
}

#[test]
fn vec_error_container_pop_empty() {
  let mut c: Vec<i32> = ErrorContainer::new();
  assert_eq!(ErrorContainer::pop(&mut c), None);
}

// --- ErrorContainer for VecDeque<E> ---

#[test]
fn vecdeque_error_container_new() {
  use std::collections::VecDeque;
  let c: VecDeque<i32> = ErrorContainer::new();
  assert!(c.is_empty());
}

#[test]
fn vecdeque_error_container_with_capacity() {
  use std::collections::VecDeque;
  let c: VecDeque<i32> = ErrorContainer::with_capacity(10);
  assert!(c.is_empty());
}

#[test]
fn vecdeque_error_container_push_pop_len() {
  use std::collections::VecDeque;
  let mut c: VecDeque<i32> = ErrorContainer::new();
  assert_eq!(ErrorContainer::len(&c), 0);
  ErrorContainer::push(&mut c, 1);
  ErrorContainer::push(&mut c, 2);
  assert_eq!(ErrorContainer::len(&c), 2);
  assert_eq!(ErrorContainer::pop(&mut c), Some(1));
  assert_eq!(ErrorContainer::pop(&mut c), Some(2));
  assert_eq!(ErrorContainer::pop(&mut c), None);
}

#[test]
fn vecdeque_error_container_iter() {
  use std::collections::VecDeque;
  let mut c: VecDeque<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 5);
  ErrorContainer::push(&mut c, 6);
  let items: Vec<_> = ErrorContainer::iter(&c).collect();
  assert_eq!(items, vec![&5, &6]);
}

#[test]
fn vecdeque_error_container_into_iter() {
  use std::collections::VecDeque;
  let mut c: VecDeque<i32> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 99);
  let items: Vec<_> = ErrorContainer::into_iter(c).collect();
  assert_eq!(items, vec![99]);
}

// --- ErrorContainer for GenericArrayDeque ---

#[test]
fn arraydeque_error_container_new() {
  use generic_arraydeque::typenum::U4;
  let c: GenericArrayDeque<i32, U4> = ErrorContainer::new();
  assert!(ErrorContainer::is_empty(&c));
}

#[test]
fn arraydeque_error_container_push_pop_len() {
  use generic_arraydeque::typenum::U4;
  let mut c: GenericArrayDeque<i32, U4> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 10);
  ErrorContainer::push(&mut c, 20);
  assert_eq!(ErrorContainer::len(&c), 2);
  assert_eq!(ErrorContainer::pop(&mut c), Some(10));
}

#[test]
fn arraydeque_error_container_try_push() {
  use generic_arraydeque::typenum::U2;
  let mut c: GenericArrayDeque<i32, U2> = ErrorContainer::new();
  assert!(ErrorContainer::try_push(&mut c, 1).is_ok());
  assert!(ErrorContainer::try_push(&mut c, 2).is_ok());
  assert!(ErrorContainer::try_push(&mut c, 3).is_err());
}

#[test]
fn arraydeque_error_container_iter() {
  use generic_arraydeque::typenum::U4;
  let mut c: GenericArrayDeque<i32, U4> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 1);
  let items: Vec<_> = ErrorContainer::iter(&c).collect();
  assert_eq!(items, vec![&1]);
}

#[test]
fn arraydeque_error_container_into_iter() {
  use generic_arraydeque::typenum::U4;
  let mut c: GenericArrayDeque<i32, U4> = ErrorContainer::new();
  ErrorContainer::push(&mut c, 42);
  let items: Vec<_> = ErrorContainer::into_iter(c).collect();
  assert_eq!(items, vec![42]);
}

#[test]
fn arraydeque_error_container_remaining_capacity() {
  use generic_arraydeque::typenum::U3;
  let mut c: GenericArrayDeque<i32, U3> = ErrorContainer::new();
  assert_eq!(ErrorContainer::remaining_capacity(&c), Some(3));
  ErrorContainer::push(&mut c, 1);
  assert_eq!(ErrorContainer::remaining_capacity(&c), Some(2));
}

// --- UnexpectedEnd::reconstruct* carry `expected` and `terminal` (#300) ---

#[test]
fn unexpected_end_reconstruct_carries_expected_and_terminal() {
  use crate::utils::Expected;

  let e: UnexpectedEnd<FileHint, usize> =
    UnexpectedEnd::maybe_expected_of(100usize, None, FileHint, Some(Expected::one("if")))
      .into_terminal();
  assert!(e.is_terminal());
  assert_eq!(e.expected(), Some(&Expected::one("if")));

  let e2 = e.reconstruct(Some("block"), |_| TokenHint);
  assert_eq!(e2.name(), Some("block"));
  assert_eq!(e2.offset(), 100);
  assert!(
    e2.is_terminal(),
    "reconstruct must not clear the terminal-stop flag"
  );
  assert_eq!(
    e2.expected(),
    Some(&Expected::one("if")),
    "reconstruct must not drop the expected set"
  );
}

#[test]
fn unexpected_end_reconstruct_with_name_carries_expected_and_terminal() {
  use crate::utils::Expected;

  let e: UnexpectedEnd<FileHint, usize> =
    UnexpectedEnd::maybe_expected_of(100usize, None, FileHint, Some(Expected::one("if")))
      .into_terminal();

  let e2 = e.reconstruct_with_name("expression", |_| TokenHint);
  assert_eq!(e2.name(), Some("expression"));
  assert_eq!(e2.offset(), 100);
  assert!(
    e2.is_terminal(),
    "reconstruct_with_name must not clear the terminal-stop flag"
  );
  assert_eq!(
    e2.expected(),
    Some(&Expected::one("if")),
    "reconstruct_with_name must not drop the expected set"
  );
}

#[test]
fn unexpected_end_reconstruct_without_name_carries_expected_and_terminal() {
  use crate::utils::Expected;

  let e: UnexpectedEnd<FileHint, usize> =
    UnexpectedEnd::maybe_expected_of(100usize, None, FileHint, Some(Expected::one("if")))
      .into_terminal();

  let e2 = e.reconstruct_without_name(|_| TokenHint);
  assert_eq!(e2.name(), None);
  assert_eq!(e2.offset(), 100);
  assert!(
    e2.is_terminal(),
    "reconstruct_without_name must not clear the terminal-stop flag"
  );
  assert_eq!(
    e2.expected(),
    Some(&Expected::one("if")),
    "reconstruct_without_name must not drop the expected set"
  );
}
