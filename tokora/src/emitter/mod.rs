use crate::{
  Lexer,
  error::{
    UnexpectedEot,
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
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " - ✅ **Pre-built bundles**: [`Fatal`], [`Verbose`], and [`Silent`] implement all traits with consistent behavior"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " - ✅ **Pre-built bundles**: [`Fatal`], `Verbose`, and [`Silent`] implement all traits with consistent behavior"
)]
/// - ✅ **Extensibility**: Create specialized emitters by implementing a subset of traits
///
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " Tokora provides several complete implementations: [`Fatal`], [`Verbose`],"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " Tokora provides several complete implementations: [`Fatal`], `Verbose`,"
)]
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
#[diagnostic::on_unimplemented(
  message = "`{Self}` is not a tokora emitter for lexer `{L}`",
  label = "this type has no `Emitter` impl, so it can neither report nor absorb diagnostics",
  note = "implement `Emitter` (usually alongside the other members of the `ComposableEmitter` bundle)"
)]
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
  ///
  /// # Under a rejecting emitter, the `Err` return *is* the delivery
  ///
  /// Worth stating because the natural reading is the opposite one — that returning `Err`
  /// declines to report and hands the diagnostic to the caller instead.
  ///
  /// The input layer dedups lexer errors against a watermark, and it advances that watermark
  /// **before** calling this method, not after. So by the time an implementation decides to
  /// return `Err`, the region is already marked as reported: a later scan over the same bytes
  /// will not offer the diagnostic again. There is exactly one delivery, and for a rejecting
  /// emitter it is the `Err` itself. An implementation that returns `Err` *and* expects to see
  /// the same error again on a re-lex will not.
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " and parsing continues (`Ok(())`). A collecting emitter like [`Verbose`] overrides this to"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " and parsing continues (`Ok(())`). A collecting emitter like `Verbose` overrides this to"
  )]
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " violation; it surfaces as wrong diagnostics, not as a panic. [`Verbose`] is the reference"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " violation; it surfaces as wrong diagnostics, not as a panic. `Verbose` is the reference"
  )]
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " continues (`Ok(())`). A collecting emitter like [`Verbose`] overrides this to record the"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " continues (`Ok(())`). A collecting emitter like `Verbose` overrides this to record the"
  )]
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " retain per-emission state (e.g. [`Verbose`]) return a value that grows with"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " retain per-emission state (e.g. `Verbose`) return a value that grows with"
  )]
  /// every recorded error, so a subsequent `rewind` can drop *exactly* the
  /// emissions made after this point. Stateless emitters ([`Fatal`], [`Silent`],
  /// [`Ignored`](crate::utils::marker::Ignored)) keep nothing to rewind and use
  /// the default `0`.
  ///
  /// # Contract: `checkpoint` is fail-atomic, and a returned mark is settled exactly once
  ///
  /// If this call unwinds, the emitter's observable state and any per-mark bookkeeping must be
  /// as if it never happened. It takes `&self` — registering per-mark state therefore requires
  /// interior mutability, which is precisely the implementation opting into managing its own
  /// atomicity: **reserve before you publish**. Grow your storage first, while nothing is
  /// registered; then write into the reserved capacity, which cannot unwind. That is also why
  /// this trait has no `reserve_checkpoint` hook: everything such a hook could do is expressible
  /// inside this body, and the input layer already orders its own capture so that nothing
  /// crate-side is pending when this is called — a hook would add a second method and no new
  /// capability, on a trait every wrapper must remember to forward.
  ///
  /// A mark that *is* returned is settled exactly once — by [`rewind`](Self::rewind) if the
  /// branch was abandoned, by [`release`](Self::release) if it was kept — and a call that
  /// unwinds owes nothing.
  ///
  /// **Residue, stated plainly:** an emitter that registers internal state and unwinds
  /// mid-`checkpoint` still strands its own state; nothing in the crate can release what was
  /// never returned to it. This clause is what tells you not to — uphold it and there is
  /// nothing to strand. The recording CST sink's `checkpoint` is the reference implementation:
  /// it reads, and registers nothing that a later settle must find.
  ///
  /// # Which operations take a mark is **not** part of the contract
  ///
  /// What the crate promises is how a mark it *does* take is settled — exactly once, by
  /// [`rewind`](Self::rewind) or [`release`](Self::release), newest-first within its family. It
  /// does **not** promise how many marks a given parse takes, nor which internal operation takes
  /// them. An input path may take a checkpoint/release cycle it turns out not to need, and may
  /// equally skip one: [`skip_while`](crate::InputRef::skip_while) answers a skip that has nothing
  /// to skip without entering the scanner at all, so under
  /// [`Partial`](crate::input::Partial) it performs *neither half* of the empty cycle the scan
  /// would have run.
  ///
  /// Nothing conforming can depend on either choice. Such a cycle is **empty** — no emission is
  /// made between its two halves — and **balanced**, so the outstanding-mark count is the same
  /// whether it ran or not; and `release` is advisory in the first place. An emitter that keys
  /// behaviour on the **number** or the **identity** of the marks a parse takes is outside this
  /// contract, and different versions of this crate may legitimately differ.
  ///
  /// The positive form of that, and the one to build against: **make this method inert** — let it
  /// do only what this contract says it does (return a mark, and register whatever bookkeeping the
  /// matching settle will consume), and always return normally. An inert `checkpoint` cannot tell
  /// whether an input path took a cycle it did not need, so every such choice is free. That is the
  /// same rule [`skip_while`](crate::InputRef::skip_while) and
  /// [`peek_head_map`](crate::InputRef::peek_head_map) state as a condition on the caller, seen
  /// from the end that implements it.
  ///
  /// **Inert is the whole requirement**; what follows is not the set of ways to miss it. Two that
  /// come up, and what each costs:
  ///
  /// * **it counts.** Harmless while the count stays inside the emitter. The moment it is shared
  ///   with a **predicate or closure the same caller supplies** — a `skip_while` predicate, a
  ///   `peek_head_map` `f` — the count leaks into a decision, and a mark this crate did or did not
  ///   take stops being a private matter: it becomes a different skip, a different committed
  ///   cursor, different tokens consumed;
  /// * **it unwinds.** The fail-atomicity clause above *permits* that, and keeps the emitter's own
  ///   state sound when it happens — but it does not make the unwind invisible, because whether
  ///   this method is called at all is a route the crate may change. `skip_while` over a resident
  ///   head that has nothing to skip returns `Ok(())` where the scan it replaces would have
  ///   propagated your panic. The two clauses live at different levels and do not conflict: this
  ///   one is about the emitter's state after an unwind, that one is about the parse. If your
  ///   `checkpoint` has a reachable panic path, the value guarantees on those two methods are not
  ///   made to you.
  ///
  /// Keep an emitter's behaviour a function of what is *emitted* — not of how many marks arrived,
  /// and not of whether taking one could fail — and the question never comes up, including for the
  /// third way of losing inertness that nobody has thought of yet. The requirement is the one
  /// sentence, not the two bullets under it.
  #[inline(always)]
  fn checkpoint(&self) -> u64 {
    0
  }

  /// Rewinds the emitter state to a previously captured [`checkpoint`](Self::checkpoint).
  ///
  /// `checkpoint` is the mark returned by [`checkpoint`](Self::checkpoint) at the
  /// save point; `cursor` is the restore offset. Emission-aware emitters
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " ([`Verbose`]) drop every diagnostic recorded after `checkpoint` — precisely"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " (`Verbose`) drop every diagnostic recorded after `checkpoint` — precisely"
  )]
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = "   defensive implementation may clamp (as [`Verbose`] does) rather than panic."
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = "   defensive implementation may clamp (as `Verbose` does) rather than panic."
  )]
  ///
  /// Violations are not detectable by the crate — they surface as missing or phantom
  /// diagnostics after backtracking, not as panics. The enforcing tests for the reference
  /// implementation are the `restore_rewinds_verbose_errors_*` family in `tests/emitter.rs`
  /// and `sync_balanced_hole_emission_unwinds_on_rollback` in `src/input/input_ref/tests.rs`.
  ///
  /// # Contract: this may run mid-unwind, and must not panic *there*
  ///
  /// A rolling-back guard settles in its `Drop`, and a `Drop` can run while a panic is already
  /// in flight — including for a [`Commit`](crate::Commit)-policy guard, which takes the
  /// rollback arm on the unwind edge. A panic raised here mid-unwind is a double panic and
  /// aborts the process. The clause [`release`](Self::release) already carries applies in full:
  /// stay observably total on the restore path. The built-in emitters are structurally
  /// non-panicking there (truncations and pops); the crate's rollback-on-drop guarantee is
  /// conditional on yours being too.
  ///
  /// The property that actually has to hold is **"never abort the process"**, and it binds
  /// only while an unwind is in progress. An implementation that can *detect* a settle
  /// violation — a mark it never issued, or one already spent — MAY panic to report it,
  /// provided it first checks `std::thread::panicking` and stays silent when that is true.
  /// A detected unpaired settle is a caller bug with no correct recovery, and reporting it at
  /// the cause beats degrading into a state the caller cannot distinguish from success; a
  /// double-panic abort would be worse than either. The recording CST `Sink` is the one
  /// in-crate implementation that takes this door (the `rowan` feature), and its
  /// `rewind`'s `# Panics` section states exactly which condition and why. An emitter with
  /// nothing to detect — every built-in one — simply inherits the stricter "never panics"
  /// reading, unchanged.
  ///
  /// Two riders come with that door, and neither is optional.
  ///
  /// - **Report before you mutate.** The panic is a wall, not a narration: raise it from a
  ///   preflight over unchanged state, so a caller that catches it is left holding the emitter
  ///   it had rather than a half-rewound one. A violation detected after the rewind has already
  ///   truncated its own log is a violation announced *from inside* the corruption it names.
  /// - **A suppressed report must still be recorded.** Silence while unwinding is a licence to
  ///   avoid the abort, not a licence to degrade invisibly: the emitter must latch the fact and
  ///   refuse — as a typed error, not a later panic — at whatever surface hands its accumulated
  ///   output onward. Otherwise a caller that catches the *original* panic can go on to collect
  ///   a result that is indistinguishable from one no violation ever touched.
  ///
  /// **The first rider buys the emitter, and only the emitter.** A compliant preflight leaves
  /// *your* state exactly as the caller left it; it does not make the crate's rollback whole.
  /// This method is called from the middle of an [`InputRef`](crate::InputRef) checkpoint
  /// restore — below the point where that restore has already popped the target off its live
  /// checkpoint lineage and released the marks of any session points open above it, and above
  /// the point where it installs the restored position, the error-reporting witnesses and the
  /// cache-push counter. A report raised here therefore tears that restore in half whatever the
  /// emitter does: the input handle is left on neither branch. That is inside the
  /// unspecified-but-bounded envelope `InputRef::restore` documents (the `unstable-raw` valve),
  /// and it is the price of being told at all. A host that catches the report must treat the
  /// **parse** as over rather than resume on that handle — what survives is the emitter and its
  /// accumulated output, which is what the second rider exists to keep honest.
  ///
  /// The condition an implementation is allowed to report this way is a caller bug, never an
  /// input-dependent one. Detecting something a malformed document can provoke and panicking on
  /// it is outside this permission entirely.
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " [`commit_point`](crate::InputRef::commit_point), a sync scan that found its target or"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " `commit_point`, a sync scan that found its target or"
  )]
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " on a per-mark table — [`Verbose`], whose mark is just its log length and whose rewind"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " on a per-mark table — `Verbose`, whose mark is just its log length and whose rewind"
  )]
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " calls inline away. A collecting emitter like [`Verbose`] overrides this to"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " calls inline away. A collecting emitter like `Verbose` overrides this to"
  )]
  /// maintain the stack and snapshot it into every diagnostic it records.
  ///
  /// Labels are `&'static str` (parser names are static), so a push never allocates.
  ///
  /// # Contract: enter/exit arrive strictly nested
  ///
  /// Every `enter_label` is paired with exactly one [`exit_label`](Self::exit_label), and the
  /// pairs nest — the sequence an implementation observes is a well-bracketed push/pop
  /// stream. [`labelled`](crate::labelled) guarantees this by construction: the pop lives in a
  /// drop guard, so it runs on **every** exit of the sub-parse — success, `?`-propagation, and
  /// panic unwind alike. No label state needs to ride a
  /// checkpoint, because the live stack follows the call structure of the wrapper scopes and
  /// a rewound emission takes its captured snapshot with it (the rewind story; see
  /// `labelled_guard_rollback_drops_labels_then_reemission_rederives` in `tests/emitter.rs`
  /// and `verbose_nested_labels_snapshot_outer_then_outer_and_inner_then_outer` in the
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " [`Verbose`] tests). A hand-rolled, unbalanced call — an exit without its enter, or an"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " `Verbose` tests). A hand-rolled, unbalanced call — an exit without its enter, or an"
  )]
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
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = " No-op by default (see [`enter_label`](Self::enter_label)); [`Verbose`] overrides"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = " No-op by default (see [`enter_label`](Self::enter_label)); `Verbose` overrides"
  )]
  /// it to pop its open-label stack. The stack therefore follows the call structure of
  /// the `labelled` wrappers exactly, so a checkpoint restore needs no label handling —
  /// no label state lives outside the wrapper scopes and the recorded log entries.
  ///
  /// # Contract: this may arrive from a drop during a panic unwind
  ///
  /// [`labelled`](crate::labelled)'s pop is a drop-guard call — that is how the pairing law
  /// survives an unwind through the sub-parse — so this can run while a panic is in flight and
  /// must not panic there. A panic raised mid-unwind aborts the process; same posture as
  /// [`release`](Self::release) and [`rewind`](Self::rewind).
  #[inline(always)]
  fn exit_label(&mut self) {}

  /// The source this emitter is **bound to**, if it is bound to one.
  ///
  /// `None` — the default — means *"I bind no source"*, which is the true answer for every
  /// emitter that only carries diagnostics. It is not a placeholder for a missing answer: a
  /// trait member may be defaulted exactly when its default is a true statement about the
  /// majority implementor, and this one is true for every emitter in this crate except the
  /// CST `Sink` (the `rowan` feature).
  ///
  /// # The law
  ///
  /// **The emitter and the parse must observe the same source *value*, not equal bytes, and the
  /// emitter's extent must cover the parse's.** An emitter that stores a `&'inp L::Source` and
  /// slices it later — a recording CST sink is the only one in this crate — answers
  /// `Some(SourceIdentity::of(that_source))`. The point at which an emitter is attached to an
  /// input then compares the two, and **panics** if they disagree *and* the backing can prove
  /// they disagree ([`Source::REFERENT_IS_BYTES`](crate::Source::REFERENT_IS_BYTES)).
  ///
  /// Two relations, because there are two failures and they are not symmetric — see
  /// [`SourceIdentity::covers`](crate::source::SourceIdentity::covers). Same origin is an
  /// equality. Extent is an ordering: a sink bound to a **longer** slice than the parse reads
  /// is the fixed-arena streaming shape and is fine, while one bound to a **shorter** slice
  /// lets peeked bytes past its end shape a tree that will not contain them.
  ///
  /// The check has to sit there, and not at materialization: a sink built over `"XY"` and
  /// driven over a same-length `"ab"` produces an event log **byte-identical** to one a legal
  /// single parse could produce, so nothing downstream has anything to discriminate on. The
  /// prefix case is worse still — the log is not merely indistinguishable, it is *valid*: fully
  /// covered, no span out of bounds, and round-tripping against the sink's own buffer.
  ///
  /// # If you are wrapping an emitter, forward this
  ///
  /// A wrapper that forwards every emission but inherits the `None` default silently disables
  /// the check for whatever it wraps. The `&mut U` blanket forwards it; a hand-written wrapper
  /// must too. This is not detectable from inside the crate — it is the same forwarding
  /// obligation `checkpoint`/`rewind`/`release` carry, and it is pinned green by
  /// `forwarder_preserves_bound_source`.
  ///
  /// # If you are writing a source-binding emitter
  ///
  /// Override this, and assert you did — `conformance::emitter::assert_binds_source` is one
  /// line and exists for exactly that. A source-binding emitter that inherits the default
  /// compiles, runs, and quietly gives up the guarantee.
  #[inline(always)]
  fn bound_source(&self) -> Option<crate::source::SourceIdentity> {
    None
  }
}

