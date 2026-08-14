use core::ops::{Add, AddAssign};

use crate::{
  span::SimpleSpan,
  utils::{
    CharLen, Lexeme, PositionedChar, human_display::DisplayHuman, knowledge::LineTerminator,
  },
};

/// A specialized `UnexpectedLexeme` for line terminators.
///
/// This type represents an unexpected line terminator character
/// encountered during lexing or parsing, along with a hint
/// describing what was expected instead.
pub type UnexpectedLineTerminator<Char, O = usize> = UnexpectedLexeme<Char, LineTerminator, O>;

/// A zero-copy error structure combining an unexpected lexeme with a diagnostic hint.
///
/// `UnexpectedLexeme` pairs a [`Lexeme`] (identifying what went wrong) with a hint
/// (describing what was expected instead). This structure is designed for building
/// rich, informative error messages without allocating strings.
///
/// # Type Parameters
///
/// - **Char**: The character type (typically `char` for UTF-8 or `u8` for bytes)
/// - **Hint**: Any type describing what was expected (often implements `Display`)
///
/// # Design Philosophy
///
/// This type stores:
/// - The **lexeme** of the unexpected fragment ([`Char`](Lexeme::Char) or [`SimpleSpan`](Lexeme::Range))
/// - A **hint** describing what was expected next (any type you choose)
///
/// The hint is left generic and unconstrained so you can carry:
/// - Simple strings: `&'static str`
/// - Token kinds: Your own `TokenKind` enum
/// - Rich structures: Custom diagnostic types with multiple suggestions
///
/// # Deref Behavior
///
/// `UnexpectedLexeme` implements `Deref` to `Lexeme<Char, O>`, so you can call all
/// `Lexeme` methods directly on an `UnexpectedLexeme` instance.
///
/// # Use Cases
///
/// - **Lexer errors**: Report unexpected characters with "expected" hints
/// - **Parser errors**: Track unexpected tokens with contextual information
/// - **Error recovery**: Store partial error info without allocating
/// - **Diagnostic tools**: Build structured error reports for IDEs
///
/// # Examples
///
/// ## Basic Error with String Hint
///
/// ```rust
/// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
///
/// let error = UnexpectedLexeme::from_positioned_char(
///     PositionedChar::with_position('!', 42),
///     "expected letter or digit"
/// );
///
/// assert!(error.is_char());
/// assert_eq!(error.lexeme().unwrap_char().position(), 42);
/// assert_eq!(*error.hint(), "expected letter or digit");
/// ```
///
/// ## With Token Kind Hint
///
/// ```rust,ignore
/// use tokora::{error::UnexpectedLexeme, SimpleSpan};
///
/// #[derive(Debug, Clone)]
/// enum Expected {
///     Token(TokenKind),
///     OneOf(Vec<TokenKind>),
/// }
///
/// let error = UnexpectedLexeme::from_range(
///     SimpleSpan::new(10, 15),
///     Expected::OneOf(vec![TokenKind::Semicolon, TokenKind::Comma])
/// );
///
/// // Use in error display
/// match error.hint() {
///     Expected::Token(kind) => println!("Expected {:?}", kind),
///     Expected::OneOf(kinds) => println!("Expected one of: {:?}", kinds),
/// }
/// ```
///
/// ## Mapping Hint
///
/// ```rust
/// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
///
/// let error = UnexpectedLexeme::from_positioned_char(
///     PositionedChar::with_position('x', 5),
///     "number"
/// );
///
/// // Transform the hint to a more detailed message
/// let detailed = error.map_hint(|hint| format!("expected {}, found 'x'", hint));
///
/// assert_eq!(detailed.hint(), "expected number, found 'x'");
/// ```
///
/// ## Accessing Lexeme via Deref
///
/// ```rust
/// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
///
/// let error = UnexpectedLexeme::from_positioned_char(
///     PositionedChar::with_position('!', 10),
///     "identifier"
/// );
///
/// // Call Lexeme methods directly
/// assert!(error.is_char());
/// let span = error.span(); // Deref to Lexeme, call span()
/// assert_eq!(span.start(), 10);
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct UnexpectedLexeme<Char, Hint, O = usize> {
  lexeme: Lexeme<Char, O>,
  hint: Hint,
}

