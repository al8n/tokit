#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests for `InputRef` methods: `sync_through`, `sync_to`, `fold`, `foldn`,
//! `foldr_within`, `foldrn`, `try_expect_map`, and `try_expect_and_then`.

mod common;

// `E` and `ctx` are exercised only by the `unstable-raw`-gated cache-rewind
// tests below; gate the import to match, keeping the valve-off build clean.
#[cfg(feature = "unstable-raw")]
use common::{E, ctx};

use common::{TestLexer, Token, TokenKind};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, cache::DefaultCache,
  emitter::Ignored, span::Spanned, utils::Expected,
};

// ── helpers ─────────────────────────────────────────────────────────────────

fn extract_num<S>(tok: Spanned<Token, S>) -> i64 {
  match tok.into_data() {
    Token::Num(n) => n,
    _ => 0,
  }
}

/// Type alias for an Ignored-emitter context (swallows unexpected token errors).
type IgnoredContext<'inp> =
  ParserContext<'inp, TestLexer<'inp>, Ignored, DefaultCache<'inp, TestLexer<'inp>>>;

/// Helper macro to create a parser with the Ignored emitter.
macro_rules! ignored_parser {
  () => {
    Parser::with_context(IgnoredContext::new(Ignored::default()))
  };
}

// ── sync_through ────────────────────────────────────────────────────────────

#[test]
fn sync_through_finds_token() {
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

  let result = ignored_parser!().apply(parse).parse_str("1 2 ; 3").unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

#[test]
fn sync_through_eof() {
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

  let result = ignored_parser!().apply(parse).parse_str("1 2 3").unwrap();
  assert!(result.is_none());
}

#[test]
fn sync_through_immediate() {
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

  let result = ignored_parser!().apply(parse).parse_str("; 1").unwrap();
  assert!(matches!(result, Some(Token::Semi)));
}

#[test]
fn sync_through_empty_input() {
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

  let result = ignored_parser!().apply(parse).parse_str("").unwrap();
  assert!(result.is_none());
}

#[test]
fn sync_through_consumes_matched_token() {
  // After sync_through finds Semi, the next token should be what follows it.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(Option<Token>, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let synced = inp.sync_through(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    let next = inp.next()?.map(|s| s.into_data());
    Ok((synced.map(|s| s.into_data()), next))
  }

  let (synced, next) = ignored_parser!().apply(parse).parse_str("1 ; 3").unwrap();
  assert!(matches!(synced, Some(Token::Semi)));
  assert!(matches!(next, Some(Token::Num(3))));
}

// ── sync_to ─────────────────────────────────────────────────────────────────

#[test]
fn sync_to_finds_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_to(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.is_some())
  }

  let found = ignored_parser!().apply(parse).parse_str("1 2 ; 3").unwrap();
  assert!(found);
}

#[test]
fn sync_to_does_not_consume_match() {
  // After sync_to finds Semi, the next token should still be Semi.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(bool, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let found = inp
      .sync_to(
        |t| matches!(t.data(), Token::Semi),
        || Some(Expected::one(TokenKind::Semi)),
      )?
      .is_some();
    let next = inp.next()?.map(|s| s.into_data());
    Ok((found, next))
  }

  let (found, next) = ignored_parser!().apply(parse).parse_str("1 2 ; 3").unwrap();
  assert!(found);
  assert!(matches!(next, Some(Token::Semi)));
}

#[test]
fn sync_to_eof() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_to(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.is_some())
  }

  let found = ignored_parser!().apply(parse).parse_str("1 2 3").unwrap();
  assert!(!found);
}

#[test]
fn sync_to_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let result = inp.sync_to(
      |t| matches!(t.data(), Token::Semi),
      || Some(Expected::one(TokenKind::Semi)),
    )?;
    Ok(result.is_some())
  }

  let found = ignored_parser!().apply(parse).parse_str("").unwrap();
  assert!(!found);
}

#[test]
fn sync_to_immediate() {
  // When the first token matches, sync_to should find it without consuming.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(bool, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let found = inp
      .sync_to(
        |t| matches!(t.data(), Token::Semi),
        || Some(Expected::one(TokenKind::Semi)),
      )?
      .is_some();
    let next = inp.next()?.map(|s| s.into_data());
    Ok((found, next))
  }

  let (found, next) = ignored_parser!().apply(parse).parse_str("; 1").unwrap();
  assert!(found);
  assert!(matches!(next, Some(Token::Semi)));
}

