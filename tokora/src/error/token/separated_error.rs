//! Separator-position-tagged unexpected-token error for separated sequences.
//!
//! [`SeparatedError`] wraps an [`UnexpectedToken`] together with the
//! [`SeparatorPosition`] at which it occurred. Carrying the position as **data**
//! (rather than encoding it in the `Lang` type parameter of `UnexpectedToken`)
//! lets a downstream error type absorb leading / trailing separator errors
//! through a single `From<SeparatedError<..>>` impl and tell them apart via the
//! [`position`](SeparatedError::position) field.
//!
//! The separator's own [`name`](SeparatedError::name) rides as data for the same reason. The
//! driver knows it — `","`, `"comma"`, whatever the punctuator calls itself — and the
//! conversion into a downstream error type happens inside a blanket impl that a downstream
//! type cannot override (doing so is a coherence error, since it already implements
//! `From<SeparatedError<..>>`). So the blanket has to put the name *in the payload*, or the
//! name is lost for every user of the family.

use crate::{Lexer, Token, error::token::UnexpectedToken, span::SimpleSpan, utils::CowStr};

/// Where, within a separated sequence, a separator-related error occurred.
///
/// Used as a data field on [`SeparatedError`] instead of overloading the `Lang`
/// type slot of [`UnexpectedToken`] with position marker types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, derive_more::Display, derive_more::IsVariant)]
#[display("{}", self.as_str())]
pub enum SeparatorPosition {
  /// The error occurred where a sequence **element** was expected — for example
  /// a separator found in place of an element (a repeated / duplicate
  /// separator, or a missing element between two separators).
  Element,
  /// The error occurred at a **leading** separator — one appearing before the
  /// first element, where the sequence's policy does not permit it.
  Leading,
  /// The error occurred at a **trailing** separator — one appearing after the
  /// last element, where the sequence's policy does not permit it.
  Trailing,
}

impl SeparatorPosition {
  /// Returns the static, lowercase string name of this position
  /// (`"element"`, `"leading"`, or `"trailing"`).
  #[inline(always)]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Element => "element",
      Self::Leading => "leading",
      Self::Trailing => "trailing",
    }
  }
}

/// A type alias for a [`SeparatedError`] for a given lexer and language.
pub type SeparatedErrorOf<'inp, L, Lang = ()> = SeparatedError<
  'inp,
  <L as Lexer<'inp>>::Token,
  <<L as Lexer<'inp>>::Token as Token<'inp>>::Kind,
  <L as Lexer<'inp>>::Span,
  Lang,
>;

