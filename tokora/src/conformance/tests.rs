//! Tests that prove the conformance kit: positive lexers (hand-rolled + the logos
//! adapter) pass every check, and deliberately-broken fixtures each trip the exact
//! check that owns their defect.

use core::convert::Infallible;

use super::Harness;
use crate::{Lexer, SimpleSpan, Token};

// ── Shared single-kind token for the hand-rolled fixtures ──────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PKind;

impl core::fmt::Display for PKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("p")
  }
}

#[derive(Clone, Debug, PartialEq)]
struct PTok;

impl Token<'_> for PTok {
  type Kind = PKind;
  type Error = Infallible;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> PKind {
    PKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// Rounds `i` up to the next UTF-8 boundary of `s`, clamped to the length.
fn boundary_after(s: &str, mut i: usize) -> usize {
  let len = s.len();
  if i >= len {
    return len;
  }
  i += 1;
  while i < len && !s.is_char_boundary(i) {
    i += 1;
  }
  i
}

// ── Positive: a gap-free per-character lexer (lossless) ─────────────────────────────

struct TileLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for TileLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
fn tile_lexer_passes_all_including_lossless() {
  Harness::<TileLexer<'_>>::over(["hello world", "a", "", "x y  z", "café"])
    .lossless()
    .run();
}

#[test]
fn tile_lexer_passes_without_lossless_too() {
  Harness::<TileLexer<'_>>::new("hello world").run();
}

// ── Positive: a syntactic lexer that skips spaces (leaves gaps) ─────────────────────

struct SyntacticLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for SyntacticLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    let bytes = self.src.as_bytes();
    // Resume from the previous token end and re-skip spaces — this is what makes a
    // trivia-skipping lexer resume correctly from a span end.
    self.start = self.end;
    while self.start < self.src.len() && bytes[self.start] == b' ' {
      self.start += 1;
    }
    if self.start >= self.src.len() {
      // Exhausted after skipping a trailing run of spaces: land the reported span at the
      // position actually reached rather than leaving `start` past a stale `end`. The input
      // layer reads `span()` here as the lexer's own end — a scan's exhaustion commit, and
      // the frontier a non-final EOF reports — so it must stay a well-ordered span, and one
      // that names the position the skipped bytes carried the lexer to.
      self.end = self.start;
      return None;
    }
    let mut e = self.start + 1;
    while e < self.src.len() && bytes[e] != b' ' {
      e += 1;
    }
    self.end = e;
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
fn syntactic_lexer_passes_without_lossless() {
  Harness::<SyntacticLexer<'_>>::over(["ab cd ef", "one  two", "solo", ""]).run();
}

#[test]
#[should_panic(expected = "lossless")]
fn syntactic_lexer_fails_lossless_knob() {
  // Skipped spaces leave gaps, so the gap-free tiling check must reject it.
  Harness::<SyntacticLexer<'_>>::new("ab cd").lossless().run();
}

// ── Negative fixtures: each trips exactly one check ─────────────────────────────────

/// Yields one zero-width `[0, 0)` token: violates monotone progress (nonempty spans).
struct ZeroWidthLexer<'a> {
  src: &'a str,
  yielded: bool,
  state: (),
}

impl<'a> Lexer<'a> for ZeroWidthLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      yielded: false,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      yielded: false,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(0, 0)
  }
  fn slice(&self) -> &'a str {
    ""
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    if self.yielded {
      return None;
    }
    self.yielded = true;
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, _n: &usize) {}
}

#[test]
#[should_panic(expected = "monotone-progress")]
fn zero_width_span_is_caught() {
  Harness::<ZeroWidthLexer<'_>>::new("abc").run();
}

/// A per-character lexer whose `slice()` always disagrees with `span()`.
struct BadSliceLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for BadSliceLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    // Wrong: never the actual span content.
    "?"
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "span/slice-coherence")]
fn incoherent_slice_is_caught() {
  Harness::<BadSliceLexer<'_>>::new("abc").run();
}

/// A per-character lexer that resurrects after exhaustion: violates sticky `None`.
struct NonStickyLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  dead: bool,
  state: (),
}

impl<'a> Lexer<'a> for NonStickyLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      dead: false,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      dead: false,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      // First `None` is honest; every later call resurrects a phantom token.
      if self.dead {
        return Some(Ok(PTok));
      }
      self.dead = true;
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "sticky-exhaustion")]
fn non_sticky_exhaustion_is_caught() {
  Harness::<NonStickyLexer<'_>>::new("abc").run();
}

/// A per-character lexer that **retracts its span at exhaustion**: the moment `lex` returns
/// `None` it reports `0..0`, so the value both readers of the post-exhaustion span depend on —
/// the `to`-shaped end-of-input commit and the partial-input frontier — points behind every
/// item it yielded. This is the broken-fixture class (`start > end` was its other face)
/// made permanently un-shippable.
struct DyingSpanLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  exhausted: bool,
  state: (),
}

impl<'a> Lexer<'a> for DyingSpanLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      exhausted: false,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      exhausted: false,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    if self.exhausted {
      // The defect: the final position is thrown away instead of kept.
      return SimpleSpan::new(0, 0);
    }
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      self.exhausted = true;
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "span-survives-exhaustion")]
fn span_that_dies_at_exhaustion_is_caught() {
  Harness::<DyingSpanLexer<'_>>::new("abc").run();
}

/// A per-character lexer whose post-exhaustion span **runs past the source**, the other
/// direction of the same clause: a refill driver handed this offset would skip bytes it never
/// received.
struct OverReachingSpanLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  exhausted: bool,
  state: (),
}

impl<'a> Lexer<'a> for OverReachingSpanLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      exhausted: false,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      exhausted: false,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    if self.exhausted {
      return SimpleSpan::new(self.start, self.src.len() + 8);
    }
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      self.exhausted = true;
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "span-survives-exhaustion")]
fn span_that_over_reaches_at_exhaustion_is_caught() {
  Harness::<OverReachingSpanLexer<'_>>::new("abc").run();
}

/// A per-character lexer whose `bump` is a no-op: resume always restarts from 0, so a
/// resume from any `k > 0` fails to reproduce the suffix.
struct IgnoreBumpLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for IgnoreBumpLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, _n: &usize) {
    // Wrong: ignores the resume offset entirely.
  }
}

#[test]
#[should_panic(expected = "state-resume")]
fn ignored_bump_breaks_resume() {
  Harness::<IgnoreBumpLexer<'_>>::new("abc").run();
}

/// A per-character lexer whose token width depends on a process-global counter (state
/// outside `State`): two fresh runs disagree, violating replay identity.
struct NonDetLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  width: usize,
  state: (),
}

static NONDET_CTR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

impl<'a> NonDetLexer<'a> {
  fn mk(src: &'a str) -> Self {
    let c = NONDET_CTR.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    Self {
      src,
      start: 0,
      end: 0,
      width: 1 + (c % 2),
      state: (),
    }
  }
}

impl<'a> Lexer<'a> for NonDetLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self::mk(src)
  }
  fn with_state(src: &'a str, _state: ()) -> Self {
    Self::mk(src)
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    let mut e = (self.start + self.width).min(self.src.len());
    while e < self.src.len() && !self.src.is_char_boundary(e) {
      e += 1;
    }
    self.end = e;
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "replay-identity")]
fn nondeterministic_lexer_is_caught() {
  Harness::<NonDetLexer<'_>>::new("abcd").run();
}

// ── Partial-input (Sans-I/O) chunked-equivalence check ──────────────────────────────

#[test]
fn tile_lexer_passes_partial_equivalence() {
  // A faithful per-character lexer reassembles chunk-by-chunk exactly like a single parse.
  Harness::<TileLexer<'_>>::over(["hello world", "a", "", "x y  z", "café"]).run_partial();
}

#[test]
fn syntactic_lexer_passes_partial_equivalence() {
  // A trivia-skipping lexer is fine too: tokens strictly before a cut are unaffected by later
  // bytes, and the frontier holdback covers the one abutting the cut.
  Harness::<SyntacticLexer<'_>>::over(["ab cd ef", "one  two", "solo", ""]).run_partial();
}

/// A per-character lexer whose token **kind depends on the buffer's final byte** — a lookahead
/// beyond `(state, offset)`. It is deterministic on a *fixed* buffer (so it passes every trait-tier
/// check) but its interior tokens change under truncation, so chunked equivalence rejects it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PeekKind {
  Plain,
  Marked,
}

impl core::fmt::Display for PeekKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      PeekKind::Plain => "plain",
      PeekKind::Marked => "marked",
    })
  }
}

#[derive(Clone, Debug, PartialEq)]
struct PeekTok(PeekKind);

impl Token<'_> for PeekTok {
  type Kind = PeekKind;
  type Error = Infallible;
  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;
  fn kind(&self) -> PeekKind {
    self.0
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

struct LastBytePeekLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for LastBytePeekLexer<'a> {
  type State = ();
  type Source = str;
  type Token = PeekTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PeekTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    // The bug: identity depends on the whole buffer's last byte, not on this token's own bytes.
    let marked = self.src.as_bytes().last() == Some(&b'z');
    Some(Ok(PeekTok(if marked {
      PeekKind::Marked
    } else {
      PeekKind::Plain
    })))
  }
  /// **A deliberately false claim, and half the point of this fixture.** The lexer reads the whole
  /// buffer's last byte to decide every item, so its honest answer is
  /// [`ReadFrontier::Unbounded`](crate::ReadFrontier::Unbounded). It claims to be decided within
  /// its own span instead — exactly what a lookahead lexer whose author has not noticed the
  /// lookahead would report. The frontier is a *contract*, not a checked fact, and `run_partial`
  /// is what checks it: the false claim buys the fixture nothing, because the divergence it
  /// produces is caught anyway.
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "partial-equivalence")]
fn truncation_unfaithful_lexer_fails_partial_equivalence() {
  // On "abz" the complete parse marks every token; on the prefix "ab" the same positions come back
  // plain — an interior token changed under truncation, which chunked equivalence catches.
  //
  // It is caught DESPITE the lexer claiming `SpanEnd`, which is what makes this the check on the
  // claim rather than on the frontier machinery: a lexer that under-reports its frontier is not
  // trusted into safety, it is falsified.
  Harness::<LastBytePeekLexer<'_>>::new("abz").run_partial();
}

// ── The error arm of chunked equivalence ────────────────────────────────────────────
//
// Every fixture above lexes without ever returning `Err`, so nothing in the partial tier
// exercised the lexer-error arm at all. That arm diverges under truncation in two ways a
// token-only comparison cannot see, and both are checked below.

/// A lexer error with a **payload**: the thing the partial tier has to compare, and the reason a
/// discarding emitter was not enough. [`Token::Error`](crate::Token::Error) is `Clone + Debug` and
/// nothing more, so the `Debug` rendering is the only signature available — and it is compared
/// against the *same build's* complete parse, never against a recorded string, so changing this
/// rendering moves both sides together and can never red the check on its own.
#[derive(Clone, Debug, PartialEq)]
struct BadByte {
  /// What the lexer decided about the bytes. A payload the lexer computed, not a rendered
  /// diagnostic: the check reads it as data, and nothing here asserts on message wording.
  reason: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
struct ETok;

impl Token<'_> for ETok {
  type Kind = PKind;
  type Error = BadByte;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> PKind {
    PKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// Every item is decided by its own bytes: an item is an error iff its own byte is `?`. Faithful
/// under truncation, errors and all.
const FAITHFUL: u8 = 0;
/// The first item is an **error** unless the buffer's last byte is `z` — so appending the missing
/// byte turns a truncated error into a token at the same span.
const FLIPS_TO_TOKEN: u8 = 1;
/// The first item is always an error, but its **payload** is decided by the buffer's last byte —
/// so appending the missing byte changes the error without changing its span.
const PAYLOAD_DRIFTS: u8 = 2;

/// A per-character lexer over `str` that can yield errors, parameterized by which truncation
/// defect (if any) it carries. `MODE` is a const parameter rather than a field because the kit
/// builds its lexers with `L::new`.
///
/// All three modes are deterministic on a *fixed* buffer, so the trait tier cannot tell them
/// apart; only chunked equivalence can, and only if it carries errors.
struct ErrLexer<'a, const MODE: u8> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a, const MODE: u8> Lexer<'a> for ErrLexer<'a, MODE> {
  type State = ();
  type Source = str;
  type Token = ETok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), BadByte> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<ETok, BadByte>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    let bytes = self.src.as_bytes();
    let sealed = bytes.last() == Some(&b'z');
    match MODE {
      FAITHFUL => {
        if bytes[self.start] == b'?' {
          Some(Err(BadByte { reason: "junk" }))
        } else {
          Some(Ok(ETok))
        }
      }
      FLIPS_TO_TOKEN if self.start == 0 && !sealed => Some(Err(BadByte { reason: "junk" })),
      PAYLOAD_DRIFTS if self.start == 0 => Some(Err(BadByte {
        reason: if sealed { "junk" } else { "cut" },
      })),
      _ => Some(Ok(ETok)),
    }
  }
  /// `SpanEnd`, and for the two defective modes a **deliberately false claim** — the same shape
  /// [`LastBytePeekLexer`] uses. At the `Unbounded` default every item is withheld and the check
  /// would pass vacuously; the claim is what puts the items in front of it.
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

type FaithfulErrLexer<'a> = ErrLexer<'a, FAITHFUL>;
type FlippingErrLexer<'a> = ErrLexer<'a, FLIPS_TO_TOKEN>;
type DriftingErrLexer<'a> = ErrLexer<'a, PAYLOAD_DRIFTS>;

#[test]
fn error_yielding_lexer_passes_every_check() {
  // Errors decided by their own bytes are as stable under truncation as tokens are, on the trait
  // tier and the partial tier both. The two defective siblings below differ from this one only in
  // WHERE the verdict comes from.
  Harness::<FaithfulErrLexer<'_>>::over(["a?b", "??", "?", "", "ab?cd"]).run();
  Harness::<FaithfulErrLexer<'_>>::over(["a?b", "??", "?", "", "ab?cd"]).run_partial();
}

// Both cells below pin the split, the position and which SIDE held the token, not just the
// `partial-equivalence` tag: a bare tag would also be satisfied by a length divergence or a
// divergence at some other cut, and the whole point of these two is that they fire on the error
// arm specifically. Neither pin reads the span's or the payload's rendering.

#[test]
#[should_panic(
  expected = "partial-equivalence] split k=2, position 0: prefix item diverges from the complete prefix: expected token"
)]
fn an_error_that_becomes_a_token_on_append_is_falsified() {
  // `"abz"` at split k=2: the prefix `"ab"` yields `Err@0..1` — emitted, since 1 < 2 puts it behind
  // the frontier — and the complete parse has `Token@0..1` at the same span. The item is not
  // withheld and it is not missing; it is a *different kind of item*, which is invisible to a
  // comparison over committed tokens alone.
  Harness::<FlippingErrLexer<'_>>::over(["abz"]).run_partial();
}

#[test]
#[should_panic(
  expected = "partial-equivalence] split k=2, position 0: prefix item diverges from the complete prefix: expected lexer error"
)]
fn an_error_whose_payload_changes_on_append_is_falsified() {
  // `"abz"` at split k=2: both runs yield `Err@0..1`, so discriminant and span agree and only the
  // payload moved — `"cut"` on the prefix, `"junk"` once the last byte arrives. An error whose
  // content depends on bytes past its own span is exactly the unfaithfulness this tier exists to
  // reject, and the span alone does not see it.
  Harness::<DriftingErrLexer<'_>>::over(["abz"]).run_partial();
}

// ── Equality is a question about the VALUE, not about a rendering ───────────────────
//
// Three cells over the two properties a comparison key needs and a `Debug` string has neither of.
// The token arm needed the same repair for a third reason: it compared only the KIND, so a payload
// could move while kind and span held still.

/// A one-kind token that carries a **payload** — the thing the token arm compared nothing of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct VKind;

impl core::fmt::Display for VKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("v")
  }
}

#[derive(Clone, Debug, PartialEq)]
struct VTok(u8);

impl Token<'_> for VTok {
  type Kind = VKind;
  type Error = Infallible;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> VKind {
    VKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// Every token's payload is its own byte: faithful under truncation, payload and all.
const OWN_BYTE: u8 = 0;
/// Every token's payload is the **buffer's last byte** — decided by input that has not arrived,
/// while the kind and the span stay exactly what a faithful lexer would produce.
const LAST_BYTE: u8 = 1;

/// A per-character lexer over `str` whose tokens carry a payload, parameterized by where that
/// payload comes from. `MODE` is a const parameter rather than a field because the kit builds its
/// lexers with `L::new`.
struct ValueLexer<'a, const MODE: u8> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a, const MODE: u8> Lexer<'a> for ValueLexer<'a, MODE> {
  type State = ();
  type Source = str;
  type Token = VTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<VTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    let bytes = self.src.as_bytes();
    let payload = match MODE {
      OWN_BYTE => bytes[self.start],
      // The defect: every token's value is read off the END of the buffer.
      _ => *bytes.last().expect("the source is non-empty here"),
    };
    Some(Ok(VTok(payload)))
  }
  /// `SpanEnd`, deliberately false in `LAST_BYTE` mode — the same shape [`LastBytePeekLexer`]
  /// uses. At the `Unbounded` default every item is withheld and the check would pass vacuously.
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

type OwnBytePayloadLexer<'a> = ValueLexer<'a, OWN_BYTE>;
type DriftingPayloadLexer<'a> = ValueLexer<'a, LAST_BYTE>;

#[test]
fn a_payload_decided_by_its_own_bytes_passes_every_check() {
  // The control for the cell below: carrying a payload is not what the check rejects.
  Harness::<OwnBytePayloadLexer<'_>>::over(["abz", "a", "", "café"]).run();
  Harness::<OwnBytePayloadLexer<'_>>::over(["abz", "a", "", "café"]).run_partial();
}

