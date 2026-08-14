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
//! Every program runs at four prefill **widths** and under three cache capacities, and the whole
//! observable tuple — including **which tokens the predicate was asked about, in order** — must
//! agree across all twelve. A width, not a repetition count, and the residency each one achieves
//! is measured rather than assumed: `DefaultCache` is `U3`, so a sweep that issued `peek::<U3>`
//! `n` times reached the same residency for every `n > 0`, and the four depths it reported were
//! two states. Each capacity now asserts which residencies it reached — four, two and one
//! respectively. That last column is the one a naive `skip_while` fast path fails:
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
//! [`peek_map`](InputRef::peek_map) is those same two edges one window wide, plus the third state a
//! head read cannot have — a window that came back **part** resident — and it is pinned in the same
//! place for that reason rather than because it has a fast path (it has none). What it does share
//! with the pair above is the thing that decides whether any of it is observable: the **emitter**.
//! A trip-truncated window and a genuinely short one are the same value, so only an emitter that
//! ACCEPTS the trip's diagnostic leaves a caller holding it — under a fatal one the parse is over
//! either way. Every cell there runs under `Verbose`, with one fatal cell beside them whose whole
//! job is to show that it cannot see the difference.
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

use generic_arraydeque::typenum::{U1, U2, U3, U4};

use crate::{
  InputRef, Token,
  cache::{Cache, CachedTokenOf, DefaultCache, PeekedTokenExt as _},
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

/// The same fixture with the one thing that decides whether a folded terminal stop is visible
/// changed: an emitter that REJECTS the diagnostic rather than accepting it.
type BalFatalCtx<'a> = (
  crate::emitter::Fatal<ByValErr>,
  DefaultCache<'a, BalLexer<'a>>,
);

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

/// The prefill **widths**. `0` is the negation — nothing is resident, so the probe finds no head
/// and the general path runs.
///
/// A width, not a repetition count. [`DefaultCache`] is `U3`, so one `peek::<U3>` saturates it and
/// a sweep that issued that same peek `n` times left the identical residency for every `n > 0`:
/// four "depths" that were two states. [`prefill_to_width`] issues one peek of the matching width
/// instead, and hands back the residency it actually achieved — which the capacity sweep below
/// asserts, per capacity, rather than assuming.
const PREFILLS: &[usize] = &[0, 1, 2, 3];

/// Fills the cache from a window of `width` tokens, and returns the residency that left.
///
/// The residency counts the parked slot as well as the cache, because under a capacity that
/// retains nothing the front of the stream *is* the parked slot. Nothing is parked by a peek, so
/// after a prefill the two agree; the sum is what makes the number mean "how much of the stream is
/// resident" under every capacity rather than only under the retaining ones.
fn prefill_to_width<'a, C>(
  inp: &mut InputRef<'a, '_, BalLexer<'a>, (Verbose<ByValErr>, C), ()>,
  width: usize,
) -> usize
where
  C: Cache<'a, BalLexer<'a>, ()>,
{
  match width {
    0 => {}
    1 => {
      let _ = inp.peek::<U1>().unwrap();
    }
    2 => {
      let _ = inp.peek::<U2>().unwrap();
    }
    3 => {
      let _ = inp.peek::<U3>().unwrap();
    }
    _ => unreachable!("`PREFILLS` runs 0..=3, the capacity of `DefaultCache`"),
  }
  Cache::len(inp.cache()) + usize::from(inp.has_front_parked())
}

/// Runs `program` under one cache capacity and one prefill width, and reports the residency the
/// prefill actually achieved alongside the observation.
fn observe_with_depth<'a, C>(program: Program, src: &'a str, prefill: usize) -> (Observed, usize)
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

  let (depth, cursor, span, is_exhausted, drained) = {
    let mut inp = input.as_ref();
    // The prefill: whatever the capacity retains of it is what the probe will find.
    let depth = prefill_to_width(&mut inp, prefill);
    run(&mut inp, program, &mut asked, &mut read);
    let cursor = *inp.cursor().as_inner();
    let span = *inp.span();
    let is_exhausted = inp.is_exhausted();
    let mut drained = std::vec::Vec::new();
    while let Some(tok) = inp.next().unwrap() {
      drained.push((*tok.span_ref(), tok.data().kind()));
    }
    (depth, cursor, span, is_exhausted, drained)
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

  (
    Observed {
      asked,
      read,
      cursor,
      span,
      is_exhausted,
      diagnostics: (lex, limit),
      drained,
    },
    depth,
  )
}

/// [`observe_with_depth`] without the residency — for the cells that pin what the sweep agrees on.
fn observe<'a, C>(program: Program, src: &'a str, prefill: usize) -> Observed
where
  C: Cache<'a, BalLexer<'a>, ()> + Default,
{
  observe_with_depth::<C>(program, src, prefill).0
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
  // Which residencies each capacity actually reached — a sweep that stops varying the thing it
  // varies is the defect this file has now been caught by three times, so it is asserted.
  let mut reached = [[false; 4]; 3];
  for &program in Program::ALL {
    for &src in SOURCES {
      let reference = observe::<DefaultCache<'_, BalLexer<'_>>>(program, src, 0);
      for &prefill in PREFILLS {
        let deque = observe_with_depth::<DefaultCache<'_, BalLexer<'_>>>(program, src, prefill);
        let one = observe_with_depth::<OneSlot<'_>>(program, src, prefill);
        let none = observe_with_depth::<()>(program, src, prefill);
        for (slot, (capacity, (got, depth))) in [
          ("deque(3)", deque),
          ("option(1)", one),
          ("blackhole(0)", none),
        ]
        .into_iter()
        .enumerate()
        {
          assert_eq!(
            got, reference,
            "{program:?} over {src:?} observed differently at prefill width {prefill} under \
             {capacity} than it does with an empty cache — a fast path that fires only when the \
             head happens to be resident has changed what the caller sees"
          );
          reached[slot][depth] = true;
        }
      }
    }
  }
  assert_eq!(
    reached,
    [
      [true, true, true, true],
      [true, true, false, false],
      [true, false, false, false],
    ],
    "each capacity must reach exactly the residencies it can hold: four under the deque, two \
     under the one-slot cache, and only the empty one under the cache that retains nothing. This \
     is the assertion the old `for _ in 0..prefill {{ peek::<U3>() }}` sweep could not have made — \
     it reached two states and reported four"
  );
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

// ── The same two edges, one window wide: `peek_map` ───────────────────────────────────────────
//
// `peek_head_map` has no fast path to defend at width > 1, so these cells are not about a route.
// They are here because they are the WINDOWED FORM of the two edges pinned directly above, and
// they add the third state a head read cannot have: a window that came back **part** resident.
//
//   width 1                                          width W
//   ─────────────────────────────────────────────    ─────────────────────────────────────────
//   the head is resident        → served, latch      the whole window is resident → served
//   or no latch                                      (the fill's `want == 0` arm returns before
//                                                     the boundary is consulted, exactly as for
//                                                     the head)
//   nothing resident, latched   → raises             SOME of it resident            → raises
//                                                    none of it resident            → raises
//
// The middle row is the whole reason `peek_map` exists. `peek::<W>` answers it `Ok` with a short
// window, and that value is byte-for-byte the one a genuinely short input produces — so a caller
// deciding on the window's LENGTH reads a halted scanner as a grammar fact.
//
// WHICH EMITTER THESE RUN UNDER IS LOAD-BEARING, and it is why the fatal cell below exists beside
// the rest. `BalCtx` is `Verbose`, which ACCEPTS a diagnostic and returns `Ok` from the emit, so
// the fill comes back with a short window and a raise is the only thing that can turn it into an
// `Err`. Under a **fatal** emitter the fill's own emit returns `Err` first and every shape in this
// section passes for the wrong reason: `peek` and `peek_map` return the same value, and a fold
// would too.

/// The fatal twin of [`tripping_input`], for the control cell below: the emit that a trip performs
/// REJECTS the diagnostic, so the fill fails before the terminal flag is ever read.
fn fatal_tripping_input(src: &str, limit: usize) -> Input<'_, BalLexer<'_>, BalFatalCtx<'_>, ()> {
  Input::with_state_and_context(
    src,
    TokenLimiter::with_limitation(limit),
    crate::input::InputContext::new(
      crate::emitter::Fatal::<ByValErr>::new(),
      DefaultCache::<'_, BalLexer<'_>>::default(),
    ),
  )
}

/// Trips `"1 2 3 4"` at a budget of three and leaves the input at the latch with `1 2 3` cached:
/// the `U4` fill lexes three tokens into the `U3` cache and the fourth trips, so the poison
/// boundary lands on the end of `3` — which is also `offset()`, the back of the cache.
///
/// From there a `U3` request is already met and a `U4` request is not, which is the pair of states
/// the two latched cells below need.
fn latched_with_three_cached(src: &str) -> Input<'_, BalLexer<'_>, BalCtx<'_>, ()> {
  let mut input = tripping_input(src, 3);
  {
    let mut inp = input.as_ref();
    assert_eq!(
      inp.peek::<U4>().map(|w| w.len()),
      Ok(3),
      "the wide fill trips on the fourth token and hands back the three it retained"
    );
  }
  input
}

#[test]
fn peek_map_serves_a_fully_resident_window_at_a_latched_boundary() {
  // The windowed form of `peek_head_map_serves_a_resident_head_at_a_latched_boundary`, and the
  // same reason it must hold: the fill's `want == 0` arm returns before the boundary is consulted,
  // so a request the front of the stream already meets is answered whatever the latch says. A
  // `peek_map` that probed the latch first would raise on a window it is holding.
  let mut input = latched_with_three_cached("1 2 3 4");
  let mut inp = input.as_ref();

  assert_eq!(
    inp.peek_map::<U3, _, _>(|w| w.len()),
    Ok(3),
    "a window the front of the stream already fills is served, latched or not"
  );
  assert_eq!(
    inp.peek_map::<U3, _, _>(|w| w.iter().map(|t| *t.span()).collect::<std::vec::Vec<_>>()),
    Ok(std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5)
    ]),
    "and it is the same window `peek` reports, in stream order"
  );
  // Nothing was consumed by either read.
  assert_eq!(
    inp.next().unwrap().map(|t| *t.span_ref()),
    Some(SimpleSpan::new(0, 1)),
    "a read consumes nothing, latched or not"
  );
}

#[test]
fn peek_map_raises_when_a_latched_boundary_leaves_the_window_part_resident() {
  // The middle row of the table above, and the state a head read cannot reach. Three of the four
  // slots are sitting in the cache; the fourth is past a latched boundary and can never be lexed.
  // `peek::<U4>` calls that `Ok` with three tokens — which is exactly what `"1 2 3"` under no
  // budget at all produces (the control at the bottom of this file's next cell).
  let mut input = latched_with_three_cached("1 2 3 4");
  let mut inp = input.as_ref();

  let ran = core::cell::Cell::new(0usize);
  assert_eq!(
    inp.peek_map::<U4, _, _>(|w| {
      ran.set(ran.get() + 1);
      w.len()
    }),
    Err(ByValErr::Lex),
    "a window truncated by a latched boundary is a terminal stop, not a short input"
  );
  assert_eq!(
    ran.get(),
    0,
    "`f` never sees a truncated window — a decision taken on one is a decision taken on a halt"
  );
  // Sticky, exactly as the head read's latch is.
  assert_eq!(inp.peek_map::<U4, _, _>(|w| w.len()), Err(ByValErr::Lex));

  // THE BEHAVIOUR THAT MUST NOT CHANGE. `peek::<U4>` is public, its Partial-mode contract is
  // documented and tested, and a caller relying on the short window is not wrong today.
  assert_eq!(
    inp.peek::<U4>().map(|w| w.len()),
    Ok(3),
    "`peek` still hands the short window back; `peek_map` is an addition, not an alteration"
  );
}

#[test]
fn peek_map_tells_a_fresh_trip_apart_from_a_genuinely_short_input() {
  // THE PROPERTY, on the fill's other terminal route: a fresh trip mid-window rather than a
  // boundary latched before the call. Two streams, the same request, the same window length back
  // from `peek` — and `peek_map` separates them.
  fn window_len<'a>(
    inp: &mut InputRef<'a, '_, BalLexer<'a>, BalCtx<'a>, ()>,
  ) -> (Result<usize, ByValErr>, Result<usize, ByValErr>) {
    (
      inp.peek::<U3>().map(|w| w.len()),
      inp.peek_map::<U3, _, _>(|w| w.len()),
    )
  }

  // (a) the scanner halts while the window fills: `1` and `2` scan, `3` trips the budget.
  let mut tripped = tripping_input("1 2 3 4", 2);
  assert_eq!(
    window_len(&mut tripped.as_ref()),
    (Ok(2), Err(ByValErr::Lex)),
    "a trip-truncated window is `Ok(2)` from `peek` and an error from `peek_map`"
  );

  // (b) the input genuinely ends after two tokens. `peek` cannot be told from (a), and `peek_map`
  //     reserves the short `Ok` window for exactly this.
  let mut short = tripping_input("1 2", usize::MAX);
  assert_eq!(
    window_len(&mut short.as_ref()),
    (Ok(2), Ok(2)),
    "a genuinely short input is a short window, not an error"
  );

  // (c) the control at full width: a met request is `Ok` on both.
  let mut full = tripping_input("1 2 3 4", usize::MAX);
  assert_eq!(window_len(&mut full.as_ref()), (Ok(3), Ok(3)));

  // And the identity closure recovers `peek` itself, terminal stop aside — so nothing the unmapped
  // read can express is lost by taking an `f`.
  let mut same = tripping_input("1 2 3 4", usize::MAX);
  let mut inp = same.as_ref();
  assert_eq!(
    inp.peek_map::<U3, _, _>(|w| w.iter().map(|t| *t.span()).collect::<std::vec::Vec<_>>()),
    Ok(std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5)
    ])
  );
}

#[test]
fn the_second_token_projection_from_peek_maps_documentation_compiles_and_decides() {
  // The shape `peek_map`'s doc comment shows, run rather than asserted — a doc that misdescribes
  // an API is a contract defect, and a fenced `text` block is not compiled by anything else.
  //
  // It is also the motivating consumer: `other + fill` (an addition of a selector named `fill`)
  // against `other + fill(0) x` (a fill modifier), where the token in second position is the only
  // separator. Here `(` versus `;` stands in for it.
  fn second_kind<'a>(
    inp: &mut InputRef<'a, '_, BalLexer<'a>, BalCtx<'a>, ()>,
  ) -> Result<Option<BalKind>, ByValErr> {
    inp.peek_map::<U2, _, _>(|w| w.iter().nth(1).map(|t| t.token().kind()))
  }

  let mut modifier = tripping_input("1 ( 2", usize::MAX);
  assert_eq!(
    second_kind(&mut modifier.as_ref()),
    Ok(Some(BalKind::LParen)),
    "the second token decides the production"
  );

  let mut addition = tripping_input("1 ; 2", usize::MAX);
  assert_eq!(second_kind(&mut addition.as_ref()), Ok(Some(BalKind::Semi)));

  let mut ends = tripping_input("1", usize::MAX);
  assert_eq!(
    second_kind(&mut ends.as_ref()),
    Ok(None),
    "the input genuinely ends after the head — `None` is the caller's own projection"
  );

  // And the case the primitive exists for: the scanner halts while the second slot fills. `peek`
  // answers this exactly as it answers the line above.
  let mut halted = tripping_input("1 ( 2", 1);
  assert_eq!(
    halted.as_ref().peek::<U2>().map(|w| w.len()),
    Ok(1),
    "`peek` reports one token, the same as an input that genuinely ends after the head"
  );
  let mut halted = tripping_input("1 ( 2", 1);
  assert_eq!(
    second_kind(&mut halted.as_ref()),
    Err(ByValErr::Lex),
    "and the projection raises rather than answering `None` and picking the other production"
  );
}

