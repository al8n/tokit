//! The diagnostic contract: what a finished error can be asked, whoever is asking.
//!
//! # The problem this exists for
//!
//! Without it, [`Display`](core::fmt::Display) is the entire escape route from an error, and the
//! rendering strategy is therefore decided inside the library that produced it. A consumer
//! wanting `miette`, `ariadne`, `codespan-reporting`, `annotate-snippets`, an LSP `Diagnostic` or
//! a protocol's own error object has to re-derive structure from a formatted sentence, or go
//! without.
//!
//! [`Diagnose`] is the structure, and only the structure. It names no renderer, takes no
//! dependency and is not behind a feature: it is `core` plus [`SimpleSpan`](crate::SimpleSpan),
//! so a build that reaches an embedded target carries it as readily as one that reaches a server.
//!
//! # What a diagnostic answers
//!
//! - a [`Code`] — the stable machine identifier, which is what a consumer keys off rather than
//!   the message text;
//! - a [`Severity`];
//! - a primary [`Location`], and optionally a phrase for it;
//! - zero or more [`Label`]s — secondary positions with static text, "declared here", "the other
//!   selection";
//! - zero or more [`PathSegment`]s — a coordinate in a produced *result*, for errors that happen
//!   after the document was accepted;
//! - a `help` line, where the rule has something actionable to say;
//! - and the message itself, through the [`Display`](core::fmt::Display) supertrait.
//!
//! # Two coordinate systems that never substitute for one another
//!
//! [`Diagnose::primary`] and [`Diagnose::label`] are positions in **source text**.
//! [`Diagnose::path_segment`] is a position in the **result tree** a request produced. A
//! diagnostic about a document answers zero path segments, and that zero is a positive statement
//! rather than a tri-state: there is no `Option` and nothing for a writer to branch on. See
//! [`PathSegment`] for what may and may not be published through it.
//!
//! # Why indexed access instead of iterators
//!
//! `fn labels(&self) -> impl Iterator<Item = Label>` and its GAT spelling both make [`Diagnose`]
//! dyn-incompatible, and `&dyn Diagnose` is the entire point: a driver holds lexical errors,
//! syntactic errors and a front end's own semantic diagnostics in one rendering pass, and boxing
//! each one to get there would allocate for a value that was designed not to. A count plus an
//! indexed accessor is dyn-safe, allocation-free, and hands a renderer its length up front —
//! which is what an LSP `relatedInformation` array and a `miette` label vector both want before
//! they start filling.
//!
//! [`DiagnoseExt`] puts the iterators back on top, for callers holding a concrete type or a
//! `&dyn Diagnose` alike.
//!
//! # Every method is required
//!
//! There are no defaulted accessors here, and that is a decision about failure direction rather
//! than an oversight. A defaulted `label` or `path_segment` lets a family under-report
//! **silently**: it compiles, it renders, and the labels are simply missing. The cost of refusing
//! the default is a handful of lines per *family*, not per variant.
//!
//! A genuinely new axis added later may ship defaulted, because impls written before the axis
//! existed cannot be expected to answer it. That direction is safe; this one is not.
//!
//! # Nothing here allocates
//!
//! [`Code`], [`Severity`], [`Location`], [`Label`] and [`PathSegment`] are all `Copy`, and label
//! and help text are `&'static str`. Everything that varies with the input — the name that was
//! rejected, the type that was expected — stays inside the message, which is rendered on demand
//! and only if somebody asks. So a renderer can read a diagnostic's whole structure, and write
//! its message into a buffer it already owns, without touching the allocator.
//! `tests/diagnostic_contract.rs` measures that rather than asserting it.
//!
//! # This module is the contract, not an implementation of it
//!
//! tokora's own error types do not implement [`Diagnose`]. The contract is published here so that
//! a front end built on tokora — whose lexical, syntactic and semantic errors come from three
//! different places — can answer one question with one trait, without inventing a private copy or
//! taking a renderer dependency to get one.
//!
//! It also does not replace the emitter vocabulary, which answers a different question.
//! [`emitter::Diagnostic`](crate::emitter) is a *record a collecting emitter kept* — a span, the
//! open [`labelled`](crate::labelled) context, and a borrowed payload — produced by tokora's
//! machinery while a parse runs. [`Diagnose`] is a *trait an error type implements* to describe
//! itself once the parse is over. They compose rather than compete: a collected record hands back
//! its payload, and the payload is what answers the contract. On the one name they share, see
//! [`Severity`].

