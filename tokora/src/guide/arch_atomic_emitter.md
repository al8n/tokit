# The atomic emitter

The [engine chapter](super::arch_parsing_engine) established that a parser consumes and *emits* on
one handle, one timeline; the [checkpoint chapter](super::arch_checkpoint_rewind) made the rewind
precise — one save pins the input cursor and the emitter's log together, one restore releases both.
This chapter takes the second of those two channels on its own terms: what the **emitter** is, why
its capability surface is a family of small traits rather than one large one, and how it marks and
rewinds its own log in step with the cursor. Like its Part III siblings it keeps the register: less
*here is how to call it*, more *here is why it is shaped this way*. The tutorial side is
[chapter 7](super::ch07_diagnostics); the flat catalog is the
[errors, emitters & context reference](super::ref_errors_emitters_context), which this chapter is
the architecture behind.

## Diagnostics are an effect, and effects are pluggable

A parser discovers a problem mid-consume: a token that does not fit, a premature end, a lexer error.
What happens *next* — stop the parse, record the problem and press on, drop it, or fold it into a
tree — is not a property of the grammar. It is a policy, and it is the same policy across the whole
parse. So tokora puts that policy in one replaceable object, the **emitter**, borrowed by the
handle alongside the scanning state, and leaves the parser writing the same line either way:

```text
inp.emitter().emit_error(Spanned::new(at, err))?;
```

The protocol is a `Result`. `Ok(())` means the diagnostic was handled as non-fatal and parsing
should continue; `Err(Self::Error)` means it is fatal and the `?` at the call site unwinds the
parse. The decisive point is that *the emitter returns that verdict, not the call site*.
[`Fatal`](crate::emitter::Fatal)'s `emit_error` returns `Err`, so the `?` ends the parse;
[`Verbose`](crate::emitter::Verbose)'s records the diagnostic and returns `Ok`, so the `?` does
nothing and the loop continues. The parser cannot tell which it is running under, and does not need
to.

Why a borrowed channel rather than a richer return type? Because the fatal/non-fatal choice is
uniform over a parse and orthogonal to any single combinator. Encoding it in return types would
thread the decision through every signature and force each combinator to re-decide it; a borrowed
effect object keeps the grammar policy-free and swaps the entire behavior at the driver, by handing
in a different context. This is the whole of the design's ergonomics: you do not write a
"collecting parser" and a "fail-fast parser" — you write *a* parser and choose an emitter.

The one-timeline law of the previous two chapters is what makes that channel safe to *emit* from,
not merely to consume from. Because the emitter rides the same rewindable timeline as consumption,
a declined speculative branch drops its diagnostics with its tokens: a diagnostic filed inside an
[`attempt`](crate::InputRef::attempt) that then declines never reaches the harvest, exactly as the
tokens it consumed never move the committed cursor. That constraint — *whatever the emitter records
must be able to unwind* — is the shape the rest of the trait is bent toward.

## The atomic capability design

The core [`Emitter`](crate::Emitter) trait is small. Trimmed of its `where L: Lexer<'a>` bounds, its
shape is:

```rust,ignore
pub trait Emitter<'a, L, Lang: ?Sized = ()> {
    type Error;

    // The three required diagnostic verbs. `Ok(())` is non-fatal (continue);
    // `Err(Self::Error)` is fatal (the `?` unwinds the parse).
    fn emit_lexer_error(&mut self, err: Spanned<<L::Token as Token<'a>>::Error, L::Span>) -> Result<(), Self::Error>;
    fn emit_unexpected_token(&mut self, err: UnexpectedTokenOf<'a, L, Lang>) -> Result<(), Self::Error>;
    fn emit_error(&mut self, err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>;

    // The one non-diagnostic method with no default (see below): the emitter must
    // say how its state unwinds.
    fn rewind(&mut self, cursor: &Cursor<'a, '_, L>, checkpoint: u64);

    // Everything past here has a blanket no-op (or inert-value) default, so a
    // fail-fast emitter inherits empty bodies and the calls inline to nothing.
    fn emit_warning(&mut self, warning: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error> { Ok(()) }
    fn emit_skipped_region(&mut self, span: L::Span, skipped: usize) -> Result<(), Self::Error> { Ok(()) }
    fn checkpoint(&self) -> u64 { 0 }
    fn release(&mut self, checkpoint: u64) {}
    fn commit_token(&mut self, tok: &L::Token, span: &L::Span) {}
    fn enter_label(&mut self, label: &'static str) {}
    fn exit_label(&mut self) {}
    fn bound_source(&self) -> Option<SourceIdentity> { None }
}
```

