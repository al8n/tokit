#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]

//! What one level of grammar nesting costs on the native stack, measured two ways over the
//! **same grammar** in the **same binary** at the **same optimisation level**.
//!
//! `recursion_tracker`'s per-level table is measured against the pratt driver. This measures
//! the other recursive shape a grammar can have: a repetition combinator whose element parser
//! re-enters the same production. The two doors differ only in how the repetition is written —
//!
//! - `combinator_node` goes through `Collect<DelimitedBy<RepeatedWhile<..>, Bracket>, Vec<_>>`,
//! - `hand_written_body` writes the loop out with `InputRef::next`,
//!
//! — so the difference between their per-level costs is what the combinator layer costs, with
//! the lexer, the cache, the emitter and the token type held fixed.
//!
//! The measurement is a stack-pointer probe: each level records the address of one of its own
//! locals, and the per-level cost is the difference between consecutive levels. That reads the
//! whole frame chain of a level, not one function's prologue, which is the number that decides
//! how deep a document can nest before the native stack aborts the process.
//!
//! No THRESHOLD is asserted on the numbers — they move with the toolchain, the target and
//! `-Cdebuginfo`, and a bound on them would be a flake rather than a contract. What *is*
//! asserted is that the samples can bear the reading at all: see [`per_level`], which refuses
//! rather than reports whenever the address sequence is not a native frame chain. A measurement
//! harness that cannot fail is not a measurement; the first version of this file differenced
//! with `saturating_sub`, which turns every layout it does not model into a printed `0 B`.
//! Run with `--nocapture` to see the numbers.

mod common;