#[test]
fn a_fatal_emitter_cannot_tell_the_two_apart_at_all() {
  // THE CONTROL FOR THE THREE CELLS ABOVE, and the reason they are written against `Verbose`.
  //
  // Here the trip's diagnostic is REJECTED, so the fill returns the emitter's own error — a
  // `Limit`, converted from the lexer error, and not the `Lex` that `From<UnexpectedEot>` produces
  // above. `peek` and `peek_map` therefore agree, and a `peek_map` that folded the terminal stop
  // into a short window would agree too. This cell is blind to the defect by construction; that is
  // what it is here to show.
  //
  // A FRESH INPUT PER READ, deliberately. Two reads on one input do not compare the two
  // primitives: the first read trips and LATCHES, so the second takes the boundary arm — which
  // emits nothing, and where `peek_map` raises `Lex` under a fatal emitter exactly as it does
  // under `Verbose`. That is the case named at the bottom of this cell, and running it here by
  // accident is how a control quietly stops being one.
  let mut by_peek = fatal_tripping_input("1 2 3 4", 2);
  assert_eq!(
    by_peek.as_ref().peek::<U3>().map(|w| w.len()),
    Err(ByValErr::Limit),
    "the fatal emit fails the fill itself, before any terminal flag is read"
  );
  let mut by_map = fatal_tripping_input("1 2 3 4", 2);
  assert_eq!(
    by_map.as_ref().peek_map::<U3, _, _>(|w| w.len()),
    Err(ByValErr::Limit),
    "so `peek_map` propagates the same value, and carries no terminal mark of its own"
  );

  // The genuinely short input is `Ok` on both here too — a fatal emitter changes nothing about the
  // half of the distinction that was never in question.
  let mut short = fatal_tripping_input("1 2", usize::MAX);
  let mut inp = short.as_ref();
  assert_eq!(inp.peek::<U3>().map(|w| w.len()), Ok(2));
  assert_eq!(inp.peek_map::<U3, _, _>(|w| w.len()), Ok(2));

  // WHAT IS DELIBERATELY NOT IN THIS CELL: an already-latched boundary. A fatal emitter is blind
  // only on the EMITTING path — at a latch the fill emits nothing and returns `Ok` with the flag
  // set, so `peek_map` raises there under a fatal emitter as readily as under `Verbose`. Putting
  // that case here would make this cell able to see the defect, which would defeat its purpose.
}

// ── The two things leaving the scanner had to re-establish: the latch, and the unwind ─────────

/// An input with no budget at all, for the unwind sweep below.
fn open_input(src: &str) -> Input<'_, BalLexer<'_>, BalCtx<'_>, ()> {
  Input::with_state_and_context(
    src,
    TokenLimiter::with_limitation(usize::MAX),
    crate::input::InputContext::new(
      Verbose::<ByValErr>::new(),
      DefaultCache::<'_, BalLexer<'_>>::default(),
    ),
  )
}

/// **Where a resource trip latches when it happens inside a trivia skip.**
///
/// Leaving the shared scanner gave up its deferred [`AtFrontier`](super::AtFrontier), which is
/// what a trip latches the poison boundary against. The complete-input route latches against
/// [`AtCursor`](super::AtCursor) — the committed cursor — instead, and the two are the same offset
/// only *because* that route commits every token it crosses as it crosses it. That is an
/// argument, so this measures the offset rather than trusting it, on both routes over the same
/// stream: `~ ~ 2` under a two-token budget, so the trip fires on the token the skip would have
/// stopped on and the durable frontier is the end of the second trivia token.
///
/// Run cold (the skip lexes the whole run) and with the first token already resident (so the
/// resident phase commits before the lexing phase latches), because those are the two ways the
/// committed cursor can arrive at the frontier.
#[test]
fn a_trip_inside_a_trivia_skip_latches_where_the_scan_latches() {
  /// `(latched boundary, committed span, cursor, limit diagnostics, what a later drain yields)`
  fn trip(
    via_scan: bool,
    prefill: usize,
  ) -> (
    Option<usize>,
    SimpleSpan,
    usize,
    usize,
    std::vec::Vec<SimpleSpan>,
  ) {
    let mut input = tripping_input("~ ~ 2", 2);
    let (latched, span, cursor, drained) = {
      let mut inp = input.as_ref();
      for _ in 0..prefill {
        let _ = inp.peek::<U1>().unwrap();
      }
      if via_scan {
        // `skip_while(is_trivia)` *is* `skip_until::<SkipWhile>(|t| !is_trivia(t))`.
        let _ = inp
          .skip_until::<SkipWhile, _, _>(|t| !t.data.is_trivia(), || None, ())
          .unwrap();
      } else {
        inp.skip_while(|t| t.data.is_trivia()).unwrap();
      }
      let latched = inp.latch_snapshot();
      let span = *inp.span();
      let cursor = *inp.cursor().as_inner();
      let mut drained = std::vec::Vec::new();
      while let Some(tok) = inp.next().unwrap() {
        drained.push(*tok.span_ref());
      }
      (latched, span, cursor, drained)
    };
    let limits = input
      .emitter()
      .errors()
      .values()
      .flatten()
      .filter(|e| **e == ByValErr::Limit)
      .count();
    (latched, span, cursor, limits, drained)
  }

  for prefill in [0usize, 1] {
    let direct = trip(false, prefill);
    let scanned = trip(true, prefill);
    assert_eq!(
      direct, scanned,
      "a trip inside a trivia skip must leave the same boundary, the same committed position, \
       the same cursor, the same diagnostics and the same remaining stream on both routes \
       (prefill {prefill})"
    );
    assert_eq!(
      direct,
      (
        Some(3),
        SimpleSpan::new(2, 3),
        3,
        1,
        std::vec::Vec::<SimpleSpan>::new()
      ),
      "and the boundary is the DURABLE FRONTIER — the end of the last skipped token, offset 3 — \
       not the cursor the call started from and not the tripping token's own span. The committed \
       span is that same token, the drain stops at the boundary, and the trip is diagnosed once \
       (prefill {prefill})"
    );
  }
}

/// **A panic mid-skip, and the state the input is left in** — the property the scan scope's
/// `Drop` provided, which this route provides by construction in one phase and with a guard of its
/// own in the other.
///
/// Under `Complete` the scope settles exactly two things: it puts the in-flight token back at the
/// front of the stream, and it commits the frontier the loop accumulated. This route accumulates
/// no uncommitted frontier, so the second has nothing to do. The first splits: the **resident**
/// run holds no token out of the stream while caller code runs, so it too has nothing to do, while
/// the **lexing** run holds each token it lexes across the predicate and owes the put-back — which
/// is why [`LexedFront`](super::skip_while) exists. This paragraph claimed the stronger, unsplit
/// version for five review rounds, and the defect it was hiding sat in the half it waved away.
///
/// The claim is checkable either way, and this checks it: a predicate that panics on its `k`-th
/// call must leave **exactly** the `k − 1` tokens it accepted consumed, and every other token
/// still reachable.
///
/// That is stronger than "the input looks consistent". A lost token would shorten the drain; a
/// token consumed in silence would shorten it too; an uncommitted prefix would leave the
/// committed span behind the tokens the predicate accepted. All three are read off the same two
/// values.
///
/// Swept over the cache depth, because the two phases of the loop reach a panicking predicate by
/// different roads: at depth 0 every token is lexed inside the call, at depth 3 the first three
/// are already resident and the panic can land on either side of the phase boundary, and at 1 and
/// 2 it lands earlier.
///
/// The depths are **widths**, and the residency each one leaves is measured. This sweep used to
/// read `for _ in 0..prefill { peek::<U3>() }` over `[0, 1, 2, 5]` and call that four depths: the
/// first of those peeks saturates the `U3` cache, so 1, 2 and 5 were one state and the claim that
/// depth 5 left "every token already resident" was false twice over — the source has five tokens
/// and the cache holds three.
#[test]
fn a_panic_mid_skip_consumes_exactly_what_the_predicate_accepted() {
  // Five tokens, with a gap between each: `(0,1) (2,3) (4,5)` trivia, then `(6,7)` and `(8,9)`.
  // The gaps matter — a cursor placed at the previous token's END rather than at the next one's
  // START is a different number here, and both are checked below.
  const SRC: &str = "~ ~ ~ 2 ;";

  let all: std::vec::Vec<SimpleSpan> = {
    let mut input = open_input(SRC);
    let mut inp = input.as_ref();
    let mut out = std::vec::Vec::new();
    while let Some(tok) = inp.next().unwrap() {
      out.push(*tok.span_ref());
    }
    out
  };
  assert_eq!(all.len(), 5, "the fixture's own shape");

  let mut depths_reached = [false; 4];
  for prefill in [0usize, 1, 2, 3] {
    for panic_on in 1..=4usize {
      let calls = Cell::new(0usize);
      let mut input = open_input(SRC);
      let mut inp = input.as_ref();
      let depth = prefill_to_width(&mut inp, prefill);
      assert_eq!(
        depth, prefill,
        "the fixture has five tokens and the cache holds three, so a width of {prefill} must \
         leave exactly {prefill} resident"
      );
      depths_reached[depth] = true;
      let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        inp.skip_while(|t| {
          calls.set(calls.get() + 1);
          assert!(
            calls.get() != panic_on,
            "the armed predicate call panics mid-skip"
          );
          t.data.is_trivia()
        })
      }));
      assert!(
        caught.is_err(),
        "call {panic_on} of the predicate must have panicked (prefill {prefill})"
      );

      let committed_end = inp.span().end();
      let cursor = *inp.cursor().as_inner();
      let mut drained = std::vec::Vec::new();
      while let Some(tok) = inp.next().unwrap() {
        drained.push(*tok.span_ref());
      }

      // The predicate accepted calls 1..panic_on-1 — every one of them trivia — and never
      // answered about the token of call `panic_on`. So exactly that many tokens are consumed.
      assert_eq!(
        drained,
        all[panic_on - 1..].to_vec(),
        "a panic on predicate call {panic_on} must leave the {} token(s) it accepted consumed \
         and every other token still reachable — no token lost to the unwind, and none consumed \
         in silence (prefill {prefill})",
        panic_on - 1
      );
      // …and the position accounts for them: the interrupted call kept its prefix, at the end of
      // the last token the predicate accepted, exactly as a committing mode's unwind edge does.
      let expected_end = if panic_on == 1 {
        0
      } else {
        all[panic_on - 2].end()
      };
      assert_eq!(
        committed_end, expected_end,
        "the committed position after the unwind is the end of the last accepted token \
         (prefill {prefill}, panic on {panic_on})"
      );
      // …and the stream resumes AT the token the predicate never answered about — not merely
      // somewhere between it and the committed position.
      //
      // This used to be the range `committed_end <= cursor && cursor <= start`, and with the gaps
      // this fixture has on purpose that range holds two answers: the token PUT BACK (cursor at
      // its own start) and the token DROPPED (cursor at the committed end, the token re-lexed by
      // the next read). Both leave the drain above identical, because a re-lex reproduces the
      // token exactly — so a range here was the whole of what a real divergence had to clear. It
      // is an equality now, and the two routes are compared against each other, field by field, in
      // `the_two_completeness_routes_observe_the_same_unwound_skip`.
      assert_eq!(
        cursor,
        all[panic_on - 1].start(),
        "the cursor must be the START of the token the predicate never answered about — that \
         token is unconsumed, and an unconsumed token lives at the front of the stream whether \
         this call found it there or lexed it a moment ago. The committed position is \
         {committed_end} (prefill {prefill}, panic on {panic_on})"
      );
    }
  }
  assert_eq!(
    depths_reached, [true; 4],
    "the sweep must actually reach all four cache depths"
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
  /// `L::State::drop`. A **wide** probe — see [`Wide`].
  StateDrop,
  /// `L::Offset::drop`, which is also how an `L::Span::drop` shows up: this span owns two offsets
  /// and the `Span` impl moves them out by value, so the span itself cannot carry a `Drop` impl.
  /// A **wide** probe.
  OffsetDrop,
  /// `L::Offset`'s `Ord`/`PartialOrd` — the trait bound asks for it and the input layer uses it.
  /// A **wide** probe.
  OffsetCmp,
  /// [`Source::len`](crate::Source::len). A **wide** probe.
  SourceLen,
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
  /// While set, the **wide** probes record too — see [`Wide`].
  static WIDE: Cell<bool> = const { Cell::new(false) };
  /// Every token [`LedgerLexer::lex`] has produced, counted whether or not the ledger is
  /// recording — the one observation that distinguishes a token SERVED from the stream from the
  /// same token produced a second time. Nothing else can: a re-lex from the committed position
  /// reproduces the token, its span and its post-token state exactly, so the values a caller reads
  /// back are identical either way and only the work differs.
  static TOKENS_LEXED: Cell<usize> = const { Cell::new(0) };
}

fn reset_tokens_lexed() {
  TOKENS_LEXED.with(|c| c.set(0));
}

fn tokens_lexed() -> usize {
  TOKENS_LEXED.with(Cell::get)
}

fn note(effect: Effect) {
  if RECORDING.with(Cell::get) {
    LEDGER.with(|l| l.borrow_mut().push(effect));
  }
}

/// The wide probes on, for as long as this guard lives.
///
/// The ledger has two settings, and the reason is the contract's own history. The **narrow**
/// setting — the default, and what every cell above uses — records the caller steps whose *order*
/// is the finding: the clones, the marks, the cache calls, the caller's own closure. Those
/// sequences are asserted whole, which is what makes them bite in both directions.
///
/// The **wide** setting adds the operations that are caller-supplied too but that no reading of
/// this contract had thought to name: the `Drop` that goes with each elided clone, the
/// `Source::len` and the `L::Offset` comparison the scan's closing clamp runs. Turning those on
/// permanently would mean editing every whole-sequence assertion above each time a further one is
/// found — which is exactly the list-maintenance the contract stopped doing when it replaced its
/// enumeration of exclusions with a condition. So they are gated, and the cell that uses them
/// asserts what the *clause* actually says: that this route's side is **empty**, whatever the
/// probe set happens to contain. A new probe added here strengthens that cell without touching
/// any other.
struct Wide;

impl Wide {
  fn on() -> Self {
    WIDE.with(|c| c.set(true));
    Self
  }
}

impl Drop for Wide {
  fn drop(&mut self) {
    WIDE.with(|c| c.set(false));
  }
}

