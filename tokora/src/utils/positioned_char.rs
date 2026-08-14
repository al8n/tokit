use core::ops::Add;

use super::CharLen;

use crate::span::SimpleSpan;

/// A character paired with its byte position in the source input.
///
/// `PositionedChar` combines a character (or character-like value) with the byte offset
/// where it appears in the source. This is particularly useful for:
///
/// - **Error reporting**: Show exactly where an unexpected character occurred
/// - **Lexer state**: Track position while processing character-by-character
/// - **Diagnostics**: Build precise error messages with column/line information
/// - **Character-level parsing**: When token-level parsing is too coarse
///
/// # Type Parameter
///
/// - `Char`: The character type, typically `char` for UTF-8 text or `u8` for bytes
///
/// # Design
///
/// This type is designed to be lightweight and efficient:
/// - **Copy**: Can be freely copied (when `Char` is `Copy`)
/// - **Small**: Just the character plus one `usize` for position
/// - **Comparable**: Supports comparison operations based on both character and position
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::utils::PositionedChar;
///
/// let ch = PositionedChar::with_position('x', 42);
///
/// assert_eq!(ch.char(), 'x');
/// assert_eq!(ch.position(), 42);
/// ```
///
/// ## Character-by-Character Processing
///
/// ```rust,ignore
/// use tokora::utils::PositionedChar;
///
/// fn process_input(input: &str) -> Vec<PositionedChar<char>> {
///     input.char_indices()
///         .map(|(pos, ch)| PositionedChar::with_position(ch, pos))
///         .collect()
/// }
///
/// let positioned = process_input("hello");
/// assert_eq!(positioned[0].char(), 'h');
/// assert_eq!(positioned[0].position(), 0);
/// ```
///
/// ## Error Reporting
///
/// ```rust,ignore
/// use tokora::utils::PositionedChar;
///
/// fn report_unexpected(pc: PositionedChar<char>, input: &str) {
///     let line_start = input[..pc.position()]
///         .rfind('\n')
///         .map(|p| p + 1)
///         .unwrap_or(0);
///
///     let column = pc.position() - line_start;
///
///     eprintln!("Unexpected character '{}' at position {} (column {})",
///         pc.char(), pc.position(), column);
/// }
/// ```
///
/// ## Mapping Characters
///
/// ```rust
/// use tokora::utils::PositionedChar;
///
/// let lowercase = PositionedChar::with_position('a', 10);
/// let uppercase = lowercase.map(|c| c.to_ascii_uppercase());
///
/// assert_eq!(uppercase.char(), 'A');
/// assert_eq!(uppercase.position(), 10); // Position preserved
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositionedChar<Char, Offset = usize> {
  char: Char,
  pub(crate) position: Offset,
}

impl<Char, Offset> PositionedChar<Char, Offset> {
  /// Create a new positioned character with the given position.
  #[inline(always)]
  pub const fn with_position(char: Char, position: Offset) -> Self {
    Self { char, position }
  }

  /// Get the character.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let pc = PositionedChar::with_position('x', 10);
  /// assert_eq!(pc.char(), 'x');
  /// ```
  #[inline(always)]
  pub const fn char(&self) -> Char
  where
    Char: Copy,
  {
    self.char
  }

  /// Get the reference to the character.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let pc = PositionedChar::with_position('x', 10);
  /// assert_eq!(pc.char_ref(), &'x');
  /// ```
  #[inline(always)]
  pub const fn char_ref(&self) -> &Char {
    &self.char
  }

  /// Get a mutable reference to the character.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut pc = PositionedChar::with_position('a', 10);
  /// *pc.char_mut() = 'b';
  /// assert_eq!(pc.char(), 'b');
  /// ```
  #[inline(always)]
  pub const fn char_mut(&mut self) -> &mut Char {
    &mut self.char
  }

  /// Get the reference of the position.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let pc = PositionedChar::with_position('x', 42);
  /// assert_eq!(pc.position(), 42);
  /// ```
  #[inline(always)]
  pub const fn position_ref(&self) -> &Offset {
    &self.position
  }

