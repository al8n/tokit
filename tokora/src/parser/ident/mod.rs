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
  ///
  /// `Lang` is read off the input, so an unbranded grammar writes the same call as a branded one.
  pub fn try_parse<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
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
  ///
  /// `Lang` is read off the input, so an unbranded grammar writes the same call as a branded one.
  pub fn parse<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
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

  /// An identifier whose spelling is **not** in `excluded` — the "Name but not `on`" /
  /// enum-value-literal production, as one atom.
  ///
  /// Declines with zero consumption when the head is not an identifier **or** is an
  /// excluded spelling. Both tests ride the same classify predicate, so there is a single
  /// classification and the decline is genuinely non-consuming.
  ///
  /// The spelling is read from the source by the token's own span rather than through
  /// [`InputRef::slice`], and the reason is the non-consumption above: the exclusion must
  /// run **inside** the predicate, where the input is mutably borrowed, so `inp.slice()`
  /// is not reachable — and `slice()` reports the *committed* span anyway, which for a
  /// token that has not been consumed is the wrong one. [`InputRef::source`] is `&'inp`
  /// and reaches in.
  pub fn try_parse_except<'inp, L, Ctx, Exp, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    excluded: &[Exp],
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
    <L::Source as Source<L::Offset>>::Slice<'inp>: crate::utils::cmp::Equivalent<Exp>,
  {
    use crate::utils::cmp::Equivalent as _;
    let source = inp.source();
    inp
      .try_expect_or_stop(|t| {
        t.data.is_identifier()
          && source
            .slice(t.span.start()..t.span.end())
            .is_some_and(|s| !excluded.iter().any(|x| s.equivalent(x)))
      })
      .map(|res| {
        res
          .map(|tok| Ident::new(tok.into_span(), inp.slice()))
          .into()
      })
  }

  /// The committed twin of [`try_parse_except`](Self::try_parse_except): a non-identifier
  /// **or** an excluded spelling consumes the token and errors with it as the found token.
  pub fn parse_except<'inp, L, Ctx, Exp, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    excluded: &[Exp],
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
    <L::Source as Source<L::Offset>>::Slice<'inp>: crate::utils::cmp::Equivalent<Exp>,
  {
    use crate::utils::cmp::Equivalent as _;
    match inp.next_or_stop()? {
      Some(spanned) => {
        let admissible = spanned.data().is_identifier()
          && inp
            .source()
            .slice(spanned.span_ref().start()..spanned.span_ref().end())
            .is_some_and(|s| !excluded.iter().any(|x| s.equivalent(x)));
        if admissible {
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
