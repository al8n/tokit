use core::marker::PhantomData;

use derive_more::Display;

use crate::utils::{CowStr, Expected};

/// A zero-sized marker indicating the parser expected more bytes when the file ended.
///
/// This hint type is used with [`UnexpectedEnd`] to create natural-reading error messages
/// like: `"unexpected end of file, expected byte"`.
///
/// # Use Case
///
/// Use `FileHint` when lexing byte-oriented input (files, byte streams) and you reach EOF
/// unexpectedly.
///
/// # Example
///
/// ```rust
/// use tokora::error::UnexpectedEnd;
///
/// let error = UnexpectedEnd::eof(100);
/// assert_eq!(error.to_string(), "unexpected end of file, expected byte");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display("byte")]
pub struct FileHint;

/// A zero-sized marker indicating the parser expected more tokens when the stream ended.
///
/// This hint type is used with [`UnexpectedEnd`] to create natural-reading error messages
/// like: `"unexpected end of token stream, expected token"`.
///
/// # Use Case
///
/// Use `TokenHint` when parsing a token stream with Chumsky and you reach end-of-tokens
/// unexpectedly.
///
/// # Example
///
/// ```rust
/// use tokora::error::UnexpectedEnd;
///
/// let error = UnexpectedEnd::eot(100);
/// assert_eq!(error.to_string(), "unexpected end of token stream, expected token");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display("token")]
pub struct TokenHint;

/// A zero-sized marker indicating the parser expected more characters when the string ended.
///
/// This hint type is used with [`UnexpectedEnd`] to create natural-reading error messages
/// like: `"unexpected end of string, expected character"`.
///
/// # Use Case
///
/// Use `CharacterHint` when parsing character-by-character and you reach end-of-string
/// unexpectedly.
///
/// # Example
///
/// ```rust
/// use tokora::error::UnexpectedEnd;
///
/// let error = UnexpectedEnd::eos(100);
/// assert_eq!(error.to_string(), "unexpected end of string, expected character");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display("character")]
pub struct CharacterHint;

/// A zero-sized marker indicating the parser expected a right hand side of an expression.
///
/// This hint type is used with [`UnexpectedEnd`] to create natural-reading error messages
/// like: `"unexpected end of expression, expected either an infix or a postfix"`.
///
/// # Example
///
/// ```rust
/// use tokora::error::{UnexpectedEnd, PrattRhsHint};
///
/// let error = UnexpectedEnd::eorhs(100);
/// assert_eq!(error.to_string(), "unexpected end of expression (right hand side), expected either an infix or a postfix");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display("either an infix or a postfix")]
pub struct PrattRhsHint;

/// A zero-sized marker indicating the parser expected a right hand side of an expression.
///
/// This hint type is used with [`UnexpectedEnd`] to create natural-reading error messages
/// like: `"unexpected end of expression, expected one of an operand, an infix or a postfix"`.
///
/// # Example
///
/// ```rust
/// use tokora::error::{UnexpectedEnd, PrattLhsHint};
///
/// let error = UnexpectedEnd::eolhs(100);
/// assert_eq!(error.to_string(), "unexpected end of expression (left hand side), expected one of an operand, an infix or a postfix");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display("one of an operand, an infix or a postfix")]
pub struct PrattLhsHint;

