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

/// A retro-wrap opens a node like a direct open does, and is refused at the same ceiling when it
/// genuinely swallows that depth — the door `start_node` alone would have left open.
///
/// The companion to `a_row_of_sibling_wraps_is_two_levels_deep_however_many_there_are`: that cell
/// holds that width is not depth, and this one holds that the fix is not simply *charging less*.
/// Here the wrap really does inherit a chain at the ceiling, and the charge has to see it.
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

/// The builder door's half of the minimum-stack contract, run rather than asserted — the sink
/// suite's twin, and the shape the review named: **a refusal that drops an already-completed
/// tree at the ceiling.**
///
/// `the_builder_refuses_a_retro_wrap_that_crosses_the_ceiling` establishes that the refusal
/// happens; this establishes that surviving it does not depend on the stack the ceiling was
/// measured on being bigger than the one this crate promises. It fails by the process dying,
/// for the reason its sink twin records.
#[test]
fn both_builder_outcomes_survive_the_ceiling_on_the_minimum_supported_stack() {
  let handle = std::thread::Builder::new()
    .stack_size(MIN_SUPPORTED_STACK)
    .spawn(|| {
      // Accepted, at the ceiling: built and dropped here.
      let green = builder_chain(MAX_TREE_DEPTH).expect("a tree at the ceiling is buildable");
      assert_eq!(green_depth(&green), MAX_TREE_DEPTH);
      drop(green);

      // Refused, with a completed ceiling-deep tree in hand: the retro-wrap is charged
      // `level + swallowed`, so it crosses while the builder is holding the whole thing.
      let builder = SyntaxTreeBuilder::<TestLang>::new();
      let checkpoint = builder.checkpoint();
      for _ in 0..MAX_TREE_DEPTH {
        builder.start_node(TestKind::Root);
      }
      builder.token(TestKind::Ident, "x");
      for _ in 0..MAX_TREE_DEPTH {
        builder.finish_node();
      }
      builder.start_node_at(checkpoint, TestKind::Root);
      assert_eq!(
        builder.finish().expect_err("the wrap crosses the ceiling"),
        FinishError::TooDeep {
          depth: MAX_TREE_DEPTH as u64,
        }
      );
    })
    .expect("the probe thread starts");
  handle
    .join()
    .expect("neither outcome may abort or panic on the stack this crate says it needs");
}

/// **Width is not depth.** `N` sibling retro-wraps under one root, each wrapping exactly one
/// token, is a tree of depth 2 however large `N` is — and `N` here is past the ceiling.
///
/// This is the counterexample to charging a wrap against the deepest child the *level* ever
/// completed: a wrap swallows only the children added after its own checkpoint, so a sibling
/// that closed before it is not in its subtree and must not be in its charge. Charging
/// level-wide made each sibling's depth feed the next one's charge, so a flat row of wraps
/// climbed one phantom level per sibling and the builder latched somewhere past the thousandth
/// — discarding a perfectly droppable two-level tree with `TooDeep`.
#[test]
fn a_row_of_sibling_wraps_is_two_levels_deep_however_many_there_are() {
  const SIBLINGS: usize = MAX_TREE_DEPTH + 64;

  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  for _ in 0..SIBLINGS {
    let checkpoint = builder.checkpoint();
    builder.token(TestKind::Ident, "x");
    builder.start_node_at(checkpoint, TestKind::Root);
    builder.finish_node();
  }
  builder.finish_node();

  let green = builder
    .finish()
    .expect("a row of one-token wraps is two levels deep, whatever its width");
  assert_eq!(
    green_depth(&green),
    2,
    "root, then one wrap per token — the width never becomes depth"
  );
}

