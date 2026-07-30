
pub trait ConsumerExt {
  fn peek_kind(&mut self) -> Result<Option<Kind>, PErr>;
}
impl<'inp, 'b> ConsumerExt for InputRef<'inp, 'b, PLexer<'inp>, PCtx<'inp>> {
  fn peek_kind(&mut self) -> Result<Option<Kind>, PErr> { ran(); Ok(None) }
}
fn drive() {
  fn body<'inp>(inp: &mut InputRef<'inp, '_, PLexer<'inp>, PCtx<'inp>>) -> Result<(), PErr> {
    let _k: Option<Kind> = inp.peek_kind()?;
    Ok(())
  }
  let _ = Parser::with_context(ctx()).apply(body).parse_str("1 2");
}
