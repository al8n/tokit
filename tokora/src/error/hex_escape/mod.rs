//! Hexadecimal escape sequence error types for lexer error reporting.
//!
//! This module provides error types for handling failures in hexadecimal
//! escape sequences during lexical analysis. It supports the `\xXX` format,
//! which requires exactly 2 hexadecimal digits.
//!
//! # Design Philosophy
//!
//! Hexadecimal escape sequences can fail in two ways:
//! - **Syntax errors**: Invalid hex digits, wrong format
//! - **Incompleteness**: Reaching end-of-input mid-escape (fewer than 2 digits)
//!
//! This module distinguishes between:
//! - **Malformed** syntax (invalid hex digits)
//! - **Incomplete** sequences (unexpected EOF or non-hex character)
//!
//! # Hex Escape Format: `\xXX`
//!
//! Hex escapes require exactly 2 hexadecimal digits and can encode:
//! - Any byte value: `\x00` to `\xFF`
//! - Common examples: `\x0A` (newline), `\x09` (tab), `\x20` (space)
//!
//! Common errors:
//! - `\x` - incomplete (no digits)
//! - `\xA` - incomplete (only 1 digit)
//! - `\xGG` - malformed (invalid hex)
//! - `\xZ9` - malformed (first digit invalid)
//!
//! # Error Type Hierarchy
//!
//! ```text
//! HexEscapeError
//! ├─ Incomplete (IncompleteHexEscape)
//! └─ Malformed (MalformedHexEscape)
//! ```
//!
//! # Examples
//!
//! ## Detecting Incomplete Hex Escapes
//!
//! ```
//! use tokora::error::HexEscapeError;
//! use tokora::{SimpleSpan, utils::{Lexeme}};
//!
//! // Incomplete: \xA (only 1 digit)
//! let error = HexEscapeError::<char>::incomplete(
//!     SimpleSpan::new(10, 13) // \xA
//! );
//! assert!(error.is_incomplete());
//! ```
//!
//! ## Detecting Malformed Hex Escapes
//!
//! ```
//! use tokora::error::{HexEscapeError, InvalidHexDigits};
//! use tokora::{SimpleSpan, utils::{PositionedChar}};
//!
//! // Invalid hex digit 'G' at position 12
//! let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
//! digits.push(PositionedChar::with_position('G', 13));
//!
//! let error = HexEscapeError::malformed(
//!     digits,
//!     SimpleSpan::new(10, 14) // \xGG
//! );
//! assert!(error.is_malformed());
//! ```

use core::ops::AddAssign;

use crate::{error::InvalidHexDigits, span::SimpleSpan, utils::human_display::DisplayHuman};
use derive_more::{From, IsVariant, TryUnwrap, Unwrap};

/// A type alias for invalid hex digits in hex escape sequences.
pub type InvalidHexEscapeDigits<Char, Offset> = InvalidHexDigits<Char, 2, Offset>;

/// An incomplete hex escape sequence error.
///
/// This error occurs when a hex escape (`\xXX`) has fewer than 2 hex digits,
/// typically due to unexpected end-of-input or a non-hex character.
///
/// # Examples
///
/// ```
/// use tokora::error::IncompleteHexEscape;
/// use tokora::SimpleSpan;
///
/// // Incomplete: \xA (only 1 hex digit)
/// let error = IncompleteHexEscape::new(
///     SimpleSpan::new(10, 13)
/// );
/// assert_eq!(error.span(), SimpleSpan::new(10, 13));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error(
  "incomplete hexadecimal escape sequence at {0}, hexadecimal escape must contains exactly two hexadecimal digits"
)]
pub struct IncompleteHexEscape<O = usize>(SimpleSpan<O>);

impl<O> IncompleteHexEscape<O> {
  /// Creates a new incomplete hex escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::IncompleteHexEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = IncompleteHexEscape::new(SimpleSpan::new(10, 12));
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
  /// use tokora::error::IncompleteHexEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = IncompleteHexEscape::new(SimpleSpan::new(10, 13));
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
  /// use tokora::error::IncompleteHexEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let error = IncompleteHexEscape::new(SimpleSpan::new(10, 13));
  /// assert_eq!(error.span_ref(), SimpleSpan::new(&10, &13));
  /// ```
  #[inline]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.0.as_ref()
  }

  /// Bumps the span or position by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::IncompleteHexEscape;
  /// use tokora::SimpleSpan;
  ///
  /// let mut error = IncompleteHexEscape::new(SimpleSpan::new(10, 12));
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

