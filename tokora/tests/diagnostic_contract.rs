//! The diagnostic contract's two load-bearing claims, measured rather than asserted.
//!
//! # What is being claimed
//!
//! 1. **Reading a diagnostic allocates nothing.** Not "little", not "amortised": a renderer can
//!    ask a `&dyn Diagnose` every question the trait has — code, severity, primary, primary
//!    label, every secondary label, every result-path segment, help — and write the message into
//!    a buffer it already owns, without touching the allocator. That is what makes the contract
//!    usable in a request path, and it holds only because every vocabulary type is `Copy` and
//!    every label and help string is `&'static`.
//!
//! 2. **The counts and the indexed accessors agree.** A renderer sizes its storage from
//!    `labels()` / `path_segments()` and then walks the indices, so a count that disagrees with
//!    its accessor is a truncated or a panicking render. The subjects below are checked for the
//!    agreement, for `None` at and past the end, and for the two iterator adapters reporting the
//!    same length.
//!
//! # How claim 1 is measured
//!
//! A counting global allocator, and a thread-local counter so that a test running beside this one
//! on another thread cannot perturb the reading. [`the_gate_counts`] is the discrimination check:
//! it shows the allocator is installed and the counter moves, so a zero reading elsewhere means
//! "nothing allocated" rather than "nothing was looking".
//!
//! There is deliberately **no warm-up**. A gate over a working set needs one, because the
//! caller's buffers have a high-water mark to reach; the contract has no working set, so needing
//! a warm-up would itself be the finding.

#![allow(missing_docs)]

use core::{
  cell::Cell,
  fmt::{self, Write as _},
  hint::black_box,
};
use std::alloc::{GlobalAlloc, Layout, System};

use tokora::{
  SimpleSpan,
  diagnostic::{Code, Diagnose, DiagnoseExt, Label, Location, PathSegment, Severity},
};

// ---------------------------------------------------------------------------------------------
// the counting allocator
// ---------------------------------------------------------------------------------------------

thread_local! {
  /// Allocation events on this thread. `alloc`, `alloc_zeroed` and a growing `realloc` all count.
  static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

struct Counting;

/// Counts every allocation event and forwards to the system allocator.
///
/// The counter is thread-local and updated through `try_with`, so an allocation made while the
/// thread's local storage is being set up or torn down is simply not counted rather than
/// re-entering it.
unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    bump();
    unsafe { System.alloc(layout) }
  }

  unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
    bump();
    unsafe { System.alloc_zeroed(layout) }
  }

  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    bump();
    unsafe { System.realloc(ptr, layout, new_size) }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn bump() {
  let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

/// Runs `body` and returns how many allocation events it caused on this thread.
fn allocations(body: impl FnOnce()) -> u64 {
  let before = ALLOCATIONS.with(Cell::get);
  body();
  ALLOCATIONS.with(Cell::get) - before
}

/// A [`fmt::Write`] sink over a fixed stack buffer.
///
/// A `String` would allocate once and then never again, which is the wrong instrument: it would
/// make an accessor that allocated on every call indistinguishable from one that did not, as long
/// as the buffer was warm. This one cannot allocate at all, so anything the counter sees came from
/// the contract.
struct StackBuffer<const N: usize> {
  bytes: [u8; N],
  len: usize,
}

impl<const N: usize> StackBuffer<N> {
  const fn new() -> Self {
    Self {
      bytes: [0; N],
      len: 0,
    }
  }

  const fn len(&self) -> usize {
    self.len
  }

  const fn clear(&mut self) {
    self.len = 0;
  }
}

impl<const N: usize> fmt::Write for StackBuffer<N> {
  fn write_str(&mut self, text: &str) -> fmt::Result {
    let end = self.len + text.len();
    if end > N {
      return Err(fmt::Error);
    }
    self.bytes[self.len..end].copy_from_slice(text.as_bytes());
    self.len = end;
    Ok(())
  }
}

// ---------------------------------------------------------------------------------------------
// the subjects
// ---------------------------------------------------------------------------------------------

/// The document-shaped diagnostic: two source positions, no result path.
///
/// This is what every lexical and syntactic error looks like — a primary span, a secondary label
/// pointing at the place that explains it, and zero path segments.
struct Redefined {
  name: &'static str,
  here: SimpleSpan,
  first: SimpleSpan,
}

impl fmt::Display for Redefined {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "`{}` is defined twice", self.name)
  }
}

