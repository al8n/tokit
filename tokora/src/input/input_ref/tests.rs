//! Input-level tests for `InputRef` scanning entry points.

use core::cell::Cell;
use std::rc::Rc;

use crate::{
  Token,
  cache::DefaultCache,
  emitter::{Silent, Verbose},
  error::{UnexpectedEot, token::UnexpectedToken},
  input::Input,
  lexer::LogosLexer,
  state::State,
};

// ── A limiter whose scan counter is SHARED across every cloned lexer ──────────
//
// `InputRef` builds a fresh lexer per operation by cloning the state, so a
// plain by-value counter (like `TokenLimiter`) hides re-scans: the temporary
// lexer's increments are discarded with it. This limiter shares its counter
// through an `Rc<Cell<_>>`, so every token a temporary lexer scans is
// observable — a frozen count across calls proves the input latched and stopped
// rebuilding lexers.

#[derive(Debug, Clone, Default)]
struct ProbeLimiter {
  /// Total tokens ever scanned, shared by every clone of this state.
  scanned: Rc<Cell<usize>>,
  limit: usize,
}

impl ProbeLimiter {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: Rc::new(Cell::new(0)),
      limit,
    }
  }

  /// A shared handle to observe the scan counter after moving the state in.
  fn counter(&self) -> Rc<Cell<usize>> {
    self.scanned.clone()
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }
}

#[derive(Debug, Clone, PartialEq)]
struct ProbeLimitExceeded;

impl State for ProbeLimiter {
  type Error = ProbeLimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.scanned.get() > self.limit {
      Err(ProbeLimitExceeded)
    } else {
      Ok(())
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
enum ProbeErr {
  Lex,
  Limit,
  /// The end-of-input error `try_expect_or_stop` surfaces on a terminal stop.
  Eot,
}

impl From<()> for ProbeErr {
  fn from(_: ()) -> Self {
    ProbeErr::Lex
  }
}

impl From<ProbeLimitExceeded> for ProbeErr {
  fn from(_: ProbeLimitExceeded) -> Self {
    ProbeErr::Limit
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for ProbeErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    ProbeErr::Eot
  }
}

// Lets `ProbeErr` back a `Verbose` emitter (via the blanket `FromEmitterError`).
// The unexpected-token path is never exercised by these tests; only the plain
// lexer-error path matters.
impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for ProbeErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ProbeErr::Lex
  }
}

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = ProbeLimiter, skip r"[ \t\r\n]+")]
enum ProbeTok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); })]
  Num,
}

impl core::fmt::Display for ProbeTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ProbeKind {
  Num,
}

impl core::fmt::Display for ProbeKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl Token<'_> for ProbeTok {
  type Kind = ProbeKind;
  type Error = ProbeErr;

  fn kind(&self) -> ProbeKind {
    ProbeKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type ProbeLexer<'a> = LogosLexer<'a, ProbeTok>;
type ProbeCtx<'a> = (Silent<ProbeErr>, DefaultCache<'a, ProbeLexer<'a>>);
type ProbeVerboseCtx<'a> = (Verbose<ProbeErr>, DefaultCache<'a, ProbeLexer<'a>>);
/// A capacity-1 cache (`Option`) over the probe lexer, for the abandoned-lineage
/// truncation on the smallest non-trivial cache.
type ProbeOptionCache<'a> = Option<crate::cache::CachedTokenOf<'a, ProbeLexer<'a>>>;
type ProbeOptionVerboseCtx<'a> = (Verbose<ProbeErr>, ProbeOptionCache<'a>);

/// Builds an input over `src` behind a limit-2 [`ProbeLimiter`], returning the
/// input and a shared handle to observe its scan counter. The third scanned
/// token trips the limiter.
fn probe_input(src: &str) -> (Input<'_, ProbeLexer<'_>, ProbeCtx<'_>, ()>, Rc<Cell<usize>>) {
  let limiter = ProbeLimiter::with_limit(2);
  let scanned = limiter.counter();
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(src, limiter, cache);
  (input, scanned)
}

#[test]
fn poisoned_input_latches_no_rescan_across_next_and_peek() {
  // Limit of 2: the third scanned token trips the limiter. A recovering
  // (`Silent`) emitter keeps going, so the trip surfaces as a bounded stop.
  let limiter = ProbeLimiter::with_limit(2);
  let scanned = limiter.counter();

  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache("1 2 3 4 5 6", limiter, cache);
  let mut inp = input.as_ref(&mut emitter);

  // Drive `next()` past the trip.
  assert!(inp.next().unwrap().is_some(), "first token");
  assert!(inp.next().unwrap().is_some(), "second token");
  // The third `next()` trips the limiter; the recovering emitter turns it into a
  // bounded stop (`None`) and latches the input.
  assert!(inp.next().unwrap().is_none(), "trip latches to None");

  let frozen = scanned.get();
  assert_eq!(frozen, 3, "scanned exactly 1, 2, 3 before latching");

  // (a) No further scanning work: repeated `next()`/`peek()` must NOT rebuild a
  // lexer or rescan the tripping token, so the shared counter stays frozen; and
  // (b) returns stay None/empty.
  for _ in 0..5 {
    assert!(inp.next().unwrap().is_none(), "poisoned next() stays None");
  }
  for _ in 0..5 {
    assert!(
      inp.peek_one().unwrap().is_none(),
      "poisoned peek() stays empty"
    );
  }

  assert_eq!(
    scanned.get(),
    frozen,
    "no lexer was rebuilt after the latch — the token counter is frozen"
  );
}

#[test]
fn poisoned_input_latches_no_rescan_across_try_expect() {
  // `try_expect(|_| true)` consumes one token per call; the third rebuilds a
  // lexer that scans the tripping token and latches.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(inp.try_expect(|_| true).unwrap().is_some(), "first token");
  assert!(inp.try_expect(|_| true).unwrap().is_some(), "second token");
  // The third scan trips the limiter; the recovering emitter turns it into a
  // bounded `None` and latches the input.
  assert!(
    inp.try_expect(|_| true).unwrap().is_none(),
    "trip latches to None"
  );

  let frozen = scanned.get();
  assert_eq!(frozen, 3, "scanned exactly 1, 2, 3 before latching");

  // Repeated calls must NOT rebuild a lexer or rescan the tripping token.
  for _ in 0..5 {
    assert!(
      inp.try_expect(|_| true).unwrap().is_none(),
      "poisoned try_expect stays None"
    );
  }
  assert_eq!(
    scanned.get(),
    frozen,
    "no lexer was rebuilt after the latch — the token counter is frozen"
  );
}

// ── `try_expect_or_stop`: the attempt primitive that surfaces terminal stops ──
//
// `try_expect` folds a terminal stop (a fresh limit trip under a recovering
// emitter, or an already-latched poison boundary) into the same `Ok(None)` as a
// legitimate decline — pinned above as intended input-layer behavior. The
// `or_stop` twin keeps `Ok(None)` for definite absence only (non-matching token,
// genuine end of input) and surfaces a terminal stop as the committed forms'
// end-of-input error instead.

#[test]
fn try_expect_or_stop_errs_on_fresh_trip() {
  // The first two calls consume `1` and `2`; the third rebuilds a lexer whose
  // scan trips the limiter — a terminal stop, not evidence of absence.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(
    inp.try_expect_or_stop(|_| true).unwrap().is_some(),
    "first token"
  );
  assert!(
    inp.try_expect_or_stop(|_| true).unwrap().is_some(),
    "second token"
  );
  assert!(
    matches!(inp.try_expect_or_stop(|_| true), Err(ProbeErr::Eot)),
    "a fresh trip surfaces the committed forms' end-of-input error, never a decline"
  );
  assert_eq!(scanned.get(), 3, "scanned exactly 1, 2, and the tripping 3");
}

#[test]
fn try_expect_or_stop_errs_on_latched_boundary_without_rescan() {
  // Latch via `next()`: the third scan trips and latches the poison boundary.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(inp.next().unwrap().is_some(), "first token");
  assert!(inp.next().unwrap().is_some(), "second token");
  assert!(inp.next().unwrap().is_none(), "trip latches to None");

  let frozen = scanned.get();
  assert_eq!(frozen, 3, "scanned exactly 1, 2, 3 before latching");

  // Every attempt at the latched boundary errs — and never rebuilds a lexer.
  for _ in 0..5 {
    assert!(
      matches!(inp.try_expect_or_stop(|_| true), Err(ProbeErr::Eot)),
      "a latched boundary surfaces the end-of-input error on every attempt"
    );
  }
  assert_eq!(
    scanned.get(),
    frozen,
    "no lexer was rebuilt after the latch — the token counter is frozen"
  );
}

#[test]
fn at_latched_boundary_witnesses_a_trip_at_the_cursor() {
  // The input-state witness the resilient collection loops re-raise on: a trip latches the poison
  // boundary at the cursor it scans from, so `at_latched_boundary` reports a terminal element
  // failure without inspecting the error type (no `MaybeTerminal` bound on the driver).
  let (mut input, _scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(!inp.at_latched_boundary(), "no trip before any scan");
  assert!(inp.next().unwrap().is_some(), "first token under the limit");
  assert!(
    inp.next().unwrap().is_some(),
    "second token under the limit"
  );
  assert!(
    !inp.at_latched_boundary(),
    "still no trip while under the limit"
  );
  assert!(
    inp.next().unwrap().is_none(),
    "the third scan trips and latches"
  );
  assert!(
    inp.at_latched_boundary(),
    "the trip is witnessed at the cursor"
  );
}

#[test]
fn try_expect_or_stop_declines_on_non_matching_token() {
  use crate::span::SimpleSpan;

  // A failing predicate is definite absence: `Ok(None)`, with the scanned token
  // put back at the cache front for the next consume.
  let (mut input, scanned) = probe_input("1 2");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(
    inp.try_expect_or_stop(|_| false).unwrap().is_none(),
    "a non-matching token declines"
  );
  assert_eq!(scanned.get(), 1, "one scan staged the declined token");

  let tok = inp
    .next()
    .unwrap()
    .expect("the declined token is still next");
  assert_eq!(
    *tok.span_ref(),
    SimpleSpan::new(0, 1),
    "the cache-front put-back is intact"
  );
  assert_eq!(
    scanned.get(),
    1,
    "the follow-up consume is served from cache, not rescanned"
  );
}

#[test]
fn try_expect_or_stop_declines_on_genuine_eoi() {
  // Empty input under a roomy limit: genuine end of input declines.
  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache("", limiter, cache);
  let mut inp = input.as_ref(&mut emitter);
  assert!(
    inp.try_expect_or_stop(|_| true).unwrap().is_none(),
    "empty input declines"
  );

  // Fully-consumed input under a roomy limit: the exhaustion is genuine too.
  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache("1", limiter, cache);
  let mut inp = input.as_ref(&mut emitter);
  assert!(inp.next().unwrap().is_some(), "consume the only token");
  assert!(
    inp.try_expect_or_stop(|_| true).unwrap().is_none(),
    "a fully-consumed input declines"
  );
}

#[test]
fn try_expect_or_stop_consumes_match() {
  use crate::span::SimpleSpan;

  let (mut input, scanned) = probe_input("1 2");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let tok = inp
    .try_expect_or_stop(|_| true)
    .unwrap()
    .expect("the matching token is consumed");
  assert_eq!(*tok.span_ref(), SimpleSpan::new(0, 1));
  assert_eq!(scanned.get(), 1, "one scan for the consumed token");

  // The cursor advanced: the next consume yields the second token.
  let tok = inp.next().unwrap().expect("cursor advanced past the match");
  assert_eq!(*tok.span_ref(), SimpleSpan::new(2, 3));
}

// ── `next_or_stop`: the committed-consume primitive that surfaces terminal stops ──
//
// `next` folds a terminal stop (a fresh limit trip, or an already-latched poison boundary) into the
// same `Ok(None)` as a genuine end of input — the fold every committed leaf used to turn into a
// plain, recoverable `UnexpectedEot`. `next_or_stop` keeps `Ok(None)` for genuine end of input only
// and surfaces a terminal stop as the committed forms' end-of-input error on the `Err` channel
// instead, so the leaf's terminal case re-raises through recovery rather than being recovered.

#[test]
fn next_or_stop_errs_on_fresh_trip() {
  // The first two calls consume `1` and `2`; the third rebuilds a lexer whose scan trips the
  // limiter — a terminal stop, not genuine end of input.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(inp.next_or_stop().unwrap().is_some(), "first token");
  assert!(inp.next_or_stop().unwrap().is_some(), "second token");
  assert!(
    matches!(inp.next_or_stop(), Err(ProbeErr::Eot)),
    "a fresh trip surfaces the committed forms' end-of-input error, never a silent None"
  );
  assert_eq!(scanned.get(), 3, "scanned exactly 1, 2, and the tripping 3");
}

#[test]
fn next_or_stop_errs_on_latched_boundary_without_rescan() {
  // Latch via `next()`: the third scan trips and latches the poison boundary.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(inp.next().unwrap().is_some(), "first token");
  assert!(inp.next().unwrap().is_some(), "second token");
  assert!(inp.next().unwrap().is_none(), "trip latches to None");

  let frozen = scanned.get();
  assert_eq!(frozen, 3, "scanned exactly 1, 2, 3 before latching");

  // Every consume at the latched boundary errs — and never rebuilds a lexer.
  for _ in 0..5 {
    assert!(
      matches!(inp.next_or_stop(), Err(ProbeErr::Eot)),
      "a latched boundary surfaces the end-of-input error on every consume"
    );
  }
  assert_eq!(
    scanned.get(),
    frozen,
    "no lexer was rebuilt after the latch — the token counter is frozen"
  );
}

#[test]
fn next_or_stop_returns_none_at_genuine_eoi() {
  // Empty input under a roomy limit: genuine end of input yields a plain `None`.
  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache("", limiter, cache);
  let mut inp = input.as_ref(&mut emitter);
  assert!(
    inp.next_or_stop().unwrap().is_none(),
    "empty input is a genuine end of input — a plain None"
  );

  // Fully-consumed input under a roomy limit: the exhaustion is genuine too, and the consumed
  // token comes back before it.
  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache("1", limiter, cache);
  let mut inp = input.as_ref(&mut emitter);
  assert!(
    inp.next_or_stop().unwrap().is_some(),
    "consume the only token"
  );
  assert!(
    inp.next_or_stop().unwrap().is_none(),
    "a fully-consumed input is a genuine end of input — a plain None"
  );
}

// ── `try_expect_map_or_stop`: the map-shaped attempt primitive (the token-pratt path) ──
//
// The same terminal-stop law as `try_expect_or_stop`, for a mapping predicate: a fresh trip is the
// committed forms' terminal end-of-input error, never a decline, while a non-matching token and a
// genuine end of input still decline.

#[test]
fn try_expect_map_or_stop_errs_on_fresh_trip() {
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(
    inp.try_expect_map_or_stop(|_| Some(())).unwrap().is_some(),
    "first token maps"
  );
  assert!(
    inp.try_expect_map_or_stop(|_| Some(())).unwrap().is_some(),
    "second token maps"
  );
  assert!(
    matches!(inp.try_expect_map_or_stop(|_| Some(())), Err(ProbeErr::Eot)),
    "a fresh trip surfaces the committed forms' end-of-input error, never a decline"
  );
  assert_eq!(scanned.get(), 3, "scanned exactly 1, 2, and the tripping 3");
}

#[test]
fn try_expect_map_or_stop_declines_on_non_matching_and_genuine_eoi() {
  use crate::span::SimpleSpan;

  // A `None` map is definite absence: `Ok(None)`, token put back at the cache front.
  let (mut input, scanned) = probe_input("1 2");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(
    inp
      .try_expect_map_or_stop(|_| None::<()>)
      .unwrap()
      .is_none(),
    "a non-matching token declines"
  );
  assert_eq!(scanned.get(), 1, "one scan staged the declined token");
  let tok = inp
    .next()
    .unwrap()
    .expect("the declined token is still next");
  assert_eq!(
    *tok.span_ref(),
    SimpleSpan::new(0, 1),
    "the cache-front put-back is intact"
  );

  // Genuine end of input under a roomy limit declines.
  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache("", limiter, cache);
  let mut inp = input.as_ref(&mut emitter);
  assert!(
    inp.try_expect_map_or_stop(|_| Some(())).unwrap().is_none(),
    "empty input declines"
  );
}

#[test]
fn poisoned_input_latches_no_rescan_across_skip_while() {
  // `skip_while(|_| true)` drains every matching token in a single call, so the
  // first call scans through the tripping token and latches.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  inp.skip_while(|_| true).unwrap();

  let frozen = scanned.get();
  assert_eq!(frozen, 3, "scanned exactly 1, 2, 3 before latching");

  // Repeated calls must short-circuit on the latch without rescanning.
  for _ in 0..5 {
    inp.skip_while(|_| true).unwrap();
  }
  assert_eq!(
    scanned.get(),
    frozen,
    "no lexer was rebuilt after the latch — the token counter is frozen"
  );
}

#[test]
fn poisoned_input_latches_no_rescan_across_sync_to() {
  // `sync_to(|_| false, ..)` never matches, so it skips through the whole input;
  // the first call scans the tripping token and latches.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(
    inp.sync_to(|_| false, || None).unwrap().is_none(),
    "no matching token before the trip"
  );

  let frozen = scanned.get();
  assert_eq!(frozen, 3, "scanned exactly 1, 2, 3 before latching");

  for _ in 0..5 {
    assert!(
      inp.sync_to(|_| false, || None).unwrap().is_none(),
      "poisoned sync_to stays None"
    );
  }
  assert_eq!(
    scanned.get(),
    frozen,
    "no lexer was rebuilt after the latch — the token counter is frozen"
  );
}

#[test]
fn poisoned_input_latches_no_rescan_across_sync_through() {
  // `sync_through(|_| false, ..)` never matches, so it skips through the whole
  // input; the first call scans the tripping token and latches.
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(
    inp.sync_through(|_| false, || None).unwrap().is_none(),
    "no matching token before the trip"
  );

  let frozen = scanned.get();
  assert_eq!(frozen, 3, "scanned exactly 1, 2, 3 before latching");

  for _ in 0..5 {
    assert!(
      inp.sync_through(|_| false, || None).unwrap().is_none(),
      "poisoned sync_through stays None"
    );
  }
  assert_eq!(
    scanned.get(),
    frozen,
    "no lexer was rebuilt after the latch — the token counter is frozen"
  );
}

#[test]
fn restore_after_peek_across_lexer_error_reemits_error_exactly_once() {
  // `@` is a lexer error between two numbers (high limit: the limiter never
  // trips, so only the plain lexer error is in play).
  //   1 @ 2 3
  //   0 2 4 6      (`@` spans [2, 3))
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );

  {
    use generic_arraydeque::typenum::U2;
    let mut inp = input.as_ref(&mut emitter);

    // Peek a window that crosses the malformed `@`; this emits (seals) its lexer
    // error and advances the dedup watermark past it. The cursor stays at 0.
    let _ = inp.peek::<U2>().unwrap();

    // Checkpoint is captured AFTER the error's emission, so `Verbose` retains the
    // error across the restore (the emission log keeps everything up to the mark).
    let ckp = inp.save();

    // Speculatively consume forward, draining the cache and lexing past the error
    // region, then abandon the branch.
    while inp.next().unwrap().is_some() {}
    inp.restore(ckp);

    // The commit path re-lexes from the checkpoint, crossing the malformed span a
    // second time. With the watermark restored to its saved value (past the
    // error), the re-lex must NOT re-emit the retained error.
    while inp.next().unwrap().is_some() {}
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "the malformed span's lexer error must appear exactly once after peek → save → restore → re-consume"
  );
}

#[test]
fn restore_drops_cache_entries_from_abandoned_lineage() {
  // A cached token prefilled BEFORE the save makes the checkpoint cursor equal the cache
  // front, so the cache rewind takes its no-op (cursor == front) branch and leaves the
  // cache untouched. A wider peek AFTER the save then crosses the malformed `@`, emitting
  // its lexer error and caching the tokens that follow it. Those post-save entries belong
  // to the abandoned continuation: restore rewinds the error's emission, and unless the
  // entries are dropped a later drain pops straight over the rewound error — so it is
  // never re-emitted. The token VALUES are faithfully memoized either way; only the scan
  // side effect (the error emission) is lost.
  //   1 @ 2 3
  //   0 . 4 6      (`@` spans [2, 3); high limit so only the plain lexer error is in play)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::{U1, U3};

  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Prefill exactly one cached token (`1`); the cursor now sits at its start, so the
    // save's cursor equals the cache front.
    let _ = inp.peek::<U1>().unwrap();

    // Save BEFORE the error is crossed: the checkpoint predates the `@` emission, so
    // restoring rewinds that emission.
    let ckp = inp.save();

    // Peek across the malformed `@`: emits its lexer error and caches the tokens that
    // follow it (`2`, `3`) — entries from the continuation we are about to abandon.
    let _ = inp.peek::<U3>().unwrap();

    // Abandon the continuation.
    inp.restore(ckp);

    // Drain to EOF on the committed path: it must re-lex the `@` region and re-emit the
    // error exactly once, then yield the full faithful token sequence.
    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7)
    ],
    "the drained stream is the full faithful token sequence"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "the rolled-back lexer error must be re-emitted exactly once after restore drops the abandoned cache entries"
  );
}

#[test]
fn restore_option_cache_capacity_one_reemits_error_once() {
  // The capacity-1 `Option` cache cannot express the abandoned-lineage hole: hitting the
  // rewind's no-op (cursor == front) branch needs a prefilled entry occupying the cache,
  // which at capacity 1 leaves no room to also cache a post-error token. The token that
  // follows the `@` overflows instead of being cached, so nothing from the abandoned
  // continuation survives the restore and the error region always re-lexes. This pins
  // that faithful behavior and guards the truncation against wrongly dropping the
  // surviving pre-save entry.
  //   1 @ 2 3      (`@` spans [2, 3))
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::{U1, U2};

  let cache: ProbeOptionCache<'_> = None;
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeOptionVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Prefill the single slot with `1`; the cursor sits at its start.
    let _ = inp.peek::<U1>().unwrap();
    let ckp = inp.save();
    // Peek across `@`: the error is emitted, but `2` cannot be cached (slot full) and
    // overflows instead of surviving the restore.
    let _ = inp.peek::<U2>().unwrap();
    inp.restore(ckp);

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7)
    ],
    "the capacity-1 cache still drains the full faithful token sequence"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(total, 1, "the error re-lexes and is emitted exactly once");
}

#[test]
fn nested_restore_retains_pre_save_cache_entries() {
  // Nested LIFO over a prefilled cache entry. Prefill exactly one cached token, then
  // stack two saves on top of it (nothing consumed between them, so BOTH checkpoint
  // cursors equal the cache front and the rewind takes its no-op branch). Peek several
  // more tokens into the continuation the restores abandon, then restore inner and
  // restore outer. The prefilled token predates both saves, so it must survive both
  // restores and be served FROM CACHE on the drain — never re-lexed.
  //
  // The push count is per-lineage state that a restore copies back to its saved value,
  // exactly like the dedup watermark and the poison boundary. The inner restore drops
  // its post-save tail (`2`,`3`) and rewinds the count to the inner save's value; the
  // outer restore then computes zero post-save survivors and keeps the prefilled `1`.
  // At HEAD the count was never rewound, so the outer restore saw a stale-high count and
  // over-dropped `1`; re-consuming re-lexed it — the shared `ProbeLimiter` counter, which
  // observes every scan, makes that re-lex visible as a nonzero delta across one `next()`.
  //   1 2 3 4 5   (all valid Nums; a `usize::MAX` limit never trips)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::{U1, U3};

  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let scanned = limiter.counter();
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    limiter,
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Prefill exactly one cached token (`1`); the cursor now equals the cache front.
    let _ = inp.peek::<U1>().unwrap();
    // Stack two saves on the prefilled front.
    let outer = inp.save();
    let inner = inp.save();
    // Peek more: caches `2`,`3` — post-save entries — lifting the input-wide push count
    // above BOTH checkpoints' saved values.
    let _ = inp.peek::<U3>().unwrap();
    // Inner restore drops the post-inner tail; outer restore must NOT over-drop `1`.
    inp.restore(inner);
    inp.restore(outer);

    // (a) The prefilled `1` is served FROM CACHE: consuming it does no scan work, so the
    // shared counter is unchanged across this single `next()`. At HEAD the outer restore
    // over-drops `1`, so it re-lexes here and the counter ticks up.
    let before = scanned.get();
    let first = inp.next().unwrap().expect("first drained token");
    assert_eq!(
      *first.span_ref(),
      SimpleSpan::new(0, 1),
      "the first drained token is `1`"
    );
    assert_eq!(
      scanned.get(),
      before,
      "the pre-save cache entry must be served from cache, never re-lexed (scan counter unchanged)"
    );

    // (b) The full token stream is faithful.
    let mut toks = std::vec![*first.span_ref()];
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
      SimpleSpan::new(8, 9),
    ],
    "the drained stream is the full faithful token sequence"
  );
  // (c) No poison diagnostic — nor any diagnostic; the input is clean and never trips.
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(total, 0, "a clean nested restore emits no diagnostic");
}

