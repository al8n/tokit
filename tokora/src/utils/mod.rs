pub use delimited::*;
pub use downcast::*;
pub use escaped::*;
pub use expected::*;
pub use generic_arraydeque::GenericArrayDeque;
pub use lexeme::*;

pub use mayber::{Maybe, MaybeMut, MaybeRef, Owned, Ref};
pub use message::CowStr;
pub use oneof::OneOf;
pub use positioned_char::*;

pub use to_equivalent::*;

/// Re-export of generic-arraydeque for direct access.
pub use generic_arraydeque::{self, typenum};

/// A module for custom comparing traits.
pub mod cmp;
/// A module for displaying in a human-friendly way.
pub mod human_display;
/// A module for displaying in SDL.
pub mod sdl_display;
/// A module for displaying in syntax trees.
pub mod syntax_tree_display;

/// Common knowledge types for lexing and parsing.
pub mod knowledge;

/// Marker types used in various utilities.
pub mod marker;

mod delimited;
mod downcast;
mod escaped;
mod expected;
mod lexeme;
mod message;
mod oneof;
mod positioned_char;
mod to_equivalent;

/// Enables destructuring a parsed element into its constituent components.
///
/// This trait provides a way to break down complex parsed elements into their
/// individual parts, taking ownership of each component. This is particularly
/// useful for transformation, analysis, or when building different representations
/// of the parsed data.
///
/// ## Design Philosophy
///
/// The trait uses an associated type rather than generic parameters to ensure
/// that each implementing type has exactly one way to be decomposed. This provides
/// type safety and makes the interface predictable for consumers.
///
/// ## Usage Patterns
///
/// Common scenarios for using this trait:
/// - **AST transformation**: Converting parsed elements into different AST representations
/// - **Analysis**: Extracting specific components for validation or processing
/// - **Serialization**: Breaking down elements for custom serialization formats
/// - **Testing**: Accessing individual components for detailed assertions
///
/// ## Examples
///
/// ```rust,ignore
/// // Extracting components for transformation
/// let float_value: FloatValue<&str, SimpleSpan> = parse_float("3.14e-2")?;
/// let (span, int_part, frac_part, exp_part) = float_value.into_components();
///
/// // Building a custom representation
/// let custom_float = CustomFloat {
///     location: span,
///     integer: int_part,
///     fractional: frac_part,
///     exponent: exp_part,
/// };
///
/// // Component analysis
/// let int_literal: IntValue<&str, SimpleSpan> = parse_int("-42")?;
/// let (span, sign, digits) = int_literal.into_components();
///
/// if sign.is_some() {
///     println!("Found negative integer at {:?}", span);
/// }
/// ```
///
/// ## Implementation Guidelines
///
/// When implementing this trait:
/// - Include all meaningful components of the parsed element
/// - Order components logically (typically: span first, then sub-components in source order)
/// - Use tuples for simple decomposition, custom structs for complex cases
/// - Ensure the decomposition is complete (no information loss)
/// - Document the component structure clearly
///
/// ## Component Ordering Convention
///
/// To maintain consistency across implementations, follow this ordering:
/// 1. **Overall span**: The span covering the entire element
/// 2. **Required components**: Core parts that are always present
/// 3. **Optional components**: Parts that may or may not be present
/// 4. **Sub-elements**: Nested parsed elements in source order
pub trait IntoComponents {
  /// The tuple or struct type containing the decomposed components.
  ///
  /// This associated type defines the structure returned by `into_components()`.
  /// It should include all meaningful parts of the parsed element in a logical
  /// order that makes sense for the specific element type.
  type Components;

  /// Consumes this element and returns its constituent components.
  ///
  /// This method breaks down the parsed element into its individual parts,
  /// providing owned access to each component. The exact structure of the
  /// returned components is defined by the `Components` associated type.
  fn into_components(self) -> Self::Components;
}

/// A trait for checking if a token is an ASCII character.
pub trait IsAsciiChar {
  /// Returns `true` if self is equal to the given ASCII character.
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool;

  /// Checks if the value is an ASCII decimal digit:
  /// U+0030 '0' ..= U+0039 '9'.
  fn is_ascii_digit(&self) -> bool;

  /// Returns `true` if self is one of the given ASCII characters.
  #[inline(always)]
  fn one_of(&self, choices: &[ascii::AsciiChar]) -> bool {
    choices.iter().any(|&ch| self.is_ascii_char(ch))
  }
}

impl<T> IsAsciiChar for &T
where
  T: IsAsciiChar + ?Sized,
{
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    <T as IsAsciiChar>::is_ascii_char(*self, ch)
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    <T as IsAsciiChar>::is_ascii_digit(*self)
  }

  #[inline(always)]
  fn one_of(&self, choices: &[ascii::AsciiChar]) -> bool {
    <T as IsAsciiChar>::one_of(*self, choices)
  }
}

impl<T> IsAsciiChar for &mut T
where
  T: IsAsciiChar + ?Sized,
{
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    <T as IsAsciiChar>::is_ascii_char(*self, ch)
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    <T as IsAsciiChar>::is_ascii_digit(*self)
  }

  #[inline(always)]
  fn one_of(&self, choices: &[ascii::AsciiChar]) -> bool {
    <T as IsAsciiChar>::one_of(*self, choices)
  }
}

impl IsAsciiChar for char {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    if self.is_ascii() {
      *self as u8 == ch as u8
    } else {
      false
    }
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    char::is_ascii_digit(self)
  }
}

