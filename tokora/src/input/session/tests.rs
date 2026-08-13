//! Session cells.
//!
//! Every fixture here uses a **by-value** `L::State`, and that is not incidental. The suite's
//! other limiter shares its tally through an `Rc`, which makes "the base state was retained"
//! and "the base state was overwritten by the last attempt's evolved state" *observationally
//! identical* — a payload that cannot discriminate the defect it is supposed to catch. A
//! by-value tally is the only shape in which the harvest rule is measurable at all.

use crate::{
  InputRef, Partial, SimpleSpan, Token,
  cache::DefaultCache,
  emitter::{Fatal, Verbose},
  error::{Incomplete, MaybeIncomplete, MaybeTerminal, token::UnexpectedToken},
  input::{Budget, PartialSession, RedriveFromBase, SessionRefusal, parse_partial},
  lexer::Lexer,
  state::State,
};

// ── A by-value scan tally ──────────────────────────────────────────────────────

/// Trips once more than `limit` words have been lexed. Copied with the state, never shared:
/// two lexers built from the same base start from the same tally, independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tally {
  scanned: usize,
  limit: usize,
}

impl Tally {
  const fn with_limit(limit: usize) -> Self {
    Self { scanned: 0, limit }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tripped;

impl State for Tally {
  type Error = Tripped;

  fn check(&self) -> Result<(), Tripped> {
    if self.scanned > self.limit {
      Err(Tripped)
    } else {
      Ok(())
    }
  }
}

// ── The lexer: `[^ ]+` words, `@` a plain lexer error ──────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WordKind;

impl core::fmt::Display for WordKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Word;

/// The two lexer-error classes the fixtures need kept apart: `@` is a plain error a
/// collecting emitter absorbs, a limiter trip is TERMINAL and latches the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexErr {
  Bad,
  Trip,
}

impl From<Tripped> for LexErr {
  fn from(_: Tripped) -> Self {
    Self::Trip
  }
}

impl Token<'_> for Word {
  type Kind = WordKind;
  type Error = LexErr;

  fn kind(&self) -> WordKind {
    WordKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

struct WordLexer<'inp> {
  src: &'inp str,
  start: usize,
  end: usize,
  state: Tally,
}

impl<'inp> Lexer<'inp> for WordLexer<'inp> {
  type State = Tally;
  type Source = str;
  type Token = Word;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp str) -> Self {
    Self::with_state(src, Tally::with_limit(usize::MAX))
  }

  fn with_state(src: &'inp str, state: Tally) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), LexErr> {
    State::check(&self.state).map_err(LexErr::from)
  }

  fn state(&self) -> &Tally {
    &self.state
  }

  fn state_mut(&mut self) -> &mut Tally {
    &mut self.state
  }

  fn into_state(self) -> Tally {
    self.state
  }

  fn source(&self) -> &'inp str {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }

  fn slice(&self) -> &'inp str {
    &self.src[self.start..self.end]
  }

  fn lex(&mut self) -> Option<Result<Word, LexErr>> {
    let bytes = self.src.as_bytes();
    self.start = self.end;
    while self.start < bytes.len() && bytes[self.start] == b' ' {
      self.start += 1;
    }
    if self.start >= bytes.len() {
      self.end = self.start;
      return None;
    }
    let mut e = self.start;
    while e < bytes.len() && bytes[e] != b' ' {
      e += 1;
    }
    self.end = e;
    if bytes[self.start] == b'@' {
      // A plain (non-terminal) lexer error, so a collecting emitter keeps going.
      return Some(Err(LexErr::Bad));
    }
    self.state.scanned += 1;
    // A trip must be surfaced by the lexer itself: `Lexer::check` is the probe `classify`
    // consults to rank an error as TERMINAL, not a per-token gate the input layer applies on
    // its own, so a lexer that never reports the failure is never ranked as tripped.
    if let Err(trip) = State::check(&self.state) {
      return Some(Err(LexErr::from(trip)));
    }
    Some(Ok(Word))
  }

  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
    self.start = self.end;
  }
}

// ── The consumer error ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum SErr {
  Lex,
  /// A limiter trip: terminal, exactly like a session refusal.
  Trip,
  Incomplete(usize),
  /// The session's own refusals, and the terminal class the coherence law demands.
  Refused(SessionRefusal),
}

