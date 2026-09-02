use super::*;

/// The tree, read once per test.
fn tree() -> (Vec<(String, String)>, BTreeMap<String, Vec<String>>) {
  let files = source_files();
  let traits = declared_traits(&files);
  (files, traits)
}

/// Runs the census over `files` and returns the refusal it must raise.
fn red(files: &[(String, String)], traits: &BTreeMap<String, Vec<String>>) -> String {
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| census(files, traits)));
  let payload = caught.expect_err("the census must red on this tree");
  payload
    .downcast_ref::<String>()
    .cloned()
    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
    .expect("a census refusal carries its message")
}

/// Rewrites `file`'s text, refusing a patch that does not apply: a plant that changed nothing
/// would red for the wrong reason, or pass for one.
fn patch(files: &[(String, String)], file: &str, from: &str, to: &str) -> Vec<(String, String)> {
  let mut out = files.to_vec();
  let entry = out
    .iter_mut()
    .find(|(name, _)| name == file)
    .unwrap_or_else(|| panic!("no `{file}` in the tree"));
  assert_eq!(
    entry.1.matches(from).count(),
    1,
    "the plant must have exactly one site in `{file}`:\n  {from}"
  );
  entry.1 = entry.1.replace(from, to);
  out
}

/// API_DIGEST_CENSUS — the shipped chapters' annotated digests quote the items they name.
///
/// The two counts are the check on the check. A filtered run that matches nothing exits `ok`, and
/// so does a census whose readers found no digest at all; pinning both numbers means a run that
/// covered less than it claims reds instead of passing quietly.
#[test]
#[cfg_attr(
  miri,
  ignore = "not a Miri subject: no UB for Miri to find; see the module doc"
)]
fn the_guide_digests_agree_with_the_items_they_quote() {
  let (files, traits) = tree();
  let (checked, discovered) = census(&files, &traits);
  assert_eq!(
    checked, 5,
    "the census checks five annotated digests: `Lexer` in two chapters, `Emitter` in two, and \
     `Syntax`. Update the count in the same commit as the annotation that moved it."
  );
  assert_eq!(
    discovered, 20,
    "structural discovery finds 20 fences that re-declare a `pub trait` and are compiled by \
     nothing. Four of the five annotated digests are among them — `arch_parsing_engine.md`'s \
     quotes three members of `Lexer` without its header, which no structural rule can attribute \
     — and the remaining sixteen are in `PENDING`. A change to this number is a digest arriving \
     or leaving."
  );
}