// ── fold ────────────────────────────────────────────────────────────────────

#[test]
fn fold_nums() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| acc + extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("1 2 3 +").unwrap();
  assert_eq!(result, 6);
}

#[test]
fn fold_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| acc + extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, 0);
}

#[test]
fn fold_all_tokens() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| acc + extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("10 20 30").unwrap();
  assert_eq!(result, 60);
}

#[test]
fn fold_product() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 1i64,
      |acc, tok| acc * extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("2 3 4").unwrap();
  assert_eq!(result, 24);
}

#[test]
fn fold_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.fold(
      |t| matches!(t.data(), Token::Num(_)),
      || 42i64,
      |acc, tok| acc + extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, 42);
}

// ── foldn ───────────────────────────────────────────────────────────────────

#[test]
fn foldn_exact() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldn(|| 0i64, |acc, tok| acc + extract_num(tok), 3)
  }

  let result = Parser::new().apply(parse).parse_str("1 2 3 4").unwrap();
  assert_eq!(result, 6);
}

#[test]
fn foldn_fewer_than_n() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldn(|| 0i64, |acc, tok| acc + extract_num(tok), 5)
  }

  // Only 2 tokens available but we asked for 5; should fold what's available.
  let result = Parser::new().apply(parse).parse_str("10 20").unwrap();
  assert_eq!(result, 30);
}

#[test]
fn foldn_zero() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldn(|| 99i64, |acc, tok| acc + extract_num(tok), 0)
  }

  let result = Parser::new().apply(parse).parse_str("1 2 3").unwrap();
  assert_eq!(result, 99);
}

#[test]
fn foldn_empty_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldn(|| 0i64, |acc, tok| acc + extract_num(tok), 3)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, 0);
}

#[test]
fn foldn_consumes_only_n_tokens() {
  // After folding 2 tokens, the third should still be available.
  fn parse<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
  ) -> Result<(i64, Option<Token>), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let sum = inp.foldn(|| 0i64, |acc, tok| acc + extract_num(tok), 2)?;
    let next = inp.next()?.map(|s| s.into_data());
    Ok((sum, next))
  }

  let (sum, next) = Parser::new().apply(parse).parse_str("1 2 3").unwrap();
  assert_eq!(sum, 3);
  assert!(matches!(next, Some(Token::Num(3))));
}

// ── foldrn ──────────────────────────────────────────────────────────────────

#[test]
fn foldrn_reverses_order() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // rfold processes right-to-left: [1,2,3] -> 3 first, then 2, then 1
    // 0*2+3=3, 3*2+2=8, 8*2+1=17
    inp.foldrn(|| 0i64, |acc, tok| acc * 2 + extract_num(tok), 3)
  }

  let result = Parser::new().apply(parse).parse_str("1 2 3").unwrap();
  assert_eq!(result, 17);
}

#[test]
fn foldrn_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldrn(|| 99i64, |acc, tok| acc + extract_num(tok), 3)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, 99);
}

#[test]
fn foldrn_unbounded_num_on_empty_input() {
  // `num = usize::MAX` is the documented "unbounded" sentinel; eagerly reserving that many
  // slots up front panics with a capacity overflow before any input is read.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldrn(|| 99i64, |acc, tok| acc + extract_num(tok), usize::MAX)
  }

  let result = Parser::new().apply(parse).parse_str("").unwrap();
  assert_eq!(result, 99);
}

#[test]
fn foldrn_fewer_than_n() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // [1, 2] with n=5: rfold processes 2 first, then 1
    // 0*2+2=2, 2*2+1=5
    inp.foldrn(|| 0i64, |acc, tok| acc * 2 + extract_num(tok), 5)
  }

  let result = Parser::new().apply(parse).parse_str("1 2").unwrap();
  assert_eq!(result, 5);
}

// ── foldr_within ────────────────────────────────────────────────────────────

#[test]
fn foldr_within_reverses_order() {
  use generic_arraydeque::typenum::U3;

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // With window capacity U3, collects exactly [1,2,3] then processes right-to-left:
    // 0*2+3=3, 3*2+2=8, 8*2+1=17
    inp.foldr_within::<_, U3, _, _, _>(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| acc * 2 + extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("1 2 3 +").unwrap();
  assert_eq!(result, 17);
}

#[test]
fn foldr_within_empty() {
  use generic_arraydeque::typenum::U4;

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldr_within::<_, U4, _, _, _>(
      |t| matches!(t.data(), Token::Num(_)),
      || 77i64,
      |acc, tok| acc + extract_num(tok),
    )
  }

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, 77);
}

