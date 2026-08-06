#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Every forwarding method lands on **its own** operation, with its arguments unchanged.
//!
//! # The defect this suite exists to catch
//!
//! Three receivers carry a thin forwarding surface onto the emitter's operations:
//! [`EmitterView`] (28 public methods), [`InputRef`]'s `emit.rs` (26) and [`ParseState`] (18).
//! They were written by copy-and-adjust, which is the shape where a wrong delegation target is
//! easiest to introduce and hardest to see. An `emit_warning` whose body calls `emit_error`
//! compiles, propagates nothing, and every other test in the crate still passes — the defect
//! surfaces as the wrong diagnostic in a consumer's output, arbitrarily far from the cause.
//! Measured, not assumed: re-pointing `InputRef::emit_missing_leading_separator` at
//! `emit_missing_separator` produced zero failures anywhere in the crate before this file
//! existed.
//!
//! "The method runs without panicking" does not detect that. What detects it is an assertion
//! that the **specific** call landed on the inner emitter, with the arguments it was handed.
//! Every test here does exactly that and nothing else: it opens a window on a recording
//! emitter, makes exactly one forwarded call, and asserts the window holds exactly one
//! recorded call — the right one, with the right arguments. An extra call, a missing call, a
//! sibling call, or a mangled argument all fail the same equality.
//!
//! # How wide the reachable defect is
//!
//! Narrower than the method count, and worth knowing before reading the rows: the capability
//! bounds sit on the **methods**, so most sibling re-pointings do not type-check at all.
//! `emit_missing_leading_separator` cannot call `emit_missing_trailing_separator` — its bound is
//! `MissingLeadingSeparatorEmitter`, which does not imply the trailing one — and the payload
//! types keep `emit_too_few` / `emit_too_many`, `emit_lexer_error` / `emit_error` and the two
//! pratt ends apart the same way. What remains reachable is the identical-signature families:
//! `emit_error` / `emit_warning`, `cst_start` / `cst_finish`, and the three missing-separator
//! methods collapsing onto the `SeparatedEmitter` supertrait's `emit_missing_separator`. Those
//! are exactly the shapes the mutation proofs in the pull request use.
//!
//! # Why a new instrument
//!
//! Four recording candidates already exist in the tree, and none of them discriminates
//! *which* method landed:
//!
//! - [`Verbose`](tokora::emitter::Verbose) records three channels — errors, warnings and
//!   recovery holes. The eleven capability channels all funnel through the same private
//!   `record` into the **error** channel, so `emit_too_few`, `emit_missing_leading_separator`
//!   and `emit_error` are one channel to it; `tests/verbose_conformance.rs` says so in its own
//!   header, for the three missing-separator siblings specifically. Its `CstEmitter` impl is
//!   the defaulted no-op set, so the four `cst_*` forwards record nothing at all.
//! - `TrackingEmitter` in `tests/handler_coverage.rs` holds three `usize` counters
//!   (`calls`, `releases`, `cst_events`). It proves a call arrived; it cannot say which.
//! - `CountEmitter` in `src/fuzz/fixtures.rs` holds one counter, for the same reason.
//! - `src/conformance/emitter.rs` is the `bound_source` kit, one member wide.
//!
//! So [`Ledger`] below records a **tagged call with its arguments** — one [`Call`] variant per
//! operation, including the five members the forwarding surface deliberately withholds
//! (`checkpoint`, `rewind`, `release`, `commit_token`, `commit_lexer_error`), so that a forward
//! that lands on one of *those* is visible too.
//!
//! # The delegation chain, and how to read a failure
//!
//! The three receivers are layered, not parallel:
//!
//! ```text
//! ParseState::emit_x  ->  InputRef::emit_x  ->  EmitterView::emit_x  ->  Emitter::emit_x
//! ```
//!
//! (the two `&self` readers on `InputRef` — `emitter_ref` and `emitter_bound_source` — are the
//! exception and reach the emitter directly, because a view needs a `&mut` borrow.)
//!
//! So a defect introduced at one layer reds that layer's test **and every layer above it**, and
//! all of those reds are true positives: the upper forwards really do deliver the wrong
//! operation once the lower one is wrong. Read the red set bottom-up — the lowest receiver that
//! fails is where the body is wrong. A defect in `ParseState::emit_warning` reds one test; the
//! same defect in `EmitterView::emit_warning` reds three.
//!
//! # What this suite does not pin
//!
//! The [`EventMark`] argument of `cst_start_at`. `EventMark` has no public constructor — a live
//! one is minted only by a recording sink — so the only value a test can obtain outside the
//! `rowan` sink machinery is the single inert mark, and one value cannot witness pass-through.
//! The bound is tight rather than worrying: the mark parameter is the only `EventMark` in scope
//! in all three bodies, so there is no other value a mis-written body could pass, and the pair
//! `(EventMark, u16)` cannot be transposed. The `kind` argument and the method identity are
//! both pinned below.
//!
//! [`EmitterView`]: tokora::EmitterView
//! [`InputRef`]: tokora::InputRef
//! [`ParseState`]: tokora::ParseState

mod common;

use std::{
  cell::{Cell, RefCell},
  rc::Rc,
  string::{String, ToString},
  vec::Vec,
};

// Read only by the FORWARDING_CENSUS section, which is gated on `pratt`; carrying the same gate
// keeps the `std,logos,trace` leg warning-free.
#[cfg(feature = "pratt")]
use std::collections::BTreeSet;

use tokora::{
  Emitter, EmitterView, InputRef, Lexer, Parse, Parser, ParserContext, Token as TokenTrait,
  cst::event::EventMark,
  delimiter::DelimiterKind,
  emitter::{
    CstEmitter, FromUnclosed, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnclosedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingToken, MissingTokenOf, SeparatedError, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  source::SourceIdentity,
  span::{SimpleSpan, Spanned},
  utils::CowStr,
};

#[cfg(feature = "map")]
use tokora::{ParseInput, ParseState, parser::Empty};

#[cfg(feature = "pratt")]
use tokora::{
  emitter::PrattEmitter,
  error::{UnexpectedEoLhs, UnexpectedEoRhs},
};

use common::TestLexer;

// ── The recorded vocabulary ───────────────────────────────────────────────────

/// The emitter's error type. Distinct variants for the two payload-carrying channels, so a
/// swap of `emit_error` and `emit_warning` is visible in the payload as well as in the tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Note {
  /// Produced by a `From` conversion the parse machinery drives; never asserted on.
  Converted,
  /// The payload the `emit_error` rows hand over.
  Alpha,
  /// The payload the `emit_warning` rows hand over.
  Beta,
}

