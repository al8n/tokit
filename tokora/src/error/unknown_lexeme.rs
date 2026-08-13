use core::ops::{Add, AddAssign};

use crate::{
  span::SimpleSpan,
  utils::{CharLen, Lexeme, PositionedChar, human_display::DisplayHuman},
};

/// A zero-copy error structure combining an unrecognized lexeme with diagnostic knowledge.
///
/// `UnknownLexeme` pairs a [`Lexeme`] (identifying the unrecognized fragment) with knowledge
/// (providing context about valid options or additional diagnostic information). This structure
/// is designed for building rich, informative error messages without allocating strings.
///
/// # Type Parameters
///
/// - **Char**: The character type (typically `char` for UTF-8 or `u8` for bytes)
/// - **Knowledge**: Any type providing diagnostic context (often implements `Display`)
///
/// # Design Philosophy
///
/// This type stores:
/// - The **lexeme** of the unrecognized fragment ([`Char`](Lexeme::Char) or [`SimpleSpan`](Lexeme::Range))
/// - **Knowledge** providing context about valid options or diagnostic information (any type you choose)
///
/// The knowledge is left generic and unconstrained so you can carry:
/// - Simple strings: `&'static str`
/// - Valid token kinds: Your own `TokenKind` enum
/// - Rich structures: Custom diagnostic types with multiple suggestions
///
/// # Deref Behavior
///
/// `UnknownLexeme` implements `Deref` to `Lexeme<Char, O>`, so you can call all
/// `Lexeme` methods directly on an `UnknownLexeme` instance.
///
/// # Use Cases
///
/// - **Lexer errors**: Report unrecognized characters with contextual information
/// - **Parser errors**: Track unknown tokens with diagnostic knowledge
/// - **Error recovery**: Store partial error info without allocating
/// - **Diagnostic tools**: Build structured error reports for IDEs
///
/// # Examples
///
/// ## Basic Error with String Knowledge
///
/// ```rust
/// use tokora::{utils::PositionedChar, error::UnknownLexeme};
///
/// let error = UnknownLexeme::from_positioned_char(
///     PositionedChar::with_position('£', 42),
///     "valid characters: letters, digits, or '_'"
/// );
///
/// assert!(error.is_char());
/// assert_eq!(error.lexeme().unwrap_char().position(), 42);
/// assert_eq!(*error.knowledge(), "valid characters: letters, digits, or '_'");
/// ```
///
/// ## With Token Kind Knowledge
///
/// ```rust,ignore
/// use tokora::{SimpleSpan, error::UnknownLexeme};
///
/// #[derive(Debug, Clone)]
/// enum ValidTokens {
///     Single(TokenKind),
///     Multiple(Vec<TokenKind>),
/// }
///
/// let error = UnknownLexeme::from_range(
///     SimpleSpan::new(10, 15),
///     ValidTokens::Multiple(vec![TokenKind::Identifier, TokenKind::Keyword])
/// );
///
/// // Use in error display
/// match error.knowledge() {
///     ValidTokens::Single(kind) => println!("Valid token: {:?}", kind),
///     ValidTokens::Multiple(kinds) => println!("Valid tokens: {:?}", kinds),
/// }
/// ```
///
/// ## Mapping Knowledge
///
/// ```rust
/// use tokora::{utils::PositionedChar, error::UnknownLexeme};
///
/// let error = UnknownLexeme::from_positioned_char(
///     PositionedChar::with_position('@', 5),
///     "digit"
/// );
///
/// // Transform the knowledge to a more detailed message
/// let detailed = error.map_knowledge(|knowledge| format!("unrecognized character, valid: {}", knowledge));
///
/// assert_eq!(detailed.knowledge(), "unrecognized character, valid: digit");
/// ```
///
/// ## Accessing Lexeme via Deref
///
/// ```rust
/// use tokora::{utils::PositionedChar, error::UnknownLexeme};
///
/// let error = UnknownLexeme::from_positioned_char(
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
pub struct UnknownLexeme<Char, Knowledge, O = usize> {
  lexeme: Lexeme<Char, O>,
  knowledge: Knowledge,
}

impl<Char, Knowledge, O> core::fmt::Display for UnknownLexeme<Char, Knowledge, O>
where
  Char: DisplayHuman,
  O: core::fmt::Display,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self.lexeme() {
      Lexeme::Char(pc) => write!(
        f,
        "unknown character '{}' encountered at {}",
        pc.char_ref().display(),
        pc.position_ref(),
      ),
      Lexeme::Range(span) => write!(f, "unknown lexeme encountered at {}", span),
    }
  }
}

impl<Char, Knowledge, O> core::error::Error for UnknownLexeme<Char, Knowledge, O>
where
  Char: DisplayHuman + core::fmt::Debug,
  Knowledge: core::fmt::Debug,
  O: core::fmt::Debug + core::fmt::Display,
{
}

