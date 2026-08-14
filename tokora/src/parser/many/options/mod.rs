use super::{Apply, With};

pub use allow_leading::AllowLeading;
pub use allow_trailing::AllowTrailing;
pub use at_least::*;
pub use at_most::*;
pub use bounded::*;
pub use require_leading::RequireLeading;
pub use require_trailing::RequireTrailing;

mod allow_leading;
mod allow_trailing;
mod at_least;
mod at_most;
mod bounded;
mod require_leading;
mod require_trailing;

/// A marker type representing the maximum number of elements allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Maximum(usize);

impl Maximum {
  /// The maximum possible value for `Maximum`.
  pub const MAX: Self = Self::new(usize::MAX);

  /// Creates a new `Maximum`.
  #[inline(always)]
  pub const fn new(n: usize) -> Self {
    Self(n)
  }

  /// Returns the maximum number of elements allowed.
  #[inline(always)]
  pub const fn get(&self) -> usize {
    self.0
  }
}

/// A marker type representing the minimum number of elements required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Minimum(usize);

impl Minimum {
  /// The minimum possible value for `Minimum`.
  pub const MIN: Self = Self::new(0);

  /// Creates a new `Minimum`.
  #[inline(always)]
  pub const fn new(n: usize) -> Self {
    Self(n)
  }

  /// Returns the minimum number of elements required.
  #[inline(always)]
  pub const fn get(&self) -> usize {
    self.0
  }
}

pub(super) struct Unbounded;

#[allow(warnings)]
#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
