//! Malformed token error type for tracking syntactically incorrect constructs.
//!
//! This module provides the [`Malformed`] type for representing errors where a token,
//! literal, or syntax construct has incorrect structure or formation, even though the
//! parser may have attempted to recognize it.
//!
//! # Design Philosophy
//!
//! "Malformed" indicates that something is **badly formed** or has **incorrect syntax**.
//! The structure itself is wrong, not just the value. This is different from [`Invalid`]
//! which indicates correct syntax but semantically invalid values.
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
//!     `"\xGG"` (correct escape syntax, invalid hex digits)
//!   - The **value** is incorrect
//!   - Syntax is correct, but the value doesn't make sense in context
//!
//! # Type Parameter
//!
//! - `Knowledge`: Provides context about what kind of construct was malformed (typically
//!   a knowledge marker type like `IntLiteral`, `StringLiteral`, etc.)
//!
//! # Type Aliases
//!
//! This module provides convenient type aliases for common literal types:
//!
//! - [`MalformedStringLiteral`] - String literals with incorrect syntax
//! - [`MalformedIntLiteral`] - Integer literals with mixed or invalid characters
//! - [`MalformedFloatLiteral`] - Float literals with incorrect format
//! - [`MalformedHexLiteral`] - Hexadecimal literals with invalid format
//! - [`MalformedBinaryLiteral`] - Binary literals with non-binary characters
//! - [`MalformedOctalLiteral`] - Octal literals with invalid format
//! - And more...
//!
//! # Examples
//!
//! ## Malformed Number Literals
//!
//! ```rust
//! use tokora::{error::Malformed, SimpleSpan, utils::{knowledge::IntLiteral}};
//!
//! // Found "123abc" - mixed digits and letters
//! let error = Malformed::int(SimpleSpan::new(10, 16));
//! assert_eq!(error.to_string(), "malformed at 10..16, did you mean int literal?");
//! ```
//!
//! ## Malformed Float Literals
//!
//! ```rust
//! use tokora::{error::Malformed, SimpleSpan, utils::{knowledge::FloatLiteral}};
//!
//! // Found "12.34.56" - multiple decimal points
//! let error = Malformed::float(SimpleSpan::new(5, 13));
//! assert_eq!(error.span(), SimpleSpan::new(5, 13));
//! ```
//!
//! ## Generic Malformed Token
//!
//! ```rust
//! use tokora::{error::Malformed, SimpleSpan};
//!
//! // Malformed with no specific knowledge
//! let error: Malformed<()> = Malformed::new(SimpleSpan::new(20, 25));
//! assert_eq!(error.to_string(), "malformed at 20..25");
//! ```
//!
//! ## With Custom Knowledge
//!
//! ```rust
//! use tokora::{error::Malformed, SimpleSpan, utils::{knowledge::HexLiteral}};
//!
//! // Found "0x" without any digits
//! let error = Malformed::with_knowledge(SimpleSpan::new(15, 17), HexLiteral::default());
//! assert_eq!(error.knowledge().is_some(), true);
//! ```

use crate::{
  span::{SimpleSpan, Span},
  utils::{human_display::DisplayHuman, knowledge::*},
};

/// A malformed string literal token.
///
/// Used when a string literal has incorrect syntax, such as unterminated strings,
/// invalid escape sequences, or malformed Unicode escapes.
pub type MalformedStringLiteral = Malformed<StringLiteral>;

/// A malformed boolean literal token.
///
/// Used when attempting to parse a boolean value but encountering incorrect syntax
/// (e.g., `tru`, `fals`, `TRUE` in case-sensitive contexts).
pub type MalformedBooleanLiteral = Malformed<BooleanLiteral>;

/// A malformed null literal token.
///
/// Used when attempting to parse a null value but encountering incorrect syntax
/// (e.g., `nul`, `NULL` in case-sensitive contexts).
pub type MalformedNullLiteral = Malformed<NullLiteral>;

/// A malformed enum literal token.
///
/// Used when an enum type declaration has incorrect syntax.
pub type MalformedEnumLiteral = Malformed<EnumLiteral>;

/// A malformed enum value literal token.
///
/// Used when an enum value has incorrect syntax.
pub type MalformedEnumValueLiteral = Malformed<EnumValueLiteral>;

