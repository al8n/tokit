//! The hot no-op reads on the input surface, and the promise that taking them changes nothing.
//!
//! Two entry points answer without reaching the general machinery when the answer is already
//! sitting at the front of the token stream:
//!
//! * [`skip_while`](InputRef::skip_while) — the head is there and the predicate rejects it, so
//!   the scan would skip nothing: no scan scope, no frontier clone, no take-test-put-back of the
//!   stopping token, no commit of a position that did not move;
//! * [`peek_head_map`](InputRef::peek_head_map), and through it
//!   [`peek_kind`](InputRef::peek_kind) and [`head_satisfies`](InputRef::head_satisfies) — the
//!   head is there, so the window fill's slot arithmetic, its boundary probe, its overflow guard
//!   and its staging copy have nothing to do.
//!
//! A fast path is only worth anything if it is **indistinguishable**, so these tests never assert
//! that one was taken. They assert that it cannot be told apart from the path it bypasses, and
//! they do it by running the same program with the condition true and with it false:
//!
//! * **the condition** — a prefill puts the head in the cache (or, under a cache that retains
//!   nothing, a stop parks it), so the probe finds it;
//! * **its negation** — no prefill, so the cache is empty and the probe finds nothing; the
//!   general path runs and must land on the same observation.
//!
//! Every program runs at four prefill depths and under three cache capacities, and the whole
//! observable tuple — including **which tokens the predicate was asked about, in order** — must
//! agree across all twelve. That last column is the one a naive `skip_while` fast path fails:
//! probing the head and then handing the same token to the scanner asks a stateful `FnMut` about
//! it twice, and a predicate that answers differently the second time would skip a token the
//! general path keeps or keep one it skips.
//!
//! The head read has its own pair of edges, and they pull in opposite directions: a resident head
//! must be served **even at a latched poison boundary** (the fill's own `want == 0` arm returns
//! before it probes the latch), and a head that is *not* resident must still raise the terminal
//! end-of-input error rather than the `Ok(None)` that means a genuine end of input. Both are
//! pinned below.
//!
//! # And what the observable tuple cannot see: the caller code each route runs
//!
//! Agreeing on every value a caller can read back is **not** the same as running the same code.
//! In a generic library the general routes also invoke caller-supplied `L::Span::clone`,
//! `L::State::clone`, `L::Offset::clone`, `Emitter::checkpoint`/`release` and `Cache` methods on
//! their way to the same answer, and a fast path that reaches the answer sooner runs fewer of
//! them. The second half of this file measures exactly that — an ordered **effect ledger** over an
//! instrumented lexer, emitter and cache — and pins both columns, because a claim about a
//! difference is only worth what its other side is worth.
//!
//! Measuring it is half the job. The other half is the **condition** under which running less
//! caller code is nevertheless invisible, and the last four cells build the callers who break it:
//! two whose predicate or closure reads the input layer, and two whose input-layer `Clone`
//! unwinds. The condition is written on the two methods as a property of the caller — *inert*
//! input-layer callbacks, and a predicate that is a *function of what it is handed* — rather than
//! as a list of the differences it excludes, so that satisfying it is something a caller checks
//! once about its own types instead of a list it has to keep up with.
//!
//! Neither comparison needs an ablation. `skip_while(|_| false)` over a resident head is precisely
//! `skip_until::<SkipWhile>` with a predicate that stops on the first token, and `peek_head_map`
//! over a resident head is precisely `peek::<U1>`'s `want == 0` arm — so both routes are reachable
//! from shipped code, over the same stream, in the same residency, with nothing varying but the
//! entry point.

use generic_arraydeque::typenum::{U1, U3};

use crate::{
  InputRef, Token,
  cache::{Cache, CachedTokenOf, DefaultCache},
  emitter::Verbose,
  input::Input,
  span::SimpleSpan,
  state::token_tracker::TokenLimiter,
};

use super::{
  scan::SkipWhile,
  tests::{BalKind, BalLexer, ByValErr},
};

/// A one-slot cache: the smallest capacity that still retains a token, so the head the probe
/// finds is a cache entry.
type OneSlot<'a> = Option<CachedTokenOf<'a, BalLexer<'a>>>;

type BalCtx<'a> = (Verbose<ByValErr>, DefaultCache<'a, BalLexer<'a>>);

/// What one run observed. `offset()`, [`is_eoi`](InputRef::is_eoi), the cache depth and the
/// lexer's token tally are deliberately absent: those are **frontier** facts, and a prefill
/// legitimately lexes ahead of the stream while a cache that retains nothing legitimately
/// re-lexes what the prefill threw away. Everything here is a function of the token stream alone
/// — [`is_exhausted`](InputRef::is_exhausted) included, which is the consumer-side question and
/// is documented as independent of the cache implementation.
#[derive(Debug, PartialEq)]
struct Observed {
  /// The tokens the skip predicate was asked about, in order — the double-ask guard.
  asked: std::vec::Vec<(SimpleSpan, BalKind)>,
  /// What the program's head reads returned, in order.
  read: std::vec::Vec<Option<BalKind>>,
  /// The resume cursor after the program: the stopping token's start.
  cursor: usize,
  /// The committed span after the program.
  span: SimpleSpan,
  is_exhausted: bool,
  /// `(lexer-class diagnostics, limit diagnostics)`.
  diagnostics: (usize, usize),
  /// The tokens a full drain then yields — the "a caller retries after the call" law.
  drained: std::vec::Vec<(SimpleSpan, BalKind)>,
}

/// The programs, each a shape the fast path actually meets in a grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Program {
  /// One trivia skip. The probe rejects the head outright when the source opens on a
  /// non-trivia token, and accepts it when the source opens on trivia.
  Skip,
  /// Two skips back to back. The second is the guaranteed no-op: whatever the first stopped on
  /// is at the front of the stream, and the predicate rejects it. This is the shipping shape —
  /// a decision-point atom skips, decides, and the atom that consumes re-skips a run the first
  /// already crossed.
  SkipTwice,
  /// Consume, then skip, then consume: the skip meets a stream the caller has been advancing,
  /// so the head it probes is one a consume left behind rather than one a prefill placed.
  ConsumeSkipConsume,
  /// A width-1 head read on its own.
  PeekKind,
  /// Skip, read the head, skip, read again: the grammar's standard decision-point pair, with
  /// both fast paths on the same stream.
  SkipPeekSkipPeek,
  /// Read the head and consume it, to the end: the last read finds nothing resident and nothing
  /// left to lex, which is the probe's `None` arm and the fill's genuine end of input.
  DrainByPeek,
}

impl Program {
  const ALL: &'static [Program] = &[
    Program::Skip,
    Program::SkipTwice,
    Program::ConsumeSkipConsume,
    Program::PeekKind,
    Program::SkipPeekSkipPeek,
    Program::DrainByPeek,
  ];
}

/// The sources, chosen so the probe's answer differs between them: one opens on a stopper, one
/// on a trivia run, one is nothing but trivia, and one is empty.
///
/// `~` is a real trivia **token**, not lexer-skipped whitespace, so the end of one token and the
/// start of the next are different offsets and a cursor placed at the wrong one is visible.
const SOURCES: &[&str] = &["2 ;", "~ ~ 2 ;", "~ ~ ~", "", "~", "2", "2 ~ ~ ;"];

/// The prefill depths. `0` is the negation — nothing is resident, so the probe finds no head and
/// the general path runs.
const PREFILLS: &[usize] = &[0, 1, 2, 3];

/// Runs `program` under one cache capacity and one prefill depth.
fn observe<'a, C>(program: Program, src: &'a str, prefill: usize) -> Observed
where
  C: Cache<'a, BalLexer<'a>, ()> + Default,
{
  let mut input = Input::<BalLexer<'a>, (Verbose<ByValErr>, C), ()>::with_state_and_context(
    src,
    TokenLimiter::with_limitation(usize::MAX),
    crate::input::InputContext::new(Verbose::<ByValErr>::new(), C::default()),
  );

  let mut asked = std::vec::Vec::new();
  let mut read = std::vec::Vec::new();

  let (cursor, span, is_exhausted, drained) = {
    let mut inp = input.as_ref();
    // The prefill: whatever the capacity retains of it is what the probe will find.
    for _ in 0..prefill {
      let _ = inp.peek::<U3>().unwrap();
    }
    run(&mut inp, program, &mut asked, &mut read);
    let cursor = *inp.cursor().as_inner();
    let span = *inp.span();
    let is_exhausted = inp.is_exhausted();
    let mut drained = std::vec::Vec::new();
    while let Some(tok) = inp.next().unwrap() {
      drained.push((*tok.span_ref(), tok.data().kind()));
    }
    (cursor, span, is_exhausted, drained)
  };

  let lex = input
    .emitter()
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Lex)
    .count();
  let limit = input
    .emitter()
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == ByValErr::Limit)
    .count();

  Observed {
    asked,
    read,
    cursor,
    span,
    is_exhausted,
    diagnostics: (lex, limit),
    drained,
  }
}