mod code;
mod location;
mod path;
mod severity;

pub use code::Code;
pub use location::{Label, Location};
pub use path::PathSegment;
pub use severity::Severity;

/// The structured half of an error, beside the message its [`Display`](core::fmt::Display) gives.
///
/// See the [module documentation](self) for what the contract is for and why it has this shape.
///
/// # Implementing it
///
/// Implement it on the value that can answer — the **resolved view**, not the aggregate. Where an
/// error's spans mean nothing without a symbol table or a schema to interpret them against, the
/// type that pairs the two is the implementor; where the error is self-contained, the error is. A
/// collection of errors is not a diagnostic and should not pretend to be one: its elements are.
///
/// The three accessor pairs are contracts, not conventions:
///
/// - [`labels`](Self::labels) must equal the number of consecutive indices from `0` for which
///   [`label`](Self::label) is `Some`, and `label(i)` must be `None` for every `i` at or beyond
///   it. The same holds for [`path_segments`](Self::path_segments) and
///   [`path_segment`](Self::path_segment). A renderer sizes its storage from the count and then
///   walks the indices; a count that disagrees with the accessor is a truncated or a panicking
///   render.
/// - [`primary`](Self::primary) is **total**. Every diagnostic has one, and an input with no
///   positions in it answers [`Location::entire`] rather than a fabricated range.
/// - Reading any of them must not allocate.
///
/// ```
/// use core::fmt;
/// use tokora::{
///   SimpleSpan,
///   diagnostic::{Code, Diagnose, DiagnoseExt, Label, Location, PathSegment, Severity},
/// };
///
/// struct Redefined {
///   name: &'static str,
///   here: SimpleSpan,
///   first: SimpleSpan,
/// }
///
/// impl fmt::Display for Redefined {
///   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///     write!(f, "`{}` is defined twice", self.name)
///   }
/// }
///
/// impl Diagnose for Redefined {
///   fn code(&self) -> Code {
///     Code::new("mylang::resolve::redefined")
///   }
///   fn severity(&self) -> Severity {
///     Severity::Error
///   }
///   fn primary(&self) -> Location {
///     Location::new(0, self.here)
///   }
///   fn primary_label(&self) -> Option<&'static str> {
///     Some("redefined here")
///   }
///   fn labels(&self) -> usize {
///     1
///   }
///   fn label(&self, index: usize) -> Option<Label> {
///     match index {
///       0 => Some(Label::new(Location::new(0, self.first), "first defined here")),
///       _ => None,
///     }
///   }
///   fn path_segments(&self) -> usize {
///     0
///   }
///   fn path_segment(&self, _: usize) -> Option<PathSegment<'_>> {
///     None
///   }
///   fn help(&self) -> Option<&'static str> {
///     Some("rename one of the two definitions")
///   }
/// }
///
/// let error = Redefined {
///   name: "width",
///   here: SimpleSpan::new(40, 45),
///   first: SimpleSpan::new(8, 13),
/// };
///
/// // The whole contract is readable through erasure, which is what it is shaped for.
/// let erased: &dyn Diagnose = &error;
/// assert_eq!(erased.code().as_str(), "mylang::resolve::redefined");
/// assert_eq!(erased.to_string(), "`width` is defined twice");
///
/// let texts: Vec<&str> = erased.labels_iter().map(|label| label.text()).collect();
/// assert_eq!(texts, ["first defined here"]);
/// assert_eq!(erased.path_segments_iter().count(), 0);
/// ```
pub trait Diagnose: core::fmt::Display {
  /// Returns the stable identifier for this diagnostic's rule.
  fn code(&self) -> Code;

