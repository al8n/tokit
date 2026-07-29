#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(warnings)]

//! Additional coverage tests for sync_through.rs, sync_to.rs, and try_expect.rs.
//! Targets uncovered lines:
//!   - sync_through_then_peek / sync_through_then_peek_with_emitter
//!   - try_expect with cache path edge cases
//!   - try_expect_map / try_expect_and_then on empty input (EOI)

mod common;

use common::{TestLexer, Token, TokenKind};
use generic_arraydeque::typenum::U1;
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, cache::DefaultCache,
  emitter::Ignored, span::Spanned, utils::Expected,
};

type IgnoredContext<'inp> =
  ParserContext<'inp, TestLexer<'inp>, Ignored, DefaultCache<'inp, TestLexer<'inp>>>;

macro_rules! ignored_parser {
  () => {
    Parser::with_context(IgnoredContext::new(Ignored::default()))
  };
}

// ── sync_through_then_peek ──────────────────────────────────────────────────

#[test]
fn sync_through_then_peek_finds_and_peeks_after() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (matched, peeked) = inp.sync_through_then_peek::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, peek_len) = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(matches!(matched, Some(Token::Semi)));
  assert_eq!(peek_len, 1); // "3" is peeked
}

#[test]
fn sync_through_then_peek_no_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (matched, peeked) = inp.sync_through_then_peek::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, peek_len) = ignored_parser!().apply(parse).parse_str("1 2 3").unwrap();
  assert!(matched.is_none());
  assert_eq!(peek_len, 0);
}

#[test]
fn sync_through_then_peek_empty_input() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (matched, peeked) = inp.sync_through_then_peek::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, peek_len) = ignored_parser!().apply(parse).parse_str("").unwrap();
  assert!(matched.is_none());
  assert_eq!(peek_len, 0);
}

#[test]
fn sync_through_then_peek_last_token() {
  // When the match is the last token, peeked should be empty
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (matched, peeked) = inp.sync_through_then_peek::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, peek_len) = ignored_parser!().apply(parse).parse_str("1 ;").unwrap();
  assert!(matches!(matched, Some(Token::Semi)));
  assert_eq!(peek_len, 0);
}

// ── sync_through_then_peek_with_emitter ─────────────────────────────────────

#[test]
fn sync_through_then_peek_with_emitter_finds_and_peeks() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (matched, peeked, _emitter) = inp.sync_through_then_peek_with_emitter::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, peek_len) = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(matches!(matched, Some(Token::Semi)));
  assert_eq!(peek_len, 1);
}

#[test]
fn sync_through_then_peek_with_emitter_no_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (matched, peeked, _emitter) = inp.sync_through_then_peek_with_emitter::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, _) = ignored_parser!().apply(parse).parse_str("1 2").unwrap();
  assert!(matched.is_none());
}

// ── sync_through_then_peek with cached tokens ───────────────────────────────

#[test]
fn sync_through_then_peek_with_cached_match() {
  // When the matching token is already in cache, sync_through_then_peek consumes
  // it and returns it as Some(matched), with peeked containing tokens after.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache with Num(42)
    drop(inp.peek::<U1>()?);
    // sync_through_then_peek: cache has Num(42) which IS a Num
    // The matching token is consumed and returned
    let (matched, peeked) =
      inp.sync_through_then_peek::<_, _, U1>(|t| matches!(t.data(), Token::Num(_)), || None)?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, peek_len) = ignored_parser!().apply(parse).parse_str("42 ;").unwrap();
  // matched is Some(Num(42)) because the matching cached token is consumed
  assert!(matches!(matched, Some(Token::Num(42))));
  assert_eq!(peek_len, 1); // peeked contains the next token (;)
}

#[test]
fn sync_through_then_peek_with_cached_non_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache with Num
    drop(inp.peek::<U1>()?);
    // Sync for Semi: cache has Num which doesn't match, should scan input
    let (matched, peeked) = inp.sync_through_then_peek::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, _) = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(matches!(matched, Some(Token::Semi)));
}

