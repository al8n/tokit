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
/// # Default Limit
///
/// The default maximum depth is **64**, and it is sized against the **tightest measured
/// configuration**, not the most generous one.
///
/// Measured on this tree, one pratt frame per level of nesting, bisected to the last depth that
/// completes on an explicitly sized **2 MiB** thread — the stack Rust gives every
/// `std::thread::spawn` and every libtest harness thread, and so the smallest stack a parse is
/// likely to get — before the native stack aborts the process. All four cells were measured, and
/// all four are what the default has to clear:
///
/// | build | typed driver | token driver |
/// |---|---|---|
/// | release (`opt-level = 3`), 2 MiB thread | **3871** frames, ~0.53 KiB each | **4247** frames, ~0.48 KiB each |
/// | debug (`opt-level = 0`), 2 MiB thread | **384** frames, ~5.3 KiB each | **125** frames, ~16.4 KiB each |
///
/// The binding cell is the **debug token driver at 125**, not the release figures above it. Sizing
/// a default against the release ceiling was the mistake this number corrects: an unconfigured
/// parse in a debug build — which is what every test suite runs — reached the native stack before
/// the limiter, and the two failure modes are not symmetric. A limit that is too *low* returns a
/// clean, catchable, documented [`RecursionLimitReached`](crate::error::RecursionLimitReached)
/// telling the caller to raise it. A limit that is too *high* aborts the process with no
/// diagnostic at all and takes the whole suite with it. Only one of those can be recovered from,
/// so the default is set where **every** measured configuration survives.
///
/// 64 is the largest power of two leaving roughly **1.9×** margin under 125, and it clears the
/// debug typed driver by 6×, the release typed driver by 60× and the release token driver by 66×.
/// It also sits far above any realistic nesting depth in a GraphQL document or an arithmetic
/// expression, so raising it is a deliberate act rather than a routine one.
///
/// **A grammar that parses untrusted, deeply nested input should still set its own limit** with
/// [`with_limitation`](Self::with_limitation) rather than inherit this default — especially on a
/// worker thread, where the stack is 2 MiB and not the main thread's 8 MiB. The figures above are
/// this crate's pratt frames on one platform and one toolchain; a grammar whose own recursive
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
/// and defaulting to [`new`](Self::new) — depth **64**, on by default. Both Pratt engines enter
/// one level per live frame through [`InputRef::descend`](crate::InputRef::descend), whose
/// [`Descent`](crate::input::Descent) guard releases the level on every exit including an
/// unwind; exceeding the limit fails the parse with the always-terminal
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached). The budget is *per input*,
/// not per parser, so two pratt parsers composed into one grammar share one depth budget. A
/// hand-written recursive combinator draws on the same cell through
/// [`InputRef::descending`](crate::InputRef::descending), which is the guard's scoped form and
/// the one to prefer.
///
/// It is deliberately **not** part of the checkpoint set: depth is a fact about the control
/// stack, and a rollback happens at the same frame depth as the save it returns to.
///
/// Separately, `RecursionLimiter` can also be used as part of a Logos lexer's `Extras` state by
/// implementing the [`State`] trait, allowing you to track nesting during *lexing*. That is a
/// different cell with a different subject: the lexer's tally is monotone in the input and its
/// trip latches the poison boundary, while the parse's descent unwinds.
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

impl RecursionLimiter {
  /// Creates a new recursion tracker.
  ///
  /// Defaults to a maximum depth of 64 — see the type's `Default Limit` section for why that
  /// number and not a larger one.
  #[inline(always)]
  pub const fn new() -> Self {
    Self {
      max: 64,
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
  {
    bail!(logos_0_14);
  }

  #[cfg(feature = "logos_0_15")]
  #[cfg_attr(docsrs, doc(cfg(feature = "logos_0_15")))]
  {
    bail!(logos_0_15);
  }

  #[cfg(feature = "logos_0_16")]
  #[cfg_attr(docsrs, doc(cfg(feature = "logos_0_16")))]
  {
    bail!(logos_0_16);
  }
};

#[cfg(test)]
mod tests;
