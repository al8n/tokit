//! The terminal-stop discriminator: the [`MaybeTerminal`] trait that lets recovery re-raise a
//! terminal stop instead of spending it as a recoverable failure.

/// Discriminates whether an error value represents a **terminal stop** — a resource limit the parse
/// tripped, which no amount of further input can clear.
///
/// # The terminal sources
///
/// **Three** values in this crate can put a terminal stop into a caller's error type, and this is
/// the whole set. Two of them answer [`is_terminal`](Self::is_terminal) for themselves, so an arm
/// holding one **delegates**; the third does not implement this trait at all, so an arm holding it
/// must answer for it. Every variant of your error type that can hold one needs its own arm:
///
/// | Source | Terminal when | Your arm writes | How it reaches you, and what a wrong arm costs |
/// |---|---|---|---|
/// | [`UnexpectedEnd`](crate::error::UnexpectedEnd) | its [`is_terminal`](crate::error::UnexpectedEnd::is_terminal) flag is raised — a **scanner** stop: a scanner resource-limit trip, or the poison boundary it latches | `e.is_terminal()` | through `From<UnexpectedEnd<…>>`, as the committed form's end-of-input error, so it reads as an ordinary end of input to a caller that does not care yet stays distinguishable from a *genuine* end of input to one that does. An arm left at `false` is **spent silently**, as a recoverable failure |
/// | [`RecursionLimitReached`](crate::error::RecursionLimitReached) | **always** — a **descent** stop: the frame budget [`InputRef::descend`](crate::InputRef::descend) enforces, which both Pratt engines enter every frame through | `e.is_terminal()` | through `From<RecursionLimitReached<…>>`, as its own type on the `Err` channel, never emitted. It latches nothing: the unwind restores the depth cell on its way out. An arm left at `false` is **spent silently** |
/// | [`SessionRefusal`](crate::input::SessionRefusal) | **always** — a **session** stop: the cross-attempt byte budget is exhausted, or an earlier attempt latched the session shut. Both are decided before any attempt work | `true`, spelled out — this type deliberately does **not** implement `MaybeTerminal`, so there is nothing to delegate to | through `From<SessionRefusal>` inside [`PartialSession::parse`](crate::input::PartialSession::parse), which then **asserts** the converted value is terminal. That assertion is unconditional, so an arm left at `false` is a **panic in a release build**, not a silent spend — see the [coherence law](crate::input::SessionRefusal#the-coherence-law) for why it is a panic and not a returned error |
///
/// Recovery is the caller that must care about the first two, and it asks *this trait* rather than
/// either type — so a grammar error that holds one of them decides the verdict by what its
/// [`is_terminal`](Self::is_terminal) returns, not by what the value inside it would have said. The
/// third has a stricter caller: the session gate does not consult the verdict, it *requires* it.
///
/// Nothing else in this crate is terminal. The trait's only other implementations here are
/// [`NonAssociativeChain`](crate::error::NonAssociativeChain) and `()`, and both keep the `false`
/// default deliberately — the first is malformed input, which recovery *may* spend; the second
/// stores nothing at all.
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
/// terminal signal — **one arm per source in the table above that it stores**, delegating where the
/// source reports for itself and answering `true` where it does not. A type that stores several must
/// answer for every one of them: an arm that answers for one source and leaves another at `false`
/// spends that half as a recoverable failure, or — for a `SessionRefusal` arm — panics the next
/// refused attempt.
///
/// A conversion that **discards** the source discards the marker with it. The converted value is
/// non-terminal whatever the value it came from said — `()` included, since the sink stores nothing
/// — and recovery will spend the stop rather than re-raise it. See
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached)'s own
/// [section on a discarding sink](crate::error::RecursionLimitReached#a-discarding-sink-erases-the-stop-and-does-not-erase-the-bound)
/// for what that costs and what it does not.
///
/// # Where the set stops being closed
///
/// The table is the complete set of terminal values **this crate** hands you, and it is closed by
/// construction rather than by recollection: those three are the only sources, and the trait's only
/// other implementations here keep the `false` default on purpose. It is *not* closed for values
/// this crate does not hand you. `MaybeTerminal` is unsealed and its default is `false`, so a
/// downstream type may report itself terminal for a limit of its own — a lexer's
/// [`Token::Error`](crate::Token::Error), an error wrapped in from another crate, a resource guard
/// in your own grammar — and no table here can enumerate those.
///
/// An arm holding one of them follows the same two rules. **Delegate** if the value implements
/// `MaybeTerminal`. Otherwise decide it with the question the whole trait is: *can more input clear
/// this?* If it can, the arm is `false`. If it cannot, and the value is a tripped limit rather than
/// malformed input, the arm is `true` — malformed input is the recoverable case, which is exactly
/// why [`NonAssociativeChain`](crate::error::NonAssociativeChain) answers `false`.
///
/// ```
/// use tokora::{
///   error::{MaybeTerminal, RecursionLimitReached, UnexpectedEot},
///   input::SessionRefusal,
///   state::recursion_tracker::{RecursionLimiter, RecursionTracker},
/// };
///
/// // A user error that keeps every terminal-capable value, so every marker survives.
/// enum MyError {
///   Eot(UnexpectedEot),
///   Depth(RecursionLimitReached),
///   Refused(SessionRefusal),
///   Other,
/// }
///
/// // The session gate converts through this impl, then asserts the result is terminal.
/// impl From<SessionRefusal> for MyError {
///   fn from(refusal: SessionRefusal) -> Self {
///     MyError::Refused(refusal)
///   }
/// }
///
/// impl MaybeTerminal for MyError {
///   fn is_terminal(&self) -> bool {
///     match self {
///       // Only when the scanner raised the flag: a genuine end of input is not terminal.
///       MyError::Eot(e) => e.is_terminal(),
///       // Always: a depth budget is never cleared by more input.
///       MyError::Depth(e) => e.is_terminal(),
///       // Always, and spelled out: `SessionRefusal` has no `is_terminal` to delegate to.
///       MyError::Refused(_) => true,
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
///
/// // The coherence law `PartialSession::parse` asserts, checked here instead of at the panic:
/// // both refusal legs must convert to a value that answers `true`.
/// for refusal in [
///   SessionRefusal::BudgetExhausted { spent: 10, budget: 8 },
///   SessionRefusal::TerminalLatched,
/// ] {
///   assert!(MyError::from(refusal).is_terminal());
/// }
/// ```
pub trait MaybeTerminal {
  /// Returns `true` iff this error value represents a terminal stop. Defaults to `false`.
  #[inline(always)]
  fn is_terminal(&self) -> bool {
    false
  }
}

/// The unit error sink is never a terminal signal: it stores nothing, so it can carry no source's
/// marker. Converting a terminal stop into `()` is therefore an opt-out of terminal re-raise — see
/// [Opting in](MaybeTerminal#opting-in). It is also why this crate ships no
/// `From<SessionRefusal>` for `()`, and why it never will: that conversion is the one the
/// [session gate](crate::input::SessionRefusal#the-coherence-law) *requires* to be terminal rather
/// than merely consulting, and a sink that always answers `false` can never satisfy it.
impl MaybeTerminal for () {}