#[test]
fn sync_through_then_peek_cached_empty_not_match_eof() {
  // Cache has a non-matching token, and remaining input has no match either
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    drop(inp.peek::<U1>()?);
    let (matched, peeked) = inp.sync_through_then_peek::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, peek_len) = ignored_parser!().apply(parse).parse_str("1 2").unwrap();
  assert!(matched.is_none());
  assert_eq!(peek_len, 0);
}

// ── try_expect with cached tokens ──────────────────────────────────────────

#[test]
fn try_expect_from_cache_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache
    drop(inp.peek::<U1>()?);
    // try_expect from cache
    let result = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(matches!(result, Some(Token::Num(42))));
}

#[test]
fn try_expect_from_cache_no_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache with Num
    drop(inp.peek::<U1>()?);
    // try_expect for Semi: should not match
    let result = inp.try_expect(|t| matches!(t.data(), Token::Semi))?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(result.is_none());
}

#[test]
fn try_expect_on_eoi() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert!(result.is_none());
}

// ── try_expect_map on empty input ──────────────────────────────────────────

#[test]
fn try_expect_map_on_eoi() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert!(result.is_none());
}

// ── try_expect_and_then on empty input ─────────────────────────────────────

#[test]
fn try_expect_and_then_on_eoi() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert!(result.is_none());
}

// ── try_expect_and_then from cache: None output path ───────────────────────
// This covers the `None => Ok(None)` branch at line 322-323 of try_expect.rs

#[test]
fn try_expect_and_then_cache_decline_none() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache with Plus
    drop(inp.peek::<U1>()?);
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(result.is_none());
}

// ── try_expect_map from cache: no match leaves token in cache ──────────────

#[test]
fn try_expect_map_cache_no_match_preserves_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<i64>, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache
    drop(inp.peek::<U1>()?);
    // try_expect_map for Num: cache has Plus, should decline
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    // The Plus should still be available
    let next = inp.next()?.map(|s| s.into_data());
    Ok((result.map(|(n, _)| n), next))
  }

  let (mapped, next) = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(mapped.is_none());
  assert!(matches!(next, Some(Token::Plus)));
}

// ── sync_to_then_peek_with_emitter ──────────────────────────────────────────

#[test]
fn sync_to_then_peek_with_emitter_finds() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(usize, bool), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (peeked, _emitter) = inp.sync_to_then_peek_with_emitter::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    let has_match = !peeked.is_empty();
    Ok((peeked.len(), has_match))
  }

  let (len, found) = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(found);
  assert_eq!(len, 1);
}

#[test]
fn sync_to_then_peek_with_emitter_no_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let (peeked, _emitter) = inp.sync_to_then_peek_with_emitter::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(peeked.len())
  }

  let len = ignored_parser!().apply(parse).parse_str("1 2 3").unwrap();
  assert_eq!(len, 0);
}

#[test]
fn sync_to_then_peek_with_emitter_cached_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<usize, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache with Semi
    drop(inp.peek::<U1>()?);
    let (peeked, _emitter) = inp.sync_to_then_peek_with_emitter::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(peeked.len())
  }

  let len = ignored_parser!().apply(parse).parse_str("; 3").unwrap();
  assert_eq!(len, 1);
}

// ── try_expect then consume: ensures token is properly consumed ────────────

#[test]
fn try_expect_consumes_and_advances() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let first = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    let second = inp.next()?.map(|s| s.into_data());
    Ok((first.map(|s| s.into_data()), second))
  }

  let (first, second) = Parser::new().apply(parse).parse_str("1 2").unwrap();
  assert!(matches!(first, Some(Token::Num(1))));
  assert!(matches!(second, Some(Token::Num(2))));
}

// ── try_expect no match puts token in cache then next() can read it ────────

