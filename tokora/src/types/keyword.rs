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
//! use tokora::{error::ErrorNode, types::recovery::RecoveryState};
//!
//! // Create placeholder for malformed identifier
//! let bad_ident = Keyword::<String, SimpleSpan, YulLang>::error(span);
//! assert!(bad_ident.is_error());
//!
//! // Create placeholder for missing identifier
//! let missing_ident = Keyword::<String, SimpleSpan, YulLang>::missing(span);
//! assert!(missing_ident.is_missing());
//! ```
//!
//! Which of the three states a keyword is in is read through
//! [`RecoveryState`](super::recovery::RecoveryState), which must be in scope, and never from the payload.
//! There is no inherent accessor: see the trait for why an inherent one cannot fail loudly. The payload of a placeholder
//! is whatever `S::error` / `S::missing` produced — for `&str` the literal `"<error>"`, a
//! value a caller can also spell by hand — and it is mutable through
//! [`source_mut`](Keyword::source_mut), so it reports what the node says rather than whether
//! the parser found one.

use core::marker::PhantomData;

use super::recovery::Status;
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
/// // Destructure into span, source and recovery status
/// let (span, source, status) = ident.into_components();
/// assert_eq!(source, "foo");
/// assert!(status.is_valid());
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
// `PartialEq` and `Eq` are **derived** while the other four are written out, and that split is
// the point rather than an inconsistency.
//
// A derive constrains every type parameter, which is why `Debug`, `Clone`, `Copy` and `Hash` are
// hand-written here: none of them reads `Lang`, so none of them should demand anything of it.
// `PartialEq`/`Eq` cannot join them. The derive is also what emits `StructuralPartialEq`, and
// without that marker a `const` of this type cannot appear in a `match` pattern — "constant of
// non-structural type", a property no amount of checking the rendered output can see, because it
// is not observable except by trying to use a value in a pattern.
//
// So the residual is stated rather than hidden: comparing or pattern-matching one of these still
// needs the language marker to be `PartialEq + Eq`. Printing, cloning, copying and hashing do
// not. That is four of the six bounds dropped against 0.9, and structural matching kept exactly
// as 0.9 had it.
#[derive(PartialEq, Eq)]
pub struct Keyword<S: ?Sized, Span = SimpleSpan, Lang: ?Sized = ()> {
  _lang: PhantomData<Lang>,
  status: Status,
  span: Span,
  ident: S,
}

// The six impls below replace a `derive`, and the only thing that changes is the bound list.
//
// A derive constrains **every** type parameter, so `#[derive(Debug)]` on a type holding a
// `PhantomData<Lang>` emitted `Lang: Debug` — a requirement on a marker that is never printed,
// never compared and never hashed, because `PhantomData<T>` implements all six for any `T`
// unconditionally. The effect was that `Keyword<..., MyLang>` was not `Debug`, `Copy` or
// comparable unless the consumer derived those on `MyLang` too, for a reason that does not
// exist. tokora#320 caught it the way such things are caught: a doctest that would not compile
// until four derives were added to the marker.
//
// Rendered output and comparison order are unchanged. `Debug` still prints every field in
// declaration order including the marker, `PartialEq` still short-circuits in that order, and
// `Hash` still feeds the same bytes — `PhantomData` hashes nothing, so omitting it is not a
// change.

impl<S, Span, Lang> ::core::fmt::Debug for Keyword<S, Span, Lang>
where
  S: ::core::fmt::Debug + ?Sized,
  Span: ::core::fmt::Debug,
  Lang: ?Sized,
{
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    f.debug_struct("Keyword")
      .field("_lang", &&self._lang)
      .field("status", &&self.status)
      .field("span", &&self.span)
      .field("ident", &&self.ident)
      .finish()
  }
}

impl<S, Span, Lang> ::core::clone::Clone for Keyword<S, Span, Lang>
where
  S: ::core::clone::Clone,
  Span: ::core::clone::Clone,
  Lang: ?Sized,
{
  #[inline]
  fn clone(&self) -> Self {
    Self {
      _lang: ::core::marker::PhantomData,
      status: self.status,
      span: ::core::clone::Clone::clone(&self.span),
      ident: ::core::clone::Clone::clone(&self.ident),
    }
  }
}

impl<S, Span, Lang> ::core::marker::Copy for Keyword<S, Span, Lang>
where
  S: ::core::marker::Copy,
  Span: ::core::marker::Copy,
  Lang: ?Sized,
{
}

impl<S, Span, Lang> ::core::hash::Hash for Keyword<S, Span, Lang>
where
  S: ::core::hash::Hash + ?Sized,
  Span: ::core::hash::Hash,
  Lang: ?Sized,
{
  #[inline]
  fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
    ::core::hash::Hash::hash(&self.status, state);
    ::core::hash::Hash::hash(&self.span, state);
    ::core::hash::Hash::hash(&self.ident, state);
  }
}

impl<S, Span, Lang: ?Sized> From<Keyword<S, Span, Lang>> for super::Ident<S, Span, Lang> {
  /// The recovery status crosses unchanged: a keyword built by
  /// [`error`](ErrorNode::error) or [`missing`](ErrorNode::missing) becomes an identifier that
  /// reports itself the same way, and only a keyword actually spelled out in the source
  /// produces a valid one. Both carriers hold the same status type, so there is nothing to
  /// fabricate here — which is what tokora#301 fixed, and why this impl no longer rebuilds
  /// through [`Ident::new`](super::Ident::new).
  ///
  /// The source is destructured exhaustively so that a `Keyword` field added later stops this
  /// impl from compiling — a cross-type rebuild cannot be made total on the target side, but it
  /// can be made total on the side whose fields it reads.
  #[inline(always)]
  fn from(keyword: Keyword<S, Span, Lang>) -> Self {
    let Keyword {
      _lang,
      status,
      span,
      ident,
    } = keyword;

    Self::with_status(span, ident, status)
  }
}

