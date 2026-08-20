/// Tests for `state::{token_tracker, recursion_tracker, tracker}`.
use tokora::state::{
  recursion_tracker::{RecursionLimiter, RecursionTracker, RecursionTrackerExt},
  token_tracker::{TokenLimiter, TokenTracker},
  tracker::{LimitExceeded, Limiter, Tracker, TrackerExt},
};

// ── TokenLimiter ────────────────────────────────────────────────────────────

#[test]
fn token_limiter_new_defaults() {
  let t = TokenLimiter::new();
  assert_eq!(t.tokens(), 0);
  assert_eq!(t.limitation(), usize::MAX);
}

#[test]
fn token_limiter_default_equals_new() {
  let a = TokenLimiter::default();
  let b = TokenLimiter::new();
  assert_eq!(a, b);
}

#[test]
fn token_limiter_with_limitation() {
  let t = TokenLimiter::with_limitation(1000);
  assert_eq!(t.limitation(), 1000);
  assert_eq!(t.tokens(), 0);
}

#[test]
fn token_limiter_increase() {
  let mut t = TokenLimiter::new();
  t.increase();
  t.increase();
  assert_eq!(t.tokens(), 2);
}

#[test]
fn token_limiter_increase_token_alias() {
  let mut t = TokenLimiter::new();
  t.increase_token();
  assert_eq!(t.tokens(), 1);
}

#[test]
fn token_limiter_check_ok() {
  let mut t = TokenLimiter::with_limitation(5);
  for _ in 0..5 {
    t.increase();
  }
  assert!(t.check().is_ok());
}

#[test]
fn token_limiter_check_exceeded() {
  let mut t = TokenLimiter::with_limitation(3);
  for _ in 0..4 {
    t.increase();
  }
  let err = t.check().unwrap_err();
  assert_eq!(err.tokens(), 4);
  assert_eq!(err.limitation(), 3);
}

#[test]
fn token_limit_exceeded_display() {
  let mut t = TokenLimiter::with_limitation(2);
  t.increase();
  t.increase();
  t.increase();
  let err = t.check().unwrap_err();
  let s = format!("{err}");
  assert!(s.contains("token limit exceeded"));
}

#[test]
fn token_tracker_trait_increase_and_check() {
  let mut t = TokenLimiter::with_limitation(2);
  <TokenLimiter as TokenTracker>::increase(&mut t);
  assert_eq!(t.tokens(), 1);
  assert!(<TokenLimiter as TokenTracker>::check(&t).is_ok());
}

// ── RecursionLimiter ────────────────────────────────────────────────────────

#[test]
fn recursion_limiter_new_defaults() {
  // `RecursionLimiter::new`'s own default is 500 — the type's general-purpose, no-native-stack-
  // assumption ceiling (suitable for a lexer's `Extras` nesting tracker, among other uses). It
  // is deliberately NOT 64, tokora's own Pratt-parser stack-safety figure: see
  // `recursion_limiter_new_is_not_governed_by_the_parser_default` for the cell that pins the
  // two apart.
  let r = RecursionLimiter::new();
  assert_eq!(r.depth(), 0);
  assert_eq!(r.limitation(), 500);
}

#[test]
fn recursion_limiter_new_is_not_governed_by_the_parser_default() {
  // `RecursionLimiter::new` carries the type's own general-purpose depth. Only `ParserContext`'s
  // and the input layer's *internal* construction requests the stack-safety-derived one
  // (`RecursionLimiter::PARSE_DEFAULT_DEPTH`, now public and feature-dependent) — bare
  // `RecursionLimiter::new` never
  // does, because it is also reachable as a lexer-side `State`/`Extras` nesting tracker with no
  // pratt parser anywhere near it.
  //
  // `Limiter::new` composes a `RecursionLimiter` too, and is reachable the very same way
  // (`Limiter` also implements `State` and is documented as usable directly as a lexer's
  // `Extras`), so it must carry the very same general-purpose depth rather than borrow the
  // parser's number either.
  assert_eq!(RecursionLimiter::new().limitation(), 500);
  assert_eq!(
    Limiter::new().recursion().limitation(),
    RecursionLimiter::new().limitation(),
    "Limiter::new's recursion component must not be silently governed by the parser's \
     stack-safety number — Limiter is reachable as a lexer-side tracker just like bare \
     RecursionLimiter"
  );
}

