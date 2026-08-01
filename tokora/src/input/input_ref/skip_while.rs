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
  /// method and `padded`. That promise, and every value promise on this method, is made to callers
  /// who meet one two-clause condition — input-layer callbacks that are **inert**, and a predicate
  /// that is a **function of what it is handed**. It is stated in full under *What is guaranteed
  /// identical* below, and it is what makes "invisible" true rather than merely usually true.
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
  /// Every one of those produces the **value** it started from, so — for a caller who meets the
  /// condition spelled out below — the early return
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
  /// ## What is guaranteed identical, and the condition that makes it so
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
  /// ### What this route does differently — the whole of it
  ///
  /// Four clauses, meant as exhaustive and not as illustration. Against the scan it replaces, for
  /// every call it answers, this route:
  ///
  /// 1. **omits** caller-supplied steps — the table above is the list — and never adds one;
  /// 2. **substitutes** a single [`Cache::front`](crate::cache::Cache::front) for the
  ///    [`pop_front`](crate::cache::Cache::pop_front) +
  ///    [`push_front`](crate::cache::Cache::push_front) pair the scan uses to look at the same
  ///    head. The cache laws make that pair an identity on the entry it came from, so the two are
  ///    the same read of the same token;
  /// 3. **reorders**: with a head the probe accepts, `pred` runs *before* the omitted steps of (1)
  ///    rather than after them;
  /// 4. hands `pred` the **same token and the same span**, once per token, in the same order —
  ///    which the residency matrix pins directly.
  ///
  /// A step this route does not take is also a value it does not produce, and therefore one it
  /// does not drop: the scan runs an `L::Span::drop` and an `L::State::drop` on frontier clones
  /// that this route never creates.
  ///
  /// ### The condition on the caller
  ///
  /// Everything guaranteed below holds for a caller who meets **both** of these. They are
  /// properties you check *once*, about your own types — not a list of differences to keep up
  /// with:
  ///
  /// * **your input-layer callbacks are inert.** Every caller-supplied item this crate may invoke
  ///   on the way to an answer — the `Clone` and `Drop` of `L::Span`, `L::State` and `L::Offset`;
  ///   [`Emitter::checkpoint`](crate::Emitter::checkpoint) and
  ///   [`release`](crate::Emitter::release); every [`Cache`](crate::cache::Cache) method; the
  ///   [`Lexer`](crate::Lexer) and its [`Source`](crate::Source) — does **only** what its own
  ///   contract says it does, and always returns normally: it does not unwind, and it does not
  ///   diverge. Then running one, running it twice, and not running it at all are the same thing
  ///   to everyone;
  /// * **`pred` is a function of what it is handed.** It answers from the *values* of the
  ///   `Spanned<&L::Token, &L::Span>` it receives — not from state some other callback wrote, and
  ///   not from the addresses those two references carry (the scan asks about a token it has moved
  ///   out of the cache; this route asks about it where it lies). Recording *that* a call
  ///   happened, in your own state, and reading the record after the call returns, is fine: the
  ///   call sequence is itself guaranteed identical below.
  ///
  /// **Why those two are the whole condition.** Any difference between the routes has to be
  /// produced by something. Clauses (1)–(4) say the crate's own contribution is the same values in
  /// the same order, so the only remaining producer is caller-supplied code; the first clause
  /// covers *all* of that code and makes omitting it, running it a different number of times, and
  /// running it in a different order produce nothing; the second says your own predicate cannot
  /// read which route produced its argument. Nothing is left over. That closure is the point of
  /// stating a condition rather than listing exclusions — a list has to anticipate every way a
  /// caller might differ, and each way found so far (a `Clone` that keeps state, a `Clone` that
  /// panics, a `Drop` elided with its clone) is an instance of one clause rather than a new entry.
  ///
  /// Two things the condition does not cover, because no fast path could: **time and stack**. This
  /// route is quicker and shallower, which is the whole reason it exists.
  ///
  /// ### For such a caller, guaranteed identical
  ///
  /// Pinned by the residency matrix: the parse result; the tokens read next and the order they
  /// arrive in; the resume [`cursor`](Self::cursor); the committed span and lexer state; the
  /// diagnostics; the poison boundary and the dedup watermark; the tokens the predicate is asked
  /// about, in order, and how many times it is asked; and the emitter's **outstanding-mark
  /// count**, which is unchanged by either route because the cycle the scan runs is empty and
  /// balanced.
  ///
  /// ### And for a caller who does not meet it
  ///
  /// Not a second list to maintain: each of these is one of the two clauses failing, and each is
  /// measured in `fast_path_tests`, so the condition is a fact rather than a caveat.
  ///
  /// * **a `Clone` that keeps state, and a `pred` that reads it** — the second clause. The table
  ///   above is the entire mechanism: the scan clones the frontier pair before it asks, this route
  ///   asks first and clones nothing, so a predicate keyed on that counter answers one way here
  ///   and the other way there. The answer to a skip predicate *is* the skip, so what differs is
  ///   not a count but the parse — the committed cursor, the tokens consumed, everything
  ///   downstream — and it differs in both directions: such a caller can make this route stop
  ///   where the scan skips the whole stream, and consume the head where the scan consumes
  ///   nothing. (`a_clone_counting_predicate_can_change_the_skip_decision`)
  /// * **a `Clone` that unwinds** — the first clause, and the sharper case, because the caller
  ///   here satisfies the second one completely. A `pred` that records nothing but its **own**
  ///   calls observes no input-layer side effect at all, and still cannot be promised the same
  ///   call sequence: with a head this route accepts, `pred` runs once and *then* the scan's
  ///   `L::Span::clone` panics, where the scan panics before asking anything — one call against
  ///   none, measured. Catch the unwind and the two routes have left your predicate in different
  ///   states. And a no-op skip that would have unwound returns `Ok(())` instead.
  ///   (`an_unwinding_caller_clone_leaves_the_predicate_with_a_different_call_count`,
  ///   `a_no_op_skip_over_a_resident_head_reaches_no_panicking_caller_clone`)
  /// * **an emitter or a cache that counts** — the first clause, with the second deciding whether
  ///   it matters. Under `Partial` a mark-keyed emitter sees one fewer complete, empty
  ///   `checkpoint`/`release` cycle per no-op skip, and a counting cache sees one `front` where
  ///   the scan makes a `pop_front` and a `push_front`. Neither can see a difference in what it
  ///   *holds*: the cycle is empty and balanced and [`release`](crate::Emitter::release) is
  ///   documented advisory, and the pop/push pair leaves the cache with the contents `front` read.
  ///   The count becomes a parse only once it reaches `pred`.
  /// * **a `Drop` that is not inert** — the first clause again, and the one a list of clone counts
  ///   would have missed: a frontier clone this route never takes is a value it never drops.
  ///
  /// The condition is reasonable, not merely convenient. A predicate that answers differently
  /// depending on **how the input layer got the token to it** is not asking a question about the
  /// input at all — it is asking about this library's internal route, and which route answers a
  /// given call is a choice this crate makes and may change in any release. A callback that
  /// unwinds asks the same question in control flow: it turns *which steps ran* into an outcome,
  /// and which steps ran is not part of the contract either.
  ///
  /// The crate meets its own condition and holds itself to it. Every span, state and offset it
  /// ships clones by copying fields, with no side effect and no panic path — surveyed impl by
  /// impl — so the first clause holds for every in-tree lexer by inspection. Every `skip_while`
  /// predicate it writes, in its own combinators, its tests, its benches, its examples and its
  /// conformance kit, answers out of the token it was handed, and the adversarial fixtures that
  /// *do* count clones read the counter in an assertion after the call, never inside the
  /// predicate. It is all written down because it is the honest boundary of the claim above.
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
