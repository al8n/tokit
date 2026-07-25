//! The terminal-stop discriminator: the [`MaybeTerminal`] trait that lets recovery re-raise a
//! terminal scanner stop instead of spending it as a recoverable failure.

/// Discriminates whether an error value represents a **terminal scanner stop** — a resource-limit
/// trip, or the poison boundary it latches.
///
/// A terminal stop is surfaced as the committed form's end-of-input error
/// ([`UnexpectedEnd`](crate::error::UnexpectedEnd)) carrying its
/// [`is_terminal`](crate::error::UnexpectedEnd::is_terminal) marker, so it reads as an ordinary end
/// of input to a caller that does not care, yet stays distinguishable from a *genuine* end of input
/// to one that does. Recovery is the caller that must care.
///
/// # The never-recoverable dual
///
/// A recoverer synthesizes a value from a **malformed** construct. A terminal stop is not malformed
/// — it is a construct the parser was forbidden to finish reading, and no amount of input clears a
/// tripped limit. Recovering it would fabricate a value from input the parser may never look at, and
/// re-entering the scanner from the recoverer only re-trips the same limit. So a terminal stop must
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
/// terminal signal — typically by delegating to the
/// [`UnexpectedEnd::is_terminal`](crate::error::UnexpectedEnd::is_terminal) it stores.
///
/// ```
/// use tokora::error::{MaybeTerminal, UnexpectedEot};
///
/// // A user error that keeps the end-of-input value so its terminal marker survives.
/// enum MyError {
///   Eot(UnexpectedEot),
///   Other,
/// }
/// impl MaybeTerminal for MyError {
///   fn is_terminal(&self) -> bool {
///     matches!(self, MyError::Eot(e) if e.is_terminal())
///   }
/// }
///
/// let genuine = MyError::Eot(UnexpectedEot::eot(7));
/// assert!(!genuine.is_terminal());
/// let tripped = MyError::Eot(UnexpectedEot::eot(7).into_terminal());
/// assert!(tripped.is_terminal());
/// ```
pub trait MaybeTerminal {
  /// Returns `true` iff this error value represents a terminal scanner stop. Defaults to `false`.
  #[inline(always)]
  fn is_terminal(&self) -> bool {
    false
  }
}

/// The unit error sink is never a terminal signal.
impl MaybeTerminal for () {}
