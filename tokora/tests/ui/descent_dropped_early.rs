// `InputRef::descend` hands back a `Descent` guard whose *lifetime is the recursion level*.
// Dropped early, the level is released before the frame it was taken for, and the parse recurses
// with the budget reading zero — which is the native stack overflow the budget exists to prevent,
// reintroduced in a line that used to compile without a word.
//
// The only thing standing between a reader and that line is `Descent`'s `#[must_use]`, and an
// attribute is exactly the kind of thing a refactor deletes in silence. This case is the rail: it
// fails to compile only because the attribute is there and only because the lint reaches a value
// that arrived through `?`. Delete the attribute and this file compiles clean, which trybuild
// reports as a FAILURE of the case.
//
// `deny` rather than the default `warn`, because trybuild judges compile-fail cases on the exit
// status: a warning would leave the build green and the rail inert.
//
// Three honest limits, recorded here rather than left for the next reader to find:
//
//   * The committed `.stderr` carries rustc's own `help: use `let _ = ...` to ignore the
//     resulting value` — which suggests the OTHER way to drop the guard early, and that one no
//     lint can catch. That is not a defect in the golden file; it is the shape of the hole, and
//     `Descent`'s docs say so in the same words.
//   * This is one shape of five. `let _ = …`, `if inp.descend().is_ok() { … }`,
//     `inp.descend()?.recursion().depth()` and `drop(inp.descend()?)` all release the level just
//     as early and all compile silently, so no ui case can exist for them. The runtime half of
//     the story — each of the five measured against a live budget — is
//     `src/input/input_ref/descent_tests.rs`.
//   * Like every case in this directory, this compiles only on the pinned MSRV toolchain, so the
//     rail is live in CI's `msrv` job and nowhere else. The sibling census test
//     (`every_ui_case_file_is_registered`) runs everywhere and keeps the file from going missing.
//     So does `descent_tests.rs`'s `#[expect(unused_must_use, …)]`, which becomes an unfulfilled
//     expectation — and, under the crate's `deny(warnings)`, a hard error — the moment the
//     attribute goes away. That one runs on every toolchain.
//
// The positive control is the doc example on `InputRef::descend` itself, which is this function
// with the guard bound, and which is compiled as a doctest on every toolchain. The shape a caller
// should reach for first is `InputRef::descending`, where the same defect is not expressible.
#![deny(unused_must_use)]
#![allow(dead_code)]

use tokora::{Emitter, InputRef, Lexer, ParseContext, error::RecursionLimitReached};

fn dropped_early<'inp, L, Ctx>(
  inp: &mut InputRef<'inp, '_, L, Ctx>,
  remaining: usize,
) -> Result<usize, <Ctx::Emitter as Emitter<'inp, L>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L>,
  <Ctx::Emitter as Emitter<'inp, L>>::Error: From<RecursionLimitReached<L::Offset, ()>>,
{
  inp.descend()?;
  match remaining {
    0 => Ok(inp.recursion().depth()),
    n => dropped_early(inp, n - 1),
  }
}

fn main() {}
