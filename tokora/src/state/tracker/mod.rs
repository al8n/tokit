use super::{
  State,
  recursion_tracker::{RecursionLimitExceeded, RecursionLimiter, RecursionTracker},
  token_tracker::{TokenLimitExceeded, TokenLimiter, TokenTracker},
};

/// Error returned when either token or recursion limits are exceeded.
///
/// This enum combines both [`TokenLimitExceeded`] and [`RecursionLimitExceeded`]
/// errors, making it easy to handle both limit types uniformly when using
/// the [`Limiter`] type.
///
/// # Variants
///
/// - **Token**: The token count limit was exceeded
/// - **Recursion**: The recursion depth limit was exceeded
///
/// # Derived Helpers
///
/// This type provides several helper methods via derive macros:
/// - `is_token()` / `is_recursion()`: Check which variant it is
/// - `unwrap_token()` / `unwrap_recursion()`: Extract the inner error (panics if wrong variant)
/// - `try_unwrap_token()` / `try_unwrap_recursion()`: Try to extract the inner error
///
/// # Examples
///
/// ## Pattern Matching
///
/// ```rust
/// use tokora::state::tracker::{Limiter, LimitExceeded};
///
/// let mut tracker = Limiter::new();
/// // ... use tracker ...
///
/// match tracker.check() {
///     Ok(_) => println!("All limits OK"),
///     Err(LimitExceeded::Token(e)) => {
///         eprintln!("Token limit exceeded: {}", e);
///     }
///     Err(LimitExceeded::Recursion(e)) => {
///         eprintln!("Recursion limit exceeded: {}", e);
///     }
///     Err(_) => { eprintln!("Unknown limit exceeded"); }
/// }
/// ```
///
/// ## Using Derived Methods
///
/// ```rust
/// use tokora::state::tracker::{Limiter, LimitExceeded};
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// let mut tracker = Limiter::with_recursion_tracker(
///     RecursionLimiter::with_limitation(2)
/// );
///
/// tracker.increase_recursion();
/// tracker.increase_recursion();
/// tracker.increase_recursion(); // Exceeds limit
///
/// if let Err(error) = tracker.check() {
///     let error: LimitExceeded = error;
///     assert!(error.is_recursion());
///     let recursion_error = error.unwrap_recursion();
///     assert_eq!(recursion_error.depth(), 3);
/// }
/// ```
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  thiserror::Error,
  derive_more::IsVariant,
  derive_more::Unwrap,
  derive_more::TryUnwrap,
)]
#[unwrap(ref)]
#[try_unwrap(ref)]
#[non_exhaustive]
pub enum LimitExceeded {
  /// The token limit has been exceeded.
  #[error(transparent)]
  Token(#[from] TokenLimitExceeded),
  /// The recursion limit has been exceeded.
  #[error(transparent)]
  Recursion(#[from] RecursionLimitExceeded),
}

/// A combined limiter that tracks both token count and recursion depth.
///
/// `Limiter` brings together [`TokenLimiter`] and [`RecursionLimiter`] into a single
/// type, providing comprehensive protection against both DoS attacks (via token limiting)
/// and stack overflow (via recursion limiting). This is the recommended choice for
/// production parsers that need robust safety guarantees.
///
/// # Components
///
/// 1. **Token Limiter**: Tracks total number of tokens processed
/// 2. **Recursion Limiter**: Tracks current recursion depth
///
/// Both limits are checked simultaneously by the [`check`](Self::check) method, which
/// returns an error if either limit is exceeded.
///
/// # Default Configuration
///
/// - **Token limit**: Unlimited (`usize::MAX`)
/// - **Recursion limit**: 500 — [`RecursionLimiter::new`]'s own general-purpose ceiling, with no
///   assumption about what one level of recursion costs. This is deliberately NOT the 64
///   tokora's own Pratt-parser wiring uses internally; see `Integration with tokora` below.
///
/// You typically want to configure at least the token limit using
/// [`with_token_tracker`](Self::with_token_tracker), and the recursion limit using
/// [`with_recursion_tracker`](Self::with_recursion_tracker) or
/// [`with_trackers`](Self::with_trackers) if 500 does not suit what a level of your own
/// recursion costs.
///
/// # Integration with tokora
///
/// `Limiter` implements the [`State`] trait and can be used directly as a Logos lexer's
/// `Extras` state, providing automatic limit checking during lexing — a position where nesting
/// costs no native stack. That is exactly why its recursion component defaults to the
/// general-purpose **500** and not the
/// [`PARSE_DEFAULT_DEPTH`](crate::state::recursion_tracker::RecursionLimiter::PARSE_DEFAULT_DEPTH)
/// tokora's own Pratt-parser wiring uses: tokora's
/// [`ParserContext`](crate::ParserContext) and [`InputContext`](crate::input::InputContext)
/// never build their recursion budget through `Limiter` — each holds a [`RecursionLimiter`]
/// directly and requests `PARSE_DEFAULT_DEPTH` explicitly instead of inheriting either type's
/// default — so
/// `Limiter` does not presume a caller reaching for it directly is on that path. If you
/// assemble your own recursive-descent parser around `Limiter` and a level of it does cost a
/// native stack frame, size the recursion component for that cost yourself with
/// [`with_recursion_tracker`](Self::with_recursion_tracker) rather than inherit 500.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::state::tracker::Limiter;
///
/// let mut tracker = Limiter::new();
///
/// // Track token processing
/// tracker.increase_token();
/// assert_eq!(tracker.token().tokens(), 1);
///
/// // Track recursion depth
/// tracker.increase_recursion();
/// assert_eq!(tracker.recursion().depth(), 1);
///
/// tracker.decrease_recursion();
/// assert_eq!(tracker.recursion().depth(), 0);
/// ```
///
/// ## Configuring Limits
///
/// ```rust
/// use tokora::state::tracker::Limiter;
/// use tokora::state::token_tracker::TokenLimiter;
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// let tracker = Limiter::with_trackers(
///     TokenLimiter::with_limitation(10000),
///     RecursionLimiter::with_limitation(100)
/// );
///
/// assert_eq!(tracker.token().limitation(), 10000);
/// assert_eq!(tracker.recursion().limitation(), 100);
/// ```
///
/// ## Checking Limits
///
/// ```rust
/// use tokora::state::tracker::Limiter;
/// use tokora::state::token_tracker::TokenLimiter;
///
/// let mut tracker = Limiter::with_token_tracker(
///     TokenLimiter::with_limitation(5)
/// );
///
/// for _ in 0..5 {
///     tracker.increase_token();
///     assert!(tracker.check().is_ok());
/// }
///
/// tracker.increase_token(); // Exceeds limit
/// assert!(tracker.check().is_err());
/// ```
///
/// ## Lexer Integration
///
/// ```rust,ignore
/// use logos::Logos;
/// use tokora::state::tracker::Limiter;
/// use tokora::state::token_tracker::TokenLimiter;
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// #[derive(Default)]
/// struct LexerState {
///     tracker: Limiter,
/// }
///
/// impl LexerState {
///     fn new() -> Self {
///         Self {
///             tracker: Limiter::with_trackers(
///                 TokenLimiter::with_limitation(10000),
///                 RecursionLimiter::with_limitation(500),
///             ),
///         }
///     }
/// }
///
/// #[derive(Logos)]
/// #[logos(extras = LexerState)]
/// enum Token {
///     #[regex(r"[a-zA-Z]+", |lex| {
///         lex.extras.tracker.increase_token();
///         lex.extras.tracker.check().ok()
///     })]
///     Word(()),
///
///     #[regex(r"\(", |lex| {
///         lex.extras.tracker.increase_token();
///         lex.extras.tracker.increase_recursion();
///         lex.extras.tracker.check().ok()
///     })]
///     LParen(()),
///
///     #[regex(r"\)", |lex| {
///         lex.extras.tracker.increase_token();
///         lex.extras.tracker.decrease_recursion();
///         Some(())
///     })]
///     RParen,
/// }
/// ```
///
/// ## Parser Integration
///
/// ```rust,ignore
/// use tokora::state::tracker::Limiter;
///
/// struct Parser {
///     tracker: Limiter,
/// }
///
/// impl Parser {
///     fn parse_expr(&mut self, input: &str) -> Result<Expr, Error> {
///         self.tracker.increase_recursion();
///         self.tracker.increase_token();
///         self.tracker.check()?; // Check both limits
///
///         let result = match input.chars().next() {
///             Some('(') => {
///                 let nested = self.parse_expr(&input[1..])?;
///                 Expr::Paren(Box::new(nested))
///             }
///             Some(c) if c.is_numeric() => {
///                 Expr::Number(c.to_digit(10).unwrap())
///             }
///             _ => return Err(Error::Unexpected),
///         };
///
///         self.tracker.decrease_recursion();
///         Ok(result)
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Limiter {
  token_tracker: TokenLimiter,
  recursion_tracker: RecursionLimiter,
}

impl Default for Limiter {
  #[inline(always)]
  fn default() -> Self {
    Self::new()
  }
}

impl Limiter {
  /// Creates a new tracker with default limits.
  ///
  /// - Token limit: Unlimited (`usize::MAX`)
  /// - Recursion limit: 500 — [`RecursionLimiter::new`]'s general-purpose ceiling, NOT tokora's
  ///   parser-facing 64; see the type's `Default Configuration` docs.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let tracker = Limiter::new();
  /// assert_eq!(tracker.recursion().limitation(), 500);
  /// assert_eq!(tracker.token().limitation(), usize::MAX);
  /// ```
  #[inline(always)]
  pub const fn new() -> Self {
    Self::with_trackers(TokenLimiter::new(), RecursionLimiter::new())
  }

  /// Creates a new tracker with the given token limiter and default (general-purpose, 500)
  /// recursion limiter — see the type's `Default Configuration` docs.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let tracker = Limiter::with_token_tracker(
  ///     TokenLimiter::with_limitation(10000)
  /// );
  ///
  /// assert_eq!(tracker.token().limitation(), 10000);
  /// assert_eq!(tracker.recursion().limitation(), 500);
  /// ```
  #[inline(always)]
  pub const fn with_token_tracker(token_tracker: TokenLimiter) -> Self {
    Self::with_trackers(token_tracker, RecursionLimiter::new())
  }

  /// Creates a new tracker with the given recursion limiter and default token limiter.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  /// use tokora::state::recursion_tracker::RecursionLimiter;
  ///
  /// let tracker = Limiter::with_recursion_tracker(
  ///     RecursionLimiter::with_limitation(100)
  /// );
  ///
  /// assert_eq!(tracker.recursion().limitation(), 100);
  /// assert_eq!(tracker.token().limitation(), usize::MAX);
  /// ```
  #[inline(always)]
  pub const fn with_recursion_tracker(recursion_tracker: RecursionLimiter) -> Self {
    Self::with_trackers(TokenLimiter::new(), recursion_tracker)
  }

  /// Creates a new tracker with the given token and recursion limiters.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  /// use tokora::state::token_tracker::TokenLimiter;
  /// use tokora::state::recursion_tracker::RecursionLimiter;
  ///
  /// let tracker = Limiter::with_trackers(
  ///     TokenLimiter::with_limitation(5000),
  ///     RecursionLimiter::with_limitation(200)
  /// );
  ///
  /// assert_eq!(tracker.token().limitation(), 5000);
  /// assert_eq!(tracker.recursion().limitation(), 200);
  /// ```
  #[inline(always)]
  pub const fn with_trackers(
    token_tracker: TokenLimiter,
    recursion_tracker: RecursionLimiter,
  ) -> Self {
    Self {
      token_tracker,
      recursion_tracker,
    }
  }

  /// Returns a reference to the token limiter.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let tracker = Limiter::new();
  /// assert_eq!(tracker.token().tokens(), 0);
  /// ```
  #[inline(always)]
  pub const fn token(&self) -> &TokenLimiter {
    &self.token_tracker
  }

  /// Returns a mutable reference to the token limiter.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let mut tracker = Limiter::new();
  /// tracker.token_mut().increase();
  /// assert_eq!(tracker.token().tokens(), 1);
  /// ```
  #[inline(always)]
  pub const fn token_mut(&mut self) -> &mut TokenLimiter {
    &mut self.token_tracker
  }

  /// Returns a reference to the recursion limiter.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let tracker = Limiter::new();
  /// assert_eq!(tracker.recursion().depth(), 0);
  /// ```
  #[inline(always)]
  pub const fn recursion(&self) -> &RecursionLimiter {
    &self.recursion_tracker
  }

  /// Returns a mutable reference to the recursion limiter.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let mut tracker = Limiter::new();
  /// tracker.recursion_mut().increase();
  /// assert_eq!(tracker.recursion().depth(), 1);
  /// ```
  #[inline(always)]
  pub const fn recursion_mut(&mut self) -> &mut RecursionLimiter {
    &mut self.recursion_tracker
  }

  /// Increases the token count by one.
  ///
  /// This should be called each time a token is processed.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let mut tracker = Limiter::new();
  /// tracker.increase_token();
  /// assert_eq!(tracker.token().tokens(), 1);
  /// ```
  #[inline(always)]
  pub const fn increase_token(&mut self) {
    self.token_mut().increase();
  }

  /// Increases the recursion depth by one.
  ///
  /// This should be called when entering a recursive function.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let mut tracker = Limiter::new();
  /// tracker.increase_recursion();
  /// assert_eq!(tracker.recursion().depth(), 1);
  /// ```
  #[inline(always)]
  pub const fn increase_recursion(&mut self) {
    self.recursion_mut().increase();
  }

  /// Decreases the recursion depth by one.
  ///
  /// This should be called when returning from a recursive function.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  ///
  /// let mut tracker = Limiter::new();
  /// tracker.increase_recursion();
  /// tracker.decrease_recursion();
  /// assert_eq!(tracker.recursion().depth(), 0);
  /// ```
  #[inline(always)]
  pub const fn decrease_recursion(&mut self) {
    self.recursion_mut().decrease();
  }

  /// Checks if any of the limits have been exceeded.
  ///
  /// Returns `Ok(())` if both limits are within bounds, or `Err(LimitExceeded)`
  /// if either the token count or recursion depth exceeds its configured maximum.
  ///
  /// The recursion limit is checked first, so if both limits are exceeded, you'll
  /// get a `LimitExceeded::Recursion` error.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::tracker::Limiter;
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let mut tracker = Limiter::with_token_tracker(
  ///     TokenLimiter::with_limitation(3)
  /// );
  ///
  /// tracker.increase_token();
  /// tracker.increase_token();
  /// assert!(tracker.check().is_ok());
  ///
  /// tracker.increase_token();
  /// tracker.increase_token(); // Exceeds limit
  /// assert!(tracker.check().is_err());
  /// ```
  #[inline(always)]
  pub fn check(&self) -> Result<(), LimitExceeded> {
    self
      .recursion_tracker
      .check()
      .map_err(LimitExceeded::from)?;
    self.token_tracker.check().map_err(LimitExceeded::from)?;
    Ok(())
  }
}

impl State for Limiter {
  type Error = LimitExceeded;

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    <Self as Tracker>::check(self)
  }
}

impl RecursionTracker for Limiter {
  type Error = LimitExceeded;

  #[inline(always)]
  fn increase(&mut self) {
    self.recursion_tracker.increase();
  }

  #[inline(always)]
  fn decrease(&mut self) {
    self.recursion_tracker.decrease();
  }

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    self.recursion_tracker.check().map_err(Into::into)
  }
}

impl TokenTracker for Limiter {
  type Error = LimitExceeded;