#[test]
#[should_panic(
  expected = "partial-equivalence] split k=2, position 0: prefix item diverges from the complete prefix: expected token"
)]
fn a_token_payload_that_changes_on_append_is_falsified() {
  // `"abz"` at split k=2: the prefix `"ab"` yields `VTok(b'b')@0..1` and the complete parse
  // `VTok(b'z')@0..1`. Same kind, same span, same discriminant — everything the tier compared
  // before — and a different value in the parser's hands. `run()` and `run_partial()` both passed
  // while the AST changed.
  Harness::<DriftingPayloadLexer<'_>>::over(["abz"]).run_partial();
}

/// Two distinct payloads with **one** `Debug` rendering — the injectivity failure, which is legal
/// and not rare (a hand-written `Debug` that prints one label for a family of variants).
#[derive(Clone, PartialEq)]
enum Collide {
  Cut,
  Junk,
}

impl core::fmt::Debug for Collide {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("LexError")
  }
}

#[derive(Clone, Debug, PartialEq)]
struct CTok;

impl Token<'_> for CTok {
  type Kind = PKind;
  type Error = Collide;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> PKind {
    PKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// [`DriftingErrLexer`]'s defect over a payload type whose `Debug` cannot tell the two values
/// apart: the first item is always an error, and *which* error is decided by the buffer's last
/// byte.
struct CollidingErrLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for CollidingErrLexer<'a> {
  type State = ();
  type Source = str;
  type Token = CTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Collide> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<CTok, Collide>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    if self.start == 0 {
      let sealed = self.src.as_bytes().last() == Some(&b'z');
      return Some(Err(if sealed { Collide::Junk } else { Collide::Cut }));
    }
    Some(Ok(CTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(
  expected = "partial-equivalence] split k=2, position 0: prefix item diverges from the complete prefix: expected lexer error"
)]
fn a_colliding_debug_does_not_hide_a_payload_that_moved() {
  // Byte for byte `an_error_whose_payload_changes_on_append_is_falsified`'s divergence, over a
  // payload whose `Debug` renders `Cut` and `Junk` identically. A comparison over the rendering
  // sees `"LexError" == "LexError"` and passes — the very drift the sibling cell was written to
  // catch, invisible to the mechanism that was catching it.
  Harness::<CollidingErrLexer<'_>>::over(["abz"]).run_partial();
}

/// How many times a [`Ticking`] payload has been rendered. A module-level counter rather than a
/// field, so two renderings of two *equal* payloads still differ — which is the property under
/// test.
static TICKING_RENDERS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// A payload whose **value** is stable and whose `Debug` never renders the same way twice — the
/// stability failure, standing in for a rendering that carries an address, a generation number or
/// any other incidental.
#[derive(Clone, PartialEq)]
struct Ticking {
  reason: &'static str,
}

impl core::fmt::Debug for Ticking {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let tick = TICKING_RENDERS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    write!(f, "{}#{tick}", self.reason)
  }
}

#[derive(Clone, Debug, PartialEq)]
struct TTok;

impl Token<'_> for TTok {
  type Kind = PKind;
  type Error = Ticking;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> PKind {
    PKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// [`FaithfulErrLexer`]'s behaviour — an item is an error iff its own byte is `?` — over the
/// unstable-`Debug` payload. Conforming in every respect the contract names.
struct TickingErrLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for TickingErrLexer<'a> {
  type State = ();
  type Source = str;
  type Token = TTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Ticking> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<TTok, Ticking>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    if self.src.as_bytes()[self.start] == b'?' {
      return Some(Err(Ticking { reason: "junk" }));
    }
    Some(Ok(TTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
fn a_nondeterministic_debug_does_not_red_a_conforming_lexer() {
  // The other direction, and the reason "compare the rendering, both sides come from the same
  // build" was not enough: the two sides are the same build and still render differently, because
  // rendering is not a function of the value. Under the `Debug` comparison this red at the first
  // split of the first input; under value equality the payloads are equal and it passes.
  //
  // `run()` is deliberately NOT called here. The trait tier keeps the same `Debug`-string key in
  // `Item::err_dbg`, so it would red this conforming lexer — the same defect, one tier up. Closing
  // it means `L::Token: PartialEq` on `run` itself, which every existing caller of the kit's
  // universal entry point would have to satisfy; that is a decision about the crate's public
  // surface, not a repair this tier can make on its own.
  Harness::<TickingErrLexer<'_>>::over(["a?b", "??", "?", ""]).run_partial();
}

// ── The anti-hang budget, and why it cannot live at the `next()` boundary ───────────
//
// `InputRef::next` is a LOOP: it keeps lexing after every lexer error it accepts until it finds
// a token or reaches end of input. A budget checked once per `next()` call therefore bounds the
// number of items the drain yields and not the work the lexer is asked to do — and the two come
// apart exactly on a malformed lexer, which is the case the kit exists to reject.
//
// The fixture below is the smallest witness: it never advances and never exhausts, so one `next()`
// never returns. The input layer's error dedup keys on the span end, so after the first report the
// log stops growing too and a log-length budget stays flat forever. Only a counter UNDERNEATH
// `next()` — one that counts every raw `Lexer::lex` attempt — can end this run.

/// A lexer that neither advances nor exhausts: every [`lex`](Lexer::lex) returns the **same**
/// nonempty error span, forever.
///
/// Deliberately not a plausible lexer. It is the malformed input the harness is supposed to
/// refuse, reduced to its two load-bearing properties: the span is nonempty (so the input layer's
/// zero-width contract check does not fire first) and it never changes (so the dedup watermark
/// suppresses every report after the first).
struct EndlessErrLexer<'a> {
  src: &'a str,
  state: (),
}

impl EndlessErrLexer<'_> {
  /// The one span this lexer ever reports: the first character of the source.
  fn only_span(&self) -> SimpleSpan {
    SimpleSpan::new(0, boundary_after(self.src, 0))
  }
}

impl<'a> Lexer<'a> for EndlessErrLexer<'a> {
  type State = ();
  type Source = str;
  type Token = ETok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self { src, state: () }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self { src, state }
  }
  fn check(&self) -> Result<(), BadByte> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    self.only_span()
  }
  fn slice(&self) -> &'a str {
    &self.src[..self.only_span().end]
  }
  fn lex(&mut self) -> Option<Result<ETok, BadByte>> {
    if self.src.is_empty() {
      return None;
    }
    Some(Err(BadByte { reason: "endless" }))
  }
  /// `SpanEnd`, so the error is not withheld at the frontier: the point of the fixture is the
  /// loop inside one `next()`, not the holdback.
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, _n: &usize) {}
}

/// The expected substring is the **per-instance** guard's wording, and that is the whole reason the
/// guard is still here: this lexer never returns from one `next()`, the aggregate tally would
/// refuse it too but only after the tier's `O(units²)` allowance, and "one lexer instance was asked
/// to lex more than N times" names a scan that will not return where the aggregate can only report
/// that the tier did too much work. If this cell starts reading the aggregate wording, the narrower
/// guard has stopped earning its place.
#[test]
#[should_panic(expected = "lex-budget] one lexer instance was asked to lex more than")]
fn an_endless_same_span_error_lexer_is_refused_not_hung() {
  // Before the budget moved underneath `next()`, this call did not fail — it HUNG. The first
  // `complete_stream` drain enters `next()`, the scanner accepts an error, skips it, lexes the
  // identical error again, and never returns; the log holds the one report the dedup let through,
  // so the per-`next()` budget it was checked against never grows.
  Harness::<EndlessErrLexer<'_>>::over(["abc"]).run_partial();
}

// ── The knob's boundary: three inputs, three separate refusals ──────────────────────
//
// These three cells used to be one, and the one was vacuous. It passed `usize::MAX` and accepted
// any panic tagged `lex-budget`, so every wrong behaviour on the way satisfied it: a clamp to a
// LOWER multiple than the maximum satisfied it, and so did the release wrapped-to-zero counter,
// which reached the per-instance refusal with the right tag for the wrong reason. A boundary test
// that cannot tell the boundary from anything near it is testing that a panic happened.
//
// So the exact maximum, one above it, and `usize::MAX` are three cells, and each asserts the
// specific message its own input should produce: the maximum is ACCEPTED and its guard fires with
// the per-instance wording, and the two above it never reach a lexer at all — the builder refuses
// them by name, which no `lex-budget` panic can be mistaken for.

/// The exact maximum is **accepted**, and the guard still fires under it.
///
/// This is the half a cap can get wrong in the permissive direction: a cap enforced one short of
/// itself is a knob whose documented maximum is a lie. `budget_multiple(65536)` over one unit gives
/// a budget of `65536 * 1 + 64 = 65600` items and a per-instance ceiling of `65601`, and this
/// lexer never returns from one `next()`, so the per-instance guard is the one that answers — the
/// exact number is in the expectation, because a ceiling derived from a clamped-down multiple
/// would print a smaller one.
#[test]
#[should_panic(expected = "lex-budget] one lexer instance was asked to lex more than 65601 times")]
fn the_exact_maximum_budget_multiple_is_accepted_and_still_refuses_an_endless_lexer() {
  Harness::<EndlessErrLexer<'_>>::over(["a"])
    .budget_multiple(super::MAX_BUDGET_MULTIPLE)
    .run_partial();
}

/// One above the maximum is **refused at the knob**, not lowered to it.
///
/// The panic has to come from the builder, so the cell never calls `run_partial`: if the cap ever
/// goes back to clamping, this fails with "did not panic" rather than being satisfied by whatever
/// the clamped run does next. The expectation carries the knob's name, the supported maximum and
/// the rejected value, none of which a `lex-budget` message contains.
#[test]
#[should_panic(
  expected = "Harness::budget_multiple is capped at 65536 items per source unit and was given 65537"
)]
fn one_above_the_maximum_budget_multiple_is_refused_at_the_knob() {
  let _ =
    Harness::<EndlessErrLexer<'_>>::over(["a"]).budget_multiple(super::MAX_BUDGET_MULTIPLE + 1);
}

/// `usize::MAX` — the historical disarm value — takes the same refusal, and that is the point.
///
/// It is a separate cell from the one above because it is the value that used to be *accepted*:
/// clamped to the cap, run, and then refused with a `lex-budget` tag that read as a verdict on the
/// lexer. The rejected value is not in the expectation because its rendering is target-width
/// dependent; the knob's name and the maximum are, and neither a clamp (which panics not at all
/// here) nor any `lex-budget` refusal can produce them.
#[test]
#[should_panic(
  expected = "Harness::budget_multiple is capped at 65536 items per source unit and was given "
)]
fn the_largest_usize_budget_multiple_is_refused_at_the_knob() {
  let _ = Harness::<EndlessErrLexer<'_>>::over(["a"]).budget_multiple(usize::MAX);
}

// ── ... and why a budget per LEXER INSTANCE is one boundary short ───────────────────
//
// The repair above put the counter on the lexer instance, on the ground that the input layer runs
// one `next()` call's whole internal loop on one instance. Both halves of that are true and the
// conclusion drawn from them is not: the layer builds a fresh lexer for EVERY `next()`
// (`Lexer::with_state` + `bump`), so a per-instance counter IS a per-call counter, and it restarts
// on every call.
//
// The fixture below is what that buys an attacker. It spends the whole per-call ceiling on repeated
// same-span errors and then yields one advancing token — on every call. The span-end dedup records
// only the first error of each call, so the item log grows by two per token and the item budget
// stays flat; the instance ceiling is never exceeded because the instance is new each time. Driven
// over every prefix by `run_partial`, raw lex work is CUBIC in the source length: measured at
// 4f39c1a, 1319 attempts over 4 units rising to 66779 over 24 — a fit to `8n³/3 + Θ(n²)` — with
// neither guard firing and no hang to notice.
//
// Only a counter that no construction, reset or rollback returns capacity to ends that, which is
// what `LexTally` is.

/// Raw `Lexer::lex` attempts made by [`StallLexer`], summed over every instance the input layer
/// builds. A `static` because the kit constructs the fixture itself — [`Lexer::with_state`] is
/// handed a source and a state and nothing else — so a per-run handle cannot reach it. One test
/// reads or writes it.
static STALL_ATTEMPTS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// The fixture's own alarm, set **above** the kit's aggregate ceiling for [`STALL_SRC`] (45004) and
/// **below** the cubic total the lexer would otherwise reach (179795, measured by lifting the
/// aggregate ceiling and this alarm together and reading the counter). The cell therefore
/// distinguishes three outcomes rather than two: the kit's `lex-budget` means the aggregate held,
/// this message means it did not, and no panic at all means the run walked the whole cube.
const STALL_TRIP: usize = 60_000;

/// 32 units — long enough that the cubic total dwarfs the quadratic ceiling with room to spare.
const STALL_SRC: &str = "abcdefghijklmnopqrstuvwxyz012345";

/// A lexer that spends **just under** the per-lexer-instance ceiling before every token it yields.
///
/// Every attempt but the last reports the same nonempty span at the current position — which the
/// contract permits, since starts must be non-decreasing rather than strictly increasing, and which
/// the dedup hides — and the last advances and hands back a token, so `next()` always returns and
/// every drain terminates. Nothing here hangs: the defect is the *total*.
struct StallLexer<'a> {
  src: &'a str,
  /// The resume cursor. Moved by a token and by `bump`, never by an error.
  at: usize,
  start: usize,
  end: usize,
  /// Attempts THIS instance has made. Reset by every rebuild, which is the point.
  burned: usize,
  state: (),
}

impl StallLexer<'_> {
  /// The per-instance ceiling, reproduced so the fixture can spend all of it and not one more.
  ///
  /// Derived from [`STALL_SRC`] rather than from `self.src`: the ceiling is a property of the
  /// tier's configured budget over the *whole* input, so every prefix drive of the partial sweep
  /// gets the same one. Reading `self.src` here would track the old, prefix-scaled ceiling and the
  /// fixture would stop spending it as the cut moved in.
  fn quota(&self) -> usize {
    super::instance_ceiling(8 * STALL_SRC.len() + 64)
  }
}

impl<'a> Lexer<'a> for StallLexer<'a> {
  type State = ();
  type Source = str;
  type Token = ETok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      at: 0,
      start: 0,
      end: 0,
      burned: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      at: 0,
      start: 0,
      end: 0,
      burned: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), BadByte> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<ETok, BadByte>> {
    let n = STALL_ATTEMPTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    assert!(
      n <= STALL_TRIP,
      "stall-fixture alarm: the aggregate reached {n} raw lex attempts over a {} unit source; \
       nothing bounded the total",
      STALL_SRC.len()
    );
    if self.at >= self.src.len() {
      return None;
    }
    self.start = self.at;
    self.end = boundary_after(self.src, self.at);
    self.burned += 1;
    if self.burned < self.quota() {
      Some(Err(BadByte { reason: "stall" }))
    } else {
      self.at = self.end;
      Some(Ok(ETok))
    }
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.at += *n;
    self.start = self.at;
    self.end = self.at;
  }
}

/// The expected substring is the **aggregate** tally's wording, not merely the `lex-budget` tag:
/// the instance ceiling says "one lexer instance was asked", and if it were what stopped this the
/// cell would red. Which counter fires is the whole subject of the cell.
#[test]
#[should_panic(expected = "lex-budget] the lexer was asked to lex more than")]
fn a_lexer_that_stalls_just_under_the_per_call_ceiling_is_refused() {
  STALL_ATTEMPTS.store(0, core::sync::atomic::Ordering::Relaxed);
  Harness::<StallLexer<'_>>::over([STALL_SRC]).run_partial();
}

// ── ... and the other direction: what the narrower guard may never refuse ────────────
//
// Every cell above is about a guard that was too LOOSE, and an attacker slipping under it. The two
// below are the same error read the other way. The per-instance ceiling was
// `DEFAULT_BUDGET_MULTIPLE * units + BUDGET_FLOOR`, computed from whatever source the instance was
// handed, which made it wrong twice: it never saw `Harness::budget_multiple`, so a lexer certified
// under a raised budget met the default anyway; and in the partial tier the "source" is a prefix,
// so the ceiling shrank with the cut. Both produce a CONFORMING LEXER REJECTED — the one outcome a
// narrower guard may not cause, because the sharper message it exists for is worthless if it is a
// lie. The repair is that the ceiling is derived from the tier's configured budget and carried on
// the tally, so it is the same for every instance and is one the item budget already permits.

/// A lexer that emits `N` errors over the source's first unit and then exhausts — legally.
///
/// Every error reports the same nonempty span, which the contract permits (starts must be
/// *non-decreasing*, not strictly increasing), and the count lives in `State` so the lexer is
/// resume-faithful: `with_state` restores how many are left and `bump` does not disturb it. Nothing
/// here is malformed and nothing here spins. Its whole content is *density*.
struct RepeatErrLexer<'a, const N: usize> {
  src: &'a str,
  /// Errors emitted so far — in the state, because that is the only channel a rebuilt lexer has,
  /// and a fixture whose density is not resume-faithful would fail an earlier check instead.
  emitted: Emitted,
}

/// [`RepeatErrLexer`]'s whole state: how many of its `N` errors are already out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Emitted(usize);

impl crate::State for Emitted {
  type Error = ();

  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

impl<const N: usize> RepeatErrLexer<'_, N> {
  /// The one span every error reports: the source's first unit, or the empty source's `0..0`.
  fn only_span(&self) -> SimpleSpan {
    SimpleSpan::new(0, boundary_after(self.src, 0))
  }
}

