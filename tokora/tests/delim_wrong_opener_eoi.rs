#![cfg(all(feature = "std", feature = "logos"))]
#![allow(clippy::type_complexity)]
//! Regression suite for issue #85 — "Delimited many parsers misclassify a cached wrong
//! opener at end of input".
//!
//! The four delimited many-drivers probe the opening delimiter with `inp.try_expect`. When
//! the next valid token is not the opener, the predicate records the captured wrong token and
//! leaves it cached. If that wrong token is also the FINAL token, the underlying lexer is now
//! at EOI, so the old `None if inp.is_eoi()` arm returned [`UnexpectedEot`] — even though a
//! real wrong token had been observed. The diagnostic therefore depended on whether another
//! token happened to follow the same wrong opener:
//!
//! - final wrong token  ⇒ (buggy) `UnexpectedEot`;
//! - same wrong token + a follower ⇒ `UnexpectedToken`, correctly expecting the opener.
//!
//! The fix discriminates on the captured evidence instead of `is_eoi`: a wrong opener is
//! always the expected-open unexpected-token diagnostic (regardless of EOI state, the token
//! staying cached/unconsumed); `UnexpectedEot` is reserved for a genuinely empty opener slot.
//!
//! Covered per driver (repeated / repeated-while / separated / separated-while):
//! 1. wrong opener as the FINAL token ⇒ expected-open `UnexpectedToken` carrying it, NOT EOT;
//! 2. the SAME wrong opener followed by another token ⇒ the IDENTICAL diagnostic (parity);
//! 3. genuinely empty input at the opener position ⇒ `UnexpectedEot` (unchanged);
//! 4. the wrong opener stays unconsumed (cursor unmoved) under a recovering emitter.

mod common;

use common::{TestLexer, Token, TokenKind};
use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan, TryParseInput,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, Verbose,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
  try_parse_input::ParseAttempt,
};

// ── A rich error type that distinguishes the opener-probe outcomes ─────────────
//
// The shared unit `E` (and `delim_unclosed`'s `RE`) collapse `UnexpectedToken` and
// `UnexpectedEot` to the same value, so they cannot witness this bug. `WE` keeps the
// wrong token's kind and span from `UnexpectedToken` distinct from `UnexpectedEot`, so the
// assertions prove *which* diagnostic was produced.

#[derive(Debug, Clone, PartialEq)]
enum WE {
  /// From `UnexpectedEot` — a genuinely empty opener position.
  Eot,
  /// From `UnexpectedToken` — the captured found token + its span.
  Wrong {
    found: Option<TokenKind>,
    span: SimpleSpan,
  },
  /// Any other diagnostic family (never the subject of these assertions).
  Other,
}

// The subject arm: capture the found token kind and the span so the assertions can prove the
// wrong opener is carried at its real position. Concrete on `Token`/`TokenKind`/`SimpleSpan`
// (the `TestLexer` instantiation) so the fields are reachable; generic over `Lang`.
impl<Lang: ?Sized> From<UnexpectedToken<'_, Token, TokenKind, SimpleSpan, Lang>> for WE {
  fn from(e: UnexpectedToken<'_, Token, TokenKind, SimpleSpan, Lang>) -> Self {
    WE::Wrong {
      found: e.found().map(TokenKind::from),
      span: e.span(),
    }
  }
}

impl From<UnexpectedEot> for WE {
  fn from(_: UnexpectedEot) -> Self {
    WE::Eot
  }
}

impl From<()> for WE {
  fn from(_: ()) -> Self {
    WE::Other
  }
}
impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for WE {
  fn from(_: FullContainer<S, Lang>) -> Self {
    WE::Other
  }
}
impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for WE {
  fn from(_: TooFew<S, Lang>) -> Self {
    WE::Other
  }
}
impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for WE {
  fn from(_: TooMany<S, Lang>) -> Self {
    WE::Other
  }
}
impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for WE {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    WE::Other
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for WE {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    WE::Other
  }
}
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for WE {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    WE::Other
  }
}
impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for WE {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    WE::Other
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for WE
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    WE::Other
  }
}

type FatalCtx = ParserContext<'static, TestLexer<'static>, Fatal<WE>>;
type VerboseCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<WE>>;

fn fatal_ctx() -> FatalCtx {
  ParserContext::new(Fatal::new())
}
fn verbose_ctx() -> VerboseCtx<'static> {
  ParserContext::new(Verbose::new())
}

// ── Element parsers / stop condition (mirrors `delim_unclosed`) ────────────────

fn try_num<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, WE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = WE>,
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

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, WE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = WE>,
{
  match inp.next()? {
    None => Err(WE::Other),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(WE::Other),
    },
  }
}

fn decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: &mut Ctx::Emitter,
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

// ── The four delimited many-drivers, in the issue's repro shape ────────────────
//
// `.at_least(1).delimited_by_braces().collect()` — a committed brace-delimited many-builder;
// the wrong opener is a leading identifier where `{` is expected.

fn go_repeated<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = WE>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .repeated()
    .at_least(1)
    .delimited_by_braces()
    .collect()
    .parse_input(inp)
}

