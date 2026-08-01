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
/// # The offset is the handback position
///
/// [`offset`](Self::offset) is **defined** as the position the input was handed back at: the byte
/// the surrounding grammar — or a recoverer — resumes reading from once the deciding read has been
/// restored. It is *not* defined as the offending operator's first byte, and the two are not
/// always the same number.
///
/// The operator's head is not a quantity the typed engine can obtain in general.
/// [`ParsePrattRHS`](crate::parser::ParsePrattRHS) is caller code holding a whole
/// [`InputRef`](crate::InputRef), and it decides for itself where its operator begins: a
/// CST-style classifier over a trivia-surfacing lexer skips whitespace and comment **tokens**
/// before it reads the operator, which the contract permits and which the driver cannot see
/// without *running* the classifier — and running it before the repeat has been decided is what
/// the driver's transaction rules forbid. So the driver reports the position it does know: the
/// point its own probe restores to. Whatever the classifier would have skipped is in front of
/// that point, and in front of the caller too:
///
/// ```text
///   1 ; 2   ; 3        `;` non-associative, whitespace lexed as tokens
///   0 2 4   8 10       the second `;` is at 8 — the repeat
///       ^              offset 5: the whitespace the handback returns, and where the caller resumes
/// ```
///
/// A grammar whose lexer skips trivia — the ordinary syntactic shape — has nothing between the
/// two, so its offset is the operator's head as well. That is a property of those grammars, not a
/// promise of this field.
///
/// **What is guaranteed, by construction rather than by agreement, is that the reported offset and
/// the resumption point are one number.** That is the property recovery correctness rests on: hand
/// the error to [`Recover`](crate::parser::Recover) or
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) and the offset it reads is the offset
/// the retry begins at.
///
/// **How each engine gets there.** Neither reconstructs the number after the fact. The token
/// engine reads it off the token it parked, and there the handback and the operator's head always
/// coincide: its classifier is a pure function of one token, so a trivia token reaching it ends
/// the expression rather than being skipped — a token-level pratt grammar is a trivia-less grammar
/// by construction. The typed engine reads the position *before* running its classifier, from the
/// token that classifier is about to see first. Reading it afterwards instead — off the committed
/// span, which by then holds the classifier's **last** token — would name the operator's *tail*
/// for a multi-token spelling (`not in`, `is not`, `<>`): neither the handback nor the head, and a
/// position no caller is ever handed.
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
  /// The offset the input was handed back at — where the surrounding grammar resumes. At or
  /// before the repeated operator's own start; strictly before it when the classifier skipped
  /// trivia tokens to reach it. See the type's docs.
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
