use core::marker::PhantomData;

use crate::{
  Commit, Rollback,
  cache::PeekedTokenExt as _,
  error::{NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs},
  span::Span as _,
};

use super::*;

/// Creates a pratt parser.
///
/// `Lang` is read off the input, so an unbranded grammar writes the same call as a branded one.
#[inline(always)]
pub fn pratt<
  'inp,
  Power,
  Lhs,
  Rhs,
  FoldPrefix,
  FoldInfix,
  FoldPostfix,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  O,
  Ctx,
  Lang,
>(
  parse_lhs: Lhs,
  parse_rhs: Rhs,
  fold_prefix: FoldPrefix,
  fold_infix: FoldInfix,
  fold_postfix: FoldPostfix,
) -> Pratt<
  Power,
  Lhs,
  Rhs,
  FoldPrefix,
  FoldInfix,
  FoldPostfix,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  O,
  Ctx,
  Lang,
>
where
  Lhs: ParsePrattLHS<'inp, Power, O, PreOp, L, Ctx, Lang>,
  Rhs: ParsePrattRHS<'inp, Power, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>,
  Power: PrattPower,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
{
  Pratt::new(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix)
}

/// A Pratt parser combinator.
///
/// Built via [`pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)`](pratt) and configured
/// with `.prefix(...)`, `.postfix(...)`, `.infix(...)`, and `.min_precedence(...)` methods.
///
/// The trailing `Cst` parameter is the CST seam, [`NoCst`] (inert, zero-cost) unless
/// [`with_cst_kinds`](Self::with_cst_kinds) configures a fold-to-kind classifier — see
/// that method for the driver-held-mark contract.
///
/// # Time and space
///
/// **Retained checkpoints are O(1) in the expression's operator depth.** A checkpoint is not
/// two words: it carries the cursor, the committed span, a clone of `L::State`, three offsets
/// and an emitter mark — `O(size_of::<L::State>())` apiece, and a lexer with a large state pays
/// for every one that is live. This driver used to hold one per *live operator cycle*: the
/// cycle's guard was opened before the operator was even classified and stayed open across the
/// recursive operand parse, so a right-associative chain of depth `d` pinned `d` of them at
/// once. It now holds at most **two**: the one expression-scoped guard
/// [`parse_input`](ParseInput::parse_input) opens, plus at most one operator probe — and the
/// probe is committed the instant the operator is accepted, *before* the recursion. Depth costs
/// no checkpoints. A grammar that nests expressions inside expressions adds one expression guard
/// per nesting level of the *grammar*, which is a property of the grammar and not of the input.
///
/// **The space costs O(1) checkpoint work per expression.** Each RHS cycle still performs
/// exactly one save/settle pair — the probe — as before, and the expression-scoped guard adds
/// **one more**, paid once per expression whether or not the expression has any operators at
/// all: a bare operand under this driver now takes a `save`/settle pair it did not take before.
/// Total checkpoint work therefore stays linear in the number of operator probes, with one extra
/// pair per expression on top. Nothing is captured twice and nothing is re-captured on the way
/// back up; the narrowing moves where a capture is *released*, and adds exactly that one pair.
///
/// ## Why the extra pair is unavoidable, and what it costs
///
/// **The obvious fix — take the checkpoint only once an operator is known to exist — has no point
/// in the code to run at.** The guard's restore target is the position *before the left-hand
/// side*, and a checkpoint of that position can only be taken before the LHS channel is entered:
/// `parse_input` takes it at `begin_with::<Rollback>()`, before `parse_lhs` runs, before anything
/// is known about whether the expression has an operator at all. There is no later point that is
/// both after "an operator exists" is known and before that operator has been consumed, so the
/// pair cannot be opened lazily. Nor can the target be reconstructed afterwards: `L::State` has no
/// inverse, the emitter is write-only, and the dedup watermarks only advance.
///
/// So a zero-operator expression pays one full `save`/settle pair it will not use: **+122
/// instructions (bench profile) / +109 (release)** for a 64-byte `L::State`, against ~895 for the
/// whole expression — **13.6% / 12.3%**. Break-even against the per-operator retention saving is
/// **1.4–1.6 operators**; at two operators and above the driver is ahead. Roughly a third of the
/// pair is the `L::State` clone and scales with it; half is the input layer's per-guard pin-set
/// and lineage bookkeeping, which every guard in the crate pays, and which the pre-narrowing
/// driver already paid once per expression for its probe.
///
/// Two figures make that checkable rather than assertable. The guard's own clone is one of the
/// **two** retained checkpoints this type guarantees at every depth: removing it reds seven cells
/// in `tests/pratt_txn_retention.rs`, including the suite's two exact-value probes —
/// `retained_state_clones_do_not_grow_with_operator_depth` and
/// `peak_live_states_are_independent_of_operator_depth` — which fall to **1** retained and **3**
/// peak against the required **2** and **4**. And a raw `save`/`commit` pair whose checkpoint is
/// never restored costs only **+39**: the clones are dead and get deleted; it is restorability
/// across an unwind that costs the rest.
///
/// **Native stack use is still O(d), and the paragraph above is not a depth bound.** A
/// right-associative chain descends one native frame per operator exactly as before; only
/// checkpoint *retention* was narrowed. The depth bound is a separate mechanism and lives one
/// layer down: the driver enters each frame through
/// [`InputRef::descend`](crate::InputRef::descend), so every live frame holds one level of the
/// input's shared [recursion budget](crate::state::recursion_tracker::RecursionLimiter) — depth
/// **500** unless the context says otherwise — and a deeper expression fails with the terminal
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) rather than exhausting the
/// native stack. The budget belongs to the *input*, not to this parser, so nested expression
/// parsers share it.
///
/// **Rollback granularity**, which the narrowing makes two scopes rather than one:
///
/// - **cycle-scoped**, for the five exits where the deciding read is handed back untouched:
///   [`PrattRHS::End`], a report the floor declines, either report-boundary stall (an admitted
///   `Infix`/`Postfix` report that consumed nothing), and the non-associative repeat — which is
///   the one of the five that ends the *parse* rather than the expression, with the operator left
///   on the input and [`NonAssociativeChain`](crate::error::NonAssociativeChain) returned. The
///   probe guard is still live for all five, so whatever the RHS parser consumed while deciding
///   is handed back untouched to the surrounding grammar.
/// - **expression-scoped**, for an **unwind** that crosses the driver while the guard is
///   undecided — through either channel, any of the three folds, the emitter, the CST seam or
///   the recursive operand parse that carries them — and for the foot-of-cycle refusal (a fold or
///   a recursion that rewound *behind* the operator it was handed). Both restore the input to
///   before the whole expression — **wider** than the per-cycle rollback they replace, and the
///   width is not free. A panic in a fold no longer leaves "the operator consumed, the right-hand
///   side absent" behind; it leaves nothing of the expression behind at all — and the
///   expression's emissions go with its input. That is a **contract**, stated in full in the next
///   section, not an incidental. The expression guard is [`Rollback`](crate::Rollback) policy, so
///   **both** restores hold identically in `std` and `no_std` builds; they do not inherit the
///   unwind-edge divergence [`Commit`](crate::Commit) documents, which is why the guard is not
///   that policy.
///
/// **The driver raises no panic of its own inside that window**, which is what keeps the two
/// scopes above a property of the *code* rather than of the build profile. Its four contract
/// assertions — the two report-boundary stalls, the prefix stall and the foot-of-cycle refusal —
/// all fire in the wrapper, *after* the expression guard has been settled: the stalls after it
/// commits, the refusal after it rolls back. A `debug_assert` raised where the violation is
/// detected would instead unwind through an undecided guard, so a debug build would take the
/// whole expression back on a path where a release build keeps it and returns the terminal error.
/// That would be a third expression-scoped restore, present in one profile only, and the section
/// below says there are two.
///
/// # Contract: an expression-scoped restore takes the expression's emissions with it
///
/// **Everything emitted before this parser was entered survives. Everything this parser emitted
/// after that — diagnostics *and* CST events alike, the left-hand side's and every already-folded
/// cycle's included — is discarded on the two exits that restore the whole expression, and on
/// those two only.** Those two are the ones listed above: an unwind that crosses the driver while
/// the guard is undecided, and the foot-of-cycle refusal. Every other exit **commits**: `Ok`,
/// every `?`-propagation out of the LHS channel, the RHS channel or a fold, and the
/// non-associative repeat, keep every emission the expression made. The cycle-scoped exits
/// discard only what the RHS parser emitted while deciding a report this expression does not fold
/// — an [`End`](PrattRHS::End), an operator the floor declines, a non-associative repeat, or a
/// report that consumed nothing — which is the same narrow discard any speculative parse makes,
/// and not this contract.
///
/// **Two in every build**, and that is part of the promise rather than a detail of it. The three
/// report-boundary contract violations — a `Prefix` report that consumed nothing, and its
/// `Infix`/`Postfix` twin in either arm — are reported as a *terminal error* in release and
/// additionally raise a `debug_assert` in a debug build. That assertion fires in the wrapper,
/// after the expression guard has committed, so the debug build's extra panic is not an extra
/// restore: both profiles keep the expression's input and emissions on those three exits, and
/// differ only in whether the violation reaches the caller as a panic or as the error. A caller
/// catching that unwind reads the same log a release caller reads from the `Err`. Pinned by the
/// three `..._keeps_the_expression_in_both_profiles` cells in `tests/pratt_txn_retention.rs`,
/// which assert the emission log and the handback with the same expected values on both sides of
/// the profile split and so fail the moment a driver-raised panic moves back inside the guard.
///
/// Read the trade as stated: a torn half-expression is replaced by an **absent** one, and the
/// emissions that described it go with it. Both exits mean "a grammar hook broke its contract",
/// and the error the second raises is *terminal* — no recoverer may fabricate a value from it — so
/// the expression is void either way; what the surrounding grammar is handed is a clean
/// pre-expression position with a clean log, which is what it needs to try a different production
/// over the same bytes without inheriting the void expression's complaints.
///
/// ## Why it cannot be narrower: emissions rewind *with* the position, always
///
/// The obvious repair — restore the position to before the expression but rewind the emitter only
/// as far as the failing probe, keeping the left-hand side's diagnostics — is not available, and
/// not for want of effort. Emission scope is not an independent knob; it is a **function of**
/// rollback scope. Four facts, each mechanical:
///
/// 1. **One checkpoint, one restore group.** A [`Checkpoint`](crate::input::Checkpoint) carries
///    the cursor, the committed span, the lexer state, the poison boundary, the two dedup
///    watermarks **and** the emitter mark, and the restore installs all of them in a single block.
///    There is no verb that takes a position from one checkpoint and an emitter mark from another;
///    the hybrid it would build is the same tear the crate already refuses on the session-point
///    drop path.
/// 2. **One emitter mark, every channel.** [`Emitter::rewind`](crate::emitter::Emitter::rewind) is
///    contracted to restore *the whole* emission state at its mark, and
///    [`commit_token`](crate::emitter::Emitter::commit_token) — the auto-emission hook the CST
///    channel is built on — rides that same mark by the same contract ("a speculative branch's
///    tokens rewind with the branch"). Rewinding the emitter to a *later* mark than the position
///    would leave a `Token` event for every token the restore just un-consumed, and the
///    surrounding grammar's re-parse would settle each of them a second time. The trait offers no
///    per-channel mark, so "keep the diagnostics, drop the events" is not expressible against it
///    at all.
/// 3. **The probe's mark is already spent.** The narrowing commits the probe the instant the
///    operator is accepted, and a commit *releases* its emitter mark —
///    [`release`](crate::emitter::Emitter::release) says the mark "will never be rewound to", and
///    the crate's own recording sink reclaims its row on it and panics in debug if a later rewind
///    asks for it back. Holding the mark instead of releasing it is holding the checkpoint, which
///    is the per-depth retention this driver exists to have removed.
/// 4. **Preserving would manufacture duplicates, not save diagnostics.** The dedup watermarks
///    travel in the same checkpoint as the log. Carry the log across the restore and they
///    desynchronise: a surrounding grammar re-parsing the retracted bytes emits that region's
///    lexer diagnostics a *second* time, on top of the preserved copies. And the driver could not
///    replay them selectively even in principle — [`Emitter`](crate::emitter::Emitter) is a
///    write-only surface with no way to read its own log back.
///
/// So the only lever on *which emissions are discarded* is *how much input is retracted*, and that
/// lever is the per-cycle checkpoint the narrowing gave up to keep retention off the operator
/// depth. Narrowing the loss means paying `O(d)` checkpoints again. The trade is named here rather
/// than left for a user to discover from an empty diagnostic list.
///
/// Pinned by `an_expression_rollback_takes_the_expressions_diagnostics_with_it` in
/// `tests/pratt_txn_retention.rs`, which asserts all three edges of the contract — kept on the
/// committing exit, discarded on the restoring one, and the pre-expression log untouched by
/// either — over an emitter whose mark covers diagnostics and token settles together, as a real
/// one does.
///
/// # What the narrowing does NOT keep: crossing-rewind refusal
///
/// A live guard **pins** its begin point, and the pin check is release-active in every allocator
/// build: a checked restore to a target *below* a live pin panics at the restore that caused it,
/// not later. The cycle guard used to pin the cycle base across classify, the recursion, the fold
/// and the CST wrap, so a rewind out of any of those to a target between the expression base and
/// the cycle base was refused loudly, in release. It no longer is: the probe is committed — pin
/// released, lineage entry gone — before that window opens, so within it the only live pin is the
/// expression base, and such a rewind now **succeeds silently**. If the re-parse then nets past
/// the enclosing watermark, no foot-of-cycle check fires anywhere and the parse can complete `Ok`
/// with tokens folded twice.
///
/// Three things bound that, and none of them is the pin:
///
/// - the **RHS channel keeps full coverage** — the probe guard is live and pinned across
///   [`ParsePrattRHS::parse_pratt_rhs`] itself, which is where a speculating operator parser
///   actually rewinds;
/// - under [`with_cst_kinds`](Self::with_cst_kinds) with a recording sink there is a
///   **conditional** backstop: a crossing rewind bumps the event stream's truncation era, and the
///   next `wrap_at` spending the now-stale [`EventMark`](crate::cst::event::EventMark) panics in
///   every build. Conditional, because it needs that frame to actually fold with a `Some` kind;
/// - otherwise the behaviour is **unspecified-but-bounded**, which is already what
///   allocator-less builds do — they keep no pin set at all and never had the refusal.
///
/// The code that trips this is a grammar bug by definition: a fold or an operand parse that
/// rewinds across an operator the driver has already folded. The narrowing trades a loud
/// detect-at-cause refusal of that bug, over one window, for retention that no longer grows with
/// depth. It is written down here rather than glossed as "uniformly broader", which it is not.
pub struct Pratt<
  Power,
  Lhs,
  Rhs,
  FoldPrefix,
  FoldInfix,
  FoldPostfix,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  O,
  Ctx,
  Lang: ?Sized = (),
  Cst = NoCst,
