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
