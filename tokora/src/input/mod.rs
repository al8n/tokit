//! The input layer: [`InputRef`], its backtracking guards, and the completeness typestate.
//!
//! # Partial input (Sans-I/O mode)
//!
//! An input carries a [`Completeness`] typestate. The default, [`Complete`], is the whole source —
//! today's behaviour, with identical generated code. [`Partial`] is a **prefix of a stream that may
//! still grow**, carrying a runtime `is_final` flag the **driver** states — through
//! [`parse_partial`]'s `is_final` argument — when the last chunk lands. While a partial input is
//! non-final, three conservative *frontier rules* at the scan chokepoint keep a construct that
//! later input could still extend from being mistaken for a finished one — each surfaces an
//! [`Incomplete`](crate::error::Incomplete) instead of yielding, emitting, or reporting end of
//! input (see [`InputRef::next`]).
//!
//! ## Finality belongs to the driver, and it only ever goes one way
//!
//! `is_final` is a fact about the **world** — *the caller has told us no more bytes are coming* —
//! not a fact about the parse. Two laws follow, and both are structural rather than advisory:
//!
//! - **A stream cannot un-end.** The bit is monotone: the internal `Input::seal` raises it and
//!   nothing lowers it. No rollback restores it, because none can: it cannot change while a parse
//!   is running.
//! - **A parser cannot end a stream.** Only the owner of the byte buffer can know the stream ended;
//!   a combinator cannot possibly know it. So the sole writer is `Input::seal`, which takes
//!   `&mut Input` — and an [`InputRef`] borrows that input for its whole life. The borrow checker
//!   therefore forbids sealing while *any* handle is alive, which is to say: while any parser,
//!   guard, attempt, or speculative branch is running. There is no `set_final` on an [`InputRef`],
//!   and that absence is the law (see [`InputRef::is_final`], which carries the compile-fail proof).
//!
//! The second law is what keeps finality out of the rollback set safely. Because it cannot change
//! during a handle's life, no rollback within that life can observe it change — so a
//! [`Checkpoint`](crate::input::Checkpoint) has nothing to save, and a restore has nothing to undo.
//! Checkpointing it would be the mirror bug: a driver seals when the last chunk lands, the parse
//! rolls back to a checkpoint taken earlier, finality reverts to `false`, and the parser waits
//! forever for bytes that will never arrive. Rollback rewinds the parse; it does not rewind the
//! world. The crate-internal `input::lineage` module carries the full cell taxonomy this sits in
//! (grep `CELL_CENSUS`).
//!
//! ## One-token frontier latency
//!
//! The holdback withholds a token whose span reaches the buffer end, so **the last token becomes
//! visible only after more input arrives or the input is marked final**. This one-token latency is
//! correct by construction: a token abutting the end could be the prefix of a longer one (`ab` may
//! become `abc`), and the only proof it will not is more bytes — or `is_final`. With `is_final`
//! set, or on a [`Complete`] input, the rules are inert and the last token yields immediately.
//!
//! ## Terminal beats incomplete, and they never substitute
//!
//! Two verdicts stop a scan, and they mean **opposite** things:
//!
//! - an [`Incomplete`](crate::error::Incomplete) means *"more input may fix this"* — the caller
//!   refills and re-drives;
//! - a **terminal** condition — a resource-limit trip ([`Lexer::check`](crate::Lexer::check) failing
//!   after a token) and the poison boundary it latches — means *"no amount of input will fix this"*
//!   — the caller must stop.
//!
//! They are mutually exclusive, and **the terminal one wins**. Nothing in this crate may disguise a
//! terminal condition as an `Incomplete`: the limit is probed, and latched, *before* the frontier
//! rules are consulted, so only a **non-terminal** item is ever withheld. A limit trip fires — its
//! diagnostic emitted, its poison boundary latched — even when the tripping token ends exactly on
//! the buffer end.
//!
//! The ranking is total because the two conditions are facts about different things. A frontier
//! *item* is **provisional**: whether those bytes are a token or a lexer error depends on bytes that
//! have not arrived, so withholding it is the conservative answer, and a truncated buffer really can
//! make a valid token look like a lex error. A limit trip is not about the item at all — it is a
//! fact about the lexer's accumulated tally, which is **monotone**: re-lexing the same prefix
//! re-trips, and appending bytes can only add to it. No refill can clear it, so answering
//! "incomplete" would be answering with a falsehood — and a costly one. A caller parsing untrusted
//! input would refill, retry, and refill again, re-lexing an ever-growing buffer while the limit
//! that exists to bound exactly that work **never fires**; an attacker who aligns a payload to the
//! chunk boundary (a token that keeps growing keeps ending at it) would bypass the recursion and
//! token limits outright. That is the denial-of-service the ranking forecloses, in the one mode —
//! network and protocol parsing — that exists to face it.
//!
//! This is the **dual** of the [never-recoverable
//! law](crate::error::Incomplete#the-never-recoverable-law), and the pair is one rule read from both
//! ends: recovery may not swallow an `Incomplete` (an unfinished construct is not a malformed one),
//! and the frontier may not swallow a terminal condition (a tripped limit is not an unfinished
//! construct). Neither verdict may be spent as the other.
//!
//! ## No growable source; the caller owns the buffer
//!
//! tokora deliberately has **no growable internal source**. An [`InputRef`] borrows one immutable
//! slice for its whole life, which is what makes zero-copy slices, checkpoints, and rollback a
//! snapshot-copy rather than a journalled edit. Resumption therefore lives with the caller: it owns
//! the byte buffer, and on an incomplete result it appends the next chunk to *its own* buffer and
//! rebuilds the input over the larger slice.
//!
//! Re-lexing the whole prefix each round keeps the frontier rules a pure function of the current
//! slice, and it is **not free**: cumulative lexer work over a session is **Θ(Σ attempt lengths)**
//! — under one-token refills, the triangular figure. That is the honest cost of a continuation-free
//! design, and it is why the arrival schedule must not be the only thing bounding it. Chunking is
//! chosen by the *peer*, so an adversarial chunker can drive Θ(L²) work over an L-byte document
//! while every per-attempt limit stays nominally satisfied. The wall is a **cross-attempt budget**,
//! which cannot live in `L::State` (a fresh per-attempt limiter admits the whole triangle; a shared
//! cumulative one counts replay as input and trips on valid documents) and therefore lives one
//! level up, in [`PartialSession`](crate::input::PartialSession) — whose
//! [`Budget`](crate::input::Budget) is a required constructor argument for
//! exactly this reason. A resuming mode that lexes only the new bytes, giving O(total bytes) for
//! parsers that commit incrementally, is designed and prototype-verified and ships in a later
//! wave; [`ReplayMode`](crate::input::ReplayMode) is sealed and additive so its arrival breaks nothing.
//!
//! ## The Sans-I/O resumption loop
//!
//! Each attempt parses under a rollback-on-drop [`Transaction`], so an incomplete attempt unwinds
//! its emissions and cursor before the retry. **The parser opens that transaction, not the
//! driver**: [`parse_partial`] deliberately does *not* wrap `f` — the caller owns transaction
//! discipline, and that ownership is precisely what makes a commit-and-carry
//! [`PartialSession`](crate::input::PartialSession) possible, because a driver-owned transaction would roll back the very
//! progress a session exists to keep. What [`parse_partial`] does is drive any
//! [`ParseInput`](crate::ParseInput)`<…, `[`Partial`]`>` — a typed fn item like the one below, a
//! named combinator, or a parser written generic over its completeness. The only requirements
//! partial mode adds are that the emitter error implement `From<Incomplete<L::Offset>>` (the
//! construction side of the frontier rules) and [`MaybeIncomplete`](crate::error::MaybeIncomplete)
//! (the recognition side: the atom-layer never-recoverable gate, and the refill loop's own
//! `is_incomplete()` check).
//!
//! ```
//! use core::convert::Infallible;
//! use tokora::{InputRef, Lexer, Partial, SimpleSpan, Source, Token, parse_partial};
//! use tokora::cache::DefaultCache;
//! use tokora::emitter::Fatal;
//! use tokora::error::{Incomplete, MaybeIncomplete};
//!
//! // A tiny word lexer: `[a-z]+` runs, spaces skipped. Resume re-skips spaces from the bumped
//! // offset, so it is faithful under the input layer's re-lexing.
//! #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
//! struct WordKind;
//! impl core::fmt::Display for WordKind {
//!   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("word") }
//! }
//! #[derive(Clone, Debug)]
//! struct Word;
//! impl Token<'_> for Word {
//!   type Kind = WordKind;
//!   type Error = Infallible;
//!   fn kind(&self) -> WordKind { WordKind }
//!   fn is_trivia(&self) -> bool { false }
//! }
//! struct WordLexer<'a> { src: &'a str, start: usize, end: usize, state: () }
//! impl<'a> Lexer<'a> for WordLexer<'a> {
//!   type State = (); type Source = str; type Token = Word;
//!   type Span = SimpleSpan; type Offset = usize;
//!   fn new(src: &'a str) -> Self { Self { src, start: 0, end: 0, state: () } }
//!   fn with_state(src: &'a str, state: ()) -> Self { Self { src, start: 0, end: 0, state } }
//!   fn check(&self) -> Result<(), Infallible> { Ok(()) }
//!   fn state(&self) -> &() { &self.state }
//!   fn state_mut(&mut self) -> &mut () { &mut self.state }
//!   fn into_state(self) {}
//!   fn source(&self) -> &'a str { self.src }
//!   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.start, self.end) }
//!   fn slice(&self) -> &'a str { &self.src[self.start..self.end] }
//!   fn lex(&mut self) -> Option<Result<Word, Infallible>> {
//!     let b = self.src.as_bytes();
//!     self.start = self.end;
//!     while self.start < b.len() && b[self.start] == b' ' { self.start += 1; }
//!     if self.start >= b.len() { self.end = self.start; return None; }
//!     let mut e = self.start;
//!     while e < b.len() && b[e] != b' ' { e += 1; }
//!     self.end = e;
//!     Some(Ok(Word))
//!   }
//!   fn bump(&mut self, n: &usize) { self.end += *n; }
//! }
//!
//! // The emitter error must build from an incomplete signal (the one partial-mode requirement)
//! // and report itself incomplete so the refill loop can detect it.
//! #[derive(Debug, PartialEq)]
//! enum PErr { Incomplete, Other }
//! impl From<Infallible> for PErr { fn from(x: Infallible) -> Self { match x {} } }
//! impl From<Incomplete<usize>> for PErr { fn from(_: Incomplete<usize>) -> Self { PErr::Incomplete } }
//! impl MaybeIncomplete for PErr {
//!   fn is_incomplete(&self) -> bool { matches!(self, PErr::Incomplete) }
//! }
//! impl<'a, T, K: Clone, S, Lg: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lg>>
//!   for PErr { fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lg>) -> Self { PErr::Other } }
//!
//! type Lex<'a> = WordLexer<'a>;
//! type Ctx<'a> = (Fatal<PErr>, DefaultCache<'a, Lex<'a>>);
//!
//! // The parser: collect every word to end of input, under a rollback-on-drop transaction so an
//! // incomplete attempt leaves no trace. `?` propagates the frontier Incomplete; the guard's drop
//! // then rolls back.
//! fn parse_words<'inp>(
//!   inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, (), Partial>,
//! ) -> Result<Vec<String>, PErr> {
//!   let mut txn = inp.begin();
//!   let mut words = Vec::new();
//!   while txn.next()?.is_some() {
//!     words.push(txn.slice().to_string());
//!   }
//!   txn.commit();
//!   Ok(words)
//! }
//!
//! // The chunks arrive in pieces; the caller owns `buffer` and grows it.
//! let chunks = ["foo b", "ar ", "baz"];
//! let mut buffer = String::new();
//! let mut parsed = None;
//! let mut incompletes = 0;
//! for (i, chunk) in chunks.iter().enumerate() {
//!   buffer.push_str(chunk);
//!   let is_final = i + 1 == chunks.len();
//!   let ctx: Ctx = (Fatal::of(), DefaultCache::<'_, Lex<'_>>::default());
//!   match parse_partial(ctx, buffer.as_str(), (), is_final, parse_words) {
//!     Ok(words) => { parsed = Some(words); break; }        // success: whole sentence parsed
//!     Err(e) if e.is_incomplete() => { incompletes += 1; } // frontier: append and re-drive
//!     Err(_) => panic!("a real parse error"),
//!   }
//! }
//! assert_eq!(parsed.unwrap(), ["foo", "bar", "baz"]);
//! assert_eq!(incompletes, 2, "the first two non-final chunks each cut a word at the frontier");
//! ```

