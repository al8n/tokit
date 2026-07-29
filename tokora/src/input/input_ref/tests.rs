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
pub(super) enum ByValErr {
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

// The `*_or_stop` primitives raise the committed end-of-input error where their plain siblings
// decline, so a fixture that exercises both needs the conversion.
impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for ByValErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
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

/// A widen-then-drain round (`peek::<U1>` → `peek::<U2>` → `peek::<U3>`, then consume the
/// window) against a by-value limit of 2, at the shape and the numbers that used to let three
/// times the configured budget through.
///
/// Each top-level `peek` that widens an already-nonempty cache builds a fresh lexer, and the
/// pair it resumes from is the whole question. Pairing the COMMITTED state with the retained
/// run's end offset re-fills token 2 as though token 1 had never been scanned — nothing was
/// consumed in between, so the committed state has not moved — and consuming then adopts each
/// entry's own wrongly-resumed state, so the by-value limiter's committed count silently falls
/// behind the number of tokens actually consumed, round after round. Resuming from the newest
/// retained token's own stored state instead makes the tally exact, so the limit binds at 2
/// however the caller widened.
///
/// This is the by-value counterpart to `ProbeLimiter`'s `Rc`-shared tally: a shared tally can
/// never go "missing" the way committed STATE can, so it cannot see this class of bug.
#[test]
fn widen_then_drain_holds_the_by_value_limit() {
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
    assert_eq!(inp.peek::<U1>().map(|w| w.len()), Ok(1), "U1 fill");
    assert_eq!(inp.peek::<U2>().map(|w| w.len()), Ok(2), "U2 fill");
    // The third step lexes token 3 under token 2's post-state, which is where the tally
    // reaches the limit: the fill trips, latches, and comes back short.
    assert_eq!(
      inp.peek::<U3>().map(|w| w.len()),
      Ok(2),
      "the third token trips the limit, so the widening fill stops short"
    );
    while inp.next().unwrap().is_some() {
      consumed += 1;
    }
    // There is no second round: the boundary the trip latched holds the drain at the same
    // point the fill stopped.
    assert_eq!(
      inp.peek::<U1>().map(|w| w.len()),
      Ok(0),
      "the latched boundary empties every later fill"
    );
    assert!(inp.next().unwrap().is_none());
  }

  let limit_diags: usize = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();