impl<'a, const N: usize> Lexer<'a> for RepeatErrLexer<'a, N> {
  type State = Emitted;
  type Source = str;
  type Token = ETok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      emitted: Emitted(0),
    }
  }
  fn with_state(src: &'a str, state: Emitted) -> Self {
    Self {
      src,
      emitted: state,
    }
  }
  fn check(&self) -> Result<(), BadByte> {
    Ok(())
  }
  fn state(&self) -> &Emitted {
    &self.emitted
  }
  fn state_mut(&mut self) -> &mut Emitted {
    &mut self.emitted
  }
  fn into_state(self) -> Emitted {
    self.emitted
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    self.only_span()
  }
  fn slice(&self) -> &'a str {
    &self.src[..self.only_span().end]
  }
  fn lex(&mut self) -> Option<Result<ETok, BadByte>> {
    if self.src.is_empty() || self.emitted.0 >= N {
      return None;
    }
    self.emitted.0 += 1;
    Some(Err(BadByte { reason: "dense" }))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, _n: &usize) {}
}

/// A raised [`Harness::budget_multiple`] must reach the guard underneath `next()`, or the knob
/// certifies nothing.
///
/// 80 errors over one unit: inside the item budget the caller configured (`128 * 1 + 64 = 192`) and
/// far outside the fixed `8 * 1 + 64 = 72` the instance ceiling used to carry. One
/// `InputRef::next` serves all 80 on one lexer, so both driving tiers refused this lexer on attempt
/// 73 while the tally they are supposedly bounded by had spent 73 of its thousands.
#[test]
fn a_dense_lexer_is_certified_under_the_budget_multiple_it_was_given() {
  Harness::<RepeatErrLexer<'_, 80>>::new("a")
    .budget_multiple(128)
    .run();
  Harness::<RepeatErrLexer<'_, 80>>::new("a")
    .budget_multiple(128)
    .run_partial();
}

/// The boundary the guard has to leave open: a run that produces **exactly** its budget of items
/// and needs one more attempt to say `None`.
///
/// 72 items over one unit is what the default budget allows to the character, and the direct tier
/// accepts the run — `lex_run` and `check_sticky` both pass. The 73rd attempt is the exhaustion
/// probe, and the instance ceiling used to be `72`, so the same run was refused as nonterminating
/// the moment it went through the input layer. A ceiling of "every item the budget allows, plus the
/// probe that ends it" is the narrowest one that cannot contradict the budget it is derived from,
/// and this cell sits on it exactly: one item more and the *item* budget refuses first.
#[test]
fn a_run_that_spends_its_whole_item_budget_still_reaches_exhaustion() {
  Harness::<RepeatErrLexer<'_, 72>>::new("a").run();
  Harness::<RepeatErrLexer<'_, 72>>::new("a").run_partial();
}

/// The other half of the same error, and the one no budget knob could have worked around: the
/// ceiling used to be computed from the source the *instance* was handed, and in the partial tier
/// that is a **prefix**.
///
/// 100 errors over 8 units is inside the default budget (`8 * 8 + 64 = 128`), so the complete drive
/// and the final one accepted it — and then the sweep reached the two-unit prefix, whose ceiling
/// was `8 * 2 + 64 = 80`, and refused the same lexer for the same behaviour. Where the cut falls is
/// not supposed to change what conforms: the run's budget is over the whole input, and so is every
/// ceiling derived from it.
#[test]
fn a_prefix_drive_holds_the_same_ceiling_as_the_whole_input() {
  Harness::<RepeatErrLexer<'_, 100>>::new("abcdefgh").run();
  Harness::<RepeatErrLexer<'_, 100>>::new("abcdefgh").run_partial();
}

/// The `non-` in non-rewindable, measured rather than argued.
///
/// tokora #285 is this defect in the shipped `TokenLimiter`: a tally living in a `Checkpoint` field
/// is handed back by a speculative rollback, and a budget you can refund is not a budget. The kit's
/// tally is a count in a `Cell` behind an `Rc` whose handle rides in the lexer state, so what a
/// restore puts back — and what a rebuilt lexer is handed — is a *pointer* to the one count.
///
/// The measurement is the **total**, not the reading at the instant of the restore. A refund does
/// not have to land on the `restore` call: the state a checkpoint holds is handed to
/// `Lexer::with_state` later, so a tally carried by value inside that state is returned then, and a
/// cell that reads the counter on both sides of `restore` alone sees nothing. So this compares one
/// straight drain against the same drain rolled back and replayed: replaying work the tally already
/// paid for must cost again.
#[test]
fn a_checkpoint_restore_does_not_refund_the_tally() {
  const SRC: &str = "hello world";
  let budget = 8 * SRC.len() + 64;
  let ceiling = super::lex_attempt_ceiling(8, budget);
  let instance = super::instance_ceiling(budget);

  let plain = super::LexTally::new(0, "test", SRC.len(), ceiling, instance);
  super::drive::<TileLexer<'_>, _>(SRC, &plain, |ir| {
    while ir.next().expect(super::NEVER_ERRS).is_some() {}
  });
  let once = plain.spent();
  // A floor with a reason, not `> 0`: `TileLexer` tiles the source one character per token, so a
  // straight drain cannot cost fewer attempts than the source has characters. Without it a plant
  // that shrinks BOTH sides — a reset on every rebuild rather than only on a rollback — satisfies
  // the ratio below on two meaningless numbers.
  assert!(
    once >= SRC.len() as u128,
    "a straight drain of {} character(s) charged only {once} attempt(s)",
    SRC.len()
  );

  let replayed = super::LexTally::new(0, "test", SRC.len(), ceiling, instance);
  super::drive::<TileLexer<'_>, _>(SRC, &replayed, |ir| {
    let ckp = ir.save();
    while ir.next().expect(super::NEVER_ERRS).is_some() {}
    ir.restore(ckp);
    while ir.next().expect(super::NEVER_ERRS).is_some() {}
  });

  assert!(
    replayed.spent() >= 2 * once,
    "a drain rolled back and replayed charged {} attempt(s) where two straight drains charge {}: \
     the rollback returned capacity the run had already spent",
    replayed.spent(),
    2 * once
  );
}

// ── The counters at their own ceilings: `>=` before the increment, not after ─────────
//
// `MAX_BUDGET_MULTIPLE` keeps a CONFIGURED ceiling inside the range a counter can pass. It says
// nothing about a DERIVED one, and while the tally counted in `usize` the derived ceiling
// saturated: at `usize::MAX` the old `spent += 1; if spent > limit` never reached its comparison,
// because the increment overflowed first. Debug panicked with the wrong message; release wrapped
// the tally to zero and handed the run its whole allowance again, as often as the work asked,
// printing nothing.
//
// The cells below are that state reached directly, because reaching it by counting means counting
// to the counter's widest value. They fail under the old order in BOTH profiles — release by not
// panicking at all, debug by panicking with "attempt to add with overflow" instead of the message
// the expectation names — and the release arm is the one that matters, since that is where the
// wrap silently continued the run.
//
// The aggregate counter is `u128` now, so its widest value is `u128::MAX`; the per-instance
// counter is still `usize`, because `instance_ceiling(budget)` is exact over a
// `representable_budget` result and grows only linearly in the source. Each cell sits on the
// widest value of the counter it is about.

/// The aggregate tally standing on the widest count it can hold, with that value as its ceiling.
///
/// The seeded constructor is `#[cfg(test)]` and mints a *new* count, so it cannot return capacity
/// to a tally a run is holding — the module wall the invariant rests on is untouched.
///
/// The ceiling here is `Derived`, so the wording is the lexer's: this cell is about the
/// *arithmetic* being total at the top of the counter's range, not about whose fault the refusal
/// is. The cell below it is the other half.
#[test]
#[should_panic(expected = "lex-budget] the lexer was asked to lex more than")]
fn a_ceiling_at_the_counters_widest_value_refuses_rather_than_wrapping_the_tally() {
  let tally = super::LexTally::preloaded(
    0,
    "saturated",
    1,
    u128::MAX,
    super::AttemptCeiling::Derived(u128::MAX),
    usize::MAX,
  );
  tally.spend();
}

/// The kit's own limit is reported as the kit's own limit.
///
/// A `Capacity` ceiling means the derivation did not fit in `u128` and the tally counted as far as
/// it could. Nothing about the lexer was decided, so the refusal may not carry the `lex-budget`
/// tag or the "the lexer was asked to lex more than" wording — a caller reads both as a verdict on
/// their code, and the truth is that the kit ran out of counter. The expectation names the tag,
/// the word INCONCLUSIVE and the explicit denial, because a message that merely *omits* the
/// accusation is one round of editing away from carrying it again.
#[test]
#[should_panic(
  expected = "kit-capacity] INCONCLUSIVE — this is a limit of the conformance kit \
                           and NOT a verdict on the lexer"
)]
fn an_exhausted_kit_capacity_is_reported_as_inconclusive_and_not_as_the_lexers_fault() {
  let tally = super::LexTally::preloaded(
    0,
    "capacity",
    1,
    u128::MAX,
    super::AttemptCeiling::Capacity,
    usize::MAX,
  );
  tally.spend();
}

/// And the two verdicts are told apart by what the tally was *built* with, not by the count.
///
/// Both cells above stand on `u128::MAX`; only the ceiling's kind differs. This one drives the
/// same distinction from the other end — a `Capacity` tally with room to spare charges normally,
/// so the kit-capacity message is reached by exhausting the counter and by nothing else.
#[test]
fn a_capacity_ceiling_still_charges_normally_below_it() {
  let tally = super::LexTally::preloaded(0, "capacity", 1, 7, super::AttemptCeiling::Capacity, 9);
  tally.spend();
  assert_eq!(
    tally.spent(),
    8,
    "a capacity-bounded tally must charge like any other until the counter is actually exhausted"
  );
  assert_eq!(
    tally.limit(),
    u128::MAX,
    "a capacity ceiling counts as far as the counter goes and no further"
  );
}

/// The per-instance counter at the ceiling its own derivation can hand it.
///
/// `instance_ceiling(budget)` is `budget + 1` and `representable_budget` may return
/// `usize::MAX - 1`, so `per_instance` can be `usize::MAX` — the identical defect one counter over.
/// The tally is given room to spare so the aggregate does not answer first: the subject here is
/// `Budgeted::spent_here`, and the expectation is the per-instance wording for that reason.
///
/// This counter stays `usize` where the aggregate widened. That is a difference in what the two
/// ceilings are, not an oversight: `instance_ceiling` is exact over a budget the kit has already
/// refused to compute unrepresentably, and it grows linearly in the source where the aggregate
/// grows quadratically, so it never becomes a bound the host's width decides.
#[test]
#[should_panic(expected = "lex-budget] one lexer instance was asked to lex more than")]
fn a_saturated_instance_ceiling_refuses_rather_than_wrapping_the_instance_counter() {
  let tally = super::LexTally::new(
    0,
    "saturated",
    1,
    super::AttemptCeiling::Derived(u128::MAX),
    usize::MAX,
  );
  let mut lexer = super::Budgeted::wrap(
    TileLexer::new("a"),
    super::Tallied {
      inner: (),
      tally: super::Rc::clone(&tally),
    },
  );
  lexer.spent_here = usize::MAX;
  lexer.spend();
}

/// The step *before* both cells above, and the other half of the bracket they leave open.
///
/// The refusals show a ceiling at the counter's widest value is enforced once the count is
/// standing on it. On their own they do not say the count *gets* there, and that is the claim the
/// record used to have backwards: it called such a ceiling a guard that could never fire, on the
/// reasoning that no counter reaches one past the largest value it holds. With `>=` asked before
/// the increment there is no "one past" to reach — the last allowed attempt charges from
/// `MAX - 1`, where `spent < limit` still holds, so the `+ 1` is in range and lands exactly on the
/// ceiling.
///
/// Together the cells bracket the boundary: `MAX - 1` charges and does not refuse, `MAX` refuses.
/// So a ceiling at the top of the counter's range is enforced after exactly `MAX` attempts — a
/// hard cap, not an absent guard. Under the old `spent += 1; spent > limit` the boundary sat one
/// attempt further out and it was the *overflow* rather than the check that ended the run, which is
/// what the cells above pin.
///
/// Both counters are here because both take the same order, each at the top of its own width:
/// `u128::MAX` for the tally, `usize::MAX` for the instance counter, which `instance_ceiling` can
/// still hand it over a budget of `usize::MAX - 1`.
#[test]
fn the_last_attempt_below_a_saturated_ceiling_lands_on_it_without_overflowing() {
  let saturated = super::LexTally::preloaded(
    0,
    "saturated",
    1,
    u128::MAX - 1,
    super::AttemptCeiling::Derived(u128::MAX),
    usize::MAX,
  );
  saturated.spend();
  assert_eq!(
    saturated.spent(),
    u128::MAX,
    "the last attempt a saturated aggregate ceiling allows must land on the ceiling"
  );

  // The same step through `Budgeted::spend`, which charges the tally first. That tally is a fresh
  // one with room to spare, so the aggregate does not answer before the instance counter moves.
  let roomy = super::LexTally::new(
    0,
    "saturated",
    1,
    super::AttemptCeiling::Derived(u128::MAX),
    usize::MAX,
  );
  let mut lexer = super::Budgeted::wrap(
    TileLexer::new("a"),
    super::Tallied {
      inner: (),
      tally: super::Rc::clone(&roomy),
    },
  );
  lexer.spent_here = usize::MAX - 1;
  lexer.spend();
  assert_eq!(
    lexer.spent_here,
    usize::MAX,
    "the last attempt a saturated instance ceiling allows must land on the ceiling"
  );
  assert_eq!(
    roomy.spent(),
    1,
    "and it was charged to the aggregate before the instance guard looked at it"
  );
}

/// The aggregate ceiling is the **same number at every target width**, which is the whole point of
/// counting in `u128`.
///
/// It did not used to be. Computed in `usize` the formula saturated, and on a 32-bit target it
/// saturated at 11,580 units at the default multiple and at 127 at the maximum — sources anyone
/// hands a test kit. The tally then stopped being a bound derived from the source and became a
/// flat `usize::MAX` cap, flat while the formula kept growing quadratically. Two lexers with
/// identical behaviour over an identical source got opposite verdicts because one host was
/// narrower than the other.
///
/// The `model` is the formula written out independently, so a change to either it or
/// `lex_attempt_ceiling` reds this cell rather than leaving a private model describing an
/// implementation that moved. The equality is now **exact and unclamped**: no `min(host_max)`,
/// because there is no host whose width the answer depends on. That is the regression pin — the
/// clamp is what a re-narrowed ceiling would need back.
///
/// `over_u32` is retained as a property of the *formula*: it is what makes 11,580 and 127
/// thresholds rather than an assertion that big numbers are big, and it is what says the four
/// bracketing cells still straddle the width that used to decide the verdict.
#[test]
fn the_aggregate_ceiling_is_the_same_number_at_every_target_width() {
  fn model(multiple: u128, units: u128) -> u128 {
    let budget = multiple * units + super::BUDGET_FLOOR as u128;
    (units + 3) * super::ATTEMPTS_PER_DRAIN_MULTIPLE as u128 * (budget + 1)
      + super::BUDGET_FLOOR as u128
  }

  const U32_MAX: u128 = u32::MAX as u128;
  let default = super::DEFAULT_BUDGET_MULTIPLE;
  let max = super::MAX_BUDGET_MULTIPLE;

  // The last row is the finding's own source: 100 KB at the default multiple, where the flat cap
  // fell about 75x short of what the formula asks for.
  for &(multiple, units, over_u32) in &[
    (default, 11_579, false),
    (default, 11_580, true),
    (max, 126, false),
    (max, 127, true),
    (default, 100_000, true),
  ] {
    let exact = model(multiple as u128, units as u128);
    assert_eq!(
      exact > U32_MAX,
      over_u32,
      "the 32-bit threshold moved: at multiple {multiple} over {units} units the aggregate \
       ceiling is {exact}, against a u32::MAX of {U32_MAX}"
    );

    // Every budget here is small — 800,064 at the largest — so it is representable at 32 bits as
    // well as at 64. Only the ceiling derived from it ever outgrew a `usize`.
    let budget = super::representable_budget(multiple, units)
      .expect("every one of these budgets is far below usize::MAX at 32 bits and at 64");
    let ceiling = super::lex_attempt_ceiling(units.saturating_add(3), budget);

    assert!(
      matches!(ceiling, super::AttemptCeiling::Derived(_)),
      "the aggregate ceiling at multiple {multiple} over {units} units came back as the kit's \
       counting capacity rather than the derived number {exact}; u128 has room for this by a \
       margin of about 2^70"
    );
    assert_eq!(
      ceiling.limit(),
      exact,
      "lex_attempt_ceiling disagrees with the formula at multiple {multiple} over {units} units \
       on a {}-bit host. If this reads {} the ceiling has been re-narrowed to the host's usize, \
       which is the defect the u128 count exists to remove",
      usize::BITS,
      usize::MAX
    );
  }
}

/// The `Capacity` arm, driven from the derivation rather than hand-built.
///
/// `u128` is finite, so `lex_attempt_ceiling` still has an arm for "this does not fit" — and no
/// source reaches it, which is exactly the condition under which an arm rots. The two cells that
/// exercise the kit-capacity *message* construct the tally directly; this one is the step before
/// them, and it is the only place the `checked_mul` chain's overflow branch is executed at all.
///
/// The arguments are ones the kit never produces: `representable_budget` returns strictly below
/// `usize::MAX`, so no real budget is `usize::MAX`. That is the point — the function is total over
/// its parameter types and not merely over the values the kit happens to pass, and a `saturating_`
/// chain that quietly returned `Derived(u128::MAX)` would be indistinguishable at every reachable
/// input.
///
/// The expected arm is worked out by hand rather than recomputed from the implementation, so this
/// is an oracle and not a mirror. At 64 bits `(2^64 - 1) * 4` is `2^66 - 4`, times a `budget + 1`
/// of `2^64` is about `2^130`, which is past `u128::MAX` — `Capacity`. At 32 bits the same product
/// is about `2^66`, which fits with 62 bits to spare — `Derived`.
#[test]
fn the_widest_possible_derivation_reports_capacity_rather_than_a_silent_maximum() {
  let widest = super::lex_attempt_ceiling(usize::MAX, usize::MAX);

  if usize::BITS >= 64 {
    assert!(
      matches!(widest, super::AttemptCeiling::Capacity),
      "a derivation of about 2^130 must report the kit's counting capacity, not a number; it \
       reported {}",
      widest.limit()
    );
    assert_eq!(
      widest.limit(),
      u128::MAX,
      "a capacity ceiling still counts as far as the counter goes"
    );
  } else {
    assert!(
      matches!(widest, super::AttemptCeiling::Derived(_)),
      "at {} bits the widest derivation is about 2^66 and fits in a u128 with room to spare, so \
       it is the derived number and not the kit's capacity",
      usize::BITS
    );
    assert!(
      widest.limit() < u128::MAX,
      "a derived ceiling that lands on u128::MAX is a saturation wearing the wrong label"
    );
  }
}

