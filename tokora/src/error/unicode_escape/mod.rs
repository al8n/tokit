//! Unicode escape sequence error types for lexer error reporting.
//!
//! This module provides comprehensive error types for handling failures in Unicode
//! escape sequences during lexical analysis. It supports both fixed-width (`\uXXXX`)
//! and variable-length (`\u{...}`) Unicode escape formats.
//!
//! # Design Philosophy
//!
//! Unicode escape sequences can fail in several ways:
//! - **Syntax errors**: Missing digits, unclosed braces, invalid characters
//! - **Semantic errors**: Surrogate values, overflow beyond valid Unicode range
//! - **Incompleteness**: Reaching end-of-input mid-escape
//!
//! This module distinguishes between:
//! - **Malformed** syntax (invalid hex digits, wrong format)
//! - **Invalid** values (surrogates, overflow)
//! - **Incomplete** sequences (unexpected EOF)
//!
//! # Unicode Escape Formats
//!
//! ## Fixed-Width Escapes: `\uXXXX`
//!
//! Fixed-width escapes require exactly 4 hexadecimal digits and can encode:
//! - Basic Multilingual Plane (BMP): `\u0000` to `\uFFFF`
//! - Surrogate pairs for characters beyond BMP (requires two escapes)
//!
//! Common errors:
//! - `\uZZ` - incomplete (only 2 digits)
//! - `\uGGGG` - malformed (invalid hex)
//! - `\uD800` - unpaired high surrogate
//!
//! ## Variable-Length Escapes: `\u{...}`
//!
//! Variable-length escapes support 1-6 hex digits and directly encode any Unicode scalar:
//! - Valid range: `\u{0}` to `\u{10FFFF}`
//! - Cannot encode surrogates: `\u{D800}` to `\u{DFFF}` are invalid
//!
//! Common errors:
//! - `\u{}` - empty braces
//! - `\u{1234567}` - too many digits (>6)
//! - `\u{D800}` - surrogate value
//! - `\u{110000}` - overflow (> 0x10FFFF)
//!
//! # Error Type Hierarchy
//!
//! ```text
//! UnicodeEscapeError
//! ├─ Fixed (FixedUnicodeEscapeError)
//! │  ├─ Incomplete
//! │  ├─ Malformed (MalformedFixedUnicodeEscape)
//! │  └─ UnpairedSurrogate
//! └─ Variable (VariableUnicodeEscapeError)
//!    ├─ Unclosed
//!    ├─ Empty
//!    ├─ TooManyDigits
//!    ├─ Malformed
//!    └─ InvalidScalar (surrogate or overflow)
//! ```
//!
//! # Examples
//!
//! ## Detecting Malformed Fixed-Width Escapes
//!
//! ```
//! use tokora::error::{UnicodeEscapeError, InvalidFixedUnicodeHexDigits};
//! use tokora::{SimpleSpan, utils::{PositionedChar}};
//!
//! // Invalid hex digit 'G' at position 12
//! let mut digits = InvalidFixedUnicodeHexDigits::<char>::from_char(12, 'G');
//! // ... collect invalid digits ...
//!
//! let error = UnicodeEscapeError::<char>::malformed_fixed_unicode_escape(
//!     digits,
//!     SimpleSpan::new(10, 16) // \uGGGG
//! );
//! ```
//!
//! ## Detecting Variable-Length Escape Errors
//!
//! ```
//! use tokora::error::UnicodeEscapeError;
//! use tokora::SimpleSpan;
//!
//! // Empty braces: \u{}
//! let error = UnicodeEscapeError::<char>::empty_variable_unicode_escape(
//!     SimpleSpan::new(5, 9)
//! );
//!
//! // Surrogate value: \u{D800}
//! let error = UnicodeEscapeError::<char>::surrogate_variable_unicode_escape(
//!     SimpleSpan::new(10, 18),
//!     0xD800
//! );
//!
//! // Overflow: \u{110000}
//! let error = UnicodeEscapeError::<char>::overflow_variable_unicode_escape(
//!     SimpleSpan::new(20, 30),
//!     0x110000
//! );
//! ```

use core::ops::{Add, AddAssign};

use crate::{
  error::{Unclosed, UnexpectedLexeme},
  punct::Brace,
  span::SimpleSpan,
  utils::{CharLen, CowStr, Lexeme, PositionedChar, human_display::DisplayHuman},
};
use derive_more::{Display, From, IsVariant, TryUnwrap, Unwrap};

/// A zero-copy container for storing 1-4 invalid unicode hex digit characters.
///
/// This structure is designed for fixed-width unicode escapes (`\uXXXX`) which
/// require exactly 4 hexadecimal digits. When parsing fails, this container holds
/// the invalid characters encountered (up to 4) with their positions, enabling
/// precise error reporting without heap allocation.
///
/// # Design
///
/// The container uses an internal representation optimized for small sizes (1-4 items)
/// to avoid heap allocation. It implements `Deref<Target = [PositionedChar<Char>]>`
/// for convenient access to the stored characters.
///
/// # Type Parameters
///
/// * `Char` - The character type (typically `char` for UTF-8 or `u8` for bytes)
///
/// # Examples
///
/// ```
/// use tokora::error::InvalidFixedUnicodeHexDigits;
/// use tokora::utils::PositionedChar;
///
/// // Create from a single invalid character
/// let digit = InvalidFixedUnicodeHexDigits::from(
///     PositionedChar::with_position('G', 12)
/// );
/// assert_eq!(digit.len(), 1);
///
/// // Create from multiple invalid characters
/// let digits = InvalidFixedUnicodeHexDigits::from_array([
///     PositionedChar::with_position('G', 12),
///     PositionedChar::with_position('H', 13),
///     PositionedChar::with_position('I', 14),
///     PositionedChar::with_position('J', 15),
/// ]);
/// assert_eq!(digits.len(), 4);
///
/// // Access as a slice
/// for ch in digits.iter() {
///     println!("Invalid hex digit at position {}", ch.position());
/// }
/// ```
pub type InvalidFixedUnicodeHexDigits<Char = char, O = usize> =
  crate::error::InvalidHexDigits<Char, 4, O>;

/// A malformed fixed-width unicode escape sequence error.
///
/// This error occurs when a fixed-width unicode escape (`\uXXXX`) contains
/// invalid hexadecimal digits. The error captures both the invalid characters
/// encountered and the span of the malformed escape sequence.
///
/// # Type Parameters
///
/// * `Char` - The character type (typically `char` for UTF-8 or `u8` for bytes)
///
/// # Examples
///
/// ```
/// use tokora::error::{MalformedFixedUnicodeEscape, InvalidFixedUnicodeHexDigits};
/// use tokora::{SimpleSpan, utils::{PositionedChar}};
///
/// // Create error for malformed escape like \uGHIJ
/// let digits = InvalidFixedUnicodeHexDigits::from_array([
///     PositionedChar::with_position('G', 12),
///     PositionedChar::with_position('H', 13),
///     PositionedChar::with_position('I', 14),
///     PositionedChar::with_position('J', 15),
/// ]);
///
/// let error = MalformedFixedUnicodeEscape::new(
///     digits,
///     SimpleSpan::new(10, 16) // \uGHIJ
/// );
///
/// assert_eq!(error.span(), SimpleSpan::new(10, 16));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MalformedFixedUnicodeEscape<Char = char, O = usize> {
  digits: InvalidFixedUnicodeHexDigits<Char, O>,
  span: SimpleSpan<O>,
}

