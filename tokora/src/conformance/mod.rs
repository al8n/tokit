//! Conformance test kit for custom [`Lexer`] implementations.
//!
//! tokora's input machinery (prefix replay after a checkpoint restore, cache
//! truncation, re-lexing a rewound region on demand) relies on the properties written
//! down in the [`Lexer`] contract. This module ships [`Harness`](crate::conformance::Harness),
//! a small builder that
//! **drives a lexer against that contract** and panics — with precise context — on the
//! first violation. It is a test tool: assert-with-context is the intended failure
//! mode, so custom-lexer authors run it from their own `#[test]`s.
//!
//! # What it checks
//!
//! **Trait tier** — driving the [`Lexer`] surface directly:
//!
//! 1. **Replay identity** — two fresh `L::new(src)` runs produce the identical
//!    token/error + span + slice sequence, to exhaustion.
//! 2. **State-resume faithfulness** — for *every* position `k`, capturing the lexer
//!    [`State`] there and resuming with
//!    `L::with_state(src, saved)` + [`bump`](crate::Lexer::bump) to `k`'s offset
//!    reproduces the original suffix from `k`. This is the prefix-replay assumption
//!    verbatim (position is threaded via `bump`, not encoded in `State`).
//! 3. **Monotone progress** — span starts are non-decreasing, every item's span is
//!    nonempty (`start < end`), and the run terminates within a generous multiple of
//!    the source length (an anti-hang guard so the kit itself never spins).
//! 4. **Sticky exhaustion** — after [`lex`](crate::Lexer::lex) returns `None`, further
//!    calls keep returning `None`; and the **span survives that exhaustion** —
//!    [`span`](crate::Lexer::span) keeps answering, well-formed, with the lexer's final
//!    position (at least the last item's end, at most the source length) and answers the same
//!    on every later call. The input layer reads that value at two sites, so an unspecified one
//!    is not academic: a `to`-shaped scan commits there at end of input, and the partial-input
//!    frontier reports `Incomplete(offset)` from it — the offset a refill driver resumes at.
//! 5. **Span / slice coherence** — every item's [`slice`](crate::Lexer::slice) equals
//!    the source over its [`span`](crate::Lexer::span), and spans lie within bounds.
//! 6. **Gap-free tiling** (optional, [`lossless`](crate::conformance::Harness::lossless)) — consecutive
//!    spans abut, the first starts at `0`, and the last ends at the source end. Off by
//!    default, since a syntactic lexer legitimately skips trivia.
//! 7. **Read-frontier purity** — [`read_frontier`](crate::Lexer::read_frontier) answers the same
//!    on every call within an item's window, and asking does not move the lexer. The *value* it
//!    reports is checked by the partial tier below, not here; this checks only that it is a read
//!    rather than a scan.
//!
//! **Integration tier** — driving an `Input` session through the machinery itself over
//! a fixed set of named, deterministic save/peek/drain/restore schedules
//! (`peek-heavy`, `save-early-restore-late`, `drain-then-restore-across-cache`,
//! `nested-savepoints`) and requiring the committed token stream to equal the
//! straight-lex stream. No randomness — the schedules are enumerated.
//!
//! **Partial-input tier** (`run_partial`, a separate entry point for
//! `usize`-offset `str` / `[u8]` sources) — driving the input in
//! [`Partial`] mode over **every split point** and requiring
//! chunked-equivalence under the frontier rules: a non-final drain of each prefix yields a
//! **prefix of** the complete-parse items before the cut and always ends incomplete, while a
//! final drain of the whole source reproduces the complete parse **exactly**. Catches lexers that
//! are unfaithful under truncation (item identity depending on input beyond `(state, offset)`),
//! and it is what falsifies a wrong
//! [`read_frontier`](crate::Lexer::read_frontier) — including a wrong
//! [`READ_FRONTIER_CLASS`](crate::Token::READ_FRONTIER_CLASS) claim behind the logos
//! adapter.
//!
//! An **item** is a committed token *or* a lexer error the input layer raised, and both halves are
//! load-bearing: the layer lexes bytes and then either commits or refuses, and truncation can turn
//! one into the other. A tier that compared only tokens could not see the error arm at all — a
//! refusal leaves no token behind, so a lexer that errors on a truncated prefix and tokenizes the
//! full input showed up as "nothing yielded yet", which is a legal prefix. See
//! [`PartialRecorder`] for exactly what an error contributes to the comparison and what it
//! deliberately does not.
//!
//! The non-final leg asks for a *prefix*, not equality, because **over-withholding is sound**: a
//! lexer that reports reading past what it emits withholds more than the pre-0.10.0 span rule did,
//! and one that reports [`Unbounded`](crate::ReadFrontier::Unbounded) withholds everything until
//! the stream is sealed. Requiring equality would fail every conforming lookahead lexer. The final
//! leg is where full equality is pinned. That relaxation is also why the items have to carry
//! errors: once the length may legitimately be short, "withheld" and "reported and thrown away"
//! are the same observation, and only an arm the harness *records* can be told apart.
//!
//! # Violation posture
//!
//! A failing check is a bug in the *lexer* (or a mismatch with the documented
//! contract), surfaced loudly. The kit never mutates the lexer's behavior; it only
//! observes and asserts.

pub mod cache;
pub mod emitter;

#[cfg(all(
  test,
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"),
  feature = "std"
))]
mod cache_tests;

use std::{cell::RefCell, format, rc::Rc, string::String, vec, vec::Vec};

use crate::{
  Lexer, Slice, Source, Span, Token,
  cache::DefaultCache,
  emitter::{Emitter, Silent},
  error::{Incomplete, MaybeIncomplete, token::UnexpectedTokenOf},
  input::{Complete, Cursor, Input, InputRef, Partial},
  span::Spanned,
};

/// Default anti-hang budget: a run may not exceed `8 * source_len + 64` items.
const DEFAULT_BUDGET_MULTIPLE: usize = 8;
/// Floor added to the budget so a short source still has generous headroom.
const BUDGET_FLOOR: usize = 64;

