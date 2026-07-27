//! Emitter capability for the unclosed-delimiter diagnostic.
//!
//! [`UnclosedEmitter`] is the delimiter-family twin of
//! [`FullContainerEmitter`](crate::emitter::FullContainerEmitter): the additive,
//! atomically-composable emit surface the delimited many-builders
//! (`.delimited::<D>().collect()`) reach for when an opener has been committed but the
//! matching closer never arrives before end-of-input.

use crate::{Lexer, error::Unclosed};

use super::Emitter;

/// The one conversion that absorbs an [`Unclosed`] diagnostic for **every** delimiter pair.
///
/// [`Unclosed<D, S, Lang>`](Unclosed) carries the delimiter pair's name as *data* — the
/// `D` parameter is pure [`PhantomData`](core::marker::PhantomData) — so a single generic
/// **method** can quantify over `D` where a `where` clause cannot: an error type states
/// `FromUnclosed` once instead of one `From<Unclosed<D, …>>` bound per pair, and the
/// quantification covers user-defined pairs a fixed list of supertraits never could.
///
/// # Discriminating the pair
///
/// Discrimination moves from the type level to the runtime name
/// ([`Unclosed::name_ref`]), which is [`Delimiter::name`](crate::delimiter::Delimiter::name)
/// for the pair that failed to close — `"()"`, `"<>"`, `"[]"` and `"{}"` for the four
/// built-in pairs. That is an error-path read only; a happy parse never calls it. Because
/// `from_unclosed` is generic over `D`, a `match` on the name can never be proven
/// exhaustive: the catch-all arm is mandatory, and it must produce an error rather than
/// panic.
///
/// A type that also wants type-level discrimination for a specific pair can still write
/// `impl From<Unclosed<Paren, …>>` alongside this impl; the two do not overlap.
///
/// # Examples
///
/// ```rust
/// use tokora::{Lexer, emitter::FromUnclosed, error::Unclosed};
///
/// #[derive(Debug, PartialEq)]
/// enum MyError {
///   UnclosedParen,
///   Unclosed(&'static str),
/// }
///
/// impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for MyError
/// where
///   L: Lexer<'inp>,
/// {
///   fn from_unclosed<D>(err: Unclosed<D, L::Span, Lang>) -> Self {
///     match err.name_ref() {
///       "()" => MyError::UnclosedParen,
///       // Mandatory: `D` is generic, so no arm set is exhaustive.
///       _ => MyError::Unclosed("other"),
///     }
///   }
/// }
/// ```
#[diagnostic::on_unimplemented(
  message = "`{Self}` cannot absorb an unclosed-delimiter diagnostic for lexer `{L}`",
  label = "this error type has no `FromUnclosed` impl",
  note = "one generic impl covers every delimiter pair, user-defined ones included: \
          `impl<'i, L: Lexer<'i>, Lang: ?Sized> FromUnclosed<'i, L, Lang> for {Self} \
          {{ fn from_unclosed<D>(e: Unclosed<D, L::Span, Lang>) -> Self {{ /* … */ }} }}`",
  note = "match on `Unclosed::name_ref` to discriminate the pair — `\"()\"`, `\"<>\"`, `\"[]\"`, \
          `\"{{}}\"` for the built-ins — and keep a catch-all arm: `D` is generic, so no arm \
          set is exhaustive"
)]
pub trait FromUnclosed<'inp, L, Lang: ?Sized = ()>: Sized
where
  L: Lexer<'inp>,
{
  /// Builds `Self` from the [`Unclosed`] diagnostic of an arbitrary delimiter pair `D`.
  ///
  /// The span is the **opening** delimiter's, so the diagnostic points at the opener that
  /// was never closed; the pair is identified by [`Unclosed::name_ref`].
  fn from_unclosed<D>(err: Unclosed<D, L::Span, Lang>) -> Self;
}

/// An emitter that handles the [`Unclosed`] diagnostic — an opening delimiter that was
/// committed but whose matching closer never arrived before end-of-input.
///
/// This is the delimiter-family analogue of
/// [`FullContainerEmitter`](crate::emitter::FullContainerEmitter): an additive sub-trait the
/// delimited many-builders (`.delimited::<D>().collect()`) require so an unterminated list is
/// reported *through the emitter* rather than silently accepted. Following the house emit
/// discipline, a fail-fast emitter ([`Fatal`](crate::emitter::Fatal)) turns the emission into
/// `Err` via the [`FromUnclosed`] conversion; a recovering emitter
/// ([`Verbose`](crate::emitter::Verbose)) records it and lets the parse return the elements
/// collected so far; a dropping emitter ([`Silent`](crate::emitter::Silent),
/// [`Ignored`](crate::utils::marker::Ignored)) discards it.
///
/// The [`Unclosed`] carries the **opening** delimiter's span — so the diagnostic points at the
/// opener that was never closed — and the delimiter pair's name
/// ([`Delimiter::name`](crate::delimiter::Delimiter::name)).
pub trait UnclosedEmitter<'a, L, Lang: ?Sized = ()>: Emitter<'a, L, Lang> {
  /// Emits the [`Unclosed`] diagnostic for a delimiter whose opener was committed but whose
  /// closer never arrived before end-of-input.
  ///
  /// The `Delimiter` type parameter is the type-level delimiter tag carried by
  /// [`Unclosed`]; the diagnostic's span is the opener's span and its name is the delimiter
  /// pair's name.
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
    Self::Error: FromUnclosed<'a, L, Lang>;
}

impl<'a, L, U, Lang: ?Sized> UnclosedEmitter<'a, L, Lang> for &mut U
where
  U: UnclosedEmitter<'a, L, Lang>,
{
  #[inline(always)]
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
    Self::Error: FromUnclosed<'a, L, Lang>,
  {
    (**self).emit_unclosed(err)
  }
}