/// The program bodies. `asked` records every token the skip predicate saw, so the comparison
/// covers the one thing a stateful `FnMut` could otherwise use to tell the two paths apart.
fn run<'a, C>(
  inp: &mut InputRef<'a, '_, BalLexer<'a>, (Verbose<ByValErr>, C), ()>,
  program: Program,
  asked: &mut std::vec::Vec<(SimpleSpan, BalKind)>,
  read: &mut std::vec::Vec<Option<BalKind>>,
) where
  C: Cache<'a, BalLexer<'a>, ()>,
{
  match program {
    Program::Skip => skip(inp, asked),
    Program::SkipTwice => {
      skip(inp, asked);
      skip(inp, asked);
    }
    Program::ConsumeSkipConsume => {
      let _ = inp.next().unwrap();
      skip(inp, asked);
      let _ = inp.next().unwrap();
    }
    Program::PeekKind => read.push(inp.peek_kind().unwrap()),
    Program::SkipPeekSkipPeek => {
      skip(inp, asked);
      read.push(inp.peek_kind().unwrap());
      skip(inp, asked);
      read.push(inp.peek_kind().unwrap());
    }
    Program::DrainByPeek => loop {
      let head = inp.peek_kind().unwrap();
      read.push(head);
      if head.is_none() {
        break;
      }
      inp.next().unwrap().expect("the head was just read");
    },
  }
}

/// One `skip_while(is_trivia)`, recording every token the predicate was asked about.
fn skip<'a, C>(
  inp: &mut InputRef<'a, '_, BalLexer<'a>, (Verbose<ByValErr>, C), ()>,
  asked: &mut std::vec::Vec<(SimpleSpan, BalKind)>,
) where
  C: Cache<'a, BalLexer<'a>, ()>,
{
  inp
    .skip_while(|t| {
      asked.push((*t.span, t.data.kind()));
      t.data.is_trivia()
    })
    .unwrap();
}

/// The condition and its negation, over every program, every source and three capacities.
///
/// The prefill sweep is what varies the fast path's condition: at depth 0 the cache is empty and
/// the probe finds nothing, so the general path runs; at every other depth the head is resident
/// (under a cache with room) and the probe answers. The capacity sweep adds the third residency —
/// under `()` nothing is ever cached, so the only head a probe can find is one a stop **parked**,
/// which is the put-back origin the fast path has to be an identity on too.
#[test]
fn fast_paths_are_invisible_to_residency() {
  for &program in Program::ALL {
    for &src in SOURCES {
      let reference = observe::<DefaultCache<'_, BalLexer<'_>>>(program, src, 0);
      for &prefill in PREFILLS {
        let deque = observe::<DefaultCache<'_, BalLexer<'_>>>(program, src, prefill);
        let one = observe::<OneSlot<'_>>(program, src, prefill);
        let none = observe::<()>(program, src, prefill);
        for (capacity, got) in [
          ("deque(3)", deque),
          ("option(1)", one),
          ("blackhole(0)", none),
        ] {
          assert_eq!(
            got, reference,
            "{program:?} over {src:?} observed differently at prefill {prefill} under \
             {capacity} than it does with an empty cache — a fast path that fires only when the \
             head happens to be resident has changed what the caller sees"
          );
        }
      }
    }
  }
}

/// The differential test above proves agreement; this pins what it agrees *on*, so a bug that
/// broke both routes in the same way could not pass unnoticed.
#[test]
fn the_agreed_skip_observation_is_the_right_one() {
  // The source opens on a stopper, so the skip skips nothing: the predicate is asked once, the
  // committed span has not moved off the origin, and the token is still there to be drained.
  let none_to_skip = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::Skip, "2 ;", 1);
  assert_eq!(
    none_to_skip.asked,
    std::vec![(SimpleSpan::new(0, 1), BalKind::Num)],
    "a no-op skip asks about the head exactly once"
  );
  assert_eq!(
    none_to_skip.cursor, 0,
    "the stopping token is still the head"
  );
  assert_eq!(
    none_to_skip.span,
    SimpleSpan::new(0, 0),
    "nothing committed"
  );
  assert_eq!(
    none_to_skip.drained,
    std::vec![
      (SimpleSpan::new(0, 1), BalKind::Num),
      (SimpleSpan::new(2, 3), BalKind::Semi),
    ],
    "a skip that skips nothing consumes nothing"
  );

  // A trivia run: the probe accepts the head, the scan takes over, and each token is still asked
  // about exactly once — the head included, and only once.
  let with_trivia = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::Skip, "~ ~ 2 ;", 3);
  assert_eq!(
    with_trivia.asked,
    std::vec![
      (SimpleSpan::new(0, 1), BalKind::Trivia),
      (SimpleSpan::new(2, 3), BalKind::Trivia),
      (SimpleSpan::new(4, 5), BalKind::Num),
    ],
    "the probe's answer is carried into the scan, not asked again"
  );
  assert_eq!(with_trivia.cursor, 4, "the cursor is the stopper's start");
  assert_eq!(
    with_trivia.span,
    SimpleSpan::new(2, 3),
    "the committed span ends at the last skipped token"
  );

  // Back-to-back skips: the second is the pure no-op, and it must not re-ask about the run the
  // first already crossed.
  let twice = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::SkipTwice, "~ ~ 2 ;", 3);
  assert_eq!(
    twice.asked,
    std::vec![
      (SimpleSpan::new(0, 1), BalKind::Trivia),
      (SimpleSpan::new(2, 3), BalKind::Trivia),
      (SimpleSpan::new(4, 5), BalKind::Num),
      (SimpleSpan::new(4, 5), BalKind::Num),
    ],
    "the second skip asks only about the head it stops on"
  );
  assert_eq!(
    twice.span, with_trivia.span,
    "the second skip commits nothing"
  );

  // Nothing but trivia: the scan runs to end of input and commits there, so the probe's accept
  // arm and the scan's own end-of-input settle both get exercised.
  let all_trivia = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::Skip, "~ ~ ~", 3);
  assert_eq!(all_trivia.drained, std::vec![], "everything was skipped");
  assert!(all_trivia.is_exhausted, "the skip ran to end of input");
  assert_eq!(
    all_trivia.span,
    SimpleSpan::new(5, 5),
    "the end-of-input settle commits at the lexer's own end, not at the last token's"
  );

  // Empty input: the probe finds no head at all, so the general path runs and settles at the
  // lexer's end.
  let empty = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::Skip, "", 3);
  assert_eq!(empty.asked, std::vec![], "there was nothing to ask about");
  assert!(empty.is_exhausted);

  // The head read: resident or not, the same kind comes back and nothing is consumed.
  let peeked = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::PeekKind, "2 ;", 2);
  assert_eq!(peeked.read, std::vec![Some(BalKind::Num)]);
  assert_eq!(peeked.cursor, 0, "a peek commits nothing");
  assert_eq!(
    peeked.span,
    SimpleSpan::new(0, 0),
    "and moves no committed position"
  );
  assert_eq!(
    peeked.drained,
    std::vec![
      (SimpleSpan::new(0, 1), BalKind::Num),
      (SimpleSpan::new(2, 3), BalKind::Semi),
    ],
    "the token a peek read is still the next one consumed"
  );

  // Genuine end of input is `Ok(None)` on both routes, never the terminal error.
  let empty_head = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::PeekKind, "", 2);
  assert_eq!(empty_head.read, std::vec![None]);

  // The decision-point pair: the skip crosses the trivia run and the read sees the token behind
  // it; the second pair is the pure no-op both fast paths were built for.
  let pair = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::SkipPeekSkipPeek, "~ ~ 2 ;", 3);
  assert_eq!(
    pair.read,
    std::vec![Some(BalKind::Num), Some(BalKind::Num)],
    "both reads see the token the skip stopped on"
  );
  assert_eq!(
    pair.asked,
    std::vec![
      (SimpleSpan::new(0, 1), BalKind::Trivia),
      (SimpleSpan::new(2, 3), BalKind::Trivia),
      (SimpleSpan::new(4, 5), BalKind::Num),
      (SimpleSpan::new(4, 5), BalKind::Num),
    ],
    "a head read in between does not make the second skip re-ask about the trivia run"
  );

  // The read drives the whole stream, so its last call is the genuine end of input.
  let drained = observe::<DefaultCache<'_, BalLexer<'_>>>(Program::DrainByPeek, "~ ~ 2 ;", 1);
  assert_eq!(
    drained.read,
    std::vec![
      Some(BalKind::Trivia),
      Some(BalKind::Trivia),
      Some(BalKind::Num),
      Some(BalKind::Semi),
      None,
    ],
    "every token is read once, then end of input"
  );
}

// ── The boundary latch: the probe reads a head that is there, and only a head that is there ──
//
// A resource-limit trip latches a poison boundary. Once the cursor reaches it the scanner refuses
// to rebuild a lexer, so a skip with an empty front commits its prefix and stops — while a skip
// whose stopper is already resident stops on that token exactly as it would have with no latch at
// all. Both are the same on either route.

/// Numbers under a token budget: the lex past the budget trips and latches.
fn tripping_input(src: &str, limit: usize) -> Input<'_, BalLexer<'_>, BalCtx<'_>, ()> {
  Input::with_state_and_context(
    src,
    TokenLimiter::with_limitation(limit),
    crate::input::InputContext::new(
      Verbose::<ByValErr>::new(),
      DefaultCache::<'_, BalLexer<'_>>::default(),
    ),
  )
}

