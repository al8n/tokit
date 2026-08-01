use core::marker::PhantomData;

/// A non-associative operator appeared a second time at its own binding power in one chain:
/// `a == b == c` where `==` is [`PrattInfix::Neither`](crate::parser::PrattInfix::Neither).
///
/// Both Pratt engines raise this, on the same trigger, with the same restored posture: the
/// deciding read is handed back — the offending operator is left on the input, unconsumed — and
/// the error is **returned**, never emitted.
///
/// # Why an error rather than an ending
///
/// Non-associativity is a property of a *chain*, and the only frame that knows a chain exists is
/// the one holding the repeat latch. Ending the expression instead — folding once and leaving
/// the second operator for the surrounding grammar — destroys that information: in any nested
/// position the enclosing frame sees an ordinary admissible operator and folds it by its own
/// rules, so `a = b ; c ; d` with `;` non-associative and `=` weaker parses to completion as
/// `((a = (b ; c)) ; d)` with nothing left over for any caller to reject. The hand-off's
/// recipient is the same engine one frame up, and it is structurally incapable of knowing the
/// constraint. So the chain is rejected where it is seen.
///
/// # Not terminal
///
/// This is malformed **input**, the classic recovery target, so
/// [`is_terminal`](crate::error::MaybeTerminal::is_terminal) is `false` and
/// [`Recover`](crate::parser::Recover) / [`InplaceRecover`](crate::parser::InplaceRecover) /
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) may spend it. A grammar that *wants*
/// the fold-once-then-stop behaviour asks for it explicitly, by wrapping the pratt parser in a
/// recovery combinator or by reclassifying the operator
/// [`Left`](crate::parser::PrattInfix::Left); tolerance is a caller policy stated in grammar
/// code, not a silent engine default.
///
/// # The offset is the handback position, and that is one specific number
///
/// [`offset`](Self::offset) is **defined** as the position the input was handed back at, and the
/// definition is checkable rather than descriptive: catch the error and
/// [`InputRef::span().end()`](crate::InputRef::span) *is* the offset. Nothing before that byte is
/// still available to the caller, and everything from it onward is — including whatever the
/// deciding read had consumed before it was handed back.
///
/// It is **not** the offending operator's first byte. The two coincide only when nothing sits
/// between them, and four ordinary things do:
///
/// ```text
///   1 ; 2 ; 3          whitespace the lexer skips
///   0 2 4 6 8          offset 5, the repeat at 6
///
///   1 ; 2   ; 3        whitespace lexed as TOKENS, skipped by the classifier
///   0 2 4   8 10       offset 5, the repeat at 8
///
///   1 < > 2 < > 3      an operator spelled with two tokens
///   0 2 4 6 8 10 12    offset 7, the repeat's head at 8 and its tail at 10
///
///   1 ; 2 @ ; 3        a non-fatal lexer error, reported and stepped over
///   0 2 4 6 8 10       offset 5, the repeat at 8
/// ```
///
/// Each of those is a region the caller *was* handed back. Naming the operator instead would
/// describe a position no caller was ever left at, and would invite a recoverer that resumes at
/// the offset to discard the region in between — the lexer error's own bytes, in the last case,
/// together with the diagnostic that was rewound with the aborted probe and is due to be
/// re-emitted.
///
/// The operator's head is also not a quantity the typed engine could report even if it wanted to.
/// [`ParsePrattRHS`](crate::parser::ParsePrattRHS) is caller code holding a whole
/// [`InputRef`](crate::InputRef) and decides for itself where its operator begins; learning how
/// much it would skip means *running* it, and running it before the repeat has been decided is
/// what the driver's transaction rules forbid.
///
/// **What is guaranteed, by identity rather than by agreement, is that the reported offset and the
/// position the input was handed back at are one number.** That is what makes the value usable:
/// `at` names a real boundary in the caller's own input, and everything from it onward is still in
/// front of whoever the error reaches. It is a fact about the *handback*, and the section below
/// says which callers are standing on it when they read the value.
///
/// **How each engine gets there — neither measures anything near the handback.** The typed engine
/// carries its cycle's committed-progress watermark, which is the value its probe's checkpoint was
/// built from and the value the probe's rollback installs back. The token engine reads
/// `span().end()` on the far side of the park, and `try_expect_map_or_stop` commits a token only
/// when the classifier accepts it, so a declined token leaves that frontier exactly where the
/// cycle found it. Both therefore report the same number on any input both can parse, and the
/// numbers agree because the two engines answer the same question, not because two mechanisms
/// happen to line up.
///
/// # Which recovery paths resume at the offset, and which do not
///
/// The identity above is about the handback, and it holds for whoever the error is handed to
/// *unchanged*. It is **not** a prediction of where a recovery combinator restarts, because two of
/// the three roll the input back further before they run: [`Recover`](crate::parser::Recover) and
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) both speculate through
/// [`try_attempt`](crate::InputRef::try_attempt), whose failure path restores the pre-attempt
/// checkpoint. What they hand a handler, or begin skipping from, is therefore **their own attempt
/// origin** — at or before this offset, and usually well before it.
///
/// Measured on `1 ; 2 ; 3` with the whole pratt parser wrapped, where the error carries `at == 5`:
///
/// | Path | The position it observes | Why |
/// |---|---|---|
/// | Catch the `Err` yourself | [`span().end()`](crate::InputRef::span) is **5** | nothing has moved since the handback |
/// | [`inplace_recover`](crate::ParseInput::inplace_recover) | `span().end()` is **5** | it never backtracks; the [`Cursor`](crate::input::Cursor) it is *also* handed names where the primary parser started, **0** |
/// | [`recover`](crate::ParseInput::recover) | `span().end()` is **0** | the attempt is rolled back before `recover_input` runs |
/// | [`skip_then_retry`](crate::ParseInput::skip_then_retry) | **0**, and the skip scans forward from there | same rollback; the first token its sync predicate is offered is the `1` at 0, and its first skipped region is `0..1` |
///
/// Both facts are true at once, and neither weakens the other. The practical consequences of the
/// second one are worth spelling out, because they are not what a reader of the offset would guess:
///
/// * A `.recover(…)` handler that renders a caret at the offset it was handed is right; one that
///   assumes the input is *at* that offset — say, by reading the next token expecting to find the
///   repeated operator — is reading from the start of the expression instead.
/// * `skip_then_retry` scans for its sync point from behind the repeat, so it can synchronise on a
///   token the expression had already folded. On `1 ; 2 ; 3` it syncs on the **first** `;` at 2,
///   three bytes behind the offset and four behind the repeated operator at 6.
///
/// **If you need a recovery that resumes where the error says**, catch the `Err` in your own
/// grammar code, or use [`inplace_recover`](crate::ParseInput::inplace_recover): those are the two
/// paths on which the input is still where the offset names.
///
/// # The rendered text names the handback too
///
/// `Display` is the surface a caller who never calls [`offset`](Self::offset) still sees — in a
/// log line, in a wrapping error's `{}`, in a snapshot — so it names the number the same way the
/// accessor does rather than as the operator's location:
///
/// ```rust
/// # use tokora::error::NonAssociativeChain;
/// let error: NonAssociativeChain = NonAssociativeChain::of(5);
/// assert_eq!(
///   error.to_string(),
///   "non-associative operator cannot be chained at its own power; input handed back at 5, at \
///    or before the operator",
/// );
/// ```
///
/// That is the render for `1 ; 2 ; 3`, where the repeated `;` starts at **6** — so a wording of the
/// form `non-associative operator at 5 …` would point a reader at a byte the operator does not
/// occupy, and the four shapes above are how far apart the two numbers get. The payload and the
/// sentence around it have to say the same thing, because the sentence is all a caller who logs or
/// wraps the error ever has.
///
/// # A per-operator contract, not whole-chain fixity resolution
///
/// The latch is one step: it is set by folding a `Neither` operator at `p`, cleared by folding
/// any infix at a different power, and left alone by a postfix fold. So `a == b < c` (with `==`
/// non-associative and `<` left-associative at the same power) is rejected, while `a < b == c`
/// is accepted as `(a < b) == c`. Languages that resolve fixity over the whole chain reject
/// both; tokora's table is per-operator, and tightening it would be a semantic expansion rather
/// than a fix.
///
/// # Example
///
/// ```rust
/// use tokora::error::{MaybeTerminal, NonAssociativeChain};
///
/// let error: NonAssociativeChain = NonAssociativeChain::of(6);
/// assert_eq!(error.offset(), 6);
/// assert!(!error.is_terminal());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error(
  "non-associative operator cannot be chained at its own power; input handed back at {at}, at \
   or before the operator"
)]
pub struct NonAssociativeChain<O = usize, Lang: ?Sized = ()> {
  /// The offset the input was handed back at — `InputRef::span().end()` once the error is in the
  /// caller's hands, and where the surrounding grammar resumes if it catches the `Err` itself or
  /// recovers in place; a rollback-based recovery restarts at its own attempt origin instead. At
  /// or before the repeated operator's own start, and strictly before it whenever anything the
  /// caller was also handed back sits between them. See the type's docs.
  at: O,
  _lang: PhantomData<Lang>,
}

