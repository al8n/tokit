use crate::State;

/// Error returned when recursion depth exceeds the configured limit.
///
/// This error provides context about both the actual recursion depth reached
/// and the maximum depth allowed, making it easy to diagnose whether the limit
/// needs adjustment or if there's a genuine infinite recursion bug.
///
/// # Example
///
/// ```rust
/// use tokora::state::recursion_tracker::{RecursionLimiter, RecursionLimitExceeded};
///
/// let mut limiter = RecursionLimiter::with_limitation(10);
///
/// // Simulate deep recursion
/// for _ in 0..15 {
///     limiter.increase();
/// }
///
/// match limiter.check() {
///     Err(error) => {
///         eprintln!("Recursion limit exceeded!");
///         eprintln!("Current depth: {}", error.depth());
///         eprintln!("Maximum allowed: {}", error.limitation());
///     }
///     Ok(_) => unreachable!(),
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("recursion limit exceeded: depth {}, maximum {}", .0.depth(), .0.limitation())]
pub struct RecursionLimitExceeded(RecursionLimiter);

impl RecursionLimitExceeded {
  /// Returns the actual recursion depth that triggered the error.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::recursion_tracker::{RecursionLimiter, RecursionLimitExceeded};
  ///
  /// let mut limiter = RecursionLimiter::with_limitation(3);
  /// limiter.increase();
  /// assert_eq!(limiter.depth(), 1);
  /// ```
  #[inline(always)]
  pub const fn depth(&self) -> usize {
    self.0.depth()
  }

  /// Returns the maximum recursion depth that was configured.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::recursion_tracker::{RecursionLimiter, RecursionLimitExceeded};
  ///
  /// let mut limiter = RecursionLimiter::with_limitation(3);
  /// assert_eq!(limiter.limitation(), 3);
  /// ```
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.0.limitation()
  }
}

