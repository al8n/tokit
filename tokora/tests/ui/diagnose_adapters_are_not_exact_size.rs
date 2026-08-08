// `Labels` and `PathSegments` must NOT implement `ExactSizeIterator`, and this file is the whole
// of what enforces it.
//
// The trait's contract is that the length is *exactly* known. These two learn their length by
// calling `Diagnose::labels` / `Diagnose::path_segments` on a value they reach through a trait
// object, and nothing makes that count agree with the accessor that actually produces the items:
// a safe impl whose two methods answer different questions is enough, and `&self` plus a `Cell`
// is enough to make the answer move between calls. `tests/diagnostic_contract.rs` measures five
// such impls.
//
// The distinction that makes this a rail rather than a preference: `FusedIterator` is a promise
// the adapters can keep, because they can fail closed at the first hole, and they do.
// `ExactSizeIterator` is a promise they cannot keep at all, and a generic renderer reads a `std`
// marker without reading this crate's prose — so documenting it as "conditional" would be a
// footnote against a trait nobody footnotes. `size_hint` does not carry the count either — its
// lower half is an obligation, not an estimate — and neither does any documented recipe for
// reading the count and reserving from it: a declared count is not a capacity, and the module
// header records why that benefit did not survive being measured.
//
// Adding either impl back makes this file COMPILE, which trybuild reports as a failure of the
// case. That is the point: prose cannot notice an added impl and no other gate in this repository
// can either.
//
// Like every case in this directory it compiles only on the pinned MSRV toolchain, so the rail is
// live in CI's `msrv` job; the sibling census (`every_ui_case_file_is_registered`) runs everywhere
// and keeps the file from going missing.

use core::fmt;

use tokora::{
  SimpleSpan,
  diagnostic::{Code, Diagnose, DiagnoseExt, Label, Location, PathSegment, Severity},
};

struct Subject;

impl fmt::Display for Subject {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("a message")
  }
}

impl Diagnose for Subject {
  fn code(&self) -> Code {
    Code::new("mylang::probe::not-exact-size")
  }
  fn severity(&self) -> Severity {
    Severity::Error
  }
  fn primary(&self) -> Location {
    Location::new(0, SimpleSpan::new(0, 1))
  }
  fn primary_label(&self) -> Option<&'static str> {
    None
  }
  fn labels(&self) -> usize {
    0
  }
  fn label(&self, _: usize) -> Option<Label> {
    None
  }
  fn path_segments(&self) -> usize {
    0
  }
  fn path_segment(&self, _: usize) -> Option<PathSegment<'_>> {
    None
  }
  fn help(&self) -> Option<&'static str> {
    None
  }
}

fn requires_exact_size<I: ExactSizeIterator>(_: I) {}

fn main() {
  let subject = Subject;
  requires_exact_size(subject.labels_iter());
  requires_exact_size(subject.path_segments_iter());
}
