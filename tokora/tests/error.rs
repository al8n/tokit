#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc as std;

#[cfg(feature = "std")]
extern crate std;

#[cfg(any(feature = "alloc", feature = "std"))]
use std::string::{String, ToString};

use tokora::SimpleSpan;
use tokora::error::token::{MissingToken, SeparatedError, SeparatorPosition, UnexpectedToken};
/// Tests for all error types in the tokora error module.
/// Exercises constructors, methods, Display/Debug impls, and transformations.
use tokora::error::*;
use tokora::utils::{CowStr, Expected, PositionedChar, knowledge::*};

// ── UnexpectedEnd ──────────────────────────────────────────────────────────────

#[test]
fn unexpected_end_eof() {
  let e = UnexpectedEnd::eof(100usize);
  assert_eq!(e.offset(), 100);
  assert_eq!(e.name(), Some("file"));
  assert_eq!(e.to_string(), "unexpected end of file, expected byte");
}

#[test]
fn unexpected_end_eot() {
  let e = UnexpectedEnd::eot(50usize);
  assert_eq!(e.offset(), 50);
  assert_eq!(e.name(), Some("token stream"));
  assert_eq!(
    e.to_string(),
    "unexpected end of token stream, expected token"
  );
}

#[test]
fn unexpected_end_eos() {
  let e = UnexpectedEnd::eos(25usize);
  assert_eq!(e.offset(), 25);
  assert_eq!(e.name(), Some("string"));
  assert_eq!(
    e.to_string(),
    "unexpected end of string, expected character"
  );
}

#[test]
fn unexpected_end_eorhs() {
  let e = UnexpectedEnd::eorhs(100usize);
  assert_eq!(e.offset(), 100);
  assert_eq!(e.name(), Some("expression (right hand side)"));
  assert_eq!(
    e.to_string(),
    "unexpected end of expression (right hand side), expected either an infix or a postfix"
  );
}

#[test]
fn unexpected_end_eolhs() {
  let e = UnexpectedEnd::eolhs(100usize);
  assert_eq!(e.offset(), 100);
  assert_eq!(e.name(), Some("expression (left hand side)"));
  assert_eq!(
    e.to_string(),
    "unexpected end of expression (left hand side), expected one of an operand, an infix or a postfix"
  );
}

#[test]
fn unexpected_end_new_no_name() {
  let e = UnexpectedEnd::new(10usize, FileHint);
  assert_eq!(e.name(), None);
  assert_eq!(e.offset(), 10);
  assert_eq!(e.to_string(), "unexpected end, expected byte");
}

#[test]
fn unexpected_end_with_hint() {
  let e = UnexpectedEnd::with_hint(15usize, TokenHint);
  assert_eq!(e.name(), None);
  assert_eq!(e.offset(), 15);
}

#[test]
fn unexpected_end_set_and_clear_name() {
  let mut e = UnexpectedEnd::new(10usize, FileHint);
  e.set_name("expression");
  assert_eq!(e.name(), Some("expression"));
  e.clear_name();
  assert_eq!(e.name(), None);
}

#[test]
#[cfg(any(feature = "alloc", feature = "std"))]
fn unexpected_end_update_name() {
  let mut e = UnexpectedEnd::eof(10usize);
  e.update_name(Some("block"));
  assert_eq!(e.name(), Some("block"));
  e.update_name(None::<String>);
  assert_eq!(e.name(), None);
}

#[test]
fn unexpected_end_bump() {
  let mut e = UnexpectedEnd::eof(10usize);
  e.bump(&5);
  assert_eq!(e.offset(), 15);
}

#[test]
fn unexpected_end_map_hint() {
  let e = UnexpectedEnd::eof(100usize);
  let e2 = e.map_hint(|_| TokenHint);
  assert_eq!(e2.name(), Some("file"));
  assert_eq!(e2.offset(), 100);
}

#[test]
fn unexpected_end_reconstruct() {
  let e = UnexpectedEnd::eof(100usize);
  let e2 = e.reconstruct(Some("block"), |_| TokenHint);
  assert_eq!(e2.name(), Some("block"));
  assert_eq!(e2.offset(), 100);
}

#[test]
fn unexpected_end_reconstruct_with_name() {
  let e = UnexpectedEnd::eof(100usize);
  let e2 = e.reconstruct_with_name("expression", |_| TokenHint);
  assert_eq!(e2.name(), Some("expression"));
}

#[test]
fn unexpected_end_reconstruct_without_name() {
  use tokora::utils::CowStr;
  let e = UnexpectedEnd::with_name(10usize, CowStr::from_static("file"), FileHint);
  let e2 = e.reconstruct_without_name(|_| TokenHint);
  assert_eq!(e2.name(), None);
}

#[test]
fn unexpected_end_replace_hint() {
  let mut e = UnexpectedEnd::eof(100usize);
  let _old = e.replace_hint(FileHint);
}

#[test]
fn unexpected_end_into_components() {
  let e = UnexpectedEnd::eof(100usize);
  let (offset, name, _hint) = e.into_components();
  assert_eq!(offset, 100);
  assert!(name.is_some());
}

#[test]
fn unexpected_end_offset_ref() {
  let e = UnexpectedEnd::eof(100usize);
  assert_eq!(*e.offset_ref(), 100);
}

#[test]
fn unexpected_end_hint_ref() {
  let e = UnexpectedEnd::eof(100usize);
  let _ = e.hint();
}

/// A stand-in for the token-kind table a committed dispatch driver attaches to its
/// end-of-input error: any `Set` other than the defaulted `&'static str`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DispatchKind(u8);

const DISPATCH_TABLE: &[DispatchKind] = &[DispatchKind(1), DispatchKind(2)];

// `offset`/`offset_ref`/`name`/`hint` do not depend on the expected set, and are readable
// at every `Set` — not only at the default one every other case in this file sits at.
// Without that the `Kind`-table spelling of an end-of-input is position-less to a consumer,
// which is exactly the arm `examples/json.rs` had to map to a bare message.
#[test]
fn unexpected_end_reads_are_set_independent() {
  let e: UnexpectedEnd<TokenHint, usize, (), DispatchKind> =
    UnexpectedEnd::eot_expected_one_of(42, DISPATCH_TABLE);

  assert_eq!(e.offset(), 42);
  assert_eq!(*e.offset_ref(), 42);
  assert_eq!(e.name(), Some("token stream"));
  let _: &TokenHint = e.hint();
  // The set itself is still there; the four reads above are simply orthogonal to it.
  assert!(matches!(e.expected(), Some(Expected::OneOf(_))));
}

#[test]
fn unexpected_end_debug() {
  let e = UnexpectedEnd::eof(100usize);
  let s = format!("{e:?}");
  assert!(s.contains("UnexpectedEnd") || s.contains("offset"));
}

#[test]
fn unexpected_end_eorhs_of() {
  let e = UnexpectedEnd::<PrattRhsHint, usize>::eorhs_of(42);
  assert_eq!(e.offset(), 42);
}

#[test]
fn unexpected_end_eolhs_of() {
  let e = UnexpectedEnd::<PrattLhsHint, usize>::eolhs_of(42);
  assert_eq!(e.offset(), 42);
}

#[test]
fn unexpected_end_eof_of() {
  let e = UnexpectedEnd::<FileHint, usize>::eof_of(42);
  assert_eq!(e.offset(), 42);
}

#[test]
fn unexpected_end_eot_of() {
  let e = UnexpectedEnd::<TokenHint, usize>::eot_of(42);
  assert_eq!(e.offset(), 42);
}

#[test]
fn unexpected_end_eos_of() {
  let e = UnexpectedEnd::<CharacterHint, usize>::eos_of(42);
  assert_eq!(e.offset(), 42);
}

