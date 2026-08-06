// THE FAILING EXIT CANNOT BE INHERITED. `CstEmitter::cst_demote` is a REQUIRED method, and this
// case is the wall.
//
// The shape below is not hypothetical — it is the exact state the compiler steers a 0.8-era
// wrapper into. Such a wrapper forwarded the four CST methods that existed when it was written;
// rebasing it onto 0.9.0 produces exactly one diagnostic, `E0053` on `cst_start`, and rustc's own
// fix-it writes the migration ("change the output type to match the trait: `-> EventMark`") while
// mentioning `cst_demote` nowhere. Apply the suggestion and, if the fifth method were defaulted,
// the impl would compile with zero warnings and be byte-perfect on every success path — and on
// `node()`'s failing exit the demote would dispatch into the inherited empty body, leaving the
// inner channel a `StartNode` no exit ever closes. That residue is byte-identical to the one an
// escaped panic leaves, so no sink-side check can ever tell the two apart: strict `finish` would
// refuse a grammar that did everything right, and `finish_partial` would materialize the node the
// grammar retracted. There is no runtime wall to build here, which is why the wall is `E0046`.
//
// So this case's subject is the ABSENCE of a default body on one trait method, and its teeth are
// mostly aimed at the future. Re-add that default "for convenience" and this file starts
// compiling; a `compile_fail` case that compiles is a failing test, and it is the only signal
// there would be — every other gate stays green, because the trap is silent by construction.
// `every_ui_case_file_is_registered` keeps it from being quietly deregistered instead.
//
// What it does NOT claim: nothing forces a wrapper to forward. Writing `fn cst_demote(..) {}` here
// would compile, and that is the whole point of the remedy — discard-by-silence becomes
// discard-in-writing, in the implementor's own diff, where a reviewer reads it. See
// `CstEmitter::cst_demote`'s *Required, not defaulted* for the failure-direction rule that makes
// this method the only one on the trait to be graded this way.
#![allow(dead_code)]

use tokora::{
  Emitter, Lexer, Token, cst::event::EventMark, emitter::CstEmitter,
  error::token::UnexpectedTokenOf, input::Cursor, span::Spanned,
};

/// A generic forwarding wrapper — the fully-public downstream population. It cannot mint an
/// `EventMark` of its own (`EventMark::new` and `::inert` are both crate-private), so every
/// downstream `cst_start` overrider is a forwarder or an instrumentor, and both owe the demote.
struct Wrapper<E>(E);

impl<'inp, L, E> Emitter<'inp, L> for Wrapper<E>
where
  L: Lexer<'inp>,
  E: Emitter<'inp, L>,
{
  type Error = E::Error;

  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), Self::Error> {
    self.0.emit_lexer_error(err)
  }

  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'inp, L, ()>,
  ) -> Result<(), Self::Error> {
    self.0.emit_unexpected_token(err)
  }

  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error> {
    self.0.emit_error(err)
  }

  fn rewind(&mut self, cursor: &Cursor<'inp, '_, L>, checkpoint: u64) {
    self.0.rewind(cursor, checkpoint);
  }
}

// The four methods a 0.8 wrapper knew about, forwarded — including `cst_start` with the return
// type rustc's fix-it supplies. `cst_demote` is deliberately absent: that omission is the case.
impl<'inp, L, E> CstEmitter<'inp, L> for Wrapper<E>
where
  L: Lexer<'inp>,
  E: CstEmitter<'inp, L>,
{
  fn cst_start(&mut self, kind: u16) -> EventMark {
    self.0.cst_start(kind)
  }

  fn cst_finish(&mut self, kind: u16) {
    self.0.cst_finish(kind);
  }

  fn cst_mark(&mut self) -> EventMark {
    self.0.cst_mark()
  }

  fn cst_start_at(&mut self, mark: EventMark, kind: u16) {
    self.0.cst_start_at(mark, kind);
  }
}

fn main() {}
