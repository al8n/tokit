//! NAME_CENSUS — the separator name the driver supplies reaches the payload, in every one of
//! the five conversions that is handed one.
//!
//! # Why a census, and why here
//!
//! Each of these five conversions is a **blanket** impl over every downstream error type that
//! implements `From<MissingTokenOf>` / `From<SeparatedErrorOf>` — which such a type must, to
//! compose with the rest of the conversion family. Coherence therefore forbids it from writing
//! its own impl to recover anything the blanket drops: whatever the blanket does with the name
//! is what *every* user of the family gets, with no way to opt out.
//!
//! All five blankets once bound the name as `_name` and threw it away, so the default
//! diagnostic could say a separator was missing but never which one. The defect had exactly
//! one textual signature — the discard binding `_name:` in the parameter list — and it was
//! invisible to every test in the suite, because the in-tree fixture error types discard the
//! payload too.
//!
//! This census pins both halves: the discard binding can never come back, and each conversion
//! that receives a name must be seen to stamp it. Scoped per method body rather than per file,
//! because the law is per conversion: a file that stamps twice in one blanket and never in its
//! neighbour would satisfy any file-level count. `grep NAME_CENSUS` finds every anchor.
//!
//! Counting is line-based and skips `//`-prefixed lines, so the rationale comments that sit
//! directly above each stamp do not count; only code does. Keep code mentions of the counted
//! names off comment-trailing positions and out of string literals in these files.

/// The `emitter/separated` sources under census — the trait definitions and their blanket
/// conversions.
const SOURCES: &[(&str, &str)] = &[
  ("mod.rs", include_str!("mod.rs")),
  ("missing_leading.rs", include_str!("missing_leading.rs")),
  ("missing_trailing.rs", include_str!("missing_trailing.rs")),
  (
    "unexpected_leading.rs",
    include_str!("unexpected_leading.rs"),
  ),
  (
    "unexpected_trailing.rs",
    include_str!("unexpected_trailing.rs"),
  ),
];

/// Fetches a censused source by name.
fn source(name: &str) -> &'static str {
  SOURCES
    .iter()
    .find(|(n, _)| *n == name)
    .map(|(_, s)| *s)
    .unwrap_or_else(|| panic!("NAME_CENSUS: `{name}` is not a censused source"))
}

/// Counts occurrences of `needle` on the non-comment lines of `hay`.
fn count(hay: &str, needle: &str) -> usize {
  hay
    .lines()
    .filter(|line| !line.trim_start().starts_with("//"))
    .map(|line| line.matches(needle).count())
    .sum()
}

/// The last top-level `impl` block of `hay` — the blanket conversion.
///
/// Each censused file ends with its blanket impl, preceded by the trait declaration (and, for
/// the emitter traits, a `&mut U` forwarder). Slicing there is what keeps [`method_body`] from
/// matching the trait's *declaration* of the same method name, whose signature carries no body
/// to census.
fn blanket(hay: &str) -> std::string::String {
  let lines: std::vec::Vec<&str> = hay.lines().collect();
  let start = lines
    .iter()
    .rposition(|line| line.starts_with("impl"))
    .unwrap_or_else(|| panic!("NAME_CENSUS: no top-level `impl` block in the source"));
  lines[start..].join("\n")
}

/// The source lines of a two-space-indented `fn <name>(` in `hay`, from its signature through
/// the closing brace at that same indentation.
fn method_body(hay: &str, name: &str) -> std::string::String {
  let paren = std::format!("fn {name}(");
  let lines: std::vec::Vec<&str> = hay.lines().collect();
  let start = lines
    .iter()
    .position(|line| line.starts_with("  ") && !line.starts_with("   ") && line.contains(&paren))
    .unwrap_or_else(|| panic!("NAME_CENSUS: no two-space-indented `fn {name}(` in the source"));
  let len = lines[start..]
    .iter()
    .position(|line| *line == "  }")
    .unwrap_or_else(|| panic!("NAME_CENSUS: `fn {name}` has no closing brace at its indent"));
  lines[start..=start + len].join("\n")
}

/// NAME_CENSUS — the discard binding is gone and cannot return. `_name:` is the defect's exact
/// signature: a parameter the signature names and the body never reads.
#[test]
fn name_census_no_conversion_discards_its_name() {
  for (file, src) in SOURCES {
    let got = count(src, "_name:");
    assert!(
      got == 0,
      "NAME_CENSUS drift: `{file}` binds a separator name to `_name:` ({got}×), discarding it. \
       A blanket conversion is the only place the name can reach a downstream error type — \
       coherence forbids that type from overriding it — so a dropped name is dropped for every \
       user of the family. Stamp it into the payload instead (grep NAME_CENSUS)."
    );
  }
}

/// NAME_CENSUS — every conversion that is handed a name stamps it, exactly once, in its own
/// blanket body.
#[test]
fn name_census_every_named_conversion_stamps_the_name() {
  // (file, conversion handed a separator name)
  let stamped: &[(&str, &str)] = &[
    ("mod.rs", "from_missing_separator"),
    ("missing_leading.rs", "from_missing_leading_separator"),
    ("missing_trailing.rs", "from_missing_trailing_separator"),
    ("unexpected_leading.rs", "from_unexpected_leading_separator"),
    (
      "unexpected_trailing.rs",
      "from_unexpected_trailing_separator",
    ),
  ];

  for (file, method) in stamped {
    let body = method_body(&blanket(source(file)), method);
    let got = count(&body, ".with_name(");
    assert!(
      got == 1,
      "NAME_CENSUS drift: `{file}`'s `{method}` stamps the separator name {got} time(s), \
       expected 1. The name the driver supplied reaches a downstream error type through this \
       blanket or not at all (grep NAME_CENSUS)."
    );
  }

  // The stamp lives in the blanket, not scattered through the trait surface: the whole-file
  // count must equal the sum of the blanket-body counts.
  for (file, src) in SOURCES {
    let in_file = count(src, ".with_name(");
    let in_blanket = count(&blanket(src), ".with_name(");
    assert!(
      in_file == in_blanket,
      "NAME_CENSUS drift: `{file}` stamps a name outside its blanket conversion ({in_file} in \
       the file against {in_blanket} in the blanket) (grep NAME_CENSUS)."
    );
  }
}

/// NAME_CENSUS — the scope, from the other side: `from_missing_element` is handed no name, so
/// it must not invent one. This pins the boundary a future "stamp everything" edit would blur.
#[test]
fn name_census_the_nameless_conversion_stamps_nothing() {
  let body = method_body(&blanket(source("mod.rs")), "from_missing_element");
  assert!(
    count(&body, ".with_name(") == 0,
    "NAME_CENSUS drift: `from_missing_element` stamps a separator name it was never given. A \
     missing *element* has no separator to name (grep NAME_CENSUS)."
  );
}