> {
  min_precedence: Power,
  parse_lhs: Lhs,
  parse_rhs: Rhs,
  fold_prefix: FoldPrefix,
  fold_infix: FoldInfix,
  fold_postfix: FoldPostfix,
  cst: Cst,
  _pre_op: PhantomData<PreOp>,
  _post_op: PhantomData<PostOp>,
  _left_assoc: PhantomData<LeftAssoc>,
  _right_assoc: PhantomData<RightAssoc>,
  _neither_assoc: PhantomData<NeitherAssoc>,
  _o: PhantomData<O>,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
}

impl<
  Power,
  Lhs,
  Rhs,
  FoldPrefix,
  FoldInfix,
  FoldPostfix,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  O,
  Ctx,
  Lang: ?Sized,
>
  Pratt<
    Power,
    Lhs,
    Rhs,
    FoldPrefix,
    FoldInfix,
    FoldPostfix,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
  >
{
  pub(crate) fn new<'a>(
    parse_lhs: Lhs,
    parse_rhs: Rhs,
    fold_prefix: FoldPrefix,
    fold_infix: FoldInfix,
    fold_postfix: FoldPostfix,
  ) -> Self
  where
    Lhs: ParsePrattLHS<'a, Power, O, PreOp, L, Ctx, Lang>,
    Rhs: ParsePrattRHS<'a, Power, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>,
    Power: PrattPower,
  {
    Self {
      parse_lhs,
      parse_rhs,
      min_precedence: Power::default(),
      fold_prefix,
      fold_infix,
      fold_postfix,
      cst: NoCst,
      _pre_op: PhantomData,
      _post_op: PhantomData,
      _left_assoc: PhantomData,
      _right_assoc: PhantomData,
      _neither_assoc: PhantomData,
      _o: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }
}

impl<
  Power,
  Lhs,
  Rhs,
  FoldPrefix,
  FoldInfix,
  FoldPostfix,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  O,
  Ctx,
  Lang: ?Sized,
  Cst,
