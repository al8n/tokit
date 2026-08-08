/// How much a diagnostic asks of its reader.
///
/// # Not [`emitter::Severity`](crate::emitter::Severity)
///
/// The two answer different questions and neither can be spelled with the other.
/// [`emitter::Severity`](crate::emitter::Severity) is a *channel selector*: it decides which of a
/// collecting emitter's two stores a record lands in during a parse, and a
/// [`Fatal`](crate::emitter::Fatal) parse reads it to decide whether to stop. It has exactly the
/// two tiers that machinery has channels for, and it is not `#[non_exhaustive]`, so a third one
/// cannot be added without breaking every consumer that matches on it.
///
/// This one is a *reporting ladder*, read after the fact by whatever renders the finished
/// diagnostic. It carries the extra rung — [`Advice`](Self::Advice) — that a lint class needs and
/// that the emitter has no channel for.
///
/// The emitter's two tiers embed into this ladder totally, so a consumer rendering collected
/// records beside [`Diagnose`](super::Diagnose) values converts with [`From`]:
///
/// ```
/// use tokora::{diagnostic, emitter};
///
/// assert_eq!(diagnostic::Severity::from(emitter::Severity::Error), diagnostic::Severity::Error);
/// assert_eq!(
///   diagnostic::Severity::from(emitter::Severity::Warning),
///   diagnostic::Severity::Warning,
/// );
/// ```
///
/// There is deliberately no conversion the other way. It would have to answer what channel an
/// [`Advice`](Self::Advice) goes down, and the honest answer is that the emitter has none.
///
/// # Three rungs, declared before anything needs the third
///
/// Every rung exists so that a contract admitting only errors does not have to be revisited when
/// the first advisory class arrives — a deprecation lint, a portability note, a style rule: advice
/// about an input that is perfectly valid and must not change any verdict. Adding the level
/// afterwards would mean revisiting every implementor; declaring it now costs one method.
///
/// It is `#[non_exhaustive]` for the same reason, in the other direction: a consumer's `match` has
/// to carry a fallback arm, so a fourth rung is an addition rather than a break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Severity {
  /// The input is refused. A verdict depends on it.
  Error,
  /// The input is accepted and something about it is worth saying.
  Warning,
  /// A suggestion, carrying no judgement about the input.
  Advice,
}

impl Severity {
  /// Returns the level's lowercase name.
  ///
  /// ```
  /// use tokora::diagnostic::Severity;
  ///
  /// assert_eq!(Severity::Error.as_str(), "error");
  /// assert_eq!(Severity::Warning.as_str(), "warning");
  /// assert_eq!(Severity::Advice.as_str(), "advice");
  /// ```
  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Error => "error",
      Self::Warning => "warning",
      Self::Advice => "advice",
    }
  }
}

impl core::fmt::Display for Severity {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(self.as_str())
  }
}

impl From<crate::emitter::Severity> for Severity {
  /// Lifts an emitter channel tier onto the reporting ladder.
  ///
  /// Total in this direction, and deliberately absent in the other: see the type's documentation.
  #[inline]
  fn from(severity: crate::emitter::Severity) -> Self {
    match severity {
      crate::emitter::Severity::Error => Self::Error,
      crate::emitter::Severity::Warning => Self::Warning,
    }
  }
}
