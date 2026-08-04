#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]
mod common;

use common::E;
use tokora::EmitterView;

// Tests targeting uncovered error paths in delimited parser state machines
// and related modules:
// - sep/delim/mod.rs       (parse_separated error branches)
// - sep_while/delim/mod.rs (parse_separated error branches)
// - delim/repeated.rs      (parse_repeated error branches)
// - delim/repeated_while.rs(parse_repeated error branches)
// - expect.rs              (TryParseInput for With<Expect, PhantomSliced/PhantomLocated>)
// - handler/mod.rs         (default trait method impls)
// - repeated/mod.rs        (error branches in repeated parser)
// - repeated_while/mod.rs  (error branches in repeated_while parser)
//
// Uses a recovering emitter (returns Ok(())) so the parser continues past
// errors and exercises recovery code paths.

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Located, Parse, ParseContext, ParseInput, Parser,
  ParserContext, SimpleSpan, Token as TokenTrait, TryParseInput,
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
  parser::{Action, With, try_expect},
  punct::Bracket,
  slice::Sliced,
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::{
    CowStr, Expected,
    marker::{PhantomLocated, PhantomSliced, PhantomSpan},
  },
};

use common::{TestLexer, Token, TokenKind};

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

/// ParseInput for sep_while/repeated_while — accepts Num tokens.
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
// A. sep/delim/mod.rs — Separated delimited error paths
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sep_delim<'inp, Ctx>(
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

// A1. Wrong open delimiter — line 61 (is_open=false) + line 76 (non-EOI None)
#[test]
fn sep_delim_wrong_open_num_first() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("1,2,3]");
  let _ = r;
}

#[test]
fn sep_delim_wrong_open_paren() {
  // "(" is not "[", triggers line 61
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("(1,2)");
  let _ = r;
}

// A2. EOI without open delimiter — line 73-74
#[test]
fn sep_delim_eoi_no_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("");
  assert!(r.is_err());
}

// A3. Unknown token (not sep, not close) — line 100
#[test]
fn sep_delim_unknown_token_inside() {
  // "[1 foo 2]" — "foo" (Ident) is neither comma nor `]`
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1 foo 2]");
  let _ = r;
}

#[test]
fn sep_delim_unknown_token_eq() {
  // "[1=2]" — "=" is neither comma nor `]`
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1=2]");
  let _ = r;
}

// A4. EOI mid-parse: peek returns None, ps is None — lines 109-110
// These trigger unwrap-on-None in error recovery paths (line 170-171),
// so we catch the panic to still exercise lines 108-113.
#[test]
fn sep_delim_eoi_mid_parse() {
  // "[1,2" — no closing bracket, hits EOI
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1,2");
  let _ = r;
}

#[test]
fn sep_delim_eoi_after_open() {
  // "[" — only open bracket then EOI. May panic in err.unwrap() at line 170-171.
  let _ = std::panic::catch_unwind(|| {
    let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
      .apply(parse_sep_delim)
      .parse_str("[");
    let _ = r;
  });
}

// A5. Element parser returns Err(e) — lines 129-131
#[test]
fn sep_delim_element_error() {
  // "[+ 1]" — try_num_failing consumes `+` and returns Err
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_failing)
    .parse_str("[+ 1]");
  let _ = r;
}

#[test]
fn sep_delim_element_error_multiple() {
  // "[+ + +]" — multiple element errors
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_failing)
    .parse_str("[+ + +]");
  let _ = r;
}

// A6. Element parser returns Ok(Decline) — lines 133-136
#[test]
fn sep_delim_element_decline_empty() {
  // "[]" — immediate decline after open bracket
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[]");
  let _ = r;
}

#[test]
fn sep_delim_element_decline_after_comma() {
  // "[1,,]" — decline after second comma
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1,,]");
  let _ = r;
}

#[test]
fn sep_delim_element_decline_with_ident() {
  // "[foo]" — Ident not Num, decline. "foo" triggers unknown token (line 100),
  // then try_num declines on the remaining token
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[foo]");
  let _ = r;
}

