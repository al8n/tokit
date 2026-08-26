use super::*;
use std::{
  string::{String, ToString},
  vec,
  vec::Vec,
};

// --- Recoverable tests ---

#[test]
fn recoverable_node() {
  let r = Recoverable::<i32>::Node(42);
  assert!(r.is_node());
  assert!(!r.is_error());
  assert!(!r.is_missing());
}

#[test]
fn recoverable_error() {
  let r = Recoverable::<i32>::Error(SimpleSpan::new(0, 5));
  assert!(!r.is_node());
  assert!(r.is_error());
  assert!(!r.is_missing());
}

#[test]
fn recoverable_missing() {
  let r = Recoverable::<i32>::Missing(SimpleSpan::new(0, 5));
  assert!(!r.is_node());
  assert!(!r.is_error());
  assert!(r.is_missing());
}

#[test]
fn recoverable_from_value() {
  let r: Recoverable<i32> = 42.into();
  assert!(r.is_node());
  assert_eq!(r.try_unwrap_node(), Ok(42));
}

#[test]
fn recoverable_error_node_impl() {
  let err = Recoverable::<i32>::error(SimpleSpan::new(0, 5));
  assert!(err.is_error());

  let missing = Recoverable::<i32>::missing(SimpleSpan::new(0, 5));
  assert!(missing.is_missing());
}

// --- Ident tests ---

#[test]
fn ident_new_and_accessors() {
  struct MyLang;
  let ident = Ident::<&str, SimpleSpan, MyLang>::new(SimpleSpan::new(0, 3), "foo");
  assert_eq!(ident.span(), SimpleSpan::new(0, 3));
  assert_eq!(ident.source(), "foo");
  assert_eq!(ident.source_ref(), &"foo");
  assert!(ident.is_valid());
  assert!(!ident.is_error());
  assert!(!ident.is_missing());
}

#[test]
fn ident_span_mut() {
  let mut ident = Ident::<&str>::new(SimpleSpan::new(0, 3), "foo");
  *ident.span_mut() = SimpleSpan::new(10, 13);
  assert_eq!(ident.span(), SimpleSpan::new(10, 13));
}

#[test]
fn ident_source_mut() {
  let mut ident = Ident::<String>::new(SimpleSpan::new(0, 3), "foo".to_string());
  *ident.source_mut() = "bar".to_string();
  assert_eq!(ident.source_ref(), "bar");
}

#[test]
fn ident_map() {
  let ident = Ident::<&str>::new(SimpleSpan::new(0, 3), "foo");
  let mapped = ident.map(|s| s.to_uppercase());
  assert_eq!(mapped.source_ref(), "FOO");
  assert_eq!(mapped.span(), SimpleSpan::new(0, 3));
}

#[test]
fn ident_map_preserves_recovery_status() {
  struct MyLang;

  type Borrowed = Ident<&'static str, SimpleSpan, MyLang>;
  type Owned = Ident<String, SimpleSpan, MyLang>;

  let span = SimpleSpan::new(4, 7);

  // Recovery status is orthogonal to the source representation, so a source-only `map` must
  // carry it across. The valid row is the non-vacuity control: it passes both before and after
  // the fix, so a green table is evidence about the two recovery rows.
  let rows: [(&str, Borrowed, &str, [bool; 3]); 3] = [
    (
      "valid",
      Borrowed::new(span, "name"),
      "NAME",
      [true, false, false],
    ),
    (
      "error",
      Borrowed::error(span),
      "<ERROR>",
      [false, true, false],
    ),
    (
      "missing",
      Borrowed::missing(span),
      "<MISSING>",
      [false, false, true],
    ),
  ];

  for (label, ident, expected_source, [valid, error, missing]) in rows {
    let mapped: Owned = ident.map(str::to_uppercase);
    assert_eq!(mapped.source_ref(), expected_source, "{label}: source");
    assert_eq!(mapped.span(), span, "{label}: span");
    assert_eq!(mapped.is_valid(), valid, "{label}: is_valid");
    assert_eq!(mapped.is_error(), error, "{label}: is_error");
    assert_eq!(mapped.is_missing(), missing, "{label}: is_missing");
  }
}

