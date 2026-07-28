# Checkpoint, rewind, and the LIFO contract

The [engine chapter](super::arch_parsing_engine) closed on a promise it deferred: a rollback
rewinds the *whole timeline*, not just the cursor — the position, the lexer state, the emitted
diagnostics, the lexer-error dedup watermark, and the poison boundary all return together — and it
does so by *copying a snapshot back*, not by replaying a journal of edits. This chapter is that
promise, made precise. It is the load-bearing internals chapter of Part III, and it keeps the
part's register: less *how to call it*, more *why it is shaped this way*.

The tutorial already taught the surface. [Chapter 6](super::ch06_backtracking) walked the whole
public backtracking vocabulary — [`attempt`](crate::InputRef::attempt) / `try_attempt`, the
[`Transaction`](crate::Transaction) guard, the stacked savepoints, the session points — and stated
the two laws they all obey: *restores are last-in, first-out*, and *restoring is a snapshot copy,
not a journal replay*. This chapter explains the single mechanism beneath all of them, why those
two laws are the shape they are, and how one checkpoint manages to rewind two independent
channels — the input cursor and the emitter's diagnostic/event log — as one unit.

## What a checkpoint captures

A [`Checkpoint`](crate::input::Checkpoint) is a snapshot of one **lineage**: the concrete history
of tokens lexed and diagnostics emitted up to the instant of the save. It is a handful of copied
facts and nothing more — no borrow of the input, no journal, no diff:

```rust,ignore
// Abbreviated: the debug-only cross-input witness id and the allocator-only lineage id
// (both explained below) are elided.
pub struct Checkpoint<'inp, 'closure, L: Lexer<'inp>> {
  cursor: Cursor<'inp, 'closure, L>,  // where the next token will be lexed from
  span: L::Span,                      // the last-consumed token's span
  state: L::State,                    // the live lexer regime, cloned
  emitter_checkpoint: u64,            // the emitter's emission mark (the diagnostic channel)
  emitted_error_end: L::Offset,       // the lexer-error dedup watermark
  poison_boundary: Option<L::Offset>, // the sticky terminal frontier (a latched limit trip)
  cache_pushes: u64,                  // the cache's monotone push count
}
```

Saving is amortized `O(1)`: it clones the lexer state (typically cheap — often a `Copy` regime
enum) and copies a few offsets. It never touches the source, because the source is one immutable
slice the input merely borrows — the design decision the [engine chapter](super::arch_parsing_engine)
called load-bearing. There is no growable internal buffer to un-edit, so a saved position is
*genuinely* just those few facts, and a restore is *genuinely* just copying them back.

That immutability is why the primitive is a snapshot rather than a diff. A mutable-buffer parser
has to record what it changed and play the changes backward; tokora has nothing to play backward.
The whole of "rewind" is: overwrite the live scanning cells with the saved ones, and truncate the
one growable thing — the emitter's log — back to the saved mark.

## One save, two channels

The subtle part is that a checkpoint spans *two* logs that a parser writes to independently.

The **position channel** is the scanning state: the cursor, the last-consumed span, the lexer
regime, the token cache, the dedup watermark, and the poison boundary. This is the input's own
ground truth and its lineage bookkeeping, and the checkpoint carries a copy of all of it.

The **emission channel** is the emitter's log — the diagnostics a parser reports and, for a
recording sink, the CST events it builds. The checkpoint does *not* copy that log; it copies a
single `u64` **mark** into it, taken with [`Emitter::checkpoint`](crate::Emitter::checkpoint) at
save time. On restore, the input hands that mark back through
[`Emitter::rewind`](crate::Emitter::rewind), and the emitter drops exactly the emissions recorded
after it.

One `save` captures both channels; one `restore` replays both. That is the entire reason a declined
speculative branch leaves *no diagnostic* behind, exactly as it leaves no cursor movement behind —
the two were saved as a unit and are restored as a unit. The reference emitter,
[`Verbose`](crate::emitter::Verbose), makes the mechanism concrete: its `checkpoint` is the length
of its emission log, and its `rewind` pops every entry recorded past the mark, newest first. Its
`release` — the settle for a branch that was *kept* rather than abandoned — is a deliberate no-op,
because a kept mark is just a number going out of scope. The demo at the end of this chapter drives
exactly this pair.

