//! Cache-capacity independence of a `to`-shaped scan stop and of every decline.
//!
//! The scanner's law is that *an unconsumed token lives at the front of the stream* — and the
//! promise it buys is that the cursor after a stop is the stopping token's start no matter who
//! lexed it. A cache that retains nothing (the blackhole `()`) is where that promise is
//! cheapest to break: every put-back is refused, so the token the scan decided not to consume
//! is simply gone, the cursor falls back to the committed position, and `is_eoi` answers a
//! different question than it does under a cache with room.
//!
//! These tests drive one program under three capacities — a three-slot deque, a one-slot
//! `Option`, and `()` — and require the whole observable tuple to be identical. `cache().len()`
//! is deliberately excluded: it is legitimately a function of the capacity. Everything else is
//! a function of the token stream.
//!
//! The fixture is [`BalLexer`] behind the by-value `TokenLimiter`, so the lexer-state tally is
//! part of the compared tuple: a capacity that drops a token and re-lexes it later shows up
//! there as well as in the positions.

use generic_arraydeque::typenum::{U1, U3};

use crate::{
  InputRef, Token, Window,
  cache::{Cache, CachedTokenOf, DefaultCache, Peeked, PeekedTokenExt},
  emitter::Verbose,
  input::Input,
  span::SimpleSpan,
  state::token_tracker::TokenLimiter,
};

use super::{
  CloseStatus,
  tests::{BalKind, BalLexer, ByValErr, parens},
};

/// A one-slot cache: the smallest capacity that can still retain a token.
type OneSlot<'a> = Option<CachedTokenOf<'a, BalLexer<'a>>>;

/// The program a capacity comparison runs before its observables are read.
#[derive(Clone, Copy, Debug)]
enum Program {
  /// Consume the first number, then sync to the second one: the `to`-shaped stop, on the token
  /// the scan lexed itself.
  SyncToStop,
  /// The trivia skip, which is the same stop under a different mode.
  SkipWhileStop,
  /// The balanced sync, whose returned hole is anchored at the cursor the stop leaves.
  SyncBalancedStop,
  /// The balanced sync stopping on the very next token: its zero-skip hole is anchored at
  /// `cursor()` itself, so a stop that dropped its token returns a different *value*.
  SyncBalancedZeroSkip,
  /// A stop, then a peek wide enough to trip the limiter: the fill must serve the stopped
  /// token from the front and lex only past it.
  PeekPastStopTrips,
  /// The same, then a further peek taken with the boundary already latched — the arm that
  /// returns whatever is at the front without touching the lexer.
  PeekAfterLatchedBoundary,
  /// `try_expect` declines the token at the cursor on the scan path.
  TryExpectDecline,
  /// `try_expect_map` declines.
  TryExpectMapDecline,
  /// `try_expect_and_then` declines.
  TryExpectAndThenDecline,
  /// `try_expect_or_stop` declines — its `Ok(None)` is documented as definite absence.
  TryExpectOrStopDecline,
  /// `try_expect_map_or_stop` declines, likewise.
  TryExpectMapOrStopDecline,
  /// `probe_close` classifies the token at the cursor as the wrong one, on the scan path.
  ProbeCloseWrongToken,
}