  #[inline(always)]
  fn increase(&mut self) {
    self.token_tracker.increase();
  }

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    self.token_tracker.check().map_err(Into::into)
  }
}

/// A tracker that combines both token and recursion tracking.
///
/// This trait carries only the four *primitive* operations. The combined update-and-check
/// operations live on [`TrackerExt`], which is blanket-implemented for every `Tracker` and
/// therefore cannot be overridden — see that trait for why the composition is deliberately not
/// a customization point.
pub trait Tracker {
  /// The error type returned when either limit is exceeded.
  type Error;

  /// Increases the token count.
  fn increase_token(&mut self);

  /// Increases the recursion depth.
  fn increase_recursion(&mut self);

  /// Decreases the recursion depth.
  fn decrease_recursion(&mut self);

  /// Checks if any of the limits have been exceeded.
  ///
  /// "Any" is the whole contract: an implementation that tracks several resources must report
  /// the first one that is over, not merely the one its caller happened to touch last.
  fn check(&self) -> Result<(), Self::Error>;
}

/// The combined update-and-check operations over a [`Tracker`].
///
/// # Why these are not `Tracker`'s own provided methods
///
/// Each operation below is exactly *one or two primitive updates followed by
/// [`Tracker::check`]*, and there is no other thing it could correctly be: the only way an
/// override can differ from the composition is by checking **less** than the whole tracker, and
/// a limit check that silently narrows is a limit that does not hold.
///
/// That is not hypothetical. These were `Tracker`'s provided methods until tokora#265, and
/// [`Limiter`] — which implements [`RecursionTracker`], [`TokenTracker`] and [`Tracker`], each
/// with its own `check` — overrode two of them and disambiguated to the *wrong* trait's `check`.
/// A recursion depth already past its maximum then answered `Ok(())` through
/// `increase_token_and_check` and through `increase_token_and_decrease_recursion_and_check` for
/// as long as the token count held, and when both limits were over, the combined form
/// contradicted `Limiter::check` about which one to report.
///
/// A blanket impl is what makes that unrepresentable rather than merely fixed. Every call that
/// **resolves to this trait** — fully-qualified and UFCS calls, generic code bounded on `Tracker`,
/// and `dyn Tracker` — runs the blanket body; the blanket body always ends in
/// `<Self as Tracker>::check`; and coherence refuses every other impl, so no type can make these
/// operations check less — or other — than its own `check` does. Specialization stays available
/// where it is meaningful, on the four primitives.
///
/// Two doors stay open, and they are the language's and the contract's, not this trait's.
/// `Tracker::check` is a required method: these operations inherit whatever it answers, so its
/// "any limit" contract is the implementor's to keep — what became unwritable is checking
/// *less than it* in a combined step. And Rust resolves a concrete dot call against inherent
/// methods first: a type that defines its own inherent `increase_token_and_check` hands *concrete*
/// callers that body — silently; no rustc or clippy lint reports the shadow — exactly as it already
/// could against the old provided methods. Migrating an old override into an inherent method is
/// therefore the one repair that keeps the defect; delete the override instead.
///
/// # Which `check` runs
///
/// The one belonging to **this** trait's [`Tracker`] impl. On a type that also implements
/// [`RecursionTracker`] or [`TokenTracker`], those traits have their own narrower checks;
/// [`RecursionTracker`] additionally has its own combined operation. Reach for the trait whose
/// scope you want — [`TokenTracker`] has none to reach for.
pub trait TrackerExt: Tracker {
  /// Increase the token count and decrease recursion depth.
  fn increase_token_and_decrease_recursion(&mut self);

