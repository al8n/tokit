use super::*;
use crate::lexer::DummyLexer;

#[test]
fn test_default_context() {
  fn assert_context<'inp, Ctx>()
  where
    Ctx: ParseContext<'inp, DummyLexer>,
  {
  }

  assert_context::<()>();
  assert_context::<FatalContext<'_, DummyLexer, ()>>();
}

#[test]
fn test_custom_context() {
  fn assert_context<'inp, Ctx>()
  where
    Ctx: ParseContext<'inp, DummyLexer>,
  {
  }

  assert_context::<(Fatal<()>, DefaultCache<'_, DummyLexer>)>();
}

/// **`provide` carries a caller's override through to the `InputContext` the parse is built
/// from** — the seam at which it could be lost, and the one place two construction sites meet.
///
/// `InputContext::new` seeds `RecursionLimiter::PARSE_DEFAULT_DEPTH`, and so does
/// `ParserContext::new_in`. Two sites seeding the same default is exactly the shape in which an
/// override survives one path and is re-seeded on another, and the only thing that stops it here
/// is an ordering: `provide` applies `with_recursion_limiter` *after* `InputContext::new`, so the
/// caller's value overwrites the seed rather than the other way round. Swap those two lines and
/// this cell reds while every end-to-end pin in `pratt_limit.rs` — which all drive
/// `ParserContext` — reds with it.
///
/// The four cache shapes are not decoration: `provide` matches on `self.cache` and builds the
/// `InputContext` in **two** arms, so an override reinstated in one arm and not the other would
/// be a defect only a cached context could see.
#[test]
fn provide_carries_a_recursion_override_through_both_of_its_arms() {
  use crate::cache::DefaultCache;
  use crate::state::recursion_tracker::RecursionLimiter;

  type Ctx<'inp> = ParserContext<'inp, DummyLexer, Fatal<()>, DefaultCache<'inp, DummyLexer>>;

  // The arm with no cache options.
  let (_, _, recursion, _) = ParseContext::<'_, DummyLexer, ()>::provide(
    Ctx::of(Fatal::of()).with_recursion_limiter(RecursionLimiter::with_limitation(7)),
  )
  .into_components();
  assert_eq!(recursion.limitation(), 7, "the uncached arm dropped it");

  // The arm that builds the cache from options.
  let (_, _, recursion, _) = ParseContext::<'_, DummyLexer, ()>::provide(
    // `()`, not `Default::default()`: `DefaultCache`'s `Options` is a unit, and passing it
    // through the `Default` call is `clippy::unit_arg` — the same lint the root manifest's
    // examples note already records tripping over.
    Ctx::with_cache_options_of(Fatal::of(), ())
      .with_recursion_limiter(RecursionLimiter::with_limitation(7)),
  )
  .into_components();
  assert_eq!(recursion.limitation(), 7, "the cached arm dropped it");

  // And with no override, the default arrives — the same constant, not a second seed that
  // happens to agree today.
  let (_, _, recursion, _) =
    ParseContext::<'_, DummyLexer, ()>::provide(Ctx::of(Fatal::of())).into_components();
  assert_eq!(
    recursion.limitation(),
    RecursionLimiter::PARSE_DEFAULT_DEPTH,
    "an unconfigured context must arrive carrying PARSE_DEFAULT_DEPTH"
  );
}

/// The token-budget twin of the cell above, and it exists for the identical reason: `provide`
/// builds the `InputContext` in **two** arms, and the override is applied after
/// `InputContext::new` re-seeds the unlimited default. Losing it in either arm hands an untrusted
/// parse an unbounded lexer while the caller's code still reads as if it had asked for a ceiling
/// — the failure mode with no diagnostic at all, since an unbounded budget never refuses.
#[test]
fn provide_carries_a_token_budget_override_through_both_of_its_arms() {
  use crate::cache::DefaultCache;
  use crate::input::TokenBudget;

  type Ctx<'inp> = ParserContext<'inp, DummyLexer, Fatal<()>, DefaultCache<'inp, DummyLexer>>;

  // The arm with no cache options.
  let (_, _, _, budget) = ParseContext::<'_, DummyLexer, ()>::provide(
    Ctx::of(Fatal::of()).with_token_budget(TokenBudget::with_limitation(7)),
  )
  .into_components();
  assert_eq!(budget.limitation(), 7, "the uncached arm dropped it");

  // The arm that builds the cache from options.
  let (_, _, _, budget) = ParseContext::<'_, DummyLexer, ()>::provide(
    Ctx::with_cache_options_of(Fatal::of(), ()).with_token_budget(TokenBudget::with_limitation(7)),
  )
  .into_components();
  assert_eq!(budget.limitation(), 7, "the cached arm dropped it");

  // And with no override, the unlimited default arrives.
  let (_, _, _, budget) =
    ParseContext::<'_, DummyLexer, ()>::provide(Ctx::of(Fatal::of())).into_components();
  assert_eq!(
    budget,
    TokenBudget::unlimited(),
    "an unconfigured context must arrive unbudgeted"
  );
}