#[test]
fn skip_while_over_a_latched_boundary_agrees_warm_and_cold() {
  // Warm: a wide peek reaches past the budget and latches the boundary, but `2` is retained, so
  // the probe finds the stopper.
  let mut warm = tripping_input("1 2 3", 2);
  let mut inp = warm.as_ref();
  assert!(inp.next().unwrap().is_some(), "the first number");
  let _ = inp.peek::<U3>();
  inp.skip_while(|t| t.data.is_trivia()).unwrap();
  let warm_seen = (*inp.cursor().as_inner(), *inp.span());
  let warm_next = inp.next().unwrap().map(|t| *t.span_ref());

  // Cold: the same stream with nothing prefetched, so the scan lexes the stopper itself.
  let mut cold = tripping_input("1 2 3", 2);
  let mut inp = cold.as_ref();
  assert!(inp.next().unwrap().is_some(), "the first number");
  inp.skip_while(|t| t.data.is_trivia()).unwrap();
  let cold_seen = (*inp.cursor().as_inner(), *inp.span());
  let cold_next = inp.next().unwrap().map(|t| *t.span_ref());

  assert_eq!(
    warm_seen, cold_seen,
    "a skip that stops on `2` leaves the same cursor and the same committed span whether the \
     token was prefetched or lexed on the spot"
  );
  assert_eq!(
    (warm_next, cold_next),
    (Some(SimpleSpan::new(2, 3)), Some(SimpleSpan::new(2, 3))),
    "and the stopper is the next token consumed either way"
  );
}

#[test]
fn skip_while_with_a_latched_boundary_and_nothing_resident_takes_the_general_path() {
  // Drain the budget, so the front is empty and the next lex trips.
  let mut input = tripping_input("1 2 3", 2);
  let mut inp = input.as_ref();
  assert!(inp.next().unwrap().is_some(), "the first number");
  assert!(inp.next().unwrap().is_some(), "the second number");
  let before = (*inp.cursor().as_inner(), *inp.span());

  // The probe finds no head, so the scanner runs: it trips, latches, and commits its (empty)
  // prefix. Nothing moves, and the call still reports success.
  inp.skip_while(|t| t.data.is_trivia()).unwrap();
  assert_eq!(
    (*inp.cursor().as_inner(), *inp.span()),
    before,
    "a skip that finds nothing to skip and cannot lex commits nothing"
  );
  assert!(
    inp.next().unwrap().is_none(),
    "the latched boundary keeps the drain at the trip"
  );
}

#[test]
fn peek_head_map_serves_a_resident_head_at_a_latched_boundary() {
  // A wide peek reaches past the budget and latches the boundary — but `2` is retained, so the
  // head read has something at the front of the stream and must answer with it. This is the
  // condition under which the probe fires, and it is the one where firing looks riskiest: the
  // shared route reaches the same answer only because its `want == 0` arm returns before the
  // boundary is ever consulted.
  let mut input = tripping_input("1 2 3", 2);
  let mut inp = input.as_ref();
  assert!(inp.next().unwrap().is_some(), "the first number");
  let _ = inp.peek::<U3>();

  assert_eq!(
    inp.peek_kind().unwrap(),
    Some(BalKind::Num),
    "a token at the front of the stream is readable whatever the latch says"
  );
  assert!(
    inp.head_satisfies(|t| t.kind() == BalKind::Num).unwrap(),
    "and the predicate form agrees"
  );
  let tok = inp.next().unwrap().expect("the resident head");
  assert_eq!(
    *tok.span_ref(),
    SimpleSpan::new(2, 3),
    "a read consumes nothing, latched or not"
  );
}

#[test]
fn peek_head_map_raises_on_a_latched_boundary_with_nothing_resident() {
  // The negation: the front is empty and the boundary is latched, so the probe finds no head and
  // the general route runs — where a short window from a terminal stop is an ERROR, not the
  // `Ok(None)` that means a genuine end of input. A probe that answered `Ok(None)` here would let
  // a production read a halted scanner as "the construct ended".
  let mut input = tripping_input("1 2 3", 2);
  let mut inp = input.as_ref();
  assert!(inp.next().unwrap().is_some(), "the first number");
  assert!(inp.next().unwrap().is_some(), "the second number");
  // The fill past the budget trips, latches, and reports the stop.
  assert_eq!(
    inp.peek_kind(),
    Err(ByValErr::Lex),
    "a terminal stop surfaces the committed end-of-input error"
  );
  // And it keeps doing so: the latch is sticky and the front stays empty.
  assert_eq!(inp.peek_kind(), Err(ByValErr::Lex));
  assert!(
    inp.next().unwrap().is_none(),
    "the drain stops at the latched boundary"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// The caller-code effect ledger
// ═══════════════════════════════════════════════════════════════════════════════════════════════
//
// The tests above pin what a caller can *observe about the input* — the tokens, the cursor, the
// committed span, the diagnostics. That is not the whole of what a fast path can change. In a
// generic library the general route also RUNS caller-supplied code on its way to the same answer:
// `L::Span::clone`, `L::State::clone`, `L::Offset::clone`, `Emitter::checkpoint`/`release`, and
// every `Cache` method. Those calls are effects in their own right — they can count, they can log,
// and they can panic.
//
// So this section instruments all of them and asks the direct question: **for the calls the fast
// paths answer, what caller code does each route run, and in what order?** The fixture below is a
// lexer, an emitter and a cache that record every such step into one ordered ledger, and the tests
// pin the ledger rather than describing it.
//
// The measured answer, and the contract it produces, is written on `skip_while` and on
// `peek_head_map`. In summary: for a no-op skip the fast path runs **no** caller clone and takes
// **no** emitter mark, where the general route runs one `L::Span::clone` + one `L::State::clone`
// under `Complete` and two of each plus a complete `checkpoint`/`release` pair under `Partial`.
// Every *value* those steps produce is the identity — which is why nothing above can see the
// difference — but the calls themselves are not made.

use core::cell::{Cell, RefCell};

use crate::{
  error::{Incomplete, MaybeIncomplete, UnexpectedEot, token::UnexpectedToken},
  input::{InputContext, Partial},
  span::Spanned,
  state::State,
};

/// One step of **caller-supplied** code the input layer ran, in the order it ran it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effect {
  /// `L::Span::clone`. Recorded as one entry: the offsets it copies are moved rather than cloned
  /// so a span clone reads as a single caller step and not as a variable number of them.
  SpanClone,
  /// `L::State::clone`.
  StateClone,
  /// `L::Offset::clone`.
  OffsetClone,
  /// `Emitter::checkpoint` — a mark taken.
  Checkpoint,
  /// `Emitter::release` — a mark settled as kept.
  Release,
  /// `Emitter::rewind` — a mark settled as unwound.
  Rewind,
  /// `Emitter::commit_token` — the committed-token side channel.
  CommitToken,
  CacheFront,
  CacheLen,
  CachePeek,
  CachePopFront,
  CachePushFront,
  CachePushBack,
  /// The caller's own closure: the `skip_while` predicate, or `peek_head_map`'s `f`.
  Caller,
}

thread_local! {
  static LEDGER: RefCell<std::vec::Vec<Effect>> = const { RefCell::new(std::vec::Vec::new()) };
  /// Only the call under test is recorded; the prefill that sets up its condition is not.
  static RECORDING: Cell<bool> = const { Cell::new(false) };
  /// While set, the next `L::Span::clone` panics instead of cloning.
  static SPAN_CLONE_BOMB: Cell<bool> = const { Cell::new(false) };
  static STATE_CLONE_BOMB: Cell<bool> = const { Cell::new(false) };
  static CHECKPOINT_BOMB: Cell<bool> = const { Cell::new(false) };
  /// While set to `Some(n)`, an `L::Offset::clone` of exactly the offset `n` panics — see
  /// [`arm_offset_clone_bomb_at`] for why this one is keyed by value where the others are not.
  static OFFSET_CLONE_BOMB_AT: Cell<Option<usize>> = const { Cell::new(None) };
  /// Set by a bomb immediately before it panics — the payload-executed witness that tells a cell
  /// which never reached the armed step apart from one whose armed step did not fire.
  static BOMB_FIRED: Cell<bool> = const { Cell::new(false) };
  /// The caller's own tally of the caller's own calls. Written by the predicate or closure in the
  /// cells at the foot of this file and read only once the call has returned or unwound — which
  /// is the shape the contract's precondition explicitly permits, and the reason those cells bite.
  static CALLER_CALLS: Cell<usize> = const { Cell::new(0) };
}

fn note(effect: Effect) {
  if RECORDING.with(Cell::get) {
    LEDGER.with(|l| l.borrow_mut().push(effect));
  }
}

/// Starts a fresh recording.
fn record() {
  LEDGER.with(|l| l.borrow_mut().clear());
  RECORDING.with(|c| c.set(true));
}

/// Ends the recording and hands back the ledger.
fn recorded() -> std::vec::Vec<Effect> {
  RECORDING.with(|c| c.set(false));
  LEDGER.with(|l| l.borrow().clone())
}

/// The ledger with the `trace` feature's own bookkeeping removed.
///
/// `peek_head_map` emits one `trace` leaf event per call on **both** routes, and building its
/// source preview clones the cursor offset. That clone is a property of the trace hook, not of the
/// route, so a build with the feature on would otherwise read one offset clone richer than the
/// same code with it off.
fn without_trace_hook(ledger: std::vec::Vec<Effect>) -> std::vec::Vec<Effect> {
  #[cfg(feature = "trace")]
  {
    let mut ledger = ledger;
    let at = ledger
      .iter()
      .position(|e| *e == Effect::OffsetClone)
      .expect("the `trace` hook's source preview clones the cursor offset, once per peek event");
    ledger.remove(at);
    ledger
  }
  #[cfg(not(feature = "trace"))]
  ledger
}

fn arm_bombs() {
  BOMB_FIRED.with(|c| c.set(false));
  SPAN_CLONE_BOMB.with(|c| c.set(true));
  STATE_CLONE_BOMB.with(|c| c.set(true));
  CHECKPOINT_BOMB.with(|c| c.set(true));
}

fn disarm_bombs() {
  SPAN_CLONE_BOMB.with(|c| c.set(false));
  STATE_CLONE_BOMB.with(|c| c.set(false));
  CHECKPOINT_BOMB.with(|c| c.set(false));
}

fn bomb_fired() -> bool {
  BOMB_FIRED.with(Cell::get)
}

fn fire(bomb: &'static std::thread::LocalKey<Cell<bool>>) -> bool {
  if bomb.with(Cell::get) {
    BOMB_FIRED.with(|c| c.set(true));
    return true;
  }
  false
}

/// Arms a panicking `L::Offset::clone` for **one particular offset value**.
///
/// The other three bombs are armed wholesale, because the step each guards is taken on exactly one
/// of the two routes. This one cannot be: with the `trace` feature on, the head read's resident
/// route clones the *frontier* offset to build the hook's source preview, so a bomb that fired on
/// every offset clone would unwind both routes before the caller's closure and measure nothing.
/// Keying it to the **committed span's end** — the offset the general route hoists above the fill
/// for its terminal error, and the only one the cell below is about — isolates that one step in a
/// `trace` build and a non-`trace` build alike. A `Clone` that unwinds for some values and not
/// others is ordinary Rust; nothing here needs it to be more contrived than that.
fn arm_offset_clone_bomb_at(offset: usize) {
  BOMB_FIRED.with(|c| c.set(false));
  OFFSET_CLONE_BOMB_AT.with(|c| c.set(Some(offset)));
}

fn disarm_offset_clone_bomb() {
  OFFSET_CLONE_BOMB_AT.with(|c| c.set(None));
}

fn fire_offset_bomb(offset: usize) -> bool {
  if OFFSET_CLONE_BOMB_AT.with(Cell::get) == Some(offset) {
    BOMB_FIRED.with(|c| c.set(true));
    return true;
  }
  false
}

/// One call of the caller's own predicate or closure, recorded twice: into the ledger, so the
/// cells can see *where* in the route it fell, and into the caller's own counter, so they can see
/// how many times it ran when the route did not finish.
fn caller_call() {
  note(Effect::Caller);
  CALLER_CALLS.with(|c| c.set(c.get() + 1));
}

fn reset_caller_calls() {
  CALLER_CALLS.with(|c| c.set(0));
}

fn caller_calls() -> usize {
  CALLER_CALLS.with(Cell::get)
}

// ── The instrumented lexer ────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct LedgerOffset(usize);

impl Clone for LedgerOffset {
  fn clone(&self) -> Self {
    note(Effect::OffsetClone);
    assert!(
      !fire_offset_bomb(self.0),
      "the armed `L::Offset::clone` panics — a caller clone was reached"
    );
    Self(self.0)
  }
}

#[derive(Debug)]
struct LedgerSrc<'a>(&'a str);

