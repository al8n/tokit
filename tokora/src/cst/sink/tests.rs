//! The named-regression suite of the rewindable event sink: the failure-corpus scenarios
//! (F-A1/F-A2/F-A3/F-A5, T3) at the mechanism level, the unified-log exactness laws, and
//! the CST_FORWARD_CENSUS source lock.

use core::num::NonZeroU32;

use crate::{
  Lexer, SimpleSpan,
  cache::DefaultCache,
  cst::{
    CstProfile, KindValidator,
    event::{Event, TOMBSTONE},
  },
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

/// The fixture dialect's whole kind space: the synthetic root, the node/list/wrap kinds, the
/// one token image, and the two kinds the sink synthesizes. A real range predicate, because
/// `accept_all` is the escape hatch and not the default.
fn in_kind_space(kind: u16) -> bool {
  matches!(
    kind,
    K_ROOT | K_NODE | K_LIST | K_WRAP | K_TOK | K_ERR | K_GAP
  )
}

/// The fixture dialect's profile: one value, handed to every construction below.
fn profile() -> CstProfile<MiniTok> {
  CstProfile::new(map_tok, KindValidator::new(in_kind_space), K_ERR, K_GAP)
}

fn verbose_sink(src: &str) -> VerboseSink<'_> {
  Sink::new(src, Verbose::new(), profile())
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
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
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
      Event::FinishNode { kind: K_NODE },
    ]
  );
}

#[test]
fn mark_appends_an_inert_tombstone() {
  let mut sink = verbose_sink("");
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
  let mut sink = verbose_sink("ab");
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
  let mut sink = verbose_sink("");
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
  let mut sink = verbose_sink("a");
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
  let mut a = verbose_sink("");
  let mut b = verbose_sink("");
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
  let mut sink = verbose_sink("abc");
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  rewind(&mut sink, ckp);
  sink.cst_token(&MiniTok(b'c'), &span(1, 2));
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish(K_WRAP);
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
  let mut sink = verbose_sink("abc");
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);

  // The speculative wrap: StartAt + finish, with the journaled fp write onto index 1.
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish(K_WRAP);
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
  let mut sink = verbose_sink("abc");
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish(K_WRAP);
  rewind(&mut sink, ckp);

  // The retry: an unrelated List over the next token.
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'd'), &span(2, 3));
  sink.cst_finish(K_LIST);

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
  let mut sink = verbose_sink("abcdef");
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
  let mut sink = verbose_sink("abcdef");
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
// still panics, the genuinely-legal cross-checkpoint close no longer does, and the
// leaked-finish misuse shape — once depth-indistinguishable — is now named by the kind
// `cst_finish` carries (see the `Sink::cst_finish` contract comment)
// ═══════════════════════════════════════════════════════════════════════════════

/// A finish with no open node anywhere is true global underflow: detect at cause in
/// debug builds. The narrowed assert leaves this case unchanged — baseline and global
/// depth already agreed at zero here, with no live checkpoint in play.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "no open node")]
fn orphan_finish_debug_asserts_at_emission() {
  let mut sink = verbose_sink("a");
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
}

/// The leaked-finish MISUSE shape, now a typed error: `cst_start(A); checkpoint m;
/// cst_start(B); rewind(m); cst_token; cst_finish(B)`. The finish was meant for B, but B's
/// start died with the rewind, so it lands on ancestor A. After the rewind the event buffer
/// is byte-identical to a legal `A`-close
/// (`cst_finish_across_a_live_checkpoint_is_legal_and_materializes`) — depth cannot tell them
/// apart, nor can the whole buffer. The kind the finish carries can, and does.
///
/// Falsified by: an `Ok` tree (the old balanced-but-wrong outcome), an `OrphanFinish` (this
/// is not underflow — a node *is* open), or a `MismatchedFinish` naming the wrong pair.
#[test]
fn mismatched_finish_kind_is_a_typed_error() {
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start(K_LIST); // the start this finish was meant to close …
  rewind(&mut sink, ckp); //  … rolled back: K_LIST never existed
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_LIST); // leaked: it would close the ancestor K_NODE instead
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the leaked finish must be named, not dressed up as a tree"),
    FinishError::MismatchedFinish {
      index: 2,
      expected: K_NODE,
      found: K_LIST
    }
  );
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
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let root = tree(green.expect("a balanced stream materializes"));
  assert_eq!(root.first_child().expect("Root[Node]").kind(), K_NODE);
}

/// The same legal history, read by the **materialization** wall rather than by the emit-time
/// assert: `cst_start(A); checkpoint; token; cst_finish(A)`. The kind `cst_finish` now carries
/// is what names the leaked finish next door, and the risk of any identity check is that it
/// re-refuses this shape — the exact false positive issue #98 was about, one layer down. It
/// must materialize `Root[Node]` with no error.
///
/// Falsified by: any `Err` at all, and in particular a `MismatchedFinish` — the frame this
/// finish lands on IS the node it named.
#[test]
fn legal_cross_checkpoint_close_still_accepted() {
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let root = tree(green.expect("a live checkpoint does not make a matching close a mismatch"));
  assert_eq!(root.first_child().expect("Root[Node]").kind(), K_NODE);
  assert_eq!(root.text().to_string(), "a");
}

/// The narrowing must still catch a *genuine* global underflow —
/// closing when nothing is open anywhere — in every debug build, checkpoint or not (the
/// protection the narrowing must preserve).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "global underflow")]
fn cst_finish_debug_asserts_on_genuine_global_underflow_across_a_checkpoint() {
  let mut sink = verbose_sink("a");
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE); // nothing was ever open — global underflow, checkpoint or not
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
  let mut sink = verbose_sink("");
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
  let mut sink = verbose_sink("");
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
  let mut sink: FatalSink<'_> = Sink::new("a", Fatal::new(), profile());
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
  let mut sink = verbose_sink("");
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
  let mut sink = verbose_sink("a");
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
  let mut sink = verbose_sink("");
  let mark = sink.cst_mark();
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  assert_eq!(sink.rows_len(), 1);
  rewind(&mut sink, ckp);
  assert_eq!(sink.rows_len(), 0, "the capture was spent");
  // The mark predates the (no-op) rewind and must still spend cleanly.
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish(K_WRAP);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Checkpoint rows — the frozen depth ledger
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn checkpoint_rows_freeze_derived_depth() {
  let mut sink = verbose_sink("");
  let first = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start(K_NODE);
  let second = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  assert_eq!(sink.rows_len(), 2);

  // Kept captures release newest-first (the LIFO settle order).
  Emitter::<MiniLexer<'_>>::release(&mut sink, second);
  Emitter::<MiniLexer<'_>>::release(&mut sink, first);
  assert_eq!(sink.rows_len(), 0);

  // The released rows became the derived-depth floor; the balance still closes.
  sink.cst_finish(K_NODE);
  assert_eq!(sink.events().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// The hole wrap — recovery skips become error nodes over the REAL tokens
// ═══════════════════════════════════════════════════════════════════════════════

/// The wrap brackets exactly the hole's buffered token events — interleaved Diag slots
/// ride inside, tokens outside the hole span stay outside.
#[test]
fn hole_wrap_brackets_the_buffered_suffix() {
  let mut sink = verbose_sink("xab");
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
      Event::FinishNode { kind: K_ERR },
      Event::Diag { error_span: None },
    ]
  );
}

