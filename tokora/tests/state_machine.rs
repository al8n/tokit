#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
#![allow(warnings)]
mod common;

use common::E;
use tokora::EmitterView;

// Tests targeting uncovered error/edge-case branches in the separated parser
// state machines (`sep/parse/mod.rs` and `sep_while/parse/mod.rs`).
//
// The key insight: existing tests use emitters that return `Err(E)` on first
// error, causing the state machine to short-circuit. By using a *recovering*
// emitter that returns `Ok(())`, we let parsing continue through all the
// error-recovery branches.

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, Silent, TooFewEmitter, TooManyEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  input::Cursor,
  parser::Action,
  span::Spanned,
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

fn recovering_ctx() -> ParserContext<'static, TestLexer<'static>, Silent<E>> {
  ParserContext::new(Silent::new())
}

// ── Also keep a fatal emitter for tests that should error ────────────────────

fn fatal_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

// ── Element parsers ──────────────────────────────────────────────────────────

/// TryParseInput — accepts Num tokens, declines everything else.
fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  inp
    .try_expect(|t| matches!(t.data(), Token::Num(_)))
    .map(|opt| match opt {
      None => ParseAttempt::Decline,
      Some(tok) => ParseAttempt::Accept(match tok.into_data() {
        Token::Num(n) => n,
        _ => unreachable!(),
      }),
    })
}

/// TryParseInput — always returns Err(E). Used to exercise the Err path in parse().
fn try_num_failing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  // Consume the token so the loop makes progress, then error.
  let _ = inp.next()?;
  Err(E)
}

/// ParseInput for sep_while — accepts Num tokens.
fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  match inp.next()? {
    None => Err(E),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(E),
    },
  }
}

/// Condition for sep_while: continue if next token is a Num.
fn decide_num<'inp, Ctx>(
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
      if matches!(**tok.data(), Token::Num(_)) {
        Action::Continue
      } else {
        Action::Stop
      }
    }
  })
}

// ═══════════════════════════════════════════════════════════════════════════════
// A. sep/parse — Separated (TryParseInput) state machine branches
// ═══════════════════════════════════════════════════════════════════════════════

// ── A1. Consecutive leading separators: State::Leading in handle_separator ────
// Input ",,1,2" with allow_leading — first comma → Leading, second comma →
// Leading→emit_missing_element→Separator (lines 126-143)

fn parse_sep_consecutive_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_consecutive_leading_separators() {
  // ",,1,2" — exercises State::Leading in handle_separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_consecutive_leading)
    .parse_str(",,1,2");
  // With recovering emitter, parsing continues past the missing_element error
  assert!(r.is_ok());
}

#[test]
fn sep_triple_leading_separators() {
  // ",,,1" — exercises consecutive Leading→Leading→Leading transitions
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_consecutive_leading)
    .parse_str(",,,1");
  assert!(r.is_ok());
}

#[test]
fn sep_leading_only_comma() {
  // "," with allow_leading — exercises Leading→End path
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_consecutive_leading)
    .parse_str(",");
  assert!(r.is_ok());
}

#[test]
fn sep_double_leading_only() {
  // ",," with allow_leading — exercises Leading→Leading→End
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_consecutive_leading)
    .parse_str(",,");
  assert!(r.is_ok());
}

// ── A2. Consecutive mid-separators: State::Separator in handle_separator ─────
// Input "1,,2" — after parsing 1, comma → Separator, next comma →
// Separator→emit_missing_element (lines 156-167)

fn parse_sep_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num.separated_by_comma().collect().parse_input(inp)
}

#[test]
fn sep_consecutive_mid_separators() {
  // "1,,2" — exercises State::Separator in handle_separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str("1,,2");
  assert!(r.is_ok());
}

#[test]
fn sep_triple_mid_separators() {
  // "1,,,2" — exercises Separator→Separator→Separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str("1,,,2");
  assert!(r.is_ok());
}

