use crate::State;

/// Error returned when token count exceeds the configured limit.
///
/// This error provides context about both the actual token count reached
/// and the maximum tokens allowed, making it easy to diagnose whether the limit
/// needs adjustment or if there's a genuine DoS attack attempt.
///
/// # Example
///
/// ```rust
/// use tokora::state::token_tracker::{TokenLimiter, TokenLimitExceeded};
///
/// let mut limiter = TokenLimiter::with_limitation(1000);
///
/// // Simulate processing many tokens
/// for _ in 0..1500 {
///     limiter.increase();
/// }
///
/// match limiter.check() {
///     Err(error) => {
///         eprintln!("Token limit exceeded!");
///         eprintln!("Tokens processed: {}", error.tokens());
///         eprintln!("Maximum allowed: {}", error.limitation());
///     }
///     Ok(_) => unreachable!(),
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("token limit exceeded: tokens {}, maximum {}", .0.tokens(), .0.limitation())]
pub struct TokenLimitExceeded(TokenLimiter);

impl TokenLimitExceeded {
  /// Returns the actual token count that triggered the error.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let mut limiter = TokenLimiter::with_limitation(10);
  /// for _ in 0..15 {
  ///     limiter.increase();
  /// }
  ///
  /// if let Err(error) = limiter.check() {
  ///     println!("Processed {} tokens", error.tokens());
  /// }
  /// ```
  #[inline(always)]
  pub const fn tokens(&self) -> usize {
    self.0.tokens()
  }

  /// Returns the maximum token count that was configured.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let mut limiter = TokenLimiter::with_limitation(10);
  /// for _ in 0..15 {
  ///     limiter.increase();
  /// }
  ///
  /// if let Err(error) = limiter.check() {
  ///     println!("Maximum tokens allowed: {}", error.limitation());
  /// }
  /// ```
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.0.limitation()
  }
}

/// A token counter that prevents DoS attacks by limiting total token count.
///
/// `TokenLimiter` helps protect parsers against denial-of-service attacks by tracking
/// the total number of tokens processed and enforcing a maximum token count. This is
/// essential for preventing attackers from exhausting system resources with extremely
/// large or deeply nested inputs.
///
/// # Default Limit
///
/// The default configuration is **unlimited** (`usize::MAX`), which means you must
/// explicitly set a limit using [`with_limitation`](Self::with_limitation) to enable
/// protection.
///
/// # Use Cases
///
/// - **Web APIs**: Limit token count when parsing untrusted user input
/// - **File parsers**: Prevent memory exhaustion from maliciously large files
/// - **Expression evaluators**: Cap complexity of user-provided expressions
/// - **Stateful lexers**: Track token count in the lexer's `Extras` state
///
/// # Integration with tokora
///
/// `TokenLimiter` can be used as part of a Logos lexer's `Extras` state by
/// implementing the [`State`] trait, allowing you to track token
/// counts during lexing.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::state::token_tracker::TokenLimiter;
///
/// let mut limiter = TokenLimiter::with_limitation(1000);
///
/// limiter.increase(); // Process a token
/// assert_eq!(limiter.tokens(), 1);
///
/// limiter.increase(); // Process another
/// assert_eq!(limiter.tokens(), 2);
/// ```
///
/// ## Checking Limits
///
/// ```rust
/// use tokora::state::token_tracker::TokenLimiter;
///
/// let mut limiter = TokenLimiter::with_limitation(5);
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
/// ## Lexer Integration Example
///
/// ```rust,ignore
/// use logos::Logos;
/// use tokora::state::token_tracker::TokenLimiter;
///
/// #[derive(Default)]
/// struct LexerState {
///     token_limiter: TokenLimiter,
/// }
///
/// #[derive(Logos, Debug)]
/// #[logos(extras = LexerState)]
/// enum Token {
///     #[regex(r"[a-zA-Z]+", |lex| {
///         lex.extras.token_limiter.increase();
///         lex.extras.token_limiter.check().ok()
///     })]
///     Word(()),
///
///     #[regex(r"[0-9]+", |lex| {
///         lex.extras.token_limiter.increase();
///         lex.extras.token_limiter.check().ok()
///     })]
///     Number(()),
/// }
/// ```
///
/// ## DoS Protection Pattern
///
/// ```rust,ignore
/// use tokora::state::token_tracker::TokenLimiter;
///
/// fn parse_untrusted_input(input: &str, max_tokens: usize) -> Result<AST, Error> {
///     let mut limiter = TokenLimiter::with_limitation(max_tokens);
///
///     for token in tokenize(input) {
///         limiter.increase();
///         limiter.check()?; // Fail fast if limit exceeded
///
///         // ... process token ...
///     }
///
///     Ok(ast)
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenLimiter {
  max: usize,
  current: usize,
}

impl Default for TokenLimiter {
  #[inline(always)]
  fn default() -> Self {
    Self::new()
  }
}

