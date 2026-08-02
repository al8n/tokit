#![cfg(all(feature = "rowan", feature = "std"))]

//! The `node()` combinator contract over a real recording sink, driven through the public
//! parse entry with the caller-held-sink threading (`&mut Sink` in the context seat):
//!
//! - a decline leaves **no node** (not even an empty one);
//! - an error-path unwind leaves **no dangling start** — materialization stays balanced;
//! - nested nodes nest LIFO;
//! - the backtrack-equivalence seed: a decline-then-retry drive materializes the exact
//!   green tree of the straight drive;
//! - the pratt driver's `with_cst_kinds` hook: `1+2*3` materializes as properly nested
//!   binary-expression nodes, folds untouched.

use core::fmt;

use tokora::{
  Emitter, InputRef, Lexer, Parse, ParseInput, Parser, SimpleSpan, Token, TryParseInput,
  cache::DefaultCache,
  cst::FinishError,
  emitter::{CstEmitter, Verbose},
  error::token::{UnexpectedToken, UnexpectedTokenOf},
  input::Cursor,
  parser::{
    PrattFoldOp, PrattInfix, PrattLHS, PrattRHS, Precedenced, node, node_at, node_opt, pratt,
  },
  span::Spanned,
  try_parse_input::ParseAttempt,
};

// ── A tiny real lexer: one byte per token ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tok(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LexErr;

impl Token<'_> for Tok {
  type Kind = u8;
  type Error = LexErr;

  // honest: byte-per-token, never skips a byte
  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> u8 {
    self.0
  }

  fn is_trivia(&self) -> bool {
    self.0 == b' '
  }
}

struct ByteLexer<'inp> {
  src: &'inp str,
  tok_start: usize,
  pos: usize,
  state: (),
}

impl<'inp> Lexer<'inp> for ByteLexer<'inp> {
  type State = ();
  type Source = str;
  type Token = Tok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp str) -> Self {
    Self {
      src,
      tok_start: 0,
      pos: 0,
      state: (),
    }
  }

  fn with_state(src: &'inp str, state: ()) -> Self {
    Self {
      src,
      tok_start: 0,
      pos: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), LexErr> {
    Ok(())
  }

  fn state(&self) -> &Self::State {
    &self.state
  }

  fn state_mut(&mut self) -> &mut Self::State {
    &mut self.state
  }

  fn into_state(self) -> Self::State {
    self.state
  }

  fn source(&self) -> &'inp str {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.tok_start, self.pos)
  }

  fn slice(&self) -> &'inp str {
    &self.src[self.tok_start..self.pos]
  }

  fn lex(&mut self) -> Option<Result<Tok, LexErr>> {
    let byte = *self.src.as_bytes().get(self.pos)?;
    self.tok_start = self.pos;
    self.pos += 1;
    if byte == b'!' {
      Some(Err(LexErr))
    } else {
      Some(Ok(Tok(byte)))
    }
  }

  fn bump(&mut self, n: &usize) {
    self.pos += *n;
    self.tok_start = self.pos;
  }
}

// ── Error plumbing ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestErr {
  Lex,
  Unexpected,
  Boom,
}

impl fmt::Display for TestErr {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{self:?}")
  }
}

impl From<LexErr> for TestErr {
  fn from(_: LexErr) -> Self {
    Self::Lex
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for TestErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Self::Unexpected
  }
}

// The two end-of-input members of `FromTokenErrors` as one `Set`-generic impl, plus the
// delimiter half. This fixture's grammar delimits nothing, but an error type absorbs the
// whole token-level set or it is not an error type.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>>
  for TestErr
{
  fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self {
    Self::Unexpected
  }
}

