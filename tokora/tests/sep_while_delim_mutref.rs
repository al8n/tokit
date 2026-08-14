#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Coverage tests exercising the **mut-ref** (`Collect<&mut DelimitedBy<...>, &mut Container>`)
//! path (Impl #3) for `SeparatedWhile` delimited combinations across all
//! 8 separator policies × 4 count variants = 32 tests.

mod common;

use common::E;
use tokora::EmitterView;

use generic_arraydeque::typenum::U1;
use tokora::{
  Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  parser::{
    Action, AllowLeading, AllowTrailing, AtLeast, AtMost, Bounded, Collect, DelimitedBy,
    RequireLeading, RequireTrailing, SeparatedWhile,
  },
  punct::Bracket,
};

use common::{TestLexer, Token};

// -- Error type ---------------------------------------------------------------

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

// -- Condition + element parser -----------------------------------------------

fn decide_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>,
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

// -- Comma type alias (zero-sized punctuator) ---------------------------------

use tokora::punct::Comma;

// -- Test macro ---------------------------------------------------------------

macro_rules! sw_delim_mutref_tests {
  ($name:ident, $wrap:expr, $input:expr) => {
    paste::paste! {
      fn [<$name _mr>]<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Vec<i64>, E>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>
          + UnexpectedLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + UnexpectedTrailingSeparatorEmitter<'inp, TestLexer<'inp>>
          + MissingLeadingSeparatorEmitter<'inp, TestLexer<'inp>>
          + MissingTrailingSeparatorEmitter<'inp, TestLexer<'inp>>,
      {
        let mut f = |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<i64, E> {
          parse_num(inp)
        };
        let cond = |peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
                     emitter: EmitterView<'_, 'inp, TestLexer<'inp>, Ctx::Emitter>|
         -> Result<Action, E> { decide_num::<Ctx>(peeked, emitter) };
        let sw = SeparatedWhile::new::<Comma>(&mut f, cond);
        let wrap_fn: fn(
          SeparatedWhile<_, Comma, _, i64, U1, TestLexer<'inp>, Ctx>,
        ) -> _ = $wrap;
        let inner = wrap_fn(sw);
        let mut delim = DelimitedBy::<_, Bracket<(), (), ()>>::new(inner);
        let mut container = Vec::<i64>::new();
        let mut collect = Collect::new(&mut delim, &mut container);
        let _span: <TestLexer<'inp> as Lexer<'inp>>::Span = collect.parse_input(inp)?;
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

// == 1. No policy (plain) =====================================================

sw_delim_mutref_tests!(plain_unb, |sw| sw, "[1,2,3]");
sw_delim_mutref_tests!(plain_min, |sw| AtLeast::new(sw, 2), "[1,2,3]");
sw_delim_mutref_tests!(plain_max, |sw| AtMost::new(sw, 3), "[1,2,3]");
sw_delim_mutref_tests!(plain_bnd, |sw| Bounded::new(sw, 4, 2), "[1,2,3]");

// == 2. allow_leading =========================================================

sw_delim_mutref_tests!(al_unb, |sw| AllowLeading::new(sw), "[,1,2,3]");
sw_delim_mutref_tests!(
  al_min,
  |sw| AllowLeading::new(AtLeast::new(sw, 2)),
  "[,1,2,3]"
);
sw_delim_mutref_tests!(
  al_max,
  |sw| AllowLeading::new(AtMost::new(sw, 3)),
  "[,1,2,3]"
);
sw_delim_mutref_tests!(
  al_bnd,
  |sw| AllowLeading::new(Bounded::new(sw, 4, 2)),
  "[,1,2,3]"
);

// == 3. allow_trailing ========================================================

sw_delim_mutref_tests!(at_unb, |sw| AllowTrailing::new(sw), "[1,2,3,]");
sw_delim_mutref_tests!(
  at_min,
  |sw| AllowTrailing::new(AtLeast::new(sw, 2)),
  "[1,2,3,]"
);
sw_delim_mutref_tests!(
  at_max,
  |sw| AllowTrailing::new(AtMost::new(sw, 3)),
  "[1,2,3,]"
);
sw_delim_mutref_tests!(
  at_bnd,
  |sw| AllowTrailing::new(Bounded::new(sw, 4, 2)),
  "[1,2,3,]"
);

// == 4. allow_surrounded (allow_leading + allow_trailing) =====================

sw_delim_mutref_tests!(
  as_unb,
  |sw| AllowLeading::new(AllowTrailing::new(sw)),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  as_min,
  |sw| AllowLeading::new(AllowTrailing::new(AtLeast::new(sw, 2))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  as_max,
  |sw| AllowLeading::new(AllowTrailing::new(AtMost::new(sw, 3))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  as_bnd,
  |sw| AllowLeading::new(AllowTrailing::new(Bounded::new(sw, 4, 2))),
  "[,1,2,3,]"
);

// == 5. require_leading =======================================================

sw_delim_mutref_tests!(rl_unb, |sw| RequireLeading::new(sw), "[,1,2,3]");
sw_delim_mutref_tests!(
  rl_min,
  |sw| RequireLeading::new(AtLeast::new(sw, 2)),
  "[,1,2,3]"
);
sw_delim_mutref_tests!(
  rl_max,
  |sw| RequireLeading::new(AtMost::new(sw, 3)),
  "[,1,2,3]"
);
sw_delim_mutref_tests!(
  rl_bnd,
  |sw| RequireLeading::new(Bounded::new(sw, 4, 2)),
  "[,1,2,3]"
);

// == 6. require_trailing ======================================================

sw_delim_mutref_tests!(rt_unb, |sw| RequireTrailing::new(sw), "[1,2,3,]");
sw_delim_mutref_tests!(
  rt_min,
  |sw| RequireTrailing::new(AtLeast::new(sw, 2)),
  "[1,2,3,]"
);
sw_delim_mutref_tests!(
  rt_max,
  |sw| RequireTrailing::new(AtMost::new(sw, 3)),
  "[1,2,3,]"
);
sw_delim_mutref_tests!(
  rt_bnd,
  |sw| RequireTrailing::new(Bounded::new(sw, 4, 2)),
  "[1,2,3,]"
);

// == 7. require_surrounded (require_leading + require_trailing) ===============

sw_delim_mutref_tests!(
  rs_unb,
  |sw| RequireLeading::new(RequireTrailing::new(sw)),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  rs_min,
  |sw| RequireLeading::new(RequireTrailing::new(AtLeast::new(sw, 2))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  rs_max,
  |sw| RequireLeading::new(RequireTrailing::new(AtMost::new(sw, 3))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  rs_bnd,
  |sw| RequireLeading::new(RequireTrailing::new(Bounded::new(sw, 4, 2))),
  "[,1,2,3,]"
);

// == 8. allow_leading_require_trailing ========================================

sw_delim_mutref_tests!(
  alrt_unb,
  |sw| AllowLeading::new(RequireTrailing::new(sw)),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  alrt_min,
  |sw| AllowLeading::new(RequireTrailing::new(AtLeast::new(sw, 2))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  alrt_max,
  |sw| AllowLeading::new(RequireTrailing::new(AtMost::new(sw, 3))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  alrt_bnd,
  |sw| AllowLeading::new(RequireTrailing::new(Bounded::new(sw, 4, 2))),
  "[,1,2,3,]"
);

// == 9. require_leading_allow_trailing ========================================

sw_delim_mutref_tests!(
  rlat_unb,
  |sw| RequireLeading::new(AllowTrailing::new(sw)),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  rlat_min,
  |sw| RequireLeading::new(AllowTrailing::new(AtLeast::new(sw, 2))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  rlat_max,
  |sw| RequireLeading::new(AllowTrailing::new(AtMost::new(sw, 3))),
  "[,1,2,3,]"
);
sw_delim_mutref_tests!(
  rlat_bnd,
  |sw| RequireLeading::new(AllowTrailing::new(Bounded::new(sw, 4, 2))),
  "[,1,2,3,]"
);
