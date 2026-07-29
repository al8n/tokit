# Changelog

All notable changes to this crate are documented here. The project follows semantic
versioning; before 1.0, a minor bump (0.x → 0.(x+1)) signals a breaking change.

## Unreleased

### Changed (breaking)

A CST sink is bound to its source when it is built, not when it is finished, and the
materialization walk no longer rescans.

1. **`Sink::new` takes the source and a `CstProfile`; `finish` and `finish_partial` no
   longer take a source.** The source is bound once, at construction, and the three loose
   `u16` parameters (`mapper`, `error_kind`, `gap_kind`) move into `CstProfile` alongside
   the new `KindValidator`.

   **What this buys, stated exactly.** There were two chances to hand the sink the wrong
   buffer: at construction and again at `finish`. The second is gone — you can no longer
   materialize against a source the sink was not built with. The first remains open, and it
   is one witness of the limitation below.

   ```rust
   // before
   let sink = Sink::new(emitter, map_kind, ERROR, GAP);
   let (green, emitter) = sink.finish(ROOT, source);

   // after
   let sink = Sink::new(source, emitter, CstProfile::new(map_kind, ERROR, GAP));
   let (green, emitter) = sink.finish(ROOT);
   ```

2. **An out-of-language kind now panics at the door, in every build.** A dialect mapper that
   returns a kind outside its own kind space — or the reserved tombstone `u16::MAX` — is
   refused by an `assert!` at the emit site (`cst_start`, `cst_start_at`, and the shared
   `record_token` body behind both token doors, plus `CstProfile::new` for the two
   synthesized kinds). Previously the same input reached `finish` and came back as
   `Err(FinishError::InvalidDialectKind)`.

   **This is a behaviour change for a caller that relied on the error.** It is a panic
   because the condition is a dialect bug, not an input condition: the mapper is the
   dialect's own code, no parse input can provoke it, and refusing at the cause reports it
   one materialization earlier than the error did.

   The honest limit, which is also stated at the site: the *per-event* validator inside
   `finish` is `cfg!(debug_assertions)`-gated, because keeping it in every build cost a
   measured **+8.3%** on ordinary materialization — an unpredictable indirect call per event
   inside a tight builder loop. Every route reachable from outside this crate goes through a
   door that validates in every build, so for external callers the wall is absolute. The one
   route with no door is `push_raw_event_for_tests`, which is `pub(crate)`. So: a **release**
   build whose event log was assembled by raw in-crate injection can materialize an
   out-of-language kind. Every test run and every CI build refuses it. `ReservedKind` is not
   gated — the tombstone band is a plain comparison and was a release wall before this round.

3. **`FinishError` gains `InvalidDiagnosticSpan`, `MismatchedFinish`, `NonUtf8Source` and
   `InvalidDialectKind`.** The sink now names the node it closed, so a mismatched
   finish reports which frame it failed against instead of producing a misparented tree.

### Fixed

- **A session's budget guard now holds in release builds.** `PartialSession` refuses an
  attempt that would exceed its budget or that follows a terminal latch, and it relies on the
  downstream `From<SessionRefusal>` conversion preserving terminal status. That requirement
  was enforced by a `debug_assert!`, so a **release** build could surface `BudgetExhausted` or
  `TerminalLatched` as an error whose `is_incomplete()` returned true — and a caller would
  then refill and retry forever against a session that refuses before doing any work. A
  zero-work infinite refill loop is precisely the denial-of-service shape the budget exists to
  prevent, and it was absent from the builds that matter. The check is now unconditional.

  **Behaviour change:** a `From<SessionRefusal>` implementation that drops terminal status now
  fails in release as it always did in debug. The cost is one `is_terminal()` call per
  *refused* attempt — never on the success path, and nothing per token or per event.

### Streaming CST is not in this release

`PartialSession::parse` now requires `Ctx::Emitter: ValueKeyedEmitter`, which makes pairing a
session with a recording `Sink` **uncompilable**. This is a deliberate scope decision with a
return path, not an omission.

The reason is that the *sound* way to use the pairing was never enforceable. A session redrives
each attempt from the base, so a threaded sink accumulates the replayed prefix; the correct
usage is a fresh sink per attempt, and **no type can observe freshness.** The choice was
therefore between shipping a combination documented as unsound and shipping a bound that makes
it unrepresentable. The bound also states something the crate already believed: `Sink`
deliberately does not implement `ValueKeyedEmitter`, on the stated ground that a sink cannot
wrap a sink.

**Diagnostics-only streaming is unaffected** — `Fatal`, `Verbose`, `Silent`, `Ignored` and
`&mut` of each all satisfy the bound, and a compile-time control pins that so a future
tightening cannot take streaming diagnostics with it unnoticed. Everything else in the
streaming lifecycle ships: the budget, the committed frontier, the terminal latch, the
refusal-coherence law.

**What returns with the witness:** the chunked-session-equals-one-shot guarantee, and the
supported fresh-sink-per-attempt story.

**The bound closes the session route only.** A hand-rolled refill loop that calls
`parse_partial` directly with a shared recording sink produces the same duplication — measured,
not inferred. `parse_partial` is unchanged: it predates this release, and a *single* call with
a sink is perfectly sound, so bounding it would forbid working code rather than only broken
code. If you hand-roll a redrive loop, build a fresh sink per attempt.

### Known limitation — a parse and the emitter serving it share no identity

Nothing in the `Emitter` or `ParseContext` contract lets an emitter learn which parse it is
serving: `Cursor` carries an offset and a `PhantomData`, and `commit_token` carries a token and
a span. More precisely: **CST emission carries no witness of the parse it belongs to.**
`CstEmitter`'s contract names no completeness parameter, no attempt and no input, so any holder
of the emitter may append events at any moment, and the event log cannot distinguish events
from the live attempt, from an abandoned one, or from no parse at all.

**One witness remains open in this release**, pinned by a test that asserts **today's wrong
answer** so that closing the contract gap flips it:

- **A sink can be bound to a foreign source.** A `Sink` built over `"XY"` and driven over a
  same-length `"ab"` yields a tree whose text is `"XY"` and whose structure came from `"ab"`.
  The lifetime proves both borrows outlive the sink, not that they are the same buffer. —
  `sink_bound_to_a_foreign_source_is_not_yet_detected` (the two sources are deliberately equal
  length, so a length check cannot satisfy it cheaply).

The stale-structure witness — an abandoned attempt's node surviving into a later attempt's tree
— is closed for the session route by the bound above, and its test was deleted rather than
weakened, because the shape it exercised no longer compiles.

**The fix is one contract with two additions, and neither is a `CstEmitter`-only change.** A
*completeness* witness on the emission contract leaves automatic emission untouched, because
`commit_token` lives on `Emitter`, not `CstEmitter` — and that same fact means a *source*
identity cannot live on `CstEmitter` either, since it would miss `Sink::commit_token`, the path
every ordinary parse uses. It has to sit where both paths cross: the point an emitter is
attached to an input. Minting the sink from the input or session does both at once, and is the
mechanism that restores streaming CST.

