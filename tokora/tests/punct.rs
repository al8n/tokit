#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
mod common;

use common::{E, TestEm, ctx};

use tokora::{
  InputRef, Parse, Parser, ParserContext,
  punct::{Comma, Punctuator},
};

use common::TestLexer;

// ── Punctuator trait default methods ────────────────────────────────────────

#[test]
fn punctuator_description_is_some() {
  let desc = <Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::description();
  assert!(desc.is_some());
  assert!(!desc.unwrap().as_str().is_empty());
}

#[test]
fn punctuator_name_is_populated() {
  let name = <Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::name();
  assert!(!name.as_str().is_empty());
}

#[test]
fn punctuator_eval_matches_kind() {
  use common::TokenKind;
  let kind = TokenKind::Comma;
  let matches = <Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::eval(&kind);
  assert!(matches);
}

#[test]
fn punctuator_eval_rejects_wrong_kind() {
  use common::TokenKind;
  let kind = TokenKind::Num;
  let matches = <Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::eval(&kind);
  assert!(!matches);
}

// ── Reference delegation ────────────────────────────────────────────────────

#[test]
fn ref_punctuator_name() {
  let name = <&Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::name();
  let orig = <Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::name();
  assert_eq!(name.as_str(), orig.as_str());
}

#[test]
fn ref_punctuator_eval() {
  use common::TokenKind;
  let kind = TokenKind::Comma;
  let matches = <&Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::eval(&kind);
  assert!(matches);
}

#[test]
fn ref_punctuator_description() {
  let desc = <&Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::description();
  let orig = <Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::description();
  assert_eq!(desc.is_some(), orig.is_some());
}

#[test]
fn ref_punctuator_kind() {
  use common::TokenKind;
  let kind = <&Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::kind();
  assert_eq!(kind, TokenKind::Comma);
}

#[test]
fn punctuator_kind() {
  use common::TokenKind;
  let kind = <Comma<(), (), ()> as Punctuator<'_, TestLexer<'_>>>::kind();
  assert_eq!(kind, TokenKind::Comma);
}

// ── Alias try_expect methods ─────────────────────────────────────────────────

#[test]
fn try_expect_minus_alias_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    // try_expect_minus is an alias for try_expect_hyphen
    Ok(inp.try_expect_minus()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("-");
  assert!(r.unwrap());
}

#[test]
fn try_expect_minus_alias_decline() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(inp.try_expect_minus()?.is_none())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert!(r.unwrap());
}

#[test]
fn try_expect_hyphen_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(inp.try_expect_hyphen()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("-");
  assert!(r.unwrap());
}

// ── Alias expect methods ─────────────────────────────────────────────────────

#[test]
fn expect_minus_alias_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_minus()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("-");
  assert!(r.unwrap());
}

#[test]
fn expect_minus_alias_eot() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_minus()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("");
  assert!(r.is_err());
}

#[test]
fn expect_hyphen_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_hyphen()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("-");
  assert!(r.unwrap());
}

#[test]
fn expect_hyphen_wrong_token() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_hyphen()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("42");
  assert!(r.is_err());
}

// ── Additional non-alias expect methods ──────────────────────────────────────

#[test]
fn expect_asterisk_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_asterisk()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("*");
  assert!(r.unwrap());
}

#[test]
fn try_expect_asterisk_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(inp.try_expect_asterisk()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("*");
  assert!(r.unwrap());
}

#[test]
fn try_expect_plus_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(inp.try_expect_plus()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("+");
  assert!(r.unwrap());
}

#[test]
fn try_expect_slash_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(inp.try_expect_slash()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("/");
  assert!(r.unwrap());
}

#[test]
fn try_expect_equal_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    Ok(inp.try_expect_equal()?.is_some())
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("=");
  assert!(r.unwrap());
}

#[test]
fn expect_plus_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_plus()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("+");
  assert!(r.unwrap());
}

#[test]
fn expect_slash_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_slash()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("/");
  assert!(r.unwrap());
}

#[test]
fn expect_equal_success() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, ParserContext<'inp, TestLexer<'inp>, TestEm>>,
  ) -> Result<bool, E> {
    inp.expect_equal()?;
    Ok(true)
  }
  let r: Result<bool, _> = Parser::with_context(ctx()).apply(parse).parse_str("=");
  assert!(r.unwrap());
}
