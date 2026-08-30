//! API_DIGEST_CENSUS — every guide fence that re-declares a public item, read against the item.
//!
//! The guide quotes tokora's public surface in *digests*: a fenced block that re-types a trait's
//! members so a chapter can talk about the shape without sending the reader to rustdoc. A digest
//! is prose, and prose is uncompiled. `270cf01` ruled out `ignore` (outside every gate), a
//! compiled doctest gates a *shadow copy* and says nothing about the real item, and `text` is
//! honest and silent. Every one of the four drifts fixed in `6351204` lived in exactly that gap:
//!
//! * `Lexer::read_frontier` written `-> ReadFrontier<usize>` where the trait says `Self::Offset`,
//!   in two chapters;
//! * the same line written **with a body**, which asserts a default the trait does not have —
//!   the half that costs somebody writing a custom lexer;
//! * `Syntax::{COMPONENTS, REQUIRED}` written `: ArrayLength` where both carry
//!   `ArrayLength + Debug + Eq + Hash`.
//!
//! `GUIDE_EVENT_CENSUS` (`cst/event.rs`) is the shape that closes this class, and this module is
//! that shape generalised. The generalisation is one change: **the coupling moves out of Rust and
//! into the chapter.** The event census hard-codes chapter, heading spelling and enum name in
//! Rust, which is right for one enumeration and is 45 copies of itself for 45 digests. Here each
//! digest names its own subject in an HTML comment above the fence — invisible in rustdoc and in
//! the mdbook build — and this file is the one reader.
//!
//! ```text
//! <!-- api-digest: Lexer; complete; alias TokenError = <Self::Token as Token<'inp>>::Error -->
//! ```
//!
//! # What the census asserts
//!
//! For every annotated digest, at the strongest level the digest's own text supports:
//!
//! 1. **Membership** — every member the digest names is declared by the item. With `complete`,
//!    every member the item declares is named by the digest.
//! 2. **Required versus defaulted** — a member written with a body asserts a default; one written
//!    `;` asserts none. Both directions are checked.
//! 3. **Signature identity, modulo declared elision** — receiver, parameter types positionally,
//!    return type, associated-type bounds, associated-const type.
//!
//! # What makes an elision legible rather than tolerated
//!
//! A digest elides on purpose and the census must not fight that. Four elisions are **declared**,
//! meaning the census reads a mark in the text and narrows what it checks at exactly that spot:
//!
//! | written | means | census |
//! |---|---|---|
//! | `…` in a bound list | the tail of the bounds is not shown | shown bounds must be a **subset** |
//! | `..` in a parameter list or type argument | that position is not shown | that type is not compared |
//! | `_` as a type | that type is not shown | that type is not compared |
//! | `alias X = <ty>` in the annotation | `X` is the digest's shorthand for `<ty>` | `X` is substituted before parsing |
//!
//! and two equivalences are **tolerated**, each a well-defined one rather than slack:
//!
//! * a method's generics and `where` clause are never compared, because dropping
//!   `where L: Lexer<'inp>` is the house style and it is legible;
//! * a leading lowercase path segment is dropped from every type spelling, so `super::Source`,
//!   `core::fmt::Debug` and `crate::error::ErrorNode` compare equal to `Source`, `Debug` and
//!   `ErrorNode` — the same item reached by a different in-scope path. The residue this leaves is
//!   real and worth naming: the census cannot tell `a::Foo` from `b::Foo`.
//!
//! Anything else the census cannot read is **refused**, never skipped — a fence whose body will
//! not parse, an alias that never fires, a member with no declaration behind it, a clause the
//! annotation grammar does not have. A census that silently covers less than it claims is the
//! defect class this module exists for.
//!
//! `names-only` is the one downgrade, for a digest written as prose rather than as Rust
//! (`fn rewind(.., u64);`, `fn enter_label / exit_label`). It keeps level 1 in both directions —
//! which is what catches a missing member — and gives up 2 and 3. It is a word in the tree, so it
//! shows in a diff, and the census still refuses any line of such a digest it cannot read.
//!
//! # Coverage, and what it does not reach
//!
//! The population is derived, not remembered: the census walks `src/`, finds every non-compiled
//! fence whose body re-declares a name the crate declares as a `pub trait`, and requires each to
//! be either annotated or named in `PENDING`. A digest added to the guide with neither reds.
//!
//! That keys on a `trait <Name>` line, and that is the whole of the guarantee. Three shapes sit
//! outside it, each derived from the tree rather than recalled, and each is a place a digest can
//! arrive without the coverage assertion noticing:
//!
//! * **A headless excerpt** — a fence quoting members without their header, the way
//!   `arch_parsing_engine.md` quotes three of `Lexer`'s. Four exist. Nothing in such a fence says
//!   which item it belongs to, so an annotation is the only thing that can attribute it, and one
//!   arriving unannotated is invisible here.
//! * **A struct or enum digest** — nine in the guide (`Checkpoint`, `InputRef`, `ParserContext`,
//!   `Recoverable`, the three Pratt step enums, `PrattFoldOp`, `Expected`). The comparison is the
//!   same shape over fields and variants instead of trait members, and is not written. The name
//!   test is weaker there too: `struct Parser` and `enum Expected` occur in *example* code as
//!   well, so a discovery rule keyed on struct names would flag three fences that re-declare
//!   nothing of the crate's.
//! * **A callback-shape digest** — `ch05_pratt.md` and `ref_pratt.md` twice quote the shapes of
//!   the `fold_prefix` / `fold_infix` / `fold_postfix` arguments to `InputRef::pratt`. Those are
//!   bounds on generic parameters rather than members of any item, so there is nothing to resolve
//!   them against; no level of this census reaches them.
//!
//! # Where it runs
//!
//! Gated on `feature = "std"`, and deliberately **not** on the guide's own
//! `all(std, logos_0_16, combinators)`. The census reads files, so it needs `std::fs`; it needs
//! nothing else, and only `std` gates whether `std::fs` exists, since `extern crate alloc as std`
//! is what `std` names in a no-std build. Wearing the guide's cfg would run it under
//! `--all-features` alone — one leg of the four the doctest matrix drives, and one of the two CI
//! runs tests on. As written it runs in all four (`rowan` implies `std`, so even the
//! `logos,rowan` leg carries it) and in both CI test legs.
//!
//! A leg with `std` off skips it silently, which is why
//! `the_guide_digests_agree_with_the_items_they_quote` pins the number of digests checked and the
//! number discovered: a filtered run that matches nothing exits `ok`, and so does a census whose
//! readers found nothing at all.

