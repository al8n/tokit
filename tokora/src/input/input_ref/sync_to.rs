use super::*;

use super::scan::{Scanned, SyncTo};

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  /// Skip tokens until the predicate matches, emitting lexer errors along the way.
  ///
  /// Advances through the stream, emitting each lexer error via the emitter. Stops
  /// before the first token for which `pred` returns `true` and returns it (without
  /// consuming). Non-matching non-error tokens are skipped but also reported via
  /// `emit_unexpected_token`.
  ///
  /// # The fatal exit commits
  ///
  /// A fatal emitter rejection mid-skip follows the sync family's fatal-exit discipline: the
  /// token that trips the emitter is committed and the error propagates, so a caller that
  /// catches it resumes *after* the reported token. This does not depend on whether the token
  /// was already in the peek cache — the cache is an invisible optimization (see
  /// [`sync_through`](Self::sync_through)).
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
  /// A panic out of the predicate, the expected-tokens closure, the lexer or the emitter
  /// **anywhere but the end-of-input settle** is an exit too, and it settles with **this method's
  /// own posture**, which is the `to`-shaped commit: **every unwind keeps**. This mode holds no
  /// pre-call snapshot — its end of input commits at the lexer's end rather than rewinding to one
  /// — so an unwind has nothing to restore *to*, and the diagnosed prefix simply stands,
  /// committed at the frontier, the end of the last skipped token. Its stop settle stops *before*
  /// the stopping token, which is why the frontier is what an interrupted stop still has to commit.
  /// That is what makes this the one member of the family whose unwind **disposition** needs no
  /// case analysis; the two rewinding scans settle by their own postures, which are not this one,
  /// and each states its own — see [`sync_through`](Self::sync_through) and
  /// [`sync_balanced`](Self::sync_balanced).
  ///
  /// What does vary is the **in-flight token**, and it turns on whether the scan had let go of it
  /// yet — a distinction the scanner's own `TokenSlot` makes, because the two states need
  /// opposite repairs:
  ///
  /// - **held** — the token is out of the stream and not yet recorded, which is true across the
  ///   predicate and nowhere else. A panic there **puts it back at the front of the stream**, so
  ///   the stream is adjacent to the committed position again and nothing re-lexes;
  /// - **handed over** — the token has gone to the stop settle, whose put-back *is* the step the
  ///   panic interrupted. It cannot be put back, because putting it back is what failed. The
  ///   retained stream is **cleared** instead, and the region re-lexes from the committed
  ///   position, which reproduces that token and everything after it; what is lost is the cache's
  ///   memo of it, not a token;
  /// - **recorded** — everywhere else, including the expected-tokens closure and the emitter's
  ///   own report, the skip has already adopted the token behind the frontier (its first act), so
  ///   nothing is out of the stream and the frontier commit covers it.
  ///
  /// So the committed position never advances past a token nothing recorded: no token is lost and
  /// no emitter mark is stranded. Measured on the handed-over case, at the stop settle, with the
  /// [`Cache::push_front`](crate::cache::Cache::push_front) it reaches armed to panic:
  /// `r9_stop_exit_panic_still_commits_the_diagnosed_prefix` (`src/input/input_ref/tests.rs`)
  /// reads a committed `(3, 5)` with both skip diagnostics standing, and the catching host's
  /// retry resumes past them — re-lexing the swallowed stopper rather than re-diagnosing the
  /// prefix.
  ///
  /// The exclusion is the one [`skip_while`](Self::skip_while) states, and it is the same settle:
  /// a committing scan accumulates the skipped run in an uncommitted frontier and its scope is
  /// disarmed before the end-of-input settle runs, so an unwind *inside* that settle drops the
  /// frontier and the position stays at the call's entry — the diagnosed prefix is **not** kept
  /// there, only the diagnostics already emitted for it are. Measured with
  /// [`Lexer::into_state`](crate::Lexer::into_state) armed to panic over `"ab cd ef gh"`:
  /// `eof_commit_interrupted(true)` in `r9_committing_eof_commit_is_atomic_in_span_and_state`
  /// (`src/input/input_ref/tests.rs`) reads `((0, 0), 0)`, the entry position beside the entry
  /// state. That cell drives the scanner at [`skip_while`](Self::skip_while)'s mode, whose
  /// end-of-input settle **is** this one's, verbatim.
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub fn sync_to<F, Exp>(
    &mut self,
    pred: F,
    exp: Exp,
  ) -> Result<
    Option<MaybeRefCachedTokenOf<'_, 'inp, L, L::Token>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
  {
    self
      .sync_to_then_peek_with_emitter::<_, _, U1>(pred, exp)
      .map(|(mut out, _)| out.pop_front())
  }

  /// Skip tokens until the predicate matches, emitting lexer errors along the way.
  ///
  /// Returns peeked tokens and a mutable reference to the emitter. A fatal emitter rejection
  /// mid-skip commits the token that tripped it, exactly as in [`sync_to`](Self::sync_to).
  #[inline(always)]
  #[allow(clippy::type_complexity)]
  pub fn sync_to_then_peek_with_emitter<'p, F, Exp, W>(
    &'p mut self,
    mut pred: F,
    mut exp: Exp,
  ) -> Result<
    (Peeked<'p, 'inp, L, W>, &'p mut Ctx::Emitter),
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    Exp: FnMut() -> Option<Expected<'inp, <L::Token as Token<'inp>>::Kind>>,
    W: Window,
  {
    trace_event!(self, "sync_to");
    // `SyncTo` leaves the match unconsumed at the cache FRONT — whether the scanner popped it out
    // of the cache or lexed it — so the peek that follows always serves it straight back out of the
    // cache and fills the rest of the window behind it. The exhausted outcomes — a poison trip
    // mid-scan or a no-match run to end of input, both of which `skip_until` has already settled —
    // return the empty peek.
    match self.skip_until::<SyncTo, _, _>(&mut pred, &mut exp, ())? {
      Scanned::Found(_) => self.peek_with_emitter::<W>(),
      Scanned::Exhausted => Ok((GenericArrayDeque::new(), self.session.emitter)),
    }
  }
}