impl<S: ?Sized, Span, Lang: ?Sized> AsSpan<Span> for Keyword<S, Span, Lang> {
  #[inline(always)]
  fn as_span(&self) -> &Span {
    self.span_ref()
  }
}

impl<S, Span, Lang: ?Sized> IntoComponents for Keyword<S, Span, Lang> {
  /// The span, the source, **and the recovery status** — every field this type holds beyond the
  /// zero-sized language marker, which the rebuild names in its own type.
  ///
  /// The status is here because this trait promises a complete decomposition and because
  /// [`with_status`](Keyword::with_status) is the inverse: without it in both, a consumer who
  /// took a carrier apart and put it back together would have to rebuild through
  /// [`new`](Keyword::new), which always declares the result valid — the same laundering
  /// tokora#303 removed from [`map`](Keyword::map), reached one door over.
  type Components = (Span, S, Status);

  #[inline(always)]
  fn into_components(self) -> Self::Components {
    Keyword::into_components(self)
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
    Self::with_status(span, source, Status::Valid)
  }

  /// Creates a keyword with an explicitly given recovery status.
  ///
  /// The inverse of [`into_components`](Self::into_components), and the only constructor that
  /// can express a state other than valid over a payload the caller chose: [`new`](Self::new)
  /// always declares the result valid, and [`ErrorNode::error`](ErrorNode::error) /
  /// [`ErrorNode::missing`](ErrorNode::missing) pick the payload themselves. A dialect with its
  /// own placeholder spelling needs this one.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::{SimpleSpan, types::{Keyword, Status}};
  /// use tokora::types::recovery::RecoveryState;
  /// // Comparing carriers needs the marker to be `PartialEq`: `PartialEq`/`Eq` stay derived
  /// // so a `const` carrier keeps working in a `match` pattern, and a derive constrains
  /// // every parameter. `Debug`, `Clone`, `Copy` and `Hash` need nothing of it.
  /// #[derive(PartialEq)]
  /// struct MyLang;
  ///
  /// let span = SimpleSpan::new(0, 4);
  /// let recovered = Keyword::<&str, SimpleSpan, MyLang>::with_status(span, "then", Status::Missing);
  ///
  /// assert!(recovered.is_missing());
  /// assert_eq!(recovered.source_ref(), &"then");
  ///
  /// // Round trip: the three components rebuild the value they came from.
  /// let (span, source, status) = recovered.into_components();
  /// let rebuilt = Keyword::<&str, SimpleSpan, MyLang>::with_status(span, source, status);
  /// assert_eq!(rebuilt, recovered);
  /// ```
  #[inline(always)]
  pub const fn with_status(span: Span, source: S, status: Status) -> Self {
    Self {
      span,
      ident: source,
      status,
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

  /// Consumes the keyword and returns the span, the source string and the recovery status.
  ///
  /// This is an inherent method with the same name as
  /// [`IntoComponents::into_components`](IntoComponents::into_components), which `Keyword` also
  /// implements, and an inherent item wins the pick at an unqualified call site. That makes the
  /// two returning *different* shapes a defect no diagnostic would report, so the trait impl
  /// delegates here rather than repeating the body. `Ident` and the `Lit*` family have no such
  /// second door.
  ///
  /// The status is part of the result because [`with_status`](Self::with_status) is the inverse
  /// and a decomposition that dropped it could only be rebuilt through [`new`](Self::new), which
  /// always declares the result valid.
  #[inline(always)]
  pub fn into_components(self) -> (Span, S, Status) {
    let Self {
      _lang,
      status,
      span,
      ident,
    } = self;

    (span, ident, status)
  }

  /// Maps the source string to a new type, preserving the span, the language, and the
  /// recovery status.
  ///
  /// Recovery status is orthogonal to the source representation: mapping `Keyword<&str>` to
  /// `Keyword<String>` changes how the spelling is stored, not whether the parser actually
  /// found a keyword there. An [`error`](ErrorNode::error) or [`missing`](ErrorNode::missing)
  /// placeholder therefore stays one across the map.
  ///
  /// Destructures `Self` exhaustively rather than rebuilding through [`Self::new`], because
  /// `new` always produces a valid keyword: a `new`-based body would drop the status without
  /// a diagnostic, which is exactly how [`Ident::map`](super::Ident::map) came to launder
  /// recovery placeholders into valid syntax. This form fails to compile instead.
  #[inline(always)]
  pub fn map<U>(self, f: impl FnOnce(S) -> U) -> Keyword<U, Span, Lang> {
    let Self {
      _lang,
      status,
      span,
      ident,
    } = self;

    Keyword {
      _lang,
      status,
      span,
      ident: f(ident),
    }
  }
}

impl<S: ?Sized, Span, Lang: ?Sized> super::recovery::RecoveryState for Keyword<S, Span, Lang> {
  #[inline(always)]
  fn status(&self) -> Status {
    self.status
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
  /// assert!(bad_ident.is_error());
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
  /// use tokora::types::Keyword;
  /// use tokora::error::ErrorNode;
  ///
  /// // Parser expected identifier after "let" but found "="
  /// // Correct: let name = 5;
  /// // Found:   let = 5;
  /// let missing_ident = Keyword::<String, SimpleSpan, YulLang>::missing(span);
  /// assert!(missing_ident.is_missing());
  /// ```
  #[inline]
  fn missing(span: Span) -> Self {
    Self::with_status(span.clone(), S::missing(span), Status::Missing)
  }
}
