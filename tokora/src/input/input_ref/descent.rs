use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

use crate::{
  error::RecursionLimitReached,
  state::recursion_tracker::{RecursionLimiter, RecursionTracker},
};

use super::*;

/// The baseline half of the **descent-trip witness** — what
/// [`InputRef::trip_snapshot`](InputRef::trip_snapshot) hands out, and the only thing
/// [`InputRef::tripped_during_attempt`](InputRef::tripped_during_attempt) accepts.
///
/// Opaque, and every absence in it is deliberate. There is **no accessor**, no `PartialEq`, no
/// `Default` and no public constructor, so the only thing a caller can do with one is hand it back
/// to the checker that issued it. That is the type doing what the prose used to do alone: the
/// witness promises "a trip happened inside this attempt" and promises nothing about the counter
/// underneath, so the difference of two baselines, the comparison of one against zero, and the
/// question "how many trips" are all unspellable rather than merely discouraged.
///
/// # What refuses a foreign baseline, measured rather than assumed
///
/// A baseline means nothing against an input that did not issue it, and the mechanisms here were
/// each pinned by planting against them, because the plausible story is wrong twice.
///
/// * **The `'closure` parameter refuses it outright, on every public path.** A baseline carries
///   the region of the handle that issued it, and the only way to obtain an [`InputRef`] from
///   outside this crate is through a closure whose handle lifetime is universally quantified —
///   `Input` is crate-internal, so there is no other door. A baseline
///   therefore cannot be parked anywhere that outlives the invocation, and it cannot be carried
///   into another one. Both shapes were compiled and refused: stashing one in a `Cell`
///   ([`trip_snapshot`](InputRef::trip_snapshot) pins it, with a control), and the realistic
///   cross-wire — a parser that runs a nested parse and judges the inner input with the outer
///   input's baseline, which is the shape a driver holding two inputs alive would write. That one
///   fails with `E0521: borrowed data escapes outside of function`.
///
/// * **Invariance is load-bearing, at an identified site, and the map is measured rather than
///   asserted** — this sentence has been wrong twice, so what follows is what flipping the brand
///   to a bare reference measurably changes and what it does not.
///
///   | shape | invariant | covariant |
///   |---|---|---|
///   | a bare region-shrinking coercion on this type | refused | **accepted** |
///   | a generic adapter: `&mut InputRef<'short>` plus a baseline `<'long>`, `'long: 'short` | refused | **accepted** |
///   | a `Cell` stash | refused | refused |
///   | a closure capture under a `for<'c>` bound | refused | refused |
///   | a hand-written [`ParseInput`](crate::ParseInput) impl carrying one in a field | refused | refused |
///
///   The **adapter row is the load-bearing one**, and it is a consumption site — which is what an
///   earlier version of this paragraph got wrong when it concluded from the last three rows that
///   the brand was redundant with [`InputRef`]'s own invariance in `'closure`. In those three the
///   handle is itself coerced or universally quantified, so its invariance already refuses and the
///   brand's cannot be seen. In the adapter the handle sits at **one fixed region and is never
///   coerced**; only the baseline moves, and then the brand is the only thing refusing. Under a
///   covariant brand that call compiles, and it is publicly reachable — carry an outer baseline
///   through an inner parse's context and the nested parse cross-feeds a foreign baseline into the
///   nonce panic. The brand is what keeps that unreachable from safe code.
///
///   Three probes agreed the brand was inert and all three shared a shape: each put the handle
///   somewhere its own invariance would refuse first. Agreement between probes that share a shape
///   is not independent evidence.
///
///
/// * **The nonce covers what is left, which is inside this crate.** Two handles minted from
///   `Input::as_ref` in one scope have ordinary inference regions that
///   unify, and both counters start at zero, so a cross-fed baseline compares *equal*. That is
///   crate-internal by construction — no consumer can reach `Input` — and it is a programmer
///   error, not a parse outcome, so
///   [`tripped_during_attempt`](InputRef::tripped_during_attempt) **panics** on it rather than
///   answering. See there for why a plausible `true` was the wrong answer.
///
/// # `Copy`, and what it is for
///
/// Deliberate. A driver reads its baseline more than once per turn — `parser::many`'s element loop
/// hands the same `trips` to the failure gate and then to the close gate — and a move-only baseline
/// would push those sites into re-taking it, which is the placement defect the type exists to make
/// hard. Duplicating one is harmless because storage is refused by the region parameter and not by
/// move semantics: two copies are as unstorable as one.
///
#[derive(Clone, Copy)]
pub struct ResourceTripBaseline<'closure> {
  /// The counter's value when the baseline was taken.
  count: u64,
  /// The identity of the input that issued it: the address of that input's resource-trip slot.
  /// Distinct inputs are distinct structs at distinct addresses, and the slot is a `u64` so it is
  /// never zero-sized.
  nonce: usize,
  /// Invariant in `'closure`, through the fn-pointer form rather than a bare reference.
  ///
  /// **Load-bearing**, and the type's own docs carry the measured map of where. Flipping it to a
  /// bare reference makes two shapes compile that must not: a plain region-shrinking coercion, and
  /// — the one that matters — a generic adapter holding the handle at one fixed region while only
  /// the baseline is coerced, which is publicly reachable and would feed a foreign baseline into
  /// the nonce panic. It is inert only where [`InputRef`]'s own invariance in `'closure` refuses
  /// first, which is not everywhere.
  _brand: PhantomData<fn(&'closure ()) -> &'closure ()>,
}

