//! The spent-sink handle: [`Cst`], what a lossless driver hands back once the parse is over.
//!
//! # Why the artefact is a different type from the recorder
//!
//! [`Sink`] is a **live** emitter: it implements the whole emitter family, so anything that can
//! name one can install it as a parse's context. That is exactly the property the wrong-source
//! class needs — a sink bound to one buffer, driven over another — and it is the property this
//! type removes. `Cst` deliberately implements **no** emitter trait, so the value a driver
//! returns cannot be fed back in as the context of a second parse. The non-implementation is the
//! guarantee; see the *Not an emitter* section on [`Cst`] for the pinned pair of doctests that
//! keeps it honest.

use rowan::GreenNode;

use crate::Lexer;

use super::{
  CstText,
  sink::{FinishError, Sink, TriviaPolicy},
};

/// A finished lossless parse: the spent [`Sink`], and the one door to its green tree.
///
/// Minted by [`parse_lossless`](super::parse_lossless) and
/// [`parse_lossless_partial`](super::parse_lossless_partial) — never by a caller — from the very
/// input the parse ran over. That is what closes the wrong-source class at its root: the buffer
/// the tree's text is sliced out of and the buffer the parse read are the *same argument* of the
/// *same call*, so there is no second name for a caller to get wrong.
///
/// # Not an emitter
///
/// `Cst` implements no emitter trait, and that omission is load bearing. Minting alone would
/// close nothing if the minted value could be re-aimed at a second parse: hand it back as a
/// context and the sink is once again bound to one source while reading another. Because `Cst`
/// cannot occupy an emitter slot, the artefact is inert the instant it is returned.
///
/// The pair below pins it. A [`Sink`] satisfies an emitter bound:
///
/// ```rust
/// use tokora::{Emitter, Lexer, SimpleSpan, Token, cst::Sink, emitter::Verbose};
///
/// fn needs_emitter<'inp, L, T>()
/// where
///   L: Lexer<'inp>,
///   T: Emitter<'inp, L, ()>,
/// {
/// }
///
/// # #[derive(Debug, Clone, Copy)]
/// # struct STok;
/// # impl Token<'_> for STok {
/// #   type Kind = u8;
/// #   type Error = ();
/// #   const SURFACES_TRIVIA: bool = true;
/// #   fn kind(&self) -> u8 { 0 }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # struct Lossless<'a> { src: &'a str, state: () }
/// # impl<'inp> Lexer<'inp> for Lossless<'inp> {
/// #   type State = (); type Source = str; type Token = STok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self { Self { src, state } }
/// #   fn check(&self) -> Result<(), ()> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(0, 0) }
/// #   fn slice(&self) -> &'inp str { "" }
/// #   fn lex(&mut self) -> Option<Result<STok, ()>> { None }
/// #   fn bump(&mut self, _: &usize) {}
/// # }
/// needs_emitter::<Lossless<'_>, Sink<'_, Lossless<'_>, Verbose<()>>>();
/// ```
///
/// The same call with `Cst` in the type position — the only difference — does not compile:
///
/// ```compile_fail
/// use tokora::{Emitter, Lexer, SimpleSpan, Token, cst::Cst, emitter::Verbose};
///
/// fn needs_emitter<'inp, L, T>()
/// where
///   L: Lexer<'inp>,
///   T: Emitter<'inp, L, ()>,
/// {
/// }
///
/// # #[derive(Debug, Clone, Copy)]
/// # struct STok;
/// # impl Token<'_> for STok {
/// #   type Kind = u8;
/// #   type Error = ();
/// #   const SURFACES_TRIVIA: bool = true;
/// #   fn kind(&self) -> u8 { 0 }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # struct Lossless<'a> { src: &'a str, state: () }
/// # impl<'inp> Lexer<'inp> for Lossless<'inp> {
/// #   type State = (); type Source = str; type Token = STok;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'inp str) -> Self { Self { src, state: () } }
/// #   fn with_state(src: &'inp str, state: ()) -> Self { Self { src, state } }
/// #   fn check(&self) -> Result<(), ()> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) -> () { self.state }
/// #   fn source(&self) -> &'inp str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(0, 0) }
/// #   fn slice(&self) -> &'inp str { "" }
/// #   fn lex(&mut self) -> Option<Result<STok, ()>> { None }
/// #   fn bump(&mut self, _: &usize) {}
/// # }
/// needs_emitter::<Lossless<'_>, Cst<'_, Lossless<'_>, Verbose<()>>>();
/// ```
///
/// # No way back to the recorder
///
/// There is no accessor that hands out the wrapped sink — by value, by `&mut`, or at all.
/// [`inner_ref`](Self::inner_ref) reaches past it to the *wrapped* emitter by shared reference,
/// and materialization ([`finish`](Self::finish) / [`finish_partial`](Self::finish_partial))
/// returns that emitter by value along with the tree. Neither yields anything an input can be
/// built around.
pub struct Cst<'inp, L, E>
where
  L: Lexer<'inp>,
{
  sink: Sink<'inp, L, E>,
  /// The parse's descent-trip count, taken off the `Input` the driver was about to drop. See
  /// [`resource_trips`](Cst::resource_trips) for why the handle is the only place it can live.
  resource_trips: usize,
}

