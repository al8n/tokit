//! Unexpected keyword error type for keyword-based parser error reporting.
//!
//! This module provides the [`UnexpectedKeyword`] type, which is a specialized error type
//! for keyword-based parsing where the expected values are always static string literals.
//!
//! # Why UnexpectedKeyword?
//!
//! While [`UnexpectedToken`](super::UnexpectedToken) is general-purpose, `UnexpectedKeyword`
//! is optimized for the common case of keyword-based languages where:
//! - The found value is always a string (or string-like type)
//! - The expected values are always known string literals (keywords)
//! - There's always a found value (unlike tokens which might hit end-of-input)
//!
//! # Common Use Cases
//!
//! - Language keyword: `if`, `while`, `for`, `class`, `fn`, etc.
//! - Control flow keyword: `break`, `continue`, `return`
//! - Declaration keyword: `let`, `const`, `var`, `type`
//! - Access modifiers: `pub`, `private`, `protected`
//!
//! # Example
//!
//! ```
//! use tokora::{SimpleSpan, error::UnexpectedKeyword};
//!
//! // Parser expected "async" but found "sync"
//! let error = UnexpectedKeyword::expected_one(
//!     SimpleSpan::new(0, 4),
//!     "sync",
//!     "async"
//! );
//!
//! assert_eq!(error.found(), &"sync");
//! assert_eq!(error.span(), SimpleSpan::new(0, 4));
//! assert_eq!(
//!     format!("{}", error),
//!     "unexpected 'sync', expected 'async' keyword"
//! );
//! ```

use crate::{
  span::{SimpleSpan, Span},
  utils::Expected,
};

/// An error representing an unexpected keyword encountered during parsing.
///
/// This error type is specifically designed for keyword-based parsing where the
/// expected values are known string literals (keyword). Unlike `UnexpectedToken`,
/// this type always has a found value and the expected values are always static strings.
///
/// # Type Parameters
///
/// * `S` - The type representing the found keyword (often `String` or `&str`)
///
/// # Examples
///
/// ```
/// use tokora::{SimpleSpan, utils::{Expected}, error::UnexpectedKeyword};
///
/// // Error when expecting a specific keyword
/// let error = UnexpectedKeyword::expected_one(
///     SimpleSpan::new(10, 16),
///     "return",
///     "fn"
/// );
/// assert_eq!(error.found(), &"return");
/// assert_eq!(error.span(), SimpleSpan::new(10, 16));
/// assert_eq!(format!("{}", error), "unexpected 'return', expected 'fn' keyword");
///
/// // Error when expecting one of multiple keywords
/// let error = UnexpectedKeyword::expected_one_of(
///     SimpleSpan::new(0, 5),
///     "class",
///     &["struct", "enum", "trait"]
/// );
/// assert_eq!(
///     format!("{}", error),
///     "unexpected 'class', expected one of: 'struct', 'enum', 'trait' keyword"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnexpectedKeyword<'a, F, S = SimpleSpan> {
  span: S,
  found: F,
  expected: Expected<'a, &'a str>,
}

impl<'a, F, S> UnexpectedKeyword<'a, F, S> {
  /// Creates a new unexpected keyword error.
  ///
  /// This is the most general constructor that accepts the span, the found keyword,
  /// and the expected keyword(s) wrapped in an `Expected` enum.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::UnexpectedKeyword};
  ///
  /// let error = UnexpectedKeyword::new(
  ///     SimpleSpan::new(5, 8),
  ///     "let",
  ///     Expected::one("const")
  /// );
  /// assert_eq!(error.found(), &"let");
  /// assert_eq!(error.span(), SimpleSpan::new(5, 8));
  /// assert_eq!(format!("{}", error), "unexpected 'let', expected 'const' keyword");
  /// ```
  #[inline]
  pub const fn new(span: S, found: F, expected: Expected<'a, &'a str>) -> Self {
    Self {
      span,
      found,
      expected,
    }
  }

