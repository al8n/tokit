use super::*;
use crate::cache::DefaultCache;
use crate::emitter::Fatal;
use crate::lexer::{DummyLexer, DummyToken};
use crate::parse_context::FatalContext;

#[test]
fn input_context_new_and_into_components() {
  let ctx = InputContext::new("emitter", 42u32);
  let (e, c) = ctx.into_components();
  assert_eq!(e, "emitter");
  assert_eq!(c, 42u32);
}

#[test]
fn input_context_different_types() {
  let ctx = InputContext::new(std::vec![1, 2, 3], Some("cache"));
  let (e, c) = ctx.into_components();
  assert_eq!(e, std::vec![1, 2, 3]);
  assert_eq!(c, Some("cache"));
}

#[test]
fn input_new_creates_input() {
  let input = Input::<'_, DummyLexer, FatalContext<'_, DummyLexer, ()>>::new("");
  // Just verify it compiles and can be created
  let _ = input;
}

#[test]
fn input_with_state_creates_input() {
  let input = Input::<'_, DummyLexer, FatalContext<'_, DummyLexer, ()>>::with_state("hello", ());
  let _ = input;
}

#[test]
fn input_with_state_and_cache() {
  let cache = DefaultCache::<'_, DummyLexer>::default();
  let input = Input::<'_, DummyLexer, FatalContext<'_, DummyLexer, ()>>::with_state_and_cache(
    "hello",
    (),
    cache,
  );
  let _ = input;
}

#[test]
fn input_clone() {
  let input = Input::<'_, DummyLexer, FatalContext<'_, DummyLexer, ()>>::new("hello");
  let cloned = input.clone();
  let _ = cloned;
}

#[test]
fn input_as_ref() {
  let mut input = Input::<'_, DummyLexer, FatalContext<'_, DummyLexer, ()>>::new("hello");
  let mut emitter = Fatal::<()>::new();
  let input_ref = input.as_ref(&mut emitter);
  let _ = input_ref;
}

// ── AS_REF_CENSUS — the roster of production `Input::as_ref` call sites ───────
//
// `front_reported_end` and `emitted_error_end` are suppression watermarks whose subject is ONE
// emitter's log, and `as_ref` chooses the emitter per borrow. See `Input::front_reported_end` for
// the class statement. They are safe today only because no call site pairs one `Input` with two
// different emitters — a property of in-crate discipline, not of the type system.
//
// # What this rail guarantees
//
// **Site identity is `(file, ordinal of the match within that file, receiver, emitter)`.** On that
// identity the rail detects **additions, removals and replacements** among literal-form call sites.
// That is all it claims.
//
// **The ordinal is a row-distinguisher and a diff aid, not a reorder oracle** — and it is load
// bearing, so do not delete it as vestigial. Six rows in `fuzz/partial.rs` are otherwise identical;
// the ordinal keeps them distinct, so removing one of them changes the multiset and is caught.
// Matches are sorted by byte offset before numbering, which makes the printed diff read in source
// order; that is a readability property, not a guarantee.
//
// **Reordering is deliberately not claimed, and this is the reason** — recorded so nobody re-adds
// the claim thinking it an oversight. It was asserted while designing the identity, never because
// anything needed it, and then cost three rounds of review to narrow (the ordinal was numbered per
// call shape; then the documentation still described the old behaviour; then a same-line pair with
// identical receiver and emitter names collapsed regardless of ordering). The disclosure this rail
// guards depends only on the **set** of call sites: no in-crate path pairs one `Input` with two
// different emitters. Two sites trading places changes nothing about that, whereas one site removed
// and a different one added — a *replacement*, which nets to the same count — changes it and is
// caught by the tuples. **A set-change rail does not need a position oracle.**
//
// **It does not identify which function or impl a site sits in.** Item scope is a fact about the
// syntax tree, and this is a line-based text scan, so it cannot compute one. An earlier version
// advertised `file + enclosing item + ordinal` while storing only a bare `fn` name, which aliased
// the three `fn parse_with_state` items in `parser/mod.rs` — one of which holds a real call site.
// Numbering repeated names was tried and rejected: it makes the roster shift wholesale when an
// unrelated same-named item is inserted above, which is a false positive, and a rail that cries
// wolf gets switched off. The claim was reduced to what a text scan actually delivers instead.
//
// **A same-file move that changes no matched text therefore passes.** That is a stated limitation,
// not a broken promise — and it is the distinction that matters, because the failure this rail has
// repeatedly had was advertising more than it did.
//
// **It is not a proof.** What protects the invariant is the disclosure at
// `Input::front_reported_end` together with `Input` being `pub(crate)` and unexported. A text rail
// cannot decide a property about emitter identity across borrows, because that property is not
// textual. **A newly discovered unmatched shape is a documented property of this rail, not a defect
// in it** — add it to the list below and move on. The exhaustive answer is the type-level one, bind
// the emitter to the `Input` so per-borrow choice cannot arise, filed for 0.9 because it also
// covers `emitted_error_end`.
//
// Known limitations, in one place: a call reached through a macro expansion or behind a generic
// helper; a non-literal receiver or emitter (a field, an index, a method call, a deref); a call
// inside an inline test module in a production file; a `.rs` file under `src` that is not part of
// the module tree, which is scanned anyway; raw string literals with hash delimiters and character
// literals, which the non-code stripper does not handle; and item scope, per above.
//
// # What it scans
//
// Every `.rs` file under the crate's `src`, with exclusions whitelisted — so a new file is covered
// by default and an exception needs a deliberate, visible edit. Comments and string literals are
// blanked before matching, because their contents are not calls. Exclusions:
//
// - whole-file test sources: `tests.rs`, `*_tests.rs`, anything under a `tests/` directory;
// - **inline** test modules: the file is read only up to `mod tests {` or `mod <name>_tests {`.
//   The brace is load-bearing. Matching `mod <name>_tests;` too would treat a *file declaration* as
//   a test region and blind the rest of the file: `conformance/mod.rs` declares `mod cache_tests;`
//   near its top, and an earlier draft truncated there, hid three real production call sites, and
//   passed its own roster comparison.

