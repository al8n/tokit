/// Tests for span.rs — SimpleSpan and Spanned types.
use tokora::SimpleSpan;
use tokora::span::{Span, Spanned};

// ── SimpleSpan::try_new ────────────────────────────────────────────────────

#[test]
fn simple_span_try_new_valid() {
  assert_eq!(SimpleSpan::try_new(5, 10), Some(SimpleSpan::new(5, 10)));
}

#[test]
fn simple_span_try_new_equal() {
  assert_eq!(SimpleSpan::try_new(5, 5), Some(SimpleSpan::new(5, 5)));
}

#[test]
fn simple_span_try_new_invalid() {
  assert_eq!(SimpleSpan::try_new(10usize, 5usize), None);
}

// ── SimpleSpan::const_new ─────────────────────────────────────────────────

#[test]
fn simple_span_const_new() {
  let s = SimpleSpan::const_new(3, 8);
  assert_eq!(s.start(), 3);
  assert_eq!(s.end(), 8);
}

#[test]
fn simple_span_try_const_new_valid() {
  assert_eq!(SimpleSpan::try_const_new(3, 8), Some(SimpleSpan::new(3, 8)));
}

#[test]
fn simple_span_try_const_new_invalid() {
  assert_eq!(SimpleSpan::try_const_new(8, 3), None);
}

// ── SimpleSpan::bump_start / bump_end / bump ──────────────────────────────

#[test]
fn simple_span_bump_start() {
  let mut s = SimpleSpan::new(5, 15);
  s.bump_start(3);
  assert_eq!(s, SimpleSpan::new(8, 15));
}

#[test]
fn simple_span_bump_end() {
  let mut s = SimpleSpan::new(5, 15);
  s.bump_end(5usize);
  assert_eq!(s, SimpleSpan::new(5, 20));
}

#[test]
fn simple_span_bump() {
  let mut s = SimpleSpan::new(5, 15);
  s.bump(&10usize);
  assert_eq!(s, SimpleSpan::new(15, 25));
}

// ── SimpleSpan const bump variants ───────────────────────────────────────

#[test]
fn simple_span_bump_start_const() {
  let mut s = SimpleSpan::new(5, 15);
  s.bump_start_const(3);
  assert_eq!(s, SimpleSpan::new(8, 15));
}

#[test]
fn simple_span_bump_end_const() {
  let mut s = SimpleSpan::new(5, 15);
  s.bump_end_const(5);
  assert_eq!(s, SimpleSpan::new(5, 20));
}

#[test]
fn simple_span_bump_const() {
  let mut s = SimpleSpan::new(5, 15);
  s.bump_const(10);
  assert_eq!(s, SimpleSpan::new(15, 25));
}

// ── SimpleSpan::set_start / set_end ──────────────────────────────────────

#[test]
fn simple_span_set_start() {
  let mut s = SimpleSpan::new(5, 15);
  s.set_start(10);
  assert_eq!(s, SimpleSpan::new(10, 15));
}

#[test]
fn simple_span_set_end() {
  let mut s = SimpleSpan::new(5, 15);
  s.set_end(20);
  assert_eq!(s, SimpleSpan::new(5, 20));
}

#[test]
fn simple_span_set_start_const() {
  let mut s = SimpleSpan::new(5, 15);
  s.set_start_const(10);
  assert_eq!(s, SimpleSpan::new(10, 15));
}

#[test]
fn simple_span_set_end_const() {
  let mut s = SimpleSpan::new(5, 15);
  s.set_end_const(20);
  assert_eq!(s, SimpleSpan::new(5, 20));
}

// ── SimpleSpan::with_start / with_end ────────────────────────────────────

#[test]
fn simple_span_with_start() {
  let s = SimpleSpan::new(5, 15).with_start(10);
  assert_eq!(s, SimpleSpan::new(10, 15));
}

#[test]
fn simple_span_with_end() {
  let s = SimpleSpan::new(5, 15).with_end(20);
  assert_eq!(s, SimpleSpan::new(5, 20));
}

#[test]
fn simple_span_with_start_const() {
  let s = SimpleSpan::new(5, 15).with_start_const(10);
  assert_eq!(s, SimpleSpan::new(10, 15));
}

