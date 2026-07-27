//! Unclosed delimiter error type for tracking missing closing delimiters.
//!
//! This module provides the [`Unclosed`] type for representing errors where an opening
//! delimiter was found but never closed before reaching end-of-input or another syntactic
//! boundary.
//!
//! # Design Philosophy
//!
//! When parsing structured text with paired delimiters (parentheses, brackets, braces,
//! quotes, etc.), it's common to encounter situations where an opening delimiter is found
//! but the corresponding closing delimiter is missing. This error type captures both:
//!
//! - **Where** the opening delimiter was found (via [`SimpleSpan`])
//! - **What** delimiter name was left unclosed (stored as a [`CowStr`])
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
//!   - The span points to the **incomplete sequence** position
//!   - Used when you expect more characters to complete a construct
//!
//! # Type Parameter
//!
//! - `Delimiter`: A type-level tag for the delimiter (typically `char`, `&'static str`, or a custom enum)
//!
//! # Examples
//!
//! ## Basic Usage with Character Delimiters
//!
//! ```rust
//! use tokora::{error::Unclosed, SimpleSpan};
//!
//! // Opening parenthesis at position 10, never closed
//! let error = Unclosed::<char>::new(SimpleSpan::new(10, 11), "(".into());
//!
//! assert_eq!(error.span(), SimpleSpan::new(10, 11));
//! assert_eq!(error.name_ref(), "(");
//! assert_eq!(error.to_string(), "unclosed delimiter '('");
//! ```
//!
//! ## Custom Delimiter Enum
//!
//! ```rust
//! use tokora::{error::Unclosed, SimpleSpan};
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Delimiter {
//!     Paren,      // ()
//!     Bracket,    // []
//!     Brace,      // {}
//!     Quote,      // ""
//!     BlockComment, // /**/
//! }
//!
//! let error: Unclosed<Delimiter> = Unclosed::new(SimpleSpan::new(5, 6), "/*".into());
//! assert_eq!(error.name_ref(), "/*");
//! ```
//!
//! ## Tracking Nested Delimiters
//!
//! ```rust
//! use tokora::{error::Unclosed, SimpleSpan};
//!
//! // When parsing: "{ foo: [ bar, baz }"
//! // The '[' at position 7 is never closed
//! let error = Unclosed::<char>::new(SimpleSpan::new(7, 8), "[".into());
//!
//! // Error reporting can show:
//! // "error at 7..8: unclosed delimiter '['"
//! ```
//!
//! ## Position Adjustment
//!
//! ```rust
//! use tokora::{error::Unclosed, SimpleSpan};
//!
//! // Error from a nested parsing context
//! let mut error = Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into());
//!
//! // Adjust to absolute position in the larger document
//! error.bump(&100);
//! assert_eq!(error.span(), SimpleSpan::new(105, 106));
//! ```

use core::marker::PhantomData;

use crate::{
  Lexer,
  emitter::FromUnclosed,
  punct::{Angle, Brace, Bracket, Paren},
  span::{SimpleSpan, Span},
  utils::CowStr,
};

/// An unclosed bracket error
pub type UnclosedBracket<S = SimpleSpan, Lang = ()> = Unclosed<Bracket, S, Lang>;
/// An unclosed parenthesis error
pub type UnclosedParen<S = SimpleSpan, Lang = ()> = Unclosed<Paren, S, Lang>;
/// An unclosed brace error
pub type UnclosedBrace<S = SimpleSpan, Lang = ()> = Unclosed<Brace, S, Lang>;
/// An unclosed angle bracket error
pub type UnclosedAngle<S = SimpleSpan, Lang = ()> = Unclosed<Angle, S, Lang>;

/// A zero-copy error type representing an unclosed delimiter.
///
/// This type tracks the position of an opening delimiter that was never closed,
/// enabling precise error reporting for missing closing delimiters in structured text.
///
/// # Type Parameter
///
/// - `Delimiter`: The type representing the delimiter (typically `char`, `&'static str`,
///   or a custom enum). Must implement `Display` for error messages.
///
/// # Common Use Cases
///
/// - **Unmatched parentheses** in expressions: `(a + b * c`
/// - **Unclosed strings** in source code: `"hello world`
/// - **Missing closing braces** in JSON: `{"key": "value"`
/// - **Incomplete block comments**: `/* This comment never ends`
/// - **Unmatched brackets** in arrays: `[1, 2, 3`
///
/// # Design
///
/// The span points to the **opening delimiter** position, not where the closing
/// delimiter was expected. This allows error messages to point users to where
/// the delimiter was opened, making it easier to find and fix the issue.
///
/// # Examples
///
/// ## Detecting Unclosed Parentheses
///
/// ```rust
/// use tokora::{error::Unclosed, SimpleSpan};
///
/// // Parse error: (1 + 2
/// //              ^--- unclosed
/// let error = Unclosed::<char>::new(SimpleSpan::new(0, 1), "(".into());
///
/// println!("Error: {} at position {}", error, error.span().start());
/// // Output: "Error: unclosed delimiter '(' at position 0"
/// ```
///
/// ## Tracking Multiple Unclosed Delimiters
///
/// ```rust
/// use tokora::{error::Unclosed, SimpleSpan};
///
/// let errors = vec![
///     Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into()),
///     Unclosed::<char>::new(SimpleSpan::new(10, 11), "[".into()),
///     Unclosed::<char>::new(SimpleSpan::new(15, 16), "(".into()),
/// ];
///
/// for error in errors {
///     eprintln!("Unclosed {} at {}", error.name_ref(), error.span());
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Unclosed<Delimiter, S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  name: CowStr,
  _delimiter: PhantomData<Delimiter>,
  _lang: PhantomData<Lang>,
}

