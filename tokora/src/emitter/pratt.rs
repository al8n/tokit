use crate::error::{NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs};

use super::*;

/// An emitter that handles pratt related errors.
pub trait PrattEmitter<'inp, L, Lang: ?Sized = ()>: Emitter<'inp, L, Lang> {
  /// Emits an error or warning for an unexpected end of left hand side error while parsing pratt expression.
  fn emit_unexpected_end_of_lhs(
    &mut self,
    err: UnexpectedEoLhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>;

  /// Emits an error or warning for an unexpected end of right hand side error while parsing pratt expression.
  fn emit_unexpected_end_of_rhs(
    &mut self,
    err: UnexpectedEoRhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>;
}

impl<'inp, L, U, Lang> PrattEmitter<'inp, L, Lang> for &mut U
where
  U: PrattEmitter<'inp, L, Lang>,
  Lang: ?Sized,
{
  #[inline(always)]
  fn emit_unexpected_end_of_lhs(
    &mut self,
    err: UnexpectedEoLhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    (**self).emit_unexpected_end_of_lhs(err)
  }

  #[inline(always)]
  fn emit_unexpected_end_of_rhs(
    &mut self,
    err: UnexpectedEoRhs<L::Offset, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'inp>,
  {
    (**self).emit_unexpected_end_of_rhs(err)
  }
}

/// A trait bound for converting pratt emitter errors into emitter errors.
///
/// The bundle is **every** failure a pratt engine can hand a caller, not only the two the
/// emitter has a hook for: the two end-of-expression reports the drivers raise on a contract
/// violation, plus the two the engines *return* rather than emit — a
/// [`RecursionLimitReached`] frame-budget trip and a [`NonAssociativeChain`] repeat. The last
/// two have no `emit_*` counterpart on [`PrattEmitter`] by design (a recording emitter must not
/// be able to swallow them), and they are listed here so that "this error type can drive a pratt
/// parse" stays one bound rather than four.
pub trait FromPrattError<'inp, L, Lang: ?Sized = ()>: FromEmitterError<'inp, L, Lang> {
  /// Creates an emitter error from an unexpected end of left hand side error.
  fn from_unexpected_end_of_lhs(err: UnexpectedEoLhs<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>;

  /// Creates an emitter error from an unexpected end of right hand side error.
  fn from_unexpected_end_of_rhs(err: UnexpectedEoRhs<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>;

  /// Creates an emitter error from a recursion-limit trip at a pratt frame prologue.
  ///
  /// An implementation that stores the value keeps its **always-terminal** marker readable
  /// through [`MaybeTerminal`](crate::error::MaybeTerminal); one that discards it opts the
  /// error type out of terminal re-raise, which is the documented `MaybeTerminal` posture.
  fn from_recursion_limit_reached(err: RecursionLimitReached<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>;

  /// Creates an emitter error from a second same-power non-associative operator in one chain.
  fn from_non_associative_chain(err: NonAssociativeChain<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>;
}

impl<'inp, T, L, Lang: ?Sized> FromPrattError<'inp, L, Lang> for T
where
  L: Lexer<'inp>,
  T: FromEmitterError<'inp, L, Lang>
    + From<UnexpectedEoLhs<L::Offset, Lang>>
    + From<UnexpectedEoRhs<L::Offset, Lang>>
    + From<RecursionLimitReached<L::Offset, Lang>>
    + From<NonAssociativeChain<L::Offset, Lang>>,
{
  #[inline(always)]
  fn from_unexpected_end_of_lhs(err: UnexpectedEoLhs<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>,
  {
    err.into()
  }

  #[inline(always)]
  fn from_unexpected_end_of_rhs(err: UnexpectedEoRhs<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>,
  {
    err.into()
  }

  #[inline(always)]
  fn from_recursion_limit_reached(err: RecursionLimitReached<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>,
  {
    err.into()
  }

  #[inline(always)]
  fn from_non_associative_chain(err: NonAssociativeChain<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>,
  {
    err.into()
  }
}