  assert_eq!(
    consumed, 2,
    "a limit of 2 stops consumption at 2 tokens, whatever the lookahead pattern"
  );
  assert_eq!(
    limit_diags, 1,
    "the limit trip is diagnosed exactly once, at the third token"
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
pub(super) enum BalTok {
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
pub(super) enum BalKind {
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

pub(super) type BalLexer<'a> = LogosLexer<'a, BalTok>;
type BalVerboseCtx<'a> = (Verbose<ByValErr>, DefaultCache<'a, BalLexer<'a>>);
type BalFatalCtx<'a> = (
  crate::emitter::Fatal<ByValErr>,
  DefaultCache<'a, BalLexer<'a>>,
);

/// The parenthesis pair table: `(` opens, `)` closes, everything else is neutral.
pub(super) fn parens(kind: &BalKind) -> Balance<char> {
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

/// The mark is the log length: a reading of the emission state, keyed on no table.
impl crate::emitter::ValueKeyedEmitter for MatrixEmitter {}

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

// ── The resume frontier: one bundled (state, offset) pair ─────────────────────
//
// `InputRef` builds a temporary lexer per operation, and the two facts it resumes from —
// the lexer state, and the byte offset to bump it to — describe one point in time. They are
// not independent: the state IS a function of the tokens lexed up to that offset. Reading
// the offset from the newest retained token while reading the state from the committed
// field pairs a position with a state from before the retained run, so the token after that
// run is lexed under a stale tally.
//
// `BalLexer`'s by-value `TokenLimiter` is the oracle for exactly that, because the tally
// lives IN the lexer state and therefore rewinds with it. A widening lookahead
// (`U1` → `U2` → `U3`) is the shape that makes the two facts diverge: every step lexes one
// more token while the committed state stands still.
//
// `ProbeLimiter` cannot see any of this — its counter is shared through an `Rc<Cell>`, so a
// temporary lexer's increments outlive the state they were made under and a rewound tally
// is invisible. That is why these use `TokenLimiter`/`BalLexer`; do not "upgrade" them to
// the shared limiter.

/// The three peek call patterns a lookahead-equivalence oracle compares.
#[derive(Clone, Copy, Debug)]
enum Lookahead {
  /// No lookahead at all.
  None,
  /// One three-deep peek.
  Wide,
  /// A widening sequence of peeks — the pattern that separates the retained run's end from
  /// the committed state.
  Widening,
}

/// The observables of a full drain under `pattern`: the consumed spans, the final committed
/// tally, and the diagnostics, in order.
fn drain_under_lookahead(
  src: &str,
  limit: usize,
  pattern: Lookahead,
) -> (std::vec::Vec<SimpleSpan>, usize, std::vec::Vec<ByValErr>) {
  use generic_arraydeque::typenum::{U1, U2, U3};

  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<BalLexer<'_>, BalVerboseCtx<'_>, ()>::with_state_and_cache(
    src,
    TokenLimiter::with_limitation(limit),
    DefaultCache::<'_, BalLexer<'_>>::default(),
  );
  let (spans, tokens) = {
    let mut inp = input.as_ref(&mut emitter);
    match pattern {
      Lookahead::None => {}
      Lookahead::Wide => {
        let _ = inp.peek::<U3>().unwrap();
      }
      Lookahead::Widening => {
        let _ = inp.peek::<U1>().unwrap();
        let _ = inp.peek::<U2>().unwrap();
        let _ = inp.peek::<U3>().unwrap();
      }
    }
    let mut spans = std::vec::Vec::new();
    while let Some(tok) = inp.next().unwrap() {
      spans.push(*tok.span_ref());
    }
    (spans, inp.state().tokens())
  };
  let diagnostics = emitter.errors().values().flatten().cloned().collect();
  (spans, tokens, diagnostics)
}

#[test]
fn widening_peek_does_not_resume_under_stale_state() {
  use generic_arraydeque::typenum::{U1, U2, U3};

  // "1 2 3 4 5 6 7" behind a two-token limit. Every widening step lexes exactly one more
  // token, so the newest retained token must carry the tally of the whole retained run.
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<BalLexer<'_>, BalVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 6 7",
    TokenLimiter::with_limitation(2),
    DefaultCache::<'_, BalLexer<'_>>::default(),
  );
  let consumed = {
    let mut inp = input.as_ref(&mut emitter);

    assert_eq!(inp.peek::<U1>().unwrap().len(), 1);
    assert_eq!(
      inp.cache().back().unwrap().state().tokens(),
      1,
      "one token lexed, so the newest retained token carries a tally of one"
    );

    assert_eq!(inp.peek::<U2>().unwrap().len(), 2);
    assert_eq!(
      inp.cache().back().unwrap().state().tokens(),
      2,
      "the second token is lexed under the first token's post-state, so the tally is two"
    );

    // The third token would take the tally past the limit, so the fill trips instead of
    // caching it: the window comes back short and the boundary latches at the second
    // token's end.
    assert_eq!(
      inp.peek::<U3>().unwrap().len(),
      2,
      "the third token trips the limit, so the fill stops with a short window"
    );
    assert_eq!(
      inp.cache().back().unwrap().state().tokens(),
      2,
      "a tripped token never enters the cache, so the newest retained tally stays two"
    );

    let mut consumed = 0usize;
    while inp.next().unwrap().is_some() {
      consumed += 1;
    }
    consumed
  };

  assert_eq!(
    consumed, 2,
    "a two-token limit bounds the drain at two tokens however deep the caller peeked"
  );
  let limits = emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();
  assert_eq!(limits, 1, "the limit trip is diagnosed exactly once");
}

#[test]
fn resume_frontier_pairs_state_with_offset() {
  use generic_arraydeque::typenum::{U1, U2, U3};

  // The pairing law, read directly off the cache: the state a retained token carries is the
  // state that produced it, so a run of `k` retained tokens ends on a tally of `k`. Nothing
  // is consumed, so the committed tally stays at zero throughout — which is precisely why
  // resuming from it would be wrong.
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<BalLexer<'_>, BalVerboseCtx<'_>, ()>::with_state_and_cache(
    "1 2 3",
    TokenLimiter::new(),
    DefaultCache::<'_, BalLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut emitter);

  assert_eq!(inp.peek::<U1>().unwrap().len(), 1);
  assert_eq!(inp.cache().back().unwrap().state().tokens(), 1);

  assert_eq!(inp.peek::<U2>().unwrap().len(), 2);
  assert_eq!(
    inp.cache().back().unwrap().state().tokens(),
    2,
    "the second retained token was lexed under the first one's post-state"
  );

  assert_eq!(inp.peek::<U3>().unwrap().len(), 3);
  assert_eq!(
    inp.cache().back().unwrap().state().tokens(),
    3,
    "the third retained token was lexed under the second one's post-state"
  );

  assert_eq!(
    inp.state().tokens(),
    0,
    "a peek consumes nothing, so the committed tally never moves — resuming from it \
     would rewind the retained run"
  );
  assert_eq!(
    inp.offset(),
    &5,
    "the resume offset is the newest retained token's end"
  );
}

#[test]
fn lookahead_equivalence_over_call_patterns() {
  // The general oracle: for a deterministic lexer whose state accumulates monotonically,
  // the tokens a full drain yields, the final committed state and the diagnostic sequence
  // are functions of the token stream — never of how deep, or in how many steps, the caller
  // looked ahead first.
  let none = drain_under_lookahead("1 2 3 4 5 6 7", 2, Lookahead::None);
  let wide = drain_under_lookahead("1 2 3 4 5 6 7", 2, Lookahead::Wide);
  let widening = drain_under_lookahead("1 2 3 4 5 6 7", 2, Lookahead::Widening);

  assert_eq!(
    none, wide,
    "one wide peek before the drain must not change what the drain observes"
  );
  assert_eq!(
    none, widening,
    "a widening peek sequence before the drain must not change what the drain observes"
  );
  assert_eq!(
    none.0.len(),
    2,
    "the two-token limit is what bounds every one of the three programs"
  );
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

  /// The fixture's kind space: the synthetic root, `map_ll`'s six token images, and the two
  /// kinds the sink synthesizes.
  fn in_ll_kind_space(kind: u16) -> bool {
    matches!(kind, K_ROOT | 10..=15 | K_ERR | K_GAP)
  }

  fn evt_sink(src: &str, fatal_at: Option<SimpleSpan>) -> EvtSink<'_> {
    let profile = crate::cst::CstProfile::new(
      map_ll,
      crate::cst::KindValidator::new(in_ll_kind_space),
      K_ERR,
      K_GAP,
    );
    Sink::new(src, MatrixEmitter::new(fatal_at), profile)
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
    let mut sink = evt_sink(cell.src, cell.fatal_at);
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
      sink.finish(K_ROOT)
    } else {
      sink.finish_partial(K_ROOT)
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
    let mut sink = evt_sink("1 2 3", None);
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
    let mut straight = evt_sink(src, None);
    let mut input = Input::<LosslessBalLexer<'_>, EvtCtx<'_>, ()>::with_state_and_cache(
      src,
      TokenLimiter::with_limitation(usize::MAX),
      DefaultCache::<LosslessBalLexer<'_>>::default(),
    );
    let mut inp = input.as_ref(&mut straight);
    drain(&mut inp);
    drop(inp);
    drop(input);
    let (straight_green, straight_emitter) = straight.finish(K_ROOT);

    // The decline-then-reparse drive of the same final timeline.
    let mut sink = evt_sink(src, None);
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
    let (green, emitter) = sink.finish(K_ROOT);

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
    let mut sink = evt_sink("1; 2", None);
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
    let mut sink = evt_sink("1 2 3 4 5 ;", None);
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
    let mut sink = evt_sink("1 2 3 ; 4", Some(SimpleSpan { start: 2, end: 3 }));
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

// ═══════════════════════════════════════════════════════════════════════════════════
// R9 §4.1 — the scan's UNWIND edge
//
// `skip_until`'s loop pops the next token out of durable state (the parked slot, or the cache
// front) into a local with no `Drop`, runs caller code — the predicate, the expected-tokens
// closure, the frontier's `State: Clone`, the lexer — and only puts it back on a *return* exit.
// A panic through any of those windows is an exit the put-back never sees: with a warm cache
// the in-flight token and the whole skipped prefix leave the stream silently (R9-F2), and a
// rewinding mode's entry mark is neither rewound nor released.
//
// Fixture note. The lexer state carries TWO tallies on purpose. `harvested` is BY VALUE, so it
// advances only along the lineage whose state is actually kept — the in-tree `Rc`-shared
// `LimitTracker` aliases across every clone and therefore reads the same whether or not the
// frontier's state was harvested by `commit_at`, which makes a correct and an incorrect harvest
// observationally identical. `odometer` is shared on purpose: it is the *work* meter (every
// token any lexer ever scans), which is what a re-lex shows up on, and it backs the budget so a
// re-lex genuinely re-burns it — the gate's measured trip.
// ═══════════════════════════════════════════════════════════════════════════════════

thread_local! {
  /// `ScanTally::clone` calls since the last arm, and the call index that panics (0 = disarmed).
  static STATE_CLONES: Cell<usize> = const { Cell::new(0) };
  static STATE_CLONE_BOMB: Cell<usize> = const { Cell::new(0) };
}

/// Arms the `L::State: Clone` window: the `at`-th clone from now on panics.
fn arm_state_clone(at: usize) {
  STATE_CLONES.with(|c| c.set(0));
  STATE_CLONE_BOMB.with(|c| c.set(at));
}

fn disarm_state_clone() {
  STATE_CLONE_BOMB.with(|c| c.set(0));
}

#[derive(Debug)]
struct ScanTally {
  /// By-value: the number of tokens scanned along the state that is actually committed.
  harvested: usize,
  /// Shared: total tokens scanned by any lexer, restore-proof — the re-lex meter and the budget.
  odometer: Rc<Cell<usize>>,
  limit: usize,
}

impl Clone for ScanTally {
  fn clone(&self) -> Self {
    let n = STATE_CLONES.with(|c| {
      let v = c.get() + 1;
      c.set(v);
      v
    });
    assert!(
      n != STATE_CLONE_BOMB.with(Cell::get),
      "R9-F2: the armed `L::State` clone (#{n}) panics"
    );
    Self {
      harvested: self.harvested,
      odometer: self.odometer.clone(),
      limit: self.limit,
    }
  }
}

impl Default for ScanTally {
  fn default() -> Self {
    Self::with_limit(usize::MAX)
  }
}

impl ScanTally {
  fn with_limit(limit: usize) -> Self {
    Self {
      harvested: 0,
      odometer: Rc::new(Cell::new(0)),
      limit,
    }
  }

  fn odometer(&self) -> Rc<Cell<usize>> {
    self.odometer.clone()
  }

  fn bump(&mut self) {
    self.harvested += 1;
    self.odometer.set(self.odometer.get() + 1);
  }
}

#[derive(Debug, Clone, PartialEq)]
struct ScanLimitExceeded;

impl State for ScanTally {
  type Error = ScanLimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.odometer.get() > self.limit {
      Err(ScanLimitExceeded)
    } else {
      Ok(())
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
enum ScanErr {
  Lex,
  Limit,
}

impl From<()> for ScanErr {
  fn from(_: ()) -> Self {
    ScanErr::Lex
  }
}

impl From<ScanLimitExceeded> for ScanErr {
  fn from(_: ScanLimitExceeded) -> Self {
    ScanErr::Limit
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for ScanErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ScanErr::Lex
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for ScanErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    ScanErr::Lex
  }
}

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = ScanTally, skip r"[ \t\r\n]+")]
enum ScanTok {
  #[regex(r"[a-z0-9]+", |lex| { lex.extras.bump(); })]
  Word,
}

impl core::fmt::Display for ScanTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScanKind {
  Word,
}

impl core::fmt::Display for ScanKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

impl Token<'_> for ScanTok {
  type Kind = ScanKind;
  type Error = ScanErr;

  fn kind(&self) -> ScanKind {
    ScanKind::Word
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type ScanLexer<'a> = LogosLexer<'a, ScanTok>;

/// A **mark-keyed** emitter with a rewindable emission log: one live row per outstanding
/// capture, and each row remembers the log length at capture. `Verbose` cannot serve here — its
/// mark is a log length, so a stranded mark is invisible; and a pure row ledger cannot show that
/// a rewind restored the emissions. The unwind edge has to be judged on both at once.
#[derive(Debug, Default)]
struct ScanLedger {
  log: std::vec::Vec<crate::span::SimpleSpan>,
  next: Cell<u64>,
  live: core::cell::RefCell<std::vec::Vec<(u64, usize)>>,
}

impl ScanLedger {
  fn emissions(&self) -> usize {
    self.log.len()
  }

  fn live_rows(&self) -> usize {
    self.live.borrow().len()
  }
}

impl<'inp, L, Lang: ?Sized> crate::Emitter<'inp, L, Lang> for ScanLedger
where
  L: crate::Lexer<'inp, Span = crate::span::SimpleSpan>,
  <L::Token as Token<'inp>>::Error: Into<ScanErr>,
{
  type Error = ScanErr;

  fn emit_lexer_error(
    &mut self,
    err: crate::span::Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), ScanErr> {
    self.log.push(*err.span_ref());
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), ScanErr> {
    self.log.push(*err.span_ref());
    Ok(())
  }

  fn emit_error(&mut self, err: crate::span::Spanned<ScanErr, L::Span>) -> Result<(), ScanErr> {
    self.log.push(*err.span_ref());
    Ok(())
  }

  fn emit_skipped_region(&mut self, span: L::Span, _skipped: usize) -> Result<(), ScanErr> {
    self.log.push(span);
    Ok(())
  }

  fn checkpoint(&self) -> u64 {
    let id = self.next.get() + 1;
    self.next.set(id);
    self.live.borrow_mut().push((id, self.log.len()));
    id
  }

  fn rewind(&mut self, _cursor: &crate::input::Cursor<'inp, '_, L>, checkpoint: u64) {
    let at = {
      let mut live = self.live.borrow_mut();
      let at = live
        .iter()
        .find(|(id, _)| *id == checkpoint)
        .map(|(_, len)| *len);
      live.retain(|(id, _)| *id < checkpoint);
      at
    };
    if let Some(len) = at {
      self.log.truncate(len);
    }
  }

  fn release(&mut self, checkpoint: u64) {
    let mut live = self.live.borrow_mut();
    if let Some(pos) = live.iter().rposition(|(id, _)| *id == checkpoint) {
      live.remove(pos);
    }
  }
}

type ScanLedgerCtx<'a> = (ScanLedger, DefaultCache<'a, ScanLexer<'a>>);

/// What a cell reads back after a caught panic: the resume cursor, the tokens still reachable,
/// the total lexing work, the surviving emissions, and the emitter's outstanding marks.
#[derive(Debug, PartialEq, Eq)]
struct AfterUnwind {
  cursor: usize,
  drained: std::vec::Vec<(usize, usize)>,
  scans: usize,
  emissions: usize,
  live_rows: usize,
}

/// One `to`-shaped run: warm the cache to `warm` tokens if asked, then `sync_to` with a
/// predicate that panics on the `panic_on`-th token it is handed (1-based), and read the input
/// back. `exp_panics_on` does the same for the expected-tokens closure instead.
fn f2_sync_to_run(
  src: &'static str,
  warm: bool,
  panic_on: Option<usize>,
  exp_panics_on: Option<usize>,
  limit: usize,
) -> AfterUnwind {
  use generic_arraydeque::typenum::U3;

  let tally = ScanTally::with_limit(limit);
  let odometer = tally.odometer();
  let cache = DefaultCache::<'_, ScanLexer<'_>>::default();
  let mut emitter = ScanLedger::default();
  let mut input =
    Input::<ScanLexer<'_>, ScanLedgerCtx<'_>, ()>::with_state_and_cache(src, tally, cache);
  let mut inp = input.as_ref(&mut emitter);

  if warm {
    let _ = inp.peek::<U3>().unwrap();
  }

  let seen = Cell::new(0usize);
  let exp_seen = Cell::new(0usize);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.sync_to(
      |_| {
        let n = seen.get() + 1;
        seen.set(n);
        assert!(
          Some(n) != panic_on,
          "R9-F2: the predicate panics on token #{n}"
        );
        false
      },
      || {
        let n = exp_seen.get() + 1;
        exp_seen.set(n);
        assert!(
          Some(n) != exp_panics_on,
          "R9-F2: the expected-tokens closure panics on skip #{n}"
        );
        None
      },
    );
  }));
  assert!(caught.is_err(), "the scan must have panicked");

  let cursor = *inp.cursor().as_inner();
  let mut drained = std::vec::Vec::new();
  while let Ok(Some(t)) = inp.next() {
    let s = *t.span_ref();
    drained.push((s.start, s.end));
  }
  AfterUnwind {
    cursor,
    drained,
    scans: odometer.get(),
    emissions: inp.emitter().emissions(),
    live_rows: inp.emitter().live_rows(),
  }
}

#[test]
fn r9_f2_panicking_pred_does_not_lose_tokens() {
  // "foo bar baz 42": foo (0,3), bar (4,7), baz (8,11), 42 (12,14). The cache is warmed to
  // three tokens, then `sync_to`'s predicate panics on the second one it is handed.
  //
  // BEFORE: the in-flight `bar` is dropped with the unowned local and the already-skipped `foo`
  // is gone behind an uncommitted frontier — two tokens vanish and the cursor jumps 0 → 8.
  // AFTER (commit posture): the skipped `foo` stays committed WITH its report, `bar` goes back
  // to the front of the stream, and nothing re-lexes.
  let got = f2_sync_to_run("foo bar baz 42", true, Some(2), None, usize::MAX);
  assert_eq!(
    got,
    AfterUnwind {
      cursor: 4,
      drained: std::vec![(4, 7), (8, 11), (12, 14)],
      scans: 4,
      emissions: 1,
      live_rows: 0,
    },
    "a panicking predicate must not lose tokens: the committing unwind edge commits the \
     diagnosed prefix and puts the in-flight token back"
  );
}

#[test]
fn r9_f2_panicking_pred_cold_cache_control() {
  // The cold-cache twin: with nothing prefetched the loop lexes both tokens itself. Same law.
  let got = f2_sync_to_run("foo bar baz 42", false, Some(2), None, usize::MAX);
  assert_eq!(
    got,
    AfterUnwind {
      cursor: 4,
      drained: std::vec![(4, 7), (8, 11), (12, 14)],
      scans: 4,
      emissions: 1,
      live_rows: 0,
    },
    "the unwind edge behaves identically whether the tokens were prefetched or freshly lexed"
  );
}

#[test]
fn r9_f2_warm_limit_no_reburn() {
  // The budget payload. "ab cd ef gh" behind a shared limit of 5: the honest run scans four
  // tokens and never trips. A restore posture that CLEARS the store on the unwind edge re-lexes
  // the untouched suffix, re-burns the shared budget, trips the limiter and latches a poison
  // boundary at a position the original lineage never reached — the exact harm §2.1 invokes to
  // justify deleting `Cache::rewind`. This cell is the guard against that posture's return.
  let got = f2_sync_to_run("ab cd ef gh", true, Some(2), None, 5);
  assert_eq!(
    got,
    AfterUnwind {
      cursor: 3,
      drained: std::vec![(3, 5), (6, 8), (9, 11)],
      scans: 4,
      emissions: 1,
      live_rows: 0,
    },
    "the committing unwind edge re-lexes nothing, so the shared budget is not re-burnt and the \
     limiter does not trip"
  );
}

#[test]
fn r9_f2_panicking_exp_closes_the_same_window() {
  // The expected-tokens closure is the SECOND caller-code window inside the skip, and the one
  // that runs with the token already consumed out of the loop's local. It is covered by the
  // same law only if the frontier adopts the token BEFORE the report is built.
  //
  // Cold cache, so the difference is on the stream rather than only on the committed span:
  // BEFORE the whole region re-lexes from zero; AFTER the diagnosed prefix stays committed.
  let got = f2_sync_to_run("ab cd ef gh", false, None, Some(2), usize::MAX);
  assert_eq!(
    got,
    AfterUnwind {
      // Nothing is cached on this path, so the resume cursor is the committed span's end —
      // the second skipped token's end, not the third token's start.
      cursor: 5,
      drained: std::vec![(6, 8), (9, 11)],
      scans: 4,
      emissions: 1,
      live_rows: 0,
    },
    "a panicking `exp` leaves the two skipped tokens committed behind the frontier — no re-lex, \
     no lost token"
  );
}

/// The rewinding twin of [`f2_sync_to_run`]: `sync_through` captures a `ThroughEntry` at the
/// caller — an emitter mark included — so its unwind edge is on the hook for the mark as well as
/// for the stream.
fn f2_sync_through_run(
  src: &'static str,
  warm: bool,
  panic_on: usize,
  limit: usize,
) -> AfterUnwind {
  use generic_arraydeque::typenum::U3;

  let tally = ScanTally::with_limit(limit);
  let odometer = tally.odometer();
  let cache = DefaultCache::<'_, ScanLexer<'_>>::default();
  let mut emitter = ScanLedger::default();
  let mut input =
    Input::<ScanLexer<'_>, ScanLedgerCtx<'_>, ()>::with_state_and_cache(src, tally, cache);
  let mut inp = input.as_ref(&mut emitter);

  if warm {
    let _ = inp.peek::<U3>().unwrap();
  }

  let seen = Cell::new(0usize);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.sync_through(
      |_| {
        let n = seen.get() + 1;
        seen.set(n);
        assert!(n != panic_on, "R9-F2: the predicate panics on token #{n}");
        false
      },
      || None,
    );
  }));
  assert!(caught.is_err(), "the scan must have panicked");

  let cursor = *inp.cursor().as_inner();
  let mut drained = std::vec::Vec::new();
  while let Ok(Some(t)) = inp.next() {
    let s = *t.span_ref();
    drained.push((s.start, s.end));
  }
  AfterUnwind {
    cursor,
    drained,
    scans: odometer.get(),
    emissions: inp.emitter().emissions(),
    live_rows: inp.emitter().live_rows(),
  }
}

#[test]
fn r9_f2_sync_through_unwind_restores_emissions() {
  // A rewinding mode's unwind edge behaves like its own no-match end of input: restore-to-entry.
  // Three things must hold TOGETHER — a `live_rows == 0` alone was a passing gate over the token
  // loss: the stream is back (all four tokens still reachable, from the top), the emissions the
  // aborted scan made are rewound (0), and the entry mark is settled exactly once.
  let got = f2_sync_through_run("ab cd ef gh", false, 2, usize::MAX);
  assert_eq!(
    got,
    AfterUnwind {
      cursor: 0,
      drained: std::vec![(0, 2), (3, 5), (6, 8), (9, 11)],
      scans: 6,
      emissions: 0,
      live_rows: 0,
    },
    "a rewinding scan's unwind edge restores the stream, rewinds its emissions, and settles its \
     entry mark"
  );
}

#[test]
fn r9_f2_sync_through_warm_unwind_prices_its_re_lex() {
  // §4.1's PRICED RESIDUE, pinned with its number rather than left as prose.
  //
  // The restore posture is the ratified one for the rewinding modes — it is what their own
  // `on_eof` already does — but at the panic edge, unlike at true end of input, the cache can
  // still hold an untouched suffix. Restoring therefore re-lexes a region the committing modes
  // would not: with three tokens prefetched, the four-token source is scanned SEVEN times.
  //
  // This cell is the price tag. The proposed ownership extension (unconsume + commit_at +
  // release, i.e. giving the rewinding panic edge the committing arm's keep posture) would read
  // `scans: 4` here and `emissions: 1`; adopting it must therefore rewrite this cell in the same
  // commit, which is exactly the visibility the residue is meant to have.
  let got = f2_sync_through_run("ab cd ef gh", true, 2, usize::MAX);
  assert_eq!(
    got,
    AfterUnwind {
      cursor: 0,
      drained: std::vec![(0, 2), (3, 5), (6, 8), (9, 11)],
      scans: 7,
      emissions: 0,
      live_rows: 0,
    },
    "the rewinding unwind edge re-lexes the warm untouched suffix — the documented, priced \
     residue of restore-to-entry at the panic edge"
  );
}

