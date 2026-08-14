use super::*;

// Use the DummyToken from the lexer module which implements LitToken with all defaults
use crate::lexer::DummyToken;

#[test]
fn default_is_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_literal());
}

#[test]
fn default_is_numeric_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_numeric_literal());
}

#[test]
fn default_is_integer_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_integer_literal());
}

#[test]
fn default_is_float_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_float_literal());
}

#[test]
fn default_is_decimal_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_decimal_literal());
}

#[test]
fn default_is_hexadecimal_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_hexadecimal_literal());
}

#[test]
fn default_is_octal_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_octal_literal());
}

#[test]
fn default_is_binary_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_binary_literal());
}

#[test]
fn default_is_hex_float_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_hex_float_literal());
}

#[test]
fn default_is_string_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_string_literal());
}

#[test]
fn default_is_inline_string_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_inline_string_literal());
}

#[test]
fn default_is_multiline_string_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_multiline_string_literal());
}

#[test]
fn default_is_raw_string_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_raw_string_literal());
}

#[test]
fn default_is_char_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_char_literal());
}

#[test]
fn default_is_byte_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_byte_literal());
}

#[test]
fn default_is_byte_string_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_byte_string_literal());
}

#[test]
fn default_is_boolean_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_boolean_literal());
}

#[test]
fn default_is_true_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_true_literal());
}

#[test]
fn default_is_false_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_false_literal());
}

#[test]
fn default_is_null_literal_false() {
  let tok = DummyToken;
  assert!(!tok.is_null_literal());
}

// Test the ref delegation
#[test]
fn ref_delegation() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_literal(r));
  assert!(!LitToken::is_numeric_literal(r));
  assert!(!LitToken::is_string_literal(r));
  assert!(!LitToken::is_boolean_literal(r));
  assert!(!LitToken::is_null_literal(r));
}

// Test all ref delegation methods individually for coverage
#[test]
fn ref_delegation_integer_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_integer_literal(r));
}

#[test]
fn ref_delegation_float_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_float_literal(r));
}

#[test]
fn ref_delegation_decimal_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_decimal_literal(r));
}

#[test]
fn ref_delegation_hexadecimal_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_hexadecimal_literal(r));
}

#[test]
fn ref_delegation_octal_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_octal_literal(r));
}

#[test]
fn ref_delegation_binary_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_binary_literal(r));
}

#[test]
fn ref_delegation_hex_float_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_hex_float_literal(r));
}

#[test]
fn ref_delegation_inline_string_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_inline_string_literal(r));
}

#[test]
fn ref_delegation_multiline_string_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_multiline_string_literal(r));
}

#[test]
fn ref_delegation_raw_string_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_raw_string_literal(r));
}

#[test]
fn ref_delegation_char_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_char_literal(r));
}

#[test]
fn ref_delegation_byte_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_byte_literal(r));
}

#[test]
fn ref_delegation_byte_string_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_byte_string_literal(r));
}

#[test]
fn ref_delegation_true_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_true_literal(r));
}

#[test]
fn ref_delegation_false_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_false_literal(r));
}

#[test]
fn ref_delegation_null_literal() {
  let tok = DummyToken;
  let r: &DummyToken = &tok;
  assert!(!LitToken::is_null_literal(r));
}

// ── Custom token types that return true for specific methods ──────────────

/// A token type that implements LitToken with `is_decimal_literal` returning true.
/// This lets us test the composite methods (is_integer_literal, is_numeric_literal,
/// is_literal) through the "true" code paths.
#[derive(Debug, Clone, PartialEq)]
struct DecimalToken;
impl core::fmt::Display for DecimalToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("decimal")
  }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DummyKind;
impl core::fmt::Display for DummyKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("dummy")
  }
}
impl Token<'_> for DecimalToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for DecimalToken {
  fn is_decimal_literal(&self) -> bool {
    true
  }
}

#[test]
fn decimal_triggers_integer_literal() {
  let tok = DecimalToken;
  assert!(tok.is_decimal_literal());
  assert!(tok.is_integer_literal());
  assert!(tok.is_numeric_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_hexadecimal_literal
#[derive(Debug, Clone, PartialEq)]
struct HexToken;
impl core::fmt::Display for HexToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("hex")
  }
}
impl Token<'_> for HexToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for HexToken {
  fn is_hexadecimal_literal(&self) -> bool {
    true
  }
}

#[test]
fn hex_triggers_integer_and_numeric_literal() {
  let tok = HexToken;
  assert!(tok.is_hexadecimal_literal());
  assert!(tok.is_integer_literal());
  assert!(tok.is_numeric_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_octal_literal
#[derive(Debug, Clone, PartialEq)]
struct OctalToken;
impl core::fmt::Display for OctalToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("octal")
  }
}
impl Token<'_> for OctalToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for OctalToken {
  fn is_octal_literal(&self) -> bool {
    true
  }
}