>
  Pratt<
    Power,
    Lhs,
    Rhs,
    FoldPrefix,
    FoldInfix,
    FoldPostfix,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
    Cst,
  >
{
  /// Configure the prefix fold for this Pratt parser.
  pub fn prefix<'inp, F>(
    self,
    folder: F,
  ) -> Pratt<
    Power,
    Lhs,
    Rhs,
    F,
    FoldInfix,
    FoldPostfix,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
    Cst,
  >
  where
    F: PrattFoldPrefix<'inp, Power, PreOp, L, O, Ctx, Lang>,
  {
    Pratt {
      parse_lhs: self.parse_lhs,
      parse_rhs: self.parse_rhs,
      min_precedence: self.min_precedence,
      fold_prefix: folder,
      fold_infix: self.fold_infix,
      fold_postfix: self.fold_postfix,
      cst: self.cst,
      _pre_op: PhantomData,
      _post_op: PhantomData,
      _left_assoc: PhantomData,
      _right_assoc: PhantomData,
      _neither_assoc: PhantomData,
      _o: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Configure the infix fold for this Pratt parser.
  pub fn infix<'inp, F>(
    self,
    folder: F,
  ) -> Pratt<
    Power,
    Lhs,
    Rhs,
    FoldPrefix,
    F,
    FoldPostfix,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
    Cst,
  >
  where
    F: PrattFoldInfix<'inp, Power, LeftAssoc, RightAssoc, NeitherAssoc, L, O, Ctx, Lang>,
  {
    Pratt {
      parse_lhs: self.parse_lhs,
      parse_rhs: self.parse_rhs,
      min_precedence: self.min_precedence,
      fold_prefix: self.fold_prefix,
      fold_infix: folder,
      fold_postfix: self.fold_postfix,
      cst: self.cst,
      _pre_op: PhantomData,
      _post_op: PhantomData,
      _left_assoc: PhantomData,
      _right_assoc: PhantomData,
      _neither_assoc: PhantomData,
      _o: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Configure the postfix fold for this Pratt parser.
  pub fn postfix<'inp, F>(
    self,
    folder: F,
  ) -> Pratt<
    Power,
    Lhs,
    Rhs,
    FoldPrefix,
    FoldInfix,
    F,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
    Cst,
  >
  where
    F: PrattFoldPostfix<'inp, Power, PostOp, L, O, Ctx, Lang>,
  {
    Pratt {
      parse_lhs: self.parse_lhs,
      parse_rhs: self.parse_rhs,
      min_precedence: self.min_precedence,
      fold_prefix: self.fold_prefix,
      fold_infix: self.fold_infix,
      fold_postfix: folder,
      cst: self.cst,
      _pre_op: PhantomData,
      _post_op: PhantomData,
      _left_assoc: PhantomData,
      _right_assoc: PhantomData,
      _neither_assoc: PhantomData,
      _o: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Configure the minimum precedence level for this Pratt parser.
  ///
  /// **Inclusive**: an operator whose binding power is *equal to* `min_precedence` is
  /// consumed; only operators strictly below it are left on the input for the surrounding
  /// grammar. Lowering it is how an expression is embedded in a larger one — but note that
  /// it lowers the bar for *every* report the RHS parser makes, including any it uses to
  /// mean "this is not an operator". Spell that one [`PrattRHS::End`], which is not a power
  /// and which no floor admits.
  pub fn min_precedence(
    self,
    min_precedence: Power,
  ) -> Pratt<
    Power,
    Lhs,
    Rhs,
    FoldPrefix,
    FoldInfix,
    FoldPostfix,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
    Cst,
  > {
    Pratt {
      parse_lhs: self.parse_lhs,
      parse_rhs: self.parse_rhs,
      min_precedence,
      fold_prefix: self.fold_prefix,
      fold_infix: self.fold_infix,
      fold_postfix: self.fold_postfix,
      cst: self.cst,
      _pre_op: PhantomData,
      _post_op: PhantomData,
      _left_assoc: PhantomData,
      _right_assoc: PhantomData,
      _neither_assoc: PhantomData,
      _o: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }

  /// Configure the CST fold-to-kind classifier: every fold this driver applies is then
  /// wrapped in a node whose kind the classifier picks from the operator (`None` records
  /// no node for that fold).
  ///
  /// # The driver holds the mark; the folds stay untouched
  ///
  /// The driver mints **one** [`EventMark`](crate::cst::event::EventMark) before parsing
  /// the expression's left-hand side and spends it once per fold — the fold hooks keep
  /// their exact signatures and never see the event channel. Same-target wraps materialize
  /// inside-out (the later fold is the outer node), and each recursive operand parse holds
  /// its own mark, so `1 + 2 * 3` builds `Bin[1, +, Bin[2, *, 3]]` under a
  /// left-to-right driver. Abandoned operator peeks roll back regions strictly younger
  /// than the mark, so the mark stays live for the whole expression by construction.
  ///
  /// # The structural gate
  ///
  /// The returned driver's [`ParseInput`] implementation requires
  /// `Ctx::Emitter: CstEmitter` — a kinds-configured pratt parser over an emitter without
  /// the event channel is a **compile error**, never a silently tree-less parse. Over a
  /// defaulted no-op [`CstEmitter`](crate::emitter::CstEmitter) the wraps cost nothing;
  /// over a recording sink they build the tree.
  ///
  /// The token-level pratt API ([`InputRef::pratt`](crate::InputRef::pratt)) has no kind
  /// seam and is documented CST-unsupported in this version.
  pub fn with_cst_kinds(
    self,
    kinds: PrattCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>,
  ) -> Pratt<
    Power,
    Lhs,
    Rhs,
    FoldPrefix,
    FoldInfix,
    FoldPostfix,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
    WithCstKinds<PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp>,
  > {
    Pratt {
      parse_lhs: self.parse_lhs,
      parse_rhs: self.parse_rhs,
      min_precedence: self.min_precedence,
      fold_prefix: self.fold_prefix,
      fold_infix: self.fold_infix,
      fold_postfix: self.fold_postfix,
      cst: WithCstKinds::new(kinds),
      _pre_op: PhantomData,
      _post_op: PhantomData,
      _left_assoc: PhantomData,
      _right_assoc: PhantomData,
      _neither_assoc: PhantomData,
      _o: PhantomData,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
    }
  }
}

impl<
  'inp,
  Power,
  Lhs,
  Rhs,
  FoldPrefix,
  FoldInfix,
  FoldPostfix,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  O,
  Ctx,
  Lang: ?Sized,
  Cst,
> ParseInput<'inp, L, O, Ctx, Lang>
  for Pratt<
    Power,
    Lhs,
    Rhs,
    FoldPrefix,
    FoldInfix,
    FoldPostfix,
    PreOp,
    LeftAssoc,
    RightAssoc,
    NeitherAssoc,
    PostOp,
    L,
    O,
    Ctx,
    Lang,
    Cst,
  >
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lhs: ParsePrattLHS<'inp, Power, O, PreOp, L, Ctx, Lang>,
  Rhs: ParsePrattRHS<'inp, Power, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>,
  FoldPrefix: PrattFoldPrefix<'inp, Power, PreOp, L, O, Ctx, Lang>,
  FoldInfix: PrattFoldInfix<'inp, Power, LeftAssoc, RightAssoc, NeitherAssoc, L, O, Ctx, Lang>,
  FoldPostfix: PrattFoldPostfix<'inp, Power, PostOp, L, O, Ctx, Lang>,
  Power: PrattPower,
  Cst: PrattCst<'inp, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>,
  // The stalled-report exits below surface the contract violation as the end-of-expression
  // error for the channel that broke it: the LHS one for a prefix report, the RHS one for an
  // infix/postfix report and for the foot of a cycle. Both halves of the pair `FromPrattError`
  // already bundles.
  //
  // `NonAssociativeChain` joins them for the same reason and at the same place: the wrapper
  // builds it, after settling, from an offset the driver captured. `RecursionLimitReached` is the
  // one conversion `parse` also carries, because the value that needs it is built by
  // `InputRef::descend` at the frame prologue — before any posture exists to be disturbed — and
  // not by a deferred effect. All four are in the `FromPrattError` bundle.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEoLhs<L::Offset, Lang>>
    + From<UnexpectedEoRhs<L::Offset, Lang>>
    + From<RecursionLimitReached<L::Offset, Lang>>
    + From<NonAssociativeChain<L::Offset, Lang>>,
{
  /// # The one guard that spans the recursion
  ///
  /// The driver below opens a `Commit` guard per operator *probe* and closes it the moment the
  /// operator is accepted, so nothing of the recursion, the folds or the CST seam runs inside a
  /// live cycle guard any more — that is what keeps retention off the operator depth (see the
  /// `Time and space` section on [`Pratt`]). Two of the postures the wide cycle guard used to
  /// cover survive it, and this is where they are honoured:
  ///
  /// - an **unwind** through either channel, a fold hook, the emitter, the CST seam or the
  ///   recursive operand parse that carries them — the state a half-applied cycle leaves is
  ///   exactly "the operator consumed, the right-hand side absent". This guard is undecided while
  ///   all of that runs, so its drop restores the input to **before the expression**: broader
  ///   than the cycle rollback it replaces, and never narrower.
  /// - the **foot-of-cycle refusal**, a fold or a recursion that rewound behind the operator it
  ///   was handed. The driver cannot restore that itself — its own probe was committed a
  ///   statement after the operator was accepted — so it reports the posture instead, as
  ///   `Fault::Rewind`, and the rollback happens here.
  ///
  /// A third posture arrives here for the opposite reason. `Fault::Stall` is the **`Keep`**
  /// posture — the three report-boundary contract violations, which have already taken the only
  /// restore they are owed — carrying its `debug_assert` out of `parse` so that this method can
  /// raise it *after* committing. Left where the violation is detected, that assertion is a
  /// driver-raised panic inside the window the first bullet describes, and the first bullet would
  /// then be true of a debug build on a path where release keeps the whole expression. Moving it
  /// out is what makes "these two exits and no others" a fact about the code rather than about
  /// `cfg(debug_assertions)`; the `Pratt` contract states it in those terms.
  ///
  /// Why the **expression** and not the cycle: the cycle's own scope no longer exists at the
  /// point either posture is decided, and a guard reopened around the recursion would put the
  /// per-depth retention straight back. An expression-wide restore covers every cycle of that
  /// expression, so nothing the narrow guard used to take back survives this one — and a good
  /// deal that it used to keep does not survive either. Both postures spend the **same** emitter
  /// mark, so both take the expression's diagnostics and its CST events with its input; that is
  /// the published contract on [`Pratt`], which also gives the four reasons it cannot be
  /// narrowed. The `Pratt` doc likewise names the one refusal this width does not preserve.
  ///
  /// # Why the policy is [`Rollback`](crate::Rollback), when every ordinary exit commits
  ///
  /// Read the match below and the guard looks mis-declared: `Ok` and `Fault::Keep` both
  /// [`commit`](crate::Transaction::commit) explicitly, so the policy can only decide the exits
  /// that leave *without* reaching an arm. That is the point — those are precisely the two the
  /// guard exists for, and the policy is what makes their restore unconditional:
  ///
  /// * An **unwinding drop** restores under either policy in a `std` build, because
  ///   [`Commit`](crate::Commit)'s "a panic is not a decision" reads
  ///   `std::thread::panicking()` and takes the rollback arm. Under `no_std` there is no such
  ///   fact to read: `core` has no `panicking()`, the constant is `false`, and a
  ///   `Commit`-policy guard **commits** on the unwind edge — the divergence `Commit` documents.
  ///   A `Commit` guard here would therefore have kept the torn expression on exactly the
  ///   configuration the unwind guarantee was written for. `Rollback` does not consult the
  ///   unwind fact at all (`P::ROLLBACK_ON_DROP` is `true`), so the restore is the same in every
  ///   build, which is what this method advertises.
  /// * `Fault::Rewind` settles through
  ///   [`rollback_abandoning_points`](crate::Transaction::rollback_abandoning_points) — the
  ///   **reconciling** rollback, which is what a rolling-back drop of this guard performs and
  ///   what an unwind through it therefore performs too. The choice against the checked
  ///   [`rollback`](crate::Transaction::rollback) is not stylistic. A checked rollback refuses —
  ///   panics, in release, in every allocator build — when the rewind would cross a live pin,
  // `begin_point` exists only in allocator builds, so the link has to be, too — the crate-wide
  // idiom for a doc reference to a feature-gated item.
  #[cfg_attr(
    any(feature = "std", feature = "alloc"),
    doc = "   and a [session point](crate::InputRef::begin_point) any hook opened and abandoned inside"
  )]
  #[cfg_attr(
    not(any(feature = "std", feature = "alloc")),
    doc = "   and a session point any hook opened and abandoned inside"
  )]
  ///   this expression is exactly such a pin. The reconciling one abandons every point younger
  ///   than the base (unpinned, lineage entry dropped, emitter mark released) and the restore
  ///   then subsumes its progress. Three reasons that is the right one of the two answers the
  ///   input layer offers:
  ///   1. this exit already carries a grammar bug out as a typed terminal error; refusing it
  ///      *before* the restore would replace that report with a release panic and leave the
  ///      damaged state committed for any host that catches — strictly worse than the error it
  ///      pre-empts, and the driver is not the party that abandoned the point;
  ///   2. the two expression-scoped exits must restore the same thing, and the unwind reaches
  ///      this guard through the reconciling drop. Settling `Rewind` through the checked path
  ///      would make them disagree on exactly the inputs where a point is live;
  ///   3. reconciling *is* the input layer's specified answer for an abandoned point crossed by
  ///      an enclosing rollback.
  ///
  ///   Naming the verb is itself part of the fix. This settle used to be spelled `drop(txn)` —
  ///   correct, because the policy is `Rollback`, but correct only as long as a reader knew that
  ///   and no later edit "tidied" it into `txn.rollback()`. The driver's five cycle-scoped exits
  ///   are the same argument arriving one scope down, and there it had already gone wrong: they
  ///   spelled their restore with the checked verb, so a hook that abandoned a point turned an
  ///   ordinary handback into a panic. Both scopes now say what they mean.
  ///
  /// `Fault::Keep` — every `?`-propagation out of caller code — commits, exactly as dropping the
  /// old `Commit` cycle guard on a `?` did. A `?` is a return and not an unwind, so no policy has
  /// ever spoken for that path. `Fault::Stall` commits identically; it is the same posture with
  /// two effects attached, and the arm below runs them in that order for the reason the arm
  /// itself gives.
  ///
  /// # Every effect fires after the settle, and only one arm's ordering is optional
  ///
  /// Each arm below does its work in exactly one order: **settle, then assert, then build the
  /// terminal error**. Both of those effects are the driver's own, both used to run one scope down
  /// inside `parse`, and both are *caller* code — the assertion compares two `L::Offset`s through
  /// `PartialOrd`, and the report clones an `L::Offset` and runs the grammar's `From`. So both can
  /// panic, and a panic raised before the settle is a rolling-back drop of this guard.
  ///
  /// The `Rewind` arm's ordering is **belt-and-braces** rather than load bearing: this arm settles
  /// by restoring, so an effect that panicked ahead of the settle would take back exactly what the
  /// settle takes back. It is kept in this order anyway, because the reason it is safe is a
  /// property of the *policy* — and a future policy change must not silently re-invert it.
  ///
  /// The `Stall` arm's ordering is **not** optional, and the asymmetry is the point: that arm
  /// settles by *committing*, so anything raised ahead of it leaves this guard undecided under an
  /// unwinding drop and rolls the whole expression back — the opposite of what the arm decided.
  /// `Rewind` is safe either way because both orderings converge on the same restore; `Stall` is
  /// not, because they diverge on every observable there is. The assertion makes that divergence a
  /// debug-only fact; the report construction makes it a fact in **both** profiles, since a
  /// panicking `L::Offset::clone` is caller code this repository already treats as reachable.
  ///
  /// Three sites construct `Stall` and one constructs `Rewind`. Both effects live here, once,
  /// instead of at any of them — and the report half of that is enforced rather than reviewed:
  /// `Fault` carries no error, and `parse` does not carry the `From<UnexpectedEo…>` bounds that
  /// building one needs, so `stalled_prefix_report`/`stalled_rhs_report` are not callable from the
  /// driver at all. See the rule stated on the private `Fault` enum. (Code spans and not links:
  /// these docs are public, and `rustdoc::private_intra_doc_links` is an error under the crate's
  /// `#![deny(warnings)]`.)
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let mut txn = input.begin_with::<Rollback>();
    match parse(
      &mut *txn,
      &mut self.parse_lhs,
      &mut self.parse_rhs,
      &mut self.fold_prefix,
      &mut self.fold_infix,
      &mut self.fold_postfix,
      PrattFloor::Inclusive(self.min_precedence.clone()),
      &self.cst,
    ) {
      Ok(value) => {
        txn.commit();
        Ok(value)
      }
      Err(Fault::Keep(error)) => {
        txn.commit();
        Err(error)
      }
      Err(Fault::Stall {
        at,
        committed_before,
        channel,
      }) => {
        // COMMIT FIRST, EVERY EFFECT SECOND, and here that ordering is load bearing rather than
        // belt-and-braces. Both effects below are the driver's own, raised on a grammar bug, and
        // both are caller code that may panic: `PartialOrd` on `L::Offset` for the assertion,
        // `L::Offset::clone` and the grammar's `From` for the report. Run either while this guard
        // is still undecided — which is where both used to live, one scope down inside `parse` —
        // and the unwind goes through a `Rollback` guard that then erases the whole expression,
        // input and emissions alike, on the one path that decided to keep both. That is a third
        // expression-scoped restore where the `Pratt` contract says there are two; the assertion
        // made it a debug-only third, the report construction made it a both-profiles third.
        // `commit` takes `self`, so past this line there is no guard left for any panic to settle:
        // the two profiles keep exactly what the cycle-scoped restore left, and they differ only
        // in whether the report arrives as a panic or as an error.
        txn.commit();
        // One match, both effects, in order: the channel picks the assertion's wording and the
        // report's constructor, and `at` is *moved* into the constructor rather than cloned —
        // there is no second copy of it to make and nothing left for a clone to panic in.
        Err(match channel {
          StalledChannel::Lhs => {
            debug_assert!(
              at > committed_before,
              "pratt: a Prefix report consumed nothing — the LHS channel must consume what it \
               reports (see ParsePrattLHS)"
            );
            stalled_prefix_report::<L, Ctx, Lang>(at)
          }
          StalledChannel::Rhs => {
            debug_assert!(
              at > committed_before,
              "pratt: an Infix/Postfix report consumed nothing — the RHS channel must consume \
               what it reports (see ParsePrattRHS)"
            );
            stalled_rhs_report::<L, Ctx, Lang>(at)
          }
        })
      }
      Err(Fault::NonAssoc { at }) => {
        // COMMIT, exactly as `Stall` does and for the same reason: this is a **cycle-scoped**
        // posture. The driver already handed the deciding read back through its own probe guard,
        // which was still live for it, so the narrow restore the surrounding grammar is owed has
        // happened and the expression's earlier folds, position and diagnostics are all owed to
        // it as well. Rolling the expression back here would erase the left-hand side and every
        // folded cycle over an error that describes *one operator* — and would take the pratt
        // parser's own diagnostics with them.
        //
        // Settle first, build second, for `Stall`'s reason: the constructor below runs the
        // grammar's `From` and clones nothing else, but a `From` is caller code, and a panic in
        // it raised while this guard were still undecided would roll the whole expression back on
        // the one path that decided to keep it. `commit` takes `self`, so past this line there is
        // no guard left for any panic to settle.
        txn.commit();
        Err(NonAssociativeChain::of(at).into())
      }
      Err(Fault::Rewind {
        at,
        committed_before,
      }) => {
        // The RECONCILING rollback, not the checked `rollback()`, and the distinction is load
        // bearing — see the `Rollback` section above. `rollback` refuses to cross a live pin; a
        // session point a hook opened and abandoned anywhere inside this expression is one, and
        // the refusal is a release panic raised *before* anything is restored. This verb
        // reconciles those points and then restores, which is both the input layer's own answer
        // for an abandoned point and, byte for byte, what an unwind through this same guard
        // already does — it is this guard's rolling-back drop, said out loud.
        txn.rollback_abandoning_points();
        // Settle, assert, build — the same order as the `Stall` arm, kept for uniformity rather
        // than out of necessity. This arm's settle and its unwind edge converge, so a panic in
        // either effect would restore what the settle restored; the order is what stops that
        // convergence from becoming load bearing somewhere it does not hold.
        debug_assert!(
          at > committed_before,
          "pratt: a fold or a recursive operand parse rewound the input behind the operator it \
           was handed"
        );
        Err(stalled_rhs_report::<L, Ctx, Lang>(at))
      }
    }
  }
}

