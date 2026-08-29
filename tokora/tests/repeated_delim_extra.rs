#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! The **spanned owning** destination on the two plain delimited families, `delim/repeated` and
//! `delim/repeated_while`, over all four count variants.
//!
//! Spelled the way the *plain* families spell it — `Collect<..>` parsed straight to
//! `Spanned<Container, L::Span>`, no `With<.., PhantomSpan>` wrapper, which is the separated
//! families' shape and is what `sep_delim_extra.rs` and `sep_while_delim_extra.rs` exercise. A
//! delimited plain list is the plain loop under a delimiter, so it inherits the plain loop's
//! destination shape and not its delimited neighbour's.
//!
//! These two families had no file here because they had no spanned impl to exercise: they
//! implemented the owning destination alone until #259 stage 3. The borrowed destination's
//! counterpart is `repetition_behavioural_matrix.rs`'s borrowed table, whose eight `b_drep_*` /
//! `b_drepw_*` rows arrived in the same change.
//!
//! Eight impls, eight rows: the four `unbounded` / `at_least` / `at_most` / `bounded`
//! specialisations of each family. The bounds are chosen so every row lands on the SUCCESS arm —
//! an impl that returned the span of an empty collection would pass a smoke test that only asked
//! whether it returned, so each row asserts the container came back with the elements in it and
//! the span covers the delimiters.

mod common;

use common::E;
use tokora::EmitterView;

use generic_arraydeque::typenum::U1;
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan, TryParseInput,
  cache::Peeked,
  emitter::{
    Fatal, FullContainerEmitter, SeparatedEmitter, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
  },
  parser::Action,
  punct::Bracket,
  span::Spanned,
  try_parse_input::ParseAttempt,
};

use common::{TestLexer, Token};

fn full_ctx() -> ParserContext<'static, TestLexer<'static>, Fatal<E>> {
  ParserContext::new(Fatal::new())
}

/// The `TryParseInput` element `delim/repeated` drives: a number, or a decline.
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

/// The `ParseInput` element `delim/repeated_while` drives, with `decide_num` as its condition.
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

/// `[1 2 3]` — three elements, so it satisfies every bound spelled below.
const INPUT: &str = "[1 2 3]";

macro_rules! drep_spanned {
  ($name:ident, { $($bound:tt)* }) => {
    paste::paste! {
      fn [<$name _sp>]<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Spanned<Vec<i64>, SimpleSpan>, E>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        try_num
          .repeated()
          $($bound)*
          .delimited::<Bracket<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      #[test]
      fn [<$name _spanned>]() {
        let r = Parser::with_context(full_ctx())
          .apply([<$name _sp>])
          .parse_str(INPUT)
          .unwrap();
        assert_eq!(r.data(), &vec![1, 2, 3]);
        assert_eq!(r.span().start(), 0);
        assert_eq!(r.span().end(), INPUT.len());
      }
    }
  };
}

macro_rules! drepw_spanned {
  ($name:ident, { $($bound:tt)* }) => {
    paste::paste! {
      fn [<$name _sp>]<'inp, Ctx>(
        inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
      ) -> Result<Spanned<Vec<i64>, SimpleSpan>, E>
      where
        Ctx: ParseContext<'inp, TestLexer<'inp>>,
        Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = E>
          + FullContainerEmitter<'inp, TestLexer<'inp>>
          + UnclosedEmitter<'inp, TestLexer<'inp>>
          + SeparatedEmitter<'inp, TestLexer<'inp>>
          + TooFewEmitter<'inp, TestLexer<'inp>>
          + TooManyEmitter<'inp, TestLexer<'inp>>,
      {
        parse_num
          .repeated_while::<_, U1>(decide_num::<Ctx>)
          $($bound)*
          .delimited::<Bracket<(), (), ()>>()
          .collect()
          .parse_input(inp)
      }

      #[test]
      fn [<$name _spanned>]() {
        let r = Parser::with_context(full_ctx())
          .apply([<$name _sp>])
          .parse_str(INPUT)
          .unwrap();
        assert_eq!(r.data(), &vec![1, 2, 3]);
        assert_eq!(r.span().start(), 0);
        assert_eq!(r.span().end(), INPUT.len());
      }
    }
  };
}

// ── delim/repeated ────────────────────────────────────────────────────────────

drep_spanned!(drep_unbounded, {});
drep_spanned!(drep_at_least, { .at_least(2) });
drep_spanned!(drep_at_most, { .at_most(3) });
drep_spanned!(drep_bounded, { .bounded(1, 3) });

// ── delim/repeated_while ──────────────────────────────────────────────────────

drepw_spanned!(drepw_unbounded, {});
drepw_spanned!(drepw_at_least, { .at_least(2) });
drepw_spanned!(drepw_at_most, { .at_most(3) });
drepw_spanned!(drepw_bounded, { .bounded(1, 3) });
