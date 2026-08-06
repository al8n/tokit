//! The synthetic, fully scriptable fixtures the fuzz harness drives: a byte-per-token [`Lexer`]
//! whose entire token/error stream is a pure function of its source bytes, an emission-counting
//! [`Emitter`], and the pure byte classifiers that form the harness's **shadow model**.
//!
//! # Why a byte-per-token lexer
//!
//! Every fuzz case controls the token stream by choosing the lexer's source bytes. [`ScriptLexer`]
//! emits exactly one token (or one lexer error) per source byte, spanning `[i, i+1)`. Two
//! consequences make it the ideal driver:
//!
//! - **Trivially resume-faithful.** The item at offset `i` depends only on `src[i]`, never on the
//!   lexer state or on look-ahead, so [`with_state`](Lexer::with_state) + [`bump`](Lexer::bump)
//!   reproduces any suffix byte-for-byte — the prefix-replay assumption the input machinery relies
//!   on after a checkpoint restore holds by construction (it is exactly the conformance kit's
//!   per-character fixture, over `[u8]`).
//! - **A closed-form shadow model.** Because the stream is a pure function of the bytes, the
//!   harness can predict every operation's outcome from the source alone with [`is_err`] and
//!   [`kind_of`] — no parallel drive of the machinery under test.

use crate::{
  Lexer, SimpleSpan, State, Token,
  cache::DefaultCache,
  emitter::Emitter,
  error::{Incomplete, MaybeIncomplete, token::UnexpectedTokenOf},
  input::Cursor,
  span::Spanned,
};

// ── The shadow model: pure byte classifiers ─────────────────────────────────────────────────────

/// Byte values `0xE0..=0xFF` lex as a **lexer error** (a token-less item the input layer reports
/// through the emitter and skips). Every other byte lexes as a token. This is the single rule both
/// [`ScriptLexer`] and the shadow model consult, so they never disagree.
#[inline]
pub(crate) const fn is_err(b: u8) -> bool {
  b >= 0xE0
}

/// The token kind a non-error byte lexes to. Delimiters and separators get their own kinds so the
/// harness can drive `sync_balanced` (nesting) and `try_expect` (kind matching) meaningfully; every
/// other byte is a plain [`FuzzKind::Word`].
#[inline]
pub(crate) const fn kind_of(b: u8) -> FuzzKind {
  match b {
    b'(' | b'[' | b'{' => FuzzKind::Open,
    b')' | b']' | b'}' => FuzzKind::Close,
    b';' | b',' => FuzzKind::Semi,
    _ => FuzzKind::Word,
  }
}

// ── Token / kind / error ────────────────────────────────────────────────────────────────────────

/// The kind discriminant of a [`FuzzTok`]. `Open`/`Close` are the balanced delimiters
/// `sync_balanced` nests over; `Semi` is a natural sync target; `Word` is everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FuzzKind {
  /// An opening delimiter (`(`, `[`, `{`).
  Open,
  /// A closing delimiter (`)`, `]`, `}`).
  Close,
  /// A separator / sync target (`;`, `,`).
  Semi,
  /// Any other byte.
  Word,
}

impl core::fmt::Display for FuzzKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      FuzzKind::Open => "open",
      FuzzKind::Close => "close",
      FuzzKind::Semi => "semi",
      FuzzKind::Word => "word",
    })
  }
}

/// The token [`ScriptLexer`] produces: just its kind (span is threaded separately by the machinery).
#[derive(Debug, Clone, Copy)]
pub struct FuzzTok {
  kind: FuzzKind,
}

/// The token's lexer-error payload — a unit; the harness never inspects it, only that an error was
/// produced at a given span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuzzTokError;

impl core::fmt::Display for FuzzTokError {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("lex error")
  }
}

impl Token<'_> for FuzzTok {
  type Kind = FuzzKind;
  type Error = FuzzTokError;

  // honest: byte-per-token, never skips a byte
  const SURFACES_TRIVIA: bool = true;

  #[inline]
  fn kind(&self) -> FuzzKind {
    self.kind
  }

  #[inline]
  fn is_trivia(&self) -> bool {
    false
  }
}

// ── Lexer state ──────────────────────────────────────────────────────────────────────────────────

/// [`ScriptLexer`]'s state: an observable `tag` that plays no part in lexing (the byte-per-token
/// stream ignores it) but rides every checkpoint, so the session-point driver can re-key it through
/// [`InputRef::state_mut`](crate::InputRef::state_mut) and watch a rollback restore it — plus an
/// optional **token limiter**, the terminal condition.
///
/// # The limiter is off by default, so the shadow model stays closed-form
///
/// [`limit`](Self::limit) defaults to `usize::MAX`: [`check`](State::check) then never fails, no
/// poison boundary is ever latched, and the token/error stream stays the pure function of the source
/// bytes ([`is_err`] / [`kind_of`]) that every other driver's model assumes. Only the partial
/// driver's limit oracle constructs a tripping state, and it does so over its own inputs.
///
/// The tally rides *inside the state* on purpose: that is where a real limiter's tally lives (the
/// [`Lexer`] contract says so), so it is cloned into every cached token and copied back by every
/// restore — the fuzzed backtracking exercises the limiter's interaction with checkpoints for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptState {
  /// An opaque marker the harness sets to observe state save/restore. Lexing never reads it.
  pub tag: u64,
  /// Tokens lexed so far under this state.
  scanned: usize,
  /// The token budget: `check()` fails once `scanned` exceeds it. `usize::MAX` never trips.
  limit: usize,
}