/// A conformance harness that drives a [`Lexer`] implementation `L` against the lexer
/// contract.
///
/// Build one over one or more source inputs, set any knobs, then call [`run`](Self::run).
/// `run` panics on the first contract violation with the input index, position,
/// operation, and expected-vs-got values; on success it returns normally. See the
/// [module docs](crate::conformance) for the full list of checks.
///
/// # Example
///
/// ```
/// use core::convert::Infallible;
/// use tokora::{Lexer, SimpleSpan, Source, Token};
/// use tokora::conformance::Harness;
///
/// // A tiny hand-rolled lexer: one token per byte, gap-free over the source.
/// #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// struct CharKind;
/// impl core::fmt::Display for CharKind {
///   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
///     f.write_str("char")
///   }
/// }
///
/// #[derive(Clone, Debug)]
/// struct CharTok;
/// impl Token<'_> for CharTok {
///   type Kind = CharKind;
///   type Error = Infallible;
///   fn kind(&self) -> CharKind { CharKind }
///   fn is_trivia(&self) -> bool { false }
/// }
///
/// struct CharLexer<'a> { src: &'a str, start: usize, end: usize, state: () }
/// impl<'a> Lexer<'a> for CharLexer<'a> {
///   type State = ();
///   type Source = str;
///   type Token = CharTok;
///   type Span = SimpleSpan;
///   type Offset = usize;
///
///   fn new(src: &'a str) -> Self { Self { src, start: 0, end: 0, state: () } }
///   fn with_state(src: &'a str, state: ()) -> Self { Self { src, start: 0, end: 0, state } }
///   fn check(&self) -> Result<(), Infallible> { Ok(()) }
///   fn state(&self) -> &() { &self.state }
///   fn state_mut(&mut self) -> &mut () { &mut self.state }
///   fn into_state(self) -> () { self.state }
///   fn source(&self) -> &'a str { self.src }
///   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.start, self.end) }
///   fn slice(&self) -> &'a str { &self.src[self.start..self.end] }
///   fn lex(&mut self) -> Option<Result<CharTok, Infallible>> {
///     self.start = self.end;
///     if self.start >= self.src.len() { return None; }
///     let mut e = self.start + 1;
///     while e < self.src.len() && !self.src.is_char_boundary(e) { e += 1; }
///     self.end = e;
///     Some(Ok(CharTok))
///   }
///   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
///   fn bump(&mut self, n: &usize) { self.end += *n; }
/// }
///
/// // A gap-free per-byte lexer passes every check, including the lossless knob.
/// Harness::<CharLexer<'_>>::new("hello world").lossless().run();
/// ```
pub struct Harness<'inp, L>
where
  L: Lexer<'inp>,
{
  inputs: Vec<&'inp L::Source>,
  lossless: bool,
  budget_multiple: usize,
}

impl<'inp, L> Harness<'inp, L>
where
  L: Lexer<'inp>,
{
  /// Creates a harness over a single source input.
  #[must_use]
  pub fn new(input: &'inp L::Source) -> Self {
    Self {
      inputs: vec![input],
      lossless: false,
      budget_multiple: DEFAULT_BUDGET_MULTIPLE,
    }
  }

  /// Creates a harness over many source inputs.
  #[must_use]
  pub fn over<I>(inputs: I) -> Self
  where
    I: IntoIterator<Item = &'inp L::Source>,
  {
    Self {
      inputs: inputs.into_iter().collect(),
      lossless: false,
      budget_multiple: DEFAULT_BUDGET_MULTIPLE,
    }
  }

  /// Adds another source input to the corpus (builder style).
  #[must_use]
  pub fn and_input(mut self, input: &'inp L::Source) -> Self {
    self.inputs.push(input);
    self
  }

  /// Additionally requires **gap-free tiling**: consecutive spans abut (`end` equals
  /// the next `start`), the first span starts at `0`, and the last span ends at the
  /// source end. Off by default — a syntactic lexer that skips trivia legitimately
  /// leaves gaps, so only enable this for a lossless lexer.
  #[must_use]
  pub fn lossless(mut self) -> Self {
    self.lossless = true;
    self
  }

  /// Overrides the anti-hang budget multiple: a run may produce at most
  /// `multiple * source_len + 64` items before the kit declares the lexer
  /// non-terminating. The default is `8`. Values below `1` are clamped to `1`.
  #[must_use]
  pub fn budget_multiple(mut self, multiple: usize) -> Self {
    self.budget_multiple = multiple.max(1);
    self
  }

  /// Runs every check against every input, panicking on the first violation.
  ///
  /// # Panics
  ///
  /// Panics — with the offending input index, position, operation, and expected-vs-got
  /// values — the moment a contract check fails. Returns normally on full conformance.
  pub fn run(&self) {
    assert!(
      !self.inputs.is_empty(),
      "tokora conformance: Harness has no inputs; construct it with `new`/`over` over at least one source"
    );
    for (idx, &src) in self.inputs.iter().enumerate() {
      let budget = self.budget(src);

      // Trait tier: capture a reference run (this enforces monotone progress, nonempty
      // spans, span/slice coherence, in-bounds spans, and the anti-hang budget inline).
      let reference = lex_run::<L>(idx, src, budget);

      // 1. Replay identity: a second fresh run must match the reference exactly.
      let replay = lex_run::<L>(idx, src, budget);
      assert_run_eq::<L>(idx, "replay-identity", &reference, &replay);

      // 4. Sticky exhaustion, and the span that must survive it.
      check_sticky::<L>(idx, src, budget);
      check_span_after_exhaustion::<L>(idx, src, budget);

      // 2. State-resume faithfulness (every position k).
      check_resume::<L>(idx, src, &reference, budget);

      // 6. Gap-free tiling (opt-in).
      if self.lossless {
        check_lossless::<L>(idx, src, &reference);
      }

      // Integration tier: the input machinery over deterministic schedules.
      check_integration::<L>(idx, src, &reference, budget);
    }
  }

  /// The anti-hang budget for `src`: `budget_multiple * source_units + BUDGET_FLOOR`.
  fn budget(&self, src: &'inp L::Source) -> usize {
    let units = src.slice(..).map(|s| s.len()).unwrap_or(0);
    self
      .budget_multiple
      .saturating_mul(units)
      .saturating_add(BUDGET_FLOOR)
  }
}