#[test]
fn unexpected_end_maybe_name() {
  use tokora::utils::CowStr;
  let e = UnexpectedEnd::maybe_name(10usize, Some(CowStr::from_static("string")), FileHint);
  assert_eq!(e.name(), Some("string"));
  assert_eq!(e.offset(), 10);
}

#[test]
fn unexpected_end_with_name() {
  use tokora::utils::CowStr;
  let e = UnexpectedEnd::with_name(20usize, CowStr::from_static("block"), FileHint);
  assert_eq!(e.name(), Some("block"));
}

#[test]
fn hint_displays() {
  assert_eq!(FileHint.to_string(), "byte");
  assert_eq!(TokenHint.to_string(), "token");
  assert_eq!(CharacterHint.to_string(), "character");
  assert_eq!(PrattRhsHint.to_string(), "either an infix or a postfix");
  assert_eq!(
    PrattLhsHint.to_string(),
    "one of an operand, an infix or a postfix"
  );
}

#[test]
fn unexpected_end_from_tuple() {
  let e: UnexpectedEof = (100usize, FileHint).into();
  assert_eq!(e.offset(), 100);
}

// ── Malformed ─────────────────────────────────────────────────────────────────

#[test]
fn malformed_int() {
  let e = Malformed::int(SimpleSpan::new(10, 16));
  assert_eq!(
    e.to_string(),
    "malformed at 10..16, did you mean int literal?"
  );
  assert_eq!(e.span(), SimpleSpan::new(10, 16));
}

#[test]
fn malformed_float() {
  let e = Malformed::float(SimpleSpan::new(5, 13));
  assert_eq!(
    e.to_string(),
    "malformed at 5..13, did you mean float literal?"
  );
}

#[test]
fn malformed_hex() {
  let e = Malformed::hex(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean hex literal?"
  );
}

#[test]
fn malformed_binary() {
  let e = Malformed::binary(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean binary literal?"
  );
}

#[test]
fn malformed_octal() {
  let e = Malformed::octal(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean octal literal?"
  );
}

#[test]
fn malformed_decimal() {
  let e = Malformed::decimal(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean decimal literal?"
  );
}

#[test]
fn malformed_string() {
  let e = Malformed::string(SimpleSpan::new(0, 5));
  assert_eq!(
    e.to_string(),
    "malformed at 0..5, did you mean string literal?"
  );
}

#[test]
fn malformed_boolean() {
  let e = Malformed::boolean(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean boolean literal?"
  );
}

#[test]
fn malformed_null() {
  let e = Malformed::null(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean null literal?"
  );
}

#[test]
fn malformed_hex_float() {
  let e = Malformed::hex_float(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean hex float literal?"
  );
}

#[test]
fn malformed_new_no_knowledge() {
  let e: Malformed<()> = Malformed::new(SimpleSpan::new(20, 25));
  assert_eq!(e.to_string(), "malformed at 20..25");
  assert!(e.knowledge().is_none());
}

#[test]
fn malformed_with_knowledge() {
  let e = Malformed::with_knowledge(SimpleSpan::new(15, 17), HexLiteral::default());
  assert!(e.knowledge().is_some());
}

#[test]
fn malformed_span_ref() {
  let e = Malformed::int(SimpleSpan::new(1, 5));
  assert_eq!(*e.span_ref(), SimpleSpan::new(1, 5));
}

#[test]
fn malformed_into_components() {
  let e = Malformed::int(SimpleSpan::new(1, 5));
  let (span, _knowledge) = e.into_components();
  assert_eq!(span, SimpleSpan::new(1, 5));
}

#[test]
fn malformed_bump() {
  let mut e = Malformed::int(SimpleSpan::new(5, 10));
  e.bump(&3usize);
  assert_eq!(e.span(), SimpleSpan::new(8, 13));
}

#[test]
fn malformed_enum_literal() {
  let e = Malformed::enumeration(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean enum literal?"
  );
}

#[test]
fn malformed_enum_value_literal() {
  let e = Malformed::enum_value(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "malformed at 0..4, did you mean enum value literal?"
  );
}

#[test]
fn malformed_debug() {
  let e = Malformed::int(SimpleSpan::new(1, 5));
  let s = format!("{e:?}");
  assert!(!s.is_empty());
}

// ── Invalid ────────────────────────────────────────────────────────────────────

#[test]
fn invalid_int() {
  let e = Invalid::int(SimpleSpan::new(10, 16));
  assert_eq!(
    e.to_string(),
    "invalid at 10..16, did you mean int literal?"
  );
}

#[test]
fn invalid_float() {
  let e = Invalid::float(SimpleSpan::new(5, 13));
  assert_eq!(
    e.to_string(),
    "invalid at 5..13, did you mean float literal?"
  );
}

#[test]
fn invalid_hex() {
  let e = Invalid::hex(SimpleSpan::new(0, 4));
  assert_eq!(e.to_string(), "invalid at 0..4, did you mean hex literal?");
}

#[test]
fn invalid_string() {
  let e = Invalid::string(SimpleSpan::new(0, 5));
  assert_eq!(
    e.to_string(),
    "invalid at 0..5, did you mean string literal?"
  );
}

#[test]
fn invalid_boolean() {
  let e = Invalid::boolean(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "invalid at 0..4, did you mean boolean literal?"
  );
}

#[test]
fn invalid_null() {
  let e = Invalid::null(SimpleSpan::new(0, 4));
  assert_eq!(e.to_string(), "invalid at 0..4, did you mean null literal?");
}

#[test]
fn invalid_hex_float() {
  let e = Invalid::hex_float(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "invalid at 0..4, did you mean hex float literal?"
  );
}

#[test]
fn invalid_decimal() {
  let e = Invalid::decimal(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "invalid at 0..4, did you mean decimal literal?"
  );
}

#[test]
fn invalid_binary() {
  let e = Invalid::binary(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "invalid at 0..4, did you mean binary literal?"
  );
}

#[test]
fn invalid_octal() {
  let e = Invalid::octal(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "invalid at 0..4, did you mean octal literal?"
  );
}

#[test]
fn invalid_new_no_knowledge() {
  let e: Invalid<()> = Invalid::new(SimpleSpan::new(20, 25));
  assert_eq!(e.to_string(), "invalid at 20..25");
}

#[test]
fn invalid_with_knowledge() {
  let e = Invalid::with_knowledge(SimpleSpan::new(15, 17), HexLiteral::default());
  assert!(e.knowledge().is_some());
}

#[test]
fn invalid_into_components() {
  let e = Invalid::int(SimpleSpan::new(1, 5));
  let (span, _k) = e.into_components();
  assert_eq!(span, SimpleSpan::new(1, 5));
}

#[test]
fn invalid_bump() {
  let mut e = Invalid::int(SimpleSpan::new(5, 10));
  e.bump(&3usize);
  assert_eq!(e.span(), SimpleSpan::new(8, 13));
}

#[test]
fn invalid_enumeration() {
  let e = Invalid::enumeration(SimpleSpan::new(0, 4));
  assert_eq!(e.to_string(), "invalid at 0..4, did you mean enum literal?");
}

#[test]
fn invalid_enum_value() {
  let e = Invalid::enum_value(SimpleSpan::new(0, 4));
  assert_eq!(
    e.to_string(),
    "invalid at 0..4, did you mean enum value literal?"
  );
}

#[test]
fn invalid_debug() {
  let e = Invalid::int(SimpleSpan::new(1, 5));
  let s = format!("{e:?}");
  assert!(!s.is_empty());
}

// ── InvalidHexDigits ──────────────────────────────────────────────────────────

