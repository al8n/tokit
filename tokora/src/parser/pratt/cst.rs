//! The typed pratt driver's CST hook: [`with_cst_kinds`](super::Pratt::with_cst_kinds) and
//! its plumbing.
//!
//! # The driver holds the mark
//!
//! A pratt expression's extent only ever *grows to the right*: the driver parses the
//! left-hand side, then folds operators onto it one loop iteration at a time. The one
//! position that names "where this expression began" is therefore known **before** anything
//! is parsed — so the driver mints one [`EventMark`] there and spends it once per fold, and
//! the fold hooks stay exactly what they were (AST builders that never see the event
//! channel). The RHS loop's per-iteration transactions roll back only regions *younger*
//! than the mark (an abandoned operator peek), so the mark stays live for the whole
//! expression by construction.
//!
//! Same-target wraps nest **inside-out**: each fold appends a later
//! [`cst_start_at`](crate::emitter::CstEmitter::cst_start_at) naming the same tombstone,
//! and same-target wraps open in reverse buffer order at materialization — the last fold
//! (the outermost application) becomes the outermost node. `1 + 2 * 3` therefore
//! materializes as `Bin[1, +, Bin[2, *, 3]]`: the recursive right-operand parse takes its
//! *own* mark at `2` and wraps the multiplication before the outer fold wraps the addition.
//!
//! # Token-level pratt is CST-unsupported
//!
//! Only the typed driver ([`Pratt`](super::Pratt), via
//! [`pratt`](fn@super::pratt)) carries this hook. The
//! token-level API ([`InputRef::pratt`](crate::InputRef::pratt)) folds expressions into
//! *synthetic tokens* — spans that cover already-folded regions with no kind seam to
//! classify — and is documented CST-unsupported in this version.

use crate::{
  InputRef, Lexer, ParseContext,
  cst::event::EventMark,
  emitter::CstEmitter,
  input::{Complete, Completeness},
  parser::PrattInfix,
};

/// The operator a pratt fold is about to apply, by reference — the input of the
/// [`with_cst_kinds`](super::Pratt::with_cst_kinds) classifier.
///
/// One variant per fold shape, borrowing the operator the driver parsed, so the classifier
/// can pick a node kind per operator (or return `None` to record no node for that fold —
/// exotic dialects can then wrap manually through the raw
/// [`CstEmitter`] surface inside their fold hooks instead).
#[derive(Debug)]
pub enum PrattFoldOp<'op, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp> {
  /// A prefix application is about to fold; borrows the prefix operator.
  Prefix(&'op PreOp),
  /// An infix application is about to fold; borrows the infix operator with its
  /// associativity.
  Infix(&'op PrattInfix<LeftAssoc, RightAssoc, NeitherAssoc>),
  /// A postfix application is about to fold; borrows the postfix operator.
  Postfix(&'op PostOp),
}

/// The classifier [`with_cst_kinds`](super::Pratt::with_cst_kinds) takes: maps each fold's
/// operator to the node kind that should wrap the folded region, or `None` for no node.
///
/// A plain `fn` pointer on purpose: the hook adds no generic parameter beyond the operator
/// types the driver already carries, so a configured [`Pratt`](super::Pratt) stays nameable.
pub type PrattCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp> =
  fn(PrattFoldOp<'_, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>) -> Option<u16>;

mod sealed {
  pub trait Sealed {}
}

/// The typed driver's CST seam, threaded through the pratt parse loop: mint the
/// driver-held mark at expression entry, classify each fold's operator, and wrap the
/// folded region.
///
/// **Sealed.** Exactly two implementations exist, and they are the whole design:
///
/// - [`NoCst`] — the default; every method is an inlined no-op with **no bound beyond the
///   core emitter**, so an unconfigured pratt parser compiles and runs exactly as it did
///   before this seam existed, over any emitter;
/// - [`WithCstKinds`] — the configured hook; its implementation is where the
///   `Ctx::Emitter: CstEmitter` bound lives, so only a parse that *asked* for tree events
///   requires the event channel. A non-CST emitter simply cannot drive a
///   kinds-configured pratt parser — a compile error, never a silently empty tree.
pub trait PrattCst<
  'inp,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  Ctx,
  Lang,
  Cmpl = Complete,
>: sealed::Sealed where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  /// Mints the driver-held mark at expression entry (`None` when no hook is configured).
  fn mark(inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>) -> Option<EventMark>;

  /// Classifies the operator a fold is about to apply into the node kind that should wrap
  /// the folded region (`None` records no node).
  fn classify(
    &self,
    op: PrattFoldOp<'_, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>,
  ) -> Option<u16>;

  /// Wraps the region recorded since `mark` in a node of `kind` — a retro-wrap
  /// (`cst_start_at` + `cst_finish`) spending the driver-held mark once per fold.
  fn wrap_at(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    mark: Option<EventMark>,
    kind: Option<u16>,
  );
}

/// The inert CST seam — the default `Cst` parameter of [`Pratt`](super::Pratt): no mark, no
/// classification, no wrap, no bound beyond the core emitter. Zero-cost: every method
/// inlines to nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCst;

impl sealed::Sealed for NoCst {}

impl<'inp, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>
  PrattCst<'inp, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang> for NoCst
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lang: ?Sized,
{
  #[inline(always)]
  fn mark(_: &mut InputRef<'inp, '_, L, Ctx, Lang>) -> Option<EventMark> {
    None
  }

  #[inline(always)]
  fn classify(
    &self,
    _: PrattFoldOp<'_, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>,
  ) -> Option<u16> {
    None
  }

  #[inline(always)]
  fn wrap_at(_: &mut InputRef<'inp, '_, L, Ctx, Lang>, _: Option<EventMark>, _: Option<u16>) {}
}

/// The configured CST seam produced by [`with_cst_kinds`](super::Pratt::with_cst_kinds):
/// holds the dialect's fold-to-kind classifier and carries the
/// `Ctx::Emitter: CstEmitter` bound — the structural gate that makes a kinds-configured
/// pratt parser refuse a non-CST emitter at compile time.
#[derive(Debug, Clone, Copy)]
pub struct WithCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp> {
  kinds: PrattCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>,
}

impl<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>
  WithCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>
{
  /// Wraps the classifier.
  #[inline(always)]
  pub(crate) const fn new(
    kinds: PrattCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>,
  ) -> Self {
    Self { kinds }
  }
}

impl<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp> sealed::Sealed
  for WithCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>
{
}

impl<'inp, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>
  PrattCst<'inp, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>
  for WithCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: CstEmitter<'inp, L, Lang>,
  Lang: ?Sized,
{
  #[inline(always)]
  fn mark(inp: &mut InputRef<'inp, '_, L, Ctx, Lang>) -> Option<EventMark> {
    Some(inp.emitter().cst_mark())
  }

  #[inline(always)]
  fn classify(
    &self,
    op: PrattFoldOp<'_, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>,
  ) -> Option<u16> {
    (self.kinds)(op)
  }

  #[inline(always)]
  fn wrap_at(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    mark: Option<EventMark>,
    kind: Option<u16>,
  ) {
    if let (Some(mark), Some(kind)) = (mark, kind) {
      let emitter = inp.emitter();
      emitter.cst_start_at(mark, kind);
      emitter.cst_finish(kind);
    }
  }
}