impl<Delimiter, O, Lang: ?Sized> core::fmt::Display for Unclosed<Delimiter, O, Lang>
where
  Delimiter: core::fmt::Display,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "unclosed delimiter '{}'", self.name)
  }
}

impl<Delimiter, O, Lang: ?Sized> core::error::Error for Unclosed<Delimiter, O, Lang>
where
  Delimiter: core::fmt::Display + core::fmt::Debug,
  O: core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

impl<S> Unclosed<Paren, S> {
  /// Creates a new unclosed parenthesis error.
  ///
  /// The span should cover the opening parenthesis to the end of the syntax element.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening parenthesis at position 3
  /// let error = Unclosed::paren(SimpleSpan::new(3, 4));
  /// assert_eq!(error.span(), SimpleSpan::new(3, 4));
  /// assert_eq!(error.name_ref(), "()");
  /// ```
  #[inline(always)]
  pub const fn paren(span: S) -> Self {
    Self::paren_of(span)
  }
}

impl<S, Lang: ?Sized> Unclosed<Paren, S, Lang> {
  /// Creates a new unclosed parenthesis error.
  ///
  /// The span should cover the opening parenthesis to the end of the syntax element.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening parenthesis at position 3
  /// let error = Unclosed::paren(SimpleSpan::new(3, 4));
  /// assert_eq!(error.span(), SimpleSpan::new(3, 4));
  /// assert_eq!(error.name_ref(), "()");
  /// ```
  #[inline(always)]
  pub const fn paren_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("()"))
  }
}

impl<S> Unclosed<Bracket, S> {
  /// Creates a new unclosed bracket error.
  ///
  /// The span should point to the position of the opening bracket.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening bracket at position 8
  /// let error = Unclosed::bracket(SimpleSpan::new(8, 9));
  /// assert_eq!(error.span(), SimpleSpan::new(8, 9));
  /// assert_eq!(error.name_ref(), "[]");
  /// ```
  #[inline(always)]
  pub const fn bracket(span: S) -> Self {
    Self::bracket_of(span)
  }
}

impl<S, Lang: ?Sized> Unclosed<Bracket, S, Lang> {
  /// Creates a new unclosed bracket error.
  ///
  /// The span should point to the position of the opening bracket.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening bracket at position 8
  /// let error = Unclosed::bracket(SimpleSpan::new(8, 9));
  /// assert_eq!(error.span(), SimpleSpan::new(8, 9));
  /// assert_eq!(error.name_ref(), "[]");
  /// ```
  #[inline(always)]
  pub const fn bracket_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("[]"))
  }
}

impl<S> Unclosed<Brace, S> {
  /// Creates a new unclosed brace error.
  ///
  /// The span should point to the position of the opening brace.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening brace at position 12
  /// let error = Unclosed::brace(SimpleSpan::new(12, 13));
  /// assert_eq!(error.span(), SimpleSpan::new(12, 13));
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline(always)]
  pub const fn brace(span: S) -> Self {
    Self::brace_of(span)
  }
}

impl<S, Lang: ?Sized> Unclosed<Brace, S, Lang> {
  /// Creates a new unclosed brace error.
  ///
  /// The span should point to the position of the opening brace.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening brace at position 12
  /// let error = Unclosed::brace(SimpleSpan::new(12, 13));
  /// assert_eq!(error.span(), SimpleSpan::new(12, 13));
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline(always)]
  pub const fn brace_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("{}"))
  }
}