/// A zero-copy, composable error type for unexpected end-of-input conditions.
///
/// `UnexpectedEnd` represents situations where the parser or lexer expected more input
/// but encountered the end of the stream instead (EOF, EOT, EOS, etc.). It's designed to:
///
/// - Avoid allocations by using [`CowStr`] for names
/// - Provide natural-reading error messages
/// - Be composable with custom hint types
/// - Implement `Error` trait for standard error handling
///
/// # Type Parameter
///
/// - `Hint`: The type describing what was expected. Typically one of:
///   - [`FileHint`]: Expected more bytes in a file
///   - [`TokenHint`]: Expected more tokens in a stream
///   - [`CharacterHint`]: Expected more characters in a string
///   - Custom types implementing `Display` for domain-specific hints
///
/// # Components
///
/// 1. **Name** (`Option<CowStr>`): What ended (e.g., "file", "block comment")
/// 2. **Hint** (generic `Hint`): What was expected next
///
/// Together, these create error messages like:
/// - `"unexpected end of file, expected byte"`
/// - `"unexpected end of block comment, expected */"`
/// - `"unexpected end, expected closing brace"`
///
/// # Zero-Copy Design
///
/// `UnexpectedEnd` uses [`CowStr`] for the name field, which means:
/// - Static strings (`&'static str`) involve no allocation
/// - Dynamic strings (`String`) are only allocated when necessary
/// - Most common cases (EOF, EOT, EOS) use compile-time constants
///
/// # Examples
///
/// ## Using Convenience Constructors
///
/// ```rust
/// use tokora::{error::{UnexpectedEnd, UnexpectedEof, UnexpectedEot}};
///
/// // Unexpected end of file at position 100
/// let eof = UnexpectedEnd::eof(100);
/// assert_eq!(eof.to_string(), "unexpected end of file, expected byte");
/// assert_eq!(eof.offset(), 100);
///
/// // Unexpected end of token stream at position 50
/// let eot = UnexpectedEnd::eot(50);
/// assert_eq!(eot.to_string(), "unexpected end of token stream, expected token");
/// assert_eq!(eot.offset(), 50);
/// ```
///
/// ## Custom Names and Hints
///
/// ```rust,ignore
/// use tokora::error::UnexpectedEnd;
/// use std::borrow::Cow;
///
/// // Custom hint type for SQL parsing
/// #[derive(Debug)]
/// struct SqlHint(&'static str);
///
/// impl std::fmt::Display for SqlHint {
///     fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
///         write!(f, "{}", self.0)
///     }
/// }
///
/// let error = UnexpectedEnd::with_name(
///     Cow::Borrowed("SELECT statement"),
///     SqlHint("FROM clause")
/// );
///
/// assert_eq!(
///     error.to_string(),
///     "unexpected end of SELECT statement, expected FROM clause"
/// );
/// ```
///
/// ## Transforming Hints
///
/// ```rust,ignore
/// use tokora::error::{UnexpectedEnd, FileHint};
///
/// let file_error: UnexpectedEnd<FileHint> = UnexpectedEnd::EOF;
///
/// // Map the hint to a more specific type
/// let custom_error = file_error.map_hint(|_| "closing brace");
///
/// assert_eq!(
///     custom_error.to_string(),
///     "unexpected end of file, expected closing brace"
/// );
/// ```
///
/// ## Error Handling
///
/// ```rust,ignore
/// use tokora::error::UnexpectedEof;
/// use std::error::Error;
///
/// fn parse_config(input: &str) -> Result<Config, Box<dyn Error>> {
///     // ... parsing logic ...
///
///     if input.is_empty() {
///         return Err(Box::new(UnexpectedEof::EOF));
///     }
///
///     Ok(config)
/// }
/// ```
/// # The optional expected-set (`Set`)
///
/// The final type parameter `Set` is the element type of an **optional** end-of-input expected
/// set — the "…, expected one of X, Y, Z" table a committed dispatch failure at end of input can
/// attach (see [`DispatchOnKind`](crate::parser::DispatchOnKind)). It is purely additive: `Set`
/// defaults to `&'static str` and every existing constructor leaves the set `None`, so the common
/// end-of-input errors (`eof`/`eot`/`eos`) and the [type aliases](UnexpectedEof) are unchanged.
/// The set is populated only through the dedicated `*_expected_*` constructors and read back
/// through [`expected`](Self::expected).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnexpectedEnd<Hint, O = usize, Lang: ?Sized = (), Set = &'static str>
where
  Set: Clone + 'static,
{
  offset: O,
  name: Option<CowStr>,
  hint: Hint,
  /// The optional end-of-input expected set (`None` for every legacy constructor). Stored with a
  /// `'static` set lifetime because the expected tables that feed it are `&'static` (static
  /// dispatch tables), which keeps `Set` a single trailing type parameter rather than forcing a
  /// lifetime onto the whole type.
  expected: Option<Expected<'static, Set>>,
  /// Whether this end-of-input error stands in for a **terminal scanner stop** (a resource-limit
  /// trip, or the poison boundary it latches) rather than a genuine end of input. `false` for every
  /// ordinary constructor; raised only through [`into_terminal`](Self::into_terminal). Read back
  /// through [`is_terminal`](Self::is_terminal) — the signal recovery keys off via
  /// [`MaybeTerminal`](crate::error::MaybeTerminal) to re-raise the stop instead of recovering it.
  terminal: bool,
  _lang: PhantomData<Lang>,
}

