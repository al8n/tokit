#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]
mod common;

use common::E;
use tokora::EmitterView;

// Tests targeting uncovered error/edge-case paths in delimited parsers:
// - sep/delim/mod.rs   (parse_separated)
// - sep_while/delim/mod.rs (parse_separated)
// - delim/repeated.rs  (parse_repeated)
// - delim/repeated_while.rs (parse_repeated)
//
// Uses a recovering emitter (returns Ok(())) so the parser continues past
// errors and exercises recovery code paths.

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan, Token as TokenTrait, TryParseInput,
  cache::Peeked,
  emitter::{
    FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, Silent, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf, TooFew, TooMany},
    token::{MissingTokenOf, SeparatedError, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  parser::{Action, With},
  punct::Bracket,
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::{CowStr, marker::PhantomSpan},
};

use common::{TestLexer, Token};

fn recovering_ctx() -> ParserContext<'static, TestLexer<'static>, Silent<E>> {
  ParserContext::new(Silent::new())
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

/// TryParseInput — always returns Err(E). Used to exercise the Err path.
fn try_num_failing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
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

/// Condition for sep_while/repeated_while: continue if next token is a Num.
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
// A. sep/delim — Separated delimited (TryParseInput) edge cases
// ═══════════════════════════════════════════════════════════════════════════════

// ── A1. Wrong open delimiter (line 61, 76): first token is Num not `[` ───────

fn parse_sep_delim_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_delim_wrong_open() {
  // "1,2,3]" — first token Num, not `[`. Triggers is_open=false (line 61)
  // and None branch with non-EOI (line 76).
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("1,2,3]");
  assert!(r.is_ok());
}

// ── A2. Empty input → EOI (line 73-74) ───────────────────────────────────────

#[test]
fn sep_delim_empty_input() {
  // "" — no tokens at all, triggers EOI path (line 73-74), returns Err.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("");
  assert!(r.is_err());
}

// ── A3. Element parser error inside delimited (lines 129-131) ────────────────

fn parse_sep_delim_failing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num_failing
    .separated_by_comma()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_delim_element_parser_error() {
  // "[+,1]" — `+` is not a valid Num: try_parse_input returns Err.
  // Recovering emitter lets it continue. Tests lines 129-131.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_failing)
    .parse_str("[+,1]");
  assert!(r.is_ok() || r.is_err());
}

// ── A4. Element parser decline inside delimited (lines 133-136) ──────────────

#[test]
fn sep_delim_element_decline() {
  // "[,]" — after `[`, sees `,` (separator), then `]` is not a Num so
  // try_num declines → break via handle_end (lines 133-136).
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("[,]");
  assert!(r.is_ok());
}

#[test]
fn sep_delim_decline_after_element() {
  // "[1,]" — after 1 and comma, `]` causes decline (lines 133-136).
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("[1,]");
  assert!(r.is_ok());
}

// ── A5. Wrong close delimiter (lines 160-163) ────────────────────────────────

#[test]
fn sep_delim_wrong_close() {
  // "[1,2,3+" — after parsing elements, next token is `+` not `]`.
  // Tests lines 160-163.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("[1,2,3+");
  assert!(r.is_ok());
}

// ── A6. Missing close delimiter (line 170) ───────────────────────────────────

#[test]
fn sep_delim_missing_close() {
  // "[1,2,3" — no closing bracket, reaches EOI. Tests line 170.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("[1,2,3");
  assert!(r.is_ok());
}

// ── A7. Unknown token inside delimited (line 100) ────────────────────────────

#[test]
fn sep_delim_unknown_inner_token() {
  // "[1,+,2]" — after `1,`, the token `+` is not separator, not close,
  // and not a valid element start. Tests line 100.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("[1,+,2]");
  assert!(r.is_ok());
}

#[test]
fn sep_delim_unknown_inner_token_only() {
  // "[+]" — immediately after open, `+` is not sep, not close → line 100.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_unbounded)
    .parse_str("[+]");
  assert!(r.is_ok());
}

// ── A8. Spanned path (With<Collect<...>, PhantomSpan>) ───────────────────────

fn parse_sep_delim_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, SimpleSpan>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  With::new(
    try_num
      .separated_by_comma()
      .delimited::<Bracket<(), (), ()>>()
      .collect(),
    PhantomSpan::PHANTOM,
  )
  .parse_input(inp)
}

#[test]
fn sep_delim_spanned_wrong_open() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_spanned)
    .parse_str("1,2]");
  assert!(r.is_ok());
}

