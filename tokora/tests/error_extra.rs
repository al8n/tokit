#![cfg(all(feature = "std", feature = "logos"))]
#![allow(warnings)]

mod common;

use common::TestLexer;
use tokora::SimpleSpan;
use tokora::emitter::FromUnclosed;
use tokora::error::*;
use tokora::utils::{CowStr, Expected, Lexeme, PositionedChar, knowledge::*};

// ── UnexpectedKeyword ────────────────────────────────────────────────────────

#[test]
fn unexpected_keyword_new() {
  let err = UnexpectedKeyword::new(SimpleSpan::new(5, 8), "let", Expected::one("const"));
  assert_eq!(err.found(), &"let");
  assert_eq!(err.span(), SimpleSpan::new(5, 8));
  assert_eq!(err.expected(), Expected::one("const"));
  assert_eq!(
    format!("{}", err),
    "unexpected 'let', expected 'const' keyword"
  );
}

#[test]
fn unexpected_keyword_expected_one() {
  let err = UnexpectedKeyword::expected_one(SimpleSpan::new(0, 3), "var", "let");
  assert_eq!(err.found(), &"var");
  assert_eq!(err.span(), SimpleSpan::new(0, 3));
  assert_eq!(
    format!("{}", err),
    "unexpected 'var', expected 'let' keyword"
  );
}

#[test]
fn unexpected_keyword_expected_one_of() {
  let err = UnexpectedKeyword::expected_one_of(
    SimpleSpan::new(0, 5),
    "class",
    &["struct", "enum", "trait"],
  );
  assert_eq!(err.found(), &"class");
  let display = format!("{}", err);
  assert!(display.contains("unexpected 'class'"));
  assert!(display.contains("'struct'"));
  assert!(display.contains("'enum'"));
  assert!(display.contains("'trait'"));
  assert!(display.contains("keyword"));
}

#[test]
fn unexpected_keyword_span_ref() {
  let err = UnexpectedKeyword::expected_one(SimpleSpan::new(20, 26), "import", "use");
  assert_eq!(err.span_ref(), &SimpleSpan::new(20, 26));
}

#[test]
fn unexpected_keyword_bump() {
  let mut err = UnexpectedKeyword::expected_one(SimpleSpan::new(10, 13), "var", "let");
  err.bump(&5);
  assert_eq!(err.span(), SimpleSpan::new(15, 18));
}

#[test]
fn unexpected_keyword_error_trait() {
  let err = UnexpectedKeyword::expected_one(SimpleSpan::new(0, 3), "var", "let");
  let e: &dyn std::error::Error = &err;
  assert!(!e.to_string().is_empty());
}

#[test]
fn unexpected_keyword_debug() {
  let err = UnexpectedKeyword::expected_one(SimpleSpan::new(0, 3), "var", "let");
  let debug = format!("{:?}", err);
  assert!(!debug.is_empty());
}

#[test]
fn unexpected_keyword_clone_eq() {
  let err = UnexpectedKeyword::expected_one(SimpleSpan::new(0, 3), "var", "let");
  let cloned = err.clone();
  assert_eq!(err, cloned);
}

// ── UnexpectedIdentifier ─────────────────────────────────────────────────────

#[test]
fn unexpected_identifier_new() {
  let err = UnexpectedIdentifier::new(SimpleSpan::new(5, 8), "foo", Expected::one("bar"));
  assert_eq!(err.found(), &"foo");
  assert_eq!(err.span(), SimpleSpan::new(5, 8));
  assert_eq!(err.expected(), Expected::one("bar"));
  assert_eq!(
    format!("{}", err),
    "unexpected 'foo', expected 'bar' identifier"
  );
}

#[test]
fn unexpected_identifier_expected_one() {
  let err = UnexpectedIdentifier::expected_one(SimpleSpan::new(0, 4), "sync", "async");
  assert_eq!(err.found(), &"sync");
  assert_eq!(
    format!("{}", err),
    "unexpected 'sync', expected 'async' identifier"
  );
}

#[test]
fn unexpected_identifier_expected_one_of() {
  let err = UnexpectedIdentifier::expected_one_of(
    SimpleSpan::new(0, 5),
    "class",
    &["struct", "enum", "trait"],
  );
  let display = format!("{}", err);
  assert!(display.contains("unexpected 'class'"));
  assert!(display.contains("'struct'"));
  assert!(display.contains("identifier"));
}