use std::{
  collections::{BTreeMap, BTreeSet},
  fs,
  path::{Path, PathBuf},
};

use quote::ToTokens;

mod tests;

/// The marker opening a digest annotation. One spelling; anything else is not an annotation.
const MARK: &str = "<!-- api-digest:";

/// The identifier the elision marks (`…`, `...`, `..`) are rewritten to before parsing, so a
/// digest with an elision in it is still Rust that `syn` will read. A type whose spelling
/// contains it is a position the digest declined to show, and is not compared.
const ELIDED: &str = "__Elided";

/// Digests the census knows about and does not yet check. An entry is bookkeeping, not a
/// clearance: it records that a fence exists and carries no annotation, and nothing about whether
/// its text is right. A discovered digest in neither this list nor an annotation reds, and an
/// entry here that matches no discovered digest reds too.
const PENDING: &[(&str, &str)] = &[
  ("guide/arch_atomic_emitter.md", "ComposableEmitter"),
  ("guide/arch_parsing_engine.md", "ParseInput"),
  ("guide/arch_source_slice.md", "Slice"),
  ("guide/arch_source_slice.md", "Source"),
  ("guide/ref_errors_emitters_context.md", "ComposableEmitter"),
  (
    "guide/ref_errors_emitters_context.md",
    "ComposableParseContext",
  ),
  ("guide/ref_errors_emitters_context.md", "MaybeIncomplete"),
  ("guide/ref_errors_emitters_context.md", "ParseContext"),
  ("guide/ref_pratt.md", "PrattEmitter"),
  ("guide/ref_pratt.md", "PrattPower"),
  ("guide/ref_pratt.md", "PrattToken"),
  ("guide/ref_types_syntax.md", "AstNode"),
  ("guide/ref_types_syntax.md", "ErrorNode"),
  ("guide/ref_types_syntax.md", "Language"),
  ("guide/ref_types_syntax.md", "Span"),
  ("guide/ref_vocabulary_macros_features.md", "Delimiter"),
];

/// `tokora/src`, the root of everything the census reads.
fn src_root() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.md` and `.rs` under `src/`, path-relative to it, sorted so a failure is reproducible.
fn source_files() -> Vec<(String, String)> {
  fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
      .unwrap_or_else(|e| panic!("API_DIGEST_CENSUS: cannot read `{}`: {e}", dir.display()))
      .map(|e| e.expect("a directory entry").path())
      .collect();
    entries.sort();
    for path in entries {
      if path.is_dir() {
        walk(&path, root, out);
      } else if matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md") | Some("rs")
      ) {
        let rel = path
          .strip_prefix(root)
          .expect("a walked path is under the root")
          .to_string_lossy()
          .replace('\\', "/");
        let text = fs::read_to_string(&path)
          .unwrap_or_else(|e| panic!("API_DIGEST_CENSUS: cannot read `{rel}`: {e}"));
        out.push((rel, text));
      }
    }
  }

  let root = src_root();
  let mut out = Vec::new();
  walk(&root, &root, &mut out);
  assert!(
    !out.is_empty(),
    "API_DIGEST_CENSUS: no sources under `{}` — the census would read nothing, which must \
     never pass as agreement",
    root.display()
  );
  out
}

// ── fences ────────────────────────────────────────────────────────────────────────────────────

/// One fenced block, wherever it lives: a chapter's markdown, or a `///` / `//!` doc comment.
#[derive(Debug)]
struct Fence {
  /// The file, relative to `src/`.
  file: String,
  /// The 1-based line of the fence's opener.
  line: usize,
  /// The fence's info string, trimmed.
  info: String,
  /// The lines between the fences, doc-comment prefix already stripped.
  body: Vec<String>,
  /// The `<!-- api-digest: … -->` line above it, if any.
  annotation: Option<String>,
}