impl Diagnose for Redefined {
  fn code(&self) -> Code {
    Code::new("mylang::resolve::redefined")
  }

  fn severity(&self) -> Severity {
    Severity::Error
  }

  fn primary(&self) -> Location {
    Location::new(0, self.here)
  }

  fn primary_label(&self) -> Option<&'static str> {
    Some("redefined here")
  }

  fn labels(&self) -> usize {
    1
  }

  fn label(&self, index: usize) -> Option<Label> {
    match index {
      0 => Some(Label::new(
        Location::new(1, self.first),
        "first defined here",
      )),
      _ => None,
    }
  }

  fn path_segments(&self) -> usize {
    0
  }

  fn path_segment(&self, _: usize) -> Option<PathSegment<'_>> {
    None
  }

  fn help(&self) -> Option<&'static str> {
    Some("rename one of the two definitions")
  }
}

/// The result-shaped diagnostic: a path into the produced tree, and no span to point at.
///
/// It answers [`Location::entire`] because the input it is about is a machine-generated response
/// rather than text anyone can edit — the documented exception, exercised here so the measurement
/// covers the `None`-span arm and a borrowed path segment.
struct Rejected {
  field: String,
  index: u32,
}

impl fmt::Display for Rejected {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "`{}[{}]` was null", self.field, self.index)
  }
}

impl Diagnose for Rejected {
  fn code(&self) -> Code {
    Code::new("mylang::execute::unexpected-null")
  }

  fn severity(&self) -> Severity {
    Severity::Warning
  }

  fn primary(&self) -> Location {
    Location::entire(2)
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
    2
  }

  fn path_segment(&self, index: usize) -> Option<PathSegment<'_>> {
    match index {
      0 => Some(PathSegment::Field(&self.field)),
      1 => Some(PathSegment::Index(self.index)),
      _ => None,
    }
  }

  fn help(&self) -> Option<&'static str> {
    None
  }
}

fn redefined() -> Redefined {
  Redefined {
    name: "width",
    here: SimpleSpan::new(40, 45),
    first: SimpleSpan::new(8, 13),
  }
}

fn rejected() -> Rejected {
  Rejected {
    field: String::from("heroes"),
    index: 2,
  }
}

/// Reads every method of the contract, including the index one past each collection, and writes
/// the message out.
///
/// Exhaustive by hand rather than by macro: a method added to `Diagnose` and not read here would
/// leave the claim covering less than it says, and there is nothing that could notice.
fn read_the_whole_contract(subject: &dyn Diagnose, sink: &mut StackBuffer<256>) {
  black_box(subject.code());
  black_box(subject.severity());
  black_box(subject.primary());
  black_box(subject.primary_label());
  black_box(subject.help());

  let labels = subject.labels();
  black_box(labels);
  for index in 0..=labels {
    black_box(subject.label(index));
  }
  let segments = subject.path_segments();
  black_box(segments);
  for index in 0..=segments {
    black_box(subject.path_segment(index));
  }

  // The provided iterators too: they are the door most renderers will actually use, and one that
  // allocated would leave the indexed measurement above technically true and practically wrong.
  for label in subject.labels_iter() {
    black_box(label);
  }
  for segment in subject.path_segments_iter() {
    black_box(segment);
  }

  sink.clear();
  write!(sink, "{subject}").expect("the message fits the buffer");
  black_box(sink.len());
}

// ---------------------------------------------------------------------------------------------
// claim 1: nothing here allocates
// ---------------------------------------------------------------------------------------------