/// Whether `name` marks a test module whose region is excluded.
#[cfg(feature = "std")]
fn is_test_mod(name: &str) -> bool {
  name == "tests" || name.ends_with("_tests")
}

/// The line index to stop reading a production file at, if it opens an inline test module.
///
/// Matches `mod <name> {` only — never `mod <name>;`. See the census note for why.
#[cfg(feature = "std")]
fn test_mod_cut(text: &str) -> usize {
  for (i, line) in text.lines().enumerate() {
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix("mod ") else {
      continue;
    };
    let name: &str = rest
      .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
      .next()
      .unwrap_or("");
    if is_test_mod(name) && rest[name.len()..].trim_start().starts_with('{') {
      return i;
    }
  }
  usize::MAX
}

/// Reads the identifier at the start of `s`.
#[cfg(feature = "std")]
fn leading_ident(s: &str) -> &str {
  let n = s
    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
    .unwrap_or(s.len());
  &s[..n]
}

/// Blanks everything that is not code — block comments, line comments, and string literal
/// contents — preserving line structure so line-oriented scanning still works.
///
/// Without this the rail reports a **false positive** on the call text appearing inside a comment
/// or a string, including this very file's extractor, which contains the literal pattern it
/// searches for. A rail that cries wolf gets bypassed, so a false positive is a defect here rather
/// than a nuisance.
///
/// Known limitation: raw string literals with hash delimiters are not handled, and character
/// literals are not either, because a scanner this size cannot tell `'a'` from a lifetime.
#[cfg(feature = "std")]
fn strip_noncode(text: &str) -> std::string::String {
  let b = text.as_bytes();
  let mut out = std::string::String::with_capacity(text.len());
  let mut block = 0usize;
  let mut i = 0usize;
  while i < b.len() {
    if block > 0 {
      if b[i..].starts_with(b"*/") {
        block -= 1;
        out.push_str("  ");
        i += 2;
      } else if b[i..].starts_with(b"/*") {
        block += 1;
        out.push_str("  ");
        i += 2;
      } else {
        out.push(if b[i] == b'\n' { '\n' } else { ' ' });
        i += 1;
      }
      continue;
    }
    if b[i..].starts_with(b"/*") {
      block = 1;
      out.push_str("  ");
      i += 2;
      continue;
    }
    if b[i..].starts_with(b"//") {
      while i < b.len() && b[i] != b'\n' {
        out.push(' ');
        i += 1;
      }
      continue;
    }
    if b[i] == b'"' {
      out.push(' ');
      i += 1;
      while i < b.len() {
        if b[i] == b'\\' {
          out.push(' ');
          i += 1;
          if i < b.len() {
            out.push(if b[i] == b'\n' { '\n' } else { ' ' });
            i += 1;
          }
          continue;
        }
        if b[i] == b'"' {
          out.push(' ');
          i += 1;
          break;
        }
        out.push(if b[i] == b'\n' { '\n' } else { ' ' });
        i += 1;
      }
      continue;
    }
    out.push(if b[i].is_ascii() { b[i] as char } else { ' ' });
    i += 1;
  }
  out
}

