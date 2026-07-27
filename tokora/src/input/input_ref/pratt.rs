use crate::{
  emitter::PrattEmitter,
  error::{UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot},
  parser::{
    PrattFloor, PrattFoldTokenInfix, PrattFoldTokenPostfix, PrattFoldTokenPrefix, PrattInfix,
    PrattLHS, PrattPower, PrattRHS,
  },
  token::PrattToken,
};

use super::*;

/// The token driver's own RHS report, normalized once by the classifying closure.
///
/// The closure has already applied the floor and the non-associative repeat guard, so what
/// crosses back is only what the fold needs — and, for an infix operator, its **true** binding
/// power. Re-wrapping a [`PrattRHS`] here would mean handing the driver a power the closure
/// had already transformed, and reconstructing the original by inverse arithmetic on the far
/// side; that reconstruction was wrong at the ladder's extremes and misfired the repeat guard.
enum TokRhs<Power> {
  Postfix,
  Infix(PrattInfix<(), (), ()>, Power),
}

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
      PrattFloor::Inclusive(min_precedence),
      &mut fold_prefix,
      &mut fold_infix,
      &mut fold_postfix,
    )
  }

  #[inline(always)]
  fn pratt_in<FoldPrefix, FoldInfix, FoldPostfix, Expr, Power>(
    &mut self,
    min_precedence: PrattFloor<Power>,
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
        let floor = PrattFloor::Inclusive(power);
        let Some(operand) = self.pratt_in(floor, fold_prefix, fold_infix, fold_postfix)? else {
          self
            .session
            .emitter
            .emit_unexpected_end_of_lhs(UnexpectedEoLhs::eolhs_of(self.offset().clone()))?;
          return Ok(Some(tok));
        };

        fold_prefix.fold_prefix(tok, operand, self.emitter())?
      }
    };

    // Step 2: parse rhs -- either an infix/postfix operator or the end of this pratt expression.
    //
    // Unconditional: `try_expect_map_or_stop` is already the complete end channel here —
    // `Ok(None)` is genuine exhaustion or a token this expression declines, and a terminal
    // scanner stop is an `Err`. A pre-gate on the scanner's frontier asked a different
    // question and truncated the expression whenever a legal peek had moved that frontier
    // past the last operator this consumer still had to fold.
    //
    // No progress guard, and this is why — the typed driver's hazard has no seam here:
    //
    // * **A report cannot be accepted without consuming.** The report is
    //   [`PrattToken::try_pratt_rhs`], a pure function of one token, and acceptance *is* the
    //   commit: `try_expect_map_or_stop` commits the token exactly when the closure below
    //   answers `Some`, and parks it — `Ok(None)`, which leaves the loop — when it answers
    //   `None`. There is no position at which grammar code can admit an operator and leave
    //   the input where it was.
    // * **No fold can move the input.** The token folds take `Spanned` tokens and the
    //   emitter; none of the three is handed an [`InputRef`], so no fold can advance the
    //   cursor into a stalled report's place, nor rewind behind a committed one.
    // * **Every descent is preceded by a commit.** The recursive call happens only in the
    //   `TokRhs::Infix` arm, after that operator token is committed, and the lexer contract
    //   makes every token nonzero-width — so depth is bounded by the token count.
    let mut prev_op_is_neither: Option<Power> = None;
    loop {
      // A terminal scanner stop mid-loop is not "the expression is complete" — surface it
      // rather than breaking, so a tripped limit cannot end the expression early.
      let Some((rhs, tok)) = self.try_expect_map_or_stop(|tok| {
        tok.try_pratt_rhs().and_then(|rhs| match rhs {
          // A classifier may spell the decline as `End`; here it means exactly what `None`
          // means — the token is not this expression's, and it stays in the stream.
          PrattRHS::End => None,
          PrattRHS::Postfix(precedenced) => {
            let power = precedenced.into_precedence();
            min_precedence.admits(&power).then_some(TokRhs::Postfix)
          }
          PrattRHS::Infix(precedenced) => {
            let (infix, lpower) = precedenced.into_components();
            (min_precedence.admits(&lpower) && prev_op_is_neither.as_ref() != Some(&lpower))
              .then(|| TokRhs::Infix(infix, lpower))
          }
        })
      })?
      else {
        break;
      };

      match rhs {
        TokRhs::Postfix => lhs = fold_postfix.fold_postfix(lhs, tok, self.emitter())?,
        TokRhs::Infix(infix, lpower) => {
          let is_neither = matches!(infix, PrattInfix::Neither(_));
          let floor = if matches!(infix, PrattInfix::Right(_)) {
            // Right-associative: the right operand admits this operator's own power.
            PrattFloor::Inclusive(lpower.clone())
          } else {
            // Left- and non-associative: the right operand stops strictly above it.
            PrattFloor::Exclusive(lpower.clone())
          };
          let Some(rhs) = self.pratt_in(floor, fold_prefix, fold_infix, fold_postfix)? else {
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