impl<'a, L, U, Lang: ?Sized> Emitter<'a, L, Lang> for &mut U
where
  U: Emitter<'a, L, Lang>,
{
  type Error = U::Error;

  /// Forwarded, and load-bearing: a wrapper that answered `None` here would silently disable
  /// the source-identity check for the emitter it wraps.
  #[inline(always)]
  fn bound_source(&self) -> Option<crate::source::SourceIdentity> {
    (**self).bound_source()
  }

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
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = "   [`Verbose`]'s mark is its log length, a token tracker's is its count, a stateless emitter's"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = "   `Verbose`'s mark is its log length, a token tracker's is its count, a stateless emitter's"
)]
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
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " Every emitter this crate offers: [`Verbose`], [`Fatal`], [`Silent`],"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " Every emitter this crate offers: `Verbose`, [`Fatal`], [`Silent`],"
)]
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

/// The collecting combinators' **default-policy** emitter surface, as one bound.
///
/// This is the ladder a separated / repeated / delimited driver needs when no count or
/// separator *policy* is attached. Attach one — `at_most`, `bounded`, `require_leading`,
/// `require_trailing` — and the driver additionally needs [`TooManyEmitter`],
/// [`MissingLeadingSeparatorEmitter`] and [`MissingTrailingSeparatorEmitter`]; that wider
/// bundle is [`PolicyComposableEmitter`], and it has this one as a supertrait. The pratt
/// engines are outside both: neither is a collecting combinator, and each names what it needs
/// at its own entry point. The **token** engine
/// ([`InputRef::pratt`](crate::InputRef::pratt)) names [`PrattEmitter`] there, beside
/// `From<UnexpectedEot>` and the two conversions for the failures it *returns*
/// ([`RecursionLimitReached`](crate::error::RecursionLimitReached) and
/// [`NonAssociativeChain`](crate::error::NonAssociativeChain)). The **typed** engine
/// ([`Pratt`](crate::parser::Pratt)) names no emitter sub-trait at all: it builds its
/// end-of-expression reports itself rather than emitting them, so its bounds are four plain
/// `From`s — `UnexpectedEoLhs`, `UnexpectedEoRhs`, and the same two returned failures.
///
/// The two tiers exist rather than one because widening this bundle would make every
/// consumer's *concrete instantiation* demand `From<TooMany>` and `From<MissingToken>` on its
/// error type — for `Fatal` and `Verbose`, whose impls carry those bounds — whether or not any
/// policy builder is ever used. That is a bound derived from trait surface rather than from
/// behaviour, which is the same defect this crate removed from `Silent`'s pratt impl.
///
/// The atomic emitter design splits diagnostics across a family of focused traits, and a
/// parser that drives the separated/repeated machinery ends up needing most of them at
/// once — a seven-line where-clause ladder repeated at every generic parser an author
/// writes. `ComposableEmitter` is that ladder as a single name: it is
/// blanket-implemented for every emitter satisfying the whole family, so a bound of
/// `E: ComposableEmitter<'inp, L, Lang>` is interchangeable with spelling out all seven
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " sub-traits. The pre-built [`Fatal`], [`Verbose`], and [`Silent`] emitters all qualify"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " sub-traits. The pre-built [`Fatal`], `Verbose`, and [`Silent`] emitters all qualify"
)]
/// whenever their error type absorbs the corresponding error families.
///
/// The context-side twin is [`ComposableParseContext`](crate::ComposableParseContext), which rides this bound on a
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
#[diagnostic::on_unimplemented(
  message = "`{Self}` is not a composable emitter for lexer `{L}`",
  label = "one or more of the seven `ComposableEmitter` members is missing for this type",
  note = "the members: `Emitter`, `FullContainerEmitter`, `SeparatedEmitter`, `UnexpectedLeadingSeparatorEmitter`, `UnexpectedTrailingSeparatorEmitter`, `TooFewEmitter`, `UnclosedEmitter` — the compiler names the missing one below"
)]
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

