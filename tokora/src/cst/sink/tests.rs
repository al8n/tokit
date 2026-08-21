//! The named-regression suite of the rewindable event sink: the failure-corpus scenarios
//! (F-A1/F-A2/F-A3/F-A5, T3) at the mechanism level, the unified-log exactness laws, and
//! the CST_FORWARD_CENSUS source lock.

use core::num::NonZeroU32;

use crate::{
  Lexer, SimpleSpan,
  cache::DefaultCache,
  cst::{
    CstProfile, KindValidator,
    event::{Event, EventMark, TOMBSTONE},
  },
  emitter::{CstEmitter, Emitter, Fatal, Verbose},
  error::token::{UnexpectedToken, UnexpectedTokenOf},
  input::{Balance, Cursor, Input},
  span::Spanned,
  token::Token,
};

use super::{Sink, TriviaPolicy};

// ── A tiny real lexer: one byte per token, `!` is a lexer error ─────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MiniTok(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MiniErr;

impl Token<'_> for MiniTok {
  type Kind = u8;
  type Error = MiniErr;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

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

  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
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
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs,
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

  impl<O, Lang: ?Sized> From<RecursionLimitReached<O, Lang>> for TestErr {
    fn from(_: RecursionLimitReached<O, Lang>) -> Self {
      Self::Unexpected
    }
  }

  impl<O, Lang: ?Sized> From<NonAssociativeChain<O, Lang>> for TestErr {
    fn from(_: NonAssociativeChain<O, Lang>) -> Self {
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

/// CELL_CENSUS's type-level half: **the sink holds no cell**, so `&Sink` — which is exactly what
/// [`InputRef::emitter_ref`](crate::InputRef::emitter_ref) hands a parser mid-parse — writes
/// nothing. The mark stack was a `RefCell` until al8n/tokora#257 purely because
/// [`Emitter::checkpoint`] took `&self`, and a caller could push a row through it that no settle
/// would ever spend.
///
/// `Sync` is the pin because every cell in `core::cell` is `!Sync`: reintroducing one fails this
/// line rather than a comment. It is a pin on that mechanism and not a proof of immutability in
/// general — an atomic is `Sync` and would pass — and it is not a threading promise; nothing in
/// the crate sends a live sink anywhere. The claim being held is the one in the field's own doc:
/// there is no cell for a shared reference to write through.
#[test]
fn the_sink_holds_no_cell_a_shared_reference_could_write_through() {
  const fn assert_sync<T: Sync>() {}
  assert_sync::<VerboseSink<'_>>();
  assert_sync::<FatalSink<'_>>();
}

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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  let mark = sink.cst_mark();
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  rewind(&mut sink, ckp);
  // Regrow: a token now occupies the mark's old index.
  sink.record_token(&MiniTok(b'b'), &span(0, 1));
  sink.cst_start_at(mark, K_WRAP);
}

/// The sharpest alias: the regrown event at the mark's index is ANOTHER tombstone, so the
/// positional check alone would validate it — only the era distinguishes the histories.
#[test]
#[should_panic(expected = "stale EventMark")]
fn stale_mark_panics_even_over_a_regrown_tombstone() {
  let mut sink = verbose_sink("");
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  rewind(&mut sink, ckp);
  sink.record_token(&MiniTok(b'c'), &span(1, 2));
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish(K_WRAP);
  assert_eq!(
    sink.forward_parent_at(0),
    NonZeroU32::new(3),
    "the wrap landed on the surviving tombstone (StartAt at index 3, target 0)"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The up-front bracket — `cst_start`'s handback and `cst_demote`'s wall
// ═══════════════════════════════════════════════════════════════════════════════
//
// `node(kind, p)` as a `ParseInput` names the node's kind at entry and closes on both exits:
// `cst_finish` on success, `cst_demote` on failure. BOTH exits are appends — the failing one
// appends an `Event::Demote` naming its own start, and materialization's canonicalization pass
// applies it — so the bracket obeys ONE rollback law whichever exit it took (the law lives in
// `cst/event.rs`). These cells hold the properties that make it so: the abandoned node
// materializes into nothing, the demote survives exactly the truncations that keep it and dies
// with the ones that do not, a rollback into the bracket's own window reopens the node on
// either exit, and every misuse of the handback is refused — in every build at the wall, or at
// the two calibrated tiers for the two the slot cannot witness.

/// The demote leaves the buffer **balanced and append-only**: one extra event, no journal
/// entry, no interior write, and the abandoned node materializes into nothing.
#[test]
fn demote_materialises_as_inert() {
  let mut sink = verbose_sink("a");
  let mark = sink.cst_start(K_NODE);
  assert_eq!(mark.index(), 0, "the handback names the slot just appended");
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_demote(mark, K_NODE);

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
      Event::Demote { target: 0 },
    ],
    "the demote APPENDS: the slot keeps its real kind for the whole parse, and the decision \
     rides an event that a rollback can truncate"
  );
  assert_eq!(
    sink.journal_len(),
    0,
    "an appended event needs no undo entry — the truncation IS the undo"
  );

  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("a demoted start leaves a balanced buffer");
  assert_eq!(text(green.clone()), "a");
  let root = tree(green);
  assert_eq!(
    root.children().count(),
    0,
    "canonicalization tombstones the slot; the abandoned node materializes into nothing"
  );
  assert_eq!(root.children_with_tokens().count(), 1, "the token is loose");
}

/// **Why the four-corpora byte-identity gate can hold at all.** The two brackets' failing exits
/// converge — at *materialization*, not in the buffer. The retro bracket leaves a `cst_mark`
/// tombstone unspent; the up-front bracket leaves a real-kind start plus the `Demote` that
/// names it, which canonicalization turns into exactly that tombstone before the walk begins.
/// So converting `ParseInput for Node` leaves every tree the error path produces bit-for-bit
/// alone, which is what the corpus hashes measure.
#[test]
fn a_demoted_start_and_an_unspent_mark_materialize_identically() {
  let mut up_front = verbose_sink("a");
  let mark = up_front.cst_start(K_NODE);
  up_front.record_token(&MiniTok(b'a'), &span(0, 1));
  up_front.cst_demote(mark, K_NODE);

  let mut retro = verbose_sink("a");
  let _unspent = retro.cst_mark();
  retro.record_token(&MiniTok(b'a'), &span(0, 1));

  assert_ne!(
    up_front.events(),
    retro.events(),
    "the buffers deliberately differ: the demote is an event, not a rewrite"
  );
  assert_eq!(
    up_front.journal_len(),
    retro.journal_len(),
    "both journal 0"
  );

  let (up_front_green, _emitter) = up_front.finish(K_ROOT);
  let (retro_green, _emitter) = retro.finish(K_ROOT);
  assert_eq!(
    up_front_green.expect("balanced"),
    retro_green.expect("balanced"),
    "the up-front bracket's Err exit materializes the retro bracket's Err exit, byte for byte"
  );
}

/// A truncation **strictly above** the `Demote` event leaves the demotion standing — the
/// ordinary direction: the bracket exited with an error and nothing rewound into its window,
/// so the abandoned node stays abandoned.
#[test]
fn demote_survives_a_truncation_strictly_above_it() {
  let mut sink = verbose_sink("ab");
  let mark = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_demote(mark, K_NODE);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  rewind(&mut sink, ckp);

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
      Event::Demote { target: 0 },
    ],
    "the rewind dropped the events above the mark and left the demote event standing"
  );
  // Balance is checked before the gap latch is consumed, so the only complaint being the
  // rewound `b`'s uncovered byte IS the still-balanced proof: a surviving Start with no
  // surviving Demote would have answered `UnclosedNodes` first.
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the rewound token leaves byte 1 uncovered"),
    FinishError::UncoveredGap { start: 1, end: 2 }
  );
}

/// A truncation **at or below** the slot erases the slot itself — which is precisely why the
/// write needs no journal entry: there is nothing left for a reverse-replay to restore onto.
#[test]
fn demote_dies_with_a_truncation_at_or_below_it() {
  let mut sink = verbose_sink("a");
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  let mark = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_demote(mark, K_NODE);
  rewind(&mut sink, ckp);

  assert!(
    sink.events().is_empty(),
    "the truncation erased the very slot the write was made on"
  );
  assert_eq!(sink.journal_len(), 0);
}

/// The sharpest staleness alias, the demote twin of
/// `stale_mark_panics_even_over_a_regrown_tombstone`: the regrown event at the mark's index is
/// another `StartNode` of the *same kind*, so the positional and kind checks alone would both
/// validate — only the era separates the histories.
#[test]
#[should_panic(expected = "cst_demote on a mark")]
fn stale_demote_panics_even_over_a_regrown_start_of_the_same_kind() {
  let mut sink = verbose_sink("");
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  let dead = sink.cst_start(K_NODE);
  rewind(&mut sink, ckp);
  let live = sink.cst_start(K_NODE);
  assert_eq!(live.index(), dead.index());
  sink.cst_demote(dead, K_NODE);
}

/// A mark minted by one sink must not demote on another, even at the exact `(index: 0, era: 0)`
/// collision two fresh sinks mint — the identity half of the wall, unconditional in every build.
#[test]
#[should_panic(expected = "different sink")]
fn foreign_sink_demote_panics() {
  let mut a = verbose_sink("");
  let mut b = verbose_sink("");
  let mark_a = a.cst_start(K_NODE);
  let mark_b = b.cst_start(K_NODE);
  assert_eq!(mark_a.index(), mark_b.index());
  assert_eq!(mark_a.era(), mark_b.era());
  b.cst_demote(mark_a, K_NODE);
}

/// An inert mark (a diagnostics-only emitter's defaulted `cst_start`) can never demote on a
/// recording sink: its reserved witness names no sink.
#[test]
#[should_panic(expected = "different sink")]
fn inert_mark_demote_panics() {
  let mut fatal = Fatal::<TestErr>::new();
  let inert = CstEmitter::<MiniLexer<'_>>::cst_start(&mut fatal, K_NODE);
  let mut sink = verbose_sink("");
  sink.cst_demote(inert, K_NODE);
}

/// The kind is checked, not merely carried: demoting with the wrong kind is the leaked-finish
/// shape moved to the emit site, and the mark names the exact slot to check, so it is caught
/// here rather than deferred to materialization.
#[test]
#[should_panic(expected = "cst_demote on a mark")]
fn wrong_kind_demote_panics() {
  let mut sink = verbose_sink("");
  let mark = sink.cst_start(K_NODE);
  sink.cst_demote(mark, K_LIST);
}

/// The two mark provenances do not cross in this direction either: a `cst_mark` tombstone is
/// spent by `cst_start_at` and is never demotable. This is the half the slot-content check
/// answers on its own — a real-kind argument against a `TOMBSTONE` slot. The other half, a
/// `TOMBSTONE` **argument**, is not a content question at all and is refused separately; see
/// `demote_with_the_tombstone_kind_panics`.
#[test]
#[should_panic(expected = "cst_demote on a mark")]
fn demote_of_a_cst_mark_tombstone_panics() {
  let mut sink = verbose_sink("");
  let mark = sink.cst_mark();
  sink.cst_demote(mark, K_NODE);
}

/// The reserved kind is refused as an **argument**, before the slot is read at all — the check
/// that makes "the spend verbs do not cross" true rather than almost-true.
///
/// A content-only wall compares the slot's kind against the caller's, so `TOMBSTONE` against a
/// live tombstone *matches*, and the demote then writes `TOMBSTONE` over `TOMBSTONE`. In a
/// release build that is a silent no-op on a slot the caller never opened. The sharper shape it
/// also retires is a **retro-wrapped** tombstone, whose `forward_parent` is live: the same
/// content-only wall waves that through too, and only `cst_demote`'s debug `forward_parent`
/// canary would have caught it, far from the mistake. One compare against an immediate closes
/// both, in every build.
#[test]
#[should_panic(expected = "reserved TOMBSTONE kind")]
fn demote_with_the_tombstone_kind_panics() {
  let mut sink = verbose_sink("");
  let mark = sink.cst_mark();
  sink.cst_demote(mark, TOMBSTONE);
}

/// **Single-use, at the two tiers the appended encoding leaves available.** A demote no longer
/// rewrites the slot, so the slot cannot witness that it was already demoted and the
/// every-build content wall has nothing to read — the same reason it cannot see a finish. The
/// debug tier catches it at cause: the prior `Demote` is a −1 above the mark, so the exact
/// suffix recount dips.
///
/// That is a *narrowing* of the every-build refusal this bracket shipped with, taken
/// deliberately in exchange for one rollback law across both exits, and the release tier below
/// is what keeps it out of the silent class.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a DOUBLE demote")]
fn a_second_demote_of_one_mark_panics_in_debug() {
  let mut sink = verbose_sink("");
  let mark = sink.cst_start(K_NODE);
  sink.cst_demote(mark, K_NODE);
  sink.cst_demote(mark, K_NODE);
}

/// **…and what a release build does with the double-demote residue instead**: canonicalization
/// refuses it, typed, through both doors. The buffer is built *directly* so the cell runs in
/// every build — in a debug build the misuse is refused at cause by the cell above and never
/// reaches materialization at all — and byte for byte it is what
/// `cst_start(K_NODE); cst_demote(mark, K_NODE); cst_demote(mark, K_NODE)` leaves behind.
///
/// The second pass over one target finds the slot its predecessor already tombstoned; there is
/// no reading of that buffer under which two brackets closed, so it is corruption rather than
/// incompleteness and `finish_partial` refuses it too.
#[test]
fn the_double_demote_residue_is_refused_typed_through_both_doors() {
  fn residue(src: &str) -> VerboseSink<'_> {
    let mut sink = verbose_sink(src);
    sink.push_raw_event_for_tests(Event::StartNode {
      kind: K_NODE,
      forward_parent: None,
    });
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    // Raw, because in a debug build `cst_demote`'s own suffix scan refuses the second one —
    // which is the very misuse this cell is about.
    sink.push_raw_event_for_tests(Event::Demote { target: 0 });
    sink.push_raw_event_for_tests(Event::Demote { target: 0 });
    sink
  }

  let (green, _emitter) = residue("a").finish(K_ROOT);
  assert_eq!(
    green.expect_err("the strict door refuses the second demote"),
    FinishError::StaleDemote {
      index: 3,
      target: 0
    }
  );

  let (green, _emitter) = residue("a").finish_partial(K_ROOT);
  assert_eq!(
    green.expect_err("and so does the tooling door — one canonicalization, one verdict"),
    FinishError::StaleDemote {
      index: 3,
      target: 0
    },
    "close_open_nodes relaxes end-of-stream opens, not a stream that closed one node twice"
  );
}

/// The canonicalization wall is positional, so it also backstops every shape the emission walls
/// already refuse in every build but the raw injection hook can still construct: a `Demote`
/// naming a `cst_mark` tombstone, and one naming a slot that is not a node start at all.
#[test]
fn canonicalization_refuses_a_demote_that_names_no_live_start() {
  let mut sink = verbose_sink("a");
  let _tomb = sink.cst_mark();
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.push_raw_event_for_tests(Event::Demote { target: 0 });
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("a cst_mark tombstone is spent by cst_start_at, never demoted"),
    FinishError::StaleDemote {
      index: 2,
      target: 0
    }
  );

  let mut sink = verbose_sink("a");
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.push_raw_event_for_tests(Event::Demote { target: 0 });
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the target holds a token, not a start"),
    FinishError::StaleDemote {
      index: 1,
      target: 0
    }
  );

  let mut sink = verbose_sink("a");
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.push_raw_event_for_tests(Event::Demote { target: 99 });
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("out of bounds"),
    FinishError::StaleDemote {
      index: 1,
      target: 99
    }
  );

  // …and the `forward_parent: None` half of the same predicate, which the emission walls make
  // unreachable (only `cst_start_at` writes that field, and its wall demands a tombstone) and
  // which is therefore raw-injectable only. It is a typed refusal rather than the debug
  // `forward_parent` canary the eager encoding carried: the demote does not own that slot.
  let mut sink = verbose_sink("a");
  sink.push_raw_event_for_tests(Event::StartNode {
    kind: K_NODE,
    forward_parent: NonZeroU32::new(1),
  });
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.push_raw_event_for_tests(Event::Demote { target: 0 });
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("a real-kind start carrying a forward_parent is not this demote's own"),
    FinishError::StaleDemote {
      index: 2,
      target: 0
    }
  );
}

/// **The positional half of the wall, and the one demote shape whose residue is balanced.**
///
/// A `Demote` naming a slot *ahead* of itself is the only stale shape that leaves nothing
/// downstream to notice: canonicalization tombstones a start the walk has not opened yet, so
/// the node vanishes, the buffer stays balanced, pops still match pushes, and the tree
/// materializes one node short of what the stream said. Every other stale target leaves a
/// stream that reads wrong somewhere — the surplus finish, the unclosed node — and is caught
/// by a later law.
///
/// The stream below is that shape at its smallest: a demote at slot 0 naming slot 1, the start
/// it names, and a token inside it. Before the positional term existed this **materialized**,
/// returning `<root><tok/></root>` with the `K_NODE` silently erased. It is raw-injectable
/// only — `cst_demote` derives its target from a live slot strictly below the event it appends
/// — which is exactly why the release backstop has to carry the term the emission wall
/// guarantees.
#[test]
fn canonicalization_refuses_a_demote_naming_a_slot_above_itself() {
  fn future_target(src: &str) -> VerboseSink<'_> {
    let mut sink = verbose_sink(src);
    // The demote lands FIRST, naming the start that follows it.
    sink.push_raw_event_for_tests(Event::Demote { target: 1 });
    sink.push_raw_event_for_tests(Event::StartNode {
      kind: K_NODE,
      forward_parent: None,
    });
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    sink
  }

  let (green, _emitter) = future_target("a").finish(K_ROOT);
  assert_eq!(
    green.expect_err("a demote may only name a slot strictly below itself"),
    FinishError::StaleDemote {
      index: 0,
      target: 1
    },
    "without the positional term this materializes a tree with the future start erased, and \
     nothing downstream can tell it apart from a stream that never opened the node"
  );

  let (green, _emitter) = future_target("a").finish_partial(K_ROOT);
  assert_eq!(
    green.expect_err("and the tooling door, which tolerates incompleteness, not corruption"),
    FinishError::StaleDemote {
      index: 0,
      target: 1
    }
  );
}

/// The boundary of the same term: `>=`, not `>`. A demote naming **its own** index is refused
/// through both doors.
///
/// Its outcome does not depend on the positional term — a slot holding a `Demote` is not a
/// `StartNode`, so the content match refuses a self-target anyway — and that is the point of
/// pinning it. The two arms must agree on the boundary, so that a future rewrite of either one
/// cannot open a gap between them.
#[test]
fn canonicalization_refuses_a_demote_naming_its_own_slot() {
  fn self_target(src: &str) -> VerboseSink<'_> {
    let mut sink = verbose_sink(src);
    sink.push_raw_event_for_tests(Event::Demote { target: 0 });
    sink
  }

  let (green, _emitter) = self_target("").finish(K_ROOT);
  assert_eq!(
    green.expect_err("a demote cannot un-open itself"),
    FinishError::StaleDemote {
      index: 0,
      target: 0
    }
  );

  let (green, _emitter) = self_target("").finish_partial(K_ROOT);
  assert_eq!(
    green.expect_err("through the tooling door too"),
    FinishError::StaleDemote {
      index: 0,
      target: 0
    }
  );
}

/// **The interleaved-exit shape, and what release actually does with it.** Two brackets whose
/// closings cross — the enclosing start demoted while the inner one is still open — is the third
/// route that dips `cst_demote`'s debug suffix recount, and the only one of the three that is
/// *not* a misuse. Canonicalization is positional and order-blind, so it tombstones both slots
/// whichever order the two `Demote`s arrive in, and the two buffers materialize the **same
/// tree**.
///
/// That is the fact the debug assert's message stakes, so it is pinned rather than asserted.
/// Both buffers are built raw, because in a debug build the interleaved order is refused at the
/// emit site (`raw_interleaved_bracket_exits_panic_in_debug`, in `tests/parser_node.rs`) and
/// would never reach materialization at all.
#[test]
fn interleaved_and_innermost_first_demotes_materialize_the_same_tree() {
  fn built(src: &str, first: u64, second: u64) -> VerboseSink<'_> {
    let mut sink = verbose_sink(src);
    sink.push_raw_event_for_tests(Event::StartNode {
      kind: K_NODE,
      forward_parent: None,
    });
    sink.push_raw_event_for_tests(Event::StartNode {
      kind: K_LIST,
      forward_parent: None,
    });
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    sink.push_raw_event_for_tests(Event::Demote { target: first });
    sink.push_raw_event_for_tests(Event::Demote { target: second });
    sink.record_token(&MiniTok(b'b'), &span(1, 2));
    sink
  }

  // Interleaved: the ENCLOSING start (slot 0) closes first, while slot 1 is still open.
  let (interleaved, _emitter) = built("ab", 0, 1).finish(K_ROOT);
  // Innermost-first: the order the blessed bracket produces structurally.
  let (innermost_first, _emitter) = built("ab", 1, 0).finish(K_ROOT);

  let interleaved = interleaved.expect("release admits the interleaved order");
  let innermost_first = innermost_first.expect("and the innermost-first order, as always");
  assert_eq!(
    shape(&interleaved),
    "Root[Tok\"a\" Tok\"b\"]",
    "both brackets abandoned: no node at all, and losslessness holds"
  );
  assert_eq!(
    interleaved, innermost_first,
    "the two closing orders are byte-identical after canonicalization — the debug refusal of \
     the interleaved order is a strictness choice on the raw surface, not early detection of a \
     release defect"
  );
}

/// The traded-away affordance, pinned rather than left to be discovered: a **demoted** start
/// mark is not a retro-wrap anchor. The slot keeps its real kind for the whole parse now, so
/// `cst_start_at`'s tombstone wall refuses it exactly as it refuses an un-demoted start mark
/// (`start_at_on_an_up_front_start_mark_panics` below). Recovery tooling that wants to wrap an
/// abandoned region mints its own `cst_mark`.
#[test]
#[should_panic(expected = "came from `cst_start`")]
fn start_at_on_a_demoted_start_mark_panics() {
  let mut sink = verbose_sink("a");
  let mark = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_demote(mark, K_NODE);
  sink.cst_start_at(mark, K_WRAP);
}

/// **The first misuse the slot cannot witness**, caught at cause — in a debug build. Nothing
/// ever rewrites the slot: `cst_finish` is appended *above* it, so afterwards the slot still
/// holds a live `StartNode` of exactly the demoted kind and identity, bounds, era and content
/// all pass. Only a recount of the events above the mark sees the close, and that recount is
/// `cfg(debug_assertions)`-only because the release backstop is a typed refusal rather than a
/// wrong tree — pinned by
/// `the_finished_then_demoted_residue_is_refused_typed_through_both_doors` below.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "already took its SUCCESS exit")]
fn demote_of_a_finished_start_panics_in_debug() {
  let mut sink = verbose_sink("a");
  let mark = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  sink.cst_demote(mark, K_NODE);
}

/// **What a release build does with the residue instead** — and it is what the `cst_demote` docs
/// now stake, so it is pinned here rather than argued.
///
/// The buffer is built *directly*, not by committing the misuse, and that is what lets the cell
/// run in **every** build: in a debug build the misuse is refused at cause by the cell above and
/// never reaches materialization at all. Byte for byte, this is what
/// `cst_start(K_NODE); token; cst_finish(K_NODE); cst_demote(mark, K_NODE)` leaves behind — the
/// real start, its finish, and the appended demote naming it.
///
/// The counting argument the docs make: canonicalization applies the demote, so the start's
/// `depth_delta` drops from 1 to 0 — one frame **push** removed and no **pop** — and pops
/// therefore exceed pushes and the replay walk must underflow. `finish` and `finish_partial`
/// are one walk over that canonicalized buffer — `close_open_nodes` relaxes end-of-stream opens
/// and gap tiling, never balance — so **both** doors refuse, typed. There is no door through
/// which this residue becomes a tree.
#[test]
fn the_finished_then_demoted_residue_is_refused_typed_through_both_doors() {
  fn residue(src: &str) -> VerboseSink<'_> {
    let mut sink = verbose_sink(src);
    sink.push_raw_event_for_tests(Event::StartNode {
      kind: K_NODE,
      forward_parent: None,
    });
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    // The finish the demote left behind. Raw throughout, because the misuse is refused at cause
    // in a debug build — which is the very imbalance this cell is about.
    sink.push_raw_event_for_tests(Event::FinishNode { kind: K_NODE });
    sink.push_raw_event_for_tests(Event::Demote { target: 0 });
    sink
  }

  assert_eq!(
    residue("a").events(),
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
      Event::Demote { target: 0 },
    ],
    "the residue is the post-misuse buffer exactly: the start, its token, the finish that \
     closed it, and the demote that came after"
  );

  let (green, _emitter) = residue("a").finish(K_ROOT);
  assert_eq!(
    green.expect_err("the strict door refuses the surplus finish"),
    FinishError::OrphanFinish { index: 2 }
  );

  let (green, _emitter) = residue("a").finish_partial(K_ROOT);
  assert_eq!(
    green.expect_err("and so does the tooling door — one walk, one balance check"),
    FinishError::OrphanFinish { index: 2 },
    "close_open_nodes relaxes end-of-stream opens, not a pop with nothing to pop"
  );
}

/// **The #98-class guard on the debug scan.** A predicate that fires on a legal history is worse
/// than no predicate at all, and this repo has shipped that mistake once: the old `cst_finish`
/// assert compared depth against a *frozen* baseline and panicked on a legal cross-checkpoint
/// close. The new scan carries no baseline — it recounts the live suffix — and this cell is the
/// busiest legal frame that recount has to admit.
///
/// Every shape here puts a −1 above the mark's slot without closing the marked node: a
/// completed child (+1 then −1, never dipping), an unspent `cst_mark` tombstone (0 outright),
/// a completed retro wrap whose `StartAt` sits above the mark in **emission** order whatever it
/// hoists to (so its +1 still precedes its −1 there), and — the shape the appended encoding
/// adds — a **nested bracket that took its own failing exit**, which is a `Start(+1)` followed
/// by its own `Demote(−1)` and nets zero exactly like the completed child. The outer demote
/// must be silent, and the buffer must still materialize.
#[test]
fn demote_scan_admits_a_busy_legal_frame() {
  let mut sink = verbose_sink("abcd");
  let outer = sink.cst_start(K_NODE);

  // A completed child node.
  sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_LIST);

  // A nested bracket that FAILED: its own Start/Demote pair nets zero above the outer mark.
  let inner = sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_demote(inner, K_LIST);

  // An unspent tombstone, left to materialize into nothing.
  let _unspent = sink.cst_mark();
  sink.record_token(&MiniTok(b'c'), &span(2, 3));

  // A completed retro wrap, hoisted below its own `StartAt` at materialization.
  let anchor = sink.cst_mark();
  sink.record_token(&MiniTok(b'd'), &span(3, 4));
  sink.cst_start_at(anchor, K_WRAP);
  sink.cst_finish(K_WRAP);

  // The demote under test: legal, and the scan must not fire on any of the above.
  sink.cst_demote(outer, K_NODE);

  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("a legal demote over a busy frame still materializes");
  assert_eq!(text(green.clone()), "abcd", "losslessness holds");
  assert_eq!(
    shape(&green),
    "Root[List[Tok\"a\"] Tok\"b\" Tok\"c\" Wrap[Tok\"d\"]]",
    "both demoted nodes vanish; every completed structure above them survives"
  );
}

/// …and the mirror wall, which is what keeps the shared `EventMark` type honest: an up-front
/// start mark is refused by `cst_start_at`. `validate_mark` demands a live **tombstone**, and a
/// real-kind `StartNode` is not one — retro-wrapping an already-open node would nest it inside
/// a wrap of itself.
#[test]
#[should_panic(expected = "came from `cst_start`")]
fn start_at_on_an_up_front_start_mark_panics() {
  let mut sink = verbose_sink("a");
  let mark = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_start_at(mark, K_WRAP);
}

