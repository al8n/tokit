use crate::{
  Accumulator, Emitter, InputRef, Lexer, ParseContext, ParseInput, Source, TryParseInput,
  emitter::{
    FullContainerEmitter, SeparatedEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  error::UnexpectedEot,
  parser::SeparatorHandler,
  punct::Punctuator,
  span::Spanned,
  token::IdentifierToken,
  try_parse_input::Accept,
  types::{Ident, IdentList},
};

/// Returns a parser for a list of identifiers separated by the given separator.
///
/// The parser will not consume any valid token if it is not a valid ident list.
///
/// `Lang` is read off the input, so an unbranded grammar writes the same call as a branded one.
#[must_use]
pub fn try_ident_list<'inp, Sep, L, Container, Ctx, Lang, Cmpl>() -> impl TryParseInput<
  'inp,
  L,
  IdentList<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Container, Lang>,
  Ctx,
  Lang,
  Cmpl,
> + 'inp
where
  L: Lexer<'inp>,
  L::Source: Source<L::Offset>,
  L::Token: IdentifierToken<'inp>,
  Sep: Punctuator<'inp, L, Lang>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
  Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
    + FullContainerEmitter<'inp, L, Lang>
    + UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>
    + UnexpectedTrailingSeparatorEmitter<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  Container: Default
    + crate::container::Container<Ident<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Lang>>
    + SeparatorHandler<'inp, L>
    + 'inp,
{
  move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
    Ident::try_parse
      .separated::<Sep>()
      .collect()
      .spanned()
      .parse_input(inp)
      .map(|seg: Spanned<Container, _>| {
        let (span, container) = seg.into_components();
        Accept(IdentList::new(span, container))
      })
  }
}
