use super::{CastNode, Language, NodeChildren, SyntaxNode};

/// Returns the first child of a specific typed node type.
///
/// Searches through the children of the parent node and returns the first child
/// that can be successfully cast to the specified node type `N`.
#[inline]
pub fn child<N: CastNode<Lang>, Lang: Language>(parent: &SyntaxNode<Lang>) -> Option<N> {
  parent.children().find_map(N::cast_node)
}

/// Returns an iterator over all children of a specific typed node type.
///
/// Iterates through all children of the parent node, yielding only those that
/// can be successfully cast to the specified node type `N`.
#[inline]
pub fn children<N: CastNode<Lang>, Lang: Language>(
  parent: &SyntaxNode<Lang>,
) -> NodeChildren<N, Lang> {
  NodeChildren::new(parent)
}

/// Returns the first token child with the specified syntax kind.
///
/// Searches through all tokens (not nodes) that are direct children of the parent
/// and returns the first one matching the specified kind.
#[inline]
pub fn token<L: Language>(parent: &SyntaxNode<L>, kind: &L::Kind) -> Option<rowan::SyntaxToken<L>> {
  parent
    .children_with_tokens()
    .filter_map(|child| {
      child
        .into_token()
        .and_then(|t| t.kind().eq(kind).then_some(t))
    })
    .next()
}

/// Returns the first token child whose kind is one of `kinds`.
///
/// Searches through all tokens (not nodes) that are direct children of the parent and returns
/// the first one whose kind matches any entry in `kinds`.
///
/// The scan is in **document order**, not in `kinds` order: checking for `kinds[0]` first and
/// falling back to `kinds[1]` only once the first search comes up empty would answer the wrong
/// token for a parent that carries both kinds, which is a difference no caller should have to
/// know about. A single pass over the children settles it, in the order the tokens actually
/// appear.
#[inline]
pub fn token_any<L: Language>(
  parent: &SyntaxNode<L>,
  kinds: &[L::Kind],
) -> Option<rowan::SyntaxToken<L>> {
  parent
    .children_with_tokens()
    .filter_map(|child| {
      child
        .into_token()
        .and_then(|t| kinds.contains(&t.kind()).then_some(t))
    })
    .next()
}

/// Returns an iterator over every token child with the specified syntax kind, in document
/// order.
///
/// Searches through all tokens (not nodes) that are direct children of the parent and yields
/// every one matching the specified kind, in the order they appear. This is [`token`]'s plural
/// form: `token` answers only its first match, so a parent that carries several token children
/// of one kind — a repeated separator or keyword token with no node kind of its own — exposes
/// only one of them without this.
#[inline]
pub fn tokens<L: Language>(parent: &SyntaxNode<L>, kind: &L::Kind) -> TokenChildren<L> {
  TokenChildren {
    inner: parent.children_with_tokens(),
    kind: *kind,
  }
}

/// An iterator over a node's direct token children of one syntax kind, in document order.
///
/// [`tokens`] is its only constructor. Unlike [`NodeChildren`], this is not an alias for an
/// upstream type: rowan has no iterator that filters a node's children down to one token kind,
/// so this crate provides one.
#[derive(Debug, Clone)]
pub struct TokenChildren<L: Language> {
  inner: rowan::SyntaxElementChildren<L>,
  kind: L::Kind,
}

impl<L: Language> Iterator for TokenChildren<L> {
  type Item = rowan::SyntaxToken<L>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    // Direct children only, exactly as `token` scans: a token belonging to a child node is that
    // node's, and reaching it here would make a parent answer for its child.
    self
      .inner
      .by_ref()
      .find_map(|child| child.into_token().filter(|t| t.kind() == self.kind))
  }
}

#[cfg(test)]
#[allow(warnings)]
mod tests;