impl From<()> for Note {
  fn from(_: ()) -> Self {
    Note::Converted
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for Note {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    Note::Converted
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Note {
  fn from(_: TooFew<S, Lang>) -> Self {
    Note::Converted
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Note {
  fn from(_: TooMany<S, Lang>) -> Self {
    Note::Converted
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Note {
  fn from(_: FullContainer<S, Lang>) -> Self {
    Note::Converted
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for Note {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    Note::Converted
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Note {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Note::Converted
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for Note {
  fn from(_: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    Note::Converted
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for Note {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    Note::Converted
  }
}

impl<Set: Clone + 'static> From<UnexpectedEot<usize, (), Set>> for Note {
  fn from(_: UnexpectedEot<usize, (), Set>) -> Self {
    Note::Converted
  }
}

impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for Note
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    Note::Converted
  }
}

#[cfg(feature = "pratt")]
impl<O, Lang: ?Sized> From<UnexpectedEoLhs<O, Lang>> for Note {
  fn from(_: UnexpectedEoLhs<O, Lang>) -> Self {
    Note::Converted
  }
}

#[cfg(feature = "pratt")]
impl<O, Lang: ?Sized> From<UnexpectedEoRhs<O, Lang>> for Note {
  fn from(_: UnexpectedEoRhs<O, Lang>) -> Self {
    Note::Converted
  }
}

/// One call that landed on the inner emitter — the operation, and the arguments it landed with.
///
/// One variant per member, including the five the forwarding surface withholds: a forward that
/// reached `commit_token` instead of an emission would otherwise be a silent pass.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
  // `Emitter`
  EmitLexerError(SimpleSpan),
  EmitUnexpectedToken(SimpleSpan),
  EmitError(SimpleSpan, Note),
  EmitWarning(SimpleSpan, Note),
  EmitSkippedRegion(SimpleSpan, usize),
  EnterLabel(&'static str),
  ExitLabel,
  BoundSource,
  // The five withheld members, recorded so a forward that lands on one is not silent.
  Checkpoint,
  Rewind(u64),
  Release(u64),
  CommitToken(SimpleSpan),
  CommitLexerError(SimpleSpan),
  // `CstEmitter`
  CstStart(u16),
  CstFinish(u16),
  CstDemote(EventMark, u16),
  CstMark,
  CstStartAt(EventMark, u16),
  // The capability channels
  TooFew(SimpleSpan, usize, usize),
  TooMany(SimpleSpan, usize, usize),
  FullContainer(SimpleSpan, usize, usize),
  MissingSeparator(String, usize),
  MissingElement(usize),
  MissingLeadingSeparator(String, usize),
  MissingTrailingSeparator(String, usize),
  UnexpectedLeadingSeparator(String, SimpleSpan),
  UnexpectedTrailingSeparator(String, SimpleSpan),
  Unclosed(SimpleSpan, DelimiterKind, String),
  #[cfg(feature = "pratt")]
  UnexpectedEndOfLhs(usize),
  #[cfg(feature = "pratt")]
  UnexpectedEndOfRhs(usize),
}

/// The shared call log. Shared by handle rather than owned by the emitter: a parse takes the
/// emitter by value into its context and never gives it back, so the read side has to be a
/// second handle onto the same storage.
#[derive(Clone, Debug, Default)]
struct Log(Rc<RefCell<Vec<Call>>>);

impl Log {
  fn push(&self, call: Call) {
    self.0.borrow_mut().push(call);
  }

  /// Empties the log and returns what was in it — both the window-open and the window-close
  /// operation, so a reading is always exactly the calls made since the previous reading.
  fn take(&self) -> Vec<Call> {
    core::mem::take(&mut *self.0.borrow_mut())
  }

  /// Whether two handles name the same storage — the identity test for `emitter_ref`.
  fn is(&self, other: &Log) -> bool {
    Rc::ptr_eq(&self.0, &other.0)
  }
}

/// The one source every parse in this suite runs over, **and** the source every [`Ledger`]
/// reports itself bound to.
///
/// Those have to be the same value, and finding that out is worth recording: `bound_source` is
/// not only a query. The parse entry compares the emitter's answer against the buffer it was
/// handed and refuses a mismatch outright (`Input::with_state_and_context` in `src/input/mod.rs`
/// panics), so a `Ledger` bound to a private sentinel string cannot be driven through a parse at
/// all. Binding it to the parse's own buffer keeps the answer distinguishable from
/// `bound_source`'s `None` default — which is all the forwarding assertion needs — while
/// satisfying the entry check.
static SOURCE: &str = "12 34";

fn bound_identity() -> SourceIdentity {
  SourceIdentity::of(SOURCE)
}

/// The recording emitter: every member appends a tagged call carrying its arguments.
///
/// `emissions` mirrors `Verbose`'s emission mark (a count that `rewind` truncates) so the input
/// layer's transaction guards see a well-behaved emitter; it is deliberately **not** the log
/// length, because the log is a call trace and a rewind must not erase the record of itself.
#[derive(Debug)]
struct Ledger {
  log: Log,
  emissions: Cell<u64>,
}

impl Ledger {
  fn new(log: Log) -> Self {
    Self {
      log,
      emissions: Cell::new(0),
    }
  }

  /// The log handle — the read side of `emitter_ref`'s identity assertion.
  fn log(&self) -> &Log {
    &self.log
  }

  /// Records a non-emission (a query, a label move, a structural event).
  fn note(&self, call: Call) {
    self.log.push(call);
  }

  /// Records an emission and advances the emission mark.
  fn emit(&self, call: Call) {
    self.emissions.set(self.emissions.get() + 1);
    self.log.push(call);
  }
}

impl<'inp> Emitter<'inp, TestLexer<'inp>> for Ledger {
  type Error = Note;

  fn emit_lexer_error(
    &mut self,
    err: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), Note> {
    self.emit(Call::EmitLexerError(*err.span_ref()));
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    err: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Note> {
    self.emit(Call::EmitUnexpectedToken(*err.span_ref()));
    Ok(())
  }

  fn emit_error(
    &mut self,
    err: Spanned<Note, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Note> {
    self.emit(Call::EmitError(*err.span_ref(), *err.data()));
    Ok(())
  }

  fn emit_warning(
    &mut self,
    warning: Spanned<Note, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Note> {
    self.emit(Call::EmitWarning(*warning.span_ref(), *warning.data()));
    Ok(())
  }

  fn emit_skipped_region(
    &mut self,
    span: <TestLexer<'inp> as Lexer<'inp>>::Span,
    skipped: usize,
  ) -> Result<(), Note> {
    self.emit(Call::EmitSkippedRegion(span, skipped));
    Ok(())
  }

  fn checkpoint(&self) -> u64 {
    self.note(Call::Checkpoint);
    self.emissions.get()
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, checkpoint: u64) {
    self.note(Call::Rewind(checkpoint));
    self.emissions.set(checkpoint);
  }

  fn release(&mut self, checkpoint: u64) {
    self.note(Call::Release(checkpoint));
  }

  fn commit_token(
    &mut self,
    _: &<TestLexer<'inp> as Lexer<'inp>>::Token,
    span: &<TestLexer<'inp> as Lexer<'inp>>::Span,
  ) {
    self.note(Call::CommitToken(*span));
  }

  fn commit_lexer_error(
    &mut self,
    err: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), Note> {
    self.emit(Call::CommitLexerError(*err.span_ref()));
    Ok(())
  }

  fn enter_label(&mut self, label: &'static str) {
    self.note(Call::EnterLabel(label));
  }

  fn exit_label(&mut self) {
    self.note(Call::ExitLabel);
  }

  fn bound_source(&self) -> Option<SourceIdentity> {
    self.note(Call::BoundSource);
    Some(bound_identity())
  }
}

impl<'inp> CstEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn cst_start(&mut self, kind: u16) -> EventMark {
    self.note(Call::CstStart(kind));
    inert_mark()
  }

  fn cst_finish(&mut self, kind: u16) {
    self.note(Call::CstFinish(kind));
  }

  fn cst_demote(&mut self, mark: EventMark, kind: u16) {
    self.note(Call::CstDemote(mark, kind));
  }

  fn cst_mark(&mut self) -> EventMark {
    self.note(Call::CstMark);
    inert_mark()
  }

  fn cst_start_at(&mut self, mark: EventMark, kind: u16) {
    self.note(Call::CstStartAt(mark, kind));
  }
}

impl<'inp> TooFewEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_too_few(
    &mut self,
    err: TooFew<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Note> {
    self.emit(Call::TooFew(*err.span_ref(), err.nums(), err.limit()));
    Ok(())
  }
}

impl<'inp> TooManyEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_too_many(
    &mut self,
    err: TooMany<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Note> {
    self.emit(Call::TooMany(*err.span_ref(), err.nums(), err.limit()));
    Ok(())
  }
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_full_container(
    &mut self,
    err: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Note> {
    self.emit(Call::FullContainer(*err.span(), err.nums(), err.capacity()));
    Ok(())
  }
}

impl<'inp> SeparatedEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_missing_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Note> {
    self.emit(Call::MissingSeparator(
      <CowStr as AsRef<str>>::as_ref(&name).to_string(),
      err.offset(),
    ));
    Ok(())
  }

  fn emit_missing_element(
    &mut self,
    err: MissingSyntaxOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Note> {
    self.emit(Call::MissingElement(err.offset()));
    Ok(())
  }
}

impl<'inp> MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_missing_leading_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Note> {
    self.emit(Call::MissingLeadingSeparator(
      <CowStr as AsRef<str>>::as_ref(&name).to_string(),
      err.offset(),
    ));
    Ok(())
  }
}

impl<'inp> MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_missing_trailing_separator(
    &mut self,
    name: CowStr,
    err: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Note> {
    self.emit(Call::MissingTrailingSeparator(
      <CowStr as AsRef<str>>::as_ref(&name).to_string(),
      err.offset(),
    ));
    Ok(())
  }
}

impl<'inp> UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_unexpected_leading_separator(
    &mut self,
    name: CowStr,
    err: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Note> {
    self.emit(Call::UnexpectedLeadingSeparator(
      <CowStr as AsRef<str>>::as_ref(&name).to_string(),
      *err.span_ref(),
    ));
    Ok(())
  }
}

impl<'inp> UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_unexpected_trailing_separator(
    &mut self,
    name: CowStr,
    err: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), Note> {
    self.emit(Call::UnexpectedTrailingSeparator(
      <CowStr as AsRef<str>>::as_ref(&name).to_string(),
      *err.span_ref(),
    ));
    Ok(())
  }
}

impl<'inp> UnclosedEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_unclosed<Delimiter>(
    &mut self,
    err: Unclosed<Delimiter, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), Note>
  where
    Note: FromUnclosed<'inp, TestLexer<'inp>>,
  {
    self.emit(Call::Unclosed(
      *err.span_ref(),
      err.kind(),
      err.name_ref().to_string(),
    ));
    Ok(())
  }
}