impl<S> Unclosed<Angle, S> {
  /// Creates a new unclosed angle bracket error.
  ///
  /// The span should point to the position of the opening angle bracket.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening angle bracket at position 20
  /// let error = Unclosed::angle(SimpleSpan::new(20, 21));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 21));
  /// assert_eq!(error.name_ref(), "<>");
  /// ```
  #[inline(always)]
  pub const fn angle(span: S) -> Self {
    Self::angle_of(span)
  }
}

impl<S, Lang: ?Sized> Unclosed<Angle, S, Lang> {
  /// Creates a new unclosed angle bracket error.
  ///
  /// The span should point to the position of the opening angle bracket.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening angle bracket at position 20
  /// let error = Unclosed::angle(SimpleSpan::new(20, 21));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 21));
  /// assert_eq!(error.name_ref(), "<>");
  /// ```
  #[inline(always)]
  pub const fn angle_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("<>"))
  }
}

impl<Delimiter, S> Unclosed<Delimiter, S> {
  /// Creates a new unclosed delimiter error.
  ///
  /// The span should point to the position of the opening delimiter.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening brace at position 5
  /// let error = Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into());
  /// assert_eq!(error.span(), SimpleSpan::new(5, 6));
  /// assert_eq!(error.name_ref(), "{");
  /// ```
  #[inline(always)]
  pub const fn new(span: S, name: CowStr) -> Self {
    Self::of(span, name)
  }
}

impl<Delimiter, S, Lang: ?Sized> Unclosed<Delimiter, S, Lang> {
  /// Creates a new unclosed delimiter error.
  ///
  /// The span should point to the position of the opening delimiter.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// // Opening brace at position 5
  /// let error = Unclosed::<char>::of(SimpleSpan::new(5, 6), "{".into());
  /// assert_eq!(error.span(), SimpleSpan::new(5, 6));
  /// assert_eq!(error.name_ref(), "{");
  /// ```
  #[inline(always)]
  pub const fn of(span: S, name: CowStr) -> Self {
    Self {
      span,
      name,
      _delimiter: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Returns the span of the opening delimiter.
  ///
  /// This is the position where the delimiter was opened but never closed.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// let error = Unclosed::<char>::new(SimpleSpan::new(10, 11), "(".into());
  /// assert_eq!(error.span(), SimpleSpan::new(10, 11));
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns a reference to the span of the opening delimiter.
  #[inline(always)]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a mutable reference to the span of the opening delimiter.
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Returns a reference to the name of the unclosed delimiter.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// let error = Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into());
  /// assert_eq!(error.name_ref(), "{");
  /// ```
  #[inline(always)]
  pub const fn name_ref(&self) -> &str {
    self.name.as_str()
  }

  /// Returns the unclosed delimiter.
  ///
  /// This method is only available when the delimiter type implements `Copy`.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unclosed, utils::CowStr, SimpleSpan};
  ///
  /// let error = Unclosed::<char>::new(SimpleSpan::new(5, 6), "[".into());
  /// assert_eq!(error.name(), CowStr::from_static("["));
  /// ```
  #[inline(always)]
  #[cfg(not(any(feature = "alloc", feature = "std")))]
  pub const fn name(&self) -> CowStr {
    self.name
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
  /// use tokora::{error::Unclosed, SimpleSpan};
  ///
  /// let mut error = Unclosed::<char>::new(SimpleSpan::new(5, 6), "(".into());
  /// error.bump(&100);
  /// assert_eq!(error.span(), SimpleSpan::new(105, 106));
  /// ```
  #[inline(always)]
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
  /// use tokora::{error::Unclosed, utils::CowStr, SimpleSpan};
  ///
  /// let error = Unclosed::<char>::new(SimpleSpan::new(10, 11), "\"".into());
  /// let (span, delimiter) = error.into_components();
  /// assert_eq!(span, SimpleSpan::new(10, 11));
  /// assert_eq!(delimiter, CowStr::from_static("\""));
  /// ```
  #[inline(always)]
  pub fn into_components(self) -> (S, CowStr) {
    (self.span, self.name)
  }
}

impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for ()
where
  L: Lexer<'inp>,
{
  #[inline(always)]
  fn from_unclosed<Delimiter>(_: Unclosed<Delimiter, L::Span, Lang>) -> Self {}
}
