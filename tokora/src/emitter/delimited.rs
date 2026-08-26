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
/// Discrimination moves from the type level to the runtime
/// [`DelimiterKind`](crate::delimiter::DelimiterKind) the [`Unclosed`] carries
/// ([`Unclosed::kind`]) — [`DelimiterKind::Paren`](crate::delimiter::DelimiterKind::Paren),
/// `Angle`, `Bracket` and `Brace` for the four built-in pairs, and
/// [`Custom`](crate::delimiter::DelimiterKind::Custom) for a pair defined outside tokora, which
/// should declare that variant for itself. That is an error-path read only; a happy parse never
/// calls it.
///
/// **Do not discriminate on [`Unclosed::name_ref`].** The name is a display string with no
/// uniqueness contract: `"[]"` is the correct name for any bracket-shaped pair, so routing on
/// it silently aliases a custom pair onto the built-in arm — the author doing exactly what
/// `name`'s documentation asks. `DelimiterKind` does not have that failure: the four built-in
/// variants are themselves `#[non_exhaustive]`, so a pair defined outside tokora cannot spell
/// one and should declare `Custom` for an identity it owns. Read
/// [`DelimiterKind`](crate::delimiter::DelimiterKind) for how far that reaches — it does not
/// make two *custom* pairs distinguishable for you, and it is a fence against dispatch keyed
/// on a display string rather than a proof of provenance: three public routes still hand a
/// built-in kind to a crate that is not tokora, and only two of them name anything. The third
/// is a copy — a generic adapter forwarding an error can pass `err.kind()` straight back into
/// `Unclosed::of` — so a built-in arm firing does not mean the caller chose it.
///
/// That fence is also why a built-in arm written outside tokora reads
/// `DelimiterKind::Paren { .. }` and not `DelimiterKind::Paren`: `#[non_exhaustive]` on a
/// variant refuses construction while leaving the braced pattern form matchable.
///
/// The catch-all arm is mandatory twice over: `from_unclosed` is generic over `D`, so no arm
/// set is exhaustive, and `DelimiterKind` is `#[non_exhaustive]`. It must produce an error
/// rather than panic.
///
/// A type that also wants type-level discrimination for a specific pair can still write
/// `impl From<Unclosed<Paren, …>>` alongside this impl; the two do not overlap.
///
/// # Examples
///
/// ```rust
/// use tokora::{Lexer, delimiter::DelimiterKind, emitter::FromUnclosed, error::Unclosed};
///
/// #[derive(Debug, PartialEq)]
/// enum MyError {
///   UnclosedParen,
///   UnclosedDocComment,
///   Unclosed(&'static str),
/// }
///
/// impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for MyError
/// where
///   L: Lexer<'inp>,
/// {
///   fn from_unclosed<D>(err: Unclosed<D, L::Span, Lang>) -> Self {
///     match err.kind() {
///       // The braces are required outside tokora: the built-in variants are
///       // `#[non_exhaustive]`, which keeps them matchable but not writable.
///       DelimiterKind::Paren { .. } => MyError::UnclosedParen,
///       // A pair this crate defines, keyed by whatever its `Delimiter::KIND` declares.
///       DelimiterKind::Custom("my_crate::DocComment") => MyError::UnclosedDocComment,
///       // Mandatory: `D` is generic and `DelimiterKind` is non-exhaustive.
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
  note = "pin the span if your error stores it — an opaque `L::Span` has no route to a \
          concrete span type: `L: Lexer<'i, Span = SimpleSpan>`",
  note = "match on `Unclosed::kind` to discriminate the pair — `DelimiterKind::Paren {{ .. }}` \
          and its `Angle`/`Bracket`/`Brace` siblings for the built-ins, where the braces are \
          required because those variants are `#[non_exhaustive]` and only tokora can write \
          one, and `DelimiterKind::Custom(\"your-key\")` for your own — and keep a catch-all \
          arm: `D` is generic and `DelimiterKind` is non-exhaustive, so no arm set is \
          exhaustive. Do not match on `Unclosed::name_ref`: it is a display string, and two \
          pairs may share one"
)]
pub trait FromUnclosed<'inp, L, Lang: ?Sized = ()>: Sized
where
  L: Lexer<'inp>,
{
  /// Builds `Self` from the [`Unclosed`] diagnostic of an arbitrary delimiter pair `D`.
  ///
  /// The span is the **opening** delimiter's, so the diagnostic points at the opener that
  /// was never closed; the pair is identified by [`Unclosed::kind`].
  /// [`Unclosed::name_ref`] is for rendering, not for routing.
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
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " ([`Verbose`](crate::emitter::Verbose)) records it and lets the parse return the elements"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " (`Verbose`) records it and lets the parse return the elements"
)]
/// collected so far; a dropping emitter ([`Silent`](crate::emitter::Silent),
/// [`Ignored`](crate::utils::marker::Ignored)) discards it.
///
/// The [`Unclosed`] carries the **opening** delimiter's span — so the diagnostic points at the
/// opener that was never closed — the pair's machine identity
/// ([`Delimiter::KIND`](crate::delimiter::Delimiter::KIND), which is what a conversion should
/// route on) and its display name
/// ([`Delimiter::name`](crate::delimiter::Delimiter::name)).
#[diagnostic::on_unimplemented(
  message = "`{Self}` cannot report an unclosed delimiter for lexer `{L}`",
  label = "missing `UnclosedEmitter` — a `ComposableEmitter` bundle member",
  note = "implement `UnclosedEmitter` (usually alongside the other members of the `ComposableEmitter` bundle)"
)]
pub trait UnclosedEmitter<'a, L, Lang: ?Sized = ()>: Emitter<'a, L, Lang> {
  /// Emits the [`Unclosed`] diagnostic for a delimiter whose opener was committed but whose
  /// closer never arrived before end-of-input.
  ///
  /// The `Delimiter` type parameter is the type-level delimiter tag carried by
  /// [`Unclosed`]; the diagnostic's span is the opener's span, its kind is the pair's
  /// [`DelimiterKind`](crate::delimiter::DelimiterKind) and its name is the pair's display
  /// name.
  ///
  /// # Why there is no `FromUnclosed` bound here
  ///
  /// [`FromUnclosed`] is named by the **implementations that convert** —
  /// [`Fatal`](crate::emitter::Fatal) returns the converted value and `Verbose` records it, so
  /// both impl blocks carry `E: FromUnclosed` — and by nothing else.
  /// [`Silent`](crate::emitter::Silent) and [`Ignored`](crate::utils::marker::Ignored) discard
  /// the diagnostic and their impls are bound-free, exactly like their siblings across the
  /// rest of the emitter family.
  ///
  /// This method used to carry `Self::Error: FromUnclosed` instead, on a "pay when you call"
  /// argument. The argument does not survive contact with the dropping emitters, which is why
  /// it is recorded here rather than removed silently: a method-level bound is paid by
  /// **every** caller of the capability, including the ones whose implementation is `Ok(())`.
  /// `Silent<E>` could not call its own no-op unless `E` implemented a conversion the body
  /// never performs, `ComposableEmitter` advertised a member that its own bound could not
  /// call, and every delimited driver restated the conversion on the error type of an emitter
  /// that might never convert anything. A bound derived from trait surface rather than from
  /// behaviour is the defect this crate removed from `Silent`'s pratt impl, and
  /// `PrattEmitter` is the shape that repair left behind:
  /// bound-free method, conversion on the converting impls.
  ///
  /// What it costs: an implementor whose body performs the conversion states it on its own
  /// impl block, as the two in-crate converting emitters do. What it buys: the bound is
  /// collected only where something is actually converted.
  ///
  /// A dropping emitter over an error type that implements *nothing* satisfies the capability,
  /// and the bound alone is enough to call the method:
  ///
  /// ```rust
  /// use tokora::{Lexer, emitter::{Silent, UnclosedEmitter}};
  ///
  /// struct Opaque;
  ///
  /// fn has_capability<'inp, L: Lexer<'inp>, E: UnclosedEmitter<'inp, L>>() {}
  ///
  /// fn probe<'inp, L: Lexer<'inp>>() {
  ///   has_capability::<L, Silent<Opaque>>();
  /// }
  /// ```
  ///
  /// The converting emitter over the same error type does not, because its body really does
  /// perform the conversion. `Fatal`'s impl block names two things — `E: FromUnclosed` and
  /// `Fatal<E, Lang>: Emitter<'a, L, Lang, Error = E>` — so the whole `Emitter` bound
  /// **including its `Error` projection** is assumed here, and the missing conversion is then
  /// the only obligation left standing. Assuming the bare `Emitter<'inp, L>` instead leaves the
  /// `Error == Opaque` equality outstanding as a *second* unsatisfied obligation, and a cell
  /// with two of them pins whichever the solver happens to report: it was `E0277` on one rustc
  /// and `E0271` on the next, and under the latter the cell stayed red even once `Opaque`
  /// implemented the conversion — red for a reason the repair does not remove, which is a cell
  /// that witnesses nothing.
  ///
  /// ```rust,compile_fail,E0277
  /// use tokora::{Lexer, emitter::{Emitter, Fatal, UnclosedEmitter}};
  ///
  /// struct Opaque;
  ///
  /// fn has_capability<'inp, L: Lexer<'inp>, E: UnclosedEmitter<'inp, L>>() {}
  ///
  /// fn probe<'inp, L: Lexer<'inp>>()
  /// where
  ///   Fatal<Opaque>: Emitter<'inp, L, Error = Opaque>,
  /// {
  ///   // `Opaque` has no `FromUnclosed` impl, and `Fatal`'s body needs one.
  ///   has_capability::<L, Fatal<Opaque>>();
  /// }
  /// ```
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: Unclosed<Delimiter, L::Span, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>;
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
  {
    (**self).emit_unclosed(err)
  }
}
