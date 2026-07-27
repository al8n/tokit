use core::marker::PhantomData;

use crate::Commit;

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
      self.min_precedence.clone(),
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
  min_precedence: Power,
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
{
  // The driver-held mark: minted before anything of this expression is parsed, spent once
  // per fold below. Each recursive operand parse takes its own mark, and same-target wraps
  // materialize inside-out, so nesting follows fold order for free. `None` (and every
  // `wrap_at`/`classify` below a no-op) when no CST kinds are configured.
  let cst_mark = Cst::mark(inp);

  // Step 1: parse lhs -- either a prefix operator or an operand
  let mut lhs = match parse_lhs.parse_pratt_lhs(inp)? {
    PrattLHS::Operand(o) => o,
    PrattLHS::Prefix(precedenced) => {
      let (operator, power) = precedenced.into_components();
      let operand = parse(
        inp,
        parse_lhs,
        parse_rhs,
        fold_prefix,
        fold_infix,
        fold_postfix,
        power.clone(),
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

  // Step 2: parse rhs -- either an infix/postfix operator or the end of this pratt expression
  let mut prev_op_is_neither: Option<Power> = None;
  while !inp.is_eoi() {
    // This loop is commit-by-default: it keeps whatever it parsed on every success and on
    // every `?`-propagation (a fail-fast emitter error carries the consumed progress out,
    // exactly as dropping a raw checkpoint used to), and rolls back only on the two exits
    // where the next operator is not part of this expression. A `Commit`-policy guard is
    // the structural match: parse through it, let the drop keep progress, and roll back
    // explicitly on those two exits.
    let mut txn = inp.begin_with::<Commit>();
    match parse_rhs.parse_pratt_rhs(&mut *txn)? {
      PrattRHS::Postfix(precedenced) => {
        let (operator, op_power) = precedenced.into_components();
        if op_power >= min_precedence {
          let kind = cst.classify(PrattFoldOp::Postfix(&operator));
          lhs = fold_postfix.fold_postfix(&mut *txn, lhs, Precedenced::new(operator, op_power))?;
          Cst::wrap_at(&mut *txn, cst_mark, kind);
        } else {
          txn.rollback();
          break;
        }
      }
      PrattRHS::Infix(infix) => {
        let lpower = infix.precedence();
        let rpower = match infix.token_ref() {
          PrattInfix::Left(_) => lpower.next(),
          PrattInfix::Right(_) => lpower.prev(),
          PrattInfix::Neither(_) => lpower.next(),
        };

        if lpower.lt(&min_precedence) || prev_op_is_neither.as_ref() == Some(lpower) {
          txn.rollback();
          break;
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
          rpower,
          cst,
        )?;
        lhs = fold_infix.fold_infix(&mut *txn, lhs, rhs, infix)?;
        Cst::wrap_at(&mut *txn, cst_mark, kind);
        prev_op_is_neither = next_neither;
      }
    }
  }

  Ok(lhs)
}
