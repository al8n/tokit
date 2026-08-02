# Changelog

All notable changes to this crate are documented here. The project follows semantic
versioning; before 1.0, a minor bump (0.x → 0.(x+1)) signals a breaking change.

<!--
Cross-references inside this file NEVER use a bare GitHub heading slug. Six headings here
are spelled `### Changed (breaking)` and seven `### Added`; GitHub numbers duplicate slugs in
document order, so `#changed-breaking` means "whichever one comes first today" and prepending
a section silently re-points every link that used it. It has happened: 53 references into
0.8.0 came to point at a one-item stub the moment `## Unreleased` was opened above them.

So a link target is an explicit anchor, declared on its own line one blank line above the
heading it names, and named `<section-key>-<heading-slug>`:

    <a id="0.8.0-changed-breaking"></a>

    ### Changed (breaking)

and referenced as `[16](#0.8.0-changed-breaking)`. The blank line is required. CommonMark
reads `<a id="…"></a>` as a paragraph of inline HTML, and a renderer that instead reads a
lone tag on its own line as an HTML *block* would swallow the line below it — the blank line
is correct under both readings, and it gives the gate one deterministic place to look.

When `## Unreleased` becomes `## 0.9.0 (date)`, its anchors and the links to them change key
with it. `ci/changelog_structure.sh` enforces every clause above and will red until they do.
-->

## Unreleased

### Changed (breaking)

