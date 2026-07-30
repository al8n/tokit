// A bundle MEMBER named directly by a driver's where-clause is a root obligation
// there, so its own curated message has to fire — the bundle headline is not enough to
// tell a consumer which member is missing.
#![allow(dead_code)]

use tokora::{Lexer, emitter::SeparatedEmitter};

struct MyEmitter;

fn needs_separated<'inp, L: Lexer<'inp>>() {
  fn inner<'inp, L: Lexer<'inp>, E: SeparatedEmitter<'inp, L>>() {}
  inner::<L, MyEmitter>();
}

fn main() {}
