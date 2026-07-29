//! Partial-input (Sans-I/O) frontier-rule tests.
//!
//! Each of the three conservative rules at the scan chokepoint gets a focused case: frontier
//! holdback (a token touching the buffer end), frontier error (a lexer error touching the buffer
//! end), and non-final EOF. Plus the two boundary properties: `is_final == true` behaves exactly
//! like a complete parse, and a *mid-buffer* token or error (strictly before the buffer end) is
//! yielded / emitted normally even while partial.
//!
//! The last section covers the **terminal-dominance law**: a limit trip whose tripping token ends
//! exactly at a non-final buffer end is *not* a frontier item to be withheld — no refill can
//! un-trip a limiter, so the trip fires (diagnostic emitted, poison boundary latched) instead of
//! surfacing `Incomplete`. Its dual is the rule it must not swallow: a genuinely *non-terminal*
//! error at the same frontier is still withheld.

use core::cell::Cell;
use std::rc::Rc;

use crate::{
  InputRef, Parse, ParseInput, Parser, Token, TryParseInput,
  cache::DefaultCache,
  emitter::{Fatal, Verbose},
  error::{Incomplete, MaybeIncomplete, token::UnexpectedToken},
  input::{ClosePayload, CloseStatus, Complete, Input, Partial, SurfaceIncomplete, parse_partial},
  lexer::LogosLexer,
  state::State,
};

// An error type that can carry the partial-input incomplete sentinel. `From<Incomplete>` is the
// exact construction path the frontier rules use (via `SurfaceIncomplete`), and `is_incomplete()`
// is what recovery keys the never-recoverable law off — the two must stay coherent.
#[derive(Debug, Clone, PartialEq)]
enum PErr {
  Lex,
  /// A resource-limit trip — the terminal outcome no amount of further input can clear.
  Limit,
  Incomplete(usize),
}

impl From<()> for PErr {
  fn from(_: ()) -> Self {
    PErr::Lex
  }
}

impl From<LimitExceeded> for PErr {
  fn from(_: LimitExceeded) -> Self {
    PErr::Limit
  }
}

impl From<Incomplete<usize>> for PErr {
  fn from(inc: Incomplete<usize>) -> Self {
    PErr::Incomplete(inc.into_offset())
  }
}

impl MaybeIncomplete for PErr {
  fn is_incomplete(&self) -> bool {
    matches!(self, PErr::Incomplete(_))
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for PErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    PErr::Lex
  }
}

impl<O, Lang: ?Sized> From<crate::error::UnexpectedEot<O, Lang>> for PErr {
  fn from(_: crate::error::UnexpectedEot<O, Lang>) -> Self {
    PErr::Lex
  }
}

impl<S, Lang: ?Sized> From<crate::error::syntax::FullContainer<S, Lang>> for PErr {
  fn from(_: crate::error::syntax::FullContainer<S, Lang>) -> Self {
    PErr::Lex
  }
}

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
enum PTok {
  #[regex(r"[a-z]+")]
  Word,
  #[regex(r"[0-9]+")]
  Num,
}

impl core::fmt::Display for PTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      PTok::Word => "word",
      PTok::Num => "number",
    })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PKind {
  Word,
  Num,
}

impl core::fmt::Display for PKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      PKind::Word => "word",
      PKind::Num => "number",
    })
  }
}

impl Token<'_> for PTok {
  type Kind = PKind;
  type Error = ();

  fn kind(&self) -> PKind {
    match self {
      PTok::Word => PKind::Word,
      PTok::Num => PKind::Num,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type Lex<'a> = LogosLexer<'a, PTok>;
type PartialCtx<'a> = (Verbose<PErr>, DefaultCache<'a, Lex<'a>>);
type CompleteCtx<'a> = (Verbose<PErr>, DefaultCache<'a, Lex<'a>>);

/// The observable outcome of draining an input to its first stop: the yielded token kinds, the
/// terminating result (`Ok(None)` for genuine end of input, `Err` otherwise), and how many
/// diagnostics the emitter collected.
struct Run {
  kinds: std::vec::Vec<PKind>,
  result: Result<Option<()>, PErr>,
  emitted: usize,
}

/// Drives a **partial** input over `src` with the given `is_final`, draining `next()` to its first
/// stop.
fn run_partial(src: &str, is_final: bool) -> Run {
  let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
    src,
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  if is_final {
    input.seal();
  }
  let mut emitter = Verbose::<PErr>::new();
  let (kinds, result) = {
    let mut inp = input.as_ref(&mut emitter);
    let mut kinds = std::vec::Vec::new();
    let result = loop {
      match inp.next() {
        Ok(Some(t)) => kinds.push(t.data().kind()),
        Ok(None) => break Ok(None),
        Err(e) => break Err(e),
      }
    };
    (kinds, result)
  };
  let emitted = emitter.errors().values().map(|g| g.len()).sum();
  Run {
    kinds,
    result,
    emitted,
  }
}

/// Drives a **complete** input over `src` — the oracle the `is_final == true` partial run must
/// match.
fn run_complete(src: &str) -> Run {
  let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
    src,
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let (kinds, result) = {
    let mut inp = input.as_ref(&mut emitter);
    let mut kinds = std::vec::Vec::new();
    let result = loop {
      match inp.next() {
        Ok(Some(t)) => kinds.push(t.data().kind()),
        Ok(None) => break Ok(None),
        Err(e) => break Err(e),
      }
    };
    (kinds, result)
  };
  let emitted = emitter.errors().values().map(|g| g.len()).sum();
  Run {
    kinds,
    result,
    emitted,
  }
}

// ── Rule 1: frontier holdback ───────────────────────────────────────────────────────

#[test]
fn holdback_token_touching_buffer_end() {
  // "foo" is one token spanning the whole buffer (0..3), so its end touches the buffer end.
  // Non-final: it may be a prefix of a longer word, so it is withheld and Incomplete surfaces.
  let run = run_partial("foo", false);
  assert!(run.kinds.is_empty(), "the frontier token is not yielded");
  assert_eq!(
    run.result,
    Err(PErr::Incomplete(3)),
    "Incomplete carries the frontier offset (the buffer end)"
  );
  assert!(
    run.result.unwrap_err().is_incomplete(),
    "the surfaced error reports itself incomplete (the never-recoverable law keys off this)"
  );
  assert_eq!(run.emitted, 0, "holdback emits nothing");
}

// ── Rule 2: frontier error ──────────────────────────────────────────────────────────

#[test]
fn holdback_error_touching_buffer_end() {
  // "foo @" — after the mid-buffer word "foo", the "@" is a lexer error at 4..5 whose span touches
  // the buffer end. Non-final: it may be a truncation artifact, so it is neither emitted nor
  // surfaced as an error — Incomplete surfaces instead.
  let run = run_partial("foo @", false);
  assert_eq!(
    run.kinds,
    std::vec![PKind::Word],
    "the mid-buffer word yields"
  );
  assert_eq!(
    run.result,
    Err(PErr::Incomplete(5)),
    "the frontier error surfaces Incomplete at the buffer end, not the lexer error"
  );
  assert_eq!(
    run.emitted, 0,
    "the frontier error is held back, not emitted"
  );
}

// ── Rule 3: non-final EOF ───────────────────────────────────────────────────────────

#[test]
fn nonfinal_eof_surfaces_incomplete() {
  // "foo " — "foo" ends at 3, strictly before the buffer end 4 (a trailing space), so it is NOT a
  // frontier token and yields normally. The whitespace tail then exhausts the lexer at a non-final
  // EOF, which surfaces Incomplete rather than genuine end of input.
  //
  // The reported offset is where the LEXER stopped, not where the last item it handed back
  // ended: the trailing space is consumed input, and a refill driver that subtracts this
  // offset from its buffer length to size the un-consumed tail must not count it as
  // pending. So the frontier here is 4, the buffer end, even though the last yielded item
  // ended at 3.
  let run = run_partial("foo ", false);
  assert_eq!(
    run.kinds,
    std::vec![PKind::Word],
    "the mid-buffer token (end < buffer end) yields normally"
  );
  assert_eq!(
    run.result,
    Err(PErr::Incomplete(4)),
    "the frontier is the lexer's end (4), not the last item's end (3) — the skipped \
     trailing space is consumed input"
  );
  assert_eq!(run.emitted, 0);
}

/// The trailing-skip case at its sharpest: an all-skipped buffer yields no item at all, so
/// an offset derived from the last yielded item would never leave its initial value even
/// though the lexer scanned every byte. Companion to `nonfinal_eof_surfaces_incomplete`.
#[test]
fn nonfinal_eof_after_an_all_skipped_buffer_reports_the_lexer_end() {
  let run = run_partial("   ", false);
  assert!(
    run.kinds.is_empty(),
    "an all-whitespace buffer yields nothing"
  );
  assert_eq!(
    run.result,
    Err(PErr::Incomplete(3)),
    "every byte was consumed by the lexer, so the refill frontier is 3"
  );
  assert_eq!(run.emitted, 0);
}

#[test]
fn nonfinal_eof_on_empty_buffer() {
  // An empty non-final chunk is entirely Incomplete: nothing to yield, more may arrive.
  // With zero bytes the lexer's end and the buffer end coincide at 0.
  let run = run_partial("", false);
  assert!(run.kinds.is_empty());
  assert_eq!(run.result, Err(PErr::Incomplete(0)));
}

// ── The non-final EOF frontier's floor and clamp ─────────────────────────────────────
//
// `Lexer::span()` once the lexer is exhausted is the one point the lexer contract leaves to
// the implementation, so the frontier a non-final EOF reports is floored by what was already
// lexed and clamped to the buffer. The logos backend reports the buffer end there, which
// makes both bounds inert on it — so they are exercised here by a hand-written lexer that
// reports a deliberately wrong post-exhaustion span, once in each direction.

/// What [`FrontierLexer`] reports from [`Lexer::span`] once it is exhausted. It travels in
/// the lexer state because `Lexer::with_state` is the only channel the input layer
/// configures a rebuilt lexer through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EofSpan {
  /// Retracts to the start of the buffer — behind every item the lexer yielded.
  Retracts,
  /// Overshoots well past the end of the buffer.
  Overshoots,
}

impl State for EofSpan {
  type Error = ();

  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

/// A hand-written lexer over the [`PTok`] vocabulary. It skips ASCII whitespace between
/// tokens exactly as the logos-backed fixture does — so the trailing-skip shape is the same
/// — and its only unusual behaviour is the span it reports at exhaustion ([`EofSpan`]).
struct FrontierLexer<'a> {
  src: &'a str,
  at: usize,
  span: crate::span::SimpleSpan,
  state: EofSpan,
}

impl<'a> crate::Lexer<'a> for FrontierLexer<'a> {
  type State = EofSpan;
  type Source = str;
  type Token = PTok;
  type Span = crate::span::SimpleSpan;
  type Offset = usize;

  fn new(src: &'a Self::Source) -> Self {
    Self::with_state(src, EofSpan::Retracts)
  }

  fn with_state(src: &'a Self::Source, state: Self::State) -> Self {
    Self {
      src,
      at: 0,
      span: crate::span::SimpleSpan { start: 0, end: 0 },
      state,
    }
  }

  fn check(&self) -> Result<(), <Self::Token as Token<'a>>::Error> {
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

  fn source(&self) -> &'a Self::Source {
    self.src
  }

  fn span(&self) -> Self::Span {
    self.span
  }

  fn slice(&self) -> &'a str {
    &self.src[self.span.start..self.span.end]
  }

  fn lex(&mut self) -> Option<Result<Self::Token, <Self::Token as Token<'a>>::Error>> {
    let bytes = self.src.as_bytes();
    while self.at < bytes.len() && bytes[self.at].is_ascii_whitespace() {
      self.at += 1;
    }
    if self.at >= bytes.len() {
      self.span = match self.state {
        EofSpan::Retracts => crate::span::SimpleSpan { start: 0, end: 0 },
        EofSpan::Overshoots => crate::span::SimpleSpan {
          start: bytes.len(),
          end: bytes.len() + 32,
        },
      };
      return None;
    }
    let start = self.at;
    let first = bytes[start];
    // Always advance at least one byte, so every item has the nonempty span the contract
    // requires.
    self.at += 1;
    let lexed = if first.is_ascii_digit() {
      while self.at < bytes.len() && bytes[self.at].is_ascii_digit() {
        self.at += 1;
      }
      Ok(PTok::Num)
    } else if first.is_ascii_lowercase() {
      while self.at < bytes.len() && bytes[self.at].is_ascii_lowercase() {
        self.at += 1;
      }
      Ok(PTok::Word)
    } else {
      Err(())
    };
    self.span = crate::span::SimpleSpan {
      start,
      end: self.at,
    };
    Some(lexed)
  }

  fn bump(&mut self, n: &Self::Offset) {
    self.at += *n;
    self.span = crate::span::SimpleSpan {
      start: self.at,
      end: self.at,
    };
  }
}