#[test]
fn recursion_limiter_default_equals_new() {
  let a = RecursionLimiter::default();
  let b = RecursionLimiter::new();
  assert_eq!(a, b);
}

#[test]
fn recursion_limiter_with_limitation() {
  let r = RecursionLimiter::with_limitation(100);
  assert_eq!(r.limitation(), 100);
}

#[test]
fn recursion_increase_and_decrease() {
  let mut r = RecursionLimiter::new();
  r.increase();
  r.increase();
  assert_eq!(r.depth(), 2);
  r.decrease();
  assert_eq!(r.depth(), 1);
  r.decrease();
  assert_eq!(r.depth(), 0);
}

#[test]
fn recursion_decrease_saturates_at_zero() {
  let mut r = RecursionLimiter::new();
  r.decrease(); // should not underflow
  assert_eq!(r.depth(), 0);
}

#[test]
fn recursion_increase_aliases() {
  let mut r = RecursionLimiter::new();
  r.increase_recursion();
  assert_eq!(r.depth(), 1);
  r.decrease_recursion();
  assert_eq!(r.depth(), 0);
}

#[test]
fn recursion_check_ok() {
  let mut r = RecursionLimiter::with_limitation(5);
  for _ in 0..5 {
    r.increase();
  }
  assert!(r.check().is_ok());
}

#[test]
fn recursion_check_exceeded() {
  let mut r = RecursionLimiter::with_limitation(3);
  for _ in 0..4 {
    r.increase();
  }
  let err = r.check().unwrap_err();
  assert_eq!(err.depth(), 4);
  assert_eq!(err.limitation(), 3);
}

#[test]
fn recursion_limit_exceeded_display() {
  let mut r = RecursionLimiter::with_limitation(2);
  for _ in 0..3 {
    r.increase();
  }
  let err = r.check().unwrap_err();
  let s = format!("{err}");
  assert!(s.contains("recursion limit exceeded"));
}

#[test]
fn recursion_tracker_trait_increase_and_check() {
  let mut r = RecursionLimiter::with_limitation(3);
  <RecursionLimiter as RecursionTracker>::increase(&mut r);
  assert_eq!(r.depth(), 1);
  assert!(<RecursionLimiter as RecursionTracker>::check(&r).is_ok());
}

#[test]
fn recursion_tracker_trait_decrease() {
  let mut r = RecursionLimiter::new();
  r.increase();
  <RecursionLimiter as RecursionTracker>::decrease(&mut r);
  assert_eq!(r.depth(), 0);
}

#[test]
fn recursion_increase_and_check() {
  let mut r = RecursionLimiter::with_limitation(2);
  assert!(r.increase_and_check().is_ok());
  assert!(r.increase_and_check().is_ok());
  assert!(r.increase_and_check().is_err());
}

// ── Limiter (combined) ──────────────────────────────────────────────────────

#[test]
fn limiter_new_defaults() {
  let l = Limiter::new();
  assert_eq!(l.token().tokens(), 0);
  assert_eq!(l.token().limitation(), usize::MAX);
  assert_eq!(l.recursion().depth(), 0);
  assert_eq!(l.recursion().limitation(), 500);
}

#[test]
fn limiter_default_equals_new() {
  let a = Limiter::default();
  let b = Limiter::new();
  assert_eq!(a, b);
}

#[test]
fn limiter_with_token_tracker() {
  let l = Limiter::with_token_tracker(TokenLimiter::with_limitation(100));
  assert_eq!(l.token().limitation(), 100);
  assert_eq!(l.recursion().limitation(), 500);
}

#[test]
fn limiter_with_recursion_tracker() {
  let l = Limiter::with_recursion_tracker(RecursionLimiter::with_limitation(50));
  assert_eq!(l.recursion().limitation(), 50);
  assert_eq!(l.token().limitation(), usize::MAX);
}

#[test]
fn limiter_with_trackers() {
  let l = Limiter::with_trackers(
    TokenLimiter::with_limitation(5000),
    RecursionLimiter::with_limitation(200),
  );
  assert_eq!(l.token().limitation(), 5000);
  assert_eq!(l.recursion().limitation(), 200);
}

#[test]
fn limiter_increase_token() {
  let mut l = Limiter::new();
  l.increase_token();
  assert_eq!(l.token().tokens(), 1);
}