#[test]
fn nested_restore_with_shared_limiter_no_spurious_poison() {
  // The same nested LIFO shape, but under a SHARED-counter limiter whose budget is one
  // scan short of tolerating the over-drop's re-lex. A faithful drain serves the prefilled
  // `1` from cache (no scan) and re-lexes only the post-save `2`,`3`,`4`,`5`, reaching a
  // count of 7 — untripped at the limit of 7. The HEAD over-drop also re-lexes `1`,
  // reaching 8 and tripping the limiter on the fifth drained token: a spurious poison latch
  // and a limit diagnostic this checkpoint's lineage never produced.
  //   1 2 3 4 5   (limit 7)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::{U1, U3};

  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    ProbeLimiter::with_limit(7),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    let _ = inp.peek::<U1>().unwrap(); // prefill `1`
    let outer = inp.save();
    let inner = inp.save();
    let _ = inp.peek::<U3>().unwrap(); // cache post-save `2`,`3`
    inp.restore(inner);
    inp.restore(outer);

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    // The faithful drain reaches EOF untripped: `1` came from cache, so the extra scan
    // that would trip the limiter is never spent.
    assert!(
      !inp.is_poisoned(),
      "serving the pre-save entry from cache must not spend the extra scan that spuriously trips the limiter"
    );
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
      SimpleSpan::new(8, 9),
    ],
    "the full faithful stream drains — no token is lost to a spurious trip"
  );
  let limit_diags = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ProbeErr::Limit)
    .count();
  assert_eq!(
    limit_diags, 0,
    "no spurious limit diagnostic — the checkpoint's lineage never tripped"
  );
}

#[test]
fn consumed_pre_save_cache_entry_relexes_identically_on_restore() {
  // A legal last-in, first-out shape over a CONSUMED pre-save cache entry. `peek(1)`
  // stages token `T`; `save` snapshots the lineage; `next()` consumes `T` FROM CACHE;
  // `restore` returns to the save. The abandoned branch already drained `T` out of the
  // cache, so the cache no longer holds it and the first post-restore read RE-LEXES `T`
  // from source. That re-lex is the architecture — a restore replays a dropped or
  // consumed cached token on demand — and by the `Lexer` determinism contract it is
  // observationally identical: the same token, the same span, the diagnostics exactly
  // once, and an in-`State` limiter recounting the same total. Only instrumentation that
  // lives OUTSIDE the lexer state (here a shared scan counter) sees the extra scan.
  //
  // This pins the current behavior: a change that snapshotted consumed cache entries to
  // skip the re-lex would alter the scan counts or the stream below and trip it.
  //   1 @ 2 3      (`@` is a lexer error spanning [2, 3); the limit never trips)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U1;

  const SRC: &str = "1 @ 2 3";

  // A fresh single pass over the same source: the faithful stream, the scan count, and
  // the diagnostic count the replay must reproduce.
  let (oracle_spans, oracle_scans, oracle_diags): (Vec<SimpleSpan>, usize, usize) = {
    let limiter = ProbeLimiter::with_limit(usize::MAX);
    let scanned = limiter.counter();
    let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
    let mut emitter = Verbose::<ProbeErr>::new();
    let mut input =
      Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(SRC, limiter, cache);
    let spans = {
      let mut inp = input.as_ref(&mut emitter);
      let mut toks = Vec::new();
      while let Some(t) = inp.next().unwrap() {
        toks.push(*t.span_ref());
      }
      toks
    };
    let diags: usize = emitter.errors().values().map(|group| group.len()).sum();
    (spans, scanned.get(), diags)
  };
  assert_eq!(
    oracle_spans,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7)
    ],
    "single-pass oracle: the faithful token stream"
  );
  assert_eq!(
    oracle_scans, 3,
    "single-pass oracle: three Nums, each scanned once"
  );
  assert_eq!(oracle_diags, 1, "single-pass oracle: the `@` error, once");

  // ── Shared observer: the consumed `T` re-lexes with exactly one extra scan. ──
  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let scanned = limiter.counter();
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(SRC, limiter, cache);
  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Stage `T` (`1`): exactly one scan.
    let _ = inp.peek::<U1>().unwrap();
    assert_eq!(scanned.get(), 1, "peek(1) scans `T` exactly once");

    let ckp = inp.save();

    // Consume `T` FROM CACHE: served from the cache, so no scan runs.
    let before_consume = scanned.get();
    let consumed = inp.next().unwrap().expect("consume `T` from cache");
    assert_eq!(
      *consumed.span_ref(),
      SimpleSpan::new(0, 1),
      "the consumed token is `T`"
    );
    assert_eq!(
      scanned.get(),
      before_consume,
      "consuming `T` from cache re-scans nothing"
    );

    // Abandon the branch. `T` is no longer cached, so the next read re-lexes it.
    inp.restore(ckp);

    // The first post-restore read RE-LEXES `T`: exactly one additional scan, and only
    // the shared counter (instrumentation outside the lexer state) observes it. This is
    // expected by design — the replay re-lexes on demand.
    let before_replay = scanned.get();
    let replayed = inp.next().unwrap().expect("re-lex `T`");
    assert_eq!(
      *replayed.span_ref(),
      SimpleSpan::new(0, 1),
      "`T` re-lexes to the same span"
    );
    assert_eq!(
      scanned.get(),
      before_replay + 1,
      "re-lexing `T` is exactly one additional scan"
    );

    let mut toks = std::vec![*replayed.span_ref()];
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  // (i) The drained stream is the full faithful sequence — `T` appears exactly once.
  assert_eq!(
    drained, oracle_spans,
    "the drained stream is the full faithful token sequence — `T` appears exactly once"
  );
  // (ii) Exactly one scan beyond a single pass: the consumed `T`'s replay.
  assert_eq!(
    scanned.get(),
    oracle_scans + 1,
    "the replay costs exactly one scan beyond a single pass — only outside-state instrumentation observes it"
  );
  // (iv) Diagnostics are emitted exactly once, matching a single pass.
  let diags: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    diags, oracle_diags,
    "the `@` error is emitted exactly once, as in a single pass"
  );

  // ── By-value limiter: restore rewinds its state, so the replay recounts identically. ──
  // The knife-edge budget equals the single-pass scan count. The consumed `T`'s replay
  // re-lexes the whole source once, and because `restore` rewinds the in-`State` counter
  // the recount reaches exactly that total — not one past it — so the budget never trips.
  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    SRC,
    TokenLimiter::with_limitation(3),
    cache,
  );
  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);
    let _ = inp.peek::<U1>().unwrap();
    let ckp = inp.save();
    let _ = inp.next().unwrap().expect("consume `T` from cache");
    inp.restore(ckp);
    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    // The rewound limiter recounts the replay identically to a first pass, reaching the
    // knife-edge exactly.
    assert!(
      !inp.is_poisoned(),
      "the restored by-value limiter recounts the replay identically; a single-pass budget never trips"
    );
    assert_eq!(
      inp.state().tokens(),
      3,
      "the replay counts exactly the single-pass token total"
    );
    toks
  };
  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7)
    ],
    "the by-value replay drains the full faithful stream"
  );
  let diags: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    diags, 1,
    "the by-value replay emits the `@` error exactly once"
  );
}

#[test]
fn consume_all_cached_then_restore_replays_faithfully() {
  // The same consumed-prefix architecture, but the abandoned branch drains the WHOLE
  // cached run at once through `consume_all_cached` before the restore. Nothing pre-save
  // survives in the cache, so the committed path re-lexes the entire run on demand. By
  // the `Lexer` determinism contract the replay is faithful: the full token stream
  // returns, the lexer error is emitted exactly once, and the by-value limiter — its
  // state rewound by the restore — recounts the replay identically, so a knife-edge
  // budget equal to the single-pass token count never trips.
  //
  // This pins the current behavior: a change that snapshotted the consumed run to skip
  // the re-lex would alter the stream, the diagnostics, or the recount below and trip it.
  //   1 @ 2 3 4 5      (`@` is a lexer error spanning [2, 3))
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  const SRC: &str = "1 @ 2 3 4 5";

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    SRC,
    // Knife-edge: the single pass scans five Nums, so a budget of five tolerates the
    // faithful replay exactly and one less would trip.
    TokenLimiter::with_limitation(5),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Fill the cache with the first three tokens (crossing and sealing `@`).
    let _ = inp.peek::<U3>().unwrap();
    let ckp = inp.save();

    // Consume the ENTIRE cached run at once; it returns the last cached token (`3`).
    let last = inp.consume_all_cached().expect("consume the cached run");
    assert_eq!(
      *last.span_ref(),
      SimpleSpan::new(6, 7),
      "consume_all_cached returns the last cached token (`3`)"
    );

    // Abandon the branch. The run is gone from the cache, so the drain re-lexes it.
    inp.restore(ckp);

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    // The rewound by-value limiter recounts the whole replay from scratch, reaching the
    // knife-edge exactly — re-lexing the consumed run costs no spurious trip.
    assert!(
      !inp.is_poisoned(),
      "the restored by-value limiter recounts the replay identically; the knife-edge budget never trips"
    );
    assert_eq!(
      inp.state().tokens(),
      5,
      "the replay counts exactly the single-pass token total"
    );
    toks
  };

  // The whole run re-lexes, then the tokens past it follow — the full faithful stream.
  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
      SimpleSpan::new(8, 9),
      SimpleSpan::new(10, 11),
    ],
    "the drained stream is the full faithful token sequence"
  );
  // The `@` error is emitted exactly once across peek → consume-run → restore → drain.
  let diags: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    diags, 1,
    "the `@` error is emitted exactly once, as in a single pass"
  );
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "non-LIFO checkpoint restore")]
fn non_lifo_watermark_restore_is_rejected_in_debug() {
  // Contract: restores are last-in, first-out. Restoring the older checkpoint A
  // invalidates every checkpoint saved after it, so restoring the younger B afterward
  // refers to a lineage that no longer exists. The debug witness rejects it. (This is
  // the dedup-watermark shape: A predates a sealed `@`, B postdates it.)
  //   1 @ 2 3      (`@` is a lexer error spanning [2, 3))
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );

  use generic_arraydeque::typenum::U2;
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save(); // older, predates the sealed `@`
  let _ = inp.peek::<U2>().unwrap(); // seals `@`, lifts the watermark
  let b = inp.save(); // younger, postdates the sealed `@`
  while inp.next().unwrap().is_some() {}

  inp.restore(a); // invalidates b
  inp.restore(b); // ✗ non-LIFO — debug panic
}

#[test]
fn restore_before_overflow_trip_reemits_limit_diagnostic_exactly_once() {
  // A limit trip during an *overflow* peek latches poison AND emits the limit
  // diagnostic together. A caller that saved BEFORE that speculative peek and then
  // restores must not be left silently poisoned: `restore` un-latches the poison
  // (the AND-clamp lowers it toward the clean saved value) in lockstep with the
  // emitter rewind that removed the speculative diagnostic. The committed drain
  // then re-lexes the region, re-trips, re-latches, and RE-EMITS the diagnostic —
  // exactly once, never a diagnostic-less latch masquerading as clean EOF.
  //   1 2 3 4 5 6   (limit 5 → the 6th scanned token trips; U6 window > U3 cache)
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    ProbeLimiter::with_limit(5),
    cache,
  );

  {
    use generic_arraydeque::typenum::U6;
    let mut inp = input.as_ref(&mut emitter);

    // save BEFORE the speculative peek: the checkpoint is clean (poisoned = false).
    let ckp = inp.save();

    // Overflow peek (U6 > U3 cache) trips the limiter mid-overflow: poison latches
    // and the limit diagnostic is sealed into the emitter.
    let _ = inp.peek::<U6>().unwrap();
    assert!(inp.is_poisoned(), "the overflow trip must latch poison");

    // Restore the pre-peek checkpoint: the emitter rewinds the speculative
    // diagnostic AND the AND-clamp lowers poison back to the clean saved value, so
    // the latch is not left stranded without its diagnostic.
    inp.restore(ckp);
    assert!(
      !inp.is_poisoned(),
      "restoring a clean checkpoint must un-latch the speculative poison"
    );

    // Drain the committed path: it re-lexes the region and re-trips the limiter.
    while inp.next().unwrap().is_some() {}

    // The re-trip re-establishes the latch — poison stays paired with its diagnostic.
    assert!(
      inp.is_poisoned(),
      "the committed re-lex must re-latch poison"
    );
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "the limit diagnostic must survive save → overflow-trip → restore → drain, reported exactly once"
  );
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "non-LIFO checkpoint restore")]
fn non_lifo_poison_boundary_restore_is_rejected_in_debug() {
  // Contract: restores are last-in, first-out — the poison-boundary analog of the
  // watermark case. A clean older A, an overflow trip that poisons the input, a
  // poisoned younger B; restoring A invalidates B, so restoring B afterward is a
  // violation the debug witness rejects.
  //   1 2 3 4 5 6   (limit 5 → the 6th scanned token trips; U6 window > U3 cache)
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    ProbeLimiter::with_limit(5),
    cache,
  );

  use generic_arraydeque::typenum::U6;
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save(); // older, clean
  let _ = inp.peek::<U6>().unwrap(); // overflow trip: poison + diagnostic
  assert!(inp.is_poisoned(), "the overflow trip must latch poison");
  let b = inp.save(); // younger, poisoned
  while inp.next().unwrap().is_some() {}

  inp.restore(a); // invalidates b
  inp.restore(b); // ✗ non-LIFO — debug panic
}

// ── The poison BOUNDARY: a drained cache prefix replays after a restore ────────
//
// These use a BY-VALUE limiter (`TokenLimiter`, checkpointed/restored with the
// lexer state) rather than the shared `ProbeLimiter`. The distinction is load
// bearing: an overflow peek never writes its temporary lexer's counter back into
// the input state, so a checkpoint taken *after* the trip still saves a clean
// count. Restoring it therefore lets the committed path re-lex the prefix from
// scratch, re-counting toward the same limit and re-tripping at the very position
// it would have — which is exactly what makes a positional boundary observable: a
// shared counter would instead re-trip on the first replayed token and hide the
// prefix again.

use crate::state::token_tracker::{TokenLimitExceeded, TokenLimiter};

#[derive(Debug, Clone, PartialEq)]
enum ByValErr {
  Lex,
  Limit,
}

impl From<()> for ByValErr {
  fn from(_: ()) -> Self {
    ByValErr::Lex
  }
}

impl From<TokenLimitExceeded> for ByValErr {
  fn from(_: TokenLimitExceeded) -> Self {
    ByValErr::Limit
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for ByValErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ByValErr::Lex
  }
}

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = TokenLimiter, skip r"[ \t\r\n]+")]
enum ByValTok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); })]
  Num,
}

impl core::fmt::Display for ByValTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ByValKind {
  Num,
}

impl core::fmt::Display for ByValKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl Token<'_> for ByValTok {
  type Kind = ByValKind;
  type Error = ByValErr;

  fn kind(&self) -> ByValKind {
    ByValKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type ByValLexer<'a> = LogosLexer<'a, ByValTok>;
type ByValVerboseCtx<'a> = (Verbose<ByValErr>, DefaultCache<'a, ByValLexer<'a>>);

#[test]
fn overflow_trip_peek_save_drain_restore_replays_prefix_and_stops_at_boundary() {
  // THE positional-boundary case. An overflow peek trips mid-window, truncates to
  // the cache-resident prefix, and latches the poison boundary at the DURABLE
  // FRONTIER — the end of the last cached token. A caller then SAVES, drains the
  // prefix speculatively, and RESTORES the same checkpoint. It must observe:
  //   (a) the prefix tokens are consumable AGAIN (same spans, same order) — the
  //       cache was drained, so the boundary lets lexing strictly before it
  //       replay the prefix from source;
  //   (b) after the prefix the stream ends AT the boundary — the trip token and
  //       everything past it are never re-scanned (frozen scan counter);
  //   (c) the limit diagnostic is retained exactly once.
  //
  // Under the old boolean latch the restore left the input fully latched, so the
  // prefix visible at save time became unreachable (the first replay `next()`
  // short-circuited to `None`): restore did not reproduce the saved state.
  //
  //   1 2 3 4 5 6   (limit 5 → the 6th scanned token trips; U6 window > U3 cache)
  //   ^0 ^2 ^4      (token 3 spans [4, 5): the durable frontier is offset 5)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U6;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(5),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    // Overflow peek (U6 > U3 cache): caches 1..=3, stages 4 & 5, trips on 6. The
    // result is truncated to the 3-token cache-resident prefix; the boundary
    // latches at the durable frontier (end of token 3, offset 5).
    {
      let peeked = inp.peek::<U6>().unwrap();
      assert_eq!(
        peeked.len(),
        3,
        "the overflow trip truncates the peek to the cache-resident prefix"
      );
    }
    assert!(
      inp.is_poisoned(),
      "the overflow trip latches the poison boundary"
    );

    // Save AFTER the trip: the checkpoint carries the boundary AND the retained
    // limit diagnostic (its emitter mark postdates the emission).
    let ckp = inp.save();

    // Speculatively drain the cached prefix. The three cached tokens come back;
    // then the boundary stops `next()` at the durable frontier (no phantom 4/5/6).
    assert_eq!(
      *inp.next().unwrap().expect("drain 1").span_ref(),
      SimpleSpan::new(0, 1)
    );
    assert_eq!(
      *inp.next().unwrap().expect("drain 2").span_ref(),
      SimpleSpan::new(2, 3)
    );
    assert_eq!(
      *inp.next().unwrap().expect("drain 3").span_ref(),
      SimpleSpan::new(4, 5)
    );
    assert!(
      inp.next().unwrap().is_none(),
      "the drain stops at the boundary — no phantom lookahead"
    );

    // Restore the SAME checkpoint: `boundary = max(saved, current)` keeps it intact.
    inp.restore(ckp);
    assert!(
      inp.is_poisoned(),
      "the boundary survives the restore (saved == current)"
    );

    // (a) The prefix is consumable AGAIN — same spans, same order — replayed from
    // source because the cache was drained.
    assert_eq!(
      *inp.next().unwrap().expect("replay 1").span_ref(),
      SimpleSpan::new(0, 1)
    );
    assert_eq!(
      *inp.next().unwrap().expect("replay 2").span_ref(),
      SimpleSpan::new(2, 3)
    );
    assert_eq!(
      *inp.next().unwrap().expect("replay 3").span_ref(),
      SimpleSpan::new(4, 5)
    );
    // (b) After the prefix the stream ends exactly at the boundary.
    assert!(
      inp.next().unwrap().is_none(),
      "the replay stops at the boundary — nothing past it is re-scanned"
    );
    // The frozen scan counter proves it: the replay re-scanned exactly the 3-token
    // prefix (the trip token past the boundary is never reached), so the current
    // lexer lineage's count is 3, not 4+.
    assert_eq!(
      inp.state().tokens(),
      3,
      "the replay scanned exactly the prefix (3), never the trip token past the boundary"
    );
  }

  // (c) The limit diagnostic is retained across save → drain → restore → replay,
  // exactly once.
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "the limit diagnostic survives save → drain → restore → replay, reported exactly once"
  );
}

#[test]
fn sync_through_trip_after_skips_commits_the_diagnosed_prefix() {
  // `sync_through` scans forward diagnosing every non-matching token; if a limit trips
  // after some are skipped, the diagnosed prefix must be COMMITTED at the durable
  // frontier — the end of the last skipped token — so the boundary latches there and a
  // later scan yields the poisoned outcome at that frontier, never rewinding to the
  // pre-call cursor and stranding tokens that were already diagnosed.
  //
  // A by-value in-`State` limiter makes the commit observable: the frontier snapshots
  // the lexer state at the moment it advances over a token, so the committed count is
  // the pre-trip prefix count (2), distinct from the total scanned (3, incl. the trip
  // token). `sync_through(|_| false, ..)` matches nothing, so it skips-and-diagnoses `1`
  // and `2`, then the 3rd scanned token (`3`, span [4, 5)) trips before any target.
  //   1 2 3 4 5 6   (limit 2 → the 3rd scanned token trips; `2` ends at offset 3)
  use crate::span::SimpleSpan;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    cache,
  );

  {
    let mut inp = input.as_ref(&mut emitter);

    // Pre-call anchor: nothing consumed yet.
    assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "pre-call span anchor");
    assert_eq!(inp.state().tokens(), 0, "pre-call token count");

    // No target is ever matched: the scan diagnoses `1` and `2`, then trips on `3`.
    assert!(
      inp.sync_through(|_| false, || None).unwrap().is_none(),
      "the trip yields the poisoned outcome — no matching token"
    );

    // (i) The diagnosed prefix is COMMITTED at the durable frontier (end of `2`,
    // offset 3), NOT stranded at the stale pre-call anchor (offset 0) that the old
    // `AtCursor` policy left behind. At HEAD this span was still `[0, 0)` and the
    // token count still 0 — the regression this test pins.
    assert!(inp.is_poisoned(), "the trip latches the poison boundary");
    assert_eq!(
      inp.span(),
      &SimpleSpan::new(2, 3),
      "committed span sits at the end of the last diagnosed token (`2`)"
    );
    assert_eq!(
      inp.state().tokens(),
      2,
      "committed state counts exactly the diagnosed prefix (`1`, `2`) — not the trip token"
    );

    // (ii) A subsequent `next()` yields the poisoned outcome AT that boundary without
    // rescanning the diagnosed tokens: the committed by-value counter stays frozen at 2
    // (no re-lex of `1`/`2`) and nothing past the boundary is scanned.
    assert!(
      inp.next().unwrap().is_none(),
      "next() stops at the committed boundary"
    );
    assert_eq!(
      inp.state().tokens(),
      2,
      "the committed lineage's scan counter is frozen — `1`/`2` are not rescanned"
    );
  }

  // (iii) Each skipped token is diagnosed exactly once (`1`, `2` → two unexpected-token
  // errors) and the limit trip exactly once (`3` → one limit error).
  let unexpected = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Lex)
    .count();
  let limit = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();
  assert_eq!(
    unexpected, 2,
    "each skipped token is diagnosed exactly once (`1`, `2`)"
  );
  assert_eq!(limit, 1, "the limit trip is diagnosed exactly once (`3`)");
}

#[test]
fn sync_through_then_peek_trip_after_skips_commits_the_diagnosed_prefix() {
  // The twin of `sync_through_trip_after_skips_commits_the_diagnosed_prefix` for the
  // separately-reachable `sync_through_then_peek` loop: the same commit-at-the-frontier
  // behavior must hold there too. `sync_through_then_peek(|_| false, ..)` matches
  // nothing, diagnoses `1` and `2`, then trips on `3` and returns no matched token and
  // an empty peek — committing the diagnosed prefix at the durable frontier (offset 3).
  //   1 2 3 4 5 6   (limit 2 → the 3rd scanned token trips; `2` ends at offset 3)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U1;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    cache,
  );

  {
    let mut inp = input.as_ref(&mut emitter);

    let (matched, peeked) = inp
      .sync_through_then_peek::<_, _, U1>(|_| false, || None)
      .unwrap();
    assert!(matched.is_none(), "the trip yields no matched token");
    assert!(peeked.is_empty(), "the trip yields an empty peek");
    // `peeked` borrows `inp` for its lifetime; release it before reusing `inp`.
    drop(peeked);

    // The diagnosed prefix is committed at the durable frontier (end of `2`, offset 3),
    // not stranded at the pre-call anchor.
    assert!(inp.is_poisoned(), "the trip latches the poison boundary");
    assert_eq!(
      inp.span(),
      &SimpleSpan::new(2, 3),
      "committed span sits at the end of the last diagnosed token (`2`)"
    );
    assert_eq!(
      inp.state().tokens(),
      2,
      "committed state counts exactly the diagnosed prefix (`1`, `2`)"
    );

    // A subsequent `next()` stops at that boundary without rescanning `1`/`2`.
    assert!(
      inp.next().unwrap().is_none(),
      "next() stops at the committed boundary"
    );
    assert_eq!(
      inp.state().tokens(),
      2,
      "the committed lineage's scan counter is frozen"
    );
  }

  let unexpected = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Lex)
    .count();
  let limit = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();
  assert_eq!(
    unexpected, 2,
    "each skipped token is diagnosed exactly once (`1`, `2`)"
  );
  assert_eq!(limit, 1, "the limit trip is diagnosed exactly once (`3`)");
}