/// Why the width mattered: an **ordinary** lexer over an **ordinary** source outran a 32-bit
/// counter, and no knob could raise it past one.
///
/// This is not a dense-lexer corner. `check_partial` drives every split point of the source, so a
/// conforming `WithinSpan` lexer emitting one item per byte is asked to lex about `k + 1` times for
/// the prefix of length `k` — the items, plus the probe that ends the drain. Summed over every
/// split of a 100 KB source that is about five billion attempts *before* the complete drain and
/// the final partial drain are counted, and `usize::MAX` at 32 bits is 4,294,967,295.
///
/// So such a lexer passed at 64 bits and took an ordinary `lex-budget` refusal at 32 — the kit
/// reporting its own arithmetic as the lexer's fault. `budget_multiple` could not help: the cap is
/// `usize::MAX` whatever the multiple, so every permitted setting collapsed to the same number.
///
/// The demand is computed rather than driven, because driving it means five billion `lex` calls.
/// What the cell pins is the *relation* — the demand exceeds a 32-bit counter, the derived ceiling
/// exceeds the demand, and the ceiling is available at every width.
#[test]
fn a_conforming_per_byte_lexer_over_100kb_outruns_a_32_bit_counter() {
  const UNITS: usize = 100_000;
  const U32_MAX: u128 = u32::MAX as u128;

  // Prefix `k` costs `k` items plus one exhaustion probe, over `k = 0..=UNITS`.
  let prefix_demand: u128 = (UNITS as u128 + 1) * (UNITS as u128 + 2) / 2;
  assert!(
    prefix_demand > U32_MAX,
    "the reachability this cell is about has gone: a per-byte lexer over {UNITS} units demands \
     {prefix_demand} prefix attempts, which no longer exceeds the {U32_MAX} a 32-bit usize holds"
  );

  let budget = super::representable_budget(super::DEFAULT_BUDGET_MULTIPLE, UNITS)
    .expect("8 * 100_000 + 64 is representable at every width");
  let ceiling = super::lex_attempt_ceiling(UNITS.saturating_add(3), budget);
  assert!(
    ceiling.limit() > prefix_demand,
    "the aggregate ceiling {} does not admit the {prefix_demand} attempts a conforming per-byte \
     lexer spends on the prefix sweep alone",
    ceiling.limit()
  );
  assert!(
    matches!(ceiling, super::AttemptCeiling::Derived(_)),
    "the ceiling for an ordinary 100 KB source must be the derived number, never the kit's \
     counting capacity"
  );
}

// ── Positive: the crate's real logos adapter (LogosLexer) ───────────────────────────

#[cfg(feature = "logos_0_16")]
mod logos_adapter {
  use super::Harness;
  use crate::Token;
  use crate::lexer::LogosLexer;

  // A syntactic token that skips whitespace (leaves gaps): NOT lossless.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  enum SynKind {
    Word,
    Num,
  }

  impl core::fmt::Display for SynKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        SynKind::Word => f.write_str("word"),
        SynKind::Num => f.write_str("num"),
      }
    }
  }

  #[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
  #[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
  enum SynTok {
    #[regex(r"[a-z]+")]
    Word,
    #[regex(r"[0-9]+")]
    Num,
  }

  impl Token<'_> for SynTok {
    type Kind = SynKind;
    type Error = ();

    // `[a-z]+` and `[0-9]+` are over disjoint character classes and neither is a prefix of the
    // other, so the DFA never probes into a longer candidate and backtracks. `WithinSpan` is
    // honest, and it is what keeps the partial cell below checking real chunked equivalence:
    // at the conservative default this fixture would withhold everything and the check would
    // pass vacuously.
    const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::WithinSpan;

    fn kind(&self) -> SynKind {
      match self {
        SynTok::Word => SynKind::Word,
        SynTok::Num => SynKind::Num,
      }
    }
    fn is_trivia(&self) -> bool {
      false
    }
  }

  type SynLexer<'a> = LogosLexer<'a, SynTok>;

  #[test]
  fn logos_syntactic_passes() {
    Harness::<SynLexer<'_>>::over(["ab 12 cd", "one two three", "42", "  x  ", ""]).run();
  }

  #[test]
  fn logos_syntactic_passes_partial_equivalence() {
    // The real logos adapter (over `str`) is faithful under truncation, so it reassembles
    // chunk-by-chunk exactly like a single parse.
    Harness::<SynLexer<'_>>::over(["ab 12 cd", "one two three", "42", "  x  ", ""]).run_partial();
  }

  // A token where whitespace is a real token, so the stream tiles gap-free: lossless.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  enum TileKind {
    Word,
    Num,
    Ws,
  }

  impl core::fmt::Display for TileKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      match self {
        TileKind::Word => f.write_str("word"),
        TileKind::Num => f.write_str("num"),
        TileKind::Ws => f.write_str("ws"),
      }
    }
  }

  #[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
  #[logos(crate = crate::logos)]
  enum TileTok {
    #[regex(r"[a-z]+")]
    Word,
    #[regex(r"[0-9]+")]
    Num,
    #[regex(r"[ \t\r\n]+")]
    Ws,
  }

  impl Token<'_> for TileTok {
    type Kind = TileKind;
    type Error = ();

    const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

    fn kind(&self) -> TileKind {
      match self {
        TileTok::Word => TileKind::Word,
        TileTok::Num => TileKind::Num,
        TileTok::Ws => TileKind::Ws,
      }
    }
    fn is_trivia(&self) -> bool {
      matches!(self, TileTok::Ws)
    }
  }

  type TileLogosLexer<'a> = LogosLexer<'a, TileTok>;

  #[test]
  fn logos_lossless_tiling_passes() {
    Harness::<TileLogosLexer<'_>>::over(["ab 12 cd", "one two", "42"])
      .lossless()
      .run();
  }
}

// ── The falsifier that predates the feature ───────────────────────────────────────────
//
// A float-and-exponent vocabulary over the real logos adapter, with **zero callbacks**. This is
// the shape #282's fix exists for, and the check that catches it is older than the fix: run
// against tokora 0.9.1 — the pre-`read_frontier` holdback, which keyed on the item's span —
// `run_partial` over `"1.5"` and `"5e-3"` already failed at split k=2 with
//
//   tokora conformance [input #0 partial-equivalence] split k=2: prefix token count diverges
//   from the complete prefix: expected 0, got 1
//
// The extra token is `Int@0..1`. logos probes into the `Float`/`Sci` arm, hits the end of the
// truncated buffer, and backtracks to the accepting prefix; the span rule then saw end 1 < 2 and
// committed it. Append the missing byte and the complete parse says `Float@0..3`.
#[cfg(feature = "logos_0_16")]
mod prefix_backtracking {
  use super::Harness;
  use crate::Token;
  use crate::lexer::LogosLexer;

  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  enum NumKind {
    Int,
    Float,
    Sci,
    Dot,
    Word,
  }

  impl core::fmt::Display for NumKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str(match self {
        NumKind::Int => "int",
        NumKind::Float => "float",
        NumKind::Sci => "sci",
        NumKind::Dot => "dot",
        NumKind::Word => "word",
      })
    }
  }

  macro_rules! num_vocabulary {
    // No class argued at the call site. The const has no default to fall into, so this arm
    // supplies the conservative value the vocabulary used to inherit — which is what the
    // `DefaultNum` cells below have always been exercising. Written as a separate arm rather
    // than as `$class = <Self as Token>::SCAN_LOOKAHEAD`, which is a const cycle.
    ($name:ident) => {
      num_vocabulary!(@body $name, const SCAN_LOOKAHEAD: crate::ScanLookahead =
        crate::ScanLookahead::Unbounded;);
    };
    ($name:ident, $class:expr) => {
      num_vocabulary!(@body $name, const SCAN_LOOKAHEAD: crate::ScanLookahead = $class;);
    };
    (@body $name:ident, $($class:tt)*) => {
      #[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
      #[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
      enum $name {
        #[regex(r"[0-9]+")]
        Int,
        #[regex(r"[0-9]+\.[0-9]+")]
        Float,
        #[regex(r"[0-9]+e[+-]?[0-9]+")]
        Sci,
        #[token(".")]
        Dot,
        #[regex(r"[a-z]+")]
        Word,
      }

      impl Token<'_> for $name {
        type Kind = NumKind;
        type Error = ();

        $($class)*

        fn kind(&self) -> NumKind {
          match self {
            $name::Int => NumKind::Int,
            $name::Float => NumKind::Float,
            $name::Sci => NumKind::Sci,
            $name::Dot => NumKind::Dot,
            $name::Word => NumKind::Word,
          }
        }
        fn is_trivia(&self) -> bool {
          false
        }
      }
    };
  }

  // The claim a vocabulary like this must NOT make, and the conservative one beside it.
  num_vocabulary!(LyingNum, crate::ScanLookahead::WithinSpan);
  num_vocabulary!(DefaultNum);

  type LyingLexer<'a> = LogosLexer<'a, LyingNum>;
  type DefaultLexer<'a> = LogosLexer<'a, DefaultNum>;

  #[test]
  #[should_panic(expected = "partial-equivalence")]
  fn a_span_end_claim_over_a_float_vocabulary_is_falsified() {
    // `"1.5"` at split k=2: the prefix `"1."` commits `Int@0..1` under a `WithinSpan` claim, and
    // the complete parse has no token ending before 2 at all.
    Harness::<LyingLexer<'_>>::over(["1.5"]).run_partial();
  }

  #[test]
  #[should_panic(expected = "partial-equivalence")]
  fn a_span_end_claim_over_an_exponent_vocabulary_is_falsified() {
    // `"5e-3"` at split k=2, the PromQL shape from the issue, with no callback in sight.
    Harness::<LyingLexer<'_>>::over(["5e-3"]).run_partial();
  }

  #[test]
  fn the_conservative_default_is_sound_over_the_same_vocabulary() {
    // The same two sources, with the class left at its `Unbounded` default: nothing is yielded
    // while the stream is open, so nothing unstable is committed and the check passes. Sound,
    // and the cost §7 of the design names — the caller buffers to the seal.
    Harness::<DefaultLexer<'_>>::over(["1.5", "5e-3", "1.", "5e", "5ex"]).run_partial();
  }

  /// The tier audits a **corpus**, not a vocabulary — the limitation
  /// [`Token::SCAN_LOOKAHEAD`](crate::Token::SCAN_LOOKAHEAD) states as an obligation on
  /// whoever writes `WithinSpan`, executed here so it is not left as prose.
  ///
  /// Same lying vocabulary, same lie, same *truncation* — and it **passes**, because the corpus
  /// omits the source the truncation would diverge from. `"1."` truncated at k=2 is `"1."` itself,
  /// whose complete parse also begins `Int@0..1`, so the prefix drain is a faithful prefix of it
  /// and nothing is observed. Only a corpus containing `"1.5"`, where the *longer* rule wins,
  /// makes the committed `Int@0..1` an item the complete parse does not have.
  ///
  /// So a green `run_partial` is evidence about the sources it was given. Deriving the corpus from
  /// the rules — one source per prefix-related pair, long enough for the longer rule to win — is
  /// the part that cannot be delegated to the kit, and this cell is what makes that concrete
  /// rather than a caveat someone reads past.
  #[test]
  fn the_same_lie_passes_over_a_corpus_that_omits_the_longer_source() {
    Harness::<LyingLexer<'_>>::over(["1."]).run_partial();
  }
}

// ── The other half of the value channel: a recorded value that is too low ──────────────
//
// `run_partial` above falsifies a wrong CLASS CLAIM. A recorded [`Probe`](crate::Probe)
// bypasses the class entirely — it answers for its item outright — so it needs its own
// falsifier, and this is it.
//
// It is also the answer to the "several candidates at the same start" question. Within one
// `lex()` call logos's scan starts are strictly increasing: a callback runs only at the leaf the
// DFA accepted, and `Filter::Skip` advances `token_start` to that match's end before rescanning,
// so no two scans in one call begin at the same offset and an accepted value always comes from
// the scan that produced the item. What provenance therefore cannot police is what that scan's
// callback did NOT see: the engine probes past its own match before settling, and a recorder
// that reports only its own bytes under-reports for its own item. That is the burden
// `State::take_probe` puts on the recorder, and the two cells below are the check on it.
#[cfg(feature = "logos_0_16")]
mod recorded_value {
  use super::Harness;
  use crate::lexer::LogosLexer;
  use crate::{Probe, State, Token};

  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  enum NumKind {
    Int,
    Float,
    Dot,
  }

  impl core::fmt::Display for NumKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str(match self {
        NumKind::Int => "int",
        NumKind::Float => "float",
        NumKind::Dot => "dot",
      })
    }
  }

  /// A state that records exactly what its callback was told to record. `HONEST` is a const
  /// parameter rather than a field because the kit builds its lexers with `L::new`, which takes
  /// the state's `Default`.
  #[derive(Clone, Copy, Debug, Default, PartialEq)]
  struct Recorder<const HONEST: bool> {
    probe: Option<Probe>,
  }

  impl<const HONEST: bool> State for Recorder<HONEST> {
    type Error = ();

    fn check(&self) -> Result<(), ()> {
      Ok(())
    }

    fn take_probe(&mut self) -> Option<Probe> {
      self.probe.take()
    }
  }

  /// The integer callback, shared by both vocabularies and living OUTSIDE the macro so its
  /// body is ordinary code: rustfmt does not format a `macro_rules!` body, and three
  /// toolchains' idea of the indentation inside one is three different answers.
  ///
  /// The engine probes at most one byte past an integer in this vocabulary: it reads `.` at
  /// `span.end` and then wants a digit at `span.end + 1` for the `Float` arm. `span.end + 1` is
  /// therefore an upper bound on every probe — over-reporting by one where the byte at
  /// `span.end` is not a `.`, which only over-withholds. `span.end` is the callback's own view,
  /// "I read my own bytes", and it is a byte short of the truth exactly when the `Float` trial
  /// ran off the end of the buffer.
  fn record<'s, T, const HONEST: bool>(lex: &mut crate::logos::Lexer<'s, T>)
  where
    T: crate::logos::Logos<'s, Extras = Recorder<HONEST>>,
  {
    let span = lex.span();
    let reach = if HONEST { span.end + 1 } else { span.end };
    lex.extras.probe = Some(Probe::new(span.start, reach));
  }

  macro_rules! recording_vocabulary {
    ($name:ident, $honest:expr) => {
      #[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
      #[logos(crate = crate::logos, extras = Recorder<$honest>, skip r"[ \t\r\n]+")]
      enum $name {
        // The item's OWN scan records, so provenance accepts the value and the class claim is
        // never consulted. `$honest` decides whether the value covers what deciding the item
        // actually read — see `record`.
        #[regex(r"[0-9]+", record)]
        Int,
        #[regex(r"[0-9]+\.[0-9]+")]
        Float,
        #[token(".")]
        Dot,
      }

      impl Token<'_> for $name {
        type Kind = NumKind;
        type Error = ();

        const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

        fn kind(&self) -> NumKind {
          match self {
            $name::Int => NumKind::Int,
            $name::Float => NumKind::Float,
            $name::Dot => NumKind::Dot,
          }
        }
        fn is_trivia(&self) -> bool {
          false
        }
      }
    };
  }

  recording_vocabulary!(UnderReportingNum, false);
  recording_vocabulary!(HonestNum, true);

  type UnderReportingLexer<'a> = LogosLexer<'a, UnderReportingNum>;
  type HonestLexer<'a> = LogosLexer<'a, HonestNum>;

  #[test]
  #[should_panic(expected = "partial-equivalence")]
  fn a_recorded_value_that_misses_the_engines_backtracking_is_falsified() {
    // `"1.5"` at split k=2. The prefix `"1."` probes offset 2, finds end of input, and
    // backtracks to `Int@0..1`; the callback recorded `ReadTo(1)`, the driver floors that at the
    // span end 1, and `1 < 2` commits it. The complete parse is one `Float@0..3` and has no
    // token ending before 2 at all.
    //
    // The declared class is `Unbounded` and would have withheld everything, so this cell fails
    // only because the value was ACCEPTED — which makes it a check on the accept path as much
    // as on the recorder.
    Harness::<UnderReportingLexer<'_>>::over(["1.5"]).run_partial();
  }

  #[test]
  fn a_recorded_value_that_covers_the_backtracking_passes() {
    // Same vocabulary, same sources, one byte more honesty: `span.end + 1` covers the probe the
    // `Float` trial makes, so the unstable integer is withheld and the stable ones still yield.
    // What `run_partial` rejects is the under-report, not the recording.
    Harness::<HonestLexer<'_>>::over(["1.5", "1.", "12 34", "1.5 2.5"]).run_partial();
  }
}

// ── #269: the trait tier compares the ITEM, and a rendering is not the item ─────────────
//
// Three cells over the two comparisons `run` draws a verdict from, plus the conforming-lexer
// control that the rendering key reddened. The partial tier already had this repair and its own
// three cells (`a_colliding_debug_...`, `an_unstable_debug_...`, `a_token_payload_that_changes_...`
// above); these are the same defects one tier over, where they survived a release longer.