impl Program {
  /// Every program, so a new one cannot be added without joining the comparison.
  const ALL: &'static [Program] = &[
    Program::SyncToStop,
    Program::SkipWhileStop,
    Program::SyncBalancedStop,
    Program::TryExpectDecline,
    Program::TryExpectMapDecline,
    Program::TryExpectAndThenDecline,
    Program::TryExpectOrStopDecline,
    Program::TryExpectMapOrStopDecline,
    Program::ProbeCloseWrongToken,
    Program::SyncBalancedZeroSkip,
    Program::PeekPastStopTrips,
    Program::PeekAfterLatchedBoundary,
  ];

  /// The token budget the by-value limiter is built with. Only the peek programs need a
  /// binding one; everything else runs unlimited so a trip cannot confound the comparison.
  const fn limit(self) -> usize {
    match self {
      Program::PeekPastStopTrips | Program::PeekAfterLatchedBoundary => 2,
      _ => usize::MAX,
    }
  }

  /// The source each program runs over. All of them end on the token the scan stops before or
  /// declines, so the stop's own `is_eoi` is exercised rather than a mid-buffer position.
  const fn src(self) -> &'static str {
    match self {
      // `"1   2"`: two numbers with a lexer-skipped gap between them, so the end of one token
      // and the start of the next are different offsets and a cursor placed at the former is
      // visible.
      Program::SyncToStop
      | Program::TryExpectDecline
      | Program::TryExpectMapDecline
      | Program::TryExpectAndThenDecline
      | Program::TryExpectOrStopDecline
      | Program::TryExpectMapOrStopDecline
      | Program::ProbeCloseWrongToken => "1   2",
      // A run of real trivia tokens, then the token the skip stops on.
      Program::SkipWhileStop => "~ ~   2",
      // An unbalanced region, then the `;` the balanced sync stops before.
      Program::SyncBalancedStop => "1 ( 2 )   ;",
      // The sync point is the very next token, so nothing is skipped.
      Program::SyncBalancedZeroSkip => "1   ;",
      // Three numbers under a two-token budget: the stop lands on the second, and a peek past
      // it reaches the third, which trips.
      Program::PeekPastStopTrips | Program::PeekAfterLatchedBoundary => "1   2   3",
    }
  }
}

/// Everything a caller can observe after the program has run, bar `cache().len()`.
#[derive(Debug, PartialEq)]
struct Observed {
  cursor: usize,
  offset: usize,
  is_eoi: bool,
  is_exhausted: bool,
  span: SimpleSpan,
  tokens: usize,
  /// `(unexpected-token diagnostics, limit diagnostics)`.
  diagnostics: (usize, usize),
  /// The tokens a full drain then yields, with their spans.
  drained: std::vec::Vec<(SimpleSpan, BalKind)>,
  /// The balanced sync's returned hole, when the program produced one.
  hole: Option<(SimpleSpan, usize)>,
  /// The spans of each window the program peeked, in order.
  peeked: std::vec::Vec<std::vec::Vec<SimpleSpan>>,
}

/// What a program produced beyond the input's own facts.
#[derive(Default)]
struct Produced {
  hole: Option<(SimpleSpan, usize)>,
  peeked: std::vec::Vec<std::vec::Vec<SimpleSpan>>,
}

