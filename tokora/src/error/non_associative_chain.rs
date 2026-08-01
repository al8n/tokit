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
/// resumption point are one number.** That is the property recovery correctness rests on: hand the
/// error to [`Recover`](crate::parser::Recover) or
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) and the offset it reads is the offset
/// the retry begins at.
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
#[error("non-associative operator at {at} cannot be chained at its own power")]
pub struct NonAssociativeChain<O = usize, Lang: ?Sized = ()> {
  /// The offset the input was handed back at — `InputRef::span().end()` once the error is in the
  /// caller's hands, and where the surrounding grammar resumes. At or before the repeated
  /// operator's own start, and strictly before it whenever anything the caller was also handed
  /// back sits between them. See the type's docs.
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

  /// Returns the handback offset — the position the surrounding grammar resumes at, which is at
  /// or before the repeated operator's own start.
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
impl<O, Lang: ?Sized> From<NonAssociativeChain<O, Lang>> for () {
  #[inline(always)]
  fn from(_: NonAssociativeChain<O, Lang>) -> Self {}
}