// The typed pratt driver's stalled-report exits, one per report channel.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoLhs<O, Lang, Set>>
  for TestErr
{
  fn from(_: tokora::error::UnexpectedEoLhs<O, Lang, Set>) -> Self {
    Self::Unexpected
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoRhs<O, Lang, Set>>
  for TestErr
{
  fn from(_: tokora::error::UnexpectedEoRhs<O, Lang, Set>) -> Self {
    Self::Unexpected
  }
}

impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for TestErr {
  fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self {
    Self::Unexpected
  }
}

impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for TestErr {
  fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self {
    Self::Unexpected
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for TestErr
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    Self::Unexpected
  }
}

// ── Dialect fixture: the unified kind space, the mapper, the tree reader ────────

const K_ROOT: u16 = 1;
const K_NODE: u16 = 2;
const K_INNER: u16 = 3;
const K_LIST: u16 = 4;
const K_WRAP: u16 = 5;
const K_BIN: u16 = 6;
const K_ERR: u16 = 90;
const K_GAP: u16 = 91;

/// Token images: `100 + byte`, so every token kind is distinct and node kinds cannot
/// collide with them.
fn map_tok(t: &Tok) -> u16 {
  100 + t.0 as u16
}

type Sink<'inp> = tokora::cst::Sink<'inp, ByteLexer<'inp>, Verbose<TestErr>>;
type Ctx<'inp, 's> = (&'s mut Sink<'inp>, DefaultCache<'inp, ByteLexer<'inp>>);
type Ir<'inp, 's, 'c> = InputRef<'inp, 'c, ByteLexer<'inp>, Ctx<'inp, 's>, ()>;

/// The fixture's kind space: the structural kinds above plus the `100 + byte` token images.
fn in_kind_space(kind: u16) -> bool {
  matches!(kind, K_ROOT..=K_BIN | K_ERR | K_GAP) || kind >= 100
}

fn sink(src: &str) -> Sink<'_> {
  let profile = tokora::cst::CstProfile::new(
    map_tok,
    tokora::cst::KindValidator::new(in_kind_space),
    K_ERR,
    K_GAP,
  );
  tokora::cst::Sink::new(src, Verbose::new(), profile)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum RawLang {}

impl rowan::Language for RawLang {
  type Kind = u16;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> u16 {
    raw.0
  }

  fn kind_to_raw(kind: u16) -> rowan::SyntaxKind {
    rowan::SyntaxKind(kind)
  }
}

fn tree(green: rowan::GreenNode) -> rowan::SyntaxNode<RawLang> {
  rowan::SyntaxNode::new_root(green)
}

/// Runs `parser` over `src` with a caller-held sink in the context seat, then materializes.
fn run<O>(
  src: &str,
  parser: impl for<'c> FnMut(&mut Ir<'_, '_, 'c>) -> Result<O, TestErr>,
) -> (
  Result<O, TestErr>,
  Result<rowan::GreenNode, tokora::cst::FinishError>,
) {
  let mut s = sink(src);
  let res =
    Parser::with_parser_and_context(parser, (&mut s, DefaultCache::<ByteLexer<'_>>::default()))
      .parse_str(src);
  let (green, _emitter) = s.finish(K_ROOT);
  (res, green)
}

// ── Little parsers ──────────────────────────────────────────────────────────────

/// Consumes exactly one token.
fn take_one(inp: &mut Ir<'_, '_, '_>) -> Result<(), TestErr> {
  match inp.next()? {
    Some(_) => Ok(()),
    None => Err(TestErr::Boom),
  }
}

/// Consumes exactly two tokens.
fn take_two(inp: &mut Ir<'_, '_, '_>) -> Result<(), TestErr> {
  take_one(inp)?;
  take_one(inp)
}

/// Declines unless the next token is `x`; consumes it when it is.
fn try_x(inp: &mut Ir<'_, '_, '_>) -> Result<ParseAttempt<()>, TestErr> {
  Ok(match inp.try_expect(|t| t.data().0 == b'x')? {
    Some(_) => ParseAttempt::Accept(()),
    None => ParseAttempt::Decline,
  })
}

/// Consumes one token, then fails.
fn boom_after_one(inp: &mut Ir<'_, '_, '_>) -> Result<(), TestErr> {
  take_one(inp)?;
  Err(TestErr::Boom)
}

// ═══════════════════════════════════════════════════════════════════════════════
// node()
// ═══════════════════════════════════════════════════════════════════════════════

/// The basic bracket: everything the sub-parse committed becomes the node's children,
/// and the round-trip law holds.
#[test]
fn node_wraps_the_committed_region() {
  let (res, green) = run("ab", |inp| node(K_NODE, take_two).parse_input(inp));
  assert_eq!(res, Ok(()));
  let green = green.expect("balanced by construction");
  let root = tree(green);
  assert_eq!(root.text().to_string(), "ab");
  let node = root.first_child().expect("Root[Node]");
  assert_eq!(node.kind(), K_NODE);
  assert_eq!(node.text().to_string(), "ab");
  assert_eq!(node.children_with_tokens().count(), 2, "both tokens inside");
}

/// A decline records no node — not an empty one, none at all — and consumes nothing:
/// the following parser sees the token and the tree holds it loose.
#[test]
fn node_decline_leaves_no_node() {
  let (res, green) = run("a", |inp| {
    let attempt = node(K_NODE, try_x).try_parse_input(inp)?;
    assert!(
      matches!(attempt, ParseAttempt::Decline),
      "the sub-parser declines on `a`"
    );
    take_one(inp)
  });
  assert_eq!(res, Ok(()));
  let root = tree(green.expect("nothing dangles"));
  assert_eq!(root.text().to_string(), "a");
  assert_eq!(root.children().count(), 0, "no node was recorded");
  assert_eq!(
    root.children_with_tokens().count(),
    1,
    "the declined token was consumed by the follower, loose in the root"
  );
}

/// An error-path unwind leaves no dangling start: the buffer stays balanced. `finish`'s
/// balance check precedes its gap-coverage check, so a dangling start would surface as
/// `UnclosedNodes`; instead the only complaint is the unconsumed `b` — `UncoveredGap`. That
/// ordering *is* the no-dangling-start proof. The committed prefix plus the tiled tail then
/// round-trip through the tooling door (`finish_partial`), with no node recorded.
#[test]
fn node_error_unwind_leaves_no_dangling_start() {
  let (res, green) = run("ab", |inp| node(K_NODE, boom_after_one).parse_input(inp));
  assert_eq!(res, Err(TestErr::Boom));
  assert_eq!(
    green.expect_err("balanced buffer: not UnclosedNodes, only the unconsumed `b`"),
    FinishError::UncoveredGap { start: 1, end: 2 }
  );

  // The aborted parse's tree: `finish_partial` tiles the unconsumed `b`, closing nothing
  // (the buffer was already balanced), so the round trip holds with no node.
  let mut s = sink("ab");
  let res = Parser::with_parser_and_context(
    |inp: &mut Ir<'_, '_, '_>| node(K_NODE, boom_after_one).parse_input(inp),
    (&mut s, DefaultCache::<ByteLexer<'_>>::default()),
  )
  .parse_str("ab");
  assert_eq!(res, Err(TestErr::Boom));
  let (green, _emitter) = s.finish_partial(K_ROOT);
  let root = tree(green.expect("finish_partial tiles the tail"));
  assert_eq!(
    root.text().to_string(),
    "ab",
    "committed `a` + gap-tiled `b`"
  );
  assert_eq!(root.children().count(), 0, "no node survives the unwind");
}

/// Nested nodes nest last-in-first-out: the inner wrap closes inside the outer one.
#[test]
fn nested_nodes_nest_lifo() {
  let (res, green) = run("abc", |inp| {
    node(K_NODE, |inp: &mut Ir<'_, '_, '_>| {
      take_one(inp)?;
      node(K_INNER, take_one).parse_input(inp)?;
      take_one(inp)
    })
    .parse_input(inp)
  });
  assert_eq!(res, Ok(()));
  let root = tree(green.expect("balanced"));
  assert_eq!(root.text().to_string(), "abc");
  let outer = root.first_child().expect("Root[Node]");
  assert_eq!(outer.kind(), K_NODE);
  assert_eq!(outer.text().to_string(), "abc");
  let inner = outer.first_child().expect("Node[.., Inner, ..]");
  assert_eq!(inner.kind(), K_INNER);
  assert_eq!(inner.text().to_string(), "b");
}

/// The backtrack-equivalence seed: an attempt that consumed, wrapped, and declined leaves
/// the retry timeline byte-identical to the straight drive's — compared as materialized
/// green trees (buffer nonces are route-dependent by design).
#[test]
fn backtrack_equivalence_seed_declined_wrap_vs_straight() {
  fn straight(inp: &mut Ir<'_, '_, '_>) -> Result<(), TestErr> {
    node(K_NODE, take_two).parse_input(inp)
  }

  let (res, green_straight) = run("ab", straight);
  assert_eq!(res, Ok(()));

  let (res, green_backtracked) = run("ab", |inp| {
    let declined: Option<()> = inp.attempt(|inp| {
      // The abandoned branch consumes and wraps a different shape...
      node(K_LIST, take_one).parse_input(inp).ok()?;
      // ...then declines: the wrap, its tokens, and its tombstone all rewind.
      None
    });
    assert!(declined.is_none());
    straight(inp)
  });
  assert_eq!(res, Ok(()));

  assert_eq!(
    green_straight.expect("straight"),
    green_backtracked.expect("backtracked"),
    "same final timeline, byte-identical green trees"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// node_at() / node_opt()
// ═══════════════════════════════════════════════════════════════════════════════

/// The retro-wrap shape: a caller-held mark taken before the first child, spent by
/// `node_at` once the continuation reveals what the child began.
#[test]
fn node_at_wraps_from_the_caller_mark() {
  let (res, green) = run("a:", |inp| {
    let mark = inp.emitter().cst_mark();
    take_one(inp)?; // the name, committed before the wrap is known
    node_at(mark, K_WRAP, take_one).parse_input(inp) // `:` decides: it was an alias
  });
  assert_eq!(res, Ok(()));
  let root = tree(green.expect("balanced"));
  assert_eq!(root.text().to_string(), "a:");
  let wrap = root.first_child().expect("Root[Wrap]");
  assert_eq!(wrap.kind(), K_WRAP);
  assert_eq!(
    wrap.text().to_string(),
    "a:",
    "the wrap reaches back to the mark, over the caller-committed name"
  );
}

/// `node_at` on the error path leaves the caller's mark unspent: no node, balanced buffer.
#[test]
fn node_at_error_unwind_spends_nothing() {
  let (res, green) = run("ab", |inp| {
    let mark = inp.emitter().cst_mark();
    take_one(inp)?;
    node_at(mark, K_WRAP, boom_after_one).parse_input(inp)
  });
  assert_eq!(res, Err(TestErr::Boom));
  let root = tree(green.expect("balanced: the mark was never spent"));
  assert_eq!(root.children().count(), 0, "no wrap on the error path");
  assert_eq!(root.text().to_string(), "ab");
}

/// `node_opt` is the optional-description shape: absent yields `None` and **no** node;
/// present yields `Some` wrapped in the node.
#[test]
fn node_opt_absent_records_nothing_present_wraps() {
  let (res, green) = run("a", |inp| {
    let absent = node_opt(K_NODE, try_x).parse_input(inp)?;
    assert_eq!(absent, None, "no `x`: declined");
    take_one(inp)
  });
  assert_eq!(res, Ok(()));
  let root = tree(green.expect("balanced"));
  assert_eq!(root.children().count(), 0, "no empty optional node");

  let (res, green) = run("xa", |inp| {
    let present = node_opt(K_NODE, try_x).parse_input(inp)?;
    assert_eq!(present, Some(()));
    take_one(inp)
  });
  assert_eq!(res, Ok(()));
  let root = tree(green.expect("balanced"));
  let node = root.first_child().expect("Root[Node, a]");
  assert_eq!(node.kind(), K_NODE);
  assert_eq!(node.text().to_string(), "x");
}

// ═══════════════════════════════════════════════════════════════════════════════
// pratt + with_cst_kinds
// ═══════════════════════════════════════════════════════════════════════════════

const PREC_SUM: i64 = 1;
const PREC_PROD: i64 = 2;

fn pratt_lhs(inp: &mut Ir<'_, '_, '_>) -> Result<PrattLHS<i64, (), i64>, TestErr> {
  match inp.next()? {
    Some(tok) if tok.data().0.is_ascii_digit() => {
      Ok(PrattLHS::Operand(i64::from(tok.data().0 - b'0')))
    }
    _ => Err(TestErr::Unexpected),
  }
}

fn pratt_rhs(inp: &mut Ir<'_, '_, '_>) -> Result<PrattRHS<u8, u8, u8, (), i64>, TestErr> {
  Ok(match inp.next()? {
    Some(tok) => match tok.data().0 {
      op @ b'+' => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(op), PREC_SUM)),
      op @ b'*' => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(op), PREC_PROD)),
      // Non-associative at the sum level, so `1=2=3` is a declared chain the driver refuses.
      op @ b'=' => PrattRHS::Infix(Precedenced::new(PrattInfix::Neither(op), PREC_SUM)),
      _ => PrattRHS::End,
    },
    None => PrattRHS::End,
  })
}

fn pratt_fold_prefix(
  _: &mut Ir<'_, '_, '_>,
  operand: i64,
  _: Precedenced<(), i64>,
) -> Result<i64, TestErr> {
  Ok(operand)
}

fn pratt_fold_infix(
  _: &mut Ir<'_, '_, '_>,
  left: i64,
  right: i64,
  op: Precedenced<PrattInfix<u8, u8, u8>, i64>,
) -> Result<i64, TestErr> {
  let op = match op.into_data() {
    PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op) => op,
  };
  Ok(match op {
    b'+' | b'=' => left + right,
    b'*' => left * right,
    _ => unreachable!(),
  })
}