#[test]
fn invalid_hex_digits_from_positioned_char() {
  let mut digits: InvalidHexDigits<char, 2> =
    InvalidHexDigits::from_positioned_char(PositionedChar::with_position('G', 12));
  assert_eq!(digits.len(), 1);
  digits.push(PositionedChar::with_position('H', 13));
  assert_eq!(digits.len(), 2);
}

#[test]
fn invalid_hex_digits_from_char() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  assert_eq!(digits.len(), 1);
}

#[test]
fn invalid_hex_digits_from_array() {
  let digits: InvalidHexDigits<char, 4> = InvalidHexDigits::from_array([
    PositionedChar::with_position('G', 12),
    PositionedChar::with_position('H', 13),
    PositionedChar::with_position('I', 14),
    PositionedChar::with_position('J', 15),
  ]);
  assert_eq!(digits.len(), 4);
}

#[test]
fn invalid_hex_digits_is_full() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_array([
    PositionedChar::with_position('G', 12),
    PositionedChar::with_position('H', 13),
  ]);
  assert!(digits.is_full());
}

#[test]
fn invalid_hex_digits_bump() {
  let mut digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  digits.bump(&5usize);
}

#[test]
fn invalid_hex_digits_debug() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  let s = format!("{digits:?}");
  assert!(!s.is_empty());
}

// ── HexEscapeError ─────────────────────────────────────────────────────────────

#[test]
fn hex_escape_incomplete() {
  let e: HexEscapeError<char> = HexEscapeError::incomplete(SimpleSpan::new(5, 7));
  assert!(e.is_incomplete());
  assert!(!e.is_malformed());
}

#[test]
fn hex_escape_malformed() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  let e: HexEscapeError<char> = HexEscapeError::malformed(digits, SimpleSpan::new(5, 7));
  assert!(!e.is_incomplete());
  assert!(e.is_malformed());
}