/// Runs `program`, returning whatever it produced.
fn run<'a, C>(
  inp: &mut InputRef<'a, '_, BalLexer<'a>, (Verbose<ByValErr>, C), ()>,
  program: Program,
) -> Produced
where
  C: Cache<'a, BalLexer<'a>, ()>,
{
  let mut produced = Produced::default();
  match program {
    Program::SyncToStop => {
      inp.next().unwrap().expect("the first number");
      let _ = inp
        .sync_to(|t| t.data().kind() == BalKind::Num, || None)
        .unwrap();
    }
    Program::SkipWhileStop => {
      inp.skip_while(|t| t.data().is_trivia()).unwrap();
    }
    Program::SyncBalancedStop | Program::SyncBalancedZeroSkip => {
      inp.next().unwrap().expect("the first number");
      produced.hole = inp
        .sync_balanced(parens, |t| t.data().kind() == BalKind::Semi)
        .unwrap()
        .map(|hole| (hole.span(), hole.skipped()));
    }
    Program::TryExpectDecline => {
      inp.next().unwrap().expect("the first number");
      assert!(
        inp
          .try_expect(|t| t.data().kind() == BalKind::Semi)
          .unwrap()
          .is_none(),
        "the number at the cursor is not a `;`"
      );
    }
    Program::TryExpectMapDecline => {
      inp.next().unwrap().expect("the first number");
      assert!(
        inp
          .try_expect_map(|t| (t.data().kind() == BalKind::Semi).then_some(()))
          .unwrap()
          .is_none()
      );
    }
    Program::TryExpectAndThenDecline => {
      inp.next().unwrap().expect("the first number");
      assert!(
        inp
          .try_expect_and_then(|t| (t.data().kind() == BalKind::Semi).then_some(Ok(())))
          .unwrap()
          .is_none()
      );
    }
    Program::TryExpectOrStopDecline => {
      inp.next().unwrap().expect("the first number");
      assert!(
        inp
          .try_expect_or_stop(|t| t.data().kind() == BalKind::Semi)
          .unwrap()
          .is_none()
      );
    }
    Program::TryExpectMapOrStopDecline => {
      inp.next().unwrap().expect("the first number");
      assert!(
        inp
          .try_expect_map_or_stop(|t| (t.data().kind() == BalKind::Semi).then_some(()))
          .unwrap()
          .is_none()
      );
    }
    Program::ProbeCloseWrongToken => {
      inp.next().unwrap().expect("the first number");
      let status = inp
        .probe_close(|t| t.data().kind() == BalKind::Semi)
        .unwrap();
      assert!(
        matches!(status, CloseStatus::WrongToken(_)),
        "the number at the cursor is not the closer"
      );
    }
    Program::PeekPastStopTrips | Program::PeekAfterLatchedBoundary => {
      inp.next().unwrap().expect("the first number");
      // The stop leaves `2` unconsumed at the front of the stream.
      let _ = inp
        .sync_to(|t| t.data().kind() == BalKind::Num, || None)
        .unwrap();
      // A three-deep peek: the front already holds one token, so the fill lexes `3` — which
      // takes the tally past the budget and trips. The window must still carry the token that
      // was waiting at the front; truncating it to nothing would hide a consumable token
      // behind a terminal stop.
      produced
        .peeked
        .push(peeked_spans::<U3>(&inp.peek::<U3>().unwrap()));
      if matches!(program, Program::PeekAfterLatchedBoundary) {
        // The boundary is latched now, so this peek takes the short-circuit that serves the
        // front without touching the lexer at all.
        produced
          .peeked
          .push(peeked_spans::<U1>(&inp.peek::<U1>().unwrap()));
      }
    }
  }
  produced
}

/// The spans of a peeked window, in order.
fn peeked_spans<'a, W>(peeked: &Peeked<'_, 'a, BalLexer<'a>, W>) -> std::vec::Vec<SimpleSpan>
where
  W: Window,
{
  peeked.iter().map(|t| *t.span()).collect()
}

/// Runs `program` under cache `C` and reports the observable tuple.
fn observe<'a, C>(program: Program) -> Observed
where
  C: Cache<'a, BalLexer<'a>, ()> + Default,
{
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<BalLexer<'a>, (Verbose<ByValErr>, C), ()>::with_state_and_cache(
    program.src(),
    TokenLimiter::with_limitation(program.limit()),
    C::default(),
  );
  let (cursor, offset, is_eoi, is_exhausted, span, tokens, drained, produced) = {
    let mut inp = input.as_ref(&mut emitter);
    let produced = run(&mut inp, program);
    let cursor = *inp.cursor().as_inner();
    let offset = *inp.offset();
    let is_eoi = inp.is_eoi();
    let is_exhausted = inp.is_exhausted();
    let span = *inp.span();
    let tokens = inp.state().tokens();
    let mut drained = std::vec::Vec::new();
    while let Some(tok) = inp.next().unwrap() {
      drained.push((*tok.span_ref(), tok.data().kind()));
    }
    (
      cursor,
      offset,
      is_eoi,
      is_exhausted,
      span,
      tokens,
      drained,
      produced,
    )
  };
  let lex = emitter
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
  Observed {
    cursor,
    offset,
    is_eoi,
    is_exhausted,
    span,
    tokens,
    diagnostics: (lex, limit),
    drained,
    hole: produced.hole,
    peeked: produced.peeked,
  }
}