impl TokenLimiter {
  /// Creates a new token tracker without limitation (effectively unlimited).
  ///
  /// The maximum token count is set to `usize::MAX`, so the limiter will never
  /// trigger unless you process an impossibly large number of tokens.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let limiter = TokenLimiter::new();
  /// assert_eq!(limiter.limitation(), usize::MAX);
  /// assert_eq!(limiter.tokens(), 0);
  /// ```
  #[inline(always)]
  pub const fn new() -> Self {
    Self {
      max: usize::MAX,
      current: 0,
    }
  }

  /// Creates a new token tracker with the given maximum number of tokens.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let limiter = TokenLimiter::with_limitation(1000);
  /// assert_eq!(limiter.limitation(), 1000);
  /// ```
  #[inline(always)]
  pub const fn with_limitation(max: usize) -> Self {
    Self { max, current: 0 }
  }

  /// Returns the current number of tokens tracked.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let mut limiter = TokenLimiter::new();
  /// limiter.increase();
  /// limiter.increase();
  /// assert_eq!(limiter.tokens(), 2);
  /// ```
  #[inline(always)]
  pub const fn tokens(&self) -> usize {
    self.current
  }

  /// Increases the token count by one.
  ///
  /// This should be called each time a token is processed. Saturates at
  /// `usize::MAX` rather than overflowing.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let mut limiter = TokenLimiter::new();
  /// assert_eq!(limiter.tokens(), 0);
  ///
  /// limiter.increase();
  /// assert_eq!(limiter.tokens(), 1);
  /// ```
  #[inline(always)]
  pub const fn increase(&mut self) {
    self.current = self.current.saturating_add(1);
  }

  /// Returns the maximum number of tokens allowed.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let limiter = TokenLimiter::with_limitation(500);
  /// assert_eq!(limiter.limitation(), 500);
  /// ```
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.max
  }

  /// Increases the token count by one.
  ///
  /// This is an alias for [`increase`](Self::increase) provided for API consistency
  /// with [`RecursionLimiter::increase_recursion`](super::recursion_tracker::RecursionLimiter::increase_recursion).
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let mut limiter = TokenLimiter::new();
  /// limiter.increase_token();
  /// assert_eq!(limiter.tokens(), 1);
  /// ```
  #[inline(always)]
  pub const fn increase_token(&mut self) {
    self.increase();
  }

  /// Checks if the token limit has been exceeded.
  ///
  /// Returns `Ok(())` if within limits, or `Err(TokenLimitExceeded)` if the
  /// token count exceeds the configured maximum.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::token_tracker::TokenLimiter;
  ///
  /// let mut limiter = TokenLimiter::with_limitation(3);
  ///
  /// limiter.increase();
  /// limiter.increase();
  /// assert!(limiter.check().is_ok());
  ///
  /// limiter.increase();
  /// assert!(limiter.check().is_ok()); // Still at limit
  ///
  /// limiter.increase();
  /// assert!(limiter.check().is_err()); // Exceeded!
  /// ```
  #[inline(always)]
  pub fn check(&self) -> Result<(), TokenLimitExceeded> {
    if self.tokens() > self.limitation() {
      Err(TokenLimitExceeded(*self))
    } else {
      Ok(())
    }
  }
}

impl State for TokenLimiter {
  type Error = TokenLimitExceeded;

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    <Self as TokenTracker>::check(self)
  }
}

/// A token tracker trait.
pub trait TokenTracker {
  /// The error type returned when the token limit is exceeded.
  type Error;

  /// Increases the token count by one.
  fn increase(&mut self);

  /// Checks if the token limit has been exceeded.
  fn check(&self) -> Result<(), Self::Error>
  where
    Self: Sized;
}

impl TokenTracker for TokenLimiter {
  type Error = TokenLimitExceeded;

  #[inline(always)]
  fn increase(&mut self) {
    self.current = self.current.saturating_add(1);
  }

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    if self.tokens() > self.limitation() {
      Err(TokenLimitExceeded(*self))
    } else {
      Ok(())
    }
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

      impl<'a, T> TokenTracker for Lexer<'a, T>
      where
        T: Logos<'a>,
        T::Extras: TokenTracker,
      {
        type Error = <T::Extras as TokenTracker>::Error;

        #[inline(always)]
        fn increase(&mut self) {
          self.extras.increase();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.extras.check()
        }
      }

      impl<'a, T> TokenTracker for LogosLexer<'a, T>
      where
        T: FromLogos<'a> + Token<'a>,
        <T::Logos as Logos<'a>>::Extras: TokenTracker,
      {
        type Error = <<T::Logos as Logos<'a>>::Extras as TokenTracker>::Error;

        #[inline(always)]
        fn increase(&mut self) {
          self.inner_mut().extras.increase();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.inner().extras.check()
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
