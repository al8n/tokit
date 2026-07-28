#![cfg(all(feature = "std", feature = "logos"))]

//! The `Verbose` write-path conformance suite: every emit channel records on the shared
//! emission log, so `checkpoint`, `rewind`, `labels()` and `diagnostics()` are exact for
//! *all* of them rather than for the core three.
//!
//! A `Verbose` channel that writes its payload map directly instead of routing through the
//! private `record`/`record_warning`/`record_hole` chokepoint leaves the log unmoved. Four
//! observable failures follow, and each has a named history below:
//!
//! - the mark does not advance, so a branch that emitted only through that channel looks
//!   empty to `checkpoint` (`e1a`);
//! - a rollback cannot see the emission, so the diagnostic survives as a phantom (`e1b`);
//! - worse, when a logged diagnostic shares the span, the rollback pops the *newest*
//!   payload — the unlogged one — and keeps the logged one it claimed to unwind: payload
//!   inversion (`e1c`);
//! - `diagnostics()` replays the log, so it under-reports and mispairs payloads with the
//!   wrong log entry's labels (`e1d`).
//!
//! The suite is written against the emitter's public surface plus the stable transaction
//! guard (`InputRef::begin`), not the `unstable-raw` save/restore pair, so it runs in the
//! default feature set.
//!
//! One deliberate limit, stated rather than papered over: `emit_missing_separator`,
//! `emit_missing_leading_separator` and `emit_missing_trailing_separator` all convert
//! through the single `From<MissingTokenOf>` blanket, so their payloads are
//! indistinguishable by construction. A per-channel payload assertion here can pin the
//! payload *family*, not which of those three produced it; the three are told apart by the
//! per-method source census instead.

mod common;

use core::ptr;
use std::collections::BTreeSet;

use tokora::{
  Emitter, InputRef, Parse, Parser, ParserContext,
  delimiter::DelimiterKind,
  emitter::{
    DiagnosticKind, FullContainerEmitter, MissingLeadingSeparatorEmitter,
    MissingTrailingSeparatorEmitter, PrattEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter,
    UnclosedEmitter, UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
    Verbose,
  },
  error::{
    Unclosed, UnexpectedEoLhs, UnexpectedEoRhs,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, SeparatorPosition, UnexpectedToken},
  },
  span::{SimpleSpan, Spanned},
  utils::CowStr,
};

use common::TestLexer;

// ── The conformance error type ────────────────────────────────────────────────
//
// One variant per payload family, so a diagnostic that survives (or vanishes) can be
// identified by value rather than by mere map membership.

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfError {
  /// The logged control payload, emitted through `emit_error` — the twin the rollback
  /// histories pair against a specialized emission at the same span.
  Control,
  /// A second `emit_error` payload, so that channel's own conformance row can tell its
  /// emission apart from the control twin it is paired against.
  Direct,
  /// The `emit_warning` payload — the warning channel's own tier.
  Warning,
  Lexer,
  Unexpected,
  TooFew,
  TooMany,
  FullContainer,
  /// Fed by all three missing-separator conversions (one payload family, one blanket).
  MissingToken,
  MissingElement,
  Separated(SeparatorPosition),
  Unclosed,
  EndOfLhs,
  EndOfRhs,
}

