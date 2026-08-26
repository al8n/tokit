#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Owning collectors are reusable, and one attempt's elements never reach the next one.
//!
//! An owning `Collect`'s container is parser-internal state on its way to becoming the return
//! value. The transfer used to run on the success arm only, so an attempt that accepted elements
//! and then failed left them behind and the *next* attempt appended to them — a successful parse
//! could return values the caller never fed it, and a long-lived parser reused across inputs
//! mixed them.
//!
//! # What each row has to be
//!
//! Every row here reuses **one** collector: it fails, then succeeds, and the assertion is on the
//! second attempt's *contents*. Asserting that the first attempt returned `Err` would pass
//! against the defect, because the defect is invisible until the collector is used again.
//!
//! The failure is produced by `at_most(1)` under a fail-fast emitter: element 1 is accepted,
//! element 2 trips the count bound and the emitter turns that into `Err` — so the first attempt
//! has genuinely collected something before it fails, which is the whole premise. The trip lands
//! at element 2 in every family now (`many::admit_element`), so every failing attempt stops with
//! its construct still open; `$step_over` is what walks the second attempt past the token the
//! first one left.
//!
//! The panic row is the one that separates "move the storage out **before** the attempt" from
//! "take it out afterwards on both arms": only the first survives an unwind.

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};

use common::{E, TestLexer, Token};
use tokora::{
  Accumulator, EmitterView, InputRef, Parse, ParseInput, Parser, ParserContext, TryParseInput,
  cache::Peeked,
  emitter::Fatal,
  parser::{Action, With},
  punct::Bracket,
  span::Spanned,
  try_parse_input::ParseAttempt,
  utils::{marker::PhantomSpan, typenum::U1},
};

type Ctx<'inp> = ParserContext<'inp, TestLexer<'inp>, Fatal<E>>;

fn fatal_ctx() -> Ctx<'static> {
  ParserContext::new(Fatal::new())
}

fn try_num<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
) -> Result<ParseAttempt<i64>, E> {
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

fn parse_num<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<i64, E> {
  match inp.next()? {
    None => Err(E),
    Some(tok) => match tok.into_data() {
      Token::Num(n) => Ok(n),
      _ => Err(E),
    },
  }
}

fn decide_num<'inp>(
  mut peeked: Peeked<'_, 'inp, TestLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Fatal<E>>,
) -> Result<Action, E> {
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

/// Reuses one owning collector across a failed attempt and a successful one, and asserts what
/// the *second* attempt returned.
macro_rules! reuse {
  ($name:ident, $out:ty, $src:literal, $step_over:literal, $expected:expr, $c:ident => $build:expr) => {
    #[test]
    fn $name() {
      fn go<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<$out, E> {
        let mut $c = $build;
        // First attempt: accepts an element, then fails on the over-limit one.
        let first: Result<$out, E> = $c.parse_input(inp);
        assert!(
          first.is_err(),
          concat!(
            stringify!($name),
            ": the first attempt must fail after collecting"
          )
        );
        // Every driver now detects a violated maximum at the element that broke it, so a failing
        // attempt stops *inside* its construct rather than after it: the separated families stop
        // on the token that ended the list, and the delimited ones stop before their closer. Step
        // over that one token so the second attempt has a construct of its own to parse.
        if $step_over {
          inp.next()?;
        }
        // Second attempt, same collector, rest of the input.
        $c.parse_input(inp)
      }
      let got = Parser::with_context(fatal_ctx())
        .apply(go)
        .parse_str($src)
        .expect("the second attempt parses cleanly");
      assert_eq!(
        got, $expected,
        concat!(
          stringify!($name),
          ": a reused owning collector must return only what the new attempt parsed"
        )
      );
    }
  };
}

// ── The four collection families, unspanned ───────────────────────────────────

reuse!(repeated, Vec<i64>, "1 2 3", false, vec![3], c =>
  try_num.repeated().at_most(1).collect());

reuse!(repeated_while, Vec<i64>, "1 2 3 +", false, vec![3], c =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(1).collect());

reuse!(separated, Vec<i64>, "1,2;3", true, vec![3], c =>
  try_num.separated_by_comma().at_most(1).collect());

reuse!(separated_while, Vec<i64>, "1,2;3+", true, vec![3], c =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(1).collect());

// ── The delimited forms of all four ───────────────────────────────────────────

reuse!(delim_repeated, Vec<i64>, "[1 2][3]", true, vec![3], c =>
  try_num.repeated().at_most(1).delimited::<Bracket<(), (), ()>>().collect());

reuse!(delim_repeated_while, Vec<i64>, "[1 2][3]", true, vec![3], c =>
  parse_num.repeated_while::<_, U1>(decide_num).at_most(1)
    .delimited::<Bracket<(), (), ()>>().collect());

reuse!(delim_separated, Vec<i64>, "[1,2][3]", true, vec![3], c =>
  try_num.separated_by_comma().at_most(1)
    .delimited::<Bracket<(), (), ()>>().collect());

reuse!(delim_separated_while, Vec<i64>, "[1,2][3]", true, vec![3], c =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).at_most(1)
    .delimited::<Bracket<(), (), ()>>().collect());