#[test]
fn simple_span_with_end_const() {
  let s = SimpleSpan::new(5, 15).with_end_const(20);
  assert_eq!(s, SimpleSpan::new(5, 20));
}

// ── SimpleSpan invariant enforcement (end >= start) ───────────────────────

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn simple_span_set_start_violates_invariant() {
  let mut s = SimpleSpan::new(0, 10);
  s.set_start(20); // start > end must panic instead of silently corrupting
}

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn simple_span_set_end_violates_invariant() {
  let mut s = SimpleSpan::new(10, 20);
  s.set_end(5); // end < start must panic
}

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn simple_span_with_start_violates_invariant() {
  let _ = SimpleSpan::new(0, 10).with_start(20);
}

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn simple_span_with_end_violates_invariant() {
  let _ = SimpleSpan::new(10, 20).with_end(5);
}

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn simple_span_bump_end_violates_invariant() {
  let mut s = SimpleSpan::<i32>::new(0, 2);
  s.bump_end(-5); // Negative bump_end causes end < start
}

// ── SimpleSpan refs ──────────────────────────────────────────────────────

#[test]
fn simple_span_start_ref() {
  let s = SimpleSpan::new(5, 15);
  assert_eq!(*s.start_ref(), 5);
}

#[test]
fn simple_span_end_ref() {
  let s = SimpleSpan::new(5, 15);
  assert_eq!(*s.end_ref(), 15);
}

#[test]
fn simple_span_start_mut() {
  let mut s = SimpleSpan::new(5, 15);
  *s.start_mut() = 8;
  assert_eq!(s.start(), 8);
}

#[test]
fn simple_span_end_mut() {
  let mut s = SimpleSpan::new(5, 15);
  *s.end_mut() = 20;
  assert_eq!(s.end(), 20);
}

// ── SimpleSpan::range ────────────────────────────────────────────────────

#[test]
fn simple_span_range() {
  let s = SimpleSpan::new(5, 15);
  let r = s.range();
  assert_eq!(*r.start, 5);
  assert_eq!(*r.end, 15);
}

// ── SimpleSpan::as_ref / as_mut ───────────────────────────────────────────

#[test]
fn simple_span_as_ref() {
  let s = SimpleSpan::new(5, 15);
  let r = s.as_ref();
  assert_eq!(**r.start_ref(), 5);
  assert_eq!(**r.end_ref(), 15);
}

#[test]
fn simple_span_as_ref_cloned() {
  let s = SimpleSpan::new(5, 15);
  let r = s.as_ref();
  let cloned: SimpleSpan<usize> = r.cloned();
  assert_eq!(cloned, s);
}

#[test]
fn simple_span_as_mut() {
  let mut s = SimpleSpan::new(5, 15);
  {
    let mut m = s.as_mut();
    **m.start_mut() = 10;
    **m.end_mut() = 20;
  }
  assert_eq!(s, SimpleSpan::new(10, 20));
}

// ── SimpleSpan From conversions ───────────────────────────────────────────

#[test]
fn simple_span_from_range() {
  let s: SimpleSpan = (5..15).into();
  assert_eq!(s, SimpleSpan::new(5, 15));
}

#[test]
fn simple_span_into_range() {
  let s = SimpleSpan::new(5, 15);
  let r: core::ops::Range<usize> = s.into();
  assert_eq!(r, 5..15);
}

#[test]
fn simple_span_from_tuple() {
  let s: SimpleSpan = (5usize, 15usize).into();
  assert_eq!(s, SimpleSpan::new(5, 15));
}

#[test]
fn simple_span_into_tuple() {
  let s = SimpleSpan::new(5, 15);
  let t: (usize, usize) = s.into();
  assert_eq!(t, (5, 15));
}

// ── Span trait impl for Range<usize> ─────────────────────────────────────

#[test]
fn range_span_new() {
  let r = <core::ops::Range<usize> as Span>::new(3, 8);
  assert_eq!(r, 3..8);
}

#[test]
fn range_span_start_ref() {
  let r = 3usize..8;
  assert_eq!(*r.start_ref(), 3);
}

#[test]
fn range_span_end_ref() {
  let r = 3usize..8;
  assert_eq!(*r.end_ref(), 8);
}