#[test]
fn limiter_token_mut() {
  let mut l = Limiter::new();
  l.token_mut().increase();
  assert_eq!(l.token().tokens(), 1);
}

#[test]
fn limiter_increase_recursion() {
  let mut l = Limiter::new();
  l.increase_recursion();
  assert_eq!(l.recursion().depth(), 1);
}

#[test]
fn limiter_recursion_mut() {
  let mut l = Limiter::new();
  l.recursion_mut().increase();
  assert_eq!(l.recursion().depth(), 1);
}

#[test]
fn limiter_decrease_recursion() {
  let mut l = Limiter::new();
  l.increase_recursion();
  l.decrease_recursion();
  assert_eq!(l.recursion().depth(), 0);
}

#[test]
fn limiter_check_ok() {
  let mut l = Limiter::with_trackers(
    TokenLimiter::with_limitation(10),
    RecursionLimiter::with_limitation(5),
  );
  l.increase_token();
  l.increase_recursion();
  assert!(l.check().is_ok());
}

#[test]
fn limiter_check_token_exceeded() {
  let mut l = Limiter::with_token_tracker(TokenLimiter::with_limitation(2));
  l.increase_token();
  l.increase_token();
  l.increase_token();
  let err = l.check().unwrap_err();
  assert!(err.is_token());
}

#[test]
fn limiter_check_recursion_exceeded() {
  let mut l = Limiter::with_recursion_tracker(RecursionLimiter::with_limitation(2));
  l.increase_recursion();
  l.increase_recursion();
  l.increase_recursion();
  let err = l.check().unwrap_err();
  assert!(err.is_recursion());
}

#[test]
fn limiter_recursion_checked_first() {
  // Both limits exceeded; recursion is checked first
  let mut l = Limiter::with_trackers(
    TokenLimiter::with_limitation(0),
    RecursionLimiter::with_limitation(0),
  );
  l.increase_token();
  l.increase_recursion();
  let err = l.check().unwrap_err();
  assert!(err.is_recursion());
}

// ── LimitExceeded ────────────────────────────────────────────────────────────

#[test]
fn limit_exceeded_is_token() {
  let mut t = TokenLimiter::with_limitation(0);
  t.increase();
  let inner = t.check().unwrap_err();
  let e = LimitExceeded::Token(inner);
  assert!(e.is_token());
  assert!(!e.is_recursion());
}

#[test]
fn limit_exceeded_is_recursion() {
  let mut r = RecursionLimiter::with_limitation(0);
  r.increase();
  let inner = r.check().unwrap_err();
  let e = LimitExceeded::Recursion(inner);
  assert!(e.is_recursion());
  assert!(!e.is_token());
}

#[test]
fn limit_exceeded_unwrap_token() {
  let mut t = TokenLimiter::with_limitation(0);
  t.increase();
  let inner = t.check().unwrap_err();
  let e = LimitExceeded::Token(inner);
  let unwrapped = e.unwrap_token();
  assert_eq!(unwrapped.tokens(), 1);
}

#[test]
fn limit_exceeded_unwrap_recursion() {
  let mut r = RecursionLimiter::with_limitation(0);
  r.increase();
  let inner = r.check().unwrap_err();
  let e = LimitExceeded::Recursion(inner);
  let unwrapped = e.unwrap_recursion();
  assert_eq!(unwrapped.depth(), 1);
}

#[test]
fn limit_exceeded_try_unwrap_token_ok() {
  let mut t = TokenLimiter::with_limitation(0);
  t.increase();
  let inner = t.check().unwrap_err();
  let e = LimitExceeded::Token(inner);
  assert!(e.try_unwrap_token().is_ok());
}

#[test]
fn limit_exceeded_try_unwrap_recursion_err() {
  let mut t = TokenLimiter::with_limitation(0);
  t.increase();
  let inner = t.check().unwrap_err();
  let e = LimitExceeded::Token(inner);
  assert!(e.try_unwrap_recursion().is_err());
}

#[test]
fn limit_exceeded_display_token() {
  let mut t = TokenLimiter::with_limitation(0);
  t.increase();
  let inner = t.check().unwrap_err();
  let e = LimitExceeded::Token(inner);
  let s = format!("{e}");
  assert!(s.contains("token"));
}

