//! Coverage tests for scattered uncovered lines in error types, type traits,
//! and the `ErrorContainer` trait defaults.

#![cfg(feature = "std")]
#![allow(warnings)]

use tokora::{
  SimpleSpan,
  error::token::{MissingToken, UnexpectedToken},
  error::{ErrorContainer, Errors, InvalidHexDigits},
  span::AsSpan,
  utils::{CowStr, PositionedChar},
};

// ── InvalidHexDigits ──────────────────────────────────────────────────────────

#[test]
fn invalid_hex_digits_from_single_element_array() {
  // Exercises lines 122-124: From<[PositionedChar<Char, O>; 1]>
  let pc = PositionedChar::with_position('G', 10usize);
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from([pc]);
  assert_eq!(digits.len(), 1);
  assert_eq!(digits[0].position(), 10);
}

#[test]
fn invalid_hex_digits_try_from_iter_success() {
  // Exercises lines 209 and 213: try_from_iter
  let chars = vec![
    PositionedChar::with_position('G', 10usize),
    PositionedChar::with_position('H', 11usize),
  ];
  let result: Option<InvalidHexDigits<char, 2>> = InvalidHexDigits::try_from_iter(chars);
  assert!(result.is_some());
  let digits = result.unwrap();
  assert_eq!(digits.len(), 2);
}

#[test]
fn invalid_hex_digits_try_from_iter_overflow() {
  // Also exercises try_from_iter with too many items
  let chars = vec![
    PositionedChar::with_position('A', 0usize),
    PositionedChar::with_position('B', 1usize),
    PositionedChar::with_position('C', 2usize),
  ];
  // Capacity is 2, so 3 items should fail
  let result: Option<InvalidHexDigits<char, 2>> = InvalidHexDigits::try_from_iter(chars);
  assert!(result.is_none());
}

#[test]
fn invalid_hex_digits_push_char() {
  // Exercises lines 250-251: push_char
  let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10usize, 'G');
  assert_eq!(digits.len(), 1);
  let pushed = digits.push_char(11usize, 'H');
  assert!(pushed);
  assert_eq!(digits.len(), 2);
  // Now full — pushing again should return false
  let not_pushed = digits.push_char(12usize, 'I');
  assert!(!not_pushed);
}

#[test]
fn invalid_hex_digits_as_ref_slice() {
  // Exercises lines 330-331: AsRef<[PositionedChar<Char, O>]>
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(5usize, 'Z');
  let slice: &[PositionedChar<char, usize>] = digits.as_ref();
  assert_eq!(slice.len(), 1);
  assert_eq!(slice[0].position(), 5);
}

#[test]
fn invalid_hex_digits_as_mut_slice() {
  // Exercises lines 340-341: AsMut<[PositionedChar<Char, O>]>
  let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(5usize, 'Z');
  {
    let slice: &mut [PositionedChar<char, usize>] = digits.as_mut();
    assert_eq!(slice.len(), 1);
  }
  // Verify the slice can be mutated
  assert_eq!(digits.len(), 1);
}

#[test]
fn invalid_hex_digits_deref_mut() {
  // Exercises lines 362-363: DerefMut
  use core::ops::DerefMut;
  let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10usize, 'G');
  digits.push(PositionedChar::with_position('H', 11usize));
  // DerefMut to get a mutable slice
  let slice: &mut [PositionedChar<char, usize>] = digits.deref_mut();
  assert_eq!(slice.len(), 2);
}

// ── Errors: AsRef<[E]> and AsMut<[E]> ────────────────────────────────────────
//
// The `AsRef<[E]>` and `AsMut<[E]>` impls on `Errors<E, C>` require `C: AsRef<[E]>`
// / `C: AsMut<[E]>`. In std mode the default container is `VecDeque<E>` which does
// NOT implement `AsRef<[E]>`, so we use `Vec<E>` explicitly as the container.

#[test]
fn errors_as_ref_slice() {
  // Exercises errors.rs lines 276-277: AsRef<[E]> for Errors<E, Vec<E>>
  let mut errors: Errors<i32, Vec<i32>> = Errors::from_container(Vec::new());
  errors.push(1);
  errors.push(2);
  let slice: &[i32] = errors.as_ref();
  assert_eq!(slice, &[1, 2]);
}

#[test]
fn errors_as_mut_slice() {
  // Exercises errors.rs lines 286-287: AsMut<[E]> for Errors<E, Vec<E>>
  let mut errors: Errors<i32, Vec<i32>> = Errors::from_container(Vec::new());
  errors.push(10);
  errors.push(20);
  {
    let slice: &mut [i32] = errors.as_mut();
    assert_eq!(slice.len(), 2);
    slice[0] = 99;
  }
  // Verify the mutation took effect
  let final_slice: &[i32] = errors.as_ref();
  assert_eq!(final_slice[0], 99);
}