/// A zero-skip hole produces no node (and the crate's caller never even emits one).
#[test]
fn zero_skip_hole_makes_no_node() {
  let mut sink = verbose_sink("a");
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 1), 0).expect("collects");
  assert_eq!(sink.events().len(), 2, "one token, one Diag — no wrap");
  assert!(matches!(sink.events()[1], Event::Diag { .. }));
}

/// A hole with no buffered token events (no auto-emission wired, or a direct call) has
/// nothing to wrap: no node, just the forwarded diagnostic.
#[test]
fn tokenless_hole_makes_no_node() {
  let mut sink = verbose_sink("");
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(0, 4), 3).expect("collects");
  assert_eq!(sink.events().len(), 1);
  assert!(matches!(sink.events()[0], Event::Diag { .. }));
}

/// The wrap survives a later rewind like any other events: a checkpoint below the hole
/// unwinds wrap and tokens together.
#[test]
fn hole_wrap_rewinds_with_the_log() {
  let mut sink = verbose_sink("xab");
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
  let mut sink = verbose_sink("a");
  {
    let mut threaded: &mut VerboseSink<'_> = &mut sink;
    CstEmitter::<MiniLexer<'_>>::cst_start(&mut threaded, K_NODE);
    let tok = MiniTok(b'a');
    let sp = span(0, 1);
    CstEmitter::<MiniLexer<'_>>::cst_token(&mut threaded, &tok, &sp);
    let mark = CstEmitter::<MiniLexer<'_>>::cst_mark(&mut threaded);
    CstEmitter::<MiniLexer<'_>>::cst_start_at(&mut threaded, mark, K_WRAP);
    CstEmitter::<MiniLexer<'_>>::cst_finish(&mut threaded, K_WRAP);
    CstEmitter::<MiniLexer<'_>>::cst_finish(&mut threaded, K_NODE);
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

  let mut sink = verbose_sink("");
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
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("a!c");
  // Source "a!c": the `!` is a lexer error — a diagnostic, never a token event.
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  let (green, emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("abc");
  dropped_b(&mut sink);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("a dropped committed token is an unexplained gap"),
    FinishError::UncoveredGap { start: 1, end: 2 }
  );

  // The tooling door tolerates the incompleteness and tiles it — the round trip still holds.
  let mut sink = verbose_sink("abc");
  dropped_b(&mut sink);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
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
  let mut sink = verbose_sink("abc");
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  let (green, _emitter) = sink.finish(K_ROOT);
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
  let (green, _emitter) = verbose_sink("").finish(K_ROOT);
  assert_eq!(text(green.expect("bare root")), "");

  let (green, _emitter) = verbose_sink("xy").finish(K_ROOT);
  assert_eq!(
    green.expect_err("nothing covers the source"),
    FinishError::UncoveredGap { start: 0, end: 2 }
  );

  let (green, _emitter) = verbose_sink("xy").finish_partial(K_ROOT);
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
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_NODE);
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("structure without tokens over a nonempty source"),
    FinishError::StructureWithoutTokens
  );

  // The retro-wrap flavour of the same shape (a spent mark, still no token).
  let mut sink = verbose_sink("ab");
  let mark = sink.cst_mark();
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish(K_WRAP);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
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
  let mut sink = verbose_sink("");
  sink.cst_start(K_NODE);
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let root = tree(green.expect("nothing to consume, nothing severed"));
  assert_eq!(root.first_child().expect("Root[Node]").kind(), K_NODE);

  // No structure and no tokens over a nonempty source: the wall stays silent (nothing was
  // built), but every byte is unexplained — the gap-coverage law refuses it, while the
  // tooling door tiles it.
  let (green, _emitter) = verbose_sink("ab").finish(K_ROOT);
  assert_eq!(
    green.expect_err("an unexplained gap, not the honest tree"),
    FinishError::UncoveredGap { start: 0, end: 2 }
  );
  let (green, _emitter) = verbose_sink("ab").finish_partial(K_ROOT);
  assert_eq!(text(green.expect("the partial door tiles it")), "ab");

  // Aborted before the first settle, open node standing: the partial door still opens —
  // the imbalance is the abort witness the wall exempts.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_NODE);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
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
  let mut sink = verbose_sink("a");
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  // The release-build shape (debug builds refuse this at emission): a finish whose start
  // was rolled back away.
  sink.push_raw_event_for_tests(Event::FinishNode { kind: K_NODE });
  let (green, _emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("open nodes refuse the total finish"),
    FinishError::UnclosedNodes { open: 2 }
  );

  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish_partial(K_ROOT);
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
  let mut sink = verbose_sink("a");
  sink.push_raw_event_for_tests(Event::Token {
    kind: TOMBSTONE,
    span: span(0, 1),
  });
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the reserved band never reaches rowan"),
    FinishError::ReservedKind { index: 0 }
  );

  let (green, _emitter) = verbose_sink("").finish(TOMBSTONE);
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
  let profile = CstProfile::new(bad_map, KindValidator::new(in_kind_space), K_ERR, K_GAP);
  let mut sink: VerboseSink<'_> = Sink::new("a", Verbose::new(), profile);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
}

/// Overlapping and non-monotone token spans are refused — the no-duplication half of the
/// round-trip law (a double emission cannot silently duplicate text).
#[test]
fn overlapping_spans_are_refused() {
  let mut sink = verbose_sink("abc");
  sink.cst_token(&MiniTok(b'a'), &span(0, 2));
  sink.cst_token(&MiniTok(b'b'), &span(1, 3));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("overlap is a hard error"),
    FinishError::OverlappingSpans { index: 1 }
  );
}

/// Offsets beyond u32 are refused whole — rowan text sizes are u32 and nothing truncates
/// silently.
#[test]
fn offset_overflow_is_refused() {
  let mut sink = verbose_sink("a");
  sink.cst_token(&MiniTok(b'a'), &span(0, u32::MAX as usize + 10));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("no silent truncation"),
    FinishError::OffsetOverflow { index: 0 }
  );
}

/// A span the source cannot slice (beyond its end) is refused: the events and the source
/// disagree and no tree should pretend otherwise.
#[test]
fn span_out_of_bounds_is_refused() {
  let mut sink = verbose_sink("ab");
  sink.cst_token(&MiniTok(b'a'), &span(0, 5));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("events and source must agree"),
    FinishError::SpanOutOfBounds { index: 0 }
  );
}

