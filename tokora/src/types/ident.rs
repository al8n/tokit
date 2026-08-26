//! Identifier types for language syntax trees.
//!
//! This module provides generic identifier types that can be used across different
//! programming languages and string representations. Identifiers are fundamental
//! building blocks in most languages, representing names for variables, functions,
//! types, and other named entities.
//!
//! # Design Philosophy
//!
//! The [`Ident`] type is generic over both the source string type (`S`) and the
//! language marker (`Lang`). This design provides maximum flexibility:
//!
//! - **String type flexibility**: Use `&str` for zero-copy parsing, `String` for
//!   owned data, or custom interned string types for memory efficiency
//! - **Language safety**: The `Lang` parameter ensures identifiers from different
//!   languages don't mix accidentally
//! - **SimpleSpan tracking**: All identifiers carry their source location for diagnostics
//!
//! # Common Usage Patterns
//!
//! ## Zero-Copy Parsing
//!
//! ```rust,ignore
//! use tokora::{SimpleSpan, types::Ident};
//!
//! // Parse identifiers without allocating
//! type YulIdent<'a> = Ident<&'a str, SimpleSpan, YulLang>;
//!
//! let ident = YulIdent::new(SimpleSpan::new(0, 3), "foo");
//! assert_eq!(ident.source_ref(), &"foo");
//! ```
//!
//! ## Owned Identifiers
//!
//! ```rust,ignore
//! // Store identifiers in AST nodes that outlive the source
//! type OwnedIdent = Ident<String, SimpleSpan, MyLang>;
//!
//! let ident = OwnedIdent::new(span, source_str.to_string());
//! ```
//!
//! ## String Interning
//!
//! ```rust,ignore
//! // Use interned strings for memory efficiency
//! type InternedIdent = Ident<Symbol, SimpleSpan, MyLang>;
//!
//! let ident = InternedIdent::new(span, interner.intern("identifier"));
//! ```
//!
//! # Error Recovery
//!
//! `Ident` implements [`ErrorNode`] when the source type `S` also implements it,
//! allowing creation of placeholder identifiers during error recovery:
//!
//! ```rust,ignore
//! use tokora::error::ErrorNode;
//!
//! // Create placeholder for malformed identifier
//! let bad_ident = Ident::<String, SimpleSpan, YulLang>::error(span);
//!
//! // Create placeholder for missing identifier
//! let missing_ident = Ident::<String, SimpleSpan, YulLang>::missing(span);
//! ```

use core::marker::PhantomData;

use super::status::Status;
use crate::{
  error::ErrorNode,
  span::{AsSpan, SimpleSpan},
  utils::IntoComponents,
};

/// A language identifier with span tracking.
///
/// Identifiers are names used in source code to refer to variables, functions,
/// types, and other named entities. This type wraps a source string representation
/// with position information and a language marker.
///
/// # Type Parameters
///
/// - `S`: The source string type (`&str`, `String`, interned string, etc.)
/// - `Lang`: Language marker type for type safety (e.g., `YulLang`, `SolidityLang`)
///
/// # Design Notes
///
/// ## Why Generic Over String Type?
///
/// Different use cases require different string representations:
/// - **Parsing**: Use `&str` for zero-copy efficiency
/// - **AST storage**: Use `String` when the AST outlives the source
/// - **Large codebases**: Use interned strings to deduplicate common identifiers
///
/// ## Why Language Marker?
///
/// The `Lang` parameter prevents mixing identifiers from different languages:
/// ```rust,ignore
/// let yul_ident: Ident<&str, SimpleSpan, YulLang> = ...;
/// let sol_ident: Ident<&str, SimpleSpan, SolidityLang> = ...;
///
/// // Compile error: type mismatch
/// // let mixed = vec![yul_ident, sol_ident];
/// ```
///
/// # Examples
///
/// ## Creating Identifiers
///
/// ```rust
/// use tokora::{SimpleSpan, types::Ident};
/// # struct MyLang;
///
/// // Zero-copy identifier
/// let span = SimpleSpan::new(5, 11);
/// let ident = Ident::<&str, SimpleSpan, MyLang>::new(span, "my_var");
///
/// assert_eq!(ident.span(), span);
/// assert_eq!(ident.source_ref(), &"my_var");
/// ```
///
/// ## Extracting Components
///
/// ```rust
/// # use tokora::{SimpleSpan, types::Ident, utils::IntoComponents};
/// # struct MyLang;
/// # let span = SimpleSpan::new(0, 3);
/// let ident = Ident::<&str, SimpleSpan, MyLang>::new(span, "foo");
///
/// // Destructure into span and source
/// let (span, source) = ident.into_components();
/// assert_eq!(source, "foo");
/// ```
///
/// ## Mutable Access
///
/// ```rust
/// # use tokora::{SimpleSpan, types::Ident};
/// # struct MyLang;
/// # let span = SimpleSpan::new(0, 3);
/// let mut ident = Ident::<String, SimpleSpan, MyLang>::new(span, "original".to_string());
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
pub struct Ident<S: ?Sized, Span = SimpleSpan, Lang: ?Sized = ()> {
  _lang: PhantomData<Lang>,
  status: Status,
  span: Span,
  ident: S,
}