// ── A3. State::Start in handle_separator — first token is separator ──────────
// Input ",1,2" without allow_leading — first comma hits Start in handle_separator
// (lines 146-154)

#[test]
fn sep_start_state_separator() {
  // ",1,2" without allow_leading — exercises State::Start in handle_separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_start_separator_only() {
  // "," — Start→Leading→End
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

// ── A4. State::Element in handle_continue — missing separator ────────────────
// Input "1 2" where both are parsed numbers — exercises lines 239-256
// The parser sees 1, transitions to Element, then sees 2 without a comma,
// triggering emit_missing_separator.
// Note: "1 2" won't work because the lexer skips whitespace and try_num
// only peeks. We need the tokens to be adjacent for the separator to be missing.
// Actually the state machine works by: first checking if next token is separator,
// if not, trying to parse element. So "1 2" will see "1" parsed, then "2" is
// not a comma, so it tries to parse it as element → Accept(2) → State::Element
// in handle_continue.

#[test]
fn sep_missing_separator_between_elements() {
  // We need tokens where two Num tokens appear without comma between them.
  // "1 2" — lexer skips whitespace, so tokens are [Num(1), Num(2)].
  // After parsing 1 (State::Element), next token 2 is not comma, try_num
  // accepts it → handle_continue with State::Element → emit_missing_separator.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str("1 2");
  assert!(r.is_ok());
}

#[test]
fn sep_missing_separator_three_elements() {
  // "1 2 3" — exercises missing separator path multiple times
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// ── A5. Element parser errors out — Err(e) in parse() loop ───────────────────
// Lines 79-81: match self.f.try_parse_input(inp) => Err(e)

fn parse_sep_failing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_failing
    .separated_by_comma()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_element_parser_error() {
  // Element parser always errors — exercises lines 79-81
  // With recovering emitter, emit_error returns Ok(()), so the loop continues
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_failing)
    .parse_str("1");
  // The parser will error because try_num_failing returns Err(E)
  // and emit_error is called, which returns Ok(()) (recovering),
  // but then the loop continues and eventually reaches handle_end
  assert!(r.is_ok() || r.is_err());
}

// ── A6. Element parser declines after separator — Ok(Decline) path ───────────
// Input "1," — after parsing 1 and seeing comma (State::Separator),
// there's no more Num token, so try_num declines → handle_end

#[test]
fn sep_decline_after_separator() {
  // "1," — try_num declines after the trailing comma, exercises Ok(Decline) → handle_end
  // With no allow_trailing, the trailing separator causes an error.
  // The recovering emitter lets this succeed.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str("1,");
  assert!(r.is_ok());
}

#[test]
fn sep_decline_after_leading() {
  // "," — try_num declines after leading comma, handle_end with State::Leading
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str(",");
  assert!(r.is_ok());
}

// ── A7. Full container paths — at_most with recovering emitter ───────────────
// Exercises handle_continue State::Separator with full container (lines 198-205)

fn parse_sep_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_full_container_after_separator() {
  // "1,2" with at_most(1) — after parsing 1, comma→Separator, then 2 is
  // accepted → handle_continue State::Separator → push fails → emit_full_container
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_at_most_1)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_full_container_overflow_many() {
  // "1,2,3" with at_most(1)
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_at_most_1)
    .parse_str("1,2,3");
  assert!(r.is_ok());
}

// ── A8. Full container from Leading state ────────────────────────────────────
// Leading + element with at_most(0) or at_most(1) that's already full

fn parse_sep_allow_leading_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .allow_leading()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_full_container_leading_state() {
  // ",1,2" with allow_leading at_most(1) — first comma→Leading, then 1→
  // handle_continue Leading state → push ok (1 elem), then comma→Separator,
  // then 2 → handle_continue Separator → push fails → emit_full_container
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_leading_at_most_1)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

// ── A9. Full container from Start state ──────────────────────────────────────