#[cfg(feature = "pratt")]
impl<'inp> PrattEmitter<'inp, TestLexer<'inp>> for Ledger {
  fn emit_unexpected_end_of_lhs(
    &mut self,
    err: UnexpectedEoLhs<<TestLexer<'inp> as Lexer<'inp>>::Offset>,
  ) -> Result<(), Note> {
    self.emit(Call::UnexpectedEndOfLhs(err.offset()));
    Ok(())
  }

  fn emit_unexpected_end_of_rhs(
    &mut self,
    err: UnexpectedEoRhs<<TestLexer<'inp> as Lexer<'inp>>::Offset>,
  ) -> Result<(), Note> {
    self.emit(Call::UnexpectedEndOfRhs(err.offset()));
    Ok(())
  }
}

/// The one [`EventMark`] a test can obtain: the inert mark a diagnostics-only emitter's
/// defaulted `cst_mark` returns. See the module header for what that bounds.
fn inert_mark() -> EventMark {
  <tokora::emitter::Ignored as CstEmitter<'static, TestLexer<'static>>>::cst_mark(
    &mut tokora::emitter::Ignored::default(),
  )
}

// ── Fixtures ──────────────────────────────────────────────────────────────────
//
// One distinct span / offset / name / count per row, so an argument that arrives from the wrong
// row is visible in the assertion's diff and not only in the tag.

fn sp(start: usize, end: usize) -> SimpleSpan {
  SimpleSpan::new(start, end)
}

const KIND_START: u16 = 0x0151;
const KIND_FINISH: u16 = 0x0152;
const KIND_START_AT: u16 = 0x0153;
const KIND_DEMOTE: u16 = 0x0154;
const LABEL: &str = "ledger-label";

// ── The assertion ─────────────────────────────────────────────────────────────

/// Asserts that the window holds exactly `want` — the whole discipline of this suite.
#[track_caller]
fn landed(receiver: &str, method: &str, got: &[Call], want: &[Call]) {
  assert_eq!(
    got, want,
    "`{receiver}::{method}` did not land on its own operation.\n  expected: {want:?}\n  \
     observed: {got:?}\nA thin forwarding method that reaches a *sibling* compiles, propagates \
     nothing, and passes every other test in this crate; this suite is the only place the \
     difference is visible. If the observed call is a neighbour of the expected one, the \
     forward's body names the wrong target. If the observed list is empty, the forward reached \
     a defaulted no-op instead of the emitter."
  );
}

// ══════════════════════════════════════════════════════════════════════════════
// `EmitterView` — 28 public methods
// ══════════════════════════════════════════════════════════════════════════════
//
// The lowest layer, and the only one reachable without a parse: `EmitterView::new` is public
// precisely so a callback body can be exercised over an emitter the test owns.

type View<'a> = EmitterView<'a, 'static, TestLexer<'static>, Ledger>;