impl<'inp, L> Harness<'inp, L>
where
  L: Lexer<'inp, Offset = usize>,
  L::State: Clone,
  // A prefix of the source is itself a `&L::Source` — true for the byte/`str` sources partial
  // parsing targets (`str[..k] : str`, `[u8][..k] : [u8]`). This is what lets the check feed the
  // lexer honest truncations without a growable source.
  L::Source: core::ops::Index<core::ops::RangeTo<usize>, Output = L::Source>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
{
  /// Runs the **partial-input (Sans-I/O) chunked-equivalence** check against every input.
  ///
  /// A separate entry point from [`run`](Self::run) because it needs a `usize`-offset,
  /// prefix-sliceable source (`str` / `[u8]`) and drives the input in
  /// [`Partial`] mode. For every split point `k` of each source it verifies that a **non-final**
  /// drain of the prefix `src[0..k]` yields a **prefix of** the complete-parse items lying
  /// strictly before `k` and always terminates as incomplete, while a **final** drain of the whole
  /// source reproduces the complete parse **exactly**. Together these are the chunked-equivalence
  /// guarantee: reassembling the chunk-by-chunk prefixes reproduces the single-shot parse.
  ///
  /// The sequence is **tokens and lexer errors interleaved**, not tokens alone: refusing a region
  /// is as much a decision about bytes as tokenizing it, and turning one into the other on append
  /// is precisely the unfaithfulness this tier exists to reject. See [`PartialRecorder`] for what
  /// an error contributes and what is deliberately left out of the comparison.
  ///
  /// This is where a lexer that is not faithful under truncation is caught — one whose item
  /// identity depends on input beyond what it reports having read (lookahead past a token that
  /// [`read_frontier`](crate::Lexer::read_frontier) does not admit to, or a
  /// [`with_state`](crate::Lexer::with_state) + [`bump`](crate::Lexer::bump) resume that does not
  /// reproduce the suffix) diverges from the complete prefix and trips this check.
  ///
  /// # Why the non-final leg asks for a prefix, not equality
  ///
  /// Because **over-withholding is sound and equality is not attainable**. A lexer that honestly
  /// reports reading past what it emits withholds more items than the pre-0.10.0 span rule did,
  /// and one that reports [`Unbounded`](crate::ReadFrontier::Unbounded) — the answer the bundled
  /// logos adapter gives by default — withholds every item until the stream is sealed. Both
  /// converge, because a refill strictly grows the buffer and sealing ends the game. Requiring
  /// equality would therefore reject every conforming lookahead lexer while testing precision
  /// rather than correctness. Yielding an item the complete parse does not have there, or a
  /// different one, still fails, and the final leg still pins full equality.
  ///
  /// # Panics
  ///
  /// Panics — tagged `partial-equivalence`, with the input index, split point, and expected-vs-got
  /// — on the first divergence. Returns normally on full conformance.
  pub fn run_partial(&self) {
    assert!(
      !self.inputs.is_empty(),
      "tokora conformance: Harness has no inputs; construct it with `new`/`over` over at least one source"
    );
    for (idx, &src) in self.inputs.iter().enumerate() {
      let budget = self.budget(src);
      check_partial::<L>(idx, src, budget);
    }
  }
}

/// The emitter error the partial check surfaces: the input layer builds it from the frontier
/// [`Incomplete`] (via [`SurfaceIncomplete`](crate::input::SurfaceIncomplete)), and it is the only
/// thing that reaches the `Err` channel — [`PartialRecorder`] never rejects a diagnostic. The
/// check never inspects the payload, only that `next` returned `Err` at the frontier.
enum PartialProbe {
  Incomplete,
}

impl<O> From<Incomplete<O>> for PartialProbe {
  #[inline(always)]
  fn from(_: Incomplete<O>) -> Self {
    PartialProbe::Incomplete
  }
}

impl MaybeIncomplete for PartialProbe {
  #[inline(always)]
  fn is_incomplete(&self) -> bool {
    true
  }
}

