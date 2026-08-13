// `Errors::overflowed` promises to report whether any error was dropped for want of capacity,
// and that fact exists nowhere but the wrapper: a bounded container that is full and one that is
// full *and has refused ten errors* are the same container. So the flag cannot be derived from
// any container state — it has to be maintained, and only `push`/`try_push` maintain it.
//
// Through 0.9.1 `Errors` derived `DerefMut`, and `derive_more`'s `#[as_mut]` generated an
// `AsMut<C>` beside it. Both handed callers the container's own insertion API, through which a
// bounded container rejects a value, returns it, and `overflowed()` goes on saying `false` — in a
// bounded configuration, silent diagnostic truncation reported as complete (al8n/tokora#247).
//
// Both doors are removed rather than documented, and this case is the rail for both. Removing
// only one would have relocated the door rather than closed it, so the second function is not a
// second case: it is the other half of the same removal, and a file that pinned only the first
// would go green over a re-derived `AsMut<C>`.
//
// Re-derive either one and this file compiles clean, which trybuild reports as a FAILURE of the
// case.
//
// What survives, and is deliberately NOT attacked here: the shared views (`Deref`, the derived
// `AsRef<C>`, `IntoIterator for &Errors`, `Display`), in-place element mutation (`iter_mut`, and
// the hand-written `AsMut<[E]>` the second function below still resolves to), and removal
// (`pop`, `clear`). None of them can create the fact `overflowed` reports. The runtime half —
// every surviving insertion door maintaining the flag, the flag surviving removal and
// reinsertion, and reading/mutating/removing all still reachable — is in
// `src/error/errors/tests.rs`, which runs on every toolchain.
//
// What it does NOT pin: `C` is the caller's own type and Rust has no bound excluding interior
// mutability, so a container that mutates itself through `&self` reaches the same state through
// `Deref`. That residue is documented on the type, in the terms `HashSet` uses for a key.
// `unused_mut` is allowed for one reason and it is an artifact rather than a signal: the only
// mutable use of the first binding is the line that no longer compiles, so rustc reports the
// `mut` as unnecessary. It is necessary in the world this case is a rail against, and the golden
// file should carry the two removals rather than a consequence of one of them.
#![allow(dead_code, unused_mut)]

use std::vec::Vec;

use tokora::error::Errors;

/// `DerefMut` — the container's own insertion API, under a name `Errors` does not declare.
fn insert_through_deref_mut() {
  let mut errors: Errors<&str> = Errors::new();
  errors.push_back("bypass");
}

/// `AsMut<C>` — the same door under another name. `AsMut<[E]>` survives and is what this now
/// resolves to, so the refusal is the element view refusing to be the container view.
fn insert_through_as_mut_container() {
  let mut errors: Errors<i32, Vec<i32>> = Errors::from_container(Vec::new());
  let _container: &mut Vec<i32> = errors.as_mut();
}

fn main() {}