#[test]
fn ident_list_of_mapped_recovery_segments_is_not_valid() {
  let span = SimpleSpan::new(4, 7);

  let error = Ident::<&str>::error(span).map(str::to_uppercase);
  let missing = Ident::<&str>::missing(span).map(str::to_uppercase);

  let path = IdentList::<String>::new(span, vec![error, missing]);
  assert!(!path.is_valid());
  assert!(path.is_error());
  assert!(path.is_missing());
}

#[test]
fn ident_into_components() {
  use crate::utils::IntoComponents;
  let ident = Ident::<&str>::new(SimpleSpan::new(0, 3), "foo");
  let (span, source) = ident.into_components();
  assert_eq!(span, SimpleSpan::new(0, 3));
  assert_eq!(source, "foo");
}

#[test]
fn ident_error_node() {
  let err = Ident::<&str>::error(SimpleSpan::new(0, 5));
  assert!(err.is_error());
  assert_eq!(err.source(), "<error>");
}

#[test]
fn ident_missing_node() {
  let missing = Ident::<&str>::missing(SimpleSpan::new(0, 5));
  assert!(missing.is_missing());
  assert_eq!(missing.source(), "<missing>");
}

// --- Keyword tests ---

#[test]
fn keyword_new_and_accessors() {
  let kw = Keyword::<&str>::new(SimpleSpan::new(5, 11), "return");
  assert_eq!(kw.span(), SimpleSpan::new(5, 11));
  assert_eq!(kw.source(), "return");
  assert_eq!(kw.source_ref(), &"return");
}

#[test]
fn keyword_span_mut() {
  let mut kw = Keyword::<&str>::new(SimpleSpan::new(0, 3), "let");
  *kw.span_mut() = SimpleSpan::new(10, 13);
  assert_eq!(kw.span(), SimpleSpan::new(10, 13));
}

#[test]
fn keyword_source_mut() {
  let mut kw = Keyword::<String>::new(SimpleSpan::new(0, 3), "let".to_string());
  *kw.source_mut() = "var".to_string();
  assert_eq!(kw.source_ref(), "var");
}

#[test]
fn keyword_map() {
  let kw = Keyword::<&str>::new(SimpleSpan::new(0, 3), "let");
  let mapped = kw.map(|s| s.to_uppercase());
  assert_eq!(mapped.source_ref(), "LET");
}

#[test]
fn keyword_into_components() {
  let kw = Keyword::<&str>::new(SimpleSpan::new(0, 3), "let");
  let (span, source) = kw.into_components();
  assert_eq!(span, SimpleSpan::new(0, 3));
  assert_eq!(source, "let");
}

#[test]
fn keyword_into_ident() {
  let kw = Keyword::<&str>::new(SimpleSpan::new(0, 3), "let");
  let ident: Ident<&str> = kw.into();
  assert_eq!(ident.source(), "let");
  assert_eq!(ident.span(), SimpleSpan::new(0, 3));
  assert!(ident.is_valid());
}

#[test]
fn keyword_error_node() {
  let err = Keyword::<&str>::error(SimpleSpan::new(0, 5));
  assert!(err.is_error());
  assert!(!err.is_valid());
  assert_eq!(err.source(), "<error>");
}

#[test]
fn keyword_missing_node() {
  let missing = Keyword::<&str>::missing(SimpleSpan::new(0, 5));
  assert!(missing.is_missing());
  assert!(!missing.is_valid());
  assert_eq!(missing.source(), "<missing>");
}

/// The conversion carries the recovery status instead of fabricating `Valid`. The valid row is
/// the non-vacuity control: it passed before this channel existed, so a green table is evidence
/// about the two recovery rows.
#[test]
fn keyword_into_ident_carries_the_recovery_status() {
  let span = SimpleSpan::new(4, 7);

  let rows: [(&str, Keyword<&str>, [bool; 3]); 3] = [
    ("valid", Keyword::new(span, "let"), [true, false, false]),
    ("error", Keyword::error(span), [false, true, false]),
    ("missing", Keyword::missing(span), [false, false, true]),
  ];

  for (label, keyword, [valid, error, missing]) in rows {
    let ident: Ident<&str> = keyword.into();
    assert_eq!(ident.is_valid(), valid, "{label}: is_valid");
    assert_eq!(ident.is_error(), error, "{label}: is_error");
    assert_eq!(ident.is_missing(), missing, "{label}: is_missing");
  }
}