impl<Char, O> core::fmt::Display for MalformedFixedUnicodeEscape<Char, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(
      f,
      "malformed hexadecimal escape sequence with invalid digits at {}, {}",
      self.span,
      self.digits_ref()
    )
  }
}

impl<Char, O> core::error::Error for MalformedFixedUnicodeEscape<Char, O>
where
  Char: DisplayHuman + core::fmt::Debug,
  O: core::fmt::Display + core::fmt::Debug,
{
}

impl<Char, O> MalformedFixedUnicodeEscape<Char, O> {
  /// Creates a new malformed fixed-width unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedFixedUnicodeEscape, InvalidFixedUnicodeHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let digits = InvalidFixedUnicodeHexDigits::from(
  ///     PositionedChar::with_position('Z', 12)
  /// );
  /// let error = MalformedFixedUnicodeEscape::new(digits, SimpleSpan::new(10, 14));
  /// ```
  #[inline]
  pub const fn new(digits: InvalidFixedUnicodeHexDigits<Char, O>, span: SimpleSpan<O>) -> Self {
    Self { digits, span }
  }

  /// Returns the invalid unicode hex digits.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedFixedUnicodeEscape, InvalidFixedUnicodeHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let digits = InvalidFixedUnicodeHexDigits::from(
  ///     PositionedChar::with_position('G', 12)
  /// );
  /// let error = MalformedFixedUnicodeEscape::new(digits, SimpleSpan::new(10, 14));
  /// assert_eq!(error.digits().len(), 1);
  /// ```
  #[inline]
  pub fn digits(&self) -> InvalidFixedUnicodeHexDigits<Char, O>
  where
    Char: Clone,
    O: Clone,
  {
    self.digits.clone()
  }

  /// Returns a reference to the invalid unicode hex digits.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedFixedUnicodeEscape, InvalidFixedUnicodeHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let digits = InvalidFixedUnicodeHexDigits::from(
  ///     PositionedChar::with_position('G', 12)
  /// );
  /// let error = MalformedFixedUnicodeEscape::new(digits, SimpleSpan::new(10, 14));
  /// assert_eq!(error.digits_ref().len(), 1);
  /// ```
  #[inline]
  pub const fn digits_ref(&self) -> &InvalidFixedUnicodeHexDigits<Char, O> {
    &self.digits
  }

  /// Returns a mutable reference to the invalid unicode hex digits.
  #[inline]
  pub const fn digits_mut(&mut self) -> &mut InvalidFixedUnicodeHexDigits<Char, O> {
    &mut self.digits
  }

  /// Returns the span of the malformed unicode escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedFixedUnicodeEscape, InvalidFixedUnicodeHexDigits};
  /// use tokora::SimpleSpan;
  ///
  /// let error = MalformedFixedUnicodeEscape::new(
  ///     InvalidFixedUnicodeHexDigits::<char>::from_char(12, 'G'),
  ///     SimpleSpan::new(10, 16)
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(10, 16));
  /// ```
  #[inline]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.span
  }

  /// Returns a reference to the span of the malformed unicode escape.
  #[inline]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.span.as_ref()
  }

  /// Returns a mutable reference to the span of the malformed unicode escape.
  #[inline]
  pub const fn span_mut(&mut self) -> SimpleSpan<&mut O> {
    self.span.as_mut()
  }

  /// Bumps the span and all digit positions by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedFixedUnicodeEscape, InvalidFixedUnicodeHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let mut error = MalformedFixedUnicodeEscape::new(
  ///     InvalidFixedUnicodeHexDigits::from(PositionedChar::with_position('G', 12)),
  ///     SimpleSpan::new(10, 16)
  /// );
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 21));
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.span.bump(n);
    self.digits_mut().bump(n);
    self
  }
}

/// The reason why a parsed value is not a valid Unicode scalar.
///
/// A valid Unicode scalar value is in the range `0x0000..=0x10FFFF`, excluding
/// the surrogate range `0xD800..=0xDFFF`.
///
/// # Examples
///
/// ```
/// use tokora::error::InvalidUnicodeScalarKind;
///
/// // Surrogate values (0xD800..=0xDFFF) are reserved for UTF-16 encoding
/// let kind = InvalidUnicodeScalarKind::Surrogate;
///
/// // Values above 0x10FFFF are beyond the Unicode codespace
/// let kind = InvalidUnicodeScalarKind::Overflow;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidUnicodeScalarKind {
  /// In the UTF-16 surrogate range: `0xD800..=0xDFFF`.
  ///
  /// These values are reserved for UTF-16 surrogate pair encoding and
  /// are not valid Unicode scalar values.
  Surrogate,

  /// Above the Unicode maximum scalar value: `> 0x10FFFF`.
  ///
  /// The highest valid Unicode code point is U+10FFFF. Values above
  /// this are outside the defined Unicode codespace.
  Overflow,
}

/// An invalid unicode scalar value error.
///
/// This error occurs when a unicode escape sequence parses successfully to a
/// numeric value, but that value is not a valid Unicode scalar. There are two
/// reasons this can happen:
///
/// 1. **Surrogate**: The value is in the range `0xD800..=0xDFFF` (UTF-16 surrogates)
/// 2. **Overflow**: The value exceeds `0x10FFFF` (the maximum Unicode code point)
///
/// # Examples
///
/// ```
/// use tokora::error::{InvalidUnicodeScalarValue, InvalidUnicodeScalarKind};
/// use tokora::SimpleSpan;
///
/// // Surrogate value error: \u{D800}
/// let error = InvalidUnicodeScalarValue::new(
///     0xD800,
///     SimpleSpan::new(10, 18),
///     InvalidUnicodeScalarKind::Surrogate
/// );
/// assert_eq!(error.codepoint(), 0xD800);
/// assert_eq!(error.kind(), InvalidUnicodeScalarKind::Surrogate);
///
/// // Overflow error: \u{110000}
/// let error = InvalidUnicodeScalarValue::new(
///     0x110000,
///     SimpleSpan::new(20, 30),
///     InvalidUnicodeScalarKind::Overflow
/// );
/// assert_eq!(error.codepoint(), 0x110000);
/// assert_eq!(error.kind(), InvalidUnicodeScalarKind::Overflow);
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct InvalidUnicodeScalarValue<O = usize> {
  value: u32,
  span: SimpleSpan<O>,
  kind: InvalidUnicodeScalarKind,
}