#[test]
fn r9_f2_panicking_state_clone_settles_the_entry_mark() {
  // The THIRD caller-code window, and the earliest one: the frontier's `L::State: Clone` at the
  // top of `skip_until`. It runs after the caller has already captured the entry mark and moved
  // the snapshot in, so nobody but the scan can settle it — and on the unwind edge nobody did.
  //
  // `sync_through`'s entry evaluates `span.clone()`, then `state.clone()`, then takes the mark;
  // the SECOND state clone from here is therefore the one inside `skip_until`, on the far side
  // of the capture.
  let tally = ScanTally::with_limit(usize::MAX);
  let cache = DefaultCache::<'_, ScanLexer<'_>>::default();
  let mut emitter = ScanLedger::default();
  let mut input =
    Input::<ScanLexer<'_>, ScanLedgerCtx<'_>, ()>::with_state_and_cache("ab cd ef", tally, cache);
  let mut inp = input.as_ref(&mut emitter);

  arm_state_clone(2);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.sync_through(|_| false, || None);
  }));
  disarm_state_clone();
  assert!(caught.is_err(), "the armed state clone must have panicked");

  assert_eq!(
    inp.emitter().live_rows(),
    0,
    "an unwind out of the frontier's state clone must still settle the entry mark the caller \
     captured — nothing else can"
  );
  assert_eq!(
    *inp.cursor().as_inner(),
    0,
    "and it must leave the position where it found it"
  );
}

// ── D28: the cache-geometry parity matrix ────────────────────────────────────────
//
// `Cache::rewind` is gone and its cursor-keyed geometry now runs in the input layer through the
// trait's own queue surface. This pins the four cursor relations × the caches that can express
// them, against the exact behaviour the three deleted bodies had: below the window clears; at
// the front keeps everything; at or past the back clears; mid-window keeps the suffix from an
// exact token start and clears when the cursor falls between two starts (the old bodies'
// binary-search `Err` arm, which the loop reaches by overshooting).

/// Builds a cache holding one token per `(start, end)` pair.
fn geometry_cache(spans: &[(usize, usize)]) -> DefaultCache<'static, ScanLexer<'static>> {
  let mut cache = DefaultCache::<'_, ScanLexer<'_>>::default();
  for (start, end) in spans {
    let tok = crate::span::Spanned::new(crate::span::SimpleSpan::new(*start, *end), ScanTok::Word);
    let _ = cache.push_back(crate::cache::CachedToken::new(tok, ScanTally::default()));
  }
  cache
}

/// The resident spans, read back without disturbing the cache.
fn resident(cache: &DefaultCache<'static, ScanLexer<'static>>) -> std::vec::Vec<(usize, usize)> {
  let mut out = std::vec::Vec::new();
  let mut probe = cache.clone();
  while let Some(tok) = probe.pop_front() {
    let spanned = tok.token();
    out.push((spanned.span_ref().start, spanned.span_ref().end));
  }
  out
}

#[test]
fn cache_geometry_parity_matrix() {
  // Resident window: [4,6) [8,10) [12,14).
  let window: &[(usize, usize)] = &[(4, 6), (8, 10), (12, 14)];
  let cases: &[(usize, &[(usize, usize)], &str)] = &[
    (2, &[], "below the window: cleared"),
    (4, window, "exactly at the front: the whole window is kept"),
    (
      8,
      &[(8, 10), (12, 14)],
      "mid-window at a token start: the suffix is kept",
    ),
    (
      12,
      &[(12, 14)],
      "at the last token's start: that token alone",
    ),
    // Between two starts the old `binary_search_by_key` took its `Err` arm and cleared; the
    // loop reaches the same fixpoint by popping the prefix and then overshooting into the
    // "before the front" case. Clearing is the safe direction: the region re-lexes.
    (
      9,
      &[],
      "between two token starts: cleared, exactly as the Err arm did",
    ),
    (14, &[], "at the back's end: cleared"),
    (20, &[], "past the back: cleared"),
  ];
  for (cursor, want, why) in cases {
    let mut cache = geometry_cache(window);
    super::reconcile_cache_geometry::<ScanLexer<'_>, _, ()>(&mut cache, cursor);
    assert_eq!(
      resident(&cache),
      want.to_vec(),
      "geometry drift at cursor {cursor} — {why}"
    );
  }

  // The empty cache is a no-op at every cursor, and the capacity-1 cache is the degenerate case
  // of the same loop: keep on an exact front match, clear otherwise.
  let mut empty = geometry_cache(&[]);
  super::reconcile_cache_geometry::<ScanLexer<'_>, _, ()>(&mut empty, &7);
  assert!(resident(&empty).is_empty(), "an empty cache stays empty");

  for (cursor, want) in [(2usize, false), (4, true), (5, false), (6, false)] {
    let mut one = geometry_cache(&[(4, 6)]);
    super::reconcile_cache_geometry::<ScanLexer<'_>, _, ()>(&mut one, &cursor);
    assert_eq!(
      !resident(&one).is_empty(),
      want,
      "capacity-1 geometry drift at cursor {cursor}"
    );
  }
}

// ═══════════════════════════════════════════════════════════════════════════════════
// The settle path's own fallible steps (Codex R9 F1/F2/F3)
//
// The unwind edge closed the six windows the loop runs caller code in. The SETTLE code is
// itself caller code: `L::Span::clone` runs inside `skip_and_report`'s report build and inside
// `set_span` on every restore, and the emitter's diagnostic path runs while a limit trip has
// already latched a poison boundary. This fixture makes each of those steps armable.
//
// A hand-rolled lexer, because the bomb has to be in `L::Span::clone` and the Logos backend's
// span is `SimpleSpan`.

thread_local! {
  /// `BombSpan::clone` calls since the last arm, and the call index that panics (0 = disarmed).
  static SPAN_CLONES: Cell<usize> = const { Cell::new(0) };
  static SPAN_BOMB: Cell<usize> = const { Cell::new(0) };
  /// Every span cloned since the last arm, as `(index, start, end)` — the probe that says which
  /// index to arm instead of guessing one.
  static SPAN_LOG: std::cell::RefCell<std::vec::Vec<(usize, usize, usize)>> =
    const { std::cell::RefCell::new(std::vec::Vec::new()) };
}

fn arm_span_clone(at: usize) {
  SPAN_CLONES.with(|c| c.set(0));
  SPAN_BOMB.with(|c| c.set(at));
  SPAN_LOG.with(|l| l.borrow_mut().clear());
}

fn disarm_span_clone() {
  SPAN_BOMB.with(|c| c.set(0));
}

thread_local! {
  /// `BombTally::drop` calls since the last arm, and the index that panics (0 = disarmed).
  ///
  /// `commit_at` writes `*ir.state = frontier.state`, and that assignment **drops the state it
  /// replaces** — caller code, inside the frontier commit, and the only step there that can be
  /// aimed at. `L::Span::end_ref` is the other caller-code step on that path, but it returns a
  /// reference and is called ubiquitously, so arming it by count cannot be pointed at one call:
  /// that instrument was built, measured to pass identically at every index it could reach, and
  /// retired rather than shipped as a cell that could not fail.
  static STATE_DROPS: Cell<usize> = const { Cell::new(0) };
  static STATE_DROP_BOMB: Cell<usize> = const { Cell::new(0) };
}

thread_local! {
  /// `BombSpan::drop` calls since the last arm, and the index that panics (0 = disarmed).
  ///
  /// The span is the FIRST half written at every paired site, and an assignment installs its new
  /// value and then drops the replaced one — so the replaced SPAN's drop is an unwind site that
  /// sits *between* the two writes. Arming the state drop instead reaches only the second
  /// assignment, which is past both writes and cannot tear. Every earlier cell in this family
  /// armed the state; this is the half that discriminates.
  static SPAN_DROPS: Cell<usize> = const { Cell::new(0) };
  static SPAN_DROP_BOMB: Cell<usize> = const { Cell::new(0) };
}

fn arm_span_drop(at: usize) {
  SPAN_DROPS.with(|c| c.set(0));
  SPAN_DROP_BOMB.with(|c| c.set(at));
}

fn disarm_span_drop() {
  SPAN_DROP_BOMB.with(|c| c.set(0));
}

fn arm_state_drop(at: usize) {
  STATE_DROPS.with(|c| c.set(0));
  STATE_DROP_BOMB.with(|c| c.set(at));
}

fn disarm_state_drop() {
  STATE_DROP_BOMB.with(|c| c.set(0));
}

fn span_clone_log() -> std::vec::Vec<(usize, usize, usize)> {
  SPAN_LOG.with(|l| l.borrow().clone())
}

thread_local! {
  /// Arms [`BombLexer::into_state`], the one caller-code step inside the end-of-input settle.
  static INTO_STATE_BOMB: Cell<bool> = const { Cell::new(false) };
}

fn arm_into_state(on: bool) {
  INTO_STATE_BOMB.with(|c| c.set(on));
}

thread_local! {
  /// Every `BombLexer::lex` call. A by-value tally is restored along with the state, so it
  /// cannot see work that a rewind un-did and then re-did; this can. It is the meter for
  /// "was the prefix re-scanned?".
  static LEX_CALLS: Cell<usize> = const { Cell::new(0) };
}

fn reset_lex_calls() {
  LEX_CALLS.with(|c| c.set(0));
}

fn lex_calls() -> usize {
  LEX_CALLS.with(Cell::get)
}

/// A span whose `Clone` is armable. Everything else is `SimpleSpan`'s behaviour.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct BombSpan {
  start: usize,
  end: usize,
}

impl Drop for BombSpan {
  fn drop(&mut self) {
    let n = SPAN_DROPS.with(|c| {
      let v = c.get() + 1;
      c.set(v);
      v
    });
    assert!(
      n != SPAN_DROP_BOMB.with(Cell::get),
      "R9 settle-path: the armed `L::Span` drop (#{n}, {}..{}) panics",
      self.start,
      self.end
    );
  }
}

impl Clone for BombSpan {
  fn clone(&self) -> Self {
    let n = SPAN_CLONES.with(|c| {
      let v = c.get() + 1;
      c.set(v);
      v
    });
    SPAN_LOG.with(|l| l.borrow_mut().push((n, self.start, self.end)));
    assert!(
      n != SPAN_BOMB.with(Cell::get),
      "R9 settle-path: the armed `L::Span` clone (#{n}, {}..{}) panics",
      self.start,
      self.end
    );
    Self {
      start: self.start,
      end: self.end,
    }
  }
}

impl crate::Span for BombSpan {
  type Offset = usize;

  fn new(start: usize, end: usize) -> Self {
    Self { start, end }
  }

  fn into_range(self) -> core::ops::Range<usize> {
    self.start..self.end
  }

  fn start_ref(&self) -> &usize {
    &self.start
  }

  fn start_mut(&mut self) -> &mut usize {
    &mut self.start
  }

  fn into_start(self) -> usize {
    self.start
  }

  fn end_ref(&self) -> &usize {
    &self.end
  }

  fn end_mut(&mut self) -> &mut usize {
    &mut self.end
  }

