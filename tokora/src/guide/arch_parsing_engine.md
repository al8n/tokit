# The parsing engine: parse while lexing

Every parser you wrote in Part II — the plain functions over an [`InputRef`](crate::InputRef) from
[chapter 2](super::ch02_parsers), the speculative guards of
[chapter 6](super::ch06_backtracking), the streaming prefixes of
[chapter 9](super::ch09_streaming) — ran on one small engine, and never had to name it. This
chapter names it. It is the opening chapter of Part III, so it sets the register the rest of the
part keeps: less *here is how to call it*, more *here is why it is shaped this way*.

The engine is the thing that turns a source and a lexer into a stream of tokens a combinator can
consume, one token at a time, without ever building a token buffer. Understanding its two objects —
the input **owner** and the **working handle** every parser is handed — and the single surface a
combinator drives is the mental model the checkpoint, emitter, and CST chapters all build on.

## Lex, then parse — and why tokora does neither in that order

The textbook pipeline runs in two phases: the lexer consumes the whole source into a `Vec<Token>`,
and then the parser consumes the vector. The phases are clean to reason about and they cost a full
materialized copy of the token stream — an allocation proportional to the input, touched twice
(once to fill, once to drain), with the parser starting only after the lexer has finished.

```text
Two-phase:   Source ──▶ Lexer ──▶ [ Vec<Token> ] ──▶ Parser
                                   ↑ one allocation, sized to the whole input
```

Tokora runs the lexer and the parser *interleaved*. The parser asks for a token; the engine lexes
exactly one and hands it over; the parser decides and asks for the next. There is no vector between
them — only a small, fixed lookahead window, buffered on the stack, for the moments a decision
needs to see a token or two ahead before committing to consume them.

```text
Parse-while-lexing:   Source ──▶ Lexer ◀──▶ Parser
                                  └── on demand, one token at a time,
                                      no token buffer between the two
```

The payoff is not incidental; it is the design goal the whole input layer is bent toward:

- **No token buffer.** Memory is `O(1)` in the input length beyond the lookahead window — the
  engine never holds the stream, only a cursor into the source and a handful of staged tokens.
- **Single pass.** A token is lexed and consumed in the same breath, so it is still warm in cache
  when the parser reads it; there is no cold second sweep over a vector.
- **Streaming falls out for free.** Because the parser pulls rather than the lexer pushing, a
  source that is only a *prefix* of a growing stream works with the same machinery — the frontier
  rules of [chapter 9](super::ch09_streaming) live at the one point where a token is pulled.

The cost tokora pays for this is that speculation cannot be "just re-run the lexer from a saved
`Vec` index" — there is no vector. It has to be a genuine snapshot-and-restore of the engine's
position, which is exactly what [backtracking](super::ch06_backtracking) is, and why it gets its
own chapter.

## Two objects: the input owner and the working handle

The engine is split in two, and the split is the load-bearing design decision of the whole layer.

- The **owner** holds the ground truth: the borrowed source, the live lexer state, the span of the
  last token, the lookahead cache, the recursion budget every descent draws on, and the bookkeeping
  the frontier and backtracking machinery keep (the finality flag, the lexer-error dedup watermark,
  the poison boundary, the checkpoint lineage). It is a crate-internal type; a parser never sees it.
- The **working handle**, [`InputRef`](crate::InputRef), is what every parser *is* handed. It is not
  a copy of the owner — it is a bundle of **borrows** into the owner, plus a borrow of the emitter.
  Every combinator, every guard, every speculative branch operates through one of these.

The entry-point trait [`Parse`](crate::Parse) is the seam between the two. Its driver builds the
owner, borrows a handle out of it, and runs the parser against the handle — the whole of it:

```rust,ignore
fn parse_with_state(self, src: &'inp L::Source, state: L::State) -> Result<O, Error> {
  let Parser { mut f, ctx, .. } = self;

  let mut input = Input::with_state_and_context(src, state, ctx.provide());
  let mut input_ref = input.as_ref();
  f.parse_input(&mut input_ref)
}
```