/// A recursion depth tracker that prevents stack overflow in recursive parsers.
///
/// `RecursionLimiter` helps protect against infinite recursion by tracking the current
/// recursion depth and enforcing a maximum depth limit. This is essential for parsers
/// that use recursive descent, as deeply nested or circular grammar rules can easily
/// cause stack overflow.
///
/// # Two Defaults, Two Subjects
///
/// [`new`](Self::new) and [`Default`] give this type its own general-purpose depth, **500**: a
/// generous, unmeasured ceiling that assumes nothing about what one level of "recursion" costs.
/// That is what a caller reaching for this type directly gets — including a lexer's `State`/
/// `Extras` nesting tracker (see `Integration with tokora` below), where a level costs no native
/// stack at all, so no stack-safety math applies to it.
///
/// **tokora's own Pratt-parser wiring does not use that default.**
/// [`ParserContext`](crate::ParserContext) and the input layer each request
/// [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) explicitly instead of inheriting
/// [`new`](Self::new)'s, because on that path a level IS a live native-stack frame, and 500 of
/// those is not safe everywhere a parse runs. The combined
/// [`Limiter`](crate::state::tracker::Limiter) is not part of that wiring — tokora's own parser
/// never builds its recursion budget through it — so its constructors inherit this same
/// general-purpose 500 rather than requesting the parser's own.
///
/// That constant is **16**, in every build and under every feature, and the whole derivation —
/// including why it was 64, why 64 was wrong, and why no feature is allowed to move it — is on
/// [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) rather than repeated here. What belongs here
/// is the measurement table it is *not* derived from, because that table is this type's own and
/// keeps being mistaken for the answer:
///
/// Measured on this tree, one pratt frame per level of nesting, bisected to the last depth that
/// completes on an explicitly sized **2 MiB** thread — the stack Rust gives every
/// `std::thread::spawn` and every libtest harness thread, and so the smallest stack a parse is
/// likely to get — before the native stack aborts the process:
///
/// | build | typed driver | token driver |
/// |---|---|---|
/// | release (`opt-level = 3`), 2 MiB thread | **3871** frames, ~0.53 KiB each | **4247** frames, ~0.48 KiB each |
/// | debug (`opt-level = 0`), 2 MiB thread | **384** frames, ~5.3 KiB each | **125** frames, ~16.4 KiB each |
///
/// The binding cell of *this* table is the **debug token driver at 125**, not the release figures
/// above it, and sizing the parser's default against the release ceiling was one mistake it
/// corrected: an unconfigured parse in a debug build — which is what every test suite runs —
/// reached the native stack before the limiter. **Sizing it against this table at all was the
/// second mistake**, because these are tokora's frames and a consumer's grammar runs *inside*
/// them; that is what `PARSE_DEFAULT_DEPTH` records, and it is why the shipped figure is derived
/// from a consumer measurement of **51** rather than from the 125 here.
///
/// The two failure modes are not symmetric, and that is what settles the direction to err in. A
/// limit that is too *low* returns a clean, catchable, documented
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) telling the caller to raise it.
/// A limit that is too *high* aborts the process with no diagnostic at all and takes the whole
/// suite with it. Only one of those can be recovered from, so the parser's own default is set
/// where **every** measured configuration survives.
///
/// This type's own 500 was never sized against anything, and that is not an oversight: nothing
/// about tallying lexer nesting, or an arbitrary caller's own notion of "depth", implies a
/// native-stack cost for this type to protect against, so there is no stack ceiling for 500 to
/// clear. It only has to stay out of the way of ordinary use while still refusing a runaway
/// count eventually, and it did exactly that for this type before the measurement above existed.
///
/// **A grammar that parses untrusted, deeply nested input should still set its own limit** with
/// [`with_limitation`](Self::with_limitation) rather than inherit either default — especially on
/// a worker thread, where the stack is 2 MiB and not the main thread's 8 MiB. The figures above
/// are this crate's pratt frames on one platform and one toolchain; a grammar whose own recursive
/// combinators sit between them spends more per level, and a build with overflow checks, extra
/// debuginfo or a different codegen unit count spends more again. Pick the limit against the
/// stack the parse will actually run on.
///
/// # Use Cases
///
/// - **Recursive descent parsers**: Track depth through grammar rules
/// - **AST traversal**: Prevent stack overflow on deeply nested trees
/// - **Expression evaluation**: Limit nesting in arithmetic/boolean expressions
/// - **Stateful lexers**: Track depth in the lexer's `Extras` state
///
/// # Integration with tokora
///
/// **This is the cell the parse itself descends through.** Every parse input carries one,
/// configured through
/// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter)
/// (or [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter))
/// and defaulting to [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) — requested explicitly by
/// that wiring, and NOT [`new`](Self::new)'s own 500.
/// Both Pratt engines enter one level per live frame through
/// [`InputRef::descend`](crate::InputRef::descend), whose [`Descent`](crate::input::Descent)
/// guard releases the level on every exit including an unwind; exceeding the limit fails the
/// parse with the always-terminal [`RecursionLimitReached`](crate::error::RecursionLimitReached).
/// The budget is *per input*, not per parser, so two pratt parsers composed into one grammar
/// share one depth budget. A hand-written recursive combinator draws on the same cell through
/// [`InputRef::descending`](crate::InputRef::descending), which is the guard's scoped form and
/// the one to prefer.
///
/// It is deliberately **not** part of the checkpoint set: depth is a fact about the control
/// stack, and a rollback happens at the same frame depth as the save it returns to.
///
/// Separately, `RecursionLimiter` can also be used as part of a Logos lexer's `Extras` state by
/// implementing the [`State`] trait, allowing you to track nesting during *lexing*. That is a
/// different cell with a different subject: the lexer's tally is monotone in the input and its
/// trip latches the poison boundary, while the parse's descent unwinds — and it is exactly why a
/// tracker built through [`new`](Self::new) (or `#[derive(Default)]`, as the example below does)
/// gets 500 rather than [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH): lexing spends no
/// native-stack frame per nesting level, so the stack-safety derivation has nothing to say about
/// it, and this type does not presume a caller reaching for it directly is on the parser's path.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// let mut limiter = RecursionLimiter::new();
///
/// limiter.increase(); // Enter recursion
/// assert_eq!(limiter.depth(), 1);
///
/// limiter.increase(); // Go deeper
/// assert_eq!(limiter.depth(), 2);
///
/// limiter.decrease(); // Return from recursion
/// assert_eq!(limiter.depth(), 1);
///
/// limiter.decrease();
/// assert_eq!(limiter.depth(), 0);
/// ```
///
/// ## Custom Limit
///
/// ```rust
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// // Allow deeper nesting for complex grammars
/// let mut limiter = RecursionLimiter::with_limitation(1000);
///
/// assert_eq!(limiter.limitation(), 1000);
/// ```
///
/// ## Checking Limits
///
/// ```rust
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// let mut limiter = RecursionLimiter::with_limitation(5);
///
/// for _ in 0..5 {
///     limiter.increase();
///     assert!(limiter.check().is_ok()); // Still within limit
/// }
///
/// limiter.increase(); // One too many
/// assert!(limiter.check().is_err()); // Limit exceeded!
/// ```
///
/// ## Recursive Parser Example
///
/// ```rust,ignore
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// fn parse_expr(input: &str, limiter: &mut RecursionLimiter) -> Result<Expr, Error> {
///     limiter.increase();
///     limiter.check()?; // Fail fast if too deep
///
///     let result = match input.chars().next() {
///         Some('(') => {
///             // Recursively parse nested expression
///             let nested = parse_expr(&input[1..], limiter)?;
///             Expr::Paren(Box::new(nested))
///         }
///         Some(c) if c.is_numeric() => Expr::Number(c.to_digit(10).unwrap()),
///         _ => return Err(Error::Unexpected),
///     };
///
///     limiter.decrease(); // Return from recursion
///     Ok(result)
/// }
/// ```
///
/// ## With Logos Lexer State
///
/// ```rust,ignore
/// use logos::Logos;
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// #[derive(Default)]
/// struct LexerState {
///     recursion: RecursionLimiter,
/// }
///
/// #[derive(Logos, Debug)]
/// #[logos(extras = LexerState)]
/// enum Token {
///     #[regex(r"\(", |lex| {
///         lex.extras.recursion.increase();
///         lex.extras.recursion.check().ok()
///     })]
///     LParen(()),
///
///     #[regex(r"\)", |lex| {
///         lex.extras.recursion.decrease();
///         Some(())
///     })]
///     RParen,
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecursionLimiter {
  max: usize,
  current: usize,
}

impl Default for RecursionLimiter {
  #[inline(always)]
  fn default() -> Self {
    Self::new()
  }
}

/// **The measured figures every stack-safety number in this crate is derived from**, named once so
/// that a constant and its derivation cannot drift apart.
///
/// They live here rather than in `native_stack`, which is the module that reads most of them,
/// because that module is gated on the `pratt` feature and
/// [`RecursionLimiter::PARSE_DEFAULT_DEPTH`] is not: a build without `pratt` still has to be one
/// whose default is justified. The full tables, the platform, and the bisection method are on
/// `native_stack`'s module docs and on the constant below.
/// # EVERY FIGURE HERE IS A **DEBUG (`opt-level = 0`)** ONE
///
/// That is not incidental, and it is why the constants derived from them are keyed on
/// `debug_assertions`. A release build's frames are far cheaper — tokora's own table is 125 debug
/// frames against 3 871 / 4 247 release ones on the same thread, ~16.4 KiB against ~0.48–0.53 KiB,
/// a **31× spread covered by one number** if a default is derived from this module and shipped
/// unconditionally. Roughly 90% of the debug figure is `opt-level = 0` stack-slot non-colouring
/// rather than code size, so it does not shrink with a smaller grammar.
///
/// The consumer side has one release reading (~3.6 KiB/level against ~41 KiB debug) and it is
/// **deliberately not a constant here**, because it was taken on one of the five axes the debug
/// bisection covered — not on architecture, the second dialect, its generic angle brackets, or the
/// alternative source representation. A derivation reading it would be exactly the
/// derived-not-measured figure this module's assertions exist to refuse. `policy` therefore ships
/// the release cells as *floors* equal to the debug cells until that sweep is run.
pub(crate) mod measured {
  /// The binding cell: a real **consumer** grammar's debug build aborts at this depth on an
  /// explicitly sized 2 MiB thread, on the tightest of its five measured axes (the others are 52,
  /// 53, 57 and 60).
  pub(crate) const CONSUMER_ABORTS_AT: usize = 51;

