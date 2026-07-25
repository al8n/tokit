use crate::{
  error::{UnexpectedEot, token::UnexpectedToken},
  span::Span,
  token::IdentifierToken,
  try_parse_input::ParseAttempt,
  types::Ident,
};

use super::*;

impl Ident<(), ()> {
  /// A parser that parses a token and returns an `Ident` instance if it matches.
  ///
  /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not an identifier,
  /// and promises no valid token is consumed.
  pub fn try_parse<'inp, L, Ctx, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, (), Cmpl>,
  ) -> Result<
    ParseAttempt<Ident<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span>>,
    <Ctx::Emitter as Emitter<'inp, L>>::Error,
  >
  where
    L: Lexer<'inp>,
    L::Token: IdentifierToken<'inp>,
    Ctx: ParseContext<'inp, L>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, ()>,
    <Ctx::Emitter as Emitter<'inp, L>>::Error: From<UnexpectedEot<L::Offset>>,
  {
    Self::try_parse_of(inp)
  }

  /// A parser that parses a token and returns an `Ident` instance if it matches for a specific language.
  ///
  /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not an identifier,
  /// and promises no valid token is consumed.
  pub fn try_parse_of<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    ParseAttempt<Ident<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Lang>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    L::Token: IdentifierToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    inp
      .try_expect_or_stop(|t| t.data.is_identifier())
      .map(|res| {
        res
          .map(|tok| Ident::new(tok.into_span(), inp.slice()))
          .into()
      })
  }

  /// A parser that parses an identifier, erroring when the next token is not an
  /// identifier.
  ///
  /// Unlike [`try_parse`](Self::try_parse), a non-identifier token is converted
  /// into an [`UnexpectedToken`] error carrying the found token, and end of
  /// input into an [`UnexpectedEot`] error.
  pub fn parse<'inp, L, Ctx, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, (), Cmpl>,
  ) -> Result<
    Ident<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span>,
    <Ctx::Emitter as Emitter<'inp, L>>::Error,
  >
  where
    L: Lexer<'inp>,
    L::Token: IdentifierToken<'inp>,
    Ctx: ParseContext<'inp, L>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, ()>,
    <Ctx::Emitter as Emitter<'inp, L>>::Error: From<UnexpectedEot<L::Offset>>
      + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span>>,
  {
    Self::parse_of(inp)
  }

  /// A parser that parses an identifier for a specific language, erroring when
  /// the next token is not an identifier.
  ///
  /// Unlike [`try_parse_of`](Self::try_parse_of), a non-identifier token is
  /// converted into an [`UnexpectedToken`] error carrying the found token, and
  /// end of input into an [`UnexpectedEot`] error.
  pub fn parse_of<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    Ident<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    L::Token: IdentifierToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
  {
    match inp.next_or_stop()? {
      Some(spanned) => {
        if spanned.data().is_identifier() {
          Ok(Ident::new(spanned.into_span(), inp.slice()))
        } else {
          let (span, tok) = spanned.into_components();
          Err(UnexpectedToken::of(span).with_found(tok).into())
        }
      }
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }
}

#[cfg(all(test, feature = "std", feature = "logos"))]
mod tests;