/// Which `Collide` variant the next fresh construction hands out. Flipped by `new` alone, so two
/// *fresh* runs alternate while a `with_state` resume reproduces the run it was captured from —
/// which isolates the replay comparison from the resume one.
static ALT_FLIP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// A lexer's choice, carried in `L::State` so a resume inherits it rather than re-flipping.
#[derive(Clone, Copy, Debug)]
struct AltChoice(bool);

impl crate::State for AltChoice {
  type Error = Infallible;
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
}

/// The first item is always an error and *which* error alternates between fresh constructions.
/// Spans, slices, kinds, item count and the error-vs-token discriminant are all held constant, so
/// the only thing that moves is the payload — and `Collide`'s `Debug` renders both as `LexError`.
struct AltErrLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: AltChoice,
}

impl<'a> Lexer<'a> for AltErrLexer<'a> {
  type State = AltChoice;
  type Source = str;
  type Token = CTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    let pick = ALT_FLIP.fetch_xor(true, core::sync::atomic::Ordering::Relaxed);
    Self {
      src,
      start: 0,
      end: 0,
      state: AltChoice(pick),
    }
  }
  fn with_state(src: &'a str, state: AltChoice) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Collide> {
    Ok(())
  }
  fn state(&self) -> &AltChoice {
    &self.state
  }
  fn state_mut(&mut self) -> &mut AltChoice {
    &mut self.state
  }
  fn into_state(self) -> AltChoice {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<CTok, Collide>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    if self.start == 0 {
      return Some(Err(if self.state.0 {
        Collide::Junk
      } else {
        Collide::Cut
      }));
    }
    Some(Ok(CTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "replay-identity] position 0: item mismatch: expected lexer error")]
fn an_error_value_that_changes_between_fresh_runs_is_falsified() {
  ALT_FLIP.store(false, core::sync::atomic::Ordering::Relaxed);
  // The observation the check has to make, made directly first: the two public error VALUES
  // differ and their `Debug` renderings do not. `run()` compared the rendering and passed (#269).
  let ea = AltErrLexer::new("ab").lex().unwrap().unwrap_err();
  let eb = AltErrLexer::new("ab").lex().unwrap().unwrap_err();
  assert_ne!(ea, eb);
  assert_eq!(format!("{ea:?}"), format!("{eb:?}"));

  ALT_FLIP.store(false, core::sync::atomic::Ordering::Relaxed);
  Harness::<AltErrLexer<'_>>::new("ab").run();
}

#[test]
fn a_conforming_lexer_with_an_unstable_debug_passes_run() {
  // The other direction, and the reason correcting the docs to describe rendering-equality would
  // not have been a fix either: `TickingErrLexer` is conforming in every respect the contract
  // names, and its payload's `Debug` never renders the same way twice. Comparing the rendering
  // reddened it — `expected err="junk#0", got err="junk#1"` — on a lexer with no defect at all.
  Harness::<TickingErrLexer<'_>>::over(["a?b", "??", "?", ""]).run();
}

/// Which token payload the next fresh construction hands out; the token twin of [`ALT_FLIP`].
static ALT_TOK_FLIP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// [`AltErrLexer`]'s defect on the token arm: same kind, same span, same slice, same item count,
/// and a **payload** that alternates between fresh runs. Nothing the trait tier used to compare
/// moves, so this passed `run()` for the third reason the kind projection is not the token.
struct AltTokLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: AltChoice,
}

impl<'a> Lexer<'a> for AltTokLexer<'a> {
  type State = AltChoice;
  type Source = str;
  type Token = VTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    let pick = ALT_TOK_FLIP.fetch_xor(true, core::sync::atomic::Ordering::Relaxed);
    Self {
      src,
      start: 0,
      end: 0,
      state: AltChoice(pick),
    }
  }
  fn with_state(src: &'a str, state: AltChoice) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &AltChoice {
    &self.state
  }
  fn state_mut(&mut self) -> &mut AltChoice {
    &mut self.state
  }
  fn into_state(self) -> AltChoice {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<VTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(VTok(u8::from(self.state.0))))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "replay-identity] position 0: item mismatch: expected token VKind")]
fn a_token_payload_that_changes_between_fresh_runs_is_falsified() {
  ALT_TOK_FLIP.store(false, core::sync::atomic::Ordering::Relaxed);
  Harness::<AltTokLexer<'_>>::new("ab").run();
}

/// Conforming under two fresh `new`s and **not** under a resume: `with_state` flips the choice,
/// so the suffix a restore reproduces carries the other `Collide` variant at the same span and
/// slice. Isolates `check_resume`'s comparison from `assert_run_eq`'s — replay identity passes.
struct ResumeDriftErrLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: AltChoice,
}

impl<'a> Lexer<'a> for ResumeDriftErrLexer<'a> {
  type State = AltChoice;
  type Source = str;
  type Token = CTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: AltChoice(false),
    }
  }
  fn with_state(src: &'a str, _state: AltChoice) -> Self {
    // The defect: a rebuilt lexer decides the error differently from the one that saved the state.
    Self {
      src,
      start: 0,
      end: 0,
      state: AltChoice(true),
    }
  }
  fn check(&self) -> Result<(), Collide> {
    Ok(())
  }
  fn state(&self) -> &AltChoice {
    &self.state
  }
  fn state_mut(&mut self) -> &mut AltChoice {
    &mut self.state
  }
  fn into_state(self) -> AltChoice {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<CTok, Collide>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    if self.start == 0 {
      return Some(Err(if self.state.0 {
        Collide::Junk
      } else {
        Collide::Cut
      }));
    }
    Some(Ok(CTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "state-resume] resume-from k=0, position 0: resumed item diverges")]
fn a_resumed_error_value_that_moved_is_falsified() {
  // Check 1 passes here — both fresh runs say `Cut` — so this reds `check_resume`'s comparison
  // and only that one. Prefix replay is the assumption the whole input layer rests on.
  Harness::<ResumeDriftErrLexer<'_>>::over(["ab"]).run();
}

// ── #295: a payload that will not equal itself is not a lexer defect ────────────────────
//
// One cell per comparison the sweep guarded, the two controls that say the guard has not traded
// a false red for a false green, and the direct-call cells for the two comparisons an earlier
// check reaches first. Every guard sits on a path that has ALREADY failed its comparison, so a
// reflexive payload can never reach one — which is what makes the controls structural rather
// than lucky.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FKind;

impl core::fmt::Display for FKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("f")
  }
}

/// A payload holding an `f64`. Its derived `PartialEq` is **non-reflexive** exactly when the float
/// is `NaN` — the case 38 pql corpus queries reached — and reflexive at every other value, which
/// is what lets the control differ from the plant in one thing only.
#[derive(Clone, Debug, PartialEq)]
struct FTok(f64);

impl Token<'_> for FTok {
  type Kind = FKind;
  type Error = Infallible;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> FKind {
    FKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

/// `NaN` for `n`, the byte's own value otherwise.
fn float_of(b: u8) -> f64 {
  if b == b'n' { f64::NAN } else { f64::from(b) }
}

/// [`ValueLexer`] over a float payload, at the same two modes: `OWN_BYTE` is conforming and
/// `LAST_BYTE` reads the payload off the end of the buffer.
struct FloatLexer<'a, const MODE: u8> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a, const MODE: u8> Lexer<'a> for FloatLexer<'a, MODE> {
  type State = ();
  type Source = str;
  type Token = FTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<FTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    let bytes = self.src.as_bytes();
    let byte = match MODE {
      OWN_BYTE => bytes[self.start],
      _ => *bytes.last().expect("the source is non-empty here"),
    };
    Some(Ok(FTok(float_of(byte))))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

type NanPayloadLexer<'a> = FloatLexer<'a, OWN_BYTE>;
type DriftingFloatLexer<'a> = FloatLexer<'a, LAST_BYTE>;

#[test]
#[should_panic(
  expected = "non-reflexive-payload] partial-equivalence split k=18446744073709551615, position 0: INCONCLUSIVE — the token payload"
)]
fn a_nan_payload_is_diagnosed_and_not_blamed_on_the_lexer() {
  // The filed case. The final leg compares the complete stream against a final drain of the SAME
  // source, so before this it reported `partial-equivalence` — a red accusing the lexer, with
  // `expected token FKind@0..1 (payload FTok(NaN)), got token FKind@0..1 (payload FTok(NaN))`:
  // two things that render identically.
  Harness::<NanPayloadLexer<'_>>::over(["n"]).run_partial();
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] replay-identity position 0: INCONCLUSIVE — the token payload"
)]
fn a_nan_payload_is_diagnosed_in_the_trait_tier_too() {
  // The trait tier compared a `kind` and a `Debug` string, so a `NaN` payload was invisible to it
  // and this was green. #269's repair is what puts the value in front of a comparison here — the
  // guard has to arrive with it, or the repair introduces the defect one tier over.
  Harness::<NanPayloadLexer<'_>>::over(["n"]).run();
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] state-resume resume-from k=0, position 0: INCONCLUSIVE — the token payload"
)]
fn a_nan_payload_is_diagnosed_at_the_resume_comparison() {
  // `check_resume`'s comparison, reached directly because `run` runs replay identity first and a
  // non-reflexive payload reds every comparison it reaches — so no lexer can arrive here through
  // the entry point. A guard that is unreachable through one order of checks is still a guard:
  // reachability is an argument about check ORDER, not a bound.
  let budget = 8 * "n".len() + 64;
  let reference = super::lex_run::<NanPayloadLexer<'_>>(0, "n", budget);
  super::check_resume::<NanPayloadLexer<'_>>(0, "n", &reference, budget);
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] partial-equivalence split k=1, position 0: INCONCLUSIVE — the token payload"
)]
fn a_nan_payload_is_diagnosed_at_the_prefix_comparison() {
  // `assert_partial_prefix_of`'s comparison, reached directly for `check_resume`'s reason: the
  // final leg runs before the split loop and fires first.
  use super::StreamItem;
  let nan = || StreamItem::<NanPayloadLexer<'_>>::Token(FTok(f64::NAN), SimpleSpan::new(0, 1));
  super::assert_partial_prefix_of::<NanPayloadLexer<'_>>(0, 1, &[nan()], &[nan()]);
}

#[test]
fn a_reflexive_payload_of_the_same_type_still_fails_the_ordinary_way() {
  // THE CONTROL, and the whole point of it: same payload type, same lexer shape, same source
  // length, genuinely non-conforming — its payload is read off the end of the buffer — and never
  // `NaN`. It must still red `partial-equivalence`, or the repair has traded a false red for a
  // false green. It is structural rather than lucky: every guard sits on a path a failed
  // comparison has already taken, and a reflexive value never fails against itself.
  let panicked = std::panic::catch_unwind(|| {
    Harness::<DriftingFloatLexer<'_>>::over(["abz"]).run_partial();
  })
  .unwrap_err();
  let msg = panicked
    .downcast_ref::<String>()
    .expect("the kit panics with a formatted message");
  assert!(
    msg.contains("partial-equivalence] split k=2, position 0: prefix item diverges"),
    "the control must keep the ORDINARY tag, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "the control must not be re-labelled as a payload problem, got: {msg}"
  );
  // And expected-vs-got must be two distinguishable values, unlike the NaN cell's.
  assert!(
    msg.contains("FTok(122.0)") && msg.contains("FTok(98.0)"),
    "{msg}"
  );
}

#[test]
fn a_conforming_lexer_over_the_same_float_payload_passes_both_tiers() {
  // The positive control: the payload TYPE is not what any of this rejects.
  Harness::<NanPayloadLexer<'_>>::over(["ab", "a", ""]).run();
  Harness::<NanPayloadLexer<'_>>::over(["ab", "a", ""]).run_partial();
}

/// A `Kind` that claims `Eq` and answers `false` to every comparison, itself included.
///
/// `Token::Kind` is bounded `Eq`, so this is a **broken total-equality promise** rather than the
/// partial one a token or error payload carries — a stronger caller defect, and exactly as wrong
/// to report as a stream divergence. It stands in for the whole `Eq`/`Ord` half of the population:
/// `Lexer::Span` is bounded `Ord` and can lie the same way.
#[derive(Clone, Copy, Debug)]
struct LyingKind;

impl PartialEq for LyingKind {
  fn eq(&self, _: &Self) -> bool {
    false
  }
}

impl Eq for LyingKind {}

/// Hand-written because `Token::Kind` requires `Hash` and deriving it beside a manual `PartialEq`
/// is what `clippy::derived_hash_with_manual_eq` exists to stop. The lint's law —
/// `k1 == k2` implies `hash(k1) == hash(k2)` — is satisfied *vacuously* here, since nothing is
/// equal to anything, so this impl is not a second lie on top of the one the fixture is for.
impl core::hash::Hash for LyingKind {
  fn hash<H: core::hash::Hasher>(&self, _: &mut H) {}
}

impl core::fmt::Display for LyingKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("lying")
  }
}

#[derive(Clone, Debug, PartialEq)]
struct LTok;

impl Token<'_> for LTok {
  type Kind = LyingKind;
  type Error = Infallible;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> LyingKind {
    LyingKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

/// A per-character lexer, conforming in everything the contract names, over [`LyingKind`].
struct LyingKindLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for LyingKindLexer<'a> {
  type State = ();
  type Source = str;
  type Token = LTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<LTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(LTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] replay-identity position 0: INCONCLUSIVE — the token's `Kind`"
)]
fn a_kind_that_does_not_equal_itself_is_diagnosed_and_not_blamed_on_the_lexer() {
  Harness::<LyingKindLexer<'_>>::over(["ab"]).run();
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] integration/peek-heavy position 0: INCONCLUSIVE — the token's `Kind`"
)]
fn a_kind_that_does_not_equal_itself_is_diagnosed_at_the_committed_stream_comparison() {
  // The integration tier's comparison, reached directly: `run` rejects such a `Kind` at replay
  // identity, three checks earlier, so nothing arrives here through the entry point. Guarding four
  // comparisons and not the fifth would leave the same defect one relocation away.
  let stream = [super::StreamItem::<LyingKindLexer<'_>>::Token(
    LTok,
    SimpleSpan::new(0, 1),
  )];
  super::assert_stream_eq::<LyingKindLexer<'_>>(
    0,
    "peek-heavy",
    super::COMMITTED,
    &stream,
    &stream,
  );
}

// ── #269 at the integration tier: the committed stream is the ITEM, not a projection of it ──
//
// The trait tier compares the whole item, the partial tier compares the whole item, and the
// integration tier reduced every committed token to `(kind, span)` — the same projection #269
// removed one tier over, on streams whose full `L::Token` values were already in hand.

/// How many `Lexer::new` constructions the kit has made in the cell below. `run()` spends a fixed
/// number of them on the trait tier and then exactly one per integration schedule (`drive` →
/// `seeded` → `L::new(src).into_state()`; every lexer the input layer itself builds comes through
/// `with_state`), so the instance index is a stable handle on WHICH of the five integration
/// streams a token belongs to.
static FLIP_INSTANCE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// The instance index the payload flips at, calibrated so that every trait-tier construction and
/// the straight integration drive fall below it and `peek-heavy` is the first above.
const FLIP_AT: usize = 6;

/// The lexer's choice, carried in `L::State` so a `with_state` resume — which is every lexer the
/// input layer builds — inherits the payload of the drive it belongs to rather than re-deciding.
#[derive(Clone, Copy, Debug)]
struct InstanceChoice(u8);

impl crate::State for InstanceChoice {
  type Error = Infallible;
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
}

/// A one-token-per-character lexer whose kind, span, slice, item count and state resume are all
/// exactly what a conforming lexer produces, and whose token **payload** is decided by which
/// `L::new` instance the stream came from.
struct InstanceFlipLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: InstanceChoice,
}

impl<'a> Lexer<'a> for InstanceFlipLexer<'a> {
  type State = InstanceChoice;
  type Source = str;
  type Token = VTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    let n = FLIP_INSTANCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    Self {
      src,
      start: 0,
      end: 0,
      state: InstanceChoice(u8::from(n >= FLIP_AT)),
    }
  }
  fn with_state(src: &'a str, state: InstanceChoice) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &InstanceChoice {
    &self.state
  }
  fn state_mut(&mut self) -> &mut InstanceChoice {
    &mut self.state
  }
  fn into_state(self) -> InstanceChoice {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<VTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(VTok(self.state.0)))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(
  expected = "integration/peek-heavy] position 0: committed token stream diverges: expected token"
)]
fn a_token_payload_that_moves_between_two_integration_schedules_is_falsified() {
  // The observation the tier has to make, made directly first and without spending an instance:
  // the two streams' tokens agree on the kind and the span — everything this tier compared before
  // 0.11.0 — and hold different values. `with_state` is the door every lexer the input layer
  // builds comes through, so this is literally the pair the two drives lex with.
  let low = InstanceFlipLexer::with_state("x", InstanceChoice(0))
    .lex()
    .expect("the source has one token")
    .expect("this lexer never errors");
  let high = InstanceFlipLexer::with_state("x", InstanceChoice(1))
    .lex()
    .expect("the source has one token")
    .expect("this lexer never errors");
  assert_eq!(low.kind(), high.kind(), "the kind is what held still");
  assert_ne!(low, high, "the value is what moved");

  FLIP_INSTANCE.store(0, core::sync::atomic::Ordering::Relaxed);
  // Every trait-tier check passes: two fresh runs, the resume at every position and the sticky
  // and span-after-exhaustion probes all fall below the flip. The straight integration drive is
  // the sixth construction and `peek-heavy` the seventh, so the two committed streams hold
  // `VTok(0)` and `VTok(1)` at the same kind, the same span and the same length — and the tier
  // reported them equal until it stopped comparing `(kind, span)`.
  Harness::<InstanceFlipLexer<'_>>::new("x").run();
}

/// [`InstanceFlipLexer`]'s defect on the **error** arm: one refused region per input, whose
/// payload is decided by which `L::new` instance the drive came from.
///
/// The token arm and the error arm are the two halves of an item, and until 0.11.0 the
/// integration tier held neither: it compared the token through a projection and ran under
/// [`Silent`](crate::emitter::Silent), which discards a refusal outright. A refusal leaves no
/// trace in the token stream, so this lexer's five schedules produced five *identical* (empty)
/// token streams while the value a parser's error channel receives changed.
struct InstanceFlipErrLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: InstanceChoice,
}