Read it as three steps. The [`ParseContext`](crate::ParseContext) hands over what it supplies —
`provide()` builds an `InputContext`, which is an emitter, a lookahead cache, **and** the recursion
budget every descent draws on
([`with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter) is where a caller
changes it; see the [errors, emitters & context reference](super::ref_errors_emitters_context) for
the emitter/cache pairing). The owner is built over the immutable source, the initial lexer state
and that context, and keeps all three for the parse's life. A handle is borrowed out of the owner —
the emitter borrow comes with it — and the parser runs against the handle, returning a value or the
emitter's error type.

Why two objects rather than one? Because the handle being a *borrow* of the owner is what makes
three separate guarantees hold at once, each enforced by the borrow checker rather than by
convention:

- **Only the driver can end a stream.** Sealing a partial stream as final takes `&mut Input`, and a
  live handle already borrows the owner — so no combinator, at any depth, can claim the stream
  ended. That is the finality law of [chapter 9](super::ch09_streaming), and it is a consequence of
  the split, not a rule bolted on.
- **A checkpoint can be a pure copy.** Because the source is one immutable slice the owner merely
  borrows, saving a position is copying a few offsets and cloning the (typically cheap) lexer state
  — not journalling edits to a buffer. More on this below, and in the Checkpoint & Rewind chapter.
- **The scanner keeps its registers.** The hot fields the per-token path touches are packed on the
  owner ahead of the bookkeeping; the handle borrows them directly. The abbreviated shape:

```rust,ignore
// Abbreviated: the parked-front-token, front-report and trace/witness fields are elided.
pub struct InputRef<'inp, 'closure, L, Ctx, Lang: ?Sized = (), Cmpl = Complete>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  input: &'closure &'inp L::Source,        // the immutable source, borrowed
  state: &'closure mut L::State,           // the live lexer state
  span: &'closure mut L::Span,             // span of the most recently consumed token
  cache: &'closure mut Ctx::Cache,         // the lookahead window
  finality: Cmpl::Finality,                // a read-only snapshot (see chapter 9)
  emitted_error_end: &'closure mut L::Offset,       // lexer-error dedup watermark
  poison_boundary: &'closure mut Option<L::Offset>, // sticky limit-trip frontier
  recursion: &'closure mut RecursionLimiter,        // descent depth + its budget
  session: Session<'inp, 'closure, L, Ctx::Emitter, Lang>, // lineage + the emitter borrow
  _marker: PhantomData<Lang>,
}
```

### The cursor and the frontier

The engine's position is a [`Cursor`](crate::InputRef::cursor) — a thin wrapper over the lexer's
offset. It marks *where the next token will be lexed from*: with the cache empty it is the raw lex
position, and with tokens staged it points at the start of the first staged token, so it always
reads as the boundary between what has been consumed and what has not. A consume advances it; a peek
never does. That single invariant — **a peek commits no progress** — is what makes the lookahead
window safe to fill speculatively, and it is what the demo at the end of this chapter asserts.

The tokens themselves come from the [`Lexer`](crate::Lexer) trait's on-demand pull. The engine
builds a lexer positioned at the cursor and calls `lex` to produce the next token; `bump` is how it
fast-forwards a freshly built lexer to the offset it should resume from (the engine constructs a
lexer per operation rather than holding one across the whole parse):

<!-- api-digest: Lexer -->
```rust,ignore
fn lex(&mut self) -> Option<Result<Self::Token, <Self::Token as Token<'inp>>::Error>>;
fn read_frontier(&self) -> ReadFrontier<Self::Offset>;   // required: there is no default
fn bump(&mut self, n: &Self::Offset);
```

`lex` returning `None` is exhaustion; `Some(Ok(tok))` is a token; `Some(Err(e))` is a lexer error
the engine routes to the emitter. All three are **required**: `read_frontier` in particular has no
default, because how far a lexer has safely read is a fact only that lexer knows — see
[chapter 9](super::ch09_streaming) for what a wrong answer costs a partial drive. It is called exactly as often as the parse demands and no more —
that "no more" is the whole point.

## A parser is a function over the handle

There is one trait every combinator implements, and its whole surface is a single method:

```rust,ignore
pub trait ParseInput<'inp, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}
```

A parser, then, is *a thing that mutates the handle and yields a value or the emitter's error*.
Composition is nothing more exotic than threading the same `&mut InputRef` through each combinator
in turn: `then` runs one parser, then the next, on the same handle; `repeated` runs its inner
parser against the handle until it stops; a hand-written function calls `next()` and `try_expect()`
directly. There is no separate "parser value" flowing between stages — only the handle, moving
forward. The tentative sibling [`TryParseInput`](crate::TryParseInput) has the same shape for
parsers that report "did not match" without emitting.

Since 0.3.0 the trait carries the same [`Completeness`](crate::Completeness) typestate as the
handle it consumes: the trailing `Cmpl` parameter names whether the drive is over a whole input
([`Complete`](crate::Complete), the default) or a still-growing chunk of a stream
([`Partial`](crate::Partial)). The default keeps every existing signature reading exactly as
before — `ParseInput<'inp, L, O, Ctx>` *is* the complete-mode trait — while a parser written
generic over `Cmpl` runs under **both** modes: [`Parse`](crate::Parse) drives it at `Complete`
and [`parse_partial`](crate::parse_partial) drives the same item at `Partial`. The dispatch
surface stays one vocabulary, not two (see [chapter 9](super::ch09_streaming) for the
write-once story and the frontier rules the partial instantiation activates).

The consume/peek surface the handle exposes is small and deliberate:

- [`next`](crate::InputRef::next) — consume the next token unconditionally (`Ok(None)` at end of
  input). This is the one call that advances the committed cursor.
- [`peek`](crate::InputRef::peek) / `peek_one` — fill the lookahead window *without* committing.
  `peek` takes a compile-time [`Window`](crate::Window) capacity (`typenum::U1` through `U32`), so
  the maximum lookahead a grammar uses is fixed at monomorphization — there is no unbounded,
  hidden lookahead. Peeked tokens land in the cache and are served from there when
  consumed. (The cache itself is the context's one pluggable seam here: the default is a
  small, fixed, stack-inline buffer, so the shipped engine allocates nothing for
  lookahead — a custom [`Cache`](crate::Cache) may choose its own storage and capacity.)
- [`try_expect`](crate::InputRef::try_expect) — the peek-or-take workhorse: examine the next token
  and either commit it (the predicate matched) or leave it staged. A predicate that never commits
  is a one-token peek.
- [`cursor`](crate::InputRef::cursor), [`slice`](crate::InputRef::slice),
  [`span`](crate::InputRef::span) — the position, the source text of the last token, and its span.

Because lookahead is explicit and capacity-bounded, dispatch is **deterministic**: a combinator
looks at a fixed window, decides, and commits — there is no implicit "try this whole branch and
unwind if it fails" behind the scenes. Speculation exists, but you ask for it by name (the next
section), which is what keeps the cost of a parse legible.

