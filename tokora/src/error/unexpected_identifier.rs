//! Unexpected identifier error type for identifier-based parser error reporting.
//!
//! This module provides the [`UnexpectedIdentifier`] type, which is a specialized error type
//! for identifier-based parsing where the expected values are always static string literals.
//!
//! # Why UnexpectedIdentifier?
//!
//! While [`UnexpectedToken`](super::UnexpectedToken) is general-purpose, `UnexpectedIdentifier`
//! is optimized for the common case of identifier-based languages where:
//! - The found value is always a string (or string-like type)
//! - The expected values are always known string literals (identifiers)
//! - There's always a found value (unlike tokens which might hit end-of-input)
//!
//! # Common Use Cases
//!
//! - Required identifiers: `main`, `self`, `this`, `super`
//! - Fixed-name hooks: `init`, `start`, `cleanup`
//! - DSL-specific identifiers with a small fixed set of names
//!
//! # Example
//!
//! ```
//! use tokora::{SimpleSpan, error::UnexpectedIdentifier};
//!
//! // Parser expected "async" but found "sync"
//! let error = UnexpectedIdentifier::expected_one(
//!     SimpleSpan::new(0, 4),
//!     "sync",
//!     "async"
//! );
//!
//! assert_eq!(error.found(), &"sync");
//! assert_eq!(error.span(), SimpleSpan::new(0, 4));
//! assert_eq!(
//!     format!("{}", error),
//!     "unexpected 'sync', expected 'async' identifier"
//! );
//! ```

use crate::{
  span::{SimpleSpan, Span},
  utils::Expected,
};

/// An error representing an unexpected identifier encountered during parsing.
///
/// This error type is specifically designed for identifier-based parsing where the
/// expected values are known string literals (identifiers). Unlike `UnexpectedToken`,
/// this type always has a found value and the expected values are always static strings.
///
/// # Type Parameters
///
/// * `S` - The type representing the found identifier (often `String` or `&str`)
///
/// # Examples
///
/// ```
/// use tokora::{SimpleSpan, utils::{Expected}, error::UnexpectedIdentifier};
///
/// // Error when expecting a specific identifier
/// let error = UnexpectedIdentifier::expected_one(
///     SimpleSpan::new(10, 16),
///     "return",
///     "fn"
/// );
/// assert_eq!(error.found(), &"return");
/// assert_eq!(error.span(), SimpleSpan::new(10, 16));
/// assert_eq!(format!("{}", error), "unexpected 'return', expected 'fn' identifier");
///
/// // Error when expecting one of multiple identifiers
/// let error = UnexpectedIdentifier::expected_one_of(
///     SimpleSpan::new(0, 5),
///     "class",
///     &["struct", "enum", "trait"]
/// );
/// assert_eq!(
///     format!("{}", error),
///     "unexpected 'class', expected one of: 'struct', 'enum', 'trait' identifier"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnexpectedIdentifier<'a, F, S = SimpleSpan> {
  span: S,
  found: F,
  expected: Expected<'a, &'a str>,
}

impl<'a, F, S> UnexpectedIdentifier<'a, F, S> {
  /// Creates a new unexpected identifier error.
  ///
  /// This is the most general constructor that accepts the span, the found identifier,
  /// and the expected identifier(s) wrapped in an `Expected` enum.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::UnexpectedIdentifier};
  ///
  /// let error = UnexpectedIdentifier::new(
  ///     SimpleSpan::new(5, 8),
  ///     "let",
  ///     Expected::one("const")
  /// );
  /// assert_eq!(error.found(), &"let");
  /// assert_eq!(error.span(), SimpleSpan::new(5, 8));
  /// assert_eq!(format!("{}", error), "unexpected 'let', expected 'const' identifier");
  /// ```
  #[inline]
  pub const fn new(span: S, found: F, expected: Expected<'a, &'a str>) -> Self {
    Self {
      span,
      found,
      expected,
    }
  }

  /// Creates a new unexpected identifier error with a single expected identifier.
  ///
  /// This is a convenience method that combines `new` with `Expected::one`.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedIdentifier};
  ///
  /// let error = UnexpectedIdentifier::expected_one(
  ///     SimpleSpan::new(0, 3),
  ///     "var",
  ///     "let"
  /// );
  /// assert_eq!(error.found(), &"var");
  /// assert_eq!(format!("{}", error), "unexpected 'var', expected 'let' identifier");
  /// ```
  #[inline]
  pub const fn expected_one(span: S, found: F, expected: &'a str) -> Self {
    Self::new(span, found, Expected::one(expected))
  }

  /// Creates a new unexpected identifier error with multiple expected identifier.
  ///
  /// This is a convenience method that combines `new` with `Expected::one_of`.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedIdentifier};
  ///
  /// let error = UnexpectedIdentifier::expected_one_of(
  ///     SimpleSpan::new(10, 18),
  ///     "function",
  ///     &["fn", "async", "const"]
  /// );
  /// assert_eq!(error.found(), &"function");
  /// assert_eq!(
  ///     format!("{}", error),
  ///     "unexpected 'function', expected one of: 'fn', 'async', 'const' identifier"
  /// );
  /// ```
  #[inline]
  pub const fn expected_one_of(span: S, found: F, expected: &'a [&'a str]) -> Self {
    Self::new(span, found, Expected::one_of(expected))
  }

  /// Returns the span of the unexpected identifier.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedIdentifier};
  ///
  /// let error = UnexpectedIdentifier::expected_one(
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

  /// Returns a reference to the found identifier.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::UnexpectedIdentifier};
  ///
  /// let error = UnexpectedIdentifier::expected_one(
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

  /// Returns the expected identifier(s).
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::UnexpectedIdentifier};
  ///
  /// let error = UnexpectedIdentifier::expected_one(
  ///     SimpleSpan::new(5, 11),
  ///     "export",
  ///     "pub"
  /// );
  /// assert_eq!(error.expected(), Expected::one("pub"));
  /// if let Expected::One(identifier) = error.expected() {
  ///     assert_eq!(identifier, "pub");
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
  /// use tokora::{SimpleSpan, error::UnexpectedIdentifier};
  ///
  /// let mut error = UnexpectedIdentifier::expected_one(
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

impl<S: core::fmt::Display> core::fmt::Display for UnexpectedIdentifier<'_, S> {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.expected {
      Expected::One(expected) => {
        write!(
          f,
          "unexpected '{}', expected '{}' identifier",
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
        write!(f, " identifier")
      }
    }
  }
}

impl<S: core::fmt::Debug + core::fmt::Display> core::error::Error for UnexpectedIdentifier<'_, S> {}
