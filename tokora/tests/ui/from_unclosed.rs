// `FromUnclosed` is named directly by `emit_unclosed` and by hand-written emitter
// where-clauses, so it is a root obligation there. Its own curated message must fire.
#![allow(dead_code)]

use tokora::{Lexer, emitter::FromUnclosed};

struct MyError;

fn needs_from_unclosed<'inp, L: Lexer<'inp>>() {
  fn inner<'inp, L: Lexer<'inp>, E: FromUnclosed<'inp, L>>() {}
  inner::<L, MyError>();
}

fn main() {}