/// Pins CURRENT WRONG behavior (issue #89): `consumed`/`limit_diags` below should read
/// the disciplined values (stop at 2, one diagnostic at the 3rd) once it is fixed.
///
/// Repeated widen-then-drain rounds (`peek::<U1>` → `peek::<U2>` → `peek::<U3>`, then
/// consume the window), by-value limit 2 — this reproduces the bug at its exact shape and
/// numbers. `lexer()` pairs COMMITTED state with the cache-back offset
/// (`input_ref/mod.rs`), and each top-level `peek` call that widens an already-nonempty
/// cache re-derives its fill's starting state from that pair instead of the cache's
/// newest entry's own stored state — so a *separate* `peek::<U2>` call made after
/// `peek::<U1>` (nothing consumed in between, so committed state is unchanged) re-fills
/// token 2 as if token 1 had never been scanned. Consuming then adopts each entry's own
/// (wrongly resumed) state, so the by-value limiter's committed count silently falls
/// behind the true number of tokens consumed. This is the by-value counterpart to
/// `ProbeLimiter`'s `Rc`-shared tally: a shared tally can never go "missing" the way
/// committed STATE can, so it cannot see this class of bug.
#[test]
fn widen_then_drain_repeated_bypasses_the_by_value_limit() {
  use generic_arraydeque::typenum::{U1, U2, U3};

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6 7",
    TokenLimiter::with_limitation(2),
    cache,
  );

  let mut consumed = 0usize;
  {
    let mut inp = input.as_ref(&mut emitter);
    // Rounds 0 and 1 each widen 1→2→3 and drain all three — every fill and every drain
    // succeeds, even though the limit is 2: the committed count never truly reaches 3.
    for round in 0..2 {
      assert_eq!(
        inp.peek::<U1>().map(|w| w.len()),
        Ok(1),
        "round {round}: U1 fill"
      );
      assert_eq!(
        inp.peek::<U2>().map(|w| w.len()),
        Ok(2),
        "round {round}: U2 fill"
      );
      assert_eq!(
        inp.peek::<U3>().map(|w| w.len()),
        Ok(3),
        "round {round}: U3 fill"
      );
      for i in 0..3 {
        match inp.next() {
          Ok(Some(_)) => consumed += 1,
          other => panic!("round {round} token {i}: expected a token, got {other:?}"),
        }
      }
    }
    // Round 2: the 7th scanned token finally trips — a single-shot fill session (unlike
    // the separate top-level calls above) threads state correctly within itself, so the
    // trip is exact once it is reached at all.
    assert_eq!(
      inp.peek::<U1>().map(|w| w.len()),
      Ok(0),
      "round 2: the trip empties the fill"
    );
    assert!(
      inp.next().unwrap().is_none(),
      "the boundary the trip latched stops next() at the same point"
    );
  }

  let limit_diags: usize = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();

  assert_eq!(
    consumed, 6,
    "pinned bug (issue #89): a limit of 2 should stop consumption at 2 tokens; the \
     widen-then-drain pattern instead lets 3x the configured budget through before the \
     limiter trips"
  );
  assert_eq!(
    limit_diags, 1,
    "pinned bug (issue #89): exactly one Limit diagnostic fires — three rounds late"
  );
}

#[test]
fn failed_sync_through_leaves_no_diagnostics() {
  // A `sync_through` whose predicate never matches scans every valid token to EOF,
  // diagnosing each as unexpected, then takes the no-match EOF exit — which commits
  // nothing: the cursor stays at the pre-call anchor so the caller can fall back from
  // the original position. Diagnostics travel with progress, so a path that commits
  // nothing leaves no trace; the tokens the caller then consumes normally must carry
  // no stale unexpected-token noise.
  //   1 2 3   (high limit: the scan reaches EOF and never trips)
  use crate::span::SimpleSpan;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Pre-call anchor: nothing consumed.
    assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "pre-call span anchor");
    assert_eq!(inp.state().tokens(), 0, "pre-call token count");

    // Never matches: scans `1`, `2`, `3`, reaches EOF with no target.
    assert!(
      inp.sync_through(|_| false, || None).unwrap().is_none(),
      "the no-match scan to EOF yields None"
    );

    // The no-match EOF path commits nothing: cursor/span/state stay at the pre-call
    // anchor so the caller can fall back from the original position.
    assert_eq!(
      inp.span(),
      &SimpleSpan::new(0, 0),
      "span stays at the pre-call anchor — the failed sync commits no progress"
    );
    assert_eq!(
      inp.state().tokens(),
      0,
      "state stays at the pre-call count — the failed sync commits no progress"
    );

    // A subsequent drain consumes every token normally.
    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5)
    ],
    "the drain consumes the full token sequence normally"
  );

  // The failed sync left no diagnostics, and a normal drain of valid tokens emits
  // none. At HEAD the failed sync retained one unexpected-token diagnostic per scanned
  // token (three) — the stale, misleading noise this fix removes.
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 0,
    "a failed sync_through leaves no diagnostics behind"
  );
}

#[test]
fn successful_sync_through_retains_skipped_token_diagnostics() {
  // The commit side of the rule: a `sync_through` that DOES match commits through the
  // target, so the unexpected-token diagnostics it emitted for the tokens it skipped on
  // the way persist — they describe real, committed progress.
  //   1 2 3   (match the third scanned token; `1` and `2` are skipped and diagnosed)
  use crate::span::SimpleSpan;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  {
    let mut inp = input.as_ref(&mut emitter);

    // Match only the third scanned token (`3`); skip `1` and `2`, diagnosing each.
    let mut seen = 0;
    let matched = inp
      .sync_through(
        |_| {
          seen += 1;
          seen == 3
        },
        || None,
      )
      .unwrap();
    assert_eq!(
      matched.map(|t| *t.span_ref()),
      Some(SimpleSpan::new(4, 5)),
      "the matching token `3` is consumed and returned"
    );
    // The match commits through the skipped prefix: the cursor advances to the end of
    // `3`, and the two skipped tokens' diagnostics describe that committed progress.
    assert_eq!(inp.span(), &SimpleSpan::new(4, 5), "committed at the match");
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 2,
    "the skipped `1` and `2` stay diagnosed — the match committed through them"
  );
}

#[test]
fn failed_sync_through_reemits_scanned_lexer_error_once() {
  use crate::span::SimpleSpan;
  // A `sync_through` whose predicate never matches scans a region containing a lexer
  // error (`@`) to EOF. Crossing `@` emits it and lifts the dedup watermark past it; the
  // no-match EOF path commits nothing, so it unwinds this call's emissions AND restores
  // the watermark to its entry value. The error is therefore neither retained (the path
  // committed nothing) nor lost: the genuine consume that follows re-crosses `@` and
  // re-emits it exactly once. Without the watermark restore the rewound error would stay
  // watermark-covered and be silently deduplicated away on the re-scan (emitted zero
  // times); at HEAD the failed sync instead retains it plus two stale unexpected tokens.
  //   1 @ 2   (`@` is a lexer error spanning [2, 3); high limit so no trip)
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    // Never matches: skips `1`, crosses `@` (emitting the lexer error and lifting the
    // watermark past it), skips `2`, reaches EOF. The no-match EOF path unwinds every
    // emission and restores the watermark.
    assert!(
      inp.sync_through(|_| false, || None).unwrap().is_none(),
      "the no-match scan to EOF yields None"
    );

    // With the watermark restored, the genuine consume re-crosses `@` and re-emits.
    while inp.next().unwrap().is_some() {}
  }

  // Exactly the one `@` lexer error is retained: the failed sync left no trace, and the
  // genuine consume re-emitted the error exactly once.
  let at = SimpleSpan::new(2, 3);
  assert_eq!(
    emitter
      .errors()
      .get(&at)
      .map(|group| group.len())
      .unwrap_or(0),
    1,
    "the scanned-past lexer error re-emits exactly once on the genuine consume"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "only the re-emitted lexer error is retained — no stale unexpected-token noise"
  );
}

#[test]
fn failed_sync_through_then_peek_leaves_no_diagnostics_and_position() {
  // The peek-variant sibling of `failed_sync_through_leaves_no_diagnostics`: a
  // `sync_through_then_peek` whose predicate never matches scans every valid token to EOF,
  // diagnosing each as unexpected, then takes the no-match EOF exit. That exit commits
  // nothing — the cursor stays at the pre-call anchor, the peek is empty, and the failed
  // scan's diagnostics are unwound — so a caller using the peek variant for recovery or
  // lookahead keeps the original position and carries no stale noise.
  //   1 2 3   (high limit: the scan reaches EOF and never trips)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U1;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Pre-call anchor: nothing consumed.
    assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "pre-call span anchor");
    assert_eq!(inp.state().tokens(), 0, "pre-call token count");

    // Never matches: scans `1`, `2`, `3`, reaches EOF with no target.
    let (matched, peeked) = inp
      .sync_through_then_peek::<_, _, U1>(|_| false, || None)
      .unwrap();
    assert!(
      matched.is_none(),
      "the no-match scan to EOF yields no token"
    );
    assert!(
      peeked.is_empty(),
      "the no-match scan to EOF yields an empty peek"
    );
    // `peeked` borrows `inp` for its lifetime; release it before reusing `inp`.
    drop(peeked);

    // The no-match EOF path commits nothing: cursor/span/state stay at the pre-call anchor
    // so the caller can fall back from the original position. At HEAD this loop instead
    // advanced span/state to the lexer EOF.
    assert_eq!(
      inp.span(),
      &SimpleSpan::new(0, 0),
      "span stays at the pre-call anchor — the failed peek-sync commits no progress"
    );
    assert_eq!(
      inp.state().tokens(),
      0,
      "state stays at the pre-call count — the failed peek-sync commits no progress"
    );

    // A subsequent drain consumes every token normally.
    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5)
    ],
    "the drain consumes the full token sequence normally"
  );

  // The failed peek-sync left no diagnostics, and a normal drain of valid tokens emits
  // none. At HEAD the failed peek-sync retained one unexpected-token diagnostic per scanned
  // token (three) — the stale, misleading noise this fix removes.
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 0,
    "a failed sync_through_then_peek leaves no diagnostics behind"
  );
}

#[test]
fn failed_sync_through_then_peek_reemits_crossed_lexer_error_once() {
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U1;
  // The peek-variant sibling of `failed_sync_through_reemits_scanned_lexer_error_once`: a
  // `sync_through_then_peek` whose predicate never matches scans a region containing a lexer
  // error (`@`) to EOF. Crossing `@` emits it and lifts the dedup watermark past it; the
  // no-match EOF path commits nothing, so it unwinds this call's emissions AND restores the
  // watermark to its entry value. The error is therefore neither retained (the path
  // committed nothing) nor lost: the genuine consume that follows re-crosses `@` and
  // re-emits it exactly once. Without the watermark restore the rewound error would stay
  // watermark-covered and be silently deduplicated away on the re-scan (emitted zero times);
  // at HEAD the failed peek-sync instead retains it plus two stale unexpected tokens.
  //   1 @ 2   (`@` is a lexer error spanning [2, 3); high limit so no trip)
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    // Never matches: skips `1`, crosses `@` (emitting the lexer error and lifting the
    // watermark past it), skips `2`, reaches EOF. The no-match EOF path unwinds every
    // emission and restores the watermark.
    let (matched, peeked) = inp
      .sync_through_then_peek::<_, _, U1>(|_| false, || None)
      .unwrap();
    assert!(
      matched.is_none(),
      "the no-match scan to EOF yields no token"
    );
    assert!(
      peeked.is_empty(),
      "the no-match scan to EOF yields an empty peek"
    );
    drop(peeked);

    // With the watermark restored, the genuine consume re-crosses `@` and re-emits.
    while inp.next().unwrap().is_some() {}
  }

  // Exactly the one `@` lexer error is retained: the failed peek-sync left no trace, and the
  // genuine consume re-emitted the error exactly once.
  let at = SimpleSpan::new(2, 3);
  assert_eq!(
    emitter
      .errors()
      .get(&at)
      .map(|group| group.len())
      .unwrap_or(0),
    1,
    "the scanned-past lexer error re-emits exactly once on the genuine consume"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "only the re-emitted lexer error is retained — no stale unexpected-token noise"
  );
}

#[test]
fn successful_sync_through_then_peek_retains_skipped_token_diagnostics() {
  // The commit side of the rule for the peek variant: a `sync_through_then_peek` that DOES
  // match commits through the target, so the unexpected-token diagnostics it emitted for the
  // tokens it skipped on the way persist — they describe real, committed progress. This pins
  // the no-trace change to the failure path only.
  //   1 2 3   (match the third scanned token; `1` and `2` are skipped and diagnosed)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U1;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  {
    let mut inp = input.as_ref(&mut emitter);

    // Match only the third scanned token (`3`); skip `1` and `2`, diagnosing each.
    let mut seen = 0;
    let (matched, peeked) = inp
      .sync_through_then_peek::<_, _, U1>(
        |_| {
          seen += 1;
          seen == 3
        },
        || None,
      )
      .unwrap();
    assert_eq!(
      matched.map(|t| *t.span_ref()),
      Some(SimpleSpan::new(4, 5)),
      "the matching token `3` is consumed and returned"
    );
    assert!(
      peeked.is_empty(),
      "`3` is the last token, so the peek after the match is empty"
    );
    drop(peeked);
    // The match commits through the skipped prefix: the cursor advances to the end of `3`,
    // and the two skipped tokens' diagnostics describe that committed progress.
    assert_eq!(inp.span(), &SimpleSpan::new(4, 5), "committed at the match");
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 2,
    "the skipped `1` and `2` stay diagnosed — the match committed through them"
  );
}

#[test]
fn failed_sync_through_with_prefilled_cache_leaves_no_trace() {
  // The finding's core case. A caller peeks lookahead (filling the cache), then a
  // `sync_through` whose predicate never matches DRAINS that cached prefix — advancing
  // span/state and diagnosing each drained token — before scanning the rest to EOF. The
  // no-match EOF exit commits nothing, so it must rewind the WHOLE call, the drained
  // prefix included: cursor/span/state return to the pre-call anchor and every drain
  // diagnostic is unwound. The formerly-cached tokens were popped, not restored, so a
  // later drain re-lexes them (by the `Lexer` determinism contract) and yields the full
  // faithful stream with no noise.
  //
  // At HEAD the snapshot was taken AFTER the drain, so the drain's span/state advance and
  // its unexpected-token diagnostics survived the failed call: the cursor ended at EOF,
  // three stale diagnostics remained, and the drained tokens were lost to the committed
  // position — the regression this pins.
  //   1 2 3   (high limit: the scan reaches EOF and never trips)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Prefill the cache with all three tokens; peeking commits no progress, so the
    // pre-call anchor is still the origin and the cursor sits at the cache front.
    let _ = inp.peek::<U3>().unwrap();
    let pre_span = *inp.span();
    let pre_tokens = inp.state().tokens();
    let pre_cursor = *inp.cursor().as_inner();
    assert_eq!(pre_span, SimpleSpan::new(0, 0), "pre-call span anchor");
    assert_eq!(pre_tokens, 0, "peek advances no committed state");
    assert_eq!(pre_cursor, 0, "pre-call cursor sits at the cache front");

    // Never matches: drains the cached `1`, `2`, `3`, reaches EOF with no target.
    assert!(
      inp.sync_through(|_| false, || None).unwrap().is_none(),
      "the no-match scan to EOF yields None"
    );

    // The failed call restored the FULL pre-call state, the drained prefix included.
    assert_eq!(
      *inp.span(),
      pre_span,
      "span restored to the pre-call anchor"
    );
    assert_eq!(
      inp.state().tokens(),
      pre_tokens,
      "state restored to the pre-call count"
    );
    assert_eq!(
      *inp.cursor().as_inner(),
      pre_cursor,
      "cursor restored to the pre-call position, not stranded at EOF"
    );

    // A subsequent drain re-lexes the formerly-cached tokens in faithful order.
    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5)
    ],
    "the drain re-lexes every formerly-cached token in faithful order"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 0,
    "a failed sync_through across a drained cache leaves no diagnostics behind"
  );
}

#[test]
fn failed_sync_through_then_peek_with_prefilled_cache_leaves_no_trace() {
  // The `sync_through_then_peek` twin of `failed_sync_through_with_prefilled_cache_leaves_no_trace`:
  // the peek variant's loop must widen the same way. Prefill the cache, then a never-matching
  // `sync_through_then_peek` drains the cached prefix and scans to EOF; the no-match EOF exit
  // restores the full pre-call state, unwinds the drain's diagnostics, and returns an empty
  // peek. A later drain re-lexes the formerly-cached tokens faithfully with no noise.
  //   1 2 3   (high limit: the scan reaches EOF and never trips)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::{U1, U3};

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    let _ = inp.peek::<U3>().unwrap();
    let pre_span = *inp.span();
    let pre_tokens = inp.state().tokens();
    let pre_cursor = *inp.cursor().as_inner();
    assert_eq!(pre_cursor, 0, "pre-call cursor sits at the cache front");

    let (matched, peeked) = inp
      .sync_through_then_peek::<_, _, U1>(|_| false, || None)
      .unwrap();
    assert!(
      matched.is_none(),
      "the no-match scan to EOF yields no token"
    );
    assert!(
      peeked.is_empty(),
      "the no-match scan to EOF yields an empty peek"
    );
    // `peeked` borrows `inp` for its lifetime; release it before reusing `inp`.
    drop(peeked);

    assert_eq!(
      *inp.span(),
      pre_span,
      "span restored to the pre-call anchor"
    );
    assert_eq!(
      inp.state().tokens(),
      pre_tokens,
      "state restored to the pre-call count"
    );
    assert_eq!(
      *inp.cursor().as_inner(),
      pre_cursor,
      "cursor restored to the pre-call position, not stranded at EOF"
    );

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5)
    ],
    "the drain re-lexes every formerly-cached token in faithful order"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 0,
    "a failed sync_through_then_peek across a drained cache leaves no diagnostics behind"
  );
}

#[test]
fn successful_sync_through_after_cache_drain_commits_and_persists() {
  // The scoping guard: the no-trace widening touches ONLY the no-match-to-EOF failure
  // path. A `sync_through` that prefills the cache, drains a non-matching prefix (from
  // cache AND by scan), then MATCHES a token beyond that prefix must still commit — the
  // drained prefix's diagnostics persist because the match made real progress through
  // them. Pinning this keeps the failure-path rewind from leaking into the success path.
  //   1 2 3   (prefill `1`; match the third scanned token — `1` drained from cache, `2`
  //            scanned — so the match lies beyond the cached prefix)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U1;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  {
    let mut inp = input.as_ref(&mut emitter);

    // Prefill only the first token, so the match (`3`) lies beyond the cached prefix.
    let _ = inp.peek::<U1>().unwrap();

    let mut seen = 0;
    let matched = inp
      .sync_through(
        |_| {
          seen += 1;
          seen == 3
        },
        || None,
      )
      .unwrap();
    assert_eq!(
      matched.map(|t| *t.span_ref()),
      Some(SimpleSpan::new(4, 5)),
      "the matching token `3` is consumed and returned"
    );
    // The match commits through the whole diagnosed prefix — the `1` drained from cache
    // and the `2` scanned — so the cursor advances to the end of `3`.
    assert_eq!(inp.span(), &SimpleSpan::new(4, 5), "committed at the match");
    assert_eq!(
      inp.state().tokens(),
      3,
      "committed state counts the whole diagnosed prefix through the match"
    );
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 2,
    "the drained `1` and scanned `2` stay diagnosed — the match committed through them"
  );
}

#[test]
fn sync_through_over_a_prefilled_cache_evaluates_the_predicate_once() {
  // The single-evaluation law: a user predicate is an `FnMut`, so a second call about the same
  // token is observable — it can count, log, allocate, and it is free to answer differently. The
  // cache-drain prologue already decides at the cached match, so `sync_through` acts on THAT
  // decision instead of re-deriving it: every token the sync examines is tested exactly once.
  //
  // A re-test that believed a second, different answer skipped the cached match, dropped into
  // the uncached scanner with a NON-EMPTY cache, and reported the call as a failed sync
  // (`None`) with the drained prefix already gone from the stream.
  //   1 2 3 4 5   (prefill `1 2 3`; the predicate matches the SECOND token it examines)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  let rest: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);
    let _ = inp.peek::<U3>().unwrap();

    // Stateful by construction: it counts its calls, so asking twice about `2` would answer
    // `false` the second time.
    let mut calls = 0usize;
    let matched = inp
      .sync_through(
        |_| {
          calls += 1;
          calls == 2
        },
        || None,
      )
      .unwrap();

    assert_eq!(
      matched.map(|t| *t.span_ref()),
      Some(SimpleSpan::new(2, 3)),
      "the cached `2` matched on its only examination and is consumed"
    );
    assert_eq!(
      calls, 2,
      "exactly the two examined tokens — the drained `1` and the matched `2` — each tested once"
    );
    assert_eq!(inp.span(), &SimpleSpan::new(2, 3), "committed at the match");
    assert_eq!(
      *inp.cursor().as_inner(),
      4,
      "the cursor sits at the token after the match"
    );

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    rest,
    std::vec![
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
      SimpleSpan::new(8, 9)
    ],
    "the stream resumes at the token after the match — nothing cached was skipped"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "one diagnostic for the drained `1` — the match committed through it"
  );
}

#[test]
fn sync_through_then_peek_over_a_prefilled_cache_evaluates_the_predicate_once() {
  // The peek variant carries the same decision out of the drain. The predicate accepts the
  // cached `2` on its only examination, so the match is consumed and the peek starts AFTER it.
  //
  // Re-testing the front and believing a second, different answer returned `None` — the failed
  // sync signal — while the drained `1` had already been consumed and its diagnostic COMMITTED
  // (this variant's cached exit rewinds nothing), and the peek then handed back the cached
  // tokens the caller was told nothing had matched in. A `None` return must leave no skipped
  // cached token and no committed diagnostic behind it.
  //   1 2 3 4 5   (prefill `1 2 3`; the predicate matches the SECOND token it examines)
  use crate::{cache::PeekedTokenExt, span::SimpleSpan};
  use generic_arraydeque::typenum::{U2, U3};

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  {
    let mut inp = input.as_ref(&mut emitter);
    let _ = inp.peek::<U3>().unwrap();

    let mut calls = 0usize;
    let (matched, peeked) = inp
      .sync_through_then_peek::<_, _, U2>(
        |_| {
          calls += 1;
          calls == 2
        },
        || None,
      )
      .unwrap();

    assert_eq!(
      matched.map(|t| *t.span_ref()),
      Some(SimpleSpan::new(2, 3)),
      "the cached `2` matched on its only examination and is consumed"
    );
    assert_eq!(
      calls, 2,
      "exactly the two examined tokens — the drained `1` and the matched `2` — each tested once"
    );
    assert_eq!(peeked.len(), 2, "the window is filled from after the match");
    assert_eq!(
      *peeked[0].span(),
      SimpleSpan::new(4, 5),
      "the peek starts at the token AFTER the match, never at the match itself"
    );
    assert_eq!(*peeked[1].span(), SimpleSpan::new(6, 7));
    // `peeked` borrows `inp` for its lifetime; release it before reusing `inp`.
    drop(peeked);

    assert_eq!(inp.span(), &SimpleSpan::new(2, 3), "committed at the match");
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "one diagnostic for the drained `1` — the match committed through it"
  );
}

#[test]
fn sync_through_never_scans_past_a_cached_match() {
  // The scanner's precondition, made structural: `sync_with` lexes from `offset()` — the end of
  // the LAST cached token — so it may only run once the drain has emptied the cache. Believing a
  // re-test's second answer entered it with `2` and `3` still cached: it lexed straight PAST
  // them, matched `4`, and committed there while the stream still owed the caller the two cached
  // tokens — a position and a cache that cannot both be right, with the drained `1` lost. The
  // predicate accepts the cached `2` on its only examination, so the scanner never runs at all.
  //   1 2 3 4 5   (prefill `1 2 3`; the predicate accepts the 2nd AND the 4th token it examines)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    TokenLimiter::with_limitation(usize::MAX),
    cache,
  );

  let rest: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);
    let _ = inp.peek::<U3>().unwrap();

    // Accepts the 2nd examined token (the cached `2`) — and would accept a 4th, which only a
    // scan past the live cache could ever reach.
    let mut calls = 0usize;
    let matched = inp
      .sync_through(
        |_| {
          calls += 1;
          calls == 2 || calls == 4
        },
        || None,
      )
      .unwrap();

    assert_eq!(
      matched.map(|t| *t.span_ref()),
      Some(SimpleSpan::new(2, 3)),
      "the cached match is returned — never a token lexed from beyond the live cache"
    );
    assert_eq!(calls, 2, "the scan stopped at the cached match");
    assert_eq!(inp.span(), &SimpleSpan::new(2, 3), "committed at the match");

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    rest,
    std::vec![
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
      SimpleSpan::new(8, 9)
    ],
    "the stream resumes in order after the match — the cache and the position agree"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(total, 1, "one diagnostic for the drained `1`");
}

#[test]
fn sync_to_returning_a_cached_match_is_not_a_cache_push() {
  // A `to`-shaped match leaves the sync token unconsumed AT THE CACHE FRONT. The scanner takes
  // every token — cached or lexed — out of the cache/lexer to decide it, so settling a CACHED match
  // puts that token straight back into the slot it left: the cache is then bit-for-bit what it was,
  // and its push history must not move.
  //
  // If the put-back were counted as a push, `restore` would compute one post-save entry too many
  // and drop the LAST prefetched token off the back — lookahead the caller had already paid to lex,
  // evicted by a rollback that PREDATES the sync but POSTDATES the peek. (That over-drop is not
  // cosmetic: `nested_restore_with_shared_limiter_no_spurious_poison` shows the re-lex it forces
  // can spend a scan the limiter's budget did not have, spuriously poisoning the input.)
  //   ; 1 2 3   (the sync point is the very first token — a zero-skip match, straight from cache)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  let mut input = bal_input("; 1 2 3", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);
    let _ = inp.peek::<U3>().unwrap(); // prefill `;`, `1`, `2`
    assert_eq!(inp.cache().len(), 3, "the peek staged three tokens");

    let ckp = inp.save();
    let matched = inp
      .sync_to(|t| matches!(t.data(), BalTok::Semi), || None)
      .unwrap()
      .map(|t| *t.span());
    assert_eq!(
      matched,
      Some(SimpleSpan::new(0, 1)),
      "the cached `;` is the sync point"
    );
    inp.restore(ckp);

    // The rollback returns to the moment after the peek, so ALL THREE prefetched tokens must still
    // be there: the sync popped one and put it straight back, which is a no-op on the cache.
    assert_eq!(
      inp.cache().len(),
      3,
      "a rollback over a sync that only put its cached match back must keep the whole prefetch"
    );
    assert_eq!(
      *inp.offset(),
      5,
      "the lex frontier still ends at the last prefetched token"
    );

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
    ],
    "the full faithful stream drains"
  );
}