impl<'a> Lexer<'a> for InstanceFlipErrLexer<'a> {
  type State = InstanceChoice;
  type Source = str;
  type Token = ETok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    let n = ERR_FLIP_INSTANCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    Self {
      src,
      start: 0,
      end: 0,
      state: InstanceChoice(u8::from(n >= FLIP_AT)),
    }
  }
  fn with_state(src: &'a str, state: InstanceChoice) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), BadByte> {
    Ok(())
  }
  fn state(&self) -> &InstanceChoice {
    &self.state
  }
  fn state_mut(&mut self) -> &mut InstanceChoice {
    &mut self.state
  }
  fn into_state(self) -> InstanceChoice {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<ETok, BadByte>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Err(BadByte {
      reason: if self.state.0 == 0 { "low" } else { "high" },
    }))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

/// [`FLIP_INSTANCE`]'s twin for the error-arm fixture; separate so the two cells can run
/// concurrently without either one moving the other's instance index.
static ERR_FLIP_INSTANCE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[test]
#[should_panic(
  expected = "integration/peek-heavy] error position 0: raised lexer-error stream diverges: expected lexer error"
)]
fn an_error_payload_that_moves_between_two_integration_schedules_is_falsified() {
  let low = InstanceFlipErrLexer::with_state("?", InstanceChoice(0))
    .lex()
    .expect("the source has one item")
    .expect_err("this lexer only errors");
  let high = InstanceFlipErrLexer::with_state("?", InstanceChoice(1))
    .lex()
    .expect("the source has one item")
    .expect_err("this lexer only errors");
  assert_ne!(low, high, "the error payload is what moved");

  ERR_FLIP_INSTANCE.store(0, core::sync::atomic::Ordering::Relaxed);
  // Both committed TOKEN streams are empty and every span agrees, so nothing the tier compared
  // before 0.11.0 — under `Silent`, which never handed it a refusal at all — could see this.
  Harness::<InstanceFlipErrLexer<'_>>::new("?").run();
}

#[test]
fn a_span_divergence_at_this_tier_is_still_reported_the_way_it_always_was() {
  // The control for the extension, and the reason this is not a rewrite: the tier compares MORE
  // than it did, and it must say exactly what it always said about a divergence the span decided.
  // `Lexer::Span` is bounded `Ord`, so the comparison that drew the verdict is sound and there is
  // nothing to refuse — same tier, same tag, same words. Only the rendering of the two sides
  // grew, because an item now carries the payload the verdict is entitled to show.
  let expected = [super::StreamItem::<InstanceFlipLexer<'_>>::Token(
    VTok(7),
    SimpleSpan::new(0, 1),
  )];
  let got = [super::StreamItem::<InstanceFlipLexer<'_>>::Token(
    VTok(7),
    SimpleSpan::new(1, 2),
  )];
  let msg = panic_message(|| {
    super::assert_stream_eq::<InstanceFlipLexer<'_>>(
      0,
      "peek-heavy",
      super::COMMITTED,
      &expected,
      &got,
    );
  });
  assert!(
    msg.contains(
      "[input #0 integration/peek-heavy] position 0: committed token stream diverges: expected"
    ),
    "a span divergence must keep the tier's own verdict and wording, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "the span decided it and a span is reflexive, so there is nothing to refuse, got: {msg}"
  );
}

#[test]
#[should_panic(expected = "cache conformance [cache non-reflexive-payload]")]
fn the_cache_kit_diagnoses_a_non_reflexive_token_instead_of_blaming_the_cache() {
  // The cache tier draws its verdicts from the same population — its observable is the whole
  // `(span, token, state)` triple and it asks `PartialEq` of two of the three — so it needs the
  // same guard, or the sweep stops one tier short. Eight `NaN`-payload tokens, a conforming
  // cache: before this the kit reported `entry-token`, "the entry at 0..1 is a DIFFERENT token
  // from the one stored there — expected FTok(NaN), got FTok(NaN)".
  crate::conformance::cache::CacheHarness::<
    NanPayloadLexer<'_>,
    crate::cache::DefaultCache<'_, NanPayloadLexer<'_>>,
  >::new("nnnnnnnn")
  .run();
}

#[test]
fn the_cache_kit_still_runs_over_a_reflexive_payload_of_the_same_type() {
  // The cache tier's control, and it is the same shape as the partial tier's: the payload TYPE is
  // not what the guard rejects, only a value that will not equal itself.
  crate::conformance::cache::CacheHarness::<
    NanPayloadLexer<'_>,
    crate::cache::DefaultCache<'_, NanPayloadLexer<'_>>,
  >::new("abcdefgh")
  .run();
}

// ── The reviewer's counterexample: a divergence the DISCRIMINANT decided ────────────

/// A lexer error payload holding an `f64` — [`FTok`]'s twin on the error arm, non-reflexive
/// exactly at `NaN`.
#[derive(Clone, Debug, PartialEq)]
struct FErr(f64);

/// A token whose *error* type is the `NaN`-capable one. Its own payload is trivial: the whole
/// point of the fixture is that the non-reflexive value sits on the arm that is NOT compared,
/// because the two sides are not the same arm at all.
#[derive(Clone, Debug, PartialEq)]
struct NTok;

impl Token<'_> for NTok {
  type Kind = FKind;
  type Error = FErr;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> FKind {
    FKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

/// [`ErrLexer`]'s `FLIPS_TO_TOKEN` defect over a `NaN`-carrying error payload: the first item is
/// an **error** unless the buffer's last byte is `z`, so the prefix `"ab"` yields
/// `LexerError(0..1, FErr(NaN))` where the complete parse of `"abz"` has `NTok@0..1`.
///
/// The truncation nonconformance is the same one [`FlippingErrLexer`] carries — those two differ
/// in the error payload and in nothing else, which is what makes them a differential pair.
struct NanErrFlipLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for NanErrFlipLexer<'a> {
  type State = ();
  type Source = str;
  type Token = NTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), FErr> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<NTok, FErr>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    let sealed = self.src.as_bytes().last() == Some(&b'z');
    if self.start == 0 && !sealed {
      return Some(Err(FErr(f64::NAN)));
    }
    Some(Ok(NTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
fn a_discriminant_divergence_is_reported_even_when_a_payload_will_not_compare() {
  // THE COUNTEREXAMPLE. `"abz"` at split k=2: the complete parse has `NTok@0..1` and the prefix
  // `"ab"` yields `LexerError(0..1, FErr(NaN))`. The two sides are not the same ARM, so the
  // comparison is settled by the discriminant and no caller equality is consulted at all — and a
  // whole-item scan, which runs afterwards and knows nothing about why, still found the `NaN` and
  // refused. That refusal claimed the kit had convicted the lexer of nothing, over exactly the
  // truncation nonconformance the panic below is about.
  //
  // Its reflexive twin is `an_error_that_becomes_a_token_on_append_is_falsified` above:
  // `FlippingErrLexer` carries the same defect with a payload that equals itself, and it has
  // always reported the ordinary tag. The two differ in the payload and in nothing else.
  let panicked = std::panic::catch_unwind(|| {
    Harness::<NanErrFlipLexer<'_>>::over(["abz"]).run_partial();
  })
  .unwrap_err();
  let msg = panicked
    .downcast_ref::<String>()
    .expect("the kit panics with a formatted message");
  assert!(
    msg.contains(
      "partial-equivalence] split k=2, position 0: prefix item diverges from the complete prefix: expected token"
    ),
    "the truncation nonconformance must be reported, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "a divergence the DISCRIMINANT decided consults no caller equality and must not be refused, got: {msg}"
  );
}

#[test]
fn a_divergence_the_span_decided_is_reported_beside_a_payload_that_will_not_compare() {
  // The counterexample's second shape, and the one that says the rule is about the OPERATION
  // rather than about the discriminant alone: both sides are tokens, the payloads are `NaN` on
  // both, and what differs is the SPAN — bounded `Ord`, so the comparison that decided the
  // verdict is sound and there is nothing to refuse. A scan reached the payload first and
  // refused, hiding a real divergence behind a diagnosis it had not established.
  use super::Item;
  let item = |end: usize| Item::<NanPayloadLexer<'_>> {
    item: Ok(FTok(f64::NAN)),
    span: SimpleSpan::new(0, end),
    slice: &"nn"[..end],
    end,
    state: (),
  };
  let panicked = std::panic::catch_unwind(|| {
    super::assert_run_eq::<NanPayloadLexer<'_>>(0, "replay-identity", &[item(1)], &[item(2)]);
  })
  .unwrap_err();
  let msg = panicked
    .downcast_ref::<String>()
    .expect("the kit panics with a formatted message");
  assert!(
    msg.contains("replay-identity] position 0: item mismatch"),
    "the span divergence must be reported, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "the comparison was decided by the span, which is reflexive, got: {msg}"
  );
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] partial-equivalence split k=1, position 0: INCONCLUSIVE — the lexer error payload"
)]
fn a_non_reflexive_error_payload_is_still_refused_when_it_is_what_decided_the_comparison() {
  // The other side of the same coin, and the #295 half of the mandate: two items on the SAME arm
  // whose spans agree, so the error payload is the operation that decided the comparison — and it
  // is the one that will not equal itself. The refusal must still fire, and it must still name
  // the error payload.
  use super::StreamItem;
  let nan = || StreamItem::<NanErrFlipLexer<'_>>::LexerError(SimpleSpan::new(0, 1), FErr(f64::NAN));
  super::assert_partial_prefix_of::<NanErrFlipLexer<'_>>(0, 1, &[nan()], &[nan()]);
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] partial-equivalence split k=18446744073709551615, position 0: INCONCLUSIVE — the lexer error payload"
)]
fn the_error_arm_is_refused_at_the_final_leg_too() {
  // `assert_partial_stream_eq`'s comparison over the error arm, reached directly: the final leg
  // fires before the split loop, so a lexer whose FINAL drain errors with a `NaN` arrives here.
  use super::StreamItem;
  let nan = || StreamItem::<NanErrFlipLexer<'_>>::LexerError(SimpleSpan::new(0, 1), FErr(f64::NAN));
  super::assert_partial_stream_eq::<NanErrFlipLexer<'_>>(0, usize::MAX, &[nan()], &[nan()]);
}

// ── The cache tier's two comparisons, pinned where the harness cannot reach them ────
//
// Both cells call the repaired comparison directly, and they have to: on the cache tier a value
// that will not equal itself is refused at the FIRST entry comparison the sweep makes — check 2's
// accepted `push_front` into a fresh cache — because on a conforming cache every span agrees and
// the non-reflexive component is the only thing that can fail. So no cache defect reachable
// through `CacheHarness::run` is ever compared *beside* such a value, and the difference between
// asking the component that decided and scanning the whole entry has nothing to show there.
// Reachability is an argument about check order; the rule the cells pin is not.

#[test]
fn a_cache_entry_divergence_the_span_decided_is_not_refused_over_a_nan_token() {
  // A permuted cache — the right token at the wrong span — beside a payload that will not compare.
  // The span is bounded `Ord`, so the comparison that produced the verdict is sound and the entry
  // message must stand. A whole-entry scan reached the token first and refused, hiding a real
  // cache defect behind a diagnosis nothing had established.
  use super::cache::{EntryPart, triple_compare};
  let want = (SimpleSpan::new(0, 1), FTok(f64::NAN), ());
  let got = (SimpleSpan::new(1, 2), FTok(f64::NAN), ());
  let (part, decided) =
    triple_compare::<NanPayloadLexer<'_>>((&want.0, &want.1, &want.2), (&got.0, &got.1, &got.2))
      .expect("the two entries differ");
  assert_eq!(part, EntryPart::Span, "the span is what differs");
  assert!(
    decided.expected().is_none() && decided.got().is_none(),
    "the span decided it and a span is reflexive, so there is nothing to refuse"
  );
}

#[test]
fn a_cache_entry_divergence_the_token_decided_is_still_refused() {
  // The #295 half at the same site: the spans agree, so the token payload is the operation that
  // decided the comparison — and it is the one that will not equal itself. Both sides refuse.
  use super::cache::{EntryPart, triple_compare};
  let entry = || (SimpleSpan::new(0, 1), FTok(f64::NAN), ());
  let (want, got) = (entry(), entry());
  let (part, decided) =
    triple_compare::<NanPayloadLexer<'_>>((&want.0, &want.1, &want.2), (&got.0, &got.1, &got.2))
      .expect("a NaN payload does not equal itself");
  assert_eq!(part, EntryPart::Token);
  assert_eq!(decided.expected(), Some("the token payload"));
  assert_eq!(decided.got(), Some("the token payload"));
}

#[test]
fn two_peeked_runs_that_differ_only_in_length_are_refused_nothing() {
  // The purity comparison. A count is not a comparison of caller values, so a run that is longer
  // than the one before it settles the verdict with no `PartialEq` involved — and a scan of one
  // run still found the `NaN` in it and refused, over a `peek` that genuinely is not pure.
  //
  // `Count` is a DECISION and not an absence, which is #324: an answer that said `None` here
  // said the same thing about two runs that agree, so a caller needing both facts had to ask a
  // second question and compose the two by hand. And a count is CONCLUSIVE, so it outranks the
  // `NaN` at position 0 rather than queueing behind it — see `Ranking`.
  use super::{Divergence, cache::runs_decided};
  let entry = || (SimpleSpan::new(0, 1), FTok(f64::NAN), ());
  let first = [entry()];
  let second: [(SimpleSpan, FTok, ()); 0] = [];
  assert!(
    matches!(
      runs_decided::<NanPayloadLexer<'_>>(&first, &second),
      Some(Divergence::Count)
    ),
    "a length divergence consults no caller equality, so there is nothing to refuse — and it is \
     still what decided the verdict"
  );
}

#[test]
fn two_equal_peeked_runs_are_the_only_thing_the_purity_comparison_answers_nothing_about() {
  // The other half of that totality, and the reason `None` can carry a meaning at all now: it
  // means the two runs are EQUAL, which is the one answer the failure path must never be reached
  // with. Two `NaN`-carrying runs are not equal — they disagree at position 0 — so the pair here
  // is reflexive on purpose.
  use super::cache::runs_decided;
  let run = [(SimpleSpan::new(0, 1), FTok(1.0), ())];
  assert!(
    runs_decided::<NanPayloadLexer<'_>>(&run, &run).is_none(),
    "two equal runs differ in nothing, so there is nothing to decide"
  );
}

#[test]
fn a_peeked_run_is_probed_at_the_position_the_two_runs_disagree_at() {
  // And where they do disagree at a position, the question is asked THERE. A scan answered from
  // the first non-reflexive entry anywhere in one run, which need not be — and here is not — the
  // position the two runs actually differ at.
  use super::{Divergence, cache::runs_decided};
  let head = || (SimpleSpan::new(0, 1), FTok(1.0), ());
  let middle = |end: usize| (SimpleSpan::new(1, end), FTok(2.0), ());
  let nan = || (SimpleSpan::new(4, 5), FTok(f64::NAN), ());
  let first = [head(), middle(2), nan()];
  let second = [head(), middle(3), nan()];
  let Some(Divergence::At(i, decided)) = runs_decided::<NanPayloadLexer<'_>>(&first, &second)
  else {
    panic!("position 1 differs in its span, so a position is what decided it")
  };
  assert_eq!(i, 1, "the divergence is at position 1, not at the NaN in 2");
  assert!(
    decided.expected().is_none() && decided.got().is_none(),
    "a span decided position 1, and a span is reflexive"
  );
}

// ── #324: the prefilled purity law composes TWO comparisons, and `||` collapsed them ───
//
// The law compares the prefix `peek` was handed and the run it appended behind that prefix. `||`
// over the two answers a `bool` that does not record which half fired, and the failure path then
// re-ran both guards — so the half that did NOT decide was probed too, and its probe can fire.
//
// These cells call `assert_prefilled_peek_pure` directly, for the reason the two entry-comparison
// cells above do and one more: the fabricated `NaN` the counterexample needs is a value no cache
// the kit can drive is able to produce. The kit's own corpus lexer carries a source slice, which
// is always reflexive, and a `NaN` in the CORPUS is refused at check 2's first entry comparison,
// long before the prefilled sweep runs. Only a cache that manufactures the value in its own
// `peek` reaches this — which is exactly the sequence below, with the manufacture done by hand.

/// The harness the cells drive the prefilled purity law through. It never touches the cache, so
/// the type parameter is only there to name one.
fn purity_harness<'a>() -> crate::conformance::cache::CacheHarness<
  'a,
  NanPayloadLexer<'a>,
  crate::cache::DefaultCache<'a, NanPayloadLexer<'a>>,
> {
  crate::conformance::cache::CacheHarness::new("nn")
}

/// The message a panic carried, or a failure saying the kit did not panic at all.
fn panic_message(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
  let panicked = std::panic::catch_unwind(f).expect_err("the purity law must fail here");
  panicked
    .downcast_ref::<String>()
    .expect("the kit panics with a formatted message")
    .clone()
}

