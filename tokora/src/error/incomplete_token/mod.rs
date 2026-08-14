use crate::{
  span::{SimpleSpan, Span},
  utils::human_display::DisplayHuman,
};

/// A token that ended before its lexical form was complete.
///
/// Raised by lexers when the input stops (or switches to a non-continuable character) partway
/// through a token whose shape demands more — a block string cut off mid-body, an exponent
/// marker with no digits, a prefix with no payload. Unlike
/// [`Incomplete`](crate::error::Incomplete) — the *control-flow* sentinel for "more input may
/// still arrive" in partial mode — an `IncompleteToken` is a genuine *diagnostic* about text
/// that is present but truncated.
///
/// The `span` covers the truncated token as far as it got: from its first byte to the offset
/// where lexing gave up. The optional `Knowledge` names what kind of token was being lexed
/// (rendered *"incomplete `<knowledge>` token at `<span>`"*); without it the message is the
/// generic *"incomplete token at `<span>`"*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IncompleteToken<Knowledge, S = SimpleSpan> {
  span: S,
  knowledge: Option<Knowledge>,
}

impl<Knowledge> core::fmt::Display for IncompleteToken<Knowledge>
where
  Knowledge: DisplayHuman,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match &self.knowledge {
      Some(knowledge) => write!(
        f,
        "incomplete {} token at {}",
        knowledge.display(),
        self.span
      ),
      None => write!(f, "incomplete token at {}", self.span),
    }
  }
}

impl<Knowledge> core::error::Error for IncompleteToken<Knowledge> where
  Knowledge: DisplayHuman + core::fmt::Debug
{
}

impl<Knowledge, S> IncompleteToken<Knowledge, S> {
  #[inline]
  const fn new_in(span: S, knowledge: Option<Knowledge>) -> Self {
    Self { span, knowledge }
  }

  /// Create a new `IncompleteToken` without any knowledge.
  #[inline]
  pub const fn new(span: S) -> Self {
    Self::new_in(span, None)
  }

  /// Create a new `IncompleteToken` with knowledge.
  #[inline]
  pub const fn with_knowledge(span: S, knowledge: Knowledge) -> Self {
    Self::new_in(span, Some(knowledge))
  }

  /// Get the span of the incomplete token.
  #[inline]
  pub const fn span(&self) -> S
  where
    S: Copy,
  {
    self.span
  }

  /// Get the knowledge for the incomplete token, if any.
  #[inline]
  pub const fn knowledge(&self) -> Option<&Knowledge> {
    self.knowledge.as_ref()
  }

  /// Decompose the `IncompleteToken` into its components.
  #[inline]
  pub fn into_components(self) -> (S, Option<Knowledge>) {
    (self.span, self.knowledge)
  }

  /// Bumps both the start and end positions of the span by the given offset.
  ///
  /// This is useful when adjusting error positions after processing or
  /// when combining spans from different contexts.
  #[inline]
  pub fn bump(&mut self, offset: &S::Offset)
  where
    S: Span,
  {
    self.span.bump(offset);
  }
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(feature = "std")]
mod tests;
