use super::*;
use crate::lexer::{DummyLexer, DummyToken};

// Regression coverage for the constructors' `Lang` threading.
//
// Before the fix, the constructors bounded their parser-function parameter as
// `F: ParseInput<'inp, L, O, Ctx>` — the trait's own `Lang` parameter was left elided
// (defaulting to `()`), even though the constructor itself is generic over an explicit `Lang`
// and the `Ctx` bound (`ParseContext<'inp, L, Lang>`) already threads it. Per the blanket
// `FnMut` impl in `parse_input.rs`, a closure whose `InputRef` names a non-`()` `Lang` only
// implements `ParseInput<..., Ctx, Lang>`, never `ParseInput<..., Ctx, ()>` — so the bound was
// unsatisfiable for any `Lang`-generic caller (e.g. a GraphQL-tagged closure through
// `Parser::with_parser::<..., GraphQL>`).
//
// These tests only need to type-check: like the other `DummyLexer`-typed tests in this
// module tree (see `parser/any/tests.rs`), they never drive the lexer (`DummyLexer::new` /
// `with_state` are `todo!()`) — the bug and the fix are both purely at the bound level.

/// A marker `Lang` distinct from `()`, standing in for a real grammar tag (e.g. `GraphQL`).
struct TestLang;

/// A parser function pinned to `TestLang`: implements `ParseInput<'inp, DummyLexer, DummyToken,
/// Ctx, TestLang>` via the blanket `FnMut` impl, and nothing else — in particular, not
/// `ParseInput<'inp, DummyLexer, DummyToken, Ctx, ()>`, which is what the pre-fix bounds
/// required regardless of the constructor's own `Lang` argument.
fn lang_probe<'inp, Ctx>(
  _input: &mut InputRef<'inp, '_, DummyLexer, Ctx, TestLang>,
) -> Result<DummyToken, <Ctx::Emitter as Emitter<'inp, DummyLexer, TestLang>>::Error>
where
  Ctx: ParseContext<'inp, DummyLexer, TestLang>,
{
  Ok(DummyToken)
}

#[test]
fn with_parser_threads_lang() {
  // RED on main: E0277, `fn(...) {lang_probe::<Ctx>}` does not implement
  // `ParseInput<'_, DummyLexer, DummyToken, FatalContext<'_, DummyLexer, (), TestLang>, ()>`.
  let _p = Parser::with_parser::<'_, DummyLexer, DummyToken, (), _, TestLang>(lang_probe);
}

#[test]
fn with_parser_and_context_threads_lang() {
  let ctx = FatalContext::<'_, DummyLexer, (), TestLang>::of(Fatal::of());
  // RED on main: same E0277 class as `with_parser`, against the caller-supplied `Ctx`.
  let _p =
    Parser::with_parser_and_context::<'_, DummyLexer, DummyToken, _, _, TestLang>(lang_probe, ctx);
}

#[test]
fn apply_threads_lang() {
  let p = Parser::new::<'_, DummyLexer, DummyToken, (), TestLang>();
  // RED on main: same E0277 class, on the `apply`-path sibling constructor.
  let _p = p.apply::<_, TestLang>(lang_probe);
}
