//! Unit tests for pure data types that don't require a lexer:
//! `Located`, `Sliced`, `IncompleteSyntax`, marker utilities, `Expected`, `ErrorNode`.

use core::fmt;

use tokora::{
  Located, SimpleSpan,
  error::{ErrorNode, IncompleteSyntax},
  slice::Sliced,
  syntax::{Language, Syntax},
  utils::{
    Expected, GenericArrayDeque, OneOf,
    marker::{Ignored, PhantomDelimited, PhantomLocated, PhantomSliced, PhantomSpan},
    typenum::{U0, U1, U2, U3},
  },
};

// ── Shared Syntax/Language setup ─────────────────────────────────────────────

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TestLang;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TestSyntaxKind;

impl fmt::Display for TestSyntaxKind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "test-syntax")
  }
}

impl Language for TestLang {
  type SyntaxKind = TestSyntaxKind;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Component {
  A,
  B,
  C,
}

impl fmt::Display for Component {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::A => write!(f, "A"),
      Self::B => write!(f, "B"),
      Self::C => write!(f, "C"),
    }
  }
}

// Syntax with 2 components
#[derive(Debug, Clone, Copy)]
struct MySyntax2;
impl Syntax for MySyntax2 {
  type Lang = TestLang;
  const KIND: TestSyntaxKind = TestSyntaxKind;
  type Component = Component;
  type COMPONENTS = U2;
  type REQUIRED = U2;
  fn possible_components() -> &'static GenericArrayDeque<Component, U2> {
    const C: &GenericArrayDeque<Component, U2> =
      &GenericArrayDeque::from_array([Component::A, Component::B]);
    C
  }
  fn required_components() -> &'static GenericArrayDeque<Component, U2> {
    const C: &GenericArrayDeque<Component, U2> =
      &GenericArrayDeque::from_array([Component::A, Component::B]);
    C
  }
}

// Syntax with 3 components
#[derive(Debug, Clone, Copy)]
struct MySyntax3;
impl Syntax for MySyntax3 {
  type Lang = TestLang;
  const KIND: TestSyntaxKind = TestSyntaxKind;
  type Component = Component;
  type COMPONENTS = U3;
  type REQUIRED = U3;
  fn possible_components() -> &'static GenericArrayDeque<Component, U3> {
    const C: &GenericArrayDeque<Component, U3> =
      &GenericArrayDeque::from_array([Component::A, Component::B, Component::C]);
    C
  }
  fn required_components() -> &'static GenericArrayDeque<Component, U3> {
    const C: &GenericArrayDeque<Component, U3> =
      &GenericArrayDeque::from_array([Component::A, Component::B, Component::C]);
    C
  }
}

// Syntax with 1 component
#[derive(Debug, Clone, Copy)]
struct MySyntax1;
impl Syntax for MySyntax1 {
  type Lang = TestLang;
  const KIND: TestSyntaxKind = TestSyntaxKind;
  type Component = Component;
  type COMPONENTS = U1;
  type REQUIRED = U1;
  fn possible_components() -> &'static GenericArrayDeque<Component, U1> {
    const C: &GenericArrayDeque<Component, U1> = &GenericArrayDeque::from_array([Component::A]);
    C
  }
  fn required_components() -> &'static GenericArrayDeque<Component, U1> {
    const C: &GenericArrayDeque<Component, U1> = &GenericArrayDeque::from_array([Component::A]);
    C
  }
}

// Syntax with 0 components
#[derive(Debug, Clone, Copy)]
#[allow(unused)]
struct MySyntax0;

impl Syntax for MySyntax0 {
  type Lang = TestLang;
  const KIND: TestSyntaxKind = TestSyntaxKind;
  type Component = Component;
  type COMPONENTS = U0;
  type REQUIRED = U0;
  fn possible_components() -> &'static GenericArrayDeque<Component, U0> {
    const C: &GenericArrayDeque<Component, U0> = &GenericArrayDeque::from_array([]);
    C
  }
  fn required_components() -> &'static GenericArrayDeque<Component, U0> {
    const C: &GenericArrayDeque<Component, U0> = &GenericArrayDeque::from_array([]);
    C
  }
}

// ── IncompleteSyntax tests ────────────────────────────────────────────────────

