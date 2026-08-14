//! The materialization view of a lexer source: [`CstText`], the one thing a green tree needs
//! from the buffer a parse ran over.
//!
//! A [`Source`](crate::source::Source) may be text or bytes; a `rowan` green tree is `&str`
//! all the way down. This trait is the narrow bridge: every shipped source type implements it,
//! string-like ones infallibly and byte-like ones by validating, and a downstream source type
//! implements it to become materializable.

use core::str::Utf8Error;

/// A source a green tree can be built from: one that can present its bytes as `&str`.
///
/// [`Cst::finish`](super::Cst::finish) slices every token's text out of the source it was
/// constructed with, and `rowan` stores text as `&str`, so materialization needs exactly this
/// one view and nothing else. Implementations divide cleanly:
///
/// - **string-like** sources (`str`, `HipStr`, the UTF-8 byte containers) are already valid
///   and return `Ok` without inspecting anything;
/// - **byte-like** sources (`[u8]`, `Bytes`, `BStr`, `HipByt`) validate, and a source that is
///   not UTF-8 surfaces as [`FinishError::NonUtf8Source`](super::FinishError::NonUtf8Source)
///   rather than as a lossy or truncated tree.
///
/// The trait is not sealed: a downstream source type opts into the lossless CST door by
/// implementing it.
pub trait CstText {
  /// This source's bytes as text.
  ///
  /// # Errors
  ///
  /// Returns the [`Utf8Error`] of the first invalid sequence when the source is byte-like and
  /// not valid UTF-8. String-like sources never fail.
  fn cst_text(&self) -> Result<&str, Utf8Error>;
}

impl CstText for str {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    Ok(self)
  }
}

impl CstText for [u8] {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    core::str::from_utf8(self)
  }
}

/// The borrowed sources — `&str`, `&[u8]`, `&BStr` — reach the trait through their referent,
/// mirroring the `Source` impls the `impl_source_for_borrowed!` macro and the `bstr` module
/// generate for exactly those forms.
impl<T> CstText for &T
where
  T: CstText + ?Sized,
{
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    T::cst_text(*self)
  }
}

#[cfg(feature = "bytes_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bytes_1")))]
impl CstText for bytes_1::Bytes {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    core::str::from_utf8(self)
  }
}

#[cfg(feature = "bstr_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "bstr_1")))]
impl CstText for bstr_1::BStr {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    core::str::from_utf8(self)
  }
}

#[cfg(feature = "smol_bytes_0_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "smol_bytes_0_1")))]
impl CstText for smol_bytes_0_1::shared::Bytes {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    core::str::from_utf8(self)
  }
}

#[cfg(feature = "smol_bytes_0_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "smol_bytes_0_1")))]
impl CstText for smol_bytes_0_1::compact::Bytes {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    core::str::from_utf8(self)
  }
}

#[cfg(feature = "smol_bytes_0_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "smol_bytes_0_1")))]
impl CstText for smol_bytes_0_1::Utf8Bytes {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    Ok(self.as_str())
  }
}

#[cfg(feature = "smol_bytes_0_1")]
#[cfg_attr(docsrs, doc(cfg(feature = "smol_bytes_0_1")))]
impl CstText for smol_bytes_0_1::compact::Utf8Bytes {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    Ok(self.as_str())
  }
}

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
impl CstText for hipstr_0_8::HipStr<'_> {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    Ok(self.as_str())
  }
}

#[cfg(feature = "hipstr_0_8")]
#[cfg_attr(docsrs, doc(cfg(feature = "hipstr_0_8")))]
impl CstText for hipstr_0_8::HipByt<'_> {
  #[inline(always)]
  fn cst_text(&self) -> Result<&str, Utf8Error> {
    core::str::from_utf8(self)
  }
}