#[test]
fn range_span_start_mut() {
  let mut r = 3usize..8;
  *r.start_mut() = 5;
  assert_eq!(r.start, 5);
}

#[test]
fn range_span_end_mut() {
  let mut r = 3usize..8;
  *r.end_mut() = 10;
  assert_eq!(r.end, 10);
}

#[test]
fn range_span_into_start() {
  let r = 3usize..8;
  assert_eq!(r.into_start(), 3);
}

#[test]
fn range_span_into_end() {
  let r = 3usize..8;
  assert_eq!(r.into_end(), 8);
}

#[test]
fn range_span_bump() {
  let mut r = 3usize..8;
  r.bump(&5);
  // bump shifts BOTH endpoints (relocation), matching SimpleSpan.
  assert_eq!(r, 8..13);
}

#[test]
fn range_span_bump_shifts_both_endpoints() {
  // Regression: `<Range<usize> as Span>::bump` must relocate the span
  // (shift start and end), not grow it by only advancing the end.
  let mut r = 3usize..8;
  <core::ops::Range<usize> as Span>::bump(&mut r, &5);
  assert_eq!(r.start, 8, "start must shift by n");
  assert_eq!(r.end, 13, "end must shift by n");
  assert_eq!(r.end - r.start, 5, "length is preserved");
}

#[test]
fn range_span_into_range() {
  let r = 3usize..8;
  let r2 = r.clone().into_range();
  assert_eq!(r2, r);
}

#[test]
fn range_span_start_end() {
  let r = 3usize..8;
  assert_eq!(r.start(), 3);
  assert_eq!(r.end(), 8);
}

// ── Span trait impl for SimpleSpan ───────────────────────────────────────

#[test]
fn simple_span_trait_into_range() {
  let s = SimpleSpan::new(5, 15);
  let r = <SimpleSpan as Span>::into_range(s);
  assert_eq!(r, 5..15);
}

#[test]
fn simple_span_trait_into_start() {
  let s = SimpleSpan::new(5, 15);
  assert_eq!(<SimpleSpan as Span>::into_start(s), 5);
}

#[test]
fn simple_span_trait_into_end() {
  let s = SimpleSpan::new(5, 15);
  assert_eq!(<SimpleSpan as Span>::into_end(s), 15);
}

#[test]
fn simple_span_trait_bump() {
  let mut s = SimpleSpan::new(5, 15);
  <SimpleSpan as Span>::bump(&mut s, &10);
  assert_eq!(s, SimpleSpan::new(15, 25));
}

#[test]
fn simple_span_trait_start_mut() {
  let mut s = SimpleSpan::new(5, 15);
  *<SimpleSpan as Span>::start_mut(&mut s) = 8;
  assert_eq!(s.start(), 8);
}

#[test]
fn simple_span_trait_end_mut() {
  let mut s = SimpleSpan::new(5, 15);
  *<SimpleSpan as Span>::end_mut(&mut s) = 20;
  assert_eq!(s.end(), 20);
}

// ── Spanned ───────────────────────────────────────────────────────────────

#[test]
fn spanned_new() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  assert_eq!(sp.span(), SimpleSpan::new(5, 10));
  assert_eq!(*sp.data(), 42);
}

#[test]
fn spanned_span_ref() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), "hello");
  assert_eq!(sp.span_ref(), &SimpleSpan::new(5, 10));
}

#[test]
fn spanned_span_mut() {
  let mut sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  sp.span_mut().set_end(20);
  assert_eq!(sp.span().end(), 20);
}

#[test]
fn spanned_data_mut() {
  let mut sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  *sp.data_mut() = 100;
  assert_eq!(*sp.data(), 100);
}

#[test]
fn spanned_into_span() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  assert_eq!(sp.into_span(), SimpleSpan::new(5, 10));
}

#[test]
fn spanned_into_data() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  assert_eq!(sp.into_data(), 42);
}

#[test]
fn spanned_into_components() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let (span, data) = sp.into_components();
  assert_eq!(span, SimpleSpan::new(5, 10));
  assert_eq!(data, 42);
}

