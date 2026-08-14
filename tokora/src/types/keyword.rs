//! Keyword types for language syntax trees.
//!
//! This module provides generic identifier types that can be used across different
//! programming languages and string representations. Keywords are fundamental
//! building blocks in most languages, representing names for variables, functions,
//! types, and other named entities.
//!
//! # Design Philosophy
//!
//! The [`Keyword`] type is generic over the source string type (`S`), the span type
//! (`Span`), and the language marker (`Lang`). This design provides maximum flexibility:
//!
//! - **String type flexibility**: Use `&str` for zero-copy parsing, `String` for
//!   owned data, or custom interned string types for memory efficiency
//! - **Language safety**: The `Lang` parameter ensures keywords from different
//!   languages don't mix accidentally
//! - **SimpleSpan tracking**: All keywords carry their source location for diagnostics
//!
//! # Common Usage Patterns
//!
//! ## Zero-Copy Parsing
//!
//! ```rust,ignore
//! use tokora::types::Keyword;
//! use tokora::SimpleSpan;
//!
//! // Parse keywords without allocating
//! type YulKeyword<'a> = Keyword<&'a str, SimpleSpan, YulLang>;
//!
//! let ident = YulKeyword::new(SimpleSpan::new(0, 3), "foo");
//! assert_eq!(ident.source_ref(), &"foo");
//! ```
//!
//! ## Owned Keywords
//!
//! ```rust,ignore
//! // Store keywords in AST nodes that outlive the source
//! type OwnedKeyword = Keyword<String, SimpleSpan, MyLang>;
//!
//! let ident = OwnedKeyword::new(span, source_str.to_string());
//! ```
//!
//! ## String Interning
//!
//! ```rust,ignore
//! // Use interned strings for memory efficiency
//! type InternedKeyword = Keyword<Symbol, SimpleSpan, MyLang>;
//!
//! let ident = InternedKeyword::new(span, interner.intern("identifier"));
//! ```
//!
//! # Error Recovery
//!
//! `Keyword` implements [`ErrorNode`] when the source type `S` also implements it,
//! allowing creation of placeholder keywords during error recovery:
//!
//! ```rust,ignore
//! use tokora::error::ErrorNode;
//!
//! // Create placeholder for malformed identifier
//! let bad_ident = Keyword::<String, SimpleSpan, YulLang>::error(span);
//!
//! // Create placeholder for missing identifier
//! let missing_ident = Keyword::<String, SimpleSpan, YulLang>::missing(span);
//! ```

use core::marker::PhantomData;

use crate::{
  error::ErrorNode,
  span::{AsSpan, SimpleSpan},
  utils::IntoComponents,
};