/// Asserts that the three capacities observe the same thing, and returns that observation so a
/// caller can pin its absolute values.
fn agree(program: Program) -> Observed {
  let deque = observe::<DefaultCache<'_, BalLexer<'_>>>(program);
  let one = observe::<OneSlot<'_>>(program);
  let none = observe::<()>(program);
  assert_eq!(
    deque, one,
    "{program:?}: a one-slot cache observes something a three-slot cache does not"
  );
  assert_eq!(
    deque, none,
    "{program:?}: a cache that retains nothing observes something a three-slot cache does not"
  );
  deque
}

#[test]
fn sync_to_stop_is_capacity_independent() {
  // The audit's history, at its exact numbers: `"1   2"`, consume `1`, then sync to the next
  // number. The scan lexes `2` itself and stops before it, so the cursor is `2`'s start (4) and
  // the lex frontier is its end (5) — in every capacity, including the one that can retain
  // nothing.
  let observed = agree(Program::SyncToStop);
  assert_eq!(
    observed.cursor, 4,
    "the cursor is the stopping token's start"
  );
  assert_eq!(observed.offset, 5, "the lex frontier is its end");
  assert!(observed.is_eoi, "that frontier is the end of the buffer");
  assert_eq!(
    observed.drained,
    std::vec![(SimpleSpan { start: 4, end: 5 }, BalKind::Num)],
    "the stopping token is the next one consumed, at its own span"
  );
}

#[test]
fn skip_while_stop_is_capacity_independent() {
  // The trivia path takes the identical settle, so a `padded` combinator's resume cursor is a
  // fact about the stream in every capacity too.
  let observed = agree(Program::SkipWhileStop);
  assert_eq!(
    observed.cursor, 6,
    "the cursor is the stopping token's start"
  );
  assert_eq!(observed.offset, 7);
  assert!(observed.is_eoi);
}

#[test]
fn sync_balanced_stop_is_capacity_independent() {
  let observed = agree(Program::SyncBalancedStop);
  assert_eq!(
    observed.hole,
    Some((SimpleSpan { start: 2, end: 7 }, 3)),
    "the hole covers the three skipped tokens"
  );
  assert_eq!(observed.cursor, 10, "the cursor is the `;`'s start");
  assert_eq!(observed.offset, 11);
}

#[test]
fn sync_balanced_zero_skip_hole_anchors_at_the_match() {
  // A zero-skip hole is anchored at `cursor()` itself, so a stop that dropped its token
  // returns a *different value* — a return moving with the cache capacity, which no contract
  // states.
  let observed = agree(Program::SyncBalancedZeroSkip);
  assert_eq!(
    observed.hole,
    Some((SimpleSpan { start: 4, end: 4 }, 0)),
    "the zero-width hole sits at the matched token's start"
  );
  assert_eq!(observed.cursor, 4);
}

#[test]
fn a_peek_past_a_stop_serves_the_stopped_token() {
  // The fill must lex only PAST what is already at the front. Here the token past it trips the
  // budget, so the window is truncated at the durability boundary — but the token that was
  // waiting at the front is durable and must survive that truncation, in every capacity.
  let observed = agree(Program::PeekPastStopTrips);
  assert_eq!(
    observed.peeked,
    std::vec![std::vec![SimpleSpan { start: 4, end: 5 }]],
    "the stopped token heads the window even though the fill behind it tripped"
  );
  assert_eq!(observed.cursor, 4);
  assert_eq!(observed.offset, 5);
  assert_eq!(
    observed.drained,
    std::vec![(SimpleSpan { start: 4, end: 5 }, BalKind::Num)],
    "the budget stops the drain after the token that was already at the front"
  );
}

#[test]
fn a_peek_at_a_latched_boundary_still_serves_the_front() {
  // The short-circuit that refuses to rebuild a lexer past a latched boundary serves whatever
  // is at the front and stops. Returning an empty window over a live, consumable token would
  // let a dispatcher read a terminal stop as a genuine end of the construct.
  let observed = agree(Program::PeekAfterLatchedBoundary);
  assert_eq!(
    observed.peeked,
    std::vec![
      std::vec![SimpleSpan { start: 4, end: 5 }],
      std::vec![SimpleSpan { start: 4, end: 5 }]
    ],
    "the second peek takes the latched-boundary arm and still reports the front token"
  );
}

