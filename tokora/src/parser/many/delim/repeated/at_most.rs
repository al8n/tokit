use crate::{container::Container as ContainerT, emitter::TooManyEmitter};

use super::*;

impl<
  'inp,
  L,
  P,
  O,
  Container,
  Ctx,
  Delim,
  Lang: ?Sized,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
> ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>
  for Collect<
    DelimitedBy<AtMost<Repeated<P, O, L, Ctx, Lang, Cmpl>>, Delim>,
    Container,
    Ctx,
    Lang,
    Cmpl,
  >
where
  Delim: Delimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: Default + ContainerT<O> + DelimiterHandler<'inp, L>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let maximum = self.parser.parser.maximum();

    self
      .attempt(|c| {
        let Collect {
          parser, container, ..
        } = c;
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut()).parse_repeated(
          inp,
          container,
          &maximum,
          // The maximum is NOT re-checked here. It is settled at the element that broke it, from
          // inside `many::admit_element` — a construct exceeds `max` iff some element saw the
          // pre-element count equal to it, so this end callback would only report a second time.
          // Reading it here was also what put the destination's refusal ahead of the count bound
          // in this family and left `Fatal`'s public error depending on the builder (#277).
          |_, _, _| Ok(()),
        )
      })
      .map(|(_, collected)| collected)
  }
}

/// The **spanned owning** destination: the container and the construct's span together.
///
/// One of the two contracts this family did not implement until
/// [#259](https://github.com/al8n/tokora/issues/259)'s stage 3.
///
/// Spelled on `With<Collect<..>, PhantomSpan>`, uniformly with every other repetition driver.
/// [`Collect`]'s "Why the spanned destination wears a wrapper" is why it cannot be written onto
/// `Collect<..>` itself.
impl<
  'inp,
  L,
  P,
  O,
  Container,
  Ctx,
  Delim,
  Lang: ?Sized,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
> ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang, Cmpl>
  for With<
    Collect<
      DelimitedBy<AtMost<Repeated<P, O, L, Ctx, Lang, Cmpl>>, Delim>,
      Container,
      Ctx,
      Lang,
      Cmpl,
    >,
    PhantomSpan,
  >
where
  Delim: Delimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: Default + ContainerT<O> + DelimiterHandler<'inp, L>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let maximum = self.primary().parser.parser.maximum();

    self
      .primary_mut()
      .attempt(|c| {
        let Collect {
          parser, container, ..
        } = c;
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut()).parse_repeated(
          inp,
          container,
          &maximum,
          |_, _, _| Ok(()),
        )
      })
      .map(|(span, collected)| Spanned::new(span, collected))
  }
}

/// The **borrowed** destination: the caller keeps the storage, so it can read what the construct
/// admitted on the **failure** arm too, which the owning form cannot expose.
///
/// The second contract this family did not implement until
/// [#259](https://github.com/al8n/tokora/issues/259)'s stage 3.
impl<
  'inp,
  'c,
  L,
  P,
  O,
  Container,
  Ctx,
  Delim,
  Lang: ?Sized,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
> ParseInput<'inp, L, L::Span, Ctx, Lang, Cmpl>
  for Collect<
    &'c mut DelimitedBy<AtMost<Repeated<P, O, L, Ctx, Lang, Cmpl>>, Delim>,
    &'c mut Container,
    Ctx,
    Lang,
    Cmpl,
  >
where
  Delim: Delimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: ContainerT<O> + DelimiterHandler<'inp, L>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let maximum = self.parser.parser.maximum();

    DelimitedBy::<_, Delim>::new(self.parser.parser.parser_mut()).parse_repeated(
      inp,
      &mut self.container,
      &maximum,
      |_, _, _| Ok(()),
    )
  }
}