#[test]
fn sync_balanced_staging_a_lexed_match_is_a_cache_push() {
  // The other half of the same rule. A match the scanner LEXED is left unconsumed at the cache
  // front too — but that is a NEW cache entry, exactly the one a peek would have made, so its push
  // IS recorded and a checkpoint saved before the call drops it on restore, like any other
  // speculative fill. `sync_balanced` is the sharp case: unlike `sync_to` it takes no peek
  // afterwards, so the entry it stages is the only one in the cache.
  //
  // Failing to record it would retain, across a rollback, a token lexed on the abandoned
  // continuation — the exact stale-cache hazard `restore_unchecked` drops post-save entries to
  // prevent.
  //   ; 1 2   (a zero-skip balanced match, lexed from an empty cache)
  use crate::span::SimpleSpan;

  let mut input = bal_input("; 1 2", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);
    assert!(inp.cache().is_empty(), "nothing is prefetched");

    let ckp = inp.save();
    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
      .unwrap()
      .expect("the `;` is the sync point");
    assert_eq!(hole.skipped(), 0, "the sync point was the very next token");
    assert_eq!(
      hole.span(),
      SimpleSpan::new(0, 0),
      "the zero-skip hole sits at the resume position — the match's own start"
    );
    assert_eq!(
      inp.cache().len(),
      1,
      "the lexed match is left unconsumed at the cache front"
    );

    inp.restore(ckp);
    assert!(
      inp.cache().is_empty(),
      "a rollback must drop the match the abandoned scan lexed into the cache"
    );
    assert_eq!(*inp.offset(), 0, "the lex frontier returns to the save");

    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5),
    ],
    "the full faithful stream drains after the rollback"
  );
}

#[test]
fn failed_sync_through_with_prefilled_cache_reemits_crossed_error_once() {
  // The watermark leg with a prefilled cache. Peek stages a token BEFORE a lexer error
  // (`@`), then a never-matching `sync_through` drains that cached token and scans across
  // `@` to EOF — emitting the lexer error and lifting the dedup watermark past it. The
  // no-match EOF exit commits nothing, so it unwinds every emission (the drain's
  // unexpected token, the `@` error, the trailing unexpected tokens) AND restores the
  // watermark to its pre-call value. The failed call leaves no trace, so the genuine
  // consume that follows re-lexes the whole region — the formerly-cached token included —
  // and reports `@` exactly once.
  //
  // At HEAD the drained token was stranded past the committed cursor (lost to the genuine
  // consume) and its unexpected-token diagnostic survived the failed call — two defects
  // this pins.
  //   1 @ 2 3   (`@` is a lexer error spanning [2, 3); high limit so no trip)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U1;

  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );

  let drained: Vec<SimpleSpan> = {
    let mut inp = input.as_ref(&mut emitter);

    // Prefill `1` only; `@` lies just beyond the cached prefix.
    let _ = inp.peek::<U1>().unwrap();

    // Never matches: drains `1`, crosses `@` (emitting it and lifting the watermark),
    // skips `2`, `3`, reaches EOF. The no-match EOF exit unwinds all of it and restores
    // the watermark to its pre-call value.
    assert!(
      inp.sync_through(|_| false, || None).unwrap().is_none(),
      "the no-match scan to EOF yields None"
    );

    // The genuine consume re-lexes the whole region, `1` included, re-emitting `@` once.
    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };

  assert_eq!(
    drained,
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7)
    ],
    "the genuine consume re-lexes every token, the formerly-cached `1` included"
  );
  let at = SimpleSpan::new(2, 3);
  assert_eq!(
    emitter
      .errors()
      .get(&at)
      .map(|group| group.len())
      .unwrap_or(0),
    1,
    "`@` is reported exactly once — on the genuine consume, not the unwound failed sync"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 1,
    "only the re-emitted `@` remains — no stale unexpected-token noise from the failed sync"
  );
}

// ── Last-in, first-out contract: the debug witness ─────────────────────────────
//
// Restoring a checkpoint invalidates every checkpoint saved after it, so restores
// must be LIFO. Debug builds track the live checkpoints exactly and panic on any
// out-of-order restore, and on any restore into a foreign input. These tests pin
// that witness; the LIFO-legal tests above pin the pure-copy behavior it protects.

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "non-LIFO checkpoint restore")]
fn alias_interleave_stale_restore_detected() {
  // THE witness test. After restoring the older A (which invalidates B), one
  // committed emission regrows the emission log to exactly B's saved mark, so a
  // length-based validity check (`B.mark <= emitter.len()`) would pass — yet B's
  // lineage is gone. The live-checkpoint id stack still rejects the restore.
  //   1 2 3 4 5 6   (limit 2 → the 3rd scanned token trips, emitting the diagnostic)
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    ProbeLimiter::with_limit(2),
    cache,
  );

  use generic_arraydeque::typenum::U6;
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save(); // older, clean, mark 0
  let _ = inp.peek::<U6>().unwrap(); // trips: emits the limit diagnostic (mark → 1)
  let b = inp.save(); // younger, mark 1
  inp.restore(a); // invalidates b; the emitter rewinds back to mark 0

  // One committed emission regrows the log to length 1 == B's saved mark.
  while inp.next().unwrap().is_some() {}

  inp.restore(b); // ✗ non-LIFO — only the id stack catches this
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "non-LIFO checkpoint restore")]
fn stale_poisoned_restore_never_exposes_tokens_past_saved_boundary() {
  // Clean older A, an overflow trip that latches an early boundary, poisoned younger
  // B, restore A (un-poisoning and invalidating B), consume through to a later
  // committed trip, then restore B. B's boundary belongs to a lineage restoring A
  // destroyed; the witness rejects the restore rather than exposing tokens between
  // the two frontiers.
  //   1 2 3 4 5 6   (limit 5 → the 6th scanned token trips; U6 window > U3 cache)
  use generic_arraydeque::typenum::U6;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(5),
    cache,
  );

  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save(); // clean older
  let _ = inp.peek::<U6>().unwrap(); // overflow trip: early boundary
  assert!(inp.is_poisoned(), "the overflow trip must latch poison");
  let b = inp.save(); // poisoned younger
  inp.restore(a); // invalidates b, un-poisons
  while inp.next().unwrap().is_some() {} // committed path re-lexes to a later trip

  inp.restore(b); // ✗ non-LIFO — debug panic
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "checkpoint restored into a foreign input")]
fn restore_with_foreign_checkpoint_rejected_in_debug() {
  // A checkpoint may only be restored into the input that created it.
  let cache1 = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let cache2 = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut em1 = Silent::<ProbeErr>::new();
  let mut em2 = Silent::<ProbeErr>::new();
  let mut in1 = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache1,
  );
  let mut in2 = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache2,
  );

  let foreign = {
    let mut r1 = in1.as_ref(&mut em1);
    r1.save()
  };
  let mut r2 = in2.as_ref(&mut em2);
  r2.restore(foreign); // ✗ created by a different input — debug panic
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "checkpoint restored into a foreign input")]
fn restore_with_clone_sibling_checkpoint_rejected_in_debug() {
  // A clone is a NEW input: a checkpoint from the original may not be restored into
  // the clone — their checkpoints must never cross.
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut em1 = Silent::<ProbeErr>::new();
  let mut em2 = Silent::<ProbeErr>::new();
  let mut original = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  let mut sibling = original.clone();

  let from_original = {
    let mut r = original.as_ref(&mut em1);
    r.save()
  };
  let mut r2 = sibling.as_ref(&mut em2);
  r2.restore(from_original); // ✗ the clone is a foreign input — debug panic
}

// ── Pure copy replays the saved lineage exactly (LIFO-legal) ───────────────────

#[test]
fn twin_checkpoint_restore_after_partial_drain_replays_identically() {
  use crate::span::SimpleSpan;
  // Two checkpoints at the same position. Draining one token then restoring the
  // younger, re-draining, then restoring the elder and re-draining yields identical
  // span sequences both times — pure copy replays the lineage exactly.
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let elder = inp.save();
  let younger = inp.save(); // same position as elder

  // Drain one token, then restore the younger and replay the full stream.
  let _ = inp.next().unwrap().expect("first token");
  inp.restore(younger);
  let mut seq_young = Vec::new();
  while let Some(tok) = inp.next().unwrap() {
    seq_young.push(*tok.span_ref());
  }

  // Restore the elder (still LIFO: the younger has been consumed) and replay again.
  inp.restore(elder);
  let mut seq_old = Vec::new();
  while let Some(tok) = inp.next().unwrap() {
    seq_old.push(*tok.span_ref());
  }

  assert_eq!(
    seq_young, seq_old,
    "both restores replay the same lineage identically"
  );
  assert_eq!(
    seq_young,
    vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
    ]
  );
}

#[test]
fn save_exactly_at_boundary_restores_empty_stream() {
  // Trip the limit by draining past it so the cursor sits exactly at the poison
  // boundary; save there; restore. Every scanner entry point yields its poisoned
  // outcome, the shared scan counter stays frozen, and the limit diagnostic is
  // retained exactly once.
  //   1 2 3 4 5 6   (limit 2 → the 3rd scanned token trips)
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let limiter = ProbeLimiter::with_limit(2);
  let scanned = limiter.counter();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    limiter,
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    // Drain to the trip: 1, 2, then the 3rd next() trips and latches at the cursor.
    assert!(inp.next().unwrap().is_some(), "first token");
    assert!(inp.next().unwrap().is_some(), "second token");
    assert!(inp.next().unwrap().is_none(), "trip latches to None");
    assert!(inp.is_poisoned(), "the trip latches the boundary");
    let frozen = scanned.get();

    let ckp = inp.save(); // saved with the cursor exactly at the boundary
    inp.restore(ckp); // LIFO restore of the poisoned checkpoint

    // Every scanner entry point returns its poisoned (empty) outcome, never rescanning.
    assert!(inp.next().unwrap().is_none(), "next() stays None");
    assert!(inp.peek_one().unwrap().is_none(), "peek stays empty");
    assert!(
      inp.try_expect(|_| true).unwrap().is_none(),
      "try_expect stays None"
    );
    assert!(
      inp.sync_to(|_| false, || None).unwrap().is_none(),
      "sync_to stays None"
    );
    assert!(
      inp.sync_through(|_| false, || None).unwrap().is_none(),
      "sync_through stays None"
    );
    inp.skip_while(|_| true).unwrap();

    assert_eq!(
      scanned.get(),
      frozen,
      "the scan counter stays frozen — nothing past the boundary is re-scanned"
    );
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(total, 1, "the limit diagnostic is retained exactly once");
}

#[test]
fn sink_emitter_trip_bounds_work_and_survives_restore() {
  // With a non-collecting (Silent) emitter the poison boundary is input-owned: it
  // survives an attempt-style rollback even though the emitter retains nothing to
  // derive it from, and no rescan ever crosses it.
  //   1 2 3 4 5 6   (limit 2 → the 3rd scanned token trips)
  let (mut input, scanned) = probe_input("1 2 3 4 5 6");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  // Trip the limit.
  assert!(inp.next().unwrap().is_some(), "first token");
  assert!(inp.next().unwrap().is_some(), "second token");
  assert!(inp.next().unwrap().is_none(), "trip latches to None");
  assert!(inp.is_poisoned(), "the trip latches the boundary");
  let frozen = scanned.get();

  // An attempt speculatively re-enters scanners, then declines and rolls back. The
  // boundary is checkpointed and copied back verbatim — the Silent emitter keeps no
  // log, so this proves the boundary is input-owned.
  let outcome = inp.attempt(|inp| {
    for _ in 0..5 {
      let _ = inp.next().unwrap();
    }
    None::<()>
  });
  assert!(outcome.is_none(), "the attempt declines and rolls back");
  assert!(
    inp.is_poisoned(),
    "the boundary survives the attempt rollback (input-owned)"
  );
  assert_eq!(
    scanned.get(),
    frozen,
    "no unbounded rescan — the shared scan counter stays frozen"
  );
}

#[test]
fn attempt_backtrack_over_trip_reemits_diagnostic_exactly_once() {
  // Inside an attempt, an overflow peek trips the limit (emitting the diagnostic);
  // the closure declines, rolling the speculative diagnostic back. The committed path
  // then re-reaches the trip and re-emits — exactly once in total, never zero.
  //   1 2 3 4 5 6   (limit 5 → the 6th scanned token trips; U6 window > U3 cache)
  use generic_arraydeque::typenum::U6;

  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(5),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    let outcome = inp.attempt(|inp| {
      let _ = inp.peek::<U6>(); // overflow trip: emits the limit diagnostic
      None::<()> // decline → rollback
    });
    assert!(outcome.is_none(), "the attempt declines and rolls back");
    assert!(
      !inp.is_poisoned(),
      "the rollback un-poisons and un-emits the speculative diagnostic"
    );

    // The committed path re-reaches the trip and re-emits.
    while inp.next().unwrap().is_some() {}
    assert!(inp.is_poisoned(), "the committed re-lex re-latches poison");
  }

  let errs: Vec<&ByValErr> = emitter.errors().values().flatten().collect();
  assert_eq!(
    errs.len(),
    1,
    "the limit diagnostic is emitted exactly once in total"
  );
  assert_eq!(*errs[0], ByValErr::Limit, "and it is the limit diagnostic");
}

#[test]
fn restore_after_interleaved_emissions_keeps_rewound_lexer_error_reemittable() {
  use crate::span::SimpleSpan;
  // A lexer error emitted interleaved with unexpected-token emissions (from sync_to),
  // then a restore of a checkpoint that predates them all. Pure copy returns the
  // watermark to its saved value, so the rewound lexer error re-emits exactly once
  // when the committed path re-reaches it — never zero times.
  //   1 @ 2   (`@` is a lexer error spanning [2, 3))
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    let a = inp.save(); // predates every emission

    // sync_to(never-match) skips `1` (an unexpected token), crosses `@` (a lexer
    // error, lifting the watermark past it), then skips `2` — interleaved emissions.
    assert!(inp.sync_to(|_| false, || None).unwrap().is_none());

    inp.restore(a); // LIFO: rolls the log back and the watermark to 0

    // The committed path re-crosses `@`; with the watermark restored, it re-emits.
    while inp.next().unwrap().is_some() {}
  }

  // Exactly the one `@` lexer error is retained — re-emitted once, never lost.
  let at = SimpleSpan::new(2, 3);
  assert_eq!(
    emitter
      .errors()
      .get(&at)
      .map(|group| group.len())
      .unwrap_or(0),
    1,
    "the rewound lexer error re-emits exactly once when re-reached"
  );
  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(total, 1, "only the re-emitted lexer error is retained");
}

#[test]
#[cfg(debug_assertions)]
fn property_random_lifo_scripts_stay_faithful_and_bounded() {
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::{U1, U2, U3};

  const SRC: &str = "1 @ 2 3 @ 4"; // Num tokens at 0,4,6,10; `@` errors at [2,3),[8,9)

  // Oracle: one fresh single pass over SRC.
  let oracle_tokens: Vec<SimpleSpan> = {
    let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
    let mut em = Verbose::<ProbeErr>::new();
    let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
      SRC,
      ProbeLimiter::with_limit(usize::MAX),
      cache,
    );
    let mut inp = input.as_ref(&mut em);
    let mut toks = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      toks.push(*t.span_ref());
    }
    toks
  };
  let oracle_next =
    |off: usize| -> Option<SimpleSpan> { oracle_tokens.iter().copied().find(|s| s.start() >= off) };
  // The `@` lexer errors a full pass would emit.
  let oracle_diags: Vec<SimpleSpan> = vec![SimpleSpan::new(2, 3), SimpleSpan::new(8, 9)];

  // Deterministic linear congruential generator (no external dev-deps).
  let mut rng: u64 = 0x0123_4567_89ab_cdef;
  let roll = |rng: &mut u64| -> u64 {
    *rng = rng
      .wrapping_mul(6364136223846793005)
      .wrapping_add(1442695040888963407);
    *rng >> 33
  };

  for _script in 0..200u32 {
    let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
    let limiter = ProbeLimiter::with_limit(usize::MAX);
    let scanned = limiter.counter();
    let mut em = Verbose::<ProbeErr>::new();
    let mut input =
      Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(SRC, limiter, cache);

    let num_ops = 8 + (roll(&mut rng) % 12) as usize; // 8..=19 ops
    {
      let mut inp = input.as_ref(&mut em);
      // Live checkpoints as a stack of (checkpoint, saved cursor offset).
      let mut live: Vec<(crate::input::Checkpoint<'_, '_, ProbeLexer<'_>>, usize)> = Vec::new();

      for _ in 0..num_ops {
        match roll(&mut rng) % 6 {
          0 => {
            // save
            let off = *inp.cursor().as_inner();
            live.push((inp.save(), off));
          }
          1 => {
            // drain k
            let k = 1 + (roll(&mut rng) % 3) as usize;
            for _ in 0..k {
              if inp.next().unwrap().is_none() {
                break;
              }
            }
          }
          2 => {
            // peek w
            match roll(&mut rng) % 3 {
              0 => {
                let _ = inp.peek::<U1>().unwrap();
              }
              1 => {
                let _ = inp.peek::<U2>().unwrap();
              }
              _ => {
                let _ = inp.peek::<U3>().unwrap();
              }
            }
          }
          3 => {
            // restore the most-recent live checkpoint (always LIFO), then verify the
            // next drained span matches a fresh parse of the same prefix.
            if let Some((ckp, off)) = live.pop() {
              inp.restore(ckp);
              let got = inp.next().unwrap().map(|t| *t.span_ref());
              assert_eq!(
                got,
                oracle_next(off),
                "after a restore to offset {off}, the next token must match a fresh parse"
              );
            }
          }
          4 => {
            // The exact class the cache-lineage hole lived in: prefill the cache, save on
            // top of it (so the checkpoint cursor equals the cache front and the rewind
            // takes its no-op branch), then peek across a lexer-error character so
            // post-error tokens are cached into the continuation this save may later
            // abandon. The checkpoint joins `live`, so a subsequent restore op exercises
            // the post-save truncation over a cache that straddles a rolled-back error.
            let _ = inp.peek::<U1>().unwrap();
            let off = *inp.cursor().as_inner();
            live.push((inp.save(), off));
            let _ = inp.peek::<U3>().unwrap();
          }
          _ => {
            // A self-contained NESTED last-in, first-out restore over a prefilled cache entry
            // — the (save, save, peek, restore, restore) shape the input-wide push count made
            // unsound. Prefill one token so BOTH saves' cursors equal the cache front, stack
            // two saves, widen the peek to cache post-save entries, then restore inner and
            // restore outer. The prefilled entry predates both saves, so the drained token must
            // match a fresh parse of the same prefix — the outer restore must not over-drop it
            // (a stale push count did, re-lexing a token whose scan side effects belonged to the
            // abandoned lineage). Both checkpoints are resolved here and never join `live`, so
            // the other ops' LIFO discipline is untouched.
            let _ = inp.peek::<U1>().unwrap();
            let off = *inp.cursor().as_inner();
            let outer = inp.save();
            let inner = inp.save();
            let _ = inp.peek::<U3>().unwrap();
            inp.restore(inner);
            inp.restore(outer);
            assert_eq!(
              inp.next().unwrap().map(|t| *t.span_ref()),
              oracle_next(off),
              "a nested LIFO restore must retain the pre-save cache entry (matches a fresh parse)"
            );
          }
        }
      }

      // Final commit drain to EOF: a full pass from the current position crosses every
      // remaining error region. A rolled-back error that a stale post-save cache entry let
      // a drain skip would re-lex to ZERO emissions here, so together with the soundness
      // checks below this pins exactly-once COMPLETENESS — the direction the hole broke.
      drop(live);
      while inp.next().unwrap().is_some() {}
    }

    // (a) total scans bounded by a generous linear budget.
    let budget = (num_ops + 1) * (oracle_tokens.len() + oracle_diags.len()) * 8;
    assert!(
      scanned.get() <= budget,
      "scans {} exceeded the linear budget {budget}",
      scanned.get()
    );

    // (b) every retained diagnostic span appears at most once and is a real error.
    for (span, group) in em.errors() {
      assert!(
        group.len() <= 1,
        "diagnostic span {span:?} retained more than once"
      );
      assert!(
        group.is_empty() || oracle_diags.contains(span),
        "retained an unexpected diagnostic span {span:?}"
      );
    }

    // (c) after the final full drain to EOF, every real error span is retained exactly
    // once. With (b) this is exactly-once completeness on the committed lineage: a stale
    // post-save cache entry that skipped a rolled-back error would drop it to zero here.
    for diag in &oracle_diags {
      assert_eq!(
        em.errors().get(diag).map(|g| g.len()).unwrap_or(0),
        1,
        "after a full final drain, the error at {diag:?} must be retained exactly once"
      );
    }
  }
}

// ── try_attempt: Result-shaped speculation ─────────────────────────────────────
//
// `try_attempt` is the fallible sibling of `attempt`: on `Ok` progress is kept, on
// `Err` the input rolls back exactly as `restore` would and the error is returned.
// The save/restore pair is closure-scoped, so it is LIFO by construction.

#[test]
fn try_attempt_ok_keeps_progress() {
  use crate::span::SimpleSpan;
  // On `Ok`, the closure's progress is kept and the value is passed through.
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let start = *inp.cursor().as_inner();
  let out: Result<i64, ()> = inp.try_attempt(|inp| {
    let _ = inp.next().unwrap().expect("first token");
    let _ = inp.next().unwrap().expect("second token");
    Ok(7)
  });
  assert_eq!(out, Ok(7), "the Ok value is returned");
  assert!(
    *inp.cursor().as_inner() > start,
    "progress is kept — the cursor advanced past the consumed tokens"
  );
  // The two consumes stuck, so the next token is the third.
  assert_eq!(
    *inp.next().unwrap().expect("third token").span_ref(),
    SimpleSpan::new(4, 5)
  );
}

#[test]
fn try_attempt_err_rolls_back_everything() {
  use crate::span::SimpleSpan;

  // ── position, span, lexer state, emission log, and the dedup watermark ─────────
  // "1 @ 2": crossing the malformed `@` inside the attempt emits its lexer error and
  // lifts the watermark. Returning `Err` must roll every one of those back.
  {
    let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
    let mut emitter = Verbose::<ByValErr>::new();
    let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
      "1 @ 2",
      TokenLimiter::with_limitation(usize::MAX),
      cache,
    );

    // Phase 1: the attempt consumes across `@` (emitting the lexer error), then
    // abandons. Position, span, and lexer state must all return to their saved values.
    {
      let mut inp = input.as_ref(&mut emitter);

      let cur0 = *inp.cursor().as_inner();
      let span0 = *inp.span();
      let tokens0 = inp.state().tokens();

      let out: Result<(), ()> = inp.try_attempt(|inp| {
        // Consume `1`, cross `@` (emits the lexer error, lifts the watermark),
        // consume `2`, then abandon the branch.
        while inp.next().unwrap().is_some() {}
        Err(())
      });
      assert_eq!(out, Err(()), "the error is returned to the caller");

      assert_eq!(*inp.cursor().as_inner(), cur0, "position rolled back");
      assert_eq!(*inp.span(), span0, "last-consumed span rolled back");
      assert_eq!(inp.state().tokens(), tokens0, "lexer state rolled back");
    }

    // The emission log was truncated by the rollback: nothing the attempt emitted
    // survives.
    let after_rollback: usize = emitter.errors().values().map(|g| g.len()).sum();
    assert_eq!(
      after_rollback, 0,
      "diagnostics emitted inside the attempt are rolled back (empty emission log)"
    );

    // Phase 2: the watermark rolled back too, so the committed path re-crosses `@`
    // and the rewound lexer error becomes re-emittable — exactly once.
    {
      let mut inp = input.as_ref(&mut emitter);
      while inp.next().unwrap().is_some() {}
    }
    let at = SimpleSpan::new(2, 3);
    assert_eq!(
      emitter.errors().get(&at).map(|g| g.len()).unwrap_or(0),
      1,
      "the rewound lexer error re-emits exactly once when re-reached"
    );
    let total: usize = emitter.errors().values().map(|g| g.len()).sum();
    assert_eq!(total, 1, "only the re-emitted lexer error is retained");
  }

  // ── the poison boundary, via a limit-trip variant ─────────────────────────────
  // An overflow peek inside the attempt trips the limiter (latching poison and
  // emitting the diagnostic); the `Err` rollback un-latches it, and the committed
  // path re-trips — the diagnostic surviving exactly once, never a diagnostic-less
  // latch.
  {
    use generic_arraydeque::typenum::U6;
    let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
    let mut emitter = Verbose::<ByValErr>::new();
    let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
      "1 2 3 4 5 6",
      TokenLimiter::with_limitation(5),
      cache,
    );
    {
      let mut inp = input.as_ref(&mut emitter);

      let out: Result<(), ()> = inp.try_attempt(|inp| {
        let _ = inp.peek::<U6>().unwrap(); // overflow trip: poison + diagnostic
        Err(())
      });
      assert_eq!(out, Err(()));
      assert!(
        !inp.is_poisoned(),
        "the Err rollback un-latches the speculative poison boundary"
      );

      // The committed path re-reaches the trip and re-latches.
      while inp.next().unwrap().is_some() {}
      assert!(inp.is_poisoned(), "the committed re-lex re-latches poison");
    }
    let total: usize = emitter.errors().values().map(|g| g.len()).sum();
    assert_eq!(
      total, 1,
      "the limit diagnostic is emitted exactly once in total"
    );
  }
}

