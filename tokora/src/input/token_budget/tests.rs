//! The budget arithmetic: the gate the driver asks before it works, and the charge it takes after.

use super::TokenBudget;

#[test]
fn an_unlimited_budget_never_refuses() {
  let mut budget = TokenBudget::unlimited();
  for _ in 0..1_000 {
    assert!(!budget.is_exhausted());
    budget.spend();
  }
  assert_eq!(budget.spent(), 1_000);
  assert!(!budget.is_exhausted());
}

#[test]
fn a_bounded_budget_charges_exactly_its_limitation() {
  let mut budget = TokenBudget::with_limitation(3);
  for _ in 0..3 {
    assert!(!budget.is_exhausted());
    budget.spend();
  }
  assert!(budget.is_exhausted());
  // The gate is a pure read, so asking it again reads the same — a refusal is not a charge, and
  // the driver re-asks it on every entry precisely because that is free and idempotent.
  assert!(budget.is_exhausted());
  assert_eq!(budget.spent(), 3);
  assert_eq!(budget.limitation(), 3);
}

#[test]
fn a_zero_budget_refuses_the_first_item() {
  let budget = TokenBudget::with_limitation(0);
  assert!(budget.is_exhausted());
  assert_eq!(budget.spent(), 0);
}

/// **Plant.** The **unlimited sentinel is not a ceiling**, and the gate has to say so out of the
/// `max` value rather than out of the comparison alone.
///
/// `unlimited()` is `max == usize::MAX`, and the gate was `spent >= max`. One charge from
/// `usize::MAX - 1` therefore lands `spent == max`, and from that point the budget that promised to
/// refuse nothing refuses **everything**, terminally.
///
/// # Why the seed and not a drain
///
/// The distance is `usize::MAX` produce-events, and this PR charges a **re-lex** as a
/// produce-event, so it is a distance one long-lived `Input` over a speculating grammar can cover —
/// not a four-billion-token document. At a 64-bit `usize` the seed is unreachable and the arithmetic
/// is the whole claim; at a **32-bit** `usize` the seeded value is `4_294_967_294`, which is why
/// this cell is also run under `--target wasm32-wasip1` (32-bit `usize`, executed by wasmtime).
/// [`the_sentinel_distance_is_a_reachable_count_at_thirty_two_bit_width`] pins the width the run
/// actually had.
#[test]
fn the_unlimited_sentinel_never_refuses_at_the_top_of_the_counter() {
  let mut budget = TokenBudget::unlimited();
  // The last state before the sentinel, reached by charging rather than by writing `spent`
  // directly, so `spend`'s own precondition is exercised on the way in.
  budget.spent = usize::MAX - 1;

  assert!(!budget.is_exhausted());
  budget.spend();

  assert_eq!(budget.spent(), usize::MAX);
  assert!(
    !budget.is_exhausted(),
    "`unlimited()` promised to refuse nothing; `spent >= usize::MAX` made it refuse everything",
  );
}

/// **Plant.** And the charge at the sentinel must not wrap. With the gate corrected, `spend` is
/// now reached *with* `spent == usize::MAX` — the one state the old precondition ruled out — so the
/// increment has to saturate. A wrap would put `spent` back to `0` and hand an unbounded budget a
/// fresh, silently-restarted counter.
///
/// Falsifying output before the repair: a `debug_assert` panic in a debug build (`spent < max` is
/// false at the sentinel) and a wrap to `0` in a release one — the same defect, reported in one
/// build and silent in the other.
#[test]
fn the_charge_at_the_sentinel_saturates_rather_than_wrapping() {
  let mut budget = TokenBudget::unlimited();
  budget.spent = usize::MAX;

  budget.spend();
  assert_eq!(budget.spent(), usize::MAX, "saturated, not wrapped to zero");
  assert!(!budget.is_exhausted());
}

/// The width the two cells above were actually run at, asserted rather than assumed.
///
/// The sentinel defect is arithmetic at any width and *reachable* only at 32 bits, and this
/// program has twice shipped a finding that existed only there. There is no 32-bit test leg, so the
/// evidence is this cell plus the target it ran under: at `wasm32-wasip1` the branch below is the
/// live one and the distance is a nine-digit number.
#[test]
fn the_sentinel_distance_is_a_reachable_count_at_thirty_two_bit_width() {
  #[cfg(target_pointer_width = "32")]
  assert_eq!(
    usize::MAX,
    4_294_967_295,
    "the 32-bit leg: the distance to the sentinel is 4.29e9 produce-events, re-lexes included",
  );
  #[cfg(target_pointer_width = "64")]
  assert_eq!(usize::MAX, 18_446_744_073_709_551_615);
  // `with_limitation(usize::MAX)` is the sentinel by value, so it must be the sentinel by
  // behaviour: nothing else would explain two budgets that compare equal and refuse differently.
  assert_eq!(
    TokenBudget::with_limitation(usize::MAX),
    TokenBudget::unlimited()
  );
}