impl Fence {
  /// `file:line`, the way every message locates a digest.
  fn at(&self) -> String {
    format!("{}:{}", self.file, self.line)
  }

  /// Whether the compiler ever sees this fence. A plain `rust` fence is a doctest and the
  /// compiler is its census; `ignore` and `text` are the fences nothing reads.
  fn compiled(&self) -> bool {
    let tags: Vec<&str> = self.info.split(',').map(str::trim).collect();
    let non_rust = ["text", "sh", "toml", "json", "console", "diff"];
    if tags.iter().any(|t| non_rust.contains(t)) {
      return false;
    }
    !tags.contains(&"ignore")
  }
}

/// Strips a line's `///` / `//!` prefix, so a fence inside a doc comment reads like a fence in a
/// chapter. A markdown line is returned unchanged.
fn undoc(line: &str) -> &str {
  let trimmed = line.trim_start();
  for open in ["///", "//!"] {
    if let Some(rest) = trimmed.strip_prefix(open) {
      return rest.strip_prefix(' ').unwrap_or(rest);
    }
  }
  line
}

/// The fence opener a line carries, as `(fence characters, info string)`.
///
/// Anchored to the *content* of the line rather than to `^`, because a fence indents under a list
/// item and hides behind a doc-comment prefix, and a pattern anchored to the start of the line is
/// blind to both.
fn fence_marker(line: &str) -> Option<(String, String)> {
  let content = undoc(line).trim_start();
  let ch = content.chars().next()?;
  if ch != '`' && ch != '~' {
    return None;
  }
  let ticks: String = content.chars().take_while(|c| *c == ch).collect();
  if ticks.len() < 3 {
    return None;
  }
  let info = content[ticks.len()..].trim().to_string();
  // A backtick-fenced block's info string may not contain a backtick.
  if ch == '`' && info.contains('`') {
    return None;
  }
  Some((ticks, info))
}

/// Every fence in `text`, with the `<!-- api-digest: … -->` annotation each carries, if any.
fn fences(file: &str, text: &str) -> Vec<Fence> {
  let lines: Vec<&str> = text.lines().collect();
  let mut out = Vec::new();
  let mut i = 0;
  while i < lines.len() {
    let Some((ticks, info)) = fence_marker(lines[i]) else {
      i += 1;
      continue;
    };
    let indent = {
      let content = undoc(lines[i]);
      content.len() - content.trim_start().len()
    };
    let mut body = Vec::new();
    let mut close = None;
    for (offset, line) in lines[i + 1..].iter().enumerate() {
      match fence_marker(line) {
        Some((c, rest))
          if c.starts_with(&ticks[..1]) && c.len() >= ticks.len() && rest.is_empty() =>
        {
          close = Some(i + 1 + offset);
          break;
        }
        _ => {
          let content = undoc(line);
          let strip = content
            .char_indices()
            .take_while(|(offset, c)| *offset < indent && *c == ' ')
            .count();
          body.push(content[strip..].to_string());
        }
      }
    }
    let Some(close) = close else {
      // An unterminated opener is not a fence; it is a line that looks like one.
      i += 1;
      continue;
    };

    // The annotation is the nearest `<!-- api-digest: … -->` above the opener, across blank
    // lines only: a paragraph between them means the comment belongs to something else.
    let mut annotation = None;
    let mut probe = i;
    while probe > 0 {
      probe -= 1;
      let line = undoc(lines[probe]).trim();
      if line.is_empty() {
        continue;
      }
      if line.starts_with(MARK) {
        annotation = Some(line.to_string());
      }
      break;
    }

    out.push(Fence {
      file: file.to_string(),
      line: i + 1,
      info,
      body,
      annotation,
    });
    i = close + 1;
  }
  out
}

// ── the annotation ────────────────────────────────────────────────────────────────────────────

/// What a digest says about itself.
#[derive(Debug)]
struct Annotation {
  /// The item the digest re-declares.
  subject: String,
  /// A `src`-relative file, when the subject's name alone is ambiguous.
  file_hint: Option<String>,
  /// Whether the digest claims to name every member the item declares.
  complete: bool,
  /// Whether the digest is prose rather than Rust, and so gets level 1 only.
  names_only: bool,
  /// The digest's shorthands, longest first so a substitution cannot eat a prefix of another.
  aliases: Vec<(String, String)>,
}