  fn into_end(self) -> usize {
    self.end
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

/// A by-value scan tally that also trips: the limit is on the tally the committed lineage
/// carries, so a restore restores the budget with it — which is what makes the poison boundary
/// the only thing left over on the unwind edge.
#[derive(Debug, Clone, PartialEq)]
struct BombTally {
  scanned: usize,
  limit: usize,
}

impl Default for BombTally {
  fn default() -> Self {
    Self {
      scanned: 0,
      limit: usize::MAX,
    }
  }
}

impl Drop for BombTally {
  fn drop(&mut self) {
    let n = STATE_DROPS.with(|c| {
      let v = c.get() + 1;
      c.set(v);
      v
    });
    assert!(
      n != STATE_DROP_BOMB.with(Cell::get),
      "R9 settle-path: the armed `L::State` drop (#{n}) panics"
    );
  }
}

#[derive(Debug, Clone, PartialEq)]
struct BombTripped;

impl State for BombTally {
  type Error = BombTripped;

  fn check(&self) -> Result<(), BombTripped> {
    if self.scanned > self.limit {
      Err(BombTripped)
    } else {
      Ok(())
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
enum BombErr {
  Lex,
  Limit,
  Incomplete,
}

impl From<()> for BombErr {
  fn from(_: ()) -> Self {
    BombErr::Lex
  }
}

impl From<BombTripped> for BombErr {
  fn from(_: BombTripped) -> Self {
    BombErr::Limit
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for BombErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    BombErr::Lex
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for BombErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    BombErr::Lex
  }
}

impl From<crate::error::Incomplete<usize>> for BombErr {
  fn from(_: crate::error::Incomplete<usize>) -> Self {
    BombErr::Incomplete
  }
}

impl crate::error::MaybeIncomplete for BombErr {
  fn is_incomplete(&self) -> bool {
    matches!(self, BombErr::Incomplete)
  }
}

#[derive(Debug, Clone, PartialEq)]
struct BombTok;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BombKind;

impl core::fmt::Display for BombKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

impl Token<'_> for BombTok {
  type Kind = BombKind;
  type Error = BombErr;

  fn kind(&self) -> BombKind {
    BombKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// A space-separated word lexer over the armable span. A tripped tally surfaces exactly as the
/// Logos backend surfaces one: the tripping token is replaced by a `Lexed::Error`.
struct BombLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: BombTally,
}

impl<'a> crate::Lexer<'a> for BombLexer<'a> {
  type State = BombTally;
  type Source = str;
  type Token = BombTok;
  type Span = BombSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self::with_state(src, BombTally::default())
  }

  fn with_state(src: &'a str, state: BombTally) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), BombErr> {
    State::check(&self.state).map_err(BombErr::from)
  }

  fn state(&self) -> &BombTally {
    &self.state
  }

  fn state_mut(&mut self) -> &mut BombTally {
    &mut self.state
  }

  fn into_state(self) -> BombTally {
    // Armable, and it needs no calibration: the input layer calls `into_state` exactly once per
    // scan, inside `on_eof`. It is caller code and nothing in the contract forbids it panicking,
    // which makes it the honest witness for a panic *inside* an exit settle.
    assert!(
      !INTO_STATE_BOMB.with(Cell::get),
      "R9 settle-path: the armed `Lexer::into_state` inside the end-of-input settle panics"
    );
    self.state
  }

  fn source(&self) -> &'a str {
    self.src
  }

  fn span(&self) -> BombSpan {
    BombSpan {
      start: self.start,
      end: self.end,
    }
  }

  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }

  fn lex(&mut self) -> Option<Result<BombTok, BombErr>> {
    LEX_CALLS.with(|c| c.set(c.get() + 1));
    let bytes = self.src.as_bytes();
    let mut i = self.end;
    while i < bytes.len() && bytes[i] == b' ' {
      i += 1;
    }
    if i >= bytes.len() {
      self.start = i;
      self.end = i;
      return None;
    }
    self.start = i;
    while i < bytes.len() && bytes[i] != b' ' {
      i += 1;
    }
    self.end = i;
    self.state.scanned += 1;
    // A trip replaces the tripping token, exactly as the bundled backend does.
    if State::check(&self.state).is_err() {
      return Some(Err(BombErr::Limit));
    }
    Some(Ok(BombTok))
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

/// A mark-keyed emitter for the settle-path cells: it counts live rows, counts emissions, and can
/// be told to panic inside its diagnostic path — the window a latched limit trip opens.
#[derive(Debug, Default)]
struct BombEmitter {
  emissions: usize,
  panic_on_emit: bool,
  /// Arms the committed-token OBSERVER. It is foreign code that `commit_token` invokes, and on a
  /// cache-hit consume the token has already left the front stream by the time it runs.
  panic_on_commit_token: bool,
  next: Cell<u64>,
  live: core::cell::RefCell<std::vec::Vec<u64>>,
  /// Settle CALLS, not surviving rows. A double release removes nothing the second time, so a
  /// row count cannot see it; these can.
  releases: usize,
  rewinds: usize,
  /// Every token the input layer settled, and — recorded against each mark — how many had been
  /// settled when that mark was taken, so a rewind can truncate them with the emissions. This is
  /// the observable for a mode that reports nothing per token: `SyncBalanced` emits one hole
  /// diagnostic, so `commit_token` is where its per-token settles show.
  commits: std::vec::Vec<(usize, usize)>,
  marks: core::cell::RefCell<std::vec::Vec<(u64, usize, usize)>>,
}

#[allow(dead_code)]
impl BombEmitter {
  fn panicking() -> Self {
    Self {
      panic_on_emit: true,
      ..Self::default()
    }
  }

  fn live_rows(&self) -> usize {
    self.live.borrow().len()
  }
}

impl<'inp, L, Lang: ?Sized> crate::Emitter<'inp, L, Lang> for BombEmitter
where
  L: crate::Lexer<'inp>,
  <L::Token as Token<'inp>>::Error: Into<BombErr>,
{
  type Error = BombErr;

  fn emit_lexer_error(
    &mut self,
    err: crate::span::Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), BombErr> {
    let _ = err;
    assert!(
      !self.panic_on_emit,
      "R9 settle-path: the emitter's diagnostic path panics"
    );
    self.emissions += 1;
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), BombErr> {
    let _ = err;
    self.emissions += 1;
    Ok(())
  }

  fn emit_error(&mut self, err: crate::span::Spanned<BombErr, L::Span>) -> Result<(), BombErr> {
    let _ = err;
    self.emissions += 1;
    Ok(())
  }

  fn commit_token(&mut self, _tok: &L::Token, span: &L::Span) {
    let _ = span;
    assert!(
      !self.panic_on_commit_token,
      "R9 settle-path: the committed-token observer panics"
    );
    let n = self.commits.len();
    self.commits.push((n, n));
  }

  fn checkpoint(&self) -> u64 {
    let id = self.next.get() + 1;
    self.next.set(id);
    self.live.borrow_mut().push(id);
    self
      .marks
      .borrow_mut()
      .push((id, self.emissions, self.commits.len()));
    id
  }

  fn rewind(&mut self, _cursor: &crate::input::Cursor<'inp, '_, L>, checkpoint: u64) {
    self.rewinds += 1;
    let at = {
      let mut marks = self.marks.borrow_mut();
      let at = marks
        .iter()
        .find(|(id, _, _)| *id == checkpoint)
        .map(|(_, e, c)| (*e, *c));
      marks.retain(|(id, _, _)| *id < checkpoint);
      at
    };
    if let Some((e, c)) = at {
      self.emissions = e;
      self.commits.truncate(c);
    }
    self.live.borrow_mut().retain(|m| *m < checkpoint);
  }

  fn release(&mut self, checkpoint: u64) {
    self.releases += 1;
    let mut live = self.live.borrow_mut();
    if let Some(pos) = live.iter().rposition(|m| *m == checkpoint) {
      live.remove(pos);
    }
  }
}

type BombCtx<'a> = (BombEmitter, DefaultCache<'a, BombLexer<'a>>);

#[test]
fn r9_settle_path_span_clone_inventory() {
  // The inventory of every `L::Span::clone` the scan path performs, pinned rather than printed.
  //
  // Two jobs. It is the calibration the cells below arm against, so they name an index that was
  // measured. And it is a rail with teeth on the discipline itself: a clone that appears in the
  // settle path — or one that moves — changes this list, so the diff that introduces it has to
  // look at the windows the cells cover instead of discovering them later.
  //
  // READ THIS BEFORE ASSUMING IT ONLY GUARDS ADDITION. The rail is **two-directional**: the
  // second assertion below pins an ABSENCE. There is no seventh clone on the
  // `sync_through`-to-end-of-input path, and there must not be one, because the restore MOVES the
  // snapshot's span instead of borrowing it — that is what deleted a caller-code step from inside
  // an exit settle rather than defending it. A count rail that only fired on growth would let
  // that regress silently the day someone reintroduces `(&snapshot.span).into()`.
  use generic_arraydeque::typenum::U3;

  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  let _ = inp.peek::<U3>().unwrap();

  arm_span_clone(0);
  let _ = inp.sync_to(|_| false, || None);
  disarm_span_clone();
  assert_eq!(
    span_clone_log(),
    std::vec![(1, 0, 0), (2, 0, 2), (3, 3, 5), (4, 6, 8), (5, 9, 11)],
    "warm `sync_to` span-clone inventory moved: #1 is `skip_until`'s entry frontier and each of \
     #2..#5 is one skipped token's report clone, taken AFTER the adopt. If a clone appears \
     before an adopt again, the token it belongs to is in neither the scope nor the frontier for \
     the length of it — re-read `skip_and_report` and re-arm \
     `r9_f3_panicking_report_span_clone_does_not_lose_a_token`."
  );

  let mut emitter2 = BombEmitter::default();
  let mut input2 = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    DefaultCache::<'_, BombLexer<'_>>::default(),
  );
  let mut inp2 = input2.as_ref(&mut emitter2);
  arm_span_clone(0);
  let _ = inp2.sync_through(|_| false, || None);
  disarm_span_clone();
  assert_eq!(
    span_clone_log(),
    std::vec![
      (1, 0, 0),
      (2, 0, 0),
      (3, 0, 2),
      (4, 3, 5),
      (5, 6, 8),
      (6, 9, 11)
    ],
    "cold `sync_through`-to-end-of-input span-clone inventory moved: #1 is the entry \
     `ThroughEntry`, #2 `skip_until`'s frontier, #3..#6 the four report clones. This assertion \
     PINS AN ABSENCE as much as a count: there is deliberately no seventh clone, because the \
     end-of-input restore MOVES the snapshot's span rather than borrowing it and so clones no \
     span at all. A seventh entry means the restore is cloning again — a caller-code step back \
     inside an exit settle, which is the window this round deleted rather than defended."
  );
}

/// What the settle-path cells read back: the committed span, the tokens still reachable, the
/// total lexing work, the surviving emissions, the outstanding marks, and the poison latch.
#[derive(Debug, PartialEq, Eq)]
struct AfterSettle {
  committed: (usize, usize),
  drained: std::vec::Vec<(usize, usize)>,
  scans: usize,
  emissions: usize,
  live_rows: usize,
  poisoned: bool,
}

#[test]
fn r9_f3_panicking_report_span_clone_does_not_lose_a_token() {
  // F3 — the round's own defect one window deeper. `skip_and_report` clones the span for the
  // report BEFORE adopting the token into the frontier, and `L::Span::clone` is caller code. In
  // a warm-cache `sync_to` the token has already been popped out of the cache and out of the
  // scope's `in_flight`, so an unwind there leaves it in neither: the committing drop can only
  // commit the PREVIOUS frontier, and the token is gone from the stream with the committed
  // position standing behind it — unaccounted for.
  //
  // The armed clone is #3, which the probe above identifies as the report clone of the SECOND
  // skipped token: #1 is `skip_until`'s entry frontier and #2 the first token's report.
  use generic_arraydeque::typenum::U3;

  let tally = BombTally::default();
  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input =
    Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache("ab cd ef gh", tally, cache);
  let mut inp = input.as_ref(&mut emitter);
  let _ = inp.peek::<U3>().unwrap();

  arm_span_clone(3);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.sync_to(|_| false, || None);
  }));
  disarm_span_clone();
  assert!(caught.is_err(), "the armed span clone must have panicked");

  let committed = {
    let s = inp.span();
    (s.start, s.end)
  };
  let mut drained = std::vec::Vec::new();
  while let Ok(Some(t)) = inp.next() {
    let s = t.span_ref();
    drained.push((s.start, s.end));
  }

  assert_eq!(
    AfterSettle {
      committed,
      drained,
      scans: inp.state().scanned,
      emissions: inp.emitter().emissions,
      live_rows: inp.emitter().live_rows(),
      poisoned: false,
    },
    AfterSettle {
      // The token whose report clone panicked is behind the committed position: it was skipped,
      // not lost. Before the adopt was hoisted this read (0, 2) — the frontier one token back,
      // with `cd` gone from the stream and accounted for nowhere.
      committed: (3, 5),
      drained: std::vec![(6, 8), (9, 11)],
      scans: 4,
      emissions: 1,
      live_rows: 0,
      poisoned: false,
    },
    "a panicking report-span clone must leave the token behind the frontier the committing \
     unwind edge commits at — no fallible caller code between taking the token and recording it"
  );
}

#[test]
fn r9_f1_rewinding_unwind_restores_the_poison_boundary() {
  // F1 — a limit trip latches the poison boundary inside `classify`, BEFORE its diagnostic is
  // emitted. If the diagnostic path then panics, the rewinding scan restores span, state, the
  // dedup watermark and the emitter mark — and leaves the freshly latched boundary standing, so
  // the input is poisoned at a position the rewound lineage never reached, with no diagnostic to
  // show for it. `ThroughEntry` snapshots four facts; the boundary was the fifth.
  let tally = BombTally {
    scanned: 0,
    limit: 1,
  };
  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::panicking();
  let mut input =
    Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache("ab cd ef gh", tally, cache);
  let mut inp = input.as_ref(&mut emitter);

  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.sync_through(|_| false, || None);
  }));
  assert!(
    caught.is_err(),
    "the emitter's diagnostic path must have panicked"
  );

  assert!(
    !inp.is_poisoned(),
    "the rewinding unwind edge left the poison boundary latched: the scan was rewound to entry, \
     so the trip that latched it is un-made and the boundary describes a position no committed \
     lineage ever reached"
  );
  assert_eq!(
    inp.emitter().live_rows(),
    0,
    "and the entry mark was still settled exactly once"
  );
}

#[test]
fn r9_f2_panicking_eof_settle_still_settles_the_mark() {
  // F2 — the end-of-input arm takes the snapshot out of the scope BEFORE `M::on_eof`, and
  // `on_eof` runs caller code. A panic there found the scope already disarmed, so `Drop` had
  // nothing left to settle with and the mark was stranded.
  //
  // The armed step is `Lexer::into_state`, which `on_eof` calls exactly once per scan — no
  // calibration, and nothing in the contract forbids it panicking. The mark at risk is the
  // Partial entry a COMMITTING mode captures: `skip_while` over a sealed partial input reaches
  // end of input holding one.
  //
  // (The rewinding modes' own end-of-input settle no longer clones anything at all: its last
  // caller-code step, the emitter cursor's `L::Offset::clone`, moved out to the entry capture.
  // This note used to say that step could not be armed because every in-tree `Offset` is
  // `usize` — `BombOffset` is that missing witness, and
  // `r9_restore_entry_is_atomic_at_every_offset_clone` is the measurement it made possible.)
  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input =
    Input::<BombLexer<'_>, BombCtx<'_>, (), crate::input::Partial>::with_state_and_cache(
      "ab cd ef gh",
      BombTally::default(),
      cache,
    );
  input.seal();
  let mut inp = input.as_ref(&mut emitter);

  arm_into_state(true);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.skip_while(|_| true);
  }));
  arm_into_state(false);
  assert!(
    caught.is_err(),
    "the armed step inside the end-of-input settle must have panicked"
  );

  assert_eq!(
    inp.emitter().live_rows(),
    0,
    "a panic inside the end-of-input settle must still settle the mark the scan captured — the \
     scope keeps a copy of it beside the snapshot precisely so a settle that unwinds part-way \
     leaves nothing outstanding"
  );
}