/// The bound is **on the structure, not on the input**, and it is asserted rather than argued.
///
/// The row above is the width case: `MAX_TREE_DEPTH + 64` children at one level, and the ledger
/// holds a handful of entries throughout because it keeps only the suffix maxima — strictly
/// decreasing depths, each at most the ceiling.
#[test]
fn the_ledger_is_bounded_by_the_ceiling_and_not_by_the_width() {
  const SIBLINGS: usize = MAX_TREE_DEPTH + 64;

  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  let mut widest = 0usize;
  for _ in 0..SIBLINGS {
    let checkpoint = builder.checkpoint();
    builder.token(TestKind::Ident, "x");
    builder.start_node_at(checkpoint, TestKind::Root);
    builder.finish_node();
    widest = widest.max(builder.ledger.borrow().suffix_max.len());
  }
  builder.finish_node();

  assert!(
    widest <= MAX_TREE_DEPTH + 1,
    "the suffix-max stack holds strictly decreasing depths bounded by the ceiling, so it cannot \
     exceed MAX_TREE_DEPTH + 1 entries — it reached {widest}"
  );
  assert!(
    widest <= 4,
    "and over a flat row it is a handful, not a function of the width: {widest} entries for \
     {SIBLINGS} children"
  );
  let _ = builder.finish().expect("still two levels deep");
}

/// **The fix is not "charge less".** A wrap that genuinely inherits depth is charged for it, and
/// the tree it produces is the depth the charge predicted.
#[test]
fn a_wrap_that_really_swallows_a_chain_is_charged_for_it() {
  const CHAIN: usize = 500;

  let builder = SyntaxTreeBuilder::<TestLang>::new();
  let checkpoint = builder.checkpoint();
  for _ in 0..CHAIN {
    builder.start_node(TestKind::Root);
  }
  builder.token(TestKind::Ident, "x");
  for _ in 0..CHAIN {
    builder.finish_node();
  }
  // The chain is complete and `CHAIN` levels deep; this wrap inherits all of it.
  assert_eq!(
    builder.ledger.borrow().deepest_after(checkpoint.at),
    CHAIN,
    "the charge is the depth of the range rowan will drain into the wrap"
  );
  builder.start_node_at(checkpoint, TestKind::Root);
  builder.finish_node();

  let green = builder.finish().expect("501 levels is under the ceiling");
  assert_eq!(
    green_depth(&green),
    CHAIN + 1,
    "the wrap is one level above everything it swallowed"
  );
}

/// A deep sibling does not poison its neighbour, and the margin is exactly one level — the cell
/// that separates "charged over the checkpoint's range" from "charged over the level".
///
/// The chain is at the ceiling under the root, so the tree is exactly at it. The wrap beside it
/// inherits nothing and is charged `0`; charged level-wide it would have been charged the chain's
/// own depth and refused a tree that fits.
#[test]
fn a_wrap_beside_a_ceiling_deep_sibling_is_charged_for_its_own_range_only() {
  let chain = MAX_TREE_DEPTH - 1;

  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  for _ in 0..chain {
    builder.start_node(TestKind::Root);
  }
  builder.token(TestKind::Ident, "x");
  for _ in 0..chain {
    builder.finish_node();
  }
  let checkpoint = builder.checkpoint();
  builder.token(TestKind::Ident, "y");
  assert_eq!(
    builder.ledger.borrow().deepest_after(checkpoint.at),
    0,
    "the sibling chain closed BEFORE this checkpoint, so it is not in this wrap's range"
  );
  builder.start_node_at(checkpoint, TestKind::Root);
  builder.finish_node();
  builder.finish_node();

  let green = builder
    .finish()
    .expect("root over a ceiling-deep chain and a one-token wrap is exactly at the ceiling");
  assert_eq!(green_depth(&green), MAX_TREE_DEPTH);
}

// ── Checkpoint reuse, order and staleness: what each one does, established ──────
//
// A checkpoint is a value the caller holds, so it can be spent twice, spent out of order, or
// spent after its level has closed. The answer for all of them is one sentence, and it is a
// property of the ledger's shape rather than a set of cases it handles: **the ledger indexes
// rowan's own flat child stream**, so a use rowan ACCEPTS wraps exactly `children[cp..]` — the
// range the charge was computed over — and a use rowan REFUSES panics in rowan, before the
// ledger is touched, because the forward happens first.