/// Reads one annotation, refusing every clause the grammar does not have.
fn parse_annotation(at: &str, raw: &str) -> Annotation {
  let inner = raw
    .strip_prefix(MARK)
    .and_then(|rest| rest.strip_suffix("-->"))
    .unwrap_or_else(|| {
      panic!("API_DIGEST_CENSUS: {at}: an annotation must close with `-->`:\n  {raw}")
    })
    .trim();

  let mut clauses = inner.split(';').map(str::trim).filter(|c| !c.is_empty());
  let head = clauses.next().unwrap_or_else(|| {
    panic!("API_DIGEST_CENSUS: {at}: an annotation must name the item it digests:\n  {raw}")
  });

  let (subject, file_hint) = match head.split_once('@') {
    Some((name, file)) => (name.trim().to_string(), Some(file.trim().to_string())),
    None => (head.to_string(), None),
  };
  assert!(
    !subject.is_empty()
      && subject
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_'),
    "API_DIGEST_CENSUS: {at}: `{subject}` is not an item name. Write \
     `{MARK} Lexer -->`, or `{MARK} Token @ token/mod.rs -->` when the name is ambiguous."
  );

  let mut annotation = Annotation {
    subject,
    file_hint,
    complete: false,
    names_only: false,
    aliases: Vec::new(),
  };
  for clause in clauses {
    if clause == "complete" {
      annotation.complete = true;
    } else if clause == "names-only" {
      annotation.names_only = true;
    } else if let Some(rest) = clause.strip_prefix("alias ") {
      let (lhs, rhs) = rest.split_once('=').unwrap_or_else(|| {
        panic!("API_DIGEST_CENSUS: {at}: an `alias` clause reads `alias X = <type>`:\n  {clause}")
      });
      let (lhs, rhs) = (lhs.trim().to_string(), rhs.trim().to_string());
      assert!(
        !lhs.is_empty() && !rhs.is_empty(),
        "API_DIGEST_CENSUS: {at}: an `alias` clause needs both sides:\n  {clause}"
      );
      annotation.aliases.push((lhs, rhs));
    } else {
      panic!(
        "API_DIGEST_CENSUS: {at}: `{clause}` is not a clause this census has. The grammar is \
         `{MARK} <Item>[ @ <file>][; complete][; names-only][; alias X = <type>]* -->`. Add the \
         clause to `parse_annotation` in the same commit as the digest that needs it — a clause \
         read as no opinion is how a census comes to cover less than it says."
      );
    }
  }
  // Longest first: `alias SliceOf<'inp, Self>` must not be shadowed by `alias SliceOf`.
  annotation
    .aliases
    .sort_by_key(|alias| std::cmp::Reverse(alias.0.len()));
  annotation
}

// ── the declared item ─────────────────────────────────────────────────────────────────────────

/// Every `pub trait` name the crate declares, and the files declaring it.
fn declared_traits(files: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
  let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
  for (file, text) in files {
    if !file.ends_with(".rs") {
      continue;
    }
    for line in text.lines() {
      let trimmed = line.trim_start();
      if trimmed.starts_with("//") {
        continue;
      }
      let Some(rest) = trimmed.strip_prefix("pub ") else {
        continue;
      };
      // `pub(crate) trait …` is not public surface the guide can quote.
      let Some(rest) = rest.strip_prefix("trait ") else {
        continue;
      };
      let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
      if !name.is_empty() {
        out.entry(name).or_default().push(file.clone());
      }
    }
  }
  out
}

/// Walks `items` and every nested `mod`, handing each trait to `visit`.
fn each_trait(items: &[syn::Item], visit: &mut impl FnMut(&syn::ItemTrait)) {
  for item in items {
    match item {
      syn::Item::Trait(t) => visit(t),
      syn::Item::Mod(m) => {
        if let Some((_, inner)) = &m.content {
          each_trait(inner, visit);
        }
      }
      _ => {}
    }
  }
}

/// The trait `name` declares, parsed from the crate's own source through `syn`.
///
/// Refuses on absence and on ambiguity rather than picking one: `Token` is two traits in this
/// crate, and a census that guessed between them would pin the wrong surface silently.
fn declared_trait(
  at: &str,
  files: &[(String, String)],
  name: &str,
  file_hint: Option<&str>,
) -> (String, syn::ItemTrait) {
  let needle = format!("trait {name}");
  let mut found: Vec<(String, syn::ItemTrait)> = Vec::new();
  for (file, text) in files {
    if !file.ends_with(".rs") || !text.contains(&needle) {
      continue;
    }
    if file_hint.is_some_and(|hint| file != hint) {
      continue;
    }
    let parsed = syn::parse_file(text).unwrap_or_else(|e| {
      panic!("API_DIGEST_CENSUS: {at}: `{file}` must parse for the census to read it: {e}")
    });
    each_trait(&parsed.items, &mut |t| {
      if t.ident == name {
        found.push((file.clone(), t.clone()));
      }
    });
  }

  assert!(
    !found.is_empty(),
    "API_DIGEST_CENSUS: {at}: no `trait {name}` in `src/`{}. The digest pins nothing, which \
     must never read as agreement — correct the annotation, or restore the item.",
    file_hint.map_or(String::new(), |h| format!(" under `{h}`"))
  );
  assert!(
    found.len() == 1,
    "API_DIGEST_CENSUS: {at}: `trait {name}` is declared {} times ({:?}); the census cannot \
     tell which one the digest quotes. Disambiguate with `{MARK} {name} @ <file> -->`.",
    found.len(),
    found.iter().map(|(f, _)| f).collect::<Vec<_>>()
  );
  found.pop().expect("exactly one")
}

// ── spelling ──────────────────────────────────────────────────────────────────────────────────

