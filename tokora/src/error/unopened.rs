//! Unopened delimiter error type for tracking closing delimiters without opening delimiters.
//!
//! This module provides the [`Unopened`] type for representing errors where a closing
//! delimiter was found but never opened (i.e., the corresponding opening delimiter is missing).
//!
//! # Design Philosophy
//!
//! When parsing structured text with paired delimiters (parentheses, brackets, braces,
//! quotes, etc.), it's common to encounter situations where a closing delimiter is found
//! but the corresponding opening delimiter is missing. This error type captures both:
//!
//! - **Where** the closing delimiter was found (via [`SimpleSpan`])
//! - **What** closing delimiter name was left unopened (stored as a [`CowStr`])
//!
//! # Unopened vs Unclosed vs Unterminated
//!
//! - **`Unopened`**: For closing delimiters found **without** a matching opening delimiter
//!   - Examples: `a + b)`, `foo]`, `bar}`
//!   - The span points to the **closing delimiter** position
//!   - Used when you find a closing delimiter that was never opened
//!
//! - **`Unclosed`**: For opening delimiters found **without** a matching closing delimiter
//!   - Examples: `(a + b`, `[foo`, `{bar`
//!   - The span points to the **opening delimiter** position
//!   - Used when you find an opening delimiter that was never closed
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
//! use tokora::{error::Unopened, SimpleSpan};
//!
//! // Closing parenthesis at position 10, never opened
//! // Example: "a + b * c)" where the ')' has no matching '('
//! let error = Unopened::<char>::new(SimpleSpan::new(10, 11), ")".into());
//!
//! assert_eq!(error.span(), SimpleSpan::new(10, 11));
//! assert_eq!(error.name_ref(), ")");
//! assert_eq!(error.to_string(), "unopened delimiter ')'");
//! ```
//!
//! ## Custom Delimiter Enum
//!
//! ```rust
//! use tokora::{error::Unopened, SimpleSpan};
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
//! let error: Unopened<Delimiter> = Unopened::new(SimpleSpan::new(5, 6), "/*".into());
//! assert_eq!(error.name_ref(), "/*");
//! ```
//!
//! ## Tracking Nested Delimiters
//!
//! ```rust
//! use tokora::{error::Unopened, SimpleSpan};
//!
//! // When parsing: "{ foo: bar, baz ] }"
//! // The ']' at position 16 was never opened
//! let error = Unopened::<char>::new(SimpleSpan::new(16, 17), "]".into());
//!
//! // Error reporting can show:
//! // "error at 16..17: unopened delimiter ']'"
//! ```
//!
//! ## Position Adjustment
//!
//! ```rust
//! use tokora::{error::Unopened, SimpleSpan};
//!
//! // Error from a nested parsing context
//! let mut error = Unopened::<char>::new(SimpleSpan::new(5, 6), "}".into());
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

/// An unopened bracket error (closing `]` without opening `[`)
pub type UnopenedBracket<S = SimpleSpan, Lang = ()> = Unopened<Bracket<(), (), Lang>, S, Lang>;
/// An unopened parenthesis error (closing `)` without opening `(`)
pub type UnopenedParen<S = SimpleSpan, Lang = ()> = Unopened<Paren<(), (), Lang>, S, Lang>;
/// An unopened brace error (closing `}` without opening `{`)
pub type UnopenedBrace<S = SimpleSpan, Lang = ()> = Unopened<Brace<(), (), Lang>, S, Lang>;
/// An unopened angle bracket error (closing `>` without opening `<`)
pub type UnopenedAngle<S = SimpleSpan, Lang = ()> = Unopened<Angle<(), (), Lang>, S, Lang>;

