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
  /// A stopping token this call found **already in the stream** is not even removed: the
  /// complete-input route below judges each token where it lies. "Left at the cache front" is
  /// therefore satisfied by doing nothing at all, rather than by a pop and a matching push.
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
  /// A panic out of the predicate, the emitter, or the lexer
  /// **anywhere but the end-of-input settle** is an exit too, and it settles with **this method's
  /// own posture**: [`sync_to`](Self::sync_to)'s commit-and-keep, reporting nothing. This mode
  /// holds no pre-call snapshot, so **every unwind keeps** — there is no earlier state left to
  /// restore *to*, only the progress already committed — and its settle stops *before* the
  /// stopping token, exactly as `sync_to`'s does, which is why the frontier is what an interrupted
  /// stop still has to commit. `REPORT_SKIPPED` is `false` for this mode, so the prefix an unwind
  /// keeps is **skipped, not diagnosed**: no unexpected-token report is ever built for a token
  /// this call skips, so a panic has nothing to lose on that axis — only the genuine lexer errors
  /// crossed along the way are ever reported, and those settle and unwind exactly as any other
  /// committed emission does. No token leaves the stream, and no emitter mark is stranded. The
  /// two routes reach that by different means, and the difference is the whole of what *Two
  /// routes, one skip* below is about — the complete-input route holds no uncommitted position at
  /// any point where caller code runs, and holds a token out of the stream at exactly one call,
  /// which `LexedFront` owns
  /// across; the partial-input route keeps the shared scanner's `ScanScope`, whose `Drop` puts the
  /// in-flight token back, commits the frontier and settles the entry mark.
  ///
  /// That one call is the predicate, over a token the **lexing** run has just produced, and it is
  /// worth stating why the resident run needs no such guard while this does: a resident token is
  /// judged where it lies, so an unwinding predicate there has nothing to repair, while a lexed one
  /// exists only in this loop's hand until it is committed or put back. Left unowned, an unwinding
  /// predicate dropped it — the complete input then re-lexed it from the previous committed span
  /// where the sealed-`Partial` input, whose scope holds the same token, resumed with it resident.
  /// The two answers are within one cursor *range* of each other, which is why the panic sweep
  /// passed over it; they are not the same observation, and
  /// `the_two_completeness_routes_observe_the_same_unwound_skip` now compares them exactly.
  ///
  /// ## The one exit the two routes answer differently: an unwind inside the end-of-input settle
  ///
  /// The exclusion in the first sentence above is not a hedge, and this is the whole of it. When a
  /// skip reaches end of input both routes finish the same way — read [`Lexer::span`](crate::Lexer::span),
  /// take [`Lexer::into_state`](crate::Lexer::into_state), write the pair — and on the scanned route
  /// those two calls run *after* `ScanScope` has been disarmed, so the frontier the settle was
  /// about to commit is dropped along with the unwind. This route has no frontier to drop: it
  /// committed each token as it crossed it, and those commits stand.
  ///
  /// Measured over `"ab cd ef gh"` with a predicate that accepts everything and `into_state` armed
  /// to panic —
  /// `the_two_completeness_routes_are_pinned_apart_on_an_interrupted_eof_settle` in
  /// `src/input/input_ref/tests.rs`, which pins **both** columns as deliberate rather than
  /// asserting they match:
  ///
  /// | after the unwind | [`Complete`](crate::input::Complete), this route | sealed [`Partial`](crate::input::Partial), the scanner |
  /// |---|---|---|
  /// | committed span, and the tally of the state beside it | `(9, 11)`, 4 | `(0, 0)`, 0 |
  /// | resume cursor | 11 | 0 |
  /// | what the next reads yield | nothing; the input is spent | all four tokens, lexed again |
  /// | [`commit_token`](crate::Emitter::commit_token) notifications the interrupted call made | 4 | 4 |
  ///
  /// **The claim is narrowed rather than the behaviour changed, and the last row is why.** Both
  /// routes told the side channel that four tokens were consumed. This route's committed position
  /// says the same thing; the scanner's says the call never happened, so a recording sink is left
  /// holding four settles for tokens the input then serves a second time. Keeping the progress a
  /// call actually made is the better of the two answers, and it is the one the per-token commit
  /// produces **by construction** — the design of this route, not an accident of where the unwind
  /// landed. Degrading it to agree would cost the whole of what *Two routes, one skip* measures,
  /// and it would buy agreement on an unwind only a **lexer, source, span or offset** callback can
  /// raise: exactly the code *The condition on the caller* below already requires to be inert, and
  /// never the predicate, which is the one callback that condition does not cover. That asymmetry
  /// is the reason the predicate divergence directly above was *fixed* and this one is *stated*.
  ///
  /// **And it is this window and no wider.** Measured rather than asserted, arming one step at a
  /// time over the same source and comparing the two routes' whole residue: `Lexer::lex` at each of
  /// its five calls, `Lexer::span` at each of its five reads *but the settle's*, the predicate at
  /// every call, the committed-token observer, and the emitter's diagnostic path over a crossed
  /// limit trip all leave the two routes identical — as does every run in which nothing
  /// unwinds at all. The cell named above ships the no-unwind and emitter controls beside the
  /// divergence; the predicate sweep is
  /// `the_two_completeness_routes_observe_the_same_unwound_skip`. What is *not* claimed either way
  /// is a caller `Clone` or `Drop` that unwinds: the two routes run different numbers of those by
  /// construction, which is the first clause of the condition failing and is listed under *And for
  /// a caller who does not meet it* below.
  ///
  /// # Two routes, one skip
  ///
  /// The grammar shape this primitive exists for — skip trivia, then look — asks for a skip at
  /// every decision point, and on a GraphQL parse those skips are **39,057 calls** for 13,014
  /// trivia tokens: 22,033 of them have nothing to skip at all. Routing each of them through the
  /// shared four-mode scanner costs a `ScanScope`, a clone of the
  /// frontier pair, a take-test-put-back of the token the skip stops on, and a commit of a
  /// position that in the common case did not move.
  ///
  /// So a [`Complete`](crate::input::Complete) input does not use the scanner here. It runs a
  /// loop of its own, in two phases:
  ///
  /// 1. **the resident run** — while a token is at the front of the stream, judge it **where it
  ///    lies** through `front`. A rejection returns immediately, having removed
  ///    nothing; an acceptance consumes it through `commit_front`, the
  ///    settle that clamps the position *before* the token leaves the stream;
  /// 2. **the lexing run** — once the stream is empty, lex through
  ///    `scan_with` exactly as the scanner does, holding each lexed token in a
  ///    `LexedFront` across the predicate, committing it there if the predicate takes it and
  ///    putting it back at the front of the stream if it does not.
  ///
  /// A [`Partial`](crate::input::Partial) input keeps the scanner. That is a **typestate** split,
  /// not a second fast path: the partial-input route owes an `Incomplete` exit that restores five
  /// facts and settles an emitter mark on every exit *including an unwind*, which is precisely
  /// what `ScanScope` exists to own, and the measured cost above is a complete-input parse.
  /// Neither route carries the other's machinery, and no route carries both.
  ///
  /// ## What this route keeps of the scan scope, and what it does not
  ///
  /// **Not "it needs no scope".** It needs the scope's *token slot*, in the lexing run and only
  /// there, and nothing else the scope carries — and the heading says so because the earlier one
  /// said the opposite over a body that was already correctly qualified, which is the half a reader
  /// retains.
  ///
  /// Under `Complete` the scope's `Drop` does exactly two things — put the in-flight token back at
  /// the front of the stream, and commit the frontier the loop accumulated — and this route makes
  /// one of them unnecessary rather than skipping it, and keeps the other where it is actually
  /// owed:
  ///
  /// | what the scope settles | what this route does instead |
  /// |---|---|
  /// | the in-flight token, **resident run** | nothing: a resident token is judged where it lies and never leaves the stream until the settle that accounts for it; the clamp — the settle's one fallible step — runs while it is still there |
  /// | the in-flight token, **lexing run** | `LexedFront`, the scope's token slot and nothing else of it: a token this loop lexed is owned across the predicate and put back by the guard's `Drop`, so the stop and an unwinding predicate take the same exit |
  /// | the uncommitted frontier | there is none: every token this route crosses is committed as it crosses it, so the committed position **is** the frontier at every instant |
  /// | the entry mark | `Complete` takes none — the capture is behind `Cmpl::PARTIAL` and never monomorphizes |
  ///
  /// The scope's own `HOLDS_ENTRY = false` for `SkipWhile`, so its unwind edge always *keeps*:
  /// the disposition this route lands on by construction is the one the scope was going to choose.
  ///
  /// The split in the first two rows is the whole of what the measurement bought, and it is the
  /// reason the guard is where it is. The census that motivates this route is **22,033 skips that
  /// skip nothing** out of 39,057 — every one of them answered by the resident run, which
  /// constructs no guard, and none of them by the lexing run, which is entered once per
  /// non-resident token and has already paid for a lex by the time it gets there.
  ///
  /// ## Where the limit trip latches
  ///
  /// A resource trip latches the poison boundary at the **durable frontier** — the offset up to
  /// which what the scan already passed stays reproducible. The scanner latches it through
  /// `AtFrontier`, its deferred, uncommitted frontier; this route latches it through
  /// `AtCursor`, the committed cursor. **They are the same offset**, and for the same reason the
  /// frontier commit disappears: this route commits each skipped token as it crosses it, so with
  /// the stream empty — the only state in which it lexes — [`offset`](Self::offset) reads
  /// `span().end()`, which is the end of the last skipped token, which is exactly what the
  /// scanner's frontier holds. Everything downstream of the latch is unchanged, because it all
  /// lives inside `scan_with`: the trip is diagnosed there, deduplicated there,
  /// and the fatal-emitter exit is taken there. This route only decides what to do afterwards, and
  /// the answer is *nothing* — the progress a trip keeps is already committed.
  ///
  /// The pre-lex probe is the other half: once the lex position has reached a latched boundary
  /// there is no token left to scan, so this route stops without rebuilding a lexer, exactly as
  /// the scanner does — and, again, with nothing left to commit.
  ///
  /// ## What is guaranteed identical, and the condition that makes it so
  ///
  /// Producing the same values is not the same as running the same code, and in a generic library
  /// the difference is not academic: `L::Span`, `L::State` and `L::Offset` are the **caller's own
  /// types**, so every operation the input layer performs on one of them — clone, drop, compare,
  /// hash, format — is caller code, as are [`Emitter::checkpoint`](crate::Emitter::checkpoint) and
  /// [`release`](crate::Emitter::release), every [`Cache`](crate::cache::Cache) method, and the
  /// [`Lexer`](crate::Lexer) with its [`Source`](crate::Source). This route runs a **different**
  /// set of them. Some are measured, on the same stream in the same residency, by the effect
  /// ledger in `fast_path_tests`, for the call that dominates the census — a skip that skips
  /// nothing:
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
  /// A skip that *does* skip goes the other way on some rows, and that is stated rather than
  /// hidden: the scanner clones the frontier pair once and clamps once at the end, while this
  /// route runs the clamp — a [`Source::len`](crate::Source::len), an `L::Offset` comparison and
  /// an `L::Span::clone` — once **per token**. It also asks the cache for its front once per
  /// token where the scan pops once per token. Fewer caller steps for the 56% of calls that skip
  /// nothing, more for the ones that skip a run, and the condition below is what makes the
  /// direction of the difference not matter.
  ///
  /// **That table is a measurement, not a boundary**, and the distinction is the whole lesson of
  /// how this contract was arrived at. The table carries the steps the ledger was built to watch.
  /// The scan's closing commit also runs a [`Source::len`](crate::Source::len) and one `L::Offset`
  /// comparison — the clamp inside `commit_position`, measured once each on the scan and zero
  /// times here for a no-op skip — and those are simply two the table never had. Successive
  /// readings of this method have each found an operation the previous naming did not contain, so
  /// the condition below is quantified over **all** caller-supplied code and every list inside it,
  /// this table included, is illustration. What `fast_path_tests` pins is the *emptiness* of this
  /// route's side for a no-op skip, not an inventory of the scan's: an inventory would be one
  /// review round out of date.
  ///
  /// ### What this route does differently — the whole of it
  ///
  /// Four clauses, meant as exhaustive and not as illustration. Against the scan it replaces, for
  /// every call it answers, this route:
  ///
  /// 1. runs a **different multiset** of caller-supplied steps — it may omit one, run one the scan
  ///    never runs, or run one a different number of times. *Which* steps is deliberately not part
  ///    of the clause: the table above measures some, the elided drops and the per-token clamp are
  ///    more, and the clause is about all of them. It was a subset relation while this route
  ///    answered only the skips that skip nothing; it is not one now, and saying so is cheaper
  ///    than maintaining the list that made it true;
  /// 2. **substitutes** a [`Cache::front`](crate::cache::Cache::front) for the
  ///    [`pop_front`](crate::cache::Cache::pop_front) +
  ///    [`push_front`](crate::cache::Cache::push_front) pair the scan uses to look at the same
  ///    head. The cache laws make that pair an identity on the entry it came from, so the two are
  ///    the same read of the same token;
  /// 3. **reorders**: `pred` runs before the steps of (1) that the scan takes ahead of it — the
  ///    entry capture, the frontier clone — rather than after them;
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
  /// * **your input-layer callbacks are inert.** Every caller-supplied operation this crate can
  ///   reach through the input layer does **only** what its own contract says it does, and always
  ///   returns normally: it does not unwind, and it does not diverge. Then running one, running it
  ///   twice, running it a hundred times and not running it at all are the same thing to everyone.
  ///   *That is the clause, and it is not a list.* The ones you are likely to be the author of:
  ///   the `Clone` and `Drop` of `L::Span`, `L::State` and `L::Offset`, and the `Ord` and `Hash`
  ///   the bounds also ask of the span and the offset;
  ///   [`Emitter::checkpoint`](crate::Emitter::checkpoint) and
  ///   [`release`](crate::Emitter::release); every [`Cache`](crate::cache::Cache) method; the
  ///   [`Lexer`](crate::Lexer) and its [`Source`](crate::Source). Those are named because they are
  ///   the ones you are likely to write, **not because they are the boundary** — the boundary is
  ///   the sentence above them, and each time this contract named a set instead, the next reading
  ///   found something outside it;
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
  /// covers *all* of that code and makes omitting it, adding one, running it a different number of
  /// times, and running it in a different order produce nothing; the second says your own
  /// predicate cannot read which route produced its argument. Nothing is left over. That closure
  /// is the point of stating a condition rather than listing exclusions — a list has to anticipate
  /// every way a caller might differ, and every way found so far is an instance of one clause
  /// rather than a new entry: a `Clone` that keeps state, a `Clone` that panics, a `Drop` elided
  /// along with its clone, the `Source::len` and `L::Offset` comparison a clamp runs, and a `pred`
  /// that reads its argument's address. The first three arrived as three separate escalations of a
  /// list that was each time believed complete; the last two arrived after the clause replaced it,
  /// and needed no change to it — nor did the per-token clamp, which is the first difference that
  /// runs *more* caller code rather than less.
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
  /// count**. Pinned across the typestate split as well: a **sealed**
  /// [`Partial`](crate::input::Partial) input takes every decision a
  /// [`Complete`](crate::input::Complete) one takes, so the two routes are held to the same
  /// observation over the same program, source and cache-capacity sweep — **with one exit
  /// excluded**, which a caller who meets the condition above cannot reach: an unwind *inside the
  /// end-of-input settle* leaves this route holding the prefix it crossed and the scanner holding
  /// nothing. *Panic unwind* above states that difference, measures it and bounds it; every
  /// callback that can raise it is one the first clause requires to be inert.
  ///
  /// ### And for a caller who does not meet it
  ///
  /// Not a second list to maintain, and not one that has to be complete — each of these is one of
  /// the two clauses failing, and each is measured in `fast_path_tests`, so the condition is a
  /// fact rather than a caveat. A caller who breaks a clause in a way no bullet here describes
  /// gets the same answer: the guarantees are not made to them.
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
  ///   call sequence: with a head this route accepts, `pred` runs once and *then* the clamp's
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
  ///   Measured over one no-op skip: one `L::State::drop` and three `L::Offset::drop`s on the scan
  ///   under `Complete`, two and seven under `Partial`, against none of either here.
  ///   (`a_no_op_skip_runs_no_caller_drop_where_the_scan_runs_one_per_frontier_clone`)
  /// * **a `pred` that reads its argument's *address*** — the second clause, in the half that says
  ///   *values*. The scan moves the head out of the cache into the scan scope and asks about it
  ///   there; this route asks about it where it lies. A predicate that compares the address it is
  ///   handed against the cache's own front entry therefore answers one way here and the other
  ///   there, and the answer to a skip predicate is the skip: one token consumed against none.
  ///   This one is not even new to the fast paths — the cache has always been an invisible
  ///   optimization, and how deep a caller peeked has always decided where a token sits — which is
  ///   exactly why it belongs in a condition on the caller rather than in a list of this route's
  ///   differences. (`an_address_reading_predicate_can_change_the_skip_decision`)
  ///
  /// The condition is reasonable, not merely convenient. A predicate that answers differently
  /// depending on **how the input layer got the token to it** is not asking a question about the
  /// input at all — it is asking about this library's internal route, and which route answers a
  /// given call is a choice this crate makes and may change in any release. A callback that
  /// unwinds asks the same question in control flow: it turns *which steps ran* into an outcome,
  /// and which steps ran is not part of the contract either.
  ///
  /// The crate meets its own condition and holds itself to it. Every span, state and offset it
  /// ships clones by copying fields and compares and hashes by derive over integers, with no side
  /// effect and no panic path — surveyed impl by impl — so the first clause holds for every
  /// in-tree lexer by inspection, and holds for the operations no table names as readily as for
  /// the ones it does. That is what checking a clause instead of a list buys. Every `skip_while`
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
    // `Cmpl::PARTIAL` is an associated const, so exactly one of these two bodies monomorphizes
    // per instantiation and the other is eliminated whole. See *Two routes, one skip* above for
    // why the split is by typestate and not by anything else.
    if Cmpl::PARTIAL {
      return self.skip_while_scanned(&mut pred);
    }
    self.skip_while_direct(&mut pred)
  }

  /// The partial-input trivia skip: the shared four-mode scanner, entered through the same
  /// resident-head probe it has always had.
  ///
  /// Kept on the scanner deliberately. A partial-input skip owes an
  /// [`Incomplete`](crate::error::Incomplete) exit that restores five facts — the committed span
  /// and state, the dedup watermark, the poison latch and every emission the aborted attempt made
  /// — and settles an emitter mark on **every** exit including an unwind, and
  /// [`ScanScope`](super::scan::ScanScope) is what owns exactly that. The measured cost the
  /// complete-input route exists to remove is a complete-input parse; paying for it here would buy
  /// a second implementation of the one thing the scope is for.
  #[inline(always)]
  fn skip_while_scanned<F>(
    &mut self,
    pred: &mut F,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    // The no-op skip, answered where the token already is. `false` unless the probe below both
    // ran and accepted; then the scan's first token is the head this already asked about, and its
    // answer is carried in rather than asked twice.
    let mut head_answered_skip = false;
    if let Some(front) = self.front() {
      if !pred(front.token) {
        return Ok(());
      }
      head_answered_skip = true;
    }

    // A partial-input trivia skip and a recovery sync are the same scan: take each token from the
    // cache while one is there and from the lexer once it is not, settle every skipped token
    // behind the frontier, and stop on the first token the predicate picks out — leaving it
    // unconsumed at the cache front. They differ only in the mode ([`SkipWhile`]: report nothing,
    // commit at end of input) and in the POLARITY of the predicate, which is this one negation: a
    // sync stops on the token it matches, a skip stops on the first token it does not.
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

  /// The complete-input trivia skip: this method's own loop, with no scan scope, no deferred
  /// frontier and no take-test-put-back.
  ///
  /// Two phases, and the boundary between them is a fact rather than a choice: phase 1 runs while
  /// a token is at the front of the stream, phase 2 runs once there is not — and the one put-back
  /// phase 2 performs is its [`LexedFront`]'s, which happens as the guard leaves the body on the
  /// way out of the call, so the phases still cannot interleave.
  ///
  /// `#[inline(always)]` for the same reason [`skip_until`](Self::skip_until) carries it: the
  /// lexer lives in an `Option` built the moment the stream runs dry, and left out of line it
  /// cannot be scalar-replaced, so it is re-loaded from the stack on every token.
  #[inline(always)]
  fn skip_while_direct<F>(
    &mut self,
    pred: &mut F,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    // ── Phase 1: the resident run — each token judged, and consumed, where it lies ──
    while let Some(front) = self.front() {
      if !pred(front.token) {
        // The stop. The token was never taken out, so "an unconsumed token lives at the front of
        // the stream" holds by having done nothing to it: no pop, no push-back, no push history.
        return Ok(());
      }
      // The settle's ONE fallible step, taken while the token is still in the stream. Three
      // caller-supplied operations live in here — `Source::len`, an `L::Offset` comparison and an
      // `L::Span::clone` — and a panic through any of them leaves the token exactly where it is
      // and the committed position exactly where it was. That ordering is what replaces the scan
      // scope FOR THIS PHASE: there is no window here in which a token is out of the stream and
      // unaccounted for, so this loop has nothing for a `Drop` to repair. Phase 2 does — see
      // `LexedFront` — and the claim is deliberately not made for the method as a whole.
      let clamped = self.clamped_span(front.token.span.into());
      // ── nothing fallible from here to the publish ──
      drop(self.commit_front(clamped));
    }

    // ── Phase 2: the lexing run — the stream is empty, so the committed position IS the frontier ──
    let mut lexing: Option<Resume<L, L::Offset>> = None;
    loop {
      if lexing.is_none() {
        // The DURABLE stop first, before the lexer is built: a budget that has already refused an
        // item owes this turn a stop, and `InputRef::refusal_already_on_record` publishes it in
        // front of the resume below rather than behind it. `AtCursor` is the durable frontier here
        // for the same reason it is the scan's below — with the stream drained, `offset()` is
        // `span().end()`, the end of the last skipped token.
        //
        // Then the positional memo: a sticky limit trip latches a poison boundary at the durable
        // frontier; once the lex position has reached it there is no token left to scan. Every
        // token this call skipped is already committed — that commit is the scanner's `commit_at`
        // here — so either stop leaves nothing to settle and the call simply stops.
        if self.refusal_already_on_record(&AtCursor) || self.reached_boundary(self.span.end_ref()) {
          return Ok(());
        }
        // The committed pair, read from one value, with the stream provably drained: exactly
        // where the scanner's frontier resume would land.
        lexing = Some(self.resume());
      }
      let resume = lexing.as_mut().expect("the lexer is built just above");
      // `scan_with` centralizes the poison latch, the dedup watermark, the partial-input frontier
      // rules and the fatal-emit discipline, handing back only the events this loop must decide.
      // `AtCursor` is the durable frontier here BECAUSE this loop commits per token: with the
      // stream drained, `offset()` is `span().end()`, the end of the last skipped token — the
      // very offset the scanner's `AtFrontier` carries.
      let scanned = self.scan_with(resume.parts_mut(), &AtCursor);
      match scanned {
        Ok(Scan::Token(tok)) => {
          // The state is paired with the token HERE, above the predicate, exactly where the
          // scanner pairs them (its `Fetched` is built at the fetch). A token without the state
          // that lexed it is not a cache entry, so it cannot be put back — and a put-back is
          // precisely what the guard below owes an unwind. Cloning after the predicate, as this
          // did, made the guard unbuildable at the moment it is needed.
          let state = resume.lexer().state().clone();
          // The one window this route has: a token OUTSIDE the stream with caller code running
          // over it. `LexedFront` owns it across `pred`, so a predicate that unwinds leaves it
          // PARKED rather than dropped — the scan scope's `TokenSlot::Held` arm, narrowed to the
          // single call that needs it and confined to the phase that already pays for a lex.
          let mut lexed = LexedFront::new(self, CachedToken::new(tok, state));
          if !pred(lexed.token()) {
            // The stop, on a token this loop lexed: put it back where an unconsumed token lives.
            // The guard performs it as it goes out of scope, so this exit and the unwind edge are
            // the SAME call — lexed, so the put-back is a genuinely new cache entry and its push
            // is recorded, the same call, with the same origin, the scanner's `to`-shaped stop
            // makes.
            return Ok(());
          }
          // Accepted: the token leaves the guard for the settle, and this route's own posture
          // takes over from the guard's. Nothing is resident and the committed position is this
          // token's start, so the ordinary settle is already atomic here: a panic through its
          // clamp drops a token the region re-lexes, against a position that never moved.
          lexed.commit();
        }
        // The trip is latched and diagnosed inside `scan_with`, and the progress it keeps is
        // already committed.
        Ok(Scan::Tripped) => return Ok(()),
        Ok(Scan::Eof) => {
          let lexer = lexing
            .take()
            .expect("the lexer is built just above")
            .into_lexer();
          // Everything from the cursor to the end of input matched and was skipped: keep that
          // progress, at the LEXER's end rather than the last token's, so trailing bytes the
          // lexer skips are accounted for. BOTH operands are caller code and both are evaluated
          // before either half of the position is written.
          //
          // THE ONE EXIT THE TWO ROUTES ANSWER DIFFERENTLY, and deliberately: the scanner runs
          // this same settle with its scope already disarmed, so an unwind through either operand
          // drops the frontier and the whole skipped run with it, while here every token is
          // already committed and the unwind keeps it. Stated, measured and bounded under *Panic
          // unwind* on `skip_while`; pinned — as deliberate, not as parity — by
          // `the_two_completeness_routes_are_pinned_apart_on_an_interrupted_eof_settle`.
          let span = lexer.span();
          let state = lexer.into_state();
          self.commit_position(span.into(), state);
          return Ok(());
        }
        // A fatal rejection of a crossed lexer error's diagnostic: `settle_fatal` already
        // committed the position inside `scan_with`, over a prefix whose tokens this loop had
        // already committed one by one. Nothing is left to settle.
        Err(e) => return Err(e),
      }
    }
  }
}