/// **The panic residue, pinned.** A panic that escapes *every* guard is the one exit the
/// up-front bracket does not close — no `catch_unwind` is involved, so there is no mechanism to
/// test, only the residue it leaves: an open node. This cell says what that residue costs, and
/// it costs nothing new — it is the pre-existing fatal-abort shape, already walled two ways.
/// (A guard-mediated unwind never reaches here: the guard's rewind truncates the start away,
/// which `demote_dies_with_a_truncation_at_or_below_it` above holds.)
#[test]
fn a_start_left_open_by_an_escaping_panic_is_refused_by_finish_and_closed_by_finish_partial() {
  let mut sink = verbose_sink("a");
  let _never_closed = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the success door refuses a leftover open node, typed"),
    FinishError::UnclosedNodes { open: 1 }
  );

  let mut sink = verbose_sink("a");
  let _never_closed = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  let green = green.expect("the tooling door closes it per its existing contract");
  assert_eq!(text(green.clone()), "a", "losslessness holds either way");
  let root = tree(green);
  assert_eq!(root.first_child().expect("Root[Node]").kind(), K_NODE);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ONE BRACKET, TWO EXITS, ONE ROLLBACK LAW
// ═══════════════════════════════════════════════════════════════════════════════
//
// These five cells are the adversarial-review evidence that decided the appended encoding,
// kept as pins. The finding they were built to demonstrate — a rollback whose target lands
// BETWEEN a `cst_start` and its failing exit — is what an in-place kind rewrite got wrong: the
// truncation kept the slot and kept the rewrite, so `finish` succeeded with the node silently
// gone where the restore contract promised it open again. The success exit never had that
// problem, because its `FinishNode` is an append the truncation removes.
//
// With both exits appended the two are the same law, and the first two cells below are the
// former defect stated as its correct outcome.

/// **The finding, inverted.** A checkpoint captured between `cst_start` and `cst_demote`
/// promised a buffer whose slot holds a live `StartNode(K_NODE)`. The rollback truncates the
/// `Demote` and delivers exactly that: the node is open again, and a `finish` that follows is
/// answered with the typed refusal the restore contract implies.
///
/// Under the eager in-place rewrite this cell read the other way — the tombstone survived the
/// rollback, the retry re-balanced the buffer, and `finish` returned a tree with the node
/// silently omitted. Nothing downstream could detect that; this is the whole reason the
/// demotion moved onto an event.
#[test]
fn a_rollback_into_the_demote_window_reopens_the_node() {
  let mut sink = verbose_sink("ab");
  let m = sink.cst_start(K_NODE); // slot 0
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // len 1: [StartNode(K_NODE)]
  sink.record_token(&MiniTok(b'a'), &span(0, 1)); // len 2
  sink.cst_demote(m, K_NODE); // len 3 — an APPEND above the checkpoint
  rewind(&mut sink, ckp); // truncate to len 1

  assert_eq!(
    sink.events(),
    &[Event::StartNode {
      kind: K_NODE,
      forward_parent: None
    }],
    "exact: the rollback restored the buffer the capture promised, demote and all"
  );

  // The retry that does not re-close. Under the eager rewrite this buffer was BALANCED and
  // `finish` handed back a tree; now the open node is still open and the refusal is typed.
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the reopened node is correctly reported open"),
    FinishError::UnclosedNodes { open: 1 }
  );
}

/// **The boundary case that killed the journal remedy, dissolved by construction.** A
/// write-time-length journal cannot fix an in-place demote, because the eager demote appended
/// *nothing*: capture-at-L → demote-at-L → rewind-to-L is a lawful rewind-to-current, a total
/// no-op that runs no truncation and no journal replay, so no length-keyed entry can tell a
/// capture taken before the demote from one taken after it. That is also the MOST common
/// failure shape (the inner parser fails before consuming, with a point opened at entry).
///
/// An appended demote ticks the length clock, so the two captures are no longer at the same
/// length and no rewind can straddle the demote invisibly.
#[test]
fn a_demote_ticks_the_length_clock_so_no_rewind_can_straddle_it() {
  let mut sink = verbose_sink("ab");
  let m = sink.cst_start(K_NODE); // slot 0
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // len 1
  sink.cst_demote(m, K_NODE); // len 2 — the clock ticked
  assert_eq!(
    sink.events().len(),
    2,
    "the failing exit is an event, so the capture at len 1 is strictly below it"
  );
  rewind(&mut sink, ckp); // a real truncation, not a rewind-to-current

  assert_eq!(
    sink.events(),
    &[Event::StartNode {
      kind: K_NODE,
      forward_parent: None
    }],
    "the capture promised StartNode(K_NODE) and got it back"
  );
}

/// The **success exit** under the same drive, unchanged — the precedent the failing exit now
/// matches rather than contradicts. The finish is an append, the rollback into the window
/// truncates it, the open node returns, and the resumed window may lawfully re-close (the
/// `#98` cross-checkpoint-close clause).
#[test]
fn the_success_exit_rollback_into_the_window_is_exact_and_reclosable() {
  let mut sink = verbose_sink("ab");
  let _m = sink.cst_start(K_NODE);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE); // success exit: an append
  rewind(&mut sink, ckp); // rollback into the window
  assert_eq!(
    sink.events(),
    &[Event::StartNode {
      kind: K_NODE,
      forward_parent: None
    }],
    "exact: the open node is back"
  );
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_finish(K_NODE); // the reopened frame re-closes — legal per #98
  let (green, _emitter) = sink.finish(K_ROOT);
  let root = tree(green.expect("balanced"));
  assert_eq!(
    root.first_child().expect("Root[Node]").kind(),
    K_NODE,
    "same drive, success exit: the node survives the window rollback"
  );
}

/// …and the failing exit's twin of that re-close, which under the eager rewrite was a **misuse
/// caught by a debug wall**: a continuation that trusted the restore law and re-closed the
/// promised node was closing a node the buffer no longer had open. It is now simply correct —
/// the node really is open again, the finish really does close it, and the tree is the one the
/// restore contract described.
#[test]
fn a_reclose_after_a_rollback_into_the_demote_window_is_legal() {
  let mut sink = verbose_sink("ab");
  let m = sink.cst_start(K_NODE);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_demote(m, K_NODE);
  rewind(&mut sink, ckp);

  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_finish(K_NODE); // trusts the promise — and the promise is now true
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("balanced");
  assert_eq!(text(green.clone()), "ab", "losslessness holds");
  assert_eq!(
    shape(&green),
    "Root[Node[Tok\"a\" Tok\"b\"]]",
    "the resumed window re-closed the node the rollback reopened"
  );
}

/// **E4, healed as a corollary.** A live row between the slot and the exit freezes a depth over
/// the prefix below it, and the eager rewrite falsified that frozen fact in place: the row went
/// on claiming depth 1 over a prefix whose start had become a tombstone, which masked
/// `cst_finish`'s debug underflow wall and cost the at-cause tier.
///
/// With the demotion on an event, a frozen depth stays true because both the `+1` start and the
/// `−1` demote are *events* — nothing below a live row can change under it. The row here
/// survives the rollback, still reads 1, and the finish it governs is correctly admitted.
#[test]
fn a_frozen_row_depth_stays_true_across_a_demote_and_its_rollback() {
  let mut sink = verbose_sink("ab");
  let m = sink.cst_start(K_NODE); // slot 0
  let _outer = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // row(mark 1, depth 1) — stays live
  sink.record_token(&MiniTok(b'a'), &span(0, 1)); // len 2
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // row(mark 2, depth 1)
  sink.record_token(&MiniTok(b'b'), &span(1, 2)); // len 3
  sink.cst_demote(m, K_NODE); // len 4 — appended, ABOVE both rows
  rewind(&mut sink, ckp); // spends row(mark 2), keeps row(mark 1)

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
    ],
    "the demote went with the rollback; the surviving row's prefix is untouched"
  );

  // The surviving row froze depth 1 over `[StartNode(K_NODE)]`, and that is still the truth,
  // so this finish is admitted rather than masked — the debug wall it consults is exact.
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("balanced");
  assert_eq!(text(green.clone()), "ab");
  assert_eq!(shape(&green), "Root[Node[Tok\"a\" Tok\"b\"]]");
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);

  // The speculative wrap: StartAt + finish, with the journaled fp write onto index 1.
  sink.cst_start_at(mark, K_WRAP);
  sink.record_token(&MiniTok(b'c'), &span(2, 3));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.cst_start_at(mark, K_WRAP);
  sink.record_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish(K_WRAP);
  rewind(&mut sink, ckp);

  // The retry: an unrelated List over the next token.
  sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'd'), &span(2, 3));
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
  let mut input = Input::<'_, MiniLexer<'_>, Ctx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      verbose_sink("abcdef"),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();

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
    input.emitter().rows_len(),
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
  let mut input = Input::<'_, MiniLexer<'_>, Ctx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      verbose_sink("abcdef"),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );

  for cycle in 0..3 {
    {
      let mut inp = input.as_ref();
      let _point = inp.begin_point();
      let _ = inp.next().expect("verbose collects").expect("a token");
      // …and the handle dies here with the point still open.
    }
    assert_eq!(
      input.emitter().rows_len(),
      0,
      "cycle {cycle}: an abandoned session point must release its emitter mark row, \
       exactly as it releases its pin and lineage entry"
    );
  }

  // No rollback rode along with the release: every token consumed through the abandoned
  // points is still on the event buffer.
  assert_eq!(
    input
      .emitter()
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
}

/// The leaked-finish MISUSE shape, now a typed error: `cst_start(A); checkpoint m;
/// cst_start(B); rewind(m); <token settles>; cst_finish(B)`. The finish was meant for B, but B's
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
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.cst_start(K_LIST); // the start this finish was meant to close …
  rewind(&mut sink, ckp); //  … rolled back: K_LIST never existed
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
/// <token settles>; cst_finish` (balanced, non-leaked). Under commit/release both events survive
/// balanced; under a rewind of `m` this very finish is what truncates and `A` reopens (the
/// contract's blessed truncate-and-reopen semantics — not exercised here, see the
/// rewind-recovery tests elsewhere in this file). The old assert panicked on it; the
/// narrowed assert must pass in both debug and release and materialize `Root[Node]`.
#[test]
fn cst_finish_across_a_live_checkpoint_is_legal_and_materializes() {
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  let _ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
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
  let first = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.cst_start(K_NODE);
  let second = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
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
  sink.record_token(&MiniTok(b'x'), &span(0, 1));
  // The hole's tokens, with a crossed lexer error between them (a Diag slot).
  sink.record_token(&MiniTok(b'a'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  sink.record_token(&MiniTok(b'b'), &span(3, 4));

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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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

/// E6's one behavioural risk, met head-on: the scan enters at the start of the pure-`Diag`
/// tail, so a wrappable token **under** a diagnostic run must still be found and wrapped
/// (al8n/tokora#305). The ceiling skips exactly the iterations the `Diag` arm would have
/// `continue`d over; it is not a boundary on how far back the wrap may reach.
#[test]
fn hole_wrap_reaches_under_a_diagnostic_run() {
  let mut sink = verbose_sink("ab");
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  // Three diagnostics with no token between them: the buffer now ends in a pure-`Diag` tail,
  // which is the shape whose rescan was quadratic.
  emit_error(&mut sink, 0, 1);
  emit_error(&mut sink, 0, 2);
  emit_error(&mut sink, 0, 3);

  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(0, 1), 1).expect("collects");

  assert!(
    matches!(sink.events()[0], Event::StartNode { kind: K_ERR, .. }),
    "the wrap still opens at the token beneath the Diag run: {:?}",
    sink.events()
  );
  assert!(
    matches!(sink.events()[5], Event::FinishNode { kind: K_ERR }),
    "…and closes above it: {:?}",
    sink.events()
  );
}

/// The wrap survives a later rewind like any other events: a checkpoint below the hole
/// unwinds wrap and tokens together.
#[test]
fn hole_wrap_rewinds_with_the_log() {
  let mut sink = verbose_sink("xab");
  sink.record_token(&MiniTok(b'x'), &span(0, 1));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'a'), &span(1, 2));
  sink.record_token(&MiniTok(b'b'), &span(2, 3));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 3), 2).expect("collects");
  assert_eq!(sink.events().len(), 6);
  rewind(&mut sink, ckp);
  assert_eq!(sink.events().len(), 1, "wrap, tokens, and Diag all unwind");
}

// ═══════════════════════════════════════════════════════════════════════════════
// #177 — the wrap start is FLOORED at the youngest positional fact
// ═══════════════════════════════════════════════════════════════════════════════
//
// `wrap_hole` splices with `insert`, so every index at or above the wrap start shifts by
// one — and checkpoint marks ARE event-buffer indices. A splice below a live mark renames
// it, and the rewind that follows tears the `Start`/`Finish` pair apart. None of these four
// cells is gated on `debug_assertions`, in either direction: the whole point of the floor is
// that it is a structural bound present in every build, so every one of them must hold
// identically in debug and in release. (Before the floor, the first cell PASSED in release
// while asserting a corrupt log and PANICKED in debug — the exact profile divergence #160
// set out to eliminate.)

/// The reported condition, inverted. A caller-chosen span reaches back over a token
/// committed before a live checkpoint; the wrap start is floored at the mark, so the rewind
/// restores the pre-checkpoint log **exactly** instead of leaving an orphan error start
/// where a committed token used to be.
#[test]
fn hole_wrap_floors_at_the_youngest_live_row() {
  let mut sink = verbose_sink("a j");
  sink.record_token(&MiniTok(b'a'), &span(0, 1)); // idx 0 — committed
  sink.record_token(&MiniTok(b' '), &span(1, 2)); // idx 1 — committed trivia
  let before = sink.events().to_vec();
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // mark 2
  sink.record_token(&MiniTok(b'j'), &span(2, 3)); // idx 2 — the junk this transaction skipped

  // The span widens BACKWARD over the already-committed trivia at idx 1 — the shape an
  // IDE-facing recoverer produces when it reports "everything from the whitespace onward".
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 3), 1).expect("collects");

  assert_eq!(
    sink.events(),
    &[
      Event::Token {
        kind: K_TOK,
        span: span(0, 1)
      },
      Event::Token {
        kind: K_TOK,
        span: span(1, 2)
      },
      Event::StartNode {
        kind: K_ERR,
        forward_parent: None
      },
      Event::Token {
        kind: K_TOK,
        span: span(2, 3)
      },
      Event::FinishNode { kind: K_ERR },
      Event::Diag { error_span: None },
    ],
    "the wrap starts AT the mark, not below it: the committed trivia stays outside"
  );

  rewind(&mut sink, ckp);
  assert_eq!(
    sink.events(),
    &before[..],
    "rewind is exact — before the floor this left `[T_a, StartNode(K_ERR)]`, a committed \
     token replaced by an unbalanced error start"
  );
}

/// Suffix discipline is untouched: when the span covers only tokens this transaction
/// settled, the floor is not the binding constraint and the whole hole is wrapped. This is
/// the shape every in-crate `sync_balanced` recovery produces.
#[test]
fn hole_wrap_covers_a_post_checkpoint_hole_whole() {
  let mut sink = verbose_sink("xyab");
  sink.record_token(&MiniTok(b'x'), &span(0, 1)); // idx 0
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // mark 1 — the floor
  sink.record_token(&MiniTok(b'y'), &span(1, 2)); // idx 1 — settled, outside the hole
  sink.record_token(&MiniTok(b'a'), &span(2, 3)); // idx 2 — the hole
  sink.record_token(&MiniTok(b'b'), &span(3, 4)); // idx 3 — the hole

  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(2, 4), 2).expect("collects");

  assert_eq!(
    sink.events(),
    &[
      Event::Token {
        kind: K_TOK,
        span: span(0, 1)
      },
      Event::Token {
        kind: K_TOK,
        span: span(1, 2)
      },
      Event::StartNode {
        kind: K_ERR,
        forward_parent: None
      },
      Event::Token {
        kind: K_TOK,
        span: span(2, 3)
      },
      Event::Token {
        kind: K_TOK,
        span: span(3, 4)
      },
      Event::FinishNode { kind: K_ERR },
      Event::Diag { error_span: None },
    ],
    "wrap start 2 sits strictly above the floor 1 — the span's own reach binds, not the floor"
  );
  let _ = ckp;
}

/// No capture at all: the floor is 0 and the wrap may start at the very first event, which
/// is what an unguarded top-level recovery has always done.
#[test]
fn hole_wrap_with_no_rows_floors_at_zero() {
  let mut sink = verbose_sink("ab");
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.record_token(&MiniTok(b'b'), &span(1, 2));

  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(0, 2), 2).expect("collects");

  assert_eq!(
    sink.events(),
    &[
      Event::StartNode {
        kind: K_ERR,
        forward_parent: None
      },
      Event::Token {
        kind: K_TOK,
        span: span(0, 1)
      },
      Event::Token {
        kind: K_TOK,
        span: span(1, 2)
      },
      Event::FinishNode { kind: K_ERR },
      Event::Diag { error_span: None },
    ],
    "an empty mark stack floors at 0 — the whole buffer is wrappable"
  );
}

/// The positive twin of al8n/tokora#257, on the geometry the audit probe attacked: **the
/// observation door observes.** Every operation a parser can reach through the `&Sink` that
/// [`InputRef::emitter_ref`](crate::InputRef::emitter_ref) hands it — the emitter trait's one
/// shared query, the sink's own shared readers, and `Debug` — runs between the two tokens, and
/// the hole still wraps both of them because the mark stack is still empty.
///
/// The probe's one extra call was `Emitter::checkpoint` through the same reference. It pushed a
/// row at mark 1, which floored the wrap there: the error node covered `"b"` alone while the
/// diagnostic said `"ab"`. That call no longer compiles — the receiver is `&mut self` — so what
/// remains to pin is that nothing which *does* compile through this door moves a row.
#[test]
fn permitted_inspection_through_a_shared_sink_reference_moves_no_row() {
  let mut sink = verbose_sink("ab");
  sink.record_token(&MiniTok(b'a'), &span(0, 1));

  {
    // The reference the handle actually hands out, and everything reachable from it.
    let observed: &VerboseSink<'_> = &sink;
    let _ = Emitter::<MiniLexer<'_>>::bound_source(observed);
    let _ = observed.inner_ref().errors().len();
    let _ = observed.error_kind();
    let _ = observed.gap_kind();
    let _ = observed.trivia_policy();
    let _ = std::format!("{observed:?}");
    assert_eq!(observed.rows_len(), 0, "an observation captures nothing");
  }

  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(0, 2), 2).expect("collects");

  assert_eq!(sink.rows_len(), 0, "and still nothing to settle");
  assert_eq!(
    sink.events(),
    &[
      Event::StartNode {
        kind: K_ERR,
        forward_parent: None
      },
      Event::Token {
        kind: K_TOK,
        span: span(0, 1)
      },
      Event::Token {
        kind: K_TOK,
        span: span(1, 2)
      },
      Event::FinishNode { kind: K_ERR },
      Event::Diag { error_span: None },
    ],
    "the recovery node covers both tokens — the diagnostic's span and the wrap agree"
  );
}

/// The boundary itself: the wrap starts at exactly the mark index. A row with `mark == at`
/// names the prefix `0..at`, which an insert *at* `at` leaves byte-for-byte alone — so the
/// rewind is still exact at the tightest legal splice point.
#[test]
fn hole_wrap_at_the_floor_boundary_rewinds_exactly() {
  let mut sink = verbose_sink("ab");
  sink.record_token(&MiniTok(b'a'), &span(0, 1)); // idx 0
  let before = sink.events().to_vec();
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // mark 1 == the eventual wrap start
  sink.record_token(&MiniTok(b'b'), &span(1, 2)); // idx 1

  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 2), 1).expect("collects");

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
      Event::FinishNode { kind: K_ERR },
      Event::Diag { error_span: None },
    ],
    "at == floor == 1: the splice lands exactly on the mark"
  );

  rewind(&mut sink, ckp);
  assert_eq!(
    sink.events(),
    &before[..],
    "rewind to a mark the splice sat on is still exact"
  );
}