#[test]
fn unexpected_identifier_span_ref_not_available_but_found_and_expected() {
  // Test found() and expected() accessors
  let err = UnexpectedIdentifier::expected_one(SimpleSpan::new(5, 11), "export", "pub");
  assert_eq!(err.found(), &"export");
  assert_eq!(err.expected(), Expected::one("pub"));
}

#[test]
fn unexpected_identifier_bump() {
  let mut err = UnexpectedIdentifier::expected_one(SimpleSpan::new(10, 13), "var", "let");
  err.bump(&5);
  assert_eq!(err.span(), SimpleSpan::new(15, 18));
}

#[test]
fn unexpected_identifier_error_trait() {
  let err = UnexpectedIdentifier::expected_one(SimpleSpan::new(0, 3), "var", "let");
  let e: &dyn std::error::Error = &err;
  assert!(!e.to_string().is_empty());
}

#[test]
fn unexpected_identifier_debug_clone_eq() {
  let err = UnexpectedIdentifier::expected_one(SimpleSpan::new(0, 3), "var", "let");
  let cloned = err.clone();
  assert_eq!(err, cloned);
  assert!(!format!("{:?}", err).is_empty());
}

// ── Unterminated ─────────────────────────────────────────────────────────────

#[test]
fn unterminated_new_and_accessors() {
  let err = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  assert_eq!(err.span(), SimpleSpan::new(5, 7));
  assert_eq!(err.knowledge(), "spread operator");
  assert_eq!(err.knowledge_ref(), &"spread operator");
  assert_eq!(err.span_ref(), &SimpleSpan::new(5, 7));
}

#[test]
fn unterminated_display() {
  let err = Unterminated::new(SimpleSpan::new(10, 12), "escape sequence");
  assert_eq!(format!("{}", err), "unterminated escape sequence");
}

#[test]
fn unterminated_bump() {
  let mut err = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  err.bump(&100);
  assert_eq!(err.span(), SimpleSpan::new(105, 107));
}

#[test]
fn unterminated_span_mut() {
  let mut err = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  *err.span_mut() = SimpleSpan::new(20, 30);
  assert_eq!(err.span(), SimpleSpan::new(20, 30));
}

#[test]
fn unterminated_into_components() {
  let err = Unterminated::new(SimpleSpan::new(10, 12), "escape sequence");
  let (span, knowledge) = err.into_components();
  assert_eq!(span, SimpleSpan::new(10, 12));
  assert_eq!(knowledge, "escape sequence");
}

#[test]
fn unterminated_error_trait() {
  let err = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  let e: &dyn std::error::Error = &err;
  assert!(!e.to_string().is_empty());
}

#[test]
fn unterminated_debug_clone_eq() {
  let err = Unterminated::new(SimpleSpan::new(5, 7), "spread operator");
  let cloned = err.clone();
  assert_eq!(err, cloned);
  assert!(!format!("{:?}", err).is_empty());
}

// ── Unclosed ─────────────────────────────────────────────────────────────────

#[test]
fn unclosed_new_and_accessors() {
  let err = Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into());
  assert_eq!(err.span(), SimpleSpan::new(5, 6));
  assert_eq!(err.name_ref(), "{");
  assert_eq!(err.span_ref(), &SimpleSpan::new(5, 6));
}

#[test]
fn unclosed_display() {
  let err = Unclosed::<char>::new(SimpleSpan::new(0, 1), "(".into());
  assert_eq!(format!("{}", err), "unclosed delimiter '('");
}

#[test]
fn unclosed_paren() {
  let err = Unclosed::paren(SimpleSpan::new(3, 4));
  assert_eq!(err.name_ref(), "()");
  assert_eq!(err.span(), SimpleSpan::new(3, 4));
}

#[test]
fn unclosed_bracket() {
  let err = Unclosed::bracket(SimpleSpan::new(8, 9));
  assert_eq!(err.name_ref(), "[]");
}

#[test]
fn unclosed_brace() {
  let err = Unclosed::brace(SimpleSpan::new(12, 13));
  assert_eq!(err.name_ref(), "{}");
}

#[test]
fn unclosed_angle() {
  let err = Unclosed::angle(SimpleSpan::new(20, 21));
  assert_eq!(err.name_ref(), "<>");
}

#[test]
fn unclosed_paren_of() {
  use tokora::punct::Paren;
  let err = Unclosed::<Paren, _>::paren_of(SimpleSpan::new(3, 4));
  assert_eq!(err.name_ref(), "()");
}