use core::marker::PhantomData;

use crate::{
  ParseContext, cache::CachedTokenOf, span::Span, state::recursion_tracker::RecursionLimiter,
};

use super::*;

pub use checkpoint::Checkpoint;
pub use completeness::{Complete, Completeness, Partial, SurfaceIncomplete};
pub use cursor::Cursor;
#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
pub(crate) use input_ref::ClosePayload;
pub(crate) use input_ref::CloseStatus;
pub(crate) use input_ref::Session;
pub use input_ref::{
  Balance, Commit, DelimClass, Descent, DropPolicy, Hole, InputRef, Rollback, Transaction,
};
pub(crate) use lineage::Lineage;
pub use session::{Budget, PartialSession, RedriveFromBase, ReplayMode, SessionRefusal};

#[cfg(any(feature = "std", feature = "alloc"))]
pub use input_ref::{SavepointId, SessionPointId, StackedTransaction};

mod checkpoint;
mod completeness;
mod cursor;
mod input_ref;
mod lineage;
mod session;

/// Storage for [`Input`]'s live-checkpoint lineage stack: inline for 8 ids so the common
/// many-small-parses workload backtracks with no per-parse heap allocation (live-checkpoint
/// nesting depth is typically 1-4), spilling to the heap only past that.
#[cfg(all(any(feature = "std", feature = "alloc"), feature = "smallvec_1"))]
pub(crate) type LineageStack = smallvec_1::SmallVec<[u64; 8]>;
#[cfg(all(any(feature = "std", feature = "alloc"), not(feature = "smallvec_1")))]
pub(crate) type LineageStack = std::vec::Vec<u64>;

