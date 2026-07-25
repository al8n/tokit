use crate::{
  emitter::PrattEmitter,
  error::{UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot},
  parser::{
    PrattFoldTokenInfix, PrattFoldTokenPostfix, PrattFoldTokenPrefix, PrattInfix, PrattLHS,
    PrattPower, PrattRHS, Precedenced,
  },
  token::PrattToken,
};

use super::*;

impl<'inp, L, Ctx, Lang: ?Sized> InputRef<'inp, '_, L, Ctx, Lang>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
{
  /// Runs a token-level Pratt expression parse over this input.
  ///
  /// This is the low-level, token-centric Pratt API. It requires the token type to implement
  /// [`PrattToken`], which classifies each token as an operand, prefix, infix, or postfix
  /// operator. The fold closures receive raw [`Spanned`] tokens rather than typed AST nodes.
  ///
  /// Equivalent to calling [`pratt_with_min_precedence`](Self::pratt_with_min_precedence) with
  /// `Power::default()` as the minimum binding power.
  ///
  /// For a more ergonomic higher-level API that works with any AST node type, prefer
  /// the [`pratt`](fn@crate::parser::pratt) free function instead.
  ///
  /// # CST-unsupported
  ///
  /// This token-level API folds expressions into **synthetic tokens** — spans covering
  /// already-folded regions with no node-kind seam to classify — so it carries no CST hook
  /// in this version. A parse that should build a syntax tree uses the typed driver and
  /// its [`with_cst_kinds`](crate::parser::Pratt::with_cst_kinds) classifier instead; the
  /// committed tokens this API consumes still auto-flow to a recording sink, but no
  /// expression *nodes* are recorded around them.
  ///
  /// # Parameters
  ///
  /// - `fold_prefix` – called with `(operator_tok, operand_tok, emitter)` when a prefix
  ///   operator and its operand have been successfully parsed.
  /// - `fold_infix` – called with `(lhs_tok, rhs_tok, operator_tok, emitter)` when an infix
  ///   operator and both operands have been parsed.
  /// - `fold_postfix` – called with `(operand_tok, operator_tok, emitter)` when a postfix
  ///   operator has been applied.
  ///
  /// # Returns
  ///
  /// `Ok(Some(tok))` with the combined expression token on success, `Ok(None)` if the
  /// input cursor did not see an LHS token, or `Err(e)` on a fatal emitter error.
  pub fn pratt<FoldPrefix, FoldInfix, FoldPostfix, Expr, Power>(
    &mut self,
    fold_prefix: FoldPrefix,
    fold_infix: FoldInfix,
    fold_postfix: FoldPostfix,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L::Token: PrattToken<'inp, Expr, Power>,
    Ctx::Emitter: PrattEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Power: PrattPower,
    FoldPrefix: PrattFoldTokenPrefix<'inp, Power, L, Ctx, Lang>,
    FoldInfix: PrattFoldTokenInfix<'inp, Power, L, Ctx, Lang>,
    FoldPostfix: PrattFoldTokenPostfix<'inp, Power, L, Ctx, Lang>,
  {
    self.pratt_with_min_precedence(fold_prefix, fold_infix, fold_postfix, Power::default())
  }

  /// Runs a token-level Pratt expression parse over this input starting at a given minimum
  /// binding power.
  ///
  /// This is the low-level, token-centric Pratt API. It requires the token type to implement
  /// [`PrattToken`], which classifies each token as an operand, prefix, infix, or postfix
  /// operator. The fold closures receive raw [`Spanned`] tokens rather than typed AST nodes.
  ///
  /// Only operators whose binding power is **greater than or equal to** `min_precedence` will be
  /// consumed. Operators below the threshold are left in the input for the surrounding
  /// context to handle. This is useful when embedding a Pratt expression inside a larger
  /// grammar — for example, parsing only the right-hand side of an infix operator at a
  /// specific precedence level.
  ///
  /// Use [`pratt`](Self::pratt) instead when you want to parse a full expression starting
  /// from `Power::default()`.
  ///
  /// # Parameters
  ///
  /// - `fold_prefix` – called with `(operator_tok, operand_tok, emitter)` when a prefix
  ///   operator and its operand have been successfully parsed.
  /// - `fold_infix` – called with `(lhs_tok, rhs_tok, operator_tok, emitter)` when an infix
  ///   operator and both operands have been parsed.
  /// - `fold_postfix` – called with `(operand_tok, operator_tok, emitter)` when a postfix
  ///   operator has been applied.
  /// - `min_precedence` – the minimum binding power; operators strictly below this level are not
  ///   consumed.
  ///
  /// # Returns
  ///
  /// `Ok(Some(tok))` with the combined expression token on success, `Ok(None)` if the
  /// input cursor did not see an LHS token, or `Err(e)` on a fatal emitter error.
  pub fn pratt_with_min_precedence<FoldPrefix, FoldInfix, FoldPostfix, Expr, Power>(
    &mut self,
    mut fold_prefix: FoldPrefix,
    mut fold_infix: FoldInfix,
    mut fold_postfix: FoldPostfix,
    min_precedence: Power,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L::Token: PrattToken<'inp, Expr, Power>,
    Ctx::Emitter: PrattEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Power: PrattPower,
    FoldPrefix: PrattFoldTokenPrefix<'inp, Power, L, Ctx, Lang>,
    FoldInfix: PrattFoldTokenInfix<'inp, Power, L, Ctx, Lang>,
    FoldPostfix: PrattFoldTokenPostfix<'inp, Power, L, Ctx, Lang>,
  {
    self.pratt_in(
      min_precedence,
      &mut fold_prefix,
      &mut fold_infix,
      &mut fold_postfix,
    )
  }

  #[inline(always)]
  fn pratt_in<FoldPrefix, FoldInfix, FoldPostfix, Expr, Power>(
    &mut self,
    min_precedence: Power,
    fold_prefix: &mut FoldPrefix,
    fold_infix: &mut FoldInfix,
    fold_postfix: &mut FoldPostfix,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L::Token: PrattToken<'inp, Expr, Power>,
    Ctx::Emitter: PrattEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Power: PrattPower,
    FoldPrefix: PrattFoldTokenPrefix<'inp, Power, L, Ctx, Lang>,
    FoldInfix: PrattFoldTokenInfix<'inp, Power, L, Ctx, Lang>,
    FoldPostfix: PrattFoldTokenPostfix<'inp, Power, L, Ctx, Lang>,
  {
    // A terminal scanner stop at the LHS position is not "no expression here" — surface it
    // instead of declining, so a tripped limit cannot masquerade as an empty expression.
    let Some((lhs, tok)) = self.try_expect_map_or_stop(|tok| tok.try_pratt_lhs())? else {
      return Ok(None);
    };

    let mut lhs = match lhs {
      PrattLHS::Operand(_) => tok,
      PrattLHS::Prefix(precedenced) => {
        let power = precedenced.into_precedence();
        let Some(operand) = self.pratt_in(power, fold_prefix, fold_infix, fold_postfix)? else {
          self
            .session
            .emitter
            .emit_unexpected_end_of_lhs(UnexpectedEoLhs::eolhs_of(self.offset().clone()))?;
          return Ok(Some(tok));
        };

        fold_prefix.fold_prefix(tok, operand, self.emitter())?
      }
    };

    // Step 2: parse rhs -- either an infix/postfix operator or the end of this pratt expression
    let mut prev_op_is_neither: Option<Power> = None;
    while !self.is_eoi() {
      // A terminal scanner stop mid-loop is not "the expression is complete" — surface it
      // rather than breaking, so a tripped limit cannot end the expression early.
      let Some((rhs, tok)) = self.try_expect_map_or_stop(|tok| {
        tok.try_pratt_rhs().and_then(|rhs| match rhs {
          PrattRHS::Postfix(precedenced) => {
            let power = precedenced.into_precedence();
            if power >= min_precedence {
              Some(PrattRHS::Postfix(Precedenced::new((), power)))
            } else {
              None
            }
          }
          PrattRHS::Infix(precedenced) => {
            let (infix, lpower) = precedenced.into_components();
            let rpower = match infix {
              PrattInfix::Left(_) => lpower.next(),
              PrattInfix::Right(_) => lpower.prev(),
              PrattInfix::Neither(_) => lpower.next(),
            };

            if lpower.lt(&min_precedence) || prev_op_is_neither.as_ref() == Some(&lpower) {
              None
            } else {
              Some(PrattRHS::Infix(Precedenced::new(infix, rpower)))
            }
          }
        })
      })?
      else {
        break;
      };

      match rhs {
        PrattRHS::Postfix(_) => lhs = fold_postfix.fold_postfix(lhs, tok, self.emitter())?,
        PrattRHS::Infix(infix) => {
          let (infix, power) = infix.into_components();
          let is_neither = matches!(infix, PrattInfix::Neither(_));
          let lpower = if matches!(infix, PrattInfix::Right(_)) {
            power.next()
          } else {
            power.prev()
          };
          let Some(rhs) = self.pratt_in(power, fold_prefix, fold_infix, fold_postfix)? else {
            self
              .session
              .emitter
              .emit_unexpected_end_of_rhs(UnexpectedEoRhs::eorhs_of(self.offset().clone()))?;
            return Ok(Some(lhs));
          };
          let infix = {
            let (span, tok) = tok.into_components();
            let infix = match infix {
              PrattInfix::Left(_) => PrattInfix::Left(tok),
              PrattInfix::Right(_) => PrattInfix::Right(tok),
              PrattInfix::Neither(_) => PrattInfix::Neither(tok),
            };
            Spanned::new(span, infix)
          };
          lhs = fold_infix.fold_infix(lhs, rhs, infix, self.emitter())?;
          prev_op_is_neither = if is_neither { Some(lpower) } else { None };
        }
      }
    }

    Ok(Some(lhs))
  }
}