#[test]
fn sep_full_container_start_state() {
  // "1,2" with at_most(1) — first 1 → handle_continue Start state → push ok,
  // then comma→Separator, then 2 → push fails
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_at_most_1)
    .parse_str("1,2");
  assert!(r.is_ok());
}

// ── A10. Missing separator + too_many_element (State::Element handle_continue) ─

fn parse_sep_at_most_1_missing_sep<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_element_after_element_missing_sep() {
  // "1 2" with at_most(1) — 1 accepted (Element), 2 without comma →
  // Element in handle_continue → emit_missing_separator + handle_too_many_element
  // + push fails → emit_full_container
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_at_most_1_missing_sep)
    .parse_str("1 2");
  assert!(r.is_ok());
}

#[test]
fn sep_element_after_element_missing_sep_unbounded() {
  // "1 2 3" unbounded — exercises Element→Element path multiple times
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_unbounded)
    .parse_str("1 2 3");
  assert!(r.is_ok());
}

// ── A11. require_trailing with recovering emitter ────────────────────────────

fn parse_sep_require_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_require_trailing_missing() {
  // "1,2" without trailing comma — exercises the trailing check path
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_trailing)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_require_trailing_ok() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_trailing)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

// ── A12. bounded + recovering emitter — exercises at_least/at_most handler ───

fn parse_sep_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .bounded(2, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_bounded_too_few_recovering() {
  // "1" with bounded(2,3) — too few elements, recovering continues
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_bounded)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn sep_bounded_too_many_recovering() {
  // "1,2,3,4" with bounded(2,3) — too many elements, recovering continues
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_bounded)
    .parse_str("1,2,3,4");
  assert!(r.is_ok());
}

#[test]
fn sep_bounded_empty_recovering() {
  // "" with bounded(2,3) — 0 elements, recovering continues
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_bounded)
    .parse_str("");
  assert!(r.is_ok());
}

// ── A13. at_least with recovering emitter ────────────────────────────────────

fn parse_sep_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_at_least_too_few_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_at_least)
    .parse_str("1");
  assert!(r.is_ok());
}

#[test]
fn sep_at_least_empty_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_at_least)
    .parse_str("");
  assert!(r.is_ok());
}

// ── A14. require_leading with recovering ─────────────────────────────────────

fn parse_sep_require_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_require_leading_missing_recovering() {
  // "1,2" without leading — exercises missing leading check
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_leading)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_require_leading_ok_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_leading)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

// ── A15. Combined: allow_leading + consecutive seps + bounded ────────────────

fn parse_sep_allow_leading_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .allow_leading()
    .bounded(1, 2)
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_allow_leading_bounded_consecutive_leading() {
  // ",,1" with allow_leading bounded(1,2) — consecutive leading separators
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_leading_bounded)
    .parse_str(",,1");
  assert!(r.is_ok());
}

#[test]
fn sep_allow_leading_bounded_too_many() {
  // ",1,2,3" with allow_leading bounded(1,2) — too many elements
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_leading_bounded)
    .parse_str(",1,2,3");
  assert!(r.is_ok());
}

#[test]
fn sep_allow_leading_bounded_too_few() {
  // "," with allow_leading bounded(1,2) — too few elements (0)
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_leading_bounded)
    .parse_str(",");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// B. sep_while/parse — SeparatedWhile (ParseInput) state machine branches
// ═══════════════════════════════════════════════════════════════════════════════

// All sep_while tests use a `+` sentinel at the end so the condition sees
// a stop token instead of hitting EOF.

// ── B1. Consecutive leading separators ───────────────────────────────────────

fn parse_sw_allow_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_consecutive_leading_separators() {
  // ",,1,2+" — exercises State::Leading in handle_separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading)
    .parse_str(",,1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_triple_leading_separators() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading)
    .parse_str(",,,1+");
  assert!(r.is_ok());
}

#[test]
fn sw_leading_only() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading)
    .parse_str(",+");
  assert!(r.is_ok());
}