#[test]
fn octal_triggers_integer_literal() {
  let tok = OctalToken;
  assert!(tok.is_octal_literal());
  assert!(tok.is_integer_literal());
}

/// Token returning true for is_binary_literal
#[derive(Debug, Clone, PartialEq)]
struct BinaryToken;
impl core::fmt::Display for BinaryToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("binary")
  }
}
impl Token<'_> for BinaryToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for BinaryToken {
  fn is_binary_literal(&self) -> bool {
    true
  }
}

#[test]
fn binary_triggers_integer_literal() {
  let tok = BinaryToken;
  assert!(tok.is_binary_literal());
  assert!(tok.is_integer_literal());
}

/// Token returning true for is_float_literal
#[derive(Debug, Clone, PartialEq)]
struct FloatToken;
impl core::fmt::Display for FloatToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("float")
  }
}
impl Token<'_> for FloatToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for FloatToken {
  fn is_float_literal(&self) -> bool {
    true
  }
}

#[test]
fn float_triggers_numeric_and_literal() {
  let tok = FloatToken;
  assert!(tok.is_float_literal());
  assert!(!tok.is_integer_literal()); // float is not integer
  assert!(tok.is_numeric_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_hex_float_literal
#[derive(Debug, Clone, PartialEq)]
struct HexFloatToken;
impl core::fmt::Display for HexFloatToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("hexfloat")
  }
}
impl Token<'_> for HexFloatToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for HexFloatToken {
  fn is_hex_float_literal(&self) -> bool {
    true
  }
}

#[test]
fn hex_float_triggers_numeric_and_literal() {
  let tok = HexFloatToken;
  assert!(tok.is_hex_float_literal());
  assert!(tok.is_numeric_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_inline_string_literal
#[derive(Debug, Clone, PartialEq)]
struct InlineStringToken;
impl core::fmt::Display for InlineStringToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("istr")
  }
}
impl Token<'_> for InlineStringToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for InlineStringToken {
  fn is_inline_string_literal(&self) -> bool {
    true
  }
}

#[test]
fn inline_string_triggers_string_and_literal() {
  let tok = InlineStringToken;
  assert!(tok.is_inline_string_literal());
  assert!(tok.is_string_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_multiline_string_literal
#[derive(Debug, Clone, PartialEq)]
struct MultilineStringToken;
impl core::fmt::Display for MultilineStringToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("mstr")
  }
}
impl Token<'_> for MultilineStringToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for MultilineStringToken {
  fn is_multiline_string_literal(&self) -> bool {
    true
  }
}

#[test]
fn multiline_string_triggers_string_and_literal() {
  let tok = MultilineStringToken;
  assert!(tok.is_multiline_string_literal());
  assert!(tok.is_string_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_raw_string_literal
#[derive(Debug, Clone, PartialEq)]
struct RawStringToken;
impl core::fmt::Display for RawStringToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("rstr")
  }
}
impl Token<'_> for RawStringToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for RawStringToken {
  fn is_raw_string_literal(&self) -> bool {
    true
  }
}

#[test]
fn raw_string_triggers_literal() {
  let tok = RawStringToken;
  assert!(tok.is_raw_string_literal());
  assert!(!tok.is_string_literal()); // raw_string is separate from string
  assert!(tok.is_literal());
}

/// Token returning true for is_char_literal
#[derive(Debug, Clone, PartialEq)]
struct CharToken;
impl core::fmt::Display for CharToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("char")
  }
}
impl Token<'_> for CharToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for CharToken {
  fn is_char_literal(&self) -> bool {
    true
  }
}

#[test]
fn char_triggers_literal() {
  let tok = CharToken;
  assert!(tok.is_char_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_byte_literal
#[derive(Debug, Clone, PartialEq)]
struct ByteToken;
impl core::fmt::Display for ByteToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("byte")
  }
}
impl Token<'_> for ByteToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for ByteToken {
  fn is_byte_literal(&self) -> bool {
    true
  }
}

#[test]
fn byte_triggers_literal() {
  let tok = ByteToken;
  assert!(tok.is_byte_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_byte_string_literal
#[derive(Debug, Clone, PartialEq)]
struct ByteStringToken;
impl core::fmt::Display for ByteStringToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("bstr")
  }
}
impl Token<'_> for ByteStringToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for ByteStringToken {
  fn is_byte_string_literal(&self) -> bool {
    true
  }
}

#[test]
fn byte_string_triggers_literal() {
  let tok = ByteStringToken;
  assert!(tok.is_byte_string_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_true_literal
#[derive(Debug, Clone, PartialEq)]
struct TrueToken;
impl core::fmt::Display for TrueToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("true")
  }
}
impl Token<'_> for TrueToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for TrueToken {
  fn is_true_literal(&self) -> bool {
    true
  }
}