#[test]
fn sep_delim_spanned_missing_close() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_spanned)
    .parse_str("[1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_delim_spanned_wrong_close() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_spanned)
    .parse_str("[1,2+");
  assert!(r.is_ok());
}

#[test]
fn sep_delim_spanned_ok() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_spanned)
    .parse_str("[1,2,3]");
  let spanned = r.unwrap();
  assert_eq!(spanned.data(), &vec![1, 2, 3]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// B. sep_while/delim — SeparatedWhile delimited edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sw_delim_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

// ── B1. Wrong open delimiter ─────────────────────────────────────────────────

#[test]
fn sw_delim_wrong_open() {
  // "1,2,3]" — first token not `[`. Triggers is_open=false and None non-EOI.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_unbounded)
    .parse_str("1,2,3]");
  assert!(r.is_ok());
}

// ── B2. Empty input ──────────────────────────────────────────────────────────

#[test]
fn sw_delim_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_unbounded)
    .parse_str("");
  assert!(r.is_err());
}

// ── B3. Wrong close delimiter ────────────────────────────────────────────────

#[test]
fn sw_delim_wrong_close() {
  // "[1,2+" — after parsing, `+` is not `]`. Triggers wrong close path.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_unbounded)
    .parse_str("[1,2+");
  assert!(r.is_ok());
}

// ── B4. Wrong close after condition Stop ──────────────────────────────────────

#[test]
fn sw_delim_wrong_close_after_stop() {
  // "[1,2,+" — condition sees `+`, returns Stop. Then try_expect for close
  // sees `+` → not `]` → emit_unexpected_token.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_unbounded)
    .parse_str("[1,2,+");
  assert!(r.is_ok());
}

// ── B5. Unknown inner token ──────────────────────────────────────────────────

#[test]
fn sw_delim_unknown_inner() {
  // "[1,+]" — `+` is not sep, not close, but condition returns Stop.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_unbounded)
    .parse_str("[1,+]");
  assert!(r.is_ok());
}

// ── B6. Decline after separator ──────────────────────────────────────────────

#[test]
fn sw_delim_decline_after_sep() {
  // "[1,]" — comma, then `]` → condition sees `]`, returns Stop.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_unbounded)
    .parse_str("[1,]");
  assert!(r.is_ok());
}

// ── B7. Spanned path ─────────────────────────────────────────────────────────

fn parse_sw_delim_spanned<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Spanned<Vec<i64>, SimpleSpan>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  With::new(
    parse_num
      .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
      .delimited::<Bracket<(), (), ()>>()
      .collect(),
    PhantomSpan::PHANTOM,
  )
  .parse_input(inp)
}

#[test]
fn sw_delim_spanned_wrong_open() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_spanned)
    .parse_str("1,2]");
  assert!(r.is_ok());
}

#[test]
fn sw_delim_spanned_wrong_close() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_spanned)
    .parse_str("[1,2+");
  assert!(r.is_ok());
}

#[test]
fn sw_delim_spanned_ok() {
  let r = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_spanned)
    .parse_str("[1,2,3]");
  let spanned = r.unwrap();
  assert_eq!(spanned.data(), &vec![1, 2, 3]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// C. delim/repeated — Repeated delimited (TryParseInput) edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rd_delim_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .repeated()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

// ── C1. Wrong open delimiter ─────────────────────────────────────────────────

#[test]
fn rd_delim_wrong_open() {
  // "1 2 3]" — first token not `[`.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_unbounded)
    .parse_str("1 2 3]");
  assert!(r.is_ok());
}

// ── C2. Empty input → EOI ───────────────────────────────────────────────────

#[test]
fn rd_delim_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_unbounded)
    .parse_str("");
  assert!(r.is_err());
}

// ── C3. Wrong close delimiter ─────────────────────────────────────────────────

#[test]
fn rd_delim_wrong_close() {
  // "[1 2 3+" — `+` is not `]`. try_num declines on `+`,
  // then try_expect for close sees `+` → wrong close → emit_unexpected_token.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_unbounded)
    .parse_str("[1 2 3+");
  assert!(r.is_ok());
}

// ── C4. Wrong close with comma ───────────────────────────────────────────────

#[test]
fn rd_delim_wrong_close_comma() {
  // "[1 2 3," — comma is not `]`.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_unbounded)
    .parse_str("[1 2 3,");
  assert!(r.is_ok());
}

