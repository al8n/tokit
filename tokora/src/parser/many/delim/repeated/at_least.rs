use crate::{container::Container as ContainerT, emitter::TooFewEmitter, error::syntax::TooFew};

use super::*;

impl<
  'inp,
  L,
  P,
  O,
  Container,
  Delim,
  Ctx,
  Lang: ?Sized,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
> ParseInput<'inp, L, Container, Ctx, Lang, Cmpl>
  for Collect<
    DelimitedBy<AtLeast<Repeated<P, O, L, Ctx, Lang, Cmpl>>, Delim>,
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
    + TooFewEmitter<'inp, L, Lang>
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
    let minimum = self.parser.parser.minimum();
    let min = minimum.get();

    self
      .attempt(|c| {
        let Collect {
          parser, container, ..
        } = c;
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut()).parse_repeated(
          inp,
          container,
          // A minimum is an end-of-construct fact, so this hook is a no-op and the check below
          // carries it. The handler is threaded anyway: `many::admit_element` takes the
          // cardinality this collection actually has, not a stand-in for it.
          &minimum,
          |nums, inp, span| {
            if min > nums {
              inp
                .emitter()
                .emit_too_few(TooFew::of(span.clone(), nums, min))?;
            }

            Ok(())
          },
        )
      })
      .map(|(_, collected)| collected)
  }
}
