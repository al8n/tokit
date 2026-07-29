use core::{
  hash::Hash,
  ops::{AddAssign, Range},
};

use crate::utils::{IntoComponents, marker::Ignored};

/// A trait representing a span in the source code.
pub trait Span {
  /// The offset type of the span.
  type Offset: Ord + Clone + Hash;

  /// Creates a new span from the given start and end offsets.
  fn new(start: Self::Offset, end: Self::Offset) -> Self;

  /// Consumes the span and returns it.
  fn into_range(self) -> core::ops::Range<Self::Offset>
  where
    Self: Sized;

  /// Returns the start offset of the span.
  #[inline(always)]
  fn start(&self) -> Self::Offset {
    self.start_ref().clone()
  }

  /// Returns the start offset of the span.
  fn start_ref(&self) -> &Self::Offset;

  /// Returns the mutable reference to the start offset of the span.
  fn start_mut(&mut self) -> &mut Self::Offset;

  /// Consumes the span and returns the start offset.
  fn into_start(self) -> Self::Offset
  where
    Self: Sized;

  /// Returns the end offset of the span.
  #[inline(always)]
  fn end(&self) -> Self::Offset {
    self.end_ref().clone()
  }

  /// Returns the end offset of the span.
  fn end_ref(&self) -> &Self::Offset;

  /// Returns the mutable reference to the end offset of the span.
  fn end_mut(&mut self) -> &mut Self::Offset;

  /// Consumes the span and returns the end offset.
  fn into_end(self) -> Self::Offset
  where
    Self: Sized;

  /// Relocates the span by `n` offsets, shifting **both** the start and the end
  /// so the length is preserved.
  ///
  /// This is a whole-span move, not an end extension. To grow the span by moving
  /// only its end, use an end-specific operation such as
  /// [`SimpleSpan::bump_end`].
  ///
  /// ## The law, and its two axes
  ///
  /// A span type answers two independent questions, and this trait's implementations are
  /// allowed to answer them differently:
  ///
  /// - **Construction / ordering.** May an inverted span exist at all? [`SimpleSpan`] says no
  ///   — [`new`](Self::new) panics on `end < start`. `Range<usize>` says yes: `Span::new(10, 5)`
  ///   yields `10..5`, and that leniency is documented rather than accidental.
  /// - **Relocation / overflow.** `bump` shifts both ends equally, so on a well-formed span it
  ///   cannot invert; it can only wrap. Where the arithmetic is concrete, an implementation
  ///   should refuse the wrap rather than publish a corrupt span.
  ///
  /// **An ordering assert on `bump` is licensed by the impl's construction-axis strictness.**
  /// A strict impl can only reach an inverted result through wrap, so asserting ordering there
  /// catches a genuine corruption. A **lenient** impl must not assert: an inverted span is a
  /// value its own documentation says you may create, and refusing to relocate it would hand
  /// out a value you are then not allowed to use. That is why `SimpleSpan`'s `bump` asserts and
  /// `Range<usize>`'s does not, while both refuse the overflow.
  ///
  /// ## What this crate can and cannot enforce
  ///
  /// `bump` is **required**, so a third-party implementation's body is its own; this law is
  /// written, not checked. Two further doors stay open by construction:
  /// [`start_mut`](Self::start_mut) and [`end_mut`](Self::end_mut) hand out
  /// `&mut Self::Offset`, and an inversion written through them is unobservable to any assert
  /// on any mutator. Closing that means removing the accessors, which is a different change in
  /// a different release.
  fn bump(&mut self, n: &Self::Offset);
}

impl Span for core::ops::Range<usize> {
  type Offset = usize;

  #[inline(always)]
  fn new(start: Self::Offset, end: Self::Offset) -> Self {
    start..end
  }

  #[inline(always)]
  fn into_range(self) -> core::ops::Range<Self::Offset> {
    self.start..self.end
  }

  #[inline(always)]
  fn start_ref(&self) -> &Self::Offset {
    &self.start
  }

  #[inline(always)]
  fn start_mut(&mut self) -> &mut Self::Offset {
    &mut self.start
  }

  #[inline(always)]
  fn into_start(self) -> Self::Offset {
    self.start
  }

  #[inline(always)]
  fn end_ref(&self) -> &Self::Offset {
    &self.end
  }

  #[inline(always)]
  fn end_mut(&mut self) -> &mut Self::Offset {
    &mut self.end
  }

  #[inline(always)]
  fn into_end(self) -> Self::Offset {
    self.end
  }

  /// ## Panics
  ///
  /// Panics if either end overflows `usize` — every build, not only debug.
  ///
  /// It does **not** assert ordering, and that is the two-axis law above applied to this impl:
  /// `Range<usize>` is the documented-**lenient** constructor, so `10..5` is a value this trait
  /// says you may create. Refusing to relocate it would make a documented-legal value
  /// unusable.
  #[inline(always)]
  fn bump(&mut self, n: &Self::Offset) {
    self.start = self.start.checked_add(*n).expect(OVERFLOW_MSG);
    self.end = self.end.checked_add(*n).expect(OVERFLOW_MSG);
  }
}