impl<Char, Knowledge, O> core::ops::Deref for UnknownLexeme<Char, Knowledge, O> {
  type Target = Lexeme<Char, O>;

  #[inline]
  fn deref(&self) -> &Self::Target {
    &self.lexeme
  }
}

impl<Char, Knowledge, O> core::ops::DerefMut for UnknownLexeme<Char, Knowledge, O> {
  #[inline]
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.lexeme
  }
}

impl<Char, O> UnknownLexeme<Char, crate::utils::knowledge::Characters, O> {
  /// Creates an `UnknownLexeme` with character knowledge.
  ///
  /// This is a convenience method for cases where no specific knowledge is provided.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, utils::{knowledge::Characters}, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::<char, Characters>::unknown_characters(
  ///     SimpleSpan::new(7, 9)     
  /// );
  ///
  /// assert!(!error.is_char());
  /// assert_eq!(error.unwrap_range().start(), 7);
  /// ```
  #[inline]
  pub const fn unknown_characters(span: SimpleSpan<O>) -> Self {
    Self::new(Lexeme::Range(span), sealed::Sealed::INIT)
  }

  /// Creates an `UnknownLexeme` with character knowledge.
  ///
  /// This is a convenience method for cases where no specific knowledge is provided.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::unknown_character(
  ///     7, '#'
  /// );
  ///
  /// assert!(error.is_char());
  /// assert_eq!(error.unwrap_char().position(), 7);
  /// ```
  #[inline]
  pub const fn unknown_character(pos: O, ch: Char) -> Self {
    Self::from_char(pos, ch, sealed::Sealed::INIT)
  }
}

impl<Char, Knowledge, O> UnknownLexeme<Char, Knowledge, O> {
  /// Creates a new `UnknownLexeme` from a lexeme and diagnostic knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::{Lexeme, PositionedChar}, error::UnknownLexeme};
  ///
  /// let lexeme = Lexeme::from(PositionedChar::with_position('§', 5));
  /// let error = UnknownLexeme::new(lexeme, "valid: identifier");
  ///
  /// assert_eq!(*error.knowledge(), "valid: identifier");
  /// ```
  #[inline]
  pub const fn new(lexeme: Lexeme<Char, O>, knowledge: Knowledge) -> Self {
    Self { lexeme, knowledge }
  }

  /// Constructs an error from a positioned character and diagnostic knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('$', 42),
  ///     "valid: alphanumeric character"
  /// );
  ///
  /// assert!(error.is_char());
  /// assert_eq!(error.unwrap_char().position(), 42);
  /// ```
  #[inline]
  pub const fn from_char(pos: O, ch: Char, knowledge: Knowledge) -> Self {
    Self::from_positioned_char(PositionedChar::with_position(ch, pos), knowledge)
  }

  /// Constructs an error from a positioned character and diagnostic knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('$', 42),
  ///     "valid: alphanumeric character"
  /// );
  ///
  /// assert!(error.is_char());
  /// assert_eq!(error.unwrap_char().position(), 42);
  /// ```
  #[inline]
  pub const fn from_positioned_char(pc: PositionedChar<Char, O>, knowledge: Knowledge) -> Self {
    Self::new(Lexeme::Char(pc), knowledge)
  }

  /// Constructs an error from a byte span and diagnostic knowledge (const version).
  ///
  /// Use this in const contexts where `Into<SimpleSpan>` conversions aren't available.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, error::UnknownLexeme};
  ///
  /// let error: UnknownLexeme<char, _> = UnknownLexeme::from_range_const(
  ///     SimpleSpan::new(10, 15),
  ///     "valid: semicolon"
  /// );
  ///
  /// assert!(error.is_range());
  /// ```
  #[inline]
  pub const fn from_range_const(span: SimpleSpan<O>, knowledge: Knowledge) -> Self {
    Self::new(Lexeme::Range(span), knowledge)
  }

  /// Constructs an error from a byte span and diagnostic knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::error::UnknownLexeme;
  ///
  /// let error: UnknownLexeme<char, _> = UnknownLexeme::from_range(10..15, "valid: closing brace");
  ///
  /// assert!(error.is_range());
  /// assert_eq!(error.unwrap_range().start(), 10);
  /// ```
  #[inline]
  pub fn from_range(span: impl Into<SimpleSpan<O>>, knowledge: Knowledge) -> Self {
    Self::new(Lexeme::Range(span.into()), knowledge)
  }

  /// Returns a reference to the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
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