impl From<LexErr> for SErr {
  fn from(e: LexErr) -> Self {
    match e {
      LexErr::Bad => Self::Lex,
      LexErr::Trip => Self::Trip,
    }
  }
}

impl From<Incomplete<usize>> for SErr {
  fn from(inc: Incomplete<usize>) -> Self {
    Self::Incomplete(inc.into_offset())
  }
}

impl From<SessionRefusal> for SErr {
  fn from(refusal: SessionRefusal) -> Self {
    Self::Refused(refusal)
  }
}

impl MaybeIncomplete for SErr {
  fn is_incomplete(&self) -> bool {
    matches!(self, Self::Incomplete(_))
  }
}

impl MaybeTerminal for SErr {
  fn is_terminal(&self) -> bool {
    matches!(self, Self::Trip | Self::Refused(_))
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for SErr {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    Self::Lex
  }
}

type Lex<'a> = WordLexer<'a>;
type FatalCtx<'a> = (Fatal<SErr>, DefaultCache<'a, Lex<'a>>);
type SharedCtx<'a, 'e> = (&'e mut Verbose<SErr>, DefaultCache<'a, Lex<'a>>);

fn fatal_ctx<'a>() -> FatalCtx<'a> {
  (Fatal::of(), DefaultCache::<'a, Lex<'a>>::default())
}

/// Counts every word to end of input. Under `Fatal` a lexer error aborts; the frontier
/// `Incomplete` propagates out on `?`.
fn count_words<'inp>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, FatalCtx<'inp>, (), Partial>,
) -> Result<usize, SErr> {
  let mut n = 0;
  while inp.next()?.is_some() {
    n += 1;
  }
  Ok(n)
}

/// The same drain over a **threaded** collector.
fn count_words_shared<'inp>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, SharedCtx<'inp, '_>, (), Partial>,
) -> Result<usize, SErr> {
  let mut n = 0;
  while let Ok(Some(_)) = inp.next() {
    n += 1;
  }
  Ok(n)
}

fn diagnostics(emitter: &Verbose<SErr>) -> usize {
  emitter.errors().values().flatten().count()
}

// ── T-15: the budget bounds cumulative work, and a terminal trip latches ───────

/// The budget refuses **before any work** once Σ attempt-lexable bytes would pass the cap,
/// and the refusal is terminal.
///
/// Falsified by: an `Ok`, a non-terminal converted refusal, a `committed` that moved across
/// the refused attempt (which would prove the gate ran after the work rather than before
/// it), or a `spent` that moved (a refused attempt spends nothing).
#[test]
fn session_budget_bounds_cumulative_work() {
  let mut session = PartialSession::new(
    Tally::with_limit(usize::MAX),
    Budget::Bytes(8),
    RedriveFromBase,
  );

  // 3 lexable bytes: inside the budget.
  assert_eq!(session.parse(fatal_ctx(), "a b", true, count_words), Ok(2));
  assert_eq!(*session.committed(), 3);
  assert_eq!(session.spent(), 3);

  // 3 + 7 = 10 > 8: refused whole, before the first byte is lexed.
  let refused = session.parse(fatal_ctx(), "a b c d", true, count_words);
  assert_eq!(
    refused,
    Err(SErr::Refused(SessionRefusal::BudgetExhausted {
      spent: 10,
      budget: 8
    })),
    "the gate reports the Σ this attempt would have reached"
  );
  assert!(
    refused.as_ref().unwrap_err().is_terminal(),
    "the coherence law: a session refusal must convert to a TERMINAL error, or the caller \
     gets a zero-work infinite refill loop"
  );
  assert_eq!(
    *session.committed(),
    3,
    "a refused attempt does no work, so the frontier cannot have moved"
  );
  assert_eq!(session.spent(), 3, "a refused attempt spends nothing");
}

/// `Budget::Unbounded` never refuses — it is a written declaration, not a silent default.
#[test]
fn unbounded_budget_never_refuses() {
  let mut session = PartialSession::new(
    Tally::with_limit(usize::MAX),
    Budget::Unbounded,
    RedriveFromBase,
  );
  for _ in 0..4 {
    assert!(
      session
        .parse(fatal_ctx(), "a b c d e", true, count_words)
        .is_ok()
    );
  }
  assert_eq!(
    session.spent(),
    36,
    "the spend is still tallied, just never gated"
  );
}