/// The measurement discriminates: an allocation inside the window is seen.
///
/// Without this, the zero readings below would be indistinguishable from an allocator that was
/// never installed.
#[test]
fn the_gate_counts() {
  // Warm the thread-local itself, so its own setup is not what is being measured.
  let _ = allocations(|| {});
  let counted = allocations(|| {
    let buffer: Vec<u8> = Vec::with_capacity(4096);
    black_box(&buffer);
  });
  assert!(
    counted >= 1,
    "the counting allocator saw nothing; the gates below would pass vacuously"
  );
}

/// Reading the whole contract through `&dyn Diagnose` and rendering it allocates nothing.
///
/// Erased on purpose: a concrete call could be inlined into nothing and prove less than it
/// appears to, and vtable dispatch is what a renderer holding three families in one collection
/// actually executes.
#[test]
fn reading_the_contract_allocates_nothing() {
  let document = redefined();
  let result = rejected();

  // Assembled outside the window: the storage is the harness's, not the contract's.
  let contract: [&dyn Diagnose; 2] = [&document, &result];
  let mut sink = StackBuffer::<256>::new();
  let _ = allocations(|| {});

  let counted = allocations(|| {
    for subject in contract {
      read_the_whole_contract(subject, &mut sink);
    }
  });
  assert_eq!(
    counted,
    0,
    "reading {} diagnostics through `&dyn Diagnose` and rendering them into a stack buffer \
     allocated {counted} times",
    contract.len()
  );
  assert!(sink.len() > 0, "the last message rendered to nothing");
}

/// And the same counter reports a real allocation on the same values, a few lines later.
///
/// The discrimination for the test above specifically: rendering the *same* diagnostic into a
/// fresh `String` does allocate, so the zero is a property of the read rather than of the window.
#[test]
fn rendering_into_a_fresh_string_does_allocate() {
  let subject: &dyn Diagnose = &redefined();
  let _ = allocations(|| {});

  let counted = allocations(|| {
    let owned = subject.to_string();
    black_box(&owned);
  });
  assert!(counted >= 1, "`to_string` should allocate");
}

// ---------------------------------------------------------------------------------------------
// claim 2: the counts and the accessors agree
// ---------------------------------------------------------------------------------------------

/// `labels()` is the number of consecutive `Some` indices from zero, and nothing past it answers.
#[test]
fn label_counts_agree_with_the_accessor() {
  for subject in [&redefined() as &dyn Diagnose, &rejected()] {
    let count = subject.labels();
    for index in 0..count {
      assert!(
        subject.label(index).is_some(),
        "`{}` counts {count} labels but answers `None` at {index}",
        subject.code()
      );
    }
    assert!(
      subject.label(count).is_none(),
      "`{}` answers a label at {count}, its own count",
      subject.code()
    );
    assert!(subject.label(count + 1).is_none());
    assert!(subject.label(usize::MAX).is_none());
  }
}

/// The same, for the result path.
#[test]
fn path_segment_counts_agree_with_the_accessor() {
  for subject in [&redefined() as &dyn Diagnose, &rejected()] {
    let count = subject.path_segments();
    for index in 0..count {
      assert!(
        subject.path_segment(index).is_some(),
        "`{}` counts {count} segments but answers `None` at {index}",
        subject.code()
      );
    }
    assert!(
      subject.path_segment(count).is_none(),
      "`{}` answers a segment at {count}, its own count",
      subject.code()
    );
    assert!(subject.path_segment(count + 1).is_none());
    assert!(subject.path_segment(usize::MAX).is_none());
  }
}

