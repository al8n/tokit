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

#[derive(Clone, Debug)]
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
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "sticky-exhaustion")]
fn non_sticky_exhaustion_is_caught() {
  Harness::<NonStickyLexer<'_>>::new("abc").run();
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

#[derive(Clone, Debug)]
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
  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

#[test]
#[should_panic(expected = "partial-equivalence")]
fn truncation_unfaithful_lexer_fails_partial_equivalence() {
  // On "abz" the complete parse marks every token; on the prefix "ab" the same positions come back
  // plain — an interior token changed under truncation, which chunked equivalence catches.
  Harness::<LastBytePeekLexer<'_>>::new("abz").run_partial();
}

// ── Positive: the crate's real logos adapter (LogosLexer) ───────────────────────────

#[cfg(feature = "logos")]
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
