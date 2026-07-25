//! The named-regression suite of the rewindable event sink: the failure-corpus scenarios
//! (F-A1/F-A2/F-A3/F-A5, T3) at the mechanism level, the unified-log exactness laws, and
//! the CST_FORWARD_CENSUS source lock.

use core::num::NonZeroU32;

use crate::{
  Lexer, SimpleSpan,
  cache::DefaultCache,
  cst::event::{Event, TOMBSTONE},
  emitter::{CstEmitter, Emitter, Fatal, Verbose},
  error::token::{UnexpectedToken, UnexpectedTokenOf},
  input::{Balance, Cursor, Input},
  span::Spanned,
  token::Token,
};

use super::Sink;

// ── A tiny real lexer: one byte per token, `!` is a lexer error ─────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MiniTok(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MiniErr;

impl Token<'_> for MiniTok {
  type Kind = u8;
  type Error = MiniErr;

  // honest: byte-per-token, never skips a byte
  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> u8 {
    self.0
  }

  fn is_trivia(&self) -> bool {
    self.0 == b' '
  }
}

struct MiniLexer<'inp> {
  src: &'inp str,
  tok_start: usize,
  pos: usize,
  state: (),
}

impl<'inp> Lexer<'inp> for MiniLexer<'inp> {
  type State = ();
  type Source = str;
  type Token = MiniTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp str) -> Self {
    Self {
      src,
      tok_start: 0,
      pos: 0,
      state: (),
    }
  }

  fn with_state(src: &'inp str, state: ()) -> Self {
    Self {
      src,
      tok_start: 0,
      pos: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), MiniErr> {
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

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.tok_start, self.pos)
  }

  fn slice(&self) -> &'inp str {
    &self.src[self.tok_start..self.pos]
  }

  fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
    let byte = *self.src.as_bytes().get(self.pos)?;
    self.tok_start = self.pos;
    self.pos += 1;
    if byte == b'!' {
      Some(Err(MiniErr))
    } else {
      Some(Ok(MiniTok(byte)))
    }
  }

  fn bump(&mut self, n: &usize) {
    self.pos += *n;
    self.tok_start = self.pos;
  }
}

// ── The test error type (FromEmitterError via the blanket) ─────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestErr {
  Lex,
  Unexpected,
  Custom(u8),
}

impl From<MiniErr> for TestErr {
  fn from(_: MiniErr) -> Self {
    Self::Lex
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for TestErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Self::Unexpected
  }
}