/// A zero-copy error type representing an unopened delimiter.
///
/// This type tracks the position of a closing delimiter that was never opened,
/// enabling precise error reporting for missing opening delimiters in structured text.
///
/// # Type Parameter
///
/// - `Delimiter`: The type representing the delimiter (typically `char`, `&'static str`,
///   or a custom enum). Must implement `Display` for error messages.
///
/// # Common Use Cases
///
/// - **Unmatched parentheses** in expressions: `a + b * c)`
/// - **Unopened strings** in source code: `hello world"`
/// - **Missing opening braces** in JSON: `"key": "value"}`
/// - **Incomplete block comments**: `/* This comment never ends`
/// - **Unmatched brackets** in arrays: `1, 2, 3]`
///
/// # Design
///
/// The span points to the **closing delimiter** position. This allows error messages
/// to point users to the exact location of the unexpected closing delimiter, making
/// it easier to find and fix the issue.
///
/// # Examples
///
/// ## Detecting Unopened Parentheses
///
/// ```rust
/// use tokora::{error::Unopened, SimpleSpan};
///
/// // Parse error: 1 + 2)
/// //                   ^--- unopened closing delimiter
/// let error = Unopened::<char>::new(SimpleSpan::new(5, 6), ")".into());
///
/// println!("Error: {} at position {}", error, error.span().start());
/// // Output: "Error: unopened delimiter ')' at position 5"
/// ```
///
/// ## Tracking Multiple Unopened Delimiters
///
/// ```rust
/// use tokora::{error::Unopened, SimpleSpan};
///
/// let errors = vec![
///     Unopened::<char>::new(SimpleSpan::new(5, 6), "}".into()),
///     Unopened::<char>::new(SimpleSpan::new(10, 11), "]".into()),
///     Unopened::<char>::new(SimpleSpan::new(15, 16), ")".into()),
/// ];
///
/// for error in errors {
///     eprintln!("Unopened {} at {}", error.name_ref(), error.span());
/// }
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Unopened<Delimiter, S = SimpleSpan, Lang: ?Sized = ()> {
  span: S,
  name: CowStr,
  _delimiter: PhantomData<Delimiter>,
  _m: PhantomData<Lang>,
}

impl<Delimiter, S, Lang: ?Sized> core::fmt::Debug for Unopened<Delimiter, S, Lang>
where
  S: core::fmt::Debug,
{
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("Unopened")
      .field("span", &self.span)
      .field("name", &self.name)
      .finish()
  }
}

impl<Delimiter, S, Lang: ?Sized> core::fmt::Display for Unopened<Delimiter, S, Lang> {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "unopened delimiter '{}'", self.name)
  }
}

impl<Delimiter, S, Lang: ?Sized> core::error::Error for Unopened<Delimiter, S, Lang>
where
  S: core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

impl<S> Unopened<Paren, S> {
  /// Creates a new unopened parenthesis error.
  ///
  /// The span should point to the position of the closing parenthesis that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing parenthesis at position 3, never opened
  /// let error = Unopened::paren(SimpleSpan::new(3, 4));
  /// assert_eq!(error.span(), SimpleSpan::new(3, 4));
  /// assert_eq!(error.name_ref(), "()");
  /// ```
  #[inline]
  pub const fn paren(span: S) -> Self {
    Self::paren_of(span)
  }
}

impl<S, Lang: ?Sized> Unopened<Paren<(), (), Lang>, S, Lang> {
  /// Creates a new unopened parenthesis error.
  ///
  /// The span should point to the position of the closing parenthesis that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing parenthesis at position 3, never opened
  /// let error: Unopened<_, SimpleSpan, ()> = Unopened::paren_of(SimpleSpan::new(3, 4));
  /// assert_eq!(error.span(), SimpleSpan::new(3, 4));
  /// assert_eq!(error.name_ref(), "()");
  /// ```
  #[inline]
  pub const fn paren_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("()"))
  }
}

impl<S> Unopened<Bracket, S> {
  /// Creates a new unopened bracket error.
  ///
  /// The span should point to the position of the closing bracket that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing bracket at position 8, never opened
  /// let error = Unopened::bracket(SimpleSpan::new(8, 9));
  /// assert_eq!(error.span(), SimpleSpan::new(8, 9));
  /// assert_eq!(error.name_ref(), "[]");
  /// ```
  #[inline]
  pub const fn bracket(span: S) -> Self {
    Self::bracket_of(span)
  }
}

impl<S, Lang: ?Sized> Unopened<Bracket<(), (), Lang>, S, Lang> {
  /// Creates a new unopened bracket error.
  ///
  /// The span should point to the position of the closing bracket that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing bracket at position 8, never opened
  /// let error: Unopened<_, SimpleSpan, ()> = Unopened::bracket_of(SimpleSpan::new(8, 9));
  /// assert_eq!(error.span(), SimpleSpan::new(8, 9));
  /// assert_eq!(error.name_ref(), "[]");
  /// ```
  #[inline]
  pub const fn bracket_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("[]"))
  }
}

impl<S> Unopened<Brace, S> {
  /// Creates a new unopened brace error.
  ///
  /// The span should point to the position of the closing brace that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing brace at position 12, never opened
  /// let error = Unopened::brace(SimpleSpan::new(12, 13));
  /// assert_eq!(error.span(), SimpleSpan::new(12, 13));
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline]
  pub const fn brace(span: S) -> Self {
    Self::brace_of(span)
  }
}