/// The floor's second term. `release` promotes a committed row to `Sink::floor`, and that
/// memo can sit **above** the innermost live row — so a floor read off `rows.last()` alone
/// would still let the splice cross a frozen `(mark, depth)` prefix. Nothing tears, but the
/// depth memo goes stale: `derived_depth` keeps summing from a prefix that no longer holds
/// the events it was frozen over, and `cst_finish`'s global-underflow check then fires on a
/// buffer whose node genuinely is open.
#[test]
fn hole_wrap_floors_at_the_released_floor_too() {
  let mut sink = verbose_sink("abcde");
  sink.cst_start(K_NODE); // idx 0 — depth +1, still open at the end
  sink.record_token(&MiniTok(b'a'), &span(0, 1)); // idx 1
  let outer = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // mark 2 — stays live
  let before = sink.events().to_vec();
  sink.record_token(&MiniTok(b'b'), &span(1, 2)); // idx 2
  sink.record_token(&MiniTok(b'c'), &span(2, 3)); // idx 3
  sink.record_token(&MiniTok(b'd'), &span(3, 4)); // idx 4
  let inner = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // mark 5
  Emitter::<MiniLexer<'_>>::release(&mut sink, inner); // committed → Sink::floor = mark 5
  sink.record_token(&MiniTok(b'e'), &span(4, 5)); // idx 5

  // Reaches down to idx 3: at or above `rows.last()` (mark 2), below the released floor (5).
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(2, 5), 3).expect("collects");

  assert_eq!(
    sink.events().len(),
    9,
    "start, four tokens, the wrap's start, the junk token, the wrap's finish, the Diag"
  );
  assert_eq!(
    sink.events()[5],
    Event::StartNode {
      kind: K_ERR,
      forward_parent: None
    },
    "the wrap starts at the RELEASED floor (5), not at the live row (2)"
  );

  // Would panic with "global underflow" on a staled memo — K_NODE is open, depth is 1.
  sink.cst_finish(K_NODE);

  rewind(&mut sink, outer);
  assert_eq!(
    sink.events(),
    &before[..],
    "the surviving live row rewinds exactly"
  );
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
    Emitter::<MiniLexer<'_>>::commit_token(&mut threaded, &tok, &sp);
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn cst_forward_census_one_helper_carries_every_channel() {
  let src = include_str!("../sink.rs");

  // The forwarded channels: 6 core Emitter (the four diagnostic `emit_*`, `emit_skipped_region`,
  // and `commit_lexer_error` — the input layer's own refusal, which is the *evidence* twin of
  // `emit_lexer_error`) + TooFew/TooMany/FullContainer + SeparatedEmitter (2) + the 4
  // leading/trailing refinements + PrattEmitter (2) + UnclosedEmitter (1 — the delimiter door,
  // once absent from the sink and therefore absent from this count too; the two omissions hid
  // each other).
  let calls = count(src, "self.forward_diag::<");
  assert!(
    calls == 18,
    "CST_FORWARD_CENSUS drift: {calls} forward_diag call sites, expected 18. A new \
     forwarded channel must route through the one helper AND bump this census in the \
     same commit (grep CST_FORWARD_CENSUS)."
  );
  assert!(
    count(src, "fn forward_diag") == 1,
    "CST_FORWARD_CENSUS drift: the helper must be defined exactly once"
  );

  // COVERAGE_EVIDENCE_CENSUS — the gap-licensing door is exactly ONE.
  //
  // `finish`'s coverage verdict reads exactly two things out of the event log: `Token` spans and
  // `Diag { error_span: Some(_) }` spans. The token half already has a one-door census above
  // (`self.inner.commit_token` == 1, and `Event::Token` is built only in `record_token`). This is
  // the other half: a `Some(..)` argument to the forward helper is the ONLY way a span becomes
  // gap-licensing evidence, and it must sit in `commit_lexer_error` — the surface the input layer
  // reaches and no caller can. Every other forwarded diagnostic passes `None`.
  //
  // A second `Some(` site would re-open the finding this census was written for: a caller-chosen
  // span, with nothing consumed for it, excusing an uncovered byte of the sink's own buffer.
  let evidence = count(src, "self.forward_diag::<Lang, _>(Some(");
  assert!(
    evidence == 1,
    "COVERAGE_EVIDENCE_CENSUS drift: {evidence} sites record a gap-licensing span, expected \
     exactly 1 (Emitter::commit_lexer_error). A span becomes coverage evidence only where the \
     INPUT LAYER refused the bytes it names; a caller-chosen span licenses nothing."
  );
  let body = src
    .split_once("fn commit_lexer_error")
    .expect("the sink overrides the evidence door")
    .1;
  assert!(
    body
      .split_once("\n  }")
      .is_some_and(|(head, _)| head.contains("self.forward_diag::<Lang, _>(Some(")),
    "COVERAGE_EVIDENCE_CENSUS drift: the one gap-licensing site must live in \
     `commit_lexer_error`"
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
     Emitter::commit_token, the token channel's one and only door"
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
    count(src, "self.inner.checkpoint()") == 1 && count(src, "checkpoint(&mut self.inner)") == 1,
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

/// CST_COMPOSITION_CENSUS — the tripwire for a whole class: every method of the emitter trait
/// family must be OVERRIDDEN by `Sink`, never left to a trait default. A defaulted inherit
/// silently severs that channel for wrapped inners (exactly how the `commit_token` gap
/// happened). Two halves: (a) every one of the 27 inventory names appears as an impl in
/// sink.rs; (b) drift tripwires on the trait definitions, so any NEW family method forces a
/// classification (override + forward, or a documented inherit) in the same commit.
///
/// No per-method gap exists beyond the ones already known, and it stays as the permanent
/// tripwire.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source, by `include_str!` and by `std::fs`: forced to run under miri \
            the walk aborts with 'unsupported operation: `opendir` not available when \
            isolation is enabled', and there is no UB surface here to be worth interpreting \
            every byte of what remains"
)]
fn cst_composition_census_every_family_method_is_overridden() {
  let src = include_str!("../sink.rs");

  // (a) The 30-method inventory: 13 core Emitter + 5 CstEmitter + 12 capability emit_*.
  // Each must appear as an `fn <name>` impl in the sink; a missing one is a severed channel.
  let overridden = [
    // 13 core Emitter
    "emit_lexer_error",
    // The evidence twin of the line above, and the sink is exactly the emitter that has to tell
    // them apart: this one records the gap-licensing span, `emit_lexer_error` records none.
    // Inheriting the default here would collapse the two doors back into one.
    "commit_lexer_error",
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
    // The sink is the one emitter in the crate that binds a source, so it is the one that
    // must answer this rather than inherit the `None` default.
    "bound_source",
    // 5 CstEmitter
    "cst_start",
    "cst_finish",
    // The up-front bracket's failing exit. It is the one CstEmitter method whose default is
    // not merely "record nothing" but "un-record something", so a defaulted inherit here would
    // leave a wrapped inner carrying a dangling open node for every `Err` a `node()` sees.
    "cst_demote",
    "cst_mark",
    "cst_start_at",
    // 12 capability emit_*
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
    // `UnclosedEmitter` — the delimiter door. It was ABSENT from this list and absent from the
    // sink, and the two absences hid each other: a hard-coded inventory cannot report a
    // capability nobody wrote down. Its cost was not cosmetic — `UnclosedEmitter` is a member
    // of `ComposableEmitter`, so a `Sink` could not satisfy the one-bound collecting surface at
    // all.
    "emit_unclosed",
  ];
  assert_eq!(
    overridden.len(),
    30,
    "the family inventory is 13 core + 5 CstEmitter + 12 capability = 30"
  );

  // DERIVED, and at the TYPE level rather than by string matching — the half a hand-written
  // inventory cannot do for itself. A list can only omit; it cannot notice an omission, which
  // is exactly how `UnclosedEmitter` went missing from BOTH the sink and this census at once.
  //
  // The first attempt swept `emitter/mod.rs` for `pub use` names and matched **nothing** — the
  // capability traits live across twelve files and are re-exported by glob. Its own positive
  // control caught that, which is why a text census was abandoned here: "does `Sink` serve this
  // capability" is a type question and answers itself at compile time.
  //
  // Scope, stated: the *family* question — does `Sink` serve every trait the public bundles
  // name — is answered by `sink_satisfies_the_public_emitter_bundles`, bound on the bundles
  // themselves. What this block adds is a second error type: `UnclosedEmitter`'s conversion
  // sits on the *method* rather than the trait, so a deliberately-thin error type must still be
  // able to satisfy it. The other capability traits require conversions `SesErr` does not
  // carry, so asserting them here would test the fixture rather than the sink.
  {
    const fn serves_unclosed<'a, L, E>()
    where
      L: Lexer<'a>,
      E: crate::emitter::UnclosedEmitter<'a, L>,
    {
    }
    serves_unclosed::<MiniLexer<'_>, SesSink<'_>>();
  }

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
    13,
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
  // The capability surface, DERIVED on both sides — no constant, because a hard-coded total is
  // the shape that let `UnclosedEmitter` through. Its file was missing from this list *and* its
  // method was missing from the inventory above, so the count still agreed with itself while
  // `Sink` failed to serve the trait at all. A number cannot notice its own omission.
  //
  // What this half covers, exactly: a **method added to a trait**. A new *defaulted* channel on
  // any capability trait keeps every bundle bound satisfiable — the default supplies it — so
  // `sink_satisfies_the_public_emitter_bundles` stays green while `Sink` silently inherits a
  // non-forwarding body. Only reading the trait sources catches that. The two mechanisms are
  // near-disjoint, not redundant: the bundle bounds catch a *trait* joining a bundle, which no
  // amount of string matching can see, and this catches a *method* joining a trait, which no
  // bound can see. The file list below is policed by its own completeness check, so neither
  // half rests on a list that is free to forget a member.
  let capability_sources = [
    ("delimited.rs", include_str!("../../emitter/delimited.rs")),
    ("pratt.rs", include_str!("../../emitter/pratt.rs")),
    (
      "repeated/too_few.rs",
      include_str!("../../emitter/repeated/too_few.rs"),
    ),
    (
      "repeated/too_many.rs",
      include_str!("../../emitter/repeated/too_many.rs"),
    ),
    (
      "repeated/full_container.rs",
      include_str!("../../emitter/repeated/full_container.rs"),
    ),
    (
      "separated/mod.rs",
      include_str!("../../emitter/separated/mod.rs"),
    ),
    (
      "separated/missing_leading.rs",
      include_str!("../../emitter/separated/missing_leading.rs"),
    ),
    (
      "separated/missing_trailing.rs",
      include_str!("../../emitter/separated/missing_trailing.rs"),
    ),
    (
      "separated/unexpected_leading.rs",
      include_str!("../../emitter/separated/unexpected_leading.rs"),
    ),
    (
      "separated/unexpected_trailing.rs",
      include_str!("../../emitter/separated/unexpected_trailing.rs"),
    ),
  ];

  // Is that list COMPLETE? It is hand-written, which is the same shape as the hand-written
  // method inventory that hid `UnclosedEmitter`, so it does not get to be trusted on its own
  // word. Every **trait** declared under `src/emitter` whose name ends in `Emitter` must live in
  // a file the list names, or be exempted **at its path, with the mechanism that covers it
  // instead**.
  //
  // Keyed on (path, trait) rather than on files, or on bare trait names, on purpose. The unit of
  // hazard is one trait at one path, so that is the unit of exemption:
  //
  //   - exempting `mod.rs` wholesale — the first spelling — excused a *third* trait added to
  //     `mod.rs` beside the core one;
  //   - exempting the bare name `Emitter` — the second spelling — excused a trait *named*
  //     `Emitter` declared anywhere at all, including an unlisted new file. An exemption that
  //     does not name where its subject lives is a wildcard wearing a specific name.
  //
  // Each exemption is also asserted **observed exactly once at its stated path**, so an
  // exemption whose subject moved, was renamed, or was deleted fails rather than quietly going
  // on excusing something that is no longer there.
  //
  // LIMITS, stated rather than implied — this is a text scanner and it is not going to become
  // anything else. It sees a declaration whose line begins, after any leading whitespace, with
  // `pub trait ` followed by the name; nesting depth no longer matters, but *form* does. It
  // misses a trait emitted by a macro, and one whose name sits on a line below its `pub trait `
  // (legal, though rustfmt does not produce it).
  //
  // A trait declared in a form this does not match is caught by the bundle bounds in
  // `sink_satisfies_the_public_emitter_bundles` if it joins a bundle, and by nothing if it does
  // not. Escaping therefore needs a conjunction: a new `*Emitter`, spelled in one of those
  // forms, joining no bundle, unimplemented by `Sink`, and unnoticed at the call site. That
  // residue is narrower than the surface it guards, and it is where this rail stops — past this
  // point the instrument costs more than the thing it measures.
  //
  // The reverse direction needs no check: a file that is listed and then deleted or renamed
  // fails `include_str!` at compile time.
  {
    /// Traits deliberately outside `capability_sources`, as `(path, trait, what covers it
    /// instead)`. A new entry here is a claim that has to be true, at that path.
    const EXEMPT: &[(&str, &str, &str)] = &[
      (
        "mod.rs",
        "Emitter",
        "the core trait: its own `  fn ` count tripwire above forces every new method to be \
         classified here",
      ),
      (
        "cst.rs",
        "CstEmitter",
        "its own `  fn ` count tripwire above, and one of the two explicit extras in \
         `sink_satisfies_the_public_emitter_bundles`",
      ),
      (
        "mod.rs",
        "ComposableEmitter",
        "a bundle, not a member — it declares no methods of its own, and `Sink`'s conformance \
         to it is asserted at the type level by `sink_satisfies_the_public_emitter_bundles`",
      ),
      (
        "mod.rs",
        "PolicyComposableEmitter",
        "likewise a bundle, asserted at the type level by \
         `sink_satisfies_the_public_emitter_bundles`",
      ),
      (
        "mod.rs",
        "ValueKeyedEmitter",
        "a method-less marker trait — it carries no channel that could be severed by a \
         non-forwarding default",
      ),
    ];

    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/emitter"));

    fn rs_files(dir: &std::path::Path, out: &mut std::vec::Vec<std::path::PathBuf>) {
      for entry in std::fs::read_dir(dir).expect("src/emitter is readable from the crate root") {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
          rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
          out.push(path);
        }
      }
    }

    /// Names of the traits a source declares whose name ends in `Emitter` — the capability
    /// shape. Deliberately loose in the safe direction: over-matching forces a trait to be
    /// listed or exempted, and can never excuse one.
    ///
    /// `trim_start` is load-bearing: matching a line that begins *exactly* `pub trait ` misses
    /// every declaration nested inside an indented `mod` block. This crate has now been bitten
    /// by that same "the pattern assumed column zero" shape three times.
    fn declared_emitter_traits(src: &str) -> std::vec::Vec<&str> {
      src
        .lines()
        .filter_map(|line| {
          let name = line
            .trim_start()
            .strip_prefix("pub trait ")?
            .split(['<', ' ', ':'])
            .next()?;
          name.ends_with("Emitter").then_some(name)
        })
        .collect()
    }

    let mut files = std::vec::Vec::new();
    rs_files(root, &mut files);

    let mut declared_traits = std::vec::Vec::new();
    for path in &files {
      let src = std::fs::read_to_string(path).expect("an emitter source is readable");
      let rel = path
        .strip_prefix(root)
        .expect("every walked path sits under src/emitter")
        .to_string_lossy()
        .replace('\\', "/");
      for name in declared_emitter_traits(&src) {
        declared_traits.push((rel.clone(), std::string::String::from(name)));
      }
    }

    // Liveness floor, and ONLY that. A floor cannot distinguish "the walk missed a trait" from
    // "there is no such trait", because the traits it does find already satisfy it — so it is
    // not asked to. It catches the wholesale case: a walk that found nothing, or a predicate
    // that matched nothing, would assert nothing below and report that as coverage.
    //
    // The detectors are the two exact assertions that follow: every exemption observed exactly
    // once at its stated path, and every discovered trait listed or exempted.
    assert!(
      declared_traits.len() >= 15,
      "the emitter-trait sweep found only {} trait(s) across {} scanned file(s) — it is \
       measuring a broken walk or a broken predicate, not a complete surface",
      declared_traits.len(),
      files.len()
    );

    // An exemption is a claim about a specific trait at a specific path. Check the claim, both
    // ways: exactly once means it is still there, and that nothing else answers to it.
    for (file, name, mechanism) in EXEMPT {
      let seen = declared_traits
        .iter()
        .filter(|(rel, trait_name)| rel == file && trait_name == name)
        .count();
      assert_eq!(
        seen, 1,
        "CST_COMPOSITION_CENSUS: the exemption for `{name}` at `src/emitter/{file}` was \
         observed {seen} time(s), not once. It is excused on the grounds that {mechanism} — if \
         the trait moved, was renamed, or was deleted, that claim no longer describes anything \
         and the exemption must move or go with it"
      );
    }

    for (rel, name) in &declared_traits {
      let listed = capability_sources.iter().any(|(file, _)| file == rel);
      let exempt = EXEMPT
        .iter()
        .any(|(file, trait_name, _)| file == rel && trait_name == name);
      assert!(
        listed || exempt,
        "CST_COMPOSITION_CENSUS: `{name}` (src/emitter/{rel}) is an emitter trait that is \
         neither declared in a file `capability_sources` names nor exempted at this path. Add \
         its file to the list so its methods are censused, or add `(\"{rel}\", \"{name}\", …)` \
         to EXEMPT with the mechanism that covers it instead — a trait outside every bundle is \
         invisible to the type-level half, which is exactly how `UnclosedEmitter` was missed"
      );
    }
  }

  /// Every `emit_*` method name declared in a source, in order, deduplicated by the caller.
  fn emit_method_names(src: &str) -> std::vec::Vec<&str> {
    src
      .match_indices("fn emit_")
      .map(|(i, _)| {
        let rest = &src[i + "fn ".len()..];
        let end = rest
          .find(|c: char| !c.is_alphanumeric() && c != '_')
          .unwrap_or(rest.len());
        &rest[..end]
      })
      .collect()
  }

  let mut declared: std::vec::Vec<&str> = capability_sources
    .iter()
    .flat_map(|(_, src)| emit_method_names(src))
    .collect();
  declared.sort_unstable();
  declared.dedup();

  // Positive control: a sweep that matched nothing would assert nothing, which is how a green
  // census can mean "the pattern is broken" instead of "the surface is covered".
  assert!(
    declared.len() >= 12,
    "the capability `emit_*` sweep found only {} method(s) — it is measuring a broken pattern, \
     not an absent channel",
    declared.len()
  );

  for method in &declared {
    assert!(
      count(src, &std::format!("fn {method}")) >= 1,
      "CST_COMPOSITION_CENSUS: a capability trait declares `{method}` and `Sink` does not \
       override it — the channel silently inherits a non-forwarding default, which is a \
       recorded diagnostic that never reaches the inner emitter"
    );
    assert!(
      overridden.contains(method),
      "CST_COMPOSITION_CENSUS: `{method}` is declared by a capability trait but missing from \
       the inventory above — add it there too, or the inventory drifts out from under the \
       surface it claims to enumerate"
    );
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// The forwarding matrix — Sink satisfies every bound its inner emitter does
// ═══════════════════════════════════════════════════════════════════════════════

/// Conformance stated against the **public bundles themselves**, not against a copy of their
/// membership: `Sink<E>` — and `&mut Sink<E>`, the `parse_partial` threading shape — satisfies
/// [`ComposableEmitter`](crate::emitter::ComposableEmitter) and
/// [`PolicyComposableEmitter`](crate::emitter::PolicyComposableEmitter) wherever `E` does.
///
/// The previous spelling named eleven traits by hand, and it had already gone stale:
/// `UnclosedEmitter` was missing from it, missing from the composition census below, and
/// missing from `Sink`. Three hand-written lists agreed with one another about a capability
/// none of them carried, which is how a `Sink` that could not satisfy `ComposableEmitter` at
/// all passed a test named for the full family. A bundle cannot drift from itself: a trait
/// added to either bundle fails **here, at compile time, naming the unsatisfied trait**, on the
/// day it joins — no census to update, no count to keep.
///
/// `CstEmitter` and `PrattEmitter` are spelled out because they sit outside both bundles by
/// design (see [`ComposableEmitter`](crate::emitter::ComposableEmitter)'s own docs — pratt's
/// bound is not part of the collecting surface, and the CST channel is orthogonal to all of
/// it). They are the residue a bundle bound cannot derive, so they are enumerated with a
/// reason rather than counted.
// `PrattEmitter` is one of the two residue bounds this cell enumerates, and it is the `pratt`
// family's.
#[cfg(feature = "pratt")]
#[test]
fn sink_satisfies_the_public_emitter_bundles() {
  use crate::emitter::{ComposableEmitter, PolicyComposableEmitter, PrattEmitter};

  /// The one-bound collecting surface. `PolicyComposableEmitter` has it as a supertrait, so
  /// this bound is implied by the next one — it is kept separate so a break reports which
  /// tier broke rather than only the wider one.
  fn composable<'inp, T>(_: &T)
  where
    T: ComposableEmitter<'inp, MiniLexer<'inp>>,
  {
  }

  /// The collecting surface plus the three emitters a count or separator policy needs.
  fn policy_composable<'inp, T>(_: &T)
  where
    T: PolicyComposableEmitter<'inp, MiniLexer<'inp>>,
  {
  }

  /// Outside both bundles by design, and therefore the only part of this that must be
  /// written down.
  fn outside_the_bundles<'inp, T>(_: &T)
  where
    T: CstEmitter<'inp, MiniLexer<'inp>> + PrattEmitter<'inp, MiniLexer<'inp>>,
  {
  }

  let mut sink = verbose_sink("");
  composable(&sink);
  policy_composable(&sink);
  outside_the_bundles(&sink);
  composable(&&mut sink);
  policy_composable(&&mut sink);
  outside_the_bundles(&&mut sink);
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

/// The fixture kinds by name, for the shape renderer below.
fn kind_name(kind: u16) -> &'static str {
  match kind {
    K_ROOT => "Root",
    K_NODE => "Node",
    K_LIST => "List",
    K_WRAP => "Wrap",
    K_TOK => "Tok",
    K_ERR => "Err",
    K_GAP => "Gap",
    other => unreachable!("kind {other} is outside the fixture dialect"),
  }
}

/// One line of tree *shape*, tokens included with their text: `Root[Node[Tok"a" Gap"!"]]`.
///
/// Placement cells assert on this rather than on a list of top-level kinds, because a flat
/// list hides the one thing they exist to pin — which node a gap ended up **inside**. It also
/// makes a failure readable: the diff is the tree, not an index into a vector.
fn shape(green: &rowan::GreenNode) -> std::string::String {
  fn render(node: &rowan::SyntaxNode<RawLang>, out: &mut std::string::String) {
    out.push_str(kind_name(node.kind()));
    out.push('[');
    for (position, child) in node.children_with_tokens().enumerate() {
      if position > 0 {
        out.push(' ');
      }
      match child {
        rowan::NodeOrToken::Node(inner) => render(&inner, out),
        rowan::NodeOrToken::Token(token) => {
          out.push_str(kind_name(token.kind()));
          out.push('"');
          out.push_str(token.text());
          out.push('"');
        }
      }
    }
    out.push(']');
  }

  let mut out = std::string::String::new();
  render(&tree(green.clone()), &mut out);
  out
}

use crate::cst::FinishError;

#[test]
fn finish_builds_the_straight_tree() {
  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("a balanced stream materializes");
  let root = tree(green.clone());
  assert_eq!(root.kind(), K_ROOT);
  let node = root.first_child().expect("Root[Node]");
  assert_eq!(node.kind(), K_NODE);
  assert_eq!(text(green), "a");
}

/// **The two lexer-error doors differ by exactly one thing, and it is the coverage span.**
///
/// `Emitter::commit_lexer_error` is the input layer's — the layer lexed those bytes and refused
/// them, so the span is evidence and rides the `Diag` slot. `Emitter::emit_lexer_error` is
/// everyone else's (a parser through `InputRef`, a callback through `EmitterView`, a wrapper
/// through either): the caller chose that span with nothing consumed for it, so the slot carries
/// `None`. Both forward the diagnostic; both occupy a log position; only one licenses a gap.
///
/// This is the event-level statement of the finding closed in this round. Its behavioural halves
/// are `a_handle_raised_lexer_error_reports_without_licensing_the_gap` and
/// `an_orphan_view_wrapper_carries_a_foreign_lexer_error_but_licenses_no_gap` in
/// `tests/parser_node.rs`, driven from outside the crate; the structural one is
/// COVERAGE_EVIDENCE_CENSUS above.
#[test]
fn only_the_input_layers_lexer_error_carries_a_coverage_span() {
  use crate::cst::event::Event;

  let mut sink = verbose_sink("ab");
  Emitter::<MiniLexer<'_>>::emit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");

  let spans: std::vec::Vec<Option<(usize, usize)>> = sink
    .events()
    .iter()
    .map(|event| match event {
      Event::Diag { error_span } => error_span.as_ref().map(|s| (*s.start_ref(), *s.end_ref())),
      other => panic!("only Diag slots here, got {other:?}"),
    })
    .collect();
  assert_eq!(
    spans,
    std::vec![None, Some((1usize, 2usize))],
    "the caller's report records no coverage span; the layer's records its own"
  );

  let (green, emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("byte 0 is explained by nothing"),
    FinishError::UncoveredGap { start: 0, end: 1 },
    "the caller-raised report over `a` licenses nothing, while the layer's over `b` does"
  );
  assert_eq!(emitter.errors().len(), 2, "both reports reached the log");
}

/// THE round-trip law, structural: an input with a lexer error (its bytes covered by no
/// committed token, since a skipped error settles nothing) still satisfies
/// `tree.text() == source` — the uncovered bytes tile as `gap_kind` tokens.
#[test]
fn round_trip_with_a_lexer_error_is_structural() {
  let mut sink = verbose_sink("a!c");
  // Source "a!c": the `!` is a lexer error — a diagnostic, never a token event.
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.record_token(&MiniTok(b'c'), &span(2, 3));
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

/// **The gap-placement law.** A gap is tiled where it *opens* — immediately after the token it
/// trails, in the node open at that moment — so the *same* uncovered region tiles into the
/// *same* node whether or not more input happens to follow it.
///
/// The two halves below are one stream and its truncation: identical events, identical refused
/// byte, identical enclosing node — the second one simply has nothing after the gap. Placement
/// used to split here, the tail becoming a child of the synthetic **root** while the mid-stream
/// run stayed inside `K_NODE`, so a consumer reading the dialect's own document node saw
/// garbage in one case and not the other for a reason that is not about the garbage.
///
/// This cell is the readable statement of the law; [`appending_a_token_never_moves_a_gap`] is
/// the law itself, checked over a corpus with the second half **generated** rather than
/// written. Three rounds of this rule shipped a hand-written "same stream, one token longer"
/// that was not one, so the mechanical check is the part that has teeth and this one is the
/// part that explains what it means.
///
/// Teeth (measured): tiling a run at the token that *reveals* it instead of the token it trails
/// — the pre-rule walk (M1) — turns the trailing half into `Root[Node[Tok"a"] Gap"!"]` and reds
/// this cell, while the mid-stream half stays green. That is the asymmetry, stated as a diff.
#[test]
fn a_trailing_gap_joins_the_node_of_the_token_it_trails() {
  // Mid-stream: the `!` at [1,2) opens the instant `a` settles, so it tiles into the node open
  // then — `K_NODE`.
  let mut sink = verbose_sink("a!b");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.record_token(&MiniTok(b'b'), &span(2, 3));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let mid = green.expect("the covering diagnostic licenses the gap");
  assert_eq!(shape(&mid), r#"Root[Node[Tok"a" Gap"!" Tok"b"]]"#);
  assert_eq!(text(mid), "a!b", "losslessness, mid-stream");

  // Trailing: the identical stream with the following token deleted. Same refused byte, same
  // enclosing node — and now nothing follows it.
  let mut sink = verbose_sink("a!");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let tail = green.expect("the covering diagnostic licenses the trailing gap too");
  assert_eq!(
    shape(&tail),
    r#"Root[Node[Tok"a" Gap"!"]]"#,
    "the tail is in `K_NODE` because `a` was: placement must not be decided by whether more \
     input followed"
  );
  assert_eq!(text(tail), "a!", "losslessness, trailing");
}

/// **Where the diagnostic sits decides nothing**, and that is a hard requirement rather than a
/// nicety. This cell moves one `emit_lexer_error` across a `cst_finish` and across an unwrapped
/// checkpoint, and the tree does not move.
///
/// Why it is hard: a prefilled lookahead cache **hoists** a lexer-class diagnostic earlier in
/// the event stream. `input_ref`'s cache-transparency matrix says so in as many words — "a peek
/// EMITS the lexer errors it crosses, when it crosses them; prefetching therefore moves such a
/// diagnostic earlier in the timeline", and its `Emission::is_lexer_class` exists precisely to
/// state that a prefill can hoist *only* that class. The token event stream is exactly invariant
/// under prefill; the diagnostic stream is not. So the two streams below are **one parse under
/// two peek schedules**, not two parses, and a rule that reads a diagnostic's position would
/// make the materialized tree a function of how far the caller happened to look ahead.
///
/// Two earlier shapes of the trailing rule did read it, and both are why this cell exists. The
/// first asked whether a close was the last event *to drive the builder*: a `Diag` drives
/// nothing, so it was skipped and the close still counted as last. The second held the close and
/// released it at **any** following event, `Diag` included — which fixed the skip and made the
/// tree depend on the hoist instead, splitting exactly the pair below into two shapes. Placement
/// now reads tokens and structure only, so neither hole is expressible.
///
/// [`hoisting_a_lexer_error_never_moves_a_gap`] checks this over a corpus, by moving every
/// diagnostic to every slot; this cell is the one worked example.
///
/// Teeth (measured): M4 — round 2's mechanism restored, a root child's close held and released
/// at the next event of any kind — reds this cell with `Root[Node[Tok"a"] Gap"!"]`, and it is
/// the only mutation that reds it **together with**
/// [`hoisting_a_lexer_error_never_moves_a_gap`]. M3 — round 1's mechanism, which skipped `Diag`
/// — leaves both green, which is exactly the shape of the hole this cell now guards: round 1
/// was peek-stable by accident, and round 2 gave that away buying an append-invariance it did
/// not get either.
#[test]
fn moving_the_lexer_error_across_the_close_does_not_move_the_gap() {
  // The parse: open `K_NODE`, settle `a`, refuse `!`, close. The refusal is recorded INSIDE the
  // node.
  let mut sink = verbose_sink("a!");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let inside = green.expect("the covering diagnostic licenses the gap");
  assert_eq!(shape(&inside), r#"Root[Node[Tok"a" Gap"!"]]"#);

  // The same parse whose caller peeked one token further before closing: the identical
  // diagnostic is emitted one slot later, AFTER the close. Nothing else differs.
  let mut sink = verbose_sink("a!");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  let (green, _emitter) = sink.finish(K_ROOT);
  let hoisted = green.expect("the covering diagnostic licenses the gap");
  assert_eq!(
    shape(&hoisted),
    r#"Root[Node[Tok"a" Gap"!"]]"#,
    "the run trails `a`, and `a` is in `K_NODE`: how far the caller peeked before closing the \
     node cannot be visible in the tree"
  );
  assert_eq!(
    inside, hoisted,
    "one parse under two peek schedules is one green tree, node for node"
  );

  // A second event kind that a rule reading event *positions* would also trip over: an
  // unwrapped checkpoint, structurally inert at materialization. Inserting one after the close
  // must be as invisible as hoisting the diagnostic was.
  let mut sink = verbose_sink("a!");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_finish(K_NODE);
  let _unwrapped = sink.cst_mark();
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect("explained trailing gap"),
    inside,
    "an abandoned checkpoint is not a fact about where the refused bytes belong"
  );
  assert_eq!(text(inside), "a!", "losslessness under every schedule");

  // And the mirror image: a run the parse refused at ROOT level, after `K_NODE` closed and
  // after a root-level token settled. It trails a root-level token, so it stays at the root —
  // whether or not another token follows. Placement is about which token a run trails, not
  // about depth for its own sake.
  let mut sink = verbose_sink("ab!c");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  sink.record_token(&MiniTok(b'c'), &span(3, 4));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained gap")),
    r#"Root[Node[Tok"a"] Tok"b" Gap"!" Tok"c"]"#,
    "mid-stream: the run trails the root-level `b`, so it is a root child"
  );

  let mut sink = verbose_sink("ab!");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained trailing gap")),
    r#"Root[Node[Tok"a"] Tok"b" Gap"!"]"#,
    "and trailing: deleting the token that follows cannot pull it into a node it never trailed"
  );
}

/// **Depth is read off the token, not chosen.** A run goes at whatever depth the token it
/// trails was committed at — `K_LIST` here, two levels down — and the *number* of frames that
/// close between the run and the end of the stream has nothing to do with it.
///
/// This replaces a rule that said "one level, into the root's last child". That phrasing was an
/// artifact of the mechanism it came from: a walk that withheld exactly one close could not
/// express any other depth, so "one level" looked like a law when it was a limitation. Both
/// halves below now agree at the deeper level, which is the point — the mid-stream half is not
/// a different answer to be reconciled, it is the same answer.
///
/// The trailing half deliberately ends with the **two closes adjacent**, so a rule that tried to
/// pick a frame at the end of the walk would have both on offer and would have to choose. This
/// one never looks: by the time the first close is read the run is already in the tree.
///
/// Teeth (measured): tiling a run at the token that reveals it instead of at the token it
/// trails (M1) reds the mid-stream half first, with `Root[Node[List[Tok"a"] Gap"!" Tok"b"]]` —
/// the run one level up, in the node that revealed it. M2, M3 and M4 red it the same way, which
/// is the point: every rule that decides depth at or after the close gets this stream wrong.
#[test]
fn a_trailing_gap_lands_at_the_depth_of_the_token_it_trails() {
  let nested = |sink: &mut VerboseSink<'_>| {
    sink.cst_start(K_NODE);
    sink.cst_start(K_LIST);
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::commit_lexer_error(sink, Spanned::new(span(1, 2), MiniErr))
      .expect("verbose collects");
    sink.cst_finish(K_LIST);
  };

  // Mid-stream: `a` settled inside `K_LIST`, so the run it trails is inside `K_LIST` — even
  // though the token that reveals the run settles one level up, in `K_NODE`.
  let mut sink = verbose_sink("a!b");
  nested(&mut sink);
  sink.record_token(&MiniTok(b'b'), &span(2, 3));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained gap")),
    r#"Root[Node[List[Tok"a" Gap"!"] Tok"b"]]"#,
    "the run opened while `K_LIST` was still open, and it was tiled then"
  );

  // Trailing: the same depth, with two closes adjacent so an end-of-walk rule would have a
  // choice to make.
  let mut sink = verbose_sink("a!");
  nested(&mut sink);
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained trailing gap")),
    r#"Root[Node[List[Tok"a" Gap"!"]]]"#,
    "deleting the following token changes nothing: the run was placed when `a` settled"
  );

  // The same trailing parse with the byte refused between the two closes rather than before
  // them — a hoist of the diagnostic by one slot. Same answer, because the diagnostic's slot is
  // not consulted; `a` is still the token the run trails.
  let mut sink = verbose_sink("a!");
  sink.cst_start(K_NODE);
  sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_LIST);
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained trailing gap")),
    r#"Root[Node[List[Tok"a" Gap"!"]]]"#,
    "still inside `K_LIST`, with the refusal recorded after `K_LIST` closed"
  );

  // Two children of the root: the run trails `b`, which settled in `K_LIST`, so `K_LIST` gets
  // it — and `K_NODE`, which closed long before, does not.
  let mut sink = verbose_sink("ab!");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  sink.cst_finish(K_LIST);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained trailing gap")),
    r#"Root[Node[Tok"a"] List[Tok"b" Gap"!"]]"#,
    "the run goes with the token before it, not with the last child for being last"
  );
}