impl crate::Source<LedgerOffset> for LedgerSrc<'_> {
  type Slice<'source>
    = &'source str
  where
    Self: 'source;

  fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  fn len(&self) -> LedgerOffset {
    LedgerOffset(self.0.len())
  }

  fn as_slice(&self) -> Self::Slice<'_> {
    self.0
  }

  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: core::ops::RangeBounds<LedgerOffset>,
  {
    self.0.get((
      range.start_bound().map(|s| s.0),
      range.end_bound().map(|s| s.0),
    ))
  }

  fn find_boundary(&self, index: LedgerOffset) -> LedgerOffset {
    if index.0 >= self.0.len() {
      return index;
    }
    let mut i = index.0;
    while !self.0.is_char_boundary(i) {
      i -= 1;
    }
    LedgerOffset(i)
  }

  fn is_boundary(&self, index: LedgerOffset) -> bool {
    self.0.is_char_boundary(index.0)
  }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct LedgerSpan {
  start: LedgerOffset,
  end: LedgerOffset,
}

impl Clone for LedgerSpan {
  fn clone(&self) -> Self {
    note(Effect::SpanClone);
    assert!(
      !fire(&SPAN_CLONE_BOMB),
      "the armed `L::Span::clone` panics — a caller clone was reached"
    );
    Self {
      start: LedgerOffset(self.start.0),
      end: LedgerOffset(self.end.0),
    }
  }
}

impl crate::Span for LedgerSpan {
  type Offset = LedgerOffset;

  fn new(start: LedgerOffset, end: LedgerOffset) -> Self {
    Self { start, end }
  }

  fn into_range(self) -> core::ops::Range<LedgerOffset> {
    self.start..self.end
  }

  fn start_ref(&self) -> &LedgerOffset {
    &self.start
  }

  fn start_mut(&mut self) -> &mut LedgerOffset {
    &mut self.start
  }

  fn into_start(self) -> LedgerOffset {
    self.start
  }

  fn end_ref(&self) -> &LedgerOffset {
    &self.end
  }

  fn end_mut(&mut self) -> &mut LedgerOffset {
    &mut self.end
  }

  fn into_end(self) -> LedgerOffset {
    self.end
  }

  fn bump(&mut self, n: &LedgerOffset) {
    self.end.0 += n.0;
  }
}

#[derive(Debug, Default, PartialEq)]
struct LedgerState {
  scanned: usize,
}

impl Clone for LedgerState {
  fn clone(&self) -> Self {
    note(Effect::StateClone);
    assert!(
      !fire(&STATE_CLONE_BOMB),
      "the armed `L::State::clone` panics — a caller clone was reached"
    );
    Self {
      scanned: self.scanned,
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
struct LedgerNeverTrips;

impl State for LedgerState {
  type Error = LedgerNeverTrips;

  fn check(&self) -> Result<(), LedgerNeverTrips> {
    Ok(())
  }
}

#[derive(Debug, Clone, PartialEq)]
enum LedgerErr {
  Lex,
  Incomplete,
}

impl From<()> for LedgerErr {
  fn from(_: ()) -> Self {
    LedgerErr::Lex
  }
}

impl From<LedgerNeverTrips> for LedgerErr {
  fn from(_: LedgerNeverTrips) -> Self {
    LedgerErr::Lex
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for LedgerErr
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    LedgerErr::Lex
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for LedgerErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    LedgerErr::Lex
  }
}

impl From<Incomplete<LedgerOffset>> for LedgerErr {
  fn from(_: Incomplete<LedgerOffset>) -> Self {
    LedgerErr::Incomplete
  }
}

impl MaybeIncomplete for LedgerErr {
  fn is_incomplete(&self) -> bool {
    matches!(self, LedgerErr::Incomplete)
  }
}

#[derive(Debug, Clone, PartialEq)]
enum LedgerTok {
  Word,
  Trivia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LedgerKind {
  Word,
  Trivia,
}

impl core::fmt::Display for LedgerKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Word => "word",
      Self::Trivia => "trivia",
    })
  }
}

impl Token<'_> for LedgerTok {
  type Kind = LedgerKind;
  type Error = LedgerErr;

  fn kind(&self) -> LedgerKind {
    match self {
      Self::Word => LedgerKind::Word,
      Self::Trivia => LedgerKind::Trivia,
    }
  }

  fn is_trivia(&self) -> bool {
    matches!(self, Self::Trivia)
  }
}

/// A space-separated word lexer over the instrumented span. A token made only of `~` is trivia,
/// so a source can open on a token the skip predicate accepts or on one it rejects.
struct LedgerLexer<'a> {
  src: &'a LedgerSrc<'a>,
  start: usize,
  end: usize,
  state: LedgerState,
}

impl<'a> crate::Lexer<'a> for LedgerLexer<'a> {
  type State = LedgerState;
  type Source = LedgerSrc<'a>;
  type Token = LedgerTok;
  type Span = LedgerSpan;
  type Offset = LedgerOffset;

  fn new(src: &'a LedgerSrc<'a>) -> Self {
    Self::with_state(src, LedgerState::default())
  }

  fn with_state(src: &'a LedgerSrc<'a>, state: LedgerState) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), LedgerErr> {
    Ok(())
  }

  fn state(&self) -> &LedgerState {
    &self.state
  }

  fn state_mut(&mut self) -> &mut LedgerState {
    &mut self.state
  }

  fn into_state(self) -> LedgerState {
    self.state
  }

  fn source(&self) -> &'a LedgerSrc<'a> {
    self.src
  }

  fn span(&self) -> LedgerSpan {
    LedgerSpan {
      start: LedgerOffset(self.start),
      end: LedgerOffset(self.end),
    }
  }

  fn slice(&self) -> &'a str {
    &self.src.0[self.start..self.end]
  }

  fn lex(&mut self) -> Option<Result<LedgerTok, LedgerErr>> {
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
    let trivia = self.src.0.as_bytes()[self.start..self.end]
      .iter()
      .all(|b| *b == b'~');
    Some(Ok(if trivia {
      LedgerTok::Trivia
    } else {
      LedgerTok::Word
    }))
  }

  fn bump(&mut self, n: &LedgerOffset) {
    self.end += n.0;
  }
}

// ── The instrumented emitter ──────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct LedgerEmitter {
  next: Cell<u64>,
  live: RefCell<std::vec::Vec<u64>>,
  emissions: usize,
}

impl LedgerEmitter {
  fn live_rows(&self) -> usize {
    self.live.borrow().len()
  }
}