// The two tests above are the whole of `foldr_within`'s pre-existing coverage, and both sit
// outside the band where its epilogue matters: `foldr_within_reverses_order` feeds exactly
// `W::CAPACITY` accepted tokens, so the capacity `break` runs the drain, and
// `foldr_within_empty` feeds none, so the drain is vacuous either way. Every run of length
// `1 ..= CAPACITY - 1` leaves the loop through the predicate-decline arm; the three tests
// below cover that band.

#[test]
fn foldr_within_short_run_folds_every_consumed_token() {
  use generic_arraydeque::typenum::U3;

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(i64, bool), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // Capacity 3, but only two accepted tokens: the run ends on the predicate decline, not on
    // the capacity break. 0*2+2=2, 2*2+1=5.
    let folded = inp.foldr_within::<_, U3, _, _, _>(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| acc * 2 + extract_num(tok),
    )?;
    let next_is_plus = matches!(inp.next()?.map(|s| s.into_data()), Some(Token::Plus));
    Ok((folded, next_is_plus))
  }

  let (folded, next_is_plus) = Parser::new().apply(parse).parse_str("1 2 +").unwrap();
  assert_eq!(
    folded, 5,
    "a run shorter than the window still has to be folded: both ints were consumed into the \
     buffer, so returning the untouched initializer would silently drop them"
  );
  assert!(
    next_is_plus,
    "the accepted run is consumed exactly once — the token after it is still `+`"
  );
}

#[test]
fn foldr_within_decline_after_partial_run() {
  use generic_arraydeque::typenum::U3;

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(i64, i64), ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    // The predicate accepts only the first token, so the run is one element long.
    let mut seen = false;
    let folded = inp.foldr_within::<_, U3, _, _, _>(
      |t| {
        if seen || !matches!(t.data(), Token::Num(_)) {
          false
        } else {
          seen = true;
          true
        }
      },
      || 0i64,
      |acc, tok| acc * 10 + extract_num(tok),
    )?;
    let survivor = inp.next()?.map(extract_num).unwrap_or(-1);
    Ok((folded, survivor))
  }

  let (folded, survivor) = Parser::new().apply(parse).parse_str("1 2").unwrap();
  assert_eq!(folded, 1, "the single accepted token is folded");
  assert_eq!(
    survivor, 2,
    "the declined token is not consumed — it survives at the cache front"
  );
}

#[test]
fn foldr_within_every_length_below_capacity() {
  use generic_arraydeque::typenum::U4;

  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp.foldr_within::<_, U4, _, _, _>(
      |t| matches!(t.data(), Token::Num(_)),
      || 0i64,
      |acc, tok| acc * 10 + extract_num(tok),
    )
  }

  // Window capacity 4; runs of 1, 2 and 3 all leave through the decline arm. The right fold
  // reads the run backwards, so `1 2 3` folds to 321.
  for (src, expected) in [("1 +", 1i64), ("1 2 +", 21), ("1 2 3 +", 321)] {
    let result = Parser::new().apply(parse).parse_str(src).unwrap();
    assert_eq!(result, expected, "input {src:?}");
  }
}

// ── try_expect_map ──────────────────────────────────────────────────────────

#[test]
fn try_expect_map_matches() {
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

  let result = Parser::new().apply(parse).parse_str("42").unwrap();
  assert_eq!(result, Some(42));
}

#[test]
fn try_expect_map_no_match() {
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

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

#[test]
fn try_expect_map_empty() {
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
  assert_eq!(result, None);
}

// ── try_expect_and_then ─────────────────────────────────────────────────────

#[test]
fn try_expect_and_then_ok() {
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

  let result = Parser::new().apply(parse).parse_str("5").unwrap();
  assert_eq!(result, Some(5));
}

#[test]
fn try_expect_and_then_err() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    inp
      .try_expect_and_then(|t| match t.data() {
        Token::Num(n) if *n > 0 => Some(Ok(*n)),
        Token::Num(_) => Some(Err(())),
        _ => None,
      })
      .map(|r| r.map(|(n, _)| n))
  }

  let result: Result<Option<i64>, ()> = Parser::new().apply(parse).parse_str("-3");
  assert!(result.is_err());
}

#[test]
fn try_expect_and_then_decline() {
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

  let result = Parser::new().apply(parse).parse_str("+").unwrap();
  assert_eq!(result, None);
}