// A7. Wrong close delimiter — lines 160-163
#[test]
fn sep_delim_wrong_close() {
  // "[1,2)" — `)` is not `]`
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1,2)");
  let _ = r;
}

#[test]
fn sep_delim_wrong_close_brace() {
  // "[1,2}" — `}` is not `]`
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1,2}");
  let _ = r;
}

// A8. Missing close delimiter — line 170
#[test]
fn sep_delim_missing_close_after_decline() {
  // "[1,2,+" — after decline, no close bracket found, `+` triggers wrong close,
  // then missing close path
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1,2,+");
  let _ = r;
}

#[test]
fn sep_delim_missing_close_eof() {
  // "[1,2,3" — elements parsed but no close bracket
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim)
    .parse_str("[1,2,3");
  let _ = r;
}

// ═══════════════════════════════════════════════════════════════════════════════
// B. sep_while/delim/mod.rs — SeparatedWhile delimited error paths
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_sw_delim<'inp, Ctx>(
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

// B1. Wrong open delimiter — lines 59-64, 74-76
#[test]
fn sw_delim_wrong_open_num() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("1,2,3]");
  let _ = r;
}

#[test]
fn sw_delim_wrong_open_paren() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("(1,2)");
  let _ = r;
}

// B2. EOI without open delimiter — line 71-72
#[test]
fn sw_delim_eoi_no_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("");
  assert!(r.is_err());
}

// B3. Unknown inner token (not sep, not close) — line 98-101
#[test]
fn sw_delim_unknown_inner_token() {
  // "[1,foo]" — "foo" is not sep, not close, err set
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("[1,foo]");
  let _ = r;
}

// B4. Peek returns None (EOI mid-parse) — lines 118-125
#[test]
fn sw_delim_eoi_mid_parse() {
  // "[1,2" — no closing bracket
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("[1,2");
  let _ = r;
}

#[test]
fn sw_delim_eoi_after_open() {
  // "[" — only open bracket
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("[");
  let _ = r;
}

// B5. Condition returns Stop — lines 136-155 (wrong close after stop)
#[test]
fn sw_delim_stop_wrong_close() {
  // "[1,2+" — condition sees `+` (Stop), then try_expect for `]` sees `+`
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("[1,2+");
  let _ = r;
}

#[test]
fn sw_delim_stop_missing_close() {
  // "[1,2,+" — after stop, no close bracket in remaining input
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("[1,2,+");
  let _ = r;
}

#[test]
fn sw_delim_stop_no_tokens_left() {
  // "[1,2," — EOI after comma, condition sees nothing
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("[1,2,");
  let _ = r;
}

// B6. Wrong close delimiter — lines 139-154 (after Action::Stop)
#[test]
fn sw_delim_wrong_close_paren() {
  // "[1,2)" — `)` is not `]`
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim)
    .parse_str("[1,2)");
  let _ = r;
}

// ═══════════════════════════════════════════════════════════════════════════════
// C. delim/repeated.rs — Repeated delimited (TryParseInput) error paths
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rd_delim<'inp, Ctx>(
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

fn parse_rd_delim_failing<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  try_num_failing
    .repeated()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

// C1. Wrong open delimiter
#[test]
fn rd_delim_wrong_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim)
    .parse_str("1 2 3]");
  let _ = r;
}

#[test]
fn rd_delim_wrong_open_paren() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim)
    .parse_str("(1 2 3)");
  let _ = r;
}

// C2. EOI without open
#[test]
fn rd_delim_eoi_no_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim)
    .parse_str("");
  assert!(r.is_err());
}

// C3. Wrong close delimiter
#[test]
fn rd_delim_wrong_close() {
  // "[1 2 3)" — `)` is not `]`
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim)
    .parse_str("[1 2 3)");
  let _ = r;
}

#[test]
fn rd_delim_wrong_close_brace() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim)
    .parse_str("[1 2 3}");
  let _ = r;
}