#[test]
fn try_expect_declines_are_capacity_independent() {
  // Every `try_expect`-family decline documents its `Ok(None)` as definite absence with the
  // token left unconsumed at the front. That is only true if the front can hold it.
  for program in [
    Program::TryExpectDecline,
    Program::TryExpectMapDecline,
    Program::TryExpectAndThenDecline,
    Program::TryExpectOrStopDecline,
    Program::TryExpectMapOrStopDecline,
  ] {
    let observed = agree(program);
    assert_eq!(
      observed.cursor, 4,
      "{program:?}: the declined token's start"
    );
    assert_eq!(observed.offset, 5, "{program:?}: its end");
    assert!(observed.is_eoi, "{program:?}");
    assert_eq!(
      observed.drained,
      std::vec![(SimpleSpan { start: 4, end: 5 }, BalKind::Num)],
      "{program:?}: the declined token is the next one consumed"
    );
  }
}

#[test]
fn probe_close_wrong_token_is_capacity_independent() {
  // `probe_close`'s scan-path `WrongToken` leaves the token in place for the downstream parse,
  // and must do so whatever the capacity.
  let observed = agree(Program::ProbeCloseWrongToken);
  assert_eq!(observed.cursor, 4);
  assert_eq!(observed.offset, 5);
  assert!(observed.is_eoi);
}

#[test]
fn every_program_is_compared() {
  // The list is the census: a scan stop or a decline added without a row here is a promise no
  // capacity comparison covers.
  assert_eq!(
    Program::ALL.len(),
    12,
    "a `to`-shaped stop or a decline joined the surface without joining this comparison"
  );
  for program in Program::ALL {
    let _ = agree(*program);
  }
}

// ── `is_exhausted`: the consumer gate, at every program point ────────────────────────
//
// The gate a driver loop wants, and the promise it makes: it is a function
// of the token stream and the consumed prefix alone, so it agrees across every capacity at
// *every* point — including after an arbitrary peek, where `cursor`/`offset`/`is_eoi` are all
// still legitimately capacity-dependent.

/// How deep the caller looked ahead before the trace was taken.
#[derive(Clone, Copy, Debug)]
enum Lookahead {
  None,
  One,
  Three,
}

/// `is_exhausted()` after `lookahead`, after `program`, and after each subsequent consume, up to
/// two consumes past the end of the stream.
fn exhaustion_trace<'a, C>(program: Program, lookahead: Lookahead) -> std::vec::Vec<bool>
where
  C: Cache<'a, BalLexer<'a>, ()> + Default,
{
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<BalLexer<'a>, (Verbose<ByValErr>, C), ()>::with_state_and_cache(
    program.src(),
    TokenLimiter::with_limitation(program.limit()),
    C::default(),
  );
  let mut inp = input.as_ref(&mut emitter);
  let _ = run(&mut inp, program);
  match lookahead {
    Lookahead::None => {}
    Lookahead::One => {
      let _ = inp.peek::<U1>().unwrap();
    }
    Lookahead::Three => {
      let _ = inp.peek::<U3>().unwrap();
    }
  }
  let mut trace = std::vec![inp.is_exhausted()];
  let mut empty = 0usize;
  while empty < 2 {
    if inp.next().unwrap().is_none() {
      empty += 1;
    }
    trace.push(inp.is_exhausted());
  }
  trace
}

#[test]
fn is_exhausted_is_capacity_independent() {
  for program in Program::ALL {
    for lookahead in [Lookahead::None, Lookahead::One, Lookahead::Three] {
      let deque = exhaustion_trace::<DefaultCache<'_, BalLexer<'_>>>(*program, lookahead);
      let one = exhaustion_trace::<OneSlot<'_>>(*program, lookahead);
      let none = exhaustion_trace::<()>(*program, lookahead);
      assert_eq!(
        deque, one,
        "{program:?} after {lookahead:?}: a one-slot cache answers the consumer gate differently"
      );
      assert_eq!(
        deque, none,
        "{program:?} after {lookahead:?}: a cache that retains nothing answers the consumer \
         gate differently"
      );
    }
  }
}

