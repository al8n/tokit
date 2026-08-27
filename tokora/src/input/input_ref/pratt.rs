use crate::{
  emitter::PrattEmitter,
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs, UnexpectedEot,
  },
  parser::{
    PrattFloor, PrattFoldTokenInfix, PrattFoldTokenPostfix, PrattFoldTokenPrefix, PrattInfix,
    PrattLHS, PrattPower, PrattRHS,
  },
  token::PrattToken,
};

use super::*;

/// The token driver's own RHS report, normalized once by the classifying closure.
///
/// The closure has already applied the floor and detected the non-associative repeat, so what
/// crosses back is only what the fold needs — and, for an infix operator, its **true** binding
/// power. Re-wrapping a [`PrattRHS`] here would mean handing the driver a power the closure
/// had already transformed, and reconstructing the original by inverse arithmetic on the far
/// side; that reconstruction was wrong at the ladder's extremes and misfired the repeat guard.
///
/// A repeat is *not* one of these variants, and neither is a zero-token continuation: the closure
/// parks its token and records why in a [`Parked`], and the loop raises the matching error instead
/// of stepping.
enum TokRhs<Power> {
  Postfix,
  Infix(PrattInfix<(), (), ()>, Power),
}

/// Why the classifying closure parked its token — the far side of a `None`, which on its own
/// cannot tell three different endings apart.
///
/// A fieldless enum rather than a pair of `bool`s, so "at most one of these holds" is a property
/// of the type instead of a sentence about the closure. No offset rides in it; the offset is read
/// on the far side, off the input itself, for the reason spelled out at the raise below.
enum Parked {
  /// The token is not this expression's operator, or is below its floor. The ordinary handback.
  Decline,
  /// The same power as the [`Neither`](PrattInfix::Neither) operator this frame just folded.
  NonAssoc,
  /// A [`PrattRHS::Adjacent`] report the floor admitted and the chain constraint allowed — a
  /// zero-token infix continuation, which this engine cannot express.
  ///
  /// **Admitted and non-repeating, checked in that order before this variant is reached.** An
  /// adjacency below the floor is the surrounding grammar's and gets [`Decline`](Self::Decline);
  /// one repeating a [`Neither`](PrattInfix::Neither) fold at the same power gets
  /// [`NonAssoc`](Self::NonAssoc). Only what is left over is the report this engine has no answer
  /// for, and only that is worth a diagnostic of its own.
  ///
  /// **Diagnosed rather than declined, and that is the whole of the reason it needs a variant.**
  /// This driver's termination argument is that acceptance *is* the commit of exactly one token of
  /// nonzero width, so every descent is preceded by a commit and depth is bounded by the token
  /// count; and its infix fold is handed `Spanned<PrattInfix<L::Token, …>, L::Span>`, a real token
  /// there is no zero-token spelling of. Treating the report as an ending instead would truncate
  /// the expression with an `Ok` and no diagnostic on any channel — the exact failure
  /// `PrattRHS::End`'s own documentation refuses for a below-floor sentinel. The typed driver
  /// ([`Pratt`](crate::parser::Pratt)) is where this shape is expressible.
  Adjacent,
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
  ///
  /// # Two failures that are not the emitter's
  ///
  /// Both are **returned**, never emitted, so no recording emitter and no rewind can turn either
  /// into a truncated-but-successful parse:
  ///
  /// - [`RecursionLimitReached`] — the parse's shared depth budget (see
  ///   [`descend`](Self::descend)) ran out at this frame's prologue. Terminal, and terminal
  ///   independently of the grammar's error type: the trip latches the input session, so a grammar
  ///   whose error discards the value on conversion (`()` included) loses the payload and still
  ///   gets the re-raise — see [`RecursionLimitReached`]'s own docs for what a discarding sink
  ///   costs and does not cost.
  /// - [`NonAssociativeChain`] — a second same-power [`PrattInfix::Neither`] operator appeared in
  ///   one chain. The operator is left on the input, unconsumed. Not terminal: recovery may
  ///   spend it.
  pub fn pratt<FoldPrefix, FoldInfix, FoldPostfix, Expr, Power>(
    &mut self,
    fold_prefix: FoldPrefix,
    fold_infix: FoldInfix,
    fold_postfix: FoldPostfix,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L::Token: PrattToken<'inp, Expr, Power>,
    Ctx::Emitter: PrattEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<RecursionLimitReached<L::Offset, Lang>>
      + From<NonAssociativeChain<L::Offset, Lang>>,
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
  ///
  /// Plus the two returned-not-emitted failures [`pratt`](Self::pratt) documents:
  /// [`RecursionLimitReached`] and [`NonAssociativeChain`].
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
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<RecursionLimitReached<L::Offset, Lang>>
      + From<NonAssociativeChain<L::Offset, Lang>>,
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
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<RecursionLimitReached<L::Offset, Lang>>
      + From<NonAssociativeChain<L::Offset, Lang>>,
    Power: PrattPower,
    FoldPrefix: PrattFoldTokenPrefix<'inp, Power, L, Ctx, Lang>,
    FoldInfix: PrattFoldTokenInfix<'inp, Power, L, Ctx, Lang>,
    FoldPostfix: PrattFoldTokenPostfix<'inp, Power, L, Ctx, Lang>,
  {
    // ONE FRAME, ONE LEVEL. Taken before the LHS read, so every recursion site below — the
    // prefix operand and the infix right operand alike — counts through its own child frame's
    // prologue and needs no bookkeeping of its own. Depth therefore equals the native pratt depth
    // it protects, this root frame included, and a future third recursion site inherits the bound
    // for free. The guard releases the level on every exit of this function, unwind included.
    //
    // Nothing of the frame exists yet when the trip raises: no read, no fold, no diagnostic — so
    // there is nothing to restore beyond the depth cell, which `descend` restores itself.
    let mut frame = self.descend()?;
    let this = &mut *frame;

    // …AND ONE FRAME, ONE STACK CHECK, on the same line and for the same frame. Composed with
    // the level above, never substituted for it: `descend` is what REFUSES a too-deep input,
    // and this is only what decides which stack the frame it just admitted runs on. With the
    // `stacker` feature off it is the identity and this closure is the frame body verbatim.
    crate::native_stack::maybe_grow(move || {
      // A terminal scanner stop at the LHS position is not "no expression here" — surface it
      // instead of declining, so a tripped limit cannot masquerade as an empty expression.
      let Some((lhs, tok)) = this.try_expect_map_or_stop(|tok| tok.try_pratt_lhs())? else {
        return Ok(None);
      };

      let mut lhs = match lhs {
        PrattLHS::Operand(_) => tok,
        PrattLHS::Prefix(precedenced) => {
          let power = precedenced.into_precedence();
          let floor = PrattFloor::Inclusive(power);
          let Some(operand) = this.pratt_in(floor, fold_prefix, fold_infix, fold_postfix)? else {
            // Recovery posture: a prefix operator whose operand never arrived is returned as the
            // expression's own token, after the diagnostic. Under a recording emitter the parse
            // therefore continues with the bare operator standing in for the expression it could
            // not build. Callers wanting the stricter posture — no value where no operand was
            // parsed — use the typed driver, which has a node to withhold.
            this
              .session
              .emitter
              .emit_unexpected_end_of_lhs(UnexpectedEoLhs::eolhs_of(this.offset().clone()))?;
            return Ok(Some(tok));
          };

          fold_prefix.fold_prefix(tok, operand, this.emitter_view())?
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
      //
      //   That is also why this engine has no ordering question to answer between a stalled report
      //   and the non-associative repeat, which the typed driver does have and had wrong: there is
      //   no stall guard here because there is no report the guard could catch. A closure that
      //   answers `Some` has committed its token, so the two conditions the typed driver must rank
      //   — "this report consumed nothing" and "this operator repeats the chain" — cannot both hold
      //   at once, and the first of them cannot hold at all. Restoring a stall guard here would be
      //   dead code; the invariant that makes it dead is the three bullets around this one.
      // * **No fold can move the input.** The token folds take `Spanned` tokens and the
      //   emitter's operations; none of the three is handed an [`InputRef`], so no fold can advance the
      //   cursor into a stalled report's place, nor rewind behind a committed one.
      // * **Every descent is preceded by a commit.** The recursive call happens only in the
      //   `TokRhs::Infix` arm, after that operator token is committed, and the lexer contract
      //   makes every token nonzero-width — so depth is bounded by the token count. Bounded by
      //   the token count is not bounded by anything a *machine* has; the frame prologue's
      //   `descend` is what bounds it by the configured budget.
      //
      //   That bullet is why `PrattRHS::Adjacent` is REFUSED here rather than served. A zero-token
      //   continuation is a descent with no commit in front of it, which is the one thing this
      //   engine's termination argument does not survive — and it has no token for `fold_infix`
      //   either. The typed driver carries the shape, and pays for it with a charge on the
      //   operand that this engine has no fold-side place to put.
      let mut prev_op_is_neither: Option<Power> = None;
      loop {
        // Why the classifying closure parked, recorded out of it rather than answered inside it.
        // All three endings must PARK the token — a decline commits nothing, and both refusals are
        // owed the same handback the `End` arm gets — so all three answer `None`; this is what
        // tells them apart on the far side. Re-declared per cycle, and written at most once per
        // `try_expect_map_or_stop` call: the closure runs against one token.
        let mut parked = Parked::Decline;
        // A terminal scanner stop mid-loop is not "the expression is complete" — surface it
        // rather than breaking, so a tripped limit cannot end the expression early.
        let step = this.try_expect_map_or_stop(|tok| {
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
              // Below the floor: the operator belongs to an enclosing expression. Park it and end
              // this one — an ordinary handback, and not an error.
              if !min_precedence.admits(&lpower) {
                return None;
              }
              // The same power as the `Neither` operator this frame just folded: a
              // declared-non-associative chain. Park the token and flag it, so the loop below
              // raises the diagnostic instead of quietly stopping.
              if prev_op_is_neither.as_ref() == Some(&lpower) {
                parked = Parked::NonAssoc;
                return None;
              }
              Some(TokRhs::Infix(infix, lpower))
            }
            // THE SAME TWO TESTS THE `Infix` ARM RUNS, AND IN THE SAME ORDER — this engine has
            // no third, because it has no report boundary to rank: acceptance here IS the commit
            // of a token. Both run before the report reaches the unsupported path at all.
            //
            // Parking unconditionally
            // reads the payload as "adjacency" and never as "adjacency *at a power*", which
            // makes two answers wrong: an adjacency BELOW the floor is the surrounding grammar's
            // operator and is owed the ordinary handback, not a refusal that ends this
            // expression with a diagnostic it did not earn; and an adjacency at the power of the
            // `Neither` operator this frame just folded is a declared-non-associative chain,
            // which is `NonAssociativeChain` here exactly as it is for a spelled operator. Both
            // rejected an otherwise valid embedded parse while leaving the token unconsumed.
            //
            // Only an ADMITTED, NON-REPEATING adjacency is the shape this engine cannot serve,
            // and only that one is refused — see `Parked::Adjacent`.
            PrattRHS::Adjacent(precedenced) => {
              let power = precedenced.into_precedence();
              if !min_precedence.admits(&power) {
                return None;
              }
              if prev_op_is_neither.as_ref() == Some(&power) {
                parked = Parked::NonAssoc;
                return None;
              }
              parked = Parked::Adjacent;
              None
            }
          })
        })?;

        // The `?` above fires FIRST, so a terminal scanner stop still outranks the repeat: the
        // ranking law is unchanged. Only once the scanner has answered does the flag decide which
        // of the two "no step" endings this is.
        //
        // THE OFFSET IS READ HERE, AFTER THE HANDBACK, and off the input rather than off the token.
        // `NonAssociativeChain`'s offset is *defined* as the position the input was handed back at,
        // and `self.span().end()` is that position by identity: `try_expect_map_or_stop` calls
        // `commit_token` only when the closure accepts, so a declined token is parked and the
        // committed span is still the one this cycle started from. That is the same quantity the
        // typed driver's probe rollback restores, and the same one `try_expect_map_or_stop` builds
        // its own terminal `UnexpectedEot` from when it stops for a scanner reason instead.
        //
        // Reading the PARKED TOKEN's start instead would be the operator's own head, and it is a
        // different number whenever the lexer skipped anything to reach it — `1 ; 2 ; 3` parks the
        // second `;` at 6 with the committed frontier at 5. That is the same over-reach the typed
        // driver kept re-deriving from a position adjacent to its restore target, and reproducing it
        // here would also split the two engines' answers on the one input shape they can both parse.
        // The park is unchanged; only what is reported about it is.
        let Some((rhs, tok)) = step else {
          match parked {
            Parked::Decline => break,
            Parked::NonAssoc => {
              return Err(NonAssociativeChain::of(this.span().end()).into());
            }
            // The same posture as the missing-right-operand exit below, and for the same reason:
            // this engine's answer to "an infix operator this cycle cannot complete" is a
            // diagnostic on the RHS channel plus the left-hand side alone, not a failed parse. A
            // fail-fast emitter turns the `?` into the `Err`; a collecting one records it and the
            // surrounding grammar carries on from a position it can see. What is refused either
            // way is the silent ending — parking the token and `break`ing would truncate the
            // expression with an `Ok` and no diagnostic on any channel, which is exactly the
            // failure `PrattRHS::End`'s own documentation refuses for a below-floor sentinel.
            //
            // No new bound buys this. `From<UnexpectedEoRhs<…>>` is deliberately absent from this
            // function's `where` clause, and adding it to reach `Err` directly would widen the
            // public `InputRef::pratt` surface for a shape this engine declines to serve.
            //
            // The offset is the handback position, read the same way and for the same reason as
            // the repeat's above.
            Parked::Adjacent => {
              this
                .session
                .emitter
                .emit_unexpected_end_of_rhs(UnexpectedEoRhs::eorhs_of(this.span().end()))?;
              return Ok(Some(lhs));
            }
          }
        };

        match rhs {
          TokRhs::Postfix => lhs = fold_postfix.fold_postfix(lhs, tok, this.emitter_view())?,
          TokRhs::Infix(infix, lpower) => {
            let is_neither = matches!(infix, PrattInfix::Neither(_));
            let floor = if matches!(infix, PrattInfix::Right(_)) {
              // Right-associative: the right operand admits this operator's own power.
              PrattFloor::Inclusive(lpower.clone())
            } else {
              // Left- and non-associative: the right operand stops strictly above it.
              PrattFloor::Exclusive(lpower.clone())
            };
            let Some(rhs) = this.pratt_in(floor, fold_prefix, fold_infix, fold_postfix)? else {
              // Same recovery posture as the prefix exit above: an infix operator whose right
              // operand never arrived yields the LHS alone, after the diagnostic — the operator
              // and its missing side are dropped from the returned expression rather than the
              // whole parse failing. The typed driver is the stricter alternative.
              this
                .session
                .emitter
                .emit_unexpected_end_of_rhs(UnexpectedEoRhs::eorhs_of(this.offset().clone()))?;
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
            lhs = fold_infix.fold_infix(lhs, rhs, infix, this.emitter_view())?;
            prev_op_is_neither = if is_neither { Some(lpower) } else { None };
          }
        }
      }

      Ok(Some(lhs))
    })
  }
}
