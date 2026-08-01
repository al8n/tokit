# The event-stream CST engine

The [checkpoint chapter](super::arch_checkpoint_rewind) closed on a composition it *sketched*: a
recording sink that buffers a second log — the CST event stream — under the very same mark the input
already uses to rewind position and diagnostics, so a declined branch drops its tree exactly as it
drops its tokens. The [atomic-emitter chapter](super::arch_atomic_emitter) one-lined the sink itself:
[`cst::Sink`](crate::cst::Sink), the recording [`CstEmitter`](crate::emitter::CstEmitter) that wraps
an inner diagnostics emitter, forwards its diagnostics, and *also* records structure. This chapter
owns the sink's internals — the deepest seam in Part III. It keeps the part's register: less *how to
call it* (that is [chapter 16](super::ch16_lossless_cst), the tutorial, which deliberately treats the
event stream as an implementation detail), more *why every mechanism is shaped the way it is*.

The details it documents are load-bearing: an era that rewinds by one, a witness dropped from a
mark, a gap tiled without a covering diagnostic — each is a *wrong tree with no witness*, the
failure class the whole design is bent to make unrepresentable. So this chapter is precise about
the invariants, not just the shapes.

## Events, not eager nodes

The obvious way to build a tree while parsing is to call a builder as you go: open a node here, push
a token there, close the node on the way out. It is also the way that does not survive backtracking.
A tree builder holds interior state — a stack of half-open nodes — that a speculative branch mutates
as it parses; when the branch declines, that state has to be *surgically undone*, node by node, or
the tree keeps a phantom of work the parse rolled back. An eager builder like rowan's
`GreenNodeBuilder` — which tokora exposes thinly as
[`SyntaxTreeBuilder`](crate::cst::SyntaxTreeBuilder) — has no rollback of its own, so tokora drives no
builder at all *during* the parse; it drives one exactly once, at materialization, from the finished
log.

Instead — following the lineage of rust-analyzer and Biome — tokora records the parse as a **flat log
of events** and *derives* the tree from the surviving events exactly once, at the end. An event is a
tiny value: *open a node of this kind*, *here is a committed token*, *close a node*. Nothing is built
while parsing; the log is just appended to. The payoff is the whole reason the design exists:

- **Backtracking rewinds the tree for free.** The events live in the emitter's rewindable channel, so
  the one mark that truncates diagnostics on a declined branch truncates its tree events in the same
  motion. There is no tree surgery on decline, because there is no tree yet — only a log, and a log
  rewinds by throwing away its suffix. A rolled-back branch's structure vanishes exactly as its
  cursor movement does.