#[test]
fn incomplete_syntax_new() {
  let e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(10, 15), Component::A);
  assert_eq!(e.len(), 1);
  assert_eq!(e.span(), SimpleSpan::new(10, 15));
}

#[test]
fn incomplete_syntax_span_ref() {
  let e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(5, 10), Component::A);
  assert_eq!(e.span_ref(), &SimpleSpan::new(5, 10));
}

#[test]
fn incomplete_syntax_span_mut() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(5, 10), Component::A);
  *e.span_mut() = SimpleSpan::new(1, 2);
  assert_eq!(e.span(), SimpleSpan::new(1, 2));
}

#[test]
fn incomplete_syntax_capacity() {
  let e = IncompleteSyntax::<MySyntax3>::new(SimpleSpan::new(0, 1), Component::A);
  assert_eq!(e.capacity(), 3);
}

#[test]
fn incomplete_syntax_is_full() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::A);
  assert!(!e.is_full());
  e.push(Component::B);
  assert!(e.is_full());
}

#[test]
fn incomplete_syntax_push() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::A);
  e.push(Component::B);
  assert_eq!(e.len(), 2);
  // Duplicate push is no-op
  e.push(Component::A);
  assert_eq!(e.len(), 2);
}

#[test]
fn incomplete_syntax_push_front() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::B);
  e.push_front(Component::A);
  assert_eq!(e.len(), 2);
  // Both components are present
  let collected: Vec<_> = e.iter().collect();
  assert!(collected.contains(&&Component::A));
  assert!(collected.contains(&&Component::B));
}

#[test]
fn incomplete_syntax_push_front_duplicate_noop() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::A);
  e.push_front(Component::A);
  assert_eq!(e.len(), 1);
}

#[test]
fn incomplete_syntax_try_push() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::A);
  // Success: returns None
  assert!(e.try_push(Component::B).is_none());
  // Overflow: returns Some
  assert_eq!(e.try_push(Component::C), Some(Component::C));
}

#[test]
fn incomplete_syntax_try_push_front() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::A);
  assert!(e.try_push_front(Component::B).is_none());
  assert_eq!(e.try_push_front(Component::C), Some(Component::C));
}

#[test]
fn incomplete_syntax_as_slice() {
  let e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  assert_eq!(e.as_slice(), &[Component::A]);
}

// ── The wrapped ring ─────────────────────────────────────────────────────────
//
// `new` + `push_front` is what puts the logical sequence across the ring boundary: the head
// moves to the last physical cell, so the deque has two segments and an accessor returning
// only the first reports a prefix. Capacity one cannot express that layout, so the
// capacity-one slice tests here pass against the defect — these are the ones that do not.

#[test]
fn incomplete_syntax_wrapped_slice_holds_every_component() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::B);
  e.push_front(Component::A);

  assert_eq!(e.len(), 2);
  assert_eq!(
    e.iter().collect::<Vec<_>>(),
    vec![&Component::A, &Component::B]
  );
  // The views agree, which is what `len`/`iter` and the slice facade stopped doing.
  assert_eq!(e.as_slice(), &[Component::A, Component::B]);
  assert_eq!(e.as_slice().len(), e.len());
  let via_as_ref: &[Component] = e.as_ref();
  assert_eq!(via_as_ref, &[Component::A, Component::B]);
}

#[test]
fn incomplete_syntax_wrapped_display_names_every_component() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::B);
  e.push_front(Component::A);
  // Not the singular "component A is missing": the count the grammar branches on is the
  // slice's, so a truncated slice changed the sentence as well as its contents.
  assert_eq!(
    format!("{e}"),
    "incomplete syntax: components A, B are missing"
  );
}

#[test]
fn incomplete_syntax_wrapped_then_pushed_back_holds_every_component() {
  // A back insertion after a front one: a second physical layout the one-segment accessor
  // reported a prefix of. Note what this one does NOT pin — removing the front door's
  // `make_contiguous` leaves it green, because the later `push` normalizes the ring the
  // `push_front` wrapped. The three tests around it are the ones that red on that plant.
  let mut e = IncompleteSyntax::<MySyntax3>::new(SimpleSpan::new(0, 1), Component::B);
  e.push_front(Component::A);
  e.push(Component::C);

  assert_eq!(e.len(), 3);
  assert_eq!(e.as_slice(), &[Component::A, Component::B, Component::C]);
  assert_eq!(
    e.iter().collect::<Vec<_>>(),
    vec![&Component::A, &Component::B, &Component::C]
  );
  assert_eq!(
    format!("{e}"),
    "incomplete syntax: components A, B, C are missing"
  );
}

