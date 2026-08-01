//! The terminal-stop discriminator: the [`MaybeTerminal`] trait that lets recovery re-raise a
//! terminal stop instead of spending it as a recoverable failure.

/// Discriminates whether an error value represents a **terminal stop** — a resource limit the parse
/// tripped, which no amount of further input can clear.
///
/// # The two carriers
///
/// Two error types in this crate can report themselves terminal, and they differ in how they say so
/// and in how they reach a caller. An error type that can hold either must be able to answer for it:
///
/// | Carrier | Terminal when | How it surfaces |
/// |---|---|---|
/// | [`UnexpectedEnd`](crate::error::UnexpectedEnd) | its [`is_terminal`](crate::error::UnexpectedEnd::is_terminal) flag is raised — a **scanner** stop: a scanner resource-limit trip, or the poison boundary it latches | as the committed form's end-of-input error, so it reads as an ordinary end of input to a caller that does not care, yet stays distinguishable from a *genuine* end of input to one that does |
/// | [`RecursionLimitReached`](crate::error::RecursionLimitReached) | **always** — a **descent** stop: the frame budget [`InputRef::descend`](crate::InputRef::descend) enforces, which both Pratt engines enter every frame through | as its own type, returned on the `Err` channel and never emitted. It latches nothing: the unwind restores the depth cell on its way out |
///
/// Recovery is the caller that must care, and it asks *this trait* rather than either type — so a
/// grammar error that holds one of them decides the verdict by what its [`is_terminal`](Self::is_terminal)
/// returns, not by what the value inside it would have said.
///
/// # The never-recoverable dual
///
/// A recoverer synthesizes a value from a **malformed** construct. A terminal stop is not malformed
/// — it is a construct the parser was forbidden to finish reading, and no amount of input clears a
/// tripped limit. Recovering it would fabricate a value from input the parser may never look at, and
/// re-entering the parser from the recoverer only re-trips the same limit. So a terminal stop must
/// be **re-raised untouched**, exactly as an [`Incomplete`](crate::error::Incomplete) is — the two
/// are duals: an incomplete says "more input may fix this", a terminal stop says "no input ever
/// will", and neither may be spent as a recoverable failure.
/// [`Recover`](crate::parser::Recover), [`InplaceRecover`](crate::parser::InplaceRecover), and
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) require this bound and re-raise, rather
/// than recover, when `is_terminal()` holds.
///
/// # Opting in
///
/// This is the minimal hook that makes the law testable on any error type, mirroring
/// [`MaybeIncomplete`](crate::error::MaybeIncomplete): the single method
/// [`is_terminal`](Self::is_terminal) has a **blanket `false` default**, so an error type opts in
/// with an empty `impl MaybeTerminal for MyError {}` and overrides the method only if it can carry a
/// terminal signal — by delegating to **whichever of the two carriers above it stores**. A type that
/// stores both must delegate to both: an arm that answers for one and leaves the other at `false`
/// spends that half as a recoverable failure.
///
/// A conversion that **discards** the carrier discards the marker with it. The converted value is
/// non-terminal whatever the value it came from said — `()` included, since the sink stores nothing
/// — and recovery will spend the stop rather than re-raise it. See
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached)'s own
/// [section on a discarding sink](crate::error::RecursionLimitReached#a-discarding-sink-erases-the-stop-and-does-not-erase-the-bound)
/// for what that costs and what it does not.
///
/// ```
/// use tokora::{
///   error::{MaybeTerminal, RecursionLimitReached, UnexpectedEot},
///   state::recursion_tracker::{RecursionLimiter, RecursionTracker},
/// };
///
/// // A user error that keeps both terminal-capable values, so both markers survive.
/// enum MyError {
///   Eot(UnexpectedEot),
///   Depth(RecursionLimitReached),
///   Other,
/// }
/// impl MaybeTerminal for MyError {
///   fn is_terminal(&self) -> bool {
///     match self {
///       // Only when the scanner raised the flag: a genuine end of input is not terminal.
///       MyError::Eot(e) => e.is_terminal(),
///       // Always: a depth budget is never cleared by more input.
///       MyError::Depth(e) => e.is_terminal(),
///       MyError::Other => false,
///     }
///   }
/// }
///
/// let genuine = MyError::Eot(UnexpectedEot::eot(7));
/// assert!(!genuine.is_terminal());
/// let tripped = MyError::Eot(UnexpectedEot::eot(7).into_terminal());
/// assert!(tripped.is_terminal());
///
/// let mut limiter = RecursionLimiter::with_limitation(1);
/// limiter.increase();
/// limiter.increase();
/// let exceeded = RecursionTracker::check(&limiter).unwrap_err();
/// assert!(MyError::Depth(RecursionLimitReached::of(7, exceeded)).is_terminal());
/// ```
pub trait MaybeTerminal {
  /// Returns `true` iff this error value represents a terminal stop. Defaults to `false`.
  #[inline(always)]
  fn is_terminal(&self) -> bool {
    false
  }
}

/// The unit error sink is never a terminal signal: it stores nothing, so it can carry neither
/// carrier's marker. Converting a terminal stop into `()` is therefore an opt-out of terminal
/// re-raise — see [Opting in](MaybeTerminal#opting-in).
impl MaybeTerminal for () {}