impl<S: ?Sized, Span, Lang: ?Sized> AsSpan<Span> for Ident<S, Span, Lang> {
  #[inline(always)]
  fn as_span(&self) -> &Span {
    self.span_ref()
  }
}

impl<S, Span, Lang: ?Sized> IntoComponents for Ident<S, Span, Lang> {
  type Components = (Span, S);

  #[inline(always)]
  fn into_components(self) -> Self::Components {
    (self.span, self.ident)
  }
}

impl<S: ?Sized, Span, Lang: ?Sized> Ident<S, Span, Lang> {
  /// Returns the span (source location) of this identifier.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Ident;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Ident::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(5, 10), "value");
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
  /// # use tokora::types::Ident;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Ident::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "foo");
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
  /// # use tokora::types::Ident;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let mut ident = Ident::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "foo");
  ///
  /// *ident.span_mut() = SimpleSpan::new(10, 13);
  /// assert_eq!(ident.span(), SimpleSpan::new(10, 13));
  /// ```
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut Span {
    &mut self.span
  }

  /// Bumps the span of the identifier by the given offset.
  #[inline(always)]
  pub fn bump(&mut self, by: &Span::Offset) -> &mut Self
  where
    Span: crate::span::Span,
  {
    self.span.bump(by);
    self
  }

  /// Returns a mutable reference to the source string.
  ///
  /// Use this to modify the identifier's text, for example during AST
  /// transformations or name mangling.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # use tokora::types::Ident;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let mut ident = Ident::<String, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "foo".to_string());
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
  /// # use tokora::types::Ident;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Ident::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 8), "variable");
  ///
  /// assert_eq!(ident.source_ref(), &"variable");
  /// assert_eq!(ident.source_ref().len(), 8);
  /// ```
  #[inline(always)]
  pub const fn source_ref(&self) -> &S {
    &self.ident
  }

  /// Returns `true` is this identifier represents an error identifier.
  #[inline(always)]
  pub const fn is_error(&self) -> bool {
    self.status.is_error()
  }

  /// Returns `true` is this identifier represents a missing identifier.
  #[inline(always)]
  pub const fn is_missing(&self) -> bool {
    self.status.is_missing()
  }

  /// Returns `true` is this identifier is valid (not error or missing).
  #[inline(always)]
  pub const fn is_valid(&self) -> bool {
    self.status.is_valid()
  }
}

impl<S, Span, Lang: ?Sized> Ident<S, Span, Lang> {
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
  /// use tokora::types::Ident;
  /// use tokora::SimpleSpan;
  /// # struct YulLang;
  ///
  /// let span = SimpleSpan::new(10, 15);
  /// let ident = Ident::<&str, SimpleSpan, YulLang>::new(span, "count");
  ///
  /// assert_eq!(ident.span(), span);
  /// assert_eq!(ident.source_ref(), &"count");
  /// ```
  #[inline(always)]
  pub const fn new(span: Span, source: S) -> Self {
    Self::with_status(span, source, Status::Valid)
  }

  #[inline(always)]
  pub(super) const fn with_status(span: Span, source: S, status: Status) -> Self {
    Self {
      span,
      ident: source,
      status,
      _lang: PhantomData,
    }
  }

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
  /// # use tokora::types::Ident;
  /// # use tokora::SimpleSpan;
  /// # struct MyLang;
  /// let ident = Ident::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 2), "id");
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

  /// Maps the source string to a new type, preserving the span, the language, and the
  /// recovery status.
  ///
  /// Recovery status is orthogonal to the source representation: mapping
  /// `Ident<&str>` to `Ident<String>` changes how the spelling is stored, not whether the
  /// parser actually found an identifier there. An [`error`](ErrorNode::error) or
  /// [`missing`](ErrorNode::missing) placeholder therefore stays one across the map.
  ///
  /// The body destructures `Self` exhaustively instead of rebuilding through
  /// [`Self::new`], and that shape is the point. `new` always produces a valid identifier,
  /// so a `new`-based body silently laundered recovery placeholders into valid syntax with
  /// nothing to flag the omission; the destructuring form fails to compile the next time a
  /// field is added rather than dropping it.
  #[inline(always)]
  pub fn map<U>(self, f: impl FnOnce(S) -> U) -> Ident<U, Span, Lang> {
    let Self {
      _lang,
      status,
      span,
      ident,
    } = self;

    Ident {
      _lang,
      status,
      span,
      ident: f(ident),
    }
  }
}

impl<S, Span, Lang: ?Sized> ErrorNode<Span> for Ident<S, Span, Lang>
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
  /// use tokora::types::Ident;
  /// use tokora::error::ErrorNode;
  ///
  /// // Parser found "123abc" where an identifier was expected
  /// let bad_ident = Ident::<String, SimpleSpan, YulLang>::error(span);
  /// ```
  #[inline]
  fn error(span: Span) -> Self {
    Self::with_status(span.clone(), S::error(span), Status::Error)
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
  /// use tokora::types::Ident;
  /// use tokora::error::ErrorNode;
  ///
  /// // Parser expected identifier after "let" but found "="
  /// // Correct: let name = 5;
  /// // Found:   let = 5;
  /// let missing_ident = Ident::<String, SimpleSpan, YulLang>::missing(span);
  /// ```
  #[inline]
  fn missing(span: Span) -> Self {
    Self::with_status(span.clone(), S::missing(span), Status::Missing)
  }
}