impl Default for ScriptState {
  #[inline]
  fn default() -> Self {
    Self {
      tag: 0,
      scanned: 0,
      limit: usize::MAX,
    }
  }
}

impl ScriptState {
  /// A limit-free state carrying `tag` — what the session driver re-keys with.
  #[inline]
  pub const fn with_tag(tag: u64) -> Self {
    Self {
      tag,
      scanned: 0,
      limit: usize::MAX,
    }
  }

  /// A state that trips once more than `limit` tokens have been lexed: the `(limit + 1)`-th token is
  /// the **tripping token**, and the lexer reports it as an error carrying that token's span —
  /// exactly what the Logos backend does when `check()` fails after a token.
  #[inline]
  pub const fn with_limit(limit: usize) -> Self {
    Self {
      tag: 0,
      scanned: 0,
      limit,
    }
  }

  /// Whether the budget is already spent — the lexer's sticky latch (see [`ScriptLexer::lex`]).
  #[inline]
  const fn tripped(&self) -> bool {
    self.scanned > self.limit
  }
}

impl State for ScriptState {
  type Error = FuzzTokError;

  #[inline]
  fn check(&self) -> Result<(), Self::Error> {
    // The terminal predicate. Limit-free by default (see the type docs), so the committed-stream
    // shadow model stays exact for every driver that does not opt in.
    if self.tripped() {
      Err(FuzzTokError)
    } else {
      Ok(())
    }
  }
}

// ── The scriptable lexer ─────────────────────────────────────────────────────────────────────────

/// A byte-per-token lexer over `[u8]`: source byte `i` lexes to one item spanning `[i, i+1)` — a
/// [`FuzzTokError`] if [`is_err`], else a [`FuzzTok`] of [`kind_of`]. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct ScriptLexer<'a> {
  src: &'a [u8],
  start: usize,
  end: usize,
  state: ScriptState,
}

impl<'a> Lexer<'a> for ScriptLexer<'a> {
  type State = ScriptState;
  type Source = [u8];
  type Token = FuzzTok;
  type Span = SimpleSpan;
  type Offset = usize;

  #[inline]
  fn new(src: &'a [u8]) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: ScriptState::default(),
    }
  }

  #[inline]
  fn with_state(src: &'a [u8], state: ScriptState) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  #[inline]
  fn check(&self) -> Result<(), FuzzTokError> {
    self.state.check()
  }

  #[inline]
  fn state(&self) -> &ScriptState {
    &self.state
  }

  #[inline]
  fn state_mut(&mut self) -> &mut ScriptState {
    &mut self.state
  }

  #[inline]
  fn into_state(self) -> ScriptState {
    self.state
  }

  #[inline]
  fn source(&self) -> &'a [u8] {
    self.src
  }

  #[inline]
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }

  #[inline]
  fn slice(&self) -> &'a [u8] {
    &self.src[self.start..self.end]
  }

  #[inline]
  fn lex(&mut self) -> Option<Result<FuzzTok, FuzzTokError>> {
    // Sticky trip: once the budget is spent this instance reports EOF forever, mirroring the Logos
    // backend's `poisoned` latch. The latch dies with the lexer — the input layer's poison boundary
    // is what persists it across the fresh lexers it builds per operation.
    if self.state.tripped() {
      return None;
    }
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = self.start + 1;
    let b = self.src[self.start];
    if is_err(b) {
      return Some(Err(FuzzTokError));
    }
    // A token: bill it, then probe. A trip REPLACES the token with a lexer error carrying that
    // token's span — the exact shape the Logos backend produces, and the shape whose span can land
    // on a chunk boundary.
    self.state.scanned += 1;
    if self.state.check().is_err() {
      return Some(Err(FuzzTokError));
    }
    Some(Ok(FuzzTok { kind: kind_of(b) }))
  }

  #[inline]
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

// ── The emitter's error type ─────────────────────────────────────────────────────────────────────

/// The parse-error type the harness's emitter carries. It is only ever *constructed* on the
/// partial-input frontier (from an [`Incomplete`]); the complete-mode drivers keep the emitter
/// non-fatal, so `next` and friends there always return `Ok`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FuzzError {
  /// The partial-input frontier surfaced [`Incomplete`] (more input may still arrive).
  Incomplete,
  /// Any recorded diagnostic (unused as a value; the harness only counts emissions).
  Diagnostic,
}

impl From<Incomplete<usize>> for FuzzError {
  #[inline]
  fn from(_: Incomplete<usize>) -> Self {
    FuzzError::Incomplete
  }
}

