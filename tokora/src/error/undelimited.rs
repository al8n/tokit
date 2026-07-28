//! Undelimited error type for tracking missing delimiter pairs.
//!
//! This module provides the [`Undelimited`] type for representing errors where content
//! was expected to be delimited but **both** opening and closing delimiters are missing.
//!
//! # Design Philosophy
//!
//! When parsing structured text with paired delimiters (parentheses, brackets, braces,
//! quotes, etc.), it's common to encounter situations where content is expected to be
//! delimited but neither the opening nor closing delimiter is present. This error type captures:
//!
//! - **Where** the undelimited content was found (via [`SimpleSpan`])
//! - **What** delimiter name was expected (stored as a [`CowStr`])
//!
//! # Undelimited vs Unclosed vs Unopened
//!
//! - **`Undelimited`**: For content missing **both** opening and closing delimiters
//!   - Examples: Expected `"hello"` but got just `hello`, expected `{...}` but got just `...`
//!   - The span points to the **undelimited content** position
//!   - Used when you expect delimiters around content but find neither
//!
//! - **`Unclosed`**: For opening delimiters found **without** a matching closing delimiter
//!   - Examples: `(a + b`, `[foo`, `{bar`
//!   - The span points to the **opening delimiter** position
//!   - Used when you find an opening delimiter that was never closed
//!
//! - **`Unopened`**: For closing delimiters found **without** a matching opening delimiter
//!   - Examples: `a + b)`, `foo]`, `bar}`
//!   - The span points to the **closing delimiter** position
//!   - Used when you find a closing delimiter that was never opened
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
//! use tokora::{error::Undelimited, SimpleSpan};
//!
//! // Expected string literal with quotes, but found undelimited content
//! // Example: hello instead of "hello"
//! let error = Undelimited::<char>::new(SimpleSpan::new(10, 15), "\"".into());
//!
//! assert_eq!(error.span(), SimpleSpan::new(10, 15));
//! assert_eq!(error.name_ref(), "\"");
//! assert_eq!(error.to_string(), "undelimited content, expected '\"'");
//! ```
//!
//! ## Custom Delimiter Enum
//!
//! ```rust
//! use tokora::{error::Undelimited, SimpleSpan};
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
//!
//! let error: Undelimited<Delimiter> = Undelimited::new(SimpleSpan::new(5, 10), "/*".into());
//! assert_eq!(error.name_ref(), "/*");
//! ```
//!
//! ## Undelimited Content in Parsing
//!
//! ```rust
//! use tokora::{error::Undelimited, SimpleSpan};
//!
//! // When parsing a configuration file expecting bracketed arrays
//! // but finding: items = foo, bar, baz (missing the brackets)
//! let error = Undelimited::<char>::new(SimpleSpan::new(9, 20), "[".into());
//!
//! // Error reporting can show:
//! // "error at 9..20: undelimited content, expected '['"
//! ```
//!
//! ## Position Adjustment
//!
//! ```rust
//! use tokora::{error::Undelimited, SimpleSpan};
//!
//! // Error from a nested parsing context
//! let mut error = Undelimited::<char>::new(SimpleSpan::new(5, 6), "}".into());
//!
//! // Adjust to absolute position in the larger document
//! error.bump(&100);
//! assert_eq!(error.span(), SimpleSpan::new(105, 106));
//! ```

use crate::{
  punct::{Angle, Brace, Bracket, Paren},
  span::{SimpleSpan, Span},
  utils::CowStr,
};
use core::marker::PhantomData;

/// Content missing both opening `[` and closing `]`
pub type UndelimitedBracket<S = SimpleSpan, Lang = ()> =
  Undelimited<Bracket<(), (), Lang>, S, Lang>;
/// Content missing both opening `(` and closing `)`
pub type UndelimitedParen<S = SimpleSpan, Lang = ()> = Undelimited<Paren<(), (), Lang>, S, Lang>;
/// Content missing both opening `{` and closing `}`
pub type UndelimitedBrace<S = SimpleSpan, Lang = ()> = Undelimited<Brace<(), (), Lang>, S, Lang>;
/// Content missing both opening `<` and closing `>`
pub type UndelimitedAngle<S = SimpleSpan, Lang = ()> = Undelimited<Angle<(), (), Lang>, S, Lang>;

