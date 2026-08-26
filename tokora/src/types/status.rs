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