/// Storage for a [`StackedTransaction`]'s live savepoints: inline for 2 (savepoint depth per
/// transaction is typically 1-4) so opening a transaction needs no per-parse heap allocation on
/// the common path, spilling to the heap only past that.
#[cfg(all(any(feature = "std", feature = "alloc"), feature = "smallvec_1"))]
pub(crate) type SavepointStack<'inp, 'closure, L> =
  smallvec_1::SmallVec<[(u64, Checkpoint<'inp, 'closure, L>); 2]>;
#[cfg(all(any(feature = "std", feature = "alloc"), not(feature = "smallvec_1")))]
pub(crate) type SavepointStack<'inp, 'closure, L> =
  std::vec::Vec<(u64, Checkpoint<'inp, 'closure, L>)>;

/// Debug-only witness of the input identity a checkpoint was created under, used by
/// [`InputRef::restore`] to reject a checkpoint restored into a foreign input.
///
/// The live-checkpoint *lineage* stack that enforces the last-in, first-out discipline
/// lives on [`Input`] itself (in its [`Lineage`] memos) and is maintained in **every**
/// allocator build; this witness carries only the cross-input identity, whose atomic id
/// source keeps it behind the debug + `target_has_atomic = "ptr"` gate. In release builds,
/// and in allocator-less builds, it is absent and `restore` performs no foreign-input
/// check.
#[cfg(all(
  debug_assertions,
  any(feature = "std", feature = "alloc"),
  target_has_atomic = "ptr"
))]
pub(crate) use witness::Witness;

#[cfg(all(
  debug_assertions,
  any(feature = "std", feature = "alloc"),
  target_has_atomic = "ptr"
))]
mod witness {
  use core::sync::atomic::{AtomicUsize, Ordering};

  /// Hands out a distinct identity to every [`Input`](super::Input) so a checkpoint
  /// carries a witness of the input that produced it.
  static NEXT_INPUT_ID: AtomicUsize = AtomicUsize::new(0);

  /// A process-unique identity for one input, stamped into every checkpoint it saves so
  /// a restore can reject a checkpoint that belongs to a different input.
  #[derive(Debug)]
  pub(crate) struct Witness {
    input_id: usize,
  }

  impl Witness {
    /// Creates a witness with a fresh, process-unique input identity.
    pub(crate) fn new() -> Self {
      Self {
        input_id: NEXT_INPUT_ID.fetch_add(1, Ordering::Relaxed),
      }
    }

    /// The identity of the input this witness belongs to.
    pub(crate) fn input_id(&self) -> usize {
      self.input_id
    }
  }

  impl Clone for Witness {
    /// A clone is a **new** input: it gets a fresh identity, so a clone's checkpoints and
    /// the original's can never be confused for one another.
    fn clone(&self) -> Self {
      Self::new()
    }
  }
}

/// The context for parsing input
pub struct InputContext<E, C> {
  emitter: E,
  cache: C,
  recursion: RecursionLimiter,
}

impl<E, C> InputContext<E, C> {
  /// Creates a new `InputContext` with the given emitter and cache.
  ///
  /// The recursion budget defaults to [`RecursionLimiter::new`] — depth **500**, protection on
  /// — and is changed with [`with_recursion_limiter`](Self::with_recursion_limiter).
  #[inline(always)]
  pub const fn new(emitter: E, cache: C) -> Self {
    Self {
      emitter,
      cache,
      recursion: RecursionLimiter::new(),
    }
  }

  /// Sets the **recursion budget** this parse descends against.
  ///
  /// The budget is per *input session*, not per parser: every recursive-descent site that goes
  /// through [`InputRef::descend`](InputRef::descend) — today both Pratt engines' frame
  /// prologues — draws on this one cell, so two expression parsers composed into one grammar
  /// cannot each spend the full depth. Exceeding it fails the parse with the always-terminal
  /// [`RecursionLimitReached`](crate::error::RecursionLimitReached).
  ///
  /// Pass [`RecursionLimiter::unlimited`] to opt out, or
  /// [`RecursionLimiter::with_limitation`] to tighten. Opting out means a sufficiently nested
  /// input can exhaust the native stack, which is what the default exists to prevent.
  #[inline(always)]
  pub const fn with_recursion_limiter(mut self, recursion: RecursionLimiter) -> Self {
    self.recursion = recursion;
    self
  }

  /// Decomposes this context into its emitter, cache, and recursion-budget components.
  #[inline(always)]
  pub fn into_components(self) -> (E, C, RecursionLimiter) {
    (self.emitter, self.cache, self.recursion)
  }
}