  /// Increases the token count and decreases the recursion depth, then checks **all** limits.
  ///
  /// The check runs after the decrement and reports what is still over, so a depth that the
  /// decrement leaves above its maximum is an error — leaving a frame does not by itself put
  /// the tracker back within bounds.
  fn increase_token_and_decrease_recursion_and_check(&mut self) -> Result<(), Self::Error>;

  /// Increases the token count, then checks **all** limits.
  ///
  /// Not just the token limit: a recursion depth that was already over when this was called is
  /// reported here.
  fn increase_token_and_check(&mut self) -> Result<(), Self::Error>;

  /// Increases the token count and the recursion depth.
  fn increase_both(&mut self);

  /// Increases the token count and the recursion depth, then checks **all** limits.
  fn increase_both_and_check(&mut self) -> Result<(), Self::Error>;
}

impl<T> TrackerExt for T
where
  T: Tracker + ?Sized,
{
  #[inline(always)]
  fn increase_token_and_decrease_recursion(&mut self) {
    self.increase_token();
    self.decrease_recursion();
  }

  #[inline(always)]
  fn increase_token_and_decrease_recursion_and_check(&mut self) -> Result<(), Self::Error> {
    self.increase_token_and_decrease_recursion();
    self.check()
  }

  #[inline(always)]
  fn increase_token_and_check(&mut self) -> Result<(), Self::Error> {
    self.increase_token();
    self.check()
  }

  #[inline(always)]
  fn increase_both(&mut self) {
    self.increase_token();
    self.increase_recursion();
  }

  #[inline(always)]
  fn increase_both_and_check(&mut self) -> Result<(), Self::Error> {
    self.increase_both();
    self.check()
  }
}