// ── ErrorContainer default methods: with_capacity and try_push ───────────────
//
// Lines 426 and 430 in error/mod.rs are the default implementations of
// `with_capacity` and `try_push` in the `ErrorContainer` trait. These defaults
// are used by implementations that don't override them. We test them by using
// a custom container that relies on the defaults.

/// A minimal custom ErrorContainer that only implements the required methods,
/// relying on the default implementations of `with_capacity` and `try_push`.
struct MinimalContainer<E> {
  items: Vec<E>,
}

impl<E> ErrorContainer<E> for MinimalContainer<E> {
  type IntoIter = std::vec::IntoIter<E>;
  type Iter<'a>
    = core::slice::Iter<'a, E>
  where
    E: 'a;

  fn new() -> Self {
    Self { items: Vec::new() }
  }

  fn push(&mut self, error: E) {
    self.items.push(error);
  }

  fn pop(&mut self) -> Option<E> {
    if self.items.is_empty() {
      None
    } else {
      Some(self.items.remove(0))
    }
  }

  fn len(&self) -> usize {
    self.items.len()
  }

  fn iter(&self) -> Self::Iter<'_> {
    self.items.iter()
  }

  fn into_iter(self) -> Self::IntoIter {
    IntoIterator::into_iter(self.items)
  }

  // NOTE: does NOT override `with_capacity` or `try_push`
  // so those default implementations will be invoked.
}

#[test]
fn error_container_default_with_capacity() {
  // Exercises line 426 in error/mod.rs: default `with_capacity` calls `Self::new()`
  let container: MinimalContainer<i32> = ErrorContainer::with_capacity(42);
  assert_eq!(ErrorContainer::len(&container), 0);
}

#[test]
fn error_container_default_try_push() {
  // Exercises line 430 in error/mod.rs: default `try_push` calls `push` and returns Ok(())
  let mut container: MinimalContainer<i32> = ErrorContainer::new();
  let result = ErrorContainer::try_push(&mut container, 99);
  assert!(result.is_ok());
  assert_eq!(ErrorContainer::len(&container), 1);
}

// ── ErrorContainer default is_empty ──────────────────────────────────────────

#[test]
fn error_container_default_is_empty() {
  // Exercises the default `is_empty` implementation (delegates to len() == 0)
  let container: MinimalContainer<i32> = ErrorContainer::new();
  assert!(ErrorContainer::is_empty(&container));
}

// ── Recoverable: AsSpan impl (types/mod.rs lines 114-117) ────────────────────
//
// The AsSpan impl for Recoverable has three branches:
//   Node(node) => node.as_span()   [line 116]
//   Error(span) | Missing(span) => span  [line 117]

use tokora::types::Recoverable;

/// A simple wrapper that implements AsSpan<SimpleSpan>.
#[derive(Debug, Clone)]
struct SpannedValue {
  span: SimpleSpan,
}

impl AsSpan<SimpleSpan> for SpannedValue {
  fn as_span(&self) -> &SimpleSpan {
    &self.span
  }
}

#[test]
fn recoverable_as_span_node_branch() {
  // Exercises line 116: Node branch of AsSpan for Recoverable
  let inner = SpannedValue {
    span: SimpleSpan::new(10, 20),
  };
  let r: Recoverable<SpannedValue> = Recoverable::Node(inner);
  assert_eq!(*r.as_span(), SimpleSpan::new(10, 20));
}

#[test]
fn recoverable_as_span_error_branch() {
  // Exercises line 117 (Error variant): AsSpan for Recoverable
  let r: Recoverable<SpannedValue> = Recoverable::Error(SimpleSpan::new(5, 15));
  assert_eq!(*r.as_span(), SimpleSpan::new(5, 15));
}

#[test]
fn recoverable_as_span_missing_branch() {
  // Exercises line 117 (Missing variant): AsSpan for Recoverable
  let r: Recoverable<SpannedValue> = Recoverable::Missing(SimpleSpan::new(0, 7));
  assert_eq!(*r.as_span(), SimpleSpan::new(0, 7));
}

// ── Recoverable: Syntax impl (types/mod.rs lines 135, 137, 140, 142) ─────────
//
// The Syntax impl for Recoverable<T> delegates to T::possible_components()
// and T::required_components(). We need a concrete T implementing Syntax.

use tokora::{
  syntax::{Language, Syntax},
  utils::{GenericArrayDeque, typenum::U2},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TestLang;

impl Language for TestLang {
  type SyntaxKind = ();
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TestComponent {
  First,
  Second,
}

impl core::fmt::Display for TestComponent {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::First => write!(f, "first"),
      Self::Second => write!(f, "second"),
    }
  }
}