/// A source-only `map` changes how the spelling is stored, not whether the parser found one.
#[test]
fn keyword_map_preserves_recovery_status() {
  let span = SimpleSpan::new(0, 3);

  let rows: [(&str, Keyword<&str>, &str, [bool; 3]); 3] = [
    (
      "valid",
      Keyword::new(span, "let"),
      "LET",
      [true, false, false],
    ),
    (
      "error",
      Keyword::error(span),
      "<ERROR>",
      [false, true, false],
    ),
    (
      "missing",
      Keyword::missing(span),
      "<MISSING>",
      [false, false, true],
    ),
  ];

  for (label, keyword, expected_source, [valid, error, missing]) in rows {
    let mapped: Keyword<String> = keyword.map(str::to_uppercase);
    assert_eq!(mapped.source_ref(), expected_source, "{label}: source");
    assert_eq!(mapped.span(), span, "{label}: span");
    assert_eq!(mapped.is_valid(), valid, "{label}: is_valid");
    assert_eq!(mapped.is_error(), error, "{label}: is_error");
    assert_eq!(mapped.is_missing(), missing, "{label}: is_missing");
  }
}

/// The payload is not the channel: spelling a sentinel into a carrier by hand does not make it
/// a recovery placeholder, and editing a placeholder's payload does not make it valid syntax.
#[test]
fn a_sentinel_payload_is_not_a_recovery_status() {
  let span = SimpleSpan::new(0, 7);

  let mut hand_written = Keyword::<&str>::new(span, "<error>");
  assert!(hand_written.is_valid());
  assert!(!hand_written.is_error());

  *hand_written.source_mut() = "<missing>";
  assert!(hand_written.is_valid());
  assert!(!hand_written.is_missing());

  let mut recovered = Keyword::<&str>::missing(span);
  *recovered.source_mut() = "let";
  assert!(recovered.is_missing());
  assert!(!recovered.is_valid());

  let mut lit = LitDecimal::<&str>::error(span);
  *lit.data_mut() = "42";
  assert!(lit.is_error());
  assert!(!lit.is_valid());
}

// --- Literal types tests ---

#[test]
fn lit_decimal_new_and_accessors() {
  let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
  assert_eq!(lit.span(), SimpleSpan::new(0, 2));
  assert_eq!(lit.data(), "42");
  assert_eq!(lit.data_ref(), &"42");
}

#[test]
fn lit_decimal_span_mut() {
  let mut lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
  *lit.span_mut() = SimpleSpan::new(10, 12);
  assert_eq!(lit.span(), SimpleSpan::new(10, 12));
}

#[test]
fn lit_decimal_data_mut() {
  let mut lit = LitDecimal::<String>::new(SimpleSpan::new(0, 2), "42".to_string());
  *lit.data_mut() = "99".to_string();
  assert_eq!(lit.data_ref(), "99");
}

#[test]
fn lit_decimal_error_node() {
  let err = LitDecimal::<&str>::error(SimpleSpan::new(0, 5));
  assert!(err.is_error());
  assert!(!err.is_valid());
  assert_eq!(err.data(), "<error>");
}

#[test]
fn lit_decimal_missing_node() {
  let missing = LitDecimal::<&str>::missing(SimpleSpan::new(0, 5));
  assert!(missing.is_missing());
  assert!(!missing.is_valid());
  assert_eq!(missing.data(), "<missing>");
}

#[test]
fn lit_bool_new() {
  let lit = LitBool::<bool>::new(SimpleSpan::new(0, 4), true);
  assert!(lit.data());
}

#[test]
fn lit_null_new() {
  let lit = LitNull::<()>::new(SimpleSpan::new(0, 4), ());
  assert_eq!(lit.span(), SimpleSpan::new(0, 4));
}

#[test]
fn lit_string_new() {
  let lit = LitString::<&str>::new(SimpleSpan::new(0, 7), "\"hello\"");
  assert_eq!(lit.data(), "\"hello\"");
}

#[test]
fn lit_hex_new() {
  let lit = LitHex::<&str>::new(SimpleSpan::new(0, 4), "0xFF");
  assert_eq!(lit.data(), "0xFF");
}

#[test]
fn lit_into_components() {
  use crate::utils::IntoComponents;
  let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
  let (span, data) = IntoComponents::into_components(lit);
  assert_eq!(span, SimpleSpan::new(0, 2));
  assert_eq!(data, "42");
}