/// One roster row: file, ordinal of the match within that file, receiver, emitter.
#[cfg(feature = "std")]
type Site = (
  std::string::String,
  usize,
  std::string::String,
  std::string::String,
);

/// Extracts every literal-form `as_ref` call site from one production source.
///
/// Two shapes are matched: the method form `recv.as_ref(&mut em)` and the UFCS form
/// `Input::as_ref(&mut recv, &mut em)`, which is ordinary Rust reaching the same method.
#[cfg(feature = "std")]
fn extract_sites(rel: &str, text: &str) -> std::vec::Vec<Site> {
  // Comments and string literals are blanked first: their contents are not calls.
  let code = strip_noncode(text);
  let cut = test_mod_cut(&code);
  let mut out: std::vec::Vec<Site> = std::vec::Vec::new();
  let mut ordinal = 0usize;
  for (i, line) in code.lines().enumerate() {
    if i >= cut {
      break;
    }
    // Both shapes are collected with their byte offset in the line, then ordered by position
    // before ordinals are assigned, so the ordinals — and the printed diff — read in source order.
    // That is readability only. It is NOT a reorder oracle: two sites that normalize to the same
    // `(receiver, emitter)` are indistinguishable whatever their order, and reordering is not among
    // the properties this rail claims. See the census note above for why it is not.
    let mut cands: std::vec::Vec<(usize, std::string::String, std::string::String)> =
      std::vec::Vec::new();

    // Method form: `recv.as_ref(&mut em)`.
    let mut rest = line;
    while let Some(at) = rest.find(".as_ref(&mut ") {
      let base = line.len() - rest.len();
      let head = &rest[..at];
      let rstart = head
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
      let receiver = &head[rstart..];
      let tail = &rest[at + ".as_ref(&mut ".len()..];
      let emitter = leading_ident(tail);
      if !receiver.is_empty() && !emitter.is_empty() {
        cands.push((
          base + rstart,
          std::string::String::from(receiver),
          std::string::String::from(emitter),
        ));
      }
      rest = &tail[emitter.len()..];
    }

    // UFCS form: `Type::as_ref(&mut recv, &mut em)`. The offset used is the start of the qualified
    // path segment, which is comparable with the method form's receiver start: both mark where the
    // call's leftmost token begins.
    let mut rest = line;
    while let Some(at) = rest.find("::as_ref(&mut ") {
      let base = line.len() - rest.len();
      let head = &rest[..at];
      let pstart = head
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .map_or(0, |i| i + 1);
      let tail = &rest[at + "::as_ref(&mut ".len()..];
      let receiver = leading_ident(tail);
      let after = tail[receiver.len()..].trim_start();
      let emitter = after
        .strip_prefix(",")
        .map(str::trim_start)
        .and_then(|s| s.strip_prefix("&mut "))
        .map(leading_ident)
        .unwrap_or("");
      if !receiver.is_empty() && !emitter.is_empty() {
        cands.push((
          base + pstart,
          std::string::String::from(receiver),
          std::string::String::from(emitter),
        ));
      }
      rest = &tail[receiver.len()..];
    }

    cands.sort_by_key(|(offset, _, _)| *offset);
    for (_, receiver, emitter) in cands {
      ordinal += 1;
      out.push((std::string::String::from(rel), ordinal, receiver, emitter));
    }
  }
  out
}

#[cfg(feature = "std")]
#[test]
fn as_ref_census_extractor_matches_both_call_shapes() {
  let text = "fn a() {\n  let x = input.as_ref(&mut emitter);\n}\nfn b() {\n  let y = \
              Input::as_ref(&mut inp2, &mut em2);\n}\n";
  let got = extract_sites("synthetic.rs", text);
  assert_eq!(
    got,
    std::vec![
      (
        std::string::String::from("synthetic.rs"),
        1,
        std::string::String::from("input"),
        std::string::String::from("emitter")
      ),
      (
        std::string::String::from("synthetic.rs"),
        2,
        std::string::String::from("inp2"),
        std::string::String::from("em2")
      ),
    ],
    "the extractor must recognize BOTH the method form and the UFCS form. The UFCS form has no \
     occurrence in the crate, so if its pattern breaks only this cell can tell you."
  );
}