/// A malformed hex escape sequence error.
///
/// This error occurs when a hex escape (`\xXX`) contains invalid hexadecimal
/// digits. The error captures both the invalid characters encountered and the
/// span of the malformed escape sequence.
///
/// # Type Parameters
///
/// * `Char` - The character type (typically `char` for UTF-8 or `u8` for bytes)
///
/// # Examples
///
/// ```
/// use tokora::error::{MalformedHexEscape, InvalidHexDigits};
/// use tokora::{SimpleSpan, utils::{PositionedChar}};
///
/// // Create error for malformed escape like \xGH
/// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
/// digits.push(PositionedChar::with_position('H', 13));
///
/// let error = MalformedHexEscape::new(
///     digits,
///     SimpleSpan::new(10, 14) // \xGH
/// );
///
/// assert_eq!(error.span(), SimpleSpan::new(10, 14));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MalformedHexEscape<Char = char, O = usize> {
  digits: InvalidHexEscapeDigits<Char, O>,
  span: SimpleSpan<O>,
}

impl<Char, O> core::fmt::Display for MalformedHexEscape<Char, O>
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

impl<Char, O> core::error::Error for MalformedHexEscape<Char, O>
where
  Char: DisplayHuman + core::fmt::Debug,
  O: core::fmt::Debug + core::fmt::Display,
{
}

impl<Char, O> MalformedHexEscape<Char, O> {
  /// Creates a new malformed hex escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedHexEscape, InvalidHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('Z', 12));
  /// let error = MalformedHexEscape::new(digits, SimpleSpan::new(10, 13));
  /// ```
  #[inline]
  pub const fn new(digits: InvalidHexEscapeDigits<Char, O>, span: SimpleSpan<O>) -> Self {
    Self { digits, span }
  }

  /// Returns the invalid hex digits.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedHexEscape, InvalidHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
  /// let error = MalformedHexEscape::new(digits, SimpleSpan::new(10, 13));
  /// assert_eq!(error.digits().len(), 1);
  /// ```
  #[inline]
  pub fn digits(&self) -> InvalidHexEscapeDigits<Char, O>
  where
    Char: Clone,
    O: Clone,
  {
    self.digits.clone()
  }

  /// Returns a reference to the invalid hex digits.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedHexEscape, InvalidHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(12, 'G');
  /// let error = MalformedHexEscape::new(digits, SimpleSpan::new(10, 13));
  /// assert_eq!(error.digits_ref().len(), 1);
  /// ```
  #[inline]
  pub const fn digits_ref(&self) -> &InvalidHexEscapeDigits<Char, O> {
    &self.digits
  }

  /// Returns a mutable reference to the invalid hex digits.
  #[inline]
  pub fn digits_mut(&mut self) -> &mut InvalidHexEscapeDigits<Char, O> {
    &mut self.digits
  }

  /// Returns the span of the malformed hex escape.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{MalformedHexEscape, InvalidHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let error = MalformedHexEscape::new(
  ///     InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12)),
  ///     SimpleSpan::new(10, 14)
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(10, 14));
  /// ```
  #[inline]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.span
  }

  /// Returns a reference to the span of the malformed hex escape.
  #[inline]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.span.as_ref()
  }

  /// Returns a mutable reference to the span of the malformed hex escape.
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
  /// use tokora::error::{MalformedHexEscape, InvalidHexDigits};
  /// use tokora::{SimpleSpan, utils::{PositionedChar}};
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
  /// let mut error = MalformedHexEscape::new(digits, SimpleSpan::new(10, 14));
  /// error.bump(&5);
  /// assert_eq!(error.span(), SimpleSpan::new(15, 19));
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.span.bump(n);
    self.digits.bump(n);
    self
  }
}