#[test]
fn hex_escape_incomplete_hex_escape_display() {
  let e = IncompleteHexEscape::new(SimpleSpan::new(5, 7));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn hex_escape_malformed_hex_escape_display() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  let e = MalformedHexEscape::new(digits, SimpleSpan::new(5, 7));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn hex_escape_error_display_incomplete() {
  let e: HexEscapeError<char> = HexEscapeError::incomplete(SimpleSpan::new(5, 7));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn hex_escape_error_display_malformed() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  let e: HexEscapeError<char> = HexEscapeError::malformed(digits, SimpleSpan::new(5, 7));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn hex_escape_incomplete_span() {
  let e = IncompleteHexEscape::new(SimpleSpan::new(5, 7));
  assert_eq!(e.span(), SimpleSpan::new(5, 7));
}

#[test]
fn hex_escape_malformed_span() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  let e = MalformedHexEscape::new(digits.clone(), SimpleSpan::new(5, 7));
  assert_eq!(e.span(), SimpleSpan::new(5, 7));
  assert_eq!(e.digits_ref(), &digits);
}

#[test]
fn hex_escape_incomplete_bump() {
  let mut e = IncompleteHexEscape::new(SimpleSpan::new(5, 7));
  e.bump(&3);
  assert_eq!(e.span(), SimpleSpan::new(8, 10));
}

#[test]
fn hex_escape_malformed_bump() {
  let digits: InvalidHexDigits<char, 2> = InvalidHexDigits::from_char(10, 'G');
  let mut e = MalformedHexEscape::new(digits, SimpleSpan::new(5, 7));
  e.bump(&3);
  assert_eq!(e.span(), SimpleSpan::new(8, 10));
}

// ── UnknownLexeme ─────────────────────────────────────────────────────────────

#[test]
fn unknown_lexeme_from_char() {
  let e: UnknownLexeme<char, ()> = UnknownLexeme::from_char(10usize, 'x', ());
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unknown_lexeme_from_positioned_char() {
  let pc = PositionedChar::with_position('x', 10usize);
  let e: UnknownLexeme<char, ()> = UnknownLexeme::from_positioned_char(pc, ());
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unknown_lexeme_bump() {
  let mut e: UnknownLexeme<char, ()> = UnknownLexeme::from_char(5usize, 'x', ());
  e.bump(&3);
}

#[test]
fn unknown_lexeme_into_components() {
  let e: UnknownLexeme<char, ()> = UnknownLexeme::from_char(10usize, 'x', ());
  let (_lexeme, _knowledge) = e.into_components();
}

#[test]
fn unknown_lexeme_debug() {
  let e: UnknownLexeme<char, ()> = UnknownLexeme::from_char(10usize, 'x', ());
  let s = format!("{e:?}");
  assert!(!s.is_empty());
}

// ── UnexpectedLexeme ─────────────────────────────────────────────────────────

#[test]
fn unexpected_lexeme_from_char() {
  let e: UnexpectedLexeme<char, &str> = UnexpectedLexeme::from_char(10usize, 'x', "identifier");
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_lexeme_from_positioned_char() {
  let pc = PositionedChar::with_position('x', 10usize);
  let e: UnexpectedLexeme<char, &str> = UnexpectedLexeme::from_positioned_char(pc, "identifier");
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_lexeme_new_line() {
  let e: UnexpectedLineTerminator<char> = UnexpectedLineTerminator::new_line(10usize, '\n');
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_lexeme_carriage_return() {
  let e: UnexpectedLineTerminator<char> = UnexpectedLineTerminator::carriage_return(10usize, '\r');
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_lexeme_carriage_return_new_line() {
  let e: UnexpectedLineTerminator<char> =
    UnexpectedLineTerminator::carriage_return_new_line(SimpleSpan::new(10, 12));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_lexeme_bump() {
  let mut e: UnexpectedLexeme<char, &str> = UnexpectedLexeme::from_char(5usize, 'x', "id");
  e.bump(&3);
}

#[test]
fn unexpected_lexeme_into_components() {
  let e: UnexpectedLexeme<char, &str> = UnexpectedLexeme::from_char(10usize, 'x', "id");
  let (_lexeme, _hint) = e.into_components();
}

#[test]
fn unexpected_lexeme_debug() {
  let e: UnexpectedLexeme<char, &str> = UnexpectedLexeme::from_char(10usize, 'x', "id");
  let s = format!("{e:?}");
  assert!(!s.is_empty());
}

// ── UnexpectedPrefix ──────────────────────────────────────────────────────────

#[test]
fn unexpected_prefix_from_char() {
  // token span is 1..5, prefix char '0' at position 0 (prefix must end before token starts)
  let e: UnexpectedPrefix<char, ()> = UnexpectedPrefix::from_char(SimpleSpan::new(1, 5), 0, '0');
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_prefix_from_positioned_char() {
  let pc = PositionedChar::with_position('0', 0usize);
  let e: UnexpectedPrefix<char, ()> =
    UnexpectedPrefix::from_positioned_char(SimpleSpan::new(1, 5), pc);
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_prefix_bump() {
  let mut e: UnexpectedPrefix<char, ()> =
    UnexpectedPrefix::from_char(SimpleSpan::new(1, 5), 0, '0');
  e.bump(&3);
}

#[test]
fn unexpected_prefix_into_components() {
  let e: UnexpectedPrefix<char, ()> = UnexpectedPrefix::from_char(SimpleSpan::new(1, 5), 0, '0');
  let (_token_span, _prefix_lexeme) = e.into_components();
}

#[test]
fn unexpected_prefix_debug() {
  let e: UnexpectedPrefix<char, ()> = UnexpectedPrefix::from_char(SimpleSpan::new(1, 5), 0, '0');
  let s = format!("{e:?}");
  assert!(!s.is_empty());
}

// ── UnexpectedSuffix ──────────────────────────────────────────────────────────

#[test]
fn unexpected_suffix_from_char() {
  // token span is 0..5, suffix char 'x' at position 5 (suffix must start at or after token end)
  let e: UnexpectedSuffix<char, ()> = UnexpectedSuffix::from_char(SimpleSpan::new(0, 5), 5, 'x');
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_suffix_from_positioned_char() {
  let pc = PositionedChar::with_position('x', 5usize);
  let e: UnexpectedSuffix<char, ()> =
    UnexpectedSuffix::from_positioned_char(SimpleSpan::new(0, 5), pc);
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unexpected_suffix_bump() {
  let mut e: UnexpectedSuffix<char, ()> =
    UnexpectedSuffix::from_char(SimpleSpan::new(0, 5), 5, 'x');
  e.bump(&3);
}

#[test]
fn unexpected_suffix_into_components() {
  let e: UnexpectedSuffix<char, ()> = UnexpectedSuffix::from_char(SimpleSpan::new(0, 5), 5, 'x');
  let (_token_span, _suffix_lexeme) = e.into_components();
}

#[test]
fn unexpected_suffix_debug() {
  let e: UnexpectedSuffix<char, ()> = UnexpectedSuffix::from_char(SimpleSpan::new(0, 5), 5, 'x');
  let s = format!("{e:?}");
  assert!(!s.is_empty());
}

// ── UnicodeEscapeError ────────────────────────────────────────────────────────

#[test]
fn unicode_escape_malformed_fixed() {
  let digits = InvalidFixedUnicodeHexDigits::from_array([
    PositionedChar::with_position('G', 12),
    PositionedChar::with_position('H', 13),
    PositionedChar::with_position('I', 14),
    PositionedChar::with_position('J', 15),
  ]);
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::malformed_fixed_unicode_escape(digits, SimpleSpan::new(10, 16));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_incomplete_fixed() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::incomplete_fixed_unicode_escape(SimpleSpan::new(5, 9));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_unpaired_surrogate() {
  use tokora::utils::Lexeme;
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::unpaired_high_surrogate(Lexeme::Range(SimpleSpan::new(5, 11)));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_empty_variable() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::empty_variable_unicode_escape(SimpleSpan::new(5, 9));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_surrogate_variable() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::surrogate_variable_unicode_escape(SimpleSpan::new(10, 18), 0xD800);
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_overflow_variable() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::overflow_variable_unicode_escape(SimpleSpan::new(20, 30), 0x110000);
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_too_many_digits_variable() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::too_many_digits_in_variable_unicode_escape(SimpleSpan::new(5, 15), 7);
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_unclosed_variable() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::unclosed_variable_unicode_escape(SimpleSpan::new(0, 10));
  let s = e.to_string();
  assert!(!s.is_empty());
}

#[test]
fn unicode_escape_is_fixed_variable() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::incomplete_fixed_unicode_escape(SimpleSpan::new(5, 9));
  assert!(e.is_fixed());
  assert!(!e.is_variable());

  let e2: UnicodeEscapeError<char> =
    UnicodeEscapeError::empty_variable_unicode_escape(SimpleSpan::new(5, 9));
  assert!(!e2.is_fixed());
  assert!(e2.is_variable());
}

#[test]
fn unicode_escape_debug() {
  let e: UnicodeEscapeError<char> =
    UnicodeEscapeError::incomplete_fixed_unicode_escape(SimpleSpan::new(5, 9));
  let s = format!("{e:?}");
  assert!(!s.is_empty());
}

// ── PositionedChar ─────────────────────────────────────────────────────────────

#[test]
fn positioned_char_with_position() {
  let pc = PositionedChar::with_position('x', 10usize);
  assert_eq!(pc.char(), 'x');
  assert_eq!(pc.position(), 10);
}

#[test]
fn positioned_char_char_ref() {
  let pc = PositionedChar::with_position('x', 10usize);
  assert_eq!(pc.char_ref(), &'x');
}

#[test]
fn positioned_char_position_ref() {
  let pc = PositionedChar::with_position('x', 10usize);
  assert_eq!(pc.position_ref(), &10);
}

#[test]
fn positioned_char_span() {
  let pc = PositionedChar::with_position('a', 5usize);
  let span = pc.span();
  assert_eq!(span.start(), 5);
}

#[test]
fn positioned_char_set_position() {
  let mut pc = PositionedChar::with_position('x', 10usize);
  pc.set_position(20);
  assert_eq!(pc.position(), 20);
}

#[test]
fn positioned_char_map() {
  let pc = PositionedChar::with_position('x', 10usize);
  let pc2 = pc.map(|c| c.to_ascii_uppercase());
  assert_eq!(pc2.char(), 'X');
}

#[test]
fn positioned_char_into_components() {
  let pc = PositionedChar::with_position('x', 10usize);
  let (ch, pos) = pc.into_components();
  assert_eq!(ch, 'x');
  assert_eq!(pos, 10);
}

#[test]
fn positioned_char_debug() {
  let pc = PositionedChar::with_position('x', 10usize);
  let s = format!("{pc:?}");
  assert!(!s.is_empty());
}

// ── SimpleSpan ────────────────────────────────────────────────────────────────

#[test]
fn simple_span_new() {
  let s = SimpleSpan::new(5, 10);
  assert_eq!(s.start(), 5);
  assert_eq!(s.end(), 10);
}

#[test]
fn simple_span_len() {
  let s = SimpleSpan::new(5, 10);
  assert_eq!(s.len(), 5);
}

#[test]
fn simple_span_is_empty() {
  let s = SimpleSpan::new(5, 5);
  assert!(s.is_empty());
  let s2 = SimpleSpan::new(5, 10);
  assert!(!s2.is_empty());
}

#[test]
fn simple_span_display() {
  let s = SimpleSpan::new(5, 10);
  assert_eq!(s.to_string(), "5..10");
}

#[test]
fn simple_span_debug() {
  let s = SimpleSpan::new(5, 10);
  let dbg = format!("{s:?}");
  assert!(!dbg.is_empty());
}

#[test]
fn simple_span_ordering() {
  let a = SimpleSpan::new(1, 5);
  let b = SimpleSpan::new(5, 10);
  assert!(a < b);
  assert_eq!(a, a);
}

// ── Missing ───────────────────────────────────────────────────────────────────

use tokora::syntax::{Language, Syntax};
use tokora::utils::{GenericArrayDeque, typenum::U0};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TestLang;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TestSyntaxKind;

impl core::fmt::Display for TestSyntaxKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "expr")
  }
}

impl Language for TestLang {
  type SyntaxKind = TestSyntaxKind;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Unit;

impl core::fmt::Display for Unit {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "unit")
  }
}

struct ExprSyntax;

impl Syntax for ExprSyntax {
  type Lang = TestLang;
  const KIND: TestSyntaxKind = TestSyntaxKind;
  type Component = Unit;
  type COMPONENTS = U0;
  type REQUIRED = U0;
  fn possible_components() -> &'static GenericArrayDeque<Unit, tokora::utils::typenum::UTerm> {
    const C: &GenericArrayDeque<Unit, U0> = &GenericArrayDeque::from_array([]);
    C
  }
  fn required_components() -> &'static GenericArrayDeque<Unit, tokora::utils::typenum::UTerm> {
    const C: &GenericArrayDeque<Unit, U0> = &GenericArrayDeque::from_array([]);
    C
  }
}

#[test]
fn missing_new_before_only() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> = Missing::new(SimpleSpan::new(5, 10));
  assert_eq!(m.before(), SimpleSpan::new(5, 10));
  assert_eq!(m.after(), None);
  assert_eq!(m.span(), SimpleSpan::new(5, 10));
}

#[test]
fn missing_between() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> =
    Missing::between(SimpleSpan::new(5, 10), SimpleSpan::new(15, 20));
  assert_eq!(m.before(), SimpleSpan::new(5, 10));
  assert_eq!(m.after(), Some(SimpleSpan::new(15, 20)));
  assert_eq!(m.span(), SimpleSpan::new(10, 15));
}

#[test]
fn missing_between_adjacent() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> =
    Missing::between(SimpleSpan::new(5, 10), SimpleSpan::new(10, 15));
  assert_eq!(m.span(), SimpleSpan::new(10, 10));
}

#[test]
fn missing_between_overlapping() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> =
    Missing::between(SimpleSpan::new(5, 12), SimpleSpan::new(10, 15));
  assert_eq!(m.span(), SimpleSpan::new(10, 10));
}

#[test]
fn missing_with_after() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> =
    Missing::new(SimpleSpan::new(5, 10)).with_after(SimpleSpan::new(15, 20));
  assert_eq!(m.after(), Some(SimpleSpan::new(15, 20)));
}

#[test]
fn missing_set_after() {
  let mut m: Missing<ExprSyntax, SimpleSpan, TestLang> = Missing::new(SimpleSpan::new(5, 10));
  m.set_after(SimpleSpan::new(20, 25));
  assert_eq!(m.after(), Some(SimpleSpan::new(20, 25)));
}

#[test]
fn missing_span_ref() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> = Missing::new(SimpleSpan::new(5, 10));
  assert_eq!(*m.span_ref(), SimpleSpan::new(5, 10));
}

