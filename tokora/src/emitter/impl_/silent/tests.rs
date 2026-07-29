use super::*;
use crate::lexer::DummyLexer;
use crate::span::SimpleSpan;
use std::format;

#[test]
fn silent_new() {
  let _s: Silent<()> = Silent::new();
}

#[test]
fn silent_default() {
  let _s: Silent<()> = Silent::default();
}

#[test]
fn silent_debug() {
  let s: Silent<()> = Silent::new();
  assert_eq!(format!("{:?}", s), "Silent");
}

#[test]
fn silent_clone_and_copy() {
  let s: Silent<()> = Silent::new();
  let s2 = s.clone();
  let s3 = s;
  let _ = (s2, s3);
}

#[test]
fn silent_emit_lexer_error_returns_ok() {
  let mut s: Silent<()> = Silent::new();
  let spanned = Spanned::new(SimpleSpan::new(0usize, 5usize), ());
  let result = <Silent<()> as Emitter<'_, DummyLexer>>::emit_lexer_error(&mut s, spanned);
  assert!(result.is_ok());
}

#[test]
fn silent_emit_error_returns_ok() {
  let mut s: Silent<()> = Silent::new();
  let spanned = Spanned::new(SimpleSpan::new(0usize, 5usize), ());
  let result = <Silent<()> as Emitter<'_, DummyLexer>>::emit_error(&mut s, spanned);
  assert!(result.is_ok());
}

#[test]
fn silent_emit_unexpected_token_returns_ok() {
  use crate::error::token::UnexpectedToken;
  use crate::lexer::DummyToken;

  let mut s: Silent<()> = Silent::new();
  let ut: UnexpectedToken<'_, DummyToken, DummyToken, SimpleSpan> =
    UnexpectedToken::new(SimpleSpan::new(0usize, 1usize));
  let result = <Silent<()> as Emitter<'_, DummyLexer>>::emit_unexpected_token(&mut s, ut);
  assert!(result.is_ok());
}

#[test]
fn silent_with_lang_type() {
  struct MyLang;
  let _s: Silent<(), MyLang> = Silent::new();
  let _s2: Silent<(), MyLang> = Silent::default();
  assert_eq!(format!("{:?}", _s), "Silent");
}

/// **The flip, bound to the crate's own declaration rather than to a payload.**
///
/// `Silent`'s `PrattEmitter` impl used to demand four conversions on the error type that its
/// bodies never perform — `FromEmitterError`, `From<UnexpectedEoLhs>`, `From<UnexpectedEoRhs>`
/// and, through `FromEmitterError`, `From<UnexpectedToken>` and `From<()>`. `Opaque` implements
/// none of them, which is the whole point: it is the *representation-independent* error a
/// dropping emitter should be usable with, exactly as `Ignored`'s bound-free twin already
/// allowed.
///
/// Before the deletion this function does not compile — `E0277` x4, naming
/// `Opaque: From<UnexpectedEnd<PrattLhsHint>>`, `From<UnexpectedEnd<PrattRhsHint>>`,
/// `From<()>` and `From<UnexpectedToken<'_, Tok, Kind>>`. After it, it does.
///
/// Falsifying output: a compile error here means a body secretly used a conversion. It does
/// not — but this cell is what says so.
#[test]
fn silent_pratt_accepts_a_representation_independent_error() {
  /// An error type that implements *no* conversions at all.
  #[derive(Debug)]
  struct Opaque;

  const fn takes_pratt<E>()
  where
    E: crate::emitter::PrattEmitter<'static, DummyLexer>,
  {
  }

  // The positive control differs in exactly one token — the error type — and compiled both
  // before and after: `()` satisfies every conversion the deleted bounds demanded, which is
  // why the defect was invisible to the existing suite.
  takes_pratt::<Silent<()>>();
  takes_pratt::<Silent<Opaque>>();
}