/// The every-build overflow message, shared by every surface whose arithmetic is concrete
/// `usize`. Named once so the `#[should_panic(expected = ...)]` cells and the `## Panics`
/// sections cannot drift apart from the code.
const OVERFLOW_MSG: &str = "span bump overflows usize";

impl<O> Span for SimpleSpan<O>
where
  O: Ord + Clone + Hash + for<'a> AddAssign<&'a O>,
{
  type Offset = O;

  #[inline(always)]
  fn new(start: Self::Offset, end: Self::Offset) -> Self {
    SimpleSpan::new(start, end)
  }

  #[inline(always)]
  fn into_range(self) -> core::ops::Range<Self::Offset> {
    self.start..self.end
  }

  #[inline(always)]
  fn start_ref(&self) -> &Self::Offset {
    self.start_ref()
  }

  #[inline(always)]
  fn start_mut(&mut self) -> &mut Self::Offset {
    self.start_mut()
  }

  #[inline(always)]
  fn into_start(self) -> Self::Offset {
    self.start
  }

  #[inline(always)]
  fn end_ref(&self) -> &Self::Offset {
    self.end_ref()
  }

  #[inline(always)]
  fn end_mut(&mut self) -> &mut Self::Offset {
    self.end_mut()
  }

  #[inline(always)]
  fn into_end(self) -> Self::Offset {
    self.end
  }

  /// ## Panics
  ///
  /// Panics if the relocation leaves `end < start`, which on a well-formed span can only
  /// happen through wrap.
  ///
  /// This is the surface the crate's own error and carrier types relocate through
  /// (`self.span.bump(by)` on a generic `L::Span`), so it is where the check earns its keep.
  /// The assert lives **here**, in the trait impl, rather than on the inherent
  /// [`SimpleSpan::bump`] — this impl's where-clause already carries `O: Ord`, so it is free,
  /// whereas adding `Ord` to the inherent method would be a real narrowing:
  /// `SimpleSpan` derives `Default`, so `SimpleSpan<NonOrd>` is constructible and
  /// `.bump()` runs on it today.
  ///
  /// **Stated residue:** `SimpleSpan::bump` called *inherently*, on a concrete `SimpleSpan<O>`,
  /// stays unguarded. That is the price of not taking the bound, and it is priced at exactly
  /// what it costs — an in-crate caller that bypasses the trait.
  ///
  /// Overflow on a generic `O` is rustc's own check: it panics in debug and **wraps in
  /// release**, where this assert then catches every wrap except a start that wraps under and
  /// lands at or below `end`. Closing that needs an arithmetic bound on
  /// [`Span::Offset`], which core cannot express.
  #[inline(always)]
  fn bump(&mut self, n: &Self::Offset) {
    self.bump(n);
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
  }
}

/// Enables accessing the source span of a parsed element.
///
/// This trait provides a way to retrieve the span information associated with
/// a parsed element without taking ownership of the element itself. This is
/// useful for scenarios where you need to reference the location of the element
/// in the source input, such as for error reporting or diagnostics.
///
/// ## Usage Patterns
/// Common scenarios for using this trait:
/// - **Error reporting**: Attaching span information to error messages
/// - **Diagnostics**: Highlighting source locations in IDEs or tools
/// - **Logging**: Recording where certain elements were parsed from
/// - **Analysis**: Performing source-based analysis or transformations
///
/// ## Implementation Notes
///
/// Implementing types should ensure that:
///   - The returned span is accurate and corresponds to the element's location in the source
///   - The method is efficient and does not involve unnecessary allocations or computations
///   - The trait is implemented for all relevant types
///   - The span information is preserved during parsing and transformations
///   - The implementation is consistent with other span-related traits
///   - The method is efficient (ideally zero-cost)
///   - The returned reference is valid for the lifetime of the element
pub trait AsSpan<Span> {
  /// Consumes this element and returns the owned source span.
  ///
  /// This method takes ownership of the element and extracts its span information
  /// as an owned value. This is useful when you need to transfer ownership of
  /// the span data to another data structure or when the element itself is no
  /// longer needed but the location information should be preserved.
  fn as_span(&self) -> &Span;
}

/// Enables consuming a parsed element to extract its source span.
///
/// This trait provides a way to take ownership of the span information from
/// a parsed element, which is useful when the element itself is no longer
/// needed but the span data should be preserved or transferred to another
/// data structure.
///
/// ## Usage Patterns
///
/// Common scenarios for using this trait:
/// - **AST construction**: Building higher-level AST nodes that need owned spans
/// - **Error collection**: Gathering span information for batch error reporting
/// - **Transformation**: Converting between different representations while preserving location
/// - **Optimization**: Avoiding clones when transferring ownership is acceptable
///
/// ## Implementation Notes
///
/// Implementing types should ensure that:
/// - The returned span is equivalent to what `AsSpan::spanned()` would return
/// - All span information is preserved during the conversion
/// - The conversion is efficient (ideally zero-cost)
pub trait IntoSpan<Span>: AsSpan<Span> {
  /// Consumes this element and returns the owned source span.
  ///
  /// This method takes ownership of the element and extracts its span information
  /// as an owned value. This is useful when you need to transfer ownership of
  /// the span data to another data structure or when the element itself is no
  /// longer needed but the location information should be preserved.
  fn into_span(self) -> Span;
}