impl<Hint, O, Lang> core::fmt::Display for UnexpectedEnd<Hint, O, Lang>
where
  Hint: core::fmt::Display,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self.name() {
      Some(name) => write!(f, "unexpected end of {name}, expected {}", self.hint),
      None => write!(f, "unexpected end, expected {}", self.hint),
    }
  }
}

impl<Hint, O, Lang> core::error::Error for UnexpectedEnd<Hint, O, Lang>
where
  Hint: core::fmt::Debug + core::fmt::Display,
  O: core::fmt::Debug,
  Lang: core::fmt::Debug,
{
}

impl<O> UnexpectedEnd<PrattRhsHint, O> {
  /// Creates an unexpected **end of expression (right hand side)** error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eorhs(100);
  /// assert_eq!(error.offset(), 100);
  /// assert_eq!(error.name(), Some("expression (right hand side)"));
  /// ```
  #[inline(always)]
  pub const fn eorhs(offset: O) -> Self {
    Self::maybe_name(
      offset,
      Some(CowStr::from_static("expression (right hand side)")),
      PrattRhsHint,
    )
  }
}

impl<O, Lang: ?Sized> UnexpectedEnd<PrattRhsHint, O, Lang> {
  /// Creates an unexpected **end of expression (right hand side)** error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eorhs(100);
  /// assert_eq!(error.offset(), 100);
  /// assert_eq!(error.name(), Some("expression (right hand side)"));
  /// ```
  #[inline(always)]
  pub const fn eorhs_of(offset: O) -> Self {
    Self::maybe_name_of(
      offset,
      Some(CowStr::from_static("expression (right hand side)")),
      PrattRhsHint,
    )
  }
}

impl<O> UnexpectedEnd<PrattLhsHint, O> {
  /// Creates an unexpected **end of expression (left hand side)** error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eolhs(100);
  /// assert_eq!(error.offset(), 100);
  /// assert_eq!(error.name(), Some("expression (left hand side)"));
  /// ```
  #[inline(always)]
  pub const fn eolhs(offset: O) -> Self {
    Self::maybe_name(
      offset,
      Some(CowStr::from_static("expression (left hand side)")),
      PrattLhsHint,
    )
  }
}

impl<O, Lang: ?Sized> UnexpectedEnd<PrattLhsHint, O, Lang> {
  /// Creates an unexpected **end of expression (left hand side)** error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eolhs(100);
  /// assert_eq!(error.offset(), 100);
  /// assert_eq!(error.name(), Some("expression (left hand side)"));
  /// ```
  #[inline(always)]
  pub const fn eolhs_of(offset: O) -> Self {
    Self::maybe_name_of(
      offset,
      Some(CowStr::from_static("expression (left hand side)")),
      PrattLhsHint,
    )
  }
}

impl<O> UnexpectedEnd<FileHint, O> {
  /// Creates an unexpected **end of file** (EOF) error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eof(100);
  /// assert_eq!(error.offset(), 100);
  /// assert_eq!(error.name(), Some("file"));
  /// ```
  #[inline(always)]
  pub const fn eof(offset: O) -> Self {
    Self::maybe_name(offset, Some(CowStr::from_static("file")), FileHint)
  }
}