impl<O> core::fmt::Display for InvalidUnicodeScalarValue<O>
where
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let cp = self.value;

    match self.kind {
      InvalidUnicodeScalarKind::Surrogate => write!(
        f,
        "invalid Unicode scalar value: surrogate code point U+{cp:04X} at {}",
        self.span
      ),
      InvalidUnicodeScalarKind::Overflow => write!(
        f,
        "invalid Unicode scalar value: code point U+{cp:04X} is out of range at {}",
        self.span
      ),
    }
  }
}

impl<O> core::error::Error for InvalidUnicodeScalarValue<O> where
  O: core::fmt::Display + core::fmt::Debug
{
}

impl<O> InvalidUnicodeScalarValue<O> {
  /// Creates a new invalid unicode scalar value error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{InvalidUnicodeScalarValue, InvalidUnicodeScalarKind};
  /// use tokora::SimpleSpan;
  ///
  /// let error = InvalidUnicodeScalarValue::new(
  ///     0xD800,
  ///     SimpleSpan::new(10, 18),
  ///     InvalidUnicodeScalarKind::Surrogate
  /// );
  /// ```
  #[inline]
  pub const fn new(value: u32, span: SimpleSpan<O>, kind: InvalidUnicodeScalarKind) -> Self {
    Self { value, span, kind }
  }

  /// Returns the invalid codepoint value.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{InvalidUnicodeScalarValue, InvalidUnicodeScalarKind};
  /// use tokora::SimpleSpan;
  ///
  /// let error = InvalidUnicodeScalarValue::new(
  ///     0xD800,
  ///     SimpleSpan::new(10, 18),
  ///     InvalidUnicodeScalarKind::Surrogate
  /// );
  /// assert_eq!(error.codepoint(), 0xD800);
  /// ```
  #[inline]
  pub const fn codepoint(&self) -> u32 {
    self.value
  }

  /// Returns the span of the invalid unicode scalar value.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{InvalidUnicodeScalarValue, InvalidUnicodeScalarKind};
  /// use tokora::SimpleSpan;
  ///
  /// let error = InvalidUnicodeScalarValue::new(
  ///     0x110000,
  ///     SimpleSpan::new(5, 15),
  ///     InvalidUnicodeScalarKind::Overflow
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(5, 15));
  /// ```
  #[inline]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.span
  }

  /// Returns a reference to the span.
  #[inline]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.span.as_ref()
  }

  /// Returns a mutable reference to the span.
  #[inline]
  pub const fn span_mut(&mut self) -> SimpleSpan<&mut O> {
    self.span.as_mut()
  }

  /// Bumps the span by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{InvalidUnicodeScalarValue, InvalidUnicodeScalarKind};
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = InvalidUnicodeScalarValue::new(
  ///     0xD800,
  ///     SimpleSpan::new(10, 18),
  ///     InvalidUnicodeScalarKind::Surrogate
  /// );
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 23));
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.span.bump(n);
    self
  }

  /// Returns the kind of invalid unicode scalar value.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{InvalidUnicodeScalarValue, InvalidUnicodeScalarKind};
  /// use tokora::SimpleSpan;
  ///
  /// let error = InvalidUnicodeScalarValue::new(
  ///     0xD800,
  ///     SimpleSpan::new(10, 18),
  ///     InvalidUnicodeScalarKind::Surrogate
  /// );
  /// assert_eq!(error.kind(), InvalidUnicodeScalarKind::Surrogate);
  /// ```
  #[inline]
  pub const fn kind(&self) -> InvalidUnicodeScalarKind {
    self.kind
  }
}

/// An empty variable-length unicode escape error.
///
/// This error occurs when a variable-length unicode escape has no hex digits between
/// the braces: `\u{}`.
///
/// A valid variable-length unicode escape requires at least one hex digit, e.g., `\u{0}`.
///
/// # Examples
///
/// ```
/// use tokora::error::EmptyVariableUnicodeEscape;
/// use tokora::SimpleSpan;
///
/// // Error for: \u{}
/// let error = EmptyVariableUnicodeEscape::new(SimpleSpan::new(10, 14));
/// assert_eq!(error.span(), SimpleSpan::new(10, 14));
/// assert_eq!(format!("{}", error), "empty variable-length unicode escape at 10..14");
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("empty variable-length unicode escape at {_0}")]
pub struct EmptyVariableUnicodeEscape<O = usize>(SimpleSpan<O>);

impl<O> EmptyVariableUnicodeEscape<O> {
  /// Creates a new empty variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::EmptyVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = EmptyVariableUnicodeEscape::new(SimpleSpan::new(5, 9));
  /// assert_eq!(error.span(), SimpleSpan::new(5, 9));
  /// ```
  #[inline]
  pub const fn new(span: SimpleSpan<O>) -> Self {
    Self(span)
  }

  /// Returns the span of the empty variable-length unicode escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::EmptyVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = EmptyVariableUnicodeEscape::new(SimpleSpan::new(10, 14));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 14));
  /// ```
  #[inline]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.0
  }

  /// Returns the span of the empty variable-length unicode escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::EmptyVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = EmptyVariableUnicodeEscape::new(SimpleSpan::new(10, 14));
  /// assert_eq!(error.span_ref(), SimpleSpan::new(&10, &14));
  /// ```
  #[inline]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.0.as_ref()
  }

  /// Returns the span of the empty variable-length unicode escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::EmptyVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = EmptyVariableUnicodeEscape::new(SimpleSpan::new(10, 14));
  /// let span = error.span_mut();
  /// assert_eq!(**span.start_ref(), 10);
  /// assert_eq!(**span.end_ref(), 14);
  /// ```
  #[inline]
  pub const fn span_mut(&mut self) -> SimpleSpan<&mut O> {
    self.0.as_mut()
  }

  /// Bumps the span by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::EmptyVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = EmptyVariableUnicodeEscape::new(SimpleSpan::new(10, 14));
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 19));
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.0.bump(n);
    self
  }
}

impl<O> core::error::Error for EmptyVariableUnicodeEscape<O> where
  O: core::fmt::Display + core::fmt::Debug
{
}

/// A malformed variable-length unicode escape sequence error.
///
/// This error occurs when a variable-length unicode escape (`\u{...}`) contains
/// invalid characters between the braces. Valid braced escapes require
/// only hexadecimal digits (0-9, a-f, A-F).
///
/// # Type Parameters
///
/// * `Char` - The character type (typically `char` for UTF-8 or `u8` for bytes)
///
/// # Examples
///
/// ```
/// use tokora::error::MalformedVariableUnicodeSequence;
/// use tokora::utils::{Lexeme, PositionedChar};
///
/// // Error for: \u{GGGG}
/// let error = MalformedVariableUnicodeSequence::<char>::from_char(12, 'G');
/// assert_eq!(
///     format!("{}", error),
///     "invalid variable-length unicode escape character 'G' at position 12"
/// );
///
/// // Error for a span of invalid characters
/// let error: MalformedVariableUnicodeSequence<char> =
///     MalformedVariableUnicodeSequence::from_range((10, 15).into());
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct MalformedVariableUnicodeSequence<Char = char, O = usize>(Lexeme<Char, O>);

