// The bundle lattice is strict, and this case is the wall between its two tiers.
//
// `ComposableEmitter` is the collecting combinators' DEFAULT-POLICY surface. A count or
// separator policy — `at_most`, `bounded`, `require_leading`, `require_trailing` — needs three
// more sub-traits, and `PolicyComposableEmitter` is the bundle that carries them.
//
// If somebody later widens `ComposableEmitter` to include them, this case starts COMPILING and
// therefore FAILS — which is exactly when the two-tier documentation must be revisited. The
// `.stderr` beside it names `TooManyEmitter` byte-exactly, so the failure says which member
// moved rather than merely that something did.
//
// The positive control is `bundle2_policy_ok` in `tests/bundle_elaboration.rs`
// (`policy_bundle_drives_at_most`), which differs from this in exactly the bundle name.
#![allow(dead_code)]

use tokora::{Lexer, emitter::{ComposableEmitter, TooManyEmitter}};

fn needs_the_policy_tier<'inp, L: Lexer<'inp>, E>()
where
  E: TooManyEmitter<'inp, L>,
{
}

fn bundle1_does_not_reach_the_policy_tier<'inp, L: Lexer<'inp>, E>()
where
  E: ComposableEmitter<'inp, L>,
{
  needs_the_policy_tier::<L, E>();
}

fn main() {}