#[test]
fn is_exhausted_never_turns_true_early() {
  // The direction that matters for a loop gate: while a token is still consumable the gate must
  // read `false`, however deep the caller peeked. `is_eoi` fails this the moment a lookahead
  // lexes through the end of the buffer, which is why it is the wrong gate.
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input =
    Input::<BalLexer<'_>, (Verbose<ByValErr>, DefaultCache<'_, BalLexer<'_>>), ()>::with_state_and_cache(
      "1 2 3",
      TokenLimiter::new(),
      DefaultCache::<'_, BalLexer<'_>>::default(),
    );
  let mut inp = input.as_ref(&mut emitter);
  assert_eq!(inp.peek::<U3>().unwrap().len(), 3);
  assert!(
    inp.is_eoi(),
    "the lookahead lexed through the end, so the frontier predicate says yes"
  );
  for _ in 0..3 {
    assert!(
      !inp.is_exhausted(),
      "three tokens are still waiting in front of the caller"
    );
    assert!(inp.next().unwrap().is_some());
  }
  assert!(inp.is_exhausted(), "now the stream really is empty");
}

#[test]
fn is_exhausted_stays_false_after_a_bare_drain_with_a_trailing_skip() {
  // The residual the other direction, pinned so it is not mistaken for a defect: `false` does not
  // promise a token. A plain `next()` drain commits no further than the last token's end, so a
  // source with trailing lexer-skipped bytes leaves the gate `false` once the stream is empty —
  // in every capacity. A consume's own outcome is the authoritative end-of-stream signal; the
  // scans that settle at exhaustion do commit the lexer's end and do reach `true`.
  fn drained_gate<'a, C>(src: &'a str, skip_to_end: bool) -> bool
  where
    C: Cache<'a, BalLexer<'a>, ()> + Default,
  {
    let mut emitter = Verbose::<ByValErr>::new();
    let mut input = Input::<BalLexer<'a>, (Verbose<ByValErr>, C), ()>::with_state_and_cache(
      src,
      TokenLimiter::new(),
      C::default(),
    );
    let mut inp = input.as_ref(&mut emitter);
    while inp.next().unwrap().is_some() {}
    if skip_to_end {
      inp.skip_while(|_| true).unwrap();
    }
    inp.is_exhausted()
  }

  for cache in ["deque", "one-slot", "none"] {
    let bare = match cache {
      "deque" => drained_gate::<DefaultCache<'_, BalLexer<'_>>>("1 2  ", false),
      "one-slot" => drained_gate::<OneSlot<'_>>("1 2  ", false),
      _ => drained_gate::<()>("1 2  ", false),
    };
    assert!(
      !bare,
      "{cache}: a bare drain leaves the committed frontier at the last token's end, so the \
       gate stays false over the trailing skip"
    );
  }

  assert!(
    drained_gate::<DefaultCache<'_, BalLexer<'_>>>("1 2  ", true),
    "a scan that settles at exhaustion commits the lexer's end, and the gate reaches true"
  );
  assert!(drained_gate::<OneSlot<'_>>("1 2  ", true));
  assert!(drained_gate::<()>("1 2  ", true));
}

// ── The parked token across a rollback and a state surgery ───────────────────────────

/// The facts a restore comparison reads, before and after.
#[derive(Debug, PartialEq)]
struct Restored {
  saved_cursor: usize,
  saved_offset: usize,
  cursor: usize,
  offset: usize,
  span_end: usize,
  tokens: usize,
  live_checkpoints: usize,
  relexed: (SimpleSpan, BalKind),
}

