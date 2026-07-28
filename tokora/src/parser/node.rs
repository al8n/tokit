//! The [`node`] combinators — the blessed CST bracketing over the event sink.
//!
//! Wrapping a parser in [`node`] records everything the sub-parse commits — tokens, trivia,
//! nested nodes — as the children of one syntax node of the given kind. The bracketing is
//! **append-only**: entry mints an inert tombstone mark
//! ([`cst_mark`](crate::emitter::CstEmitter::cst_mark)), and only a *successful* exit spends
//! it as a retro-wrap ([`cst_start_at`](crate::emitter::CstEmitter::cst_start_at) +
//! [`cst_finish`](crate::emitter::CstEmitter::cst_finish)). There is therefore **never an
//! open node between entry and exit** — the `labelled` finish-on-both-exits discipline,
//! made structural rather than dutiful:
//!
//! - a **decline** leaves the tombstone unspent — no node, not even an empty one (the
//!   optional-`Description` shape wants exactly this: see [`node_opt`]);
//! - an **error-path unwind** (`?`-propagation out of the sub-parser, or a panic caught by
//!   an enclosing guard) leaves the tombstone unspent — no dangling `Start` for a later
//!   finish to mispair with; the enclosing rollback, if any, truncates the tombstone with
//!   the rest of the abandoned branch;
//! - a **success** wraps precisely the region recorded since entry.
//!
//! # The structural gate
//!
//! Every combinator here bounds `Ctx::Emitter:`[`CstEmitter`] — the one user-ruled gate of
//! the CST design. A diagnostics-only parse (over [`Fatal`](crate::emitter::Fatal),
//! [`Verbose`](crate::emitter::Verbose), …) satisfies the bound through the defaulted no-op
//! event methods, so one parser assembly serves both configurations: tree-less at zero cost
//! (the mark is inert, the wrap calls inline to nothing), or tree-building over a recording
//! sink — by emitter choice alone. A wrapper emitter that does not implement (and forward)
//! [`CstEmitter`] cannot drive a `node`-bearing parser at all: a **compile error**, never a
//! silently empty tree.
//!
//! # Marks are LIFO by construction
//!
//! [`node`] and [`node_opt`] mint their mark and spend it inside one call frame, so nested
//! nodes nest their wraps last-in-first-out structurally — no discipline is asked of the
//! caller. [`node_at`] spends a caller-held mark (the retro-wrap shape: mark, parse a
//! prefix, *then* decide it was the start of something bigger); the recording sink
//! validates every spend — stale marks (a mark whose branch was rolled back) **panic in
//! every build**, and a wrap that would cross another node's boundary is a typed
//! materialization error, never a silently wrong tree.
//!
//! # Trivia lands inside
//!
//! Committed tokens auto-flow to the sink at their settle, so whatever the sub-parser
//! consumes — including trivia skipped by [`padded`](crate::parser::Padded)-style wrappers
//! *inside* it — is recorded between the mark and the wrap and materializes inside the
//! node (the innermost-open-node-at-commit placement).

use crate::{
  Emitter, InputRef, Lexer, ParseContext, ParseInput, TryParseInput, cst::event::EventMark,
  emitter::CstEmitter, try_parse_input::ParseAttempt,
};

/// Wraps `parser` in a syntax node of `kind`: on success, everything the sub-parse
/// committed becomes the node's children; on a decline or an error-path unwind, no node is
/// recorded — the full bracket contract is in the module-level docs above.
///
/// The wrapper implements both [`ParseInput`] and [`TryParseInput`], so it can wrap plain
/// parsers and declining `try_`-parsers alike — a declined attempt leaves no node and, per
/// the decline convention, no consumed input. For the `Option`-shaped optional-node result,
/// see [`node_opt`]; to spend a caller-held mark instead of minting one, see [`node_at`].
///
/// `kind` is a dialect u16 from the unified kind space; the tombstone value
/// ([`TOMBSTONE`](crate::cst::event::TOMBSTONE), `u16::MAX`) is reserved and rejected by
/// recording sinks.
#[inline(always)]
pub const fn node<P>(kind: u16, parser: P) -> Node<P> {
  Node { kind, parser }
}

/// Wraps `parser` in a syntax node of `kind` anchored at the **caller-held** `mark` — the
/// retro-wrap combinator for shapes discovered after their first child was parsed (the
/// `Field` alias: mark, parse a name, and only a following `:` reveals the name began an
/// `Alias`).
///
/// On success, the node wraps everything recorded since `mark` — including whatever the
/// caller committed between minting the mark and running this parser. On a decline or an
/// error-path unwind the mark is left unspent: the caller may spend it later ([`Marker`]
/// is the single-use discipline for that decision tree) or leave it forever — an unspent
/// tombstone materializes into nothing.
///
/// Same-target wraps nest **outward**: wrapping the same mark again (or via
/// [`Marker::precede`](crate::cst::event::Marker)) makes the later wrap the outer node.
///
/// # Panics
///
/// A recording sink panics in every build when `mark` is stale — minted by a rolled-back
/// branch (or by a different sink). Spending a stale mark would wrap an unrelated region:
/// the wrong-tree class nothing downstream can detect, so it is refused at the spend.
///
/// [`Marker`]: crate::cst::event::Marker
#[inline(always)]
pub const fn node_at<P>(mark: EventMark, kind: u16, parser: P) -> NodeAt<P> {
  NodeAt { mark, kind, parser }
}

