use core::marker::PhantomData;

use crate::state::recursion_tracker::RecursionLimitExceeded;

/// The configured recursion limit was exceeded while entering one level of recursive descent.
///
/// Raised by [`InputRef::descend`](crate::InputRef::descend), the RAII primitive both Pratt
/// engines enter their frame prologue through, and carried out on the `Err` channel — it is
/// **returned, never emitted**, so no rewind can erase it and no recording emitter can turn a
/// tripped budget into a truncated-but-successful parse.
///
/// # Always terminal
///
/// [`is_terminal`](crate::error::MaybeTerminal::is_terminal) is `true` for **every** value of
/// this type: there is no non-terminal constructor, because no amount of further input clears a
/// depth budget. That is the same law a scanner limit trip carries, reached through the same
/// [`MaybeTerminal`](crate::error::MaybeTerminal) gate that
/// [`Recover`](crate::parser::Recover), [`InplaceRecover`](crate::parser::InplaceRecover) and
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) consult: a recoverer handed this
/// value must re-raise it untouched rather than synthesize a node from input it was forbidden to
/// read. An error type that stores this one and delegates its own `is_terminal` keeps that
/// property; one whose `From` discards the value opts out of it, which is the pre-existing
/// [`MaybeTerminal`](crate::error::MaybeTerminal) posture and not a hole this type introduces.
///
/// # A discarding sink erases the stop, and does not erase the bound
///
/// The terminality above is a property of *this* type, and recovery reads it off the type the
/// grammar actually names. So a grammar whose error is `()` — or any type whose `From` for this
/// one drops the value rather than delegating — gets a converted error that reports
/// `is_terminal() == false`, and **recovery spends the trip instead of re-raising it**. That is
/// the documented opt-out reaching a resource guard rather than a malformed-input report, and it
/// is worth stating here rather than only at the [`From`] impl, because this is where a caller
/// reads the terminality promise.
///
/// What is lost is exactly the *stop*, and it is worth being precise about what is not:
///
/// * **The native stack is already back.** [`Descent`](crate::input::Descent)'s destructor
///   releases its level on the unwind that carries this error out, so by the time any recoverer
///   is handed the converted value, every frame the budget was protecting has returned. Measured,
///   not assumed: on a 200-level trip the recoverer's frame sits 160 bytes from the pre-parse
///   baseline in a debug build and 0 in a release one, against a descent that reached ~1 MiB and
///   ~97 KiB respectively. The guard's stack-safety purpose therefore survives the sink intact.
/// * **The budget is unchanged.** The depth cell reads back to what it was before the parse, so a
///   recoverer that spends the trip and descends again starts from the same depth and meets the
///   same limit. Re-tripping is bounded, not compounding.
/// * **The retries terminate.** A spent trip is a retried trip, and the retry loops are bounded by
///   their own zero-progress guards rather than by terminality:
///   [`skip_then_retry`](crate::ParseInput::skip_then_retry) consumes its sync token per
///   continuing cycle and runs out of sync points, and a repetition over a recovering element
///   stalls on the first element that commits nothing.
///
/// What *is* lost is the verdict and the input. A recoverer will synthesize a node for a
/// construct it was never allowed to read, and `skip_then_retry` will commit skipped regions
/// buying progress that cannot help — no quantity of skipped input makes the next descent
/// shallower. `tokora/tests/pratt_limit_unit_sink.rs` pins each of these against the delegating
/// counterpart in `tokora/tests/pratt_limit.rs`.
///
/// **A grammar that needs the stop semantics must not discard this value.** Give the grammar's
/// error type a variant that stores this one — or at minimum a marker — and delegate:
///
/// ```rust
/// use tokora::error::{MaybeTerminal, RecursionLimitReached};
///
/// enum MyError {
///   Limit(RecursionLimitReached<usize>),
///   // …
/// }
///
/// impl From<RecursionLimitReached<usize>> for MyError {
///   fn from(e: RecursionLimitReached<usize>) -> Self {
///     Self::Limit(e)
///   }
/// }
///
/// impl MaybeTerminal for MyError {
///   fn is_terminal(&self) -> bool {
///     match self {
///       Self::Limit(e) => e.is_terminal(), // always true; recovery re-raises
///     }
///   }
/// }
/// ```
///
/// The `()` sink stays available for grammars that only need the `Err` channel — it is what lets a
/// parser drive the Pratt engines without declaring an error type at all — but it is a statement
/// that no error of any kind carries information, resource trips included.
///
/// # Scanner trips latch; descent trips unwind
///
/// A scanner limit trip latches the input's poison boundary, because the lexer's tally is
/// monotone in the input and the tripped fact survives any rollback. Descent depth is the
/// opposite kind of fact: it is scoped to the *control stack* and is fully restored by the
/// unwind itself, since every live frame's guard decrements as it pops. So this error latches
/// **nothing**. By the time it reaches a driver the input holds no tripped condition at all, and
/// latching one would poison legitimately shallow parsing of the rest of the file after an
/// outer-level recovery — while a caller that re-enters simply re-descends and re-trips, bounded
/// by the same limit each time. The asymmetry is deliberate.
///
/// # The offset
///
/// [`offset`](Self::offset) is **committed consumption** — `span().end()` at the moment the
/// frame was entered — the same metric the pratt drivers' stall exits report, and therefore
/// independent of how much lookahead happens to be cached. A parse with a prefilled peek window
/// trips at the same offset as one with an empty cache.
///
/// # Example
///
/// ```rust
/// use tokora::{
///   error::{MaybeTerminal, RecursionLimitReached},
///   state::recursion_tracker::{RecursionLimiter, RecursionTracker},
/// };
///
/// let mut limiter = RecursionLimiter::with_limitation(1);
/// limiter.increase();
/// limiter.increase();
/// let exceeded = RecursionTracker::check(&limiter).unwrap_err();
///
/// let error: RecursionLimitReached = RecursionLimitReached::of(7, exceeded);
/// assert_eq!(error.offset(), 7);
/// assert_eq!(error.depth(), 2);
/// assert_eq!(error.limitation(), 1);
/// assert!(error.is_terminal());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("recursion limit reached at {at}: depth {}, maximum {}", .exceeded.depth(), .exceeded.limitation())]
pub struct RecursionLimitReached<O = usize, Lang: ?Sized = ()> {
  /// Committed consumption at the frame that could not be entered.
  at: O,
  /// The depth and the limitation it exceeded, as the tracker reported them.
  exceeded: RecursionLimitExceeded,
  _lang: PhantomData<Lang>,
}