/// A lightweight span representing a range of positions in source input.
///
/// `SimpleSpan` is a simple but powerful type that tracks where in the source code a particular
/// element came from. It stores just two byte offsets: the start and end positions.
/// While similar to [`Range<usize>`], `SimpleSpan` provides additional methods tailored for
/// working with source locations in parsers and compilers.
///
/// # Use Cases
///
/// - **Error Reporting**: Show users exactly where errors occurred in their code
/// - **Source Mapping**: Track how parsed elements relate to original source
/// - **IDE Integration**: Enable features like go-to-definition and hover tooltips
/// - **Code Formatting**: Preserve the original location of code elements
/// - **Debugging**: Understand which part of input produced which AST node
///
/// # Design
///
/// `SimpleSpan` is designed to be:
/// - **Copy**: Can be freely copied without allocation (just two `usize` values)
/// - **Comparable**: Supports equality and ordering for span-based algorithms
/// - **Hashable**: Can be used as map/set keys for span-indexed data structures
/// - **Chumsky-compatible**: Implements `chumsky::span::SimpleSpan` for parser integration
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::SimpleSpan;
///
/// // Create a span covering characters 10-20
/// let span = SimpleSpan::new(10, 20);
///
/// assert_eq!(span.start(), 10);
/// assert_eq!(span.end(), 20);
/// assert_eq!(span.len(), 10);
/// assert!(!span.is_empty());
/// ```
///
/// ## Safe Creation
///
/// ```rust
/// use tokora::SimpleSpan;
///
/// // try_new returns None for invalid spans
/// assert!(SimpleSpan::try_new(10, 5).is_none());  // end < start
/// assert!(SimpleSpan::try_new(10, 10).is_some()); // empty span is valid
/// assert!(SimpleSpan::try_new(10, 20).is_some()); // normal span
/// ```
///
/// ## SimpleSpan Manipulation
///
/// ```rust
/// use tokora::SimpleSpan;
///
/// let mut span = SimpleSpan::new(10, 20);
///
/// // Move the start forward
/// span.bump_start(5);
/// assert_eq!(span.start(), 15);
///
/// // Extend the end
/// span.bump_end(10);
/// assert_eq!(span.end(), 30);
///
/// // Shift the entire span
/// span.bump(&5);
/// assert_eq!(span.start(), 20);
/// assert_eq!(span.end(), 35);
/// ```
///
/// ## Builder-Style Methods
///
/// ```rust
/// use tokora::SimpleSpan;
///
/// let span = SimpleSpan::new(0, 10)
///     .with_start(5)
///     .with_end(15);
///
/// assert_eq!(span.start(), 5);
/// assert_eq!(span.end(), 15);
/// ```
///
/// ## Error Reporting Example
///
/// ```rust,ignore
/// use tokora::SimpleSpan;
///
/// fn report_error(message: &str, span: SimpleSpan, source: &str) {
///     let line_start = source[..span.start()].rfind('\n')
///         .map(|pos| pos + 1)
///         .unwrap_or(0);
///     let line_end = source[span.end()..]
///         .find('\n')
///         .map(|pos| span.end() + pos)
///         .unwrap_or(source.len());
///
///     let line = &source[line_start..line_end];
///     let column = span.start() - line_start;
///
///     eprintln!("Error: {}", message);
///     eprintln!("{}", line);
///     eprintln!("{}^", " ".repeat(column));
/// }
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct SimpleSpan<Offset = usize> {
  pub(crate) start: Offset,
  pub(crate) end: Offset,
}

impl<O> core::fmt::Display for SimpleSpan<O>
where
  O: core::fmt::Display,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{}..{}", self.start, self.end)
  }
}

impl<O> SimpleSpan<&O>
where
  O: Clone,
{
  /// Clone the span into owned offsets.
  #[inline(always)]
  pub fn cloned(self) -> SimpleSpan<O> {
    SimpleSpan {
      start: self.start.clone(),
      end: self.end.clone(),
    }
  }
}

impl SimpleSpan {
  /// Create a new span.
  ///
  /// ## Panics
  ///
  /// Panics if `end < start`.
  #[inline(always)]
  pub const fn const_new(start: usize, end: usize) -> Self {
    assert!(end >= start, "end must be greater than or equal to start");
    Self { start, end }
  }

  /// Try to create a new span.
  ///
  /// Returns `None` if `end < start`.
  #[inline(always)]
  pub const fn try_const_new(start: usize, end: usize) -> Option<Self> {
    if end >= start {
      Some(Self { start, end })
    } else {
      None
    }
  }

  /// Bump the start of the span by `n`.
  ///
  /// ## Panics
  ///
  /// - if the start overflows `usize` — in **every** build, not only debug;
  /// - if the result would leave `start > end`.
  ///
  /// The overflow arm is not decoration. `+=` wraps in release, so this function used to
  /// return `(0, usize::MAX)` from `(MAX - 1, MAX)` with the ordering assert passing over the
  /// corruption — this section promised a panic the code did not perform.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.bump_start_const(3);
  /// assert_eq!(span, SimpleSpan::new(8, 15));
  /// ```
  #[inline(always)]
  pub const fn bump_start_const(&mut self, n: usize) -> &mut Self {
    self.start = match self.start.checked_add(n) {
      Some(v) => v,
      None => panic!("span bump overflows usize"),
    };
    assert!(
      self.start <= self.end,
      "start must be less than or equal to end"
    );
    self
  }