/// A terminal error latches the session sticky: every later attempt is refused before any
/// work, and the refusal is itself terminal.
///
/// The trip is a real one — a one-word limiter over a two-word buffer — not a synthetic
/// `Err`. Falsified by: a second attempt that runs (its `spent` would move), or a
/// `TerminalLatched` that converts to a non-terminal error.
#[test]
fn session_latches_terminal_trips() {
  let mut session = PartialSession::new(Tally::with_limit(1), Budget::Unbounded, RedriveFromBase);

  assert_eq!(
    session.parse(fatal_ctx(), "a b", true, count_words),
    Err(SErr::Trip),
    "the second word passes the limiter, and a trip is terminal by the crate's own law"
  );
  assert!(
    session.is_latched(),
    "an Err-shaped terminal latches the session"
  );

  let retried = session.parse(fatal_ctx(), "a b c", true, count_words);
  assert_eq!(
    retried,
    Err(SErr::Refused(SessionRefusal::TerminalLatched)),
    "the latch refuses the retry before any work"
  );
  assert!(retried.as_ref().unwrap_err().is_terminal());
  assert_eq!(
    session.spent(),
    3,
    "the refused retry spent nothing: only the first attempt's 3 bytes are on the tally"
  );
}

// ── The harvest rule: the base state is retained, never overwritten ────────────

/// Under `RedriveFromBase` the harvest is `(committed, terminal)` and **nothing else**. The
/// evolved `L::State` is not written back, because one field cannot be both the base the
/// next seed needs and whatever the last attempt happened to reach.
///
/// The payload is a by-value limiter of 5 over the chunks `["aa bb", " cc", " dd"]`, whose
/// final attempt is a four-word document. Falsified by: a final result other than `Ok(4)`.
/// Harvesting the state instead makes attempt 2 seed at the tally attempt 1 reached and
/// attempt 3 seed at the tally attempt 2 reached, so the limiter trips inside the final
/// document and the drain comes back short — tokens lost, on a write that has no reader in
/// this mode. The two non-final attempts end on the frontier rules (their last word abuts
/// the buffer end), which is orthogonal and is asserted as such.
#[test]
fn redrive_retains_the_base_state_and_never_harvests_it() {
  let mut session = PartialSession::new(Tally::with_limit(5), Budget::Unbounded, RedriveFromBase);

  let mut buffer = std::string::String::new();
  let mut trail = std::vec::Vec::new();
  let chunks = ["aa bb", " cc", " dd"];
  for (i, chunk) in chunks.iter().enumerate() {
    buffer.push_str(chunk);
    let is_final = i + 1 == chunks.len();
    trail.push(session.parse(fatal_ctx(), buffer.as_str(), is_final, count_words));
  }

  assert!(
    trail[0].as_ref().is_err_and(MaybeIncomplete::is_incomplete)
      && trail[1].as_ref().is_err_and(MaybeIncomplete::is_incomplete),
    "the two non-final attempts stop on the frontier, not on the limiter: {trail:?}"
  );
  assert_eq!(
    trail[2],
    Ok(4),
    "the final attempt re-drives all four words from the RETAINED base tally; seeding from \
     a harvested state trips the limiter of 5 mid-document instead"
  );
  assert_eq!(*session.committed(), 11);
}

/// The same rule, read through the limiter directly: a by-value tally accumulates **within**
/// an attempt (so the fixture can discriminate at all) and resets **between** them.
///
/// Falsified by: an attempt that does not truncate (which would mean the tally never
/// accumulates, and every cell above is vacuous), or a second attempt that truncates earlier
/// than the first over the same prefix (which would mean the base was overwritten).
#[test]
fn the_by_value_tally_accumulates_within_an_attempt_and_resets_between_them() {
  let mut session = PartialSession::new(Tally::with_limit(3), Budget::Unbounded, RedriveFromBase);

  // Three words against a limit of three: exactly at the edge, so it does not trip.
  let first = session.parse(fatal_ctx(), "a b c", true, count_words);
  assert_eq!(first, Ok(3), "three words sit exactly on the limit");

  // The identical buffer, again. A retained base gives the identical answer; a harvested
  // base would start above the limit and truncate at the first word.
  let second = session.parse(fatal_ctx(), "a b c", true, count_words);
  assert_eq!(
    second,
    Ok(3),
    "the base tally was not overwritten by attempt one; seeding from the harvested tally \
     of three would trip on this attempt's very first word"
  );
}