struct TestSyntaxNode;

impl Syntax for TestSyntaxNode {
  type Lang = TestLang;
  const KIND: () = ();
  type Component = TestComponent;
  type COMPONENTS = U2;
  type REQUIRED = U2;

  fn possible_components() -> &'static GenericArrayDeque<Self::Component, Self::COMPONENTS> {
    static COMPONENTS: GenericArrayDeque<TestComponent, U2> = {
      let mut deque = GenericArrayDeque::new();
      deque.push_back(TestComponent::First);
      deque.push_back(TestComponent::Second);
      deque
    };
    &COMPONENTS
  }

  fn required_components() -> &'static GenericArrayDeque<Self::Component, Self::REQUIRED> {
    static REQUIRED: GenericArrayDeque<TestComponent, U2> = {
      let mut deque = GenericArrayDeque::new();
      deque.push_back(TestComponent::First);
      deque.push_back(TestComponent::Second);
      deque
    };
    &REQUIRED
  }
}

#[test]
fn recoverable_syntax_possible_components() {
  // Exercises lines 135-137: possible_components() delegation via Recoverable<T>
  let components = <Recoverable<TestSyntaxNode> as Syntax>::possible_components();
  assert_eq!(components.len(), 2);
}

#[test]
fn recoverable_syntax_required_components() {
  // Exercises lines 140-142: required_components() delegation via Recoverable<T>
  let required = <Recoverable<TestSyntaxNode> as Syntax>::required_components();
  assert_eq!(required.len(), 2);
}

// ── bytes_1::Bytes ErrorNode (error/mod.rs lines 371-372, 376-377) ───────────
//
// These lines are only compiled when the `bytes_1` feature is enabled.

#[cfg(feature = "bytes_1")]
mod bytes_error_node {
  use bytes_1::Bytes;
  use tokora::{SimpleSpan, error::ErrorNode};

  #[test]
  fn bytes_error_node_error() {
    // Exercises line 371-372
    let node = <Bytes as ErrorNode>::error(SimpleSpan::new(0, 5));
    assert_eq!(node.as_ref(), b"<error>");
  }

  #[test]
  fn bytes_error_node_missing() {
    // Exercises line 376-377
    let node = <Bytes as ErrorNode>::missing(SimpleSpan::new(0, 5));
    assert_eq!(node.as_ref(), b"<missing>");
  }
}

// ── MissingToken constructors, accessors, and formatting ─────────────────────
//
// Merged from missing_token.rs (was gated with #![cfg(feature = "std")];
// that gate is already applied file-wide above).

#[test]
fn new_constructor() {
  let mt: MissingToken<'_, (), usize> = MissingToken::new(99);
  assert_eq!(*mt.offset_ref(), 99);
}

#[test]
fn with_message_builder() {
  let mt: MissingToken<'_, (), usize> =
    MissingToken::new(0).with_message(CowStr::from_static("hello"));
  assert_eq!(mt.message().unwrap().as_str(), "hello");
}

#[test]
fn offset_copy() {
  let mt: MissingToken<'_, (), usize> = MissingToken::new(42);
  assert_eq!(mt.offset(), 42);
}

#[test]
fn offset_mut() {
  let mut mt: MissingToken<'_, (), usize> = MissingToken::new(0);
  *mt.offset_mut() = 100;
  assert_eq!(mt.offset(), 100);
}

#[test]
fn message_mut() {
  let mut mt: MissingToken<'_, (), usize> =
    MissingToken::new(0).with_message(CowStr::from_static("old"));
  if let Some(m) = mt.message_mut() {
    *m = CowStr::from_static("new");
  }
  assert_eq!(mt.message().unwrap().as_str(), "new");
}

#[test]
fn into_components() {
  let mt: MissingToken<'_, (), usize> = MissingToken::new(42)
    .with_message(CowStr::from_static("msg"))
    .with_name(CowStr::from_static("comma"));
  let (off, exp, msg, name) = mt.into_components();
  assert_eq!(off, 42);
  assert!(exp.is_none());
  assert_eq!(msg.unwrap().as_str(), "msg");
  assert_eq!(name.unwrap().as_str(), "comma");
}

#[test]
fn display_formatting() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let mt: MissingToken<'_, &str, usize> = MissingToken::new(0);
  let _ = format!("{}", Show(mt));
}

#[test]
fn debug_formatting() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Debug for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.debug_fmt(f)
    }
  }

  let mt: MissingToken<'_, &str, usize> = MissingToken::new(0);
  let _ = format!("{:?}", Show(mt));
}

#[test]
fn from_missing_token_for_unit() {
  let mt: MissingToken<'_, (), usize> = MissingToken::new(0);
  let _: () = mt.into();
}
