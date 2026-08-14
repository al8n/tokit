use derive_more::{Display, IsVariant};

use super::human_display::DisplayHuman;

/// A displayable hex float literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("hex float literal")]
pub struct HexFloatLiteral(pub(crate) ());

impl DisplayHuman for HexFloatLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable float literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("float literal")]
pub struct FloatLiteral(pub(crate) ());

impl DisplayHuman for FloatLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable int literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("int literal")]
pub struct IntLiteral(pub(crate) ());

impl DisplayHuman for IntLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable decimal literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("decimal literal")]
pub struct DecimalLiteral(pub(crate) ());

impl DisplayHuman for DecimalLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable hex literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("hex literal")]
pub struct HexLiteral(pub(crate) ());

impl DisplayHuman for HexLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable binary literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("binary literal")]
pub struct BinaryLiteral(pub(crate) ());

impl DisplayHuman for BinaryLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable octal literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("octal literal")]
pub struct OctalLiteral(pub(crate) ());

impl DisplayHuman for OctalLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable string literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("string literal")]
pub struct StringLiteral(pub(crate) ());

impl DisplayHuman for StringLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable boolean literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("boolean literal")]
pub struct BooleanLiteral(pub(crate) ());

impl DisplayHuman for BooleanLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable null literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("null literal")]
pub struct NullLiteral(pub(crate) ());

impl DisplayHuman for NullLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable enum literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("enum literal")]
pub struct EnumLiteral(pub(crate) ());

impl DisplayHuman for EnumLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable enum literal description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("enum value literal")]
pub struct EnumValueLiteral(pub(crate) ());

impl DisplayHuman for EnumValueLiteral {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// A displayable character description.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Display)]
#[display("characters")]
pub struct Characters(pub(crate) ());

impl DisplayHuman for Characters {
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    ::core::fmt::Display::fmt(self, f)
  }
}

/// An enumeration of line terminator types.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, IsVariant, Display)]
pub enum LineTerminator {
  /// A newline character (`\n`).
  #[display("\\n")]
  NewLine,
  /// A carriage return character (`\r`).
  #[display("\\r")]
  CarriageReturn,
  /// A carriage return followed by a newline (`\r\n`).
  #[display("\\r\\n")]
  CarriageReturnNewLine,
}
