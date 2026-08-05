#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! `SeparatorHandler::on_separator`'s delivery law.
//!
//! Every separator token a driver consumes is delivered exactly once, in source order — the
//! leading one, every one between elements, every duplicate in a run, and the trailing one.
//! Before this was fixed the drivers delivered the exact *complement* of that set: only the
//! anomalous separators (leading and duplicates) arrived, and the happy-path and trailing ones
//! never did, so a container that recorded separators recorded the wrong ones.
//!
//! Every in-crate container is a blackhole for this event, so nothing in the tree could
//! observe the defect. The recording container below is the observer the crate lacked.
//!
//! # Sentinel token
//!
//! The two non-delimited `*_while` drivers consult their condition through a lookahead window
//! and must not be asked to peek at EOF, so their inputs end in `+`.

mod common;

use common::{E, TestLexer, Token};
use tokora::EmitterView;
use tokora::{
  Accumulator, InputRef, Lexer, Parse, ParseInput, Parser, ParserContext, TryParseInput,
  cache::Peeked,
  container::Container,
  emitter::Verbose,
  parser::{Action, DelimiterHandler, SeparatorHandler},
  punct::Bracket,
  span::{Span, Spanned},
  try_parse_input::ParseAttempt,
  utils::typenum::U1,
};

// ── The observer the crate lacks ──────────────────────────────────────────────

/// A container that actually implements `on_separator`, recording the start offset of every
/// separator it is handed. Deliberately does **not** override `OBSERVES_SEPARATORS`, so it
/// exercises the defaulted `true` path a downstream implementor gets.
#[derive(Debug, Default)]
struct Recording {
  elems: Vec<i64>,
  seps: Vec<usize>,
  delims: Vec<usize>,
}

impl Container<i64> for Recording {
  fn push(&mut self, item: i64) -> Result<(), i64> {
    self.elems.push(item);
    Ok(())
  }

  fn first(&self) -> Option<&i64> {
    self.elems.first()
  }

  fn last(&self) -> Option<&i64> {
    self.elems.last()
  }

  fn len(&self) -> usize {
    self.elems.len()
  }

  fn max_capacity(&self) -> usize {
    usize::MAX
  }
}

// Written against a generic `L` with a `usize` offset rather than `TestLexer` directly: inside
// a `where TestLexer<'inp>: Lexer<'inp>` method body the offset is an opaque associated type,
// whereas a param-env bound carries its value.
impl<'inp, L> SeparatorHandler<'inp, L> for Recording
where
  L: Lexer<'inp>,
  L::Span: Span<Offset = usize>,
{
  fn on_separator(&mut self, sep: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    self.seps.push(sep.span_ref().start());
  }
}

impl<'inp, L> DelimiterHandler<'inp, L> for Recording
where
  L: Lexer<'inp>,
  L::Span: Span<Offset = usize>,
{
  fn on_open_delimiter(&mut self, open: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    self.delims.push(open.span_ref().start());
  }

  fn on_close_delimiter(&mut self, close: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    self.delims.push(close.span_ref().start());
  }
}

// ── Fixture ───────────────────────────────────────────────────────────────────

type VCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<E>>;

fn verbose_ctx() -> VCtx<'static> {
  ParserContext::new(Verbose::new())
}

fn try_num<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
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

fn parse_num<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<i64, E> {
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
  _: EmitterView<'_, 'inp, TestLexer<'inp>, Verbose<E>>,
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

macro_rules! delivery {
  ($name:ident, $src:literal, $expected:expr, $inp:ident => $build:expr) => {
    #[test]
    fn $name() {
      fn go<'inp>(
        $inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
      ) -> Result<Recording, E> {
        $build
      }
      let rec = Parser::with_context(verbose_ctx())
        .apply(go)
        .parse_str($src)
        .unwrap();
      let expected: Vec<usize> = $expected;
      assert_eq!(
        rec.seps, expected,
        concat!(
          stringify!($name),
          " on ",
          $src,
          ": every consumed separator is delivered exactly once, in source order"
        )
      );
    }
  };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Happy-path separators: `1,2,3` has commas at offsets 1 and 3. Before the fix the
// `State::Element -> Separator` arm stored the token without ever delivering it, so a
// recording container saw NOTHING on a well-formed list.
// ═══════════════════════════════════════════════════════════════════════════════

delivery!(happy_sep, "1,2,3", vec![1, 3], inp =>
  try_num.separated_by_comma().collect().parse_input(inp));

delivery!(happy_sep_delim, "[1,2,3]", vec![2, 4], inp =>
  try_num.separated_by_comma().delimited::<Bracket<(), (), ()>>().collect().parse_input(inp));

delivery!(happy_sep_while, "1,2,3+", vec![1, 3], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).collect().parse_input(inp));