The engine chapter framed all of this as *"consumption and emission share one handle, and one
timeline"*. The checkpoint is the object that makes the timeline rewindable: it is the seam where
the two channels are pinned to a single point and released from it together.

## Why a copy, not a merge — and the cell taxonomy

The predecessor design tried to be cleverer, and that is where the bugs lived. When restore
*reconciles* saved and current state — as the previous `Verbose` rewind did, keeping or dropping
live diagnostics by a **span-end offset heuristic** (*retain every diagnostic whose span ends at
or before the restore offset*) rather than truncating by emission order — it cannot tell two
diagnostics at the same offset apart, and it silently keeps or drops the wrong one. The
golden model refuses reconciliation on principle: restore **overwrites** the scanning cells and
**truncates** the emission log by mark. There is no heuristic to be wrong, because there is no
merge.

Making that refusal hold as the code grows takes a discipline, because "restore" means something
*different* for each cell an input owns, and a cell added without deciding which meaning it has is
precisely the defect that has shipped — twice (a cache-push counter, then the finality bit, each
added *next to* the backtracking bookkeeping instead of *through* it). So every mutable cell an
input owns is classified into exactly one of five classes, and the class **is** the restore rule:

| Class | Cells | What restore does |
|---|---|---|
| **Ground truth** | lexer state, last-consumed span, token cache, the emitter's log | overwrite from the snapshot (the log by truncation to the saved mark) |
| **Lineage memos** | dedup watermark, poison boundary, cache-push count, the live-checkpoint stack, the pin set, the open session points | pure-copy the saved value — with two structural exceptions noted below |
| **Monotone id sources** | the checkpoint-id counter, the savepoint sequence | **nothing** — rewinding a counter would reissue a live id, and a colliding id is worse than none |
| **World facts** | the `is_final` finality bit | **nothing** — a rollback rewinds the *parse*, not the *world* (a stream cannot un-end) |
| **Witness / instrumentation** | the input's identity, the `trace` nesting depth | **nothing** — neither affects scanning |

The three lineage memos in the middle row are the checkpoint's `emitted_error_end`,
`poison_boundary`, and `cache_pushes` fields: facts *about* the saved lineage that a last-in,
first-out restore returns to exactly, so they copy back verbatim. They move together for a reason —
a speculative peek that trips a resource limit latches the poison boundary, emits the limit
diagnostic, and lifts the dedup watermark in one step, so a restore that unwinds that peek must put
all three back paired. The two structural exceptions are still lineage memos, but their mechanics
differ: the live-checkpoint stack is *popped through* the restored id rather than snapshot-copied,
and the pin set is left untouched (a restore never changes which guards are live) — as is the
open session-point stack, a restore below whose base is refused by its pin.

The **world fact** row is the one place the discipline must *not* reach, and it is enforced
structurally rather than remembered. A working handle borrows the input for its whole life, so the
finality bit — which only the driver can flip, and only with `&mut Input` — is *unreachable* while
any parser, guard, or speculative branch runs. It cannot change during a handle's life, so no
rollback can observe it change, so a checkpoint has nothing to save. Restoring it would be the
mirror bug: a rollback across a legitimate seal would un-end an ended stream, and the parser would
wait forever for input that will never come. This is the finality law of
[chapter 9](super::ch09_streaming), seen from the checkpoint's side.

The taxonomy is not a comment that hopes to stay true. A single crate-internal function
destructures the input **exhaustively** — no `..` — and binds every field. Adding a cell is
therefore a *compile error at the guardian*, at the table that asks which class the new cell is in
and what restore must do to it. It is generic and never instantiated, so it costs zero bytes; it is
purely a wall. (It is greppable, too: `grep CELL_CENSUS` finds it from anywhere in the tree.)

## The last-in, first-out contract, and how misuse is caught

Restoring a checkpoint **invalidates every checkpoint saved after it**. This is the one law the
type system does not, on its own, enforce — so it is worth being exact about *why* it holds and how
a violation is caught.

The reason is structural, not stylistic. Restoring an older checkpoint truncates the emission log
below a younger checkpoint's mark and un-lexes the tokens the younger position depends on. A
truncated log cannot be rebuilt, so there is simply *no correct state* a later restore of the
younger checkpoint could produce. Last-in, first-out is not a convention laid over the mechanism;
it is the only order in which the mechanism has a defined answer.