/// The private driver's error, carrying the **rollback posture** its wrapper must take.
///
/// The driver's per-cycle `Commit` guard used to stay open across the recursive operand parse,
/// the folds and the CST wrap, which made "roll this cycle back" something the loop could always
/// do for itself. It no longer is: the probe is committed the instant the operator is accepted,
/// so by the time the *foot* of the cycle discovers that a fold or a recursion rewound behind
/// that operator, there is no live guard of this frame left to restore through. The posture
/// still has to be honoured — it is the whole reason the old check ran a statement early, inside
/// the scope — so the driver stops carrying it in control flow and carries it in the error
/// instead, and [`ParseInput::parse_input`] acts on it.
///
/// A second thing rides out for a second reason. The wrapper's guard is undecided for the whole
/// of `parse`, so anything raised inside it that can panic is an unwind through that guard: the
/// expression is restored and its emissions erased on paths where the posture that was decided
/// keeps both. That is not a posture the driver can honour locally either, and the answer is the
/// same — the offsets travel in the posture and the wrapper performs the effects once it has
/// settled.
///
/// Four variants, and the asymmetry is the point:
///
/// * [`Keep`](Self::Keep) is the ordinary posture and the one every `?` out of caller code
///   takes. It reproduces exactly what dropping an undecided `Commit` guard did: the input keeps
///   whatever it consumed, a fail-fast emitter error carries the consumed progress out, and the
///   surrounding grammar sees the same position it saw before.
/// * [`Stall`](Self::Stall) is `Keep` **plus two deferred effects** — the assertion and the
///   terminal report — and nothing else: it settles by committing, exactly as `Keep` does. Three
///   construction sites, all of them the "consume what you report" violation — the two
///   report-boundary stalls, which have already rolled their own cycle back through a probe guard
///   that was still live at that point, and the prefix stall, which by its own precondition has
///   nothing to restore. None of them may assert **or report** in place: the cycle-scoped restore
///   they take (or do not need) is the *narrow* one the surrounding grammar is owed, and any panic
///   raised before returning would replace it with the expression-scoped one.
/// * [`NonAssoc`](Self::NonAssoc) is `Stall` with one deferred effect instead of two: no
///   assertion (this is malformed *input*, not a grammar bug) and a **non-terminal** report,
///   `NonAssociativeChain`. One construction site, the repeat guard in the `Infix` arm, which —
///   like the two report-boundary stalls — has already handed the deciding read back through a
///   still-live probe guard. It settles by committing for exactly their reason: the narrow
///   restore is the one that was owed, and the expression's earlier folds and diagnostics are
///   not this operator's to erase.
/// * [`Rewind`](Self::Rewind) has exactly **one** construction site, the foot-of-cycle refusal.
///   It carries the two offsets so the wrapper can assert and report *after* the restore rather
///   than before — the ordering discipline the check has always had, moved out with it.
///
/// # A decided posture carries data, never effects
///
/// The assertion was the first effect to ride out, and moving *it* alone fixed one line rather
/// than the class. The terminal error was still built where the violation was detected, and
/// building one clones an `L::Offset` and runs a grammar-supplied `From` — caller code, in **both**
/// profiles, inside a window the assertion had only ever made hazardous in debug. A panic there is
/// a panic between the branch that *decides* a posture and the settle that honours it, and every
/// such panic takes the rolling-back exit: the right answer for `Rewind`, and the exact opposite of
/// what `Stall` decided.
///
/// So the rule is stated once, on the payload rather than at the sites: **past the branch that
/// decides a posture, `parse` performs no work that can fail and no work that touches parse
/// state.** Every field of `Stall`, `NonAssoc` and `Rewind` is a value that already exists at that
/// branch, so constructing any of them is one to three moves and nothing else. None carries an
/// `E`, because there is no way to build an `E` that is not caller code. [`Keep`](Self::Keep) is
/// the exception that proves the rule: its `E` was built by caller code *before* control came back
/// to the driver, so `map_err(Fault::Keep)` moves an error rather than making one.
///
/// `NonAssoc`'s offset obeys the rule the same way, and it took one statement to arrange: the
/// offending operator's start is read at the top of the *cycle*, ahead of the RHS classifier and
/// therefore ahead of the floor test, the report boundary and the repeat test alike, so the branch
/// that decides this posture is handed a value that already exists. Reading it inside the branch
/// would have put an `L::Offset::clone` — caller code — between the decision and the settle, which
/// is the class the rule exists to close.
///
/// Reading it *there* rather than after the classifier is also what makes the value right, and
/// that is a separate argument from this one. The probe restores to the point the read is taken
/// at, so the token peeked there is the token the caller is handed back; the committed span after
/// the classifier holds its **last** token instead, which for a multi-token operator names the
/// operator's tail. See the capture site, and `NonAssociativeChain`'s own documentation.
///
/// The rule has the compiler behind it and not only this paragraph. `parse` does **not** carry the
/// `From<UnexpectedEoLhs<…>> + From<UnexpectedEoRhs<…>> + From<NonAssociativeChain<…>>` bounds —
/// only [`ParseInput::parse_input`] does — so `stalled_prefix_report`, `stalled_rhs_report` and
/// the repeat's own report are not callable from the driver at all. Re-introducing a pre-settle
/// report construction is not a subtle edit that reviews clean; it is a build error at the call
/// site, naming the bound the driver deliberately does not have.
///
/// The one conversion `parse` *does* carry is `From<RecursionLimitReached<…>>`, and it is not an
/// exception to the rule: the value is built by
/// [`InputRef::descend`](crate::InputRef::descend) at the **frame prologue**, before this frame
/// has a CST mark, a watermark, a probe or a posture — so there is no decided posture for a panic
/// in that conversion to contradict, and the exposure is the one every call into caller code
/// inside `parse` already has (an unwind through the undecided expression guard, which restores
/// the expression: the documented exit).
///
/// What the rule does **not** reach is stated with it rather than left to be discovered. The two
/// report-boundary stalls and the non-associative repeat settle their *probe* —
/// `rollback_abandoning_points` — after the branch and before the return, because the guard being
/// settled is a local that cannot outlive the frame. That restore is the first half of each
/// posture rather than work done alongside it, and a panic inside it is the input layer's own
/// restore failing, which is the same exposure every rollback in the crate has, including the
/// non-fault probe exits a few lines up.
///
/// # The settle obligation
///
/// **Every caller of `parse` must settle a `Rewind` against an enclosing guard, and must then
/// perform the deferred effects — the assertion and the terminal report — of whichever posture it
/// received.** There are exactly three call sites: [`ParseInput::parse_input`], which owns the
/// expression-scoped guard and is where all of that is discharged, and `parse`'s own two
/// recursions — the prefix operand parse and the infix operand parse — which propagate with a bare
/// `?` *because* they have no guard of their own to settle against and the frame above them does.
/// That is the invariant, and it is written here rather than left to be inferred: a future second
/// entry point into `parse` that forwards `Fault::Rewind` to its caller without a guard behind it
/// inherits a restore that never happens, and the symptom is not a compile error — it is a grammar
/// bug reported with the damage still committed, which is the exact failure the check was written
/// to prevent. A `Fault::Stall` forwarded past the last guard is the quieter twin: the assertion is
/// simply never raised, and a "consume what you report" violation stops being loud in debug. The
/// report itself cannot go missing the same way — a posture that reaches no wrapper reaches no
/// caller either, since the only way out of a `Fault` is the `match` that spends it.
///
/// # What the width costs
///
/// The restore `Rewind` asks for is the whole expression, so it also **erases every emission that
/// expression made** — the left-hand side's diagnostics, every already-folded cycle's, and their
/// CST events, including real complaints about the user's input that have nothing to do with the
/// fold that misbehaved. The cycle-scoped rollback this replaces kept them. An unwind through the
/// expression guard costs exactly the same, because it spends the same emitter mark; the only
/// difference is that its wraps may be half-built rather than complete.
///
/// This is not a private note. It is stated as a **contract on [`Pratt`]** — what survives, what
/// does not, and on which exits — because a caller cannot be expected to infer an empty diagnostic
/// list from a driver's retention strategy. The four mechanical reasons it cannot be narrowed
/// (one checkpoint carries the position *and* the emitter mark; one emitter mark carries the
/// diagnostics *and* the CST token events; the probe's mark is released, not held; and preserving
/// across the restore desynchronises the dedup watermarks into duplicates) live there too, in the
/// docs a downstream grammar author can actually read. The short version for this file: emission
/// scope is a **function of** rollback scope, so the only way to discard less is to retract less,
/// and retracting less means paying `O(d)` checkpoints again.
///
/// Pinned by `an_expression_rollback_takes_the_expressions_diagnostics_with_it` in
/// `tests/pratt_txn_retention.rs`, so this stays a decision under test rather than an accident of
/// which guard happens to be live.
///
/// It never escapes this module: [`ParseInput::parse_input`] destructures it into the emitter's
/// own error and the posture is spent there.
enum Fault<E, Off> {
  /// Keep whatever the input consumed and report `E` — the commit-by-default posture.
  ///
  /// The one variant that carries an error, and the reason it may is that it does not *build* one:
  /// caller code built this `E` and handed it back, and `map_err(Fault::Keep)` moves it. See the
  /// rule on the enum.
  Keep(E),
  /// Commit, then assert `at > committed_before` in `channel`'s wording, then build and report
  /// `channel`'s terminal error — the [`Keep`](Self::Keep) posture with both effects moved out
  /// past the settle.
  Stall {
    /// Committed consumption measured the instant the report crossed back, and the offset the
    /// terminal error is built from once the guard has committed.
    at: Off,
    /// Committed consumption the report started from — the value `at` failed to exceed.
    committed_before: Off,
    /// Which channel broke "consume what you report": the wording the assertion takes, and which
    /// of the two report constructors the wrapper calls.
    channel: StalledChannel,
  },
  /// Commit, then build and report [`NonAssociativeChain`] — the [`Keep`](Self::Keep) posture
  /// with one deferred effect and no assertion.
  ///
  /// The `Stall` shape minus the second offset and the channel marker: there is nothing to assert
  /// here, because a repeated non-associative operator is malformed *input* rather than a grammar
  /// bug, and the report it carries is correspondingly **non-terminal**. Like the two
  /// report-boundary stalls, its own cycle-scoped restore has already happened through a probe
  /// guard that was still live for it, so the wrapper's settle is a commit.
  NonAssoc {
    /// The **start of the repeated operator**, parked back on the input by the probe's rollback —
    /// captured before the RHS classifier ran, which is the only point in the cycle at which it
    /// equals the position that rollback restores to.
    at: Off,
  },
  /// Restore the input to before the expression, then assert `at > committed_before`, then build
  /// and report the RHS terminal error.
  Rewind {
    /// Committed consumption measured at the foot of the cycle, and the offset the terminal error
    /// is built from once the restore has happened.
    at: Off,
    /// Committed consumption the cycle started from — the value `at` failed to exceed. Named for
    /// the watermark and not for the precedence `floor` a few lines up in `parse`, which is a
    /// different quantity entirely.
    committed_before: Off,
  },
}