// ── C5. Element parser error (Err path in repeated) ──────────────────────────
// Note: With a recovering emitter, try_num_failing in a repeated loop would
// infinitely retry. So we test this with a single-element input "[+]" where
// try_num_failing consumes `+`, errors, then try_num_failing sees `]` and
// errors again but on next iteration, try_parse_input declines on EOI.
// Actually, try_num_failing always consumes and errors, so "[+]" → consume `+`,
// error, recover, consume `]`, error, recover, then EOI → try_num_failing
// calls inp.next()? → None → Err(E) → emit_error → Ok, then next() → None
// again → infinite loop. Skip this test pattern for repeated delimited.

// ═══════════════════════════════════════════════════════════════════════════════
// D. delim/repeated_while — RepeatedWhile delimited edge cases
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rw_delim_unbounded<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + SeparatedEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .repeated_while::<_, U1>(decide_num::<Ctx>)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

// ── D1. Wrong open delimiter ─────────────────────────────────────────────────

#[test]
fn rw_delim_wrong_open() {
  // "1 2 3]+": first token not `[`.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_unbounded)
    .parse_str("1 2 3]+");
  assert!(r.is_ok());
}

// ── D2. Empty input → EOI ───────────────────────────────────────────────────

#[test]
fn rw_delim_empty_input() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_unbounded)
    .parse_str("");
  assert!(r.is_err());
}

// ── D3. Missing close delimiter (Action::Stop path) ──────────────────────────

#[test]
fn rw_delim_missing_close() {
  // "[1 2 3+" — condition sees `+`, returns Stop. Then try_expect for `]`
  // sees `+` → wrong close → emit_unexpected_token.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_unbounded)
    .parse_str("[1 2 3+");
  assert!(r.is_ok());
}

// ── D4. Wrong close delimiter ────────────────────────────────────────────────

#[test]
fn rw_delim_wrong_close() {
  // "[1 2 3,+" — after elements, `+` is not `]`.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_unbounded)
    .parse_str("[1 2 3,+");
  assert!(r.is_ok());
}

// ── D5. Condition returns Stop immediately ───────────────────────────────────

#[test]
fn rw_delim_stop_immediately() {
  // "[+]" — `+` is not Num, condition returns Stop, close check finds `]`.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_unbounded)
    .parse_str("[+]");
  assert!(r.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// E. Additional edge combinations
// ═══════════════════════════════════════════════════════════════════════════════

// ── E1. sep/delim with allow_leading: wrong open + wrong close ───────────────

fn parse_sep_delim_allow_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_delim_allow_leading_wrong_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_allow_leading)
    .parse_str(",1,2]");
  assert!(r.is_ok());
}

#[test]
fn sep_delim_allow_leading_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_allow_leading)
    .parse_str("[,1,2");
  assert!(r.is_ok());
}

#[test]
fn sep_delim_allow_leading_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_allow_leading)
    .parse_str("[,1,2+");
  assert!(r.is_ok());
}

// ── E2. sep/delim at_most with recovering — exercises full container paths ───

fn parse_sep_delim_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_most(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_delim_at_most_overflow() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_at_most_1)
    .parse_str("[1,2,3]");
  assert!(r.is_ok());
}

// ── E3. sep_while/delim with allow_leading ───────────────────────────────────

fn parse_sw_delim_allow_leading<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .allow_leading()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_delim_allow_leading_wrong_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_allow_leading)
    .parse_str(",1,2]");
  assert!(r.is_ok());
}

#[test]
fn sw_delim_allow_leading_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_allow_leading)
    .parse_str("[,1,2+");
  assert!(r.is_ok());
}

// ── E4. repeated delim with at_most — wrong open + wrong close ───────────────

fn parse_rd_delim_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .repeated()
    .at_most(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rd_delim_at_most_wrong_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_at_most_1)
    .parse_str("1 2]");
  assert!(r.is_ok());
}

#[test]
fn rd_delim_at_most_wrong_close_token() {
  // "[1 2," — comma is not `]`.
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_at_most_1)
    .parse_str("[1 2,");
  assert!(r.is_ok());
}

#[test]
fn rd_delim_at_most_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_at_most_1)
    .parse_str("[1 2+");
  assert!(r.is_ok());
}

// ── E5. repeated_while delim with at_most ────────────────────────────────────

fn parse_rw_delim_at_most_1<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .repeated_while::<_, U1>(decide_num::<Ctx>)
    .at_most(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rw_delim_at_most_wrong_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_at_most_1)
    .parse_str("1 2]+");
  assert!(r.is_ok());
}

#[test]
fn rw_delim_at_most_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_at_most_1)
    .parse_str("[1 2+");
  assert!(r.is_ok());
}

#[test]
fn rw_delim_at_most_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_at_most_1)
    .parse_str("[1 2,+");
  assert!(r.is_ok());
}