#[test]
fn try_attempt_nested_lifo() {
  use crate::span::SimpleSpan;

  // A `try_attempt` nested inside an `attempt`: the inner `Err` rollback is fully
  // contained, and the outer keeps its own progress. The closure-scoped save/restore
  // pairs nest as a stack, so the LIFO witness never fires.
  {
    let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
    let mut emitter = Silent::<ProbeErr>::new();
    let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
      "1 2 3 4",
      ProbeLimiter::with_limit(usize::MAX),
      cache,
    );
    let mut inp = input.as_ref(&mut emitter);

    let out = inp.attempt(|inp| {
      let _ = inp.next().unwrap().expect("outer consumes 1");
      let inner: Result<(), ()> = inp.try_attempt(|inp| {
        let _ = inp.next().unwrap().expect("inner consumes 2");
        Err(()) // inner rolls back to just after 1
      });
      assert!(inner.is_err(), "the inner try_attempt returned Err");
      Some(()) // outer keeps its own progress (only 1 consumed)
    });
    assert!(out.is_some(), "the outer attempt kept progress");
    // The inner's consume of 2 was rolled back; the next token is 2.
    assert_eq!(
      *inp.next().unwrap().expect("token 2").span_ref(),
      SimpleSpan::new(2, 3)
    );
  }

  // The mirror image: an `attempt` nested inside a `try_attempt`.
  {
    let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
    let mut emitter = Silent::<ProbeErr>::new();
    let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
      "1 2 3 4",
      ProbeLimiter::with_limit(usize::MAX),
      cache,
    );
    let mut inp = input.as_ref(&mut emitter);

    let out: Result<(), ()> = inp.try_attempt(|inp| {
      let _ = inp.next().unwrap().expect("outer consumes 1");
      let inner = inp.attempt(|inp| {
        let _ = inp.next().unwrap().expect("inner consumes 2");
        None::<()> // inner rolls back to just after 1
      });
      assert!(inner.is_none(), "the inner attempt returned None");
      Ok(()) // outer keeps its own progress (only 1 consumed)
    });
    assert!(out.is_ok(), "the outer try_attempt kept progress");
    assert_eq!(
      *inp.next().unwrap().expect("token 2").span_ref(),
      SimpleSpan::new(2, 3)
    );
  }
}

// ── attempt / try_attempt: raw restore below the checkpoint panics AT THE RESTORE ─
//
// The closure receives `&mut InputRef` and can raw-restore to a checkpoint saved BEFORE the
// attempt began. `attempt` PINS its held checkpoint on entry, so such a restore — which would
// pop that pinned checkpoint off the live lineage — panics AT THE RESTORE, inside the closure,
// in every allocator build. A LIFO-clean raw pair taken and released above the attempt's own
// checkpoint is unaffected. (The former detect-at-use behavior — a stale panic in the decline
// arm — is now an unreachable backstop in allocator builds.)

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn attempt_inner_raw_restore_below_checkpoint_panics_at_restore() {
  // Converted from `attempt_rollback_after_inner_raw_restore_below_checkpoint`. Inside the
  // attempt, raw-restore to a checkpoint older than the attempt's own. The attempt pins its
  // checkpoint on entry, so the raw restore panics AT THE RESTORE. At HEAD the raw restore
  // succeeded and the decline's rollback arm panicked as stale ("attempt checkpoint is
  // stale"); post-fix the pinned restore panics first.
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save(); // raw checkpoint, below the attempt's checkpoint
  let _ = inp.next().unwrap().expect("consume 1");

  let _out: Option<()> = inp.attempt(|inp| {
    let _ = inp.next().unwrap().expect("consume 2");
    inp.restore(a); // POST-FIX: panics here — restoring A would pop the attempt's pinned checkpoint
    None
  });
}

#[test]
#[should_panic(
  expected = "restore would invalidate a live transaction guard or attempt (the target predates its begin point)"
)]
fn try_attempt_inner_raw_restore_below_checkpoint_panics_at_restore() {
  // The `try_attempt` twin of the attempt test: the pinned restore panics inside the closure.
  // Converted from `try_attempt_err_after_inner_raw_restore_below_checkpoint`.
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save();
  let _ = inp.next().unwrap().expect("consume 1");

  let _out: Result<(), ()> = inp.try_attempt(|inp| {
    let _ = inp.next().unwrap().expect("consume 2");
    inp.restore(a); // POST-FIX: panics here
    Err(())
  });
}

#[test]
fn attempt_inner_lifo_clean_raw_pair_is_legal() {
  // Negative control: a raw save/restore pair taken and released entirely inside the attempt
  // (ABOVE the attempt's own pinned checkpoint) is LIFO-legal and must NOT trip the pin — the
  // attempt's checkpoint sits below it and is never popped. The attempt keeps its progress.
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let _ = inp.next().unwrap().expect("consume 1");
  let out: Option<u32> = inp.attempt(|inp| {
    let c = inp.save(); // raw checkpoint ABOVE the attempt's checkpoint
    let _ = inp.next().unwrap().expect("consume 2");
    inp.restore(c); // legal (LIFO): pops only c — the attempt's pinned checkpoint stays live
    Some(7)
  });
  assert_eq!(
    out,
    Some(7),
    "the attempt kept its progress after the legal inner raw pair"
  );
}

// ── A hand-rolled lexer that yields a ZERO-WIDTH token span ────────────────────
//
// The bundled Logos backend never produces an empty span, but the `Lexer` trait
// permits hand-written lexers that do. A zero-width token sitting at the poison
// boundary is excluded by the positional gate yet advances nothing, silently
// breaking replay and termination — so the contract forbids it and the single
// lexing chokepoint (`lex_within_boundary`) debug-asserts against it. This fixture
// yields one `[0, 0)` token to drive that assert.

#[cfg(debug_assertions)]
#[derive(Debug, Clone, PartialEq)]
struct ZeroWidthErr;

#[cfg(debug_assertions)]
impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for ZeroWidthErr
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ZeroWidthErr
  }
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ZeroWidthTok;

#[cfg(debug_assertions)]
impl core::fmt::Display for ZeroWidthTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "zero-width")
  }
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ZeroWidthKind;

#[cfg(debug_assertions)]
impl core::fmt::Display for ZeroWidthKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "zero-width")
  }
}

#[cfg(debug_assertions)]
impl Token<'_> for ZeroWidthTok {
  type Kind = ZeroWidthKind;
  type Error = ZeroWidthErr;

  fn kind(&self) -> ZeroWidthKind {
    ZeroWidthKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// A lexer that yields exactly one zero-width `[0, 0)` token, then end of input.
#[cfg(debug_assertions)]
struct ZeroWidthLexer<'inp> {
  src: &'inp str,
  state: (),
  yielded: bool,
}

#[cfg(debug_assertions)]
impl<'inp> crate::Lexer<'inp> for ZeroWidthLexer<'inp> {
  type State = ();
  type Source = str;
  type Token = ZeroWidthTok;
  type Span = crate::SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp str) -> Self {
    Self {
      src,
      state: (),
      yielded: false,
    }
  }

  fn with_state(src: &'inp str, state: ()) -> Self {
    Self {
      src,
      state,
      yielded: false,
    }
  }

  fn check(&self) -> Result<(), ZeroWidthErr> {
    Ok(())
  }

  fn state(&self) -> &Self::State {
    &self.state
  }

  fn state_mut(&mut self) -> &mut Self::State {
    &mut self.state
  }

  fn into_state(self) -> Self::State {
    self.state
  }

  fn source(&self) -> &'inp str {
    self.src
  }

  fn span(&self) -> Self::Span {
    // The zero-width span the contract forbids.
    crate::SimpleSpan::new(0, 0)
  }

  fn slice(&self) -> <Self::Source as crate::Source<Self::Offset>>::Slice<'inp> {
    ""
  }

  fn lex(&mut self) -> Option<Result<ZeroWidthTok, ZeroWidthErr>> {
    if self.yielded {
      return None;
    }
    self.yielded = true;
    Some(Ok(ZeroWidthTok))
  }

  fn bump(&mut self, _n: &usize) {}
}

#[cfg(debug_assertions)]
type ZeroWidthVerboseCtx<'a> = (Verbose<ZeroWidthErr>, DefaultCache<'a, ZeroWidthLexer<'a>>);

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "lexer contract violation")]
fn boundary_replay_zero_width_token_contract() {
  // A hand-rolled lexer that yields a zero-width token trips the debug assert at the
  // single lexing chokepoint before the empty span can corrupt any positional fact.
  let cache = DefaultCache::<'_, ZeroWidthLexer<'_>>::default();
  let mut emitter = Verbose::<ZeroWidthErr>::new();
  let mut input = Input::<ZeroWidthLexer<'_>, ZeroWidthVerboseCtx<'_>, ()>::with_state_and_cache(
    "abc",
    (),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  let _ = inp.next();
}

// ── State surgery re-keys the cache, watermark, and poison boundary ─────────────
//
// `set_state`/`state_mut` document that replacing the lexer state re-keys every
// forward-scanning fact — the token cache, the lexer-error dedup watermark, and the
// poison boundary. These four pin that the re-key actually happens on BOTH public
// state-surgery APIs, keyed to the current committed cursor, and that it re-homes offset
// facts without rewriting emission history. (The re-key governs FORWARD scanning and is
// itself transactional — a checkpoint saved before the surgery restores across it,
// undoing it; the transaction/stacked guard suites pin that.)

#[test]
fn set_state_after_limit_trip_resumes_scanning() {
  // Trip a by-value limiter, then replace the state with a fresh, non-tripped one. The
  // re-key drops the poison boundary, so scanning resumes PAST the old boundary and the
  // stream completes; the old regime's limit diagnostic stays in the log exactly once (the
  // re-key re-homes offset facts, it never rewrites history that described a real event).
  //   1 2 3 4 5 6   (limit 2 → the 3rd scanned token trips; `2` ends at the frontier offset 3)
  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    // Drive `next()` past the trip: `1` and `2` consume, the 3rd scan trips → None.
    assert!(inp.next().unwrap().is_some(), "first token");
    assert!(inp.next().unwrap().is_some(), "second token");
    assert!(
      inp.next().unwrap().is_none(),
      "the limit trip latches to None"
    );
    assert!(
      inp.is_poisoned(),
      "the limit trip latched the poison boundary"
    );

    // Replace the state with a fresh, non-tripped limiter — the documented limit-recovery
    // path. The re-key drops the poison boundary.
    inp.set_state(TokenLimiter::with_limitation(usize::MAX));
    assert!(
      !inp.is_poisoned(),
      "state replacement re-keys the poison boundary away (HEAD leaves it latched)"
    );

    // Scanning now resumes PAST the old boundary and the stream completes. At HEAD the
    // stale boundary made this first `next()` return `None` at the old frontier.
    let mut resumed = 0usize;
    while inp.next().unwrap().is_some() {
      resumed += 1;
    }
    assert_eq!(
      resumed, 4,
      "scans past the old boundary: 3, 4, 5, 6 all lex under the fresh state"
    );
  }

  // The old regime's limit diagnostic remains exactly once.
  let limit = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();
  assert_eq!(
    limit, 1,
    "the pre-replacement limit diagnostic stays in the log exactly once"
  );
}

#[test]
fn state_mut_applies_the_same_rekey() {
  // The `state_mut` twin of `set_state_after_limit_trip_resumes_scanning`: taking the
  // state mutably applies the same EAGER re-key, so resetting the tripped limiter through
  // the returned `&mut` resumes scanning past the old boundary.
  //   1 2 3 4 5 6   (limit 2 → the 3rd scanned token trips; `2` ends at the frontier offset 3)
  let cache = DefaultCache::<'_, ByValLexer<'_>>::default();
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<ByValLexer<'_>, ByValVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6",
    TokenLimiter::with_limitation(2),
    cache,
  );
  {
    let mut inp = input.as_ref(&mut emitter);

    assert!(inp.next().unwrap().is_some(), "first token");
    assert!(inp.next().unwrap().is_some(), "second token");
    assert!(
      inp.next().unwrap().is_none(),
      "the limit trip latches to None"
    );
    assert!(
      inp.is_poisoned(),
      "the limit trip latched the poison boundary"
    );

    // `state_mut` re-keys EAGERLY on the call — before any mutation through the returned
    // `&mut`. Dropping the borrow without touching it already cleared the poison boundary.
    // At HEAD `state_mut` only cleared the checkpoint lineage, leaving the boundary latched.
    let _ = inp.state_mut();
    assert!(
      !inp.is_poisoned(),
      "state_mut eagerly re-keys the poison boundary away, before any mutation"
    );

    // Now reset the tripped limiter in place so scanning resumes under a fresh budget.
    *inp.state_mut() = TokenLimiter::with_limitation(usize::MAX);

    let mut resumed = 0usize;
    while inp.next().unwrap().is_some() {
      resumed += 1;
    }
    assert_eq!(
      resumed, 4,
      "the state_mut re-key lets scanning resume past the old boundary"
    );
  }

  let limit = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();
  assert_eq!(
    limit, 1,
    "the pre-replacement limit diagnostic stays in the log exactly once"
  );
}

#[test]
fn set_state_clears_stale_cache() {
  // Peek fills the cache under the old state; replacing the state must clear it, so the
  // next read RE-LEXES from the cursor instead of serving a dead cached token. The shared
  // scan counter makes the re-lex observable — at HEAD the cached token is served and the
  // counter does not move.
  //   1 2 3 4   (high limit: never trips; the point is the cache clear)
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  let limiter = ProbeLimiter::with_limit(usize::MAX);
  let scanned = limiter.counter();
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input =
    Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache("1 2 3 4", limiter, cache);
  let mut inp = input.as_ref(&mut emitter);

  // Peek three tokens: scans `1`, `2`, `3` into the cache.
  let _ = inp.peek::<U3>().unwrap();
  assert_eq!(scanned.get(), 3, "peek scanned 1, 2, 3 into the cache");

  // Replace the state with a fresh limiter that SHARES the same scan counter, so a re-lex
  // under the new state stays observable through `scanned`. The re-key empties the cache.
  inp.set_state(ProbeLimiter {
    scanned: scanned.clone(),
    limit: usize::MAX,
  });
  assert!(
    inp.cache().is_empty(),
    "state replacement clears the token cache (HEAD leaves the stale entries)"
  );

  // The next read re-lexes from the cursor — the shared counter climbs because the dead
  // cache no longer serves the token. At HEAD the counter would stay frozen at 3.
  let before = scanned.get();
  let tok = inp.next().unwrap().expect("re-lexed token");
  assert_eq!(
    *tok.span_ref(),
    SimpleSpan::new(0, 1),
    "the re-lex resumes from the cursor (token `1`)"
  );
  assert_eq!(
    scanned.get(),
    before + 1,
    "the token was re-scanned under the new state, not served from the dead cache"
  );
}

#[test]
fn set_state_resets_watermark_to_cursor() {
  // Peek across a malformed `@`: seals its lexer error and lifts the dedup watermark past
  // it. Replacing the state re-keys the watermark back to the committed cursor (behind the
  // error) AND clears the cache holding the tokens that skipped `@`, so draining re-lexes
  // the region under the NEW regime and the error reports AGAIN — one entry per regime (the
  // documented peek-ahead-speculation edge). At HEAD the stale cache/watermark suppress the
  // second report, so only one entry survives.
  //   1 @ 2 3
  //   0 2 4 6      (`@` spans [2, 3))
  use generic_arraydeque::typenum::U2;

  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Verbose::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 @ 2 3",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );

  {
    let mut inp = input.as_ref(&mut emitter);

    // Peek a window crossing `@`: seals its lexer error and lifts the watermark past
    // [2, 3). The cursor stays at 0 and the cache holds the valid tokens that skipped `@`.
    let _ = inp.peek::<U2>().unwrap();

    // Replace the state: the re-key resets the watermark to the committed cursor (0) and
    // clears the cache.
    inp.set_state(ProbeLimiter::with_limit(usize::MAX));

    // Drain: the cleared cache forces a re-lex from the cursor across `@`, and the reset
    // watermark lets the re-lexed error report again under the new regime.
    while inp.next().unwrap().is_some() {}
  }

  let total: usize = emitter.errors().values().map(|group| group.len()).sum();
  assert_eq!(
    total, 2,
    "the `@` lexer error reports once per regime: at the peek, then again after the re-key re-lexes it"
  );
}

// ── Raw checkpoints: the save → (restore | commit) discipline ────────────────────
//
// Every saved checkpoint should end in exactly one of `restore` (abandon progress) or
// `commit` (keep progress). The two tests below pin both halves: a checkpoint MERELY
// DROPPED on the success path strands its lineage id (the documented leak, kept as a
// pinned-behavior test), while `commit` is the verb that keeps the progress AND releases
// the id so the input-owned lineage stack stays bounded across commit-heavy loops.

/// LEAK CAPTURE (pinned behavior, NOT a bug to fix with a `Drop` impl): a raw checkpoint
/// that is simply dropped on the success path never releases its lineage id — only
/// [`restore`] or [`commit`] do. A `Checkpoint` owns no borrow it could release on drop,
/// so 100 successful speculations that drop their checkpoints grow the input's live
/// lineage stack by 100. The fix for the unbounded growth is the explicit `commit` verb
/// (see `raw_checkpoint_commit_releases_lineage`), not a `Drop` impl — this test freezes
/// the drop-leaks behavior so a future `Drop`-based change would be caught here.
///
/// [`restore`]: crate::InputRef::restore
/// [`commit`]: crate::InputRef::commit
#[test]
fn raw_checkpoint_drop_leaks_lineage_without_commit() {
  let (mut input, _scanned) = probe_input("1 2 3 4");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let baseline = inp.live_checkpoints_len();
  for _ in 0..100 {
    let _ckp = inp.save();
    // Success path: drop the checkpoint without restoring OR committing. Nothing pops
    // its id, so it lingers on the live lineage stack.
  }
  assert_eq!(
    inp.live_checkpoints_len(),
    baseline + 100,
    "a raw checkpoint dropped without commit strands its lineage id: 100 drops grow the stack by 100"
  );
}

/// The fix: [`commit`](crate::InputRef::commit) consumes a raw checkpoint, keeps all
/// progress, and releases its lineage id — the verb missing next to `restore`. 100
/// save → commit cycles must leave the lineage stack at baseline every iteration, so the
/// stack stays bounded across successful speculation (contrast the drop-leak sibling).
#[test]
fn raw_checkpoint_commit_releases_lineage() {
  let (mut input, _scanned) = probe_input("1 2 3 4");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let baseline = inp.live_checkpoints_len();
  for _ in 0..100 {
    let ckp = inp.save();
    // Success path: keep progress, release the lineage id (O(1) — the id is the stack top).
    inp.commit(ckp);
    assert_eq!(
      inp.live_checkpoints_len(),
      baseline,
      "each commit forgets its id — the live stack returns to baseline every iteration"
    );
  }
}

/// The documented retry pattern with the success arm committing: each round runs a couple
/// of speculative probes that `restore` (fail), then a succeeding attempt that `commit`s
/// (keep). The lineage stack is flat after every round, and the consumed token stream is
/// faithful (all four tokens, in order), proving `commit` keeps progress while the probes'
/// restores rewind cleanly.
#[test]
fn raw_retry_loop_with_commit_stays_flat() {
  // A high limit: the speculative probes re-scan tokens, and we do not want the shared
  // limiter to trip and turn `next()` into a bounded `None` mid-stream.
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  let mut emitter = Silent::<ProbeErr>::new();
  let mut input = Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4",
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let baseline = inp.live_checkpoints_len();
  let mut consumed = Vec::new();

  loop {
    // Two failed speculative probes that roll back to the current position…
    for _ in 0..2 {
      let probe = inp.save();
      let _ = inp.next().unwrap(); // look ahead speculatively
      inp.restore(probe); // fail: roll back to where we were
    }
    // …then the succeeding attempt keeps its progress via `commit`.
    let ckp = inp.save();
    match inp.next().unwrap() {
      Some(tok) => {
        consumed.push(*tok.span_ref());
        inp.commit(ckp); // success: keep progress, release the lineage id
      }
      None => {
        inp.commit(ckp); // end of input: nothing consumed, still release the id
        break;
      }
    }
    assert_eq!(
      inp.live_checkpoints_len(),
      baseline,
      "retry round leaves the stack flat: failed probes restored, the success committed"
    );
  }

  assert_eq!(
    consumed.len(),
    4,
    "the retry loop consumed all four tokens despite the speculative probes"
  );
  assert!(
    consumed.windows(2).all(|w| w[0].start < w[1].start),
    "the committed token stream is faithful and in order"
  );
}

/// Committing an already-invalidated checkpoint is a harmless no-op — no panic (even under
/// debug assertions), no state change. Save A, save B, restore the older A (which pops B
/// off the live lineage), then commit the dead B: its id is simply absent, so the forget
/// removes nothing and the input state is untouched.
#[test]
fn commit_of_invalidated_checkpoint_is_noop() {
  let (mut input, _scanned) = probe_input("1 2 3 4");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let a = inp.save();
  let _ = inp.next().unwrap(); // consume `1` so B captures a distinct position
  let b = inp.save();

  // Restoring the OLDER `a` rolls back to position 0 and invalidates the younger `b`.
  inp.restore(a);
  let len_after_restore = inp.live_checkpoints_len();
  let cursor_after_restore = *inp.cursor().as_inner();

  // Commit the dead `b`: no panic, and nothing changes.
  inp.commit(b);

  assert_eq!(
    inp.live_checkpoints_len(),
    len_after_restore,
    "committing a dead checkpoint removes nothing — the lineage stack is unchanged"
  );
  assert_eq!(
    *inp.cursor().as_inner(),
    cursor_after_restore,
    "committing a dead checkpoint touches no input state"
  );
}

// ── Balanced synchronization: the sync_balanced contract ─────────────────────
//
// A delimiter-capable token set behind the by-value `TokenLimiter` (high limit unless a
// test trips it), with `ByValErr` as the shared error type. The classifier marks the
// parentheses as a pair; everything else is neutral.

use crate::input::Balance;

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = TokenLimiter, skip r"[ \t\r\n]+")]
enum BalTok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); })]
  Num,
  #[token("(", |lex| { lex.extras.increase(); })]
  LParen,
  #[token(")", |lex| { lex.extras.increase(); })]
  RParen,
  #[token(";", |lex| { lex.extras.increase(); })]
  Semi,
  /// The one kind [`Token::is_trivia`] reports as skippable, so the `padded` combinators — which
  /// are `skip_while(is_trivia)` around a parser — have something to skip in the
  /// [cache-transparency matrix](cache_transparency_matrix).
  ///
  /// It is a real token, not lexer-level `skip`ped whitespace, and that distinction is what gives
  /// this fixture its teeth: the lexer skips the spaces *between* tokens, so the end of one token
  /// and the start of the next are DIFFERENT offsets. A resume cursor placed at the former rather
  /// than the latter is therefore visible here — which is exactly the divergence a skip that threw
  /// its stopping token away used to produce.
  #[token("~", |lex| { lex.extras.increase(); })]
  Trivia,
}

impl core::fmt::Display for BalTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Num => "number",
      Self::LParen => "`(`",
      Self::RParen => "`)`",
      Self::Semi => "`;`",
      Self::Trivia => "trivia",
    })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BalKind {
  Num,
  LParen,
  RParen,
  Semi,
  Trivia,
}

impl core::fmt::Display for BalKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Num => "number",
      Self::LParen => "`(`",
      Self::RParen => "`)`",
      Self::Semi => "`;`",
      Self::Trivia => "trivia",
    })
  }
}

impl Token<'_> for BalTok {
  type Kind = BalKind;
  type Error = ByValErr;

  fn kind(&self) -> BalKind {
    match self {
      Self::Num => BalKind::Num,
      Self::LParen => BalKind::LParen,
      Self::RParen => BalKind::RParen,
      Self::Semi => BalKind::Semi,
      Self::Trivia => BalKind::Trivia,
    }
  }

  fn is_trivia(&self) -> bool {
    matches!(self, Self::Trivia)
  }
}

type BalLexer<'a> = LogosLexer<'a, BalTok>;
type BalVerboseCtx<'a> = (Verbose<ByValErr>, DefaultCache<'a, BalLexer<'a>>);
type BalFatalCtx<'a> = (
  crate::emitter::Fatal<ByValErr>,
  DefaultCache<'a, BalLexer<'a>>,
);

