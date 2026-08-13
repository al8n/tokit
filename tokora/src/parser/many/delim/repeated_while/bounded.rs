use crate::{
  container::Container as ContainerT,
  emitter::{TooFewEmitter, TooManyEmitter},
  error::syntax::{TooFew, TooMany},
};

use super::*;

impl<'inp, L, P, O, Condition, Container, Ctx, Delim, W, Lang: ?Sized>
  ParseInput<'inp, L, Container, Ctx, Lang>
  for Collect<
    DelimitedBy<Bounded<RepeatedWhile<P, Condition, O, W, L, Ctx, Lang>>, Delim>,
    Container,
    Ctx,
    Lang,
  >
where
  Delim: Delimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  P: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  W: Window,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + TooFewEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
    From<UnexpectedEot<L::Offset, Lang>> + FromUnclosed<'inp, L, Lang>,
  Container: Default + ContainerT<O> + DelimiterHandler<'inp, L>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let max = self.parser.parser.maximum().get();
    let min = self.parser.parser.minimum().get();

    self
      .attempt(|c| {
        let Collect {
          parser, container, ..
        } = c;
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut()).parse_repeated(
          inp,
          container,
          |nums, inp, span| {
            if min > nums {
              inp
                .emitter()
                .emit_too_few(TooFew::of(span.clone(), nums, min))?;
            }

            if nums > max {
              inp
                .emitter()
                .emit_too_many(TooMany::of(span.clone(), max + 1, max))?;
            }
            Ok(())
          },
        )
      })
      .map(|(_, collected)| collected)
  }
}