/// An [`UnexpectedToken`] error tagged with the [`SeparatorPosition`] at which
/// it was produced within a separated sequence.
///
/// This is the payload the separator emitter conversion traits speak: the
/// leading / trailing separator emitters wrap the offending token here and
/// stamp the position, so a downstream error type distinguishes the cases by
/// reading [`position`](Self::position) rather than by matching distinct types.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SeparatedError<'a, T, Kind: Clone, S = SimpleSpan, Lang: ?Sized = ()> {
  position: SeparatorPosition,
  name: Option<CowStr>,
  inner: UnexpectedToken<'a, T, Kind, S, Lang>,
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> SeparatedError<'a, T, Kind, S, Lang> {
  /// Creates a new `SeparatedError` at `position` wrapping `inner`.
  #[inline(always)]
  pub const fn new(
    position: SeparatorPosition,
    inner: UnexpectedToken<'a, T, Kind, S, Lang>,
  ) -> Self {
    Self {
      position,
      name: None,
      inner,
    }
  }

  /// Creates a `SeparatedError` at the [`Leading`](SeparatorPosition::Leading) position.
  #[inline(always)]
  pub const fn leading(inner: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Self::new(SeparatorPosition::Leading, inner)
  }

  /// Creates a `SeparatedError` at the [`Trailing`](SeparatorPosition::Trailing) position.
  #[inline(always)]
  pub const fn trailing(inner: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Self::new(SeparatorPosition::Trailing, inner)
  }

  /// Creates a `SeparatedError` at the [`Element`](SeparatorPosition::Element) position.
  #[inline(always)]
  pub const fn element(inner: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Self::new(SeparatorPosition::Element, inner)
  }

  /// Stamps the human-readable separator name the driver supplied (`Sep::name()`), so a
  /// downstream `From<SeparatedError<..>>` impl — and the default diagnostics — can say
  /// *which* separator was involved.
  #[inline(always)]
  pub fn with_name(self, name: CowStr) -> Self {
    Self {
      position: self.position,
      name: Some(name),
      inner: self.inner,
    }
  }

  /// Returns the position at which this separator error occurred.
  #[inline(always)]
  pub const fn position(&self) -> SeparatorPosition {
    self.position
  }

  /// Returns the stamped separator name, if any — see [`with_name`](Self::with_name).
  #[inline(always)]
  pub const fn name(&self) -> Option<&CowStr> {
    self.name.as_ref()
  }

  /// Returns a reference to the wrapped [`UnexpectedToken`].
  #[inline(always)]
  pub const fn inner_ref(&self) -> &UnexpectedToken<'a, T, Kind, S, Lang> {
    &self.inner
  }

  /// Returns a mutable reference to the wrapped [`UnexpectedToken`].
  #[inline(always)]
  pub const fn inner_mut(&mut self) -> &mut UnexpectedToken<'a, T, Kind, S, Lang> {
    &mut self.inner
  }

  /// Consumes the error, returning the wrapped [`UnexpectedToken`] and **dropping** both the
  /// [`position`](Self::position) and the stamped [`name`](Self::name) — this is the "I only
  /// want the token" seam. Use [`into_components`](Self::into_components) to take the error
  /// apart without losing what makes it a *separator* error.
  #[inline(always)]
  pub fn into_inner(self) -> UnexpectedToken<'a, T, Kind, S, Lang> {
    self.inner
  }

  /// Consumes the error, returning its position, its stamped separator name, and the wrapped
  /// [`UnexpectedToken`].
  ///
  /// The name is returned rather than dropped on purpose: this is the destructuring seam of
  /// the very type that exists to carry the separator's identity, and a tuple that quietly
  /// omitted it would recreate the loss the name channel was added to close.
  #[inline(always)]
  pub fn into_components(
    self,
  ) -> (
    SeparatorPosition,
    Option<CowStr>,
    UnexpectedToken<'a, T, Kind, S, Lang>,
  ) {
    (self.position, self.name, self.inner)
  }
}

// Allow unit to be used as an error sink for tests and no-op emitters.
impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for () {
  #[inline(always)]
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {}
}

impl<T, Kind: Clone, S, Lang: ?Sized> SeparatedError<'_, T, Kind, S, Lang>
where
  S: crate::span::Span,
{
  /// Formats the error using the provided formatter in display style.
  ///
  /// Half of the [`name`](Self::name) channel's user-visible point had nothing to render
  /// through: the name was readable only by destructuring through
  /// [`into_components`](Self::into_components). A diagnostic that knows *which* separator was
  /// involved and cannot say so is a channel that exists for its author, not its reader.
  ///
  /// The name is quoted into the opening clause, matching how the rest of the crate renders a
  /// token's own name (`missing token 'comma' at 12`, `unclosed delimiter '('`). The wrapped
  /// [`UnexpectedToken`]'s own rendering follows, so the two carriers speak with one voice.
  ///
  /// ```text
  /// separator 'comma' at the leading position: unexpected token ':', expected ';'
  /// separator at the trailing position: unexpected token ':', expected ';'
  /// ```
  ///
  /// Note the single "expected". This renderer is **born** under the composition rule the two
  /// older carriers were repaired to: [`Expected`](crate::utils::Expected)'s `Display` opens
  /// with the word in every variant, so a composing site never writes it itself.
  #[inline(always)]
  pub fn display_fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result
  where
    T: core::fmt::Display,
    Kind: core::fmt::Display,
  {
    match &self.name {
      Some(name) => write!(
        f,
        "separator '{}' at the {} position: ",
        name, self.position
      )?,
      None => write!(f, "separator at the {} position: ", self.position)?,
    }
    self.inner.display_fmt(f)
  }
}

impl<T, Kind: Clone, S, Lang: ?Sized> core::fmt::Display for SeparatedError<'_, T, Kind, S, Lang>
where
  T: core::fmt::Display,
  Kind: core::fmt::Display,
  S: crate::span::Span,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    self.display_fmt(f)
  }
}

impl<T, Kind: Clone, S, Lang: ?Sized> core::fmt::Debug for SeparatedError<'_, T, Kind, S, Lang>
where
  T: core::fmt::Debug,
  Kind: core::fmt::Debug,
  S: core::fmt::Debug,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("SeparatedError")
      .field("position", &self.position)
      .field("name", &self.name)
      .field("span", self.inner.span_ref())
      .field("found", &self.inner.found())
      .field("expected", &self.inner.expected())
      .finish()
  }
}