impl<O, Lang: ?Sized> UnexpectedEnd<FileHint, O, Lang> {
  /// Creates an unexpected **end of file** (EOF) error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eof(100);
  /// assert_eq!(error.offset(), 100);
  /// assert_eq!(error.name(), Some("file"));
  /// ```
  #[inline(always)]
  pub const fn eof_of(offset: O) -> Self {
    Self::maybe_name_of(offset, Some(CowStr::from_static("file")), FileHint)
  }
}

impl<O> UnexpectedEnd<TokenHint, O> {
  /// Creates an unexpected **end of token stream** (EOT) error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eot(50);
  /// assert_eq!(error.offset(), 50);
  /// assert_eq!(error.name(), Some("token stream"));
  /// ```
  #[inline(always)]
  pub const fn eot(offset: O) -> Self {
    Self::maybe_name(offset, Some(CowStr::from_static("token stream")), TokenHint)
  }
}

impl<O, Lang: ?Sized> UnexpectedEnd<TokenHint, O, Lang> {
  /// Creates an unexpected **end of token stream** (EOT) error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eot(50);
  /// assert_eq!(error.offset(), 50);
  /// assert_eq!(error.name(), Some("token stream"));
  /// ```
  #[inline(always)]
  pub const fn eot_of(offset: O) -> Self {
    Self::maybe_name_of(offset, Some(CowStr::from_static("token stream")), TokenHint)
  }
}

impl<O> UnexpectedEnd<CharacterHint, O> {
  /// Creates an unexpected **end of string** (EOS) error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eos(25);
  /// assert_eq!(error.offset(), 25);
  /// assert_eq!(error.name(), Some("string"));
  /// ```
  #[inline(always)]
  pub const fn eos(offset: O) -> Self {
    Self::maybe_name(offset, Some(CowStr::from_static("string")), CharacterHint)
  }
}

impl<O, Lang: ?Sized> UnexpectedEnd<CharacterHint, O, Lang> {
  /// Creates an unexpected **end of string** (EOS) error at the given offset.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eos(25);
  /// assert_eq!(error.offset(), 25);
  /// assert_eq!(error.name(), Some("string"));
  /// ```
  #[inline(always)]
  pub const fn eos_of(offset: O) -> Self {
    Self::maybe_name_of(offset, Some(CowStr::from_static("string")), CharacterHint)
  }
}

impl<Hint, O> UnexpectedEnd<Hint, O> {
  /// Creates a new unexpected end with the given offset and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{FileHint, UnexpectedEnd}};
  ///
  /// let error = UnexpectedEnd::new(10, FileHint);
  /// assert_eq!(error.name(), None);
  /// assert_eq!(error.offset(), 10);
  /// ```
  #[inline(always)]
  pub const fn new(offset: O, hint: Hint) -> Self {
    Self::maybe_name(offset, None, hint)
  }

  /// Creates a new unexpected end with the given offset, optional name, and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  ///
  /// # #[cfg(feature = "std")] {
  /// use tokora::{error::{FileHint, UnexpectedEnd}, utils::CowStr};
  ///
  /// let error = UnexpectedEnd::maybe_name(10, Some(CowStr::from_static("string")), FileHint);
  /// assert_eq!(error.name(), Some("string"));
  /// assert_eq!(error.offset(), 10);
  /// # }
  /// ```
  #[inline(always)]
  pub const fn maybe_name(offset: O, name: Option<CowStr>, hint: Hint) -> Self {
    Self::maybe_name_of(offset, name, hint)
  }

  /// Creates a new unexpected end with the given offset, name, and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// # #[cfg(feature = "std")] {
  /// use tokora::{error::{FileHint, UnexpectedEnd}, utils::CowStr};
  ///
  /// let error = UnexpectedEnd::with_name(20, CowStr::from_static("block"), FileHint);
  /// assert_eq!(error.name(), Some("block"));
  /// assert_eq!(error.offset(), 20);
  /// # }
  /// ```
  #[inline(always)]
  pub const fn with_name(offset: O, name: CowStr, hint: Hint) -> Self {
    Self::with_name_of(offset, name, hint)
  }