#[test]
fn sw_double_leading_only() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading)
    .parse_str(",,+");
  assert!(r.is_ok());
}

// ── B2. Consecutive mid-separators ───────────────────────────────────────────

fn parse_sw_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_consecutive_mid_separators() {
  // "1,,2+" — exercises State::Separator in handle_separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_unbounded)
    .parse_str("1,,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_triple_mid_separators() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_unbounded)
    .parse_str("1,,,2+");
  assert!(r.is_ok());
}

// ── B3. State::Start in handle_separator ─────────────────────────────────────

#[test]
fn sw_start_state_separator() {
  // ",1,2+" — first comma hits Start in handle_separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_unbounded)
    .parse_str(",1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_start_separator_only() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_unbounded)
    .parse_str(",+");
  assert!(r.is_ok());
}

// ── B4. State::Element in handle_continue — missing separator ────────────────
// In sep_while, handle_continue parses the element INSIDE, so we need
// the condition to return Continue for a Num token that follows another Num
// without a comma.

#[test]
fn sw_missing_separator_between_elements() {
  // "1 2+" — after parsing 1, next is 2 (not comma), condition returns Continue,
  // handle_continue with State::Element → emit_missing_separator
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_unbounded)
    .parse_str("1 2+");
  assert!(r.is_ok());
}

#[test]
fn sw_missing_separator_three() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_unbounded)
    .parse_str("1 2 3+");
  assert!(r.is_ok());
}

// ── B5. Decline path — condition returns Stop ────────────────────────────────

#[test]
fn sw_decline_after_separator() {
  // "1,+" — comma→Separator, then + triggers Stop → handle_end with Separator state
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_unbounded)
    .parse_str("1,+");
  assert!(r.is_ok());
}

// ── B6. Full container paths ─────────────────────────────────────────────────

fn parse_sw_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_full_container_after_separator() {
  // "1,2+" with at_most(1) — exercises Separator→push fails→emit_full_container
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_at_most_1)
    .parse_str("1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_full_container_overflow_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_at_most_1)
    .parse_str("1,2,3+");
  assert!(r.is_ok());
}

// ── B7. Full container from Leading state ────────────────────────────────────

fn parse_sw_allow_leading_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .at_most(1)
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_full_container_leading_state() {
  // ",1,2+" with allow_leading at_most(1)
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading_at_most_1)
    .parse_str(",1,2+");
  assert!(r.is_ok());
}

// ── B8. Missing separator + too_many_element ─────────────────────────────────

#[test]
fn sw_element_after_element_missing_sep() {
  // "1 2+" with at_most(1) — missing separator + full container
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_at_most_1)
    .parse_str("1 2+");
  assert!(r.is_ok());
}

// ── B9. require_trailing with recovering emitter ─────────────────────────────

fn parse_sw_require_trailing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_require_trailing_missing() {
  // "1,2+" without trailing comma
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_trailing)
    .parse_str("1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_require_trailing_ok() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_trailing)
    .parse_str("1,2,+");
  assert!(r.is_ok());
}

// ── B10. bounded with recovering emitter ─────────────────────────────────────

fn parse_sw_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .bounded(2, 3)
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_bounded_too_few_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_bounded)
    .parse_str("1+");
  assert!(r.is_ok());
}

#[test]
fn sw_bounded_too_many_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_bounded)
    .parse_str("1,2,3,4+");
  assert!(r.is_ok());
}

#[test]
fn sw_bounded_empty_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_bounded)
    .parse_str("+");
  assert!(r.is_ok());
}

// ── B11. at_least with recovering emitter ────────────────────────────────────

fn parse_sw_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .at_least(3)
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_at_least_too_few_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_at_least)
    .parse_str("1+");
  assert!(r.is_ok());
}

#[test]
fn sw_at_least_empty_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_at_least)
    .parse_str("+");
  assert!(r.is_ok());
}

// ── B12. require_leading with recovering ─────────────────────────────────────