/// A type's comparison spelling: its tokens, one space apart, with leading lowercase path
/// segments dropped.
///
/// Dropping them is the one tolerated equivalence in the type comparison. `super::Source`,
/// `core::fmt::Debug` and `crate::error::ErrorNode` are the same items as `Source`, `Debug` and
/// `ErrorNode`, reached by a different in-scope path, and a guide that writes the short form is
/// not drifting. `Self::Offset` survives because `Self` is not lowercase.
fn spell(tokens: &impl ToTokens) -> String {
  let raw = tokens.to_token_stream().to_string();
  let mut words: Vec<String> = Vec::new();
  for word in raw.split_whitespace() {
    words.push(word.to_string());
  }
  // Drop `<lowercase-ident> ::` wherever it opens a path. Repeated because `a :: b :: Foo`
  // needs two passes.
  loop {
    let mut cut = None;
    for i in 0..words.len().saturating_sub(1) {
      let head = &words[i];
      let starts_lower = head
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
      let is_ident = head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !head.chars().next().is_some_and(|c| c.is_ascii_digit());
      let opens_path = words[i + 1] == "::";
      // Not after a `::`: `Self :: token :: X` would have to keep `token`.
      let after_sep = i > 0 && words[i - 1] == "::";
      if starts_lower && is_ident && opens_path && !after_sep && head != ELIDED {
        cut = Some(i);
        break;
      }
    }
    match cut {
      Some(i) => {
        words.drain(i..=i + 1);
      }
      None => break,
    }
  }
  join(&words)
}

/// Rejoins a token sequence the way somebody writes it, so a message quotes a type a reader can
/// find in the source rather than `ReadFrontier < Self :: Offset >`.
fn join(words: &[String]) -> String {
  const NO_SPACE_BEFORE: &[&str] = &[",", ";", ">", ")", "]", "::", "!"];
  const NO_SPACE_AFTER: &[&str] = &["<", "(", "[", "::", "&", "?", "#"];
  let mut out = String::new();
  for (index, word) in words.iter().enumerate() {
    // A `<` opening a generic argument list hugs the name it qualifies (`Option<T>`); one
    // opening a qualified path does not (`Result<(), <Self::Token as Token<'a>>::Error>`).
    let previous = index
      .checked_sub(1)
      .map(|i| words[i].as_str())
      .unwrap_or("");
    let opens_arguments = word == "<"
      && (matches!(previous, ">" | "]" | ")")
        || previous
          .chars()
          .next()
          .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_'));
    let space = index > 0
      && !opens_arguments
      && !NO_SPACE_BEFORE.contains(&word.as_str())
      && !NO_SPACE_AFTER.contains(&previous);
    if space {
      out.push(' ');
    }
    out.push_str(word);
  }
  out
}

/// Whether a spelling names a position the digest declined to show.
fn is_elided(spelling: &str) -> bool {
  spelling == "_" || spelling.contains(ELIDED)
}

// ── reading a digest ──────────────────────────────────────────────────────────────────────────

/// The digest's body as Rust: aliases substituted, elision marks rewritten to [`ELIDED`], and
/// wrapped in a trait when the fence quotes members without their header.
fn digest_trait(at: &str, fence: &Fence, annotation: &Annotation) -> syn::ItemTrait {
  let mut body = fence.body.join("\n");

  for (lhs, rhs) in &annotation.aliases {
    assert!(
      body.contains(lhs.as_str()),
      "API_DIGEST_CENSUS: {at}: the annotation declares `alias {lhs} = {rhs}` and the digest \
       never writes `{lhs}`. A shorthand nothing uses is a claim about text that is not there; \
       drop the clause, or fix the spelling it was meant to cover."
    );
    body = body.replace(lhs.as_str(), rhs);
  }
  for mark in ["…", "...", ".."] {
    body = body.replace(mark, ELIDED);
  }

  let headed = body
    .lines()
    .map(str::trim_start)
    .find(|l| !l.is_empty() && !l.starts_with("//"))
    .is_some_and(|l| l.starts_with("trait ") || l.starts_with("pub trait "));
  let source = if headed {
    body
  } else {
    format!("trait __Digest {{\n{body}\n}}")
  };

  // A digest with two items in one fence (`trait Syntax { … } trait AstNode { … }`) parses as a
  // file; the census takes the one the annotation names.
  let parsed = syn::parse_file(&source).unwrap_or_else(|e| {
    panic!(
      "API_DIGEST_CENSUS: {at}: this digest is annotated but will not parse as Rust: {e}\n\
       Every position a digest declines to show has a mark the census reads — `…` for the tail \
       of a bound list, `..` for a parameter or type argument, `_` for a type, `alias X = <ty>` \
       in the annotation for a shorthand. If the fence is prose rather than Rust, say so with \
       `; names-only` and keep level 1. What must never happen is the fence being skipped.\n\
       --- as the census read it ---\n{source}"
    )
  });

  let mut candidates: Vec<syn::ItemTrait> = Vec::new();
  each_trait(&parsed.items, &mut |t| {
    if t.ident == annotation.subject || t.ident == "__Digest" {
      candidates.push(t.clone());
    }
  });
  assert!(
    candidates.len() == 1,
    "API_DIGEST_CENSUS: {at}: the fence carries {} declarations of `{}`; a digest quotes one \
     item, so split the fence or point the annotation at the other.",
    candidates.len(),
    annotation.subject
  );
  candidates.pop().expect("exactly one")
}

/// The member names a prose digest lists, read by refusing every line it cannot account for.
fn prose_members(at: &str, fence: &Fence) -> Vec<String> {
  let mut names = Vec::new();
  for raw in &fence.body {
    let line = match raw.split_once("//") {
      Some((code, _)) => code,
      None => raw,
    }
    .trim();
    if line.is_empty() || line == "}" || line == "{" {
      continue;
    }
    if line.starts_with("trait ") || line.starts_with("pub trait ") {
      continue;
    }
    let rest = [
      "fn ",
      "type ",
      "const ",
      "pub fn ",
      "pub type ",
      "pub const ",
    ]
    .iter()
    .find_map(|kw| line.strip_prefix(kw))
    .unwrap_or_else(|| {
      panic!(
        "API_DIGEST_CENSUS: {at}: a `names-only` digest carries a line the census cannot read. \
           Every line must be blank, a `//` comment, a brace, a `trait` header, or a member \
           opening `fn` / `type` / `const` — a line nothing parses is a member the census would \
           silently miss:\n  {raw}"
      )
    });
    // `fn enter_label / exit_label` names two members on one line.
    for part in rest.split('/') {
      let name: String = part
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
      if name.is_empty() {
        continue;
      }
      names.push(name);
      // Only the first segment of `fn f(a / b)` is a member; a `/` inside parentheses is not a
      // second name, so stop once the line has opened one.
      if part.contains('(') {
        break;
      }
    }
  }
  assert!(
    !names.is_empty(),
    "API_DIGEST_CENSUS: {at}: a `names-only` digest that names no member checks nothing"
  );
  names
}

// ── comparison ────────────────────────────────────────────────────────────────────────────────

/// One member of a trait, as the census compares them.
struct Member<'a> {
  name: String,
  item: &'a syn::TraitItem,
}