/// A language identifier with span tracking.
///
/// Keywords are names used in source code to refer to variables, functions,
/// types, and other named entities. This type wraps a source string representation
/// with position information and a language marker.
///
/// # Type Parameters
///
/// - `S`: The source string type (`&str`, `String`, interned string, etc.)
/// - `Span`: The span type tracking the keyword's source location (defaults to [`SimpleSpan`])
/// - `Lang`: Language marker type for type safety (e.g., `YulLang`, `SolidityLang`)
///
/// # Design Notes
///
/// ## Why Generic Over String Type?
///
/// Different use cases require different string representations:
/// - **Parsing**: Use `&str` for zero-copy efficiency
/// - **AST storage**: Use `String` when the AST outlives the source
/// - **Large codebases**: Use interned strings to deduplicate common keywords
///
/// ## Why Language Marker?
///
/// The `Lang` parameter prevents mixing keywords from different languages:
/// ```rust,ignore
/// let yul_ident: Keyword<&str, SimpleSpan, YulLang> = ...;
/// let sol_ident: Keyword<&str, SimpleSpan, SolidityLang> = ...;
///
/// // Compile error: type mismatch
/// // let mixed = vec![yul_ident, sol_ident];
/// ```
///
/// # Examples
///
/// ## Creating Keywords
///
/// ```rust
/// use tokora::types::Keyword;
/// use tokora::SimpleSpan;
/// # struct MyLang;
///
/// // Zero-copy identifier
/// let span = SimpleSpan::new(5, 11);
/// let ident = Keyword::<&str, SimpleSpan, MyLang>::new(span, "my_var");
///
/// assert_eq!(ident.span(), span);
/// assert_eq!(ident.source_ref(), &"my_var");
/// ```
///
/// ## Extracting Components
///
/// ```rust
/// # use tokora::types::Keyword;
/// # use tokora::SimpleSpan;
/// # use tokora::utils::IntoComponents;
/// # struct MyLang;
/// # let span = SimpleSpan::new(0, 3);
/// let ident = Keyword::<&str, SimpleSpan, MyLang>::new(span, "foo");
///
/// // Destructure into span and source
/// let (span, source) = ident.into_components();
/// assert_eq!(source, "foo");
/// ```
///
/// ## Mutable Access
///
/// ```rust
/// # use tokora::types::Keyword;
/// # use tokora::SimpleSpan;
/// # struct MyLang;
/// # let span = SimpleSpan::new(0, 3);
/// let mut ident = Keyword::<String, SimpleSpan, MyLang>::new(span, "original".to_string());
///
/// // Update the source string
/// *ident.source_mut() = "modified".to_string();
/// assert_eq!(ident.source_ref(), "modified");
///
/// // Update the span
/// *ident.span_mut() = SimpleSpan::new(10, 18);
/// assert_eq!(ident.span(), SimpleSpan::new(10, 18));
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Keyword<S: ?Sized, Span = SimpleSpan, Lang: ?Sized = ()> {
  _lang: PhantomData<Lang>,
  span: Span,
  ident: S,
}

impl<S, Span, Lang: ?Sized> From<Keyword<S, Span, Lang>> for super::Ident<S, Span, Lang> {
  #[inline(always)]
  fn from(keyword: Keyword<S, Span, Lang>) -> Self {
    Self::new(keyword.span, keyword.ident)
  }
}

impl<S: ?Sized, Span, Lang: ?Sized> AsSpan<Span> for Keyword<S, Span, Lang> {
  #[inline(always)]
  fn as_span(&self) -> &Span {
    self.span_ref()
  }
}

impl<S, Span, Lang: ?Sized> IntoComponents for Keyword<S, Span, Lang> {
  type Components = (Span, S);

  #[inline(always)]
  fn into_components(self) -> Self::Components {
    (self.span, self.ident)
  }
}

impl<S, Span, Lang: ?Sized> Keyword<S, Span, Lang> {
  /// Creates a new identifier with the given span and source string.
  ///
  /// # Parameters
  ///
  /// - `span`: The source location of this identifier
  /// - `source`: The identifier string (can be `&str`, `String`, or custom type)
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::types::Keyword;
  /// use tokora::SimpleSpan;
  /// # struct YulLang;
  ///
  /// let span = SimpleSpan::new(10, 15);
  /// let ident = Keyword::<&str, SimpleSpan, YulLang>::new(span, "count");
  ///
  /// assert_eq!(ident.span(), span);
  /// assert_eq!(ident.source_ref(), &"count");
  /// ```
  #[inline(always)]
  pub const fn new(span: Span, source: S) -> Self {
    Self {
      span,
      ident: source,
      _lang: PhantomData,
    }
  }
}

impl<S: ?Sized, Span, Lang: ?Sized> Keyword<S, Span, Lang> {
  /// Returns the span (source location) of this identifier.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Keyword;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Keyword::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(5, 10), "value");
  ///
  /// assert_eq!(ident.span(), SimpleSpan::new(5, 10));
  /// ```
  #[inline(always)]
  pub const fn span(&self) -> Span
  where
    Span: Copy,
  {
    self.span
  }

  /// Returns an immutable reference to the span.
  ///
  /// Use this when you need to borrow the span without copying it.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Keyword;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Keyword::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "foo");
  ///
  /// let span_ref = ident.span_ref();
  /// assert_eq!(*span_ref, SimpleSpan::new(0, 3));
  /// ```
  #[inline(always)]
  pub const fn span_ref(&self) -> &Span {
    &self.span
  }

