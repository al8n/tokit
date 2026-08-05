#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests exercising the **mut-ref** (`Collect<&mut ..., &mut Container>`) code
//! path for every separator-policy x count-modifier combination in the
//! `sep/parse` directory (non-delimited).
//!
//! 8 policies x 4 count variants + 4 base (no policy) = 36 tests.

mod common;

use common::E;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  parser::{
    AllowLeading, AllowTrailing, AtLeast, AtMost, Bounded, Collect, RequireLeading,
    RequireTrailing, Separated,
  },
  punct::Comma,
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

// -- Error type ---------------------------------------------------------------

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

// -- Element parser -----------------------------------------------------------

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

// -- Test macro ---------------------------------------------------------------

macro_rules! sep_mr {
  ($name:ident, |$s:ident| $build:expr, $input:expr) => {
    paste::paste! {
      fn [<$name _mr>]<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, E>
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
        let mut f = try_num;
        let $s = Separated::new::<Comma>(&mut f);
        let mut inner = $build;
        let mut container = Vec::<i64>::new();
        let mut collect = Collect::new(&mut inner, &mut container);
        let _span = collect.parse_input(inp)?;
        Ok(core::mem::take(&mut container))
      }

      #[test]
      fn [<$name _mutref>]() {
        let r = Parser::with_context(full_ctx())
          .apply([<$name _mr>])
          .parse_str($input)
          .unwrap();
        assert!(!r.is_empty());
      }
    }
  };
}

// == Base (no policy) -- 4 tests ==

sep_mr!(base_unb, |sep| sep, "1,2,3");
sep_mr!(base_min, |sep| AtLeast::new(sep, 2), "1,2,3");
sep_mr!(base_max, |sep| AtMost::new(sep, 3), "1,2,3");
sep_mr!(base_bnd, |sep| Bounded::new(sep, 4, 2), "1,2,3");

// == 1. allow_leading (4 count variants) ==

sep_mr!(al_unb, |sep| AllowLeading::new(sep), ",1,2,3");
sep_mr!(
  al_min,
  |sep| AllowLeading::new(AtLeast::new(sep, 2)),
  ",1,2,3"
);
sep_mr!(
  al_max,
  |sep| AllowLeading::new(AtMost::new(sep, 3)),
  ",1,2,3"
);
sep_mr!(
  al_bnd,
  |sep| AllowLeading::new(Bounded::new(sep, 4, 2)),
  ",1,2,3"
);

// == 2. allow_trailing (4 count variants) ==

sep_mr!(at_unb, |sep| AllowTrailing::new(sep), "1,2,3,");
sep_mr!(
  at_min,
  |sep| AllowTrailing::new(AtLeast::new(sep, 2)),
  "1,2,3,"
);
sep_mr!(
  at_max,
  |sep| AllowTrailing::new(AtMost::new(sep, 3)),
  "1,2,3,"
);
sep_mr!(
  at_bnd,
  |sep| AllowTrailing::new(Bounded::new(sep, 4, 2)),
  "1,2,3,"
);

// == 3. allow_surrounded (allow_leading + allow_trailing) (4 count variants) ==

sep_mr!(
  as_unb,
  |sep| AllowLeading::new(AllowTrailing::new(sep)),
  ",1,2,3,"
);
sep_mr!(
  as_min,
  |sep| AllowLeading::new(AllowTrailing::new(AtLeast::new(sep, 2))),
  ",1,2,3,"
);
sep_mr!(
  as_max,
  |sep| AllowLeading::new(AllowTrailing::new(AtMost::new(sep, 3))),
  ",1,2,3,"
);
sep_mr!(
  as_bnd,
  |sep| AllowLeading::new(AllowTrailing::new(Bounded::new(sep, 4, 2))),
  ",1,2,3,"
);

// == 4. require_leading (4 count variants) ==

sep_mr!(rl_unb, |sep| RequireLeading::new(sep), ",1,2,3");
sep_mr!(
  rl_min,
  |sep| RequireLeading::new(AtLeast::new(sep, 2)),
  ",1,2,3"
);
sep_mr!(
  rl_max,
  |sep| RequireLeading::new(AtMost::new(sep, 3)),
  ",1,2,3"
);
sep_mr!(
  rl_bnd,
  |sep| RequireLeading::new(Bounded::new(sep, 4, 2)),
  ",1,2,3"
);

// == 5. require_trailing (4 count variants) ==

sep_mr!(rt_unb, |sep| RequireTrailing::new(sep), "1,2,3,");
sep_mr!(
  rt_min,
  |sep| RequireTrailing::new(AtLeast::new(sep, 2)),
  "1,2,3,"
);
sep_mr!(
  rt_max,
  |sep| RequireTrailing::new(AtMost::new(sep, 3)),
  "1,2,3,"
);
sep_mr!(
  rt_bnd,
  |sep| RequireTrailing::new(Bounded::new(sep, 4, 2)),
  "1,2,3,"
);

// == 6. require_surrounded (require_leading + require_trailing) (4 count variants) ==

sep_mr!(
  rs_unb,
  |sep| RequireLeading::new(RequireTrailing::new(sep)),
  ",1,2,3,"
);
sep_mr!(
  rs_min,
  |sep| RequireLeading::new(RequireTrailing::new(AtLeast::new(sep, 2))),
  ",1,2,3,"
);
sep_mr!(
  rs_max,
  |sep| RequireLeading::new(RequireTrailing::new(AtMost::new(sep, 3))),
  ",1,2,3,"
);
sep_mr!(
  rs_bnd,
  |sep| RequireLeading::new(RequireTrailing::new(Bounded::new(sep, 4, 2))),
  ",1,2,3,"
);

// == 7. allow_leading_require_trailing (4 count variants) ==

sep_mr!(
  alrt_unb,
  |sep| AllowLeading::new(RequireTrailing::new(sep)),
  ",1,2,3,"
);
sep_mr!(
  alrt_min,
  |sep| AllowLeading::new(RequireTrailing::new(AtLeast::new(sep, 2))),
  ",1,2,3,"
);
sep_mr!(
  alrt_max,
  |sep| AllowLeading::new(RequireTrailing::new(AtMost::new(sep, 3))),
  ",1,2,3,"
);
sep_mr!(
  alrt_bnd,
  |sep| AllowLeading::new(RequireTrailing::new(Bounded::new(sep, 4, 2))),
  ",1,2,3,"
);

// == 8. require_leading_allow_trailing (4 count variants) ==

sep_mr!(
  rlat_unb,
  |sep| RequireLeading::new(AllowTrailing::new(sep)),
  ",1,2,3,"
);
sep_mr!(
  rlat_min,
  |sep| RequireLeading::new(AllowTrailing::new(AtLeast::new(sep, 2))),
  ",1,2,3,"
);
sep_mr!(
  rlat_max,
  |sep| RequireLeading::new(AllowTrailing::new(AtMost::new(sep, 3))),
  ",1,2,3,"
);
sep_mr!(
  rlat_bnd,
  |sep| RequireLeading::new(AllowTrailing::new(Bounded::new(sep, 4, 2))),
  ",1,2,3,"
);