#[test]
fn unclosed_bracket_of() {
  use tokora::punct::Bracket;
  let err = Unclosed::<Bracket, _>::bracket_of(SimpleSpan::new(8, 9));
  assert_eq!(err.name_ref(), "[]");
}

#[test]
fn unclosed_brace_of() {
  use tokora::punct::Brace;
  let err = Unclosed::<Brace, _>::brace_of(SimpleSpan::new(12, 13));
  assert_eq!(err.name_ref(), "{}");
}

#[test]
fn unclosed_angle_of() {
  use tokora::punct::Angle;
  let err = Unclosed::<Angle, _>::angle_of(SimpleSpan::new(20, 21));
  assert_eq!(err.name_ref(), "<>");
}

#[test]
fn unclosed_of() {
  let err = Unclosed::<char>::of(SimpleSpan::new(5, 6), "{".into());
  assert_eq!(err.name_ref(), "{");
}

#[test]
fn unclosed_span_mut() {
  let mut err = Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into());
  *err.span_mut() = SimpleSpan::new(50, 60);
  assert_eq!(err.span(), SimpleSpan::new(50, 60));
}

#[test]
fn unclosed_bump() {
  let mut err = Unclosed::<char>::new(SimpleSpan::new(5, 6), "(".into());
  err.bump(&100);
  assert_eq!(err.span(), SimpleSpan::new(105, 106));
}

#[test]
fn unclosed_into_components() {
  let err = Unclosed::<char>::new(SimpleSpan::new(10, 11), "\"".into());
  let (span, name) = err.into_components();
  assert_eq!(span, SimpleSpan::new(10, 11));
  assert_eq!(name, CowStr::from("\""));
}

// The blackhole error type absorbs the unclosed diagnostic through the umbrella
// (`FromUnclosed`), which quantifies over the delimiter pair; the per-pair
// `From<Unclosed<D, …>> for ()` it replaced is gone.
#[test]
fn unclosed_from_into_unit() {
  let err = Unclosed::<char, SimpleSpan>::new(SimpleSpan::new(5, 6), "{".into());
  let _: () = <() as FromUnclosed<'_, TestLexer<'_>>>::from_unclosed(err);
}

#[test]
fn unclosed_error_trait() {
  let err = Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into());
  let e: &dyn std::error::Error = &err;
  assert!(!e.to_string().is_empty());
}

#[test]
fn unclosed_debug_clone_eq() {
  let err = Unclosed::<char>::new(SimpleSpan::new(5, 6), "{".into());
  let cloned = err.clone();
  assert_eq!(err, cloned);
  assert!(!format!("{:?}", err).is_empty());
}

// ── UnknownLexeme (uncovered paths) ──────────────────────────────────────────

#[test]
fn unknown_lexeme_from_range_const() {
  let err: UnknownLexeme<char, _> =
    UnknownLexeme::from_range_const(SimpleSpan::new(10, 15), "valid: semicolon");
  assert!(err.is_range());
  assert_eq!(err.unwrap_range().start(), 10);
  assert_eq!(*err.knowledge(), "valid: semicolon");
}

#[test]
fn unknown_lexeme_from_range() {
  let err: UnknownLexeme<char, _> = UnknownLexeme::from_range(10..15, "valid: closing brace");
  assert!(err.is_range());
  assert_eq!(err.unwrap_range().start(), 10);
}

#[test]
fn unknown_lexeme_unknown_characters() {
  let err = UnknownLexeme::<char, Characters>::unknown_characters(SimpleSpan::new(7, 9));
  assert!(!err.is_char());
  assert_eq!(err.unwrap_range().start(), 7);
}

#[test]
fn unknown_lexeme_unknown_character() {
  let err = UnknownLexeme::unknown_character(7usize, '#');
  assert!(err.is_char());
  assert_eq!(err.unwrap_char().position(), 7);
}

#[test]
fn unknown_lexeme_knowledge_mut() {
  let mut err = UnknownLexeme::from_positioned_char(
    PositionedChar::with_position('x', 5usize),
    String::from("valid: digit"),
  );
  err.knowledge_mut().push_str(" or letter");
  assert_eq!(err.knowledge(), "valid: digit or letter");
}

#[test]
fn unknown_lexeme_into_lexeme() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('!', 10usize), "identifier");
  let lexeme = err.into_lexeme();
  assert!(lexeme.is_char());
}