/// The parenthesis pair table: `(` opens, `)` closes, everything else is neutral.
fn parens(kind: &BalKind) -> Balance<char> {
  match kind {
    BalKind::LParen => Balance::Open('('),
    BalKind::RParen => Balance::Close('('),
    _ => Balance::Neutral,
  }
}

fn bal_input(src: &str, limit: usize) -> Input<'_, BalLexer<'_>, BalVerboseCtx<'_>, ()> {
  Input::with_state_and_cache(
    src,
    TokenLimiter::with_limitation(limit),
    DefaultCache::<'_, BalLexer<'_>>::default(),
  )
}

#[test]
fn sync_balanced_skips_enclosed_sync_tokens() {
  // Nesting: the `;` inside the parenthesized garbage is at depth 1, where the sync
  // predicate is never consulted, so the skip runs through it to the depth-0 `;`.
  //   ( ; ) ;
  //   0 2 4 6
  use crate::span::SimpleSpan;

  let mut input = bal_input("( ; ) ;", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
      .unwrap()
      .expect("the depth-0 `;` is a sync point");
    assert_eq!(
      hole.span(),
      SimpleSpan::new(0, 5),
      "the hole covers the skipped region `( ; )`"
    );
    assert_eq!(hole.skipped(), 3, "three tokens skipped into the hole");

    // Stopped BEFORE the depth-0 sync token: committed at the last skipped token.
    assert_eq!(inp.span(), &SimpleSpan::new(4, 5), "committed at `)`");
    let next = inp.next().unwrap().expect("the sync token is next");
    assert_eq!(*next.span_ref(), SimpleSpan::new(6, 7), "the depth-0 `;`");
    assert!(matches!(next.data(), BalTok::Semi));
  }

  // One diagnostic per hole — and no per-token unexpected-token noise.
  assert_eq!(
    emitter
      .skipped_regions()
      .get(&crate::span::SimpleSpan::new(0, 5)),
    Some(&std::vec![3usize]),
    "exactly one skipped-region record, with the hole span and count"
  );
  assert_eq!(
    emitter
      .skipped_regions()
      .values()
      .map(|g| g.len())
      .sum::<usize>(),
    1,
    "exactly one emit_skipped_region call"
  );
  let total: usize = emitter.errors().values().map(|g| g.len()).sum();
  assert_eq!(total, 0, "the skipped tokens are not reported individually");
}

#[test]
fn sync_balanced_stray_closer_is_garbage_and_depth_saturates() {
  // A stray `)` at depth 0 that is not a sync point is skipped as garbage; the depth
  // saturates at zero, so the following depth-0 `;` still syncs.
  //   ) 1 ;
  //   0 2 4
  use crate::span::SimpleSpan;

  let mut input = bal_input(") 1 ;", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
      .unwrap()
      .expect("the `;` is a sync point");
    assert_eq!(hole.span(), SimpleSpan::new(0, 3), "the hole covers `) 1`");
    assert_eq!(hole.skipped(), 2);

    let next = inp.next().unwrap().expect("the sync token is next");
    assert!(matches!(next.data(), BalTok::Semi));
    assert_eq!(*next.span_ref(), SimpleSpan::new(4, 5));
  }
}

#[test]
fn sync_balanced_stray_closer_in_sync_set_syncs_at_depth_zero() {
  // The classic `}` recovery target: a closer at depth 0 that IS in the sync set syncs —
  // the predicate is consulted before the classifier at depth 0.
  //   1 )
  //   0 2
  use crate::span::SimpleSpan;

  let mut input = bal_input("1 )", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::RParen))
      .unwrap()
      .expect("the depth-0 `)` is the sync point");
    assert_eq!(hole.span(), SimpleSpan::new(0, 1), "the hole covers `1`");
    assert_eq!(hole.skipped(), 1);

    let next = inp.next().unwrap().expect("the sync token is next");
    assert!(matches!(next.data(), BalTok::RParen));
    assert_eq!(*next.span_ref(), SimpleSpan::new(2, 3));
  }
}

#[test]
fn sync_balanced_opener_in_sync_set_syncs_before_counting() {
  // The depth-0 predicate is consulted before the classifier for openers too: syncing to
  // `(` stops before it instead of opening a pair.
  //   1 (
  //   0 2
  use crate::span::SimpleSpan;

  let mut input = bal_input("1 (", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::LParen))
      .unwrap()
      .expect("the depth-0 `(` is the sync point");
    assert_eq!(hole.skipped(), 1);

    let next = inp.next().unwrap().expect("the sync token is next");
    assert!(matches!(next.data(), BalTok::LParen));
    assert_eq!(*next.span_ref(), SimpleSpan::new(2, 3));
  }
}

#[test]
fn sync_balanced_zero_skip_success_emits_no_diagnostic() {
  // The sync point is the very next token: success with an empty, zero-width hole at the
  // resume position — and one-diagnostic-per-hole means no diagnostic for an empty hole.
  use crate::span::SimpleSpan;

  let mut input = bal_input("; 1", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
      .unwrap()
      .expect("the `;` is immediately at hand");
    assert_eq!(hole.skipped(), 0, "nothing was skipped");
    assert_eq!(
      hole.span(),
      SimpleSpan::new(0, 0),
      "a zero-skip hole is zero-width at the resume position"
    );

    assert_eq!(inp.span(), &SimpleSpan::new(0, 0), "no progress committed");
    let next = inp.next().unwrap().expect("the sync token is next");
    assert!(matches!(next.data(), BalTok::Semi));
  }

  let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
  assert_eq!(holes, 0, "an empty hole is not reported");
}

#[test]
fn sync_balanced_finds_sync_point_in_prefilled_cache() {
  // The skipped prefix and the sync point can both sit in peeked lookahead: the drain
  // commits the skipped cached tokens (with no per-token diagnostics) and stops at the
  // cached sync point, which stays cached for the caller.
  //   1 ;
  //   0 2
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U2;

  let mut input = bal_input("1 ;", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    drop(inp.peek::<U2>().unwrap());

    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
      .unwrap()
      .expect("the cached `;` is the sync point");
    assert_eq!(hole.span(), SimpleSpan::new(0, 1), "the hole covers `1`");
    assert_eq!(hole.skipped(), 1);

    assert_eq!(inp.span(), &SimpleSpan::new(0, 1), "committed at `1`");
    let next = inp.next().unwrap().expect("the cached sync token is next");
    assert!(matches!(next.data(), BalTok::Semi));
    assert_eq!(*next.span_ref(), SimpleSpan::new(2, 3));
  }

  let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
  assert_eq!(holes, 1, "exactly one hole for the drained prefix");
  let total: usize = emitter.errors().values().map(|g| g.len()).sum();
  assert_eq!(total, 0, "no per-token diagnostics for the drained prefix");
}

#[test]
fn failed_sync_balanced_leaves_no_trace() {
  // No sync point before end of input: the balanced sync fails and the no-trace law
  // applies — position and state rewound to the pre-call anchor, and no hole diagnostic
  // (one diagnostic per hole means none for a failed hole).
  //   1 2 3
  use crate::span::SimpleSpan;

  let mut input = bal_input("1 2 3", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    assert!(
      inp
        .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
        .unwrap()
        .is_none(),
      "no sync point: the balanced sync fails"
    );

    assert_eq!(
      inp.span(),
      &SimpleSpan::new(0, 0),
      "span stays at the pre-call anchor"
    );
    assert_eq!(inp.state().tokens(), 0, "state stays at the pre-call count");

    let mut spans = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      spans.push(*t.span_ref());
    }
    assert_eq!(
      spans,
      std::vec![
        SimpleSpan::new(0, 1),
        SimpleSpan::new(2, 3),
        SimpleSpan::new(4, 5)
      ],
      "the drain consumes the full token sequence normally"
    );
  }

  let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
  assert_eq!(holes, 0, "a failed hole is never reported");
  let total: usize = emitter.errors().values().map(|g| g.len()).sum();
  assert_eq!(total, 0, "a failed balanced sync leaves no diagnostics");
}

#[test]
fn failed_sync_balanced_with_prefilled_cache_leaves_no_trace() {
  // The no-trace law holds across a prefilled cache: the drained cached prefix is rewound
  // too, and the next read re-lexes it identically.
  //   1 2 3
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U2;

  let mut input = bal_input("1 2 3", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    drop(inp.peek::<U2>().unwrap());

    assert!(
      inp
        .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
        .unwrap()
        .is_none(),
      "no sync point: the balanced sync fails"
    );

    assert_eq!(
      inp.span(),
      &SimpleSpan::new(0, 0),
      "the drained cache prefix is rewound with the rest"
    );

    let mut spans = Vec::new();
    while let Some(t) = inp.next().unwrap() {
      spans.push(*t.span_ref());
    }
    assert_eq!(
      spans,
      std::vec![
        SimpleSpan::new(0, 1),
        SimpleSpan::new(2, 3),
        SimpleSpan::new(4, 5)
      ],
      "the formerly-cached tokens replay identically"
    );
  }

  let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
  assert_eq!(holes, 0, "a failed hole is never reported");
}

#[test]
fn failed_sync_balanced_reemits_crossed_lexer_error_once() {
  // A failed balanced sync unwinds the lexer errors it crossed AND restores the dedup
  // watermark, so the genuine consume that follows re-reports each exactly once.
  //   1 @ 2   (`@` is a lexer error spanning [2, 3))
  use crate::span::SimpleSpan;

  let mut input = bal_input("1 @ 2", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    assert!(
      inp
        .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
        .unwrap()
        .is_none(),
      "no sync point: the balanced sync fails"
    );

    while inp.next().unwrap().is_some() {}
  }

  let at = SimpleSpan::new(2, 3);
  assert_eq!(
    emitter.errors().get(&at).map(|g| g.len()).unwrap_or(0),
    1,
    "the crossed lexer error re-emits exactly once on the genuine consume"
  );
  let total: usize = emitter.errors().values().map(|g| g.len()).sum();
  assert_eq!(total, 1, "no other diagnostics survive the failed sync");
}

#[test]
fn sync_balanced_trip_commits_prefix_without_hole_diagnostic() {
  // A resource-limit trip mid-skip follows the sync-family trip contract: the skipped
  // prefix is committed at the durable frontier and the poison latches there — but the
  // sync itself failed, so NO hole diagnostic is emitted (only the limit error persists).
  //   1 2 3 4   (limit 2 → the 3rd scanned token trips; `2` ends at offset 3)
  use crate::span::SimpleSpan;

  let mut input = bal_input("1 2 3 4", 2);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    assert!(
      inp
        .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
        .unwrap()
        .is_none(),
      "the trip yields the poisoned outcome — no sync point"
    );

    assert!(inp.is_poisoned(), "the trip latches the poison boundary");
    assert_eq!(
      inp.span(),
      &SimpleSpan::new(2, 3),
      "committed at the end of the last skipped token (`2`)"
    );
    assert_eq!(
      inp.state().tokens(),
      2,
      "committed state counts exactly the skipped prefix"
    );
  }

  let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
  assert_eq!(holes, 0, "a tripped (failed) sync reports no hole");
  let limit = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();
  assert_eq!(limit, 1, "the limit trip is diagnosed exactly once");
  let lex = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Lex)
    .count();
  assert_eq!(lex, 0, "no per-token unexpected diagnostics from the skip");
}

#[test]
fn sync_balanced_fatal_emitter_mid_skip_commits_the_error_token() {
  // A fatal emitter rejection during a mid-skip emission follows the sync-family
  // fatal-exit discipline (the sync_through trip-commit precedent): the token that trips
  // the fatal emitter is committed, and the error propagates.
  //   1 @ ;   (`@` is a lexer error spanning [2, 3); `Fatal` rejects its emission)
  use crate::span::SimpleSpan;

  let mut input = Input::<BalLexer<'_>, BalFatalCtx<'_>, ()>::with_state_and_cache(
    "1 @ ;",
    TokenLimiter::with_limitation(usize::MAX),
    DefaultCache::<'_, BalLexer<'_>>::default(),
  );
  let mut emitter = crate::emitter::Fatal::<ByValErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let r = inp.sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi));
  assert_eq!(
    r,
    Err(ByValErr::Lex),
    "the fatal emitter's rejection propagates"
  );
  assert_eq!(
    inp.span(),
    &SimpleSpan::new(2, 3),
    "the token that tripped the fatal emitter is committed"
  );
}

#[test]
fn sync_balanced_hole_emission_unwinds_on_rollback() {
  // The hole diagnostic is rewind-safe by construction: it rides the emitter log, so an
  // enclosing attempt that rolls the skip back unwinds it like any other emission — and a
  // clean re-run records it exactly once again.
  //   1 2 ;
  use crate::span::SimpleSpan;

  let mut input = bal_input("1 2 ;", usize::MAX);
  let mut emitter = Verbose::<ByValErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);

    let declined: Option<()> = inp.attempt(|inp| {
      let hole = inp
        .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
        .unwrap()
        .expect("the `;` is a sync point");
      assert_eq!(hole.skipped(), 2);
      None
    });
    assert!(declined.is_none(), "the attempt declines and rolls back");

    assert_eq!(
      inp.span(),
      &SimpleSpan::new(0, 0),
      "the rollback restores the pre-skip position"
    );
    // The rolled-back hole emission is gone; a clean re-run records it exactly once.
    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), BalTok::Semi))
      .unwrap()
      .expect("the `;` is still the sync point");
    assert_eq!(hole.span(), SimpleSpan::new(0, 3));
    assert_eq!(hole.skipped(), 2);
  }

  assert_eq!(
    emitter
      .skipped_regions()
      .get(&crate::span::SimpleSpan::new(0, 3)),
    Some(&std::vec![2usize]),
    "exactly one hole record survives: the rolled-back one was unwound"
  );
  let holes: usize = emitter.skipped_regions().values().map(|g| g.len()).sum();
  assert_eq!(holes, 1);
}

// ── Panic safety: an unwinding closure must not strand the attempt's pinned begin point ──────
//
// `attempt`/`try_attempt` pin their begin point and then hand the input to user code. If that
// code panics and the host catches the unwind — a test harness, a fuzzer, an editor server: any
// host that refuses to die on a panic — the settle arms never run. So the begin point is *held*
// by a rollback-on-drop `Transaction`, and its `Drop` releases the pin and the lineage id on the
// unwind edge exactly as it does on a decline. Without that, the pin would sit on the input for
// the rest of its life, for an attempt nobody can ever settle: a later restore to an older target
// scans upward, meets the orphan, and panics spuriously — and the live stack grows without bound.
//
// `catch_unwind` needs an unwinding runtime. A `panic = "abort"` build cannot run these two, and
// has no need to: there the process dies at the panic, so no input survives to be poisoned.

/// An unlimited `Silent` probe input: the attempts below speculate over real tokens, so the
/// limiter must never trip (contrast [`probe_input`], whose limit of 2 is the point of it).
fn unlimited_probe_input(src: &str) -> Input<'_, ProbeLexer<'_>, ProbeCtx<'_>, ()> {
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  Input::<ProbeLexer<'_>, ProbeCtx<'_>, ()>::with_state_and_cache(
    src,
    ProbeLimiter::with_limit(usize::MAX),
    cache,
  )
}

#[test]
fn attempt_closure_panic_releases_the_pinned_begin_point() {
  let mut input = unlimited_probe_input("1 2 3 4 5");
  let mut emitter = Silent::<ProbeErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    let baseline = inp.live_checkpoints_len();

    // An OLDER checkpoint — the restore target a stranded pin would poison.
    let outer = inp.save();
    let _ = inp.next().unwrap().expect("1");

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let _: Option<()> = inp.attempt(|inp| {
        let _ = inp.next().unwrap().expect("2");
        panic!("the attempt's closure unwinds")
      });
    }));
    assert!(caught.is_err(), "the panic unwound out of the attempt");

    // The abandoned begin point released BOTH memos on the way out, so the older checkpoint is
    // still restorable. A pin left above `outer` would make this restore panic instead
    // ("restore would invalidate a live transaction guard or attempt").
    inp.restore(outer);
    assert_eq!(
      inp.live_checkpoints_len(),
      baseline,
      "the unwind left no live checkpoint behind either"
    );
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "a caught panic inside `attempt` leaves no pin: the pin set holds exactly the live begin \
     points, and with the attempt gone there are none"
  );
}

#[test]
fn try_attempt_closure_panic_releases_the_pinned_begin_point() {
  let mut input = unlimited_probe_input("1 2 3 4 5");
  let mut emitter = Silent::<ProbeErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    let baseline = inp.live_checkpoints_len();

    let outer = inp.save();
    let _ = inp.next().unwrap().expect("1");

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let _: Result<(), ()> = inp.try_attempt(|inp| {
        let _ = inp.next().unwrap().expect("2");
        panic!("the attempt's closure unwinds")
      });
    }));
    assert!(caught.is_err(), "the panic unwound out of the attempt");

    inp.restore(outer);
    assert_eq!(
      inp.live_checkpoints_len(),
      baseline,
      "the unwind left no live checkpoint behind either"
    );
  }
  assert_eq!(
    input.pinned_checkpoints_len(),
    0,
    "a caught panic inside `try_attempt` leaves no pin either"
  );
}

// ── The cache-transparency matrix ────────────────────────────────────────────
//
// Every scanner in this crate skips a run of tokens and then stops on one: the sync family
// (`sync_to`/`sync_through`/`sync_balanced`) stops on the token its predicate matches, and
// `skip_while` — the trivia path, and therefore `padded` — stops on the first token its predicate
// rejects. Each used to be TWO parallel implementations of "take a token and act on it": a
// cache-drain prologue popping tokens a peek had already lexed, and a loop lexing them itself.
// Nothing in the types forced the two to agree, and yet the peek cache is an INVISIBLE
// OPTIMIZATION — whether a token happened to be prefetched must not change one thing a caller can
// observe about the call.
//
// This matrix is the enforcement. Every entry point runs the same logical token stream twice —
// once from an empty cache, once with the first N tokens peeked into it — and the two runs must
// agree on everything the caller sees:
//
//   * the return value (matched token, peeked window, hole, `padded`'s parsed output);
//   * the committed span and lexer state;
//   * the poison boundary;
//   * the tokens a later drain yields — the "a recovery retries after the error" law, the one the
//     fatal-trip divergence broke;
//   * the resume cursor. For the scans that never report a skipped token this is asserted
//     EXACTLY (see `Entry::pins_the_resume_cursor`); for the rest, the bounded law below;
//   * the diagnostics the call itself emits, in order, and every diagnostic of the whole run
//     exactly once;
//   * which tokens the predicate was asked about, in order: a stateful `FnMut` must not be able
//     to tell that it is being driven by a drain rather than by a lex.
//
// Two consequences of a prefilled cache ARE visible, and both are stated in the contract docs
// (`sync_through`, and the dedup rule on `emit_lexer_error_deduped`):
//
//   1. the cache holds lookahead the uncached run has not lexed yet, so `offset()` (the lex
//      frontier) and the cache depth run ahead of it. The token stream does not, and that is what
//      is asserted;
//   2. a peek EMITS the lexer errors it crosses, when it crosses them. Prefetching therefore
//      moves such a diagnostic earlier in the timeline, and the dedup watermark then keeps the
//      sync (or a later replay) from repeating it. The invariant that survives — and is asserted
//      exactly, not loosely — is that the cached run emits precisely what the uncached run
//      emitted, minus the entries the prefill had already reported, and that no diagnostic is
//      ever lost or doubled.
//
// Adding a cell is one row in `CELLS`; every cell runs against every entry point.

use generic_arraydeque::typenum::{U1 as W1, U2 as W2, U3 as W3};

use crate::{
  InputRef, ParseInput, Window,
  cache::{Peeked, PeekedTokenExt},
  emitter::Emitter,
  error::token::UnexpectedTokenOf,
  input::Cursor,
  span::{SimpleSpan, Spanned},
};

/// One recorded emission: which channel it came through, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Emission {
  /// `emit_unexpected_token` — the per-skipped-token diagnostic BOTH sync paths make, and the
  /// only emission a cached token can trip a fatal emitter with (a lexer error is never cached).
  Unexpected(SimpleSpan),
  /// `emit_lexer_error` with a plain lexeme error.
  Lex(SimpleSpan),
  /// `emit_lexer_error` with a sticky limit trip.
  Limit(SimpleSpan),
  /// `emit_skipped_region` — `sync_balanced`'s one-per-hole note.
  Hole(SimpleSpan, usize),
}

impl Emission {
  /// Whether a *peek* could have produced this entry. A peek emits the lexer errors it crosses
  /// and nothing else: it never diagnoses an unexpected token, never reports a hole. The matrix
  /// asserts this, so a prefill can only ever hoist a lexer-class diagnostic.
  const fn is_lexer_class(&self) -> bool {
    matches!(self, Self::Lex(_) | Self::Limit(_))
  }
}

/// The matrix emitter: an ordered, rewindable emission log plus a single span it rejects as
/// fatal.
///
/// The span key is what makes the fatal cells differential. An index-keyed trip would fire at a
/// different *token* in the two runs (the cached run's prefill emits the lexer errors it
/// crosses, shifting every later index); a span-keyed one rejects the diagnostic of the same
/// token in both, which is exactly the comparison the matrix needs.
#[derive(Debug)]
struct MatrixEmitter {
  log: std::vec::Vec<Emission>,
  fatal_at: Option<SimpleSpan>,
}

impl MatrixEmitter {
  fn new(fatal_at: Option<SimpleSpan>) -> Self {
    Self {
      log: std::vec::Vec::new(),
      fatal_at,
    }
  }

  /// Records the emission, then rejects it if this cell made that span fatal. Recording first is
  /// deliberate: the diagnostic *was* offered, and the log is what the matrix compares.
  fn record(&mut self, entry: Emission, span: SimpleSpan, err: ByValErr) -> Result<(), ByValErr> {
    self.log.push(entry);
    match self.fatal_at {
      Some(fatal) if fatal == span => Err(err),
      _ => Ok(()),
    }
  }
}

impl<'inp, L, Lang: ?Sized> Emitter<'inp, L, Lang> for MatrixEmitter
where
  L: crate::Lexer<'inp, Span = SimpleSpan>,
  <L::Token as Token<'inp>>::Error: Into<ByValErr>,
{
  type Error = ByValErr;

  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), ByValErr> {
    let (span, err) = err.into_components();
    let err: ByValErr = err.into();
    let entry = match err {
      ByValErr::Limit => Emission::Limit(span),
      ByValErr::Lex => Emission::Lex(span),
    };
    self.record(entry, span, err)
  }

  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), ByValErr> {
    let span = *err.span_ref();
    self.record(Emission::Unexpected(span), span, ByValErr::Lex)
  }

  fn emit_error(&mut self, err: Spanned<ByValErr, L::Span>) -> Result<(), ByValErr> {
    let (span, err) = err.into_components();
    let entry = match err {
      ByValErr::Limit => Emission::Limit(span),
      ByValErr::Lex => Emission::Lex(span),
    };
    self.record(entry, span, err)
  }

  fn emit_skipped_region(&mut self, span: L::Span, skipped: usize) -> Result<(), ByValErr> {
    self.record(Emission::Hole(span, skipped), span, ByValErr::Lex)
  }

  fn checkpoint(&self) -> u64 {
    self.log.len() as u64
  }

  fn rewind(&mut self, _cursor: &Cursor<'inp, '_, L>, checkpoint: u64) {
    let mark = (checkpoint as usize).min(self.log.len());
    self.log.truncate(mark);
  }
}

type MatrixCtx<'a> = (MatrixEmitter, DefaultCache<'a, BalLexer<'a>>);
type MatrixRef<'inp, 'closure> = InputRef<'inp, 'closure, BalLexer<'inp>, MatrixCtx<'inp>, ()>;

/// Every public entry point that drives the shared scanner: the six of the sync family, plus
/// `skip_while` — the trivia path — and `padded`, the combinator built on it. Each takes its
/// tokens from the cache while one is there and from the lexer once it is not, so each is on the
/// hook for cache transparency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
  To,
  ToThenPeekWithEmitter,
  Through,
  ThroughThenPeek,
  ThroughThenPeekWithEmitter,
  Balanced,
  SkipWhile,
  Padded,
}

