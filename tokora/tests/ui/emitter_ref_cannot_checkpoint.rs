// `InputRef::emitter_ref` is the observation door: a **shared** reference to the parse's own
// emitter, for reading a concrete emitter's state mid-parse. Capturing a checkpoint is not an
// observation — it registers per-mark state a later `rewind` or `release` must find, and the
// input layer owns the lineage those marks belong to.
//
// Through 0.9.1 `Emitter::checkpoint` took `&self`, so the observation door handed out that
// capability and the line below compiled. Under the recording `Sink` it pushed a mark-stack row
// no `InputRef` lineage owned and no settle would spend; a row's index is a structural floor for
// the hole wrap, so a recovery whose diagnostic spanned both tokens of `"ab"` materialized an
// error node over one of them — a fully covered, silently wrong tree rather than a refusal
// (al8n/tokora#257).
//
// The repair is the receiver, not a warning: a source census over the crate's own call sites
// cannot reach a downstream caller, and the signature said the call was allowed. This case is
// the rail. It is deliberately written against a **generic** `Ctx::Emitter` rather than against
// the sink whose corruption was found, because the wall is not sink-specific: no emitter, of any
// type, is checkpointable through a shared reference. Widen `checkpoint`'s receiver back to
// `&self` and this file compiles clean, which trybuild reports as a FAILURE of the case.
//
// The same wall covers `EmitterView::emitter_ref`, the callback surface's twin door, for the
// same reason and by the same signature.
//
// What it does NOT pin: `Ctx::Emitter` is the caller's own type and Rust has no bound excluding
// interior mutability, so an emitter that mutates itself through `&self` still can. That residue
// is documented on both accessors, in the terms `HashSet` uses for a key, and no compile-fail
// case can close it.
//
// The runtime half — that everything which *does* compile through this door moves no row, and
// that the wrap still covers both tokens after it — is
// `permitted_inspection_through_a_shared_sink_reference_moves_no_row` in `src/cst/sink/tests.rs`,
// which runs on every toolchain.
#![allow(dead_code)]

use tokora::{Emitter, InputRef, Lexer, ParseContext};

fn stray_checkpoint<'inp, L, Ctx>(inp: &mut InputRef<'inp, '_, L, Ctx>) -> u64
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L>,
{
  Emitter::<L>::checkpoint(inp.emitter_ref())
}

fn main() {}
