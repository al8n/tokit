// R8: a redriving `PartialSession` refuses a RECORDING emitter at compile time. A session
// re-drives from base every attempt, so an emitter that records what it is told accumulates
// the replayed prefix; `Sink` is deliberately not a `ValueKeyedEmitter` — its marks are
// positions in an event log, not a function of emission state — and `PartialSession::parse`
// leans on exactly that fact.
#![allow(dead_code)]

use tokora::{
  Lexer,
  cst::Sink,
  emitter::{ValueKeyedEmitter, Verbose},
};

fn session_refuses_a_recording_sink<'inp, L: Lexer<'inp>>() {
  fn inner<E: ValueKeyedEmitter>() {}
  inner::<Sink<'inp, L, Verbose<()>>>();
}

fn main() {}
