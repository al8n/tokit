//! Missing token error type for parser error reporting.
//!
//! This module provides the [`MissingToken`] type, which represents parser errors
//! when a missing token is encountered. It captures both the location of the error,
//! what tokens were expected, and an optional message.
//!
//! # Design Philosophy
//!
//! `MissingToken` is designed to provide rich, actionable error messages:
//! - **Location tracking**: The `offset` field pinpoints exactly where the error occurred
//! - **Flexible expectations**: Can express single or multiple alternative expected tokens
//! - **Position adjustment**: The `bump()` method allows adjusting error positions when
//!   combining errors from different parsing contexts
//!
//! # Common Patterns
//!
//! ## End of Input Errors
//!
//! When the parser reaches the end of input with a missing token, use constructors without a found token:
//!
//! ```
//! use tokora::{SimpleSpan, error::token::MissingToken};
//!
//! // Simple missing token error
//! let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one(
//!     SimpleSpan::new(100, 100),
//!     "}"
//! );
//! assert_eq!(error.offset(), SimpleSpan::new(100, 100));
//! ```
//!
//! ## Unexpected Token Errors
//!
//! When a specific token was found but something else was expected:
//!
//! ```
//! use tokora::{SimpleSpan, utils::Expected, error::token::MissingToken};
//!
//! let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one(
//!     SimpleSpan::new(10, 15),
//!     "else"
//! );
//! assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "else"));
//! ```

use core::{marker::PhantomData, ops::AddAssign};

use crate::{
  Lexer, Token,
  utils::{CowStr, Expected},
};

/// A type alias for a `MissingToken` error for a given lexer and separator.
pub type MissingTokenOf<'inp, L, Lang = ()> = MissingToken<
  'inp,
  <<L as Lexer<'inp>>::Token as Token<'inp>>::Kind,
  <L as Lexer<'inp>>::Offset,
  Lang,
>;

/// An error representing a missing token encountered during parsing.
///
/// This error type captures the location (offset) and what token(s) were expected.
/// It's commonly used in parsers to provide
/// detailed error messages when the input doesn't match the expected syntax.
///
/// Three optional channels ride alongside the offset, each answering a different question:
/// [`expected`](Self::expected) is machine-readable (token *kinds*), [`message`](Self::message)
/// is the caller's free text, and [`name`](Self::name) is the human-readable name of the token
/// this error is about — what a separated-sequence driver calls its separator, for instance.
/// They are separate because a conversion that needs to stamp one must not have to choose
/// between clobbering another and dropping its own information.
///
/// # Type Parameters
///
/// * `T` - The type of the actual token that was found
/// * `Kind` - The type of the expected token (often an enum of token kinds)
///
/// # Examples
///
/// ```
/// use tokora::{SimpleSpan, utils::Expected, error::token::MissingToken};
///
/// // Error when expecting a specific token
/// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one(
///     SimpleSpan::new(10, 15),
///     "}"
/// );
/// assert_eq!(error.offset(), SimpleSpan::new(10, 15));
///
/// // Error when expecting one of multiple tokens
/// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one_of(
///     SimpleSpan::new(0, 10),
///     &["if", "while", "for"]
/// );
/// if let Some(Expected::OneOf(values)) = error.expected() {
///     assert_eq!(values.as_slice(), &["if", "while", "for"]);
/// }
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MissingToken<'a, Kind: Clone, O = usize, Lang: ?Sized = ()> {
  offset: O,
  expected: Option<Expected<'a, Kind>>,
  message: Option<CowStr>,
  name: Option<CowStr>,
  _lang: PhantomData<Lang>,
}