/// Which channel broke the "consume what you report" contract — the only thing that differs
/// between the three [`Fault::Stall`] sites once the assertion and the report have both been moved
/// to the wrapper, and so the thing that picks both the assertion's wording and the report's
/// constructor there.
///
/// A two-variant marker rather than the `&'static str` message itself, and the reason is size:
/// `Fault` is the error half of the `Result` the driver returns through **every** frame of its
/// own recursion, and a `&str` field is two words where a fieldless marker is free. Free, not
/// one byte, and measured rather than hoped for — see the tripwire below. The two wordings stay
/// as literals at the assertion instead, which also keeps a `should_panic(expected = …)` matching
/// text that actually appears in the source.
#[derive(Clone, Copy)]
enum StalledChannel {
  /// A [`PrattLHS::Prefix`] report that consumed nothing.
  Lhs,
  /// A [`PrattRHS::Infix`]/[`Postfix`](PrattRHS::Postfix) report the floor admitted that consumed
  /// nothing.
  Rhs,
}

// ── What the posture costs the recursion ─────────────────────────────────────
//
// `Fault` is the error half of the `Result` the driver returns through **every** frame of its own
// recursion, so a field added to it is a field in every frame's return slot. [`Fault::Stall`]
// carries three, for a diagnostic on a cold path, and the fair question is what the hot path pays
// for them.
//
// Measured, not assumed: **nothing**. `size_of::<Fault<E, Off>>()` is byte-for-byte the
// `Keep`/`Rewind` shape in every configuration pinned below, down to the all-`u8` one that has no
// padding anywhere for a spare byte to hide in. [`StalledChannel`] is why — a two-variant
// fieldless enum leaves 254 of its 256 bit patterns unused, and rustc spends that niche on
// `Fault`'s own discriminant, so the channel marker pays for the tag it would otherwise have had
// to sit beside.
//
// Deferring the terminal report to the wrapper as well took the `error: E` field off `Stall` and
// `Rewind`, so both mirrors below shed it together and the relation is unchanged. Where `E` is a
// ZST — the benchmark's own instantiation — nothing moved at all; where it is not, the recursive
// slot got *smaller*, which no pin here is in a position to notice and none needs to.
//
// For the pratt benchmark's instantiation — `benches/parser_combinators.rs` reports a ZST error
// through a `LogosLexer`, whose `Offset` is `usize` — that is three words of `Fault`, and a
// `Result` around the benchmark's 32-byte fold output that is 32 bytes: the same return slot the
// driver had before it carried a posture at all.
//
// The pins are relational because `Off` is `usize` for every lexer in the tree, which would make
// a byte count a 64-bit-only fact; the one absolute pin below is stated in words for the same
// reason.

/// The footprint budget [`Fault`] is held to: the same enum minus [`Stall`](Fault::Stall) — the
/// two postures the driver carried before the stalled report was named.
///
/// A mirror rather than a byte count so the pin holds on every target, and a mirror rather than a
/// comment so a new posture cannot widen the recursive return slot in silence: add one that
/// does not fit and the crate stops compiling, here, by name. A *deliberate* change to
/// [`Fault::Rewind`] fails it too, which is the point — the budget is then re-stated in the same
/// commit that spends it, rather than drifting. It has been, once, and exactly that way: when the
/// terminal report moved out to the wrapper, `Rewind`'s `error: E` went with it and this mirror
/// dropped the same field in the same commit.
///
/// [`Fault::NonAssoc`] was added without touching this mirror, and that is the mechanism working
/// rather than an omission: one `Off` field is strictly inside [`Stall`](Fault::Stall)'s three,
/// and the discriminant still rides [`StalledChannel`]'s niche, which has 254 spare patterns and
/// needs three. The rows below re-check it in four instantiations including the all-align-1 one.
#[allow(dead_code, reason = "constructed nowhere: it exists to be measured")]
enum FaultBudget<E, Off> {
  Keep(E),
  Rewind { at: Off, committed_before: Off },
}

/// The benchmark's fold output, mirrored for layout: `benches/parser_combinators.rs` folds to
/// `Spanned<BenchTok, SimpleSpan>`, and `BenchTok` is an eleven-variant enum whose largest payload
/// is an `i64`.
///
/// An enum and not a same-sized struct, because both halves of its layout are load bearing. The
/// size is one; the **niche** its discriminant leaves is the other, and that niche is where
/// `Result`'s own tag ends up. Measured: with it the benchmark's recursive return slot is 32
/// bytes, without it 40 — so a `(u64, u64)` stand-in of identical size and alignment would have
/// pinned a slot one word wider than the one the benchmark actually returns.
#[allow(dead_code, reason = "constructed nowhere: it exists to be measured")]
enum BenchOperand {
  Int(i64),
  Other,
}

