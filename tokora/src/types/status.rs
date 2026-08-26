/// The recovery state of a syntax carrier: whether the parser found the construct, found
/// something malformed where it should have been, or found nothing at all.
///
/// [`Ident`](super::Ident), [`Keyword`](super::Keyword) and every `Lit*` type hold one of these.
/// The rule they are all keeping — that a carrier reports its recovery state through this field
/// and never through its payload — is stated once, on
/// [`ErrorNode`](crate::error::ErrorNode), under *Two Shapes of Implementor*.
///
/// # Why it is a value a consumer can hold
///
/// It is not decoration on the carrier: it is the third thing every carrier is made of, beside
/// the span and the payload. So it appears in
/// [`IntoComponents::Components`](crate::utils::IntoComponents::Components) — which promises a
/// *complete* decomposition — and it is accepted back by each carrier's `with_status`
/// constructor. Those two are inverses, which is what makes the round trip an identity in all
/// three states; without the status in both, a decompose-and-rebuild would report every
/// recovered node as valid syntax, the same laundering tokora#303 removed from `Ident::map`.
///
/// A consumer does not have to interpret it to carry it, and mostly should not: pass it through
/// `with_status`, or ask it one of the three questions below.
///
/// # Non-exhaustive
///
/// A fourth recovery state is admissible in a future version, so a `match` on this enum needs a
/// wildcard arm. [`is_valid`](Self::is_valid) is not `!is_error() && !is_missing()` for that
/// reason — ask the question you mean.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum Status {
  /// The parser found the construct, spelled out in the source.
  Valid,
  /// The parser found something where the construct should have been, and it was malformed —
  /// the state [`ErrorNode::error`](crate::error::ErrorNode::error) declares.
  Error,
  /// The parser found nothing where the construct was required — the state
  /// [`ErrorNode::missing`](crate::error::ErrorNode::missing) declares.
  Missing,
}

impl Status {
  /// Returns `true` if this is the malformed-content state.
  #[inline(always)]
  pub const fn is_error(self) -> bool {
    matches!(self, Self::Error)
  }

  /// Returns `true` if this is the absent-content state.
  #[inline(always)]
  pub const fn is_missing(self) -> bool {
    matches!(self, Self::Missing)
  }

  /// Returns `true` if this is the found-in-the-source state.
  #[inline(always)]
  pub const fn is_valid(self) -> bool {
    matches!(self, Self::Valid)
  }
}

/// Reads the recovery state of a syntax carrier: [`Ident`](super::Ident),
/// [`Keyword`](super::Keyword) and every `Lit*` type implement it.
///
/// # Why the three questions live on a trait
///
/// `is_valid`, `is_error` and `is_missing` are names a consumer may already have on their own
/// extension trait, meaning something of their own — "this literal's payload parses as a number",
/// say. Rust prefers an **inherent** method over a trait's at an unqualified call site, so
/// shipping these as inherent methods would have made `literal.is_valid()` keep compiling and
/// quietly stop calling the consumer's check, with no diagnostic anywhere. tokora#320 widened
/// [`IntoComponents`](crate::utils::IntoComponents) rather than withdrawing it precisely because
/// widening fails loudly, and the same standard applied here rules inherent predicates out: a
/// silent change of meaning on upgrade is the one outcome that standard exists to prevent.
///
/// On a trait the clash cannot be silent. A consumer who imports both gets an ambiguity error
/// naming both candidates and picks with `RecoveryState::is_valid(&x)` or their own equivalent; a
/// consumer who imports neither, or only their own, keeps exactly the behaviour they had.
///
/// # `status` is on this trait too, and nowhere else
///
/// It was briefly an inherent `const fn` on each carrier, on the reasoning that a *return* type
/// which did not exist before could not be displaced silently: a consumer whose own `status()`
/// returned something else would get a type error. **That reasoning is wrong**, and the
/// counterexample is one line:
///
/// ```rust,ignore
/// literal.status().is_valid()
/// ```
///
/// A differing return type is only loud if the caller *stores* the value. When the next step is a
/// method both types offer — and [`Status`] offers exactly the names a consumer's semantic status
/// is most likely to carry — the whole chain still typechecks and the meaning changes underneath
/// it. Since `new` marks any caller-supplied payload [`Status::Valid`], a literal their own check
/// would have rejected then passes.
///
/// So the test a name has to survive is not *does the signature differ* but **does the whole call
/// chain still typecheck with a different meaning**, and an inherent accessor of any name fails
/// it whenever the returned types share a method. There is no inherent reader left to displace
/// anything: `x.status()` and `Carrier::status(&x)` both resolve here.
///
/// # What that costs
///
/// A trait method cannot be `const fn`, so the recovery state is **not readable in a const
/// context** by any spelling. Nothing in this crate read it in one, and the state is a parse
/// result, which is not const-constructible in practice.
///
/// Two ways to keep const were available and both are worse. A `pub` field is reachable by
/// nothing method-call syntax can displace — but a public field is a public *setter*, and
/// `x.status = Status::Valid` would launder a recovery placeholder into valid syntax by
/// assignment, which is the whole defect this type exists to close. A receiver-less associated
/// function is unreachable by method syntax, but `Carrier::status(&x)` in path position displaces
/// a consumer's trait item the same way, so it trades a certainty for an improbability — and the
/// reasoning it would rest on is the reasoning that was just wrong.
pub trait RecoveryState {
  /// The recovery state this value is in.
  fn status(&self) -> Status;

  /// Returns `true` if the parser found this construct spelled out in the source.
  #[inline]
  fn is_valid(&self) -> bool {
    self.status().is_valid()
  }

  /// Returns `true` if this value is a malformed-content placeholder.
  #[inline]
  fn is_error(&self) -> bool {
    self.status().is_error()
  }

  /// Returns `true` if this value is an absent-content placeholder.
  #[inline]
  fn is_missing(&self) -> bool {
    self.status().is_missing()
  }
}