#[test]
fn incomplete_syntax_wrapped_from_two_front_pushes() {
  let mut e = IncompleteSyntax::<MySyntax3>::new(SimpleSpan::new(0, 1), Component::C);
  e.push_front(Component::B);
  e.push_front(Component::A);

  assert_eq!(e.as_slice(), &[Component::A, Component::B, Component::C]);
  assert_eq!(e.as_slice().len(), e.len());
}

#[test]
fn incomplete_syntax_iter() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::A);
  e.push(Component::B);
  let collected: Vec<_> = e.iter().collect();
  assert_eq!(collected, vec![&Component::A, &Component::B]);
}

#[test]
fn incomplete_syntax_bump() {
  let mut e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(10, 15), Component::A);
  e.bump(&5);
  assert_eq!(e.span(), SimpleSpan::new(15, 20));
}

#[test]
fn incomplete_syntax_as_ref() {
  let e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  let slice: &[Component] = e.as_ref();
  assert_eq!(slice, &[Component::A]);
}

/// The impl that survived, pinned as a bound rather than as a call: the shared view is what
/// generic code takes, and it is the one whose fixed `&self` receiver forced the contiguity
/// discipline. `AsMut<[Component]>` is deliberately not implemented — see *The set is total*
/// on `IncompleteSyntax`.
#[test]
fn incomplete_syntax_satisfies_as_ref_bound() {
  fn takes_shared_slice<T: AsRef<[Component]>>(value: &T) -> usize {
    value.as_ref().len()
  }

  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::B);
  e.push_front(Component::A);
  assert_eq!(takes_shared_slice(&e), 2);
}

#[test]
fn incomplete_syntax_from_iter_some() {
  let e = IncompleteSyntax::<MySyntax2>::from_iter(
    SimpleSpan::new(0, 5),
    vec![Component::A, Component::B],
  );
  assert!(e.is_some());
  assert_eq!(e.unwrap().len(), 2);
}

#[test]
fn incomplete_syntax_from_iter_none_empty() {
  let e = IncompleteSyntax::<MySyntax2>::from_iter(
    SimpleSpan::new(0, 5),
    core::iter::empty::<Component>(),
  );
  assert!(e.is_none());
}

#[test]
fn incomplete_syntax_from_iter_dedup() {
  // Duplicates are silently ignored
  let e = IncompleteSyntax::<MySyntax2>::from_iter(
    SimpleSpan::new(0, 5),
    vec![Component::A, Component::A],
  );
  assert!(e.is_some());
  assert_eq!(e.unwrap().len(), 1);
}

// ── from_iter overflow ───────────────────────────────────────────────────────
//
// The documented behaviour on overflow is `None`, and that is what these assert. A test
// phrased as "we did not truncate" — say `len() == 3` — would also pass on a `from_iter` that
// grew the buffer, which is a different contract from the one the `Option` states.

#[test]
fn incomplete_syntax_from_iter_none_on_unique_overflow() {
  let e = IncompleteSyntax::<MySyntax2>::from_iter(
    SimpleSpan::new(0, 5),
    vec![Component::A, Component::B, Component::C],
  );
  assert!(
    e.is_none(),
    "a third unique component does not fit a two-component syntax, so the documented answer \
     is None — not Some holding the [A, B] prefix"
  );
}

#[test]
fn incomplete_syntax_from_iter_none_whatever_the_order() {
  // An early prefix must not win: with truncation the answer depended on which two components
  // the iterator happened to yield first, so every ordering has to be refused alike.
  for order in [
    vec![Component::A, Component::B, Component::C],
    vec![Component::A, Component::C, Component::B],
    vec![Component::B, Component::A, Component::C],
    vec![Component::B, Component::C, Component::A],
    vec![Component::C, Component::A, Component::B],
    vec![Component::C, Component::B, Component::A],
  ] {
    let e = IncompleteSyntax::<MySyntax2>::from_iter(SimpleSpan::new(0, 5), order.clone());
    assert!(e.is_none(), "expected None for {order:?}");
  }
}