#[test]
fn unsized_carriers_expose_borrowed_accessors() {
  trait TestLang {}

  fn ident_accessors(ident: &Ident<str>) {
    let _: SimpleSpan = ident.span();
    let _: &SimpleSpan = ident.span_ref();
    let _: &str = ident.source_ref();
    let _: &SimpleSpan = ident.as_span();
    let _ = (ident.is_valid(), ident.is_error(), ident.is_missing());
  }

  fn ident_mut_accessors(ident: &mut Ident<str>) {
    let _: &mut SimpleSpan = ident.span_mut();
    let _: &mut str = ident.source_mut();
  }

  fn keyword_accessors(keyword: &Keyword<str>) {
    let _: SimpleSpan = keyword.span();
    let _: &SimpleSpan = keyword.span_ref();
    let _: &str = keyword.source_ref();
    let _: &SimpleSpan = keyword.as_span();
  }

  fn keyword_mut_accessors(keyword: &mut Keyword<str>) {
    let _: &mut SimpleSpan = keyword.span_mut();
    let _: &mut str = keyword.source_mut();
  }

  fn literal_accessors(literal: &LitDecimal<str>) {
    let _: SimpleSpan = literal.span();
    let _: &SimpleSpan = literal.span_ref();
    let _: &str = literal.data_ref();
    let _: &SimpleSpan = literal.as_span();
  }

  fn literal_mut_accessors(literal: &mut LitDecimal<str>) {
    let _: &mut SimpleSpan = literal.span_mut();
    let _: &mut str = literal.data_mut();
  }

  fn unsized_language_markers(
    _: &Ident<str, SimpleSpan, dyn TestLang>,
    _: &Keyword<str, SimpleSpan, dyn TestLang>,
    _: &LitDecimal<str, SimpleSpan, dyn TestLang>,
  ) {
  }

  type DynLangIdentList = IdentList<
    &'static str,
    SimpleSpan,
    Vec<Ident<&'static str, SimpleSpan, dyn TestLang>>,
    dyn TestLang,
  >;

  fn ident_list_accessors(list: &DynLangIdentList) {
    let _: SimpleSpan = list.span();
    let _: &SimpleSpan = list.span_ref();
    let _: &SimpleSpan = list.as_span();
    let _ = (list.is_valid(), list.is_error(), list.is_missing());
    let _: &[Ident<&str, SimpleSpan, dyn TestLang>] = list.identifiers_slice();
  }

  let _ = ident_accessors as fn(&Ident<str>);
  let _ = ident_mut_accessors as fn(&mut Ident<str>);
  let _ = keyword_accessors as fn(&Keyword<str>);
  let _ = keyword_mut_accessors as fn(&mut Keyword<str>);
  let _ = literal_accessors as fn(&LitDecimal<str>);
  let _ = literal_mut_accessors as fn(&mut LitDecimal<str>);
  let _ = ident_list_accessors as fn(&DynLangIdentList);
  let _ = unsized_language_markers
    as fn(
      &Ident<str, SimpleSpan, dyn TestLang>,
      &Keyword<str, SimpleSpan, dyn TestLang>,
      &LitDecimal<str, SimpleSpan, dyn TestLang>,
    );
}

// --- IdentList tests ---

#[test]
fn ident_list_new_and_accessors() {
  let idents = vec![
    Ident::<&str>::new(SimpleSpan::new(0, 3), "foo"),
    Ident::<&str>::new(SimpleSpan::new(4, 7), "bar"),
  ];
  let list = IdentList::<&str>::new(SimpleSpan::new(0, 7), idents);
  assert_eq!(list.span(), SimpleSpan::new(0, 7));
  assert_eq!(list.identifiers_slice().len(), 2);
  assert!(!list.is_empty());
  assert!(list.is_valid());
  assert!(!list.is_error());
  assert!(!list.is_missing());
}

#[test]
fn ident_list_empty() {
  let list = IdentList::<&str>::new(SimpleSpan::new(0, 0), Vec::new());
  assert!(list.is_empty());
}

#[test]
fn ident_list_with_error() {
  let idents = vec![
    Ident::<&str>::new(SimpleSpan::new(0, 3), "foo"),
    Ident::<&str>::error(SimpleSpan::new(4, 7)),
  ];
  let list = IdentList::<&str>::new(SimpleSpan::new(0, 7), idents);
  assert!(!list.is_valid());
  assert!(list.is_error());
}