// The remaining conversions the atomic emitter traits' blanket `From*Error` impls ask of a
// bundle's error type — the full-family conformance test below names every trait.
const _: () = {
  use crate::error::{
    UnexpectedEoLhs, UnexpectedEoRhs,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError},
  };

  impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for TestErr {
    fn from(_: TooFew<S, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for TestErr {
    fn from(_: TooMany<S, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for TestErr {
    fn from(_: FullContainer<S, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for TestErr {
    fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for TestErr {
    fn from(_: MissingSyntax<O, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for TestErr {
    fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<O, Lang: ?Sized> From<UnexpectedEoLhs<O, Lang>> for TestErr {
    fn from(_: UnexpectedEoLhs<O, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<O, Lang: ?Sized> From<UnexpectedEoRhs<O, Lang>> for TestErr {
    fn from(_: UnexpectedEoRhs<O, Lang>) -> Self {
      Self::Unexpected
    }
  }
};

// ── Dialect fixture: the unified kind space and the mapper ─────────────────────

const K_NODE: u16 = 2;
const K_LIST: u16 = 3;
const K_WRAP: u16 = 4;
const K_TOK: u16 = 10;
const K_ERR: u16 = 90;
const K_GAP: u16 = 91;

fn map_tok(_: &MiniTok) -> u16 {
  K_TOK
}

type VerboseSink<'inp> = Sink<'inp, MiniLexer<'inp>, Verbose<TestErr>>;
type FatalSink<'inp> = Sink<'inp, MiniLexer<'inp>, Fatal<TestErr>>;

fn verbose_sink<'inp>() -> VerboseSink<'inp> {
  Sink::new(Verbose::new(), map_tok, K_ERR, K_GAP)
}

fn span(start: usize, end: usize) -> SimpleSpan {
  SimpleSpan::new(start, end)
}

/// Drives the sink's `Emitter::rewind` directly, the way the input layer does at a restore.
fn rewind(sink: &mut VerboseSink<'_>, mark: u64) {
  let origin = 0usize;
  Emitter::<MiniLexer<'_>>::rewind(sink, Cursor::from_ref(&origin), mark);
}

fn emit_error(sink: &mut VerboseSink<'_>, at: usize, tag: u8) {
  Emitter::<MiniLexer<'_>>::emit_error(sink, Spanned::new(span(at, at + 1), TestErr::Custom(tag)))
    .expect("verbose emitters collect");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Emission shapes
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn emissions_buffer_in_order() {
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish();
  assert_eq!(
    sink.events(),
    &[
      Event::StartNode {
        kind: K_NODE,
        forward_parent: None
      },
      Event::Token {
        kind: K_TOK,
        span: span(0, 1)
      },
      Event::FinishNode,
    ]
  );
}

#[test]
fn mark_appends_an_inert_tombstone() {
  let mut sink = verbose_sink();
  let mark = sink.cst_mark();
  assert_eq!(mark.index(), 0);
  assert_eq!(
    sink.events(),
    &[Event::StartNode {
      kind: TOMBSTONE,
      forward_parent: None
    }]
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// F-A5 — stale marks panic in every build (the savepoint posture)
// ═══════════════════════════════════════════════════════════════════════════════

/// A rewind truncates the tombstone; unrelated events regrow over its index; spending the
/// mark must panic, not wrap the regrown region.
#[test]
#[should_panic(expected = "stale EventMark")]
fn stale_mark_spend_panics_after_truncate_and_regrow() {
  let mut sink = verbose_sink();
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  rewind(&mut sink, ckp);
  // Regrow: a token now occupies the mark's old index.
  sink.cst_token(&MiniTok(b'b'), &span(0, 1));
  sink.cst_start_at(mark, K_WRAP);
}

/// The sharpest alias: the regrown event at the mark's index is ANOTHER tombstone, so the
/// positional check alone would validate it — only the era distinguishes the histories.
#[test]
#[should_panic(expected = "stale EventMark")]
fn stale_mark_panics_even_over_a_regrown_tombstone() {
  let mut sink = verbose_sink();
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  let dead = sink.cst_mark();
  rewind(&mut sink, ckp);
  // Regrow a fresh tombstone at the very same index.
  let live = sink.cst_mark();
  assert_eq!(live.index(), dead.index());
  sink.cst_start_at(dead, K_WRAP);
}

/// An inert mark (a diagnostics-only emitter's defaulted `cst_mark`) can never spend on a
/// recording sink.
#[test]
#[should_panic(expected = "EventMark")]
fn inert_mark_spend_panics() {
  let mut fatal = Fatal::<TestErr>::new();
  let inert = CstEmitter::<MiniLexer<'_>>::cst_mark(&mut fatal);
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_start_at(inert, K_WRAP);
}

/// A mark minted by one sink must not validate on another, even when the foreign sink has
/// a live tombstone at the same index and era — the exact `(index: 0, era: 0)` collision
/// two fresh sinks mint. The witness runs in **every build**: this is the release-mode
/// regression (`cargo test --release`) for the cross-sink spend that the positional and
/// era checks alone would silently accept, wrapping an unrelated history.
#[test]
#[should_panic(expected = "different sink")]
fn foreign_sink_mark_panics() {
  let mut a = verbose_sink();
  let mut b = verbose_sink();
  let mark_a = a.cst_mark();
  let mark_b = b.cst_mark();
  assert_eq!(mark_a.index(), mark_b.index());
  assert_eq!(mark_a.era(), mark_b.era());
  b.cst_start_at(mark_a, K_WRAP);
}

/// The witness counter can never wrap and reissue: a wrap of `usize::MAX` back to `0` is
/// the inert-mark id, and reissuing prior ids lets a foreign mark validate on an unrelated
/// sink — the exact wrong-tree class the witness exists to kill. The primitive is tested at
/// its boundary directly (set to `MAX`, the next allocation aborts rather than roll over);
/// constructing 2^{32,64} sinks is not feasible, so the counter itself is the unit here.
#[test]
#[should_panic(expected = "witness counter exhausted")]
fn witness_counter_aborts_before_wrapping() {
  use core::sync::atomic::AtomicUsize;
  let counter = AtomicUsize::new(usize::MAX);
  let _ = super::bump_witness(&counter);
}

/// The legal counterpart: rewinds strictly above a mark leave it spendable forever (the
/// pratt shape — an entry mark surviving per-iteration rollbacks).
#[test]
fn mark_survives_rewinds_strictly_above_it() {
  let mut sink = verbose_sink();
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  rewind(&mut sink, ckp);
  sink.cst_token(&MiniTok(b'c'), &span(1, 2));
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish();
  assert_eq!(
    sink.forward_parent_at(0),
    NonZeroU32::new(3),
    "the wrap landed on the surviving tombstone (StartAt at index 3, target 0)"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// F-A2 / F-A3 — the forward_parent write dies to journal + era
// ═══════════════════════════════════════════════════════════════════════════════

/// F-A2 (the dangle): a wrap inside a to-be-declined branch writes the tombstone's
/// forward_parent; the decline truncates the StartAt but the write targets a pre-mark
/// slot — only the journal's reverse-replay restores it. Without it, the pointer dangles
/// above the truncation.
#[test]
fn rewind_reverses_the_forward_parent_write() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);

  // The speculative wrap: StartAt + finish, with the journaled fp write onto index 1.
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish();
  assert_eq!(sink.forward_parent_at(1), NonZeroU32::new(2));
  assert_eq!(sink.journal_len(), 1);

  // The decline: truncation must reverse the interior write, not just drop the suffix.
  rewind(&mut sink, ckp);
  assert_eq!(
    sink.forward_parent_at(1),
    None,
    "the journaled forward_parent write survived the rewind (F-A2's dangling pointer)"
  );
  assert_eq!(sink.journal_len(), 0);
  assert_eq!(sink.events().len(), ckp as usize);
}

/// F-A3 (the steal): after the decline, the retry opens an unrelated node and parses on.
/// With the write reversed, nothing ties the retry's events to the abandoned wrap — the
/// tombstone is pristine and no StartAt names it.
#[test]
fn regrown_branch_cannot_inherit_a_dead_wrap() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish();
  rewind(&mut sink, ckp);

  // The retry: an unrelated List over the next token.
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'd'), &span(2, 3));
  sink.cst_finish();

  assert_eq!(
    sink.forward_parent_at(1),
    None,
    "the dead wrap leaked into the retry's timeline (F-A3's stolen start)"
  );
  assert!(
    !sink
      .events()
      .iter()
      .any(|ev| matches!(ev, Event::StartAt { .. })),
    "no StartAt survives the decline"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// T3 — release pops the kept capture: the mark stack holds exactly the live rows
// ═══════════════════════════════════════════════════════════════════════════════

/// Through the public `attempt` API (the T3 repro shape): committed attempts release
/// their capture, declined attempts rewind it — the stack never grows across a
/// commit-heavy loop, so a stale row can never alias a fresh capture at the same length.
#[test]
fn release_keeps_the_mark_stack_at_live_captures() {
  type Ctx<'inp> = (VerboseSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);
  let mut sink = verbose_sink();
  let mut input = Input::<'_, MiniLexer<'_>, Ctx<'_>, ()>::new("abcdef");
  {
    let mut inp = input.as_ref(&mut sink);

    // T3's alias shape first: a declined attempt and a committed attempt capture at the
    // SAME buffer length (the u64s alias); the stack must spend each capture at its
    // settle, leaving no row either time.
    let declined: Option<()> = inp.attempt(|inp| {
      let _ = inp.next();
      None
    });
    assert!(declined.is_none());
    let committed: Option<()> = inp.attempt(|inp| inp.next().ok().flatten().map(|_| ()));
    assert!(committed.is_some());

    // Then the commit-heavy loop.
    for _ in 0..4 {
      let _ = inp.attempt(|inp| inp.next().ok().flatten().map(|_| ()));
    }
  }
  assert_eq!(
    sink.rows_len(),
    0,
    "kept captures must be released (T3: a stranded row is a stale alias for the next \
     same-length capture)"
  );
}

/// The release no-growth oracle applied to the **session-abandon** path — the W11
/// pin-leak class one layer up. A handle dropped with open session points releases their
/// pins and lineage entries (`Session`'s drop); it must release their **emitter marks**
/// too, or a long-lived sink strands one `MarkRow` per abandoned begin_point/drop cycle.
/// And per the no-rollback-on-drop law, the release reclaims bookkeeping only: the
/// progress committed through the open point — its token events — stays.
#[test]
fn abandoned_session_points_release_their_emitter_marks() {
  type Ctx<'inp> = (VerboseSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);
  let mut sink = verbose_sink();
  let mut input = Input::<'_, MiniLexer<'_>, Ctx<'_>, ()>::new("abcdef");

  for cycle in 0..3 {
    {
      let mut inp = input.as_ref(&mut sink);
      let _point = inp.begin_point();
      let _ = inp.next().expect("verbose collects").expect("a token");
      // …and the handle dies here with the point still open.
    }
    assert_eq!(
      sink.rows_len(),
      0,
      "cycle {cycle}: an abandoned session point must release its emitter mark row, \
       exactly as it releases its pin and lineage entry"
    );
  }

  // No rollback rode along with the release: every token consumed through the abandoned
  // points is still on the event buffer.
  assert_eq!(
    sink
      .events()
      .iter()
      .filter(|ev| matches!(ev, Event::Token { .. }))
      .count(),
    3,
    "drop released bookkeeping, not progress: the settled tokens survive"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// F-A1 (issue #98) — the narrowed cst_finish assert: true global underflow
// still panics; the genuinely-legal cross-checkpoint close no longer does; and the
// leaked-finish misuse shape is a documented, depth-indistinguishable limitation
// (see the `Sink::cst_finish` contract comment)
// ═══════════════════════════════════════════════════════════════════════════════

/// A finish with no open node anywhere is true global underflow: detect at cause in
/// debug builds. The narrowed assert leaves this case unchanged — baseline and global
/// depth already agreed at zero here, with no live checkpoint in play.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "no open node")]
fn orphan_finish_debug_asserts_at_emission() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish();
}

/// This pins a KNOWN LIMITATION, not a legality guarantee. This is the
/// leaked-finish MISUSE shape: `cst_start(A); checkpoint m; cst_start(B); rewind(m);
/// cst_token; cst_finish`. The finish was meant for B, but B's start died with the rewind,
/// so it silently closes ancestor A. After the rewind the event buffer is IDENTICAL to a
/// legal `A`-close (`cst_finish_across_a_live_checkpoint_is_legal_and_materializes`) —
/// depth cannot tell them apart, nor can the whole buffer; only the intended opener's
/// identity on `cst_finish` could. So this builds a balanced (wrong-node) tree with no
/// error: `FinishError::OrphanFinish` fires only on TRUE underflow, never on this shape.
/// This test pins that gap: the narrowed `> 0` assert (correctly) no longer
/// panics here, and materialization (correctly, given depth-only info) does not wall it.
/// In this coded scenario the rewind spends the only checkpoint, so at finish time no
/// mark row is live: baseline reads 0 and both the old (> innermost-live-frozen-baseline)
/// and the new (> 0) predicates pass it. The two asserts differ ONLY when a checkpoint row
/// is still LIVE at finish — the legal cross-checkpoint close in
/// `cst_finish_across_a_live_checkpoint_is_legal_and_materializes`. This
/// test therefore pins the balanced-wrong-node materialization outcome and the
/// depth-indistinguishability principle, not a behavior the old assert uniquely caught.
/// Catching the misuse at cause needs an opener-identity API change on `cst_finish`
/// (deferred).
#[test]
fn finish_after_a_rewound_start_closes_the_ancestor_indistinguishably() {
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start(K_LIST); // the start this finish was meant to close …
  rewind(&mut sink, ckp); //  … rolled back: K_LIST never existed
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(); // leaked: silently closes the ancestor K_NODE instead
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  // No error, and the tree is balanced-but-wrong: the finish closed K_NODE, which reads
  // exactly like a legal K_NODE close. The misuse-detection gap, pinned.
  let root = tree(green.expect("balanced (wrong-node) tree — the misuse is not walled"));
  assert_eq!(root.first_child().expect("Root[Node]").kind(), K_NODE);
}

/// The genuinely-legal case that must not panic — a node whose start was emitted and
/// NEVER rolled back, closed across a still-live checkpoint: `cst_start(A); checkpoint m;
/// cst_token; cst_finish` (balanced, non-leaked). Under commit/release both events survive
/// balanced; under a rewind of `m` this very finish is what truncates and `A` reopens (the
/// contract's blessed truncate-and-reopen semantics — not exercised here, see the
/// rewind-recovery tests elsewhere in this file). The old assert panicked on it; the
/// narrowed assert must pass in both debug and release and materialize `Root[Node]`.
#[test]
fn cst_finish_across_a_live_checkpoint_is_legal_and_materializes() {
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish();
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  let root = tree(green.expect("a balanced stream materializes"));
  assert_eq!(root.first_child().expect("Root[Node]").kind(), K_NODE);
}

/// The narrowing must still catch a *genuine* global underflow —
/// closing when nothing is open anywhere — in every debug build, checkpoint or not (the
/// protection the narrowing must preserve).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "global underflow")]
fn cst_finish_debug_asserts_on_genuine_global_underflow_across_a_checkpoint() {
  let mut sink = verbose_sink();
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(); // nothing was ever open — global underflow, checkpoint or not
}

// ═══════════════════════════════════════════════════════════════════════════════
// The unified log — one mark governs both channels
// ═══════════════════════════════════════════════════════════════════════════════

/// Rewind recovers the inner emitter's state from the mark-stack row captured at
/// `checkpoint` — the row snapshots `inner.checkpoint()` when the mark is taken, and
/// `rewind` hands that exact reading back to the inner: exactly the diagnostics recorded
/// below the mark survive, on values not guesses.
#[test]
fn rewind_recovers_the_inner_mark_from_the_mark_row() {
  let mut sink = verbose_sink();
  emit_error(&mut sink, 0, 1);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  emit_error(&mut sink, 1, 2);
  emit_error(&mut sink, 2, 3);
  assert_eq!(sink.inner_ref().errors().len(), 3);

  rewind(&mut sink, ckp);
  let errors = sink.inner_ref().errors();
  assert_eq!(
    errors.values().map(|group| group.len()).sum::<usize>(),
    1,
    "exactly the pre-mark diagnostic survives"
  );
  assert!(errors.contains_key(&span(0, 1)));
}

/// With no Diag slot below the mark, recovery falls back to the sink's base reading — the
/// inner returns to its construction-time state.
#[test]
fn rewind_to_origin_recovers_the_base_inner_mark() {
  let mut sink = verbose_sink();
  emit_error(&mut sink, 0, 1);
  emit_error(&mut sink, 1, 2);
  rewind(&mut sink, 0);
  assert!(sink.inner_ref().errors().is_empty());
  assert_eq!(sink.events().len(), 0);
}

/// Record-then-propagate: the Diag slot lands on the `Err` edge too, so a fatal unwind's
/// guard-driven rewind still sees an exact log.
#[test]
fn diag_slot_lands_on_the_err_edge_too() {
  let mut sink: FatalSink<'_> = Sink::new(Fatal::new(), map_tok, K_ERR, K_GAP);
  let verdict =
    Emitter::<MiniLexer<'_>>::emit_error(&mut sink, Spanned::new(span(0, 1), TestErr::Custom(9)));
  assert!(verdict.is_err(), "fatal emitters reject");
  assert_eq!(
    sink.events().len(),
    1,
    "the forwarded diagnostic occupies its Diag slot even when the inner verdict is Err"
  );
  assert!(matches!(sink.events()[0], Event::Diag { .. }));
}

/// Labels forward to the inner emitter (they are scope state, not emissions — no Diag
/// slot), and the inner snapshots them into its own entries as usual.
#[test]
fn labels_forward_without_diag_slots() {
  let mut sink = verbose_sink();
  Emitter::<MiniLexer<'_>>::enter_label(&mut sink, "field");
  assert_eq!(sink.events().len(), 0, "a label is not an emission");
  emit_error(&mut sink, 0, 1);
  Emitter::<MiniLexer<'_>>::exit_label(&mut sink);
  let labels = sink.inner_ref().labels();
  assert_eq!(labels[&span(0, 1)][0], std::vec!["field"]);
}

/// An out-of-range mark is ignored outright — a total no-op, never a panic. (It must NOT
/// clamp: clamping to the length would spend a live row at the current mark;
/// see `out_of_range_rewind_spends_no_live_row`.)
#[test]
fn rewind_ignores_out_of_range_marks() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  rewind(&mut sink, u64::MAX);
  assert_eq!(
    sink.events().len(),
    1,
    "an out-of-range rewind truncates nothing"
  );
}

/// A rewind to the current length spends the capture's row but truncates nothing — the
/// era does not bump, so previously issued marks stay live.
#[test]
fn rewind_to_current_mark_is_truncation_free() {
  let mut sink = verbose_sink();
  let mark = sink.cst_mark();
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  assert_eq!(sink.rows_len(), 1);
  rewind(&mut sink, ckp);
  assert_eq!(sink.rows_len(), 0, "the capture was spent");
  // The mark predates the (no-op) rewind and must still spend cleanly.
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Checkpoint rows — the frozen depth ledger
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn checkpoint_rows_freeze_derived_depth() {
  let mut sink = verbose_sink();
  let first = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start(K_NODE);
  let second = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  assert_eq!(sink.rows_len(), 2);

  // Kept captures release newest-first (the LIFO settle order).
  Emitter::<MiniLexer<'_>>::release(&mut sink, second);
  Emitter::<MiniLexer<'_>>::release(&mut sink, first);
  assert_eq!(sink.rows_len(), 0);

  // The released rows became the derived-depth floor; the balance still closes.
  sink.cst_finish();
  assert_eq!(sink.events().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// The hole wrap — recovery skips become error nodes over the REAL tokens
// ═══════════════════════════════════════════════════════════════════════════════

/// The wrap brackets exactly the hole's buffered token events — interleaved Diag slots
/// ride inside, tokens outside the hole span stay outside.
#[test]
fn hole_wrap_brackets_the_buffered_suffix() {
  let mut sink = verbose_sink();
  // A committed token BEFORE the hole: outside the wrap.
  sink.cst_token(&MiniTok(b'x'), &span(0, 1));
  // The hole's tokens, with a crossed lexer error between them (a Diag slot).
  sink.cst_token(&MiniTok(b'a'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  sink.cst_token(&MiniTok(b'b'), &span(3, 4));

  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 4), 2)
    .expect("verbose collects");

  assert_eq!(
    sink.events(),
    &[
      Event::Token {
        kind: K_TOK,
        span: span(0, 1)
      },
      Event::StartNode {
        kind: K_ERR,
        forward_parent: None
      },
      Event::Token {
        kind: K_TOK,
        span: span(1, 2)
      },
      Event::Diag {
        error_span: Some(span(2, 3))
      },
      Event::Token {
        kind: K_TOK,
        span: span(3, 4)
      },
      Event::FinishNode,
      Event::Diag { error_span: None },
    ]
  );
}

/// A zero-skip hole produces no node (and the crate's caller never even emits one).
#[test]
fn zero_skip_hole_makes_no_node() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 1), 0).expect("collects");
  assert_eq!(sink.events().len(), 2, "one token, one Diag — no wrap");
  assert!(matches!(sink.events()[1], Event::Diag { .. }));
}

/// A hole with no buffered token events (no auto-emission wired, or a direct call) has
/// nothing to wrap: no node, just the forwarded diagnostic.
#[test]
fn tokenless_hole_makes_no_node() {
  let mut sink = verbose_sink();
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(0, 4), 3).expect("collects");
  assert_eq!(sink.events().len(), 1);
  assert!(matches!(sink.events()[0], Event::Diag { .. }));
}

/// The wrap survives a later rewind like any other events: a checkpoint below the hole
/// unwinds wrap and tokens together.
#[test]
fn hole_wrap_rewinds_with_the_log() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'x'), &span(0, 1));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'a'), &span(1, 2));
  sink.cst_token(&MiniTok(b'b'), &span(2, 3));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 3), 2).expect("collects");
  assert_eq!(sink.events().len(), 6);
  rewind(&mut sink, ckp);
  assert_eq!(sink.events().len(), 1, "wrap, tokens, and Diag all unwind");
}

// ═══════════════════════════════════════════════════════════════════════════════
// The &mut threading shape — events flow through the blanket impl
// ═══════════════════════════════════════════════════════════════════════════════

/// The parse_partial round-threading configuration: a `&mut Sink<E>` in the emitter
/// seat. Every event lands in the sink through the blanket forward.
#[test]
fn mut_ref_sink_records_events() {
  let mut sink = verbose_sink();
  {
    let mut threaded: &mut VerboseSink<'_> = &mut sink;
    CstEmitter::<MiniLexer<'_>>::cst_start(&mut threaded, K_NODE);
    let tok = MiniTok(b'a');
    let sp = span(0, 1);
    CstEmitter::<MiniLexer<'_>>::cst_token(&mut threaded, &tok, &sp);
    let mark = CstEmitter::<MiniLexer<'_>>::cst_mark(&mut threaded);
    CstEmitter::<MiniLexer<'_>>::cst_start_at(&mut threaded, mark, K_WRAP);
    CstEmitter::<MiniLexer<'_>>::cst_finish(&mut threaded);
    CstEmitter::<MiniLexer<'_>>::cst_finish(&mut threaded);
  }
  assert_eq!(sink.events().len(), 6);
}

// ═══════════════════════════════════════════════════════════════════════════════
// CST_FORWARD_CENSUS — the source lock on the one-helper discipline
// ═══════════════════════════════════════════════════════════════════════════════

/// Counts occurrences of `needle` on the non-comment lines of the sink source.
fn count(hay: &str, needle: &str) -> usize {
  hay
    .lines()
    .filter(|line| !line.trim_start().starts_with("//"))
    .map(|line| line.matches(needle).count())
    .sum()
}

/// CST_FORWARD_CENSUS — every forwarded diagnostic of every implemented emitter trait
/// routes through the ONE helper (`forward_diag`), which records the Diag slot on Ok and
/// Err alike. A new atomic emitter trait that bypasses it is the one bug class this law
/// exists to prevent: its diagnostics would reach the inner emitter without occupying a
/// log position, skewing every later rewind recovery.
#[test]
fn cst_forward_census_one_helper_carries_every_channel() {
  let src = include_str!("../sink.rs");

  // The forwarded channels: 5 core Emitter + TooFew/TooMany/FullContainer +
  // SeparatedEmitter (2) + the 4 leading/trailing refinements + PrattEmitter (2).
  let calls = count(src, "self.forward_diag::<");
  assert!(
    calls == 16,
    "CST_FORWARD_CENSUS drift: {calls} forward_diag call sites, expected 16. A new \
     forwarded channel must route through the one helper AND bump this census in the \
     same commit (grep CST_FORWARD_CENSUS)."
  );
  assert!(
    count(src, "fn forward_diag") == 1,
    "CST_FORWARD_CENSUS drift: the helper must be defined exactly once"
  );

  // No emit bypasses the helper: the only `self.inner` touches are the helper's own
  // closure seam, the settle forward (`commit_token`), the two label scope calls, the
  // rewind recovery, and the read-only reaches (base reading, accessor, Debug).
  assert!(
    count(src, "self.inner.emit") == 0,
    "CST_FORWARD_CENSUS drift: a diagnostic is forwarded outside the census helper"
  );
  assert!(
    count(src, "self.inner.rewind(") == 1,
    "CST_FORWARD_CENSUS drift: the inner emitter rewinds exactly once, in the sink's \
     rewind recovery"
  );
  assert!(
    count(src, "self.inner.enter_label(") == 1 && count(src, "self.inner.exit_label(") == 1,
    "CST_FORWARD_CENSUS drift: labels forward directly (scope state, not emissions) — \
     exactly once each"
  );

  // The settle channel forwards exactly once, and only from `commit_token`.
  assert!(
    count(src, "self.inner.commit_token(") == 1,
    "CST_FORWARD_CENSUS drift: the settle channel forwards exactly once, in \
     Emitter::commit_token (cst_token is raw event transport and must NOT fabricate a settle)"
  );
  // Option A: the sink NEVER forwards release — inner checkpoints are value-keyed readings.
  assert!(
    count(src, "self.inner.release") == 0,
    "CST_FORWARD_CENSUS drift: the sink never forwards release — inner checkpoints are \
     value-keyed READINGS needing no reclamation (see the Inner-emitter contract on Sink). \
     Forwarding would deliver duplicate and out-of-LIFO releases under raw mixes."
  );
  // The inner's reading is captured in exactly two places: the mark-stack row and the base.
  assert!(
    count(src, "self.inner.checkpoint()") == 1 && count(src, "checkpoint(&self.inner)") == 1,
    "CST_FORWARD_CENSUS drift: the inner's reading is captured in exactly two places — the \
     mark-stack row (sink checkpoint) and the base prime (base_inner_mark)"
  );
  // Every inner-advancing surface primes the base first: forward_diag + commit_token, plus
  // the rewind fallback read = 3.
  assert!(
    count(src, "self.base_inner_mark") == 3,
    "CST_FORWARD_CENSUS drift: every inner-advancing surface primes the base first — \
     forward_diag (emissions) and commit_token (settles) — plus the rewind fallback read"
  );
}

/// CST_COMPOSITION_CENSUS — the R3-class tripwire: every method of the emitter trait family
/// must be OVERRIDDEN by `Sink`, never left to a trait default. A defaulted inherit
/// silently severs that channel for wrapped inners (exactly how the `commit_token` R3 gap
/// happened). Two halves: (a) every one of the 27 inventory names appears as an impl in
/// sink.rs; (b) drift tripwires on the trait definitions, so any NEW family method forces a
/// classification (override + forward, or a documented inherit) in the same commit.
///
/// GREEN at 123f840 — the audit's proof that beyond Findings 1 and 2 no third per-method gap
/// exists — and it stays as the permanent tripwire.
#[test]
fn cst_composition_census_every_family_method_is_overridden() {
  let src = include_str!("../sink.rs");

  // (a) The 27-method inventory: 11 core Emitter + 5 CstEmitter + 11 capability emit_*.
  // Each must appear as an `fn <name>` impl in the sink; a missing one is a severed channel.
  let overridden = [
    // 11 core Emitter
    "emit_lexer_error",
    "emit_unexpected_token",
    "emit_error",
    "emit_warning",
    "emit_skipped_region",
    "checkpoint",
    "rewind",
    "release",
    "commit_token",
    "enter_label",
    "exit_label",
    // 5 CstEmitter
    "cst_start",
    "cst_token",
    "cst_finish",
    "cst_mark",
    "cst_start_at",
    // 11 capability emit_*
    "emit_too_few",
    "emit_too_many",
    "emit_full_container",
    "emit_missing_separator",
    "emit_missing_element",
    "emit_missing_leading_separator",
    "emit_missing_trailing_separator",
    "emit_unexpected_leading_separator",
    "emit_unexpected_trailing_separator",
    "emit_unexpected_end_of_lhs",
    "emit_unexpected_end_of_rhs",
  ];
  assert_eq!(
    overridden.len(),
    27,
    "the family inventory is 11 core + 5 CstEmitter + 11 capability = 27"
  );
  for name in overridden {
    assert!(
      count(src, &std::format!("fn {name}")) >= 1,
      "CST_COMPOSITION_CENSUS: Sink does not override `{name}` — a defaulted inherit \
       silently severs that channel for wrapped inners (the R3 class)"
    );
  }

  // (b) Drift tripwires pinning the trait definitions: a NEW family method bumps one of these
  // counts and forces its classification (override + forward, or a documented inherit) here.
  let core = include_str!("../../emitter/mod.rs");
  let trait_body = &core[core.find("pub trait Emitter<").unwrap()
    ..core.find("impl<'a, L, U, Lang: ?Sized> Emitter").unwrap()];
  assert_eq!(
    count(trait_body, "  fn "),
    11,
    "core Emitter method count drifted: classify the new method (override + forward, or a \
     documented inherit) and update this census"
  );
  let cst = include_str!("../../emitter/cst.rs");
  let cst_body = &cst[cst.find("pub trait CstEmitter<").unwrap()..cst.find("for &mut U").unwrap()];
  assert_eq!(
    count(cst_body, "  fn "),
    5,
    "CstEmitter method count drifted"
  );
  let cap_total: usize = [
    include_str!("../../emitter/pratt.rs"),
    include_str!("../../emitter/repeated/too_few.rs"),
    include_str!("../../emitter/repeated/too_many.rs"),
    include_str!("../../emitter/repeated/full_container.rs"),
    include_str!("../../emitter/separated/mod.rs"),
    include_str!("../../emitter/separated/missing_leading.rs"),
    include_str!("../../emitter/separated/missing_trailing.rs"),
    include_str!("../../emitter/separated/unexpected_leading.rs"),
    include_str!("../../emitter/separated/unexpected_trailing.rs"),
  ]
  .iter()
  .map(|src| count(src, "fn emit_"))
  .sum();
  assert_eq!(
    cap_total, 22,
    "capability trait surface drifted (11 methods x trait def + &mut blanket): classify the \
     new channel in Sink and update this census"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The forwarding matrix — Sink satisfies every bound its inner emitter does
// ═══════════════════════════════════════════════════════════════════════════════

/// The `ComposableEmitter`-shaped conformance: a context bound naming the
/// full emitter trait family — core + the six atomic capability traits + the separated
/// refinements + pratt — accepts `Sink<E>` (and `&mut Sink<E>`, the parse_partial
/// threading shape) wherever it accepts `E`.
#[test]
fn sink_satisfies_the_full_emitter_family() {
  use crate::emitter::{
    FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    PrattEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  };

  fn composable<'inp, T>(_: &T)
  where
    T: Emitter<'inp, MiniLexer<'inp>>
      + CstEmitter<'inp, MiniLexer<'inp>>
      + TooFewEmitter<'inp, MiniLexer<'inp>>
      + TooManyEmitter<'inp, MiniLexer<'inp>>
      + FullContainerEmitter<'inp, MiniLexer<'inp>>
      + SeparatedEmitter<'inp, MiniLexer<'inp>>
      + MissingLeadingSeparatorEmitter<'inp, MiniLexer<'inp>>
      + MissingTrailingSeparatorEmitter<'inp, MiniLexer<'inp>>
      + UnexpectedLeadingSeparatorEmitter<'inp, MiniLexer<'inp>>
      + UnexpectedTrailingSeparatorEmitter<'inp, MiniLexer<'inp>>
      + PrattEmitter<'inp, MiniLexer<'inp>>,
  {
  }

  let mut sink = verbose_sink();
  composable(&sink);
  composable(&&mut sink);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Materialization — finish() as the typed-error wall, gap tiling as the law
// ═══════════════════════════════════════════════════════════════════════════════

/// A raw-u16 language: kinds pass through untouched, so tests assert on the dialect
/// constants directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum RawLang {}

impl rowan::Language for RawLang {
  type Kind = u16;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> u16 {
    raw.0
  }

  fn kind_to_raw(kind: u16) -> rowan::SyntaxKind {
    rowan::SyntaxKind(kind)
  }
}

const K_ROOT: u16 = 1;

fn tree(green: rowan::GreenNode) -> rowan::SyntaxNode<RawLang> {
  rowan::SyntaxNode::<RawLang>::new_root(green)
}

fn text(green: rowan::GreenNode) -> std::string::String {
  tree(green).text().to_string()
}

use crate::cst::FinishError;

#[test]
fn finish_builds_the_straight_tree() {
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish();
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  let green = green.expect("a balanced stream materializes");
  let root = tree(green.clone());
  assert_eq!(root.kind(), K_ROOT);
  let node = root.first_child().expect("Root[Node]");
  assert_eq!(node.kind(), K_NODE);
  assert_eq!(text(green), "a");
}

/// THE round-trip law, structural: an input with a lexer error (its bytes covered by no
/// committed token, since a skipped error settles nothing) still satisfies
/// `tree.text() == source` — the uncovered bytes tile as `gap_kind` tokens.
#[test]
fn round_trip_with_a_lexer_error_is_structural() {
  let mut sink = verbose_sink();
  // Source "a!c": the `!` is a lexer error — a diagnostic, never a token event.
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  let (green, emitter) = sink.finish(K_ROOT, "a!c");
  let green = green.expect("gap tiling makes the error-bearing input materialize");
  assert_eq!(
    text(green.clone()),
    "a!c",
    "losslessness is structural, not lexer luck"
  );

  // The gap is a real token of the configured kind, and the diagnostic survived.
  let root = tree(green);
  let kinds: std::vec::Vec<u16> = root.children_with_tokens().map(|el| el.kind()).collect();
  assert_eq!(kinds, std::vec![K_TOK, K_GAP, K_TOK]);
  assert_eq!(emitter.errors().len(), 1);
}

/// The gap-coverage law at the mechanism level (the partial-drop signature the zero-token
/// wall cannot see): tokens `a` and `c` survive over `"abc"` but the `b` at `[1, 2)` was
/// dropped and no lexer error covers it. `finish` refuses the unexplained gap with the exact
/// dropped span; `finish_partial` — the tooling door — tiles it instead. A gap a lexer error
/// *does* cover stays legal under `finish` (the round-trip oracle above is that green case).
#[test]
fn uncovered_gap_refused_by_finish_tiled_by_partial() {
  let dropped_b = |sink: &mut VerboseSink<'_>| {
    sink.cst_token(&MiniTok(b'a'), &span(0, 1));
    sink.cst_token(&MiniTok(b'c'), &span(2, 3)); // the `b` at [1,2) never settled
  };

  // The success door refuses the unexplained gap, naming exactly the dropped byte range.
  let mut sink = verbose_sink();
  dropped_b(&mut sink);
  let (green, _emitter) = sink.finish(K_ROOT, "abc");
  assert_eq!(
    green.expect_err("a dropped committed token is an unexplained gap"),
    FinishError::UncoveredGap { start: 1, end: 2 }
  );

  // The tooling door tolerates the incompleteness and tiles it — the round trip still holds.
  let mut sink = verbose_sink();
  dropped_b(&mut sink);
  let (green, _emitter) = sink.finish_partial(K_ROOT, "abc");
  assert_eq!(
    text(green.expect("finish_partial tiles the uncovered gap")),
    "abc"
  );
}

/// Leading and trailing uncovered bytes tile too — **when a recorded lexer error explains
/// them**. Here the lexer refused the leading and trailing bytes (diagnostics, no tokens),
/// so both tile as gaps around the one committed token and the round trip holds. An
/// *un*explained leading/trailing gap is the `UncoveredGap` refusal, covered separately.
#[test]
fn leading_and_trailing_gaps_tile() {
  let mut sink = verbose_sink();
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  let (green, _emitter) = sink.finish(K_ROOT, "abc");
  assert_eq!(
    text(green.expect("error-covered leading and trailing gaps tile")),
    "abc"
  );
}

/// An empty buffer over an empty source is a bare root; over a nonempty source the whole
/// span is one *unexplained* gap — `finish` refuses it (nothing covers those bytes),
/// `finish_partial` tiles it (the tooling door).
#[test]
fn empty_buffer_finishes() {
  let (green, _emitter) = verbose_sink().finish(K_ROOT, "");
  assert_eq!(text(green.expect("bare root")), "");

  let (green, _emitter) = verbose_sink().finish(K_ROOT, "xy");
  assert_eq!(
    green.expect_err("nothing covers the source"),
    FinishError::UncoveredGap { start: 0, end: 2 }
  );

  let (green, _emitter) = verbose_sink().finish_partial(K_ROOT, "xy");
  assert_eq!(text(green.expect("the tooling door tiles it")), "xy");
}

/// The token-channel wall, at the mechanism level: a *balanced* stream that builds
/// structure without one committed token over a nonempty source is the
/// half-forwarding-wrapper signature (structuring forwarded, `Emitter::commit_token`
/// inherited as the core no-op) — refused by `finish` AND `finish_partial` alike, never
/// dressed up by gap tiling as a plausible tree. The driven regression is
/// `half_forwarding_wrapper_is_refused_at_finish` in `tests/parser_node.rs`.
#[test]
fn balanced_structure_without_tokens_is_refused() {
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  sink.cst_finish();
  let (green, _emitter) = sink.finish(K_ROOT, "ab");
  assert_eq!(
    green.expect_err("structure without tokens over a nonempty source"),
    FinishError::StructureWithoutTokens
  );

  // The retro-wrap flavour of the same shape (a spent mark, still no token).
  let mut sink = verbose_sink();
  let mark = sink.cst_mark();
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish();
  let (green, _emitter) = sink.finish_partial(K_ROOT, "ab");
  assert_eq!(
    green.expect_err("the wall holds through the partial door too — the stream is balanced"),
    FinishError::StructureWithoutTokens
  );
}

/// The wall's exact boundary, so it can never overfire: an **empty source** makes a
/// token-less node legal (there was nothing to consume), a token-less stream with **no
/// structure** is an unexplained gap `finish` refuses but `finish_partial` tiles, and an
/// **aborted** stream with open nodes keeps its `finish_partial` door (the open nodes are
/// the abort witness).
#[test]
fn token_channel_wall_boundaries() {
  // Empty source: a token-less node is a legitimate empty match.
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  sink.cst_finish();
  let (green, _emitter) = sink.finish(K_ROOT, "");
  let root = tree(green.expect("nothing to consume, nothing severed"));
  assert_eq!(root.first_child().expect("Root[Node]").kind(), K_NODE);

  // No structure and no tokens over a nonempty source: the wall stays silent (nothing was
  // built), but every byte is unexplained — the gap-coverage law refuses it, while the
  // tooling door tiles it.
  let (green, _emitter) = verbose_sink().finish(K_ROOT, "ab");
  assert_eq!(
    green.expect_err("an unexplained gap, not the honest tree"),
    FinishError::UncoveredGap { start: 0, end: 2 }
  );
  let (green, _emitter) = verbose_sink().finish_partial(K_ROOT, "ab");
  assert_eq!(text(green.expect("the partial door tiles it")), "ab");

  // Aborted before the first settle, open node standing: the partial door still opens —
  // the imbalance is the abort witness the wall exempts.
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  let (green, _emitter) = sink.finish_partial(K_ROOT, "ab");
  assert_eq!(
    text(green.expect("the abort shape keeps its tooling door")),
    "ab"
  );
}

/// F-A6/F-A1 at the wall: an orphan finish is a typed error — rowan's silent absorption
/// of one level of imbalance under the root wrapper is unreachable, because the sink's
/// own stack refuses before the builder sees the pop.
#[test]
fn orphan_finish_is_a_typed_error_not_an_absorbed_close() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  // The release-build shape (debug builds refuse this at emission): a finish whose start
  // was rolled back away.
  sink.push_raw_event_for_tests(Event::FinishNode);
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  assert_eq!(
    green.expect_err("the imbalance must be refused, never absorbed"),
    FinishError::OrphanFinish { index: 1 }
  );
}

/// A fatal abort leaves open nodes: `finish` refuses with the open count;
/// `finish_partial` closes them (the explicit apollo-style opt-in) and the round-trip
/// law still holds on the partial tree.
#[test]
fn unclosed_nodes_refuse_finish_but_finish_partial_closes() {
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  assert_eq!(
    green.expect_err("open nodes refuse the total finish"),
    FinishError::UnclosedNodes { open: 2 }
  );

  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish_partial(K_ROOT, "a");
  let green = green.expect("the partial opt-in closes the open nodes");
  assert_eq!(text(green.clone()), "a");
  let root = tree(green);
  let node = root.first_child().expect("Root[Node[..]]");
  assert_eq!(node.kind(), K_NODE);
  assert_eq!(node.first_child().expect("Node[List[..]]").kind(), K_LIST);
}

/// F-A7's release backstop: a reserved (tombstone-band) kind on a token event is refused
/// at materialization — rowan would otherwise defer it to a query-time panic arbitrarily
/// far from the parse.
#[test]
fn reserved_kind_is_refused_at_finish() {
  let mut sink = verbose_sink();
  sink.push_raw_event_for_tests(Event::Token {
    kind: TOMBSTONE,
    span: span(0, 1),
  });
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  assert_eq!(
    green.expect_err("the reserved band never reaches rowan"),
    FinishError::ReservedKind { index: 0 }
  );

  let (green, _emitter) = verbose_sink().finish(TOMBSTONE, "");
  assert_eq!(
    green.expect_err("the root kind is validated too"),
    FinishError::ReservedRootKind
  );
}

/// F-A7 at cause: the emission-time debug assert catches a mapper that leaks the
/// reserved band, at the commit that used it.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "reserved tombstone kind")]
fn tombstone_mapper_debug_asserts_at_emission() {
  fn bad_map(_: &MiniTok) -> u16 {
    TOMBSTONE
  }
  let mut sink: VerboseSink<'_> = Sink::new(Verbose::new(), bad_map, K_ERR, K_GAP);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
}

/// Overlapping and non-monotone token spans are refused — the no-duplication half of the
/// round-trip law (a double emission cannot silently duplicate text).
#[test]
fn overlapping_spans_are_refused() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 2));
  sink.cst_token(&MiniTok(b'b'), &span(1, 3));
  let (green, _emitter) = sink.finish(K_ROOT, "abc");
  assert_eq!(
    green.expect_err("overlap is a hard error"),
    FinishError::OverlappingSpans { index: 1 }
  );
}

/// Offsets beyond u32 are refused whole — rowan text sizes are u32 and nothing truncates
/// silently.
#[test]
fn offset_overflow_is_refused() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, u32::MAX as usize + 10));
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  assert_eq!(
    green.expect_err("no silent truncation"),
    FinishError::OffsetOverflow { index: 0 }
  );
}

