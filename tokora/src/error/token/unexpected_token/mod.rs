//! Unexpected token error type for parser error reporting.
//!
//! This module provides the [`UnexpectedToken`] type, which represents parser errors
//! when an unexpected token is encountered. It captures both the location of the error,
//! what token was found (if any), and what tokens were expected.
//!
//! # Design Philosophy
//!
//! `UnexpectedToken` is designed to provide rich, actionable error messages:
//! - **Location tracking**: The `span` field pinpoints exactly where the error occurred
//! - **Optional found token**: Distinguishes between unexpected tokens and end-of-input
//! - **Flexible expectations**: Can express single or multiple alternative expected tokens
//! - **Position adjustment**: The `bump()` method allows adjusting error positions when
//!   combining errors from different parsing contexts
//!
//! # Common Patterns
//!
//! ## End of Input Errors
//!
//! When the parser reaches the end of input unexpectedly, use constructors without a found token:
//!
//! ```
//! use tokora::{SimpleSpan, error::token::UnexpectedToken};
//!
//! // Simple end-of-input error (no found token)
//! let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
//!     SimpleSpan::new(100, 100),
//!     "}"
//! );
//! assert!(error.found().is_none());
//! assert_eq!(error.span(), SimpleSpan::new(100, 100));
//! ```
//!
//! ## Unexpected Token Errors
//!
//! When a specific token was found but something else was expected:
//!
//! ```
//! use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
//!
//! let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
//!     SimpleSpan::new(10, 15),
//!     "else",
//!     "if"
//! );
//! assert_eq!(error.span(), SimpleSpan::new(10, 15));
//! assert_eq!(error.found(), Some(&"else"));
//! assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "if"));
//! ```

use core::marker::PhantomData;

use crate::{
  Lexer, Token,
  span::{SimpleSpan, Span},
  utils::Expected,
};

/// A type alias for an `UnexpectedToken` error for a given lexer and language.
pub type UnexpectedTokenOf<'inp, L, Lang = ()> = UnexpectedToken<
  'inp,
  <L as Lexer<'inp>>::Token,
  <<L as Lexer<'inp>>::Token as Token<'inp>>::Kind,
  <L as Lexer<'inp>>::Span,
  Lang,
>;

/// An error representing an unexpected token encountered during parsing.
///
/// This error type captures the location (span), what token was found (if any),
/// and what token(s) were expected. It's commonly used in parsers to provide
/// detailed error messages when the input doesn't match the expected syntax.
///
/// # Type Parameters
///
/// * `T` - The type of the actual token that was found
/// * `Kind` - The type of the expected token (often an enum of token kinds)
///
/// # Examples
///
/// ```
/// use tokora::{SimpleSpan, utils::Expected, error::token::UnexpectedToken};
///
/// // Error when expecting a specific token but got something else
/// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
///     SimpleSpan::new(10, 15),
///     "}",
///     "{"
/// );
/// assert_eq!(error.span(), SimpleSpan::new(10, 15));
/// assert_eq!(error.found(), Some(&"}"));
/// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "{"));
///
/// // Error when expecting one of multiple tokens
/// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_of_with_found(
///     SimpleSpan::new(0, 10),
///     "identifier",
///     &["if", "while", "for"]
/// );
/// assert_eq!(error.found(), Some(&"identifier"));
/// if let Some(Expected::OneOf(values)) = error.expected() {
///     assert_eq!(values.as_slice(), &["if", "while", "for"]);
/// }
///
/// // Error when reaching end of input unexpectedly
/// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
///     SimpleSpan::new(100, 100),
///     "}"
/// );
/// assert!(error.found().is_none());
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct UnexpectedToken<'a, T, Kind: Clone, S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  found: Option<T>,
  expected: Option<Expected<'a, Kind>>,
  _lang: PhantomData<Lang>,
}

// Allow unit to be used as an error sink for tests and no-op emitters.
impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for () {
  #[inline(always)]
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {}
}

impl<'a, T, Kind: Clone, S> UnexpectedToken<'a, T, Kind, S> {
  /// Creates a new unexpected token error.
  ///
  /// This error indicates that an unexpected token was encountered,
  /// without specifying what token was found or expected.
  #[inline(always)]
  pub const fn new(span: S) -> Self {
    Self::of(span)
  }