use core::cell::RefCell;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, EmitterView, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser,
  ParserContext, Token as TokenTrait,
  cache::Peeked,
  emitter::{FromUnclosed, FullContainerEmitter, UnclosedEmitter},
  error::{
    Unclosed, UnexpectedEot,
    syntax::FullContainer,
    token::{UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  parser::Action,
  span::Spanned,
};

use common::{TestLexer, Token};

thread_local! {
  /// One stack-pointer sample per nesting level, innermost last.
  static MARKS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Records the address of a local of the CALLING frame, which is what makes the samples
/// measure the caller's stack position rather than this helper's.
fn mark(anchor: &u8) {
  MARKS.with(|m| m.borrow_mut().push(anchor as *const u8 as usize));
}

fn take_marks() -> Vec<usize> {
  MARKS.with(|m| core::mem::take(&mut *m.borrow_mut()))
}

/// This build's reason that the address of a local is **not** a position on a native stack, if
/// it has one.
///
/// Two instrumentations break the reading, and neither can be tuned around:
///
/// * **Miri** interprets the program, so a frame's locals are a virtual allocation rather than
///   an offset into any real stack. `cfg!(miri)` is set for the interpreted build and is the
///   stable spelling.
/// * **A sanitizer** may relocate a frame's locals onto a heap-allocated *fake stack* — ASan's
///   `detect_stack_use_after_return` machinery. There is no stable `cfg` to test:
///   `cfg(sanitize = "address")` needs `feature(cfg_sanitize)`, which a stable-compiled
///   integration test cannot carry. So the leg announces itself instead — `ci/sanitizer.sh`
///   exports `TOKORA_SANITIZER` for every sanitizer it runs and its command is
///   `cargo test --tests --all-features`, which SELECTS this file. Read at run time rather than
///   through `option_env!`, so a cached build cannot carry a stale answer either way.
///   `tests/pratt_limit_unit_sink.rs` gates its own address witness on the same variable.
///
/// This matters more here than a `cfg` would suggest. [`per_level`]'s own checks cannot tell a
/// fake stack from a real one: allocations handed out monotonically with a constant stride pass
/// the ordering check and the nonzero check, and the harness would then print allocation
/// spacing as bytes per nesting level. A probe that prints a plausible number in an environment
/// it cannot measure is the defect this probe was added to remove, one level up.
///
/// The argument is the environment's value rather than a read inside the body, so the refusal
/// path has a positive control that does not have to mutate the process environment.
fn frames_are_relocated(sanitizer: Option<&str>) -> Option<String> {
  if cfg!(miri) {
    return Some(String::from(
      "miri: frame locals are separate virtual allocations, not offsets into a contiguous \
       native stack, so their differences are allocation spacing",
    ));
  }
  match sanitizer {
    Some(san) if !san.is_empty() => Some(format!(
      "TOKORA_SANITIZER={san}: a sanitizer-instrumented build may relocate a frame's locals onto \
       a heap-backed fake stack, so the address of a local is not where its frame sits"
    )),
    _ => None,
  }
}

/// [`frames_are_relocated`] against the real environment.
fn relocation_reason() -> Option<String> {
  frames_are_relocated(std::env::var("TOKORA_SANITIZER").ok().as_deref())
}

/// Says — on the process's **real** stderr — that no figure was produced and why.
///
/// Not `eprintln!`: libtest captures a passing test's output, so the one run where the
/// measurement is skipped would be the one run where nobody can see that it was.
fn announce_skipped(why: &str) {
  use std::io::Write as _;
  let mut err = std::io::stderr().lock();
  let _ = writeln!(err, "stack_per_level: NO MEASUREMENT TAKEN — {why}");
}

/// The per-level cost, taken as the median of the consecutive differences so that one
/// irregular level (the outermost, the innermost) does not set the number.
///
/// # What this refuses to answer
///
/// Differencing two addresses is only a frame size on a **contiguous native stack**, and
/// nothing in the type system says the samples came from one. Each way they might not be is
/// rejected rather than reported, and the order of the checks is load-bearing:
///
/// - **The environment relocates frames.** [`frames_are_relocated`] first, before anything is
///   read out of the samples, because a fake stack can produce samples that pass every check
///   below.
/// - **A delta is zero.** Every level here is a real recursive call, so a zero means the
///   samples are not measuring what the caller thinks. This is checked BEFORE direction: under
///   the strict `>` / `<` this file used to carry, a repeated address failed the ordering check
///   first and the zero-delta assertion could not fire at all — a dedicated refusal path with
///   no reachable input is not a refusal path.
/// - **The addresses are not a chain at all.** Checked after, and non-strictly, so it rejects
///   exactly the shape the zero check does not: samples that go both ways. An interpreter that
///   gives each frame's locals its own virtual allocation produces addresses with no relation
///   to frame size; this catches the unordered case, and the refusal above catches the ordered
///   one.
///
/// [`usize::abs_diff`] rather than `saturating_sub` makes an upward-growing stack a *supported*
/// layout instead of a silent `0 B` for every level.
///
/// `per_level_with` takes the relocation reason as an argument so every refusal here has a
/// positive control in [`refusals`]; [`per_level`] is the same function against the real
/// environment.
fn per_level(marks: &[usize]) -> usize {
  per_level_with(relocation_reason(), marks)
}

fn per_level_with(relocated: Option<String>, marks: &[usize]) -> usize {
  assert!(
    relocated.is_none(),
    "refusing to read frame sizes out of these samples — {}",
    relocated.unwrap_or_default()
  );
  assert!(
    marks.len() >= 2,
    "need at least two levels to difference, got {}",
    marks.len()
  );

  let mut deltas: Vec<usize> = marks.windows(2).map(|w| w[0].abs_diff(w[1])).collect();
  assert!(
    deltas.iter().all(|&d| d != 0),
    "a nesting level cost zero bytes of stack, which no real call does; the samples are not a \
     frame chain: {marks:?}"
  );

  let descending = marks.windows(2).all(|w| w[0] >= w[1]);
  let ascending = marks.windows(2).all(|w| w[0] <= w[1]);
  assert!(
    descending || ascending,
    "stack samples are not monotone in one direction, so consecutive differences are not frame \
     sizes: {marks:?}"
  );

  deltas.sort_unstable();
  deltas[deltas.len() / 2]
}

// ── error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct NestError;

impl From<()> for NestError {
  fn from(_: ()) -> Self {
    NestError
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for NestError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    NestError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for NestError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    NestError
  }
}

impl From<UnexpectedEot> for NestError {
  fn from(_: UnexpectedEot) -> Self {
    NestError
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for NestError {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    NestError
  }
}

impl<'inp, L, Lang: ?Sized> FromUnclosed<'inp, L, Lang> for NestError
where
  L: Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    NestError
  }
}

// ── emitter ───────────────────────────────────────────────────────────────────

struct NestEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for NestEmitter {
  type Error = NestError;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), NestError> {
    Err(NestError)
  }

  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), NestError> {
    Err(NestError)
  }

  fn emit_error(
    &mut self,
    err: Spanned<NestError, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), NestError> {
    Err(err.into_data())
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, _: u64) {}
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for NestEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), NestError> {
    Err(NestError)
  }
}