- **Materialization is a single validated pass.** Building once, from a complete log, means the
  builder is only ever driven with already-checked operations — so rowan can never panic under it,
  and every losslessness and balance law is enforced in one place (see
  [materialization](#materialization-one-walk-that-builds-and-validates) below).

The event stream is therefore not an optimization detail; it is the data structure that makes
*"the tree participates in the one rollback contract"* true rather than aspirational.

## The event vocabulary

The vocabulary lives in [`cst::event`](crate::cst::event), and it is rowan-free — it compiles in
every build, because the *recording* half of the CST design (the marks, the `node` combinators) is
unconditional; only the *materializing* half is gated on `rowan`. The log itself is a `Vec` of one
crate-internal `Event` enum, and the governing law is stated up front: **an event buffer changes in
exactly two ways** — *append* (the `cst_*` emission methods) and *suffix-truncate* (a
[`rewind`](crate::Emitter::rewind)). No operation ever rewrites the *kind* of an interior slot. That
two-verb discipline is what makes rewind-by-truncation exact: the prefix below any live mark is
immutable, so truncating to a mark restores the buffer to precisely the state it had when the mark
was captured. (There is one journaled exception — an acceleration field — developed with the sink
below; it is legal *only because* it is reversed on rewind, so the law holds observationally.)

### The five events

- **`StartNode { kind, forward_parent }`** opens a node of `kind`, closed by a matching `FinishNode`
  — *unless* `kind` is the reserved [`TOMBSTONE`](crate::cst::event::TOMBSTONE) value (`u16::MAX`), in
  which case the slot is an **inert mark** that pairs with no finish and materializes into nothing.
  The `forward_parent` field is the journaled acceleration; ignore it until the sink.
- **`Token { kind, span }`** is one committed token: its dialect-mapped kind and its source span,
  appended exactly once per settled token. Peeks, declines, and unconsumed stoppers append nothing —
  only a *settle* records a token (the [`commit_token`](crate::Emitter::commit_token) hook the
  emitter chapter named).
- **`FinishNode`** closes the innermost open node — plain stack discipline.
- **`StartAt { kind, target }`** retro-opens a node of `kind` at the buffer position of the tombstone
  named by `target`. This is the **append-only form of retro-parenting**: rather than rewrite a
  tombstone into a real start in place, you *append* a `StartAt` that names it. Same-target `StartAt`s
  open in **reverse buffer order** at materialization — the later wrap becomes the *outer* node,
  because its finish is necessarily appended later. The in-place alternative (rewriting the
  tombstone's kind) is banned by law: an interior write below a live emitter mark would survive the
  truncation that was supposed to erase the branch that made it.
- **`Diag { error_span }`** is a forwarded-diagnostic slot — a marker in the *event* log for one
  diagnostic that was forwarded to the wrapped emitter, on `Ok` and `Err` alike. It is skipped at
  materialization, with one exception that is the design's single deliberate channel coupling: a
  **lexer-error** slot carries the offending source span in `error_span`, so `finish` can tell a byte
  a lexer legitimately refused from a byte a dropped token lost. Living *in the event log* means the
  span rewinds with the branch that saw it — an abandoned lexer error stops covering anything, for
  free. Every other diagnostic (unexpected token, missing element, …) points at tokens that settled
  or at zero-width absences, covers no gap, and stores `None`.

Balance is **derived** from the log, never cached beside it: a real `StartNode` or a `StartAt` is
`+1`, a `FinishNode` is `−1`, and a tombstone, a `Token`, and a `Diag` are all `0`. A malformed
buffer is *representable* — the raw `cst_*` surface is sharp on purpose — but it is *unrepresentable
as a successful materialization*: `finish` walks the log and returns a typed error rather than
building a wrong tree.

### Marks carry an era and a witness

A retro-wrap needs a handle to the tombstone it will anchor at. That handle is an
[`EventMark`](crate::cst::event::EventMark), and the subtle part is that *index-in-bounds is not
validity*. Truncate-and-regrow is the normal backtracking rhythm, so "the buffer has an event at
index 3" says nothing about whether that event is still the tombstone a mark was minted for — a
rewind can truncate it away and unrelated events can regrow over index 3. So a mark is a **positional
witness plus identity**, three fields:

- **`index`** — the tombstone's buffer slot;
- **`era`** — the truncation history the mark was issued under;
- **`sink`** — the identity of the one recording sink that minted it.

Two `(index, era)` pairs coincide trivially — two fresh sinks both mint `(0, 0)` — so identity is not
optional decoration. A recording sink validates all three at *every* spend and **panics in every
build** on a stale *or foreign* mark. This is the savepoint posture, and it is deliberate: both are
parser bugs (the branch that conceived the wrap was rolled back, or the mark belongs to another parse
entirely), not input-dependent conditions, and the silent alternative — wrapping whatever sits at
that index — is a wrong tree nothing downstream can detect. An emitter with no event channel returns
an **inert** mark (index `u64::MAX`, the reserved witness id `0`), which fails a recording sink's
identity wall deterministically rather than wrapping anything.

Why brand marks at all, rather than trust the parser to only spend live ones? Because stale- and
foreign-mark misuse is precisely the bug class earlier iterations shipped, and the type system cannot
catch it: a mark is `Copy` and may legitimately outlive its combinator frame (a pratt driver holds
one across arbitrarily many operator iterations, spending it once per fold). Branding moves the
detection from "hope" to "panics at the spend, at the cause."

## The rewindable sink

[`cst::Sink`](crate::cst::Sink) is where the vocabulary meets the emitter contract. It wraps an inner
emitter `E`, forwards the *entire* emitter trait family to it (so any context bound `E` satisfies,
`Sink<E>` satisfies too), and buffers the event stream — one rewindable timeline for tree and
diagnostics alike. Its cells are classified by the same `CELL_CENSUS` discipline the
[checkpoint chapter](super::arch_checkpoint_rewind) described for the input layer: a crate-internal
function destructures the sink exhaustively — no `..` — so a new field cannot be added without
declaring which class it is in and what a rewind must do to it.

### One timeline: events beside diagnostics

The sink's [`checkpoint`](crate::Emitter::checkpoint) is simply the event-log length: one positional
mark over one unified log, exactly `Verbose`'s architecture. Every diagnostic forwarded to the inner
emitter occupies a `Diag` slot *inside* the event buffer, appended by one census-marked helper on
`Ok` and `Err` alike (record-then-propagate: a fatal unwind that skipped the slot on the `Err` edge
would drop an `error_span` a later `finish` needs). So the whole of [`rewind`](crate::Emitter::rewind)
is: truncate the buffer to the mark, reverse-replay the undo journal, and rewind the inner emitter to
the reading its mark-stack row captured. One mark governs both channels because both channels *are*
the one log — the tree events and the diagnostic order-slots interleaved on a single timeline.

There is deliberately **no** `&mut` accessor to the inner emitter, only a shared
[`inner_ref`](crate::cst::Sink::inner_ref). A caller who could drive the inner emitter's `rewind`
directly would shear the event log from the diagnostic log with no witness — the exact desync the
one-timeline law forbids. Ownership of the inner comes back only from materialization, which consumes
the sink.

### The mark stack, the journal, and the era ledger

Three cells carry the rewind machinery, and each has a *distinct* restore rule — the reason the
census matters.

The **mark stack** (`rows`) holds one `MarkRow` per live checkpoint capture. A row is three frozen
facts: the captured `mark` (the event-log length), the derived open-node `depth` at capture time, and
the **inner emitter's own checkpoint reading**, captured by value. Depth is a frozen fact about a
prefix, never a live counter — there is no depth counter anywhere in the sink; every query recounts
the events above the nearest frozen row (or the released *floor*, a memo of the newest settled row
that keeps recounts short across commit-heavy loops). A cached counter would need its own restore
rule; a derived one is restored by truncation for free. Each row is spent by *exactly one* of
[`release`](crate::Emitter::release) (the branch was kept) or `rewind` (it was abandoned) — the
settle discipline the input layer's release census locks.

The **undo journal** exists for the one law-breaking write the design permits. When
[`cst_start_at`](crate::emitter::CstEmitter::cst_start_at) appends a `StartAt`, it also writes back
onto its *target tombstone* a `forward_parent`: the relative offset to the newest `StartAt` naming
that tombstone. This is an in-place mutation of an interior slot — otherwise banned outright — and it
is legal *only because every write is journaled*. The journal records `(at_len, index,
old_forward_parent)`, and a rewind reverse-replays the entries whose `StartAt` died, restoring each
overwritten value newest-first. The pointer is never required for correctness — materialization
recovers every wrap from the `StartAt` events themselves — but it is both an acceleration and an
integrity canary: `finish` checks that a set `forward_parent` still names a live `StartAt` of its
target, and the **dangling pointer of an abandoned branch** (a
[`DanglingForwardParent`](crate::cst::FinishError)) is exactly the silent corruption the journal
exists to kill. In-place mutation plus a reverse-replay journal is the pure-copy discipline of the
checkpoint chapter, lifted to events.

The **era ledger** (`TruncationLedger`) is the cell that makes stale marks detectable, and it is the
one most worth being exact about. It is two parts — a monotone **era source** and a merged
**truncation stack** — and both are *monotone, never rewound*. This inverts the usual rewind
instinct, and the inversion is the point:

- The era source is bumped by `+1` on every recorded truncation and **never rolled back**. Rewinding
  it would reissue an era a dead mark was minted under, and let that dead mark validate.
- The truncation stack is a *witness of truncations*: a rewind **appends** to it (a rewind *is* a
  truncation) and never removes from it. Forgetting a truncation would false-accept exactly the stale
  mark the record existed to kill.

A mark is stale iff **some truncation younger than the mark's era reached the mark's index or below**.
The staleness query is one binary search: the stack is kept strictly increasing in both era and
low-water mark (a new truncation subsumes every recorded truncation at an equal-or-higher low-water
mark — anything an older, shallower entry would invalidate, the newer, deeper one also invalidates —
so subsumed entries are merged away on push). The entries younger than a mark's era are therefore a
suffix, the smallest low-water mark among them is that suffix's first entry, and a single lookup
decides. A truncation strictly *above* a mark's index leaves it live; one that *reaches* its index
kills it forever; and a mark issued *after* a truncation is untouched by it. Regrow-then-truncate-
shallow keeps both records, because they invalidate different ranges. This is the mechanism behind
the flat claim "truncation makes old marks stale forever."

The sink's identity — the `witness` stamped into every mark — comes from a process-unique, 1-based
atomic counter (`0` is reserved for the inert mark). It is minted unconditionally in every build,
because the witness is the *every-build* half of mark validation, and it is **never reissued**: the
allocator is a `fetch_update` that *aborts* on overflow rather than wrapping `usize::MAX` back to `0`
— a wrap would be doubly wrong (`0` is the inert id, and every id after it reissues a live one). Sinks
move and a dead sink's address can be reused, so an address would not do; a monotone counter is never
reused for the process's life.

### The value-keyed inner, developed

Here is the composition the checkpoint chapter sketched and deferred, developed in full. The sink
composes with its wrapped emitter through checkpoint **readings**, never mark **resources**:
`checkpoint` captures `inner.checkpoint()` onto the mark-stack row as a plain `u64`, `rewind` hands a
captured reading back to `inner.rewind`, and `release` pops the sink's own row *without forwarding* —
the inner is never told about kept branches, because a kept reading is just a number going out of
scope. This requires the inner to be **value-keyed**: a pure monotone `checkpoint`, a drop-by-value
`rewind`, a no-op `release` — the shape of [`Verbose`](crate::emitter::Verbose),
[`Fatal`](crate::emitter::Fatal), [`Silent`](crate::emitter::Silent), and
[`Ignored`](crate::emitter::Ignored), and of every `Verbose`-shaped collector. (A *table-keyed* inner
that allocated per-`checkpoint` bookkeeping is explicitly unsupported here; it belongs at the input
layer's direct seam, where the settle discipline is 1:1.)

The rule that keeps the two logs pinned together is that the sink rewinds the inner **only to a
reading it knows exactly** — it never fabricates one. On a `rewind` to `mark`:

- The sink spends the mark-stack captures at or above `mark`: everything strictly above dies with the
  branch, and the newest capture *at exactly* `mark` is the row being rewound to — its stored inner
  reading is the exact target. Every disciplined path (a guard, an [`attempt`](crate::InputRef::attempt),
  the scan family, a correct raw save/restore) lands here.
- If nothing was truncated — `mark` equals the current length — the inner is left **untouched**. The
  surviving events are the whole log, so every inner-side record they reference must survive too; this
  is the trait's rewind-to-current no-op law, upheld on every channel.
- For a no-row unwind to the **origin** (`mark == 0`, an empty event log), the target is the inner's
  **construction-time reading**. That reading is primed lazily at the first inner-advancing touch (a
  forwarded diagnostic or a settled token — the sink's only two advancing surfaces), and it *provably*
  equals the reading at construction: the sink exposes no `&mut` path to the inner, so the inner
  cannot advance before the sink's own first advancing call, and every advancing surface primes the
  base before forwarding. An empty event log therefore pairs with exactly the construction reading.

Everything else is **refused rather than guessed**. An out-of-range *future* mark — one strictly
*above* the current length, naming a log position that does not exist yet — is a **total no-op on
every channel**: events, the mark stack, the floor, the journal, the era ledger, and the inner alike.
(Clamping it to the current length instead — the pre-redesign behavior — would let a future mark spend
the live row of a *real* checkpoint taken at that length, and that checkpoint's own later rewind would
then find no row: the desync.) And a truncating rewind to a *mid-log* mark that **no live row
captured** has no exact inner reading anywhere — the mark was never returned by `checkpoint`, or its
capture was already spent. That is undisciplined raw use: debug builds panic at the cause (the
sink-level twin of the input layer's LIFO witness), and release builds keep the sink's own channels
exact and leave the inner untouched — one-sided staleness that preserves every inner-side record the
surviving prefix still references, never a fabricated reading that would destroy committed inner
state. The sink hands the inner only readings it can prove, or nothing.

One more append-shaped write deserves a note, because it is the single censused exception to
"append + suffix-truncate": the **recovery-hole wrap**. When [chapter 8](super::ch08_recovery)'s
recovery skips a garbage region, the sink brackets the hole's already-buffered token events in a
`StartNode(error_kind) … FinishNode` pair. Those tokens are the buffer's *suffix* by construction —
they settled during the scan, after every live mark was captured, and the scanner runs no user code —
so the wrap is a *prefix-preserving splice* entirely above every live mark: one insert at the first
hole token, one appended finish, with the journal's positions bumped to stay exact. It never disturbs
a slot any live mark can name, which is why it is a lawful member of the append family rather than a
violation of it.

## Materialization: one walk that builds and validates

[`finish`](crate::cst::Sink::finish) consumes the sink and turns the surviving events into a rowan
green tree in a single forward walk — driving the builder only with operations it has *already*
checked, so rowan can never panic under it, and returning a typed
[`FinishError`](crate::cst::FinishError) on the first violation instead. It **never panics**. The one
walk enforces, together: balance (an orphan finish or a leftover open is a typed error — rowan's
silent one-level absorb under a root wrapper is unreachable, because the walk refuses first);
retro-wrap integrity (a `StartAt` whose target is not a live tombstone is a
[`StaleStartAt`](crate::cst::FinishError); a dangling `forward_parent` is the journal's finish-time
canary); kind hygiene (the reserved tombstone band); span discipline (monotone, non-overlapping,
in-bounds, `u32`-fitting); and the two losslessness laws below.

Same-target wraps are resolved in a pre-pass that groups the `StartAt`s by target and validates every
`forward_parent` canary; the main walk then opens each target's wraps *latest-first* at the
tombstone's position (so the last-declared wrap is the outermost node), and a hoisted wrap that would
close *before* its own declaration is an [`ImproperWrap`](crate::cst::FinishError) — a wrap crossing a
node boundary instead of enclosing whole subtrees.

### Gap-tiling and the coverage law

Losslessness — `tree.text() == source`, byte for byte — is **structural**, not a property the lexer is
trusted to provide. As the walk lays down committed tokens in source order, any run of source bytes no
committed token covers is *tiled* with a `gap_kind` token in the currently open node. A skipped-
whitespace region, an undrained tail, a poisoned truncation: whatever the events left uncovered
becomes a gap tile, so the round-trip holds for *every* input, lexer errors included.

"The currently open node" is more precise than it looks, and the precision is the whole of the
placement rule: **a gap is tiled where it opens**, not where it is noticed. An uncovered run opens the
instant the token before it settles — that is the moment the parse stopped covering the source — so it
is emitted immediately after that token, in the node open then, and it is in the tree before the next
event is read. The **trailing** run is therefore not a case: the bytes after the last committed token
are tiled by that token like any other run, so `Root[Document[Tok] Gap]` is `Root[Document[Tok Gap]]`
and the node widens over the tail. The one clause left over is a run that **no token precedes** — a
source that begins with bytes no token claims — which has no such moment and so tiles where the walk
first sees it: at the first committed token, or, with no committed token anywhere, at the end of the
walk, in whatever node is open there. A wholly unlexable source consequently keeps its run beside the
document node rather than inside it, and [`finish_partial`](crate::cst::Sink::finish_partial) tiles
before it closes the frames an unbalanced stream left open, so there the fallback lands in the
innermost **open** node.

Two invariances follow, and they are why the rule is stated at the *opening* moment rather than at any
later one. First, nothing that follows a run can move it: two streams sharing a prefix through the
token a run trails place that run identically, including when one of them simply stops there — so
placement never depends on whether more input happened to follow. Second, no **diagnostic** can move it
either, because placement reads the token and structure events only. That second one is load-bearing
rather than tidy: a prefilled lookahead cache emits the lexer errors it crosses when it crosses them,
so prefetching *hoists* a lexer-class diagnostic earlier in the event stream. The token stream is
exactly invariant under prefill and the diagnostic stream is not, so a rule that read a diagnostic's
position would make the materialized tree a function of how far the caller happened to peek. Coverage —
the next paragraph's subject — still consults every diagnostic, but through the merged *set* of
recorded spans, which is order-independent for the same reason.

But tiling is not unconditional, and the condition is the design's **one deliberate coupling of the
diagnostic and CST channels**. Elsewhere the two are independent — a `Diag` slot is invisible to the
tree. At `finish`, though, a tiled byte must be *explained*: `finish` tiles a gap only where a recorded
**lexer-error** diagnostic covers it (the lexer saw bytes it could not tokenize, said so, and
committed no token there). A gap with no covering error and no covering token is a **dropped committed
token** — the partial-forwarding-wrapper signature — and is refused as an
[`UncoveredGap`](crate::cst::FinishError::UncoveredGap) rather than dressed up as a plausible-but-lossy
tree. This is why the lexer-error span rides *in the event log* (the `error_span` of a `Diag` slot):
so it rewinds with its branch, and so its *span* is available to license its gap at exactly the one
moment the channels are allowed to cross.

Two more walls guard the same seam. A balanced stream that builds structure but carries **no committed
token at all** over a nonempty source *no lexer error explains* is a
[`StructureWithoutTokens`](crate::cst::FinishError): the signature of a wrapper emitter that forwarded
the `CstEmitter` structuring surface but inherited the no-op `commit_token`, so every token silently
vanished. It is refused ahead of the gap-coverage law so the all-dropped case earns that precise
message rather than an uncovered-gap report over the whole source. The qualifier is what keeps the wall
off a legitimate shape it would otherwise share a symptom with: the wall reads the **same** evidence the
coverage law does, and a **lossless** grammar opens its root node before it can know whether any token
follows — so a buffer holding nothing lexable (an unterminated string mid-edit) reaches `finish` as one
error span, one open-then-closed node, and zero tokens. Every byte is explained there, so that tree
tiles rather than being refused.
And [`finish_partial`](crate::cst::Sink::finish_partial) is the tooling door for an *incomplete*
parse: it **closes** open nodes rather than reporting [`UnclosedNodes`](crate::cst::FinishError), and
**tiles** every gap rather than refusing an uncovered one — the two ways an incomplete parse
legitimately differs from a complete one (a fatal abort leaves nodes open; a fail-fast lexer error
leaves an un-diagnosed tail). Every other law is enforced identically; the exemptions are exactly the
incompleteness signals, nothing more.

### The compile-time trivia wall

The coverage law only closes if *every source byte reaches the sink* as a token or a reported lexer
error. A lexer that silently skips whitespace breaks that premise: a skipped-whitespace gap would be
**indistinguishable** from a dropped committed token, and the sink could not tell the lossless case
from the corrupt one. So [`Sink::new`](crate::cst::Sink::new) refuses a skipping lexer at **compile
time**: an inline-`const` assertion on `L::SURFACES_TRIVIA` fires a post-monomorphization error at the
offending call site (at build/test/doc time — *not* under `cargo check`, which never monomorphizes the
call). A lossless sink structurally requires a trivia-surfacing lexer; the wall makes that a
type-level fact rather than a runtime hope. Chapter 16 demonstrates both sides of the wall as compiling
and `compile_fail` doctests.

One materialization-time policy remains configurable: [`TriviaPolicy`](crate::cst::TriviaPolicy). Its
only variant today is the provable one, `AsEmitted` — a committed trivia token materializes into
whichever node was open when it settled (call-site placement), which is deterministic, cache-
transparent, and origin-blind. This is deliberately *not* the Roslyn/Swift "leading trivia attaches
forward" policy; a token-attached view is a later materialization-time extension, which is the only
reason the enum exists at all.

## The combinator surface

You rarely emit events by hand. The [`node`](crate::parser::node()) family is the blessed bracketing
over the sink, and its encoding is worth one architectural note because it explains why backtracking
stays clean: `node` and [`node_opt`](crate::parser::node_opt()) are **append-only and never leave a
node open**. Entry mints an inert tombstone; only a *successful* exit spends it as a retro-wrap
(`cst_start_at` + `cst_finish`). A decline or an error-path unwind leaves the tombstone unspent — no
dangling `StartNode` for a later finish to mispair with, and nothing for a rollback to surgically
undo. The `labelled` finish-on-both-exits discipline, made structural rather than dutiful.

[`node_at`](crate::parser::node_at()) spends a *caller-held* mark — the retro-wrap shape: mark, parse a
prefix, then decide it was the start of something bigger. For the common single-wrap decision,
[`Marker`](crate::cst::event::Marker) wraps a raw mark in a compile-time single-use typestate:
`complete` spends it into a node and yields a [`CompletedMarker`](crate::cst::event::CompletedMarker),
`abandon` consumes it leaving the tombstone inert, and `precede` — a further *outer* wrap — exists
*only* on `CompletedMarker`, so wrapping an abandoned or still-open intent is unrepresentable rather
than merely checked. The raw `EventMark` stays `Copy` and multi-spend for the pratt shape that needs
it. The [combinator reference](super::ref_combinators) catalogs the full surface; the point here is
only that every one of these lowers to the append-only vocabulary above.

## The rewind, in one parse

The headline claim compiles: a speculative branch builds a *whole node* — tokens and structure, all
recorded — then declines, and its events truncate as if they never happened; the real parse then
builds the tree for keeps, and [`finish`](crate::cst::Sink::finish) materializes a lossless tree. The
proof is an equivalence: the same source, parsed straight and parsed through the declined speculation,
materializes **byte-identical green trees**. That equivalence is a tested law of the sink, not an
accident of the example. The lexer is a tiny hand-written lossless `CharLexer` — every byte surfaces
as a token, so `SURFACES_TRIVIA` is honestly `true` — and nothing but core tokora types is in play.

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   InputRef, Lexer, SimpleSpan, Token as TokenT,
#   error::{UnexpectedEot, token::UnexpectedToken},
# };
# use tokora::span::Span as _;
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, Copy, PartialEq)]
# enum Tok { Num, Plus }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Num, Plus }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl TokenT<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   // The lexer surfaces every byte — the compile-time wall on `Sink::new` requires it.
#   const SURFACES_TRIVIA: bool = true;
#   fn kind(&self) -> Kind { match self { Tok::Num => Kind::Num, Tok::Plus => Kind::Plus } }
#   fn is_trivia(&self) -> bool { false }
# }
# struct CharLexer<'a> { src: &'a str, pos: usize, tok: SimpleSpan, state: () }
# impl<'a> Lexer<'a> for CharLexer<'a> {
#   type State = (); type Source = str; type Token = Tok; type Span = SimpleSpan; type Offset = usize;
#   fn new(src: &'a str) -> Self { Self { src, pos: 0, tok: SimpleSpan::new(0, 0), state: () } }
#   fn with_state(src: &'a str, _: ()) -> Self { Self::new(src) }
#   fn check(&self) -> Result<(), Infallible> { Ok(()) }
#   fn state(&self) -> &() { &self.state }
#   fn state_mut(&mut self) -> &mut () { &mut self.state }
#   fn into_state(self) -> Self::State {}
#   fn source(&self) -> &'a str { self.src }
#   fn span(&self) -> SimpleSpan { self.tok }
#   fn slice(&self) -> &'a str { &self.src[self.tok.start()..self.tok.end()] }
#   fn lex(&mut self) -> Option<Result<Tok, Infallible>> {
#     let bytes = self.src.as_bytes();
#     // No skipping: every byte becomes a token, so the round trip is structural.
#     if self.pos >= bytes.len() { return None; }
#     let (start, c) = (self.pos, bytes[self.pos]);
#     self.pos += 1;
#     self.tok = SimpleSpan::new(start, self.pos);
#     Some(Ok(if c == b'+' { Tok::Plus } else { Tok::Num }))
#   }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
use rowan::Language;
use tokora::{
  Emitter, InputRef as In, Parse, ParseContext, ParseInput, Parser,
  cache::DefaultCache,
  cst::{CstProfile, KindValidator, Sink},
  emitter::{CstEmitter, Fatal},
  parser::node,
};

// The dialect's whole u16 kind space: token images, then the node kind, then bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
enum K { Num, Plus, Expr, Error, Gap, Root }

// The sink-side mapper: committed tokens enter the tree through this one match.
fn map_token(t: &Tok) -> u16 {
  (match t { Tok::Num => K::Num, Tok::Plus => K::Plus }) as u16
}

// The dialect's CST profile: the mapper, the predicate that names the whole kind space
// (every discriminant up to `Root`), and the two bookkeeping kinds.
fn profile() -> CstProfile<Tok> {
  CstProfile::new(
    map_token,
    KindValidator::new(|k| k <= K::Root as u16),
    K::Error as u16,
    K::Gap as u16,
  )
}

// Rowan's raw <-> typed bargain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Lang {}
impl Language for Lang {
  type Kind = K;
  fn kind_from_raw(raw: rowan::SyntaxKind) -> K {
    const KINDS: [K; 6] = [K::Num, K::Plus, K::Expr, K::Error, K::Gap, K::Root];
    KINDS[raw.0 as usize]
  }
  fn kind_to_raw(k: K) -> rowan::SyntaxKind { rowan::SyntaxKind(k as u16) }
}

type Ln<'a> = CharLexer<'a>;

// One `Expr` node wrapping every token the sub-parse commits. `node` mints a tombstone on
// entry and — only on success — spends it as a retro-wrap; there is never an open node
// between entry and exit, which is exactly why a rolled-back branch leaves nothing behind.
fn expr<'inp, Ctx>(inp: &mut In<'inp, '_, Ln<'inp>, Ctx>) -> Result<(), Error>
where
  Ctx: ParseContext<'inp, Ln<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, Ln<'inp>> + Emitter<'inp, Ln<'inp>, Error = Error>,
{
  node(K::Expr as u16, |inp: &mut In<'inp, '_, Ln<'inp>, Ctx>| {
    // Each consumed token settles, so `commit_token` records a `Token` event for it.
    while inp.next()?.is_some() {}
    Ok(())
  })
  .parse_input(inp)
}

// Build the WHOLE node speculatively, then decline: every event the branch buffered — the
// tombstone, the token settles, the retro-wrap — truncates on the one rewind mark. Then
// build it again, for keeps.
fn decline_then_parse<'inp, Ctx>(inp: &mut In<'inp, '_, Ln<'inp>, Ctx>) -> Result<(), Error>
where
  Ctx: ParseContext<'inp, Ln<'inp>>,
  Ctx::Emitter: CstEmitter<'inp, Ln<'inp>> + Emitter<'inp, Ln<'inp>, Error = Error>,
{
  let declined: Option<()> = inp.attempt(|inp| {
    expr(inp).ok()?;
    None // the branch did real work; declining rewinds all of it
  });
  assert!(declined.is_none());
  expr(inp)
}

let src = "1+2";

// Straight drive.
let mut straight: Sink<'_, Ln<'_>, _> =
  Sink::new(src, Fatal::<Error>::new(), profile());
Parser::with_context((&mut straight, DefaultCache::<Ln<'_>>::default()))
  .apply(expr)
  .parse_str(src)
  .unwrap();
let (green_straight, _) = straight.finish(K::Root as u16);

// Same source, through the declined speculation.
let mut backtracked: Sink<'_, Ln<'_>, _> =
  Sink::new(src, Fatal::<Error>::new(), profile());
Parser::with_context((&mut backtracked, DefaultCache::<Ln<'_>>::default()))
  .apply(decline_then_parse)
  .parse_str(src)
  .unwrap();
let (green_backtracked, _) = backtracked.finish(K::Root as u16);

// The declined branch left NO phantom: the two green trees are byte-identical.
let green = green_straight.unwrap();
assert_eq!(green, green_backtracked.unwrap());

// And the round-trip law holds — the reason to build a CST at all.
let tree = rowan::SyntaxNode::<Lang>::new_root(green);
assert_eq!(tree.text().to_string(), src);

// The structure is the grammar's: Root > Expr > [Num "1", Plus "+", Num "2"].
let expr_node = tree.first_child().unwrap();
assert_eq!(expr_node.kind(), K::Expr);
assert_eq!(expr_node.children_with_tokens().count(), 3);
```

Had the declined branch's events *not* rewound, the backtracked tree would carry a phantom `Expr` and
a duplicate run of tokens, and both the byte-identity assertion and the round-trip would fail. They
hold because one checkpoint mark pinned the event log, the diagnostics, and the cursor to the begin
point, and one rewind released all three from it — the tree rewound for free.

## Where to go next

This chapter is the last of Part III's four seams onto the one engine:

- **The mark the sink rides** — the pure-copy checkpoint, its last-in-first-out contract, and the
  cell taxonomy this chapter's own census mirrors: the
  [Checkpoint, rewind & the LIFO contract](super::arch_checkpoint_rewind) chapter, where the
  value-keyed-inner composition developed above was first sketched.
- **The emitter the sink wraps** — the atomic [`Emitter`](crate::Emitter) capability family, the
  value-keyed `checkpoint` / `rewind` / `release` trio, and the `CstEmitter` structuring surface the
  `node` combinators drive: the [Atomic Emitter chapter](super::arch_atomic_emitter).
- **The tutorial, end to end** — building a real GraphQL-shaped CST with the `node` combinators, the
  typed tree views, recovery error-nodes, and the round-trip oracle, without ever touching an event:
  [chapter 16](super::ch16_lossless_cst).
- **The flat catalog** — every `node` combinator, the marks, and the emitter surface as terse
  reference entries: the [combinator reference](super::ref_combinators) and the
  [errors, emitters & context reference](super::ref_errors_emitters_context).