  /// Creates an unexpected token error without a found token.
  ///
  /// This is useful when the parser reaches the end of input unexpectedly.
  /// The error will indicate "unexpected token" in its display message.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Expected, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::with_expected(
  ///     SimpleSpan::new(100, 101),
  ///     Expected::one("}")
  /// );
  /// assert!(error.found().is_none());
  /// assert_eq!(error.span(), SimpleSpan::new(100, 101));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "}"));
  /// ```
  #[inline(always)]
  pub const fn with_expected(span: S, expected: Expected<'a, Kind>) -> Self {
    Self::with_expected_of(span, expected)
  }

  /// Creates an unexpected token error without a found token.
  ///
  /// This is useful when the parser reaches the end of input unexpectedly.
  /// The error will indicate "unexpected token" in its display message.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::maybe_expected(
  ///     SimpleSpan::new(100, 101),
  ///     Some(Expected::one("}"))
  /// );
  /// assert!(error.found().is_none());
  /// assert_eq!(error.span(), SimpleSpan::new(100, 101));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "}"));
  /// ```
  #[inline(always)]
  pub const fn maybe_expected(span: S, expected: Option<Expected<'a, Kind>>) -> Self {
    Self::maybe_expected_of(span, expected)
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> UnexpectedToken<'a, T, Kind, S, Lang> {
  #[inline(always)]
  pub(super) const fn new_in(
    span: S,
    found: Option<T>,
    expected: Option<Expected<'a, Kind>>,
  ) -> Self {
    Self {
      span,
      found,
      expected,
      _lang: PhantomData,
    }
  }

  /// Creates a new unexpected token error.
  ///
  /// This error indicates that an unexpected token was encountered,
  /// without specifying what token was found or expected.
  #[inline(always)]
  pub const fn of(span: S) -> Self {
    Self::new_in(span, None, None)
  }

  /// Creates an unexpected token error without a found token.
  ///
  /// This is useful when the parser reaches the end of input unexpectedly.
  /// The error will indicate "unexpected token" in its display message.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::with_expected_of(
  ///     SimpleSpan::new(100, 101),
  ///     Expected::one("}")
  /// );
  /// assert!(error.found().is_none());
  /// assert_eq!(error.span(), SimpleSpan::new(100, 101));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "}"));
  /// ```
  #[inline(always)]
  pub const fn with_expected_of(span: S, expected: Expected<'a, Kind>) -> Self {
    Self::new_in(span, None, Some(expected))
  }

  /// Creates an unexpected token error without a found token.
  ///
  /// This is useful when the parser reaches the end of input unexpectedly.
  /// The error will indicate "unexpected token" in its display message.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::maybe_expected_of(
  ///     SimpleSpan::new(100, 101),
  ///     Some(Expected::one("}"))
  /// );
  /// assert!(error.found().is_none());
  /// assert_eq!(error.span(), SimpleSpan::new(100, 101));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "}"));
  /// ```
  #[inline(always)]
  pub const fn maybe_expected_of(span: S, expected: Option<Expected<'a, Kind>>) -> Self {
    Self::new_in(span, None, expected)
  }

  /// Creates a new unexpected token error with a single expected token.
  ///
  /// This is a convenience method that combines `new` with `Expected::one`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Expected, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(50, 51),
  ///     ";"
  /// );
  /// assert!(error.found().is_none());
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == ";"));
  /// ```
  #[inline(always)]
  pub const fn expected_one(span: S, expected: Kind) -> Self {
    Self::with_expected_of(span, Expected::one(expected))
  }

  /// Creates a new unexpected token error with a single expected token.
  ///
  /// This is a convenience method that combines `new` with `Expected::one`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Expected, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(50, 51),
  ///     ":",
  ///     ";"
  /// );
  /// assert!(error.found().is_some());
  /// assert_eq!(error.found(), Some(&":"));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == ";"));
  /// ```
  #[inline(always)]
  pub const fn expected_one_with_found(span: S, found: T, expected: Kind) -> Self {
    Self::new_in(span, Some(found), Some(Expected::one(expected)))
  }

  /// Creates a new unexpected token error with multiple expected tokens.
  ///
  /// This is a convenience method that combines `new` with `Expected::one_of`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Expected, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_of(
  ///     SimpleSpan::new(25, 26),
  ///     &["+", "-", "*", "/"]
  /// );
  /// assert!(error.found().is_none());
  /// if let Some(Expected::OneOf(values)) = error.expected() {
  ///     assert_eq!(values.as_slice(), &["+", "-", "*", "/"]);
  /// }
  /// ```
  #[inline(always)]
  pub const fn expected_one_of(span: S, expected: &'static [Kind]) -> Self {
    Self::with_expected_of(span, Expected::one_of(expected))
  }

  /// Creates a new unexpected token error with multiple expected tokens.
  ///
  /// This is a convenience method that combines `new` with `Expected::one_of`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Expected, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_of_with_found(
  ///     SimpleSpan::new(25, 26),
  ///     ":",
  ///     &["+", "-", "*", "/"]
  /// );
  /// assert!(!error.found().is_none());
  /// assert_eq!(error.found(), Some(&":"));
  /// if let Some(Expected::OneOf(values)) = error.expected() {
  ///     assert_eq!(values.as_slice(), &["+", "-", "*", "/"]);
  /// }
  /// ```
  #[inline(always)]
  pub const fn expected_one_of_with_found(span: S, found: T, expected: &'static [Kind]) -> Self {
    Self::new_in(span, Some(found), Some(Expected::one_of(expected)))
  }

  /// Creates a new unexpected token error with an optional found token.
  ///
  /// This is the most general constructor. When `found` is `None`, the error
  /// indicates the end of input was reached. When `found` is `Some`, it indicates
  /// an unexpected token was encountered.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// // With a found token
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(10, 14),
  ///     "if"
  /// ).maybe_found(Some("else"));
  /// assert_eq!(error.found(), Some(&"else"));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "if"));
  ///
  /// // Without a found token (end of input)
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(50, 50),
  ///     "if"
  /// ).maybe_found(None);
  /// assert_eq!(error.found(), None);
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "if"));
  /// ```
  #[inline(always)]
  pub fn maybe_found(mut self, found: Option<T>) -> Self {
    self.found = found;
    self
  }

  /// Creates a new unexpected token error with an optional found token.
  ///
  /// This is the most general constructor. When `found` is `None`, the error
  /// indicates the end of input was reached. When `found` is `Some`, it indicates
  /// an unexpected token was encountered.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// // With a found token
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(10, 14),
  ///     "if"
  /// ).maybe_found_const(Some("else"));
  /// assert_eq!(error.found(), Some(&"else"));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "if"));
  ///
  /// // Without a found token (end of input)
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(50, 50),
  ///     "if"
  /// ).maybe_found_const(None);
  /// assert_eq!(error.found(), None);
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "if"));
  /// ```
  #[inline(always)]
  pub fn maybe_found_const(mut self, found: Option<T>) -> Self
  where
    T: Copy,
  {
    self.found = found;
    self
  }

  /// Creates a new unexpected token error with a found token.
  ///
  /// This indicates that a specific token was encountered when a different
  /// token (or one of several alternative tokens) was expected.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(5, 10),
  ///     "fn"
  /// ).with_found("class");
  /// assert_eq!(error.found(), Some(&"class"));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "fn"));
  /// ```
  #[inline(always)]
  pub fn with_found(mut self, found: T) -> Self {
    self.found = Some(found);
    self
  }

  /// Creates a new unexpected token error with a found token.
  ///
  /// This indicates that a specific token was encountered when a different
  /// token (or one of several alternative tokens) was expected.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(5, 10),
  ///     "fn"
  /// ).with_found_const("class");
  /// assert_eq!(error.found(), Some(&"class"));
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "fn"));
  /// ```
  #[inline(always)]
  pub fn with_found_const(mut self, found: T) -> Self
  where
    T: Copy,
  {
    self.found = Some(found);
    self
  }

  /// Returns the span of the unexpected token.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(10, 15),
  ///     "identifier",
  ///     "number"
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(10, 15));
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns the span of the unexpected token.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(10, 15),
  ///     "identifier",
  ///     "number"
  /// );
  /// assert_eq!(error.span_ref(), &SimpleSpan::new(10, 15));
  /// ```
  #[inline(always)]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a mutable reference to the span of the unexpected token.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let mut error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(10, 15),
  ///     "identifier",
  ///     "number"
  /// );
  /// *error.span_mut() = SimpleSpan::new(15, 20);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 20));
  /// ```
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Returns a reference to the found token, if any.
  ///
  /// Returns `None` if the error represents an unexpected end of input.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(0, 10),
  ///     "identifier",
  ///     "number"
  /// );
  /// assert_eq!(error.found(), Some(&"identifier"));
  ///
  /// let eof_error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one(
  ///     SimpleSpan::new(100, 100),
  ///     "}"
  /// );
  /// assert_eq!(eof_error.found(), None);
  /// ```
  #[inline(always)]
  pub const fn found(&self) -> Option<&T> {
    self.found.as_ref()
  }

  /// Returns a reference to the expected token(s).
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(5, 6),
  ///     "}",
  ///     "{"
  /// );
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "{"));
  /// ```
  #[inline(always)]
  pub const fn expected(&self) -> Option<&Expected<'a, Kind>> {
    self.expected.as_ref()
  }

  /// Bumps both the start and end positions of the span by the given offset.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let mut error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(10, 15),
  ///     "}",
  ///     "{"
  /// );
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 20));
  /// ```
  #[inline(always)]
  pub fn bump(&mut self, offset: &S::Offset)
  where
    S: Span,
  {
    self.span.bump(offset);
  }

  /// Maps the expected token(s) using the provided function.
  ///
  /// This is useful for transforming the expected token type while preserving
  /// the rest of the error information.
  ///
  /// ## Examples
  ///
  /// ```
  /// # #[cfg(feature = "std")] {
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(0, 5),
  ///     "identifier",
  ///     "number"
  /// );
  /// let mapped_error = error.map_expected(|expected| {
  ///     // Transform the expected token type here
  ///     Expected::one(expected.unwrap_one().to_string())
  /// });
  /// # }
  /// ```
  pub fn map_expected<F, Kind2>(self, f: F) -> UnexpectedToken<'a, T, Kind2, S>
  where
    F: FnOnce(Expected<'a, Kind>) -> Expected<'a, Kind2>,
    Kind2: Clone,
  {
    UnexpectedToken {
      span: self.span,
      found: self.found,
      expected: self.expected.map(f),
      _lang: PhantomData,
    }
  }

  /// Consumes the error and returns its components.
  ///
  /// This method deconstructs the error into its constituent parts:
  /// the span, the found token (if any), and the expected token(s).
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Expected}, error::token::UnexpectedToken};
  ///
  /// let error: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::expected_one_with_found(
  ///     SimpleSpan::new(5, 6),
  ///     "}",
  ///     "{"
  /// );
  /// let (span, found, expected) = error.into_components();
  /// assert_eq!(span, SimpleSpan::new(5, 6));
  /// assert_eq!(found, Some("}"));
  /// assert_eq!(expected, Some(Expected::one("{")));
  /// ```
  #[inline(always)]
  pub fn into_components(self) -> (S, Option<T>, Option<Expected<'a, Kind>>) {
    (self.span, self.found, self.expected)
  }
}

