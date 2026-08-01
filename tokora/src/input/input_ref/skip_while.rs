use super::*;

use super::scan::SkipWhile;

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  /// Consumes consecutive tokens matching `pred` without reporting them.
  ///
  /// Advances the cursor past every leading token for which `pred` returns
  /// `true`, stopping before the first token for which it returns `false` (that
  /// token is left unconsumed) or at end of input.
  ///
  /// Unlike [`sync_to`](Self::sync_to), the skipped tokens are **not** reported
  /// through `emit_unexpected_token`: they are expected and simply dropped.
  /// Genuine lexer errors encountered while skipping are still emitted, so a
  /// fatal emitter can abort on a malformed token. Already-cached (peeked)
  /// tokens are drained identically to freshly-lexed ones.
  ///
  /// This is the primitive used to skip trivia (whitespace, comments) in the
  /// `padded`, `padded_left`, and `padded_right` combinators, where trivia must
  /// be consumed but must never surface as an error.
  ///
  /// # The token that stops the skip is left at the cache front
  ///
  /// A token this call examined but did not consume is **unconsumed**: it is put back at the front
  /// of the peek cache — where [`try_expect`](Self::try_expect) puts the token its predicate
  /// declined, and the one place [`cursor`](Self::cursor) reads. So the resume cursor after a skip
  /// is the stopping token's start, whether that token had been peeked into the cache beforehand or
  /// this call lexed it a moment ago, and the next read serves it without re-lexing. The cache is
  /// an invisible optimization here as everywhere: nothing a caller can observe about a
  /// `skip_while` — the committed span and lexer state, the cursor, the diagnostics, the poison
  /// boundary, the dedup watermark, the tokens read next — depends on how deep it had peeked. The
  /// `cache_transparency_matrix` tests in `src/input/input_ref/tests.rs` pin that across this
  /// method and `padded`.
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
  ///
  /// # A skip that skips nothing never enters the scanner
  ///
  /// The grammar shape this primitive exists for — skip trivia, then look — asks for a skip at
  /// every decision point, and most of those decision points have nothing to skip: the head is
  /// already at the front of the stream and the predicate rejects it on sight. Measured on a
  /// GraphQL parse, that is 22,033 of 39,057 calls.
  ///
  /// So the head is asked **where it lies**, before anything is built. A rejection returns
  /// immediately, and the general scan is what it would have run to reach the same state:
  ///
  /// | the general path does | and it lands on |
  /// |---|---|
  /// | build the scan scope and clone the frontier pair (`L::Span` + `L::State`) | values it never advances — nothing is skipped |
  /// | take the head out of the stream | …to put the very same token straight back |
  /// | run the predicate | the same answer, on the same token |
  /// | commit the untouched frontier | the span and state already committed |
  ///
  /// Every one of those is the identity here, so the early return leaves exactly the state the
  /// scan would have: the stopping token unconsumed at the front of the stream (this call never
  /// removes it), the committed span and lexer state where they were, and no diagnostic, no
  /// watermark move and no poison latch — a skip that skips nothing emits nothing on either
  /// route. The put-back is an identity on **both** origins, which is why the probe does not have
  /// to exclude a parked head: a cache pop followed by a front push restores the entry it came
  /// from and records no push, and a cache only ever refuses a front push when it is **full** —
  /// so the cache that parked the token in the first place refuses again and the token re-parks,
  /// with no push recorded either.
  ///
  /// The predicate still sees each token **exactly once**, which is a promise a stateful `FnMut`
  /// can check and the cache-transparency matrix does check: a head the probe accepts is not asked
  /// again, its answer is carried into the scan.
  #[inline(always)]
  pub fn skip_while<F>(
    &mut self,
    mut pred: F,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    // ── The no-op skip, answered where the token already is (see the section above) ──
    // `false` unless the probe below both ran and accepted; then the scan's first token is the
    // head this already asked about, and its answer is carried in rather than asked twice.
    let mut head_answered_skip = false;
    if let Some(front) = self.front() {
      if !pred(front.token) {
        return Ok(());
      }
      head_answered_skip = true;
    }

    // A trivia skip and a recovery sync are the same scan: take each token from the cache while one
    // is there and from the lexer once it is not, settle every skipped token behind the frontier,
    // and stop on the first token the predicate picks out — leaving it unconsumed at the cache
    // front. They differ only in the mode ([`SkipWhile`]: report nothing, commit at end of input)
    // and in the POLARITY of the predicate, which is this one negation: a sync stops on the token
    // it matches, a skip stops on the first token it does not. Sharing the loop is what keeps the
    // hot trivia path and the cold recovery path from drifting apart — the defect they twice did.
    self
      .skip_until::<SkipWhile, _, _>(
        |t| {
          // ONE predicate call per token, still. The scan's first fetch is the very head the
          // probe accepted — nothing runs in between that could change the front of the stream —
          // so its answer is replayed here instead of asking `pred` a second time about a token
          // it has already judged. A stateful predicate would otherwise be able to tell that it
          // was driven through this method rather than through the scanner, and could answer
          // differently the second time.
          if head_answered_skip {
            head_answered_skip = false;
            return false;
          }
          !pred(t)
        },
        || None,
        (),
      )
      .map(|_| ())
  }
}