/// API_DIGEST_CENSUS — the census reds on each drift the parent branch fixed by hand.
///
/// Every plant is a **historical** defect, reverted in place, one error at a time. A synthetic
/// mutation proves the mutation; these prove the census would have caught what the tree actually
/// shipped. The control on the control is the first line: the unmutated tree passes, so a red
/// below is the plant and not the harness.
#[test]
#[cfg_attr(
  miri,
  ignore = "not a Miri subject: no UB for Miri to find; see the module doc"
)]
fn the_census_reds_on_the_drift_it_claims() {
  let (files, traits) = tree();
  census(&files, &traits);

  // ── Plant 1 (6351204, half one): the return type hardcoded the `str`/`[u8]` offset. ────────
  let planted = patch(
    &files,
    "guide/arch_parsing_engine.md",
    "fn read_frontier(&self) -> ReadFrontier<Self::Offset>;",
    "fn read_frontier(&self) -> ReadFrontier<usize>;",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("guide/arch_parsing_engine.md:")
      && message.contains("`Lexer::read_frontier` returns `ReadFrontier<Self::Offset>`")
      && message.contains("the digest writes `ReadFrontier<usize>`"),
    "the return-type arm must name the chapter, the member and both spellings:\n{message}"
  );

  // The same error in the other chapter, which is where a reader building a lexer meets it.
  let planted = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "fn read_frontier(&self) -> ReadFrontier<Self::Offset>;",
    "fn read_frontier(&self) -> ReadFrontier<usize>;",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("guide/recipe_custom_lexer.md:")
      && message.contains("`Lexer::read_frontier` returns"),
    "the return-type arm must fire in the recipe chapter too:\n{message}"
  );

  // ── Plant 2 (6351204, half two): a body in a list of trait signatures asserts a default. ───
  let planted = patch(
    &files,
    "guide/arch_parsing_engine.md",
    "fn read_frontier(&self) -> ReadFrontier<Self::Offset>;",
    "fn read_frontier(&self) -> ReadFrontier<Self::Offset> { ReadFrontier::SpanEnd }",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("guide/arch_parsing_engine.md:")
      && message.contains("`Lexer::read_frontier` with a body, which asserts it is defaulted")
      && message.contains("declares it **required**"),
    "the defaultedness arm must say which half is wrong, on its own:\n{message}"
  );

  // And the line exactly as it shipped: both halves at once, both reported.
  let planted = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "fn read_frontier(&self) -> ReadFrontier<Self::Offset>;",
    "fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("2 finding(s)")
      && message.contains("asserts it is defaulted")
      && message.contains("the digest writes `ReadFrontier<usize>`"),
    "the line as it shipped carries two errors and must be reported as two:\n{message}"
  );

  // ── Plants 3 and 4 (6351204): `Syntax`'s associated-type bounds, dropped. ──────────────────
  for member in ["COMPONENTS", "REQUIRED"] {
    let real = if member == "COMPONENTS" {
      (
        "type COMPONENTS: ArrayLength + Debug + Eq + Hash;   // type-level count (typenum, via generic-arraydeque)",
        "type COMPONENTS: ArrayLength;   // type-level count (typenum, via generic-arraydeque)",
      )
    } else {
      (
        "type REQUIRED:   ArrayLength + Debug + Eq + Hash;   // type-level count of the required subset",
        "type REQUIRED:   ArrayLength;   // type-level count of the required subset",
      )
    };
    let planted = patch(&files, "guide/ref_types_syntax.md", real.0, real.1);
    let message = red(&planted, &traits);
    assert!(
      message.contains("guide/ref_types_syntax.md:")
        && message.contains(&format!("bounds `Syntax::{member}`"))
        && message.contains("\"Debug\"")
        && message.contains("\"Eq\"")
        && message.contains("\"Hash\""),
      "the bound arm must name the chapter, the member and every bound dropped:\n{message}"
    );
  }

  // ── The completeness half, on the pair that genuinely disagrees. ───────────────────────────
  //
  // `arch_atomic_emitter.md` names all fourteen members of `Emitter`; drop one and the census
  // must say which, because a digest that reads as the whole surface is the one a reader trusts
  // to be whole.
  let planted = patch(
    &files,
    "guide/arch_atomic_emitter.md",
    "    fn bound_source(&self) -> Option<SourceIdentity> { None }\n",
    "",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("guide/arch_atomic_emitter.md:")
      && message.contains("says it is complete and does not name [\"bound_source\"]"),
    "the completeness arm must name the member the digest dropped:\n{message}"
  );

  // ── And the refusals, which are what make it a gate rather than a survey. ──────────────────

  // A digest the census cannot parse is refused, never skipped.
  let planted = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "fn bump(&mut self, n: &Self::Offset);",
    "fn bump(&mut self, n)",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("annotated but will not parse as Rust"),
    "an unparseable digest must refuse:\n{message}"
  );

  // A member with no declaration behind it.
  let planted = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "fn into_state(self) -> Self::State;",
    "fn into_state(self) -> Self::State;\n    fn rewind_to(&mut self, n: usize);",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("names `rewind_to`, which `trait Lexer`")
      && message.contains("does not declare"),
    "a member the trait does not declare must red:\n{message}"
  );

  // An alias that fires nowhere: a shorthand nothing uses is a claim about absent text.
  let planted = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "alias TokenError = ",
    "alias NotWritten = ",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("the digest never writes `NotWritten`"),
    "a dead alias must refuse:\n{message}"
  );

  // A clause the grammar does not have is refused rather than read as no opinion.
  let planted = patch(
    &files,
    "guide/ref_types_syntax.md",
    "<!-- api-digest: Syntax; complete -->",
    "<!-- api-digest: Syntax; complete; roughly -->",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("`roughly` is not a clause this census has"),
    "an unknown clause must refuse:\n{message}"
  );

  // A digest that arrives with no annotation and no `PENDING` entry.
  let planted = patch(
    &files,
    "guide/ref_types_syntax.md",
    "```text\ntrait Language: Sized",
    "```text\ntrait ParseContext {\n}\ntrait Language: Sized",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("re-declare a public trait in a fence nothing compiles")
      && message.contains("ParseContext"),
    "an unannotated digest must red rather than be quietly uncovered:\n{message}"
  );

  // A `PENDING` entry matching nothing hides the next digest that would have matched it.
  let planted = patch(
    &files,
    "guide/ref_pratt.md",
    "trait PrattPower",
    "trait __GonePrattPower",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("are listed in `PENDING` and no such digest was found")
      && message.contains("PrattPower"),
    "a stale pending entry must red:\n{message}"
  );

  // An annotated fence the compiler builds: the doctest would gate a shadow copy, and the
  // annotation would read as protection nothing provides.
  let planted = patch(
    &files,
    "guide/ref_types_syntax.md",
    "<!-- api-digest: Syntax; complete -->\n```text",
    "<!-- api-digest: Syntax; complete -->\n```rust",
  );
  let message = red(&planted, &traits);
  assert!(
    message.contains("this fence is annotated and the compiler builds it"),
    "an annotated compiled fence must refuse:\n{message}"
  );
}