impl From<()> for ConfError {
  fn from(_: ()) -> Self {
    ConfError::Lexer
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for ConfError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    ConfError::Unexpected
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for ConfError {
  fn from(_: TooFew<S, Lang>) -> Self {
    ConfError::TooFew
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for ConfError {
  fn from(_: TooMany<S, Lang>) -> Self {
    ConfError::TooMany
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for ConfError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    ConfError::FullContainer
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for ConfError {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    ConfError::MissingToken
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for ConfError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    ConfError::MissingElement
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, Kind, S, Lang>> for ConfError {
  fn from(err: SeparatedError<'a, T, Kind, S, Lang>) -> Self {
    ConfError::Separated(err.position())
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for ConfError {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    ConfError::Unclosed
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for ConfError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    ConfError::Unclosed
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEoLhs<O, Lang>> for ConfError {
  fn from(_: UnexpectedEoLhs<O, Lang>) -> Self {
    ConfError::EndOfLhs
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEoRhs<O, Lang>> for ConfError {
  fn from(_: UnexpectedEoRhs<O, Lang>) -> Self {
    ConfError::EndOfRhs
  }
}

// ── Emitter-level drivers ─────────────────────────────────────────────────────
//
// Every driver takes `&mut Verbose<ConfError>`, so the same call works on a bare emitter,
// on `inp.emitter()`, and on a transaction guard's `tx.emitter()`.

type Vb = Verbose<ConfError>;

fn mark(v: &Vb) -> u64 {
  <Vb as Emitter<'_, TestLexer<'_>>>::checkpoint(v)
}

fn enter(v: &mut Vb, label: &'static str) {
  <Vb as Emitter<'_, TestLexer<'_>>>::enter_label(v, label);
}

fn exit(v: &mut Vb) {
  <Vb as Emitter<'_, TestLexer<'_>>>::exit_label(v);
}

/// The logged control channel: already routed through `record` before this suite existed.
fn control(v: &mut Vb, span: SimpleSpan) {
  <Vb as Emitter<'_, TestLexer<'_>>>::emit_error(v, Spanned::new(span, ConfError::Control))
    .expect("Verbose collects rather than propagates");
}

fn too_few(v: &mut Vb, span: SimpleSpan) {
  <Vb as TooFewEmitter<'_, TestLexer<'_>>>::emit_too_few(v, TooFew::new(span, 1, 3))
    .expect("Verbose collects rather than propagates");
}

fn full_container(v: &mut Vb, span: SimpleSpan) {
  <Vb as FullContainerEmitter<'_, TestLexer<'_>>>::emit_full_container(
    v,
    FullContainer::new(span, 10, 5),
  )
  .expect("Verbose collects rather than propagates");
}

// ── Contexts ──────────────────────────────────────────────────────────────────

type ConfCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Vb>;

fn conf_ctx() -> ConfCtx<'static> {
  ParserContext::new(Verbose::new())
}

type ConfIr<'inp, 'c> = InputRef<'inp, 'c, TestLexer<'inp>, ConfCtx<'inp>>;

/// Drives `run` over `"12 34"` through a `Verbose` context, panicking on a propagated error.
fn drive(run: impl for<'inp, 'c> FnOnce(&mut ConfIr<'inp, 'c>) -> Result<(), ConfError>) {
  let mut run = Some(run);
  let r: Result<(), ConfError> = Parser::with_context(conf_ctx())
    .apply(move |inp: &mut ConfIr<'_, '_>| (run.take().expect("driven once"))(inp))
    .parse_str("12 34");
  r.expect("the conformance drivers never propagate");
}

// ═══════════════════════════════════════════════════════════════════════════════
// The write-path histories
// ═══════════════════════════════════════════════════════════════════════════════

/// Every channel's emission moves the shared mark by exactly one and snapshots the open
/// labels — the specialized channels no less than the logged ones.
#[test]
fn e1a_flipped_specialized_emissions_advance_the_mark_and_snapshot_labels() {
  let mut v = Verbose::<ConfError>::new();
  let few = SimpleSpan::new(0usize, 1usize);
  let full = SimpleSpan::new(2usize, 3usize);
  let ctrl = SimpleSpan::new(4usize, 5usize);

  enter(&mut v, "e1a");
  let m0 = mark(&v);

  too_few(&mut v, few);
  assert_eq!(
    mark(&v),
    m0 + 1,
    "a specialized emission must advance the emission mark"
  );

  full_container(&mut v, full);
  assert_eq!(
    mark(&v),
    m0 + 2,
    "the second specialized emission must advance the mark again"
  );

  control(&mut v, ctrl);
  assert_eq!(
    mark(&v),
    m0 + 3,
    "the logged control channel keeps its one-per-emission rate"
  );
  exit(&mut v);

  assert_eq!(
    v.labels().get(&few),
    Some(&vec![vec!["e1a"]]),
    "a specialized emission captures the open-label snapshot"
  );
  assert_eq!(
    v.labels().get(&full),
    Some(&vec![vec!["e1a"]]),
    "a specialized emission captures the open-label snapshot"
  );
  assert_eq!(
    v.labels().get(&ctrl),
    Some(&vec![vec!["e1a"]]),
    "the logged control channel is unchanged"
  );
}

/// A rolled-back branch leaves nothing behind on any channel: the specialized emission is
/// rewound exactly like the logged one beside it.
#[test]
fn e1b_flipped_rollback_leaves_no_phantom() {
  drive(|inp| {
    let s = SimpleSpan::new(0usize, 1usize);
    let m0 = mark(inp.emitter());

    {
      let mut tx = inp.begin();
      too_few(tx.emitter(), s);
      control(tx.emitter(), s);
      tx.rollback();
    }

    assert_eq!(
      mark(inp.emitter()),
      m0,
      "the mark returns to the guard entry"
    );
    assert!(
      !inp.emitter().errors().contains_key(&s),
      "a rolled-back branch leaves no diagnostic: {:?}",
      inp.emitter().errors().get(&s)
    );
    assert!(
      !inp.emitter().labels().contains_key(&s),
      "a rolled-back branch leaves no label snapshot"
    );
    Ok(())
  });
}

/// The payload-inversion oracle. With a logged emission (`Control`) followed by a
/// specialized one (`TooFew`) at the same span, an unlogged specialized write makes the
/// rollback pop the *newest* payload — the specialized one — and keep the logged payload it
/// claimed to unwind, while the label group is emptied. Both maps must instead come back
/// clean and stay parallel.
#[test]
fn e1c_flipped_rollback_unwinds_both_never_inverting() {
  drive(|inp| {
    let s = SimpleSpan::new(0usize, 1usize);
    let m0 = mark(inp.emitter());

    {
      let mut tx = inp.begin();
      control(tx.emitter(), s);
      too_few(tx.emitter(), s);
      tx.rollback();
    }

    assert_eq!(
      mark(inp.emitter()),
      m0,
      "the mark returns to the guard entry"
    );
    assert!(
      !inp.emitter().errors().contains_key(&s),
      "the rollback must remove both emissions, not swap which one it keeps: {:?}",
      inp.emitter().errors().get(&s)
    );
    // The parallel-map invariant, observably: the payload group and the label group at a
    // span must vanish together. Inversion breaks it — the label group empties while a
    // payload survives.
    assert_eq!(
      inp.emitter().errors().get(&s).map(Vec::len),
      inp.emitter().labels().get(&s).map(Vec::len),
      "payload group and label snapshots must stay parallel across a rollback"
    );
    Ok(())
  });
}

/// `diagnostics()` replays the store: every recorded diagnostic, in emission order, each
/// paired with its own payload slot rather than a neighbour's.
#[test]
fn e1d_flipped_replay_agrees_with_the_store() {
  let mut v = Verbose::<ConfError>::new();
  let s = SimpleSpan::new(0usize, 1usize);

  too_few(&mut v, s); // slot 0 — specialized
  control(&mut v, s); // slot 1 — logged

  assert_eq!(
    v.errors()[&s].len(),
    2,
    "both emissions are in the store at one span"
  );

  let replayed: Vec<_> = v.diagnostics().collect();
  assert_eq!(
    replayed.len(),
    2,
    "replay yields every recorded diagnostic: {:?}",
    replayed.iter().map(|d| d.payload()).collect::<Vec<_>>()
  );

  let group = &v.errors()[&s];
  assert_eq!(replayed[0].payload(), Some(&ConfError::TooFew));
  assert_eq!(replayed[1].payload(), Some(&ConfError::Control));
  assert!(
    ptr::eq(
      replayed[0].payload().expect("an error payload"),
      &group[0] as *const ConfError
    ),
    "slot 0 replays the store's slot 0"
  );
  assert!(
    ptr::eq(
      replayed[1].payload().expect("an error payload"),
      &group[1] as *const ConfError
    ),
    "slot 1 replays the store's slot 1"
  );
}

/// Rollback-then-retry records once, not twice: the abandoned attempt's diagnostic is gone
/// before the retry re-emits it.
#[test]
fn retry_after_rollback_records_exactly_once() {
  drive(|inp| {
    let s = SimpleSpan::new(0usize, 1usize);

    {
      let mut tx = inp.begin();
      too_few(tx.emitter(), s);
      tx.rollback();
    }
    too_few(inp.emitter(), s);

    assert_eq!(
      inp.emitter().errors().get(&s).map(Vec::len),
      Some(1),
      "the abandoned attempt must not leave a duplicate behind"
    );
    Ok(())
  });
}

// ═══════════════════════════════════════════════════════════════════════════════
// The per-channel conformance matrix
// ═══════════════════════════════════════════════════════════════════════════════
//
// Every emit channel obeys the same two laws, so every channel gets the same two tests. The
// rows are declared through one macro, which makes the covered set *data* — CONFORMANCE_CENSUS
// at the bottom of this file compares it against the channels the emitter actually declares,
// in both directions, so a twelfth channel cannot ship uncovered and a deleted row cannot go
// unnoticed.

/// What one channel's emission should look like once recorded: the payload family it converts
/// into, or — for the recovery-hole channel, which has no payload — the skipped-token count.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Expect {
  Error(ConfError),
  Warning(ConfError),
  Hole(usize),
}

/// T-a — the channel records on the shared log: the mark advances by exactly one, the payload
/// lands in its family's group at the expected span, the open-label snapshot lands beside it,
/// and the replay yields exactly that one diagnostic, by slot identity and exact size.
fn t_a(span: SimpleSpan, expect: Expect, invoke: impl FnOnce(&mut Vb)) {
  let mut v = Verbose::<ConfError>::new();
  enter(&mut v, "conf-ctx");
  let m0 = mark(&v);

  invoke(&mut v);

  assert_eq!(
    mark(&v),
    m0 + 1,
    "the emission must advance the shared mark by exactly one"
  );

  let labels = match &expect {
    Expect::Error(want) => {
      let group = v
        .errors()
        .get(&span)
        .unwrap_or_else(|| panic!("no error group recorded at {span:?}"));
      assert_eq!(group.len(), 1, "exactly one payload in the span's group");
      assert_eq!(
        &group[0], want,
        "the channel's conversion produced the wrong payload family"
      );
      v.labels().get(&span)
    }
    Expect::Warning(want) => {
      let group = v
        .warnings()
        .get(&span)
        .unwrap_or_else(|| panic!("no warning group recorded at {span:?}"));
      assert_eq!(group.len(), 1, "exactly one payload in the span's group");
      assert_eq!(
        &group[0], want,
        "the channel's conversion produced the wrong payload family"
      );
      v.warning_labels().get(&span)
    }
    Expect::Hole(want) => {
      let group = v
        .skipped_regions()
        .get(&span)
        .unwrap_or_else(|| panic!("no hole group recorded at {span:?}"));
      assert_eq!(group.len(), 1, "exactly one record in the span's group");
      assert_eq!(&group[0], want, "the recorded skipped-token count");
      v.skipped_region_labels().get(&span)
    }
  };
  assert_eq!(
    labels,
    Some(&vec![vec!["conf-ctx"]]),
    "the label snapshot is captured in lockstep with the payload"
  );
  exit(&mut v);

  assert_eq!(
    v.diagnostics().len(),
    1,
    "replay is exact-size and yields the one recorded diagnostic"
  );
  assert_eq!(v.diagnostics().size_hint(), (1, Some(1)));

  let d = v.diagnostics().next().expect("one recorded diagnostic");
  assert_eq!(*d.span(), span, "the replayed diagnostic keys on its span");
  match (&expect, d.kind()) {
    (Expect::Error(_), DiagnosticKind::Error(payload)) => assert!(
      core::ptr::eq(payload, &v.errors()[&span][0] as *const ConfError),
      "replay hands back the store's own slot, not a copy of a neighbour's"
    ),
    (Expect::Warning(_), DiagnosticKind::Warning(payload)) => assert!(
      core::ptr::eq(payload, &v.warnings()[&span][0] as *const ConfError),
      "replay hands back the store's own slot, not a copy of a neighbour's"
    ),
    (Expect::Hole(want), DiagnosticKind::SkippedRegion(skipped)) => {
      assert_eq!(skipped, *want, "replay carries the skipped-token count")
    }
    (expected, got) => panic!("replay kind {got:?} does not match {expected:?}"),
  }
}

/// T-b — the channel rewinds exactly: its emission advances the mark inside a guard, a
/// rollback puts the mark back and removes the emission, and the logged twin recorded at the
/// *same span* before the guard survives untouched. Pre-fix, a bypassing channel fails this on
/// both halves at once — the mark never moved, and the rollback removed the twin instead.
fn t_b(span: SimpleSpan, expect: Expect, invoke: impl FnOnce(&mut Vb)) {
  drive(move |inp| {
    control(inp.emitter(), span);
    let m1 = mark(inp.emitter());

    {
      let mut tx = inp.begin();
      invoke(tx.emitter());
      assert_eq!(
        mark(tx.emitter()),
        m1 + 1,
        "the emission must advance the mark inside the branch"
      );
      tx.rollback();
    }

    assert_eq!(
      mark(inp.emitter()),
      m1,
      "the rollback must return the mark to the guard entry"
    );
    assert_eq!(
      inp.emitter().errors().get(&span).map(Vec::len),
      Some(1),
      "the pre-guard twin survives, alone: {:?}",
      inp.emitter().errors().get(&span)
    );
    assert_eq!(
      inp.emitter().errors()[&span][0],
      ConfError::Control,
      "the surviving payload is the pre-guard twin, not the rolled-back emission"
    );
    assert_eq!(
      inp.emitter().labels().get(&span).map(Vec::len),
      Some(1),
      "the label group unwinds in lockstep with its payload group"
    );

    match &expect {
      // The error family shares the twin's map, so the len-1 check above already proves the
      // rolled-back payload is gone.
      Expect::Error(_) => {}
      Expect::Warning(_) => assert!(
        inp.emitter().warnings().get(&span).is_none(),
        "the rolled-back warning must be gone: {:?}",
        inp.emitter().warnings().get(&span)
      ),
      Expect::Hole(_) => assert!(
        inp.emitter().skipped_regions().get(&span).is_none(),
        "the rolled-back hole record must be gone: {:?}",
        inp.emitter().skipped_regions().get(&span)
      ),
    }

    let diags: Vec<_> = inp.emitter().diagnostics().collect();
    assert_eq!(diags.len(), 1, "replay agrees: exactly the surviving twin");
    assert_eq!(diags[0].payload(), Some(&ConfError::Control));
    Ok(())
  });
}

/// Declares one conformance row per emit channel: its expected span key, the payload it should
/// produce, and how to invoke it. Each row generates T-a and T-b, and every channel name is
/// collected into [`COVERED_CHANNELS`] for the census below.
macro_rules! verbose_channel_conformance {
  ($( $chan:ident => { span: $span:expr, expect: $expect:expr, invoke: $invoke:expr } ),+ $(,)?) => {
    /// The channels this suite covers — compared against the emitter's declared channels by
    /// `conformance_census_every_verbose_emit_channel_is_covered`.
    const COVERED_CHANNELS: &[&str] = &[$(stringify!($chan)),+];

    $(
      mod $chan {
        use super::*;

        #[test]
        fn records_on_the_shared_log() {
          t_a($span, $expect, $invoke);
        }

        #[test]
        fn rewinds_exactly_and_keeps_the_presave_twin() {
          t_b($span, $expect, $invoke);
        }
      }
    )+
  };
}

verbose_channel_conformance! {
  // ── The core channels: already law-pinned elsewhere, included so the template is total
  // and a regression looks the same wherever it lands.
  emit_lexer_error => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Error(ConfError::Lexer),
    invoke: |v: &mut Vb| {
      <Vb as Emitter<'_, TestLexer<'_>>>::emit_lexer_error(
        v,
        Spanned::new(SimpleSpan::new(0usize, 2usize), ()),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_error => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Error(ConfError::Direct),
    invoke: |v: &mut Vb| {
      <Vb as Emitter<'_, TestLexer<'_>>>::emit_error(
        v,
        Spanned::new(SimpleSpan::new(0usize, 2usize), ConfError::Direct),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_unexpected_token => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Error(ConfError::Unexpected),
    invoke: |v: &mut Vb| {
      <Vb as Emitter<'_, TestLexer<'_>>>::emit_unexpected_token(
        v,
        UnexpectedToken::new(SimpleSpan::new(0usize, 2usize)),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_warning => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Warning(ConfError::Warning),
    invoke: |v: &mut Vb| {
      <Vb as Emitter<'_, TestLexer<'_>>>::emit_warning(
        v,
        Spanned::new(SimpleSpan::new(0usize, 2usize), ConfError::Warning),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_skipped_region => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Hole(3),
    invoke: |v: &mut Vb| {
      <Vb as Emitter<'_, TestLexer<'_>>>::emit_skipped_region(
        v,
        SimpleSpan::new(0usize, 2usize),
        3,
      )
      .expect("Verbose collects rather than propagates");
    }
  },

  // ── The specialized channels: the eleven this round routed through the chokepoint.
  emit_full_container => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Error(ConfError::FullContainer),
    invoke: |v: &mut Vb| full_container(v, SimpleSpan::new(0usize, 2usize))
  },
  emit_missing_separator => {
    span: SimpleSpan::new(0usize, 0usize),
    expect: Expect::Error(ConfError::MissingToken),
    invoke: |v: &mut Vb| {
      <Vb as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_separator(
        v,
        CowStr::from_static("comma"),
        MissingToken::new(0usize),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_missing_element => {
    span: SimpleSpan::new(0usize, 0usize),
    expect: Expect::Error(ConfError::MissingElement),
    invoke: |v: &mut Vb| {
      <Vb as SeparatedEmitter<'_, TestLexer<'_>>>::emit_missing_element(
        v,
        MissingSyntax::new(0usize),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_missing_leading_separator => {
    span: SimpleSpan::new(0usize, 0usize),
    expect: Expect::Error(ConfError::MissingToken),
    invoke: |v: &mut Vb| {
      <Vb as MissingLeadingSeparatorEmitter<'_, TestLexer<'_>>>::emit_missing_leading_separator(
        v,
        CowStr::from_static("comma"),
        MissingToken::new(0usize),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_missing_trailing_separator => {
    span: SimpleSpan::new(0usize, 0usize),
    expect: Expect::Error(ConfError::MissingToken),
    invoke: |v: &mut Vb| {
      <Vb as MissingTrailingSeparatorEmitter<'_, TestLexer<'_>>>::emit_missing_trailing_separator(
        v,
        CowStr::from_static("comma"),
        MissingToken::new(0usize),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_unexpected_leading_separator => {
    span: SimpleSpan::new(0usize, 1usize),
    expect: Expect::Error(ConfError::Separated(SeparatorPosition::Leading)),
    invoke: |v: &mut Vb| {
      <Vb as UnexpectedLeadingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_leading_separator(
        v,
        CowStr::from_static("comma"),
        UnexpectedToken::new(SimpleSpan::new(0usize, 1usize)),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_unexpected_trailing_separator => {
    span: SimpleSpan::new(0usize, 1usize),
    expect: Expect::Error(ConfError::Separated(SeparatorPosition::Trailing)),
    invoke: |v: &mut Vb| {
      <Vb as UnexpectedTrailingSeparatorEmitter<'_, TestLexer<'_>>>::emit_unexpected_trailing_separator(
        v,
        CowStr::from_static("comma"),
        UnexpectedToken::new(SimpleSpan::new(0usize, 1usize)),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_too_few => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Error(ConfError::TooFew),
    invoke: |v: &mut Vb| too_few(v, SimpleSpan::new(0usize, 2usize))
  },
  emit_too_many => {
    span: SimpleSpan::new(0usize, 2usize),
    expect: Expect::Error(ConfError::TooMany),
    invoke: |v: &mut Vb| {
      <Vb as TooManyEmitter<'_, TestLexer<'_>>>::emit_too_many(
        v,
        TooMany::new(SimpleSpan::new(0usize, 2usize), 10, 5),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_unexpected_end_of_lhs => {
    span: SimpleSpan::new(0usize, 0usize),
    expect: Expect::Error(ConfError::EndOfLhs),
    invoke: |v: &mut Vb| {
      <Vb as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_lhs(
        v,
        UnexpectedEoLhs::eolhs(0usize),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  emit_unexpected_end_of_rhs => {
    span: SimpleSpan::new(0usize, 0usize),
    expect: Expect::Error(ConfError::EndOfRhs),
    invoke: |v: &mut Vb| {
      <Vb as PrattEmitter<'_, TestLexer<'_>>>::emit_unexpected_end_of_rhs(
        v,
        UnexpectedEoRhs::eorhs(0usize),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
  // The control row: `emit_unclosed` already routed through the chokepoint before this round,
  // so it proves the template pins a property rather than describing the repair.
  emit_unclosed => {
    span: SimpleSpan::new(0usize, 1usize),
    expect: Expect::Error(ConfError::Unclosed),
    invoke: |v: &mut Vb| {
      <Vb as UnclosedEmitter<'_, TestLexer<'_>>>::emit_unclosed(
        v,
        Unclosed::<char>::new(SimpleSpan::new(0usize, 1usize), DelimiterKind::Custom("probe"), CowStr::from_static("brace")),
      )
      .expect("Verbose collects rather than propagates");
    }
  },
}

// ── CONFORMANCE_CENSUS ────────────────────────────────────────────────────────
//
// The matrix above is only total if its row list matches the emitter's channel list. Adding
// `emit_new_thing` to any Verbose impl file must fail here until a macro row — which *generates*
// the two tests — exists; deleting a row must fail in the other direction. `grep
// CONFORMANCE_CENSUS` finds the anchor; RECORD_CENSUS in `src/emitter/impl_/verbose/` pins the
// module list this include list mirrors.

/// The Verbose channel-bearing sources, read from the test binary. Mirrors RECORD_CENSUS's
/// `SOURCES` minus the storage and the read-side iterator, neither of which declares a channel.
const VERBOSE_SOURCES: &[&str] = &[
  include_str!("../src/emitter/impl_/verbose/mod.rs"),
  include_str!("../src/emitter/impl_/verbose/full_container.rs"),
  include_str!("../src/emitter/impl_/verbose/missing_leading_separator.rs"),
  include_str!("../src/emitter/impl_/verbose/missing_trailing_separator.rs"),
  include_str!("../src/emitter/impl_/verbose/pratt.rs"),
  include_str!("../src/emitter/impl_/verbose/separator.rs"),
  include_str!("../src/emitter/impl_/verbose/too_few.rs"),
  include_str!("../src/emitter/impl_/verbose/too_many.rs"),
  include_str!("../src/emitter/impl_/verbose/unclosed.rs"),
  include_str!("../src/emitter/impl_/verbose/unexpected_leading_separator.rs"),
  include_str!("../src/emitter/impl_/verbose/unexpected_trailing_separator.rs"),
];

/// Every `fn emit_*` declared on a non-comment line of `src`, by name.
///
/// The name runs to the first `(` or `<`, which handles both the plain signatures and
/// `emit_unclosed`'s own generic parameter.
fn declared_channels(src: &str) -> Vec<String> {
  let mut out = Vec::new();
  for line in src.lines().filter(|l| !l.trim_start().starts_with("//")) {
    let mut rest = line;
    while let Some(at) = rest.find("fn emit_") {
      let after = &rest[at + "fn ".len()..];
      let end = after.find(['(', '<']).unwrap_or(after.len());
      out.push(after[..end].trim().to_string());
      rest = &after[end..];
    }
  }
  out
}

/// CONFORMANCE_CENSUS — the matrix covers every Verbose emit channel, and covers nothing that
/// is not one.
#[test]
fn conformance_census_every_verbose_emit_channel_is_covered() {
  let declared: BTreeSet<String> = VERBOSE_SOURCES
    .iter()
    .flat_map(|src| declared_channels(src))
    .collect();
  let covered: BTreeSet<String> = COVERED_CHANNELS.iter().map(|c| c.to_string()).collect();

  let uncovered: Vec<&String> = declared.difference(&covered).collect();
  assert!(
    uncovered.is_empty(),
    "CONFORMANCE_CENSUS drift: {uncovered:?} declared by a Verbose impl but not covered by the \
     matrix. A channel without a conformance row can lose its route to the record chokepoint \
     without any test noticing — add a `verbose_channel_conformance!` row in the same commit \
     (grep CONFORMANCE_CENSUS)."
  );

  let stale: Vec<&String> = covered.difference(&declared).collect();
  assert!(
    stale.is_empty(),
    "CONFORMANCE_CENSUS drift: {stale:?} covered by the matrix but no longer declared by any \
     Verbose impl. Drop the row, or restore the channel (grep CONFORMANCE_CENSUS)."
  );

  assert_eq!(
    declared.len(),
    17,
    "CONFORMANCE_CENSUS drift: {} distinct Verbose emit channel(s), expected 17. Update this \
     count together with RECORD_CENSUS's totals in the same commit (grep CONFORMANCE_CENSUS).",
    declared.len()
  );
}

// ── The sink leg ──────────────────────────────────────────────────────────────
//
// `Sink::new` const-asserts a trivia-surfacing lexer, which the shared logos `TestLexer` is
// not, so this leg carries its own byte-per-token lexer — the same shape
// `tests/parser_node.rs` uses for the identical reason.

#[cfg(feature = "rowan")]
mod sink_leg {
  use super::{ConfError, TooFewEmitter};

  use tokora::{
    Emitter, InputRef, Lexer, Parse, Parser, SimpleSpan, Token, cache::DefaultCache,
    emitter::Verbose, error::syntax::TooFew,
  };

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  struct Tok(u8);

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  struct LexErr;

  impl From<LexErr> for ConfError {
    fn from(_: LexErr) -> Self {
      ConfError::Lexer
    }
  }

  impl Token<'_> for Tok {
    type Kind = u8;
    type Error = LexErr;

    // Honest: byte-per-token, never skips a byte.
    const SURFACES_TRIVIA: bool = true;

    fn kind(&self) -> u8 {
      self.0
    }

    fn is_trivia(&self) -> bool {
      self.0 == b' '
    }
  }

  struct ByteLexer<'inp> {
    src: &'inp str,
    tok_start: usize,
    pos: usize,
    state: (),
  }

  impl<'inp> Lexer<'inp> for ByteLexer<'inp> {
    type State = ();
    type Source = str;
    type Token = Tok;
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

    fn check(&self) -> Result<(), LexErr> {
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

    fn lex(&mut self) -> Option<Result<Tok, LexErr>> {
      let byte = *self.src.as_bytes().get(self.pos)?;
      self.tok_start = self.pos;
      self.pos += 1;
      Some(Ok(Tok(byte)))
    }

    fn bump(&mut self, n: &usize) {
      self.pos += *n;
      self.tok_start = self.pos;
    }
  }

  const K_ROOT: u16 = 1;
  const K_ERR: u16 = 90;
  const K_GAP: u16 = 91;

  fn map_tok(t: &Tok) -> u16 {
    100 + t.0 as u16
  }

  type ConfSink<'inp> = tokora::cst::Sink<'inp, ByteLexer<'inp>, Verbose<ConfError>>;
  type SinkCtx<'inp, 's> = (&'s mut ConfSink<'inp>, DefaultCache<'inp, ByteLexer<'inp>>);
  type SinkIr<'inp, 's, 'c> = InputRef<'inp, 'c, ByteLexer<'inp>, SinkCtx<'inp, 's>>;

  /// Under a recording sink, a rolled-back specialized emission is undone on the inner
  /// emitter too: the sink hands its captured inner reading back at `rewind`, and the inner
  /// now has a reading to move. The sink's own event truncation is pinned elsewhere.
  #[test]
  fn sink_verbose_inner_agrees_after_rollback() {
    let mut sink: ConfSink<'_> = tokora::cst::Sink::new(Verbose::new(), map_tok, K_ERR, K_GAP);
    let span = SimpleSpan::new(0usize, 1usize);

    let res: Result<(), ConfError> = Parser::with_parser_and_context(
      |inp: &mut SinkIr<'_, '_, '_>| {
        let inner_before =
          <Verbose<ConfError> as Emitter<'_, ByteLexer<'_>>>::checkpoint(inp.emitter().inner_ref());
        {
          let mut tx = inp.begin();
          <&mut ConfSink<'_> as TooFewEmitter<'_, ByteLexer<'_>>>::emit_too_few(
            tx.emitter(),
            TooFew::new(span, 1, 3),
          )?;
          tx.rollback();
        }
        let inner_after =
          <Verbose<ConfError> as Emitter<'_, ByteLexer<'_>>>::checkpoint(inp.emitter().inner_ref());
        assert_eq!(
          inner_after, inner_before,
          "the sink's rewind restores the inner emitter's own reading"
        );
        assert!(
          inp.emitter().inner_ref().errors().is_empty(),
          "the forwarded specialized diagnostic is undone with the branch: {:?}",
          inp.emitter().inner_ref().errors()
        );
        Ok(())
      },
      (&mut sink, DefaultCache::<ByteLexer<'_>>::default()),
    )
    .parse_str("ab");
    res.expect("the sink leg never propagates");

    let (_green, emitter) = sink.finish(K_ROOT, "ab");
    assert!(
      emitter.errors().is_empty(),
      "nothing survives the rolled-back branch"
    );
  }
}