/// A zero-copy error type representing undelimited content.
///
/// This type tracks the position of content that should have been delimited by a pair
/// of delimiters (opening and closing) but neither delimiter was present.
///
/// # Type Parameter
///
/// - `Delimiter`: The type representing the delimiter (typically `char`, `&'static str`,
///   or a custom enum). Must implement `Display` for error messages.
///
/// # Common Use Cases
///
/// - **Unquoted strings**: Expected `"hello"` but found just `hello`
/// - **Missing array brackets**: Expected `[1, 2, 3]` but found just `1, 2, 3`
/// - **Missing braces**: Expected `{key: value}` but found just `key: value`
/// - **Undelimited expressions**: Expected `(a + b)` but found just `a + b`
/// - **Missing block delimiters**: Expected `/* comment */` but found just `comment`
///
/// # Design
///
/// The span points to the **undelimited content** position, indicating where delimiters
/// were expected but not found. This allows error messages to highlight the content
/// that should have been wrapped in delimiters.
///
/// # Examples
///
/// ## Detecting Undelimited Content
///
/// ```rust
/// use tokora::{error::Undelimited, SimpleSpan};
///
/// // Parse error: expected string literal "hello" but found hello
/// //              ^^^^^--- undelimited content
/// let error = Undelimited::<char>::new(SimpleSpan::new(0, 5), "\"".into());
///
/// println!("Error: {} at position {}", error, error.span().start());
/// // Output: "Error: undelimited content, expected '\"' at position 0"
/// ```
///
/// ## Tracking Multiple Undelimited Regions
///
/// ```rust
/// use tokora::{error::Undelimited, SimpleSpan};
///
/// let errors = vec![
///     Undelimited::<char>::new(SimpleSpan::new(5, 10), "{".into()),   // Missing braces
///     Undelimited::<char>::new(SimpleSpan::new(15, 20), "[".into()),  // Missing brackets
///     Undelimited::<char>::new(SimpleSpan::new(25, 30), "\"".into()), // Missing quotes
/// ];
///
/// for error in errors {
///     eprintln!("Undelimited content at {}, expected {}", error.span(), error.name_ref());
/// }
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Undelimited<Delimiter, S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  name: CowStr,
  _delimiter: PhantomData<Delimiter>,
  _lang: PhantomData<Lang>,
}

impl<Delimiter, S, Lang: ?Sized> core::fmt::Debug for Undelimited<Delimiter, S, Lang>
where
  S: core::fmt::Debug,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("Undelimited")
      .field("span", &self.span)
      .field("name", &self.name)
      .finish()
  }
}

impl<Delimiter, S, Lang: ?Sized> core::fmt::Display for Undelimited<Delimiter, S, Lang> {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "undelimited content, expected '{}'", self.name)
  }
}

impl<Delimiter, S, Lang: ?Sized> core::error::Error for Undelimited<Delimiter, S, Lang>
where
  S: core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

impl<S> Undelimited<Paren, S> {
  /// Creates a new undelimited content error for missing parentheses.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in parentheses but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, utils::CowStr, SimpleSpan};
  ///
  /// // Undelimited content from position 3 to 7
  /// let error = Undelimited::paren(SimpleSpan::new(3, 7));
  /// assert_eq!(error.span(), SimpleSpan::new(3, 7));
  /// assert_eq!(error.name_ref(), "()");
  /// ```
  #[inline(always)]
  pub const fn paren(span: S) -> Self {
    Self::paren_of(span)
  }
}

impl<S, Lang: ?Sized> Undelimited<Paren<(), (), Lang>, S, Lang> {
  /// Creates a new undelimited content error for missing parentheses.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in parentheses but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, utils::CowStr, SimpleSpan};
  ///
  /// // Undelimited content from position 3 to 7
  /// let error: Undelimited<_, SimpleSpan, ()> = Undelimited::paren_of(SimpleSpan::new(3, 7));
  /// assert_eq!(error.span(), SimpleSpan::new(3, 7));
  /// assert_eq!(error.name_ref(), "()");
  /// ```
  #[inline(always)]
  pub const fn paren_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("()"))
  }
}

impl<S> Undelimited<Bracket, S> {
  /// Creates a new undelimited content error for missing brackets.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in brackets but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, utils::CowStr, SimpleSpan};
  ///
  /// // Undelimited content from position 8 to 15
  /// let error = Undelimited::bracket(SimpleSpan::new(8, 15));
  /// assert_eq!(error.span(), SimpleSpan::new(8, 15));
  /// assert_eq!(error.name_ref(), "[]");
  /// ```
  #[inline(always)]
  pub const fn bracket(span: S) -> Self {
    Self::bracket_of(span)
  }
}

impl<S, Lang: ?Sized> Undelimited<Bracket<(), (), Lang>, S, Lang> {
  /// Creates a new undelimited content error for missing brackets.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in brackets but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, utils::CowStr, SimpleSpan};
  ///
  /// // Undelimited content from position 8 to 15
  /// let error: Undelimited<_, SimpleSpan, ()> = Undelimited::bracket_of(SimpleSpan::new(8, 15));
  /// assert_eq!(error.span(), SimpleSpan::new(8, 15));
  /// assert_eq!(error.name_ref(), "[]");
  /// ```
  #[inline(always)]
  pub const fn bracket_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("[]"))
  }
}