/// What the benchmark's driver recursion returns, spelled once: `Result<BenchSlot, Fault<…>>`.
#[allow(
  dead_code,
  reason = "used only inside `const` size assertions; 1.87's lint misses it"
)]
type BenchSlot = crate::span::Spanned<BenchOperand, crate::span::SimpleSpan>;

/// One row of the tripwire: [`Fault`], and the `Result` the recursion actually returns, both held
/// to [`FaultBudget`].
macro_rules! fault_within_budget {
  ($($why:literal => <$e:ty, $off:ty, $o:ty>,)+) => {$(
    const _: () = assert!(
      size_of::<Fault<$e, $off>>() == size_of::<FaultBudget<$e, $off>>(),
      concat!(
        "pratt: `Fault` outgrew the `Keep`/`Rewind` budget for ", $why, ". It is the error half \
         of the driver's recursive `Result`, so this is paid once per frame. Shrink the new \
         payload — a fieldless marker rides free in the discriminant's niche, a `&'static str` \
         does not — or re-state `FaultBudget` in this commit."
      )
    );
    const _: () = assert!(
      size_of::<Result<$o, Fault<$e, $off>>>() == size_of::<Result<$o, FaultBudget<$e, $off>>>(),
      concat!(
        "pratt: the driver's recursive return slot outgrew the `Keep`/`Rewind` budget for ",
        $why, ". See the `Fault` assertion above it."
      )
    );
  )+};
}

fault_within_budget! {
  // The benchmark, exactly: `struct BenchError;` is a ZST, and `LogosLexer`'s `Offset` is `usize`.
  "the pratt benchmark's instantiation" => <(), usize, BenchSlot>,
  // A grammar whose error is one word — an index into a diagnostic arena, say.
  "a word-sized error" => <usize, usize, BenchSlot>,
  // An error large enough to dominate the payload, so the offsets fall inside its own footprint.
  "an error that dominates the payload" => <[u64; 4], usize, ()>,
  // The adversarial shape: everything align-1, so there is no padding anywhere for a stray byte
  // to hide in. This is the row that would have caught a marker encoded as a plain field.
  "a lexer with byte-wide offsets" => <u8, u8, ()>,
}

// The benchmark's own numbers, absolute rather than relational. Both are stated as relations
// between types rather than as byte counts, so they hold on a 32-bit target too.

// A ZST error and two offsets, packed with the discriminant into three words.
const _: () = assert!(
  size_of::<Fault<(), usize>>() == 3 * size_of::<usize>(),
  "pratt: the benchmark instantiation's `Fault` is no longer three words. Nothing is broken by \
   this on its own — but the per-frame footprint the transaction narrowing was measured against \
   has moved, so re-measure before quoting the old numbers."
);

// And the number that actually matters, because it is what every recursive frame returns: the
// posture is free. `Result<BenchSlot, Fault<…>>` is the size of the operand ALONE — the fault, its
// two offsets and the channel marker all fit in the operand's own niche, so the driver's return
// slot is exactly what it was before it carried a posture at all.
const _: () = assert!(
  size_of::<Result<BenchSlot, Fault<(), usize>>>() == size_of::<BenchSlot>(),
  "pratt: the driver's recursive return slot is no longer free for the benchmark instantiation — \
   `Result<O, Fault<…>>` now costs more than `O`. The transaction narrowing's per-frame win was \
   measured against a slot that cost nothing extra; re-measure before quoting it."
);

// The one private driver threads the whole fold configuration plus the CST seam through
// its own recursion; a parameter bundle would be assembled and torn apart at every
// recursive call for no reader benefit.
//
// The error is [`Fault`], not the emitter's error: this frame can no longer restore every
// posture for itself, so the two it cannot are named in the error and honoured by the one
// caller, `ParseInput::parse_input`. A `Fault::Rewind` raised by a *recursive* call therefore
// propagates through every frame above it untouched and is settled once, against the whole
// expression — which is the widening the narrowing is paid for with.
#[allow(clippy::too_many_arguments)]
fn parse<
  'inp,
  Power,
  Lhs,
  Rhs,
  FoldPrefix,
  FoldInfix,
  FoldPostfix,
  PreOp,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  PostOp,
  L,
  O,
  Ctx,
  Lang: ?Sized,
  Cst,
