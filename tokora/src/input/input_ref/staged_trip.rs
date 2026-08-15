//! The **sealed carrier** of a terminal scanner stop: [`StagedTrip`].
//!
//! # Why this is a module and not a bare `enum` beside its two users
//!
//! The split between staging a stop and publishing it exists so that the one fallible step — the
//! `Offset::Ord` clamp, which is *caller* code — is necessarily complete before the infallible
//! writes are callable. Its docs claimed that this held **by construction**. It did not: a bare
//! `enum StagedTrip { Keep, Lower(O) }` declared in `mod.rs` has variants visible throughout
//! `input_ref` and every descendant of it, which is precisely the set of modules that can also
//! reach `publish_scanner_trip`. Any of them could have written `StagedTrip::Lower(b)` directly
//! and published a boundary that had never been compared against the latch — no forged caller
//! existed, but nothing said one could not compile.
//!
//! A child module is what installs the claim. The representation
//! ([`Clamped`]) is private to this file, and [`StagedTrip`] is a newtype whose single field is
//! private, so the parent module can *name* the type and *move* it and can do nothing else with
//! it. [`StagedTrip::stage`] is the only constructor in the language, and it performs the clamp;
//! [`StagedTrip::install`] is the only consumer, and it performs no comparison at all.
//!
//! # What is compiler-enforced here, and what is not
//!
//! Enforced: **a published stop was clamped.** There is no expression outside this file that
//! produces a `StagedTrip`, so there is no path to `publish_scanner_trip` that skipped the
//! comparison.
//!
//! *Not* enforced, and named rather than folded in: this seals the **carrier**, not the field.
//! `InputRef::poison_boundary` is `pub(super)`, so a future edit could still write the latch
//! without going through a stop at all. That residual is the census's (`TRIP_CENSUS` in
//! `census_tests.rs`), not the type system's.

/// A terminal scanner stop whose one fallible step is already taken: the boundary write, decided
/// and not yet performed.
///
/// The carrier between `InputRef::stage_scanner_trip` and `InputRef::publish_scanner_trip`, and
/// the reason the split is **structural** rather than an ordering someone has to maintain.
/// Publishing takes one of these by value, only [`stage`](Self::stage) makes one, and staging is
/// the only step of a stop that runs caller code — so the comparison is necessarily complete
/// before the publication is callable, and there is no spelling of "publish, then compare".
///
/// It is not `Option<O>`: publishing *returns* an `Option<O>` — the boundary it displaced — and
/// two adjacent options with opposite meanings is exactly the confusion this window was lost to
/// once already. The two are kept apart by a named representation that the publication reads and
/// nobody else can write.
pub(super) struct StagedTrip<O>(Clamped<O>);

/// The representation, private to this module. Naming it outside is `error[E0603]`.
enum Clamped<O> {
  /// An already-latched boundary is at least as poisoned; the publication is the count alone.
  Keep,
  /// This stop establishes the frontier, or lowers it; the publication installs it.
  Lower(O),
}

impl<O: Ord> StagedTrip<O> {
  /// Decides what the boundary write will be, and performs no write at all.
  ///
  /// A trip can only maintain or increase poison, so the frontier it records is the more-poisoned
  /// (smaller) of `existing` and `boundary`. That clamp is an `Offset::Ord` comparison — **caller
  /// code** — and it is the only step of a stop that can unwind. Taking it here, off a shared
  /// borrow of the latch, is what lets both callers run it *before* the decision that obliges
  /// them to publish, so nothing consumer-controlled is left between the decision and the write.
  #[inline(always)]
  pub(super) fn stage(existing: Option<&O>, boundary: O) -> Self {
    match existing {
      Some(existing) if *existing <= boundary => Self(Clamped::Keep),
      _ => Self(Clamped::Lower(boundary)),
    }
  }

  /// Installs the staged frontier into `latch`, and hands back the boundary it displaced.
  ///
  /// **Nothing in this body can unwind.** The comparison is already decided — that is what the
  /// type says — and what is left is at most one `Option::replace`, which *is* `mem::replace`: a
  /// move, not an assignment. `*latch = Some(..)` would drop the displaced value **before**
  /// installing the new one, putting caller `Drop` code inside the publication; the displaced
  /// value is returned instead, and `#[must_use]` makes the caller name it.
  #[must_use = "drop the displaced boundary AFTER the publication: dropping it here puts caller \
                code between the two writes that record a terminal stop"]
  #[inline(always)]
  pub(super) fn install(self, latch: &mut Option<O>) -> Option<O> {
    match self.0 {
      Clamped::Keep => None,
      Clamped::Lower(boundary) => latch.replace(boundary),
    }
  }
}
