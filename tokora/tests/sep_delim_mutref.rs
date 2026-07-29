#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Tests exercising the **mut-ref** (`Collect<&mut DelimitedBy<...>, &mut Container>`)
//! code path (Impl #3) for every separator-policy x count-modifier combination
//! in the `parser/many/sep/delim/` directory.
//!
//! 32 policy combos + 3 base (no policy) = 35 tests.

mod common;

use common::E;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext, SimpleSpan,
  emitter::{
    Fatal, FullContainerEmitter, MissingLeadingSeparatorEmitter, MissingTrailingSeparatorEmitter,
    SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  parser::{
    AllowLeading, AllowTrailing, AtLeast, AtMost, Bounded, Collect, DelimitedBy, RequireLeading,
    RequireTrailing, Separated,
  },
  punct::{Bracket, Comma},
  try_parse_input::ParseAttempt,
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

/// Generates a mut-ref path test. The `$wrap` macro is called with the
/// `Separated` value to produce the inner parser for `DelimitedBy::new(...)`.
macro_rules! sep_delim_mutref_tests {
  ($name:ident, $wrap:ident, $input:expr) => {
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
        let mut f = |inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>| -> Result<ParseAttempt<i64>, E> {
          try_num(inp)
        };
        let sep = Separated::new::<Comma>(&mut f);
        let inner = $wrap!(sep);
        let mut delim = DelimitedBy::<_, Bracket<(), (), ()>>::new(inner);
        let mut container = Vec::<i64>::new();
        let mut collect = Collect::new(&mut delim, &mut container);
        let _span: SimpleSpan = collect.parse_input(inp)?;
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

// ── Wrapper macros for each policy+count combination ──────────────────────────

macro_rules! wrap_identity {
  ($sep:expr) => {
    $sep
  };
}
macro_rules! wrap_at_least {
  ($sep:expr) => {
    AtLeast::new($sep, 2)
  };
}
macro_rules! wrap_at_most {
  ($sep:expr) => {
    AtMost::new($sep, 3)
  };
}
macro_rules! wrap_al {
  ($sep:expr) => {
    AllowLeading::new($sep)
  };
}
macro_rules! wrap_al_min {
  ($sep:expr) => {
    AllowLeading::new(AtLeast::new($sep, 2))
  };
}
macro_rules! wrap_al_max {
  ($sep:expr) => {
    AllowLeading::new(AtMost::new($sep, 3))
  };
}
macro_rules! wrap_al_bnd {
  ($sep:expr) => {
    AllowLeading::new(Bounded::new($sep, 4, 2))
  };
}

macro_rules! wrap_at {
  ($sep:expr) => {
    AllowTrailing::new($sep)
  };
}
macro_rules! wrap_at_min {
  ($sep:expr) => {
    AllowTrailing::new(AtLeast::new($sep, 2))
  };
}
macro_rules! wrap_at_max {
  ($sep:expr) => {
    AllowTrailing::new(AtMost::new($sep, 3))
  };
}
macro_rules! wrap_at_bnd {
  ($sep:expr) => {
    AllowTrailing::new(Bounded::new($sep, 4, 2))
  };
}

macro_rules! wrap_as {
  ($sep:expr) => {
    AllowLeading::new(AllowTrailing::new($sep))
  };
}
macro_rules! wrap_as_min {
  ($sep:expr) => {
    AllowLeading::new(AllowTrailing::new(AtLeast::new($sep, 2)))
  };
}
macro_rules! wrap_as_max {
  ($sep:expr) => {
    AllowLeading::new(AllowTrailing::new(AtMost::new($sep, 3)))
  };
}
macro_rules! wrap_as_bnd {
  ($sep:expr) => {
    AllowLeading::new(AllowTrailing::new(Bounded::new($sep, 4, 2)))
  };
}

macro_rules! wrap_rl {
  ($sep:expr) => {
    RequireLeading::new($sep)
  };
}
macro_rules! wrap_rl_min {
  ($sep:expr) => {
    RequireLeading::new(AtLeast::new($sep, 2))
  };
}
macro_rules! wrap_rl_max {
  ($sep:expr) => {
    RequireLeading::new(AtMost::new($sep, 3))
  };
}
macro_rules! wrap_rl_bnd {
  ($sep:expr) => {
    RequireLeading::new(Bounded::new($sep, 4, 2))
  };
}

macro_rules! wrap_rt {
  ($sep:expr) => {
    RequireTrailing::new($sep)
  };
}
macro_rules! wrap_rt_min {
  ($sep:expr) => {
    RequireTrailing::new(AtLeast::new($sep, 2))
  };
}
macro_rules! wrap_rt_max {
  ($sep:expr) => {
    RequireTrailing::new(AtMost::new($sep, 3))
  };
}
macro_rules! wrap_rt_bnd {
  ($sep:expr) => {
    RequireTrailing::new(Bounded::new($sep, 4, 2))
  };
}

macro_rules! wrap_rs {
  ($sep:expr) => {
    RequireLeading::new(RequireTrailing::new($sep))
  };
}
macro_rules! wrap_rs_min {
  ($sep:expr) => {
    RequireLeading::new(RequireTrailing::new(AtLeast::new($sep, 2)))
  };
}
macro_rules! wrap_rs_max {
  ($sep:expr) => {
    RequireLeading::new(RequireTrailing::new(AtMost::new($sep, 3)))
  };
}
macro_rules! wrap_rs_bnd {
  ($sep:expr) => {
    RequireLeading::new(RequireTrailing::new(Bounded::new($sep, 4, 2)))
  };
}

macro_rules! wrap_alrt {
  ($sep:expr) => {
    AllowLeading::new(RequireTrailing::new($sep))
  };
}
macro_rules! wrap_alrt_min {
  ($sep:expr) => {
    AllowLeading::new(RequireTrailing::new(AtLeast::new($sep, 2)))
  };
}
macro_rules! wrap_alrt_max {
  ($sep:expr) => {
    AllowLeading::new(RequireTrailing::new(AtMost::new($sep, 3)))
  };
}
macro_rules! wrap_alrt_bnd {
  ($sep:expr) => {
    AllowLeading::new(RequireTrailing::new(Bounded::new($sep, 4, 2)))
  };
}

macro_rules! wrap_rlat {
  ($sep:expr) => {
    RequireLeading::new(AllowTrailing::new($sep))
  };
}
macro_rules! wrap_rlat_min {
  ($sep:expr) => {
    RequireLeading::new(AllowTrailing::new(AtLeast::new($sep, 2)))
  };
}
macro_rules! wrap_rlat_max {
  ($sep:expr) => {
    RequireLeading::new(AllowTrailing::new(AtMost::new($sep, 3)))
  };
}
macro_rules! wrap_rlat_bnd {
  ($sep:expr) => {
    RequireLeading::new(AllowTrailing::new(Bounded::new($sep, 4, 2)))
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// 0. No policy (base case, 3 count variants — bounded has no impl)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(base_unb, wrap_identity, "[1,2,3]");
sep_delim_mutref_tests!(base_min, wrap_at_least, "[1,2,3]");
sep_delim_mutref_tests!(base_max, wrap_at_most, "[1,2,3]");

// ═══════════════════════════════════════════════════════════════════════════════
// 1. allow_leading (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(al_unb, wrap_al, "[,1,2,3]");
sep_delim_mutref_tests!(al_min, wrap_al_min, "[,1,2,3]");
sep_delim_mutref_tests!(al_max, wrap_al_max, "[,1,2,3]");
sep_delim_mutref_tests!(al_bnd, wrap_al_bnd, "[,1,2,3]");

// ═══════════════════════════════════════════════════════════════════════════════
// 2. allow_trailing (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(at_unb, wrap_at, "[1,2,3,]");
sep_delim_mutref_tests!(at_min, wrap_at_min, "[1,2,3,]");
sep_delim_mutref_tests!(at_max, wrap_at_max, "[1,2,3,]");
sep_delim_mutref_tests!(at_bnd, wrap_at_bnd, "[1,2,3,]");

// ═══════════════════════════════════════════════════════════════════════════════
// 3. allow_surrounded (AllowLeading<AllowTrailing<...>>) (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(as_unb, wrap_as, "[,1,2,3,]");
sep_delim_mutref_tests!(as_min, wrap_as_min, "[,1,2,3,]");
sep_delim_mutref_tests!(as_max, wrap_as_max, "[,1,2,3,]");
sep_delim_mutref_tests!(as_bnd, wrap_as_bnd, "[,1,2,3,]");

// ═══════════════════════════════════════════════════════════════════════════════
// 4. require_leading (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(rl_unb, wrap_rl, "[,1,2,3]");
sep_delim_mutref_tests!(rl_min, wrap_rl_min, "[,1,2,3]");
sep_delim_mutref_tests!(rl_max, wrap_rl_max, "[,1,2,3]");
sep_delim_mutref_tests!(rl_bnd, wrap_rl_bnd, "[,1,2,3]");

// ═══════════════════════════════════════════════════════════════════════════════
// 5. require_trailing (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(rt_unb, wrap_rt, "[1,2,3,]");
sep_delim_mutref_tests!(rt_min, wrap_rt_min, "[1,2,3,]");
sep_delim_mutref_tests!(rt_max, wrap_rt_max, "[1,2,3,]");
sep_delim_mutref_tests!(rt_bnd, wrap_rt_bnd, "[1,2,3,]");

// ═══════════════════════════════════════════════════════════════════════════════
// 6. require_surrounded (RequireLeading<RequireTrailing<...>>) (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(rs_unb, wrap_rs, "[,1,2,3,]");
sep_delim_mutref_tests!(rs_min, wrap_rs_min, "[,1,2,3,]");
sep_delim_mutref_tests!(rs_max, wrap_rs_max, "[,1,2,3,]");
sep_delim_mutref_tests!(rs_bnd, wrap_rs_bnd, "[,1,2,3,]");

// ═══════════════════════════════════════════════════════════════════════════════
// 7. allow_leading_require_trailing (AllowLeading<RequireTrailing<...>>)
//    (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(alrt_unb, wrap_alrt, "[,1,2,3,]");
sep_delim_mutref_tests!(alrt_min, wrap_alrt_min, "[,1,2,3,]");
sep_delim_mutref_tests!(alrt_max, wrap_alrt_max, "[,1,2,3,]");
sep_delim_mutref_tests!(alrt_bnd, wrap_alrt_bnd, "[,1,2,3,]");

// ═══════════════════════════════════════════════════════════════════════════════
// 8. require_leading_allow_trailing (RequireLeading<AllowTrailing<...>>)
//    (4 count variants)
// ═══════════════════════════════════════════════════════════════════════════════

sep_delim_mutref_tests!(rlat_unb, wrap_rlat, "[,1,2,3,]");
sep_delim_mutref_tests!(rlat_min, wrap_rlat_min, "[,1,2,3,]");
sep_delim_mutref_tests!(rlat_max, wrap_rlat_max, "[,1,2,3,]");
sep_delim_mutref_tests!(rlat_bnd, wrap_rlat_bnd, "[,1,2,3,]");
