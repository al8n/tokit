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
/// That constant is **16**, or **1024** with the `stacker` feature, and the whole derivation of
/// both — including why it was 64 and why 64 was wrong — is on
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

  /// The margin the old default was rejected for not having, in tenths.
  ///
  /// `100` under `125` is 1.25× and was called too thin to survive another platform's codegen;
  /// `64` under `125` is 1.9× and was accepted. Any default this crate ships must clear its
  /// binding cell by at least the accepted figure, and stating it as a constant is what makes the
  /// next person to move a default move this line too.
  ///
  /// **A margin against a stack, so it exists only when a stack is what bounds the default.** With
  /// the `stacker` feature on there is no native ceiling for the default to keep clear of, and a
  /// figure kept alive there would be a number with nothing to compare against — which is how a
  /// constant starts meaning two things.
  #[cfg(not(feature = "stacker"))]
  pub(crate) const MIN_MARGIN_TENTHS: usize = 19;
}

/// The derivation of [`RecursionLimiter::PARSE_DEFAULT_DEPTH`], enforced by the compiler.
///
/// These were `#[test]`s for one revision and clippy was right to refuse them: every operand is a
/// constant, so the question is settled at compile time and a *test* is the weaker place to settle
/// it. As `const` assertions a tree whose default contradicts its own measurement does not build,
/// which is a stronger guarantee than one whose test suite reddens — and it holds in every leg,
/// including the ones that never run a test.
///
/// The cost is the message: `panic!` in a const context takes a string literal and no arguments, so
/// these cannot print the numbers they compared. They name what to go and read instead.
const _: () = {
  use measured::{CONSUMER_ABORTS_AT, CONSUMER_BYTES_PER_LEVEL, TOKORA_ABORTS_AT};

  let default = RecursionLimiter::PARSE_DEFAULT_DEPTH;

  #[cfg(not(feature = "stacker"))]
  {
    use measured::MIN_MARGIN_TENTHS;

    // THE WHOLE DEFECT, AS ONE INEQUALITY. `PARSE_DEFAULT_DEPTH` was 64 and `CONSUMER_ABORTS_AT`
    // is 51, so this assertion fails on the tree that shipped it: the native stack got there
    // before the limiter could, and the refusal the budget exists to produce was unreachable.
    assert!(
      default * MIN_MARGIN_TENTHS <= CONSUMER_ABORTS_AT * 10,
      "PARSE_DEFAULT_DEPTH does not clear CONSUMER_ABORTS_AT — the depth at which a measured \
       consumer grammar aborts on a 2 MiB thread — by MIN_MARGIN_TENTHS, the margin this crate \
       already requires of such a number. See PARSE_DEFAULT_DEPTH's derivation."
    );
    // A guard against the derivation being 'repaired' by re-reading the wrong row: sizing against
    // tokora's own 125 admits 64, which is exactly the number that was wrong.
    assert!(
      TOKORA_ABORTS_AT > CONSUMER_ABORTS_AT,
      "the two measured rows have stopped disagreeing, which means one was edited to match the \
       other rather than re-measured"
    );
    assert!(
      default <= CONSUMER_ABORTS_AT * 10 / MIN_MARGIN_TENTHS,
      "PARSE_DEFAULT_DEPTH is inside what tokora's own row admits but outside what the consumer \
       row does; that is the original defect, not a smaller version of it"
    );
    // And the other direction, read as bytes: a default that clears the abort point by being
    // absurdly small is not an improvement either.
    assert!(
      default * CONSUMER_BYTES_PER_LEVEL < measured::STACK,
      "an unconfigured parse can reach more native stack than the 2 MiB the derivation is \
       stated on"
    );
  }

  #[cfg(feature = "stacker")]
  {
    // With the feature on the ceiling is a policy choice about memory, so the guards are about
    // memory. The default must buy a depth the thread stack could not have — otherwise the
    // feature is paying for a ceiling it did not raise — while still costing an amount an
    // unconfigured parse may reasonably reach.
    assert!(
      default > CONSUMER_ABORTS_AT * 4,
      "the `stacker` PARSE_DEFAULT_DEPTH is not meaningfully past what a 2 MiB thread already \
       gave that grammar, so the feature raised no ceiling"
    );
    assert!(
      default * CONSUMER_BYTES_PER_LEVEL <= 64 * 1024 * 1024,
      "an unconfigured parse can reach more stack-segment memory before the budget refuses it \
       than PARSE_DEFAULT_DEPTH's derivation claims"
    );
    // Past BOTH measured rows, not just the consumer one. The consumer row is what the
    // stack-bounded default is derived from, but the claim this feature makes is that neither
    // row bounds it any more, and tokora's own is the larger of the two.
    assert!(
      default > TOKORA_ABORTS_AT * 4,
      "the `stacker` PARSE_DEFAULT_DEPTH does not clear tokora's own measured ceiling by enough \
       to be a policy figure rather than a stack one"
    );
  }
};