#[test]
fn incomplete_syntax_from_iter_duplicates_do_not_count_as_overflow() {
  let e = IncompleteSyntax::<MySyntax2>::from_iter(
    SimpleSpan::new(0, 5),
    vec![Component::A, Component::A, Component::B],
  )
  .expect("two unique components fit a two-component syntax");
  assert_eq!(e.as_slice(), &[Component::A, Component::B]);
}

#[test]
fn incomplete_syntax_from_iter_none_when_duplicates_precede_the_overflow() {
  // The dedup pass must not be mistaken for headroom: `A, A, B` fills the buffer and `C` is
  // still a unique component with nowhere to go.
  let e = IncompleteSyntax::<MySyntax2>::from_iter(
    SimpleSpan::new(0, 5),
    vec![Component::A, Component::A, Component::B, Component::C],
  );
  assert!(e.is_none());
}

#[test]
fn incomplete_syntax_from_iter_exactly_capacity_keeps_every_component() {
  for order in [
    vec![Component::A, Component::B],
    vec![Component::B, Component::A],
  ] {
    let expected = order.clone();
    let e = IncompleteSyntax::<MySyntax2>::from_iter(SimpleSpan::new(0, 5), order)
      .expect("a full buffer is not an overflow");
    assert_eq!(e.len(), 2);
    assert_eq!(e.as_slice(), expected.as_slice());
  }
}

#[test]
fn incomplete_syntax_from_iter_stops_advancing_at_the_overflow() {
  // The refusal is immediate, so the component that overflowed is the last one taken.
  let mut taken = Vec::new();
  let e = IncompleteSyntax::<MySyntax2>::from_iter(
    SimpleSpan::new(0, 5),
    [Component::A, Component::B, Component::C, Component::A]
      .into_iter()
      .inspect(|c| taken.push(c.clone())),
  );
  assert!(e.is_none());
  assert_eq!(taken, vec![Component::A, Component::B, Component::C]);
}

#[test]
fn incomplete_syntax_display_single() {
  let e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  assert_eq!(format!("{e}"), "incomplete syntax: component A is missing");
}

#[test]
fn incomplete_syntax_display_multiple() {
  let mut e = IncompleteSyntax::<MySyntax2>::new(SimpleSpan::new(0, 1), Component::A);
  e.push(Component::B);
  assert_eq!(
    format!("{e}"),
    "incomplete syntax: components A, B are missing"
  );
}

#[test]
fn incomplete_syntax_debug() {
  let e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  let s = format!("{e:?}");
  assert!(s.contains("IncompleteSyntax") || s.contains("components"));
}

#[test]
fn incomplete_syntax_clone() {
  let e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  let e2 = e.clone();
  assert_eq!(e, e2);
}

#[test]
fn incomplete_syntax_eq() {
  let e1 = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  let e2 = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  assert_eq!(e1, e2);
}

#[test]
fn incomplete_syntax_hash() {
  use std::collections::HashSet;
  let mut set = HashSet::new();
  let e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  set.insert(e.clone());
  assert!(set.contains(&e));
}

#[test]
#[should_panic]
fn incomplete_syntax_push_overflow_panics() {
  let mut e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  e.push(Component::B); // This should panic: buffer full
}

#[test]
#[should_panic]
fn incomplete_syntax_push_front_overflow_panics() {
  let mut e = IncompleteSyntax::<MySyntax1>::new(SimpleSpan::new(0, 1), Component::A);
  e.push_front(Component::B); // This should panic: buffer full
}

// ── Interior mutability defeats uniqueness, not soundness (see `IncompleteSyntax`'s docs,
// "Uniqueness is a logic error") ────────────────────────────────────────────────────────────