type FrontierCtx<'a> = (Verbose<PErr>, DefaultCache<'a, FrontierLexer<'a>>);

/// Drives a non-final partial input over `src` with the hand-written lexer in `mode`,
/// draining `next()` to its first stop.
fn run_frontier(src: &str, mode: EofSpan) -> Result<Option<()>, PErr> {
  let mut input = Input::<FrontierLexer<'_>, FrontierCtx<'_>, (), Partial>::with_state_and_cache(
    src,
    mode,
    DefaultCache::<'_, FrontierLexer<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  loop {
    match inp.next() {
      Ok(Some(_)) => {}
      Ok(None) => break Ok(None),
      Err(e) => break Err(e),
    }
  }
}

#[test]
fn nonfinal_eof_offset_is_never_behind_the_last_item() {
  // The floor. This lexer reports a post-exhaustion span of `0..0`, behind everything it
  // yielded; the frontier must still be at least the end of the last item ("bar", 4..7),
  // never the retracted 0.
  assert_eq!(
    run_frontier("foo bar ", EofSpan::Retracts),
    Err(PErr::Incomplete(7)),
    "a lexer whose post-exhaustion span retracts must not drag the refill frontier \
     behind the items it already yielded"
  );
}

#[test]
fn nonfinal_eof_offset_is_clamped_to_the_buffer() {
  // The clamp. This lexer reports a post-exhaustion span ending far past the buffer; the
  // frontier must not hand a refill driver an offset outside its own buffer.
  assert_eq!(
    run_frontier("foo bar ", EofSpan::Overshoots),
    Err(PErr::Incomplete(8)),
    "a lexer whose post-exhaustion span overshoots must not push the refill frontier \
     past the buffer end"
  );
}

// ── Mid-buffer items are unaffected while partial ─────────────────────────────────────

#[test]
fn mid_buffer_error_is_emitted_normally_while_partial() {
  // "foo @ bar" non-final: "foo" (0..3) yields, the "@" error (4..5) is *mid-buffer* (before the
  // end) so it is emitted and skipped exactly as in complete mode, and only "bar" (6..9, touching
  // the end) is held back → Incomplete. The mid-buffer error must still reach the emitter.
  let run = run_partial("foo @ bar", false);
  assert_eq!(run.kinds, std::vec![PKind::Word]);
  assert_eq!(
    run.result,
    Err(PErr::Incomplete(9)),
    "the trailing word touches the end and is held back"
  );
  assert_eq!(
    run.emitted, 1,
    "the mid-buffer lexer error is emitted normally in partial mode"
  );
}

// ── `is_final == true` is exact parity with a complete parse ─────────────────────────

#[test]
fn is_final_matches_complete() {
  // With is_final == true, a partial input behaves exactly like a complete one: every token is
  // yielded (the frontier holdback is off), a trailing error is emitted, and EOF is genuine.
  for src in ["foo", "foo bar baz", "foo @ bar", "12 ab 34", "", "x"] {
    let partial = run_partial(src, true);
    let complete = run_complete(src);
    assert_eq!(
      partial.kinds, complete.kinds,
      "final partial and complete yield the same tokens for {src:?}"
    );
    assert_eq!(
      partial.result, complete.result,
      "final partial and complete end the same way for {src:?}"
    );
    assert_eq!(
      partial.emitted, complete.emitted,
      "final partial and complete emit the same diagnostics for {src:?}"
    );
  }
}

// ── The complete path is untouched: it never surfaces Incomplete ─────────────────────

#[test]
fn complete_never_surfaces_incomplete() {
  // The same "foo" that a non-final partial holds back is a genuine, whole token in complete mode.
  let run = run_complete("foo");
  assert_eq!(run.kinds, std::vec![PKind::Word]);
  assert_eq!(run.result, Ok(None), "complete mode reaches genuine EOF");
}

// ── Exhaustive chunked-equivalence oracle over every split point ──────────────────────

/// A full observation of a drain: each yielded token as `(kind, start, end)`, each emitted lexer
/// error's `(start, end)` in span order, and the terminating result.
struct Trace {
  tokens: std::vec::Vec<(PKind, usize, usize)>,
  errors: std::vec::Vec<(usize, usize)>,
  result: Result<Option<()>, PErr>,
}

/// Drains a partial input over `src` at the given `is_final`, capturing the full [`Trace`].
fn trace_partial(src: &str, is_final: bool) -> Trace {
  let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
    src,
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  if is_final {
    input.seal();
  }
  let mut emitter = Verbose::<PErr>::new();
  let (tokens, result) = {
    let mut inp = input.as_ref(&mut emitter);
    let mut tokens = std::vec::Vec::new();
    let result = loop {
      match inp.next() {
        Ok(Some(t)) => tokens.push((t.data().kind(), *t.span().start_ref(), *t.span().end_ref())),
        Ok(None) => break Ok(None),
        Err(e) => break Err(e),
      }
    };
    (tokens, result)
  };
  let errors = collect_errors(&emitter);
  Trace {
    tokens,
    errors,
    result,
  }
}

/// Drains a complete input over `src`, capturing the full [`Trace`] — the oracle a chunked partial
/// run is checked against.
fn trace_complete(src: &str) -> Trace {
  let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
    src,
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let (tokens, result) = {
    let mut inp = input.as_ref(&mut emitter);
    let mut tokens = std::vec::Vec::new();
    let result = loop {
      match inp.next() {
        Ok(Some(t)) => tokens.push((t.data().kind(), *t.span().start_ref(), *t.span().end_ref())),
        Ok(None) => break Ok(None),
        Err(e) => break Err(e),
      }
    };
    (tokens, result)
  };
  let errors = collect_errors(&emitter);
  Trace {
    tokens,
    errors,
    result,
  }
}

/// Collects every emitted lexer error's `(start, end)` in span order from a verbose emitter.
fn collect_errors(emitter: &Verbose<PErr>) -> std::vec::Vec<(usize, usize)> {
  emitter
    .errors()
    .iter()
    .flat_map(|(span, group)| {
      let se = (*span.start_ref(), *span.end_ref());
      group.iter().map(move |_| se)
    })
    .collect()
}

/// The correctness oracle (crate-side, exhaustive): for **every** split point `k` of each corpus
/// string, a non-final partial drain of the prefix `src[0..k]` must
///
/// 1. yield exactly the complete-parse tokens that lie strictly before `k` (the frontier holdback
///    withholds the one touching the cut),
/// 2. emit exactly the complete-parse lexer errors that lie strictly before `k` (the frontier error
///    is held back), and
/// 3. always terminate with an `Incomplete` (a non-final drain never reports genuine end of input),
///
/// while a *final* drain of the whole string reproduces the complete parse exactly (the "complete
/// over the full input" leg of the resumption loop). Together these are the chunked-equivalence
/// guarantee: reassembling the chunk-by-chunk prefixes yields the same tokens and emission log as a
/// single complete parse.
#[test]
fn chunked_equivalence_over_every_split_point() {
  // A corpus mixing words, numbers, trailing/leading/interior whitespace, and lexer errors (`@`).
  const CORPUS: &[&str] = &[
    "",
    "a",
    "foo bar baz",
    "12 ab 345 cd",
    "  lead",
    "trail  ",
    "ab@cd",
    "foo @ bar @ baz",
    "a b c d e f",
    "x1 y2 z3",
  ];

  for &src in CORPUS {
    let complete = trace_complete(src);

    // The "complete over the full input" leg: a final partial drain equals the complete parse.
    let final_partial = trace_partial(src, true);
    assert_eq!(
      final_partial.tokens, complete.tokens,
      "final partial tokens must equal complete for {src:?}"
    );
    assert_eq!(
      final_partial.errors, complete.errors,
      "final partial emission log must equal complete for {src:?}"
    );
    assert_eq!(
      final_partial.result, complete.result,
      "final partial terminal must equal complete for {src:?}"
    );

    for k in 0..=src.len() {
      if !src.is_char_boundary(k) {
        continue;
      }
      let prefix = trace_partial(&src[..k], false);

      let expected_tokens: std::vec::Vec<_> = complete
        .tokens
        .iter()
        .copied()
        .filter(|&(_, _, end)| end < k)
        .collect();
      assert_eq!(
        prefix.tokens, expected_tokens,
        "prefix tokens diverge from the complete prefix for {src:?} at k={k}"
      );

      let expected_errors: std::vec::Vec<_> = complete
        .errors
        .iter()
        .copied()
        .filter(|&(_, end)| end < k)
        .collect();
      assert_eq!(
        prefix.errors, expected_errors,
        "prefix emission log diverges from the complete prefix for {src:?} at k={k}"
      );

      match &prefix.result {
        Err(e) => assert!(
          e.is_incomplete(),
          "a non-final prefix must terminate Incomplete for {src:?} at k={k}, got {e:?}"
        ),
        Ok(none) => panic!(
          "a non-final prefix never reports genuine end of input for {src:?} at k={k}, got Ok({none:?})"
        ),
      }
    }
  }
}

// ── The terminal-dominance law: a trip at the frontier is NOT an incomplete ───────────
//
// The three frontier rules withhold an item that later input could still change. A limit trip is
// not such an item: a limiter's tally is monotone, so no refill can un-trip it. The tripping token
// landing exactly on the buffer end must therefore fire the limit — diagnostic emitted, poison
// boundary latched — rather than surface `Incomplete` and invite the caller to feed more bytes to a
// limit that will never fire (the streaming DoS).

/// A limiter whose tally is **shared** across every lexer the input rebuilds (an `Rc<Cell<_>>`,
/// exactly as `tests::ProbeLimiter`), so a test can watch whether the tripping token was scanned at
/// all — and, after the latch, that it is never rescanned.
#[derive(Debug, Clone)]
struct LimitTracker {
  scanned: Rc<Cell<usize>>,
  limit: usize,
}

impl Default for LimitTracker {
  fn default() -> Self {
    // A limit-free default: only an explicitly constructed tracker ever trips.
    Self::with_limit(usize::MAX)
  }
}

impl LimitTracker {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: Rc::new(Cell::new(0)),
      limit,
    }
  }

  /// A shared handle on the scan counter, kept after the state is moved into the input.
  fn counter(&self) -> Rc<Cell<usize>> {
    self.scanned.clone()
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }
}

#[derive(Debug, Clone, PartialEq)]
struct LimitExceeded;

