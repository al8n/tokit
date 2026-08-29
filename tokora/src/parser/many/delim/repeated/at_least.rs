use crate::{container::Container as ContainerT, emitter::TooFewEmitter};

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

    self
      .attempt(|c| {
        let Collect {
          parser, container, ..
        } = c;
        // One handler, for both halves of the cardinality: `many::admit_element` runs its
        // element hook (a no-op for a minimum, which no element in the middle of a construct can
        // settle) and the driver's end pass runs its `on_stop`, which emits the `TooFew` over the
        // whole delimited span. The hand-written closure this replaced emitted the identical
        // diagnostic from the identical span.
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut())
          .parse_repeated(inp, container, &minimum)
      })
      .map(|(_, collected)| collected)
  }
}

/// The **spanned owning** destination: the container and the construct's span together.
///
/// One of the two contracts this family did not implement until
/// [#259](https://github.com/al8n/tokora/issues/259)'s stage 3. Nothing about being delimited
/// stopped it — `sep/delim` and `sep_while/delim` are delimited and implement all four — and the
/// reason it was missing was that this driver was spelled as a loop of its own rather than as the
/// plain one under a delimiter. Driving the shared loop is what made the contracts follow.
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
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>
    + TooFewEmitter<'inp, L, Lang>
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
    let minimum = self.parser.parser.minimum();
    self
      .attempt(|c| {
        let Collect {
          parser, container, ..
        } = c;
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut())
          .parse_repeated(inp, container, &minimum)
      })
      .map(|(span, collected)| Spanned::new(span, collected))
  }
}

/// The **borrowed** destination: the caller keeps the storage, so it can read what the construct
/// admitted on the **failure** arm too, which the owning form cannot expose.
///
/// The second contract this family did not implement until
/// [#259](https://github.com/al8n/tokora/issues/259)'s stage 3, and the one
/// `tokora/tests/repetition_behavioural_matrix.rs`'s layer B8 is about — with it, the matrix's
/// borrowed table covers all eight drivers instead of six.
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
    &'c mut DelimitedBy<AtLeast<Repeated<P, O, L, Ctx, Lang, Cmpl>>, Delim>,
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
    + TooFewEmitter<'inp, L, Lang>
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
    let minimum = self.parser.parser.minimum();
    DelimitedBy::<_, Delim>::new(self.parser.parser.parser_mut()).parse_repeated(
      inp,
      &mut self.container,
      &minimum,
    )
  }
}