impl<Char, Hint, O> core::fmt::Display for UnexpectedLexeme<Char, Hint, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.lexeme {
      Lexeme::Char(pc) => write!(
        f,
        "unexpected character '{}' at position {}",
        pc.char_ref().display(),
        pc.position_ref(),
      ),
      Lexeme::Range(span) => write!(f, "unexpected characters at {}", span,),
    }
  }
}

impl<Char, Hint, O> core::error::Error for UnexpectedLexeme<Char, Hint, O>
where
  Char: DisplayHuman + core::fmt::Debug,
  Hint: core::fmt::Debug,
  O: core::fmt::Debug + core::fmt::Display,
{
}

impl<Char, Hint, O> core::ops::Deref for UnexpectedLexeme<Char, Hint, O> {
  type Target = Lexeme<Char, O>;

  #[inline]
  fn deref(&self) -> &Self::Target {
    &self.lexeme
  }
}

impl<Char, Hint, O> core::ops::DerefMut for UnexpectedLexeme<Char, Hint, O> {
  #[inline]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.lexeme
  }
}

impl<Char, O> UnexpectedLexeme<Char, LineTerminator, O> {
  /// Creates a new `UnexpectedLineTerminator` from a lexeme and line terminator hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::{Lexeme, PositionedChar, knowledge::LineTerminator}};
  ///
  /// let error = UnexpectedLexeme::new_line(5, '\n');
  ///
  /// assert_eq!(*error.hint(), LineTerminator::NewLine);
  /// ```
  #[inline]
  pub const fn new_line(pos: O, ch: Char) -> Self {
    Self::from_char(pos, ch, LineTerminator::NewLine)
  }

  /// Creates a new unexpected carriage return error.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::{Lexeme, PositionedChar, knowledge::LineTerminator}};
  ///
  /// let error = UnexpectedLexeme::carriage_return(5, '\r');
  ///
  /// assert_eq!(*error.hint(), LineTerminator::CarriageReturn);
  /// ```
  #[inline]
  pub const fn carriage_return(pos: O, ch: Char) -> Self {
    Self::from_char(pos, ch, LineTerminator::CarriageReturn)
  }

  /// Creates a new unexpected carriage return + newline error.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, SimpleSpan, utils::{knowledge::LineTerminator}};
  ///
  /// let error = UnexpectedLexeme::<char, _>::carriage_return_new_line((5..7).into());
  ///
  /// assert_eq!(*error.hint(), LineTerminator::CarriageReturnNewLine);
  /// ```
  #[inline]
  pub const fn carriage_return_new_line(span: SimpleSpan<O>) -> Self {
    Self::from_range_const(span, LineTerminator::CarriageReturnNewLine)
  }
}

impl<Char, Hint, O> UnexpectedLexeme<Char, Hint, O> {
  /// Creates a new `UnexpectedLexeme` from a lexeme and hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::{Lexeme, PositionedChar}};
  ///
  /// let lexeme = Lexeme::from(PositionedChar::with_position('!', 5));
  /// let error = UnexpectedLexeme::new(lexeme, "identifier");
  ///
  /// assert_eq!(*error.hint(), "identifier");
  /// ```
  #[inline]
  pub const fn new(lexeme: Lexeme<Char, O>, hint: Hint) -> Self {
    Self { lexeme, hint }
  }

  /// Constructs an error from a position, character and hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedLexeme;
  ///
  /// let error = UnexpectedLexeme::from_char(
  ///     42,
  ///     '$',
  ///     "alphanumeric character"
  /// );
  ///
  /// assert!(error.is_char());
  /// assert_eq!(error.unwrap_char().position(), 42);
  /// ```
  #[inline]
  pub const fn from_char(pos: O, ch: Char, hint: Hint) -> Self {
    Self::from_positioned_char(PositionedChar::with_position(ch, pos), hint)
  }

