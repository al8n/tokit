use crate::{
  Emitter, Lexer, ParseContext,
  emitter::{MissingTrailingSeparatorEmitter, SeparatedEmitter, UnexpectedLeadingSeparatorEmitter},
  error::{
    syntax::MissingSyntaxOf,
    token::{MissingTokenOf, UnexpectedTokenOf},
  },
  input::{Cursor, InputRef},
  parser::{
    RequireTrailing,
    many::{ContinueStateHandler, EndStateHandler, SeparatorStateHandler, Unbounded},
  },
  punct::Punctuator,
  span::{Span, Spanned},
};

impl<'inp, 'closure, Sep, O, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>
  EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl> for RequireTrailing<Unbounded>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
    + MissingTrailingSeparatorEmitter<'inp, L, Lang>
    + UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>,
  Sep: Punctuator<'inp, L, Lang>,
{
  #[inline(always)]
  fn handle_start_state(
    &self,
    _: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Ok(inp.span_since(anchor))
  }

  #[inline(always)]
  fn handle_element_state(
    &self,
    _: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let span = inp.span_since(anchor);
    inp
      .emitter()
      .emit_missing_trailing_separator(Sep::name(), MissingTokenOf::<'_, L, Lang>::of(span.end()))
      .map(|_| span)
  }

  #[inline(always)]
  fn handle_leading_state(
    &self,
    _: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    spanned: Spanned<L::Token, L::Span>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    inp
      .emitter()
      .emit_missing_element(MissingSyntaxOf::<'_, L, Lang>::of(
        spanned.span_ref().start(),
      ))
      .map(|_| inp.span_since(anchor))
  }

  #[inline(always)]
  fn handle_separator_state(
    &self,
    _: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    _: Spanned<L::Token, L::Span>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Ok(inp.span_since(anchor))
  }
}

impl<'inp, 'closure, Sep, O, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>
  ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl> for RequireTrailing<Unbounded>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + MissingTrailingSeparatorEmitter<'inp, L, Lang>,
{
  #[inline(always)]
  fn handle_start_state(
    &self,
    _: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    _: L::Offset,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Ok(())
  }
}

impl<'inp, 'closure, Sep, O, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>
  SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl> for RequireTrailing<Unbounded>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
    + MissingTrailingSeparatorEmitter<'inp, L, Lang>
    + UnexpectedLeadingSeparatorEmitter<'inp, L, Lang>,
  Sep: Punctuator<'inp, L, Lang>,
{
  #[inline(always)]
  fn handle_start_state(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    sep_tok: &Spanned<L::Token, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    let (span, tok) = sep_tok.clone().into_components();
    inp.emitter().emit_unexpected_leading_separator(
      Sep::name(),
      UnexpectedTokenOf::<'_, L, Lang>::of(span).with_found(tok),
    )
  }
}