// ── The stop exit's own handover window (Codex, second pass) ────────────────────
//
// `ScanMode::on_stop` fuses TWO handovers: it settles the stopping token (for a `to`-shaped mode,
// back into the stream through the public `Cache::push_front`) and then commits the frontier. Both
// were handed to it before the scope was disarmed, so a panic in the first one took the second
// down with it: the diagnosed prefix was never committed, the position stayed at entry, and a
// catching host that retried re-lexed and re-diagnosed the whole prefix.

thread_local! {
  /// Arms the cache's `push_front`, which is what `unconsume` — and therefore a `to`-shaped
  /// stop — reaches. It is public trait code and nothing makes it infallible.
  static PUSH_FRONT_BOMB: Cell<bool> = const { Cell::new(false) };
}

fn arm_push_front(on: bool) {
  PUSH_FRONT_BOMB.with(|c| c.set(on));
}

/// A capacity-3 ring with an armable `push_front`, standing in for `DefaultCache`.
struct BombCache<'a, L>
where
  L: crate::Lexer<'a>,
{
  items: std::collections::VecDeque<crate::cache::CachedTokenOf<'a, L>>,
}

impl<'a, L, Lang: ?Sized> crate::cache::Cache<'a, L, Lang> for BombCache<'a, L>
where
  L: crate::Lexer<'a>,
{
  type Options = ();

  const RETAINS_FRONT: bool = true;

  fn new() -> Self {
    Self {
      items: std::collections::VecDeque::with_capacity(3),
    }
  }

  fn with_options((): ()) -> Self {
    <Self as crate::cache::Cache<'a, L, Lang>>::new()
  }

  fn len(&self) -> usize {
    self.items.len()
  }

  fn remaining(&self) -> usize {
    3 - self.items.len()
  }

  fn push_front(
    &mut self,
    tok: crate::cache::CachedTokenOf<'a, L>,
  ) -> Result<crate::cache::CachedTokenRefOf<'_, 'a, L>, crate::cache::CachedTokenOf<'a, L>> {
    assert!(
      !PUSH_FRONT_BOMB.with(Cell::get),
      "R9 stop-exit: the armed `Cache::push_front` panics"
    );
    if self.items.len() == 3 {
      return Err(tok);
    }
    self.items.push_front(tok);
    Ok(self.items.front().expect("just pushed").as_ref())
  }

  fn push_back(
    &mut self,
    tok: crate::cache::CachedTokenOf<'a, L>,
  ) -> Result<crate::cache::CachedTokenRefOf<'_, 'a, L>, crate::cache::CachedTokenOf<'a, L>> {
    if self.items.len() == 3 {
      return Err(tok);
    }
    self.items.push_back(tok);
    Ok(self.items.back().expect("just pushed").as_ref())
  }

  fn pop_front(&mut self) -> Option<crate::cache::CachedTokenOf<'a, L>> {
    self.items.pop_front()
  }

  fn pop_back(&mut self) -> Option<crate::cache::CachedTokenOf<'a, L>> {
    self.items.pop_back()
  }

  fn clear(&mut self) {
    self.items.clear();
  }

  fn peek<'p, W>(
    &'p self,
    buf: &mut generic_arraydeque::GenericArrayDeque<
      crate::cache::MaybeRefCachedTokenOf<'p, 'a, L>,
      W::CAPACITY,
    >,
  ) where
    W: crate::Window,
  {
    let fill = buf.remaining_capacity().min(self.items.len());
    for tok in self.items.iter().take(fill) {
      buf.push_back(mayber::Maybe::Ref(tok.as_ref()));
    }
  }

  fn front(&self) -> Option<crate::cache::CachedTokenRefOf<'_, 'a, L>> {
    self.items.front().map(crate::cache::CachedToken::as_ref)
  }

  fn back(&self) -> Option<crate::cache::CachedTokenRefOf<'_, 'a, L>> {
    self.items.back().map(crate::cache::CachedToken::as_ref)
  }
}

type BombCacheCtx<'a> = (BombEmitter, BombCache<'a, BombLexer<'a>>);

#[test]
fn r9_stop_exit_panic_still_commits_the_diagnosed_prefix() {
  // A warm `sync_to` that skips two tokens (diagnosing each) and then STOPS on the third, with
  // the `push_front` its stop settle reaches armed to panic. The host catches and retries — which
  // is the whole point: what a retry sees is what this defect costs.
  //
  // The stopping token is swallowed by the panicking `push_front` and nothing can un-swallow it.
  // What must survive is everything else: the two diagnosed skips stay committed, so the retry
  // resumes AFTER them, re-lexes the stopper, and adds no second copy of their diagnostics.
  use generic_arraydeque::typenum::U3;

  let cache = <BombCache<'_, BombLexer<'_>> as crate::cache::Cache<'_, BombLexer<'_>, ()>>::new();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCacheCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  let _ = inp.peek::<U3>().unwrap();

  arm_push_front(true);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.sync_to(|t| t.span_ref().start >= 6, || None);
  }));
  arm_push_front(false);
  assert!(
    caught.is_err(),
    "the armed `push_front` inside the stop settle must have panicked"
  );

  let committed_after_panic = {
    let s = inp.span();
    (s.start, s.end)
  };
  let emissions_after_panic = inp.emitter().emissions;

  // The retry a catching host performs.
  let retry = inp.sync_to(|t| t.span_ref().start >= 6, || None);
  assert!(retry.is_ok(), "the retry itself must not error");

  assert_eq!(
    (
      committed_after_panic,
      emissions_after_panic,
      inp.emitter().emissions,
      inp.emitter().live_rows(),
    ),
    ((3, 5), 2, 2, 0),
    "a panic inside the stop settle must leave the diagnosed prefix COMMITTED: the position \
     stands at the last skipped token, the two skip diagnostics stay at two, and the retry \
     resumes past them instead of re-lexing and re-diagnosing the whole prefix"
  );
}

/// Drives one rewinding mode to a stop and returns `(releases, rewinds, live_rows)`.
///
/// Both members, not one: naming a cell for the CLASS and instantiating a single member is how
/// the mode-versus-exit defect survived a round. `SyncThrough` and `SyncBalanced` are the class,
/// and they are exactly the two whose stop dispositions differ from their end-of-input ones.
fn rewinding_stop_settles(balanced: bool) -> (usize, usize, usize) {
  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  if balanced {
    use crate::input::Balance;
    let hole = inp
      .sync_balanced(
        |_k: &BombKind| Balance::<char>::Neutral,
        |t| t.span_ref().start >= 6,
      )
      .expect("the sync itself must not error");
    assert!(hole.is_some(), "the balanced scan found its target");
  } else {
    let found = inp
      .sync_through(|t| t.span_ref().start >= 6, || None)
      .expect("the sync itself must not error");
    assert!(found.is_some(), "the scan found its target");
  }
  (
    inp.emitter().releases,
    inp.emitter().rewinds,
    inp.emitter().live_rows(),
  )
}

/// The same over a scan that matches nothing and reaches end of input.
fn rewinding_no_match_settles(balanced: bool) -> (usize, usize, usize) {
  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  if balanced {
    use crate::input::Balance;
    let hole = inp
      .sync_balanced(|_k: &BombKind| Balance::<char>::Neutral, |_| false)
      .expect("the sync itself must not error");
    assert!(hole.is_none(), "nothing matched");
  } else {
    let found = inp
      .sync_through(|_| false, || None)
      .expect("the sync itself must not error");
    assert!(found.is_none(), "nothing matched");
  }
  (
    inp.emitter().releases,
    inp.emitter().rewinds,
    inp.emitter().live_rows(),
  )
}

#[test]
fn r9_rewinding_stop_settles_its_entry_mark_exactly_once() {
  // On the stop path, is a rewinding mode's entry mark settled EXACTLY once? A live-row count
  // cannot answer it — a second release removes nothing and reads clean — so the emitter counts
  // settle CALLS instead. Run over BOTH rewinding modes.
  //
  // One capture, one settle, and the settle is a release (the stop kept its progress), with the
  // scope's stranded-mark fallback contributing nothing because the exit finished.
  for balanced in [false, true] {
    assert_eq!(
      rewinding_stop_settles(balanced),
      (1, 0, 0),
      "a rewinding stop settles its entry mark exactly once, by release — not twice, and not by \
       rewind (the stop kept its progress). balanced = {balanced}"
    );
  }
}

#[test]
fn r9_rewinding_no_match_settles_its_entry_mark_exactly_once() {
  // The dual: the no-match end-of-input exit spends the same mark by rewinding to it, once —
  // and again over both members, since this is the exit where their dispositions AGREE and the
  // pair is what shows that the stop exit's disagreement is real rather than an artefact.
  for balanced in [false, true] {
    assert_eq!(
      rewinding_no_match_settles(balanced),
      (0, 1, 0),
      "a rewinding no-match exit settles its entry mark exactly once, by rewind. \
       balanced = {balanced}"
    );
  }
}

#[test]
fn r9_balanced_stop_exit_panic_keeps_the_prefix_like_its_own_stop_does() {
  // The two axes cross at exactly one mode. `SyncBalanced` REWINDS at end of input
  // (`HOLDS_ENTRY`) and COMMITS the frontier on a stop (`COMMITS_FRONTIER_ON_STOP`) — so a guard
  // that branches on the mode alone gives its stop exit the wrong disposition: the normal stop
  // keeps the diagnosed prefix and the interrupted stop throws it away, and a catching host
  // re-scans work the stop decision had already classified as kept.
  //
  // Disposition belongs to the EXIT, not the mode, and this is the mode that proves it.
  use crate::input::Balance;
  use generic_arraydeque::typenum::U3;

  let cache = <BombCache<'_, BombLexer<'_>> as crate::cache::Cache<'_, BombLexer<'_>, ()>>::new();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCacheCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  reset_lex_calls();
  let _ = inp.peek::<U3>().unwrap();

  arm_push_front(true);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.sync_balanced(
      |_k: &BombKind| Balance::<char>::Neutral,
      |t| t.span_ref().start >= 6,
    );
  }));
  arm_push_front(false);
  assert!(
    caught.is_err(),
    "the armed `push_front` inside the balanced stop settle must have panicked"
  );

  let committed_after_panic = {
    let s = inp.span();
    (s.start, s.end)
  };

  // The retry a catching host performs.
  let retry = inp.sync_balanced(
    |_k: &BombKind| Balance::<char>::Neutral,
    |t| t.span_ref().start >= 6,
  );
  assert!(retry.is_ok(), "the retry itself must not error");

  assert_eq!(
    (
      committed_after_panic,
      lex_calls(),
      inp.emitter().live_rows()
    ),
    ((3, 5), 4, 0),
    "a balanced stop interrupted mid-settle must keep the prefix its own stop exit keeps: the \
     position stands at the last skipped token and the retry re-lexes only the swallowed \
     stopper. Rewinding to entry instead throws away work the stop had already classified as \
     kept, and the retry re-scans the whole prefix."
  );
}

/// Drives a stop on the **second** of three prefilled cache entries with `push_front` armed, then
/// drains. Returns `(committed span after the catch, the tokens the retry can still reach)`.
///
/// The cache position is the whole point. When the stopper is the LAST resident entry, losing it
/// self-heals: nothing younger is retained, so `cursor()` falls back to the committed position and
/// re-lexing meets the token again. When something younger IS retained, the committed position and
/// the cache front are no longer adjacent — the retry reads the younger entry and the swallowed
/// token is skipped with no signal at all. The earlier cells stopped on the last entry, which is
/// exactly the position where the hole cannot manifest.
fn stop_mid_cache_then_drain(balanced: bool) -> ((usize, usize), std::vec::Vec<(usize, usize)>) {
  use generic_arraydeque::typenum::U3;

  let cache = <BombCache<'_, BombLexer<'_>> as crate::cache::Cache<'_, BombLexer<'_>, ()>>::new();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCacheCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  // Three resident: (0,2) (3,5) (6,8). The scan skips the first and stops on the second, so (6,8)
  // is still retained when the stop settle panics.
  let _ = inp.peek::<U3>().unwrap();

  arm_push_front(true);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    if balanced {
      use crate::input::Balance;
      let _ = inp.sync_balanced(
        |_k: &BombKind| Balance::<char>::Neutral,
        |t| t.span_ref().start >= 3,
      );
    } else {
      let _ = inp.sync_to(|t| t.span_ref().start >= 3, || None);
    }
  }));
  arm_push_front(false);
  assert!(
    caught.is_err(),
    "the armed `push_front` inside the stop settle must have panicked"
  );

  let committed = {
    let s = inp.span();
    (s.start, s.end)
  };
  let mut drained = std::vec::Vec::new();
  while let Ok(Some(t)) = inp.next() {
    let s = t.span_ref();
    drained.push((s.start, s.end));
  }
  (committed, drained)
}

#[test]
fn r9_stop_mid_cache_panic_does_not_skip_the_swallowed_token() {
  // The reachable half of the same class: a stop settle that panics with a younger cache suffix
  // still resident. The token the settle swallowed is not lost from the STREAM — the committed
  // position is still behind it, so re-lexing reproduces it — but only if nothing stale sits in
  // front of that position. The retained suffix is exactly that stale thing.
  for balanced in [false, true] {
    assert_eq!(
      stop_mid_cache_then_drain(balanced),
      ((0, 2), std::vec![(3, 5), (6, 8), (9, 11)]),
      "a stop settle interrupted mid-handover must leave the stream contiguous with the \
       committed position, so the retry meets the swallowed token again instead of resuming at \
       the younger cache entry and skipping it silently. balanced = {balanced}"
    );
  }
}

/// Interrupts the **frontier commit** at the stop exit of a rewinding mode. `commit_at` writes
/// `*ir.state = frontier.state`, and that assignment drops the state it replaces — caller code,
/// inside the commit, and aimable by count.
///
/// Returns `(committed span, surviving emissions, surviving per-token settles, live marks)`.
fn frontier_commit_interrupted(at: usize) -> ((usize, usize), usize, usize, usize) {
  frontier_commit_interrupted_in(at, true).0
}