/// A trait's members by name, refusing a form the census has no comparison for.
fn members<'a>(at: &str, which: &str, item: &'a syn::ItemTrait) -> Vec<Member<'a>> {
  item
    .items
    .iter()
    .map(|entry| {
      let name = match entry {
        syn::TraitItem::Fn(f) => f.sig.ident.to_string(),
        syn::TraitItem::Type(t) => t.ident.to_string(),
        syn::TraitItem::Const(c) => c.ident.to_string(),
        other => panic!(
          "API_DIGEST_CENSUS: {at}: the {which} carries a trait member the census has no \
           comparison for ({}), so it would be skipped: {}",
          match other {
            syn::TraitItem::Macro(_) => "a macro invocation",
            syn::TraitItem::Verbatim(_) => "an unparsed token stream",
            _ => "an unrecognized form",
          },
          spell(other)
        ),
      };
      Member { name, item: entry }
    })
    .collect()
}

/// Whether a member carries a default: a body, or a `= …` on a type or const.
fn defaulted(item: &syn::TraitItem) -> bool {
  match item {
    syn::TraitItem::Fn(f) => f.default.is_some(),
    syn::TraitItem::Type(t) => t.default.is_some(),
    syn::TraitItem::Const(c) => c.default.is_some(),
    _ => false,
  }
}

/// A member's kind, for a message that says what changed.
fn kind(item: &syn::TraitItem) -> &'static str {
  match item {
    syn::TraitItem::Fn(_) => "fn",
    syn::TraitItem::Type(_) => "type",
    syn::TraitItem::Const(_) => "const",
    _ => "member",
  }
}

/// A bound list as a set of spellings, and whether the digest left it open with `…`.
fn bounds(
  list: &syn::punctuated::Punctuated<syn::TypeParamBound, syn::Token![+]>,
) -> (BTreeSet<String>, bool) {
  let mut set = BTreeSet::new();
  let mut open = false;
  for bound in list {
    let spelling = spell(bound);
    if is_elided(&spelling) {
      open = true;
    } else {
      set.insert(spelling);
    }
  }
  (set, open)
}

/// The receiver a method takes, reduced to what is load-bearing: `self` by value, by shared
/// reference, or by unique reference. Lifetimes are not compared, because a digest that writes
/// `&self` for `&'a self` is eliding legibly.
fn receiver(sig: &syn::Signature) -> &'static str {
  match sig.inputs.first() {
    Some(syn::FnArg::Receiver(r)) => match (&r.kind, r.mutability.is_some()) {
      (syn::ReceiverKind::Reference(_, _, Some(_)), _) => "&mut self",
      (syn::ReceiverKind::Reference(_, _, None), _) => "&self",
      (syn::ReceiverKind::Typed(..), _) => "self: <type>",
      (_, true) => "mut self",
      (_, false) => "self",
    },
    _ => "no receiver",
  }
}

/// The types a method takes after its receiver, in order.
fn parameters(sig: &syn::Signature) -> Vec<String> {
  sig
    .inputs
    .iter()
    .filter_map(|arg| match arg {
      syn::FnArg::Typed(t) => Some(spell(&*t.ty)),
      syn::FnArg::Receiver(_) => None,
    })
    .collect()
}

