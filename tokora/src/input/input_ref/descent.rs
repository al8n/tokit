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
  /// entered is a level left" a property of the type system on every exit path rather than of
  /// caller discipline — short of leaking the guard instead of dropping it, which is Rust's
  /// universal `mem::forget` caveat and is covered on
  /// [`Descent`](Descent#what-the-type-system-enforces-here-and-what-it-does-not).
  /// *Balance* is the property that buys; **where** a level is left is the guard's scope, and
  /// that is the caller's to place — unless the scope is
  /// [`descending`](Self::descending)'s closure, which places it for you. See
  /// [`Descent`](Descent#what-the-type-system-enforces-here-and-what-it-does-not).
  ///
  /// Configure it with
  /// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter)
  /// or [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter).
  #[inline(always)]
  pub const fn recursion(&self) -> &RecursionLimiter {
    self.recursion
  }

  /// Runs `f` **one level of recursive descent deeper**, or fails terminally if the configured
  /// limit is exceeded.
  ///
  /// **This is the form to write a recursive combinator in.** The level is raised before `f`
  /// runs and released when `f` returns, propagates with `?`, or unwinds — and nothing `f` can
  /// write releases it earlier, because `f` is handed the *input* and never the guard, and
  /// [`recursion`](Self::recursion) is read-only. The scope of the level and the extent of the
  /// frame are therefore the same region by construction, rather than by the caller placing a
  /// binding correctly.
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
  ///   inp.descending(|inp| match remaining {
  ///     0 => Ok(inp.recursion().depth()),
  ///     n => nested(inp, n - 1),
  ///   })
  /// }
  /// ```
  ///
  /// # Errors thread through untouched, so `?` composes
  ///
  /// `f` returns the frame's own `Result<T, E>` and it is returned unchanged, exactly as
  /// [`try_attempt`](Self::try_attempt) threads its closure's error. `E` is not tied to the
  /// emitter's error type — the trip is *returned*, never emitted, so it is built directly as `E`
  /// through the `From` bound, which the frame's error type satisfies for the same reason
  /// [`descend`](Self::descend)'s does.
  ///
  /// Write the **whole** frame body as the closure and its `return`s keep their meaning: the
  /// closure returns `Result<T, E>` and this method returns it verbatim, so `return Err(e)` and
  /// `return Ok(v)` still leave the frame. What a closure cannot host is a `return` meant for an
  /// *enclosing* function, or a `break`/`continue` aimed at a loop outside it; a body shaped that
  /// way is what [`descend`](Self::descend) is still public for.
  ///
  /// # If `f` panics
  ///
  /// The level is released on the unwind, by the guard's destructor, in `std` and `no_std` alike
  /// — the same edge [`descend`](Self::descend) covers, and for the same reason. A host that
  /// catches the unwind is handed an input whose depth is what it was before the call.
  ///
  /// # What one level means
  ///
  /// The same thing it means for [`descend`](Self::descend): whatever the caller says it means,
  /// counted per call and shared by every parser on this input.
  #[inline(always)]
  pub fn descending<F, T, E>(&mut self, f: F) -> Result<T, E>
  where
    F: FnOnce(&mut Self) -> Result<T, E>,
    E: From<RecursionLimitReached<L::Offset, Lang>>,
  {
    // The guard lives in THIS frame, not the caller's and not the closure's. `f` never receives
    // it — only a reborrow of the input through `DerefMut` — so it can neither move it out nor
    // forget it, and the two ways a caller-placed binding goes wrong (dropped before the
    // recursion, or never bound at all) are not expressible here.
    let mut frame = self.raise_level()?;
    f(&mut frame)
  }

  /// Enters **one level of recursive descent**, or fails terminally if the configured limit is
  /// exceeded — the **low-level escape hatch** under [`descending`](Self::descending).
  ///
  /// **Reach for [`descending`](Self::descending) instead unless the frame's body cannot be a
  /// closure** — because it must `return` out of an enclosing function, or `break`/`continue` a
  /// loop outside it, or because it is large enough that relocating it is a change in its own
  /// right. This method hands the level back as an ordinary value, so *where the level ends is
  /// caller code*, and four measured spellings end it before the recursion it was taken for; only
  /// one of the four warns. The list, the measurements and the reason no rule can close it are on
  /// [`Descent`](Descent#what-the-type-system-enforces-here-and-what-it-does-not).
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
  /// **Bind it.** There is no "ascend" to forget, but there is a scope to place wrong:
  /// `inp.descend()?;` as a bare statement compiles, because `?` takes the `Result` apart and
  /// leaves the guard a temporary that dies at the semicolon — releasing the level *before* the
  /// recursion it was taken for, and putting the native stack back at risk with the budget
  /// reading zero. [`Descent`] is `#[must_use]` so that one line warns, and a warning on that one
  /// line is all it is: see [what the guard does and does not
  /// enforce](Descent#what-the-type-system-enforces-here-and-what-it-does-not).
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
  /// **Terminality is stored on the input, not in the error payload.** The trip arm latches
  /// `Input::resource_trip` before it builds anything the grammar can see, and the
  /// three combinators above consult that cell **in addition to**
  /// [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal) on the converted value. So the
  /// re-raise holds for a grammar error that stores the value and delegates `is_terminal`, and
  /// equally for one whose `From` discards it — `()` included. A discarding sink loses the
  /// *payload*; it does not lose the *stop*, because the stop was never the payload's to carry.
  /// That is invariance, not new semantics: across those three combinators it makes every
  /// supported sink behave the way a delegating one already did. It does **not** reach the
  /// resilient collection loops, which swallow an element's trip for every error type alike — see
  /// `Input::resource_trip`'s own documentation for that gap and why it is a separate decision.
  ///
  /// What is latched is the **fact that the budget was exceeded**, and not the depth. A scanner
  /// limit trip latches the poison boundary because the lexer's tally is monotone in the input;
  /// descent *depth* is the opposite kind of fact and is fully restored by the unwind that carries
  /// the error out, so it is not latched and must not be. That a budget was once exceeded is
  /// monotone in exactly the scanner's sense — no unwind, no rollback and no further input can
  /// un-exceed it — so it is latched, and latched for the parse's remaining life. The two latches
  /// differ in what they record: a scanner trip has a position, so it latches *where*; a descent
  /// trip has only a control stack, so it latches *whether*. See [`RecursionLimitReached`].
  ///
  /// # Example
  ///
  /// [`descending`](Self::descending)'s example, written out by hand — which is all the guard
  /// form is. Keep the binding and the shadowing `let inp = &mut *frame;` together, because that
  /// pair is what makes every line below reach the input *through* the level.
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
    self.raise_level().map_err(Into::into)
  }

  /// RAISE, CHECK, ARM — the one copy of the sequence, shared by [`descend`](Self::descend) and
  /// [`descending`](Self::descending) so the two spellings cannot drift apart.
  ///
  /// Returns the trip **unconverted**. The `From` that builds the caller's error therefore runs in
  /// the caller, which is what keeps the documented ordering — decrement first, latch second,
  /// grammar code after — true for both spellings even if that conversion panics.
  ///
  /// This is the **one writer** of [`resource_trip`](Self::resource_trip), which is what makes
  /// "grammar code cannot lower the latch" a property of the module rather than of discipline:
  /// the cell is `pub(super)`, no method hands out a mutable route to it, and both Pratt engines
  /// — typed and token — reach the budget only through here, so both inherit the latch by
  /// construction rather than by each remembering to set it.
  #[inline(always)]
  fn raise_level(
    &mut self,
  ) -> Result<Descent<'_, 'inp, 'closure, L, Ctx, Lang, Cmpl>, RecursionLimitReached<L::Offset, Lang>>
  {
    // RAISE, CHECK, ARM — and nothing else may sit between the three. `increase` and `check` are
    // const arithmetic over two `usize`s, so neither can fail and neither can panic; the trip arm
    // lowers the depth again and latches the trip before it touches caller code, and the `From`
    // that builds the error runs after both. Past the `Ok`, the witness is armed and its
    // destructor owns the level. There is therefore no window in which a level is raised and
    // unowned, and none in which a trip has been decided and is not yet recorded.
    self.recursion.increase();
    if let Err(exceeded) = RecursionTracker::check(&*self.recursion) {
      self.recursion.decrease();
      // LATCH, before the caller's `From` and before the offset read below — a store to a `bool`
      // the caller cannot reach, so a conversion that panics, or a recoverer that swallows the
      // converted value, cannot leave the session claiming its budget was never exceeded. The
      // decrement above and this store are the two halves of one fact: the depth is what is live
      // now, this is what happened.
      *self.resource_trip = true;
      // Committed consumption, not the cache-front cursor: the same metric the pratt drivers'
      // stall exits report, so the offset does not move with how much lookahead is buffered.
      return Err(RecursionLimitReached::of(self.span().end(), exceeded));
    }
    Ok(Descent { input: self })
  }

  /// Whether a **resource budget was exceeded** at any point in this input session — the
  /// input-side witness for resource terminality, and the answer to "where is resource
  /// terminality stored".
  ///
  /// Set-once by [`raise_level`](Self::raise_level)'s trip arm and never lowered: a
  /// [`Checkpoint`](crate::input::Checkpoint) does not carry it, a restore does not touch it, and
  /// [`Descent`]'s `Drop` releases only the depth. Read by
  /// [`Recover`](crate::parser::Recover), [`InplaceRecover`](crate::parser::InplaceRecover) and
  /// [`skip_then_retry`](crate::ParseInput::skip_then_retry) *beside*
  /// [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal), so a grammar error type that
  /// discards [`RecursionLimitReached`] on conversion — `()` does — still cannot turn a tripped
  /// budget into recovery.
  ///
  /// Costs one `bool` load on a recovery decision and nothing anywhere else: no scan, no
  /// lookahead fill and no token commit reads or writes it.
  #[inline(always)]
  pub(crate) const fn resource_trip(&self) -> bool {
    *self.resource_trip
  }
}

