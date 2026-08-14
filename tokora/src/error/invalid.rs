//! Invalid value error type for tracking semantically incorrect values.
//!
//! This module provides the [`Invalid`] type for representing errors where a value
//! is syntactically correct but semantically invalid in the given context.
//!
//! # Design Philosophy
//!
//! "Invalid" indicates that a value is **not valid** in the current context, even though
//! its syntax is correct. The form/structure is fine, but the value itself doesn't make
//! sense. This is different from [`Malformed`] which indicates incorrect syntax.
//!
//! # Malformed vs Invalid
//!
//! - **`Malformed`**: Syntax/structure is wrong (badly formed)
//!   - Examples: `123abc` (mixed digits and letters), `0x` (missing hex digits),
//!     `"unterminated string`, `12.34.56` (malformed float)
//!   - The **form** is incorrect
//!   - Parser recognized what it *tried* to be, but it's syntactically broken
//!
//! - **`Invalid`**: Value/semantics is wrong (not valid)
//!   - Examples: `999` for a month value, `0777` in strict octal mode,
//!     `256` for a byte value, `-1` for an unsigned type
//!   - The **value** is incorrect
//!   - Syntax is correct, but the value doesn't make sense in context
//!
//! # Type Parameter
//!
//! - `Knowledge`: Provides context about what kind of value was invalid (typically
//!   a knowledge marker type like `IntLiteral`, `StringLiteral`, etc.)
//!
//! # Type Aliases
//!
//! This module provides convenient type aliases for common literal types:
//!
//! - [`InvalidStringLiteral`] - String values that are semantically invalid
//! - [`InvalidIntLiteral`] - Integer values out of range or context-inappropriate
//! - [`InvalidFloatLiteral`] - Float values that are semantically invalid
//! - [`InvalidHexLiteral`] - Hexadecimal values invalid in context
//! - [`InvalidBinaryLiteral`] - Binary values invalid in context
//! - [`InvalidOctalLiteral`] - Octal values invalid in context
//! - And more...
//!
//! # Examples
//!
//! ## Invalid Range Values
//!
//! ```rust
//! use tokora::{error::Invalid, SimpleSpan, utils::{knowledge::IntLiteral}};
//!
//! // Found "256" when parsing a byte (range 0-255)
//! let error = Invalid::int(SimpleSpan::new(10, 13));
//! assert_eq!(error.to_string(), "invalid at 10..13, did you mean int literal?");
//! ```
//!
//! ## Invalid Context Values
//!
//! ```rust
//! use tokora::{error::Invalid, SimpleSpan, utils::{knowledge::OctalLiteral}};
//!
//! // Found "0777" in strict mode where leading zeros aren't allowed
//! let error = Invalid::octal(SimpleSpan::new(5, 9));
//! assert_eq!(error.span(), SimpleSpan::new(5, 9));
//! ```
//!
//! ## Generic Invalid Value
//!
//! ```rust
//! use tokora::{error::Invalid, SimpleSpan};
//!
//! // Invalid with no specific knowledge
//! let error: Invalid<()> = Invalid::new(SimpleSpan::new(20, 25));
//! assert_eq!(error.to_string(), "invalid at 20..25");
//! ```
//!
//! ## With Custom Knowledge
//!
//! ```rust
//! use tokora::{error::Invalid, SimpleSpan, utils::{knowledge::HexLiteral}};
//!
//! // Found valid hex syntax but value out of range
//! let error = Invalid::with_knowledge(SimpleSpan::new(15, 20), HexLiteral::default());
//! assert_eq!(error.knowledge().is_some(), true);
//! ```

use crate::{
  span::{SimpleSpan, Span},
  utils::{human_display::DisplayHuman, knowledge::*},
};

/// An invalid string literal value.
///
/// Used when a string is syntactically correct but semantically invalid in context.
pub type InvalidStringLiteral = Invalid<StringLiteral>;

/// An invalid boolean literal value.
///
/// Used when a boolean value is correctly parsed but invalid in the given context.
pub type InvalidBooleanLiteral = Invalid<BooleanLiteral>;

/// An invalid null literal value.
///
/// Used when a null value appears in a context where it's not allowed.
pub type InvalidNullLiteral = Invalid<NullLiteral>;

/// An invalid enum literal value.
///
/// Used when an enum type is correctly formed but invalid in context.
pub type InvalidEnumLiteral = Invalid<EnumLiteral>;

/// An invalid enum value literal.
///
/// Used when an enum value is correctly formed but doesn't match any variant.
pub type InvalidEnumValueLiteral = Invalid<EnumValueLiteral>;

/// An invalid octal literal value.
///
/// Used when an octal number is syntactically correct but invalid (e.g., out of range).
pub type InvalidOctalLiteral = Invalid<OctalLiteral>;

