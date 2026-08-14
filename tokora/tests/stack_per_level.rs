#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
#![allow(warnings)]

//! What one level of grammar nesting costs on the native stack, measured three ways over the
//! **same grammar** in the **same binary** at the **same optimisation level**.
//!
//! `recursion_tracker`'s per-level table is measured against the pratt driver. This measures
//! the other recursive shape a grammar can have: a repetition combinator whose element parser
//! re-enters the same production. The three doors differ only in how the repetition is written —
//!
//! - `combinator_node` goes through `Collect<DelimitedBy<RepeatedWhile<..>, Bracket>, Vec<_>>`,
//! - `separated_node` through `Collect<DelimitedBy<SeparatedWhile<..>, Bracket>, Vec<_>>`,
//! - `hand_written_body` writes the loop out with `InputRef::next`,
//!
//! — so the difference between their per-level costs is what each combinator layer costs, with
//! the lexer, the cache, the emitter and the token type held fixed.
//!
//! The separated door is here because it is the shape no corpus in this repository measured.
//! The five criterion benches monomorphize `Repeated`, `Separated` and `Collect` out of
//! `parser::many` and nothing else — no `DelimitedBy`, no `SeparatedWhile`, no cardinality or
//! policy wrapper — so a change that costs stack in a recursive separated grammar is invisible
//! to them, and it is the deeper of the two combinator doors.
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
//!
//! **The figures are opt-in.** `TOKORA_STACK_PROBE=native cargo test … -- --nocapture` prints
//! them; without that variable the parse is still checked and no number is produced. Nothing a
//! stable-compiled integration test can read proves the absence of instrumentation, and this file
//! previously treated that absence of proof as proof of absence: under
//! `RUSTFLAGS=-Zsanitizer=address` with `detect_stack_use_after_return=1` it printed
//! `combinator 4096 B, separated 6144 B, hand-written 256 B` — ASan fake-stack size classes,
//! reported as bytes of stack per nesting level.

mod common;

use core::cell::RefCell;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, EmitterView, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser,
  ParserContext, Token as TokenTrait,
  cache::Peeked,
  emitter::{
    FromUnclosed, FullContainerEmitter, SeparatedEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntaxOf},
    token::{MissingTokenOf, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  parser::Action,
  span::Spanned,
  utils::CowStr,
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

/// This build's reason that the address of a local is **not** a position on a native stack, if it
/// has one — see [`common::native_stack`], which both address-differencing probes now share.
///
/// The reading needs a native stack, and this file cannot tell a fake one from a real one on its
/// own: allocations handed out monotonically with a constant stride pass [`per_level`]'s ordering
/// check and its nonzero check alike, and the harness would then print allocation spacing as bytes
/// per nesting level. So the question is settled before the samples are read, and settled by the
/// **absence of a positive statement** rather than the absence of a signal: what this file used to
/// do was read `TOKORA_SANITIZER`, which nothing but `ci/sanitizer.sh` exports, so a direct
/// `RUSTFLAGS=-Zsanitizer=address cargo test` printed a fake stack's spacing as a frame size.
fn relocation_reason() -> Option<String> {
  common::native_stack::measurement_refusal_here()
}

/// Says — on the process's **real** stderr — that no figure was produced and why.
fn announce_skipped(why: &str) {
  common::native_stack::announce_skipped("stack_per_level", why);
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
/// - **The build relocates frames, or cannot be shown not to.** [`relocation_reason`] first,
///   before anything is read out of the samples, because a fake stack can produce samples that
///   pass every check below.
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

impl<'inp> SeparatedEmitter<'inp, TestLexer<'inp>> for NestEmitter {
  fn emit_missing_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), NestError> {
    Err(NestError)
  }

  fn emit_missing_element(
    &mut self,
    _: MissingSyntaxOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), NestError> {
    Err(NestError)
  }
}

impl<'inp> UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>> for NestEmitter {
  fn emit_unexpected_leading_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), NestError> {
    Err(NestError)
  }
}

