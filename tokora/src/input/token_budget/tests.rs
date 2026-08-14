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
