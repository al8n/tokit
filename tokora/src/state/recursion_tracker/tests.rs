use super::*;

// ══════════════════════════════════════════════════════════════════════════════
// The shipped numbers, as LITERALS
// ══════════════════════════════════════════════════════════════════════════════
//
// **These are the only cells in the tree that write the budgets out.** Everywhere else — every
// depth in `pratt_limit.rs`, `pratt_recovery.rs`, `pratt_txn_retention.rs` and the benches — reads
// `PARSE_DEFAULT_DEPTH` off the library, which is right for those cells and would be wrong as the
// *whole* of the coverage: a suite that derives its expectation from the subject moves its own
// boundary when the subject moves, so a wrong constant leaves it green. That is the shape this
// file closes, and closing it needs exactly one independent statement of each number.
//
// A cell per matrix cell, keyed the way `policy` is keyed. `debug_assertions` rather than one
// unconditional literal because the two profiles are separate policy decisions that happen to
// agree today: writing one literal for both would silently re-pin the release cell to the debug
// figure the day somebody raises it, which is the drift this comment block exists to prevent.
//
// They are `#[test]`s and not `const` assertions on purpose — the derivations already have those,
// and a second copy of the derivation would red for the same reason at the same time. What a
// literal adds is a statement made from *outside* the derivation, so a derivation that is wrong in
// its own terms still has something to disagree with.

/// The unsegmented default, debug build.
#[cfg(debug_assertions)]
#[test]
fn the_parse_default_depth_is_sixteen() {
  assert_eq!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH,
    16,
    "the shipped debug parse default is 16 — the deepest power of two leaving more than 3x under \
     the 51 at which a measured consumer grammar aborts on a 2 MiB thread. If this moved on \
     purpose, the derivation in `policy` moved with it and this literal is the second half of \
     that edit."
  );
}

/// The unsegmented default, release build. The same 16, and separately stated because it is a
/// separate decision: a floor equal to the debug cell until the release bisection exists.
#[cfg(not(debug_assertions))]
#[test]
fn the_parse_default_depth_is_sixteen_in_release() {
  assert_eq!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH,
    16,
    "the shipped release parse default is a FLOOR equal to the debug one, not a derivation — see \
     `policy::PARSE_DEFAULT_RELEASE`. Raising it means running the five-axis consumer bisection \
     against a release build, not extrapolating from debug's ratio."
  );
}

/// The segmented-Pratt budget, debug build.
#[cfg(all(feature = "stacker", debug_assertions))]
#[test]
fn the_segmented_pratt_depth_is_1024() {
  assert_eq!(
    RecursionLimiter::SEGMENTED_PRATT_DEPTH,
    1024,
    "the shipped debug segmented budget is 1024 — 64 MiB of stack segments at the binding ~41 KiB \
     per level. 512 is the value the old range-shaped guard accepted while the docs said 1024."
  );
  // The *ordering* claim — that the segmented figure is the larger of the two, or there was no
  // reason to name it apart — is settled at compile time beside the derivations, where clippy
  // rightly insists a comparison of two constants belongs. Restating it here would be a second
  // copy that reds at the same moment for the same reason.
}

/// The segmented-Pratt budget, release build. Same floor argument as the unsegmented pair.
#[cfg(all(feature = "stacker", not(debug_assertions)))]
#[test]
fn the_segmented_pratt_depth_is_1024_in_release() {
  assert_eq!(
    RecursionLimiter::SEGMENTED_PRATT_DEPTH,
    1024,
    "the shipped release segmented budget is a FLOOR equal to the debug one — see \
     `policy::SEGMENTED_PRATT_RELEASE`"
  );
}

/// **The constant that must NOT move with a feature**, stated as its own claim.
///
/// For one revision `PARSE_DEFAULT_DEPTH` was `if cfg!(feature = "stacker") { 1024 } else { 16 }`,
/// and the two literal cells above cannot catch that on their own: each one runs in only one
/// configuration, so each would simply see whichever number its own leg was given. This is the
/// cross-configuration statement — the same 16 in a `stacker` build as in a plain one — and it is
/// the runtime twin of the const assertion that prices the default against a 2 MiB thread.
///
/// The reason it holds is not that 1024 is too big for the *Pratt* path. It is that this constant
/// seeds every `InputContext`, and that cell is read by hand-written descent through
/// `InputRef::descend` / `descending` from a consumer's own productions — frames the segmented
/// prologue never touches, since its only two callers are tokora's own Pratt engines.
#[cfg(feature = "stacker")]
#[test]
fn enabling_stacker_does_not_move_the_shared_default() {
  assert_eq!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH,
    16,
    "the `stacker` feature has moved the SHARED recursion budget. It segments tokora's two Pratt \
     frame prologues and nothing else, so it justifies no change to the cell every unsegmented \
     `descend` reads. The figure it does justify is SEGMENTED_PRATT_DEPTH, which a caller opts \
     into."
  );
}

// ══════════════════════════════════════════════════════════════════════════════
// The number the derivation gives when it is pointed at the door the budget is spent on
// ══════════════════════════════════════════════════════════════════════════════
//
// Same job as the four cells above and for the same reason: `policy` derives these from
// `measured`, so a literal is the only statement made from OUTSIDE the derivation. It matters more
// here than for a shipped cell, because nothing else in the tree reads either constant — the
// compile-time relations beside them pin what the numbers must be *relative to* the measured rows,
// and these two pin what they are.
//
// Unconditional rather than keyed on `debug_assertions`: both are derived from their own measured
// row in every build, and neither is selected by the profile flag.

/// What repointing [`PARSE_DEFAULT_DEBUG`](super::policy::PARSE_DEFAULT_DEBUG) at the lossless row
/// produces — and it is not what the issue that found the mis-pointing inferred.
#[test]
fn the_repointed_debug_default_is_128() {
  assert_eq!(
    super::policy::PARSE_DEFAULT_DEBUG_ON_THE_LOSSLESS_ROW,
    128,
    "the derivation over the lossless row no longer produces 128. Either the row was re-measured \
     — in which case this literal is the second half of that edit — or MIN_HEADROOM moved, in \
     which case the shipped default moved too and the four cells above should have reddened first."
  );
}

/// The release twin, and the first release figure in this file that is a derivation rather than a
/// floor.
#[test]
fn the_repointed_release_default_is_1024() {
  assert_eq!(
    super::policy::PARSE_DEFAULT_RELEASE_ON_THE_LOSSLESS_ROW,
    1024,
    "the derivation over the release lossless row no longer produces 1024"
  );
}

#[test]
fn recursion_increase_saturates_at_max() {
  // At the ceiling `increase` must saturate rather than overflow-panic,
  // keeping symmetry with the saturating `decrease`.
  let mut r = RecursionLimiter {
    max: usize::MAX,
    current: usize::MAX,
  };
  r.increase();
  assert_eq!(r.depth(), usize::MAX);
}