>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  parse_lhs: &mut Lhs,
  parse_rhs: &mut Rhs,
  fold_prefix: &mut FoldPrefix,
  fold_infix: &mut FoldInfix,
  fold_postfix: &mut FoldPostfix,
  min_precedence: PrattFloor<Power>,
  cst: &Cst,
) -> Result<O, Fault<<Ctx::Emitter as Emitter<'inp, L, Lang>>::Error, L::Offset>>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Lhs: ParsePrattLHS<'inp, Power, O, PreOp, L, Ctx, Lang>,
  Rhs: ParsePrattRHS<'inp, Power, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>,
  FoldPrefix: PrattFoldPrefix<'inp, Power, PreOp, L, O, Ctx, Lang>,
  FoldInfix: PrattFoldInfix<'inp, Power, LeftAssoc, RightAssoc, NeitherAssoc, L, O, Ctx, Lang>,
  FoldPostfix: PrattFoldPostfix<'inp, Power, PostOp, L, O, Ctx, Lang>,
  Power: PrattPower,
  Cst: PrattCst<'inp, PreOp, LeftAssoc, RightAssoc, NeitherAssoc, PostOp, L, Ctx, Lang>,
  // THE ONE BOUND THAT IS HERE, and why it is not the exception the three below are. The frame
  // prologue enters a level of the input's recursion budget, and `InputRef::descend` builds its
  // own error, so this function needs the conversion. It runs before this frame has a CST mark, a
  // watermark, a probe or a decided posture — there is nothing for a panicking conversion to
  // contradict, only the ordinary undecided-guard exposure every call into caller code inside
  // this function already has.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<RecursionLimitReached<L::Offset, Lang>>,
  // THE THREE BOUNDS THAT ARE NOT HERE. The stalled-report exits below surface the contract
  // violation as the end-of-expression error for the channel that broke it — the LHS one for a
  // prefix report, the RHS one for an infix/postfix report and for the foot of a cycle — and the
  // repeat guard surfaces `NonAssociativeChain`, but none of them *builds* its error.
  // `From<UnexpectedEoLhs<…>> + From<UnexpectedEoRhs<…>> + From<NonAssociativeChain<…>>` is
  // carried by the wrapper's impl block alone, so `stalled_prefix_report`/`stalled_rhs_report`
  // and `NonAssociativeChain::of(..).into()` cannot be called from this function at all. That is
  // deliberate and it is the mechanical half of `Fault`'s "a decided posture carries data, never
  // effects": building a report runs a grammar's `From`, caller code, and every line of this
  // function runs inside an undecided expression-scoped `Rollback` guard. Restoring these bounds
  // re-opens that window, so it fails to compile here rather than reviewing clean at the five
  // sites.
{
  // ONE FRAME, ONE LEVEL — ahead of the CST mark, the LHS watermark and every read, because at
  // this line the frame owns nothing that a trip would have to restore. Every recursion site
  // below (the prefix operand, the infix right operand) and the `parse_input` root each enter
  // through their own prologue, so depth equals the native pratt depth it protects with no
  // per-site bookkeeping — and a future third site inherits the bound for free. The guard
  // releases the level on every exit of this function, unwind included, in `std` and `no_std`
  // alike.
  //
  // `Fault::Keep` for the trip: nothing of *this* frame is consumed, and the enclosing
  // expression's progress and emissions are owed to it exactly as they are on any other
  // `?`-propagation out of caller code.
  let mut frame = inp.descend().map_err(Fault::Keep)?;
  let inp = &mut *frame;

  // The driver-held mark: minted before anything of this expression is parsed, spent once
  // per fold below. Each recursive operand parse takes its own mark, and same-target wraps
  // materialize inside-out, so nesting follows fold order for free. `None` (and every
  // `wrap_at`/`classify` below a no-op) when no CST kinds are configured.
  let cst_mark = Cst::mark(inp);

  // Step 1: parse lhs -- either a prefix operator or an operand
  //
  // The LHS watermark, read before the channel is asked, on the same metric the RHS boundary
  // uses: committed consumption (`span().end()`), never the cache-front cursor.
  let before_lhs = inp.span().end();
  let mut lhs = match parse_lhs.parse_pratt_lhs(inp).map_err(Fault::Keep)? {
    PrattLHS::Operand(o) => o,
    PrattLHS::Prefix(precedenced) => {
      // The report boundary — and on this path it is the recursion guard, because the operand
      // parse below re-enters this function at the *same input position*. An LHS parser that
      // reports `Prefix` from a peek is handed its own input back, reports again, and descends
      // until the stack runs out; short of that, the fold and the wrap behind it record an
      // operator that was never consumed. Read before all three, for the reason the RHS
      // boundary is read before its recursion and fold: those can move input the report did
      // not, and a watermark compared after them reads their progress as the report's.
      //
      // Held against *every* `Prefix`, with no admitted/declined distinction to sit behind:
      // `PrattLHS` has two variants and no floor applies to either, so the driver acts on this
      // report the moment it is made. The RHS guard's exemption — a report the floor legitimately
      // declines consumes nothing — has no counterpart here.
      let after_report = inp.span().end();
      // `Stall`, which is `Keep` with the assertion and the terminal report both carried out to
      // the wrapper: this exit precedes the recursion, the fold and the wrap, and its own
      // precondition is that committed consumption did not advance — there is nothing of this
      // report to restore, and erasing what the LHS parser emitted on its way here is not the
      // driver's business. See `stalled_prefix_report`.
      //
      // DATA RIDES OUT, EFFECTS DO NOT, and that is the whole of it: this frame runs inside the
      // expression-scoped `Rollback` guard `ParseInput::parse_input` holds, so anything raised at
      // this line that can panic unwinds through an undecided guard and takes the whole expression
      // — the one thing this exit exists to keep. A `debug_assert` is one such thing; so is
      // `stalled_prefix_report`, which clones an `L::Offset` and runs the grammar's `From`. Both
      // are the wrapper's, after its commit. The two offsets and the channel travel in the posture
      // and are already-owned values here, so building it is three moves. Same discipline the
      // foot-of-cycle refusal has always had, applied to the exit that had it backwards.
      if after_report <= before_lhs {
        return Err(Fault::Stall {
          at: after_report,
          committed_before: before_lhs,
          channel: StalledChannel::Lhs,
        });
      }
      let (operator, power) = precedenced.into_components();
      let operand = parse(
        inp,
        parse_lhs,
        parse_rhs,
        fold_prefix,
        fold_infix,
        fold_postfix,
        PrattFloor::Inclusive(power.clone()),
        cst,
      )?;
      // Classify before the fold consumes the operator; wrap only after the fold
      // succeeded — a `?`-exit leaves no node, exactly the `node()` posture.
      let kind = cst.classify(PrattFoldOp::Prefix(&operator));
      let folded = fold_prefix
        .fold_prefix(inp, operand, Precedenced::new(operator, power))
        .map_err(Fault::Keep)?;
      Cst::wrap_at(inp, cst_mark, kind);
      folded
    }
  };

  // Step 2: parse rhs -- either an infix/postfix operator or the end of this pratt expression.
  //
  // The loop is unconditional: where this expression ends is the RHS channel's answer alone,
  // never a position test. A pre-gate on the scanner's frontier answers a different question
  // (has the *newest retained token* reached the buffer end?), so any legal peek through the
  // end of input truncated the expression while unconsumed operators still sat in front of
  // this consumer. Termination is carried by the progress guard at the report boundary below.
  let mut prev_op_is_neither: Option<Power> = None;
  // The progress watermark, taken after the LHS so the first cycle measures only the RHS. The
  // metric is committed consumption (`span().end()`), never the cache-front cursor
  // ([`InputRef::cursor`]) — a lookahead fill moves that across skipped trivia without
  // consuming, reading a zero-width report as false progress.
  let mut committed = inp.span().end();
  loop {
    // The operator PROBE, and nothing more. This guard used to span the whole cycle — the
    // recursion, the folds and the CST wrap all ran inside it — which is what made retention
    // O(operator depth): a right-associative chain held one full checkpoint (cursor, span,
    // `L::State` clone, three offsets, an emitter mark) per live frame. Its scope is now exactly
    // the question it exists to answer: *is the next operator ours?* Everything past that answer
    // runs on `inp`.
    //
    // Still `Commit`-policy, and still commit-by-default for the same reason: an emitter error
    // out of the RHS parser carries the consumed progress out (E1), and only the exits where the
    // operator is not part of this expression restore.
    //
    // THE ROLLBACK VERB, decided once here for all five restoring exits below. Every one of them
    // spells its restore `rollback_abandoning_points` rather than `rollback`, because of what
    // runs inside this scope: `parse_pratt_rhs` is grammar-author code holding a full
    // `InputRef`, so it may legally open a session point and abandon it while deciding — the
    // input layer's contract for `begin_point` says an abandoned point keeps its progress and is
    // released with the handle, not that it is a bug. An abandoned point pins its base, and that
    // base is younger than this probe's, so the CHECKED rollback would refuse to cross it: a
    // release panic, in every allocator build, raised before anything is restored. That would
    // turn `End`, a floor decline, a non-associative repeat or a stalled report — every exit that
    // hands the deciding read back — into a panic, with the deciding read still consumed for any
    // host that catches, and with the expression guard above then taking back the whole
    // expression rather than just this cycle's read. The reconciling verb abandons those points
    // and restores, which is the input layer's specified answer for a point an enclosing rollback
    // reaches below and exactly what the expression guard's own rolling-back drop does for
    // `Fault::Rewind`. The two scopes therefore restore the same thing on the same inputs.
    //
    // This is not a licence for the probe to be careless: the driver opens no point of its own,
    // and every point crossed here was opened by a hook and left open by it.
    let mut txn = inp.begin_with::<Commit>();
    // THE POSITION A NON-ASSOCIATIVE TRIP WOULD NAME, read HERE — inside the probe, ahead of the
    // classifier, and only on a cycle whose latch is armed.
    //
    // `NonAssociativeChain`'s contract is that its offset is the offending operator's own start,
    // which is also the position the surrounding grammar resumes from once the deciding read is
    // handed back. This is the only place in the cycle where those two are the *same fact*: the
    // handback restores to exactly this point, so the token peeked here is byte-for-byte the token
    // the caller reads next. Nothing derived after the classifier has that property — the input's
    // committed span then holds the classifier's **last** token, which for a multi-token operator
    // (`not in`, `is not`, a two-token `<>`) names the operator's tail while the handback returns
    // its head. Measured on a two-token operator, that read reported offset 10 for an operator the
    // caller was handed back at offset 8; this one reports 8, with an empty lookahead cache and a
    // prefilled one alike, because a peek *is* the cache fill and the token's own span does not
    // move with how much was peeked before it.
    //
    // It also obeys `Fault`'s rule — a posture carries values that already exist at the branch
    // deciding it — more cheaply than the read it replaces: that one paid an `L::Offset::clone` on
    // *every* infix cycle, this one pays a peek only on the cycle after a `Neither` fold, and the
    // peek's lex is work the classifier's own first read would have done a statement later.
    //
    // The error is propagated as `Fault::Keep`, the driver's ordinary posture for anything caller
    // code raises. The one behaviour this adds is narrow and deliberate: on a latched cycle whose
    // next token cannot be lexed at all, the failure now surfaces here rather than from whichever
    // read the classifier chose to make — and a classifier that would have declined *without*
    // reading that token is the only case where the two differ.
    //
    // `None` is genuine exhaustion or a terminal stop folded into one (`peek_one`'s documented
    // shape). Neither can reach the repeat guard below: that guard now sits behind the report
    // boundary, which refuses any report that committed nothing, and a cycle with no token left
    // cannot commit one. The committed frontier stands in so the value is total rather than
    // fallible, and no path reads it.
    let latched_at = if prev_op_is_neither.is_some() {
      Some(match txn.peek_one().map_err(Fault::Keep)? {
        Some(tok) => tok.span().start(),
        None => committed.clone(),
      })
    } else {
      None
    };
    let report = parse_rhs.parse_pratt_rhs(&mut *txn).map_err(Fault::Keep)?;
    // The report boundary. Read the instant the report crosses back, before this cycle
    // recurses and before any fold runs, because both of those can move the input on a
    // report that moved none — and then a watermark compared at the foot of the cycle reads
    // their progress as the report's and never fires. Only an *accepted* report is held to
    // this: a report the floor declines is not this expression's operator, is rolled back,
    // and legitimately consumes nothing. See `ParsePrattRHS`'s "consume what you report".
    let after_report = txn.span().end();
    match report {
      // The expression ends here: restore whatever the decision consumed and stop. No
      // classify, no fold, no wrap — nothing of this cycle reaches the CST seam. Reconciling
      // rollback, for the reason given where the probe is opened.
      PrattRHS::End => {
        txn.rollback_abandoning_points();
        break;
      }
      PrattRHS::Postfix(precedenced) => {
        let (operator, op_power) = precedenced.into_components();
        if !min_precedence.admits(&op_power) {
          txn.rollback_abandoning_points();
          break;
        }
        // `<=`, not `==`: the watermark cannot regress across a report, so anything not
        // strictly ahead is a stall. The cycle-scoped restore happens here, on the spot, through
        // a probe guard that is still live for it — the narrower and more truthful of the two
        // available restores, and the one the surrounding grammar is owed.
        //
        // NO ASSERTION AND NO REPORT IN THIS SCOPE. Both used to sit between the rollback and the
        // return, which ordered them correctly against the *probe* and not at all against the
        // guard above it: this frame runs inside the expression-scoped `Rollback` guard, so a
        // `debug_assert` — or the `L::Offset::clone` and grammar `From` a report constructor runs
        // — raised here unwinds through a guard that is still undecided and erases the whole
        // expression: the left-hand side, every folded cycle and all of their emissions, on the
        // same input where the committing settle keeps every one of them. The offsets ride out in
        // `Fault::Stall` and the wrapper commits before it asserts and before it builds, so the
        // two profiles restore the same thing and the `Pratt` contract's two exits stay two.
        //
        // The rollback above the return is the exception, and it is one: it settles this frame's
        // own probe, a local that cannot outlive the frame, and it is the first half of the
        // `Stall` posture rather than work done alongside it. Nothing else runs between the branch
        // and the return.
        if after_report <= committed {
          txn.rollback_abandoning_points();
          return Err(Fault::Stall {
            at: after_report,
            committed_before: committed,
            channel: StalledChannel::Rhs,
          });
        }
        // THE NARROWING. Every exit that restores is behind this line; everything ahead of it
        // is commit-by-default, and was already commit-by-default when the guard spanned it —
        // `?` out of the fold kept, and so does this. The probe's checkpoint is released here
        // rather than after the fold, which is the entire space fix for the postfix arm, and
        // the reason `fold_postfix` and `wrap_at` below take `inp` and not `&mut *txn`.
        //
        // What the release also gives up: the probe's PIN. A live guard pins its begin point and
        // a checked restore below a live pin panics at the cause, in release, in every allocator
        // build. Past this line the only pin left is the expression base, so a fold that rewinds
        // to a target between the two is no longer refused where it happens — see the
        // crossing-rewind section on `Pratt`. Deliberate, bounded, and not "uniformly broader".
        txn.commit();
        let kind = cst.classify(PrattFoldOp::Postfix(&operator));
        lhs = fold_postfix
          .fold_postfix(inp, lhs, Precedenced::new(operator, op_power))
          .map_err(Fault::Keep)?;
        Cst::wrap_at(inp, cst_mark, kind);
      }
      PrattRHS::Infix(infix) => {
        let lpower = infix.precedence();
        let floor = match infix.token_ref() {
          // Right-associative: the right operand admits this operator's own power, so the
          // equal-power operator to the right is consumed by the inner call. Descending one
          // level below it — the arithmetic this replaces — admitted a strictly *weaker*
          // operator into the right operand.
          PrattInfix::Right(_) => PrattFloor::Inclusive(lpower.clone()),
          // Left- and non-associative: the right operand stops strictly above this power,
          // so the equal-power operator folds into this call instead.
          PrattInfix::Left(_) | PrattInfix::Neither(_) => PrattFloor::Exclusive(lpower.clone()),
        };

        // THREE TESTS, AND THE ORDER IS THE CONTRACT: floor, then report boundary, then the
        // chain constraint. Each one's precondition is the previous one's answer.
        //
        // Below the floor: this operator belongs to an enclosing expression. Hand the deciding
        // read back and end this one — an ordinary decline, and not an error.
        //
        // FIRST, deliberately: an operator the floor declines is not this expression's to judge,
        // so it can neither be held to this expression's report boundary — a declined report
        // legitimately consumes nothing, which is `ParsePrattRHS`'s own exemption — nor trip this
        // expression's chain constraint.
        if !min_precedence.admits(lpower) {
          txn.rollback_abandoning_points();
          break;
        }

        // The report boundary again, and here it is also the recursion guard: a
        // right-associative report admits its own power, so an inner call handed the same
        // zero-width report re-reports it and descends again — without the check, until the
        // stack runs out. Restore here, assert and report in the wrapper: same shape and same
        // reason as the Postfix arm above, where the argument is written out.
        //
        // SECOND, and ahead of the repeat guard below rather than behind it, because the two
        // answer questions of different rank. A report that consumed nothing is a **contract
        // violation by the RHS classifier** — grammar code that admitted an operator and left the
        // input where it was — and this exit is the one that says so, terminally. The repeat is a
        // statement about the *input*: malformed, non-terminal, and spendable by a recoverer.
        // Ordered the other way, a classifier that re-reports the chain's own operator at zero
        // width has its bug re-described as the user's bad input and handed to recovery, which
        // then spends it and re-enters a cycle that will report exactly the same thing — the
        // non-terminating shape this boundary exists to refuse. A parser that cannot advance
        // cannot produce a meaningful diagnosis of what it is reading, so the violation wins.
        if after_report <= committed {
          txn.rollback_abandoning_points();
          return Err(Fault::Stall {
            at: after_report,
            committed_before: committed,
            channel: StalledChannel::Rhs,
          });
        }

        // THE NON-ASSOCIATIVE REPEAT. This frame folded a `Neither` operator at exactly this
        // power, and here is a second infix at the same power — any associativity, since the
        // constraint is a property of the chain and not of the newcomer's variant. Declining it
        // instead would destroy the only copy of that fact: the enclosing frame sees an ordinary
        // admissible operator, folds it by its own rules, and `a = b ; c ; d` parses to
        // completion as `((a = (b ; c)) ; d)` with nothing left over for any caller to reject.
        //
        // THIRD, and by the time it is reached both of the questions above are settled: the
        // operator is this expression's, and it was really consumed. So this branch judges an
        // operator that exists, which is what makes its offset meaningful.
        //
        // Restore, then return the posture; the wrapper commits and builds the report. The
        // restore is the `End` arm's own — the same reconciling verb, handing the operator back
        // unconsumed so the position a caller sees is the offending operator's — and it is the
        // first half of this posture rather than work done alongside it. Nothing between the
        // branch and the return can fail: `latched_at` was read before the classifier ran and is
        // moved.
        //
        // `filter` rather than a second `if`, because `latched_at` is `Some` exactly when
        // `prev_op_is_neither` is: both are read from that one latch inside this one cycle, the
        // peek above under `is_some()` and the equality here. The pair therefore cannot drift, and
        // the `Option` carries the offset rather than an assertion that it exists.
        if let Some(at) = latched_at.filter(|_| prev_op_is_neither.as_ref() == Some(lpower)) {
          txn.rollback_abandoning_points();
          return Err(Fault::NonAssoc { at });
        }

        let next_neither = if matches!(infix.token_ref(), PrattInfix::Neither(_)) {
          Some((*lpower).clone())
        } else {
          None
        };
        // THE NARROWING, and this arm is the one it is for: the recursive operand parse below
        // is where a right-associative chain descends, and holding this checkpoint across it
        // is what pinned one per frame. The last restoring exit — the floor decline and the
        // non-associative repeat above, the stall a statement up — is already behind us, so
        // there is no decision left for this guard to serve.
        //
        // Same forfeit as the Postfix arm, and here it spans the recursion too: past this line
        // the cycle base is neither pinned nor in the lineage, so a rewind out of the recursion,
        // the fold or the wrap to a target above the expression base and below this point is no
        // longer refused at the cause. The crossing-rewind section on `Pratt` states what does
        // and does not still catch it.
        txn.commit();
        let kind = cst.classify(PrattFoldOp::Infix(infix.token_ref()));
        let rhs = parse(
          inp,
          parse_lhs,
          parse_rhs,
          fold_prefix,
          fold_infix,
          fold_postfix,
          floor,
          cst,
        )?;
        lhs = fold_infix
          .fold_infix(inp, lhs, rhs, infix)
          .map_err(Fault::Keep)?;
        Cst::wrap_at(inp, cst_mark, kind);
        prev_op_is_neither = next_neither;
      }
    }

    // Secondary. Reached only on the fold-continue paths — every exit above leaves the loop
    // first — and the report boundary has already proven *this* cycle's operator was consumed,
    // so what is left for this to catch is the other direction: a fold, or a recursive operand
    // parse, that rewound the input behind the operator it was handed. Same metric, same `<=`.
    //
    // The refusal must still RESTORE and not merely report — the whole reason the old check ran
    // one statement early, inside the cycle's scope. That scope is gone: the probe was committed
    // the moment the operator was accepted, so this frame has no live guard of its own to roll
    // back through, and reopening one around the recursion would put the per-depth retention
    // straight back. So the posture is named instead — `Fault::Rewind` — and the expression-scoped
    // guard in `ParseInput::parse_input` performs the restore. What it takes back is a superset
    // of what the cycle guard did: the whole expression, not this cycle.
    //
    // The assertion goes with it, and stays *after* the restore. The reason it always gave for
    // that ordering — "`Commit` keeps on drop, unwind included" — was true of the guard family
    // before the unwind edge was settled crate-wide, and is now true only under `no_std`; the
    // expression guard is `Rollback` policy precisely so its restore does not depend on that
    // fact at all. The ordering is kept anyway, belt-and-braces, and it now lives in the wrapper
    // — which is why the two offsets ride along in the posture rather than being compared here.
    //
    // The terminal report goes with it too, and on THIS exit that is uniformity rather than
    // necessity: a panic building it here would unwind through the same guard that is about to
    // roll back, so the two converge. `Fault::Stall`'s do not, and one enum cannot carry an error
    // for one posture and not the other without inviting the next edit to add it back to both. So
    // neither carries one, and `parse` does not even hold the bounds that would let it build one.
    let new_committed = inp.span().end();
    if new_committed <= committed {
      return Err(Fault::Rewind {
        at: new_committed,
        committed_before: committed,
      });
    }
    committed = new_committed;
  }

  Ok(lhs)
}