/// A malformed octal literal token.
///
/// Used when an octal number has incorrect syntax (e.g., `0o89`, `0778`).
pub type MalformedOctalLiteral = Malformed<OctalLiteral>;

/// A malformed decimal literal token.
///
/// Used when a decimal number has incorrect syntax (e.g., `123abc`, `45..67`).
pub type MalformedDecimalLiteral = Malformed<DecimalLiteral>;

/// A malformed hexadecimal literal token.
///
/// Used when a hex number has incorrect syntax (e.g., `0x`, `0xGHI`).
pub type MalformedHexLiteral = Malformed<HexLiteral>;

/// A malformed integer literal token.
///
/// Used when an integer has incorrect syntax (e.g., mixed digits and letters).
pub type MalformedIntLiteral = Malformed<IntLiteral>;

/// A malformed binary literal token.
///
/// Used when a binary number has incorrect syntax (e.g., `0b`, `0b102`).
pub type MalformedBinaryLiteral = Malformed<BinaryLiteral>;

/// A malformed floating-point literal token.
///
/// Used when a float has incorrect syntax (e.g., `12.34.56`, `1.e`, `.`).
pub type MalformedFloatLiteral = Malformed<FloatLiteral>;

/// A malformed hexadecimal floating-point literal token.
///
/// Used when a hex float has incorrect syntax (e.g., `0x1.p`).
pub type MalformedHexFloatLiteral = Malformed<HexFloatLiteral>;

/// A zero-copy error type representing a malformed token, literal, or syntax construct.
///
/// This type tracks the position of a syntactically incorrect construct, along with
/// optional knowledge about what kind of construct was attempted.
///
/// # Type Parameter
///
/// - `Knowledge`: Provides context about what was being parsed (typically a knowledge
///   marker type like `IntLiteral`, `StringLiteral`, etc.). Must implement `DisplayHuman`
///   for error messages.
///
/// # Common Use Cases
///
/// - **Malformed numbers**: `123abc`, `0x` (missing hex digits), `12.34.56`
/// - **Malformed strings**: Unterminated strings, invalid escape sequences
/// - **Malformed identifiers**: Mixed invalid characters
/// - **Malformed operators**: Incomplete multi-character operators
///
/// # Design
///
/// The `knowledge` field is optional, allowing this type to be used both with and
/// without specific context about what was being parsed. When knowledge is present,
/// error messages can include "did you mean X?" suggestions.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::{error::Malformed, SimpleSpan};
///
/// // Malformed token at position 10-15
/// let error: Malformed<()> = Malformed::new(SimpleSpan::new(10, 15));
/// assert_eq!(error.span(), SimpleSpan::new(10, 15));
/// assert_eq!(error.to_string(), "malformed at 10..15");
/// ```
///
/// ## With Knowledge Context
///
/// ```rust
/// use tokora::{error::Malformed, SimpleSpan, utils::{knowledge::IntLiteral}};
///
/// // Found "123abc" when parsing integer
/// let error = Malformed::int(SimpleSpan::new(5, 11));
/// assert_eq!(error.to_string(), "malformed at 5..11, did you mean int literal?");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Malformed<Knowledge, S = SimpleSpan> {
  span: S,
  knowledge: Option<Knowledge>,
}

impl<Knowledge, S> core::fmt::Display for Malformed<Knowledge, S>
where
  Knowledge: DisplayHuman,
  S: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.knowledge {
      Some(knowledge) => write!(
        f,
        "malformed at {}, did you mean {}?",
        self.span,
        knowledge.display()
      ),
      None => write!(f, "malformed at {}", self.span),
    }
  }
}

impl<Knowledge, S> core::error::Error for Malformed<Knowledge, S>
where
  Knowledge: DisplayHuman + core::fmt::Debug,
  S: core::fmt::Debug + core::fmt::Display,
{
}

impl<S> Malformed<BooleanLiteral, S> {
  /// Create a new Malformed knowledge for a boolean literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::BooleanLiteral};
  ///
  /// let error = Malformed::boolean(SimpleSpan::new(10, 14));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 14));
  /// assert_eq!(error.knowledge(), Some(&BooleanLiteral::default()));
  /// ```
  #[inline]
  pub const fn boolean(span: S) -> Self {
    Self::with_knowledge(span, BooleanLiteral(()))
  }
}

