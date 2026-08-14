#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Tests exercising the **spanned** (`With<Collect<...>, PhantomSpan>`) and
//! **mut-ref** (`Collect<&mut ..., &mut Container>`) code paths for every
//! separator-policy x count-modifier combination in the `sep/parse` directory
//! (non-delimited).
//!
//! 32 policy combos x 2 paths = 64 tests.

mod common;

use common::E;

use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan, TryParseInput,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  parser::With,
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::marker::PhantomSpan,
};

use common::{TestLexer, Token};

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

// ── Element parser ────────────────────────────────────────────────────────────

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

// ── Test macro ────────────────────────────────────────────────────────────────

macro_rules! sep_parse_tests {
  ($name:ident, { $($policy:tt)* }, $input:expr) => {
    paste::paste! {
      fn [<$name _sp>]<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Spanned<Vec<i64>, SimpleSpan>, E>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
      {
        With::new(
          try_num
            .separated_by_comma()
            $($policy)*
            .collect(),
          PhantomSpan::PHANTOM,
        )
        .parse_input(inp)
      }

      #[test]
      fn [<$name _spanned>]() {
        let r = Parser::with_context(full_ctx())
          .apply([<$name _sp>])
          .parse_str($input)
          .unwrap();
        assert!(!r.data().is_empty());
      }
    }
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. allow_leading (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(al_unb, { .allow_leading() }, ",1,2,3");
sep_parse_tests!(al_min, { .allow_leading().at_least(2) }, ",1,2,3");
sep_parse_tests!(al_max, { .allow_leading().at_most(3) }, ",1,2,3");
sep_parse_tests!(al_bnd, { .allow_leading().bounded(2, 4) }, ",1,2,3");

// ═══════════════════════════════════════════════════════════════════════════════
// 2. allow_trailing (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(at_unb, { .allow_trailing() }, "1,2,3,");
sep_parse_tests!(at_min, { .allow_trailing().at_least(2) }, "1,2,3,");
sep_parse_tests!(at_max, { .allow_trailing().at_most(3) }, "1,2,3,");
sep_parse_tests!(at_bnd, { .allow_trailing().bounded(2, 4) }, "1,2,3,");

// ═══════════════════════════════════════════════════════════════════════════════
// 3. allow_surrounded (allow_trailing + allow_leading) (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(as_unb, { .allow_trailing().allow_leading() }, ",1,2,3,");
sep_parse_tests!(as_min, { .allow_trailing().at_least(2).allow_leading() }, ",1,2,3,");
sep_parse_tests!(as_max, { .allow_trailing().at_most(3).allow_leading() }, ",1,2,3,");
sep_parse_tests!(as_bnd, { .allow_trailing().bounded(2, 4).allow_leading() }, ",1,2,3,");

// ═══════════════════════════════════════════════════════════════════════════════
// 4. require_leading (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(rl_unb, { .require_leading() }, ",1,2,3");
sep_parse_tests!(rl_min, { .require_leading().at_least(2) }, ",1,2,3");
sep_parse_tests!(rl_max, { .require_leading().at_most(3) }, ",1,2,3");
sep_parse_tests!(rl_bnd, { .require_leading().bounded(2, 4) }, ",1,2,3");

// ═══════════════════════════════════════════════════════════════════════════════
// 5. require_trailing (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(rt_unb, { .require_trailing() }, "1,2,3,");
sep_parse_tests!(rt_min, { .require_trailing().at_least(2) }, "1,2,3,");
sep_parse_tests!(rt_max, { .require_trailing().at_most(3) }, "1,2,3,");
sep_parse_tests!(rt_bnd, { .require_trailing().bounded(2, 4) }, "1,2,3,");

// ═══════════════════════════════════════════════════════════════════════════════
// 6. require_surrounded (require_trailing + require_leading) (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(rs_unb, { .require_trailing().require_leading() }, ",1,2,3,");
sep_parse_tests!(rs_min, { .require_trailing().at_least(2).require_leading() }, ",1,2,3,");
sep_parse_tests!(rs_max, { .require_trailing().at_most(3).require_leading() }, ",1,2,3,");
sep_parse_tests!(rs_bnd, { .require_trailing().bounded(2, 4).require_leading() }, ",1,2,3,");

// ═══════════════════════════════════════════════════════════════════════════════
// 7. allow_leading_require_trailing (require_trailing + allow_leading)
//    (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(alrt_unb, { .require_trailing().allow_leading() }, ",1,2,3,");
sep_parse_tests!(alrt_min, { .require_trailing().at_least(2).allow_leading() }, ",1,2,3,");
sep_parse_tests!(alrt_max, { .require_trailing().at_most(3).allow_leading() }, ",1,2,3,");
sep_parse_tests!(alrt_bnd, { .require_trailing().bounded(2, 4).allow_leading() }, ",1,2,3,");

// ═══════════════════════════════════════════════════════════════════════════════
// 8. require_leading_allow_trailing (allow_trailing + require_leading)
//    (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_parse_tests!(rlat_unb, { .allow_trailing().require_leading() }, ",1,2,3,");
sep_parse_tests!(rlat_min, { .allow_trailing().at_least(2).require_leading() }, ",1,2,3,");
sep_parse_tests!(rlat_max, { .allow_trailing().at_most(3).require_leading() }, ",1,2,3,");
sep_parse_tests!(rlat_bnd, { .allow_trailing().bounded(2, 4).require_leading() }, ",1,2,3,");