fn go_repeated_while<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = WE>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .repeated_while::<_, U1>(decide_num::<Ctx>)
    .at_least(1)
    .delimited_by_braces()
    .collect()
    .parse_input(inp)
}

fn go_separated<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = WE>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  try_num
    .separated_by_comma()
    .at_least(1)
    .delimited_by_braces()
    .collect()
    .parse_input(inp)
}

fn go_separated_while<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, WE>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = WE>
    + SeparatedEmitter<'inp, TestLexer<'inp>>
    + FullContainerEmitter<'inp, TestLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
    + TooFewEmitter<'inp, TestLexer<'inp>>
    + TooManyEmitter<'inp, TestLexer<'inp>>
    + UnclosedEmitter<'inp, TestLexer<'inp>>,
{
  parse_num
    .separated_by_comma_while::<_, U1>(decide_num::<Ctx>)
    .at_least(1)
    .delimited_by_braces()
    .collect()
    .parse_input(inp)
}

// ── Per-driver cases ──────────────────────────────────────────────────────────
//
// The wrong opener `x` (an identifier) is at bytes 0..1 in both `"x"` and `"x 1"`.

macro_rules! driver_cases {
  ($go:ident, $classify:ident, $eot:ident, $unconsumed:ident) => {
    // Cases 1 + 2 + parity: the wrong opener is the expected-open UnexpectedToken carrying
    // that token (NOT UnexpectedEot), whether or not another token follows it.
    #[test]
    fn $classify() {
      let final_wrong: Result<Vec<i64>, WE> =
        Parser::with_context(fatal_ctx()).apply($go).parse_str("x");
      let followed: Result<Vec<i64>, WE> =
        Parser::with_context(fatal_ctx()).apply($go).parse_str("x 1");

      // Case 1 — the FINAL wrong opener. This arm is red before the fix (returns WE::Eot).
      assert!(
        matches!(
          &final_wrong,
          Err(WE::Wrong { found: Some(TokenKind::Ident), span }) if *span == SimpleSpan::new(0, 1)
        ),
        "final wrong opener must be the expected-open UnexpectedToken (Ident @ 0..1), got {final_wrong:?}",
      );
      // Case 2 — the same wrong opener followed by another token (already correct pre-fix).
      assert!(
        matches!(
          &followed,
          Err(WE::Wrong { found: Some(TokenKind::Ident), span }) if *span == SimpleSpan::new(0, 1)
        ),
        "followed wrong opener must be the expected-open UnexpectedToken (Ident @ 0..1), got {followed:?}",
      );
      // Parity (the core assertion of #85): the two diagnostics are IDENTICAL.
      assert_eq!(
        final_wrong, followed,
        "final vs followed wrong-opener diagnostics must be identical",
      );
    }

    // Case 3 — genuinely empty input at the opener position stays UnexpectedEot (unchanged).
    #[test]
    fn $eot() {
      let empty: Result<Vec<i64>, WE> =
        Parser::with_context(fatal_ctx()).apply($go).parse_str("");
      assert_eq!(
        empty,
        Err(WE::Eot),
        "empty input at the opener position must be UnexpectedEot",
      );
    }

    // Case 1, recovering: the wrong opener stays cached/unconsumed (the cursor never moves
    // past it) and the expected-open diagnostic is recorded. Before the fix this path also
    // hard-returned UnexpectedEot even under a recovering emitter, so `.unwrap()` was red.
    #[test]
    fn $unconsumed() {
      fn probe<'inp>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
      ) -> Result<(Vec<i64>, usize, usize, Vec<WE>), WE> {
        let before = *inp.cursor().as_inner();
        let items = $go(inp)?;
        let after = *inp.cursor().as_inner();
        let recorded: Vec<WE> = inp.emitter().errors().values().flatten().cloned().collect();
        Ok((items, before, after, recorded))
      }
      let (items, before, after, recorded) = Parser::with_context(verbose_ctx())
        .apply(probe)
        .parse_str("x")
        .unwrap();
      assert_eq!(items, Vec::<i64>::new(), "recovery collects no elements");
      assert_eq!(
        before, after,
        "the wrong opener token stays unconsumed — the cursor never moves past it",
      );
      assert!(
        recorded.iter().any(|e| matches!(
          e,
          WE::Wrong { found: Some(TokenKind::Ident), span } if *span == SimpleSpan::new(0, 1)
        )),
        "the expected-open unexpected-token must be recorded: {recorded:?}",
      );
    }
  };
}

driver_cases!(
  go_repeated,
  repeated_final_wrong_opener_is_unexpected_token,
  repeated_empty_input_is_eot,
  repeated_wrong_opener_unconsumed
);
driver_cases!(
  go_repeated_while,
  repeated_while_final_wrong_opener_is_unexpected_token,
  repeated_while_empty_input_is_eot,
  repeated_while_wrong_opener_unconsumed
);
driver_cases!(
  go_separated,
  separated_final_wrong_opener_is_unexpected_token,
  separated_empty_input_is_eot,
  separated_wrong_opener_unconsumed
);
driver_cases!(
  go_separated_while,
  separated_while_final_wrong_opener_is_unexpected_token,
  separated_while_empty_input_is_eot,
  separated_while_wrong_opener_unconsumed
);
