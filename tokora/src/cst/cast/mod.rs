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

#[cfg(test)]
#[allow(warnings)]
mod tests;
