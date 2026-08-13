//! The dialect's CST profile: the whole of what a recording sink needs to know about the
//! dialect's `u16` kind space, stated once and reused at every construction.
//!
//! [`CstProfile`] bundles the four facts that used to arrive as loose arguments — the token
//! mapper, the kind [`validator`](CstProfile::validator), the recovery-hole
//! [`error_kind`](CstProfile::error_kind), and the gap-tile
//! [`gap_kind`](CstProfile::gap_kind) — so a dialect states them in one place and hands the
//! same value to every sink it builds.

use super::event::TOMBSTONE;

/// A named predicate over the dialect's `u16` kind space: the authority that decides which
/// raw kinds this dialect can actually name.
///
/// Materialization runs it over the root kind and over every node and token kind in the
/// recorded stream, so a kind outside the dialect's own space is refused with a typed error
/// instead of reaching `rowan` — where the only remaining wall is a `kind_from_raw` panic at
/// query time, arbitrarily far from the parse that produced it.
///
/// # Why a plain `fn(u16) -> bool` and not a `Lang` bound
///
/// The obvious-looking alternative is to hang the kind authority on the [`Dialect`] brand, or
/// to require `Lang: rowan::Language` and let `kind_from_raw` be the judge. Neither works, and
/// the reason is structural rather than stylistic:
///
/// - a dialect's brand is *not* a kind authority. [`Dialect::Lang`] is a **grammar brand** —
///   declared `type Lang: ?Sized`, a marker whose whole job is to keep two grammars'
///   vocabularies from unifying. A kind authority would instead have to be a
///   `rowan::Language`, a strictly stronger and differently-shaped requirement, so the brand
///   parameter cannot carry the predicate;
/// - and `rowan::Language` could not produce the error even if it were required here:
///   `kind_from_raw` has no fallible form. It returns a kind or panics, so a `Lang` parameter
///   alone can never yield a *typed* rejection — which is exactly what this wall is for.
///
/// Hence the predicate travels as data. [`accept_all`](Self::accept_all) is the written-down
/// opt-out for a caller who really does accept every kind.
///
/// [`Dialect`]: crate::Dialect
/// [`Dialect::Lang`]: crate::Dialect::Lang
#[derive(Clone, Copy)]
pub struct KindValidator(fn(u16) -> bool);

impl core::fmt::Debug for KindValidator {
  /// Renders no function pointer: a derived `Debug` would print a code address, which changes
  /// from build to build and would make every `Debug` render of a [`CstProfile`]-carrying type
  /// unstable.
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("KindValidator(..)")
  }
}

impl KindValidator {
  /// Wraps a dialect predicate: `true` for every kind the dialect can name, `false`
  /// otherwise.
  #[inline]
  pub const fn new(f: fn(u16) -> bool) -> Self {
    Self(f)
  }

  /// The explicit opt-out: every kind is admitted (except the reserved
  /// [`TOMBSTONE`] band, which no validator can re-admit).
  ///
  /// Written out rather than defaulted, so a caller who accepts the whole `u16` space has
  /// said so in the source instead of inheriting it by omission.
  #[inline]
  pub const fn accept_all() -> Self {
    Self(|_| true)
  }

  /// The effective predicate: the dialect's own answer, with the [`TOMBSTONE`] floor ANDed in
  /// unconditionally.
  ///
  /// The floor is not the dialect's to lift — the reserved value is what marks an inert
  /// tombstone slot in the event log — so [`accept_all`](Self::accept_all) does not re-admit
  /// it either.
  #[inline]
  pub(crate) fn admits(&self, kind: u16) -> bool {
    kind != TOMBSTONE && (self.0)(kind)
  }
}

/// The dialect's CST profile: everything a recording sink needs to know about the kind space,
/// stated once per dialect and reused at every construction.
///
/// The four facts travel together because they are one decision: which `u16`s this dialect
/// names ([`validator`](Self::validator)), how its tokens become them
/// ([`mapper`](Self::mapper)), and the two the sink synthesizes on its own behalf — the node
/// kind wrapped around a recovery hole ([`error_kind`](Self::error_kind)) and the token kind
/// tiled over source bytes no committed token covers ([`gap_kind`](Self::gap_kind)). Both
/// synthesized kinds are checked against the validator at construction, so a profile cannot
/// describe a sink that would refuse its own output.
///
/// `Debug`, `Clone` and `Copy` are implemented without a bound on `T`: the token type appears
/// only inside `fn(&T) -> u16`, which is `Copy` for every `T`, so a dialect whose token type
/// is neither `Copy` nor `Debug` still gets a freely reusable profile value.
#[non_exhaustive]
pub struct CstProfile<T> {
  /// Maps each committed token into the dialect's unified kind space.
  mapper: fn(&T) -> u16,
  /// Decides which raw kinds this dialect can name.
  validator: KindValidator,
  /// The node kind wrapped around a recovery hole's skipped tokens.
  error_kind: u16,
  /// The token kind tiled over source bytes no committed token covers.
  gap_kind: u16,
}

impl<T> core::fmt::Debug for CstProfile<T> {
  /// Renders the two kind values only. The mapper and the validator are function pointers
  /// whose derived render is a code address — the same reason the CST `Sink`'s own
  /// `Debug` omits its mapper.
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("CstProfile")
      .field("error_kind", &self.error_kind)
      .field("gap_kind", &self.gap_kind)
      .finish_non_exhaustive()
  }
}

impl<T> Clone for CstProfile<T> {
  #[inline]
  fn clone(&self) -> Self {
    *self
  }
}

impl<T> Copy for CstProfile<T> {}

impl<T> CstProfile<T> {
  /// States the dialect's kind space: all four facts, none defaulted.
  ///
  /// # Panics
  ///
  /// In **every** build, if either synthesized kind is unusable — the reserved
  /// [`TOMBSTONE`] value, or a kind `validator` itself rejects. Both are construction-time
  /// contradictions (the sink would emit a kind its own materialization refuses), so they are
  /// refused where the mistake is, not one parse later. The message names the offending
  /// field.
  #[inline]
  pub fn new(
    mapper: fn(&T) -> u16,
    validator: KindValidator,
    error_kind: u16,
    gap_kind: u16,
  ) -> Self {
    assert!(
      error_kind != TOMBSTONE,
      "CstProfile.error_kind is the reserved TOMBSTONE"
    );
    assert!(
      gap_kind != TOMBSTONE,
      "CstProfile.gap_kind is the reserved TOMBSTONE"
    );
    assert!(
      validator.admits(error_kind),
      "CstProfile.error_kind is rejected by this profile's own validator"
    );
    assert!(
      validator.admits(gap_kind),
      "CstProfile.gap_kind is rejected by this profile's own validator"
    );
    Self {
      mapper,
      validator,
      error_kind,
      gap_kind,
    }
  }

  /// The node kind wrapped around a recovery hole's skipped tokens.
  #[inline]
  pub const fn error_kind(&self) -> u16 {
    self.error_kind
  }

  /// The token kind tiled over source bytes no committed token covers.
  #[inline]
  pub const fn gap_kind(&self) -> u16 {
    self.gap_kind
  }

  /// The dialect's kind predicate.
  #[inline]
  pub const fn validator(&self) -> KindValidator {
    self.validator
  }

  /// The dialect's token mapper.
  #[inline]
  pub const fn mapper(&self) -> fn(&T) -> u16 {
    self.mapper
  }
}