## Emission rides alongside consumption

A parser does not consume on one channel and report diagnostics on another, disconnected one. The
emitter is borrowed *by the handle* — it lives in the same `session` cell as the backtracking
bookkeeping — so the handle's own [`emit_*`](crate::InputRef::emit_error) forwarding methods
and the crate's structured emission paths write to the same object the consume path is
driving. (The emitter is reached *through* the handle, never handed out as a value: a
`&mut Ctx::Emitter` is itself installable as another parse's emitter, which is how a
recording sink could be aimed at a buffer it was not built over. The callbacks that used to
receive the emitter — a `*_while` condition, a `peek_then` handler, a token-level pratt fold —
receive an [`EmitterView`](crate::EmitterView) for the same reason: the same operations, in a
value no emitter slot can take. The read side,
[`emitter_ref`](crate::InputRef::emitter_ref), stays open.) Consumption and emission share one
handle, and — the part that matters for what follows — **one timeline**.

Two channels ride that timeline, and neither is a second pass:

- **Diagnostics.** A mismatched token, a premature end, a lexer error — these are emitted through
  the [`Emitter`](crate::Emitter) as they are discovered, mid-consume. Whether an emission is fatal
  (unwind now) or merely recorded (keep going, collect more) is the emitter's choice, not the
  parser's; the same parser runs fail-fast or collecting depending only on the context it was given.
  The atomic emitter design behind that is the subject of the [Atomic Emitter chapter](super::arch_atomic_emitter).