impl<'inp, L, Lang: ?Sized> crate::Emitter<'inp, L, Lang> for LedgerEmitter
where
  L: crate::Lexer<'inp>,
  <L::Token as Token<'inp>>::Error: Into<LedgerErr>,
{
  type Error = LedgerErr;

  fn emit_lexer_error(
    &mut self,
    _err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), LedgerErr> {
    self.emissions += 1;
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    _err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), LedgerErr> {
    self.emissions += 1;
    Ok(())
  }

  fn emit_error(&mut self, _err: Spanned<LedgerErr, L::Span>) -> Result<(), LedgerErr> {
    self.emissions += 1;
    Ok(())
  }

  fn commit_token(&mut self, _tok: &L::Token, _span: &L::Span) {
    note(Effect::CommitToken);
  }

  fn checkpoint(&self) -> u64 {
    note(Effect::Checkpoint);
    assert!(
      !fire(&CHECKPOINT_BOMB),
      "the armed `Emitter::checkpoint` panics — a mark was taken"
    );
    let id = self.next.get() + 1;
    self.next.set(id);
    self.live.borrow_mut().push(id);
    id
  }

  fn rewind(&mut self, _cursor: &crate::input::Cursor<'inp, '_, L>, checkpoint: u64) {
    note(Effect::Rewind);
    self.live.borrow_mut().retain(|m| *m < checkpoint);
  }

  fn release(&mut self, checkpoint: u64) {
    note(Effect::Release);
    let mut live = self.live.borrow_mut();
    if let Some(pos) = live.iter().rposition(|m| *m == checkpoint) {
      live.remove(pos);
    }
  }
}

// ── The instrumented cache ────────────────────────────────────────────────────────────────────

/// `DefaultCache` with every trait method recorded. The cache is caller code too: a `Cache` impl
/// can count its calls or refuse to be called, so which cache methods a route runs is part of the
/// same question as which clones it performs.
#[derive(Default)]
struct LedgerCache<'a>(Inner<'a>);

/// The cache the wrapper delegates to. Every delegation below is fully qualified through
/// [`Cache`]: `GenericArrayDeque` has inherent methods of the same names, and an unqualified call
/// would silently reach those instead of the trait's.
type Inner<'a> = DefaultCache<'a, LedgerLexer<'a>>;

impl<'a> Cache<'a, LedgerLexer<'a>, ()> for LedgerCache<'a> {
  type Options = ();

  const RETAINS_FRONT: bool = <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::RETAINS_FRONT;

  fn new() -> Self {
    Self(<Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::new())
  }

  fn with_options(_options: ()) -> Self {
    <Self as Cache<'a, LedgerLexer<'a>, ()>>::new()
  }

  fn len(&self) -> usize {
    note(Effect::CacheLen);
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::len(&self.0)
  }

  fn remaining(&self) -> usize {
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::remaining(&self.0)
  }

  fn push_front(
    &mut self,
    tok: CachedTokenOf<'a, LedgerLexer<'a>>,
  ) -> Result<
    crate::cache::CachedTokenRefOf<'_, 'a, LedgerLexer<'a>>,
    CachedTokenOf<'a, LedgerLexer<'a>>,
  > {
    note(Effect::CachePushFront);
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::push_front(&mut self.0, tok)
  }

  fn push_back(
    &mut self,
    tok: CachedTokenOf<'a, LedgerLexer<'a>>,
  ) -> Result<
    crate::cache::CachedTokenRefOf<'_, 'a, LedgerLexer<'a>>,
    CachedTokenOf<'a, LedgerLexer<'a>>,
  > {
    note(Effect::CachePushBack);
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::push_back(&mut self.0, tok)
  }

  fn pop_front(&mut self) -> Option<CachedTokenOf<'a, LedgerLexer<'a>>> {
    note(Effect::CachePopFront);
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::pop_front(&mut self.0)
  }

  fn pop_back(&mut self) -> Option<CachedTokenOf<'a, LedgerLexer<'a>>> {
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::pop_back(&mut self.0)
  }

  fn clear(&mut self) {
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::clear(&mut self.0);
  }

  fn peek<'p, W>(
    &'p self,
    buf: &mut generic_arraydeque::GenericArrayDeque<
      crate::cache::MaybeRefCachedTokenOf<'p, 'a, LedgerLexer<'a>>,
      W::CAPACITY,
    >,
  ) where
    W: crate::Window,
  {
    note(Effect::CachePeek);
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::peek::<W>(&self.0, buf);
  }

  fn front(&self) -> Option<crate::cache::CachedTokenRefOf<'_, 'a, LedgerLexer<'a>>> {
    note(Effect::CacheFront);
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::front(&self.0)
  }

  fn back(&self) -> Option<crate::cache::CachedTokenRefOf<'_, 'a, LedgerLexer<'a>>> {
    <Inner<'a> as Cache<'a, LedgerLexer<'a>, ()>>::back(&self.0)
  }
}

type LedgerCtx<'a> = (LedgerEmitter, LedgerCache<'a>);

fn ledger_input<'a>(src: &'a LedgerSrc<'a>) -> Input<'a, LedgerLexer<'a>, LedgerCtx<'a>, ()> {
  Input::with_state_and_context(
    src,
    LedgerState::default(),
    InputContext::new(LedgerEmitter::default(), LedgerCache::default()),
  )
}

fn ledger_partial_input<'a>(
  src: &'a LedgerSrc<'a>,
) -> Input<'a, LedgerLexer<'a>, LedgerCtx<'a>, (), Partial> {
  Input::with_state_and_context(
    src,
    LedgerState::default(),
    InputContext::new(LedgerEmitter::default(), LedgerCache::default()),
  )
}

// ── The two routes, over the same stream, both on shipped code ────────────────────────────────
//
// The comparison needs no ablation. `skip_while(|_| false)` over a resident head is exactly
// `skip_until::<SkipWhile>` with a predicate that stops on the first token — and `skip_until` is
// callable from here. So the fast path and the route it replaces run over the *same* input, in the
// *same* residency, and the only variable is which entry point is used.
//
// Likewise for the head read: `peek::<U1>()` over a resident head is precisely the
// `peek_with_emitter_inner` `want == 0` arm that `peek_head_map` would have taken.

/// `skip_while(|_| false)` over a stream whose head is resident: the no-op skip, answered by the
/// fast path.
fn noop_skip_ledger() -> std::vec::Vec<Effect> {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  inp
    .skip_while(|_| {
      note(Effect::Caller);
      false
    })
    .unwrap();
  recorded()
}

/// The scan that call would otherwise have entered, over the same stream in the same residency.
fn general_scan_ledger() -> std::vec::Vec<Effect> {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  let _ = inp
    .skip_until::<SkipWhile, _, _>(
      |_| {
        note(Effect::Caller);
        true
      },
      || None,
      (),
    )
    .unwrap();
  recorded()
}

/// The `Partial` pair, where the scanner additionally captures an entry and takes an emitter mark.
/// Each also reports the emitter's outstanding-row count afterwards.
fn noop_skip_ledger_partial() -> (std::vec::Vec<Effect>, usize) {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_partial_input(&src);
  input.seal();
  let ledger = {
    let mut inp = input.as_ref();
    let _ = inp.peek::<U3>().unwrap();
    record();
    inp
      .skip_while(|_| {
        note(Effect::Caller);
        false
      })
      .unwrap();
    recorded()
  };
  let live = input.emitter().live_rows();
  (ledger, live)
}

fn general_scan_ledger_partial() -> (std::vec::Vec<Effect>, usize) {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_partial_input(&src);
  input.seal();
  let ledger = {
    let mut inp = input.as_ref();
    let _ = inp.peek::<U3>().unwrap();
    record();
    let _ = inp
      .skip_until::<SkipWhile, _, _>(
        |_| {
          note(Effect::Caller);
          true
        },
        || None,
        (),
      )
      .unwrap();
    recorded()
  };
  let live = input.emitter().live_rows();
  (ledger, live)
}

/// A width-1 head read over a resident head — the call the second fast path answers.
fn head_read_ledger() -> std::vec::Vec<Effect> {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  let _ = inp
    .peek_head_map(|_| {
      note(Effect::Caller);
    })
    .unwrap();
  recorded()
}

/// The window fill that read would otherwise have gone through, in the same residency: its
/// `want == 0` arm.
fn general_fill_ledger() -> std::vec::Vec<Effect> {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  let _ = inp.peek::<U1>().unwrap();
  recorded()
}

/// A `skip_while(is_trivia)` over a source that opens on trivia, so the probe **accepts** the head
/// and the scan runs behind it. Under [`Partial`], where the scanner's entry capture takes an
/// emitter mark, this is what places the predicate call relative to that mark.
fn accepted_head_ledger_partial() -> std::vec::Vec<Effect> {
  let src = LedgerSrc("~ ab cd");
  let mut input = ledger_partial_input(&src);
  input.seal();
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  inp
    .skip_while(|t| {
      note(Effect::Caller);
      t.data.is_trivia()
    })
    .unwrap();
  recorded()
}

fn contains(ledger: &[Effect], effect: Effect) -> bool {
  ledger.contains(&effect)
}

fn position_of(ledger: &[Effect], effect: Effect) -> Option<usize> {
  ledger.iter().position(|e| *e == effect)
}

// ── The measurement ───────────────────────────────────────────────────────────────────────────

