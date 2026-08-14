use core::ops::{Add, AddAssign};

use derive_more::{From, IsVariant, TryUnwrap, Unwrap};

use super::{CharLen, PositionedChar};

use crate::span::SimpleSpan;

/// A compact, zero-copy description of a lexeme in source code.
///
/// `Lexeme` is a space-efficient way to represent either a single character or
/// a span of bytes from the original source. It **does not own text** - instead
/// it carries just enough information to identify the lexeme's location.
///
/// # Variants
///
/// - **Char**: A single positioned character (e.g., an unexpected '`{`' at position 42)
/// - **Span**: A byte range into the source (e.g., an unexpected keyword spanning bytes 100-105)
///
/// # Design Philosophy
///
/// This type is designed for error reporting where you need to identify problematic
/// source locations without allocating strings. By storing only positions/ranges,
/// you can defer string slicing until error display time, keeping errors lightweight.
///
/// # Derived Helpers
///
/// This type provides several helper methods via derive macros:
/// - `is_char()` / `is_range()`: Check which variant it is
/// - `unwrap_char()` / `unwrap_range()`: Extract the inner value (panics if wrong variant)
/// - `try_unwrap_char()` / `try_unwrap_range()`: Try to extract the inner value
///
/// # Use Cases
///
/// - **Error reporting**: Identify unexpected tokens without copying text
/// - **Lexer errors**: Report malformed tokens with precise locations
/// - **Parser errors**: Track problematic syntax fragments
/// - **Diagnostic tools**: Build rich error messages with source context
///
/// # Examples
///
/// ## Single Character Lexeme
///
/// ```rust
/// use tokora::utils::{Lexeme, PositionedChar};
///
/// let pc = PositionedChar::with_position('!', 42);
/// let lexeme = Lexeme::from(pc);
///
/// assert!(lexeme.is_char());
/// assert_eq!(lexeme.unwrap_char().char(), '!');
/// assert_eq!(lexeme.unwrap_char().position(), 42);
/// ```
///
/// ## Span Lexeme
///
/// ```rust
/// use tokora::{SimpleSpan, utils::Lexeme};
///
/// let span = SimpleSpan::new(10, 15); // bytes 10-15
/// let lexeme: Lexeme<char> = Lexeme::from(span);
///
/// assert!(lexeme.is_range());
/// assert_eq!(lexeme.unwrap_range().start(), 10);
/// assert_eq!(lexeme.unwrap_range().end(), 15);
/// ```
///
/// ## Getting Span from Either Variant
///
/// ```rust
/// use tokora::utils::{Lexeme, PositionedChar};
///
/// let lexeme = Lexeme::from(PositionedChar::with_position('x', 5));
///
/// // 'x' is 1 byte in UTF-8
/// let span = lexeme.span();
/// assert_eq!(span.start(), 5);
/// assert_eq!(span.end(), 6);
/// ```
///
/// ## Mapping Characters
///
/// ```rust
/// use tokora::utils::{Lexeme, PositionedChar};
///
/// let lexeme = Lexeme::from(PositionedChar::with_position('a', 10));
/// let upper = lexeme.map(|c| c.to_ascii_uppercase());
///
/// assert_eq!(upper.unwrap_char().char(), 'A');
/// assert_eq!(upper.unwrap_char().position(), 10); // Position preserved
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IsVariant, Unwrap, TryUnwrap, From)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
pub enum Lexeme<Char = char, O = usize> {
  /// A single positioned character with its byte position.
  ///
  /// Use this variant when the unexpected lexeme is exactly one character long.
  Char(PositionedChar<Char, O>),

  /// A half-open byte range `[start, end)` into the original source.
  ///
  /// The range must be non-empty (`start < end`) and point into the same
  /// buffer that was tokenized. Prefer UTF-8 boundary indices if you plan to
  /// slice `&str`.
  ///
  /// Use this variant when the unexpected lexeme spans multiple characters
  /// or when you want to represent a multi-byte token.
  Range(SimpleSpan<O>),
}

impl<Char, O> core::fmt::Display for Lexeme<Char, O>
where
  Char: super::human_display::DisplayHuman,
  O: core::fmt::Display,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Char(pc) => write!(f, "'{}' at {}", pc.char_ref().display(), pc.position_ref()),
      Self::Range(span) => write!(f, "{}", span),
    }
  }
}