  /// Creates a new unexpected end with the given offset and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, TokenHint}};
  ///
  /// let error = UnexpectedEnd::with_hint(15, TokenHint);
  /// assert_eq!(error.name(), None);
  /// assert_eq!(error.offset(), 15);
  /// ```
  #[inline(always)]
  pub const fn with_hint(offset: O, hint: Hint) -> Self {
    Self::with_hint_of(offset, hint)
  }
}

impl<Hint, O, Lang: ?Sized> UnexpectedEnd<Hint, O, Lang> {
  /// Creates a new unexpected end with the given offset and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{FileHint, UnexpectedEnd}};
  ///
  /// let error = UnexpectedEnd::new(10, FileHint);
  /// assert_eq!(error.name(), None);
  /// assert_eq!(error.offset(), 10);
  /// ```
  #[inline(always)]
  pub const fn of(offset: O, hint: Hint) -> Self {
    Self::maybe_name_of(offset, None, hint)
  }

  /// Creates a new unexpected end with the given offset, optional name, and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  ///
  /// # #[cfg(feature = "std")] {
  /// use tokora::{error::{FileHint, UnexpectedEnd}, utils::CowStr};
  ///
  /// let error = UnexpectedEnd::maybe_name(10, Some(CowStr::from_static("string")), FileHint);
  /// assert_eq!(error.name(), Some("string"));
  /// assert_eq!(error.offset(), 10);
  /// # }
  /// ```
  #[inline(always)]
  pub const fn maybe_name_of(offset: O, name: Option<CowStr>, hint: Hint) -> Self {
    Self {
      offset,
      name,
      hint,
      expected: None,
      terminal: false,
      _lang: PhantomData,
    }
  }

  /// Creates a new unexpected end with the given offset, name, and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// # #[cfg(feature = "std")] {
  /// use tokora::{error::{FileHint, UnexpectedEnd}, utils::CowStr};
  ///
  /// let error = UnexpectedEnd::with_name(20, CowStr::from_static("block"), FileHint);
  /// assert_eq!(error.name(), Some("block"));
  /// assert_eq!(error.offset(), 20);
  /// # }
  /// ```
  #[inline(always)]
  pub const fn with_name_of(offset: O, name: CowStr, hint: Hint) -> Self {
    Self::maybe_name_of(offset, Some(name), hint)
  }

  /// Creates a new unexpected end with the given offset and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, TokenHint}};
  ///
  /// let error = UnexpectedEnd::with_hint(15, TokenHint);
  /// assert_eq!(error.name(), None);
  /// assert_eq!(error.offset(), 15);
  /// ```
  #[inline(always)]
  pub const fn with_hint_of(offset: O, hint: Hint) -> Self {
    Self {
      offset,
      name: None,
      hint,
      expected: None,
      terminal: false,
      _lang: PhantomData,
    }
  }

  /// Sets the name.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, FileHint}};
  ///
  /// let mut error = UnexpectedEnd::new(10, FileHint);
  /// error.set_name("expression");
  /// assert_eq!(error.name(), Some("expression"));
  /// ```
  #[inline(always)]
  pub fn set_name(&mut self, name: impl Into<CowStr>) -> &mut Self {
    self.name = Some(name.into());
    self
  }

  /// Updates the name.
  ///
  /// ## Example
  ///
  /// ```rust
  /// # #[cfg(feature = "std")] {
  /// use tokora::{error::{FileHint, UnexpectedEnd}, utils::CowStr};
  ///
  /// let mut error = UnexpectedEnd::with_name(10, CowStr::from_static("old"), FileHint);
  /// error.update_name(Some("new"));
  /// assert_eq!(error.name(), Some("new"));
  /// # }
  /// ```
  #[inline(always)]
  pub fn update_name(&mut self, name: Option<impl Into<CowStr>>) -> &mut Self {
    self.name = name.map(Into::into);
    self
  }

