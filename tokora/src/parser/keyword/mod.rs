use crate::{
  ParseInput, TryParseInput,
  error::{UnexpectedEot, token::UnexpectedToken},
  span::Span,
  token::KeywordToken,
  try_parse_input::ParseAttempt,
  types::Keyword,
  utils::cmp::Equivalent,
};

use super::*;

impl Keyword<(), ()> {
  /// A parser that parses a token and returns a `Keyword` instance if it matches.
  ///
  /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not a keyword,
  /// and promises no valid token is consumed.
  ///
  /// `Lang` is read off the input, so an unbranded grammar writes the same call as a branded one.
  pub fn try_parse<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    ParseAttempt<Keyword<L::Token, L::Span, Lang>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    inp
      .try_expect_or_stop(|t| t.into_data().is_keyword())
      .map(|opt_tok| {
        opt_tok
          .map(|tok| {
            let (span, t) = tok.into_components();
            Keyword::new(span, t)
          })
          .into()
      })
  }

  /// A parser that parses any keyword, erroring when the next token is not a
  /// keyword.
  ///
  /// Unlike [`try_parse`](Self::try_parse), a non-keyword token is
  /// converted into an [`UnexpectedToken`] error carrying the found token, and
  /// end of input into an [`UnexpectedEot`] error.
  pub fn parse<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<Keyword<L::Token, L::Span, Lang>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
  {
    match inp.next_or_stop()? {
      Some(spanned) => {
        if spanned.data().is_keyword() {
          let (span, t) = spanned.into_components();
          Ok(Keyword::new(span, t))
        } else {
          let (span, tok) = spanned.into_components();
          Err(UnexpectedToken::of(span).with_found(tok).into())
        }
      }
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }

  /// A parser that parses a token and returns a `Keyword` instance if it matches.
  ///
  /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not a keyword,
  /// and promises no valid token is consumed.
  pub fn try_parse_sliced<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    ParseAttempt<Keyword<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Lang>>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    inp
      .try_expect_or_stop(|t| t.into_data().is_keyword())
      .map(|opt_tok| {
        opt_tok
          .map(|tok| Keyword::new(tok.into_span(), inp.slice()))
          .into()
      })
  }

  /// A parser that parses any keyword and returns its source slice, erroring
  /// when the next token is not a keyword.
  ///
  /// Unlike [`try_parse_sliced`](Self::try_parse_sliced), a non-keyword
  /// token is converted into an [`UnexpectedToken`] error carrying the found
  /// token, and end of input into an [`UnexpectedEot`] error.
  pub fn parse_sliced<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    Keyword<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
  {
    match inp.next_or_stop()? {
      Some(spanned) => {
        if spanned.data().is_keyword() {
          Ok(Keyword::new(spanned.into_span(), inp.slice()))
        } else {
          let (span, tok) = spanned.into_components();
          Err(UnexpectedToken::of(span).with_found(tok).into())
        }
      }
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }

  /// A parser that parses a specific keyword and returns a `Keyword` instance if it matches.
  ///
  /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not the expected keyword,
  /// and promises no valid token is consumed.
  #[must_use]
  pub fn try_parse_exact<'inp, L, Ctx, Exp, Lang: ?Sized, Cmpl>(
    expected: &Exp,
  ) -> impl TryParseInput<'inp, L, Keyword<L::Token, L::Span, Lang>, Ctx, Lang, Cmpl>
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    str: Equivalent<Exp>,
  {
    move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
      inp
        .try_expect_or_stop(|t| {
          t.into_data()
            .keyword()
            .is_some_and(|k| k.equivalent(expected))
        })
        .map(|opt_tok| {
          opt_tok
            .map(|tok| {
              let (span, t) = tok.into_components();
              Keyword::new(span, t)
            })
            .into()
        })
    }
  }

  /// A parser that parses a specific keyword, erroring when the next token is
  /// not that keyword.
  ///
  /// Unlike [`try_parse_exact`](Self::try_parse_exact), an unexpected
  /// token is converted into an [`UnexpectedToken`] error carrying the found
  /// token, and end of input into an [`UnexpectedEot`] error.
  #[must_use]
  pub fn parse_exact<'inp, L, Ctx, Exp, Lang: ?Sized, Cmpl>(
    expected: &Exp,
  ) -> impl ParseInput<'inp, L, Keyword<L::Token, L::Span, Lang>, Ctx, Lang, Cmpl>
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
    str: Equivalent<Exp>,
  {
    move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| match inp.next_or_stop()? {
      Some(spanned) => {
        if spanned
          .data()
          .keyword()
          .is_some_and(|k| k.equivalent(expected))
        {
          let (span, t) = spanned.into_components();
          Ok(Keyword::new(span, t))
        } else {
          let (span, tok) = spanned.into_components();
          Err(UnexpectedToken::of(span).with_found(tok).into())
        }
      }
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }

  /// A parser that parses a specific keyword and returns a `Keyword` instance if it matches.
  ///
  /// If the function returns `Ok(ParseAttempt::Decline)`, it means the next token is not the expected keyword,
  /// and promises no valid token is consumed.
  #[must_use]
  pub fn try_parse_exact_sliced<'inp, L, Ctx, Exp, Lang: ?Sized, Cmpl>(
    expected: &Exp,
  ) -> impl TryParseInput<
    'inp,
    L,
    Keyword<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Lang>,
    Ctx,
    Lang,
    Cmpl,
  >
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    str: Equivalent<Exp>,
  {
    move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| {
      inp
        .try_expect_or_stop(|t| {
          t.into_data()
            .keyword()
            .is_some_and(|k| k.equivalent(expected))
        })
        .map(|opt_tok| {
          opt_tok
            .map(|tok| Keyword::new(tok.into_span(), inp.slice()))
            .into()
        })
    }
  }

  /// A parser that parses a specific keyword and returns its source slice,
  /// erroring when the next token is not that keyword.
  ///
  /// Unlike [`try_parse_exact_sliced`](Self::try_parse_exact_sliced), an
  /// unexpected token is converted into an [`UnexpectedToken`] error carrying
  /// the found token, and end of input into an [`UnexpectedEot`] error.
  #[must_use]
  pub fn parse_exact_sliced<'inp, L, Ctx, Exp, Lang: ?Sized, Cmpl>(
    expected: &Exp,
  ) -> impl ParseInput<
    'inp,
    L,
    Keyword<<L::Source as Source<L::Offset>>::Slice<'inp>, L::Span, Lang>,
    Ctx,
    Lang,
    Cmpl,
  >
  where
    L: Lexer<'inp>,
    L::Token: KeywordToken<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
      + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
    str: Equivalent<Exp>,
  {
    move |inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>| match inp.next_or_stop()? {
      Some(spanned) => {
        if spanned
          .data()
          .keyword()
          .is_some_and(|k| k.equivalent(expected))
        {
          Ok(Keyword::new(spanned.into_span(), inp.slice()))
        } else {
          let (span, tok) = spanned.into_components();
          Err(UnexpectedToken::of(span).with_found(tok).into())
        }
      }
      None => Err(UnexpectedEot::eot_of(inp.span().end()).into()),
    }
  }
}

#[cfg(all(
  test,
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
mod tests;