impl<Char, O> Lexeme<Char, O> {
  /// Creates a new `Lexeme` from a `Char` and its position.
  ///
  /// ## Example
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}};
  ///
  /// let char_lexeme = Lexeme::from_char(5, 'x');
  /// assert_eq!(char_lexeme.start(), 5);
  /// ```
  #[inline(always)]
  pub const fn from_char(pos: O, ch: Char) -> Self {
    Self::Char(PositionedChar::with_position(ch, pos))
  }

  /// Creates a new `Lexeme` from a range
  ///
  /// ## Example
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Lexeme};
  ///
  /// let l = Lexeme::<char>::from_range(5..10);
  /// assert_eq!(l.start(), 5);
  /// assert_eq!(l.end(), 10);
  /// assert!(l.is_range());
  /// ```
  #[inline(always)]
  pub fn from_range(range: impl Into<SimpleSpan<O>>) -> Self {
    Self::Range(range.into())
  }

  /// Creates a new `Lexeme` from a range
  ///
  /// ## Example
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Lexeme};
  ///
  /// let l = Lexeme::<char>::from_range_const(SimpleSpan::new(5, 10));
  /// assert_eq!(l.start(), 5);
  /// assert_eq!(l.end(), 10);
  /// assert!(l.is_range());
  /// ```
  #[inline(always)]
  pub const fn from_range_const(span: SimpleSpan<O>) -> Self {
    Self::Range(span)
  }

  /// Returns the start position of the lexeme.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}};
  ///
  /// let char_lexeme = Lexeme::from(PositionedChar::with_position('x', 5));
  /// assert_eq!(char_lexeme.start(), 5);
  ///
  /// let span_lexeme: Lexeme<char> = Lexeme::from(SimpleSpan::new(10, 15));
  /// assert_eq!(span_lexeme.start(), 10);
  /// ```
  #[inline(always)]
  pub const fn start(&self) -> O
  where
    O: Copy,
  {
    match self {
      Self::Char(pc) => pc.position(),
      Self::Range(r) => r.start(),
    }
  }

  /// Returns the start position of the lexeme.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}};
  ///
  /// let char_lexeme = Lexeme::from(PositionedChar::with_position('x', 5));
  /// assert_eq!(char_lexeme.start(), 5);
  ///
  /// let span_lexeme: Lexeme<char> = Lexeme::from(SimpleSpan::new(10, 15));
  /// assert_eq!(span_lexeme.start(), 10);
  /// ```
  #[inline(always)]
  pub const fn start_ref(&self) -> &O {
    match self {
      Self::Char(pc) => pc.position_ref(),
      Self::Range(r) => r.start_ref(),
    }
  }

  /// Returns the end position of the lexeme.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}};
  ///
  /// let char_lexeme = Lexeme::from(PositionedChar::with_position('x', 5));
  /// assert_eq!(char_lexeme.end(), 6);
  ///
  /// let span_lexeme: Lexeme<char> = Lexeme::from(SimpleSpan::new(10, 15));
  /// assert_eq!(span_lexeme.end(), 15);
  /// ```
  #[inline(always)]
  pub fn end(&self) -> O
  where
    Char: CharLen,
    O: Clone,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    match self {
      Self::Char(pc) => pc.position_ref() + pc.char_ref().char_len(),
      Self::Range(r) => r.end_ref().clone(),
    }
  }

  /// Maps the character type to another type if this is a [`Char`](Lexeme::Char) variant.
  ///
  /// The [`Span`](Lexeme::Range) variant is left unchanged, as it doesn't contain
  /// a character value.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::utils::{Lexeme, PositionedChar};
  ///
  /// let lexeme = Lexeme::from(PositionedChar::with_position('a', 5));
  /// let upper = lexeme.map(|c| c.to_ascii_uppercase());
  ///
  /// assert_eq!(upper.unwrap_char().char(), 'A');
  /// ```
  #[inline(always)]
  pub fn map<F, NewChar>(self, f: F) -> Lexeme<NewChar, O>
  where
    F: FnOnce(Char) -> NewChar,
  {
    match self {
      Self::Char(pc) => Lexeme::Char(pc.map(f)),
      Self::Range(r) => Lexeme::Range(r),
    }
  }

  /// Returns the byte span covered by this lexeme using a custom length function.
  ///
  /// For the [`Char`](Lexeme::Char) variant, the provided `len_of` function is
  /// called to determine how many bytes the character occupies. For the
  /// [`Span`](Lexeme::Range) variant, the span is returned directly.
  ///
  /// Use this method when your `Char` type doesn't implement [`CharLen`].
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::utils::{Lexeme, PositionedChar};
  ///
  /// let lexeme = Lexeme::from(PositionedChar::with_position('€', 10));
  ///
  /// // '€' is 3 bytes in UTF-8
  /// let span = lexeme.span_with(|c: &char| c.len_utf8());
  /// assert_eq!(span.start(), 10);
  /// assert_eq!(span.end(), 13);
  /// ```
  #[inline(always)]
  pub fn span_with(&self, len_of: impl FnOnce(&Char) -> usize) -> SimpleSpan<O>
  where
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    match self {
      Self::Char(pc) => {
        let start = pc.position_ref();
        let end = start + len_of(pc.char_ref());
        SimpleSpan::new(start.clone(), end)
      }
      Self::Range(r) => r.clone(),
    }
  }

  /// Returns the byte span covered by this lexeme.
  ///
  /// For the [`Char`](Lexeme::Char) variant, uses the [`CharLen`] trait to
  /// determine how many bytes the character occupies. For the [`Span`](Lexeme::Range)
  /// variant, returns the span directly.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}};
  ///
  /// // Single character
  /// let char_lexeme = Lexeme::from(PositionedChar::with_position('x', 5));
  /// assert_eq!(char_lexeme.span(), SimpleSpan::new(5, 6));
  ///
  /// // Span
  /// let span_lexeme: Lexeme<char> = Lexeme::from(SimpleSpan::new(10, 15));
  /// assert_eq!(span_lexeme.span(), SimpleSpan::new(10, 15));
  /// ```
  #[inline(always)]
  pub fn span(&self) -> SimpleSpan<O>
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    match self {
      Self::Char(pc) => pc.span(),
      Self::Range(r) => r.clone(),
    }
  }

  /// Adjusts the position/span by adding `n` bytes to the offset.
  ///
  /// For the [`Char`](Lexeme::Char) variant, bumps the character's position.
  /// For the [`Span`](Lexeme::Range) variant, bumps both start and end of the span.
  ///
  /// Returns a mutable reference to self for method chaining.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::utils::{Lexeme, PositionedChar};
  ///
  /// let mut lexeme = Lexeme::from(PositionedChar::with_position('x', 5));
  /// lexeme.bump(&10);
  ///
  /// assert_eq!(lexeme.unwrap_char().position(), 15);
  /// ```
  #[inline(always)]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    match self {
      Self::Char(positioned_char) => {
        positioned_char.position += n;
        self
      }
      Self::Range(span) => {
        span.bump(n);
        self
      }
    }
  }
}
