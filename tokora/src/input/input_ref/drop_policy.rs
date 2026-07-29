//! Compile-time drop policy for the transaction guards.
//!
//! An undecided [`Transaction`](super::Transaction) /
//! [`StackedTransaction`](super::StackedTransaction) needs a rule for what its `Drop`
//! does. That rule is a **typestate**, not a runtime field: the guard carries a
//! zero-sized policy marker as a type parameter, so the choice is fixed at the type
//! level (it cannot be forgotten or mutated after the guard is built) and each flavour
//! monomorphizes to a branch-free drop.
//!
//! - [`Rollback`] — the speculative default: dropping an undecided guard restores the
//!   input to the begin point, exactly as an explicit `rollback` would. Uncommitted
//!   speculative work is discarded, the database default.
//! - [`Commit`] — commit-by-default: dropping an undecided guard *keeps* the progress,
//!   identical to dropping a raw [`Checkpoint`](crate::input::Checkpoint) — including
//!   when an error propagates out of the guard through `?` under a fail-fast emitter, in
//!   which case the drop keeps the progress consumed up to the error rather than rolling
//!   it back.
//!
//! Each guard has a default constructor that selects [`Rollback`]
//! ([`begin`](super::InputRef::begin), [`begin_stacked`](super::InputRef::begin_stacked))
//! and a generic one that selects any policy
//! ([`begin_with`](super::InputRef::begin_with),
//! [`begin_stacked_with`](super::InputRef::begin_stacked_with)). With both policies the
//! guard family is capability-complete over every legal (last-in, first-out) flow: the
//! speculative shape and the commit-by-default shape (the dual exercised by the Pratt
//! operator loop) each have a guard, so no legal flow is forced back to raw
//! `save`/`restore`.

pub(crate) mod sealed {
  /// Seals [`DropPolicy`](super::DropPolicy) and carries its drop-behaviour selector.
  ///
  /// Only nameable inside this crate, so no downstream type can implement it — and hence
  /// none can implement [`DropPolicy`](super::DropPolicy). The set of policies is closed
  /// to exactly the two markers defined here.
  pub trait Sealed {
    /// Whether an *undecided* guard of this policy restores (rolls back) on drop.
    ///
    /// The guards' single generic `Drop` impl branches on this `const`; because it is a
    /// compile-time constant, each policy monomorphizes to a straight-line drop with the
    /// other arm eliminated.
    const ROLLBACK_ON_DROP: bool;
  }
}

/// The drop policy of a transaction guard — what an *undecided* guard does when dropped.
///
/// A closed set of two zero-sized markers, [`Rollback`] and [`Commit`], chosen as a type
/// parameter on [`Transaction`](super::Transaction) and
/// [`StackedTransaction`](super::StackedTransaction). The trait is **sealed**: exactly
/// these two policies exist, and the choice is a compile-time typestate rather than a
/// runtime flag. Each marker's documentation says when to reach for it; the constructors
/// that select one are [`begin`](super::InputRef::begin) /
/// [`begin_with`](super::InputRef::begin_with) and their stacked counterparts.
///
/// # The rollback guarantee is conditional on the emitter's and cache's own laws
///
/// A rolling-back drop calls foreign code — [`Emitter::rewind`](crate::Emitter::rewind) and the
/// [`Cache`](crate::cache::Cache) restore-path operations — and it may run mid-unwind, where a
/// second panic aborts the process. Both surfaces therefore carry an explicit non-panicking law
/// for exactly this call site. An implementation that breaks it turns a rollback into either an
/// abort (mid-unwind) or a partially-restored input (an ordinary drop a host catches); the
/// crate cannot detect either, and the same unspecified-but-bounded posture applies as to any
/// other contract violation.
pub trait DropPolicy: sealed::Sealed {}

/// The speculative, rollback-on-drop policy — the default.
///
/// An undecided [`Transaction`](super::Transaction) /
/// [`StackedTransaction`](super::StackedTransaction) with this policy restores the input
/// to its begin point when dropped, exactly as an explicit
/// [`rollback`](super::Transaction::rollback) would: uncommitted speculative work is
/// discarded, the database default. This is the policy selected by
/// [`begin`](super::InputRef::begin) and
/// [`begin_stacked`](super::InputRef::begin_stacked).
#[derive(Debug)]
pub struct Rollback;

/// The commit-by-default, keep-on-drop policy.
///
/// An undecided guard with this policy *keeps* its progress when dropped — identical to
/// dropping a raw [`Checkpoint`](crate::input::Checkpoint), including when an error
/// propagates out of the guard through `?` under a fail-fast emitter (the drop keeps the
/// progress consumed up to the error, never rolling it back). It is the dual of the
/// speculative default and the shape a commit-by-default loop wants — the Pratt operator
/// loop keeps progress on every success and every `?`-propagation and rolls back only on
/// its two "operator isn't ours" exits. Select it with
/// [`begin_with`](super::InputRef::begin_with) /
/// [`begin_stacked_with`](super::InputRef::begin_stacked_with).
///
/// # A panic is not a decision
///
/// Keeping progress is what this policy means for an *ordinary* drop. An undecided guard
/// dropped while the thread is **unwinding** rolls back instead (std builds): a panic aborts
/// the region rather than completing it, and promoting half an iteration would leave the input
/// in a state no non-panicking execution can produce — visible to any host that catches. The
/// explicit [`commit`](super::Transaction::commit) is untouched, and so is the `?` path, which
/// is a return and not an unwind.
///
/// Under `no_std` the divergence is documented rather than fixed: `core` exposes no
/// `panicking()`, so an unwinding drop there still commits. It is observable only from a std
/// host embedding a `no_std`-configured tokora — an uncaught panic otherwise ends the program,
/// and `catch_unwind` is std-only.
#[derive(Debug)]
pub struct Commit;

impl sealed::Sealed for Rollback {
  const ROLLBACK_ON_DROP: bool = true;
}
impl DropPolicy for Rollback {}

impl sealed::Sealed for Commit {
  const ROLLBACK_ON_DROP: bool = false;
}
impl DropPolicy for Commit {}

/// Whether the current thread is unwinding from a panic — the fact an undecided guard's `Drop`
/// consults before it decides.
///
/// A `Drop` that runs while a panic is in flight is not an ordinary end of scope: it is an
/// abort of the region, and promoting the region's speculative progress there produces an input
/// state no non-panicking execution can reach (the operator consumed, its right-hand side
/// absent). Every invariant a consumer derives from "input states are normal-flow-reachable"
/// stops holding for a host that catches. So a std build reads the unwind fact and rolls back.
///
/// Under `no_std` there is no such fact to read — `core` has no `panicking()` — so the constant
/// `false` keeps the commit-on-unwind behaviour there, which is the documented divergence on
/// [`Commit`]. It is observable only from a std host embedding a `no_std`-configured tokora: an
/// uncaught panic ends the program, and `catch_unwind` is std-only.
#[cfg(feature = "std")]
#[inline]
pub(crate) fn unwinding() -> bool {
  std::thread::panicking()
}

/// The `no_std` half of [`unwinding`]: no panic fact exists, so an undecided guard decides by
/// its policy alone and the two arms monomorphize exactly as before.
#[cfg(not(feature = "std"))]
#[inline(always)]
pub(crate) fn unwinding() -> bool {
  false
}
