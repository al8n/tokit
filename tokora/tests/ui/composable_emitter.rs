// The bundle's own headline. A type handed to a `ComposableEmitter`-generic
// signature with no emitter impl at all must get a curated message, not rustc's default
// trait-bound phrasing.
#![allow(dead_code)]

use tokora::{Lexer, emitter::ComposableEmitter};

struct MyEmitter;

fn needs_bundle<'inp, L: Lexer<'inp>>() {
  fn inner<'inp, L: Lexer<'inp>, E: ComposableEmitter<'inp, L>>() {}
  inner::<L, MyEmitter>();
}

fn main() {}