// ── consume_cached (from input_ref_coverage) ────────────────────────────────

#[test]
fn consume_cached_one_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let tok = inp.consume_cached_one();
    Ok(tok.map(|t| match t.into_data() {
      Token::Num(n) => n,
      _ => -1,
    }))
  }

  let r = ignored_parser!().apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), None);
}

#[test]
fn consume_cached_one_after_peek() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Option<i64>, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.peek_one()?;
    let tok = inp.consume_cached_one();
    Ok(tok.map(|t| match t.into_data() {
      Token::Num(n) => n,
      _ => -1,
    }))
  }

  let r = ignored_parser!().apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), Some(42));
}

#[test]
fn consume_all_cached_empty() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    Ok(inp.consume_all_cached().is_none())
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 3");
  assert!(r.unwrap());
}

#[test]
fn consume_all_cached_after_peek() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let _ = inp.peek_one()?;
    Ok(inp.consume_all_cached().is_some())
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 3");
  assert!(r.unwrap());
}

#[test]
fn sync_through_finds_comma_token() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let found = inp.sync_through(
      |t| matches!(t.data(), Token::Comma),
      || Some(Expected::one(TokenKind::Comma)),
    )?;
    Ok(found.is_some())
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 , 3");
  assert!(r.unwrap());
}

#[test]
fn sync_through_no_match() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<bool, ()>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = ()>,
  {
    let found = inp.sync_through(
      |t| matches!(t.data(), Token::Comma),
      || Some(Expected::one(TokenKind::Comma)),
    )?;
    Ok(found.is_none())
  }

  let r = ignored_parser!().apply(parse).parse_str("1 2 3");
  assert!(r.unwrap());
}

// ── cache rewind (from cache_rewind_coverage) ────────────────────────────────

#[cfg(feature = "unstable-raw")]
#[test]
fn rewind_to_start_after_consuming() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<(i64, i64), E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let ckp = inp.save();
    let first = match inp.next()? {
      Some(tok) => match tok.into_data() {
        Token::Num(n) => n,
        _ => return Err(E),
      },
      None => return Err(E),
    };
    inp.restore(ckp);
    let again = match inp.next()? {
      Some(tok) => match tok.into_data() {
        Token::Num(n) => n,
        _ => return Err(E),
      },
      None => return Err(E),
    };
    Ok((first, again))
  }
  let r: Result<(i64, i64), _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  let (a, b) = r.unwrap();
  assert_eq!(a, b);
  assert_eq!(a, 42);
}

#[cfg(feature = "unstable-raw")]
#[test]
fn rewind_after_peek_populates_cache() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    // Save checkpoint before any peeking, then peek to populate cache,
    // then restore to verify the cache rewind works correctly.
    let ckp = inp.save();
    let _ = inp.peek_one()?;
    inp.restore(ckp);
    match inp.next()? {
      Some(tok) => match tok.into_data() {
        Token::Num(n) => Ok(n),
        _ => Err(E),
      },
      None => Err(E),
    }
  }
  let r: Result<i64, _> = Parser::with_context(ctx()).apply(parse).parse_str("42 99");
  assert_eq!(r.unwrap(), 42);
}

#[cfg(feature = "unstable-raw")]
#[test]
fn rewind_mid_stream() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<Vec<i64>, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let _ = inp.next()?;
    let ckp = inp.save();
    let _ = inp.next()?;
    let _ = inp.next()?;
    inp.restore(ckp);
    let mut results = Vec::new();
    while let Some(tok) = inp.next()? {
      if let Token::Num(n) = tok.into_data() {
        results.push(n);
      }
    }
    Ok(results)
  }
  let r: Result<Vec<i64>, _> = Parser::with_context(ctx()).apply(parse).parse_str("1 2 3");
  let nums = r.unwrap();
  assert_eq!(nums, vec![2, 3]);
}

#[cfg(feature = "unstable-raw")]
#[test]
fn rewind_with_empty_remaining_input() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, E>
  where
    Ctx: ParseContext<'inp, TestLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
  {
    let ckp = inp.save();
    while inp.next()?.is_some() {}
    inp.restore(ckp);
    match inp.next()? {
      Some(tok) => match tok.into_data() {
        Token::Num(n) => Ok(n),
        _ => Err(E),
      },
      None => Err(E),
    }
  }
  let r: Result<i64, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert_eq!(r.unwrap(), 42);
}