impl<'inp> UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>> for NestEmitter {
  fn emit_unexpected_trailing_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
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

/// The extra emitter surface the separated door needs on top of [`NestBound`].
trait SepNestBound<'inp>:
  SeparatedEmitter<'inp, TestLexer<'inp>>
  + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
  + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
{
}

impl<'inp, E> SepNestBound<'inp> for E where
  E: SeparatedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
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

// ── door 1b: the same grammar through the SEPARATED repetition combinator ─────

/// `node := '[' node (',' node)* ']'`, with the repetition written as
/// `Collect<DelimitedBy<SeparatedWhile<..>, Bracket>, Vec<_>>`.
///
/// This is the shape [`combinator_node`] does not reach. The five criterion benches
/// monomorphize `Repeated`, `Separated` and `Collect` and nothing else out of `parser::many` —
/// no `DelimitedBy`, no `SeparatedWhile`, no cardinality or policy wrapper — so an attribute
/// change that costs stack HERE is invisible to a corpus made of them, and a recursive
/// separated grammar is exactly where a per-level regression would hide. It goes through
/// `parser::many::sep_while::delim::unbounded`, one of the 144 leaf modules the four
/// `impl_separated_*` macros generate.
///
/// The document is the same `[[[…]]]`: one child per level, so the separator is never taken and
/// the doors stay comparable level for level.
fn separated_node<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<usize, NestError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: NestBound<'inp> + SepNestBound<'inp>,
{
  let anchor = 0u8;
  mark(&anchor);

  let children: Vec<usize> = separated_node::<Ctx>
    .separated_by_comma_while::<_, U1>(more_children::<Ctx>)
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

  let separated: Result<usize, NestError> = Parser::with_context(nest_ctx())
    .apply(separated_node)
    .parse_str(&src);
  let separated_marks = take_marks();
  let separated_nodes = separated.expect("separated door rejected a well-formed document");

  let hand: Result<usize, NestError> = Parser::with_context(nest_ctx())
    .apply(hand_written_node)
    .parse_str(&src);
  let hand_marks = take_marks();
  let hand_nodes = hand.expect("hand-written door rejected a well-formed document");

  assert_eq!(
    combinator_nodes, hand_nodes,
    "the two doors disagree on what they parsed"
  );
  assert_eq!(
    separated_nodes, hand_nodes,
    "the separated door disagrees with the other two on what it parsed"
  );
  assert_eq!(combinator_nodes, DEPTH);
  assert_eq!(combinator_marks.len(), DEPTH);
  assert_eq!(separated_marks.len(), DEPTH);
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
  let separated_cost = per_level(&separated_marks);
  let hand_cost = per_level(&hand_marks);
  println!(
    "per nesting level: combinator {combinator_cost} B, separated {separated_cost} B, \
     hand-written {hand_cost} B, ratios {:.1}x / {:.1}x",
    combinator_cost as f64 / hand_cost.max(1) as f64,
    separated_cost as f64 / hand_cost.max(1) as f64
  );
}