/// Prints neither field, and that is the point rather than an omission.
///
/// A derive would render `count`, which is the session-absolute reading the type exists to make
/// unspellable — `trip_snapshot() != 0` does not compile, and a derived `Debug` would hand the
/// same number back through `{:?}` in one line. It would also render `nonce`, which is the address
/// of an internal slot and belongs in no user-visible output at all. Neither is a fact about the
/// baseline that a caller is entitled to; the only fact there is is the verdict, and
/// [`InputRef::tripped_during_attempt`] is the one door onto it.
///
/// `a_baseline_debug_render_leaks_neither_the_count_nor_the_nonce` asserts on the rendered string,
/// because this impl is one `#[derive]` away from regressing.
impl core::fmt::Debug for ResourceTripBaseline<'_> {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("ResourceTripBaseline(..)")
  }
}

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
  /// **Terminality is stored on the input, not in the error payload.** The trip arm counts the trip
  /// on `Input::resource_trips` before it builds anything the grammar can see, and the
  /// three combinators above consult that cell **in addition to**
  /// [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal) on the converted value. So the
  /// re-raise holds for a grammar error that stores the value and delegates `is_terminal`, and
  /// equally for one whose `From` discards it — `()` included. A discarding sink loses the
  /// *payload*; it does not lose the *stop*, because the stop was never the payload's to carry.
  /// That is invariance, not new semantics: across those three combinators it makes every
  /// supported sink behave the way a delegating one already did.
  ///
  /// The **resilient collection loops** — `repeated`, `separated` and their delimited forms — read
  /// the same cell, and there it is not a second opinion but the only one: those swallow arms carry
  /// no `MaybeTerminal` bound, so it is the whole of what stops an element's trip being
  /// emitted as a diagnostic and looped past. That one is not invariance — it was spent for every
  /// error type before #148 — so it changes those families' behaviour on a trip. See
  /// `Input::resource_trips` and `parser::many`'s `GATE_CENSUS`.
  ///
  /// **What every one of those sites tests is a per-attempt transition, not the session fact.**
  /// The cell is monotone and is never cleared, so reading it absolutely would say "this parse has
  /// tripped" forever after the first trip — and refuse recovery, and refuse emit-and-continue, for
  /// every later failure including ordinary syntax errors in unrelated constructs. Each site
  /// therefore takes a [`trip_snapshot`](Self::trip_snapshot) before the attempt it is judging and
  /// asks [`tripped_during_attempt`](Self::tripped_during_attempt) after it — the same pair a
  /// consumer outside this crate now reads. A real trip is still re-raised wherever it actually
  /// happens, including a second one after grammar code caught the first.
  ///
  /// What is recorded is the **fact that the budget was exceeded**, and not the depth. A scanner
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
  /// This is the **one writer** of the session's resource-trip counter, which is what makes
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
      // LATCH, before the caller's `From` and before the offset read below — a bump of a counter
      // the caller cannot reach, so a conversion that panics, or a recoverer that swallows the
      // converted value, cannot leave the session claiming its budget was never exceeded. The
      // decrement above and this store are the two halves of one fact: the depth is what is live
      // now, this is what happened.
      //
      // Counting rather than latching a `bool` is what makes the reading attempt-relative: a
      // consulting site compares the value against the one its attempt started with, and a second
      // trip after a caught first one must not compare equal to it.
      //
      // SATURATING on a `u64` cell, and this used to be `wrapping_add` on a `usize` one. The
      // argument for wrapping was that an inequality needs consecutive values to differ, which it
      // gives — but the reading also needs the value never to RETURN to a baseline after a
      // positive number of trips, and wrapping is the one behaviour that guarantees it does. At
      // `usize` width that aliased after 2^32 refusals on a 32-bit target, which is a loop rather
      // than a lifetime once a zero recursion limit makes every call of this function a trip.
      // Saturating alone then made the mirror mistake — the verdict's fail-closed disjunct fires
      // at the ceiling whether or not anything tripped — so the width is what carries the repair
      // and the disjunct only covers a point no program reaches. See
      // `crate::input::TRIP_COUNTER_EXHAUSTED` for both ceilings and the measured cost.
      *self.resource_trips = self.resource_trips.saturating_add(1);
      // Committed consumption, not the cache-front cursor: the same metric the pratt drivers'
      // stall exits report, so the offset does not move with how much lookahead is buffered.
      return Err(RecursionLimitReached::of(self.span().end(), exceeded));
    }
    Ok(Descent { input: self })
  }

  /// Snapshots the session's **resource-trip counter** — how many times a resource budget has been
  /// exceeded in this input session so far — for an attempt-relative terminality witness.
  ///
  /// The descent twin of the scanner's `latch_snapshot`, and used exactly as
  /// `scanner_trip_snapshot` is: take the baseline once per
  /// attempt, hand it back to [`tripped_during_attempt`](Self::tripped_during_attempt) when
  /// judging that attempt's failure.
  ///
  /// The counter itself is a **monotone session fact**, counted up by the trip arm behind
  /// [`descend`](Self::descend) and never lowered: a [`Checkpoint`](crate::input::Checkpoint) does
  /// not carry it, a restore does not touch it, and [`Descent`]'s `Drop` releases only the depth.
  /// That arm is its one writer and no method on this handle hands out a mutable route to the
  /// cell, so a consumer can read this witness and cannot forge it.
  ///
  /// # The value is a baseline, and that is all the type lets it be
  ///
  /// [`ResourceTripBaseline`] is opaque: no accessor, no `PartialEq`, no constructor. Hand it back
  /// to [`tripped_during_attempt`](Self::tripped_during_attempt) and nothing else is expressible —
  /// not the difference of two baselines, not a trip count, and not the session-absolute reading
  /// below. The contract used to be a sentence asking callers not to do those things; it is now
  /// the type refusing to let them.
  ///
  /// **The session-absolute reading is the one that had to become unspellable.**
  /// `trip_snapshot() != 0` would be "did this parse exceed a budget at all" — a true statement
  /// about the session, and deliberately *not* the question any consulting site has. It stays true
  /// forever once grammar code catches one trip and carries on, so a site that reads it mid-parse
  /// charges every later failure — an ordinary syntax error in an unrelated construct included —
  /// with a stop that is already over, and one deep construct early in a document suppresses every
  /// diagnostic after it. `tokora/tests/root_loop_trip_witness.rs` measures what that costs. The
  /// question itself is legitimate *after* the parse, where there is no later attempt to poison,
  /// and it has a published answer there: `cst::Cst::resource_trips` (feature `rowan`).
  ///
  /// # A baseline cannot leave the handle that issued it
  ///
  /// It carries the handle's `'closure`, and a parser reaches its input through a universally
  /// quantified handle lifetime, so there is no region a caller can name that one could be parked
  /// in. That is the shape of carrying a baseline across a
  /// [`PartialSession`](crate::input::PartialSession) redrive — which builds a fresh input and a
  /// fresh counter starting at zero, so the carried baseline would compare against a different
  /// cell and the next input's first trip would read as "nothing happened".
  ///
  /// **The control first**, because a `compile_fail` that fails for the wrong reason proves
  /// nothing. The identical function stashing an ordinary `usize` read off the same handle
  /// compiles, so the scaffolding — the `Cell`, the bounds, the borrow of `inp` — is sound:
  ///
  /// ```rust
  /// use core::cell::Cell;
  /// use tokora::{Emitter, InputRef, Lexer, ParseContext};
  ///
  /// fn stash_a_number<'inp, L, Ctx>(
  ///   inp: &mut InputRef<'inp, '_, L, Ctx>,
  ///   cell: &Cell<Option<usize>>,
  /// ) where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   cell.set(Some(inp.recursion().depth()));
  /// }
  /// ```
  ///
  /// Change only the value's type, and it stops compiling — `'1 must outlive 'static`:
  ///
  /// ```compile_fail
  /// use core::cell::Cell;
  /// use tokora::{Emitter, InputRef, Lexer, ParseContext, input::ResourceTripBaseline};
  ///
  /// fn stash_a_baseline<'inp, L, Ctx>(
  ///   inp: &mut InputRef<'inp, '_, L, Ctx>,
  ///   cell: &Cell<Option<ResourceTripBaseline<'static>>>,
  /// ) where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   // error[E0521]: borrowed data escapes outside of function
  ///   cell.set(Some(inp.trip_snapshot()));
  /// }
  /// ```
  ///
  /// The legal use is the one the loop actually needs, and it compiles:
  ///
  /// ```rust
  /// use tokora::{Emitter, InputRef, Lexer, ParseContext};
  ///
  /// fn judge_one_attempt<'inp, L, Ctx>(inp: &mut InputRef<'inp, '_, L, Ctx>) -> bool
  /// where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   let trips = inp.trip_snapshot();
  ///   // … the attempt this baseline judges runs here …
  ///   inp.tripped_during_attempt(trips)
  /// }
  /// ```
  ///
  /// **The realistic cross-wire is refused too**, and it is the shape a driver holding two inputs
  /// alive would actually write: a parser that runs a nested parse and judges the *inner* input
  /// with the *outer* input's baseline. Compiled and refused with `E0521: borrowed data escapes
  /// outside of function`. The theorem underneath both refusals is that a baseline fixed at one
  /// region cannot be handed to a frame that demands every region:
  ///
  /// Its control first, again: the same closure capturing a region-free `usize` off the same
  /// handle satisfies the same bound, so the refusal below is the value's type and not the shape.
  ///
  /// ```rust
  /// use tokora::{InputRef, Lexer, ParseContext};
  ///
  /// fn as_a_parser_over_a_number<'inp, 'closure, L, Ctx>(
  ///   outer: &mut InputRef<'inp, 'closure, L, Ctx>,
  /// ) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx>) -> bool
  /// where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   let depth: usize = outer.recursion().depth();
  ///   move |inner: &mut InputRef<'inp, '_, L, Ctx>| inner.recursion().depth() == depth
  /// }
  /// ```
  ///
  /// ```compile_fail
  /// use tokora::{InputRef, Lexer, ParseContext, input::ResourceTripBaseline};
  ///
  /// /// The closure a nested parse would be handed: it must work at EVERY handle region, and the
  /// /// captured baseline is fixed at one.
  /// fn as_a_parser<'inp, 'closure, L, Ctx>(
  ///   outer: &mut InputRef<'inp, 'closure, L, Ctx>,
  /// ) -> impl for<'c> FnMut(&mut InputRef<'inp, 'c, L, Ctx>) -> bool
  /// where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   let base: ResourceTripBaseline<'closure> = outer.trip_snapshot();
  ///   move |inner: &mut InputRef<'inp, '_, L, Ctx>| inner.tripped_during_attempt(base)
  /// }
  /// ```
  ///
  /// **A hand-written [`ParseInput`](crate::ParseInput) impl is refused too**, and it is the shape
  /// a parser combinator carrying a baseline in a field would take. Its control first — the same
  /// impl carrying a region-free `usize` — which compiles:
  ///
  /// ```rust
  /// use tokora::{Emitter, InputRef, Lexer, ParseContext, ParseInput};
  ///
  /// struct Control {
  ///   depth: usize,
  /// }
  ///
  /// impl<'inp, L, Ctx> ParseInput<'inp, L, bool, Ctx> for Control {
  ///   fn parse_input(
  ///     &mut self,
  ///     input: &mut InputRef<'inp, '_, L, Ctx>,
  ///   ) -> Result<bool, <Ctx::Emitter as Emitter<'inp, L>>::Error>
  ///   where
  ///     L: Lexer<'inp>,
  ///     Ctx: ParseContext<'inp, L>,
  ///   {
  ///     Ok(input.recursion().depth() == self.depth)
  ///   }
  /// }
  /// ```
  ///
  /// and the baseline in the same field position, which does not — the method's handle region is
  /// fresh for every call, and the field's is fixed by the struct:
  ///
  /// ```compile_fail
  /// use tokora::{Emitter, InputRef, Lexer, ParseContext, ParseInput, input::ResourceTripBaseline};
  ///
  /// struct Probe<'closure> {
  ///   base: ResourceTripBaseline<'closure>,
  /// }
  ///
  /// impl<'inp, 'closure, L, Ctx> ParseInput<'inp, L, bool, Ctx> for Probe<'closure> {
  ///   fn parse_input(
  ///     &mut self,
  ///     input: &mut InputRef<'inp, '_, L, Ctx>,
  ///   ) -> Result<bool, <Ctx::Emitter as Emitter<'inp, L>>::Error>
  ///   where
  ///     L: Lexer<'inp>,
  ///     Ctx: ParseContext<'inp, L>,
  ///   {
  ///     // error: lifetime may not live long enough
  ///     Ok(input.tripped_during_attempt(self.base))
  ///   }
  /// }
  /// ```
  ///
  /// **The invariance rails are different cells**, because the three shapes above are refused
  /// under either variance — see [`ResourceTripBaseline`] for the measured map. Two shapes the
  /// brand alone decides. The bare coercion:
  ///
  /// ```compile_fail
  /// use tokora::input::ResourceTripBaseline;
  ///
  /// // error: lifetime may not live long enough — invariant, so `'long` cannot shrink
  /// fn shrink<'long: 'short, 'short>(
  ///   b: ResourceTripBaseline<'long>,
  /// ) -> ResourceTripBaseline<'short> {
  ///   b
  /// }
  /// ```
  ///
  /// and the one that makes it matter — a generic adapter holding the handle at **one fixed
  /// region**, so `InputRef`'s own invariance never enters and only the baseline is asked to
  /// coerce. Its control first, the same adapter over a region-free `usize`:
  ///
  /// ```rust
  /// use tokora::{Emitter, InputRef, Lexer, ParseContext};
  ///
  /// fn adapt_a_number<'inp, 'short, L, Ctx>(
  ///   inp: &mut InputRef<'inp, 'short, L, Ctx>,
  ///   depth: usize,
  /// ) -> bool
  /// where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   inp.recursion().depth() == depth
  /// }
  /// ```
  ///
  /// and the baseline in the same position, which does not compile — and **would** compile under a
  /// covariant brand, which is the whole of why the brand is not prophylaxis:
  ///
  /// ```compile_fail
  /// use tokora::{Emitter, InputRef, Lexer, ParseContext, input::ResourceTripBaseline};
  ///
  /// fn adapt<'inp, 'long: 'short, 'short, L, Ctx>(
  ///   inp: &mut InputRef<'inp, 'short, L, Ctx>,
  ///   base: ResourceTripBaseline<'long>,
  /// ) -> bool
  /// where
  ///   L: Lexer<'inp>,
  ///   Ctx: ParseContext<'inp, L>,
  /// {
  ///   // error: lifetime may not live long enough — only the baseline is coerced here, so this is
  ///   // the brand refusing and not the handle's own invariance
  ///   inp.tripped_during_attempt(base)
  /// }
  /// ```
  ///
  /// What the region parameter does **not** catch is two inputs alive at once *inside this crate*,
  /// where handles are minted directly and their regions unify. That is a programmer error rather
  /// than a parse outcome, and
  /// [`tripped_during_attempt`](Self::tripped_during_attempt) **panics** on it instead of
  /// answering. [`ResourceTripBaseline`] has every half and says which was measured how.
  ///
  /// # Why a pair, and not one guard that takes the snapshot for you
  ///
  /// A guard — one call that opens the attempt, runs it and answers — is harder to misuse, and
  /// this crate has one: `parser::recovery_gate`'s attempt chokepoint, which owns the closure its
  /// combinator hands it and therefore owns the unit the baselines belong to. It is not the shape
  /// this crate's *loops* use. `parser::many`'s twelve drivers take this baseline by hand, inside
  /// their element loop, and pass it to a chokepoint that reads it — because **the two baselines
  /// have opposite granularities, and neither can be derived from the other**. This one is per
  /// *element*: hoisted out of the loop it is arithmetically the session-absolute read above. The
  /// scanner's is per *collection*: taken per element it is re-read after each trip an element
  /// caught, so every later exit concludes cleanly over a budget that is spent. Both fusions are
  /// measured defects, and `parser::many`'s `GATE_CENSUS` pins both placements in both directions.
  ///
  /// A guard that snapshots for you fixes both baselines to one unit, which is that fusion. So
  /// what this crate centralizes is the **verdict**, never the baseline, and the baseline is
  /// published as a value the caller places. A consumer whose unit *is* a closure can still build
  /// the guard on top of this pair; a consumer whose unit is a region of its own loop could not
  /// have built this pair on top of a guard.
  ///
  /// # Why it is public, and why it is inherent
  ///
  /// [`tripped_during_attempt`](Self::tripped_during_attempt) carries the first argument.
  ///
  /// **Inherent rather than an opt-in trait, deliberately.** An inherent item wins the method pick
  /// over an extension trait's, so adding these two names to a type that already shipped can take
  /// a call away from a consumer who wrote either name on [`InputRef`] — silently, where the
  /// paired `let b = inp.trip_snapshot(); inp.tripped_during_attempt(b)` compiles on both sides
  /// and infers the argument. A trait would avoid that and cost an import. The form is chosen
  /// anyway, because `InputRef` carries its whole surface as inherent methods and a witness that
  /// had to be imported would be unreadable inside a generic production that imports nothing — an
  /// opt-in trait for these two alone would be the only such accessor in the crate. The cost is
  /// paid where this crate pays it: disclosed in the CHANGELOG's "no diagnostic at the call site"
  /// section, with the UFCS spelling that restores the consumer's item, and classified in
  /// `ci/name_collision/no_collision.txt` with what that classification does *not* establish.
  ///
  /// # What it costs, on both word sizes
  ///
  /// A `u64` load and one address read per attempt, and nothing anywhere else: no scan, no
  /// lookahead fill and no token commit reads or writes either. The count is **one machine word on
  /// a 64-bit target and two on a 32-bit one** — it is `u64` rather than `usize` for the reason
  /// `input::TRIP_COUNTER_EXHAUSTED` gives, and this is the price
  /// of it.
  ///
  /// **This call is on the hot path of every *successful* element**, because the descent baseline
  /// is taken per element — so the price was measured rather than argued. Every figure below is
  /// `thumbv6m-none-eabi` at `-O`, the narrowest supported target, except where it says otherwise.
  ///
  /// | | `usize` | `u64` | delta | runs |
  /// |---|---|---|---|---|
  /// | this call | 5 instr | 7 instr | **+2** | per element |
  /// | [`tripped_during_attempt`](Self::tripped_during_attempt) | 17 instr | 22 instr | +5 | 0-2 per collection termination, by class — see its own table |
  /// | an element loop (N of the first, one of the second) | 31 instr | 38 instr | +7 | per collection |
  /// | that loop's stack frame | 16 B | 24 B | +8 B | per collection |
  /// | [`ResourceTripBaseline`] itself | 8 B, align 4 | 16 B, align 8 | **×2** | carried per element |
  ///
  /// The **layout** row is the one an instruction count does not show: the baseline is `Copy` and
  /// passed by value, so a 32-bit element loop carries twice the value it used to. It is in the
  /// accounting because it was missed the first time this was priced.
  ///
  /// Whole-parse, on `wasm32-wasip1` — the widest 32-bit target that can be *executed* here, since
  /// `i686` cannot link on this host — a collection-parse binary grew **33 bytes** (120,421 →
  /// 120,454, +0.027%), and interleaved executed latency showed **no regression in either
  /// workload shape**: a long single collection at `-5.8%` min / `-5.3%` median, and many short
  /// collections — the shape the per-termination `+5` makes expensive — at `-0.35%` min / `-7.25%`
  /// median, n=12 each. Both sides land at or below zero, which is an instrument that cannot
  /// resolve the change rather than a speedup.
  ///
  /// **What the executed measurement covers**, stated rather than generalised: the probe is a
  /// non-delimited collection terminating by **decline** — one of the six classes in the table
  /// above. Of the five it does not exercise, four are **cheaper or equal** per termination (a
  /// direct closer evaluates no verdict, a probed closer only the descent one, and both failure
  /// classes sit behind cheaper reads that can short-circuit either away). The one that is not
  /// cheaper is a [`skip_then_retry`](crate::ParseInput::skip_then_retry) workload, which pays both
  /// verdicts per **successful** sync or advance step through `recovery_step`; that shape is
  /// unmeasured, and it is named here rather than folded into the number.
  ///
  /// **Accepted on that scope**: two instructions and eight stack bytes per element loop, against
  /// an element that has already run a cache probe or a full lex and commit, buys a counter a
  /// 32-bit loop cannot exhaust. Keeping `count` at `usize` and moving the exhaustion distinction
  /// to a second cell would buy back the four bytes and add a load at the same sites, and the
  /// executed measurement gives it nothing to recover.
  #[inline(always)]
  pub fn trip_snapshot(&self) -> ResourceTripBaseline<'closure> {
    ResourceTripBaseline {
      count: *self.resource_trips,
      nonce: self.trip_nonce(),
      _brand: PhantomData,
    }
  }

  /// The identity of **this** input, for [`ResourceTripBaseline`]'s nonce: the address of the
  /// resource-trip slot it borrows.
  ///
  /// The same derivation [`begin_point`](InputRef::begin_point) uses for its session ids, off this
  /// witness's own cell rather than the poison boundary. Two simultaneously-live inputs are
  /// distinct structs at distinct addresses and the slot is a `u64`, so it is never zero-sized and
  /// the two can never collide.
  #[inline(always)]
  fn trip_nonce(&self) -> usize {
    core::ptr::from_ref(&*self.resource_trips).addr()
  }

  /// Whether a **resource budget was exceeded during the attempt** that took `since` as its
  /// [`trip_snapshot`](Self::trip_snapshot) baseline.
  ///
  /// The input-side witness for resource terminality, and the answer to "did the failure I am
  /// judging come from a tripped budget". Read by [`Recover`](crate::parser::Recover),
  /// [`InplaceRecover`](crate::parser::InplaceRecover) and
  /// [`skip_then_retry`](crate::ParseInput::skip_then_retry) *beside*
  /// [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal), and by the four resilient
  /// collection loops (`repeated`, `separated` and their delimited forms) on its own, since those
  /// carry no `MaybeTerminal` bound. Either way a grammar error type that discards
  /// [`RecursionLimitReached`] on conversion — `()` does — still cannot turn a tripped budget into
  /// recovery, nor into a diagnostic the collection keeps parsing past.
  ///
  /// **Attempt-relative, not session-absolute** — the same discipline the scanner's
  /// `latched_during_attempt` applies to its latch, and for the same reason. The cell it reads is
  /// monotone, so an absolute reading answers "has this *parse* ever tripped", which is true
  /// forever once grammar code catches a trip and carries on. Every later failure in the session —
  /// an ordinary syntax error in an unrelated construct included — would then be re-raised as
  /// though the budget had stopped it, and a single deep expression early in a document would
  /// suppress every diagnostic after it. Comparing against the baseline asks the question the
  /// site actually has: *this* attempt, not this parse.
  ///
  /// A **count** rather than a `bool` is what makes the comparison hold up a second time. A
  /// set-once flag compares equal to a baseline taken after an earlier caught trip, so the next
  /// genuine trip would read as "nothing happened here" — narrowing the witness into a hole. The
  /// counter changes on every trip, so `!=` is exactly "a trip happened inside this attempt".
  ///
  /// # The granularity floor: one attempt, and it fails closed
  ///
  /// This answers "**a** trip happened while the attempt ran". It does **not** answer "the `Err` I
  /// am holding **is** that trip". The two come apart inside a single attempt: grammar code that
  /// catches a trip itself, carries on, and then fails *ordinarily before the attempt ends* leaves
  /// the counter moved, and the ordinary failure is re-raised as though the budget had stopped it.
  ///
  /// So the resolution of this witness is **one attempt** — one speculative parse for
  /// [`Recover`](crate::parser::Recover) and
  /// [`InplaceRecover`](crate::parser::InplaceRecover), one retry cycle for
  /// [`skip_then_retry`](crate::ParseInput::skip_then_retry), one *element* for the resilient
  /// collection loops. Within that unit the verdict **fails closed** at the recovery, failure,
  /// absence and real-closer gates: an ordinary failure sharing its unit with a caught trip is
  /// re-raised, never the reverse, and a real trip reaching one of those gates is never recovered
  /// from and never filed as a diagnostic. It says nothing about `Accept` — an element that catches
  /// the trip and still answers `Accept` spends it, for every error type, on purpose; see
  /// `parser::many`'s module docs, "the channel neither chokepoint closes" section. Outside the
  /// unit nothing is charged at all, which is the whole point of taking a baseline.
  ///
  /// **The strong form is not implementable at this layer.** Deciding whether the error in hand is
  /// the trip means interrogating its value, and the grammar's error type may be `()`, whose `From`
  /// discards [`RecursionLimitReached`] entirely — a sink that discards is the reason this witness
  /// lives on the input instead of in the error. Any design claiming to tell the two apart is
  /// either reading a payload that is not there, or is the escape hatch below wearing a different
  /// name.
  ///
  /// **The escape hatch, if a consumer ever needs one:** an explicit *rebaseline* — code that
  /// deliberately catches a trip declaring it settled, so the enclosing baselines move past it and
  /// the attempt is judged only on what happens after. That is a cooperative operation, and it is
  /// the design to build if the floor ever costs somebody something real. It is not built: no
  /// consumer needs it yet, and this crate does not publish API on speculation.
  ///
  /// `tokora/tests/collection_resource_trip.rs` and `tokora/tests/pratt_limit_unit_sink.rs` each
  /// pin one cell on this behaviour, paired against the cell that moves the catch outside the unit
  /// and gets the opposite answer.
  ///
  /// # Why it is public, and why its scanner twin is not
  ///
  /// This crate does not publish API on speculation, and this pair was crate-private for as long
  /// as the only sites judging an attempt were its own. What changed is a consumer with the
  /// defect. smear's GraphQL parser catches a failed definition at each document root and has to
  /// decide whether that failure ends the document (al8n/smear#169); it decided by reading the
  /// error, which is the decision this counter exists to replace. A nesting refusal that reached a
  /// root loop as an ordinary error resynchronised, re-read the abandoned nest at document level,
  /// and reported one diagnostic per remaining token — 67 for one refusal at 66 levels, 804 at
  /// 800, growing with the document.
  ///
  /// That repair took three rounds because every round left the verdict resting on something a
  /// *caller* implements. [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal) is the
  /// grammar's own answer, and a `From` that discards [`RecursionLimitReached`] — `()` does —
  /// answers `false` over a real trip. A latch of the parser's own would have to live in
  /// `L::State` or beside the poison boundary, and a [`Checkpoint`](crate::input::Checkpoint)
  /// carries both, so a speculative rollback refunds it. This cell is neither: written by the trip
  /// arm before any grammar code runs, outside the rollback set, one writer, no mutable route.
  ///
  /// **The scanner twin was published in the same change and withdrawn before release**, and the
  /// asymmetry is the point rather than an accident of scope. That counter has a second public
  /// contract it cannot see — [`set_state`](InputRef::set_state) drops the poison boundary as the
  /// crate's *documented* limit-recovery path, and never touches the counter — so a loop following
  /// its own documentation can recover, read a whole document, and still be told it was truncated.
  /// `InputRef::scanner_trip_snapshot` carries that measurement, and also the narrower statement
  /// that replaced an earlier overclaim: [`try_expect_or_stop`](InputRef::try_expect_or_stop)
  /// covers the *declining* exits and is **not** a replacement for that pair, because a rejecting
  /// emitter's trip is built and propagated from inside it. No public witness answers that path
  /// today, and none did before this change either. **Nothing analogous exists here**, and the
  /// reason is the channel: a descent refusal is *returned* by
  /// [`descend`](Self::descend) and never routed through an emitter, so no emitter can unmark it
  /// or convert it away from the counter that already recorded it —
  /// `the_descent_witness_holds_under_a_rejecting_emitter` measures exactly that. No public API
  /// clears or re-keys the descent counter either: a budget once
  /// exceeded cannot be un-exceeded by more input, by an unwind, or by a rollback, and the one
  /// cooperative escape hatch — the rebaseline above — is deliberately not built. The witness a
  /// consumer can rely on is exactly the one with no second contract to conflict with.
  ///
  /// What is published is the **reading**. There is no writer here, no way to lower the counter,
  /// and no rebaseline: the granularity floor above is the floor for a consumer too.
  /// `tokora/tests/root_loop_trip_witness.rs` is the outside-the-crate use, including the cell
  /// that measures what the amplification costs a root loop with no witness and the cell that
  /// measures what a baseline hoisted out of the loop costs one that has it.
  ///
  /// # A baseline from another input is a panic, not a verdict
  ///
  /// It **panics** if `since` was issued by a different [`InputRef`], and the alternative was
  /// considered and rejected: answering `true` — "fail closed", the direction every *real*
  /// reading of this witness fails in — is the wrong answer for this one, because the two failures
  /// do not cost the same thing. A spurious `true` tells a root loop its attempt tripped, and a
  /// root loop told that **ends a document that was fine and discards the valid suffix**. The
  /// truncated parse still returns `Ok`, so the mistake survives testing and points at nothing.
  /// That is the identical failure shape as the state-recovery residue that kept the scanner twin
  /// crate-internal, and it would be indefensible to build it in here on purpose.
  ///
  /// A cross-fed baseline is a **programmer error**, not a parse outcome. It cannot be produced by
  /// input, hostile or otherwise — only by code that wired two inputs together — and no consumer
  /// can write it at all: the region parameter refuses every public path, measured, and
  /// `Input` is crate-internal so there is no other way to hold two
  /// handles whose regions unify. What the panic guards is this crate's own sites, and any future
  /// door that hands out a handle outside a closure boundary: it announces itself at the crossing
  /// instead of silently truncating, and it is distinguishable from a real trip by not being a
  /// `bool` at all.
  ///
  /// The check is one word-pair comparison on a path that already runs one, and its branch is
  /// cold.
  ///
  /// # When each verdict runs — the table, derived from every call site
  ///
  /// **This is the one copy of this model.** Three previous versions of it were each written from
  /// the call sites the author happened to look at, and each was wrong in a different way: "on the
  /// failure arm only" missed the clean exits, "on every normal termination" missed that some
  /// terminations evaluate nothing and some evaluate only half, and both missed the two
  /// `recovery_gate` sites entirely. So this is enumerated from the code: there are exactly **five**
  /// places in the crate that evaluate either verdict, and the six termination classes fall out of
  /// them rather than being asserted beside them.
  ///
  /// | class | site | descent | scanner | reached when |
  /// |---|---|---|---|---|
  /// | failed element | `many::file_element_failure` | 4th term | 3rd term | an element returned `Err`. Both sit behind `is_incomplete_error` and `at_committed_boundary`, so either can be short-circuited away |
  /// | decline or stall | `many::absence_after_element` | 3rd term | **1st term** | a driver concludes absence. The scanner verdict always runs; the descent one only if the scanner and latch reads are both false |
  /// | probed closer | `many::close_after_element` | only term | **never** | a probe verdict handed over a real closer. The scanner reading is deliberately absent — a pre-trip closer settles the position question and only that |
  /// | direct closer | the mid-scan arms of `sep/delim` and `sep_while/delim` | **never** | **never** | a closer committed straight from the driver's own scan. The cycle's final `trip_snapshot` is paid and **no verdict is** |
  /// | recovery failure | `parser::recovery_gate::judge` | 3rd term | 4th term | a recovery attempt returned `Err`. Both sit behind `is_incomplete()` and `is_terminal()` |
  /// | successful recovery step | `parser::recovery_gate::recovery_step` | **1st term** | 2nd term | a `skip_then_retry` skip or advance **succeeded**. The descent verdict always runs — the one place either runs after something worked |
  ///
  /// What follows from the table, and what does not: a verdict is **absent from an accepted,
  /// progressing element**, which is the arm the per-element baseline exists to leave alone. Beyond
  /// that there is no single frequency — a collection terminating through a direct closer pays
  /// **none**, through a probed closer pays the **descent one alone**, and through a decline or
  /// stall pays **both**. Anything that states one number per collection is describing one class
  /// and calling it the model.
  ///
  /// Costs a nonce comparison and a `u64` comparison. Measured on `thumbv6m-none-eabi`, `-O`: **17
  /// instructions at `usize`, 22 at `u64`**. [`trip_snapshot`](Self::trip_snapshot) carries the
  /// whole cost table and the scope the measurement covers.
  ///
  /// # Panics
  ///
  /// If `since` came from a different input than `self`.
  #[inline(always)]
  pub fn tripped_during_attempt(&self, since: ResourceTripBaseline<'closure>) -> bool {
    assert!(
      since.nonce == self.trip_nonce(),
      "ResourceTripBaseline came from a different input than the one judging it. A baseline is \
       only meaningful against its own input's counter; comparing it here would answer about a \
       parse this handle knows nothing about. Take the baseline from the same handle that reads \
       the verdict."
    );
    *self.resource_trips == crate::input::TRIP_COUNTER_EXHAUSTED
      || *self.resource_trips != since.count
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