#[test]
fn missing_kind() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> = Missing::new(SimpleSpan::new(5, 10));
  assert_eq!(m.kind(), TestSyntaxKind);
}

#[test]
fn missing_bump() {
  let mut m: Missing<ExprSyntax, SimpleSpan, TestLang> =
    Missing::between(SimpleSpan::new(5, 10), SimpleSpan::new(15, 20));
  m.bump(&10usize);
  assert_eq!(m.before(), SimpleSpan::new(15, 20));
  assert_eq!(m.after(), Some(SimpleSpan::new(25, 30)));
}

#[test]
fn missing_debug() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> = Missing::new(SimpleSpan::new(5, 10));
  let s = format!("{m:?}");
  assert!(s.contains("Missing"));
}

#[test]
fn missing_display_with_after() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> =
    Missing::between(SimpleSpan::new(5, 10), SimpleSpan::new(15, 20));
  let s = format!("{m}");
  assert!(s.contains("missing"));
}

#[test]
fn missing_display_without_after() {
  let m: Missing<ExprSyntax, SimpleSpan, TestLang> = Missing::new(SimpleSpan::new(5, 10));
  let s = format!("{m}");
  assert!(s.contains("missing") && s.contains("after"));
}

// ── Undelimited ───────────────────────────────────────────────────────────────

use tokora::error::{Undelimited, Unopened};
use tokora::punct::{Angle, Brace, Bracket, Paren};

#[test]
fn undelimited_new() {
  let e: Undelimited<char> = Undelimited::new(SimpleSpan::new(10, 15), "\"".into());
  assert_eq!(e.span(), SimpleSpan::new(10, 15));
  assert_eq!(e.name_ref(), "\"");
}

#[test]
fn undelimited_paren() {
  let e: Undelimited<Paren> = Undelimited::paren(SimpleSpan::new(3, 7));
  assert_eq!(e.span(), SimpleSpan::new(3, 7));
  assert_eq!(e.name_ref(), "()");
}

#[test]
fn undelimited_paren_of() {
  let e: Undelimited<Paren, SimpleSpan, ()> = Undelimited::paren_of(SimpleSpan::new(3, 7));
  assert_eq!(e.name_ref(), "()");
}

#[test]
fn undelimited_bracket() {
  let e: Undelimited<Bracket> = Undelimited::bracket(SimpleSpan::new(8, 15));
  assert_eq!(e.span(), SimpleSpan::new(8, 15));
  assert_eq!(e.name_ref(), "[]");
}

#[test]
fn undelimited_bracket_of() {
  let e: Undelimited<Bracket, SimpleSpan, ()> = Undelimited::bracket_of(SimpleSpan::new(8, 15));
  assert_eq!(e.name_ref(), "[]");
}

#[test]
fn undelimited_brace() {
  let e: Undelimited<Brace> = Undelimited::brace(SimpleSpan::new(12, 20));
  assert_eq!(e.span(), SimpleSpan::new(12, 20));
  assert_eq!(e.name_ref(), "{}");
}

#[test]
fn undelimited_brace_of() {
  let e: Undelimited<Brace, SimpleSpan, ()> = Undelimited::brace_of(SimpleSpan::new(12, 20));
  assert_eq!(e.name_ref(), "{}");
}

#[test]
fn undelimited_angle() {
  let e: Undelimited<Angle> = Undelimited::angle(SimpleSpan::new(0, 5));
  assert_eq!(e.span(), SimpleSpan::new(0, 5));
  assert_eq!(e.name_ref(), "<>");
}

#[test]
fn undelimited_angle_of() {
  let e: Undelimited<Angle, SimpleSpan, ()> = Undelimited::angle_of(SimpleSpan::new(0, 5));
  assert_eq!(e.name_ref(), "<>");
}

#[test]
fn undelimited_span_ref() {
  let e: Undelimited<char> = Undelimited::new(SimpleSpan::new(5, 10), "x".into());
  assert_eq!(*e.span_ref(), SimpleSpan::new(5, 10));
}

#[test]
fn undelimited_span_mut() {
  let mut e: Undelimited<char> = Undelimited::new(SimpleSpan::new(5, 10), "x".into());
  *e.span_mut() = SimpleSpan::new(1, 2);
  assert_eq!(e.span(), SimpleSpan::new(1, 2));
}

#[test]
fn undelimited_bump() {
  let mut e: Undelimited<char> = Undelimited::new(SimpleSpan::new(5, 10), "(".into());
  e.bump(&100);
  assert_eq!(e.span(), SimpleSpan::new(105, 110));
}

#[test]
fn undelimited_into_components() {
  let e: Undelimited<char> = Undelimited::new(SimpleSpan::new(10, 15), "\"".into());
  let (span, delim) = e.into_components();
  assert_eq!(span, SimpleSpan::new(10, 15));
  assert_eq!(delim, CowStr::from_static("\""));
}

#[test]
fn undelimited_display() {
  let e: Undelimited<char> = Undelimited::new(SimpleSpan::new(10, 15), "\"".into());
  assert_eq!(e.to_string(), "undelimited content, expected '\"'");
}

#[test]
fn undelimited_debug() {
  let e: Undelimited<char> = Undelimited::new(SimpleSpan::new(10, 15), "\"".into());
  let s = format!("{e:?}");
  assert!(s.contains("Undelimited"));
}

#[test]
fn undelimited_into_unit() {
  let e: Undelimited<char> = Undelimited::new(SimpleSpan::new(10, 15), "\"".into());
  let _unit: () = e.into();
}

// ── Unopened ──────────────────────────────────────────────────────────────────

#[test]
fn unopened_new() {
  let e: Unopened<char> = Unopened::new(SimpleSpan::new(10, 11), ")".into());
  assert_eq!(e.span(), SimpleSpan::new(10, 11));
  assert_eq!(e.name_ref(), ")");
}

#[test]
fn unopened_paren() {
  let e: Unopened<Paren> = Unopened::paren(SimpleSpan::new(3, 4));
  assert_eq!(e.span(), SimpleSpan::new(3, 4));
  assert_eq!(e.name_ref(), "()");
}

#[test]
fn unopened_paren_of() {
  let e: Unopened<Paren, SimpleSpan, ()> = Unopened::paren_of(SimpleSpan::new(3, 4));
  assert_eq!(e.name_ref(), "()");
}