[`bound_source`](crate::Emitter::bound_source) is the newest of the defaulted members and the
one with a forwarding obligation: an emitter that binds a particular source — the CST
`Sink` does — overrides it so a mismatched drive is refused, and a
**wrapper** that forwards every emission but inherits the `None` default silently disables
that check for whatever it wraps. It is the same obligation `checkpoint`/`rewind`/`release`
carry.

Three verbs are not enough for a real grammar. A bounded repetition can report *too few* elements; a
separated list can report a stray or a missing separator; a Pratt loop can report an operator with
no operand; a tree build needs structure events. Folding all of those into the core trait would make
every emitter — including a two-line fail-fast one — answer questions it will never be asked, and
make every parser depend on the whole surface whether or not it uses it.

So tokora splits each scenario into its own focused trait, each an extension of
[`Emitter`](crate::Emitter), and lets a combinator name **only** the capabilities it actually uses.
Call it the Lego rule: `separated` bounds the separator capabilities, `repeated` the count
capabilities, a Pratt parse the [`PrattEmitter`](crate::emitter::PrattEmitter), and a plain
`expect` nothing beyond the base. An emitter, symmetrically, implements only the capabilities its
parsers need.

| Capability trait | Reports (the parsing shape) | In [`ComposableEmitter`](crate::emitter::ComposableEmitter)? |
|---|---|:--:|
| [`TooFewEmitter`](crate::emitter::TooFewEmitter) | a repetition below its minimum | ✅ |
| [`TooManyEmitter`](crate::emitter::TooManyEmitter) | a repetition above its maximum | — |
| [`FullContainerEmitter`](crate::emitter::FullContainerEmitter) | a fixed-capacity container out of room | ✅ |
| [`SeparatedEmitter`](crate::emitter::SeparatedEmitter) | a missing separator or a missing element | ✅ |
| [`UnexpectedLeadingSeparatorEmitter`](crate::emitter::UnexpectedLeadingSeparatorEmitter) / [`…Trailing…`](crate::emitter::UnexpectedTrailingSeparatorEmitter) | a stray leading / trailing separator | ✅ |
| [`MissingLeadingSeparatorEmitter`](crate::emitter::MissingLeadingSeparatorEmitter) / [`…Trailing…`](crate::emitter::MissingTrailingSeparatorEmitter) | a required leading / trailing separator absent | — |
| [`UnclosedEmitter`](crate::emitter::UnclosedEmitter) | an opener committed whose closer never arrived | ✅ |
| [`PrattEmitter`](crate::emitter::PrattEmitter) | an operator with no left- or right-hand side | — |
| [`CstEmitter`](crate::emitter::CstEmitter) | tree structure events (not a diagnostic) | — |

Each capability comes with a matching `From…Error` blanket. The method payloads are the leaf error
types from [`crate::error`], and the trait that wires a leaf to a method
— [`FromEmitterError`](crate::emitter::FromEmitterError) for the base surface, and one per capability
— is blanket-implemented off a plain `From<LeafError>` impl on your error enum. So implementing a
capability on a *pre-built* emitter is nothing you do: give your error type the `From` impls and
`Fatal` / `Verbose` / `Silent` gain the capability for free. The genuinely new code in a custom
emitter is almost always just the base surface.

### Bundling the common family

The separated/repeated machinery ends up needing most of the family at once, which would be a
six-line `where`-clause ladder at every generic parser that drives it.
[`ComposableEmitter`](crate::emitter::ComposableEmitter) is that ladder as one name:

```rust,ignore
pub trait ComposableEmitter<'inp, L, Lang: ?Sized = ()>:
    Emitter<'inp, L, Lang>
    + FullContainerEmitter<'inp, L, Lang>
    + SeparatedEmitter<'inp, L, Lang>
    + UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>
    + UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>
    + TooFewEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>
{}
```

It is blanket-implemented for every emitter that satisfies the whole family, so a bound of
`E: ComposableEmitter` is interchangeable with spelling out the seven sub-traits — that is the
**default-policy** surface, and
[`PolicyComposableEmitter`](crate::emitter::PolicyComposableEmitter) is the same bundle widened
by the three a count or separator policy needs — and its
context-side twin, [`ComposableParseContext`](crate::ComposableParseContext), collapses a whole parse context to one bound in
the same way — and goes one rung further, riding
[`FromTokenErrors`](crate::emitter::FromTokenErrors) on the emitter's `Error` so the leaf `From`
conversions come with it. The four capabilities *outside* the bundle
([`TooManyEmitter`](crate::emitter::TooManyEmitter), the missing-separator pair,
[`PrattEmitter`](crate::emitter::PrattEmitter), [`CstEmitter`](crate::emitter::CstEmitter)) are the
less-common ones, named on demand by the parsers that use them; the pre-built emitters implement all
of them regardless.