#[test]
fn incomplete_syntax_interior_mutable_component_defeats_uniqueness_but_not_soundness() {
  // A component shaped exactly like the hazard the type-level docs describe: `Eq`, `Hash` and
  // `Display` are all derived from a `Cell`, so a caller holding only `&self` — exactly what
  // `as_slice`/`AsRef` hand out — can still change what they report.
  use core::{
    cell::Cell,
    hash::{Hash, Hasher},
  };
  use std::collections::hash_map::DefaultHasher;

  fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
  }

  #[derive(Debug, Clone)]
  struct MutableComponent(Cell<u8>);

  impl PartialEq for MutableComponent {
    fn eq(&self, other: &Self) -> bool {
      self.0.get() == other.0.get()
    }
  }
  impl Eq for MutableComponent {}

  impl Hash for MutableComponent {
    fn hash<H: Hasher>(&self, state: &mut H) {
      self.0.get().hash(state);
    }
  }

  impl fmt::Display for MutableComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(f, "{}", self.0.get())
    }
  }

  #[derive(Debug, Clone, Copy)]
  struct MutableSyntax;

  impl Syntax for MutableSyntax {
    type Lang = TestLang;
    const KIND: TestSyntaxKind = TestSyntaxKind;
    type Component = MutableComponent;
    type COMPONENTS = U2;
    type REQUIRED = U2;

    // `IncompleteSyntax::new`/`push`/`try_push`/`as_slice` — everything this test calls —
    // never call either accessor, so neither body runs here.
    fn possible_components() -> &'static GenericArrayDeque<MutableComponent, U2> {
      unimplemented!("not exercised by this test")
    }
    fn required_components() -> &'static GenericArrayDeque<MutableComponent, U2> {
      unimplemented!("not exercised by this test")
    }
  }

  let mut a =
    IncompleteSyntax::<MutableSyntax>::new(SimpleSpan::new(0, 1), MutableComponent(Cell::new(1)));
  a.push(MutableComponent(Cell::new(2)));

  let mut b =
    IncompleteSyntax::<MutableSyntax>::new(SimpleSpan::new(0, 1), MutableComponent(Cell::new(1)));
  b.push(MutableComponent(Cell::new(3))); // distinct from `a` — dedup saw 1 != 3 at insertion

  // Distinct as built: the deduplicating door told them apart using each component's `Eq` as
  // it stood at insertion time.
  assert_ne!(a, b);
  assert_ne!(format!("{a}"), format!("{b}"));

  // Mutate `b`'s second component through nothing but the shared accessor the type still
  // exposes: `as_slice()` hands out `&MutableComponent`, and `Cell::set` needs only `&self`.
  b.as_slice()[1].0.set(2);

  // "Every view is complete" is unconditional and untouched by any of this: both still report
  // two components — mutating a component does not change how many `IncompleteSyntax` holds.
  assert_eq!(a.len(), 2);
  assert_eq!(b.len(), 2);
  assert_eq!(b.as_slice().len(), 2);

  // A logic error, not undefined behaviour: equality between the two `IncompleteSyntax`
  // values, hashing one, and the message `Display` produces all now disagree with what `b`
  // was built from and agree with what its mutated component currently reports. This is one
  // shape the violation takes — `IncompleteSyntax`'s docs deliberately do not enumerate them —
  // and the fullness path below is another. (`hash_of` reads the value directly, rather than
  // putting it in a real `HashSet`/`HashMap`, which is its own footgun clippy already names:
  // `clippy::mutable_key_type` fires on exactly this key shape — corroborating evidence for the
  // same hazard, one level up.)
  assert_eq!(a, b);
  assert_eq!(format!("{a}"), format!("{b}"));
  assert_eq!(
    hash_of(&a),
    hash_of(&b),
    "a == b now holds, so a correct Hash impl is required to agree too"
  );

  // The deduplicating door is fooled the same way: a component that now reports what `b`'s
  // mutated component reports is absorbed as a duplicate — "admitting what looks like a
  // duplicate", exactly as documented.
  assert_eq!(b.try_push(MutableComponent(Cell::new(2))), None);
  assert_eq!(b.len(), 2);

  // The fullness path: this type also decides whether it has room, and that decision does not
  // re-derive itself from the current `Eq` view. `is_full` counts physical slots, and the slot
  // `b`'s mutated component still occupies was never freed.
  assert!(b.is_full());

  // `3` is what `b`'s second component reported before the mutation above overwrote it in
  // place. Pushed now, it is `Eq` to nothing `b` currently holds — a genuinely new component
  // by the deduplicating door's own rules — yet `b` is full, so `try_push` hands it back
  // instead of storing it.
  assert_eq!(
    b.try_push(MutableComponent(Cell::new(3))),
    Some(MutableComponent(Cell::new(3))),
    "3 is Eq to nothing b currently holds, so this is a genuinely new component — refused only \
     because the slot the mutation made b look free of was never physically freed"
  );
  assert_eq!(b.len(), 2);

  // `push` is `try_push` plus a panic on `Some` — a door whose only other documented failure
  // is the caller's own bug. It panics here too, on a component the deduplicating door has
  // never once called a duplicate.
  let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    b.push(MutableComponent(Cell::new(3)));
  }));
  assert!(
    panicked.is_err(),
    "push should panic: b reports is_full() and 3 is not a duplicate of anything it holds"
  );
}