impl<Char, O> core::fmt::Display for MalformedVariableUnicodeSequence<Char, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self.lexeme_ref() {
      Lexeme::Char(positioned_char) => write!(
        f,
        "invalid variable-length unicode escape character '{}' at position {}",
        positioned_char.char_ref().display(),
        positioned_char.position_ref()
      ),
      Lexeme::Range(span) => write!(
        f,
        "malformed variable-length unicode escape sequence at {}",
        span
      ),
    }
  }
}

impl<Char, O> core::error::Error for MalformedVariableUnicodeSequence<Char, O>
where
  Char: DisplayHuman + core::fmt::Debug,
  O: core::fmt::Display + core::fmt::Debug,
{
}

impl<Char, O> MalformedVariableUnicodeSequence<Char, O> {
  /// Creates a new malformed variable-length unicode escape error from a lexeme.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::MalformedVariableUnicodeSequence;
  /// use tokora::utils::{Lexeme, PositionedChar};
  ///
  /// let lexeme = Lexeme::from(PositionedChar::with_position('Z', 15));
  /// let error = MalformedVariableUnicodeSequence::new(lexeme);
  /// ```
  #[inline]
  pub const fn new(lexeme: Lexeme<Char, O>) -> Self {
    Self(lexeme)
  }

  /// Creates a new malformed variable-length unicode escape error from a positioned character.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::MalformedVariableUnicodeSequence;
  ///
  /// let error = MalformedVariableUnicodeSequence::from_char(42, 'X');
  /// assert_eq!(
  ///     format!("{}", error),
  ///     "invalid variable-length unicode escape character 'X' at position 42"
  /// );
  /// ```
  #[inline]
  pub const fn from_char(pos: O, ch: Char) -> Self {
    Self::from_positioned_char(PositionedChar::with_position(ch, pos))
  }

  /// Creates a new malformed variable-length unicode escape error from a positioned character.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{error::MalformedVariableUnicodeSequence, utils::PositionedChar};
  ///
  /// let error = MalformedVariableUnicodeSequence::from_positioned_char(PositionedChar::with_position('X', 42));
  /// assert_eq!(
  ///     format!("{}", error),
  ///     "invalid variable-length unicode escape character 'X' at position 42"
  /// );
  /// ```
  #[inline]
  pub const fn from_positioned_char(ch: PositionedChar<Char, O>) -> Self {
    Self(Lexeme::Char(ch))
  }

  /// Creates a new malformed variable-length unicode escape error from a span.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::MalformedVariableUnicodeSequence;
  /// use tokora::SimpleSpan;
  ///
  /// let error: MalformedVariableUnicodeSequence<char> =
  ///     MalformedVariableUnicodeSequence::from_range(SimpleSpan::new(10, 15));
  /// ```
  #[inline]
  pub const fn from_range(span: SimpleSpan<O>) -> Self {
    Self(Lexeme::Range(span))
  }

  /// Returns the span of the malformed variable-length unicode escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::MalformedVariableUnicodeSequence;
  /// use tokora::SimpleSpan;
  ///
  /// let error = MalformedVariableUnicodeSequence::from_char(10, 'G');
  /// assert_eq!(error.span(), SimpleSpan::new(10, 11));
  /// ```
  #[inline]
  pub fn span(&self) -> SimpleSpan<O>
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    self.0.span()
  }

  /// Returns the lexeme of the malformed variable-length unicode escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::MalformedVariableUnicodeSequence;
  /// use tokora::utils::Lexeme;
  ///
  /// let error = MalformedVariableUnicodeSequence::from_char(10, 'G');
  /// assert!(error.lexeme().is_char());
  /// ```
  #[inline]
  pub const fn lexeme(&self) -> Lexeme<Char, O>
  where
    Char: Copy,
    O: Copy,
  {
    self.0
  }

  /// Returns a reference to the lexeme.
  #[inline]
  pub const fn lexeme_ref(&self) -> &Lexeme<Char, O> {
    &self.0
  }

  /// Bumps the span or position by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::MalformedVariableUnicodeSequence;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = MalformedVariableUnicodeSequence::from_char(10, 'G');
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 16));
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.0.bump(n);
    self
  }
}

/// Too many digits in variable-length unicode escape error.
///
/// A valid variable-length unicode escape can have between 1 and 6 hex digits.
/// This error occurs when more than 6 hex digits are found.
///
/// # Examples
///
/// ```
/// use tokora::error::TooManyDigitsInVariableUnicodeEscape;
/// use tokora::SimpleSpan;
///
/// // Error for: \u{1234567} (7 digits, limit is 6)
/// let error = TooManyDigitsInVariableUnicodeEscape::new(
///     SimpleSpan::new(10, 21),
///     7
/// );
/// assert_eq!(error.count(), 7);
/// assert_eq!(
///     format!("{}", error),
///     "too many digits (7) in variable-length unicode escape at 10..21"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[display("too many digits ({_1}) in variable-length unicode escape at {_0}")]
pub struct TooManyDigitsInVariableUnicodeEscape<O = usize>(SimpleSpan<O>, usize);

impl<O> TooManyDigitsInVariableUnicodeEscape<O> {
  /// Creates a new too many digits in variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::TooManyDigitsInVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = TooManyDigitsInVariableUnicodeEscape::new(SimpleSpan::new(5, 15), 8);
  /// assert_eq!(error.count(), 8);
  /// ```
  #[inline]
  pub const fn new(span: SimpleSpan<O>, count: usize) -> Self {
    Self(span, count)
  }

  /// Returns the span of the too many digits error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::TooManyDigitsInVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = TooManyDigitsInVariableUnicodeEscape::new(SimpleSpan::new(10, 20), 7);
  /// assert_eq!(error.span(), SimpleSpan::new(10, 20));
  /// ```
  #[inline]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.0
  }

  /// Returns the span of the too many digits error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::TooManyDigitsInVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = TooManyDigitsInVariableUnicodeEscape::new(SimpleSpan::new(10, 20), 7);
  /// assert_eq!(error.span(), SimpleSpan::new(10, 20));
  /// ```
  #[inline]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.0.as_ref()
  }

  /// Returns the span of the too many digits error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::TooManyDigitsInVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = TooManyDigitsInVariableUnicodeEscape::new(SimpleSpan::new(10, 20), 7);
  /// assert_eq!(error.span(), SimpleSpan::new(10, 20));
  /// ```
  #[inline]
  pub const fn span_mut(&mut self) -> SimpleSpan<&mut O> {
    self.0.as_mut()
  }

  /// Returns the count of hex digits found.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::TooManyDigitsInVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = TooManyDigitsInVariableUnicodeEscape::new(SimpleSpan::new(10, 20), 7);
  /// assert_eq!(error.count(), 7);
  /// ```
  #[inline]
  pub const fn count(&self) -> usize {
    self.1
  }

  /// Bumps the span by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::TooManyDigitsInVariableUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = TooManyDigitsInVariableUnicodeEscape::new(SimpleSpan::new(10, 20), 7);
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 25));
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.0.bump(n);
    self
  }
}