1. **A gap is now tiled where it opens — in the node of the token it trails — not where it is
   noticed.** Where an uncovered run of source bytes lands no longer depends on what happens
   after the run is already determined. A run used to be tiled at the token that *revealed* it,
   or, for the **trailing** run, at the end of the walk; both are moments at which the parse may
   have left the node it was in when it stopped covering the source. So the same garbage produced
   two tree shapes, chosen by whether more input happened to follow: for a lossless dialect the
   tail landed *outside* the document node while identical garbage mid-document landed inside it.

   The rule now: an uncovered run opens the instant the token before it settles, and it is tiled
   there, in the node open at that moment. It is in the tree before the next event is read, so
   nothing that follows can move it. One clause covers the run that **no token precedes** — a
   source starting with bytes no token claims — which tiles where the walk first sees it: at the
   first committed token, or, if the parse committed no token at all, at the end of the walk in
   whatever node is open there.

   `Root[Document[Tok] Gap]` is now `Root[Document[Tok Gap]]`, and the node **widens over** the
   run it takes — a `Document@0..11` before a four-byte tail is `Document@0..15` after. A run
   nested deeper goes deeper: it lands at the depth of the token it trails, not one level below
   the root by fiat.

   Two placement laws hold by construction and are pinned over a corpus rather than as examples.
   *Appending a token never moves a gap*: two streams sharing a prefix through the token a run
   trails place that run in the same node, including when one of them stops there. *Hoisting a
   lexer error never moves a gap*: placement reads the token and structure events only. The
   second matters because a prefilled lookahead cache emits the lexer errors it crosses when it
   crosses them, so prefetching moves such a diagnostic earlier in the event stream — the token
   stream is exactly invariant under prefill and the diagnostic stream is not, and a rule reading
   a diagnostic's position would make the tree a function of how far the caller peeked.

   Unchanged: a **leading** run still tiles at the first token that follows it, inside whatever
   node that token lands in; a run trailing a root-level token stays a root child; a source with
   **nothing lexable in it** keeps its run beside the document node (`Root[Document@0..0,
   Gap@0..len]`), because there is no token for it to trail and the identical parse with one
   lexable byte appended puts it at the root too; and
   [`finish_partial`](https://docs.rs/tokora/latest/tokora/cst/struct.Sink.html#method.finish_partial)
   on an **unbalanced** stream still tiles a token-less run into the innermost *open* node. On a
   balanced stream both doors agree, as they did before.

   `tree.text() == source` is untouched: every uncovered byte still tiles, byte-exactly, under
   every one of these placements. Only the shape moved.

   **Re-bless any snapshot of a tree built over a source with an uncovered run that follows a
   committed token** — a truncated parse, an unterminated string or block string, a stray
   unlexable punctuator after real input. Consumers that walk by text see no change; consumers
   that walk by node, or that assert on a rendered tree, see the run inside the node of the token
   before it. A source with *no* committed token at all is not affected.

2. **A recursion-limit trip is now terminal for every grammar error type, `()` included — neither
   recovery nor a collection driver can spend it.** This closes 0.8.0's [known limitation — a discarding error sink erases
   the recursion trip's stop, not its
   bound](#0.8.0-known-limitation--a-discarding-error-sink-erases-the-recursion-trips-stop-not-its-bound),
   which recorded the behaviour rather than answering it. The answer is that a resource bound an
   unrelated error sink can opt out of is not a bound.

   [`RecursionLimitReached`](https://docs.rs/tokora/latest/tokora/error/struct.RecursionLimitReached.html)
   has always been terminal for every value, but
   [`recover`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.recover),
   [`inplace_recover`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.inplace_recover)
   and
   [`skip_then_retry`](https://docs.rs/tokora/latest/tokora/trait.ParseInput.html#method.skip_then_retry)
   read that off the **converted** value — after the grammar's `From` had run. A `From` that
   discards the payload discards the marker with it, so a `()`-errored grammar got
   `is_terminal() == false` back and recovery **spent** the trip.

   **Where resource terminality is stored now:** on the **input session**, in a monotone counter
   (`Input::resource_trips`, crate-internal, bumped by
   [`InputRef::descend`](https://docs.rs/tokora/latest/tokora/struct.InputRef.html#method.descend)'s
   trip arm before the grammar's `From` runs, so a panicking conversion cannot skip it). The three
   combinators read it **beside** `MaybeTerminal::is_terminal`. No conversion the grammar writes
   can reach it, and nothing lowers it: a `Checkpoint` does not carry it and a restore does not
   touch it. Grammar error conversion therefore cannot affect it — which is the question 0.8.0's
   entry left open, answered here in the terms it asked for.

   **What the cell records and what the combinators test are different questions, deliberately.**
   The cell is a session fact — *this parse exceeded a budget, this many times* — and it is never
   cleared, because nothing can un-exceed a budget. Every site that consults it is judging **one
   attempt**, so it snapshots the counter before that attempt and re-raises only when the count
   moved *during* it. The two answers come apart exactly where grammar code catches a trip itself
   and parses on: the session has tripped forever after, while the next attempt is an ordinary one.
   Reading the session fact there would refuse recovery — and, below, emit-and-continue — for every
   later failure in the document, ordinary syntax errors included. A real trip is still re-raised
   wherever one happens, a second trip after a caught first one included.

   **What changes for you, and only if your error type discards the value:**

   | your grammar's error type | before | now |
   |---|---|---|
   | stores it and delegates `is_terminal` | re-raised | re-raised — **unchanged** |
   | `()`, or any `From` that drops it | recovery ran; `skip_then_retry` skipped and committed | re-raised |

   Concretely, measured on one ladder and one 32-deep input: `.recover(..)` used to return
   `Ok(<the recoverer's synthesized value>)` and now returns `Err`; `.skip_then_retry(..)` used to
   hand the surrounding grammar back offset **68** — the whole chain and the sync token consumed
   and committed — and now hands back **0**, which is the number a delegating error type always
   read. Same ladder, same limit, two error types, one answer.

   **If you relied on the old behaviour**, the fix is the budget, not the sink: raise it with
   [`with_recursion_limiter`](https://docs.rs/tokora/latest/tokora/struct.ParserContext.html#method.with_recursion_limiter).
   Recovering from an exhausted depth budget was never sound — no quantity of skipped input makes
   the next descent shallower, so those cycles spent input for a verdict they could not change. If
   you want the trip's *details* (offset, depth, limitation), that still requires an error type
   that stores the value; the discarding sink loses the payload exactly as before.

   **The same stop now also reaches the resilient collection loops — and that half was never
   sink-dependent.** `repeated`, `separated` and their delimited forms swallow an element's `Err`
   by design: emit it as a diagnostic and keep looping. Their gate re-raised the frontier
   `Incomplete` and the *scanner*'s committed boundary, and a descent trip latches neither — it has
   a control stack rather than a position, so there is no boundary for it to latch. A trip inside
   an element was therefore filed as an ordinary diagnostic and the loop went on to the next one,
   bounded only by the no-progress guard. Unlike the recovery half above, that happened for **every**
   error type: a delegating one was spent there exactly as `()` was. The gate reads the session
   counter too now — and, like the recovery half, it reads it against a baseline taken **once per
   element**, so what re-raises is a trip *that element* caused.

   | driving an element that trips the depth budget | before | now |
   |---|---|---|
   | `repeated()`, `separated_by(..)`, and both delimited forms | the trip filed as a diagnostic; the loop continued to the next element and returned `Ok` with whatever it collected | the collection returns `Err` at the first trip, with **nothing filed** |
   | the same, for an element that fails **ordinarily** after some earlier construct's trip was caught and parsed past | the failure filed as a diagnostic; the loop continued | unchanged — the failure filed as a diagnostic; the loop continues |

   **This half changes behaviour for delegating and discarding error types alike**, so a grammar
   that used to receive a truncated-with-diagnostics container over a too-deep input now receives a
   failed parse. The remedy is the same one as above and for the same reason: raise the budget with
   `with_recursion_limiter`. A container assembled from elements the budget forbade reading was
   never a description of the input, and the diagnostic that named the trip was filed against a
   construct the parse had already been told to stop reading.

   **The scope of the change, stated rather than left to be found: it is the element's own trip
   that ends the collection, not the session's.** A parse that catches a trip and goes on keeps
   emit-and-continue recovery for the constructs after it — which is what an editor or language
   server needs, since one deeply-nested expression must not suppress the rest of the file's
   diagnostics. The same holds one nesting level up: an inner collection's trip that an element
   swallows is not charged to the enclosing collection's next ordinary failure.

   **The granularity floor, because "the constructs after it" has a resolution.** The witness is a
   counter, and a counter proves that *a* trip happened during the unit being judged — not that the
   error being judged *is* that trip. The unit is one attempt for `recover` and `inplace_recover`,
   one retry cycle for `skip_then_retry`, and one **element** for the collections. So grammar code
   that catches a trip itself and then fails **ordinarily inside that same unit** has the ordinary
   failure re-raised rather than recovered or filed. Move the catch one construct further out and
   it recovers, or is filed and looped past, exactly as an untripped parse would; that contrast is
   pinned by a test on each side of it.

   The floor **fails closed**: a real trip is never recovered from and never filed as a diagnostic,
   and the only over-charged case is an ordinary failure that shares its unit with a caught trip.
   It cannot be lowered by reading the error, because the error type is allowed to be `()` and
   discard the trip — which is why the witness is on the input in the first place. Lowering it
   needs a cooperative *rebaseline* published for code that deliberately catches a trip and wants
   the enclosing baseline moved past it. That is not in this release; no public name changes if it
   is ever added.

   — *(#148 R1)*

### Source-breaking additions that can change behaviour with *no diagnostic at the call site*

**This release adds no public names.** The one entry here is a disclosure about a name **0.8.0**
added, measured for the first time now, and it is filed here rather than folded into the shipped
0.8.0 section because it is news a 0.7.3 consumer needs and 0.8.0's own text does not carry it.

#### `RecursionLimiter::unlimited` — measured SILENT, and it is your call site to check

**You are exposed if you wrote an `unlimited()` associated function on `RecursionLimiter`.** That
type is public in **0.7.3**, so this is not hypothetical: any consumer holding one already had
somewhere to hang the name, and 0.8.0's
[`RecursionLimiter::unlimited`](https://docs.rs/tokora/latest/tokora/state/recursion_tracker/struct.RecursionLimiter.html#method.unlimited)
now competes for it by path.

Reproduced two-sided by `ci/name_collision/`, base `60f27a3` against this branch:

```text
SILENT  unlimited/discarded    base=witness=1  head=witness=0
```

Both sides compile, **neither emits any diagnostic**, and the two run different functions: yours
on 0.7.3, tokora's on 0.8.0 and later. `unlimited/used` is `loud` on the same probe, so a
discarded return is the whole of the difference — `RecursionLimiter` carries no `#[must_use]`, and
`unused_must_use` does not fire on a plain struct. **The remedy is UFCS**: `MyTrait::unlimited(..)`
or `<RecursionLimiter as MyExt>::unlimited()` pins your function by name and is immune to this.

`#[must_use]` was considered and rejected as the fix: it would not reach the case that matters —
a consumer who *assigns* the result, which the harness cannot generate and which stays silent
regardless — while firing on legitimate discards. Disclosure is the honest instrument here.

Two things about *why this arrives late*, both of which are the point rather than an excuse:

- 0.8.0's exposure table already lists `unlimited` under "you wrote it on `RecursionLimiter`", so
  the *name* was disclosed. What was not disclosed is that this one is **measured silent**, in a
  category the harness's README says a silent row "would be news" in — the first
  `inherent_assoc_fn` row ever to score one. The probe could not construct it at the time: #147
  introduced the owner and `gen_probe.py` had no template for it, so every row on that owner came
  back `FATAL` — an *incomplete* verdict, which is not a clean one. #148's Stage A added the
  templates and the `new-owner` verdict, and the finding fell out immediately.
- It is disclosed on **this** branch and could not wait. The probe's inventory is a two-sided
  delta: once this merges, `unlimited` exists on both sides, the row leaves every future plan, and
  the harness can never re-litigate it. A green run after that would mean "not probed", not "not
  colliding".

Recorded in `ci/name_collision/disclosed.txt`, whose fourth-and-fifth-row split now states plainly
that the earlier four ride a bounded receiver and **this one does not**: `RecursionLimiter` is a
concrete public struct with no bound to reject anybody.

### Performance

3. **`Sink::finish` replays the event log once instead of twice.** Materialization used to make
   a full gather pass over every event before the walk that drives the builder: it validated
   kinds and retro-wrap targets, collected the recorded lexer-error spans, and read the
   uncovered runs off the token spans so the walk could tile a run at the token it trails. All
   three now happen inside the walk.

   The canaries move to the arm of the event they were already about. The error spans are still
   merged into a **set** and the coverage verdict is still order-independent — it is simply
   decided after the walk instead of before it, which it can be because
   [`UncoveredGap`](https://docs.rs/tokora/latest/tokora/cst/enum.FinishError.html) was only
   ever consumed at the end. The run lookahead is now a *monotone cursor*: a run's tile is armed
   at the token that opens it and forced at the next builder-visible event, which resolves its
   far end by scanning forward to the next token — never rescanning, and skipping outright every
   region whose run the following token resolves for free.

   Measured on a 57.7 KB GraphQL document (64,085 events): **1,727 µs → 1,663 µs, −64 µs**,
   with the produced tree byte-identical across the clean, perturbed, hand-broken and
   every-prefix corpora.

   **One behaviour changes, and only on a malformed stream: error precedence.** Two passes meant
   every gather-class refusal (`ReservedKind`, `StaleStartAt`, `DanglingForwardParent`,
   `InvalidDiagnosticSpan`, `InvalidDialectKind`) outranked every walk-class refusal
   (`OrphanFinish`, `ImproperWrap`, `MismatchedFinish`, `OverlappingSpans`, `SpanOutOfBounds`,
   `OffsetOverflow`) whatever their buffer indices. One pass reports the **first violation in
   buffer order**. No stream that was refused is now accepted, and none that was accepted is now
   refused; a stream with two defects may name the other one. Within a single event the order is
   unchanged — each fused arm runs its gather checks ahead of its walk checks.

4. **A `Sink` reserves its event log at construction, from the source's length.** The log used
   to grow from empty, doubling about sixteen times over a 57.7 KB document and copying roughly
   4 MiB in the process. `Sink::new` now asks for the capacity that doubling would have arrived
   at — the source's byte length rounded up to a power of two, capped at 65,536 events (2 MiB) —
   so the same final block is bought once.

   The predictor is sound only because a sink is compile-time restricted to trivia-surfacing
   lexers: every source byte reaches it as a token or a reported lexer error, so the event count
   tracks the byte count (0.80–1.11 events per byte across this crate's corpora). The **cap** is
   what keeps that from being a liability for a grammar whose tokens are long — past it the byte
   count stops being evidence and the `Vec` resumes doubling — and the **rounding** is what keeps
   the reservation from backfiring: reserving the raw length under-reserves a lossless log, which
   buys a large eager allocation *and* a double-sized reallocation on top, and measured slower
   than reserving nothing at all. An empty source still allocates nothing.

   Measured on the same 57.7 KB document, paired over fourteen rounds: **−5 µs (σ 8)**. The
   allocation count and the ~4 MiB of copying go regardless of what the wall clock on one
   allocator says.


5. **A `trace`-feature preview no longer costs the entire remaining source, every time.**
 `InputRef`'s per-event source preview (`trace_preview`, behind the `trace` feature) built
 `format!("{rest:?}")` over the *whole* remaining source — potentially the entire input, at
 every instrumented combinator call (`peek`, `begin`/`commit`/`rollback`, `try_expect`,
 several per token) — then walked that string a second time to count it, just to keep the
 first 24 characters. Θ(remaining input) per event, unconditionally, made Θ(n²) over a parse:
 benchmarking with `--all-features` (which enables `trace`) against a ~128 KiB fixture wrote
 133 MB of stderr in 5 minutes without finishing the cheapest bench cell's warm-up.

 A bounded `fmt::Write` sink now aborts the `Debug` call as soon as it has enough output to
 answer the 24-character window and the ellipsis, so the preview never walks more of the
 remaining source than a `Debug` impl's own internal batching forces. Measured against this
 crate's own `input_scan` bench fixture shape (newline-delimited, digits and punctuation): a
 single preview at offset 0 goes from 250µs to 208ns at 128 KiB and from 10.5ms to 958ns at
 8 MiB; a full traced parse over that shape scales linearly in token count on both sides of
 the fix, at a per-token cost that no longer grows with how much input is left.

 That bound is conditional, not universal: it holds for a `Debug` impl that writes
 incrementally, and it does not hold for one that front-loads its entire output before its
 first write. Every `Source`/`Slice` pairing this crate ships was checked against that line;
 the guarantee is asserted for exactly these, not for `Source` in general.

 A third shape is not narrow at all, and is the only one of the three reachable from outside
 this crate. `Source`'s `Slice` associated type is public and implementable downstream, and
 `Slice`'s own bound (`PartialEq + Eq + Debug`) puts no streaming requirement on its `Debug`
 impl. A conforming impl may scan, escape and allocate its *entire* remaining slice into a
 temporary and call `write_str` once; this sink cannot shorten work that already ran before
 it was ever invoked, so the cost stays exactly the `O(remaining source)` this fix exists to
 remove. Output is still correct even here — the window and the ellipsis decision come from a
 genuine prefix of the untruncated dump no matter how it was produced — so this is a cost
 gap, not a correctness one, but it is the most severe of the three: unbounded, not merely
 narrower, and reachable with an ordinary trait implementation, no unsafe or misuse of this
 crate required. No in-tree backing does this today; `bounded_debug`'s doc comment and its
 `bounded_debug_is_correct_but_unbounded_for_a_front_loading_debug_impl` test carry a fixture
 that exercises exactly this shape.

 The other two remain narrower and **found, not fixed**: a `[u8]`-shaped backing (`[u8]`,
 `HipByt`, the `smol-bytes` byte types) bounds its allocation and escaping work but still
 iterates every remaining element, because `Formatter::debug_list` drives its iterator to
 completion independently of write failures; and a `str`-shaped backing (`str`, `HipStr`,
 `Utf8Bytes`) still costs `O(run)` for a run of characters with nothing escape-worthy in it,
 because `Debug for str` accumulates such a run before its first write. Realistic text —
 which breaks such runs at newlines, quotes, etc. — is unaffected, but a contrived input like
 this crate's own `int_run_source`/`comma_list_source` bench fixtures (digits and spaces for
 ~128 KiB at a stretch) is not; `bytes::Bytes` and `bstr::BStr` have neither gap, since both
 hand-write their `Debug` loop with `?` on every write.

 Output is unchanged: every trace line reads identically to before this fix, character for
 character, ellipsis included — checked against a spread of inputs (empty, short, exactly at
 the 24-character boundary, far longer, and content that escapes heavily: newlines, tabs,
 both quote characters, backslashes, non-ASCII, and a multi-byte scalar straddling the cut).

### Fixed

- **The collection gate's own census could pass by not looking.** `parser::many`'s `GATE_CENSUS`
  read four hard-coded sources and counted one exact spelling of the swallow beneath the gate —
  `emit_error(Spanned::new(span,`. A swallow spelled any other way, or written in a fifth source,
  moved neither tally, so the equality it asserted still held and the census stayed green over an
  ungated emit-and-continue site. A census quoted as evidence that answers by not looking is worse
  than none.

  The swallow is now a single chokepoint — one `emit_error` call in the whole `many`/`fold` tree,
  with the three never-recoverable witnesses above it — so a driver physically has nothing to spell
  differently. The census matches the **call** rather than its arguments and asserts the count is
  zero in every driver source; it scans the chokepoint's own body for the three witnesses ahead of
  the emission; and a third test requires every module of the driver trees to be classified, so the
  source list cannot fall behind the tree. Every scan panics when the thing it looks for is absent
  instead of finding nothing to check. Shown non-vacuous by planting a swallow in a fifth source
  with a spelling the old shape did not match — it fails, naming the file, then the plant was
  removed. No behaviour change. — *(#148, verification debt)*

  **And the same defect, one level in: the witness scan proved presence, not gating.** The scan
  above requires each of the three never-recoverable witnesses to appear once in the source ahead
  of the emission. Textual presence ahead of a call is not control-flow domination — a body that
  reads all three into `let _ =` bindings and then emits unconditionally satisfies it with the gate
  entirely gone. Compiled and run: the census stayed green.

  Answered by mutation rather than by a stronger scan, since proving domination from source text
  means writing a Rust parser inside a test module that must also build under
  `--no-default-features`. Each witness was neutered in turn and the whole `--all-features` suite
  run: the frontier-`Incomplete` witness reds 2 tests, the descent-trip witness reds 12 — and
  **`at_committed_boundary()` red nothing at all**, in the entire suite. Its only cell was negative
  (a boundary the cursor has *not* reached must not be charged to an ordinary failure), which a
  deleted witness also satisfies. Unguarded, a collection that runs onto a poison boundary files
  the stop as an ordinary syntax error and returns a **silently truncated success** — `Ok` over
  input the scanner never read.

  `tokora/tests/collection_terminal_stop.rs` gains the positive direction: five `r1b_*` cells, one
  per try-driven family plus the truncation case, each with the file's non-vacuity control (the
  same probe run twice with only the scan limit changed, required to disagree, and required to have
  actually tripped). All five are red under the mutation and green without it. The census now states
  what a needle scan proves and names the suite that proves the rest, and both it and the
  chokepoint's own docs carry the re-check procedure. No behaviour change. — *(#148, verification
  debt)*

- **The recursion-limit test suite's stack-address witness made every Miri job and the ASan job
  red, on `main`.** `pratt_limit_unit_sink`'s unwind cell corroborates its two depth-cell
  assertions out-of-band by comparing the addresses of two stack locals. That comparison measures
  frame liveness only while the addresses are native stack offsets: ASan can relocate a frame's
  locals onto a heap-allocated fake stack and Miri hands out virtual allocation addresses. Three of
  four instrumented hosts measured the *unwound* frame as farther from the baseline than the
  descent it had already left — inverted operands, so no threshold rescues the comparison — while a
  fourth (ASan on aarch64-darwin) passed on the same source. A relation whose answer changes with
  the runner is reporting the runner, so it is now scoped to native builds via `cfg!(miri)` plus a
  `TOKORA_SANITIZER` variable `ci/sanitizer.sh` exports for every leg it runs.

  The two depth-cell assertions stay **active** under Miri and every sanitizer, and the gated one
  is not replaced by a second library-side counter: reading the same cell twice is not
  corroboration. A skipped assertion announces itself on the process's real stderr — not through
  `eprintln!`, which libtest swallows for a passing test — so the one build where the check does
  not run is not the one build where nobody can see that. — *(#148, verification debt)*

- **The name-collision gate reported an incomplete verdict as if it were a result.**
  `gen_probe.py` had no template for the owners #147 introduced, so every row on them came back
  `FATAL`. Templates alone were not enough: a probe naming an owner the same diff introduces cannot
  compile against the base ref, and base-no-compile was unconditionally `INCONCL`. A new
  `new-owner` verdict says exactly that, and carries two witnesses so it cannot be manufactured —
  rustc must name the owner as unresolved on base and must not on head. The verdict is complete
  now: 27/27 rows probed, 0 FATAL, 0 INCONCL, 0 UNPROBED. — *(#148, verification debt)*

- **That verdict proved base was broken and never checked that head ran.** Its two witnesses
  required rustc to name the owner unresolved on base and to stay silent about it on head — but
  silence is also what a head build broken for an unrelated reason looks like: `no-compile`,
  `upstream-fail`, `bad-witness(...)` and `unreached` none say the owner is unresolved either, so a
  head that failed to compile the probe for a reason having nothing to do with the owner — or that
  never reached the call at all — still read as proof the owner is new. `new-owner` now
  additionally requires head to show a completed run, checked by an allowlist of the one shape
  that is evidence (`witness=*`) rather than a denylist of the shapes already known to be broken,
  which is silent about the next one nobody has named yet. Re-run against a genuine pre-#147 base,
  9 of the 14 `RecursionLimitReached` rows the prior entry counted turn out to have been passing on
  exactly this hole: `map_offset` and `of`'s templates call the real method with fewer arguments
  than it takes, and the other five methods' `used` spelling forces a `u8` return type none of them
  have, so head never compiled on any of the nine. They now score `INCONCL`, correctly — only the
  five methods whose `discarded` spelling silently and successfully calls the real inherent method
  were ever provable, and the fix does not widen to paper over the rest. — *(#148, verification
  debt)*

- **That fix's own allowlist was still too wide, and a sibling verdict had none at all.**
  `new-owner`'s `case "$h" in witness=*)` accepts `witness=1` exactly as readily as `witness=0` —
  but the marker is CONSUMER-CALLS, attribution rather than existence (see the comment where it is
  assigned), and `witness=1` means the CONSUMER's own extension item took the call, i.e. the
  probed name on the new owner was never a candidate this run. A template that compiles clean
  while constructing no real collision would still have scored `new-owner`. The allowlist now
  requires `witness=0*` specifically — the one shape that says tokora's item won the resolution —
  and a `witness=1` head reads `INCONCL`, naming the reason.

  Separately, the `glob-err`/`glob-ok` verdicts intercepted only the LITERAL STRING `no-compile` on
  the base side before trusting head's evidence. A base broken for any other reason —
  `upstream-fail` chief among them — fell through into the head-only checks and let a head-side
  ambiguity decide the verdict alone, with no valid before-state at all. The base side is now
  checked against an allowlist too: `no-compile` or `unreached` (the only two shapes a
  compiling-or-rejected glob probe can produce, since `glob_name`/`glob_macro`'s `drive()` calls
  only `ran()` and never `reached()`), anything else `INCONCL`.

  Both proved in the failing direction, against a real glob collision and a real `new-owner` row,
  then reverted — `git diff` confirmed clean before this fix was committed. A forced
  `base=upstream-fail` alongside a genuine head-side ambiguity moved the glob row from `glob-err`
  to `INCONCL`; a forced `witness=1` (explicit UFCS on the consumer trait, bypassing inherent-method
  resolution) on a real `new-owner` row moved it from `new-owner` to `INCONCL`. Re-run against the
  same pre-#147 base as the prior entry, the tally is unchanged — 5 `new-owner`, 9 `INCONCL` —
  because none of the 14 real rows there naturally produce `witness=1`; only the forced case
  exercises the new branch. — *(#148, verification debt)*

- **That base-side allowlist had a mirror-image hole on the head side, and it was the more
  exploitable of the two.** After the fix above, `glob-ok`'s only remaining check was
  `elif [ -n "$said" ]` — accept once rustc says *something* ambiguity-shaped anywhere in the
  head log, with no check on `$h` itself. `upstream-fail`, `bad-witness(...)` and any
  `witness=*` all read as a pass exactly as readily as a genuine `unreached`, so a head build
  broken for a reason having nothing to do with the probed name — provided an attributed
  ambiguity diagnostic happened to be sitting in the same log — still scored `glob-ok`. That is
  acceptance on the absence of a completed head-side probe: the same defect class the base-side
  fix above had just closed, on the other side of the same row.

  `h` is now held to the identical two-shape allowlist as `b`: `no-compile` or `unreached`,
  because `glob_name`/`glob_macro`'s `drive()` calls only `ran()` and never `reached()` — a head
  that actually finishes compiling and running can only ever read `unreached` here, never
  `witness=*`. `glob-ok` now requires `h = unreached` specifically; `upstream-fail`,
  `bad-witness(...)` and any `witness=*` read `INCONCL`, naming the reason, instead of falling
  through.

  Proved in the failing direction against the same pre-#147 base and the same
  `RecursionLimitReached` glob row as the prior entry: a forced `head=upstream-fail` alongside
  the row's genuine, untouched ambiguity diagnostic moved it from `glob-ok` to `INCONCL`; forcing
  `bad-witness(calls=2,reached=0)`, `witness=0` and `witness=1` the same way each land `INCONCL`
  too. Reverted afterward; `git diff` confirmed clean before this fix was committed. Re-run
  unperturbed, the same row reaches its natural `glob-err` verdict, unchanged from before this
  fix.

  Audited the rest of the file for the same asymmetry — a verdict proving one side reached a
  conclusion without proving the other did. `new-owner`'s two witnesses already check
  `base_unresolved` and `head_unresolved` by the same predicate, and the item-row ladder's
  `unreached`/`upstream-fail`/`bad-witness` filter matches against `"$b$h"` concatenated, which
  catches either side by construction; the base-only `case "$b" in witness=1*|no-compile)`
  further down is not a shape gap of this kind; it fixes the pre-release baseline's expected
  value, and every value `$h` can still hold past it is already handled by name in the branches
  below. This glob-row pair was the only place in the file carrying an allowlist on one side and
  none on the other. — *(#148, verification debt)*



6. **A `trace`-feature preview no longer reports a partial `Debug` rendering as the complete,
 short value it is not.** `bounded_debug` — the bounded sink `trace_preview` (behind the
 `trace` feature) uses to build its per-event source preview — discarded the `Result` from its
 own `write!` call outright, so its truncation flag came from the sink's private bookkeeping
 alone: `true` only when the sink itself had refused a write after filling its 24-character
 window. A `Debug` impl can fail for a reason that has nothing to do with the sink —
 `fmt::Result` is a plain `Result`, and neither the trait nor `Slice`'s bound (`PartialEq + Eq
 + Debug`) forbids it — and that case left the flag `false`: the preview then rendered
 whatever partial content had been written, with no ellipsis, as though it were the whole
 value.

 The single flag is replaced with a three-state outcome — `Complete`, `WindowTruncated`, or
 `FormatFailed` — because a bool cannot carry both facts a reader needs kept apart: whether
 there is more of the value than the window shows, and whether the value's own `Debug` impl
 finished at all. Folding a foreign `Err` into the same flag a real truncation sets is just as
 wrong in the other direction: a `Debug` impl can write its complete, short rendering and only
 then fail for a reason of its own, and reporting that as truncated appends `…` to a trace
 line for a value that has no more content, claiming a continuation that does not exist.

 The sink's own bookkeeping (`sink.truncated`) is what tells the two causes apart, and does so
 reliably: the sink has exactly one branch that ever returns `Err`, and that branch sets
 `sink.truncated` in the same statement, so a `write!` failure the sink did not record cannot
 be the sink's. `trace_preview` now renders each state distinctly — no marker for `Complete`,
 `…` for `WindowTruncated`, and a separate `<fmt error>` marker for `FormatFailed` that says
 the renderer broke without claiming missing content. This is a correctness fix, not a
 performance one: it changes output only for a `Debug` impl that fails on its own account,
 which no in-tree `Source`/`Slice` pairing does, so every case that already reached the sink
 normally — which is to say, every case this crate has ever exercised — renders
 byte-identically to before.


## 0.8.0 (2026-07-31)

The whole of a 52-defect audit campaign lands in one release. Entries are grouped by **kind**, not by the round that produced them: a reader upgrading wants every breaking change in one place. Round provenance rides as an inline tag — *(R7, #117)* — and the pull-request bodies carry the full trail.

### Upgrading from 0.7.3 — the migration table

Every break a 0.7.3 consumer can hit, in one place. **Rows are ordered by how the break
announces itself, not by how many consumers hit it** — the ones your build will *not* point at
come first, because those are the ones you have to go looking for. Each row links to the
numbered entry below that carries the full reasoning.

#### These do not fail to compile. Read them first.

| Old behaviour | New behaviour | Why, in one line | Item |
|---|---|---|---|
| A `SimpleSpan` const mutator silently published an inverted or wrapped span — `with_end_const(2)` on `(5, 15)` gave `(5, 2)`; `bump_start_const` at `(MAX-1, MAX)` gave `(0, usize::MAX)` in release | panics: `end must be greater than or equal to start` / `span bump overflows usize` | The span was **already corrupt** and the program was carrying it. The panic is the bug becoming visible, not a new refusal. | [36](#0.8.0-changed-breaking) |
| A properly closed `separated_by_*().delimited()` list ignored `at_least` / `at_most` / `bounded` and the separator policy | the policy runs: a recovering emitter records the diagnostic, a fail-fast one returns `Err` where it returned `Ok` | The end-state pass was dead code on that exit only. | [1](#0.8.0-changed-breaking) |
| `TooMany` carried the running count, or the limit itself (`found 2 … exceeds … 2`) | `limit() + 1`, emitted once per construct | Payloads that contradicted themselves. | [2](#0.8.0-changed-breaking) |
| `FullContainer` re-emitted per dropped element, counting past its own capacity | once per construct, on the whole-construct span | | [3](#0.8.0-changed-breaking) |
| Bounded containers turned a satisfied `at_least` into `TooFew`, and swallowed a violated `at_most` | bounds judge the elements **parsed**, not the ones stored | Only bounded-capacity containers were affected. | [4](#0.8.0-changed-breaking) |
| `SeparatorHandler::on_separator` received only leading and duplicate separators | receives every separator in source order | It received the exact complement of its documented contract. Opt out with `OBSERVES_SEPARATORS`. | [5](#0.8.0-changed-breaking) |
| Delimited separated drivers returned a span measured from the first element on some exits | the whole construct's span, opener through closer, on every success exit | | [6](#0.8.0-changed-breaking) |
| A dialect mapper returning an out-of-language kind reached `finish` as `Err(InvalidDialectKind)` | panics at the emit site, in every build | It is a dialect bug, not an input condition; no parse input can provoke it. | [14](#0.8.0-changed-breaking) |
| A panic in caller code during `skip_until` lost the in-flight token and the skipped prefix | the stream is left consistent on the panic path too | Only observable to a host that catches a panic across a parse and keeps using the input. | [9](#0.8.0-changed-breaking) |
| `Emitter::commit_token`'s observer ran before the committed position was published | runs **after** the position is published, before the replaced pair is dropped | An observer that panics no longer leaves the input naming a token the stream does not hold. | [10](#0.8.0-changed-breaking) |
| An `Incomplete` scan exit committed the frontier it reached | exits as it entered | A refill driver resumed at a half-consumed position. | [12](#0.8.0-changed-breaking) |
| A terminal scanner stop (resource limit, poison boundary) reached callers as an ordinary end-of-input **decline** | surfaces as a terminal error **when your emitter accepts the diagnostic**, and recovery re-raises it instead of retrying. A *rejecting* emitter's `Err` is built from your lexer's error value and carries no mark, so recovery can still spend that trip — `MaybeTerminal`'s doc has that path and the arm it needs | Recovery was retrying against a scanner that had already given up. | [16](#0.8.0-changed-breaking) |
| Collection drivers could stall on zero progress, or mask a terminal stop as a successful end | both exits are errors | | [17](#0.8.0-changed-breaking) |
| `…, expected expected '}'` in `MissingToken` and `UnexpectedToken` renders | `…, expected '}'` | `Expected`'s own `Display` already supplies the word. **Re-bless frozen renders.** | [37](#0.8.0-changed-breaking) |
| `UnexpectedEnd`'s derived `Debug` / `Eq` / `Hash` | include a `terminal: bool` field | Shipped four rounds ago and disclosed nowhere until now. **Re-bless frozen renders.** | [16](#0.8.0-changed-breaking) |
| `Unclosed`'s derived `Debug`; `SeparatedError` / `MissingToken` derived `Debug`, `Eq`, `Hash` | include `kind` / the name channel | See *Debug and rendered output* below for the complete list. | [20](#0.8.0-changed-breaking), [28](#0.8.0-changed-breaking) |
| A slice-choice id out of range panicked with `index out of bounds` | `choice id {id} out of bounds for {len} branches` | Message text only; reachability is unchanged. | [35](#0.8.0-changed-breaking) |
| A wrong opening delimiter produced **two** diagnostics naming the same token — once as the wrong opener, once again as a close miss | exactly one | Only under a recording emitter, and only while the first report is still live in the log. Assertions that counted diagnostics on the recovering path change. | [40](#0.8.0-changed-breaking) |
| A second same-power `PrattInfix::Neither` operator in one chain folded left in silence — `7 = 1 ; 2 ; 3` returned `Ok(((7=(1;2));3))` with the **whole input consumed**, so no end-of-input check had anything to catch | the parse fails with `NonAssociativeChain`, the operator left on the input unconsumed | Handing the operator up re-associates the chain across an enclosing frame that cannot know the constraint. Not terminal: recovery may still spend it. | [41](#0.8.0-changed-breaking) |
| Recursive descent was unbounded — a deep enough expression exhausted the native stack and **aborted the process** | a shared per-input depth budget, **64** by default, failing the parse with the always-terminal `RecursionLimitReached` | An abort carries no diagnostic and cannot be caught; a refusal names the knob that raises it. `RecursionLimiter::unlimited()` restores 0.7.3's behaviour. | [42](#0.8.0-changed-breaking) |
| `RecursionLimiter::new` / `Default`, and `Limiter::new` / `Limiter::with_token_tracker`, defaulted to depth **500** | **64** | Reaches code with no pratt parser in it: the same type doubles as a **lexer-side** nesting tracker in a `State` / `Extras` position. | [43](#0.8.0-changed-breaking) |
| Your own `peek_kind` / `labelled` / `opt` / … resolved to *your* method | may resolve to tokora's, with **no diagnostic at the call site** | 16 new inherent items and 6 new defaulted trait methods enter the method space. Whether yours still wins is decided by rustc's **pick order**, not by inherent-ness. | [source-breaking additions that can change behaviour with *no diagnostic at the call site*](#0.8.0-source-breaking-additions-that-can-change-behaviour-with-no-diagnostic-at-the-call-site) |
| — | a new `warning: unused import` naming one of *your* combinator traits | That warning is the **only** breadcrumb the silent case gives you: the steal stranded the import. | [source-breaking additions that can change behaviour with *no diagnostic at the call site*](#0.8.0-source-breaking-additions-that-can-change-behaviour-with-no-diagnostic-at-the-call-site) |

#### These fail to compile. Your build will point at every one.

| Old spelling | New spelling | Why, in one line | Item |
|---|---|---|---|
| `ParseCtx` | `ComposableParseContext` | Renamed and completed with `FromTokenErrors`, so one bound elaborates to the whole leaf surface. | [27](#0.8.0-changed-breaking) |
| `X::parse_of`, `X::try_parse_of`, `list_of`, … (167 items) | `X::parse`, `X::try_parse`, `list`, … | `Lang` is inferred from the input. A call site that spelled its generics gains one argument in last position; `_` is almost always enough. | [29](#0.8.0-changed-breaking) |
| `Parser<…, Error, …>` with eight constructors | `Parser` without the `Error` phantom; four constructors | | [32](#0.8.0-changed-breaking) |
| `E: From<Unclosed<Paren, …>> + From<Unclosed<Brace, …>> + …` | `E: FromUnclosed<'inp, L, Lang>` | One bound covers every pair. A residual per-delimiter bound now fails as a bare `E0277` — `From` is foreign and cannot be annotated. | [28](#0.8.0-changed-breaking) |
| `match err.name_ref() { "[]" => … }` | `match err.kind() { DelimiterKind::Bracket { .. } => … }` | A display name was never a dispatch key: `"[]"` is correct for *any* bracket-shaped pair. Custom pairs declare `DelimiterKind::Custom`. | [28](#0.8.0-changed-breaking) |
| `impl Delimiter for MyPair` | `const KIND: DelimiterKind = DelimiterKind::Custom("…")` required | No default: a defaulted identity is one the author never chose. | [28](#0.8.0-changed-breaking) |
| `Unclosed::new(span, name)` | `Unclosed::new(span, kind, name)` | Or reach for `Unclosed::paren` and siblings. | [28](#0.8.0-changed-breaking) |
| `Paren<(), (), LangA>` satisfying `Delimiter<'_, L, LangB>` | marker brand **is** the context language | A pair or separator copy-pasted out of a sibling dialect used to type-check silently. | [30](#0.8.0-changed-breaking), [33](#0.8.0-changed-breaking) |
| `UnclosedParen<S, Lang>` = `Unclosed<Paren<(),(),()>, S, Lang>` | `Unclosed<Paren<(),(),Lang>, S, Lang>` | Unchanged at `Lang = ()`. | [33](#0.8.0-changed-breaking) |
| `T::Kind: 'static` restated at five projections | hoisted to the `Token` trait | | [31](#0.8.0-changed-breaking) |
| `match rhs { … }` over `PrattRHS` | add an `End` arm | Deliberately not `#[non_exhaustive]`: a wildcard arm is the silent end-swallowing this removes. | [21](#0.8.0-changed-breaking) |
| `PrattPower::next` / `prev` | removed | They were the arithmetic that made the floor bugs expressible. Breaks callers **and** implementors. | [24](#0.8.0-changed-breaking) |
| `impl Cache { fn rewind(…) }` | delete it | Rollback had two possible owners and they disagreed. No caller loses a capability. | [7](#0.8.0-changed-breaking) |
| A `Cache` whose `push_front` can panic | must not panic on the restore path | Vacuously conforming before; not now. | [8](#0.8.0-changed-breaking) |
| A `Lexer` whose `Span`/`Offset` ops can panic on the settle path | must not panic | The one window in the consume path that cannot be closed by reordering. `Clone`, not `Copy`, is still the bound. | [11](#0.8.0-changed-breaking) |
| `Sink::new(emitter, map, ERR, GAP)` then `sink.finish(ROOT, source)` | `Sink::new(source, emitter, CstProfile::new(map, ERR, GAP))` then `sink.finish(ROOT)` | The source is bound once, at construction. | [13](#0.8.0-changed-breaking) |
| `emitter.cst_finish()` | `emitter.cst_finish(KIND)` | The kind the matching `cst_start` used. It is a **defaulted** method, so only callers and overriding implementors are affected. *Found by the mechanical API diff; disclosed nowhere before.* | [15](#0.8.0-changed-breaking) |
| `Sink<'_, L, Sink<…>>` | a compile error — the inner emitter must be `ValueKeyedEmitter` | Two mark spaces claiming the same keys. | [18](#0.8.0-changed-breaking) |
| `inp.begin_point(); … inp.commit_point();` | `let p = inp.begin_point(); … inp.commit_point(p);` | Nothing tied a settle to the point it settled. Points settle newest-first; four misuse conditions panic by name. | [19](#0.8.0-changed-breaking) |
| `let (o, e, m) = missing.into_components();` | `let (o, e, m, name) = …` | And `SeparatedError`'s pair becomes a triple. `..` is not available on a tuple pattern, so the compiler points at every site. | [20](#0.8.0-changed-breaking) |
| An error type without `From<UnexpectedEot<…>>` on the peek / `Expect` / fold / `Repeated` surfaces | add it, or name `ComposableParseContext` | A terminal stop has to be expressible as an error. | [16](#0.8.0-changed-breaking), [17](#0.8.0-changed-breaking) |
| An error type without `MaybeTerminal` | `impl MaybeTerminal for MyError {}` is enough | Unless the type can itself carry a terminal stop, and **three** that this crate builds and marks can reach it: an `UnexpectedEnd` whose flag the scanner may raise; a `RecursionLimitReached` — new in this release, required of every pratt-driving error type — terminal for **every** value; and a `SessionRefusal`, terminal for every value too but *not* a `MaybeTerminal` implementor, so its arm is written `true` by hand. Answer for each one you store. An arm left at `false` is spent as a recoverable failure — except the refusal, which `PartialSession::parse` asserts on unconditionally, so that one panics a release build. Those three are what the crate knows it produces, **not** a proof that nothing else is terminal: a scanner trip a *rejecting* emitter refuses propagates as that emitter's `Err`, built from your lexer's error value and unmarked, so the arm holding your lexer error may need answering too — `MaybeTerminal`'s doc has that path and the rule for it. | [16](#0.8.0-changed-breaking), [46](#0.8.0-changed-breaking) |
| `<[P; N] as ParseChoice>::Id::new(i)` (0-based) | `::new(i + 1)` (1-based) | `RangedUsize`'s bounds are inclusive, so the old id space admitted `N` and every `[P; 0]` id panicked. Tuple choices are unaffected. | [35](#0.8.0-changed-breaking) |
| `match op { … }` over `fuzz::Op` without a wildcard | add an `IsExhausted` arm | `fuzz` feature only. *Found by the mechanical API diff; disclosed nowhere before.* | [38](#0.8.0-changed-breaking) |
| `dyn SeparatorHandler` | no longer object-safe | `OBSERVES_SEPARATORS` is an associated const. Nothing in this crate used it. | [5](#0.8.0-changed-breaking) |
| `let (e, c) = ctx.into_components();` | `let (e, c, recursion) = …` | `InputContext` carries the recursion budget now, and a decomposition that dropped it would hand the input an unconfigured one. `..` is not available on a tuple pattern, so the compiler points at every site. | [45](#0.8.0-changed-breaking) |
| An error type driving either pratt engine without `From<RecursionLimitReached<…>>` and `From<NonAssociativeChain<…>>` | add both | The engines **return** these two rather than emitting them, so the entry-point bounds ask for them. Two `From` impls; every example in this repo shows the shape. | [46](#0.8.0-changed-breaking) |
| `parser.labelled("x")` resolving to your own trait's method, that trait in scope | `E0034`, when yours sits at the **same pick** | Six new defaulted names on `ParseInput` / `TryParseInput`. The fix is UFCS; rustc prints the two suggestions. | [source-breaking additions that fail *loudly*](#0.8.0-source-breaking-additions-that-fail-loudly) |
| `use tokora::*;` — or a **module** glob such as `use tokora::error::*;` / `use tokora::input::*;` — beside another glob exporting the same name | `E0659`, or `ambiguous_glob_imports` for a *macro* name on rustc ≥ 1.95 | 16 new glob-reachable names — `select!` against `tokio::select!` is the real one. Three of them reach you only through a module glob, never the root: `RecursionLimitReached` and `NonAssociativeChain` via `tokora::error`, `Descent` via `tokora::input`. Measured on three toolchains; the remedy differs by name kind. | [source-breaking additions that fail *loudly*](#0.8.0-source-breaking-additions-that-fail-loudly) |

#### Additive — nothing to do, listed so you know it exists

`PolicyComposableEmitter` / `PolicyParseContext` (the count- and separator-policy bundle tier)
· `Emitter::bound_source` and `Source::REFERENT_IS_BYTES` (both defaulted) · `SourceIdentity`,
with `addr()` / `extent()` / `covers()` · `conformance::cache`
and `conformance::emitter` · `Dialect` · `CstText` · `PartialSession` and the streaming
lifecycle · `SeparatedError: Display` · `InputRef::is_exhausted()` · `Cache::RETAINS_FRONT` ·
`BStr` in the `Equivalent` family · `CstProfile` / `KindValidator` un-gated from `rowan` ·
`Silent`'s pratt impl loses four bounds it never used.

That list is additive **in the type system**. The names added below are not: adding a name
is a source break, and the two *Source-breaking additions* sections below say which
diagnostic you will get.

#### If you froze `Debug` or `Display` output

Every movement is listed in one place under **Debug and rendered output** below. A
private-field `Debug` delta broke a real consumer's frozen renders and cost a bisect during
this campaign, which is why that list exists at all.

#### Cross-references from earlier design documents

This release's `## 0.8.0` section was consolidated from twenty `###` headings carrying
five independently-numbered lists, so numeric cross-references written before the cut
("breaking change 3") do not resolve. The pull-request body carries the full old→new map.

### The new names, and why adding a name is still a break

This release adds 16 glob-reachable root/module names, one new public module
(`tokora::cst::kinds`, its own glob namespace), 6 trait-declared methods, and 16 inherent
items — **13 receiver methods and 3 associated functions**, which resolve by different rules
and are listed separately below for that reason. The items themselves are under
[Added](#0.8.0-added) with the rest of the release. The lists here are generated from a
rustdoc-JSON diff of the two sides at the `std,logos,trace,rowan` feature point; the six
inherent items and three glob names that items [41–46](#0.8.0-changed-breaking) add were taken from
the branch diff under the same criteria rather than from a separate hand audit.

### The rule for new names, stated as weakly as it can honestly be stated

> **Any new public name can silently re-resolve a same-named item in your code. Whether
> you are told depends on spelling details that belong to the compiler, not to this
> crate.**

Four successively more precise versions of this rule were written during development and
each was falsified — never by a wrong mechanism, always by a **spelling nobody wrote down**.
The last one shipped a trait on the strength of "this name cannot collide silently, because
its type parameter never infers"; a turbofish supplies it, and discarding the return made
the collision silent with no error and no warning. That trait is not in this release.

So the rule above is the only form of it worth relying on, and the practical advice is
**detection, not prediction**:

> **If you have a method, associated function, free function, type or macro named any of
> the names in the two sections below, check that call site after upgrading.** The names
> are the complete generated list.

Everything after this point is *what was measured*, on the toolchains named. It is useful
for debugging an actual collision. It is not a guarantee, and a spelling not listed here is
not thereby safe.

<a id="0.8.0-source-breaking-additions-that-can-change-behaviour-with-no-diagnostic-at-the-call-site"></a>

### Source-breaking additions that can change behaviour with *no diagnostic at the call site*

#### Are you exposed? This question terminates.

**You are exposed if you have written a method or associated function with one of the names
below, on one of these receiver types:**

| you wrote it on | which names |
|---|---|
| `InputRef` | `try_expect_take`, `try_expect_take_or_stop`, `peek_head_map`, `head_satisfies`, `peek_kind`, `attempt_parse`, `spanning`, `descending`, `descend`, `recursion` |
| `ParseAttempt` | `into_option` |
| `Ident` | `parse_except`, `try_parse_except` |
| `InputContext`, `ParserContext` | `with_recursion_limiter` |
| `RecursionLimiter` | `unlimited` (an associated function — see the resolution rule below) |
| any parser-shaped type — one implementing `ParseInput` or `TryParseInput` | `labelled`, `traced`, `list_until`, `separated1_by`, `peek_then_head`, `opt` |

That is a finite question you can answer about your own code, and it is the one to answer.
**Whether the compiler then tells you depends on how close your signature is to ours — so do
not rely on it telling you, and do not rely on using the return value.** The remedy is UFCS:
`MyTrait::peek_kind(inp)` pins your method by name and is immune to all of this.

**Why there is no shortlist of "the silent ones".** An earlier draft of this entry named
four, measured. It was still wrong: the measurement covered one shape of consumer. Whether a
collision goes silent depends on how close the consumer's signature is to tokora's — a
property of code that does not exist yet — so a list of silent names is not a wrong answer,
it is not a well-formed one. The table above is the question we *can* answer for you.

#### The dangerous case is the one that reads as safest

**If you wrote the helper first**, with the same name and a compatible signature — because
you needed `peek_kind` or `head_satisfies` while tokora lacked them — then tokora's method
takes the call with **no error and no warning, even when you use the return value**.
Measured, byte-identical consumer source, base against this release:

```
base  CONSUMER-CALLS: 1     head  CONSUMER-CALLS: 0     zero diagnostics on both sides
```

reproduced for `peek_kind`, `head_satisfies` and `peek_head_map` with the result bound by `?`
at the caller. **A discarded return is not the precondition; a compatible signature is
enough.**

And the inversion that matters: **the closer your helper is to ours, the quieter the
substitution — and the more likely it also changes behaviour.** `peek_kind` and
`head_satisfies` deliberately have *different halt semantics* from the `peek_one`-based
helper a consumer would have written; that difference is why they exist (see the
terminal-stop entries below). So a near-identical replacement is the most dangerous case
here, not the safest.

Nothing in this release detects this for you. `ci/name_collision/` probes a consumer whose
signature *differs* from tokora's — the loud case — and reports argument-taking names as
`loud` on the strength of its own probe's arity. That limit is stated in
`ci/name_collision/README.md`, and its pass message spells the same limits out inline — that the
inherent-method and associated-function probes take no arguments, so a `loud` verdict on an
argument-taking name in those two categories comes from the probe's own arity, while the
trait-method probes pass real arguments and their rows are genuine — so a reader of a CI log
is not told "PASS" without being told what it excludes.
Three reproductions of the case it cannot generate are checked in beside it.

#### The one sub-case that is fully characterised

A consumer whose signature is *incompatible* with tokora's, calling with the return
**discarded**. There the outcome does depend on the return type. This is a sub-case, not the
boundary — it is here because it is the part the tooling covers:

| name | discarded value | reported as |
|---|---|---|
| `peek_kind` | `Result<…>` | ``warning: unused `Result` that must be used`` |
| `list_until` | `impl FnMut(…)` | ``warning: unused implementer of `FnMut` that must be used`` |
| `opt` | `impl FnMut(…)` | ``warning: unused implementer of `FnMut` that must be used`` |
| `into_option` | `Option<…>` | **nothing** — `Option` is not `#[must_use]` as a type |
| `labelled` · `traced` · `peek_then_head` | a plain adapter struct | **nothing** |

The discriminator there is whether the `unused_must_use` lint fires: a callable return is
covered by the lint's own rule for unused function-like values, an `Option` is not covered
at all, and a plain struct is not either. It is not "returns `Result`" and not
"is `#[must_use]`".

**There is deliberately no "full silent surface" list here.** An earlier draft gave one, of
fourteen names — and it omitted `parse_except` and `try_parse_except`, which are exactly the
two the paragraph above refuses to call safe. Any such list is the well-formedness problem
again. **The names to check are the exposure table at the top of this section** — it lists every
method and associated-function name this release adds, against the receiver type each one lands
on. No count is restated here: a restated count is a second inventory that can fall behind the
first, and this one twice did. Which of those names go silent for *your* code depends on your
signatures, which is a thing only your code can answer.

What was measured, on rustc 1.87, 1.95 stable and 1.97-nightly:

- **Pick order, not inherent-ness, decides.** For `recv.m(..)` rustc walks the receiver's
  autoderef chain and, at each step, tries the receiver by value, then `&`, then `&mut`.
  Each is a *pick*. Within one pick an inherent candidate beats a trait candidate — but a
  tokora method taken **by value** sits at an earlier pick than your `&self` method on the
  same type and wins regardless, even against your *inherent* method. A method reached
  through a type parameter's bound (`fn f<T: MyTrait>(t: &T)`) buckets inherent at that
  pick, so generic code that names its combinator in a `where` clause is protected and
  concrete code is not.
- **How loud it is depends on your code's shape, not on the steal.** A fully inferable call
  site flips **silently** — compiles, no warning, runs the other method. A generic one
  leaves type variables unresolved and gets **`E0282`**. A concrete incompatible one gets
  **`E0308`**, or **`E0061`** on an arity mismatch, pointing into tokora's source. None of
  the three names the real cause.
- **A discarded return removes the last defence.** If a call ignores the result there is no
  type for the compiler to disagree about, and the steal is silent even where the same call
  with its return *used* is loud. Which names that reaches is the `unused_must_use` table
  earlier in this section: four report nothing, three report a warning, and the rest fail to compile — but
  only for a consumer whose signature is *incompatible* with ours, which is the sub-case
  that table describes.
- **The one breadcrumb.** If your method came from an extension trait you imported with a
  `use`, the steal strands that import and rustc emits **`warning: unused import`** naming
  the trait. An unused-import warning on a combinator trait after upgrading means one of
  its methods was re-resolved. If the trait is declared in the same file, you get nothing
  at all. The remedy either way is UFCS: `MyTrait::labelled(parser, "name")`.

<a id="0.8.0-source-breaking-additions-that-fail-loudly"></a>

### Source-breaking additions that fail *loudly*

- **`E0034`, "multiple applicable items in scope"**, when one of the 6 new defaulted method
  names meets a same-named method from another trait **at the same pick**, with that trait
  in scope. Blanket-ness is irrelevant — a trait with exactly one impl for one concrete
  input type collides identically. rustc prints its own disambiguation suggestions; the fix
  is UFCS.

- **Associated functions resolve by a different rule, and it is not the receiver walk.**
  `Ident::parse_except` and `Ident::try_parse_except` are inherent **associated functions**.
  A path call `Type::name(..)` has no receiver, so no autoderef and no autoref happen;
  inherent associated items simply beat trait ones. A consumer trait that supplied either
  name loses it.

  **These two are not more strongly protected than the methods above, and an earlier draft
  of this entry said they were.** It claimed every collision shape here is loud, on the
  strength of the probe — but that probe declares a zero-argument consumer against a
  two-argument function, so its `loud` verdict comes from its own arity and measures
  nothing about the name. Treat these exactly like the receiver methods: if you wrote
  `parse_except` on `Ident`, check the call site and do not rely on a diagnostic.

  What *is* established, for the narrow sub-case of an incompatible signature with the
  return **discarded**: both functions return `Result`, which is `#[must_use]`, so a
  discarded result warns.

  **Unsupported is not the same as disproven, and the difference is the whole finding.**
  Two attempts to build a silently-stealing consumer for `parse_except` produced a compile
  error instead; a different reviewer, with a different signature, produced a silent steal.
  Neither outcome settles it, because whether a collision is reported depends on how close
  the consumer's signature is to ours — and that is a property of code that does not exist
  yet. So this entry does not claim these two names are safe, and does not claim they are
  unsafe. It claims the earlier assertion of safety had nothing behind it.

- **Ambiguity between two glob imports**, for the 16 new glob-reachable names — `Pinned`,
  `pinned`, `WhileHead`, `WhileKind`, `while_head`, `while_kind`, `select`, `try_select`,
  `attempt`, `syntax_kinds` via `tokora`; `dispatch_take`, `try_dispatch_take` via
  `tokora::parser`; `kinds` via `tokora::cst`; `RecursionLimitReached` and
  `NonAssociativeChain` via `tokora::error`; and `Descent` via `tokora::input`. The new module
  `tokora::cst::kinds` opens a glob namespace of its own. `select!` against `tokio::select!` or
  `futures::select!` is the real-world instance. **The diagnostic and its remedy differ by what
  kind of name collides, and — for macros only — by toolchain:**

  | Colliding name | 1.87 | 1.95 stable | 1.97 nightly |
  |---|---|---|---|
  | item (`Pinned`, `while_head`, …) | hard `error[E0659]` | hard `error[E0659]` | hard `error[E0659]` |
  | macro (`select`, `try_select`, `attempt`, `syntax_kinds`) | hard `error[E0659]` | `ambiguous_glob_imports` **warning** | `ambiguous_glob_imports` **error** |

  `#[allow(ambiguous_glob_imports)]` **does not suppress the item rows on any toolchain**,
  and does not suppress the macro row on 1.87 either. For an item name there is exactly one
  fix: an explicit `use` naming the one you meant.

**Why the names were not changed instead.** 0.8.0 is a breaking release and these are the
right names. One name *was* removed rather than disclosed — see `pinned` under
[Added](#0.8.0-added) — because it was the only addition that planted a candidate on every
`Sized` type in your program, and the argument for accepting that was the loudness
guarantee the turbofish spelling refuted. A source census of the known consumer, run
during development, found zero declarations of any of these names and zero
`use tokora[::…]::*` glob imports. **That census is not re-runnable from this repository —
its script is not shipped here — so treat it as a development note rather than a check you
can repeat.** It also saw one consumer at one commit. The disclosure above is the control;
the census was never more than corroboration.

<a id="0.8.0-changed-breaking"></a>

### Changed (breaking)

Numbered once across the whole release. The groups are ordered by **how a break announces itself**: the ones that do not fail to compile come first, because nothing in your build will point at them.

*Repetition drivers' end-state accounting.* Six deliberate behaviour changes, all in the
repetition drivers' end-state accounting. No API is removed or renamed.

1. **A properly closed separated + delimited list now reports its count bounds and separator
   policy.** `…separated_by_*().delimited::<D>()` left its element loop through the mid-scan
   closer without running the end-state pass, so on every well-formed, properly closed list the
   whole of `at_least` / `at_most` / `bounded` and the leading/trailing separator policy was
   dead code — the sibling `separated_*_while` driver ran it on the identical path. Under a
   recovering emitter such a list now records the diagnostic it always should have; under a
   fail-fast emitter a bounds- or policy-violating closed list is now an `Err` where it used to
   be a silent `Ok`.

   — *(R7, #117)*

2. **`TooMany` is emitted once per construct, with a count that exceeds the limit it names.**
   `nums()` is now `limit() + 1` at every emission site. It used to be either the running count
   (so the same four-element history yielded a different payload from each builder) or, from
   the mid-loop hook, the limit itself — a "found 2 … exceeds … 2" that contradicted itself.
   The duplicate emission (mid-loop hook plus a delegating end check) is gone.

   — *(R7, #117)*

3. **`FullContainer` is emitted once per construct**, on the whole-construct span in every
   driver, with a `nums()` that includes the refused element. A container that refuses one push
   refuses every later one, so the old per-dropped-element re-emission produced a count that
   climbed past the capacity it named.

   — *(R7, #117)*

4. **Count bounds now judge the elements the driver parsed, not the ones the container
   stored.** In the separated drivers a bounded container used to turn a satisfied `at_least`
   into a spurious `TooFew`, and — worse — to swallow a violated `at_most` entirely by clamping
   the count the check reads. Only bounded-capacity containers are affected; with `Vec`,
   `VecDeque`, `SmallVec` and `TinyVec` the stored and parsed counts are always equal.

   — *(R7, #117)*

5. **`SeparatorHandler::on_separator` now receives every separator in source order.** It used
   to receive the exact complement of its documented contract — only the leading separator and
   the duplicates in a run, never the happy-path or trailing ones. A container that ignores
   separators can opt out with the new defaulted `SeparatorHandler::OBSERVES_SEPARATORS`
   associated const, which suppresses both the call and the clone that feeds it; every
   container in this crate does. Adding the const makes `SeparatorHandler` no longer
   object-safe — `dyn SeparatorHandler` no longer compiles. Nothing in this crate used it.

   — *(R7, #117)*

6. **The delimited separated drivers return the whole construct's span**, opener through
   closer, on every success exit. Some exits previously returned a span measured from the first
   element instead, so the returned span excluded the opener and depended on which exit the
   parse took.

   — *(R7, #117)*

*The input layer's unwind edges.* Nothing that can unwind runs between taking a token out of the
stream and recording where it went. Every scan, guard and emitter edge that previously had caller
code in that gap now settles on the panic path as well as the return path, and the rollback
protocol has one owner instead of two.

7. **`Cache::rewind` is removed.** Rollback was implementable in two places — the cache
   could rewind itself, and the input drives the same rollback through its lineage — so a
   cache that took the first route and an input that took the second disagreed about what
   the stream held. There is now one owner. Implementors of `Cache` delete the method; the
   four in-tree implementations did, and no caller loses a capability, because every public
   rollback path already went through the input.

   — *(R9)*

8. **`Cache`'s non-panicking restore-path law now names `push_front`.** The clause previously
   listed the reading and removing operations the crate calls while restoring — `pop_front`,
   `pop_back`, `clear`, `front`, `front_span`, `len` — and omitted the one *writing* operation,
   which is exactly the one the new unwind edge in item 9 depends on. An implementation whose
   `push_front` can panic was vacuously conforming before and is not now.

   The rider is stated rather than implied: a `push_front` that panics is the single case the
   crate **cannot** make whole, because it has already taken the token by value and nothing can
   un-take it. Every other restore-path operation is recoverable.

   — *(R9)*

9. **A panic inside caller code no longer corrupts the stream.** `skip_until` pops a token
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

   — *(R9)*

10. **The committed position is written as one step, everywhere.** The position is a *pair* —
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

    — *(R9)*

11. **`Lexer` gains a clause: the settle path's operations must not panic.** This is a real new
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

    — *(R9)*

12. **An `Incomplete` scan exit leaves no trace.** A scan that ran out of input mid-decision
    used to commit the frontier it had reached, so a caller that topped up the source and
    retried resumed at a position the first attempt had already half-consumed. It now exits
    as it entered.

    — *(R9)*

*CST sink construction and materialization.* A CST sink is bound to its source when it is built,
not when it is finished, and the materialization walk no longer rescans.

13. **`Sink::new` takes the source and a `CstProfile`; `finish` and `finish_partial` no
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

    — *(R8, #123)*

14. **An out-of-language kind now panics at the door, in every build.** A dialect mapper that
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

    — *(R8, #123)*

15. **`FinishError` gains `InvalidDiagnosticSpan`, `MismatchedFinish`, `NonUtf8Source` and
    `InvalidDialectKind`, and `CstEmitter::cst_finish` takes the kind it intends to close.**
    The sink now names the node it closed, so a mismatched finish reports which frame it
    failed against instead of producing a misparented tree — and the identity it compares
    against has to come from somewhere, so `cst_finish` gained a `kind: u16` parameter. It is
    a **defaulted** trait method, so an implementor that does not override it is unaffected;
    every *caller* and every overriding implementor passes the kind it passed to the matching
    `cst_start` / `cst_start_at`.

    **Migration:** `emitter.cst_finish()` becomes `emitter.cst_finish(KIND)`, with `KIND` the
    value the matching open used. Passing a different kind is what
    `FinishError::MismatchedFinish` reports.

    — *(R8, #123)*

*Terminal stops, collection drivers and session points.* A scanner that stops on a resource
limit or a poison boundary is not an end of input, and a driver that cannot make progress is
not a driver that succeeded. Four rounds of repair share one shape: a condition that used to
be reported as *absence* is now reported as *failure*, and the types say so.

16. **A terminal scanner stop is surfaced as an error, never as a decline.** A resource-limit
    trip or a latched poison boundary used to reach a caller as an ordinary end-of-input
    decline, so recovery treated it as "nothing here" and retried against a scanner that had
    already given up. `UnexpectedEnd` now carries a **terminal** flag, raised through
    `into_terminal()` and read back through `is_terminal()`, and the new public trait
    `error::MaybeTerminal` is the channel recovery keys off to re-raise the stop instead of
    recovering it.

    **This is a bound addition on the error type.** `From<UnexpectedEot<L::Offset, Lang>>` is
    now required on the committed peek and `Expect` surfaces, and a downstream error type must
    implement `MaybeTerminal` — an empty `impl MaybeTerminal for MyError {}` is enough unless
    the type can itself carry a terminal stop, in which case it delegates. This release ships a
    **second** terminal carrier alongside the flag: `RecursionLimitReached` (item 46), which is
    terminal for every value, so a type that stores both delegates to both. A **third** terminal
    source ships in this same release and is easy to miss precisely because it is not a
    `MaybeTerminal` implementor at all: `SessionRefusal`. `PartialSession::parse` converts it
    through the consumer's `From` and then asserts the result is terminal, unconditionally — so a
    `Refused(SessionRefusal)` arm left at `false` panics a release build rather than being spent
    (see *A session's budget guard now holds in release builds*, under Fixed). Those three are the
    values this crate **builds and marks**, which is not the same as a closed set of terminal
    conditions: when a *rejecting* emitter refuses a scanner trip's diagnostic, the emitter's `Err`
    is what propagates, built from your lexer's error value through
    `From<<L::Token as Token>::Error>` with no marker on it — the same trip, unmarked, reaching you
    through a different carrier. The trait's own doc carries the three-source table, that path, and
    the rule for an arm the table does not name, including terminal carriers of your own that this
    crate cannot enumerate.
    The try-leaf surface gained the `*_or_stop` shape
    (`InputRef::next_or_stop`, `try_expect_map_or_stop`, `peek_with_emitter_terminal`) so a leaf
    can report the stop without inventing a decline.

    **Derived `Debug`, `Eq` and `Hash` on `UnexpectedEnd` moved with the field.** The struct
    derives all three, so the new private `terminal: bool` appears in every `{:?}` render and
    participates in equality and hashing. A consumer with frozen renders re-blesses; a consumer
    comparing two errors that differ only in terminal status now sees them as unequal, which is
    the point. *This break shipped four rounds ago and was never disclosed anywhere until this
    entry.*

    — *(R1, #111)*

17. **`From<UnexpectedEot<L::Offset, Lang>>` is required at thirteen further sites.** The
    collection drivers stopped masking a terminal stop as a successful end, and stopped
    spinning on zero progress; both exits have to be expressible as an error, so
    `Repeated::parse`, the plain fold wings (`Fold`, `TryFold`, `TryFoldWith`, `RFold`) and the
    `Repeated` caller impls carry the conversion. A production that already names
    `ComposableParseContext` (or `FromTokenErrors`) gets this for free; one that spells its
    conversions individually adds this one.

    — *(R2, #112)*

18. **A `Sink`'s inner emitter must implement `ValueKeyedEmitter` — `Sink<Sink<E>>` is now a
    compile error.** A sink keys its bookkeeping by emitter mark; nesting one inside another
    made two independent mark spaces claim the same keys, so an abandon on the outer sink
    settled captures the inner one still owned. `ValueKeyedEmitter` is a new public marker
    trait with no members, implemented by every diagnostics emitter and deliberately not by
    `Sink`, which is what makes the nesting unrepresentable rather than documented against.

    — *(R3, #113)*

19. **`begin_point` returns a `#[must_use] SessionPointId`, and `commit_point` /
    `rollback_point` take it.** The three were argument-less, so nothing tied a settle to the
    point it settled and an out-of-order or duplicated settle silently corrupted the pin set.
    The id is branded to the handle and to the point, and four misuse conditions now panic by
    name: `no live session point`, `session point settled out of order`, `stale session point`,
    `foreign session point`.

    **Migration:** bind the return value — `let p = inp.begin_point();` — and pass it back:
    `inp.commit_point(p)` / `inp.rollback_point(p)`. Points settle newest-first.

    — *(R3, #113)*

20. **The two separator/token error carriers gained a name channel, widening
    `into_components` on both.** `MissingToken::into_components` returns a 4-tuple
    (`offset`, `expected`, `message`, **`name`**) where it returned a 3-tuple;
    `SeparatedError::into_components` returns a 3-tuple (`position`, `token`, **`name`**) where
    it returned a pair. Both types gain `with_name` and `name()`, and both derive `Eq` and
    `Hash` over the new field, so two otherwise-identical errors carrying different separator
    names no longer compare equal.

    **Migration:** add a binding for the trailing element at every `into_components`
    destructuring; `..` is not available on a tuple pattern, so this is a mechanical edit the
    compiler points at.

    — *(R4, #114)*

*The Pratt driver.* The Pratt driver ends an expression when the RHS channel says so, not when the
input position looks finished — and a report that consumes nothing can no longer be folded.

21. **`PrattRHS` gains an `End` variant.** A grammar says the expression is over on the
    same channel it uses to report operators, instead of relying on a sentinel operator
    below the minimum power. Exhaustive matches on `PrattRHS` must add an arm; the
    compile error is deliberate, which is why the enum is not `#[non_exhaustive]` — a
    wildcard arm is exactly the silent end-swallowing this change removes.

    — *(R6, #120)*

22. **The `is_eoi` loop gates are gone.** They ended the expression on a *position*,
    which a lookahead could move: a widening peek made a typed parse of `1+2` return
    `1`. The RHS channel now decides.

    — *(R6, #120)*

23. **Recursion floors are explicit.** `PrattFloor` replaces the arithmetic that
    reconstructed a floor from a power. Right-associative operators recurse at their own
    power (an off-by-one that made a right-associative pin yield 14 where 10 is correct);
    left and non-associative recurse strictly above it, which also removes the wrong
    parses the old code produced at `Power::MAX`, where the arithmetic saturated instead
    of separating. The token flavour carries its true left power rather than
    reconstructing it.

    — *(R6, #120)*

24. **`PrattPower::next` and `prev` are removed.** They were the arithmetic that made
    the floor bugs expressible; with them gone, reintroducing driver-side power
    arithmetic is a compile error rather than a review comment. This breaks callers, not
    only implementors, and no lint would have flagged them as unused — a required trait
    method warns nowhere. Note also that every in-repo implementation was non-saturating
    despite the trait's own documentation asking for saturation, so each carried a latent
    debug overflow panic.

    — *(R6, #120)*

25. **A report that consumes nothing is refused, terminally.** The typed driver checks
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

    — *(R6, #120)*

26. **The trace channel is more verbose.** With the position gate gone, the RHS channel
    is consulted at exhaustion, so traces gain the consultations the gate used to skip.
    Parse results and non-trace output are unchanged.

    — *(R6, #120)*

The token driver needs none of this: `PrattToken::try_pratt_rhs` and `try_pratt_lhs`
take only `&self` with no `InputRef`, so a report there cannot be made without
consuming, and no token fold receives an `InputRef`. That asymmetry is why the guard
lives in one driver and not the other.

*One bound per production, and the language fences.* The bound a grammar production writes is now
**one line**. Where a production previously restated every conversion its callees might need, it
names a single context bundle and the compiler elaborates the rest.

27. **`ParseCtx` is renamed `ComposableParseContext`** and completed. It now carries
    `FromTokenErrors` — the five token-level conversions as one supertrait — so the nested
    associated-type bound elaborates to callers. `ParseContext` is unchanged. The rename
    touches 59 occurrences across 14 files; the crate-root re-export set gained and lost
    nothing.

    — *(W-api-A, #118)*

28. **`FromUnclosed` replaces the per-delimiter `From<Unclosed<D, …>>` family.** One bound
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

    — *(W-api-A, #118 / #121)*

29. **The `_of` language twins are gone — 167 public items removed.** Every `X::parse_of` is
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

    — *(W-api-A, #118)*

30. **The fluent `separated_by_*` family works for branded grammars.** It previously failed on
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

    — *(W-api-A, #122)*

31. **`Token::Kind: 'static` is hoisted to the trait**, deleting five projection restatements.
    Four superficially similar bounds remain: they constrain the dispatch structs' own free
    `Kind` parameter, which the hoist does not reach.

    — *(W-api-A, #118)*

32. **`Parser` loses its `Error` phantom parameter** and gains value-driven entry points
    (`parse`, `parse_with`, `parse_with_state`); its constructors collapse from eight to four.

    — *(W-api-A, #118)*

33. **The delimiter side gets item 30's fix.** `Delimiter` and `TypedDelimiter`'s blanket impls
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

    — *(W-api-A, #122)*

*Choice dispatch.* An id type whose whole purpose is to make an out-of-range branch
unrepresentable was admitting one.

35. **Array-choice branch ids are 1-based: `<[P; N] as ParseChoice>::Id` is
    `RangedUsize<1, N>`.** `RangedUsize`'s bounds are *inclusive*, so `RangedUsize<0, N>`
    admitted `N` — one past the last index — and dispatching it panicked. `[P; 0]` was worse:
    `RangedUsize<0, 0>` admits `0`, so **every** id for an empty array choice panicked.

    The repair is a bijection rather than a bounds check: exactly `N` representable values,
    `1..=N`, onto the indices `0..N`, so every id that exists is a valid index. A bounds check
    would have left the boundary value constructible and the panic reachable, which is the
    thing the ranged id was for. The audit's own suggestion — `RangedUsize<0, {N - 1}>` — is
    not writable: a generic `N` in a const operation is `error: generic parameters may not be
    used in const operations`, and `generic_const_exprs` is unstable on every supported
    toolchain.

    `[P; 0]` moves from "every dispatch panics at runtime" to "no id exists, and constructing
    one is a compile error" (`E0080`, from `deranged`'s own range assertion, at the point of
    use — an unused `[P; 0]` choice still compiles).

    **Migration:** `Id::new(i)` for the old 0-based `i` becomes `Id::new(i + 1)`, and
    `Id::get()` returns the 1-based ordinal. The boundary value that used to panic no longer
    constructs.

    The two slice impls (`[P]`, `&mut [P]`) have no compile-time repair — length is a runtime
    fact — so they gain a **named** refusal instead: `choice id {id} out of bounds for {len}
    branches` where the raw `index out of bounds` message used to be, plus the documented
    bounds the audit assumed were already there.

    — *(R10)*

*Span mutators.* `SimpleSpan` enforced its ordering invariant on one of its four mutator
surfaces. The other three assigned and added without checking, so a corrupt span was produced
silently and carried onward — and the `## Panics` sections documented panics the code did not
perform.

36. **Every `SimpleSpan` mutator the crate owns now refuses to publish a corrupt span, in every
    build.** Six `*_const` twins gained the ordering assert their non-const twins already
    carried, with the twin's byte-exact message; `bump_start_const`, `bump_end_const`,
    `bump_const` and `<Range<usize> as Span>::bump` gained an every-profile `checked_add` with
    the message `"span bump overflows usize"`; and `<SimpleSpan<O> as Span>::bump` — the
    surface this crate's own error and carrier types relocate through — gained the ordering
    assert after its delegation.

    **This turns silently-corrupt results into panics on code that runs today.** Unlike a
    refusal that rejects a correct program, these surface a defect the program is already
    carrying: `with_end_const(2)` on `(5, 15)` produced `(5, 2)`, and `bump_start_const` at
    `(MAX - 1, MAX)` produced `(0, usize::MAX)` in release with the one existing assert passing
    over the corruption. The migration line is *"your span was already inverted — here is
    where"*.

    **Two surfaces deliberately do not get an ordering assert, and the reason is a law rather
    than an omission.** `bump` shifts both ends equally, so on a well-formed span it can only
    fail under wrap; on an ill-formed one it fires on a precondition the caller did not
    violate. So the ordering assert is licensed by an impl's **construction-axis** strictness.
    `SimpleSpan` refuses inverted construction, so its assert can only catch a wrap — it stays.
    `Range<usize>` documents *lenient* construction (`Span::new(10, 5)` yields `10..5`), so an
    inverted range is a value the trait says you may create; asserting on relocation would hand
    out a value you are then not allowed to use. It keeps the overflow check only.

    The inherent `SimpleSpan::bump` also stays unguarded, and that is priced rather than
    overlooked: guarding it means adding `O: Ord`, which is a real narrowing with a reachable
    inhabitant — `SimpleSpan` derives `Default`, so `SimpleSpan::<NonOrd>::default().bump()`
    compiles and runs today. The guard sits on the `Span` impl instead, whose where-clause
    already carries `Ord`, so it costs nothing. The residue is an in-crate caller that bypasses
    the trait.

    **Stated residue, and it reaches the public non-const mutators too.** On a generic `O`,
    overflow is rustc's own check — a panic in debug, a wrap in release. `SimpleSpan::bump`,
    `bump_start` and `bump_end` are all generic, so `checked_add` is not expressible there
    (`E0599` on a type parameter), and in release the ordering assert catches a wrap **only
    when the wrapped value lands on the wrong side**. `bump_start` at `(MAX - 1, MAX)` wraps
    the start to `1`, still `<= end`, and publishes silently; `bump_end` on `(0, MAX)` wraps
    the end to `2`, still `>= start`, likewise. Both are pinned by release-profile cells, so
    the gap is visible rather than implied.

    **The checked surface for `usize` already exists, and both methods' docs now name it:** the
    `*_const` twins perform the same operation on the concrete type with a real `checked_add`,
    and a `const fn` is callable at run time. Their `## Panics` sections previously promised a
    panic the release build does not perform — the exact fiction this item exists to remove,
    which survived on these two until it was pointed out.

    Closing it *in the generic methods* needs an arithmetic bound on `Span::Offset`, which core
    cannot express — a trait definition or a dependency, plus a breaking narrowing. That stays
    a 0.9 candidate.

    The `Span` trait itself gains the written law (the two axes above) rather than a check:
    `bump` is required, so an implementor's body is theirs. `start_mut` / `end_mut` remain open
    doors by construction — an inversion written through them is unobservable to any assert on
    any mutator.

    — *(R10)*

*Rendered output.* One composition rule, applied at every carrier that violates it.

37. **Error renders say "expected" once.** `Expected`'s own `Display` opens with the word in
    every variant (`expected '}'`, `expected one of: …`), and two carriers wrote it again when
    composing one — so `MissingToken` rendered `missing token 'comma' at 5, expected expected
    '}'` and `UnexpectedToken` rendered `unexpected token, expected expected '}'`. The
    composing sites now supply only the separator.

    **`UnexpectedToken` was the unpinned one, and that is why its stutter outlived the
    repair.** Four pins recorded `MissingToken`'s doubled output as long-standing behaviour;
    `UnexpectedToken`'s renders were pinned nowhere, at any of its three shapes. Both carriers
    are fixed in one commit, and all shapes of both are now pinned byte-exactly.

    **Migration:** a consumer that freezes these renders re-blesses; the delta is exactly one
    deleted word per composed render. A consumer that converts the carriers structurally
    through `From` — which is what the reference consumer does — sees nothing.

    — *(R10)*

*Typed CST views.*

38. **`cst::Node<Lang>` binds `Syntax<Lang = Lang>`.** The supertrait list was
    `Element<Lang> + Syntax`, leaving `Syntax::Lang` free — so a type could be `Element<A> +
    Syntax<Lang = B>` and still satisfy `Node<A>`: a node claiming membership in one language
    while the `KIND` constant that says which node it *is* answers for another. Nothing rejected
    it, and the contradiction surfaced later as a cast that never matches.

    **Implementor-breaking:** a downstream `Node` impl whose brands disagree stops compiling.
    That is the fix. Every in-tree impl and the reference consumer brand coherently already.

    — *(R10)*

*Test-support surface.* Last, because it is opt-in: the `fuzz` feature's alphabet is public
API and moved like any other.

39. **`fuzz::Op` gains an `IsExhausted` variant.** The fuzz alphabet tracks the real public
    input surface one-for-one, so `InputRef::is_exhausted()` earned an op. `Op` is an ordinary
    exhaustive enum: a downstream `match` over `Op::ALL` without a wildcard arm stops
    compiling, which is deliberate — the same compile-time exhaustiveness prod the crate's own
    `Op::label` relies on. `Op::COUNT` moves with it, and the variant was inserted in surface
    order rather than appended, so the fieldless discriminants of every later variant shift by
    one. Nothing in the crate casts them and `Op` carries no `#[repr]`, so the values were
    never a contract — recorded because a mechanical API diff reports them and a reader should
    not have to wonder whether it matters.

    *This item exists because a `cargo semver-checks` run against the published 0.7.3 found
    it, and no PR body, commit message or surface grep had.*

    — *(R5, #116)*

*Diagnostic counts on the recovering path.* Last in this list because it removes a report
rather than adding one, and only where that report was a duplicate.

40. **A wrong opening delimiter is reported once, not twice.** All four delimited many-drivers
    (`repeated`, `repeated_while`, `separated`, `separated_while`) probe the opener, and on a
    wrong token they record it as an expected-open `UnexpectedToken` and leave it **cached and
    unconsumed**. The close-position probe later re-saw that same cached token and reported it
    a second time, as an expected-close miss. Under a recording emitter both landed; the two
    entries carried an identical span.

    The report is now suppressed **iff the emitter's log still holds a live report naming that
    front token** — and the witness for that is a **lineage fact on the input**, not a flag in the
    driver's stack frame.

    That distinction is the substance of the fix. "Has this token already been reported?" is a
    *transactional* question, and a parser's stack frame has no rollback semantics. A driver-local
    flag — committed position, with or without a re-key counter beside it — cannot observe a
    restore, because a restore is exactly the operation that erases frame-visible traces: it
    rewinds the emitter, destroys the stream's front, and returns position to a value the frame
    cannot tell apart from "nothing happened". Three successive frame-local witnesses were built
    and each leaked, always toward the same failure: a **deleted** diagnostic.

    So `Input` now carries a front-report watermark. `Some(end)` means the emitter's current log
    holds an unexpected-token report whose subject is the token the current regime lexes at the
    stream front, ending at `end`. Three writers keep that inductive, and no others: it is
    published by the same body that appends the report and only if the append succeeded; it is
    cleared by a re-key, before the cache clear, so an unwind through that caller code leaves it
    disarmed and a later arm emits; and it is captured into every `Checkpoint` and restored in the
    same no-caller-code block as the emitter rewind. A rollback below the flag truncates the report
    and disarms the watermark as one state; a rollback above it keeps both, because the report
    predates the mark.

    The general law, now stated in the cell census: **a witness must share the rollback semantics
    of the fact it witnesses.** The cache-push counter is restored because a restore drops exactly
    the entries it counts; this watermark is restored because its subject is the emission log, and
    the log is restored in the same move.

    A close miss naming a **different** token, or arriving once the report for the flagged one is
    no longer live, is untouched: `"{1 2 x"`-shaped input keeps both of its reports.

    Because the predicate reads input state rather than a driver local, all eight close-miss arms
    ask it with one identical line — so uniformity across the drivers is a property of the
    mechanism rather than a claim about each driver's control flow.

    **Only recording emitters are affected.** Under a fail-fast emitter the first emit aborts
    the driver, so the second report never existed there and no `Fatal`-path behaviour moves.
    What changes is any assertion that counted diagnostics on the recovering path.

    The suppression is applied at **all eight** `CloseStatus::WrongToken` arms across the four
    drivers, not only the four that today's tests reach — gating a subset is how a fixed defect
    relocates to its own sibling.

    — *(R11, D43(b))*

*The Pratt engines' recursion budget, and the non-associative contract.* Appended after item 40
rather than renumbered into the Pratt group at 21–26, because every number in this section is a
live cross-reference. Inside the group the breaks that do not fail to compile come first, as
everywhere else.

41. **A non-associative operator that repeats is a syntax error, where it used to fold left in
    silence.** Once a frame has folded a `PrattInfix::Neither` operator at
    power `p`, a second *infix* operator at exactly `p` — whatever its own associativity — now
    fails the parse with `NonAssociativeChain`, with the offending operator left on the input
    unconsumed. Both engines raise it, on the same trigger, with the same restored posture, and it
    is **returned** rather than emitted, so no recording emitter and no rewind can absorb it.

    Previously the repeat was simply admitted. At the top level that merely left a tail behind —
    `1 ; 2 ; 3` folded to `(1;2)` — but nested, it had no caller-side remedy at all:
    `7 = 1 ; 2 ; 3` returned `Ok(((7=(1;2));3))` with the **whole input consumed**, so nothing was
    left over for an end-of-input check to catch. That shape is the reason this is an error and not
    an ending. Non-associativity is a property of a *chain*, and the only frame that knows the
    chain exists is the one holding the latch; hand the operator up instead and the recipient is
    the same engine one frame up, which sees an ordinary admissible operator, folds it by its own
    rules, and re-associates the chain across itself. It is structurally incapable of knowing the
    constraint.

    **The contract is per-operator, not whole-chain fixity resolution.** The latch is cleared by
    folding an infix at a different power and is untouched by a postfix fold, so `a == b < c` —
    this variant, then `Left` at the same power — is rejected while `a < b == c` is accepted.

    **Not terminal**, deliberately: this is malformed input, the classic recovery target, so
    `Recover`, `InplaceRecover` and `skip_then_retry` may spend it. A grammar that wants the old
    fold-once-then-stop behaviour asks for it in grammar code — wrap the pratt parser in a recovery
    combinator, or reclassify the operator `Left` — because tolerance is a caller policy and not a
    silent engine default. Where a repeat coincides with a zero-consumption report, item 25's stall
    outranks it: the report boundary is checked first, since a driver must not diagnose a chain
    built out of a contract violation.

    — *(R12; the two exits ranked in R13)*

42. **Recursive descent is bounded, and the bound is on by default.** Both Pratt engines enter one
    level per live frame through the new `InputRef::descend`, whose `Descent` guard releases the
    level on **every** exit of the frame — return, `?`, or unwind, identically in `std` and
    `no_std` — and exceeding the budget fails the parse with the always-terminal
    `RecursionLimitReached`. It too is returned rather than emitted, so a tripped budget cannot
    reach a caller as a truncated-but-successful parse.

    Previously the descent was bounded only by the token count, which is not a bound a *machine*
    has. The red-before probe for this item did not fail an assertion; it **aborted the test
    process** with `fatal runtime error: stack overflow`.

    The budget is per *input session* rather than per parser — two expression parsers composed into
    one grammar draw on one depth, which is what makes it a bound on native stack use rather than
    on any single production — and it is configured with the new
    `InputContext::with_recursion_limiter` / `ParserContext::with_recursion_limiter`.
    **`RecursionLimiter::unlimited()` restores 0.7.3's behaviour exactly**, at 0.7.3's risk.

    **The default is 64, and it is sized against the tightest measured configuration rather than
    the most generous one.** Bisected on this tree, one pratt frame per level, on an explicitly
    sized **2 MiB** thread — what `std::thread::spawn` and every libtest harness thread gets, and
    so the smallest stack a parse is likely to run on:

    | build | typed driver | token driver |
    |---|---|---|
    | release (`opt-level = 3`) | 3871 frames | 4247 frames |
    | debug (`opt-level = 0`) | 384 frames | **125 frames** |

    The binding cell is the debug token driver at 125, not the release figures above it, and the
    asymmetry is the whole argument: a limit that is too low returns a clean, catchable error
    naming the knob that raises it, while one that is too high aborts the process with no
    diagnostic and takes the suite with it. Only one of those is recoverable, so the default is set
    where every measured configuration survives. 64 is the largest power of two leaving ~1.9×
    margin under 125, and it clears the other three cells by 6×, 60× and 66×. A grammar parsing
    untrusted, deeply nested input should still set its own limit against the stack the parse will
    actually run on, rather than inherit this one.

    **Your own recursive combinators draw on the same budget, and `InputRef::descending` is how.**
    It takes the frame's body as a closure and owns the level for exactly that body:

    ```rust,ignore
    inp.descending(|inp| match remaining {
      0 => Ok(inp.recursion().depth()),
      n => nested(inp, n - 1),
    })
    ```

    The closure's error is returned untouched, so `?` inside composes with whatever the frame
    already returns and the trip is built as the frame's own error type; a body written entirely
    as the closure keeps its `return`s; a panic releases the level on the unwind. Nothing the
    closure can write releases it earlier — it is handed the input, never the guard, and
    `InputRef::recursion` is read-only.

    **`InputRef::descend` is also public, as the low-level escape hatch, and it is easy to write
    wrong.** It hands the level back as an ordinary value, so *where the level ends is caller
    code*. `Descent` is `#[must_use]`, which catches **one** spelling of getting that wrong and
    not the others. Measured on this tree at limitation 8, over 200 recursive calls, with the
    abort depth bisected one process per depth on a 2 MiB thread:

    | frame body | diagnostic | at 200 calls | aborts by |
    |---|---|---|---|
    | `inp.descending(\|inp\| …)` | — | `Err(RecursionLimitReached { depth: 9, limitation: 8 })` | never |
    | `let mut frame = inp.descend()?; let inp = &mut *frame;` | — | same | never |
    | `inp.descend()?;` | `unused_must_use` | `Ok`, depth 0 | 5 000 |
    | `let _ = inp.descend()?;` | none | `Ok`, depth 0 | 5 000 |
    | `if inp.descend().is_ok() { … }` | none | `Ok`, depth 0 | 5 000 |
    | `let d = inp.descend()?.recursion().depth();` | none | `Ok`, depth 1 | 4 000 |
    | `drop(inp.descend()?);` | none | `Ok`, depth 0 | 5 000 |

    An abort is `fatal runtime error: stack overflow` — the failure this whole item exists to
    delete, reinstated by a line that compiles. The abort depths are a demonstration rather than
    a constant: they are one debug build's frame size, probed at 1 000-level steps, and one row
    moved a whole step between two builds of the same source. The four silent rows are **not a
    closed list**:
    any expression that consumes the guard and lets it die before the recursion behaves the same
    way, and rustc's own `help:` on the caught row suggests the second one. Early release cannot
    be forbidden, because it is sometimes what a frame wants, so what closes the question is the
    *shape*: `descending` makes the level's scope and the body the same region by construction,
    the bound guard makes them the same region by discipline. What the type system guarantees on
    its own — for both spellings — is *balance* (every level entered is left exactly once, by the
    destructor, unwind included) and that a **live** guard is the only route to the input.

    Rails: `tests/ui/descent_dropped_early.rs` pins the attribute on the MSRV toolchain,
    `src/input/input_ref/descent_tests.rs` pins every row of the table above at runtime on every
    toolchain, and its `#[expect(unused_must_use, …)]` turns a deleted attribute into a hard error
    under the crate's `deny(warnings)`.

    — *(R12; the default resized against measurement in R13; the guard marked `#[must_use]` in
    R22; `descending` added and the other three early-release shapes measured in R23)*

43. **`RecursionLimiter`'s default depth drops from 500 to 64 in every constructor that supplies
    one** — `RecursionLimiter::new` and its `Default`, and `Limiter::new` /
    `Limiter::with_token_tracker`, which build one for you. This is the row that reaches code with
    no pratt parser in it: the same type doubles as a **lexer-side** nesting tracker in a `State` /
    `Extras` position, where it counts lexing nesting, costs no native stack, and where 500 was
    never sized against anything. A lexer that inherited the default and nests deeper than 64 now
    trips where it did not.

    Spell the limit you meant with `RecursionLimiter::with_limitation`. The one `500` left standing
    in the docs is the lexer-extras example in `Limiter`'s own documentation, and it is left
    deliberately: there the number is the example's explicit choice rather than a default, and the
    two subjects are worth keeping apart.

    — *(R13)*

44. **`NonAssociativeChain`'s offset is *defined* as the handback position** — the driver's own
    restore target — and not a position measured anywhere near the offending operator. `1 ; 2 ; 3`
    reports **5**, where a formulation written earlier in this campaign reported 6.

    The definition is checkable rather than descriptive, and that is the point of it: catch the
    error and **`InputRef::span().end()` *is* the offset**. Everything from that byte onward is
    still available to the caller — including whatever the deciding read had consumed before it was
    handed back.

    That identity is about the **handback**, and it is not a statement about where a recovery
    combinator restarts. `Recover` and `skip_then_retry` speculate through `try_attempt`, whose
    failure path restores the pre-attempt checkpoint before the handler runs or the skip loop
    starts, so both resume at their own attempt origin instead; `InplaceRecover` never backtracks
    and is the one that stands where the offset names. Measured on `1 ; 2 ; 3` with the whole pratt
    parser wrapped, where the error carries 5: catching the `Err` yourself reads 5,
    `inplace_recover` reads 5, `recover` reads **0**, and `skip_then_retry` scans forward from
    **0** — its first sync candidate is the `1` at 0 and its first skipped region is `0..1`.

    Reading it as a pointer at the operator is wrong in four ordinary shapes, each measured:
    whitespace the lexer skips (`1 ; 2 ; 3` — offset 5, the repeat at 6); whitespace surfaced as
    trivia *tokens* and skipped by the classifier (offset 5, the repeat at 8); an operator spelled
    with two tokens (offset 7, head at 8, tail at 10); and a non-fatal lexer error, reported and
    stepped over (offset 5, the repeat at 8). Naming the operator would describe a position no
    caller was ever left at, and would invite a recoverer resuming there to discard the region in
    between — in the last shape, the lexer error's own bytes together with the diagnostic that was
    rewound with the aborted probe and is due to be re-emitted. The operator's head is not
    obtainable in general anyway: `parse_pratt_rhs` holds a whole `InputRef` and decides for itself
    where its operator begins, so the only way to learn what it would skip is to run it — and the
    repeat has to be decided before it may.

    **`Display` names it the same way**, because the text is what a caller who never calls
    `offset()` still has: `non-associative operator cannot be chained at its own power; input
    handed back at 5, at or before the operator`. It does not report the operator's position,
    for the reason above — the engine does not have it. See *Debug and rendered output*.

    — *(R14 defined it, R15 made the number the restore target)*

45. **`InputContext::into_components` returns `(E, C, RecursionLimiter)`** where it returned
    `(E, C)`. The context carries the recursion budget now, and a decomposition that dropped it
    would hand the input an unconfigured one. `InputContext::new`'s arity is unchanged — the budget
    defaults, and `with_recursion_limiter` sets it — so only the destructuring moves, and `..` is
    not available on a tuple pattern, so the compiler points at every site.

    — *(R12)*

46. **Both Pratt engines require two more conversions from a grammar's error type.** The token
    engine's `pratt`, `pratt_with_min_precedence` and `pratt_in`, and the typed driver's
    `ParseInput::parse_input` impl block, gain `From<RecursionLimitReached<L::Offset, Lang>>` and
    `From<NonAssociativeChain<L::Offset, Lang>>`. Two `From` impls on your error type; every
    example and bench in this repo carries the shape.

    **`From`, and not a `FromPrattError` method, because these two never pass through an
    emitter.** Neither has an `emit_*` counterpart on `PrattEmitter`, by design — a recording
    emitter must not be able to swallow a resource trip or a rejected chain — so the engines
    *return* them, and a returned error becomes the caller's type through `From`. The trip's
    value is built and converted at `InputRef::descending` / `InputRef::descend`, the recursion
    entry every recursive-descent grammar shares whether or not it has a pratt parser in it; the
    repeat is built by each engine at its own exit. Neither point has an emitter in scope, so a
    conversion method on the emitter-side bundle would be one nothing could ever call.
    `FromPrattError`
    keeps exactly the two conversions a `PrattEmitter` body performs, and **no hand-written
    `FromPrattError` impl has to change.**

    The choice this obligation carries is unchanged, only its spelling: an impl that *stores* the
    trip keeps its always-terminal marker readable through `MaybeTerminal`; one that discards it
    opts the error type out of terminal re-raise (see *Known limitation — a discarding error
    sink…* below). Both are legitimate, and a `From` impl is the author's own code either way, so
    neither is picked for you.

    — *(R12, R21)*

### Debug and rendered output

Every `Debug` / `Display` movement in this release, in one place, because a consumer who
freezes renders needs one list rather than a reading of the whole changelog. This subsection
exists because a private-field `Debug` delta broke a real consumer's frozen parity renders
during this campaign and cost a bisect — and because the *earliest* of these shipped four
rounds ago and was disclosed nowhere until now.

**Mechanism is stated per row, because it decides who has to review the next field
addition.** A *derived* `Debug` moves by itself the moment a field is added; a *hand-written*
one is a maintained surface that a field addition silently leaves stale unless someone edits
it. Both are listed, and which is which is not guesswork here — it was measured, and it is not
what an earlier inventory of this release assumed.

| Type | What moved | Mechanism | Round |
|---|---|---|---|
| `UnexpectedEnd` | gained a private `terminal: bool`, so `{:?}` renders it and `Eq` / `Hash` account for it — two errors differing only in terminal status stop comparing equal | **derived** | R1, #111 |
| `Unclosed` | gained the `kind` field | **derived** | #121 |
| `MissingToken` | gained the `name` channel; `Eq` / `Hash` derive over it | `Debug` is **hand-written** (`debug_fmt`), and already names the field | R4, #114 |
| `SeparatedError` | gained the `name` channel; `Eq` / `Hash` derive over it | `Debug` is **hand-written**, and already names the field | R4, #114 |
| `FinishError` | gained `InvalidDiagnosticSpan`, `MismatchedFinish`, `NonUtf8Source`, `InvalidDialectKind` | derived | R8, #123 |
| `CstProfile`, `KindValidator` | new types; `Debug` deliberately does **not** render the fn pointer (its derived form is a code address) | hand-written | R8, #123 |
| `PartialSession`, `Budget`, `SessionRefusal` | new types | `PartialSession` hand-written (pinned field list); the other two derived | R8, #123 |
| `MissingToken`, `UnexpectedToken` | `Display` drops the doubled word: `…, expected expected '}'` becomes `…, expected '}'` | rendering change, both carriers | R10 |
| `SeparatedError` | gained a `Display` it never had | new rendered surface | R10 |
| `RecursionLimitReached`, `NonAssociativeChain` | new types, so nothing frozen moves; listed because both are carriers a consumer will render — `Debug` derived, `Display` from `thiserror`. Each render names its offset the way the type's accessor does, not as a construct's location: `recursion limit reached at 15: depth 9, maximum 8` (committed consumption at the frame that could not be entered) and `non-associative operator cannot be chained at its own power; input handed back at 5, at or before the operator` (the handback of item 44 — on `1 ; 2 ; 3` the repeated operator starts at 6, and the render deliberately does not claim to name it). Both strings, and both derived `Debug`s, are pinned in `tests/render_freeze.rs` | derived | R12 |

**New panic messages.** A consumer that captures panic payloads — a test harness, a supervisor
loop, a `catch_unwind` host — sees new observable strings even though no `Debug` or `Display`
impl moved:

- `"end must be greater than or equal to start"` and `"start must be less than or equal to end"`
  on six `SimpleSpan` const mutators and on `<SimpleSpan<O> as Span>::bump`;
- `"span bump overflows usize"` on three const mutators and on `<Range<usize> as Span>::bump`;
- `"choice id {id} out of bounds for {len} branches"`, replacing the raw slice-index message;
- `"checkpoint id space exhausted"` / `"savepoint sequence exhausted"` /
  `"cache-push counter exhausted"` on the input's witness counters;
- the two source-identity refusals at the parse entry — a different source value, and a bound
  extent shorter than the parse reads.

**Of these, only the span messages are reachable from a program that runs today**, and that is
the point of them: they replace a silently corrupt span with a panic that names where it went
wrong. The rest fire only on misuse, on exhaustion of a 2^64 id space, or — for the
source-identity refusals — only where an unequal reference *proves* an unequal source, or where
the emitter is bound to a strictly shorter slice of that source than the parse reads. Neither is
something a correct program produces: the second refuses only the direction that can smuggle
structure, and leaves the longer-bound streaming direction alone.

<a id="0.8.0-added"></a>

### Added

New public items that arrived *as part of* a breaking change are described in full at the
item that carries them, and listed here only so scanning this section does not miss them:
`error::MaybeTerminal` (item 16), `ValueKeyedEmitter` (item 18), `SessionPointId` (item 19),
`MissingToken`/`SeparatedError`'s `with_name` and `name` (item 20), `PrattFloor` (item 23),
`DelimiterKind` and `Delimiter::KIND` (item 28), `SeparatorHandler::OBSERVES_SEPARATORS`
(item 5), `CstProfile` and `KindValidator` (item 13), `error::NonAssociativeChain` (item 41),
`error::RecursionLimitReached` with `input::Descent`, `InputRef::descending`,
`InputRef::descend`, `InputRef::recursion`, `RecursionLimiter::unlimited`,
`InputContext::with_recursion_limiter` and `ParserContext::with_recursion_limiter` (item 42). **`FromPrattError` is not on this list**:
item 46's two new obligations are `From` impls on your own error type, and the trait gains no
member, so a hand-written `FromPrattError` impl compiles unchanged.

- **The logos adapter works on 0.14 and 0.15, and is tested there.** `logos_0_14` /
  `logos_0_15` / `logos_0_16` were already separate features, but `tokora::logos` (and its
  `__private` twin) re-exported 0.16 only, and every one of the 79 integration test files was
  gated on the plain `logos` feature — which *implies* 0.16. The consequence was that the
  per-version CI legs compiled a crate whose adapter integration tests were all invisible to
  them: they ran 2452 tests each and not one exercised a logos adapter.

  Both aliases now follow the same newest-wins precedence chain the adapter re-exports use
  (0.16 > 0.15 > 0.14), and the 118 `feature = "logos"` gates across 99 files became the
  three-version disjunction. `--features logos` still means exactly what it meant — the
  feature still enables 0.16 — so no existing consumer changes.

  Measured: the versioned legs go from **2452 to 6020 tests** at 0.14 and 0.15, all passing.
  Exactly one source construct was version-divergent across the whole surface — logos 0.16's
  `allow_greedy` nested attribute in one test fixture — and it is now a per-version
  `cfg_attr` split rather than a 0.16-only file.
  — *(R11)*

- **Oracle pins for the checkpoint capture window.** Taking a checkpoint (`save`, and so
  `begin`, `attempt` and session points) registers two things a later settle must find — a
  lineage entry and an emitter mark — and a `Checkpoint` has no `Drop` to settle them with, so
  an unwind between the first registration and the finished value would strand it where nothing
  could ever find it. The capture has run every fallible step ahead of both registrations since
  that was fixed, and nothing pinned it from the outside.

  Three tests now do, one per kind of caller code the window runs: a panicking `L::Span::clone`,
  a panicking `L::State::clone`, and a **foreign `Emitter::checkpoint` that panics**. Each
  asserts that the armed payload actually fired *and* that the world across the caught unwind is
  the world before it — live checkpoints, pins, emitter marks, cursor, span, recorded
  diagnostics. These are additive pins, not fixes: they were red before the reordering and are
  green now, and their red was demonstrated by mutating the capture order rather than claimed.
  The emitter cell is deliberately a law boundary — it asserts tokora's half only, because a
  mark the emitter never handed back is the emitter's to reconcile.
  — *(R11)*

- **Streaming sessions.** `PartialSession`, `Budget`, `SessionRefusal`, `RedriveFromBase` and
  the sealed `ReplayMode` give `parse_partial` a lifecycle: a partial parse survives its
  refills instead of restarting, under a budget that a caller sets and can observe. See the
  known limitation below before pairing a session with a recording `Sink`.
  — *(R8, #123)*

- **`CstText`** — the source-to-`&str` bridge, so a byte-like source materializes a tree when
  it is valid UTF-8 and returns `FinishError::NonUtf8Source` with the offending offset when it
  is not, rather than being unusable.
  — *(R8, #123)*

- **`SeparatedError` gains `Display`.** The D30 separator-name channel was readable only by
  destructuring through `into_components`, so a diagnostic that knew *which* separator was
  involved had no way to say so — half the channel's user-visible point had nothing to render
  through. It is born under the composition rule below, with a single "expected".
  — *(R10)*

- **`BStr` joins the `Equivalent` family.** `utils::cmp`'s module documentation listed `BStr`
  among its backings while the impls were absent from **both** files the family spans, so
  `S: Equivalent<str>` refused `&BStr` with a bare `E0277`. The `Equivalent<str>` /
  `Equivalent<[u8]>` / `Equivalent<BStr>` triple and the `ToEquivalent`/`IntoEquivalent` half
  now exist, gated on `bstr_1` like the `Source` impls.

  The two files are deliberately **not** collapsed into one macro this round: the compile-time
  assert lattice is the anti-drift mechanism, and its rows fail to compile if either half of a
  backing goes missing. That is teeth by construction, where a macro would be organisation-only
  churn.
  — *(R10)*

- **`PolicyComposableEmitter` and `PolicyParseContext`** — a second bundle tier, for the count
  and separator **policy** builders. `ComposableEmitter` covers the collecting combinators at
  their default policy; `at_most` / `bounded` additionally need `TooManyEmitter`, and
  `require_leading` / `require_trailing` need the missing-separator pair. Before this there was
  no bundle carrying those, so a production using a policy builder fell back to spelling the
  ladder — the exact restatement a bundle exists to remove, reappearing one tier up.

  Two tiers rather than one widened bundle, deliberately: widening would make every consumer's
  *concrete instantiation* demand `From<TooMany>` and `From<MissingToken>` on its error type —
  for `Fatal` and `Verbose`, whose impls carry those bounds — whether or not a policy builder
  is ever used. Bundle-1 is a supertrait of bundle-2, so the lattice is strict and each tier is
  true to its own documentation. Pratt is outside both and names its own emitter.

  Additive: bundle-1 is untouched, so nothing migrates. **The re-scoped documentation is the
  other half of the fix** — `ComposableEmitter`'s and `ComposableParseContext`'s docs, and the
  three guide chapters that repeated the claim, said they covered a surface they did not.
  — *(R10)*

- **`Silent`'s `PrattEmitter` impl loses four bounds it never used.** It demanded
  `FromEmitterError` plus `From<UnexpectedEoLhs>` and `From<UnexpectedEoRhs>` on the error type
  while both bodies discard their payloads, which made `Silent` representation-dependent where
  its three siblings are not — `Ignored`'s twin has carried no bounds all along. Removing impl
  bounds only widens the set of admissible types, so nothing migrates; a `Silent<E>` whose `E`
  implements no conversions now drives the token pratt driver. The typed driver's own
  `From<UnexpectedEoLhs/EoRhs>` obligations are unaffected — those are stated at its call
  sites, and `Fatal` and `Verbose` keep their bounds because their bodies use the conversions.
  — *(R10)*

- **`CstProfile` and `KindValidator` no longer require the `rowan` feature.** Both are
  fn-pointer and data only, with no rowan dependence at all, so the gate was placement rather
  than design. It mattered because a `macro_rules!` expansion cannot cfg on tokora's features:
  a macro whose validator arm names `KindValidator` was forcing `rowan` onto every consumer of
  the macro, including those who never build a tree. Strictly more configurations compile; the
  sink half keeps its gate.
  — *(R10)*

- **A CST `Sink` implements `UnclosedEmitter`.** It was the one capability trait the sink did
  not serve, and the omission was not cosmetic: `UnclosedEmitter` is a member of
  `ComposableEmitter`, so a `Sink` could not satisfy the one-bound collecting surface at all —
  a delimited CST parse could not name the bundle its diagnostics-only sibling names. The
  forwarding census could not see it either, because it hard-coded a capability list with the
  same gap; the two omissions hid each other. The census now asserts the membership at the type
  level instead of by string matching.
  — *(R10)*

- **A parse and its emitter can share a source identity.** `Emitter::bound_source` (defaulted
  `None`, so nothing migrates) lets an emitter declare the source it binds; the point at which
  an emitter is attached to an input compares that against the source the parse reads and
  refuses a **provable** mismatch. `Source::REFERENT_IS_BYTES` (defaulted `false`) is how a
  backing says whether an unequal reference proves anything — `true` only for the `?Sized`
  backings, where the reference *is* the data.

  `SourceIdentity` carries **two** things, because there are two failures and they are not
  symmetric. `addr()` is the offset origin, and it is an equality: `&buf[..2]` and `&buf[..3]`
  share one, `&buf[1..]` does not. `extent()` is the byte length, and it is an *ordering* —
  `covers()` is the named relation, and it requires the emitter's extent to be **at least** the
  parse's:

  - a sink bound to a **shorter** slice than the parse reads is refused. It needs no
    out-of-bounds span to do harm: a parser can peek past the sink's end, commit nothing there,
    and let those bytes choose the tree's shape. The result materializes with no
    `SpanOutOfBounds` and no `UncoveredGap`, and round-trips against the sink's own buffer — so
    nothing downstream can see that the structure came from bytes the text does not contain.
  - a sink bound to a **longer** slice is the fixed-arena streaming shape and is accepted. Every
    span the parse emits lies inside the sink's extent, so every slice is correct; the unparsed
    tail is `finish`'s question, answered as `UncoveredGap` or tiled by `finish_partial`.

  See the known limitation below for what this closes and what it leaves.
  — *(R10)*

- **`conformance::emitter`** — the two-assertion kit for an emitter that binds a source:
  `assert_binds_source` catches the author who inherited `bound_source`'s default while
  actually binding one, and `assert_forwards_bound_source` catches a wrapper that forwards
  every emission but not this one. Both failure modes are silent otherwise, and neither is
  detectable from inside tokora.
  — *(R10)*

- **`conformance::cache`** — the cache contract as a runnable kit rather than prose.
  `CacheHarness` drives an implementation through the rollback, put-back and lookahead laws
  the trait states, so a third-party `Cache` can be checked against them instead of
  inferred to comply.
  — *(R9)*

- **`Dialect`** — a two-item anchor (`Lang`, `Lexer`) with the projection aliases `LexerOf`,
  `LangOf`, `DialectSlice`, `DialectInput` and `DialectErrorOf`. Everything else a production
  needs — source, slice, token, span, offset, token capabilities — is reachable by projection,
  so pin those in a one-line subtrait rather than as further associated items.

  There is deliberately no `DialectCtx`. A context bound written through `LangOf<'inp, D>`
  does not unify with one written at the brand: the param-env predicate stays at the
  projection while the obligation normalises. This is documented at the type with the
  mechanism and the verbatim failure, so the shape is not rediscovered by trial.
  — *(W-api-A, #118)*

- **`#[diagnostic::on_unimplemented]` on `FromTokenErrors`, `FromUnclosed` and `Dialect`.**
  A missing conversion now reports what is missing and carries a copyable impl skeleton,
  rather than surfacing as a nested obligation inside crate internals.

  Note for consumers: rustc attaches the *root* obligation's attribute. A bound reached
  through your own blanket-implemented subtrait reports that subtrait, not ours — annotate
  your own dialect traits if you want curated text there.
  — *(W-api-A, #118)*

- **`InputRef::is_exhausted()`** — the consumer-exhaustion predicate: no lexed token is waiting and
  the lexer frontier has reached the end of the buffer. This is the correct gate for a driver loop,
  where `is_eoi()` is not: `is_eoi` answers `true` the moment any lookahead lexes through the end,
  while the tokens that lookahead produced are still unconsumed. `is_exhausted()` is independent of
  the cache implementation.
  — *(R5, #116)*

- **`Cache::RETAINS_FRONT`** — a new trait constant declaring that a front push into an **empty**
  cache always succeeds, i.e. the cache can retain at least one token. It defaults to `false`, so
  the addition is semver-compatible and an existing implementation keeps working unchanged. A cache
  that declares `true` lets the input layer prove its parked-front slot statically unreachable, so
  every probe of it folds away at monomorphization; the shipped caches declare it accordingly
  (`Option` `true`, `GenericArrayDeque<_, N>` `N != 0`, the black holes `false`). A cache that
  declares `true` and then refuses a front push into an empty cache is violating the contract, and
  now panics at the refusal rather than losing the token.
  — *(R5, #116)*

- **Match-first token dispatch.** `select!` / `try_select!` and their runtime,
  `tokora::parser::dispatch_take` / `try_dispatch_take`. One arm per kind, the kind table
  written once beside the patterns, the head classified once against the whole table, and
  each arm receiving the **moved** payload — replacing one annotated closure per kind plus a
  dead re-match, and the hand-written `unreachable!()` for "the kind matched but the variant
  did not" (the runtime turns that into a typed error carrying the table). Both runtimes are
  generic over completeness, so streaming (`Partial`) parsers can use them; the context must
  be a `ComposableParseContext`. The kind expressions must be **const-promotable** — a
  non-promotable one is `E0716` pointing at the invocation rather than the arm; the macro's
  own docs say so and name the fix.
  — *(W-api-B)*

- **Head observation that does not fold a halt into absence.** `InputRef::peek_head_map`,
  `head_satisfies` and `peek_kind`. `peek_one` answers `Ok(None)` for genuine end of input
  **and** for a resource-limit trip or a latched poison boundary, so a production that
  decides on the answer reads a halt as a grammar fact. The new family reserves `Ok(None)`
  for the real end of input and raises the terminal end-of-input error otherwise — marked
  terminal **when your emitter accepts the trip's diagnostic**. A *rejecting* emitter's `Err`
  is built from your lexer's error value and propagates from the fill unmarked, so recovery
  can still spend that trip — `MaybeTerminal`'s doc has that path and the arm it needs.
  `peek_one` is unchanged and is not deprecated.
  — *(W-api-B)*

- **Classify-then-take.** `InputRef::try_expect_take` / `try_expect_take_or_stop`: one
  classification by reference, the commit, then the token **by value** into a projection.
  `try_expect_map` projects by reference, so payloads cannot move out of it.
  — *(W-api-B)*

- **Three-way speculation and an imperative span bracket.** `InputRef::attempt_parse`
  (`Accept` commits, `Decline` rolls back with no fabricated error, `Err` rolls back and
  propagates) and `InputRef::spanning`.
  — *(W-api-B)*

- **Width-1 decisions without a window.** `while_head`, `while_kind` (and their types
  `WhileHead` / `WhileKind`) plus `ParseInput::peek_then_head`. The adapters are `Decision`
  impls pinned at `W = U1`, which is what removes the `::<_, U1>` turbofish, the `Peeked`
  window and the `Ctx` parameter from a hook's signature.
  — *(W-api-B)*

- **Output pinning.** `pinned::<O2, _>(parser)` and `Pinned<P, O>`, for a receiver with
  several output impls (`Collect`, `With`, `FailWith`, `Accepted`) that is ambiguous at
  every downstream site not naming the output. `Pinned`'s phantom is `fn() -> O`, so
  pinning does not move `O`'s auto-traits onto the parser and stays covariant in `O`.
  A fluent `.outputs::<O2>()` method was built and then **removed before release**: it had
  to be a blanket trait method, which plants a candidate on every `Sized` type, and a
  by-value candidate is picked before a consumer's same-named `&self` method. That was
  accepted while it was believed such a collision must be loud; a turbofish with a
  discarded return makes it silent, and an extension method whose job is to pin a type has
  no inferred spelling, so the silent path is its default. A free function resolves by path
  and can shadow nothing. The fluent form stays purely additive later.
  — *(W-api-B)*

- **Three-way ergonomics.** `ParseAttempt::into_option` (the existing `From` conversion, with
  a name that can be found) and `attempt!` — `?` for the three-way vocabulary, early-returning
  `Ok(ParseAttempt::Decline)`.
  — *(W-api-B)*

- **Exclusion parsing.** `Ident::try_parse_except` / `Ident::parse_except`: "a Name, but not
  `on`". The exclusion runs inside the classify predicate, so a decline consumes nothing.
  — *(W-api-B)*

- **Fluent parity** for the free functions most reached for: `ParseInput::{labelled, traced,
  list_until, separated1_by}` and `TryParseInput::opt`. `list_until` and `separated1_by`
  inherit the `Complete` pin of the free `list` / `separated1` they delegate to; that is
  stated on each, with the contrast against `select!`.
  — *(W-api-B)*

- **A declared CST kind space.** `syntax_kinds!` and the module `tokora::cst::kinds`. A
  dialect hand-keeps three things that must agree — the `#[repr(u16)]` enum, the
  declaration-order array its consumers index, and the predicate it hands to `CstProfile` —
  and a disagreement between them is the one shape the sink's own enforcement cannot see,
  because it only ever sees one side. The macro generates all three, plus a checked
  `from_raw`. No feature gate: it names `KindValidator`, which this release un-gates from
  `rowan`, so a `no_std` consumer that cannot enable `rowan` can still declare its kind space
  — verified from a `no_std`, no-`alloc`, no-`rowan` consumer crate on both stable and the
  1.87 MSRV.
  — *(W-api-B)*

- **Emitter diagnostics.** Eight `#[diagnostic::on_unimplemented]` messages across `Emitter`,
  `ComposableEmitter` and the six members between them, so a type short of the bundle is told
  which member is missing rather than getting rustc's default trait-bound phrasing.
  — *(W-api-B)*

### Fixed

- **An `Input` is bound to exactly one emitter, at construction, for its whole life.** The input
  stores two **suppression** watermarks whose meaning is relative to *one emitter's log* —
  `emitted_error_end` (lexer-error dedup, shipped) and `front_reported_end` (close-miss
  suppression, added this release) — while `Input::as_ref` took the emitter as a **parameter**,
  so which log a watermark described was chosen per borrow. A watermark carried into a different
  log claims a report that log never received, and because these watermarks suppress, the
  consequence is a **silently dropped diagnostic**, not a duplicated one.

  It was latent: no in-crate path paired one input with two emitters, and `Input` is
  `pub(crate)` and unexported, so no external path existed. That is a property of in-crate
  discipline, though, not of the type system — which is what this replaces. `Input` now owns an
  `emitter: Ctx::Emitter`; `with_state_and_cache` becomes `with_state_and_context`, taking
  exactly the `InputContext` that `ParseContext::provide` already returns; and `as_ref` loses its
  emitter argument. The per-borrow choice is not merely unexercised, it is **unconstructable**.

  Three consequences worth naming. The **source-identity handshake** moves into the constructor,
  which is where the pair now forms — so it runs once per input rather than once per handle, and
  covers every borrow by construction. `impl Clone for Input` is **deleted**: a clone would have
  had to duplicate an emission log, which is not a thing a log can be. And the `{emitter mark,
  emitted_error_end, front_reported_end, poison_boundary}` group that `Checkpoint` has always
  saved and restored as a unit is now a unit in the *live* structure too — before, two of the
  four were facts about a log the input did not hold.

  **The `as_ref` call-site census from #127 is deleted in the same change**, and the reason is
  the point: that rail guarded the claim *"no in-crate path pairs one `Input` with two different
  emitters"* during the window when the claim needed guarding. There is no longer an emitter
  argument for a new call site to mispair, so the claim is unconstructable rather than merely
  unviolated, and the rail has nothing left that could fail. A rail whose property becomes
  structural dies with the property rather than lingering as a check that can no longer check —
  the *check that stops being a check* class this campaign catalogued. Its replacement is the
  type system. One cell arrives in its place, `dedup_watermark_survives_a_reborrow_of_one_input`,
  which pins the only shape still *writable*: a watermark raised in one handle still suppresses
  the next handle's re-lex of the same region. That cell is also what forecloses the cheaper
  remedy the issue weighed — clearing every log-dependent watermark on borrow — from being
  reintroduced later as a tidy-up, since under it the cell reports twice.

  No public item changes signature: `Input` is `pub(crate)`, and `InputRef` — which *is*
  re-exported, from both the crate root and the prelude — is untouched, as are `Session`,
  `Checkpoint`, the scan paths, transactions and session points.
  — *(R11, #128)*

- **The documentation builds at every feature point.** `cargo doc --no-default-features` had
  never been run: it exited 101 with `error: could not document tokora` and **39 unresolved
  intra-doc links** across 15 files. The crate denies `rustdoc::broken_intra_doc_links` via
  `[lints] workspace = true`, so this was a hard error rather than a warning — but the only
  doc job built `--all-features`, where every gated target exists and therefore every link
  resolves. The blind spot was structural: an all-features doc build cannot see this class.

  Each affected link now carries a per-configuration pair — the gated arm is byte-identical
  to what shipped, the ungated arm renders the same sentence with the unavailable item as
  plain code text rather than a dead link. The targets are all gated `any(std, alloc)`
  (`Verbose` and its family, `StackedTransaction`, the session-point and stacked-transaction
  entry points, `foldrn`, `separated1`), so both configurations render clean prose. No
  crate- or module-level `allow` was added: an exemption whose subject is "all broken links
  here" would silently absorb the next genuinely broken one.
  — *(R11)*

- **Documented what the dispatch tables actually do on a malformed table.** `DispatchOnKind`
  and `FusedDispatchOnKind` resolve duplicate kinds **first-wins** (the lookup stops at the
  first match, so a later duplicate is dead), and an oversized table is checked only by a
  `debug_assert!` — in release, an entry past the branch range can never select a branch and a
  kind found only there classifies as a dispatch miss. Neither is rejected at construction,
  deliberately: refusing at build time would turn a misuse into a behaviour change. Nothing
  moved; the docs now say what the code does. The non-fused constructor also gained the fused
  twin's "or a latched limit boundary" parenthetical on its end-of-input row.
  — *(R11)*

- **Documented the token-level Pratt driver's recovery posture.** When a prefix operator's
  operand never arrives, the driver reports the diagnostic and then returns **the operator
  token itself** as the expression; an infix operator missing its right operand yields the LHS
  alone. Under a recording emitter the parse continues carrying that stand-in. The token driver
  has no node to withhold, so this is deliberate — callers wanting the stricter posture use the
  typed driver — but it was written down nowhere and had no cell. Both exits are now documented
  and the prefix exit is pinned.
  — *(R11)*

- **Corrected where `finish_partial` places a trailing gap.** The tiling comment said the tail
  tiles "into the root". Measured: the tail is tiled **before** the open frames are closed, so
  when the stream ends with a node still open the gap becomes a child of the **innermost open
  node**. Under `finish` the question cannot arise — an unbalanced stream is refused — so the
  old sentence was true only for the case `finish_partial` exists to relax. `tree.text() ==
  source` holds either way; it is the placement that differs, and tooling that walks by node
  rather than by text can see it. Behaviour unchanged; the construct now has its first
  placement pin, which is why the sentence was unverifiable for as long as it was.
  — *(R11)*

- **The CAS-less `no_std` source-backend rows say what they require.** Four rows in the feature
  reference read "as the dep allows". `bytes`, `hipstr` and `smol_bytes` reach a refcounted
  buffer through `Arc`-shaped sharing and need atomic compare-and-swap, so they cannot build on
  a target like `thumbv6m-none-eabi`; `bstr` is the CAS-free choice and is pinned by a CI cell
  on that exact target. The CAS-needing three are documented rather than pinned as
  expected-failures, since an expected-failure cell would go green for the wrong reason the day
  an upstream crate gained a CAS-free fallback.
  — *(R11)*

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
  — *(R8, #123)*

- The `cache` module no longer advertises a dynamic, allocator-backed cache with unlimited
  lookahead. No such implementation exists, `alloc` does not add one, and none could serve a
  public peek past 32 tokens — `Window` is sealed at U1–U32. Lookahead beyond the window is
  what transactions are for.
  — *(R9)*

- **A regime change could silently corrupt lexer-error deduplication.** `set_state` and
  `state_mut` share a body that cleared the cache, dropped the parked token and
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
  — *(R9)*

- **A checkpoint rollback is all-or-nothing.** `restore_unchecked` interleaved caller code with
  the facts it was installing: cache eviction,
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
  — *(R9)*

- **An interrupted restore can no longer make an abandoned scan's diagnostics permanent.**
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

  — *(R9)*

- **A token left unconsumed by a `to`-shaped scan stop or by a `try_expect*` / `probe_close`
  decline is now retained even when the configured `Cache` refuses it**, so the cursor, offset,
  `is_eoi`, the `sync_balanced` hole anchor and the lexer-state tally after such a stop are the
  same under every cache capacity — including a zero-capacity `()`, where the token used to be
  dropped and re-lexed by the next call. `ClosePayload::CacheFront` is renamed
  `ClosePayload::AtFront` (crate-internal).
  — *(R5, #116)*

- **`InputRef::lexer()` now resumes under the state that produced the newest retained token**
  rather than under the committed state, so a widening lookahead no longer lexes the token after
  a retained run as if that run had never been scanned. A by-value `Lexer::State` limiter is no
  longer bypassed by the lookahead pattern, and the tokens a drain yields, the final state and
  the diagnostics it emits no longer depend on how deep — or in how many steps — the caller
  peeked first.
  — *(R5, #116)*

- **A non-final EOF now reports the lexer's own end offset** instead of the end of the last item
  it yielded, so bytes the lexer skipped before exhaustion (trailing whitespace, a comment tail)
  are no longer omitted from the `Incomplete` frontier a refill driver reads. The reported offset
  is floored by what was already lexed and clamped to the buffer.
  — *(R5, #116)*

- **`InputRef::foldr_within` no longer drops a run shorter than its window.** The exhaustion
  arm returned the untouched initializer *after* consuming the run into the fold's buffer, so
  every run of length `1 ..= W::CAPACITY - 1` was silently discarded on the `Ok` path. Both
  exits now converge on the single reverse drain, as the sibling `foldrn` already did.
  — *(R7, #117)*

- **A render-freeze suite.** One pinned `Debug` and `Display` string per public error type
  (`tests/render_freeze.rs`). Rendered text is public API that no signature describes and no
  tool reads: `cargo semver-checks` compares shapes, and a derived `Debug` that gains a field
  has the shape it always had. That gap cost this crate twice — `UnexpectedEnd`'s undisclosed
  `terminal` field, and a doubled "expected" that survived in the one carrier nobody had
  pinned. A failure here does not mean something broke; it means a rendered surface moved and
  has to be disclosed, and blessing it is a reviewed diff of the exact bytes a consumer's
  frozen fixtures will see.
  — *(R10)*

- **The `no_std` tiers now execute in CI, and the first run of that cell was red.**
  `cargo build --no-default-features` gated a production-code violation but never compiled a
  test target, so nothing `no_std`-shaped had ever *run* — 1553 tests in a configuration that
  executed zero. Enabling it immediately found a dead-code failure (`scan_tick` has no call
  site without an allocator, since its callers are allocator-gated) that predates this round
  and that no gate could see. Fixed, and the two host runs plus D46b's positive
  `thumbv6m` + `bstr` cell are now CI steps.
  — *(R10)*

- **The input's three monotone witness counters are checked, and loud at the wrap point.**
  `next_ckp_id`, `savepoint_seq` and `cache_pushes` incremented with a bare `+= 1`. The stakes
  are higher than bookkeeping: the checkpoint ids are what make the live-checkpoint list
  sorted-ascending, and this release's `contains` / `pop_through` fast paths are binary searches
  *licensed by* that sortedness — a wrapped id violates it with nothing to say so. Each
  increment now panics by name instead of rolling over, following the CST sink's own witness
  counter. 2^64 is not a practical horizon; the point is that a shipped optimization's
  precondition cannot break in silence.
  — *(R10)*

- **The session-point abandonment law is written where users read it.** An abandoned point
  commits, and the campaign carried that to the end as an open *policy decision* between
  commit, rollback and a hybrid. Two of the three are not buildable: `Session::drop` holds the
  lineage and the emitter but not the input, so it cannot restore span, state or cursor —
  "rollback on abandon" is unwritable from that site, and the one thing that is writable
  (rewinding emissions while keeping the position) is the torn state the settle discipline
  exists to prevent. A drop-time `debug_assert` is refused too: abandonment via `?` through an
  enclosing rollback is a legal history. Documented, not changed.
  — *(R10)*

- **`commit_probed`'s temporal contract is enforced in debug builds.** A close payload is valid
  only while the committed cursor sits where `probe_close` left it — everything in the gap,
  including the end-state pass the deferred drivers run there, must be cursor-neutral. That
  lived in prose. The payload now carries the probe-time committed position behind
  `debug_assertions` and the commit checks it, so an edit that consumes or rewinds in the gap
  names itself instead of settling the wrong token. `ClosePayload` is crate-internal, so no
  public surface moves.
  — *(R10)*

- **`Fatal`'s `Default` and `Debug` are `Lang`-generic**, like its `Clone`/`Copy` and like
  `Silent`'s twins. They were written `impl<T: ?Sized> ... for Fatal<T>` — `Fatal<T, ()>` only —
  so `Fatal::<E, MyLang>::default()` did not resolve and a branded `Fatal` could not be printed
  at all. Additive: impls widen, and the render at `Lang = ()` is unchanged.
  — *(R10)*

- **A transposed compile-assert now pins something.** `Ignored`'s no-op `SeparatedEmitter`
  assert instantiated the trait as `<'a, Sep, L>` — separator in the lexer slot, lexer in the
  language slot — and compiled anyway, because `Ignored` implements the trait for any parameters
  at all. An assert over an unconstrained type type-checks whatever you hand it. Rewritten at
  the real signature and exercised at both an unbranded and a branded language; transposing the
  slots back now fails the build.
  — *(R10)*

- **`Emitter::emit_lexer_error`'s contract says what an `Err` return means.** Under a rejecting
  emitter the `Err` **is** the delivery: the input layer advances its dedup watermark *before*
  calling, so a later scan over the same region will not offer the diagnostic again. The
  natural reading is the opposite one, which is why it is now written down at the trait and at
  `Fatal`'s impl.
  — *(R10)*

- **Documentation that was false, corrected rather than softened.** Four `find_boundary` docs
  claimed the result "is always a valid slice position" *and* that indices at or beyond the end
  come back unchanged — which cannot both hold, since `len + 1` returned unchanged is not a
  slice position. They now state the rounding direction (**down**), the in-range guarantee, and
  the out-of-range behaviour separately, and the trait-level doc gains the word "down" it was
  missing. `Transaction`'s "the guard is two words" is replaced by the measured truth: the
  guard embeds a whole `Checkpoint` — cursor, span, `L::State`, two `u64` marks, poison
  boundary — 88 bytes with a near-stateless lexer and `O(size_of::<L::State>())` in general.
  Its op-count claims were true and are kept. `utils::cmp`'s backing list now names `BStr` and
  the `smol_bytes` family, which it implements.
  — *(R10)*

- **`UnclosedEmitter::emit_unclosed`'s `FromUnclosed` bound placement is recorded as chosen.**
  It sits on the method rather than on the trait so an emitter that never routes an unclosed
  pair pays nothing; hoisting it would force every implementor's error type to carry the
  conversion unconditionally. Documented so a later cleanup does not "fix" it.
  — *(R10)*

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

  — *(R8, #123)*

- A nested rollback is **linear in lineage depth** rather than quadratic.
- **The scan path costs measurably more, and the wall-clock figure is not yet trustworthy.**
  The ownership that closes item 9 is the cost. It cannot be relocated: the benched path
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

  — *(R9)*

### What CI enforces now that it did not at 0.7.3

A reader deciding whether to upgrade trusts gates, not adjectives. Every one of these was
added because something got through the previous set.

- **The changelog has a structural check.** It is the one load-bearing release artifact no
  compiler, linter or test reads, and it had already failed in production: a rebase resolved it
  as a verbatim union — textually correct, no conflict markers — and silently ate a heading,
  filing six breaking changes under `### Performance` while every gate passed. The check runs
  in seconds with no toolchain and is a merge-blocker, not advisory.
- **The `no_std` tiers execute.** The previous job *built* the crate without `std`, which never
  compiles a test target — so nothing `no_std`-shaped had ever run. Enabling it found a failure
  that predated the change and that no gate could have seen. That configuration now runs
  **1553 tests where it ran zero**, and the CAS-less backing is checked on a real CAS-less
  target.
- **The compile-time diagnostic rails assert they ran.** The trybuild harness pins its
  `.stderr` files to the MSRV toolchain and returns early on any other rustc — passing, having
  compiled nothing. A skip was indistinguishable from a pass. It now reports an executed-case
  count, the CI step asserts the count rather than the exit status, and a separate check
  compares the case directory against the harness's registry by exact set equality, because a
  floor cannot detect a missing member.
- **The docs are built without default features, not only with all of them.** An all-features
  doc build documents every item, so a link to a feature-gated target always resolves there —
  it is structurally incapable of seeing a broken link in a smaller configuration. The bare
  build had never run, and it was failing outright with 39 unresolved links. Acceptance is the
  exit status, not a diagnostic count: rustdoc aborts early, so a count is a floor and a floor
  cannot detect a missing member.
- **The per-version logos legs actually exercise the adapter.** The `logos parity (0.14, 0.15)`
  matrix job existed, but the crate's 79 adapter integration files were gated on a feature that
  implies 0.16, so the job ran 2452 tests containing zero logos-adapter coverage — a green gate
  over an untested configuration. Those gates now name all three versions, and the same job
  runs **6020 tests** per version.
- **A guide-only change is no longer un-gated.** The guide chapters are Markdown compiled as
  doctests, so a blanket `**.md` path filter meant a guide-only pull request landed with zero
  CI.
- **The public API is diffed mechanically against the last published version.** Recall-shaped
  sources — pull-request bodies, commit messages, surface greps — are structurally blind to
  bound additions and to derived-`Debug` movement, and in this release two of those three
  sources turned out not to exist at all for the earliest rounds. The mechanical diff found two
  breaking changes that would otherwise have shipped undisclosed. Advisory-red, because it
  rides rustdoc-JSON on a pinned nightly and can redden without the crate changing.
- **Runtime parity across the three supported logos majors.** The crate advertises 0.14, 0.15
  and 0.16, and until now the per-version matrix only ever *clippy-checked* them — no test had
  executed against 0.14 or 0.15 once. The first run of these cells was red, on a dead-code
  failure that predates this round: an odometer module gated on `std` whose only readers are
  gated on `logos`, so a `std, logos_0_N` test build had two unused functions and
  `#![deny(warnings)]` refused it. **That is the second never-executed configuration this round
  found broken on arrival**, after the `no_std` one. Both now run, and 0.14 and 0.15 pass an
  identical 2231 tests. *(0.16 runs 2324: the adapter's own test module is still hard-coded to
  0.16, so 93 adapter-level tests remain single-version. That gap is real, named, and not
  closed here.)*
- **The deep fuzz sweep runs per pull request.** Seeds `0..50_000`, `#[ignore]`d since it was
  written and never run anywhere. It was expected to need a monthly cadence; measured, it costs
  **0.10s in release** (0.85s in debug — the ratio is what says it is doing the work), so the
  whole cost is a build this job has already paid.
- **Miri covers the `unstable-raw` surface**, `--lib` only. The raw twins are the surface two
  rounds reordered around capture windows, and unit-level is where their coverage is;
  interpreting 94 integration targets a second time is hours for almost nothing.
- **The sanitizer script has a caller.** `ci/sanitizer.sh` existed, worked, and was referenced
  by no job — so it had never run on any branch of this campaign. That is the same class as a
  configuration CI compiles and never executes, one level up: not a gate that covers nothing, a
  gate that nothing invokes. ASan and TSan are now one job each; the target moved from a
  hard-coded constant into the matrix, because which sanitizers exist is a property of the
  target.
- **The reference consumer is checked at a pinned ref**, on demand and weekly. Per-round human
  discipline for the whole campaign, and this is the last round, so it becomes infrastructure
  or it stops happening.

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

— *(R8, #123)*

### Known limitation — a parse and the emitter serving it share no identity (one witness closed, one open)

Nothing in the `Emitter` or `ParseContext` contract lets an emitter learn which parse it is
serving: `Cursor` carries an offset and a `PhantomData`, and `commit_token` carries a token and
a span. R8 filed this as an invariant with **two** witnesses and said the fix was *"one contract
with two additions"*. **One of the two landed in this same release; the other did not**, so the
statement is rewritten witness by witness rather than left describing a tree that no longer
exists.

**Witness 1 — a sink bound to a foreign source — CLOSED where an inequality is proof.**
`Emitter::bound_source` (defaulted, `None`) lets an emitter declare the source it binds, and the
point at which an emitter is attached to an input compares that against the source the parse
reads. Since #128 that point is the input's **constructor** — where the two are bound for the
input's whole life — so the comparison happens once per input and covers every borrow. A `Sink`
built over `"XY"` and driven over a same-length `"ab"` now **panics at the parse entry** instead
of materializing a tree whose text is `"XY"` and whose structure came from `"ab"`. Equal length was always the point: no length check caught it, and `finish` never could,
because the event log a wrongly-paired sink produces is byte-identical to one a legal single
parse could produce. The cell that pinned the wrong answer is now
`sink_bound_to_a_foreign_source_is_refused` and asserts the panic.

**The residue, narrowed and stated rather than dropped.** The refusal fires only where
`Source::REFERENT_IS_BYTES` says an unequal reference *proves* an unequal source — the `?Sized`
backings (`str`, `[u8]`, `BStr`), where the reference is the data. For a **sized** backing the
reference addresses a variable rather than the bytes: two `bytes::Bytes` handles cloned from one
`Arc` name the same buffer at two addresses, and refusing them would convert a working program
into a runtime panic. So for the owned handles — and for the `&str` / `&[u8]` reference forms —
the door stays open, exactly as it was. That is the honest price of refusing to break correct
code, and a third party closes it for their own backing by overriding one `const`.

What the check covers is wider than "3 of 13 `Source` impls" suggests, because the population
that matters is *declaring lexers*: every lexer in this tree declares `type Source = str` or
`= [u8]`, and the logos adapter inherits logos' own. **No shipped lexer declares a reference
form.**

**The wrapper hole is NOT closed, and four shapes reach a wrong-buffer tree.** The seam can
only compare an answer it receives, and every route below produces silence or a forged answer
instead:

- **A wrapper that hides `bound_source`.** It forwards every emission and omits that one
   method, so the seam concludes "this emitter binds no source". No bound catches it: a wrapper
   writing `impl CstEmitter for W {}` satisfies `Ctx::Emitter: CstEmitter` while inheriting
   every default, and a provided-method override is invisible to a bound.
- **A parse whose events are all diagnostics.** No token is committed, so nothing token-shaped
   exists to key on, while the lexer diagnostics' spans still license gap tiling over every
   byte.
- **Pre-arming.** `Emitter::bound_source` is a public trait method and `InputRef::emitter()`
   hands parser code `&mut Ctx::Emitter`, so any holder can query the binding. Nothing a sink
   records about "I was asked" can distinguish the parse entry from anyone else.
- **Hand-emitted `cst_token` spans.** The manual emission door carries a span, so a grammar can
   choose which source bytes the tree shows while consuming nothing.

**A finish-time wall against (1) was built during this round and removed before release.** It
caught the naive wrapper and each of (2), (3), (4) was then found to walk around it — every fix
relocated the hole, because the witness was flags on the sink and *the sink cannot tell who set
them*. Shipping a `FinishError` variant that names a protection with three known bypasses is
the same defect as a `## Panics` section promising a panic the code does not perform, and this
release deleted nine of those. All four shapes are pinned by cells asserting today's answer.

**What closes the class is minting the sink from the input** — `input.sink(inner, profile)` —
which makes the binding neither hideable nor forgeable because there is nothing to hide or
forge: the sink takes the input's own borrow. It is R8's own named successor and it needs its
own design and adversarial cycle, which is why it is 0.9 and not a late addition here.

**Witness 2 — the completeness witness — UNTOUCHED.**

**Witness 2 — the completeness witness — UNTOUCHED.** `CstEmitter`'s contract still names no
completeness parameter, no attempt and no input, so any holder of the emitter may append events
at any moment and the event log still cannot distinguish the live attempt from an abandoned one.
Every route is an instance of that: the `InputRef` and `ParseState` accessors that stay generic
over completeness — deliberately, because `Partial` legitimately needs them for **diagnostic**
emission — and every combinator that hands either to a user callback. Pinning individual
combinators to `Complete` walls an instance; it cannot wall the class, which is why this stays
contract work rather than a door-by-door patch.

The stale-structure witness — an abandoned attempt's node surviving into a later attempt's tree
— is closed for the session route by the `ValueKeyedEmitter` bound above, and its test was
deleted rather than weakened, because the shape it exercised no longer compiles.

**So R8's sentence still holds, and now reads "one landed, one open".** A *source* identity
could not live on `CstEmitter`, because it would miss `Sink::commit_token` — the path every
ordinary parse uses — which is why it sits on `Emitter` and is checked where an emitter meets an
input. A *completeness* witness on the same contract is what closes witness 2 and what returns
streaming CST.

**A better repair exists and is deliberately not this one.** Minting the sink from the input or
session — `input.sink(inner, profile)` — hands the sink the input's own borrow, making a foreign
binding **unrepresentable** rather than detected, closes witness 2 at the same time, and is "the
mechanism that restores streaming CST". It is a new public constructor surface and a consumer
migration, so it is a feature for a later release rather than something to land in the round
whose job is to publish. **When it lands, `Emitter::bound_source` and
`Source::REFERENT_IS_BYTES` are retired rather than accumulated** — two mechanisms for one
invariant is how a crate ends up with three.

Refusing any of this at materialization remains **impossible**, not merely expensive: the bad
event logs are byte-identical to logs a legal single parse could produce, so `finish` has
nothing to discriminate on. That is why the check has to sit at the seam or nowhere.

— *(R8, #123; witness 1 closed by R10)*

### Known limitation — `finish` does not refuse duplicate zero-width token spans

Separate and smaller, listed apart because it is **not** the identity gap: the monotone-span
wall never fires when duplicated spans are both `[0,0)`, so such a log materializes. A
conforming lexer cannot produce one — a token is a span of consumed bytes, and a lexer
returning zero-width tokens without advancing does not terminate — so this is reachable only
through the raw event surface or a third-party `Lexer` that ignores the contract, which is
what the sink's release walls exist for. It pins a property of *materialization*, not of the
parse-to-emitter contract, and it flips when `finish` starts refusing these spans. —
`duplicate_zero_width_tokens_are_not_yet_detected`

— *(R8, #123)*

<a id="0.8.0-known-limitation--a-discarding-error-sink-erases-the-recursion-trips-stop-not-its-bound"></a>

### Known limitation — a discarding error sink erases the recursion trip's stop, not its bound

Item 42's `RecursionLimitReached` is always terminal, and the recovery combinators decide whether
to re-raise by asking the **converted** error — the type the grammar actually names — for
`MaybeTerminal::is_terminal()`. So a grammar whose `From` for it discards the value, `()` included,
gets `false` back and **recovery spends the trip** instead of re-raising it. That is the
pre-existing `MaybeTerminal` opt-out reaching a *resource guard* rather than a malformed-input
report. The conversion was deliberately not removed and the contract deliberately not redesigned:
16 error types under `error::` carry a `From<…> for ()`, 14 of them before this release, so these
are two more instances of a crate-wide design rather than a new one.

**What is not lost — measured, not reasoned:**

- **The native stack is already back.** `Descent`'s destructor releases every level on the unwind
  that carries the error out, so by the time any recoverer sees the converted value, every frame
  the budget was protecting has returned. On a 200-level trip the recoverer's frame sits **160
  bytes** from the pre-parse baseline in a debug build and **0** in a release one, against descents
  that reached ~1 MiB and ~97 KiB. The guard's stack-safety purpose survives the sink intact.
- **The depth budget is unchanged.** The cell reads back to its pre-parse value, so a recoverer
  that spends a trip and descends again starts from the same depth and meets the same limit.
  Re-tripping is bounded, not compounding.
- **The retries terminate**, on their own zero-progress guards rather than on terminality:
  `skip_then_retry` consumes its sync token per continuing cycle and runs out of sync points, and a
  repetition over a recovering element stalls on the first element that commits nothing.

**What is lost is the stop, and the input it costs to discover there was one.** Measured on one
ladder, one limit and one 32-deep input under `skip_then_retry`, the verdict is `Err` either way —
a caller matching on the discriminant sees no difference at all. The difference is where the input
is left:

| the grammar's error type | result | input handed back to the surrounding grammar |
|---|---|---|
| delegating (stores the value) | `Err` | offset **0** — re-raised before any skip, nothing consumed |
| `()` (discards it) | `Err` | offset **68** — the 32-deep chain *and* the sync token consumed and committed |

Skipping is how recovery buys progress over a *malformed* construct, and a depth budget is not one:
no quantity of skipped input makes the next descent shallower, so those cycles spend input for a
verdict they cannot change.

**If your grammar needs the stop, give the error type a variant that stores this one and delegate
`is_terminal`** — the type's own documentation carries that as a compiling example. The `()` sink
stays available and means exactly what it says: that no error of any kind carries information,
resource trips included.

**`NonAssociativeChain` is the control here, and it is inert.** It is `is_terminal() == false` to
begin with, so recovery spends it through a delegating type and through `()` alike and the sink
decides nothing; what a sink costs there is only the offset, which is the ordinary price of
discarding a payload and not a change of contract. The two conversions items 41 and 42 add are
therefore not one hazard — only the resource guard's is load-bearing.

**And the class is older than this release.** `UnexpectedEnd` — the type behind `UnexpectedEot`,
`UnexpectedEoLhs` and `UnexpectedEoRhs`, whose terminal values are real driver output — is the only
other type under `error::` that overrides `is_terminal`, it carries a `From<…> for ()` of its own,
and both have shipped since before this campaign. Whether a discarding sink should be able to erase
terminality for a resource guard as opposed to a grammar error is a question about `MaybeTerminal`,
not about these two types, and it is recorded here rather than answered.

— *(R16)*

### Notes

`#[diagnostic::do_not_recommend]` was evaluated and rejected. It does not change whether the
curated message appears; its only effect is to hide *which* member is missing, which is the
one datum a consumer needs. Measured on 1.87 and on nightly.

— *(W-api-A, #118)*

**`finish_*` unification attempted and stopped, on the close predicate and close-miss slots.**
`parens`/`braces`/`brackets`/`angles` and `delimited::<D>` share their close body already
(`commit_delim_close`); what the four named parsers duplicate is the five slots they fill it
with. Folding them onto the generic path stops on the two slots that ask the same question
through different traits: the named route asks the **token** (`PunctuatorToken`'s
`close_paren() -> Option<Kind>`), the generic route asks the **pair marker**
(`Punctuator::kind()`, which needs `<L::Token as Token>::Kind: From<CloseParen>`). Neither trait
implies the other, so the fold does not compile without adding `Kind: From<OpenX>` and
`Kind: From<CloseX>` to the four public named parsers — measured, four `E0277`s. Unification here
is signature-stable or it does not happen; the two routes stay disclosed, and 0.9 is equally
breaking-capable if the bounds are ever worth taking.

Their agreement is now pinned rather than assumed (`tests/delimited_route_parity.rs`): the close
predicate over every token kind × all four pairs, the close-miss error compared as a value, and a
16-row corpus — 4 pairs × {unclosed-at-EOF, wrong-closer, wrong-opener, nested-unclosed} — parsed
both ways under a recording emitter. **No render deltas: all 16 rows are identical**, diagnostics
and final cursor alike. The suite exists because nothing in the type system ties a token type's
two punctuator declarations together, so a type whose declarations disagreed would send the two
routes to different diagnostics silently.
— *(R11)*

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