/// The return type's spelling; `()` for a method that returns nothing.
fn returns(sig: &syn::Signature) -> String {
  match &sig.output {
    syn::ReturnType::Default => "()".to_string(),
    syn::ReturnType::Type(_, ty) => spell(&**ty),
  }
}

/// The census: one digest against the item it quotes. Every disagreement is collected so a run
/// reports all of them, rather than one per edit.
fn compare(
  at: &str,
  annotation: &Annotation,
  declared_in: &str,
  declared: &syn::ItemTrait,
  digest: &syn::ItemTrait,
  drift: &mut Vec<String>,
) {
  let subject = &annotation.subject;
  let real = members(at, "declaration", declared);
  let quoted = members(at, "digest", digest);

  let real_by_name: BTreeMap<&str, &Member<'_>> =
    real.iter().map(|m| (m.name.as_str(), m)).collect();
  assert_eq!(
    real_by_name.len(),
    real.len(),
    "API_DIGEST_CENSUS: {at}: `trait {subject}` in `{declared_in}` declares a member twice; \
     the census cannot pair them"
  );

  // Level 1, digest → declaration.
  for member in &quoted {
    let Some(actual) = real_by_name.get(member.name.as_str()) else {
      drift.push(format!(
        "{at}: the digest of `{subject}` names `{}`, which `trait {subject}` in `{declared_in}` \
         does not declare. A reader implementing from this chapter writes a member the compiler \
         will reject. Drop it, or restore the item.",
        member.name
      ));
      continue;
    };

    if annotation.names_only {
      continue;
    }

    // Level 2.
    let (shown, is) = (defaulted(member.item), defaulted(actual.item));
    if shown != is {
      drift.push(if shown {
        format!(
          "{at}: the digest writes `{subject}::{}` with a body, which asserts it is defaulted. \
           `{declared_in}` declares it **required**, with no default. Somebody implementing from \
           this chapter would take the behavior for free and find it is theirs to write. Write \
           it `;`.",
          member.name
        )
      } else {
        format!(
          "{at}: the digest writes `{subject}::{}` with a `;`, which asserts it is required. \
           `{declared_in}` gives it a default. Show the body, so a reader knows what they \
           inherit by not overriding it.",
          member.name
        )
      });
    }

    // Level 3.
    match (member.item, actual.item) {
      (syn::TraitItem::Fn(shown), syn::TraitItem::Fn(real)) => {
        if receiver(&shown.sig) != receiver(&real.sig) {
          drift.push(format!(
            "{at}: `{subject}::{}` takes `{}` in `{declared_in}` and the digest writes `{}`.",
            member.name,
            receiver(&real.sig),
            receiver(&shown.sig)
          ));
        }
        let (shown_params, real_params) = (parameters(&shown.sig), parameters(&real.sig));
        if shown_params.len() != real_params.len() {
          drift.push(format!(
            "{at}: `{subject}::{}` takes {} parameter(s) in `{declared_in}` and the digest \
             writes {}: {real_params:?} versus {shown_params:?}.",
            member.name,
            real_params.len(),
            shown_params.len()
          ));
        } else {
          for (index, (shown_ty, real_ty)) in
            shown_params.iter().zip(real_params.iter()).enumerate()
          {
            if !is_elided(shown_ty) && shown_ty != real_ty {
              drift.push(format!(
                "{at}: `{subject}::{}` parameter {index} is `{real_ty}` in `{declared_in}` and \
                 the digest writes `{shown_ty}`.",
                member.name
              ));
            }
          }
        }
        let (shown_ret, real_ret) = (returns(&shown.sig), returns(&real.sig));
        if !is_elided(&shown_ret) && shown_ret != real_ret {
          drift.push(format!(
            "{at}: `{subject}::{}` returns `{real_ret}` in `{declared_in}` and the digest writes \
             `{shown_ret}`.",
            member.name
          ));
        }
      }
      (syn::TraitItem::Type(shown), syn::TraitItem::Type(real)) => {
        let (shown_bounds, open) = bounds(&shown.bounds);
        let (real_bounds, _) = bounds(&real.bounds);
        let missing: Vec<&String> = real_bounds.difference(&shown_bounds).collect();
        let invented: Vec<&String> = shown_bounds.difference(&real_bounds).collect();
        if !invented.is_empty() {
          drift.push(format!(
            "{at}: the digest bounds `{subject}::{}` by {invented:?}, which `{declared_in}` does \
             not require.",
            member.name
          ));
        }
        if !open && !missing.is_empty() {
          drift.push(format!(
            "{at}: the digest bounds `{subject}::{}` by {shown_bounds:?} and `{declared_in}` \
             declares {real_bounds:?} — {missing:?} is dropped with nothing saying so. Write the \
             bound, or end the list `+ …` to say the tail is not shown.",
            member.name
          ));
        }
      }
      (syn::TraitItem::Const(shown), syn::TraitItem::Const(real)) => {
        let (shown_ty, real_ty) = (spell(&shown.ty), spell(&real.ty));
        if !is_elided(&shown_ty) && shown_ty != real_ty {
          drift.push(format!(
            "{at}: `{subject}::{}` is `{real_ty}` in `{declared_in}` and the digest writes \
             `{shown_ty}`.",
            member.name
          ));
        }
      }
      (shown, real) => drift.push(format!(
        "{at}: the digest writes `{subject}::{}` as a `{}` and `{declared_in}` declares it a \
         `{}`.",
        member.name,
        kind(shown),
        kind(real)
      )),
    }
  }

  // Level 1, declaration → digest. Only a digest that claims completeness owes this.
  if annotation.complete {
    let quoted_names: BTreeSet<&str> = quoted.iter().map(|m| m.name.as_str()).collect();
    let missing: Vec<&str> = real
      .iter()
      .map(|m| m.name.as_str())
      .filter(|name| !quoted_names.contains(name))
      .collect();
    if !missing.is_empty() {
      drift.push(format!(
        "{at}: the digest of `{subject}` says it is complete and does not name {missing:?}, \
         which `{declared_in}` declares. A digest that reads as the whole surface teaches a model \
         with a hole in it. Add the member, or drop `complete` from the annotation.",
      ));
    }
    assert_eq!(
      quoted_names.len(),
      quoted.len(),
      "API_DIGEST_CENSUS: {at}: the digest of `{subject}` names a member twice"
    );
  }
}

