#![cfg(all(feature = "rowan", feature = "std", feature = "combinators"))]

//! The `node()` combinator contract over a real recording sink, driven through the public
//! lossless parse entry (`parse_lossless`, which mints the sink from the source it parses):
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
  InputRef, Lexer, Parse, ParseInput, Parser, SimpleSpan, Token, TryParseInput,
  cache::DefaultCache,
  cst::{CstProfile, FinishError, parse_lossless},
  emitter::Verbose,
  error::token::UnexpectedToken,
  parser::{
    PrattFoldOp, PrattInfix, PrattLHS, PrattRHS, Precedenced, node, node_at, node_opt, pratt,
  },
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
type Ctx<'inp> = (Sink<'inp>, DefaultCache<'inp, ByteLexer<'inp>>);
type Ir<'inp, 'c> = InputRef<'inp, 'c, ByteLexer<'inp>, Ctx<'inp>, ()>;

/// The fixture's kind space: the structural kinds above plus the `100 + byte` token images.
fn in_kind_space(kind: u16) -> bool {
  matches!(kind, K_ROOT..=K_BIN | K_ERR | K_GAP) || kind >= 100
}

fn profile() -> CstProfile<Tok> {
  CstProfile::new(
    map_tok,
    tokora::cst::KindValidator::new(in_kind_space),
    K_ERR,
    K_GAP,
  )
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

/// Runs `parser` over `src` through the lossless driver, then materializes.
fn run<O>(
  src: &str,
  parser: impl for<'c> FnMut(&mut Ir<'_, 'c>) -> Result<O, TestErr>,
) -> (
  Result<O, TestErr>,
  Result<rowan::GreenNode, tokora::cst::FinishError>,
) {
  let (cst, res) = parse_lossless(
    src,
    (),
    Verbose::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    parser,
  );
  let (green, _emitter) = cst.finish(K_ROOT);
  (res, green)
}

// ── Little parsers ──────────────────────────────────────────────────────────────

/// Consumes exactly one token.
fn take_one(inp: &mut Ir<'_, '_>) -> Result<(), TestErr> {
  match inp.next()? {
    Some(_) => Ok(()),
    None => Err(TestErr::Boom),
  }
}

/// Consumes exactly two tokens.
fn take_two(inp: &mut Ir<'_, '_>) -> Result<(), TestErr> {
  take_one(inp)?;
  take_one(inp)
}

/// Declines unless the next token is `x`; consumes it when it is.
fn try_x(inp: &mut Ir<'_, '_>) -> Result<ParseAttempt<()>, TestErr> {
  Ok(match inp.try_expect(|t| t.data().0 == b'x')? {
    Some(_) => ParseAttempt::Accept(()),
    None => ParseAttempt::Decline,
  })
}

/// Consumes one token, then fails.
fn boom_after_one(inp: &mut Ir<'_, '_>) -> Result<(), TestErr> {
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
  let (cst, res) = parse_lossless(
    "ab",
    (),
    Verbose::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    |inp: &mut Ir<'_, '_>| node(K_NODE, boom_after_one).parse_input(inp),
  );
  assert_eq!(res, Err(TestErr::Boom));
  let (green, _emitter) = cst.finish_partial(K_ROOT);
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
    node(K_NODE, |inp: &mut Ir<'_, '_>| {
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
  fn straight(inp: &mut Ir<'_, '_>) -> Result<(), TestErr> {
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
    let mark = inp.cst_mark();
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
    let mark = inp.cst_mark();
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

fn pratt_lhs(inp: &mut Ir<'_, '_>) -> Result<PrattLHS<i64, (), i64>, TestErr> {
  match inp.next()? {
    Some(tok) if tok.data().0.is_ascii_digit() => {
      Ok(PrattLHS::Operand(i64::from(tok.data().0 - b'0')))
    }
    _ => Err(TestErr::Unexpected),
  }
}

fn pratt_rhs(inp: &mut Ir<'_, '_>) -> Result<PrattRHS<u8, u8, u8, (), i64>, TestErr> {
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
  _: &mut Ir<'_, '_>,
  operand: i64,
  _: Precedenced<(), i64>,
) -> Result<i64, TestErr> {
  Ok(operand)
}

fn pratt_fold_infix(
  _: &mut Ir<'_, '_>,
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
  _: &mut Ir<'_, '_>,
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
  let parser = pratt(
    pratt_lhs,
    pratt_rhs,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  )
  .with_cst_kinds(bin_kinds);
  let (cst, res) = parse_lossless(
    "1+2*3",
    (),
    Verbose::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    parser,
  );
  assert_eq!(res, Ok(7), "the folds still fold");

  let (green, _emitter) = cst.finish(K_ROOT);
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
  let parser = pratt(
    pratt_lhs,
    pratt_rhs,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  )
  .with_cst_kinds(bin_kinds);
  let (cst, res) = parse_lossless(
    "1=2=3",
    (),
    Verbose::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    parser,
  );
  assert_eq!(
    res,
    Err(TestErr::Unexpected),
    "the repeated `=` fails the parse"
  );

  // The unconsumed handback (`=3`) is tiled rather than reported as a gap: what this cell is
  // about is which *nodes* exist, not who covers the tail.
  let (green, _emitter) = cst.finish_partial(K_ROOT);
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
  let parser = pratt(
    pratt_lhs,
    pratt_rhs,
    pratt_fold_prefix,
    pratt_fold_infix,
    pratt_fold_postfix,
  );
  let (cst, res) = parse_lossless(
    "1+2*3",
    (),
    Verbose::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    parser,
  );
  assert_eq!(res, Ok(7));

  let (green, _emitter) = cst.finish(K_ROOT);
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
// The gap-licensing door: a caller-raised lexer error is a diagnostic, not evidence
// ═══════════════════════════════════════════════════════════════════════════════
//
// `finish`'s gap-coverage law tiles a source byte no committed token covers only where a
// **recorded lexer-error diagnostic** covers it — that span is the one thing that licenses a
// gap, and licensing is *structural*: it decides whether the tree round-trips or the
// materialization is refused. So the span has to come from the same place the token spans do —
// the input layer, over bytes it lexed and refused — and from nowhere else.
//
// Two doors reach the recording sink's `Emitter::emit_lexer_error` with a **caller-chosen** span
// and no consumption behind it: the handle's forwarder (`InputRef::emit_lexer_error`, and
// `ParseState`'s re-export of it) and `EmitterView`'s, which is what a callback holds and what a
// downstream crate can wrap under the orphan rules. That is the exact shape the deleted
// `CstEmitter::cst_token` had on the token channel — a span the caller picks, nothing consumed —
// and both cells below pin it shut: the report still reaches the diagnostic log, and it licenses
// nothing.
//
// Both cells are **falsifiable**: each asserts the refusal that the base branch did *not*
// produce. Run against the pre-fix tree, each returned `Ok` with the uncovered byte tiled as
// `K_GAP` — the plausible tree the coverage law exists to refuse.
//
// **And it is two doors, not three.** `finish`'s verdict is a function of exactly three things —
// the recorded `Token` spans, the recorded `Diag` spans, and the bound source's length — so the
// question "what else can reach coverage?" is answerable by inspection rather than by hope. Every
// other span-carrying method on the two forwarding surfaces records `error_span: None`:
// `emit_unexpected_token`, `emit_error`, `emit_warning`, `emit_skipped_region`, and the twelve
// capability channels (`emit_too_few` / `too_many` / `full_container`, the six separator members,
// `emit_unclosed`, and the two pratt end-of-operand reports). `emit_skipped_region` is the one
// with a *structural* effect besides its slot — it brackets already-buffered token events in an
// `error_kind` node — and that effect adds no token and no span, so it moves no byte's coverage.
// `cst_start` / `cst_finish` / `cst_mark` / `cst_start_at` carry a kind and no span at all.
// COVERAGE_EVIDENCE_CENSUS (`src/cst/sink/tests.rs`) is the standing form of that enumeration.

/// The unconsumed byte both cells attack: `run` drives `ab`, consumes only `a`, and `b` is a
/// perfectly lexable token the parse simply never took. `finish` must call that out.
fn consume_only_the_first(inp: &mut Ir<'_, '_>) -> Result<(), TestErr> {
  take_one(inp)
}

/// **The handle route.** A parser that raises a lexer error over bytes it never consumed must
/// not thereby license them: `inp.emit_lexer_error(span)` is a *report*, and the report is
/// delivered — but `finish` still refuses the uncovered `b`.
///
/// Falsified by an `Ok` tree (the pre-fix verdict) or by a missing diagnostic.
#[test]
fn a_handle_raised_lexer_error_reports_without_licensing_the_gap() {
  let (cst, res) = parse_lossless(
    "ab",
    (),
    Verbose::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    |inp: &mut Ir<'_, '_>| {
      consume_only_the_first(inp)?;
      // The attack: a caller-chosen span over the byte the parse is about to walk away from.
      inp.emit_lexer_error(tokora::span::Spanned::new(
        SimpleSpan::new(1usize, 2),
        LexErr,
      ))
    },
  );
  assert_eq!(res, Ok(()), "the parse itself succeeds — that is the trap");

  let (green, emitter) = cst.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the caller's report licenses nothing"),
    FinishError::UncoveredGap { start: 1, end: 2 },
    "a caller-chosen span must not excuse a byte no token covers"
  );
  assert_eq!(
    emitter.errors().len(),
    1,
    "the capability is intact: the report still reached the diagnostic log"
  );
}

type ViewOfSink<'a, 'inp> = tokora::EmitterView<'a, 'inp, ByteLexer<'inp>, Sink<'inp>>;
type ByteSpan<'inp> = <ByteLexer<'inp> as Lexer<'inp>>::Span;
type ByteLexErr<'inp> = <<ByteLexer<'inp> as Lexer<'inp>>::Token as Token<'inp>>::Error;

/// The orphan impl the crate documents as reachable: foreign trait, foreign `Self`, and the
/// **local** `ByteLexer` in the parameter list with no uncovered type parameter before it. It
/// forwards the four members the trait requires onto the view and inherits every default —
/// including `bound_source`, which is the point.
struct Orphan<'a, 'inp>(ViewOfSink<'a, 'inp>);

impl<'inp> tokora::Emitter<'inp, ByteLexer<'inp>> for Orphan<'_, 'inp> {
  type Error = TestErr;

  fn emit_lexer_error(
    &mut self,
    err: tokora::span::Spanned<ByteLexErr<'inp>, ByteSpan<'inp>>,
  ) -> Result<(), TestErr> {
    self.0.emit_lexer_error(err)
  }

  fn emit_unexpected_token(
    &mut self,
    err: tokora::error::token::UnexpectedTokenOf<'inp, ByteLexer<'inp>, ()>,
  ) -> Result<(), TestErr> {
    self.0.emit_unexpected_token(err)
  }

  fn emit_error(
    &mut self,
    err: tokora::span::Spanned<TestErr, ByteSpan<'inp>>,
  ) -> Result<(), TestErr> {
    self.0.emit_error(err)
  }

  fn rewind(&mut self, _cursor: &tokora::input::Cursor<'inp, '_, ByteLexer<'inp>>, _ckp: u64) {}
}

type OrphanCtx<'a, 'inp> = (Orphan<'a, 'inp>, DefaultCache<'inp, ByteLexer<'inp>>);

/// The foreign parse's whole grammar: drain it, and let the input layer raise the refusals.
fn drain_foreign<'inp>(
  fin: &mut InputRef<'inp, '_, ByteLexer<'inp>, OrphanCtx<'_, 'inp>, ()>,
) -> Result<(), TestErr> {
  while let Ok(Some(_)) = fin.next() {}
  Ok(())
}

/// **The orphan-view route**, end to end, from outside the crate.
///
/// `EmitterView` implements no emitter trait *in this crate*, but a downstream crate whose own
/// lexer appears in the parameter list may implement one for it (`ByteLexer` is local here, and
/// no uncovered type parameter precedes it) and install that wrapper as the emitter of a
/// **second parse over a foreign buffer**. The second parse's own input layer then raises
/// genuine lexer errors — whose spans are offsets into the *foreign* buffer — and forwards them
/// through the view into the original sink.
///
/// The foreign buffer `z!` is chosen so its refused byte sits at offset `1..2`, exactly where the
/// original source's unconsumed `b` sits. Pre-fix, that foreign error licensed the original
/// source's byte and `finish` returned a plausible `ab` tree. It must not.
///
/// `bound_source` is deliberately **not** forwarded — the documented residue of the runtime
/// handshake — so the second parse's attach-time source check sees an emitter that binds no
/// source and says nothing. That is what makes this route reachable at all, and why the wall has
/// to be the one below.
#[test]
fn an_orphan_view_wrapper_carries_a_foreign_lexer_error_but_licenses_no_gap() {
  let (cst, res) = parse_lossless(
    "ab",
    (),
    Verbose::new(),
    profile(),
    DefaultCache::<ByteLexer<'_>>::default(),
    |inp: &mut Ir<'_, '_>| {
      consume_only_the_first(inp)?;
      // The one public door onto the view, taken from inside the original parse.
      let (_peeked, view) = inp.peek_with_emitter::<tokora::utils::typenum::U1>()?;
      // A second parse, over a buffer this sink never saw, emitting into it. Drained: `z`
      // lexes, `!` does not — and the layer reports the refusal itself, with its own span,
      // through the wrapper and into the original sink.
      let _ = tokora::parser::parse_with(
        drain_foreign,
        "z!",
        (Orphan(view), DefaultCache::<ByteLexer<'_>>::default()),
      );
      Ok(())
    },
  );
  assert_eq!(res, Ok(()), "the parse itself succeeds — that is the trap");

  let (green, emitter) = cst.finish(K_ROOT);
  assert_eq!(
    green.expect_err("a foreign buffer's refusal licenses nothing here"),
    FinishError::UncoveredGap { start: 1, end: 2 },
    "the original source's `b` is covered by no token and explained by no lexer error of \
     its own"
  );
  assert_eq!(
    emitter.errors().len(),
    1,
    "the foreign diagnostic still arrived: the channel is forwarded, not severed"
  );
}