  /// The tightest cell of **tokora's own** table — the debug token driver on the same thread.
  ///
  /// Kept beside the consumer figure precisely because it is the one the old default was sized
  /// against. Every guard below that names `CONSUMER_ABORTS_AT` is a place where using this
  /// number instead is the defect.
  pub(crate) const TOKORA_ABORTS_AT: usize = 125;

  /// The thread every figure is stated on, and the one `std::thread::spawn` hands out.
  pub(crate) const STACK: usize = 2 * 1024 * 1024;

  /// Bytes of native stack one level of the heaviest measured grammar spends — **derived** from
  /// the bisection rather than written down separately, so the two cannot disagree.
  pub(crate) const CONSUMER_BYTES_PER_LEVEL: usize = STACK / CONSUMER_ABORTS_AT;
}

/// **What this crate CHOSE**, given what [`measured`] records — kept apart from it because the two
/// go wrong in different ways and only one of them can be re-measured.
///
/// A number in [`measured`] is answerable to a bisection: if it is wrong, run the bisection again.
/// A number here is answerable to an argument, and the argument is written on it. The reason for
/// the split is the defect this module's assertions exist to catch: a range-shaped guard reads as
/// though it were checking a derivation while in fact admitting every value the argument would
/// have rejected. So each shipped constant below is *derived* — computed from a measurement and a
/// policy — and the published constant is then asserted **equal** to its derivation, rather than
/// merely inside a band around it.
///
/// # A matrix of four cells, not two numbers
///
/// Two axes, and each one is a population whose frames carry a different guarantee:
///
/// | | `stacker` off | `stacker` on |
/// |---|---|---|
/// | debug | [`PARSE_DEFAULT_DEBUG`] — derived | [`SEGMENTED_PRATT_DEBUG`] — derived |
/// | release | [`PARSE_DEFAULT_RELEASE`] — **floor** | [`SEGMENTED_PRATT_RELEASE`] — **floor** |
///
/// Every cell is named and pinned by an `==` of its own. The two release cells are **floors set
/// equal to their debug siblings, and are not derivations**: [`measured`] contains no release
/// bisection to derive them from, and inventing one from debug's ratio is the defect. What the
/// split buys today is that filling them in later is a value change and not a refactor — and that
/// no assertion in this file claims a release derivation nobody performed.
pub(crate) mod policy {
  use super::measured;

  /// How many times the shipped parse-side default must fit under the binding measured cell.
  ///
  /// Three, and the three is the whole of what makes 16 rather than 32 the answer: 32 leaves
  /// 1.59× under the consumer row's 51, which is *below* the 1.9× that made 64 look safe on
  /// tokora's own row, and well inside the range another platform's codegen moves.
  pub(crate) const MIN_HEADROOM: usize = 3;

  /// The margin the old default was rejected for not having, in tenths.
  ///
  /// `100` under `125` is 1.25× and was called too thin to survive another platform's codegen;
  /// `64` under `125` is 1.9× and was accepted. Any default this crate ships must clear its
  /// binding cell by at least the accepted figure, and stating it as a constant is what makes the
  /// next person to move a default move this line too.
  ///
  /// It is the *weaker* of the two requirements — [`MIN_HEADROOM`] is 30 tenths — and it is kept
  /// beside it rather than folded into it because it is the figure the rejected default was judged
  /// against, so it is what makes "and it still clears the bar 64 was held to" checkable.
  pub(crate) const MIN_MARGIN_TENTHS: usize = 19;

  /// **The parse-side default in a debug build, derived**: the deepest power of two that still
  /// fits [`MIN_HEADROOM`] times under [`measured::CONSUMER_ABORTS_AT`].
  ///
  /// 16, from 51 and 3 — `16 × 3 = 48 < 51`, and `32 × 3 = 96` is not. A power of two because
  /// every default this crate has shipped has been one and a reader expects it, and the *deepest*
  /// one because the asymmetry the constant's docs argue says to err low, not to err small.
  ///
  /// This exists so the published constant can be asserted **equal** to something. A bare
  /// inequality against the measurement admits every smaller value including 1, which is what the
  /// guards in this file used to do.
  pub(crate) const PARSE_DEFAULT_DEBUG: usize = {
    let mut depth = 1;
    while depth * 2 * MIN_HEADROOM < measured::CONSUMER_ABORTS_AT {
      depth *= 2;
    }
    depth
  };

  /// **The parse-side default in a release build — a FLOOR, not a derivation.**
  ///
  /// A release frame is far cheaper than a debug one (see [`measured`]'s header: 31× on tokora's
  /// own table, ~11× on the one consumer release reading), so the honest release figure is
  /// *larger* than the debug one and this is therefore conservative in the recoverable direction:
  /// a caller who needs the depth gets a catchable
  /// [`RecursionLimitReached`](crate::error::RecursionLimitReached) naming the knob, rather than an
  /// abort.
  ///
  /// **What would raise it:** the same five-axis bisection [`measured::CONSUMER_ABORTS_AT`]
  /// records, run against a release build — architecture, the primary dialect, a second dialect,
  /// that dialect's generic brackets, and the alternative source representation. Until all five
  /// exist, a release `CONSUMER_ABORTS_AT` would be one reading standing in for a table, which is
  /// the shape this whole file refuses. When they exist, add them to [`measured`], derive this the
  /// way [`PARSE_DEFAULT_DEBUG`] is derived, and delete the *provisional* equality assertion that
  /// names this constant.
  pub(crate) const PARSE_DEFAULT_RELEASE: usize = PARSE_DEFAULT_DEBUG;

