use derive_more::{Display, IsVariant};

/// The severity tier of a diagnostic.
///
/// Tokora models two tiers only — [`Error`](Self::Error) and [`Warning`](Self::Warning) —
/// deliberately keeping the ladder short (no `Info`/`Hint`/`Note` rungs until a caller
/// actually needs them). The tier is a *classification*, not a control-flow decision: whether
/// a diagnostic stops parsing is the [`Emitter`](crate::Emitter)'s policy, not the severity's.
/// A [`Fatal`](crate::emitter::Fatal) parse stops on an error and ignores a warning; a
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " [`Verbose`](crate::emitter::Verbose) parse collects both into parallel channels."
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " `Verbose` parse collects both into parallel channels."
)]
///
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " The severity rides the read-side [`Diagnostic`](crate::emitter::Diagnostic) view so a"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " The severity rides the read-side `Diagnostic` view so a"
)]
/// downstream renderer (ariadne, miette, a custom reporter) can map each entry onto its own
/// report kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IsVariant, Display)]
#[display("{}", self.as_str())]
pub enum Severity {
  /// A hard diagnostic: something is wrong. Under a fail-fast emitter an error stops the parse;
  /// under a collecting emitter it lands in the error channel.
  Error,
  /// A soft diagnostic: something is worth reporting but does not, on its own, stop anything. A
  /// warning is *never* fatal — a fail-fast parse has no warning sink and drops it, while a
  /// collecting emitter keeps it in a channel parallel to the errors.
  Warning,
}

impl Severity {
  /// Returns the stable, lowercase string name of this severity tier.
  ///
  /// This is the single source of truth for the tier's name; [`Display`](core::fmt::Display)
  /// routes through it.
  ///
  /// ```
  /// use tokora::emitter::Severity;
  ///
  /// assert_eq!(Severity::Error.as_str(), "error");
  /// assert_eq!(Severity::Warning.as_str(), "warning");
  /// assert_eq!(Severity::Warning.to_string(), "warning");
  /// ```
  #[inline(always)]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Error => "error",
      Self::Warning => "warning",
    }
  }
}