#[test]
fn unopened_bracket() {
  let e: Unopened<Bracket> = Unopened::bracket(SimpleSpan::new(8, 9));
  assert_eq!(e.span(), SimpleSpan::new(8, 9));
  assert_eq!(e.name_ref(), "[]");
}

#[test]
fn unopened_bracket_of() {
  let e: Unopened<Bracket, SimpleSpan, ()> = Unopened::bracket_of(SimpleSpan::new(8, 9));
  assert_eq!(e.name_ref(), "[]");
}

#[test]
fn unopened_brace() {
  let e: Unopened<Brace> = Unopened::brace(SimpleSpan::new(12, 13));
  assert_eq!(e.span(), SimpleSpan::new(12, 13));
  assert_eq!(e.name_ref(), "{}");
}

#[test]
fn unopened_brace_of() {
  let e: Unopened<Brace, SimpleSpan, ()> = Unopened::brace_of(SimpleSpan::new(12, 13));
  assert_eq!(e.name_ref(), "{}");
}

#[test]
fn unopened_angle() {
  let e: Unopened<Angle> = Unopened::angle(SimpleSpan::new(20, 21));
  assert_eq!(e.span(), SimpleSpan::new(20, 21));
  assert_eq!(e.name_ref(), "<>");
}

#[test]
fn unopened_angle_of() {
  let e: Unopened<Angle, SimpleSpan, ()> = Unopened::angle_of(SimpleSpan::new(20, 21));
  assert_eq!(e.name_ref(), "<>");
}

#[test]
fn unopened_span_ref() {
  let e: Unopened<char> = Unopened::new(SimpleSpan::new(5, 6), ")".into());
  assert_eq!(*e.span_ref(), SimpleSpan::new(5, 6));
}

#[test]
fn unopened_span_mut() {
  let mut e: Unopened<char> = Unopened::new(SimpleSpan::new(5, 6), ")".into());
  *e.span_mut() = SimpleSpan::new(1, 2);
  assert_eq!(e.span(), SimpleSpan::new(1, 2));
}

#[test]
fn unopened_bump() {
  let mut e: Unopened<char> = Unopened::new(SimpleSpan::new(5, 6), ")".into());
  e.bump(&100);
  assert_eq!(e.span(), SimpleSpan::new(105, 106));
}

#[test]
fn unopened_into_components() {
  let e: Unopened<char> = Unopened::new(SimpleSpan::new(10, 11), "\"".into());
  let (span, delim) = e.into_components();
  assert_eq!(span, SimpleSpan::new(10, 11));
  assert_eq!(delim, CowStr::from_static("\""));
}

#[test]
fn unopened_display() {
  let e: Unopened<char> = Unopened::new(SimpleSpan::new(10, 11), ")".into());
  assert_eq!(e.to_string(), "unopened delimiter ')'");
}

#[test]
fn unopened_debug() {
  let e: Unopened<char> = Unopened::new(SimpleSpan::new(10, 11), ")".into());
  let s = format!("{e:?}");
  assert!(s.contains("Unopened"));
}

#[test]
fn unopened_into_unit() {
  let e: Unopened<char> = Unopened::new(SimpleSpan::new(10, 11), ")".into());
  let _unit: () = e.into();
}

// ── UnexpectedToken ────────────────────────────────────────────────────────────

#[test]
fn unexpected_token_new() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::new(SimpleSpan::new(5, 10));
  assert_eq!(e.span(), SimpleSpan::new(5, 10));
  assert!(e.found().is_none());
  assert!(e.expected().is_none());
}

#[test]
fn unexpected_token_of() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::of(SimpleSpan::new(5, 10));
  assert_eq!(e.span(), SimpleSpan::new(5, 10));
}

#[test]
fn unexpected_token_expected_one() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "}");
  assert!(e.found().is_none());
  assert!(matches!(e.expected(), Some(Expected::One(v)) if *v == "}"));
}

#[test]
fn unexpected_token_expected_one_with_found() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(5, 10), ":", ";");
  assert_eq!(e.found(), Some(&":"));
  assert!(matches!(e.expected(), Some(Expected::One(v)) if *v == ";"));
}

#[test]
fn unexpected_token_expected_one_of() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one_of(SimpleSpan::new(5, 10), &["+", "-"]);
  assert!(e.found().is_none());
  if let Some(Expected::OneOf(vals)) = e.expected() {
    assert_eq!(vals.as_slice(), &["+", "-"]);
  } else {
    panic!("expected OneOf");
  }
}

#[test]
fn unexpected_token_expected_one_of_with_found() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one_of_with_found(SimpleSpan::new(5, 10), "x", &["+", "-"]);
  assert_eq!(e.found(), Some(&"x"));
  if let Some(Expected::OneOf(vals)) = e.expected() {
    assert_eq!(vals.as_slice(), &["+", "-"]);
  } else {
    panic!("expected OneOf");
  }
}

#[test]
fn unexpected_token_with_expected() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::with_expected(SimpleSpan::new(5, 10), Expected::one("}"));
  assert!(e.found().is_none());
}

#[test]
fn unexpected_token_with_expected_of() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::with_expected_of(SimpleSpan::new(5, 10), Expected::one("}"));
  assert!(e.found().is_none());
}

#[test]
fn unexpected_token_maybe_expected() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::maybe_expected(SimpleSpan::new(5, 10), Some(Expected::one("}")));
  assert!(e.expected().is_some());
  let e2: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::maybe_expected(SimpleSpan::new(5, 10), None);
  assert!(e2.expected().is_none());
}

#[test]
fn unexpected_token_maybe_expected_of() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::maybe_expected_of(SimpleSpan::new(5, 10), None);
  assert!(e.expected().is_none());
}

#[test]
fn unexpected_token_with_found() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "if").with_found("else");
  assert_eq!(e.found(), Some(&"else"));
}

#[test]
fn unexpected_token_with_found_const() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "if").with_found_const("else");
  assert_eq!(e.found(), Some(&"else"));
}

#[test]
fn unexpected_token_maybe_found() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "if").maybe_found(Some("else"));
  assert_eq!(e.found(), Some(&"else"));
  let e2: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "if").maybe_found(None);
  assert!(e2.found().is_none());
}

#[test]
fn unexpected_token_maybe_found_const() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "if").maybe_found_const(Some("else"));
  assert_eq!(e.found(), Some(&"else"));
}

#[test]
fn unexpected_token_span_ref() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "if");
  assert_eq!(*e.span_ref(), SimpleSpan::new(5, 10));
}

#[test]
fn unexpected_token_span_mut() {
  let mut e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "if");
  *e.span_mut() = SimpleSpan::new(1, 2);
  assert_eq!(e.span(), SimpleSpan::new(1, 2));
}

#[test]
fn unexpected_token_bump() {
  let mut e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(10, 15), "}", "{");
  e.bump(&5);
  assert_eq!(e.span(), SimpleSpan::new(15, 20));
}

#[test]
fn unexpected_token_into_components() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(5, 6), "}", "{");
  let (span, found, expected) = e.into_components();
  assert_eq!(span, SimpleSpan::new(5, 6));
  assert_eq!(found, Some("}"));
  assert_eq!(expected, Some(Expected::one("{")));
}

#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn unexpected_token_map_expected() {
  use std::string::ToString;

  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(0, 5), "identifier", "number");
  let _mapped = e.map_expected(|ex| Expected::one(ex.unwrap_one().to_string()));
}

#[test]
fn unexpected_token_display_fmt_with_found_and_expected() {
  struct Show<'a>(UnexpectedToken<'a, &'a str, &'a str, SimpleSpan>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(5, 10), "}", "{");
  let s = format!("{}", Show(e));
  assert!(s.contains("unexpected token") && s.contains("}"));
}