// ── The spanned owning outputs, which take through the same helper ────────────

#[test]
fn spanned_repeated() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<Spanned<Vec<i64>, tokora::span::SimpleSpan>, E> {
    let mut c = try_num.repeated().at_most(1).collect();
    let first: Result<Spanned<Vec<i64>, _>, E> = c.parse_input(inp);
    assert!(first.is_err());
    c.parse_input(inp)
  }
  let got = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("1 2 3")
    .expect("the second attempt parses cleanly");
  assert_eq!(*got.data(), vec![3]);
}

#[test]
fn spanned_separated() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<Spanned<Vec<i64>, tokora::span::SimpleSpan>, E> {
    let mut c = With::new(
      try_num.separated_by_comma().at_most(1).collect(),
      PhantomSpan::PHANTOM,
    );
    let first: Result<Spanned<Vec<i64>, _>, E> = c.parse_input(inp);
    assert!(first.is_err());
    inp.next()?;
    c.parse_input(inp)
  }
  let got = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("1,2;3")
    .expect("the second attempt parses cleanly");
  assert_eq!(*got.data(), vec![3]);
}

// ── The seed, and the unwind ──────────────────────────────────────────────────

/// A `collect_with` seed belongs to the first attempt and shares its fate. A failed attempt
/// drops the seed with the partial elements collected on top of it, rather than leaving the two
/// as the next attempt's starting point.
#[test]
fn a_failed_attempt_drops_its_seed() {
  fn go<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<Vec<i64>, E> {
    let mut c = try_num.repeated().at_most(1).collect_with(vec![100i64]);
    let first: Result<Vec<i64>, E> = c.parse_input(inp);
    assert!(first.is_err());
    c.parse_input(inp)
  }
  let got = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("1 2 3")
    .expect("the second attempt parses cleanly");
  assert_eq!(
    got,
    vec![3],
    "neither the seed nor the failed attempt's element may seed the next attempt"
  );
}

/// The row that pins *when* the storage moves. Taking the container out after the attempt on
/// both arms passes every row above; only moving it out **before** the attempt survives an
/// unwind, because from then on the partial collection is a local that ordinary unwinding drops.
#[test]
fn a_caught_panic_leaves_no_residue() {
  fn boom<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<ParseAttempt<i64>, E> {
    let attempt = try_num(inp)?;
    if let ParseAttempt::Accept(2) = attempt {
      panic!("element parser panicked on 2");
    }
    Ok(attempt)
  }

  fn go<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<Vec<i64>, E> {
    let mut c = boom.repeated().collect();
    let unwound = catch_unwind(AssertUnwindSafe(|| {
      let _: Vec<i64> = c.parse_input(inp)?;
      Ok::<(), E>(())
    }));
    assert!(unwound.is_err(), "the element parser must have panicked");
    c.parse_input(inp)
  }

  let previous = std::panic::take_hook();
  std::panic::set_hook(Box::new(|_| {}));
  let got = Parser::with_context(fatal_ctx())
    .apply(go)
    .parse_str("1 2 3");
  std::panic::set_hook(previous);

  assert_eq!(
    got.expect("the second attempt parses cleanly"),
    vec![3],
    "the element the panicking attempt had already collected must not survive into the next one"
  );
}
