// `Node<Lang>` binds `Syntax<Lang = Lang>`, so a node cannot claim membership in
// one language while its kind authority answers for another.
//
// `Element<A>` says which tree the node lives in. `Syntax` carries the `KIND` constant that
// says which node it *is*, typed `<Self::Lang as Language>::SyntaxKind`. While `Syntax::Lang`
// was unconstrained, the type below satisfied `Node<A>` with its kind authority pointing at
// `B` — nothing rejected it, and the contradiction surfaced later as a cast that never matches.
//
// The positive control is `node_brand_matches` in `tests/misc.rs`, which differs from this in
// exactly one token: `type Lang = B` becomes `type Lang = A`.
//
// The fixture is deliberately complete in every other respect — both languages derive the full
// `Language` bound set and `Crossed` derives `Debug` — so the committed `.stderr` contains
// EXACTLY ONE error, the `E0271` brand mismatch. A `compile_fail` case that also fails for
// missing derives would keep failing if the brand equality were deleted, which is a rail that
// cannot fail for the reason it names.
#![allow(dead_code)]

use tokora::cst::{Element, Node, error::SyntaxError};
use tokora::syntax::Syntax;
use tokora::utils::{GenericArrayDeque, typenum::U0};

use rowan::{Language, SyntaxNode};

macro_rules! lang {
  ($name:ident, $kind:ident) => {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[repr(u16)]
    pub enum $kind {
      Only = 0,
      #[allow(dead_code)]
      Root = 1,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum $name {}

    impl Language for $name {
      type Kind = $kind;
      fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        match raw.0 {
          0 => $kind::Only,
          _ => $kind::Root,
        }
      }
      fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
      }
    }
  };
}

lang!(A, KindA);
lang!(B, KindB);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoComponent;
impl core::fmt::Display for NoComponent {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("none")
  }
}

#[derive(Debug)]
struct Crossed(SyntaxNode<A>);

impl Syntax for Crossed {
  // The mismatch: this node lives in `A`'s tree but names `B` as its kind authority.
  type Lang = B;
  const KIND: KindB = KindB::Only;
  type Component = NoComponent;
  type COMPONENTS = U0;
  type REQUIRED = U0;

  fn possible_components() -> &'static GenericArrayDeque<Self::Component, U0> {
    const C: &GenericArrayDeque<NoComponent, U0> = &GenericArrayDeque::from_array([]);
    C
  }
  fn required_components() -> &'static GenericArrayDeque<Self::Component, U0> {
    const C: &GenericArrayDeque<NoComponent, U0> = &GenericArrayDeque::from_array([]);
    C
  }
}

impl Element<A> for Crossed {
  const KIND: KindA = KindA::Only;
  fn castable(kind: KindA) -> bool {
    kind == KindA::Only
  }
}

impl Node<A> for Crossed {
  fn try_cast_node(syntax: SyntaxNode<A>) -> Result<Self, SyntaxError<Self, A>> {
    Ok(Self(syntax))
  }
  fn syntax(&self) -> &SyntaxNode<A> {
    &self.0
  }
}

fn main() {}