impl<'inp, L, E> core::fmt::Debug for Cst<'inp, L, E>
where
  L: Lexer<'inp>,
  E: core::fmt::Debug,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("Cst")
      .field("sink", &self.sink)
      .field("resource_trips", &self.resource_trips)
      .finish()
  }
}

impl<'inp, L, E> Cst<'inp, L, E>
where
  L: Lexer<'inp>,
{
  /// Wraps a spent sink, together with the descent-trip count of the parse that spent it.
  ///
  /// Crate-private on purpose: the drivers are the only minters, and a public constructor here
  /// would re-open the very seam [`Sink::new`] closed — a caller could pair a sink with any parse
  /// and then wrap the result. The second argument is why that matters twice over: it is a fact
  /// about the parse, not about the sink, and a caller who could state it separately could state
  /// a clean count over a tree that tripped.
  ///
  /// `resource_trips` comes from `Input::resource_trips`, read **before** `into_emitter` consumes
  /// the input.
  #[inline]
  pub(crate) const fn from_sink(sink: Sink<'inp, L, E>, resource_trips: usize) -> Self {
    Self {
      sink,
      resource_trips,
    }
  }

  /// How many times this parse exceeded its **recursion budget** — zero for a parse that stayed
  /// inside it.
  ///
  /// # The question this answers, and why nothing else could
  ///
  /// A descent trip reaches the caller on exactly one channel: the driver's `Result`. It is
  /// deliberately **not** an emitter event —
  /// [`RecursionLimitReached`](crate::error::RecursionLimitReached) has no `emit_*` counterpart,
  /// because a recording emitter that could absorb a resource trip would be worse than one that
  /// is hard to observe. The consequence is that a lossless consumer whose product is the tree
  /// plus its diagnostics — the ordinary shape, and the reason
  /// [`parse_lossless`](super::parse_lossless) returns the tree first — sees **nothing**: the
  /// diagnostics are empty, the tree round-trips (`tree.text() == source`) exactly as an untripped
  /// parse's does, and only the discarded `Result` said why it is shaped that way. This is the
  /// other channel, and it survives on the handle rather than in the tuple.
  ///
  /// It cannot be recovered later from anything else. [`InputRef::recursion`](crate::InputRef)
  /// reports the *live* depth, and the trip path releases the frame before building the error, so
  /// the budget reads clean the instant the error exists; the input that held the counter is
  /// dropped by the driver on the same line that mints this handle.
  ///
  /// # Session-absolute, and only sound because the parse is over
  ///
  /// The cell is a **monotone session fact** — it counts up and nothing lowers it, because
  /// nothing can un-exceed a budget. Read absolutely, it answers *"did this parse ever trip"*.
  ///
  /// That is precisely the reading every site **inside** a parse must not take: `recover`,
  /// `inplace_recover`, `skip_then_retry` and the resilient collection loops all snapshot the
  /// counter and ask whether it moved during the one attempt they are judging, because an
  /// absolute reading there would let a single deep construct early in a document re-raise every
  /// later failure and suppress every later diagnostic. The reading is safe *here* for the reason
  /// it is unsafe there: there is no later attempt to poison. The parse is finished.
  ///
  /// A **count** rather than a `bool`, for the same reason the cell is one: it composes. Two
  /// trips are distinguishable from one, which a flag cannot do — and a grammar that catches a
  /// trip itself and parses on is supported, so more than one is reachable.
  ///
  /// # What it does not tell you
  ///
  /// Not *where*: a descent trip has a control stack rather than a position, and latches no
  /// boundary. Not *whether the tree is truncated*: a grammar that caught the trip may have
  /// carried on and produced a complete construct. It is the existence question, which is the one
  /// a consumer holding a silent tree could not previously ask at all.
  ///
  /// # The boundary
  ///
  /// Frames admitted equals the limiter's `limitation` exactly — the `limitation + 1`th descent
  /// is the one refused. For a Pratt-driven grammar the root expression spends the first frame, so
  /// the deepest *nested* construct that parses clean is `limitation - 1`.
  #[inline]
  pub const fn resource_trips(&self) -> usize {
    self.resource_trips
  }

  /// Sets the materialization-time trivia placement policy (builder form).
  ///
  /// It lives here rather than on the driver because it is read exactly once, by the
  /// materialization walk, and never during the parse: nothing a running parser does can
  /// observe it. A driver argument would put a knob that cannot influence the parse in the
  /// parse's own argument list; this door is the one that consumes it.
  #[inline]
  #[must_use]
  pub fn with_trivia_policy(mut self, policy: TriviaPolicy) -> Self {
    self.sink = self.sink.with_trivia_policy(policy);
    self
  }

  /// The configured trivia policy.
  #[inline]
  pub const fn trivia_policy(&self) -> TriviaPolicy {
    self.sink.trivia_policy()
  }

  /// The configured recovery-hole node kind.
  #[inline]
  pub const fn error_kind(&self) -> u16 {
    self.sink.error_kind()
  }

  /// The configured gap-tile token kind.
  #[inline]
  pub const fn gap_kind(&self) -> u16 {
    self.sink.gap_kind()
  }

  /// The emitter the sink wrapped, by shared reference — the collected diagnostics of the parse
  /// that produced this tree, before materialization gives them back by value.
  #[inline]
  pub const fn inner_ref(&self) -> &E {
    self.sink.inner_ref()
  }

  /// Materializes the buffered events into a green tree wrapped in `root_kind`, returning
  /// the inner emitter either way — the handle is consumed exactly once, and the
  /// diagnostics survive the tree.
  ///
  /// The text comes from the source the driver was **called** with (read through
  /// [`CstText`](super::CstText)), so the spans and the bytes they slice cannot come from two
  /// different buffers; a byte-backed source that is not UTF-8 is refused once, here
  /// ([`FinishError::NonUtf8Source`]).
  ///
  /// The replay validates and builds in one walk: balance, close identity
  /// ([`FinishError::MismatchedFinish`]), retro-wrap integrity, kind hygiene (the reserved
  /// band and the root kind, refused unconditionally; every other kind, refused at the
  /// emission doors in every build too, and re-checked here only as a debug-assertions-gated
  /// backstop — [`FinishError::InvalidDialectKind`]), span discipline for tokens and for the
  /// diagnostic
  /// spans that license gaps ([`FinishError::InvalidDiagnosticSpan`]), the token-channel wall
  /// ([`FinishError::StructureWithoutTokens`] — structure with zero committed tokens
  /// over a nonempty source *no lexer error explains* is a severed `commit_token` channel,
  /// not a tree), the
  /// **gap-coverage law** (every uncovered byte tiles as a `gap_kind` token only where a
  /// recorded lexer error explains it; an unexplained gap is a dropped committed token —
  /// [`FinishError::UncoveredGap`]) — so on success `tree.text() == source` holds and
  /// every gap in it is a byte the lexer legitimately refused. On the first violation the
  /// half-built green state is dropped and a typed [`FinishError`] comes back instead;
  /// this method **never panics**.
  ///
  /// One refusal precedes the walk rather than arising from it: a sink that had to degrade a
  /// rewind it could not perform — an unpaired settle detected mid-unwind, where reporting it
  /// would have aborted the process — refuses here
  /// ([`FinishError::UnpairedSettle`]) instead of materializing a log that describes a
  /// rollback that never happened.
  ///
  /// # Abort semantics
  ///
  /// - An `Incomplete` parse (needs more input) should not be materialized through this door:
  ///   it left open nodes and an un-diagnosed tail, which is the pair `finish` refuses and
  ///   [`finish_partial`](Self::finish_partial) tolerates. **The handle is not a
  ///   continuation.** It holds *that attempt's* buffered events, and no operation on it takes
  ///   a larger buffer, resumes the lexer, or writes another event into its sink. To carry on,
  ///   drop it and drive [`parse_lossless_partial`](super::parse_lossless_partial) again over
  ///   the enlarged slice, exactly as that function's own note says — each attempt builds a
  ///   fresh input and a fresh sink and re-lexes the prefix, so cumulative work is
  ///   **Θ(Σ attempt lengths)**.
  /// - A fatal abort leaves open nodes; `finish` refuses them
  ///   ([`FinishError::UnclosedNodes`]) — [`finish_partial`](Self::finish_partial) is
  ///   the explicit opt-in that closes them for tooling.
  ///
  /// ## What an incomplete handle *is* for
  ///
  /// Three things, and they are all readings of one finished attempt rather than steps toward
  /// the next one:
  ///
  /// - [`resource_trips`](Self::resource_trips) — whether that attempt exceeded its recursion
  ///   budget. This handle is the only place the fact survives, and for a partial drive the
  ///   count is **per attempt**: a refill loop that wants the stream's total sums the handles
  ///   it read.
  /// - [`inner_ref`](Self::inner_ref) — the diagnostics the attempt collected, by shared
  ///   reference, before a materialization gives the emitter back by value.
  /// - [`finish_partial`](Self::finish_partial) — a deliberately truncated tree, when one is
  ///   useful to tooling. It is a snapshot of what has been parsed so far, not a prefix that a
  ///   later call extends.
  ///
  /// ## Why there is no resume
  ///
  /// Not an omission this door could quietly grow. A recording sink carried across redrives
  /// corrupts its own event log, which is why [`Sink`] deliberately does **not** implement
  /// [`ValueKeyedEmitter`](crate::emitter::ValueKeyedEmitter) and why
  /// [`PartialSession::parse`](crate::input::PartialSession::parse) — the bounded cross-attempt
  /// owner the ordinary partial parser hands untrusted input to — cannot be paired with one.
  /// Resuming *this* handle is that composition, and the type system refuses it. A supported
  /// bounded CST retry needs its own session type, owning the budget, the terminal latch and a
  /// fresh sink per attempt; that is tracked as al8n/tokora#251 and is not something a
  /// materialization handle can imply by documentation.
  ///
  /// # Where a gap lands
  ///
  /// **A gap is tiled where it opens.** An uncovered run opens the instant the token before it
  /// settles — that is the moment the parse stopped covering the source — so the run is tiled
  /// immediately after that token, in the node that was open then. It is in the tree before the
  /// next event is read, so nothing that happens afterwards can move it.
  ///
  /// One clause covers the run that has no token before it (the source starts with bytes no
  /// token claims): there is no such moment, so it is tiled where the walk first sees it — at
  /// the first committed token, or, if the parse committed **no** token at all, at the end of
  /// the walk in whatever node is open there.
  ///
  /// Two properties follow, and they are the whole point of stating placement this way. Both are
  /// pinned as laws over a corpus rather than as table rows, because a hand-written "the same
  /// stream, one token longer" is exactly the thing three rounds of this rule got wrong.
  ///
  /// - **Nothing that follows a run can move it.** Two streams that share a prefix through the
  ///   token a run trails place that run in the same node — including when one of them simply
  ///   *stops* there. Placement is never decided by whether more input happened to follow, which
  ///   is the asymmetry this rule exists to remove: the bytes after the last committed token of
  ///   a document join that document, exactly as an identical run mid-document does, and
  ///   `Root[Document[Tok] Gap]` is now `Root[Document[Tok Gap]]`.
  /// - **A diagnostic cannot move it either.** Placement reads the token and structure events
  ///   only; a `Diag` is never consulted. That is not tidiness — a prefilled lookahead cache
  ///   *hoists* a lexer-class diagnostic earlier in the event stream (`input_ref`'s
  ///   cache-transparency matrix says so in as many words), so a rule that read a diagnostic's
  ///   position would make the tree a function of how far the caller happened to peek. Coverage
  ///   still consults every diagnostic, through the merged **set** of recorded spans, which is
  ///   order-independent for the same reason.
  ///
  /// The node a run joins **widens over it**: a node spanning `0..11` before a four-byte tail
  /// spans `0..15` after. That is the rule working, not a side effect — the node now contains
  /// those bytes. It applies at every extent, a zero-width node included: a node whose only
  /// content is a zero-width committed token still takes the run that token trails.
  ///
  /// A source with **nothing lexable in it** therefore keeps its tail at the root. There is no
  /// token for the run to trail, so the fallback clause applies and the run tiles where the walk
  /// ends: `Root[Document@0..0, Gap@0..len]`. That is not an exemption carved out for the
  /// degenerate case — it is the same clause that governs a leading run, and it is forced: the
  /// identical parse with one lexable byte appended puts that run at the root too, so any other
  /// answer would re-open the asymmetry rather than close it.
  ///
  /// `tree.text() == source` holds under every placement; it is the *shape* this rule fixes.
  /// [`finish_partial`](Self::finish_partial), which tolerates an unbalanced stream, states in
  /// its own note the one case this rule does not reach.
  ///
  /// # The fail-fast boundary (precise)
  ///
  /// The gap-coverage guarantee holds for a **collecting** inner emitter (the lossless
  /// case): every lexer error is recorded, with its span, before the parse moves on, so
  /// every refused byte is explained and `finish` succeeds. Under a **fail-fast** emitter
  /// ([`Fatal`](crate::emitter::Fatal)) the first lexer error aborts the parse, so the bytes
  /// past it are never lexed and no diagnostic covers them: `finish` then refuses (an
  /// [`UncoveredGap`](FinishError::UncoveredGap), or [`UnclosedNodes`](FinishError::UnclosedNodes)
  /// if the abort left a node open). That is by design — inspect such a partial parse through
  /// [`finish_partial`](Self::finish_partial), which tiles the un-diagnosed tail, or accept no
  /// tree. The guarantee is stated only for the collecting case for exactly this reason.
  #[inline]
  pub fn finish(self, root_kind: u16) -> (Result<GreenNode, FinishError>, E)
  where
    L::Offset: TryInto<u32>,
    L::Source: CstText,
  {
    self.sink.finish(root_kind)
  }

  /// [`finish`](Self::finish), but the two **incompleteness** signals a partial parse leaves
  /// are tolerated instead of refused, so tooling can inspect a fatally-aborted or truncated
  /// parse: open nodes at the end of the stream are **closed** (not
  /// [`UnclosedNodes`](FinishError::UnclosedNodes)), and an uncovered gap is **tiled** (not
  /// [`UncoveredGap`](FinishError::UncoveredGap)) — the un-diagnosed tail of a fail-fast
  /// abort becomes one `gap_kind` run so `tree.text() == source` still holds. Every other law
  /// (balance underflow, close identity, wrap integrity, kind hygiene, span discipline —
  /// diagnostic spans included: a malformed one is corruption, not incompleteness, and is
  /// refused through **both** doors — and the token-channel wall for *balanced* streams, since
  /// the zero-token severance is corruption too) is enforced identically. The exemptions are
  /// the two ways an incomplete parse differs from a complete one; refusing them would defeat
  /// the door.
  ///
  /// A **degraded rewind** ([`FinishError::UnpairedSettle`]) is refused through this door for
  /// the same reason a malformed diagnostic span is: it is corruption, not incompleteness. The
  /// log is not a *partial* record of the parse — it is a complete record of a branch the
  /// parser abandoned and the sink was unable to roll back. Tooling inspecting a fatally
  /// aborted parse is exactly the caller this door exists for, and exactly the caller that must
  /// not be handed that tree.
  ///
  /// # Where a gap lands
  ///
  /// This door places every gap exactly where [`finish`](Self::finish) does, by the rule that
  /// method's own *Where a gap lands* note states in full: a run is tiled where it opens, in the
  /// node open at the token it trails. Nothing about an unbalanced stream changes that — a run
  /// trailing a token inside a node the stream never closed still lands in that node.
  ///
  /// The one case only this door can reach is the run that trails **no** token: the fallback
  /// clause tiles it at the end of the walk, and here that is **before** the open frames are
  /// closed, so it becomes a child of the **innermost open node** rather than of the root.
  /// `finish` never sees it, since an unbalanced stream is refused outright. `tree.text() ==
  /// source` holds either way; it is the *placement* that differs, and tooling that walks by
  /// node rather than by text will see it.
  #[inline]
  pub fn finish_partial(self, root_kind: u16) -> (Result<GreenNode, FinishError>, E)
  where
    L::Offset: TryInto<u32>,
    L::Source: CstText,
  {
    self.sink.finish_partial(root_kind)
  }
}