fn pratt_fold_postfix(
  _: &mut Ir<'_, '_, '_>,
  operand: i64,
  _: Precedenced<(), i64>,
) -> Result<i64, TestErr> {
  Ok(operand)
}

/// Every infix fold wraps as a binary expression; nothing else records a node.
fn bin_kinds(op: PrattFoldOp<'_, (), u8, u8, u8, ()>) -> Option<u16> {
  match op {
    PrattFoldOp::Infix(_) => Some(K_BIN),
    PrattFoldOp::Prefix(_) | PrattFoldOp::Postfix(_) => None,
  }
}

/// The failing-first pratt shape: `1+2*3` materializes as properly nested binary
/// expressions — multiplication inside addition — with the fold hooks untouched (they
/// compute the same `i64` they always did).
#[test]
fn pratt_with_cst_kinds_materializes_nested_bin_exprs() {
  let mut s = sink("1+2*3");
  let parser = pratt(
    pratt_lhs,
    pratt_rhs,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  )
  .with_cst_kinds(bin_kinds);
  let res =
    Parser::with_parser_and_context(parser, (&mut s, DefaultCache::<ByteLexer<'_>>::default()))
      .parse_str("1+2*3");
  assert_eq!(res, Ok(7), "the folds still fold");

  let (green, _emitter) = s.finish(K_ROOT);
  let root = tree(green.expect("driver-held marks balance"));
  assert_eq!(root.text().to_string(), "1+2*3");

  let outer = root.first_child().expect("Root[Bin]");
  assert_eq!(outer.kind(), K_BIN);
  assert_eq!(outer.text().to_string(), "1+2*3");

  let inner = outer.first_child().expect("Bin[1, +, Bin[2, *, 3]]");
  assert_eq!(inner.kind(), K_BIN, "the higher-power fold nests inside");
  assert_eq!(inner.text().to_string(), "2*3");
  assert_eq!(
    inner.children().count(),
    0,
    "the inner expression is tokens only"
  );
}

/// **A non-associative repeat records no node for the aborted cycle, and the folds before it
/// keep theirs.**
///
/// `1=2=3` with `=` non-associative: the driver folds `1=2` (one `K_BIN`), then meets the second
/// `=` at the same power and fails. The failing cycle aborts before `classify`, before the fold
/// and before `wrap_at`, and the probe's rollback erases the events its deciding read recorded —
/// so exactly one node exists. The already-folded cycle survives because the repeat is a
/// *cycle-scoped* exit: the wrapper commits the expression guard rather than rolling it back.
#[test]
fn pratt_with_cst_kinds_records_no_node_for_a_non_associative_repeat() {
  let mut s = sink("1=2=3");
  let parser = pratt(
    pratt_lhs,
    pratt_rhs,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  )
  .with_cst_kinds(bin_kinds);
  let res =
    Parser::with_parser_and_context(parser, (&mut s, DefaultCache::<ByteLexer<'_>>::default()))
      .parse_str("1=2=3");
  assert_eq!(
    res,
    Err(TestErr::Unexpected),
    "the repeated `=` fails the parse"
  );

  // The unconsumed handback (`=3`) is tiled rather than reported as a gap: what this cell is
  // about is which *nodes* exist, not who covers the tail.
  let (green, _emitter) = s.finish_partial(K_ROOT);
  let root = tree(green.expect("driver-held marks balance"));
  assert_eq!(root.text().to_string(), "1=2=3");
  let kids: Vec<_> = root.children().collect();
  assert_eq!(
    kids.len(),
    1,
    "one node for the surviving fold, none for the aborted cycle"
  );
  assert_eq!(kids[0].kind(), K_BIN);
  // *A gap is tiled where it opens*: the handback's uncovered run opens the instant `2` settles,
  // and `2` settled inside the surviving `K_BIN`, so the run tiles there and the node widens over
  // it — `1=2=3`, not `1=2`. That is placement, not folding: the fold still covers exactly the
  // three tokens it folded, which is what the aborted cycle recording no node means here.
  assert_eq!(kids[0].text().to_string(), "1=2=3");
  let folded: String = kids[0]
    .children_with_tokens()
    .filter_map(|el| el.into_token())
    .filter(|t| t.kind() != K_GAP)
    .map(|t| t.text().to_string())
    .collect();
  assert_eq!(
    folded, "1=2",
    "the surviving fold's own tokens are `1=2`; the trailing gap it houses is not one of them"
  );
}

/// The unconfigured driver stays tree-silent over a recording sink: no `with_cst_kinds`,
/// no expression nodes — only the committed tokens flow (and here nothing flows into
/// nodes at all).
#[test]
fn pratt_without_cst_kinds_records_no_nodes() {
  let mut s = sink("1+2*3");
  let parser = pratt(
    pratt_lhs,
    pratt_rhs,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  );
  let res =
    Parser::with_parser_and_context(parser, (&mut s, DefaultCache::<ByteLexer<'_>>::default()))
      .parse_str("1+2*3");
  assert_eq!(res, Ok(7));

  let (green, _emitter) = s.finish(K_ROOT);
  let root = tree(green.expect("token-only timelines balance trivially"));
  assert_eq!(root.text().to_string(), "1+2*3");
  assert_eq!(root.children().count(), 0, "no nodes without the hook");
}

/// One assembly, two configurations: the same `node`-bearing parser drives over a plain
/// diagnostics emitter (no sink anywhere) through the defaulted no-op event channel.
#[test]
fn node_over_a_diagnostics_only_emitter_is_inert() {
  use tokora::FatalContext;

  fn parser<'inp>(
    inp: &mut InputRef<'inp, '_, ByteLexer<'inp>, FatalContext<'inp, ByteLexer<'inp>, TestErr>, ()>,
  ) -> Result<(), TestErr> {
    node(
      K_NODE,
      |inp: &mut InputRef<
        'inp,
        '_,
        ByteLexer<'inp>,
        FatalContext<'inp, ByteLexer<'inp>, TestErr>,
        (),
      >| {
        match inp.next()? {
          Some(_) => Ok(()),
          None => Err(TestErr::Boom),
        }
      },
    )
    .parse_input(inp)
  }

  let res = Parser::with_parser(parser).parse_str("a");
  assert_eq!(
    res,
    Ok(()),
    "the inert event channel costs nothing and changes nothing"
  );
}
// ═══════════════════════════════════════════════════════════════════════════════
// The partial-forwarding hole: structuring forwarded, token channel severed
// ═══════════════════════════════════════════════════════════════════════════════

/// The half-forwarding wrapper of the partial-forwarding hole: the generic emitter
/// wrapper a downstream author writes by forwarding every **required** [`Emitter`]
/// method plus the backtracking trio (the compiler and the parse demand those) and the
/// whole [`CstEmitter`] structuring surface (the `node()` bound demands that) — while
/// inheriting the defaulted no-op [`Emitter::commit_token`], which severs the
/// auto-emission token channel even though every structuring event still flows.
struct HalfForward<E> {
  inner: E,
}

impl<'a, L, E> Emitter<'a, L> for HalfForward<E>
where
  L: Lexer<'a>,
  E: Emitter<'a, L>,
{
  type Error = E::Error;

  /// Forwarded deliberately, so this fixture models a wrapper that drops *tokens* and nothing
  /// else. One fixture, one defect.
  ///
  /// Omitting it would additionally make this a wrapper that hides its inner sink's bound
  /// source — a **separate, and currently open, residue**: the parse-entry seam check can only
  /// compare an answer it receives, so a hidden binding reads as "this emitter binds no
  /// source" and the pairing is accepted. Nothing downstream refuses it, which is exactly why
  /// a fixture must not carry that defect by accident while claiming to test another. See the
  /// four pinned residues in `cst::sink::tests`.
  fn bound_source(&self) -> Option<tokora::source::SourceIdentity> {
    self.inner.bound_source()
  }

  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.inner.emit_lexer_error(err)
  }

  fn emit_unexpected_token(&mut self, err: UnexpectedTokenOf<'a, L>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.inner.emit_unexpected_token(err)
  }

  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.inner.emit_error(err)
  }

  fn checkpoint(&self) -> u64 {
    self.inner.checkpoint()
  }

  fn rewind(&mut self, cursor: &Cursor<'a, '_, L>, checkpoint: u64)
  where
    L: Lexer<'a>,
  {
    self.inner.rewind(cursor, checkpoint)
  }

  fn release(&mut self, checkpoint: u64) {
    self.inner.release(checkpoint)
  }

  // `commit_token` is NOT forwarded: the wrapper inherits the core no-op default, and
  // every committed token silently vanishes between the input layer and the sink.
}

impl<'a, L, E> CstEmitter<'a, L> for HalfForward<E>
where
  L: Lexer<'a>,
  E: CstEmitter<'a, L>,
{
  fn cst_start(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_start(kind)
  }

  fn cst_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_token(tok, span)
  }

  fn cst_finish(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_finish(kind)
  }

  fn cst_mark(&mut self) -> tokora::cst::event::EventMark
  where
    L: Lexer<'a>,
  {
    self.inner.cst_mark()
  }

  fn cst_start_at(&mut self, mark: tokora::cst::event::EventMark, kind: u16)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_start_at(mark, kind)
  }
}

type HfCtx<'s, 'inp> = (
  HalfForward<&'s mut Sink<'inp>>,
  DefaultCache<'inp, ByteLexer<'inp>>,
);
type HfIr<'inp, 's, 'c> = InputRef<'inp, 'c, ByteLexer<'inp>, HfCtx<'s, 'inp>, ()>;

/// Consumes exactly two tokens through the half-forwarding context.
fn hf_take_two(inp: &mut HfIr<'_, '_, '_>) -> Result<(), TestErr> {
  for _ in 0..2 {
    match inp.next()? {
      Some(_) => {}
      None => return Err(TestErr::Boom),
    }
  }
  Ok(())
}

/// The finding's failing-first regression: a wrapper that forwards the structuring
/// surface but not the committed-token hook must not produce a *silently plausible*
/// materialization. The parse succeeds and records structure, every committed token is
/// dropped between the input layer and the sink, and `finish` — the success door —
/// refuses with a typed error instead of returning a gap-tiled tree with empty nodes.
#[test]
fn half_forwarding_wrapper_is_refused_at_finish() {
  let mut s = sink("ab");
  let res = Parser::with_parser_and_context(
    |inp: &mut HfIr<'_, '_, '_>| node(K_NODE, hf_take_two).parse_input(inp),
    (
      HalfForward { inner: &mut s },
      DefaultCache::<ByteLexer<'_>>::default(),
    ),
  )
  .parse_str("ab");
  assert_eq!(res, Ok(()), "the parse itself succeeds — that is the trap");

  let (green, _emitter) = s.finish(K_ROOT);
  let err = match green {
    Err(err) => err,
    Ok(tree_ok) => panic!(
      "finish must refuse the severed token channel, got a plausible tree: {:?}",
      tree(tree_ok).text()
    ),
  };
  assert!(
    matches!(err, FinishError::StructureWithoutTokens),
    "expected StructureWithoutTokens, got {err:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The PARTIAL-forwarding hole: structure + SOME tokens forwarded, others severed
// ═══════════════════════════════════════════════════════════════════════════════

/// The partial-forwarding sibling of [`HalfForward`]: it forwards the structuring surface
/// and the **first** committed token, then severs the channel. Because a token *does*
/// survive, the zero-token wall ([`FinishError::StructureWithoutTokens`]) cannot fire —
/// the dropped tokens vanish as uncovered source bytes instead, the signature only the
/// gap-coverage law ([`FinishError::UncoveredGap`]) catches.
struct HalfForwardPartial<E> {
  inner: E,
  forwarded: usize,
}

impl<'a, L, E> Emitter<'a, L> for HalfForwardPartial<E>
where
  L: Lexer<'a>,
  E: Emitter<'a, L>,
{
  type Error = E::Error;

  /// Forwarded deliberately, so this fixture models a wrapper that drops *tokens* and nothing
  /// else. One fixture, one defect.
  ///
  /// Omitting it would additionally make this a wrapper that hides its inner sink's bound
  /// source — a **separate, and currently open, residue**: the parse-entry seam check can only
  /// compare an answer it receives, so a hidden binding reads as "this emitter binds no
  /// source" and the pairing is accepted. Nothing downstream refuses it, which is exactly why
  /// a fixture must not carry that defect by accident while claiming to test another. See the
  /// four pinned residues in `cst::sink::tests`.
  fn bound_source(&self) -> Option<tokora::source::SourceIdentity> {
    self.inner.bound_source()
  }

  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.inner.emit_lexer_error(err)
  }

  fn emit_unexpected_token(&mut self, err: UnexpectedTokenOf<'a, L>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.inner.emit_unexpected_token(err)
  }

  fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.inner.emit_error(err)
  }

  fn checkpoint(&self) -> u64 {
    self.inner.checkpoint()
  }

  fn rewind(&mut self, cursor: &Cursor<'a, '_, L>, checkpoint: u64)
  where
    L: Lexer<'a>,
  {
    self.inner.rewind(cursor, checkpoint)
  }

  fn release(&mut self, checkpoint: u64) {
    self.inner.release(checkpoint)
  }

  // The partial sever: the first committed token flows, every later one is dropped — so a
  // token survives (the zero-token wall stays silent) but its peers become uncovered bytes.
  fn commit_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'a>,
  {
    if self.forwarded < 1 {
      self.forwarded += 1;
      self.inner.commit_token(tok, span);
    }
  }
}

impl<'a, L, E> CstEmitter<'a, L> for HalfForwardPartial<E>
where
  L: Lexer<'a>,
  E: CstEmitter<'a, L>,
{
  fn cst_start(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_start(kind)
  }

  fn cst_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_token(tok, span)
  }

  fn cst_finish(&mut self, kind: u16)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_finish(kind)
  }

  fn cst_mark(&mut self) -> tokora::cst::event::EventMark
  where
    L: Lexer<'a>,
  {
    self.inner.cst_mark()
  }

  fn cst_start_at(&mut self, mark: tokora::cst::event::EventMark, kind: u16)
  where
    L: Lexer<'a>,
  {
    self.inner.cst_start_at(mark, kind)
  }
}