#[test]
fn a_prefilled_impurity_the_prefix_decided_is_not_withheld_over_a_nan_the_appended_run_holds() {
  // THE COUNTEREXAMPLE. The prefix already proves `peek` impure and its divergence is settled by
  // the SPAN, which is reflexive — so that half's probe says nothing. The appended pair is not
  // what decided anything: it differs because the second peek fabricated a `NaN`, and asking it
  // withheld a verdict the kit had established, over a value no verdict rests on.
  let prefix_first = [(SimpleSpan::new(0, 1), FTok(1.0), ())];
  let prefix_second = [(SimpleSpan::new(9, 10), FTok(1.0), ())];
  let appended_first = [(SimpleSpan::new(4, 5), FTok(2.0), ())];
  let appended_second = [(SimpleSpan::new(4, 5), FTok(f64::NAN), ())];
  let msg = panic_message(|| {
    purity_harness().assert_prefilled_peek_pure(
      "prefilled",
      1,
      "a full cache",
      (&prefix_first, &prefix_second),
      (&appended_first, &appended_second),
    );
  });
  assert!(
    msg.contains("pure-peek/prefilled at depth 1] against a full cache: two prefilled peeks on an unchanged cache disagreed in the prefix the buffer already held: [(SimpleSpan { start: 0, end: 1 }, FTok(1.0), ())] then [(SimpleSpan { start: 9, end: 10 }, FTok(1.0), ())]"),
    "the impurity the prefix decided must be reported, and reported with the prefix's own two runs, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "the verdict rests on a span, which is reflexive; the NaN is in the pair that decided nothing, got: {msg}"
  );
  assert!(
    !msg.contains("FTok(NaN)"),
    "the appended pair decided nothing and must not be rendered as though it had, got: {msg}"
  );
}

#[test]
fn a_nan_in_the_appended_pair_that_actually_decided_is_still_refused() {
  // CONTROL 1, the appended half: the same fabricated `NaN`, with the prefix now reproduced
  // exactly. Nothing else decided, so the token payload is what the verdict rests on and the
  // refusal must stand — or the repair has traded a false green for a false red.
  let prefix = [(SimpleSpan::new(0, 1), FTok(1.0), ())];
  let appended_first = [(SimpleSpan::new(4, 5), FTok(2.0), ())];
  let appended_second = [(SimpleSpan::new(4, 5), FTok(f64::NAN), ())];
  let msg = panic_message(|| {
    purity_harness().assert_prefilled_peek_pure(
      "prefilled",
      1,
      "a full cache",
      (&prefix, &prefix),
      (&appended_first, &appended_second),
    );
  });
  assert!(
    msg.contains(
      "non-reflexive-payload] pure-peek/prefilled at depth 1 against a full cache, position 0: INCONCLUSIVE — the token payload of the second appended entry (SimpleSpan { start: 4, end: 5 }, FTok(NaN), ())"
    ),
    "a NaN that decided the appended comparison must still be refused, named as the appended side and rendered as the appended ENTRY — the value the verdict rests on, got: {msg}"
  );
}

#[test]
fn a_nan_in_the_prefix_pair_that_actually_decided_is_still_refused() {
  // CONTROL 1, the prefix half — the same rule on the other side of the decision, which is what
  // pins the two halves to their own labels. The spans agree here, so the token payload is what
  // decided the prefix comparison.
  let prefix_first = [(SimpleSpan::new(0, 1), FTok(1.0), ())];
  let prefix_second = [(SimpleSpan::new(0, 1), FTok(f64::NAN), ())];
  let appended = [(SimpleSpan::new(4, 5), FTok(2.0), ())];
  let msg = panic_message(|| {
    purity_harness().assert_prefilled_peek_pure(
      "prefilled",
      1,
      "a full cache",
      (&prefix_first, &prefix_second),
      (&appended, &appended),
    );
  });
  assert!(
    msg.contains(
      "non-reflexive-payload] pure-peek/prefilled at depth 1 against a full cache, position 0: INCONCLUSIVE — the token payload of the second prefix entry (SimpleSpan { start: 0, end: 1 }, FTok(NaN), ())"
    ),
    "a NaN that decided the prefix comparison must still be refused, and rendered as the prefix ENTRY, got: {msg}"
  );
}

#[test]
fn a_prefix_impurity_with_reflexive_payloads_throughout_still_reds_the_ordinary_way() {
  // CONTROL 2: nothing here is non-reflexive, and the cache is genuinely impure on the prefix.
  // The ordinary `pure-peek` failure is the whole point of the law and must survive the repair.
  let prefix_first = [(SimpleSpan::new(0, 1), FTok(1.0), ())];
  let prefix_second = [(SimpleSpan::new(9, 10), FTok(1.0), ())];
  let appended = [(SimpleSpan::new(4, 5), FTok(2.0), ())];
  let msg = panic_message(|| {
    purity_harness().assert_prefilled_peek_pure(
      "prefilled",
      1,
      "a full cache",
      (&prefix_first, &prefix_second),
      (&appended, &appended),
    );
  });
  assert!(
    msg.contains("pure-peek/prefilled at depth 1] against a full cache: two prefilled peeks on an unchanged cache disagreed in the prefix the buffer already held: [(SimpleSpan { start: 0, end: 1 }, FTok(1.0), ())] then [(SimpleSpan { start: 9, end: 10 }, FTok(1.0), ())]"),
    "the ordinary impurity failure must keep its tag, its wording and the deciding half's two runs, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "there is no non-reflexive value anywhere in this pair, got: {msg}"
  );
}

#[test]
fn an_appended_decided_impurity_is_reported_as_appended_and_not_as_the_prefix() {
  // CONTROL 3, and it pulls against the other two: a repair that always blamed the prefix would
  // pass the counterexample and control 2 while replacing one misattribution with another. The
  // prefix is reproduced exactly here and the appended run is not, so the appended half is what
  // decided — and the failure has to say so.
  let prefix = [(SimpleSpan::new(0, 1), FTok(1.0), ())];
  let appended_first = [(SimpleSpan::new(4, 5), FTok(2.0), ())];
  let appended_second = [(SimpleSpan::new(6, 7), FTok(2.0), ())];
  let msg = panic_message(|| {
    purity_harness().assert_prefilled_peek_pure(
      "prefilled",
      1,
      "a full cache",
      (&prefix, &prefix),
      (&appended_first, &appended_second),
    );
  });
  assert!(
    msg.contains("disagreed in the run it appended behind that prefix: [(SimpleSpan { start: 4, end: 5 }, FTok(2.0), ())] then [(SimpleSpan { start: 6, end: 7 }, FTok(2.0), ())]"),
    "an appended-decided impurity must be reported as appended, with the appended run's own two values, got: {msg}"
  );
  assert!(
    !msg.contains("the prefix the buffer already held")
      && !msg.contains("SimpleSpan { start: 0, end: 1 }"),
    "the prefix was reproduced exactly and must be neither blamed nor printed, got: {msg}"
  );
}

#[test]
fn two_prefilled_peeks_that_agree_in_both_halves_are_not_a_failure() {
  // The positive control, and the one that keeps the law from being a check that cannot pass:
  // both halves reproduced, `NaN` nowhere, no panic.
  let prefix = [(SimpleSpan::new(0, 1), FTok(1.0), ())];
  let appended = [(SimpleSpan::new(4, 5), FTok(2.0), ())];
  purity_harness().assert_prefilled_peek_pure(
    "prefilled",
    1,
    "a full cache",
    (&prefix, &prefix),
    (&appended, &appended),
  );
}

// ── #324 round 5: a conclusive difference outranks an inconclusive one ──────────────
//
// Rounds 1–4 built the rule that the kit never reports a diagnosis it did not establish. These
// cells are its mirror: it never withholds one it did. Every one of them is differential — at the
// commit before this round each produced the wrong answer quoted in its own comment.

/// A one-character-per-token lexer over the `NaN`-capable payload whose `L::State` is a
/// **comparable** counter.
///
/// [`FloatLexer`]'s state is `()`, and a `()` state cannot differ — so the third of
/// `triple_compare`'s three questions had no fixture that could make it fire while the token
/// question was already failing, which is exactly the arrangement round 5's second finding lives
/// in. This is that fixture and it is conforming in everything else.
struct StateLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: Emitted,
}

impl<'a> Lexer<'a> for StateLexer<'a> {
  type State = Emitted;
  type Source = str;
  type Token = FTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: Emitted(0),
    }
  }
  fn with_state(src: &'a str, state: Emitted) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &Emitted {
    &self.state
  }
  fn state_mut(&mut self) -> &mut Emitted {
    &mut self.state
  }
  fn into_state(self) -> Emitted {
    self.state
  }
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<FTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    self.state.0 += 1;
    Some(Ok(FTok(float_of(self.src.as_bytes()[self.start]))))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

/// One captured item of the `NaN`-capable float lexer, for the stream comparisons that take
/// `Item`s directly. Every component but the payload is the caller's to choose, so a cell can
/// hold spans, slices and kinds constant and move exactly one thing.
fn float_item(
  value: f64,
  start: usize,
  end: usize,
) -> super::Item<'static, NanPayloadLexer<'static>> {
  super::Item {
    item: Ok(FTok(value)),
    span: SimpleSpan::new(start, end),
    slice: "x",
    end,
    state: (),
  }
}

#[test]
fn an_extra_item_outranks_a_non_reflexive_payload_at_position_zero() {
  // THE FINDING. `[Tok(NaN)]` against `[Tok(NaN), Tok(0.0)]`: identical kind, span and slice at
  // position 0, so only the payload can decide there — and it cannot decide anything, because
  // `NaN != NaN`. The extra item proves replay divergence with NO caller equality consulted at
  // all, and it sat behind the refusal.
  //
  // Before: "[input #0 replay-identity] position 0: INCONCLUSIVE — the token payload …".
  let msg = panic_message(|| {
    super::assert_run_eq::<NanPayloadLexer<'_>>(
      0,
      "replay-identity",
      &[float_item(f64::NAN, 0, 1)],
      &[float_item(f64::NAN, 0, 1), float_item(0.0, 1, 2)],
    );
  });
  assert!(
    msg.contains("[input #0 replay-identity] length mismatch: expected 1 items, got 2"),
    "an item COUNT consults no caller equality, so it convicts the lexer on its own, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "the kit established replay divergence and may not withhold it, got: {msg}"
  );
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] replay-identity position 0: INCONCLUSIVE — the token payload"
)]
fn a_non_reflexive_payload_with_nothing_conclusive_anywhere_is_still_refused() {
  // CONTROL 1, and it pulls the opposite way from the finding: two streams of the same length,
  // identical in every component a total equality settles, differing only in a payload that will
  // not equal itself. Nothing conclusive exists ANYWHERE, so the retained fallback is the answer
  // and #295's repair survives.
  super::assert_run_eq::<NanPayloadLexer<'_>>(
    0,
    "replay-identity",
    &[float_item(f64::NAN, 0, 1)],
    &[float_item(f64::NAN, 0, 1)],
  );
}

#[test]
fn an_ordinary_divergence_over_reflexive_payloads_keeps_its_component_and_its_wording() {
  // CONTROL 2. The ordinary case is a caller `PartialEq` over two values that are BOTH self-equal
  // — the whole population of honest failures — and ranking must not demote it to a fallback. It
  // is conclusive, it is reported at the first position it occurs, and the words are the ones the
  // kit has always used.
  let msg = panic_message(|| {
    super::assert_run_eq::<NanPayloadLexer<'_>>(
      0,
      "replay-identity",
      &[float_item(1.0, 0, 1)],
      &[float_item(2.0, 0, 1)],
    );
  });
  assert!(
    msg.contains("[input #0 replay-identity] position 0: item mismatch: expected"),
    "a reflexive payload divergence keeps the ordinary tag and wording, got: {msg}"
  );
  assert!(
    msg.contains("FTok(1.0)") && msg.contains("FTok(2.0)"),
    "and it names two distinguishable values, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "both sides equal themselves, so there is nothing to refuse, got: {msg}"
  );
}

#[test]
fn a_conclusive_difference_later_in_the_stream_is_found_and_not_only_one_at_the_same_position() {
  // CONTROL 3, and it pulls a third way: a repair that only looked at the REST OF THE SAME ITEM
  // would pass the finding and control 1 and still stop at the first position that differs. The
  // `NaN` is at position 0; the span divergence — settled by `Lexer::Span`, bounded `Ord` — is at
  // position 1, and it is what the kit has established.
  //
  // Before: "[input #0 replay-identity] position 0: INCONCLUSIVE — the token payload …".
  let msg = panic_message(|| {
    super::assert_run_eq::<NanPayloadLexer<'_>>(
      0,
      "replay-identity",
      &[float_item(f64::NAN, 0, 1), float_item(1.0, 1, 2)],
      &[float_item(f64::NAN, 0, 1), float_item(1.0, 1, 3)],
    );
  });
  assert!(
    msg.contains("[input #0 replay-identity] position 1: item mismatch: expected"),
    "the conclusive difference is at position 1 and the search must reach it, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "position 1 is settled by a span, so there is nothing to refuse, got: {msg}"
  );
}

#[test]
fn a_reflexive_state_corruption_outranks_a_non_reflexive_token() {
  // THE SECOND FINDING, at the shared function it lives in. Same span, a `NaN` token on both
  // sides, and two DIFFERENT `L::State`s that each equal themselves. `triple_compare` returned at
  // the token and never examined the state, so the guard called the whole thing inconclusive over
  // a cache the unexamined state convicts on its own.
  //
  // Before: `Some((EntryPart::Token, …))`, both sides non-reflexive, hence a refusal.
  use super::cache::{EntryPart, triple_compare};
  let span = SimpleSpan::new(0, 1);
  let nan = FTok(f64::NAN);
  let Some((part, decided)) =
    triple_compare::<StateLexer<'_>>((&span, &nan, &Emitted(7)), (&span, &nan, &Emitted(9)))
  else {
    panic!("the state differs, so the triple differs")
  };
  assert_eq!(
    part,
    EntryPart::State,
    "the state is what the kit established and what it must report"
  );
  assert!(
    decided.expected().is_none() && decided.got().is_none(),
    "both states equal themselves, so there is nothing to refuse"
  );
}

#[test]
fn a_span_still_outranks_a_state_when_the_span_comparison_is_sound() {
  // The control for "span first, always". Ranking may only move an answer PAST a step the kit
  // could not draw a verdict from; a sound span still speaks before everything behind it.
  use super::cache::{EntryPart, triple_compare};
  let token = FTok(1.0);
  let Some((part, _)) = triple_compare::<StateLexer<'_>>(
    (&SimpleSpan::new(0, 1), &token, &Emitted(7)),
    (&SimpleSpan::new(0, 2), &token, &Emitted(9)),
  ) else {
    panic!("the span and the state both differ")
  };
  assert_eq!(part, EntryPart::Span, "span first, always");
}

#[test]
fn a_purity_run_length_outranks_a_non_reflexive_entry_at_a_shared_position() {
  // The same ordering, one level up, on the comparison the cache's purity laws read. A `NaN` at
  // position 0 and a second run one entry longer: the length is the kit's own `len()`, so no
  // caller equality stands behind it.
  //
  // Before: `At(0, …)` over the payload, and a refusal.
  use super::{Divergence, cache::runs_decided};
  let nan = || (SimpleSpan::new(0, 1), FTok(f64::NAN), ());
  let first = [nan()];
  let second = [nan(), (SimpleSpan::new(1, 2), FTok(1.0), ())];
  assert!(
    matches!(
      runs_decided::<NanPayloadLexer<'_>>(&first, &second),
      Some(Divergence::Count)
    ),
    "a count outranks a payload that will not equal itself"
  );
}

// ── #324 round 5 residue: `Eq` is a marker, and nothing checks its promise ──────────
//
// Round 4 classified a second population of verdict-drawing comparisons as settled by TOTAL
// equalities and therefore outside the guarded population — span/slice coherence, read-frontier
// purity, the span after exhaustion, gap-free tiling, and the cache's span-valued laws. The
// classification is what these cells attack. `Eq` and `Ord` promise reflexivity and nothing
// verifies the promise, so the same lying impl that gets a `non-reflexive-payload` refusal at an
// item comparison got a red naming the caller's lexer three hundred lines away. One answer, not
// two.

/// A source slice whose `PartialEq` answers `false` to everything, itself included, while
/// claiming [`Eq`].
///
/// This is the brief's own falsifier and it is a legal, compiling implementation of the
/// [`Slice`](crate::Slice) bound: nothing in the trait system, and nothing in this kit before
/// now, asks whether the promise `Eq` makes is kept.
#[derive(Clone, Copy, Debug)]
struct LyingSlice<'a>(&'a str);

impl PartialEq for LyingSlice<'_> {
  fn eq(&self, _: &Self) -> bool {
    false
  }
}

impl Eq for LyingSlice<'_> {}

impl<'source> crate::Slice<'source> for LyingSlice<'source> {
  type Char = char;
  type Iter<'a>
    = core::str::Chars<'a>
  where
    Self: 'a;
  type PositionedIter<'a>
    = core::str::CharIndices<'a>
  where
    Self: 'a;

  fn iter<'a>(&'a self) -> Self::Iter<'a>
  where
    Self: 'a,
  {
    self.0.chars()
  }
  fn positioned_iter<'a>(&'a self) -> Self::PositionedIter<'a>
  where
    Self: 'a,
  {
    self.0.char_indices()
  }
  fn len(&self) -> usize {
    self.0.len()
  }
}

/// A `str`-backed source whose slices are [`LyingSlice`]s. Everything else about it is `str`'s
/// own behaviour.
#[derive(Debug)]
struct LyingSliceSource(&'static str);

impl crate::Source<usize> for LyingSliceSource {
  type Slice<'source>
    = LyingSlice<'source>
  where
    Self: 'source;

  fn is_empty(&self) -> bool {
    self.0.is_empty()
  }
  fn len(&self) -> usize {
    self.0.len()
  }
  fn as_slice(&self) -> Self::Slice<'_> {
    LyingSlice(self.0)
  }
  fn slice<R>(&self, range: R) -> Option<Self::Slice<'_>>
  where
    R: core::ops::RangeBounds<usize>,
  {
    self
      .0
      .get((
        range.start_bound().map(|s| *s),
        range.end_bound().map(|s| *s),
      ))
      .map(LyingSlice)
  }
  fn is_boundary(&self, index: usize) -> bool {
    self.0.is_char_boundary(index)
  }
}