/// A StartAt whose target is not a live tombstone is refused (the release backstop
/// behind the panic-at-spend validation).
#[test]
fn stale_start_at_target_is_refused_at_finish() {
  let mut sink = verbose_sink("a");
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.push_raw_event_for_tests(Event::StartAt {
    kind: K_WRAP,
    target: 0,
    prev: None,
  });
  sink.push_raw_event_for_tests(Event::FinishNode { kind: K_WRAP });
  let (green, _emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("a");
  sink.push_raw_event_for_tests(Event::StartNode {
    kind: TOMBSTONE,
    forward_parent: NonZeroU32::new(2),
  });
  sink.push_raw_event_for_tests(Event::Token {
    kind: K_TOK,
    span: span(0, 1),
  });
  let (green, _emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE); // closes K_NODE — the mark is now interior to a closed node
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish(K_WRAP);
  let (green, _emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("1+2+3");
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'1'), &span(0, 1));
  sink.cst_token(&MiniTok(b'+'), &span(1, 2));
  sink.cst_token(&MiniTok(b'2'), &span(2, 3));
  sink.cst_start_at(mark, K_WRAP); // fold 1: Bin[1,+,2]
  sink.cst_finish(K_WRAP);
  sink.cst_token(&MiniTok(b'+'), &span(3, 4));
  sink.cst_token(&MiniTok(b'3'), &span(4, 5));
  sink.cst_start_at(mark, K_WRAP); // fold 2: the OUTER Bin
  sink.cst_finish(K_WRAP);

  let (green, _emitter) = sink.finish(K_ROOT);
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

/// Two wraps on ONE target, closed by two kind-carrying finishes: both are accepted, and the
/// tree is `K_NODE[K_WRAP[tok]]`.
///
/// This is the cell that fixes the order the kind check has to agree with. Same-target wraps
/// are hoisted to the target and opened **newest-first**, so the LAST-declared wrap is the
/// OUTER node and the first `cst_finish` closes the FIRST-declared one. An identity check
/// derived from the emission-order suffix would expect the opposite pairing and refuse this
/// legal history — which is exactly why `Sink::cst_finish` carries no emit-time kind assert.
///
/// Falsified by: a `MismatchedFinish` (the check read the order backwards), or a tree with
/// `K_WRAP` outside `K_NODE`.
#[test]
fn two_wraps_on_one_target_close_in_materialization_order() {
  let mut sink = verbose_sink("a");
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_start_at(mark, K_WRAP); // declared first  → the INNER node
  sink.cst_start_at(mark, K_NODE); // declared second → the OUTER node
  sink.cst_finish(K_WRAP);
  sink.cst_finish(K_NODE);

  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("both closes match the frames they land on");
  assert_eq!(text(green.clone()), "a");
  let root = tree(green);
  let outer = root.first_child().expect("Root[Node]");
  assert_eq!(outer.kind(), K_NODE, "the later wrap is the outer node");
  let inner = outer.first_child().expect("Node[Wrap]");
  assert_eq!(inner.kind(), K_WRAP);
  assert_eq!(inner.text().to_string(), "a");
}

/// The Marker typestate over a real sink: complete wraps the marked region; precede
/// wraps the completed node from the same tombstone (the alias shape, then the outer
/// layer).
#[test]
fn marker_complete_and_precede_build_nested_wraps() {
  use crate::cst::event::Marker;

  let mut sink = verbose_sink("a:b");
  let marker = Marker::new(sink.cst_mark());
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_token(&MiniTok(b':'), &span(1, 2));
  let completed = marker.complete(&mut sink, K_WRAP); // Alias[a, :]
  let outer = completed.precede();
  sink.cst_token(&MiniTok(b'b'), &span(2, 3));
  let _outer = outer.complete(&mut sink, K_NODE); // Field[Alias[a,:], b]

  let (green, _emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("abd");
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink);
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish(K_WRAP);
  rewind(&mut sink, ckp);

  // The retry consumes a different shape.
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'd'), &span(2, 3));
  sink.cst_finish(K_LIST);

  let (green, _emitter) = sink.finish(K_ROOT);
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
    sink.cst_finish(K_NODE);
  };

  let mut straight = verbose_sink("ab");
  drive(&mut straight);
  let (straight_green, _emitter) = straight.finish(K_ROOT);

  let mut backtracked = verbose_sink("ab");
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&backtracked);
  backtracked.cst_start(K_LIST);
  backtracked.cst_token(&MiniTok(b'a'), &span(0, 1));
  rewind(&mut backtracked, ckp);
  drive(&mut backtracked);
  let (backtracked_green, _emitter) = backtracked.finish(K_ROOT);

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
  let mut sink = verbose_sink("a");
  let _unspent = sink.cst_mark();
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  emit_error(&mut sink, 0, 7);
  let (green, emitter) = sink.finish(K_ROOT);
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
  let mut sink = verbose_sink("{xy}");
  sink.cst_token(&MiniTok(b'{'), &span(0, 1));
  // The scan settles two garbage tokens, then the hole is reported.
  sink.cst_token(&MiniTok(b'x'), &span(1, 2));
  sink.cst_token(&MiniTok(b'y'), &span(2, 3));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 3), 2).expect("collects");
  sink.cst_token(&MiniTok(b'}'), &span(3, 4));

  let (green, _emitter) = sink.finish(K_ROOT);
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

  let mut sink = verbose_sink("abc");
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
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  let green = green.expect("token-only timeline, partial parse");
  assert_eq!(text(green), "abc", "committed tokens + the gap-tiled tail");
}

/// Scan-skipped tokens settle behind the frontier and flow to the tree; the stopper the
/// scan examined but did not consume waits for its real consume.
#[test]
fn auto_emission_scan_skips_flow_and_the_stopper_waits() {
  let mut sink = verbose_sink("xy;z");
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
  let mut sink: FatalSink<'_> = Sink::new("!a", Fatal::new(), profile());
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
  let mut sink = verbose_sink("!a");
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
  let (green, _emitter) = sink.finish(K_ROOT);
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
  let mut sink: CountingSink<'_> = Sink::new("a(b)c;d", CountingEmitter::default(), profile());
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
  let mut sink: JournalingSink<'_> = Sink::new("abc", JournalingEmitter::default(), profile());
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
  let mut sink: JournalingSink<'_> = Sink::new("a!bc", JournalingEmitter::default(), profile());
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
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());

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
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());

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
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
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
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
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
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
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
fn sink_with_open_nodes(src: &str, depth: usize) -> VerboseSink<'_> {
  let mut sink = verbose_sink(src);
  for _ in 0..depth {
    sink.cst_start(K_NODE);
  }
  sink
}

/// Closes `depth` enclosing nodes through the handle's emitter — the valid closes whose
/// depth baseline a dead row would corrupt.
fn close_open_nodes(sink: &mut VerboseSink<'_>, depth: usize) {
  for _ in 0..depth {
    CstEmitter::<MiniLexer<'_>, ()>::cst_finish(sink, K_NODE);
  }
}

