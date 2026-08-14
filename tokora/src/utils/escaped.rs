//! Escape sequence representation types for lexical analysis.
//!
//! This module provides types for representing escape sequences encountered during
//! lexing. These are **not error types** - they represent successfully recognized
//! escape sequence structures that may later be validated or transformed.
//!
//! # Design Philosophy
//!
//! Escape sequences in source code (like `\n`, `\t`, `\xFF`) consist of:
//! - A **backslash prefix** (`\`)
//! - The **escape content** (the character(s) after the backslash)
//!
//! These types capture both parts to enable:
//! - Precise error reporting with correct spans
//! - Distinguishing between the escape syntax and its semantic meaning
//! - Supporting various escape formats (single-char, hex, unicode, etc.)
//!
//! # Escape Sequence Types
//!
//! ## Single-Character Escapes: `\n`, `\t`, `\r`
//!
//! Use [`SingleCharEscape`] for escapes with a single character after the backslash:
//! - Simple escapes: `\n`, `\t`, `\r`, `\\`, `\'`, `\"`
//! - Prefix for longer escapes: `\x`, `\u` (before parsing the rest)
//!
//! ## Multi-Character Escapes: `\xFF`, `\u1234`
//!
//! Use [`MultiCharEscape`] for escapes with multiple characters after the backslash:
//! - Hex escapes: `\xAB` (the sequence "xAB")
//! - Unicode escapes: `\u1234`, `\u{10FFFF}` (the sequence after `\u`)
//!
//! ## Generic Escape Representation
//!
//! Use [`EscapedLexeme`] for a unified representation that can hold either type:
//! - Wraps a [`Lexeme`] (Char or SimpleSpan)
//! - Includes the full span of the escape sequence
//!
//! # Examples
//!
//! ## Representing a Simple Escape
//!
//! ```
//! use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
//!
//! // For the escape sequence `\n` at position 10-12:
//! let escape = SingleCharEscape::from_positioned_char(
//!     SimpleSpan::new(10, 12),                      // Covers both '\' and 'n'
//!     PositionedChar::with_position('n', 11), // The 'n' at position 11
//! );
//!
//! assert_eq!(escape.char(), 'n');
//! assert_eq!(escape.position(), 11);
//! assert_eq!(escape.span(), SimpleSpan::new(10, 12));
//! ```
//!
//! ## Representing a Hex Escape
//!
//! ```
//! use tokora::{SimpleSpan, utils::MultiCharEscape};
//!
//! // For the escape sequence `\xFF` at position 5-9:
//! let escape = MultiCharEscape::new(
//!     SimpleSpan::new(6, 9),  // Just "xFF" (after the backslash)
//!     SimpleSpan::new(5, 9)   // Full escape including '\x'
//! );
//!
//! assert_eq!(escape.content(), SimpleSpan::new(6, 9));
//! assert_eq!(escape.span(), SimpleSpan::new(5, 9));
//! ```

use core::ops::AddAssign;

use crate::{
  span::SimpleSpan,
  utils::{Lexeme, human_display::DisplayHuman},
};

use super::PositionedChar;

/// A single-character escape sequence representation.
///
/// This type represents escape sequences with exactly one character after the
/// backslash, such as `\n`, `\t`, `\r`, `\\`, `\'`, `\"`.
///
/// # Structure
///
/// The type stores:
/// - **character**: The single character after the backslash (with its position)
/// - **span**: The full span covering both `\` and the character
///
/// For example, in `\n` at position 10-12:
/// - `character` would be `'n'` at position 11
/// - `span` would be 10..12 (covering both `\` and `n`)
///
/// # Type Parameters
///
/// * `Char` - The character type (typically `char` for UTF-8 or `u8` for bytes)
///
/// # Examples
///
/// ```
/// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
///
/// // Escape sequence `\n` at positions 10-12
/// let newline = SingleCharEscape::from_positioned_char(
///     SimpleSpan::new(10, 12),
///     PositionedChar::with_position('n', 11),
/// );
///
/// assert_eq!(newline.char(), 'n');
/// assert_eq!(newline.position(), 11);
/// assert_eq!(newline.span(), SimpleSpan::new(10, 12));
///
/// // Escape sequence `\t` at positions 20-22
/// let tab = SingleCharEscape::from_positioned_char(
///     SimpleSpan::new(20, 22),
///     PositionedChar::with_position('t', 21),
/// );
///
/// assert_eq!(tab.char(), 't');
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SingleCharEscape<Char = char, O = usize> {
  character: PositionedChar<Char, O>,
  span: SimpleSpan<O>,
}