impl State for LimitTracker {
  type Error = LimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.scanned.get() > self.limit {
      Err(LimitExceeded)
    } else {
      Ok(())
    }
  }
}

/// A word lexer behind the limiter: every word bumps the tally, so the `(limit + 1)`-th word is the
/// tripping token — the Logos backend turns the post-token `check()` failure into a `Lexed::Error`
/// carrying that token's span. `@` is a plain (non-limit) lexer error, for the dual case.
#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = LimitTracker, skip r"[ \t\r\n]+")]
enum LTok {
  #[regex(r"[a-z]+", |lex| { lex.extras.increase(); })]
  Word,
}

impl core::fmt::Display for LTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

impl Token<'_> for LTok {
  type Kind = PKind;
  type Error = PErr;

  fn kind(&self) -> PKind {
    PKind::Word
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type LimLex<'a> = LogosLexer<'a, LTok>;
type LimCtx<'a> = (Verbose<PErr>, DefaultCache<'a, LimLex<'a>>);
type LimFatalCtx<'a> = (Fatal<PErr>, DefaultCache<'a, LimLex<'a>>);

/// What a limited partial drain is judged on: the yielded tokens, how it terminated, how many limit
/// diagnostics reached the emitter, and whether the input latched its poison boundary.
struct LimRun {
  kinds: std::vec::Vec<PKind>,
  result: Result<Option<()>, PErr>,
  limit_diags: usize,
  poisoned: bool,
}

/// Counts the limit diagnostics a verbose emitter collected.
fn limit_diags(emitter: &Verbose<PErr>) -> usize {
  emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == PErr::Limit)
    .count()
}

/// Drains `next()` over a **partial** input behind a `limit`-token limiter.
fn run_limited(src: &str, limit: usize, is_final: bool) -> LimRun {
  let tracker = LimitTracker::with_limit(limit);
  let mut input = Input::<LimLex<'_>, LimCtx<'_>, (), Partial>::with_state_and_cache(
    src,
    tracker,
    DefaultCache::<'_, LimLex<'_>>::default(),
  );
  if is_final {
    input.seal();
  }
  let mut emitter = Verbose::<PErr>::new();
  let (kinds, result, poisoned) = {
    let mut inp = input.as_ref(&mut emitter);
    let mut kinds = std::vec::Vec::new();
    let result = loop {
      match inp.next() {
        Ok(Some(t)) => kinds.push(t.data().kind()),
        Ok(None) => break Ok(None),
        Err(e) => break Err(e),
      }
    };
    (kinds, result, inp.is_poisoned())
  };
  LimRun {
    kinds,
    result,
    limit_diags: limit_diags(&emitter),
    poisoned,
  }
}

#[test]
fn frontier_limit_trip_is_terminal_not_incomplete() {
  // "a b c": three words; the limiter trips on the third. Its span is 4..5 — its end IS the buffer
  // end (len 5) — so it sits exactly on the non-final frontier, the alignment an attacker picks.
  //
  // Before the ordering fix the frontier holdback ran first and returned Incomplete here: no
  // diagnostic, no latch, and a streaming caller told to feed MORE bytes to a limit that had
  // already been exceeded.
  let run = run_limited("a b c", 2, false);

  assert_eq!(
    run.kinds,
    std::vec![PKind::Word, PKind::Word],
    "the two tokens under the limit yield"
  );
  assert_eq!(
    run.result,
    Ok(None),
    "the trip is TERMINAL: it stops the scan, and never masquerades as Incomplete"
  );
  assert!(
    !matches!(run.result, Err(ref e) if e.is_incomplete()),
    "a terminal trip must never surface on the Incomplete channel"
  );
  assert_eq!(
    run.limit_diags, 1,
    "the limit diagnostic IS emitted, even though the tripping token touches the buffer end"
  );
  assert!(
    run.poisoned,
    "the poison boundary IS latched, so no later operation can rescan past the trip"
  );
}

#[test]
fn frontier_limit_trip_is_terminal_on_the_peek_fill() {
  // The same alignment, reached through `peek` instead of `next`: the peek fill lexes the tripping
  // token at the frontier. A peek that merely withheld it would latch nothing and emit nothing, and
  // the caller's follow-up `next()` would then be told Incomplete — the same mask, one path over.
  let tracker = LimitTracker::with_limit(2);
  let scanned = tracker.counter();
  let mut input = Input::<LimLex<'_>, LimCtx<'_>, (), Partial>::with_state_and_cache(
    "a b c",
    tracker,
    DefaultCache::<'_, LimLex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let (peeked, poisoned, after) = {
    use generic_arraydeque::typenum::U4;

    // Born open: a fresh `Partial` input is non-final until a driver seals it.
    let mut inp = input.as_ref(&mut emitter);

    // A window of 4 over a 3-token source: the fill runs into the tripping token.
    let peeked = inp
      .peek::<U4>()
      .expect("a peek never surfaces Incomplete")
      .len();
    let poisoned = inp.is_poisoned();

    // Draining afterwards must reach the same terminal stop — never Incomplete.
    let mut kinds = std::vec::Vec::new();
    let after = loop {
      match inp.next() {
        Ok(Some(t)) => kinds.push(t.data().kind()),
        Ok(None) => break Ok(kinds),
        Err(e) => break Err(e),
      }
    };
    (peeked, poisoned, after)
  };

  assert_eq!(peeked, 2, "the peek serves the two tokens under the limit");
  assert!(
    poisoned,
    "the peek fill latched the poison boundary on the frontier trip"
  );
  assert_eq!(
    limit_diags(&emitter),
    1,
    "the peek fill emitted the limit diagnostic rather than silently withholding it"
  );
  assert_eq!(
    after,
    Ok(std::vec![PKind::Word, PKind::Word]),
    "the follow-up drain serves the cached prefix and stops TERMINALLY, not Incomplete"
  );
  assert_eq!(
    scanned.get(),
    3,
    "the tripping token was scanned exactly once; the latch stops it being rescanned"
  );
}

#[test]
fn frontier_nonterminal_error_is_still_withheld() {
  // The rule the fix must NOT break. `@` is a plain lexer error at 2..3, touching the buffer end,
  // and the limiter has NOT tripped (limit 5, one word scanned). A truncated buffer really can make
  // a valid token look like a lex error, so this one IS withheld: Incomplete, nothing emitted,
  // nothing latched. The holdback was narrowed to non-terminal items, not removed.
  let run = run_limited("a @", 5, false);

  assert_eq!(
    run.kinds,
    std::vec![PKind::Word],
    "the mid-buffer word yields"
  );
  assert_eq!(
    run.result,
    Err(PErr::Incomplete(3)),
    "a NON-terminal frontier error is still held back as Incomplete"
  );
  assert_eq!(run.limit_diags, 0, "no limit was tripped");
  assert!(
    !run.poisoned,
    "a non-terminal frontier error latches no poison boundary"
  );
}

#[test]
fn mid_buffer_limit_trip_is_unaffected() {
  // The control: the same trip, but with the tripping token strictly before the buffer end (a
  // trailing space). This always worked — it is the alignment the attacker avoids — and it must
  // keep behaving identically to the frontier case, which is the point of the law.
  let run = run_limited("a b c ", 2, false);

  assert_eq!(run.kinds, std::vec![PKind::Word, PKind::Word]);
  assert_eq!(run.result, Ok(None), "the trip stops the scan");
  assert_eq!(run.limit_diags, 1);
  assert!(run.poisoned);
}

#[test]
fn final_and_complete_agree_with_the_frontier_trip() {
  // A final partial drain and the frontier-trip case observe the same terminal facts: `is_final`
  // changes nothing about a trip, because a trip was never about the frontier.
  let fin = run_limited("a b c", 2, true);
  assert_eq!(fin.kinds, std::vec![PKind::Word, PKind::Word]);
  assert_eq!(fin.result, Ok(None));
  assert_eq!(fin.limit_diags, 1);
  assert!(fin.poisoned);
}

// ── The driver-level DoS: the refill loop must TERMINATE ───────────────────────────────

/// The parser every driver round runs: drain to the first stop, counting tokens. `?` propagates
/// whatever the input surfaces — an `Incomplete` (refill and re-drive) or a terminal error (stop).
fn drain_all<'inp>(
  inp: &mut InputRef<'inp, '_, LimLex<'inp>, LimCtx<'inp>, (), Partial>,
) -> Result<usize, PErr> {
  let mut n = 0usize;
  while inp.next()?.is_some() {
    n += 1;
  }
  Ok(n)
}

/// The same drain under a **fatal** emitter, which rejects the limit diagnostic instead of
/// recovering from it.
fn drain_all_fatal<'inp>(
  inp: &mut InputRef<'inp, '_, LimLex<'inp>, LimFatalCtx<'inp>, (), Partial>,
) -> Result<usize, PErr> {
  let mut n = 0usize;
  while inp.next()?.is_some() {
    n += 1;
  }
  Ok(n)
}

/// One round of the documented Sans-I/O refill loop over `buffer`: build a `Partial` input, drain
/// it, and report what the caller would see.
fn drive_round(buffer: &str, limit: usize) -> Result<usize, PErr> {
  let ctx: LimCtx<'_> = (
    Verbose::<PErr>::new(),
    DefaultCache::<'_, LimLex<'_>>::default(),
  );
  parse_partial(
    ctx,
    buffer,
    LimitTracker::with_limit(limit),
    /* is_final */ false,
    drain_all,
  )
}

#[test]
fn parse_partial_refill_loop_terminates_on_a_frontier_trip() {
  // THE denial-of-service, driven end to end. The attacker extends the tripping token one byte at a
  // time, so it ends exactly at the buffer end on EVERY round: "a b c", "a b cc", "a b ccc", …
  //
  // If a frontier trip surfaced as Incomplete, each round would say "send more" — the caller would
  // refill forever, re-lexing an ever-growing buffer, and the token limit would never fire. The
  // loop below caps the rounds and fails if the cap is reached: it must stop on round ONE.
  let mut buffer = std::string::String::from("a b c");
  let mut rounds = 0usize;
  let outcome = loop {
    rounds += 1;
    assert!(
      rounds <= 8,
      "the refill loop never terminated: a frontier limit trip was masked as Incomplete, so the \
       limit never fired and an attacker aligned to the chunk boundary drives an unbounded refill"
    );
    match drive_round(&buffer, 2) {
      // The caller's own refill step: append the attacker's next byte and re-drive.
      Err(ref e) if e.is_incomplete() => buffer.push('c'),
      other => break other,
    }
  };

  assert_eq!(rounds, 1, "the trip terminates the loop on the FIRST round");
  assert_eq!(
    outcome,
    Ok(2),
    "the recovering emitter turns the trip into a bounded stop after the two tokens under the limit"
  );
}

#[test]
fn parse_partial_frontier_trip_is_fatal_under_a_fatal_emitter() {
  // The same round under a `Fatal` emitter: the limit diagnostic is *rejected*, so the trip leaves
  // on the `Err` channel — as `PErr::Limit`, which reports itself NOT incomplete, so the refill
  // loop stops instead of asking for bytes that cannot help.
  let ctx: LimFatalCtx<'_> = (Fatal::of(), DefaultCache::<'_, LimLex<'_>>::default());
  let out: Result<usize, PErr> = parse_partial(
    ctx,
    "a b c",
    LimitTracker::with_limit(2),
    /* is_final */ false,
    drain_all_fatal,
  );

  assert_eq!(
    out,
    Err(PErr::Limit),
    "the fatal emitter surfaces the limit trip itself, not an Incomplete"
  );
  assert!(
    !out.unwrap_err().is_incomplete(),
    "a terminal trip never reports itself incomplete — the refill loop must not retry it"
  );
}