/// Wraps a declining `try_`-parser in a syntax node of `kind`, yielding `Option`: an
/// accepted attempt becomes `Some` wrapped in the node, a decline becomes `None` with **no
/// node recorded** — the optional-description shape (`Description : StringValue?`), where
/// an absent description must produce no empty `Description` node.
///
/// Equivalent to [`opt`](crate::parser::opt)`(`[`node`]`(kind, parser))` with the attempt
/// shape already adapted away.
#[inline(always)]
pub const fn node_opt<P>(kind: u16, parser: P) -> NodeOpt<P> {
  NodeOpt {
    node: Node { kind, parser },
  }
}

/// The parser wrapper produced by [`node`].
///
/// Delegates to the inner parser between an entry [`cst_mark`] and — on success only — the
/// retro-wrap spend ([`cst_start_at`] + [`cst_finish`]); the module-level docs hold the
/// full contract.
///
/// [`cst_mark`]: crate::emitter::CstEmitter::cst_mark
/// [`cst_start_at`]: crate::emitter::CstEmitter::cst_start_at
/// [`cst_finish`]: crate::emitter::CstEmitter::cst_finish
#[derive(Debug, Clone, Copy)]
pub struct Node<P> {
  kind: u16,
  parser: P,
}

/// The parser wrapper produced by [`node_at`]: [`Node`], anchored at a caller-held mark
/// instead of minting its own.
#[derive(Debug, Clone, Copy)]
pub struct NodeAt<P> {
  mark: EventMark,
  kind: u16,
  parser: P,
}

/// The parser wrapper produced by [`node_opt`]: [`Node`] over a declining parser, with the
/// attempt adapted to `Option`.
#[derive(Debug, Clone, Copy)]
pub struct NodeOpt<P> {
  node: Node<P>,
}

/// Spends `mark` as a node of `kind` wrapping everything recorded since it — the one wrap
/// body all three combinators share.
#[inline(always)]
fn wrap<'inp, L, Ctx, Lang>(
  input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  mark: EventMark,
  kind: u16,
) where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: CstEmitter<'inp, L, Lang>,
  Lang: ?Sized,
{
  let emitter = input.emitter();
  emitter.cst_start_at(mark, kind);
  emitter.cst_finish(kind);
}

// STAYS COMPLETE-ONLY (0.3.0 — the CST seam): Partial event semantics — what a `Sink`
// does with events from a parse that ends `Incomplete` and is then re-driven over a
// grown buffer — is the separately-deferred CST-partial design (ledger: CST spec item 8).
// The `node` family's impls stay pinned at `Complete` so the compiler enforces that
// deferral: no events flow under `Partial` until the event column exists.
impl<'inp, L, O, Ctx, Lang, P> ParseInput<'inp, L, O, Ctx, Lang> for Node<P>
where
  Lang: ?Sized,
  P: ParseInput<'inp, L, O, Ctx, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: CstEmitter<'inp, L, Lang>,
{
  #[inline]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let mark = input.emitter().cst_mark();
    let res = self.parser.parse_input(input);
    if res.is_ok() {
      wrap(input, mark, self.kind);
    }
    res
  }
}

impl<'inp, L, O, Ctx, Lang, P> TryParseInput<'inp, L, O, Ctx, Lang> for Node<P>
where
  Lang: ?Sized,
  P: TryParseInput<'inp, L, O, Ctx, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: CstEmitter<'inp, L, Lang>,
{
  #[inline]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let mark = input.emitter().cst_mark();
    let res = self.parser.try_parse_input(input);
    if matches!(res, Ok(ParseAttempt::Accept(_))) {
      wrap(input, mark, self.kind);
    }
    res
  }
}

impl<'inp, L, O, Ctx, Lang, P> ParseInput<'inp, L, O, Ctx, Lang> for NodeAt<P>
where
  Lang: ?Sized,
  P: ParseInput<'inp, L, O, Ctx, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: CstEmitter<'inp, L, Lang>,
{
  #[inline]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let res = self.parser.parse_input(input);
    if res.is_ok() {
      wrap(input, self.mark, self.kind);
    }
    res
  }
}

impl<'inp, L, O, Ctx, Lang, P> TryParseInput<'inp, L, O, Ctx, Lang> for NodeAt<P>
where
  Lang: ?Sized,
  P: TryParseInput<'inp, L, O, Ctx, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: CstEmitter<'inp, L, Lang>,
{
  #[inline]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    let res = self.parser.try_parse_input(input);
    if matches!(res, Ok(ParseAttempt::Accept(_))) {
      wrap(input, self.mark, self.kind);
    }
    res
  }
}

impl<'inp, L, O, Ctx, Lang, P> ParseInput<'inp, L, Option<O>, Ctx, Lang> for NodeOpt<P>
where
  Lang: ?Sized,
  P: TryParseInput<'inp, L, O, Ctx, Lang>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: CstEmitter<'inp, L, Lang>,
{
  #[inline]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<Option<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    self.node.try_parse_input(input).map(Option::from)
  }
}