  /// Constructs an error from a positioned character and hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('$', 42),
  ///     "alphanumeric character"
  /// );
  ///
  /// assert!(error.is_char());
  /// assert_eq!(error.unwrap_char().position(), 42);
  /// ```
  #[inline]
  pub const fn from_positioned_char(pc: PositionedChar<Char, O>, hint: Hint) -> Self {
    Self::new(Lexeme::Char(pc), hint)
  }

  /// Constructs an error from a byte span and hint (const version).
  ///
  /// Use this in const contexts where `Into<SimpleSpan>` conversions aren't available.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, SimpleSpan};
  ///
  /// let error: UnexpectedLexeme<char, _> = UnexpectedLexeme::from_range_const(
  ///     SimpleSpan::new(10, 15),
  ///     "semicolon"
  /// );
  ///
  /// assert!(error.is_range());
  /// ```
  #[inline]
  pub const fn from_range_const(span: SimpleSpan<O>, hint: Hint) -> Self {
    Self::new(Lexeme::Range(span), hint)
  }

  /// Constructs an error from a byte span and hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedLexeme;
  ///
  /// let error: UnexpectedLexeme<char, _> = UnexpectedLexeme::from_range(10..15, "closing brace");
  ///
  /// assert!(error.is_range());
  /// assert_eq!(error.unwrap_range().start(), 10);
  /// ```
  #[inline]
  pub fn from_range(span: impl Into<SimpleSpan<O>>, hint: Hint) -> Self {
    Self::new(Lexeme::Range(span.into()), hint)
  }

  /// Returns a reference to the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 5),
  ///     "digit"
  /// );
  ///
  /// assert!(error.lexeme().is_char());
  /// ```
  #[inline]
  pub const fn lexeme(&self) -> &Lexeme<Char, O> {
    &self.lexeme
  }

  /// Returns a reference to the hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 5),
  ///     "expected digit"
  /// );
  ///
  /// assert_eq!(*error.hint(), "expected digit");
  /// ```
  #[inline]
  pub const fn hint(&self) -> &Hint {
    &self.hint
  }

  /// Returns a mutable reference to the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let mut error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 5),
  ///     "digit"
  /// );
  ///
  /// error.lexeme_mut().bump(&10);
  /// assert_eq!(error.unwrap_char().position(), 15);
  /// ```
  #[inline]
  pub const fn lexeme_mut(&mut self) -> &mut Lexeme<Char, O> {
    &mut self.lexeme
  }

  /// Returns a mutable reference to the hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let mut error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 5),
  ///     String::from("digit")
  /// );
  ///
  /// error.hint_mut().push_str(" or letter");
  /// assert_eq!(error.hint(), "digit or letter");
  /// ```
  #[inline]
  pub const fn hint_mut(&mut self) -> &mut Hint {
    &mut self.hint
  }

  /// Consumes self and returns the lexeme and hint as a tuple.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('!', 10),
  ///     "identifier"
  /// );
  ///
  /// let (lexeme, hint) = error.into_components();
  /// assert!(lexeme.is_char());
  /// assert_eq!(hint, "identifier");
  /// ```
  #[inline]
  pub fn into_components(self) -> (Lexeme<Char, O>, Hint) {
    (self.lexeme, self.hint)
  }

  /// Consumes self and returns only the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('!', 10),
  ///     "identifier"
  /// );
  ///
  /// let lexeme = error.into_lexeme();
  /// assert!(lexeme.is_char());
  /// ```
  #[inline]
  pub fn into_lexeme(self) -> Lexeme<Char, O> {
    self.lexeme
  }