/// [`ComposableEmitter`] plus the three emitters a **count or separator policy** needs.
///
/// `at_most` / `bounded` add [`TooManyEmitter`]; `require_leading` and `require_trailing` add
/// [`MissingLeadingSeparatorEmitter`] and [`MissingTrailingSeparatorEmitter`]. A production
/// that attaches any of those to a collecting combinator needs this bundle where an
/// unpolicied one needs [`ComposableEmitter`].
///
/// The lattice is strict — `ComposableEmitter` is a supertrait of this one — and each tier is
/// true to its own documentation, which is the point: a bundle that covered neither path
/// honestly is what the two-tier split replaces. Pratt stays outside both (see
/// [`ComposableEmitter`]).
///
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " All four in-crate emitters ([`Fatal`], [`Verbose`], [`Silent`],"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " All four in-crate emitters ([`Fatal`], `Verbose`, [`Silent`],"
)]
/// [`Ignored`](crate::utils::marker::Ignored)) implement the three added sub-traits already,
/// so the blanket impl covers them with no new impls.
///
/// The context-side twin is [`PolicyParseContext`](crate::PolicyParseContext).
pub trait PolicyComposableEmitter<'inp, L, Lang: ?Sized = ()>:
  ComposableEmitter<'inp, L, Lang>
  + TooManyEmitter<'inp, L, Lang>
  + MissingLeadingSeparatorEmitter<'inp, L, Lang>
  + MissingTrailingSeparatorEmitter<'inp, L, Lang>