impl Tracker for Limiter {
  type Error = LimitExceeded;

  #[inline(always)]
  fn increase_token(&mut self) {
    self.token_tracker.increase();
  }

  #[inline(always)]
  fn increase_recursion(&mut self) {
    self.recursion_tracker.increase();
  }

  #[inline(always)]
  fn decrease_recursion(&mut self) {
    self.recursion_tracker.decrease();
  }

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    self
      .recursion_tracker
      .check()
      .map_err(LimitExceeded::from)?;
    self.token_tracker.check().map_err(LimitExceeded::from)?;
    Ok(())
  }
}

const _: () = {
  #[allow(dead_code, unused_macros)]
  macro_rules! bail {
    ($lib:ident) => {
      use $lib::{Lexer, Logos};

      use crate::{
        Token,
        lexer::$lib::{FromLogos, LogosLexer},
      };

      impl<'a, T> Tracker for Lexer<'a, T>
      where
        T: Logos<'a>,
        T::Extras: Tracker,
      {
        type Error = <T::Extras as Tracker>::Error;

        #[inline(always)]
        fn increase_token(&mut self) {
          self.extras.increase_token();
        }

        #[inline(always)]
        fn increase_recursion(&mut self) {
          self.extras.increase_recursion();
        }

        #[inline(always)]
        fn decrease_recursion(&mut self) {
          self.extras.decrease_recursion();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.extras.check()
        }
      }

      impl<'a, T> Tracker for LogosLexer<'a, T>
      where
        T: FromLogos<'a> + Token<'a>,
        <T::Logos as Logos<'a>>::Extras: Tracker,
      {
        type Error = <<T::Logos as Logos<'a>>::Extras as Tracker>::Error;

        #[inline(always)]
        fn increase_token(&mut self) {
          self.inner_mut().increase_token();
        }

        #[inline(always)]
        fn increase_recursion(&mut self) {
          self.inner_mut().increase_recursion();
        }

        #[inline(always)]
        fn decrease_recursion(&mut self) {
          self.inner_mut().decrease_recursion();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.inner().check()
        }
      }
    };
  }

  #[cfg(feature = "logos_0_16")]
  #[cfg_attr(docsrs, doc(cfg(feature = "logos_0_16")))]
  const _: () = {
    bail!(logos_0_16);
  };
};

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
