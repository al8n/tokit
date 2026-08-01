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
  /// method and `padded`. That promise, and every value promise on this method, is made to
  /// callers whose predicate does not observe the input layer's own side effects; the precondition
  /// is stated in full under *What is guaranteed identical* below, and it is what makes "invisible"
  /// true rather than merely usually true.
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
  /// Every one of those produces the **value** it started from, so — for a predicate that answers
  /// about the token it was handed, which is the precondition spelled out below — the early return
  /// leaves exactly the state the scan would have: the stopping token unconsumed at the front of
  /// the stream (this call never removes it), the committed span and lexer state where they were,
  /// and no diagnostic, no watermark move and no poison latch — a skip that skips nothing emits
  /// nothing on either route. The put-back is an identity on **both** origins, which is why the probe does
  /// not have to exclude a parked head: a cache pop followed by a front push restores the entry it
  /// came from and records no push, and a cache only ever refuses a front push when it is
  /// **full** — so the cache that parked the token in the first place refuses again and the token
  /// re-parks, with no push recorded either.
  ///
  /// The predicate still sees each token **exactly once**, which is a promise a stateful `FnMut`
  /// can check and the cache-transparency matrix does check: a head the probe accepts is not asked
  /// again, its answer is carried into the scan.
  ///
  /// ## What is guaranteed identical — for which callers, and what is not
  ///
  /// Producing the same values is not the same as running the same code, and in a generic library
  /// the difference is not academic: `L::Span::clone`, `L::State::clone`, `L::Offset::clone`,
  /// `Emitter::checkpoint`/`release` and every [`Cache`](crate::cache::Cache) method are all
  /// **caller-supplied**. This route runs fewer of them. Both columns are measured, on the same
  /// stream in the same residency, by the effect ledger in `fast_path_tests`:
  ///
  /// | caller-supplied step, for one no-op skip | this route | the scan it replaces |
  /// |---|---|---|
  /// | `L::Span::clone` | 0 | 1 — **2** under [`Partial`](crate::input::Partial) |
  /// | `L::State::clone` | 0 | 1 — **2** under `Partial` |
  /// | `L::Offset::clone`, taken directly | 0 | 0 — **2** under `Partial` |
  /// | `Emitter::checkpoint`, then `release` | none | none — **one of each** under `Partial` |
  /// | `Cache` | one `front` | one `pop_front`, one `push_front` |
  /// | the predicate | 1 | 1 |
  ///
  /// The offset row counts only the clones the input layer takes itself — the entry capture's
  /// dedup watermark and rewind offset. A caller's own `L::Span::clone` may clone offsets on top
  /// of that, so a span type built from two clonable offsets sees two more per span clone; the
  /// in-tree witness measures 2 under `Complete` and 6 under `Partial` on that shape, against 0
  /// on this route.
  ///
  /// ### The precondition: `pred` must not be able to see any of that
  ///
  /// **Everything guaranteed below is guaranteed to callers whose `pred` does not observe
  /// input-layer side effects** — how many times `L::Span`, `L::State` or `L::Offset` was cloned,
  /// whether an [`Emitter`](crate::Emitter) mark was taken, and which
  /// [`Cache`](crate::cache::Cache) operations ran. That is a real condition and not a formality:
  /// Rust does not require `Clone` to be pure, `L::Span`, `L::State` and `L::Offset` are *your*
  /// types, and `pred` is `FnMut`. A `Clone` that bumps a counter and a predicate that reads it
  /// are both ordinary Rust, and no bound on this method forbids either.
  ///
  /// **Violate it and what differs is not a count — it is the parse.** The table above is the
  /// entire mechanism: the scan clones the frontier pair *before* it asks anything, and this route
  /// asks first and clones nothing, so a predicate keyed on that counter answers one way here and
  /// the other way there. The answer to a skip predicate *is* the skip, so the two routes then
  /// disagree about **which tokens are consumed**, and the resume cursor, the committed span and
  /// lexer state, and everything downstream diverge with them — in both directions: such a caller
  /// can make this route stop where the scan skips the whole stream, and make it consume the head
  /// where the scan consumes nothing. `a_clone_counting_predicate_can_change_the_skip_decision` in
  /// `fast_path_tests` is that caller, written down and measured, so that the exclusion is a fact
  /// rather than a caveat.
  ///
  /// The condition is reasonable, not merely convenient. A predicate that answers differently
  /// depending on **how the input layer got the token to it** is not asking a question about the
  /// input at all — it is asking about this library's internal route, and which route answers a
  /// given call is a choice this crate makes and may change in any release. Ask about the token
  /// and the span you were handed and the condition holds by construction; every predicate in this
  /// crate, its tests and its examples is of that shape. (Recording *that* a call happened is
  /// fine, and is itself guaranteed identical below — it is letting the recording change the
  /// answer that is out of contract.)
  ///
  /// ### For such a caller, guaranteed identical
  ///
  /// Pinned by the residency matrix: the parse result; the tokens read next and the order they
  /// arrive in; the resume [`cursor`](Self::cursor); the committed span and lexer state; the
  /// diagnostics; the poison boundary and the dedup watermark; the tokens the predicate is asked
  /// about, in order; and the emitter's **outstanding-mark count**, which is unchanged by either
  /// route because the cycle the scan runs is empty and balanced.
  ///
  /// ### For such a caller, still not identical
  ///
  /// Stated rather than argued away — these survive the precondition, because none of them is
  /// something `pred` observes:
  ///
  /// * a `Clone` for `L::Span` or `L::State` that **panics** is reachable from the scan and not
  ///   from here — a no-op skip that would have unwound returns `Ok(())` instead;
  /// * an emitter that counts its `checkpoint`/`release` **calls** sees one fewer complete, empty
  ///   cycle per no-op skip under `Partial`. It cannot see a difference in what it *holds*:
  ///   nothing is emitted between the two halves, and [`release`](crate::Emitter::release) is
  ///   documented advisory — correctness must not depend on it being called. Which operations
  ///   take a mark is not part of the [`Emitter`](crate::Emitter) contract;
  /// * a `Cache` that counts its calls sees one `front` where the scan makes a `pop_front` and a
  ///   `push_front`. Both are within the cache contract, which fixes what each operation *means*
  ///   and leaves the choice of operations to the input layer;
  /// * with a head the probe **accepts**, `pred` runs *before* the scanner's entry capture rather
  ///   than after it, so a predicate and an emitter that share state observe the two in the
  ///   opposite order.
  ///
  /// None of that is reachable through this crate's own surface: no span, state, offset, emitter
  /// or cache this crate ships has a `Clone` or a method with an observable side effect, and for a
  /// caller who meets the precondition nothing that can be asked of the input afterwards differs.
  /// The crate also holds itself to what it asks: every `skip_while` predicate it writes — in its
  /// own combinators, its tests, its benches, its examples and its conformance kit — answers out of
  /// the token it was handed, and its adversarial fixtures that *do* count clones read the counter
  /// in an assertion after the call, never inside the predicate. It is all written down because it
  /// is the honest boundary of the claim above.
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
