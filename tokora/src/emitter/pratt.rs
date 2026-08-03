use crate::error::{UnexpectedEoLhs, UnexpectedEoRhs};

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
/// Exactly the conversions a [`PrattEmitter`] body performs: the two end-of-expression reports
/// the pratt drivers raise on a contract violation, one per channel. It is the bound the
/// built-in emitters' [`PrattEmitter`] impls carry, and the blanket impl below means two `From`
/// impls on your error type are all it takes to satisfy it.
///
/// # The two pratt failures that are deliberately not here
///
/// A pratt engine can also fail with a
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) frame-budget trip or a
/// [`NonAssociativeChain`](crate::error::NonAssociativeChain) repeat, and **neither is an
/// emitter conversion**. Neither has an `emit_*` counterpart on [`PrattEmitter`], by design — a
/// recording emitter must not be able to swallow a resource trip or a rejected chain — so
/// neither passes through an emitter at all: the engines *return* them, and a returned error
/// becomes the caller's type through `From`.
///
/// So the pratt entry points name
/// `From<RecursionLimitReached<L::Offset, Lang>>` and
/// `From<NonAssociativeChain<L::Offset, Lang>>` on the error type **directly**, beside this
/// bound, and this trait has no method for either. A method here would be one nothing calls:
/// the trip is built and converted inside
/// [`InputRef::descend`](crate::InputRef::descend) — the recursion guard every recursive-descent
/// grammar shares, pratt or not — and the repeat is built by each engine at its own exit. Both
/// happen where no emitter is in scope.
///
/// The trip's `From` impl decides how much of the trip you can *read*, and nothing more. One that
/// **stores** the value keeps the offset, the depth and the limitation, and its always-terminal
/// marker readable through [`MaybeTerminal`](crate::error::MaybeTerminal); one that discards it
/// keeps none of that. Neither decides whether recovery re-raises: the trip latches the input
/// session, and the recovery combinators read that latch beside `MaybeTerminal`. See
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached)'s own docs for what a
/// discarding sink costs and what it does not.
pub trait FromPrattError<'inp, L, Lang: ?Sized = ()>: FromEmitterError<'inp, L, Lang> {
  /// Creates an emitter error from an unexpected end of left hand side error.
  fn from_unexpected_end_of_lhs(err: UnexpectedEoLhs<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>;

  /// Creates an emitter error from an unexpected end of right hand side error.
  fn from_unexpected_end_of_rhs(err: UnexpectedEoRhs<L::Offset, Lang>) -> Self
  where
    L: Lexer<'inp>;
}

impl<'inp, T, L, Lang: ?Sized> FromPrattError<'inp, L, Lang> for T
where
  L: Lexer<'inp>,
  T: FromEmitterError<'inp, L, Lang>
    + From<UnexpectedEoLhs<L::Offset, Lang>>
    + From<UnexpectedEoRhs<L::Offset, Lang>>,
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
}
