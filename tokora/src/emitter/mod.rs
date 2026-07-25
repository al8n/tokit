use crate::{
  Lexer,
  error::{
    syntax::{FullContainer, TooFew, TooMany},
    token::UnexpectedTokenOf,
  },
  input::Cursor,
  span::Spanned,
};

use super::Token;

pub use cst::*;
pub use delimited::*;
pub use impl_::*;
pub use pratt::*;
pub use repeated::*;
pub use separated::*;
pub use severity::*;

mod cst;
mod delimited;
mod impl_;
mod pratt;
mod repeated;
mod separated;
mod severity;

/// A trait for handling and emitting errors during tokenization and parsing.
///
/// `Emitter` provides a unified interface for error handling in the tokenization pipeline.
/// Implementations can decide whether errors are fatal (stop processing) or non-fatal
/// (logged and processing continues). This is particularly useful when you want to collect
/// multiple errors before stopping, or when implementing error recovery.
///
/// # Atomically Composable Trait Design
///
/// Tokora's emitter system uses an **atomically composable trait design**. Instead of one monolithic
/// emitter interface, error handling is broken down into small, focused traits, each responsible for
/// a specific parsing scenario:
///
/// - **Core**: [`Emitter`] - Base error handling (lexer errors, unexpected tokens)
/// - **Repetition**: [`TooFewEmitter`], [`TooManyEmitter`], [`FullContainerEmitter`]
/// - **Separation**: [`SeparatedEmitter`], [`UnexpectedLeadingSeparatorEmitter`], [`UnexpectedTrailingSeparatorEmitter`]
///
/// This atomic design provides:
/// - ✅ **Fine-grained control**: Implement only the traits you need for your use case
/// - ✅ **Composability**: Mix and match traits to build custom error handling strategies
/// - ✅ **Pre-built bundles**: [`Fatal`], [`Verbose`], and [`Silent`] implement all traits with consistent behavior
/// - ✅ **Extensibility**: Create specialized emitters by implementing a subset of traits
///
/// Tokora provides several complete implementations: [`Fatal`], [`Verbose`],
/// [`Silent`], and [`Ignored`](crate::utils::marker::Ignored). However, the atomic trait system
/// encourages you to create custom emitters tailored to your specific needs by implementing only the
/// traits relevant to your parser.
///
/// # Error Handling Strategy
///
/// The emitter uses a `Result`-based approach where:
/// - `Ok(())` means the error was handled as non-fatal and processing should continue
/// - `Err(error)` means the error is fatal and processing should stop immediately
///
/// # Use Cases
///
/// - **Error Collection**: Accumulate multiple errors before reporting them all at once
/// - **Error Recovery**: Log errors but continue parsing to find more issues
/// - **Fail-Fast**: Stop on the first error by always returning `Err`
/// - **Filtering**: Only treat certain error types as fatal
/// - **Custom Strategies**: Implement domain-specific error handling (e.g., max error limits, severity filtering, telemetry)
///
/// # Example: Custom Emitter with Error Limit
///
/// ```ignore
/// use tokora::emitter::{Emitter, TooFewEmitter, TooManyEmitter};
///
/// struct MaxErrorsEmitter {
///     errors: Vec<String>,
///     max_errors: usize,
/// }
///
/// // Implement the core Emitter trait
/// impl<'a, L> Emitter<'a, L> for MaxErrorsEmitter {
///     type Error = String;
///
///     fn emit_lexer_error(&mut self, err: Spanned<...>) -> Result<(), Self::Error> {
///         self.errors.push(format!("Lexer error: {:?}", err));
///         if self.errors.len() >= self.max_errors {
///             Err("Too many errors".to_string())
///         } else {
///             Ok(())
///         }
///     }
///     // ... other Emitter methods
/// }
///
/// // Optionally implement atomic traits for specific error scenarios
/// impl<'a, O, L> TooFewEmitter<'a, O, L> for MaxErrorsEmitter {
///     fn emit_too_few(&mut self, err: TooFew<...>) -> Result<(), Self::Error> {
///         self.errors.push(format!("Too few elements: {:?}", err));
///         if self.errors.len() >= self.max_errors {
///             Err("Too many errors".to_string())
///         } else {
///             Ok(())
///         }
///     }
/// }
/// // Implement other atomic traits as needed: TooManyEmitter, SeparatedEmitter, etc.
/// ```
pub trait Emitter<'a, L, Lang: ?Sized = ()> {
  /// The error type that this emitter produces.
  ///
  /// This is the type returned when a fatal error occurs (via `Err(Self::Error)`).
  /// It can be any type that represents your application's error model.
  type Error;

  /// Emits a lexer error from the underlying Logos tokenizer.
  ///
  /// This method is called when Logos encounters an error during lexing (e.g.,
  /// invalid input that doesn't match any token pattern). The implementation
  /// decides whether to treat it as fatal or non-fatal.
  ///
  /// # Parameters
  ///
  /// - `err`: The lexer error wrapped with its source span
  ///
  /// # Returns
  ///
  /// - `Ok(())` if the error should be treated as non-fatal (processing continues)
  /// - `Err(Self::Error)` if the error is fatal (processing stops immediately)
  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>;

  /// Emits an unexpected token error encountered during parsing.
  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'a, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>;

  /// Emits a custom error from the application or parser.
  ///
  /// This method is called for application-level errors (not lexer errors).
  /// Like `emit_token_error`, the implementation decides whether the error
  /// is fatal or should be logged and processing continued.
  ///
  /// # Parameters
  ///
  /// - `err`: The application error wrapped with its source span
  ///
  /// # Returns
  ///
  /// - `Ok(())` if the error should be treated as non-fatal (processing continues)
  /// - `Err(Self::Error)` if the error is fatal (processing stops immediately)
  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>;

  /// Emits a warning — a diagnostic that, by contract, does not stop parsing.
  ///
  /// A warning carries the *same* payload as [`emit_error`](Self::emit_error) (a
  /// [`Spanned<Self::Error, _>`](Spanned)): a warning **is** a diagnostic, just one classified
  /// at the [`Severity::Warning`] tier rather than [`Severity::Error`]. This is an additive,
  /// second channel for future callers (e.g. a lossless collecting parse) — nothing in the
  /// existing emit paths reclassifies through it.
  ///
  /// Like the diagnostic-label capabilities, this is a method with a **blanket no-op default**:
  /// stateless emitters ([`Fatal`], [`Silent`], [`Ignored`](crate::utils::marker::Ignored))
  /// inherit the empty body — a fail-fast parse has no warning sink, so the warning is dropped
  /// and parsing continues (`Ok(())`). A collecting emitter like [`Verbose`] overrides this to
  /// record the warning into a channel parallel to its errors. The `Result` return is what lets
  /// a bespoke emitter escalate a warning to fatal if it wishes; the built-in emitters never do.
  ///
  /// # Contract: recorded warnings rewind with the log
  ///
  /// An implementation that *records* warnings must account for them in
  /// [`checkpoint`](Self::checkpoint) and drop them in [`rewind`](Self::rewind), exactly as it
  /// does for errors — one emission timeline across every channel. The rewind story is the
  /// reason: a speculative branch ([`attempt`](crate::InputRef::attempt), a
  /// [`Transaction`](crate::Transaction) rollback) unwinds *everything* it emitted, and a
  /// warning recorded outside the checkpointed state would survive the rollback as a phantom —
  /// attributed to a branch the parse abandoned. Nothing in the crate can detect that
  /// violation; it surfaces as wrong diagnostics, not as a panic. [`Verbose`] is the reference
  /// implementation (a shared, channel-tagged log; see its
  /// `fatal_ignores_warnings_but_errors_still_stop` and
  /// `verbose_warnings_collect_with_labels_parallel_to_errors` tests in `tests/emitter.rs`).
  #[inline(always)]
  fn emit_warning(&mut self, warning: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    let _ = warning;
    Ok(())
  }

  /// Emits the one-per-hole diagnostic for a region that balanced synchronization skipped:
  /// `span` covers the skipped region and `skipped` counts the tokens it dropped (see
  /// [`Hole`](crate::input::Hole) and [`sync_balanced`](crate::InputRef::sync_balanced)).
  ///
  /// Like [`emit_warning`](Self::emit_warning) and the diagnostic-label capabilities, this is
  /// an additive capability with a **blanket no-op default**: stateless emitters ([`Fatal`],
  /// [`Silent`], [`Ignored`](crate::utils::marker::Ignored)) inherit the empty body — a
  /// fail-fast parse keeps no record of recovery skips, so the note is dropped and parsing
  /// continues (`Ok(())`). A collecting emitter like [`Verbose`] overrides this to record the
  /// hole on its shared emission log, so a checkpoint rewind unwinds it together with the
  /// other diagnostics of the abandoned branch. The `Result` return lets a bespoke emitter
  /// reject a skip as fatal (e.g. a hole budget); the built-in emitters never do.
  ///
  /// # Contract: one call per hole, and recorded holes rewind with the log
  ///
  /// The *caller* side of the law lives on [`sync_balanced`](crate::InputRef::sync_balanced):
  /// the input layer calls this **exactly once per successful skip that dropped at least one
  /// token** — never per skipped token, never for a zero-skip sync, never for a failed sync
  /// (a limit trip or a no-match run emits no hole). An implementation may therefore count
  /// calls as holes. The *implementor* side mirrors
  /// [`emit_warning`](Self::emit_warning): a recorded hole must be covered by
  /// [`checkpoint`](Self::checkpoint)/[`rewind`](Self::rewind), because an enclosing rollback
  /// unwinds the skip it describes — a hole that survives its skip's rollback describes a
  /// recovery that never happened. The violation is undetectable by the crate (wrong
  /// diagnostics, no panic); the enforcing tests are
  /// `sync_balanced_hole_emission_unwinds_on_rollback` and
  /// `sync_balanced_trip_commits_prefix_without_hole_diagnostic` in
  /// `src/input/input_ref/tests.rs`.
  #[inline(always)]
  fn emit_skipped_region(&mut self, span: L::Span, skipped: usize) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    let _ = (span, skipped);
    Ok(())
  }

  /// Captures the emitter's current emission checkpoint for a later [`rewind`](Self::rewind).
  ///
  /// A checkpoint is a monotonically increasing emission mark: emitters that
  /// retain per-emission state (e.g. [`Verbose`]) return a value that grows with
  /// every recorded error, so a subsequent `rewind` can drop *exactly* the
  /// emissions made after this point. Stateless emitters ([`Fatal`], [`Silent`],
  /// [`Ignored`](crate::utils::marker::Ignored)) keep nothing to rewind and use
  /// the default `0`.
  #[inline(always)]
  fn checkpoint(&self) -> u64 {
    0
  }

  /// Rewinds the emitter state to a previously captured [`checkpoint`](Self::checkpoint).
  ///
  /// `checkpoint` is the mark returned by [`checkpoint`](Self::checkpoint) at the
  /// save point; `cursor` is the restore offset. Emission-aware emitters
  /// ([`Verbose`]) drop every diagnostic recorded after `checkpoint` — precisely
  /// the emissions of the abandoned branch, regardless of their span — and ignore
  /// `cursor`. `cursor` is retained for emitters that key their own rollback on
  /// the source offset. Stateless emitters ignore both.
  ///
  /// # Contract: rewind restores the emission state at the mark, across every channel
  ///
  /// The input layer pairs this with [`checkpoint`](Self::checkpoint) around every
  /// speculative region: a mark is captured at [`save`](crate::InputRef::save) time and handed
  /// back here when the branch is abandoned. Implementations must uphold, for any mark `m`
  /// previously returned by [`checkpoint`](Self::checkpoint):
  ///
  /// - **every channel rewinds** — after `rewind(_, m)`, the observable emission state (errors,
  ///   warnings, skipped-region records, and their label snapshots alike) is exactly what it
  ///   was when `m` was captured; recording a channel outside the mark leaks phantom
  ///   diagnostics into abandoned branches (see the channel contracts on
  ///   [`emit_warning`](Self::emit_warning) and
  ///   [`emit_skipped_region`](Self::emit_skipped_region));
  /// - **monotone marks** — [`checkpoint`](Self::checkpoint) never decreases as emissions are
  ///   recorded, so an older mark always names a prefix of a younger one and nested rewinds
  ///   unwind cleanly (`rewind` to the current mark is a no-op);
  /// - **rewind-only-backwards** — the input layer never hands in a mark from the future; a
  ///   defensive implementation may clamp (as [`Verbose`] does) rather than panic.
  ///
  /// Violations are not detectable by the crate — they surface as missing or phantom
  /// diagnostics after backtracking, not as panics. The enforcing tests for the reference
  /// implementation are the `restore_rewinds_verbose_errors_*` family in `tests/emitter.rs`
  /// and `sync_balanced_hole_emission_unwinds_on_rollback` in `src/input/input_ref/tests.rs`.
  fn rewind(&mut self, cursor: &Cursor<'a, '_, L>, checkpoint: u64)
  where
    L: Lexer<'a>;

  /// Releases a previously captured [`checkpoint`](Self::checkpoint) whose branch was
  /// **kept** — the dual of [`rewind`](Self::rewind), which spends one whose branch was
  /// abandoned.
  ///
  /// The input layer captures a mark around every speculative region and settles it in
  /// exactly one of two ways: a rollback hands it to [`rewind`](Self::rewind); a commit —
  /// a transaction guard's commit (explicit or on drop), an
  /// [`attempt`](crate::InputRef::attempt)/[`try_attempt`](crate::InputRef::try_attempt)
  /// success, a stacked guard's savepoint release, a session
  /// [`commit_point`](crate::InputRef::commit_point), a sync scan that found its target or
  /// otherwise kept its progress — hands it here. `release(m)` tells the emitter that `m`
  /// will **never** be rewound to by that settle, so any bookkeeping keyed on the mark can be
  /// reclaimed instead of stranding one dead row per committed guard — commit-heavy loops (a
  /// pratt operator loop saves per iteration) would otherwise grow such state without bound.
  ///
  /// The mark-keyed bookkeeping this exists for is the kind that lives at the **input layer's
  /// own seam**, where the settle discipline is 1:1 — one capture, one settle, right here. Do
  /// not read it as licence to key per-capture state inside a *wrapper*: a wrapper's inner
  /// emitter is settled by the wrapper's own rules, and the recording CST sink deliberately
  /// forwards no release at all (its inner must be a [`ValueKeyedEmitter`], which by
  /// definition has nothing to reclaim).
  ///
  /// # Contract: advisory, and strictly bookkeeping
  ///
  /// `release` is **advisory**: an emitter MAY reclaim per-checkpoint bookkeeping on it, and
  /// releasing must never change the observable emission state — no diagnostic appears,
  /// disappears, or reorders because a mark was released. Correctness must not depend on it
  /// being called: emitters that key their rollback state on emission *values* rather than
  /// on a per-mark table — [`Verbose`], whose mark is just its log length and whose rewind
  /// pops entries by value — legitimately inherit the no-op default, and stateless emitters
  /// ([`Fatal`], [`Silent`], [`Ignored`](crate::utils::marker::Ignored)) have nothing to
  /// reclaim in the first place. One mark is released at most once, and marks arrive
  /// last-in, first-out on the crate's own paths (guards and scans settle newest-first).
  /// Two settles are newest-first *within their own family* but may interleave the families,
  /// so they are not globally last-in, first-out: a session point abandoned with its handle
  /// releases at the handle's drop, and a guard's rollback-on-drop releases the savepoints it
  /// holds and then the session points it invalidates. Both can run **mid-unwind**, which is
  /// why releasing must stay observably pure and panic-free — and why no compliant emitter can
  /// observe the order in the first place: the end state is the same multiset either way.
  /// A mark abandoned outside those paths (an `unstable-raw` checkpoint merely dropped) is
  /// simply never released — bounded-but-unswept bookkeeping, reclaimed by the next
  /// enclosing `rewind`/`release` at or below it, per the crate's unspecified-but-bounded
  /// posture on undisciplined use.
  #[inline(always)]
  fn release(&mut self, checkpoint: u64) {
    let _ = checkpoint;
  }

  /// Observes one **committed token** — the settle-timeline twin of
  /// [`release`](Self::release): the input layer calls this exactly once per token, at the
  /// moment the token settles (consumed, or skipped behind a scan frontier), from its two
  /// censused settle surfaces and nowhere else.
  ///
  /// This is the auto-emission chokepoint of the CST channel: the recording `Sink`
  /// overrides it to record a `Token` event (mapping the token through its dialect-supplied
  /// mapper), which is what makes **every** consuming atom and combinator tree-producing
  /// with zero per-atom code — including the tokens a recovery sync or a trivia skip
  /// consumes. Peeks, declines, [`unconsume`](crate::InputRef)-style stoppers, position
  /// writes, rejected lexer-error items, and end-of-input commits are **not** token
  /// settles and never arrive here.
  ///
  /// # Contract: bookkeeping on the emission timeline
  ///
  /// Like [`release`](Self::release), this is a defaulted no-op capability — diagnostics
  /// emitters have nothing to observe, and the reference-taking signature makes the
  /// defaulted call compile to nothing. An implementation that *records* settles must
  /// cover them with [`checkpoint`](Self::checkpoint)/[`rewind`](Self::rewind) exactly as
  /// it covers diagnostics — a speculative branch's tokens rewind with the branch, and a
  /// re-consume after a rollback re-settles them exactly once per surviving timeline. A
  /// **wrapper** emitter that records or forwards events must forward this too; the
  /// tree-structuring surface lives on the [`CstEmitter`] subtrait, whose bound on the
  /// `node()` combinators is what stops a non-forwarding wrapper from silently producing
  /// an empty tree. Forwarding the subtrait while inheriting this no-op severs the token
  /// channel invisibly at parse time; the recording sink's `finish` refuses that shape
  /// with a typed error (`cst::FinishError::StructureWithoutTokens`)
  /// rather than materializing a plausible gap-tiled tree.
  #[inline(always)]
  fn commit_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'a>,
  {
    let _ = (tok, span);
  }

  /// Pushes a diagnostic label onto the emitter's open-label stack, opening a
  /// *"while parsing X"* context for the duration of a [`labelled`](crate::labelled)
  /// sub-parse.
  ///
  /// This is an additive capability with a **blanket no-op default**: stateless
  /// emitters ([`Fatal`], [`Silent`], [`Ignored`](crate::utils::marker::Ignored))
  /// inherit the empty body, so a label pair around them costs nothing — the two
  /// calls inline away. A collecting emitter like [`Verbose`] overrides this to
  /// maintain the stack and snapshot it into every diagnostic it records.
  ///
  /// Labels are `&'static str` (parser names are static), so a push never allocates.
  ///
  /// # Contract: enter/exit arrive strictly nested
  ///
  /// Every `enter_label` is paired with exactly one [`exit_label`](Self::exit_label), and the
  /// pairs nest — the sequence an implementation observes is a well-bracketed push/pop
  /// stream. [`labelled`](crate::labelled) guarantees this by construction: it brackets the
  /// sub-parse and pops on **both** the success and error paths, so the law holds even when
  /// the inner parser's failure propagates out through `?`. No label state needs to ride a
  /// checkpoint, because the live stack follows the call structure of the wrapper scopes and
  /// a rewound emission takes its captured snapshot with it (the rewind story; see
  /// `labelled_guard_rollback_drops_labels_then_reemission_rederives` in `tests/emitter.rs`
  /// and `verbose_nested_labels_snapshot_outer_then_outer_and_inner_then_outer` in the
  /// [`Verbose`] tests). A hand-rolled, unbalanced call — an exit without its enter, or an
  /// enter never exited — is not detected: nothing panics, but every later snapshot carries
  /// the drifted context, so diagnostics are attributed to the wrong *"while parsing X"*
  /// scope. Route labels through [`labelled`](crate::labelled) rather than calling this pair
  /// directly.
  #[inline(always)]
  fn enter_label(&mut self, label: &'static str) {
    let _ = label;
  }

  /// Pops the most recently [`enter_label`](Self::enter_label)ed label as its
  /// [`labelled`](crate::labelled) scope closes.
  ///
  /// No-op by default (see [`enter_label`](Self::enter_label)); [`Verbose`] overrides
  /// it to pop its open-label stack. The stack therefore follows the call structure of
  /// the `labelled` wrappers exactly, so a checkpoint restore needs no label handling —
  /// no label state lives outside the wrapper scopes and the recorded log entries.
  #[inline(always)]
  fn exit_label(&mut self) {}
}

