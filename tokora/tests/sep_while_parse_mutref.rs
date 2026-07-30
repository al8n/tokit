#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests exercising the **mut-ref** (`Collect<&mut ..., &mut Container>`) code
//! path for every separator-policy x count-modifier combination in the
//! `sep_while/parse` directory (non-delimited).
//!
//! 8 policies x 4 count variants + 4 base (no policy) = 36 tests.

mod common;

use common::E;

use generic_arraydeque::typenum::U1;
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  parser::{
    Action, AllowLeading, AllowTrailing, AtLeast, AtMost, Bounded, Collect, RequireLeading,
    RequireTrailing, SeparatedWhile,
  },
  punct::Comma,
};

use common::{TestLexer, Token};

// -- Error type ---------------------------------------------------------------

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

// -- Condition + element parser -----------------------------------------------

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

fn parse_num<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>) -> Result<i64, E>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>,
{
  match inp.next()? {
    None => Err(E),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(E),
    },
  }
}

// -- Test macro ---------------------------------------------------------------

macro_rules! sw_mr {
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
        let mut f = parse_num;
        let mut cond = decide_num::<Ctx>;
        let $s: SeparatedWhile<_, Comma, _, i64, U1, _, Ctx, _> =
          SeparatedWhile::new::<Comma>(&mut f, &mut cond);
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

sw_mr!(base_unb, |sw| sw, "1,2,3");
sw_mr!(base_min, |sw| AtLeast::new(sw, 2), "1,2,3");
sw_mr!(base_max, |sw| AtMost::new(sw, 3), "1,2,3");
sw_mr!(base_bnd, |sw| Bounded::new(sw, 4, 2), "1,2,3");

// == 1. allow_leading (4 count variants) ==

sw_mr!(al_unb, |sw| AllowLeading::new(sw), ",1,2,3");
sw_mr!(
  al_min,
  |sw| AllowLeading::new(AtLeast::new(sw, 2)),
  ",1,2,3"
);
sw_mr!(al_max, |sw| AllowLeading::new(AtMost::new(sw, 3)), ",1,2,3");
sw_mr!(
  al_bnd,
  |sw| AllowLeading::new(Bounded::new(sw, 4, 2)),
  ",1,2,3"
);

// == 2. allow_trailing (4 count variants) ==

sw_mr!(at_unb, |sw| AllowTrailing::new(sw), "1,2,3,");
sw_mr!(
  at_min,
  |sw| AllowTrailing::new(AtLeast::new(sw, 2)),
  "1,2,3,"
);
sw_mr!(
  at_max,
  |sw| AllowTrailing::new(AtMost::new(sw, 3)),
  "1,2,3,"
);
sw_mr!(
  at_bnd,
  |sw| AllowTrailing::new(Bounded::new(sw, 4, 2)),
  "1,2,3,"
);

// == 3. allow_surrounded (allow_leading + allow_trailing) (4 count variants) ==

sw_mr!(
  as_unb,
  |sw| AllowLeading::new(AllowTrailing::new(sw)),
  ",1,2,3,"
);
sw_mr!(
  as_min,
  |sw| AllowLeading::new(AllowTrailing::new(AtLeast::new(sw, 2))),
  ",1,2,3,"
);
sw_mr!(
  as_max,
  |sw| AllowLeading::new(AllowTrailing::new(AtMost::new(sw, 3))),
  ",1,2,3,"
);
sw_mr!(
  as_bnd,
  |sw| AllowLeading::new(AllowTrailing::new(Bounded::new(sw, 4, 2))),
  ",1,2,3,"
);

// == 4. require_leading (4 count variants) ==

sw_mr!(rl_unb, |sw| RequireLeading::new(sw), ",1,2,3");
sw_mr!(
  rl_min,
  |sw| RequireLeading::new(AtLeast::new(sw, 2)),
  ",1,2,3"
);
sw_mr!(
  rl_max,
  |sw| RequireLeading::new(AtMost::new(sw, 3)),
  ",1,2,3"
);
sw_mr!(
  rl_bnd,
  |sw| RequireLeading::new(Bounded::new(sw, 4, 2)),
  ",1,2,3"
);

// == 5. require_trailing (4 count variants) ==

sw_mr!(rt_unb, |sw| RequireTrailing::new(sw), "1,2,3,");
sw_mr!(
  rt_min,
  |sw| RequireTrailing::new(AtLeast::new(sw, 2)),
  "1,2,3,"
);
sw_mr!(
  rt_max,
  |sw| RequireTrailing::new(AtMost::new(sw, 3)),
  "1,2,3,"
);
sw_mr!(
  rt_bnd,
  |sw| RequireTrailing::new(Bounded::new(sw, 4, 2)),
  "1,2,3,"
);

// == 6. require_surrounded (require_leading + require_trailing) (4 count variants) ==

sw_mr!(
  rs_unb,
  |sw| RequireLeading::new(RequireTrailing::new(sw)),
  ",1,2,3,"
);
sw_mr!(
  rs_min,
  |sw| RequireLeading::new(RequireTrailing::new(AtLeast::new(sw, 2))),
  ",1,2,3,"
);
sw_mr!(
  rs_max,
  |sw| RequireLeading::new(RequireTrailing::new(AtMost::new(sw, 3))),
  ",1,2,3,"
);
sw_mr!(
  rs_bnd,
  |sw| RequireLeading::new(RequireTrailing::new(Bounded::new(sw, 4, 2))),
  ",1,2,3,"
);

// == 7. allow_leading_require_trailing (4 count variants) ==

sw_mr!(
  alrt_unb,
  |sw| AllowLeading::new(RequireTrailing::new(sw)),
  ",1,2,3,"
);
sw_mr!(
  alrt_min,
  |sw| AllowLeading::new(RequireTrailing::new(AtLeast::new(sw, 2))),
  ",1,2,3,"
);
sw_mr!(
  alrt_max,
  |sw| AllowLeading::new(RequireTrailing::new(AtMost::new(sw, 3))),
  ",1,2,3,"
);
sw_mr!(
  alrt_bnd,
  |sw| AllowLeading::new(RequireTrailing::new(Bounded::new(sw, 4, 2))),
  ",1,2,3,"
);

// == 8. require_leading_allow_trailing (4 count variants) ==

sw_mr!(
  rlat_unb,
  |sw| RequireLeading::new(AllowTrailing::new(sw)),
  ",1,2,3,"
);
sw_mr!(
  rlat_min,
  |sw| RequireLeading::new(AllowTrailing::new(AtLeast::new(sw, 2))),
  ",1,2,3,"
);
sw_mr!(
  rlat_max,
  |sw| RequireLeading::new(AllowTrailing::new(AtMost::new(sw, 3))),
  ",1,2,3,"
);
sw_mr!(
  rlat_bnd,
  |sw| RequireLeading::new(AllowTrailing::new(Bounded::new(sw, 4, 2))),
  ",1,2,3,"
);