/// The two shapes in which a trailing run stays a **root** child, and they are not exceptions:
/// in one the token it trails settled at root level, in the other there is no token at all.
///
/// Stated as cells because a reader who has just learnt "the tail joins the last child's
/// content" will expect the tail to descend here, and silence is how a rule acquires an
/// undocumented third case.
///
/// Teeth (measured): holding a root child's close and never releasing it (M2 — "the tail joins
/// the last child" read as the rule rather than as a consequence) makes the first half
/// `Root[Node[Tok"a" Tok"b" Gap"!"]]`, swallowing a **root-level token** into the node before
/// it. The same mutation reds `hole_wrap_materializes_as_an_error_node_with_real_tokens` on
/// `tree.text()`, so a withheld close is not even a placement-only change.
#[test]
fn a_trailing_gap_stays_at_the_root_when_the_token_it_trails_did() {
  // The run trails a root-level settle, made after `K_NODE` closed.
  let mut sink = verbose_sink("ab!");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_finish(K_NODE);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
    .expect("verbose collects");
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("explained trailing gap");
  assert_eq!(
    shape(&green),
    r#"Root[Node[Tok"a"] Tok"b" Gap"!"]"#,
    "the run trails `b`, and `b` is a root child: it stays where `b` is"
  );
  assert_eq!(text(green), "ab!");

  // The root has NO children: nothing lexable, and no structure over it either.
  let mut sink = verbose_sink("!");
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained trailing gap")),
    r#"Root[Gap"!"]"#,
    "no token to trail and no node open: the root keeps it"
  );
}

/// **A LEADING run — one no token trails — is the single case placed where it is discovered**,
/// and it is unchanged from before this rule.
///
/// There is no "the token before it" to attach to, so it tiles at the first committed token, in
/// whatever node that token lands in. That is not a loophole in the rule: appending events
/// cannot change which token is first, so the placement is as fixed as any other, and
/// [`appending_a_token_never_moves_a_gap`] covers this shape in its corpus.
///
/// The second half is the same clause with **no** committed token anywhere, which is what a
/// wholly unlexable source produces —
/// [`a_source_with_nothing_lexable_keeps_its_gap_at_the_root_either_way`] is that case in full.
///
/// Teeth (measured): the over-consistent reading of the fallback — tile a leading run at the
/// very start of the walk, in the root, because that is where it "opens" (M5) — yields
/// `Root[Gap"!" Node[Tok"b"]]`, ejecting leading trivia from the node the parse opened over it
/// and leaving that node starting at offset 1. This cell and
/// [`finish_partial_trailing_gap_tiles_into_the_innermost_open_node`] are what rule that out.
#[test]
fn a_leading_gap_tiles_at_the_first_token_that_follows_it() {
  let mut sink = verbose_sink("!b");
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("explained gap");
  assert_eq!(
    shape(&green),
    r#"Root[Node[Gap"!" Tok"b"]]"#,
    "no token precedes the run, so it tiles where the walk first sees it — inside `K_NODE`, \
     with the token that revealed it"
  );
  assert_eq!(text(green), "!b");

  // The refusal recorded after `K_NODE` opened instead of before it: the same tree, because a
  // diagnostic's slot is never read.
  let mut sink = verbose_sink("!b");
  sink.cst_start(K_NODE);
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained gap")),
    r#"Root[Node[Gap"!" Tok"b"]]"#
  );
}

/// **A source with nothing lexable in it keeps its gap at the root** — the fallback clause, on
/// the input that makes it sharpest.
///
/// This is what a lossless grammar produces for an unterminated string, an unterminated block
/// string, a lone stray punctuator: the root production opens its document node before it can
/// know whether a token follows, every byte is explained by a lexer error, and the parse
/// finishes with **zero committed tokens**. So there is no token for the run to trail, the
/// fallback clause applies, and the tree is `Root[Node@0..0[], Gap@0..len]`.
///
/// **This is the one shape an earlier round of the rule moved and this one moves back**, so the
/// reasoning is recorded rather than left as a diff. That round put the run inside the node,
/// making it `Root[Node@0..len[Gap]]`, and argued the document node "was opened over those
/// bytes". The argument is appealing and it is wrong for a measurable reason: the identical
/// parse with one lexable byte appended tiles that same run **at the root** — there is still no
/// token before it — so descending here would have made the shape depend on whether a lexable
/// byte followed, which is the exact asymmetry this branch exists to delete. It was not a gain
/// with an unlucky edge; it was an instance of the defect, and
/// [`appending_a_token_never_moves_a_gap`] measures it as one.
///
/// The second half is the twin that decides it. One committed token is enough for the node to
/// take the run — because now there *is* a token to trail — and the node widens over it. So the
/// two halves differ, and they differ for a reason that is about the parse: in one the document
/// matched something and the garbage follows it, in the other the document matched nothing at
/// all and the garbage is beside it. Nothing about either answer changes when input is appended.
///
/// Teeth (measured): the most sensitive cell in the battery — all five mutations red it, with
/// **three** distinct wrong answers. M2/M3/M4 put the run inside the node
/// (`Root[Node[Gap"ab"]]`, the shape this reverts); M5 puts it before the node
/// (`Root[Gap"ab" Node[]]`); M1 leaves the first two blocks alone and reds the third instead
/// (`Root[Node[Tok"a"] Gap"b"]`), which is the pre-rule shape.
#[test]
fn a_source_with_nothing_lexable_keeps_its_gap_at_the_root_either_way() {
  let mut sink = verbose_sink("ab");
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_start(K_NODE);
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("the lexer refused every byte and said so");
  assert_eq!(
    shape(&green),
    r#"Root[Node[] Gap"ab"]"#,
    "no committed token anywhere, so the run trails nothing and tiles where the walk ends"
  );
  assert_eq!(
    tree(green.clone())
      .first_child()
      .expect("Root[Node[..]]")
      .text_range(),
    rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(0)),
    "the node measures what it matched, which is nothing"
  );
  assert_eq!(text(green), "ab");

  // The twin that forces the answer above: the identical parse with one lexable byte appended
  // at the end of the source. The run is the same `[0,2)`, still preceded by no token, and it
  // is still a root child — so the first half must place it at the root too, or placement would
  // once again turn on whether a lexable byte followed.
  let mut sink = verbose_sink("abc");
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_start(K_NODE);
  sink.cst_finish(K_NODE);
  sink.record_token(&MiniTok(b'c'), &span(2, 3));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    shape(&green.expect("explained gap")),
    r#"Root[Node[] Gap"ab" Tok"c"]"#,
    "one lexable byte after the garbage, and the garbage is still a root child: this is the \
     twin that makes the first half's answer the only symmetric one"
  );

  // And the twin that shows the fallback is narrow: one committed token inside the node is
  // enough for the run to trail it, and the node widens over the run.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(1, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("explained gap");
  assert_eq!(
    shape(&green),
    r#"Root[Node[Tok"a" Gap"b"]]"#,
    "there is a token to trail now, so the run is inside the node with it"
  );
  assert_eq!(
    tree(green)
      .first_child()
      .expect("Root[Node[..]]")
      .text_range(),
    rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(2)),
    "the node widens over the run it took: `0..1` becomes `0..2`"
  );
}

// ── The two placement laws, checked over a corpus rather than in prose ──────────
//
// Three successive shapes of the gap-placement rule shipped with a table of hand-written
// "the same stream, one token longer" pairs, and each time the pair that escaped was one
// nobody had written down. The pairs below are **generated** from the corpus instead: the
// second member of every append pair is the first with one event pushed onto it, and the
// peek variants are the same event vector with one entry moved. That is the difference
// between a table that documents a rule and a check that constrains it.

/// One buffered event, as data, so a stream can be transformed rather than retyped.
#[derive(Clone, Copy, Debug)]
enum PlacementOp {
  Start(u16),
  Finish(u16),
  Tok(u8, usize, usize),
  Diag(usize, usize),
  Mark,
}

/// Replays a `PlacementOp` script through the real sink doors.
fn placement_drive(
  src: &str,
  ops: &[PlacementOp],
  partial: bool,
) -> Result<rowan::GreenNode, FinishError> {
  let mut sink = verbose_sink(src);
  for op in ops {
    match *op {
      PlacementOp::Start(kind) => {
        let _up_front = sink.cst_start(kind);
      }
      PlacementOp::Finish(kind) => sink.cst_finish(kind),
      PlacementOp::Tok(byte, lo, hi) => sink.record_token(&MiniTok(byte), &span(lo, hi)),
      PlacementOp::Diag(lo, hi) => {
        Emitter::<MiniLexer<'_>>::commit_lexer_error(
          &mut sink,
          Spanned::new(span(lo, hi), MiniErr),
        )
        .expect("verbose collects");
      }
      PlacementOp::Mark => {
        let _abandoned = sink.cst_mark();
      }
    }
  }
  if partial {
    sink.finish_partial(K_ROOT).0
  } else {
    sink.finish(K_ROOT).0
  }
}

/// Every `Gap` token's position in the tree, as `Root>Node>List:"!"`. Appending a root-level
/// token at the very end cannot change any of these strings — it adds a *later* sibling and
/// re-parents nothing — so an append pair that disagrees here has moved a gap.
fn gap_ancestry(green: &rowan::GreenNode) -> std::vec::Vec<std::string::String> {
  fn walk(
    node: &rowan::SyntaxNode<RawLang>,
    path: &mut std::vec::Vec<&'static str>,
    out: &mut std::vec::Vec<std::string::String>,
  ) {
    path.push(kind_name(node.kind()));
    for child in node.children_with_tokens() {
      match child {
        rowan::NodeOrToken::Node(inner) => walk(&inner, path, out),
        rowan::NodeOrToken::Token(token) => {
          if token.kind() == K_GAP {
            out.push(std::format!("{}:{:?}", path.join(">"), token.text()));
          }
        }
      }
    }
    path.pop();
  }
  let mut out = std::vec::Vec::new();
  walk(&tree(green.clone()), &mut std::vec::Vec::new(), &mut out);
  out
}

/// The corpus both laws run over: `(label, source, script)`. Every shape the placement cells
/// above state in prose is here, plus the orderings none of them thought to write — a
/// diagnostic on either side of a close, an abandoned checkpoint after one, adjacent closes,
/// a leading run, a run with no committed token anywhere.
fn placement_corpus() -> std::vec::Vec<(&'static str, &'static str, std::vec::Vec<PlacementOp>)> {
  use PlacementOp::{Diag, Finish, Mark, Start, Tok};
  std::vec![
    (
      "trailing, refusal inside the node",
      "a!",
      std::vec![Start(K_NODE), Tok(b'a', 0, 1), Diag(1, 2), Finish(K_NODE)]
    ),
    (
      "trailing, refusal after the close",
      "a!",
      std::vec![Start(K_NODE), Tok(b'a', 0, 1), Finish(K_NODE), Diag(1, 2)]
    ),
    (
      "trailing, abandoned checkpoint after the close",
      "a!",
      std::vec![
        Start(K_NODE),
        Tok(b'a', 0, 1),
        Diag(1, 2),
        Finish(K_NODE),
        Mark
      ]
    ),
    (
      "mid-stream, token settles inside the node",
      "a!b",
      std::vec![
        Start(K_NODE),
        Tok(b'a', 0, 1),
        Diag(1, 2),
        Tok(b'b', 2, 3),
        Finish(K_NODE)
      ]
    ),
    (
      "mid-stream, token settles at the root",
      "a!b",
      std::vec![
        Start(K_NODE),
        Tok(b'a', 0, 1),
        Finish(K_NODE),
        Diag(1, 2),
        Tok(b'b', 2, 3)
      ]
    ),
    (
      "trailing, nested, refusal before the inner close",
      "a!",
      std::vec![
        Start(K_NODE),
        Start(K_LIST),
        Tok(b'a', 0, 1),
        Diag(1, 2),
        Finish(K_LIST),
        Finish(K_NODE)
      ]
    ),
    (
      "trailing, nested, refusal between the two closes",
      "a!",
      std::vec![
        Start(K_NODE),
        Start(K_LIST),
        Tok(b'a', 0, 1),
        Finish(K_LIST),
        Diag(1, 2),
        Finish(K_NODE)
      ]
    ),
    (
      "trailing, two root children, refusal inside the last",
      "ab!",
      std::vec![
        Start(K_NODE),
        Tok(b'a', 0, 1),
        Finish(K_NODE),
        Start(K_LIST),
        Tok(b'b', 1, 2),
        Diag(2, 3),
        Finish(K_LIST)
      ]
    ),
    (
      "trailing, two root children, refusal after the last close",
      "ab!",
      std::vec![
        Start(K_NODE),
        Tok(b'a', 0, 1),
        Finish(K_NODE),
        Start(K_LIST),
        Tok(b'b', 1, 2),
        Finish(K_LIST),
        Diag(2, 3)
      ]
    ),
    (
      "trailing, the root's last child is a token",
      "ab!",
      std::vec![
        Start(K_NODE),
        Tok(b'a', 0, 1),
        Finish(K_NODE),
        Tok(b'b', 1, 2),
        Diag(2, 3)
      ]
    ),
    (
      "trailing, the root has no children",
      "!",
      std::vec![Diag(0, 1)]
    ),
    (
      "no committed token, refusal before the node opens",
      "ab",
      std::vec![Diag(0, 2), Start(K_NODE), Finish(K_NODE)]
    ),
    (
      "no committed token, refusal inside the node",
      "ab",
      std::vec![Start(K_NODE), Diag(0, 2), Finish(K_NODE)]
    ),
    (
      "leading run before the first token",
      "!b",
      std::vec![Diag(0, 1), Start(K_NODE), Tok(b'b', 1, 2), Finish(K_NODE)]
    ),
    (
      "leading and trailing runs around one token",
      "!b?",
      std::vec![
        Start(K_NODE),
        Diag(0, 1),
        Tok(b'b', 1, 2),
        Diag(2, 3),
        Finish(K_NODE)
      ]
    ),
    (
      "a frame that opens and closes over no token at all",
      "a!",
      std::vec![
        Start(K_NODE),
        Tok(b'a', 0, 1),
        Diag(1, 2),
        Finish(K_NODE),
        Start(K_LIST),
        Finish(K_LIST)
      ]
    ),
    (
      "an unbalanced stream, for the tolerant door",
      "ab",
      std::vec![Start(K_NODE), Tok(b'a', 0, 1)]
    ),
  ]
}

/// **Law 1 — nothing that follows a run can move it.** For every stream in the corpus, appending
/// one root-level token that starts exactly where the source ended must leave every gap in the
/// node it was already in.
///
/// This is the law the branch is named for, and the check three rounds of it did not have. Each
/// round wrote its own "mid-stream twin" by hand and each hand-written twin was a *different
/// parse* rather than the same one continued, so the twin agreed while the append did not. Here
/// the twin is `ops.push(Tok(len, len + 1))` and cannot be anything else.
///
/// Only the `Ok`/`Ok` pairs are compared: appending a token can legitimately change whether the
/// **zero-token wall** fires, and that is a coverage verdict, not a placement one.
///
/// Teeth (measured): **both** shipped mechanisms red this cell — round 1's (M3, hold the close
/// unless the next event drives the builder) and round 2's (M4, hold it until the next event of
/// any kind), each on `trailing, refusal inside the node` first. The pre-rule walk (M1) and the
/// never-release reading (M2) leave it green, and that is the division of labour: this cell says
/// a rule must be *consistent*, and the shape cells above say **which** consistent rule.
#[test]
fn appending_a_token_never_moves_a_gap() {
  for (label, src, ops) in placement_corpus() {
    for partial in [false, true] {
      let short = placement_drive(src, &ops, partial);
      let long_src = std::format!("{src}x");
      let mut long_ops = ops.clone();
      long_ops.push(PlacementOp::Tok(b'x', src.len(), src.len() + 1));
      let long = placement_drive(&long_src, &long_ops, partial);
      if let (Ok(short), Ok(long)) = (&short, &long) {
        assert_eq!(
          gap_ancestry(short),
          gap_ancestry(long),
          "{label} (partial={partial}): appending a token moved a gap\n  \
           without it: {}\n  with it:    {}",
          shape(short),
          shape(long)
        );
        assert_eq!(text(short.clone()), src, "{label}: losslessness, short");
        assert_eq!(text(long.clone()), long_src, "{label}: losslessness, long");
      }
    }
  }
}

/// **Law 2 — a hoisting diagnostic cannot move a run either.** For every stream in the corpus,
/// moving each `emit_lexer_error` to every other slot must produce the byte-identical green
/// tree.
///
/// This is not a hygiene property, it is a correctness one. A prefilled lookahead cache emits
/// the lexer errors it crosses *when it crosses them*, so prefetching moves such a diagnostic
/// earlier in the event stream — `input_ref`'s cache-transparency matrix states exactly that,
/// and its `Emission::is_lexer_class` exists to say that a prefill can hoist only that class.
/// The token event stream is invariant under prefill; the diagnostic stream is not. So the
/// variants below are one parse under different peek schedules, and if the tree moved, the CST
/// would be a function of how far the caller happened to look ahead.
///
/// Teeth (measured): round 2's mechanism — hold a root child's close, release it at the next
/// event whatever it is (M4) — reds this cell, first on `trailing, refusal inside the node`
/// with the diagnostic moved from slot 2 to slot 3: the `Diag` released the hold, so which side
/// of the close it fell on decided the tree. It is the **only** mutation in the battery that
/// reds this cell. Round 1's (M3) leaves it green, having skipped `Diag` for an unrelated
/// reason, and law 1 above is what catches that one instead.
#[test]
fn hoisting_a_lexer_error_never_moves_a_gap() {
  for (label, src, ops) in placement_corpus() {
    for partial in [false, true] {
      let Ok(baseline) = placement_drive(src, &ops, partial) else {
        continue;
      };
      for from in 0..ops.len() {
        if !matches!(ops[from], PlacementOp::Diag(..)) {
          continue;
        }
        for to in 0..ops.len() {
          if to == from {
            continue;
          }
          let mut moved = ops.clone();
          let diag = moved.remove(from);
          moved.insert(to, diag);
          let other = placement_drive(src, &moved, partial).unwrap_or_else(|err| {
            std::panic!("{label}: hoisting a diagnostic broke the parse: {err:?}")
          });
          assert_eq!(
            baseline,
            other,
            "{label} (partial={partial}): moving the diagnostic from slot {from} to {to} \
             changed the tree\n  before: {}\n  after:  {}",
            shape(&baseline),
            shape(&other)
          );
        }
      }
    }
  }
}

/// Both doors agree on a **balanced** stream: `finish_partial` is the tolerant door for
/// *incompleteness*, and a balanced stream is not incomplete, so it must not get a different
/// tree. Pinned because the tail's placement is now decided in the walk rather than after it,
/// and the walk is shared.
#[test]
fn finish_partial_places_a_balanced_trailing_gap_exactly_as_finish_does() {
  let balanced = |sink: &mut VerboseSink<'_>| {
    sink.cst_start(K_NODE);
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::commit_lexer_error(sink, Spanned::new(span(1, 2), MiniErr))
      .expect("verbose collects");
    sink.cst_finish(K_NODE);
  };

  let mut sink = verbose_sink("a!");
  balanced(&mut sink);
  let (strict, _emitter) = sink.finish(K_ROOT);

  let mut sink = verbose_sink("a!");
  balanced(&mut sink);
  let (tolerant, _emitter) = sink.finish_partial(K_ROOT);

  let strict = strict.expect("balanced and explained");
  let tolerant = tolerant.expect("balanced and explained");
  assert_eq!(shape(&strict), r#"Root[Node[Tok"a" Gap"!"]]"#);
  assert_eq!(
    strict, tolerant,
    "the tooling door relaxes incompleteness, not placement: a balanced stream has one tree"
  );
}

/// Where a trailing gap lands when `finish_partial` is called with a node still open. Here the
/// run *does* trail a token — `a`, inside the open `K_NODE` — so the ordinary clause already
/// answers it, and the answer is the one this door promised before the rule changed.
///
/// The end-of-walk tiling that the doc calls the innermost-open-node case is the **fallback**
/// clause, and it is reachable through this door alone: a run no token precedes, tiled before
/// the loop that closes the open frames, so rowan appends it to whatever node is still open.
/// `finish` never sees that shape — an unbalanced stream is refused outright — and the second
/// half below is it.
///
/// Teeth (measured): tiling a leading run at the start of the walk instead (M5) yields
/// `Root[Gap"ab" Node[List[]]]`, putting the run outside both open frames.
#[test]
fn finish_partial_trailing_gap_tiles_into_the_innermost_open_node() {
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  // `K_NODE` is deliberately never finished, and `b` at [1,2) is an uncovered trailing gap.
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  let root = tree(green.expect("finish_partial tolerates the open node and tiles the tail"));

  // The root's only child is the open node — the gap did NOT tile beside it.
  let top: std::vec::Vec<u16> = root.children_with_tokens().map(|el| el.kind()).collect();
  assert_eq!(
    top,
    std::vec![K_NODE],
    "the trailing gap must not be a child of the root while a node is open",
  );

  // It tiled inside the innermost open node, after that node's own token.
  let inner = root
    .first_child()
    .expect("the open node survives as a node");
  let kinds: std::vec::Vec<u16> = inner.children_with_tokens().map(|el| el.kind()).collect();
  assert_eq!(
    kinds,
    std::vec![K_TOK, K_GAP],
    "the trailing gap tiles into the innermost OPEN node, not the root",
  );

  // Losslessness holds either way — which is why placement needed its own pin.
  assert_eq!(root.text().to_string(), "ab");

  // The fallback clause, which only this door can reach: no committed token at all, two frames
  // open. The run trails nothing, so it tiles at the end of the walk — and the walk ends with
  // `K_LIST` open, so `K_LIST` gets it rather than the root.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_NODE);
  sink.cst_start(K_LIST);
  let (green, _emitter) = sink.finish_partial(K_ROOT);
  assert_eq!(
    shape(&green.expect("the tooling door tiles the un-diagnosed run")),
    r#"Root[Node[List[Gap"ab"]]]"#,
    "the innermost OPEN node takes a run no token precedes"
  );
}

/// The gap-coverage law at the mechanism level (the partial-drop signature the zero-token
/// wall cannot see): tokens `a` and `c` survive over `"abc"` but the `b` at `[1, 2)` was
/// dropped and no lexer error covers it. `finish` refuses the unexplained gap with the exact
/// dropped span; `finish_partial` — the tooling door — tiles it instead. A gap a lexer error
/// *does* cover stays legal under `finish` (the round-trip oracle above is that green case).
#[test]
fn uncovered_gap_refused_by_finish_tiled_by_partial() {
  let dropped_b = |sink: &mut VerboseSink<'_>| {
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    sink.record_token(&MiniTok(b'c'), &span(2, 3)); // the `b` at [1,2) never settled
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
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(2, 3), MiniErr))
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

/// The wall's **lossless** boundary: a token-free parse whose every source byte a recorded
/// lexer error explains is an honest parse of unlexable bytes, not a severed token channel.
///
/// This is the shape a lossless grammar produces for a source with nothing lexable in it —
/// `"unterminated` in an editor buffer — because its root production opens the document node
/// before it can know whether any token follows. The wall used to refuse it, which made a
/// half-typed string a materialization failure and, one `expect` upstream, a crash.
///
/// The two halves are the whole of the distinction the wall now draws, over the *same*
/// zero-token structure and the *same* nonempty source: with the covering error the tree is
/// honest and tiles, without it the bytes are unexplained and the wall fires. Deleting the
/// `first_uncovered_gap` test reds the first half; deleting the wall reds the second.
#[test]
fn structure_without_tokens_is_legal_where_a_lexer_error_explains_every_byte() {
  // Explained: one error over the whole source, one open-then-closed node, no tokens.
  let mut sink = verbose_sink("ab");
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 2), MiniErr))
    .expect("verbose collects");
  sink.cst_start(K_NODE);
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("the lexer refused every byte and said so — an honest tree");
  assert_eq!(text(green.clone()), "ab", "every byte tiles as a gap");
  assert_eq!(
    shape(&green),
    r#"Root[Node[] Gap"ab"]"#,
    "the node the grammar opened must survive — beside the tile, since a run that no committed \
     token precedes has nothing to trail and stays at the root"
  );

  // Unexplained: the identical stream minus the diagnostic is the severed channel, and the
  // wall still names it precisely rather than deferring to the gap-coverage law.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_NODE);
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("nothing explains these bytes"),
    FinishError::StructureWithoutTokens
  );

  // Partially explained is still the severed channel: one byte with no covering error is
  // enough, and the wall — not `UncoveredGap` — is what reports it.
  let mut sink = verbose_sink("ab");
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(0, 1), MiniErr))
    .expect("verbose collects");
  sink.cst_start(K_NODE);
  sink.cst_finish(K_NODE);
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("byte 1 is unexplained"),
    FinishError::StructureWithoutTokens
  );
}

/// F-A6/F-A1 at the wall: an orphan finish is a typed error — rowan's silent absorption
/// of one level of imbalance under the root wrapper is unreachable, because the sink's
/// own stack refuses before the builder sees the pop.
#[test]
fn orphan_finish_is_a_typed_error_not_an_absorbed_close() {
  let mut sink = verbose_sink("a");
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("open nodes refuse the total finish"),
    FinishError::UnclosedNodes { open: 2 }
  );

  let mut sink = verbose_sink("a");
  sink.cst_start(K_NODE);
  sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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

/// F-A7 at cause: the emission-time assert catches a mapper that leaks the reserved band,
/// at the commit that used it.
///
/// Deliberately NOT `cfg(debug_assertions)`-gated: `record_token`'s admission check is a
/// plain `assert!`, unconditional in every build (unlike `cst_finish`'s neighbouring
/// global-underflow `debug_assert!`), so this panic must reproduce under
/// `cargo test --release` too.
#[test]
#[should_panic(expected = "reserved tombstone kind")]
fn tombstone_mapper_asserts_at_emission_in_every_build() {
  fn bad_map(_: &MiniTok) -> u16 {
    TOMBSTONE
  }
  let profile = CstProfile::new(bad_map, KindValidator::new(in_kind_space), K_ERR, K_GAP);
  let mut sink: VerboseSink<'_> = Sink::new("a", Verbose::new(), profile);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
}

/// Overlapping and non-monotone token spans are refused — the no-duplication half of the
/// round-trip law (a double emission cannot silently duplicate text).
#[test]
fn overlapping_spans_are_refused() {
  let mut sink = verbose_sink("abc");
  sink.record_token(&MiniTok(b'a'), &span(0, 2));
  sink.record_token(&MiniTok(b'b'), &span(1, 3));
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
  sink.record_token(&MiniTok(b'a'), &span(0, u32::MAX as usize + 10));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 5));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'1'), &span(0, 1));
  sink.record_token(&MiniTok(b'+'), &span(1, 2));
  sink.record_token(&MiniTok(b'2'), &span(2, 3));
  sink.cst_start_at(mark, K_WRAP); // fold 1: Bin[1,+,2]
  sink.cst_finish(K_WRAP);
  sink.record_token(&MiniTok(b'+'), &span(3, 4));
  sink.record_token(&MiniTok(b'3'), &span(4, 5));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.record_token(&MiniTok(b':'), &span(1, 2));
  let completed = marker.complete(&mut sink, K_WRAP); // Alias[a, :]
  let outer = completed.precede();
  sink.record_token(&MiniTok(b'b'), &span(2, 3));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  let mark = sink.cst_mark();
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.cst_start_at(mark, K_WRAP);
  sink.record_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish(K_WRAP);
  rewind(&mut sink, ckp);

  // The retry consumes a different shape.
  sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'd'), &span(2, 3));
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
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    sink.record_token(&MiniTok(b'b'), &span(1, 2));
    sink.cst_finish(K_NODE);
  };

  let mut straight = verbose_sink("ab");
  drive(&mut straight);
  let (straight_green, _emitter) = straight.finish(K_ROOT);

  let mut backtracked = verbose_sink("ab");
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut backtracked);
  backtracked.cst_start(K_LIST);
  backtracked.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'{'), &span(0, 1));
  // The scan settles two garbage tokens, then the hole is reported.
  sink.record_token(&MiniTok(b'x'), &span(1, 2));
  sink.record_token(&MiniTok(b'y'), &span(2, 3));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 3), 2).expect("collects");
  sink.record_token(&MiniTok(b'}'), &span(3, 4));

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

  let mut input = Input::<MiniLexer<'_>, SinkCtx<'_>>::with_state_and_context(
    "abc",
    (),
    crate::input::InputContext::new(
      verbose_sink("abc"),
      DefaultCache::<MiniLexer<'_>>::default(),
    ),
  );
  let mut inp = input.as_ref();

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
  // The parse consumed a prefix and stopped: `c` is unconsumed. That incompleteness is the
  // tooling door's remit (`finish_partial` tiles the tail); strict `finish` would refuse the
  // unexplained trailing gap.
  let (green, _emitter) = input.into_emitter().finish_partial(K_ROOT);
  let green = green.expect("token-only timeline, partial parse");
  assert_eq!(text(green), "abc", "committed tokens + the gap-tiled tail");
}

