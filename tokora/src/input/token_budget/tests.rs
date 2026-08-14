//! The budget arithmetic: what a charge costs, and what a refusal does not.

use super::TokenBudget;

#[test]
fn an_unlimited_budget_never_refuses() {
  let mut budget = TokenBudget::unlimited();
  for _ in 0..1_000 {
    assert!(budget.charge());
  }
  assert_eq!(budget.spent(), 1_000);
  assert!(!budget.is_exhausted());
}

#[test]
fn a_bounded_budget_charges_exactly_its_limitation() {
  let mut budget = TokenBudget::with_limitation(3);
  assert!(budget.charge());
  assert!(budget.charge());
  assert!(budget.charge());
  assert!(budget.is_exhausted());
  // Refusing is not a charge, and refusing twice reads the same as refusing once.
  assert!(!budget.charge());
  assert!(!budget.charge());
  assert_eq!(budget.spent(), 3);
  assert_eq!(budget.limitation(), 3);
}

#[test]
fn a_zero_budget_refuses_the_first_item() {
  let mut budget = TokenBudget::with_limitation(0);
  assert!(budget.is_exhausted());
  assert!(!budget.charge());
  assert_eq!(budget.spent(), 0);
}
