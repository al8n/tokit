use crate::{container::Container as ContainerT, emitter::TooManyEmitter};

use super::*;

impl<'inp, L, P, O, Condition, Container, Ctx, Delim, W, Lang: ?Sized>
  ParseInput<'inp, L, Container, Ctx, Lang>
  for Collect<
    DelimitedBy<AtMost<RepeatedWhile<P, Condition, O, W, L, Ctx, Lang>>, Delim>,
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
    + UnclosedEmitter<'inp, L, Lang>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
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
    let maximum = self.parser.parser.maximum();

    self
      .attempt(|c| {
        let Collect {
          parser, container, ..
        } = c;
        // The maximum is settled at the element that broke it, from inside
        // `many::admit_element` — a construct exceeds `max` iff some element saw the pre-element
        // count equal to it — so `Maximum::on_stop` re-checks nothing. Reading it from an end
        // callback was what put the destination's refusal ahead of the count bound in this family
        // and left `Fatal`'s public error depending on the builder (#277).
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut())
          .parse_repeated(inp, container, &maximum)
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
impl<'inp, L, P, O, Condition, Container, Ctx, Delim, W, Lang: ?Sized>
  ParseInput<'inp, L, Spanned<Container, L::Span>, Ctx, Lang>
  for Collect<
    DelimitedBy<AtMost<RepeatedWhile<P, Condition, O, W, L, Ctx, Lang>>, Delim>,
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
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: Default + ContainerT<O> + DelimiterHandler<'inp, L>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<Spanned<Container, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
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
        DelimitedBy::<_, Delim>::new(parser.parser.parser_mut())
          .parse_repeated(inp, container, &maximum)
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
impl<'inp, 'c, L, P, O, Condition, Container, Ctx, Delim, W, Lang: ?Sized>
  ParseInput<'inp, L, L::Span, Ctx, Lang>
  for Collect<
    &'c mut DelimitedBy<AtMost<RepeatedWhile<P, Condition, O, W, L, Ctx, Lang>>, Delim>,
    &'c mut Container,
    Ctx,
    Lang,
  >
where
  Delim: Delimiter<'inp, L, Lang>,
  L: Lexer<'inp>,
  P: ParseInput<'inp, L, O, Ctx, Lang>,
  Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
  W: Window,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>
    + TooManyEmitter<'inp, L, Lang>
    + UnclosedEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: ContainerT<O> + DelimiterHandler<'inp, L>,
{
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
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
    )
  }
}