Refusing any of this at materialization is not merely expensive, it is **impossible**: the bad
event logs are byte-identical to logs a legal single parse could produce, so `finish` has
nothing to discriminate on.

**The shape of the gap, stated as an invariant rather than a list of doors.** *CST emission
carries no witness of the parse it belongs to.* `CstEmitter`'s contract names no completeness
parameter, no attempt and no input, so any holder of the emitter may append events at any
moment, and the event log cannot distinguish events from the live attempt, from an abandoned
one, or from no parse at all.

Every route is an instance of that: the `InputRef` and `ParseState` accessors that stay
generic over completeness — deliberately, because `Partial` legitimately needs them for
**diagnostic** emission — and every combinator that hands either to a user callback. The
sink's own owner needs no route at all. Pinning individual combinators to `Complete` walls an
instance; it cannot wall the class, which is why this is deferred as contract work rather than
patched door by door.

**The fix is one contract with two additions.** A *completeness* witness on the emission
contract closes witness 2 while leaving automatic emission untouched — `commit_token` lives on
`Emitter`, not `CstEmitter` — so the supported streaming shape survives it. A *source
identity* on the same contract closes witness 1.

### Known limitation — `finish` does not refuse duplicate zero-width token spans

Separate and smaller, listed apart because it is **not** the identity gap: the monotone-span
wall never fires when duplicated spans are both `[0,0)`, so such a log materializes. A
conforming lexer cannot produce one — a token is a span of consumed bytes, and a lexer
returning zero-width tokens without advancing does not terminate — so this is reachable only
through the raw event surface or a third-party `Lexer` that ignores the contract, which is
what the sink's release walls exist for. It pins a property of *materialization*, not of the
parse-to-emitter contract, and it flips when `finish` starts refusing these spans. —
`duplicate_zero_width_tokens_are_not_yet_detected`

### Added

- **Streaming sessions.** `PartialSession`, `Budget`, `SessionRefusal`, `RedriveFromBase` and
  the sealed `ReplayMode` give `parse_partial` a lifecycle: a partial parse survives its
  refills instead of restarting, under a budget that a caller sets and can observe. See the
  known limitation above before pairing a session with a recording `Sink`.
- **`CstText`** — the source-to-`&str` bridge, so a byte-like source materializes a tree when
  it is valid UTF-8 and returns `FinishError::NonUtf8Source` with the offending offset when it
  is not, rather than being unusable.

### Performance

The materialization walk was one linear pass **plus a from-zero coverage rescan per gap**,
which is Θ(n²) in the number of diagnosed gaps. It is now two passes that are linear in
events — a gather, then the walk against a shared monotone cursor — **plus one `k log k`
ordering of the `k` recorded diagnostic spans.** With one lexer error per token `k` is
Θ(events), so materialization is **O(n log n)**, not linear; the quadratic term is what this
round removes.

A distribution sort that would make it genuinely linear was implemented and **rejected on
measurement**: a fixed four-pass radix costs 15n against a 9n budget at every size, an
adaptive one steps between one and two passes across the probe range and reports growth
indistinguishable from real nonlinearity, and both pay ~1024 fixed operations per `finish` in
the common case where a parse records a handful of diagnostics. It remains a candidate with
its own measurement.

- `finish_error_dense` — **15.9× faster** (4 096 alternating error/token pairs). A growth
  probe pins the shape rather than the constant: 799 / 3 199 / 12 799 units of replay work
  against bounds of 900 / 3 600 / 14 400, growth **4.00×** for a 4× input.
- `finish_wrap_heavy` — **3.62× faster** (2 048 retro-wrap targets).
- `finish_clean` — **18% slower, and this ships.** It is the harness's designated
  no-regression control, so it is disclosed rather than folded into an average: ~19 → ~22 ns
  per token over 4 096 tokens, measured on `finish` alone. It reproduces at all nine
  alignment residues of a padding sweep (min +12.4%, max +22.8%), so it is a real cost and
  not the layout artifact its bimodal raw readings first suggested. Two candidate causes are
  eliminated — the event footprint is unchanged (`size_of::<Event<SimpleSpan>>()` is 32 on
  both sides) and the new reachability bitset never allocates on clean input — and the
  residual is not yet attributed. `finish`'s internals are private, so an attribution can
  land in a patch release without breaking anyone.

### Changed (breaking) — the input layer's unwind edges

Nothing that can unwind runs between taking a token out of the stream and recording where
it went. Every scan, guard and emitter edge that previously had caller code in that gap now
settles on the panic path as well as the return path, and the rollback protocol has one
owner instead of two.

1. **`Cache::rewind` is removed.** Rollback was implementable in two places — the cache
   could rewind itself, and the input drives the same rollback through its lineage — so a
   cache that took the first route and an input that took the second disagreed about what
   the stream held. There is now one owner. Implementors of `Cache` delete the method; the
   four in-tree implementations did, and no caller loses a capability, because every public
   rollback path already went through the input.

2. **`Cache`'s non-panicking restore-path law now names `push_front`.** The clause previously
   listed the reading and removing operations the crate calls while restoring — `pop_front`,
   `pop_back`, `clear`, `front`, `front_span`, `len` — and omitted the one *writing* operation,
   which is exactly the one the new unwind edge in item 3 depends on. An implementation whose
   `push_front` can panic was vacuously conforming before and is not now.

   The rider is stated rather than implied: a `push_front` that panics is the single case the
   crate **cannot** make whole, because it has already taken the token by value and nothing can
   un-take it. Every other restore-path operation is recoverable.

3. **A panic inside caller code no longer corrupts the stream.** `skip_until` pops a token
   out of durable state — the parked slot, or the cache front — and then runs caller code
   over it: the predicate, the expected-tokens closure, `L::State: Clone`, the lexer
   itself. Any of those can panic, and a panic through one used to be an exit that no
   put-back and no settle ever saw. The in-flight token was dropped with an unowned local,
   the skipped prefix stayed behind a frontier that was never committed, and a rewinding
   mode's entry mark was neither rewound nor released. **With a warm cache that lost tokens
   outright.** A `Verbose` record could likewise tear if the payload's own allocation
   panicked mid-push.

   The unwind edge is split by **disposition**, and each half mirrors an exit the scan
   already owns. Committing modes (`SyncTo`, `SkipWhile`) behave as their fatal exit does —
   commit the diagnosed prefix, put the in-flight token back — so a host that catches and
   retries resumes *after* the diagnosed prefix with no duplicate reports. Rewinding modes
   (`SyncThrough`, `SyncBalanced`) restore the full pre-call state and rewind the mark **when
   the scan is abandoned mid-flight**.

   Disposition follows the **exit, not the mode**: once a stop has been decided, an
   interrupted stop keeps the diagnosed prefix for every mode. `SyncBalanced` is where that
   distinction is load-bearing — it rewinds at end of input and keeps on a stop — so the
   guard reads which exit it was interrupted in rather than which mode it is running. A uniform clear was tried and
   measured doing real harm on the committing path — it discarded pre-entry cache entries,
   the re-lex re-burned a shared limit budget, and a poison boundary latched at a position
   the original lineage never reached.

   This is a behaviour change only for a host that catches a panic across a parse and keeps
   using the input. Callers that let a panic propagate see no difference.