/// One item of the sequence the partial tier compares — the unit chunked equivalence is a
/// statement *about*.
///
/// A committed **token** and a **lexer error** are both items, because the input layer lexes bytes
/// and then either commits a token or refuses the region and reports it. Truncation can turn one
/// into the other, and a comparison over tokens alone cannot see that: a refusal leaves no trace
/// in the token stream at all. So the error is an item here, and the prefix property is asked of
/// the interleaved sequence.
enum PartialItem<'inp, L>
where
  L: Lexer<'inp>,
{
  /// A committed token: its kind and its span.
  Token(<L::Token as Token<'inp>>::Kind, L::Span),
  /// A lexer error the input layer raised over bytes it lexed and refused: its span, and the
  /// `Debug` rendering of the payload — see [`PartialRecorder`] for what that does and does not
  /// assert.
  LexerError(L::Span, String),
}

impl<'inp, L> Clone for PartialItem<'inp, L>
where
  L: Lexer<'inp>,
{
  fn clone(&self) -> Self {
    match self {
      Self::Token(kind, span) => Self::Token(*kind, span.clone()),
      Self::LexerError(span, sig) => Self::LexerError(span.clone(), sig.clone()),
    }
  }
}

impl<'inp, L> PartialItem<'inp, L>
where
  L: Lexer<'inp>,
{
  /// The item's span end — the offset the "strictly before the cut" filter reads.
  fn end(&self) -> &L::Offset {
    match self {
      Self::Token(_, span) | Self::LexerError(span, _) => span.end_ref(),
    }
  }

  /// Whether two items agree on discriminant, span, and kind-or-error-signature.
  fn sig_eq(&self, other: &Self) -> bool
  where
    <L::Token as Token<'inp>>::Kind: PartialEq,
  {
    match (self, other) {
      (Self::Token(a, sa), Self::Token(b, sb)) => a == b && sa == sb,
      (Self::LexerError(sa, a), Self::LexerError(sb, b)) => sa == sb && a == b,
      _ => false,
    }
  }

  /// A human-readable one-line rendering for panic context.
  fn describe(&self) -> String
  where
    <L::Token as Token<'inp>>::Kind: core::fmt::Debug,
  {
    match self {
      Self::Token(kind, span) => format!("token {kind:?}@{span:?}"),
      Self::LexerError(span, sig) => format!("lexer error {sig}@{span:?}"),
    }
  }
}

/// The shared, ordered log both partial-tier drains append to: [`PartialRecorder`] pushes each
/// lexer error the input layer reports, the drain loop pushes each token `next` hands back, and
/// because the layer reports an error during the very `next` call that goes on to find the
/// following token, the vector ends up being the interleaved item stream in production order.
type PartialLog<'inp, L> = Rc<RefCell<Vec<PartialItem<'inp, L>>>>;

/// The emitter the partial tier drives, and the reason it is not [`Silent`].
///
/// `Silent` discards every lexer error, which made the whole error arm invisible to chunked
/// equivalence: a lexer could refuse a region on a truncated buffer and tokenize the same region
/// once the missing bytes arrived, and the check saw an empty token list on one side and a token
/// on the other — a legal *prefix*, and green.
///
/// # What it compares, and what it deliberately does not
///
/// It records exactly three things per error: that the item **was** an error, its **span**, and
/// the **`Debug` rendering of the lexer's error payload**. That is the minimum that separates the
/// two divergences the token-only comparison could not see — an error that becomes a token at the
/// same span, and an error whose payload changes once the missing bytes arrive.
///
/// It deliberately records **nothing from the diagnostic channel**: no rendered message, no
/// [`Severity`](crate::emitter::Severity), no label stack, no `Diagnostic`. That is why this is a
/// purpose-built recorder and not [`Verbose`](crate::emitter::Verbose) — collecting diagnostics
/// would put presentation into a correctness check. Nor does it record the parser-facing doors
/// ([`emit_error`](Emitter::emit_error),
/// [`emit_unexpected_token`](Emitter::emit_unexpected_token)); the tier runs no parser, so the
/// only route into [`emit_lexer_error`](Emitter::emit_lexer_error) is the input layer's own
/// deduped refusal, arriving through the default
/// [`commit_lexer_error`](Emitter::commit_lexer_error) forward.
///
/// The `Debug` rendering is not an assertion on wording, and cannot become one. Both sides of
/// every comparison come from the **same build of the same lexer** inside one
/// [`run_partial`](Harness::run_partial) call — the complete parse is the oracle, never a recorded
/// string — so editing an error type's `Debug` moves `expected` and `got` together. The check reds
/// only when one build renders the same span differently for a prefix than for the full input,
/// which is the defect. `Debug` is also the only signature available: [`Token::Error`] is
/// `Clone + Debug` and nothing more, and it is already the comparison key the trait tier uses.
struct PartialRecorder<'inp, L>
where
  L: Lexer<'inp>,
{
  log: PartialLog<'inp, L>,
}

impl<'inp, L> Emitter<'inp, L> for PartialRecorder<'inp, L>
where
  L: Lexer<'inp>,
{
  type Error = PartialProbe;

  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), Self::Error> {
    let (span, payload) = err.into_components();
    self
      .log
      .borrow_mut()
      .push(PartialItem::LexerError(span, format!("{payload:?}")));
    Ok(())
  }

  fn emit_unexpected_token(&mut self, _: UnexpectedTokenOf<'inp, L>) -> Result<(), Self::Error> {
    Ok(())
  }

  fn emit_error(&mut self, _: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error> {
    Ok(())
  }

  /// A recording emitter owes the log the same rollback discipline a collecting one does, so the
  /// mark is the log's length and a rewind truncates to it. Both drains here are straight forward
  /// drains and never save or restore, so nothing calls this today; implementing it means a later
  /// schedule that does backtrack cannot silently accumulate phantom items.
  fn checkpoint(&mut self) -> u64 {
    self.log.borrow().len() as u64
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, L>, checkpoint: u64) {
    self.log.borrow_mut().truncate(checkpoint as usize);
  }
}

/// The partial-check context: a [`PartialRecorder`] over [`PartialProbe`] and the default cache.
type PartialConfCtx<'inp, L> = (PartialRecorder<'inp, L>, DefaultCache<'inp, L>);

/// Drives a partial input over `src` at `is_final`, returning the committed **item** stream —
/// tokens and lexer errors interleaved in production order — and whether the drain terminated
/// incomplete (rather than at genuine end of input).
fn partial_stream<'inp, L>(
  src: &'inp L::Source,
  is_final: bool,
  budget: usize,
  idx: usize,
) -> (Vec<PartialItem<'inp, L>>, bool)
where
  L: Lexer<'inp>,
  L::State: Clone,
{
  let log: PartialLog<'inp, L> = Rc::new(RefCell::new(Vec::new()));
  let context = crate::input::InputContext::new(
    PartialRecorder {
      log: Rc::clone(&log),
    },
    DefaultCache::<'inp, L>::default(),
  );
  let state = L::new(src).into_state();
  let mut input = Input::<'inp, L, PartialConfCtx<'inp, L>, (), Partial>::with_state_and_context(
    src, state, context,
  );
  // The driver states the world fact before any handle exists — the only place it can.
  if is_final {
    input.seal();
  }
  let mut ir = input.as_ref();
  let incomplete = loop {
    if log.borrow().len() > budget {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] a partial drain exceeded the budget of {budget} items (the lexer may not terminate or re-lexes without progress)"
      );
    }
    match ir.next() {
      Ok(Some(spanned)) => {
        let (span, tok) = spanned.into_components();
        log.borrow_mut().push(PartialItem::Token(tok.kind(), span));
      }
      Ok(None) => break false,
      Err(_) => break true,
    }
  };
  let out = core::mem::take(&mut *log.borrow_mut());
  (out, incomplete)
}

