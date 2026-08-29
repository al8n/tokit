use crate::emitter::TooFewEmitter;

use super::*;

impl<'inp, L, F, Condition, O, Container, Ctx, Lang: ?Sized, W>
  ParseInput<'inp, L, Container, Ctx, Lang>
  for Collect<AtLeast<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang>>, Container, Ctx, Lang>
where
  L: Lexer<'inp>,
  F: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  W: Window,
  Ctx::Emitter: TooFewEmitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Ctx: ParseContext<'inp, L, Lang>,
  Container: Default + crate::container::Container<O>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self
      .attempt(|mut c| c.parse_input(inp))
      .map(|(_, collected)| collected)
  }
}

impl<'inp, L, F, Condition, O, Container, Ctx, Lang: ?Sized, W>
  ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang>
  for With<
    Collect<AtLeast<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang>>, Container, Ctx, Lang>,
    PhantomSpan,
  >
where
  L: Lexer<'inp>,
  F: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  W: Window,
  Ctx::Emitter: TooFewEmitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Ctx: ParseContext<'inp, L, Lang>,
  Container: Default + crate::container::Container<O>,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self
      .primary_mut()
      .attempt(|mut c| c.parse_input(inp))
      .map(|(span, collected)| Spanned::new(span, collected))
  }
}

impl<'inp, 'c, L, F, Condition, O, Container, Ctx, Lang: ?Sized, W>
  ParseInput<'inp, L, L::Span, Ctx, Lang>
  for Collect<
    &'c mut AtLeast<RepeatedWhile<F, Condition, O, W, L, Ctx, Lang>>,
    &'c mut Container,
    Ctx,
    Lang,
  >
where
  L: Lexer<'inp>,
  F: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  W: Window,
  Ctx::Emitter: TooFewEmitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Ctx: ParseContext<'inp, L, Lang>,
  Container: crate::container::Container<O>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self
      .parser
      .parser
      .parse(inp, &mut self.container, &self.parser.minimum)
  }
}