impl<Char, O> core::fmt::Display for SingleCharEscape<Char, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "\\{} at {}", self.char_ref().display(), self.span)
  }
}

impl<Char, O> SingleCharEscape<Char, O> {
  /// Creates a new single-character escape sequence.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// let escape = SingleCharEscape::from_char(
  ///     SimpleSpan::new(14, 16),
  ///     15,
  ///    'r'
  /// );
  /// assert_eq!(escape.char(), 'r');
  /// ```
  #[inline(always)]
  pub fn from_char(span: SimpleSpan<O>, pos: O, ch: Char) -> Self {
    Self::from_positioned_char(span, PositionedChar::with_position(ch, pos))
  }

  /// Creates a new single-character escape sequence.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// let escape = SingleCharEscape::from_positioned_char(
  ///     SimpleSpan::new(14, 16),
  ///     PositionedChar::with_position('r', 15),
  /// );
  /// assert_eq!(escape.char(), 'r');
  /// ```
  #[inline(always)]
  pub const fn from_positioned_char(
    span: SimpleSpan<O>,
    character: PositionedChar<Char, O>,
  ) -> Self {
    Self { character, span }
  }

  /// Returns the character after the backslash.
  ///
  /// For example, for `\n`, this returns `'n'`.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// let escape = SingleCharEscape::from_positioned_char(
  ///     SimpleSpan::new(4, 6),
  ///     PositionedChar::with_position('n', 5),
  /// );
  /// assert_eq!(escape.char(), 'n');
  /// ```
  #[inline(always)]
  pub const fn char(&self) -> Char
  where
    Char: Copy,
  {
    self.character.char()
  }

  /// Returns a reference to the character after the backslash.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// let escape = SingleCharEscape::from_positioned_char(
  ///     SimpleSpan::new(9, 11),
  ///     PositionedChar::with_position('t', 10),
  /// );
  /// assert_eq!(*escape.char_ref(), 't');
  /// ```
  #[inline(always)]
  pub const fn char_ref(&self) -> &Char {
    self.character.char_ref()
  }

  /// Returns a mutable reference to the character after the backslash.
  #[inline(always)]
  pub const fn char_mut(&mut self) -> &mut Char {
    self.character.char_mut()
  }
  /// Returns the position of the character (not including the backslash).
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// // Escape `\n` at positions 10-12: '\' at 10, 'n' at 11
  /// let escape = SingleCharEscape::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('n', 11),
  /// );
  /// assert_eq!(escape.position(), 11); // Position of 'n', not '\'
  /// ```
  #[inline(always)]
  pub const fn position(&self) -> O
  where
    O: Copy,
  {
    self.character.position()
  }

  /// Returns the position of the character (not including the backslash).
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// // Escape `\n` at positions 10-12: '\' at 10, 'n' at 11
  /// let escape = SingleCharEscape::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('n', 11),
  /// );
  /// assert_eq!(escape.position_ref(), &11); // Position of 'n', not '\'
  /// ```
  #[inline(always)]
  pub const fn position_ref(&self) -> &O {
    self.character.position_ref()
  }