impl Entry {
  const ALL: &'static [Entry] = &[
    Entry::To,
    Entry::ToThenPeekWithEmitter,
    Entry::Through,
    Entry::ThroughThenPeek,
    Entry::ThroughThenPeekWithEmitter,
    Entry::Balanced,
    Entry::SkipWhile,
    Entry::Padded,
  ];

  const fn name(self) -> &'static str {
    match self {
      Entry::To => "sync_to",
      Entry::ToThenPeekWithEmitter => "sync_to_then_peek_with_emitter",
      Entry::Through => "sync_through",
      Entry::ThroughThenPeek => "sync_through_then_peek",
      Entry::ThroughThenPeekWithEmitter => "sync_through_then_peek_with_emitter",
      Entry::Balanced => "sync_balanced",
      Entry::SkipWhile => "skip_while",
      Entry::Padded => "padded",
    }
  }

  /// Whether this entry point's resume [`cursor`](InputRef::cursor) is asserted **exactly** equal
  /// across the two runs, rather than only by the bounded law.
  ///
  /// `cursor()` is `front cached token's start`, or the committed span's end when nothing is
  /// cached. A scan therefore pins it exactly — cached or not — when both of these hold:
  ///
  /// - it **leaves the token it stopped on unconsumed**, so that token is the cache front in both
  ///   runs (any deeper lookahead the prefill bought sits *behind* it and cannot be seen), and any
  ///   leftover-free exit — end of input, a limit trip, a fatal lexer error — is reached only with
  ///   the cache drained, so both runs land on the committed span's end;
  /// - it **reports no skipped token**, so it has no fatal exit that can strand an un-drained
  ///   prefill at the cache front. `sync_to` fails exactly here: an emitter that rejects a skipped
  ///   token's diagnostic stops the drain mid-cache, leaving the rest of the prefill in front of
  ///   the cursor — real, declared cache-dependence, and why the reporting entries get the bounded
  ///   law instead.
  ///
  /// `skip_while`, `padded` and `sync_balanced` satisfy both. For `skip_while` and `padded` that is
  /// the whole point of this matrix: the cursor after a trivia skip was the cache-dependent
  /// observable, moving with the caller's lookahead depth, until the skip started leaving its
  /// stopping token at the cache front on both origins.
  const fn pins_the_resume_cursor(self) -> bool {
    matches!(self, Entry::Balanced | Entry::SkipWhile | Entry::Padded)
  }
}

/// `padded`'s inner parser in the matrix: consumes exactly one token and reports it, so a cell can
/// compare `padded`'s parsed output as well as the state it left behind.
///
/// It emits nothing itself, so every diagnostic a `padded` cell makes comes from the two trivia
/// skips around it — which is precisely what the cell is comparing.
struct TakeOne;

/// The emitter error the matrix context surfaces — spelled through the associated-type chain the
/// [`ParseInput`] signature demands (it is `ByValErr`, but the trait will not take the shortcut).
type MatrixErr<'inp> =
  <<MatrixCtx<'inp> as crate::ParseContext<'inp, BalLexer<'inp>, ()>>::Emitter as Emitter<
    'inp,
    BalLexer<'inp>,
    (),
  >>::Error;

impl<'inp> ParseInput<'inp, BalLexer<'inp>, Option<(SimpleSpan, BalTok)>, MatrixCtx<'inp>, ()>
  for TakeOne
{
  fn parse_input(
    &mut self,
    inp: &mut MatrixRef<'inp, '_>,
  ) -> Result<Option<(SimpleSpan, BalTok)>, MatrixErr<'inp>> {
    Ok(inp.next()?.map(Spanned::into_components))
  }
}

/// The normalized outcome of a sync call, in one shape across all six entry points: each fills
/// the fields it has, so the differential comparison is a plain `==`.
#[derive(Debug, Clone, PartialEq)]
struct Ret {
  /// The sync token the call surfaced — peeked by the `to` family, consumed by the `through`
  /// family.
  matched: Option<(SimpleSpan, BalTok)>,
  /// The window the `_then_peek` variants peeked after settling.
  peeked: std::vec::Vec<SimpleSpan>,
  /// The region `sync_balanced` describes.
  hole: Option<(SimpleSpan, usize)>,
  /// The fatal rejection, if the emitter made one.
  err: Option<ByValErr>,
}

impl Ret {
  fn empty() -> Self {
    Self {
      matched: None,
      peeked: std::vec::Vec::new(),
      hole: None,
      err: None,
    }
  }

  fn fatal(err: ByValErr) -> Self {
    Self {
      err: Some(err),
      ..Self::empty()
    }
  }
}

/// Everything a caller can observe about a run: the call's outcome, the input state it left, and
/// the whole emission timeline — plus the token stream a retry would then see.
#[derive(Debug)]
struct Obs {
  ret: Ret,
  span: SimpleSpan,
  tokens: usize,
  poison: Option<usize>,
  /// The poison boundary once the stream has been drained: by then the two runs have read exactly
  /// the same source, so the same limit trips at the same durable frontier — it must agree exactly.
  poison_drained: Option<usize>,
  cursor: usize,
  /// The lexer-error dedup watermark right after the call.
  watermark: usize,
  /// The same watermark once the stream has been drained: by then the two runs have read exactly
  /// the same source, so it must agree exactly.
  watermark_drained: usize,
  /// The tokens the predicate was asked about, in order. A stateful `FnMut` sees exactly this.
  pred_calls: std::vec::Vec<SimpleSpan>,
  /// What the run emitted BEFORE the sync call: the setup consume, and then — in a cached run
  /// only — the prefill peek. The two runs share the consume, so the difference between their
  /// setup logs is exactly what the peek hoisted.
  setup_log: std::vec::Vec<Emission>,
  /// What the sync call itself emitted.
  sync_log: std::vec::Vec<Emission>,
  /// The tokens a post-call drain yields: the "recovery retries after the error" law.
  replay: std::vec::Vec<(SimpleSpan, BalTok)>,
  /// What that drain emitted.
  replay_log: std::vec::Vec<Emission>,
  /// DECLARED to be allowed to differ: the lex frontier and the cache depth run ahead when the
  /// caller prefetched. Recorded so a failure message can show them, never asserted equal.
  offset: usize,
  cache_len: usize,
}

/// One cell of the matrix: a token stream, and the trip (if any) it is built to provoke.
struct MatrixCell {
  name: &'static str,
  src: &'static str,
  /// The token limit; `usize::MAX` unless the cell trips it mid-skip.
  limit: usize,
  /// The span whose diagnostic the emitter rejects, if the cell trips a fatal emitter.
  fatal_at: Option<SimpleSpan>,
  /// Tokens consumed with `next` before the sync, so the call starts from a non-zero committed
  /// position. Both runs consume them identically; only the peek that follows differs.
  consume_first: usize,
  /// How many tokens the cached runs peek before syncing. Every one is compared against the
  /// uncached run of the same cell.
  prefills: &'static [usize],
  /// Entry points this cell is KNOWN to diverge on. The main matrix skips exactly these pairs
  /// and [`cache_transparency_known_divergences`] drives them instead, so the divergence is
  /// parked and named, never silently accepted.
  ///
  /// **Empty for every cell.** The family has one skip-and-report path and one match settle for
  /// cached and lexed tokens alike, so there is no divergence left to park — and
  /// [`cache_transparency_known_divergences`] now enforces that the parking lot stays empty.
  diverges: &'static [Entry],
}