/// One `EmitterView` row: a fresh ledger, a fresh view, exactly one forwarded call, and the
/// window it produced. One test per row and one ledger per test, so a mis-pointed forward reds
/// this row and no other at this layer.
macro_rules! view_case {
  ($(#[$attr:meta])* $name:ident, $method:literal, |$v:ident| $body:block, $want:expr) => {
    $(#[$attr])*
    #[test]
    fn $name() {
      let log = Log::default();
      let mut ledger = Ledger::new(log.clone());
      {
        let mut view: View<'_> = EmitterView::new(&mut ledger);
        let $v = &mut view;
        $body
      }
      landed("EmitterView", $method, &log.take(), &$want);
    }
  };
}

/// `new` hands back a view onto **that** emitter: the emission a view built over `ledger` makes
/// lands in `ledger`'s log, and the view's own reader names the same storage.
#[test]
fn view_new() {
  let log = Log::default();
  let mut ledger = Ledger::new(log.clone());
  {
    let mut view: View<'_> = EmitterView::new(&mut ledger);
    assert!(
      view.emitter_ref().log().is(&log),
      "`EmitterView::new` built a view onto storage other than the emitter it was handed"
    );
    view
      .emit_error(Spanned::new(sp(11, 12), Note::Alpha))
      .expect("the ledger records rather than propagates");
  }
  landed(
    "EmitterView",
    "new",
    &log.take(),
    &[Call::EmitError(sp(11, 12), Note::Alpha)],
  );
}

/// `reborrow` yields a second view onto the **same** emitter, not a view onto nothing: an
/// emission through the reborrow reaches the original ledger.
#[test]
fn view_reborrow() {
  let log = Log::default();
  let mut ledger = Ledger::new(log.clone());
  {
    let mut view: View<'_> = EmitterView::new(&mut ledger);
    let mut second = view.reborrow();
    assert!(
      second.emitter_ref().log().is(&log),
      "`EmitterView::reborrow` yielded a view onto other storage"
    );
    second
      .emit_warning(Spanned::new(sp(13, 14), Note::Beta))
      .expect("the ledger records rather than propagates");
  }
  landed(
    "EmitterView",
    "reborrow",
    &log.take(),
    &[Call::EmitWarning(sp(13, 14), Note::Beta)],
  );
}

/// `emitter_ref` reads the parse's own emitter — proved by identity of the shared storage, and
/// by the read itself recording nothing.
#[test]
fn view_emitter_ref() {
  let log = Log::default();
  let mut ledger = Ledger::new(log.clone());
  {
    let view: View<'_> = EmitterView::new(&mut ledger);
    assert!(
      view.emitter_ref().log().is(&log),
      "`EmitterView::emitter_ref` handed back an emitter other than the one the view lends"
    );
  }
  landed("EmitterView", "emitter_ref", &log.take(), &[]);
}

view_case!(
  view_emit_lexer_error,
  "emit_lexer_error",
  |v| {
    v.emit_lexer_error(Spanned::new(sp(42, 43), ()))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitLexerError(sp(42, 43))]
);

view_case!(
  view_emit_unexpected_token,
  "emit_unexpected_token",
  |v| {
    v.emit_unexpected_token(UnexpectedToken::new(sp(44, 45)))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitUnexpectedToken(sp(44, 45))]
);

view_case!(
  view_emit_error,
  "emit_error",
  |v| {
    v.emit_error(Spanned::new(sp(11, 12), Note::Alpha))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitError(sp(11, 12), Note::Alpha)]
);

view_case!(
  view_emit_warning,
  "emit_warning",
  |v| {
    v.emit_warning(Spanned::new(sp(13, 14), Note::Beta))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitWarning(sp(13, 14), Note::Beta)]
);

view_case!(
  view_emit_skipped_region,
  "emit_skipped_region",
  |v| {
    v.emit_skipped_region(sp(46, 47), 8)
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitSkippedRegion(sp(46, 47), 8)]
);

view_case!(
  view_enter_label,
  "enter_label",
  |v| {
    v.enter_label(LABEL);
  },
  [Call::EnterLabel(LABEL)]
);

view_case!(
  view_exit_label,
  "exit_label",
  |v| {
    v.exit_label();
  },
  [Call::ExitLabel]
);

view_case!(
  view_bound_source,
  "bound_source",
  |v| {
    assert_eq!(
      v.bound_source(),
      Some(bound_identity()),
      "`EmitterView::bound_source` did not report the emitter's own binding"
    );
  },
  [Call::BoundSource]
);

view_case!(
  view_cst_start,
  "cst_start",
  |v| {
    assert_eq!(
      v.cst_start(KIND_START),
      inert_mark(),
      "`EmitterView::cst_start` did not hand back the mark the emitter minted"
    );
  },
  [Call::CstStart(KIND_START)]
);

view_case!(
  view_cst_finish,
  "cst_finish",
  |v| {
    v.cst_finish(KIND_FINISH);
  },
  [Call::CstFinish(KIND_FINISH)]
);

view_case!(
  view_cst_demote,
  "cst_demote",
  |v| {
    v.cst_demote(inert_mark(), KIND_DEMOTE);
  },
  [Call::CstDemote(inert_mark(), KIND_DEMOTE)]
);

view_case!(
  view_cst_mark,
  "cst_mark",
  |v| {
    assert_eq!(
      v.cst_mark(),
      inert_mark(),
      "`EmitterView::cst_mark` did not hand back the mark the emitter minted"
    );
  },
  [Call::CstMark]
);

view_case!(
  view_cst_start_at,
  "cst_start_at",
  |v| {
    v.cst_start_at(inert_mark(), KIND_START_AT);
  },
  [Call::CstStartAt(inert_mark(), KIND_START_AT)]
);

view_case!(
  view_emit_too_few,
  "emit_too_few",
  |v| {
    v.emit_too_few(TooFew::new(sp(20, 21), 1, 3))
      .expect("the ledger records rather than propagates");
  },
  [Call::TooFew(sp(20, 21), 1, 3)]
);

view_case!(
  view_emit_too_many,
  "emit_too_many",
  |v| {
    v.emit_too_many(TooMany::new(sp(22, 23), 9, 4))
      .expect("the ledger records rather than propagates");
  },
  [Call::TooMany(sp(22, 23), 9, 4)]
);

view_case!(
  view_emit_full_container,
  "emit_full_container",
  |v| {
    v.emit_full_container(FullContainer::new(sp(24, 25), 10, 5))
      .expect("the ledger records rather than propagates");
  },
  [Call::FullContainer(sp(24, 25), 10, 5)]
);

view_case!(
  view_emit_missing_separator,
  "emit_missing_separator",
  |v| {
    v.emit_missing_separator(CowStr::from_static("sep"), MissingToken::new(30usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingSeparator("sep".to_string(), 30)]
);

view_case!(
  view_emit_missing_element,
  "emit_missing_element",
  |v| {
    v.emit_missing_element(MissingSyntax::new(31usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingElement(31)]
);

view_case!(
  view_emit_missing_leading_separator,
  "emit_missing_leading_separator",
  |v| {
    v.emit_missing_leading_separator(CowStr::from_static("lead"), MissingToken::new(32usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingLeadingSeparator("lead".to_string(), 32)]
);

view_case!(
  view_emit_missing_trailing_separator,
  "emit_missing_trailing_separator",
  |v| {
    v.emit_missing_trailing_separator(CowStr::from_static("trail"), MissingToken::new(33usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingTrailingSeparator("trail".to_string(), 33)]
);

view_case!(
  view_emit_unexpected_leading_separator,
  "emit_unexpected_leading_separator",
  |v| {
    v.emit_unexpected_leading_separator(
      CowStr::from_static("ulead"),
      UnexpectedToken::new(sp(34, 35)),
    )
    .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedLeadingSeparator(
    "ulead".to_string(),
    sp(34, 35)
  )]
);

view_case!(
  view_emit_unexpected_trailing_separator,
  "emit_unexpected_trailing_separator",
  |v| {
    v.emit_unexpected_trailing_separator(
      CowStr::from_static("utrail"),
      UnexpectedToken::new(sp(36, 37)),
    )
    .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedTrailingSeparator(
    "utrail".to_string(),
    sp(36, 37)
  )]
);

view_case!(
  view_emit_unclosed,
  "emit_unclosed",
  |v| {
    v.emit_unclosed::<()>(Unclosed::new(
      sp(38, 39),
      DelimiterKind::Custom("ledger-pair"),
      CowStr::from_static("ledger"),
    ))
    .expect("the ledger records rather than propagates");
  },
  [Call::Unclosed(
    sp(38, 39),
    DelimiterKind::Custom("ledger-pair"),
    "ledger".to_string()
  )]
);

view_case!(
  #[cfg(feature = "pratt")]
  view_emit_unexpected_end_of_lhs,
  "emit_unexpected_end_of_lhs",
  |v| {
    v.emit_unexpected_end_of_lhs(UnexpectedEoLhs::eolhs(40usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedEndOfLhs(40)]
);

view_case!(
  #[cfg(feature = "pratt")]
  view_emit_unexpected_end_of_rhs,
  "emit_unexpected_end_of_rhs",
  |v| {
    v.emit_unexpected_end_of_rhs(UnexpectedEoRhs::eorhs(41usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedEndOfRhs(41)]
);

// ══════════════════════════════════════════════════════════════════════════════
// `InputRef` — 26 public methods
// ══════════════════════════════════════════════════════════════════════════════
//
// Reached through a real parse: the handle's emitter accessor is crate-private, so the subject
// has to be driven rather than constructed. The window is opened *inside* the parse body and
// closed immediately after the call, so the entry's and exit's own emitter traffic — settles,
// checkpoints, the end-of-input check — is never in the reading.

type Ir<'inp, 'c> =
  InputRef<'inp, 'c, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Ledger>>;

/// The callback's view of the parse — the `map_with` parameter's type, spelled once so a row
/// that only *reads* through it still pins the context.
#[cfg(feature = "map")]
type St<'a, 'inp, 'c> =
  ParseState<'a, 'inp, 'c, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, Ledger>>;

/// One `InputRef` row: a fresh parse over a fresh ledger, exactly one forwarded call inside a
/// cleared window.
macro_rules! handle_case {
  ($(#[$attr:meta])* $name:ident, $method:literal, |$h:ident| $body:block, $want:expr) => {
    $(#[$attr])*
    #[test]
    fn $name() {
      let log = Log::default();
      let seen: Rc<RefCell<Vec<Call>>> = Rc::new(RefCell::new(Vec::new()));
      {
        let log = log.clone();
        let seen = seen.clone();
        let ctx = ParserContext::new(Ledger::new(log.clone()));
        let result: Result<(), Note> = Parser::with_context(ctx)
          .apply(move |inp: &mut Ir<'_, '_>| {
            let _ = log.take();
            {
              let $h = &mut *inp;
              $body
            }
            *seen.borrow_mut() = log.take();
            Ok(())
          })
          .parse_str(SOURCE);
        result.expect("the ledger records rather than propagates");
      }
      let got = core::mem::take(&mut *seen.borrow_mut());
      landed("InputRef", $method, &got, &$want);
    }
  };
}

/// `emitter_ref` reads the parse's **own** emitter, proved by storage identity — and reads it
/// without emitting.
#[test]
fn handle_emitter_ref() {
  let log = Log::default();
  let seen: Rc<RefCell<Vec<Call>>> = Rc::new(RefCell::new(Vec::new()));
  {
    let log = log.clone();
    let seen = seen.clone();
    let ctx = ParserContext::new(Ledger::new(log.clone()));
    let result: Result<(), Note> = Parser::with_context(ctx)
      .apply(move |inp: &mut Ir<'_, '_>| {
        let _ = log.take();
        assert!(
          inp.emitter_ref().log().is(&log),
          "`InputRef::emitter_ref` handed back an emitter other than the parse's own"
        );
        *seen.borrow_mut() = log.take();
        Ok(())
      })
      .parse_str(SOURCE);
    result.expect("the ledger records rather than propagates");
  }
  let got = core::mem::take(&mut *seen.borrow_mut());
  landed("InputRef", "emitter_ref", &got, &[]);
}

handle_case!(
  handle_emit_lexer_error,
  "emit_lexer_error",
  |h| {
    h.emit_lexer_error(Spanned::new(sp(42, 43), ()))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitLexerError(sp(42, 43))]
);

handle_case!(
  handle_emit_unexpected_token,
  "emit_unexpected_token",
  |h| {
    h.emit_unexpected_token(UnexpectedToken::new(sp(44, 45)))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitUnexpectedToken(sp(44, 45))]
);

handle_case!(
  handle_emit_error,
  "emit_error",
  |h| {
    h.emit_error(Spanned::new(sp(11, 12), Note::Alpha))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitError(sp(11, 12), Note::Alpha)]
);

handle_case!(
  handle_emit_warning,
  "emit_warning",
  |h| {
    h.emit_warning(Spanned::new(sp(13, 14), Note::Beta))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitWarning(sp(13, 14), Note::Beta)]
);

handle_case!(
  handle_emit_skipped_region,
  "emit_skipped_region",
  |h| {
    h.emit_skipped_region(sp(46, 47), 8)
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitSkippedRegion(sp(46, 47), 8)]
);

handle_case!(
  handle_enter_label,
  "enter_label",
  |h| {
    h.enter_label(LABEL);
  },
  [Call::EnterLabel(LABEL)]
);

handle_case!(
  handle_exit_label,
  "exit_label",
  |h| {
    h.exit_label();
  },
  [Call::ExitLabel]
);

handle_case!(
  handle_emitter_bound_source,
  "emitter_bound_source",
  |h| {
    assert_eq!(
      h.emitter_bound_source(),
      Some(bound_identity()),
      "`InputRef::emitter_bound_source` did not report the emitter's own binding"
    );
  },
  [Call::BoundSource]
);

handle_case!(
  handle_cst_start,
  "cst_start",
  |h| {
    assert_eq!(
      h.cst_start(KIND_START),
      inert_mark(),
      "`InputRef::cst_start` did not hand back the mark the emitter minted"
    );
  },
  [Call::CstStart(KIND_START)]
);

handle_case!(
  handle_cst_finish,
  "cst_finish",
  |h| {
    h.cst_finish(KIND_FINISH);
  },
  [Call::CstFinish(KIND_FINISH)]
);

handle_case!(
  handle_cst_demote,
  "cst_demote",
  |h| {
    h.cst_demote(inert_mark(), KIND_DEMOTE);
  },
  [Call::CstDemote(inert_mark(), KIND_DEMOTE)]
);

handle_case!(
  handle_cst_mark,
  "cst_mark",
  |h| {
    assert_eq!(
      h.cst_mark(),
      inert_mark(),
      "`InputRef::cst_mark` did not hand back the mark the emitter minted"
    );
  },
  [Call::CstMark]
);

handle_case!(
  handle_cst_start_at,
  "cst_start_at",
  |h| {
    h.cst_start_at(inert_mark(), KIND_START_AT);
  },
  [Call::CstStartAt(inert_mark(), KIND_START_AT)]
);

handle_case!(
  handle_emit_too_few,
  "emit_too_few",
  |h| {
    h.emit_too_few(TooFew::new(sp(20, 21), 1, 3))
      .expect("the ledger records rather than propagates");
  },
  [Call::TooFew(sp(20, 21), 1, 3)]
);

handle_case!(
  handle_emit_too_many,
  "emit_too_many",
  |h| {
    h.emit_too_many(TooMany::new(sp(22, 23), 9, 4))
      .expect("the ledger records rather than propagates");
  },
  [Call::TooMany(sp(22, 23), 9, 4)]
);

handle_case!(
  handle_emit_full_container,
  "emit_full_container",
  |h| {
    h.emit_full_container(FullContainer::new(sp(24, 25), 10, 5))
      .expect("the ledger records rather than propagates");
  },
  [Call::FullContainer(sp(24, 25), 10, 5)]
);

handle_case!(
  handle_emit_missing_separator,
  "emit_missing_separator",
  |h| {
    h.emit_missing_separator(CowStr::from_static("sep"), MissingToken::new(30usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingSeparator("sep".to_string(), 30)]
);

handle_case!(
  handle_emit_missing_element,
  "emit_missing_element",
  |h| {
    h.emit_missing_element(MissingSyntax::new(31usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingElement(31)]
);

handle_case!(
  handle_emit_missing_leading_separator,
  "emit_missing_leading_separator",
  |h| {
    h.emit_missing_leading_separator(CowStr::from_static("lead"), MissingToken::new(32usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingLeadingSeparator("lead".to_string(), 32)]
);

handle_case!(
  handle_emit_missing_trailing_separator,
  "emit_missing_trailing_separator",
  |h| {
    h.emit_missing_trailing_separator(CowStr::from_static("trail"), MissingToken::new(33usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::MissingTrailingSeparator("trail".to_string(), 33)]
);

handle_case!(
  handle_emit_unexpected_leading_separator,
  "emit_unexpected_leading_separator",
  |h| {
    h.emit_unexpected_leading_separator(
      CowStr::from_static("ulead"),
      UnexpectedToken::new(sp(34, 35)),
    )
    .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedLeadingSeparator(
    "ulead".to_string(),
    sp(34, 35)
  )]
);

handle_case!(
  handle_emit_unexpected_trailing_separator,
  "emit_unexpected_trailing_separator",
  |h| {
    h.emit_unexpected_trailing_separator(
      CowStr::from_static("utrail"),
      UnexpectedToken::new(sp(36, 37)),
    )
    .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedTrailingSeparator(
    "utrail".to_string(),
    sp(36, 37)
  )]
);

handle_case!(
  handle_emit_unclosed,
  "emit_unclosed",
  |h| {
    h.emit_unclosed::<()>(Unclosed::new(
      sp(38, 39),
      DelimiterKind::Custom("ledger-pair"),
      CowStr::from_static("ledger"),
    ))
    .expect("the ledger records rather than propagates");
  },
  [Call::Unclosed(
    sp(38, 39),
    DelimiterKind::Custom("ledger-pair"),
    "ledger".to_string()
  )]
);

handle_case!(
  #[cfg(feature = "pratt")]
  handle_emit_unexpected_end_of_lhs,
  "emit_unexpected_end_of_lhs",
  |h| {
    h.emit_unexpected_end_of_lhs(UnexpectedEoLhs::eolhs(40usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedEndOfLhs(40)]
);

handle_case!(
  #[cfg(feature = "pratt")]
  handle_emit_unexpected_end_of_rhs,
  "emit_unexpected_end_of_rhs",
  |h| {
    h.emit_unexpected_end_of_rhs(UnexpectedEoRhs::eorhs(41usize))
      .expect("the ledger records rather than propagates");
  },
  [Call::UnexpectedEndOfRhs(41)]
);

// ══════════════════════════════════════════════════════════════════════════════
// `ParseState` — 18 public methods
// ══════════════════════════════════════════════════════════════════════════════
//
// `ParseState::new` is `pub(super)`, so the subject is **reached**, not constructed: a real
// `map_with` callback is the door, and it hands the state by value. Every one of the seventeen
// public methods is reachable that way; none of them turned out to be a public method no
// consumer can call.
//
// That door is what makes EVERY item in this section `map`-gated, the `state_case!`
// INVOCATIONS included. The macro's own body spells `.map_with(...)`, so it is gated too — and
// a gated `macro_rules!` does not merely expand to nothing when its feature is off, it stops
// existing, so an ungated invocation of it is a hard `cannot find macro` rather than an empty
// expansion. The twelve rows below shipped without their attribute and reddened
// `--no-default-features --features std,logos,trace --tests` (the third `feature combinations`
// leg) for two commits on main; see #215. A new row here needs the attribute for the same
// reason its neighbours carry one.

/// One `ParseState` row: a real parse, a real `map_with` callback, one forwarded call inside a
/// cleared window. `$st` is the state binding the callback receives by value.
#[cfg(feature = "map")]
macro_rules! state_case {
  ($(#[$attr:meta])* $name:ident, $method:literal, |$st:ident| $body:block, $want:expr) => {
    $(#[$attr])*
    #[test]
    fn $name() {
      let log = Log::default();
      let seen: Rc<RefCell<Vec<Call>>> = Rc::new(RefCell::new(Vec::new()));
      {
        let log = log.clone();
        let seen = seen.clone();
        let ctx = ParserContext::new(Ledger::new(log.clone()));
        let result: Result<(), Note> = Parser::with_context(ctx)
          .apply(move |inp: &mut Ir<'_, '_>| {
            Empty::new()
              .map_with(|(), mut state| {
                let _ = log.take();
                {
                  let $st = &mut state;
                  $body
                }
                *seen.borrow_mut() = log.take();
              })
              .parse_input(inp)
          })
          .parse_str(SOURCE);
        result.expect("the ledger records rather than propagates");
      }
      let got = core::mem::take(&mut *seen.borrow_mut());
      landed("ParseState", $method, &got, &$want);
    }
  };
}

/// `emitter_ref` reads the parse's own emitter from inside the callback, and reads it without
/// emitting.
#[cfg(feature = "map")]
#[test]
fn state_emitter_ref() {
  let log = Log::default();
  let seen: Rc<RefCell<Vec<Call>>> = Rc::new(RefCell::new(Vec::new()));
  {
    let log = log.clone();
    let seen = seen.clone();
    let ctx = ParserContext::new(Ledger::new(log.clone()));
    let result: Result<(), Note> = Parser::with_context(ctx)
      .apply(move |inp: &mut Ir<'_, '_>| {
        Empty::new()
          .map_with(|(), state: St<'_, '_, '_>| {
            let _ = log.take();
            assert!(
              state.emitter_ref().log().is(&log),
              "`ParseState::emitter_ref` handed back an emitter other than the parse's own"
            );
            *seen.borrow_mut() = log.take();
          })
          .parse_input(inp)
      })
      .parse_str(SOURCE);
    result.expect("the ledger records rather than propagates");
  }
  let got = core::mem::take(&mut *seen.borrow_mut());
  landed("ParseState", "emitter_ref", &got, &[]);
}

#[cfg(feature = "map")]
state_case!(
  state_emit_lexer_error,
  "emit_lexer_error",
  |st| {
    st.emit_lexer_error(Spanned::new(sp(42, 43), ()))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitLexerError(sp(42, 43))]
);

#[cfg(feature = "map")]
state_case!(
  state_emit_unexpected_token,
  "emit_unexpected_token",
  |st| {
    st.emit_unexpected_token(UnexpectedToken::new(sp(44, 45)))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitUnexpectedToken(sp(44, 45))]
);

#[cfg(feature = "map")]
state_case!(
  state_emit_error,
  "emit_error",
  |st| {
    st.emit_error(Spanned::new(sp(11, 12), Note::Alpha))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitError(sp(11, 12), Note::Alpha)]
);

#[cfg(feature = "map")]
state_case!(
  state_emit_warning,
  "emit_warning",
  |st| {
    st.emit_warning(Spanned::new(sp(13, 14), Note::Beta))
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitWarning(sp(13, 14), Note::Beta)]
);

#[cfg(feature = "map")]
state_case!(
  state_emit_skipped_region,
  "emit_skipped_region",
  |st| {
    st.emit_skipped_region(sp(46, 47), 8)
      .expect("the ledger records rather than propagates");
  },
  [Call::EmitSkippedRegion(sp(46, 47), 8)]
);

#[cfg(feature = "map")]
state_case!(
  state_enter_label,
  "enter_label",
  |st| {
    st.enter_label(LABEL);
  },
  [Call::EnterLabel(LABEL)]
);

#[cfg(feature = "map")]
state_case!(
  state_exit_label,
  "exit_label",
  |st| {
    st.exit_label();
  },
  [Call::ExitLabel]
);

#[cfg(feature = "map")]
state_case!(
  state_emitter_bound_source,
  "emitter_bound_source",
  |st| {
    assert_eq!(
      st.emitter_bound_source(),
      Some(bound_identity()),
      "`ParseState::emitter_bound_source` did not report the emitter's own binding"
    );
  },
  [Call::BoundSource]
);

#[cfg(feature = "map")]
state_case!(
  state_cst_start,
  "cst_start",
  |st| {
    assert_eq!(
      st.cst_start(KIND_START),
      inert_mark(),
      "`ParseState::cst_start` did not hand back the mark the emitter minted"
    );
  },
  [Call::CstStart(KIND_START)]
);

#[cfg(feature = "map")]
state_case!(
  state_cst_finish,
  "cst_finish",
  |st| {
    st.cst_finish(KIND_FINISH);
  },
  [Call::CstFinish(KIND_FINISH)]
);

#[cfg(feature = "map")]
state_case!(
  state_cst_demote,
  "cst_demote",
  |st| {
    st.cst_demote(inert_mark(), KIND_DEMOTE);
  },
  [Call::CstDemote(inert_mark(), KIND_DEMOTE)]
);

#[cfg(feature = "map")]
state_case!(
  state_cst_mark,
  "cst_mark",
  |st| {
    assert_eq!(
      st.cst_mark(),
      inert_mark(),
      "`ParseState::cst_mark` did not hand back the mark the emitter minted"
    );
  },
  [Call::CstMark]
);

#[cfg(feature = "map")]
state_case!(
  state_cst_start_at,
  "cst_start_at",
  |st| {
    st.cst_start_at(inert_mark(), KIND_START_AT);
  },
  [Call::CstStartAt(inert_mark(), KIND_START_AT)]
);

// ── The four non-emitter forwards on `ParseState` ────────────────────────────
//
// `span`, `slice`, `state` and `state_mut` forward to the *input handle* rather than to the
// emitter, and each has a plausible wrong target one identifier away: `span_since(&start)` and
// `slice_since(&start)` describe the region the sub-parser consumed, while the handle's own
// `span()`/`slice()` describe the token at the front. The two are equal for exactly the inputs
// that consume one token, so the assertions below run over a two-token region.

/// Consumes both tokens of [`SOURCE`], so a callback mapped over it covers the whole buffer and
/// a `span_since(start)` / `span()` mix-up is visible. A plain `fn` parser rather than
/// `any().then(any())`, so the two rows below need no combinator family beyond `map`.
#[cfg(feature = "map")]
fn two_tokens(inp: &mut Ir<'_, '_>) -> Result<(), Note> {
  inp.try_expect(|_| true)?;
  inp.try_expect(|_| true)?;
  Ok(())
}

/// `span` covers the region the sub-parser consumed, not the token at the front.
#[cfg(feature = "map")]
#[test]
fn state_span() {
  let ctx = ParserContext::new(Ledger::new(Log::default()));
  let result: Result<(), Note> = Parser::with_context(ctx)
    .apply(move |inp: &mut Ir<'_, '_>| {
      two_tokens
        .map_with(|(), state: St<'_, '_, '_>| {
          assert_eq!(
            state.span(),
            sp(0, 5),
            "`ParseState::span` must cover the region the sub-parser consumed (`span_since` \
             from the callback's start cursor), not the front token's own span"
          );
        })
        .parse_input(inp)
    })
    .parse_str(SOURCE);
  result.expect("the ledger records rather than propagates");
}

/// `slice` cuts the region the sub-parser consumed, for the same reason.
#[cfg(feature = "map")]
#[test]
fn state_slice() {
  let ctx = ParserContext::new(Ledger::new(Log::default()));
  let result: Result<(), Note> = Parser::with_context(ctx)
    .apply(move |inp: &mut Ir<'_, '_>| {
      two_tokens
        .map_with(|(), state: St<'_, '_, '_>| {
          assert_eq!(
            state.slice(),
            Some("12 34"),
            "`ParseState::slice` must cut the region the sub-parser consumed (`slice_since` \
             from the callback's start cursor), not the front token's own slice"
          );
        })
        .parse_input(inp)
    })
    .parse_str(SOURCE);
  result.expect("the ledger records rather than propagates");
}

// The `state` / `state_mut` pair needs a lexer whose state is **not** `()`. The shared
// `TestLexer` is `LogosLexer<'_, Token>`, whose `State` is the token's logos `Extras` — and
// `common::Token` declares none, so its state is the unit. Over a unit state every candidate
// slot a mis-written forward could reach holds the same value *and* (being zero-sized) can share
// an address, so neither a value assertion nor a pointer assertion discriminates anything. So
// this pair is driven over a probe token that declares `extras`, which makes the state a real
// `u32` that a write can change and a read can see.

/// The probe lexer's state: a `u32` dial, so a write is a change a read can see.
#[cfg(feature = "map")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Dial(u32);

#[cfg(feature = "map")]
impl tokora::State for Dial {
  type Error = ();

  fn check(&self) -> Result<(), ()> {
    Ok(())
  }
}

/// The probe token's kind. One variant; the interesting part is the state beside it.
#[cfg(feature = "map")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NumKind;

#[cfg(feature = "map")]
impl core::fmt::Display for NumKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("number")
  }
}

/// The probe lexer's token: one numeral, and a [`Dial`] of lexer state.
#[cfg(feature = "map")]
#[derive(Debug, Clone, PartialEq, tokora::logos::Logos)]
#[logos(crate = tokora::logos, extras = Dial, skip r"[ \t\r\n]+")]
enum ProbeToken {
  #[regex(r"-?[0-9]+")]
  Num,
}

#[cfg(feature = "map")]
impl core::fmt::Display for ProbeToken {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("number")
  }
}

#[cfg(feature = "map")]
impl TokenTrait<'_> for ProbeToken {
  type Kind = NumKind;
  type Error = ();

  fn kind(&self) -> NumKind {
    NumKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

#[cfg(feature = "map")]
type ProbeLexer<'a> = tokora::lexer::LogosLexer<'a, ProbeToken>;

#[cfg(feature = "map")]
type ProbeIr<'inp, 'c> = InputRef<
  'inp,
  'c,
  ProbeLexer<'inp>,
  ParserContext<'inp, ProbeLexer<'inp>, tokora::emitter::Ignored>,
>;

#[cfg(feature = "map")]
type ProbeSt<'a, 'inp, 'c> = ParseState<
  'a,
  'inp,
  'c,
  ProbeLexer<'inp>,
  ParserContext<'inp, ProbeLexer<'inp>, tokora::emitter::Ignored>,
>;

#[cfg(feature = "map")]
fn probe_token(inp: &mut ProbeIr<'_, '_>) -> Result<(), ()> {
  inp.try_expect(|_| true)?;
  Ok(())
}

/// `state` and `state_mut` reach the **same** lexer state: a write through the mutable door is
/// visible through the shared one, and the shared one starts at the state the parse was entered
/// with. A forward onto any other slot fails one half or the other.
#[cfg(feature = "map")]
#[test]
fn state_state_and_state_mut() {
  const ENTRY: Dial = Dial(0x0BAD_C0DE);
  const WRITTEN: Dial = Dial(0x0F1E_2D3C);

  let ctx: ParserContext<'_, ProbeLexer<'_>, tokora::emitter::Ignored> =
    ParserContext::new(tokora::emitter::Ignored::default());
  let result: Result<(), ()> = Parser::with_context(ctx)
    .apply(move |inp: &mut ProbeIr<'_, '_>| {
      probe_token
        .map_with(move |(), mut state: ProbeSt<'_, '_, '_>| {
          assert_eq!(
            *state.state(),
            ENTRY,
            "`ParseState::state` must read the parse's own lexer state, which this parse was \
             entered with"
          );
          *state.state_mut() = WRITTEN;
          assert_eq!(
            *state.state(),
            WRITTEN,
            "`ParseState::state_mut` must hand back the same lexer state `state` reads — a \
             write through the mutable door is invisible through the shared one only if the two \
             forwards reach different slots"
          );
        })
        .parse_input(inp)
    })
    .parse_str_with_state(SOURCE, ENTRY);
  result.expect("the ignoring emitter never propagates");
}

// ══════════════════════════════════════════════════════════════════════════════
// FORWARDING_CENSUS
// ══════════════════════════════════════════════════════════════════════════════
//
// The suite above is only total if its row list matches each receiver's public surface. A
// forward added to `view.rs`, `emit.rs` or `parse_state/mod.rs` must fail here until a row
// exists; a row for a method that no longer exists must fail in the other direction. `grep
// FORWARDING_CENSUS` finds the anchor.
//
// Gated on `pratt` because the census reads source **text**, where the two pratt forwards are
// always present, while their rows are compiled only when the feature is on. The gate is on
// every item of the section, not only on the `#[test]`: the six constants and the two functions
// serve that one test and nothing else, so gating the test alone leaves them dead — eight
// `never used` warnings on any leg without `pratt`, which is exactly where a real warning would
// have to be spotted.

#[cfg(feature = "pratt")]
const VIEW_SOURCE: &str = include_str!("../src/emitter/view.rs");
#[cfg(feature = "pratt")]
const HANDLE_SOURCE: &str = include_str!("../src/input/input_ref/emit.rs");
#[cfg(feature = "pratt")]
const STATE_SOURCE: &str = include_str!("../src/parse_state/mod.rs");

/// The rows this suite carries for `EmitterView`.
#[cfg(feature = "pratt")]
const VIEW_COVERED: &[&str] = &[
  "new",
  "reborrow",
  "emitter_ref",
  "emit_lexer_error",
  "emit_unexpected_token",
  "emit_error",
  "emit_warning",
  "emit_skipped_region",
  "enter_label",
  "exit_label",
  "bound_source",
  "cst_start",
  "cst_finish",
  "cst_demote",
  "cst_mark",
  "cst_start_at",
  "emit_too_few",
  "emit_too_many",
  "emit_full_container",
  "emit_missing_separator",
  "emit_missing_element",
  "emit_missing_leading_separator",
  "emit_missing_trailing_separator",
  "emit_unexpected_leading_separator",
  "emit_unexpected_trailing_separator",
  "emit_unclosed",
  "emit_unexpected_end_of_lhs",
  "emit_unexpected_end_of_rhs",
];

/// The rows this suite carries for `InputRef`'s emit surface.
#[cfg(feature = "pratt")]
const HANDLE_COVERED: &[&str] = &[
  "emitter_ref",
  "emit_lexer_error",
  "emit_unexpected_token",
  "emit_error",
  "emit_warning",
  "emit_skipped_region",
  "enter_label",
  "exit_label",
  "emitter_bound_source",
  "cst_start",
  "cst_finish",
  "cst_demote",
  "cst_mark",
  "cst_start_at",
  "emit_too_few",
  "emit_too_many",
  "emit_full_container",
  "emit_missing_separator",
  "emit_missing_element",
  "emit_missing_leading_separator",
  "emit_missing_trailing_separator",
  "emit_unexpected_leading_separator",
  "emit_unexpected_trailing_separator",
  "emit_unclosed",
  "emit_unexpected_end_of_lhs",
  "emit_unexpected_end_of_rhs",
];

/// The rows this suite carries for `ParseState`.
#[cfg(feature = "pratt")]
const STATE_COVERED: &[&str] = &[
  "span",
  "emitter_ref",
  "emit_lexer_error",
  "emit_unexpected_token",
  "emit_error",
  "emit_warning",
  "emit_skipped_region",
  "enter_label",
  "exit_label",
  "emitter_bound_source",
  "cst_start",
  "cst_finish",
  "cst_demote",
  "cst_mark",
  "cst_start_at",
  "state",
  "state_mut",
  "slice",
];

/// Every `pub fn` / `pub const fn` declared on a non-comment line of `src`, by name. Restricted
/// visibilities (`pub(crate)`, `pub(super)`) are not part of the forwarding surface and are not
/// collected.
#[cfg(feature = "pratt")]
fn declared_forwards(src: &str) -> BTreeSet<String> {
  let mut out = BTreeSet::new();
  for line in src.lines() {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
      continue;
    }
    let rest = match trimmed.strip_prefix("pub fn ") {
      Some(rest) => rest,
      None => match trimmed.strip_prefix("pub const fn ") {
        Some(rest) => rest,
        None => continue,
      },
    };
    let end = rest.find(['(', '<']).unwrap_or(rest.len());
    out.insert(rest[..end].trim().to_string());
  }
  out
}

#[cfg(feature = "pratt")]
#[track_caller]
fn census(receiver: &str, src: &str, covered: &[&str], expected: usize) {
  let declared = declared_forwards(src);
  let covered: BTreeSet<String> = covered.iter().map(|m| (*m).to_string()).collect();

  let uncovered: Vec<&String> = declared.difference(&covered).collect();
  assert!(
    uncovered.is_empty(),
    "FORWARDING_CENSUS drift: {uncovered:?} declared on `{receiver}` but carried by no row. A \
     forward with no row can be re-pointed at a sibling with nothing in the crate noticing — \
     add the row in the same commit (grep FORWARDING_CENSUS)."
  );

  let stale: Vec<&String> = covered.difference(&declared).collect();
  assert!(
    stale.is_empty(),
    "FORWARDING_CENSUS drift: {stale:?} carried by a row but no longer declared on \
     `{receiver}`. Drop the row, or restore the method (grep FORWARDING_CENSUS)."
  );

  assert_eq!(
    declared.len(),
    expected,
    "FORWARDING_CENSUS drift: `{receiver}` declares {} public method(s), expected {expected}. \
     Update this count in the same commit as the surface change (grep FORWARDING_CENSUS).",
    declared.len()
  );
}

/// FORWARDING_CENSUS — every public method of all three receivers carries a row above.
#[cfg(feature = "pratt")]
#[test]
fn forwarding_census_every_public_forward_is_covered() {
  census("EmitterView", VIEW_SOURCE, VIEW_COVERED, 28);
  census("InputRef", HANDLE_SOURCE, HANDLE_COVERED, 26);
  census("ParseState", STATE_SOURCE, STATE_COVERED, 18);
}