where
  L: Lexer<'inp>,
{
}

impl<'inp, L, Lang: ?Sized, T> PolicyComposableEmitter<'inp, L, Lang> for T
where
  L: Lexer<'inp>,
  T: ComposableEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + MissingLeadingSeparatorEmitter<'inp, L, Lang>
    + MissingTrailingSeparatorEmitter<'inp, L, Lang>,
{
}

/// Every token-level failure a grammar can hit, as one conversion bound on the error type.
///
/// A leaf atom does not choose which of these it might raise — reading a token can fail
/// four ways, and a delimited production a fifth — so an error type that absorbs some but
/// not all of them is not usable as *an* error type, only as one for the subset of the
/// grammar that happens to avoid the rest. `FromTokenErrors` is that whole set as a single
/// name, blanket-implemented for every type satisfying all five members, so a signature
/// states it once instead of restating the ladder at every generic parser.
///
/// The context-side twin is
/// [`ComposableParseContext`](crate::ComposableParseContext), which rides this bound on a
/// [`ParseContext`](crate::ParseContext)'s emitter error through
/// [`ComposableEmitter`] — so one context bound supplies both the emitter surface and
/// these conversions.
///
/// # The two end-of-input members
///
/// [`UnexpectedEnd`](crate::error::UnexpectedEnd) carries the *expected set* as a type
/// parameter, and the library instantiates it two ways: the `_or_stop` family raises the
/// default `Set = &'static str` set, while the committed dispatch drivers feed their
/// caller-supplied `&'static [Kind]` classification table straight into the diagnostic. A
/// grammar that uses both must absorb both, so both are named here. **One `Set`-generic
/// `From` impl satisfies both** — that is the whole cost, and it is why an implementor
/// should write the generic form rather than two pinned ones:
///
/// ```rust
/// # use tokora::error::UnexpectedEot;
/// # struct MyError;
/// impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for MyError {
///   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { MyError }
/// }
/// ```
///
/// # Examples
///
/// The bundle elaborates back to the whole ladder — the one-bound function can call into
/// code demanding the individual conversions:
///
/// ```rust
/// use tokora::{Lexer, Token, emitter::{FromTokenErrors, FromUnclosed}};
/// use tokora::error::{UnexpectedEot, token::UnexpectedTokenOf};
///
/// // The ladder a leaf atom would otherwise carry…
/// fn ladder<'inp, L, E>()
/// where
///   L: Lexer<'inp>,
///   E: From<UnexpectedEot<L::Offset, ()>>
///     + From<UnexpectedEot<L::Offset, (), <L::Token as Token<'inp>>::Kind>>
///     + From<UnexpectedTokenOf<'inp, L>>
///     + From<<L::Token as Token<'inp>>::Error>
///     + FromUnclosed<'inp, L>,
/// {
/// }
///
/// // …collapses to a single bound that still unlocks every rung.
/// fn bundled<'inp, L, E>()
/// where
///   L: Lexer<'inp>,
///   E: FromTokenErrors<'inp, L>,
/// {
///   ladder::<L, E>()
/// }
/// ```
#[diagnostic::on_unimplemented(
  message = "`{Self}` cannot yet be a tokora parse error for lexer `{L}`",
  label = "this error type is missing one or more token-level conversions",
  note = "an error type must absorb all five token-level failures: \
          `From<UnexpectedEot<L::Offset, Lang>>`, \
          `From<UnexpectedEot<L::Offset, Lang, <L::Token as Token>::Kind>>`, \
          `From<UnexpectedTokenOf<'_, L, Lang>>`, \
          `From<<L::Token as Token>::Error>`, and `FromUnclosed<'_, L, Lang>`",
  note = "the two end-of-input members are one `Set`-generic impl: \
          `impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for {Self} \
          {{ fn from(e: UnexpectedEot<O, Lang, Set>) -> Self {{ /* … */ }} }}`",
  note = "the delimiter half is one generic impl: \
          `impl<'i, L: Lexer<'i>, Lang: ?Sized> FromUnclosed<'i, L, Lang> for {Self} \
          {{ fn from_unclosed<D>(e: Unclosed<D, L::Span, Lang>) -> Self {{ /* … */ }} }}`",
  note = "pin the span if your error stores it — an opaque `L::Span` has no route to a \
          concrete span type: `L: Lexer<'i, Span = SimpleSpan>`"
)]
pub trait FromTokenErrors<'inp, L, Lang: ?Sized = ()>:
  From<UnexpectedEot<<L as Lexer<'inp>>::Offset, Lang>>
  + From<
    UnexpectedEot<
      <L as Lexer<'inp>>::Offset,
      Lang,
      <<L as Lexer<'inp>>::Token as Token<'inp>>::Kind,
    >,
  > + From<UnexpectedTokenOf<'inp, L, Lang>>
  + From<<<L as Lexer<'inp>>::Token as Token<'inp>>::Error>
  + FromUnclosed<'inp, L, Lang>
