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
/// | [`RecursionLimitReached`](crate::error::RecursionLimitReached) | **always** — a **descent** stop: the frame budget [`InputRef::descend`](crate::InputRef::descend) enforces, which both Pratt engines enter every frame through | `e.is_terminal()` | through `From<RecursionLimitReached<…>>`, as its own type on the `Err` channel, never emitted. **This is the one row whose stop does not depend on your arm**: the trip latches the input session, and the three recovery combinators read that latch beside this trait, so an arm left at `false` — `()` included — loses the *payload* and still re-raises. Write the arm anyway if you want the offset and the depth |
/// | [`SessionRefusal`](crate::input::SessionRefusal) | **always** — a **session** stop: the cross-attempt byte budget is exhausted, or an earlier attempt latched the session shut. Both are decided before any attempt work | `true`, spelled out — this type deliberately does **not** implement `MaybeTerminal`, so there is nothing to delegate to | through `From<SessionRefusal>` inside [`PartialSession::parse`](crate::input::PartialSession::parse), which then **asserts** the converted value is terminal. That assertion is unconditional, so an arm left at `false` is a **panic in a release build**, not a silent spend — see the [coherence law](crate::input::SessionRefusal#the-coherence-law) for why it is a panic and not a returned error |
///
/// Recovery is the caller that must care about the first two, and it asks *this trait* rather than
/// either type. For [`UnexpectedEnd`](crate::error::UnexpectedEnd) that is the whole answer: a
/// grammar error holding one decides the verdict by what its [`is_terminal`](Self::is_terminal)
/// returns, not by what the value inside it would have said. For
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) the payload arm still matters —
/// for the details it carries (the offset, the depth) and for a value your own code fabricated
/// rather than received from a real trip — but it no longer decides the verdict alone: an actual
/// input-descent trip is decided by the session latch **in addition to** the trait check, so an arm
/// left at `false` — `()` included — no longer settles the question by itself. The third has a
/// stricter caller: the session gate does not consult the verdict, it *requires* it.
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
/// than recover, when `is_terminal()` holds — and on two input-side witnesses beside it, because a
/// terminal *event* can reach them through a carrier nothing marked. See
/// [Where the set stops being closed](MaybeTerminal#where-the-set-stops-being-closed).
///
/// # The channel no witness is consulted on: a value the grammar consumed input to produce
///
/// Every gate that law drives — this trait, and the two input-side witnesses beside it: the
/// **descent** counter [`InputRef::descend`](crate::InputRef::descend) bumps, and the **scanner**
/// counter the crate's terminal predicate bumps, which the three recovery combinators read and the
/// twelve resilient collection drivers read on its own, carrying no `MaybeTerminal` bound at
/// all — sits on a channel where *this crate* is the one drawing a conclusion. A recoverer
/// synthesizes a value for a construct that failed; a collection driver concludes the construct
/// ended because its element declined, made no progress, or was followed by a closer. Each infers
/// something from a stop the caller was never told about, so each has to guard its own inference.
///
/// **No witness is consulted when the grammar produces the value itself — and consumed input to
/// produce it.** Grammar code is entitled to catch a terminal stop — a
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) out of a nested descent, a
/// scanner trip inside a speculation of its own — and what it does next is the whole distinction:
/// code that catches a stop, consumes, and still yields a value has **answered** it, where code
/// that catches one and then declines has **swallowed** it, asserting an absence built on a stop
/// nothing else can see. An element that catches a trip, still consumes, and returns
/// [`ParseAttempt::Accept`](crate::try_parse_input::ParseAttempt) is the first: it hands the driver
/// a value, and the driver collects it exactly as it would from an element no budget ever touched.
/// The collection then closes normally — the delimited `separated` driver commits a real closer
/// left at the following slot straight from its own mid-scan arm, one of the two **direct** closers
/// its gate census exempts, and exempt precisely because only an accepting element can precede
/// one — and succeeds.
///
/// ## The exemption stops at a zero-width `Accept`, and that is the case to get right
///
/// *Consumes* is load-bearing above, and this is the shape the answered/swallowed framing does not
/// settle on its own. An element that catches a stop and returns a value **while consuming
/// nothing** has produced a value — but it has not moved the parse, and the driver's very next act
/// is to read that absence of progress as *no more elements*. **That** conclusion is the driver's
/// own, manufactured from a stop the caller was never told about, which is exactly what every gate
/// above exists to guard. So the stall is gated, through the same chokepoint a decline reaches and
/// by all three witnesses at once: the collection ends on the `Err` channel with a terminal
/// end-of-input instead of yielding a truncated container. Producing a value is therefore not the
/// test — *concluding nothing of the driver's* is, and a zero-width return concludes something.
///
/// It is not a corner case, either. The four `*_while` collection drivers and the `*_while` folds
/// take their element through [`ParseInput`](crate::ParseInput), which has **no decline channel at
/// all**, so returning a value while consuming nothing is the only way one of their elements can
/// report absence: there the zero-width return *is* the decline.
/// `tokora/tests/collection_resource_trip.rs` drives both absence exits of the four try-driven
/// families through the guard in its section 4 —
/// `a_stalling_element_that_answered_a_trip_does_not_end_repeated` and its three siblings — and
/// every one of the other eight drivers in its section 7.
///
/// **The consuming half of the boundary is as designed, not a gate nobody has reached yet.**
/// Refusing an `Accept` that consumed would mean a value-producing parser can never recover from a
/// budget it deliberately caught — a contract this crate makes for no other error, since nothing
/// stops a grammar from catching *any* error and returning a value without diagnosing it, and
/// terminal stops are not singled out for that treatment. Section 6 of the same file pins that half
/// in both directions, so the boundary cannot drift silently: narrower, if a *consuming* `Accept`
/// starts being gated, or wider, if the absence and closer exits stop being.
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
/// — and recovery will spend the stop rather than re-raise it. **The one exception is the resource
/// budget**: a [`RecursionLimitReached`](crate::error::RecursionLimitReached) trip is recorded on
/// the input session as well as in the value, and the three recovery combinators read both, so that
/// stop survives a discarding conversion. Every other row of the table above is yours to carry. See
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached)'s own
/// [section on the payload and the stop](crate::error::RecursionLimitReached#the-stop-does-not-travel-in-the-payload-so-a-discarding-sink-cannot-drop-it)
/// for why exactly one row works that way and what it still costs to discard the value.
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
/// [`is_terminal`](Self::is_terminal) is whatever your own arm says. This crate still does not
/// **mark** that path — there is no `UnexpectedEnd` on it to raise a flag on.
///
/// What it does now is not depend on the mark. The scanner's trip is **counted on the input
/// session**, and [`Recover`](crate::parser::Recover),
/// [`InplaceRecover`](crate::parser::InplaceRecover) and
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) read that counter — attempt-relative,
/// against a per-attempt snapshot — beside this trait, exactly as they read the session counter for
/// a descent trip. So an unmarked scanner trip is **re-raised rather than spent** whatever your arm
/// says, and the two emitters now agree: same trip, same verdict, accepting sink or rejecting one.
/// Before that, a `false` arm here meant the recoverer ran, re-entered the scanner and re-tripped
/// the same limit.
///
/// A **counter** and not the poison boundary that same trip latches, and the difference is not
/// cosmetic: the boundary is inside the rollback set, so grammar code that catches the stop inside
/// a speculation of its own has that rollback erase it before any enclosing gate looks. A count
/// nothing lowers is what survives that, at any nesting depth.
///
/// The arm is still yours to answer for, and the rule below is still how — for two reasons that
/// outlive the recovery combinators. A [`PartialSession`](crate::input::PartialSession)'s terminal
/// latch reads *only* this trait, so a `false` arm still admits the next attempt over a spent
/// budget; and your own code, and any caller that asks `is_terminal()` for itself, still gets the
/// answer your arm gives.
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
/// | `false` on a real terminal stop | [`Recover`](crate::parser::Recover), [`InplaceRecover`](crate::parser::InplaceRecover) and [`skip_then_retry`](crate::ParseInput::skip_then_retry) **spend** it: they synthesize a value and re-enter the parser, which re-trips the same limit. Under a [`PartialSession`](crate::input::PartialSession) the terminal latch misses it too, so the next attempt is admitted and re-lexes the same prefix. Nothing in this crate bounds either loop. The three recovery combinators are covered for the **two limits this crate itself enforces** — a scanner trip through the input's poison latch, a descent trip through the session counter, neither of which your conversion can discard — so what a `false` arm still spends there is a limit of *your own*; the `PartialSession` latch has no such second witness and misses every one |
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
///       // emitter rejected lands here unmarked. Recovery no longer depends on this arm for one —
///       // it reads the input's poison latch too — but a `PartialSession`'s terminal latch does,
///       // and so does anything of yours that asks.
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
  #[inline]
  fn is_terminal(&self) -> bool {
    false
  }
}

/// The unit error sink is never a terminal signal: it stores nothing, so it can carry no source's
/// marker — `is_terminal` on `()` always answers `false`. For a stop whose only carrier is the
/// converted value, that makes converting it into `()` an opt-out of terminal re-raise — see
/// [Opting in](MaybeTerminal#opting-in). **The one exception is
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached)**: converting a trip to `()`
/// still loses the *payload* — the offset, the depth, the limitation — but not the *stop*, because
/// the trip also latches the input session, and the three recovery combinators consult that latch
/// beside this trait. A `()`-errored grammar therefore still re-raises an actual input-descent
/// trip, exactly as a delegating error type does. See
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached)'s own
/// [section on the payload and the stop](crate::error::RecursionLimitReached#the-stop-does-not-travel-in-the-payload-so-a-discarding-sink-cannot-drop-it)
/// for the detail. It is also why this crate ships no `From<SessionRefusal>` for `()`, and why it
/// never will: that conversion is the one the
/// [session gate](crate::input::SessionRefusal#the-coherence-law) *requires* to be terminal rather
/// than merely consulting, and a sink that always answers `false` can never satisfy it.
impl MaybeTerminal for () {}