#[test]
fn unknown_lexeme_into_knowledge() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('!', 10usize), "identifier");
  let knowledge = err.into_knowledge();
  assert_eq!(knowledge, "identifier");
}

#[test]
fn unknown_lexeme_into_components() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('!', 10usize), "identifier");
  let (lexeme, knowledge) = err.into_components();
  assert!(lexeme.is_char());
  assert_eq!(knowledge, "identifier");
}

#[test]
fn unknown_lexeme_map_char() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('a', 5usize), "digit");
  let upper = err.map_char(|c| c.to_ascii_uppercase());
  assert_eq!(upper.unwrap_char().char(), 'A');
  assert_eq!(*upper.knowledge(), "digit");
}

#[test]
fn unknown_lexeme_map_knowledge() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('@', 5usize), "digit");
  let detailed = err.map_knowledge(|k| format!("unrecognized, valid: {}", k));
  assert_eq!(detailed.knowledge(), "unrecognized, valid: digit");
}

#[test]
fn unknown_lexeme_map_both() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('a', 5usize), "number");
  let transformed = err.map(|c| c.to_ascii_uppercase(), |k| format!("valid: {}", k));
  assert_eq!(transformed.unwrap_char().char(), 'A');
  assert_eq!(transformed.knowledge(), "valid: number");
}

#[test]
fn unknown_lexeme_span_with() {
  let err = UnknownLexeme::from_positioned_char(
    PositionedChar::with_position('\u{20AC}', 5usize), // euro sign, 3 bytes
    "ASCII character",
  );
  let span = err.span_with(|c: &char| c.len_utf8());
  assert_eq!(span.start(), 5);
  assert_eq!(span.end(), 8);
}

#[test]
fn unknown_lexeme_display_char() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('$', 42usize), "identifier");
  let display = format!("{}", err);
  assert!(display.contains("unknown character"));
  assert!(display.contains("42"));
}

#[test]
fn unknown_lexeme_display_range() {
  let err: UnknownLexeme<char, _> = UnknownLexeme::from_range(10..15, "valid token");
  let display = format!("{}", err);
  assert!(display.contains("unknown lexeme"));
}

#[test]
fn unknown_lexeme_error_trait() {
  let err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('$', 42usize), "identifier");
  let e: &dyn std::error::Error = &err;
  assert!(!e.to_string().is_empty());
}

#[test]
fn unknown_lexeme_deref_mut() {
  let mut err =
    UnknownLexeme::from_positioned_char(PositionedChar::with_position('x', 5usize), "digit");
  // DerefMut to Lexeme
  err.lexeme_mut().bump(&10);
  assert_eq!(err.unwrap_char().position(), 15);
}

#[test]
fn unknown_lexeme_new_with_lexeme() {
  let lexeme = Lexeme::from(PositionedChar::with_position('\u{00A7}', 5usize));
  let err = UnknownLexeme::new(lexeme, "valid: identifier");
  assert_eq!(*err.knowledge(), "valid: identifier");
  assert!(err.lexeme().is_char());
}

// ── UnexpectedLexeme (uncovered paths) ───────────────────────────────────────

#[test]
fn unexpected_lexeme_from_range_const() {
  let err: UnexpectedLexeme<char, _> =
    UnexpectedLexeme::from_range_const(SimpleSpan::new(10, 15), "semicolon");
  assert!(err.is_range());
}

#[test]
fn unexpected_lexeme_from_range() {
  let err: UnexpectedLexeme<char, _> = UnexpectedLexeme::from_range(10..15, "closing brace");
  assert!(err.is_range());
  assert_eq!(err.unwrap_range().start(), 10);
}

#[test]
fn unexpected_lexeme_new_line() {
  let err = UnexpectedLexeme::new_line(5usize, '\n');
  assert_eq!(*err.hint(), LineTerminator::NewLine);
}

#[test]
fn unexpected_lexeme_carriage_return() {
  let err = UnexpectedLexeme::carriage_return(5usize, '\r');
  assert_eq!(*err.hint(), LineTerminator::CarriageReturn);
}

#[test]
fn unexpected_lexeme_carriage_return_new_line() {
  let err = UnexpectedLexeme::<char, _>::carriage_return_new_line(SimpleSpan::new(5, 7));
  assert_eq!(*err.hint(), LineTerminator::CarriageReturnNewLine);
}