// ── T-15b(a)/(c): cross-attempt diagnostic completeness ────────────────────────

/// **T-15b(a).** With the ordinary by-value ctx — a fresh collector per attempt — the
/// **final** attempt's emitter is the complete story, because a from-base attempt re-derives
/// every diagnostic from byte 0.
///
/// Falsified by: a final count that differs from the one-shot parse's count.
#[test]
fn session_diagnostics_are_complete_across_attempts() {
  let chunks = ["a @ b", " c", " d"];

  // The one-shot reference: one attempt over the whole buffer.
  let mut one = Verbose::<SErr>::new();
  {
    let ctx: SharedCtx<'_, '_> = (&mut one, DefaultCache::<'_, Lex<'_>>::default());
    let _ = parse_partial(
      ctx,
      "a @ b c d",
      Tally::with_limit(usize::MAX),
      true,
      count_words_shared,
    );
  }
  let one_shot = diagnostics(&one);
  assert!(
    one_shot >= 1,
    "the payload must emit, or the cell cannot discriminate"
  );

  // The session, with a FRESH collector per attempt — the ordinary by-value ctx.
  let mut session = PartialSession::new(
    Tally::with_limit(usize::MAX),
    Budget::Unbounded,
    RedriveFromBase,
  );
  let mut buffer = std::string::String::new();
  let mut final_count = 0;
  for (i, chunk) in chunks.iter().enumerate() {
    buffer.push_str(chunk);
    let is_final = i + 1 == chunks.len();
    let mut emitter = Verbose::<SErr>::new();
    let ctx: SharedCtx<'_, '_> = (&mut emitter, DefaultCache::<'_, Lex<'_>>::default());
    let _ = session.parse(ctx, buffer.as_str(), is_final, count_words_shared);
    final_count = diagnostics(&emitter);
  }

  assert_eq!(
    final_count, one_shot,
    "the last attempt re-derived every diagnostic from base, so it IS the complete set"
  );
}

/// **T-15b(c).** Threading ONE collector through the by-value ctx is callable, and it
/// **duplicates** the prefix's diagnostics once per attempt: complete-but-noisy, never
/// lossy. Disclosed and pinned rather than claimed impossible.
///
/// The payload puts `@` mid-buffer in the FIRST attempt, so duplication and exactly-once
/// give different numbers. Falsified by: a final count equal to the one-shot count (which is
/// what a payload whose first attempt emits nothing would produce, and why such a payload
/// cannot discriminate this claim).
#[test]
fn threaded_collector_duplicates_rather_than_loses() {
  let chunks = ["a @ b", " c", " d"];

  let mut shared = Verbose::<SErr>::new();
  let mut session = PartialSession::new(
    Tally::with_limit(usize::MAX),
    Budget::Unbounded,
    RedriveFromBase,
  );
  let mut buffer = std::string::String::new();
  let mut trail = std::vec::Vec::new();
  for (i, chunk) in chunks.iter().enumerate() {
    buffer.push_str(chunk);
    let is_final = i + 1 == chunks.len();
    let ctx: SharedCtx<'_, '_> = (&mut shared, DefaultCache::<'_, Lex<'_>>::default());
    let _ = session.parse(ctx, buffer.as_str(), is_final, count_words_shared);
    trail.push(diagnostics(&shared));
  }

  assert_eq!(
    trail,
    std::vec![1, 2, 3],
    "one source error, re-derived once per re-drive: cumulative, never lost"
  );
  assert!(
    trail[trail.len() - 1] > 1,
    "duplication is the disclosed cost of threading; exactly-once would read 1"
  );
}

// ── T-17: the no-session path keeps its documented Θ ───────────────────────────

