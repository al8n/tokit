use core::marker::PhantomData;

use crate::{TryParseInput, try_parse_input::ParseAttempt};

// Only `map_with` below names it, and that method is `map`'s.
#[cfg(feature = "map")]
use crate::parse_state::ParseState;

use super::*;

/// A combinator that wraps a `TryParseInput` parser, producing a parser that will apply combinators to the accepted output.
pub struct Accepted<P, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  pub(crate) parser: P,
  _l: PhantomData<L>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _o: PhantomData<O>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, O, L, Ctx, Lang: ?Sized, Cmpl> Accepted<P, L, O, Ctx, Lang, Cmpl> {
  /// Creates a new `Accepted` parser.
  #[inline(always)]
  pub(crate) const fn new(parser: P) -> Self {
    Self {
      parser,
      _l: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _o: PhantomData,
      _cmpl: PhantomData,
    }
  }

  /// Transforms the output of this parser using the given function.
  #[cfg(feature = "map")]
  #[cfg_attr(docsrs, doc(cfg(feature = "map")))]
  #[inline(always)]
  pub fn map<'inp, U, F>(
    self,
    f: F,
  ) -> Accepted<Map<P, F, L, Ctx, O, U, Lang, Cmpl>, L, U, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    F: FnMut(O) -> U + 'inp,
  {
    Accepted::new(Map::new(self.parser, f))
  }

  /// Transforms the output of this parser using the given function.
  #[cfg(feature = "map")]
  #[cfg_attr(docsrs, doc(cfg(feature = "map")))]
  #[inline(always)]
  pub fn map_with<'inp, U, F>(
    self,
    f: F,
  ) -> Accepted<MapWith<P, F, L, Ctx, O, U, Lang, Cmpl>, L, U, Ctx, Lang, Cmpl>
  where
    Self: Sized,
    L: Lexer<'inp>,
    F: FnMut(O, ParseState<'_, 'inp, '_, L, Ctx, Lang>) -> U,
  {
    Accepted::new(MapWith::new(self.parser, f))
  }
}

impl<'inp, L, P, O, Ctx, Lang: ?Sized, Cmpl> ParseInput<'inp, L, ParseAttempt<O>, Ctx, Lang, Cmpl>
  for Accepted<P, L, O, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.try_parse_input(input)
  }
}

impl<'inp, L, P, O, Ctx, Lang: ?Sized, Cmpl> ParseInput<'inp, L, Option<O>, Ctx, Lang, Cmpl>
  for Accepted<P, L, O, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Option<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.try_parse_input(input).map(Into::into)
  }
}

impl<'inp, L, P, O, Ctx, Lang: ?Sized, Cmpl> TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Accepted<P, L, O, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn try_parse_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.try_parse_input(input)
  }
}