#[test]
fn ident_list_with_missing() {
  let idents = vec![Ident::<&str>::missing(SimpleSpan::new(0, 3))];
  let list = IdentList::<&str>::new(SimpleSpan::new(0, 3), idents);
  assert!(list.is_missing());
}

// --- Additional Keyword tests for coverage ---

#[test]
fn keyword_span_ref() {
  let kw = Keyword::<&str>::new(SimpleSpan::new(5, 11), "return");
  assert_eq!(*kw.span_ref(), SimpleSpan::new(5, 11));
}

#[test]
fn keyword_as_span() {
  let kw = Keyword::<&str>::new(SimpleSpan::new(5, 11), "return");
  assert_eq!(*AsSpan::as_span(&kw), SimpleSpan::new(5, 11));
}

#[test]
fn keyword_into_components_trait() {
  use crate::utils::IntoComponents;
  let kw = Keyword::<&str>::new(SimpleSpan::new(0, 3), "let");
  let (span, source) = IntoComponents::into_components(kw);
  assert_eq!(span, SimpleSpan::new(0, 3));
  assert_eq!(source, "let");
}

#[test]
fn keyword_into_components_method() {
  let kw = Keyword::<&str>::new(SimpleSpan::new(0, 3), "let");
  let (span, source) = kw.into_components();
  assert_eq!(span, SimpleSpan::new(0, 3));
  assert_eq!(source, "let");
}

// --- Additional Ident tests for coverage ---

#[test]
fn ident_span_ref() {
  let ident = Ident::<&str>::new(SimpleSpan::new(0, 3), "foo");
  assert_eq!(*ident.span_ref(), SimpleSpan::new(0, 3));
}

#[test]
fn ident_as_span() {
  let ident = Ident::<&str>::new(SimpleSpan::new(0, 3), "foo");
  assert_eq!(*AsSpan::as_span(&ident), SimpleSpan::new(0, 3));
}

#[test]
fn ident_into_components_trait() {
  use crate::utils::IntoComponents;
  let ident = Ident::<&str>::new(SimpleSpan::new(0, 3), "foo");
  let (span, source) = IntoComponents::into_components(ident);
  assert_eq!(span, SimpleSpan::new(0, 3));
  assert_eq!(source, "foo");
}

#[test]
fn ident_bump() {
  let mut ident = Ident::<&str>::new(SimpleSpan::new(0, 3), "foo");
  ident.bump(&5);
  assert_eq!(ident.span(), SimpleSpan::new(5, 8));
}

// --- Additional IdentList tests for coverage ---

#[test]
fn ident_list_span_ref() {
  let list = IdentList::<&str>::new(SimpleSpan::new(0, 7), Vec::new());
  assert_eq!(*list.span_ref(), SimpleSpan::new(0, 7));
}

#[test]
fn ident_list_span_mut() {
  let mut list = IdentList::<&str>::new(SimpleSpan::new(0, 7), Vec::new());
  *list.span_mut() = SimpleSpan::new(10, 17);
  assert_eq!(list.span(), SimpleSpan::new(10, 17));
}

#[test]
fn ident_list_as_span() {
  let list = IdentList::<&str>::new(SimpleSpan::new(0, 7), Vec::new());
  assert_eq!(*AsSpan::as_span(&list), SimpleSpan::new(0, 7));
}

#[test]
fn ident_list_identifiers() {
  let idents = vec![Ident::<&str>::new(SimpleSpan::new(0, 3), "foo")];
  let list = IdentList::<&str>::new(SimpleSpan::new(0, 3), idents.clone());
  assert_eq!(list.identifiers().len(), 1);
}

#[test]
fn ident_list_bump() {
  let idents = vec![
    Ident::<&str>::new(SimpleSpan::new(0, 3), "foo"),
    Ident::<&str>::new(SimpleSpan::new(4, 7), "bar"),
  ];
  let mut list = IdentList::<&str>::new(SimpleSpan::new(0, 7), idents);
  list.bump(&10);
  assert_eq!(list.span(), SimpleSpan::new(10, 17));
  assert_eq!(list.identifiers_slice()[0].span(), SimpleSpan::new(10, 13));
  assert_eq!(list.identifiers_slice()[1].span(), SimpleSpan::new(14, 17));
}

