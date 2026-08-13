use core::ops::{Add, AddAssign};

use crate::{
  span::SimpleSpan,
  utils::{CharLen, Lexeme, PositionedChar, human_display::DisplayHuman},
};

/// An error indicating that an unexpected prefix was found after a valid token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnexpectedPrefix<Char, Knowledge, O = usize> {
  token: SimpleSpan<O>,
  prefix: Lexeme<Char, O>,
  knowledge: Option<Knowledge>,
}

impl<Char, Knowledge, O> UnexpectedPrefix<Char, Knowledge, O> {
  /// Create a new `UnexpectedPrefix` error indicating a leading zero was found.
  ///
  /// ## Panics
  /// - If the prefix overlaps the token span (i.e., if it ends after the token span starts).
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{knowledge::IntLiteral}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, IntLiteral> = UnexpectedPrefix::leading_zero(
  ///   SimpleSpan::new(1, 5),
  ///   0,
  ///   '0'
  /// );
  /// ```
  #[inline]
  pub fn leading_zero(token: SimpleSpan<O>, pos: O, ch: Char) -> Self
  where
    Knowledge: DenyLeadingZero,
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    Self::from_char(token, pos, ch).with_knowledge(Knowledge::INIT)
  }

  /// Create a new `UnexpectedPrefix` error from the given token span and the prefix span
  ///
  /// ## Panics
  /// - If the prefix span ends after the token span starts.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{knowledge::IntLiteral}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, IntLiteral> = UnexpectedPrefix::leading_zeros(
  ///   SimpleSpan::new(6, 10),
  ///   SimpleSpan::new(0, 6)
  /// );
  /// ```
  #[inline]
  pub fn leading_zeros(token: SimpleSpan<O>, span: SimpleSpan<O>) -> Self
  where
    Knowledge: DenyLeadingZero,
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    Self::from_prefix(token, span).with_knowledge(Knowledge::INIT)
  }
}

impl<Char, Knowledge, O> UnexpectedPrefix<Char, Knowledge, O> {
  /// Creates a new `UnexpectedPrefix` error with the span of the valid token and the unexpected prefix.
  ///
  /// ## Panics
  /// - If the prefix span ends after the token span starts.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::new(
  ///     SimpleSpan::new(1, 5),
  ///     Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  /// ```
  #[inline]
  pub fn new(token: SimpleSpan<O>, prefix: Lexeme<Char, O>) -> Self
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    assert!(
      prefix.end().le(token.start_ref()),
      "prefix ends after token starts"
    );