  /// The cell this build selects, and the value
  /// [`RecursionLimiter::PARSE_DEFAULT_DEPTH`](super::RecursionLimiter::PARSE_DEFAULT_DEPTH) is
  /// pinned to.
  ///
  /// # `debug_assertions` is a **proxy** for the profile, and the caveat is live
  ///
  /// It is the only profile signal a constant can read, and it is not `opt-level`: a build with
  /// `debug-assertions = false` at `opt-level = 0`, or the reverse, gets the other arm. It is also
  /// *this crate's* flag — a per-package profile override that compiles tokora differently from the
  /// consumer's grammar makes the two disagree about what a frame costs.
  ///
  /// **Both arms are currently the same number, so neither can be observed today.** Whoever raises
  /// [`PARSE_DEFAULT_RELEASE`] makes both live in the same commit, and owes this paragraph a
  /// decision rather than a re-reading.
  pub(crate) const PARSE_DEFAULT: usize = if cfg!(debug_assertions) {
    PARSE_DEFAULT_DEBUG
  } else {
    PARSE_DEFAULT_RELEASE
  };

  /// **The one number the `stacker` feature actually gets to choose**: how much stack-segment
  /// memory an unconfigured segmented parse may reach before the budget refuses it.
  ///
  /// With segments there is no native ceiling left for a depth to clear, so the bound stops being
  /// a hardware fact and becomes a memory policy — and this is that policy, stated in bytes,
  /// which is the unit the policy is actually about. The depth follows from it and the binding
  /// per-level cost; it is not chosen separately.
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENT_MEMORY_CEILING: usize = 64 * 1024 * 1024;

  /// **The segmented-Pratt budget in a debug build, derived**: the deepest power of two whose
  /// segments, at the binding per-level cost, still fit inside [`SEGMENT_MEMORY_CEILING`].
  ///
  /// 1024, from 64 MiB and ~41 KiB — `1024 × 41 KiB ≈ 40 MiB`, and `2048` is ≈80 MiB, past it.
  ///
  /// Same job as [`PARSE_DEFAULT_DEBUG`] and for the same reason: the guard this replaces accepted
  /// anything from 501 to 1632, so 512 passed while contradicting the documented figure.
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENTED_PRATT_DEBUG: usize = {
    let mut depth = 1;
    while depth * 2 * measured::CONSUMER_BYTES_PER_LEVEL <= SEGMENT_MEMORY_CEILING {
      depth *= 2;
    }
    depth
  };

  /// **The segmented-Pratt budget in a release build — a FLOOR, on the same terms as
  /// [`PARSE_DEFAULT_RELEASE`].**
  ///
  /// This one is a memory policy rather than a stack one, so what a release measurement buys here
  /// is different: the same 64 MiB ceiling holds *more* levels when a level is cheaper, so the
  /// derived release figure would again be larger. It needs the same missing per-level table, so
  /// it waits for the same sweep.
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENTED_PRATT_RELEASE: usize = SEGMENTED_PRATT_DEBUG;

  /// The cell this build selects, and the value
  /// [`RecursionLimiter::SEGMENTED_PRATT_DEPTH`](super::RecursionLimiter::SEGMENTED_PRATT_DEPTH)
  /// is pinned to. Same `debug_assertions` caveat as [`PARSE_DEFAULT`].
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENTED_PRATT: usize = if cfg!(debug_assertions) {
    SEGMENTED_PRATT_DEBUG
  } else {
    SEGMENTED_PRATT_RELEASE
  };
}