impl<'a, L, U, Lang: ?Sized> Emitter<'a, L, Lang> for &mut U
where
  U: Emitter<'a, L, Lang>,
{
  type Error = U::Error;

  #[inline(always)]
  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    (**self).emit_lexer_error(err)
  }

  #[inline(always)]
  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'a, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    (**self).emit_unexpected_token(err)
  }

  #[inline(always)]
  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    (**self).emit_error(err)
  }

  #[inline(always)]
  fn emit_warning(&mut self, warning: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    (**self).emit_warning(warning)
  }

  #[inline(always)]
  fn emit_skipped_region(&mut self, span: L::Span, skipped: usize) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    (**self).emit_skipped_region(span, skipped)
  }

  #[inline(always)]
  fn checkpoint(&self) -> u64 {
    (**self).checkpoint()
  }

  #[inline(always)]
  fn rewind(&mut self, cursor: &Cursor<'a, '_, L>, checkpoint: u64)
  where
    L: Lexer<'a>,
  {
    (**self).rewind(cursor, checkpoint)
  }

  #[inline(always)]
  fn release(&mut self, checkpoint: u64) {
    (**self).release(checkpoint)
  }

  #[inline(always)]
  fn commit_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'a>,
  {
    (**self).commit_token(tok, span)
  }

  #[inline(always)]
  fn enter_label(&mut self, label: &'static str) {
    (**self).enter_label(label)
  }

  #[inline(always)]
  fn exit_label(&mut self) {
    (**self).exit_label()
  }
}