impl<'inp> UnclosedEmitter<'inp, TestLexer<'inp>> for NestEmitter {
  fn emit_unclosed<Delimiter>(
    &mut self,
    _: Unclosed<Delimiter, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), NestError> {
    Err(NestError)
  }
}

fn nest_ctx<'inp>() -> ParserContext<'inp, TestLexer<'inp>, NestEmitter> {
  ParserContext::new(NestEmitter)
}

trait NestBound<'inp>:
  Emitter<'inp, TestLexer<'inp>, Error = NestError>
  + FullContainerEmitter<'inp, TestLexer<'inp>>
  + UnclosedEmitter<'inp, TestLexer<'inp>>
{
}

impl<'inp, E> NestBound<'inp> for E where
  E: Emitter<'inp, TestLexer<'inp>, Error = NestError>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
{
}

// ── door 1: through the repetition combinator ─────────────────────────────────

/// Continue while the next token opens another child; stop on anything else, which for a
/// well-formed document is the closing bracket.
fn more_children<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
) -> Result<Action, <Ctx::Emitter as Emitter<'inp, TestLexer<'inp>>>::Error>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
{
  Ok(match peeked.pop_front() {
    None => Action::Stop,
    Some(tok) => {
      let tok = tok
        .as_maybe_ref()
        .map(|t| t.token().copied(), |t| t.token())
        .into_inner();
      match **tok.data() {
        Token::LBracket => Action::Continue,
        _ => Action::Stop,
      }
    }
  })
}

/// `node := '[' node* ']'`, with the repetition written as
/// `Collect<DelimitedBy<RepeatedWhile<..>, Bracket>, Vec<_>>`. Returns the node count of the
/// subtree, so the two doors have a value to agree on.
fn combinator_node<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<usize, NestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: NestBound<'inp>,
{
  let anchor = 0u8;
  mark(&anchor);

  let children: Vec<usize> = combinator_node::<Ctx>
    .repeated_while::<_, U1>(more_children::<Ctx>)
    .delimited_by_brackets()
    .collect()
    .parse_input(inp)?;

  Ok(1 + children.iter().sum::<usize>())
}

// ── door 2: the same grammar, loop written out ────────────────────────────────

/// `node := '[' node* ']'`, entered with the opening bracket already consumed. One frame per
/// nesting level, exactly like the combinator door, and reaching only `InputRef::next`.
fn hand_written_body<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<usize, NestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = NestError>,
{
  let anchor = 0u8;
  mark(&anchor);

  let mut nodes = 1;
  loop {
    match inp.next()? {
      None => return Err(NestError),
      Some(tok) => match tok.into_data() {
        Token::RBracket => return Ok(nodes),
        Token::LBracket => nodes += hand_written_body(inp)?,
        _ => return Err(NestError),
      },
    }
  }
}

fn hand_written_node<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<usize, NestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = NestError>,
{
  match inp.next()? {
    Some(tok) if matches!(tok.data(), Token::LBracket) => hand_written_body(inp),
    _ => Err(NestError),
  }
}

// ── the measurement ───────────────────────────────────────────────────────────

/// `[[[…]]]` nested `depth` deep, with a trailing sentinel so the combinator door's decision
/// peek always has a token to look at rather than hitting end of input.
fn nested(depth: usize) -> String {
  let mut s = String::with_capacity(depth * 2 + 1);
  for _ in 0..depth {
    s.push('[');
  }
  for _ in 0..depth {
    s.push(']');
  }
  s.push('+');
  s
}

