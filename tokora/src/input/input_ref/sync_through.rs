use super::*;

use super::scan::{Scanned, SyncThrough, ThroughEntry};

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  /// Skip tokens until the predicate matches, emitting lexer errors along the way.
  ///
  /// If the predicate matches, the matching token is consumed and returned.
  ///
  /// Diagnostics travel with progress: a match (or a resource-limit trip)
  /// commits the skipped prefix, so the diagnostics describing it persist. A
  /// no-match run to end of input commits nothing — the cursor stays at the
  /// pre-call position — and leaves no trace: the emissions made during the
  /// failed scan are unwound and the lexer-error deduplication watermark is
  /// restored, so a later genuine consume of the same region reports its
  /// errors exactly once.
  ///
  /// This holds even when the caller had prefilled the cache with peeked
  /// lookahead: a failed sync rewinds the drained cache prefix too, restoring
  /// the pre-call position, at the cost of re-lexing those formerly-cached
  /// tokens on the next read.
  ///
  /// # The fatal exit commits, and the cache never changes it
  ///
  /// A fatal emitter rejection mid-skip follows the sync family's fatal-exit discipline: the
  /// token that trips the emitter — a skipped token whose unexpected-token diagnostic the
  /// emitter rejected, or a lexer error it rejected — is **committed**, and the error
  /// propagates. A caller that catches it therefore resumes *after* the reported token, and
  /// never re-reads or re-reports it.
  ///
  /// Whether that token had already been peeked into the cache makes no difference: the cache
  /// is an invisible optimization, so every observable of a sync call — its return, the
  /// committed position and lexer state, the diagnostics it emits, the poison boundary, and
  /// the lexer-error dedup watermark — is a function of the token stream alone, never of how
  /// much of it had been prefetched. (The one thing a caller *can* see is that a peek emits
  /// the lexer errors it crosses when it crosses them, so prefetching moves such a diagnostic
  /// earlier in the log; the dedup watermark still reports it exactly once.) The
  /// `cache_transparency_matrix` tests in `src/input/input_ref/tests.rs` pin this
  /// across the whole family.
  ///
  /// # Partial mode: an `Incomplete` exit leaves no trace
  ///
  /// Under [`Partial`](crate::input::Partial), a non-final buffer can end mid-scan and this
  /// method surfaces `Incomplete`. That exit commits nothing, so it keeps nothing: the position,
  /// the lexer state, the dedup watermark and every emission the aborted attempt made — each
  /// skipped token's `commit_token` event included — are restored to the call's entry. Refill
  /// and call again and the retry is idempotent; nothing accumulates per attempt.
  ///
  /// # Panic unwind
  ///
  /// A panic out of the predicate, the expected-tokens closure, the lexer or the emitter is an
  /// exit too, and it settles — this method's `to`-shaped commit posture keeps the diagnosed
  /// prefix and puts the in-flight token back; the rewinding scans
  /// ([`sync_through`](Self::sync_through), [`sync_balanced`](Self::sync_balanced)) restore to
  /// the call's entry instead. Either way no token leaves the stream and no emitter mark is
  /// stranded.
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub fn sync_through<F, Exp>(
    &mut self,
    mut pred: F,
    mut exp: Exp,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
  {
    trace_event!(self, "sync_through");
    // A no-match run to end of input must leave no trace — even across a prefilled cache. The
    // scanner below skips and diagnoses the cached tokens as readily as the ones it lexes itself,
    // and may cross lexer errors on the way (lifting the dedup watermark). Snapshot the pre-call
    // position (span + lexer state), the emitter's emission mark, and the watermark HERE — BEFORE
    // the scan — so the end-of-input exit can restore the FULL pre-call state, drained cache prefix
    // and all. A match or a limit trip commits the whole diagnosed prefix (that skipping was real
    // progress en route to it), so this snapshot goes unused on those paths; only the no-match
    // end-of-input exit rewinds to it. This is an internal positional rewind, not a `Checkpoint`:
    // it threads no lineage entry.
    //
    // CAPTURE_WINDOW — the watermark clone is hoisted above the mark deliberately. The emitter mark
    // IS a capture: it owes a `rewind` or a `release` on every exit (RELEASE_CENSUS), and only
    // `skip_until`, once it owns `snapshot`, can pay. `L::Offset::clone` is caller-supplied code
    // that may allocate, so evaluating it as a later argument would leave a fallible step BETWEEN
    // the capture and its owner, where an unwind strands one mark-keyed row of the emitter's
    // checkpoint stack with nothing left that knows to reclaim it. Taken first, it can only fail
    // while no mark exists. The rest of the window is infallible: the two clones ahead of the mark
    // are outside it, `ThroughEntry::new` is a `const fn` that only moves, and the `&mut` reborrows
    // between the binding and `skip_until` allocate nothing.
    let error_end = self.emitted_error_end.clone();
    // Same window, same reason: the rewind cursor is caller code too, and it is cloned here so
    // that `restore_entry` — which runs with a mark outstanding — performs none.
    let rewind_to = self.span.end_ref().clone();
    let latch = self.latch_snapshot();
    let snapshot = ThroughEntry::new(
      self.span.clone(),
      self.state.clone(),
      self.session.emitter.checkpoint(),
      error_end,
      rewind_to,
      latch,
    );

    // `SyncThrough` consumes the match (`Scanned::Found`) — the same two lines whether the scanner
    // popped it off the cache or lexed it; a poison trip commits the diagnosed prefix at the
    // durable frontier and a no-match run to end of input rewinds to `snapshot`, both yielding the
    // exhausted outcome (`Ok(None)`) with the position already settled.
    match self.skip_until::<SyncThrough, _, _>(&mut pred, &mut exp, snapshot)? {
      Scanned::Found(tok) => Ok(tok),
      Scanned::Exhausted => Ok(None),
    }
  }

  /// Skip tokens until the predicate matches, emitting lexer errors along the way.
  ///
  /// If the predicate matches, the matching token is consumed and returned with the tokens
  /// peeked after it.
  ///
  /// Diagnostics travel with progress, exactly as in [`sync_through`](Self::sync_through): a
  /// match commits the skipped prefix, so the diagnostics describing it persist. A no-match
  /// run to end of input commits nothing — the cursor stays at the pre-call position — and
  /// leaves no trace: the failed scan's emissions are unwound and the lexer-error
  /// deduplication watermark is restored, so a later genuine consume of the same region
  /// reports its errors exactly once. The returned peek is then empty. As in
  /// [`sync_through`](Self::sync_through), the pre-call position is restored even when the
  /// caller had prefilled the cache with peeked lookahead — the drained cache prefix is
  /// rewound too, at the cost of re-lexing those tokens on the next read. A fatal emitter
  /// rejection mid-skip commits the token that tripped it, and the cache does not change
  /// that either (see [`sync_through`](Self::sync_through)).
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub fn sync_through_then_peek<'p, F, Exp, W>(
    &'p mut self,
    pred: F,
    exp: Exp,
  ) -> Result<
    (Option<Spanned<L::Token, L::Span>>, Peeked<'p, 'inp, L, W>),
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
    W: Window,
  {
    let (tok, peeked, _) = self.sync_through_then_peek_with_emitter::<_, _, W>(pred, exp)?;
    Ok((tok, peeked))
  }

  /// Skip tokens until the predicate matches, emitting lexer errors along the way.
  ///
  /// Returns the matched token, peeked tokens, and a mutable reference to the emitter.
  ///
  /// Diagnostics travel with progress, exactly as in [`sync_through`](Self::sync_through): a
  /// match commits the skipped prefix, so its diagnostics persist. A no-match run to end of
  /// input commits nothing — the cursor stays at the pre-call position — and leaves no trace:
  /// the failed scan's emissions are unwound and the lexer-error deduplication watermark is
  /// restored, so a later genuine consume of the same region reports its errors exactly once.
  /// The returned peek is then empty. As in [`sync_through`](Self::sync_through), the pre-call
  /// position is restored even when the caller had prefilled the cache with peeked lookahead —
  /// the drained cache prefix is rewound too, at the cost of re-lexing those tokens on the
  /// next read — and a fatal emitter rejection mid-skip commits the token that tripped it,
  /// cached or not.
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub fn sync_through_then_peek_with_emitter<'p, F, Exp, W>(
    &'p mut self,
    mut pred: F,
    mut exp: Exp,
  ) -> Result<
    (
      Option<Spanned<L::Token, L::Span>>,
      Peeked<'p, 'inp, L, W>,
      &'p mut Ctx::Emitter,
    ),
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
    W: Window,
  {
    trace_event!(self, "sync_through_then_peek");
    // Snapshot the pre-call state BEFORE the scan, so the no-match end-of-input exit can rewind the
    // FULL pre-call state — the drained cache prefix and the diagnostics alike (see
    // [`sync_through`](Self::sync_through)).
    //
    // CAPTURE_WINDOW — the watermark clone is hoisted above the mark for the reason
    // [`sync_through`](Self::sync_through) gives: a caller-supplied `L::Offset::clone` evaluated
    // after the capture would be a fallible step between the mark and the `skip_until` call that
    // owns (and settles) it, and an unwind there strands one row of the emitter's checkpoint stack.
    let error_end = self.emitted_error_end.clone();
    // Same window, same reason: the rewind cursor is caller code too, and it is cloned here so
    // that `restore_entry` — which runs with a mark outstanding — performs none.
    let rewind_to = self.span.end_ref().clone();
    let latch = self.latch_snapshot();
    let snapshot = ThroughEntry::new(
      self.span.clone(),
      self.state.clone(),
      self.session.emitter.checkpoint(),
      error_end,
      rewind_to,
      latch,
    );

    match self.skip_until::<SyncThrough, _, _>(&mut pred, &mut exp, snapshot)? {
      // The match is consumed, cached or lexed; peek the tokens after it.
      Scanned::Found(tok) => {
        let (peeked, emitter) = self.peek_with_emitter::<W>()?;
        Ok((tok, peeked, emitter))
      }
      // The exhausted outcomes — a poison trip committed at the durable frontier, or a
      // no-match run to end of input rewound to `snapshot` — yield no match and an empty peek.
      Scanned::Exhausted => Ok((None, GenericArrayDeque::new(), self.session.emitter)),
    }
  }
}