  /// Returns the span of the entire escape sequence.
  ///
  /// This includes both the backslash and the character.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// let escape = SingleCharEscape::from_positioned_char(
  ///     SimpleSpan::new(4, 6),
  ///     PositionedChar::with_position('r', 5),
  /// );
  /// assert_eq!(escape.span(), SimpleSpan::new(4, 6)); // Covers both '\' and 'r'
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.span
  }

  /// Returns a reference to the span.
  #[inline(always)]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.span.as_ref()
  }

  /// Returns a mutable reference to the span.
  #[inline(always)]
  pub const fn span_mut(&mut self) -> SimpleSpan<&mut O> {
    self.span.as_mut()
  }

  /// Bumps both the span and character position by `n`.
  ///
  /// This is useful when adjusting positions after processing or when
  /// combining escape sequences from different parsing contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{SingleCharEscape, PositionedChar}};
  ///
  /// let mut escape = SingleCharEscape::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('n', 11),
  /// );
  ///
  /// escape.bump(&5);
  ///
  /// assert_eq!(escape.position(), 16);      // Was 11, now 16
  /// assert_eq!(escape.span(), SimpleSpan::new(15, 17)); // Was 10-12, now 15-17
  /// ```
  #[inline]
  pub fn bump(&mut self, offset: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.span.bump(offset);
    self.character.bump_position(offset);
    self
  }
}

/// A multi-character escape sequence representation.
///
/// This type represents escape sequences with multiple characters after the
/// backslash, such as `\xFF` (hex escape) or `\u1234` (unicode escape).
///
/// # Structure
///
/// The type stores:
/// - **content**: A span covering just the characters after the backslash
/// - **span**: The full span covering `\` and all following characters
///
/// For example, in `\xFF` at position 5-9:
/// - `content` would be 6..9 (just "xFF")
/// - `span` would be 5..9 (covering `\`, `x`, `F`, `F`)
///
/// # Examples
///
/// ```
/// use tokora::{SimpleSpan, utils::MultiCharEscape};
///
/// // Hex escape `\xFF` at positions 5-9
/// let hex = MultiCharEscape::new(
///     SimpleSpan::new(6, 9),  // Just "xFF"
///     SimpleSpan::new(5, 9)   // Full escape including backslash
/// );
///
/// assert_eq!(hex.content(), SimpleSpan::new(6, 9));
/// assert_eq!(hex.span(), SimpleSpan::new(5, 9));
///
/// // Unicode escape `\u1234` at positions 10-16
/// let unicode = MultiCharEscape::new(
///     SimpleSpan::new(11, 16), // Just "u1234"
///     SimpleSpan::new(10, 16)  // Full escape
/// );
///
/// assert_eq!(unicode.content(), SimpleSpan::new(11, 16));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MultiCharEscape<S = SimpleSpan> {
  content: S,
  span: S,
}

impl<S> core::fmt::Display for MultiCharEscape<S>
where
  S: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "escape sequence at {}", self.span)
  }
}

impl<S> MultiCharEscape<S> {
  /// Creates a new multi-character escape sequence.
  ///
  /// ## Parameters
  ///
  /// - `content`: SimpleSpan of the characters after the backslash
  /// - `span`: Full span including the backslash
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::MultiCharEscape};
  ///
  /// let escape = MultiCharEscape::new(
  ///     SimpleSpan::new(6, 9),  // "xFF"
  ///     SimpleSpan::new(5, 9)   // "\xFF"
  /// );
  /// ```
  #[inline(always)]
  pub const fn new(content: S, span: S) -> Self {
    Self { content, span }
  }

  /// Returns the span of the content (characters after the backslash).
  ///
  /// For `\xFF`, this returns the span covering "xFF" (not including `\`).
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::MultiCharEscape};
  ///
  /// let escape = MultiCharEscape::new(
  ///     SimpleSpan::new(6, 9),
  ///     SimpleSpan::new(5, 9)
  /// );
  /// assert_eq!(escape.content(), SimpleSpan::new(6, 9));
  /// ```
  #[inline(always)]
  pub const fn content(&self) -> S
  where
    S: Copy,
  {
    self.content
  }

  /// Returns a reference to the content span.
  #[inline(always)]
  pub const fn content_ref(&self) -> &S {
    &self.content
  }

  /// Returns a mutable reference to the content span.
  #[inline(always)]
  pub const fn content_mut(&mut self) -> &mut S {
    &mut self.content
  }

  /// Returns the span of the entire escape sequence.
  ///
  /// This includes both the backslash and all following characters.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::MultiCharEscape};
  ///
  /// let escape = MultiCharEscape::new(
  ///     SimpleSpan::new(6, 9),
  ///     SimpleSpan::new(5, 9)
  /// );
  /// assert_eq!(escape.span(), SimpleSpan::new(5, 9)); // Full `\xFF`
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Returns a reference to the span.
  #[inline(always)]
  pub const fn span_ref(&self) -> &S {
    &self.span
  }

  /// Returns a mutable reference to the span.
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut S {
    &mut self.span
  }

  /// Bumps both the content span and full span by `n`.
  ///
  /// This is useful when adjusting positions after processing or when
  /// combining escape sequences from different parsing contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::MultiCharEscape};
  ///
  /// let mut escape = MultiCharEscape::new(
  ///     SimpleSpan::new(6, 9),
  ///     SimpleSpan::new(5, 9)
  /// );
  ///
  /// escape.bump(&10);
  ///
  /// assert_eq!(escape.content(), SimpleSpan::new(16, 19));
  /// assert_eq!(escape.span(), SimpleSpan::new(15, 19));
  /// ```
  #[inline]
  pub fn bump(&mut self, offset: &S::Offset) -> &mut Self
  where
    S: crate::span::Span,
  {
    self.span.bump(offset);
    self.content.bump(offset);
    self
  }
}