/// `parse_partial` re-drives the **whole prefix** every attempt. Pinned as documented
/// behavior, not as an open defect: the docs now state the Θ and the session exists.
///
/// The instrument needs no counter. Hold the limiter at **one** word while the prefix grows:
/// from the second attempt on the drain is cut to two words, because the attempt re-lexed
/// the whole prefix and the tally passed the limit on its second token. Falsified by: an
/// attempt yielding all `k` words, which is exactly what a resuming driver would produce —
/// it would lex one new word, leave the tally at one, and never trip.
#[test]
fn parse_partial_replays_the_whole_prefix_quadratically() {
  let mut buffer = std::string::String::new();
  let mut yielded = std::vec::Vec::new();
  for k in 1..=4usize {
    let _ = k;
    if k > 1 {
      buffer.push(' ');
    }
    buffer.push('a');

    let outcome = parse_partial(
      fatal_ctx(),
      buffer.as_str(),
      Tally::with_limit(1),
      true,
      count_words,
    );
    yielded.push(outcome);
  }

  assert_eq!(
    yielded,
    std::vec![Ok(1), Err(SErr::Trip), Err(SErr::Trip), Err(SErr::Trip)],
    "every attempt re-lexes from byte zero, so a one-word limiter trips on every attempt \
     past the first no matter how much of the prefix was already parsed last round — Sigma \
     attempt lengths, the honest triangular figure the docs now state"
  );
}

// ── The coherence law must hold in the builds that matter ──────────────────────

/// A consumer whose `From<SessionRefusal>` **lies**: it reports the refusal as incomplete.
/// This is the impl the coherence law forbids, and the one that produces the zero-work
/// infinite refill loop the budget exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LyingErr {
  Lex,
  Incomplete(usize),
  Refused,
}

impl From<LexErr> for LyingErr {
  fn from(_: LexErr) -> Self {
    Self::Lex
  }
}

impl From<Incomplete<usize>> for LyingErr {
  fn from(inc: Incomplete<usize>) -> Self {
    Self::Incomplete(inc.into_offset())
  }
}

impl From<SessionRefusal> for LyingErr {
  fn from(_: SessionRefusal) -> Self {
    Self::Refused
  }
}

impl MaybeIncomplete for LyingErr {
  fn is_incomplete(&self) -> bool {
    // THE LIE: a refusal reports itself as "more input may fix this".
    matches!(self, Self::Incomplete(_) | Self::Refused)
  }
}

impl MaybeTerminal for LyingErr {
  fn is_terminal(&self) -> bool {
    false
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for LyingErr {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    Self::Lex
  }
}

type LyingCtx<'a> = (Fatal<LyingErr>, DefaultCache<'a, Lex<'a>>);

fn lying_ctx<'a>() -> LyingCtx<'a> {
  (Fatal::of(), DefaultCache::<'a, Lex<'a>>::default())
}

fn count_lying<'inp>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, LyingCtx<'inp>, (), Partial>,
) -> Result<usize, LyingErr> {
  let mut n = 0;
  while inp.next()?.is_some() {
    n += 1;
  }
  Ok(n)
}

/// A `From<SessionRefusal>` impl that reports the refusal as incomplete must be refused
/// **loudly, in every build** — not just where `debug_assertions` happen to be on.
///
/// The caller's refill loop keys on `is_incomplete()`. A refusal that lies about it gives a
/// zero-work infinite loop: refill, refuse before any work, identical error, forever — the
/// exact denial of service the budget exists to prevent, and it would be absent from precisely
/// the builds that face untrusted input.
///
/// Falsified by: no panic. Run this under `cargo test --release` as well as in debug — a cell
/// that only fires under `debug_assertions` cannot pin a release-build defect.
#[test]
#[should_panic(expected = "must yield a terminal error")]
fn a_lying_refusal_conversion_is_refused_in_every_build() {
  let mut session = PartialSession::new(
    Tally::with_limit(usize::MAX),
    Budget::Bytes(1),
    RedriveFromBase,
  );
  // 3 lexable bytes against a budget of 1: the gate refuses before any work, and the refusal
  // is converted through the lying impl.
  let _ = session.parse(lying_ctx(), "a b", true, count_lying);
}

/// The session's emitter bound must keep admitting every diagnostics-only emitter, including
/// the borrowed forms a threaded collector uses. This is the positive control for the bound
/// that excludes a recording `Sink`: without it, tightening the session's emitter requirement
/// could silently take streaming diagnostics with it and no cell would notice.
///
/// Falsified by: a compile error here — which is what a bound too tight to admit `Fatal`,
/// `Verbose`, `Silent`, or a `&mut` of any of them would produce.
const _: fn() = || {
  fn admits<E: crate::emitter::ValueKeyedEmitter>() {}
  admits::<Fatal<SErr>>();
  admits::<Verbose<SErr>>();
  admits::<crate::emitter::Silent<SErr>>();
  admits::<&mut Fatal<SErr>>();
  admits::<&mut Verbose<SErr>>();
};