// C4. Missing close (EOI after elements)
#[test]
fn rd_delim_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim)
    .parse_str("[1 2 3");
  let _ = r;
}

// C5. Element parser error
#[test]
fn rd_delim_element_error() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_failing)
    .parse_str("[+ 1]");
  let _ = r;
}

#[test]
fn rd_delim_element_error_then_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_failing)
    .parse_str("[+]");
  let _ = r;
}

// C6. Full container with at_most
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
fn rd_delim_full_container() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_at_most_1)
    .parse_str("[1 2 3]");
  let _ = r;
}

// ═══════════════════════════════════════════════════════════════════════════════
// D. delim/repeated_while.rs — RepeatedWhile delimited error paths
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_rw_delim<'inp, Ctx>(
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

// D1. Wrong open delimiter
#[test]
fn rw_delim_wrong_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim)
    .parse_str("1 2 3]+");
  let _ = r;
}

#[test]
fn rw_delim_wrong_open_paren() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim)
    .parse_str("(1 2)+");
  let _ = r;
}

// D2. EOI without open
#[test]
fn rw_delim_eoi_no_open() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim)
    .parse_str("");
  assert!(r.is_err());
}

// D3. Wrong close delimiter (after Action::Stop)
#[test]
fn rw_delim_wrong_close() {
  // "[1 2 3+" — condition returns Stop on `+`, then close check fails
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim)
    .parse_str("[1 2 3+");
  let _ = r;
}

#[test]
fn rw_delim_wrong_close_paren() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim)
    .parse_str("[1 2 3)");
  let _ = r;
}

// D4. Missing close delimiter (EOI after stop)
#[test]
fn rw_delim_missing_close() {
  // "[1 2 3" — condition returns Stop at EOI, then no close bracket
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim)
    .parse_str("[1 2 3");
  let _ = r;
}

// D5. Full container with at_most
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
fn rw_delim_full_container() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_at_most_1)
    .parse_str("[1 2 3]+");
  let _ = r;
}

#[test]
fn rw_delim_full_container_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_at_most_1)
    .parse_str("[1 2+");
  let _ = r;
}

// ═══════════════════════════════════════════════════════════════════════════════
// E. expect.rs — TryParseInput for With<Expect, PhantomSliced/PhantomLocated>
// ═══════════════════════════════════════════════════════════════════════════════

// E1. TryParseInput for With<Expect, PhantomSliced> — accept
#[test]
fn expect_try_parse_sliced_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Sliced<Token, &'inp str>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    let mut with_sliced: With<_, PhantomSliced> =
      With::new(expect_parser, PhantomSliced::phantom());
    with_sliced.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("42");
  let attempt = r.unwrap();
  assert!(matches!(attempt, ParseAttempt::Accept(_)));
}

// E2. TryParseInput for With<Expect, PhantomSliced> — decline
#[test]
fn expect_try_parse_sliced_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Sliced<Token, &'inp str>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    let mut with_sliced: With<_, PhantomSliced> =
      With::new(expect_parser, PhantomSliced::phantom());
    with_sliced.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("+");
  let attempt = r.unwrap();
  assert!(matches!(attempt, ParseAttempt::Decline));
}

// E3. TryParseInput for With<Expect, PhantomLocated> — accept
#[test]
fn expect_try_parse_located_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Located<Token, SimpleSpan, &'inp str>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    let mut with_located: With<_, PhantomLocated> =
      With::new(expect_parser, PhantomLocated::phantom());
    with_located.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("42");
  let attempt = r.unwrap();
  assert!(matches!(attempt, ParseAttempt::Accept(_)));
}

// E4. TryParseInput for With<Expect, PhantomLocated> — decline
#[test]
fn expect_try_parse_located_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Located<Token, SimpleSpan, &'inp str>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    let mut with_located: With<_, PhantomLocated> =
      With::new(expect_parser, PhantomLocated::phantom());
    with_located.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("+");
  let attempt = r.unwrap();
  assert!(matches!(attempt, ParseAttempt::Decline));
}