/// The derivation of [`RecursionLimiter::PARSE_DEFAULT_DEPTH`], enforced by the compiler.
///
/// These were `#[test]`s for one revision and clippy was right to refuse them: every operand is a
/// constant, so the question is settled at compile time and a *test* is the weaker place to settle
/// it. As `const` assertions a tree whose default contradicts its own measurement does not build,
/// which is a stronger guarantee than one whose test suite reddens — and it holds in every leg,
/// including the ones that never run a test.
///
/// # Equality first, arithmetic second
///
/// **The first assertion of each pair is an `==`.** For one revision they were all inequalities,
/// and an inequality that reads like a derivation is not one: the guards here imposed only *upper*
/// bounds on the unsegmented default, so a default of **1** satisfied every predicate, and the
/// segmented block accepted **anything from 501 to 1632**, so 512 passed while contradicting the
/// figure the docs published. A claim that "putting the default back to 64 fails the build" was
/// true of 64 and false as the general statement it was offered as.
///
/// So each published constant is now asserted equal to a value in [`policy`] that is *computed*
/// from a measurement and a stated policy, and the arithmetic those derivations rest on is
/// asserted separately, on the named values. Both halves are needed: the equality is what pins the
/// number, and the arithmetic is what stops the derivation being "repaired" by moving a policy
/// figure until it produces whatever the constant already said.
///
/// # Every cell of the matrix, by name
///
/// A single `==` against "whichever arm this build selected" is not a pin, it is the same
/// range-shaped guard with one member: the *other* arm is then unchecked in this build and
/// unchecked in every other, because no build compiles both. So the cells are asserted
/// individually — [`policy::PARSE_DEFAULT_DEBUG`] and [`policy::PARSE_DEFAULT_RELEASE`] each on
/// their own line, in every build, whichever one `debug_assertions` selects — and the selection
/// itself is asserted separately, against the arm the flag names.
///
/// The two release cells get **two** assertions and the pair is deliberate: a standing `>=` that
/// survives the measurement (a release frame is cheaper, so a release budget may never be the
/// *smaller* of the two) and a provisional `==` that records that no release derivation has been
/// performed. The second is the line to delete when it has been.
///
/// The cost is the message: `panic!` in a const context takes a string literal and no arguments, so
/// these cannot print the numbers they compared. They name what to go and read instead.
const _: () = {
  use measured::{CONSUMER_ABORTS_AT, CONSUMER_BYTES_PER_LEVEL, STACK, TOKORA_ABORTS_AT};
  use policy::{
    MIN_HEADROOM, MIN_MARGIN_TENTHS, PARSE_DEFAULT, PARSE_DEFAULT_DEBUG, PARSE_DEFAULT_RELEASE,
  };

  // THE NUMBER, PINNED. Not "inside a band the derivation would tolerate" — the value the
  // derivation produces, and no other. 64 fails this, and so do 1, 32 and 1024.
  assert!(
    PARSE_DEFAULT_DEBUG * MIN_HEADROOM < CONSUMER_ABORTS_AT
      && PARSE_DEFAULT_DEBUG * 2 * MIN_HEADROOM >= CONSUMER_ABORTS_AT,
    "PARSE_DEFAULT_DEBUG is not the number its own derivation produces — the DEEPEST power of two \
     that fits MIN_HEADROOM times under CONSUMER_ABORTS_AT. Both halves matter: the first is the \
     safety requirement, the second is what makes it a derivation rather than a bound satisfied \
     by 1."
  );
  // THE STANDING ORDER between the two profiles, and the one that survives the release sweep. A
  // release frame is cheaper than a debug one, so a release budget may be larger and may never be
  // smaller; getting this backwards ships debug-sized frames against a release-sized budget.
  assert!(
    PARSE_DEFAULT_RELEASE >= PARSE_DEFAULT_DEBUG,
    "the release parse default is below the debug one, which inverts the only thing known about \
     the two profiles: a release frame is cheaper, so its budget can only be the larger"
  );
  // THE PROVISIONAL LINE. **Delete this when, and only when, the release bisection exists** —
  // see PARSE_DEFAULT_RELEASE for the five axes it owes. Until then, a release value above the
  // debug one is a number nobody measured, and this is what stops it being written.
  assert!(
    PARSE_DEFAULT_RELEASE == PARSE_DEFAULT_DEBUG,
    "the release parse default has been raised above the debug one without a release measurement \
     in `measured` to derive it from. See PARSE_DEFAULT_RELEASE: it is a floor, and raising it \
     means running the five-axis bisection, not extrapolating from debug's ratio."
  );
  // And the selection, so that the published constant is the arm this build's flag names rather
  // than whichever one somebody typed.
  assert!(
    PARSE_DEFAULT
      == if cfg!(debug_assertions) {
        PARSE_DEFAULT_DEBUG
      } else {
        PARSE_DEFAULT_RELEASE
      },
    "PARSE_DEFAULT does not select the cell `debug_assertions` names"
  );
  assert!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH == PARSE_DEFAULT,
    "PARSE_DEFAULT_DEPTH is not the value `policy` derives for this build. If the shipped default \
     should move, move the measurement or the policy it is derived from; editing the published \
     constant alone is the defect this assertion exists for."
  );

  // THE WHOLE ORIGINAL DEFECT, AS ONE INEQUALITY, now stated over the derived value.
  // `PARSE_DEFAULT_DEPTH` was 64 and `CONSUMER_ABORTS_AT` is 51, so a derivation producing 64
  // fails here: the native stack got there before the limiter could, and the refusal the budget
  // exists to produce was unreachable.
  assert!(
    PARSE_DEFAULT_DEBUG * MIN_MARGIN_TENTHS <= CONSUMER_ABORTS_AT * 10,
    "the derived debug default does not clear CONSUMER_ABORTS_AT — the depth at which a measured \
     consumer grammar aborts on a 2 MiB thread — by MIN_MARGIN_TENTHS, the margin this crate \
     already requires of such a number. See PARSE_DEFAULT_DEPTH's derivation."
  );
  assert!(
    MIN_HEADROOM * 10 > MIN_MARGIN_TENTHS,
    "MIN_HEADROOM has been loosened below MIN_MARGIN_TENTHS, the margin the rejected default was \
     already held to — which would let this crate ship a default it had itself refused"
  );
  // A guard against the derivation being 'repaired' by re-reading the wrong row: sizing against
  // tokora's own 125 admits 64, which is exactly the number that was wrong.
  assert!(
    TOKORA_ABORTS_AT > CONSUMER_ABORTS_AT,
    "the two measured rows have stopped disagreeing, which means one was edited to match the \
     other rather than re-measured"
  );

  // **NO `stacker` ARM, and that is the point of this round.** This constant seeds every
  // `InputContext`, and it is read by hand-written public descent through `InputRef::descend` /
  // `descending` — paths `native_stack::maybe_grow` never touches, because its only two callers
  // are the Pratt prologues. So it has to be safe for a consumer who segmented nothing, whatever
  // tokora compiled for itself: re-introducing a `cfg!(feature = "stacker")` arm here fails this
  // line, because 1024 levels of the heaviest measured grammar is ≈41 MiB and the thread is 2.
  //
  // The debug cell is what it is priced against, because `CONSUMER_BYTES_PER_LEVEL` is a debug
  // figure and pricing a release budget with a debug cost is the conservative direction.
  assert!(
    PARSE_DEFAULT_DEBUG * CONSUMER_BYTES_PER_LEVEL < STACK,
    "an unconfigured parse can reach more native stack than the 2 MiB the derivation is stated \
     on. A budget that only a segmented path could survive does not belong on the cell every \
     unsegmented `descend` reads; see RecursionLimiter::SEGMENTED_PRATT_DEPTH."
  );
  assert!(
    PARSE_DEFAULT_RELEASE * CONSUMER_BYTES_PER_LEVEL < STACK,
    "the release parse default, priced at the DEBUG per-level cost, no longer fits the 2 MiB \
     thread. Once `measured` carries a release per-level figure this assertion should be priced \
     at that one instead — until then the debug cost is the only one there is, and it is the \
     conservative direction."
  );
};

