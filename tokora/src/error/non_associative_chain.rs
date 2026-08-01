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
/// # The offset, and one documented limit of it
///
/// [`offset`](Self::offset) is the **start** of the deciding token. The token engine reads it
/// straight off the token it parked, so it is exactly the offending operator's start. The typed
/// engine reads the committed span of the last token the RHS classifier consumed while deciding,
/// which is the same thing for a one-token operator and the *final* token of a multi-token one.
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
  /// The start offset of the deciding token — the repeated operator, left on the input.
  at: O,
  _lang: PhantomData<Lang>,
}

impl<O, Lang: ?Sized> NonAssociativeChain<O, Lang> {
  /// Creates the error for a repeat whose deciding token starts at `at`.
  #[inline(always)]
  pub const fn of(at: O) -> Self {
    Self {
      at,
      _lang: PhantomData,
    }
  }

  /// Returns the start offset of the repeated operator.
  #[inline(always)]
  pub const fn offset(&self) -> O
  where
    O: Copy,
  {
    self.at
  }

  /// Returns a reference to the start offset of the repeated operator.
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
