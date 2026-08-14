//! Unterminated sequence error type for tracking incomplete constructs.
//!
//! This module provides the [`Unterminated`] type for representing errors where a
//! multi-character sequence or operator was started but not completed before reaching
//! end-of-input or another syntactic boundary.
//!
//! # Design Philosophy
//!
//! When parsing languages with multi-character operators or sequences, it's common to
//! encounter situations where a sequence is started but never completed. This error type
//! captures both:
//!
//! - **Where** the incomplete sequence was found (via [`SimpleSpan`])
//! - **What** kind of construct was incomplete (via the generic `Knowledge` parameter)
//!
//! # Unclosed vs Unterminated
//!
//! - **`Unclosed`**: For **paired delimiters** that have distinct opening and closing forms
//!   - Examples: `(...)`, `[...]`, `{...}`, `"..."`, `/*...*/`
//!   - The span points to the **opening delimiter** position
//!   - Used when you expect a matching closing delimiter
//!
//! - **`Unterminated`**: For **sequences or operators** that need completion
//!   - Examples: GraphQL's `...` spread operator (where `.` or `..` is incomplete)
//!   - Examples: `<` that should be `<=` or `<<`, `&` that should be `&&`
//!   - The span points to the **incomplete sequence** position
//!   - Used when you expect more characters to complete a construct
//!
//! # Type Parameter
//!
//! - `Knowledge`: The type providing context about what was incomplete (typically a string
//!   or a custom enum describing the expected construct)
//!
//! # Examples
//!
//! ## GraphQL Spread Operator
//!
//! ```rust
//! use tokora::{error::Unterminated, SimpleSpan};
//!
//! // In GraphQL, '...' is the spread operator
//! // If we find only '.' or '..' at EOF, it's unterminated
//! let error = Unterminated::new(SimpleSpan::new(10, 12), "spread operator");
//!
//! assert_eq!(error.span(), SimpleSpan::new(10, 12));
//! assert_eq!(error.knowledge(), "spread operator");
//! assert_eq!(error.to_string(), "unterminated spread operator");
//! ```
//!
//! ## Custom Knowledge Enum
//!
//! ```rust
//! use tokora::{error::Unterminated, SimpleSpan};
//! use core::fmt;
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Operator {
//!     SpreadOperator,    // ... (need 3 dots)
//!     LogicalAnd,        // && (need 2 ampersands)
//!     LeftShift,         // << (need 2 angle brackets)
//!     LessOrEqual,       // <= (need equals after less-than)
//! }
//!
//! impl fmt::Display for Operator {
//!     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//!         match self {
//!             Self::SpreadOperator => write!(f, "spread operator '...'"),
//!             Self::LogicalAnd => write!(f, "logical AND operator '&&'"),
//!             Self::LeftShift => write!(f, "left shift operator '<<'"),
//!             Self::LessOrEqual => write!(f, "less-than-or-equal operator '<='"),
//!         }
//!     }
//! }
//!
//! // Found only '&' when expecting '&&'
//! let error = Unterminated::new(SimpleSpan::new(5, 6), Operator::LogicalAnd);
//! assert_eq!(error.to_string(), "unterminated logical AND operator '&&'");
//! ```
//!
//! ## Incomplete Multi-Character Operators
//!
//! ```rust
//! use tokora::{error::Unterminated, SimpleSpan};
//!
//! // Source: "if x < "
//! //           pos: 5^
//! // Found '<' at EOF, could be '<', '<=', '<<', etc.
//! let error = Unterminated::new(SimpleSpan::new(5, 6), "comparison or shift operator");
//! ```
//!
//! ## Position Adjustment
//!
//! ```rust
//! use tokora::{error::Unterminated, SimpleSpan};
//!
//! // Error from a nested parsing context
//! let mut error = Unterminated::new(SimpleSpan::new(5, 7), "string escape sequence");
//!
//! // Adjust to absolute position in the larger document
//! error.bump(&100);
//! assert_eq!(error.span(), SimpleSpan::new(105, 107));
//! ```

use crate::span::{SimpleSpan, Span};

/// A zero-copy error type representing an unterminated sequence or operator.
///
/// This type tracks the position of an incomplete multi-character sequence,
/// enabling precise error reporting for operators or constructs that require
/// additional characters to be complete.
///
/// # Type Parameter
///
/// - `Knowledge`: The type providing context about what was incomplete (typically
///   `&'static str` or a custom enum). Must implement `Display` for error messages.
///
/// # Common Use Cases
///
/// - **Incomplete spread operators**: `..` instead of `...` in GraphQL or JavaScript
/// - **Incomplete logical operators**: `&` instead of `&&`, `|` instead of `||`
/// - **Incomplete comparison operators**: `<` instead of `<=` or `<<`
/// - **Incomplete escape sequences**: `\` at end of string
/// - **Incomplete multi-char tokens**: `#` instead of `##` for token pasting
///
/// # Design
///
/// The span points to the **incomplete sequence** position (what was actually found),
/// not where the complete sequence was expected. The `Knowledge` parameter provides
/// context about what the complete sequence should have been.
///
/// # Examples
///
/// ## Detecting Incomplete Operators
///
/// ```rust
/// use tokora::{error::Unterminated, SimpleSpan};
///
/// // Found '&' at position 10, expected '&&'
/// let error = Unterminated::new(SimpleSpan::new(10, 11), "logical AND operator");
///
/// println!("Error: {} at position {}", error, error.span().start());
/// // Output: "Error: unterminated logical AND operator at position 10"
/// ```
///
/// ## Tracking Multiple Unterminated Sequences
///
/// ```rust
/// use tokora::{error::Unterminated, SimpleSpan};
///
/// let errors = vec![
///     Unterminated::new(SimpleSpan::new(5, 7), "spread operator"),    // .. instead of ...
///     Unterminated::new(SimpleSpan::new(10, 11), "logical OR"),       // | instead of ||
///     Unterminated::new(SimpleSpan::new(15, 16), "left shift"),       // < instead of <<
/// ];
///
/// for error in errors {
///     eprintln!("Unterminated {} at {}", error.knowledge(), error.span());
/// }
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("unterminated {knowledge}")]
pub struct Unterminated<Knowledge, S = SimpleSpan> {
  /// The span of the incomplete sequence — where the truncated form was actually found, not
  /// where its completion was expected.
  span: S,
  /// What the complete sequence would have been (e.g. `"string literal"`, `"logical AND
  /// operator"`) — the human-facing name rendered by [`Display`](core::fmt::Display).
  knowledge: Knowledge,
}