    Self {
      token,
      prefix,
      knowledge: None,
    }
  }

  /// Create a new `UnexpectedPrefix` error from the given token span and character with position.
  ///
  /// ## Panics
  /// - If the prefix overlaps the token span (i.e., if it ends after the token span starts).
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::from_char(
  ///    SimpleSpan::new(1, 5),
  ///    0,
  ///   'x'
  /// );
  /// ```
  #[inline]
  pub fn from_char(token: SimpleSpan<O>, pos: O, ch: Char) -> Self
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    Self::from_positioned_char(token, PositionedChar::with_position(ch, pos))
  }

  /// Create a new `UnexpectedPrefix` error from the given token span and character with position.
  ///
  /// ## Panics
  /// - If the prefix overlaps the token span (i.e., if it ends after the token span starts).
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::from_positioned_char(
  ///    SimpleSpan::new(1, 5),
  ///    PositionedChar::with_position('x', 0)
  /// );
  /// ```
  #[inline]
  pub fn from_positioned_char(token: SimpleSpan<O>, ch: PositionedChar<Char, O>) -> Self
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    Self::new(token, Lexeme::Char(ch))
  }

  /// Adds knowledge to the `UnexpectedPrefix` error.
  #[inline]
  pub const fn with_knowledge_const(mut self, knowledge: Knowledge) -> Self
  where
    Knowledge: Copy,
  {
    self.knowledge = Some(knowledge);
    self
  }

  /// Adds knowledge to the `UnexpectedPrefix` error.
  #[inline]
  pub fn with_knowledge(mut self, knowledge: Knowledge) -> Self {
    self.knowledge = Some(knowledge);
    self
  }

  /// Create a new `UnexpectedPrefix` error from the given token span and the prefix span
  ///
  /// ## Panics
  /// - If the prefix span ends after the token span starts.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::from_prefix(
  ///   SimpleSpan::new(6, 10),
  ///   SimpleSpan::new(0, 6)
  /// );
  /// ```
  #[inline]
  pub fn from_prefix(token: SimpleSpan<O>, span: SimpleSpan<O>) -> Self
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    Self::new(token, Lexeme::Range(span))
  }

  /// Returns the full span since the start of the unexpected prefix to the end of the valid token.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::new(
  ///   SimpleSpan::new(1, 5),
  ///   Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  /// assert_eq!(error.span(), SimpleSpan::new(0, 5));
  /// ```
  #[inline]
  pub fn span(&self) -> SimpleSpan<O>
  where
    Char: CharLen,
    O: Clone + Ord,
  {
    let end = self.token.end_ref();
    let start = match &self.prefix {
      Lexeme::Char(positioned_char) => positioned_char.position_ref().clone(),
      Lexeme::Range(span) => span.start_ref().clone(),
    };
    SimpleSpan::new(start, end.clone())
  }

  /// Returns the span of the valid token before the unexpected prefix.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::new(
  ///     SimpleSpan::new(1, 5),
  ///     Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  /// assert_eq!(error.token(), SimpleSpan::new(1, 5));
  /// ```
  #[inline]
  pub const fn token(&self) -> SimpleSpan<O>
  where
    O: Copy,
  {
    self.token
  }

  /// Returns the unexpected prefix found before the valid token.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::new(
  ///    SimpleSpan::new(1, 5),
  ///   Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  ///
  /// assert_eq!(
  ///   error.prefix(),
  ///   &Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  /// ```
  #[inline]
  pub const fn prefix(&self) -> &Lexeme<Char, O> {
    &self.prefix
  }

  /// Consumes the error and returns the unexpected prefix.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::new(
  ///   SimpleSpan::new(1, 5),
  ///   Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  /// let (token, prefix) = error.into_components();
  /// assert_eq!(token, SimpleSpan::new(1, 5));
  /// assert_eq!(
  ///   prefix,
  ///   Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  /// ```
  #[inline]
  pub fn into_components(self) -> (SimpleSpan<O>, Lexeme<Char, O>) {
    (self.token, self.prefix)
  }

  /// Bumps both the start and end positions of the token span by the given offset.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  ///
  /// ## Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{Lexeme, PositionedChar}, error::UnexpectedPrefix};
  ///
  /// let mut error: UnexpectedPrefix<char, ()> = UnexpectedPrefix::new(
  ///   SimpleSpan::new(1, 5),
  ///   Lexeme::Char(PositionedChar::with_position('x', 0))
  /// );
  /// error.bump(&10);
  /// assert_eq!(error.token(), SimpleSpan::new(11, 15));
  /// ```
  #[inline]
  pub fn bump(&mut self, offset: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.token.bump(offset);
    self
  }
}

impl<Char, Knowledge> core::fmt::Display for UnexpectedPrefix<Char, Knowledge>
where
  Char: DisplayHuman,
  Knowledge: DisplayHuman,
{
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.prefix {
      Lexeme::Char(positioned_char) => match &self.knowledge {
        None => write!(
          f,
          "unexpected prefix '{}' at position {} found before {}",
          positioned_char.char_ref().display(),
          positioned_char.position(),
          self.token
        ),
        Some(knowledge) => {
          write!(
            f,
            "unexpected prefix '{}' at position {} found before '{}'@({})",
            positioned_char.char_ref().display(),
            positioned_char.position(),
            knowledge.display(),
            self.token
          )
        }
      },
      Lexeme::Range(span) => match &self.knowledge {
        Some(knowledge) => write!(
          f,
          "unexpected prefix at {} found before '{}'@({})",
          span,
          knowledge.display(),
          self.token
        ),
        None => write!(
          f,
          "unexpected prefix at {} found before {}",
          span, self.token
        ),
      },
    }
  }
}

impl<Char, Knowledge> core::error::Error for UnexpectedPrefix<Char, Knowledge>
where
  Char: DisplayHuman + core::fmt::Debug,
  Knowledge: DisplayHuman + core::fmt::Debug,
{
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(feature = "std")]
mod tests;

/// A marker trait indicating that leading zeros are not allowed for the implementing knowledge type.
pub trait DenyLeadingZero: sealed::Sealed {}

impl<T> DenyLeadingZero for T where T: sealed::Sealed {}

mod sealed {
  use crate::utils::knowledge::{
    BinaryLiteral, FloatLiteral, HexFloatLiteral, HexLiteral, IntLiteral, OctalLiteral,
  };

  pub trait Sealed {
    const INIT: Self;
  }

  impl Sealed for FloatLiteral {
    const INIT: Self = FloatLiteral(());
  }

  impl Sealed for HexFloatLiteral {
    const INIT: Self = HexFloatLiteral(());
  }

  impl Sealed for IntLiteral {
    const INIT: Self = IntLiteral(());
  }

  impl Sealed for HexLiteral {
    const INIT: Self = HexLiteral(());
  }

  impl Sealed for BinaryLiteral {
    const INIT: Self = BinaryLiteral(());
  }

  impl Sealed for OctalLiteral {
    const INIT: Self = OctalLiteral(());
  }
}