/// A span the source cannot slice (beyond its end) is refused: the events and the source
/// disagree and no tree should pretend otherwise.
#[test]
fn span_out_of_bounds_is_refused() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 5));
  let (green, _emitter) = sink.finish(K_ROOT, "ab");
  assert_eq!(
    green.expect_err("events and source must agree"),
    FinishError::SpanOutOfBounds { index: 0 }
  );
}

/// A StartAt whose target is not a live tombstone is refused (the release backstop
/// behind the panic-at-spend validation).
#[test]
fn stale_start_at_target_is_refused_at_finish() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.push_raw_event_for_tests(Event::StartAt {
    kind: K_WRAP,
    target: 0,
  });
  sink.push_raw_event_for_tests(Event::FinishNode);
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  assert_eq!(
    green.expect_err("a wrap must target a tombstone"),
    FinishError::StaleStartAt {
      index: 1,
      target: 0
    }
  );
}

/// The journal-integrity canary: a forward_parent pointer with no matching StartAt is
/// the un-journaled abandoned wrap (the F-A2/F-A3 corruption shape) — surfaced as a
/// typed error, never a stolen start.
#[test]
fn dangling_forward_parent_is_refused_at_finish() {
  let mut sink = verbose_sink();
  sink.push_raw_event_for_tests(Event::StartNode {
    kind: TOMBSTONE,
    forward_parent: NonZeroU32::new(2),
  });
  sink.push_raw_event_for_tests(Event::Token {
    kind: K_TOK,
    span: span(0, 1),
  });
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  assert_eq!(
    green.expect_err("a dangling wrap pointer is corruption, not a tree"),
    FinishError::DanglingForwardParent { index: 0 }
  );
}