/// Every annotated digest in the crate, checked; and every unannotated one, refused.
///
/// Returns `(checked, discovered)` so the callers can pin both counts: a census that checked
/// nothing must never read as agreement.
fn census(files: &[(String, String)], traits: &BTreeMap<String, Vec<String>>) -> (usize, usize) {
  let mut drift: Vec<String> = Vec::new();
  let mut checked = 0usize;
  let mut discovered: BTreeSet<(String, String)> = BTreeSet::new();
  let mut annotated: BTreeSet<(String, String)> = BTreeSet::new();

  for (file, text) in files {
    for fence in fences(file, text) {
      let at = fence.at();

      // Structural discovery: a fence nothing compiles that re-declares a name the crate
      // declares as a `pub trait` is a digest, whether or not anyone annotated it.
      if !fence.compiled() {
        for line in &fence.body {
          let line = line.trim_start();
          let Some(rest) = line
            .strip_prefix("pub trait ")
            .or_else(|| line.strip_prefix("trait "))
          else {
            continue;
          };
          let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
          if traits.contains_key(&name) {
            discovered.insert((file.clone(), name));
          }
        }
      }

      let Some(raw) = &fence.annotation else {
        continue;
      };
      let annotation = parse_annotation(&at, raw);
      annotated.insert((file.clone(), annotation.subject.clone()));
      assert!(
        !fence.compiled(),
        "API_DIGEST_CENSUS: {at}: this fence is annotated and the compiler builds it. A \
         compiled fence is a doctest, and the doctest gates a shadow copy of `{}` rather than \
         `{}` itself — the annotation would read as protection nothing provides. Drop the \
         annotation, or make the fence `text`.",
        annotation.subject,
        annotation.subject
      );

      let (declared_in, declared) = declared_trait(
        &at,
        files,
        &annotation.subject,
        annotation.file_hint.as_deref(),
      );

      if annotation.names_only {
        let names = prose_members(&at, &fence);
        let synthetic: syn::ItemTrait = syn::parse_str(&format!(
          "trait __Digest {{ {} }}",
          names
            .iter()
            .map(|n| format!("fn {n}();"))
            .collect::<Vec<_>>()
            .join(" ")
        ))
        .expect("member names synthesize a trait");
        compare(
          &at,
          &annotation,
          &declared_in,
          &declared,
          &synthetic,
          &mut drift,
        );
      } else {
        let digest = digest_trait(&at, &fence, &annotation);
        compare(
          &at,
          &annotation,
          &declared_in,
          &declared,
          &digest,
          &mut drift,
        );
      }
      checked += 1;
    }
  }

  let pending: BTreeSet<(String, String)> = PENDING
    .iter()
    .map(|(f, n)| ((*f).to_string(), (*n).to_string()))
    .collect();
  let uncovered: Vec<&(String, String)> = discovered
    .iter()
    .filter(|key| !annotated.contains(*key) && !pending.contains(*key))
    .collect();
  assert!(
    uncovered.is_empty(),
    "API_DIGEST_CENSUS: {uncovered:?} re-declare a public trait in a fence nothing compiles, \
     with no `{MARK} … -->` above them and no entry in `PENDING`. A digest outside the census is \
     a claim about the crate that nothing reads. Annotate it, or add it to `PENDING` in the same \
     commit (grep API_DIGEST_CENSUS)."
  );
  let stale: Vec<&(String, String)> = pending
    .iter()
    .filter(|key| !discovered.contains(*key))
    .collect();
  assert!(
    stale.is_empty(),
    "API_DIGEST_CENSUS: {stale:?} are listed in `PENDING` and no such digest was found. A \
     pending entry that matches nothing hides the next one that should have matched it — drop \
     the entry, or restore the digest."
  );

  assert!(
    drift.is_empty(),
    "API_DIGEST_CENSUS drift ({} finding(s)):\n\n{}\n",
    drift.len(),
    drift.join("\n\n")
  );

  (checked, discovered.len())
}