#[test]
fn true_triggers_boolean_and_literal() {
  let tok = TrueToken;
  assert!(tok.is_true_literal());
  assert!(tok.is_boolean_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_false_literal
#[derive(Debug, Clone, PartialEq)]
struct FalseToken;
impl core::fmt::Display for FalseToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("false")
  }
}
impl Token<'_> for FalseToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for FalseToken {
  fn is_false_literal(&self) -> bool {
    true
  }
}

#[test]
fn false_triggers_boolean_and_literal() {
  let tok = FalseToken;
  assert!(tok.is_false_literal());
  assert!(tok.is_boolean_literal());
  assert!(tok.is_literal());
}

/// Token returning true for is_null_literal
#[derive(Debug, Clone, PartialEq)]
struct NullToken;
impl core::fmt::Display for NullToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("null")
  }
}
impl Token<'_> for NullToken {
  type Kind = DummyKind;
  type Error = ();

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> DummyKind {
    DummyKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}
impl LitToken<'_> for NullToken {
  fn is_null_literal(&self) -> bool {
    true
  }
}

#[test]
fn null_triggers_literal() {
  let tok = NullToken;
  assert!(tok.is_null_literal());
  assert!(tok.is_literal());
}

// ── Ref delegation with true-returning tokens ──────────────────────────

#[test]
fn ref_delegation_decimal_returns_true() {
  let tok = DecimalToken;
  let r: &DecimalToken = &tok;
  assert!(LitToken::is_decimal_literal(r));
  assert!(LitToken::is_integer_literal(r));
  assert!(LitToken::is_numeric_literal(r));
  assert!(LitToken::is_literal(r));
}

#[test]
fn ref_delegation_float_returns_true() {
  let tok = FloatToken;
  let r: &FloatToken = &tok;
  assert!(LitToken::is_float_literal(r));
  assert!(LitToken::is_numeric_literal(r));
}

#[test]
fn ref_delegation_hex_float_returns_true() {
  let tok = HexFloatToken;
  let r: &HexFloatToken = &tok;
  assert!(LitToken::is_hex_float_literal(r));
  assert!(LitToken::is_numeric_literal(r));
}

#[test]
fn ref_delegation_inline_string_returns_true() {
  let tok = InlineStringToken;
  let r: &InlineStringToken = &tok;
  assert!(LitToken::is_inline_string_literal(r));
  assert!(LitToken::is_string_literal(r));
}

#[test]
fn ref_delegation_multiline_string_returns_true() {
  let tok = MultilineStringToken;
  let r: &MultilineStringToken = &tok;
  assert!(LitToken::is_multiline_string_literal(r));
  assert!(LitToken::is_string_literal(r));
}

#[test]
fn ref_delegation_raw_string_returns_true() {
  let tok = RawStringToken;
  let r: &RawStringToken = &tok;
  assert!(LitToken::is_raw_string_literal(r));
  assert!(LitToken::is_literal(r));
}

#[test]
fn ref_delegation_char_returns_true() {
  let tok = CharToken;
  let r: &CharToken = &tok;
  assert!(LitToken::is_char_literal(r));
  assert!(LitToken::is_literal(r));
}

#[test]
fn ref_delegation_byte_returns_true() {
  let tok = ByteToken;
  let r: &ByteToken = &tok;
  assert!(LitToken::is_byte_literal(r));
  assert!(LitToken::is_literal(r));
}

#[test]
fn ref_delegation_byte_string_returns_true() {
  let tok = ByteStringToken;
  let r: &ByteStringToken = &tok;
  assert!(LitToken::is_byte_string_literal(r));
  assert!(LitToken::is_literal(r));
}

#[test]
fn ref_delegation_true_returns_true() {
  let tok = TrueToken;
  let r: &TrueToken = &tok;
  assert!(LitToken::is_true_literal(r));
  assert!(LitToken::is_boolean_literal(r));
}

#[test]
fn ref_delegation_false_returns_true() {
  let tok = FalseToken;
  let r: &FalseToken = &tok;
  assert!(LitToken::is_false_literal(r));
  assert!(LitToken::is_boolean_literal(r));
}

#[test]
fn ref_delegation_null_returns_true() {
  let tok = NullToken;
  let r: &NullToken = &tok;
  assert!(LitToken::is_null_literal(r));
  assert!(LitToken::is_literal(r));
}

#[test]
fn ref_delegation_hex_returns_true() {
  let tok = HexToken;
  let r: &HexToken = &tok;
  assert!(LitToken::is_hexadecimal_literal(r));
  assert!(LitToken::is_integer_literal(r));
}

#[test]
fn ref_delegation_octal_returns_true() {
  let tok = OctalToken;
  let r: &OctalToken = &tok;
  assert!(LitToken::is_octal_literal(r));
  assert!(LitToken::is_integer_literal(r));
}

#[test]
fn ref_delegation_binary_returns_true() {
  let tok = BinaryToken;
  let r: &BinaryToken = &tok;
  assert!(LitToken::is_binary_literal(r));
  assert!(LitToken::is_integer_literal(r));
}
