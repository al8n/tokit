use crate::{ParseContext, input::Input, lexer::LogosLexer};

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
enum Tok {
  #[regex(r"[a-z]+")]
  Word,
  #[regex(r"[0-9]+")]
  Num,
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Tok::Word => write!(f, "word"),
      Tok::Num => write!(f, "num"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokKind {
  Word,
  Num,
}

impl core::fmt::Display for TokKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokKind::Word => write!(f, "word"),
      TokKind::Num => write!(f, "num"),
    }
  }
}

impl crate::Token<'_> for Tok {
  type Kind = TokKind;
  type Error = ();

  const READ_FRONTIER_CLASS: crate::ReadFrontierClass = crate::ReadFrontierClass::Unbounded;
  fn kind(&self) -> TokKind {
    match self {
      Tok::Word => TokKind::Word,
      Tok::Num => TokKind::Num,
    }
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

type TestLexer<'a> = LogosLexer<'a, Tok>;

fn parse_with<'inp, F, O>(src: &'inp str, mut f: F) -> Result<O, ()>
where
  F: for<'c> FnMut(&mut crate::input::InputRef<'inp, 'c, TestLexer<'inp>, (), ()>) -> Result<O, ()>,
{
  let (emitter, cache, recursion) =
    <() as ParseContext<'_, TestLexer<'_>>>::provide(()).into_components();
  let mut input = Input::<TestLexer<'inp>, (), ()>::with_state_and_context(
    src,
    (),
    crate::input::InputContext::new(emitter, cache).with_recursion_limiter(recursion),
  );
  let mut inp_ref = input.as_ref();
  f(&mut inp_ref)
}

#[test]
fn consume_cached_one_after_peek() {
  parse_with("abc 123", |inp| {
    use generic_arraydeque::typenum::U2;
    let peeked = inp.peek::<U2>()?;
    drop(peeked);
    let tok = inp.consume_cached_one();
    assert!(tok.is_some());
    let tok = tok.unwrap();
    assert_eq!(tok.data, Tok::Word);
    Ok(())
  })
  .unwrap();
}

#[test]
fn consume_cached_one_empty_cache() {
  parse_with("abc", |inp| {
    let tok = inp.consume_cached_one();
    assert!(tok.is_none());
    Ok(())
  })
  .unwrap();
}

#[test]
fn consume_cached_to_predicate() {
  parse_with("abc 123 def", |inp| {
    use generic_arraydeque::typenum::U3;
    let peeked = inp.peek::<U3>()?;
    drop(peeked);
    let last = inp.consume_cached_to(|t| matches!(t.token().data(), Tok::Num));
    assert!(last.is_some());
    let last = last.unwrap();
    assert_eq!(last.data, Tok::Word);
    Ok(())
  })
  .unwrap();
}

#[test]
fn consume_cached_while_predicate() {
  parse_with("abc 123 def", |inp| {
    use generic_arraydeque::typenum::U3;
    let peeked = inp.peek::<U3>()?;
    drop(peeked);
    let last = inp.consume_cached_while(|t| matches!(t.token().data(), Tok::Word));
    assert!(last.is_some());
    let last = last.unwrap();
    assert_eq!(last.data, Tok::Word);
    Ok(())
  })
  .unwrap();
}

#[test]
fn consume_all_cached() {
  parse_with("abc 123 def", |inp| {
    use generic_arraydeque::typenum::U3;
    let peeked = inp.peek::<U3>()?;
    drop(peeked);
    let last = inp.consume_all_cached();
    assert!(last.is_some());
    let last = last.unwrap();
    assert_eq!(last.data, Tok::Word);
    Ok(())
  })
  .unwrap();
}

#[test]
fn consume_all_cached_empty() {
  parse_with("abc", |inp| {
    let last = inp.consume_all_cached();
    assert!(last.is_none());
    Ok(())
  })
  .unwrap();
}
