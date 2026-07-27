// R8: an error type that absorbs none of the five token-level failures cannot be a tokora
// parse error, and the curated `FromTokenErrors` message is what the consumer must read.
#![allow(dead_code)]

use tokora::{Lexer, emitter::FromTokenErrors};

struct MyError;

fn needs_bundle<'inp, L: Lexer<'inp>>() {
  fn inner<'inp, L: Lexer<'inp>, E: FromTokenErrors<'inp, L>>() {}
  inner::<L, MyError>();
}

fn main() {}