/// An invalid decimal literal value.
///
/// Used when a decimal number is correctly formed but out of range or invalid in context.
pub type InvalidDecimalLiteral = Invalid<DecimalLiteral>;

/// An invalid hexadecimal literal value.
///
/// Used when a hex number is correctly formed but out of range or invalid in context.
pub type InvalidHexLiteral = Invalid<HexLiteral>;

/// An invalid integer literal value.
///
/// Used when an integer is correctly formed but out of range (e.g., `256` for a u8).
pub type InvalidIntLiteral = Invalid<IntLiteral>;

/// An invalid binary literal value.
///
/// Used when a binary number is correctly formed but invalid in context.
pub type InvalidBinaryLiteral = Invalid<BinaryLiteral>;

/// An invalid floating-point literal value.
///
/// Used when a float is correctly formed but invalid (e.g., `Infinity`, `NaN` in strict mode).
pub type InvalidFloatLiteral = Invalid<FloatLiteral>;

/// An invalid hexadecimal floating-point literal value.
///
/// Used when a hex float is correctly formed but invalid in context.
pub type InvalidHexFloatLiteral = Invalid<HexFloatLiteral>;

/// A zero-copy error type representing an invalid value.
///
/// This type tracks the position of a semantically invalid value, along with
/// optional knowledge about what kind of value was expected.
///
/// # Type Parameter
///
/// - `Knowledge`: Provides context about what was being validated (typically a knowledge
///   marker type like `IntLiteral`, `StringLiteral`, etc.). Must implement `DisplayHuman`
///   for error messages.
///
/// # Common Use Cases
///
/// - **Out of range values**: `256` for u8, `-1` for unsigned types, `999` for months
/// - **Context-inappropriate values**: `0777` in strict mode, `null` where not allowed
/// - **Semantically invalid**: Values that parse correctly but violate domain constraints
///
/// # Design
///
/// The `knowledge` field is optional, allowing this type to be used both with and
/// without specific context about what was being validated. When knowledge is present,
/// error messages can include "did you mean X?" suggestions.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::{error::Invalid, SimpleSpan};
///
/// // Invalid value at position 10-13
/// let error: Invalid<()> = Invalid::new(SimpleSpan::new(10, 13));
/// assert_eq!(error.span(), SimpleSpan::new(10, 13));
/// assert_eq!(error.to_string(), "invalid at 10..13");
/// ```
///
/// ## With Knowledge Context
///
/// ```rust
/// use tokora::{error::Invalid, SimpleSpan, utils::{knowledge::IntLiteral}};
///
/// // Found "256" when expecting u8 (0-255)
/// let error = Invalid::int(SimpleSpan::new(5, 8));
/// assert_eq!(error.to_string(), "invalid at 5..8, did you mean int literal?");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Invalid<Knowledge, S = SimpleSpan> {
  span: S,
  knowledge: Option<Knowledge>,
}

impl<Knowledge, S> core::fmt::Display for Invalid<Knowledge, S>
where
  Knowledge: DisplayHuman,
  S: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.knowledge {
      Some(knowledge) => write!(
        f,
        "invalid at {}, did you mean {}?",
        self.span,
        knowledge.display()
      ),
      None => write!(f, "invalid at {}", self.span),
    }
  }
}

impl<Knowledge, S> core::error::Error for Invalid<Knowledge, S>
where
  Knowledge: DisplayHuman + core::fmt::Debug,
  S: core::fmt::Debug + core::fmt::Display,
{
}

impl<S> Invalid<BooleanLiteral, S> {
  /// Create a new Invalid knowledge for a boolean literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::BooleanLiteral};
  ///
  /// let error = Invalid::boolean(SimpleSpan::new(10, 14));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 14));
  /// assert_eq!(error.knowledge(), Some(&BooleanLiteral::default()));
  /// ```
  #[inline]
  pub const fn boolean(span: S) -> Self {
    Self::with_knowledge(span, BooleanLiteral(()))
  }
}

impl<S> Invalid<NullLiteral, S> {
  /// Create a new Invalid knowledge for a null literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::NullLiteral};
  ///
  /// let error = Invalid::null(SimpleSpan::new(20, 24));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 24));
  /// assert_eq!(error.knowledge(), Some(&NullLiteral::default()));
  /// ```
  #[inline]
  pub const fn null(span: S) -> Self {
    Self::with_knowledge(span, NullLiteral(()))
  }
}

impl<S> Invalid<EnumLiteral, S> {
  /// Create a new Invalid knowledge for an enum literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::EnumLiteral};
  ///
  /// let error = Invalid::enumeration(SimpleSpan::new(30, 40));
  /// assert_eq!(error.span(), SimpleSpan::new(30, 40));
  /// assert_eq!(error.knowledge(), Some(&EnumLiteral::default()));
  /// ```
  #[inline]
  pub const fn enumeration(span: S) -> Self {
    Self::with_knowledge(span, EnumLiteral(()))
  }
}

