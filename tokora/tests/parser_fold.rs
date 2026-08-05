#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests for fold, try_fold, fold_while, try_fold_while, rfold, rfold_while combinators.

mod common;

use common::{TestLexer, Token, TokenKind};
use generic_arraydeque::typenum::U1;
use tokora::EmitterView;
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, TryParseInput,
  cache::Peeked,
  parser::{Action, expect},
  try_parse_input::ParseAttempt,
  utils::Expected,
};

// ── helpers ───────────────────────────────────────────────────────────────────

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
  .map(|t| match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  })
  .parse_input(inp)
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

// Condition: continue while next token is Num, stop otherwise.
// Uses same pattern as decide_num in parser_sep_while.rs.
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

// ── TryParseInput::fold ───────────────────────────────────────────────────────

#[test]
fn fold_sum_multiple_numbers() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.fold(|| 0i64, |acc, x| acc + x).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("1 2 3 4").unwrap(), 10);
}

#[test]
fn fold_empty_input_returns_init() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.fold(|| 99i64, |acc, x| acc + x).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("").unwrap(), 99);
}

#[test]
fn fold_single_number() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.fold(|| 0i64, |acc, x| acc + x).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("42").unwrap(), 42);
}

#[test]
fn fold_product() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.fold(|| 1i64, |acc, x| acc * x).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("2 3 4").unwrap(), 24);
}

#[test]
fn fold_stops_at_non_num() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.fold(|| 0i64, |acc, x| acc + x).parse_input(inp)
  }
  // Stops after "1 2" when it sees "+"
  assert_eq!(Parser::new().apply(p).parse_str("1 2 +").unwrap(), 3);
}

#[test]
fn fold_zero_width_element_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Accepts without ever touching `inp`, up to a budget: a driver missing the no-progress
    // guard loops until the budget runs out instead of the input running out.
    let mut budget = 5usize;
    let elem = move |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        ParseAttempt::Accept(1)
      } else {
        ParseAttempt::Decline
      })
    };
    elem.fold(|| 0usize, |acc, x| acc + x).parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(result, 1);
}

// ── TryParseInput::try_fold ───────────────────────────────────────────────────

#[test]
fn try_fold_sum_multiple_numbers() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .try_fold(|| 0i64, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("5 10 15").unwrap(), 30);
}

#[test]
fn try_fold_empty_input() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .try_fold(|| 100i64, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("").unwrap(), 100);
}

#[test]
fn try_fold_accumulator_fails_propagates_error() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .try_fold(|| 0i64, |_acc, x| if x > 10 { Err(()) } else { Ok(x) })
      .parse_input(inp)
  }
  // 5 is fine, then 20 causes failure
  assert!(Parser::new().apply(p).parse_str("5 20").is_err());
  assert_eq!(Parser::new().apply(p).parse_str("3 5").unwrap(), 5);
}

#[test]
fn try_fold_zero_width_element_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut budget = 5usize;
    let elem = move |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        ParseAttempt::Accept(1)
      } else {
        ParseAttempt::Decline
      })
    };
    elem
      .try_fold(|| 0usize, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(result, 1);
}

// ── ParseInput::fold_while ────────────────────────────────────────────────────

#[test]
fn fold_while_sum_while_num() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("1 2 3 +").unwrap(), 6);
}

#[test]
fn fold_while_stops_immediately_on_non_num() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }
  // "+" is not a Num, stops immediately with init value
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), 0);
}

#[test]
fn fold_while_empty_input_returns_init() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 42i64, |acc, x| acc + x)
      .parse_input(inp)
  }
  // EOF peeks as None → stops immediately
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), 42);
}

#[test]
fn fold_while_single_element() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("7 +").unwrap(), 7);
}

#[test]
fn fold_while_zero_width_element_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Continues unconditionally, up to a budget, ignoring the peeked window entirely.
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>|
          -> Result<Action, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<usize, ()> { Ok(1) };
    elem
      .fold_while::<_, _, _, U1>(cond, || 0usize, |acc, x| acc + x)
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(result, 1);
}

// ── ParseInput::try_fold_while ────────────────────────────────────────────────

#[test]
fn try_fold_while_sum_with_fallible_acc() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("10 20 30 +").unwrap(), 60);
}