impl<O> core::error::Error for TooManyDigitsInVariableUnicodeEscape<O> where
  O: core::fmt::Display + core::fmt::Debug + 'static
{
}

/// An error encountered during lexing for `\u{...}` (variable-length) unicode escape sequences.
///
/// Variable-length unicode escapes allow 1-6 hexadecimal digits to encode any valid Unicode
/// scalar value (U+0000 to U+10FFFF, excluding surrogates U+D800 to U+DFFF).
///
/// # Variants
///
/// - **Unclosed**: The opening brace was not closed, e.g., `\u{1234`
/// - **Empty**: The braces contained no digits, e.g., `\u{}`
/// - **TooManyDigits**: More than 6 hex digits inside the braces, e.g., `\u{1234567}`
/// - **Malformed**: Invalid characters (non-hex) inside the braces, e.g., `\u{GGGG}`
/// - **InvalidScalar**: Valid hex but invalid Unicode scalar (surrogate or overflow)
///
/// # Examples
///
/// ```
/// use tokora::error::VariableUnicodeEscapeError;
/// use tokora::SimpleSpan;
///
/// // Empty braces
/// let error = VariableUnicodeEscapeError::<char>::empty(SimpleSpan::new(10, 14));
/// assert!(error.is_empty());
///
/// // Too many digits
/// let error = VariableUnicodeEscapeError::<char>::too_many_digits(SimpleSpan::new(5, 16), 7);
/// assert!(error.is_too_many_digits());
///
/// // Surrogate value
/// let error = VariableUnicodeEscapeError::<char>::surrogate(SimpleSpan::new(10, 18), 0xD800);
/// assert!(error.is_invalid_scalar());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, From, IsVariant, TryUnwrap, Unwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
#[non_exhaustive]
pub enum VariableUnicodeEscapeError<Char = char, O = usize> {
  /// The opening brace was not closed: `\u{1234`.
  Unclosed(Unclosed<Brace, SimpleSpan<O>>),

  /// The braces contained **no** digits: `\u{}`.
  Empty(EmptyVariableUnicodeEscape<O>),

  /// More than 6 hex digits inside the braces.
  TooManyDigits(TooManyDigitsInVariableUnicodeEscape<O>),

  /// A malformed sequence of unicode in the braces.
  Malformed(MalformedVariableUnicodeSequence<Char, O>),

  /// Parsed number is not a Unicode scalar value (surrogate or > 0x10_FFFF).
  InvalidScalar(InvalidUnicodeScalarValue<O>),
}

impl<Char, O> core::fmt::Display for VariableUnicodeEscapeError<Char, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Unclosed(err) => {
        write!(
          f,
          "unclosed variable-length unicode escape at {}",
          err.span_ref()
        )
      }
      Self::Empty(err) => err.fmt(f),
      Self::TooManyDigits(err) => err.fmt(f),
      Self::Malformed(err) => err.fmt(f),
      Self::InvalidScalar(err) => err.fmt(f),
    }
  }
}

impl<Char, O> core::error::Error for VariableUnicodeEscapeError<Char, O>
where
  Char: DisplayHuman + core::fmt::Debug + 'static,
  O: core::fmt::Display + core::fmt::Debug + 'static,
{
  fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
    match self {
      Self::Unclosed(err) => Some(err),
      Self::Empty(err) => Some(err),
      Self::TooManyDigits(err) => Some(err),
      Self::Malformed(err) => Some(err),
      Self::InvalidScalar(err) => Some(err),
    }
  }
}

impl<Char, O> VariableUnicodeEscapeError<Char, O> {
  /// Creates an empty variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::VariableUnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error: VariableUnicodeEscapeError<char> =
  ///     VariableUnicodeEscapeError::empty(SimpleSpan::new(10, 14));
  /// assert!(error.is_empty());
  /// ```
  #[inline]
  pub const fn empty(span: SimpleSpan<O>) -> Self {
    Self::Empty(EmptyVariableUnicodeEscape::new(span))
  }

  /// Creates a too many digits in variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::VariableUnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error: VariableUnicodeEscapeError<char> =
  ///     VariableUnicodeEscapeError::too_many_digits(SimpleSpan::new(5, 15), 7);
  /// assert!(error.is_too_many_digits());
  /// ```
  #[inline]
  pub const fn too_many_digits(span: SimpleSpan<O>, count: usize) -> Self {
    Self::TooManyDigits(TooManyDigitsInVariableUnicodeEscape::new(span, count))
  }

  /// Creates an unclosed brace in variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::VariableUnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error: VariableUnicodeEscapeError<char> =
  ///     VariableUnicodeEscapeError::unclosed(SimpleSpan::new(10, 15));
  /// assert!(error.is_unclosed());
  /// ```
  #[inline]
  pub const fn unclosed(span: SimpleSpan<O>) -> Self {
    Self::Unclosed(Unclosed::new(
      span,
      crate::delimiter::DelimiterKind::Brace,
      CowStr::from_static("{}"),
    ))
  }

  /// Creates an overflow error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::VariableUnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error: VariableUnicodeEscapeError<char> =
  ///     VariableUnicodeEscapeError::overflow(SimpleSpan::new(10, 20), 0x110000);
  /// assert!(error.is_invalid_scalar());
  /// ```
  #[inline]
  pub const fn overflow(span: SimpleSpan<O>, codepoint: u32) -> Self {
    Self::InvalidScalar(InvalidUnicodeScalarValue::new(
      codepoint,
      span,
      InvalidUnicodeScalarKind::Overflow,
    ))
  }

  /// Creates a surrogate error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::VariableUnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error: VariableUnicodeEscapeError<char> =
  ///     VariableUnicodeEscapeError::surrogate(SimpleSpan::new(10, 18), 0xD800);
  /// assert!(error.is_invalid_scalar());
  /// ```
  #[inline]
  pub const fn surrogate(span: SimpleSpan<O>, codepoint: u32) -> Self {
    Self::InvalidScalar(InvalidUnicodeScalarValue::new(
      codepoint,
      span,
      InvalidUnicodeScalarKind::Surrogate,
    ))
  }

  /// Bumps the span of the error by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::VariableUnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error: VariableUnicodeEscapeError<char> =
  ///     VariableUnicodeEscapeError::empty(SimpleSpan::new(10, 14));
  /// error.bump(&5);
  /// // Now the span would be adjusted by 5
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone + Ord + core::hash::Hash,
  {
    match self {
      Self::Unclosed(err) => {
        err.bump(n);
      }
      Self::Empty(err) => {
        err.bump(n);
      }
      Self::TooManyDigits(err) => {
        err.bump(n);
      }
      Self::Malformed(err) => {
        err.bump(n);
      }
      Self::InvalidScalar(err) => {
        err.bump(n);
      }
    }
    self
  }