4. **The committed position is written as one step, everywhere.** The position is a *pair* —
   the span, and the lexer state that produced it — and many sites wrote it in two steps with
   caller code in between. At end of input, `SyncTo::on_eof` advanced the span and then called
   `lexer.into_state()`; a panic there left the span advanced with the entry state still paired
   to it, so a host that caught and resumed lexed from the new offset under a state that had
   seen nothing. Reachable through `sync_to`, `skip_while` and `padded`, and only for
   **stateful** lexers — the population least likely to notice.

   **Advancing the span without supplying the state is no longer expressible.** The span-only
   setters are gone — once every caller had to pass the state, the compiler reported them as
   dead code — and the internal token settle carries the state to a single funnel that
   evaluates every fallible step first, writes both halves as infallible moves, and drops the
   replaced values afterwards. A `Drop` is caller code; dropping in place would run it between
   the halves, which is the tear itself. Where a restore has more to do than the position, the
   funnel hands the replaced pair back so the caller drops it only after the watermark, the
   poison latch and the emitter mark are restored too.

   **Both halves are installed before either replaced value is dropped**, at every site that
   writes one. That ordering is the property a future change has to preserve, and the reason
   is easy to miss: an assignment installs its new value and *then* drops the old one, so
   `span = …; state = …;` lets the replaced span's destructor run **between** the two writes.
   If it unwinds there, the second write never happens and one token's span is published
   beside the previous token's state — off by exactly one, silently, and only for spans or
   states that have a destructor at all.

   This is internal surgery: `Emitter::commit_token`'s signature is unchanged, so emitter
   implementations need no edit. The one implementor-visible consequence is an ordering change
   — the `commit_token` observer hook now runs **after** the position is published and
   **before** the replaced pair is dropped, so a panicking destructor cannot swallow the
   notification.
   Every caller reaches it having already taken the token off the front of the stream, so
   notifying first would leave the committed position naming a token the stream no longer
   holds, and a host that caught the observer's panic would resume past a token it never saw.
   An observer that panics now leaves the input consistent, with only the notification missing.

5. **`Lexer` gains a clause: the settle path's operations must not panic.** This is a real new
   constraint on `Span` and `Offset` implementations, not a footnote, and it is the one window
   in the consume path that **cannot** be closed by reordering.

   The input layer settles a consumed token by computing the committed position **from that
   token's own span**. The span does not exist until the token has been popped, so those steps
   necessarily run after the token has left the stream and before the position has moved. The
   operations are `Source::len`; `Span::end_ref` and the `Ord` on `Offset` that decide whether
   a clamp is needed; `Span::clone` on the common path; and, only when the span is clamped to
   the source, `Span::start_ref` plus `Offset::clone` plus `Span::new`. **None of them may
   panic.**

   Two operations that were in this window during development are **not** in it and are
   deliberately absent from that list: the `Drop` of the span and state a commit replaces, and
   the `Drop` of the `Source::len` temporary. Both were moved out — the replaced pair is handed
   back to the caller and dies after the settle, and the length temporary now rides out with
   it. Nothing above is removable for the same reason, stated once: **every one of them is
   computed from the token's own span, which does not exist until the token has been taken.**

   The posture matches `Emitter::release`/`rewind` and the `Cache` restore-path operations, and
   for the same reason: the operation runs where nothing else can repair it.

   **The bounds are unchanged: `Lexer::Span` still requires `Clone`, not `Copy`.** A span that
   allocates, refcounts or carries a `Drop` is still welcome — an `Arc`-carrying file id clones
   by bumping a refcount, which cannot panic, and satisfies this clause fine. `Copy` would force
   the clause for free, and was rejected precisely because it would ban working code to close a
   window that a non-panicking `Clone` already closes. The in-tree types happen to satisfy it
   trivially (`SimpleSpan` is `Copy`; `usize` offsets neither allocate nor drop), which is why
   the default configuration pays nothing.

   Like the two clauses it matches, this is an obligation the crate **states and cannot check** —
   Rust has no "this impl does not panic" bound. A span whose `Clone` can allocate and abort
   under memory pressure will still lose a token, and nothing catches it at compile time.

   **What you get in return is a statable guarantee:** if your span's clone, construction,
   comparison and drop do not panic, a consumed token is never lost.

   Two alternatives were measured and rejected. Cloning the span before the pop costs an extra
   `front()` read on `next`, the hottest path in the crate; passing the span by value only
   relocates the clone — it helps `next`, which destructures, and hurts `consume_cached_one`,
   which keeps the `Spanned`.

6. **An `Incomplete` scan exit leaves no trace.** A scan that ran out of input mid-decision
   used to commit the frontier it had reached, so a caller that topped up the source and
   retried resumed at a position the first attempt had already half-consumed. It now exits
   as it entered.

### Fixed — a regime change could silently corrupt lexer-error deduplication

`set_state` and `state_mut` share a body that cleared the cache, dropped the parked token and
cleared the poison boundary, **then** cloned an offset, **then** re-anchored the dedup watermark.
`Offset::clone` is caller code. A panic there left the watermark holding the **dead regime's**
value on an input that had just been unpoisoned — so for the rest of the parse, lexer-error
dedup compared new-regime spans against a stale mark and **silently suppressed or duplicated
diagnostics**.

The read is now hoisted above every mutation and takes the span's end directly, which is provably
the same value: the cursor reports the parked token, else the cache front, else the span — and the
two clears exist precisely to empty the first two, so the fallback branch was the one that ran
either way. The old "cleared before the cursor read" ordering was documenting a coupling that
existed only to steer the cursor into that branch.

### Fixed — a checkpoint rollback is all-or-nothing

`restore_unchecked` interleaved caller code with the facts it was installing: cache eviction,
session-point abandonment and a span clamp all ran between the lineage pop and the position
restore. A panic in any of them left the input **half-rolled-back** — lineage, cache and emitter
on the checkpoint's branch while the position and the remaining facts stayed on the abandoned one.

The body is now two phases. **Phase 1 runs every operation that can execute caller code, while
the input is still wholly on the branch being abandoned** — so a panic there leaves it there,
which is the *unchanged* half of all-or-nothing. **Phase 2 installs every fact with no caller
code among them.** (Staging the evicted values to a single drop site was the obvious repair and
is not available: checkpoints work in allocator-less builds, so there is nowhere to put an
unbounded number of evicted entries. The ordering achieves the same guarantee without one.)

**The residue, stated precisely:** a rollback is atomic unless a **cached token's, or an abandoned
session point's, own `Span`/`State` drop panics** — unreachable for every type this crate ships,
since they are `Copy`. Such a panic leaves the input on the branch it was already on, with part of
a cache that would have been discarded anyway.

