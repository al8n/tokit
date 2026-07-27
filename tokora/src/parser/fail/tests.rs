use super::*;

// Use simple non-Lexer types for PhantomData-only type params
// Fail<F, L, O, Ctx, Lang> only needs L, Ctx, Lang for PhantomData in new()
type TestFail = Fail<fn() -> (), (), (), (), ()>;
type TestFailWith = FailWith<fn() -> (), (), (), (), ()>;

#[test]
fn fail_new_debug_clone() {
  fn make_err() -> () {}
  let f: TestFail = Fail::new(make_err);
  let _ = format!("{:?}", f);
  let f2 = f.clone();
  let _ = format!("{:?}", f2);
}

#[test]
fn fail_with_new_debug_clone() {
  fn make_err() -> () {}
  let f: TestFailWith = FailWith::new(make_err);
  let _ = format!("{:?}", f);
  let f2 = f.clone();
  let _ = format!("{:?}", f2);
}

#[test]
fn fail_copy() {
  fn make_err() -> () {}
  let f: TestFail = Fail::new(make_err);
  let f2 = f;
  let f3 = f; // copy
  let _ = format!("{:?}", f2);
  let _ = format!("{:?}", f3);
}

#[test]
fn fail_with_copy() {
  fn make_err() -> () {}
  let f: TestFailWith = FailWith::new(make_err);
  let f2 = f;
  let f3 = f; // copy
  let _ = format!("{:?}", f2);
  let _ = format!("{:?}", f3);
}

#[test]
fn fail_free_fn_lang_inferred() {
  use crate::lexer::DummyLexer;
  use crate::parse_context::FatalContext;
  let _f = fail::<'_, fn() -> (), DummyLexer, (), FatalContext<'_, DummyLexer, ()>, _>(|| ());
}

#[test]
fn fail_free_fn_lang_named() {
  use crate::lexer::DummyLexer;
  use crate::parse_context::FatalContext;
  let _f = fail::<'_, fn() -> (), DummyLexer, (), FatalContext<'_, DummyLexer, (), ()>, ()>(|| ());
}

// The `fail_with` free function involves complex
// lifetime constraints on ParseState that make standalone construction
// impractical without a full parser infrastructure. The FailWith::new
// constructor is already tested above.