  /// Returns a reference to the diagnostic knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 5),
  ///     "valid: digit"
  /// );
  ///
  /// assert_eq!(*error.knowledge(), "valid: digit");
  /// ```
  #[inline]
  pub const fn knowledge(&self) -> &Knowledge {
    &self.knowledge
  }

  /// Returns a mutable reference to the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let mut error = UnknownLexeme::from_positioned_char(
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

  /// Returns a mutable reference to the diagnostic knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let mut error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('x', 5),
  ///     String::from("valid: digit")
  /// );
  ///
  /// error.knowledge_mut().push_str(" or letter");
  /// assert_eq!(error.knowledge(), "valid: digit or letter");
  /// ```
  #[inline]
  pub const fn knowledge_mut(&mut self) -> &mut Knowledge {
    &mut self.knowledge
  }

  /// Consumes self and returns the lexeme and knowledge as a tuple.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('!', 10),
  ///     "identifier"
  /// );
  ///
  /// let (lexeme, knowledge) = error.into_components();
  /// assert!(lexeme.is_char());
  /// assert_eq!(knowledge, "identifier");
  /// ```
  #[inline]
  pub fn into_components(self) -> (Lexeme<Char, O>, Knowledge) {
    (self.lexeme, self.knowledge)
  }

  /// Consumes self and returns only the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
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

  /// Consumes self and returns only the knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('!', 10),
  ///     "identifier"
  /// );
  ///
  /// let knowledge = error.into_knowledge();
  /// assert_eq!(knowledge, "identifier");
  /// ```
  #[inline]
  pub fn into_knowledge(self) -> Knowledge {
    self.knowledge
  }

  /// Returns the byte span covered by this lexeme using a custom length function.
  ///
  /// This delegates to [`Lexeme::span_with`].
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
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
  /// use tokora::{SimpleSpan, utils::{PositionedChar}, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
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

  /// Maps the character type to another type, preserving the knowledge.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('a', 5),
  ///     "digit"
  /// );
  ///
  /// let upper = error.map_char(|c| c.to_ascii_uppercase());
  /// assert_eq!(upper.unwrap_char().char(), 'A');
  /// assert_eq!(*upper.knowledge(), "digit");
  /// ```
  #[inline]
  pub fn map_char<F, NewChar>(self, f: F) -> UnknownLexeme<NewChar, Knowledge, O>
  where
    F: FnMut(Char) -> NewChar,
  {
    UnknownLexeme {
      lexeme: self.lexeme.map(f),
      knowledge: self.knowledge,
    }
  }

  /// Maps the knowledge type to another type, preserving the lexeme.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('!', 5),
  ///     "digit"
  /// );
  ///
  /// let detailed = error.map_knowledge(|h| format!("unrecognized, valid: {}", h));
  /// assert_eq!(detailed.knowledge(), "unrecognized, valid: digit");
  /// ```
  #[inline]
  pub fn map_knowledge<F, NewKnowledge>(self, f: F) -> UnknownLexeme<Char, NewKnowledge, O>
  where
    F: FnOnce(Knowledge) -> NewKnowledge,
  {
    UnknownLexeme {
      lexeme: self.lexeme,
      knowledge: f(self.knowledge),
    }
  }

  /// Maps both the character and knowledge types to other types.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let error = UnknownLexeme::from_positioned_char(
  ///     PositionedChar::with_position('a', 5),
  ///     "number"
  /// );
  ///
  /// let transformed = error.map(
  ///     |c| c.to_ascii_uppercase(),
  ///     |h| format!("unrecognized, valid: {}", h)
  /// );
  ///
  /// assert_eq!(transformed.unwrap_char().char(), 'A');
  /// assert_eq!(transformed.knowledge(), "unrecognized, valid: number");
  /// ```
  #[inline]
  pub fn map<F, NewChar, G, NewKnowledge>(
    self,
    f: F,
    g: G,
  ) -> UnknownLexeme<NewChar, NewKnowledge, O>
  where
    F: FnMut(Char) -> NewChar,
    G: FnOnce(Knowledge) -> NewKnowledge,
  {
    UnknownLexeme {
      lexeme: self.lexeme.map(f),
      knowledge: g(self.knowledge),
    }
  }

  /// Adjusts the lexeme's position/span by adding `n` bytes.
  ///
  /// Returns a mutable reference to self for method chaining.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{utils::PositionedChar, error::UnknownLexeme};
  ///
  /// let mut error = UnknownLexeme::from_positioned_char(
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

/// A marker trait for types that may represent unknown lexemes.
pub trait MaybeUnknown: sealed::Sealed {}

impl<T> MaybeUnknown for T where T: sealed::Sealed {}

mod sealed {
  use crate::utils::knowledge::Characters;

  pub trait Sealed {
    const INIT: Self;
  }

  impl Sealed for Characters {
    const INIT: Self = Self(());
  }
}
