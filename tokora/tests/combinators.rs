#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]
mod common;

use common::E;
use tokora::EmitterView;

use common::{TestLexer, Token, TokenKind};
use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Branch, Emitter, InputRef, Lexer, Parse, ParseChoice, ParseContext, ParseInput,
  Parser, ParserContext, Token as TokenTrait, TryParseInput,
  cache::Peeked,
  emitter::FullContainerEmitter,
  error::{UnexpectedEot, syntax::FullContainer, token::UnexpectedToken},
  input::Cursor,
  parser::{Action, Any, Empty, expect, try_expect},
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::Expected,
};

// ── Full emitter (needed for repeated .collect()) ───────────────────────────

struct FullEmitter;

impl<'inp> Emitter<'inp, TestLexer<'inp>> for FullEmitter {
  type Error = E;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(E)
  }

  fn emit_unexpected_token(
    &mut self,
    _: tokora::error::token::UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(E)
  }

  fn emit_error(&mut self, err: Spanned<E, <TestLexer<'inp> as Lexer<'inp>>::Span>) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(err.into_data())
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, _: u64)
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
  }
}

impl<'inp> FullContainerEmitter<'inp, TestLexer<'inp>> for FullEmitter {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), E>
  where
    TestLexer<'inp>: Lexer<'inp>,
  {
    Err(E)
  }
}

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, FullEmitter> {
  ParserContext::new(FullEmitter)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
{
  expect(|t: &Token| {
    if matches!(t, Token::Num(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Num))
    }
  })
  .parse_input(inp)
  .map(|t| match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  })
}

fn try_num_e<'inp, Ctx>(
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

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, ()>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
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

fn while_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
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

// ── expect.rs: TryParseInput impls for Expect ───────────────────────────────

#[test]
fn expect_try_parse_input_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_expect(|t: &Token| matches!(t, Token::Plus)).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(matches!(result, ParseAttempt::Accept(Token::Plus)));
}

#[test]
fn expect_try_parse_input_decline_mismatch() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_expect(|t: &Token| matches!(t, Token::Plus)).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn expect_try_parse_input_decline_empty() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_expect(|t: &Token| matches!(t, Token::Plus)).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

// ── map.rs: TryParseInput impls ─────────────────────────────────────────────

#[test]
fn map_try_parse_input_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.accepted().map(|n| n * 10).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, ParseAttempt::Accept(420));
}

#[test]
fn map_try_parse_input_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.accepted().map(|n| n * 10).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn map_try_parse_input_empty() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.accepted().map(|n| n * 10).try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn map_with_try_parse_input_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .accepted()
      .map_with(|n, _state| n * 3)
      .try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("7").unwrap();
  assert_eq!(result, ParseAttempt::Accept(21));
}

#[test]
fn map_with_try_parse_input_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .accepted()
      .map_with(|n, _state| n * 3)
      .try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

// ── peek_then.rs: TryParseInput impl (uses peek_then_try + Decision) ────────

#[test]
fn peek_then_try_parse_input_continue() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .peek_then_try::<_, U1>(while_num::<Ctx>)
      .try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("42 10").unwrap();
  assert_eq!(result, ParseAttempt::Accept(42));
}

#[test]
fn peek_then_try_parse_input_stop() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .peek_then_try::<_, U1>(while_num::<Ctx>)
      .try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn peek_then_try_parse_input_empty() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .peek_then_try::<_, U1>(while_num::<Ctx>)
      .try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

// ── peek_then_choice.rs: TryParseInput impl ─────────────────────────────────

#[test]
fn peek_then_choice_try_parse_input_accept() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    (Any::new(), Any::new())
      .peek_then_try_choice::<_, U1>(|_toks, _| Ok(Some(Branch::B0)))
      .try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(matches!(result, ParseAttempt::Accept(Token::Plus)));
}

#[test]
fn peek_then_choice_try_parse_input_decline() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<ParseAttempt<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    (Any::new(), Any::new())
      .peek_then_try_choice::<_, U1>(|_toks, _| Ok(None))
      .try_parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, ParseAttempt::Decline);
}

#[test]
fn peek_then_choice_parse_input_b0() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    (Any::new(), Any::new())
      .peek_then_choice::<_, U1>(|_toks, _| Ok(Branch::B0))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

#[test]
fn peek_then_choice_parse_input_b1() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Token, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    (Any::new(), Any::new())
      .peek_then_choice::<_, U1>(|_toks, _| Ok(Branch::B1))
      .parse_input(inp)
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

// ── repeated/mod.rs (using full_ctx for FullContainerEmitter) ───────────────

#[test]
fn repeated_collect_basic() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = E> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    try_num_e.repeated().collect().parse_input(inp)
  }

  let result: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse)
    .parse_str("1 2 3")
    .unwrap();
  assert_eq!(result, vec![1, 2, 3]);
}

#[test]
fn repeated_collect_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = E> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    try_num_e.repeated().collect().parse_input(inp)
  }

  let result: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse)
    .parse_str("")
    .unwrap();
  assert_eq!(result, Vec::<i64>::new());
}

#[test]
fn repeated_collect_single() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = E> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    try_num_e.repeated().collect().parse_input(inp)
  }

  let result: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse)
    .parse_str("99")
    .unwrap();
  assert_eq!(result, vec![99]);
}

#[test]
fn repeated_stops_on_non_num() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = E> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    try_num_e.repeated().collect().parse_input(inp)
  }

  let result: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse)
    .parse_str("1 2 +")
    .unwrap();
  assert_eq!(result, vec![1, 2]);
}

// ── repeated_while/mod.rs ───────────────────────────────────────────────────

fn while_num_e<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
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

fn parse_num_e<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  expect(|t: &Token| {
    if matches!(t, Token::Num(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Num))
    }
  })
  .parse_input(inp)
  .map(|t| match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  })
}

#[test]
fn repeated_while_collect_basic() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = E> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num_e
      .repeated_while::<_, U1>(while_num_e::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let result: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse)
    .parse_str("1 2 3")
    .unwrap();
  assert_eq!(result, vec![1, 2, 3]);
}

#[test]
fn repeated_while_collect_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = E> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num_e
      .repeated_while::<_, U1>(while_num_e::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let result: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse)
    .parse_str("")
    .unwrap();
  assert_eq!(result, Vec::<i64>::new());
}

#[test]
fn repeated_while_stops_on_non_num() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TestLexer<'inp>, Error = E> + FullContainerEmitter<'inp, TestLexer<'inp>>,
  {
    parse_num_e
      .repeated_while::<_, U1>(while_num_e::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let result: Vec<i64> = Parser::with_context(full_ctx())
    .apply(parse)
    .parse_str("1 2 +")
    .unwrap();
  assert_eq!(result, vec![1, 2]);
}

// ── parser/mod.rs: builder methods ──────────────────────────────────────────

#[test]
fn parser_default() {
  let p: tokora::parser::Parser<
    (),
    TestLexer<'_>,
    Token,
    tokora::FatalContext<'_, TestLexer<'_>, E>,
  > = Default::default();
  let result = p.apply(Any::new()).parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

#[test]
fn parser_with_parser() {
  let p = Parser::with_parser::<TestLexer<'_>, Token, (), _, ()>(Any::new());
  let result = p.parse_str("+").unwrap();
  assert_eq!(result, Token::Plus);
}

// ── Action variants ─────────────────────────────────────────────────────────

#[test]
fn action_is_variant() {
  assert!(Action::Stop.is_stop());
  assert!(!Action::Stop.is_continue());
  assert!(Action::Continue.is_continue());
  assert!(!Action::Continue.is_stop());
}