/// A per-character lexer over [`LyingSliceSource`], conforming in everything the contract names.
/// The only thing wrong anywhere near it is the caller's own slice equality.
struct LyingSliceLexer<'a> {
  src: &'a LyingSliceSource,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for LyingSliceLexer<'a> {
  type State = ();
  type Source = LyingSliceSource;
  type Token = PTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a LyingSliceSource) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a LyingSliceSource, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a LyingSliceSource {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> LyingSlice<'a> {
    LyingSlice(&self.src.0[self.start..self.end])
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.0.len() {
      return None;
    }
    self.end = boundary_after(self.src.0, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
fn a_slice_that_will_not_equal_itself_is_refused_and_not_blamed_on_the_lexer() {
  // THE FALSIFIER round 4's exemption had to survive, and it does not. Before this the kit said
  // "[input #0 span/slice-coherence] position 0: slice() disagrees with the source at span 0..1:
  // source LyingSlice("a"), slice() LyingSlice("a")" — a red naming the caller's LEXER, with two
  // values that render identically, which is #295 verbatim at a site round 4 called exempt.
  let src = LyingSliceSource("ab");
  let msg = panic_message(|| {
    let _ = super::lex_run::<LyingSliceLexer<'_>>(0, &src, 8 * 2 + 64);
  });
  assert!(
    msg
      .contains("non-reflexive-payload] span/slice-coherence position 0: INCONCLUSIVE — the slice"),
    "a broken TOTAL equality gets the same answer as a broken partial one, got: {msg}"
  );
}

#[test]
fn an_honest_slice_that_genuinely_disagrees_still_reds_span_slice_coherence() {
  // The control, and the one that keeps the guard from trading a false red for a false green: a
  // lexer whose `slice()` really does disagree with its own span, over a slice type that equals
  // itself. `BadSliceLexer` is the kit's existing fixture for exactly that, and its cell
  // (`incoherent_slice_is_caught`) still reds the ordinary way — asserted here as well so the two
  // sit next to each other.
  let msg = panic_message(|| {
    Harness::<BadSliceLexer<'_>>::new("abc").run();
  });
  assert!(
    msg.contains("span/slice-coherence") && !msg.contains("non-reflexive-payload"),
    "a slice that equals itself and disagrees is a lexer defect and must be reported as one, got: {msg}"
  );
}

/// An offset whose `PartialEq` answers `false` to everything, itself included, while claiming
/// [`Eq`] through [`Ord`] — and whose **ordering** is honest.
///
/// The two halves are separate impls, which is the whole point: `cmp` and `eq` are independent
/// caller code, so an `Ord` can be a total order for every purpose that compares two *different*
/// values and still break the one law `Eq` adds on top of it. Every `<`-shaped check the kit runs
/// therefore behaves exactly as it does over `usize`, and only the equalities move — which is what
/// makes this a differential fixture rather than a lexer that fails everything.
///
/// [`Lexer::Span`](crate::Lexer::Span) is `SimpleSpan<LyingOffset>` here, whose derived
/// `PartialEq` is elementwise, so the span lies too. One fixture, both components.
#[derive(Clone, Copy, Debug, Default)]
struct LyingOffset(usize);

/// Hand-written for [`LyingKind`]'s reason: `Lexer::Offset` requires `Hash`, and deriving it
/// beside a manual `PartialEq` is what `clippy::derived_hash_with_manual_eq` exists to stop. The
/// lint's law — `a == b` implies `hash(a) == hash(b)` — holds *vacuously*, since nothing is equal
/// to anything, so this is not a second lie on top of the one the fixture is for.
impl core::hash::Hash for LyingOffset {
  fn hash<H: core::hash::Hasher>(&self, _: &mut H) {}
}

impl PartialEq for LyingOffset {
  fn eq(&self, _: &Self) -> bool {
    false
  }
}

impl Eq for LyingOffset {}

impl PartialOrd for LyingOffset {
  fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for LyingOffset {
  fn cmp(&self, other: &Self) -> core::cmp::Ordering {
    self.0.cmp(&other.0)
  }
}

impl core::ops::AddAssign<&LyingOffset> for LyingOffset {
  fn add_assign(&mut self, rhs: &Self) {
    self.0 += rhs.0;
  }
}

/// A `str`-backed source addressed by [`LyingOffset`]. Everything else about it is `str`'s own
/// behaviour.
#[derive(Debug)]
struct LyingOffsetSource(&'static str);

impl crate::Source<LyingOffset> for LyingOffsetSource {
  type Slice<'source>
    = &'source str
  where
    Self: 'source;

  fn is_empty(&self) -> bool {
    self.0.is_empty()
  }
  fn len(&self) -> LyingOffset {
    LyingOffset(self.0.len())
  }
  fn as_slice(&self) -> &str {
    self.0
  }
  fn slice<R>(&self, range: R) -> Option<&str>
  where
    R: core::ops::RangeBounds<LyingOffset>,
  {
    use core::ops::Bound;
    let start = match range.start_bound() {
      Bound::Included(o) => o.0,
      Bound::Excluded(o) => o.0 + 1,
      Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
      Bound::Included(o) => o.0 + 1,
      Bound::Excluded(o) => o.0,
      Bound::Unbounded => self.0.len(),
    };
    self.0.get(start..end)
  }
  fn is_boundary(&self, index: LyingOffset) -> bool {
    self.0.is_char_boundary(index.0)
  }
}

/// A per-character lexer over [`LyingOffsetSource`], conforming in everything the contract names.
///
/// It reports its frontier as [`ReadTo`](crate::ReadFrontier::ReadTo) rather than
/// [`SpanEnd`](crate::ReadFrontier::SpanEnd) because `ReadFrontier`'s equality is derived from the
/// offset's: a unit variant compares equal whatever the offset does, so `SpanEnd` would hide the
/// very thing this fixture exists to show.
struct LyingOffsetLexer<'a> {
  src: &'a LyingOffsetSource,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for LyingOffsetLexer<'a> {
  type State = ();
  type Source = LyingOffsetSource;
  type Token = PTok;
  type Span = SimpleSpan<LyingOffset>;
  type Offset = LyingOffset;

  fn new(src: &'a LyingOffsetSource) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a LyingOffsetSource, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), Infallible> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a LyingOffsetSource {
    self.src
  }
  fn span(&self) -> SimpleSpan<LyingOffset> {
    SimpleSpan::new(LyingOffset(self.start), LyingOffset(self.end))
  }
  fn slice(&self) -> &'a str {
    &self.src.0[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<PTok, Infallible>> {
    self.start = self.end;
    if self.start >= self.src.0.len() {
      return None;
    }
    self.end = boundary_after(self.src.0, self.start);
    Some(Ok(PTok))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<LyingOffset> {
    crate::ReadFrontier::ReadTo(LyingOffset(self.end))
  }
  fn bump(&mut self, n: &LyingOffset) {
    self.end += n.0;
  }
}

/// The items a conforming [`LyingOffsetLexer`] run over `src` produces, captured WITHOUT the
/// guarded checks that would refuse first.
///
/// `lex_run` refuses at read-frontier purity for this fixture, which is the first equality it
/// reaches — so the checks that run later have to be reached directly, exactly as
/// `a_nan_payload_is_diagnosed_at_the_resume_comparison` reaches `check_resume`. Reachability is
/// an argument about check ORDER; a guard behind another guard is still a guard.
fn lying_offset_items(src: &LyingOffsetSource) -> Vec<super::Item<'_, LyingOffsetLexer<'_>>> {
  let mut lexer = <LyingOffsetLexer<'_> as Lexer<'_>>::new(src);
  let mut out = Vec::new();
  while let Some(item) = lexer.lex() {
    let span = lexer.span();
    let end = *span.end_ref();
    out.push(super::Item {
      item,
      span,
      slice: lexer.slice(),
      end,
      state: (),
    });
  }
  out
}

#[test]
fn a_read_frontier_that_will_not_equal_itself_is_refused_and_not_blamed_on_the_lexer() {
  // Before: "[input #0 read-frontier] position 0: read_frontier() changed between calls: first
  // read ReadTo(LyingOffset(1)), probe #0 read ReadTo(LyingOffset(1))" — a purity accusation over
  // two readings that are the same reading, rendered identically.
  let src = LyingOffsetSource("ab");
  let msg = panic_message(|| {
    let _ = super::lex_run::<LyingOffsetLexer<'_>>(0, &src, 8 * 2 + 64);
  });
  assert!(
    msg.contains(
      "non-reflexive-payload] read-frontier position 0: INCONCLUSIVE — the read frontier"
    ),
    "the frontier's equality is the offset's, and a broken one is not a purity defect, got: {msg}"
  );
}

#[test]
fn a_span_that_will_not_equal_itself_is_refused_after_exhaustion_too() {
  // Before: "[input #0 span-survives-exhaustion] span() changed after exhaustion: first read
  // 2..2, probe #0 read 2..2". The clause this enforces is read by a refill driver, so the red it
  // used to produce sent a caller looking for a lexer bug that is not there.
  let src = LyingOffsetSource("ab");
  let msg = panic_message(|| {
    super::check_span_after_exhaustion::<LyingOffsetLexer<'_>>(0, &src, 8 * 2 + 64);
  });
  assert!(
    msg.contains(
      "non-reflexive-payload] span-survives-exhaustion after exhaustion: INCONCLUSIVE — the span"
    ),
    "the span after exhaustion is compared for stability, and the comparison is the caller's, got: {msg}"
  );
}

#[test]
fn an_offset_that_will_not_equal_itself_is_refused_by_the_tiling_law() {
  // Before: "[input #0 lossless] position 0: first span 0..1 does not start at 0" — a tiling
  // accusation over an offset that IS zero. Gap-free tiling is four `Offset` equalities and every
  // one of them was unguarded; this is the one a run reaches first.
  let src = LyingOffsetSource("ab");
  let items = lying_offset_items(&src);
  let msg = panic_message(|| {
    super::check_lossless::<LyingOffsetLexer<'_>>(0, &src, &items);
  });
  assert!(
    msg.contains("non-reflexive-payload] lossless position 0: INCONCLUSIVE — the span offset"),
    "a tiling verdict is drawn from `L::Offset` equality and inherits the same refusal, got: {msg}"
  );
}

#[test]
fn an_honest_lexer_over_the_same_lying_offset_type_is_not_what_any_of_this_rejects() {
  // The positive control for the whole residue repair: the ORDERING half of `LyingOffset` is
  // honest, so every `<`-shaped check passes, and the refusals above are reached only because an
  // equality was asked. Nothing here may pass silently that a `usize`-offset lexer would fail, so
  // the same source through the same checks must still reach a refusal rather than a green.
  let src = LyingOffsetSource("ab");
  let items = lying_offset_items(&src);
  assert_eq!(items.len(), 2, "the fixture lexes one token per character");
  assert!(
    items[0].span.start_ref().cmp(items[0].span.end_ref()) == core::cmp::Ordering::Less,
    "the ordering half is honest, which is what keeps the fixture differential"
  );
}

#[test]
fn the_cache_kits_span_valued_laws_are_shadowed_by_its_entry_comparison_today() {
  // **This cell is NOT differential for the residue, and says so on purpose.** The cache kit's
  // eleven span-valued comparisons are guarded now, and with a wholly-lying `L::Span` not one of
  // them is reachable: `run()` reaches an ENTRY comparison first, at `accepted-push`, and that
  // one has been guarded since round 2. Removing `guard_entry` and re-running this fixture lands
  // on `accepted-push`'s own verdict — never on a span-valued law — which is how the shadowing
  // was established rather than assumed.
  //
  // So what this pins is the ORDER, not the repair: it is the cell that changes if a future
  // reordering makes a span-valued law the first comparison a lying span reaches. The repair at
  // those eleven sites is defensive for exactly that reason — reachability is an argument about
  // check order and not a bound, which is the same argument
  // `a_nan_payload_is_diagnosed_at_the_resume_comparison` rests on — and it costs a call on a
  // path already panicking.
  let src = LyingOffsetSource("abcdefghijklmnop");
  let msg = panic_message(|| {
    crate::conformance::cache::CacheHarness::<
      LyingOffsetLexer<'_>,
      crate::cache::DefaultCache<'_, LyingOffsetLexer<'_>>,
    >::new(&src)
    .run();
  });
  assert!(
    msg.contains("non-reflexive-payload] accepted-push")
      && msg.contains("INCONCLUSIVE — the span of the got entry"),
    "a lying `L::Span` must reach the ENTRY comparison first and be refused there, got: {msg}"
  );
}

/// A `NaN`-capable token payload whose **error** type is constructible.
///
/// [`FTok`]'s error is `Infallible`, so no fixture could put a non-reflexive value on the
/// committed arm and a real divergence on the raised one at the same time — which is exactly the
/// arrangement the integration tier's two-arm ordering fails in. This is the pair of arms in one
/// token type.
#[derive(Clone, Debug, PartialEq)]
struct MTok(f64);

impl Token<'_> for MTok {
  type Kind = FKind;
  type Error = FErr;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> FKind {
    FKind
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

/// A per-character lexer over [`MTok`], conforming in everything the contract names. It exists to
/// name the type parameter the two-arm comparison is generic over; the cells below build the
/// observations by hand, exactly as the partial tier's own refusal cells do.
struct MixedArmLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: (),
}

impl<'a> Lexer<'a> for MixedArmLexer<'a> {
  type State = ();
  type Source = str;
  type Token = MTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state: (),
    }
  }
  fn with_state(src: &'a str, state: ()) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }
  fn check(&self) -> Result<(), FErr> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {}
  fn source(&self) -> &'a str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }
  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }
  fn lex(&mut self) -> Option<Result<MTok, FErr>> {
    self.start = self.end;
    if self.start >= self.src.len() {
      return None;
    }
    self.end = boundary_after(self.src, self.start);
    Some(Ok(MTok(float_of(self.src.as_bytes()[self.start]))))
  }
  fn read_frontier(&self) -> crate::ReadFrontier<usize> {
    crate::ReadFrontier::SpanEnd
  }
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
fn a_raised_error_divergence_is_not_withheld_over_a_nan_the_committed_arm_holds() {
  // The integration tier's two arms are one verdict, and they were two `assert` calls in a fixed
  // order — so a committed-token divergence a `NaN` decided withheld a raised-error divergence a
  // SPAN had settled. That is round 5's finding one level above the position/count ordering.
  //
  // Before: "[input #0 non-reflexive-payload] integration/peek-heavy position 0: INCONCLUSIVE —
  // the token payload …", over an error arm that convicts the layer on its own.
  use super::{Observed, StreamItem};
  let nan_token = || StreamItem::<MixedArmLexer<'_>>::Token(MTok(f64::NAN), SimpleSpan::new(0, 1));
  let error_at =
    |end: usize| StreamItem::<MixedArmLexer<'_>>::LexerError(SimpleSpan::new(1, end), FErr(1.0));
  let expected = Observed {
    tokens: vec![nan_token()],
    errors: vec![error_at(2)],
  };
  let got = Observed {
    tokens: vec![nan_token()],
    errors: vec![error_at(3)],
  };
  let msg = panic_message(|| {
    super::assert_observed_eq::<MixedArmLexer<'_>>(0, "peek-heavy", &expected, &got);
  });
  assert!(
    msg.contains(
      "[input #0 integration/peek-heavy] error position 0: raised lexer-error stream diverges"
    ),
    "the error arm's span divergence is conclusive and must be what speaks, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "a span settled the error arm, so there is nothing to refuse, got: {msg}"
  );
}

#[test]
#[should_panic(
  expected = "non-reflexive-payload] integration/peek-heavy position 0: INCONCLUSIVE — the token payload"
)]
fn a_committed_arm_nan_with_a_reproduced_error_arm_is_still_refused() {
  // The control for it: the same committed arm, and an error arm that agrees. Nothing conclusive
  // exists anywhere, so the retained fallback is the answer and the arm ordering is unchanged.
  use super::{Observed, StreamItem};
  let nan_token = || StreamItem::<MixedArmLexer<'_>>::Token(MTok(f64::NAN), SimpleSpan::new(0, 1));
  let error = || StreamItem::<MixedArmLexer<'_>>::LexerError(SimpleSpan::new(1, 2), FErr(1.0));
  let expected = Observed {
    tokens: vec![nan_token()],
    errors: vec![error()],
  };
  let got = Observed {
    tokens: vec![nan_token()],
    errors: vec![error()],
  };
  super::assert_observed_eq::<MixedArmLexer<'_>>(0, "peek-heavy", &expected, &got);
}

#[test]
fn a_prefilled_impurity_the_appended_run_decided_is_not_withheld_over_a_nan_in_the_prefix() {
  // THE COUNTEREXAMPLE'S MIRROR, and the third level the same finding lives at. The existing cell
  // above runs the other way — a conclusive prefix and a `NaN` in the appended run — and passed
  // because `find_map` takes the prefix first. This one is the direction `find_map` got wrong:
  // the prefix's divergence is a `NaN` that decides nothing, and the appended run's is a span.
  //
  // Before: "[cache non-reflexive-payload] pure-peek/prefilled at depth 1 … INCONCLUSIVE — the
  // token payload of the first prefix entry …".
  let prefix_first = [(SimpleSpan::new(0, 1), FTok(f64::NAN), ())];
  let prefix_second = [(SimpleSpan::new(0, 1), FTok(f64::NAN), ())];
  let appended_first = [(SimpleSpan::new(4, 5), FTok(2.0), ())];
  let appended_second = [(SimpleSpan::new(6, 7), FTok(2.0), ())];
  let msg = panic_message(|| {
    purity_harness().assert_prefilled_peek_pure(
      "prefilled",
      1,
      "a full cache",
      (&prefix_first, &prefix_second),
      (&appended_first, &appended_second),
    );
  });
  assert!(
    msg.contains("disagreed in the run it appended behind that prefix"),
    "the appended half is what the kit established, got: {msg}"
  );
  assert!(
    !msg.contains("non-reflexive-payload"),
    "a span settled the appended half, so there is nothing to refuse, got: {msg}"
  );
}