/// Drives a complete input over `src`, returning the committed item stream — the oracle a chunked
/// partial run is checked against.
fn complete_stream<'inp, L>(
  src: &'inp L::Source,
  budget: usize,
  idx: usize,
) -> Vec<PartialItem<'inp, L>>
where
  L: Lexer<'inp>,
  L::State: Clone,
{
  let log: PartialLog<'inp, L> = Rc::new(RefCell::new(Vec::new()));
  let context = crate::input::InputContext::new(
    PartialRecorder {
      log: Rc::clone(&log),
    },
    DefaultCache::<'inp, L>::default(),
  );
  let state = L::new(src).into_state();
  let mut input = Input::<'inp, L, PartialConfCtx<'inp, L>, (), Complete>::with_state_and_context(
    src, state, context,
  );
  let mut ir = input.as_ref();
  loop {
    if log.borrow().len() > budget {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] a complete drain exceeded the budget of {budget} items"
      );
    }
    match ir
      .next()
      .unwrap_or_else(|_| unreachable!("complete mode never surfaces Incomplete"))
    {
      Some(spanned) => {
        let (span, tok) = spanned.into_components();
        log.borrow_mut().push(PartialItem::Token(tok.kind(), span));
      }
      None => break,
    }
  }
  core::mem::take(&mut *log.borrow_mut())
}

/// The partial-equivalence check for one source: exhaustive over every split point.
fn check_partial<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize)
where
  L: Lexer<'inp, Offset = usize>,
  L::State: Clone,
  L::Source: core::ops::Index<core::ops::RangeTo<usize>, Output = L::Source>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
{
  let complete = complete_stream::<L>(src, budget, idx);

  // The "complete over the full input" leg: a final partial drain reproduces the complete parse.
  let (final_items, final_incomplete) = partial_stream::<L>(src, true, budget, idx);
  if final_incomplete {
    panic!(
      "tokora conformance [input #{idx} partial-equivalence] a FINAL partial drain surfaced Incomplete; a final input must reach genuine end of input like a complete parse"
    );
  }
  assert_partial_stream_eq::<L>(idx, usize::MAX, &complete, &final_items);

  let len = src.len();
  for k in 0..=len {
    if !src.is_boundary(k) {
      continue;
    }
    let prefix: &L::Source = &src[..k];
    let (prefix_items, incomplete) = partial_stream::<L>(prefix, false, budget, idx);

    // A non-final prefix yields a PREFIX of the complete ITEMS strictly before the cut. Equality
    // is not the property, and since 0.10.0 it is not even attainable: the holdback keys on the
    // lexer's reported read frontier, so a lexer that reads past what it emits withholds more than
    // the span rule did, and one that reports `Unbounded` withholds everything. Over-withholding
    // is sound — it converges, since a refill strictly grows the buffer and sealing ends the game
    // — so requiring equality here would fail every conforming lookahead lexer, and the check
    // would be a check on precision rather than on correctness.
    //
    // What is never sound, and still fails: an item the complete parse does not have before the
    // cut, or a different one at the same position. "Different" includes an error where the
    // complete parse has a token and an error whose payload moved, which is why the sequence
    // carries lexer errors and not only tokens: relaxing the LENGTH to a prefix is what makes
    // "withheld" indistinguishable from "reported and discarded", so an arm the harness discards
    // is an arm the relaxation makes invisible. Those are exactly what a lexer whose items change
    // under truncation produces, and they are what the *final* leg above pins in full.
    let expected: Vec<_> = complete
      .iter()
      .filter(|item| *item.end() < k)
      .cloned()
      .collect();
    assert_partial_prefix_of::<L>(idx, k, &expected, &prefix_items);

    if !incomplete {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] split k={k}: a non-final prefix drain reached genuine end of input; it must surface Incomplete (more input may arrive)"
      );
    }
  }
}

/// Asserts two committed item streams are identical, tagged `partial-equivalence`.
fn assert_partial_stream_eq<'inp, L>(
  idx: usize,
  k: usize,
  expected: &[PartialItem<'inp, L>],
  got: &[PartialItem<'inp, L>],
) where
  L: Lexer<'inp>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
{
  let n = expected.len().min(got.len());
  for i in 0..n {
    if !expected[i].sig_eq(&got[i]) {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] split k={k}, position {i}: prefix item diverges from the complete prefix: expected {}, got {}",
        expected[i].describe(),
        got[i].describe()
      );
    }
  }
  if expected.len() != got.len() {
    panic!(
      "tokora conformance [input #{idx} partial-equivalence] split k={k}: prefix item count diverges from the complete prefix: expected {}, got {}",
      expected.len(),
      got.len()
    );
  }
}

/// Asserts a non-final prefix drain is a **prefix of** the complete items before the cut: every
/// item it yielded matches — a token against a token, an error against the same error — and it
/// yielded no *extra* one. Withholding more is allowed; changing what an item **is** never was —
/// see [`check_partial`] for why that is the property rather than equality.
fn assert_partial_prefix_of<'inp, L>(
  idx: usize,
  k: usize,
  expected: &[PartialItem<'inp, L>],
  got: &[PartialItem<'inp, L>],
) where
  L: Lexer<'inp>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
{
  let n = expected.len().min(got.len());
  for i in 0..n {
    if !expected[i].sig_eq(&got[i]) {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] split k={k}, position {i}: prefix item diverges from the complete prefix: expected {}, got {}. An item that is an ERROR on the truncated buffer and a TOKEN on the full one — or an error whose payload moved — was decided from bytes that had not arrived.",
        expected[i].describe(),
        got[i].describe()
      );
    }
  }
  if got.len() > expected.len() {
    panic!(
      "tokora conformance [input #{idx} partial-equivalence] split k={k}: a non-final prefix drain yielded {} items but the complete parse has only {} ending strictly before the cut. The extra one was decided from bytes that had not arrived; report a read frontier that reaches the buffer end (Lexer::read_frontier) so it is withheld.",
      got.len(),
      expected.len()
    );
  }
}

/// One observed lexer item, captured with everything the checks compare or resume from.
struct Item<'inp, L>
where
  L: Lexer<'inp>,
{
  /// `true` if the item was an [`Err`], `false` for a token.
  is_error: bool,
  /// The token kind (for a token) or `None` (for an error).
  kind: Option<<L::Token as Token<'inp>>::Kind>,
  /// The error's `Debug` rendering (for an error) or empty (for a token). `Token::Error`
  /// is only `Debug`, not `PartialEq`, so its `Debug` string is the comparison key.
  err_dbg: String,
  /// The item's span.
  span: L::Span,
  /// The item's slice.
  slice: <L::Source as Source<L::Offset>>::Slice<'inp>,
  /// The span end = the offset a resume from *after* this item bumps to.
  end: L::Offset,
  /// The lexer state observed right after this item was produced.
  state: L::State,
}