type HfpCtx<'s, 'inp> = (
  HalfForwardPartial<&'s mut Sink<'inp>>,
  DefaultCache<'inp, ByteLexer<'inp>>,
);
type HfpIr<'inp, 's, 'c> = InputRef<'inp, 'c, ByteLexer<'inp>, HfpCtx<'s, 'inp>, ()>;

/// Consumes exactly two tokens through the partial-forwarding context.
fn hfp_take_two(inp: &mut HfpIr<'_, '_, '_>) -> Result<(), TestErr> {
  for _ in 0..2 {
    match inp.next()? {
      Some(_) => {}
      None => return Err(TestErr::Boom),
    }
  }
  Ok(())
}

/// The finding's failing-first regression (RED→GREEN): a wrapper that forwards structure and
/// only *some* committed tokens leaves the survivors in the tree and the dropped ones as
/// uncovered source bytes. The zero-token wall can no longer speak (a token survived), so
/// `finish` — the success door — must catch the loss through the gap-coverage law:
/// `UncoveredGap` over exactly the dropped token's bytes, not a plausible gap-tiled tree.
#[test]
fn partial_forwarding_wrapper_is_refused_at_finish() {
  let mut s = sink("ab");
  let res = Parser::with_parser_and_context(
    |inp: &mut HfpIr<'_, '_, '_>| node(K_NODE, hfp_take_two).parse_input(inp),
    (
      HalfForwardPartial {
        inner: &mut s,
        forwarded: 0,
      },
      DefaultCache::<ByteLexer<'_>>::default(),
    ),
  )
  .parse_str("ab");
  assert_eq!(res, Ok(()), "the parse itself succeeds — that is the trap");

  let (green, _emitter) = s.finish(K_ROOT);
  let err = match green {
    Err(err) => err,
    Ok(tree_ok) => panic!(
      "finish must refuse the dropped token, got a plausible tree: {:?}",
      tree(tree_ok).text()
    ),
  };
  assert!(
    matches!(err, FinishError::UncoveredGap { start: 1, end: 2 }),
    "expected UncoveredGap {{ start: 1, end: 2 }} over the dropped 'b', got {err:?}"
  );
}