### Fixed — an interrupted restore can no longer make an abandoned scan's diagnostics permanent

`restore_entry` restored the input's position and facts and **then** cloned a cursor to hand
`Emitter::rewind`. `Offset::clone` is caller code, so a panic there left the input restored with
the emitter mark **not rewound** — and the scan scope's fallback then *released* that mark, making
an abandoned scan's diagnostics and token-observer effects permanent while the parser retried from
the restored position.

**Hoisting the clone within the body does not fix this**, which is worth stating because it is the
obvious repair: wherever in `restore_entry` the clone sits, failing it still skips the rewind. The
operation had to leave the body entirely. The scan's entry record now carries the rewind cursor,
cloned at capture — **before the mark exists** — so `restore_entry` performs no caller operations
at all.

The same round removed three further in-place field writes on the restore paths where a panic
would have left the input and the checkpoint disagreeing about the dedup watermark and the poison
latch.

### Added

- **`conformance::cache`** — the cache contract as a runnable kit rather than prose.
  `CacheHarness` drives an implementation through the rollback, put-back and lookahead laws
  the trait states, so a third-party `Cache` can be checked against them instead of
  inferred to comply.

### Performance

- A nested rollback is **linear in lineage depth** rather than quadratic.
- **The scan path costs measurably more, and the wall-clock figure is not yet trustworthy.**
  The ownership that closes item 3 is the cost. It cannot be relocated: the benched path
  lexes with no peek, so there is no stream slot to borrow the token from, and tracking each
  handover separately — which is what makes the unwind edge correct — is state the scan must
  carry.

  What is measured and solid is code size: the `input_scan` bench closure grows **+560 bytes
  against 0.7.3**, and a **356-byte `drop_glue::<ScanScope>`** symbol now exists where every
  earlier revision of this round measured none. Two mitigations were tried and both reverted
  on measurement — outlining the whole `Drop` as cold made a second bench worse.

  An earlier revision of this round disclosed **+1.5%** on `skip_trivia_next`. **That figure
  is stale** — it was taken when the symbol above was 1 680 bytes and it is now 2 100 — and
  it is deliberately not restated here rather than reprinted at a precision it no longer has.
  A replacement is owed on a quiet machine before release; it may be materially larger.
  Against this, `failed_sync_through_over_8` improved 1.4–2.5% when last measured cleanly.

### Fixed (documentation)

- The `cache` module no longer advertises a dynamic, allocator-backed cache with unlimited
  lookahead. No such implementation exists, `alloc` does not add one, and none could serve a
  public peek past 32 tokens — `Window` is sealed at U1–U32. Lookahead beyond the window is
  what transactions are for.

### Changed (breaking)

The Pratt driver ends an expression when the RHS channel says so, not when the input
position looks finished — and a report that consumes nothing can no longer be folded.

1. **`PrattRHS` gains an `End` variant.** A grammar says the expression is over on the
   same channel it uses to report operators, instead of relying on a sentinel operator
   below the minimum power. Exhaustive matches on `PrattRHS` must add an arm; the
   compile error is deliberate, which is why the enum is not `#[non_exhaustive]` — a
   wildcard arm is exactly the silent end-swallowing this change removes.

2. **The `is_eoi` loop gates are gone.** They ended the expression on a *position*,
   which a lookahead could move: a widening peek made a typed parse of `1+2` return
   `1`. The RHS channel now decides.

3. **Recursion floors are explicit.** `PrattFloor` replaces the arithmetic that
   reconstructed a floor from a power. Right-associative operators recurse at their own
   power (an off-by-one that made a right-associative pin yield 14 where 10 is correct);
   left and non-associative recurse strictly above it, which also removes the wrong
   parses the old code produced at `Power::MAX`, where the arithmetic saturated instead
   of separating. The token flavour carries its true left power rather than
   reconstructing it.

4. **`PrattPower::next` and `prev` are removed.** They were the arithmetic that made
   the floor bugs expressible; with them gone, reintroducing driver-side power
   arithmetic is a compile error rather than a review comment. This breaks callers, not
   only implementors, and no lint would have flagged them as unused — a required trait
   method warns nowhere. Note also that every in-repo implementation was non-saturating
   despite the trait's own documentation asking for saturation, so each carried a latent
   debug overflow panic.

5. **A report that consumes nothing is refused, terminally.** The typed driver checks
   committed consumption at the report boundary — after the floor and rejection logic,
   before any classify, fold, wrap, or recursion — and on a stall rolls the cycle back
   and returns a terminal `UnexpectedEoLhs` or `UnexpectedEoRhs`. Previously a
   zero-consumption report could be folded: a prefix reported after a peek recursed at
   the same position until the stack overflowed, and a zero-width infix produced
   `Ok(6)` from `1 2 3` with two phantom folds and no diagnostic on any channel.

   This adds `From<UnexpectedEoLhs<…>>` and `From<UnexpectedEoRhs<…>>` to the typed
   driver's error requirements. They are the engine's terminal exits for a
   `parse_lhs`/`parse_rhs` contract violation — ordinary operand exhaustion is still
   `UnexpectedEot` and is unchanged.

6. **The trace channel is more verbose.** With the position gate gone, the RHS channel
   is consulted at exhaustion, so traces gain the consultations the gate used to skip.
   Parse results and non-trace output are unchanged.

The token driver needs none of this: `PrattToken::try_pratt_rhs` and `try_pratt_lhs`
take only `&self` with no `InputRef`, so a report there cannot be made without
consuming, and no token fold receives an `InputRef`. That asymmetry is why the guard
lives in one driver and not the other.

### Changed (breaking)

The bound a grammar production writes is now **one line**. Where a production previously
restated every conversion its callees might need, it names a single context bundle and the
compiler elaborates the rest.

1. **`ParseCtx` is renamed `ComposableParseContext`** and completed. It now carries
   `FromTokenErrors` — the five token-level conversions as one supertrait — so the nested
   associated-type bound elaborates to callers. `ParseContext` is unchanged. The rename
   touches 59 occurrences across 14 files; the crate-root re-export set gained and lost
   nothing.