/// The scenario axis. `;` is the sync point throughout, so one predicate drives every cell; the
/// stream decides what the scan meets on the way there. Single-space separation puts token `i`
/// at `[2i, 2i+1)`.
///
/// A cached run may peek at most 3 tokens (`DefaultCache` is `U3`), and its prefill must not
/// itself trip the emitter — `fatal_on_lexer_error` therefore stops at 2, one short of the `@`.
const CELLS: &[MatrixCell] = &[
  // The predicate matches the very first token: the `to`/balanced modes settle before it and the
  // `through` mode consumes it, all without a single skip.
  MatrixCell {
    name: "match_immediate",
    src: "; 1 2 3",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // Three skips, then the match — so the cached runs split the SAME skip run across the drain and
  // the loop at three different points.
  MatrixCell {
    name: "match_after_3_skips",
    src: "1 2 3 ; 4",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // No sync point at all: the scan runs to end of input. `sync_to` keeps the diagnosed progress;
  // `sync_through`/`sync_balanced` rewind the whole call — INCLUDING the drained cache prefix.
  MatrixCell {
    name: "never_matches_runs_to_eof",
    src: "1 2 3 4",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // THE FATAL TRIP ON A SKIPPED TOKEN'S DIAGNOSTIC. `2` is rejected: at prefill 2 and 3 it trips
  // inside the cache drain, at prefill 1 and 0 inside the `sync_with` loop. The two paths must
  // leave the input in the same place — the finding this matrix was built for.
  MatrixCell {
    name: "fatal_emitter_on_skipped_token",
    src: "1 2 3 ; 4",
    limit: usize::MAX,
    fatal_at: Some(SimpleSpan { start: 2, end: 3 }),
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // The fatal trip on a LEXER ERROR instead. A lexer error is never cached, so this can only ever
  // trip in the loop — but the cached runs still reach it with a drained prefix behind them.
  MatrixCell {
    name: "fatal_emitter_on_lexer_error",
    src: "1 2 @ 3 ; 4",
    limit: usize::MAX,
    fatal_at: Some(SimpleSpan { start: 4, end: 5 }),
    consume_first: 0,
    prefills: &[1, 2],
    diverges: &[],
  },
  // A sticky limit trip mid-skip: the 3rd scanned token trips. At prefill 3 the PEEK trips and
  // latches the boundary; at prefill 1/2 and uncached the sync's own scan does. Both must commit
  // the diagnosed prefix at the same durable frontier.
  MatrixCell {
    name: "limit_trip_mid_skip",
    src: "1 2 3 4 5 ;",
    limit: 2,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // A lexer error crossed mid-skip, non-fatally. At prefill 3 the sync point itself lands in the
  // cache BEHIND the crossed error, so the drain answers the whole call.
  MatrixCell {
    name: "lexer_error_crossed_mid_skip",
    src: "1 @ 2 ; 3",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // A crossed lexer error AND a no-match run to end of input: the `through`/balanced rewind must
  // unwind the drained prefix's diagnostics while leaving the peek's own error report — and the
  // restored watermark must still report that error exactly once overall.
  MatrixCell {
    name: "lexer_error_then_eof_rewind",
    src: "1 @ 2",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2],
    diverges: &[],
  },
  // The skip run starts from a non-zero committed position: `1` is consumed first, so the drain
  // and the loop meet a stream that is already part-read.
  MatrixCell {
    name: "skips_after_a_consume",
    src: "1 2 3 ; 4",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 1,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // A ZERO-SKIP sync from a non-zero committed position — the sync point is the very next token.
  //
  // The cell that caught the settle divergence: `sync_balanced`'s zero-skip `Hole` is anchored at
  // `cursor()`, and `cursor()` reads the cache — it is the front cached token's START when
  // something is cached and the committed span's END otherwise. While the family had two match
  // settles they left DIFFERENT cache states behind — the drain stopped with the match at the cache
  // front, the loop lexed the match, settled before it and left the cache empty — so the hole
  // landed on the token's start after a peek and on the previous token's end without one: a
  // RETURNED VALUE that moved with the lookahead depth. One settle now leaves the match unconsumed
  // at the cache front on both origins, so the cursor is the match's start either way.
  MatrixCell {
    name: "zero_skip_after_a_consume",
    src: "1 ; 2 3",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 1,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // ── The trivia cells: `padded`'s five stop shapes ──────────────────────────────────────────
  //
  // `~` is the one trivia kind, so these are the cells where `padded`'s own predicate (`is_trivia`)
  // has a run to skip and its two skips actually scan. The sync entries and `skip_while` drive them
  // too, from the same `;` sync point.
  //
  // Trivia BOTH SIDES: the leading skip drops `~`, the parse takes `1`, the trailing skip drops the
  // second `~` and stops before `;`. The stop shape is "stops after k".
  MatrixCell {
    name: "trivia_padding_both_sides",
    src: "~ 1 ~ ;",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // Trivia STOPS IMMEDIATELY: no leading trivia at all, so the leading skip stops on the very first
  // token it looks at — the zero-skip shape, on both origins.
  MatrixCell {
    name: "trivia_absent_stops_immediately",
    src: "1 ~ ;",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // Trivia RUNS TO EOF: everything is trivia, so `padded`'s leading skip consumes the whole input,
  // the inner parse sees end of input, and the trailing skip has nothing left.
  MatrixCell {
    name: "trivia_runs_to_eof",
    src: "~ ~ ~",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // A LEXER ERROR CROSSED by the trivia skip, non-fatally: `@` sits between the trivia and the
  // token the skip stops on, so the uncached run crosses it inside the skip's own scan while the
  // cached run's PEEK crossed it long before — and the two must still commit the same span, the
  // same lexer state, and the same cursor. (This is the shape that pinned the frontier's second
  // notion of "how far have we got": a skipped lexer error is not a token, and settling on it made
  // the committed span depend on who crossed it.)
  MatrixCell {
    name: "trivia_crosses_a_lexer_error",
    src: "~ @ 1 ;",
    limit: usize::MAX,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
  // A FATAL EMITTER trips on that crossed lexer error. A lexer error is never cached, so this can
  // only ever trip inside a scan — but the cached run reaches it with a drained trivia prefix
  // behind it. The prefill stops at 1: a deeper peek would cross `@` and trip the emitter during
  // the setup, and the two runs would not be comparable.
  MatrixCell {
    name: "trivia_fatal_on_a_lexer_error",
    src: "~ @ 1 ;",
    limit: usize::MAX,
    fatal_at: Some(SimpleSpan { start: 2, end: 3 }),
    consume_first: 0,
    prefills: &[1],
    diverges: &[],
  },
  // A LIMIT TRIP mid-trivia-run: the 3rd token trips. At prefill 3 the PEEK trips and latches the
  // boundary; at prefill 1/2 and uncached the leading skip's own scan does. Both must commit the
  // skipped trivia at the same durable frontier, and `padded` must then see the poisoned input.
  MatrixCell {
    name: "trivia_limit_trip_mid_run",
    src: "~ ~ ~ ~ ;",
    limit: 2,
    fatal_at: None,
    consume_first: 0,
    prefills: &[1, 2, 3],
    diverges: &[],
  },
];

/// The spans of a peeked window, in order.
fn peeked_spans<'inp, W>(peeked: &Peeked<'_, 'inp, BalLexer<'inp>, W>) -> std::vec::Vec<SimpleSpan>
where
  W: Window,
{
  (0..peeked.len()).map(|i| *peeked[i].span()).collect()
}

/// Runs one entry point over `inp`, normalizing its return into the shared [`Ret`] shape.
///
/// The sync predicate is the same for every cell (`;` is the sync point) and is instrumented: it
/// records the span of every token it is asked about, so the matrix can pin that a stateful
/// `FnMut` cannot tell the drain from the loop.
fn run_entry(
  entry: Entry,
  inp: &mut MatrixRef<'_, '_>,
  calls: &mut std::vec::Vec<SimpleSpan>,
) -> Ret {
  macro_rules! pred {
    () => {
      |t: Spanned<&BalTok, &SimpleSpan>| {
        calls.push(*t.span());
        matches!(t.data(), BalTok::Semi)
      }
    };
  }

  match entry {
    Entry::To => match inp.sync_to(pred!(), || None) {
      // The `to` family stops BEFORE the match and peeks it back.
      Ok(matched) => Ret {
        matched: matched.map(|t| (*t.span(), t.token().clone())),
        ..Ret::empty()
      },
      Err(e) => Ret::fatal(e),
    },
    Entry::ToThenPeekWithEmitter => {
      match inp.sync_to_then_peek_with_emitter::<_, _, W2>(pred!(), || None) {
        Ok((peeked, _)) => Ret {
          matched: (!peeked.is_empty()).then(|| (*peeked[0].span(), peeked[0].token().clone())),
          peeked: peeked_spans::<W2>(&peeked),
          ..Ret::empty()
        },
        Err(e) => Ret::fatal(e),
      }
    }
    Entry::Through => match inp.sync_through(pred!(), || None) {
      // The `through` family consumes the match and hands it over.
      Ok(matched) => Ret {
        matched: matched.map(Spanned::into_components),
        ..Ret::empty()
      },
      Err(e) => Ret::fatal(e),
    },
    Entry::ThroughThenPeek => match inp.sync_through_then_peek::<_, _, W2>(pred!(), || None) {
      Ok((matched, peeked)) => Ret {
        matched: matched.map(Spanned::into_components),
        peeked: peeked_spans::<W2>(&peeked),
        ..Ret::empty()
      },
      Err(e) => Ret::fatal(e),
    },
    Entry::ThroughThenPeekWithEmitter => {
      match inp.sync_through_then_peek_with_emitter::<_, _, W2>(pred!(), || None) {
        Ok((matched, peeked, _)) => Ret {
          matched: matched.map(Spanned::into_components),
          peeked: peeked_spans::<W2>(&peeked),
          ..Ret::empty()
        },
        Err(e) => Ret::fatal(e),
      }
    }
    Entry::Balanced => match inp.sync_balanced(parens, pred!()) {
      Ok(hole) => Ret {
        hole: hole.map(|h| (h.span(), h.skipped())),
        ..Ret::empty()
      },
      Err(e) => Ret::fatal(e),
    },
    // The trivia path, driven over the very same cells and the very same sync point: `skip_while`
    // stops on the token its predicate REJECTS, so the negated sync predicate makes it skip to
    // exactly where `sync_to` syncs to — a direct differential against the reporting twin, over
    // every stop shape the cells provoke (immediate, after k, end of input, across a lexer error,
    // into a limit trip, into a fatal emitter). The instrumented predicate rides along, so the
    // exactly-once law is pinned here too.
    Entry::SkipWhile => {
      let mut stop = pred!();
      match inp.skip_while(|t| !stop(t)) {
        Ok(()) => Ret::empty(),
        Err(e) => Ret::fatal(e),
      }
    }
    // `padded` — `skip_while(is_trivia)` on either side of a parser — driven through the real
    // combinator, so the cells cover the composite: the leading skip, the parse, the trailing
    // skip, and the parsed value itself (carried in `matched`). Its predicate is `is_trivia`, not
    // the cells' sync predicate, so `pred_calls` stays empty for it; the trivia cells are what give
    // it a run to skip.
    Entry::Padded => match TakeOne.padded().parse_input(inp) {
      Ok(parsed) => Ret {
        matched: parsed,
        ..Ret::empty()
      },
      Err(e) => Ret::fatal(e),
    },
  }
}

/// Runs one cell of the matrix once: prefill `prefill` tokens into the cache (0 = uncached), call
/// the entry point, then observe everything the caller could — including the token stream a retry
/// would see.
fn run_cell(cell: &MatrixCell, entry: Entry, prefill: usize) -> Obs {
  let mut emitter = MatrixEmitter::new(cell.fatal_at);
  let mut input = Input::<BalLexer<'_>, MatrixCtx<'_>, ()>::with_state_and_cache(
    cell.src,
    TokenLimiter::with_limitation(cell.limit),
    DefaultCache::<'_, BalLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut emitter);

  // Both runs read the same prefix, so the sync starts from the same committed position.
  for _ in 0..cell.consume_first {
    inp
      .next()
      .expect("the setup consume must not trip the emitter")
      .expect("the cell must have a token to consume first");
  }

  // The prefill is the ONLY difference between the two runs of a cell. A cell that would make its
  // own prefill trip the emitter is misconfigured — the two runs would not be comparable.
  match prefill {
    0 => {}
    1 => {
      inp
        .peek::<W1>()
        .expect("the prefill must not trip the emitter");
    }
    2 => {
      inp
        .peek::<W2>()
        .expect("the prefill must not trip the emitter");
    }
    3 => {
      inp
        .peek::<W3>()
        .expect("the prefill must not trip the emitter");
    }
    n => panic!("prefill {n} exceeds the U3 cache window"),
  }
  let setup_log = inp.emitter().log.clone();
  let entry_mark = setup_log.len();

  let mut pred_calls = std::vec::Vec::new();
  let ret = run_entry(entry, &mut inp, &mut pred_calls);

  let span = *inp.span();
  let tokens = inp.state().tokens();
  let poison = *inp.poison_boundary;
  let cursor = *inp.cursor().as_inner();
  let watermark = *inp.emitted_error_end;
  let offset = *inp.offset();
  let cache_len = inp.cache().len();
  let sync_log = inp.emitter().log[entry_mark..].to_vec();
  let sync_mark = inp.emitter().log.len();

  // THE RETRY. A recovering caller that catches a fatal emitter error — or simply carries on
  // after a failed sync — reads the stream from wherever the call left it. Draining it here folds
  // the cursor, the cache contents, the poison boundary and the dedup watermark into a single
  // observable, and it is precisely the observable the fatal-trip divergence broke.
  // `Ok(None)` (end of input or the poison boundary) and `Err` (a fatal emitter) both end the
  // drain; either way what was read is what a retry would have seen.
  let mut replay = std::vec::Vec::new();
  while let Ok(Some(tok)) = inp.next() {
    replay.push(tok.into_components());
  }
  let replay_log = inp.emitter().log[sync_mark..].to_vec();
  let watermark_drained = *inp.emitted_error_end;
  let poison_drained = *inp.poison_boundary;

  Obs {
    ret,
    span,
    tokens,
    poison,
    poison_drained,
    cursor,
    watermark,
    watermark_drained,
    pred_calls,
    setup_log,
    sync_log,
    replay,
    replay_log,
    offset,
    cache_len,
  }
}

/// Drops one occurrence of each `hoisted` entry from `base`, preserving order.
///
/// This is the exact — and only — carve-out the emission comparison grants a prefilled cache: a
/// lexer error the peek already reported is not reported a second time, because the dedup
/// watermark suppresses it. Nothing else may go missing, and nothing may be added.
fn without_hoisted(base: &[Emission], hoisted: &[Emission]) -> std::vec::Vec<Emission> {
  let mut pool = hoisted.to_vec();
  base
    .iter()
    .copied()
    .filter(|e| match pool.iter().position(|h| h == e) {
      Some(i) => {
        pool.remove(i);
        false
      }
      None => true,
    })
    .collect()
}

/// The differential assertion: the cached run and the uncached run of the same cell must be
/// observationally identical.
fn assert_cache_transparent(
  entry: Entry,
  cell: &MatrixCell,
  prefill: usize,
  uncached: &Obs,
  cached: &Obs,
) {
  let at = std::format!(
    "{} / {} / prefill={} (src {:?})",
    entry.name(),
    cell.name,
    prefill,
    cell.src
  );

  // Both runs ran the same setup consume, so the cached run's pre-call log EXTENDS the uncached
  // one; the tail is exactly what the prefill peek hoisted.
  assert!(
    cached.setup_log.starts_with(&uncached.setup_log),
    "[{at}] the two runs share their setup, so the cached pre-call log must extend the uncached \
     one (cached {:?}, uncached {:?})",
    cached.setup_log,
    uncached.setup_log
  );
  let hoisted = &cached.setup_log[uncached.setup_log.len()..];

  // ── What the call did ────────────────────────────────────────────────────
  assert_eq!(
    cached.ret, uncached.ret,
    "[{at}] the return value must not depend on the cache"
  );
  assert_eq!(
    cached.span, uncached.span,
    "[{at}] the committed span must not depend on the cache \
     (cached offset {} / cache {}, uncached offset {} / cache {})",
    cached.offset, cached.cache_len, uncached.offset, uncached.cache_len
  );
  assert_eq!(
    cached.tokens, uncached.tokens,
    "[{at}] the committed lexer state must not depend on the cache"
  );
  // The poison boundary is a fact about how far the input has been LEXED, not about what the call
  // did — and a prefill genuinely lexes further, so a peek deep enough to trip the token limiter
  // latches the boundary before the call under test even runs. (Every cell whose *scan* reaches the
  // trip therefore latches it in both runs, and lands in the first assertion below; only an entry
  // that stops short — `padded`, whose skips scan trivia and nothing else — can meet a cell whose
  // trip lies past where it ever looks.) So the law is stated over the three things that are the
  // scanner's and not the peek's:
  //
  //   1. WHERE it latched. A durable frontier is the end of the last durable token, which is a
  //      function of the token stream and the limit — never of who lexed it. Two runs that both
  //      latched must therefore name the SAME offset. This is the assertion `limit_trip_mid_skip`
  //      was built for: the sync's own trip and the peek's must agree on the frontier.
  //   2. That a prefetch may only find the trip EARLIER, never hide it: an uncached run that
  //      latched forces the cached run — which lexed at least as far — to have latched too.
  //   3. That nothing is lost. Once the stream is drained both runs have read exactly the same
  //      source, so the same limit trips at the same place: they must agree exactly.
  match (uncached.poison, cached.poison) {
    (Some(u), Some(c)) => assert_eq!(
      c, u,
      "[{at}] a latched poison boundary must name the same durable frontier, cached or not"
    ),
    (Some(u), None) => panic!(
      "[{at}] the cached run lexed at least as far, so it cannot have MISSED a limit trip the \
       uncached run latched (uncached {u}, cached none)"
    ),
    (None, _) => {}
  }
  assert_eq!(
    cached.poison_drained, uncached.poison_drained,
    "[{at}] once the stream is drained the poison boundary must agree"
  );
  // `cursor()` is DECLARED cache-dependent in general: its own contract says it "points to the
  // start of the first cached token" when one is cached and to the committed position otherwise,
  // and a plain `next()` already moves it differently depending on whether the next token was
  // peeked (the cached run has lexed across the intervening whitespace; the uncached one has not).
  // So the law here is the BOUNDED one — the cursor never precedes what the call committed, and
  // never passes the token the stream yields next — plus monotonicity: a prefetch may only SHARPEN
  // the resume point, never move it backwards. Both runs then denote the same next token, which the
  // replay above already pinned.
  for (label, obs) in [("uncached", uncached), ("cached", cached)] {
    assert!(
      obs.span.end <= obs.cursor,
      "[{at}] ({label}) the cursor must never precede the committed span end ({:?} vs {})",
      obs.span,
      obs.cursor
    );
    if let Some((next, _)) = obs.replay.first() {
      assert!(
        obs.cursor <= next.start,
        "[{at}] ({label}) the cursor must never pass the next token the stream yields ({} vs {:?})",
        obs.cursor,
        next
      );
    }
  }
  assert!(
    uncached.cursor <= cached.cursor,
    "[{at}] a prefetch may only sharpen the resume cursor, never move it back ({} < {})",
    cached.cursor,
    uncached.cursor
  );
  // …and for the scans that leave their stopping token unconsumed and report none of what they
  // skipped, the bounded law is not the law: the cursor is EXACTLY cache-independent. Both runs
  // leave the same token at the cache front (any deeper prefill sits behind it), and every
  // leftover-free exit is reached with the cache drained. `skip_while` — the trivia path, and so
  // `padded` — is the scan this pins: its cursor used to move with the caller's lookahead depth,
  // because a stopping token it had LEXED was thrown away while a cached one was kept. See
  // `Entry::pins_the_resume_cursor`.
  if entry.pins_the_resume_cursor() {
    assert_eq!(
      cached.cursor, uncached.cursor,
      "[{at}] the resume cursor must not depend on the cache: this scan leaves the token it \
       stopped on at the cache front whichever origin it came from \
       (cached cache {}, uncached cache {})",
      cached.cache_len, uncached.cache_len
    );
  }

  // The two DECLARED artifacts of a prefilled cache, pinned in the only direction they may move:
  // a peek lexes further ahead and holds more lookahead than the uncached run — never less. A
  // cached run that ended up SHORTER would mean the sync had thrown away tokens the caller had
  // already paid to lex.
  assert!(
    cached.offset >= uncached.offset,
    "[{at}] the lex frontier may only run ahead in the cached run ({} < {})",
    cached.offset,
    uncached.offset
  );
  assert!(
    cached.cache_len >= uncached.cache_len,
    "[{at}] the cached run may only hold more lookahead ({} < {})",
    cached.cache_len,
    uncached.cache_len
  );

  // A stateful `FnMut` predicate must not be able to tell the drain from the loop: same tokens,
  // same order, once each. (The previous round's bug — the prologue evaluating `pred` and
  // discarding the answer, so the loop asked again — shows up here as a repeated span.)
  assert_eq!(
    cached.pred_calls, uncached.pred_calls,
    "[{at}] the predicate must be asked about the same tokens, in the same order"
  );

  // ── What a retry then sees ───────────────────────────────────────────────
  assert_eq!(
    cached.replay, uncached.replay,
    "[{at}] a retry after the call must read the same token stream"
  );

  // ── What was diagnosed ───────────────────────────────────────────────────
  // A peek emits the lexer errors it crosses, and nothing else: it never diagnoses an unexpected
  // token and never reports a hole. So a prefill can only ever hoist a lexer-class entry, and the
  // carve-out below can never hide a missing sync diagnostic.
  assert!(
    hoisted.iter().all(Emission::is_lexer_class),
    "[{at}] a peek may only emit lexer errors, got {hoisted:?}"
  );
  assert_eq!(
    cached.sync_log,
    without_hoisted(&uncached.sync_log, hoisted),
    "[{at}] the call's own diagnostics must not depend on the cache \
     (uncached {:?}, prefill already reported {hoisted:?})",
    uncached.sync_log
  );
  assert_eq!(
    cached.replay_log,
    without_hoisted(&uncached.replay_log, hoisted),
    "[{at}] the retry's diagnostics must not depend on the cache \
     (uncached {:?}, prefill already reported {hoisted:?})",
    uncached.replay_log
  );

  // Exactly once, across the whole run: nothing the prefill hoisted is lost, nothing is doubled.
  let mut all_uncached = uncached.setup_log.clone();
  all_uncached.extend_from_slice(&uncached.sync_log);
  all_uncached.extend_from_slice(&uncached.replay_log);
  all_uncached.sort();
  let mut all_cached = cached.setup_log.clone();
  all_cached.extend_from_slice(&cached.sync_log);
  all_cached.extend_from_slice(&cached.replay_log);
  all_cached.sort();
  assert_eq!(
    all_cached, all_uncached,
    "[{at}] every diagnostic of the run must be reported exactly once, cached or not"
  );

  // The dedup watermark. Immediately after the call it may run AHEAD in the cached run — the peek
  // genuinely read further — but it may never LAG, which would let a reported error be reported
  // again. Once the stream is drained the two must agree exactly.
  assert!(
    cached.watermark >= uncached.watermark,
    "[{at}] the dedup watermark must never lag the uncached run ({} < {})",
    cached.watermark,
    uncached.watermark
  );
  assert_eq!(
    cached.watermark_drained, uncached.watermark_drained,
    "[{at}] once the stream is drained the dedup watermark must agree"
  );
}

/// Every (entry point x scenario x prefill) triple — and every scanner is transparent on all of
/// them.
///
/// There are **no exclusions**: `cell.diverges` is empty for every cell, so the `continue` below
/// never fires and every triple is asserted here, in the default suite.
/// [`cache_transparency_known_divergences`] holds the mechanism that would park one, and
/// enforces that nothing ever is.
#[test]
fn cache_transparency_matrix() {
  for cell in CELLS {
    for &entry in Entry::ALL {
      if cell.diverges.contains(&entry) {
        continue;
      }
      let uncached = run_cell(cell, entry, 0);
      for &prefill in cell.prefills {
        let cached = run_cell(cell, entry, prefill);
        assert_cache_transparent(entry, cell, prefill, &uncached, &cached);
      }
    }
  }
}

/// The parking lot for cells a scanner is NOT transparent on — **and it is empty**.
///
/// It was not always. While each scanner had two implementations of "take a token and act on it" —
/// a cache-drain prologue and a lexing loop — nothing forced them to settle the stopping token the
/// same way, and they did not: the drain stopped with that token still at the cache front, while
/// the loop lexed it, settled before it, and threw it away. `sync_balanced` anchors its zero-skip
/// [`Hole`](crate::input::Hole) at `cursor()`, which reads the cache, so the same call returned a
/// different hole depending on how deep the caller had peeked — a returned value moving with the
/// lookahead depth, which no contract states. `skip_while` had the identical split and leaked it
/// into the resume cursor instead of a return value. This test drove those cells, was expected to
/// fail, and was ignored to keep the suite green while the divergence stood.
///
/// There is now ONE loop over cached and lexed tokens alike, one skip-and-report path, and one
/// settle that leaves the stopping token unconsumed at the cache front whichever origin it came
/// from — so the divergence has nowhere to live, every cell is transparent, and
/// [`cache_transparency_matrix`] above drives all of them with no exclusions.
///
/// What is left here is the mechanism, kept honest in both directions: the parking lot must STAY
/// empty (a future divergence may not be quietly parked out of the main matrix), and anything ever
/// parked in it must still satisfy the very same assertions.
#[test]
fn cache_transparency_known_divergences() {
  for cell in CELLS {
    assert!(
      cell.diverges.is_empty(),
      "[{}] a cache divergence was parked out of the main matrix. The crate has one \
       skip-and-report path and one settle for cached and lexed tokens alike, so a \
       divergence is a defect in that path — fix it, do not park it: {:?}",
      cell.name,
      cell.diverges,
    );
    for &entry in cell.diverges {
      let uncached = run_cell(cell, entry, 0);
      for &prefill in cell.prefills {
        let cached = run_cell(cell, entry, prefill);
        assert_cache_transparent(entry, cell, prefill, &uncached, &cached);
      }
    }
  }
}

// ── The CST event channel joins the oracles (`rowan`) ─────────────────────────
//
// The auto-emission hook rides the settle surfaces, so the event channel inherits every
// law those surfaces already answer for — and must prove it against the SAME oracles:
// the cache-transparency matrix gains an event column (exact equality of materialized
// trees, cached vs uncached, every entry × cell × prefill — found/eof/trip/fatal shapes
// included), and the no-trace suite gains an event count (failed syncs leave ZERO
// events; trip/fatal arms keep their settled prefix — that is trip-commit, not a leak).
// Trees, not raw buffers, are the compared observable: mark nonces, truncation eras, and
// Diag-slot interleavings are route-dependent by design.
//
// The event column drives the LOSSLESS TWIN of the fixture ([`LosslessBalTok`]), not the
// syntactic `BalTok`: the lossless (`gap_kind`) sink now structurally refuses a
// trivia-skipping lexer at compile time, so the event oracles need a lexer that surfaces
// every byte. The diagnostics matrix proper keeps the syntactic `BalTok` and its
// offset-discontinuity teeth (the skip between tokens is what makes a resume-cursor bug
// visible), so nothing in that suite changes.
//
// Two premises shift under the twin (accepted, no code change): the zero-skip premise of
// `zero_skip_after_a_consume` and the peek-trips-at-W3 premise of the two limit cells hold
// only for the syntactic fixture. The event column does not lean on them — it pins
// cache-transparency (cached tree == uncached tree per cell × entry × prefill) — while the
// syntactic diagnostics matrix continues to pin those shape premises.

#[cfg(feature = "rowan")]
mod cst_event_oracles {
  use super::*;
  use crate::cst::{Sink, event::Event};

  const K_ROOT: u16 = 1;
  const K_ERR: u16 = 90;
  const K_GAP: u16 = 91;

  /// The lossless twin of [`BalTok`]: the same vocabulary over the same sources, but
  /// whitespace SURFACES as a real `Ws` token instead of dying in a lexer-level `skip` —
  /// the fixture a lossless (`gap_kind`) sink structurally requires. `SURFACES_TRIVIA =
  /// true` is honest here: every source byte becomes a token or a reported lexer error.
  ///
  /// The `Ws` rule deliberately does NOT tick the limiter: cell `limit`s keep the same
  /// real-token meaning they have in the syntactic matrix, so every cell stays valid
  /// without a per-cell audit. (`~` remains a counted, real trivia token — the `padded`
  /// entries still have a counted kind to skip.)
  #[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
  #[logos(crate = crate::logos, extras = TokenLimiter)]
  enum LosslessBalTok {
    #[regex(r"[0-9]+", |lex| { lex.extras.increase(); })]
    Num,
    #[token("(", |lex| { lex.extras.increase(); })]
    LParen,
    #[token(")", |lex| { lex.extras.increase(); })]
    RParen,
    #[token(";", |lex| { lex.extras.increase(); })]
    Semi,
    #[token("~", |lex| { lex.extras.increase(); })]
    Trivia,
    #[regex(r"[ \t\r\n]+")]
    Ws,
  }

  impl core::fmt::Display for LosslessBalTok {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str(match self {
        Self::Num => "number",
        Self::LParen => "`(`",
        Self::RParen => "`)`",
        Self::Semi => "`;`",
        Self::Trivia => "trivia",
        Self::Ws => "whitespace",
      })
    }
  }

  impl Token<'_> for LosslessBalTok {
    type Kind = BalKind;
    type Error = ByValErr;

    const SURFACES_TRIVIA: bool = true;

    fn kind(&self) -> BalKind {
      match self {
        Self::Num => BalKind::Num,
        Self::LParen => BalKind::LParen,
        Self::RParen => BalKind::RParen,
        Self::Semi => BalKind::Semi,
        Self::Trivia => BalKind::Trivia,
        // Ws conflates to the trivia kind: both are trivia-category to the parser, and it
        // lets `parens` (fn(&BalKind) -> Balance<char>) be reused verbatim.
        Self::Ws => BalKind::Trivia,
      }
    }

    fn is_trivia(&self) -> bool {
      matches!(self, Self::Trivia | Self::Ws)
    }
  }

  type LosslessBalLexer<'a> = LogosLexer<'a, LosslessBalTok>;

  fn map_ll(t: &LosslessBalTok) -> u16 {
    match t {
      LosslessBalTok::Num => 10,
      LosslessBalTok::LParen => 11,
      LosslessBalTok::RParen => 12,
      LosslessBalTok::Semi => 13,
      LosslessBalTok::Trivia => 14,
      // Distinct from `~` (14) so trees show which trivia was which; 15 collides with
      // nothing (K_ERR 90 / K_GAP 91 / K_ROOT 1).
      LosslessBalTok::Ws => 15,
    }
  }

  type EvtSink<'a> = Sink<'a, LosslessBalLexer<'a>, MatrixEmitter>;
  type EvtCtx<'a> = (EvtSink<'a>, DefaultCache<'a, LosslessBalLexer<'a>>);
  type EvtRef<'inp, 'c> = InputRef<'inp, 'c, LosslessBalLexer<'inp>, EvtCtx<'inp>, ()>;

  fn evt_sink<'a>(fatal_at: Option<SimpleSpan>) -> EvtSink<'a> {
    Sink::new(MatrixEmitter::new(fatal_at), map_ll, K_ERR, K_GAP)
  }

  /// The committed spans of the buffered `Token` events, in order.
  fn token_event_spans(sink: &EvtSink<'_>) -> std::vec::Vec<SimpleSpan> {
    sink
      .events()
      .iter()
      .filter_map(|ev| match ev {
        Event::Token { span, .. } => Some(*span),
        _ => None,
      })
      .collect()
  }

  /// The emitter error the sink context surfaces, spelled through the associated-type
  /// chain the [`ParseInput`] signature demands (it is `ByValErr`, but the trait will
  /// not take the shortcut).
  type EvtErr<'inp> =
    <<EvtCtx<'inp> as crate::ParseContext<'inp, LosslessBalLexer<'inp>, ()>>::Emitter as Emitter<
      'inp,
      LosslessBalLexer<'inp>,
      (),
    >>::Error;

  /// `padded`'s inner parser over the sink context (the event twin of the matrix's
  /// `TakeOne`).
  struct TakeOneCst;

  impl<'inp>
    ParseInput<'inp, LosslessBalLexer<'inp>, Option<(SimpleSpan, LosslessBalTok)>, EvtCtx<'inp>, ()>
    for TakeOneCst
  {
    fn parse_input(
      &mut self,
      inp: &mut EvtRef<'inp, '_>,
    ) -> Result<Option<(SimpleSpan, LosslessBalTok)>, EvtErr<'inp>> {
      Ok(inp.next()?.map(Spanned::into_components))
    }
  }

  /// Runs one entry point over the sink context, discarding the return — the matrix
  /// proper already pins returns and diagnostics; this column pins the event channel.
  fn run_entry_cst(entry: Entry, inp: &mut EvtRef<'_, '_>) {
    let pred = |t: Spanned<&LosslessBalTok, &SimpleSpan>| matches!(t.data(), LosslessBalTok::Semi);
    match entry {
      Entry::To => {
        let _ = inp.sync_to(pred, || None);
      }
      Entry::ToThenPeekWithEmitter => {
        let _ = inp.sync_to_then_peek_with_emitter::<_, _, W2>(pred, || None);
      }
      Entry::Through => {
        let _ = inp.sync_through(pred, || None);
      }
      Entry::ThroughThenPeek => {
        let _ = inp.sync_through_then_peek::<_, _, W2>(pred, || None);
      }
      Entry::ThroughThenPeekWithEmitter => {
        let _ = inp.sync_through_then_peek_with_emitter::<_, _, W2>(pred, || None);
      }
      Entry::Balanced => {
        let _ = inp.sync_balanced(parens, pred);
      }
      Entry::SkipWhile => {
        let _ = inp.skip_while(|t| !pred(t));
      }
      Entry::Padded => {
        let _ = TakeOneCst.padded().parse_input(inp);
      }
    }
  }

  /// One cell run under the recording sink: setup consume, prefill, the entry, the
  /// retry drain, then materialization.
  fn run_cell_cst(cell: &MatrixCell, entry: Entry, prefill: usize) -> rowan::GreenNode {
    let mut sink = evt_sink(cell.fatal_at);
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      cell.src,
      TokenLimiter::with_limitation(cell.limit),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut sink);

    for _ in 0..cell.consume_first {
      inp
        .next()
        .expect("the setup consume must not trip the emitter")
        .expect("the cell must have a token to consume first");
    }
    match prefill {
      0 => {}
      1 => {
        inp
          .peek::<W1>()
          .expect("the prefill must not trip the emitter");
      }
      2 => {
        inp
          .peek::<W2>()
          .expect("the prefill must not trip the emitter");
      }
      3 => {
        inp
          .peek::<W3>()
          .expect("the prefill must not trip the emitter");
      }
      n => panic!("prefill {n} exceeds the U3 cache window"),
    }

    run_entry_cst(entry, &mut inp);

    // The retry drain: fold everything a recovering caller would then read into the
    // final timeline, so the compared trees cover the whole stream.
    while let Ok(Some(_)) = inp.next() {}

    drop(inp);
    drop(input);
    // The door matches the run's terminal class. {early-halting cells} =
    // {limit cells} ∪ {fatal cells}, exactly (Rev 1 §A): the retry-drain stops only on
    // Ok(None)=EOF or Err=fatal-verdict, so the frontier reaches EOF for every entry iff the
    // lexer cannot poison (limit == MAX) AND no diagnostic can be rejected (fatal_at == None).
    // Everything else may leave an un-lexed tail — the finish_partial domain — for at least
    // one entry, and finish_partial is a byte-identical superset for the entries that do drain.
    let (green, _emitter) = if cell.limit == usize::MAX && cell.fatal_at.is_none() {
      sink.finish(K_ROOT, cell.src)
    } else {
      sink.finish_partial(K_ROOT, cell.src)
    };
    green.unwrap_or_else(|e| {
      panic!(
        "[{} / {}] the settle-driven buffer must materialize: {e:?}",
        entry.name(),
        cell.name
      )
    })
  }

  /// The matrix event column: cached and uncached runs of every (entry × cell ×
  /// prefill) triple materialize **identical** green trees — the token-event stream is
  /// exactly invariant under prefill (stronger than diagnostics, which may hoist).
  #[test]
  fn cache_transparency_matrix_event_column() {
    for cell in CELLS {
      for &entry in Entry::ALL {
        let uncached = run_cell_cst(cell, entry, 0);
        for &prefill in cell.prefills {
          let cached = run_cell_cst(cell, entry, prefill);
          assert_eq!(
            cached,
            uncached,
            "[{} / {} / prefill={}] the event channel must be exactly cache-transparent \
             (materialized trees diverged)",
            entry.name(),
            cell.name,
            prefill
          );
        }
      }
    }
  }

  /// T1 — the drained-cache failed sync: a no-match `sync_through` that drained a
  /// prefilled cache rewinds to exactly its entry mark, so **zero** events survive the
  /// failure — the no-trace law's event column.
  #[test]
  fn t1_failed_sync_over_drained_cache_leaves_zero_events() {
    let mut sink = evt_sink(None);
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      "1 2 3",
      TokenLimiter::with_limitation(usize::MAX),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut sink);

    inp.peek::<W2>().expect("prefill is clean");
    let matched = inp
      .sync_through(|t| matches!(t.data(), LosslessBalTok::Semi), || None)
      .expect("verbose-shaped emitter collects");
    assert!(matched.is_none(), "no sync point exists");
    assert_eq!(
      inp.emitter().events().len(),
      0,
      "the failed sync rewound to its entry mark: no event of any kind survives (T1)"
    );

    // The exactly-once pairing: the re-lex after the rewind settles each token once.
    while let Ok(Some(_)) = inp.next() {}
    assert_eq!(
      token_event_spans(inp.emitter()),
      &[
        SimpleSpan { start: 0, end: 1 },
        SimpleSpan { start: 1, end: 2 },
        SimpleSpan { start: 2, end: 3 },
        SimpleSpan { start: 3, end: 4 },
        SimpleSpan { start: 4, end: 5 }
      ],
      "the re-lex re-emits exactly once per token (T1/T2 pairing)"
    );
  }

  /// T2 — exactly-once per final timeline: an attempt that consumed across a lexer
  /// error and declined leaves zero events; the committed re-parse settles every token
  /// exactly once, and the tree equals the straight drive's byte for byte. No watermark
  /// analog exists on the event channel — token events are never emitted early.
  #[test]
  fn t2_reconsume_after_decline_emits_exactly_once() {
    let src = "1 @ 2";
    let drain = |inp: &mut EvtRef<'_, '_>| while let Ok(Some(_)) = inp.next() {};

    // The straight drive.
    let mut straight = evt_sink(None);
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      src,
      TokenLimiter::with_limitation(usize::MAX),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut straight);
    drain(&mut inp);
    drop(inp);
    drop(input);
    let (straight_green, straight_emitter) = straight.finish(K_ROOT, src);

    // The decline-then-reparse drive of the same final timeline.
    let mut sink = evt_sink(None);
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      src,
      TokenLimiter::with_limitation(usize::MAX),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut sink);
    let declined: Option<()> = inp.attempt(|inp2| {
      // Consume across the lexer error: four settles (`1`, the leading Ws, the Ws past the
      // crossed `@`, then `2`) plus the crossed error's diagnostic.
      inp2.next().expect("collects").expect("1");
      inp2.next().expect("collects").expect("leading Ws");
      inp2
        .next()
        .expect("collects")
        .expect("Ws after the crossed @");
      inp2.next().expect("collects").expect("2");
      None
    });
    assert!(declined.is_none());
    assert_eq!(
      inp.emitter().events().len(),
      0,
      "the decline rewound every event with the branch (T2)"
    );
    drain(&mut inp);
    assert_eq!(
      token_event_spans(inp.emitter()),
      &[
        SimpleSpan { start: 0, end: 1 },
        SimpleSpan { start: 1, end: 2 },
        SimpleSpan { start: 3, end: 4 },
        SimpleSpan { start: 4, end: 5 }
      ],
      "each committed token settles exactly once on the final timeline (T2)"
    );
    drop(inp);
    drop(input);
    let (green, emitter) = sink.finish(K_ROOT, src);

    assert_eq!(
      green.expect("balanced"),
      straight_green.expect("balanced"),
      "the re-parsed timeline materializes the straight drive's tree, byte for byte"
    );
    assert_eq!(
      emitter.log, straight_emitter.log,
      "and the diagnostic timeline agrees too: the crossed error is reported exactly once"
    );
  }

  /// T4 — the zero-skip sync is event-silent: nothing settles, so nothing may emit —
  /// the whole event buffer (token events AND diagnostic slots) is untouched.
  #[test]
  fn t4_zero_skip_sync_emits_nothing() {
    let mut sink = evt_sink(None);
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      "1; 2",
      TokenLimiter::with_limitation(usize::MAX),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut sink);

    inp.next().expect("collects").expect("1");
    let before = inp.emitter().events().len();
    assert_eq!(token_event_spans(inp.emitter()).len(), 1);

    // The sync point is the very next token: a zero-skip sync settles nothing (the
    // returned hole describes an empty region; its one-per-hole diagnostic is never
    // emitted for zero skips, so the emitter sees no traffic at all).
    let hole = inp
      .sync_balanced(parens, |t| matches!(t.data(), LosslessBalTok::Semi))
      .expect("collects");
    assert_eq!(
      hole.map(|h| h.skipped()),
      Some(0),
      "the zero-skip sync describes an empty hole"
    );
    assert_eq!(
      inp.emitter().events().len(),
      before,
      "the zero-skip sync left the event buffer untouched (T4)"
    );

    // The stopper's settle is its real consume.
    inp.next().expect("collects").expect(";");
    assert_eq!(token_event_spans(inp.emitter()).len(), 2);
  }

  /// The trip/fatal arms of the no-trace suite, event column: committed progress keeps
  /// its settled tokens' events — that is trip-commit, not a leak.
  #[test]
  fn trip_and_fatal_sync_arms_keep_settled_events() {
    // A sticky limit trip mid-skip: the two durable skipped tokens' events persist.
    let mut sink = evt_sink(None);
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      "1 2 3 4 5 ;",
      TokenLimiter::with_limitation(2),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut sink);
    let matched = inp
      .sync_through(|t| matches!(t.data(), LosslessBalTok::Semi), || None)
      .expect("a trip is not a fatal exit");
    assert!(
      matched.is_none(),
      "the trip ends the scan before the sync point"
    );
    assert_eq!(
      token_event_spans(inp.emitter()),
      &[
        SimpleSpan { start: 0, end: 1 },
        SimpleSpan { start: 1, end: 2 },
        SimpleSpan { start: 2, end: 3 },
        SimpleSpan { start: 3, end: 4 }
      ],
      "the diagnosed prefix's settles persist across the trip (trip-commit)"
    );
    drop(inp);
    drop(input);

    // A fatal rejection of a skipped token's diagnostic: the token settled BEFORE the
    // verdict, so its event rides out with the committed position.
    let mut sink = evt_sink(Some(SimpleSpan { start: 2, end: 3 }));
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      "1 2 3 ; 4",
      TokenLimiter::with_limitation(usize::MAX),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut sink);
    let res = inp.sync_to(|t| matches!(t.data(), LosslessBalTok::Semi), || None);
    assert!(res.is_err(), "the emitter rejects token 2's diagnostic");
    assert_eq!(
      token_event_spans(inp.emitter()),
      &[
        SimpleSpan { start: 0, end: 1 },
        SimpleSpan { start: 1, end: 2 },
        SimpleSpan { start: 2, end: 3 }
      ],
      "event and commit stay paired on the fatal exit: the reported token settled first"
    );
  }
}
