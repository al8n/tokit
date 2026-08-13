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
//! The assertion here is only that both doors accept the same document and agree on the shape
//! they parsed. The stack numbers are printed, not asserted: they move with the toolchain, the
//! target and `-Cdebuginfo`, and a threshold on them would be a flake rather than a contract.
//! Run with `--nocapture` to see them.

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

/// The per-level cost, taken as the median of the consecutive differences so that one
/// irregular level (the outermost, the innermost) does not set the number.
fn per_level(marks: &[usize]) -> usize {
  let mut deltas: Vec<usize> = marks
    .windows(2)
    .map(|w| w[0].saturating_sub(w[1]))
    .collect();
  assert!(!deltas.is_empty(), "need at least two levels to difference");
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

  let combinator_cost = per_level(&combinator_marks);
  let hand_cost = per_level(&hand_marks);
  println!(
    "per nesting level: combinator {combinator_cost} B, hand-written {hand_cost} B, \
     ratio {:.1}x",
    combinator_cost as f64 / hand_cost.max(1) as f64
  );
}
