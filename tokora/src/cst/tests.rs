use super::*;
use rowan::{Language, SyntaxKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TestKind {
  Root,
  Ident,
  Plus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TestLang {}

impl Language for TestLang {
  type Kind = TestKind;

  fn kind_from_raw(raw: SyntaxKind) -> TestKind {
    match raw.0 {
      0 => TestKind::Root,
      1 => TestKind::Ident,
      2 => TestKind::Plus,
      _ => panic!("unknown kind"),
    }
  }

  fn kind_to_raw(kind: TestKind) -> SyntaxKind {
    match kind {
      TestKind::Root => SyntaxKind(0),
      TestKind::Ident => SyntaxKind(1),
      TestKind::Plus => SyntaxKind(2),
    }
  }
}

#[test]
fn builder_new_and_default() {
  let b1 = SyntaxTreeBuilder::<TestLang>::new();
  let b2 = SyntaxTreeBuilder::<TestLang>::default();
  // Just verify they can be created
  let _ = format!("{:?}", b1);
  let _ = format!("{:?}", b2);
}

#[test]
fn builder_simple_tree() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  builder.token(TestKind::Ident, "hello");
  builder.finish_node();
  let green = builder.finish().expect("under the depth ceiling");

  let root = rowan::SyntaxNode::<TestLang>::new_root(green);
  assert_eq!(root.kind(), TestKind::Root);
  assert_eq!(root.to_string(), "hello");
}

#[test]
fn builder_with_checkpoint() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);

  let checkpoint = builder.checkpoint();
  builder.token(TestKind::Ident, "foo");

  // Wrap the identifier in a new node retroactively
  builder.start_node_at(checkpoint, TestKind::Root);
  builder.finish_node();

  builder.finish_node();
  let green = builder.finish().expect("under the depth ceiling");
  let root = rowan::SyntaxNode::<TestLang>::new_root(green);
  assert_eq!(root.to_string(), "foo");
}

#[test]
fn builder_multiple_tokens() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  builder.token(TestKind::Ident, "a");
  builder.token(TestKind::Plus, "+");
  builder.token(TestKind::Ident, "b");
  builder.finish_node();
  let green = builder.finish().expect("under the depth ceiling");

  let root = rowan::SyntaxNode::<TestLang>::new_root(green);
  assert_eq!(root.to_string(), "a+b");
}

#[test]
fn cst_node_children_clone() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  builder.token(TestKind::Ident, "hello");
  builder.finish_node();
  let green = builder.finish().expect("under the depth ceiling");
  let root = rowan::SyntaxNode::<TestLang>::new_root(green);

  let children: NodeChildren<rowan::SyntaxNode<TestLang>, TestLang> = NodeChildren::new(&root);
  let _cloned = children.clone();
}

#[test]
fn cst_node_children_by_kind() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  builder.start_node(TestKind::Root);
  builder.token(TestKind::Ident, "inner");
  builder.finish_node();
  builder.finish_node();
  let green = builder.finish().expect("under the depth ceiling");
  let root = rowan::SyntaxNode::<TestLang>::new_root(green);

  let children: NodeChildren<rowan::SyntaxNode<TestLang>, TestLang> = NodeChildren::new(&root);
  let matching: Vec<_> = children.by_kind(|k| k == TestKind::Root).collect();
  assert_eq!(matching.len(), 1);
}

// ── The depth ceiling at the builder door (al8n/tokora#316) ─────────────────────
//
// `SyntaxTreeBuilder` has no event log behind it, so nothing but the builder itself can bound
// what it hands to `rowan`. What these cells cannot pin is the same half the sink's cells
// cannot: a tree deep enough to ACTUALLY abort would have to exist to be asserted about, and
// it dies in its own destructor with no unwind to catch. See the `cst/sink/tests.rs` header
// for the boundary and `cst/mod.rs` for the bisection the ceiling is derived from.

/// A green tree's own nesting depth, walked iteratively for the reason the sink suite's twin
/// states: an oracle for a depth ceiling must not itself recurse over the tree.
fn green_depth(root: &rowan::GreenNode) -> usize {
  let mut deepest = 0usize;
  let mut stack: std::vec::Vec<(&rowan::GreenNodeData, usize)> = std::vec![(&**root, 1usize)];
  while let Some((node, at)) = stack.pop() {
    deepest = deepest.max(at);
    for child in node.children() {
      if let rowan::NodeOrToken::Node(inner) = child {
        stack.push((inner, at + 1));
      }
    }
  }
  deepest
}

/// A chain of `opens` nested nodes around one token, built through the public door.
fn builder_chain(opens: usize) -> Result<rowan::GreenNode, FinishError> {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  for _ in 0..opens {
    builder.start_node(TestKind::Root);
  }
  builder.token(TestKind::Ident, "x");
  for _ in 0..opens {
    builder.finish_node();
  }
  builder.finish()
}

/// The wall, from both sides. There is no root wrapper at this door, so the ceiling is the
/// open count exactly.
#[test]
fn the_builder_accepts_a_tree_at_the_ceiling_and_refuses_one_past_it() {
  let green = builder_chain(MAX_TREE_DEPTH).expect("a tree exactly at the ceiling is buildable");
  assert_eq!(
    green_depth(&green),
    MAX_TREE_DEPTH,
    "the accepted tree must sit exactly ON the ceiling"
  );

  assert_eq!(
    builder_chain(MAX_TREE_DEPTH + 1).expect_err("one open past the ceiling is refused"),
    FinishError::TooDeep {
      depth: MAX_TREE_DEPTH as u64,
    }
  );
}

/// A retro-wrap opens a node like a direct open does, and is refused at the same ceiling —
/// the door `start_node` alone would have left open.
#[test]
fn the_builder_refuses_a_retro_wrap_that_crosses_the_ceiling() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  let checkpoint = builder.checkpoint();
  for _ in 0..MAX_TREE_DEPTH {
    builder.start_node(TestKind::Root);
  }
  builder.token(TestKind::Ident, "x");
  for _ in 0..MAX_TREE_DEPTH {
    builder.finish_node();
  }
  // Everything above is legal and exactly at the ceiling; this wrap is the level past it.
  builder.start_node_at(checkpoint, TestKind::Root);
  builder.finish_node();

  assert_eq!(
    builder
      .finish()
      .expect_err("a retro-wrap nests exactly as deep as a direct open"),
    FinishError::TooDeep {
      depth: MAX_TREE_DEPTH as u64,
    }
  );
}

/// The latch is total and it never clears: once an open has been refused, nothing this builder
/// records afterwards reaches `rowan` and `finish` cannot start succeeding again.
///
/// The alternative — suppressing only the over-deep opens — leaves a caller's tokens landing in
/// the wrong parent, which is a *plausible* wrong tree. This shape leaves an obviously partial
/// one, and it is never handed back either way.
#[test]
fn a_refused_open_latches_the_builder_for_good() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  for _ in 0..=MAX_TREE_DEPTH {
    builder.start_node(TestKind::Root);
  }
  // Unwind the whole chain and build a second, shallow, perfectly legal tree on top.
  for _ in 0..=MAX_TREE_DEPTH {
    builder.finish_node();
  }
  builder.start_node(TestKind::Root);
  builder.token(TestKind::Ident, "x");
  builder.finish_node();

  assert_eq!(
    builder
      .finish()
      .expect_err("a refusal cannot be un-refused by later, shallower work"),
    FinishError::TooDeep {
      depth: MAX_TREE_DEPTH as u64,
    }
  );
}