#[test]
fn unexpected_token_display_fmt_with_found_no_expected() {
  struct Show<'a>(UnexpectedToken<'a, &'a str, &'a str, SimpleSpan>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::new(SimpleSpan::new(5, 10)).with_found("x");
  let s = format!("{}", Show(e));
  assert!(s.contains("unexpected token"));
}

#[test]
fn unexpected_token_display_fmt_no_found_with_expected() {
  struct Show<'a>(UnexpectedToken<'a, &'a str, &'a str, SimpleSpan>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "}");
  let s = format!("{}", Show(e));
  assert!(s.contains("unexpected token") && s.contains("}"));
}

#[test]
fn unexpected_token_display_fmt_neither() {
  struct Show<'a>(UnexpectedToken<'a, &'a str, &'a str, SimpleSpan>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> = UnexpectedToken::new(SimpleSpan::new(5, 10));
  let s = format!("{}", Show(e));
  assert_eq!(s, "unexpected token");
}

#[test]
fn unexpected_token_debug_fmt() {
  struct Show<'a>(UnexpectedToken<'a, &'a str, &'a str, SimpleSpan>);

  impl core::fmt::Debug for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.debug_fmt(f)
    }
  }

  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "}");
  let s = format!("{:?}", Show(e));
  assert!(s.contains("UnexpectedToken"));
}

#[test]
fn unexpected_token_into_unit() {
  let e: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::expected_one(SimpleSpan::new(5, 10), "}");
  let _unit: () = e.into();
}

// ── MissingToken ──────────────────────────────────────────────────────────────

#[test]
fn missing_token_new() {
  let e: MissingToken<'_, &str, SimpleSpan> = MissingToken::new(SimpleSpan::new(5, 6));
  assert_eq!(e.offset(), SimpleSpan::new(5, 6));
  assert!(e.expected().is_none());
  assert!(e.message().is_none());
}

#[test]
fn missing_token_of() {
  let e: MissingToken<'_, &str, SimpleSpan> = MissingToken::of(SimpleSpan::new(5, 6));
  assert_eq!(e.offset(), SimpleSpan::new(5, 6));
}

#[test]
fn missing_token_with_message() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::new(SimpleSpan::new(5, 6)).with_message(CowStr::from_static("semicolon needed"));
  assert_eq!(e.message().map(|m| m.as_str()), Some("semicolon needed"));
}

#[test]
fn missing_token_with_message_of() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::new(SimpleSpan::new(5, 6)).with_message(CowStr::from_static("needed"));
  assert!(e.message().is_some());
}

#[test]
fn missing_token_expected_one() {
  let e: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}");
  assert!(matches!(e.expected(), Some(Expected::One(v)) if *v == "}"));
}

#[test]
fn missing_token_expected_one_with_found() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::expected_one_with_found(SimpleSpan::new(5, 6), "}");
  assert!(matches!(e.expected(), Some(Expected::One(v)) if *v == "}"));
}

#[test]
fn missing_token_expected_one_of() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::expected_one_of(SimpleSpan::new(5, 6), &["+", "-"]);
  if let Some(Expected::OneOf(vals)) = e.expected() {
    assert_eq!(vals.as_slice(), &["+", "-"]);
  } else {
    panic!("expected OneOf");
  }
}

#[test]
fn missing_token_expected_one_of_with_found() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::expected_one_of_with_found(SimpleSpan::new(5, 6), &["+", "-"]);
  if let Some(Expected::OneOf(vals)) = e.expected() {
    assert_eq!(vals.as_slice(), &["+", "-"]);
  } else {
    panic!("expected OneOf");
  }
}

#[test]
fn missing_token_with_expected() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::new(SimpleSpan::new(5, 6)).with_expected(Expected::one("}"));
  assert!(e.expected().is_some());
}

#[test]
fn missing_token_offset_ref() {
  let e: MissingToken<'_, &str, SimpleSpan> = MissingToken::new(SimpleSpan::new(5, 6));
  assert_eq!(*e.offset_ref(), SimpleSpan::new(5, 6));
}

#[test]
fn missing_token_offset_mut() {
  let mut e: MissingToken<'_, &str> = MissingToken::expected_one(10, "}");
  *e.offset_mut() = 12;
  assert_eq!(e.offset(), 12);
}

#[test]
fn missing_token_message_mut() {
  let mut e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::of(SimpleSpan::new(5, 6)).with_message(CowStr::from_static("msg"));
  if let Some(m) = e.message_mut() {
    assert_eq!(m.as_str(), "msg");
  } else {
    panic!("expected message");
  }
}

#[test]
fn missing_token_bump() {
  let mut e: MissingToken<'_, &str> = MissingToken::expected_one(10, "}");
  e.bump(&5);
  assert_eq!(e.offset(), 15);
}

#[test]
fn missing_token_into_components() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::expected_one(SimpleSpan::new(5, 6), "}");
  let (offset, expected, message, name) = e.into_components();
  assert_eq!(offset, SimpleSpan::new(5, 6));
  assert_eq!(expected, Some(Expected::one("}")));
  assert_eq!(message, None);
  assert_eq!(name, None);
}

// The destructuring seam has to carry the name for the same reason the channel exists: a
// downstream `From<MissingToken>` that takes the error apart is exactly the consumer the
// separator conversions stamp the name for, and a tuple that omitted it would put the loss
// back one step further along.
#[test]
fn missing_token_into_components_yields_the_stamped_name() {
  let e: MissingToken<'_, &str, SimpleSpan> =
    MissingToken::expected_one(SimpleSpan::new(5, 6), "}").with_name(CowStr::from_static("comma"));
  let (offset, expected, message, name) = e.into_components();
  assert_eq!(offset, SimpleSpan::new(5, 6));
  assert_eq!(expected, Some(Expected::one("}")));
  assert_eq!(message, None);
  assert_eq!(name.as_ref().map(CowStr::as_str), Some("comma"));
}

// The two `Option<CowStr>` slots are distinct channels, so destructuring must not conflate
// them: a name and a message set together come back in their own positions.
#[test]
fn missing_token_into_components_keeps_name_and_message_apart() {
  let e: MissingToken<'_, &str, SimpleSpan> = MissingToken::of(SimpleSpan::new(5, 6))
    .with_name(CowStr::from_static("comma"))
    .with_message(CowStr::from_static("needed"));
  let (_offset, _expected, message, name) = e.into_components();
  assert_eq!(message.as_ref().map(CowStr::as_str), Some("needed"));
  assert_eq!(name.as_ref().map(CowStr::as_str), Some("comma"));
}

#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_map_expected() {
  use std::string::ToString;

  let e: MissingToken<'_, &str> = MissingToken::expected_one(0, "identifier");
  let _mapped = e.map_expected(|ex| Expected::one(ex.unwrap_one().to_string()));
}

// `map_expected` rebuilds the struct field by field, so it is the one transform that can drop
// a channel by omission rather than by decision. Every channel it is not there to change must
// come out the other side.
#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_map_expected_preserves_the_other_channels() {
  use std::string::ToString;

  let e: MissingToken<'_, &str> = MissingToken::expected_one(7, "identifier")
    .with_message(CowStr::from_static("needed"))
    .with_name(CowStr::from_static("comma"));
  let mapped = e.map_expected(|ex| Expected::one(ex.unwrap_one().to_string()));

  assert_eq!(mapped.offset(), 7);
  assert_eq!(mapped.message().map(CowStr::as_str), Some("needed"));
  assert_eq!(mapped.name().map(CowStr::as_str), Some("comma"));
}

#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_display_fmt_with_expected() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}");
  let s = format!("{}", Show(e));
  assert!(s.contains("missing token") && s.contains("}"));
}