/// The derivation of [`RecursionLimiter::SEGMENTED_PRATT_DEPTH`], on the same terms as the block
/// above: every cell of the profile matrix pinned by name, arithmetic second.
#[cfg(feature = "stacker")]
const _: () = {
  use measured::{CONSUMER_ABORTS_AT, CONSUMER_BYTES_PER_LEVEL, TOKORA_ABORTS_AT};
  use policy::{
    SEGMENT_MEMORY_CEILING, SEGMENTED_PRATT, SEGMENTED_PRATT_DEBUG, SEGMENTED_PRATT_RELEASE,
  };

  // 512 fails this, and so did every other value in the 501..=1632 band the inequality-only
  // guards accepted. Both halves again: within the ceiling, and the deepest such power of two.
  assert!(
    SEGMENTED_PRATT_DEBUG * CONSUMER_BYTES_PER_LEVEL <= SEGMENT_MEMORY_CEILING
      && SEGMENTED_PRATT_DEBUG * 2 * CONSUMER_BYTES_PER_LEVEL > SEGMENT_MEMORY_CEILING,
    "SEGMENTED_PRATT_DEBUG is not the number its own derivation produces — the DEEPEST power of \
     two whose segments fit inside SEGMENT_MEMORY_CEILING at the binding per-level cost. Move the \
     memory policy, not the published depth."
  );
  // The same standing order and the same provisional line as the unsegmented pair, for the same
  // two reasons: a cheaper release level fits MORE of itself in the same memory ceiling, so the
  // release cell can only be the larger — and no release per-level figure exists to derive it.
  assert!(
    SEGMENTED_PRATT_RELEASE >= SEGMENTED_PRATT_DEBUG,
    "the release segmented budget is below the debug one, which inverts what a cheaper level does \
     to a fixed memory ceiling"
  );
  // **Delete with PARSE_DEFAULT_RELEASE's twin, and not before**: same missing sweep.
  assert!(
    SEGMENTED_PRATT_RELEASE == SEGMENTED_PRATT_DEBUG,
    "the release segmented budget has been raised above the debug one without a release per-level \
     figure in `measured` to derive it from. See SEGMENTED_PRATT_RELEASE."
  );
  assert!(
    SEGMENTED_PRATT
      == if cfg!(debug_assertions) {
        SEGMENTED_PRATT_DEBUG
      } else {
        SEGMENTED_PRATT_RELEASE
      },
    "SEGMENTED_PRATT does not select the cell `debug_assertions` names"
  );
  assert!(
    RecursionLimiter::SEGMENTED_PRATT_DEPTH == SEGMENTED_PRATT,
    "SEGMENTED_PRATT_DEPTH is not the value `policy` derives for this build"
  );

  // With the feature on, the ceiling is a policy choice about memory, so the arithmetic is about
  // memory: the budget must buy a depth the thread stack could not have — otherwise the feature is
  // paying for a ceiling it did not raise.
  assert!(
    SEGMENTED_PRATT_DEBUG > CONSUMER_ABORTS_AT * 4,
    "SEGMENTED_PRATT_DEPTH is not meaningfully past what a 2 MiB thread already gave that \
     grammar, so the feature raised no ceiling"
  );
  // Past BOTH measured rows, not just the consumer one. The consumer row is what the
  // stack-bounded default is derived from, but the claim this feature makes is that neither
  // row bounds it any more, and tokora's own is the larger of the two.
  assert!(
    SEGMENTED_PRATT_DEBUG > TOKORA_ABORTS_AT * 4,
    "SEGMENTED_PRATT_DEPTH does not clear tokora's own measured ceiling by enough to be a policy \
     figure rather than a stack one"
  );
  // And the separation itself: the segmented figure has to be strictly the larger of the two, or
  // there was no reason to name it apart from the default. Stated per profile, because the two
  // pairs move independently once the release sweep lands.
  assert!(
    SEGMENTED_PRATT_DEBUG > policy::PARSE_DEFAULT_DEBUG
      && SEGMENTED_PRATT_RELEASE > policy::PARSE_DEFAULT_RELEASE,
    "SEGMENTED_PRATT_DEPTH is not above PARSE_DEFAULT_DEPTH in one of the two profiles, so the \
     constants have collapsed into one there and the segmented policy buys nothing"
  );
};