impl<S, Lang: ?Sized> Unopened<Brace<(), (), Lang>, S, Lang> {
  /// Creates a new unopened brace error.
  ///
  /// The span should point to the position of the closing brace that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing brace at position 12, never opened
  /// let error: Unopened<_, SimpleSpan, ()> = Unopened::brace_of(SimpleSpan::new(12, 13));
  /// assert_eq!(error.span(), SimpleSpan::new(12, 13));
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline]
  pub const fn brace_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("{}"))
  }
}

impl<S> Unopened<Angle, S> {
  /// Creates a new unopened angle bracket error.
  ///
  /// The span should point to the position of the closing angle bracket that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing angle bracket at position 20, never opened
  /// let error = Unopened::angle(SimpleSpan::new(20, 21));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 21));
  /// assert_eq!(error.name_ref(), "<>");
  /// ```
  #[inline]
  pub const fn angle(span: S) -> Self {
    Self::of(span, CowStr::from_static("<>"))
  }
}

impl<S, Lang: ?Sized> Unopened<Angle<(), (), Lang>, S, Lang> {
  /// Creates a new unopened angle bracket error.
  ///
  /// The span should point to the position of the closing angle bracket that was never opened.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing angle bracket at position 20, never opened
  /// let error: Unopened<_, SimpleSpan, ()> = Unopened::angle_of(SimpleSpan::new(20, 21));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 21));
  /// assert_eq!(error.name_ref(), "<>");
  /// ```
  #[inline]
  pub const fn angle_of(span: S) -> Self {
    Self::of(span, CowStr::from_static("<>"))
  }
}

impl<Delimiter, S> Unopened<Delimiter, S> {
  /// Creates a new unopened delimiter error.
  ///
  /// The span should point to the position of the closing delimiter that was never opened.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing brace at position 5, never opened
  /// let error = Unopened::<char>::of(SimpleSpan::new(5, 6), "}".into());
  /// assert_eq!(error.span(), SimpleSpan::new(5, 6));
  /// assert_eq!(error.name_ref(), "}");
  /// ```
  #[inline]
  pub const fn new(span: S, name: CowStr) -> Self {
    Self::of(span, name)
  }
}

impl<Delimiter, S, Lang: ?Sized> Unopened<Delimiter, S, Lang> {
  /// Creates a new unopened delimiter error.
  ///
  /// The span should point to the position of the closing delimiter that was never opened.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// // Closing brace at position 5, never opened
  /// let error = Unopened::<char>::new(SimpleSpan::new(5, 6), "}".into());
  /// assert_eq!(error.span(), SimpleSpan::new(5, 6));
  /// assert_eq!(error.name_ref(), "}");
  /// ```
  #[inline]
  pub const fn of(span: S, name: CowStr) -> Self {
    Self {
      span,
      name,
      _delimiter: PhantomData,
      _m: PhantomData,
    }
  }

  /// Returns the span of the closing delimiter.
  ///
  /// This is the position where the closing delimiter was found without a matching opening.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// let error = Unopened::<char>::new(SimpleSpan::new(10, 11), ")".into());
  /// assert_eq!(error.span(), SimpleSpan::new(10, 11));
  /// ```
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns a reference to the span of the opening delimiter.
  #[inline]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a mutable reference to the span of the opening delimiter.
  #[inline]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Returns a reference to the unopened delimiter.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// let error = Unopened::<char>::new(SimpleSpan::new(5, 6), "{}".into());
  /// assert_eq!(error.name_ref(), "{}");
  /// ```
  #[inline]
  pub const fn name_ref(&self) -> &str {
    self.name.as_str()
  }

  /// Returns the unopened delimiter.
  ///
  /// This method is only available when the delimiter type implements `Copy`.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// let error = Unopened::<char>::new(SimpleSpan::new(5, 6), "]".into());
  /// assert_eq!(error.name_ref(), "]");
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
  /// use tokora::{error::Unopened, SimpleSpan};
  ///
  /// let mut error = Unopened::<char>::new(SimpleSpan::new(5, 6), ")".into());
  /// error.bump(&100);
  /// assert_eq!(error.span(), SimpleSpan::new(105, 106));
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
  /// use tokora::{error::Unopened, utils::CowStr, SimpleSpan};
  ///
  /// let error = Unopened::<char>::new(SimpleSpan::new(10, 11), "\"".into());
  /// let (span, delimiter) = error.into_components();
  /// assert_eq!(span, SimpleSpan::new(10, 11));
  /// assert_eq!(delimiter, CowStr::from_static("\""));
  /// ```
  #[inline]
  pub fn into_components(self) -> (S, CowStr) {
    (self.span, self.name)
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unopened<Delimiter, S, Lang>> for () {
  #[inline]
  fn from(_: Unopened<Delimiter, S, Lang>) -> Self {}
}
