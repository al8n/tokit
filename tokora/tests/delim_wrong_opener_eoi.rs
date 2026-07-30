#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
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
//! 4. the wrong opener stays unconsumed (cursor unmoved) under a recovering emitter;
//! 5. the wrong opener is reported exactly ONCE under a recording emitter, and the `"x"` /
//!    `"x 1"` recorded vectors stay identical (the parity duty on the recording path);
//! 6. a close miss naming a DIFFERENT token than the wrong opener is preserved.
//!
//! Scenarios 5 and 6 are the suppression pair. Scenarios 1-4 run under `Fatal`, where the first
//! emit aborts the driver, so they cannot see a double report at all: the defect lived only
//! on the recording path.

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

// ── Scenarios 5 + 6 — one junk token, one report ──────────────────────────────
//
// Scenario 5 pins the suppression. Before the fix every family recorded the same `W@0..1`
// TWICE under a recording emitter: the opener arm flagged the wrong opener, the token stayed
// cached and unconsumed, and the close probe then re-saw that very token and reported it
// again as a close miss.
//
// Scenario 6 is its twin and pins the other direction: when a genuinely DIFFERENT token sits
// at the close position, that second report is real and must survive. The pair makes the
// relation impossible to invert quietly — a suppression keyed on anything coarser than
// committed consumption (the naive `open_span.is_none()` gate, a rejected cheap fix)
// passes scenario 5 and DELETES scenario 6's `Ident@4..5`.
//
// Both assert the full recorded vector, never a count and never `any`: `Verbose::errors()` is
// a `BTreeMap<Span, Vec<Error>>`, so `.values().flatten()` is span-ordered and deterministic.

fn wrong(kind: TokenKind, start: usize, end: usize) -> WE {
  WE::Wrong {
    found: Some(kind),
    span: SimpleSpan::new(start, end),
  }
}

macro_rules! flagged_once_cases {
  ($go:ident, $once:ident, $distinct:ident, $distinct_expected:expr) => {
    #[test]
    fn $once() {
      fn probe<'inp>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
      ) -> Result<Vec<WE>, WE> {
        $go(inp)?;
        Ok(inp.emitter().errors().values().flatten().cloned().collect())
      }
      let final_wrong: Vec<WE> = Parser::with_context(verbose_ctx())
        .apply(probe)
        .parse_str("x")
        .expect("a recording emitter recovers past a wrong opener");
      let followed: Vec<WE> = Parser::with_context(verbose_ctx())
        .apply(probe)
        .parse_str("x 1")
        .expect("a recording emitter recovers past a wrong opener");

      assert_eq!(
        final_wrong,
        vec![WE::Other, wrong(TokenKind::Ident, 0, 1)],
        "the wrong opener must be reported exactly once (the TooFew secondary, then one \
         expected-open unexpected-token); a second identical entry is the close probe \
         re-reporting the same still-cached token",
      );
      assert_eq!(
        final_wrong, followed,
        "final vs followed wrong-opener recorded vectors must be identical — the #85 parity \
         duty, extended from the Fatal path to the recording path",
      );
    }

    #[test]
    fn $distinct() {
      fn probe<'inp>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
      ) -> Result<Vec<WE>, WE> {
        $go(inp)?;
        Ok(inp.emitter().errors().values().flatten().cloned().collect())
      }
      let recorded: Vec<WE> = Parser::with_context(verbose_ctx())
        .apply(probe)
        .parse_str("1 2 x")
        .expect("a recording emitter recovers past a close miss");
      assert_eq!(
        recorded, $distinct_expected,
        "a close miss naming a DIFFERENT token than the wrong opener is genuine and must \
         survive suppression",
      );
    }
  };
}

flagged_once_cases!(
  go_repeated,
  repeated_wrong_opener_is_flagged_once,
  repeated_distinct_close_miss_is_preserved,
  vec![wrong(TokenKind::Num, 0, 1), wrong(TokenKind::Ident, 4, 5)]
);
flagged_once_cases!(
  go_repeated_while,
  repeated_while_wrong_opener_is_flagged_once,
  repeated_while_distinct_close_miss_is_preserved,
  vec![wrong(TokenKind::Num, 0, 1), wrong(TokenKind::Ident, 4, 5)]
);
flagged_once_cases!(
  go_separated,
  separated_wrong_opener_is_flagged_once,
  separated_distinct_close_miss_is_preserved,
  vec![
    wrong(TokenKind::Num, 0, 1),
    WE::Other,
    wrong(TokenKind::Ident, 4, 5)
  ]
);
flagged_once_cases!(
  go_separated_while,
  separated_while_wrong_opener_is_flagged_once,
  separated_while_distinct_close_miss_is_preserved,
  vec![
    wrong(TokenKind::Num, 0, 1),
    WE::Other,
    wrong(TokenKind::Ident, 4, 5)
  ]
);

