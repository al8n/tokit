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

#[test]
#[should_panic(expected = "lex-budget")]
fn an_endless_same_span_error_lexer_is_refused_not_hung() {
  // Before the budget moved underneath `next()`, this call did not fail — it HUNG. The first
  // `complete_stream` drain enters `next()`, the scanner accepts an error, skips it, lexes the
  // identical error again, and never returns; the log holds the one report the dedup let through,
  // so the per-`next()` budget it was checked against never grows.
  Harness::<EndlessErrLexer<'_>>::over(["abc"]).run_partial();
}

// ── Positive: the crate's real logos adapter (LogosLexer) ───────────────────────────

#[cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"))]
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
    // other, so the DFA never probes into a longer candidate and backtracks. `SpanEnd` is
    // honest, and it is what keeps the partial cell below checking real chunked equivalence:
    // at the conservative default this fixture would withhold everything and the check would
    // pass vacuously.
    const READ_FRONTIER_CLASS: crate::ReadFrontierClass = crate::ReadFrontierClass::SpanEnd;

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
#[cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"))]
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
    // No class declared: the vocabulary takes `Token`'s own default. Written as a separate arm
    // rather than as `$class = <Self as Token>::READ_FRONTIER_CLASS`, which is a const cycle —
    // and the point of this arm is to exercise the DEFAULT, not to restate it.
    ($name:ident) => {
      num_vocabulary!(@body $name,);
    };
    ($name:ident, $class:expr) => {
      num_vocabulary!(@body $name, const READ_FRONTIER_CLASS: crate::ReadFrontierClass = $class;);
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

  // The claim a vocabulary like this must NOT make, and the claim it gets by default.
  num_vocabulary!(LyingNum, crate::ReadFrontierClass::SpanEnd);
  num_vocabulary!(DefaultNum);

  type LyingLexer<'a> = LogosLexer<'a, LyingNum>;
  type DefaultLexer<'a> = LogosLexer<'a, DefaultNum>;

  #[test]
  #[should_panic(expected = "partial-equivalence")]
  fn a_span_end_claim_over_a_float_vocabulary_is_falsified() {
    // `"1.5"` at split k=2: the prefix `"1."` commits `Int@0..1` under a `SpanEnd` claim, and
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
// `State::probe` puts on the recorder, and the two cells below are the check on it.
#[cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"))]
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

    fn probe(&self) -> Option<Probe> {
      self.probe
    }

    fn clear_probe(&mut self) {
      self.probe = None;
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
    // The class default is `Unbounded` and would have withheld everything, so this cell fails
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
