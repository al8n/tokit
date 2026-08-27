use core::marker::PhantomData;

use crate::{
  Commit, Rollback,
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
  Lang: ?Sized,
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
/// [`RecursionLimiter::PARSE_DEFAULT_DEPTH`](crate::state::recursion_tracker::RecursionLimiter::PARSE_DEFAULT_DEPTH)
/// unless the context says otherwise — and a deeper expression fails with the terminal
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) rather than exhausting the
/// native stack. The budget belongs to the *input*, not to this parser, so nested expression
/// parsers share it.
///
/// **Rollback granularity**, which the narrowing makes two scopes rather than one:
///
/// - **cycle-scoped**, for the six exits where the deciding read is handed back untouched:
///   [`PrattRHS::End`], a report the floor declines, either report-boundary stall (an admitted
///   `Infix`/`Postfix` report that consumed nothing), the non-associative repeat — which ends the
///   *parse* rather than the expression, with the operator left on the input and
///   [`NonAssociativeChain`](crate::error::NonAssociativeChain) returned — and the
///   [`Adjacent`](PrattRHS::Adjacent) **debt** refusal, a zero-token continuation reported in a
///   frame that has committed nothing since the enclosing one descended. The probe guard is still
///   live for all six, so whatever the RHS parser consumed while deciding is handed back untouched
///   to the surrounding grammar. `Adjacent`'s *other* refusal — a continuation whose operand
///   consumed nothing — is not among them: it is decided after the probe was committed and settles
///   by committing, so there is nothing of it to hand back.
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
/// scopes above a property of the *code* rather than of the build profile. Its six contract
/// assertions — the two report-boundary stalls, the prefix stall, the adjacency charge, the
/// adjacency debt and the foot-of-cycle refusal —
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
/// — an [`End`](PrattRHS::End), an operator the floor declines, a non-associative repeat, a
/// report that consumed nothing, or a zero-token continuation the enclosing one has already been
/// paid for — which is the same narrow discard any speculative parse makes,
/// and not this contract.
///
/// **Two in every build**, and that is part of the promise rather than a detail of it. The three
/// report-boundary contract violations — a `Prefix` report that consumed nothing, and its
/// `Infix`/`Postfix` twin in either arm — together with the two
/// [`Adjacent`](PrattRHS::Adjacent) refusals, whose obligation sits on the operand and on the
/// frames below rather than on the report but whose posture is theirs exactly, are reported as a
/// *terminal error* in release and additionally raise a
/// `debug_assert` in a debug build. That assertion fires in the wrapper,
/// after the expression guard has committed, so the debug build's extra panic is not an extra
/// restore: both profiles keep the expression's input and emissions on those five exits, and
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
  ///
  /// # This knob is half of the *restriction* idiom
  ///
  /// rust-analyzer and rustc both carry a `Restrictions` bitset into expression parsing —
  /// "no struct literal here", "this is a statement position", "stop before `|`" — and it is worth
  /// saying where the equivalent lives, because tokora has no such type and a reader can conclude
  /// from that that the idiom is unavailable. It is not; it is split across two places that are
  /// documented apart:
  ///
  /// * **the part that is a precedence** is this method. "Parse an expression, but stop before
  ///   any operator weaker than `p`" is a floor, and the floor is the driver's own — it applies to
  ///   every report, including those made by a recursion the grammar did not write.
  /// * **the part that is a mode** is state on the channel *value*. `parse_lhs` and `parse_rhs`
  ///   are ordinary parsers you construct, so a channel that is a struct with a
  ///   `forbid_structs: bool` — or any other flag — carries the mode without the driver knowing it
  ///   exists. Build one `Pratt` per mode, or read the flag in the classifier and report
  ///   [`End`](PrattRHS::End) where the mode forbids continuing.
  ///
  /// The one shape neither half covers is an operator whose *report variant* depends on what
  /// follows it — `0..` as a postfix where `0..b` is an infix. That is the classifier's own
  /// decision and it needs nothing from here: [`ParsePrattRHS`] holds a whole
  /// [`InputRef`](crate::InputRef) and may look as far ahead as it likes before choosing a
  /// variant, provided it consumes what it finally reports.
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
  // bundles — but this driver builds them itself rather than emitting them, so it names the two
  // `From`s here and does not require `PrattEmitter` at all.
  //
  // `NonAssociativeChain` joins them for the same reason and at the same place: the wrapper
  // builds it, after settling, from an offset the driver captured. `RecursionLimitReached` is the
  // one conversion `parse` also carries, because the value that needs it is built by
  // `InputRef::descend` at the frame prologue — before any posture exists to be disturbed — and
  // not by a deferred effect. All four are *returned* by this driver, never emitted, so all four
  // are stated as direct `From` obligations here; `FromPrattError` covers only the two an emitter
  // body converts, and none of these four go through an emitter body.
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
  /// posture — the three report-boundary contract violations plus the adjacency refusal, all
  /// four of which have already taken the only restore they are owed — carrying its
  /// `debug_assert` out of `parse` so that this method can raise it *after* committing. Left where the violation is detected, that assertion is a
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
  /// Five sites construct `Stall` and two construct `Rewind`. Both effects live here, once,
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
      // No adjacency is outstanding at the root: nothing above this expression descended on a
      // zero-token continuation, so the first one it meets is owed no advance.
      None,
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
          StalledChannel::Adjacency => {
            debug_assert!(
              at > committed_before,
              "pratt: an Adjacent continuation's right operand consumed nothing — a zero-token \
               operator is paid for by its operand (see PrattRHS::Adjacent)"
            );
            stalled_rhs_report::<L, Ctx, Lang>(at)
          }
          StalledChannel::AdjacencyDebt => {
            debug_assert!(
              at > committed_before,
              "pratt: an Adjacent continuation descended with no input committed since the \
               enclosing one — one advancement pays for one zero-token operator (see \
               PrattRHS::Adjacent)"
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
///   terminal report — and nothing else: it settles by committing, exactly as `Keep` does. Five
///   construction sites, all of them "the expression continues here and nothing was consumed to
///   say so" — the two report-boundary stalls, which have already rolled their own cycle back
///   through a probe guard that was still live at that point; the prefix stall, which by its own
///   precondition has nothing to restore; and the two adjacency refusals, which are the same law
///   measured where a zero-token operator can be held to it, since it has no spelling for a report
///   boundary to measure. Of those two the *charge* measures this cycle's operand and by its own
///   precondition — the cycle advanced nothing — has nothing to restore, while the *debt* test
///   measures the frames above and rolls its own cycle back through a still-live probe as the
///   report boundaries do. None of them may assert **or report** in place: the cycle-scoped restore
///   they take (or do not need) is the *narrow* one the surrounding grammar is owed, and any panic
///   raised before returning would replace it with the expression-scoped one.
/// * [`NonAssoc`](Self::NonAssoc) is `Stall` with one deferred effect instead of two: no
///   assertion (this is malformed *input*, not a grammar bug) and a **non-terminal** report,
///   `NonAssociativeChain`. Two construction sites, the repeat guard in the `Infix` arm and its
///   twin in the `Adjacent` one, which — like the two report-boundary stalls — have already
///   handed the deciding read back through a still-live probe guard. It settles by committing for
///   exactly their reason: the narrow restore is the one that was owed, and the expression's
///   earlier folds and diagnostics are not this operator's to erase.
/// * [`Rewind`](Self::Rewind) has **two** construction sites, and they are one refusal read at
///   two points: the foot of the cycle, and the `Adjacent` arm's charge, which reaches the same
///   condition a fold earlier because it must decide *before* the fold it prices. It carries the
///   two offsets so the wrapper can assert and report *after* the restore rather than before —
///   the ordering discipline the check has always had, moved out with it.
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
/// `NonAssoc`'s offset obeys the rule with **no arrangement at all**, which is the strongest form
/// available: it carries `committed`, the cycle's own progress watermark, and carries it by
/// **move**. Nothing is read, cloned or peeked to build this posture, so the window the rule is
/// about does not exist for it — not "is empty", but has no statement in it.
///
/// That is also, separately, what makes the value *right*. `committed` is `inp.span().end()` read
/// at the one program point that `begin_with` snapshots into the probe's checkpoint, and
/// `rollback_abandoning_points` — the first half of this posture — installs that snapshot back. So
/// the offset and the position the caller is handed are one value from one read, and
/// `inp.span().end() == at` holds after the return for every grammar shape.
///
/// The rule and the correctness argument used to pull against each other here, and that is worth
/// recording because it is what a post-restore read would revive: reading the resumption position
/// *after* the rollback is the most direct spelling of "the offset is where the input is", but it
/// puts an `L::Offset::clone` — caller code, fallible in both profiles — between the branch that
/// decided this posture and the return that honours it, which is exactly what the rule forbids.
/// Taking the restore target instead needs no read, so there is nothing left to rank.
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
    /// Committed consumption read where `channel`'s own law is decided, and the offset the
    /// terminal error is built from once the guard has committed. That is the report boundary for
    /// the three channels whose law is "consume what you report" — including
    /// [`AdjacencyDebt`](StalledChannel::AdjacencyDebt), whose law is read at the same boundary
    /// one step before the descent — and the position the recursion returned at for
    /// [`Adjacency`](StalledChannel::Adjacency), whose law is about the operand.
    at: Off,
    /// The value `at` failed to exceed: committed consumption where the measured span began, or —
    /// for [`AdjacencyDebt`](StalledChannel::AdjacencyDebt) — the inherited watermark it was held
    /// against.
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
    /// The **handback position**: the probe's own restore target, moved out of the cycle's
    /// `committed` watermark rather than measured anywhere near it. After the rollback that
    /// precedes this posture, `inp.span().end()` is this number. It sits at or before the repeated
    /// operator's own start — strictly before it whenever anything the caller is also handed back
    /// sits in between: whitespace the lexer skips, trivia tokens a `ParsePrattRHS` would skip, or
    /// a region a non-fatal lexer error was reported over.
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

/// Which side of "the expression continues here, and something was consumed to say so" was
/// broken — the only thing that differs between the five [`Fault::Stall`] sites once the
/// assertion and the report have both been moved to the wrapper, and so the thing that picks both
/// the assertion's wording and the report's constructor there.
///
/// Three of the five are the "consume what you report" violation, measured at the report
/// boundary. The other two are the same law measured where a zero-token operator can be held to
/// it, since its report is exempt for having no spelling: [`Adjacency`](Self::Adjacency) puts the
/// obligation on the **operand** and reads it when the recursion returns, and
/// [`AdjacencyDebt`](Self::AdjacencyDebt) puts it on the **frames below** and reads it before the
/// recursion happens. Two readings and not one, because a charge taken on the way back up prices
/// nothing that the way down already built.
///
/// Both of those two are measured *from* the report boundary even so, and that is the exemption's
/// other half rather than a contradiction of it: an exempt classifier may have consumed, and what
/// it consumed pays the adjacency its own frame is already inside — never the one it is reporting.
///
/// A fieldless marker rather than the `&'static str` message itself, and the reason is size:
/// `Fault` is the error half of the `Result` the driver returns through **every** frame of its
/// own recursion, and a `&str` field is two words where a fieldless marker is free. Free, not
/// one byte, and measured rather than hoped for — see the tripwire below. The wordings stay
/// as literals at the assertion instead, which also keeps a `should_panic(expected = …)` matching
/// text that actually appears in the source.
#[derive(Clone, Copy)]
enum StalledChannel {
  /// A [`PrattLHS::Prefix`] report that consumed nothing.
  Lhs,
  /// A [`PrattRHS::Infix`]/[`Postfix`](PrattRHS::Postfix) report the floor admitted that consumed
  /// nothing.
  Rhs,
  /// A [`PrattRHS::Adjacent`] continuation the floor admitted whose right operand consumed
  /// nothing past the position the classifier left the input at — so nothing paid for this
  /// continuation, and the next cycle would report the same one over the same bytes.
  ///
  /// Read from the report boundary rather than from the top of the cycle, because whatever an
  /// exempt classifier took on its way to the report is the *enclosing* adjacency's payment and
  /// never this operand's.
  Adjacency,
  /// A [`PrattRHS::Adjacent`] continuation the floor admitted in a frame that has committed
  /// nothing — the reporting classifier's own consumption included — since the enclosing
  /// continuation descended, so descending would stack a second zero-token frame on input the
  /// first one has already been paid with.
  ///
  /// The same law as [`Adjacency`](Self::Adjacency), read one step earlier: that one prices this
  /// cycle's operand once the recursion has returned, this one prices the recursion before it
  /// happens. A charge that only ever runs on the way back up cannot bound what the way down
  /// builds.
  AdjacencyDebt,
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
/// and the discriminant still rides [`StalledChannel`]'s niche, which needs three spare patterns
/// and — now that a fourth channel names the adjacency debt as well as the charge — has 252. The
/// rows below re-check it in four instantiations including the all-align-1 one.
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
  // THE OUTSTANDING ADJACENCY, and the second thing a frame inherits from its caller after the
  // floor. `Some(p)` means this frame is the right operand of a [`PrattRHS::Adjacent`]
  // continuation that descended at committed position `p`; `None` means no zero-token
  // continuation is outstanding above it. The `Adjacent` arm below refuses to descend again until
  // committed consumption has passed `p`, and descends carrying its own position — so the value
  // is strictly increasing along any root-to-leaf path, and one advancement cannot discharge two
  // adjacencies. That arm states why a frame-local charge alone does not bound the nesting.
  //
  // `p` IS WHERE THE ENCLOSING CLASSIFIER LEFT THE INPUT, not where its cycle started. A report
  // exempt from the report boundary may legally have consumed on its way to being made, and those
  // bytes discharge the adjacency that frame was already inside rather than paying for the one it
  // is reporting — see the rule stated in the `Adjacent` arm. Where the classifier consumes
  // nothing the two positions coincide, which is why the distinction is invisible in most
  // grammars and load bearing in a CST-shaped one.
  //
  // A BORROW, not an owned offset: what it points at is the enclosing frame's own `after_report`,
  // a local of the loop turn that made the descent and so alive for the whole of the call, and
  // passing it by reference is what keeps the pass-through sites — the prefix operand and the
  // infix right operand, neither of which is an adjacency — free of an `L::Offset::clone` each.
  adjacency_watermark: Option<&L::Offset>,
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
  // re-opens that window, so it fails to compile here rather than reviewing clean at the six
  // sites.
{
  // ONE FRAME, ONE LEVEL — ahead of the CST mark, the LHS watermark and every read, because at
  // this line the frame owns nothing that a trip would have to restore. Every recursion site
  // below (the prefix operand, the infix right operand, the adjacency's right operand) and the
  // `parse_input` root each enter through their own prologue, so depth equals the native pratt
  // depth it protects with no per-site bookkeeping — the third site inherited it for free, and a
  // fourth would too. The guard releases the level on every exit of this function, unwind
  // included, in `std` and `no_std` alike.
  //
  // It is a bound on what the MACHINE has, never on what the document bought: a one-byte input
  // whose classifier escalates can reach this limit and fail with `RecursionLimitReached` while
  // every input-side law is still satisfied. That gap is the adjacency debt's, not this guard's.
  //
  // `Fault::Keep` for the trip: nothing of *this* frame is consumed, and the enclosing
  // expression's progress and emissions are owed to it exactly as they are on any other
  // `?`-propagation out of caller code.
  let mut frame = inp.descend().map_err(Fault::Keep)?;
  let inp = &mut *frame;

  // …AND ONE FRAME, ONE STACK CHECK, on the same line and for the same frame. Composed with
  // the level above, never substituted for it: `descend` is what REFUSES a too-deep input, and
  // this is only what decides which stack the frame it just admitted runs on. With the
  // `stacker` feature off it is the identity and this closure is the frame body verbatim.
  //
  // Inside the level and not outside it, so a trip is still decided before any stack is
  // allocated for a frame that is not going to run — and inside the `Descent`'s scope, so the
  // guard whose destructor releases the level stays on the CALLER's stack rather than on a
  // segment that is unmapped before the unwind reaches it.
  crate::native_stack::maybe_grow(move || {
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
          // PASSED THROUGH, neither cleared nor raised. Clearing it is the whole defect back
          // again: a grammar alternating `Adjacent` with a zero-width prefix would descend a
          // frame per level with the debt forgotten at every other one. Raising it to this
          // frame's position would be stricter than the rule needs — the operand below is welcome
          // to spend the prefix operator's own byte, which the report boundary a statement above
          // has already proved was committed.
          adjacency_watermark,
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
    //
    // IT IS ALSO THE NON-ASSOCIATIVE TRIP'S OFFSET, and that is not a second job bolted on: this
    // value and the probe's rollback target are the same read of the same field. `begin_with` a few
    // lines down snapshots `inp.span()` into its checkpoint with **no statement in between** — read
    // here on the first cycle, at the foot of the loop on every later one — and
    // `rollback_abandoning_points` installs that snapshot back verbatim. So after any restoring exit
    // `inp.span().end() == committed`, by identity rather than by agreement, and the offset a repeat
    // reports is the position the caller is measurably at. See the repeat guard below, and
    // `NonAssociativeChain`'s own documentation for why the offset is *defined* that way.
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
      // THE ROLLBACK VERB, decided once here for all six restoring exits below. Every one of them
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
          // restore is the `End` arm's own — the same reconciling verb, handing back everything the
          // classifier consumed while deciding, the operator included — and it is the first half of
          // this posture rather than work done alongside it. Nothing between the branch and the
          // return can fail: `committed` is an `L::Offset` this frame already owns, and it is
          // **moved**, not cloned.
          //
          // THE OFFSET IS THE RESTORE TARGET, NOT SOMETHING MEASURED NEAR IT. `committed` is the
          // value `begin_with` snapshotted into this probe's checkpoint (see its declaration above
          // the loop) and the value the line above just installed back, so the number this posture
          // carries and the position the caller is at after the handback are one value from one
          // read — `inp.span().end() == at`, checkable by the caller and pinned that way in
          // `pratt_limit.rs`.
          //
          // That is what closes the class three review rounds kept re-finding. Every earlier source
          // was adjacent to the restore target rather than equal to it, and each one drifted on a
          // different input: the committed span read AFTER the classifier names the classifier's
          // last token — an operator's tail for a multi-token spelling (`not in`, `is not`, `<>`) —
          // and a `peek_one` taken BEFORE it names the first token the scanner can produce, which
          // skips whatever the scanner skips. Trivia the classifier would have skipped, a
          // multi-token operator's gap and a non-fatal lexer error the peek stepped over and emitted
          // are all bytes that sit between the restore target and that first token; each in turn was
          // reported as a position the caller had not been handed. The restore target has no such
          // window: nothing runs between the read and the checkpoint, and the rollback's whole job is
          // to put that checkpoint back.
          //
          // What this offset does NOT claim is the offending operator's head, and that is not a
          // shortfall the driver could close. `parse_pratt_rhs` is caller code holding a whole
          // `InputRef` and decides for itself where its operator begins; learning how much it would
          // skip means running it, and running it before the repeat is decided is what this scope's
          // transaction rules forbid. `NonAssociativeChain`'s own documentation states the
          // definition, and the token engine's park site states why the two coincide there.
          if prev_op_is_neither.as_ref() == Some(lpower) {
            txn.rollback_abandoning_points();
            return Err(Fault::NonAssoc { at: committed });
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
            // PASSED THROUGH, for the prefix operand's reason exactly. The operand below may
            // spend this operator's own byte on an adjacency of its own — that is a byte the
            // report boundary above proved committed — but the debt does not vanish because a
            // spelled operator stood between the two zero-token ones.
            adjacency_watermark,
            cst,
          )?;
          lhs = fold_infix
            .fold_infix(inp, lhs, rhs, infix)
            .map_err(Fault::Keep)?;
          Cst::wrap_at(inp, cst_mark, kind);
          prev_op_is_neither = next_neither;
        }
        // THE ZERO-TOKEN CONTINUATION. The `Infix` arm with the report boundary taken out and the
        // charge it bought put back somewhere the operand can pay it. Everything that arm does for
        // reasons unrelated to having consumed a token, this arm does identically.
        PrattRHS::Adjacent(precedenced) => {
          let (operator, op_power) = precedenced.into_components();

          // FLOOR FIRST, and for the `Infix` arm's reason exactly: an operator the floor declines
          // is not this expression's to judge, so it trips none of this expression's constraints.
          //
          // That this test runs at all is the point of the variant. The alternative a grammar has
          // without it — report the already-parsed right operand as a `Postfix` — puts the right
          // operand's floor in the production, where the driver's own precedence laws do not
          // reach it. Here the driver keeps it.
          if !min_precedence.admits(&op_power) {
            txn.rollback_abandoning_points();
            break;
          }

          // NO REPORT BOUNDARY, AND THAT IS NOT AN OMISSION. `after_report` is measured for every
          // other admitted report because "consume what you report" is a law about an operator
          // that has a spelling; this operator has none, so the law has nothing to measure. Both
          // outcomes are legal here: a classifier that peeked and consumed nothing, and a
          // CST-shaped one that consumed the trivia sitting between two adjacent operands before
          // deciding they were adjacent. What the boundary bought — a guarantee that this cycle
          // cannot repeat over the same bytes — is bought in two places instead: the charge below,
          // off this cycle's operand, and the debt test immediately following, off the frames
          // above.
          //
          // `after_report` IS STILL THE LINE ALL THREE OF THIS ARM'S MEASUREMENTS SIT ON, and that
          // does not follow from the exemption — it is the second half of the same fact. The
          // exemption says the classifier MAY have moved the input between `committed`, which this
          // loop reads at the top of the turn and which therefore names the position before
          // `parse_pratt_rhs` ran, and `after_report`, which is the position it left. Something
          // then has to say where those bytes go, and the rule is:
          //
          //     CLASSIFIER CONSUMPTION PAYS THE ENCLOSING ADJACENCY, NEVER THE CONTINUATION THE
          //     CLASSIFIER REPORTS.
          //
          // Bytes taken before the report discharge the debt the frames above are owed; bytes
          // taken after it pay for this continuation. Every byte pays exactly one debt and none
          // pays two. Measured from `committed` the same trivia pays twice and in both directions
          // at once: it satisfies this cycle's charge with input the right operand never consumed
          // — a fold, a wrap and another turn for bytes that were the operator's — while staying
          // invisible to the debt test, which is the one obligation it really does discharge, so a
          // legal parse is refused and refused terminally.
          //
          // The structural bound survives this and tightens under it. `after_report` is never
          // behind `committed` on any input the driver does not already refuse, so the watermark
          // rises at least as high as it did, the strict increase the debt test demands is
          // unchanged, and the descents along a root-to-leaf path still sit at strictly increasing
          // committed offsets.
          //
          // THE ADJACENCY DEBT, and it is the *prospective* half of the same law. The charge below
          // is frame-local and retrospective — it runs only once the recursive operand parse has
          // returned — so on its own it prices this frame's cycles and nothing above them. A
          // stateful classifier is contract-valid and may make its powers a function of the input:
          // report a strictly increasing power at every level over zero-width operands, consume a
          // single byte in the deepest frame, and every ancestor compares that byte against the
          // same committed position it descended from. One byte then passes every charge and buys
          // k frames, k folds and k CST wraps, with the escalation reaching the recursion limit
          // before any charge executes. The power ladder is the grammar's only when the powers
          // come from the grammar, so `Exclusive` below bounds the *same-power* chain and nothing
          // else.
          //
          // What bounds the rest is inherited rather than local. `adjacency_watermark` is the
          // committed position the nearest outstanding continuation descended at, and this test
          // refuses to descend again until committed consumption has passed it — strictly. The
          // descent below then carries THIS turn's position, so the watermark strictly increases
          // at every zero-token descent along a root-to-leaf path and never decreases anywhere
          // else. A single advancement therefore discharges exactly one adjacency: the next one
          // down is measured against the position that advancement reached, not against the one
          // its parent was measured against. `Adjacent` frames on a path are consequently no more
          // numerous than the distinct committed offsets between the outermost and the innermost
          // — bytes, on any input this crate parses.
          //
          // MEASURED AT `after_report`, by the rule above: the classifier's own bytes are the
          // enclosing continuation's payment, so they are exactly what this test must see. Read at
          // `committed` it cannot see them, and a frame whose classifier committed real input past
          // the watermark before reporting is refused for an advance it made.
          //
          // FIRST OF THE TWO REFUSALS, and ahead of the repeat guard below for the reason the
          // `Infix` arm ranks its report boundary there: this is a **terminal** violation of the
          // driver's own progress law, the repeat is *malformed input* a recoverer may spend, and
          // a recoverer that spends the repeat here would re-enter a cycle reporting the same
          // zero-token continuation over the same bytes. The read that decides it, and the copy of
          // the watermark the posture carries, both happen BEFORE the branch commits to a posture
          // — so a panicking `L::Offset` comparison or clone is the ordinary undecided-guard
          // exposure every other line of this function has, and nothing but the rollback runs
          // between the decision and the return.
          //
          // Restore, then the posture, exactly as the `Infix` arm's report boundary does: the
          // probe is still live, the deciding read is handed back through it, and the wrapper
          // commits before it asserts and before it builds.
          //
          // THE TWO OFFSETS ARE THE DECIDING READ AND THE VALUE IT FAILED TO PASS — `after_report`
          // and the watermark — so the wrapper's `at > committed_before` is the negation of the
          // branch that got here, in the same two values, and names THIS rule rather than the
          // operand charge's, whose pair is `after_operand` against `after_report`. It fires here
          // always and by construction, which is the property that makes the debug profile's half
          // of the contract worth having.
          //
          // `at` IS NO LONGER THE RESTORE TARGET, and it does not need to be: `Fault::Stall`'s
          // offset is the measurement its channel's law was decided by, which is what the `Lhs`
          // and `Rhs` channels have always carried. The restore target is still `committed` —
          // `rollback_abandoning_points` reinstalls the probe's checkpoint, which is that read
          // verbatim — and the identity survives anyway rather than being asserted: reaching this
          // branch forces `after_report == committed == owed`, because the frame opened at or past
          // the watermark and committed consumption does not regress across the LHS or the report,
          // so `after_report <= owed` can only hold with all three equal. The number the caller is
          // handed and the number the report carries are therefore still one number, derived where
          // they used to be stipulated. Only an input that has already broken the driver's
          // monotonicity law separates them, and the `Rhs` channel one arm up has that same
          // exposure in the same shape. `NonAssoc` below is where the handback position is
          // definitional, and it still reads `committed`.
          let unpaid = match adjacency_watermark {
            Some(owed) if after_report <= *owed => Some(owed.clone()),
            _ => None,
          };
          if let Some(owed) = unpaid {
            txn.rollback_abandoning_points();
            return Err(Fault::Stall {
              at: after_report,
              committed_before: owed,
              channel: StalledChannel::AdjacencyDebt,
            });
          }

          // THE NON-ASSOCIATIVE REPEAT, unchanged in meaning and third here as it is there.
          if prev_op_is_neither.as_ref() == Some(&op_power) {
            txn.rollback_abandoning_points();
            return Err(Fault::NonAssoc { at: committed });
          }

          // THE NARROWING, as in the `Infix` arm: the last restoring exit is behind us, so the
          // probe has no decision left to serve and is released before the recursion rather than
          // across it.
          txn.commit();

          // LEFT-ASSOCIATIVE, WITH NOTHING TO CHOOSE — and this line is half of the depth bound,
          // not a defaulting. `Inclusive(p)` admits `p` into the right operand, so an inner frame
          // handed a zero-token continuation at `p` reports the same continuation and descends
          // again with no byte consumed between the two. `Exclusive(p)` makes the inner frame
          // decline it, so a same-power chain iterates HERE, in one frame, where the charge below
          // prices every turn. What it leaves open is a strictly *increasing* power chain, which
          // is a property of the grammar's ladder only when the powers come from the grammar; a
          // stateful classifier makes them a function of the input, which is why the other half of
          // the bound is the debt test above and not this line.
          //
          // The fold sees `PrattInfix::Left`, so `fold_infix` and the CST classifier need no arm of
          // their own; a grammar that must tell an adjacency from a spelled left-associative
          // operator does it in the payload, which is where it already tells its spelled siblings
          // apart.
          let infix = Precedenced::new(PrattInfix::Left(operator), op_power.clone());
          let kind = cst.classify(PrattFoldOp::Infix(infix.token_ref()));
          let rhs = parse(
            inp,
            parse_lhs,
            parse_rhs,
            fold_prefix,
            fold_infix,
            fold_postfix,
            PrattFloor::Exclusive(op_power),
            // THE DEBT THIS DESCENT INCURS, and the only site that raises the watermark. Every
            // frame below inherits this position until one of them is itself an adjacency
            // descent, so the operand parse — however deep, and through however many spelled
            // operators — may not take another zero-token continuation until committed
            // consumption has passed the position this one descended from.
            //
            // `after_report`, which is that position: the probe was committed a statement above,
            // so this is literally where the recursion begins. Carrying `committed` instead would
            // spend the classifier's own bytes twice — once discharging the enclosing adjacency,
            // and again admitting a continuation below that added no byte of its own — which is
            // the same double-payment the debt test above refuses, one frame further down. A
            // borrow of a local of this loop turn, which nothing touches until the call returns.
            Some(&after_report),
            cst,
          )?;

          // THE CHARGE, and it is in front of the work it prices. The work a continuation buys is
          // the fold, the CST wrap and another turn of this loop; all three are below this line.
          // The dimension is the one the loop terminates on — committed consumption, the same
          // `span().end()` every other guard in this driver reads — and the population is the
          // continuations themselves, one charge each, none of them refundable by a later cycle.
          // So this frame's loop buys exactly as many zero-token continuations as it has bytes to
          // pay for, and the honest grammar this variant exists for never notices: `labelFilter
          // labelFilter` advances by a whole operand per turn.
          //
          // AGAINST `after_report`, WHICH IS WHAT MAKES IT THE OPERAND'S CHARGE. `committed` is
          // this turn's starting position, and between it and `after_report` lie whatever bytes
          // the classifier took on its way to a report the boundary exempts — the enclosing
          // adjacency's payment, by the rule at the top of this arm, and not this operand's.
          // Charged from `committed` the trivia pays a debt it does not owe: a right operand that
          // consumed nothing buys the fold, the wrap and another turn on the strength of bytes
          // that were the operator's, which is the one outcome this charge exists to refuse and
          // which the public contract answers with "the right operand pays".
          //
          // FRAME-LOCAL, and that is why it is not the whole bound. It runs after the recursion
          // returns, so the frames the recursion built have already been built; what prices those
          // is the debt test above, which runs before the descent. The two are the same law read
          // at the two points a zero-token continuation can be paid for — this cycle's own
          // operand, and the frames stacked underneath it.
          //
          // The two failures are told apart because their restores differ. Moving BACKWARDS is the
          // foot-of-cycle refusal reached early — a hook rewinding out of the recursion — and it
          // takes that posture's expression-scoped restore. Not moving at all is this arm's own
          // contract violation, the operand as empty as the operator, and it settles like every
          // other stalled report: commit, so the position the surrounding grammar is handed is the
          // one this cycle started from and the operand parse's own diagnostics survive to say why
          // it was empty. Reporting the second as the first would erase those diagnostics over a
          // failure that took nothing away.
          //
          // MOVING THE BOUNDARY OPENS A BAND THAT COULD NOT EXIST BEFORE — `after_operand` strictly
          // between `committed` and `after_report`, an operand parse that rewound past the
          // classifier's bytes without reaching the cycle start — and the rule above decides it
          // rather than a new one. The operand was handed the input at `after_report`, because the
          // probe was committed and the recursion entered there; anything below that is the
          // recursion moving backwards behind what it was handed, which is the foot-of-cycle
          // refusal's own sentence and takes its restore. `Stall` keeps exactly the case it
          // described before: the operand that did not move at all.
          //
          // NESTED RATHER THAN AN `if`-EXPRESSION, so the posture rule holds for each arm
          // separately: every comparison here is a branch that has not yet decided a posture, and
          // each posture that IS decided is returned with nothing between the decision and the
          // return. Written as `Err(if a < b { Rewind } else { Stall })` the refining comparison
          // would run after the outer branch had already committed to *some* posture, and a panic
          // in caller-supplied `PartialOrd` would then take the expression-scoped exit on a path
          // that had decided to keep it.
          let after_operand = inp.span().end();
          if after_operand <= after_report {
            if after_operand < after_report {
              return Err(Fault::Rewind {
                at: after_operand,
                committed_before: after_report,
              });
            }
            return Err(Fault::Stall {
              at: after_operand,
              committed_before: after_report,
              channel: StalledChannel::Adjacency,
            });
          }

          lhs = fold_infix
            .fold_infix(inp, lhs, rhs, infix)
            .map_err(Fault::Keep)?;
          Cst::wrap_at(inp, cst_mark, kind);
          // An adjacency folds as `Left`, so it clears the latch exactly as a spelled
          // left-associative operator does. It can trip one; it never arms one.
          prev_op_is_neither = None;
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
  })
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