  /// Bump the end of the span by `n`.
  ///
  /// ## Panics
  ///
  /// - if the end overflows `usize` — in **every** build;
  /// - if the result would leave `end < start`. Growing the end cannot invert a well-formed
  ///   span, so this arm is reachable only through the wrap the arm above already refuses; it
  ///   is defence in depth against a future edit, and it carries the non-const twin's exact
  ///   message.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.bump_end_const(5);
  /// assert_eq!(span, SimpleSpan::new(5, 20));
  /// ```
  #[inline(always)]
  pub const fn bump_end_const(&mut self, n: usize) -> &mut Self {
    self.end = match self.end.checked_add(n) {
      Some(v) => v,
      None => panic!("span bump overflows usize"),
    };
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Bump the start and the end of the span by `n`.
  ///
  /// ## Panics
  ///
  /// - if either end overflows `usize` — in **every** build;
  /// - if the result would leave `end < start`. A whole-span move preserves ordering, so this
  ///   arm is unreachable while the overflow arm above holds: the `checked_add` dominates it.
  ///   It is defence in depth against a future edit and is deliberately **not** pinned by a
  ///   test — a cell written against it could never fail for the reason it names.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.bump_const(10);
  /// assert_eq!(span, SimpleSpan::new(15, 25));
  /// ```
  #[inline(always)]
  pub const fn bump_const(&mut self, n: usize) -> &mut Self {
    self.start = match self.start.checked_add(n) {
      Some(v) => v,
      None => panic!("span bump overflows usize"),
    };
    self.end = match self.end.checked_add(n) {
      Some(v) => v,
      None => panic!("span bump overflows usize"),
    };
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Set the start of the span, returning a mutable reference to self.
  ///
  /// ## Panics
  ///
  /// Panics if the assignment would leave `end < start` — in **every** build. This is an
  /// assignment, not arithmetic, so there is no overflow arm; inversion is reachable with no
  /// arithmetic at all, which is what made the silent version a real corruption rather than a
  /// theoretical one.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.set_start_const(10);
  /// assert_eq!(span, SimpleSpan::new(10, 15));
  /// ```
  #[inline(always)]
  pub const fn set_start_const(&mut self, start: usize) -> &mut Self {
    self.start = start;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Set the end of the span, returning a mutable reference to self.
  ///
  /// ## Panics
  ///
  /// Panics if the assignment would leave `end < start` — in **every** build. This is an
  /// assignment, not arithmetic, so there is no overflow arm; inversion is reachable with no
  /// arithmetic at all, which is what made the silent version a real corruption rather than a
  /// theoretical one.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.set_end_const(20);
  /// assert_eq!(span, SimpleSpan::new(5, 20));
  /// ```
  #[inline(always)]
  pub const fn set_end_const(&mut self, end: usize) -> &mut Self {
    self.end = end;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Set the start of the span, returning self.
  ///
  /// ## Panics
  ///
  /// Panics if the assignment would leave `end < start` — in **every** build. An assignment,
  /// not arithmetic, so there is no overflow arm.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15).with_start_const(10);
  /// assert_eq!(span, SimpleSpan::new(10, 15));
  /// ```
  #[inline(always)]
  pub const fn with_start_const(mut self, start: usize) -> Self {
    self.start = start;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Set the end of the span, returning self.
  ///
  /// ## Panics
  ///
  /// Panics if the assignment would leave `end < start` — in **every** build. An assignment,
  /// not arithmetic, so there is no overflow arm.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15).with_end_const(20);
  /// assert_eq!(span, SimpleSpan::new(5, 20));
  /// ```
  #[inline(always)]
  pub const fn with_end_const(mut self, end: usize) -> Self {
    self.end = end;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }
}

impl<O> SimpleSpan<O> {
  /// Convert to a span of references.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15);
  /// let span_ref = span.as_ref();
  /// assert_eq!(**span_ref.start_ref(), 5);
  /// assert_eq!(**span_ref.end_ref(), 15);
  /// ```
  #[inline(always)]
  pub const fn as_ref(&self) -> SimpleSpan<&O> {
    SimpleSpan {
      start: &self.start,
      end: &self.end,
    }
  }

  /// Convert to a span of mutable references.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// let mut span_mut = span.as_mut();
  /// **span_mut.start_mut() = 10;
  /// **span_mut.end_mut() = 20;
  /// assert_eq!(span, SimpleSpan::new(10, 20));
  /// ```
  #[inline(always)]
  pub const fn as_mut(&mut self) -> SimpleSpan<&mut O> {
    SimpleSpan {
      start: &mut self.start,
      end: &mut self.end,
    }
  }

  /// Create a new span.
  ///
  /// ## Panics
  ///
  /// Panics if `end < start`.
  #[inline(always)]
  pub fn new(start: O, end: O) -> Self
  where
    O: Ord,
  {
    assert!(end >= start, "end must be greater than or equal to start");
    Self { start, end }
  }

  /// Try to create a new span.
  ///
  /// Returns `None` if `end < start`.
  #[inline(always)]
  pub fn try_new(start: O, end: O) -> Option<Self>
  where
    O: Ord,
  {
    if end >= start {
      Some(Self { start, end })
    } else {
      None
    }
  }

  /// Bump the start of the span by `n`.
  ///
  /// ## Panics
  ///
  /// Panics if the result would leave `start > end` — in every build.
  ///
  /// **It does NOT panic on overflow, and that is a stated residue rather than an oversight.**
  /// `O` is generic here, so `checked_add` is not expressible: [`Span::Offset`] declares no
  /// arithmetic bound, core has no `CheckedAdd`, and asking for one on a type parameter is
  /// `E0599`. In **debug**, rustc's own overflow check fires. In **release** the `+=` wraps,
  /// and the ordering assert below catches that wrap only when the wrapped value lands on the
  /// wrong side of `end` — a wrap that lands back inside the span is published silently.
  ///
  /// **If your offsets are `usize`, use [`bump_start_const`](Self::bump_start_const) instead.** It is the same
  /// operation on the concrete type, and being concrete it carries a real `checked_add` that
  /// panics with `"span bump overflows usize"` in every profile. It is a `const fn`, which is
  /// callable at run time like any other function.
  ///
  /// Closing it *here* needs an arithmetic bound on the offset — a trait definition or a new
  /// dependency, plus a breaking narrowing of this method — which is design work rather than a
  /// line in a publishing release.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.bump_start(3);
  /// assert_eq!(span, SimpleSpan::new(8, 15));
  /// ```
  #[inline(always)]
  pub fn bump_start(&mut self, n: O) -> &mut Self
  where
    O: AddAssign<O> + Ord,
  {
    self.start += n;
    assert!(
      self.start <= self.end,
      "start must be less than or equal to end"
    );
    self
  }

  /// Bump the end of the span by `n`.
  ///
  /// ## Panics
  ///
  /// Panics if the result would leave `end < start` — in every build.
  ///
  /// **It does NOT panic on overflow, and that is a stated residue rather than an oversight.**
  /// `O` is generic here, so `checked_add` is not expressible: [`Span::Offset`] declares no
  /// arithmetic bound, core has no `CheckedAdd`, and asking for one on a type parameter is
  /// `E0599`. In **debug**, rustc's own overflow check fires. In **release** the `+=` wraps,
  /// and the ordering assert below catches that wrap only when the wrapped value lands on the
  /// wrong side of `start` — a wrap that lands back inside the span is published silently.
  ///
  /// **If your offsets are `usize`, use [`bump_end_const`](Self::bump_end_const) instead.** It is the same
  /// operation on the concrete type, and being concrete it carries a real `checked_add` that
  /// panics with `"span bump overflows usize"` in every profile. It is a `const fn`, which is
  /// callable at run time like any other function.
  ///
  /// Closing it *here* needs an arithmetic bound on the offset — a trait definition or a new
  /// dependency, plus a breaking narrowing of this method — which is design work rather than a
  /// line in a publishing release.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.bump_end(5);
  /// assert_eq!(span, SimpleSpan::new(5, 20));
  /// ```
  #[inline(always)]
  pub fn bump_end(&mut self, n: O) -> &mut Self
  where
    O: AddAssign<O> + Ord,
  {
    self.end += n;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Bump the start and the end of the span by `n`.
  ///
  /// ## Panics
  ///
  /// **Nothing here — deliberately, and this is a stated residue rather than an oversight.**
  ///
  /// Guarding this method would mean adding `O: Ord` to its where-clause, and that is a real
  /// narrowing with a reachable inhabitant: `SimpleSpan` derives [`Default`], so
  /// `SimpleSpan::<NonOrd>::default().bump(&n)` compiles and runs today for an `O` that is
  /// `Default + AddAssign<&Self> + Clone` and not `Ord`. Taking the bound would break that
  /// code to catch a corruption on a path that already has a guard elsewhere.
  ///
  /// The guard lives on [`<SimpleSpan<O> as Span>::bump`](Span::bump) instead — that impl's
  /// where-clause already carries `O: Ord`, so the assert is free there — and that is the
  /// surface this crate's own error and carrier types relocate through. What stays unguarded
  /// is this method called **inherently**, on a concrete `SimpleSpan<O>`, bypassing the trait.
  ///
  /// Overflow is rustc's: a panic in debug, a wrap in release.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.bump(&10);
  /// assert_eq!(span, SimpleSpan::new(15, 25));
  /// ```
  #[inline(always)]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.start += n;
    self.end += n;
    self
  }

  /// Set the start of the span, returning a mutable reference to self.
  ///
  /// ## Panics
  ///
  /// Panics if `start > end`.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.set_start(10);
  /// assert_eq!(span, SimpleSpan::new(10, 15));
  /// ```
  #[inline(always)]
  pub fn set_start(&mut self, start: O) -> &mut Self
  where
    O: Ord,
  {
    self.start = start;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Set the end of the span, returning a mutable reference to self.
  ///
  /// ## Panics
  ///
  /// Panics if `end < start`.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// span.set_end(20);
  /// assert_eq!(span, SimpleSpan::new(5, 20));
  /// ```
  #[inline(always)]
  pub fn set_end(&mut self, end: O) -> &mut Self
  where
    O: Ord,
  {
    self.end = end;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Set the start of the span, returning self.
  ///
  /// ## Panics
  ///
  /// Panics if `start > end`.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15).with_start(10);
  /// assert_eq!(span, SimpleSpan::new(10, 15));
  /// ```
  #[inline(always)]
  pub fn with_start(mut self, start: O) -> Self
  where
    O: Ord,
  {
    self.start = start;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Set the end of the span, returning self.
  ///
  /// ## Panics
  ///
  /// Panics if `end < start`.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15).with_end(20);
  /// assert_eq!(span, SimpleSpan::new(5, 20));
  /// ```
  #[inline(always)]
  pub fn with_end(mut self, end: O) -> Self
  where
    O: Ord,
  {
    self.end = end;
    assert!(
      self.end >= self.start,
      "end must be greater than or equal to start"
    );
    self
  }

  /// Get the start of the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15);
  /// assert_eq!(span.start(), 5);
  /// ```
  #[inline(always)]
  pub const fn start(&self) -> O
  where
    O: Copy,
  {
    self.start
  }

  /// Get the reference to the start of the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15);
  ///
  /// assert_eq!(*span.start_ref(), 5);
  /// ```
  #[inline(always)]
  pub const fn start_ref(&self) -> &O {
    &self.start
  }

  /// Get the mutable reference to the start of the span.
  ///
  /// Unlike [`set_start`](Self::set_start), this raw accessor cannot enforce the
  /// `end >= start` invariant: it is the unchecked escape hatch, and the caller
  /// is responsible for keeping the span well-formed.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// *span.start_mut() = 10;
  /// assert_eq!(span.start(), 10);
  /// ```
  #[inline(always)]
  pub const fn start_mut(&mut self) -> &mut O {
    &mut self.start
  }

  /// Get the end of the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15);
  /// assert_eq!(span.end(), 15);
  /// ```
  #[inline(always)]
  pub const fn end(&self) -> O
  where
    O: Copy,
  {
    self.end
  }

  /// Get the reference to the end of the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15);
  ///
  /// assert_eq!(*span.end_ref(), 15);
  /// ```
  #[inline(always)]
  pub const fn end_ref(&self) -> &O {
    &self.end
  }

  /// Get the mutable reference to the end of the span.
  ///
  /// Unlike [`set_end`](Self::set_end), this raw accessor cannot enforce the
  /// `end >= start` invariant: it is the unchecked escape hatch, and the caller
  /// is responsible for keeping the span well-formed.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let mut span = SimpleSpan::new(5, 15);
  /// *span.end_mut() = 20;
  /// assert_eq!(span.end(), 20);
  /// ```
  #[inline(always)]
  pub const fn end_mut(&mut self) -> &mut O {
    &mut self.end
  }

  /// Get the length of the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15);
  /// assert_eq!(span.len(), 10);
  /// ```
  #[inline(always)]
  pub fn len(&self) -> O
  where
    O: for<'a> core::ops::Sub<&'a O, Output = O> + Clone,
  {
    self.end.clone().sub(&self.start)
  }

  /// Check if the span is empty.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let empty = SimpleSpan::new(5, 5);
  /// assert!(empty.is_empty());
  ///
  /// let not_empty = SimpleSpan::new(5, 15);
  /// assert!(!not_empty.is_empty());
  /// ```
  #[inline(always)]
  pub fn is_empty(&self) -> bool
  where
    O: PartialEq,
  {
    self.start == self.end
  }

  /// Returns a range covering the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  ///
  /// let span = SimpleSpan::new(5, 15);
  /// assert_eq!(span.range(), (&5..&15));
  /// ```
  #[inline(always)]
  pub fn range(&self) -> Range<&O> {
    &self.start..&self.end
  }
}

impl<O> From<Range<O>> for SimpleSpan<O>
where
  O: Ord,
{
  #[inline(always)]
  fn from(range: Range<O>) -> Self {
    Self::new(range.start, range.end)
  }
}

impl<O> From<SimpleSpan<O>> for Range<O> {
  #[inline(always)]
  fn from(span: SimpleSpan<O>) -> Self {
    span.start..span.end
  }
}

impl<O> From<(O, O)> for SimpleSpan<O>
where
  O: Ord,
{
  #[inline(always)]
  fn from((start, end): (O, O)) -> Self {
    Self::new(start, end)
  }
}

impl<O> From<SimpleSpan<O>> for (O, O) {
  #[inline(always)]
  fn from(span: SimpleSpan<O>) -> Self {
    (span.start, span.end)
  }
}

/// A value paired with its source location span.
///
/// `Spanned<D>` combines a value of type `D` with a span `S` that indicates where in
/// the source input the value came from. This is fundamental for building parsers and
/// compilers that need to track source locations for error reporting, debugging, and
/// IDE integration.
///
/// # Design
///
/// `Spanned` uses public fields for direct access, but also provides accessor methods
/// for consistency. It implements `Deref` and `DerefMut` to allow transparent access
/// to the inner data while keeping span information available when needed.
///
/// # Common Patterns
///
/// ## Transparent Access via Deref
///
/// Thanks to `Deref`, you can call methods on the wrapped value directly:
///
/// ```rust
/// use tokora::SimpleSpan;
/// use tokora::span::Spanned;
///
/// let spanned_str = Spanned::new(SimpleSpan::new(0, 5), "hello");
///
/// // Can call str methods directly
/// assert_eq!(spanned_str.len(), 5);
/// assert_eq!(spanned_str.to_uppercase(), "HELLO");
///
/// // But can still access the span
/// assert_eq!(spanned_str.span().start(), 0);
/// ```
///
/// ## Mapping Values While Preserving Spans
///
/// ```rust,ignore
/// use tokora::SimpleSpan;
/// use tokora::span::Spanned;
///
/// let spanned_num = Spanned::new(SimpleSpan::new(10, 12), "42");
///
/// // Parse the string, keeping the same span
/// let parsed: Spanned<i32> = Spanned::new(
///     spanned_num.span,
///     spanned_num.data.parse().unwrap()
/// );
///
/// assert_eq!(*parsed, 42);
/// assert_eq!(parsed.span().start(), 10);
/// ```
///
/// ## Building AST Nodes with Locations
///
/// ```rust,ignore
/// use tokora::SimpleSpan;
/// use tokora::span::Spanned;
///
/// enum Expr {
///     Number(i64),
///     Add(Box<Spanned<Expr>>, Box<Spanned<Expr>>),
/// }
///
/// // Each AST node knows its source location
/// let left = Spanned::new(SimpleSpan::new(0, 2), Expr::Number(1));
/// let right = Spanned::new(SimpleSpan::new(5, 7), Expr::Number(2));
///
/// let add = Spanned::new(
///     SimpleSpan::new(0, 7), // Covers the whole expression
///     Expr::Add(Box::new(left), Box::new(right))
/// );
/// ```
///
/// ## Error Reporting with Context
///
/// ```rust,ignore
/// fn type_error<T>(expected: &str, got: &Spanned<T>) -> Error
/// where
///     T: core::fmt::Debug
/// {
///     Error {
///         message: format!("Expected {}, got {:?}", expected, got.data),
///         span: *got.span(),
///         help: Some("Try using a different type".to_string()),
///     }
/// }
/// ```
///
/// # Trait Implementations
///
/// - **`Deref` / `DerefMut`**: Access the inner data transparently
/// - **`Display`**: Delegates to the inner data's `Display` implementation
/// - **`AsSpan` / `IntoSpan`**: Extract just the span information
/// - **`IntoComponents`**: Destructure into `(Span, D)` tuple
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::SimpleSpan;
/// use tokora::span::Spanned;
///
/// let span = SimpleSpan::new(10, 15);
/// let spanned = Spanned::new(span, "hello");
///
/// assert_eq!(spanned.span(), span);
/// assert_eq!(spanned.data(), &"hello");
/// assert_eq!(*spanned, "hello"); // Via Deref
/// ```
///
/// ## Destructuring
///
/// ```rust
/// use tokora::SimpleSpan;
/// use tokora::span::Spanned;
///
/// let spanned = Spanned::new(SimpleSpan::new(0, 5), 42);
///
/// let (span, value) = spanned.into_components();
/// assert_eq!(span.start(), 0);
/// assert_eq!(value, 42);
/// ```
///
/// ## Mutable Access
///
/// ```rust
/// use tokora::SimpleSpan;
/// use tokora::span::Spanned;
///
/// let mut spanned = Spanned::new(SimpleSpan::new(0, 1), 10);
///
/// // Modify the data
/// *spanned += 5;
/// assert_eq!(*spanned, 15);
///
/// // Modify the span
/// spanned.span_mut().bump_end(4);
/// assert_eq!(spanned.span().end(), 5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct Spanned<D, S = SimpleSpan> {
  /// The source location span of the data.
  ///
  /// This indicates where in the source input this value came from,
  /// expressed as byte offsets.
  pub span: S,

  /// The wrapped data value.
  ///
  /// This is the actual parsed or processed value, paired with its
  /// source location for error reporting and debugging.
  pub data: D,
}

impl<D, S> AsRef<S> for Spanned<D, S> {
  #[inline(always)]
  fn as_ref(&self) -> &S {
    self.span_ref()
  }
}

impl<D, S> AsSpan<S> for Spanned<D, S> {
  #[inline(always)]
  fn as_span(&self) -> &S {
    AsRef::as_ref(self)
  }
}

impl<D, S> IntoSpan<S> for Spanned<D, S> {
  #[inline(always)]
  fn into_span(self) -> S {
    self.span
  }
}

impl<D, S> core::ops::Deref for Spanned<D, S> {
  type Target = D;

  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    &self.data
  }
}

impl<D, S> core::ops::DerefMut for Spanned<D, S> {
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.data
  }
}

impl<D, S> core::fmt::Display for Spanned<D, S>
where
  D: core::fmt::Display,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.data.fmt(f)
  }
}

impl<D, S> core::error::Error for Spanned<D, S>
where
  D: core::error::Error,
  S: core::fmt::Debug,
{
}

impl<D, S> IntoComponents for Spanned<D, S> {
  type Components = (S, D);

  #[inline(always)]
  fn into_components(self) -> Self::Components {
    (self.span, self.data)
  }
}

impl<D, S> Spanned<&D, &S> {
  /// Returns a copied version of the spanned value.
  #[inline(always)]
  pub const fn copied(&self) -> Spanned<D, S>
  where
    D: Copy,
    S: Copy,
  {
    Spanned {
      span: *self.span,
      data: *self.data,
    }
  }

  /// Returns a cloned version of the spanned value.
  #[inline(always)]
  pub fn cloned(&self) -> Spanned<D, S>
  where
    D: Clone,
    S: Clone,
  {
    self.map(Clone::clone, Clone::clone)
  }
}

impl<D, S> Spanned<D, S> {
  /// Create a new spanned value.
  #[inline(always)]
  pub const fn new(span: S, data: D) -> Self {
    Self { span, data }
  }

  /// Get a reference to the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  /// use tokora::span::Spanned;
  ///
  /// let spanned = Spanned::new(SimpleSpan::new(5, 10), "data");
  /// assert_eq!(spanned.span(), SimpleSpan::new(5, 10));
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Get a reference to the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  /// use tokora::span::Spanned;
  ///
  /// let spanned = Spanned::new(SimpleSpan::new(5, 10), "data");
  /// assert_eq!(spanned.span_ref(), &SimpleSpan::new(5, 10));
  /// ```
  #[inline(always)]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Get a mutable reference to the span.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  /// use tokora::span::Spanned;
  ///
  /// let mut spanned = Spanned::new(SimpleSpan::new(5, 10), "data");
  /// spanned.span_mut().set_end(15);
  /// assert_eq!(spanned.span().end(), 15);
  /// ```
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Get a reference to the data.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  /// use tokora::span::Spanned;
  ///
  /// let spanned = Spanned::new(SimpleSpan::new(5, 10), 42);
  /// assert_eq!(*spanned.data(), 42);
  /// ```
  #[inline(always)]
  pub const fn data(&self) -> &D {
    &self.data
  }

  /// Get a mutable reference to the data.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  /// use tokora::span::Spanned;
  ///
  /// let mut spanned = Spanned::new(SimpleSpan::new(5, 10), 42);
  /// *spanned.data_mut() = 100;
  /// assert_eq!(*spanned.data(), 100);
  /// ```
  #[inline(always)]
  pub const fn data_mut(&mut self) -> &mut D {
    &mut self.data
  }

  /// Returns a reference to the span and data.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  /// use tokora::span::Spanned;
  ///
  /// let spanned = Spanned::new(SimpleSpan::new(5, 10), String::from("hello"));
  /// let borrowed: Spanned<&String, &SimpleSpan> = spanned.as_ref();
  /// assert_eq!(borrowed.data(), &"hello");
  /// ```
  #[inline(always)]
  pub const fn as_ref(&self) -> Spanned<&D, &S> {
    Spanned {
      span: &self.span,
      data: &self.data,
    }
  }

  /// Returns a mutable reference to the span and data.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::SimpleSpan;
  /// use tokora::span::Spanned;
  ///
  /// let mut spanned = Spanned::new(SimpleSpan::new(5, 10), String::from("hello"));
  /// let mut borrowed: Spanned<&mut String, &mut SimpleSpan> = spanned.as_mut();
  /// borrowed.data.push_str(" world");
  /// assert_eq!(spanned.data(), &"hello world");
  /// ```
  #[inline(always)]
  pub const fn as_mut(&mut self) -> Spanned<&mut D, &mut S> {
    Spanned {
      span: &mut self.span,
      data: &mut self.data,
    }
  }

  /// Consume the spanned value and return the span.
  #[inline(always)]
  pub fn into_span(self) -> S {
    self.span
  }

  /// Consume the spanned value and return the data.
  #[inline(always)]
  pub fn into_data(self) -> D {
    self.data
  }

  /// Decompose the spanned value into its span and data.
  #[inline(always)]
  pub fn into_components(self) -> (S, D) {
    (self.span, self.data)
  }

  /// Map the data to a new value, preserving the span.
  #[inline]
  pub fn map_data<F, U>(self, f: F) -> Spanned<U, S>
  where
    F: FnOnce(D) -> U,
  {
    Spanned {
      span: self.span,
      data: f(self.data),
    }
  }

  /// Map the span to a new value, preserving the data.
  #[inline]
  pub fn map_span<F, T>(self, f: F) -> Spanned<D, T>
  where
    F: FnOnce(S) -> T,
  {
    Spanned {
      span: f(self.span),
      data: self.data,
    }
  }

  /// Map both the span and data to new values.
  #[inline]
  pub fn map<F, G, U, T>(self, f: F, g: G) -> Spanned<U, T>
  where
    F: FnOnce(S) -> T,
    G: FnOnce(D) -> U,
  {
    Spanned {
      span: f(self.span),
      data: g(self.data),
    }
  }
}

impl<D, S> From<Spanned<D, S>> for () {
  #[inline(always)]
  fn from(_: Spanned<D, S>) -> Self {}
}

impl<D, S> From<Spanned<D, S>> for Ignored<()> {
  #[inline(always)]
  fn from(_: Spanned<D, S>) -> Self {
    Ignored::default()
  }
}
