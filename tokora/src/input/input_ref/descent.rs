use core::ops::{Deref, DerefMut};

use crate::{
  error::RecursionLimitReached,
  state::recursion_tracker::{RecursionLimiter, RecursionTracker},
};

use super::*;

impl<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// The **recursion budget** this parse descends against: the live depth and the limit it may
  /// not exceed.
  ///
  /// Read-only, deliberately. There is no `recursion_mut`: the cell has exactly one writer, the
  /// [`Descent`] guard [`descend`](Self::descend) hands out, which is what makes "every level
  /// entered is a level left" a property of the type system rather than of caller discipline.
  ///
  /// Configure it with
  /// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter)
  /// or [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter).
  #[inline(always)]
  pub const fn recursion(&self) -> &RecursionLimiter {
    self.recursion
  }

  /// Enters **one level of recursive descent**, or fails terminally if the configured limit is
  /// exceeded.
  ///
  /// The returned [`Descent`] guard *is* the level: it derefs to this handle, so the frame's body
  /// runs through it, and leaving its scope — by return, by `?`, or by an unwind — releases the
  /// level. There is no matching "ascend" to forget.
  ///
  /// ```rust,ignore
  /// let mut frame = inp.descend()?;   // one level, for as long as `frame` lives
  /// let inp = &mut *frame;            // the body below is unchanged
  /// ```
  ///
  /// # What one level means
  ///
  /// Whatever the caller says it means — the primitive counts calls, not grammar constructs. The
  /// two Pratt engines call it once at each frame prologue, so for them the budget reads "live
  /// pratt frames on this input", the root expression included. The budget is a property of the
  /// **input session**, not of a parser, so two recursive parsers composed into one grammar draw
  /// on one depth: that is what makes it a bound on native stack use rather than on any single
  /// production.
  ///
  /// # On `Err`, the depth is exactly what it was
  ///
  /// The trip path lowers the depth again *before* it builds the error, so a caller that catches
  /// the failure and parses something else finds the cell describing only the frames that are
  /// genuinely live. Building the error is the grammar's own `From`, and it runs after the
  /// decrement, so even a panicking conversion cannot leak a level.
  ///
  /// # The error is terminal, and it is returned
  ///
  /// [`RecursionLimitReached`] is terminal for every value: no amount of further input clears a
  /// depth budget, so [`Recover`](crate::parser::Recover),
  /// [`InplaceRecover`](crate::parser::InplaceRecover) and
  /// [`skip_then_retry`](crate::ParseInput::skip_then_retry) re-raise it rather than synthesizing
  /// a node. It is *returned*, never emitted, so no rewind can erase it and no recording emitter
  /// can turn a tripped budget into a truncated-but-successful parse. Its offset is committed
  /// consumption at frame entry — cache-independent, so a prefilled lookahead window trips at the
  /// same place an empty one does.
  ///
  /// Nothing is latched on the input. A scanner limit trip latches the poison boundary because
  /// the lexer's tally is monotone in the input; descent depth is the opposite kind of fact and
  /// is fully restored by the unwind that carries the error out. See [`RecursionLimitReached`].
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::{
  ///   Emitter, InputRef, Lexer, ParseContext,
  ///   error::RecursionLimitReached,
  /// };
  ///
  /// /// A recursive production that counts against the parse's shared depth budget.
  /// fn nested<'inp, L, Ctx>(
  ///   inp: &mut InputRef<'inp, '_, L, Ctx>,
  ///   remaining: usize,
  /// ) -> Result<usize, <Ctx::Emitter as Emitter<'inp, L>>::Error>
  /// where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  ///   <Ctx::Emitter as Emitter<'inp, L>>::Error: From<RecursionLimitReached<L::Offset, ()>>,
  /// {
  ///   let mut frame = inp.descend()?;
  ///   let inp = &mut *frame;
  ///   match remaining {
  ///     0 => Ok(inp.recursion().depth()),
  ///     n => nested(inp, n - 1),
  ///   }
  /// }
  /// ```
  #[inline(always)]
  pub fn descend(
    &mut self,
  ) -> Result<
    Descent<'_, 'inp, 'closure, L, Ctx, Lang, Cmpl>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<RecursionLimitReached<L::Offset, Lang>>,
  {
    // RAISE, CHECK, ARM — and nothing else may sit between the three. `increase` and `check` are
    // const arithmetic over two `usize`s, so neither can fail and neither can panic; the trip arm
    // lowers the depth again before it touches caller code, and the `From` that builds the error
    // runs after that decrement. Past the `Ok`, the witness is armed and its destructor owns the
    // level. There is therefore no window in which a level is raised and unowned.
    self.recursion.increase();
    if let Err(exceeded) = RecursionTracker::check(&*self.recursion) {
      self.recursion.decrease();
      // Committed consumption, not the cache-front cursor: the same metric the pratt drivers'
      // stall exits report, so the offset does not move with how much lookahead is buffered.
      return Err(RecursionLimitReached::of(self.span().end(), exceeded).into());
    }
    Ok(Descent { input: self })
  }
}

/// One live level of recursive descent, held for the length of the frame that entered it.
///
/// Handed out by [`InputRef::descend`]. It derefs to the [`InputRef`] it was taken from, so the
/// frame's body runs *through* the guard — and because the guard holds the handle's `&mut`
/// exclusively, there is no way to keep parsing while dropping the level early. Leaving the
/// scope releases it: on `Ok`, on a `?`-propagation, and on an unwind, in `std` and `no_std`
/// alike.
///
/// # Why a guard and not a pair of calls
///
/// Depth is *covariant with frame entry and exit, including unwinds*, and invariant under input
/// rollback. That combination has exactly one correct witness. A manual `decrease` at the exits
/// misses the unwind; a cell inside the lexer state or the [`Checkpoint`](crate::input::Checkpoint)
/// set would be restored by rollbacks that do not pop any frame — double-restored on a `std`
/// unwind (where an undecided [`Commit`] guard rolls back) and leaked on a `no_std` one (where it
/// commits). The destructor is the one witness whose behaviour does not fork on the unwind edge.
pub struct Descent<'r, 'inp, 'closure, L, Ctx, Lang: ?Sized = (), Cmpl = Complete>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  input: &'r mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
}

impl<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl> Deref
  for Descent<'_, 'inp, 'closure, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  type Target = InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>;

  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    self.input
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> DerefMut for Descent<'_, 'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.input
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> Drop for Descent<'_, 'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Releases the level. Two instructions and no branch, which is why — unlike the transaction
  /// guard's settle — there is nothing here worth outlining: a destructor is emitted at every
  /// unwind edge of its owner, and this one costs a load, a subtract and a store there.
  ///
  /// Policy-independent by construction. It does not consult `thread::panicking()`, so it does
  /// the same thing on the unwind edge as on the return edge, in every build.
  #[inline(always)]
  fn drop(&mut self) {
    debug_assert!(
      self.input.recursion.depth() > 0,
      "tokora: a `Descent` guard released a level that was never raised — the recursion cell's \
       only writer is `InputRef::descend`, so this means the depth was lowered behind it"
    );
    self.input.recursion.decrease();
  }
}