where
  L: Lexer<'inp>,
{
}

// Deliberately **no** `#[diagnostic::do_not_recommend]` here. The claim that it is required
// to make the curated `on_unimplemented` message above fire at all is false, and was measured
// rather than reasoned: rustc walks up to the nearest annotated trait and attaches its message
// whether or not this impl is marked. Both arms of the measurement, on an error type missing
// only the two end-of-input members:
//
// * with the attribute — 3 errors; `help:` reads `the trait FromTokenErrors<…> is not
//   implemented`, so the *identity of the missing member* is gone;
// * without it — 4 errors; `help:` names `From<UnexpectedEnd<TokenHint>>` and
//   `From<UnexpectedEnd<TokenHint, usize, (), TokenKind>>`, i.e. exactly what is missing.
//
// So its only real effect is to trade the missing member's identity for one error instead of
// one per member. Naming the member is the actionable datum, the enumerating `note` above
// already bounds the verbosity at five, and one fewer behaviour resting on rustc's obligation
// walk is worth more than the deduplication. Do not add it back on the strength of the
// original "the message will not fire without it" premise — that premise was tested and is
// wrong.
impl<'inp, L, Lang: ?Sized, T> FromTokenErrors<'inp, L, Lang> for T
where
  L: Lexer<'inp>,
  T: From<UnexpectedEot<<L as Lexer<'inp>>::Offset, Lang>>
    + From<
      UnexpectedEot<
        <L as Lexer<'inp>>::Offset,
        Lang,
        <<L as Lexer<'inp>>::Token as Token<'inp>>::Kind,
      >,
    > + From<UnexpectedTokenOf<'inp, L, Lang>>
    + From<<<L as Lexer<'inp>>::Token as Token<'inp>>::Error>
    + FromUnclosed<'inp, L, Lang>,
{
}