/// Scan-skipped tokens settle behind the frontier and flow to the tree; the stopper the
/// scan examined but did not consume waits for its real consume.
#[test]
fn auto_emission_scan_skips_flow_and_the_stopper_waits() {
  let sink = verbose_sink("xy;z");
  let mut input = Input::<MiniLexer<'_>, SinkCtx<'_>>::with_state_and_context(
    "xy;z",
    (),
    crate::input::InputContext::new(sink, DefaultCache::<MiniLexer<'_>>::default()),
  );
  let mut inp = input.as_ref();

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
  let mut input =
    Input::<MiniLexer<'_>, (FatalSink<'_>, DefaultCache<'_, MiniLexer<'_>>)>::with_state_and_context(
      "!a",
      (),crate::input::InputContext::new(Sink::new("!a", Fatal::new(), profile()),
      DefaultCache::<MiniLexer<'_>>::default()));
  let mut inp = input.as_ref();

  let res = inp.next();
  assert!(res.is_err(), "the fatal emitter rejects the lexer error");

  drop(inp);
  let events = input.emitter().events();
  assert!(
    events.iter().all(|ev| !matches!(ev, Event::Token { .. })),
    "a rejected error item settles a position, never a token event: {events:?}"
  );
}

/// A non-fatal lexer error is a diagnostic (a `Diag` slot), never a token event; end of
/// input commits nothing.
#[test]
fn auto_emission_lexer_error_and_eof_emit_no_token_event() {
  let mut input = Input::<MiniLexer<'_>, SinkCtx<'_>>::with_state_and_context(
    "!a",
    (),
    crate::input::InputContext::new(verbose_sink("!a"), DefaultCache::<MiniLexer<'_>>::default()),
  );
  let mut inp = input.as_ref();

  // next() crosses the error (reported through the Diag channel) and yields `a`.
  let tok = inp.next().expect("verbose collects").expect("a token");
  assert_eq!(*tok.span_ref(), span(1, 2));
  assert_eq!(token_spans(inp.emitter()), &[span(1, 2)]);

  // End of input: a position commit, not a settle.
  assert!(inp.next().expect("collects").is_none());
  assert_eq!(token_spans(inp.emitter()), &[span(1, 2)]);

  drop(inp);
  let (green, _emitter) = input.into_emitter().finish(K_ROOT);
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
  let sink: CountingSink<'_> = Sink::new("a(b)c;d", CountingEmitter::default(), profile());
  let mut input = Input::<MiniLexer<'_>, CountingCtx<'_>>::with_state_and_context(
    "a(b)c;d",
    (),
    crate::input::InputContext::new(sink, DefaultCache::<MiniLexer<'_>>::default()),
  );
  let mut inp = input.as_ref();

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

  let recorded = input
    .emitter()
    .events()
    .iter()
    .filter(|ev| matches!(ev, Event::Token { .. }))
    .count();
  assert_eq!(
    recorded, 7,
    "a, (, b, ), c, ;, d: seven committed tokens on the sink's own event stream"
  );
  assert_eq!(
    input.emitter().inner_ref().committed,
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

  fn checkpoint(&mut self) -> u64 {
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
  let mut input = Input::<MiniLexer<'_>, JournalingCtx<'_>>::with_state_and_context(
    "abc",
    (),
    crate::input::InputContext::new(
      Sink::new("abc", JournalingEmitter::default(), profile()),
      DefaultCache::<MiniLexer<'_>>::default(),
    ),
  );
  let mut inp = input.as_ref();

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

  let recorded = input
    .emitter()
    .events()
    .iter()
    .filter(|ev| matches!(ev, Event::Token { .. }))
    .count();
  assert_eq!(recorded, 2, "a, b survive on the sink's own event stream");
  assert_eq!(
    input.emitter().inner_ref().journal,
    std::vec![JEntry::Token, JEntry::Token],
    "the inner must be rewound to its checkpoint reading (a, b survive), not past them"
  );
  assert_eq!(
    input.emitter().inner_ref().journal.len(),
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
  let mut input = Input::<MiniLexer<'_>, JournalingCtx<'_>>::with_state_and_context(
    "a!bc",
    (),
    crate::input::InputContext::new(
      Sink::new("a!bc", JournalingEmitter::default(), profile()),
      DefaultCache::<MiniLexer<'_>>::default(),
    ),
  );
  let mut inp = input.as_ref();

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

  assert_eq!(
    input.emitter().inner_ref().journal,
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

/// CONTRACT (mid-log no-row case): a truncating rewind to a mark no checkpoint ever captured
/// has NO exact inner reading anywhere — an **unpaired settle**, which is a parser bug and
/// never an input-dependent condition. It panics at cause in **EVERY** build.
///
/// Deliberately NOT `cfg(debug_assertions)`-gated: the whole point of the hardening is that
/// the release profile behaves identically, so this case must be exercised by
/// `cargo test --release` too. It was a `debug_assertions`-only wall, deferring to the input
/// layer's LIFO witness one level up — but that witness is itself debug-only, so a release
/// build had no wall on either layer and the event log silently sheared away from the
/// diagnostic log. (Pre-fix it went further still and paired the surviving prefix with the
/// construction-time base, destroying committed inner state the log still carried.)
#[test]
#[should_panic(expected = "rewind to a mid-log mark with no captured row")]
fn no_row_middle_rewind_panics_at_cause_in_every_build() {
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));
  let origin = 0usize;
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), 1);
}

/// The panic is a WALL, not a narration: it is raised before the rewind's first mutation, so a
/// host that catches it is left holding the sink it had, not a sheared one.
///
/// This is the half the unconditional wall was missing. The check used to sit at the
/// inner-target match, which runs *after* the rows at and above the mark are spent, after
/// `events.truncate`, after the journal reverse-replay and after the ledger's truncation
/// record. Caught there, the caller kept a sink whose event log had been rewound and whose
/// inner emitter had not — and `finish_partial` would then hand that back as a tree, which is
/// the very shear the wall exists to prevent, merely announced on the way past. The verdict is
/// now a read-only preflight over the unchanged mark stack.
///
/// Every channel is checked, not just the log: the mark stack (a stale row above the mark is
/// how a *later* disciplined rewind gets refused), the journal, the floor's depth memo, and the
/// era ledger — whose truncation record cannot be taken back and would strand every live
/// `EventMark` below it. Then materialization, because a tree is what a caught panic actually
/// lets a host reach.
#[test]
fn caught_unpaired_settle_panic_leaves_the_sink_exactly_as_it_was() {
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());

  // A live capture ABOVE the mark the bad rewind will name, so the row spend has something to
  // destroy, and a retro-wrap mark so the journal and the ledger do too.
  let tomb = CstEmitter::<MiniLexer<'_>, ()>::cst_mark(&mut sink);
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  let live = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));
  CstEmitter::<MiniLexer<'_>, ()>::cst_start_at(&mut sink, tomb, K_NODE);
  CstEmitter::<MiniLexer<'_>, ()>::cst_finish(&mut sink, K_NODE);

  let events_before = sink.events().len();
  let journal_before = sink.journal_len();
  let rows_before = sink.rows_len();
  let forward_parent_before = sink.forward_parent_at(0);
  let inner_before = sink.inner_ref().journal.clone();
  assert!(
    rows_before == 1 && journal_before == 1 && forward_parent_before.is_some(),
    "the fixture must actually arm every channel the rewind would touch: rows \
     {rows_before}, journal {journal_before}, forward_parent {forward_parent_before:?}"
  );

  // Mark 1 is mid-log and no live row captured it — the unpaired settle. `live` sits above it
  // and would be popped; the log would truncate to 1; the journal entry would reverse-replay;
  // the ledger would record a truncation.
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let origin = 0usize;
    Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), 1);
  }));
  let payload = caught.expect_err("an unpaired settle panics on the normal path");
  let message = payload
    .downcast_ref::<std::string::String>()
    .cloned()
    .expect("the wall's payload is a formatted String");
  assert!(
    message.contains("rewind to a mid-log mark with no captured row"),
    "the wall must be what fired, not something downstream of it: {message}"
  );

  // ── Nothing moved. Every cell the rewind would have written, on both sides. ──
  assert_eq!(
    sink.events().len(),
    events_before,
    "E1: the event log must not be truncated by a rewind that was refused"
  );
  assert_eq!(
    sink.journal_len(),
    journal_before,
    "E2: the undo journal must not be reverse-replayed by a rewind that was refused"
  );
  assert_eq!(
    sink.forward_parent_at(0),
    forward_parent_before,
    "E2: the retro-wrap's forward_parent must not be restored by a rewind that was refused"
  );
  assert_eq!(
    sink.rows_len(),
    rows_before,
    "E4: the live capture above the mark must survive — spending it here is what makes a \
     LATER disciplined rewind of it get refused too"
  );
  assert_eq!(
    sink.inner_ref().journal,
    inner_before,
    "the inner is untouched, as it always was on this path"
  );

  // E3: the era ledger records truncations and never un-records them, so a refused rewind
  // that reached it would strand the mark permanently. The mark is still spendable.
  let fresh = CstEmitter::<MiniLexer<'_>, ()>::cst_mark(&mut sink);
  CstEmitter::<MiniLexer<'_>, ()>::cst_start_at(&mut sink, fresh, K_LIST);

  // And the capture above the mark is still live: its own rewind finds its row, so it does not
  // trip the wall a second time on a perfectly disciplined settle.
  let origin = 0usize;
  Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), live);
  assert_eq!(sink.rows_len(), 0, "the surviving capture was spendable");
  assert_eq!(sink.events().len(), live as usize);
  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token],
    "the disciplined rewind restored the inner to its captured reading"
  );
}

/// The same question asked of the materialization door, which is what a host that catches the
/// panic can actually reach: the tree must be the tree of the un-rewound sink, with the two
/// channels agreeing on how many tokens settled.
///
/// Against the post-damage wall this fails twice over — the log is short one token, so the
/// tree carries a `Gap` tile where `b` should be, and the inner still holds both tokens.
#[test]
fn a_caught_unpaired_settle_panic_still_materializes_the_unsheared_tree() {
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));

  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let origin = 0usize;
    Emitter::<MiniLexer<'_>>::rewind(&mut sink, Cursor::from_ref(&origin), 1);
  }));
  assert!(
    caught.is_err(),
    "an unpaired settle panics on the normal path"
  );

  let inner_tokens = sink
    .inner_ref()
    .journal
    .iter()
    .filter(|entry| matches!(entry, JEntry::Token))
    .count();
  let (green, _inner) = sink.finish_partial(K_ROOT);
  let green = green.expect("a refused rewind that changed nothing leaves a materializable log");
  assert_eq!(
    shape(&green),
    "Root[Tok\"a\" Tok\"b\"]",
    "the surviving log is the whole log: no truncation, and therefore no gap tile standing \
     in for the token a half-done rewind would have dropped"
  );
  let tree_tokens = tree(green)
    .descendants_with_tokens()
    .filter(|element| element.as_token().is_some_and(|tok| tok.kind() == K_TOK))
    .count();
  assert_eq!(
    tree_tokens, inner_tokens,
    "the two channels must agree on how many tokens settled — that equality IS the absence \
     of shear"
  );
}

/// The mid-unwind carve-out, and the reason the wall could be made unconditional at all:
/// `Emitter::rewind` may run from a rolling-back guard's `Drop` while a panic is already in
/// flight, where raising a second panic is a **double panic that aborts the process** — no
/// unwinding, no `catch_unwind`, the test binary simply dies. So the report is suppressed when
/// `std::thread::panicking()` is true.
///
/// This test would ABORT rather than fail if the carve-out were removed, which is exactly the
/// outcome it exists to prevent: it drives the no-row mid-log rewind from a `Drop` running
/// under a live panic, and asserts the ORIGINAL panic is what `catch_unwind` observes.
///
/// What the carve-out is *not* allowed to be is a quiet shear. Suppressing the report used to
/// leave the previous posture in place — the sink's own channels rewound, the inner not — and
/// that state then materialized as an ordinary tree. It now degrades to a **total no-op**
/// instead, on every channel, and latches: both channels are left describing the same history,
/// and the fact that a rewind was refused survives to the finish door.
#[test]
fn no_row_middle_rewind_degrades_to_nothing_mid_unwind_instead_of_aborting() {
  /// Settles the sink from a destructor, the way a rolling-back guard does.
  struct RollbackOnDrop<'a, 'inp>(&'a mut JournalingSink<'inp>);

  impl Drop for RollbackOnDrop<'_, '_> {
    fn drop(&mut self) {
      let origin = 0usize;
      // Mid-log, no row: the condition that panics on the normal path.
      Emitter::<MiniLexer<'_>>::rewind(self.0, Cursor::from_ref(&origin), 1);
    }
  }

  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));

  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let _guard = RollbackOnDrop(&mut sink);
    panic!("the original failure");
  }));

  let payload = caught.expect_err("the original panic must propagate");
  let message = payload
    .downcast_ref::<&str>()
    .copied()
    .expect("the original payload survives");
  assert_eq!(
    message, "the original failure",
    "the settle wall must not replace (or double) the panic already unwinding"
  );

  // Silent, but not sheared: the sink has no correct rewind to perform here, so it performs
  // none. Both channels still describe the same history.
  assert_eq!(
    sink.events().len(),
    2,
    "the refused rewind is a total no-op on the sink's own log too — half-rewinding it is \
     exactly the shear the wall exists to prevent"
  );
  assert_eq!(
    sink.inner_ref().journal,
    std::vec![JEntry::Token, JEntry::Token],
    "the inner is untouched, and now so is the log it is in step with"
  );

  // And the degradation is recorded: a host that catches the original panic cannot go on to
  // obtain a tree that silently reflects a rollback that never ran.
  let (green, _inner) = sink.finish_partial(K_ROOT);
  let err = green.expect_err("a degraded rewind must refuse materialization");
  assert_eq!(
    err,
    FinishError::UnpairedSettle { mark: 1, len: 2 },
    "the refusal names the rewind that was refused"
  );
}

/// The poison refuses through **both** doors, and latches: `finish_partial` is the door for an
/// *incomplete* parse, and a log describing a rollback that never happened is not incomplete —
/// it is a complete record of the wrong branch. This is the same posture the neighbouring
/// emission-time walls take (`cst_finish`'s global-underflow assert at cause, with
/// `FinishError::OrphanFinish` as the release backstop at materialization), except that here
/// the at-cause report is suppressed by the unwind rather than by the profile, which makes the
/// typed refusal the only signal a caller can ever see.
#[test]
fn a_degraded_rewind_refuses_both_materialization_doors() {
  /// Drives the mid-unwind rewind, then hands the sink back so a door can be tried.
  fn degraded_sink<'inp>() -> JournalingSink<'inp> {
    struct RollbackOnDrop<'a, 'inp>(&'a mut JournalingSink<'inp>);

    impl Drop for RollbackOnDrop<'_, '_> {
      fn drop(&mut self) {
        let origin = 0usize;
        Emitter::<MiniLexer<'_>>::rewind(self.0, Cursor::from_ref(&origin), 1);
        // A LATER, perfectly disciplined rewind must not launder the refusal away.
        Emitter::<MiniLexer<'_>>::rewind(self.0, Cursor::from_ref(&origin), 0);
      }
    }

    let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
    Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let _guard = RollbackOnDrop(&mut sink);
      panic!("the original failure");
    }));
    assert!(caught.is_err(), "the original panic must propagate");
    sink
  }

  // The strict door.
  let (green, _inner) = degraded_sink().finish(K_ROOT);
  assert_eq!(
    green.expect_err("finish refuses a degraded rewind"),
    FinishError::UnpairedSettle { mark: 1, len: 2 }
  );

  // The tooling door — the one that tolerates open nodes and uncovered gaps, and must not
  // tolerate this.
  let (green, _inner) = degraded_sink().finish_partial(K_ROOT);
  assert_eq!(
    green.expect_err("finish_partial refuses a degraded rewind too"),
    FinishError::UnpairedSettle { mark: 1, len: 2 },
    "the LATCH: a later lawful rewind to the origin must not clear the record"
  );
}

/// The `release` half of the settle contract stays SILENT, and this pins that it is a
/// decision rather than an omission. Both non-top outcomes are specified behaviour, mirrored
/// from `InputRef::commit`'s own cost model — a linear removal when a younger capture is
/// still live, and a harmless no-op ("no panic, in any build") when the mark is already gone.
/// Hardening either would convert a documented guarantee into a crash.
///
/// Also pins the structural claim that lets the asymmetry stand: a middle `remove` fires only
/// when the top row's mark differs, so at least two rows are present and `rows` can never be
/// emptied by it — the mark stack stays a superset of the live captures, never a subset.
#[test]
fn release_of_a_non_innermost_mark_is_lawful_and_silent() {
  let mut sink = verbose_sink("abc");

  // An outer capture, some traffic, then an inner capture: two rows at distinct marks.
  let outer = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  let inner = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'b'), &span(1, 2));
  assert!(outer < inner, "the captures sit at distinct marks");
  assert_eq!(sink.rows_len(), 2);

  // Release the OUTER one first — out of stack order, which the settle contract permits
  // across families. No panic, and the inner row survives.
  Emitter::<MiniLexer<'_>>::release(&mut sink, outer);
  assert_eq!(
    sink.rows_len(),
    1,
    "a middle removal takes exactly one row and cannot empty the stack"
  );

  // The inner capture is still fully spendable: its row is live, so its rewind finds an exact
  // inner reading and never reaches the unpaired-settle wall.
  rewind(&mut sink, inner);
  assert_eq!(sink.rows_len(), 0);
  assert_eq!(
    sink.events().len(),
    inner as usize,
    "the inner capture rewinds exactly to its own mark"
  );

  // Releasing a mark that is already gone is the documented no-op — in every build.
  Emitter::<MiniLexer<'_>>::release(&mut sink, outer);
  Emitter::<MiniLexer<'_>>::release(&mut sink, inner);
  assert_eq!(sink.rows_len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// What the wall costs the layer above it: `InputRef::restore_unchecked`
// ═══════════════════════════════════════════════════════════════════════════════

/// MEASUREMENT, not a guarantee: `InputRef::restore_unchecked` is **not** transactional across
/// a `Sink` that reports an unpaired settle, and this records exactly how it fails so the claim
/// is evidence rather than inspection.
///
/// The body is phase-separated (`SETTLE_CENSUS`) so that everything running caller code happens
/// while the input is still wholly on the abandoned branch, and everything below the
/// `CENSUS_PHASE_BOUNDARY` installs facts with no caller code among them. `Emitter::rewind` is
/// the one call below that line, admitted there on the strength of a contract that said an
/// emitter may not panic on this path. That contract is now weaker — an emitter that can
/// *detect* an unpaired settle may report it when no unwind is in flight — so the phase
/// boundary's premise no longer holds, and a panic from the emitter tears the restore in half:
///
/// - the lineage has already been popped through the target id (the restored shape), while
/// - the position, the three witness facts and the cache-push counter are never installed (the
///   abandoned shape).
///
/// Both halves are asserted below. This is a **found, not fixed** condition: the tear predates
/// the wall becoming unconditional (the same panic existed in debug builds), it is reachable
/// only through the same double-settle bug the wall is reporting, and closing it would mean
/// reordering `restore_unchecked`'s phase 2 — which cannot make the call whole anyway, because
/// phase 1 has already abandoned the session points above the target and cleared the parked
/// slot. What the fix in the sink *does* guarantee is the other half of the question: the
/// **emitter** is transactional. It is asserted here too.
///
/// Gated to the same feature set as `live_checkpoints_len`, the lineage observable.
#[cfg(all(feature = "logos_0_16", feature = "std"))]
#[test]
fn restore_unchecked_is_not_transactional_across_the_settle_wall() {
  let mut input = Input::<MiniLexer<'_>, JournalingCtx<'_>>::with_state_and_context(
    "abcd",
    (),
    crate::input::InputContext::new(
      Sink::new("abcd", JournalingEmitter::default(), profile()),
      DefaultCache::<MiniLexer<'_>>::default(),
    ),
  );
  // The handle and the `outer` checkpoint it borrows share one scope, so the input can be
  // consumed for the materialization check afterwards.
  {
    let mut inp = input.as_ref();

    // Two nested saves with a settle between them, so their emitter marks differ.
    inp.next().expect("collects").expect("a token");
    let _outer = inp.save();
    inp.next().expect("collects").expect("b token");
    // The sink's mark IS the event-log length, so this is `doomed`'s emitter checkpoint.
    let doomed_mark = inp.emitter().events().len() as u64;
    let doomed = inp.save();
    inp.next().expect("collects").expect("c token");
    assert_eq!(inp.live_checkpoints_len(), 2, "both ids are live going in");

    // Spend `doomed`'s emitter row WITHOUT the lineage pop the input's own commit funnel would
    // pair with it. That is exactly the state a double settle leaves behind — a live lineage id
    // whose emitter row is gone — and it is the only way this wall is reachable through the
    // input at all.
    Emitter::<MiniLexer<'_>>::release(inp.emitter(), doomed_mark);
    assert_eq!(
      inp.emitter().rows_len(),
      1,
      "only `outer`'s row is left; `doomed`'s mark is now unpaired"
    );

    let committed_before = *crate::Span::end_ref(inp.span());
    let events_before = inp.emitter().events().len();
    let rows_before = inp.emitter().rows_len();
    let inner_before = inp.emitter().inner_ref().journal.clone();
    assert!(
      doomed_mark < events_before as u64,
      "the rewind must TRUNCATE for the wall to apply: mark {doomed_mark} of {events_before}"
    );

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      inp.restore_unchecked(doomed);
    }));
    assert!(
      caught.is_err(),
      "the doubly-settled mark is mid-log with no live row: the wall must fire"
    );

    // ── The emitter half: TRANSACTIONAL. The preflight decided before touching a cell. ──
    assert_eq!(
      inp.emitter().events().len(),
      events_before,
      "the sink's event log is untouched by the rewind it refused"
    );
    assert_eq!(inp.emitter().rows_len(), rows_before);
    assert_eq!(inp.emitter().inner_ref().journal, inner_before);

    // ── The input half: TORN. One observable from each side of the phase boundary. ──
    assert_eq!(
      *crate::Span::end_ref(inp.span()),
      committed_before,
      "NOT restored: `install_position` sits BELOW the emitter rewind and never ran, so the \
     input is still on the abandoned branch"
    );
    assert_eq!(
      inp.live_checkpoints_len(),
      1,
      "but ALREADY invalidated: `live_pop_through` sits ABOVE the emitter rewind and did run. \
     The restore landed between its two admissible outcomes — the input layer is not \
     transactional across an emitter that reports on this path"
    );
  }

  // The emitter is, though, all the way to the door: a normal-path report leaves no poison, so
  // the log still materializes and it is the log the parse actually produced.
  let sink = input.into_emitter();
  let (green, _inner) = sink.finish_partial(K_ROOT);
  let green = green.expect("a refused rewind that changed nothing leaves a materializable log");
  assert_eq!(
    text(green),
    "abcd",
    "and it is still lossless: three settled tokens plus the untouched tail"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Out-of-range marks are ignored BEFORE the row lookup
// ═══════════════════════════════════════════════════════════════════════════════

/// REGRESSION: an out-of-range FUTURE mark (`checkpoint > len`) is a rewind to
/// a point the log has not reached — a TOTAL no-op on every channel, the mark stack included.
/// Pre-fix, `mark = checkpoint.min(len)` clamped it to the length BEFORE the row lookup, so a
/// future mark masqueraded as a rewind-to-current and spent the live row of a REAL checkpoint
/// taken at the current length; that checkpoint's own later rewind then found no row — which,
/// now that the mid-log no-row wall is unconditional, would panic on a perfectly disciplined
/// mark in every build. (Pre-fix it fired only in debug, and the release build instead let the
/// inner ghost the abandoned branch's records.) The wall being loud is exactly why this
/// early-return has to stay exact rather than defensive.
#[test]
fn out_of_range_rewind_spends_no_live_row() {
  let mut sink: JournalingSink<'_> = Sink::new("ab", JournalingEmitter::default(), profile());
  let origin = 0usize;

  // One settled token, then a live checkpoint AT the current length: len == 1, row at 1.
  Emitter::<MiniLexer<'_>>::commit_token(&mut sink, &MiniTok(b'a'), &span(0, 1));
  let c = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
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
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      sink_with_open_nodes("abcdef", 1),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();
    let mut txn = inp.begin_stacked();
    let sp1 = txn.savepoint();
    let _sp2 = txn.savepoint();
    let _sp3 = txn.savepoint();
    txn.rollback_to(sp1);
    txn.commit();
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    input.emitter().rows_len(),
    0,
    "`rollback_to` + `commit` must leave no live row: the younger savepoints' captures \
     alias the target's mark, and a single restore cannot name them"
  );
}

/// The whole-transaction rollback restores only the begin point, so the savepoints above it
/// have to be settled first.
#[test]
fn stacked_whole_rollback_settles_every_aliased_savepoint_row() {
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      sink_with_open_nodes("abcdef", 1),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();
    let mut txn = inp.begin_stacked();
    let _sp1 = txn.savepoint();
    let _sp2 = txn.savepoint();
    txn.rollback();
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    input.emitter().rows_len(),
    0,
    "a whole `rollback` must settle every savepoint capture before restoring the base"
  );
}

/// The rolling-back drop — `begin_stacked`'s default policy, and the arm every `?`
/// early-return and every unwind through a stacked scope takes. It holds the savepoint
/// checkpoints at the moment their marks die, so it is the only place they can be settled.
#[test]
fn stacked_rollback_on_drop_settles_every_aliased_savepoint_row() {
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      sink_with_open_nodes("abcdef", 1),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();
    {
      let mut txn = inp.begin_stacked();
      let _sp1 = txn.savepoint();
      let _sp2 = txn.savepoint();
      // …dropped undecided: the rollback-on-drop arm.
    }
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    input.emitter().rows_len(),
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
    let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::with_state_and_context(
      "abcdef",
      (),
      crate::input::InputContext::new(
        sink_with_open_nodes("abcdef", depth),
        DefaultCache::<'_, MiniLexer<'_>>::default(),
      ),
    );
    {
      let mut inp = input.as_ref();
      {
        let mut txn = inp.begin_stacked();
        let _sp1 = txn.savepoint();
        let _sp2 = txn.savepoint();
        let _sp3 = txn.savepoint();
      }
      close_open_nodes(inp.emitter(), depth);
    }
    assert_eq!(
      input.emitter().rows_len(),
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
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      sink_with_open_nodes("abcdef", 1),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();
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
    input.emitter().rows_len(),
    0,
    "a lineage-invalidated savepoint still owns an emitter capture: the drop must settle it"
  );
}