#[test]
fn unexpected_lexeme_hint_mut() {
  let mut err = UnexpectedLexeme::from_positioned_char(
    PositionedChar::with_position('x', 5usize),
    String::from("digit"),
  );
  err.hint_mut().push_str(" or letter");
  assert_eq!(err.hint(), "digit or letter");
}

#[test]
fn unexpected_lexeme_into_lexeme() {
  let err = UnexpectedLexeme::from_positioned_char(
    PositionedChar::with_position('!', 10usize),
    "identifier",
  );
  let lexeme = err.into_lexeme();
  assert!(lexeme.is_char());
}

#[test]
fn unexpected_lexeme_into_hint() {
  let err = UnexpectedLexeme::from_positioned_char(
    PositionedChar::with_position('!', 10usize),
    "identifier",
  );
  let hint = err.into_hint();
  assert_eq!(hint, "identifier");
}

#[test]
fn unexpected_lexeme_into_components() {
  let err = UnexpectedLexeme::from_positioned_char(
    PositionedChar::with_position('!', 10usize),
    "identifier",
  );
  let (lexeme, hint) = err.into_components();
  assert!(lexeme.is_char());
  assert_eq!(hint, "identifier");
}

#[test]
fn unexpected_lexeme_map_char() {
  let err =
    UnexpectedLexeme::from_positioned_char(PositionedChar::with_position('a', 5usize), "digit");
  let upper = err.map_char(|c| c.to_ascii_uppercase());
  assert_eq!(upper.unwrap_char().char(), 'A');
  assert_eq!(*upper.hint(), "digit");
}

#[test]
fn unexpected_lexeme_map_hint() {
  let err =
    UnexpectedLexeme::from_positioned_char(PositionedChar::with_position('!', 5usize), "digit");
  let detailed = err.map_hint(|h| format!("expected {}", h));
  assert_eq!(detailed.hint(), "expected digit");
}

#[test]
fn unexpected_lexeme_map_both() {
  let err =
    UnexpectedLexeme::from_positioned_char(PositionedChar::with_position('a', 5usize), "number");
  let transformed = err.map(|c| c.to_ascii_uppercase(), |h| format!("expected {}", h));
  assert_eq!(transformed.unwrap_char().char(), 'A');
  assert_eq!(transformed.hint(), "expected number");
}

#[test]
fn unexpected_lexeme_span_with() {
  let err = UnexpectedLexeme::from_positioned_char(
    PositionedChar::with_position('\u{20AC}', 5usize),
    "ASCII character",
  );
  let span = err.span_with(|c: &char| c.len_utf8());
  assert_eq!(span.start(), 5);
  assert_eq!(span.end(), 8);
}

#[test]
fn unexpected_lexeme_display_char() {
  let err = UnexpectedLexeme::from_positioned_char(
    PositionedChar::with_position('$', 42usize),
    "identifier",
  );
  let display = format!("{}", err);
  assert!(display.contains("unexpected character"));
  assert!(display.contains("42"));
}

#[test]
fn unexpected_lexeme_display_range() {
  let err: UnexpectedLexeme<char, _> = UnexpectedLexeme::from_range(10..15, "token");
  let display = format!("{}", err);
  assert!(display.contains("unexpected characters"));
}

#[test]
fn unexpected_lexeme_error_trait() {
  let err = UnexpectedLexeme::from_positioned_char(
    PositionedChar::with_position('$', 42usize),
    "identifier",
  );
  let e: &dyn std::error::Error = &err;
  assert!(!e.to_string().is_empty());
}

#[test]
fn unexpected_lexeme_new_with_lexeme() {
  let lexeme = Lexeme::from(PositionedChar::with_position('!', 5usize));
  let err = UnexpectedLexeme::new(lexeme, "identifier");
  assert_eq!(*err.hint(), "identifier");
  assert!(err.lexeme().is_char());
}

#[test]
fn unexpected_lexeme_from_char() {
  let err = UnexpectedLexeme::from_char(42usize, '$', "alphanumeric character");
  assert!(err.is_char());
  assert_eq!(err.unwrap_char().position(), 42);
}

#[test]
fn unexpected_lexeme_deref_mut() {
  let mut err =
    UnexpectedLexeme::from_positioned_char(PositionedChar::with_position('x', 5usize), "digit");
  // DerefMut -- use the mutable deref to bump the underlying lexeme
  use std::ops::DerefMut;
  let lexeme_ref: &mut Lexeme<char, usize> = err.deref_mut();
  lexeme_ref.bump(&10);
  assert_eq!(err.unwrap_char().position(), 15);
}