impl<T, Kind: Clone, S, Lang: ?Sized> UnexpectedToken<'_, T, Kind, S, Lang>
where
  S: Span,
{
  /// Creates a debug representation of the unexpected token error.
  pub fn debug_fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result
  where
    T: core::fmt::Debug,
    Kind: core::fmt::Debug,
    S: core::fmt::Debug,
  {
    f.debug_struct("UnexpectedToken")
      .field("span", &self.span)
      .field("found", &self.found)
      .field("expected", &self.expected)
      .finish()
  }

  /// Creates a display representation of the unexpected token error.
  pub fn display_fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result
  where
    T: core::fmt::Display,
    Kind: core::fmt::Display,
  {
    // Same composition rule as `MissingToken::display_fmt`: `Expected`'s `Display` opens with
    // the word "expected" in every variant, so these sites supply only the separator. This
    // carrier's renders were never pinned, which is why its stutter outlived the one that was.
    match &self.found {
      Some(found) => match &self.expected {
        Some(expected) => write!(f, "unexpected token '{}', {}", found, expected),
        None => write!(f, "unexpected token '{}'", found),
      },
      None => match &self.expected {
        Some(expected) => write!(f, "unexpected token, {}", expected),
        None => write!(f, "unexpected token"),
      },
    }
  }
}