#[test]
fn spanned_map_data() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let sp2 = sp.map_data(|x| x * 2);
  assert_eq!(*sp2.data(), 84);
  assert_eq!(sp2.span(), SimpleSpan::new(5, 10));
}

#[test]
fn spanned_map_span() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let sp2 = sp.map_span(|s| SimpleSpan::new(s.start() + 1, s.end() + 1));
  assert_eq!(sp2.span(), SimpleSpan::new(6, 11));
  assert_eq!(*sp2.data(), 42);
}

#[test]
fn spanned_map() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let sp2 = sp.map(|s| SimpleSpan::new(s.start() * 2, s.end() * 2), |x| x + 1);
  assert_eq!(sp2.span(), SimpleSpan::new(10, 20));
  assert_eq!(*sp2.data(), 43);
}

#[test]
fn spanned_deref() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), "hello");
  assert_eq!(sp.len(), 5); // calls str::len via Deref
}

#[test]
fn spanned_deref_mut() {
  let mut sp = Spanned::new(SimpleSpan::new(0, 1), 10i32);
  *sp += 5;
  assert_eq!(*sp, 15);
}

#[test]
fn spanned_display() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  assert_eq!(sp.to_string(), "42");
}

#[test]
fn spanned_as_ref() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), "hello");
  let r = sp.as_ref();
  assert_eq!(*r.data(), &"hello");
  assert_eq!(*r.span_ref(), &SimpleSpan::new(5, 10));
}

#[test]
fn spanned_as_mut() {
  let mut sp = Spanned::new(SimpleSpan::new(5, 10), String::from("hello"));
  {
    let m = sp.as_mut();
    m.data.push_str(" world");
  }
  assert_eq!(sp.data(), &"hello world");
}

#[test]
fn spanned_as_ref_cloned() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42i32);
  let r: Spanned<&i32, &SimpleSpan> = sp.as_ref();
  let cloned = r.cloned();
  assert_eq!(cloned.span(), SimpleSpan::new(5, 10));
  assert_eq!(*cloned.data(), 42);
}

#[test]
fn spanned_as_ref_copied() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42i32);
  let r: Spanned<&i32, &SimpleSpan> = sp.as_ref();
  let copied = r.copied();
  assert_eq!(copied.span(), SimpleSpan::new(5, 10));
  assert_eq!(*copied.data(), 42);
}

#[test]
fn spanned_as_span_trait() {
  use tokora::span::AsSpan;
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  assert_eq!(sp.as_span(), &SimpleSpan::new(5, 10));
}

#[test]
fn spanned_into_span_trait() {
  use tokora::span::IntoSpan;
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  assert_eq!(IntoSpan::into_span(sp), SimpleSpan::new(5, 10));
}

#[test]
fn spanned_as_ref_trait() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let r: &SimpleSpan = AsRef::as_ref(&sp);
  assert_eq!(*r, SimpleSpan::new(5, 10));
}

#[test]
fn spanned_from_into_unit() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let _unit: () = sp.into();
}

#[test]
fn spanned_debug() {
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let s = format!("{sp:?}");
  assert!(!s.is_empty());
}

#[test]
fn spanned_ordering() {
  let a = Spanned::new(SimpleSpan::new(1, 5), 10);
  let b = Spanned::new(SimpleSpan::new(5, 10), 20);
  assert!(a < b);
}

#[test]
fn spanned_hash() {
  use std::collections::HashSet;
  let sp = Spanned::new(SimpleSpan::new(5, 10), 42);
  let mut set = HashSet::new();
  set.insert(sp);
  assert!(set.contains(&sp));
}

// ── The span invariant, enforced on every surface the crate owns ─────────────
//
// `SimpleSpan`'s ordering invariant used to be enforced on one of its four mutator surfaces.
// The const twins assigned and added without checking, so `with_end_const(2)` on `(5, 15)`
// silently produced `(5, 2)` — a corrupt span the program then carried — and
// `bump_start_const` at `(MAX - 1, MAX)` wrapped to `(0, usize::MAX)` in release with the one
// existing assert passing over the corruption.
//
// The cell roster below is split by **what can actually fail**, because "one cell per ported
// assert" would have produced cells whose payloads are unreachable.

