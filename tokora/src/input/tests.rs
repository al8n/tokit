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
  let (e, c, r, b) = ctx.into_components();
  assert_eq!(e, "emitter");
  assert_eq!(c, 42u32);
  // `new` carries the default budget: protection on, at tokora's own native-stack-safe depth —
  // NOT `RecursionLimiter::new()`'s own general-purpose default, which is a different number for
  // a different subject (see `RecursionLimiter`'s `Two Defaults, Two Subjects` docs).
  //
  // Named rather than written out: the claim being made here is that `new` carries the PARSE
  // default AT ALL, not what that default is. The number itself is stated as a literal in exactly
  // one place — `state/recursion_tracker/tests.rs` — and derived from the measurement by the
  // `const` assertions beside it. Writing it out here too would be a third copy that has to be
  // found and edited whenever the first two move.
  assert_eq!(
    r.limitation(),
    crate::state::recursion_tracker::RecursionLimiter::PARSE_DEFAULT_DEPTH
  );
  assert_ne!(
    r.limitation(),
    crate::state::recursion_tracker::RecursionLimiter::new().limitation(),
    "InputContext's parser-facing default must not equal RecursionLimiter::new()'s general one"
  );
  assert_eq!(r.depth(), 0);
  // The token budget's default is the OPPOSITE of the recursion budget's: protection OFF. A depth
  // ceiling protects the native stack, which every build shares, so a number can be picked for
  // everyone; an item ceiling is a property of the grammar and the document, so seeding one would
  // change the behaviour of every parse that never asked for it.
  assert_eq!(b, crate::input::TokenBudget::unlimited());
  assert_eq!(b.limitation(), usize::MAX);
  // A context carries a CEILING and no spend, so there is no `b.spent()` to read here — the tally
  // is the input's and is built at `Input::with_state_and_context`. The cell below drives that
  // through and reads it off the input instead.
  assert_eq!(
    crate::input::TokenBudgetTally::new(b).spent(),
    0,
    "and what the context hands the input starts at zero",
  );
}

#[test]
fn input_context_with_recursion_limiter_replaces_the_default() {
  let ctx = InputContext::new("emitter", 42u32)
    .with_recursion_limiter(crate::state::recursion_tracker::RecursionLimiter::unlimited());
  let (_, _, r, _) = ctx.into_components();
  assert_eq!(r.limitation(), usize::MAX);
  assert_eq!(r.depth(), 0);
}

#[test]
fn input_context_with_token_budget_replaces_the_default() {
  let ctx = InputContext::new("emitter", 42u32)
    .with_token_budget(crate::input::TokenBudget::with_limitation(7));
  let (_, _, r, b) = ctx.into_components();
  assert_eq!(b.limitation(), 7);
  assert_eq!(
    crate::input::TokenBudgetTally::new(b).spent(),
    0,
    "the door sets a ceiling; the spend is the input's and starts at zero",
  );
  // And it did not disturb the cell beside it.
  assert_eq!(
    r.limitation(),
    crate::state::recursion_tracker::RecursionLimiter::PARSE_DEFAULT_DEPTH
  );
}

#[test]
fn input_context_different_types() {
  let ctx = InputContext::new(std::vec![1, 2, 3], Some("cache"));
  let (e, c, _, _) = ctx.into_components();
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

/// The regime-id allocator's exhaustion forces *"the regime changed"*, never *"the regime is the
/// same"* — the safe direction, because the unsafe one is a false stall.
///
/// `REGIME_EXHAUSTED` is the id `install_rekey` hands out once the allocator can no longer climb,
/// so two different regimes really can wear it at once. Saturating into an ordinary value instead
/// would make a re-key at the ceiling compare **equal**, and `InputRef::scanner_outcome` would
/// answer `ScannerOutcome::Stalled` — *repeating reproduces the trip* — over an input whose regime
/// the caller has just replaced. This pins the exclusion that stops it, which no parse could reach
/// by re-keying.
#[test]
fn an_exhausted_regime_id_is_equal_to_nothing_including_itself() {
  use super::{REGIME_EXHAUSTED, same_regime};

  assert!(
    same_regime(0, 0),
    "the initial regime is the same as itself"
  );
  assert!(same_regime(7, 7), "an ordinary id is the same as itself");
  assert!(!same_regime(7, 8), "different ids are different regimes");
  assert!(
    !same_regime(REGIME_EXHAUSTED, REGIME_EXHAUSTED),
    "an exhausted allocator names every regime alike, so its id may never carry an equality — \
     answering `true` here is exactly the false `Stalled` the sentinel exists to prevent"
  );
  assert!(
    !same_regime(REGIME_EXHAUSTED, 7) && !same_regime(7, REGIME_EXHAUSTED),
    "and it is not the same as any ordinary id either, in both argument orders"
  );
}
