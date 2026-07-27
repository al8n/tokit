use crate::parser::{PrattLHS, PrattRHS};

use super::Token;

/// A trait for tokens that can run a pratt on them.
pub trait PrattToken<'a, Expr: ?Sized, Power = i64>: Token<'a> {
  /// Returns `true` if the token is kind of an operand token or a prefix token of the `Expr`.
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), Power>>;

  /// Returns `Some(rhs)` if the token is an infix or postfix operator of the `Expr`,
  /// or `None` if it is not part of the expression at this position.
  ///
  /// `None` is this trait's end channel: the token stays in the stream for the surrounding
  /// grammar. [`PrattRHS::End`] means exactly the same thing here and is treated
  /// identically, so a classifier shared with the typed driver needs no translation — but
  /// `None` is the spelling to prefer, since it is the one the signature already offers.
  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), Power>>;
}