#[test]
fn refill_loop_still_refills_on_a_genuine_incomplete() {
  // The counterweight: an ordinary truncated word DOES resume. The loop refills until the word is
  // whole and the input is marked final — proving the fix narrowed the holdback to terminal items
  // rather than turning every frontier item into a stop.
  let chunks = ["foo b", "ar ", "baz"];
  let mut buffer = std::string::String::new();
  let mut incompletes = 0usize;
  let mut parsed = None;
  for (i, chunk) in chunks.iter().enumerate() {
    buffer.push_str(chunk);
    let is_final = i + 1 == chunks.len();
    let ctx: LimCtx<'_> = (
      Verbose::<PErr>::new(),
      DefaultCache::<'_, LimLex<'_>>::default(),
    );
    let out = parse_partial(
      ctx,
      buffer.as_str(),
      LimitTracker::with_limit(usize::MAX),
      is_final,
      drain_all,
    );
    match out {
      Ok(n) => {
        parsed = Some(n);
        break;
      }
      Err(e) if e.is_incomplete() => incompletes += 1,
      Err(e) => panic!("unexpected error {e:?}"),
    }
  }
  assert_eq!(
    parsed,
    Some(3),
    "the three words parse once the input is final"
  );
  assert_eq!(
    incompletes, 2,
    "the two non-final chunks each cut a word at the frontier and DID ask for more"
  );
}

// ── Finality is a WORLD fact: monotone, driver-owned, and outside the rollback set ────────────
//
// The two halves of one law, and they are mirrors of each other. Break either and a streaming
// parser is wrong in a way the type system used to permit:
//
//   * a parser that could END a stream would lose the frontier holdback — speculate,
//     `set_final(true)`, fail, roll back, and the rollback would not undo it (rollback rewinds the
//     PARSE, not the WORLD), so the next read hands back a token the frontier owed an `Incomplete`
//     for. That is the leak.
//   * a rollback that could UN-END a stream — the "obvious" fix of checkpointing the flag and
//     restoring it — would leave a parser asking for a refill that can never come. That is a hang,
//     and it is strictly worse than the leak it fixes.
//
// Both bugs share the premise that a parser can touch the bit at all. It cannot: `is_final` is
// settable only through the owning `Input` (`seal`, monotone), which an `InputRef` mutably borrows
// for its whole life. The compile-fail proof of the unreachability lives on `InputRef::is_final`;
// these two are the observable laws it buys.

/// LAW: a failed speculative branch cannot cost the frontier holdback.
///
/// The source ends mid-construct (`cd` touches the buffer end of a non-final buffer), so every read
/// past `ab` owes an `Incomplete`. A parser throws the crate's entire speculative surface at the
/// input and abandons all of it. Nothing it did — at any depth, through any guard — may leave the
/// input final, and the frontier must still owe `Incomplete` afterwards.
#[test]
fn speculation_cannot_end_the_stream() {
  let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
    "ab cd",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  // NOT sealed: the driver has not said the stream ended, so `cd` may yet become `cdef`.
  let mut emitter = Verbose::<PErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(!inp.is_final(), "a fresh partial input is born open");

  // `ab` is clear of the frontier and yields normally.
  assert!(inp.next().expect("ab is clear of the frontier").is_some());

  // Every rollback shape the crate has, all of them abandoned.
  let declined: Option<()> = inp.attempt(|i| {
    let _ = i.next();
    None
  });
  assert!(declined.is_none());

  let errored: Result<(), ()> = inp.try_attempt(|i| {
    let _ = i.next();
    Err(())
  });
  assert!(errored.is_err());

  {
    let mut txn = inp.begin();
    let _ = txn.next();
    txn.rollback();
  }
  {
    // Undecided drop under the `Rollback` policy.
    let mut txn = inp.begin();
    let _ = txn.next();
  }
  {
    let mut txn = inp.begin_stacked();
    let _ = txn.next();
    let sp = txn.savepoint();
    let _ = txn.next();
    txn.rollback_to(sp);
    txn.rollback();
  }
  let point = inp.begin_point();
  let _ = inp.next();
  inp.rollback_point(point);

  // The world did not move: no parser said the stream ended, so it did not end.
  assert!(
    !inp.is_final(),
    "a speculative branch ENDED THE STREAM: the frontier holdback is gone and the next read will \
     hand back a token that later input could still extend"
  );

  // And the observable half: the frontier still owes an Incomplete for `cd`.
  match inp.next() {
    Err(PErr::Incomplete(at)) => assert_eq!(at, 5, "the frontier is the buffer end"),
    other => panic!(
      "LAW VIOLATED: a rolled-back speculative branch cost the frontier holdback — `cd` touches a \
       NON-FINAL buffer end and is owed an Incomplete, but next() yielded {other:?}"
    ),
  }
}

/// LAW (the mirror): a rollback cannot un-end a stream the driver already ended.
///
/// The driver seals — the last chunk landed, the socket is closed, there are no more bytes. A parser
/// then speculates across that fact and rolls every bit of it back. The input must still be final,
/// and the drain must reach genuine end of input: an `Incomplete` here would be a request for a
/// refill that can never be satisfied, and the caller would loop forever.
///
/// This is the test that fails on the "obvious" fix (checkpoint the finality flag, restore it on
/// rollback) — it closes the leak the sibling test guards and opens this hang in its place.
#[test]
fn rollback_cannot_un_end_a_sealed_stream() {
  let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
    "ab cd",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  // The world fact: the stream has ENDED. Only the driver can say this, and only here — with no
  // handle alive, which is exactly when a driver can honestly know it.
  input.seal();

  let mut emitter = Verbose::<PErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(inp.is_final(), "the driver sealed the stream");

  // The same full speculative surface, all of it abandoned.
  let declined: Option<()> = inp.attempt(|i| {
    let _ = i.next();
    let _ = i.next();
    None
  });
  assert!(declined.is_none());

  let errored: Result<(), ()> = inp.try_attempt(|i| {
    let _ = i.next();
    Err(())
  });
  assert!(errored.is_err());

  {
    let mut txn = inp.begin();
    let _ = txn.next();
    txn.rollback();
  }
  {
    let mut txn = inp.begin();
    let _ = txn.next();
  }
  {
    let mut txn = inp.begin_stacked();
    let _ = txn.next();
    let sp = txn.savepoint();
    let _ = txn.next();
    txn.rollback_to(sp);
    txn.rollback();
  }
  let point = inp.begin_point();
  let _ = inp.next();
  inp.rollback_point(point);

  assert!(
    inp.is_final(),
    "A ROLLBACK UN-ENDED A SEALED STREAM: the parser will now wait forever for bytes that will \
     never arrive"
  );

  // The observable half: a full drain reaches genuine end of input, and NEVER Incomplete —
  // including on `cd`, which touches the buffer end but is no longer a frontier, because the
  // frontier is gone: the stream ended.
  let mut kinds = std::vec::Vec::new();
  loop {
    match inp.next() {
      Ok(Some(t)) => kinds.push(t.data().kind()),
      Ok(None) => break,
      Err(e) => panic!(
        "LAW VIOLATED: a SEALED stream surfaced {e:?} after a rollback — the refill it asks for can \
         never come, so the caller loops forever"
      ),
    }
  }
  assert_eq!(
    kinds,
    [PKind::Word, PKind::Word],
    "a sealed drain yields every token, including the one at the buffer end"
  );
}

/// The seal is **monotone**: sealing twice is a no-op, and there is no inverse anywhere in the
/// crate to un-seal with. The type system carries the law — this pins that `seal` itself does not
/// quietly toggle.
#[test]
fn the_seal_is_monotone() {
  let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
    "ab cd",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  assert!(!input.as_ref(&mut emitter).is_final(), "born open");

  input.seal();
  assert!(input.as_ref(&mut emitter).is_final(), "sealed");

  input.seal();
  assert!(
    input.as_ref(&mut emitter).is_final(),
    "sealing an already-sealed stream is a no-op, never a toggle"
  );
}

// ── Write once, run in both modes ───────────────────────────────────────────────────
//
// ONE parser fn, generic over the completeness typestate, drives green under BOTH the
// complete combinator driver (`Parser…parse_str`, `Cmpl = Complete`) and the Sans-I/O
// partial driver (`parse_partial`, `Cmpl = Partial`) — the release's point, at runtime.

/// The write-once parser: collects every token kind to end of input under a
/// rollback-on-drop transaction. Generic over `Cmpl`; each drive site instantiates it.
fn kinds_generic<'inp, Cmpl>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, PartialCtx<'inp>, (), Cmpl>,
) -> Result<std::vec::Vec<PKind>, PErr>
where
  Cmpl: SurfaceIncomplete<'inp, Lex<'inp>, PartialCtx<'inp>, ()>,
{
  let mut txn = inp.begin();
  let mut kinds = std::vec::Vec::new();
  while let Some(t) = txn.next()? {
    kinds.push(t.data().kind());
  }
  txn.commit();
  Ok(kinds)
}

/// Drives `kinds_generic` COMPLETE through the public combinator driver.
fn drive_complete(src: &str) -> std::vec::Vec<PKind> {
  let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
  Parser::with_context(ctx)
    .apply(kinds_generic)
    .parse_str(src)
    .expect("complete drive of the write-once parser succeeds")
}

/// Drives `kinds_generic` PARTIAL through `parse_partial` over a chunked buffer,
/// returning the parsed kinds and how many rounds surfaced `Incomplete` (refills).
fn drive_partial_chunked(chunks: &[&str]) -> (std::vec::Vec<PKind>, usize) {
  let mut buffer = std::string::String::new();
  let mut incompletes = 0;
  for (i, chunk) in chunks.iter().enumerate() {
    buffer.push_str(chunk);
    let is_final = i + 1 == chunks.len();
    let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
    match parse_partial(ctx, buffer.as_str(), (), is_final, kinds_generic) {
      Ok(kinds) => return (kinds, incompletes),
      Err(e) if e.is_incomplete() => incompletes += 1,
      Err(e) => panic!("a real parse error: {e:?}"),
    }
  }
  panic!("the final chunk must complete the parse");
}

#[test]
fn write_once_runs_both_modes() {
  // The probe's exact shape: ["foo b", "ar ", "baz"] — the first two non-final
  // rounds each cut a token at the frontier, the sealed round parses the whole sentence.
  let complete = drive_complete("foo bar baz");
  let (partial, incompletes) = drive_partial_chunked(&["foo b", "ar ", "baz"]);
  assert_eq!(complete, std::vec![PKind::Word, PKind::Word, PKind::Word]);
  assert_eq!(partial, complete, "one parser fn, two modes, one answer");
  assert_eq!(
    incompletes, 2,
    "each non-final chunk surfaced exactly one Incomplete"
  );
}

#[test]
fn write_once_chunk_sweep_equivalence() {
  // Write-once as a deterministic sweep (the fuzz oracle's "chunked prefixes" shape): for EVERY
  // cut point of the corpus, the two-chunk partial drive must (1) surface Incomplete on
  // the non-final round — this parser drains to end of input, and rule 3 makes a
  // non-final end Incomplete — and (2) end with output identical to the complete drive.
  let corpus = "alpha beta42 gamma 7delta epsilon";
  let oracle = drive_complete(corpus);
  for cut in 1..corpus.len() {
    let (kinds, incompletes) = drive_partial_chunked(&[&corpus[..cut], &corpus[cut..]]);
    assert_eq!(
      kinds, oracle,
      "chunked equivalence must hold at cut point {cut}"
    );
    assert_eq!(
      incompletes, 1,
      "the non-final round at cut {cut} surfaces exactly one Incomplete"
    );
  }
}

