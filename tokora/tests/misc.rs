#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
mod common;

// Targeted coverage tests for scattered uncovered lines in CST, delimiter,
// input, parser, and utility modules.

#[allow(unused_imports)]
use common::{TestLexer, Token, TokenKind};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, cache::DefaultCache,
  emitter::Ignored,
};

// ── Context/parser helpers ────────────────────────────────────────────────────

type IgnoredContext<'inp> =
  ParserContext<'inp, TestLexer<'inp>, Ignored, DefaultCache<'inp, TestLexer<'inp>>>;

macro_rules! ignored_parser {
  () => {
    Parser::with_context(IgnoredContext::new(Ignored::default()))
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// cst/cast.rs and cst/mod.rs
// The cast::child and cast::children functions require Node which requires
// Syntax (a complex trait). The cast.rs functions are tested in inline unit
// tests inside the file. We exercise the builder-based paths (which the file
// already covers), plus NodeChildren and the source_string/clone variants
// which require a proper Syntax impl.
//
// We provide a minimal Syntax + Node impl to cover lines 474-475, 482-486,
// 493-497, 551-552, and cast.rs 8-9, 17, 20.
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(all(test, feature = "rowan"))]
mod cst_coverage {
  use rowan::{GreenNodeBuilder, Language as RowanLanguage, SyntaxKind, SyntaxNode};
  use tokora::{
    cst::{Element, Node, NodeChildren, SyntaxTreeBuilder, cast, error::NodeMismatch},
    syntax::Syntax,
    utils::{GenericArrayDeque, typenum::U0},
  };

  // ── Minimal test language ────────────────────────────────────────────────────
  // Note: rowan::Language already provides a blanket impl for tokora::syntax::Language,
  // so we only need to impl rowan::Language.

  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  enum K {
    Root,
    Inner,
  }

  impl core::fmt::Display for K {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        K::Root => write!(f, "Root"),
        K::Inner => write!(f, "Inner"),
      }
    }
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  enum Lang {}

  impl RowanLanguage for Lang {
    type Kind = K;
    fn kind_from_raw(raw: SyntaxKind) -> K {
      match raw.0 {
        0 => K::Root,
        _ => K::Inner,
      }
    }
    fn kind_to_raw(k: K) -> SyntaxKind {
      match k {
        K::Root => SyntaxKind(0),
        K::Inner => SyntaxKind(1),
      }
    }
  }

  // ── Component type for Syntax ────────────────────────────────────────────────

  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  struct NoComponent;

  impl core::fmt::Display for NoComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      write!(f, "none")
    }
  }

  // ── InnerNode typed wrapper ──────────────────────────────────────────────────

  #[derive(Debug)]
  struct InnerNode(SyntaxNode<Lang>);

  impl Syntax for InnerNode {
    type Lang = Lang;
    const KIND: K = K::Inner;
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

  impl Element<Lang> for InnerNode {
    const KIND: K = K::Inner;
    fn castable(kind: K) -> bool {
      kind == K::Inner
    }
  }

  impl Node<Lang> for InnerNode {
    fn try_cast_node(
      syntax: SyntaxNode<Lang>,
    ) -> Result<Self, tokora::cst::error::SyntaxError<Self, Lang>> {
      if Self::castable(syntax.kind()) {
        Ok(Self(syntax))
      } else {
        Err(NodeMismatch::new(syntax).into())
      }
    }
    fn syntax(&self) -> &SyntaxNode<Lang> {
      &self.0
    }
  }

  fn make_nested() -> SyntaxNode<Lang> {
    let mut b = GreenNodeBuilder::new();
    b.start_node(Lang::kind_to_raw(K::Root));
    b.start_node(Lang::kind_to_raw(K::Inner));
    b.token(Lang::kind_to_raw(K::Inner), "a");
    b.finish_node();
    b.start_node(Lang::kind_to_raw(K::Inner));
    b.token(Lang::kind_to_raw(K::Inner), "b");
    b.finish_node();
    b.finish_node();
    SyntaxNode::new_root(b.finish())
  }

  // ── Tests ──────────────────────────────────────────────────────────────────

  #[test]
  fn cast_child_finds_first_inner() {
    // Covers cast.rs lines 8-9: child()
    let root = make_nested();
    let found: Option<InnerNode> = cast::child::<InnerNode, Lang>(&root);
    assert!(found.is_some());
    assert_eq!(found.unwrap().syntax().to_string(), "a");
  }

  #[test]
  fn cast_child_returns_none_for_missing() {
    let mut b = GreenNodeBuilder::new();
    b.start_node(Lang::kind_to_raw(K::Root));
    b.token(Lang::kind_to_raw(K::Root), "x");
    b.finish_node();
    let root = SyntaxNode::<Lang>::new_root(b.finish());
    let found: Option<InnerNode> = cast::child::<InnerNode, Lang>(&root);
    assert!(found.is_none());
  }

  #[test]
  fn cast_children_collects_all() {
    // Covers cast.rs lines 17, 20: children()
    let root = make_nested();
    let collected: Vec<InnerNode> = cast::children::<InnerNode, Lang>(&root).collect();
    assert_eq!(collected.len(), 2);
  }

  #[test]
  fn cst_node_source_string() {
    // Covers cst/mod.rs lines 474-475: source_string()
    let root = make_nested();
    let inner: InnerNode = cast::child::<InnerNode, Lang>(&root).unwrap();
    assert_eq!(inner.source_string(), "a");
  }

  // `cst_node_clone_for_update` was deleted when `rowan` 0.17 removed the mutable
  // red-tree API it exercised. There is no replacement to re-point it at: 0.17 has no
  // `SyntaxNode::clone_for_update`, so `Node::clone_for_update` is gone too. The
  // immutable deep copy below is a different operation and already had its own test.

  #[test]
  fn cst_node_clone_subtree() {
    // Covers cst/mod.rs lines 493-497: clone_subtree()
    let root = make_nested();
    let inner: InnerNode = cast::child::<InnerNode, Lang>(&root).unwrap();
    let cloned = inner.clone_subtree();
    assert_eq!(cloned.source_string(), "a");
  }

  #[test]
  fn cst_node_children_iterator_next() {
    // Covers cst/mod.rs lines 551-552: NodeChildren::next()
    let root = make_nested();
    let mut iter: NodeChildren<InnerNode, Lang> = cast::children::<InnerNode, Lang>(&root);
    let first = iter.next();
    let second = iter.next();
    let third = iter.next();
    assert!(first.is_some());
    assert!(second.is_some());
    assert!(third.is_none());
  }

  #[test]
  fn syntax_tree_builder_basic() {
    // Exercise SyntaxTreeBuilder
    let builder = SyntaxTreeBuilder::<Lang>::new();
    builder.start_node(K::Root);
    builder.token(K::Inner, "hello");
    builder.finish_node();
    let green = builder.finish().expect("under the depth ceiling");
    let root = SyntaxNode::<Lang>::new_root(green);
    assert_eq!(root.to_string(), "hello");
  }

  #[test]
  fn cst_node_text_method() {
    // Covers cst/mod.rs lines 432, 436: Token::text()
    // (exercised via SyntaxNode::to_string which calls token text internally)
    let root = make_nested();
    let s = root.to_string();
    assert!(s.contains('a'));
    assert!(s.contains('b'));
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// delimiter.rs — lines 68-69, 89-90
// Delimiter::name() for built-in delimiters, and the deref impl.
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod delimiter_coverage {
  use crate::common::TestLexer;
  use tokora::{
    delimiter::Delimiter,
    punct::{Brace, Bracket, Paren},
  };

  #[test]
  fn paren_delimiter_name() {
    // Covers delimiter.rs lines 68-69: impl_builtin_delimiter! name()
    let name = <Paren<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::name();
    assert_eq!(name.as_str(), "()");
  }

  #[test]
  fn bracket_delimiter_name() {
    let name = <Bracket<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::name();
    assert_eq!(name.as_str(), "[]");
  }

  #[test]
  fn brace_delimiter_name() {
    let name = <Brace<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::name();
    assert_eq!(name.as_str(), "{}");
  }

  #[test]
  fn ref_delimiter_name_deref() {
    // Covers delimiter.rs lines 88-90: impl_deref! name() for &D
    let name = <&Paren<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::name();
    assert_eq!(name.as_str(), "()");
  }

  #[test]
  fn mut_ref_delimiter_name_deref() {
    // Covers delimiter.rs lines 88-90: impl_deref! name() for &mut D
    let name = <&mut Paren<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::name();
    assert_eq!(name.as_str(), "()");
  }

  #[test]
  fn ref_delimiter_is_open() {
    // Covers the is_open deref impl
    use crate::common::TokenKind;
    let kind = TokenKind::LParen;
    let result = <&Paren<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::is_open(&kind);
    assert!(result);
  }

  #[test]
  fn ref_delimiter_is_close() {
    // Covers the is_close deref impl
    use crate::common::TokenKind;
    let kind = TokenKind::RParen;
    let result = <&Paren<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::is_close(&kind);
    assert!(result);
  }

  #[test]
  fn mut_ref_delimiter_is_open() {
    use crate::common::TokenKind;
    let kind = TokenKind::LBrace;
    let result = <&mut Brace<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::is_open(&kind);
    assert!(result);
  }

  #[test]
  fn mut_ref_delimiter_is_close() {
    use crate::common::TokenKind;
    let kind = TokenKind::RBrace;
    let result = <&mut Brace<(), (), ()> as Delimiter<'_, TestLexer<'_>>>::is_close(&kind);
    assert!(result);
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// input/input_ref/mod.rs — lines 158, 161, 194, 358, 386-391, 402
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn inputref_attempt_failure_restores_state() {
  // Covers input_ref/mod.rs line 194: attempt() returning None -> restore
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Attempt to consume a Plus — will fail since input starts with "42"
    let failed = inp.attempt(|i| {
      let tok = i.next().ok()??;
      if matches!(tok.data(), Token::Plus) {
        Some(99i64)
      } else {
        None
      }
    });
    assert!(failed.is_none());

    // After failed attempt, cursor is restored, so we can still read "42"
    match inp.next()? {
      Some(tok) => match tok.into_data() {
        Token::Num(n) => Ok(n),
        _ => Err(()),
      },
      None => Err(()),
    }
  }

  let r = ignored_parser!().apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn inputref_attempt_success() {
  // Covers attempt() success branch
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.attempt(|i| {
      let tok = i.next().ok()??;
      match tok.into_data() {
        Token::Num(n) => Some(n),
        _ => None,
      }
    });
    Ok(result.unwrap_or(0))
  }

  let r = ignored_parser!().apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn inputref_set_span_after_consume_within_bounds() {
  // Covers input_ref/mod.rs lines 161-163 (no cache, new.end < input.len())
  // by consuming tokens from a multi-token input
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut nums = Vec::new();
    while let Some(tok) = inp.next()? {
      if let Token::Num(n) = tok.into_data() {
        nums.push(n);
      }
    }
    Ok(nums)
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![1, 2, 3]);
}

#[test]
fn inputref_set_span_cache_front_path() {
  // Covers input_ref/mod.rs line 158: set_span_after_consume with cache.front_span
  // by peeking (fills cache) then consuming (new.end >= cache.start)

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // peek fills the cache
    let _ = inp.peek_one()?;
    let mut nums = Vec::new();
    // consuming from cache triggers set_span_after_consume with cache.front_span set
    while let Some(tok) = inp.next()? {
      if let Token::Num(n) = tok.into_data() {
        nums.push(n);
      }
    }
    Ok(nums)
  }

  let r = ignored_parser!().apply(parse).parse_str("10 20 30");
  assert_eq!(r.unwrap(), vec![10, 20, 30]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// input/input_ref/fold.rs — lines 25, 50, 85, 126
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn inputref_fold_sums_tokens() {
  // Covers fold.rs line 25: fold loop body (try_expect returns Some -> op called)
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| {
        acc
          + match tok.into_data() {
            Token::Num(n) => n,
            _ => 0,
          }
      },
    )
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 3");
  assert_eq!(r.unwrap(), 6);
}

#[test]
fn inputref_fold_empty_returns_init() {
  // Covers fold.rs line 30: loop exits immediately returning Ok(output)
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 42i64,
      |acc, _| acc,
    )
  }

  let r = ignored_parser!().apply(parse).parse_str("+");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn inputref_foldn_hits_limit() {
  // Covers fold.rs lines 50-52: `if n >= num { return Ok(output); }`
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldn(
      || 0i64,
      |acc, tok| {
        acc
          + match tok.into_data() {
            Token::Num(n) => n,
            _ => 0,
          }
      },
      2,
    )
  }

  // Only 2 tokens consumed even though there are 4
  let r = ignored_parser!().apply(parse).parse_str("10 20 30 40");
  assert_eq!(r.unwrap(), 30); // 10 + 20
}

#[test]
fn inputref_foldn_zero_limit() {
  // Covers the n >= num exit immediately (n=0, num=0)
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldn(|| 99i64, |acc, _| acc, 0)
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 3");
  assert_eq!(r.unwrap(), 99);
}

#[test]
fn inputref_foldr_within_hits_capacity() {
  // Covers fold.rs line 85: `if buf.len() >= CAPACITY { break; }`
  use generic_arraydeque::typenum::U2;

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldr_within::<_, U2, _, _, _>(
      |t| matches!(t.data(), Token::Num(_)),
      Vec::new,
      |mut acc, tok| {
        if let Token::Num(n) = tok.into_data() {
          acc.push(n);
        }
        acc
      },
    )
  }

  // foldr_within with U2: buffers at most 2 tokens then pops right-to-left
  let r = ignored_parser!().apply(parse).parse_str("10 20 30");
  let nums = r.unwrap();
  // buf fills [10, 20], then pops: 20 first, then 10
  assert_eq!(nums, vec![20, 10]);
}