/// A generic escape sequence representation using a lexeme.
///
/// This type provides a unified representation for escape sequences that can
/// contain either a single character or a multi-character sequence. It wraps
/// a [`Lexeme`] (which can be either [`Lexeme::Char`] or [`Lexeme::Range`]) along
/// with the full span of the escape.
///
/// # Use Cases
///
/// Use this type when you need to handle escape sequences generically without
/// knowing in advance whether they're single-character or multi-character:
///
/// - During initial lexing before categorizing escape types
/// - In error reporting where escape type doesn't matter
/// - When building AST nodes that accept any escape format
///
/// # Type Parameters
///
/// * `Char` - The character type (typically `char` for UTF-8 or `u8` for bytes)
///
/// # Examples
///
/// ## Creating from a Single Character
///
/// ```
/// use tokora::{SimpleSpan, utils::{EscapedLexeme, PositionedChar}};
///
/// let escape = EscapedLexeme::from_positioned_char(
///     SimpleSpan::new(10, 12),                       // Full span `\n`
///     PositionedChar::with_position('n', 11)   // Just the 'n'
/// );
///
/// assert_eq!(escape.span(), SimpleSpan::new(10, 12));
/// assert!(escape.lexeme_ref().is_char());
/// ```
///
/// ## Creating from a Sequence
///
/// ```
/// use tokora::{SimpleSpan, utils::EscapedLexeme};
///
/// let escape: EscapedLexeme = EscapedLexeme::from_sequence(
///     SimpleSpan::new(5, 9),   // Full span `\xFF`
///     SimpleSpan::new(6, 9)    // Just "xFF"
/// );
///
/// assert_eq!(escape.span(), SimpleSpan::new(5, 9));
/// assert!(escape.lexeme_ref().is_range());
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct EscapedLexeme<Char = char, O = usize> {
  span: SimpleSpan<O>,
  lexeme: Lexeme<Char, O>,
}

impl<Char, O> core::fmt::Display for EscapedLexeme<Char, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self.lexeme {
      Lexeme::Char(ref ch) => write!(f, "\\{} at {}", ch.char_ref().display(), ch.position_ref()),
      Lexeme::Range(ref range) => write!(f, "escape sequence at {}", range),
    }
  }
}

