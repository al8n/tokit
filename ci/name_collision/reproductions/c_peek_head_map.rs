
pub trait ConsumerExt {
  fn peek_head_map<O, F>(&mut self, f: F) -> Result<Option<O>, PErr>
  where F: FnOnce(tokora::span::Spanned<&Tok, &tokora::SimpleSpan>) -> O;
}
impl<'inp, 'b> ConsumerExt for InputRef<'inp, 'b, PLexer<'inp>, PCtx<'inp>> {
  fn peek_head_map<O, F>(&mut self, _f: F) -> Result<Option<O>, PErr>
  where F: FnOnce(tokora::span::Spanned<&Tok, &tokora::SimpleSpan>) -> O { ran(); Ok(None) }
}
fn drive() {
  fn body<'inp>(inp: &mut InputRef<'inp, '_, PLexer<'inp>, PCtx<'inp>>) -> Result<(), PErr> {
    let _v: Option<u8> = inp.peek_head_map(|_sp| 1u8)?;
    Ok(())
  }
  let _ = Parser::with_context(ctx()).apply(body).parse_str("1 2");
}
