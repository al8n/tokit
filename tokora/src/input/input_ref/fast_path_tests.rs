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

use generic_arraydeque::typenum::U3;

use crate::{
  InputRef, Token,
  cache::{Cache, CachedTokenOf, DefaultCache},
  emitter::Verbose,
  input::Input,
  span::SimpleSpan,
  state::token_tracker::TokenLimiter,
};

use super::tests::{BalKind, BalLexer, ByValErr};

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