  /// Returns the span of the error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::VariableUnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error: VariableUnicodeEscapeError<char> =
  ///    VariableUnicodeEscapeError::empty(SimpleSpan::new(10, 14));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 14));
  /// ```
  pub fn span(&self) -> SimpleSpan<O>
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    match self {
      Self::Unclosed(err) => err.span_ref().clone(),
      Self::Empty(err) => err.span_ref().cloned(),
      Self::TooManyDigits(err) => err.span_ref().cloned(),
      Self::Malformed(err) => err.span(),
      Self::InvalidScalar(err) => err.span_ref().cloned(),
    }
  }
}

/// A hint describing why a surrogate is unpaired.
///
/// In UTF-16 encoding, surrogates must come in pairs (high followed by low).
/// This hint indicates which half of the pair was found without its match.
///
/// # Examples
///
/// ```
/// use tokora::error::UnpairedSurrogateHint;
///
/// let hint = UnpairedSurrogateHint::High;
/// assert_eq!(format!("{}", hint), "high surrogate");
///
/// let hint = UnpairedSurrogateHint::Low;
/// assert_eq!(format!("{}", hint), "low surrogate");
/// ```
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Display, IsVariant)]
pub enum UnpairedSurrogateHint {
  /// An unpaired high surrogate (U+D800..U+DBFF).
  ///
  /// A high surrogate must be followed by a low surrogate to form
  /// a valid UTF-16 surrogate pair.
  #[display("high surrogate")]
  High,

  /// An unpaired low surrogate (U+DC00..U+DFFF).
  ///
  /// A low surrogate must be preceded by a high surrogate to form
  /// a valid UTF-16 surrogate pair.
  #[display("low surrogate")]
  Low,
}

/// An incomplete fixed-width unicode escape sequence error.
///
/// This error occurs when a fixed-width unicode escape (`\uXXXX`) has fewer than 4 hex digits,
/// typically due to unexpected end-of-input or a non-hex character.
///
/// # Examples
///
/// ```
/// use tokora::error::IncompleteFixedUnicodeEscape;
/// use tokora::SimpleSpan;
///
/// // Incomplete: \u00A (only 3 hex digits)
/// let error = IncompleteFixedUnicodeEscape::new(
///     SimpleSpan::new(10, 13)
/// );
/// assert_eq!(error.span(), SimpleSpan::new(10, 13));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error(
  "incomplete fixed-width unicode escape sequence at {0}, fixed-width unicode escape must contains exactly four hexadecimal digits"
)]
pub struct IncompleteFixedUnicodeEscape<O = usize>(SimpleSpan<O>);

impl<O> IncompleteFixedUnicodeEscape<O> {
  /// Creates a new incomplete fixed-width unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::IncompleteFixedUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = IncompleteFixedUnicodeEscape::new(SimpleSpan::new(10, 12));
  /// ```
  #[inline]
  pub const fn new(span: SimpleSpan<O>) -> Self {
    Self(span)
  }

  /// Returns the span of the incomplete hex escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::IncompleteFixedUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = IncompleteFixedUnicodeEscape::new(SimpleSpan::new(10, 13));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 13));
  /// ```
  #[inline]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.0
  }

  /// Returns the span of the incomplete hex escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::IncompleteFixedUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = IncompleteFixedUnicodeEscape::new(SimpleSpan::new(10, 13));
  /// assert_eq!(error.span_ref(), SimpleSpan::new(&10, &13));
  /// ```
  #[inline]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.0.as_ref()
  }

  /// Returns the span of the incomplete hex escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::IncompleteFixedUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = IncompleteFixedUnicodeEscape::new(SimpleSpan::new(10, 13));
  /// let span = error.span_mut();
  /// assert_eq!(**span.start_ref(), 10);
  /// assert_eq!(**span.end_ref(), 13);
  /// ```
  #[inline]
  pub const fn span_mut(&mut self) -> SimpleSpan<&mut O> {
    self.0.as_mut()
  }

  /// Bumps the span or position by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::IncompleteFixedUnicodeEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = IncompleteFixedUnicodeEscape::new(SimpleSpan::new(10, 12));
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 17));
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.0.bump(n);
    self
  }
}

/// An error encountered during lexing for `\uXXXX` (fixed-width) unicode escape sequences.
///
/// Fixed-width unicode escapes require exactly 4 hexadecimal digits after `\u`.
/// They can encode values from U+0000 to U+FFFF (the Basic Multilingual Plane).
///
/// For characters beyond the BMP, UTF-16 surrogate pairs are used:
/// - High surrogate: U+D800..U+DBFF followed by
/// - Low surrogate: U+DC00..U+DFFF
///
/// # Variants
///
/// - **Incomplete**: The escape has fewer than 4 hex digits, e.g., `\uAB`
/// - **Malformed**: The 4 characters are not valid hexadecimal, e.g., `\uGGGG`
/// - **UnpairedSurrogate**: A surrogate value without its pair, e.g., `\uD800` alone
///
/// # Examples
///
/// ```
/// use tokora::error::{FixedUnicodeEscapeError, IncompleteFixedUnicodeEscape};
/// use tokora::{SimpleSpan, utils::{Lexeme}};
///
/// // Incomplete escape: \uAB (only 2 hex digits)
/// let error: FixedUnicodeEscapeError =
///     FixedUnicodeEscapeError::Incomplete(IncompleteFixedUnicodeEscape::new(SimpleSpan::new(10, 14)));
/// assert!(error.is_incomplete());
///
/// // Unpaired high surrogate: \uD800
/// let error = FixedUnicodeEscapeError::<char>::unpaired_high_surrogate(
///     Lexeme::Range(SimpleSpan::new(5, 11))
/// );
/// assert!(error.is_unpaired_surrogate());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, From, IsVariant, TryUnwrap, Unwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
#[non_exhaustive]
pub enum FixedUnicodeEscapeError<Char = char, O = usize> {
  /// An incomplete fixed-width unicode escape sequence.
  ///
  /// This occurs when the escape has fewer than 4 hex digits, typically
  /// due to unexpected end-of-input or a non-hex character.
  Incomplete(IncompleteFixedUnicodeEscape<O>),

  /// A malformed fixed-width unicode escape sequence.
  ///
  /// This occurs when 4 characters follow `\u` but they are not all
  /// valid hexadecimal digits.
  Malformed(MalformedFixedUnicodeEscape<Char, O>),

  /// An unpaired surrogate in a fixed-width unicode escape sequence.
  ///
  /// This occurs when a surrogate value (U+D800..U+DFFF) appears without
  /// its required pair.
  UnpairedSurrogate(UnexpectedLexeme<Char, UnpairedSurrogateHint, O>),
}