/// A wrap that crosses a node boundary (mark taken inside a node, completed after the
/// node closed) is refused — the hoisted open would otherwise steal the enclosing
/// node's finish.
#[test]
fn improper_wrap_across_a_node_boundary_is_refused() {
  let mut sink = verbose_sink();
  sink.cst_start(K_NODE);
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(); // closes K_NODE — the mark is now interior to a closed node
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish();
  let (green, _emitter) = sink.finish(K_ROOT, "a");
  assert_eq!(
    green.expect_err("a wrap cannot cross a node boundary"),
    FinishError::ImproperWrap {
      start_at: 4,
      finish: 3
    }
  );
}

/// L1c pinned: the pratt double-wrap — same-target StartAts open in reverse buffer
/// order, so `1+2+3` replays as Bin(Bin(1,+,2),+,3).
#[test]
fn pratt_double_wrap_replays_inside_out() {
  let mut sink = verbose_sink();
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'1'), &span(0, 1));
  sink.cst_token(&MiniTok(b'+'), &span(1, 2));
  sink.cst_token(&MiniTok(b'2'), &span(2, 3));
  sink.cst_start_at(mark, K_WRAP); // fold 1: Bin[1,+,2]
  sink.cst_finish();
  sink.cst_token(&MiniTok(b'+'), &span(3, 4));
  sink.cst_token(&MiniTok(b'3'), &span(4, 5));
  sink.cst_start_at(mark, K_WRAP); // fold 2: the OUTER Bin
  sink.cst_finish();

  let (green, _emitter) = sink.finish(K_ROOT, "1+2+3");
  let green = green.expect("the double wrap is balanced");
  assert_eq!(text(green.clone()), "1+2+3");
  let root = tree(green);
  let outer = root.first_child().expect("Root[Bin]");
  assert_eq!(outer.kind(), K_WRAP);
  assert_eq!(outer.text().to_string(), "1+2+3");
  let inner = outer.first_child().expect("Bin[Bin[..],+,3]");
  assert_eq!(inner.kind(), K_WRAP);
  assert_eq!(inner.text().to_string(), "1+2");
}