/// `rollback_to` keeps the target and destroys every younger savepoint. Each of those
/// younger captures aliases the target's mark here, so each must be settled on its own
/// before the single restore.
#[test]
fn stacked_rollback_to_settles_every_aliased_savepoint_row() {
  let mut sink = sink_with_open_nodes("abcdef", 1);
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
  let mut sink = sink_with_open_nodes("abcdef", 1);
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
  let mut sink = sink_with_open_nodes("abcdef", 1);
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
    let mut sink = sink_with_open_nodes("abcdef", depth);
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
  let mut sink = sink_with_open_nodes("abcdef", 1);
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
  let mut sink = sink_with_open_nodes("abcdef", 1);
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
  let mut sink = sink_with_open_nodes("abcdef", 1);
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
    let mut sink: BorrowedSink<'_, '_> = Sink::new("abcdef", &mut verbose, profile());
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

// ── The replay-work law W (§10a) and the Diag-arm pins ─────────────────────────
//
// `W` ticks once at the top of every loop body inside `replay` and its helpers, so it is
// ONE quantity measured identically before and after the walk rewrite — not a needle
// counter, which restates the payload's shape and reads its required GREEN on unfixed
// code. The inventory of iteration constructs (ticked vs. justified) lives on the
// counter's own module, `super::finish::w`.

/// The error-dense payload: `n` alternating (1-byte lexer error, 1-byte token) pairs over a
/// `2n`-byte source. `2n` events (n `Diag`-with-span + n `Token`), `n` gap tiles, no wraps,
/// no trailing close, and — because each gap is explained by its own diagnostic — an `Ok`
/// tree. Returns `(W, events, gap_tiles)`.
fn error_dense_w(n: usize) -> (u64, u64, u64) {
  let src = "!a".repeat(n);
  let mut sink = verbose_sink(&src);
  for i in 0..n {
    let lo = 2 * i;
    Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(lo, lo + 1), MiniErr))
      .expect("verbose collects");
    sink.cst_token(&MiniTok(b'a'), &span(lo + 1, lo + 2));
  }
  super::finish::w::reset();
  let (green, _emitter) = sink.finish(K_ROOT);
  let w = super::finish::w::read();
  let green = green.expect("every gap is explained by the diagnostic that names it");
  assert_eq!(text(green), src, "the payload is lossless at every n");
  (w, 2 * n as u64, n as u64)
}

/// `⌈log₂ k⌉` — the same multiplier `replay` charges the sort by, restated here so the cell
/// derives its expectation rather than copying a number.
fn ceil_log2(k: u64) -> u64 {
  if k < 2 {
    0
  } else {
    u64::from(k.next_power_of_two().trailing_zeros())
  }
}

/// **T-3 — the round's acceptance gate.** Replay work matches its stated shape on error-dense
/// input: **linear in events, plus one `k log k` ordering of the recorded diagnostic spans**.
///
/// The name of this cell used to say "is linear", and the cell used to report 4.00× growth for
/// a 4× input at every size. Both were artifacts: the sort was charged a flat element count,
/// which made the charged quantity linear by construction, so the growth clause could not fail
/// for the reason it existed. Charged at its real cost the same payload reports 4.57× — the
/// sort became visible to its own gate.
///
/// Falsified by: any n outside the two-sided law `events ≤ W ≤ 3 × (events + gap_tiles)`,
/// any per-4×n growth ratio above 4.5, or an exact-composition mismatch against `W == 2n`.
/// The lower bound is not decoration — it is what catches a deleted or misplaced tick, the
/// failure mode an instrument swap is most exposed to.
///
/// Measured on the base (`548fd9a`, before the rewrite): `W` = 5 550 / 82 200 / 1 288 800
/// against the bound 900 / 3 600 / 14 400 — 6.17× / 22.83× / 89.50× over, with per-4×n
/// growth 14.81 / 15.68 (the quadratic signature ≈ 16).
#[test]
fn replay_work_matches_its_stated_shape_on_error_dense_input() {
  let mut ws = std::vec::Vec::new();
  for n in [100usize, 400, 1600] {
    let (w, events, tiles) = error_dense_w(n);
    let diags = n as u64; // one Diag-with-span per pair
    // The law is stated in the shape the code actually has: linear in events, plus the sort's
    // own `k log k`. Folding that term in is what keeps the bound tight — a reintroduced
    // from-zero rescan reads 5 550 / 82 200 / 1 288 800, orders above either term.
    let sort_term = diags * ceil_log2(diags);
    let bound = 3 * (events + tiles) + sort_term;
    assert!(
      w >= events,
      "W = {w} at n = {n} is below the event count {events}: a tick was deleted or \
       misplaced, and the instrument is no longer measuring the walk"
    );
    assert!(
      w <= bound,
      "W = {w} at n = {n} exceeds 3 × (events + gap_tiles) + k·⌈log₂ k⌉ = {bound}: a \
       from-zero rescan is back in the walk"
    );
    // The exact composition, derived term by term from the inventory on
    // `super::finish::w`. An exact pin means any NEW ticked iteration anywhere in `replay`
    // fails here and forces a conscious inventory update — the band alone would absorb it.
    let expected = events            // pass 1: the gather loop, once per event
      + sort_term                    // the sort, charged at its real `k log k` cost
      + diags                        // the cover-merge loop, one per gathered span
      + events                       // pass 2: the walk, once per event
      + (2 * tiles - 1); // the shared cover cursor: one advance per retired interval
    // (n - 1 of them), plus one terminal probe per gap (n)
    assert_eq!(
      w, expected,
      "W = {w} at n = {n} is not the composition the inventory accounts for ({expected}); \
       a new ticked iteration must be added to the inventory deliberately"
    );
    ws.push(w);
  }
  for pair in ws.windows(2) {
    let growth = pair[1] as f64 / pair[0] as f64;
    // 4.00 would be linear. The sort's `k log k` puts this legitimately at ~4.5-4.6 for these
    // sizes, and a reintroduced from-zero rescan reads 14.81 / 15.68. So this clause separates
    // `n log n` from `n²`; it can no longer separate `n log n` from `n`, and the
    // exact-composition assertion above is what does that instead.
    assert!(
      growth <= 4.7,
      "W grew {growth:.2}× for a 4× larger input ({} → {}): past even the sort's k log k",
      pair[0],
      pair[1]
    );
  }
}

/// The exact-composition pin's second cell (§10a): a wrap-bearing payload, where `W` must
/// equal `2 × events + chain_hops` — both passes over every event, plus one hop per
/// retro-wrap link followed. `m` single-wrap targets: 4 events each (mark, token, `StartAt`,
/// finish) and one chain hop each; with no diagnostics, the sort charge, the merge loop and
/// the cover cursor all contribute nothing.
///
/// Falsified by: any other total. In particular the reachability bitvec's own zeroing is
/// *justified*, not ticked (one `alloc_zeroed` of `ceil(events/64)` words, no per-event
/// iteration), so it must not appear in this sum.
///
/// Note what this cell deliberately does **not** measure: the per-materialization `BTreeMap`
/// the chain replaced ticks the same either way, because its work happened inside a keyed
/// container `W` cannot see. That win is real and is measured where it is visible —
/// `benches/cst.rs`, id `finish_wrap_heavy`. Naming the blind spot is the point of having an
/// inventory at all.
#[test]
fn replay_work_on_wraps_is_events_plus_chain_hops() {
  let m = 64usize;
  let src = "a".repeat(m);
  let mut sink = verbose_sink(&src);
  for i in 0..m {
    let mark = sink.cst_mark();
    sink.cst_token(&MiniTok(b'a'), &span(i, i + 1));
    sink.cst_start_at(mark, K_WRAP);
    sink.cst_finish(K_WRAP);
  }
  super::finish::w::reset();
  let (green, _emitter) = sink.finish(K_ROOT);
  let w = super::finish::w::read();
  let green = green.expect("m well-formed single-wrap targets materialize");
  assert_eq!(text(green), src);

  let events = 4 * m as u64; // mark + token + StartAt + finish
  let chain_hops = m as u64; // one StartAt per target
  assert_eq!(
    w,
    2 * events + chain_hops,
    "W must be exactly two passes over {events} events plus {chain_hops} chain hops"
  );
}

