use super::*;
use std::format;

// --- LimitExceeded tests ---

fn make_token_error() -> LimitExceeded {
  let mut limiter = TokenLimiter::with_limitation(1);
  limiter.increase();
  limiter.increase();
  let token_err = limiter.check().unwrap_err();
  LimitExceeded::from(token_err)
}

fn make_recursion_error() -> LimitExceeded {
  let mut limiter = RecursionLimiter::with_limitation(1);
  limiter.increase();
  limiter.increase();
  let rec_err = limiter.check().unwrap_err();
  LimitExceeded::from(rec_err)
}

#[test]
fn limit_exceeded_is_token() {
  let err = make_token_error();
  assert!(err.is_token());
  assert!(!err.is_recursion());
}

#[test]
fn limit_exceeded_is_recursion() {
  let err = make_recursion_error();
  assert!(!err.is_token());
  assert!(err.is_recursion());
}

#[test]
fn limit_exceeded_unwrap_token() {
  let err = make_token_error();
  let inner = err.unwrap_token_ref();
  assert_eq!(inner.limitation(), 1);
}

#[test]
fn limit_exceeded_unwrap_recursion() {
  let err = make_recursion_error();
  let inner = err.unwrap_recursion_ref();
  assert_eq!(inner.limitation(), 1);
}

#[test]
fn limit_exceeded_try_unwrap_token() {
  let err = make_token_error();
  assert!(err.try_unwrap_token_ref().is_ok());

  let err = make_recursion_error();
  assert!(err.try_unwrap_token_ref().is_err());
}

#[test]
fn limit_exceeded_try_unwrap_recursion() {
  let err = make_recursion_error();
  assert!(err.try_unwrap_recursion_ref().is_ok());

  let err = make_token_error();
  assert!(err.try_unwrap_recursion_ref().is_err());
}

#[test]
fn limit_exceeded_from_token() {
  let err = make_token_error();
  assert!(err.is_token());
}

#[test]
fn limit_exceeded_from_recursion() {
  let err = make_recursion_error();
  assert!(err.is_recursion());
}

#[test]
fn limit_exceeded_display() {
  let err = make_token_error();
  let msg = format!("{}", err);
  assert!(!msg.is_empty());

  let err = make_recursion_error();
  let msg = format!("{}", err);
  assert!(!msg.is_empty());
}

// --- Limiter tests ---

#[test]
fn limiter_default() {
  let limiter = Limiter::default();
  assert_eq!(limiter.token().tokens(), 0);
  assert_eq!(limiter.recursion().depth(), 0);
}

#[test]
fn limiter_with_token_tracker() {
  let limiter = Limiter::with_token_tracker(TokenLimiter::with_limitation(100));
  assert_eq!(limiter.token().limitation(), 100);
  assert_eq!(limiter.recursion().limitation(), 500); // default
}

#[test]
fn limiter_with_recursion_tracker() {
  let limiter = Limiter::with_recursion_tracker(RecursionLimiter::with_limitation(50));
  assert_eq!(limiter.recursion().limitation(), 50);
  assert_eq!(limiter.token().limitation(), usize::MAX); // default
}

#[test]
fn limiter_token_mut() {
  let mut limiter = Limiter::new();
  limiter.token_mut().increase();
  assert_eq!(limiter.token().tokens(), 1);
}

#[test]
fn limiter_recursion_mut() {
  let mut limiter = Limiter::new();
  limiter.recursion_mut().increase();
  assert_eq!(limiter.recursion().depth(), 1);
}

#[test]
fn limiter_check_recursion_exceeded() {
  let mut limiter = Limiter::with_recursion_tracker(RecursionLimiter::with_limitation(2));
  limiter.increase_recursion();
  limiter.increase_recursion();
  assert!(limiter.check().is_ok());
  limiter.increase_recursion();
  let err = limiter.check().unwrap_err();
  assert!(err.is_recursion());
}

#[test]
fn limiter_check_token_exceeded() {
  let mut limiter = Limiter::with_token_tracker(TokenLimiter::with_limitation(2));
  limiter.increase_token();
  limiter.increase_token();
  assert!(limiter.check().is_ok());
  limiter.increase_token();
  let err = limiter.check().unwrap_err();
  assert!(err.is_token());
}

#[test]
fn limiter_state_check() {
  let limiter = Limiter::new();
  assert!(State::check(&limiter).is_ok());
}

#[test]
fn limiter_recursion_tracker_trait() {
  let mut limiter = Limiter::new();
  RecursionTracker::increase(&mut limiter);
  assert_eq!(limiter.recursion().depth(), 1);
  RecursionTracker::decrease(&mut limiter);
  assert_eq!(limiter.recursion().depth(), 0);
  assert!(RecursionTracker::check(&limiter).is_ok());
}

#[test]
fn limiter_token_tracker_trait() {
  let mut limiter = Limiter::new();
  TokenTracker::increase(&mut limiter);
  assert_eq!(limiter.token().tokens(), 1);
  assert!(TokenTracker::check(&limiter).is_ok());
}

// --- Tracker trait tests ---

#[test]
fn tracker_increase_token_and_decrease_recursion() {
  let mut limiter = Limiter::new();
  limiter.increase_recursion();
  assert_eq!(limiter.recursion().depth(), 1);
  Tracker::increase_token_and_decrease_recursion(&mut limiter);
  assert_eq!(limiter.token().tokens(), 1);
  assert_eq!(limiter.recursion().depth(), 0);
}

#[test]
fn tracker_increase_token_and_decrease_recursion_and_check() {
  let mut limiter = Limiter::new();
  limiter.increase_recursion();
  assert!(Tracker::increase_token_and_decrease_recursion_and_check(&mut limiter).is_ok());
  assert_eq!(limiter.token().tokens(), 1);
  assert_eq!(limiter.recursion().depth(), 0);
}

#[test]
fn tracker_increase_token_and_check() {
  let mut limiter = Limiter::new();
  assert!(Tracker::increase_token_and_check(&mut limiter).is_ok());
  assert_eq!(limiter.token().tokens(), 1);
}

#[test]
fn tracker_increase_both() {
  let mut limiter = Limiter::new();
  Tracker::increase_both(&mut limiter);
  assert_eq!(limiter.token().tokens(), 1);
  assert_eq!(limiter.recursion().depth(), 1);
}

#[test]
fn tracker_increase_both_and_check() {
  let mut limiter = Limiter::new();
  assert!(Tracker::increase_both_and_check(&mut limiter).is_ok());
  assert_eq!(limiter.token().tokens(), 1);
  assert_eq!(limiter.recursion().depth(), 1);
}

#[test]
fn limiter_recursion_tracker_check_exceeded() {
  let mut limiter = Limiter::with_recursion_tracker(RecursionLimiter::with_limitation(1));
  RecursionTracker::increase(&mut limiter);
  RecursionTracker::increase(&mut limiter);
  let err = RecursionTracker::check(&limiter).unwrap_err();
  assert!(err.is_recursion());
}

#[test]
fn limiter_token_tracker_check_exceeded() {
  let mut limiter = Limiter::with_token_tracker(TokenLimiter::with_limitation(1));
  TokenTracker::increase(&mut limiter);
  TokenTracker::increase(&mut limiter);
  let err = TokenTracker::check(&limiter).unwrap_err();
  assert!(err.is_token());
}