/// The Marker typestate over a real sink: complete wraps the marked region; precede
/// wraps the completed node from the same tombstone (the alias shape, then the outer
/// layer).
#[test]
fn marker_complete_and_precede_build_nested_wraps() {
  use crate::cst::event::Marker;

  let mut sink = verbose_sink();
  let marker = Marker::new(sink.cst_mark());
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_token(&MiniTok(b':'), &span(1, 2));
  let completed = marker.complete(&mut sink, K_WRAP); // Alias[a, :]
  let outer = completed.precede();
  sink.cst_token(&MiniTok(b'b'), &span(2, 3));
  let _outer = outer.complete(&mut sink, K_NODE); // Field[Alias[a,:], b]

  let (green, _emitter) = sink.finish(K_ROOT, "a:b");
  let green = green.expect("nested wraps balance");
  assert_eq!(text(green.clone()), "a:b");
  let root = tree(green);
  let field = root.first_child().expect("Root[Field]");
  assert_eq!(field.kind(), K_NODE);
  let alias = field.first_child().expect("Field[Alias[..], b]");
  assert_eq!(alias.kind(), K_WRAP);
  assert_eq!(alias.text().to_string(), "a:");
}

/// F-A3 at the tree: the declined wrap leaves no trace — the retry's tree is exactly the
/// straight tree, gap-tiled over the byte the abandoned branch had consumed.
#[test]
fn declined_wrap_leaves_the_retry_tree_pristine() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish();
  rewind(&mut sink, ckp);

  // The retry consumes a different shape.
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'd'), &span(2, 3));
  sink.cst_finish();

  let (green, _emitter) = sink.finish(K_ROOT, "abd");
  let green = green.expect("the retry timeline is clean");
  assert_eq!(text(green.clone()), "abd");
  let root = tree(green);
  let kinds: std::vec::Vec<u16> = root.children_with_tokens().map(|el| el.kind()).collect();
  assert_eq!(
    kinds,
    std::vec![K_TOK, K_TOK, K_LIST],
    "no wrap node survives the decline (F-A3's steal is unrepresentable)"
  );
}

/// Backtrack equivalence, seed form: a straight drive and a decline-then-retry drive of
/// the same final timeline materialize byte-identical green trees.
#[test]
fn backtrack_equivalence_yields_identical_green_trees() {
  let drive = |sink: &mut VerboseSink<'_>| {
    sink.cst_start(K_NODE);
    sink.cst_token(&MiniTok(b'a'), &span(0, 1));
    sink.cst_token(&MiniTok(b'b'), &span(1, 2));
    sink.cst_finish();
  };

  let mut straight = verbose_sink();
  drive(&mut straight);
  let (straight_green, _emitter) = straight.finish(K_ROOT, "ab");

  let mut backtracked = verbose_sink();
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&backtracked);
  backtracked.cst_start(K_LIST);
  backtracked.cst_token(&MiniTok(b'a'), &span(0, 1));
  rewind(&mut backtracked, ckp);
  drive(&mut backtracked);
  let (backtracked_green, _emitter) = backtracked.finish(K_ROOT, "ab");

  assert_eq!(
    straight_green.expect("straight"),
    backtracked_green.expect("backtracked"),
    "same final timeline, byte-identical green tree"
  );
}

/// Diag slots and unwrapped tombstones are invisible to the tree, and the inner emitter
/// comes back with its diagnostics intact.
#[test]
fn diag_slots_and_inert_tombstones_are_invisible() {
  let mut sink = verbose_sink();
  let _unspent = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  emit_error(&mut sink, 0, 7);
  let (green, emitter) = sink.finish(K_ROOT, "a");
  let green = green.expect("marks and diag slots are structural silence");
  assert_eq!(text(green.clone()), "a");
  let root = tree(green);
  assert_eq!(
    root.children_with_tokens().count(),
    1,
    "one token, nothing else"
  );
  assert_eq!(
    emitter.errors().len(),
    1,
    "the diagnostics survive materialization"
  );
}

