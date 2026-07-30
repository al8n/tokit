
// The consumer who wrote the helper FIRST, with the natural signature — same name, same
// arity, same return type. And the return is USED, which the CHANGELOG says is loud.
pub trait ConsumerExt {
  fn head_satisfies<F>(&mut self, pred: F) -> Result<bool, PErr>
  where
    F: FnOnce(&Tok) -> bool;
}

impl<'inp, 'b> ConsumerExt for InputRef<'inp, 'b, PLexer<'inp>, PCtx<'inp>> {
  fn head_satisfies<F>(&mut self, _pred: F) -> Result<bool, PErr>
  where
    F: FnOnce(&Tok) -> bool,
  {
    ran();
    Ok(false)
  }
}

fn drive() {
  fn body<'inp>(inp: &mut InputRef<'inp, '_, PLexer<'inp>, PCtx<'inp>>) -> Result<(), PErr> {
    let _hit: bool = inp.head_satisfies(|t| matches!(t, Tok::Word))?;
    Ok(())
  }
  let _ = Parser::with_context(ctx()).apply(body).parse_str("1 2");
}