fn stop_save_consume_restore<'a, C>() -> Restored
where
  C: Cache<'a, BalLexer<'a>, ()> + Default,
{
  let mut emitter = Verbose::<ByValErr>::new();
  let mut input = Input::<BalLexer<'a>, (Verbose<ByValErr>, C), ()>::with_state_and_cache(
    "1   2",
    TokenLimiter::new(),
    C::default(),
  );
  let mut inp = input.as_ref(&mut emitter);
  inp.next().unwrap().expect("the first number");
  let _ = inp
    .sync_to(|t| t.data().kind() == BalKind::Num, || None)
    .unwrap();

  let saved_cursor = *inp.cursor().as_inner();
  let saved_offset = *inp.offset();
  let live_before = inp.live_checkpoints_len();
  let ckp = inp.save();
  inp.next().unwrap().expect("the retained second number");
  inp.restore(ckp);
  assert_eq!(
    inp.live_checkpoints_len(),
    live_before,
    "the restore closes the checkpoint it spent"
  );

  let cursor = *inp.cursor().as_inner();
  let offset = *inp.offset();
  let span_end = *inp.span().end_ref();
  let tokens = inp.state().tokens();
  let relexed = inp.next().unwrap().expect("the second number, re-lexed");
  Restored {
    saved_cursor,
    saved_offset,
    cursor,
    offset,
    span_end,
    tokens,
    live_checkpoints: inp.live_checkpoints_len(),
    relexed: (*relexed.span_ref(), relexed.data().kind()),
  }
}

#[test]
fn restore_drops_the_parked_token() {
  // A restore does not memoize a *consumed* token back — not a cached one, and not a parked one.
  // So the post-restore facts are the checkpoint's COMMITTED facts and the token re-lexes on
  // demand: that is the lineage model, not a gap in it. What the parked slot must not do is make
  // the restore path capacity-dependent, so the twin below asserts identical observables.
  let deque = stop_save_consume_restore::<DefaultCache<'_, BalLexer<'_>>>();
  let none = stop_save_consume_restore::<()>();
  assert_eq!(
    deque, none,
    "the restore path is the same whether the stopped token was cached or parked"
  );
  assert_eq!(
    (deque.saved_cursor, deque.saved_offset),
    (4, 5),
    "at the save point the stopped token is retained at the front"
  );
  assert_eq!(
    (deque.cursor, deque.offset, deque.span_end),
    (1, 1, 1),
    "after the restore the facts are the checkpoint's committed facts — the consumed token \
     is deliberately not memoized back"
  );
  assert_eq!(deque.tokens, 1, "the tally returns to the restored state");
  assert_eq!(
    deque.relexed,
    (SimpleSpan { start: 4, end: 5 }, BalKind::Num),
    "the token re-lexes on demand, identical by the lexer determinism contract"
  );
}

#[test]
fn state_surgery_drops_the_parked_token() {
  // The parked token was lexed under the outgoing regime, exactly like a cache entry, so the
  // re-key that follows a state replacement drops it too.
  fn after_surgery<'a, C>() -> (usize, usize)
  where
    C: Cache<'a, BalLexer<'a>, ()> + Default,
  {
    let mut emitter = Verbose::<ByValErr>::new();
    let mut input = Input::<BalLexer<'a>, (Verbose<ByValErr>, C), ()>::with_state_and_cache(
      "1   2",
      TokenLimiter::new(),
      C::default(),
    );
    let mut inp = input.as_ref(&mut emitter);
    inp.next().unwrap().expect("the first number");
    let _ = inp
      .sync_to(|t| t.data().kind() == BalKind::Num, || None)
      .unwrap();
    assert_eq!(*inp.cursor().as_inner(), 4, "the stop retains the token");
    inp.set_state(TokenLimiter::new());
    (*inp.cursor().as_inner(), *inp.offset())
  }

  assert_eq!(
    after_surgery::<DefaultCache<'_, BalLexer<'_>>>(),
    (1, 1),
    "the retained token goes with the dead regime"
  );
  assert_eq!(after_surgery::<OneSlot<'_>>(), (1, 1));
  assert_eq!(after_surgery::<()>(), (1, 1));
}