impl<Kind: Clone, O> MissingToken<'_, Kind, O> {
  /// Creates a new missing token error.
  ///
  /// This error indicates that a missing token was encountered,
  /// without specifying what token was found or expected.
  #[inline]
  pub const fn new(offset: O) -> Self {
    Self::of(offset)
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> MissingToken<'a, Kind, O, Lang> {
  #[inline]
  pub(super) const fn new_in(
    offset: O,
    expected: Option<Expected<'a, Kind>>,
    message: Option<CowStr>,
    name: Option<CowStr>,
  ) -> Self {
    Self {
      offset,
      expected,
      message,
      name,
      _lang: PhantomData,
    }
  }

  /// Creates a new missing token error.
  ///
  /// This error indicates that a missing token was encountered,
  /// without specifying what token was found or expected.
  #[inline]
  pub const fn of(offset: O) -> Self {
    Self::new_in(offset, None, None, None)
  }

  /// Adds knowledge to the `MissingToken` error.
  ///
  /// This method allows attaching additional context or information
  /// to the error, which can be useful for debugging or reporting.
  #[inline]
  pub fn with_message(self, message: CowStr) -> Self {
    Self::new_in(self.offset, self.expected, Some(message), self.name)
  }

  /// Stamps the human-readable name of the token this error is about — the separator name a
  /// separated-sequence driver supplied, for the errors the separator conversions produce.
  ///
  /// A channel of its own rather than a phrasing pushed into
  /// [`with_message`](Self::with_message): the message is the caller's free text, and a
  /// conversion that overwrote it would lose whatever the caller meant by it — while a
  /// conversion that declined to overwrite it would lose the name instead. Keeping the two
  /// apart also keeps the name safe from [`with_expected`](Self::with_expected), which clears
  /// the message channel.
  #[inline]
  pub fn with_name(self, name: CowStr) -> Self {
    Self::new_in(self.offset, self.expected, self.message, Some(name))
  }

  /// Creates a missing token error without a found token.
  ///
  /// This is useful when the parser reaches the end of input with a missing token.
  /// The error will indicate "missing end of input" in its display message.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Expected, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, usize> = MissingToken::new(
  ///     100,
  /// ).with_expected(Expected::one("}"));
  /// assert_eq!(error.offset(), 100);
  /// if let Some(Expected::One(value)) = error.expected() {
  ///     assert_eq!(*value, "}");
  /// }
  /// ```
  #[inline]
  pub fn with_expected(self, expected: Expected<'a, Kind>) -> Self {
    Self::new_in(self.offset, Some(expected), None, self.name)
  }

  /// Creates a new missing token error with a single expected token.
  ///
  /// This is a convenience method that combines `new` with `Expected::one`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one(
  ///     SimpleSpan::new(50, 51),
  ///     ";"
  /// );
  /// assert_eq!(error.offset(), SimpleSpan::new(50, 51));
  /// ```
  #[inline]
  pub const fn expected_one(offset: O, expected: Kind) -> Self {
    Self::new_in(offset, Some(Expected::one(expected)), None, None)
  }

  /// Creates a new missing token error with a single expected token.
  ///
  /// This is a convenience method that combines `new` with `Expected::one`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one_with_found(
  ///     SimpleSpan::new(50, 51),
  ///     ";"
  /// );
  /// assert_eq!(error.offset(), SimpleSpan::new(50, 51));
  /// ```
  #[inline]
  pub const fn expected_one_with_found(offset: O, expected: Kind) -> Self {
    Self::new_in(offset, Some(Expected::one(expected)), None, None)
  }

  /// Creates a new missing token error with multiple expected tokens.
  ///
  /// This is a convenience method that combines `new` with `Expected::one_of`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```ignore
  /// use tokora::{SimpleSpan, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one_of(
  ///     SimpleSpan::new(25, 26),
  ///     &["+", "-", "*", "/"]
  /// );
  /// assert_eq!(error.offset(), SimpleSpan::new(25, 26));
  /// ```
  #[inline]
  pub const fn expected_one_of(offset: O, expected: &'static [Kind]) -> Self {
    Self::new_in(offset, Some(Expected::one_of(expected)), None, None)
  }

  /// Creates a new missing token error with multiple expected tokens.
  ///
  /// This is a convenience method that combines `new` with `Expected::one_of`.
  /// The error has no found token, indicating the end of input was reached.
  ///
  /// # Examples
  ///
  /// ```ignore
  /// use tokora::{SimpleSpan, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one_of_with_found(
  ///     SimpleSpan::new(25, 26),
  ///     &["+", "-", "*", "/"]
  /// );
  /// assert_eq!(error.offset(), SimpleSpan::new(25, 26));
  /// ```
  #[inline]
  pub const fn expected_one_of_with_found(offset: O, expected: &'static [Kind]) -> Self {
    Self::new_in(offset, Some(Expected::one_of(expected)), None, None)
  }

  /// Returns the offset of the missing token.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one(
  ///     SimpleSpan::new(10, 15),
  ///     "identifier"
  /// );
  /// assert_eq!(error.offset(), SimpleSpan::new(10, 15));
  /// ```
  #[inline]
  pub const fn offset(&self) -> O
  where
    O: Copy,
  {
    self.offset
  }

  /// Returns the offset of the missing token.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::error::token::MissingToken;
  ///
  /// let error: MissingToken<'_, &str> = MissingToken::expected_one(10, "identifier");
  /// assert_eq!(error.offset_ref(), &10);
  /// ```
  #[inline]
  pub const fn offset_ref(&self) -> &O {
    &self.offset
  }

  /// Returns the offset of the missing token.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::error::token::MissingToken;
  ///
  /// let mut error: MissingToken<'_, &str> = MissingToken::expected_one(10, "identifier");
  /// *error.offset_mut() = 12;
  /// assert_eq!(error.offset(), 12);
  /// ```
  #[inline]
  pub const fn offset_mut(&mut self) -> &mut O {
    &mut self.offset
  }

  /// Returns a reference to the custom message, if any.
  #[inline]
  pub const fn message(&self) -> Option<&CowStr> {
    self.message.as_ref()
  }

  /// Returns a mutable reference to the custom message, if any.
  #[inline]
  pub fn message_mut(&mut self) -> Option<&mut CowStr> {
    self.message.as_mut()
  }

  /// Returns the stamped token name, if any — see [`with_name`](Self::with_name).
  #[inline]
  pub const fn name(&self) -> Option<&CowStr> {
    self.name.as_ref()
  }

  /// Returns a reference to the expected token(s).
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::Expected, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, SimpleSpan> = MissingToken::expected_one(SimpleSpan::new(5, 6), "}");
  /// assert!(matches!(error.expected(), Some(Expected::One(value)) if *value == "}"));
  /// ```
  #[inline]
  pub const fn expected(&self) -> Option<&Expected<'a, Kind>> {
    self.expected.as_ref()
  }

  /// Bumps the offset by the given amount.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining offsets from different contexts.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::error::token::MissingToken;
  ///
  /// let mut error: MissingToken<'_, &str> = MissingToken::expected_one(10, "}");
  /// error.bump(&5);
  /// assert_eq!(error.offset(), 15);
  /// ```
  #[inline]
  pub fn bump(&mut self, offset: &O)
  where
    O: for<'b> AddAssign<&'b O>,
  {
    self.offset += offset;
  }

  /// Maps the expected token(s) using the provided function.
  ///
  /// This is useful for transforming the expected token type while preserving
  /// the rest of the error information.
  ///
  /// ## Examples
  ///
  /// ```
  /// # #[cfg(feature = "std")] {
  /// use tokora::{utils::Expected, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str> = MissingToken::expected_one(0, "identifier");
  /// let mapped_error = error.map_expected(|expected| {
  ///     // Transform the expected token type here
  ///     Expected::one(expected.unwrap_one().to_string())
  /// });
  /// # }
  /// ```
  pub fn map_expected<F, Kind2>(self, f: F) -> MissingToken<'a, Kind2, O, Lang>
  where
    F: FnOnce(Expected<'a, Kind>) -> Expected<'a, Kind2>,
    Kind2: Clone,
  {
    MissingToken {
      offset: self.offset,
      expected: self.expected.map(f),
      message: self.message,
      name: self.name,
      _lang: PhantomData,
    }
  }

  /// Consumes the error and returns its components: the offset, the expected token(s), the
  /// optional message, and the stamped token [`name`](Self::name), in field order.
  ///
  /// The name is returned rather than dropped for the same reason
  /// [`SeparatedError::into_components`](crate::error::token::SeparatedError::into_components)
  /// returns its own: a downstream `From<MissingToken>` impl that takes the error apart is
  /// precisely the consumer the separator conversions stamp the name for, and those
  /// conversions are blanket impls such a type cannot override. A tuple that omitted the name
  /// would simply move the loss one seam further along.
  ///
  /// The message and the name are both `Option<CowStr>` but are distinct channels — the
  /// caller's free text and the token's own name — and arrive in that order.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::{SimpleSpan, utils::{CowStr, Expected}, error::token::MissingToken};
  ///
  /// let error: MissingToken<'_, &str, SimpleSpan> =
  ///     MissingToken::expected_one(SimpleSpan::new(5, 6), "}").with_name(CowStr::from_static("brace"));
  /// let (offset, expected, message, name) = error.into_components();
  /// assert_eq!(offset, SimpleSpan::new(5, 6));
  /// assert_eq!(expected, Some(Expected::one("}")));
  /// assert_eq!(message, None);
  /// assert_eq!(name.as_ref().map(CowStr::as_str), Some("brace"));
  /// ```
  #[inline]
  pub fn into_components(
    self,
  ) -> (
    O,
    Option<Expected<'a, Kind>>,
    Option<CowStr>,
    Option<CowStr>,
  ) {
    (self.offset, self.expected, self.message, self.name)
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for () {
  #[inline]
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {}
}

impl<Kind: Clone, O, Lang: ?Sized> MissingToken<'_, Kind, O, Lang> {
  /// Formats the error using the provided formatter in debug style.
  #[inline]
  pub fn debug_fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result
  where
    O: core::fmt::Debug,
    Kind: core::fmt::Debug,
  {
    f.debug_struct("MissingToken")
      .field("offset", &self.offset)
      .field("expected", &self.expected)
      .field("message", &self.message)
      .field("name", &self.name)
      .finish()
  }

  /// Formats the error using the provided formatter in display style.
  ///
  /// A stamped [`name`](Self::name) is quoted into the opening clause — `missing token
  /// 'comma' at 12` — matching how the rest of the crate renders a token's own name
  /// (`unclosed delimiter '('`, `unopened delimiter ')'`). Naming it there rather than
  /// appending a clause is what makes the separator conversions' stamp reach a reader: the
  /// point of the channel is that the diagnostic says *which* separator was missing, not that
  /// one was.
  ///
  /// Without a name the rendering is byte-for-byte what it always was, so the channel is
  /// purely additive for every error that never carried one.
  #[inline]
  pub fn display_fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result
  where
    O: core::fmt::Display,
    Kind: core::fmt::Display,
  {
    match &self.name {
      Some(name) => write!(f, "missing token '{}' at {}", name, self.offset)?,
      None => write!(f, "missing token at {}", self.offset)?,
    }
    if let Some(expected) = &self.expected {
      // NOT `", expected {}"`: `Expected`'s own `Display` opens with the word in *every*
      // variant (`expected '}'`, `expected one of: …`), so composing one here rendered
      // "expected expected '}'". The inner Display always supplies the word, so the composing
      // site supplies only the separator.
      write!(f, ", {}", expected)?;
    }
    if let Some(message) = &self.message {
      write!(f, ", message: {}", message)?;
    }
    Ok(())
  }
}