/// Records `effect` only while a [`Wide`] guard is live.
fn note_wide(effect: Effect) {
  if WIDE.with(Cell::get) {
    note(effect);
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

// ── The two ways the fixture is made to FAIL ──────────────────────────────────────────────────
//
// Everything above runs over a lexer that cannot fail: `LedgerLexer::lex` yielded ordinary tokens
// and end of input, and its `check()` returned `Ok(())` unconditionally, so no cell could reach a
// lexer error, a limit trip, or a rejecting emitter. Those are exactly the conditions the
// complete-input route's unwind and latch arguments are about, so the drift guard at the foot of
// this file needs them.
//
// Both faults are ADDITIVE and both are off by default. A `!`-run is a lexer error and no source
// outside the fault sweep contains one; the trip needs a live [`Budget`] guard, without which
// `check()` reads one thread-local and answers `Ok`. `LedgerState`'s own
// [`LedgerNeverTrips`](LedgerNeverTrips) is left exactly as it was — it is not the switch anyway,
// since the input layer's terminal predicate asks the LEXER.

thread_local! {
  /// While `Some(n)`, [`LedgerLexer::check`] reports a trip once the lexer's tally has passed `n`
  /// tokens — the fixture's [`TokenLimiter`](crate::state::token_tracker::TokenLimiter).
  ///
  /// The budget lives beside the state rather than in it because the tally it is compared against
  /// (`LedgerState::scanned`) is the state's, and adding a second field to `LedgerState` would
  /// change a type six cells above measure clones and drops of.
  static LEDGER_BUDGET: Cell<Option<usize>> = const { Cell::new(None) };
}

/// A token budget in force for as long as this guard lives — the [`Wide`] pattern, for the lexer's
/// terminal predicate.
struct Budget;

impl Budget {
  fn of(tokens: usize) -> Self {
    LEDGER_BUDGET.with(|c| c.set(Some(tokens)));
    Self
  }

  /// No budget at all, so the lexer cannot trip. Returned as a guard rather than as nothing, so a
  /// caller sweeping over `Option<usize>` holds one value of one type either way.
  fn unlimited() -> Self {
    LEDGER_BUDGET.with(|c| c.set(None));
    Self
  }
}

impl Drop for Budget {
  fn drop(&mut self) {
    LEDGER_BUDGET.with(|c| c.set(None));
  }
}

// ── The instrumented lexer ────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, PartialEq, Eq, Hash)]
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

/// A wide probe: `L::Offset: Ord` is a bound the caller satisfies, so every comparison the input
/// layer makes on an offset is a caller call. `Ord` is written out rather than derived for that
/// reason alone.
impl Ord for LedgerOffset {
  fn cmp(&self, other: &Self) -> core::cmp::Ordering {
    note_wide(Effect::OffsetCmp);
    self.0.cmp(&other.0)
  }
}

impl PartialOrd for LedgerOffset {
  fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
    Some(self.cmp(other))
  }
}

/// A wide probe. Every clone this offset takes is a value that must later die, and the route that
/// does not take the clone does not run this either.
impl Drop for LedgerOffset {
  fn drop(&mut self) {
    note_wide(Effect::OffsetDrop);
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

  /// A wide probe: the clamp inside `commit_position` asks the source how long it is, and a
  /// `Source` is the caller's.
  fn len(&self) -> LedgerOffset {
    note_wide(Effect::SourceLen);
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

/// A wide probe — the other half of a frontier clone the fast path never takes.
impl Drop for LedgerState {
  fn drop(&mut self) {
    note_wide(Effect::StateDrop);
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
  /// A **terminal** limit trip, as distinct from an ordinary lexer error. Only
  /// [`LedgerLexer::check`] produces it, and only while a [`Budget`] guard is live.
  Limit,
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

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

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

  /// The input layer's **terminal predicate**, and the only one it has:
  /// [`latch_if_limit_tripped`](InputRef::latch_if_limit_tripped) asks the *lexer*, never the
  /// [`State`]. `LedgerState::check` is therefore not the trip switch its name suggests, and
  /// leaving it [`LedgerNeverTrips`] is what keeps every cell above lexing exactly as it did:
  /// with no [`Budget`] guard live this reads one thread-local and answers `Ok`.
  fn check(&self) -> Result<(), LedgerErr> {
    match LEDGER_BUDGET.with(Cell::get) {
      Some(budget) if self.state.scanned > budget => Err(LedgerErr::Limit),
      _ => Ok(()),
    }
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
    let text = &bytes[self.start..self.end];

    // A run of `!` is a NON-TERMINAL lexer error: it bumps no tally, `check()` stays `Ok`, so
    // nothing latches and the scan crosses it looking for the next token. No source in this file
    // outside the fault sweep contains one.
    if text.iter().all(|b| *b == b'!') {
      return Some(Err(LedgerErr::Lex));
    }

    self.state.scanned += 1;
    // Beside the tally, and for a different reader: `state.scanned` is COMMITTED state, restored
    // by a rollback and re-reached by a re-lex, so it cannot say how much lexing happened. This
    // counter is monotone for the whole test and says exactly that.
    TOKENS_LEXED.with(|c| c.set(c.get() + 1));
    // A TRIP REPLACES THE TOKEN IT FIRED ON, which is the shape `classify` is written against: the
    // terminal condition reaches the input layer as a `Lexed::Error` carrying the tripping token's
    // span, and `check()` — asked next — agrees that the lexer has tripped. Inert unless a
    // [`Budget`] guard is live.
    if let Err(tripped) = crate::Lexer::check(self) {
      return Some(Err(tripped));
    }

    let trivia = text.iter().all(|b| *b == b'~');
    Some(Ok(if trivia {
      LedgerTok::Trivia
    } else {
      LedgerTok::Word
    }))
  }

  fn read_frontier(&self) -> crate::ReadFrontier<LedgerOffset> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &LedgerOffset) {
    self.end += n.0;
  }
}

// ── The instrumented emitter ──────────────────────────────────────────────────────────────────

/// One lexer diagnostic the emitter was offered: what it was, where, and whether it was taken.
///
/// The span is recorded as its `Debug` form because that is the only shape the
/// [`Emitter`](crate::Emitter) impl below can read one in: it is generic over `L`, so `L::Span` is
/// known only to be [`Debug`](core::fmt::Debug) and there is no way to pull a number out of a
/// caller offset. It compares across the two routes, which is what the record is for.
#[derive(Debug, Clone, PartialEq)]
struct Diagnostic {
  class: LedgerErr,
  at: std::string::String,
  accepted: bool,
}

#[derive(Debug, Default)]
struct LedgerEmitter {
  next: Cell<u64>,
  live: RefCell<std::vec::Vec<u64>>,
  emissions: usize,
  /// Every lexer diagnostic offered, in order — the observation a dedup watermark governs.
  offered: std::vec::Vec<Diagnostic>,
  /// `(unexpected-token reports, other errors)`. `SkipWhile` reports nothing, so both stay 0;
  /// they are recorded so that a route which *started* reporting is red rather than invisible.
  reports: (usize, usize),
  /// While `Some(n)`, the `n`-th lexer diagnostic and every later one is **rejected** — the fatal
  /// emitter the `skip_while` contract names ("a fatal emitter can abort on a malformed token").
  reject_from: Option<usize>,
}

impl LedgerEmitter {
  /// An emitter that rejects the `nth` lexer diagnostic it is offered, and every one after it.
  fn rejecting_from(nth: usize) -> Self {
    Self {
      reject_from: Some(nth),
      ..Self::default()
    }
  }

  fn live_rows(&self) -> usize {
    self.live.borrow().len()
  }

  fn offered(&self) -> &[Diagnostic] {
    &self.offered
  }

  fn reports(&self) -> (usize, usize) {
    self.reports
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
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), LedgerErr> {
    self.emissions += 1;
    let (span, err) = err.into_components();
    let class: LedgerErr = err.into();
    let accepted = !self
      .reject_from
      .is_some_and(|nth| self.offered.len() + 1 >= nth);
    self.offered.push(Diagnostic {
      class: class.clone(),
      at: std::format!("{span:?}"),
      accepted,
    });
    if accepted { Ok(()) } else { Err(class) }
  }

  fn emit_unexpected_token(
    &mut self,
    _err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), LedgerErr> {
    self.emissions += 1;
    self.reports.0 += 1;
    Ok(())
  }

  fn emit_error(&mut self, _err: Spanned<LedgerErr, L::Span>) -> Result<(), LedgerErr> {
    self.emissions += 1;
    self.reports.1 += 1;
    Ok(())
  }

  fn commit_token(&mut self, _tok: &L::Token, _span: &L::Span) {
    note(Effect::CommitToken);
  }

  fn checkpoint(&mut self) -> u64 {
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
  ledger_input_with(src, LedgerEmitter::default())
}

fn ledger_partial_input<'a>(
  src: &'a LedgerSrc<'a>,
) -> Input<'a, LedgerLexer<'a>, LedgerCtx<'a>, (), Partial> {
  ledger_partial_input_with(src, LedgerEmitter::default())
}

/// The same input under an emitter and a cache capacity the caller chose — a rejecting emitter for
/// the fatal-emit cells, and the capacity axis for the drift sweep. Both default back to
/// [`LedgerCtx`] by inference wherever the return type names it.
fn ledger_input_with<'a, C>(
  src: &'a LedgerSrc<'a>,
  emitter: LedgerEmitter,
) -> Input<'a, LedgerLexer<'a>, (LedgerEmitter, C), ()>
where
  C: Cache<'a, LedgerLexer<'a>, ()> + Default,
{
  Input::with_state_and_context(
    src,
    LedgerState::default(),
    InputContext::new(emitter, C::default()),
  )
}

fn ledger_partial_input_with<'a, C>(
  src: &'a LedgerSrc<'a>,
  emitter: LedgerEmitter,
) -> Input<'a, LedgerLexer<'a>, (LedgerEmitter, C), (), Partial>
where
  C: Cache<'a, LedgerLexer<'a>, ()> + Default,
{
  Input::with_state_and_context(
    src,
    LedgerState::default(),
    InputContext::new(emitter, C::default()),
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
//   1. every caller-supplied operation the input layer can reach is INERT: it does only what its
//      own contract says it does, always returns, does not unwind, does not diverge. ALL of it —
//      the clause is a quantifier, not a set. The ones a caller is likely to have written are the
//      `Clone`, `Drop`, `Ord` and `Hash` of `L::Span` and `L::Offset`, the `Clone` and `Drop` of
//      `L::State`, the `Emitter` methods these paths reach, every `Cache` method, and the `Lexer`
//      with its `Source`; they are named for legibility and are explicitly NOT the boundary;
//   2. the caller's predicate or closure is a FUNCTION OF WHAT IT IS HANDED: it answers from the
//      *values* of the `Spanned<&L::Token, &L::Span>` it receives — not from state another
//      callback wrote, and not from the addresses those two references carry.
//
// The cells below are those two clauses made concrete: each builds a caller that fails one of
// them and shows that the failure is not a call count but a **different parse** — a different
// skip decision, a different committed cursor, different tokens consumed, a different returned
// value, or a different number of predicate calls surviving an unwind. They are instances, not an
// enumeration: a caller who breaks a clause in a way none of them describes is outside the
// guarantee just the same, and adding a cell here never narrows the clause.
//
// Clause 2 fails in the clone-counting, offset-counting and address-reading cells; clause 1 fails
// in the two unwinding ones and in the elided-drop one. The unwinding pair matters most, because
// their callers satisfy clause 2 in full: a predicate that records only its own calls observes no
// input-layer side effect at all, and still cannot be promised the same call sequence once a
// caller `Clone` panics.
//
// They exist so that the condition is a measured fact rather than a caveat nobody checked, and
// they pin a **documented precondition, not a behaviour**. No caller may rely on either column:
// which route answers a given call is this crate's choice and may change between versions, so the
// only thing a caller can do with the difference below is meet the condition. `Clone` is not
// required to be pure, total or non-unwinding in Rust, and `L::Span`, `L::State` and `L::Offset`
// are the caller's own types, which is exactly why the condition has to be written down instead
// of assumed.
//
// All of them are red against a build that runs the slow route: ablate either fast path and its
// two columns collapse onto each other, and the assertion that carries the finding fails.

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

// ── Clause 1 again, on the operations no list of clone counts reaches ──────────────────────────

/// How many of `effect` a ledger holds.
fn count_of(ledger: &[Effect], effect: Effect) -> usize {
  ledger.iter().filter(|e| **e == effect).count()
}

/// **Clause 1, on `Drop`** — and, in the same measurement, the demonstration that clause 1 is a
/// quantifier rather than a table.
///
/// A clone this route does not take is a value it never produces, and therefore one it never
/// drops. `Drop` is caller code exactly as `Clone` is, and no enumeration of this contract's
/// exclusions ever listed it: it was found by working out what a *condition* would have to say,
/// not by a fifth reading of the differences.
///
/// So the assertion is shaped the way the clause is. With the [wide](Wide) probes on — the `Drop`
/// of `L::State` and `L::Offset`, the [`Source::len`](crate::Source::len) and the `L::Offset`
/// comparison the scan's closing clamp runs — this route's caller-code ledger is **still exactly
/// `[CacheFront, Caller]`**, byte for byte what it is with the probes off. It is not that this
/// route runs few of the operations somebody thought to name; it is that it runs *none of them*.
/// The scan, over the same stream in the same residency:
///
/// | one no-op skip | this route | the scan | under `Partial` |
/// |---|---:|---:|---|
/// | `L::State::drop` | 0 | 1 | 0 against 2 |
/// | `L::Offset::drop` | 0 | 3 | 0 against 7 |
/// | `Source::len` | 0 | 1 | 0 against 1 |
/// | `L::Offset` compare | 0 | 1 | 0 against 1 |
///
/// The last two rows are the point of the whole exercise: they are caller-supplied operations that
/// the measured table on [`skip_while`](InputRef::skip_while) does not carry and that no round of
/// review had named, and the contract needed no amendment to cover them, because it stopped
/// naming a set.
///
/// This is a **documented precondition, not a supported behaviour.** Neither column is a promise.
/// Red against a build that runs the slow route: ablate the probe and the left column becomes the
/// right one — the whole-ledger assertion fails first, and every zero after it.
#[test]
fn a_no_op_skip_runs_no_caller_drop_where_the_scan_runs_one_per_frontier_clone() {
  let (fast, scan, fast_partial, scan_partial) = {
    let _wide = Wide::on();
    (
      noop_skip_ledger(),
      general_scan_ledger(),
      noop_skip_ledger_partial().0,
      general_scan_ledger_partial().0,
    )
  };

  // ── The emptiness, which is what the clause actually claims ──
  assert_eq!(
    fast,
    std::vec![Effect::CacheFront, Effect::Caller],
    "with every probe this fixture has switched on, the fast path's caller-code ledger is \
     unchanged: one `Cache::front`, one predicate call. Nothing was hiding behind the narrow \
     probe set"
  );
  assert_eq!(
    fast_partial,
    std::vec![Effect::CacheFront, Effect::Caller],
    "and the same under `Partial`"
  );

  // ── The drops, which are what a list of clone counts could not have reached ──
  for (label, ledger, states, offsets) in [
    ("the scan", &scan, 1, 3),
    ("the scan under `Partial`", &scan_partial, 2, 7),
  ] {
    assert_eq!(
      count_of(ledger, Effect::StateDrop),
      states,
      "{label} clones the frontier state and later drops it — caller code both times. Got \
       {ledger:?}"
    );
    assert_eq!(
      count_of(ledger, Effect::OffsetDrop),
      offsets,
      "{label} drops the offsets those clones own. This span moves its offsets out by value, so \
       an `L::Span::drop` reaches the ledger as its offsets' drops. Got {ledger:?}"
    );
  }
  for (label, ledger) in [("", &fast), (" under `Partial`", &fast_partial)] {
    assert_eq!(
      (
        count_of(ledger, Effect::StateDrop),
        count_of(ledger, Effect::OffsetDrop)
      ),
      (0, 0),
      "a value this route never clones is a value it never drops{label}"
    );
  }
  assert_ne!(
    count_of(&scan, Effect::StateDrop),
    count_of(&fast, Effect::StateDrop),
    "one `L::State::drop` against none — the finding, in the operation the enumeration form never \
     had an entry for"
  );

  // ── And the two the measured table on `skip_while` does not carry, for the same reason ──
  for (label, ledger) in [
    ("the scan", &scan),
    ("the scan under `Partial`", &scan_partial),
  ] {
    assert_eq!(
      (
        count_of(ledger, Effect::SourceLen),
        count_of(ledger, Effect::OffsetCmp)
      ),
      (1, 1),
      "{label} finishes through `commit_at` -> `commit_position` -> the clamp, which asks the \
       caller's `Source` how long it is and compares the caller's `L::Offset` against the answer. \
       Both are caller code; neither appears in any table on these methods. Got {ledger:?}"
    );
  }
  for (label, ledger) in [("", &fast), (" under `Partial`", &fast_partial)] {
    assert_eq!(
      (
        count_of(ledger, Effect::SourceLen),
        count_of(ledger, Effect::OffsetCmp)
      ),
      (0, 0),
      "this route commits nothing, so it runs neither{label} — which the condition covers without \
       ever having named them"
    );
  }
}

// ── Clause 2 again, in the half that says *values* and not addresses ───────────────────────────

thread_local! {
  /// The address of the token the cache holds at its front when a call begins.
  static CACHE_FRONT_ADDR: Cell<usize> = const { Cell::new(0) };
  /// The address of each token the predicate was handed, in order.
  static HANDED_ADDRS: RefCell<std::vec::Vec<usize>> = const { RefCell::new(std::vec::Vec::new()) };
}

fn addr_of(tok: &LedgerTok) -> usize {
  core::ptr::from_ref(tok) as usize
}

/// "Skip this token if it is the very entry that is still sitting in the cache."
///
/// Nothing about the token's **value** is read — not its kind, not its span, not its text. The
/// predicate answers purely from *where* its argument lives, which is the one thing clause 2
/// forbids and the reason that clause says *values* rather than merely *pure*.
fn skip_the_token_that_is_still_in_the_cache(t: Spanned<&LedgerTok, &LedgerSpan>) -> bool {
  let handed = addr_of(t.data);
  HANDED_ADDRS.with(|a| a.borrow_mut().push(handed));
  handed == CACHE_FRONT_ADDR.with(Cell::get)
}

/// What one route did: where it left the stream, and the addresses its predicate saw.
fn skip_while_asking_about_addresses(via_scan: bool) -> (Left, usize, std::vec::Vec<usize>) {
  let src = LedgerSrc("ab cd ef");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();

  // Read before the ledger opens, so it costs no recorded step: the address of the entry the
  // cache is holding at its front, which is what the probe hands over and what the scan moves.
  let front = addr_of(inp.front().expect("the head is resident").token.data);
  CACHE_FRONT_ADDR.with(|c| c.set(front));
  HANDED_ADDRS.with(|a| a.borrow_mut().clear());

  record();
  if via_scan {
    // `skip_while(pred)` *is* `skip_until::<SkipWhile>(|t| !pred(t))` — the same predicate over
    // the same stream in the same residency, entered the other way.
    let _ = inp
      .skip_until::<SkipWhile, _, _>(
        |t| !skip_the_token_that_is_still_in_the_cache(t),
        || None,
        (),
      )
      .unwrap();
  } else {
    inp
      .skip_while(skip_the_token_that_is_still_in_the_cache)
      .unwrap();
  }
  let _ = recorded();

  let handed = HANDED_ADDRS.with(|a| a.borrow().clone());
  let cursor = inp.cursor().as_inner().0;
  let remaining = drain_spans(&mut inp);
  (Left { cursor, remaining }, front, handed)
}

/// **Clause 2, on the address rather than the value.** A predicate that reads *where* its argument
/// lives — and nothing else about it — makes the two routes skip different tokens.
///
/// This route hands `pred` the cache entry **where it lies**. The scan pops the head out of the
/// cache into the scan scope and asks about it there, putting it back afterwards. Same token, same
/// value, same span; two addresses. A predicate that compares the address it was handed against
/// the cache's own front entry therefore answers `true` here and `false` there — and the answer to
/// a skip predicate is the skip:
///
/// | resident head | this route | the scan |
/// |---|---|---|
/// | is my argument the entry still in the cache? | yes — skip it | no — stop |
/// | tokens consumed | **1** | **0** |
///
/// This class was never reported by any review of the fast paths, and it is not even new to them:
/// the cache has always been an invisible optimization, and how deep a caller peeked has always
/// decided whether a token is read in place or copied. That is precisely why it belongs in a
/// condition on the caller and not in a list of one route's differences — the positive form covers
/// it without an amendment, which is what the positive form is for.
///
/// This is a **documented precondition, not a supported behaviour.** Neither column is a promise.
/// Red against a build that runs the slow route: ablate the probe and this route reads the moved
/// token too, both columns become the scan's, and the `assert_ne!` fails.
#[test]
fn an_address_reading_predicate_can_change_the_skip_decision() {
  let (fast, fast_front, fast_handed) = skip_while_asking_about_addresses(false);
  let (scan, scan_front, scan_handed) = skip_while_asking_about_addresses(true);

  // ── The mechanism, before the consequence: where each route's predicate was pointed ──
  assert_eq!(
    fast_handed.first().copied(),
    Some(fast_front),
    "this route asks about the head where it lies, so the first address the predicate sees IS \
     the cache's front entry"
  );
  assert_ne!(
    scan_handed.first().copied(),
    Some(scan_front),
    "the scan asks about a token it has moved out of the cache and into the scan scope, so the \
     same predicate is pointed somewhere else"
  );

  // ── The consequence: a different skip, from a predicate that read no value at all ──
  assert_eq!(
    fast,
    Left {
      cursor: 3,
      remaining: std::vec![(3, 5), (6, 8)],
    },
    "the head is the cache entry, so the predicate skips it; the next token is somewhere else \
     again, so the skip stops there — one token consumed"
  );
  assert_eq!(
    scan,
    Left {
      cursor: 0,
      remaining: std::vec![(0, 2), (3, 5), (6, 8)],
    },
    "the scan's first token is already out of the cache when the predicate sees it, so the same \
     predicate stops immediately — nothing consumed"
  );
  assert_ne!(
    fast, scan,
    "one token consumed against none, from a predicate that never looked at a kind, a span or a \
     byte. `pred` is a function of what it is handed means the VALUES it is handed, and this is \
     the cell that makes the word load-bearing"
  );
}

// ── The clamp's ordering, measured on the one operation that can unwind inside it ─────────────

/// **The settle's fallible step runs while the token is still in the stream — in the RESIDENT
/// run** — measured, not argued.
///
/// The qualifier is load-bearing and belongs in the lead rather than in a clause below it. It is
/// the resident run that clamps before the removal; the lexing run has no token in the stream to
/// clamp against and settles through the ordinary [`commit_token`](InputRef::commit_token), whose
/// clamp runs in the window this cell is about. That window is the crate's standing posture for
/// every 1:1 consume and is not what this cell measures.
///
/// The resident run's per-token settle is a clamp
/// ([`Source::len`](crate::Source::len), an `L::Offset` comparison and an `L::Span::clone`)
/// followed by a removal and two moves, in that order, and the order is the whole of why that
/// phase needs no scan scope. Reverse it — clamp inside the settle, as
/// [`commit_token`](InputRef::commit_token) does for the 1:1 consume paths — and a panicking
/// `L::Span::clone` leaves the token popped, the position behind it, and the younger cache
/// entries resident in front of the hole.
///
/// So the armed clone is fired on a head the predicate **accepts**, and what is asserted is the
/// stream: the token is still there, the cursor has not moved, and a later drain yields every
/// token including the one the clamp was clamping. The ledger beside it says the unwind happened
/// where it was aimed — probe, predicate, then the clone — and not somewhere earlier.
///
/// This is not a claim about a supported behaviour: an unwinding `Clone` breaks the inertness
/// clause, and the cells above measure what that costs a caller. It is a claim about the *crate's*
/// side of the bargain — that the input this route leaves behind is whole even then, because
/// nothing is ever half-removed.
#[test]
fn an_unwinding_clamp_leaves_the_token_it_was_clamping_in_the_stream() {
  let src = LedgerSrc("~ ab cd");
  let mut input = ledger_input(&src);
  let mut inp = input.as_ref();
  let _ = inp.peek::<U3>().unwrap();

  record();
  arm_bombs();
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    inp.skip_while(|t| {
      note(Effect::Caller);
      t.data.is_trivia()
    })
  }));
  disarm_bombs();
  let ledger = recorded();

  assert!(caught.is_err() && bomb_fired(), "the armed clone panicked");
  assert_eq!(
    ledger,
    std::vec![Effect::CacheFront, Effect::Caller, Effect::SpanClone],
    "the clamp is the third step and the removal has not happened yet: probe, predicate, then \
     the clone that unwinds"
  );
  assert_eq!(
    inp.cursor().as_inner().0,
    0,
    "the cursor has not moved — the token the clamp was clamping is still the head"
  );
  assert_eq!(
    crate::Span::end(inp.span()).0,
    0,
    "and nothing was committed"
  );
  assert_eq!(
    drain_spans(&mut inp),
    std::vec![(0, 1), (2, 4), (5, 7)],
    "every token is still reachable, the accepted trivia token included: an unwinding clamp \
     cannot lose one, because the token has not left the stream when it runs"
  );
}