impl RecursionLimiter {
  /// tokora's own recursion budget for a parse — **16**, in every build — requested explicitly by
  /// [`ParserContext`](crate::ParserContext) and the input layer instead of inherited from
  /// [`new`](Self::new).
  ///
  /// # It is one number, and no feature moves it
  ///
  /// For one revision it was `16`, or `1024` with the `stacker` feature, and that was the same
  /// defect as the one below wearing a different hat. This constant seeds **every**
  /// [`InputContext`](crate::input::InputContext), and the cell it seeds is read by hand-written
  /// public descent — [`InputRef::descend`](crate::InputRef::descend) and
  /// [`descending`](crate::InputRef::descending) — from a consumer's *own* productions. Those
  /// frames sit on the ordinary native stack: the segmented prologue is inside tokora's two Pratt
  /// engines and nowhere else, so a feature that segments tokora's frames says nothing whatever
  /// about a caller's. Moving a shared budget to a figure only the segmented path can survive is a
  /// number derived over one population and applied to a wider one, which is precisely what made
  /// 64 wrong.
  ///
  /// The figure that *is* defensible over segmented Pratt frames is published separately, as
  #[cfg_attr(
    feature = "stacker",
    doc = "[`SEGMENTED_PRATT_DEPTH`](Self::SEGMENTED_PRATT_DEPTH), which a caller whose whole"
  )]
  #[cfg_attr(
    not(feature = "stacker"),
    doc = "`SEGMENTED_PRATT_DEPTH` (`stacker` only), which a caller whose whole"
  )]
  /// descent is Pratt frames opts into by hand. Opting in is the point: only the caller knows
  /// whether that precondition holds of their grammar.
  ///
  /// # Why the default is 16, and why it used to be 64
  ///
  /// **64 was wrong, and it was wrong in the direction that aborts.** It was derived from the
  /// table in `Two Defaults, Two Subjects` — tokora's own worst cell, the debug token driver's
  /// **125** frames at ~16.4 KiB on a 2 MiB thread — with roughly 1.9× of margin. But that table
  /// measures *this crate's* frames, and a consumer's grammar does not sit beside tokora's
  /// recursion, it sits **inside** it: the productions the pratt driver calls are the consumer's,
  /// so one level of nesting pays for a tokora frame *and* a consumer frame, and the number to
  /// size against is the sum.
  ///
  /// Measured on a real consumer grammar, debug, on the same explicitly sized 2 MiB thread: it
  /// aborts at **51** levels — ~41 KiB each, 2.5× tokora's own — with four further axes at 60, 57,
  /// 53 and 52. **So 64 sat above the depth at which a real grammar aborts**, and for that
  /// consumer the limiter could never fire: the native stack got there first, with
  /// `fatal runtime error: stack overflow` and no diagnostic. That is precisely the failure the
  /// 500 → 64 change had been made to fix, surviving one layer further out.
  ///
  /// 16 is the largest power of two leaving more than 3× under the binding cell of 51 — the next
  /// one up, 32, leaves 1.59×, which is *below* the 1.9× that made 64 look safe and well inside
  /// the range a different platform's codegen moves. It clears tokora's own tightest cell by 7.8×.
  ///
  /// **This is a reduction, and a grammar that legitimately nests deeper must now say so** — with
  /// [`with_limitation`](Self::with_limitation) through
  /// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter),
  /// [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter), or —
  /// for a lossless parse, whose driver builds its own context —
  #[cfg_attr(
    feature = "rowan",
    doc = "[`parse_lossless_with_context`](crate::cst::parse_lossless_with_context)."
  )]
  #[cfg_attr(
    not(feature = "rowan"),
    doc = "`cst::parse_lossless_with_context` (`rowan` only)."
  )]
  /// That asymmetry is
  /// deliberate and is the same one the type's docs argue: too low returns a clean, catchable
  /// [`RecursionLimitReached`](crate::error::RecursionLimitReached) naming the knob that raises it;
  /// too high aborts the process and takes the caller's whole program with it. Only one of the two
  /// can be recovered from.
  ///
  /// **`stacker` is not the answer to "I need more depth" either**, and that is a change from the
  /// revision that introduced it: enabling the feature no longer moves this number. It moves what
  /// *tokora's own Pratt frames* cost, and publishes the budget that fact justifies for a caller
  /// to request explicitly:
  #[cfg_attr(
    feature = "stacker",
    doc = "[`SEGMENTED_PRATT_DEPTH`](Self::SEGMENTED_PRATT_DEPTH)."
  )]
  #[cfg_attr(
    not(feature = "stacker"),
    doc = "`SEGMENTED_PRATT_DEPTH`, published only when that feature is on."
  )]
  ///
  /// # It is also keyed on the build profile, and today both cells read 16
  ///
  /// A debug frame and a release frame are not the same size — tokora's own table is ~16.4 KiB
  /// against ~0.48–0.53 KiB, a 31× spread — and 16 is derived from the **debug** bisection. So the
  /// constant selects on `debug_assertions`, and a release build will eventually get its own,
  /// larger figure. It does not have one yet: the five-axis consumer bisection behind 16 has not
  /// been run against a release build, and extrapolating it from debug's ratio would be the same
  /// derived-from-the-wrong-population mistake one more time. **The release cell is therefore a
  /// floor set equal to the debug one**, which is conservative in the recoverable direction, and a
  /// `const` assertion refuses to let it be raised until the measurement lands. Nothing about
  /// today's *behaviour* differs between the two profiles.
  pub const PARSE_DEFAULT_DEPTH: usize = policy::PARSE_DEFAULT;

  /// The recursion budget that is defensible **when every level of the descent is a segmented
  /// Pratt frame** — 1024, and `stacker`-only because that is the feature that makes the sentence
  /// true.
  ///
  /// Nothing installs it. It is a figure to hand to
  /// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter)
  /// or [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter),
  /// and the reason it is opt-in rather than the default is the whole content of the constant:
  /// **only the caller knows whether the precondition holds of their grammar.**
  ///
  /// # The precondition, stated as narrowly as it actually is
  ///
  /// `stacker` puts `stacker::maybe_grow` on the frame prologue of tokora's **two Pratt engines**,
  /// and on nothing else. A frame that enters within 256 KiB of the end of its stack
  /// continues on a fresh 2 MiB heap segment, so for those frames the ceiling stops being a
  /// hardware constant.
  ///
  /// It is **not** on [`InputRef::descend`](crate::InputRef::descend) or
  /// [`descending`](crate::InputRef::descending) as a consumer calls them from their own
  /// productions. Those frames are ordinary native frames on an ordinary thread stack, they draw
  /// on the *same* shared budget cell, and 1024 of them at the heaviest measured per-level cost is
  /// ≈41 MiB against a 2 MiB thread — the native abort this crate's whole recursion story exists
  /// to delete. So: pass this constant when the deep part of your grammar is
  /// [`pratt`](crate::parser::pratt) or [`InputRef::pratt`](crate::InputRef::pratt) frames,
  /// and do not pass it because a build happens to have the feature on.
  ///
  /// # Why 1024
  ///
  /// With segments there is no native ceiling left to clear, so the bound is a **policy choice
  /// about memory** and is derived as one: 64 MiB of stack segments, at the binding ~41 KiB per
  /// level from the consumer row of the measurement table, is 1024 levels (2048 would be ≈80 MiB).
  /// That is generous for an unconfigured parse, bounded, reached by no realistic document, and
  /// 20× the depth at which that same grammar aborts *without* the feature — which is the whole of
  /// what the feature buys.
  ///
  /// It is keyed on `debug_assertions` for the reason
  /// [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) is — a cheaper release level fits more of
  /// itself inside the same 64 MiB — and the release cell is likewise a floor equal to the debug
  /// one until the per-level table exists to derive it from.
  ///
  /// # What it is NOT
  ///
  /// **It does not make depth unlimited, and it is not decoration under a feature that already
  /// solved the problem.** A segment is a heap allocation; stacker's allocator is `mmap` plus an
  /// assertion, not a fallible reserve; so a deep enough input still ends the process, with
  /// nothing on any `Result` channel for a caller to catch and nothing for
  /// [`MaybeTerminal`](crate::error::MaybeTerminal) to report. Measured, with the feature on and
  /// the limiter set to [`unlimited`](Self::unlimited): the parse returns `Ok` past every depth a
  /// thread stack could have held — 1 000 000 levels, 5 588 MB, no refusal at any depth — and the
  /// run ends when the machine's memory does.
  ///
  /// So **the budget is what makes a too-deep input refusable, and `stacker` is not a substitute
  /// for it.** A future reader concluding the budget is now redundant, and deleting the
  /// [`InputRef::descend`](crate::InputRef::descend) that spends it, restores the abort both
  /// numbers exist to delete.
  #[cfg(feature = "stacker")]
  #[cfg_attr(docsrs, doc(cfg(feature = "stacker")))]
  pub const SEGMENTED_PRATT_DEPTH: usize = policy::SEGMENTED_PRATT;

  /// Creates a new recursion tracker.
  ///
  /// Defaults to a maximum depth of 500 — this type's own general-purpose ceiling, with no
  /// assumption about what one level costs. tokora's Pratt-parser wiring does not inherit this
  /// default; see the type's `Two Defaults, Two Subjects` docs.
  #[inline(always)]
  pub const fn new() -> Self {
    Self {
      max: 500,
      current: 0,
    }
  }

  /// Creates a new recursion tracker with the given maximum depth.
  #[inline(always)]
  pub const fn with_limitation(max: usize) -> Self {
    Self { max, current: 0 }
  }

  /// Creates a tracker that never trips — `with_limitation(usize::MAX)`.
  ///
  /// "No limit" is spelled rather than implied: a parse configured with this one still counts
  /// its descent, so [`InputRef::recursion`](crate::InputRef::recursion) stays readable, but
  /// [`check`](Self::check) can never fail. Use it to opt a parse out of the default budget
  /// (see [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter))
  /// when the grammar's depth is bounded by something other than the input.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::recursion_tracker::RecursionLimiter;
  ///
  /// let mut limiter = RecursionLimiter::unlimited();
  /// assert_eq!(limiter.limitation(), usize::MAX);
  ///
  /// for _ in 0..10_000 {
  ///   limiter.increase();
  /// }
  /// assert!(limiter.check().is_ok());
  /// ```
  #[inline(always)]
  pub const fn unlimited() -> Self {
    Self::with_limitation(usize::MAX)
  }

  /// Returns the current depth of the recursion.
  #[inline(always)]
  pub const fn depth(&self) -> usize {
    self.current
  }

  /// Returns the maximum depth of the recursion.
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.max
  }

  /// Increase the current depth of the recursion.
  ///
  /// Saturates at `usize::MAX`, mirroring the saturating [`decrease`](Self::decrease).
  #[inline(always)]
  pub const fn increase(&mut self) {
    self.current = self.current.saturating_add(1);
  }

  /// Decrease the current depth of the recursion.
  #[inline(always)]
  pub const fn decrease(&mut self) {
    self.current = self.current.saturating_sub(1);
  }

  /// Increases the recursion depth.
  #[inline(always)]
  pub const fn increase_recursion(&mut self) {
    self.increase();
  }

  /// Decrease the current depth of the recursion.
  #[inline(always)]
  pub const fn decrease_recursion(&mut self) {
    self.decrease();
  }

  /// Checks if the recursion limit has been exceeded.
  #[inline(always)]
  pub const fn check(&self) -> Result<(), RecursionLimitExceeded> {
    if self.depth() > self.limitation() {
      Err(RecursionLimitExceeded(*self))
    } else {
      Ok(())
    }
  }
}

