use crate::{
  Emitter, Lexer, ParseContext,
  emitter::{MissingLeadingSeparatorEmitter, SeparatedEmitter, TooFewEmitter},
  error::{syntax::MissingSyntaxOf, token::MissingTokenOf},
  input::{Cursor, InputRef},
  parser::{
    AllowTrailing, Minimum, RequireLeading,
    many::{ContinueStateHandler, EndStateHandler, SeparatorStateHandler},
  },
  punct::Punctuator,
  span::{Span, Spanned},
};

impl<'inp, 'closure, Sep, O, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>
  EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>
  for RequireLeading<AllowTrailing<Minimum>>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + TooFewEmitter<'inp, L, Lang>,
{
  #[inline(always)]
  fn handle_start_state(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.parser.check(inp, anchor, num_elems)
  }

  #[inline(always)]
  fn handle_element_state(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.parser.check(inp, anchor, num_elems)
  }

  #[inline(always)]
  fn handle_leading_state(
    &self,
    num_elems: usize,
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
      .emit_missing_element(MissingSyntaxOf::<'_, L, Lang>::of(spanned.span_ref().end()))
      .and_then(|_| self.parser.parser.check(inp, anchor, num_elems))
  }

  #[inline(always)]
  fn handle_separator_state(
    &self,
    num_elems: usize,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    _: Spanned<L::Token, L::Span>,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parser.parser.check(inp, anchor, num_elems)
  }
}

impl<'inp, 'closure, Sep, O, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>
  ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>
  for RequireLeading<AllowTrailing<Minimum>>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + MissingLeadingSeparatorEmitter<'inp, L, Lang>,
  Sep: Punctuator<'inp, L, Lang>,
{
  #[inline(always)]
  fn handle_start_state(
    &self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    off: L::Offset,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    inp
      .emitter()
      .emit_missing_leading_separator(Sep::name(), MissingTokenOf::<'_, L, Lang>::of(off))
  }
}

impl<'inp, 'closure, Sep, O, L, Ctx, Lang: ?Sized, Cmpl: crate::input::Completeness>
  SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>
  for RequireLeading<AllowTrailing<Minimum>>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
{
  #[inline(always)]
  fn handle_start_state(
    &self,
    _: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    _: &Spanned<L::Token, L::Span>,
  ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    Ok(())
  }
}