impl<S> Invalid<EnumValueLiteral, S> {
  /// Create a new Invalid knowledge for an enum value literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::EnumValueLiteral};
  ///
  /// let error = Invalid::enum_value(SimpleSpan::new(45, 49));
  /// assert_eq!(error.span(), SimpleSpan::new(45, 49));
  /// assert_eq!(error.knowledge(), Some(&EnumValueLiteral::default()));
  /// ```
  #[inline]
  pub const fn enum_value(span: S) -> Self {
    Self::with_knowledge(span, EnumValueLiteral(()))
  }
}

impl<S> Invalid<DecimalLiteral, S> {
  /// Create a new Invalid knowledge for a decimal literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::DecimalLiteral};
  ///
  /// let error = Invalid::decimal(SimpleSpan::new(150, 160));
  /// assert_eq!(error.span(), SimpleSpan::new(150, 160));
  /// assert_eq!(error.knowledge(), Some(&DecimalLiteral::default()));
  /// ```
  #[inline]
  pub const fn decimal(span: S) -> Self {
    Self::with_knowledge(span, DecimalLiteral(()))
  }
}

impl<S> Invalid<OctalLiteral, S> {
  /// Create a new Invalid knowledge for an octal literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::OctalLiteral};
  ///
  /// let error = Invalid::octal(SimpleSpan::new(50, 60));
  /// assert_eq!(error.span(), SimpleSpan::new(50, 60));
  /// assert_eq!(error.knowledge(), Some(&OctalLiteral::default()));
  /// ```
  #[inline]
  pub const fn octal(span: S) -> Self {
    Self::with_knowledge(span, OctalLiteral(()))
  }
}

impl<S> Invalid<StringLiteral, S> {
  /// Create a new Invalid knowledge for a string literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::StringLiteral};
  ///
  /// let error = Invalid::string(SimpleSpan::new(70, 80));
  /// assert_eq!(error.span(), SimpleSpan::new(70, 80));
  /// assert_eq!(error.knowledge(), Some(&StringLiteral::default()));
  /// ```
  #[inline]
  pub const fn string(span: S) -> Self {
    Self::with_knowledge(span, StringLiteral(()))
  }
}

impl<S> Invalid<HexLiteral, S> {
  /// Create a new Invalid knowledge for a hex literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::HexLiteral};
  ///
  /// let error = Invalid::hex(SimpleSpan::new(90, 100));
  /// assert_eq!(error.span(), SimpleSpan::new(90, 100));
  /// assert_eq!(error.knowledge(), Some(&HexLiteral::default()));
  /// ```
  #[inline]
  pub const fn hex(span: S) -> Self {
    Self::with_knowledge(span, HexLiteral(()))
  }
}

impl<S> Invalid<IntLiteral, S> {
  /// Create a new Invalid knowledge for an int literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::IntLiteral};
  ///
  /// let error = Invalid::int(SimpleSpan::new(105, 110));
  /// assert_eq!(error.span(), SimpleSpan::new(105, 110));
  /// assert_eq!(error.knowledge(), Some(&IntLiteral::default()));
  /// ```
  #[inline]
  pub const fn int(span: S) -> Self {
    Self::with_knowledge(span, IntLiteral(()))
  }
}

impl<S> Invalid<BinaryLiteral, S> {
  /// Create a new Invalid knowledge for a binary literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::BinaryLiteral};
  ///
  /// let error = Invalid::binary(SimpleSpan::new(115, 120));
  /// assert_eq!(error.span(), SimpleSpan::new(115, 120));
  /// assert_eq!(error.knowledge(), Some(&BinaryLiteral::default()));
  /// ```
  #[inline]
  pub const fn binary(span: S) -> Self {
    Self::with_knowledge(span, BinaryLiteral(()))
  }
}

impl<S> Invalid<FloatLiteral, S> {
  /// Create a new Invalid knowledge for a float literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::FloatLiteral};
  ///
  /// let error = Invalid::float(SimpleSpan::new(125, 130));
  /// assert_eq!(error.span(), SimpleSpan::new(125, 130));
  /// assert_eq!(error.knowledge(), Some(&FloatLiteral::default()));
  /// ```
  #[inline]
  pub const fn float(span: S) -> Self {
    Self::with_knowledge(span, FloatLiteral(()))
  }
}

impl<S> Invalid<HexFloatLiteral, S> {
  /// Create a new Invalid knowledge for a hex float literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::knowledge::HexFloatLiteral};
  ///
  /// let error = Invalid::hex_float(SimpleSpan::new(135, 140));
  /// assert_eq!(error.span(), SimpleSpan::new(135, 140));
  /// assert_eq!(error.knowledge(), Some(&HexFloatLiteral::default()));
  /// ```
  #[inline]
  pub const fn hex_float(span: S) -> Self {
    Self::with_knowledge(span, HexFloatLiteral(()))
  }
}