/// **The finding, measured.** A no-op skip over a resident head runs no caller clone and takes no
/// emitter mark; the scan it replaces runs both.
///
/// Both ledgers are asserted whole, so the cell is red in either direction: it fails if the fast
/// path stops firing (the scan's steps appear on the left), it fails if the fast path grows work
/// of its own, and it fails if the *scan* stops performing the steps the right-hand list claims —
/// which is what would make this a comparison of nothing.
#[test]
fn a_no_op_skip_runs_no_caller_clone_where_the_scan_runs_two() {
  assert_eq!(
    noop_skip_ledger(),
    std::vec![Effect::CacheFront, Effect::Caller],
    "the fast path: one `Cache::front`, one predicate call, and nothing else caller-supplied"
  );
  assert_eq!(
    general_scan_ledger(),
    std::vec![
      Effect::SpanClone,
      Effect::StateClone,
      Effect::CachePopFront,
      Effect::Caller,
      Effect::CachePushFront,
    ],
    "the scan it replaces, over the same stream in the same residency: the frontier pair is \
     cloned — `L::Span::clone` and `L::State::clone`, both CALLER code — the head is taken out of \
     the cache, tested, and pushed straight back. Every value is the identity; the calls are not"
  );
}

/// The same pair under [`Partial`], where the scanner captures an entry and the capture takes an
/// [`Emitter::checkpoint`](crate::Emitter::checkpoint).
///
/// This is the sharpest form of the difference: a mark-keyed emitter sees a complete, empty
/// checkpoint/release cycle on one route and nothing at all on the other. Both leave zero rows
/// outstanding, which is the part of the emitter contract that is observable *as state* — so the
/// divergence is in the call sequence, not in the emitter's condition afterwards.
#[test]
fn a_no_op_skip_takes_no_emitter_mark_where_the_scan_takes_and_settles_one() {
  let (fast, fast_live) = noop_skip_ledger_partial();
  assert_eq!(
    fast,
    std::vec![Effect::CacheFront, Effect::Caller],
    "under `Partial` too, the fast path takes no mark and clones nothing"
  );
  assert_eq!(fast_live, 0, "and leaves no row outstanding");

  let (general, general_live) = general_scan_ledger_partial();
  assert_eq!(
    general,
    std::vec![
      Effect::OffsetClone,
      Effect::OffsetClone,
      Effect::SpanClone,
      Effect::StateClone,
      Effect::Checkpoint,
      Effect::SpanClone,
      Effect::StateClone,
      Effect::CachePopFront,
      Effect::Caller,
      Effect::CachePushFront,
      Effect::Release,
    ],
    "the scan captures an entry first — two `L::Offset::clone`s, an `L::Span::clone`, an \
     `L::State::clone` and a MARK — then clones the frontier pair, and settles the mark on the \
     stop. Eleven caller-code calls where the fast path makes two"
  );
  assert_eq!(
    general_live, 0,
    "the cycle is complete on this route: what the two routes disagree about is whether \
     `checkpoint` and `release` were CALLED, not what the emitter holds afterwards"
  );
}

/// The difference made concrete: a caller `Clone` that panics.
///
/// An `L::Span::clone`, an `L::State::clone` and an `Emitter::checkpoint` are all armed to panic.
/// Over a resident head `skip_while` returns `Ok(())` and **no bomb fires**; the scan it replaces,
/// over the same stream in the same residency, unwinds — and the fired flag says the armed step
/// was genuinely reached rather than some other panic being caught.
///
/// So this is not a claim that the two routes are indistinguishable. It is the pin on exactly how
/// they differ, in the direction the difference runs, and it is the cell that fails the day the
/// fast path stops firing: the first arm would then panic.
#[test]
fn a_no_op_skip_over_a_resident_head_reaches_no_panicking_caller_clone() {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();

  arm_bombs();
  let fast = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inp.skip_while(|_| false)));
  disarm_bombs();
  assert!(
    matches!(fast, Ok(Ok(()))),
    "a no-op skip over a resident head returns success without touching an armed caller clone"
  );
  assert!(
    !bomb_fired(),
    "and no armed step ran at all — if one had, the assertion above would be passing for the \
     wrong reason"
  );

  // The same bombs, the same stream, the same resident head — the other route.
  let general_src = LedgerSrc("ab cd ef");
  let mut general_input = ledger_input(&general_src);
  let mut general = general_input.as_ref();
  let _ = general.peek::<U3>().unwrap();
  arm_bombs();
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = general.skip_until::<SkipWhile, _, _>(|_| true, || None, ());
  }));
  disarm_bombs();
  assert!(
    caught.is_err(),
    "the scan clones the frontier pair before it asks anything, so it reaches the armed clone"
  );
  assert!(
    bomb_fired(),
    "and it is the ARMED step that panicked — the payload-executed witness"
  );
}

/// The second half of the divergence: **order**.
///
/// With a head the probe accepts, the fast path asks the predicate *before* the scanner builds
/// anything — before the `Partial` entry capture takes its emitter mark. The scan asks it after.
/// Both end with the same marks settled and the same tokens skipped, so nothing in the observable
/// tuple can see it; a predicate that reads the emitter, or a caller that records mark ids against
/// its own events, can.
#[test]
fn an_accepted_head_is_asked_before_the_scanner_captures_its_entry() {
  let ledger = accepted_head_ledger_partial();
  let asked = position_of(&ledger, Effect::Caller).expect("the predicate ran");
  let mark = position_of(&ledger, Effect::Checkpoint)
    .expect("the `Partial` entry capture takes a mark once the scan is entered");
  assert!(
    asked < mark,
    "the probe's predicate call comes before the scanner's entry capture on this route, and the \
     contract on `skip_while` says so. Got {ledger:?}"
  );

  let general = general_scan_ledger_partial().0;
  let general_asked = position_of(&general, Effect::Caller).expect("the predicate ran");
  let general_mark = position_of(&general, Effect::Checkpoint).expect("the entry capture ran");
  assert!(
    general_mark < general_asked,
    "and it is the other way round on the scan — which is what makes the line above a statement \
     about a difference. Got {general:?}"
  );
}

/// The head read's own ledger, against the fill it replaces under the same condition.
///
/// The answer to "does the second fast path skip a caller-code effect too?" is yes, and these are
/// the ones: the fill's `Cache::len` and `Cache::peek` become a single `Cache::front`. No clone,
/// no mark, no emitter call is involved on either route.
#[test]
fn a_resident_head_read_replaces_the_fills_cache_work_with_one_front_probe() {
  assert_eq!(
    without_trace_hook(head_read_ledger()),
    std::vec![Effect::CacheFront, Effect::Caller],
    "the fast path: one cache read, then the caller's own closure"
  );
  assert_eq!(
    without_trace_hook(general_fill_ledger()),
    std::vec![Effect::CacheLen, Effect::CachePeek],
    "the fill's `want == 0` arm over the same resident head: two `Cache` calls, and — being a \
     pure read of an already-met request — nothing else"
  );
}

/// The head read's one clone, and the fact that it is the *only* one on either route.
///
/// `peek_head_map` hoists `self.span().end()` above the fill for the terminal end-of-input error
/// it may have to raise. That is an `L::Offset::clone`, it is caller code, and with a head in hand
/// the arms that need it are unreachable — so the fast path does not read it. Nothing else about
/// a head read differs: a peek commits nothing and marks nothing either way.
#[test]
fn the_general_head_read_route_clones_the_end_offset_the_fast_path_never_reads() {
  let cold = head_read_ledger_cold();
  assert!(
    contains(&cold, Effect::OffsetClone),
    "the general route clones the committed span's end offset before the fill. Got {cold:?}"
  );
  assert!(
    !contains(&cold, Effect::Checkpoint) && !contains(&cold, Effect::Release),
    "but a head read takes no emitter mark on EITHER route. Got {cold:?}"
  );
  assert!(
    !contains(&without_trace_hook(head_read_ledger()), Effect::OffsetClone),
    "and the resident-head route reads no offset at all"
  );
}

/// A head read with nothing resident: the general route, including the lex it has to perform.
fn head_read_ledger_cold() -> std::vec::Vec<Effect> {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  record();
  let _ = inp
    .peek_head_map(|_| {
      note(Effect::Caller);
    })
    .unwrap();
  recorded()
}

// ── The precondition, and that it is load-bearing ──────────────────────────────────────────────
//
// !!! NOTHING BELOW IS SUPPORTED BEHAVIOUR. !!!
//
// Everything above measures a difference in WHICH caller code each route runs, and concludes that
// no *value* a caller can read back differs. That conclusion has a condition on it, stated on
// `skip_while` and on `peek_head_map` as two clauses about the caller rather than as a list of
// exclusions:
//
//   1. the caller's input-layer callbacks — the `Clone`/`Drop` of `L::Span`, `L::State` and
//      `L::Offset`, the `Emitter` methods these paths reach, every `Cache` method, the `Lexer`
//      and its `Source` — are INERT: each does only what its own contract says it does, always
//      returns, and never unwinds;
//   2. the caller's predicate or closure is a FUNCTION OF WHAT IT IS HANDED: it answers from the
//      values of the `Spanned<&L::Token, &L::Span>` it receives, and from nothing else.
//
// The four cells below are those two clauses made concrete: each builds a caller that fails one
// of them and shows that the failure is not a call count but a **different parse** — a different
// skip decision, a different committed cursor, different tokens consumed, a different returned
// value, or a different number of predicate calls surviving an unwind. Clause 2 fails in the
// first two; clause 1 — the *non-unwinding* half — fails in the last two, and those two matter
// most, because their callers satisfy clause 2 in full: a predicate that records only its own
// calls observes no input-layer side effect at all, and still cannot be promised the same call
// sequence once a caller `Clone` panics.
//
// They exist so that the condition is a measured fact rather than a caveat nobody checked, and
// they pin a **documented precondition, not a behaviour**. No caller may rely on either column:
// which route answers a given call is this crate's choice and may change between versions, so the
// only thing a caller can do with the difference below is meet the condition. `Clone` is not
// required to be pure, total or non-unwinding in Rust, and `L::Span`, `L::State` and `L::Offset`
// are the caller's own types, which is exactly why the condition has to be written down instead
// of assumed.
//
// All four are red against a build that runs the slow route: ablate either fast path and its two
// columns collapse onto each other, and the `assert_ne!` that carries the finding fails.

