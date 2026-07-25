use crate::emitter::{TooFewEmitter, TooManyEmitter};

use super::*;

impl<
  'inp,
  L,
  F,
  O,
  Container,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
> ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>
  for Collect<Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>>, Container, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx::Emitter: TooFewEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + FullContainerEmitter<'inp, L, Lang>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: Default + crate::container::Container<O>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self
      .as_mut()
      .parse_input(inp)
      .map(|_| core::mem::take(&mut self.container))
  }
}

impl<
  'inp,
  L,
  F,
  O,
  Container,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
> ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang, Cmpl>
  for Collect<Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>>, Container, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx::Emitter: TooFewEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + FullContainerEmitter<'inp, L, Lang>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: Default + crate::container::Container<O>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self
      .as_mut()
      .parse_input(inp)
      .map(|span| Spanned::new(span, core::mem::take(&mut self.container)))
  }
}

impl<
  'inp,
  'c,
  L,
  F,
  O,
  Container,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
> ParseInput<'inp, L, L::Span, Ctx, Lang, Cmpl>
  for Collect<
    &'c mut Bounded<Repeated<F, O, L, Ctx, Lang, Cmpl>>,
    &'c mut Container,
    Ctx,
    Lang,
    Cmpl,
  >
where
  L: Lexer<'inp>,
  F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx::Emitter: TooFewEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + FullContainerEmitter<'inp, L, Lang>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: crate::container::Container<O>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let max = self.parser.maximum();
    let min = self.parser.minimum();

    self
      .parser
      .parser
      .parse(inp, &mut self.container, &With::new(min, max))
  }
}