impl<S> Undelimited<Brace, S> {
  /// Creates a new undelimited content error for missing braces.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in braces but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// // Undelimited content from position 12 to 20
  /// let error = Undelimited::brace(SimpleSpan::new(12, 20));
  /// assert_eq!(error.span(), SimpleSpan::new(12, 20));
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline(always)]
  pub const fn brace(span: S) -> Self {
    Self::brace_of(span)
  }
}

impl<S, Lang: ?Sized> Undelimited<Brace<(), (), Lang>, S, Lang> {
  /// Creates a new undelimited content error for missing braces.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in braces but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// // Undelimited content from position 12 to 20
  /// let error: Undelimited<_, SimpleSpan, ()> = Undelimited::brace_of(SimpleSpan::new(12, 20));
  /// assert_eq!(error.span(), SimpleSpan::new(12, 20));
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline(always)]
  pub const fn brace_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("{}"))
  }
}

impl<S> Undelimited<Angle, S> {
  /// Creates a new undelimited content error for missing angle brackets.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in angle brackets but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// // Undelimited content from position 20 to 25
  /// let error = Undelimited::angle(SimpleSpan::new(20, 25));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 25));
  /// assert_eq!(error.name_ref(), "<>");
  /// ```
  #[inline(always)]
  pub const fn angle(span: S) -> Self {
    Self::angle_of(span)
  }
}

impl<S, Lang: ?Sized> Undelimited<Angle<(), (), Lang>, S, Lang> {
  /// Creates a new undelimited content error for missing angle brackets.
  ///
  /// The span should point to the position of the content that should have been
  /// wrapped in angle brackets but wasn't.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// // Undelimited content from position 20 to 25
  /// let error: Undelimited<_, SimpleSpan, ()> = Undelimited::angle_of(SimpleSpan::new(20, 25));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 25));
  /// assert_eq!(error.name_ref(), "<>");
  /// ```
  #[inline(always)]
  pub const fn angle_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("<>"))
  }
}

impl<Delimiter, S> Undelimited<Delimiter, S> {
  /// Creates a new undelimited content error.
  ///
  /// The span should point to the position of the content that should have been
  /// delimited but wasn't.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, utils::CowStr, SimpleSpan};
  ///
  /// // Undelimited content from position 5 to 10
  /// let error = Undelimited::<char>::of(SimpleSpan::new(5, 10), "{".into());
  /// assert_eq!(error.span(), SimpleSpan::new(5, 10));
  /// assert_eq!(error.name_ref(), "{");
  /// ```
  #[inline(always)]
  pub const fn new(span: S, name: CowStr) -> Self {
    Self::of(span, name)
  }
}

impl<Delimiter, S, Lang: ?Sized> Undelimited<Delimiter, S, Lang> {
  /// Creates a new undelimited content error.
  ///
  /// The span should point to the position of the content that should have been
  /// delimited but wasn't.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// // Undelimited content from position 5 to 10
  /// let error = Undelimited::<char>::new(SimpleSpan::new(5, 10), "{".into());
  /// assert_eq!(error.span(), SimpleSpan::new(5, 10));
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

  /// Returns the span of the undelimited content.
  ///
  /// This is the position of the content that should have been delimited but wasn't.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, utils::CowStr, SimpleSpan};
  ///
  /// let error = Undelimited::<char>::new(SimpleSpan::new(10, 15), "\"".into());
  /// assert_eq!(error.span(), SimpleSpan::new(10, 15));
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

  /// Returns a reference to the expected delimiter.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// let error = Undelimited::<char>::new(SimpleSpan::new(5, 10), "{}".into());
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline(always)]
  pub const fn name_ref(&self) -> &str {
    self.name.as_str()
  }

  /// Returns the expected delimiter.
  ///
  /// This method is only available when the delimiter type implements `Copy`.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// let error = Undelimited::<char>::new(SimpleSpan::new(5, 10), "[]".into());
  /// assert_eq!(error.name(), "[]".into());
  /// ```
  #[inline(always)]
  #[cfg(not(any(feature = "std", feature = "alloc")))]
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
  /// use tokora::{error::Undelimited, SimpleSpan};
  ///
  /// let mut error = Undelimited::<char>::new(SimpleSpan::new(5, 10), "(".into());
  /// error.bump(&100);
  /// assert_eq!(error.span(), SimpleSpan::new(105, 110));
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
  /// use tokora::{error::Undelimited, utils::CowStr, SimpleSpan};
  ///
  /// let error = Undelimited::<char>::new(SimpleSpan::new(10, 15), "\"".into());
  /// let (span, delimiter) = error.into_components();
  /// assert_eq!(span, SimpleSpan::new(10, 15));
  /// assert_eq!(delimiter, CowStr::from_static("\""));
  /// ```
  #[inline(always)]
  pub fn into_components(self) -> (S, CowStr) {
    (self.span, self.name)
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Undelimited<Delimiter, S, Lang>> for () {
  #[inline(always)]
  fn from(_: Undelimited<Delimiter, S, Lang>) -> Self {}
}
