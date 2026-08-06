// THEOREM, second half: the manual-`impl` route is refused too.
//
// The companion case (`session_point_cannot_cross_into_a_bracket.rs`) drives a **closure**
// through the blanket `ParseInput` impl, so a reader could reasonably suspect the refusal is an
// artifact of closure inference — that spelling the parser out by hand would let a
// pre-opened `SessionPointId` in through the front door. It does not, and this case is why:
// the wall is the trait's own quantification, not the closure's.
//
// A hand-written implementor may store whatever it likes, including a branded id. But
// `parse_input`'s handle lifetime is fresh at every call — `&mut InputRef<'inp, '_, …>` — and
// `SessionPointId<'closure>` is invariant, so a stored id branded with the *struct's* region
// can be neither shortened nor lengthened into the method's. rustc reports the equality
// requirement in both directions below, and names the invariance as the cause.
//
// Together the two cases say: **no safe surface can settle a point across a `node()`
// boundary**, which is what makes the bracket's every-build `validate_start_mark` assert
// unreachable from the blessed combinator, and what a future session verb must re-prove. See
// the module docs on `src/parser/node.rs`.
#![allow(dead_code)]

use tokora::{
  Emitter, InputRef, Lexer, ParseContext, ParseInput, SessionPointId, emitter::CstEmitter,
  parser::node,
};

const K_NODE: u16 = 1;

/// A parser that smuggles in an id opened before the bracket that will drive it.
struct Rollback<'closure> {
  id: SessionPointId<'closure>,
}

impl<'inp, L, Ctx> ParseInput<'inp, L, (), Ctx, ()> for Rollback<'_>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, ()>,
{
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, ()>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, ()>>::Error> {
    // The one offending line, exactly as in the closure case.
    input.rollback_point(self.id);
    Ok(())
  }
}

// The call site the impl above exists to serve — kept so the case shows the whole shape it
// refuses, not just an orphan impl. It contributes no error of its own.
fn drive<'inp, 'closure, L, Ctx>(
  input: &mut InputRef<'inp, 'closure, L, Ctx, ()>,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, ()>>::Error>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, ()>,
  Ctx::Emitter: CstEmitter<'inp, L, ()>,
{
  let id: SessionPointId<'closure> = input.begin_point();
  node(K_NODE, Rollback { id }).parse_input(input)
}

fn main() {}