/// The one exit for a prefix report that committed nothing.
///
/// The LHS twin of [`stalled_rhs_report`], and the same posture for the same reason: a report
/// that took no input is a grammar bug, not an ending, so the driver raises the crate's
/// end-of-expression error for the channel that broke its contract — here the **LHS** one,
/// since a prefix operator is an LHS report — marked terminal, because no amount of input
/// clears it and no recoverer may fabricate a value from it.
///
/// **Nothing is rolled back, and nothing needs to be.** The RHS twin rolls its cycle's
/// transaction back because a fold or a recursion may already have moved the input; this exit
/// is taken *before* the recursion, the fold and the wrap, and its own precondition is that
/// committed consumption did not advance — so there is no progress of this report to restore,
/// and the driver has no business erasing whatever the LHS parser chose to emit on its way to
/// the violation.
///
/// That last clause is why the driver's posture is [`Fault::Stall`] and why **this function is not
/// called from the driver at all**. "Do not erase what the LHS emitted" is only true if nothing
/// panics on the way out; the expression-scoped guard is undecided throughout `parse`, so an
/// assertion raised at the detection site erases exactly those emissions in debug — and so does a
/// panicking `L::Offset::clone` or a panicking grammar `From`, which is what building this error
/// runs, in both profiles. The one caller is [`ParseInput::parse_input`], after its commit; `parse`
/// does not carry the `From<UnexpectedEoLhs<…>>` bound, so it could not call this even by mistake.
#[cold]
#[inline(never)]
fn stalled_prefix_report<'inp, L, Ctx, Lang: ?Sized>(
  at: L::Offset,
) -> <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEoLhs<L::Offset, Lang>>,
{
  UnexpectedEoLhs::eolhs_of(at).into_terminal().into()
}

/// The one exit for a pratt cycle that committed nothing.
///
/// Two driver postures, one condition. At the **report boundary**: an `Infix`/`Postfix` report the
/// floor admitted that consumed nothing, so no phantom fold runs and nothing of the cycle
/// reaches the CST seam. At the **foot of the cycle**: a fold, or a recursive operand parse,
/// that rewound behind the operator it was handed.
///
/// Both restore before this is built, and both must. **They no longer restore the same thing, or
/// through the same guard**, and the difference is the part that is easy to get wrong twice:
///
/// * The **report boundary** sits *before* the operator is accepted, so that frame's own probe
///   transaction is still live. The driver rolls the deciding read back on the spot and reports
///   [`Fault::Stall`] — the restore has already happened, and it is cycle-scoped: the expression's
///   earlier progress and diagnostics stay, and the wrapper's settle for that posture is a commit.
/// * The **foot of the cycle** sits *after* the probe was committed, deliberately, because
///   holding it across the recursion is what made checkpoint retention grow with operator depth.
///   That frame therefore has no guard of its own left to roll back through, and it does **not**
///   restore: it reports the posture, as [`Fault::Rewind`], and the expression-scoped guard in
///   [`ParseInput::parse_input`] performs an **expression-scoped** restore. That is wider than
///   the cycle rollback it replaces — it also erases every emission the expression made — and the
///   width is a published contract on [`Pratt`] rather than something sold as free.
///
/// What both postures preserve is the ordering: the settle each is owed happens before this error
/// exists. Build it a statement earlier and the input position, the emission log and the CST events
/// of the fold that rewound are still live when the constructor runs — which matters because the
/// constructor is *fallible caller code*: `L::Offset::clone` and the grammar's own `From`. A panic
/// in either, raised inside `parse`, unwinds through an undecided expression guard and takes the
/// whole expression, which is the failure this shape exists to prevent and is the reason
/// **`parse` does not call this function** and does not carry the bound that would let it.
///
/// **Nor does anything assert in the driver's frame**, which is the same discipline one step
/// earlier. The assertions belong to whichever restore has already happened — cycle-scoped for the
/// report boundary, expression-scoped for the foot of the cycle — and a `debug_assert` raised in
/// `parse` would belong to neither. Both postures therefore carry their offsets to
/// [`ParseInput::parse_input`](ParseInput::parse_input), which settles, then asserts, then calls
/// this — committing for `Stall`, restoring for `Rewind`.
///
/// Both are grammar bugs, not malformed input, and neither may simply end the expression:
/// that would hand the caller an `Ok` over a truncated parse with no diagnostic on any
/// channel, which is exactly the class of failure the position pre-gate used to produce. The
/// vocabulary is the crate's end-of-RHS error, marked terminal by the same rule a scanner stop
/// is: no amount of input clears it and no recoverer may fabricate a value from it, so
/// [`MaybeTerminal`](crate::error::MaybeTerminal) makes recovery re-raise it untouched.
#[cold]
#[inline(never)]
fn stalled_rhs_report<'inp, L, Ctx, Lang: ?Sized>(
  at: L::Offset,
) -> <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEoRhs<L::Offset, Lang>>,
{
  UnexpectedEoRhs::eorhs_of(at).into_terminal().into()
}