/// The control: `release` and `commit` already settle each capture individually, so they are
/// exact over aliased marks. They are the shape the abandon paths must match.
#[test]
fn stacked_release_and_commit_settle_every_aliased_row() {
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      sink_with_open_nodes("abcdef", 1),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();
    let mut txn = inp.begin_stacked();
    let sp1 = txn.savepoint();
    let _sp2 = txn.savepoint();
    let _sp3 = txn.savepoint();
    txn.release(sp1);
    txn.commit();
    close_open_nodes(inp.emitter(), 1);
  }
  assert_eq!(
    input.emitter().rows_len(),
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
  let mut input = Input::<'_, MiniLexer<'_>, StackedCtx<'_>, ()>::with_state_and_context(
    "abcdef",
    (),
    crate::input::InputContext::new(
      sink_with_open_nodes("abcdef", 1),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
    ),
  );
  {
    let mut inp = input.as_ref();
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
    input.emitter().rows_len(),
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
    let mut input = Input::<'_, MiniLexer<'_>, BorrowedCtx<'_, '_>, ()>::with_state_and_context(
      "abcdef",
      (),
      crate::input::InputContext::new(
        Sink::new("abcdef", &mut verbose, profile()),
        DefaultCache::<'_, MiniLexer<'_>>::default(),
      ),
    );
    {
      let mut inp = input.as_ref();
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
      input.emitter().rows_len(),
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

// ── The replay-work law W and the Diag-arm pins ────────────────────────────────
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
    Emitter::<MiniLexer<'_>>::commit_lexer_error(
      &mut sink,
      Spanned::new(span(lo, lo + 1), MiniErr),
    )
    .expect("verbose collects");
    sink.record_token(&MiniTok(b'a'), &span(lo + 1, lo + 2));
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

/// **T-3 — the acceptance gate.** Replay work matches its stated shape on error-dense
/// input: **linear in events, plus one `k log k` ordering of the recorded diagnostic spans**.
///
/// The name of this cell used to say "is linear", and the cell used to report 4.00× growth for
/// a 4× input at every size. Both were artifacts: the sort was charged a flat element count,
/// which made the charged quantity linear by construction, so the growth clause could not fail
/// for the reason it existed. Charged at its real cost the same payload reports 4.57× — the
/// sort became visible to its own gate.
///
/// Falsified by: any n outside the two-sided law `events ≤ W ≤ 3 × (events + gap_tiles)`,
/// any per-4×n growth ratio above 4.5, or an exact-composition mismatch.
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
    //
    // The single-pass walk dropped one whole `events` term (the gather pass) and added one
    // `tiles` term (the post-walk sweep of the tiled runs). The lookahead that replaced the
    // gather pass contributes **nothing at all** on this payload, and that is the point of
    // saying so here: every run in it is resolved by the token that follows it, so
    // `tile_pending_run` is never reached.
    let expected = events            // the walk, once per event
      + sort_term                    // the sort, charged at its real `k log k` cost
      + diags                        // the cover-merge loop, one per gathered span
      + tiles                        // the post-walk sweep of the tiled runs, one per run
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

/// The exact-composition pin's second cell: a wrap-bearing payload, where `W` must
/// equal `events + chain_hops + lookahead` — the one pass over every event, one hop per
/// retro-wrap link followed, and the monotone lookahead that replaced the gather pass. `m`
/// single-wrap targets: 4 events each (mark, token, `StartAt`, finish) and one chain hop
/// each; with no diagnostics, the sort charge, the merge loop and the cover cursor all
/// contribute nothing, and nothing is tiled.
///
/// This is the payload where the lookahead **does** fire, which is why it is worth pinning
/// separately from the error-dense one where it never does: each token is followed by a
/// `StartAt` and a finish before the next token, so the finish forces a resolution and the
/// cursor walks to the next token — two positions each, and one terminal position for the
/// last token, where the cursor runs off the end and the run ends at the source's end.
/// Every one of those resolutions finds the run empty; not one gap is tiled.
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
    sink.record_token(&MiniTok(b'a'), &span(i, i + 1));
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
  // Two lookahead positions per forced resolution (the `StartAt` and the next mark), for the
  // first m - 1 tokens, plus the one terminal position the last token's resolution reaches.
  let lookahead = 2 * (m as u64 - 1) + 1;
  assert_eq!(
    w,
    events + chain_hops + lookahead,
    "W must be exactly one pass over {events} events, plus {chain_hops} chain hops, plus \
     {lookahead} monotone lookahead positions"
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
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::commit_lexer_error(sink, Spanned::new(span(2, 3), MiniErr))
      .expect("verbose collects");
    sink.record_token(&MiniTok(b'e'), &span(4, 5));
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
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::commit_lexer_error(sink, Spanned::new(span(2, 4), MiniErr))
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
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("balance is checked before the gap latch is consumed"),
    FinishError::UnclosedNodes { open: 1 }
  );

  // (B) a later overlapping token span outranks it.
  let mut sink = verbose_sink("abc");
  sink.record_token(&MiniTok(b'b'), &span(1, 2)); // reveals the unexplained gap [0,1)
  sink.record_token(&MiniTok(b'a'), &span(0, 1)); // index 1: non-monotone
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    green.expect_err("the in-walk span wall answers first"),
    FinishError::OverlappingSpans { index: 1 }
  );

  // (C) a later out-of-bounds token span outranks it.
  let mut sink = verbose_sink("abc");
  sink.record_token(&MiniTok(b'b'), &span(1, 2)); // reveals the unexplained gap [0,1)
  sink.record_token(&MiniTok(b'c'), &span(2, 99)); // index 1: past the source end
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
    sink.record_token(&MiniTok(b'a'), &span(0, 1));
    Emitter::<MiniLexer<'_>>::commit_lexer_error(sink, Spanned::new(span(3, 4), MiniErr))
      .expect("verbose collects");
    sink.record_token(&MiniTok(b'f'), &span(5, 6));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1)); // index 1
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink); // the truncation point, index 2

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
/// `debug_assertions`-gated: it has teeth in debug-assertions test runs and the CI profiles
/// that run this cell; release-profile test runs do not exercise that wall.
///
/// **Debug-only, and deliberately so.** The wall this exercises is `cfg!(debug_assertions)`-
/// gated in the walk, because keeping it per-event in release costs a measured 4.4% on ordinary
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

/// The root kind has no door in front of it the way a node, token, or retro-wrap kind does:
/// `finish`/`finish_partial` take it as a bare `u16` argument, so it cannot be walled off at an
/// emit site the way `cst_start` walls off node kinds. It is instead checked right here,
/// unconditionally, in every build — the one `InvalidDialectKind` construction site a caller
/// outside this crate can reach, through the entirely ordinary public door (no raw event
/// injection, unlike [`out_of_language_kind_refused`]).
///
/// Falsified by: an `Ok` tree, `ReservedRootKind` (the chosen kind is not the tombstone), or an
/// `InvalidDialectKind` naming the wrong index or kind. Unlike the debug-only cell above, this
/// one carries no `cfg!(debug_assertions)` gate, because the root-kind check it pins does not
/// either — the refusal it asserts must reproduce identically under `cargo test --release`.
#[test]
fn out_of_space_root_kind_refused_through_the_public_door() {
  const BAD_ROOT: u16 = 9_999;

  let sink = verbose_sink("a");
  let (green, _emitter) = sink.finish(BAD_ROOT);
  assert_eq!(
    green.expect_err("a root kind outside the dialect's space is not a tree"),
    FinishError::InvalidDialectKind {
      index: 0,
      kind: BAD_ROOT
    }
  );

  // `finish_partial` replays through the same `replay` call, so it refuses the same way.
  let sink = verbose_sink("a");
  let (green, _emitter) = sink.finish_partial(BAD_ROOT);
  assert_eq!(
    green.expect_err("finish_partial refuses the same out-of-space root kind"),
    FinishError::InvalidDialectKind {
      index: 0,
      kind: BAD_ROOT
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
    Emitter::<MiniLexer<'_>>::commit_lexer_error(sink, Spanned::new(span(0, 99), MiniErr))
      .expect("verbose collects");
    sink.record_token(&MiniTok(b'c'), &span(2, 3));
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
  sink.record_token(&MiniTok(b'c'), &span(2, 3));
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

  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.record_token(&MiniTok(0xC3), &span(1, 3));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));

  // A retro-wrap.
  let mark = sink.cst_mark();
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_start_at(mark, K_WRAP);
  sink.cst_finish(K_WRAP);

  // A direct node.
  sink.cst_start(K_LIST);
  sink.record_token(&MiniTok(b'c'), &span(2, 3));
  sink.cst_finish(K_LIST);

  // A recovery hole: the sink synthesizes the K_ERR wrap itself.
  sink.record_token(&MiniTok(b'd'), &span(3, 4));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(3, 4), 1).expect("collects");

  // A refused byte: the sink synthesizes the K_GAP tile itself.
  Emitter::<MiniLexer<'_>>::commit_lexer_error(&mut sink, Spanned::new(span(4, 5), MiniErr))
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

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

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

  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
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

// The collecting bundle's conversions — needed by the `repeated_while` pin below, which is the
// one shape in this file that goes through a public collecting combinator.
impl<S, Lang: ?Sized> From<crate::error::syntax::FullContainer<S, Lang>> for SesErr {
  fn from(_: crate::error::syntax::FullContainer<S, Lang>) -> Self {
    Self::Lex
  }
}

impl<S, Lang: ?Sized> From<crate::error::syntax::TooFew<S, Lang>> for SesErr {
  fn from(_: crate::error::syntax::TooFew<S, Lang>) -> Self {
    Self::Lex
  }
}

impl<S, Lang: ?Sized> From<crate::error::syntax::TooMany<S, Lang>> for SesErr {
  fn from(_: crate::error::syntax::TooMany<S, Lang>) -> Self {
    Self::Lex
  }
}

impl<O, Lang: ?Sized> From<crate::error::syntax::MissingSyntax<O, Lang>> for SesErr {
  fn from(_: crate::error::syntax::MissingSyntax<O, Lang>) -> Self {
    Self::Lex
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<crate::error::token::SeparatedError<'a, T, K, S, Lang>>
  for SesErr
{
  fn from(_: crate::error::token::SeparatedError<'a, T, K, S, Lang>) -> Self {
    Self::Lex
  }
}

impl<D, S, Lang: ?Sized> From<crate::error::Unclosed<D, S, Lang>> for SesErr {
  fn from(_: crate::error::Unclosed<D, S, Lang>) -> Self {
    Self::Lex
  }
}

impl<H, O, Lang: ?Sized, Set: Clone> From<crate::error::UnexpectedEnd<H, O, Lang, Set>> for SesErr {
  fn from(_: crate::error::UnexpectedEnd<H, O, Lang, Set>) -> Self {
    Self::Lex
  }
}

type SesSink<'inp> = Sink<'inp, MiniLexer<'inp>, Verbose<SesErr>>;
type SesCtx<'inp, 'e> = (&'e mut SesSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);

/// A deliberately **incomplete** forwarder: every emission goes through — the diagnostic
/// surface, the settle channel, and the `CstEmitter` structuring surface below — `bound_source`
/// does not. It exists to pin the residue, not to be imitated — a real wrapper forwards
/// `bound_source` exactly as the `&mut U` blanket does.
struct NonForwardingWrapper<'a, 'inp>(&'a mut SesSink<'inp>);

type NonFwdCtx<'inp, 'e> = (
  NonForwardingWrapper<'e, 'inp>,
  DefaultCache<'inp, MiniLexer<'inp>>,
);

type MiniSpan<'inp> = <MiniLexer<'inp> as Lexer<'inp>>::Span;
type MiniToken<'inp> = <MiniLexer<'inp> as Lexer<'inp>>::Token;
type MiniLexErr<'inp> = <MiniToken<'inp> as crate::Token<'inp>>::Error;

impl<'inp> crate::Emitter<'inp, MiniLexer<'inp>> for NonForwardingWrapper<'_, 'inp> {
  type Error = SesErr;

  fn emit_lexer_error(
    &mut self,
    err: Spanned<MiniLexErr<'inp>, MiniSpan<'inp>>,
  ) -> Result<(), Self::Error> {
    self.0.emit_lexer_error(err)
  }

  // Forwarded like `commit_token`: the input layer's own refusals must reach the sink AS
  // refusals, or the coverage evidence they carry is lost and a legitimately unlexable region
  // stops being explained.
  fn commit_lexer_error(
    &mut self,
    err: Spanned<MiniLexErr<'inp>, MiniSpan<'inp>>,
  ) -> Result<(), Self::Error> {
    self.0.commit_lexer_error(err)
  }

  fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, MiniLexer<'inp>, ()>,
  ) -> Result<(), Self::Error> {
    self.0.emit_unexpected_token(err)
  }

  fn emit_error(&mut self, err: Spanned<Self::Error, MiniSpan<'inp>>) -> Result<(), Self::Error> {
    self.0.emit_error(err)
  }

  fn emit_warning(&mut self, w: Spanned<Self::Error, MiniSpan<'inp>>) -> Result<(), Self::Error> {
    self.0.emit_warning(w)
  }

  fn emit_skipped_region(
    &mut self,
    span: MiniSpan<'inp>,
    skipped: usize,
  ) -> Result<(), Self::Error> {
    self.0.emit_skipped_region(span, skipped)
  }

  fn checkpoint(&mut self) -> u64 {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::checkpoint(&mut *self.0)
  }

  fn rewind(&mut self, cursor: &crate::input::Cursor<'inp, '_, MiniLexer<'inp>>, ckp: u64) {
    self.0.rewind(cursor, ckp)
  }

  fn release(&mut self, ckp: u64) {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::release(&mut *self.0, ckp)
  }

  fn commit_token(&mut self, tok: &MiniToken<'inp>, span: &MiniSpan<'inp>) {
    self.0.commit_token(tok, span)
  }

  // `bound_source` is DELIBERATELY not forwarded — that omission is the whole point of the
  // type, and `a_non_forwarding_wrapper_disables_the_check_and_that_is_pinned` asserts what
  // it costs.
}

/// The structuring surface, forwarded whole — because a wrapper that did **not** forward it
/// could not carry a CST parse at all (`CstEmitter` is a bound, not a default, on the `node()`
/// combinators), so the realistic adversary is a wrapper that forwards everything a tree needs
/// and still hides the binding. Pin 5 is the shape that needs it.
///
/// One of the five is no longer forwarded voluntarily: as of 0.9.0 `cst_demote` is a **required**
/// method, so rustc refuses this impl unless the line below is present. That is the whole reach of
/// that change and it is worth stating here, next to the residue it does *not* touch — this type's
/// omission is `Emitter::bound_source`, which is defaulted on the **core** trait and stays that
/// way, so what the three cells below pin is unchanged. What is gone is the *silent* half of the
/// same shape on the CST surface: a wrapper can still discard the failing exit, but only by
/// writing the discard where its own reviewer reads it.
///
/// The `cst_demote` line below is pinned by `a_wrapper_forwards_the_node_brackets_failing_exit`,
/// and needs to be: emptying it once left the whole lib suite green, because pin 5 drives
/// `cst_start`/`cst_finish` only and every other demote exercise bypasses a wrapper entirely.
impl<'inp> CstEmitter<'inp, MiniLexer<'inp>> for NonForwardingWrapper<'_, 'inp> {
  fn cst_start(&mut self, kind: u16) -> EventMark {
    self.0.cst_start(kind)
  }

  fn cst_finish(&mut self, kind: u16) {
    self.0.cst_finish(kind);
  }

  fn cst_demote(&mut self, mark: EventMark, kind: u16) {
    self.0.cst_demote(mark, kind);
  }

  fn cst_mark(&mut self) -> EventMark {
    self.0.cst_mark()
  }

  fn cst_start_at(&mut self, mark: EventMark, kind: u16) {
    self.0.cst_start_at(mark, kind);
  }
}

// ── The source-identity handshake ──────────────────────────────────────────────

/// **The matching-source control, and it comes first deliberately.**
///
/// `SourceIdentity`'s projection is proven separately (it compiles at the MSRV and `str`,
/// `[u8]` and `BStr` views of one buffer agree). What that does *not* prove is the **wiring**:
/// that `Input`'s `input` field and `Sink`'s `source` field really do observe the same
/// referent when both are reached through the crate's own construction paths. If they did not,
/// the refusal cell below would pass for the wrong reason — it would be catching the crate
/// disagreeing with itself rather than catching a foreign source.
///
/// Falsifying output: a panic. That means the two sides' projections diverge on a pairing that
/// is correct, and the check is unusable as written.
#[test]
fn sink_and_parse_over_one_buffer_agree_on_identity() {
  let buf = std::string::String::from("ab");
  let src: &str = &buf;

  let mut sink: SesSink<'_> = Sink::new(src, Verbose::new(), profile());
  {
    let mut input =
      crate::input::Input::<MiniLexer<'_>, SesCtx<'_, '_>, ()>::with_state_and_context(
        src,
        (),
        crate::input::InputContext::new(&mut sink, DefaultCache::<'_, MiniLexer<'_>>::default()),
      );
    let mut inp = input.as_ref();
    while let Ok(Some(_)) = inp.next() {}
  }
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect("a matching pairing must materialize");
  assert_eq!(
    text(green),
    "ab",
    "the tree's text is the one buffer both sides hold"
  );
}

/// **The flip.** This cell used to be named `sink_bound_to_a_foreign_source_is_not_yet_detected`
/// and asserted today's *wrong* answer — a tree whose text was `"XY"` and whose structure came
/// from `"ab"` — so that closing the hole would flip it. Closing it did.
///
/// `Sink::new` binds a source, and `'inp` proves that borrow and the parser's borrow both
/// outlive the sink — **not that they are the same buffer**. Equal length is the point: a
/// length check alone never caught this, and `finish` never could, because the event log a
/// wrongly-paired sink produces is byte-identical to one a legal single parse could produce.
/// The identity handshake at the point the emitter is attached to the input is the only place
/// with both halves in hand.
///
/// `MiniLexer::Source = str`, which is unsized, so `REFERENT_IS_BYTES` is `true` and an
/// unequal reference here is *proof* of an unequal source. The companion cell below pins what
/// happens where it is not proof.
///
/// Falsifying output: no panic. That means the check did not fire at the seam.
#[test]
#[should_panic(expected = "bound to a different source value")]
fn sink_bound_to_a_foreign_source_is_refused() {
  let mut sink: SesSink<'_> = Sink::new("XY", Verbose::new(), profile());
  let mut input = crate::input::Input::<MiniLexer<'_>, SesCtx<'_, '_>, ()>::with_state_and_context(
    "ab",
    (),
    crate::input::InputContext::new(&mut sink, DefaultCache::<'_, MiniLexer<'_>>::default()),
  );
  let mut inp = input.as_ref();
  while let Ok(Some(_)) = inp.next() {}
}

/// **The cell that guards the conservative posture. Without it the posture is a paragraph.**
///
/// The refusal fires only where an unequal reference *proves* an unequal source. Here the two
/// sides hold the same buffer through two different `&str` **locals** — a shape a caller
/// writes without thinking — and the projection of a `&str` would measure where the pointer is
/// stored, not where the bytes are. `L::Source` is `str`, so the projection reads through to
/// the bytes and this is accepted for the right reason; the sized-backing case is pinned by
/// the `REFERENT_IS_BYTES` unit below, which is where an inequality genuinely proves nothing.
///
/// Falsifying output: a panic. That would mean the conservative posture leaked and the check
/// is refusing correct code after all.
#[test]
fn a_second_reference_to_one_buffer_is_not_refused() {
  let buf = std::string::String::from("ab");
  let sink_view: &str = &buf;
  let parse_view: &str = &buf[..];

  let mut sink: SesSink<'_> = Sink::new(sink_view, Verbose::new(), profile());
  {
    let mut input =
      crate::input::Input::<MiniLexer<'_>, SesCtx<'_, '_>, ()>::with_state_and_context(
        parse_view,
        (),
        crate::input::InputContext::new(&mut sink, DefaultCache::<'_, MiniLexer<'_>>::default()),
      );
    let mut inp = input.as_ref();
    while let Ok(Some(_)) = inp.next() {}
  }
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    text(green.expect("two views of one buffer are one source")),
    "ab"
  );
}

/// The `REFERENT_IS_BYTES` census, as **compile-time** assertions, because the whole
/// conservative posture rests on exactly three backings answering `true` and that is a fact
/// about types rather than about a run.
///
/// Falsifying output: a build failure. Either a `?Sized` backing lost its override — the check
/// goes inert on the crate's own lexers — or a **sized** one gained one, which is the direction
/// that matters: the check would start refusing correct code.
const _: () = {
  use crate::Source;

  assert!(
    <str as Source<usize>>::REFERENT_IS_BYTES,
    "str is unsized: the reference is the data"
  );
  assert!(
    <[u8] as Source<usize>>::REFERENT_IS_BYTES,
    "[u8] is unsized: the reference is the data"
  );
  assert!(
    !<&str as Source<usize>>::REFERENT_IS_BYTES,
    "&str is Sized: the reference addresses the pointer variable, not the bytes"
  );
  assert!(
    !<&[u8] as Source<usize>>::REFERENT_IS_BYTES,
    "&[u8] is Sized: the reference addresses the pointer variable, not the bytes"
  );
};

#[cfg(feature = "bstr_1")]
const _: () = {
  use crate::Source;

  assert!(
    <bstr_1::BStr as Source<usize>>::REFERENT_IS_BYTES,
    "BStr is unsized"
  );
  assert!(
    !<&bstr_1::BStr as Source<usize>>::REFERENT_IS_BYTES,
    "&BStr is Sized"
  );
};

#[cfg(feature = "bytes_1")]
const _: () = {
  use crate::Source;

  assert!(
    !<bytes_1::Bytes as Source<usize>>::REFERENT_IS_BYTES,
    "an owned handle addresses a variable; two clones of one Arc are the same source at two \
     addresses, and refusing them would panic correct code"
  );
};

/// **The forwarder cell, and it is load-bearing.** A wrapper that forwards every emission but
/// inherits `bound_source`'s `None` default silently disables the check for whatever it wraps
/// — the same forwarding obligation `checkpoint`/`rewind`/`release` carry.
///
/// Falsifying output: `None` from the `&mut` blanket, which is the silently-disabled state.
#[test]
fn forwarder_preserves_bound_source() {
  use crate::Emitter;

  let buf = std::string::String::from("ab");
  let src: &str = &buf;
  let mut sink: SesSink<'_> = Sink::new(src, Verbose::new(), profile());

  let direct = Emitter::<'_, MiniLexer<'_>, ()>::bound_source(&sink);
  assert!(direct.is_some(), "a sink binds a source");

  let borrowed = &mut sink;
  let through_blanket = Emitter::<'_, MiniLexer<'_>, ()>::bound_source(&borrowed);
  assert_eq!(
    through_blanket, direct,
    "the `&mut U` blanket must forward `bound_source`; answering `None` here would disable \
     the identity check for every sink reached through a mutable borrow — which is how every \
     parse reaches one"
  );
}

/// **The prefix cell.** Two slices of ONE buffer share an origin, so an origin-only identity
/// accepts a sink bound to `&buf[..2]` while the parse reads `&buf[..3]`.
///
/// The attack needs no out-of-bounds span, which is why `FinishError::SpanOutOfBounds` cannot
/// see it: the parser reads byte 2 and lets it choose **structure**, then commits only spans
/// inside `0..2`. `finish` materializes a tree over the sink's shorter text with no
/// out-of-bounds event anywhere, and the round-trip law still holds — `tree.text()` is exactly
/// the sink's buffer. What escaped is structure shaped by a byte absent from the materialized
/// source: the original wrong-source class, surviving for same-allocation prefixes.
///
/// Falsifying output: no panic. That means the seam compared origins and ignored extent.
#[test]
#[should_panic(expected = "shorter extent")]
fn sink_bound_to_a_prefix_of_the_parsed_source_is_refused() {
  let buf = std::string::String::from("abc");
  let sink_src: &str = &buf[..2];
  let parse_src: &str = &buf[..3];

  // Same origin — an origin-only identity cannot tell these apart.
  assert_eq!(
    crate::source::SourceIdentity::of(sink_src).addr(),
    crate::source::SourceIdentity::of(parse_src).addr(),
    "the premise of this cell: two prefixes of one buffer share an offset origin"
  );

  let mut sink: SesSink<'_> = Sink::new(sink_src, Verbose::new(), profile());
  let mut input = crate::input::Input::<MiniLexer<'_>, SesCtx<'_, '_>, ()>::with_state_and_context(
    parse_src,
    (),
    crate::input::InputContext::new(&mut sink, DefaultCache::<'_, MiniLexer<'_>>::default()),
  );
  // The seam. Everything below is what accepting this pairing costs.
  let mut inp = input.as_ref();

  // Consume exactly the two bytes the sink's buffer holds. Both commit, both are in bounds,
  // and together they cover `0..2` completely — so neither `SpanOutOfBounds` nor
  // `UncoveredGap` has anything to report.
  inp.next().expect("lexes").expect("byte 0");
  inp.next().expect("lexes").expect("byte 1");

  // Now PEEK byte 2. `peek_one` does not advance the cursor and commits nothing, so this byte
  // enters no span and no event — it only informs a decision.
  let saw_a_byte_the_sink_does_not_have = inp.peek_one().expect("peeks").is_some();
  drop(inp);

  // …and the decision picks the tree's shape.
  let root_kind = if saw_a_byte_the_sink_does_not_have {
    K_LIST
  } else {
    K_NODE
  };
  let (green, _emitter) = sink.finish(root_kind);
  let green = green.expect("every committed span is in bounds and `0..2` is fully covered");
  assert_eq!(text(green.clone()), "ab", "the text is the sink's buffer");
  assert_eq!(
    tree(green).kind(),
    K_LIST,
    "and the shape came from byte 2, which is not in that text — with no out-of-bounds span \
     and no uncovered gap anywhere for a later check to catch"
  );
}

/// The other direction, and it must stay **accepted at the seam**: a sink bound to a *longer*
/// extent than the parse reads is the fixed-arena streaming shape, not a defect. Every span the
/// parse produces lies inside the parse extent, which is a subset of the sink's, so every text
/// slice is correct. The trailing bytes are merely uncovered — and that is a question `finish`
/// already owns and answers, by name.
///
/// This is why the extent conjunct is an **ordering and not an equality**: one representation
/// must not stand for two cases, and these two cases have opposite answers. The cell asserts
/// both halves — the seam lets it through, and the tail is then reported rather than ignored.
///
/// Falsifying output: a panic at `as_ref`. That would mean the conjunct went in the wrong
/// direction and the streaming case is now refused at the door.
#[test]
fn sink_bound_to_a_longer_extent_than_the_parse_is_accepted_at_the_seam() {
  let buf = std::string::String::from("abc");
  let sink_src: &str = &buf[..3];
  let parse_src: &str = &buf[..2];

  let mut sink: SesSink<'_> = Sink::new(sink_src, Verbose::new(), profile());
  {
    let mut input =
      crate::input::Input::<MiniLexer<'_>, SesCtx<'_, '_>, ()>::with_state_and_context(
        parse_src,
        (),
        crate::input::InputContext::new(&mut sink, DefaultCache::<'_, MiniLexer<'_>>::default()),
      );
    // The seam accepts: a longer-bound sink is a capability, not a leak.
    let mut inp = input.as_ref();
    while let Ok(Some(_)) = inp.next() {}
  }

  // The uncovered tail is then `finish`'s business, and it names it rather than materializing a
  // tree whose text quietly exceeds what the parse saw. (`finish_partial` is the opt-in that
  // tiles it instead — the streaming caller's choice to make, not the seam's.)
  let (strict, _emitter) = sink.finish(K_ROOT);
  assert!(
    matches!(strict, Err(FinishError::UncoveredGap { start: 2, end: 3 })),
    "the unparsed tail is reported by name, not silently accepted: {strict:?}"
  );
}

/// The policy as a **table**, so it is one artifact rather than two assertions a reader has to
/// reassemble. `SourceIdentity::covers` is the named relation; the seam enforces it as two
/// separate asserts only so a failure says which half broke.
///
/// The asymmetry is the whole content: origin is an equality, extent is an ordering, and the
/// ordering runs one way. This has been got backwards once — the first ruling was
/// `sink_len <= parse_len` — so it is pinned in every direction rather than described.
///
/// Falsifying output: any row disagreeing. That means `covers` and the seam have drifted apart,
/// and the seam is then enforcing something the type does not document.
#[test]
fn source_identity_covers_is_the_stated_policy_in_every_direction() {
  use crate::source::SourceIdentity;

  let buf = std::string::String::from("abcd");
  let other = std::string::String::from("wxyz");

  let full = SourceIdentity::of(&buf[..4]);
  let prefix = SourceIdentity::of(&buf[..2]);
  let shifted = SourceIdentity::of(&buf[1..]);
  let foreign = SourceIdentity::of(&other[..4]);

  // Same origin, equal extent — the ordinary pairing.
  assert!(full.covers(full), "a source covers itself");

  // Same origin, sink LONGER than the parse — the fixed-arena streaming shape. Accepted.
  assert!(
    full.covers(prefix),
    "a sink bound to the whole buffer may serve a parse over a prefix: every span the parse \
     emits lies inside the sink's extent, and the unparsed tail is `finish`'s business"
  );

  // Same origin, sink SHORTER than the parse. Refused.
  assert!(
    !prefix.covers(full),
    "a sink bound to a prefix must NOT serve a longer parse: peeked bytes past its end can \
     shape a tree that will not contain them, with nothing downstream able to see it"
  );

  // A shifted view is a different origin, not a shorter extent — every offset means something
  // else there, so no extent relation can rescue it.
  assert!(
    !full.covers(shifted),
    "a shifted view is a different origin"
  );
  assert!(!shifted.covers(full), "…in both directions");

  // Different allocation entirely.
  assert!(!full.covers(foreign), "a foreign buffer is never covered");
  assert!(!foreign.covers(full), "…in both directions");

  // And the two halves really are the two halves.
  assert_eq!(full.addr(), prefix.addr(), "prefixes share an origin");
  assert_ne!(full.extent(), prefix.extent(), "…and differ in extent");
  assert_ne!(full.addr(), shifted.addr(), "a shifted view does not");
}

