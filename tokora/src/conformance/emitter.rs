//! Conformance kit for emitters that **bind a source**.
//!
//! # Why this exists
//!
//! [`Emitter::bound_source`] is a *defaulted* member. That is the right shape — its default,
//! `None`, is the true statement *"I bind no source"* for every emitter that only carries
//! diagnostics, which is 30 of the 31 core `Emitter` implementations in this crate and, as far
//! as the crate can measure, all of the external ones. A required member would force *an*
//! answer rather than the right one: a cargo-culted `None` written to silence a compile error
//! is byte-identical to a forgotten override, so requiring it buys a keystroke, not a thought.
//!
//! But a default that is right for the majority is wrong for the minority, and the minority
//! here is exactly the population that needs it: an emitter that **stores a source and slices
//! it later**. If such an emitter inherits the default, it compiles, it runs, and it silently
//! gives up the guarantee — the parse entry has nothing to compare, so a sink bound to a
//! foreign buffer is accepted and materializes a tree whose text came from one source and
//! whose structure came from another. Nothing downstream can see it: the event log is
//! byte-identical to one a legal single parse could produce.
//!
//! So the obligation is stated, and made runnable. This is the same answer the crate gives for
//! the [`Cache`](crate::Cache) queue laws and the [`Lexer`] contract clauses: a
//! contract that only exists as prose is a contract nobody can check.
//!
//! # What it checks
//!
//! 1. [`assert_binds_source`] — a source-binding emitter answers `Some`, and answers with the
//!    identity of the source it was actually given. An emitter that inherits the default fails
//!    here, which is the whole point.
//! 2. [`assert_forwards_bound_source`] — a *wrapper* around a source-binding emitter reports
//!    the same identity as the emitter it wraps. The `&mut U` blanket in this crate forwards
//!    it; a hand-written wrapper that forwards every emission but not this one disables the
//!    check for whatever it wraps, and that is invisible from inside tokora.
//!
//! # Violation posture
//!
//! Assert-with-context, like the rest of `conformance`: these are test tools, run from the
//! implementor's own `#[test]`s, and a failure is a bug in the emitter.

use crate::{Emitter, Lexer, source::SourceIdentity};

/// Asserts that `emitter` reports the source it was built over.
///
/// Call this from a `#[test]` in the crate that defines a source-binding emitter, passing the
/// same source value the emitter was constructed with.
///
/// # Panics
///
/// If the emitter answers `None` (it inherited [`Emitter::bound_source`]'s default while
/// actually binding a source), or answers a different identity than `source`'s.
#[track_caller]
pub fn assert_binds_source<'inp, L, Lang, E>(emitter: &E, source: &'inp L::Source)
where
  L: Lexer<'inp>,
  Lang: ?Sized,
  E: Emitter<'inp, L, Lang>,
{
  let expected = SourceIdentity::of(source);
  match emitter.bound_source() {
    None => panic!(
      "this emitter binds a source but inherits `Emitter::bound_source`'s `None` default. \
       The parse entry then has nothing to compare, so a parse driven over a *different* \
       buffer is accepted and produces a tree whose text is yours and whose structure is the \
       parse's. Override it: `fn bound_source(&self) -> Option<SourceIdentity> {{ \
       Some(SourceIdentity::of(self.source)) }}`"
    ),
    Some(got) => assert!(
      got == expected,
      "this emitter reports a different source ({got}) than the one it was given \
       ({expected}). `bound_source` must project the source the emitter will actually slice, \
       not a copy of the reference to it — for a sized `L::Source` the two are different \
       addresses"
    ),
  }
}

/// Asserts that `wrapper` reports the same bound source as the `inner` emitter it wraps.
///
/// # Panics
///
/// If the wrapper answers `None` where the inner answers `Some`, or answers a different
/// identity.
#[track_caller]
pub fn assert_forwards_bound_source<'inp, L, Lang, Inner, Wrapper>(wrapper: &Wrapper, inner: &Inner)
where
  L: Lexer<'inp>,
  Lang: ?Sized,
  Inner: Emitter<'inp, L, Lang>,
  Wrapper: Emitter<'inp, L, Lang>,
{
  let expected = Emitter::<'inp, L, Lang>::bound_source(inner);
  let got = Emitter::<'inp, L, Lang>::bound_source(wrapper);
  assert!(
    got == expected,
    "this wrapper does not forward `bound_source` (wrapper: {got:?}, inner: {expected:?}). \
     A wrapper that forwards every emission but not this one silently disables the \
     source-identity check for the emitter it wraps — the same forwarding obligation \
     `checkpoint` / `rewind` / `release` carry"
  );
}