/// A zero-copy token stream adapter that bridges Logos and Chumsky.
///
/// `Input` is the core integration layer between [Logos](https://github.com/maciejhirsz/logos)
/// lexical analysis and [Chumsky](https://github.com/zesterer/chumsky) parser combinators.
/// It efficiently wraps a Logos token source and implements all necessary Chumsky input traits,
/// allowing you to use Chumsky parsers directly on Logos tokens.
///
/// # Zero-Copy Design
///
/// `Input` doesn't allocate or copy tokens. Instead, it maintains a cursor position
/// and calls Logos on-demand as the parser consumes tokens. This makes it efficient for
/// large inputs and streaming scenarios.
///
/// # State Management
///
/// For stateful lexers (those with non-`()` `Extras`), `Input` maintains the lexer
/// state and passes it through token-by-token. This allows for context-sensitive lexing
/// patterns. Because the adapter clones `Extras` each time it polls Logos, it is best to
/// keep your state `Copy` or otherwise cheap to clone. If you need heavy state, consider
/// storing handles (e.g. `Arc`) inside `Extras` so clones stay inexpensive.
///
/// # Type Parameters
///
/// - `'inp`: The lifetime of the input source
/// - `T`: The token type implementing [`Token<'inp>`]
///
/// # Implemented Traits
///
/// This type implements all core Chumsky input traits:
/// - [`Input`](chumsky::input::Input) - Basic input stream functionality
/// - [`ValueInput`](chumsky::input::ValueInput) - Token-by-token consumption
/// - [`SliceInput`](chumsky::input::SliceInput) - Slice extraction from source
/// - [`ExactSizeInput`](chumsky::input::ExactSizeInput) - Known input length
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use tokora::{Token, Input, TokenExt};
/// use logos::Logos;
/// use chumsky::prelude::*;
///
/// #[derive(Logos, Debug, Clone, Copy, PartialEq)]
/// #[logos(skip r"[ \t\n]+")]
/// enum MyTokens {
///     #[regex(r"[0-9]+")]
///     Number,
///     #[token("+")]
///     Plus,
/// }
///
/// // Create a token stream from input
/// let input = "42 + 13";
/// let stream = MyToken::lexer(input); // Returns Input<'_, MyToken>
///
/// // Use with Chumsky parsers
/// let parser = any().repeated().collect::<Vec<_>>();
/// let tokens = parser.parse(stream).into_result();
/// ```
///
/// ## Stateful Lexing
///
/// ```rust,ignore
/// #[derive(Default, Clone)]
/// struct LexerState {
///     brace_count: usize,
/// }
///
/// #[derive(Logos, Debug, Clone, Copy)]
/// #[logos(extras = LexerState)]
/// enum MyTokens {
///     #[token("{", |lex| lex.extras.brace_count += 1)]
///     LBrace,
///     #[token("}", |lex| lex.extras.brace_count -= 1)]
///     RBrace,
/// }
///
/// let input = "{ { } }";
/// let initial_state = LexerState::default();
/// let stream = Input::with_state_and_context(input, initial_state, ctx.provide());
/// ```
pub(crate) struct Input<'inp, L, Ctx = DefaultCache<'inp, L>, Lang: ?Sized = (), Cmpl = Complete>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  input: &'inp L::Source,
  state: L::State,
  span: L::Span,
  cache: Ctx::Cache,
  /// The **parked front token**: one lexed-but-unconsumed token that the configurable cache
  /// refused to retain.
  ///
  /// Every `to`-shaped scan stop and every scan-path decline must leave its token unconsumed, and
  /// the crate's answer is "an unconsumed token lives at the front of the stream"
  /// (`input_ref::scan`'s module docs). A zero-capacity cache accepts no push, so that token used
  /// to be **dropped** — and with it the promise that the cursor after a stop is the stopping
  /// token's start *no matter who lexed it*. This slot is the retained home the promise needs,
  /// outside `Ctx::Cache` so no cache implementation can refuse it.
  ///
  /// # It is the FRONT of the stream
  ///
  /// Logically the stream is `parked?` then `cache[front..back]`. That ordering is why
  /// [`InputRef::cursor`] consults this slot **first** and [`InputRef::offset`] consults it
  /// **last** — the parked token is the oldest retained token and the cache back is the newest.
  ///
  /// # Occupied only when the cache refused a put-back
  ///
  /// Every put-back site runs with a cache that has room (the scanner pops at most one token
  /// before deciding on it; the `try_expect*`/`probe_close` scan paths are reached only with
  /// nothing at the front), so for every cache in this crate a refusal means capacity 0, and
  /// therefore an occupied slot implies an empty cache. The reads do not *depend* on that — they
  /// are ordered by the stream order above and are correct for any `Cache` — but
  /// [`InputRef::hold_front`] debug-asserts it, so a `Cache` implementation that refuses a push
  /// into a non-empty cache is caught at its cause rather than producing an out-of-order front.
  ///
  /// # Uncounted, and dropped by every restore
  ///
  /// It is *not* a cache entry: it is never recorded on the cache-push counter, and
  /// [`InputRef::restore`] clears it outright rather than replaying it. That is sound for the same
  /// reason the cache's post-save tail-drop is: the restored span/state land at or before the
  /// parked token's start, so the region re-lexes and — by the `Lexer` determinism contract —
  /// reproduces it. The extra scan a drop costs is the *baseline* behaviour of the only cache that
  /// can park anything.
  pending: Option<CachedTokenOf<'inp, L>>,
  /// The completeness finality (`is_final`) storage: the zero-sized `()` for [`Complete`] — so the
  /// complete input has the identical layout it had before this typestate existed — and a `bool`
  /// for [`Partial`]. The frontier rules read it only when [`Completeness::PARTIAL`] holds, so it
  /// is inert (and free) on the complete path.
  ///
  /// # The one WORLD cell, and why it is not in the rollback set
  ///
  /// This is the crate's only **world fact** (see the [cell taxonomy](lineage)): it records what
  /// the *caller* knows about the outside world — *no more bytes are coming* — not what the parse
  /// has done. It is therefore **monotone** ([`seal`](Self::seal) raises it; nothing lowers it) and
  /// **driver-owned**: its sole writer is [`seal`](Self::seal), which takes `&mut Input`.
  ///
  /// That last part is the whole guarantee, and the borrow checker enforces it. An
  /// [`InputRef`](InputRef) — the only thing a parser is ever handed — mutably borrows this
  /// `Input` for its entire life, so **while any handle exists, this cell is unreachable**. Every
  /// parser, guard, attempt, and speculative branch lives inside such a handle. So finality is
  /// *constant for the life of a handle*, no rollback can ever observe it change, and a
  /// [`Checkpoint`] has nothing to save. It is outside the rollback set by construction, not by
  /// omission.
  ///
  /// Checkpointing it instead would be the **mirror bug**: a driver seals when the last chunk
  /// lands, a parse rolls back to a checkpoint taken earlier, finality reverts to `false`, and the
  /// parser waits forever for bytes that will never arrive. Rollback rewinds the *parse*; it does
  /// not rewind the *world*.
  finality: Cmpl::Finality,
  /// High-water mark: lexer errors whose span ends at or before this offset have
  /// already been emitted (e.g. during a peek that lexed past this point), so the
  /// consume path must not report them again when it re-lexes the same region.
  ///
  /// Its subject is **the bound [`emitter`](Self::emitter)'s log**, and that is a structural
  /// fact rather than a discipline: the emitter is bound at
  /// [`with_state_and_context`](Self::with_state_and_context) and owned for the input's whole
  /// life, so this mark can only ever describe the one log it was raised against. See the
  /// [`emitter`](Self::emitter) field for the aggregate the two suppression watermarks form
  /// with it.
  emitted_error_end: L::Offset,
  /// The **front-report watermark**: a lineage fact about the emitter's log.
  ///
  /// `Some(end)` means the emitter's **current** log holds an unexpected-token report whose
  /// subject is the token the current regime lexes at the stream front, ending at `end`. `None`
  /// claims nothing.
  ///
  /// It exists because "has this front token already been reported?" is a *transactional*
  /// question and cannot be answered from a parser's stack frame. A driver-local flag — committed
  /// position, with or without a re-key count beside it — cannot observe a restore, because a
  /// restore is exactly the operation that erases frame-visible traces: it rewinds the emitter,
  /// destroys the front, and returns position to a value the frame cannot distinguish from
  /// "nothing happened".
  ///
  /// The invariant is kept inductive by three writers and no others:
  ///
  /// - **set** — [`InputRef::emit_unexpected_front`](InputRef::emit_unexpected_front) publishes it
  ///   in the same body that appends the report, and only if the append returned `Ok`;
  /// - **clear** — a re-key ([`set_state`](InputRef::set_state) / [`state_mut`](InputRef::state_mut))
  ///   takes it, because the regime that lexed its subject has died. Cleared *before* the cache
  ///   clear, so an unwind through that caller code leaves it already disarmed and a later arm
  ///   emits rather than suppresses;
  /// - **restore** — captured into every [`Checkpoint`] and restored beside
  ///   `emitted_error_end`, in the same no-caller-code block as the emitter rewind.
  ///
  /// A witness must share the rollback semantics of the fact it witnesses. This one does: its
  /// subject is the emission log, and the log is restored in the same move.
  ///
  /// # Which log, and why that is no longer a question
  ///
  /// The log is **the bound [`emitter`](Self::emitter)'s**, decided once at
  /// [`with_state_and_context`](Self::with_state_and_context) and unchangeable thereafter. A
  /// suppression watermark whose subject is one emitter's log, stored on a struct whose emitter
  /// were chosen *per borrow*, would be unsound by construction — set under one log and read
  /// under another it claims a report that log never received, and because these watermarks
  /// **suppress**, the consequence is a silently dropped diagnostic rather than a duplicated
  /// one. Binding the emitter to the input is what makes that shape unconstructable:
  /// [`as_ref`](Self::as_ref) takes no emitter, so there is no per-borrow choice to get wrong.
  ///
  /// `poison_boundary`, below, was never in that class and still is not: its subject is the
  /// *limiter's* trip, which travels with the `Input` itself and means the same thing under any
  /// log. It gates control flow, not dedup.
  front_reported_end: Option<L::Offset>,
  /// Sticky limit-error boundary, held at the input level so it survives across
  /// the fresh lexers `InputRef` builds per operation.
  ///
  /// `None` is unpoisoned. `Some(off)` records the *durable frontier* a limit trip
  /// latched at: the offset up to which the pre-trip tokens remain reproducible by
  /// re-lexing. When a lexer surfaces a state/limit error (its
  /// [`check`](crate::Lexer::check) fails after scanning a token) the temporary
  /// lexer latches to EOF, but that latch dies with the lexer; persisting the
  /// frontier here means a scanner whose lex position has reached `off`
  /// short-circuits to its poisoned outcome **without** rebuilding a lexer or
  /// rescanning the tripping token, keeping error-recovery work bounded on
  /// untrusted input. Lexing *strictly before* the frontier still proceeds, so a
  /// cache prefix drained after the trip stays replayable. It is a fact of one
  /// lineage: [`save`](InputRef::save) captures it and [`restore`](InputRef::restore)
  /// copies it back verbatim, since a last-in, first-out restore returns to exactly
  /// the lineage the checkpoint recorded.
  ///
  /// # A trip is TERMINAL, and terminal outranks incomplete
  ///
  /// A latched boundary is the crate's **terminal** condition: it means *no amount of further input
  /// will change this outcome*. That is what separates it from an
  /// [`Incomplete`](crate::error::Incomplete), which means the opposite — *more input may fix
  /// this* — and the two must never substitute for each other. In [`Partial`] mode they can meet, on
  /// the very item the frontier rules hold back, and the rule is that **the trip wins**: it is
  /// probed and latched *before* the holdback is consulted, so a trip whose tripping token happens
  /// to end exactly on a chunk boundary still emits its diagnostic and still latches here. See
  /// [the law](crate::input#terminal-beats-incomplete-and-they-never-substitute) for why the ranking
  /// is total, and what an attacker could do without it.
  poison_boundary: Option<L::Offset>,
  /// The **recursion budget** this parse descends against: live descent depth plus the limit it
  /// may not exceed, configured at [`with_state_and_context`](Self::with_state_and_context) from
  /// [`InputContext::with_recursion_limiter`] and defaulting to depth 500.
  ///
  /// Its one writer is the [`Descent`](InputRef::descend) guard: [`InputRef::descend`] raises the
  /// depth, and the guard's `Drop` lowers it on **every** exit of the frame — return, `?`, or
  /// unwind, identically in `std` and `no_std`. There is no `recursion_mut`, so no caller can
  /// leave the cell unbalanced.
  ///
  /// # Deliberately outside the rollback set
  ///
  /// A [`Checkpoint`] does **not** carry it, and that is the same argument
  /// [`finality`](Self::finality) makes one field up, arriving from the other direction. Depth is
  /// a fact about the *control stack*, not about input progress: a save and the restore that
  /// returns to it happen at the same frame depth by construction, so the cell cannot be observed
  /// to change across the pair — and a restore performed at a *different* depth (expressible only
  /// through `unstable-raw` checkpoint smuggling) must not clobber the true live depth with a
  /// stale one. On an unwind, frames pop; only a witness that pops with them is right, and that
  /// witness is the guard's destructor, which behaves the same under both drop policies and both
  /// `cfg`s. State-carried depth would instead be double-restored on a `std` unwind and leaked on
  /// a `no_std` one.
  ///
  /// It is also *not* the lexer's [`State`](crate::State), for three independent reasons: a scan
  /// commit overwrites `state` wholesale with the token's own snapshot, `state_mut` re-keys the
  /// world (dropping the token cache **and** any latched poison boundary), and a checkpoint
  /// captures it. See [`RecursionLimiter`] for the lexer-side tracker, which is a different cell
  /// with a different subject.
  recursion: RecursionLimiter,
  /// The **bound emitter** — the one emission log this input's parse writes, owned here for the
  /// input's whole life and paired with it at
  /// [`with_state_and_context`](Self::with_state_and_context).
  ///
  /// # The aggregate this anchors
  ///
  /// The emission mark, [`emitted_error_end`](Self::emitted_error_end),
  /// [`front_reported_end`](Self::front_reported_end) and
  /// [`poison_boundary`](Self::poison_boundary) are **one restore group**: a
  /// [`Checkpoint`] captures all four and a [`restore`](InputRef::restore) puts all four back
  /// together, in one no-caller-code block (see [`Checkpoint`]'s fields). The rollback structure
  /// has always treated them as a unit; owning the emitter here is what makes the *live*
  /// structure say the same thing. Before it, two of the four were facts about a log the input
  /// did not hold, and pairing them with a different log was a call-site discipline the compiler
  /// could not check.
  ///
  /// # No mid-life mutator, deliberately
  ///
  /// There is [`emitter`](Self::emitter) (shared, for observation between handles) and
  /// [`into_emitter`](Self::into_emitter) (by value, once the parse is over) — and no
  /// `emitter_mut`. Mutation during a parse goes through an [`InputRef`], whose
  /// [`emitter`](InputRef::emitter) is the audited road every emission and every rewind already
  /// travels. A second way in would be a second place to reason about the aggregate above.
  emitter: Ctx::Emitter,
  /// The lineage memos — the bookkeeping backtracking rewinds an abandoned continuation with:
  /// the live-checkpoint stack, the pin set, and the cache-push/checkpoint-id/savepoint counters.
  /// Gathered behind one guardian (see [`Lineage`] and its [module](lineage) for the
  /// single-writer taxonomy) and kept after the ground-truth cells above, so the scanner-hot
  /// fields pack ahead of it on the `next()` path.
  lineage: Lineage,
  /// Trace nesting depth (the `trace` feature). [`traced`](crate::traced) bumps it on enter
  /// and drops it on exit, so instrumentation indents by call depth. A plain field, borrowed
  /// by [`InputRef`]; trace events travel out of band (stderr), never through the emitter, so
  /// a rewind never eats them.
  #[cfg(feature = "trace")]
  depth: usize,
  /// Debug-only witness of the input identity a checkpoint was created under (see
  /// [`InputRef::restore`]); the lineage stack itself lives in the [`Lineage`] memos.
  #[cfg(all(
    debug_assertions,
    any(feature = "std", feature = "alloc"),
    target_has_atomic = "ptr"
  ))]
  witness: Witness,
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> core::fmt::Debug for Input<'inp, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::Source: core::fmt::Debug,
  L::State: core::fmt::Debug,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Cache: core::fmt::Debug,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("Input")
      .field("input", &self.input)
      .field("state", &self.state)
      .field("span", &self.span)
      .field("cache", &self.cache)
      // A `CachedToken` has no `Debug`; whether one is parked is the observable that matters.
      .field("parked", &self.pending.is_some())
      .finish()
  }
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> Input<'inp, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// Creates an input over `input` with `state`, **bound to** the emitter and cache in `context`
  /// — the one point at which a parse and the emitter serving it are paired, and the reason
  /// nothing downstream has to re-establish which log a watermark describes. `context` is exactly
  /// what [`ParseContext::provide`](crate::ParseContext::provide) returns.
  ///
  /// # Panics
  ///
  /// Twice, for the two ways a pairing can be wrong — see
  /// [`SourceIdentity::covers`](crate::source::SourceIdentity::covers):
  ///
  /// - if the emitter is bound to a **provably different** source value than this input reads;
  /// - if it is bound to a **strictly shorter extent** of that source than this input reads.
  ///   The parse can peek past the sink's end without committing anything there and still let
  ///   those bytes shape the tree, and the result carries no out-of-bounds span and no
  ///   uncovered gap for a later check to find. A **longer** bound extent is the fixed-arena
  ///   streaming shape and is not refused.
  ///
  /// This is the one point at which both halves are in hand, which is why the check lives here:
  /// the event log a wrongly-paired sink produces is byte-identical to a legal one — and for
  /// the prefix case it is not merely indistinguishable but *valid* — so nothing downstream can
  /// tell. Because the pairing is then fixed for the input's whole life, checking it here covers
  /// **every** borrow, and costs one check per input rather than one per handle.
  ///
  /// Both refusals are deliberately conservative: they fire only where
  /// [`Source::REFERENT_IS_BYTES`](crate::Source::REFERENT_IS_BYTES) says the reference *is*
  /// the data, so that an inequality is proof and the extent is a byte length rather than a
  /// handle size. For a sized backing (every owned handle, and the `&str` / `&[u8]` reference
  /// forms) neither projection means anything, so nothing is refused. No program that produces
  /// a correct tree today reaches either panic.
  ///
  /// It is **not** `const`, and could not be: the address projection is not const-callable at
  /// the MSRV (`E0015` on `<*const u8>::addr`). `Input` is `pub(crate)`, so this is not a
  /// consumer-visible change.
  #[inline(always)]
  pub fn with_state_and_context(
    input: &'inp L::Source,
    state: L::State,
    context: InputContext<Ctx::Emitter, Ctx::Cache>,
  ) -> Self {
    let (emitter, cache, recursion) = context.into_components();
    // The source-identity handshake. See `Emitter::bound_source`.
    //
    // `bound_source()` is `None` for every emitter that binds no source — 30 of the 31 core
    // `Emitter` impls in this tree — so this folds to nothing for them. Once per `Input`, at the
    // moment the pair forms: never per borrow, never per token.
    // Spelled without a let-chain on purpose: let-chains are unstable at the MSRV (1.87),
    // and this crate has now shipped that mistake twice.
    if <L::Source as crate::Source<L::Offset>>::REFERENT_IS_BYTES {
      if let Some(bound) = crate::Emitter::<'inp, L, Lang>::bound_source(&emitter) {
        let current = crate::source::SourceIdentity::of(input);
        // Two questions, two answers. Origin is an equality; extent is an ordering, and only
        // in one direction. Collapsing them into a single `==` would refuse the streaming
        // shape; collapsing them into origin alone is what let a prefix through.
        assert!(
          bound.addr() == current.addr(),
          "the emitter is bound to a different source value than this parse reads \
           (emitter: {bound}, parse: {current}) — bind the sink to the same `&source` you \
           hand the parser. A same-length foreign source produces a tree whose text is the \
           sink's and whose structure is the parse's, and no later check can see it."
        );
        assert!(
          bound.extent() >= current.extent(),
          "the emitter is bound to a shorter extent of this source than the parse reads \
           (emitter: {bound}, parse: {current}) — bind the sink to a slice at least as long \
           as the one you hand the parser. The parse can peek past the sink's end, commit \
           nothing there, and still let those bytes choose the tree's shape; the result \
           materializes with no out-of-bounds span and no uncovered gap, so nothing later \
           sees it. The opposite pairing — a sink bound to a LONGER extent — is the \
           fixed-arena streaming shape and is accepted here; its unparsed tail is `finish`'s \
           business, reported as `UncoveredGap` or tiled by `finish_partial`."
        );
      }
    }

    Self {
      input,
      state,
      span: L::Span::new(L::Offset::default(), L::Offset::default()),
      cache,
      pending: None,
      // Born OPEN: non-final (`Partial`) or final-by-definition (`Complete`). A streaming driver
      // states the end of the stream by calling [`seal`](Self::seal), the one monotone finality
      // transition.
      finality: Cmpl::initial(),
      emitted_error_end: L::Offset::default(),
      front_reported_end: None,
      poison_boundary: None,
      // The caller's budget, moved in as given. Every constructor of `RecursionLimiter` starts
      // at depth 0, so this is depth 0 for every context the crate or a caller can build without
      // deliberately walking one deeper first — and a deliberately-deep one is a caller stating
      // that this parse continues someone else's descent, which is exactly what the cell means.
      recursion,
      emitter,
      lineage: Lineage::new(),
      #[cfg(feature = "trace")]
      depth: 0,
      #[cfg(all(
        debug_assertions,
        any(feature = "std", feature = "alloc"),
        target_has_atomic = "ptr"
      ))]
      witness: Witness::new(),
    }
  }

  /// The bound emitter, by shared reference — observability *between* handles, which is the one
  /// window in which an `Input` is reachable and its log is settled.
  ///
  /// Deliberately shared-only; see the [field](Self::emitter) for why there is no `emitter_mut`.
  ///
  /// `allow(dead_code)` rather than a feature gate: the callers are the `fuzz` harness's oracles
  /// plus test suites behind a dozen different feature combinations, so an exact gate would be a
  /// long disjunction that has to be re-derived every time a suite moves — and getting it wrong
  /// fails the build in one matrix leg only. It is generic and monomorphized only where called,
  /// so an unused build pays nothing for it.
  #[allow(dead_code)]
  #[inline(always)]
  pub(crate) fn emitter(&self) -> &Ctx::Emitter {
    &self.emitter
  }

  /// Consumes the input and returns the bound emitter — the extraction point for an **owned
  /// recording** emitter (a CST [`Sink`](crate::cst::Sink)), whose `finish` takes `self` and so
  /// must outlive the input that fed it.
  #[allow(dead_code)]
  #[inline(always)]
  pub(crate) fn into_emitter(self) -> Ctx::Emitter {
    self.emitter
  }

  /// **Seals** the input: the stream has ended, and no further bytes will arrive. The sole writer
  /// of the [`finality`](Self::finality) world cell, and the sole way a
  /// [`Partial`] input ever becomes final.
  ///
  /// # Monotone, and driver-only
  ///
  /// Two properties, and each is load bearing:
  ///
  /// - **Monotone.** It raises the bit and has no inverse — there is no `set_final(false)`, in any
  ///   build, on any type. **A stream cannot un-end**, so nothing in the crate can express the
  ///   claim that it did. Sealing twice is a no-op; sealing a [`Complete`] input is a no-op (it is
  ///   final by definition).
  /// - **Driver-only.** It takes `&mut Input` — and an [`InputRef`] mutably borrows the `Input` for
  ///   its whole life. So a driver may seal only when **no handle is alive**, and therefore never
  ///   while a parser, a guard, an attempt, or any speculative branch is running. The borrow
  ///   checker, not a convention, is what keeps a combinator from asserting a fact about the world
  ///   that only the owner of the byte buffer can know.
  ///
  /// Together they put finality *outside the rollback set by construction*: it cannot change during
  /// a handle's life, so no rollback within that life can observe it change, so a [`Checkpoint`]
  /// has nothing to save and a restore has nothing to undo.
  ///
  /// A driver that already knows the finality at construction says so through
  /// [`parse_partial`]'s `is_final` argument, which routes here. A driver holding a live input
  /// across chunks (the in-place two-phase shape: drain non-final, learn the socket closed, drain
  /// on) seals between handles.
  #[inline(always)]
  pub(crate) fn seal(&mut self) {
    Cmpl::seal(&mut self.finality);
  }

  /// The number of **pinned** checkpoints on this input's [`Lineage`] — the begin points of the
  /// currently-live guards, attempts, and [session points](InputRef::begin_point).
  ///
  /// Asked of the [`Input`] rather than of an [`InputRef`] because the question this answers is
  /// about the moment *after* a handle dies: an [`InputRef`] dropped with session points still
  /// open releases their pins ([`InputRef`]'s `Drop`), so this reads `0` again — the pin set holds
  /// exactly the live begin points, and with no handle alive there are none. Gated to its callers
  /// (the session tests and the `fuzz` harness's abandon oracle).
  #[cfg(any(
    all(
      test,
      any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
      feature = "std"
    ),
    all(feature = "fuzz", feature = "std")
  ))]
  pub(crate) fn pinned_checkpoints_len(&self) -> usize {
    self.lineage.pinned_len()
  }

  /// The number of **live** checkpoints on this input's [`Lineage`] — the ids saved and neither
  /// restored nor released. The [`Input`]-level twin of
  /// [`InputRef::live_checkpoints_len`](InputRef), for the same after-the-handle-dies question:
  /// an abandoned session point releases its lineage entry too, so it does not strand one.
  #[cfg(all(
    test,
    any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
    feature = "std"
  ))]
  pub(crate) fn live_checkpoints_len(&self) -> usize {
    self.lineage.live_len()
  }

  /// Creates a zero-copy reference adapter for this input, borrowing the **bound** emitter.
  ///
  /// There is no emitter parameter and no per-borrow emitter choice: the pairing was made at
  /// [`with_state_and_context`](Self::with_state_and_context), which is also where the
  /// source-identity handshake that used to live here now runs. Two handles taken from one input
  /// therefore always write the same log, so a suppression watermark carried across them
  /// describes exactly the log it was raised against.
  #[inline(always)]
  pub fn as_ref<'closure>(&'closure mut self) -> InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl> {
    InputRef {
      input: &self.input,
      state: &mut self.state,
      cache: &mut self.cache,
      pending: &mut self.pending,
      span: &mut self.span,
      // A read-only SNAPSHOT of the world cell, `Copy` (a ZST for `Complete`) — copied rather than
      // borrowed so the frontier rules read it without an extra word, keeping the `Complete`
      // reference layout unchanged (a `&()` would still cost a pointer). The handle exposes no
      // mutator, and this borrow of `self` locks out `seal` for the handle's whole life, so the
      // snapshot cannot go stale: finality is CONSTANT while any handle lives.
      finality: self.finality,
      emitted_error_end: &mut self.emitted_error_end,
      front_reported_end: &mut self.front_reported_end,
      poison_boundary: &mut self.poison_boundary,
      // The recursion budget, borrowed like the ground-truth cells above rather than snapshotted
      // like `finality`: a handle's frames raise and lower it, and the value must outlive them.
      recursion: &mut self.recursion,
      // The lineage memos, the emitter borrow, and the session-point stack, in one cell (see
      // `input_ref::session`): the stack starts empty and stays unallocated until the first
      // `InputRef::begin_point`, so a reference that never opens a session pays a few zeroed
      // words once, here, and nothing thereafter. The cell's `Drop` is what releases a point
      // abandoned with the handle — its pin, its lineage entry, and its emitter mark alike
      // (the emitter borrow lives in the cell precisely so that drop can reach it).
      session: Session::new(&mut self.lineage, &mut self.emitter),
      #[cfg(feature = "trace")]
      depth: &mut self.depth,
      #[cfg(all(
        debug_assertions,
        any(feature = "std", feature = "alloc"),
        target_has_atomic = "ptr"
      ))]
      witness: &self.witness,
      _marker: PhantomData,
    }
  }
}

