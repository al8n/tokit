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
//! indexed accessor is dyn-safe and allocation-free, and that is the whole of the reason.
//!
//! [`DiagnoseExt`] puts the iterators back on top, for callers holding a concrete type or a
//! `&dyn Diagnose` alike.
//!
//! # A declared count is not a capacity
//!
//! The shape above was originally justified partly as a way to **pre-size** a renderer's storage —
//! read the count, `Vec::with_capacity`, then fill. Written here so the next person does not
//! re-propose it: that benefit was asserted and never measured, and the hazard on the other side
//! of it was measured.
//!
//! A count comes from the implementor. This module documents, and `tests/diagnostic_contract.rs`
//! exercises, safe impls that declare more than they can produce — one of them a million labels
//! behind a single real one. `Vec`'s [`FromIterator`] and [`Extend`] reserve from whatever number
//! they are handed, so that impl costs **48 MB for one 48-byte [`Label`]**, and a reservation made
//! before the walk starts is made before [`DiagnoseExt`]'s fail-closed adapters can protect
//! anything. Against that sits a handful of reallocations on a vector that in practice holds
//! nought to five labels. That is not a trade; it is a hazard with a rationalisation attached.
//!
//! So tokora offers no pre-sizing route: not [`ExactSizeIterator`], not
//! [`size_hint`](Iterator::size_hint), and not a recommendation to read the count and reserve from
//! it. The adapters are the path. A consumer that owns every [`Diagnose`] impl it renders knows
//! its own counts and may pre-size from them — that is its trust decision about its own code,
//! not advice from here.
//!
//! # What the adapters do when an impl breaks the law
//!
//! [`Diagnose`]'s count and its accessor are required to agree (see the trait's own
//! documentation), but nothing in the type system makes them, and an impl that disagrees needs
//! no `unsafe` and no panic to do it — two methods answering different questions is enough, and
//! `&self` plus a `Cell` is enough to make the answer move between calls.
//!
//! That is the implementor's defect, and [`Labels`] and [`PathSegments`] still have to be sound
//! under it, because the marker traits they carry are **promises the adapters make**, not
//! promises the implementor signed. A generic renderer relies on them without ever seeing the
//! [`Diagnose`] impl.
//!
//! So the adapters **fail closed**: reaching an index the accessor answers `None` for retires the
//! cursor, and every later call answers `None`. That makes
//! [`FusedIterator`](core::iter::FusedIterator) true by construction. Anything past the hole is
//! dropped, which is the direction to fail in — a renderer that shows too few labels is
//! repairable, and one walking a resumed iterator is not.
//!
//! **Neither adapter implements [`ExactSizeIterator`], and neither should.** That trait's contract
//! is that the length is *exactly* known, and a length read through a trait object whose count may
//! disagree with its accessor is not. Documenting it as conditional would not help: nobody reads a
//! crate's prose before trusting a `std` marker.
//!
//! **Nor does [`size_hint`](Iterator::size_hint) forward the count.** It answers
//! `(0, Some(remaining))`. Its *upper* half may be loose and this one is a genuine cap — the walk
//! cannot run past the count it was sized from. Its *lower* half may not: that is a claim the
//! iterator will produce at least that many, and retire-on-hole means the adapter can never make
//! it above zero, because the next index may be a hole and end the walk. `Vec`'s
//! [`FromIterator`] and [`Extend`] both reserve from the lower bound, so forwarding a count of a
//! million behind one real label reserves for a million — measured at `capacity = 1_000_000` on a
//! 48-byte [`Label`], from both, on every toolchain this crate builds against.
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
//! [`Code`], [`Severity`], [`Location`], [`Label`] and [`PathSegment`] are all `Copy`, so reading
//! the structure off a diagnostic moves no ownership and reaches no allocator. What varies with
//! the input is kept out of that structure by **two** mechanisms, and the difference between them
//! is the design rather than an implementation detail:
//!
//! - **Variable prose stays in the message.** The name that was rejected, the type that was
//!   expected, the two selections that would not merge — [`Display`](core::fmt::Display) formats
//!   those, on demand and only if somebody asks. No accessor carries them, which is what lets
//!   label and help text be `&'static str`.
//! - **Variable structured data is borrowed.** A response key in a [`PathSegment::Field`] is taken
//!   from the document, so it cannot be `&'static` — and it must **not** be pushed into the
//!   message instead, because a result path that exists only as prose is not a result path and
//!   nothing downstream can read it. It is borrowed from storage the diagnostic already holds,
//!   which is what [`PathSegment`]'s lifetime parameter is for.
//!
//! So a renderer can read a diagnostic's whole structure, and write its message into a buffer it
//! already owns, without touching the allocator. `tests/diagnostic_contract.rs` measures that
//! rather than asserting it, over a subject whose path segment borrows from an owned `String`.
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
/// The three accessor pairs are obligations on the impl, not conventions:
///
/// - [`labels`](Self::labels) must equal the number of consecutive indices from `0` for which
///   [`label`](Self::label) is `Some`, and `label(i)` must be `None` for every `i` at or beyond
///   it. The same holds for [`path_segments`](Self::path_segments) and
///   [`path_segment`](Self::path_segment). Break it and your own labels go missing: the adapters
///   stop at the first hole, so a reader sees a truncated diagnostic rather than a corrupted one,
///   and nothing downstream can recover what was past it.
/// - [`primary`](Self::primary) is **total**. Every diagnostic has one, and an input with no
///   positions in it answers [`Location::entire`] rather than a fabricated range.
/// - Reading any of them must not allocate.
///
/// # Reading one
///
/// Walk the labels and the path through [`labels_iter`](DiagnoseExt::labels_iter) and
/// [`path_segments_iter`](DiagnoseExt::path_segments_iter).
///
/// The counts above are an obligation the *implementor* took on, and they read to a consumer like
/// a number to act on. They are not: a count is what one impl claims, the adapters are what it can
/// actually produce, and only the second is something tokora stands behind. Above all a count is
/// not a capacity — [the module documentation](self#a-declared-count-is-not-a-capacity) carries
/// the measurement that settled it.
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
///
/// See [`PathSegments`]' sibling note and the [module documentation](self#what-the-adapters-do-when-an-impl-breaks-the-law)
/// for what this does when [`Diagnose::labels`] and [`Diagnose::label`] disagree.
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
    let Some(label) = self.diagnostic.label(self.next) else {
      // Fail closed. An accessor answering `None` before the count said to stop is an impl
      // breaking `Diagnose`'s law, and simply returning that `None` would break a promise THIS
      // TYPE made: the next call would read the following index and could answer `Some`, which
      // is exactly what `FusedIterator` rules out. Retiring the cursor makes the marker true by
      // construction rather than by trusting the implementor, at the cost of dropping anything
      // past the hole — the direction to fail in, because a renderer that stops early is
      // repairable and one that walks a resumed iterator is not.
      self.next = self.end;
      return None;
    };
    self.next += 1;
    Some(label)
  }

  /// `(0, Some(what the count has left))`.
  ///
  /// The upper bound is real: [`next`](Iterator::next) can never run past the count it was
  /// sized from, so nothing can yield more than this.
  ///
  /// The lower bound is `0`, and that is an obligation rather than a style choice.
  /// [`Iterator::size_hint`]'s lower half is a claim that at least that many items *will* be
  /// produced, and with retire-on-hole this adapter can never make it above zero — whatever
  /// [`Diagnose::labels`] said, the very next index may be a hole and end the walk. Reporting the
  /// count there is not a permissible over-estimate; only the upper bound is allowed to be loose.
  ///
  /// It is not free to get wrong. `Vec`'s [`FromIterator`] and [`Extend`] reserve from the lower
  /// bound, so a count of a million behind one real label reserves for a million: measured at
  /// `capacity = 1_000_000` on a 48-byte [`Label`], from both, on every toolchain this crate
  /// builds against. Reading the count off [`Diagnose`] and reserving from it by hand has the
  /// same cost for the same reason — see
  /// [the module documentation](self#a-declared-count-is-not-a-capacity).
  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    // `next` is only ever advanced to a value at or below `end`, so this cannot underflow. One
    // expression with no branch: when `remaining` is 0 this is already the exact `(0, Some(0))`.
    let remaining = self.end - self.next;
    (0, Some(remaining))
  }
}

impl<D> core::iter::FusedIterator for Labels<'_, D> where D: Diagnose + ?Sized {}

/// The iterator [`DiagnoseExt::path_segments_iter`] returns.
///
/// See [`Labels`]' sibling note and the [module documentation](self#what-the-adapters-do-when-an-impl-breaks-the-law)
/// for what this does when [`Diagnose::path_segments`] and [`Diagnose::path_segment`] disagree.
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
    // Fail closed on a hole, for the reason written out on `Labels::next`.
    let Some(segment) = self.diagnostic.path_segment(self.next) else {
      self.next = self.end;
      return None;
    };
    self.next += 1;
    Some(segment)
  }

  /// `(0, Some(what the count has left))`, on the same terms as [`Labels`]' — a true cap, and a
  /// lower bound this adapter cannot raise above zero.
  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    // `next` is only ever advanced to a value at or below `end`, so this cannot underflow.
    let remaining = self.end - self.next;
    (0, Some(remaining))
  }
}

impl<D> core::iter::FusedIterator for PathSegments<'_, D> where D: Diagnose + ?Sized {}