/// API_DIGEST_CENSUS — the negative control: the census is indifferent to how a digest is
/// *written*, and reds only on what it *says*.
///
/// A check that fights the house style makes the documentation worse, so each rewrite below
/// changes the text without changing the claim, and every one must stay green. A plant that reds
/// for the wrong reason is worse than no plant; these are what tell the two apart.
#[test]
#[cfg_attr(
  miri,
  ignore = "not a Miri subject: no UB for Miri to find; see the module doc"
)]
fn the_census_is_green_on_a_digest_rewritten_without_changing_its_meaning() {
  let (files, traits) = tree();

  // Re-aligned: the column padding the chapter uses to make a signature list scannable.
  let rewrapped = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "    fn span(&self)  -> Self::Span;                 // the current token's span",
    "    fn span(&self) -> Self::Span;   // the current token's span",
  );
  census(&rewrapped, &traits);

  // A comment added to a line that had none, and one taken away.
  let recommented = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "    fn into_state(self) -> Self::State;",
    "    fn into_state(self) -> Self::State;   // consumes the lexer, keeps the mode",
  );
  census(&recommented, &traits);
  let uncommented = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "    fn bump(&mut self, n: &Self::Offset);",
    "    fn bump(&mut self, n: &Self::Offset);\n",
  );
  census(&uncommented, &traits);

  // Reordered: the census does not read declaration order, because prose order should serve the
  // reader. `state`/`state_mut`/`into_state` swapped for `into_state`/`state`/`state_mut`.
  let reordered = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "    fn state(&self) -> &Self::State;\n    fn state_mut(&mut self) -> &mut Self::State;\n    fn into_state(self) -> Self::State;",
    "    fn into_state(self) -> Self::State;\n    fn state(&self) -> &Self::State;\n    fn state_mut(&mut self) -> &mut Self::State;",
  );
  census(&reordered, &traits);

  // A parameter renamed: a digest names arguments for the reader, and the trait's own spelling
  // is not a claim the chapter has to reproduce.
  let renamed = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "fn with_state(src: &'inp Self::Source, state: Self::State) -> Self;",
    "fn with_state(source: &'inp Self::Source, resume_from: Self::State) -> Self;",
  );
  census(&renamed, &traits);

  // A type written by its full in-scope path where the digest had the short one.
  let requalified = patch(
    &files,
    "guide/ref_types_syntax.md",
    "type Component: Display + Debug + Clone + PartialEq + Eq + Hash;",
    "type Component: core::fmt::Display + core::fmt::Debug + Clone + PartialEq + Eq + core::hash::Hash;",
  );
  census(&requalified, &traits);

  // A bound list closed out to the real one where the digest had left it open with `…`.
  let spelled_out = patch(
    &files,
    "guide/recipe_custom_lexer.md",
    "type Offset: …;",
    "type Offset: Default + Debug + Ord + Clone + Hash;",
  );
  census(&spelled_out, &traits);
}