/// How many caller `Clone` calls the input layer has run since [`record`] started this ledger.
///
/// This is the hazard in one function: a `Clone` that bumps a counter, and a predicate that reads
/// it. Both halves are ordinary Rust and neither is forbidden by any trait bound in this crate.
fn clones_so_far() -> usize {
  LEDGER.with(|l| {
    l.borrow()
      .iter()
      .filter(|e| {
        matches!(
          e,
          Effect::SpanClone | Effect::StateClone | Effect::OffsetClone
        )
      })
      .count()
  })
}

/// The same reading, narrowed to `L::Offset::clone` — the one the head read's routes differ by.
fn offset_clones_so_far() -> usize {
  LEDGER.with(|l| {
    l.borrow()
      .iter()
      .filter(|e| **e == Effect::OffsetClone)
      .count()
  })
}

/// What a route left the stream in: the resume cursor, and every token still to be read.
#[derive(Debug, PartialEq, Eq)]
struct Left {
  cursor: usize,
  remaining: std::vec::Vec<(usize, usize)>,
}

/// Drains the stream, as `(start, end)` byte pairs — "which tokens did this call consume", read
/// from the other side.
fn drain_spans<'a>(
  inp: &mut InputRef<'a, '_, LedgerLexer<'a>, LedgerCtx<'a>, ()>,
) -> std::vec::Vec<(usize, usize)> {
  let mut out = std::vec::Vec::new();
  while let Some(tok) = inp.next().unwrap() {
    let span = tok.span_ref();
    out.push((span.start.0, span.end.0));
  }
  out
}

/// "Skip this token once the input layer has cloned something." On the fast path nothing has been
/// cloned by the time the head is asked, so this answers *keep* and the skip stops; the scan
/// clones the frontier pair before it asks anything, so the same predicate answers *skip*.
fn skip_once_something_has_been_cloned(clones: usize) -> bool {
  clones > 0
}

/// The mirror image: "skip only while nothing has been cloned yet."
fn skip_only_while_nothing_has_been_cloned(clones: usize) -> bool {
  clones == 0
}

/// `skip_while` over a resident head, with a predicate whose answer is a function of the input
/// layer's clone count rather than of the token it was handed.
fn skip_while_asking_about_clones(skip_when: fn(usize) -> bool) -> Left {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  inp.skip_while(|_| skip_when(clones_so_far())).unwrap();
  let _ = recorded();
  let cursor = inp.cursor().as_inner().0;
  let remaining = drain_spans(&mut inp);
  Left { cursor, remaining }
}

/// The same predicate, over the same stream in the same residency, driven through the scan the
/// fast path replaces — `skip_while(pred)` *is* `skip_until::<SkipWhile>(|t| !pred(t))`.
fn scan_asking_about_clones(skip_when: fn(usize) -> bool) -> Left {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  let _ = inp
    .skip_until::<SkipWhile, _, _>(|_| !skip_when(clones_so_far()), || None, ())
    .unwrap();
  let _ = recorded();
  let cursor = inp.cursor().as_inner().0;
  let remaining = drain_spans(&mut inp);
  Left { cursor, remaining }
}

/// **The precondition, in the direction that matters most.** A predicate that reads the input
/// layer's clone counter makes the two routes disagree about *what to skip* — so the value
/// guarantees on [`skip_while`](InputRef::skip_while) are conditional on the caller, and the
/// contract now says so.
///
/// The mechanism is the measured table above and nothing else: the scan clones the frontier pair
/// **before** it asks the predicate anything, and the fast path asks first and clones nothing. A
/// predicate keyed on that counter therefore answers one way here and the other way there, and
/// the answer to a skip predicate is the skip.
///
/// Both directions are pinned, because the reviewer's claim is symmetric — the fast path can stop
/// where the scan skips *and* skip where the scan stops:
///
/// | resident head | fast path | the scan |
/// |---|---|---|
/// | the probe **rejects** it (`clones > 0`, and nothing is cloned yet) | consumes nothing | consumes everything |
/// | the probe **accepts** it (`clones == 0`) | consumes the head | consumes nothing |
///
/// This is a **documented precondition, not a supported behaviour.** Neither column is a promise:
/// which route answers a call is this crate's choice, and a caller who can tell them apart is
/// asking the library a question about itself. The cell is here to prove the exclusion earns its
/// place in the contract, and it is red against a build that runs the slow route — ablate the
/// fast path and the two columns become the same.
#[test]
fn a_clone_counting_predicate_can_change_the_skip_decision() {
  // ── The probe REJECTS the head: the fast path stops, the scan skips ──
  let fast = skip_while_asking_about_clones(skip_once_something_has_been_cloned);
  let scan = scan_asking_about_clones(skip_once_something_has_been_cloned);
  assert_eq!(
    fast,
    Left {
      cursor: 0,
      remaining: std::vec![(0, 2), (3, 5), (6, 8)],
    },
    "the head is asked before anything is cloned, so the predicate says keep and the skip stops \
     on the first token — nothing consumed"
  );
  assert_eq!(
    scan,
    Left {
      cursor: 8,
      remaining: std::vec![],
    },
    "the scan clones the frontier pair first, so the SAME predicate says skip, and keeps saying \
     it: every token in the stream is consumed"
  );
  assert_ne!(
    fast, scan,
    "and that is the finding: not a clone count, a different parse. The cursor differs and the \
     tokens consumed differ, from one predicate that answers a question about the library rather \
     than about the input"
  );

  // ── The probe ACCEPTS the head: the fast path skips, the scan stops ──
  let fast = skip_while_asking_about_clones(skip_only_while_nothing_has_been_cloned);
  let scan = scan_asking_about_clones(skip_only_while_nothing_has_been_cloned);
  assert_eq!(
    fast,
    Left {
      cursor: 3,
      remaining: std::vec![(3, 5), (6, 8)],
    },
    "nothing is cloned when the head is asked, so the probe accepts it and its answer is carried \
     into the scan; by the second token the frontier pair has been cloned and the skip stops"
  );
  assert_eq!(
    scan,
    Left {
      cursor: 0,
      remaining: std::vec![(0, 2), (3, 5), (6, 8)],
    },
    "the scan has already cloned the frontier pair when it asks about that same first token, so \
     it stops there and skips nothing"
  );
  assert_ne!(
    fast, scan,
    "the divergence runs both ways — one token consumed against none, which is why the \
     precondition is stated as a condition on the caller and not as a note about counters"
  );
}

/// The caller's `f`, shared by both routes: its value is a function of the input layer's
/// `L::Offset::clone` count and not of the head it was handed.
fn f_reads_the_offset_clone_count(_head: Spanned<&LedgerTok, &LedgerSpan>) -> usize {
  offset_clones_so_far()
}

/// [`peek_head_map`](InputRef::peek_head_map) over a resident head — the fast path — with that
/// `f`.
fn head_read_value_fast() -> usize {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  let value = inp.peek_head_map(f_reads_the_offset_clone_count).unwrap();
  let _ = recorded();
  value.expect("the head is resident, so `f` ran")
}

/// The same `f`, over the same stream in the same residency, driven through the body
/// `peek_head_map` runs when its probe finds nothing: the hoisted end-offset clone, the fill, and
/// the same `f` over the entry the fill hands back.
fn head_read_value_general() -> usize {
  use crate::cache::PeekedTokenExt as _;

  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  let value = {
    // Hoisted above the fill exactly as `peek_head_map` hoists it: the `L::Offset::clone` the
    // terminal end-of-input error would need, taken whether or not that error can arise.
    let _end = crate::Span::end(inp.span());
    let (mut peeked, _terminal, _emitter) = inp.peek_with_emitter_terminal::<U1>().unwrap();
    let head = peeked.pop_front().expect("the head is resident");
    f_reads_the_offset_clone_count(Spanned::new(head.span(), head.token()))
  };
  let _ = recorded();
  value
}

