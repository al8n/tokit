use core::{fmt, marker::PhantomData};

use crate::{
  span::{SimpleSpan, Span},
  syntax::{Language, Syntax},
};

/// Describes a missing syntax element between two anchors.
///
/// `Missing<T, Lang>` remembers the span immediately **before** the missing syntax and,
/// optionally, the span immediately **after** it. This allows diagnostics to highlight the
/// precise gap where the parser expected a `T` node. The generic `T` refers to a
/// [`Syntax`] implementation so the error can report the exact syntax kind via
/// [`Syntax::KIND`], while `Lang` ties the error to a specific [`Language`].
///
/// # When to Use
///
/// - A required AST/CST node is completely absent (e.g., missing identifier or block)
/// - Delimited structures contain consecutive items without the expected syntax between them
/// - Recovery wants to surface “something should be here” while pointing to the surrounding
///   context instead of fabricating bogus spans
///
/// # Anchors
///
/// - **`before`**: SimpleSpan of the last token confidently parsed before the missing node.
/// - **`after` (optional)**: SimpleSpan of the first token parsed after the missing node.
///   When `after` is `None` the gap is assumed to be at end-of-input or before an unknown
///   boundary.
///
/// The [`span`](Self::span) method derives a zero-width span when only `before` is known, or
/// the exclusive range between `before.end()` and `after.start()` when both anchors exist.
///
/// # Examples
///
/// ```rust,ignore
/// use tokora::{
///     error::Missing,
///     syntax::{Language, Syntax},
///     SimpleSpan,
/// };
///
/// // Suppose `ParameterListSyntax` implements `Syntax<KIND = SyntaxKind::ParameterList>`
/// let before = SimpleSpan::new(10, 11); // '('
/// let after = SimpleSpan::new(12, 13);  // ')'
/// let error = Missing::<ParameterListSyntax, MyLang>::between(before, after);
///
/// assert_eq!(error.kind(), SyntaxKind::ParameterList);
/// assert_eq!(error.span(), SimpleSpan::new(11, 12)); // gap between '(' and ')'
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Missing<T, S = SimpleSpan, Lang = ()> {
  before: S,
  after: Option<S>,
  span: Option<S>,
  _syntax: PhantomData<T>,
  _lang: PhantomData<Lang>,
}

impl<T, S, Lang> Missing<T, S, Lang> {
  /// Creates a missing error that occurs **after** the provided span.
  ///
  /// Use this when the parser reached the end of a construct without finding the required
  /// syntax (e.g., missing trailing expression before `}` or end-of-input).
  #[inline]
  pub fn new(before: S) -> Self {
    Self {
      before,
      after: None,
      span: None,
      _syntax: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Creates a missing error bounded by both a `before` and `after` span.
  ///
  /// The resulting [`span`](Self::span) covers the gap between `before.end()` and
  /// `after.start()`. When the anchors overlap (e.g., consecutive tokens), the gap collapses
  /// to a zero-width span at `after.start()`.
  ///
  /// # Panics
  /// - If before and after are overlapping in a way that makes it impossible to determine a gap.
  #[inline]
  pub fn between(before: S, after: S) -> Self
  where
    S: Span + Clone,
  {
    let start = before.end_ref();
    let end = after.start_ref();

    let span = if end >= start {
      S::new(start.clone(), end.clone())
    } else {
      S::new(end.clone(), end.clone())
    };
    Self {
      before,
      after: Some(after),
      span: Some(span),
      _syntax: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Updates the trailing anchor, returning `self` for chaining.
  #[inline]
  pub fn with_after(mut self, after: S) -> Self {
    self.after = Some(after);
    self
  }

  /// Sets/overwrites the trailing anchor in-place.
  #[inline]
  pub fn set_after(&mut self, after: S) {
    self.after = Some(after);
  }

  /// Returns the span immediately preceding the missing syntax.
  #[inline]
  pub const fn before(&self) -> S
  where
    S: Copy,
  {
    self.before
  }

  /// Returns the optional span immediately following the missing syntax.
  #[inline]
  pub const fn after(&self) -> Option<S>
  where
    S: Copy,
  {
    self.after
  }

  /// Returns the span representing the gap where the syntax should have existed.
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    match self.span.as_ref() {
      Some(span) => *span,
      None => self.before,
    }
  }

  /// Returns the span representing the gap where the syntax should have existed.
  #[inline]
  pub const fn span_ref(&self) -> &S {
    match self.span.as_ref() {
      Some(span) => span,
      None => &self.before,
    }
  }

  /// Bumps the spans of the missing node by the specified offset.
  #[inline]
  pub fn bump(&mut self, offset: &S::Offset) -> &mut Self
  where
    S: Span,
  {
    self.before.bump(offset);
    if let Some(after) = &mut self.after {
      after.bump(offset);
      self.span = Some(Self::full_span(&self.before, after));
    }

    self
  }

  /// Returns the syntax kind of the missing node.
  #[inline]
  pub const fn kind(&self) -> <Lang as Language>::SyntaxKind
  where
    T: Syntax<Lang = Lang>,
    Lang: Language,
  {
    T::KIND
  }

  fn full_span(before: &S, after: &S) -> S
  where
    S: Span,
  {
    let before_start = before.start_ref();
    let before_end = before.end_ref();
    let end_start = after.start_ref();
    let end_end = after.end_ref();

    assert!(
      end_start >= before_end,
      "cannot determine full span: before.end() > after.start()"
    );

    assert!(
      end_end >= before_start,
      "cannot determine full span: before.start() > after.end()"
    );

    assert!(
      before_end >= before_start,
      "cannot determine full span: before.end() < before.start()"
    );

    S::new(before_start.clone(), end_end.clone())
  }
}

impl<T, S, Lang> fmt::Display for Missing<T, S, Lang>
where
  T: Syntax<Lang = Lang>,
  Lang: Language,
  <Lang as Language>::SyntaxKind: fmt::Display,
  S: fmt::Display,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match &self.after {
      Some(after) => write!(
        f,
        "missing {} between {} and {}",
        self.kind(),
        self.before,
        after
      ),
      None => write!(f, "missing {} after {}", self.kind(), self.before),
    }
  }
}

impl<T, S, Lang> fmt::Debug for Missing<T, S, Lang>
where
  S: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Missing")
      .field("before", &self.before)
      .field("after", &self.after)
      .finish()
  }
}

impl<T, S, Lang> core::error::Error for Missing<T, S, Lang>
where
  T: Syntax<Lang = Lang>,
  Lang: Language,
  <Lang as Language>::SyntaxKind: fmt::Display + fmt::Debug,
  S: fmt::Debug + fmt::Display,
{
}
