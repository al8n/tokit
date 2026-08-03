use super::*;
use rowan::{GreenNodeBuilder, Language as _, SyntaxKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TK {
  Root,
  Ident,
  Plus,
  Str,
  BlockStr,
  Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TLang {}

impl Language for TLang {
  type Kind = TK;

  fn kind_from_raw(raw: SyntaxKind) -> TK {
    match raw.0 {
      0 => TK::Root,
      1 => TK::Ident,
      2 => TK::Plus,
      3 => TK::Str,
      4 => TK::BlockStr,
      5 => TK::Group,
      _ => panic!("unknown"),
    }
  }

  fn kind_to_raw(kind: TK) -> SyntaxKind {
    match kind {
      TK::Root => SyntaxKind(0),
      TK::Ident => SyntaxKind(1),
      TK::Plus => SyntaxKind(2),
      TK::Str => SyntaxKind(3),
      TK::BlockStr => SyntaxKind(4),
      TK::Group => SyntaxKind(5),
    }
  }
}

fn make_tree() -> SyntaxNode<TLang> {
  let mut builder = GreenNodeBuilder::new();
  builder.start_node(TLang::kind_to_raw(TK::Root));
  builder.token(TLang::kind_to_raw(TK::Ident), "x");
  builder.token(TLang::kind_to_raw(TK::Plus), "+");
  builder.finish_node();
  SyntaxNode::new_root(builder.finish())
}

#[test]
fn token_finds_by_kind() {
  let root = make_tree();
  let plus = token(&root, &TK::Plus);
  assert!(plus.is_some());
  assert_eq!(plus.unwrap().text(), "+");
}

#[test]
fn token_finds_ident() {
  let root = make_tree();
  let ident = token(&root, &TK::Ident);
  assert!(ident.is_some());
  assert_eq!(ident.unwrap().text(), "x");
}

#[test]
fn token_returns_none_when_not_found() {
  let mut builder = GreenNodeBuilder::new();
  builder.start_node(TLang::kind_to_raw(TK::Root));
  builder.token(TLang::kind_to_raw(TK::Ident), "x");
  builder.finish_node();
  let root = SyntaxNode::<TLang>::new_root(builder.finish());
  let plus = token(&root, &TK::Plus);
  assert!(plus.is_none());
}

// ── Navigation without the parser's component model ──────────────────────────
//
// `Ident` implements `CastNode` and *nothing else*: no `Element`, no `Node`, and above all no
// `Syntax` — so no `Component` enum, no `COMPONENTS`, no `REQUIRED`, no component functions.
// That is the entire reason the helpers are bound on `CastNode`, so it is asserted here rather
// than argued. If someone re-narrows the bound to `Node`, this module stops compiling.

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ident(SyntaxNode<TLang>);

impl CastNode<TLang> for Ident {
  fn cast_node(syntax: SyntaxNode<TLang>) -> Option<Self> {
    (syntax.kind() == TK::Ident).then_some(Self(syntax))
  }
}

/// `Root` over three child *nodes*: `Ident("x")`, `Plus("+")`, `Ident("y")`.
///
/// The interleaved `Plus` is what makes the filtering assertions meaningful.
fn make_node_tree() -> SyntaxNode<TLang> {
  let mut builder = GreenNodeBuilder::new();
  builder.start_node(TLang::kind_to_raw(TK::Root));
  for (kind, text) in [(TK::Ident, "x"), (TK::Plus, "+"), (TK::Ident, "y")] {
    builder.start_node(TLang::kind_to_raw(kind));
    builder.token(TLang::kind_to_raw(kind), text);
    builder.finish_node();
  }
  builder.finish_node();
  SyntaxNode::new_root(builder.finish())
}

#[test]
fn child_casts_a_node_that_only_implements_cast_node() {
  let root = make_node_tree();
  let first: Option<Ident> = child::<Ident, TLang>(&root);
  assert_eq!(first.map(|i| i.0.to_string()), Some("x".to_string()));
}

#[test]
fn child_returns_none_when_no_child_casts() {
  let mut builder = GreenNodeBuilder::new();
  builder.start_node(TLang::kind_to_raw(TK::Root));
  builder.start_node(TLang::kind_to_raw(TK::Plus));
  builder.token(TLang::kind_to_raw(TK::Plus), "+");
  builder.finish_node();
  builder.finish_node();
  let root = SyntaxNode::<TLang>::new_root(builder.finish());
  assert_eq!(child::<Ident, TLang>(&root), None);
}

#[test]
fn children_skips_the_children_that_do_not_cast() {
  let root = make_node_tree();
  let texts: Vec<String> = children::<Ident, TLang>(&root)
    .map(|i| i.0.to_string())
    .collect();
  assert_eq!(texts, vec!["x".to_string(), "y".to_string()]);
}

#[test]
fn node_children_iterates_a_cast_node_only_type() {
  let root = make_node_tree();
  let mut iter: NodeChildren<Ident, TLang> = children::<Ident, TLang>(&root);
  assert!(iter.next().is_some());
  assert!(iter.next().is_some());
  assert!(iter.next().is_none());
}

// ── `token_any` and `tokens` ──────────────────────────────────────────────────

/// `Root` over four direct *tokens* of four different kinds: `Ident("a")`, `Str("s")`,
/// `Plus("+")`, `BlockStr("bs")`. `Str` precedes `BlockStr`, which is what makes the
/// kinds-order-independence assertion meaningful.
fn make_two_kind_tree() -> SyntaxNode<TLang> {
  let mut builder = GreenNodeBuilder::new();
  builder.start_node(TLang::kind_to_raw(TK::Root));
  builder.token(TLang::kind_to_raw(TK::Ident), "a");
  builder.token(TLang::kind_to_raw(TK::Str), "s");
  builder.token(TLang::kind_to_raw(TK::Plus), "+");
  builder.token(TLang::kind_to_raw(TK::BlockStr), "bs");
  builder.finish_node();
  SyntaxNode::new_root(builder.finish())
}

/// `Root` over three direct `Ident` tokens, interleaved with `Plus`: `"a" "+" "b" "+" "c"`.
fn make_repeated_kind_tree() -> SyntaxNode<TLang> {
  let mut builder = GreenNodeBuilder::new();
  builder.start_node(TLang::kind_to_raw(TK::Root));
  for (kind, text) in [
    (TK::Ident, "a"),
    (TK::Plus, "+"),
    (TK::Ident, "b"),
    (TK::Plus, "+"),
    (TK::Ident, "c"),
  ] {
    builder.token(TLang::kind_to_raw(kind), text);
  }
  builder.finish_node();
  SyntaxNode::new_root(builder.finish())
}

/// `Root[Ident("first") Group[Ident("nested")] Ident("second")]`. `"nested"` is an `Ident`
/// *token*, but it is a direct child of `Group`, not of `Root` — exactly the shape that
/// distinguishes a direct-children scan from a recursive one.
fn make_tree_with_nested_token() -> SyntaxNode<TLang> {
  let mut builder = GreenNodeBuilder::new();
  builder.start_node(TLang::kind_to_raw(TK::Root));
  builder.token(TLang::kind_to_raw(TK::Ident), "first");
  builder.start_node(TLang::kind_to_raw(TK::Group));
  builder.token(TLang::kind_to_raw(TK::Ident), "nested");
  builder.finish_node();
  builder.token(TLang::kind_to_raw(TK::Ident), "second");
  builder.finish_node();
  SyntaxNode::new_root(builder.finish())
}

#[test]
fn token_any_finds_the_document_order_first_match() {
  let root = make_two_kind_tree();
  let found = token_any(&root, &[TK::Str, TK::BlockStr]);
  assert_eq!(found.map(|t| t.text().to_string()), Some("s".to_string()));
}

#[test]
fn token_any_answer_does_not_depend_on_the_order_of_kinds() {
  // `Str` precedes `BlockStr` in the tree, so both orderings of the slice must answer `Str`.
  // A "try kinds[0], then kinds[1]" implementation answers `BlockStr` for the reversed slice
  // and fails this.
  let root = make_two_kind_tree();

  let forward = token_any(&root, &[TK::Str, TK::BlockStr]);
  let reversed = token_any(&root, &[TK::BlockStr, TK::Str]);

  assert_eq!(forward, reversed);
  assert_eq!(forward.map(|t| t.text().to_string()), Some("s".to_string()));
}

#[test]
fn token_any_with_empty_kinds_finds_nothing() {
  let root = make_two_kind_tree();
  assert_eq!(token_any(&root, &[]), None);
}

#[test]
fn token_any_with_unmatched_kinds_finds_nothing() {
  let root = make_tree();
  assert_eq!(token_any(&root, &[TK::Str, TK::BlockStr]), None);
}

#[test]
fn token_any_only_sees_direct_token_children() {
  let root = make_tree_with_nested_token();
  // `Group`'s own `Ident` must not answer for `Root`: only `"first"` is a direct child, and it
  // precedes `"second"`, the other direct child.
  let found = token_any(&root, &[TK::Ident]);
  assert_eq!(
    found.map(|t| t.text().to_string()),
    Some("first".to_string())
  );
}

#[test]
fn tokens_yields_every_match_in_document_order() {
  let root = make_repeated_kind_tree();
  let texts: Vec<String> = tokens(&root, &TK::Ident)
    .map(|t| t.text().to_string())
    .collect();
  assert_eq!(
    texts,
    vec!["a".to_string(), "b".to_string(), "c".to_string()]
  );
}

#[test]
fn tokens_yields_nothing_when_there_are_no_matches() {
  let root = make_repeated_kind_tree();
  let mut iter = tokens(&root, &TK::BlockStr);
  assert!(iter.next().is_none());
}

#[test]
fn tokens_only_sees_direct_token_children() {
  let root = make_tree_with_nested_token();
  let texts: Vec<String> = tokens(&root, &TK::Ident)
    .map(|t| t.text().to_string())
    .collect();
  // "nested" lives inside the `Group` child node, not directly under `Root`. A recursive scan
  // would answer three matches instead of two.
  assert_eq!(texts, vec!["first".to_string(), "second".to_string()]);
}