impl<Knowledge, S> Unterminated<Knowledge, S> {
  /// Creates a new unterminated sequence error.
  ///
  /// The span should point to the position of the incomplete sequence.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unterminated, SimpleSpan};
  ///
  /// // Found '..' instead of '...' at positions 5-7
  /// let error = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  /// assert_eq!(error.span(), SimpleSpan::new(5, 7));
  /// assert_eq!(error.knowledge(), "spread operator");
  /// ```
  #[inline]
  pub const fn new(span: S, knowledge: Knowledge) -> Self {
    Self { span, knowledge }
  }

  /// Returns the span of the incomplete sequence.
  ///
  /// This is the position where the incomplete sequence was found.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unterminated, SimpleSpan};
  ///
  /// let error = Unterminated::new(SimpleSpan::new(10, 11), "logical AND");
  /// assert_eq!(error.span(), SimpleSpan::new(10, 11));
  /// ```
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns a reference to the span of the incomplete sequence.
  #[inline]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a mutable reference to the span of the incomplete sequence.
  #[inline]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Returns a reference to the knowledge about what was incomplete.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unterminated, SimpleSpan};
  ///
  /// let error = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  /// assert_eq!(error.knowledge_ref(), &"spread operator");
  /// ```
  #[inline]
  pub const fn knowledge_ref(&self) -> &Knowledge {
    &self.knowledge
  }

  /// Returns the knowledge about what was incomplete.
  ///
  /// This method is only available when the knowledge type implements `Copy`.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unterminated, SimpleSpan};
  ///
  /// let error = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  /// assert_eq!(error.knowledge(), "spread operator");
  /// ```
  #[inline]
  pub const fn knowledge(&self) -> Knowledge
  where
    Knowledge: Copy,
  {
    self.knowledge
  }

  /// Bumps the span by the given offset.
  ///
  /// This adjusts both the start and end positions of the span, which is useful
  /// when adjusting error positions after processing or when combining errors
  /// from different parsing contexts.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unterminated, SimpleSpan};
  ///
  /// let mut error = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  /// error.bump(&100);
  /// assert_eq!(error.span(), SimpleSpan::new(105, 107));
  /// ```
  #[inline]
  pub fn bump(&mut self, offset: &S::Offset) -> &mut Self
  where
    S: Span,
  {
    self.span.bump(offset);
    self
  }

  /// Consumes the error and returns its components.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unterminated, SimpleSpan};
  ///
  /// let error = Unterminated::new(SimpleSpan::new(10, 12), "escape sequence");
  /// let (span, knowledge) = error.into_components();
  /// assert_eq!(span, SimpleSpan::new(10, 12));
  /// assert_eq!(knowledge, "escape sequence");
  /// ```
  #[inline]
  pub fn into_components(self) -> (S, Knowledge) {
    (self.span, self.knowledge)
  }
}