// ── The typestate split, and the drift it could hide ──────────────────────────────────────────

/// One fixture the drift guard sweeps: a source, and the faults the lexer and the emitter are
/// armed with over it.
///
/// The faults exist because the routes' own arguments are about them. The complete-input route
/// gave up the scan scope on the strength of two claims — that a trip latches at the same offset
/// there as the scanner's deferred frontier latches it, and that nothing is ever half-removed when
/// caller code unwinds — and a sweep over a lexer that cannot fail never reaches the first one at
/// all.
#[derive(Debug, Clone, Copy)]
struct Fixture {
  text: &'static str,
  /// A token budget: the lexer trips on the `budget + 1`-th token it produces, and the trip
  /// **replaces** that token, exactly as the Logos backend reports one.
  budget: Option<usize>,
  /// `Some(n)`: the emitter rejects the `n`-th lexer diagnostic it is offered, and every later
  /// one — the fatal emitter [`skip_while`](InputRef::skip_while) documents.
  reject_from: Option<usize>,
}

impl Fixture {
  const fn clean(text: &'static str) -> Self {
    Self {
      text,
      budget: None,
      reject_from: None,
    }
  }

  const fn tripping(text: &'static str, budget: usize) -> Self {
    Self {
      text,
      budget: Some(budget),
      reject_from: None,
    }
  }

  const fn fatal(text: &'static str, reject_from: usize) -> Self {
    Self {
      text,
      budget: None,
      reject_from: Some(reject_from),
    }
  }

  const fn tripping_and_fatal(text: &'static str, budget: usize, reject_from: usize) -> Self {
    Self {
      text,
      budget: Some(budget),
      reject_from: Some(reject_from),
    }
  }

  fn emitter(self) -> LedgerEmitter {
    match self.reject_from {
      Some(nth) => LedgerEmitter::rejecting_from(nth),
      None => LedgerEmitter::default(),
    }
  }

  /// A fixture's identity — its source text and constructor arguments — as distinct from its
  /// *effects* on a sweep's tallies. Two different fixtures can land on the same effects; identity
  /// is the part a replacement fixture cannot also reproduce.
  fn identity(self) -> (&'static str, Option<usize>, Option<usize>) {
    (self.text, self.budget, self.reject_from)
  }
}

/// What one `skip_while` sweep observed, on an input of either completeness.
///
/// Deliberately not the effect ledger: the two routes run different caller code by construction,
/// and what has to agree is the *observation*. Every field here is a fact about the token stream,
/// the diagnostics, or the emitter's own condition — the set
/// [`skip_while`](InputRef::skip_while) promises identical, and the whole of it, because a
/// divergence in any one of them is a divergence. The two the fault fixtures add are the ones the
/// route's own argument turns on: the **latch** (where a trip poisons the input) and the **dedup
/// watermark** (which decides whether a crossed lexer error is diagnosed twice).
#[derive(Debug, PartialEq)]
struct SkipRun {
  /// What the prefill peek returned, and the residency it **actually achieved** — see
  /// [`prefill_to`] for why the second is measured rather than assumed.
  peeked: Result<usize, LedgerErr>,
  depth: usize,
  parked: bool,
  /// The tokens the predicate was asked about, in order, with the answer it gave.
  asked: std::vec::Vec<(usize, usize, bool)>,
  /// What each skip returned. A rejecting emitter puts an `Err` here.
  returned: std::vec::Vec<Result<(), LedgerErr>>,
  /// The resume cursor after the program: the stopping token's start.
  cursor: usize,
  /// The committed span after the program.
  span: (usize, usize),
  /// The committed lexer state — the token tally, which is the fact a trip is keyed on, and the
  /// half of the position that a torn commit would leave describing a different token than
  /// `span` does.
  scanned: usize,
  is_exhausted: bool,
  /// The poison boundary a trip latched, if one did.
  latch: Option<usize>,
  /// The lexer-error dedup watermark.
  dedup: usize,
  /// The tokens a full drain then yields — the "a caller retries after the call" law — and how
  /// that drain ended.
  drained: std::vec::Vec<(usize, usize)>,
  drain_end: Result<(), LedgerErr>,
  /// Every lexer diagnostic the emitter was offered, in order, with whether it took it.
  diagnostics: std::vec::Vec<Diagnostic>,
  /// `(unexpected-token reports, other errors)`. A trivia skip reports nothing on either route;
  /// recorded so that a route which *started* reporting is red rather than invisible.
  reports: (usize, usize),
  /// Emitter marks still outstanding: a route that took one and forgot it is red here.
  live_rows: usize,
}

impl SkipRun {
  /// The half of the observation that has to be read once the handle is gone.
  fn record_emitter(&mut self, emitter: &LedgerEmitter) {
    self.diagnostics = emitter.offered().to_vec();
    self.reports = emitter.reports();
    self.live_rows = emitter.live_rows();
  }
}

