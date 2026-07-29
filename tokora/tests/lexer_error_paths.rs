#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
mod common;

use common::E;

use tokora::{
  Emitter, InputRef, Lexer, Parse, Parser, ParserContext, Token as TokenTrait,
  error::token::UnexpectedTokenOf, input::Cursor, span::Spanned,
};

use common::{TestLexer, Token};

// ── Fatal emitter: errors on lexer errors ───────────────────────────────────

struct FatalEm;
impl<'inp> Emitter<'inp, TestLexer<'inp>> for FatalEm {
  type Error = E;
  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), E> {
    Err(E)
  }
  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), E> {
    Err(E)
  }
  fn emit_error(
    &mut self,
    err: Spanned<E, <TestLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), E> {
    Err(err.into_data())
  }
  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, _: u64) {}
}

// ── Recovering emitter: skips over lexer errors ─────────────────────────────

struct RecoveringEm;
impl<'inp> Emitter<'inp, TestLexer<'inp>> for RecoveringEm {
  type Error = E;
  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TestLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TestLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), E> {
    Ok(()) // recover, skip the error
  }
  fn emit_unexpected_token(
    &mut self,
    _: UnexpectedTokenOf<'inp, TestLexer<'inp>>,
  ) -> Result<(), E> {
    Ok(()) // recover
  }
  fn emit_error(&mut self, _: Spanned<E, <TestLexer<'inp> as Lexer<'inp>>::Span>) -> Result<(), E> {
    Ok(())
  }
  fn rewind(&mut self, _: &Cursor<'inp, '_, TestLexer<'inp>>, _: u64) {}
}

fn fatal_ctx() -> ParserContext<'static, TestLexer<'static>, FatalEm> {
  ParserContext::new(FatalEm)
}

fn recovering_ctx() -> ParserContext<'static, TestLexer<'static>, RecoveringEm> {
  ParserContext::new(RecoveringEm)
}

// ── try_expect with lexer error (fatal) ─────────────────────────────────────

#[test]
fn try_expect_lexer_error_fatal() {
  // "@" is not a valid token for our lexer, produces a lexer error.
  // FatalEm returns Err on lexer error → covers the Err(e) branch in try_expect_on_input.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, FatalEm>>,
  ) -> Result<bool, E> {
    Ok(
      inp
        .try_expect(|t| matches!(t.data(), Token::Num(_)))?
        .is_some(),
    )
  }
  let r: Result<bool, _> = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.is_err());
}

#[test]
fn try_expect_lexer_error_recovering() {
  // "@" produces a lexer error. RecoveringEm returns Ok → skips error, finds "42".
  // Covers the Ok(_) branch in try_expect_on_input.
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    Ok(
      inp
        .try_expect(|t| matches!(t.data(), Token::Num(_)))?
        .is_some(),
    )
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.unwrap());
}

// ── try_expect_map with lexer error ─────────────────────────────────────────

#[test]
fn try_expect_map_lexer_error_fatal() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, FatalEm>>,
  ) -> Result<Option<i64>, E> {
    let r = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(r.map(|(n, _)| n))
  }
  let r: Result<Option<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.is_err());
}

#[test]
fn try_expect_map_lexer_error_recovering() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<Option<i64>, E> {
    let r = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(r.map(|(n, _)| n))
  }
  let r: Result<Option<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert_eq!(r.unwrap(), Some(42));
}

// ── try_expect_and_then with lexer error ────────────────────────────────────

#[test]
fn try_expect_and_then_lexer_error_fatal() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, FatalEm>>,
  ) -> Result<Option<i64>, E> {
    let r = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(r.map(|(n, _)| n))
  }
  let r: Result<Option<i64>, _> = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.is_err());
}

#[test]
fn try_expect_and_then_lexer_error_recovering() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<Option<i64>, E> {
    let r = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(r.map(|(n, _)| n))
  }
  let r: Result<Option<i64>, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert_eq!(r.unwrap(), Some(42));
}

// ── sync_through with lexer errors ──────────────────────────────────────────

#[test]
fn sync_through_lexer_error_fatal() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, FatalEm>>,
  ) -> Result<bool, E> {
    let r = inp.sync_through(|t| matches!(t.data(), Token::Num(_)), || None)?;
    Ok(r.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.is_err());
}

#[test]
fn sync_through_lexer_error_recovering() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    let r = inp.sync_through(|t| matches!(t.data(), Token::Num(_)), || None)?;
    Ok(r.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.unwrap());
}

