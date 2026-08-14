#[cfg(any(feature = "std", feature = "alloc"))]
use std::{borrow::Cow, vec::Vec};

#[cfg(not(any(feature = "std", feature = "alloc")))]
type Inner<'a, T> = &'a [T];

#[cfg(any(feature = "std", feature = "alloc"))]
type Inner<'a, T> = Cow<'a, [T]>;

/// Feature-aware slice with a unified API.
///
/// - **`no_std` + `no_alloc`**: stores a `&'a [T]`
/// - **`alloc` / `std`**: stores a `Cow<'a, [T]>`
#[derive(
  Debug,
  Clone,
  PartialEq,
  Eq,
  Hash,
  derive_more::Display,
  derive_more::AsMut,
  derive_more::AsRef,
  derive_more::Deref,
  derive_more::DerefMut,
)]
#[cfg_attr(not(any(feature = "std", feature = "alloc")), derive(Copy))]
#[repr(transparent)]
#[display("{inner}")]
pub struct OneOf<'a, T: Clone> {
  #[deref]
  #[deref_mut]
  #[as_ref]
  #[as_mut]
  inner: Inner<'a, T>,
}

impl<'a, T: Clone> OneOf<'a, T> {
  /// Creates a new `OneOf` from the provided representation.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::utils::OneOf;
  ///
  /// let values = OneOf::from_slice(&["greeting", "salutation"]);
  /// assert_eq!(values.as_slice(), &["greeting", "salutation"]);
  /// ```
  #[inline]
  pub fn new(inner: impl Into<Inner<'a, T>>) -> Self {
    Self {
      inner: inner.into(),
    }
  }

  /// Creates a `OneOf` from a slice without allocation.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::utils::OneOf;
  ///
  /// const MSG: OneOf<'static, &str> = OneOf::from_slice(&["hello"]);
  /// assert_eq!(MSG.as_slice(), &["hello"]);
  /// ```
  #[inline]
  pub const fn from_slice(value: &'a [T]) -> Self {
    Self {
      inner: Self::borrow_const(value),
    }
  }

  /// Returns the inner slice.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::utils::OneOf;
  ///
  /// let msg = OneOf::from_slice(&["world"]);
  /// assert_eq!(msg.as_slice(), &["world"]);
  /// ```
  #[inline]
  pub const fn as_slice(&self) -> &[T] {
    Self::as_inner_helper(&self.inner)
  }

  /// Borrows the inner representation.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::utils::OneOf;
  /// # #[cfg(any(feature = "std", feature = "alloc"))]
  /// # {
  /// use std::borrow::Cow;
  ///
  /// let msg = OneOf::from_slice(&["inner"]);
  /// assert!(matches!(msg.as_inner(), &Cow::Borrowed(&["inner"])));
  /// # }
  /// ```
  #[inline]
  pub const fn as_inner(&self) -> &Inner<'a, T> {
    &self.inner
  }

  /// Consumes the message and returns the inner representation.
  ///
  /// ## Examples
  ///
  /// ```
  /// use tokora::utils::OneOf;
  /// # #[cfg(any(feature = "std", feature = "alloc"))]
  /// # {
  /// use std::borrow::Cow;
  ///
  /// let msg = OneOf::from_slice(&["consume"]);
  /// let inner = msg.into_inner();
  /// assert_eq!(inner, Cow::Borrowed(&["consume"]));
  /// # }
  /// ```
  #[inline]
  pub fn into_inner(self) -> Inner<'a, T> {
    self.inner
  }
}

impl<'a, T: Clone> From<&'a [T]> for OneOf<'a, T> {
  #[inline]
  fn from(value: &'a [T]) -> Self {
    Self {
      inner: Self::borrow_const(value),
    }
  }
}

impl<T: Clone> AsRef<[T]> for OneOf<'_, T> {
  #[inline]
  fn as_ref(&self) -> &[T] {
    self.as_inner()
  }
}

impl<T: Clone> core::borrow::Borrow<[T]> for OneOf<'_, T> {
  #[inline]
  fn borrow(&self) -> &[T] {
    self.as_inner()
  }
}

#[cfg(not(any(feature = "std", feature = "alloc")))]
const _: () = {
  impl<'a, T: Clone> OneOf<'a, T> {
    #[inline]
    const fn borrow_const(value: &'a [T]) -> Inner<'a, T> {
      value
    }

    #[inline]
    const fn as_inner_helper(inner: &Inner<'a, T>) -> &'a [T] {
      inner
    }
  }

  impl<'a, T: Clone> From<OneOf<'a, T>> for &'a [T] {
    #[inline]
    fn from(value: OneOf<'a, T>) -> Self {
      value.inner
    }
  }

  impl<'a, T: Clone> From<&OneOf<'a, T>> for &'a [T] {
    #[inline]
    fn from(value: &OneOf<'a, T>) -> Self {
      value.inner
    }
  }
};

#[cfg(any(feature = "std", feature = "alloc"))]
const _: () = {
  impl<'a, T: Clone> OneOf<'a, T> {
    #[inline]
    const fn borrow_const(value: &'a [T]) -> Inner<'a, T> {
      Cow::Borrowed(value)
    }

    #[inline]
    const fn as_inner_helper<'b>(inner: &'b Inner<'a, T>) -> &'b [T] {
      match inner {
        Cow::Borrowed(value) => value,
        Cow::Owned(value) => value.as_slice(),
      }
    }

    /// Creates a message by taking ownership of a vector.
    ///
    /// ## Examples
    ///
    /// ```
    /// use tokora::utils::OneOf;
    ///
    /// let values = OneOf::from_vec(vec!["owned", "slice"]);
    /// assert_eq!(values.as_slice(), &["owned", "slice"]);
    /// ```
    #[inline]
    pub fn from_vec(value: Vec<T>) -> Self {
      Self {
        inner: Cow::Owned(value),
      }
    }

    /// Ensures the message is owned and returns a mutable reference.
    ///
    /// ## Examples
    ///
    /// ```
    /// use tokora::utils::OneOf;
    ///
    /// let mut values = OneOf::from_vec(vec![1, 2, 3]);
    /// values.to_mut()[1] = 42;
    /// assert_eq!(values.as_slice(), &[1, 42, 3]);
    /// ```
    #[inline]
    pub fn to_mut(&mut self) -> &mut [T] {
      self.inner.to_mut()
    }
  }

  impl<T: Clone> From<Vec<T>> for OneOf<'_, T> {
    #[inline]
    fn from(value: Vec<T>) -> Self {
      OneOf::from_vec(value)
    }
  }

  impl<'a, T: Clone> From<Cow<'a, [T]>> for OneOf<'a, T> {
    #[inline]
    fn from(value: Cow<'a, [T]>) -> Self {
      Self { inner: value }
    }
  }

  impl<'a, T: Clone> From<OneOf<'a, T>> for Cow<'a, [T]> {
    #[inline]
    fn from(value: OneOf<'a, T>) -> Self {
      value.inner
    }
  }

  impl<'a, T: Clone> From<&OneOf<'a, T>> for Cow<'a, [T]> {
    #[inline]
    fn from(value: &OneOf<'a, T>) -> Self {
      value.inner.clone()
    }
  }
};
