//! The [`node`] combinators — the blessed CST bracketing over the event sink.
//!
//! Wrapping a parser in [`node`] records everything the sub-parse commits — tokens, trivia,
//! nested nodes — as the children of one syntax node of the given kind. Every combinator here
//! is a **both-exits bracket**; they differ only in *when the node's kind is named*, and that
//! split is measured rather than stylistic.
//!
//! # Two brackets, one contract
//!
//! **Up front — [`Node`] as a [`ParseInput`].** This impl cannot decline, so the kind is known
//! at entry: it emits [`cst_start`](crate::emitter::CstEmitter::cst_start) immediately and
//! closes on both exits — [`cst_finish`](crate::emitter::CstEmitter::cst_finish) on success,
//! [`cst_demote`](crate::emitter::CstEmitter::cst_demote) (spending the mark `cst_start` handed
//! back) on failure. An open node therefore exists **only inside the frame**, and both exits
//! close it. Two events per node instead of three, and no tombstone whose only purpose is to be
//! named later.
//!
//! **Retro — every other impl.** [`Node`] as a [`TryParseInput`], [`node_at`] in both shapes,
//! and [`node_opt`] mint an inert tombstone mark
//! ([`cst_mark`](crate::emitter::CstEmitter::cst_mark)) and spend it as a retro-wrap
//! ([`cst_start_at`](crate::emitter::CstEmitter::cst_start_at) + `cst_finish`) only on a
//! successful exit, so **no node is ever open between entry and exit**. That is not a leftover:
//! a declining parser's kind is not knowable at entry (a decline must leave *nothing*, and an
//! up-front start would have to be demoted on a path that also has to consume nothing), and
//! [`node_at`]'s whole purpose is a kind decided after its first child was parsed. The
//! retro path is also what the pratt driver runs on, where the kind is a function of an
//! operator not yet read.
//!
//! Both shapes are the `labelled` finish-on-both-exits discipline made structural rather than
//! dutiful, and on every non-success **return** — a decline or an `Err` — the two shapes leave
//! the identical event buffer:
//!
//! - a **decline** leaves the tombstone unspent — no node, not even an empty one (the
//!   optional-`Description` shape wants exactly this: see [`node_opt`]);
//! - an **error-path unwind** (`?`-propagation out of the sub-parser) leaves an inert
//!   tombstone — the retro bracket by never spending its mark, the up-front bracket by
//!   demoting its start back to one — so there is no dangling `Start` for a later finish to
//!   mispair with; the enclosing rollback, if any, truncates that slot with the rest of the
//!   abandoned branch;
//! - a **success** wraps precisely the region recorded since entry. The buffers differ here
//!   and always did — three events for the retro shape, two for the up-front one — and
//!   converge only at materialization, which hoists a retro-wrap to its target and builds the
//!   same tree from either;
//! - a **panic** is the one non-success exit outside this equivalence, and the only one where
//!   the two shapes' residues genuinely differ. See *The panic residue* below.
//!
//! # The panic residue
//!
//! A panic is the one exit the up-front bracket does not close, and it is deliberately left to
//! the machinery that already handles it rather than given a mechanism here:
//!
//! - a panic caught by an **enclosing guard** unwinds through that guard's rewind, which
//!   truncates the buffer to a mark at or below the `cst_start` — the node goes with the
//!   branch, exactly as the tombstone does today (the rewind-mid-unwind clause);
//! - a panic escaping **every** guard leaves the node open. Nothing observes that until
//!   materialization, which refuses the leftover in the typed way it already refuses any
//!   imbalance (`cst::FinishError::UnclosedNodes`), while
//!   [`finish_partial`](crate::cst::Cst::finish_partial) closes it per its existing contract.
//!   A wrong tree is not reachable through this door; a typed refusal is.
//!
//! The delta versus the retro shape, stated plainly rather than left to be inferred, because
//! this is a **behaviour change** and not merely a documented residue. Under the retro bracket
//! an escaped panic left an unspent tombstone: `finish()` could return a tree that silently
//! omitted the interrupted frame whenever byte coverage happened to be complete (and
//! `UncoveredGap` when it was not), and `finish_partial` produced no node for it either. Under
//! the up-front bracket `finish()` **refuses** — `UnclosedNodes`, always, for any interrupted
//! frame — and `finish_partial` materialises the interrupted frame's node containing everything
//! recorded up to the panic, which is that door's published closes-open-nodes contract answering
//! exactly what a host that caught a panic and asked for a partial tree requested. The strict
//! door therefore got *stricter*, not looser. Both halves are pinned by
//! `a_start_left_open_by_an_escaping_panic_is_refused_by_finish_and_closed_by_finish_partial`
//! in the sink suite.
//!
//! # The structural gate
//!
//! Every combinator here bounds `Ctx::Emitter:`[`CstEmitter`] — the one user-ruled gate of
//! the CST design. A diagnostics-only parse (over [`Fatal`](crate::emitter::Fatal),
//! [`Verbose`](crate::emitter::Verbose), …) satisfies the bound through the defaulted no-op
//! event methods, so one parser assembly serves both configurations: tree-less at zero cost
//! (every mark is inert, the start/finish/wrap/demote calls inline to nothing and the
//! handback is a dead `Copy` constant), or tree-building over a recording sink — by emitter
//! choice alone. A wrapper emitter that does not implement (and forward)
//! [`CstEmitter`] cannot drive a `node`-bearing parser at all: a **compile error**, never a
//! silently empty tree.
//!
//! # Marks are LIFO by construction
//!
//! [`node`] and [`node_opt`] mint their mark and spend it inside one call frame, so nested
//! nodes nest last-in-first-out structurally — no discipline is asked of the caller.
//! [`node_at`] spends a caller-held mark (the retro-wrap shape: mark, parse a
//! prefix, *then* decide it was the start of something bigger); the recording sink
//! validates every spend — stale marks (a mark whose branch was rolled back) **panic in
//! every build**, and a wrap that would cross another node's boundary is a typed
//! materialization error, never a silently wrong tree. The up-front bracket's handback is
//! validated on the same wall: a demote of a stale, foreign, already-**demoted** or
//! wrong-kind mark panics in every build too, as does a demote naming the reserved tombstone
//! kind. The one misuse that wall cannot see is a demote of a start whose node was already
//! finished — the slot cannot witness a finish appended above it — and that one is caught at
//! the misuse site in debug builds and refused at materialization in release, typed and
//! through both doors; see
//! [`cst_demote`](crate::emitter::CstEmitter::cst_demote) for why it can never be a silently
//! wrong tree.
//!
//! # Trivia lands inside
//!
//! Committed tokens auto-flow to the sink at their settle, so whatever the sub-parser
//! consumes — including trivia skipped by [`padded`](crate::parser::Padded)-style wrappers
//! *inside* it — is recorded between the bracket's two events and materializes inside the
//! node (the innermost-open-node-at-commit placement). The two brackets place trivia
//! identically: materialization hoists a retro-wrap to its target, so the node opens at the
//! mark's slot either way.

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
/// Delegates to the inner parser inside a both-exits bracket whose shape depends on which
/// trait drives it — the up-front [`cst_start`] / [`cst_finish`] | [`cst_demote`] pair as a
/// [`ParseInput`], the retro [`cst_mark`] / [`cst_start_at`] pair as a [`TryParseInput`],
/// because a declining parser's kind is not knowable at entry. The module-level docs hold the
/// full contract.
///
/// [`cst_start`]: crate::emitter::CstEmitter::cst_start
/// [`cst_demote`]: crate::emitter::CstEmitter::cst_demote
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
    // The UP-FRONT bracket: this impl cannot decline, so the node's kind is knowable at
    // entry and the retro-wrap's third event — the tombstone that exists only to be named
    // later — is not needed. See the module docs for why the *other* three impls keep it.
    let mark = input.emitter().cst_start(self.kind);
    let res = self.parser.parse_input(input);
    match &res {
      Ok(_) => input.emitter().cst_finish(self.kind),
      Err(_) => input.emitter().cst_demote(mark, self.kind),
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