delivery!(happy_sep_while_delim, "[1,2,3]", vec![2, 4], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp));

// ═══════════════════════════════════════════════════════════════════════════════
// The full ordered set: `,1,,2,` mixes a leading separator (offset 0), a happy-path one
// (2), a duplicate (3) and a trailing one (5). Before the fix only the anomalous two —
// offsets 0 and 3 — were delivered, i.e. the exact complement of the documented law and
// out of source order relative to the ones that were missing.
// ═══════════════════════════════════════════════════════════════════════════════

delivery!(every_position_sep, ",1,,2,", vec![0, 2, 3, 5], inp =>
  try_num.separated_by_comma().collect().parse_input(inp));

delivery!(every_position_sep_delim, "[,1,,2,]", vec![1, 3, 4, 6], inp =>
  try_num.separated_by_comma().delimited::<Bracket<(), (), ()>>().collect().parse_input(inp));

delivery!(every_position_sep_while, ",1,,2,+", vec![0, 2, 3, 5], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num).collect().parse_input(inp));

delivery!(every_position_sep_while_delim, "[,1,,2,]", vec![1, 3, 4, 6], inp =>
  parse_num.separated_by_comma_while::<_, U1>(decide_num)
    .delimited::<Bracket<(), (), ()>>().collect().parse_input(inp));

// ═══════════════════════════════════════════════════════════════════════════════
// The opt-out guard. `OBSERVES_SEPARATORS = false` is what keeps delivery free for the
// crate's own containers — every one of them is a blackhole, and the constant folds the
// call and its clone out of the generated code entirely. This container has a real
// `on_separator` body and still must receive nothing, so the guard is proven to be read at
// the delivery site rather than merely declared on the impl.
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Default)]
struct Deaf {
  elems: Vec<i64>,
  seps: Vec<usize>,
}

impl Container<i64> for Deaf {
  fn push(&mut self, item: i64) -> Result<(), i64> {
    self.elems.push(item);
    Ok(())
  }

  fn first(&self) -> Option<&i64> {
    self.elems.first()
  }

  fn last(&self) -> Option<&i64> {
    self.elems.last()
  }

  fn len(&self) -> usize {
    self.elems.len()
  }

  fn max_capacity(&self) -> usize {
    usize::MAX
  }
}

impl<'inp, L> SeparatorHandler<'inp, L> for Deaf
where
  L: Lexer<'inp>,
  L::Span: Span<Offset = usize>,
{
  const OBSERVES_SEPARATORS: bool = false;

  fn on_separator(&mut self, sep: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
    self.seps.push(sep.span_ref().start());
  }
}

impl<'inp, L> DelimiterHandler<'inp, L> for Deaf
where
  L: Lexer<'inp>,
{
  fn on_open_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
  }

  fn on_close_delimiter(&mut self, _: Spanned<L::Token, L::Span>)
  where
    L: Lexer<'inp>,
  {
  }
}

#[test]
fn opting_out_suppresses_delivery() {
  fn go<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<Deaf, E> {
    try_num.separated_by_comma().collect().parse_input(inp)
  }
  let rec = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str(",1,,2,")
    .unwrap();
  assert_eq!(
    rec.elems,
    vec![1, 2],
    "the elements are still collected — only the separator channel is off"
  );
  assert!(
    rec.seps.is_empty(),
    "`OBSERVES_SEPARATORS = false` must suppress every delivery, including the anomalous \
     separators the drivers used to hand over unconditionally; got {:?}",
    rec.seps
  );
}