### The one capability that binds rather than defaults

[`CstEmitter`](crate::emitter::CstEmitter) is the exception worth naming, because it inverts the
default's purpose. Its methods (`cst_start` / `cst_token` / `cst_finish`, and the retro-wrap pair
`cst_mark` / `cst_start_at`) all have no-op defaults, so `Fatal` / `Verbose` / `Silent` /
[`Ignored`](crate::emitter::Ignored) are `CstEmitter` for free and a tree-less parse compiles the
event calls to nothing. But a tree-*producing* parse path bounds `Ctx::Emitter: CstEmitter` anyway —
not to reach a method the default already provides, but to make the bound itself load-bearing: a
wrapper emitter that forwarded the diagnostic surface and forgot the structure surface would produce
a parse whose diagnostics flow perfectly and whose tree is *silently empty*. On every other
capability that is an annoyance; on this one it is a wrong tree nothing downstream can detect, so CST
is the one place the design binds instead of trusting a default. The recording implementation is the
`rowan`-gated `cst::Sink`, the subject of the event-stream CST chapter (its
vocabulary lives in [`crate::cst`]).

## The one method the design refuses to default

Everything past the three emit verbs is defaulted — with a single, deliberate exception:
[`rewind`](crate::Emitter::rewind) has no default body. Every emitter must write it, and that is a
design decision, not an oversight.

The reason is the one-timeline law. A *recording* emitter that inherited a no-op `rewind` would keep
the diagnostics of branches the parse abandoned, attributed to code that never committed — a phantom
diagnostic. Nothing in the crate can detect that: it surfaces as wrong output, never as a panic (the
contracts on [`emit_warning`](crate::Emitter::emit_warning) and
[`rewind`](crate::Emitter::rewind) spell this out). By refusing to default the method, the design
forces every emitter author to answer *how does my state unwind?* For a stateless emitter the answer
is a trivially empty body — [`Fatal`](crate::emitter::Fatal), [`Silent`](crate::emitter::Silent),
and [`Ignored`](crate::emitter::Ignored) each write one — but the compiler makes them write it, so
the question is never skipped by accident. `checkpoint` defaults to `0` and `release` defaults to a
no-op precisely because a stateless emitter genuinely has nothing to mark or reclaim; `rewind` is
the one place where silence would be a correctness bug, so silence is not allowed to be the default.

## checkpoint / rewind / release: the emitter's transactional surface

Three methods make the emitter's log rewindable in step with the cursor, and together they are a
**value-keyed reading model** — no per-save resource, no journal of edits.

- [`checkpoint`](crate::Emitter::checkpoint)`(&self) -> u64` returns a *reading*: a monotone mark
  that names how much has been emitted so far. It borrows `&self` and allocates nothing — it is a
  measurement, not a save. It is also **fail-atomic**: if it unwinds, nothing may have changed. The
  `&self` receiver is the tell — an emitter that registers per-mark state needs interior mutability
  to do it, and owning that atomicity is the price: *reserve before you publish*. That is why there
  is no `reserve_checkpoint` hook to pair with it; reserve-then-publish is already expressible
  inside the body, and the input layer orders its own capture so nothing crate-side is pending when
  the call is made.
- [`rewind`](crate::Emitter::rewind)`(cursor, mark)` restores the emission state *to that reading*,
  dropping exactly what was recorded after it. Like `release`, it can be called **from a drop that
  is already unwinding** — a rolling-back guard settles in its `Drop` — so it must not panic there;
  a panic mid-unwind aborts. The same rider is on
  [`exit_label`](crate::Emitter::exit_label), whose pop `labelled` performs from a drop guard.
- [`release`](crate::Emitter::release)`(mark)` is the eviction dual: it tells the emitter a mark was
  **kept** rather than rewound, so any per-mark bookkeeping can be reclaimed.