#[test]
fn sync_through_skip_wrong_tokens() {
  // sync_through should skip non-matching tokens and emit unexpected_token for each
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    let r = inp.sync_through(|t| matches!(t.data(), Token::Num(_)), || None)?;
    Ok(r.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str(", ; 42");
  assert!(r.unwrap());
}

#[test]
fn sync_through_no_match() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    let r = inp.sync_through(|t| matches!(t.data(), Token::Num(_)), || None)?;
    Ok(r.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str(", ;");
  assert!(r.unwrap());
}

// ── sync_through with cached tokens ─────────────────────────────────────────

// ── sync_through with cached tokens ─────────────────────────────────────────

#[test]
fn sync_through_cached_first_token_matches() {
  // If cache has a matching token as the first element, sync_through should find it.
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    // peek to fill cache with 42
    let _ = inp.peek_one()?;
    let r = inp.sync_through(|t| matches!(t.data(), Token::Num(_)), || None)?;
    Ok(r.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("42");
  // If this returns false (Ok(false)), the matching cached token was NOT consumed by sync_through.
  // This may indicate a bug in sync_matched_in_cache when the first cached token matches.
  let found = r.expect("should not error with recovering emitter");
  assert!(
    found,
    "sync_through should find matching token even when it's the first cached token"
  );
}

#[test]
fn sync_through_cached_skip_non_matching_then_match() {
  // Cache has a non-matching token, then input has a matching one.
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    // peek to fill cache with comma
    let _ = inp.peek_one()?;
    let r = inp.sync_through(|t| matches!(t.data(), Token::Num(_)), || None)?;
    Ok(r.is_some())
  }
  // Comma is cached, then 42 should match from input
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str(", 42");
  assert!(r.unwrap());
}

// ── sync_through_then_peek ──────────────────────────────────────────────────

#[test]
fn sync_through_then_peek_match() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    let (tok, _peeked) = inp.sync_through_then_peek::<_, _, generic_arraydeque::typenum::U1>(
      |t| matches!(t.data(), Token::Num(_)),
      || None,
    )?;
    Ok(tok.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("42 ,");
  assert!(r.unwrap());
}

#[test]
fn sync_through_then_peek_no_match() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    let (tok, _peeked) = inp.sync_through_then_peek::<_, _, generic_arraydeque::typenum::U1>(
      |t| matches!(t.data(), Token::Num(_)),
      || None,
    )?;
    Ok(tok.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str(",");
  assert!(r.unwrap());
}

// sync_through_then_peek with cached non-matching then matching from input

#[test]
fn sync_through_then_peek_cached_non_matching() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    // peek to fill cache with comma (non-matching)
    let _ = inp.peek_one()?;
    let (tok, _peeked) = inp.sync_through_then_peek::<_, _, generic_arraydeque::typenum::U1>(
      |t| matches!(t.data(), Token::Num(_)),
      || None,
    )?;
    Ok(tok.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str(", 42");
  assert!(r.unwrap());
}

#[test]
fn sync_through_then_peek_skip_and_find() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    let (tok, _peeked) = inp.sync_through_then_peek::<_, _, generic_arraydeque::typenum::U1>(
      |t| matches!(t.data(), Token::Num(_)),
      || None,
    )?;
    Ok(tok.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str(", ; 42");
  assert!(r.unwrap());
}

#[test]
fn sync_through_then_peek_lexer_error() {
  fn parse<'inp>(
    inp: &mut InputRef<
      'inp,
      '_,
      TestLexer<'inp>,
      ParserContext<'inp, TestLexer<'inp>, RecoveringEm>,
    >,
  ) -> Result<bool, E> {
    let (tok, _peeked) = inp.sync_through_then_peek::<_, _, generic_arraydeque::typenum::U1>(
      |t| matches!(t.data(), Token::Num(_)),
      || None,
    )?;
    Ok(tok.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(recovering_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.unwrap());
}

#[test]
fn sync_through_then_peek_lexer_error_fatal() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, FatalEm>>,
  ) -> Result<bool, E> {
    let (tok, _peeked) = inp.sync_through_then_peek::<_, _, generic_arraydeque::typenum::U1>(
      |t| matches!(t.data(), Token::Num(_)),
      || None,
    )?;
    Ok(tok.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str("@ 42");
  assert!(r.is_err());
}

// ── expect_<punct> EOT errors ───────────────────────────────────────────────

#[test]
fn expect_asterisk_eot() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, FatalEm>>,
  ) -> Result<bool, E> {
    inp.expect_asterisk()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(fatal_ctx()).apply(parse).parse_str("");
  assert!(r.is_err());
}

#[test]
fn expect_plus_wrong_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, FatalEm>>,
  ) -> Result<bool, E> {
    inp.expect_plus()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(fatal_ctx())
    .apply(parse)
    .parse_str(",");
  assert!(r.is_err());
}