#[test]
fn inputref_foldrn_hits_limit() {
  // Covers fold.rs line 126: `if n >= num { break; }`
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldrn(
      Vec::new,
      |mut acc, tok| {
        if let Token::Num(n) = tok.into_data() {
          acc.push(n);
        }
        acc
      },
      2,
    )
  }

  // foldrn(2) collects [1,2] from "1 2 3 4", then pops right-to-left: 2 first, then 1
  let r = ignored_parser!().apply(parse).parse_str("1 2 3 4");
  assert_eq!(r.unwrap(), vec![2, 1]);
}

#[test]
fn inputref_foldrn_zero_limit() {
  // Covers fold.rs line 126 immediately (n=0 >= num=0 -> break)
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldrn(Vec::new, |acc, _| acc, 0)
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 3");
  assert_eq!(r.unwrap(), vec![]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// utils/cmp.rs — lines 99-100, 110-111, 130-131, 135
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod cmp_coverage {
  use tokora::utils::cmp::Equivalent;

  #[test]
  fn str_equivalent_to_bytes() {
    // Covers str::equivalent (lines 120-122)
    assert!("hello".equivalent(b"hello" as &[u8]));
    assert!(!"hello".equivalent(b"world" as &[u8]));
  }

  #[test]
  fn bytes_equivalent_to_str() {
    // Covers [u8]::equivalent (lines 130-131)
    assert!(b"hello".as_ref().equivalent("hello"));
    assert!(!b"hello".as_ref().equivalent("world"));
  }

  #[test]
  fn ref_str_equivalent() {
    // Covers &T::Equivalent impl (lines 99-100): &&str -> Equivalent<[u8]>
    let s: &str = "hello";
    assert!((&s).equivalent(b"hello" as &[u8]));
  }

  #[test]
  #[allow(clippy::unnecessary_mut_passed)]
  fn mut_ref_str_equivalent() {
    // Covers &mut T::Equivalent impl (lines 110-111)
    let mut s: &str = "hello";
    assert!((&mut s).equivalent(b"hello" as &[u8]));
  }

  #[test]
  fn ref_bytes_equivalent() {
    // &[u8] via &T impl
    let bytes: &[u8] = b"world";
    assert!(bytes.equivalent("world"));
  }

  #[test]
  fn str_equivalent_to_str() {
    // str ↔ str via AsRef<[u8]>
    assert!("abc".equivalent("abc"));
    assert!(!"abc".equivalent("xyz"));
  }

  #[test]
  fn bytes_equivalent_to_bytes() {
    assert!(b"hello".as_ref().equivalent(b"hello" as &[u8]));
    assert!(!b"hello".as_ref().equivalent(b"world" as &[u8]));
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// utils/positioned_char.rs — lines 254, 256-257, 276, 278-279, 326-327
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod positioned_char_coverage {
  use tokora::utils::PositionedChar;

  #[test]
  fn positioned_char_as_ref() {
    // Covers lines 254, 256-257: as_ref()
    let pc = PositionedChar::with_position('x', 42usize);
    let pc_ref = pc.as_ref();
    assert_eq!(**pc_ref.char_ref(), 'x');
    assert_eq!(*pc_ref.position_ref(), &42usize);
  }

  #[test]
  fn positioned_char_as_mut() {
    // Covers lines 276, 278-279: as_mut()
    let mut pc = PositionedChar::with_position('a', 10usize);
    {
      let mut pc_mut = pc.as_mut();
      **pc_mut.char_mut() = 'b';
    }
    assert_eq!(pc.char(), 'b');
    assert_eq!(pc.position(), 10usize);
  }

  #[test]
  fn positioned_char_display() {
    // Covers lines 326-327: Display impl
    let pc = PositionedChar::with_position('Z', 0usize);
    assert_eq!(format!("{}", pc), "Z");
  }

  #[test]
  fn positioned_char_display_non_ascii() {
    let pc = PositionedChar::with_position('€', 0usize);
    assert_eq!(format!("{}", pc), "€");
  }

  #[test]
  fn positioned_char_as_ref_position_deref() {
    let pc = PositionedChar::with_position('y', 7usize);
    let pc_ref = pc.as_ref();
    assert_eq!(*pc_ref.position_ref(), &7usize);
  }

  #[test]
  fn positioned_char_as_mut_char_ref() {
    // as_mut() returns PositionedChar<&mut Char, &mut Offset>; verify char_ref works
    let mut pc = PositionedChar::with_position('c', 5usize);
    {
      let pc_mut = pc.as_mut();
      // char_ref() on &mut char gives &&mut char; deref once to get &char
      let _ch: &&mut char = pc_mut.char_ref();
    }
    assert_eq!(pc.char(), 'c');
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// utils/mod.rs — additional coverage
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod utils_misc_coverage {
  use tokora::utils::{CharLen, IsAsciiChar, PositionedChar};

  #[test]
  fn is_ascii_char_double_ref() {
    let ch = 'a';
    // &&T -> IsAsciiChar via &T impl chain
    assert!(IsAsciiChar::is_ascii_char(&&&ch, ascii::AsciiChar::a));
    assert!(IsAsciiChar::is_ascii_digit(&&&'5'));
  }

  #[test]
  fn is_ascii_char_mut_ref() {
    let mut ch = 'z';
    assert!(!IsAsciiChar::is_ascii_char(&&mut ch, ascii::AsciiChar::a));
    assert!(!IsAsciiChar::is_ascii_digit(&&mut ch));
  }

  #[test]
  fn one_of_with_mut_ref() {
    let choices = &[ascii::AsciiChar::a];
    let mut ch = 'a';
    assert!(IsAsciiChar::one_of(&&mut ch, choices));
  }

  #[test]
  fn char_len_multibyte() {
    assert_eq!(CharLen::char_len(&'a'), 1);
    assert_eq!(CharLen::char_len(&'é'), 2);
    assert_eq!(CharLen::char_len(&'€'), 3);
    assert_eq!(CharLen::char_len(&'🦀'), 4);
  }

  #[test]
  fn char_len_positioned_char_multibyte() {
    let pc = PositionedChar::with_position('€', 5usize);
    assert_eq!(CharLen::char_len(&pc), 3);
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// lexer/mod.rs — lines 124, 139, 143, 254-257, 302-303
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod lexer_coverage {
  use crate::common::Token;
  use tokora::{
    Lexer,
    lexer::LogosLexer,
    lexer::{IntoLexer, Lexed},
  };

  #[test]
  fn lexed_clone_error_variant() {
    // Covers lexer/mod.rs lines 124-126: Clone for Lexed::Error
    let lexed: Lexed<'_, Token> = Lexed::Error(());
    let cloned = lexed.clone();
    assert!(cloned.is_error());
  }

  #[test]
  fn lexed_clone_token_variant() {
    let lexed: Lexed<'_, Token> = Lexed::Token(Token::Plus);
    let cloned = lexed.clone();
    assert!(cloned.is_token());
  }

  #[test]
  fn lexed_lex_from_lexer() {
    // Covers lexer/mod.rs line 139: Lexed::lex()
    let mut lexer = LogosLexer::<Token>::new("42");
    let result = Lexed::lex(&mut lexer);
    assert!(result.is_some());
    assert!(result.unwrap().is_token());
  }

  #[test]
  fn lexed_lex_spanned_from_lexer() {
    // Covers lexer/mod.rs line 148: Lexed::lex_spanned()
    let mut lexer = LogosLexer::<Token>::new("42");
    let result = Lexed::lex_spanned(&mut lexer);
    assert!(result.is_some());
  }

  #[test]
  fn lexed_lex_returns_none_at_eof() {
    let mut lexer = LogosLexer::<Token>::new("");
    let result = Lexed::<Token>::lex(&mut lexer);
    assert!(result.is_none());
  }

  #[test]
  fn into_lexer_returns_self() {
    // Covers lexer/mod.rs lines 302-303: IntoLexer::into_lexer()
    let lexer = LogosLexer::<Token>::new("1 2 3");
    let _lexer2: LogosLexer<'_, Token> = lexer.into_lexer();
  }

  // Note: Lexed::Display requires T::Error: Display, but our test Token has Error=().
  // The Display impl is exercised via doc-tests in the source. Skipping here.
}

// ═══════════════════════════════════════════════════════════════════════════════
// parser/mod.rs — Parser construction methods
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(()),
    },
    None => Err(()),
  }
}

#[test]
fn parser_new_at_an_explicit_lang() {
  // Covers Parser::new() at an explicit language
  let p: tokora::Parser<(), TestLexer<'_>, i64, _> =
    Parser::new::<'_, TestLexer<'_>, i64, (), ()>();
  let r = p.apply(parse_num).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn parser_with_parser_construction() {
  // Covers Parser::with_parser()
  let r = Parser::with_parser::<'_, TestLexer<'_>, i64, (), _, ()>(parse_num).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn parser_apply_at_an_explicit_lang() {
  // Covers Parser::apply() at an explicit language
  let r: Result<i64, ()> = Parser::new().apply::<_, ()>(parse_num).parse_str("99");
  assert_eq!(r.unwrap(), 99);
}

#[test]
fn parse_trait_default_parse() {
  // Covers Parse::parse() default (delegates to parse_with_state)
  let r = Parser::new().apply(parse_num).parse("42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn parse_trait_parse_str_with_state() {
  // Covers Parse::parse_str_with_state() default
  let r = Parser::new()
    .apply(parse_num)
    .parse_str_with_state("100", ());
  assert_eq!(r.unwrap(), 100);
}

// ── The value-driven entry points ─────────────────────────────────────────────
//
// `parse_num` names neither a lexer instantiation nor an error type beyond `()`, so every
// parameter of each free function is solved from the call: the source value carries `'inp`
// and `S`, `L` follows from `Lexer<'inp, Source = S>`, and `O` / `E` from the annotation.

#[test]
fn free_parse() {
  let r: Result<i64, ()> = tokora::parse(parse_num, "42");
  assert_eq!(r.unwrap(), 42);
}

#[test]
fn free_parse_with_state() {
  let r: Result<i64, ()> = tokora::parse_with_state(parse_num, "100", ());
  assert_eq!(r.unwrap(), 100);
}

#[test]
fn free_parse_with_context() {
  // The context's own emitter decides the error type, so there is nothing to annotate.
  let ctx: IgnoredContext<'_> = IgnoredContext::new(Ignored::default());
  let r = tokora::parse_with(parse_num, "7", ctx);
  assert_eq!(r.unwrap(), 7);
}

#[test]
fn free_parse_reports_the_error() {
  let r: Result<i64, ()> = tokora::parse(parse_num, "");
  assert!(r.is_err());
}

// ── Feature-gated IsAsciiChar impls ──────────────────────────────────────────

#[cfg(feature = "bstr_1")]
mod bstr_coverage {
  use tokora::utils::IsAsciiChar;

  #[test]
  fn bstr_is_ascii_char() {
    let b = bstr_1::BStr::new(b"a");
    assert!(IsAsciiChar::is_ascii_char(b, ascii::AsciiChar::a));
    assert!(!IsAsciiChar::is_ascii_char(b, ascii::AsciiChar::b));
  }

  #[test]
  fn bstr_is_ascii_digit() {
    assert!(IsAsciiChar::is_ascii_digit(bstr_1::BStr::new(b"5")));
    assert!(!IsAsciiChar::is_ascii_digit(bstr_1::BStr::new(b"a")));
    assert!(!IsAsciiChar::is_ascii_digit(bstr_1::BStr::new(b"55")));
  }
}

#[cfg(feature = "bytes_1")]
mod bytes_coverage {
  use tokora::utils::IsAsciiChar;

  #[test]
  fn bytes_is_ascii_char() {
    let b = bytes_1::Bytes::from_static(b"a");
    assert!(IsAsciiChar::is_ascii_char(&b, ascii::AsciiChar::a));
    assert!(!IsAsciiChar::is_ascii_char(&b, ascii::AsciiChar::b));
  }

  #[test]
  fn bytes_is_ascii_digit() {
    let d = bytes_1::Bytes::from_static(b"5");
    assert!(IsAsciiChar::is_ascii_digit(&d));
    let a = bytes_1::Bytes::from_static(b"a");
    assert!(!IsAsciiChar::is_ascii_digit(&a));
  }
}

#[cfg(feature = "hipstr_0_8")]
mod hipstr_coverage {
  use tokora::utils::IsAsciiChar;

  #[test]
  fn hipbyt_is_ascii_char() {
    let b = hipstr_0_8::HipByt::borrowed(b"a");
    assert!(IsAsciiChar::is_ascii_char(&*b, ascii::AsciiChar::a));
    assert!(!IsAsciiChar::is_ascii_char(&*b, ascii::AsciiChar::b));
  }

  #[test]
  fn hipbyt_is_ascii_digit() {
    let d = hipstr_0_8::HipByt::borrowed(b"5");
    assert!(IsAsciiChar::is_ascii_digit(&*d));
    let a = hipstr_0_8::HipByt::borrowed(b"a");
    assert!(!IsAsciiChar::is_ascii_digit(&*a));
  }

  #[test]
  fn hipstr_is_ascii_char() {
    let s = hipstr_0_8::HipStr::borrowed("a");
    assert!(IsAsciiChar::is_ascii_char(&*s, ascii::AsciiChar::a));
    assert!(!IsAsciiChar::is_ascii_char(&*s, ascii::AsciiChar::b));
  }

  #[test]
  fn hipstr_is_ascii_digit() {
    let d = hipstr_0_8::HipStr::borrowed("5");
    assert!(IsAsciiChar::is_ascii_digit(&*d));
    let a = hipstr_0_8::HipStr::borrowed("a");
    assert!(!IsAsciiChar::is_ascii_digit(&*a));
  }
}