impl RecursionLimiter {
  /// tokora's own recursion budget for a Pratt-driven parse — requested explicitly by
  /// [`ParserContext`](crate::ParserContext) and the input layer instead of inherited from
  /// [`new`](Self::new), and **not one number**:
  ///
  /// | build | value | what bounds it |
  /// |---|---|---|
  /// | default | **16** | the native stack of a 2 MiB thread, for a *consumer's* frames |
  /// | `stacker` feature on | **1024** | a policy choice about heap, because the stack no longer decides |
  ///
  /// A single constant that silently meant two things is how the defect below happened, so the
  /// two are stated apart and this one says which is active. Read it rather than assuming:
  /// enabling a feature changes it.
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
  /// or by enabling `stacker`. That asymmetry is deliberate and is the same one the type's docs
  /// argue: too low returns a clean, catchable
  /// [`RecursionLimitReached`](crate::error::RecursionLimitReached) naming the knob that raises it;
  /// too high aborts the process and takes the caller's whole program with it. Only one of the two
  /// can be recovered from.
  ///
  /// # Why the `stacker` default is 1024, and what it is NOT
  ///
  /// With the `stacker` feature on, a pratt frame inside the red zone of its stack continues on a
  /// fresh heap segment, so exceeding the thread stack is no longer fatal and the ceiling stops
  /// being a hardware constant. It becomes a **policy choice about memory**: at the binding ~41
  /// KiB per level, 1024 levels is ≈41 MiB of segments — generous for an unconfigured parse,
  /// bounded, and reached by no realistic document. It is also 20× the depth at which that same
  /// grammar aborts *without* the feature, which is the whole of what the feature buys.
  ///
  /// **It does not make depth unlimited, and this constant is not decoration under it.** A segment
  /// is a heap allocation; stacker's allocator is `mmap` plus an assertion, not a fallible
  /// reserve; so a deep enough input still ends the process, with nothing on any `Result` channel
  /// for a caller to catch and nothing for
  /// [`MaybeTerminal`](crate::error::MaybeTerminal) to report. Measured, with the feature on and
  /// the limiter set to [`unlimited`](Self::unlimited): the parse returns `Ok` past every depth a
  /// thread stack could have held, and the run ends when the machine's memory does.
  ///
  /// So **the budget is what makes a too-deep input refusable, and `stacker` is not a substitute
  /// for it.** A future reader concluding this constant is now redundant, and deleting the
  /// [`InputRef::descend`](crate::InputRef::descend) that spends it, restores the abort both
  /// numbers exist to delete.
  ///
  /// # One item, two values
  ///
  /// Written as a `cfg!` inside the initializer rather than as two `#[cfg]`-gated constants,
  /// because rustdoc documents only the arm a given build selects: a `#[cfg]` pair would publish
  /// whichever half `--all-features` happens to enable and leave everything above this line
  /// invisible in the other configuration. One item is documented in both.
  pub const PARSE_DEFAULT_DEPTH: usize = if cfg!(feature = "stacker") { 1024 } else { 16 };

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