2. **`FromUnclosed` replaces the per-delimiter `From<Unclosed<D, …>>` family.** One bound
   covers every delimiter pair, discriminated at runtime with a mandatory catch-all arm.
   `UnclosedEmitter::emit_unclosed`'s method bound moves with it. Type-level per-pair `From`
   impls still work alongside the umbrella, and remain the documented way to discriminate at
   the type level.

   **Migration:** delete the per-delimiter `From<Unclosed<…>>` bounds from your where-clauses
   — a residual one now fails as a bare `E0277` with no note, because `From` is a foreign
   trait we cannot annotate.

   **The discriminator is `Unclosed::kind`, not the display name.** Two further breaking
   changes carry it:

   - **`Delimiter` gains a required `const KIND: DelimiterKind`** (new, exported from
     `tokora::delimiter`). No default: a defaulted identity is one the author never chose.
   - **`Unclosed::new` and `Unclosed::of` take the kind** between the span and the name, and
     `Unclosed::kind()` reads it back. The four typed constructors (`Unclosed::paren` and
     siblings) are unchanged.

   Routing on `Unclosed::name_ref` was unsound as a dispatch key and is no longer documented
   as one. `name` is a display string with no uniqueness contract — `"[]"` is the correct name
   for *any* bracket-shaped pair — so a consumer who defined a custom pair and named it
   correctly had it silently absorbed by the built-in `Bracket` arm, reporting a character the
   source never contained. A pair tokora does not define should declare `DelimiterKind::Custom`,
   a variant that can never equal a built-in.

   **The accident is unrepresentable: `DelimiterKind`'s four built-in variants are themselves
   `#[non_exhaustive]`.** Only tokora can *write* one. `DelimiterKind::Bracket` in another
   crate is a privacy error (`E0603`) — as a `Delimiter::KIND` declaration, as the `kind`
   argument to `Unclosed::new`/`of`, anywhere — so the author who reaches for `Bracket`
   because `"[]"` was the right name for their pair no longer compiles. Matching is untouched
   at a cost of one token: **an arm outside tokora is written `DelimiterKind::Bracket { .. }`**,
   the pattern form `#[non_exhaustive]` on a variant leaves open. A `compile_fail,E0603`
   doctest on `DelimiterKind` pins it, paired with a positive control differing in one token.

   **That fences the spelling, not the value, and it is not a provenance check.** A built-in
   kind is still obtainable by a crate that is not tokora, three ways: by projecting
   `<tokora::punct::Bracket<…> as Delimiter<…>>::KIND`, since the const is public and its type
   unconstrained; by reading `Unclosed::kind` and passing it back to `Unclosed::new`/`of`; or
   from `Unclosed::bracket`/`bracket_of` and their `paren`/`brace`/`angle` siblings, which mint
   a built-in-kinded error in one public call and need no `Delimiter` impl at all. The first
   and third name tokora's own pair; the second does not — a generic adapter forwarding an
   error copies whatever kind it was handed. So a `DelimiterKind::Bracket { .. }` arm firing
   means dispatch was not keyed on a display string — not that the caller chose the kind, and
   not that the diagnostic came from tokora. All three routes are pinned
   green in `tests/unclosed_kind_dispatch.rs`, asserting the forgery succeeds, so closing one
   later breaks a line rather than making this entry quietly wrong.

   The residue is accepted deliberately. Closing the projection alone is possible — a
   `#[doc(hidden)]` provided method with a parameter type unnameable outside tokora does it,
   while a sealed supertrait plus a blanket impl over a public key trait does not compile at
   all (`E0119` against the `&D` forwarding impl) — but it leaves the other two routes open and
   buys no stronger claim. Closing all three means making the eight typed constructors
   crate-private and reshaping `new`/`of` so they cannot take a kind, which makes
   `Unclosed<char>` unconstructible outside tokora. That is refused; `DelimiterKind`'s docs
   carry the full reasoning.

   What this does **not** fix, also stated at the type: two custom pairs in one crate can still
   both declare `Custom("mine")`; and a pair the arm set forgot still lands in the catch-all —
   over a generic `D` the compile-time obligation is gone for good, and `#[non_exhaustive]`
   keeps the catch-all mandatory.

   **Migration:** replace `match err.name_ref()` with `match err.kind()` and the string
   literals with `DelimiterKind::` variants — `DelimiterKind::Bracket { .. }` for a built-in,
   braces included; add `const KIND` to any custom `Delimiter` impl —
   `DelimiterKind::Custom("your-key")`, the only variant your crate can name; add the kind
   argument to any direct `Unclosed::new`/`of` call, or reach for
   `Unclosed::bracket`/`bracket_of` and their siblings where the pair is one of tokora's.
   `name_ref` is unchanged and remains correct for rendering.

3. **The `_of` language twins are gone — 167 public items removed.** Every `X::parse_of` is
   now `X::parse`, inferring `Lang` from the input. This covers 148 punctuator methods
   (74 markers × 2), `Keyword` ×8, `Ident` ×2, `Any` ×3, six free functions, and two more per
   type generated by `keyword!`. **`list_of` is renamed `list`** — it had no bare twin, so the
   suffix carried no meaning.

   *Not* collapsed, because `Lang` is genuinely unrecoverable there: `ParserContext::of` and
   `with_cache_options_of` (an emitter does not determine its own language brand), the
   argument-less constructors, and every error constructor `_of` (their `Lang` is a phantom
   brand reached only through a `From` obligation). The `one_of` family is unrelated — it
   means "one of these", not "of this language".

   **Migration:** drop the `_of` suffix. A call site that already spelled its generics gains
   one type argument, `Lang`, in last position; `_` is almost always sufficient.

4. **The fluent `separated_by_*` family works for branded grammars.** It previously failed on
   `.collect()` with an error pointing frames away from its cause, because the generated
   methods hard-coded the bare marker `Comma<(), (), ()>`. They now instantiate the separator
   at the caller's language — `Comma<(), (), Lang>` — so the return type of every
   `separated_by_*` method gains that argument.

   The marker's own brand and the context language remain the **same** parameter on the
   `Punctuator` impls: a marker branded for one grammar is not a punctuator of another.
   Widening the impl is a second route to the same fix and is deliberately not taken — it
   would let a separator copy-pasted out of a sibling dialect type-check silently. The fence
   is pinned by a `compile_fail,E0277` doctest on `Punctuator`, paired with a positive
   control differing in one token.

5. **`Token::Kind: 'static` is hoisted to the trait**, deleting five projection restatements.
   Four superficially similar bounds remain: they constrain the dispatch structs' own free
   `Kind` parameter, which the hoist does not reach.

6. **`Parser` loses its `Error` phantom parameter** and gains value-driven entry points
   (`parse`, `parse_with`, `parse_with_state`); its constructors collapse from eight to four.