/// The hole wrap materializes as one error node holding the REAL skipped tokens.
#[test]
fn hole_wrap_materializes_as_an_error_node_with_real_tokens() {
  let mut sink = verbose_sink();
  sink.cst_token(&MiniTok(b'{'), &span(0, 1));
  // The scan settles two garbage tokens, then the hole is reported.
  sink.cst_token(&MiniTok(b'x'), &span(1, 2));
  sink.cst_token(&MiniTok(b'y'), &span(2, 3));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 3), 2).expect("collects");
  sink.cst_token(&MiniTok(b'}'), &span(3, 4));

  let (green, _emitter) = sink.finish(K_ROOT, "{xy}");
  let green = green.expect("the hole wrap balances");
  assert_eq!(text(green.clone()), "{xy}");
  let root = tree(green);
  let error_node = root.first_child().expect("Root[.., Error[..], ..]");
  assert_eq!(error_node.kind(), K_ERR);
  assert_eq!(error_node.text().to_string(), "xy");
  assert_eq!(
    error_node.children_with_tokens().count(),
    2,
    "the REAL skipped tokens are the error node's children"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Auto-emission: the input layer's settle hook drives the tree
// ═══════════════════════════════════════════════════════════════════════════════
//
// These drive a REAL input (MiniLexer under the sink in the emitter seat) through the
// public consume surface and pin the settle law at the event buffer: a token event
// appears exactly when a token settles — consumed, or skipped behind a scan frontier —
// and never for a peek, a decline, an unconsumed stopper, a rejected lexer error, or
// end of input.

/// The committed spans of the buffered `Token` events, in order.
fn token_spans(sink: &VerboseSink<'_>) -> std::vec::Vec<SimpleSpan> {
  sink
    .events()
    .iter()
    .filter_map(|ev| match ev {
      Event::Token { span, .. } => Some(*span),
      _ => None,
    })
    .collect()
}

type SinkCtx<'inp> = (VerboseSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);

/// Consume settles flow to the tree as they commit; peeks and declines emit nothing.
#[test]
fn auto_emission_settles_flow_peeks_and_declines_do_not() {
  use generic_arraydeque::typenum::U2;

  let mut sink = verbose_sink();
  let mut input = Input::<MiniLexer<'_>, SinkCtx<'_>>::with_state_and_cache(
    "abc",
    (),
    DefaultCache::<MiniLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut sink);

  // A peek lexes ahead but settles nothing.
  inp.peek::<U2>().expect("verbose collects");
  assert_eq!(
    token_spans(inp.emitter()),
    &[],
    "a peek emits no token event"
  );

  // A consume settles the cached token: exactly one event, at the settle.
  inp.next().expect("collects").expect("a token");
  assert_eq!(token_spans(inp.emitter()), &[span(0, 1)]);

  // An accepting try_expect settles; a declining one does not.
  inp
    .try_expect(|t| t.data().0 == b'b')
    .expect("collects")
    .expect("b matches");
  assert_eq!(token_spans(inp.emitter()), &[span(0, 1), span(1, 2)]);
  assert!(
    inp
      .try_expect(|t| t.data().0 == b'z')
      .expect("collects")
      .is_none(),
    "z does not match"
  );
  assert_eq!(
    token_spans(inp.emitter()),
    &[span(0, 1), span(1, 2)],
    "a decline emits nothing"
  );

  drop(inp);
  drop(input);
  // The parse consumed a prefix and stopped: `c` is unconsumed. That incompleteness is the
  // tooling door's remit (`finish_partial` tiles the tail); strict `finish` would refuse the
  // unexplained trailing gap.
  let (green, _emitter) = sink.finish_partial(K_ROOT, "abc");
  let green = green.expect("token-only timeline, partial parse");
  assert_eq!(text(green), "abc", "committed tokens + the gap-tiled tail");
}

/// Scan-skipped tokens settle behind the frontier and flow to the tree; the stopper the
/// scan examined but did not consume waits for its real consume.
#[test]
fn auto_emission_scan_skips_flow_and_the_stopper_waits() {
  let mut sink = verbose_sink();
  let mut input = Input::<MiniLexer<'_>, SinkCtx<'_>>::with_state_and_cache(
    "xy;z",
    (),
    DefaultCache::<MiniLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut sink);

  // sync_to skips `x` and `y` (reported + settled behind the frontier) and stops BEFORE
  // `;`, leaving it unconsumed at the cache front.
  let found = inp
    .sync_to(|t| t.data().0 == b';', || None)
    .expect("verbose collects")
    .is_some();
  assert!(found, "the sync point exists");
  assert_eq!(
    token_spans(inp.emitter()),
    &[span(0, 1), span(1, 2)],
    "both skipped tokens flowed at their skip settle; the unconsumed stopper did not"
  );

  // Consuming the stopper is its settle.
  inp.next().expect("collects").expect("the stopper");
  assert_eq!(
    token_spans(inp.emitter()),
    &[span(0, 1), span(1, 2), span(2, 3)],
    "the stopper's event fires at its real consume, exactly once"
  );
}

/// A rejected lexer error (`settle_fatal`) writes a position, not a token: no event.
#[test]
fn auto_emission_settle_fatal_emits_no_token_event() {
  let mut sink: FatalSink<'_> = Sink::new(Fatal::new(), map_tok, K_ERR, K_GAP);
  let mut input =
    Input::<MiniLexer<'_>, (FatalSink<'_>, DefaultCache<'_, MiniLexer<'_>>)>::with_state_and_cache(
      "!a",
      (),
      DefaultCache::<MiniLexer<'_>>::default(),
    );
  let mut inp = input.as_ref(&mut sink);

  let res = inp.next();
  assert!(res.is_err(), "the fatal emitter rejects the lexer error");

  drop(inp);
  drop(input);
  let events = sink.events();
  assert!(
    events.iter().all(|ev| !matches!(ev, Event::Token { .. })),
    "a rejected error item settles a position, never a token event: {events:?}"
  );
}