// ── Located tests ─────────────────────────────────────────────────────────────

#[test]
fn located_new() {
  let loc = Located::new("file.rs", SimpleSpan::new(10, 15), "hello");
  assert_eq!(loc.slice(), "file.rs");
  assert_eq!(loc.span(), SimpleSpan::new(10, 15));
  assert_eq!(loc.data(), &"hello");
}

#[test]
fn located_deref() {
  let loc = Located::new("file.rs", SimpleSpan::new(0, 5), "hello");
  assert_eq!(loc.len(), 5); // deref to &str
  assert_eq!(*loc, "hello");
}

#[test]
fn located_deref_mut() {
  let mut loc = Located::new("file.rs", SimpleSpan::new(0, 2), 10i32);
  *loc += 5;
  assert_eq!(*loc, 15);
}

#[test]
fn located_display() {
  let loc = Located::new("file.rs", SimpleSpan::new(0, 5), 42i32);
  assert_eq!(format!("{loc}"), "42");
}

#[test]
fn located_slice_ref() {
  let loc = Located::new("config.toml", SimpleSpan::new(5, 10), "data");
  assert_eq!(loc.slice_ref(), &"config.toml");
}

#[test]
fn located_slice_mut() {
  let mut loc = Located::new("old.txt", SimpleSpan::new(0, 3), "data");
  *loc.slice_mut() = "new.txt";
  assert_eq!(loc.slice(), "new.txt");
}

#[test]
fn located_span_ref() {
  let loc = Located::new("file.rs", SimpleSpan::new(5, 10), "data");
  assert_eq!(loc.span_ref(), &SimpleSpan::new(5, 10));
}

#[test]
fn located_span_mut() {
  let mut loc = Located::new("file.rs", SimpleSpan::new(0, 5), "data");
  loc.span_mut().set_end(10);
  assert_eq!(loc.span().end(), 10);
}

#[test]
fn located_data_mut() {
  let mut loc = Located::new("file.txt", SimpleSpan::new(0, 2), 42i32);
  *loc.data_mut() = 100;
  assert_eq!(*loc.data(), 100);
}

#[test]
fn located_as_ref() {
  let loc = Located::new("file.txt", SimpleSpan::new(0, 5), 42i32);
  let borrowed = loc.as_ref();
  assert_eq!(**borrowed.data(), 42);
}

#[test]
fn located_as_mut() {
  let mut loc = Located::new(
    String::from("file.txt"),
    SimpleSpan::new(0, 5),
    String::from("hello"),
  );
  {
    let mut borrowed = loc.as_mut();
    borrowed.data_mut().push_str(" world");
  }
  assert_eq!(loc.data(), &"hello world");
}

#[test]
fn located_into_slice() {
  let loc = Located::new("file.rs", SimpleSpan::new(0, 5), "hello");
  assert_eq!(loc.into_slice(), "file.rs");
}

#[test]
fn located_into_span() {
  let loc = Located::new("file.rs", SimpleSpan::new(5, 10), "hello");
  assert_eq!(loc.into_span(), SimpleSpan::new(5, 10));
}

#[test]
fn located_into_data() {
  let loc = Located::new("file.rs", SimpleSpan::new(0, 5), 42i32);
  assert_eq!(loc.into_data(), 42);
}

#[test]
fn located_into_components() {
  let loc = Located::new("main.rs", SimpleSpan::new(10, 20), 42i32);
  let (slice, span, value) = loc.into_components();
  assert_eq!(slice, "main.rs");
  assert_eq!(span, SimpleSpan::new(10, 20));
  assert_eq!(value, 42);
}

#[test]
fn located_into_spanned() {
  let loc = Located::new("file.rs", SimpleSpan::new(5, 10), 42i32);
  let spanned = loc.into_spanned();
  assert_eq!(spanned.span(), SimpleSpan::new(5, 10));
  assert_eq!(*spanned, 42);
}