[`Verbose`](crate::emitter::Verbose) is the reference. Its `checkpoint` is the length of its emission
log; its `rewind` pops every entry recorded past the mark, newest first, each dropping from the
channel the log entry names; and its `release` is a deliberate no-op, because its rollback state
lives *in the recorded values themselves* — a kept mark is just a number going out of scope, with
nothing to evict. That the model keys on emission **order** rather than on a source offset is what
lets `rewind` drop a speculative zero-width diagnostic while keeping an earlier one at the *same*
offset — the distinction a span-offset heuristic could not make, and the exact defect the
predecessor design shipped (see the checkpoint chapter's
[why-a-copy-not-a-merge](super::arch_checkpoint_rewind) discussion).

The advisory status of `release` is the subtle half. A mark can be abandoned with *neither* a rewind
nor a release — a raw checkpoint merely dropped, say — so an emitter whose correctness depended on
`release` being called would leak. It must not: `release` is strictly bookkeeping, and releasing may
never change the observable emission state. Value-keyed emitters (`Verbose`, `Fatal`, `Silent`,
`Ignored`) inherit the no-op and are correct; only an emitter that keeps a genuine per-mark *table* —
a buffering sink with a checkpoint stack — overrides `release` to pop the kept row, and even a missed
release there is bounded-but-unswept, reclaimed by the next enclosing rewind, never wrong. This is
why the reference posture is value-keyed: the property "correctness does not depend on `release`"
falls out of it for free.

How the *input* drives this trio — one save pinning both the cursor and this mark, and a recording
`cst::Sink` buffering a second (event) log under the very same mark so the tree
rewinds exactly as the diagnostics do — is the [checkpoint chapter](super::arch_checkpoint_rewind)'s
subject, developed there in full. This chapter owns only the emitter's half of the contract.

One last hook belongs to the same timeline. [`commit_token`](crate::Emitter::commit_token) is called
by the input exactly once per **settled** token — consumed, or skipped behind a scan frontier — and
nowhere else. A diagnostics emitter inherits its no-op default; the recording sink overrides it to
record a token event, which is what makes *every* consuming combinator tree-producing with zero
per-combinator code. That auto-emission hook, paired with the `CstEmitter` structuring surface
above, is what the event-stream CST chapter is about.

## The built-ins as design points

Each shipped emitter is a point in a small design space — *what to do with a diagnostic* — and
reading them side by side is the clearest way to see the trait's range.

| Emitter | `Error` | Effect | State |
|---|---|---|---|
| [`Fatal<E>`](crate::emitter::Fatal) | `E` | returns the error, so the first diagnostic ends the parse | none (zero-sized) |
| [`Verbose<E, S>`](crate::emitter::Verbose) | `E` | records every diagnostic and continues | span-keyed channels on one log (`std`/`alloc`) |
| [`Silent<E>`](crate::emitter::Silent) | `E` | drops every diagnostic; keeps the error type | none (zero-sized) |
| [`Ignored`](crate::emitter::Ignored) | `()` | drops everything; error type collapses to `()` | none (zero-sized) |
| `cst::Sink` | inner's | buffers tree events on the rewind timeline, forwarding diagnostics to an inner emitter | event log + inner (`rowan`) |

- **[`Fatal`](crate::emitter::Fatal)** is fail-fast, and the default a bare
  [`Parser::new()`](crate::Parser) installs. Every emit verb returns `Err`, so the first diagnostic
  *is* the `Err` the caller already handles; it stores nothing, allocates nothing, and its `rewind`
  is empty because there is nothing to unwind.
- **[`Verbose`](crate::emitter::Verbose)** is record-and-continue, and the reference value-keyed
  emitter. It collects hard errors and soft warnings into parallel span-keyed channels — plus a
  third channel for recovery holes — all threaded on *one* emission log, so a rewind unwinds an
  abandoned branch's records together and its read-side [`diagnostics()`](crate::emitter::Verbose::diagnostics)
  view can replay every channel interleaved in true emission order. (Whether a diagnostic is an error
  or a warning is a [`Severity`](crate::emitter::Severity) *classification*, never a control-flow
  decision — that stays the emitter's.)
- **[`Silent`](crate::emitter::Silent)** discards but keeps your error type `E`, so it slots into the
  same signatures as `Fatal`/`Verbose` for a best-effort parse where the diagnostics are unwanted.
  **[`Ignored`](crate::emitter::Ignored)** goes further and collapses the error type to `()`, for
  when you want the value and never the diagnostics.
- **`cst::Sink`** is the recording `CstEmitter` the capability section pointed at:
  it wraps an inner diagnostics emitter, forwards its diagnostics, and *also* buffers the CST event
  stream under the one rewind mark. Its internals — the event vocabulary and the `finish`
  materialization — are the event-stream CST chapter's; see [`crate::cst`].

## One assembly, two effect channels

The headline claim compiles: one parser, written once, run under two emitters that reinterpret its
single `emit_error` line — a fail-fast stop under one, a recorded-and-continue under the other. The
lexer is the same tiny hand-written `CharLexer` over single-character tokens the
[engine](super::arch_parsing_engine) and [checkpoint](super::arch_checkpoint_rewind) chapters used,
so nothing but core tokora types is in play.

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
  cache::DefaultCache,
  emitter::{Severity, Silent, Verbose},
  span::Spanned,
};