impl State for RecursionLimiter {
  type Error = RecursionLimitExceeded;

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    <Self as RecursionTracker>::check(self)
  }
}

/// A recursion tracker trait.
pub trait RecursionTracker {
  /// The error type returned when the recursion limit is exceeded.
  type Error;

  /// Increases the recursion depth.
  fn increase(&mut self);

  /// Decreases the recursion depth.
  fn decrease(&mut self);

  /// Checks if the recursion limit has been exceeded.
  fn check(&self) -> Result<(), Self::Error>;

  /// Increases the recursion depth and checks the limit.
  #[inline(always)]
  fn increase_and_check(&mut self) -> Result<(), Self::Error> {
    self.increase();
    self.check()
  }
}

impl RecursionTracker for RecursionLimiter {
  type Error = RecursionLimitExceeded;

  #[inline(always)]
  fn increase(&mut self) {
    self.current = self.current.saturating_add(1);
  }

  #[inline(always)]
  fn decrease(&mut self) {
    self.current = self.current.saturating_sub(1);
  }

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    if self.depth() > self.limitation() {
      Err(RecursionLimitExceeded(*self))
    } else {
      Ok(())
    }
  }
}

const _: () = {
  #[allow(dead_code, unused_macros)]
  macro_rules! bail {
    ($lib:ident) => {
      use crate::lexer::$lib::{FromLogos, LogosLexer};
      use $lib::{Lexer, Logos};

      impl<'a, T> RecursionTracker for Lexer<'a, T>
      where
        T: Logos<'a>,
        T::Extras: RecursionTracker,
      {
        type Error = <T::Extras as RecursionTracker>::Error;

        #[inline(always)]
        fn increase(&mut self) {
          self.extras.increase();
        }

        #[inline(always)]
        fn decrease(&mut self) {
          self.extras.decrease();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.extras.check()
        }

        #[inline(always)]
        fn increase_and_check(&mut self) -> Result<(), Self::Error> {
          self.extras.increase_and_check()
        }
      }

      impl<'a, T> RecursionTracker for LogosLexer<'a, T>
      where
        T: FromLogos<'a>,
        <T::Logos as Logos<'a>>::Extras: RecursionTracker,
      {
        type Error = <<T::Logos as Logos<'a>>::Extras as RecursionTracker>::Error;

        #[inline(always)]
        fn increase(&mut self) {
          self.inner_mut().increase();
        }

        #[inline(always)]
        fn decrease(&mut self) {
          self.inner_mut().decrease();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.inner().check()
        }

        #[inline(always)]
        fn increase_and_check(&mut self) -> Result<(), Self::Error> {
          self.inner_mut().increase_and_check()
        }
      }
    };
  }

  #[cfg(feature = "logos_0_14")]
  #[cfg_attr(docsrs, doc(cfg(feature = "logos_0_14")))]
  const _: () = {
    bail!(logos_0_14);
  };

  #[cfg(feature = "logos_0_15")]
  #[cfg_attr(docsrs, doc(cfg(feature = "logos_0_15")))]
  const _: () = {
    bail!(logos_0_15);
  };

  #[cfg(feature = "logos_0_16")]
  #[cfg_attr(docsrs, doc(cfg(feature = "logos_0_16")))]
  const _: () = {
    bail!(logos_0_16);
  };
};

#[cfg(test)]
mod tests;
