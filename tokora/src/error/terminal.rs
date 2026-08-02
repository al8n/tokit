//! The terminal-stop discriminator: the [`MaybeTerminal`] trait that lets recovery re-raise a
//! terminal stop instead of spending it as a recoverable failure.

/// Discriminates whether an error value represents a **terminal stop** — a resource limit the parse
/// tripped, which no amount of further input can clear.
///
/// # The terminal sources
///
/// **Three** values in this crate are *built* as terminal and handed to a caller's error type.
/// Two of them answer [`is_terminal`](Self::is_terminal) for themselves, so an arm holding one
/// **delegates**; the third does not implement this trait at all, so an arm holding it must answer
/// for it. Every variant of your error type that can hold one needs its own arm:
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
/// No other value in this crate **reports itself** terminal. The trait's only other implementations
/// here are [`NonAssociativeChain`](crate::error::NonAssociativeChain) and `()`, and both keep the
/// `false` default deliberately — the first is malformed input, which recovery *may* spend; the
/// second stores nothing at all.
///
/// # This list is what the crate knows it produces, not a proof of closure
///
/// Read the table as *"handle these three"*, **not** as *"nothing else can be terminal"*. It
/// enumerates the values this crate builds and marks; it does not enumerate the **events** that can
/// reach you as terminal conditions, and those are not the same set. A terminal event can arrive
/// through a carrier nobody marked — including from inside this crate, on a path the table does not
/// name. That is not hypothetical, and the concrete one is in
/// [Where the set stops being closed](MaybeTerminal#where-the-set-stops-being-closed), together
/// with the rule to write when your arm holds something the table never mentions.
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
/// source reports for itself and answering `true` where it does not, **and one arm for anything else
/// a tripped limit can reach it through**, decided by
/// [the rule below](MaybeTerminal#where-the-set-stops-being-closed). A type that stores several must
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
/// The table is closed over one thing only: the terminal-**marked** values this crate constructs.
/// Those three exhaust *that* set, and the trait's only other implementations here keep the `false`
/// default on purpose. That is a statement about carriers. It is **not** a statement about terminal
/// *conditions*, and the two come apart in both directions.
///
/// **Inside this crate — a scanner trip your emitter rejected.** When the scanner trips a resource
/// limit it reports the diagnostic to your emitter first. If the emitter *accepts* it, the consume
/// path builds the committed form's end-of-input error and marks it terminal, and you get row one of
/// the table. If the emitter **rejects** it — returns `Err`, which for
/// [`Fatal`](crate::emitter::Fatal) is not a refusal to report but *is* the report, and which any
/// fail-fast emitter of your own does too — that `Err` is what propagates, and it was built by
/// `From<<L::Token as Token>::Error>` from the lexer's own error value. No
/// [`UnexpectedEnd`](crate::error::UnexpectedEnd) is constructed on that path, so
/// [`into_terminal`](crate::error::UnexpectedEnd::into_terminal) never runs and **nothing marks it**.
/// The same trip, at the same position, reaches you as a plain lexer error whose
/// [`is_terminal`](Self::is_terminal) is whatever your own arm says — and if that arm says `false`,
/// recovery spends it and the session's terminal latch never closes. This crate does not mark that
/// path; the arm is yours to answer for, and the rule below is how.
///
/// **Outside this crate — anything you wrap.** `MaybeTerminal` is unsealed and its default is
/// `false`, so a downstream type may report itself terminal for a limit of its own: an error wrapped
/// in from another crate, a resource guard in your own grammar, a budget your
/// [`State::check`](crate::state::State::check) enforces. No table here can enumerate those.
///
/// ## The rule for an arm this table does not name
///
/// Three steps, in order.
///
/// 1. **Delegate** if the value implements `MaybeTerminal`. Its own answer is the accurate one.
/// 2. **Otherwise ask what can arrive through the arm, not what the arm is called.** The variant
///    that catches people is the one your lexer-error conversion builds: every error type a
///    rejecting emitter can produce needs `From<<L::Token as Token>::Error>`
///    ([`FromEmitterError`](crate::emitter::FromEmitterError) requires it), so *that* variant is
///    exactly where a rejected scanner trip lands. If your lexer's
///    [`State::check`](crate::state::State::check) can refuse — a token budget, a nesting cap, a
///    byte cap — then the lexer-error arm can hold a tripped limit, and it is terminal when it does.
/// 3. **If you still cannot tell, answer `true`.** The question the whole trait asks is *can more
///    input clear this?*, and when you cannot answer it the two mistakes are not symmetric:
///
/// | Wrong arm | What it costs |
/// |---|---|
/// | `false` on a real terminal stop | [`Recover`](crate::parser::Recover), [`InplaceRecover`](crate::parser::InplaceRecover) and [`skip_then_retry`](crate::ParseInput::skip_then_retry) **spend** it: they synthesize a value and re-enter the parser, which re-trips the same limit. Under a [`PartialSession`](crate::input::PartialSession) the terminal latch misses it too, so the next attempt is admitted and re-lexes the same prefix. Nothing in this crate bounds either loop |
/// | `true` on a recoverable failure | recovery re-raises what it could have repaired, so one diagnostic becomes a hard stop; under a `PartialSession` it also latches the session shut and later attempts are refused. Both fail **closed** — you lose recovery, loudly, and never spin |
///
/// Malformed input is the one case where `false` is affirmatively right, and you can always
/// recognize it: you built the value from a construct the **grammar** rejected, not from a limit the
/// **runtime** refused. That is exactly why
/// [`NonAssociativeChain`](crate::error::NonAssociativeChain) answers `false`.
///
/// ```
/// use tokora::{
///   error::{MaybeTerminal, RecursionLimitReached, UnexpectedEot},
///   input::SessionRefusal,
///   state::recursion_tracker::{RecursionLimiter, RecursionTracker},
/// };
///
/// // The lexer's own error type — what `From<<L::Token as Token>::Error>` builds from. A
/// // REJECTING emitter converts one of these straight into `MyError` when it refuses a scanner
/// // diagnostic, so this arm can hold a tripped limit with nothing at all marking it.
/// enum LexError {
///   /// A byte no token can start with: malformed input, which recovery may spend.
///   BadChar,
///   /// The lexer's `State::check` refused — a budget is spent, and no input clears it.
///   LimitTripped,
/// }
///
/// // A user error that keeps every terminal-capable value, so every marker survives.
/// enum MyError {
///   Eot(UnexpectedEot),
///   Depth(RecursionLimitReached),
///   Refused(SessionRefusal),
///   Lex(LexError),
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
///       // The arm no table names, decided by PROVENANCE: a scanner trip whose diagnostic the
///       // emitter rejected lands here unmarked, so this arm is the only thing that can tell
///       // recovery to re-raise it rather than spend it and re-trip the same limit.
///       MyError::Lex(LexError::LimitTripped) => true,
///       MyError::Lex(LexError::BadChar) => false,
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
///
/// // And the arm the table does not name: same variant, opposite answers, decided by where the
/// // value came from — a refused limit is terminal, rejected input is not.
/// assert!(MyError::Lex(LexError::LimitTripped).is_terminal());
/// assert!(!MyError::Lex(LexError::BadChar).is_terminal());
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