impl<S> Malformed<NullLiteral, S> {
  /// Create a new Malformed knowledge for a null literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::NullLiteral};
  ///
  /// let error = Malformed::null(SimpleSpan::new(20, 24));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 24));
  /// assert_eq!(error.knowledge(), Some(&NullLiteral::default()));
  /// ```
  #[inline]
  pub const fn null(span: S) -> Self {
    Self::with_knowledge(span, NullLiteral(()))
  }
}

impl<S> Malformed<EnumLiteral, S> {
  /// Create a new Malformed knowledge for an enum literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::EnumLiteral};
  ///
  /// let error = Malformed::enumeration(SimpleSpan::new(30, 40));
  /// assert_eq!(error.span(), SimpleSpan::new(30, 40));
  /// assert_eq!(error.knowledge(), Some(&EnumLiteral::default()));
  /// ```
  #[inline]
  pub const fn enumeration(span: S) -> Self {
    Self::with_knowledge(span, EnumLiteral(()))
  }
}

impl<S> Malformed<EnumValueLiteral, S> {
  /// Create a new Malformed knowledge for an enum value literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::EnumValueLiteral};
  ///
  /// let error = Malformed::enum_value(SimpleSpan::new(45, 49));
  /// assert_eq!(error.span(), SimpleSpan::new(45, 49));
  /// assert_eq!(error.knowledge(), Some(&EnumValueLiteral::default()));
  /// ```
  #[inline]
  pub const fn enum_value(span: S) -> Self {
    Self::with_knowledge(span, EnumValueLiteral(()))
  }
}

impl<S> Malformed<DecimalLiteral, S> {
  /// Create a new Malformed knowledge for a decimal literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::DecimalLiteral};
  ///
  /// let error = Malformed::decimal(SimpleSpan::new(150, 160));
  /// assert_eq!(error.span(), SimpleSpan::new(150, 160));
  /// assert_eq!(error.knowledge(), Some(&DecimalLiteral::default()));
  /// ```
  #[inline]
  pub const fn decimal(span: S) -> Self {
    Self::with_knowledge(span, DecimalLiteral(()))
  }
}

impl<S> Malformed<OctalLiteral, S> {
  /// Create a new Malformed knowledge for an octal literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::OctalLiteral};
  ///
  /// let error = Malformed::octal(SimpleSpan::new(50, 60));
  /// assert_eq!(error.span(), SimpleSpan::new(50, 60));
  /// assert_eq!(error.knowledge(), Some(&OctalLiteral::default()));
  /// ```
  #[inline]
  pub const fn octal(span: S) -> Self {
    Self::with_knowledge(span, OctalLiteral(()))
  }
}

impl<S> Malformed<StringLiteral, S> {
  /// Create a new Malformed knowledge for a string literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::StringLiteral};
  ///
  /// let error = Malformed::string(SimpleSpan::new(70, 80));
  /// assert_eq!(error.span(), SimpleSpan::new(70, 80));
  /// assert_eq!(error.knowledge(), Some(&StringLiteral::default()));
  /// ```
  #[inline]
  pub const fn string(span: S) -> Self {
    Self::with_knowledge(span, StringLiteral(()))
  }
}

impl<S> Malformed<HexLiteral, S> {
  /// Create a new Malformed knowledge for a hex literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::HexLiteral};
  ///
  /// let error = Malformed::hex(SimpleSpan::new(90, 100));
  /// assert_eq!(error.span(), SimpleSpan::new(90, 100));
  /// assert_eq!(error.knowledge(), Some(&HexLiteral::default()));
  /// ```
  #[inline]
  pub const fn hex(span: S) -> Self {
    Self::with_knowledge(span, HexLiteral(()))
  }
}

impl<S> Malformed<IntLiteral, S> {
  /// Create a new Malformed knowledge for an int literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::IntLiteral};
  ///
  /// let error = Malformed::int(SimpleSpan::new(105, 110));
  /// assert_eq!(error.span(), SimpleSpan::new(105, 110));
  /// assert_eq!(error.knowledge(), Some(&IntLiteral::default()));
  /// ```
  #[inline]
  pub const fn int(span: S) -> Self {
    Self::with_knowledge(span, IntLiteral(()))
  }
}

