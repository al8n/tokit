#![allow(single_use_lifetimes)]

mod sealed {
  pub trait Sealed<T: ?Sized> {}

  impl<T, O> Sealed<O> for &T where T: Sealed<O> + ?Sized {}
}

/// A trait for converting to an equivalent type.
///
/// e.g. `&[u8]` is equivalent to [`bytes::Bytes`](https://docs.rs/bytes/latest/bytes/struct.Bytes.html)
pub trait ToEquivalent<T>: sealed::Sealed<T> {
  /// Converts this element to an equivalent type `T`.
  fn to_equivalent(&self) -> T;
}

impl<T, O> ToEquivalent<O> for &T
where
  T: ToEquivalent<O> + ?Sized,
{
  #[inline(always)]
  fn to_equivalent(&self) -> O {
    T::to_equivalent(self)
  }
}

impl sealed::Sealed<str> for str {}
impl<'de: 'a, 'a> sealed::Sealed<&'a str> for &'de str {}

impl sealed::Sealed<[u8]> for [u8] {}
impl<'de: 'a, 'a> sealed::Sealed<&'a [u8]> for &'de [u8] {}

impl<'de: 'a, 'a> ToEquivalent<&'a str> for &'de str {
  #[inline(always)]
  fn to_equivalent(&self) -> &'a str {
    self
  }
}

impl<'de: 'a, 'a> ToEquivalent<&'a [u8]> for &'de [u8] {
  #[inline(always)]
  fn to_equivalent(&self) -> &'a [u8] {
    self
  }
}

/// A trait for converting into an equivalent type.
///
/// e.g. `&[u8]` is equivalent to [`bytes::Bytes`](https://docs.rs/bytes/latest/bytes/struct.Bytes.html)
///
/// ## Example
///
/// ```rust
/// use tokora::utils::IntoEquivalent;
///
/// let bytes: &[u8] = b"hello";
/// let str_ref: &str = "world";
///
/// // Identity conversions (consuming)
/// let _: &[u8] = bytes.into_equivalent();
/// let _: &str = str_ref.into_equivalent();
/// ```
pub trait IntoEquivalent<T>: sealed::Sealed<T> {
  /// Consumes this element and converts it into an equivalent type `T`.
  ///
  /// ## Example
  ///
  /// ```rust
  /// use tokora::utils::IntoEquivalent;
  ///
  /// let data: &[u8] = b"test";
  /// let result: &[u8] = data.into_equivalent();
  /// assert_eq!(result, b"test");
  /// ```
  fn into_equivalent(self) -> T;
}

impl<'de: 'a, 'a> IntoEquivalent<&'a str> for &'de str {
  #[inline(always)]
  fn into_equivalent(self) -> &'a str {
    self
  }
}

impl<'de: 'a, 'a> IntoEquivalent<&'a [u8]> for &'de [u8] {
  #[inline(always)]
  fn into_equivalent(self) -> &'a [u8] {
    self
  }
}

#[cfg(test)]
#[allow(warnings)]
mod tests;

#[cfg(feature = "bytes_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
const _: () = {
  use bytes_1::Bytes;
  use sealed::Sealed;

  impl Sealed<Bytes> for [u8] {}

  impl ToEquivalent<Bytes> for [u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> Bytes {
      Bytes::copy_from_slice(self)
    }
  }

  impl IntoEquivalent<Bytes> for &[u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> Bytes {
      Bytes::copy_from_slice(self)
    }
  }

  fn __assert_bytes_to_equivalent_impl() {
    fn _assert<T: ToEquivalent<Bytes> + ?Sized>() {}

    _assert::<[u8]>();
    _assert::<&[u8]>();
  }

  fn __assert_bytes_into_equivalent_impl() {
    fn _assert<T: IntoEquivalent<Bytes> + ?Sized>() {}

    _assert::<&[u8]>();
  }
};