/// The same over either family, and additionally reporting the committed **lexer state** — the
/// half of the position pair a torn `commit_at` leaves behind.
fn frontier_commit_interrupted_in(
  at: usize,
  balanced: bool,
) -> (((usize, usize), usize, usize, usize), usize) {
  use generic_arraydeque::typenum::U3;

  let cache = <BombCache<'_, BombLexer<'_>> as crate::cache::Cache<'_, BombLexer<'_>, ()>>::new();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCacheCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  let _ = inp.peek::<U3>().unwrap();

  arm_state_drop(at);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    if balanced {
      let _ = inp.sync_balanced(
        |_k: &BombKind| crate::input::Balance::<char>::Neutral,
        |t| t.span_ref().start >= 3,
      );
    } else {
      let _ = inp.sync_to(|t| t.span_ref().start >= 3, || None);
    }
  }));
  disarm_state_drop();
  assert!(caught.is_err(), "the armed state drop must have panicked");

  let s = inp.span();
  (
    (
      (s.start, s.end),
      inp.emitter().emissions,
      inp.emitter().commits.len(),
      inp.emitter().live_rows(),
    ),
    inp.state().scanned,
  )
}

#[test]
fn r9_frontier_commit_interrupted_abandons_rather_than_half_keeping() {
  // The other half of the class, and the half a sticky flag cannot express. At the stop exit the
  // frontier leaves the scope one statement before `commit_at` records it, so the scope's ability
  // to complete the keep changes *during* the exit.
  //
  // NOTE ON WHAT THIS ARMS, corrected after the pair-tearing class was stated properly: the
  // armed step is the drop of the STATE the commit replaced, which happens after `commit_position`
  // has installed both halves. So the position it leaves is WHOLE, not torn — this cell is about
  // the DISPOSITION, not about atomicity, and it cannot observe a tear. (The cell that can is
  // `r9_adopt_pair_is_never_published_half_written`, which arms the replaced SPAN's drop: the span
  // is the first half written, so its drop is the unwind site that sits between the two.)
  //
  // What an interrupted commit leaves here is a frontier already handed over: the scope can no
  // longer perform the keep the stop's disposition asked for, and keeping anyway leaves a
  // rewinding mode's per-token settles describing a prefix the position does not cover, which a
  // retry duplicates. Abandoning rewinds them, which is what a rewinding mode's posture is for.
  //
  // Measured both ways: with the frontier clause the reading is the restore below; without it the
  // scope keeps the torn state and reads ((0, 2), 0, 1, 0).
  assert_eq!(
    frontier_commit_interrupted(2),
    ((0, 0), 0, 0, 0),
    "an interrupted frontier commit must abandon: position restored, the scan's emissions and \
     per-token settles rewound with the mark, nothing outstanding"
  );
}

#[test]
fn r9_state_drop_inventory() {
  // Calibration for the cell above, pinned rather than printed. Drop #2 is the one `commit_at`
  // performs; #1 is earlier in the exit and #3 later, and both read as an untorn outcome. If the
  // sequence moves, the cell is arming a different window and this says so instead of letting it
  // pass on the wrong one.
  assert_eq!(
    (
      frontier_commit_interrupted(1),
      frontier_commit_interrupted(3)
    ),
    (((0, 0), 0, 0, 0), ((0, 2), 0, 1, 0)),
    "the stop exit's `L::State` drop sequence moved; re-aim \
     `r9_frontier_commit_interrupted_abandons_rather_than_half_keeping`"
  );
}

/// Drives a committing scan to end of input with `Lexer::into_state` armed — the caller code
/// that `SyncTo::on_eof` used to run BETWEEN the two halves of the position write — and reports
/// `(committed span, committed state's tally)`.
fn eof_commit_interrupted() -> ((usize, usize), usize) {
  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  arm_into_state(true);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.skip_while(|_| true);
  }));
  arm_into_state(false);
  assert!(caught.is_err(), "the armed `into_state` must have panicked");

  let sp = inp.span();
  ((sp.start, sp.end), inp.state().scanned)
}

#[test]
fn r9_committing_eof_commit_is_atomic_in_span_and_state() {
  // The committing modes have no snapshot, so nothing can restore them — which makes the ATOMICITY
  // of the position write their only protection. `SyncTo::on_eof` wrote the span from
  // `lexer.span()` and then ran `lexer.into_state()`, and both are caller code with one of them
  // BETWEEN the two halves of the pair.
  //
  // Interrupted there it left `span = (11, 11)` — the lexer's end — paired with the ENTRY state,
  // tally 0. A host that catches and resumes then lexes from offset 11 under a state that has seen
  // nothing: silent stream corruption, and only for stateful lexers, which is the population least
  // able to notice. The span and the state must move together or not at all.
  assert_eq!(
    eof_commit_interrupted(),
    ((0, 0), 0),
    "an interrupted end-of-input commit must leave the position pair WHOLE: both halves are \
     computed before either is written, so an unwind in the caller code that produces them lands \
     with nothing written at all"
  );
}

/// A consume whose committed-token **observer** panics, on a warm cache. `driver` picks which
/// consume surface. Returns the tokens the input can still reach afterwards.
fn observer_panic_then_drain(driver: u8) -> (std::vec::Vec<(usize, usize)>, (usize, usize)) {
  use generic_arraydeque::typenum::U3;

  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter {
    panic_on_commit_token: true,
    ..BombEmitter::default()
  };
  let mut input = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  // Three resident, so the token the consume takes has a younger suffix behind it.
  let _ = inp.peek::<U3>().unwrap();

  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match driver {
    0 => {
      let _ = inp.next();
    }
    1 => {
      let _ = inp.try_expect(|_| true);
    }
    _ => {
      let _ = inp.consume_cached_one();
    }
  }));
  assert!(caught.is_err(), "the armed observer must have panicked");
  inp.emitter().panic_on_commit_token = false;

  let committed = {
    let s = inp.span();
    (s.start, s.end)
  };
  let mut drained = std::vec::Vec::new();
  while let Ok(Some(t)) = inp.next() {
    let s = t.span_ref();
    drained.push((s.start, s.end));
  }
  (drained, committed)
}

#[test]
fn r9_observer_panic_on_a_cache_hit_does_not_skip_the_token() {
  // `commit_token` invokes the observer and then publishes the position. On a CACHE-HIT consume
  // that order is wrong in the one way that matters: the token has already been popped off the
  // front stream before `commit_token` is called, so a panicking observer leaves the position
  // behind a token the stream no longer holds, with younger entries still resident in front of
  // it. `cursor()` then reads the younger entry and the token is skipped in silence.
  //
  // Note this is the OPPOSITE ordering from the one that is right when nothing has been removed
  // yet: there, notifying first means a panicking observer publishes nothing. On the consume
  // surface the removal has already happened, so publishing the position is what accounts for it —
  // and a missing observer notification is a documented contract violation rather than a lost
  // token.
  for driver in 0u8..=2 {
    assert_eq!(
      observer_panic_then_drain(driver),
      (std::vec![(3, 5), (6, 8), (9, 11)], (0, 2)),
      "a panicking committed-token observer must leave the position ACCOUNTING for the token \
       the consume had already taken off the front stream. The drain reads the same either way \
       — the token is gone from the cache regardless — so the committed span is the \
       discriminator: at (0, 0) it names a position the stream no longer starts at, and the \
       token has vanished into the gap (driver {driver})"
    );
  }
}

/// Interrupts `AtFrontier::adopt` by arming the drop of the span it REPLACES — the unwind site
/// that sits between adopt's two assignments. Returns `(committed span, committed tally)`.
fn adopt_span_drop_interrupted(at: usize) -> ((usize, usize), usize, usize) {
  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  arm_span_drop(at);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.skip_while(|_| true);
  }));
  disarm_span_drop();
  assert!(caught.is_err(), "the armed span drop must have panicked");

  let s = inp.span();
  (
    (s.start, s.end),
    inp.state().scanned,
    inp.emitter().commits.len(),
  )
}

#[test]
fn r9_adopt_pair_is_never_published_half_written() {
  // `AtFrontier::adopt` writes the scan's OWN span/state pair, and a committing mode's unwind
  // edge commits that frontier — so a tear there reaches the input as one token's span beside the
  // previous token's lexer state.
  //
  // The payload has to arm the drop of the span being REPLACED. An assignment installs its new
  // value and then drops the old one, so the replaced span's `Drop` is an unwind site sitting
  // between adopt's two writes; if it unwinds, the state assignment never runs. Arming the STATE
  // drop instead reaches only the second assignment, which is past both writes — that is a cell
  // that cannot fail, and it is exactly what an earlier revision of this cell did.
  //
  // The assertion is the INVARIANT, not an index: whatever span is committed, the tally beside it
  // must be the one the lexer had after producing that span. Indices shift when the drop order
  // changes — which is precisely what the fix does — so pinning them would re-break this cell for
  // the wrong reason. Before the fix these read span-of-token-N beside the tally after token N-1.
  for at in 1usize..=6 {
    let Ok((span, tally, _)) = std::panic::catch_unwind(|| adopt_span_drop_interrupted(at)) else {
      continue; // that index is not a reachable span drop in this run
    };
    let want = match span {
      (0, 0) => 0,
      (0, 2) => 1,
      (3, 5) => 2,
      (6, 8) => 3,
      (9, 11) | (11, 11) => 4,
      other => panic!("unexpected committed span {other:?}"),
    };
    assert_eq!(
      tally, want,
      "the frontier published a half-written pair when the replaced span's drop at index {at} \
       panicked: span {span:?} beside tally {tally}, but that span was produced with tally \
       {want}. Install both halves before letting either replaced value drop."
    );
  }
}

/// A warm-cache consume with the drop of the span it REPLACES armed. Returns
/// `(committed span, tokens the observer saw)`.
fn consume_replaced_span_drop(at: usize) -> ((usize, usize), usize) {
  use generic_arraydeque::typenum::U3;

  let cache = DefaultCache::<'_, BombLexer<'_>>::default();
  let mut emitter = BombEmitter::default();
  let mut input = Input::<BombLexer<'_>, BombCtx<'_>, ()>::with_state_and_cache(
    "ab cd ef gh",
    BombTally::default(),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);
  let _ = inp.peek::<U3>().unwrap();

  arm_span_drop(at);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = inp.next();
  }));
  disarm_span_drop();
  assert!(caught.is_err(), "the armed span drop must have panicked");

  let s = inp.span();
  ((s.start, s.end), inp.emitter().commits.len())
}

#[test]
fn r9_consume_notifies_the_observer_before_the_replaced_pair_drops() {
  // The other door into the missing-notification hole. `commit_token` publishes the position and
  // then notifies the committed-token observer — but if it publishes through a helper that drops
  // the replaced pair before returning, those drops are caller code sitting between the publish
  // and the notify. A panicking `L::Span::Drop` there leaves the token consumed and the position
  // advanced with the observer never told: a CST sink or event log silently misses a token that
  // the input has moved past.
  //
  // So the consume holds the replaced pair, notifies, and only then lets it go. Asserted on both
  // halves at once — the position advanced AND the observer saw it — because either alone passes
  // for the wrong reason.
  for at in 1usize..=2 {
    assert_eq!(
      consume_replaced_span_drop(at),
      ((0, 2), 1),
      "a panicking drop of the replaced span at index {at} must not swallow the observer's \
       notification: the position advanced past the token, so the observer has to have seen it"
    );
  }
}

#[test]
fn r9_skip_notifies_the_observer_before_the_adopted_pair_drops() {
  // The skip path's copy of the notify-before-drop discipline. `skip_and_report` adopts the token
  // into the frontier and then calls the committed-token observer — but `adopt` drops the pair it
  // replaced before returning, and those drops are caller code. A panicking replaced-SPAN drop
  // therefore skips the observer call, while a committing mode's unwind edge still keeps and
  // commits that frontier: progress published, observer never told.
  //
  // Asserted on both halves, because either alone passes for the wrong reason: the frontier must
  // have advanced to the token AND the observer must have seen it.
  for at in 1usize..=4 {
    let Ok((span, tally, seen)) = std::panic::catch_unwind(|| adopt_span_drop_interrupted(at))
    else {
      continue;
    };
    let want_tally = match span {
      (0, 0) => 0,
      (0, 2) => 1,
      (3, 5) => 2,
      (6, 8) => 3,
      (9, 11) | (11, 11) => 4,
      other => panic!("unexpected committed span {other:?}"),
    };
    assert_eq!(
      (tally, seen),
      (want_tally, want_tally),
      "at index {at} the committed span {span:?} was published with tally {tally} and {seen} \\
       observed settle(s); both must equal {want_tally} — the frontier advanced past tokens the \\
       observer was never told about"
    );
  }
}

// ── State surgery's own window (Codex, round 12) ────────────────────────────