/// The prefill, at a **real** cache depth.
///
/// [`DefaultCache`] is `U3`, so ONE `peek::<U3>` saturates it. The sweep this replaces issued that
/// same peek `prefill` times and called the result four depths; depths 1, 2 and 3 left the
/// identical residency, so it swept two states and reported four. A depth has to come from the
/// **width** of the window, and it is then MEASURED — `SkipRun::depth` is what the cache actually
/// holds afterwards, not how many peeks were issued, and it is compared across the routes like
/// every other field.
///
/// A short source, a lexer error inside the window (an error is not a token and takes no slot) or
/// a trip mid-fill all leave a depth **below** the width asked for. That is the honest number, and
/// it is the one recorded; the sweeps below assert which depths were reached rather than assuming
/// the width was met.
fn prefill_to<'a, C, Cmpl>(
  inp: &mut InputRef<'a, '_, LedgerLexer<'a>, (LedgerEmitter, C), (), Cmpl>,
  width: usize,
) -> Result<usize, LedgerErr>
where
  C: Cache<'a, LedgerLexer<'a>, ()>,
  Cmpl: crate::input::SurfaceIncomplete<'a, LedgerLexer<'a>, (LedgerEmitter, C), ()>,
{
  match width {
    0 => Ok(0),
    1 => inp.peek::<U1>().map(|peeked| peeked.len()),
    2 => inp.peek::<U2>().map(|peeked| peeked.len()),
    3 => inp.peek::<U3>().map(|peeked| peeked.len()),
    _ => unreachable!("the sweeps run widths 0..=3, the capacity of `DefaultCache`"),
  }
}

/// One `skip_while` sweep over an input of either completeness, as its caller-visible outcome.
fn ledger_skip_run<'a, C, Cmpl>(
  inp: &mut InputRef<'a, '_, LedgerLexer<'a>, (LedgerEmitter, C), (), Cmpl>,
  prefill: usize,
  skips: usize,
) -> SkipRun
where
  C: Cache<'a, LedgerLexer<'a>, ()>,
  Cmpl: crate::input::SurfaceIncomplete<'a, LedgerLexer<'a>, (LedgerEmitter, C), ()>,
{
  let peeked = prefill_to(inp, prefill);
  let parked = inp.has_front_parked();
  // The residency, counting the parked slot: under a capacity that retains nothing the front of
  // the stream IS that slot, so a cache-only count would report 0 for every state.
  let depth = Cache::len(inp.cache()) + usize::from(parked);

  let mut asked = std::vec::Vec::new();
  let mut returned = std::vec::Vec::new();
  for _ in 0..skips {
    returned.push(inp.skip_while(|t| {
      asked.push((t.span.start.0, t.span.end.0, t.data.is_trivia()));
      t.data.is_trivia()
    }));
  }

  let cursor = inp.cursor().as_inner().0;
  let span = (inp.span().start.0, inp.span().end.0);
  let scanned = inp.state().scanned;
  let is_exhausted = inp.is_exhausted();
  let latch = inp.latch_snapshot().map(|at| at.0);
  let dedup = inp.emitted_error_end.0;

  let mut drained = std::vec::Vec::new();
  let drain_end = loop {
    match inp.next() {
      Ok(Some(tok)) => drained.push((tok.span_ref().start.0, tok.span_ref().end.0)),
      Ok(None) => break Ok(()),
      Err(e) => break Err(e),
    }
  };

  SkipRun {
    peeked,
    depth,
    parked,
    asked,
    returned,
    cursor,
    span,
    scanned,
    is_exhausted,
    latch,
    dedup,
    drained,
    drain_end,
    // Filled by `record_emitter` once the handle is gone.
    diagnostics: std::vec::Vec::new(),
    reports: (0, 0),
    live_rows: 0,
  }
}

/// A cache capacity the sweep can be generic over.
///
/// It has to be a trait with a lifetime-generic associated type rather than a plain type
/// parameter: the source outlives nothing outside [`both_routes`], so `C: Cache<'a, …>` cannot be
/// named by a caller who does not have `'a`.
trait Capacity {
  const NAME: &'static str;
  /// The residencies the widths 0..=3 can reach under this capacity, as a `[bool; 4]`.
  const REACHES: [bool; 4];
  type Cache<'a>: Cache<'a, LedgerLexer<'a>, ()> + Default;
}

/// The instrumented `U3` deque — the capacity every other cell in this file runs under.
struct Deque3;

impl Capacity for Deque3 {
  const NAME: &'static str = "ledger-deque(3)";
  const REACHES: [bool; 4] = [true, true, true, true];
  type Cache<'a> = LedgerCache<'a>;
}

/// One slot: the smallest capacity that still retains a token.
struct Slot1;

impl Capacity for Slot1 {
  const NAME: &'static str = "option(1)";
  const REACHES: [bool; 4] = [true, true, false, false];
  type Cache<'a> = Option<CachedTokenOf<'a, LedgerLexer<'a>>>;
}

/// No cache at all. `RETAINS_FRONT` is `false` here, so `can_park()` is **true** and the front of
/// the stream is the parked slot — the one residency in which the two routes reach a resident head
/// by different code (this route's `front`, the scanner's `Origin::Parked` prologue) and in which
/// [`AtCursor`](super::AtCursor) reads the parked token's end rather than the cache's back.
struct NoCache;

impl Capacity for NoCache {
  const NAME: &'static str = "blackhole(0)";
  const REACHES: [bool; 4] = [true, false, false, false];
  type Cache<'a> = ();
}

/// One fixture, at one prefill width, under one cache capacity, with `skips` skips — run down both
/// routes.
fn both_routes<K>(fixture: Fixture, prefill: usize, skips: usize) -> (SkipRun, SkipRun)
where
  K: Capacity,
{
  let src = LedgerSrc(fixture.text);
  // In force for both runs, and gone by the time this returns.
  let _budget = match fixture.budget {
    Some(tokens) => Budget::of(tokens),
    None => Budget::unlimited(),
  };

  let mut complete = ledger_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  let mut direct = {
    let mut inp = complete.as_ref();
    ledger_skip_run(&mut inp, prefill, skips)
  };
  direct.record_emitter(complete.emitter());

  let mut partial = ledger_partial_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  partial.seal();
  let mut scanned = {
    let mut inp = partial.as_ref();
    ledger_skip_run(&mut inp, prefill, skips)
  };
  scanned.record_emitter(partial.emitter());

  (direct, scanned)
}

/// What a sweep actually reached — the guard on the guard.
///
/// This file's drift guard passed for a year by not looking: its lexer could not fail, its sources
/// were words and trivia, and three of its four prefill depths were the same state. A tally that a
/// fixture set stops exercising a condition is therefore part of the test, not a nicety: each
/// sweep below asserts what it reached, so a fixture edited into vacuity is red rather than green.
#[derive(Debug, Default)]
struct Reached {
  /// Which residencies the prefill actually achieved, per capacity, in the order the sweep runs
  /// them: the `U3` deque, the one-slot cache, then the cache that retains nothing.
  depths: [[bool; 4]; 3],
  /// Cases whose input ended up poisoned.
  latched: usize,
  /// Non-limit lexer diagnostics offered, and limit ones.
  lex_diagnostics: usize,
  limit_diagnostics: usize,
  /// Diagnostics the emitter rejected.
  rejected: usize,
  /// Skips that returned `Err`, and drains that did.
  skip_errors: usize,
  drain_errors: usize,
  /// Cases where the prefill peek itself failed.
  peek_errors: usize,
}

impl Reached {
  fn record(&mut self, capacity: usize, run: &SkipRun) {
    self.depths[capacity][run.depth] = true;
    self.latched += usize::from(run.latch.is_some());
    for diagnostic in &run.diagnostics {
      match diagnostic.class {
        LedgerErr::Limit => self.limit_diagnostics += 1,
        _ => self.lex_diagnostics += 1,
      }
      self.rejected += usize::from(!diagnostic.accepted);
    }
    self.skip_errors += run.returned.iter().filter(|r| r.is_err()).count();
    self.drain_errors += usize::from(run.drain_end.is_err());
    self.peek_errors += usize::from(run.peeked.is_err());
  }
}

/// One capacity's worth of the sweep.
fn sweep_capacity<K>(fixtures: &[Fixture], slot: usize, reached: &mut Reached)
where
  K: Capacity,
{
  for &fixture in fixtures {
    for prefill in 0..=3usize {
      for skips in [1usize, 2] {
        let (direct, scanned) = both_routes::<K>(fixture, prefill, skips);
        assert_eq!(
          direct,
          scanned,
          "the complete-input route and the scanned partial route must observe the same skip \
           over {fixture:?} at prefill width {prefill} with {skips} skip(s) under {}",
          K::NAME
        );
        assert!(
          direct.depth <= prefill,
          "a prefill of width {prefill} cannot leave {} tokens resident over {fixture:?} under {}",
          direct.depth,
          K::NAME
        );
        reached.record(slot, &direct);
      }
    }
  }
  assert_eq!(
    reached.depths[slot],
    K::REACHES,
    "{} must reach exactly the residencies it can hold",
    K::NAME
  );
}

/// Runs one fixture set down both routes at every prefill width, every cache capacity and both
/// skip counts, asserting the two observations agree, and reports what the set reached.
fn sweep(fixtures: &[Fixture]) -> Reached {
  let mut reached = Reached::default();
  sweep_capacity::<Deque3>(fixtures, 0, &mut reached);
  sweep_capacity::<Slot1>(fixtures, 1, &mut reached);
  sweep_capacity::<NoCache>(fixtures, 2, &mut reached);
  reached
}

/// **The drift guard for the typestate split.**
///
/// [`skip_while`](InputRef::skip_while) runs its own loop on a [`Complete`](crate::input::Complete)
/// input and the shared scanner on a [`Partial`](crate::input::Partial) one, and two
/// implementations of one thing are the defect class the shared scanner exists to prevent — the
/// cache-drain prologue and the lexing loop each implemented skip-and-stop once, and they
/// repeatedly disagreed.
///
/// What closes it here is that the split is *observable*: a **sealed** partial input takes every
/// decision a complete one takes, because both partial-input rules are written
/// `Cmpl::PARTIAL && !self.is_final() && …`. So the two routes must land on the same observation
/// over the same stream, and this sweep — every source, four **measured** cache depths, one skip
/// and two back-to-back skips — is the assertion that they do. A change to either route that the
/// other does not get fails here rather than in a user's streaming parse.
///
/// The whole caller-visible tuple is compared; [`SkipRun`] is the list. The depths are the
/// [`prefill_to`] ones — a width per depth, and the depth **measured** afterwards, because
/// `peek::<U3>` repeated is one state and not three.
///
/// This cell is the clean half. The conditions the routes' arguments are actually about — a lexer
/// error crossed mid-skip, a limit trip, a rejecting emitter — are the sweep below.
#[test]
fn the_two_completeness_routes_observe_the_same_skip() {
  const CLEAN: &[Fixture] = &[
    Fixture::clean("ab cd ef"),
    Fixture::clean("~ ab cd"),
    Fixture::clean("~ ~ ab"),
    Fixture::clean("~ ~ ~"),
    Fixture::clean(""),
    Fixture::clean("~"),
    Fixture::clean("ab ~ ~ cd"),
  ];

  let reached = sweep(CLEAN);
  assert_eq!(
    (
      reached.latched,
      reached.lex_diagnostics,
      reached.limit_diagnostics,
      reached.skip_errors
    ),
    (0, 0, 0, 0),
    "these fixtures are the CLEAN half: nothing here may latch, diagnose or fail, or the sweep \
     below is not adding the conditions it claims to. Got {reached:?}"
  );
}