/// **T-19 (pin, green on the base and after).** A diagnostic that starts strictly *after*
/// the tiling cursor must not let the run before it escape untiled: the tree stays
/// lossless, and the unexplained prefix is still named as today's leftmost run.
///
/// Teeth: implement the `Diag` arm as `[max(covered, start), end)` (tile == licence) and
/// the text becomes `"acde"` — `tree.text() != source`, losslessness gone — with the
/// refusal moving to `UncoveredGap { start: 3, end: 4 }`.
#[test]
fn diag_starting_after_covered_keeps_the_tree_lossless() {
  // H1: source "abcde", events Token[0,1) · Diag[2,3) · Token[4,5).
  let h1 = |sink: &mut VerboseSink<'_>| {
    sink.cst_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::emit_lexer_error(sink, Spanned::new(span(2, 3), MiniErr))
      .expect("verbose collects");
    sink.cst_token(&MiniTok(b'e'), &span(4, 5));
  };

  let mut sink = verbose_sink("abcde");
  h1(&mut sink);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  assert_eq!(
    text(green.expect("the tooling door tiles every gap")),
    "abcde",
    "no source byte may be skipped by the Diag arm"
  );

  let mut sink = verbose_sink("abcde");
  h1(&mut sink);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("byte [1,2) is covered by no token and named by no diagnostic"),
    FinishError::UncoveredGap { start: 1, end: 2 }
  );
}

/// **T-20 (pin, green on the base and after).** A diagnostic absorbing to the source end
/// still leaves an earlier dropped committed token refused — the tile and the licence are
/// different intervals, and only the licence may explain a byte.
///
/// Teeth: with tile == licence this returns **`Ok`** with text `"acd"` — a dropped
/// committed token silently materialized, the crate's worst failure class.
#[test]
fn diag_absorbing_to_source_end_still_refuses_the_dropped_token() {
  // H2: source "abcd", events Token[0,1) · Diag[2,4).
  let h2 = |sink: &mut VerboseSink<'_>| {
    sink.cst_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::emit_lexer_error(sink, Spanned::new(span(2, 4), MiniErr))
      .expect("verbose collects");
  };

  let mut sink = verbose_sink("abcd");
  h2(&mut sink);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("byte [1,2) is explained by nothing"),
    FinishError::UncoveredGap { start: 1, end: 2 }
  );

  let mut sink = verbose_sink("abcd");
  h2(&mut sink);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  assert_eq!(
    text(green.expect("the tooling door tiles it")),
    "abcd",
    "losslessness holds through the absorbing diagnostic"
  );
}

/// **T-21 (pin, green on the base and after).** `UncoveredGap` keeps its precedence: it is
/// latched during the walk and consumed at the END of it, so every in-walk wall and the
/// balance wall still answer first.
///
/// Teeth: refuse in-walk at the token instead of latching, and all three cells become
/// `UncoveredGap { start: 0, end: 1 }`.
#[test]
fn uncovered_gap_keeps_its_error_precedence() {
  // (A) an unclosed node outranks the uncovered gap.
  let mut sink = verbose_sink("abc");
  sink.cst_start(K_NODE);
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("balance is checked before the gap latch is consumed"),
    FinishError::UnclosedNodes { open: 1 }
  );

  // (B) a later overlapping token span outranks it.
  let mut sink = verbose_sink("abc");
  sink.cst_token(&MiniTok(b'b'), &span(1, 2)); // reveals the unexplained gap [0,1)
  sink.cst_token(&MiniTok(b'a'), &span(0, 1)); // index 1: non-monotone
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the in-walk span wall answers first"),
    FinishError::OverlappingSpans { index: 1 }
  );

  // (C) a later out-of-bounds token span outranks it.
  let mut sink = verbose_sink("abc");
  sink.cst_token(&MiniTok(b'b'), &span(1, 2)); // reveals the unexplained gap [0,1)
  sink.cst_token(&MiniTok(b'c'), &span(2, 99)); // index 1: past the source end
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the in-walk bounds wall answers first"),
    FinishError::SpanOutOfBounds { index: 1 }
  );
}

/// **T-22 (pin, green on the base and after).** A partially explained run names today's
/// exact span — the leftmost byte that no committed token covers and no diagnostic span
/// covers, with the end at the first licence that begins after it.
///
/// Teeth: a bare `[covered, end)` tile with no separate licence names nothing here and
/// returns `Ok`.
#[test]
fn partially_explained_run_names_todays_span() {
  // "abcdef" / Token[0,1) · Diag[3,4) · Token[5,6): [1,3) is unexplained, [3,4) is named.
  let h = |sink: &mut VerboseSink<'_>| {
    sink.cst_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::emit_lexer_error(sink, Spanned::new(span(3, 4), MiniErr))
      .expect("verbose collects");
    sink.cst_token(&MiniTok(b'f'), &span(5, 6));
  };

  let mut sink = verbose_sink("abcdef");
  h(&mut sink);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("[1,3) is covered by neither a token nor a diagnostic"),
    FinishError::UncoveredGap { start: 1, end: 3 }
  );

  let mut sink = verbose_sink("abcdef");
  h(&mut sink);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  assert_eq!(
    text(green.expect("the tooling door tiles the whole run")),
    "abcdef"
  );
}

// ── T-18: the retro-wrap chain ─────────────────────────────────────────────────

/// The chain survives the normal backtracking rhythm. Two wraps on one target, a truncation
/// between them, a regrow, and a fresh spend: the tombstone's head pointer and every `prev`
/// link must describe exactly the wraps that survived.
///
/// This is the property that makes the chain safe without any extra bookkeeping: `prev`
/// always points strictly *older*, and truncation removes only a suffix, so a surviving
/// `StartAt`'s chain is intact by construction. Falsified by: a head pointer naming a
/// truncated slot, a link that outlives the wrap it named, or a materialization that loses a
/// node.
#[test]
fn wrap_chain_survives_rewind_and_regrow() {
  let mut sink = verbose_sink("a");
  let mark = sink.cst_mark(); // index 0
  sink.cst_token(&MiniTok(b'a'), &span(0, 1)); // index 1
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&sink); // the truncation point, index 2

  // The branch that dies: one wrap declared and then rolled back.
  sink.cst_start_at(mark, K_WRAP); // index 2
  assert_eq!(
    sink.forward_parent_at(0),
    NonZeroU32::new(2),
    "the head points at the newest wrap while it is live"
  );
  rewind(&mut sink, ckp);
  assert_eq!(
    sink.forward_parent_at(0),
    None,
    "the journal restored the head along with the truncation"
  );

  // Regrow: two wraps on the same target, the second declared after the first.
  sink.cst_start_at(mark, K_WRAP); // index 2
  sink.cst_start_at(mark, K_NODE); // index 3
  assert_eq!(
    sink.forward_parent_at(0),
    NonZeroU32::new(3),
    "the head names the NEWEST wrap; the older one rides its `prev`"
  );
  sink.cst_finish(K_WRAP);
  sink.cst_finish(K_NODE);

  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("a regrown chain materializes");
  assert_eq!(text(green.clone()), "a");
  // Materialization order is newest-first, so the LAST-declared wrap is the outer node.
  let root = tree(green);
  let outer = root
    .children()
    .next()
    .expect("one retro-wrapped node under the root");
  assert_eq!(outer.kind(), K_NODE, "the later wrap is the outer node");
  let inner = outer
    .children()
    .next()
    .expect("the earlier wrap nests inside");
  assert_eq!(inner.kind(), K_WRAP);
}