/// Marks an [`Emitter`] whose checkpoint reading is a **value**, not a key into a table.
///
/// [`checkpoint`](Emitter::checkpoint) returns a `u64`, and two very different emitters satisfy
/// that signature:
///
/// - **value-keyed** — the reading *is* a fact about the emission state and nothing else exists:
///   [`Verbose`]'s mark is its log length, a token tracker's is its count, a stateless emitter's
///   is the constant `0`. [`rewind`](Emitter::rewind) restores by value — reclaiming everything
///   above the mark as a range — and [`release`](Emitter::release) has nothing to reclaim.
/// - **table-keyed** — the reading is a *key* into per-capture bookkeeping the emitter allocated
///   at [`checkpoint`](Emitter::checkpoint) time and expects to reclaim at the matching settle.
///
/// This marker asserts the former. It is a semantic promise the type system cannot check:
///
/// > **Equal emission state implies equal reading.** The reading is a function of the emission
/// > *state*, never of the *call history*: capturing a mark does not change what a later capture
/// > returns, and two captures with no emission between them read the same value.
///
/// That is strictly stronger than "a pure monotone reading" — a per-call counter is monotone and
/// allocates nothing, yet reads differently on its second call, which the promise excludes.
///
/// # Why the promise is load bearing
///
/// The recording CST sink requires it for **correctness**, not tidiness. It keys one mark-stack
/// row per live capture on the event-log length, so captures taken with no events between them
/// are *aliases*: same mark, same frozen depth, same inner reading. Its settles remove "the
/// newest row at this mark", which is exact only because aliased rows are indistinguishable —
/// removing any one of them leaves the same multiset of live rows. An inner whose reading varied
/// per capture would break that algebra: the rows would differ, and spending the wrong one hands
/// a later rewind an inner target that never described the surviving log. Separately, the sink's
/// [`release`](Emitter::release) deliberately forwards nothing, so a table-keyed inner would
/// strand one allocated row per committed capture with nothing able to reclaim it.
///
/// # Implementors
///
/// Every emitter this crate offers: [`Verbose`], [`Fatal`], [`Silent`],
/// [`Ignored`](crate::utils::marker::Ignored), and `&mut U` for value-keyed `U`, so a borrowed
/// emitter composes exactly like an owned one. Implement it for your own emitter if — and only
/// if — it keeps the promise above.
///
/// The recording `Sink` is deliberately **not** an implementor: its own
/// [`checkpoint`](Emitter::checkpoint) pushes a row it expects a later settle to reclaim, which
/// is precisely the table-keyed shape. A sink wrapping a sink is therefore a compile error rather
/// than a configuration that strands a row per capture.
pub trait ValueKeyedEmitter {}