Three layers guard it, strongest first:

- **The lifetime brand (compile time).** A `Checkpoint` is branded with the invariant `'closure`
  lifetime of the handle that saved it. Every handle a parser receives arrives through a
  `for<'closure>` closure, so any two handles carry rigidly distinct brands that cannot unify —
  and restoring a checkpoint one handle saved into a *different* handle is a **compile error**, not
  a runtime check. This is what makes the [`Transaction`](crate::Transaction) guard's nesting
  safe: an inner guard mutably borrows its parent for its whole life, so deciding the parent while
  a child is still live does not *compile*. The most common LIFO violation is unrepresentable.

- **The pin set (every allocator build, detect-at-cause).** A guard, an `attempt`, or a session
  point logically borrows the timeline from its begin point forward, so it **pins** that begin
  point on the input's lineage. A raw restore that would pop a pinned checkpoint off the lineage —
  a restore reaching *below* a live guard's foundation — **panics at the restore itself**, where
  the mistake is made, rather than letting the guard continue on a torn base. This is a real
  runtime check kept in release, not a debug assertion.

- **The live-checkpoint witness (debug builds).** Debug builds track the live checkpoints exactly
  and panic on any out-of-order restore, with a message that begins `non-LIFO checkpoint restore`.
  Because `cargo test` compiles with debug assertions on, exercising a parser's backtracking paths
  in tests surfaces a violation immediately. A companion assert re-checks that a checkpoint belongs
  to *this* input — a backstop for the one construction the brand cannot separate (two inputs
  borrowed in a single crate-internal scope).

Release builds without the pin trip do not check a raw non-LIFO restore, and the contract is honest
about what that costs: the input is left **unspecified but bounded**. Even then — no undefined
behavior, no leak, no panic originating in the crate, every scan terminates (the resource-limiter
state travels *inside* the checkpoint, so a re-reached limit re-trips rather than rescanning without
bound), and the input stays usable. What is *not* guaranteed is diagnostic fidelity: a diagnostic
may go missing or be attributed to the wrong branch. That bounded-but-imperfect floor is the whole
reason the raw triple is gated away behind a feature and the guards are the supported surface.

## The guards are the surface; raw save/restore is the valve

The raw `save` / `restore` / `commit` triple is the primitive everything is built on, and it is
**not** the API you are meant to reach for. It is public only under the `unstable-raw` feature;
without it the three methods are crate-internal, so a downstream crate cannot even *express* a
non-LIFO restore, and the whole hazard class of the previous section is unrepresentable. The
supported surface upholds the contract by construction:

- [`Transaction`](crate::Transaction) — the guard from [`begin`](crate::InputRef::begin). Parse
  *through* it (it dereferences to the handle), then [`commit`](crate::Transaction::commit) to keep
  the work or [`rollback`](crate::Transaction::rollback) to discard it. Say nothing and the drop
  decides; the default is rollback, so every early exit — a `break`, a `?`, a return — rewinds on
  the way out. The drop policy is a zero-sized [typestate](crate::DropPolicy): `begin_with::<Commit>`
  flips it to keep-on-drop for operator loops whose common path is success — except on a panic
  unwind, which takes the rollback arm whatever the policy says (std builds), because an unwind
  aborts the region rather than completing it. Deciding is one branch over an `Option<Checkpoint>`;
  there is no journaling to unwind.

- [`StackedTransaction`](crate::StackedTransaction) — the guard from
  [`begin_stacked`](crate::InputRef::begin_stacked), for several live fallback points at once.
  Its [`savepoint`](crate::StackedTransaction::savepoint)s follow SQL semantics:
  [`rollback_to`](crate::StackedTransaction::rollback_to) an older one destroys every younger one
  (out-of-order revival is impossible *by construction* — the savepoint vector truncates from the
  top), while the target stays valid for a later rollback;
  [`release`](crate::StackedTransaction::release) forgets savepoints while keeping the progress. A
  [`SavepointId`](crate::SavepointId) is lifetime-branded to its transaction, so it cannot outlive
  it, and a foreign or stale id panics in every build.

