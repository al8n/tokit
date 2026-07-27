use core::marker::PhantomData;

use crate::{
  Commit,
  error::{UnexpectedEoLhs, UnexpectedEoRhs},
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
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<UnexpectedEoLhs<L::Offset, Lang>> + From<UnexpectedEoRhs<L::Offset, Lang>>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    parse(
      input,
      &mut self.parse_lhs,
      &mut self.parse_rhs,
      &mut self.fold_prefix,
      &mut self.fold_infix,
      &mut self.fold_postfix,
      PrattFloor::Inclusive(self.min_precedence.clone()),
      &self.cst,
    )
  }
}

// The one private driver threads the whole fold configuration plus the CST seam through
// its own recursion; a parameter bundle would be assembled and torn apart at every
// recursive call for no reader benefit.
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
) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
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
  // infix/postfix report and for the foot of a cycle.
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<UnexpectedEoLhs<L::Offset, Lang>> + From<UnexpectedEoRhs<L::Offset, Lang>>,
{
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
  let mut lhs = match parse_lhs.parse_pratt_lhs(inp)? {
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
      debug_assert!(
        after_report > before_lhs,
        "pratt: a Prefix report consumed nothing — the LHS channel must consume what it \
         reports (see ParsePrattLHS)"
      );
      if after_report <= before_lhs {
        return Err(stalled_prefix_report::<L, Ctx, Lang>(after_report));
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
      let folded = fold_prefix.fold_prefix(inp, operand, Precedenced::new(operator, power))?;
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
    // The cycle's transaction is also the scope the secondary guard has to decide in: a
    // `Commit` guard keeps this cycle's input, emission and CST effects the moment it drops,
    // so a check placed after the scope can only report damage it has already let through.
    let new_committed = {
      // This loop is commit-by-default: it keeps whatever it parsed on every success and on
      // every `?`-propagation (a fail-fast emitter error carries the consumed progress out,
      // exactly as dropping a raw checkpoint used to), and rolls back only on the exits where
      // the next operator is not part of this expression. A `Commit`-policy guard is
      // the structural match: parse through it, let the drop keep progress, and roll back
      // explicitly on those exits.
      let mut txn = inp.begin_with::<Commit>();
      let report = parse_rhs.parse_pratt_rhs(&mut *txn)?;
      // The report boundary. Read the instant the report crosses back, before this cycle
      // recurses and before any fold runs, because both of those can move the input on a
      // report that moved none — and then a watermark compared at the foot of the cycle reads
      // their progress as the report's and never fires. Only an *accepted* report is held to
      // this: a report the floor declines is not this expression's operator, is rolled back,
      // and legitimately consumes nothing. See `ParsePrattRHS`'s "consume what you report".
      let after_report = txn.span().end();
      match report {
        // The expression ends here: restore whatever the decision consumed and stop. No
        // classify, no fold, no wrap — nothing of this cycle reaches the CST seam.
        PrattRHS::End => {
          txn.rollback();
          break;
        }
        PrattRHS::Postfix(precedenced) => {
          let (operator, op_power) = precedenced.into_components();
          if !min_precedence.admits(&op_power) {
            txn.rollback();
            break;
          }
          // `<=`, not `==`: the watermark cannot regress across a report, so anything not
          // strictly ahead is a stall. Rollback runs before the assertion: an undecided
          // `Commit` guard keeps its progress on drop, unwind included, so a panic raised ahead
          // of the rollback would leave this cycle's effects committed instead of restoring
          // them, which is the one thing this guard exists to prevent.
          if after_report <= committed {
            txn.rollback();
            debug_assert!(
              after_report > committed,
              "pratt: an Infix/Postfix report consumed nothing — the RHS channel must consume \
               what it reports (see ParsePrattRHS)"
            );
            return Err(stalled_rhs_report::<L, Ctx, Lang>(after_report));
          }
          let kind = cst.classify(PrattFoldOp::Postfix(&operator));
          lhs = fold_postfix.fold_postfix(&mut *txn, lhs, Precedenced::new(operator, op_power))?;
          Cst::wrap_at(&mut *txn, cst_mark, kind);
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

          if !min_precedence.admits(lpower) || prev_op_is_neither.as_ref() == Some(lpower) {
            txn.rollback();
            break;
          }

          // The report boundary again, and here it is also the recursion guard: a
          // right-associative report admits its own power, so an inner call handed the same
          // zero-width report re-reports it and descends again — without the check, until the
          // stack runs out. Rollback before the assertion, same reason as the Postfix arm above:
          // a panic ahead of it would leave a `Commit` guard to keep, not restore, this cycle.
          if after_report <= committed {
            txn.rollback();
            debug_assert!(
              after_report > committed,
              "pratt: an Infix/Postfix report consumed nothing — the RHS channel must consume \
               what it reports (see ParsePrattRHS)"
            );
            return Err(stalled_rhs_report::<L, Ctx, Lang>(after_report));
          }

          let next_neither = if matches!(infix.token_ref(), PrattInfix::Neither(_)) {
            Some((*lpower).clone())
          } else {
            None
          };
          let kind = cst.classify(PrattFoldOp::Infix(infix.token_ref()));
          let rhs = parse(
            &mut *txn,
            parse_lhs,
            parse_rhs,
            fold_prefix,
            fold_infix,
            fold_postfix,
            floor,
            cst,
          )?;
          lhs = fold_infix.fold_infix(&mut *txn, lhs, rhs, infix)?;
          Cst::wrap_at(&mut *txn, cst_mark, kind);
          prev_op_is_neither = next_neither;
        }
      }

      // Secondary, and inside the transaction on purpose. Reached only on the fold-continue
      // paths — every exit above leaves the loop first — and the report boundary has already
      // proven *this* cycle's operator was consumed, so what is left for this to catch is the
      // other direction: a fold, or a recursive operand parse, that rewound the input behind
      // the operator it was handed. Same metric, same `<=`.
      //
      // The rollback is the point of the placement. This exit is a grammar bug, and what it
      // must not do is leave the cycle's half-applied effects behind: at the foot of the
      // scope the `Commit` guard has already kept the input position, the emission log and
      // the CST events of a fold that rewound, and no later `Err` takes them back. Deciding
      // here, one statement earlier, is what makes the refusal restore the cycle instead of
      // merely reporting on it.
      let new_committed = txn.span().end();
      // Same metric, same `<=` — and the assertion sits *after* the rollback for the same
      // reason the rollback sits inside the scope: `Commit` keeps on drop whether that drop is
      // an ordinary return or a panic unwinding through it, so a debug build that asserted first
      // would panic past the rollback and keep exactly what release discards.
      if new_committed <= committed {
        txn.rollback();
        debug_assert!(
          new_committed > committed,
          "pratt: a fold or a recursive operand parse rewound the input behind the operator it \
           was handed"
        );
        return Err(stalled_rhs_report::<L, Ctx, Lang>(new_committed));
      }
      new_committed
    };
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
/// Two callers, one condition. At the **report boundary**: an `Infix`/`Postfix` report the
/// floor admitted that consumed nothing, so no phantom fold runs and nothing of the cycle
/// reaches the CST seam. At the **foot of the cycle**, still inside the same scope: a fold, or
/// a recursive operand parse, that rewound behind the operator it was handed.
///
/// Both callers roll the cycle's transaction back before raising, and both must, for the same
/// reason — the guard is `Commit`-policy, so anything left to its drop is kept. The foot check
/// is the one where that is easy to get wrong: run it a statement later, outside the scope, and
/// the input position, the emission log and the CST events of the fold that rewound are already
/// committed by the time the error is built.
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
