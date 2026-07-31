use super::*;
use rowan::{GreenNodeBuilder, Language as _, SyntaxKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TK {
  Root,
  Ident,
  Plus,
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
      _ => panic!("unknown"),
    }
  }

  fn kind_to_raw(kind: TK) -> SyntaxKind {
    match kind {
      TK::Root => SyntaxKind(0),
      TK::Ident => SyntaxKind(1),
      TK::Plus => SyntaxKind(2),
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