// --- Additional Lit type tests for coverage ---

#[test]
fn lit_generic_new() {
  let lit = Lit::<&str>::new(SimpleSpan::new(0, 5), "value");
  assert_eq!(lit.data(), "value");
  assert_eq!(lit.span(), SimpleSpan::new(0, 5));
}

#[test]
fn lit_as_span() {
  let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
  assert_eq!(*AsSpan::as_span(&lit), SimpleSpan::new(0, 2));
}

#[test]
fn lit_octal_new() {
  let lit = LitOctal::<&str>::new(SimpleSpan::new(0, 4), "0o77");
  assert_eq!(lit.data(), "0o77");
}

#[test]
fn lit_binary_new() {
  let lit = LitBinary::<&str>::new(SimpleSpan::new(0, 6), "0b1010");
  assert_eq!(lit.data(), "0b1010");
}

#[test]
fn lit_float_new() {
  let lit = LitFloat::<&str>::new(SimpleSpan::new(0, 4), "3.14");
  assert_eq!(lit.data(), "3.14");
}

#[test]
fn lit_hex_float_new() {
  let lit = LitHexFloat::<&str>::new(SimpleSpan::new(0, 6), "0x1.8p3");
  assert_eq!(lit.data(), "0x1.8p3");
}

#[test]
fn lit_multiline_string_new() {
  let lit = LitMultilineString::<&str>::new(SimpleSpan::new(0, 10), "\"\"\"hi\"\"\"");
  assert_eq!(lit.data(), "\"\"\"hi\"\"\"");
}

#[test]
fn lit_raw_string_new() {
  let lit = LitRawString::<&str>::new(SimpleSpan::new(0, 8), "r\"hello\"");
  assert_eq!(lit.data(), "r\"hello\"");
}

#[test]
fn lit_char_new() {
  let lit = LitChar::<char>::new(SimpleSpan::new(0, 3), 'a');
  assert_eq!(lit.data(), 'a');
}

#[test]
fn lit_byte_new() {
  let lit = LitByte::<u8>::new(SimpleSpan::new(0, 4), b'a');
  assert_eq!(lit.data(), b'a');
}

#[test]
fn lit_byte_string_new() {
  let lit = LitByteString::<&str>::new(SimpleSpan::new(0, 8), "b\"bytes\"");
  assert_eq!(lit.data(), "b\"bytes\"");
}

#[test]
fn lit_true_new() {
  let lit = LitTrue::<()>::new(SimpleSpan::new(0, 4), ());
  assert_eq!(lit.span(), SimpleSpan::new(0, 4));
}

#[test]
fn lit_false_new() {
  let lit = LitFalse::<()>::new(SimpleSpan::new(0, 5), ());
  assert_eq!(lit.span(), SimpleSpan::new(0, 5));
}

#[test]
fn lit_decimal_into_components_trait() {
  use crate::utils::IntoComponents;
  let lit = LitHex::<&str>::new(SimpleSpan::new(0, 4), "0xFF");
  let (span, data) = IntoComponents::into_components(lit);
  assert_eq!(span, SimpleSpan::new(0, 4));
  assert_eq!(data, "0xFF");
}

#[test]
fn lit_error_node_generic() {
  let err = Lit::<&str>::error(SimpleSpan::new(0, 5));
  assert!(err.is_error());
  assert!(!err.is_valid());
  assert_eq!(err.data(), "<error>");
}

#[test]
fn lit_missing_node_generic() {
  let missing = Lit::<&str>::missing(SimpleSpan::new(0, 5));
  assert!(missing.is_missing());
  assert!(!missing.is_valid());
  assert_eq!(missing.data(), "<missing>");
}

/// A literal built by `new` is valid, whatever its data type — the status is the channel, and
/// `new` is the door that declares it.
#[test]
fn a_literal_built_by_new_is_valid() {
  assert!(LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42").is_valid());
  assert!(LitBool::<bool>::new(SimpleSpan::new(0, 4), true).is_valid());
  assert!(LitNull::<()>::new(SimpleSpan::new(0, 4), ()).is_valid());
}

#[test]
fn lit_bump() {
  let mut lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
  lit.bump(&5);
  assert_eq!(lit.span(), SimpleSpan::new(5, 7));
}