impl<Knowledge, S> Invalid<Knowledge, S> {
  /// Creates a new invalid value error without specific knowledge context.
  ///
  /// Use this constructor when you know a value is invalid but don't have (or need)
  /// specific context about what kind of value it was supposed to be.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan};
  ///
  /// // Found an invalid value at position 10-13, no specific context
  /// let error: Invalid<()> = Invalid::new(SimpleSpan::new(10, 13));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 13));
  /// assert_eq!(error.knowledge(), None);
  /// assert_eq!(error.to_string(), "invalid at 10..13");
  /// ```
  #[inline]
  pub const fn new(span: S) -> Self {
    Self {
      span,
      knowledge: None,
    }
  }

  /// Creates a new invalid value error with specific knowledge context.
  ///
  /// Use this constructor when you have specific context about what kind of value
  /// was expected, which enables more helpful error messages with "did you mean X?"
  /// suggestions.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::{knowledge::IntLiteral}};
  ///
  /// // Found "256" when parsing u8 (valid range 0-255)
  /// let error = Invalid::with_knowledge(SimpleSpan::new(5, 8), IntLiteral::default());
  /// assert_eq!(error.span(), SimpleSpan::new(5, 8));
  /// assert_eq!(error.knowledge(), Some(&IntLiteral::default()));
  /// assert_eq!(error.to_string(), "invalid at 5..8, did you mean int literal?");
  /// ```
  #[inline]
  pub const fn with_knowledge(span: S, knowledge: Knowledge) -> Self {
    Self {
      span,
      knowledge: Some(knowledge),
    }
  }

  /// Returns the span of the invalid value.
  ///
  /// This indicates where in the input the invalid value was found.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan};
  ///
  /// let error: Invalid<()> = Invalid::new(SimpleSpan::new(10, 15));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 15));
  /// ```
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns a reference to the span of the invalid value.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan};
  ///
  /// let error: Invalid<()> = Invalid::new(SimpleSpan::new(10, 15));
  /// assert_eq!(error.span_ref(), &SimpleSpan::new(10, 15));
  /// ```
  #[inline]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a mutable reference to the span of the invalid value.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan};
  ///
  /// let mut error: Invalid<()> = Invalid::new(SimpleSpan::new(10, 15));
  /// *error.span_mut() = SimpleSpan::new(20, 25);
  /// assert_eq!(error.span(), SimpleSpan::new(20, 25));
  /// ```
  #[inline]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Returns the knowledge context for this invalid value, if any.
  ///
  /// The knowledge provides context about what kind of value was expected,
  /// which is used to generate helpful error messages.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::{knowledge::IntLiteral}};
  ///
  /// let error = Invalid::int(SimpleSpan::new(5, 8));
  /// assert_eq!(error.knowledge(), Some(&IntLiteral::default()));
  ///
  /// let error_no_context: Invalid<()> = Invalid::new(SimpleSpan::new(10, 15));
  /// assert_eq!(error_no_context.knowledge(), None);
  /// ```
  #[inline]
  pub const fn knowledge(&self) -> Option<&Knowledge> {
    self.knowledge.as_ref()
  }

  /// Consumes the error and returns its components.
  ///
  /// Returns a tuple of `(span, knowledge)` where knowledge may be `None` if
  /// the error was created without specific context.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan, utils::{knowledge::IntLiteral}};
  ///
  /// let error = Invalid::int(SimpleSpan::new(10, 15));
  /// let (span, knowledge) = error.into_components();
  /// assert_eq!(span, SimpleSpan::new(10, 15));
  /// assert_eq!(knowledge, Some(IntLiteral::default()));
  /// ```
  #[inline]
  pub fn into_components(self) -> (S, Option<Knowledge>) {
    (self.span, self.knowledge)
  }

  /// Bumps both the start and end positions of the span by the given offset.
  ///
  /// This adjusts the error's position, which is useful when adjusting error
  /// positions after processing or when combining errors from different parsing
  /// contexts. Returns `&mut self` to allow method chaining.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan};
  ///
  /// let mut error: Invalid<()> = Invalid::new(SimpleSpan::new(5, 10));
  /// error.bump(&100);
  /// assert_eq!(error.span(), SimpleSpan::new(105, 110));
  /// ```
  ///
  /// ## Method Chaining
  ///
  /// ```rust
  /// use tokora::{error::Invalid, SimpleSpan};
  ///
  /// let mut error: Invalid<()> = Invalid::new(SimpleSpan::new(5, 10));
  /// error.bump(&100).bump(&50);
  /// assert_eq!(error.span(), SimpleSpan::new(155, 160));
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