// ── Ordering flips: inversion reachable with no arithmetic at all ─────────────

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn set_start_const_refuses_inversion() {
  let mut span = SimpleSpan::new(5usize, 15usize);
  span.set_start_const(20);
}

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn set_end_const_refuses_inversion() {
  let mut span = SimpleSpan::new(5usize, 15usize);
  span.set_end_const(2);
}

#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn with_start_const_refuses_inversion() {
  let _ = SimpleSpan::new(5usize, 15usize).with_start_const(20);
}

/// The defect this pins: `with_end_const(2)` on `(5, 15)` yielded `(5, 2)`, silently.
#[test]
#[should_panic(expected = "end must be greater than or equal to start")]
fn with_end_const_refuses_inversion() {
  let _ = SimpleSpan::new(5usize, 15usize).with_end_const(2);
}

// ── Overflow flips: concrete arithmetic, every build ─────────────────────────
//
// The release run is the one that means anything. In debug, rustc's own overflow check would
// fire on a bare `+=` and green a cell that tests rustc rather than tokora — so these assert
// on the **message**, which only tokora's `checked_add` produces.

#[test]
#[should_panic(expected = "span bump overflows usize")]
fn bump_start_const_refuses_overflow() {
  let mut span = SimpleSpan::new(usize::MAX - 1, usize::MAX);
  span.bump_start_const(5);
}

#[test]
#[should_panic(expected = "span bump overflows usize")]
fn bump_end_const_refuses_overflow() {
  let mut span = SimpleSpan::new(0usize, usize::MAX);
  span.bump_end_const(1);
}

#[test]
#[should_panic(expected = "span bump overflows usize")]
fn bump_const_refuses_overflow() {
  let mut span = SimpleSpan::new(0usize, usize::MAX);
  span.bump_const(1);
}

#[test]
#[should_panic(expected = "span bump overflows usize")]
fn range_span_bump_refuses_overflow() {
  let mut r: core::ops::Range<usize> = 0..usize::MAX;
  Span::bump(&mut r, &1);
}

// ── The two-axis law, pinned ─────────────────────────────────────────────────

/// The documented divergence between the crate's two `Span` impls, on the **construction**
/// axis: `SimpleSpan` is strict, `Range<usize>` is lenient. The C2 resolution preserves both.
#[test]
fn construction_axis_diverges_by_design() {
  assert!(std::panic::catch_unwind(|| <SimpleSpan as Span>::new(10usize, 5usize)).is_err());

  // Compared field-wise rather than against a `10..5` literal: an inverted range literal is a
  // clippy denial, and the point of this cell is that the *constructor* produces one.
  let lenient = <core::ops::Range<usize> as Span>::new(10, 5);
  assert_eq!((lenient.start, lenient.end), (10, 5));
}

/// **The other half of the C2 law, and the cell that makes it a decision rather than a
/// sentence.** `Range<usize>` documents lenient construction, so `10..5` is a value the trait
/// says you may create — and relocating it must therefore keep working. An ordering assert on
/// `Range<usize>::bump` would hand out a value you are then not allowed to use.
///
/// Falsifying output: a panic. That would mean the ordering assert leaked onto the lenient
/// impl.
#[test]
fn range_span_relocates_an_inverted_range_without_complaint() {
  let mut r = <core::ops::Range<usize> as Span>::new(10, 5);
  Span::bump(&mut r, &1);
  assert_eq!(
    (r.start, r.end),
    (11, 6),
    "a lenient impl relocates the value its leniency produced"
  );
}

// ── Keep-greens: the asserts must not fire on well-formed input ──────────────