#[test]
fn limit_exceeded_display_recursion() {
  let mut r = RecursionLimiter::with_limitation(0);
  r.increase();
  let inner = r.check().unwrap_err();
  let e = LimitExceeded::Recursion(inner);
  let s = format!("{e}");
  assert!(s.contains("recursion"));
}

// ── Tracker trait on Limiter ─────────────────────────────────────────────────

#[test]
fn tracker_trait_increase_token() {
  let mut l = Limiter::new();
  <Limiter as Tracker>::increase_token(&mut l);
  assert_eq!(l.token().tokens(), 1);
}

#[test]
fn tracker_trait_increase_recursion() {
  let mut l = Limiter::new();
  <Limiter as Tracker>::increase_recursion(&mut l);
  assert_eq!(l.recursion().depth(), 1);
}

#[test]
fn tracker_trait_decrease_recursion() {
  let mut l = Limiter::new();
  l.increase_recursion();
  <Limiter as Tracker>::decrease_recursion(&mut l);
  assert_eq!(l.recursion().depth(), 0);
}

#[test]
fn tracker_trait_check() {
  let l = Limiter::new();
  assert!(<Limiter as Tracker>::check(&l).is_ok());
}

#[test]
fn tracker_increase_both() {
  let mut l = Limiter::new();
  l.increase_both();
  assert_eq!(l.token().tokens(), 1);
  assert_eq!(l.recursion().depth(), 1);
}

#[test]
fn tracker_increase_token_and_decrease_recursion() {
  let mut l = Limiter::new();
  l.increase_recursion();
  l.increase_token_and_decrease_recursion();
  assert_eq!(l.recursion().depth(), 0);
  assert_eq!(l.token().tokens(), 1);
}

#[test]
fn tracker_increase_token_and_check() {
  let mut l = Limiter::with_token_tracker(TokenLimiter::with_limitation(1));
  assert!(<Limiter as TrackerExt>::increase_token_and_check(&mut l).is_ok());
  assert!(<Limiter as TrackerExt>::increase_token_and_check(&mut l).is_err());
}

#[test]
fn tracker_increase_both_and_check() {
  let mut l = Limiter::with_recursion_tracker(RecursionLimiter::with_limitation(1));
  l.increase_both_and_check().unwrap();
  let err = l.increase_both_and_check().unwrap_err();
  assert!(err.is_recursion());
}

#[test]
fn tracker_increase_token_and_decrease_recursion_and_check() {
  let mut l = Limiter::with_token_tracker(TokenLimiter::with_limitation(1));
  l.increase_recursion();
  assert!(l.increase_token_and_decrease_recursion_and_check().is_ok());
  l.increase_recursion();
  assert!(l.increase_token_and_decrease_recursion_and_check().is_err());
}

// ── The combined check is the WHOLE tracker's check ──────────────────────────
//
// tokora#265. `Limiter` used to override the two combined `Tracker` methods and end each one
// with `<Self as TokenTracker>::check`, so a recursion depth already past its maximum answered
// `Ok(())` for as long as the token side held — including the decrementing form, whose whole
// purpose is to report what is left over after the decrement.
//
// Every cell below pins the token maximum at `usize::MAX` so ONLY recursion can fail, and
// asserts the token side is inside its limit at the instant of the check. Without that
// assertion a cell that happened to trip the token limit would pass against the defect, which
// is exactly why the pre-existing cells above are non-discriminating: each of them drives the
// token counter over its own maximum, or drives recursion back within its limit before
// checking.

/// A limiter that can only ever fail on recursion.
fn recursion_only(limitation: usize) -> Limiter {
  Limiter::with_trackers(
    TokenLimiter::with_limitation(usize::MAX),
    RecursionLimiter::with_limitation(limitation),
  )
}

/// The token side is inside its limit and the recursion side is outside its own, so a full
/// tracker check must fail and a token-only check must not.
fn assert_only_recursion_is_over(l: &Limiter) {
  assert!(
    <Limiter as TokenTracker>::check(l).is_ok(),
    "non-vacuity: the token side must be inside its limit, so only a full check can fail"
  );
  assert!(
    l.recursion().depth() > l.recursion().limitation(),
    "non-vacuity: the recursion side must be outside its limit at the moment of the check"
  );
}

