// THEOREM. A `SessionPointId` minted **outside** a `node()` bracket cannot be settled
// **inside** it. This case is the load-bearing half of that proof: the closure form.
//
// # Why the bracket's correctness rests on this
//
// `Node`'s `ParseInput` impl is an up-front bracket — `cst_start` on entry, and on the `Err`
// exit a `cst_demote` spending the mark that start handed back. `validate_start_mark` asserts,
// **in every build**, that the mark still names a live open node; a rollback that truncated the
// buffer *below* the start would stale it and the demote would panic in release.
//
// A rollback below an in-progress bracket's start is exactly what settling a point opened
// before the bracket does. Nothing at runtime stops it: `take_point`'s settle scan is
// newest-first, and a point opened before the bracket has no younger open above it, so the scan
// accepts it. **The type system is the only wall**, and it is this:
//
// - `SessionPointId<'closure>` is invariant in `'closure` (an `fn(&'closure ()) -> &'closure ()`
//   brand, deliberately not a covariant reference);
// - a parser reaches its input through `ParseInput::parse_input`, whose handle lifetime is
//   universally quantified — `&mut InputRef<'inp, '_, …>`, a fresh region per call;
// - an invariant token branded with one concrete region cannot be handed to a frame that
//   demands every region. The two cannot unify in either direction, and rustc says so below.
//
// The mirror shape — a point opened *inside* the bracket and rolled back there — is legal,
// harmless, and green: it sits **above** the slot, so the truncation keeps the start. That
// asymmetry is pinned at runtime by
// `a_point_opened_inside_the_bracket_and_rolled_back_inside_keeps_the_demote_valid` in
// `tests/parser_node.rs`; the panic this case makes unreachable is pinned as a raw-surface
// contract by `raw_demote_after_a_rollback_below_the_start_panics` beside it.
//
// **If this file ever compiles, the wall is gone** and `node()` can panic in a release build.
// Any new session-verb surface — on `ParseState`, or any other handle a parser can reach — must
// re-prove this theorem before it ships. See the module docs on `src/parser/node.rs`.
#![allow(dead_code)]

use tokora::{
  Emitter, InputRef, Lexer, ParseContext, ParseInput, SessionPointId, emitter::CstEmitter,
  parser::node,
};

const K_NODE: u16 = 1;

// Generic over the lexer and the parse context, so the refusal is a property of the bound and
// not of one fixture's concrete types.
fn cross_into_the_bracket<'inp, 'closure, L, Ctx>(
  input: &mut InputRef<'inp, 'closure, L, Ctx, ()>,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, ()>>::Error>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, ()>,
  Ctx::Emitter: CstEmitter<'inp, L, ()>,
{
  // Opened BEFORE the bracket: its brand is this frame's concrete `'closure`.
  let id: SessionPointId<'closure> = input.begin_point();
  node(K_NODE, move |inner: &mut InputRef<'inp, '_, L, Ctx, ()>| {
    // The one offending line. The body is otherwise inert and returns `Ok(())`, so nothing
    // else here can carry the error: settling this id inside the bracket is the whole case.
    inner.rollback_point(id);
    Ok(())
  })
  .parse_input(input)
}

fn main() {}