/// The extent projection on the **third** `REFERENT_IS_BYTES` backing. `str` and `[u8]` are
/// exercised by every cell above; `BStr` is not, and the extent half rests on it being
/// `#[repr(transparent)]` over `[u8]` so that `size_of_val` reads a byte length rather than a
/// struct size. That is a fact about someone else's type, so it is checked rather than assumed.
///
/// Falsifying output: a wrong extent. That would mean the seam compares a struct size against a
/// byte length for `BStr` sources, refusing or accepting on nonsense.
#[cfg(feature = "bstr_1")]
#[test]
fn bstr_projects_a_byte_extent_like_the_other_unsized_backings() {
  use crate::source::SourceIdentity;
  use bstr_1::ByteSlice;

  let buf = b"abcd";
  let full: &bstr_1::BStr = buf[..].as_bstr();
  let prefix: &bstr_1::BStr = buf[..2].as_bstr();

  assert_eq!(
    SourceIdentity::of(full).extent(),
    4,
    "BStr is repr(transparent) over [u8], so its extent is a byte length"
  );
  assert_eq!(SourceIdentity::of(prefix).extent(), 2);
  assert_eq!(
    SourceIdentity::of(full).addr(),
    SourceIdentity::of(prefix).addr(),
    "…and two BStr prefixes of one buffer still share an origin"
  );
  assert!(SourceIdentity::of(full).covers(SourceIdentity::of(prefix)));
  assert!(!SourceIdentity::of(prefix).covers(SourceIdentity::of(full)));
}

/// The `conformance::emitter` kit is the mitigation for the one residual the defaulted
/// `bound_source` leaves — a source-binding emitter whose author forgets to override it. Here
/// the kit is exercised in both directions on the crate's own types, so it is a tool that has
/// been shown to work rather than a tool that has been shipped.
#[test]
#[cfg(feature = "conformance")]
fn conformance_emitter_kit_accepts_the_sink_and_its_forwarder() {
  use crate::conformance::emitter::{assert_binds_source, assert_forwards_bound_source};

  let buf = std::string::String::from("ab");
  let src: &str = &buf;
  let mut sink: SesSink<'_> = Sink::new(src, Verbose::new(), profile());

  assert_binds_source::<MiniLexer<'_>, (), _>(&sink, src);

  let mut probe: SesSink<'_> = Sink::new(src, Verbose::new(), profile());
  let borrowed = &mut sink;
  assert_forwards_bound_source::<MiniLexer<'_>, (), _, _>(&borrowed, &probe);
  // Silence the unused-mut path: `probe` exists only as an independent `Some` to compare
  // against, and both sinks bind the same buffer.
  let _ = &mut probe;
}

/// The kit's own RED: an emitter that binds **no** source fails `assert_binds_source`, which
/// is precisely the state a source-binding author who forgot the override would be in.
#[test]
#[cfg(feature = "conformance")]
#[should_panic(expected = "inherits `Emitter::bound_source`'s `None` default")]
fn conformance_emitter_kit_refuses_an_emitter_that_binds_nothing() {
  use crate::conformance::emitter::assert_binds_source;

  let src: &str = "ab";
  let em: Verbose<SesErr> = Verbose::new();
  assert_binds_source::<MiniLexer<'_>, (), _>(&em, src);
}

/// The kit's second RED: a wrapper that does not forward is caught.
#[test]
#[cfg(feature = "conformance")]
#[should_panic(expected = "does not forward `bound_source`")]
fn conformance_emitter_kit_refuses_a_non_forwarding_wrapper() {
  use crate::conformance::emitter::assert_forwards_bound_source;

  let buf = std::string::String::from("ab");
  let src: &str = &buf;
  let mut sink: SesSink<'_> = Sink::new(src, Verbose::new(), profile());
  let inner_probe: SesSink<'_> = Sink::new(src, Verbose::new(), profile());
  let wrapper = NonForwardingWrapper(&mut sink);
  assert_forwards_bound_source::<MiniLexer<'_>, (), _, _>(&wrapper, &inner_probe);
}

/// **THE WRAPPER HAZARD, PINNED IN-CRATE (1 of 3 surviving cells).** A wrapper that forwards
/// every emission but hides `Emitter::bound_source` reports `None`, so the seam concludes "this
/// emitter binds no source" and accepts a pairing it would otherwise refuse.
///
/// A finish-time wall against this was built and **removed**: it caught this shape and three
/// separate bypasses were then found for it — pre-arming through the public `bound_source()`
/// accessor, a parse whose events are all diagnostics, and hand-emitted token spans. Each fix
/// relocated the hole rather than closing it, because the witness was flags on the sink and the
/// sink cannot tell who set them. A typed `FinishError` implying a protection with three
/// published bypasses is the `## Panics` fiction this release deleted nine of, in a different
/// medium.
///
/// **What 0.9.0 narrowed, and what it left alone.** The cell used to state the general form as
/// "a wrapper writing `impl CstEmitter for W {}` satisfies every CST bound while inheriting every
/// default". That sentence is now stale in the good direction and is reworded below: `cst_demote`
/// — the node bracket's failing exit — is a **required** method, so the empty impl no longer
/// compiles and the one default whose inheritance produced a *silently wrong tree* cannot be
/// inherited at all. The residue this cell pins is untouched by that, and the reason is worth
/// keeping next to it: `bound_source` lives on the **core** `Emitter` trait, where the defaults
/// are the promise diagnostics-only emitters were made, and its inheritance fails toward an
/// *absence* (the check does not run) rather than toward a wrong tree. Same grading, opposite
/// answer — see `CstEmitter::cst_demote`'s *Required, not defaulted*.
///
/// **The handle no longer hands out the sink; a callback parameter still does.** Wrapping a
/// sink needs a `Sink` value or a `&mut Sink`, and the *handle* routes are shut — `Sink::new`,
/// `InputRef::emitter` and `Input::emitter`/`into_emitter` are crate-private,
/// `ParseState::emitter` is gone, `Cst` yields only the inner emitter, and
/// `InputRef::emitter_ref` hands back `&Sink`, which cannot occupy an emitter slot because every
/// recording method takes `&mut self`. The compile-time pin of that is the `compile_fail` pair
/// on `cst::parse_lossless`. But `Decision::decide` and the token-level pratt folds still take
/// `&mut Ctx::Emitter` as a parameter, so this shape remains reachable from outside — see
/// `a_wrapper_around_the_live_sink_still_reaches_a_callback_parameter`, which mounts exactly this
/// wrapper through that door.
#[test]
fn a_non_forwarding_wrapper_is_not_caught() {
  let mut sink: SesSink<'_> = Sink::new("XY", Verbose::new(), profile());
  {
    let mut input =
      crate::input::Input::<MiniLexer<'_>, NonFwdCtx<'_, '_>, ()>::with_state_and_context(
        "ab",
        (),
        crate::input::InputContext::new(
          NonForwardingWrapper(&mut sink),
          DefaultCache::<'_, MiniLexer<'_>>::default(),
        ),
      );
    let mut inp = input.as_ref();
    while let Ok(Some(_)) = inp.next() {}
  }
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    text(green.expect("the wrapper hid the binding, so nothing refused the pairing")),
    "XY",
    "the residue: an emitter wrapper that does not forward `bound_source` gives up the \
     guarantee for whatever it wraps, and no bound can see the omission — a wrapper writing \
     `impl CstEmitter for W {{ fn cst_demote(..) {{}} }}`, the smallest impl that still \
     compiles, satisfies every CST bound while inheriting every default it is still allowed to"
  );
}

/// **THE WRAPPER HAZARD (2 of 3): a parse whose events are all diagnostics.**
///
/// No token is committed, so nothing token-shaped exists for a downstream check to key on —
/// while the lexer diagnostics' spans still license gap tokens at materialization. A
/// same-length foreign source therefore yields a fully covered tree over the sink's buffer,
/// from a parse that read a different one, with no token for the zero-token wall and no
/// uncovered byte for the gap wall.
///
/// This is the shape the other fixtures structurally could not produce: every one of them
/// commits at least one token.
///
/// The stream here builds **no structure**, which is why the zero-token wall was never in its
/// path even before the wall was qualified. Add a node and it was — pin 5 is that variant, and
/// records that it no longer is.
///
/// Reachable exactly as cell 1 is: the handle no longer yields the `&mut Sink` this shape wraps,
/// but a `*_while` condition's emitter parameter still does.
#[test]
fn an_all_diagnostic_parse_through_a_non_forwarding_wrapper_is_not_caught() {
  let mut sink: SesSink<'_> = Sink::new("XY", Verbose::new(), profile());
  {
    // Every byte of "!!" lexes as an error, so not one token is committed.
    let mut input =
      crate::input::Input::<MiniLexer<'_>, NonFwdCtx<'_, '_>, ()>::with_state_and_context(
        "!!",
        (),
        crate::input::InputContext::new(
          NonForwardingWrapper(&mut sink),
          DefaultCache::<'_, MiniLexer<'_>>::default(),
        ),
      );
    let mut inp = input.as_ref();
    while let Ok(Some(_)) = inp.next() {}
  }
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect(
    "today: a diagnostics-only parse arms no parse witness, so the wall stays silent and the \
     gap-licensed tree materializes over the sink's own buffer",
  );
  assert_eq!(
    text(green),
    "XY",
    "the residue, stated exactly: the parse read \"!!\" and the tree's text is the sink's \
     \"XY\" — equal length, fully gap-tiled, and nothing downstream can see it"
  );
}

/// **A PIN OF A KNOWN-OPEN RESIDUE: the binding can be *pre-armed*.**
/// `Emitter::bound_source` is a public trait method, and the handle forwards it
/// (`InputRef::emitter_bound_source`), so anything holding an emitter — or reaching one through
/// a parse — can query it. Any state a sink recorded from "I was asked" is therefore settable by
/// a caller who is not the parse entry, which is why a sink-side witness cannot encode *who*
/// asked. That is the reason the wrong-source class had to be closed by *construction* (the
/// source named once, the emitter never handed out) rather than by a flag.
///
/// Still open as stated — a query anyone can make — and narrower than it was: a consumer can no
/// longer reach a `Sink` to ask.
///
/// This cell records the reachability rather than a tree: the accessor answers for anyone.
#[test]
fn bound_source_is_publicly_queryable_so_no_sink_side_witness_can_encode_provenance() {
  use crate::Emitter;

  let buf = std::string::String::from("ab");
  let src: &str = &buf;
  let sink: SesSink<'_> = Sink::new(src, Verbose::new(), profile());

  // Anybody. No parse, no attachment, no seam.
  let answered = Emitter::<'_, MiniLexer<'_>, ()>::bound_source(&sink);
  assert!(
    answered.is_some(),
    "the binding is readable by any holder — so 'this sink was asked' is a fact about callers \
     in general, never about the parse entry in particular"
  );
}

/// **A RETIRED RESIDUE: hand-emitted token spans, closed by deleting the door.**
///
/// This was a pin of an open hole. `CstEmitter::cst_token` was a public raw transport carrying a
/// caller-chosen span and consuming nothing, so a grammar could push token events naming any
/// source bytes it liked while the auto-emission channel stayed silent — and the resulting log
/// is byte-identical to one a legal parse could produce, so nothing downstream could key on it.
/// Minting did not close it: the door was on the emitter trait, not on the sink's construction.
///
/// **The trait member is gone.** The token channel now has exactly one door — the defaulted
/// [`Emitter::commit_token`], which only the input layer's settle surfaces call — and the body
/// below is the sink's own crate-private recorder, reachable from nowhere else. A consumer
/// cannot write this test: `cst_token` does not exist, `record_token` is private, and
/// `commit_token` needs a `&mut Sink` no public route yields.
///
/// What survives here is the *shape* — a span in the log that arrived without a settle — because
/// that is what any future check would have to key on, and because it is still the sink's
/// behaviour when the crate's own recorder is driven directly.
#[test]
fn a_recorded_span_with_no_settle_behind_it_materializes_like_any_other() {
  let mut sink = verbose_sink("ab");
  // No parse, no `commit_token`: just spans, chosen by the caller.
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  let (green, _emitter) = sink.finish(K_ROOT);
  assert_eq!(
    text(green.expect("hand-emitted spans materialize like any other")),
    "ab",
    "the shape, retained: a Token event is a span, and materialization asks nothing about how \
     it got there — which is why the only door into it is the input layer's settle hook"
  );
}

/// **THE WRAPPER HAZARD (3 of 3): cell 2's shape with a node in it — and the widening was
/// the qualifying commit's.**
///
/// The bypass is cell 2's exactly: a wrapper that hides `bound_source`, a parse over a foreign
/// buffer of equal length, every event a diagnostic. The one addition is the one a **lossless**
/// grammar always makes — it opens its document node before it can know whether a token follows
/// — and it is forwarded through the same wrapper as everything else, because a wrapper that
/// did not forward the structuring surface could not carry a CST parse at all.
///
/// **The delta, stated plainly.** Up to this commit — on `main` before it — this variant was
/// refused: the unqualified zero-token wall read `saw_structure && !saw_token && source_len > 0`
/// and answered `StructureWithoutTokens`, so this pairing did not materialize while pin 2's
/// structureless one did. That refusal was
/// **incidental, not a protection**: the wall's witness is "no committed token arrived", which
/// is a fact about the emitter chain and says nothing about *which buffer* the parse read — it
/// refused this shape and the honest all-error parse of an unlexable source in the same breath,
/// with the same message, and could not tell them apart. Qualifying it with
/// `first_uncovered_gap.is_some()` gives up the accident and keeps every stream the wall was
/// built for: here the diagnostics explain every byte, so the wall stays silent and the tree
/// materializes as the forwarded node holding one gap tile over the sink's own bytes.
///
/// Nothing was traded to buy that. A wall keyed on the absence of a token could never have
/// separated a foreign pairing from a native one, which is the same reason the finish-time wall
/// pin 1 describes was built and then removed: the witness was flags on the sink, and the sink
/// cannot tell who set them.
///
/// Recorded as an acceptance the crate does **not** want — this cell asserts what materializes
/// today, not a refusal anyone should read as a guarantee.
///
/// Reachable exactly as cells 1 and 2 are: shut on the handle, open on a callback parameter.
#[test]
fn a_structured_all_diagnostic_parse_through_a_non_forwarding_wrapper_is_not_caught() {
  let mut sink: SesSink<'_> = Sink::new("XY", Verbose::new(), profile());
  {
    // Every byte of "!!" lexes as an error, so not one token is committed.
    let mut input =
      crate::input::Input::<MiniLexer<'_>, NonFwdCtx<'_, '_>, ()>::with_state_and_context(
        "!!",
        (),
        crate::input::InputContext::new(
          NonForwardingWrapper(&mut sink),
          DefaultCache::<'_, MiniLexer<'_>>::default(),
        ),
      );
    let mut inp = input.as_ref();
    // The lossless root: opened before the first lex, closed after the last, through the
    // wrapper — the shape every lossless grammar produces, and what the unqualified wall
    // refused.
    inp.emitter().cst_start(K_NODE);
    while let Ok(Some(_)) = inp.next() {}
    inp.emitter().cst_finish(K_NODE);
  }
  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect(
    "today: the diagnostics explain every byte, so the qualified zero-token wall stays silent \
     and the gap-licensed tree materializes — the unqualified wall answered \
     StructureWithoutTokens here",
  );
  let root = tree(green.clone());
  let node = root
    .first_child()
    .expect("the node the wrapper forwarded survives");
  assert_eq!(node.kind(), K_NODE);
  assert_eq!(
    shape(&green),
    r#"Root[Node[] Gap"XY"]"#,
    "no token ever settled at all, so the run trails nothing and tiles beside the forwarded \
     node rather than inside it"
  );
  assert_eq!(
    text(green),
    "XY",
    "the residue as this commit widened it: the parse read \"!!\" and a *structured* tree's \
     text is the sink's \"XY\" — the shape the unqualified wall refused, accepted now, and by \
     accident either way"
  );
}

/// **THE WRAPPER HAZARD, THE OTHER HALF: the forward that is load-bearing.**
///
/// The three cells above pin what a wrapper's *omission* costs. This one pins the opposite
/// direction — that `NonForwardingWrapper`'s `cst_demote` line does something — and it exists
/// because nothing in the tree drove the node bracket's **failing** exit through a wrapper at
/// all. Pin 5 drives `cst_start`/`cst_finish` only, and every other demote exercise is
/// direct-on-sink, through `ParseState`, or through the view/handle forwarding surfaces — none
/// interposes a consumer-shaped wrapper. So flipping that fixture's `cst_demote` body to a
/// discard left the whole lib suite green, and the in-crate model of the wrapper hazard was
/// unexercised on the one method whose loss is *silent*.
///
/// **Why demote alone earns a cell.** [`CstEmitter::cst_demote`]'s *Required, not defaulted*
/// grades the five by failure direction: the four defaulted methods fail toward a loud, typed
/// absence, and demote fails toward a silent **presence** — a swallowed demote leaves the
/// `StartNode` open, and a sink cannot tell a swallowed demote from a legal panic residue.
/// Requiredness (0.9.0) closed the *inherited* discard; a discard written deliberately still
/// compiles, and that is the population this cell is against.
///
/// **Its falsifying edit, which is the acceptance it landed under.** Empty the forward at
/// `impl CstEmitter for NonForwardingWrapper` and this cell goes red loudly and typed — strict
/// `finish` answers `Err(FinishError::UnclosedNodes { open: 1 })`, because the demote never
/// reached the sink. Restore it and the cell greens. A check that has only ever been green over
/// a correct wrapper proves nothing, so the flip was run in both directions before this landed.
///
/// The pairing is the matching-source control's, not the residue cells' — one buffer on both
/// sides — so what is measured here is the forward and nothing else.
#[test]
fn a_wrapper_forwards_the_node_brackets_failing_exit() {
  let buf = std::string::String::from("a");
  let src: &str = &buf;
  let mut sink: SesSink<'_> = Sink::new(src, Verbose::new(), profile());
  {
    let mut input =
      crate::input::Input::<MiniLexer<'_>, NonFwdCtx<'_, '_>, ()>::with_state_and_context(
        src,
        (),
        crate::input::InputContext::new(
          NonForwardingWrapper(&mut sink),
          DefaultCache::<'_, MiniLexer<'_>>::default(),
        ),
      );
    let mut inp = input.as_ref();
    // The up-front bracket, opened through the wrapper and abandoned through it. The handback
    // is the *inner sink's* mark — a wrapper can mint none of its own — so only the forward
    // returns it to the buffer that issued it.
    let mark = inp.emitter().cst_start(K_NODE);
    while let Ok(Some(_)) = inp.next() {}
    inp.emitter().cst_demote(mark, K_NODE);
  }

  let (green, _emitter) = sink.finish(K_ROOT);
  let green = green.expect(
    "the demote arrived, so the buffer is balanced and STRICT finish materializes — a wrapper \
     that discarded it answers UnclosedNodes here instead, which is this cell's red",
  );
  assert_eq!(
    shape(&green),
    r#"Root[Tok"a"]"#,
    "`demote_materialises_as_inert`'s outcome, reached through a wrapper: canonicalization \
     tombstones the slot, so the abandoned node materializes into nothing and the committed \
     token survives loose beside it"
  );
  assert_eq!(text(green), "a", "the demote costs the tree no text");
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
  sink.record_token(&MiniTok(b'a'), &span(0, 1));

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

// ── The event log's width, and the capacity reserved for it ────────────────────

/// One event is **32 bytes** for a `SimpleSpan` lexer, and nothing may quietly change that.
///
/// The event log is the largest thing a materialization touches — a 57.7 KB GraphQL document
/// records 64,085 of these, appended once and walked once — so the element's width is a
/// performance property of the whole sink rather than an implementation detail, and it is the
/// constant the construction-time reservation is sized against. A variant that widened the
/// element would multiply both the log's memory traffic and that reservation, and no other
/// cell in this suite would notice.
#[test]
fn one_event_is_thirty_two_bytes() {
  assert_eq!(
    core::mem::size_of::<Event<SimpleSpan>>(),
    32,
    "the event width moved: a variant was added or widened, and the log's memory traffic \
     moved with it"
  );
}

/// The construction-time reservation is **the source's length rounded up to a power of two,
/// capped** — the capacity the `Vec`'s own doubling would have reached, bought in one step.
///
/// The rounding is the load-bearing half and it is pinned as a *measured* fact, not a
/// preference: reserving the raw length instead under-reserves a lossless log (1.11 events per
/// byte on the reference document), so the log overruns the reservation and pays a
/// double-sized reallocation on top of a large eager one — measured **slower than reserving
/// nothing**. A power of two is exactly the block the growth would have ended on.
///
/// The cap is the safety half: past it the byte count stops being evidence about the event
/// count, and a grammar whose tokens are long must not reserve gigabytes.
///
/// Falsified by: a reservation that is not a power of two (the trap above), one that scales
/// past the cap, or one that allocates for an empty source.
#[test]
fn the_event_log_reserves_the_doubling_chains_own_capacity() {
  let empty = verbose_sink("");
  assert_eq!(
    empty.events_capacity(),
    0,
    "an empty source must not allocate an event log at all"
  );

  let small = "a".repeat(210);
  let small_sink = verbose_sink(&small);
  assert_eq!(
    small_sink.events_capacity(),
    256,
    "a 210-byte source reserves 256 events — the capacity doubling would have reached, and \
     not one allocation more"
  );

  // The reference document's ratio, restated as the property that matters: the reservation
  // must be at or above the event count, or it buys a big allocation AND a reallocation.
  let alias_len = 57_741usize;
  assert!(
    super::event_capacity_for(alias_len) >= 64_085,
    "a source of {alias_len} bytes reserves {} events, below the 64,085 a lossless parse of \
     it records: the log would overrun its own reservation",
    super::event_capacity_for(alias_len)
  );

  let big = "a".repeat(super::EVENT_CAPACITY_CAP * 2);
  let big_sink = verbose_sink(&big);
  assert_eq!(
    big_sink.events_capacity(),
    super::EVENT_CAPACITY_CAP,
    "past the cap the reservation stops: the byte count is no longer evidence about the \
     event count, and a grammar with long tokens must not reserve gigabytes"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The partial-forwarding hole: structuring forwarded, token channel severed
// ═══════════════════════════════════════════════════════════════════════════════
//
// These two cells were `tests/parser_node.rs` integration tests, driven with a wrapper in the
// context seat of the public parse entry. They live in-crate now, because `Sink::new` is
// crate-private and the lossless drivers pin `Ctx::Emitter` to `Sink` **by name** — so a
// wrapper can no longer occupy that seat from outside at all. What they pin is unchanged and
// still worth pinning: the materialization walls that catch a severed token channel, whichever
// route produced one.

/// The half-forwarding wrapper: the generic emitter wrapper a downstream author writes by
/// forwarding every **required** [`Emitter`] method plus the backtracking trio (the compiler
/// and the parse demand those) and the whole [`CstEmitter`] structuring surface (the `node()`
/// bound demands that) — while inheriting the defaulted no-op [`Emitter::commit_token`], which
/// severs the auto-emission token channel even though every structuring event still flows.
///
/// `bound_source` **is** forwarded, so this fixture models a wrapper that drops *tokens* and
/// nothing else. One fixture, one defect: omitting it would additionally make this a wrapper
/// that hides its inner sink's bound source, which is a separate (and open) residue — pinned
/// on its own by `a_non_forwarding_wrapper_is_not_caught` above.
struct HalfForward<'a, 'inp>(&'a mut SesSink<'inp>);

type HfCtx<'inp, 'e> = (HalfForward<'e, 'inp>, DefaultCache<'inp, MiniLexer<'inp>>);
type HfIr<'inp, 'e, 'c> = crate::InputRef<'inp, 'c, MiniLexer<'inp>, HfCtx<'inp, 'e>, ()>;

impl<'inp> crate::Emitter<'inp, MiniLexer<'inp>> for HalfForward<'_, 'inp> {
  type Error = SesErr;

  fn bound_source(&self) -> Option<crate::source::SourceIdentity> {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::bound_source(&*self.0)
  }

  fn emit_lexer_error(
    &mut self,
    err: Spanned<MiniLexErr<'inp>, MiniSpan<'inp>>,
  ) -> Result<(), Self::Error> {
    self.0.emit_lexer_error(err)
  }

  // Forwarded like `commit_token`: the input layer's own refusals must reach the sink AS
  // refusals, or the coverage evidence they carry is lost and a legitimately unlexable region
  // stops being explained.
  fn commit_lexer_error(
    &mut self,
    err: Spanned<MiniLexErr<'inp>, MiniSpan<'inp>>,
  ) -> Result<(), Self::Error> {
    self.0.commit_lexer_error(err)
  }

  fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, MiniLexer<'inp>, ()>,
  ) -> Result<(), Self::Error> {
    self.0.emit_unexpected_token(err)
  }

  fn emit_error(&mut self, err: Spanned<Self::Error, MiniSpan<'inp>>) -> Result<(), Self::Error> {
    self.0.emit_error(err)
  }

  fn checkpoint(&mut self) -> u64 {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::checkpoint(&mut *self.0)
  }

  fn rewind(&mut self, cursor: &Cursor<'inp, '_, MiniLexer<'inp>>, ckp: u64) {
    self.0.rewind(cursor, ckp)
  }

  fn release(&mut self, ckp: u64) {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::release(&mut *self.0, ckp)
  }

  // `commit_token` is DELIBERATELY not forwarded: the defaulted no-op is the sever.
}

impl<'inp> CstEmitter<'inp, MiniLexer<'inp>> for HalfForward<'_, 'inp> {
  fn cst_start(&mut self, kind: u16) -> EventMark {
    self.0.cst_start(kind)
  }

  fn cst_finish(&mut self, kind: u16) {
    self.0.cst_finish(kind);
  }

  fn cst_demote(&mut self, mark: EventMark, kind: u16) {
    self.0.cst_demote(mark, kind);
  }

  fn cst_mark(&mut self) -> EventMark {
    self.0.cst_mark()
  }

  fn cst_start_at(&mut self, mark: EventMark, kind: u16) {
    self.0.cst_start_at(mark, kind);
  }
}

/// The partial-forwarding sibling of [`HalfForward`]: it forwards the structuring surface and
/// the **first** committed token, then severs the channel. Because a token *does* survive, the
/// zero-token wall ([`FinishError::StructureWithoutTokens`]) cannot fire — the dropped tokens
/// vanish as uncovered source bytes instead, the signature only the gap-coverage law
/// ([`FinishError::UncoveredGap`]) catches.
struct HalfForwardPartial<'a, 'inp> {
  inner: &'a mut SesSink<'inp>,
  forwarded: usize,
}

type HfpCtx<'inp, 'e> = (
  HalfForwardPartial<'e, 'inp>,
  DefaultCache<'inp, MiniLexer<'inp>>,
);
type HfpIr<'inp, 'e, 'c> = crate::InputRef<'inp, 'c, MiniLexer<'inp>, HfpCtx<'inp, 'e>, ()>;