- [`attempt`](crate::InputRef::attempt) / `try_attempt` and the
  [session points](crate::InputRef::begin_point) round out the surface for closure-shaped and
  externally-driven speculation respectively. Chapter 6 is the usage reference for all of these;
  the point *here* is only that each holds its begin-point checkpoint internally, settles it in
  exactly one of restore-or-commit, and never hands it out — so the LIFO contract is theirs to keep,
  not yours.

The through-line: the raw triple has a contract a human must uphold by hand, and every guard
upholds it mechanically — a nested guard's out-of-order decision is a borrow error, a raw restore
below a live guard is a pinned-base panic, and a merely-dropped guard settles safely. Guards first,
always.

## Composing with a tree-building emitter

The one-timeline promise has to survive one more composition: the lossless CST. When a parse builds
a tree, committed tokens and structure flow to a recording emitter — a `cst::Sink`
wrapping an inner diagnostics emitter — through the same
[`Emitter::commit_token`](crate::Emitter::commit_token) hook the engine chapter named. The sink
buffers a second log (the event stream) *and* forwards every diagnostic to its inner emitter, and
its `checkpoint` / `rewind` / `release` must rewind **both** under the one mark the input already
manages. Get that wrong and a rolled-back branch leaves a phantom node in the tree even though its
diagnostic vanished — the two channels sheared apart.

The sink keeps them together with a **value-keyed inner** contract, and the discipline is worth
naming at the architecture level (the lossless-CST chapter — chapter 16 — and the
emitter reference carry the API detail). The sink's own mark is the event-log length; at each
`checkpoint` it also freezes, on a small stack, the inner emitter's *own* checkpoint reading — a
plain `u64`, captured by value, not a resource to reclaim. On `rewind` it truncates its event log
to the mark, replays its undo journal, and then rewinds the inner emitter **only to a reading it
knows exactly**:

- the captured reading of the row being rewound to — every disciplined path (a guard, an `attempt`,
  the scan family, a correct raw pair) lands here;
- nothing at all when the mark is the current length — a rewind that truncates nothing must leave
  the inner alone, because the surviving events are the whole log and every inner-side record they
  reference must survive with them;
- the inner's **construction-time** reading for a full unwind to the origin (an empty event log
  provably pairs with the reading the inner had at construction).

Everything else is refused rather than guessed. An out-of-range *future* mark — one strictly above
the current length, naming a log position that does not exist yet — is a **total no-op on every
channel**: events, the mark stack, the journal, the era ledger, and the inner alike. (Clamping it
to the current length instead — the pre-redesign behavior — would let a future mark spend the live
row of a real checkpoint taken at that length, desyncing the two logs.) And a truncating rewind to
a *mid-log* mark that no live row captured has no exact inner reading anywhere; that is undisciplined
raw use, so debug builds panic at the cause — the sink-level twin of the input's LIFO witness — and
release builds keep the sink's own channels exact and leave the inner untouched, never fabricating a
reading. The sink hands the inner only readings it can prove, or nothing.

Two consequences fall out of "value-keyed". First, the inner emitter must itself be value-keyed —
`checkpoint` a pure monotone reading, `rewind` a drop-by-value, `release` a no-op — which is exactly
the shape of `Verbose`, `Fatal`, `Silent`, and `Ignored`. Second, `release` on the sink pops its own
row *without forwarding* to the inner, because a kept reading needs no cleanup; this is what keeps a
commit-heavy loop (a Pratt operator loop saves per iteration) from stranding one dead row per
committed branch. The [`Emitter::release`](crate::Emitter::release) hook exists precisely so the
input can tell a buffering sink *"this mark will never be rewound to"* and let it reclaim the row —
one timeline, kept bounded.

## The rewind, end to end