/// **The same precondition on the second fast path.** `F: FnOnce(..) -> O` can capture shared
/// state, and the general route clones an `L::Offset` before it calls `f` — so an `f` that reads
/// that counter returns a **different `O`** depending on which route answered.
///
/// The value guarantee on [`peek_head_map`](InputRef::peek_head_map) is "the same value handed to
/// `f`, and therefore the same value returned". The first half survives this caller; the second
/// does not, because `f` is not a function of what it was handed. And the consequence is not
/// confined to a peek: [`head_satisfies`](InputRef::head_satisfies) and
/// [`peek_kind`](InputRef::peek_kind) ride this call, so an `O` that differs is a grammar
/// decision that differs.
///
/// The difference is exactly one clone — `self.span().end()`, hoisted above the fill for the
/// terminal error the general route may have to raise — so the assertion is stated as a
/// difference rather than as two absolute numbers: with the `trace` feature on, both routes are
/// one richer, because the trace hook clones the cursor offset on each.
///
/// This is a **documented precondition, not a supported behaviour.** The measured column is not a
/// promise about how many offsets either route clones; it is the proof that an `f` able to count
/// them is a caller the contract has to exclude. Red against a build that runs the slow route:
/// ablate the fast path and both routes return the same number.
#[test]
fn an_offset_clone_counting_f_can_change_the_value_peek_head_map_returns() {
  let fast = head_read_value_fast();
  let general = head_read_value_general();
  assert_ne!(
    fast, general,
    "one `f`, one stream, one residency — and a different `O` out of each route. That is a \
     different parse for any caller who builds a decision on it"
  );
  assert_eq!(
    general,
    fast + 1,
    "and the difference is the single hoisted `L::Offset::clone`: the end offset the general \
     route takes for an end-of-input error that a resident head makes unreachable (fast={fast}, \
     general={general})"
  );
}

// ── The other clause: a callback that UNWINDS, for a caller who observes nothing ───────────────
//
// The two cells above fail clause 2 — their predicate and their `f` read the input layer. The two
// below satisfy clause 2 completely and fail clause 1 instead, in its non-unwinding half. That is
// the sharper statement, and the one three rounds of narrowing kept missing: a caller can record
// nothing but its own calls, answer from nothing but the token it was handed, and still see the
// two routes disagree — because *whether the caller's code was reached at all* depends on where
// in the route a panicking caller `Clone` sits.

/// A predicate that observes nothing: it records its own call and accepts every token. Under
/// clause 2 this caller is entirely in contract — it reads no clone count, no mark, no cache call.
fn recording_accepting_pred(_t: Spanned<&LedgerTok, &LedgerSpan>) -> bool {
  caller_call();
  true
}

/// The same shape for the head read: `f` records its own call and returns nothing.
fn recording_f(_head: Spanned<&LedgerTok, &LedgerSpan>) {
  caller_call();
}

/// **Clause 1, the non-unwinding half, on `skip_while`.** A caller `Clone` that panics decides
/// *how many times the predicate ran*, and the two routes answer differently.
///
/// The head is one the probe **accepts**, so this route asks `pred` and then enters the scan —
/// where the frontier clone is armed. The scan it replaces clones the frontier pair before it asks
/// anything, so the same armed clone fires before the first predicate call:
///
/// | route | order | predicate calls before the unwind |
/// |---|---|---|
/// | this one | `Cache::front`, `pred`, **`L::Span::clone` panics** | **1** |
/// | the scan | **`L::Span::clone` panics** | **0** |
///
/// The predicate here reads nothing — it records its own calls and returns `true` — so it meets
/// the "function of what it is handed" clause in full. What it fails is the *other* clause, and
/// not even by its own doing: the `Clone` unwinds. Catch the unwind and the caller's retry state
/// differs by one call between the routes, which is why the guarantee on the predicate call
/// sequence is stated under a condition that says **non-unwinding** and not merely **pure**.
///
/// This is a **documented precondition, not a supported behaviour.** Neither column is a promise.
/// Red against a build that runs the slow route: ablate the probe and both columns read 0.
#[test]
fn an_unwinding_caller_clone_leaves_the_predicate_with_a_different_call_count() {
  // ── This route: the head is asked, and the armed clone comes after ──
  let src = LedgerSrc("~ ab cd");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  record();
  reset_caller_calls();
  arm_bombs();
  let fast = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    inp.skip_while(recording_accepting_pred)
  }));
  disarm_bombs();
  let fast_ledger = recorded();
  let fast_calls = caller_calls();
  let fast_fired = bomb_fired();

  assert!(
    fast.is_err(),
    "the probe accepts the head, so this route still enters the scan — whose frontier clone is \
     armed. The fast path is not a claim that nothing can panic; it is a claim about ORDER"
  );
  assert!(
    fast_fired,
    "and it is the armed clone that panicked, not something else"
  );
  assert_eq!(
    fast_ledger,
    std::vec![Effect::CacheFront, Effect::Caller, Effect::SpanClone],
    "the order is the whole finding: probe, predicate, THEN the clone that unwinds"
  );
  assert_eq!(
    fast_calls, 1,
    "so the predicate ran exactly once before the unwind"
  );

  // ── The scan it replaces: same stream, same residency, same predicate, opposite order ──
  let scan_src = LedgerSrc("~ ab cd");
  let mut scan_input = ledger_input(&scan_src);
  let mut scan = scan_input.as_ref();
  let _ = scan.peek::<U3>().unwrap();
  record();
  reset_caller_calls();
  arm_bombs();
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _ = scan.skip_until::<SkipWhile, _, _>(|t| !recording_accepting_pred(t), || None, ());
  }));
  disarm_bombs();
  let scan_ledger = recorded();
  let scan_calls = caller_calls();

  assert!(
    caught.is_err() && bomb_fired(),
    "the scan reaches the armed clone too — it is the same armed clone"
  );
  assert_eq!(
    scan_ledger,
    std::vec![Effect::SpanClone],
    "but it is the FIRST caller step the scan takes, so nothing else got to run"
  );
  assert_eq!(scan_calls, 0, "and the predicate never ran at all");

  assert_ne!(
    fast_calls, scan_calls,
    "one predicate call against none, from a predicate that observes nothing and a `Clone` that \
     panics. A caller who catches the unwind and retries starts from different state depending on \
     which route this crate took — which is why the value guarantees are conditioned on callbacks \
     that do not unwind, and not only on callbacks that are pure"
  );
}

/// **Clause 1, the non-unwinding half, on `peek_head_map`.** The same class on the head read, and
/// here it decides not how often `f` ran but *whether it ran at all*.
///
/// The general route hoists `self.span().end()` above the fill — one `L::Offset::clone`, taken for
/// a terminal error a resident head makes unreachable. Arm that clone to panic and the general
/// route unwinds before `f`; this route never reads the offset, so `f` runs and the call returns
/// `Ok(Some(_))`. One armed clone, one stream, one residency, and one route produces a value where
/// the other produces a panic.
///
/// The bomb is keyed to that one offset value rather than to every offset clone, because under
/// `trace` this route clones the frontier offset for the hook's preview — see
/// [`arm_offset_clone_bomb_at`]. The cell therefore measures the same difference with the feature
/// on and off.
///
/// `f` here observes nothing but its own calls, so — as in the cell above — the caller is fully
/// within the "function of what it is handed" clause and outside the non-unwinding one.
///
/// This is a **documented precondition, not a supported behaviour.** Red against a build that runs
/// the slow route: ablate the probe and this route unwinds too, with `f` never called.
#[test]
fn an_unwinding_offset_clone_decides_whether_peek_head_maps_closure_runs_at_all() {
  // The offset the general route hoists: the committed span's end. Read before the ledger opens,
  // so arming it neither records a step nor fires on the read that discovers it.
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();
  let hoisted = crate::Span::end(inp.span()).0;

  record();
  reset_caller_calls();
  arm_offset_clone_bomb_at(hoisted);
  let fast = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    inp.peek_head_map(recording_f)
  }));
  disarm_offset_clone_bomb();
  let fast_ledger = recorded();
  let fast_calls = caller_calls();

  assert!(
    matches!(fast, Ok(Ok(Some(())))),
    "this route reads the head where it lies, so the armed offset is never cloned and the call \
     answers normally"
  );
  assert!(!bomb_fired(), "nothing armed ran");
  assert_eq!(
    without_trace_hook(fast_ledger),
    std::vec![Effect::CacheFront, Effect::Caller],
    "one cache read, then the caller's own closure"
  );
  assert_eq!(fast_calls, 1, "and `f` ran");

  // ── The body `peek_head_map` runs when its probe finds nothing, over the same resident head ──
  let general_src = LedgerSrc("ab cd ef");
  let mut general_input = ledger_input(&general_src);
  let mut general = general_input.as_ref();
  let _ = general.peek::<U3>().unwrap();
  assert_eq!(
    crate::Span::end(general.span()).0,
    hoisted,
    "same stream, same residency, same committed span — the armed offset is the same one"
  );

  record();
  reset_caller_calls();
  arm_offset_clone_bomb_at(hoisted);
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    use crate::cache::PeekedTokenExt as _;
    // Exactly what the method runs when its probe misses, in the order it runs it.
    let _end = crate::Span::end(general.span());
    let (mut peeked, _terminal, _emitter) = general.peek_with_emitter_terminal::<U1>().unwrap();
    let head = peeked.pop_front().expect("the head is resident");
    recording_f(Spanned::new(head.span(), head.token()));
  }));
  disarm_offset_clone_bomb();
  let general_ledger = recorded();
  let general_calls = caller_calls();

  assert!(
    caught.is_err() && bomb_fired(),
    "the hoisted clone is the general route's first caller step, and it is armed"
  );
  assert_eq!(
    general_ledger,
    std::vec![Effect::OffsetClone],
    "so the route gets no further: no fill, no cache read, no `f`"
  );
  assert_eq!(general_calls, 0, "`f` never ran");

  assert_ne!(
    fast_calls, general_calls,
    "`f` ran on one route and not on the other, from an `f` that observes nothing. `Ok(Some(_))` \
     against an unwind is the largest form the difference takes, and it is why the condition on \
     these methods names unwinding explicitly"
  );
}