impl<Char, O> EscapedLexeme<Char, O> {
  /// Creates a new escaped lexeme.
  ///
  /// ## Parameters
  ///
  /// - `span`: The full span of the escape sequence (including backslash)
  /// - `lexeme`: The content after the backslash (as a Lexeme)
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{EscapedLexeme, Lexeme, PositionedChar}};
  ///
  /// let lexeme = Lexeme::from(PositionedChar::with_position('n', 11));
  /// let escape = EscapedLexeme::new(SimpleSpan::new(10, 12), lexeme);
  /// ```
  #[inline(always)]
  pub const fn new(span: SimpleSpan<O>, lexeme: Lexeme<Char, O>) -> Self {
    Self { span, lexeme }
  }

  /// Creates an escaped lexeme from a span and positioned character.
  ///
  /// This is a convenience constructor for single-character escapes.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{EscapedLexeme, PositionedChar}};
  ///
  /// let escape = EscapedLexeme::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('t', 11)
  /// );
  ///
  /// assert!(escape.lexeme_ref().is_char());
  /// ```
  #[inline(always)]
  pub const fn from_positioned_char(span: SimpleSpan<O>, ch: PositionedChar<Char, O>) -> Self {
    Self::new(span, Lexeme::Char(ch))
  }

  /// Creates an escaped lexeme from a span and positioned character.
  ///
  /// This is a convenience constructor for single-character escapes.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{EscapedLexeme, PositionedChar}};
  ///
  /// let escape = EscapedLexeme::from_char(
  ///     SimpleSpan::new(10, 12),
  ///     11,
  ///    't'
  /// );
  ///
  /// assert!(escape.lexeme_ref().is_char());
  /// ```
  #[inline(always)]
  pub const fn from_char(span: SimpleSpan<O>, pos: O, ch: Char) -> Self {
    Self::from_positioned_char(span, PositionedChar::with_position(ch, pos))
  }

  /// Creates an escaped lexeme from a span and content span.
  ///
  /// This is a convenience constructor for multi-character escapes.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::EscapedLexeme};
  ///
  /// let escape: EscapedLexeme = EscapedLexeme::from_sequence(
  ///     SimpleSpan::new(5, 9),   // Full `\xFF`
  ///     SimpleSpan::new(6, 9)    // Just "xFF"
  /// );
  ///
  /// assert!(escape.lexeme_ref().is_range());
  /// ```
  #[inline(always)]
  pub const fn from_sequence(span: SimpleSpan<O>, content: SimpleSpan<O>) -> Self {
    Self::new(span, Lexeme::Range(content))
  }

  /// Returns the span of the entire escape sequence.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{EscapedLexeme, PositionedChar}};
  ///
  /// let escape = EscapedLexeme::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('n', 11)
  /// );
  /// assert_eq!(escape.span(), SimpleSpan::new(10, 12));
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.span
  }

  /// Returns a reference to the span.
  #[inline(always)]
  pub const fn span_ref(&self) -> SimpleSpan<&O> {
    self.span.as_ref()
  }

  /// Returns a mutable reference to the span.
  #[inline(always)]
  pub const fn span_mut(&mut self) -> SimpleSpan<&mut O> {
    self.span.as_mut()
  }

  /// Returns the lexeme representing the escape content.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{EscapedLexeme, PositionedChar}};
  ///
  /// let escape = EscapedLexeme::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('n', 11)
  /// );
  ///
  /// let lexeme = escape.lexeme();
  /// assert!(lexeme.is_char());
  /// ```
  #[inline(always)]
  pub const fn lexeme(&self) -> Lexeme<Char, O>
  where
    Char: Copy,
    O: Copy,
  {
    self.lexeme
  }

  /// Returns a reference to the lexeme.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{EscapedLexeme, PositionedChar}};
  ///
  /// let escape = EscapedLexeme::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('n', 11)
  /// );
  /// assert!(escape.lexeme_ref().is_char());
  /// ```
  #[inline(always)]
  pub const fn lexeme_ref(&self) -> &Lexeme<Char, O> {
    &self.lexeme
  }

  /// Returns a mutable reference to the lexeme.
  #[inline(always)]
  pub const fn lexeme_mut(&mut self) -> &mut Lexeme<Char, O> {
    &mut self.lexeme
  }

  /// Bumps the span and lexeme by `n`.
  ///
  /// This is useful when adjusting positions after processing or when
  /// combining escape sequences from different parsing contexts.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{EscapedLexeme, PositionedChar}};
  ///
  /// let mut escape = EscapedLexeme::from_positioned_char(
  ///     SimpleSpan::new(10, 12),
  ///     PositionedChar::with_position('n', 11)
  /// );
  ///
  /// escape.bump(&5);
  /// assert_eq!(escape.span(), SimpleSpan::new(15, 17));
  /// ```
  #[inline(always)]
  pub fn bump(&mut self, offset: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.span.bump(offset);
    self.lexeme.bump(offset);
    self
  }
}