One compiling parse exercises the headline claim: a speculative branch that consumes a token **and**
files a diagnostic, then declines — and *both* the cursor and the diagnostic rewind together. The
emitter is [`Verbose`](crate::emitter::Verbose) so the dropped diagnostic is observable after the
parse; the lexer is the same tiny hand-written `CharLexer` over single-character tokens the
[engine chapter](super::arch_parsing_engine) used, so nothing but core tokora types is in play.

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   InputRef, Lexer, SimpleSpan, Token,
#   error::{UnexpectedEot, token::UnexpectedToken},
# };
# use tokora::span::Span as _;
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(u32), Plus }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Plus }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   fn kind(&self) -> Kind { match self { Tok::Digit(_) => Kind::Digit, Tok::Plus => Kind::Plus } }
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
#     while self.pos < bytes.len() && bytes[self.pos] == b' ' { self.pos += 1; }
#     if self.pos >= bytes.len() { return None; }
#     let (start, c) = (self.pos, bytes[self.pos] as char);
#     self.pos += 1;
#     self.tok = SimpleSpan::new(start, self.pos);
#     Some(Ok(match c { '+' => Tok::Plus, _ => Tok::Digit(c as u32 - '0' as u32) }))
#   }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
use tokora::{
  Emitter, Parse, ParseContext, Parser,
  cache::DefaultCache, emitter::Verbose, span::Spanned,
};

// Generic over the parse context, so the very same parser can run under any emitter —
// here it will run under `Verbose`, which records diagnostics instead of failing fast.
fn rewind_demo<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CharLexer<'inp>, Ctx>,
) -> Result<Vec<Kind>, Error>
where
  Ctx: ParseContext<'inp, CharLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CharLexer<'inp>, Error = Error>,
{
  let start = *inp.cursor().as_inner();

  // A speculative branch that consumes a token AND files a diagnostic, then declines.
  // Both live on one timeline, so the decline rewinds both: the cursor returns to `start`,
  // and the diagnostic is dropped from the emitter's log.
  let declined = inp.attempt(|inp| {
    let _ = inp.next();                     // consume: the cursor moves forward
    let at = *inp.span();
    // File a real diagnostic. Under `Verbose` this is recorded (and returns `Ok`); the
    // branch is about to decline, so it should not survive.
    let _ = inp.emitter().emit_error(Spanned::new(at, Error));
    None::<()>                              // decline → roll the whole branch back
  });
  assert!(declined.is_none());
  assert_eq!(*inp.cursor().as_inner(), start, "position rewound to the begin point");

  // The real parse now sees every token the speculation had consumed — proof the cursor
  // came back. It emits nothing of its own.
  let mut kinds = Vec::new();
  while let Some(tok) = inp.next()? {
    kinds.push(tok.data().kind());
  }
  Ok(kinds)
}

let mut emitter = Verbose::<Error>::new();
let cache = DefaultCache::<'_, CharLexer<'_>>::default();
let kinds = Parser::with_context((&mut emitter, cache))
  .apply(rewind_demo)
  .parse_str("1+2")
  .expect("Verbose files diagnostics rather than failing the parse");

// Position rewound: the real parse saw all three tokens the speculation had consumed. Had
// the cursor NOT come back, it would have seen only the two after the speculated token.
assert_eq!(kinds, vec![Kind::Digit, Kind::Plus, Kind::Digit]);

// Diagnostics rewound: the error the declined branch filed is gone. Had the emission
// timeline NOT rewound with the cursor, this count would be exactly one.
assert_eq!(emitter.errors().values().flatten().count(), 0);
```

The two assertions are the chapter in miniature: the token count proves the position channel
rewound, the error count proves the emission channel rewound, and they rewound because a single
checkpoint pinned both to the begin point and a single restore released both from it.

## Where to go next

The checkpoint is one of the four seams Part III opens onto the same engine:

- **How the input is stored** — the `Source` / `Slice` seam that lets this same machinery, and the
  same pure-copy checkpoint, read `&str`, `&[u8]`, or an owned reference-counted buffer without the
  grammar changing: the [Source, Slice & storage backends](super::arch_source_slice) chapter.
- **How the emitter marks and rewinds its own log** — the atomic
  [`Emitter`](crate::Emitter) capability family, the value-keyed `checkpoint` / `rewind` /
  [`release`](crate::Emitter::release) trio this chapter leaned on, and how `Fatal` / `Verbose` /
  `Silent` get their rewind behavior: the [Atomic Emitter chapter](super::arch_atomic_emitter).
- **How committed tokens become a lossless tree** — the [`CstEmitter`](crate::emitter::CstEmitter)
  hook and the [`cst`](crate::cst) event stream the `cst::Sink` buffers and
  rewinds under this chapter's mark: the event-stream CST chapter, which is where the
  value-keyed-inner composition sketched above is developed in full.