/// Spending one checkpoint twice is legal, and the second wrap is charged over what it actually
/// wraps — the first wrap.
#[test]
fn a_checkpoint_spent_twice_charges_the_second_wrap_over_the_first() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  let checkpoint = builder.checkpoint();
  builder.token(TestKind::Ident, "x");

  builder.start_node_at(checkpoint, TestKind::Root);
  builder.finish_node();
  assert_eq!(
    builder.ledger.borrow().deepest_after(checkpoint.at),
    1,
    "the first wrap is now the only child in that range, and it is one level deep"
  );

  builder.start_node_at(checkpoint, TestKind::Root);
  builder.finish_node();
  builder.finish_node();

  let green = builder.finish().expect("three levels");
  assert_eq!(
    green_depth(&green),
    3,
    "root, the second wrap, the first wrap — the second is charged for the first, not beside it"
  );
}

/// Checkpoints spent out of order are each charged over their own range.
#[test]
fn checkpoints_spent_out_of_order_are_charged_over_their_own_ranges() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  let outer = builder.checkpoint();
  builder.token(TestKind::Ident, "a");
  let inner = builder.checkpoint();
  builder.token(TestKind::Ident, "b");

  // The later checkpoint first: it wraps only `b`.
  builder.start_node_at(inner, TestKind::Root);
  builder.finish_node();
  // Then the earlier one, which now wraps `a` and that wrap.
  builder.start_node_at(outer, TestKind::Root);
  builder.finish_node();
  builder.finish_node();

  let green = builder.finish().expect("three levels");
  assert_eq!(
    green_depth(&green),
    3,
    "root, the outer wrap, the inner wrap"
  );
}

/// A checkpoint taken at a node's first child, spent after that node closed, is a use `rowan`
/// **accepts** — and the ledger charges it exactly, because it is charged over the same flat
/// range `rowan` wraps.
#[test]
fn a_checkpoint_rowan_still_accepts_after_its_node_closed_is_charged_exactly() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  let checkpoint = builder.checkpoint();
  builder.start_node(TestKind::Root);
  builder.token(TestKind::Ident, "x");
  builder.finish_node();

  // The node is closed; `rowan` accepts the checkpoint here because it names that node's slot.
  assert_eq!(
    builder.ledger.borrow().deepest_after(checkpoint.at),
    1,
    "the range now holds the closed node, one level deep, and the wrap inherits it"
  );
  builder.start_node_at(checkpoint, TestKind::Root);
  builder.finish_node();
  builder.finish_node();

  let green = builder.finish().expect("three levels");
  assert_eq!(
    green_depth(&green),
    3,
    "root, the wrap, the node it wrapped"
  );
}

/// A checkpoint whose children a `finish_node` already drained is `rowan`'s panic, in `rowan`,
/// and the ledger never sees it — the forward happens before the ledger moves.
#[test]
#[should_panic(expected = "checkpoint no longer valid")]
fn a_checkpoint_left_behind_by_a_finish_node_panics_in_rowan() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  builder.start_node(TestKind::Root);
  builder.token(TestKind::Ident, "a");
  builder.token(TestKind::Ident, "b");
  let checkpoint = builder.checkpoint();
  builder.finish_node();
  builder.start_node_at(checkpoint, TestKind::Root);
}

/// And a checkpoint from before the current node started is `rowan`'s other assert, on the same
/// terms.
#[test]
#[should_panic(expected = "checkpoint no longer valid")]
fn a_checkpoint_from_below_the_open_node_panics_in_rowan() {
  let builder = SyntaxTreeBuilder::<TestLang>::new();
  let checkpoint = builder.checkpoint();
  builder.token(TestKind::Ident, "a");
  builder.start_node(TestKind::Root);
  builder.start_node_at(checkpoint, TestKind::Root);
}