impl<Char, O> core::fmt::Display for FixedUnicodeEscapeError<Char, O>
where
  Char: DisplayHuman + CharLen,
  O: core::fmt::Display + Clone + Ord,
  for<'a> &'a O: Add<usize, Output = O>,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Incomplete(err) => err.fmt(f),
      Self::Malformed(err) => err.fmt(f),
      Self::UnpairedSurrogate(err) => match err.hint() {
        UnpairedSurrogateHint::High => write!(
          f,
          "unpaired high surrogate in fixed-width unicode escape at {}",
          err.span(),
        ),
        UnpairedSurrogateHint::Low => write!(
          f,
          "unpaired low surrogate in fixed-width unicode escape at {}",
          err.span()
        ),
      },
    }
  }
}

impl<Char, O> core::error::Error for FixedUnicodeEscapeError<Char, O>
where
  Char: DisplayHuman + CharLen + core::fmt::Debug + 'static,
  O: core::fmt::Display + core::fmt::Debug + 'static + Clone + Ord,
  for<'a> &'a O: Add<usize, Output = O>,
{
  fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
    match self {
      Self::Incomplete(_) => None,
      Self::Malformed(err) => Some(err),
      Self::UnpairedSurrogate(err) => Some(err),
    }
  }
}

impl<Char, O> FixedUnicodeEscapeError<Char, O> {
  /// Creates an unpaired high surrogate error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::FixedUnicodeEscapeError;
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let error = FixedUnicodeEscapeError::<char>::unpaired_high_surrogate(
  ///     Lexeme::Range(SimpleSpan::new(10, 16))
  /// );
  /// assert!(error.is_unpaired_surrogate());
  /// ```
  #[inline]
  pub const fn unpaired_high_surrogate(lexeme: Lexeme<Char, O>) -> Self {
    Self::UnpairedSurrogate(UnexpectedLexeme::new(lexeme, UnpairedSurrogateHint::High))
  }

  /// Creates an unpaired low surrogate error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::FixedUnicodeEscapeError;
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let error = FixedUnicodeEscapeError::<char>::unpaired_low_surrogate(
  ///     Lexeme::Range(SimpleSpan::new(10, 16))
  /// );
  /// assert!(error.is_unpaired_surrogate());
  /// ```
  #[inline]
  pub const fn unpaired_low_surrogate(lexeme: Lexeme<Char, O>) -> Self {
    Self::UnpairedSurrogate(UnexpectedLexeme::new(lexeme, UnpairedSurrogateHint::Low))
  }

  /// Bumps the span or position of the error by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{FixedUnicodeEscapeError, IncompleteFixedUnicodeEscape};
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let mut error: FixedUnicodeEscapeError =
  ///     FixedUnicodeEscapeError::Incomplete(IncompleteFixedUnicodeEscape::new(SimpleSpan::new(10, 14)));
  /// error.bump(&5);
  /// // The span is now adjusted
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    match self {
      Self::Incomplete(lexeme) => {
        lexeme.bump(n);
      }
      Self::Malformed(seq) => {
        seq.bump(n);
      }
      Self::UnpairedSurrogate(lexeme) => {
        lexeme.bump(n);
      }
    }
    self
  }

  /// Returns the span of the error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{FixedUnicodeEscapeError, IncompleteFixedUnicodeEscape};
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let error: FixedUnicodeEscapeError =
  ///    FixedUnicodeEscapeError::Incomplete(IncompleteFixedUnicodeEscape::new(SimpleSpan::new(
  ///       10, 14)));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 14));
  /// ```
  #[inline]
  pub fn span(&self) -> SimpleSpan<O>
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    match self {
      Self::Incomplete(lexeme) => lexeme.span_ref().cloned(),
      Self::Malformed(seq) => seq.span_ref().cloned(),
      Self::UnpairedSurrogate(lexeme) => lexeme.span(),
    }
  }
}

/// An error encountered during lexing for unicode escape sequences.
///
/// This is the top-level error type for all unicode escape sequence failures.
/// It distinguishes between fixed-width (`\uXXXX`) and variable-length (`\u{...}`) formats.
///
/// # Variants
///
/// - **Fixed**: An error in a fixed-width unicode escape sequence (`\uXXXX`)
/// - **Variable**: An error in a variable-length unicode escape sequence (`\u{...}`)
///
/// # Examples
///
/// ## Fixed-Width Escape Errors
///
/// ```
/// use tokora::error::UnicodeEscapeError;
/// use tokora::{SimpleSpan, utils::{Lexeme}};
///
/// // Incomplete fixed-width escape: \uAB
/// let error = UnicodeEscapeError::<char>::incomplete_fixed_unicode_escape(
///     SimpleSpan::new(10, 14)
/// );
/// assert!(error.is_fixed());
///
/// // Unpaired high surrogate: \uD800
/// let error = UnicodeEscapeError::<char>::unpaired_high_surrogate(
///     Lexeme::Range(SimpleSpan::new(5, 11))
/// );
/// assert!(error.is_fixed());
/// ```
///
/// ## Variable-Length Escape Errors
///
/// ```
/// use tokora::error::UnicodeEscapeError;
/// use tokora::SimpleSpan;
///
/// // Empty braces: \u{}
/// let error = UnicodeEscapeError::<char>::empty_variable_unicode_escape(
///     SimpleSpan::new(10, 14)
/// );
/// assert!(error.is_variable());
///
/// // Too many digits: \u{1234567}
/// let error = UnicodeEscapeError::<char>::too_many_digits_in_variable_unicode_escape(
///     SimpleSpan::new(5, 16),
///     7
/// );
/// assert!(error.is_variable());
///
/// // Surrogate value: \u{D800}
/// let error = UnicodeEscapeError::<char>::surrogate_variable_unicode_escape(
///     SimpleSpan::new(10, 18),
///     0xD800
/// );
/// assert!(error.is_variable());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, From, IsVariant, TryUnwrap, Unwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
#[non_exhaustive]
pub enum UnicodeEscapeError<Char = char, O = usize> {
  /// An error in a fixed-width unicode escape sequence (`\uXXXX`).
  Fixed(FixedUnicodeEscapeError<Char, O>),
  /// An error in a variable-length unicode escape sequence (`\u{...}`).
  Variable(VariableUnicodeEscapeError<Char, O>),
}

impl<Char, O> core::fmt::Display for UnicodeEscapeError<Char, O>
where
  Char: DisplayHuman + CharLen,
  O: core::fmt::Display + Clone + Ord,
  for<'a> &'a O: Add<usize, Output = O>,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Fixed(err) => err.fmt(f),
      Self::Variable(err) => err.fmt(f),
    }
  }
}

impl<Char, O> core::error::Error for UnicodeEscapeError<Char, O>
where
  Char: DisplayHuman + CharLen + core::fmt::Debug + 'static,
  O: core::fmt::Display + core::fmt::Debug + Clone + Ord + 'static,
  for<'a> &'a O: Add<usize, Output = O>,
{
  fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
    match self {
      Self::Fixed(err) => Some(err),
      Self::Variable(err) => Some(err),
    }
  }
}