// One parser, generic over the parse context, so the SAME assembly runs under any emitter. Its
// only diagnostic decision is not made here: `emit_error` returns `Ok` under a collecting emitter
// and `Err` under a fail-fast one, and the `?` obeys whichever it is handed.
fn digits<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CharLexer<'inp>, Ctx>,
) -> Result<Vec<u32>, Error>
where
  Ctx: ParseContext<'inp, CharLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CharLexer<'inp>, Error = Error>,
{
  let mut out = Vec::new();
  while let Some(tok) = inp.next()? {
    let at = *inp.span();
    match tok.into_data() {
      Tok::Digit(n) => out.push(n),
      // A `+` where a digit belongs. THE line the emitter reinterprets: a fatal stop under
      // `Fatal`, filed-and-continue under `Verbose`, dropped under `Silent`.
      Tok::Plus => inp.emitter().emit_error(Spanned::new(at, Error))?,
    }
  }
  Ok(out)
}

// ── Fatal (what `Parser::new()` installs): the first emitted error is the `Err` the caller
//    already handles. Nothing is stored, nothing is allocated. ──
assert_eq!(Parser::new().apply(digits).parse_str("1+2"), Err(Error));

// ── Verbose: the very same `digits`, run to the end of the input. `emit_error` now records
//    rather than returning, so the `?` is a no-op and the parse completes. ──
let mut emitter = Verbose::<Error>::new();
let cache = DefaultCache::<'_, CharLexer<'_>>::default();
let out = Parser::with_context((&mut emitter, cache))
  .apply(digits)
  .parse_str("1+2")
  .expect("Verbose files the diagnostic rather than failing the parse");

// Both digits came through; the `+` was filed, not fatal.
assert_eq!(out, vec![1, 2]);
// The diagnostic is read back off the emitter — the effect channel — after the parse.
assert_eq!(emitter.errors().values().flatten().count(), 1);
// The read-side view classifies it: one Error-tier diagnostic, in emission order.
let tiers: Vec<Severity> = emitter.diagnostics().map(|d| d.severity()).collect();
assert_eq!(tiers, [Severity::Error]);

// ── Silent: the discard channel. Same recovered value as Verbose, but nothing is kept. ──
let cache = DefaultCache::<'_, CharLexer<'_>>::default();
let out = Parser::with_context((Silent::<Error>::new(), cache))
  .apply(digits)
  .parse_str("1+2")
  .expect("Silent drops the diagnostic and never fails");
assert_eq!(out, vec![1, 2]);
```

Three runs, one `digits`. The grammar never branched on the emitter; the emitter branched on the
grammar's behalf. That is the atomic emitter design in one line of code seen three ways.

## Where to go next

The emitter is one of the four seams Part III opens onto the same engine:

- **How the input drives this log in step with the cursor** — one save pinning both channels, the
  LIFO contract, and how a `cst::Sink` buffers a second event log under this
  chapter's mark: the [Checkpoint, rewind & the LIFO contract](super::arch_checkpoint_rewind)
  chapter, which develops the value-keyed-inner composition in full.
- **How committed tokens become a lossless tree** — the [`CstEmitter`](crate::emitter::CstEmitter)
  structuring surface and the [`commit_token`](crate::Emitter::commit_token) auto-emission hook this
  chapter named, and the [`cst`](crate::cst) event stream the `cst::Sink`
  buffers and rewinds: the event-stream CST chapter.
- **How the input is stored** — the `Source` / `Slice` seam this same machinery reads through, byte-
  or text-shaped, owned or borrowed: the [Source, Slice & storage backends](super::arch_source_slice)
  chapter.
- **The flat catalog** — every emitter, capability sub-trait, and read-side type as terse reference
  entries, plus the error taxonomy and the `ParseContext` / `ComposableParseContext` bundle: the
  [errors, emitters & context reference](super::ref_errors_emitters_context).