#[test]
fn located_into_sliced() {
  let loc = Located::new("file.rs", SimpleSpan::new(5, 10), 42i32);
  let sliced = loc.into_sliced();
  assert_eq!(sliced.slice(), "file.rs");
  assert_eq!(*sliced, 42);
}

#[test]
fn located_map_data() {
  let loc = Located::new("input.txt", SimpleSpan::new(5, 7), "42");
  let parsed: Located<i32, SimpleSpan, &str> = loc.map_data(|s| s.parse().unwrap());
  assert_eq!(*parsed, 42);
  assert_eq!(parsed.slice(), "input.txt");
}

#[test]
fn located_into_components_via_trait() {
  let loc = Located::new("main.rs", SimpleSpan::new(0, 5), 10i32);
  let (sl, sp, d) = loc.into_components();
  assert_eq!(sl, "main.rs");
  assert_eq!(sp, SimpleSpan::new(0, 5));
  assert_eq!(d, 10);
}

#[test]
#[allow(clippy::clone_on_copy)]
fn located_clone_eq_ord() {
  let a = Located::new("a", SimpleSpan::new(0, 1), 1i32);
  let b = Located::new("a", SimpleSpan::new(0, 1), 1i32);
  assert_eq!(a, b);
  let c = a.clone();
  assert_eq!(a, c);
}

// ── Sliced tests ──────────────────────────────────────────────────────────────

#[test]
fn sliced_new() {
  let s = Sliced::new("file.rs", "data");
  assert_eq!(s.slice(), "file.rs");
  assert_eq!(s.data(), &"data");
}

#[test]
fn sliced_deref() {
  let s = Sliced::new("file.rs", "hello");
  assert_eq!(s.len(), 5); // deref to &str
}

#[test]
fn sliced_deref_mut() {
  let mut s = Sliced::new("file.rs", 10i32);
  *s += 5;
  assert_eq!(*s, 15);
}

#[test]
fn sliced_display() {
  let s = Sliced::new("file.rs", 42i32);
  assert_eq!(format!("{s}"), "42");
}

#[test]
fn sliced_slice_ref() {
  let s = Sliced::new("config.toml", "data");
  assert_eq!(s.slice_ref(), &"config.toml");
}

#[test]
fn sliced_slice_mut() {
  let mut s = Sliced::new("old.txt", "data");
  *s.slice_mut() = "new.txt";
  assert_eq!(s.slice(), "new.txt");
}

#[test]
fn sliced_data_mut() {
  let mut s = Sliced::new("file.txt", 42i32);
  *s.data_mut() = 100;
  assert_eq!(*s.data(), 100);
}

#[test]
fn sliced_as_ref_trait() {
  // AsRef<Src> impl on Sliced returns &Src
  let s = Sliced::new("file.rs", "data");
  let r: &&str = <Sliced<_, _> as AsRef<&str>>::as_ref(&s);
  assert_eq!(*r, "file.rs");
}

#[test]
fn sliced_as_ref_method() {
  // Inherent as_ref() returns Sliced<&D, &Src>
  let s = Sliced::new("file.txt", 42i32);
  let borrowed = s.as_ref();
  assert_eq!(**borrowed.data(), 42);
}

#[test]
fn sliced_as_mut() {
  let mut s = Sliced::new(String::from("file.txt"), String::from("hello"));
  {
    let mut borrowed = s.as_mut();
    borrowed.data_mut().push_str(" world");
  }
  assert_eq!(s.data(), &"hello world");
}

#[test]
fn sliced_into_slice() {
  let s = Sliced::new("file.rs", "hello");
  assert_eq!(s.into_slice(), "file.rs");
}

#[test]
fn sliced_into_data() {
  let s = Sliced::new("file.rs", 42i32);
  assert_eq!(s.into_data(), 42);
}

#[test]
fn sliced_into_components() {
  let s = Sliced::new("main.rs", 42i32);
  let (sl, d) = s.into_components();
  assert_eq!(sl, "main.rs");
  assert_eq!(d, 42);
}

#[test]
fn sliced_into_components_via_trait() {
  let s = Sliced::new("main.rs", 10i32);
  let (sl, d) = s.into_components();
  assert_eq!(sl, "main.rs");
  assert_eq!(d, 10);
}