#[test]
fn typed_local_fn_at_parse_partial() {
  // The concrete-`Partial` doctest pattern (a typed local fn, not a generic one) stays
  // the supported spelling under the trait bound.
  fn local<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, PartialCtx<'inp>, (), Partial>,
  ) -> Result<usize, PErr> {
    let mut txn = inp.begin();
    let mut n = 0;
    while txn.next()?.is_some() {
      n += 1;
    }
    txn.commit();
    Ok(n)
  }
  let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
  assert_eq!(parse_partial(ctx, "foo bar", (), true, local), Ok(2));
}

// ── The gate mechanism: `SurfaceIncomplete::is_incomplete_error` ─────────────────────

#[test]
fn is_incomplete_error_constant_false_at_complete() {
  // Complete's arm is a constant, bound-free `false` — even for an error value that
  // *would* read incomplete: a complete input never constructs one, so the atom-layer
  // gate `if Cmpl::is_incomplete_error(&err)` const-folds away on the complete path.
  assert!(!<Complete as SurfaceIncomplete<
    '_,
    Lex<'_>,
    PartialCtx<'_>,
    (),
  >>::is_incomplete_error(&PErr::Incomplete(0)));
  assert!(!<Complete as SurfaceIncomplete<
    '_,
    Lex<'_>,
    PartialCtx<'_>,
    (),
  >>::is_incomplete_error(&PErr::Lex));
}

#[test]
fn is_incomplete_error_routes_through_maybe_incomplete_at_partial() {
  // Partial routes through `MaybeIncomplete`: exactly the incomplete sentinel reads true;
  // a plain error and a terminal limit trip both read false (terminal outranks incomplete
  // and must never be re-raised as one).
  assert!(<Partial as SurfaceIncomplete<
    '_,
    Lex<'_>,
    PartialCtx<'_>,
    (),
  >>::is_incomplete_error(&PErr::Incomplete(3)));
  assert!(!<Partial as SurfaceIncomplete<
    '_,
    Lex<'_>,
    PartialCtx<'_>,
    (),
  >>::is_incomplete_error(&PErr::Lex));
  assert!(!<Partial as SurfaceIncomplete<
    '_,
    Lex<'_>,
    PartialCtx<'_>,
    (),
  >>::is_incomplete_error(&PErr::Limit));
}

// ── The generalized scanner drivers under Partial ───────────────────────────────────
//
// Each G-class driver (0.3.0: `try_expect`, `skip_while`, `sync_to`, `sync_through`,
// `sync_balanced`, `fold`/`foldn`, `consume_cached_*`) runs under `Partial` through its
// now-generic header. The matrix per driver: non-final ⇒ `Incomplete` at the frontier
// (conservative — later input could extend what it stopped on); final ⇒ identical to the
// complete-mode outcome. Plus the poison-at-frontier precedence case for `try_expect`.

/// Inlines a partial-input drive: builds the input over `$src`, seals per `$is_final`,
/// hands the handle to `$body`, and evaluates to `(body_output, emitted_diagnostics)`.
macro_rules! with_partial {
  ($src:expr, $is_final:expr, |$inp:ident| $body:expr) => {{
    let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
      $src,
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    if $is_final {
      input.seal();
    }
    let mut emitter = Verbose::<PErr>::new();
    let out = {
      #[allow(unused_mut)]
      let mut $inp = input.as_ref(&mut emitter);
      $body
    };
    let emitted: usize = emitter.errors().values().map(|g| g.len()).sum();
    (out, emitted)
  }};
}

#[test]
fn try_expect_partial_matrix() {
  // Non-final, the only token touches the buffer end: withheld, Incomplete.
  let (r, emitted) = with_partial!("foo", false, |inp| inp.try_expect(|_| true));
  assert_eq!(r, Err(PErr::Incomplete(3)));
  assert_eq!(emitted, 0);
  // Final: consumed, exactly as complete mode.
  let (r, _) = with_partial!("foo", true, |inp| {
    inp.try_expect(|_| true).map(|t| t.map(|s| s.data().kind()))
  });
  assert_eq!(r, Ok(Some(PKind::Word)));
  // The decline path puts a MID-BUFFER token back without any incomplete: "foo" ends
  // strictly before the trailing space, so it lexes even non-final, and the declining
  // predicate leaves it cached for the next consume.
  let (r, emitted) = with_partial!("foo ", false, |inp| {
    let declined = inp
      .try_expect(|t| matches!(t.data(), PTok::Num))
      .expect("the decline path is not an error");
    assert!(declined.is_none(), "the word is not a number");
    inp
      .try_expect(|t| matches!(t.data(), PTok::Word))
      .map(|t| t.map(|s| s.data().kind()))
  });
  assert_eq!(r, Ok(Some(PKind::Word)));
  assert_eq!(emitted, 0);
}

#[test]
fn try_expect_or_stop_partial_matrix() {
  // Terminal beats incomplete ON THE ATTEMPT PATH: a limit trip at the attempt is a
  // terminal stop, so `try_expect_or_stop` surfaces an error that is NOT Incomplete —
  // even though the tripping token sits at the non-final frontier. ("a b c": the two
  // words under the limit are consumed, then the attempt's scan trips on `c`.)
  let tracker = LimitTracker::with_limit(2);
  let mut input = Input::<LimLex<'_>, LimCtx<'_>, (), Partial>::with_state_and_cache(
    "a b c",
    tracker,
    DefaultCache::<'_, LimLex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    assert!(inp.next().unwrap().is_some(), "first word under the limit");
    assert!(inp.next().unwrap().is_some(), "second word under the limit");
    let err = inp
      .try_expect_or_stop(|_| true)
      .expect_err("a trip at the attempt is an error, never a decline");
    assert!(
      !err.is_incomplete(),
      "terminal beats incomplete on the attempt path, got {err:?}"
    );
  }
  assert_eq!(
    limit_diags(&emitter),
    1,
    "the trip's own diagnostic reached the emitter"
  );

  // A plain (non-terminal) frontier holdback at the attempt still surfaces
  // Incomplete on the `Err` channel, exactly as `try_expect` — the unchanged
  // `scan_with` routing.
  let (r, emitted) = with_partial!("foo", false, |inp| inp.try_expect_or_stop(|_| true));
  assert_eq!(r, Err(PErr::Incomplete(3)));
  assert_eq!(emitted, 0);
}

#[test]
fn skip_while_partial_matrix() {
  // Non-final: the trivia run's last word touches the buffer end — it may extend, so the
  // run is not finishable yet. (This is what hands `padded` its partial semantics.)
  let (r, _) = with_partial!("foo bar", false, |inp| {
    inp.skip_while(|t| matches!(t.data(), PTok::Word))
  });
  assert_eq!(r, Err(PErr::Incomplete(7)));
  // Final: the run completes and the stopper is left at the cache front.
  let (r, _) = with_partial!("foo 42", true, |inp| {
    inp
      .skip_while(|t| matches!(t.data(), PTok::Word))
      .expect("the final-mode skip completes");
    inp.next().map(|t| t.map(|s| s.data().kind()))
  });
  assert_eq!(r, Ok(Some(PKind::Num)));
}

#[test]
fn sync_to_partial_matrix() {
  // Non-final: the would-be sync token touches the buffer end — the next chunk could
  // extend it ("42" → "425"), so recovery-sync is not decidable yet: Incomplete.
  let (r, emitted) = with_partial!("foo bar 42", false, |inp| {
    inp
      .sync_to(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.is_some())
  });
  assert_eq!(r, Err(PErr::Incomplete(10)));
  assert_eq!(
    emitted, 0,
    "an `Incomplete` exit leaves NO TRACE: the mid-buffer words this attempt skipped and \
     diagnosed are rewound with the rest of the attempt, so refill-and-retry is idempotent. \
     (Before 0.8.0 this arm pinned the opposite — emit-as-you-go — which was D35 stated as \
     intent: the diagnostics accumulated once per retry.)"
  );
  // Final: syncs to the number, diagnosing the two skipped words — complete-identical.
  let (r, emitted) = with_partial!("foo bar 42", true, |inp| {
    inp
      .sync_to(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.is_some())
  });
  assert_eq!(r, Ok(true));
  assert_eq!(
    emitted, 2,
    "each skipped token is diagnosed, as in complete mode"
  );
}

#[test]
fn sync_through_partial_matrix() {
  let (r, _) = with_partial!("foo bar 42", false, |inp| {
    inp
      .sync_through(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.map(|t| t.data().kind()))
  });
  assert_eq!(r, Err(PErr::Incomplete(10)));
  let (r, emitted) = with_partial!("foo bar 42", true, |inp| {
    inp
      .sync_through(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.map(|t| t.data().kind()))
  });
  assert_eq!(
    r,
    Ok(Some(PKind::Num)),
    "through-sync consumes the sync token"
  );
  assert_eq!(emitted, 2);
}

#[test]
fn sync_balanced_partial_matrix() {
  use crate::input::Balance;
  // Non-final: same conservatism through the balanced scanner.
  let (r, _) = with_partial!("foo bar 42", false, |inp| {
    inp
      .sync_balanced(
        |_k: &PKind| Balance::<char>::Neutral,
        |t| matches!(t.data(), PTok::Num),
      )
      .map(|h| h.is_some())
  });
  assert_eq!(r, Err(PErr::Incomplete(10)));
  // Final: the hole over the skipped words is produced, with no per-token diagnostics.
  let (r, _) = with_partial!("foo bar 42", true, |inp| {
    inp
      .sync_balanced(
        |_k: &PKind| Balance::<char>::Neutral,
        |t| matches!(t.data(), PTok::Num),
      )
      .map(|h| h.is_some())
  });
  assert_eq!(r, Ok(true));
}

#[test]
fn fold_partial_matrix() {
  // Non-final: the last folded word touches the end — the fold is not finishable.
  let (r, _) = with_partial!("foo bar baz", false, |inp| {
    inp.fold(|t| matches!(t.data(), PTok::Word), || 0usize, |n, _| n + 1)
  });
  assert_eq!(r, Err(PErr::Incomplete(11)));
  // Final: all three words fold — complete-identical.
  let (r, _) = with_partial!("foo bar baz", true, |inp| {
    inp.fold(|t| matches!(t.data(), PTok::Word), || 0usize, |n, _| n + 1)
  });
  assert_eq!(r, Ok(3));
}

#[test]
fn foldn_stops_mid_buffer_without_incomplete() {
  // `foldn(2)` consumes exactly the two MID-BUFFER words and never reaches the frontier
  // token, so even a non-final run completes: the frontier rules hold back only what the
  // drive actually touches.
  let (r, _) = with_partial!("foo bar baz", false, |inp| {
    inp.foldn(|| 0usize, |n, _| n + 1, 2)
  });
  assert_eq!(r, Ok(2));
}

#[test]
fn consume_cached_never_surfaces_incomplete() {
  // `consume_cached_*` never lexes (bound-only generalization, no `SurfaceIncomplete`):
  // it pops what peeking already cached, and the peek fill never caches a frontier token,
  // so the mid-buffer word is consumable and the frontier word simply is not there.
  let (r, emitted) = with_partial!("foo bar", false, |inp| {
    let peeked = inp
      .peek_one()
      .expect("the partial peek never errs — a short window, not an error")
      .is_some();
    let first = inp.consume_cached_one().map(|t| t.data().kind());
    let second = inp.consume_cached_one().map(|t| t.data().kind());
    (peeked, first, second)
  });
  assert_eq!(r, (true, Some(PKind::Word), None));
  assert_eq!(emitted, 0);
}