  /// Creates a new unexpected keyword error with a single expected keyword.
  ///
  /// This is a convenience method that combines `new` with `Expected::one`.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedKeyword};
  ///
  /// let error = UnexpectedKeyword::expected_one(
  ///     SimpleSpan::new(0, 3),
  ///     "var",
  ///     "let"
  /// );
  /// assert_eq!(error.found(), &"var");
  /// assert_eq!(format!("{}", error), "unexpected 'var', expected 'let' keyword");
  /// ```
  #[inline]
  pub const fn expected_one(span: S, found: F, expected: &'a str) -> Self {
    Self::new(span, found, Expected::one(expected))
  }

  /// Creates a new unexpected keywords error with multiple expected keyword.
  ///
  /// This is a convenience method that combines `new` with `Expected::one_of`.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedKeyword};
  ///
  /// let error = UnexpectedKeyword::expected_one_of(
  ///     SimpleSpan::new(10, 18),
  ///     "function",
  ///     &["fn", "async", "const"]
  /// );
  /// assert_eq!(error.found(), &"function");
  /// assert_eq!(
  ///     format!("{}", error),
  ///     "unexpected 'function', expected one of: 'fn', 'async', 'const' keyword"
  /// );
  /// ```
  #[inline]
  pub const fn expected_one_of(span: S, found: F, expected: &'a [&'a str]) -> Self {
    Self::new(span, found, Expected::one_of(expected))
  }

  /// Returns the span of the unexpected keyword.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedKeyword};
  ///
  /// let error = UnexpectedKeyword::expected_one(
  ///     SimpleSpan::new(20, 26),
  ///     "import",
  ///     "use"
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(20, 26));
  /// ```
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns the span of the unexpected keyword.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedKeyword};
  ///
  /// let error = UnexpectedKeyword::expected_one(
  ///     SimpleSpan::new(20, 26),
  ///     "import",
  ///     "use"
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(20, 26));
  /// ```
  #[inline]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a reference to the found keyword.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedKeyword};
  ///
  /// let error = UnexpectedKeyword::expected_one(
  ///     SimpleSpan::new(0, 6),
  ///     "import",
  ///     "use"
  /// );
  /// assert_eq!(error.found(), &"import");
  /// ```
  #[inline]
  pub const fn found(&self) -> &F {
    &self.found
  }

  /// Returns the expected keyword(s).
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::UnexpectedKeyword};
  ///
  /// let error = UnexpectedKeyword::expected_one(
  ///     SimpleSpan::new(5, 11),
  ///     "export",
  ///     "pub"
  /// );
  /// assert_eq!(error.expected(), Expected::one("pub"));
  /// if let Expected::One(keyword) = error.expected() {
  ///     assert_eq!(keyword, "pub");
  /// }
  /// ```
  #[inline]
  pub fn expected(&self) -> Expected<'a, &'a str> {
    self.expected.clone()
  }

  /// Bumps both the start and end positions of the span by the given offset.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedKeyword};
  ///
  /// let mut error = UnexpectedKeyword::expected_one(
  ///     SimpleSpan::new(10, 13),
  ///     "var",
  ///     "let"
  /// );
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 18));
  /// ```
  #[inline]
  pub fn bump(&mut self, offset: &S::Offset) -> &mut Self
  where
    S: Span,
  {
    self.span.bump(offset);
    self
  }
}

impl<S: core::fmt::Display> core::fmt::Display for UnexpectedKeyword<'_, S> {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.expected {
      Expected::One(expected) => {
        write!(
          f,
          "unexpected '{}', expected '{}' keyword",
          self.found, expected
        )
      }
      Expected::OneOf(expected) => {
        write!(f, "unexpected '{}', expected one of: ", self.found)?;
        for (i, kw) in expected.iter().enumerate() {
          if i > 0 {
            write!(f, ", ")?;
          }
          write!(f, "'{}'", kw)?;
        }
        write!(f, " keyword")
      }
    }
  }
}

impl<S: core::fmt::Debug + core::fmt::Display> core::error::Error for UnexpectedKeyword<'_, S> {}
