use super::*;
use crate::span::SimpleSpan;

use std::format;

#[test]
fn full_container_new() {
  let err = FullContainer::new(SimpleSpan::new(0, 5), 10, 5);
  assert_eq!(*err.span(), SimpleSpan::new(0, 5));
  assert_eq!(err.nums(), 10);
  assert_eq!(err.capacity(), 5);
}

#[test]
fn full_container_of_with_lang() {
  struct MyLang;
  let err = FullContainer::<SimpleSpan, MyLang>::of(SimpleSpan::new(0, 5), 10, 5);
  assert_eq!(err.nums(), 10);
  assert_eq!(err.capacity(), 5);
}

#[test]
fn full_container_bump() {
  let mut err = FullContainer::new(SimpleSpan::new(0, 5), 10, 5);
  err.bump(&10);
  assert_eq!(*err.span(), SimpleSpan::new(10, 15));
}

#[test]
fn full_container_into_unit() {
  let err = FullContainer::new(SimpleSpan::new(0, 5), 10, 5);
  let _: () = err.into();
}

#[test]
fn full_container_display() {
  let err = FullContainer::new(SimpleSpan::new(2, 8), 10, 5);
  let msg = format!("{err}");
  assert_eq!(
    msg,
    "element 10 of this construct was refused by a destination that holds at most 5"
  );
}

/// The refusal is stated even when the two numbers cannot be ordered — a destination already
/// occupied when the construct started refuses the first element it is handed, at any capacity.
/// The old wording made this render "found 1 elements, which exceeds the maximum capacity of 1".
#[test]
fn full_container_display_does_not_claim_an_exceedance() {
  let err = FullContainer::new(SimpleSpan::new(0, 1), 1, 1);
  assert_eq!(
    format!("{err}"),
    "element 1 of this construct was refused by a destination that holds at most 1"
  );
}