impl IsAsciiChar for u8 {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    *self == ch as u8
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    u8::is_ascii_digit(self)
  }
}

impl IsAsciiChar for str {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    self.len() == 1 && self.as_bytes()[0] == ch as u8
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    self.len() == 1 && self.as_bytes()[0].is_ascii_digit()
  }
}

impl IsAsciiChar for [u8] {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    self.len() == 1 && self[0] == ch as u8
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    self.len() == 1 && self[0].is_ascii_digit()
  }
}

#[cfg(feature = "bstr_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bstr_1")))]
impl IsAsciiChar for bstr_1::BStr {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    <[u8] as IsAsciiChar>::is_ascii_char(self, ch)
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    <[u8] as IsAsciiChar>::is_ascii_digit(self)
  }
}

#[cfg(feature = "bytes_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
impl IsAsciiChar for bytes_1::Bytes {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    <[u8] as IsAsciiChar>::is_ascii_char(self, ch)
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    <[u8] as IsAsciiChar>::is_ascii_digit(self)
  }
}

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
impl IsAsciiChar for hipstr_0_8::HipByt<'_> {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    <[u8] as IsAsciiChar>::is_ascii_char(self, ch)
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    <[u8] as IsAsciiChar>::is_ascii_digit(self)
  }
}

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
impl IsAsciiChar for hipstr_0_8::HipStr<'_> {
  #[inline(always)]
  fn is_ascii_char(&self, ch: ascii::AsciiChar) -> bool {
    <str as IsAsciiChar>::is_ascii_char(self, ch)
  }

  #[inline(always)]
  fn is_ascii_digit(&self) -> bool {
    <str as IsAsciiChar>::is_ascii_digit(self)
  }
}

/// A trait for character-like types that can report their encoded length in bytes.
///
/// `CharLen` provides a uniform way to query the byte length of different character
/// types, which is essential for converting positioned characters into byte spans.
///
/// # Implementations
///
/// tokora provides implementations for:
/// - **`u8`**: Always returns `1` (single byte)
/// - **`char`**: Returns `len_utf8()` (1-4 bytes depending on the character)
/// - **`&T`**: Delegates to `T::len()` for any `T: CharLen`
///
/// # Design Note
///
/// This trait is **sealed** and cannot be implemented outside of tokora. If you need
/// to work with a custom character type, use [`Lexeme::span_with`] or
/// [`UnknownLexeme::from_range`](crate::error::UnknownLexeme::from_range) and provide your own length function.
///
/// # Use Cases
///
/// - **Span calculation**: Convert positioned characters to byte spans automatically
/// - **UTF-8 handling**: Properly account for multi-byte characters
/// - **Error reporting**: Determine the exact byte range of an unexpected character
///
/// # Examples
///
/// ## Automatic Length Detection
///
/// ```rust
/// use tokora::utils::{Lexeme, PositionedChar};
///
/// // ASCII character (1 byte)
/// let ascii = Lexeme::from(PositionedChar::with_position('a', 10));
/// let span = ascii.span();
/// assert_eq!(span.len(), 1);
///
/// // Multi-byte UTF-8 character (3 bytes)
/// let emoji = Lexeme::from(PositionedChar::with_position('€', 20));
/// let span = emoji.span();
/// assert_eq!(span.len(), 3);
/// ```
///
/// ## With Custom Length Function
///
/// ```rust
/// use tokora::utils::{Lexeme, PositionedChar};
///
/// // For types that don't implement CharLen, use span_with
/// struct CustomChar(char);
///
/// let lexeme = Lexeme::from(PositionedChar::with_position(CustomChar('€'), 5));
/// let span = lexeme.span_with(|c| c.0.len_utf8());
///
/// assert_eq!(span.start(), 5);
/// assert_eq!(span.end(), 8);
/// ```
#[allow(clippy::len_without_is_empty)]
pub trait CharLen: sealed::Sealed {
  /// Returns the length of this character in bytes.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::utils::{Lexeme, PositionedChar};
  ///
  /// // The trait is used internally by span()
  /// let ascii = Lexeme::from(PositionedChar::with_position('A', 0));
  /// assert_eq!(ascii.span().len(), 1);
  ///
  /// let euro = Lexeme::from(PositionedChar::with_position('€', 0));
  /// assert_eq!(euro.span().len(), 3);
  ///
  /// let crab = Lexeme::from(PositionedChar::with_position('🦀', 0));
  /// assert_eq!(crab.span().len(), 4);
  /// ```
  fn char_len(&self) -> usize;
}

mod sealed {
  use super::{CharLen, PositionedChar};

  pub trait Sealed {}

  impl Sealed for u8 {}
  impl Sealed for char {}
  impl<T: Sealed> Sealed for PositionedChar<T> {}

  impl<T: Sealed> Sealed for &T {}

  impl CharLen for u8 {
    #[inline(always)]
    fn char_len(&self) -> usize {
      1
    }
  }

  impl CharLen for char {
    #[inline(always)]
    fn char_len(&self) -> usize {
      self.len_utf8()
    }
  }

  impl<T: CharLen> CharLen for PositionedChar<T> {
    #[inline(always)]
    fn char_len(&self) -> usize {
      self.char_ref().char_len()
    }
  }

  impl<T: CharLen> CharLen for &T {
    #[inline(always)]
    fn char_len(&self) -> usize {
      (*self).char_len()
    }
  }
}

#[cfg(test)]
mod tests;