/// A positive control for every way [`per_level_with`] refuses, plus the two layouts it is
/// supposed to accept.
///
/// Without these the refusals are unfalsifiable: the file's first version differenced with
/// `saturating_sub` and printed `0 B` for every layout it did not model, and the fix that
/// replaced it carried two assertions that no input could reach.
mod refusals {
  use super::common::native_stack::{
    ANNOUNCE, FrameReuseArm, OPT_IN, StackEvidence, announce_arm, asan_fake_stack_enabled,
    asan_fake_stack_enabled_from, fake_stack_default, fake_stack_default_for, frame_reuse_arm,
    frames_reused, instrumentation_refusal, measurement_refusal, with_evidence_here,
  };
  use super::per_level_with;

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
      measurement_refusal(StackEvidence {
        compiler_sanitizers: Some("address"),
        ..StackEvidence::native()
      }),
      &[9000, 8000, 7000, 6000],
    );
  }

  /// Each detector, one at a time, against an otherwise all-clear build.
  ///
  /// `StackEvidence::native()` is the all-clear, so every case below differs from a measurable
  /// build in exactly one field and the assertion names which one.
  #[test]
  fn every_detector_refuses_on_its_own() {
    let cases: [(&str, StackEvidence<'_>); 5] = [
      (
        "miri",
        StackEvidence {
          miri: true,
          ..StackEvidence::native()
        },
      ),
      (
        "the compiler reports sanitize=address",
        StackEvidence {
          compiler_sanitizers: Some("address"),
          ..StackEvidence::native()
        },
      ),
      (
        "the compiler would not report a cfg list",
        StackEvidence {
          build_flags_opaque: true,
          ..StackEvidence::native()
        },
      ),
      (
        "TOKORA_SANITIZER=thread",
        StackEvidence {
          announced_sanitizer: Some("thread"),
          ..StackEvidence::native()
        },
      ),
      (
        "frames are not reused",
        StackEvidence {
          frames_reused: false,
          ..StackEvidence::native()
        },
      ),
    ];
    for (what, e) in cases {
      assert!(
        instrumentation_refusal(e).is_some(),
        "{what} did not refuse an assertion"
      );
      assert!(
        measurement_refusal(e).is_some(),
        "{what} did not refuse a measurement"
      );
    }
    assert!(instrumentation_refusal(StackEvidence::native()).is_none());
    assert!(measurement_refusal(StackEvidence::native()).is_none());
  }

  /// **The fix, as a control.** An unset affirmation refuses even when nothing was detected — the
  /// case a direct `RUSTFLAGS=-Zsanitizer=address cargo test` used to take straight to a printed
  /// number, because the only signal the probe read was one the compiler never sets.
  #[test]
  fn an_unaffirmed_build_takes_no_measurement() {
    let unaffirmed = StackEvidence {
      affirmed_native: false,
      ..StackEvidence::native()
    };
    let why = measurement_refusal(unaffirmed).expect("an unaffirmed build must not be measured");
    assert!(
      why.contains(OPT_IN),
      "the refusal must name {OPT_IN}: {why}"
    );
    // …and it refuses ONLY a measurement. An assertion that would fail loudly under a sanitizer
    // must not be skipped merely because nobody set a variable.
    assert!(instrumentation_refusal(unaffirmed).is_none());
  }

  /// The affirmation is not an override: a detected sanitizer still refuses, and the refusal names
  /// the sanitizer rather than the missing variable.
  #[test]
  fn affirming_a_native_build_cannot_overrule_a_detected_sanitizer() {
    let e = StackEvidence {
      compiler_sanitizers: Some("address"),
      affirmed_native: true,
      ..StackEvidence::native()
    };
    let why = measurement_refusal(e).expect("an affirmed sanitizer build must not be measured");
    assert!(why.contains("sanitize=address"), "{why}");
    assert!(!why.contains(OPT_IN), "{why}");
  }

  /// Empty is not a sanitizer, and neither is unset: `ci/sanitizer.sh` exports a name for every
  /// leg it runs, so an empty value is a variable someone cleared rather than a leg.
  #[test]
  fn an_empty_announcement_is_not_a_sanitizer() {
    for value in [None, Some("")] {
      let e = StackEvidence {
        announced_sanitizer: value,
        ..StackEvidence::native()
      };
      assert!(
        instrumentation_refusal(e).is_none(),
        "{value:?} was read as a sanitizer"
      );
    }
  }

  /// The one detector that observes rather than asks, run against this process.
  ///
  /// # Three builds, three different things to assert
  ///
  /// [`frames_reused`] observes **relocation**, and relocation is not instrumentation. This
  /// control used to assert `frames_reused() == !instrumented` with `instrumented` true for any
  /// reported or announced sanitizer, which is a contradiction in two of the configurations CI
  /// actually runs:
  ///
  /// * **ThreadSanitizer** — in the matrix, next to `address` — instruments memory accesses and
  ///   leaves frames exactly where they were, so sequential calls reuse one frame and the old
  ///   assertion failed while detection and refusal were both working.
  /// * **ASan with the fake stack off**, which has to be *asked for* on Linux:
  ///   `ci/sanitizer.sh` exports `ASAN_OPTIONS` without `detect_stack_use_after_return`, and
  ///   compiler-rt defaults that flag to on for non-Android Linux — see
  ///   [`fake_stack_default`]. So the leg the sanitizer matrix actually runs relocates, and the
  ///   contradiction applies to the `address` leg only once the runtime is explicitly told not to.
  ///
  /// So the arm is chosen by [`frame_reuse_arm`], which asks the two questions that decide
  /// relocation — Miri, or ASan *with* the fake stack switched on — rather than the question that
  /// decides instrumentation:
  ///
  /// | [`FrameReuseArm`] | this build | what is asserted |
  /// |---|---|---|
  /// | `Relocating` | Miri, ASan + fake stack | the detector **fires**: frames are not reused |
  /// | `Declared` | TSan, ASan with the fake stack off | the refusal **stands without the observation**: both strictness levels still refuse with `frames_reused` forced true |
  /// | `Native` | nothing detected | the detector is **quiet**: frames are reused, so it does not refuse every native run |
  ///
  /// The middle arm asserts nothing about [`frames_reused`] in either direction. That is
  /// deliberate: what must hold under a non-relocating sanitizer is that the *declared*
  /// instrumentation carries the refusal on its own, and an ASan runtime that changed its
  /// fake-stack default would then move this build between the first two arms without inventing a
  /// failure. Nor does the first arm require a declaration — a build that relocates is exactly the
  /// build whose instrumentation may have arrived by a route `build.rs` cannot see, which is the
  /// case the observation exists for.
  ///
  /// # Which arm ran is reported, because a silent arm is how this went wrong
  ///
  /// An arm that ignores the observation is the right thing to assert and the wrong thing to
  /// reach by accident: while the fake-stack default was read as off, every CI `address` run took
  /// the middle arm on a build whose frames were in fact relocating, and passed without ever
  /// putting [`frames_reused`] to the question. Nothing in a green log distinguished that from
  /// the arm being exercised. So the arm is announced on the real stderr **before** any assertion
  /// can end the test, and `ci/sanitizer.sh` runs the `address` leg twice — fake stack off, then
  /// on — and fails the cell unless the two land in different arms.
  #[test]
  fn the_frame_reuse_detector_agrees_with_this_build() {
    let reused = frames_reused();
    let arm = frame_reuse_arm();
    // Before the assertions, so a failing leg still says which arm it was in.
    announce_arm("stack_per_level", arm);

    // Read from the raw source rather than through the gathered evidence, so the middle arm's
    // assertion covers the wiring from `build.rs` to the decision and not just the decision.
    let compiler_says = option_env!("TOKORA_BUILD_SANITIZERS");

    match arm {
      FrameReuseArm::Relocating => assert!(
        !reused,
        "this build relocates frames — Miri, or ASan with the fake stack on — and the detector did \
         not see it"
      ),

      // TSan, or ASan with the fake stack off. `frames_reused()` is right to report *reused*
      // here, so the property that has to hold is the other one: the build is refused anyway. The
      // observation is set aside for the question, which is what makes the answer a statement
      // about the declaration alone.
      FrameReuseArm::Declared => {
        let why = with_evidence_here(|e| {
          instrumentation_refusal(StackEvidence {
            frames_reused: true,
            ..e
          })
        })
        .expect(
          "a declared sanitizer build was not refused once the frame observation was set aside — \
           `pratt_limit_unit_sink`'s address witness runs on exactly this decision, and under a \
           sanitizer that does not relocate it would run against redzoned frames",
        );
        if let Some(list) = compiler_says {
          assert!(
            why.contains(&format!("sanitize={list}")),
            "the compiler reported sanitize={list} for this build and the refusal does not say \
             so: {why}"
          );
        }
      }

      FrameReuseArm::Native => assert!(
        reused,
        "nothing declares this build instrumented, yet frames are not reused — either the detector \
         refuses every native run, which is a refusal nobody would keep, or instrumentation reached \
         this process by a route build.rs cannot see"
      ),
    }
  }

  /// The arm is what the announcement says it is, and the two disagreeing is not possible.
  ///
  /// `ci/sanitizer.sh` reads the arm off a printed line; if the printed name and the branch could
  /// come apart, that gate would be pinning a label. They cannot — both come from one
  /// [`frame_reuse_arm`] call — and this pins the third leg of the triangle: that the arm agrees
  /// with the raw declaration sources it is derived from.
  #[test]
  fn the_announced_arm_is_the_arm_this_build_is_in() {
    let arm = frame_reuse_arm();
    let declared = option_env!("TOKORA_BUILD_SANITIZERS").is_some()
      || option_env!("TOKORA_BUILD_FLAGS_OPAQUE").is_some()
      || std::env::var(ANNOUNCE).is_ok_and(|v| !v.is_empty());

    match arm {
      // Miri relocates without any declaration at all, which is why this one is not `declared`.
      FrameReuseArm::Relocating => assert!(cfg!(miri) || declared, "{arm:?}"),
      FrameReuseArm::Declared => assert!(declared, "{arm:?} on a build nothing declares"),
      FrameReuseArm::Native => assert!(!declared, "{arm:?} on a build something declares"),
    }
    // Every row, not the one this host is in. Asking only about `arm` pins whichever name the
    // machine running the test happens to reach and leaves the other two free to be retargeted —
    // and the two the CI gates match on are exactly the two an ordinary host never reaches.
    let names: [(FrameReuseArm, &str); 3] = [
      (FrameReuseArm::Relocating, "relocating"),
      (FrameReuseArm::Declared, "declared"),
      (FrameReuseArm::Native, "native"),
    ];
    for (each, name) in names {
      assert_eq!(
        each.as_str(),
        name,
        "ci/sanitizer.sh and ci/stack_probe.sh match on these names"
      );
    }
  }

  /// [`asan_fake_stack_enabled_from`] against the strings it has to read, including the one
  /// `ci/sanitizer.sh` actually exports.
  ///
  /// The live wiring is exercised by the arm chosen above; this pins the parse, which is what
  /// decides which arm that is.
  ///
  /// # Every row is run against **both** platform defaults
  ///
  /// The previous table asked only what an unspecified flag means with the default hardcoded to
  /// off, so it agreed with a lookup that answered "off" for a Linux run where the fake stack was
  /// on. A row is only about the parse if it says what the parse does to a default it did not
  /// choose: the `default` column is what compiler-rt would start from, and the rows where the
  /// options do not mention the flag have to return it unchanged.
  #[test]
  fn the_fake_stack_flag_is_read_the_way_the_sanitizer_reads_it() {
    // (compiler-rt's default for the platform, ASAN_OPTIONS, the fake stack is on)
    let cases: [(bool, Option<&str>, bool); 20] = [
      // Unspecified in every spelling of unspecified: the default survives, both ways.
      (false, None, false),
      (true, None, true),
      (false, Some(""), false),
      (true, Some(""), true),
      // What `ci/sanitizer.sh` exports. It does not mention the flag, so on Linux the `address`
      // leg DOES relocate — the row this table used to get wrong.
      (false, Some("detect_odr_violation=0 detect_leaks=0"), false),
      (true, Some("detect_odr_violation=0 detect_leaks=0"), true),
      // An explicit setting overrides the default in both directions.
      (false, Some("detect_stack_use_after_return=1"), true),
      (true, Some("detect_stack_use_after_return=0"), false),
      (false, Some("detect_stack_use_after_return=true"), true),
      (true, Some("detect_stack_use_after_return=false"), false),
      (false, Some("detect_stack_use_after_return=yes"), true),
      (true, Some("detect_stack_use_after_return=no"), false),
      // Every separator the sanitizer's parser accepts.
      (
        false,
        Some("detect_leaks=0:detect_stack_use_after_return=1"),
        true,
      ),
      (
        false,
        Some("detect_stack_use_after_return=1,detect_leaks=0"),
        true,
      ),
      (
        false,
        Some("detect_leaks=0\ndetect_stack_use_after_return=1"),
        true,
      ),
      // A repeated flag takes its LAST value, as the sanitizer's own parser does.
      (
        false,
        Some("detect_stack_use_after_return=1 detect_stack_use_after_return=0"),
        false,
      ),
      (
        true,
        Some("detect_stack_use_after_return=0 detect_stack_use_after_return=1"),
        true,
      ),
      // Not a prefix match: this is a different flag name, so the default stands.
      (false, Some("detect_stack_use_after_return_2=1"), false),
      // `FlagHandler<bool>::Parse` is a case-sensitive compare against `1`/`true`/`yes` and
      // `0`/`false`/`no`; `True` is none of them, so the runtime reports `Invalid value for bool
      // option` and aborts rather than reading it as true. Watched, along with `TRUE`, `False`,
      // `bogus` and the empty value. Nothing is written, so the default stands here too.
      (true, Some("detect_stack_use_after_return=True"), true),
      (false, Some("detect_stack_use_after_return=TRUE"), false),
    ];
    for (default, options, expected) in cases {
      assert_eq!(
        asan_fake_stack_enabled_from(default, options),
        expected,
        "default={default}, ASAN_OPTIONS={options:?}"
      );
    }
  }

  /// **The fix, as a control.** An unspecified flag means the platform's default, and the platform
  /// default is compiler-rt's.
  ///
  /// [`asan_fake_stack_enabled`] returning a hardcoded `false` for an absent `ASAN_OPTIONS` is the
  /// defect this replaces: right on this repository's development host, wrong on the Linux runner
  /// the sanitizer matrix uses — so three configurations could be verified by hand, pre and post,
  /// and still leave CI classifying a relocating build as a stationary one.
  ///
  /// # The row that matters is one this host cannot be
  ///
  /// Every platform is asked, not only the one running the test. `compiler-rt/lib/asan/`
  /// `asan_flags.inc` declares the flag with the default `SANITIZER_LINUX && !SANITIZER_ANDROID`,
  /// and Rust splits Android out of `target_os`, so the table is `linux` and nothing else. Asking
  /// only about the host would make the correction and the defect indistinguishable from macOS,
  /// where both answer `false`.
  #[test]
  fn an_unspecified_flag_means_the_platform_default() {
    let table: [(&str, bool); 8] = [
      // The one the whole finding is about, and the one this host cannot evaluate for itself.
      ("linux", true),
      // `SANITIZER_ANDROID` is carved back out. Rust spells it as its own `target_os`, and the
      // sanitizer matrix's `cross` neighbours build for it.
      ("android", false),
      ("macos", false),
      ("ios", false),
      ("windows", false),
      ("freebsd", false),
      ("netbsd", false),
      ("fuchsia", false),
    ];
    for (target_os, on) in table {
      assert_eq!(
        fake_stack_default_for(target_os),
        on,
        "compiler-rt's default for target_os = {target_os:?}"
      );
    }

    assert_eq!(
      fake_stack_default(),
      fake_stack_default_for(std::env::consts::OS),
      "this build's answer must come from the table, not from a second opinion"
    );
    assert_eq!(
      asan_fake_stack_enabled(None),
      fake_stack_default(),
      "an absent ASAN_OPTIONS must mean compiler-rt's default, not `false`"
    );
    assert_eq!(
      asan_fake_stack_enabled(Some("detect_odr_violation=0 detect_leaks=0")),
      fake_stack_default(),
      "`ci/sanitizer.sh`'s own options do not mention the flag, so they do not turn it off"
    );
  }
}