  /// Returns how much the diagnostic asks of its reader.
  fn severity(&self) -> Severity;

  /// Returns the position the diagnostic is about.
  fn primary(&self) -> Location;

  /// Returns a phrase to put beside the primary position, where a short one says more than an
  /// underline alone.
  fn primary_label(&self) -> Option<&'static str>;

  /// Returns how many secondary labels the diagnostic carries.
  fn labels(&self) -> usize;

  /// Returns the `index`th secondary label, or `None` past the end.
  fn label(&self, index: usize) -> Option<Label>;

  /// Returns how many result-path segments the diagnostic carries.
  ///
  /// Zero is the answer for every diagnostic that is about a document rather than a result, and
  /// it is the positive statement that this diagnostic cannot be associated with a particular
  /// field of one rather than a stand-in for it.
  fn path_segments(&self) -> usize;

  /// Returns the `index`th result-path segment, or `None` past the end.
  fn path_segment(&self, index: usize) -> Option<PathSegment<'_>>;

  /// Returns what the reader can do about it, where the rule has something actionable to say.
  fn help(&self) -> Option<&'static str>;
}

/// Erasure is the shape the whole contract is built for, so a method added in a form that is not
/// dyn-compatible fails here rather than in the first consumer that tries to erase one.
const _: Option<&dyn Diagnose> = None;

/// Iterator ergonomics over [`Diagnose`]'s indexed accessors.
///
/// Blanket-implemented, including for `dyn Diagnose`, so a renderer never has to write the index
/// loop the dyn-compatibility of [`Diagnose`] costs it.
pub trait DiagnoseExt: Diagnose {
  /// Iterates the secondary labels.
  fn labels_iter(&self) -> Labels<'_, Self>;

  /// Iterates the result-path segments, root first.
  fn path_segments_iter(&self) -> PathSegments<'_, Self>;
}

impl<D> DiagnoseExt for D
where
  D: Diagnose + ?Sized,
{
  #[inline]
  fn labels_iter(&self) -> Labels<'_, Self> {
    Labels {
      diagnostic: self,
      next: 0,
      end: self.labels(),
    }
  }

  #[inline]
  fn path_segments_iter(&self) -> PathSegments<'_, Self> {
    PathSegments {
      diagnostic: self,
      next: 0,
      end: self.path_segments(),
    }
  }
}

/// The iterator [`DiagnoseExt::labels_iter`] returns.
#[derive(Debug)]
pub struct Labels<'a, D: ?Sized> {
  diagnostic: &'a D,
  next: usize,
  end: usize,
}

impl<D> Iterator for Labels<'_, D>
where
  D: Diagnose + ?Sized,
{
  type Item = Label;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    if self.next >= self.end {
      return None;
    }
    let label = self.diagnostic.label(self.next);
    self.next += 1;
    label
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = self.end - self.next;
    (remaining, Some(remaining))
  }
}

impl<D> ExactSizeIterator for Labels<'_, D> where D: Diagnose + ?Sized {}

impl<D> core::iter::FusedIterator for Labels<'_, D> where D: Diagnose + ?Sized {}

/// The iterator [`DiagnoseExt::path_segments_iter`] returns.
#[derive(Debug)]
pub struct PathSegments<'a, D: ?Sized> {
  diagnostic: &'a D,
  next: usize,
  end: usize,
}

impl<'a, D> Iterator for PathSegments<'a, D>
where
  D: Diagnose + ?Sized,
{
  type Item = PathSegment<'a>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    if self.next >= self.end {
      return None;
    }
    let segment = self.diagnostic.path_segment(self.next);
    self.next += 1;
    segment
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = self.end - self.next;
    (remaining, Some(remaining))
  }
}

impl<D> ExactSizeIterator for PathSegments<'_, D> where D: Diagnose + ?Sized {}

impl<D> core::iter::FusedIterator for PathSegments<'_, D> where D: Diagnose + ?Sized {}