// ── Scenario 7 — the rare arms: zero-width × wrong-opener composites ──────────
//
// Each driver carries more than one `CloseStatus::WrongToken` arm, and the suppression is
// applied at every one of them. Scenarios 5/6 above reach only the IN-LOOP arm of each
// family; the function-level four-way-probe arms are reached, in the rest of the suite, only
// by zero-width-element fixtures — which never carry a wrong opener, so they never arrive
// there with the flag set.
//
// These composites are the missing intersection: a wrong opener (so the flag is armed and the
// junk token stays cached at the front) PLUS a zero-width element (so the loop makes no
// committed progress and breaks to the function-level probe, which then re-sees that same
// cached token). They are the cells that make the suppression at those arms falsifiable
// rather than merely present.
//
// The element accepts without consuming, so the stall guard breaks the loop after one push;
// the epilogue's probe then re-sees the still-cached wrong opener. Expected in every case:
// the wrong opener recorded exactly ONCE.

const ONE_FLAG: fn() -> Vec<WE> = || vec![wrong(TokenKind::Ident, 0, 1)];

// Reaches `parser/many/delim/repeated.rs`'s function-level four-way probe.
#[test]
fn repeated_zero_width_stall_arm_flags_wrong_opener_once() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<Vec<WE>, WE> {
    let elem = |_: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>| -> Result<
      ParseAttempt<i64>,
      WE,
    > { Ok(ParseAttempt::Accept(0)) };
    let _: Vec<i64> = elem
      .repeated()
      .delimited_by_braces()
      .collect()
      .parse_input(inp)?;
    Ok(inp.emitter().errors().values().flatten().cloned().collect())
  }
  let recorded: Vec<WE> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("x")
    .expect("a recording emitter recovers past a wrong opener");
  assert_eq!(
    recorded,
    ONE_FLAG(),
    "the stall epilogue's close probe re-sees the still-cached wrong opener; it must not \
     report it a second time",
  );
}

// Reaches `parser/many/delim/repeated_while.rs`'s function-level four-way probe. The
// condition must say `Continue` at least once, or the in-loop arm settles it instead.
#[test]
fn repeated_while_zero_width_stall_arm_flags_wrong_opener_once() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<Vec<WE>, WE> {
    let mut budget = 3usize;
    let cond =
      move |_: Peeked<'_, 'inp, TestLexer<'inp>, U1>, _: &mut Verbose<WE>| -> Result<Action, WE> {
        Ok(if budget > 0 {
          budget -= 1;
          Action::Continue
        } else {
          Action::Stop
        })
      };
    let elem =
      |_: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>| -> Result<i64, WE> { Ok(0) };
    let _: Vec<i64> = elem
      .repeated_while::<_, U1>(cond)
      .delimited_by_braces()
      .collect()
      .parse_input(inp)?;
    Ok(inp.emitter().errors().values().flatten().cloned().collect())
  }
  let recorded: Vec<WE> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("x")
    .expect("a recording emitter recovers past a wrong opener");
  assert_eq!(
    recorded,
    ONE_FLAG(),
    "the stall epilogue's close probe re-sees the still-cached wrong opener; it must not \
     report it a second time",
  );
}

// Reaches `parser/many/sep_while/delim/mod.rs`'s in-loop stall probe (the third of that
// file's three arms).
#[test]
fn separated_while_zero_width_stall_arm_flags_wrong_opener_once() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>,
  ) -> Result<Vec<WE>, WE> {
    let mut budget = 3usize;
    let cond =
      move |_: Peeked<'_, 'inp, TestLexer<'inp>, U1>, _: &mut Verbose<WE>| -> Result<Action, WE> {
        Ok(if budget > 0 {
          budget -= 1;
          Action::Continue
        } else {
          Action::Stop
        })
      };
    let elem =
      |_: &mut InputRef<'inp, '_, TestLexer<'inp>, VerboseCtx<'inp>>| -> Result<i64, WE> { Ok(0) };
    let _: Vec<i64> = elem
      .separated_by_comma_while::<_, U1>(cond)
      .delimited_by_braces()
      .collect()
      .parse_input(inp)?;
    Ok(inp.emitter().errors().values().flatten().cloned().collect())
  }
  let recorded: Vec<WE> = Parser::with_context(verbose_ctx())
    .apply(parse)
    .parse_str("x")
    .expect("a recording emitter recovers past a wrong opener");
  assert_eq!(
    recorded,
    ONE_FLAG(),
    "the stall probe re-sees the still-cached wrong opener; it must not report it a second \
     time",
  );
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