7. **The delimiter side gets item 4's fix.** `Delimiter` and `TypedDelimiter`'s blanket impls
   for `Paren`, `Brace`, `Bracket` and `Angle` carried a marker-language parameter independent
   of the context language, so `Paren<(), (), LangA>` satisfied `Delimiter<'_, L, LangB>` and a
   production copy-pasted out of a sibling dialect type-checked silently. The two are now the
   **same** parameter.

   As with the separators, the widening was never what made the fluent family work. Every
   `delimited_by_*` and `try_delimited_by_*` — on `ParseInput` and on all eleven many-builder
   surfaces — now instantiates the pair at the caller's language, so their return types gain
   that argument. The seven option builders (`AtLeast`, `AtMost`, `Bounded`, and the four
   leading/trailing options) are generic only over the parser they wrap and have no language
   to name, so their four `delimited_by_*` methods take the brand as a **method** type
   parameter instead; the driving impl's `Delim: Delimiter<'inp, L, Lang>` obligation unifies
   it with the context language. A builder stored in a `let` and never driven has to name the
   brand itself.

   The pair-typed error vocabulary moves with it, or two routes to one shape would disagree on
   the error type. `parens`, `braces`, `brackets`, `angles` and their attempt twins type the
   `Unclosed` they emit on the pair at their own language, and the twelve
   `Unclosed*`/`Unopened*`/`Undelimited*` aliases plus their twelve `Lang`-generic constructor
   blocks name it the same way — `UnclosedParen<S, Lang>` is now
   `Unclosed<Paren<(), (), Lang>, S, Lang>`.

   **Migration:** at `Lang = ()` every spelling is unchanged, since `Paren` already means
   `Paren<(), (), ()>`. A branded grammar spells the pair's brand in two places: a
   `TypedDelimiter` bound forwarded through a **generic** lexer (`Bracket<(), (), Lang>:
   TypedDelimiter<'inp, L, Lang>`), and a turbofish that names the pair
   (`delimited::<Bracket<(), (), MyLang>, …>`). At a concrete lexer the impl resolves and
   neither is written. The fluent methods need nothing. The fence is pinned by two
   `compile_fail,E0277` doctests on `Delimiter`, paired with a positive control differing from
   each in one token.

### Added

- **`Dialect`** — a two-item anchor (`Lang`, `Lexer`) with the projection aliases `LexerOf`,
  `LangOf`, `DialectSlice`, `DialectInput` and `DialectErrorOf`. Everything else a production
  needs — source, slice, token, span, offset, token capabilities — is reachable by projection,
  so pin those in a one-line subtrait rather than as further associated items.

  There is deliberately no `DialectCtx`. A context bound written through `LangOf<'inp, D>`
  does not unify with one written at the brand: the param-env predicate stays at the
  projection while the obligation normalises. This is documented at the type with the
  mechanism and the verbatim failure, so the shape is not rediscovered by trial.

- **`#[diagnostic::on_unimplemented]` on `FromTokenErrors`, `FromUnclosed` and `Dialect`.**
  A missing conversion now reports what is missing and carries a copyable impl skeleton,
  rather than surfacing as a nested obligation inside crate internals.

  Note for consumers: rustc attaches the *root* obligation's attribute. A bound reached
  through your own blanket-implemented subtrait reports that subtrait, not ours — annotate
  your own dialect traits if you want curated text there.

### Notes

`#[diagnostic::do_not_recommend]` was evaluated and rejected. It does not change whether the
curated message appears; its only effect is to hide *which* member is missing, which is the
one datum a consumer needs. Measured on 1.87 and on nightly.

### Added

- **`InputRef::is_exhausted()`** — the consumer-exhaustion predicate: no lexed token is waiting and
  the lexer frontier has reached the end of the buffer. This is the correct gate for a driver loop,
  where `is_eoi()` is not: `is_eoi` answers `true` the moment any lookahead lexes through the end,
  while the tokens that lookahead produced are still unconsumed. `is_exhausted()` is independent of
  the cache implementation.
- **`Cache::RETAINS_FRONT`** — a new trait constant declaring that a front push into an **empty**
  cache always succeeds, i.e. the cache can retain at least one token. It defaults to `false`, so
  the addition is semver-compatible and an existing implementation keeps working unchanged. A cache
  that declares `true` lets the input layer prove its parked-front slot statically unreachable, so
  every probe of it folds away at monomorphization; the shipped caches declare it accordingly
  (`Option` `true`, `GenericArrayDeque<_, N>` `N != 0`, the black holes `false`). A cache that
  declares `true` and then refuses a front push into an empty cache is violating the contract, and
  now panics at the refusal rather than losing the token.

### Fixed

- **A token left unconsumed by a `to`-shaped scan stop or by a `try_expect*` / `probe_close`
  decline is now retained even when the configured `Cache` refuses it**, so the cursor, offset,
  `is_eoi`, the `sync_balanced` hole anchor and the lexer-state tally after such a stop are the
  same under every cache capacity — including a zero-capacity `()`, where the token used to be
  dropped and re-lexed by the next call. `ClosePayload::CacheFront` is renamed
  `ClosePayload::AtFront` (crate-internal).
- **`InputRef::lexer()` now resumes under the state that produced the newest retained token**
  rather than under the committed state, so a widening lookahead no longer lexes the token after
  a retained run as if that run had never been scanned. A by-value `Lexer::State` limiter is no
  longer bypassed by the lookahead pattern, and the tokens a drain yields, the final state and
  the diagnostics it emits no longer depend on how deep — or in how many steps — the caller
  peeked first.
- **A non-final EOF now reports the lexer's own end offset** instead of the end of the last item
  it yielded, so bytes the lexer skipped before exhaustion (trailing whitespace, a comment tail)
  are no longer omitted from the `Incomplete` frontier a refill driver reads. The reported offset
  is floored by what was already lexed and clamped to the buffer.

### Fixed (behaviour-breaking)

Six deliberate behaviour changes, all in the repetition drivers' end-state accounting. No
API is removed or renamed.

1. **A properly closed separated + delimited list now reports its count bounds and separator
   policy.** `…separated_by_*().delimited::<D>()` left its element loop through the mid-scan
   closer without running the end-state pass, so on every well-formed, properly closed list the
   whole of `at_least` / `at_most` / `bounded` and the leading/trailing separator policy was
   dead code — the sibling `separated_*_while` driver ran it on the identical path. Under a
   recovering emitter such a list now records the diagnostic it always should have; under a
   fail-fast emitter a bounds- or policy-violating closed list is now an `Err` where it used to
   be a silent `Ok`.
2. **`TooMany` is emitted once per construct, with a count that exceeds the limit it names.**
   `nums()` is now `limit() + 1` at every emission site. It used to be either the running count
   (so the same four-element history yielded a different payload from each builder) or, from
   the mid-loop hook, the limit itself — a "found 2 … exceeds … 2" that contradicted itself.
   The duplicate emission (mid-loop hook plus a delegating end check) is gone.
3. **`FullContainer` is emitted once per construct**, on the whole-construct span in every
   driver, with a `nums()` that includes the refused element. A container that refuses one push
   refuses every later one, so the old per-dropped-element re-emission produced a count that
   climbed past the capacity it named.
4. **Count bounds now judge the elements the driver parsed, not the ones the container
   stored.** In the separated drivers a bounded container used to turn a satisfied `at_least`
   into a spurious `TooFew`, and — worse — to swallow a violated `at_most` entirely by clamping
   the count the check reads. Only bounded-capacity containers are affected; with `Vec`,
   `VecDeque`, `SmallVec` and `TinyVec` the stored and parsed counts are always equal.