#[test]
fn try_expect_poison_at_frontier_beats_incomplete() {
  // Terminal beats incomplete THROUGH the generalized driver: "a b c" behind a 2-word
  // limiter, non-final — the tripping third word ends exactly at the buffer end, and the
  // trip must fire as `Limit` (diagnostic + latch), never be re-raised as `Incomplete`.
  let tracker = LimitTracker::with_limit(2);
  let mut input = Input::<LimLex<'_>, LimCtx<'_>, (), Partial>::with_state_and_cache(
    "a b c",
    tracker,
    DefaultCache::<'_, LimLex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let (results, poisoned) = {
    let mut inp = input.as_ref(&mut emitter);
    let a = inp.try_expect(|_| true).map(|t| t.is_some());
    let b = inp.try_expect(|_| true).map(|t| t.is_some());
    let c = inp.try_expect(|_| true).map(|t| t.is_some());
    ((a, b, c), inp.is_poisoned())
  };
  assert_eq!(results.0, Ok(true));
  assert_eq!(results.1, Ok(true));
  // Under the collecting emitter the trip's diagnostic is EMITTED (not returned), the
  // poison boundary latches, and the drive reads the terminal stop — a decline at the
  // boundary, exactly as the `next()`-driven twin above — and NEVER `Incomplete`.
  assert_eq!(
    results.2,
    Ok(false),
    "the frontier-aligned trip is a terminal stop, never re-raised as incomplete"
  );
  assert!(poisoned, "the poison boundary latched at the trip");
  assert_eq!(
    limit_diags(&emitter),
    1,
    "the trip was diagnosed exactly once"
  );
}

// ── The combinator Lego under both modes (the chain oracle) ──────────────────────────
//
// ONE `Cmpl`-generic CHAIN — free leaf atom → adapter → try-driven collection — assembled
// once and driven under Complete AND Partial-chunked to equivalence. This is the test the
// probe could not run: it needs the whole atoms wave (threaded builder returns, A-class
// leaf impls, B-class collection impls) to compose.

/// The write-once Lego chain, assembled INSIDE one `Cmpl`-generic fn (the corpus's
/// parser-fn shape) and driven under both modes: `expect(word) → map(kind) → repeated →
/// collect -> map`, crossing a leaf try-atom, the try-driven collection, and a
/// threaded adapter — assembled once, driven in both modes.
fn lego_chain<'inp, Ctx, Cmpl>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx, (), Cmpl>,
) -> Result<std::vec::Vec<PKind>, PErr>
where
  Ctx: crate::ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: crate::Emitter<'inp, Lex<'inp>, Error = PErr>
    + crate::emitter::FullContainerEmitter<'inp, Lex<'inp>>,
  Cmpl: SurfaceIncomplete<'inp, Lex<'inp>, Ctx, ()>,
{
  use crate::Accumulator as _;
  try_word
    .repeated()
    .collect()
    .map(|words: std::vec::Vec<PTok>| words.into_iter().map(|t| t.kind()).collect())
    .parse_input(inp)
}

/// The chain's leaf: a `Cmpl`-generic try-atom over the scan chokepoint (the same
/// decline-channel shape as the crate's `Ident::try_parse` leaf).
fn try_word<'inp, Ctx, Cmpl>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx, (), Cmpl>,
) -> Result<crate::try_parse_input::ParseAttempt<PTok>, PErr>
where
  Ctx: crate::ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: crate::Emitter<'inp, Lex<'inp>, Error = PErr>,
  Cmpl: SurfaceIncomplete<'inp, Lex<'inp>, Ctx, ()>,
{
  Ok(
    inp
      .try_expect(|t| matches!(t.data(), PTok::Word))?
      .map(|s| s.into_data())
      .into(),
  )
}

#[test]
fn lego_chain_runs_both_modes_to_equivalence() {
  // Complete drive of the assembled chain.
  let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
  let complete: std::vec::Vec<PKind> = Parser::with_context(ctx)
    .apply(lego_chain)
    .parse_str("foo bar baz")
    .expect("the Complete drive of the chain succeeds");
  assert_eq!(complete, std::vec![PKind::Word; 3]);

  // Partial-chunked drive of the SAME chain fn, over the probe's cut shape and then a
  // full deterministic sweep.
  let mut buffer = std::string::String::new();
  let mut incompletes = 0;
  let chunks = ["foo b", "ar ", "baz"];
  let mut parsed = None;
  for (i, chunk) in chunks.iter().enumerate() {
    buffer.push_str(chunk);
    let is_final = i + 1 == chunks.len();
    let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
    match parse_partial(ctx, buffer.as_str(), (), is_final, lego_chain) {
      Ok(kinds) => {
        parsed = Some(kinds);
        break;
      }
      Err(e) if e.is_incomplete() => incompletes += 1,
      Err(e) => panic!("a real parse error: {e:?}"),
    }
  }
  assert_eq!(
    parsed.as_ref(),
    Some(&complete),
    "one chain, two modes, one answer"
  );
  assert_eq!(incompletes, 2);

  let corpus = "foo bar baz qux";
  for cut in 1..corpus.len() {
    let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
    let round1 = parse_partial(ctx, &corpus[..cut], (), false, lego_chain);
    assert!(
      matches!(&round1, Err(e) if e.is_incomplete()),
      "a non-final prefix of a drain-all chain is Incomplete at cut {cut}"
    );
    let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
    let sealed =
      parse_partial(ctx, corpus, (), true, lego_chain).expect("the sealed drive completes");
    assert_eq!(
      sealed,
      std::vec![PKind::Word; 4],
      "chunked equivalence at cut {cut}"
    );
  }
}

#[test]
fn lego_chain_shape_at_a_partial_drive_is_annotation_free() {
  // The T5 pin at crate level: the SAME chain shape spelled inline against a CONCRETE
  // `Partial` handle — no `Cmpl` annotation and no turbofish anywhere in the chain.
  use crate::Accumulator as _;
  fn drive<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx, (), Partial>,
  ) -> Result<std::vec::Vec<PKind>, PErr>
  where
    Ctx: crate::ParseContext<'inp, Lex<'inp>>,
    Ctx::Emitter: crate::Emitter<'inp, Lex<'inp>, Error = PErr>
      + crate::emitter::FullContainerEmitter<'inp, Lex<'inp>>,
  {
    try_word
      .repeated()
      .collect()
      .map(|words: std::vec::Vec<PTok>| words.into_iter().map(|t| t.kind()).collect())
      .parse_input(inp)
  }
  let ctx: PartialCtx<'_> = (Verbose::new(), DefaultCache::<'_, Lex<'_>>::default());
  let kinds =
    parse_partial(ctx, "foo bar", (), true, drive).expect("the sealed partial drive completes");
  assert_eq!(kinds, std::vec![PKind::Word; 2]);
}

// ── The never-recoverable gate through the resilient collections ─────────────────────

/// An element that CONSUMES a word and then requires a number: a missing number is a
/// plain `Lex` error (the resilient arm's food), while a frontier-cut word surfaces
/// `Incomplete` out of the scan (the gate's food).
fn word_then_num<'inp, Ctx, Cmpl>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx, (), Cmpl>,
) -> Result<crate::try_parse_input::ParseAttempt<PKind>, PErr>
where
  Ctx: crate::ParseContext<'inp, Lex<'inp>>,
  Ctx::Emitter: crate::Emitter<'inp, Lex<'inp>, Error = PErr>,
  Cmpl: SurfaceIncomplete<'inp, Lex<'inp>, Ctx, ()>,
{
  use crate::try_parse_input::{Accept, Decline};
  match inp.try_expect(|t| matches!(t.data(), PTok::Word))? {
    None => Ok(Decline),
    Some(_) => match inp.try_expect(|t| matches!(t.data(), PTok::Num))? {
      Some(_) => Ok(Accept(PKind::Word)),
      None => Err(PErr::Lex),
    },
  }
}

#[test]
fn gate_propagates_frontier_incomplete_out_of_repeated() {
  use crate::Accumulator as _;
  // Non-final: element 1 completes mid-buffer; element 2's word is cut at the frontier.
  // The collection loop's resilient arm must NOT spend that `Incomplete` as a diagnostic
  // — the gate re-raises it, the loop stops, and the emitter log is untouched.
  let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
    "foo 1 ba",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let out: Result<std::vec::Vec<PKind>, PErr> = {
    let mut inp = input.as_ref(&mut emitter);
    word_then_num.repeated().collect().parse_input(&mut inp)
  };
  assert_eq!(
    out,
    Err(PErr::Incomplete(8)),
    "the frontier incomplete PROPAGATES"
  );
  let emitted: usize = emitter.errors().values().map(|g| g.len()).sum();
  assert_eq!(emitted, 0, "no emit-and-continue: the log is clean");
}

#[test]
fn gate_is_inert_when_final_and_matches_complete() {
  use crate::Accumulator as _;
  // The same input sealed: the missing number after "bar" is a genuine error and the
  // resilient arm emits-and-continues — byte-for-byte the Complete-mode outcome.
  fn drive_partial_final(src: &str) -> (Result<std::vec::Vec<PKind>, PErr>, usize) {
    let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
      src,
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    input.seal();
    let mut emitter = Verbose::<PErr>::new();
    let out = {
      let mut inp = input.as_ref(&mut emitter);
      word_then_num.repeated().collect().parse_input(&mut inp)
    };
    (out, emitter.errors().values().map(|g| g.len()).sum())
  }
  fn drive_complete(src: &str) -> (Result<std::vec::Vec<PKind>, PErr>, usize) {
    let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
      src,
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    let mut emitter = Verbose::<PErr>::new();
    let out = {
      let mut inp = input.as_ref(&mut emitter);
      word_then_num.repeated().collect().parse_input(&mut inp)
    };
    (out, emitter.errors().values().map(|g| g.len()).sum())
  }
  let sealed = drive_partial_final("foo 1 bar");
  let complete = drive_complete("foo 1 bar");
  assert_eq!(
    sealed, complete,
    "final-mode resilience is Complete-identical"
  );
  assert_eq!(
    sealed.0,
    Ok(std::vec![PKind::Word]),
    "one full element collected"
  );
  assert_eq!(
    sealed.1, 1,
    "the missing number was diagnosed resiliently, once"
  );
}

// ── The close-status probe: EOF vs a terminal stop, and no-consume ────────────────────
//
// `InputRef::probe_close` is the structural fix for the fold where a delimited driver's
// `try_expect`-based close classifier read `Ok(None)` as "no closer here" and emitted
// `Unclosed` — even when the `None` was really a terminal scanner stop (a limit trip or a
// latched poison boundary), which already carries its own diagnostic. The probe keeps
// `Eof` and `Tripped` apart, so a terminal stop never grows a spurious `Unclosed`; all
// four delimited drivers route their close classification through it.

#[test]
fn probe_close_at_genuine_eof_is_eof() {
  // A complete input drained to exhaustion: the probe reports genuine end of input as
  // `Eof` — the delimited drivers' one and only `Unclosed` path.
  let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
    "a b",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let mut inp = input.as_ref(&mut emitter);
  assert!(matches!(inp.next(), Ok(Some(_))));
  assert!(matches!(inp.next(), Ok(Some(_))));
  assert!(matches!(inp.next(), Ok(None)), "the input is exhausted");
  assert!(
    matches!(inp.probe_close(|_| false), Ok(CloseStatus::Eof)),
    "genuine end of input probes as `Eof`"
  );
}