impl<'inp, L> Item<'inp, L>
where
  L: Lexer<'inp>,
{
  /// Whether two items agree on discriminant, kind/error, span, and slice.
  fn sig_eq(&self, other: &Self) -> bool {
    self.is_error == other.is_error
      && self.kind == other.kind
      && self.err_dbg == other.err_dbg
      && self.span == other.span
      && self.slice == other.slice
  }

  /// A human-readable one-line rendering for panic context.
  fn describe(&self) -> String {
    format!(
      "{{ error={}, kind={:?}, err={:?}, span={:?}, slice={:?} }}",
      self.is_error, self.kind, self.err_dbg, self.span, self.slice
    )
  }
}

/// Runs `L::new(src)` to exhaustion, enforcing the always-on trait-tier invariants
/// inline (monotone progress, nonempty + in-bounds spans, span/slice coherence, the
/// anti-hang budget) and returning the captured items.
fn lex_run<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize) -> Vec<Item<'inp, L>>
where
  L: Lexer<'inp>,
{
  let src_len = src.len();
  let mut lexer = L::new(src);
  let mut out: Vec<Item<'inp, L>> = Vec::new();
  let mut prev_start: Option<L::Offset> = None;

  loop {
    if out.len() > budget {
      panic!(
        "tokora conformance [input #{idx} monotone-progress] position {}: lex() produced more than the budget of {budget} items without exhausting; the lexer may not terminate",
        out.len()
      );
    }
    let Some(res) = lexer.lex() else { break };
    let span = lexer.span();
    let slice = lexer.slice();
    let state = lexer.state().clone();
    let start = span.start_ref().clone();
    let end = span.end_ref().clone();
    let pos = out.len();

    // 3. Nonempty span.
    if end <= start {
      panic!(
        "tokora conformance [input #{idx} monotone-progress] position {pos}: zero-width or reversed span {span:?}; every item must satisfy start < end"
      );
    }
    // 3. Monotone (non-decreasing) span starts.
    if let Some(ps) = &prev_start
      && start < *ps
    {
      panic!(
        "tokora conformance [input #{idx} monotone-progress] position {pos}: span start moved backward: previous start {ps:?}, this span {span:?}"
      );
    }
    prev_start = Some(start.clone());

    // 5. Spans within source bounds.
    if end > src_len {
      panic!(
        "tokora conformance [input #{idx} span/slice-coherence] position {pos}: span {span:?} ends past the source length {src_len:?}"
      );
    }
    // 5. slice() equals the source content at span().
    match src.slice(start.clone()..end.clone()) {
      Some(from_source) => {
        if from_source != slice {
          panic!(
            "tokora conformance [input #{idx} span/slice-coherence] position {pos}: slice() disagrees with the source at span {span:?}: source {from_source:?}, slice() {slice:?}"
          );
        }
      }
      None => panic!(
        "tokora conformance [input #{idx} span/slice-coherence] position {pos}: span {span:?} does not address a valid source slice"
      ),
    }

    // 7. The read frontier is a pure read of recorded fact: repeated calls agree, and asking does
    // not advance the lexer or probe new input. Only the after-an-item window is specified, which
    // is exactly where this asks — the same window `span()` and `slice()` answer in.
    let frontier = lexer.read_frontier();
    for probe in 0..3 {
      let again = lexer.read_frontier();
      if again != frontier {
        panic!(
          "tokora conformance [input #{idx} read-frontier] position {pos}: read_frontier() changed between calls: first read {frontier:?}, probe #{probe} read {again:?}. It must be a pure read of recorded fact — the input layer's contract lets it be called at most once per item, but nothing may depend on that."
        );
      }
    }
    if lexer.span() != span {
      panic!(
        "tokora conformance [input #{idx} read-frontier] position {pos}: read_frontier() moved the lexer: span() was {span:?} before the call and {:?} after. It must not advance the lexer or probe new input.",
        lexer.span()
      );
    }

    let (is_error, kind, err_dbg) = match res {
      Ok(tok) => (false, Some(tok.kind()), String::new()),
      Err(err) => (true, None, format!("{err:?}")),
    };
    out.push(Item {
      is_error,
      kind,
      err_dbg,
      span,
      slice,
      end,
      state,
    });
  }

  out
}

/// Asserts two captured runs are item-for-item identical (check 1 / used by resume).
fn assert_run_eq<'inp, L>(idx: usize, op: &str, expected: &[Item<'inp, L>], got: &[Item<'inp, L>])
where
  L: Lexer<'inp>,
{
  let n = expected.len().min(got.len());
  for i in 0..n {
    if !expected[i].sig_eq(&got[i]) {
      panic!(
        "tokora conformance [input #{idx} {op}] position {i}: item mismatch: expected {}, got {}",
        expected[i].describe(),
        got[i].describe()
      );
    }
  }
  if expected.len() != got.len() {
    panic!(
      "tokora conformance [input #{idx} {op}] length mismatch: expected {} items, got {}",
      expected.len(),
      got.len()
    );
  }
}

/// Check 4: after the first `None`, `lex()` must keep returning `None`.
fn check_sticky<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize)
where
  L: Lexer<'inp>,
{
  let mut lexer = L::new(src);
  let mut n = 0usize;
  while lexer.lex().is_some() {
    n += 1;
    if n > budget {
      panic!(
        "tokora conformance [input #{idx} sticky-exhaustion] lex() produced more than the budget of {budget} items without exhausting"
      );
    }
  }
  for probe in 0..4 {
    if lexer.lex().is_some() {
      panic!(
        "tokora conformance [input #{idx} sticky-exhaustion] position {n}: lex() returned Some on probe #{probe} after returning None; exhaustion must be sticky"
      );
    }
  }
}