/// **The same guard, over the conditions the two routes' arguments are about.**
///
/// The cell above sweeps a lexer that cannot fail. Everything the complete-input route had to
/// re-establish when it left the scanner is about a lexer that can:
///
/// * **the latch.** The scanner latches a trip through its deferred
///   [`AtFrontier`](super::AtFrontier); this route latches through
///   [`AtCursor`](super::AtCursor), and they are the same offset only *because* this route commits
///   every token as it crosses it. That is an argument about a trip, and the clean sweep contains
///   none;
/// * **the crossed error.** A non-limit lexer error moves no frontier and is diagnosed exactly
///   once, deduplicated against a watermark that a prefill may already have raised. Which route
///   crossed it must not decide whether it is reported twice, or where the position lands
///   afterwards;
/// * **the fatal exit.** A rejecting emitter turns a diagnostic into the call's `Err`. Under the
///   scanner that exit keeps the scan's progress and settles the entry mark; on this route the
///   progress is already committed token by token and there is no mark. Both must leave the same
///   input behind.
///
/// The fixtures below arm the lexer (`!` runs lex as errors; a [`Budget`] makes it trip) and the
/// emitter (a rejection from the `n`-th diagnostic on), and the comparison is the same whole
/// [`SkipRun`] — including the latch and the dedup watermark, which only these cases can move.
///
/// This cell pins `FAULTY` two ways, and they catch different edits. `FAULTY_IDENTITIES`, right
/// below the fixture list, pins WHICH fixtures run — each one's exact source text and constructor
/// arguments — so a fixture swapped for a different one reds on that alone, whatever totals the
/// replacement happens to produce; the zero-budget trip and the trip behind a crossed lexer error
/// are each a single fixture below, with no sibling to cover for it if it were swapped out. The
/// exact seven-tuple after the sweep pins what the surviving identities actually reached, so an
/// edit that keeps every fixture's identity but changes its effect — a lexer or scanner change,
/// not a fixture change — still reds there. Neither guard subsumes the other.
///
/// Red against a broken route — **measured**, by making each break and reading the failure, not
/// asserted:
///
/// | deliberate breakage of the complete-input route | what this cell reports |
/// |---|---|
/// | latch at a *deferred* frontier, built when the lexing phase opens and never adopted, as the scanner's `AtFrontier` is before its first `adopt` | `~ ~ ~ ab` under a 2-token budget: `latch: Some(0)` against `Some(3)` |
/// | drop the pre-lex `reached_boundary` probe in phase 2 | the same fixture with **two** skips: `span: (0, 3)` against `(2, 3)` — the second skip rebuilds a lexer at a latched boundary and commits a span that starts where the first one did |
/// | swallow a fatal emitter rejection (`Err(e) => return Ok(())`) | `~ !! ~ ab` with the first diagnostic refused: `returned: [Ok(())]` against `[Err(Lex)]` |
///
/// None of the three is caught by the clean sweep above. The first is also caught by
/// [`a_trip_inside_a_trivia_skip_latches_where_the_scan_latches`]; the second by
/// `input_ref::tests::cache_transparency_matrix`; for the third, the whole of
/// `cargo test -p tokora --all-features --lib` is **green** without this cell and the next.
///
/// The pop-then-clamp reorder is not in the table because this cell adds nothing to it: it is
/// caught by [`an_unwinding_clamp_leaves_the_token_it_was_clamping_in_the_stream`] and by
/// [`an_unwinding_caller_clone_leaves_the_predicate_with_a_different_call_count`], and by those
/// two alone, with or without these fixtures. Reordering the clamp changes nothing a
/// non-unwinding run can see, which is the point of those cells.
#[test]
fn the_two_completeness_routes_observe_the_same_skip_when_the_lexer_or_the_emitter_fails() {
  const FAULTY: &[Fixture] = &[
    // ── A non-limit lexer error, in every position a skip can meet one ──
    // The error is behind the stopping token, so the skip never crosses it.
    Fixture::clean("ab !! cd"),
    // The skip's first act is to cross one.
    Fixture::clean("!! ab"),
    // Crossed after a trivia token, with a token behind it to stop on.
    Fixture::clean("~ !! ab"),
    // INSIDE the skipped run: trivia, error, trivia — the frontier must not move onto the error.
    Fixture::clean("~ !! ~ ab"),
    // Two in a row, then trivia to end of input.
    Fixture::clean("~ !! !! ~"),
    // The last thing before end of input, so the skip's EOF settle happens past an error.
    Fixture::clean("~ !!"),
    // Nothing but an error.
    Fixture::clean("!!"),
    // An error the prefill lexes and diagnoses, which the skip then re-lexes: the dedup watermark
    // decides whether it is reported once or twice, and it must decide the same on both routes.
    Fixture::clean("!! !! ab cd"),
    // ── A limit trip ──
    // The trip fires on the third trivia token, mid-skip, with tokens already committed.
    Fixture::tripping("~ ~ ~ ab", 2),
    // The trip fires on the token the skip would have stopped on.
    Fixture::tripping("~ ~ ab cd", 2),
    // One token behind the first skip.
    Fixture::tripping("~ ab cd", 1),
    // The skip stops on the head; the trip is behind it and is never reached by the skip itself.
    Fixture::tripping("ab cd ef", 1),
    // Nothing but trivia, so the trip lands with the skip still running.
    Fixture::tripping("~ ~ ~ ~", 2),
    // Spent before the first token: the whole input is dead from offset 0.
    Fixture::tripping("~ ~ ~ ab", 0),
    // A crossed lexer error on the way to a trip — the two faults in one stream.
    Fixture::tripping("~ !! ~ ~ ab", 2),
    // ── A rejecting emitter ──
    Fixture::fatal("~ !! ~ ab", 1),
    Fixture::fatal("~ !! !! ab", 2),
    Fixture::fatal("!! ab cd", 1),
    // The error is BEHIND the stopping token, so the skip returns cleanly and the rejection lands
    // on the later drain instead — the one fixture whose `drain_end` is an `Err`. At the wider
    // prefills the peek meets it first, and that has to agree across the routes too.
    Fixture::fatal("ab !! cd", 1),
    // A rejected TRIP diagnostic: the boundary is latched before the verdict is even returned, so
    // the fatal exit has to keep it.
    Fixture::tripping_and_fatal("~ ~ ~ ab", 2, 1),
    Fixture::tripping_and_fatal("~ !! ~ ~ ab", 2, 2),
  ];

  // The inventory guard: what FAULTY IS, pinned independently of what it adds up to. A replacement
  // fixture cannot reproduce another fixture's identity — only, possibly, its effects — so this is
  // what actually closes the swap Codex's finding describes: a fixture exercising a distinct
  // high-risk route (a zero-budget trip, a trip behind a crossed lexer error) traded for a
  // different one that happens to land on the same seven totals below. `Fixture::identity` is a
  // plain projection of the struct's own fields; it does not know which constructor built the
  // fixture, so this list is not a second copy of the constructor calls above it.
  const FAULTY_IDENTITIES: &[(&str, Option<usize>, Option<usize>)] = &[
    ("ab !! cd", None, None),
    ("!! ab", None, None),
    ("~ !! ab", None, None),
    ("~ !! ~ ab", None, None),
    ("~ !! !! ~", None, None),
    ("~ !!", None, None),
    ("!!", None, None),
    ("!! !! ab cd", None, None),
    ("~ ~ ~ ab", Some(2), None),
    ("~ ~ ab cd", Some(2), None),
    ("~ ab cd", Some(1), None),
    ("ab cd ef", Some(1), None),
    ("~ ~ ~ ~", Some(2), None),
    ("~ ~ ~ ab", Some(0), None),
    ("~ !! ~ ~ ab", Some(2), None),
    ("~ !! ~ ab", None, Some(1)),
    ("~ !! !! ab", None, Some(2)),
    ("!! ab cd", None, Some(1)),
    ("ab !! cd", None, Some(1)),
    ("~ ~ ~ ab", Some(2), Some(1)),
    ("~ !! ~ ~ ab", Some(2), Some(2)),
  ];
  assert_eq!(
    FAULTY.len(),
    FAULTY_IDENTITIES.len(),
    "FAULTY gained or lost a fixture: this pins the COUNT separately from the per-fixture check \
     below, because `Iterator::zip` stops at the shorter side and would not otherwise notice one \
     list running longer than the other. Update FAULTY_IDENTITIES to match, then treat the \
     seven-tuple below as unverified until you have re-derived it (see its own message)."
  );
  for (i, (&fixture, &expected)) in FAULTY.iter().zip(FAULTY_IDENTITIES).enumerate() {
    let got = fixture.identity();
    assert_eq!(
      got, expected,
      "fixture #{i} was replaced: this sweep pins WHICH fixtures it runs, not only what they add \
       up to, because a replacement fixture can reproduce the exact totals below while exercising \
       a different route than the one documented above it — the zero-budget trip and the trip \
       behind a crossed lexer error are each one fixture, with no sibling to carry their share of \
       the totals if swapped out. If this is an intentional edit, update FAULTY_IDENTITIES to \
       match and re-derive the seven-tuple below rather than assuming it still holds. Expected \
       {expected:?}, found {got:?}."
    );
  }

  let reached = sweep(FAULTY);
  // The secondary checksum: the loop above pins WHICH fixtures ran; this pins what they reached,
  // so an edit that keeps every fixture's identity but changes its effect (a lexer or scanner
  // change, not a fixture change) still reds here.
  assert_eq!(
    (
      reached.latched,
      reached.lex_diagnostics,
      reached.limit_diagnostics,
      reached.rejected,
      reached.skip_errors,
      reached.peek_errors,
      reached.drain_errors,
    ),
    (204, 408, 216, 144, 66, 66, 12),
    "and it must actually reach every condition it exists for EXACTLY THIS MANY TIMES — a latched \
     boundary, a non-limit lexer diagnostic, a limit one, a rejected one, a skip that returns \
     `Err`, a prefill peek that does, and a later drain that does. Every counter here is summed \
     over the whole fixture set, so a mere `> 0` check lets one high-risk fixture's removal hide \
     behind the other twenty: this tuple is exact instead, so dropping ANY single fixture below \
     changes at least one total and reds. A fixture SWAPPED for a different one is what \
     FAULTY_IDENTITIES above catches; this tuple is the checksum on what the surviving identities \
     produced. If you added or edited a fixture on purpose and that is why this failed: confirm — \
     by reading the diff, not by pattern-matching on this message — that the new totals reflect \
     only your intended change, then update the tuple above to match (temporarily swap this \
     `assert_eq!` for `dbg!(&reached);` and run with `--nocapture` to read the new numbers off). \
     Do not paste in a new number without doing that. Got {reached:?}"
  );
}

/// The sweep above proves the two routes agree; this pins what they agree *on*, so a change that
/// broke both of them in the same way could not pass unnoticed.
///
/// Three values, one per fault, read off the complete-input route at prefill 0 with one skip.
#[test]
fn the_agreed_failing_skip_observation_is_the_right_one() {
  // ── A trip mid-skip ──
  // `~ ~ ~ ab` under a two-token budget: the first two trivia tokens are crossed and committed,
  // the third trips. The boundary is the DURABLE FRONTIER — the end of the last skipped token,
  // offset 3 — not the tripping token's own span, and not the cursor the call started from.
  let (direct, scanned) = both_routes::<Deque3>(Fixture::tripping("~ ~ ~ ab", 2), 0, 1);
  assert_eq!(direct, scanned);
  assert_eq!(direct.latch, Some(3), "the latch is the durable frontier");
  assert_eq!(direct.span, (2, 3), "committed at the last skipped token");
  assert_eq!(direct.scanned, 2, "and the state that produced it");
  assert_eq!(direct.cursor, 3, "the cursor is the committed end");
  assert_eq!(
    direct.diagnostics.len(),
    1,
    "the trip is diagnosed exactly once. Got {:?}",
    direct.diagnostics
  );
  assert_eq!(direct.diagnostics[0].class, LedgerErr::Limit);
  assert!(direct.diagnostics[0].accepted);
  assert_eq!(
    direct.drained,
    std::vec::Vec::new(),
    "and the drain stops at the boundary"
  );
  assert_eq!(
    direct.returned,
    std::vec![Ok(())],
    "the skip still succeeds"
  );

  // ── A crossed non-limit lexer error ──
  // `~ !! ~ ab`: both trivia tokens are skipped and the error between them is diagnosed once. The
  // error moves no frontier, so the committed span is the SECOND trivia token, and the watermark
  // is the error's end.
  let (direct, scanned) = both_routes::<Deque3>(Fixture::clean("~ !! ~ ab"), 0, 1);
  assert_eq!(direct, scanned);
  assert_eq!(direct.latch, None, "a plain lexer error latches nothing");
  assert_eq!(
    direct.span,
    (5, 6),
    "the frontier tracks TOKENS: it is the trivia token BEHIND the error, not the error"
  );
  assert_eq!(direct.dedup, 4, "the watermark is the error's end");
  assert_eq!(
    direct.diagnostics.len(),
    1,
    "one error, one diagnostic. Got {:?}",
    direct.diagnostics
  );
  assert_eq!(direct.diagnostics[0].class, LedgerErr::Lex);
  assert_eq!(
    direct.drained,
    std::vec![(7, 9)],
    "and the stopping token is still there"
  );

  // The same source with the error already lexed by a wide prefill: the dedup watermark is the
  // reason it is diagnosed once and not twice, and it is the same one either way.
  let (deep, deep_scanned) = both_routes::<Deque3>(Fixture::clean("~ !! ~ ab"), 3, 1);
  assert_eq!(deep, deep_scanned);
  assert_eq!(
    deep.diagnostics.len(),
    1,
    "the prefill emitted it and the skip re-lexed the same region. Got {:?}",
    deep.diagnostics
  );

  // ── A rejecting emitter ──
  // The same stream, with the first diagnostic refused: the skip propagates the `Err`, and the
  // position is settled at the LEXER — the rejected item's span — rather than left mid-scan.
  let (direct, scanned) = both_routes::<Deque3>(Fixture::fatal("~ !! ~ ab", 1), 0, 1);
  assert_eq!(direct, scanned);
  assert_eq!(
    direct.returned,
    std::vec![Err(LedgerErr::Lex)],
    "a rejected lexer diagnostic is the call's error"
  );
  assert_eq!(
    direct.diagnostics.len(),
    1,
    "and nothing was offered after it. Got {:?}",
    direct.diagnostics
  );
  assert!(!direct.diagnostics[0].accepted);
  assert_eq!(
    direct.span,
    (2, 4),
    "the fatal exit settles at the rejected item's span"
  );
  assert_eq!(
    direct.live_rows, 0,
    "and neither route leaves an emitter mark outstanding on the way out"
  );
}

// ── The parked front: the residency a peek cannot construct, but a decline does ─────────────────

/// Parks the front token via a **declining** [`try_expect`](InputRef::try_expect), over a source
/// neither route has touched yet.
///
/// [`prefill_to`] cannot reach the state this leaves. It only ever peeks, and a peek that finds
/// nothing resident under a zero-capacity cache has no token to decline — the residency it can
/// produce tops out at the empty one `prefill_to(_, 0)` already is. A decline does not go through
/// the cache at all: the token that comes back `false` from `pred` goes into the pending slot
/// [`hold_front`](super::InputRef) keeps outside it, which is exactly where ordinary parser code
/// leaves a token every time an atom peeks ahead, does not like what it sees, and lets the next
/// combinator have it.
///
/// # Panics
///
/// If the fixture's source does not lex cleanly, or the decline does not leave a token parked —
/// either would mean the caller handed this a fixture it cannot do its job over.
fn park_front<'a, C, Cmpl>(
  inp: &mut InputRef<'a, '_, LedgerLexer<'a>, (LedgerEmitter, C), (), Cmpl>,
) where
  C: Cache<'a, LedgerLexer<'a>, ()>,
  Cmpl: crate::input::SurfaceIncomplete<'a, LedgerLexer<'a>, (LedgerEmitter, C), ()>,
{
  let declined = inp
    .try_expect(|_| false)
    .expect("the fixture's source lexes cleanly, so a decline is the only outcome");
  assert!(
    declined.is_none(),
    "an unconditionally declining predicate must decline"
  );
  assert!(
    inp.has_front_parked(),
    "a decline against an empty zero-capacity cache must park the token rather than lose it"
  );
}

/// [`both_routes`], but the front is parked by [`park_front`] before the sweep runs instead of
/// filled by [`prefill_to`] — the one residency [`prefill_to`] cannot reach under a zero-capacity
/// cache.
///
/// Fixed to [`NoCache`] rather than generic the way `both_routes` is: parking is only possible
/// where `RETAINS_FRONT` is false, so a decline against [`Slot1`] or [`Deque3`] is simply a cache
/// push that succeeds, and `park_front`'s own assertion would fire rather than silently sweep
/// nothing — the type parameter is kept, and misuse reported loudly, rather than the function
/// hard-coding `NoCache` and hiding that condition.
///
/// Kept apart from [`sweep`] and [`Reached`] on purpose: feeding this through `Reached::depths`
/// would credit [`prefill_to`] with a residency only a decline reaches — exactly the conflation
/// between cache depth and the pending slot that left this state unswept.
fn both_routes_over_a_parked_front<K>(fixture: Fixture, skips: usize) -> (SkipRun, SkipRun)
where
  K: Capacity,
{
  let src = LedgerSrc(fixture.text);
  // In force for both runs, and gone by the time this returns — see `both_routes`.
  let _budget = match fixture.budget {
    Some(tokens) => Budget::of(tokens),
    None => Budget::unlimited(),
  };

  let mut complete = ledger_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  let mut direct = {
    let mut inp = complete.as_ref();
    park_front(&mut inp);
    ledger_skip_run(&mut inp, 0, skips)
  };
  direct.record_emitter(complete.emitter());

  let mut partial = ledger_partial_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  partial.seal();
  let mut scanned = {
    let mut inp = partial.as_ref();
    park_front(&mut inp);
    ledger_skip_run(&mut inp, 0, skips)
  };
  scanned.record_emitter(partial.emitter());

  (direct, scanned)
}