  /// Get the position.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let pc = PositionedChar::with_position('x', 42);
  /// assert_eq!(pc.position(), 42);
  /// ```
  #[inline(always)]
  pub const fn position(&self) -> Offset
  where
    Offset: Copy,
  {
    self.position
  }

  /// Returns the span covers this positioned character.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{Span, utils::PositionedChar};
  ///
  ///
  /// let pc = PositionedChar::with_position('x', 42);
  /// assert_eq!(pc.span(), Span::new(42, 43));
  /// ```
  #[inline(always)]
  pub fn span(&self) -> SimpleSpan<Offset>
  where
    Char: CharLen,
    Offset: Clone + Ord,
    for<'a> &'a Offset: Add<usize, Output = Offset>,
  {
    let start = self.position_ref();
    let end = start + self.char_ref().char_len();
    SimpleSpan::new(start.clone(), end)
  }

  /// Set the position, returning a mutable reference of the positioned character.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut pc = PositionedChar::with_position('x', 10);
  /// pc.set_position(20);
  /// assert_eq!(pc.position(), 20);
  /// ```
  #[inline(always)]
  pub fn set_position(&mut self, position: Offset) -> &mut Self {
    self.position = position;
    self
  }

  /// Bump the position by `n`,  returning a mutable reference of the positioned character.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut pc = PositionedChar::with_position('x', 10);
  /// pc.bump_position(&5);
  /// assert_eq!(pc.position(), 15);
  /// ```
  #[inline(always)]
  pub fn bump_position<'a>(&'a mut self, n: &'a Offset) -> &'a mut Self
  where
    Offset: core::ops::AddAssign<&'a Offset>,
  {
    self.position += n;
    self
  }

  /// Converts the positioned character to a reference.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let pc = PositionedChar::with_position('x', 10);
  /// let pc_ref = pc.as_ref();
  /// assert_eq!(**pc_ref.char_ref(), 'x');
  /// assert_eq!(pc_ref.position(), &10);
  /// ```
  #[inline(always)]
  pub const fn as_ref(&self) -> PositionedChar<&Char, &Offset> {
    PositionedChar {
      char: &self.char,
      position: &self.position,
    }
  }

  /// Converts the positioned character to a mutable reference.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let mut pc = PositionedChar::with_position('a', 10);
  /// {
  ///     let mut pc_mut = pc.as_mut();
  ///     **pc_mut.char_mut() = 'b';
  /// }
  /// assert_eq!(pc.char(), 'b');
  /// ```
  #[inline(always)]
  pub const fn as_mut(&mut self) -> PositionedChar<&mut Char, &mut Offset> {
    PositionedChar {
      char: &mut self.char,
      position: &mut self.position,
    }
  }

  /// Maps the character to another character, returning a new positioned character.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let pc = PositionedChar::with_position('a', 10);
  /// let upper = pc.map(|c| c.to_ascii_uppercase());
  /// assert_eq!(upper.char(), 'A');
  /// assert_eq!(upper.position(), 10);
  /// ```
  #[inline(always)]
  pub fn map<NewChar, F>(self, f: F) -> PositionedChar<NewChar, Offset>
  where
    F: FnOnce(Char) -> NewChar,
  {
    PositionedChar {
      char: f(self.char),
      position: self.position,
    }
  }

  /// Consumes the positioned character, returning the character and its position.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::PositionedChar;
  ///
  /// let pc = PositionedChar::with_position('x', 42);
  /// let (ch, pos) = pc.into_components();
  /// assert_eq!(ch, 'x');
  /// assert_eq!(pos, 42);
  /// ```
  #[inline(always)]
  pub fn into_components(self) -> (Char, Offset) {
    (self.char, self.position)
  }
}

impl<Char: core::fmt::Display, Offset> core::fmt::Display for PositionedChar<Char, Offset> {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{}", self.char)
  }
}