#[cfg(feature = "bstr_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bstr_1")))]
const _: () = {
  use bstr_1::BStr;
  use sealed::Sealed;

  // `BStr` is a `repr(transparent)` view over `[u8]`, so the conversion is a reborrow rather
  // than a copy — which is why this mirrors the `&'de str -> &'a str` identity idiom above
  // rather than the owned-handle idiom `bytes_1`/`hipstr` use. `[u8]` cannot implement
  // `ToEquivalent<&'a BStr>` directly: `to_equivalent(&self)` has only an anonymous borrow to
  // hand back, and the reference form is where the source lifetime is nameable.
  impl<'de: 'a, 'a> Sealed<&'a BStr> for &'de [u8] {}

  impl<'de: 'a, 'a> ToEquivalent<&'a BStr> for &'de [u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> &'a BStr {
      BStr::new(*self)
    }
  }

  impl<'de: 'a, 'a> IntoEquivalent<&'a BStr> for &'de [u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> &'a BStr {
      BStr::new(self)
    }
  }

  // The lattice row. It fails to compile if either half of this family's two files loses its
  // BStr block, which is the anti-drift mechanism — the reason the two files are deliberately
  // not collapsed into one macro in this release.
  fn __assert_bstr_equivalence_family() {
    fn _to<'a, T: ToEquivalent<&'a BStr>>() {}
    fn _into<'a, T: IntoEquivalent<&'a BStr>>() {}

    _to::<&[u8]>();
    _into::<&[u8]>();
  }
};

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
const _: () = {
  use hipstr_0_8::{HipByt, HipStr};
  use sealed::Sealed;

  impl Sealed<HipStr<'_>> for str {}

  impl<'a> ToEquivalent<HipStr<'a>> for str {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> HipStr<'a> {
      HipStr::from(self)
    }
  }

  impl<'a> IntoEquivalent<HipStr<'a>> for &str {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> HipStr<'a> {
      HipStr::from(self)
    }
  }

  impl Sealed<HipByt<'_>> for [u8] {}

  impl<'a> ToEquivalent<HipByt<'a>> for [u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> HipByt<'a> {
      HipByt::from(self)
    }
  }

  impl<'a> IntoEquivalent<HipByt<'a>> for &[u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> HipByt<'a> {
      HipByt::from(self)
    }
  }
};

#[cfg(feature = "smol_bytes_0_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "smol_bytes_0_1")))]
const _: () = {
  use sealed::Sealed;
  use smol_bytes_0_1::{Utf8Bytes, compact, shared};

  impl Sealed<shared::Bytes> for [u8] {}

  impl ToEquivalent<shared::Bytes> for [u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> shared::Bytes {
      shared::Bytes::copy_from_slice(self)
    }
  }

  impl IntoEquivalent<shared::Bytes> for &[u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> shared::Bytes {
      shared::Bytes::copy_from_slice(self)
    }
  }

  impl Sealed<compact::Bytes> for [u8] {}

  impl ToEquivalent<compact::Bytes> for [u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> compact::Bytes {
      compact::Bytes::copy_from_slice(self)
    }
  }

  impl IntoEquivalent<compact::Bytes> for &[u8] {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> compact::Bytes {
      compact::Bytes::copy_from_slice(self)
    }
  }

  impl Sealed<Utf8Bytes> for str {}

  impl ToEquivalent<Utf8Bytes> for str {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> Utf8Bytes {
      Utf8Bytes::from(self)
    }
  }

  impl IntoEquivalent<Utf8Bytes> for &str {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> Utf8Bytes {
      Utf8Bytes::from(self)
    }
  }

  impl Sealed<compact::Utf8Bytes> for str {}

  impl ToEquivalent<compact::Utf8Bytes> for str {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn to_equivalent(&self) -> compact::Utf8Bytes {
      compact::Utf8Bytes::from(self)
    }
  }

  impl IntoEquivalent<compact::Utf8Bytes> for &str {
    #[cfg_attr(test, inline)]
    #[cfg_attr(not(test), inline(always))]
    fn into_equivalent(self) -> compact::Utf8Bytes {
      compact::Utf8Bytes::from(self)
    }
  }

  fn __assert_smol_bytes_to_equivalent_impl() {
    fn _assert_shared<T: ToEquivalent<shared::Bytes> + ?Sized>() {}
    fn _assert_compact<T: ToEquivalent<compact::Bytes> + ?Sized>() {}
    fn _assert_utf8<T: ToEquivalent<Utf8Bytes> + ?Sized>() {}
    fn _assert_compact_utf8<T: ToEquivalent<compact::Utf8Bytes> + ?Sized>() {}

    _assert_shared::<[u8]>();
    _assert_shared::<&[u8]>();
    _assert_compact::<[u8]>();
    _assert_compact::<&[u8]>();
    _assert_utf8::<str>();
    _assert_utf8::<&str>();
    _assert_compact_utf8::<str>();
    _assert_compact_utf8::<&str>();
  }

  fn __assert_smol_bytes_into_equivalent_impl() {
    fn _assert_shared<T: IntoEquivalent<shared::Bytes> + ?Sized>() {}
    fn _assert_compact<T: IntoEquivalent<compact::Bytes> + ?Sized>() {}
    fn _assert_utf8<T: IntoEquivalent<Utf8Bytes> + ?Sized>() {}
    fn _assert_compact_utf8<T: IntoEquivalent<compact::Utf8Bytes> + ?Sized>() {}

    _assert_shared::<&[u8]>();
    _assert_compact::<&[u8]>();
    _assert_utf8::<&str>();
    _assert_compact_utf8::<&str>();
  }
};