/// **The parked front — the residency [`prefill_to`] cannot reach, and the sweep above cannot
/// see.**
///
/// [`NoCache::REACHES`] is `[true, false, false, false]`, and that is the whole truth about what a
/// **peek** can fill: a zero-capacity cache has nowhere to put a second token, so every width above
/// 0 collapses back to depth 0. But a token **declined** — not asked for again, just left behind —
/// never goes through the cache at all, so it is not bound by that ceiling: it goes into the
/// pending slot, and ordinary parser code reaches that constantly, through
/// [`try_expect`](InputRef::try_expect) and every combinator built on it — so the `REACHES` tuple is
/// honest about what a peek reaches, not about everything the pending slot can hold.
///
/// The sweep above does brush the REJECTING half of this by accident, and saying so is more honest
/// than the stronger claim that it never parks anything: `skip_while`'s own lexing phase parks its
/// stopping token exactly as a decline does (`hold_front`, off `Origin::Lexer`), so under `NoCache`
/// the second of a fixture's two back-to-back skips can already begin parked. But that token is
/// always the one the FIRST skip's predicate just rejected, and the second skip asks the identical,
/// deterministic predicate — so it can only be rejected again, through the `front` probe both
/// routes share, before either the scanner loop or a commit runs. Neither sweep tracks this as a
/// state of its own, and neither can ever reach the ACCEPTING half this way: that needs a decline
/// for a reason unrelated to `skip_while`'s own predicate, which only an outside caller —
/// [`park_front`]'s `try_expect(|_| false)` here — can manufacture.
///
/// It matters because the two routes reach a resident head by different code **only** here. A
/// resident cache entry (`Slot1`, `Deque3`) is popped and pushed back by both routes through the
/// same [`Cache::front`]/`pop_front`/`push_front` calls, and under `NoCache` a peek never leaves
/// anything to find. A **parked** front is read by both routes through the same
/// [`front`](super::InputRef) probe first — so a REJECTING predicate stops identically on both, and
/// that half of this state is not new — but an ACCEPTING predicate then splits: the complete-input
/// route consumes it in place through [`commit_front`](super::InputRef), while the sealed-partial
/// route hands it to the shared scanner, which re-fetches it as
/// [`Origin::Parked`](super::Origin::Parked) — the one origin `Slot1` and `Deque3` never produce —
/// and folds it into the deferred frontier through [`AtFrontier::adopt`](super::AtFrontier::adopt)
/// rather than committing it directly.
///
/// Two cases, kept apart from prefill depth on purpose — see [`both_routes_over_a_parked_front`]:
/// parking is the PENDING SLOT, not cache residency, and conflating the two is what left this route
/// unswept in the first place.
///
/// * **accepted.** `"~ ab"` — the decline parks a TRIVIA token, which the skip predicate takes:
///   both routes consume it, then meet the same freshly-lexed `"ab"`, which the predicate rejects,
///   so it lands back at the front and the run ends parked again.
/// * **rejecting.** `"ab cd"` — the decline parks a WORD, which the predicate refuses outright.
///   Neither route's skip loop runs at all: the stop is the shared `front` probe itself, so the
///   parked token, the position and the residency are all untouched.
///
/// # What this cell cannot see, and what does
///
/// [`SkipRun`] is the caller-visible *values* a skip leaves behind; the committed-token side
/// channel [`Emitter::commit_token`](crate::Emitter::commit_token) feeds is not one of them, so no
/// field here can carry it. Measured: removing the notification from a site a parked token's
/// consumption reaches leaves this cell green. A regression confined to that one notification, on
/// a parked token, had no observer anywhere in this sweep family — closing it needed a case built
/// on the effect ledger rather than another [`SkipRun`] field, and
/// [`the_committed_token_side_channel_is_fed_over_a_parked_front`] at the foot of this file is
/// that case. It stays a separate cell on purpose: the two ask different questions of the same
/// residency, and folding the ledger into `SkipRun` would make every value comparison in this file
/// depend on caller code the two routes are *entitled* to run differently.
#[test]
fn the_two_completeness_routes_observe_the_same_skip_over_a_parked_front() {
  // ── accepted: the parked token is trivia, and the predicate takes it ──
  let (direct, scanned) = both_routes_over_a_parked_front::<NoCache>(Fixture::clean("~ ab"), 1);
  assert_eq!(
    direct,
    scanned,
    "the complete-input route and the scanned partial route must observe the same skip over an \
     ACCEPTED parked front under {}",
    NoCache::NAME
  );
  assert!(
    direct.parked,
    "the front is the parked slot, not a cache entry, going into the skip"
  );
  assert_eq!(
    direct.depth, 1,
    "a parked token is one resident token, exactly like a cached one"
  );
  assert_eq!(
    direct.asked,
    std::vec![(0, 1, true), (2, 4, false)],
    "the parked trivia token is asked about once — not twice — then the freshly-lexed word that \
     stops the skip. Got {:?}",
    direct.asked
  );
  assert_eq!(direct.returned, std::vec![Ok(())]);
  assert_eq!(
    direct.cursor, 2,
    "resumed at the word the skip stopped on, itself now the parked token"
  );
  assert_eq!(
    direct.span,
    (0, 1),
    "committed exactly the parked trivia token"
  );
  assert_eq!(
    direct.scanned, 1,
    "one token accounted for: the trivia the decline had already lexed"
  );
  assert_eq!(
    direct.drained,
    std::vec![(2, 4)],
    "the word the skip put back is still there for the next call"
  );
  assert_eq!(
    direct.live_rows, 0,
    "neither route leaves an emitter mark outstanding on the way out"
  );

  // ── rejecting: the parked token is a word, and the predicate declines it outright ──
  let (direct, scanned) = both_routes_over_a_parked_front::<NoCache>(Fixture::clean("ab cd"), 1);
  assert_eq!(
    direct,
    scanned,
    "the complete-input route and the scanned partial route must observe the same skip over a \
     REJECTING parked front under {}",
    NoCache::NAME
  );
  assert!(
    direct.parked,
    "the front is the parked slot going into the skip"
  );
  assert_eq!(
    direct.depth, 1,
    "a parked token is one resident token, exactly like a cached one"
  );
  assert_eq!(
    direct.asked,
    std::vec![(0, 2, false)],
    "the parked word is asked about once, and the skip stops there without lexing anything"
  );
  assert_eq!(direct.returned, std::vec![Ok(())]);
  assert_eq!(
    direct.cursor, 0,
    "nothing consumed: the resume cursor is exactly where it started"
  );
  assert_eq!(direct.span, (0, 0), "nothing committed either");
  assert_eq!(
    direct.scanned, 0,
    "no token has ever been committed to the input's own state"
  );
  assert_eq!(
    direct.drained,
    std::vec![(0, 2), (3, 5)],
    "the parked word and the one behind it are both still there for the next call"
  );
  assert_eq!(
    direct.live_rows, 0,
    "neither route leaves an emitter mark outstanding on the way out"
  );
}

// ── The unwind edge: the one call this route runs with a token outside the stream ────────────────

/// Committed-token notifications in one recording.
///
/// [`Effect::CommitToken`] has been recorded by the ledger emitter since the ledger existed and
/// read back by nothing, which is exactly the gap the two cells at the foot of this file close: it
/// is the one thing a `skip_while` does that no *value* a caller reads off the input carries.
fn commit_notifications(effects: &[Effect]) -> usize {
  effects
    .iter()
    .filter(|effect| **effect == Effect::CommitToken)
    .count()
}

/// One `skip_while` interrupted by a panicking predicate, as the state the unwind left behind.
///
/// A second struct rather than a field on [`SkipRun`], because the three facts that matter here
/// are meaningless for a call that returned: where the token the predicate never answered about
/// ended up, whether the next read had to produce it a second time, and how many committed-token
/// notifications the interrupted call had already made.
#[derive(Debug, PartialEq)]
struct UnwoundSkip {
  /// The residency going IN, measured (see [`prefill_to`]). Also the sweep's discriminator: the
  /// predicate is asked about the resident tokens first, so a panic on call `k` lands on a
  /// **freshly-lexed** token exactly when `k` is past this and the call was reached at all.
  depth_in: usize,
  parked_in: bool,
  /// Whether the armed call was reached. `false` is a legitimate outcome — a skip that stops or
  /// exhausts before call `panic_on` — and the two routes have to agree on that too.
  caught: bool,
  /// What the call returned when it was NOT interrupted.
  returned: Option<Result<(), LedgerErr>>,
  /// The predicate's own tally, and the tokens it was handed — the panicking call included, since
  /// it records its argument before it panics.
  calls: usize,
  asked: std::vec::Vec<(usize, usize)>,
  /// **The residency the unwind left.** A stopper the unwind dropped leaves nothing here; one it
  /// put back is the parked slot or the cache's front, depending on the capacity.
  parked: bool,
  depth: usize,
  /// **The resume cursor** — the fact the old range assertion could not pin. A stopper put back
  /// resumes at its own start; one dropped resumes at the committed end. Every fixture below has a
  /// gap between the two, so they are different numbers and not merely different residencies.
  cursor: usize,
  span: (usize, usize),
  scanned: usize,
  latch: Option<usize>,
  dedup: usize,
  /// Committed-token notifications the interrupted call fed the side channel, off the effect
  /// ledger.
  commits: usize,
  /// Tokens the lexer produced **during the call**, and then **during the drain after it**.
  ///
  /// The second is the direct measurement of *was the stopper re-lexed*, and it counts the whole
  /// drain rather than that one token because the lexer cannot be asked about one token: a stopper
  /// left resident is SERVED out of the stream, so the drain lexes one fewer than it yields; one
  /// the unwind dropped is produced a second time, so the drain lexes exactly what it yields.
  /// Nothing a caller reads back can tell those apart — a re-lex from the committed position
  /// reproduces the token, its span and its post-token state exactly.
  lexed: usize,
  drain_lexed: usize,
  drained: std::vec::Vec<(usize, usize)>,
  drain_end: Result<(), LedgerErr>,
  diagnostics: std::vec::Vec<Diagnostic>,
  reports: (usize, usize),
  live_rows: usize,
}

impl UnwoundSkip {
  /// The half of the observation that has to be read once the handle is gone — see
  /// [`SkipRun::record_emitter`].
  fn record_emitter(&mut self, emitter: &LedgerEmitter) {
    self.diagnostics = emitter.offered().to_vec();
    self.reports = emitter.reports();
    self.live_rows = emitter.live_rows();
  }
}

/// One `skip_while` down one route, with the predicate armed to panic on its `panic_on`-th call.
fn ledger_unwound_skip<'a, C, Cmpl>(
  inp: &mut InputRef<'a, '_, LedgerLexer<'a>, (LedgerEmitter, C), (), Cmpl>,
  prefill: usize,
  panic_on: usize,
) -> UnwoundSkip
where
  C: Cache<'a, LedgerLexer<'a>, ()>,
  Cmpl: crate::input::SurfaceIncomplete<'a, LedgerLexer<'a>, (LedgerEmitter, C), ()>,
{
  let _ = prefill_to(inp, prefill);
  let parked_in = inp.has_front_parked();
  let depth_in = Cache::len(inp.cache()) + usize::from(parked_in);

  let asked = RefCell::new(std::vec::Vec::new());
  reset_caller_calls();
  reset_tokens_lexed();
  record();
  let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    inp.skip_while(|t| {
      caller_call();
      asked.borrow_mut().push((t.span.start.0, t.span.end.0));
      assert!(
        caller_calls() != panic_on,
        "the armed predicate call panics mid-skip"
      );
      t.data.is_trivia()
    })
  }));
  let effects = recorded();
  let lexed = tokens_lexed();
  let caught = outcome.is_err();
  let returned = outcome.ok();

  let parked = inp.has_front_parked();
  let depth = Cache::len(inp.cache()) + usize::from(parked);
  let cursor = inp.cursor().as_inner().0;
  let span = (inp.span().start.0, inp.span().end.0);
  let scanned = inp.state().scanned;
  let latch = inp.latch_snapshot().map(|at| at.0);
  let dedup = inp.emitted_error_end.0;

  reset_tokens_lexed();
  let mut drained = std::vec::Vec::new();
  let drain_end = loop {
    match inp.next() {
      Ok(Some(tok)) => drained.push((tok.span_ref().start.0, tok.span_ref().end.0)),
      Ok(None) => break Ok(()),
      Err(e) => break Err(e),
    }
  };
  let drain_lexed = tokens_lexed();

  UnwoundSkip {
    depth_in,
    parked_in,
    caught,
    returned,
    calls: caller_calls(),
    asked: asked.into_inner(),
    parked,
    depth,
    cursor,
    span,
    scanned,
    latch,
    dedup,
    commits: commit_notifications(&effects),
    lexed,
    drain_lexed,
    drained,
    drain_end,
    // Filled by `record_emitter` once the handle is gone.
    diagnostics: std::vec::Vec::new(),
    reports: (0, 0),
    live_rows: 0,
  }
}

/// [`both_routes`], for a predicate armed to panic.
fn both_routes_under_an_unwinding_predicate<K>(
  fixture: Fixture,
  prefill: usize,
  panic_on: usize,
) -> (UnwoundSkip, UnwoundSkip)
where
  K: Capacity,
{
  let src = LedgerSrc(fixture.text);
  // In force for both runs, and gone by the time this returns — see `both_routes`.
  let _budget = match fixture.budget {
    Some(tokens) => Budget::of(tokens),
    None => Budget::unlimited(),
  };

  let mut complete = ledger_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  let mut direct = {
    let mut inp = complete.as_ref();
    ledger_unwound_skip(&mut inp, prefill, panic_on)
  };
  direct.record_emitter(complete.emitter());

  let mut partial = ledger_partial_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  partial.seal();
  let mut scanned = {
    let mut inp = partial.as_ref();
    ledger_unwound_skip(&mut inp, prefill, panic_on)
  };
  scanned.record_emitter(partial.emitter());

  (direct, scanned)
}

/// [`both_routes_over_a_parked_front`], for a predicate armed to panic — the same window, entered
/// from the one residency a peek cannot build.
fn both_routes_over_a_parked_front_under_an_unwinding_predicate<K>(
  fixture: Fixture,
  panic_on: usize,
) -> (UnwoundSkip, UnwoundSkip)
where
  K: Capacity,
{
  let src = LedgerSrc(fixture.text);
  let _budget = match fixture.budget {
    Some(tokens) => Budget::of(tokens),
    None => Budget::unlimited(),
  };

  let mut complete = ledger_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  let mut direct = {
    let mut inp = complete.as_ref();
    park_front(&mut inp);
    ledger_unwound_skip(&mut inp, 0, panic_on)
  };
  direct.record_emitter(complete.emitter());

  let mut partial = ledger_partial_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  partial.seal();
  let mut scanned = {
    let mut inp = partial.as_ref();
    park_front(&mut inp);
    ledger_unwound_skip(&mut inp, 0, panic_on)
  };
  scanned.record_emitter(partial.emitter());

  (direct, scanned)
}