#[test]
#[cfg_attr(
  miri,
  ignore = "a stack-pointer probe needs a contiguous native stack; Miri models frame locals as \
            separate virtual allocations, so the deltas are allocation spacing rather than frame \
            sizes"
)]
fn per_nesting_level_stack_cost() {
  const DEPTH: usize = 12;
  let src = nested(DEPTH);

  take_marks();
  let combinator: Result<usize, NestError> = Parser::with_context(nest_ctx())
    .apply(combinator_node)
    .parse_str(&src);
  let combinator_marks = take_marks();
  let combinator_nodes = combinator.expect("combinator door rejected a well-formed document");

  let hand: Result<usize, NestError> = Parser::with_context(nest_ctx())
    .apply(hand_written_node)
    .parse_str(&src);
  let hand_marks = take_marks();
  let hand_nodes = hand.expect("hand-written door rejected a well-formed document");

  assert_eq!(
    combinator_nodes, hand_nodes,
    "the two doors disagree on what they parsed"
  );
  assert_eq!(combinator_nodes, DEPTH);
  assert_eq!(combinator_marks.len(), DEPTH);
  assert_eq!(hand_marks.len(), DEPTH);

  // Everything above is a parse the two doors have to agree on, and it holds under any
  // instrumentation. Only the numbers below need a native stack, so the refusal is here rather
  // than at the top of the test: an environment that cannot be measured still gets its parse
  // checked, and gets no figure.
  if let Some(why) = relocation_reason() {
    announce_skipped(&why);
    return;
  }

  let combinator_cost = per_level(&combinator_marks);
  let hand_cost = per_level(&hand_marks);
  println!(
    "per nesting level: combinator {combinator_cost} B, hand-written {hand_cost} B, \
     ratio {:.1}x",
    combinator_cost as f64 / hand_cost.max(1) as f64
  );
}

/// A positive control for every way [`per_level_with`] refuses, plus the two layouts it is
/// supposed to accept.
///
/// Without these the refusals are unfalsifiable: the file's first version differenced with
/// `saturating_sub` and printed `0 B` for every layout it did not model, and the fix that
/// replaced it carried two assertions that no input could reach.
mod refusals {
  use super::{frames_are_relocated, per_level_with};

  /// Innermost frame at the lowest address — the common layout.
  #[test]
  fn accepts_descending() {
    assert_eq!(per_level_with(None, &[9000, 8000, 7000, 6000]), 1000);
  }

  /// An upward-growing stack is SUPPORTED, not merely detected: the same magnitude comes back.
  #[test]
  fn accepts_ascending() {
    assert_eq!(per_level_with(None, &[6000, 7000, 8000, 9000]), 1000);
  }

  /// Irregular levels do not set the number; the median does.
  #[test]
  fn median_ignores_one_irregular_level() {
    assert_eq!(per_level_with(None, &[9000, 8000, 7000, 500]), 1000);
  }

  #[test]
  #[should_panic(expected = "need at least two levels to difference, got 1")]
  fn refuses_undersized() {
    per_level_with(None, &[9000]);
  }

  /// The check that could not fire before the reorder: strict monotonicity rejected a repeated
  /// address first, so the zero-delta refusal had no reachable input.
  #[test]
  #[should_panic(expected = "cost zero bytes of stack")]
  fn refuses_zero_delta() {
    per_level_with(None, &[9000, 8000, 8000, 7000]);
  }

  /// …including when the repeat is the only pair, which is the shape a probe that samples one
  /// frame twice produces.
  #[test]
  #[should_panic(expected = "cost zero bytes of stack")]
  fn refuses_all_zero_deltas() {
    per_level_with(None, &[9000, 9000, 9000]);
  }

  #[test]
  #[should_panic(expected = "not monotone in one direction")]
  fn refuses_reversed() {
    per_level_with(None, &[9000, 8000, 8500, 7000]);
  }

  #[test]
  #[should_panic(expected = "refusing to read frame sizes out of these samples")]
  fn refuses_relocated_frames() {
    // Samples that would otherwise pass every check — a fake stack can look exactly like this.
    per_level_with(
      frames_are_relocated(Some("address")),
      &[9000, 8000, 7000, 6000],
    );
  }

  #[test]
  fn sanitizer_variable_is_read_as_presence() {
    assert!(frames_are_relocated(Some("address")).is_some());
    assert!(frames_are_relocated(Some("thread")).is_some());
    // Unset and set-but-empty are the same thing, and neither is a sanitizer. Under Miri the
    // detector fires on `cfg!(miri)` regardless, so it is the one build where these hold the
    // other way.
    if !cfg!(miri) {
      assert!(frames_are_relocated(None).is_none());
      assert!(frames_are_relocated(Some("")).is_none());
    }
  }
}