/// A non-fatal lexer error is a diagnostic (a `Diag` slot), never a token event; end of
/// input commits nothing.
#[test]
fn auto_emission_lexer_error_and_eof_emit_no_token_event() {
  let mut sink = verbose_sink();
  let mut input = Input::<MiniLexer<'_>, SinkCtx<'_>>::with_state_and_cache(
    "!a",
    (),
    DefaultCache::<MiniLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut sink);

  // next() crosses the error (reported through the Diag channel) and yields `a`.
  let tok = inp.next().expect("verbose collects").expect("a token");
  assert_eq!(*tok.span_ref(), span(1, 2));
  assert_eq!(token_spans(inp.emitter()), &[span(1, 2)]);

  // End of input: a position commit, not a settle.
  assert!(inp.next().expect("collects").is_none());
  assert_eq!(token_spans(inp.emitter()), &[span(1, 2)]);

  drop(inp);
  drop(input);
  let (green, _emitter) = sink.finish(K_ROOT, "!a");
  assert_eq!(
    text(green.expect("gap tiling covers the error byte")),
    "!a",
    "round trip holds on the error-bearing input"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Composition: the wrapped emitter must observe every committed token
// ═══════════════════════════════════════════════════════════════════════════════

/// A test-only inner emitter that counts every token [`Emitter::commit_token`] observes
/// — the composition witness for the sink's forwarding contract: a wrapped emitter that
/// tracks token-indexed state must see every token the sink's own CST event stream
/// records, recovery-skipped tokens included, or its count silently reads zero behind a
/// live diagnostic stream.
#[derive(Debug, Default)]
struct CountingEmitter {
  committed: usize,
}

impl<'a, L, Lang: ?Sized> Emitter<'a, L, Lang> for CountingEmitter {
  type Error = TestErr;

  fn emit_lexer_error(
    &mut self,
    _err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  fn emit_error(&mut self, _err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    _err: UnexpectedTokenOf<'a, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    Ok(())
  }

  fn rewind(&mut self, _cursor: &Cursor<'a, '_, L>, _checkpoint: u64)
  where
    L: Lexer<'a>,
  {
  }

  fn commit_token(&mut self, tok: &L::Token, span: &L::Span)
  where
    L: Lexer<'a>,
  {
    let _ = (tok, span);
    self.committed += 1;
  }
}

/// The defaulted constant mark is trivially a reading of the emission state.
impl crate::emitter::ValueKeyedEmitter for CountingEmitter {}

type CountingSink<'inp> = Sink<'inp, MiniLexer<'inp>, CountingEmitter>;
type CountingCtx<'inp> = (CountingSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);

/// The parenthesis pair table for the recovery-skip scenario below: `(` opens, `)`
/// closes, everything else (including the `;` sync point) is neutral.
fn counting_parens(kind: &u8) -> Balance<u8> {
  match *kind {
    b'(' => Balance::Open(b'('),
    b')' => Balance::Close(b'('),
    _ => Balance::Neutral,
  }
}

/// THE REGRESSION: `Sink::commit_token` must forward to the wrapped emitter, not
/// just record its own CST event. Drives a real parse — a plain consume, a
/// `sync_balanced` recovery skip (whose skipped tokens settle through `commit_token`
/// exactly like a consume, per the auto-emission contract), and the resumed consumes
/// after the sync point — through a `CountingEmitter` inner wrapped in `Sink`. Before
/// the forwarding fix this reads 0 (`commit_token` never reached `self.inner`); after,
/// it tracks the sink's own token-event count exactly, recovery-skipped tokens included.
#[test]
fn commit_token_forwards_to_the_inner_emitter_recovery_skips_included() {
  let mut sink: CountingSink<'_> = Sink::new(CountingEmitter::default(), map_tok, K_ERR, K_GAP);
  let mut input = Input::<MiniLexer<'_>, CountingCtx<'_>>::with_state_and_cache(
    "a(b)c;d",
    (),
    DefaultCache::<MiniLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut sink);

  // A plain consume: `a`.
  inp.next().expect("collects").expect("a token");

  // `sync_balanced` skips `(`, `b`, `)`, `c` (4 tokens, nesting-balanced through the
  // parens) and stops before the depth-0 `;` — the recovery path whose skipped tokens
  // settle behind the frontier via the same `commit_token` hook a consume uses.
  let hole = inp
    .sync_balanced(counting_parens, |t| t.data().0 == b';')
    .expect("collects")
    .expect("the depth-0 `;` is a sync point");
  assert_eq!(
    hole.skipped(),
    4,
    "`(`, `b`, `)`, `c` all fall inside the hole"
  );

  // The sync point and the trailing token both settle as ordinary consumes.
  inp.next().expect("collects").expect("the `;` sync point");
  inp.next().expect("collects").expect("the trailing `d`");

  drop(inp);
  drop(input);

  let recorded = sink
    .events()
    .iter()
    .filter(|ev| matches!(ev, Event::Token { .. }))
    .count();
  assert_eq!(
    recorded, 7,
    "a, (, b, ), c, ;, d: seven committed tokens on the sink's own event stream"
  );
  assert_eq!(
    sink.inner_ref().committed,
    recorded,
    "Sink::commit_token must forward to the wrapped emitter: the inner's count must \
     match the sink's own token-event count exactly, recovery-skipped tokens included"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// A decline rewinds the inner emitter to its CHECKPOINT reading (rewind contract)
// ═══════════════════════════════════════════════════════════════════════════════

/// A test-only inner that journals every forward (token AND diagnostic) with a value-keyed
/// checkpoint — `checkpoint` = journal length, `rewind` = truncate to the mark — the exact
/// downstream shape the sink's inner-rewind contract must support. Records enough to prove
/// the sink hands it the right target: a rewind must keep every entry before the mark and
/// drop every one after. Unlike [`CountingEmitter`] (a monotone counter that cannot observe a
/// rewind), this inner is genuinely rewindable, so it witnesses *where* the sink rewinds it.
#[derive(Debug, Default)]
struct JournalingEmitter {
  journal: std::vec::Vec<JEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JEntry {
  Token,
  Diag,
}

impl<'a, L, Lang: ?Sized> Emitter<'a, L, Lang> for JournalingEmitter {
  type Error = TestErr;

  fn emit_lexer_error(
    &mut self,
    _err: Spanned<<L::Token as Token<'a>>::Error, L::Span>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.journal.push(JEntry::Diag);
    Ok(())
  }

  fn emit_error(&mut self, _err: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.journal.push(JEntry::Diag);
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    _err: UnexpectedTokenOf<'a, L, Lang>,
  ) -> Result<(), Self::Error>
  where
    L: Lexer<'a>,
  {
    self.journal.push(JEntry::Diag);
    Ok(())
  }

  fn checkpoint(&self) -> u64 {
    self.journal.len() as u64
  }

  fn rewind(&mut self, _cursor: &Cursor<'a, '_, L>, checkpoint: u64)
  where
    L: Lexer<'a>,
  {
    self
      .journal
      .truncate((checkpoint as usize).min(self.journal.len()));
  }

  fn commit_token(&mut self, _tok: &L::Token, _span: &L::Span)
  where
    L: Lexer<'a>,
  {
    self.journal.push(JEntry::Token);
  }
}

/// The mark is the journal length: a reading of the emission state, keyed on no table.
impl crate::emitter::ValueKeyedEmitter for JournalingEmitter {}

type JournalingSink<'inp> = Sink<'inp, MiniLexer<'inp>, JournalingEmitter>;
type JournalingCtx<'inp> = (JournalingSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);

/// REGRESSION (variant A — no diagnostic): a decline must rewind the inner emitter to its
/// **checkpoint** reading, keeping every token forwarded before the mark. Over `"abc"`,
/// consume `a`,`b`, then an [`attempt`](crate::InputRef::attempt) that consumes `c` and
/// declines. The tokens `a`,`b` settled through `commit_token` **before** the attempt's
/// checkpoint, so they must survive on the inner exactly as they survive on the sink's own
/// event log — the inner's surviving journal must equal the sink's surviving `Token` count.
///
/// Pre-fix the sink derived the inner rewind target from the last surviving `Diag` slot; with
/// no diagnostic it fell to `base`, so the two surviving tokens were **not** restored to the
/// inner (the inner's reading and the sink's disagree — permanent desync). Post-fix the
/// checkpoint captured the inner's own reading and the rewind restores it exactly.
#[test]
fn decline_rewinds_inner_to_checkpoint_reading_no_diag() {
  let mut sink: JournalingSink<'_> = Sink::new(JournalingEmitter::default(), map_tok, K_ERR, K_GAP);
  let mut input = Input::<MiniLexer<'_>, JournalingCtx<'_>>::with_state_and_cache(
    "abc",
    (),
    DefaultCache::<MiniLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut sink);

  // Two plain consumes BEFORE the speculative region: both settle through `commit_token`.
  inp.next().expect("collects").expect("a token");
  inp.next().expect("collects").expect("b token");

  // The attempt captures a checkpoint, consumes `c`, then declines — rewinding the inner.
  let declined: Option<()> = inp.attempt(|inp2| {
    inp2.next().expect("collects").expect("c token");
    None
  });
  assert!(
    declined.is_none(),
    "the closure returned None: the attempt declines"
  );

  drop(inp);
  drop(input);

  let recorded = sink
    .events()
    .iter()
    .filter(|ev| matches!(ev, Event::Token { .. }))
    .count();
  assert_eq!(recorded, 2, "a, b survive on the sink's own event stream");
  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token, JEntry::Token],
    "the inner must be rewound to its checkpoint reading (a, b survive), not past them"
  );
  assert_eq!(
    sink.inner_ref().journal.len(),
    recorded,
    "the inner's surviving forwards must agree with the sink's own token-event count"
  );
}

/// REGRESSION (variant B — a diagnostic before the checkpoint): the surviving-`Diag`-slot
/// derivation missed tokens forwarded **after** the last diagnostic. Over `"a!bc"`, consume
/// `a`, then a single `next()` crosses the `!` lexer error (forwarding a `Diag`) and yields
/// `b`; then an `attempt` consumes `c` and declines. The checkpoint sits after `a`,diag,`b`.
///
/// Pre-fix the rewind used the surviving `!` slot's reading (after `a`,diag), dropping the `b`
/// that settled after it — the inner journal came back `[Token, Diag]` (length 2). Post-fix
/// the checkpoint captured the inner's reading, so `b` survives: `[Token, Diag, Token]`.
#[test]
fn decline_rewinds_inner_to_checkpoint_reading_across_diag() {
  let mut sink: JournalingSink<'_> = Sink::new(JournalingEmitter::default(), map_tok, K_ERR, K_GAP);
  let mut input = Input::<MiniLexer<'_>, JournalingCtx<'_>>::with_state_and_cache(
    "a!bc",
    (),
    DefaultCache::<MiniLexer<'_>>::default(),
  );
  let mut inp = input.as_ref(&mut sink);

  // `a`, then a consume that crosses the `!` lexer error (a forwarded Diag) and yields `b`.
  inp.next().expect("collects").expect("a token");
  inp
    .next()
    .expect("collects")
    .expect("b token, crossing the ! lexer error");

  let declined: Option<()> = inp.attempt(|inp2| {
    inp2.next().expect("collects").expect("c token");
    None
  });
  assert!(
    declined.is_none(),
    "the closure returned None: the attempt declines"
  );

  drop(inp);
  drop(input);

  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token, JEntry::Diag, JEntry::Token],
    "the inner must be rewound to its checkpoint reading: a, the crossed diag, and b all \
     survive — the diag-slot derivation dropped the b forwarded after the diagnostic"
  );
}

/// REGRESSION: the no-row base rewind must restore the inner to its
/// construction-time reading even when settled TOKENS were the only forwards. Pre-fix,
/// `commit_token` advanced the inner without priming the base, so the no-row rewind captured a
/// post-token reading and the inner retained tokens the sink log dropped — a one-timeline
/// shear on the raw fallback path.
#[test]
fn no_row_base_rewind_restores_the_construction_reading() {
  let mut sink: JournalingSink<'_> = Sink::new(JournalingEmitter::default(), map_tok, K_ERR, K_GAP);

  // One settled token, forwarded to the inner — and no checkpoint ever captured.
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  assert_eq!(sink.inner_ref().journal, std::vec![JEntry::Token]);
  assert_eq!(sink.events().len(), 1);

  // A raw rewind to the origin finds no mark-stack row: the base fallback fires.
  let origin = 0usize;
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), 0);

  assert_eq!(sink.events().len(), 0, "the sink log drops the token");
  assert_eq!(
    sink.inner_ref().journal.len(),
    0,
    "the inner returns to its construction-time reading — it must not retain tokens the \
     sink log dropped"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The inner is rewound only to a reading the sink knows EXACTLY
// ═══════════════════════════════════════════════════════════════════════════════

/// REGRESSION: a no-row rewind that truncates NOTHING — the mark is out of
/// range above, or sits exactly at, the current log length — must be a no-op on EVERY
/// channel, the inner
/// included: the surviving events are the whole log, so every inner-side record they
/// reference must survive with them (the trait's rewind-to-current law). Pre-fix the no-row
/// fallback rewound the inner to base unconditionally: the sink log kept the settled token,
/// the inner dropped it — silent one-timeline shear on a lawful no-op call.
#[test]
fn no_row_truncation_free_rewind_leaves_the_inner_untouched() {
  let mut sink: JournalingSink<'_> = Sink::new(JournalingEmitter::default(), map_tok, K_ERR, K_GAP);

  // One settled token, forwarded to the inner — and no checkpoint ever captured.
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  let origin = 0usize;

  // Out of range above the log length: a total no-op — truncates nothing, moves nothing.
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), u64::MAX);
  assert_eq!(sink.events().len(), 1, "the event log keeps the token");
  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token],
    "the inner keeps the token the log kept — a truncation-free rewind touches no channel"
  );

  // Exactly at the length: the rewind-to-current law — in range, same observables.
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), 1);
  assert_eq!(sink.events().len(), 1);
  assert_eq!(sink.inner_ref().journal, std::vec![JEntry::Token]);
}

/// REGRESSION (mid-log no-row case): a truncating rewind to a mark no checkpoint ever
/// captured has NO exact inner reading anywhere — undisciplined raw use, witnessed at cause
/// in debug builds (the sink-level twin of the input layer's LIFO witness) instead of
/// silently corrupting a channel. Pre-fix it silently paired the surviving prefix with the
/// construction-time base, destroying committed inner state the log still carried.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "rewind to a mid-log mark with no captured row")]
fn no_row_middle_rewind_debug_asserts_at_cause() {
  let mut sink: JournalingSink<'_> = Sink::new(JournalingEmitter::default(), map_tok, K_ERR, K_GAP);
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));
  let origin = 0usize;
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), 1);
}

/// The release twin of the debug witness: a truncating no-row mid-log rewind still rewinds
/// the sink's OWN channels exactly, and refuses to guess an inner reading — the inner stays
/// put (bounded one-sided staleness), never dropped to base (which would destroy inner-side
/// records the surviving prefix still references).
#[cfg(not(debug_assertions))]
#[test]
fn no_row_middle_rewind_leaves_the_inner_untouched_in_release() {
  let mut sink: JournalingSink<'_> = Sink::new(JournalingEmitter::default(), map_tok, K_ERR, K_GAP);
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));
  let origin = 0usize;
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), 1);
  assert_eq!(
    sink.events().len(),
    1,
    "the sink's own log truncates exactly"
  );
  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token, JEntry::Token],
    "no exact reading exists for a mid-log no-row mark: the inner is left untouched"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Out-of-range marks are ignored BEFORE the row lookup