#[test]
fn sliced_map_data() {
  let s = Sliced::new("input.txt", "42");
  let parsed: Sliced<i32, &str> = s.map_data(|v| v.parse().unwrap());
  assert_eq!(*parsed, 42);
  assert_eq!(parsed.slice(), "input.txt");
}

// ── utils::marker tests ───────────────────────────────────────────────────────

#[test]
fn ignored_from() {
  let _: Ignored<String> = "hello".to_string().into();
  let _: Ignored<i32> = 42i32.into();
}

#[test]
fn ignored_default() {
  let _: Ignored<u8> = Ignored::default();
}

#[test]
fn ignored_debug() {
  let i: Ignored<u8> = Ignored::default();
  assert_eq!(format!("{i:?}"), "Ignored");
}

#[test]
#[allow(clippy::clone_on_copy)]
fn ignored_clone_copy() {
  let i: Ignored<u8> = Ignored::default();
  let _j = i; // Copy
  let _k = i.clone(); // Clone
}

#[test]
fn phantom_span() {
  let ps = PhantomSpan::phantom();
  assert_eq!(ps, PhantomSpan::PHANTOM);
  let _ = format!("{ps:?}");
}

#[test]
fn phantom_sliced() {
  let ps = PhantomSliced::phantom();
  assert_eq!(ps, PhantomSliced::PHANTOM);
}

#[test]
fn phantom_located() {
  let pl = PhantomLocated::phantom();
  assert_eq!(pl, PhantomLocated::PHANTOM);
}

#[test]
fn phantom_delimited() {
  let pd = PhantomDelimited::phantom();
  assert_eq!(pd, PhantomDelimited::PHANTOM);
}

// ── Expected tests ────────────────────────────────────────────────────────────

#[test]
fn expected_one_display() {
  let e: Expected<'_, _> = Expected::one("identifier");
  assert_eq!(format!("{e}"), "expected 'identifier'");
}

#[test]
fn expected_one_of_display() {
  let e: Expected<'_, _> = Expected::one_of(&["if", "while", "for"]);
  assert_eq!(format!("{e}"), "expected one of: 'if', 'while', 'for'");
}

#[test]
fn expected_one_of_from_oneof() {
  let oo: OneOf<'_, _> = OneOf::from_slice(&["a", "b"]);
  let e = Expected::OneOf(oo);
  let s = format!("{e}");
  assert!(s.contains("expected one of"));
}

#[test]
fn expected_is_one() {
  let e: Expected<'_, _> = Expected::one("x");
  assert!(e.is_one());
  assert!(!e.is_one_of());
}

#[test]
fn expected_is_one_of() {
  let e: Expected<'_, _> = Expected::one_of(&["x"]);
  assert!(e.is_one_of());
  assert!(!e.is_one());
}

// ── ErrorNode tests ───────────────────────────────────────────────────────────

#[test]
fn error_node_str() {
  let s: &str = <&str as ErrorNode>::error(SimpleSpan::new(0, 1));
  assert_eq!(s, "<error>");
  let m: &str = <&str as ErrorNode>::missing(SimpleSpan::new(0, 1));
  assert_eq!(m, "<missing>");
}

#[test]
fn error_node_bytes() {
  let s: &[u8] = <&[u8] as ErrorNode>::error(SimpleSpan::new(0, 1));
  assert_eq!(s, b"<error>");
  let m: &[u8] = <&[u8] as ErrorNode>::missing(SimpleSpan::new(0, 1));
  assert_eq!(m, b"<missing>");
}

// ── Syntax trait accessors ────────────────────────────────────────────────────

#[test]
fn syntax_possible_components() {
  let pc = MySyntax2::possible_components();
  assert_eq!(pc.len(), 2);
}

#[test]
fn syntax_required_components() {
  let rc = MySyntax2::required_components();
  assert_eq!(rc.len(), 2);
}

#[test]
fn syntax_kind_display() {
  let k = MySyntax2::KIND;
  assert_eq!(format!("{k}"), "test-syntax");
}

// ── Sliced via IntoComponents ─────────────────────────────────────────────────

#[test]
#[allow(clippy::clone_on_copy)]
fn sliced_clone_eq() {
  let a = Sliced::new("file.rs", 42i32);
  let b = a.clone();
  assert_eq!(a, b);
}