/// A `StartAt` that no chain reaches is **refused**, never silently dropped.
///
/// This is the one integrity question following chains introduces. The old shape
/// materialized every `StartAt` from a keyed index built over the whole buffer, so an
/// unreachable one could not exist; a chain walk only materializes what the target's head
/// pointer leads to, and a node that vanishes from an otherwise-`Ok` tree is exactly the
/// lossless-but-wrong-shape failure this walk exists to refuse.
///
/// The cell is built by raw event injection **on purpose**: the public API cannot produce an
/// unreachable `StartAt` — `cst_start_at` writes the head pointer and the journal restores it
/// — and that unreachability-through-the-API is precisely the integrity being pinned, not a
/// gap in it. Falsified by: an `Ok` tree (the node dropped in silence), or any other error.
#[test]
fn unreachable_start_at_is_refused_not_dropped() {
  let mut sink = verbose_sink("a");
  // A tombstone whose head names the SECOND wrap only.
  sink.push_raw_event_for_tests(Event::StartNode {
    kind: TOMBSTONE,
    forward_parent: NonZeroU32::new(3),
  }); // index 0
  sink.push_raw_event_for_tests(Event::Token {
    kind: K_TOK,
    span: span(0, 1),
  }); // index 1
  // index 2: a wrap of the same target that the chain does not reach — its `prev` is unset,
  // and the head skips over it.
  sink.push_raw_event_for_tests(Event::StartAt {
    kind: K_WRAP,
    target: 0,
    prev: None,
  });
  sink.push_raw_event_for_tests(Event::StartAt {
    kind: K_NODE,
    target: 0,
    prev: None,
  }); // index 3
  sink.push_raw_event_for_tests(Event::FinishNode { kind: K_NODE });

  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("an unreachable retro-wrap is corruption, not a node to drop"),
    FinishError::DanglingForwardParent { index: 0 }
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The boundary-validation wave: the profile refuses its own contradictions at
// construction, and materialization refuses out-of-language kinds, malformed
// diagnostic spans, and non-UTF-8 sources
// ═══════════════════════════════════════════════════════════════════════════════

/// A profile whose gap kind IS the reserved tombstone is a contradiction the sink could never
/// materialize (its own tiling would leak the reserved band), so it is refused where the
/// mistake is — at construction, in every build — and the message names the field.
///
/// Falsified by: no panic, or a panic that does not name `CstProfile.gap_kind`.
#[test]
#[should_panic(expected = "CstProfile.gap_kind")]
fn tombstone_gap_kind_refused_at_construction() {
  let _ = CstProfile::new(map_tok, KindValidator::accept_all(), K_ERR, TOMBSTONE);
}

/// The error-kind twin of the cell above: a recovery-hole wrap of the reserved kind would be
/// refused by the sink's own materialization, so the profile refuses it first.
///
/// Falsified by: no panic, or a panic that does not name `CstProfile.error_kind`.
#[test]
#[should_panic(expected = "CstProfile.error_kind")]
fn tombstone_error_kind_refused_at_construction() {
  let _ = CstProfile::new(map_tok, KindValidator::accept_all(), TOMBSTONE, K_GAP);
}

/// A kind the dialect's own validator rejects never reaches rowan: materialization refuses it
/// with the offending index and value, instead of leaving a `kind_from_raw` panic to fire at
/// some later query.
///
/// Falsified by: an `Ok` tree, a `ReservedKind` (60 000 is not the reserved band), or an
/// `InvalidDialectKind` naming a different kind.
///
/// Both halves reach materialization through raw event injection, which is the point: that is
/// the ONE route by which a kind arrives without passing a door, and it is `pub(crate)`. Every
/// route a caller outside this crate can take — `cst_start`, `cst_start_at`, and the single
/// `record_token` body behind both token doors — validates in every build and panics at the
/// cause. This wall is the in-crate backstop for the raw route, and it is
/// `debug_assertions`-gated: it has teeth in every test run and in CI, and a release build
/// pays nothing for it.
///
/// **Debug-only, and deliberately so.** The wall this exercises is `cfg!(debug_assertions)`-
/// gated in the walk, because keeping it per-event in release cost a measured 8.3% on ordinary
/// materialization while every externally reachable door already validates unconditionally.
/// This cell would therefore pass vacuously under `cargo test --release` — it would observe an
/// `Ok` tree and no refusal — so it is compiled only where the wall exists. The doors' own
/// every-build refusals are pinned separately and run in both profiles.
#[cfg(debug_assertions)]
#[test]
fn out_of_language_kind_refused() {
  const OUT_OF_LANGUAGE: u16 = 60_000;

  // A dialect that names only the low kinds — and a stream that leaks one far above them.
  let narrow = CstProfile::new(map_tok, KindValidator::new(|k| k < 100), K_ERR, K_GAP);
  let mut sink: VerboseSink<'_> = Sink::new("a", Verbose::new(), narrow);
  sink.push_raw_event_for_tests(Event::Token {
    kind: OUT_OF_LANGUAGE,
    span: span(0, 1),
  });
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("a kind outside the dialect's space is not a tree"),
    FinishError::InvalidDialectKind {
      index: 0,
      kind: OUT_OF_LANGUAGE
    }
  );

  // The node channel is walled by the same predicate, from the same one call.
  let narrow = CstProfile::new(map_tok, KindValidator::new(|k| k < 100), K_ERR, K_GAP);
  let mut sink: VerboseSink<'_> = Sink::new("a", Verbose::new(), narrow);
  sink.push_raw_event_for_tests(Event::StartNode {
    kind: OUT_OF_LANGUAGE,
    forward_parent: None,
  });
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("node kinds are validated too"),
    FinishError::InvalidDialectKind {
      index: 0,
      kind: OUT_OF_LANGUAGE
    }
  );
}

/// A diagnostic span is evidence — it is the one thing that licenses a gap tile — so a span
/// that does not slice the source is refused rather than clamped into range, through **both**
/// doors. The old clamp turned `0..99` over `"abc"` into `0..3`, which covered the dropped
/// byte and let the tree through.
///
/// The control is the point of the cell: without the diagnostic the same stream yields
/// `UncoveredGap { start: 0, end: 2 }`, so the refusal above is the span wall firing and not
/// the gap law firing for an unrelated reason.
///
/// Falsified by: an `Ok` tree from either door, an `UncoveredGap` where the malformed span is
/// present, or a control that does not reach `UncoveredGap`.
#[test]
fn malformed_diag_span_refused_by_both_doors() {
  let malformed = |sink: &mut VerboseSink<'_>| {
    // `0..99` over a 3-byte source: an end far past the source end.
    Emitter::<MiniLexer<'_>>::emit_lexer_error(sink, Spanned::new(span(0, 99), MiniErr))
      .expect("verbose collects");
    sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  };

  let mut sink = verbose_sink("abc");
  malformed(&mut sink);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("a span that cannot slice the source licenses nothing"),
    FinishError::InvalidDiagnosticSpan { index: 0 }
  );

  let mut sink = verbose_sink("abc");
  malformed(&mut sink);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  assert_eq!(
    green.expect_err("the tooling door tolerates incompleteness, not corruption"),
    FinishError::InvalidDiagnosticSpan { index: 0 }
  );

  // The control: the same dropped bytes, no diagnostic at all.
  let mut sink = verbose_sink("abc");
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the cell must not be passing for the wrong reason"),
    FinishError::UncoveredGap { start: 0, end: 2 }
  );
}