// ═══════════════════════════════════════════════════════════════════════════════

/// REGRESSION: an out-of-range FUTURE mark (`checkpoint > len`) is a rewind to
/// a point the log has not reached — a TOTAL no-op on every channel, the mark stack included.
/// Pre-fix, `mark = checkpoint.min(len)` clamped it to the length BEFORE the row lookup, so a
/// future mark masqueraded as a rewind-to-current and spent the live row of a REAL checkpoint
/// taken at the current length; that checkpoint's own later rewind then found no row — the
/// mid-log no-row witness fired on a disciplined mark in debug, the inner ghosted the
/// abandoned branch's records in release.
#[test]
fn out_of_range_rewind_spends_no_live_row() {
  let mut sink: JournalingSink<'_> = Sink::new(JournalingEmitter::default(), map_tok, K_ERR, K_GAP);
  let origin = 0usize;

  // One settled token, then a live checkpoint AT the current length: len == 1, row at 1.
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  let c = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  assert_eq!(c, 1, "the checkpoint sits exactly at the current length");
  assert_eq!(sink.rows_len(), 1);

  // Out of range, strictly above the length: a total no-op on EVERY channel.
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), u64::MAX);
  assert_eq!(sink.events().len(), 1, "the event log keeps the token");
  assert_eq!(
    sink.rows_len(),
    1,
    "the live row at len is NOT spent by an out-of-range mark"
  );
  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token],
    "the inner is untouched"
  );

  // The aftermath the clamp used to poison: more traffic, then the LEGITIMATE rewind of `c` —
  // it must find its live row and restore both timelines to the mark (not trip the mid-log
  // no-row witness on a disciplined mark).
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), c);
  assert_eq!(
    sink.events().len(),
    1,
    "rewound to the mark: the second token is gone"
  );
  assert_eq!(sink.rows_len(), 0, "the legitimate rewind spent the row");
  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token],
    "the inner rewound to the row's captured reading — no ghost of the abandoned token"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Every stacked savepoint's capture is settled on every abandon path
// ═══════════════════════════════════════════════════════════════════════════════
//
// A savepoint's mark is the event-log length, so savepoints taken with no events between
// them all read the SAME mark. A single rewind cannot name them: it drops rows strictly
// above the mark plus the newest row at it, which leaves every older aliased row behind as
// a dead row the stack still counts. The abandon paths therefore have to settle each
// capture individually, newest-first, before the one rewind that returns to the target.
// The oracle is the row count: it must return to zero on every path.

/// The type alias the stacked-settle probes share.
type StackedCtx<'inp> = (VerboseSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);

/// A sink with `depth` nodes open, ready to be wrapped in an input.
fn sink_with_open_nodes<'inp>(depth: usize) -> VerboseSink<'inp> {
  let mut sink = verbose_sink();
  for _ in 0..depth {
    sink.cst_start(K_NODE);
  }
  sink
}

/// Closes `depth` enclosing nodes through the handle's emitter — the valid closes whose
/// depth baseline a dead row would corrupt.
fn close_open_nodes(sink: &mut VerboseSink<'_>, depth: usize) {
  for _ in 0..depth {
    CstEmitter::<MiniLexer<'_>, ()>::cst_finish(sink);
  }
}

/// `rollback_to` keeps the target and destroys every younger savepoint. Each of those
/// younger captures aliases the target's mark here, so each must be settled on its own
/// before the single restore.
#[test]
fn stacked_rollback_to_settles_every_aliased_savepoint_row() {
  let mut sink = sink_with_open_nodes(1);
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::new("abcdef");
  {
    let mut inp = input.as_ref(&mut sink);
    let mut txn = inp.begin_stacked();
    let sp1 = txn.savepoint();
    let _sp2 = txn.savepoint();
    let _sp3 = txn.savepoint();
    txn.rollback_to(sp1);
    txn.commit();
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    sink.rows_len(),
    0,
    "`rollback_to` + `commit` must leave no live row: the younger savepoints' captures \
     alias the target's mark, and a single restore cannot name them"
  );
}

/// The whole-transaction rollback restores only the begin point, so the savepoints above it
/// have to be settled first.
#[test]
fn stacked_whole_rollback_settles_every_aliased_savepoint_row() {
  let mut sink = sink_with_open_nodes(1);
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::new("abcdef");
  {
    let mut inp = input.as_ref(&mut sink);
    let mut txn = inp.begin_stacked();
    let _sp1 = txn.savepoint();
    let _sp2 = txn.savepoint();
    txn.rollback();
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    sink.rows_len(),
    0,
    "a whole `rollback` must settle every savepoint capture before restoring the base"
  );
}

/// The rolling-back drop — `begin_stacked`'s default policy, and the arm every `?`
/// early-return and every unwind through a stacked scope takes. It holds the savepoint
/// checkpoints at the moment their marks die, so it is the only place they can be settled.
#[test]
fn stacked_rollback_on_drop_settles_every_aliased_savepoint_row() {
  let mut sink = sink_with_open_nodes(1);
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::new("abcdef");
  {
    let mut inp = input.as_ref(&mut sink);
    {
      let mut txn = inp.begin_stacked();
      let _sp1 = txn.savepoint();
      let _sp2 = txn.savepoint();
      // …dropped undecided: the rollback-on-drop arm.
    }
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    sink.rows_len(),
    0,
    "an undecided rolling-back drop must settle every savepoint capture, exactly as the \
     commit-policy arm does"
  );
}

/// The same law across open-node depths: a row carries the derived depth frozen at its
/// capture, and that frozen fact is what a later depth recount anchors on. Settling has to
/// be exact at every depth, and every enclosing close must stay accepted.
#[test]
fn stacked_savepoint_rows_settle_at_every_open_node_depth() {
  for depth in 1..=3usize {
    let mut sink = sink_with_open_nodes(depth);
    let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::new("abcdef");
    {
      let mut inp = input.as_ref(&mut sink);
      {
        let mut txn = inp.begin_stacked();
        let _sp1 = txn.savepoint();
        let _sp2 = txn.savepoint();
        let _sp3 = txn.savepoint();
      }
      close_open_nodes(inp.emitter(), depth);
    }
    assert_eq!(
      sink.rows_len(),
      0,
      "depth {depth}: every aliased capture must be settled whatever the frozen depth"
    );
  }
}

/// The raw-restore leg: a raw restore below the savepoints (but above the base) leaves them
/// off the live lineage — detect-at-use — but their emitter captures are still live rows,
/// and the guard's drop is still the only holder of the checkpoints that carry them.
#[test]
fn stacked_raw_restore_below_savepoints_still_settles_their_rows() {
  let mut sink = sink_with_open_nodes(1);
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::new("abcdef");
  {
    let mut inp = input.as_ref(&mut sink);
    {
      let mut txn = inp.begin_stacked();
      let raw = txn.save();
      let _sp1 = txn.savepoint();
      let _sp2 = txn.savepoint();
      txn.restore(raw);
      // …then dropped undecided, with two lineage-invalidated savepoints still held.
    }
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    sink.rows_len(),
    0,
    "a lineage-invalidated savepoint still owns an emitter capture: the drop must settle it"
  );
}

/// The control: `release` and `commit` already settle each capture individually, so they are
/// exact over aliased marks. They are the shape the abandon paths must match.
#[test]
fn stacked_release_and_commit_settle_every_aliased_row() {
  let mut sink = sink_with_open_nodes(1);
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::new("abcdef");
  {
    let mut inp = input.as_ref(&mut sink);
    let mut txn = inp.begin_stacked();
    let sp1 = txn.savepoint();
    let _sp2 = txn.savepoint();
    let _sp3 = txn.savepoint();
    txn.release(sp1);
    txn.commit();
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    sink.rows_len(),
    0,
    "the settle-each funnel is exact over aliased marks"
  );
}

/// The composition of the two settle families on the one drop path: session points and
/// savepoints interleave (a point can be opened through the guard), so a rolling-back drop
/// has to settle **both** — the savepoints from the guard's own stack, the point from the
/// rewind's reconciliation — before the base's rewind sweeps what is left.
#[test]
fn interleaved_session_point_and_savepoints_all_settle_on_drop() {
  let mut sink = sink_with_open_nodes(1);
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::new("abcdef");
  {
    let mut inp = input.as_ref(&mut sink);
    {
      let mut txn = inp.begin_stacked();
      let _sp1 = txn.savepoint();
      let _point = txn.begin_point();
      let _sp2 = txn.savepoint();
      // …dropped undecided, with the point still open between the two savepoints.
    }
    assert_eq!(inp.points(), 0, "the rollback reconciled the open point");
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    sink.rows_len(),
    0,
    "both settle families ran: no savepoint capture and no session-point capture is left"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The inner-emitter contract is a bound, and it survives being borrowed
// ═══════════════════════════════════════════════════════════════════════════════

/// A **borrowed** inner is as admissible as an owned one: `Emitter` is implemented for
/// `&mut U`, and the value-keyed promise is forwarded through that same shape, so
/// `Sink<&mut Verbose>` composes exactly like `Sink<Verbose>` — captures settle to zero rows
/// and the inner keeps every diagnostic below a kept mark. (A table-keyed inner is refused at
/// compile time instead; the wall is the `compile_fail` example on `Sink::new`.)
#[test]
fn a_borrowed_value_keyed_inner_composes_like_an_owned_one() {
  type BorrowedSink<'inp, 'e> = Sink<'inp, MiniLexer<'inp>, &'e mut Verbose<TestErr>>;
  type BorrowedCtx<'inp, 'e> = (BorrowedSink<'inp, 'e>, DefaultCache<'inp, MiniLexer<'inp>>);

  let mut verbose = Verbose::<TestErr>::new();
  {
    let mut sink: BorrowedSink<'_, '_> = Sink::new(&mut verbose, map_tok, K_ERR, K_GAP);
    let mut input = Input::<'_, MiniLexer<'_>, BorrowedCtx<'_, '_>, ()>::new("abcdef");
    {
      let mut inp = input.as_ref(&mut sink);
      // A committed attempt and a declined one, both capturing at the same buffer length.
      let committed: Option<()> = inp.attempt(|inp| inp.next().ok().flatten().map(|_| ()));
      assert!(committed.is_some());
      let declined: Option<()> = inp.attempt(|inp| {
        let _ = inp.next();
        None
      });
      assert!(declined.is_none());
    }
    assert_eq!(
      sink.rows_len(),
      0,
      "every capture settled through the borrowed inner exactly as through an owned one"
    );
  }
  assert_eq!(
    verbose.errors().len(),
    0,
    "the borrowed inner is still the one the sink forwarded to"
  );
}