#[test]
fn probe_close_at_a_terminal_trip_is_tripped_not_eof() {
  // "a b c" behind a 2-token limiter: the third word trips. After the two under-limit
  // tokens drain — the trip latches the poison boundary — `probe_close` must report a
  // terminal stop as `Tripped`, NOT `Eof`, which a delimited driver would otherwise grow
  // into a spurious `Unclosed`. The probe emits nothing itself: the trip's own limit
  // diagnostic stays the only one recorded.
  let mut input = Input::<LimLex<'_>, LimCtx<'_>, (), Partial>::with_state_and_cache(
    "a b c",
    LimitTracker::with_limit(2),
    DefaultCache::<'_, LimLex<'_>>::default(),
  );
  input.seal();
  let mut emitter = Verbose::<PErr>::new();
  let status = {
    let mut inp = input.as_ref(&mut emitter);
    assert!(matches!(inp.next(), Ok(Some(_))), "first under-limit word");
    assert!(matches!(inp.next(), Ok(Some(_))), "second under-limit word");
    assert!(
      matches!(inp.next(), Ok(None)),
      "the third word trips the limiter — a terminal stop, not more tokens"
    );
    inp
      .probe_close(|_| false)
      .expect("a recovering emitter never fails the probe")
  };
  assert!(
    matches!(status, CloseStatus::Tripped),
    "a terminal scanner stop must probe as `Tripped`, never `Eof`"
  );
  assert_eq!(
    limit_diags(&emitter),
    1,
    "only the limit trip's own diagnostic is recorded — the probe adds none"
  );
}

#[test]
fn probe_close_wrong_token_leaves_the_front_in_place() {
  // A rejecting probe is a peek: it classifies the front token as a wrong token but
  // never advances the committed cursor, so a follow-up `next()` still yields that token.
  let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
    "a",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  assert!(
    matches!(inp.probe_close(|_| false), Ok(CloseStatus::WrongToken(_))),
    "a rejected front token is `WrongToken`, left in place"
  );
  assert!(
    matches!(inp.next(), Ok(Some(_))),
    "WrongToken must not consume: the token is still there"
  );
  assert!(
    matches!(inp.next(), Ok(None)),
    "and then the input is exhausted"
  );
}

#[test]
fn probe_close_carries_the_closer_out_and_commit_probed_advances() {
  // An accepting probe takes the closer OUT of the input (carried in the `Close`
  // payload), and `commit_probed` settles it by value — advancing the cursor over it
  // with no re-scan. After the commit a follow-up `next()` sees the input exhausted: the
  // closer is not left behind for a re-lex (the blackhole-cache double-scan the fix
  // removes). This replaces the old "probe never advances / follow-up next() re-yields
  // the closer" contract, which the carry-out changes for the `Close` case.
  let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
    "a",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  let carried = match inp.probe_close(|_| true) {
    Ok(CloseStatus::Close(ct)) => ct,
    _ => panic!("an accepted front token must probe as `Close`"),
  };
  let committed = inp.commit_probed(carried);
  assert_eq!(
    committed.data().kind(),
    PKind::Word,
    "commit_probed returns the carried closer"
  );
  assert!(
    matches!(inp.next(), Ok(None)),
    "commit_probed advanced the cursor over the closer: the input is now exhausted"
  );
}

#[test]
fn commit_probed_lexes_the_closer_once_under_every_cache() {
  // The `separated_while` fallback: its delimited driver commits the closer via
  // the in-loop `try_expect` whenever it is at the cursor, so the `probe_close` Close arm —
  // the Shape-B site this fix changes — is not reachable through the driver on a valid list.
  // Pin the arm's *mechanism* directly instead, with a shared scan counter: `probe_close`
  // classifies the closer by lexing it ONCE, and `commit_probed` settles that carried token
  // by value with no re-lex, under the blackhole `()` cache and `DefaultCache` alike. (The
  // `sep` family reaches the identical Shape-B site through the driver — see
  // `tests/probe_close_no_rescan.rs`; pre-fix, `()` re-lexed the closer for a tally of 2.)

  // Blackhole `()` cache: the pre-fix push-back-then-`try_expect` re-lexed the closer.
  let tracker = LimitTracker::with_limit(usize::MAX);
  let scanned = tracker.counter();
  let mut input =
    Input::<LimLex<'_>, (Verbose<PErr>, ()), (), Complete>::with_state_and_cache("b", tracker, ());
  let mut emitter = Verbose::<PErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    let carried = match inp.probe_close(|_| true) {
      Ok(CloseStatus::Close(ct)) => ct,
      _ => panic!("the front word must probe as `Close`"),
    };
    let _ = inp.commit_probed(carried);
    assert!(
      matches!(inp.next(), Ok(None)),
      "commit_probed committed the closer: the input is exhausted"
    );
  }
  assert_eq!(
    scanned.get(),
    1,
    "blackhole `()`: the closer is lexed exactly once (pre-fix: 2)"
  );

  // DefaultCache twin: capacity-independent — also exactly once.
  let tracker = LimitTracker::with_limit(usize::MAX);
  let scanned = tracker.counter();
  let mut input = Input::<LimLex<'_>, LimCtx<'_>, (), Complete>::with_state_and_cache(
    "b",
    tracker,
    DefaultCache::<'_, LimLex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    let carried = match inp.probe_close(|_| true) {
      Ok(CloseStatus::Close(ct)) => ct,
      _ => panic!("the front word must probe as `Close`"),
    };
    let _ = inp.commit_probed(carried);
    assert!(
      matches!(inp.next(), Ok(None)),
      "commit_probed committed the closer: the input is exhausted"
    );
  }
  assert_eq!(
    scanned.get(),
    1,
    "DefaultCache: the closer is lexed exactly once"
  );

  // The law is "cache-independently, in any cache capacity", and the two cells above
  // covered only 0 and the default (U3). The #75 regression this pins was CAPACITY-dependent,
  // so the ends of the range are exactly where the evidence was missing: capacity 1, the
  // smallest cache that retains anything, and a capacity above the default.
  //
  // Additive pins, stated plainly: the property is believed to hold today (`ClosePayload::
  // Scanned` settles by value and `CacheFront` pops the front, neither of which reads the
  // capacity). The defect is missing evidence, and these are the evidence. A red cell here
  // would be a new finding.
  let tracker = LimitTracker::with_limit(usize::MAX);
  let scanned = tracker.counter();
  let mut input = Input::<
    LimLex<'_>,
    (
      Verbose<PErr>,
      Option<crate::cache::CachedTokenOf<'_, LimLex<'_>>>,
    ),
    (),
    Complete,
  >::with_state_and_cache("b", tracker, None);
  let mut emitter = Verbose::<PErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    let carried = match inp.probe_close(|_| true) {
      Ok(CloseStatus::Close(ct)) => ct,
      _ => panic!("the front word must probe as `Close`"),
    };
    let _ = inp.commit_probed(carried);
    assert!(
      matches!(inp.next(), Ok(None)),
      "commit_probed committed the closer: the input is exhausted"
    );
  }
  assert_eq!(
    scanned.get(),
    1,
    "capacity-1 `Option` cache: the closer is lexed exactly once"
  );

  let tracker = LimitTracker::with_limit(usize::MAX);
  let scanned = tracker.counter();
  let mut input = Input::<
    LimLex<'_>,
    (
      Verbose<PErr>,
      ::generic_arraydeque::GenericArrayDeque<
        crate::cache::CachedTokenOf<'_, LimLex<'_>>,
        ::generic_arraydeque::typenum::U8,
      >,
    ),
    (),
    Complete,
  >::with_state_and_cache("b", tracker, Default::default());
  let mut emitter = Verbose::<PErr>::new();
  {
    let mut inp = input.as_ref(&mut emitter);
    let carried = match inp.probe_close(|_| true) {
      Ok(CloseStatus::Close(ct)) => ct,
      _ => panic!("the front word must probe as `Close`"),
    };
    let _ = inp.commit_probed(carried);
    assert!(
      matches!(inp.next(), Ok(None)),
      "commit_probed committed the closer: the input is exhausted"
    );
  }
  assert_eq!(
    scanned.get(),
    1,
    "capacity-8 ring (above the default): the closer is lexed exactly once"
  );
}

#[test]
fn probe_close_cache_front_is_cursor_neutral_and_recovery_safe() {
  // Regression: the cache holds the closer PLUS a trailing lookahead token. On the
  // cache path `probe_close` must classify the closer at the FRONT by peek — NOT pop it — so
  // the probe stays cursor-neutral until the caller's real commit point. This matters for the
  // deferred (`separated`/`separated_while`) drivers, whose `handle_end` runs BETWEEN the probe
  // and the commit and spans the elements off `cursor()` (which reads the cache front).
  //
  // A word lexer stands in: "a b" caches the closer `a` (offset 0) plus the trailing `b`
  // (offset 2). Popping the closer eagerly at probe time would advance `cursor()` from 0 to 2
  // (over-including the closer in a `span_since`) and, if the caller errors before committing,
  // drop the popped closer while `b` survives (recovery would skip the closer).
  use generic_arraydeque::typenum::U2;

  // ── (a) cursor-neutral: probe_close must not advance over the cached closer ──
  {
    let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
      "a b",
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    let mut emitter = Verbose::<PErr>::new();
    let mut inp = input.as_ref(&mut emitter);
    // Fill the cache with BOTH tokens: [closer `a`, trailing `b`].
    assert_eq!(
      inp
        .peek::<U2>()
        .expect("a peek never surfaces Incomplete here")
        .len(),
      2,
      "the cache now holds [closer, trailing]"
    );
    let at_closer = *inp.cursor().as_inner();
    let payload = match inp.probe_close(|_| true) {
      Ok(CloseStatus::Close(p)) => p,
      _ => panic!("a cached front closer must probe as `Close`"),
    };
    assert!(
      matches!(payload, ClosePayload::AtFront { .. }),
      "the cache path classifies the closer by PEEK (AtFront), never popping it at probe time"
    );
    assert_eq!(
      inp.cursor().as_inner(),
      &at_closer,
      "probe_close(AtFront) must not advance cursor() — the closer stays at the front \
       (span_since(anchor), whose end IS cursor(), must not over-include the closer)"
    );
  }

  // ── (b) recovery-safe: an error before commit leaves the closer in the cache ──
  {
    let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
      "a b",
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    let mut emitter = Verbose::<PErr>::new();
    let mut inp = input.as_ref(&mut emitter);
    inp.peek::<U2>().expect("peek");
    // Classify the closer, then simulate the deferred driver erroring out of `handle_end`
    // before committing: the `Close` payload is discarded uncommitted (dropping it is a no-op —
    // on the cache path it holds no owned closer, only the `AtFront` marker). The closer must
    // still be at the cache front (not popped-and-dropped while the trailing token survives).
    match inp.probe_close(|_| true) {
      Ok(CloseStatus::Close(_)) => {}
      _ => panic!("cached front closer must probe as `Close`"),
    }
    let mut remaining = 0;
    while inp
      .next()
      .expect("recovering emitter never fails here")
      .is_some()
    {
      remaining += 1;
    }
    assert_eq!(
      remaining, 2,
      "closer retained for recovery: BOTH closer and trailing remain (pre-fix: 1 — the eager \
       pop dropped the closer and recovery skipped it)"
    );
  }

  // ── (c) commit-once: commit_probed pops+settles the cached closer exactly once ──
  {
    let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
      "a b",
      (),
      DefaultCache::<'_, Lex<'_>>::default(),
    );
    let mut emitter = Verbose::<PErr>::new();
    let mut inp = input.as_ref(&mut emitter);
    inp.peek::<U2>().expect("peek");
    let payload = match inp.probe_close(|_| true) {
      Ok(CloseStatus::Close(p)) => p,
      _ => panic!("cached front closer must probe as `Close`"),
    };
    let committed = inp.commit_probed(payload);
    assert_eq!(
      committed.data().kind(),
      PKind::Word,
      "commit_probed returns the closer"
    );
    let mut after = 0;
    while inp.next().expect("exhaust").is_some() {
      after += 1;
    }
    assert_eq!(
      after, 1,
      "the closer committed exactly once; only the trailing token remains"
    );
  }
}