impl<Char, O> UnicodeEscapeError<Char, O> {
  /// Creates an unpaired high surrogate error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let error = UnicodeEscapeError::<char>::unpaired_high_surrogate(
  ///     Lexeme::Range(SimpleSpan::new(10, 16))
  /// );
  /// assert!(error.is_fixed());
  /// ```
  #[inline]
  pub const fn unpaired_high_surrogate(lexeme: Lexeme<Char, O>) -> Self {
    Self::Fixed(FixedUnicodeEscapeError::unpaired_high_surrogate(lexeme))
  }

  /// Creates an unpaired low surrogate error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let error = UnicodeEscapeError::<char>::unpaired_low_surrogate(
  ///     Lexeme::Range(SimpleSpan::new(10, 16))
  /// );
  /// assert!(error.is_fixed());
  /// ```
  #[inline]
  pub const fn unpaired_low_surrogate(lexeme: Lexeme<Char, O>) -> Self {
    Self::Fixed(FixedUnicodeEscapeError::unpaired_low_surrogate(lexeme))
  }

  /// Creates an incomplete fixed-width unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error = UnicodeEscapeError::<char>::incomplete_fixed_unicode_escape(
  ///     SimpleSpan::new(10, 14)
  /// );
  /// assert!(error.is_fixed());
  /// ```
  #[inline]
  pub const fn incomplete_fixed_unicode_escape(span: SimpleSpan<O>) -> Self {
    Self::Fixed(FixedUnicodeEscapeError::Incomplete(
      IncompleteFixedUnicodeEscape::new(span),
    ))
  }

  /// Creates a malformed fixed-width unicode escape sequence error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{UnicodeEscapeError, InvalidFixedUnicodeHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let digits = InvalidFixedUnicodeHexDigits::from(
  ///     PositionedChar::with_position('G', 12)
  /// );
  /// let error = UnicodeEscapeError::malformed_fixed_unicode_escape(
  ///     digits,
  ///     SimpleSpan::new(10, 16)
  /// );
  /// assert!(error.is_fixed());
  /// ```
  #[inline]
  pub const fn malformed_fixed_unicode_escape(
    digits: InvalidFixedUnicodeHexDigits<Char, O>,
    span: SimpleSpan<O>,
  ) -> Self {
    Self::Fixed(FixedUnicodeEscapeError::Malformed(
      MalformedFixedUnicodeEscape::new(digits, span),
    ))
  }

  /// Creates an empty variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error = UnicodeEscapeError::<char>::empty_variable_unicode_escape(
  ///     SimpleSpan::new(10, 14)
  /// );
  /// assert!(error.is_variable());
  /// ```
  #[inline]
  pub const fn empty_variable_unicode_escape(span: SimpleSpan<O>) -> Self {
    Self::Variable(VariableUnicodeEscapeError::empty(span))
  }

  /// Creates a too many digits in variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error = UnicodeEscapeError::<char>::too_many_digits_in_variable_unicode_escape(
  ///     SimpleSpan::new(5, 16),
  ///     7
  /// );
  /// assert!(error.is_variable());
  /// ```
  #[inline]
  pub const fn too_many_digits_in_variable_unicode_escape(
    span: SimpleSpan<O>,
    count: usize,
  ) -> Self {
    Self::Variable(VariableUnicodeEscapeError::too_many_digits(span, count))
  }

  /// Creates an unclosed brace in variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error = UnicodeEscapeError::<char>::unclosed_variable_unicode_escape(
  ///     SimpleSpan::new(10, 15)
  /// );
  /// assert!(error.is_variable());
  /// ```
  #[inline]
  pub const fn unclosed_variable_unicode_escape(span: SimpleSpan<O>) -> Self {
    Self::Variable(VariableUnicodeEscapeError::unclosed(span))
  }

  /// Creates a surrogate in variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error = UnicodeEscapeError::<char>::surrogate_variable_unicode_escape(
  ///     SimpleSpan::new(10, 18),
  ///     0xD800
  /// );
  /// assert!(error.is_variable());
  /// ```
  #[inline]
  pub const fn surrogate_variable_unicode_escape(span: SimpleSpan<O>, codepoint: u32) -> Self {
    Self::Variable(VariableUnicodeEscapeError::surrogate(span, codepoint))
  }

  /// Creates an overflow in variable-length unicode escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error = UnicodeEscapeError::<char>::overflow_variable_unicode_escape(
  ///     SimpleSpan::new(10, 20),
  ///     0x110000
  /// );
  /// assert!(error.is_variable());
  /// ```
  #[inline]
  pub const fn overflow_variable_unicode_escape(span: SimpleSpan<O>, codepoint: u32) -> Self {
    Self::Variable(VariableUnicodeEscapeError::overflow(span, codepoint))
  }

  /// Creates a malformed variable-length unicode escape error from a character.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  ///
  /// let error = UnicodeEscapeError::<char>::invalid_variable_unicode_escape_char(12, 'G');
  /// assert!(error.is_variable());
  /// ```
  #[inline]
  pub const fn invalid_variable_unicode_escape_char(pos: O, ch: Char) -> Self {
    Self::Variable(VariableUnicodeEscapeError::Malformed(
      MalformedVariableUnicodeSequence::from_char(pos, ch),
    ))
  }

  /// Creates a malformed variable-length unicode escape error from a span.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error: UnicodeEscapeError =
  ///     UnicodeEscapeError::<char>::invalid_variable_unicode_escape_sequence(
  ///         SimpleSpan::new(10, 15)
  ///     );
  /// assert!(error.is_variable());
  /// ```
  #[inline]
  pub const fn invalid_variable_unicode_escape_sequence(span: SimpleSpan<O>) -> Self {
    Self::Variable(VariableUnicodeEscapeError::Malformed(
      MalformedVariableUnicodeSequence::from_range(span),
    ))
  }

  /// Bumps the span or position of the error by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::UnicodeEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = UnicodeEscapeError::<char>::empty_variable_unicode_escape(
  ///     SimpleSpan::new(10, 14)
  /// );
  /// error.bump(&5);
  /// // The span is now adjusted by 5
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone + Ord + core::hash::Hash,
  {
    match self {
      Self::Fixed(err) => {
        err.bump(n);
      }
      Self::Variable(err) => {
        err.bump(n);
      }
    }
    self
  }

  /// Returns the span of the error.
  ///
  /// ## Examples
  ///
  /// ```
  ///
  /// use tokora::error::UnicodeEscapeError;
  ///
  /// use tokora::SimpleSpan;
  ///
  /// let error = UnicodeEscapeError::<char>::empty_variable_unicode_escape(
  ///    SimpleSpan::new(10, 14)
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(10, 14));
  /// ```
  #[inline]
  pub fn span(&self) -> SimpleSpan<O>
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    match self {
      Self::Fixed(err) => err.span(),
      Self::Variable(err) => err.span(),
    }
  }
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