  /// Clear the name.
  ///
  /// ## Example
  ///
  /// ```rust
  /// # #[cfg(feature = "std")] {
  /// use tokora::{error::{FileHint, UnexpectedEnd}, utils::CowStr};
  ///
  /// let mut error = UnexpectedEnd::with_name(10, CowStr::from_static("block"), FileHint);
  /// error.clear_name();
  /// assert_eq!(error.name(), None);
  /// # }
  /// ```
  #[inline(always)]
  pub fn clear_name(&mut self) -> &mut Self {
    self.name = None;
    self
  }

  /// Returns the name, if any.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eof(100);
  /// assert_eq!(error.name(), Some("file"));
  /// ```
  #[inline(always)]
  pub const fn name(&self) -> Option<&str> {
    match &self.name {
      Some(name) => Some(name.as_str()),
      None => None,
    }
  }

  /// Returns the hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, FileHint}};
  ///
  /// let error = UnexpectedEnd::eof(100);
  /// // FileHint is a zero-sized type
  /// let _ = error.hint();
  /// ```
  #[inline(always)]
  pub const fn hint(&self) -> &Hint {
    &self.hint
  }

  /// Replace the hint, returning the old one.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, FileHint}};
  ///
  /// let mut error = UnexpectedEnd::eof(100);
  /// let old_hint = error.replace_hint(FileHint);
  /// // old_hint is FileHint
  /// ```
  #[inline(always)]
  pub fn replace_hint(&mut self, new: Hint) -> Hint {
    core::mem::replace(&mut self.hint, new)
  }

  /// Maps the hint to another type.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, FileHint, TokenHint}};
  ///
  /// let file_error = UnexpectedEnd::eof(100);
  /// let token_error = file_error.map_hint(|_| TokenHint);
  /// assert_eq!(token_error.name(), Some("file"));
  /// assert_eq!(token_error.offset(), 100);
  /// ```
  #[inline(always)]
  pub fn map_hint<F, NewHint>(self, f: F) -> UnexpectedEnd<NewHint, O, Lang>
  where
    F: FnOnce(Hint) -> NewHint,
  {
    UnexpectedEnd {
      offset: self.offset,
      name: self.name,
      hint: f(self.hint),
      expected: self.expected,
      terminal: self.terminal,
      _lang: PhantomData,
    }
  }

  /// Reconstructs the error with a new (optional) name and a transformed hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, FileHint, TokenHint}};
  ///
  /// let file_error = UnexpectedEnd::eof(100);
  /// let token_error = file_error.reconstruct(Some("block"), |_| TokenHint);
  /// assert_eq!(token_error.name(), Some("block"));
  /// assert_eq!(token_error.offset(), 100);
  /// ```
  #[inline(always)]
  pub fn reconstruct<F, NewHint>(
    self,
    name: Option<impl Into<CowStr>>,
    f: F,
  ) -> UnexpectedEnd<NewHint, O, Lang>
  where
    F: FnOnce(Hint) -> NewHint,
  {
    UnexpectedEnd::maybe_name_of(self.offset, name.map(Into::into), f(self.hint))
  }

  /// Reconstructs the error with a new name and a transformed hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{UnexpectedEnd, FileHint, TokenHint}};
  ///
  /// let file_error = UnexpectedEnd::eof(100);
  /// let token_error = file_error.reconstruct_with_name("expression", |_| TokenHint);
  /// assert_eq!(token_error.name(), Some("expression"));
  /// assert_eq!(token_error.offset(), 100);
  /// ```
  #[inline(always)]
  pub fn reconstruct_with_name<F, NewHint>(
    self,
    name: impl Into<CowStr>,
    f: F,
  ) -> UnexpectedEnd<NewHint, O, Lang>
  where
    F: FnOnce(Hint) -> NewHint,
  {
    UnexpectedEnd::with_name_of(self.offset, name.into(), f(self.hint))
  }

  /// Reconstructs the error with a transformed hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// # #[cfg(feature = "std")] {
  /// use tokora::{error::{UnexpectedEnd, FileHint, TokenHint}, utils::CowStr};
  ///
  /// let file_error = UnexpectedEnd::with_name(10, CowStr::from_static("file"), FileHint);
  /// let token_error = file_error.reconstruct_without_name(|_| TokenHint);
  /// assert_eq!(token_error.name(), None);
  /// assert_eq!(token_error.offset(), 10);
  /// # }
  /// ```
  #[inline(always)]
  pub fn reconstruct_without_name<F, NewHint>(self, f: F) -> UnexpectedEnd<NewHint, O, Lang>
  where
    F: FnOnce(Hint) -> NewHint,
  {
    UnexpectedEnd::of(self.offset, f(self.hint))
  }

