//! Missing syntax error type for parser error reporting.
//!
//! This module provides the [`MissingSyntax`] type, which represents parser errors
//! when a missing token is encountered. It captures the offset of the error and
//! an optional custom message.
//!
//! # Design Philosophy
//!
//! `MissingSyntax` is designed to provide rich, actionable error messages:
//! - **Location tracking**: The `offset` field pinpoints exactly where the error occurred
//! - **Optional message**: Attach custom context to the missing syntax
//! - **Position adjustment**: The `bump()` method allows adjusting error positions when
//!   combining errors from different parsing contexts

use core::{marker::PhantomData, ops::AddAssign};

use crate::{Lexer, utils::CowStr};

/// A type alias for a `MissingSyntax` error for a given lexer and separator.
pub type MissingSyntaxOf<'inp, L, Lang = ()> = MissingSyntax<<L as Lexer<'inp>>::Offset, Lang>;

/// An error representing a missing token encountered during parsing.
///
/// This error type captures the location (offset) and an optional message.
/// It's commonly used in parsers to provide
/// detailed error messages when the input doesn't match the expected syntax.
///
/// # Type Parameters
///
/// # Examples
///
/// ```ignore
/// use tokora::error::syntax::MissingSyntax;
///
/// // Basic missing syntax error with no message
/// let error = MissingSyntax::new(10);
/// assert_eq!(error.offset(), 10);
///
/// // Attach a custom message
/// let error = MissingSyntax::with_message(20, "expected expression".into());
/// assert_eq!(error.message().unwrap(), "expected expression");
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MissingSyntax<Offset = usize, Lang: ?Sized = ()> {
  offset: Offset,
  msg: Option<CowStr>,
  _lang: PhantomData<Lang>,
}

impl<O> MissingSyntax<O> {
  /// Creates a new missing token error.
  ///
  /// This error indicates that a missing token was encountered,
  /// without specifying what token was found or expected.
  #[inline]
  pub const fn new(offset: O) -> Self {
    Self::of(offset)
  }

  /// Sets a custom message for the error.
  #[inline]
  pub const fn with_message(offset: O, message: CowStr) -> Self {
    Self::with_message_of(offset, message)
  }
}

impl<O, Lang: ?Sized> MissingSyntax<O, Lang> {
  #[inline]
  pub(super) const fn new_in(offset: O, message: Option<CowStr>) -> Self {
    Self {
      offset,
      msg: message,
      _lang: PhantomData,
    }
  }

  /// Creates a new missing token error for the given language.
  #[inline]
  pub const fn of(offset: O) -> Self {
    Self::new_in(offset, None)
  }

  /// Sets a custom message for the error.
  #[inline]
  pub const fn with_message_of(offset: O, msg: CowStr) -> Self {
    Self::new_in(offset, Some(msg))
  }

  /// Returns the offset of the missing token.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::error::syntax::MissingSyntax;
  ///
  /// let error = MissingSyntax::new(10);
  /// assert_eq!(error.offset(), 10);
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
  /// use tokora::error::syntax::MissingSyntax;
  ///
  /// let error = MissingSyntax::new(10);
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
  /// use tokora::error::syntax::MissingSyntax;
  ///
  /// let mut error = MissingSyntax::new(10);
  /// *error.offset_mut() = 12;
  /// assert_eq!(error.offset(), 12);
  /// ```
  #[inline]
  pub const fn offset_mut(&mut self) -> &mut O {
    &mut self.offset
  }

  /// Returns the custom message associated with the error, if any.
  #[inline]
  pub const fn message(&self) -> Option<&CowStr> {
    self.msg.as_ref()
  }

  /// Returns the custom message associated with the error, if any.
  #[inline]
  pub fn message_mut(&mut self) -> Option<&mut CowStr> {
    self.msg.as_mut()
  }

  /// Bumps the offset by the given amount.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining offsets from different contexts.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::error::syntax::MissingSyntax;
  ///
  /// let mut error = MissingSyntax::new(10);
  /// error.bump(&5);
  /// assert_eq!(error.offset(), 15);
  /// ```
  #[inline]
  pub fn bump(&mut self, offset: &O) -> &mut Self
  where
    O: for<'b> AddAssign<&'b O>,
  {
    self.offset += offset;
    self
  }

  /// Consumes the error and returns its components.
  ///
  /// This method deconstructs the error into its constituent parts:
  /// the offset and the optional message.
  ///
  /// # Examples
  ///
  /// ```
  /// use tokora::error::syntax::MissingSyntax;
  ///
  /// let error = MissingSyntax::new(10);
  /// let (offset, msg) = error.into_components();
  /// assert_eq!(offset, 10);
  /// assert_eq!(msg, None);
  /// ```
  #[inline]
  pub fn into_components(self) -> (O, Option<CowStr>) {
    (self.offset, self.msg)
  }
}

impl<O, Lang> From<MissingSyntax<O, Lang>> for () {
  #[inline]
  fn from(_: MissingSyntax<O, Lang>) -> Self {}
}

impl<O, Lang: ?Sized> core::fmt::Debug for MissingSyntax<O, Lang>
where
  O: core::fmt::Debug,
{
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("MissingSyntax")
      .field("offset", &self.offset)
      .field("message", &self.msg)
      .finish()
  }
}

impl<O, Lang: ?Sized> core::fmt::Display for MissingSyntax<O, Lang>
where
  O: core::fmt::Display,
{
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.msg {
      Some(msg) => write!(f, "missing syntax at offset {}: {}", self.offset, msg),
      None => write!(f, "missing syntax at offset {}", self.offset),
    }
  }
}

impl<O, Lang: ?Sized> core::error::Error for MissingSyntax<O, Lang> where
  O: core::fmt::Debug + core::fmt::Display
{
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