#[test]
fn try_fold_while_acc_fails_propagates_error() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while::<_, _, _, U1>(
        while_num::<Ctx>,
        || 0i64,
        |_acc, x| if x > 5 { Err(()) } else { Ok(x) },
      )
      .parse_input(inp)
  }
  assert!(Parser::new().apply(p).parse_str("3 10 +").is_err());
}

#[test]
fn try_fold_while_empty_returns_init() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .try_fold_while::<_, _, _, U1>(while_num::<Ctx>, || 55i64, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), 55);
}

#[test]
fn try_fold_while_zero_width_element_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>|
          -> Result<Action, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<usize, ()> { Ok(1) };
    elem
      .try_fold_while::<_, _, _, U1>(cond, || 0usize, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(result, 1);
}

// ── rfold_while ───────────────────────────────────────────────────────────────
// rfold processes elements right-to-left (last element first).
// With acc = |acc, x| acc * 2 + x, the result differs from left fold.
// fold([1,2,3], 0, |acc, x| acc*2+x) = ((0*2+1)*2+2)*2+3 = 11
// rfold([1,2,3], 0, |acc, x| acc*2+x) = 3 first, then 2, then 1:
//   0*2+3=3, 3*2+2=8, 8*2+1=17

#[test]
fn rfold_while_reverses_fold() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .rfold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc * 2 + x)
      .parse_input(inp)
  }
  // rfold processes right-to-left: 3 first, then 2, then 1
  // 0*2+3=3, 3*2+2=8, 8*2+1=17
  let result = Parser::new().apply(p).parse_str("1 2 3 +").unwrap();
  assert_eq!(result, 17);
}

#[test]
fn rfold_while_empty_returns_init() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    parse_num
      .rfold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("+").unwrap(), 0);
}

#[test]
fn rfold_while_zero_width_element_does_not_buffer_unboundedly() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>|
          -> Result<Action, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<usize, ()> { Ok(1) };
    elem
      .rfold_while::<_, _, _, U1>(cond, || 0usize, |acc, x| acc + x)
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(result, 1);
}

// ── rfold (alloc-based) ───────────────────────────────────────────────────────
// rfold on TryParseInput: also processes right-to-left.

#[test]
fn rfold_reverses_accumulated() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // rfold([1,2,3], 0, |acc,x| acc*2+x): 0*2+3=3, 3*2+2=8, 8*2+1=17
    try_num
      .rfold(|| 0i64, |acc, x| acc * 2 + x)
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("1 2 3").unwrap();
  assert_eq!(result, 17);
}

#[test]
fn rfold_empty_returns_init() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.rfold(|| 99i64, |acc, x| acc + x).parse_input(inp)
  }
  assert_eq!(Parser::new().apply(p).parse_str("").unwrap(), 99);
}

#[test]
fn rfold_single_element() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .rfold(|| 0i64, |acc, x| acc * 2 + x)
      .parse_input(inp)
  }
  // single element [5]: 0*2+5=5
  assert_eq!(Parser::new().apply(p).parse_str("5").unwrap(), 5);
}

#[test]
fn rfold_stops_at_non_num() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num.rfold(|| 0i64, |acc, x| acc + x).parse_input(inp)
  }
  // Stops at "+" and sums [1, 2] from right = 1+2 = 3
  assert_eq!(Parser::new().apply(p).parse_str("1 2 +").unwrap(), 3);
}

/// Family pin for the buffered-fold class: `RFold` buffers its elements and drains them on a
/// single tail, so the drain must run on the *decline* exit too and must run in reverse. The
/// two pre-existing pins each cover one half — `rfold_reverses_accumulated` is order-sensitive
/// but consumes its whole input, `rfold_stops_at_non_num` has the decline exit but a
/// commutative accumulator. This one is both at once.
#[test]
fn rfold_short_run_reverse_folds_on_the_decline_exit() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    try_num
      .rfold(|| 0i64, |acc, x| acc * 10 + x)
      .parse_input(inp)
  }
  // Declines at "+" with [1, 2] buffered; right-to-left that is 0*10+2=2, 2*10+1=21.
  assert_eq!(Parser::new().apply(p).parse_str("1 2 +").unwrap(), 21);
}

#[test]
fn rfold_zero_width_element_does_not_buffer_unboundedly() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut budget = 5usize;
    let elem = move |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        ParseAttempt::Accept(1)
      } else {
        ParseAttempt::Decline
      })
    };
    elem.rfold(|| 0usize, |acc, x| acc + x).parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str("").unwrap();
  assert_eq!(
    result, 1,
    "the buffering wing must not grow unbounded on a zero-width accept"
  );
}