#[test]
fn every_const_mutator_leaves_legal_input_alone() {
  let mut a = SimpleSpan::new(5usize, 15usize);
  assert_eq!(*a.set_start_const(10), SimpleSpan::new(10, 15));
  let mut b = SimpleSpan::new(5usize, 15usize);
  assert_eq!(*b.set_end_const(20), SimpleSpan::new(5, 20));
  assert_eq!(
    SimpleSpan::new(5usize, 15usize).with_start_const(10),
    SimpleSpan::new(10, 15)
  );
  assert_eq!(
    SimpleSpan::new(5usize, 15usize).with_end_const(20),
    SimpleSpan::new(5, 20)
  );
  let mut c = SimpleSpan::new(5usize, 15usize);
  assert_eq!(*c.bump_start_const(3), SimpleSpan::new(8, 15));
  let mut d = SimpleSpan::new(5usize, 15usize);
  assert_eq!(*d.bump_end_const(5), SimpleSpan::new(5, 20));
  let mut e = SimpleSpan::new(5usize, 15usize);
  assert_eq!(*e.bump_const(10), SimpleSpan::new(15, 25));

  // The generic surface the relocation belt actually calls.
  let mut f = SimpleSpan::new(5usize, 15usize);
  Span::bump(&mut f, &10);
  assert_eq!(f, SimpleSpan::new(15, 25));

  let mut g: core::ops::Range<usize> = 5..15;
  Span::bump(&mut g, &10);
  assert_eq!(g, 15..25);
}

// ── The generic-offset overflow residue, pinned in the profile where it exists ──
//
// The expressibility split was "ordering everywhere the crate owns, overflow where the type is
// concrete". `bump_start` / `bump_end` are **generic** over `O` — `checked_add` is `E0599` on a
// type parameter — so they fall on the residue side. These cells pin that residue where it is
// observable, which is release only: in debug rustc's own overflow check fires first, so a
// debug-only cell would be testing rustc.
//
// They are the combination the ordering cells cannot see: a wrap that still satisfies
// `start <= end`, so no assert fires and a corrupt span is published. Falsified by a panic,
// which would mean the residue closed and these become the flip.

/// `bump_start` at `usize::MAX`: the start wraps to a small value that is still `<= end`, so the
/// ordering assert passes over the corruption. Release only.
#[test]
#[cfg(not(debug_assertions))]
fn release_bump_start_wraps_past_its_own_ordering_assert() {
  let mut span = SimpleSpan::new(usize::MAX - 1, usize::MAX);
  span.bump_start(3usize);
  assert_eq!(
    (*span.start_ref(), *span.end_ref()),
    (1, usize::MAX),
    "release: the start wrapped to 1, which is <= end, so the ordering assert never fired. \
     This is the stated generic-offset residue — `bump_start_const` is the checked surface."
  );
}

/// `bump_end` has the **same** hole, and finding it needs the right `start`. With a high start
/// the wrapped end lands under it and the ordering assert fires; with a low start the wrapped
/// end is still `>= start`, nothing fires, and a span of length `MAX` silently becomes one of
/// length 2. Both branches are pinned, because a cell that only exercised the caught one would
/// read as proof the surface is walled.
#[test]
#[cfg(not(debug_assertions))]
fn release_bump_end_wrap_escapes_when_start_is_low() {
  let mut span = SimpleSpan::new(0usize, usize::MAX);
  span.bump_end(3usize);
  assert_eq!(
    (*span.start_ref(), *span.end_ref()),
    (0, 2),
    "release: the end wrapped to 2, still >= start, so nothing fired — the same residue as      `bump_start`, reachable whenever the wrapped value lands back inside the span"
  );
}

/// The other branch: a high `start` puts the wrapped end on the wrong side, and the ordering
/// assert does catch it. This is what "caught only when it lands on the wrong side" means.
#[test]
#[cfg(not(debug_assertions))]
#[should_panic(expected = "end must be greater than or equal to start")]
fn release_bump_end_wrap_is_caught_when_start_is_high() {
  let mut span = SimpleSpan::new(5usize, usize::MAX);
  span.bump_end(3usize);
}

/// The concrete surface that *is* guaranteed, named in both methods' docs so a reader who hits
/// the residue has somewhere to go. Runs in both profiles: `checked_add` is unconditional.
#[test]
fn the_const_twins_are_the_checked_surface_for_usize() {
  let overflow = std::panic::catch_unwind(|| {
    let mut span = SimpleSpan::new(usize::MAX - 1, usize::MAX);
    span.bump_start_const(3);
  });
  assert!(
    overflow.is_err(),
    "bump_start_const is the checked twin and must refuse the overflow the generic method \
     publishes"
  );

  // And it is callable at run time despite being a `const fn`.
  let mut ok = SimpleSpan::new(5usize, 15usize);
  assert_eq!(*ok.bump_start_const(3), SimpleSpan::new(8, 15));
}