/// Check 4b: the span survives exhaustion — well-formed, ending at the lexer's final position
/// within `[last item end, source len]`, and stable across repeated calls.
///
/// This is the clause's executable form. The clause was written because the input layer grew a
/// *second* reader of the value (the partial-input frontier, after the `to`-shaped
/// end-of-input commit), and the first fixture that reader met left `start > end`.
fn check_span_after_exhaustion<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize)
where
  L: Lexer<'inp>,
{
  let src_len = src.len();
  let mut lexer = L::new(src);
  let mut last_end: Option<L::Offset> = None;
  let mut n = 0usize;
  while lexer.lex().is_some() {
    n += 1;
    if n > budget {
      panic!(
        "tokora conformance [input #{idx} span-survives-exhaustion] lex() produced more than the budget of {budget} items without exhausting"
      );
    }
    last_end = Some(lexer.span().end());
  }

  let span = lexer.span();
  let start = span.start_ref().clone();
  let end = span.end_ref().clone();

  if end < start {
    panic!(
      "tokora conformance [input #{idx} span-survives-exhaustion] after exhaustion span() is malformed: {span:?} has start > end. It must stay well-formed — the partial-input frontier reports this offset to a refill driver."
    );
  }
  if let Some(item_end) = &last_end
    && end < *item_end
  {
    panic!(
      "tokora conformance [input #{idx} span-survives-exhaustion] after exhaustion span() ends at {end:?}, before the last item's end {item_end:?}. The final position can only be at or past the last item the lexer yielded; a retracting span hands a refill driver an offset it has already consumed."
    );
  }
  if end > src_len {
    panic!(
      "tokora conformance [input #{idx} span-survives-exhaustion] after exhaustion span() ends at {end:?}, past the source length {src_len:?}. The final position is bounded by the source; an over-reporting span hands a refill driver an offset past its own buffer."
    );
  }
  for probe in 0..3 {
    let again = lexer.span();
    if again != span {
      panic!(
        "tokora conformance [input #{idx} span-survives-exhaustion] span() changed after exhaustion: first read {span:?}, probe #{probe} read {again:?}. Repeated reads must agree — the input layer reads it more than once."
      );
    }
  }
}

/// Check 2: for every position `k`, resuming from the captured (state, offset) pair via
/// `with_state` + `bump` reproduces the original suffix from `k`.
fn check_resume<'inp, L>(
  idx: usize,
  src: &'inp L::Source,
  reference: &[Item<'inp, L>],
  budget: usize,
) where
  L: Lexer<'inp>,
{
  for k in 0..=reference.len() {
    // The resume point before item k: for k == 0 the initial state at offset 0, else
    // the state captured right after item k-1, at that item's span end.
    let (state, offset) = if k == 0 {
      (L::new(src).into_state(), L::Offset::default())
    } else {
      (reference[k - 1].state.clone(), reference[k - 1].end.clone())
    };

    let mut resumed = L::with_state(src, state);
    resumed.bump(&offset);

    let mut produced = 0usize;
    loop {
      if produced > budget {
        panic!(
          "tokora conformance [input #{idx} state-resume] resume-from k={k}: produced more than the budget of {budget} items without exhausting"
        );
      }
      let Some(res) = resumed.lex() else { break };
      let span = resumed.span();
      let slice = resumed.slice();
      let (is_error, kind, err_dbg) = match res {
        Ok(tok) => (false, Some(tok.kind()), String::new()),
        Err(err) => (true, None, format!("{err:?}")),
      };
      let end = span.end_ref().clone();
      let observed = Item::<L> {
        is_error,
        kind,
        err_dbg,
        span,
        slice,
        end,
        state: resumed.state().clone(),
      };

      match reference.get(k + produced) {
        Some(expected) => {
          if !expected.sig_eq(&observed) {
            panic!(
              "tokora conformance [input #{idx} state-resume] resume-from k={k}, position {produced}: resumed item diverges from the original suffix: expected {}, got {}",
              expected.describe(),
              observed.describe()
            );
          }
        }
        None => panic!(
          "tokora conformance [input #{idx} state-resume] resume-from k={k}, position {produced}: resume produced MORE items than the original suffix ({} remaining)",
          reference.len() - k
        ),
      }
      produced += 1;
    }

    if k + produced != reference.len() {
      panic!(
        "tokora conformance [input #{idx} state-resume] resume-from k={k}: resume produced FEWER items than the original suffix: expected {}, got {produced}",
        reference.len() - k
      );
    }
  }
}

/// Check 6: gap-free tiling — first span starts at 0, consecutive spans abut, the last
/// ends at the source end.
fn check_lossless<'inp, L>(idx: usize, src: &'inp L::Source, reference: &[Item<'inp, L>])
where
  L: Lexer<'inp>,
{
  let src_len = src.len();
  let zero = L::Offset::default();

  let Some(first) = reference.first() else {
    // An empty stream is gap-free tiling of an empty source; a non-empty source with
    // no tokens leaves the whole thing untiled.
    if src_len != zero {
      panic!(
        "tokora conformance [input #{idx} lossless] the lexer produced no items but the source is non-empty (length {src_len:?}); lossless tiling requires covering the whole source"
      );
    }
    return;
  };

  if *first.span.start_ref() != zero {
    panic!(
      "tokora conformance [input #{idx} lossless] position 0: first span {:?} does not start at 0",
      first.span
    );
  }

  let mut prev_end = first.span.end_ref().clone();
  for (i, item) in reference.iter().enumerate().skip(1) {
    let start = item.span.start_ref().clone();
    if start != prev_end {
      panic!(
        "tokora conformance [input #{idx} lossless] position {i}: gap or overlap — previous span ended at {prev_end:?} but this span {:?} starts at {start:?}",
        item.span
      );
    }
    prev_end = item.span.end_ref().clone();
  }

  if prev_end != src_len {
    panic!(
      "tokora conformance [input #{idx} lossless] the last span ends at {prev_end:?} but the source ends at {src_len:?}; lossless tiling must reach the source end"
    );
  }
}

/// The parse context the integration tier drives the input machinery under: a
/// [`Silent`] emitter (so a lexer error never aborts a run) over the default cache.
type ConfCtx<'inp, L> = (
  Silent<<<L as Lexer<'inp>>::Token as Token<'inp>>::Error>,
  DefaultCache<'inp, L>,
);