5. **`SeparatorHandler::on_separator` now receives every separator in source order.** It used
   to receive the exact complement of its documented contract — only the leading separator and
   the duplicates in a run, never the happy-path or trailing ones. A container that ignores
   separators can opt out with the new defaulted `SeparatorHandler::OBSERVES_SEPARATORS`
   associated const, which suppresses both the call and the clone that feeds it; every
   container in this crate does. Adding the const makes `SeparatorHandler` no longer
   object-safe — `dyn SeparatorHandler` no longer compiles. Nothing in this crate used it.
6. **The delimited separated drivers return the whole construct's span**, opener through
   closer, on every success exit. Some exits previously returned a span measured from the first
   element instead, so the returned span excluded the opener and depended on which exit the
   parse took.

### Fixed

- **`InputRef::foldr_within` no longer drops a run shorter than its window.** The exhaustion
  arm returned the untouched initializer *after* consuming the run into the fold's buffer, so
  every run of length `1 ..= W::CAPACITY - 1` was silently discarded on the `Ok` path. Both
  exits now converge on the single reverse drain, as the sibling `foldrn` already did.

## 0.7.3 (2026-07-24)

### Added

- `FusedDispatchOnKind` now implements `TryParseInput`: a table hit remains lex-once, while a
  table miss or end-of-input returns `ParseAttempt::Decline` without consuming valid input.

## 0.7.2 (2026-07-24)

### Added

- **Fallible projection traits.** `tokora::utils::Downcast<T>` and
  `DowncastRef<T>` provide owned and borrowed `Option<T>` projections for domain
  types without imposing blanket implementations or a concrete conversion model.

### Fixed

- **Wrong opening delimiter at end of input is no longer misreported as end-of-input.** The
  delimited many-drivers (`repeated`, `repeated-while`, `separated`, and `separated-while`
  `…delimited::<D>()`) now report a wrong opening delimiter as the expected-open unexpected-token
  diagnostic carrying that token, even when it is the final token — instead of `UnexpectedEot`.
  The wrong token stays unconsumed, and the diagnostic no longer depends on whether another
  token happens to follow it. `UnexpectedEot` is now reserved for a genuinely empty opener
  position. Fixes issue #85.

## 0.7.0 (2026-07-23)

### Added

- **Lifetime-preserving borrowed sources.** `Source<usize>` is now implemented explicitly for
  `&'data str`, `&'data [u8]`, and (with `bstr_1`) `&'data BStr`. Their associated slices retain
  `'data` even when the source value itself is borrowed for a shorter call.

### Changed (breaking)

- **`Slice<'source>` now guarantees validity for `'source`.** The trait has a
  `+ 'source` supertrait requirement. Canonical implementations live on `str`, `[u8]`, `BStr`,
  and the optional backend value types; a shared-reference blanket implementation forwards
  `&T`, including nested references, without changing the representation or iterator behavior.
  External `Slice` implementations must ensure that the slice value and its represented data
  outlive `'source`.
- **Borrowed backend slices preserve their representation and longest available lifetime.**
  `BStr` sources now yield `&BStr` instead of `&[u8]`. `HipStr<'data>` and `HipByt<'data>`
  sources now yield `HipStr<'data>` and `HipByt<'data>` rather than shortening the associated
  lifetime to the method borrow.
- **`Source` references are intentionally explicit rather than blanket-forwarded.** This avoids
  shortening a lifetime carried by the source through an unrelated outer borrow. Owned backends
  such as `Bytes`, `HipStr`, and the smol-bytes types remain `Source` implementations on their
  owner types; borrowing an owner to call its methods remains unchanged.

### Migration

- Update custom `Slice<'source>` implementations so the implementing type satisfies
  `'source`. Prefer implementing `Slice` on the canonical representation and let the shared
  reference blanket provide `&T`.
- Code that names `<BStr as Source<usize>>::Slice<'a>` as `&'a [u8]` should use `&'a BStr`
  instead; call `AsRef::<[u8]>::as_ref` when a byte slice is specifically required.

## 0.6.2 (2026-07-23)

### Changed

- `Ident`, `Keyword`, and the literal carriers now accept unsized source/data types.
  Borrowed accessors are available for carriers such as `Ident<str>`, `Keyword<str>`, and
  `LitDecimal<str>`; construction and other by-value operations remain limited to sized
  source/data types. Unsized language markers are now supported consistently by generated
  literals, `Ident` recovery, and `IdentList` APIs; the established public generic order is
  unchanged.

## 0.6.1 (2026-07-22)

### Added

- **Method-form delimiter combinators.** Every `ParseInput` parser can now use
  `delimited::<D>()` directly, with `delimited_by_parens`, `delimited_by_braces`,
  `delimited_by_brackets`, and `delimited_by_angles` as named conveniences.
- **Named delimiters for many-builders.** The same delimiter method family is available on
  `Repeated`, `RepeatedWhile`, `Separated`, `SeparatedWhile`, `AtLeast`, `AtMost`, `Bounded`,
  `AllowLeading`, `AllowTrailing`, `RequireLeading`, and `RequireTrailing`. Existing
  repeated/separated `delimited::<D>()` chains remain source-compatible, including nested
  cardinality and separator-policy wrappers.

## 0.6.0 (2026-07-22)

### Changed (breaking)

- **Delimiter-specific `Unclosed` diagnostics.** Delimited shape and many parsers now emit `Unclosed<D, …>` using the delimiter marker directly; built-in shapes use their concrete `Paren`, `Brace`, `Bracket`, or `Angle` tag. Consumer error types should provide the corresponding `From<Unclosed<…>>` conversions.
- **Composable emitter bundle includes unclosed diagnostics.** `ComposableEmitter`, and therefore `ParseCtx`, now includes `UnclosedEmitter`; custom composite emitters must implement it.
- **Built-in delimiter markers are language-neutral.** Bare `Paren`, `Brace`, `Bracket`, and `Angle` work with any parser `Lang`; the marker language no longer has to match the parse language.

## 0.5.1 (2026-07-21)

### Added

- **Full smol-bytes integration** — `Source`, `Slice`, `Equivalent`, and `ToEquivalent`/`IntoEquivalent` impls for all four `smol-bytes` types: `shared::Bytes`, `compact::Bytes`, `Utf8Bytes` (`shared::Utf8Bytes`), and `compact::Utf8Bytes`. `Source`/`Slice` treat the byte types as `u8` sequences and the UTF-8 types as `char` sequences with UTF-8-boundary-aware indexing; `Equivalent` compares all four types via their byte view, mirroring the existing `bytes`/`hipstr` impls; `ToEquivalent`/`IntoEquivalent` convert the byte types from `[u8]`/`&[u8]` and the UTF-8 types from `str`/`&str`. Feature-gated behind `smol_bytes_0_1`.

## 0.5.0 (2026-07-20)

### Added

- **`Source::as_slice`** — returns the entire source as its `Slice` associated type (the same contents as a full-range `slice(..)`). Element-agnostic across `str`, `[u8]`, `Bytes`, `HipStr`/`HipByt`, smol-bytes `shared`/`compact`/`Utf8Bytes`, and `BStr`; owned sources share the whole source via `clone`, borrowed sources return `self`/`as_ref`.

