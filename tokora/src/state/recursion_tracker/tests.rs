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

/// The unsegmented default, debug build. **32**, from the two-tier rule — see `policy::two_tier`.
#[cfg(debug_assertions)]
#[test]
fn the_parse_default_depth_is_thirty_two() {
  assert_eq!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH,
    32,
    "the shipped debug parse default is 32 — the deepest power of two leaving MIN_HEADROOM under \
     the 125 at which tokora's own Pratt driver aborts on a 2 MiB thread AND fitting that thread \
     at the heaviest modelled per-level cost. If this moved on purpose, the derivation in \
     `policy` moved with it and this literal is the second half of that edit."
  );
}

/// The unsegmented default, release build. **256, not 32**, and separately stated because it is a
/// separate derivation — the release bisection the floor was waiting for now exists.
///
/// Worth reading beside `the_segmented_pratt_depth_is_1024_in_release`: for one revision both the
/// release parse default and the release segmented budget would have read **1024**, and two
/// different constants asserting one literal in one profile is a layer that has stopped
/// discriminating. They differ, and `SEGMENTED_PRATT_RELEASE > PARSE_DEFAULT_RELEASE` compiles it.
#[cfg(not(debug_assertions))]
#[test]
fn the_parse_default_depth_is_two_hundred_fifty_six_in_release() {
  assert_eq!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH,
    256,
    "the shipped release parse default is 256 — a DERIVATION now rather than a floor, fixed by the \
     fit tier at the heaviest modelled RELEASE per-level cost. Note it is not 1024: that is what \
     the headroom tier alone would have allowed, and it collides with SEGMENTED_PRATT_RELEASE."
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
/// cross-configuration statement — the same number in a `stacker` build as in a plain one — and it
/// is the runtime twin of the const assertion that prices the default against a 2 MiB thread.
///
/// The reason it holds is not that 1024 is too big for the *Pratt* path. It is that this constant
/// seeds every `InputContext`, and that cell is read by hand-written descent through
/// `InputRef::descend` / `descending` from a consumer's own productions — frames the segmented
/// prologue never touches, since its only two callers are tokora's own Pratt engines.
///
/// # Two axes now, and this cell holds one of them fixed
///
/// It carried a bare literal for as long as both profile arms held one number, and that was enough:
/// the only axis that could move the value was the feature. The arms differ now, so a bare literal
/// makes this a statement about the *profile* as well — and a wrong one, which is how it reds in a
/// release build while the feature it is about is behaving perfectly. **Measured: this cell was the
/// only failure in an otherwise green `--release` run of the whole suite**, and no leg of the
/// standard gate matrix runs one.
///
/// So the expected value is selected the way the constant is, and what is left is the claim the
/// cell is actually for: whatever this profile's number is, `stacker` does not change it.
#[cfg(feature = "stacker")]
#[test]
fn enabling_stacker_does_not_move_the_shared_default() {
  // Literals, not `policy::PARSE_DEFAULT`: recomputing the expectation from the derivation is what
  // would make this a tautology. These are the same two numbers the arm cells above state, and
  // they are stated again here because this cell runs in a configuration neither of them can see.
  let expected = if cfg!(debug_assertions) { 32 } else { 256 };
  assert_eq!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH,
    expected,
    "the `stacker` feature has moved the SHARED recursion budget. It segments tokora's two Pratt \
     frame prologues and nothing else, so it justifies no change to the cell every unsegmented \
     `descend` reads. The figure it does justify is SEGMENTED_PRATT_DEPTH, which a caller opts \
     into."
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