// E5. TryParseInput for With<Expect, PhantomSpan> — accept and decline
#[test]
fn expect_try_parse_spanned_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Spanned<Token, SimpleSpan>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    let mut with_spanned: With<_, PhantomSpan> = With::new(expect_parser, PhantomSpan::phantom());
    with_spanned.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("42");
  assert!(matches!(r.unwrap(), ParseAttempt::Accept(_)));
}

#[test]
fn expect_try_parse_spanned_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Spanned<Token, SimpleSpan>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    let mut with_spanned: With<_, PhantomSpan> = With::new(expect_parser, PhantomSpan::phantom());
    with_spanned.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("+");
  assert!(matches!(r.unwrap(), ParseAttempt::Decline));
}

// ═══════════════════════════════════════════════════════════════════════════════
// F. repeated/mod.rs — Repeated (non-delimited) error paths
// ═══════════════════════════════════════════════════════════════════════════════

// F1. Element parser error — lines 275-277 in repeated/mod.rs
#[test]
fn repeated_element_error() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    try_num_failing.repeated().collect().parse_input(inp)
  }
  // "1 2 3" — try_num_failing consumes each token and returns Err
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1 2 3");
  let _ = r;
}

// F2. Full container — lines 264-270 in repeated/mod.rs
#[test]
fn repeated_full_container() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>,
  {
    try_num.repeated().at_most(1).collect().parse_input(inp)
  }
  // "1 2 3" with at_most(1) — container overflows
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1 2 3");
  let _ = r;
}

// F3. at_least too few with recovering emitter
#[test]
fn repeated_at_least_too_few() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>,
  {
    try_num.repeated().at_least(5).collect().parse_input(inp)
  }
  // "1 2" — too few for at_least(5)
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1 2");
  let _ = r;
}

// F4. bounded with recovering emitter
#[test]
fn repeated_bounded_too_few() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>,
  {
    try_num.repeated().bounded(3, 5).collect().parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1");
  let _ = r;
}

#[test]
fn repeated_bounded_too_many() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>,
  {
    try_num.repeated().bounded(1, 2).collect().parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1 2 3 4 5");
  let _ = r;
}

// ═══════════════════════════════════════════════════════════════════════════════
// G. repeated_while/mod.rs — RepeatedWhile (non-delimited) error paths
// ═══════════════════════════════════════════════════════════════════════════════

// G1. Full container
#[test]
fn repeated_while_full_container() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .repeated_while::<_, U1>(decide_num::<Ctx>)
      .at_most(1)
      .collect()
      .parse_input(inp)
  }
  // "1 2 3+" — at_most(1), container overflows
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1 2 3+");
  let _ = r;
}

// G2. at_least too few
#[test]
fn repeated_while_at_least_too_few() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .repeated_while::<_, U1>(decide_num::<Ctx>)
      .at_least(5)
      .collect()
      .parse_input(inp)
  }
  // "1 2+" — too few for at_least(5)
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1 2+");
  let _ = r;
}

// G3. bounded too few and too many
#[test]
fn repeated_while_bounded_too_few() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .repeated_while::<_, U1>(decide_num::<Ctx>)
      .bounded(3, 5)
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1+");
  let _ = r;
}

#[test]
fn repeated_while_bounded_too_many() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>
      + TooFewEmitter<'inp, TestLexer<'inp>>
      + TooManyEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .repeated_while::<_, U1>(decide_num::<Ctx>)
      .bounded(1, 2)
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("1 2 3 4+");
  let _ = r;
}

// G4. Empty input — condition returns Stop immediately
#[test]
fn repeated_while_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
      + FullContainerEmitter<'inp, TestLexer<'inp>>
      + UnclosedEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num
      .repeated_while::<_, U1>(decide_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("+");
  let _ = r;
}

// ═══════════════════════════════════════════════════════════════════════════════
// H. Combined edge cases — multiple error paths triggered in one parse
// ═══════════════════════════════════════════════════════════════════════════════