/// `set_state` re-keys every offset-dependent fact and installs a new `L::State`. Both halves run
/// caller `Drop` code, and more of it than the site looks like it does: a `CachedToken` carries
/// `state: L::State`, so the re-key's cache clear and parked-slot clear each drop caller states,
/// and the state write drops the one it displaced.
///
/// So this does not arm one drop and call the site covered — the first ordinal is a *cached*
/// state, not the replaced one, and arming only it would have passed against a torn body. It
/// sweeps every ordinal a `set_state` reaches and demands the same five facts at each: state
/// surgery is all-or-nothing no matter which caller `Drop` fails.
#[test]
fn r9_set_state_is_atomic_at_every_caller_drop() {
  use generic_arraydeque::typenum::U2;

  // One scenario, replayed. `run(None)` counts the caller drops a whole `set_state` performs;
  // `run(Some(n))` detonates the n-th and returns what a host that caught it can observe.
  fn run(bomb_at: Option<usize>) -> (usize, usize, usize, usize, bool, bool) {
    let cache = DefaultCache::<'_, BombLexer<'_>>::default();
    let mut emitter = BombEmitter::default();
    let mut input =
      Input::<BombLexer<'_>, BombCtx<'_>, (), crate::input::Complete>::with_state_and_cache(
        "ab cd ef gh",
        BombTally::default(),
        cache,
      );
    let mut inp = input.as_ref(&mut emitter);

    // Give the re-key something to tear down: cached entries, each carrying an `L::State`.
    let _ = inp.peek::<U2>();
    assert!(
      !inp.cache().is_empty(),
      "the cache must be non-empty or the re-key drops nothing and the sweep proves nothing"
    );

    arm_state_drop(bomb_at.unwrap_or(0));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      inp.set_state(BombTally {
        scanned: 7,
        limit: 99,
      });
    }));
    disarm_state_drop();
    let drops = STATE_DROPS.with(Cell::get);
    assert_eq!(
      caught.is_err(),
      bomb_at.is_some(),
      "the armed drop must panic and the unarmed run must not"
    );
    (
      drops,
      inp.state().scanned,
      inp.state().limit,
      inp.cache().len(),
      inp.has_front_parked(),
      inp.is_poisoned(),
    )
  }

  let (total, ..) = run(None);
  assert!(
    total >= 2,
    "the sweep needs at least a cached state and the displaced one to be meaningful, saw {total}"
  );

  for n in 1..=total {
    let (_, scanned, limit, cached, parked, poisoned) = run(Some(n));
    assert!(
      scanned == 7 && limit == 99,
      "caller drop #{n} of {total}: the new state must be installed whole — `mem::replace` puts \
       it in before any caller code runs, so no `Drop` failure can leave the old one, or half of \
       the new one, behind (saw scanned={scanned}, limit={limit})"
    );
    assert!(
      !parked && !poisoned,
      "caller drop #{n} of {total}: the parked slot and the poison latch are settled by `take()` \
       before anything can panic, so they are cleared however the re-key ends (parked={parked}, \
       poisoned={poisoned})"
    );
    let _ = cached;
  }
}

// ── An armable `L::Offset` (Codex, round 12) ────────────────────────────────

thread_local! {
  /// `BombOffset::clone` calls since the last arm, and the index that panics (0 = disarmed).
  static OFFSET_CLONES: Cell<usize> = const { Cell::new(0) };
  static OFFSET_BOMB: Cell<usize> = const { Cell::new(0) };
}

fn arm_offset_clone(at: usize) {
  OFFSET_CLONES.with(|c| c.set(0));
  OFFSET_BOMB.with(|c| c.set(at));
}

fn disarm_offset_clone() -> usize {
  OFFSET_BOMB.with(|c| c.set(0));
  OFFSET_CLONES.with(Cell::get)
}

/// An `L::Offset` whose `Clone` can be armed, and the source and lexer needed to reach one.
///
/// Every offset the crate ships in-tree is `usize`, so `L::Offset::clone` — caller code that the
/// settle and restore paths both run — had no witness at all, and the note in
/// `r9_f2_panicking_eof_settle_still_settles_the_mark` said exactly that. Two rounds of findings
/// against `restore_entry` were argued on contract grounds for want of this type.
///
/// It brings its own `Source` and `Lexer` rather than re-keying `BombLexer`: `str` has a single
/// `Source<usize>` impl and the crate leans on that being unique for inference, so adding a second
/// one breaks type resolution in `source` and `completeness` — 44 errors, in production modules,
/// to buy a test instrument. A private wrapper costs nothing outside this file.
#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct BombOffset(usize);

impl Clone for BombOffset {
  fn clone(&self) -> Self {
    let n = OFFSET_CLONES.with(|c| {
      let v = c.get() + 1;
      c.set(v);
      v
    });
    assert!(
      n != OFFSET_BOMB.with(Cell::get),
      "R9 settle-path: the armed `L::Offset` clone (#{n}, at {}) panics",
      self.0
    );
    Self(self.0)
  }
}

#[derive(Debug)]
struct BombSrc<'a>(&'a str);

impl crate::Source<BombOffset> for BombSrc<'_> {
  type Slice<'source>
    = &'source str
  where
    Self: 'source;

  fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  fn len(&self) -> BombOffset {
    BombOffset(self.0.len())
  }

  fn as_slice(&self) -> Self::Slice<'_> {
    self.0
  }

  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: core::ops::RangeBounds<BombOffset>,
  {
    self.0.get((
      range.start_bound().map(|s| s.0),
      range.end_bound().map(|s| s.0),
    ))
  }

  fn find_boundary(&self, index: BombOffset) -> BombOffset {
    if index.0 >= self.0.len() {
      return index;
    }
    let mut i = index.0;
    while !self.0.is_char_boundary(i) {
      i -= 1;
    }
    BombOffset(i)
  }

  fn is_boundary(&self, index: BombOffset) -> bool {
    self.0.is_char_boundary(index.0)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct OffsetSpan {
  start: BombOffset,
  end: BombOffset,
}

impl crate::Span for OffsetSpan {
  type Offset = BombOffset;

  fn new(start: BombOffset, end: BombOffset) -> Self {
    Self { start, end }
  }

  fn into_range(self) -> core::ops::Range<BombOffset> {
    self.start..self.end
  }

  fn start_ref(&self) -> &BombOffset {
    &self.start
  }

  fn start_mut(&mut self) -> &mut BombOffset {
    &mut self.start
  }

  fn into_start(self) -> BombOffset {
    self.start
  }

  fn end_ref(&self) -> &BombOffset {
    &self.end
  }

  fn end_mut(&mut self) -> &mut BombOffset {
    &mut self.end
  }

  fn into_end(self) -> BombOffset {
    self.end
  }

  fn bump(&mut self, n: &BombOffset) {
    self.end.0 += n.0;
  }
}

struct OffsetLexer<'a> {
  src: &'a BombSrc<'a>,
  start: usize,
  end: usize,
  state: BombTally,
}

impl<'a> crate::Lexer<'a> for OffsetLexer<'a> {
  type State = BombTally;
  type Source = BombSrc<'a>;
  type Token = BombTok;
  type Span = OffsetSpan;
  type Offset = BombOffset;

  fn new(src: &'a BombSrc<'a>) -> Self {
    Self::with_state(src, BombTally::default())
  }

  fn with_state(src: &'a BombSrc<'a>, state: BombTally) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), BombErr> {
    State::check(&self.state).map_err(BombErr::from)
  }

  fn state(&self) -> &BombTally {
    &self.state
  }

  fn state_mut(&mut self) -> &mut BombTally {
    &mut self.state
  }

  fn into_state(self) -> BombTally {
    self.state
  }

  fn source(&self) -> &'a BombSrc<'a> {
    self.src
  }

  fn span(&self) -> OffsetSpan {
    OffsetSpan {
      start: BombOffset(self.start),
      end: BombOffset(self.end),
    }
  }

  fn slice(&self) -> &'a str {
    &self.src.0[self.start..self.end]
  }

  fn lex(&mut self) -> Option<Result<BombTok, BombErr>> {
    let bytes = self.src.0.as_bytes();
    let mut i = self.end;
    while i < bytes.len() && bytes[i] == b' ' {
      i += 1;
    }
    if i >= bytes.len() {
      self.start = i;
      self.end = i;
      return None;
    }
    self.start = i;
    while i < bytes.len() && bytes[i] != b' ' {
      i += 1;
    }
    self.end = i;
    self.state.scanned += 1;
    if State::check(&self.state).is_err() {
      return Some(Err(BombErr::Limit));
    }
    Some(Ok(BombTok))
  }

  fn bump(&mut self, n: &BombOffset) {
    self.end += n.0;
  }
}

type OffsetCtx<'a> = (BombEmitter, DefaultCache<'a, OffsetLexer<'a>>);

/// `restore_entry` — the site carried as a disclosed limitation for two rounds, now measured.
///
/// A rewinding scan that reaches end of input without matching restores its full entry state and
/// rewinds the emitter to it. The rewind cursor used to be read back off the input AFTER the
/// position was written (`self.cursor().clone()`), putting an `L::Offset::clone` between the
/// position write and the rewind that completes it. A panic there left the input restored with
/// the mark un-rewound, and `ScanScope::drop` then RELEASED it — so an abandoned scan's
/// diagnostics became permanent while the parser retried from the restored position.
///
/// The read now comes off the entry's own span end, hoisted above every mutation. This sweeps
/// every offset clone the call performs and demands the same outcome at each: no stranded mark,
/// and a position that is either fully restored or never moved — never one with the other's half.
#[test]
fn r9_restore_entry_is_atomic_at_every_offset_clone() {
  fn run(bomb_at: Option<usize>) -> (usize, usize, usize, usize) {
    let src = BombSrc("ab cd ef gh");
    let cache = DefaultCache::<'_, OffsetLexer<'_>>::default();
    let mut emitter = BombEmitter::default();
    let mut input =
      Input::<OffsetLexer<'_>, OffsetCtx<'_>, (), crate::input::Complete>::with_state_and_cache(
        &src,
        BombTally::default(),
        cache,
      );
    let mut inp = input.as_ref(&mut emitter);

    arm_offset_clone(bomb_at.unwrap_or(0));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      // Never matches, so the scan skips (and diagnoses) every token and leaves through the
      // end-of-input arm — the one exit that reaches `restore_entry`.
      let _ = inp.sync_through(|_| false, || None);
    }));
    let clones = disarm_offset_clone();
    assert_eq!(
      caught.is_err(),
      bomb_at.is_some(),
      "the armed clone must panic and the unarmed run must not"
    );
    let resumed = inp.cursor().as_inner().0;
    let em = inp.emitter();
    (clones, resumed, em.live_rows(), em.emissions)
  }

  let (total, resumed, live, emissions) = run(None);
  assert!(
    total >= 2,
    "the sweep needs several offset clones to be meaningful, saw {total}"
  );
  assert!(
    resumed == 0 && live == 0 && emissions == 0,
    "the clean run must restore to the entry position, strand nothing, and rewind away every \
     diagnostic the abandoned scan made (at {resumed}, {live} live, {emissions} emissions)"
  );

  for n in 1..=total {
    let (_, resumed, live, emissions) = run(Some(n));
    assert!(
      live == 0,
      "offset clone #{n} of {total}: the entry mark must be settled exactly once however the \
       call unwinds — a stranded mark is the emitter holding a branch nobody will ever rewind \
       or release ({live} left live)"
    );
    assert!(
      resumed == 0,
      "offset clone #{n} of {total}: the scan committed nothing, so a caught panic must leave \
       the position at entry (resumed at {resumed})"
    );
    // The one that matters, and the one a position check alone cannot see. A mark settled by
    // RELEASE instead of by REWIND leaves the input correctly rewound and the emitter still
    // holding the abandoned scan's diagnostics — same position, same live-row count, permanently
    // wrong log. Only the emission count separates them.
    assert!(
      emissions == 0,
      "offset clone #{n} of {total}: the abandoned scan's diagnostics must be REWOUND away, not \
       released — {emissions} survived. The rewind cursor must be captured with the entry, \
       before the mark exists; any fallible step inside `restore_entry` skips the rewind and \
       `ScanScope::drop` then releases the mark, making an abandoned branch's output permanent"
    );
  }
}

// ── The whole restore, not its tail (Codex, round 13) ───────────────────────

/// `restore_unchecked` is the body four consecutive rounds each found something in. This is the
/// measurement for its phase-separated form.
///
/// A checkpoint rollback is all-or-nothing, so the assertion is exactly that: for every armed
/// caller `Drop`, the input must end up **either fully restored or fully unchanged**, never
/// between. That is stronger than the split check this cell used to make, which only looked at
/// one pairing (emitter rewound, position not) and so read clean against the eviction hazard.
///
/// The fixture leaves **resident post-save cache entries** — the previous one drained to end of
/// input, which left the tail prune with nothing to pop and the round-14 hazard unexercised.
/// Each resident entry is a `CachedToken` owning an `L::State`, so pruning it runs caller code.
///
/// What this cell does and does not guard, plainly:
///
/// - **Armed and asserted**: every `L::State::drop` the rollback performs (three of them here,
///   from the evicted entries and the replaced pair) must leave the input all-or-nothing, and the
///   body must perform zero `L::Span` clones. Both hold.
/// - **NOT demonstrated**: that this cell catches an eviction moved BELOW the phase boundary. I
///   relocated the tail prune and `reconcile_cache_geometry` in turn and no observable moved —
///   the two evictors are interchangeable here (whichever runs first drains the cache, and the
///   other then finds `survivors == 0`), and I could not build a fixture that separates them. So
///   the phase ordering is guarded by the RAIL, whose teeth are demonstrated on all three
///   eviction shapes, and not by this cell. It is a regression guard on the property, not on the
///   ordering that produces it.
/// - **Held by construction, not measured**: the parked slot's drop, unreachable because
///   `DefaultCache` accepts a front push so nothing ever parks; and the abandoned session points'
///   `Checkpoint` drops, which need a nested point stack this fixture does not build.
#[test]
fn r9_restore_unchecked_is_all_or_nothing_at_every_caller_drop() {
  use generic_arraydeque::typenum::U2;

  /// `(state_ops, span_ops, cursor, rewinds, poisoned)` — the cache length is deliberately NOT
  /// observed: a cache is a pure memo, so a partly-evicted one is indistinguishable from a full
  /// one to any consumer, and demanding it be all-or-nothing would be demanding the wrong thing.
  fn run(state_bomb: usize, span_bomb: usize, restore: bool) -> (usize, usize, usize, usize, bool) {
    let cache = DefaultCache::<'_, BombLexer<'_>>::default();
    let mut emitter = BombEmitter::default();
    let mut input =
      Input::<BombLexer<'_>, BombCtx<'_>, (), crate::input::Complete>::with_state_and_cache(
        "ab cd ef gh ij kl",
        BombTally::default(),
        cache,
      );
    let mut inp = input.as_ref(&mut emitter);

    // Save with the cache EMPTY, so the checkpoint's cursor equals the position the post-save
    // entries will start at. `reconcile_cache_geometry` then finds `cursor == front_start` and
    // returns without popping, which leaves the tail prune as the ONLY evictor — and therefore
    // the only owner of the armed drops. With both evictors able to do the work they are
    // interchangeable, whichever runs first drains the cache, and moving either one across the
    // phase boundary changes nothing observable: the fixture reads clean against the very defect
    // it exists to catch.
    let _ = inp.next();
    let ckp = inp.save();
    // Post-save pushes, still resident at restore time. Each is a `CachedToken` owning an
    // `L::State`, so pruning it runs caller code.
    let _ = inp.peek::<U2>();

    arm_state_drop(state_bomb);
    arm_span_clone(span_bomb);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      if restore {
        inp.restore(ckp);
      }
    }));
    disarm_state_drop();
    disarm_span_clone();
    let state_ops = STATE_DROPS.with(Cell::get);
    let span_ops = SPAN_CLONES.with(Cell::get);
    let _ = caught;

    // The COMMITTED span end, not `cursor()`. `cursor()` reports the cache front when there is
    // one and `span.end` when there is not, so evicting a cache entry moves it — and eviction is
    // exactly the thing this body is allowed to do before the rollback begins. Reading the
    // committed position instead asks the semantic question: has the input moved branches?
    let committed = *crate::Span::end_ref(&*inp.span);
    let poisoned = inp.is_poisoned();
    let em = inp.emitter();
    (state_ops, span_ops, committed, em.rewinds, poisoned)
  }

  let (state_total, span_total, c_done, r_done, p_done) = run(0, 0, true);
  assert!(
    state_total >= 1,
    "the fixture must leave resident post-save entries for the prune to evict — each carries the \
     `L::State` whose drop this sweeps — saw {state_total}"
  );
  assert!(
    span_total == 0,
    "a restore ran {span_total} `L::Span` clone(s); the body must move its span in, not borrow it"
  );

  // The two admissible outcomes. `restored` is what a clean rollback produces; `untouched` is
  // what the input looks like if the rollback never began.
  let restored = (c_done, r_done, p_done);
  // The genuine pre-restore state, taken by NOT restoring. Arming an ordinal that never fires
  // reproduces the restored state instead, which would make the two references identical and the
  // assertion vacuous — it silently did, until this cell reported a legitimate outcome as a
  // violation.
  let untouched = {
    let (_, _, c, r, p) = run(0, 0, false);
    (c, r, p)
  };
  assert!(
    restored != untouched,
    "the fixture must actually move the input, or the all-or-nothing check proves nothing \
     (both states are {restored:?})"
  );

  for n in 1..=state_total {
    let (_, _, c, r, p) = run(n, 0, true);
    assert!(
      (c, r, p) == restored || (c, r, p) == untouched,
      "evicted-entry state drop #{n} of {state_total}: the rollback landed between its two \
       admissible outcomes — got (committed {c}, rewinds {r}, poisoned {p}), which is neither \
       fully restored {restored:?} nor fully unchanged {untouched:?}. Every eviction must sit \
       above the phase boundary, where a panic leaves the input on the branch it was already on. \
       (The cache's residency is deliberately not part of this comparison — a memo may be partly \
       evicted without the input having moved.)"
    );
  }
}

