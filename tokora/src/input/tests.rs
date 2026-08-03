use super::*;
use crate::cache::DefaultCache;
use crate::emitter::Fatal;
use crate::lexer::DummyLexer;
use crate::parse_context::FatalContext;

/// The emitter/cache pair an `Input` binds, spelled the way `ParseContext::provide` spells it.
fn dummy_context() -> InputContext<Fatal<()>, DefaultCache<'static, DummyLexer>> {
  InputContext::new(
    Fatal::<()>::new(),
    DefaultCache::<'_, DummyLexer>::default(),
  )
}

#[test]
fn input_context_new_and_into_components() {
  let ctx = InputContext::new("emitter", 42u32);
  let (e, c, r) = ctx.into_components();
  assert_eq!(e, "emitter");
  assert_eq!(c, 42u32);
  // `new` carries the default budget: protection on, at tokora's own native-stack-safe depth —
  // NOT `RecursionLimiter::new()`'s own general-purpose default, which is a different number for
  // a different subject (see `RecursionLimiter`'s `Two Defaults, Two Subjects` docs).
  assert_eq!(r.limitation(), 64);
  assert_ne!(
    r.limitation(),
    crate::state::recursion_tracker::RecursionLimiter::new().limitation(),
    "InputContext's parser-facing default must not equal RecursionLimiter::new()'s general one"
  );
  assert_eq!(r.depth(), 0);
}

#[test]
fn input_context_with_recursion_limiter_replaces_the_default() {
  let ctx = InputContext::new("emitter", 42u32)
    .with_recursion_limiter(crate::state::recursion_tracker::RecursionLimiter::unlimited());
  let (_, _, r) = ctx.into_components();
  assert_eq!(r.limitation(), usize::MAX);
  assert_eq!(r.depth(), 0);
}

#[test]
fn input_context_different_types() {
  let ctx = InputContext::new(std::vec![1, 2, 3], Some("cache"));
  let (e, c, _) = ctx.into_components();
  assert_eq!(e, std::vec![1, 2, 3]);
  assert_eq!(c, Some("cache"));
}

#[test]
fn input_with_state_and_context() {
  let input = Input::<'_, DummyLexer, FatalContext<'_, DummyLexer, ()>>::with_state_and_context(
    "hello",
    (),
    dummy_context(),
  );
  let _ = input;
}

/// `as_ref` takes no emitter: the handle borrows the one the input was constructed with, so there
/// is no per-borrow pairing left for a call site to get wrong.
#[test]
fn input_as_ref_borrows_the_bound_emitter() {
  let mut input = Input::<'_, DummyLexer, FatalContext<'_, DummyLexer, ()>>::with_state_and_context(
    "hello",
    (),
    dummy_context(),
  );
  let input_ref = input.as_ref();
  let _ = input_ref;
}