/// **The drift guard on the unwind edge**, and the reason it is a separate cell from
/// [`a_panic_mid_skip_consumes_exactly_what_the_predicate_accepted`].
///
/// That cell asks the right question of one route: a predicate that panics on its `k`-th call must
/// leave exactly the `k − 1` tokens it accepted consumed. Its closing check on where the stream
/// resumes is a **range** —
///
/// ```text
/// committed_end <= cursor && cursor <= all[panic_on - 1].start()
/// ```
///
/// — and with an inter-token gap that range has two members. A stopper the unwind PUT BACK resumes
/// at its own start; a stopper the unwind DROPPED resumes at the committed end and is lexed again
/// by the next read. Both satisfy the range, both leave the drain identical (a re-lex reproduces
/// the token, its span and its post-token state exactly), and the difference between them is
/// precisely a typestate divergence: the sealed-[`Partial`] route's
/// [`ScanScope`](super::scan::ScanScope) holds the token in `TokenSlot::Held` and puts it back,
/// so the complete-input route must too.
///
/// So this cell compares the two routes **exactly**, on the facts a range cannot express:
///
/// * the resume **cursor**, over fixtures whose tokens are separated by a gap;
/// * the **front residency** the unwind left, in both spellings — the parked slot under a cache
///   that retains nothing, a cache entry under one that does;
/// * whether the stopper was **re-lexed**, counted at the lexer rather than inferred, because
///   every value a caller can read back off the input is identical either way;
/// * the **committed-token notifications** the interrupted call had already made, off the effect
///   ledger — the side channel no [`SkipRun`] field carries.
///
/// …plus the whole of [`UnwoundSkip`], as the neighbouring sweeps compare the whole of `SkipRun`.
///
/// Red against `a826ffe`, measured rather than asserted: `"~ ~ ab cd"` at prefill 0 with the panic
/// on call 3 reported `cursor: 3` against `4`, `parked: false` against `true`, `depth: 0` against
/// `1` and `drain_lexed: 2` against `1` — the complete-input route dropping the freshly-lexed
/// stopper where the scanned route parked it. The panic sweep above stayed green throughout.
///
/// # What this cell does not cover, and where that is pinned instead
///
/// **The predicate, at every call — and nothing else about the unwind edge.** The two routes are
/// held to the same observation for a panic out of the predicate, out of the emitter, and out of
/// the lexer, with **one** exit excluded: an unwind *inside the end-of-input settle*, where the
/// scanner has already disarmed its scope and drops the frontier it was about to commit while the
/// complete-input route keeps a prefix it committed token by token. That is deliberate, it is
/// documented in `skip_while`'s *Panic unwind* section, and it is pinned as a **difference** — both
/// columns asserted, so a run that turned it into agreement would be red too — by
/// `the_two_completeness_routes_are_pinned_apart_on_an_interrupted_eof_settle` in
/// `super::tests`. A green run of this cell is therefore not proof of parity across the unwind
/// edge; it is proof of parity across the predicate's part of it.
#[test]
fn the_two_completeness_routes_observe_the_same_unwound_skip() {
  const UNWOUND: &[Fixture] = &[
    // Trivia, trivia, then a word the predicate rejects, every token separated by a gap. Call 3
    // is the case this cell exists for: the first freshly-lexed stopper after a skipped token.
    Fixture::clean("~ ~ ab cd"),
    // Nothing but trivia, so the run ends at end of input and a panic on the last call lands on a
    // token the skip would have ACCEPTED rather than one it would have stopped on.
    Fixture::clean("~ ~ ~"),
  ];

  // What the sweep reached, so a fixture edited into vacuity is red rather than green — the
  // discriminator is `depth_in`, because the predicate is handed the resident tokens first.
  let (mut on_lexed, mut on_resident, mut never_fired) = (0usize, 0usize, 0usize);
  for &fixture in UNWOUND {
    for prefill in 0..=3usize {
      for panic_on in 1..=4usize {
        for (capacity, (direct, scanned)) in [
          (
            Deque3::NAME,
            both_routes_under_an_unwinding_predicate::<Deque3>(fixture, prefill, panic_on),
          ),
          (
            Slot1::NAME,
            both_routes_under_an_unwinding_predicate::<Slot1>(fixture, prefill, panic_on),
          ),
          (
            NoCache::NAME,
            both_routes_under_an_unwinding_predicate::<NoCache>(fixture, prefill, panic_on),
          ),
        ] {
          assert_eq!(
            direct, scanned,
            "the complete-input route and the scanned partial route must observe the same \
             INTERRUPTED skip over {fixture:?} at prefill width {prefill}, with the predicate \
             panicking on call {panic_on}, under {capacity}"
          );
          if !direct.caught {
            never_fired += 1;
          } else if panic_on > direct.depth_in {
            on_lexed += 1;
          } else {
            on_resident += 1;
          }
        }
      }
    }
  }
  assert_eq!(
    (on_lexed, on_resident, never_fired),
    (54, 18, 24),
    "and the sweep must actually land the panic on a FRESHLY-LEXED token, on a RESIDENT one, and \
     miss it altogether, exactly this many times. The first is the window this cell exists for and \
     the only one in which the routes could ever have diverged; the other two are the control. \
     Derived, not read off a run: both fixtures answer the predicate three times (two trivia then \
     a rejected word; three trivia then end of input), so `panic_on == 4` never fires and that is \
     2 fixtures x 4 widths x 3 capacities = 24. Of the three that do fire, the ones past `depth_in` \
     land on a lexed token — `Deque3` reaches depths 0,1,2,3 (3+2+1+0 = 6 lexed, 6 resident), \
     `Slot1` reaches 0,1,1,1 (3+2+2+2 = 9 lexed, 3 resident) and `NoCache` reaches 0 four times \
     (12 lexed, 0 resident), for 27 and 9 per fixture. Re-derive the same way — do not adjust \
     these to whatever came out — if you change the fixtures, the widths or the capacities."
  );

  // ── What they agree ON, at the one cell the range assertion could not separate ──
  // `"~ ~ ab cd"` at prefill 0: two trivia tokens skipped and committed, then the word at (4, 6)
  // freshly lexed and handed to a predicate that unwinds. Offset 3 is the committed end and offset
  // 4 is the stopper's start, and the gap between them is the whole measurement.
  let (direct, scanned) =
    both_routes_under_an_unwinding_predicate::<NoCache>(Fixture::clean("~ ~ ab cd"), 0, 3);
  assert_eq!(direct, scanned);
  assert!(direct.caught, "the armed call was reached");
  assert_eq!(
    direct.asked,
    std::vec![(0, 1), (2, 3), (4, 6)],
    "two trivia tokens, then the word the predicate panicked on"
  );
  assert_eq!(
    direct.span,
    (2, 3),
    "the prefix the predicate accepted is kept, committed at the last of it"
  );
  assert_eq!(
    direct.cursor, 4,
    "and the stopper is AT THE FRONT: the resume cursor is its own start (4), not the committed \
     end (3). Both satisfied the old range check"
  );
  assert!(
    direct.parked,
    "under a cache that retains nothing the put-back is the parked slot"
  );
  assert_eq!(direct.depth, 1, "one token resident, and it is that one");
  assert_eq!(
    direct.drained,
    std::vec![(4, 6), (7, 9)],
    "and the drain yields the stopper first"
  );
  assert_eq!(
    direct.drain_lexed, 1,
    "…having LEXED only the token behind it. The stopper is served out of the stream; dropped, it \
     would be produced a second time and this would be 2 — the fact a re-lex otherwise hides, \
     because it reproduces the same token with the same span and the same post-token state"
  );
  assert_eq!(
    direct.commits, 2,
    "one committed-token notification per ACCEPTED token, and none for the stopper: an unwind is \
     not a settle"
  );
  assert_eq!(direct.live_rows, 0);

  // The same call under a cache that retains its front: the put-back is a cache push rather than a
  // park, and everything a caller reads is the same either way.
  let (direct, scanned) =
    both_routes_under_an_unwinding_predicate::<Deque3>(Fixture::clean("~ ~ ab cd"), 0, 3);
  assert_eq!(direct, scanned);
  assert!(
    !direct.parked,
    "a cache with room takes the put-back, so nothing parks"
  );
  assert_eq!(direct.depth, 1, "…and it is resident there instead");
  assert_eq!(direct.cursor, 4);
  assert_eq!(
    direct.drain_lexed, 1,
    "the stopper is served out of the cache, not produced again"
  );
  assert_eq!(direct.commits, 2);

  // ── The same window, entered from the residency only a decline can build ──
  // `"~ ab"` with the front parked by `try_expect(|_| false)`: call 1 is the parked trivia token,
  // which both routes consume, and call 2 is the freshly-lexed word — the first token this call
  // ever holds outside the stream.
  let (direct, scanned) = both_routes_over_a_parked_front_under_an_unwinding_predicate::<NoCache>(
    Fixture::clean("~ ab"),
    2,
  );
  assert_eq!(
    direct, scanned,
    "the two routes must observe the same INTERRUPTED skip over a parked front"
  );
  assert!(direct.parked_in, "the front is the parked slot going in");
  assert!(direct.caught);
  assert_eq!(direct.asked, std::vec![(0, 1), (2, 4)]);
  assert_eq!(direct.span, (0, 1), "the parked trivia token is committed");
  assert_eq!(
    direct.cursor, 2,
    "and the stopper is parked in its place: the resume cursor is its start, not the committed \
     end (1)"
  );
  assert!(direct.parked, "one token parked going out, as going in");
  assert_eq!(direct.depth, 1);
  assert_eq!(
    direct.drained,
    std::vec![(2, 4)],
    "the stopper is the last token, so it is the whole drain…"
  );
  assert_eq!(
    direct.drain_lexed, 0,
    "…and the drain lexes NOTHING to produce it: dropped, it would be re-lexed and this would be 1"
  );
  assert_eq!(direct.commits, 1);
  assert_eq!(direct.live_rows, 0);
}

/// The committed-token notifications one route makes over a parked front, split by what made them.
#[derive(Debug, PartialEq)]
struct ParkedCommits {
  /// The decline that parks the token. `unconsume` is **not** a settle, so this is zero — and it
  /// is measured rather than assumed, because the notification is a side channel with no other
  /// observer.
  parking: usize,
  /// The skip that follows it.
  skipping: usize,
  /// …and what was actually consumed, so a route that fires the right NUMBER of notifications
  /// over the wrong tokens is still red.
  span: (usize, usize),
  cursor: usize,
  drained: std::vec::Vec<(usize, usize)>,
}

fn ledger_parked_front_commits<'a, C, Cmpl>(
  inp: &mut InputRef<'a, '_, LedgerLexer<'a>, (LedgerEmitter, C), (), Cmpl>,
) -> ParkedCommits
where
  C: Cache<'a, LedgerLexer<'a>, ()>,
  Cmpl: crate::input::SurfaceIncomplete<'a, LedgerLexer<'a>, (LedgerEmitter, C), ()>,
{
  record();
  park_front(inp);
  let parking = commit_notifications(&recorded());

  record();
  inp
    .skip_while(|t| t.data.is_trivia())
    .expect("the fixtures here lex cleanly");
  let skipping = commit_notifications(&recorded());

  let cursor = inp.cursor().as_inner().0;
  let span = (inp.span().start.0, inp.span().end.0);
  let mut drained = std::vec::Vec::new();
  while let Ok(Some(tok)) = inp.next() {
    drained.push((tok.span_ref().start.0, tok.span_ref().end.0));
  }

  ParkedCommits {
    parking,
    skipping,
    span,
    cursor,
    drained,
  }
}

fn both_routes_parked_front_commits<K>(fixture: Fixture) -> (ParkedCommits, ParkedCommits)
where
  K: Capacity,
{
  let src = LedgerSrc(fixture.text);
  let _budget = Budget::unlimited();

  let mut complete = ledger_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  let direct = {
    let mut inp = complete.as_ref();
    ledger_parked_front_commits(&mut inp)
  };

  let mut partial = ledger_partial_input_with::<K::Cache<'_>>(&src, fixture.emitter());
  partial.seal();
  let scanned = {
    let mut inp = partial.as_ref();
    ledger_parked_front_commits(&mut inp)
  };

  (direct, scanned)
}

/// **The committed-token side channel, over a parked front** — the cell
/// [`the_two_completeness_routes_observe_the_same_skip_over_a_parked_front`] says it cannot be.
///
/// That cell's own closing note is the specification for this one. [`SkipRun`] is the caller-visible
/// *values* a skip leaves behind, and [`Emitter::commit_token`](crate::Emitter::commit_token) is not
/// one of them: it is recorded by the ledger emitter as [`Effect::CommitToken`] and read back by no
/// cell in this file. Measured there: removing the notification from **both** sites a parked token's
/// consumption can reach — the complete-input settle (`settle_committed_token`) and the scanner's
/// `skip_and_report` — left every test in this file green, that cell included. A recording CST sink
/// would have lost a token with nothing red anywhere.
///
/// This closes it, on the residency that needs it most: a parked token is the one head the two
/// routes consume through **different** code — `front` + `commit_front` on the complete route, the
/// `Origin::Parked` fetch and `AtFrontier::adopt` on the scanned one — so it is the one head whose
/// notification could go missing on one route and not the other.
///
/// Three facts, and the middle one is the whole point:
///
/// * the **decline that parks** notifies nothing. `unconsume` is a put-back, not a settle, and
///   SETTLE_CENSUS lists it among the non-settles; this measures that rather than restating it;
/// * the **skip** notifies exactly once per token it consumed, on both routes;
/// * and the tokens it consumed are the right ones, so a route that fires the right count over the
///   wrong stream is still red.
///
/// Red against a deliberate breakage of **either** site, measured rather than asserted — each
/// removal made, this cell run, and the edit reverted:
///
/// | the notification removed from | what this cell reports |
/// |---|---|
/// | `settle_committed_token` (the complete route's settle) | `skipping: 0` against `1` — the complete route stops feeding the channel and the scanned one still does |
/// | `skip_and_report` (the scanned route's skip settle) | `skipping: 1` against `0` — the mirror image |
///
/// The second is the isolating one, and it is what makes the gap concrete: over that edit **every
/// other cell in this file stays green** (25 passed, 2 failed — this cell and
/// [`the_two_completeness_routes_observe_the_same_unwound_skip`], which reads the same channel).
/// The first is caught more widely, and saying so is more honest than claiming it is not:
/// `settle_committed_token` is every 1:1 consume settle in the crate, so removing it also reds
/// twenty cells in `cst::sink`, `input_ref::tests` and the census. Neither fact is a substitute for
/// the other — the parked-front path is reachable through both sites, and this cell is the only
/// thing in the file that watches either of them.
#[test]
fn the_committed_token_side_channel_is_fed_over_a_parked_front() {
  // ── accepted: the parked token is trivia, so the skip consumes it ──
  let (direct, scanned) = both_routes_parked_front_commits::<NoCache>(Fixture::clean("~ ab"));
  assert_eq!(
    direct,
    scanned,
    "the complete-input route and the scanned partial route must feed the committed-token side \
     channel identically over an ACCEPTED parked front under {}",
    NoCache::NAME
  );
  assert_eq!(
    direct,
    ParkedCommits {
      parking: 0,
      skipping: 1,
      span: (0, 1),
      cursor: 2,
      drained: std::vec![(2, 4)],
    },
    "the decline that parks the token notifies nothing — it consumed nothing — and the skip that \
     consumes it notifies exactly once, for that one token"
  );

  // ── rejecting: the parked token is a word, so nothing is consumed and nothing is notified ──
  let (direct, scanned) = both_routes_parked_front_commits::<NoCache>(Fixture::clean("ab cd"));
  assert_eq!(
    direct,
    scanned,
    "…and identically over a REJECTING parked front under {}",
    NoCache::NAME
  );
  assert_eq!(
    direct,
    ParkedCommits {
      parking: 0,
      skipping: 0,
      span: (0, 0),
      cursor: 0,
      drained: std::vec![(0, 2), (3, 5)],
    },
    "a skip that consumes nothing notifies nothing: the side channel is keyed on the settle, not \
     on the call"
  );
}