impl MaybeIncomplete for FuzzError {
  #[inline]
  fn is_incomplete(&self) -> bool {
    matches!(self, FuzzError::Incomplete)
  }
}

// ── The emission-counting emitter ────────────────────────────────────────────────────────────────

/// An [`Emitter`] that records nothing but a **monotone emission count**, mirroring
/// [`Verbose`](crate::emitter::Verbose)'s `log.len()` semantics exactly: every emission (any
/// channel) bumps the count, [`checkpoint`](Emitter::checkpoint) reads it, and
/// [`rewind`](Emitter::rewind) truncates it back to a mark. It is the minimal faithful mirror of
/// the emission timeline — the machinery under test is the input layer's `checkpoint`/`rewind`
/// *calls*, which this emitter merely reflects, so a failed op that forgets to unwind its emissions
/// leaves the count high and the no-trace oracle fires.
#[derive(Debug, Clone, Default)]
pub struct CountEmitter {
  count: u64,
}

impl CountEmitter {
  /// A fresh emitter with an empty log.
  #[inline]
  pub const fn new() -> Self {
    Self { count: 0 }
  }

  /// The current emission count — the observable the no-trace and LIFO oracles snapshot.
  #[inline]
  pub const fn count(&self) -> u64 {
    self.count
  }
}

impl<'a, L, Lang: ?Sized> Emitter<'a, L, Lang> for CountEmitter
where
  L: Lexer<'a>,
{
  type Error = FuzzError;

  #[inline]
  fn emit_lexer_error(
    &mut self,
    _err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), FuzzError> {
    self.count += 1;
    Ok(())
  }

  #[inline]
  fn emit_unexpected_token(
    &mut self,
    _err: UnexpectedTokenOf<'a, L, Lang>,
  ) -> Result<(), FuzzError> {
    self.count += 1;
    Ok(())
  }

  #[inline]
  fn emit_error(&mut self, _err: Spanned<FuzzError, L::Span>) -> Result<(), FuzzError> {
    self.count += 1;
    Ok(())
  }

  #[inline]
  fn emit_warning(&mut self, _warning: Spanned<FuzzError, L::Span>) -> Result<(), FuzzError> {
    self.count += 1;
    Ok(())
  }

  #[inline]
  fn emit_skipped_region(&mut self, _span: L::Span, _skipped: usize) -> Result<(), FuzzError> {
    self.count += 1;
    Ok(())
  }

  #[inline]
  fn checkpoint(&self) -> u64 {
    self.count
  }

  #[inline]
  fn rewind(&mut self, _cursor: &Cursor<'a, '_, L>, checkpoint: u64) {
    // Faithful mirror of Verbose: drop every emission after the mark by truncating the count.
    self.count = checkpoint;
  }
}

/// The emission count is a reading of the emission state and nothing else — a monotone
/// mirror of `Verbose`'s log length — so `CountEmitter` may sit inside the recording sink.
impl crate::emitter::ValueKeyedEmitter for CountEmitter {}

/// The event channel, defaulted: `CountEmitter` records no tree, so the CST fuzz ops run
/// their no-op halves here (inert marks, ignored wraps) and the harness's `node`-bearing
/// scripts satisfy the `Ctx::Emitter: CstEmitter` bound. The recording twin — the same
/// ops over a real `Sink` with the tree-equality oracle — is the `rowan`-gated
/// `fuzz::cst` case kind.
///
/// One member is not defaulted and so is written out: `cst_demote`, the node bracket's failing
/// exit, is discarded here because there is no event channel to un-open — the deliberate
/// counterpart of a wrapper, which must forward it (see `CstEmitter::cst_demote`).
impl<'a, L, Lang: ?Sized> crate::emitter::CstEmitter<'a, L, Lang> for CountEmitter
where
  L: Lexer<'a>,
{
  #[inline(always)]
  fn cst_demote(&mut self, mark: crate::cst::event::EventMark, kind: u16) {
    let _ = (mark, kind);
  }
}

// ── Context wiring ───────────────────────────────────────────────────────────────────────────────

/// The parse context the harness drives the input under: the [`CountEmitter`] over the default
/// cache. Mirrors the conformance kit's `ConfCtx`, but with a *recording* emitter so the emission
/// oracles have something to observe.
pub(crate) type FuzzCtx<'a> = (CountEmitter, DefaultCache<'a, ScriptLexer<'a>>);

/// A default cache for the fuzz lexer.
#[inline]
pub(crate) fn cache<'a>() -> DefaultCache<'a, ScriptLexer<'a>> {
  DefaultCache::<'a, ScriptLexer<'a>>::default()
}

/// The lexer's initial state over `src` (position is threaded by the input, not the state).
/// Limit-free: only the partial driver's limit oracle asks for a tripping state
/// ([`ScriptState::with_limit`]).
#[inline]
pub(crate) fn initial_state(src: &[u8]) -> ScriptState {
  ScriptLexer::new(src).into_state()
}