/// Builds a fresh `Input` session over `src` and hands its [`InputRef`] to `f`.
fn drive<'inp, L, R>(
  src: &'inp L::Source,
  f: impl FnOnce(&mut InputRef<'inp, '_, L, ConfCtx<'inp, L>, ()>) -> R,
) -> R
where
  L: Lexer<'inp>,
{
  let context = crate::input::InputContext::new(
    Silent::<<L::Token as Token<'inp>>::Error>::new(),
    DefaultCache::<'inp, L>::default(),
  );
  let state = L::new(src).into_state();
  let mut input =
    Input::<'inp, L, ConfCtx<'inp, L>, ()>::with_state_and_context(src, state, context);
  let mut input_ref = input.as_ref();
  f(&mut input_ref)
}

/// Drives `next()` to exhaustion, collecting the committed (kind, span) token stream.
fn drain_all<'inp, L>(
  input_ref: &mut InputRef<'inp, '_, L, ConfCtx<'inp, L>, ()>,
  budget: usize,
) -> Vec<(<L::Token as Token<'inp>>::Kind, L::Span)>
where
  L: Lexer<'inp>,
{
  let mut out = Vec::new();
  loop {
    if out.len() > budget {
      panic!("tokora conformance integration: next() exceeded the budget of {budget} tokens");
    }
    match input_ref
      .next()
      .expect("the conformance kit's Silent emitter never returns Err")
    {
      Some(spanned) => {
        let (span, tok) = spanned.into_components();
        out.push((tok.kind(), span));
      }
      None => break,
    }
  }
  out
}

/// Integration tier: the committed stream from each named schedule must equal the
/// straight-lex stream, and the straight stream must equal the raw-lex tokens.
fn check_integration<'inp, L>(
  idx: usize,
  src: &'inp L::Source,
  reference: &[Item<'inp, L>],
  budget: usize,
) where
  L: Lexer<'inp>,
{
  use generic_arraydeque::typenum::U3;

  // The straight-lex reference: `next()` to exhaustion, no backtracking.
  let straight = drive::<L, _>(src, |ir| drain_all::<L>(ir, budget));

  // Cross-check: the input layer's `next()` stream must equal the raw-lex tokens
  // (errors are skipped by `next()`, so filter the reference to its Ok items).
  let raw_tokens: Vec<(<L::Token as Token<'inp>>::Kind, L::Span)> = reference
    .iter()
    .filter_map(|it| it.kind.map(|k| (k, it.span.clone())))
    .collect();
  assert_stream_eq::<L>(idx, "raw-lex-vs-next", &raw_tokens, &straight);

  // peek-heavy: fill the cache before every consume; the drain path re-serves cached
  // tokens and re-lexes past the window.
  let peek_heavy = drive::<L, _>(src, |ir| {
    let mut out = Vec::new();
    loop {
      if out.len() > budget {
        panic!("tokora conformance [input #{idx} integration/peek-heavy] exceeded budget");
      }
      let _ = ir
        .peek::<U3>()
        .expect("the conformance kit's Silent emitter never returns Err");
      match ir
        .next()
        .expect("the conformance kit's Silent emitter never returns Err")
      {
        Some(spanned) => {
          let (span, tok) = spanned.into_components();
          out.push((tok.kind(), span));
        }
        None => break,
      }
    }
    out
  });
  assert_stream_eq::<L>(idx, "peek-heavy", &straight, &peek_heavy);

  // save-early-restore-late: save at 0, consume a prefix, abandon it, then drain the
  // whole stream — which must re-lex the rewound prefix identically.
  let save_early = drive::<L, _>(src, |ir| {
    let ckp = ir.save();
    for _ in 0..3 {
      if ir
        .next()
        .expect("the conformance kit's Silent emitter never returns Err")
        .is_none()
      {
        break;
      }
    }
    ir.restore(ckp);
    drain_all::<L>(ir, budget)
  });
  assert_stream_eq::<L>(idx, "save-early-restore-late", &straight, &save_early);

  // drain-then-restore-across-cache: fill the cache, drain it and lex past it, then
  // restore to a save that predates the cache — the post-save cache is dropped and the
  // region re-lexes on demand.
  let across_cache = drive::<L, _>(src, |ir| {
    let ckp = ir.save();
    let _ = ir
      .peek::<U3>()
      .expect("the conformance kit's Silent emitter never returns Err");
    for _ in 0..4 {
      if ir
        .next()
        .expect("the conformance kit's Silent emitter never returns Err")
        .is_none()
      {
        break;
      }
    }
    ir.restore(ckp);
    drain_all::<L>(ir, budget)
  });
  assert_stream_eq::<L>(
    idx,
    "drain-then-restore-across-cache",
    &straight,
    &across_cache,
  );

  // nested-savepoints: outer save, consume, inner save, consume, restore inner (LIFO),
  // consume, restore outer, drain. Exercises nested last-in-first-out restores.
  let nested = drive::<L, _>(src, |ir| {
    let outer = ir.save();
    let _ = ir
      .next()
      .expect("the conformance kit's Silent emitter never returns Err");
    let inner = ir.save();
    let _ = ir
      .next()
      .expect("the conformance kit's Silent emitter never returns Err");
    ir.restore(inner);
    let _ = ir
      .next()
      .expect("the conformance kit's Silent emitter never returns Err");
    ir.restore(outer);
    drain_all::<L>(ir, budget)
  });
  assert_stream_eq::<L>(idx, "nested-savepoints", &straight, &nested);
}

/// Asserts two committed (kind, span) token streams are identical, with position context.
fn assert_stream_eq<'inp, L>(
  idx: usize,
  sched: &str,
  expected: &[(<L::Token as Token<'inp>>::Kind, L::Span)],
  got: &[(<L::Token as Token<'inp>>::Kind, L::Span)],
) where
  L: Lexer<'inp>,
{
  let n = expected.len().min(got.len());
  for i in 0..n {
    if expected[i] != got[i] {
      panic!(
        "tokora conformance [input #{idx} integration/{sched}] position {i}: committed token stream diverges: expected {:?}, got {:?}",
        expected[i], got[i]
      );
    }
  }
  if expected.len() != got.len() {
    panic!(
      "tokora conformance [input #{idx} integration/{sched}] committed token stream length differs: straight-lex has {}, schedule has {}",
      expected.len(),
      got.len()
    );
  }
}

#[cfg(test)]
mod tests;