// ── A byte-backed source: the CstText validation boundary ──────────────────────

/// The byte-source twin of [`MiniLexer`]: one byte per token, `Source = [u8]`, so the
/// materialization path has to go through `CstText`'s validating impl rather than the
/// infallible `str` one.
struct ByteSrcLexer<'inp> {
  src: &'inp [u8],
  tok_start: usize,
  pos: usize,
  state: (),
}

impl<'inp> Lexer<'inp> for ByteSrcLexer<'inp> {
  type State = ();
  type Source = [u8];
  type Token = MiniTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp [u8]) -> Self {
    Self {
      src,
      tok_start: 0,
      pos: 0,
      state: (),
    }
  }

  fn with_state(src: &'inp [u8], state: ()) -> Self {
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

  fn state(&self) -> &() {
    &self.state
  }

  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }

  fn into_state(self) {
    self.state
  }

  fn source(&self) -> &'inp [u8] {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.tok_start, self.pos)
  }

  fn slice(&self) -> &'inp [u8] {
    &self.src[self.tok_start..self.pos]
  }

  fn lex(&mut self) -> Option<Result<MiniTok, MiniErr>> {
    let byte = *self.src.get(self.pos)?;
    self.tok_start = self.pos;
    self.pos += 1;
    Some(Ok(MiniTok(byte)))
  }

  fn bump(&mut self, n: &usize) {
    self.pos += *n;
    self.tok_start = self.pos;
  }
}

type ByteSink<'inp> = Sink<'inp, ByteSrcLexer<'inp>, Verbose<TestErr>>;

/// A byte-backed source whose bytes ARE valid UTF-8 materializes exactly as a `str` source
/// would: `CstText` validates once, at `finish`, and hands the walk a `&str`.
///
/// Falsified by: any `Err`, or a tree whose text is not the source.
#[test]
fn utf8_byte_source_materializes() {
  let src: &[u8] = "aé".as_bytes(); // 3 bytes: 'a', then a 2-byte 'é'
  let mut sink: ByteSink<'_> = Sink::new(src, Verbose::new(), profile());
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_token(&MiniTok(0xC3), &span(1, 3));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    text(green.expect("valid UTF-8 bytes are a source like any other")),
    "aé"
  );
}

/// A byte-backed source that is NOT UTF-8 cannot become a green tree — rowan stores text as
/// `&str` — so it is refused once, whole, naming how far the source was valid. Nothing is
/// lossily transcoded and no token is dropped.
///
/// Falsified by: an `Ok` tree, a per-token `SpanOutOfBounds`, or a `valid_up_to` that is not
/// the first invalid byte's offset.
#[test]
fn non_utf8_byte_source_refused() {
  // `a`, then a lone continuation byte: invalid from offset 1.
  let src: &[u8] = &[b'a', 0x80, b'c'];
  let mut sink: ByteSink<'_> = Sink::new(src, Verbose::new(), profile());
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("a non-UTF-8 source has no green tree"),
    FinishError::NonUtf8Source { valid_up_to: 1 }
  );
}

// ── The rowan-side payoff: an Ok tree never panics a conforming Language ───────

/// A strict `rowan::Language` whose `kind_from_raw` refuses anything outside the fixture's
/// kind space — the shape a real dialect writes when it maps raw u16s back to a `#[repr(u16)]`
/// enum by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum StrictLang {}

impl rowan::Language for StrictLang {
  type Kind = u16;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> u16 {
    assert!(
      in_kind_space(raw.0),
      "kind {} is outside the dialect's kind space",
      raw.0
    );
    raw.0
  }

  fn kind_to_raw(kind: u16) -> rowan::SyntaxKind {
    rowan::SyntaxKind(kind)
  }
}

/// The wave's whole point, stated as a property: if `finish` returns `Ok`, every kind in the
/// tree — the root, direct nodes, retro-wraps, the synthesized error node, committed tokens,
/// and the synthesized gap tile — is one the dialect's validator admits. A full traversal
/// through a `Language` that asserts that range must therefore complete without panicking.
///
/// Falsified by: a panic from `kind_from_raw` (some channel synthesizes a kind outside the
/// declared space), or an `Err` from a stream built entirely of admitted kinds.
#[test]
fn no_ok_tree_panics_a_conforming_language() {
  let mut sink = verbose_sink("abcde");
  sink.cst_start(K_NODE);
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));

  // A retro-wrap.
  let mark = sink.cst_mark();
  sink.cst_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish(K_WRAP);

  // A direct node.
  sink.cst_start(K_LIST);
  sink.cst_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish(K_LIST);

  // A recovery hole: the sink synthesizes the K_ERR wrap itself.
  sink.cst_token(&MiniTok(b'd'), &span(3, 4));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(3, 4), 1).expect("collects");

  // A refused byte: the sink synthesizes the K_GAP tile itself.
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(4, 5), MiniErr))
    .expect("verbose collects");
  sink.cst_finish(K_NODE);

  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("every kind in this stream is admitted by the fixture validator");
  let root = rowan::SyntaxNode::<StrictLang>::new_root(green);
  assert_eq!(root.text().to_string(), "abcde");

  // The traversal is the assertion: every `kind()` runs `kind_from_raw`.
  let seen: std::vec::Vec<u16> = root
    .descendants_with_tokens()
    .map(|element| element.kind())
    .collect();
  assert!(
    seen.contains(&K_ERR) && seen.contains(&K_GAP) && seen.contains(&K_WRAP),
    "the cell must actually exercise the synthesized kinds: {seen:?}"
  );
}

// ── The profile is reusable across constructions, for ANY token type ───────────

/// A token type that is deliberately NOT `Copy`: it owns a `String`. The [`Token`] trait asks
/// only for `Clone`, so this is a legal dialect token — and it is the one shape that can tell
/// a hand-written `Copy`/`Clone` on [`CstProfile`] apart from a derived one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnedTok(std::string::String);

impl Token<'_> for OwnedTok {
  type Kind = u8;
  type Error = MiniErr;

  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> u8 {
    0
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

struct OwnedLexer<'inp> {
  src: &'inp str,
  pos: usize,
  state: (),
}

impl<'inp> Lexer<'inp> for OwnedLexer<'inp> {
  type State = ();
  type Source = str;
  type Token = OwnedTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp str) -> Self {
    Self {
      src,
      pos: 0,
      state: (),
    }
  }

  fn with_state(src: &'inp str, state: ()) -> Self {
    Self { src, pos: 0, state }
  }

  fn check(&self) -> Result<(), MiniErr> {
    Ok(())
  }

  fn state(&self) -> &() {
    &self.state
  }

  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }

  fn into_state(self) {
    self.state
  }

  fn source(&self) -> &'inp str {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.pos, self.pos)
  }

  fn slice(&self) -> &'inp str {
    ""
  }

  fn lex(&mut self) -> Option<Result<OwnedTok, MiniErr>> {
    let byte = *self.src.as_bytes().get(self.pos)?;
    self.pos += 1;
    Some(Ok(OwnedTok(
      std::string::String::from_utf8_lossy(&[byte]).into_owned(),
    )))
  }

  fn bump(&mut self, n: &usize) {
    self.pos += *n;
  }
}