/// One live level of recursive descent, held for the length of the frame that entered it.
///
/// Handed out by [`InputRef::descend`], the low-level form. It derefs to the [`InputRef`] it was
/// taken from, so the frame's body runs *through* the guard: while the guard is alive it holds the
/// handle's `&mut` exclusively, so nothing can read this frame's input outside this frame's level.
/// Leaving the guard's scope releases the level: on `Ok`, on a `?`-propagation, and on an unwind,
/// in `std` and `no_std` alike.
///
/// [`InputRef::descending`] hands out no guard at all — it owns one for the span of a closure —
/// and a frame written that way cannot reach any of what follows.
///
/// # What the type system enforces here, and what it does not
///
/// Two of the three properties are the compiler's. The third is not, and it is the one that goes
/// wrong, so it is stated rather than left to be discovered:
///
/// * **Balance is guaranteed on every exit path.** Every level entered is left, exactly once, by
///   this type's `Drop`. There is no `recursion_mut` and no "ascend" to forget, and no exit path —
///   return, `?`, or an unwind — skips a destructor. That much *is* a property of the type system
///   rather than of caller discipline. What it is not proof against is *leaking* the guard rather
///   than dropping it: `mem::forget`, `ManuallyDrop` and `Box::leak` skip every destructor in Rust
///   and this one is not special. Measured, a forgotten guard holds its level for the rest of the
///   parse — `recursion().depth()` never comes back down and the input stays usable at the raised
///   depth — which is the opposite failure from the four rows below and the milder one: it
///   *tightens* the budget rather than removing it, so the worst outcome is a spurious trip and
///   never a native abort.
/// * **Nesting through a live guard is guaranteed.** The exclusive borrow means that for as long
///   as the guard exists it is the only route to the input, so no code can parse this frame from
///   outside its own level.
/// * **Where the level *ends* is not.** It ends where the guard's scope ends, and choosing that
///   scope is ordinary caller code. The scope can be put in the wrong place:
///
///   ```rust,ignore
///   inp.descend()?;        // `?` unwraps, and the guard is a temporary: the level dies HERE
///   recurse(inp, n - 1)    // …so this recursion runs with the budget already given back
///   ```
///
///   That shape type-checks, and it silently reinstates the unbounded descent — and with it the
///   native stack overflow — that the budget exists to prevent. The guard is therefore
///   `#[must_use]`, so the line above raises `unused_must_use` (*unused … that must be used*)
///   with the correct shape in the note. `tests/ui/descent_dropped_early.rs` pins that it does.
///
/// ## Exactly what the lint catches, measured
///
/// **One shape of five.** Each row below is a hand-written recursive combinator run against a
/// `RecursionLimiter` of **8** through 200 nested calls, compiled under `clippy -D warnings`. The
/// *depth* is what `recursion().depth()` reads at the deepest frame — a bound frame stops at 9 and
/// never reaches it. The last column is the shallowest probed depth (1 000, 2 000, … 8 000, one
/// process each) at which the same shape overflows a 2 MiB thread and takes the process down with
/// `fatal runtime error: stack overflow`:
///
/// | frame body | diagnostic | result at 200 | aborts by |
/// |---|---|---|---|
/// | `inp.descending(\|inp\| …)` | — | `Err(RecursionLimitReached)`, depth 9 | never |
/// | `let mut frame = inp.descend()?; let inp = &mut *frame;` | — | `Err(RecursionLimitReached)`, depth 9 | never |
/// | `inp.descend()?;` | **`unused_must_use`** | `Ok`, depth 0 | 5 000 |
/// | `let _ = inp.descend()?;` | none | `Ok`, depth 0 | 5 000 |
/// | `if inp.descend().is_ok() { … }` | none | `Ok`, depth 0 | 5 000 |
/// | `let d = inp.descend()?.recursion().depth();` | none | `Ok`, depth 1 | 4 000 |
/// | `drop(inp.descend()?);` | none | `Ok`, depth 0 | 5 000 |
///
/// The abort column is a **demonstration, not a constant**: it is one debug build's frame size on
/// one platform, and it moved by a whole probe step between two builds of the same source. What is
/// stable is the shape of the finding — the budget is configured, it is consulted, it never sees
/// more than one live level, and the native abort it exists to delete comes back.
///
/// So the attribute catches the bare statement and nothing else, and the four silent rows are not
/// a closed list — **any** expression that consumes the `Result` or the guard and lets it die
/// before the recursion does the same thing. rustc's own `help:` on the one caught row suggests
/// `let _ = …`, which is the second row.
///
/// Nothing in this type can close that. Releasing a level early is *legal* — a frame that finishes
/// its recursion and then keeps parsing at the shallower depth wants exactly that — so there is no
/// predicate separating the intent from the mistake and no rule to make it unrepresentable. What
/// closes it for a given frame is writing that frame so the level's scope and the body are the
/// same region, which [`InputRef::descending`] does by construction and this spelling does by
/// discipline:
///
///   ```rust,ignore
///   let mut frame = inp.descend()?;
///   let inp = &mut *frame;   // every line below runs inside the level, recursion included
///   ```
///
/// The runtime cells behind the table are in `src/input/input_ref/descent_tests.rs`.
///
/// # Why a guard and not a pair of calls
///
/// Depth is *covariant with frame entry and exit, including unwinds*, and invariant under input
/// rollback. That combination has exactly one correct witness. A manual `decrease` at the exits
/// misses the unwind; a cell inside the lexer state or the [`Checkpoint`](crate::input::Checkpoint)
/// set would be restored by rollbacks that do not pop any frame — double-restored on a `std`
/// unwind (where an undecided [`Commit`] guard rolls back) and leaked on a `no_std` one (where it
/// commits). The destructor is the one witness whose behaviour does not fork on the unwind edge.
#[must_use = "write the frame as `inp.descending(|inp| ...)`, which holds the level for the whole \
              body — or hold this guard in a binding for the whole frame, \
              `let mut frame = inp.descend()?; let inp = &mut *frame;`. Dropping it releases the \
              recursion level, and `inp.descend()?;` as a statement drops it before the frame it \
              is meant to bound"]
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
