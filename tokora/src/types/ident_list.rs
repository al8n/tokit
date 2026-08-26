use core::marker::PhantomData;

use crate::{
  span::AsSpan,
  types::{Ident, RecoveryState},
};

/// A list of identifiers.
///
/// `S` remains sized because the default container stores `Ident<S, Span>` values
/// inline in a `Vec`; use the individual [`Ident`] carrier for an unsized source.
#[cfg(any(feature = "alloc", feature = "std"))]
pub struct IdentList<
  S,
  Span = crate::span::SimpleSpan,
  Container = std::vec::Vec<Ident<S, Span>>,
  Lang: ?Sized = (),
> {
  span: Span,
  identifiers: Container,
  _m: PhantomData<S>,
  _lang: PhantomData<Lang>,
}

/// A list of identifiers.
///
/// `S` remains sized to keep this type's API consistent with its `Vec`-backed
/// form; use the individual [`Ident`] carrier for an unsized source.
#[cfg(not(any(feature = "alloc", feature = "std")))]
pub struct IdentList<S, Span, Container, Lang: ?Sized = ()> {
  span: Span,
  identifiers: Container,
  _m: PhantomData<S>,
  _lang: PhantomData<Lang>,
}

// The six impls below replace a `derive`, and the only thing that changes is the bound list.
//
// A derive constrains **every** type parameter, so `#[derive(Debug)]` on a type holding a
// `PhantomData<Lang>` emitted `Lang: Debug` — a requirement on a marker that is never printed,
// never compared and never hashed, because `PhantomData<T>` implements all six for any `T`
// unconditionally. The effect was that `IdentList<..., MyLang>` was not `Debug`, `Copy` or
// comparable unless the consumer derived those on `MyLang` too, for a reason that does not
// exist. tokora#320 caught it the way such things are caught: a doctest that would not compile
// until four derives were added to the marker.
//
// Rendered output and comparison order are unchanged. `Debug` still prints every field in
// declaration order including the marker, `PartialEq` still short-circuits in that order, and
// `Hash` still feeds the same bytes — `PhantomData` hashes nothing, so omitting it is not a
// change.

impl<S, Span, Container, Lang> ::core::fmt::Debug for IdentList<S, Span, Container, Lang>
where
  Span: ::core::fmt::Debug,
  Container: ::core::fmt::Debug,
  Lang: ?Sized,
{
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    // Declaration order, which is the order the derive printed in: `span`, `identifiers`, then
    // the two markers. Nothing about the rendering changes here, only the bounds above.
    f.debug_struct("IdentList")
      .field("span", &&self.span)
      .field("identifiers", &&self.identifiers)
      .field("_m", &&self._m)
      .field("_lang", &&self._lang)
      .finish()
  }
}

impl<S, Span, Container, Lang> ::core::clone::Clone for IdentList<S, Span, Container, Lang>
where
  Span: ::core::clone::Clone,
  Container: ::core::clone::Clone,
  Lang: ?Sized,
{
  #[inline]
  fn clone(&self) -> Self {
    Self {
      _m: ::core::marker::PhantomData,
      _lang: ::core::marker::PhantomData,
      span: ::core::clone::Clone::clone(&self.span),
      identifiers: ::core::clone::Clone::clone(&self.identifiers),
    }
  }
}

impl<S, Span, Container, Lang> ::core::marker::Copy for IdentList<S, Span, Container, Lang>
where
  Span: ::core::marker::Copy,
  Container: ::core::marker::Copy,
  Lang: ?Sized,
{
}

impl<S, Span, Container, Lang> ::core::cmp::PartialEq for IdentList<S, Span, Container, Lang>
where
  Span: ::core::cmp::PartialEq,
  Container: ::core::cmp::PartialEq,
  Lang: ?Sized,
{
  #[inline]
  fn eq(&self, other: &Self) -> bool {
    self.span == other.span && self.identifiers == other.identifiers
  }
}