/// The adapters report the count up front and shrink as they are walked, on a `dyn` receiver.
#[test]
fn the_adapters_are_exact_sized_and_fused() {
  let subject: &dyn Diagnose = &rejected();

  let mut segments = subject.path_segments_iter();
  assert_eq!(segments.len(), 2);
  assert_eq!(segments.size_hint(), (2, Some(2)));
  assert_eq!(segments.next(), Some(PathSegment::Field("heroes")));
  assert_eq!(segments.len(), 1);
  assert_eq!(segments.next(), Some(PathSegment::Index(2)));
  assert_eq!(segments.len(), 0);
  assert_eq!(segments.next(), None);
  // Fused: exhausted stays exhausted.
  assert_eq!(segments.next(), None);

  let mut labels = subject.labels_iter();
  assert_eq!(labels.len(), 0);
  assert_eq!(labels.next(), None);

  let document: &dyn Diagnose = &redefined();
  let mut labels = document.labels_iter();
  assert_eq!(labels.len(), 1);
  let label = labels.next().expect("one label");
  assert_eq!(label.text(), "first defined here");
  assert_eq!(label.location().source(), 1);
  assert_eq!(label.location().span(), Some(SimpleSpan::new(8, 13)));
  assert_eq!(labels.next(), None);
  assert_eq!(labels.next(), None);
}

/// The adapter yields exactly what the indexed accessor does, for both subjects.
#[test]
fn the_adapters_agree_with_the_indices() {
  for subject in [&redefined() as &dyn Diagnose, &rejected()] {
    let indexed: Vec<_> = (0..subject.labels())
      .map(|index| subject.label(index).expect("counted"))
      .collect();
    let iterated: Vec<_> = subject.labels_iter().collect();
    assert_eq!(indexed, iterated);

    let indexed: Vec<_> = (0..subject.path_segments())
      .map(|index| subject.path_segment(index).expect("counted"))
      .collect();
    let iterated: Vec<_> = subject.path_segments_iter().collect();
    assert_eq!(indexed, iterated);
  }
}

// ---------------------------------------------------------------------------------------------
// the vocabulary
// ---------------------------------------------------------------------------------------------

/// `primary` is total: a diagnostic with no position answers the whole input, not a fabricated
/// range.
#[test]
fn primary_is_total() {
  let ranged: &dyn Diagnose = &redefined();
  let whole: &dyn Diagnose = &rejected();

  assert_eq!(ranged.primary().source(), 0);
  assert_eq!(ranged.primary().span(), Some(SimpleSpan::new(40, 45)));
  assert!(!ranged.primary().is_entire());

  assert_eq!(whole.primary().source(), 2);
  assert_eq!(whole.primary().span(), None);
  assert!(whole.primary().is_entire());
}

#[test]
fn a_code_renders_as_its_identifier() {
  let code = Code::new("mylang::parse::missing-semicolon");
  assert_eq!(code.as_str(), "mylang::parse::missing-semicolon");
  assert_eq!(code.to_string(), "mylang::parse::missing-semicolon");
  assert_eq!(code.as_ref(), "mylang::parse::missing-semicolon");
  assert_eq!(code, Code::new("mylang::parse::missing-semicolon"));
  assert_ne!(code, Code::new("mylang::parse::missing-comma"));
}

#[test]
fn a_severity_renders_as_its_lowercase_name() {
  assert_eq!(Severity::Error.as_str(), "error");
  assert_eq!(Severity::Warning.as_str(), "warning");
  assert_eq!(Severity::Advice.as_str(), "advice");
  assert_eq!(Severity::Advice.to_string(), "advice");
}

/// The emitter's two channel tiers embed into the reporting ladder, and the mapping is total.
#[test]
fn the_emitter_tiers_lift_onto_the_ladder() {
  assert_eq!(
    Severity::from(tokora::emitter::Severity::Error),
    Severity::Error
  );
  assert_eq!(
    Severity::from(tokora::emitter::Severity::Warning),
    Severity::Warning
  );
}

#[test]
fn a_path_segment_renders_as_its_step() {
  assert_eq!(PathSegment::Field("heroes").to_string(), "heroes");
  assert_eq!(PathSegment::Index(0).to_string(), "0");
  assert_eq!(PathSegment::Index(12).to_string(), "12");
}