#[test]
fn try_expect_no_match_caches_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // try_expect for Semi on input "42": no match, token goes to cache
    let result = inp.try_expect(|t| matches!(t.data(), Token::Semi))?;
    // next() should return the cached token
    let next = inp.next()?.map(|s| s.into_data());
    Ok((result.map(|s| s.into_data()), next))
  }

  let (result, next) = Parser::new().apply(parse).parse_str("42").unwrap();
  assert!(result.is_none());
  assert!(matches!(next, Some(Token::Num(42))));
}

// ── try_expect_map no match puts token in cache ────────────────────────────

#[test]
fn try_expect_map_no_match_caches_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<i64>, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    let next = inp.next()?.map(|s| s.into_data());
    Ok((result.map(|(n, _)| n), next))
  }

  let (mapped, next) = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(mapped.is_none());
  assert!(matches!(next, Some(Token::Plus)));
}

// ── try_expect_and_then no match puts token in cache ───────────────────────

#[test]
fn try_expect_and_then_no_match_caches_token() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<i64>, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) => Some(Ok(*n)),
      _ => None,
    })?;
    let next = inp.next()?.map(|s| s.into_data());
    Ok((result.map(|(n, _)| n), next))
  }

  let (mapped, next) = Parser::new().apply(parse).parse_str("+").unwrap();
  assert!(mapped.is_none());
  assert!(matches!(next, Some(Token::Plus)));
}

// ── Using Option cache type ─────────────────────────────────────────────────

type OptionCacheCtx<'inp> = ParserContext<
  'inp,
  TestLexer<'inp>,
  Ignored,
  Option<tokora::cache::CachedTokenOf<'inp, TestLexer<'inp>>>,
>;

macro_rules! option_cache_parser {
  () => {
    Parser::with_context(OptionCacheCtx::new(Ignored::default()))
  };
}

#[test]
fn option_cache_try_expect_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect(|t| matches!(t.data(), Token::Num(_)))?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = option_cache_parser!().apply(parse).parse_str("42").unwrap();
  assert!(matches!(result, Some(Token::Num(42))));
}

#[test]
fn option_cache_try_expect_no_match() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect(|t| matches!(t.data(), Token::Semi))?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = option_cache_parser!().apply(parse).parse_str("42").unwrap();
  assert!(result.is_none());
}

#[test]
fn option_cache_sync_through() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<Option<Token>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.map(|s| s.into_data()))
  }

  let result = option_cache_parser!()
    .apply(parse)
    .parse_str("1 ; 3")
    .unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

#[test]
fn option_cache_try_expect_map() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_map(|t| match t.data() {
      Token::Num(n) => Some(*n),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = option_cache_parser!().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn option_cache_try_expect_and_then() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.try_expect_and_then(|t| match t.data() {
      Token::Num(n) if *n > 0 => Some(Ok(*n)),
      Token::Num(_) => Some(Err(())),
      _ => None,
    })?;
    Ok(result.map(|(n, _tok)| n))
  }

  let result = option_cache_parser!().apply(parse).parse_str("5").unwrap();
  assert_eq!(result, Some(5));
}

#[test]
fn option_cache_next_and_is_eoi() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, bool), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let tok = inp.next()?.map(|s| s.into_data());
    let eoi = inp.is_eoi();
    Ok((tok, eoi))
  }

  let (tok, eoi) = option_cache_parser!().apply(parse).parse_str("42").unwrap();
  assert!(matches!(tok, Some(Token::Num(42))));
  assert!(eoi);
}

// ── sync_through with cache then peek ───────────────────────────────────────
// This exercises the sync_through_then_peek_with_emitter cache=non-empty path

#[test]
fn sync_through_then_peek_with_emitter_cached_non_match_then_scan() {
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, usize), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Peek to fill cache with Num(1)
    drop(inp.peek::<U1>()?);
    // sync_through_then_peek_with_emitter: cache has non-match, scan for Semi
    let (matched, peeked, _emitter) = inp.sync_through_then_peek_with_emitter::<_, _, U1>(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok((matched.map(|s| s.into_data()), peeked.len()))
  }

  let (matched, _) = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(matches!(matched, Some(Token::Semi)));
}