impl<S, Span, Container, Lang> ::core::cmp::Eq for IdentList<S, Span, Container, Lang>
where
  Span: ::core::cmp::Eq,
  Container: ::core::cmp::Eq,
  Lang: ?Sized,
{
}

impl<S, Span, Container, Lang> ::core::hash::Hash for IdentList<S, Span, Container, Lang>
where
  Span: ::core::hash::Hash,
  Container: ::core::hash::Hash,
  Lang: ?Sized,
{
  #[inline]
  fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
    ::core::hash::Hash::hash(&self.span, state);
    ::core::hash::Hash::hash(&self.identifiers, state);
  }
}

impl<S, Span, Container, Lang: ?Sized> AsSpan<Span> for IdentList<S, Span, Container, Lang> {
  #[inline(always)]
  fn as_span(&self) -> &Span {
    self.span_ref()
  }
}

/// The three predicates below are **inherent and stay inherent**, unlike the carriers'.
///
/// They are not the same question. A carrier is in exactly one of three states, which is what
/// [`RecoveryState`] reports; a list is an aggregate, and `is_error` and `is_missing` can both be
/// true of one at the same time. No single [`Status`](super::Status) can say that, so this type
/// does not implement the trait and there is no name for it to displace — these three predate
/// tokora#320 and are unchanged by it.
impl<S, Span, Container, Lang: ?Sized> IdentList<S, Span, Container, Lang> {
  /// Returns `true` if all identifiers in the path are valid.
  #[inline(always)]
  pub fn is_valid(&self) -> bool
  where
    Container: AsRef<[Ident<S, Span, Lang>]>,
  {
    self.identifiers.as_ref().iter().all(|seg| seg.is_valid())
  }

  /// Returns `true` if any segment in the path is an error node.
  #[inline(always)]
  pub fn is_error(&self) -> bool
  where
    Container: AsRef<[Ident<S, Span, Lang>]>,
  {
    self.identifiers.as_ref().iter().any(|seg| seg.is_error())
  }

  /// Returns `true` if any segment in the path is a missing node.
  #[inline(always)]
  pub fn is_missing(&self) -> bool
  where
    Container: AsRef<[Ident<S, Span, Lang>]>,
  {
    self.identifiers.as_ref().iter().any(|seg| seg.is_missing())
  }
}

impl<S, Span, Container, Lang: ?Sized> IdentList<S, Span, Container, Lang> {
  /// Create a new path.
  #[inline(always)]
  pub const fn new(span: Span, identifiers: Container) -> Self {
    Self {
      span,
      identifiers,
      _m: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Get the span of the path.
  #[inline(always)]
  pub const fn span(&self) -> Span
  where
    Span: Copy,
  {
    self.span
  }

  /// Get the reference to the span of the path.
  #[inline(always)]
  pub const fn span_ref(&self) -> &Span {
    &self.span
  }

  /// Get the mutable reference to the span of the path.
  #[inline(always)]
  pub const fn span_mut(&mut self) -> &mut Span {
    &mut self.span
  }

  /// Bump the span of the path by the given offset.
  #[inline(always)]
  pub fn bump(&mut self, by: &Span::Offset) -> &mut Self
  where
    Span: crate::span::Span,
    Container: AsMut<[Ident<S, Span, Lang>]>,
  {
    self.span.bump(by);
    self.identifiers.as_mut().iter_mut().for_each(|seg| {
      seg.bump(by);
    });
    self
  }

  /// Get the identifiers of the path.
  #[inline(always)]
  pub const fn identifiers(&self) -> &Container {
    &self.identifiers
  }

  /// Returns the slice of the path identifiers.
  #[inline(always)]
  pub fn identifiers_slice(&self) -> &[Ident<S, Span, Lang>]
  where
    Container: AsRef<[Ident<S, Span, Lang>]>,
  {
    self.identifiers.as_ref()
  }

  /// Returns `true` if the path has no identifiers.
  #[inline(always)]
  pub fn is_empty(&self) -> bool
  where
    Container: AsRef<[Ident<S, Span, Lang>]>,
  {
    self.identifiers.as_ref().is_empty()
  }
}