#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_display_fmt_with_expected_and_message() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: MissingToken<'_, &str, usize> = MissingToken::new(5)
    .with_expected(Expected::one("}"))
    .with_message(CowStr::from_static("needed"));
  let s = format!("{}", Show(e));
  assert!(s.contains("missing token") && s.contains("needed"));
}

#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_display_fmt_no_expected_with_message() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: MissingToken<'_, &str, usize> =
    MissingToken::of(5).with_message(CowStr::from_static("needed"));
  let s = format!("{}", Show(e));
  assert!(s.contains("missing token") && s.contains("needed"));
}

#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_display_fmt_neither() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let e: MissingToken<'_, &str, usize> = MissingToken::new(5);
  let s = format!("{}", Show(e));
  assert!(s.contains("missing token"));
}

// The stamped name is the whole point of the name channel: a separated-sequence driver knows
// what its separator is called, and the default rendering is the path on which that reaches a
// human. A renderer that ignored it would leave every such diagnostic saying only that *a*
// token was missing.
#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_display_fmt_names_the_stamped_token() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let bare: MissingToken<'_, &str, usize> =
    MissingToken::of(5).with_name(CowStr::from_static("comma"));
  assert_eq!(format!("{}", Show(bare)), "missing token 'comma' at 5");

  // `Expected`'s own `Display` opens with the word "expected" in every variant, so the
  // composing site supplies only the separator. These four pins used to record the doubled
  // "expected expected" as this renderer's long-standing output; they are the flip.
  let expected: MissingToken<'_, &str, usize> =
    MissingToken::expected_one(5, "}").with_name(CowStr::from_static("comma"));
  assert_eq!(
    format!("{}", Show(expected)),
    "missing token 'comma' at 5, expected '}'"
  );

  let both: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}")
    .with_name(CowStr::from_static("comma"))
    .with_message(CowStr::from_static("needed"));
  assert_eq!(
    format!("{}", Show(both)),
    "missing token 'comma' at 5, expected '}', message: needed"
  );
}

// The name is additive: an error that never carried one must render exactly as it always did,
// so adding the channel cannot silently reword every existing diagnostic.
#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_display_fmt_unnamed_is_unchanged() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let bare: MissingToken<'_, &str, usize> = MissingToken::of(5);
  assert_eq!(format!("{}", Show(bare)), "missing token at 5");

  let expected: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}");
  assert_eq!(
    format!("{}", Show(expected)),
    "missing token at 5, expected '}'"
  );

  let message: MissingToken<'_, &str, usize> =
    MissingToken::of(5).with_message(CowStr::from_static("needed"));
  assert_eq!(
    format!("{}", Show(message)),
    "missing token at 5, message: needed"
  );

  let both: MissingToken<'_, &str, usize> =
    MissingToken::expected_one(5, "}").with_message(CowStr::from_static("needed"));
  assert_eq!(
    format!("{}", Show(both)),
    "missing token at 5, expected '}', message: needed"
  );
}

#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_debug_fmt() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Debug for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.debug_fmt(f)
    }
  }

  let e: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}");
  let s = format!("{:?}", Show(e));
  assert!(s.contains("MissingToken"));
}

// The debug rendering is the other reader of the name channel; it names the field so a stamped
// name is visible without going through the accessor.
#[test]
#[cfg(any(feature = "std", feature = "alloc"))]
fn missing_token_debug_fmt_shows_the_stamped_name() {
  struct Show<'a>(MissingToken<'a, &'a str, usize>);

  impl core::fmt::Debug for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.debug_fmt(f)
    }
  }

  let e: MissingToken<'_, &str, usize> =
    MissingToken::expected_one(5, "}").with_name(CowStr::from_static("comma"));
  let s = format!("{:?}", Show(e));
  assert!(s.contains("name"), "{s}");
  assert!(s.contains("comma"), "{s}");
}

#[test]
fn missing_token_into_unit() {
  let e: MissingToken<'_, &str, usize> = MissingToken::new(5);
  let _unit: () = e.into();
}

// ── One composition rule, three carriers ─────────────────────────────────────
//
// `Expected`'s `Display` opens with the word "expected" in EVERY variant, so any site that
// composes one must not write the word itself. Two carriers did, and only one of them was
// pinned — which is exactly why the unpinned one's stutter outlived the repair of the pinned
// one. The third carrier is new here and is born under the rule.

/// `UnexpectedToken`'s renders were never pinned at all. Before this commit these read
/// `unexpected token, expected expected '}'` and nothing in the suite said so.
#[test]
fn unexpected_token_render_says_expected_once() {
  struct Show<'a>(UnexpectedToken<'a, &'a str, &'a str, SimpleSpan>);
  impl core::fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      self.0.display_fmt(f)
    }
  }

  let no_found = UnexpectedToken::expected_one(SimpleSpan::new(5usize, 10usize), "}");
  assert_eq!(
    format!("{}", Show(no_found)),
    "unexpected token, expected '}'"
  );

  let with_found =
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(5usize, 10usize), ":", ";");
  assert_eq!(
    format!("{}", Show(with_found)),
    "unexpected token ':', expected ';'"
  );

  let one_of = UnexpectedToken::expected_one_of(SimpleSpan::new(5usize, 10usize), &["+", "-"]);
  assert_eq!(
    format!("{}", Show(one_of)),
    "unexpected token, expected one of: '+', '-'"
  );

  // The no-`Expected` arms never composed and must not move.
  let bare: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::new(SimpleSpan::new(5usize, 10usize));
  assert_eq!(format!("{}", Show(bare)), "unexpected token");
}

/// `SeparatedError` had no `Display` at all: its name channel was readable only by
/// destructuring through `into_components`, so a diagnostic that knew *which* separator was
/// involved could not say so. Byte-exact, and born with a single "expected".
#[test]
fn separated_error_renders_its_name_channel() {
  let inner = UnexpectedToken::expected_one_with_found(SimpleSpan::new(5usize, 10usize), ":", ";");
  let named: SeparatedError<'_, &str, &str, SimpleSpan> =
    SeparatedError::leading(inner.clone()).with_name(CowStr::from_static("comma"));
  assert_eq!(
    format!("{named}"),
    "separator 'comma' at the leading position: unexpected token ':', expected ';'"
  );

  let unnamed: SeparatedError<'_, &str, &str, SimpleSpan> = SeparatedError::trailing(inner);
  assert_eq!(
    format!("{unnamed}"),
    "separator at the trailing position: unexpected token ':', expected ';'"
  );

  let element: SeparatedError<'_, &str, &str, SimpleSpan> = SeparatedError::element(
    UnexpectedToken::expected_one_of(SimpleSpan::new(0usize, 1usize), &["+", "-"]),
  );
  assert_eq!(
    format!("{element}"),
    "separator at the element position: unexpected token, expected one of: '+', '-'"
  );
}

/// The two readers of the name channel must agree. A `Display` that rendered a name
/// `into_components` did not hand back — or vice versa — would be two answers to one question.
#[test]
fn separated_error_display_and_into_components_agree_on_the_name() {
  let inner = UnexpectedToken::expected_one_with_found(SimpleSpan::new(5usize, 10usize), ":", ";");
  let named: SeparatedError<'_, &str, &str, SimpleSpan> =
    SeparatedError::leading(inner).with_name(CowStr::from_static("comma"));

  let rendered = format!("{named}");
  let (position, name, _inner) = named.into_components();

  assert_eq!(position, SeparatorPosition::Leading);
  let name = name.expect("the stamped name survives destructuring");
  assert!(
    rendered.contains(&format!("'{name}'")),
    "Display must quote the same name `into_components` returns; rendered {rendered:?}, \
     name {name:?}"
  );
}