  /// Returns a mutable reference to the span.
  ///
  /// Use this to update the identifier's source location, for example during
  /// AST transformations or span adjustments.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Keyword;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let mut ident = Keyword::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "foo");
  ///
  /// *ident.span_mut() = SimpleSpan::new(10, 13);
  /// assert_eq!(ident.span(), SimpleSpan::new(10, 13));
  /// ```
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut Span {
    &mut self.span
  }

  /// Returns a mutable reference to the source string.
  ///
  /// Use this to modify the identifier's text, for example during AST
  /// transformations or name mangling.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Keyword;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let mut ident = Keyword::<String, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "foo".to_string());
  ///
  /// *ident.source_mut() = "bar".to_string();
  /// assert_eq!(ident.source_ref(), "bar");
  /// ```
  #[inline(always)]
  pub const fn source_mut(&mut self) -> &mut S {
    &mut self.ident
  }

  /// Returns an immutable reference to the source string.
  ///
  /// This is the most common way to access the identifier's text without
  /// consuming or copying it.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Keyword;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Keyword::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 8), "variable");
  ///
  /// assert_eq!(ident.source_ref(), &"variable");
  /// assert_eq!(ident.source_ref().len(), 8);
  /// ```
  #[inline(always)]
  pub const fn source_ref(&self) -> &S {
    &self.ident
  }
}

impl<S, Span, Lang: ?Sized> Keyword<S, Span, Lang> {
  /// Returns a copy of the source string by value.
  ///
  /// This method is only available when the source type `S` implements [`Copy`].
  /// Useful for types like `&str` or interned string handles.
  ///
  /// For owned types like `String`, use [`Self::source_ref`] to
  /// avoid cloning, or consume the identifier with
  /// [`crate::utils::IntoComponents::into_components`].
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Keyword;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Keyword::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 2), "id");
  ///
  /// let source: &str = ident.source(); // Copy
  /// assert_eq!(source, "id");
  /// // ident is still usable
  /// assert_eq!(ident.source_ref(), &"id");
  /// ```
  #[inline(always)]
  pub const fn source(&self) -> S
  where
    S: Copy,
  {
    self.ident
  }

  /// Consumes the identifier and returns the span and source string.
  #[inline(always)]
  pub fn into_components(self) -> (Span, S) {
    (self.span, self.ident)
  }

  /// Maps the source string to a new type, preserving the span and language.
  #[inline(always)]
  pub fn map<U>(self, f: impl FnOnce(S) -> U) -> Keyword<U, Span, Lang> {
    Keyword::new(self.span, f(self.ident))
  }
}

impl<S, Span, Lang: ?Sized> ErrorNode<Span> for Keyword<S, Span, Lang>
where
  S: ErrorNode<Span>,
  Span: Clone,
{
  /// Creates a placeholder identifier for **malformed content**.
  ///
  /// Used during error recovery when the parser encounters invalid identifier
  /// syntax. The source string `S` will also be created as an error placeholder.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::types::Keyword;
  /// use tokora::error::ErrorNode;
  ///
  /// // Parser found "123abc" where an identifier was expected
  /// let bad_ident = Keyword::<String, SimpleSpan, YulLang>::error(span);
  /// ```
  #[inline]
  fn error(span: Span) -> Self {
    Self::new(span.clone(), S::error(span))
  }

  /// Creates a placeholder identifier for **missing required content**.
  ///
  /// Used during error recovery when the parser expects an identifier but
  /// finds nothing at all. The source string `S` will also be created as
  /// a missing placeholder.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::types::Keyword;
  /// use tokora::error::ErrorNode;
  ///
  /// // Parser expected identifier after "let" but found "="
  /// // Correct: let name = 5;
  /// // Found:   let = 5;
  /// let missing_ident = Keyword::<String, SimpleSpan, YulLang>::missing(span);
  /// ```
  #[inline]
  fn missing(span: Span) -> Self {
    Self::new(span.clone(), S::missing(span))
  }
}