impl<S> Malformed<BinaryLiteral, S> {
  /// Create a new Malformed knowledge for a binary literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::BinaryLiteral};
  ///
  /// let error = Malformed::binary(SimpleSpan::new(115, 120));
  /// assert_eq!(error.span(), SimpleSpan::new(115, 120));
  /// assert_eq!(error.knowledge(), Some(&BinaryLiteral::default()));
  /// ```
  #[inline]
  pub const fn binary(span: S) -> Self {
    Self::with_knowledge(span, BinaryLiteral(()))
  }
}

impl<S> Malformed<FloatLiteral, S> {
  /// Create a new Malformed knowledge for a float literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::FloatLiteral};
  ///
  /// let error = Malformed::float(SimpleSpan::new(125, 130));
  /// assert_eq!(error.span(), SimpleSpan::new(125, 130));
  /// assert_eq!(error.knowledge(), Some(&FloatLiteral::default()));
  /// ```
  #[inline]
  pub const fn float(span: S) -> Self {
    Self::with_knowledge(span, FloatLiteral(()))
  }
}

impl<S> Malformed<HexFloatLiteral, S> {
  /// Create a new Malformed knowledge for a hex float literal from a SimpleSpan
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::knowledge::HexFloatLiteral};
  ///
  /// let error = Malformed::hex_float(SimpleSpan::new(135, 140));
  /// assert_eq!(error.span(), SimpleSpan::new(135, 140));
  /// assert_eq!(error.knowledge(), Some(&HexFloatLiteral::default()));
  /// ```
  #[inline]
  pub const fn hex_float(span: S) -> Self {
    Self::with_knowledge(span, HexFloatLiteral(()))
  }
}

impl<Knowledge, S> Malformed<Knowledge, S> {
  /// Creates a new malformed error without specific knowledge context.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan};
  ///
  /// let error: Malformed<()> = Malformed::new(SimpleSpan::new(10, 15));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 15));
  /// assert_eq!(error.knowledge(), None);
  /// ```
  #[inline]
  pub const fn new(span: S) -> Self {
    Self {
      span,
      knowledge: None,
    }
  }

  /// Creates a new malformed error with knowledge about what was being parsed.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::{knowledge::IntLiteral}};
  ///
  /// let error = Malformed::with_knowledge(SimpleSpan::new(5, 10), IntLiteral::default());
  /// assert_eq!(error.knowledge().is_some(), true);
  /// ```
  #[inline]
  pub const fn with_knowledge(span: S, knowledge: Knowledge) -> Self {
    Self {
      span,
      knowledge: Some(knowledge),
    }
  }

  /// Returns the span of the malformed construct.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan};
  ///
  /// let error: Malformed<()> = Malformed::new(SimpleSpan::new(20, 25));
  /// assert_eq!(error.span(), SimpleSpan::new(20, 25));
  /// ```
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns a reference to the span of the malformed construct.
  #[inline]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a mutable reference to the span of the malformed construct.
  #[inline]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Returns the knowledge about what was being parsed, if available.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan, utils::{knowledge::FloatLiteral}};
  ///
  /// let error = Malformed::float(SimpleSpan::new(10, 15));
  /// assert!(error.knowledge().is_some());
  /// ```
  #[inline]
  pub const fn knowledge(&self) -> Option<&Knowledge> {
    self.knowledge.as_ref()
  }

  /// Consumes the error and returns its components.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{error::Malformed, SimpleSpan};
  ///
  /// let error: Malformed<()> = Malformed::new(SimpleSpan::new(15, 20));
  /// let (span, knowledge) = error.into_components();
  /// assert_eq!(span, SimpleSpan::new(15, 20));
  /// assert_eq!(knowledge, None);
  /// ```
  #[inline]
  pub fn into_components(self) -> (S, Option<Knowledge>) {
    (self.span, self.knowledge)
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
  /// use tokora::{error::Malformed, SimpleSpan};
  ///
  /// let mut error: Malformed<()> = Malformed::new(SimpleSpan::new(10, 15));
  /// error.bump(&100);
  /// assert_eq!(error.span(), SimpleSpan::new(110, 115));
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