  /// Returns the offset of the unexpected end.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eof(100);
  /// assert_eq!(error.offset(), 100);
  /// ```
  #[inline(always)]
  pub const fn offset(&self) -> O
  where
    O: Copy,
  {
    self.offset
  }

  /// Returns a reference to the offset of the unexpected end.
  #[inline(always)]
  pub const fn offset_ref(&self) -> &O {
    &self.offset
  }

  /// Returns a mutable reference to the offset of the unexpected end.
  #[inline(always)]
  pub const fn offset_mut(&mut self) -> &mut O {
    &mut self.offset
  }

  /// Bumps the offset by `n`.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining errors from different parsing contexts.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let mut error = UnexpectedEnd::eof(10);
  /// error.bump(&5);
  /// assert_eq!(error.offset(), 15);
  /// ```
  #[inline(always)]
  pub fn bump(&mut self, n: &O) -> &mut Self
  where
    O: for<'a> core::ops::AddAssign<&'a O>,
  {
    self.offset += n;
    self
  }

  /// Consumes the unexpected end and returns the offset, name, and hint.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::error::UnexpectedEnd;
  ///
  /// let error = UnexpectedEnd::eof(100);
  /// let (offset, name, hint) = error.into_components();
  /// assert_eq!(offset, 100);
  /// assert_eq!(name, Some("file".into()));
  /// ```
  #[inline(always)]
  pub fn into_components(self) -> (O, Option<CowStr>, Hint) {
    (self.offset, self.name, self.hint)
  }
}

impl<Hint, O, Lang: ?Sized, Set> UnexpectedEnd<Hint, O, Lang, Set>
where
  Set: Clone + 'static,
{
  /// Creates an unexpected end carrying an explicit **optional expected set** — the additive
  /// end-of-input variant. The other constructors leave the set `None`; this is the general form
  /// that populates it.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{TokenHint, UnexpectedEnd}, utils::{CowStr, Expected}};
  ///
  /// let error: UnexpectedEnd<TokenHint, usize, (), &'static str> = UnexpectedEnd::maybe_expected_of(
  ///     10,
  ///     Some(CowStr::from_static("token stream")),
  ///     TokenHint,
  ///     Some(Expected::one_of(&["if", "while"])),
  /// );
  /// assert!(matches!(error.expected(), Some(Expected::OneOf(_))));
  /// ```
  #[inline(always)]
  pub const fn maybe_expected_of(
    offset: O,
    name: Option<CowStr>,
    hint: Hint,
    expected: Option<Expected<'static, Set>>,
  ) -> Self {
    Self {
      offset,
      name,
      hint,
      expected,
      terminal: false,
      _lang: PhantomData,
    }
  }

  /// Returns the optional end-of-input expected set, if one was attached.
  ///
  /// `None` for every end-of-input error built through the legacy constructors
  /// (`eof`/`eot`/`eos`/…); `Some` only for the `*_expected_*` forms — e.g. a committed
  /// [`DispatchOnKind`](crate::parser::DispatchOnKind) failure at end of input reports its whole
  /// dispatch table here.
  #[inline(always)]
  pub const fn expected(&self) -> Option<&Expected<'static, Set>> {
    self.expected.as_ref()
  }

  /// Returns whether this end-of-input error stands in for a **terminal scanner stop** — a
  /// resource-limit trip, or the poison boundary it latches — rather than a genuine end of input.
  ///
  /// `false` for every ordinary constructor; `true` only after [`into_terminal`](Self::into_terminal).
  /// This is the signal [`MaybeTerminal`](crate::error::MaybeTerminal) exposes so recovery re-raises
  /// the stop instead of spending it as a recoverable failure.
  #[inline(always)]
  pub const fn is_terminal(&self) -> bool {
    self.terminal
  }

  /// Marks this end-of-input error as standing in for a **terminal scanner stop**, returning it by
  /// value.
  ///
  /// The attempt/decline leaves and the delimited close use this so a resource-limit trip surfaces
  /// as the committed form's end-of-input error yet stays distinguishable from a genuine end of
  /// input — see [`is_terminal`](Self::is_terminal) and
  /// [`MaybeTerminal`](crate::error::MaybeTerminal).
  #[inline(always)]
  #[must_use]
  pub fn into_terminal(mut self) -> Self {
    self.terminal = true;
    self
  }
}