/// The freshly-lexed token, owned across the one call that can unwind while it is out of the
/// stream.
///
/// # Why the complete-input route needs *this much* scope and no more
///
/// [`skip_while_direct`](InputRef::skip_while_direct) gave up the scan scope on the strength of one
/// sentence: *no token is out of the stream at any point where caller code runs*. That is true of
/// the resident run by construction — a token there is judged where it lies and removed only by the
/// settle that accounts for it — and it was **false** of the lexing run for exactly one call.
/// `scan_with` hands back a token into a local; `pred` then ran over it with nothing owning it, so
/// a predicate that unwound dropped it. The complete input resumed from the previous committed span
/// and re-lexed the token; the sealed-[`Partial`](crate::input::Partial) input, whose
/// [`ScanScope`](super::scan::ScanScope) holds the same token in `TokenSlot::Held`, put it back at
/// the front and resumed there. Two routes, one skip, two different resume cursors under
/// `catch_unwind` — and the panic sweep could not see it, because a cursor *range* between the
/// committed end and the next token's start admits both answers.
///
/// So the guard is the scope's token slot and **nothing else** of it. It carries no frontier (this
/// route commits every token as it crosses it, so the committed position is the frontier at every
/// instant), no snapshot and no mark (`Complete` takes none), and it exists only inside the lexing
/// run — which is reached once per non-resident token and already pays for a lex. The resident run
/// above never constructs one: putting the scope back there is what the measurement this route
/// exists for would have paid for.
///
/// # Its two outcomes are one put-back and one settle
///
/// * **accepted** — [`commit`](Self::commit) hands the token to the ordinary settle and leaves the
///   guard holding nothing, so the settle's own posture takes over unchanged: a panic through its
///   clamp drops a token the region re-lexes against a position that never moved, which is the
///   crate's standing posture for every 1:1 consume and is *not* what this guard is about;
/// * **stopped, or unwound** — `Drop` puts the token back at the front of the stream. The normal
///   stop and the unwind edge are therefore the same call rather than two that must be kept in
///   step, which is the property the scanner's `Drop` has and the reason this is a guard and not an
///   extra `if`.
///
/// # What it costs, measured — and it is not near zero
///
/// **It gives back about three fifths of this route's own win — roughly two fifths is what
/// ships**, and the number is written here because the shape that produced it is not obvious and
/// the next reader will otherwise assume, as this one did, that a guard on the slow path is free.
///
/// `benchmarks/examples/perfloop` on smear's `bench/apollo-comparison`: minimum over nine blocks,
/// `apollo-parser` as an unchanged control, the three builds **interleaved** within each
/// repetition so drift hits all of them equally, five repetitions, and any repetition whose
/// control reading sat more than 2% above the floor discarded as contended.
///
/// | | alias (57 741 B) | supergraph (7 494 B) |
/// |---|---|---|
/// | `origin/main` — the shared scanner | 1600.2 µs | 159.0 µs |
/// | this route, no guard | 1321.0 µs (−17.4%) | 133.7 µs (−15.9%) |
/// | **this route, shipped** | **1482.5 µs (−7.4%)** | **148.1 µs (−6.9%)** |
///
/// So the guard is **+12.2% / +10.8%** of a whole parse, and the route is still ahead of the
/// scanner it replaced by roughly 7%. The fraction in the heading is that table's own arithmetic,
/// written out because it was got backwards once: the win without the guard is 17.4 and 15.9
/// points, the win with it is 7.4 and 6.9, so the guard hands back 10.0 of 17.4 and 9.0 of 15.9 —
/// 57.5% and 56.6%, three fifths — and what ships is the other two.
///
/// **The whole of it is the unwind cleanup region, not the guard's work.** Two measurements pin
/// that, and together they say there is nothing left to optimize. Both come from a separate
/// same-session shape survey whose own no-guard baseline read 1345 µs / 134.0 µs, so they are
/// quoted as differences against that and not against the table above:
///
/// * the identical struct with the `Drop` impl **deleted** — same `Option`, same accessors, same
///   moves — is free (1307 µs / 135.3 µs). So the wrapper, the `Option` and the token's round trip
///   through it cost nothing;
/// * the guard **with** its `Drop`, compiled `panic = "abort"`, is also free (1328 µs against 1363
///   µs for the same build without it). So a build that cannot unwind pays nothing at all.
///
/// A destructor in this loop makes every call in its scope an `invoke` with a landing pad, and the
/// lexing loop is hot: it is entered once per token that is not already resident, which on a real
/// parse is most of them. Six shapes were built and timed, and this one — the guard owning the
/// token across the predicate, inline, in `skip_while_direct` — is the **cheapest** of them.
/// Against that survey's baseline: scoping the guard to the predicate call alone (+280 µs), taking
/// its `Drop` out of line (+200 µs), moving the predicate call into an `#[inline(never)]` helper so
/// the loop itself carries no destructor (+310 µs) and dropping `skip_while_direct` to plain
/// `#[inline]` (+140 µs) are all worse or no better; outlining the whole lexing phase is a wash
/// with this (+140 µs) and costs 10 µs of its own even without a guard. Removing the guard's own
/// panicking branches (`expect` → a total `match`) changes nothing, so the region is not held open
/// by anything this file added.
///
/// The correctness this buys is not optional — the two typestate routes disagreed on the resume
/// cursor under `catch_unwind` without it — so the cost is reported rather than traded away. What
/// is *not* paid is the thing the route was written for: the **resident run** builds no guard, so
/// the 22,033-of-39,057 skips that skip nothing are untouched, and so is every skip whose tokens
/// are already in the stream.
struct LexedFront<'g, 'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  ir: &'g mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  /// The token, while it is still the guard's. `None` once [`commit`](Self::commit) has handed it
  /// to the settle — which both performs the accept and disarms the put-back, so the two cannot
  /// drift apart.
  tok: Option<CachedTokenOf<'inp, L>>,
}