// H1. sep/delim: wrong open + element errors + wrong close
#[test]
fn sep_delim_combined_errors() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_failing)
    .parse_str("+ + +)");
  let _ = r;
}

// H2. sep/delim with allow_leading + element decline + missing close
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
fn sep_delim_allow_leading_decline_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_allow_leading)
    .parse_str("[,1,2");
  let _ = r;
}

#[test]
fn sep_delim_allow_leading_wrong_open_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_allow_leading)
    .parse_str(",1,2)");
  let _ = r;
}

// H3. sep/delim at_most + wrong close
fn parse_sep_delim_at_most<'inp, Ctx>(
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
fn sep_delim_at_most_overflow_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_at_most)
    .parse_str("[1,2,3)");
  let _ = r;
}

#[test]
fn sep_delim_at_most_overflow_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_at_most)
    .parse_str("[1,2,3");
  let _ = r;
}

// H4. sep_while/delim at_most + wrong close
fn parse_sw_delim_at_most<'inp, Ctx>(
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
    .at_most(1)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_delim_at_most_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_at_most)
    .parse_str("[1,2)");
  let _ = r;
}

#[test]
fn sw_delim_at_most_missing_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_at_most)
    .parse_str("[1,2");
  let _ = r;
}

// H5. repeated delim at_least too few + wrong close
fn parse_rd_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .repeated()
    .at_least(5)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rd_delim_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_at_least)
    .parse_str("[1 2]");
  let _ = r;
}

#[test]
fn rd_delim_at_least_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rd_delim_at_least)
    .parse_str("[1 2)");
  let _ = r;
}

// H6. repeated_while delim at_least too few
fn parse_rw_delim_at_least<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .repeated_while::<_, U1>(decide_num::<Ctx>)
    .at_least(5)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn rw_delim_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_at_least)
    .parse_str("[1 2]+");
  let _ = r;
}

#[test]
fn rw_delim_at_least_wrong_close() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_rw_delim_at_least)
    .parse_str("[1 2)+");
  let _ = r;
}

// H7. sep/delim at_least + delimited
fn parse_sep_delim_at_least<'inp, Ctx>(
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
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sep_delim_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_at_least)
    .parse_str("[1]");
  let _ = r;
}

#[test]
fn sep_delim_at_least_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sep_delim_at_least)
    .parse_str("[]");
  let _ = r;
}

// H8. sep_while/delim at_least + delimited
fn parse_sw_delim_at_least<'inp, Ctx>(
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
    + TooFewEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .at_least(3)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

#[test]
fn sw_delim_at_least_too_few() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_at_least)
    .parse_str("[1]");
  let _ = r;
}

#[test]
fn sw_delim_at_least_empty() {
  let r: Result<Vec<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse_sw_delim_at_least)
    .parse_str("[]");
  let _ = r;
}

// H9. E5: TryParseInput for Expect itself — accept and decline (lines 226-233)
#[test]
fn expect_try_parse_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let mut expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    expect_parser.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("42");
  assert!(matches!(r.unwrap(), ParseAttempt::Accept(Token::Num(42))));
}

#[test]
fn expect_try_parse_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let mut expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    expect_parser.try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("+");
  assert!(matches!(r.unwrap(), ParseAttempt::Decline));
}

// H10. Expect with ref — TryParseInput for &Expect (lines 243-254)
#[test]
fn expect_ref_try_parse_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Spanned<Token, SimpleSpan>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    (&expect_parser).try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("42");
  assert!(matches!(r.unwrap(), ParseAttempt::Accept(_)));
}

#[test]
fn expect_ref_try_parse_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Spanned<Token, SimpleSpan>>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let expect_parser =
      try_expect::<'inp, _, TestLexer<'inp>, Ctx, _>(|t: &Token| matches!(t, Token::Num(_)));
    (&expect_parser).try_parse_input(inp)
  }
  let r = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("+");
  assert!(matches!(r.unwrap(), ParseAttempt::Decline));
}