impl<O, Lang: ?Sized> NonAssociativeChain<O, Lang> {
  /// Creates the error for a repeat handed back at `at`.
  #[inline(always)]
  pub const fn of(at: O) -> Self {
    Self {
      at,
      _lang: PhantomData,
    }
  }

  /// Returns the handback offset — the position the input was left at, which is at or before the
  /// repeated operator's own start.
  ///
  /// It is where a grammar that catches this error, or one that recovers with
  /// [`inplace_recover`](crate::ParseInput::inplace_recover), resumes.
  /// [`Recover`](crate::parser::Recover) and
  /// [`skip_then_retry`](crate::ParseInput::skip_then_retry) restore their own attempt origin
  /// first and restart there instead — see the type's docs.
  #[inline(always)]
  pub const fn offset(&self) -> O
  where
    O: Copy,
  {
    self.at
  }

  /// Returns a reference to the handback offset.
  #[inline(always)]
  pub const fn offset_ref(&self) -> &O {
    &self.at
  }

  /// Rewrites the offset — the shape a nested parse uses to lift an inner offset into its
  /// enclosing document's coordinates.
  #[inline(always)]
  pub fn map_offset<U, F>(self, f: F) -> NonAssociativeChain<U, Lang>
  where
    F: FnOnce(O) -> U,
  {
    NonAssociativeChain {
      at: f(self.at),
      _lang: PhantomData,
    }
  }
}

/// Malformed input, not a resource trip: recovery may spend it. The blanket `false` is the
/// correct classification and this impl exists to state it.
impl<O, Lang: ?Sized> crate::error::MaybeTerminal for NonAssociativeChain<O, Lang> {}

/// The unit error sink absorbs a repeat like every other error, so a `()`-errored grammar still
/// drives the pratt engines.
///
/// Unlike [`RecursionLimitReached`](crate::error::RecursionLimitReached)'s, this conversion is
/// **inert** with respect to recovery: the value is already non-terminal, so a recoverer spends it
/// either way and the sink decides nothing. `tokora/tests/pratt_limit_unit_sink.rs` runs the same
/// recovery through `()` and through a delegating error type and gets the same answer. What is
/// lost is the offset, and with it the ability to resume at a named position — the ordinary cost
/// of a discarding sink, not a change of contract.
impl<O, Lang: ?Sized> From<NonAssociativeChain<O, Lang>> for () {
  #[inline(always)]
  fn from(_: NonAssociativeChain<O, Lang>) -> Self {}
}
