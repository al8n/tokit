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
/// read.
///
/// # The stop does not travel in the payload, so a discarding sink cannot drop it
///
/// The terminality above is a property of *this* type — and a grammar's error type is allowed to
/// discard this type. `()` does; so does any `From` that drops the value rather than delegating.
/// Were the stop carried only here, a resource bound would be something an unrelated error sink
/// could opt out of, and it was: until issue #148 a `()`-errored grammar **spent** the trip,
/// synthesized a node for a construct it was forbidden to read, and let `skip_then_retry` commit
/// skipped input buying progress that cannot help.
///
/// It is stored on the **input session** instead. The trip arm of
/// [`InputRef::descend`](crate::InputRef::descend) counts the trip on `Input::resource_trips` —
/// monotone, outside the rollback set, with no writer that lowers it — before it builds anything
/// the grammar can see. The three combinators above read that counter beside `is_terminal()`, and
/// the resilient collection loops read it on its own (they carry no `MaybeTerminal` bound). Each
/// reads it *relative to the attempt it is judging*, by snapshotting it before that attempt and
/// comparing after; the cell records the session fact, the comparison asks the per-attempt one. So
/// the sink loses the *value* and every supported sink observes the same *stop*. A grammar that
/// wants the payload (the offset, the depth, the limitation) must still store it; a grammar that
/// only wants to not-recover gets that for free.
///
/// What was never at risk, and is still measured rather than assumed:
///
/// * **The native stack is already back.** [`Descent`](crate::input::Descent)'s destructor
///   releases its level on the unwind that carries this error out, so by the time the error
///   surfaces anywhere outside the engine, every frame the budget was protecting has returned. On
///   a 200-level trip the surfacing frame sits 48 bytes from the pre-parse baseline in a debug
///   build and 0 in a release one, against a descent that reached ~1 MiB and ~97 KiB
///   respectively.
/// * **The budget is unchanged.** The depth cell reads back to what it was before the parse. A
///   caller that catches the error itself and descends again starts from the same depth and meets
///   the same limit; re-tripping is bounded, not compounding.
///
/// `tokora/tests/pratt_limit_unit_sink.rs` drives every one of these through `()` and pins each
/// against its delegating counterpart in `tokora/tests/pratt_limit.rs`. The two files agreeing is
/// the property; they disagreed before #148, and the cells that recorded the disagreement were
/// inverted rather than deleted.
///
/// **A grammar that needs the trip's *details* must still store this value.** Give the grammar's
/// error type a variant that holds it and delegate `is_terminal`:
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
/// parser drive the Pratt engines without declaring an error type at all — and it now means what
/// it says and no more: that no error carries *information*, not that no error carries a stop.
///
/// # Both trips latch; they latch different kinds of fact
///
/// A scanner limit trip latches the input's poison boundary, because the lexer's tally is monotone
/// in the input. A descent trip counts on `resource_trips` for the same reason at one remove:
/// *that a budget was exceeded* is monotone even though the depth is not. The **depth** is the fact
/// that
/// is not latched and must not be — it is scoped to the control stack and fully restored by the
/// unwind itself, since every live frame's guard decrements as it pops. So a driver handed this
/// error finds the depth cell exactly as it was and the session marked as having tripped.
///
/// The asymmetry that remains is *what* each latch records: a scanner trip has a position, so it
/// latches **where**, and lexing stops crossing it; a descent trip has only a control stack, so it
/// latches **whether**, and it stops recovery rather than lexing. Neither can be un-latched.
///
/// # Two consequences, stated rather than left to be discovered
///
/// **What the sites test is a per-attempt transition, not the session fact.** The cell is a
/// monotone count and is never cleared, so *reading it absolutely* would mean that once anything
/// catches a trip and parses on, every later recovery and every later collection swallow in the
/// session is refused — ordinary syntax errors that have nothing to do with the budget included,
/// which a terminality-preserving error type would have recovered, since the later error is a
/// different value. That is why each site snapshots the counter before the attempt it judges and
/// asks whether it moved *during* it. A budget trip is still terminal wherever one actually
/// happens, a second one after a caught first included; what the comparison rules out is charging
/// an unrelated failure with a stop it did not cause. It still fails closed at the boundary that
/// matters: it never synthesizes over a real trip.
///
/// **The resilient collection loops re-raise a trip on three exits, not on every path through an
/// element.** `repeated`, `separated` and their delimited forms swallow an element's `Err` by
/// design — emit it as a diagnostic and keep looping — but a descent trip is not spent that way
/// through any of three exits: an element's own `Err` (the gate re-raises it there alongside a
/// frontier `Incomplete` and the *scanner*'s committed boundary), the element declining (or a cycle
/// making no progress), and a real closer committed just after either of those. A trip that reaches
/// one of those three ends the collection on the `Err` channel, or is refused as the absence it
/// would otherwise be read as, instead of yielding a truncated container with the trip filed among
/// its diagnostics. That was the behaviour before #148, and it was never sink-dependent: a
/// delegating error type was spent there exactly as `()` was. The baseline is per *element*, so an
/// ordinary failure later in the same collection — or in the enclosing one, where an inner
/// collection's trip was swallowed — is emitted and looped past as it always was. See
/// `parser::many`'s `GATE_CENSUS` for how the three exits stay wired.
///
/// **A fourth path is exempt, deliberately and permanently: an element that catches the trip, still
/// consumes, and still answers `Accept`.** The three exits above are gated because the driver is
/// the one concluding the construct ended, manufacturing that conclusion from a stop the caller
/// never learns about, so it has to guard its own inference. `Accept` is not that: the element
/// produced the value, and the driver is faithfully collecting what it was handed, not concluding
/// anything of its own. None of the three runs, and the value is collected past exactly as it would
/// be from an element the budget never touched. See `parser::many`'s module docs, "the channel
/// neither chokepoint closes" section, for the reasoning at length, and
/// `tokora/tests/collection_resource_trip.rs`'s section 6 for the pin.
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
/// drives the pratt engines. The **payload** is lost with the value — the offset, the depth and
/// the limitation — and `()` is never terminal, so the
/// [`MaybeTerminal`](crate::error::MaybeTerminal) gate reads `false` on the converted value.
///
/// The **stop** is not lost. It does not travel here: the trip latches the input session before
/// this conversion runs, and recovery reads that latch beside `is_terminal()`, so a `()`-errored
/// grammar re-raises a trip exactly as a delegating one does. See
/// [the type's own section](RecursionLimitReached#the-stop-does-not-travel-in-the-payload-so-a-discarding-sink-cannot-drop-it)
/// for the measurements and for what a grammar that needs the *details* should do instead.
impl<O, Lang: ?Sized> From<RecursionLimitReached<O, Lang>> for () {
  #[inline(always)]
  fn from(_: RecursionLimitReached<O, Lang>) -> Self {}
}