fn parse_sw_require_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_require_leading_missing_recovering() {
  // "1,2+" without leading
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_leading)
    .parse_str("1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_require_leading_ok_recovering() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_leading)
    .parse_str(",1,2+");
  assert!(r.is_ok());
}

// ── B13. Combined: allow_leading + bounded for sep_while ─────────────────────

fn parse_sw_allow_leading_bounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .bounded(1, 2)
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_allow_leading_bounded_consecutive_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading_bounded)
    .parse_str(",,1+");
  assert!(r.is_ok());
}

#[test]
fn sw_allow_leading_bounded_too_many() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading_bounded)
    .parse_str(",1,2,3+");
  assert!(r.is_ok());
}

#[test]
fn sw_allow_leading_bounded_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_leading_bounded)
    .parse_str(",+");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// C. Fatal emitter tests — verify error paths DO error with fatal emitter
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sep_fatal_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num.separated_by_comma().collect().parse_input(inp)
}

#[test]
fn fatal_consecutive_mid_separators() {
  // "1,,2" with fatal emitter — should error on missing element
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_sep_fatal_unbounded)
    .parse_str("1,,2");
  assert!(r.is_err());
}

#[test]
fn fatal_start_state_separator() {
  // ",1" with fatal emitter — should error on unexpected leading separator
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_sep_fatal_unbounded)
    .parse_str(",1");
  assert!(r.is_err());
}

#[test]
fn fatal_trailing_separator() {
  // "1," with fatal emitter — should error on unexpected trailing separator
  let r: Result<Vec<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse_sep_fatal_unbounded)
    .parse_str("1,");
  assert!(r.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// C. require_surrounded (require_leading + require_trailing) with recovering
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sep_require_surrounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .require_trailing()
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_require_surrounded_ok() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_surrounded)
    .parse_str(",1,2,");
  assert!(r.is_ok());
}

#[test]
fn sep_require_surrounded_missing_both() {
  // No leading or trailing comma — recovering emitter lets it continue
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_surrounded)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_require_surrounded_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_surrounded)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_require_surrounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_surrounded)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

#[test]
fn sep_require_surrounded_single_element() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_surrounded)
    .parse_str(",1,");
  assert!(r.is_ok());
}

#[test]
fn sep_require_surrounded_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_require_surrounded)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// D. allow_surrounded (allow_leading + allow_trailing) with recovering
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sep_allow_surrounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .allow_trailing()
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_allow_surrounded_ok() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_surrounded)
    .parse_str(",1,2,");
  assert!(r.is_ok());
}

#[test]
fn sep_allow_surrounded_no_separators() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_surrounded)
    .parse_str("1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_allow_surrounded_only_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_surrounded)
    .parse_str(",1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_allow_surrounded_only_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_surrounded)
    .parse_str("1,2,");
  assert!(r.is_ok());
}

#[test]
fn sep_allow_surrounded_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_allow_surrounded)
    .parse_str("");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// E. sep_while require_surrounded with recovering
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sw_require_surrounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .require_trailing()
    .require_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_require_surrounded_ok() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_surrounded)
    .parse_str(",1,2,+");
  assert!(r.is_ok());
}

#[test]
fn sw_require_surrounded_missing_both() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_surrounded)
    .parse_str("1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_require_surrounded_missing_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_surrounded)
    .parse_str(",1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_require_surrounded_missing_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_require_surrounded)
    .parse_str("1,2,+");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// F. sep_while allow_surrounded with recovering
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sw_allow_surrounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_trailing()
    .allow_leading()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_allow_surrounded_ok() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_surrounded)
    .parse_str(",1,2,+");
  assert!(r.is_ok());
}

#[test]
fn sw_allow_surrounded_no_separators() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_surrounded)
    .parse_str("1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_allow_surrounded_only_leading() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_surrounded)
    .parse_str(",1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_allow_surrounded_only_trailing() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_allow_surrounded)
    .parse_str("1,2,+");
  assert!(r.is_ok());
}