  /// Consumes self and returns only the hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('!', 10),
  ///     "identifier"
  /// );
  ///
  /// let hint = error.into_hint();
  /// assert_eq!(hint, "identifier");
  /// ```
  #[inline]
  pub fn into_hint(self) -> Hint {
    self.hint
  }

  /// Returns the byte span covered by this lexeme using a custom length function.
  ///
  /// This delegates to [`Lexeme::span_with`].
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('€', 5),
  ///     "ASCII character"
  /// );
  ///
  /// let span = error.span_with(|c: &char| c.len_utf8());
  /// assert_eq!(span.start(), 5);
  /// assert_eq!(span.end(), 8); // '€' is 3 bytes
  /// ```
  #[inline]
  pub fn span_with(&self, len_of: impl FnOnce(&Char) -> usize) -> SimpleSpan<O>
  where
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    self.lexeme.span_with(len_of)
  }

  /// Returns the byte span covered by this lexeme.
  ///
  /// This delegates to [`Lexeme::span`].
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, SimpleSpan, utils::{PositionedChar}};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 10),
  ///     "digit"
  /// );
  ///
  /// assert_eq!(error.span(), SimpleSpan::new(10, 11));
  /// ```
  #[inline]
  pub fn span(&self) -> SimpleSpan<O>
  where
    Char: CharLen,
    O: Clone + Ord,
    for<'a> &'a O: Add<usize, Output = O>,
  {
    self.lexeme.span()
  }

  /// Maps the character type to another type, preserving the hint.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('a', 5),
  ///     "digit"
  /// );
  ///
  /// let upper = error.map_char(|c| c.to_ascii_uppercase());
  /// assert_eq!(upper.unwrap_char().char(), 'A');
  /// assert_eq!(*upper.hint(), "digit");
  /// ```
  #[inline]
  pub fn map_char<F, NewChar>(self, f: F) -> UnexpectedLexeme<NewChar, Hint, O>
  where
    F: FnMut(Char) -> NewChar,
  {
    UnexpectedLexeme {
      lexeme: self.lexeme.map(f),
      hint: self.hint,
    }
  }

  /// Maps the hint type to another type, preserving the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('!', 5),
  ///     "digit"
  /// );
  ///
  /// let detailed = error.map_hint(|h| format!("expected {}", h));
  /// assert_eq!(detailed.hint(), "expected digit");
  /// ```
  #[inline]
  pub fn map_hint<F, NewHint>(self, f: F) -> UnexpectedLexeme<Char, NewHint, O>
  where
    F: FnOnce(Hint) -> NewHint,
  {
    UnexpectedLexeme {
      lexeme: self.lexeme,
      hint: f(self.hint),
    }
  }

  /// Maps both the character and hint types to other types.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('a', 5),
  ///     "number"
  /// );
  ///
  /// let transformed = error.map(
  ///     |c| c.to_ascii_uppercase(),
  ///     |h| format!("expected {}", h)
  /// );
  ///
  /// assert_eq!(transformed.unwrap_char().char(), 'A');
  /// assert_eq!(transformed.hint(), "expected number");
  /// ```
  #[inline]
  pub fn map<F, NewChar, G, NewHint>(self, f: F, g: G) -> UnexpectedLexeme<NewChar, NewHint, O>
  where
    F: FnMut(Char) -> NewChar,
    G: FnOnce(Hint) -> NewHint,
  {
    UnexpectedLexeme {
      lexeme: self.lexeme.map(f),
      hint: g(self.hint),
    }
  }

  /// Adjusts the lexeme's position/span by adding `n` bytes.
  ///
  /// Returns a mutable reference to self for method chaining.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{error::UnexpectedLexeme, utils::PositionedChar};
  ///
  /// let mut error = UnexpectedLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 5),
  ///     "digit"
  /// );
  ///
  /// error.bump(&10);
  /// assert_eq!(error.unwrap_char().position(), 15);
  /// ```
  #[inline]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> AddAssign<&'a O> + Clone,
  {
    self.lexeme.bump(n);
    self
  }
}