impl<'g, 'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl>
  LexedFront<'g, 'inp, 'closure, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Takes ownership of a token the loop has just lexed. Infallible and a pure move: the pairing
  /// with the lexer state — the one fallible step of building a cache entry — happens at the call
  /// site, above this, so there is no window between the token existing and being owned.
  #[inline(always)]
  fn new(
    ir: &'g mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    tok: CachedTokenOf<'inp, L>,
  ) -> Self {
    Self { ir, tok: Some(tok) }
  }

  /// The token, borrowed, for the one site that evaluates the predicate — the scanner's
  /// `TokenSlot::held`, by the same name and for the same reason.
  #[inline(always)]
  fn token(&self) -> Spanned<&L::Token, &L::Span> {
    self
      .tok
      .as_ref()
      .expect("the predicate runs with the token held")
      .token()
  }

  /// The accept: hand the token to the ordinary settle and disarm the put-back in the same move.
  ///
  /// SETTLE_CENSUS — the complete-input trivia skip's lexing-phase `commit_token`, moved onto the
  /// guard so that "the token is the guard's" and "the token has been settled" cannot both be true.
  /// The `take` is a move, not a removal from durable state: this token was never in the stream, so
  /// it opens no removal window of the kind the census rails on.
  #[inline(always)]
  fn commit(&mut self) {
    let (tok, state) = self
      .tok
      .take()
      .expect("one settle per lexed token")
      .into_components();
    self.ir.commit_token(tok.data(), tok.span_ref(), state);
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> Drop for LexedFront<'_, 'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  fn drop(&mut self) {
    // Armed by the token's presence and disarmed by the accept's own `take`, so the accepted path
    // reaches here with nothing to do and the two exits that DO owe a put-back — the stop, and a
    // predicate that unwound — perform the identical one.
    if let Some(tok) = self.tok.take() {
      self.ir.hold_front(tok, Origin::Lexer);
    }
  }
}