impl<O, Lang: ?Sized, Set> UnexpectedEnd<TokenHint, O, Lang, Set>
where
  Set: Clone + 'static,
{
  /// Creates an unexpected **end of token stream** (EOT) carrying the full **expected set** — the
  /// "…, expected one of X, Y, Z" table a committed dispatch failure attaches at end of input.
  ///
  /// This is the end-of-input sibling of
  /// [`UnexpectedToken::expected_one_of`](crate::error::token::UnexpectedToken::expected_one_of):
  /// same "token stream" name and [`TokenHint`] as [`eot`](Self::eot), plus the expected set.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::{error::{TokenHint, UnexpectedEnd}, utils::Expected};
  ///
  /// let error: UnexpectedEnd<TokenHint, usize, ()> =
  ///     UnexpectedEnd::eot_expected_one_of(50, &["if", "while", "for"]);
  /// assert_eq!(error.offset(), 50);
  /// assert_eq!(error.name(), Some("token stream"));
  /// if let Some(Expected::OneOf(set)) = error.expected() {
  ///     assert_eq!(set.as_slice(), &["if", "while", "for"]);
  /// }
  /// ```
  #[inline(always)]
  pub const fn eot_expected_one_of(offset: O, expected: &'static [Set]) -> Self {
    Self::maybe_expected_of(
      offset,
      Some(CowStr::from_static("token stream")),
      TokenHint,
      Some(Expected::one_of(expected)),
    )
  }
}

impl<Hint, O, Lang: ?Sized, Set> From<UnexpectedEnd<Hint, O, Lang, Set>> for ()
where
  Set: Clone + 'static,
{
  #[inline(always)]
  fn from(_: UnexpectedEnd<Hint, O, Lang, Set>) -> Self {}
}

/// A type alias for unexpected EOF.
pub type UnexpectedEof<O = usize, Lang = (), Set = &'static str> =
  UnexpectedEnd<FileHint, O, Lang, Set>;
/// A type alias for unexpected end of token stream.
pub type UnexpectedEot<O = usize, Lang = (), Set = &'static str> =
  UnexpectedEnd<TokenHint, O, Lang, Set>;
/// A type alias for unexpected end of string.
pub type UnexpectedEos<O = usize, Lang = (), Set = &'static str> =
  UnexpectedEnd<CharacterHint, O, Lang, Set>;
/// A type alias for unexpected end of right hand side.
pub type UnexpectedEoRhs<O = usize, Lang = (), Set = &'static str> =
  UnexpectedEnd<PrattRhsHint, O, Lang, Set>;
/// A type alias for unexpected end of left hand side.
pub type UnexpectedEoLhs<O = usize, Lang = (), Set = &'static str> =
  UnexpectedEnd<PrattLhsHint, O, Lang, Set>;

impl<Hint, O, Lang: ?Sized> From<(O, Hint)> for UnexpectedEnd<Hint, O, Lang> {
  #[inline(always)]
  fn from((offset, hint): (O, Hint)) -> Self {
    Self::of(offset, hint)
  }
}

impl<Hint, O, Lang: ?Sized, Set> crate::error::MaybeTerminal for UnexpectedEnd<Hint, O, Lang, Set>
where
  Set: Clone + 'static,
{
  #[inline(always)]
  fn is_terminal(&self) -> bool {
    self.terminal
  }
}