fn map_owned(_: &OwnedTok) -> u16 {
  K_TOK
}

/// One [`CstProfile`] value, two sinks — over a dialect whose token type is **not** `Copy`.
///
/// This is the cell that makes the hand-written `Debug`/`Clone`/`Copy` impls testable at all:
/// the in-tree `MiniTok` is `Copy`, so a derived `Copy` (which would add `T: Copy`) would go
/// unnoticed against it. Here a derive fails to compile — the second `Sink::new` reports
/// `error[E0382]: use of moved value` — and `Debug` would additionally demand `T: Debug`.
///
/// Falsified by: a compile error at either construction, or a `Debug` render that leaks a
/// function pointer's code address (which changes every build).
#[test]
fn profile_is_reusable_with_a_non_copy_token() {
  let profile: CstProfile<OwnedTok> =
    CstProfile::new(map_owned, KindValidator::new(in_kind_space), K_ERR, K_GAP);

  // The same profile VALUE, used twice: only a `Copy` free of a `T: Copy` bound allows this.
  let first: Sink<'_, OwnedLexer<'_>, Verbose<TestErr>> = Sink::new("a", Verbose::new(), profile);
  let second: Sink<'_, OwnedLexer<'_>, Verbose<TestErr>> = Sink::new("b", Verbose::new(), profile);

  assert_eq!(first.error_kind(), K_ERR);
  assert_eq!(second.gap_kind(), K_GAP);

  // Debug is hand-written too, and renders no code address for the mapper or the validator.
  let rendered = std::format!("{profile:?}");
  assert_eq!(rendered, "CstProfile { error_kind: 90, gap_kind: 91, .. }");
  assert_eq!(
    std::format!("{:?}", profile.validator()),
    "KindValidator(..)"
  );
}

/// The session's own error type, kept separate from `TestErr` so the partial-mode and
/// session conversions do not widen an error every other cell in this file matches on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SesErr {
  Lex,
  Incomplete(usize),
  Refused(crate::input::SessionRefusal),
}

impl From<MiniErr> for SesErr {
  fn from(_: MiniErr) -> Self {
    Self::Lex
  }
}

impl From<crate::error::Incomplete<usize>> for SesErr {
  fn from(inc: crate::error::Incomplete<usize>) -> Self {
    Self::Incomplete(inc.into_offset())
  }
}

impl From<crate::input::SessionRefusal> for SesErr {
  fn from(refusal: crate::input::SessionRefusal) -> Self {
    Self::Refused(refusal)
  }
}

impl crate::error::MaybeIncomplete for SesErr {
  fn is_incomplete(&self) -> bool {
    matches!(self, Self::Incomplete(_))
  }
}

impl crate::error::MaybeTerminal for SesErr {
  fn is_terminal(&self) -> bool {
    matches!(self, Self::Refused(_))
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for SesErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Self::Lex
  }
}

type SesSink<'inp> = Sink<'inp, MiniLexer<'inp>, Verbose<SesErr>>;
type SesCtx<'inp, 'e> = (&'e mut SesSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);

// ── Two unchecked identities ───────────────────────────────────────────────────

/// **A PIN OF A KNOWN-OPEN DEFECT, not of correct behaviour.** It asserts today's wrong
/// answer so that closing the hole flips it, and so the hole cannot be quietly forgotten.
///
/// `Sink::new` binds a source, and `'inp` proves that borrow and the parser's borrow both
/// outlive the sink — **not that they are the same buffer**. Nothing ties them: no pointer
/// comparison, no length comparison, no identity. So a sink bound to one buffer and used
/// while parsing another of the same length materializes a tree whose *text* is the sink's
/// buffer and whose *structure* came from the parser's, and every existing wall passes it.
///
/// Equal length is the point: a length check alone would not catch this, so a future fix
/// cannot satisfy this cell cheaply.
///
/// What binding at construction actually bought is the removal of the **late** half of this
/// class — `finish` no longer takes a source, so it can no longer be handed a different one.
/// The **early** half, choosing the wrong buffer at construction, is still open. Closing it
/// needs an identity the sink can check, and the sink observes no handle on the parser's
/// buffer through any `Emitter` hook, so it belongs with the emitter/context contract rather
/// than here.
///
/// Flips when that lands: this cell should then refuse, and its `expect` becomes an
/// `expect_err`.
#[test]
fn sink_bound_to_a_foreign_source_is_not_yet_detected() {
  let mut sink: SesSink<'_> = Sink::new("XY", Verbose::new(), profile());
  {
    let mut borrowed = &mut sink;
    let mut input = crate::input::Input::<MiniLexer<'_>, SesCtx<'_, '_>, ()>::new("ab");
    let mut inp = input.as_ref(&mut borrowed);
    while let Ok(Some(_)) = inp.next() {}
  }
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("equal length hides the mismatch from every existing wall");
  assert_eq!(
    text(green),
    "XY",
    "the parse consumed \"ab\"; the tree's text is the sink's own buffer — the wrong-tree \
     class this binding was supposed to close, still open on its early half"
  );
}

/// **A PIN OF A KNOWN-OPEN DEFECT.** Duplicated **zero-width** token events survive: the
/// monotone wall is `start < covered`, and two `[0, 0)` spans never satisfy it.
///
/// **This is a SEPARATE defect from the parse-to-emitter identity gap, and it is deliberately
/// not grouped with it.** Nothing here involves a session, a second attempt, or two parses:
/// the log below is one a *single* parse could produce, so an identity check between a parse
/// and its emitter would close nothing here and this cell would keep passing on its wrong
/// answer. What it actually pins is a materialization property — `finish` does not refuse
/// duplicate zero-width token spans — and it flips only when materialization starts doing so.
///
/// Zero-width committed tokens are outside what a conforming lexer produces: a token is a span
/// of consumed bytes, and a lexer that returns them without advancing does not terminate. So
/// this is unreachable by driving a well-behaved dialect, which is why it is a smaller defect
/// than its neighbours rather than a member of their class. It is pinned anyway for two routes
/// the crate already treats as real: the raw event surface used here, and a third-party
/// `Lexer` that does not honour the contract — the sink's release walls exist precisely for
/// histories the emission-time asserts refuse.
#[test]
fn duplicate_zero_width_tokens_are_not_yet_detected() {
  let mut sink = verbose_sink("a");
  for _ in 0..2 {
    sink.push_raw_event_for_tests(Event::Token {
      kind: K_TOK,
      span: span(0, 0),
    });
  }
  sink.cst_token(&MiniTok(b'a'), &span(0, 1));

  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("zero-width spans slip past the monotone wall");
  assert_eq!(text(green.clone()), "a");
  let kinds: std::vec::Vec<u16> = tree(green)
    .children_with_tokens()
    .map(|e| e.kind())
    .collect();
  assert_eq!(
    kinds,
    std::vec![K_TOK, K_TOK, K_TOK],
    "three tokens where one was real: the duplicates are indistinguishable in the tree"
  );
}