/// Opens a **Sans-I/O partial parse session** over `src` and drives the parser `f` — any
/// [`ParseInput`]`<'inp, L, O, Ctx, Lang, `[`Partial`]`>` — returning whatever
/// `f` returns.
///
/// This is the partial-mode entry point, the [`Partial`] instantiation of the same trait
/// vocabulary the complete-only [`Parse`] API drives at its default: a typed fn item, a named
/// combinator chain, or a parser written generic over its completeness parameter all satisfy the
/// bound (write once, run in both modes — see the [`input`](self) module docs). `f`
/// receives an [`InputRef`] in [`Partial`] mode set to `is_final`, so the
/// [frontier rules](crate::input) are active while non-final: consuming across the frontier
/// surfaces an [`Incomplete`](crate::error::Incomplete) on the `Err` channel, which the parser
/// propagates out. `ctx` supplies the emitter and cache exactly as [`Parse`] does.
///
/// The `Partial: SurfaceIncomplete` bound requires that the emitter's error implements
/// `From<Incomplete<L::Offset>>` (constructing the frontier sentinel) and
/// [`MaybeIncomplete`](crate::error::MaybeIncomplete) (recognizing it — the atom-layer
/// never-recoverable gate and the caller's own refill check); complete parses see neither.
///
/// # The refill loop
///
/// The caller owns the buffer. Each attempt builds a fresh input over the current buffer slice, so
/// there is **no growable source inside tokora**: on an incomplete result the caller appends the
/// next chunk to *its own* buffer and calls this again over the larger slice. Inside `f`, parsing
/// under a rollback-on-drop [`Transaction`] means an incomplete attempt unwinds its emissions and
/// cursor cleanly before the retry — a transaction `f` opens, because this driver deliberately
/// wraps nothing. See the [`input`] module docs for the full runnable loop and the one-token
/// frontier-latency guarantee.
///
/// # Cost
///
/// Every attempt re-lexes the whole prefix, so cumulative lexer work across a refill loop is
/// **Θ(Σ attempt lengths)** — the triangular figure under one-token refills. This function returns
/// nothing that could carry a committed frontier or a cross-attempt budget, so a caller facing
/// untrusted arrival fragmentation should drive [`PartialSession`] instead, whose [`Budget`] bounds
/// exactly that sum.
#[inline]
pub fn parse_partial<'inp, L, Ctx, Lang, O, P>(
  ctx: Ctx,
  src: &'inp L::Source,
  state: L::State,
  is_final: bool,
  mut f: P,
) -> Result<O, <Ctx::Emitter as crate::Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Lang: ?Sized,
  Ctx: ParseContext<'inp, L, Lang>,
  Partial: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: crate::ParseInput<'inp, L, O, Ctx, Lang, Partial>,
{
  let mut input = Input::<L, Ctx, Lang, Partial>::with_state_and_context(src, state, ctx.provide());
  // The driver states the world fact BEFORE the parser ever sees a handle — and it is the only
  // party that can. `seal` takes `&mut Input`, so it is unreachable from the `&mut InputRef` `f`
  // is handed: `f` cannot end the stream, at any depth, inside any speculative branch.
  if is_final {
    input.seal();
  }
  let mut input_ref = input.as_ref();
  f.parse_input(&mut input_ref)
}

#[cfg(test)]
#[allow(warnings)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