// ── Trivia-gap twins: committed-progress guard ────────────────────────────────
//
// The no-progress guard must key off committed consumption, not the cache-front cursor. Behind
// leading trivia the very first lookahead fill jumps the cursor across the skipped bytes without
// consuming anything, so a cursor-keyed guard reads that as progress and runs one extra cycle. The
// input `" 1"` (a leading space the lexer skips, then one token) is the minimal gap. The `_while`
// twins let the driver's own decision peek fill the cache; the plain fold/rfold twins peek from
// inside a zero-width element (Tier B).

#[test]
fn fold_zero_width_element_with_leading_trivia_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peeks the frontier (moving the cache-front cursor across the leading trivia) but consumes
    // nothing, then accepts. A cursor-keyed guard reads the peek's cursor jump as progress.
    let elem =
      |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
        let _ = inp.peek_one()?;
        Ok(ParseAttempt::Accept(1))
      };
    elem.fold(|| 0usize, |acc, x| acc + x).parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(result, 1);
}

#[test]
fn try_fold_zero_width_element_with_leading_trivia_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let elem =
      |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
        let _ = inp.peek_one()?;
        Ok(ParseAttempt::Accept(1))
      };
    elem
      .try_fold(|| 0usize, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(result, 1);
}

#[test]
fn rfold_zero_width_element_with_leading_trivia_does_not_buffer_unboundedly() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let elem =
      |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
        let _ = inp.peek_one()?;
        Ok(ParseAttempt::Accept(1))
      };
    elem.rfold(|| 0usize, |acc, x| acc + x).parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(result, 1);
}

#[test]
fn fold_while_zero_width_element_with_leading_trivia_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>|
          -> Result<Action, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<usize, ()> { Ok(1) };
    elem
      .fold_while::<_, _, _, U1>(cond, || 0usize, |acc, x| acc + x)
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(result, 1);
}

#[test]
fn try_fold_while_zero_width_element_with_leading_trivia_stops_after_one_accumulation() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>|
          -> Result<Action, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<usize, ()> { Ok(1) };
    elem
      .try_fold_while::<_, _, _, U1>(cond, || 0usize, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(result, 1);
}

#[test]
fn rfold_while_zero_width_element_with_leading_trivia_does_not_buffer_unboundedly() {
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut budget = 5usize;
    let cond = move |_peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     _emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>|
          -> Result<Action, ()> {
      Ok(if budget > 0 {
        budget -= 1;
        Action::Continue
      } else {
        Action::Stop
      })
    };
    let elem = |_inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<usize, ()> { Ok(1) };
    elem
      .rfold_while::<_, _, _, U1>(cond, || 0usize, |acc, x| acc + x)
      .parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(result, 1);
}

#[test]
fn fold_alternating_rollback_element_terminates() {
  // The oscillator shape the committed metric closes. The element alternates: (a) a peek-only
  // zero-width accept caches the frontier token, so the cache-front cursor sits at the token start
  // `T`; (b) an attempt consumes that cached token then declines, rolling back and dropping the
  // cached token so the cursor falls back to the committed position `P`. Under the old cache-front
  // metric the cursor reads `P → T → P → T …` as endless progress — an unbounded livelock; the
  // committed watermark stays at `P` and stops after the first accumulation. The element never
  // commits, so this must terminate.
  fn p<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let mut flip = false;
    let elem =
      move |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<usize>, ()> {
        if flip {
          // (b) consume the cached token inside an attempt, then decline — the rollback drops the
          // cached token, so the cache-front cursor falls back to the committed position.
          let _ = inp.try_attempt(|i| -> Result<(), ()> {
            let _ = i.next()?;
            Err(())
          });
        } else {
          // (a) peek-only: caches the frontier token without consuming, moving only the cursor.
          let _ = inp.peek_one()?;
        }
        flip = !flip;
        Ok(ParseAttempt::Accept(1))
      };
    elem.fold(|| 0usize, |acc, x| acc + x).parse_input(inp)
  }
  let result = Parser::new().apply(p).parse_str(" 1").unwrap();
  assert_eq!(
    result, 1,
    "the committed-progress metric stops the rollback oscillator after one accumulation"
  );
}