### Changed (breaking)

- `Source::as_slice` is a **required** trait method (no default body): any external `Source` implementor must now provide it. This is the source-breaking change behind the 0.5.0 bump.

## 0.4.0 (2026-07-20)

### Fixed

- **Unterminated delimited many-builders now report the opener as `Unclosed` through the
  emitter instead of silently accepting the input.** A delimited many-builder
  (`item.repeated(…)`, `item.repeated_while(…)`, `item.separated_by_*(…)`, or
  `item.separated_by_*_while(…)` closed with `.delimited::<D>().collect()`) driven over input
  whose closing delimiter never arrives before end-of-input — e.g. `"(1 2"`, `"[1 2"`, `"{1 2"` —
  used to return `Ok` with the elements parsed so far. It now emits an `Unclosed` diagnostic
  carrying the **opener's span** and the delimiter pair's name:
  - under a fail-fast `Fatal` emitter the parse fails with it (via the `From<Unclosed<…>>`
    conversion);
  - under a recovering `Verbose` emitter the diagnostic is recorded and the parse recovers,
    yielding the elements collected so far.

  A wrong token where the closer belongs still reports the existing unexpected-token
  (expected-close) vocabulary. The `separated`+delimited driver — which previously *did* error
  at end-of-input, but with a stale unexpected-token pointing at the last element rather than at
  the opener — now reports `Unclosed` at the opener like the other three drivers.

  The close-status diagnostic is the **primary**: the `separated`/`separated_while` delimited
  drivers emit it **before** the end-state secondaries (`TooFew`, separator policy), so a
  fail-fast emitter fails with `Unclosed` on e.g. `[` under `at_least(1)` or `[1,2,` at
  end-of-input rather than letting the secondary short-circuit it, and a recovering emitter
  records primary-then-secondaries in order. The plain `repeated`/`repeated_while` delimited
  drivers already ordered the close-status diagnostic before their bound checks.

- **The delimited many-builders commit the closer without re-lexing it, fixing a
  blackhole-cache (`ParserContext<_, _, ()>`) double-scan on the success path.** Internal,
  non-breaking. `InputRef::probe_close` used to classify the closer by scanning it and then
  push the scanned token back to the cache for a follow-up `try_expect` to commit; under the
  blackhole cache `()` the push-back is a no-op, so the closer was dropped and the follow-up
  `try_expect` **re-scanned** it. That second scan is observable to a stateful or
  resource-limited lexer — a valid delimited list (e.g. `(a)`) could trip its limiter, or hit
  the "unreachable" recovery path, on otherwise-valid input. `probe_close` now classifies the
  closer without consuming it — carrying the scanned token out by value, or leaving a cached
  closer at the front — and a new by-value commit primitive advances the cursor over it once, at
  the driver's own commit point, with zero re-scans in every cache capacity. Because the probe
  stays cursor-neutral until that commit, the deferred (`separated`/`separated_while`) drivers
  span their elements correctly and an error before the commit leaves the closer available for
  recovery. All four delimited many-builders (`repeated`, `repeated_while`, `separated_by_*`,
  `separated_by_*_while`) adopt it; the `DefaultCache` path is unchanged (it already scanned the
  closer exactly once). This also removes the same latent double-scan from the `Unclosed` fix
  above, which shipped the identical push-back pattern.

- **Unterminated delimited shape parsers now report the opener as `Unclosed` through the
  emitter, matching the many-builders.** The delimited shape parsers (`delimited::<D>`,
  `parens`, `braces`, `brackets`, `angles`, and their `try_` twins) used to raise a plain
  unexpected-token / end-of-input error when the closing delimiter never arrived; they now
  follow the same four-way close-miss law as the delimited many-builders above. End of input
  with the opener still open emits an `Unclosed` diagnostic carrying the **opener's span** and
  the delimiter pair's name (a fail-fast `Fatal` emitter fails with it; a recovering `Verbose`
  emitter records it and recovers, yielding the construct with a closer synthesized at the
  insertion point — a zero-width span); a wrong token where the closer belongs stays the
  existing unexpected-token (expected-close) diagnostic, not `Unclosed`; and a terminal scanner
  stop surfaces the committed form's end-of-input error, adding no `Unclosed`. The `try_` twins
  keep their decline law (absent opener ⇒ `Ok(None)`, zero consumption) and, once the opener is
  committed, report `Unclosed` on an unterminated group — never a silent decline.

### Changed (breaking)

- Added `UnclosedEmitter`, a new atomically-composable emitter sub-trait
  (`tokora::emitter::UnclosedEmitter`) with a single `emit_unclosed` method, implemented by the
  built-in `Fatal`, `Verbose`, `Silent`, and `Ignored` emitters.
- The delimited many-builder `ParseInput`/`Collect` implementations gained two bounds:
  - `Ctx::Emitter: UnclosedEmitter<'inp, L, Lang>` — **a custom emitter must now implement
    `UnclosedEmitter`** to be usable with `.delimited::<D>().collect()`;
  - `<Ctx::Emitter as Emitter<…>>::Error: From<Unclosed<(), L::Span, Lang>>` — **an error type
    used with a delimited many-builder must gain a `From<Unclosed<…>>` arm.**

  Both are source-breaking for consumers whose emitter or error types do not already satisfy the
  new bounds, hence the 0.4.0 (breaking) classification. The delimiter identity travels in the
  `Unclosed`'s name (`CowStr`); the type-level delimiter tag is the erased `()` (the builder
  reborrows the delimiter internally, so a `Delim`-parameterized bound would not unify across the
  builder's own indirection).
- The delimited **shape** parsers (`delimited`/`parens`/`braces`/`brackets`/`angles` and their
  `try_` twins) gained the same two bounds as the many-builders — `Ctx::Emitter:
  UnclosedEmitter<'inp, L, Lang>` and `<Ctx::Emitter as Emitter<…>>::Error: From<Unclosed<(),
  L::Span, Lang>>`. Source-breaking for shape-parser consumers whose emitter or error types do
  not already satisfy them, on the same footing as the many-builder change above.

### Migration

- Add a `From<Unclosed<…>>` arm to any error type used with `.delimited::<D>().collect()`, e.g.
  `impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for MyError { … }`. See the `json`
  example's `JsonError::Unclosed` arm for a worked pattern.
- If you use a custom emitter (not `Fatal`/`Verbose`/`Silent`/`Ignored`), implement
  `UnclosedEmitter` for it, mirroring your `FullContainerEmitter` impl: a fail-fast emitter
  converts the `Unclosed` to `Err` via `From`; a recovering emitter records it on its diagnostic
  log and returns `Ok(())`; a dropping emitter returns `Ok(())`.
- The same two migration steps apply to the delimited **shape** parsers (`delimited`/`parens`/
  `braces`/`brackets`/`angles` and their `try_` twins): add a `From<Unclosed<…>>` arm to the
  error type, and implement `UnclosedEmitter` for a custom emitter.