impl<O, Lang: ?Sized> RecursionLimitReached<O, Lang> {
  /// Creates the trip error at `at` from the tracker's own report.
  #[inline(always)]
  pub const fn of(at: O, exceeded: RecursionLimitExceeded) -> Self {
    Self {
      at,
      exceeded,
      _lang: PhantomData,
    }
  }

  /// Returns the offset the trip was raised at — committed consumption at frame entry.
  #[inline(always)]
  pub const fn offset(&self) -> O
  where
    O: Copy,
  {
    self.at
  }

  /// Returns a reference to the offset the trip was raised at.
  #[inline(always)]
  pub const fn offset_ref(&self) -> &O {
    &self.at
  }

  /// Returns the tracker's report: the depth reached and the limitation it exceeded.
  #[inline(always)]
  pub const fn exceeded(&self) -> RecursionLimitExceeded {
    self.exceeded
  }

  /// Returns the recursion depth that tripped the limit.
  #[inline(always)]
  pub const fn depth(&self) -> usize {
    self.exceeded.depth()
  }

  /// Returns the configured maximum depth.
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.exceeded.limitation()
  }

  /// Rewrites the offset — the shape a nested parse uses to lift an inner offset into its
  /// enclosing document's coordinates.
  #[inline(always)]
  pub fn map_offset<U, F>(self, f: F) -> RecursionLimitReached<U, Lang>
  where
    F: FnOnce(O) -> U,
  {
    RecursionLimitReached {
      at: f(self.at),
      exceeded: self.exceeded,
      _lang: PhantomData,
    }
  }
}

/// **Always** terminal: no amount of input clears a depth budget, so recovery must re-raise
/// rather than spend it.
impl<O, Lang: ?Sized> crate::error::MaybeTerminal for RecursionLimitReached<O, Lang> {
  #[inline(always)]
  fn is_terminal(&self) -> bool {
    true
  }
}

/// The unit error sink absorbs a trip like every other error, so a `()`-errored grammar still
/// drives the pratt engines. The terminal marker is lost with the value — the documented
/// [`MaybeTerminal`](crate::error::MaybeTerminal) opt-out, and `()` is never terminal.
///
/// A grammar that recovers therefore **spends** a trip it would otherwise re-raise. The stack is
/// already unwound by the time it does, so what the sink erases is the stop and not the bound; see
/// [the type's own section](RecursionLimitReached#a-discarding-sink-erases-the-stop-and-does-not-erase-the-bound)
/// for the measurements and for what a grammar that needs the stop should do instead.
impl<O, Lang: ?Sized> From<RecursionLimitReached<O, Lang>> for () {
  #[inline(always)]
  fn from(_: RecursionLimitReached<O, Lang>) -> Self {}
}