// The roster, in one place so "which emitters may sit inside the sink" is a single legible list
// rather than a property scattered across the impl modules.
#[cfg(any(feature = "std", feature = "alloc"))]
impl<Error, S, Lang: ?Sized> ValueKeyedEmitter for Verbose<Error, S, Lang> {}
impl<T: ?Sized, Lang: ?Sized> ValueKeyedEmitter for Fatal<T, Lang> {}
impl<T: ?Sized, Lang: ?Sized> ValueKeyedEmitter for Silent<T, Lang> {}
impl<T: ?Sized> ValueKeyedEmitter for crate::utils::marker::Ignored<T> {}

/// Forwarded so a **borrowed** emitter is as composable as an owned one: `Emitter` is
/// implemented for `&mut U`, and without this a value-keyed emitter would lose the property the
/// moment it was passed by reference.
impl<U: ValueKeyedEmitter> ValueKeyedEmitter for &mut U {}

/// A trait bound for generic emitter error conversion.
pub trait FromEmitterError<'a, L, Lang: ?Sized = ()> {
  /// Creates an emitter error from a lexer error.
  fn from_lexer_error(err: Spanned<<L::Token as Token<'a>>::Error, L::Span>) -> Self
  where
    L: Lexer<'a>;

  /// Creates an emitter error from an unexpected token error.
  fn from_unexpected_token(err: UnexpectedTokenOf<'a, L, Lang>) -> Self
  where
    L: Lexer<'a>;
}

impl<'a, T, L, Lang: ?Sized> FromEmitterError<'a, L, Lang> for T
where
  L: Lexer<'a>,
  T: From<<L::Token as Token<'a>>::Error> + From<UnexpectedTokenOf<'a, L, Lang>>,
{
  #[inline(always)]
  fn from_lexer_error(err: Spanned<<L::Token as Token<'a>>::Error, L::Span>) -> Self
  where
    L: Lexer<'a>,
  {
    err.into_data().into()
  }

  #[inline(always)]
  fn from_unexpected_token(err: UnexpectedTokenOf<'a, L, Lang>) -> Self
  where
    L: Lexer<'a>,
  {
    err.into()
  }
}

/// Everything the collecting combinators require from an emitter, as one bound.
///
/// The atomic emitter design splits diagnostics across a family of focused traits, and a
/// parser that drives the separated/repeated machinery ends up needing most of them at
/// once — a seven-line where-clause ladder repeated at every generic parser an author
/// writes. `ComposableEmitter` is that ladder as a single name: it is
/// blanket-implemented for every emitter satisfying the whole family, so a bound of
/// `E: ComposableEmitter<'inp, L, Lang>` is interchangeable with spelling out all seven
/// sub-traits. The pre-built [`Fatal`], [`Verbose`], and [`Silent`] emitters all qualify
/// whenever their error type absorbs the corresponding error families.
///
/// The context-side twin is [`ParseCtx`](crate::ParseCtx), which rides this bound on a
/// [`ParseContext`](crate::ParseContext)'s emitter so a whole parsing context collapses
/// to one bound as well.
///
/// # Examples
///
/// The bundle elaborates back to the entire family — the one-bound function can call
/// into the seven-bound ladder:
///
/// ```rust
/// use tokora::{ComposableEmitter, Emitter, Lexer};
/// use tokora::emitter::{
///   FullContainerEmitter, SeparatedEmitter, TooFewEmitter, UnexpectedLeadingSeparatorEmitter,
///   UnexpectedTrailingSeparatorEmitter, UnclosedEmitter,
/// };
///
/// // The where-clause ladder an atom would otherwise carry…
/// fn ladder<'inp, L, E>()
/// where
///   L: Lexer<'inp>,
///   E: Emitter<'inp, L>
///     + FullContainerEmitter<'inp, L>
///     + SeparatedEmitter<'inp, L>
///     + UnexpectedLeadingSeparatorEmitter<'inp, L>
///     + UnexpectedTrailingSeparatorEmitter<'inp, L>
///     + TooFewEmitter<'inp, L>
///     + UnclosedEmitter<'inp, L>,
/// {
/// }
///
/// // …collapses to a single bound that still unlocks every rung.
/// fn bundled<'inp, L, E>()
/// where
///   L: Lexer<'inp>,
///   E: ComposableEmitter<'inp, L>,
/// {
///   ladder::<L, E>()
/// }
/// ```
pub trait ComposableEmitter<'inp, L, Lang: ?Sized = ()>:
  Emitter<'inp, L, Lang>
  + FullContainerEmitter<'inp, L, Lang>
  + SeparatedEmitter<'inp, L, Lang>
  + UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>
  + UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>
  + TooFewEmitter<'inp, L, Lang>
  + UnclosedEmitter<'inp, L, Lang>
where
  L: Lexer<'inp>,
{
}

impl<'inp, L, Lang: ?Sized, T> ComposableEmitter<'inp, L, Lang> for T
where
  L: Lexer<'inp>,
  T: Emitter<'inp, L, Lang>
    + FullContainerEmitter<'inp, L, Lang>
    + SeparatedEmitter<'inp, L, Lang>
    + UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>
    + UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>
    + TooFewEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>,
{
}