#[test]
fn tracker_token_increment_still_checks_preexisting_recursion_excess() {
  let mut l = recursion_only(1);
  l.increase_recursion();
  l.increase_recursion();
  assert_only_recursion_is_over(&l);

  let err = <Limiter as TrackerExt>::increase_token_and_check(&mut l)
    .expect_err("pre-existing recursion excess must be reported");
  assert!(err.is_recursion());
  assert_only_recursion_is_over(&l);
}

#[test]
fn tracker_combined_decrease_still_checks_remaining_recursion_excess() {
  let mut l = recursion_only(1);
  l.increase_recursion();
  l.increase_recursion();
  l.increase_recursion();

  // The decrement takes the depth from 3 to 2, which still exceeds the maximum of 1.
  let err = l
    .increase_token_and_decrease_recursion_and_check()
    .expect_err("remaining recursion excess must be reported");
  assert!(err.is_recursion());
  assert_eq!(l.recursion().depth(), 2);
  assert_only_recursion_is_over(&l);
}

#[test]
fn tracker_combined_decrease_succeeds_once_the_decrement_lands_within_the_limit() {
  let mut l = recursion_only(1);
  l.increase_recursion();
  l.increase_recursion();

  // 2 -> 1, which is the maximum and therefore not an excess.
  l.increase_token_and_decrease_recursion_and_check()
    .expect("a decrement that lands on the maximum is within the limit");
  assert_eq!(l.recursion().depth(), 1);
  assert_eq!(l.token().tokens(), 1);
}

#[test]
fn tracker_combined_methods_keep_recursion_first_precedence() {
  let mut l = Limiter::with_trackers(
    TokenLimiter::with_limitation(0),
    RecursionLimiter::with_limitation(0),
  );
  l.increase_recursion();

  // Both sides are over once the token increment lands, and `Limiter::check` reports recursion
  // first, so the combined form must agree with it rather than answer with the token error.
  let combined = <Limiter as TrackerExt>::increase_token_and_check(&mut l)
    .expect_err("both limits are exceeded");
  assert!(combined.is_recursion());
  assert!(<Limiter as TokenTracker>::check(&l).is_err());
  assert!(matches!(l.check(), Err(e) if e.is_recursion()));
}

#[test]
fn tracker_increase_both_and_check_reports_preexisting_recursion_excess() {
  let mut l = recursion_only(1);
  l.increase_recursion();
  l.increase_recursion();
  assert_only_recursion_is_over(&l);

  let err = l
    .increase_both_and_check()
    .expect_err("the increment pushes recursion further past a maximum it already exceeded");
  assert!(err.is_recursion());
}

// ── The same cells through the logos forwarders ──────────────────────────────
//
// A `Limiter` installed as logos `Extras` reaches these methods through the blanket
// `TrackerExt` impl on `Lexer<'a, T>`, so the forwarder cannot narrow what the extras answer.

#[cfg(feature = "logos_0_16")]
mod logos_forwarding {
  use super::{Limiter, Tracker, TrackerExt, recursion_only};
  use tokora::logos::{self, Logos};

  #[derive(Logos, Debug, PartialEq)]
  #[logos(crate = logos, extras = Limiter)]
  enum Tok {
    #[token("a")]
    A,
  }

  #[test]
  fn lexer_forwards_the_full_check_on_token_increment() {
    let mut lexer = Tok::lexer_with_extras("a", recursion_only(1));
    Tracker::increase_recursion(&mut lexer);
    Tracker::increase_recursion(&mut lexer);

    let err = TrackerExt::increase_token_and_check(&mut lexer)
      .expect_err("the extras' recursion excess must survive the forwarder");
    assert!(err.is_recursion());
    assert_eq!(lexer.extras.recursion().depth(), 2);
  }

  #[test]
  fn lexer_forwards_the_full_check_on_decrement() {
    let mut lexer = Tok::lexer_with_extras("a", recursion_only(1));
    Tracker::increase_recursion(&mut lexer);
    Tracker::increase_recursion(&mut lexer);
    Tracker::increase_recursion(&mut lexer);

    let err = TrackerExt::increase_token_and_decrease_recursion_and_check(&mut lexer)
      .expect_err("the excess left after the decrement must survive the forwarder");
    assert!(err.is_recursion());
    assert_eq!(lexer.extras.recursion().depth(), 2);
  }
}