impl<'inp> crate::Emitter<'inp, MiniLexer<'inp>> for HalfForwardPartial<'_, 'inp> {
  type Error = SesErr;

  fn bound_source(&self) -> Option<crate::source::SourceIdentity> {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::bound_source(&*self.inner)
  }

  fn emit_lexer_error(
    &mut self,
    err: Spanned<MiniLexErr<'inp>, MiniSpan<'inp>>,
  ) -> Result<(), Self::Error> {
    self.inner.emit_lexer_error(err)
  }

  // Forwarded like `commit_token`: the input layer's own refusals must reach the sink AS
  // refusals, or the coverage evidence they carry is lost and a legitimately unlexable region
  // stops being explained.
  fn commit_lexer_error(
    &mut self,
    err: Spanned<MiniLexErr<'inp>, MiniSpan<'inp>>,
  ) -> Result<(), Self::Error> {
    self.inner.commit_lexer_error(err)
  }

  fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, MiniLexer<'inp>, ()>,
  ) -> Result<(), Self::Error> {
    self.inner.emit_unexpected_token(err)
  }

  fn emit_error(&mut self, err: Spanned<Self::Error, MiniSpan<'inp>>) -> Result<(), Self::Error> {
    self.inner.emit_error(err)
  }

  fn checkpoint(&mut self) -> u64 {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::checkpoint(&mut *self.inner)
  }

  fn rewind(&mut self, cursor: &Cursor<'inp, '_, MiniLexer<'inp>>, ckp: u64) {
    self.inner.rewind(cursor, ckp)
  }

  fn release(&mut self, ckp: u64) {
    crate::Emitter::<'inp, MiniLexer<'inp>, ()>::release(&mut *self.inner, ckp)
  }

  // The partial sever: the first committed token flows, every later one is dropped — so a
  // token survives (the zero-token wall stays silent) but its peers become uncovered bytes.
  fn commit_token(&mut self, tok: &MiniToken<'inp>, span: &MiniSpan<'inp>) {
    if self.forwarded < 1 {
      self.forwarded += 1;
      self.inner.commit_token(tok, span);
    }
  }
}

impl<'inp> CstEmitter<'inp, MiniLexer<'inp>> for HalfForwardPartial<'_, 'inp> {
  fn cst_start(&mut self, kind: u16) -> EventMark {
    self.inner.cst_start(kind)
  }

  fn cst_finish(&mut self, kind: u16) {
    self.inner.cst_finish(kind);
  }

  fn cst_demote(&mut self, mark: EventMark, kind: u16) {
    self.inner.cst_demote(mark, kind);
  }

  fn cst_mark(&mut self) -> EventMark {
    self.inner.cst_mark()
  }

  fn cst_start_at(&mut self, mark: EventMark, kind: u16) {
    self.inner.cst_start_at(mark, kind);
  }
}

/// A wrapper that forwards the structuring surface but not the committed-token hook must not
/// produce a *silently plausible* materialization. The parse succeeds and records structure,
/// every committed token is dropped between the input layer and the sink, and `finish` — the
/// success door — refuses with a typed error instead of returning a gap-tiled tree with empty
/// nodes.
#[test]
fn half_forwarding_wrapper_is_refused_at_finish() {
  let mut sink: SesSink<'_> = Sink::new("ab", Verbose::new(), profile());
  {
    let mut input = crate::input::Input::<MiniLexer<'_>, HfCtx<'_, '_>, ()>::with_state_and_context(
      "ab",
      (),
      crate::input::InputContext::new(
        HalfForward(&mut sink),
        DefaultCache::<'_, MiniLexer<'_>>::default(),
      ),
    );
    let mut inp = input.as_ref();
    use crate::ParseInput as _;
    let res = crate::parser::node(K_NODE, |inp: &mut HfIr<'_, '_, '_>| {
      for _ in 0..2 {
        assert!(inp.next()?.is_some());
      }
      Ok(())
    })
    .parse_input(&mut inp);
    assert_eq!(res, Ok(()), "the parse itself succeeds — that is the trap");
  }

  let (green, _emitter) = sink.finish(K_ROOT);
  let err = match green {
    Err(err) => err,
    Ok(tree_ok) => panic!(
      "finish must refuse the severed token channel, got a plausible tree: {:?}",
      text(tree_ok)
    ),
  };
  assert!(
    matches!(err, FinishError::StructureWithoutTokens),
    "expected StructureWithoutTokens, got {err:?}"
  );
}

/// A wrapper that forwards structure and only *some* committed tokens leaves the survivors in
/// the tree and the dropped ones as uncovered source bytes. The zero-token wall can no longer
/// speak (a token survived), so `finish` — the success door — must catch the loss through the
/// gap-coverage law: `UncoveredGap` over exactly the dropped token's bytes, not a plausible
/// gap-tiled tree.
#[test]
fn partial_forwarding_wrapper_is_refused_at_finish() {
  let mut sink: SesSink<'_> = Sink::new("ab", Verbose::new(), profile());
  {
    let mut input =
      crate::input::Input::<MiniLexer<'_>, HfpCtx<'_, '_>, ()>::with_state_and_context(
        "ab",
        (),
        crate::input::InputContext::new(
          HalfForwardPartial {
            inner: &mut sink,
            forwarded: 0,
          },
          DefaultCache::<'_, MiniLexer<'_>>::default(),
        ),
      );
    let mut inp = input.as_ref();
    use crate::ParseInput as _;
    let res = crate::parser::node(K_NODE, |inp: &mut HfpIr<'_, '_, '_>| {
      for _ in 0..2 {
        assert!(inp.next()?.is_some());
      }
      Ok(())
    })
    .parse_input(&mut inp);
    assert_eq!(res, Ok(()), "the parse itself succeeds — that is the trap");
  }

  let (green, _emitter) = sink.finish(K_ROOT);
  let err = match green {
    Err(err) => err,
    Ok(tree_ok) => panic!(
      "finish must refuse the dropped token, got a plausible tree: {:?}",
      text(tree_ok)
    ),
  };
  assert!(
    matches!(err, FinishError::UncoveredGap { start: 1, end: 2 }),
    "expected UncoveredGap {{ start: 1, end: 2 }} over the dropped 'b', got {err:?}"
  );
}

// ═══════════════════════════════════════════════════════════════════════════════
// The lossless drivers: the sink is minted from the source the parse reads
// ═══════════════════════════════════════════════════════════════════════════════

type DrvCtx<'inp> = (SesSink<'inp>, DefaultCache<'inp, MiniLexer<'inp>>);
type DrvIr<'inp, 'c, Cmpl> = crate::InputRef<'inp, 'c, MiniLexer<'inp>, DrvCtx<'inp>, (), Cmpl>;

/// Drains every token, in either completeness mode.
fn drv_drain<'inp, Cmpl>(inp: &mut DrvIr<'inp, '_, Cmpl>) -> Result<usize, SesErr>
where
  Cmpl: crate::SurfaceIncomplete<'inp, MiniLexer<'inp>, DrvCtx<'inp>, ()>,
{
  let mut n = 0;
  while inp.next()?.is_some() {
    n += 1;
  }
  Ok(n)
}

/// The driver's whole point: one `src` argument feeds both the sink and the input, so the
/// buffer the tree's text is sliced out of and the buffer the parse read are the same bytes.
/// There is no second name to get wrong.
#[test]
fn parse_lossless_mints_the_sink_from_the_source_it_parses() {
  let src = "a b";
  let (cst, parsed) = crate::cst::parse_lossless(
    src,
    (),
    Verbose::<SesErr>::new(),
    profile(),
    DefaultCache::<'_, MiniLexer<'_>>::default(),
    drv_drain::<crate::Complete>,
  );
  assert_eq!(parsed, Ok(3));

  let (green, emitter) = cst.finish(K_ROOT);
  assert_eq!(text(green.expect("a covered parse materializes")), src);
  assert!(emitter.errors().is_empty());
}

/// The partial sibling honours `is_final` exactly as `parse_partial` does: non-final, running
/// off the end of the buffer is the frontier, not an end of input.
#[test]
fn parse_lossless_partial_surfaces_incomplete_while_non_final() {
  let (cst, parsed) = crate::cst::parse_lossless_partial(
    "ab",
    (),
    Verbose::<SesErr>::new(),
    profile(),
    DefaultCache::<'_, MiniLexer<'_>>::default(),
    false,
    drv_drain::<crate::Partial>,
  );
  assert!(
    matches!(parsed, Err(SesErr::Incomplete(_))),
    "non-final: the end of the buffer is the frontier, got {parsed:?}"
  );
  // The events recorded so far are this attempt's, not a continuation a later call extends;
  // the tooling door still round-trips over the bytes this attempt saw.
  let (green, _emitter) = cst.finish_partial(K_ROOT);
  assert_eq!(
    text(green.expect("finish_partial tiles what is missing")),
    "ab"
  );
}

/// Sealed, the same drive is an ordinary complete parse.
#[test]
fn parse_lossless_partial_completes_when_final() {
  let (cst, parsed) = crate::cst::parse_lossless_partial(
    "ab",
    (),
    Verbose::<SesErr>::new(),
    profile(),
    DefaultCache::<'_, MiniLexer<'_>>::default(),
    true,
    drv_drain::<crate::Partial>,
  );
  assert_eq!(parsed, Ok(2));
  let (green, _emitter) = cst.finish(K_ROOT);
  assert_eq!(
    text(green.expect("a sealed, covered parse materializes")),
    "ab"
  );
}

/// The retry lifecycle the `Cst::finish` abort-semantics note prescribes, executed: an
/// `Incomplete` attempt's handle is **dropped**, and the next attempt re-drives the enlarged
/// slice from byte zero into a fresh input and a fresh sink.
///
/// This is the shape the documentation used to contradict — it called the buffered events "the
/// resumable state", which invited keeping the handle and finishing it once more input arrived.
/// No operation on the handle does that, so the executable statement of the lifecycle is that
/// discarding and re-driving reaches a tree identical to the one-shot parse of the same bytes.
/// The prefix is re-lexed and re-recorded every attempt, which is the Θ(Σ attempt lengths) cost
/// `parse_lossless_partial` documents and the reason #251 wants a bounded owner for it.
#[test]
fn an_incomplete_lossless_attempt_is_discarded_and_redriven_over_the_larger_slice() {
  // Two refills, each a whole-buffer attempt over a prefix that is not yet final. Every handle
  // is dropped at the end of its iteration: none of them carries into the next.
  for prefix in ["a", "ab"] {
    let (cst, parsed) = crate::cst::parse_lossless_partial(
      prefix,
      (),
      Verbose::<SesErr>::new(),
      profile(),
      DefaultCache::<'_, MiniLexer<'_>>::default(),
      false,
      drv_drain::<crate::Partial>,
    );
    assert!(
      matches!(parsed, Err(SesErr::Incomplete(_))),
      "non-final {prefix:?}: the end of the buffer is the frontier, got {parsed:?}"
    );
    // Attempt-local, and readable while the handle lives: this is what an incomplete handle is
    // for, in place of the resume it never had.
    assert_eq!(cst.resource_trips(), 0);
    assert!(cst.inner_ref().errors().is_empty());
  }

  // The final attempt re-lexes the whole buffer from zero, and its tree is the one a caller
  // would have got from a single complete parse of the same bytes.
  let (cst, parsed) = crate::cst::parse_lossless_partial(
    "ab",
    (),
    Verbose::<SesErr>::new(),
    profile(),
    DefaultCache::<'_, MiniLexer<'_>>::default(),
    true,
    drv_drain::<crate::Partial>,
  );
  assert_eq!(parsed, Ok(2));
  let (redriven, _emitter) = cst.finish(K_ROOT);

  let (one_shot, parsed) = crate::cst::parse_lossless(
    "ab",
    (),
    Verbose::<SesErr>::new(),
    profile(),
    DefaultCache::<'_, MiniLexer<'_>>::default(),
    drv_drain::<crate::Complete>,
  );
  assert_eq!(parsed, Ok(2));
  let (one_shot, _emitter) = one_shot.finish(K_ROOT);

  assert_eq!(
    redriven.expect("the sealed final attempt materializes"),
    one_shot.expect("the one-shot parse materializes"),
  );
}

/// The trivia policy moved from the sink's builder to the handle's, because it is read once,
/// by materialization, and never during the parse. Setting it on the handle reaches the walk.
#[test]
fn cst_carries_the_trivia_policy_to_materialization() {
  let (cst, parsed) = crate::cst::parse_lossless(
    "a b",
    (),
    Verbose::<SesErr>::new(),
    profile(),
    DefaultCache::<'_, MiniLexer<'_>>::default(),
    drv_drain::<crate::Complete>,
  );
  assert_eq!(parsed, Ok(3));

  let cst = cst.with_trivia_policy(TriviaPolicy::AsEmitted);
  assert_eq!(cst.trivia_policy(), TriviaPolicy::AsEmitted);
  assert_eq!(cst.error_kind(), K_ERR);
  assert_eq!(cst.gap_kind(), K_GAP);
  assert!(cst.inner_ref().errors().is_empty());

  let (green, _emitter) = cst.finish(K_ROOT);
  assert_eq!(
    text(green.expect("AsEmitted is the replay the walk performs")),
    "a b"
  );
}

// RESIDUE 6 IS CLOSED, AND ITS PIN LIVES WHERE IT CAN HOLD.
//
// Both halves of it — `InputRef::emitter()` and the callback parameters — were once pinned by
// in-crate cells here, and neither could be. This file is *inside* the crate, where
// `InputRef::emitter()` is still callable and `Sink::new` is still reachable, so a cell mounted
// here goes on compiling no matter what the public surface says. A wall that only outsiders hit
// has to be pinned from outside.
//
// Their replacement is the `compile_fail` family in `cst::parse_lossless`'s "The sink is not
// handed out by the handle" section, compiled as its own crate against the published API:
//
// - the handle route — the same wrapper, the same un-pinned entry, the same foreign buffer,
//   refused at `inp.emitter()`;
// - the callback route — the ex-cell
//   `a_wrapper_around_the_live_sink_still_reaches_a_callback_parameter` transcribed verbatim,
//   refused because a `*_while` condition's second parameter is no longer `&mut Sink`;
// - the callback route respelled — the same attack with the parameter written as the
//   `EmitterView` the trait now demands, refused because a view implements no emitter trait and
//   so cannot be wrapped into a context. That one is the wall itself; the first two are the
//   shapes it stops.
//
// A runnable cell beside them shows what a condition may still do — report inline, structure
// inline, and stop — because what it is handed is the emitter's whole emitting surface under the
// emitter's own names.
//
// What is NOT closed is stated there too, under "What this does not close": a downstream crate
// can implement `Emitter` for `EmitterView<'_, '_, ItsOwnLexer, Sink<..>>` within the orphan
// rules. What such an implementation cannot reach is either producer of this sink's coverage
// machinery: `commit_token` (a token is what pairs a span with a byte) and `commit_lexer_error`
// (a recorded refusal span is what licenses a byte to have no token). Neither is on the view's
// surface. The second one was found open a round after the first was shut, and its cross-crate
// pin is `an_orphan_view_wrapper_carries_a_foreign_lexer_error_but_licenses_no_gap` in
// `tests/parser_node.rs`, with the event-level statement at
// `only_the_input_layers_lexer_error_carries_a_coverage_span` above.

// ═══════════════════════════════════════════════════════════════════════════════
// E5 — the maintained open-node depth (al8n/tokora#253) and the journal theorem (#250)
// ═══════════════════════════════════════════════════════════════════════════════

/// The maintained scalars against their oracles, after every step of whatever the caller just
/// did.
///
/// E6 (the `Diag`-tail start) rides E5's coverage deliberately: the two are written by the same
/// one append door and restored by the same rule off the same row, so the paths that can drift
/// one are the paths that can drift the other — and every caller of this helper already walks
/// all of them (nesting, tombstones, retro-wraps, demotes, tokens, diagnostics, hole wraps,
/// checkpoint/release, and all four rewind arms).
#[track_caller]
fn depth_matches_oracle(sink: &VerboseSink<'_>, at: &str) {
  assert_eq!(
    sink.depth(),
    sink.recount_depth(),
    "the maintained depth drifted from a full recount after {at}"
  );
  assert_eq!(
    sink.diag_tail,
    sink.recount_diag_tail(),
    "the maintained Diag-tail start drifted from a full recount after {at}"
  );
  // E7/E8 the same way, and this one carries more than drift detection: the demote wall's
  // verdict is now the chain's, so this equality IS the equivalence the wall rests on, checked
  // against a from-scratch replay on every shape these callers build.
  #[cfg(debug_assertions)]
  assert_eq!(
    sink.open_chain(),
    sink.recount_open_chain(),
    "the maintained open-node chain drifted from a full replay after {at}"
  );
}

/// DEPTH_CENSUS — the source lock behind E5's exactness claim. The scalar cannot drift by
/// omission only while **one** site appends to the log and **one** site splices it, each
/// charging the event's own `depth_delta`. A second `events.push` is exactly the shape that
/// makes a maintained counter wrong, and it is invisible to every output-tree test.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn depth_census_one_append_door_and_one_splice() {
  let src = include_str!("../sink.rs");

  assert_eq!(
    count(src, "self.events.push("),
    1,
    "DEPTH_CENSUS drift: the event log has exactly ONE append door (`push_event`), which is \
     where E5 is charged. A second push appends an event whose depth_delta nobody counted."
  );
  assert_eq!(
    count(src, "fn push_event"),
    1,
    "DEPTH_CENSUS drift: the one append door must be defined exactly once"
  );
  assert_eq!(
    count(src, "self.events.insert("),
    1,
    "DEPTH_CENSUS drift: the hole wrap's prefix-preserving splice is the ONE log mutation \
     that is not an append, and it charges its own half of the wrap"
  );
  assert_eq!(
    count(src, "self.depth +="),
    2,
    "DEPTH_CENSUS drift: E5 is charged in exactly two places — the append door and the hole \
     wrap's spliced StartNode"
  );
  assert_eq!(
    count(src, "self.depth = "),
    1,
    "DEPTH_CENSUS drift: E5 is RESTORED in exactly one place — the truncating arm of rewind, \
     from the spent row's frozen depth"
  );
  // #250: the deleted fix-up must stay deleted. A journal walk here is Theta(J) per hole and
  // provably writes nothing (see the theorem at the splice).
  assert_eq!(
    count(src, "for entry in &mut self.journal"),
    0,
    "al8n/tokora#250 regressed: wrap_hole is walking the undo journal again. Every entry's \
     StartAt sits strictly below the splice point, so the walk is Theta(J x H) of provably \
     dead work — the O(1) newest-entry assert is what pins that."
  );
}

/// Every write and restore path for E5, checked against the full recount after each step:
/// nesting, tombstones, retro-wraps, demotes, tokens, diagnostics, and a hole wrap.
#[test]
fn depth_tracks_the_recount_across_every_emission_shape() {
  let mut sink = verbose_sink("abcd");
  depth_matches_oracle(&sink, "construction");
  assert_eq!(sink.depth(), 0, "an empty log has no open node");

  sink.cst_start(K_LIST);
  depth_matches_oracle(&sink, "cst_start");
  assert_eq!(sink.depth(), 1);

  let tomb = sink.cst_mark();
  depth_matches_oracle(&sink, "cst_mark (an inert tombstone: delta 0)");
  assert_eq!(
    sink.depth(),
    1,
    "a tombstone opens nothing until it is spent"
  );

  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  depth_matches_oracle(&sink, "record_token");

  emit_error(&mut sink, 0, 1);
  depth_matches_oracle(&sink, "a forwarded diagnostic");

  sink.cst_start_at(tomb, K_WRAP);
  depth_matches_oracle(&sink, "cst_start_at (a retro-wrap: delta +1)");
  assert_eq!(sink.depth(), 2);
  sink.cst_finish(K_WRAP);
  depth_matches_oracle(&sink, "the retro-wrap's finish");
  assert_eq!(sink.depth(), 1);

  let start = sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  sink.cst_demote(start, K_NODE);
  depth_matches_oracle(&sink, "cst_demote (the failing exit: delta -1)");
  assert_eq!(sink.depth(), 1, "Start(+1) … Demote(-1) nets zero");

  sink.record_token(&MiniTok(b'c'), &span(2, 3));
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(2, 3), 1)
    .expect("verbose emitters collect");
  depth_matches_oracle(&sink, "a hole wrap (Start + Finish: net zero)");
  assert_eq!(sink.depth(), 1, "the hole wrap is depth-neutral");

  sink.cst_finish(K_LIST);
  depth_matches_oracle(&sink, "the outermost finish");
  assert_eq!(sink.depth(), 0);
}

/// Deep nesting, then unwound: the scalar must be exact at every level in both directions.
#[test]
fn depth_tracks_the_recount_across_deep_nesting() {
  const D: usize = 64;
  let mut sink = verbose_sink("");
  for i in 0..D {
    sink.cst_start(K_NODE);
    assert_eq!(sink.depth(), i as i64 + 1);
    depth_matches_oracle(&sink, "a nested start");
  }
  for i in (0..D).rev() {
    sink.cst_finish(K_NODE);
    assert_eq!(sink.depth(), i as i64);
    depth_matches_oracle(&sink, "a nested finish");
  }
}

/// The four arms of `rewind`, each restoring E5 the way its own contract says it must:
/// a spent row's frozen depth, the origin's zero, and the two arms that truncate nothing.
#[test]
fn depth_restores_across_every_rewind_arm() {
  // (1) A truncating rewind onto a captured row: the row's frozen depth.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_LIST);
  sink.cst_start(K_NODE);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  assert_eq!(sink.depth(), 2, "the row froze depth 2");
  sink.cst_start(K_WRAP);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  assert_eq!(sink.depth(), 3);
  rewind(&mut sink, ckp);
  depth_matches_oracle(&sink, "a truncating rewind onto a captured row");
  assert_eq!(sink.depth(), 2, "the abandoned branch's open node is gone");

  // Regrowth over the rewound region stays exact.
  sink.cst_start(K_WRAP);
  sink.cst_finish(K_WRAP);
  depth_matches_oracle(&sink, "regrowth after a rewind");
  assert_eq!(sink.depth(), 2);

  // (2) Rewind to the ORIGIN with no row: depth 0, the empty log's own value.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_LIST);
  sink.cst_start(K_NODE);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  assert_eq!(sink.depth(), 2);
  rewind(&mut sink, 0);
  depth_matches_oracle(&sink, "a no-row unwind to the origin");
  assert_eq!(sink.depth(), 0);
  assert!(sink.events().is_empty());

  // (3) Rewind to CURRENT length: truncates nothing, so the scalar is already right.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_LIST);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  rewind(&mut sink, ckp);
  depth_matches_oracle(&sink, "a rewind-to-current");
  assert_eq!(sink.depth(), 1, "the still-open node stays open");

  // (4) An out-of-range FUTURE mark: a total no-op on every channel, E5 included.
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_LIST);
  sink.cst_start(K_NODE);
  rewind(&mut sink, 999);
  depth_matches_oracle(&sink, "an out-of-range future mark");
  assert_eq!(sink.depth(), 2, "a future mark rewinds nothing");

  // (5) TRUNCATE AND REOPEN — the arm a scalar depth cannot tell from arm (1), and the one
  // E7/E8 have to get right on their own. The node open at the mark is CLOSED above it and a
  // different node is opened in its place, so the rewind must reopen the first: same depth,
  // different node. It is also the case that makes reclaiming a closed node's chain slot
  // unsound — a reclaiming design hands the freed slot to `K_NODE`, the rewind then drops it as
  // "above the mark", and the row's frozen head is left naming nothing (see `OpenNode`).
  let mut sink = verbose_sink("ab");
  sink.cst_start(K_LIST);
  let ckp = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.cst_finish(K_LIST);
  sink.cst_start(K_NODE);
  assert_eq!(sink.depth(), 1, "same depth as the mark, a different node");
  rewind(&mut sink, ckp);
  depth_matches_oracle(&sink, "a truncate-and-reopen rewind");
  assert_eq!(sink.depth(), 1);
  #[cfg(debug_assertions)]
  assert_eq!(
    sink.open_chain(),
    std::vec![0],
    "the rewind must reopen the node the mark had open, not leave the one that replaced it"
  );
}

/// A released row promotes the floor and spends no depth: `release` touches no event, so E5
/// must be untouched by it — and a later rewind below the released mark must still restore
/// from the surviving row rather than from the floor.
#[test]
fn depth_survives_release_and_a_rewind_below_the_released_floor() {
  let mut sink = verbose_sink("abc");
  sink.cst_start(K_LIST);
  let outer = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.cst_start(K_NODE);
  let inner = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  Emitter::<MiniLexer<'_>>::release(&mut sink, inner);
  depth_matches_oracle(&sink, "release");
  assert_eq!(sink.depth(), 2, "a kept branch changes no event");

  sink.cst_start(K_WRAP);
  assert_eq!(sink.depth(), 3);
  rewind(&mut sink, outer);
  depth_matches_oracle(&sink, "a rewind below the released floor");
  assert_eq!(sink.depth(), 1);
}

/// The raw injection hook charges E5 like every other append — the shapes it builds are
/// exactly the corrupt ones the oracle most needs to agree about.
#[test]
fn depth_tracks_the_recount_across_raw_injection() {
  let mut sink = verbose_sink("");
  sink.push_raw_event_for_tests(Event::StartNode {
    kind: K_NODE,
    forward_parent: None,
  });
  depth_matches_oracle(&sink, "an injected StartNode");
  sink.push_raw_event_for_tests(Event::FinishNode { kind: K_NODE });
  depth_matches_oracle(&sink, "an injected FinishNode");
  // The orphan finish the emission door refuses: the scalar goes negative, which is why E5 is
  // signed — an unsigned cell would wrap and hide exactly the state the check looks for.
  sink.push_raw_event_for_tests(Event::FinishNode { kind: K_NODE });
  depth_matches_oracle(&sink, "an injected orphan finish");
  assert_eq!(sink.depth(), -1);
}

/// al8n/tokora#250 — the undo journal stays exact across a hole wrap with **no** fix-up pass.
/// The proof is that every entry's `StartAt` sits strictly below the splice point; this is the
/// executable half: wrap a hole above a live retro-wrap chain, then rewind below the chain and
/// require the journal to restore the tombstone's `forward_parent` to what it was.
#[test]
fn a_hole_wrap_leaves_the_undo_journal_exact_without_a_fixup() {
  let mut sink = verbose_sink("abcd");
  let tomb = sink.cst_mark();
  assert_eq!(sink.forward_parent_at(0), None);

  // Two same-target retro-wraps: two live journal entries, the chain head on the tombstone.
  sink.record_token(&MiniTok(b'a'), &span(0, 1));
  sink.cst_start_at(tomb, K_WRAP);
  sink.cst_finish(K_WRAP);
  let after_first = Emitter::<MiniLexer<'_>>::checkpoint(&mut sink);
  let head_after_first = sink.forward_parent_at(0);
  sink.cst_start_at(tomb, K_WRAP);
  sink.cst_finish(K_WRAP);
  assert_eq!(sink.journal_len(), 2, "two live journaled writes");

  // A recovery hole above them: one token, wrapped in place.
  sink.record_token(&MiniTok(b'b'), &span(1, 2));
  let before = sink.events().len();
  Emitter::<MiniLexer<'_>>::emit_skipped_region(&mut sink, span(1, 2), 1)
    .expect("verbose emitters collect");
  assert_eq!(
    sink.events().len(),
    before + 3,
    "the wrap splices a StartNode, appends a FinishNode, and the report takes a Diag slot"
  );
  assert_eq!(
    sink.journal_len(),
    2,
    "a hole wrap neither adds nor drops a journaled write"
  );
  depth_matches_oracle(&sink, "a hole wrap above a live retro-wrap chain");

  // The entries still name what they named: rewinding below the SECOND wrap must restore the
  // chain head the FIRST wrap left. A journal index renamed by the splice would restore the
  // wrong slot (or none) and leave the tombstone pointing at a wrap that no longer exists.
  rewind(&mut sink, after_first);
  assert_eq!(
    sink.forward_parent_at(0),
    head_after_first,
    "the undo journal restored the exact chain head the first wrap left"
  );
  assert_eq!(sink.journal_len(), 1, "the second wrap's entry popped");

  // And all the way down: the tombstone comes back unwrapped.
  rewind(&mut sink, 0);
  assert_eq!(sink.journal_len(), 0);
  assert_eq!(sink.depth(), 0);
}
