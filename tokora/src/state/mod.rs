use core::convert::Infallible;

/// Trackers for preventing infinite recursion in parsers.
pub mod recursion_tracker;
/// A token tracker for tracking tokens in a lexer.
pub mod token_tracker;
/// A tracker for tracking recursion depth and tokens.
pub mod tracker;

/// The state trait for lexers
pub trait State: core::fmt::Debug + Clone {
  /// The error type of the state.
  type Error: Clone;

  /// Checks the state for errors.
  ///
  /// Not to be confused with [`Check::check`](crate::Check::check) (the parser-side value
  /// predicate) or [`Lexer::check`](crate::Lexer::check) (the lexer-level probe that
  /// typically wraps this one into the token's error type).
  fn check(&self) -> Result<(), Self::Error>;
}

impl State for () {
  type Error = ();

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

impl State for Infallible {
  type Error = Infallible;

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(feature = "std")]
mod tests;