/// `set_state` — state surgery's own fallible step, measured with the armable `L::Offset`.
///
/// The re-key clones the committed position, and `L::Offset::clone` is caller code. While that
/// clone sat below the state write, an unwind in it left the input carrying the NEW lexer state
/// beside a cache, poison latch and dedup watermark still keyed to the OLD regime — a state
/// surgery half-applied, and the halves disagreeing about which regime the input is in.
///
/// The violation is stated as exactly that pairing: the new state may only be observed together
/// with re-keyed facts. Either both moved or neither did.
#[test]
fn r9_set_state_is_atomic_at_every_offset_clone() {
  use generic_arraydeque::typenum::U2;

  /// `(clones, scanned, cache_len)`
  fn run(bomb_at: usize) -> (usize, usize, usize) {
    let src = BombSrc("ab cd ef gh");
    let cache = DefaultCache::<'_, OffsetLexer<'_>>::default();
    let mut emitter = BombEmitter::default();
    let mut input =
      Input::<OffsetLexer<'_>, OffsetCtx<'_>, (), crate::input::Complete>::with_state_and_cache(
        &src,
        BombTally::default(),
        cache,
      );
    let mut inp = input.as_ref(&mut emitter);

    // Old-regime facts the re-key is supposed to clear, so "not re-keyed" is observable.
    let _ = inp.peek::<U2>();
    assert!(
      !inp.cache().is_empty(),
      "the fixture needs a populated cache or the re-key clears nothing observable"
    );

    arm_offset_clone(bomb_at);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      inp.set_state(BombTally {
        scanned: 7,
        limit: 99,
      });
    }));
    let clones = disarm_offset_clone();
    (clones, inp.state().scanned, inp.cache().len())
  }

  let (total, scanned, cached) = run(0);
  assert!(
    total >= 1,
    "the sweep needs `set_state` to perform at least one offset clone, saw {total}"
  );
  assert!(
    scanned == 7 && cached == 0,
    "the clean run must install the new state AND re-key the facts (scanned={scanned}, \
     cache={cached})"
  );

  for n in 1..=total {
    let (_, scanned, cached) = run(n);
    let installed = scanned == 7;
    let rekeyed = cached == 0;
    assert!(
      installed == rekeyed,
      "offset clone #{n} of {total}: the input carries state scanned={scanned} beside a cache of \
       {cached} — the new lexer state and the re-keyed facts came apart. Every fallible step of a \
       re-key must run ABOVE the state write, so an unwind leaves the whole surgery undone \
       rather than half of it applied"
    );
  }
}

// ---------------------------------------------------------------------------
// `try_expect_take` / `try_expect_take_or_stop`.
//
// The or_stop asymmetry IS the contract, so the pair is one cell: at a latched
// boundary the or_stop form raises and the non-stop form folds to `Ok(None)`. A
// body that folds both — or raises in both — flips it.
// ---------------------------------------------------------------------------

#[test]
fn wapi_b_try_expect_take_or_stop_errs_at_a_latched_boundary() {
  let (mut input, _scanned) = probe_input("1 2 3");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(inp.next().unwrap().is_some());
  assert!(inp.next().unwrap().is_some());
  assert!(
    inp.next().unwrap().is_none(),
    "the third scan trips and latches"
  );
  assert!(
    matches!(
      inp.try_expect_take_or_stop(|_| true, |sp| Ok(sp.span.start())),
      Err(ProbeErr::Eot)
    ),
    "or_stop take treats the trip as terminal, never as absence"
  );
  // The non-stop twin documents the fold — this asymmetry IS the or_stop contract.
  assert!(
    matches!(
      inp.try_expect_take(|_| true, |sp| Ok(sp.span.start())),
      Ok(None)
    ),
    "the non-stop take folds the latch into Ok(None), as try_expect does"
  );
}

#[test]
fn wapi_b_try_expect_take_classifies_before_it_takes() {
  // A declined classification must not consume: the follow-up take sees the same
  // head. If `take` ran before `classify`, the second call would read the *next*
  // token (or none at all) and the span would not be 0..1.
  let (mut input, _scanned) = probe_input("1 2");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(
    matches!(
      inp.try_expect_take(|_| false, |sp| Ok(sp.span.start())),
      Ok(None)
    ),
    "a declined classification takes nothing"
  );
  assert_eq!(
    inp.try_expect_take(|_| true, |sp| Ok((sp.span.start(), sp.span.end()))),
    Ok(Some((0, 1))),
    "the declined head is still at the front — the give-back is real"
  );
}

#[test]
fn wapi_b_try_expect_take_project_error_is_a_real_error_post_commit() {
  // `project` runs after the commit, so its error is an error and not a decline —
  // and the token it errored on is gone: the next take reads the SECOND token.
  let (mut input, _scanned) = probe_input("1 2");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  let out: Result<Option<usize>, ProbeErr> =
    inp.try_expect_take(|_| true, |_sp| Err(ProbeErr::Lex));
  assert_eq!(
    out,
    Err(ProbeErr::Lex),
    "a project error propagates as an error"
  );
  assert_eq!(
    inp.try_expect_take(|_| true, |sp| Ok((sp.span.start(), sp.span.end()))),
    Ok(Some((2, 3))),
    "the projected-over token was committed before `project` ran"
  );
}

// ---------------------------------------------------------------------------
// The terminal-vs-EOF distinction, executed at a latched
// boundary. A body that folds a trip into `Ok(None)` flips every one of them.
// ---------------------------------------------------------------------------

#[test]
fn wapi_b_peek_kind_is_terminal_aware_at_a_latched_boundary() {
  let (mut input, _scanned) = probe_input("1 2 3");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(inp.next().unwrap().is_some());
  assert!(inp.next().unwrap().is_some());
  assert!(
    inp.next().unwrap().is_none(),
    "the third scan trips and latches"
  );
  assert!(
    matches!(inp.peek_kind(), Err(ProbeErr::Eot)),
    "a latched boundary is an error from peek_kind, never a silent None"
  );
  assert!(
    matches!(inp.head_satisfies(|_| true), Err(ProbeErr::Eot)),
    "head_satisfies raises at the latch instead of answering false"
  );
  assert!(
    matches!(inp.peek_head_map(|sp| sp.span.start()), Err(ProbeErr::Eot)),
    "peek_head_map raises at the latch"
  );
  // The control: the raw read this family exists to replace folds the same latch.
  assert!(
    matches!(inp.peek_one(), Ok(None)),
    "peek_one still folds a latch into Ok(None) — that IS the defect the family fixes"
  );
}

#[test]
fn wapi_b_peek_kind_reads_genuine_eof_as_none() {
  // The positive twin: under the limit, exhaustion is Ok(None), not an error.
  let (mut input, _scanned) = probe_input("1");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(inp.next().unwrap().is_some());
  assert!(
    matches!(inp.peek_kind(), Ok(None)),
    "genuine end of input peeks as None"
  );
  assert!(
    matches!(inp.head_satisfies(|_| true), Ok(false)),
    "and head_satisfies answers false there rather than raising"
  );
}

#[test]
fn wapi_b_peek_head_map_does_not_consume_the_head() {
  let (mut input, _scanned) = probe_input("1 2");
  let mut emitter = Silent::<ProbeErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert_eq!(
    inp.peek_head_map(|sp| (sp.span.start(), sp.span.end())),
    Ok(Some((0, 1)))
  );
  assert_eq!(
    inp.peek_head_map(|sp| (sp.span.start(), sp.span.end())),
    Ok(Some((0, 1))),
    "a peek is not a take — the second read sees the same head"
  );
  let taken = inp
    .next()
    .unwrap()
    .expect("the head is still there to consume");
  assert_eq!(taken.span_ref().start(), 0);
}

// ---------------------------------------------------------------------------
// The terminal **mark**, not merely "an error".
//
// `ProbeErr`'s `From<UnexpectedEot>` discards every field, so the cells above
// prove `Err` rather than `Ok(None)` — they cannot see whether `.into_terminal()`
// was applied. `TermErr` preserves the flag, so these cells fail if the mark is
// dropped, and they fail the other way too if it were applied unconditionally:
// the genuine-EOF arm asserts `is_terminal() == false`.
//
// The whole lexer stack is reused; only the emitter's error type changes, so
// this block is pure addition beside the existing `ProbeErr` fixtures.
// ---------------------------------------------------------------------------

/// A probe error that **keeps** the terminal flag `UnexpectedEot` carries.
#[derive(Debug, Clone, PartialEq)]
enum TermErr {
  Lex,
  Limit,
  /// `terminal` is `UnexpectedEot::is_terminal()` at the moment of conversion.
  Eot {
    terminal: bool,
  },
}

impl From<()> for TermErr {
  fn from(_: ()) -> Self {
    TermErr::Lex
  }
}

impl From<ProbeErr> for TermErr {
  fn from(e: ProbeErr) -> Self {
    match e {
      ProbeErr::Limit => TermErr::Limit,
      _ => TermErr::Lex,
    }
  }
}

impl From<ProbeLimitExceeded> for TermErr {
  fn from(_: ProbeLimitExceeded) -> Self {
    TermErr::Limit
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for TermErr {
  fn from(e: UnexpectedEot<O, Lang, Set>) -> Self {
    TermErr::Eot {
      terminal: e.is_terminal(),
    }
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for TermErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    TermErr::Lex
  }
}

impl<'a, L: crate::Lexer<'a>, Lang: ?Sized> crate::emitter::FromUnclosed<'a, L, Lang> for TermErr {
  fn from_unclosed<D>(_: crate::error::Unclosed<D, L::Span, Lang>) -> Self {
    TermErr::Lex
  }
}

type TermCtx<'a> = (Silent<TermErr>, DefaultCache<'a, ProbeLexer<'a>>);

/// The `probe_input` fixture retyped onto the terminal-preserving error.
fn term_input(src: &str) -> Input<'_, ProbeLexer<'_>, TermCtx<'_>, ()> {
  let limiter = ProbeLimiter::with_limit(2);
  let cache = DefaultCache::<'_, ProbeLexer<'_>>::default();
  Input::<ProbeLexer<'_>, TermCtx<'_>, ()>::with_state_and_cache(src, limiter, cache)
}

#[test]
fn wapi_b_peek_kind_marks_the_eot_terminal_at_a_latched_boundary() {
  let mut input = term_input("1 2 3");
  let mut emitter = Silent::<TermErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(inp.next().unwrap().is_some());
  assert!(inp.next().unwrap().is_some());
  assert!(
    inp.next().unwrap().is_none(),
    "the third scan trips and latches"
  );
  assert_eq!(
    inp.peek_kind(),
    Err(TermErr::Eot { terminal: true }),
    "peek_kind's latched EOT must be MARKED terminal, not merely an error"
  );
}

#[test]
fn wapi_b_peek_kind_leaves_a_genuine_eof_unmarked() {
  // The other direction: a mark applied unconditionally would flip this.
  let mut input = term_input("1");
  let mut emitter = Silent::<TermErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(inp.next().unwrap().is_some());
  assert_eq!(
    inp.peek_kind(),
    Ok(None),
    "genuine end of input is not an error at all here"
  );
}
