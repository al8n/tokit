/// A marker type representing ignored values.
pub struct Ignored<T: ?Sized>(core::marker::PhantomData<T>);

impl<T> From<T> for Ignored<T> {
  #[inline(always)]
  fn from(_: T) -> Self {
    Self(core::marker::PhantomData)
  }
}

impl<T: ?Sized> Default for Ignored<T> {
  #[inline(always)]
  fn default() -> Self {
    Self(core::marker::PhantomData)
  }
}

impl<T: ?Sized> core::fmt::Debug for Ignored<T> {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Ignored")
  }
}

impl<T: ?Sized> Clone for Ignored<T> {
  #[inline(always)]
  fn clone(&self) -> Self {
    *self
  }
}

impl<T: ?Sized> Copy for Ignored<T> {}

/// A span with no meaningful offsets, used as a type marker.
pub type PhantomSpan = crate::span::SimpleSpan<()>;

impl PhantomSpan {
  /// A zero-sized span for phantom usage.
  pub const PHANTOM: Self = Self { start: (), end: () };

  /// Returns the phantom span.
  #[inline(always)]
  pub const fn phantom() -> Self {
    Self::PHANTOM
  }
}

/// A sliced value with no meaningful slice, used as a type marker.
pub type PhantomSliced = crate::slice::Sliced<(), ()>;

impl PhantomSliced {
  /// A zero-sized sliced value for phantom usage.
  pub const PHANTOM: Self = Self {
    slice: (),
    data: (),
  };

  /// Returns the phantom sliced value.
  #[inline(always)]
  pub const fn phantom() -> Self {
    Self::PHANTOM
  }
}

/// A located value with no meaningful source or span, used as a type marker.
pub type PhantomLocated = crate::located::Located<(), (), ()>;

impl PhantomLocated {
  /// A zero-sized located value for phantom usage.
  pub const PHANTOM: Self = Self {
    slice: (),
    span: (),
    data: (),
  };

  /// Returns the phantom located value.
  #[inline(always)]
  pub const fn phantom() -> Self {
    Self::PHANTOM
  }
}

/// A phantom delimited value with no meaningful delimiters, span, or data, used as a type marker.
pub type PhantomDelimited = super::Delimited<(), (), (), ()>;

impl PhantomDelimited {
  /// A zero-sized delimited value for phantom usage.
  pub const PHANTOM: Self = Self {
    open: (),
    close: (),
    span: (),
    data: (),
  };

  /// Returns the phantom delimited value.
  #[inline(always)]
  pub const fn phantom() -> Self {
    Self::PHANTOM
  }
}