/// API_DIGEST_CENSUS — the readers themselves, over inputs that are wrong in each way a fence can
/// be wrong. The real chapters cannot exercise a fence that does not exist in them.
#[test]
#[cfg_attr(
  miri,
  ignore = "not a Miri subject: no UB for Miri to find; see the module doc"
)]
fn the_fence_reader_sees_what_the_renderers_see() {
  // Indented under a list item, and behind a doc-comment prefix: both were missed on this exact
  // file set before, and a reader anchored to the start of the line sees neither.
  let indented = "- a bullet, and under it:\n\n  ```text\n  trait Indented {}\n  ```\n";
  let found = fences("x.md", indented);
  assert_eq!(found.len(), 1, "an indented fence is a fence");
  assert_eq!(found[0].body, ["trait Indented {}"]);

  let in_doc = "/// prose\n///\n/// ```text\n/// trait InDoc {}\n/// ```\npub struct S;\n";
  let found = fences("x.rs", in_doc);
  assert_eq!(found.len(), 1, "a fence behind `/// ` is a fence");
  assert_eq!(found[0].body, ["trait InDoc {}"]);

  // The annotation reaches across blank lines and no further: a paragraph between the comment
  // and the fence means the comment belongs to something else.
  let attached = "<!-- api-digest: A -->\n\n```text\ntrait A {}\n```\n";
  assert!(fences("x.md", attached)[0].annotation.is_some());
  let detached = "<!-- api-digest: A -->\n\nprose\n\n```text\ntrait A {}\n```\n";
  assert!(fences("x.md", detached)[0].annotation.is_none());

  // An opener with no closer is a line that looks like a fence, not a fence.
  assert!(fences("x.md", "```text\ntrait A {}\n").is_empty());

  // What the compiler builds, and what it does not.
  let compiled =
    |info: &str| fences("x.md", &format!("```{info}\ntrait A {{}}\n```\n"))[0].compiled();
  assert!(compiled("rust"));
  assert!(compiled("rust,compile_fail,E0277"));
  assert!(!compiled("text"));
  assert!(!compiled("rust,ignore"));
  assert!(!compiled("ignore"));
}

/// API_DIGEST_CENSUS — the spelling normalizer tolerates one equivalence and nothing else.
#[test]
#[cfg_attr(
  miri,
  ignore = "not a Miri subject: no UB for Miri to find; see the module doc"
)]
fn a_type_spelling_drops_only_its_module_path() {
  let spelling = |src: &str| spell(&syn::parse_str::<syn::Type>(src).expect("a type"));

  assert_eq!(
    spelling("super::Source<Self::Offset>"),
    spelling("Source<Self::Offset>")
  );
  assert_eq!(spelling("core::fmt::Debug"), spelling("Debug"));
  assert_eq!(
    spelling("crate::error::ErrorNode<S>"),
    spelling("ErrorNode<S>")
  );
  // `Self` is not a module, so an associated type keeps its qualifier.
  assert_ne!(spelling("Self::Offset"), spelling("Offset"));
  assert_ne!(
    spelling("ReadFrontier<usize>"),
    spelling("ReadFrontier<Self::Offset>")
  );
  // A generic argument is not a path prefix.
  assert_eq!(spelling("Vec<u8>"), spelling("Vec<u8>"));
  assert_ne!(spelling("Vec<u8>"), spelling("Vec<u16>"));
}
