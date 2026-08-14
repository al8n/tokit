use core::marker::PhantomData;

use crate::{span::AsSpan, types::Ident};

/// A list of identifiers.
///
/// `S` remains sized because the default container stores `Ident<S, Span>` values
/// inline in a `Vec`; use the individual [`Ident`] carrier for an unsized source.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg(not(any(feature = "alloc", feature = "std")))]
pub struct IdentList<S, Span, Container, Lang: ?Sized = ()> {
  span: Span,
  identifiers: Container,
  _m: PhantomData<S>,
  _lang: PhantomData<Lang>,
}

impl<S, Span, Container, Lang: ?Sized> AsSpan<Span> for IdentList<S, Span, Container, Lang> {
  #[inline(always)]
  fn as_span(&self) -> &Span {
    self.span_ref()
  }
}

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