// ═══════════════════════════════════════════════════════════════════════════════════
// A scanner's `Incomplete` exit must leave NO TRACE
//
// `skip_until`'s `Err` arm is written for fatals ("`settle_fatal` already committed the
// position"), which is false for `Incomplete`: that exit commits nothing, yet every token the
// scan already skipped has flowed to `commit_token` and (for the reporting modes) to
// `emit_unexpected_token`, and a rewinding mode's entry mark has been RELEASED as kept
// progress. Position at entry, effects standing: a refill-and-retry is not idempotent.
// ═══════════════════════════════════════════════════════════════════════════════════

/// Records the three observables an aborted attempt must not move: the diagnostics it emitted,
/// the tokens it settled through `commit_token`, and the emitter marks it left outstanding. The
/// mark is table-keyed (a live row per capture, remembering both log lengths), so a rewind
/// restores the logs and a leaked mark is visible — neither of which `Verbose` can show.
#[derive(Debug, Default)]
struct PartialJournal {
  emitted: std::vec::Vec<crate::span::SimpleSpan>,
  committed: std::vec::Vec<crate::span::SimpleSpan>,
  next: Cell<u64>,
  live: core::cell::RefCell<std::vec::Vec<(u64, usize, usize)>>,
}

impl PartialJournal {
  fn live_rows(&self) -> usize {
    self.live.borrow().len()
  }
}

impl<'inp, L, Lang: ?Sized> crate::Emitter<'inp, L, Lang> for PartialJournal
where
  L: crate::Lexer<'inp, Span = crate::span::SimpleSpan>,
  <L::Token as Token<'inp>>::Error: Into<PErr>,
{
  type Error = PErr;

  fn emit_lexer_error(
    &mut self,
    err: crate::span::Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), PErr> {
    self.emitted.push(*err.span_ref());
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    err: crate::error::token::UnexpectedTokenOf<'inp, L, Lang>,
  ) -> Result<(), PErr> {
    self.emitted.push(*err.span_ref());
    Ok(())
  }

  fn emit_error(&mut self, err: crate::span::Spanned<PErr, L::Span>) -> Result<(), PErr> {
    self.emitted.push(*err.span_ref());
    Ok(())
  }

  fn emit_skipped_region(&mut self, span: L::Span, _skipped: usize) -> Result<(), PErr> {
    self.emitted.push(span);
    Ok(())
  }

  fn commit_token(&mut self, _tok: &L::Token, span: &L::Span) {
    self.committed.push(*span);
  }

  fn checkpoint(&self) -> u64 {
    let id = self.next.get() + 1;
    self.next.set(id);
    self
      .live
      .borrow_mut()
      .push((id, self.emitted.len(), self.committed.len()));
    id
  }

  fn rewind(&mut self, _cursor: &crate::input::Cursor<'inp, '_, L>, checkpoint: u64) {
    let at = {
      let mut live = self.live.borrow_mut();
      let at = live
        .iter()
        .find(|(id, _, _)| *id == checkpoint)
        .map(|(_, e, c)| (*e, *c));
      live.retain(|(id, _, _)| *id < checkpoint);
      at
    };
    if let Some((e, c)) = at {
      self.emitted.truncate(e);
      self.committed.truncate(c);
    }
  }

  fn release(&mut self, checkpoint: u64) {
    let mut live = self.live.borrow_mut();
    if let Some(pos) = live.iter().rposition(|(id, _, _)| *id == checkpoint) {
      live.remove(pos);
    }
  }
}

type JournalCtx<'a> = (PartialJournal, DefaultCache<'a, Lex<'a>>);

#[test]
fn sync_to_incomplete_retry_is_trace_free() {
  // Non-final "foo bar 42": the number touches the buffer end, so the sync is not decidable
  // and the scan surfaces `Incomplete(10)`. Two aborted attempts must leave nothing behind;
  // the sealed third call then diagnoses its two skipped words exactly once.
  let mut input = Input::<Lex<'_>, PartialCtx<'_>, (), Partial>::with_state_and_cache(
    "foo bar 42",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();

  let r1 = {
    let mut inp = input.as_ref(&mut emitter);
    inp
      .sync_to(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.is_some())
  };
  let after_first: usize = emitter.errors().values().map(|g| g.len()).sum();

  let r2 = {
    let mut inp = input.as_ref(&mut emitter);
    inp
      .sync_to(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.is_some())
  };
  let after_two: usize = emitter.errors().values().map(|g| g.len()).sum();

  input.seal();
  let r3 = {
    let mut inp = input.as_ref(&mut emitter);
    inp
      .sync_to(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.is_some())
  };
  let after_success: usize = emitter.errors().values().map(|g| g.len()).sum();

  assert_eq!(
    r1,
    Err(PErr::Incomplete(10)),
    "the first attempt is undecided"
  );
  assert_eq!(r2, Err(PErr::Incomplete(10)), "so is the retry");
  assert_eq!(
    after_first, 0,
    "an aborted attempt leaves no diagnostic — an `Incomplete` exit leaves no trace"
  );
  assert_eq!(
    after_two, 0,
    "and neither does the retry: re-driving is idempotent"
  );
  assert_eq!(r3, Ok(true), "the sealed call completes the sync");
  assert_eq!(
    after_success, 2,
    "the completing sync keeps exactly its own two diagnoses, exactly once"
  );
}

#[test]
fn skip_while_incomplete_retry_settles_exactly_once() {
  // The settle-timeline twin: `commit_token` fires once per token a scan skips, so an aborted
  // attempt that keeps its settles breaks `Emitter::commit_token`'s exactly-once law across
  // retries. The journal emitter records the settles and rewinds them with the mark.
  let mut input = Input::<Lex<'_>, JournalCtx<'_>, (), Partial>::with_state_and_cache(
    "foo bar 42",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = PartialJournal::default();

  let r1 = {
    let mut inp = input.as_ref(&mut emitter);
    inp.skip_while(|t| matches!(t.data(), PTok::Word))
  };
  let after_first = emitter.committed.clone();

  let r2 = {
    let mut inp = input.as_ref(&mut emitter);
    inp.skip_while(|t| matches!(t.data(), PTok::Word))
  };
  let after_two = emitter.committed.clone();

  input.seal();
  let r3 = {
    let mut inp = input.as_ref(&mut emitter);
    inp.skip_while(|t| matches!(t.data(), PTok::Word))
  };
  let final_journal: std::vec::Vec<(usize, usize)> =
    emitter.committed.iter().map(|s| (s.start, s.end)).collect();

  assert_eq!(r1, Err(PErr::Incomplete(10)));
  assert_eq!(r2, Err(PErr::Incomplete(10)));
  assert!(
    after_first.is_empty(),
    "an aborted skip settles nothing: {after_first:?}"
  );
  assert!(
    after_two.is_empty(),
    "and the retry settles nothing either: {after_two:?}"
  );
  assert_eq!(r3, Ok(()));
  assert_eq!(
    final_journal,
    std::vec![(0, 3), (4, 7)],
    "the sealed completion settles each skipped token exactly once"
  );
  assert_eq!(
    emitter.live_rows(),
    0,
    "no attempt left an emitter mark outstanding"
  );
}

#[test]
fn sync_through_incomplete_then_refill_then_eof_leaves_no_trace() {
  // The POLICY ARBITER. `sync_through`'s no-match end of input rewinds the full
  // pre-call state, so a failed sync across a refill must leave the input exactly as it was.
  // Under commit-on-incomplete the first attempt's skipped prefix is committed at the
  // `Incomplete` exit, and the post-refill EOF rewind then restores only to the RESUMED
  // position — so the first attempt's skips and diagnostics survive. This cell rejects that.
  let mut input = Input::<Lex<'_>, JournalCtx<'_>, (), Partial>::with_state_and_cache(
    "foo bar",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = PartialJournal::default();

  let r1 = {
    let mut inp = input.as_ref(&mut emitter);
    inp
      .sync_through(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.is_some())
  };
  assert_eq!(
    r1,
    Err(PErr::Incomplete(7)),
    "the trailing word touches the buffer end: undecided"
  );

  input.seal();
  let (r2, cursor) = {
    let mut inp = input.as_ref(&mut emitter);
    let r = inp
      .sync_through(|t| matches!(t.data(), PTok::Num), || None)
      .map(|s| s.is_some());
    (r, *inp.cursor().as_inner())
  };

  assert_eq!(r2, Ok(false), "no number is ever found");
  assert_eq!(
    cursor, 0,
    "the failed sync rewound to the pre-call position"
  );
  assert!(
    emitter.emitted.is_empty(),
    "no diagnostic survives a failed sync across a refill: {:?}",
    emitter.emitted
  );
  assert!(
    emitter.committed.is_empty(),
    "no settle survives it either: {:?}",
    emitter.committed
  );
  assert_eq!(
    emitter.live_rows(),
    0,
    "both attempts settled their entry mark exactly once"
  );
}

/// **`commit_probed`'s temporal tripwire, and it is one cell with two payloads.**
///
/// A `ClosePayload` is valid only while the committed cursor sits where `probe_close` left it.
/// That contract lived in prose, and prose cannot fail: a future edit that consumed or rewound
/// between the probe and the commit would settle the wrong token, silently. The stamp makes it
/// checkable in debug.
///
/// Gated with the repo's own idiom rather than `#[cfg_attr(not(debug_assertions), ignore)]`: an
/// ignored cell is a release run with nothing executing, which is the vacuous shape this crate
/// refuses. Here the **same body** runs in both profiles and asserts the debug outcome (the
/// panic) or the release outcome (the misuse proceeds, and the state assertions say what
/// "proceeds" means).
///
/// Reachable test-side only because `ClosePayload` is crate-internal — this is an author-side
/// wiring contract, not an input condition, so there is no public route to it.
#[test]
#[cfg_attr(
  debug_assertions,
  should_panic(expected = "the committed cursor moved between `probe_close` and the commit")
)]
fn commit_probed_rejects_a_rewound_payload() {
  let mut input = Input::<Lex<'_>, CompleteCtx<'_>, (), Complete>::with_state_and_cache(
    "a b",
    (),
    DefaultCache::<'_, Lex<'_>>::default(),
  );
  let mut emitter = Verbose::<PErr>::new();
  let mut inp = input.as_ref(&mut emitter);

  use generic_arraydeque::typenum::U2;

  assert_eq!(
    inp
      .peek::<U2>()
      .expect("a peek never surfaces Incomplete here")
      .len(),
    2
  );

  let payload = match inp.probe_close(|_| true) {
    Ok(CloseStatus::Close(p)) => p,
    _ => panic!("a cached front closer must probe as `Close`"),
  };

  // The misuse: consume in the gap the payload's contract says must be cursor-neutral. The
  // committed cursor moves, so the payload now names a token that is no longer at the front.
  let _ = inp.next().expect("a token is available");

  let settled = inp.commit_probed(payload);

  // Release limb: no assert exists, so the misuse proceeds. Pin what it actually does, so the
  // release run carries a real payload rather than an absence. Both tokens lex as `Word`, so
  // the SPAN is what says which one was settled — the probed closer is `a` at 0..1.
  assert_eq!(
    (
      *settled.span_ref().start_ref(),
      *settled.span_ref().end_ref()
    ),
    (2, 3),
    "release: with the tripwire compiled out, the rewound payload settles whatever is at the \
     front NOW — `b` at 2..3, not the probed `a` at 0..1. That is the corruption the debug \
     assert exists to name."
  );
}