/// An error encountered during lexing for `\xXX` hex escape sequences.
///
/// Hex escapes require exactly 2 hexadecimal digits after `\x`.
/// They can encode any byte value from 0x00 to 0xFF.
///
/// # Variants
///
/// - **Incomplete**: The escape has fewer than 2 hex digits, e.g., `\x`, `\xA`
/// - **Malformed**: The 2 characters are not valid hexadecimal, e.g., `\xGG`, `\xZ9`
///
/// # Examples
///
/// ## Incomplete Escape
///
/// ```
/// use tokora::error::HexEscapeError;
/// use tokora::SimpleSpan;
///
/// // Incomplete: \xA (only 1 digit)
/// let error = HexEscapeError::<char>::incomplete(
///     SimpleSpan::new(10, 13)
/// );
/// assert!(error.is_incomplete());
/// ```
///
/// ## Malformed Escape
///
/// ```
/// use tokora::error::{HexEscapeError, InvalidHexDigits};
/// use tokora::{SimpleSpan, utils::{PositionedChar}};
///
/// // Invalid hex: \xGG
/// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
/// digits.push(PositionedChar::with_position('G', 13));
///
/// let error = HexEscapeError::malformed(digits, SimpleSpan::new(10, 14));
/// assert!(error.is_malformed());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, From, IsVariant, TryUnwrap, Unwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
#[non_exhaustive]
pub enum HexEscapeError<Char = char, O = usize> {
  /// An incomplete hex escape sequence.
  ///
  /// This occurs when the escape has fewer than 2 hex digits, typically
  /// due to unexpected end-of-input or a non-hex character.
  ///
  /// Examples: `\x`, `\xA`
  Incomplete(IncompleteHexEscape<O>),

  /// A malformed hex escape sequence.
  ///
  /// This occurs when 2 characters follow `\x` but they are not both
  /// valid hexadecimal digits.
  ///
  /// Examples: `\xGG`, `\xZ9`, `\xAZ`
  Malformed(MalformedHexEscape<Char, O>),
}

impl<Char, O> core::fmt::Display for HexEscapeError<Char, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Incomplete(err) => err.fmt(f),
      Self::Malformed(malformed) => malformed.fmt(f),
    }
  }
}

impl<Char, O> core::error::Error for HexEscapeError<Char, O>
where
  Char: DisplayHuman + core::fmt::Debug + 'static,
  O: core::fmt::Debug + core::fmt::Display + 'static,
{
  fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
    match self {
      Self::Incomplete(err) => Some(err),
      Self::Malformed(err) => Some(err),
    }
  }
}

impl<Char, O> HexEscapeError<Char, O> {
  /// Creates an incomplete hex escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::HexEscapeError;
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let error = HexEscapeError::<char>::incomplete(
  ///     SimpleSpan::new(10, 12)
  /// );
  /// assert!(error.is_incomplete());
  /// ```
  #[inline]
  pub const fn incomplete(span: SimpleSpan<O>) -> Self {
    Self::Incomplete(IncompleteHexEscape::new(span))
  }

  /// Creates a malformed hex escape error.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::{HexEscapeError, InvalidHexDigits};
  /// use tokora::SimpleSpan;
  ///
  /// let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(12, 'G');
  /// let error = HexEscapeError::malformed(digits, SimpleSpan::new(10, 13));
  /// assert!(error.is_malformed());
  /// ```
  #[inline]
  pub const fn malformed(digits: InvalidHexEscapeDigits<Char, O>, span: SimpleSpan<O>) -> Self {
    Self::Malformed(MalformedHexEscape::new(digits, span))
  }

  /// Returns the span of the hex escape error.
  ///
  /// This returns the span where the error occurred, which could be either
  /// the incomplete sequence or the malformed escape sequence.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::HexEscapeError;
  /// use tokora::SimpleSpan;
  ///
  /// let error = HexEscapeError::<char>::incomplete(SimpleSpan::new(10, 12));
  /// assert_eq!(error.span(), SimpleSpan::new(10, 12));
  /// ```
  #[inline]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    match self {
      Self::Incomplete(incomplete) => incomplete.span(),
      Self::Malformed(malformed) => malformed.span(),
    }
  }

  /// Bumps the span or position of the error by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::error::HexEscapeError;
  /// use tokora::{SimpleSpan, utils::{Lexeme}};
  ///
  /// let mut error = HexEscapeError::<char>::incomplete(
  ///     SimpleSpan::new(10, 12)
  /// );
  /// error.bump(&5);
  /// // The span is now adjusted
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    match self {
      Self::Incomplete(incomplete) => {
        incomplete.bump(n);
      }
      Self::Malformed(malformed) => {
        malformed.bump(n);
      }
    }
    self
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