- **Committed tokens.** Every token that settles flows to one emitter hook, once. That hook is the
  seam the lossless CST rides: a recording sink turns each settled token into a tree event, which is
  how *every* consuming combinator becomes tree-producing with no per-combinator code. A
  diagnostics-only emitter leaves the hook a no-op and pays nothing. The
  [`CstEmitter`](crate::emitter::CstEmitter) capability and the event stream it feeds are the
  subject of the event-stream CST chapter.

The reason to introduce both channels *here*, in the engine chapter, is that they are why
backtracking has to rewind more than a cursor.

## Backtracking, at a glance

The handle can save its position and later return to it. You reach for this by name —
[`attempt`](crate::InputRef::attempt) / `try_attempt` for a single speculative closure,
[`begin`](crate::InputRef::begin) for an imperative [`Transaction`](crate::Transaction) guard — and
the tutorial covered the full surface in [chapter 6](super::ch06_backtracking).

The one fact the engine chapter needs you to carry forward is this: **a rollback rewinds the whole
timeline, not just the cursor.** When a speculative branch is abandoned, the engine restores, as a
unit, the position, the lexer state, the emitted diagnostics, the lexer-error dedup watermark, and
the poison boundary — so a branch that emitted an error and then backed out leaves *no* diagnostic
behind, exactly as it leaves no cursor movement behind. Consumption and emission were on one
timeline going forward; they are on one timeline going backward too.

And because the owner merely borrows an immutable source (there is no growable internal buffer to
un-edit), a saved checkpoint is a **pure copy** of those few facts, and a restore is copying them
back — not replaying a journal of edits. That is the shape of it at a glance; the mechanism — how
the copy stays cheap, how nested checkpoints keep a last-in-first-out discipline, how the emitter is
told to drop exactly the abandoned branch's emissions — is deliberately left to two later chapters:

- the **Checkpoint & Rewind chapter** for the snapshot/restore machinery and the
  [`Checkpoint`](crate::input::Checkpoint) it copies;
- the [Atomic Emitter chapter](super::arch_atomic_emitter) for how an emitter marks and rewinds its
  own log in step, through [`Emitter::rewind`](crate::Emitter::rewind).

This chapter asserts only the *observable* half — position returns — in code below; the emission
half is the emitter chapter's to demonstrate.

## The engine, end to end

One compiling parse exercises every claim above: the lexer runs on demand, a peek commits no
progress, `next` advances the committed cursor, a combinator rides the very same handle, and a
declined `attempt` returns the position. The lexer here is a tiny hand-written `CharLexer` over
single-character tokens (digits and `+`), so nothing but core tokora types is in play.