#[cfg(feature = "std")]
#[test]
fn as_ref_census_roster_is_unchanged() {
  /// The pinned roster: one entry per physical call site.
  const ROSTER: &[(&str, usize, &str, &str)] = &[
    ("conformance/mod.rs", 1, "input", "emitter"),
    ("conformance/mod.rs", 2, "input", "emitter"),
    ("conformance/mod.rs", 3, "input", "emitter"),
    ("fuzz/consume.rs", 1, "input", "emitter"),
    ("fuzz/cst.rs", 1, "input", "sink"),
    ("fuzz/partial.rs", 1, "input", "emitter"),
    ("fuzz/partial.rs", 2, "input", "emitter"),
    ("fuzz/partial.rs", 3, "input", "emitter"),
    ("fuzz/partial.rs", 4, "input", "emitter"),
    ("fuzz/partial.rs", 5, "input", "emitter"),
    ("fuzz/partial.rs", 6, "input", "emitter"),
    ("fuzz/session.rs", 1, "input", "emitter"),
    ("fuzz/session.rs", 2, "input", "emitter"),
    ("input/mod.rs", 1, "input", "emitter"),
    ("input/session.rs", 1, "input", "emitter"),
    ("parser/mod.rs", 1, "input", "emitter"),
  ];

  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
  let mut files: std::vec::Vec<std::path::PathBuf> = std::vec::Vec::new();
  let mut stack = std::vec![root.clone()];
  while let Some(dir) = stack.pop() {
    for entry in std::fs::read_dir(&dir).expect("the crate's own src tree is readable") {
      let path = entry.expect("a readable directory entry").path();
      let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .expect("a UTF-8 path component")
        .to_owned();
      if path.is_dir() {
        if name != "tests" {
          stack.push(path);
        }
      } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
        files.push(path);
      }
    }
  }
  files.sort();

  let mut got: std::vec::Vec<Site> = std::vec::Vec::new();
  for path in &files {
    let text = std::fs::read_to_string(path).expect("a readable source file");
    let rel = path
      .strip_prefix(&root)
      .expect("every scanned file is under src")
      .to_str()
      .expect("a UTF-8 relative path")
      .replace('\\', "/");
    got.extend(extract_sites(&rel, &text));
  }
  got.sort();

  // Two positive controls, because a rail that stops looking passes trivially. The first catches a
  // broken walk, the second a broken match. The UFCS pattern has its own cell above, since the
  // crate contains no instance of that shape for these to observe.
  assert!(
    files.len() >= 100,
    "AS_REF_CENSUS: the walker found only {} source file(s) under `src`, so it stopped walking \
     rather than the crate shrinking. Fix the walker before trusting the roster.",
    files.len()
  );
  assert!(
    got.len() >= 8,
    "AS_REF_CENSUS: the matcher found only {} call site(s), so it stopped matching rather than the \
     crate stopping calling `as_ref`. Re-read the shapes it matches.",
    got.len()
  );

  let want: std::vec::Vec<Site> = ROSTER
    .iter()
    .map(|(f, n, r, e)| {
      (
        std::string::String::from(*f),
        *n,
        std::string::String::from(*r),
        std::string::String::from(*e),
      )
    })
    .collect();

  if got != want {
    let mut report = std::string::String::from(
      "AS_REF_CENSUS drift: the roster of production `Input::as_ref` call sites changed.\n\n",
    );
    for row in &want {
      if !got.contains(row) {
        report.push_str(&std::format!("  REMOVED  {row:?}\n"));
      }
    }
    for row in &got {
      if !want.contains(row) {
        report.push_str(&std::format!("  ADDED    {row:?}\n"));
      }
    }
    report.push_str(
      "\nThis is not automatically a defect — but `front_reported_end` and `emitted_error_end` \
       are suppression watermarks describing ONE emitter's log, and this call chooses the emitter. \
       If a new or moved site pairs an `Input` that may carry a watermark with a DIFFERENT emitter \
       than the one the watermark was set under, the watermark claims a report that log never \
       received, and because these watermarks suppress, the diagnostic is silently dropped.\n\nSo: \
       confirm the emitter is the same log, or clear the watermark at the new site, then update the \
       roster in the same commit. Read `Input::front_reported_end` first.",
    );
    panic!("{report}");
  }
}
