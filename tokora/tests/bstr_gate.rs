#![cfg(feature = "bstr_1")]

//! Regression test for the `parse_bstr` feature gate.
//!
//! `Parse::parse_bstr` / `Parse::parse_bstr_with_state` must be compiled in
//! whenever the canonical `bstr_1` feature is enabled, not only via the `bstr`
//! alias — mirroring the `parse_bytes` gate proof in `bytes_gate.rs`.
//!
//! This is a compile-time gate proof: `uses_parse_bstr` only type-checks when
//! both driver methods exist under `bstr_1` and route to a `Source = [u8]` lexer.

use tokora::{Lexer, Parse};

#[allow(dead_code)]
fn uses_parse_bstr<'inp, P, L, O, E, Lang>(
  p: P,
  q: P,
  src: &'inp bstr_1::BStr,
  state: L::State,
) -> (Result<O, E>, Result<O, E>)
where
  P: Parse<'inp, L, O, E, Lang>,
  L: Lexer<'inp, Source = [u8]>,
  L::State: Default,
  Lang: ?Sized,
{
  (p.parse_bstr(src), q.parse_bstr_with_state(src, state))
}

#[test]
fn parse_bstr_available_under_bstr_1() {
  // The gate is proven by `uses_parse_bstr` above type-checking. Exercise the
  // `bstr_1` dependency at runtime so this is a live target as well.
  let b = bstr_1::BStr::new(b"abc");
  assert_eq!(b.len(), 3);
}

// ── BStr joins the Equivalent family ─────────────────────────────────────────

/// The P08 flip. Before: `E0277: the trait Equivalent<str> is not implemented for BStr` — the
/// module doc *advertised* `BStr` in its backing list while the impls were absent from both
/// files of the two-file family. After: it compiles, and agrees with byte equality.
///
/// `&str` is the positive control that always compiled, and is what made the gap invisible.
#[test]
fn bstr_is_equivalent_in_left_position() {
  use tokora::utils::cmp::Equivalent;

  fn needs_left<S>(s: &S, probe: &str) -> bool
  where
    S: Equivalent<str> + ?Sized,
  {
    s.equivalent(probe)
  }

  let b = bstr_1::BStr::new(b"hello");
  assert!(needs_left(b, "hello"), "BStr agrees with equal bytes");
  assert!(!needs_left(b, "hellp"), "and disagrees with unequal bytes");

  // The control, differing in exactly the backing.
  assert!(needs_left("hello", "hello"));
}

/// The other two directions of the triple, plus the reverse rows the blanket supplies.
#[test]
fn bstr_equivalence_covers_the_triple() {
  use tokora::utils::cmp::Equivalent;

  let b = bstr_1::BStr::new(b"hello");
  assert!(Equivalent::<[u8]>::equivalent(b, b"hello".as_slice()));
  assert!(Equivalent::<bstr_1::BStr>::equivalent(
    b,
    bstr_1::BStr::new(b"hello")
  ));
  // Reverse directions, through the `AsRef<[u8]>` blanket.
  assert!(Equivalent::<bstr_1::BStr>::equivalent("hello", b));
  assert!(Equivalent::<bstr_1::BStr>::equivalent(
    b"hello".as_slice(),
    b
  ));
}

/// The second file of the family. The memory this repair exists to honour is that the family
/// is TWO files and a backing needs impls in both — `utils/cmp.rs` had none and
/// `utils/to_equivalent` had none, and the module doc listed `BStr` anyway.
#[test]
fn bstr_joins_the_to_equivalent_half() {
  use tokora::utils::{IntoEquivalent, ToEquivalent};

  let raw: &[u8] = b"hello";
  let viewed: &bstr_1::BStr = ToEquivalent::to_equivalent(&raw);
  assert_eq!(viewed, bstr_1::BStr::new(b"hello"));

  let consumed: &bstr_1::BStr = IntoEquivalent::into_equivalent(raw);
  assert_eq!(consumed, bstr_1::BStr::new(b"hello"));
}