```rust
# use core::{convert::Infallible, fmt};
# use tokora::{
#   FatalContext, InputRef, Lexer, SimpleSpan, Token,
#   error::{UnexpectedEot, token::UnexpectedToken},
# };
# use tokora::span::Span as _;
# #[derive(Debug, PartialEq)]
# struct Error;
# impl From<Infallible> for Error { fn from(e: Infallible) -> Self { match e {} } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for Error { fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { Error } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Error { fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { Error } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for Error { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Error } }
# impl tokora::error::MaybeIncomplete for Error {}
# #[derive(Debug, Clone, PartialEq)]
# enum Tok { Digit(u32), Plus }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum Kind { Digit, Plus }
# impl fmt::Display for Kind { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") } }
# impl Token<'_> for Tok {
#   type Kind = Kind;
#   type Error = Infallible;
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
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
#   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
#   fn bump(&mut self, n: &usize) { self.pos += n; }
# }
# type Ctx<'a> = FatalContext<'a, CharLexer<'a>, Error>;
use tokora::{Parse, Parser, ParseInput as _, parser::expect, utils::Expected};

// A parser is a plain function over the working handle: it drives the same `&mut InputRef`
// that every combinator drives.
fn engine_demo<'a>(
  inp: &mut InputRef<'a, '_, CharLexer<'a>, Ctx<'a>>,
) -> Result<Vec<(Kind, usize)>, Error> {
  // The lexer has not run yet: the committed cursor sits at offset 0.
  assert_eq!(*inp.cursor().as_inner(), 0);

  // `try_expect` with a predicate that never commits is a one-token peek. The engine lexes
  // the token on demand into the lookahead window — but leaves it staged, so the cursor
  // does NOT move. A peek commits no progress.
  let peeked = inp.try_expect(|_t| false)?;
  assert!(peeked.is_none());
  assert_eq!(*inp.cursor().as_inner(), 0, "a peek commits no progress");

  // `next()` serves that staged token from the cache (no re-lex) and commits it. The token
  // carries the span the lexer computed for it — evidence the work happened on demand.
  let first = inp.next()?.expect("a first token");
  assert_eq!(first.data().kind(), Kind::Digit);
  assert_eq!(first.span().end(), 1);
  assert_eq!(*inp.cursor().as_inner(), 1, "one token consumed, cursor advanced");

  // A combinator rides the very same handle. `expect` consumes through `InputRef` exactly as
  // the hand-written calls above do — composition is threading one handle forward.
  let plus = expect(|t: &Tok| if matches!(t, Tok::Plus) { Ok(()) }
                    else { Err(Expected::one(Kind::Plus)) })
    .parse_input(inp)?;
  assert_eq!(plus.kind(), Kind::Plus);

  // Backtracking, at a glance: consume speculatively inside `attempt`, then decline. The
  // rollback returns the cursor to the begin point (and the lexer state, and any emissions
  // with it — see the Checkpoint & Rewind chapter).
  let before = *inp.cursor().as_inner();
  let declined = inp.attempt(|inp| { let _ = inp.next(); None::<()> });
  assert!(declined.is_none());
  assert_eq!(*inp.cursor().as_inner(), before, "a declined attempt rewinds position");

  // Drain whatever remains, on demand, recording each token's kind and end offset.
  let mut rest = Vec::new();
  while let Some(tok) = inp.next()? {
    rest.push((tok.data().kind(), tok.span().end()));
  }
  Ok(rest)
}

// Drive it. `parse_str` builds the owner, borrows a handle, and runs `engine_demo` against it.
let rest = Parser::with_parser(engine_demo).parse_str("1+2").unwrap();
assert_eq!(rest, vec![(Kind::Digit, 3)]); // only the final `2` is left to drain
```

## Where to go next

You now have the mental model the rest of Part III refines along four axes:

- **How the input is stored** — the `Source`/`Slice` seam that lets this same engine read `&str`,
  `&[u8]`, or an owned reference-counted buffer without the grammar changing: the
  [Source, Slice & storage backends](super::arch_source_slice) chapter.
- **How a checkpoint is taken and restored** — the pure-copy snapshot, its last-in-first-out
  discipline, and the [`Checkpoint`](crate::input::Checkpoint) it copies: the Checkpoint & Rewind
  chapter.
- **How the emitter marks and rewinds its log in step with the cursor** — the atomic
  [`Emitter`](crate::Emitter) capability family and [`Emitter::rewind`](crate::Emitter::rewind): the
  [Atomic Emitter chapter](super::arch_atomic_emitter).
- **How committed tokens become a lossless tree** — the [`CstEmitter`](crate::emitter::CstEmitter)
  hook and the [`cst`](crate::cst) event stream it feeds: the event-stream CST chapter.
