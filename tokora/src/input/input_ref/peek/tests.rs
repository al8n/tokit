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
  let (mut emitter, cache) = <() as ParseContext<'_, TestLexer<'_>>>::provide(()).into_components();
  let mut input = Input::<TestLexer<'inp>, (), ()>::with_state_and_cache(src, (), cache);
  let mut inp_ref = input.as_ref(&mut emitter);
  f(&mut inp_ref)
}

#[test]
fn peek_one_returns_token() {
  parse_with("abc 123", |inp| {
    let peeked = inp.peek_one()?;
    assert!(peeked.is_some());
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_one_empty_input() {
  parse_with("", |inp| {
    let peeked = inp.peek_one()?;
    assert!(peeked.is_none());
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_window() {
  parse_with("abc 123 def", |inp| {
    use generic_arraydeque::typenum::U2;
    let peeked = inp.peek::<U2>()?;
    assert_eq!(peeked.len(), 2);
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_with_emitter_test() {
  parse_with("abc 123", |inp| {
    use generic_arraydeque::typenum::U2;
    let (peeked, _emitter) = inp.peek_with_emitter::<U2>()?;
    assert_eq!(peeked.len(), 2);
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_window_larger_than_input() {
  parse_with("abc", |inp| {
    use generic_arraydeque::typenum::U3;
    let peeked = inp.peek::<U3>()?;
    assert_eq!(peeked.len(), 1);
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_does_not_consume() {
  parse_with("abc 123", |inp| {
    use generic_arraydeque::typenum::U1;
    {
      let peeked = inp.peek::<U1>()?;
      assert_eq!(peeked.len(), 1);
    }
    {
      let peeked = inp.peek::<U1>()?;
      assert_eq!(peeked.len(), 1);
    }
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_window_exceeds_cache_capacity() {
  // U4 window on default U3 cache — triggers overflow path (lines 76-126)
  parse_with("abc 123 def ghi", |inp| {
    use generic_arraydeque::typenum::U4;
    let peeked = inp.peek::<U4>()?;
    // Should see all 4 tokens even though cache can only hold 3
    assert_eq!(peeked.len(), 4);
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_overflow_tokens_correct() {
  // Verify overflowed tokens have correct data
  parse_with("abc 123 def ghi jkl", |inp| {
    use generic_arraydeque::typenum::U4;
    {
      let peeked = inp.peek::<U4>()?;
      assert_eq!(peeked.len(), 4);
    }
    // Peek again — should get same result (tokens cached or re-lexed)
    {
      let peeked2 = inp.peek::<U4>()?;
      assert_eq!(peeked2.len(), 4);
    }
    Ok(())
  })
  .unwrap();
}

#[test]
fn peek_overflow_then_consume() {
  // Peek with overflow, then consume tokens normally
  parse_with("abc 123 def ghi", |inp| {
    use generic_arraydeque::typenum::U4;
    {
      let peeked = inp.peek::<U4>()?;
      assert_eq!(peeked.len(), 4);
    }
    // Consume should work correctly after overflow peek
    let tok = inp.next()?;
    assert!(tok.is_some());
    Ok(())
  })
  .unwrap();
}

#[test]
fn slice_after_peek_returns_consumed_token() {
  // Consume the first token so the target is no longer at offset 0, then peek
  // to fill the cache and consume from it. `slice()` must return the text of
  // the just-consumed token, not the whole consumed prefix.
  parse_with("foo bar", |inp| {
    assert!(inp.next()?.is_some());
    assert!(inp.peek_one()?.is_some());
    assert!(inp.next()?.is_some());
    assert_eq!(inp.slice(), "bar");
    Ok(())
  })
  .unwrap();
}

#[test]
fn cursor_targets_first_cached_token_start() {
  use generic_arraydeque::typenum::U2;
  // "a1" lexes to two adjacent tokens: Word(0..1), Num(1..2).
  parse_with("a1", |inp| {
    {
      let peeked = inp.peek::<U2>()?;
      assert_eq!(peeked.len(), 2);
    }
    // The cursor must point at the START of the first cached token (0),
    // not its end (1).
    assert_eq!(*inp.cursor().as_inner(), 0usize);
    Ok(())
  })
  .unwrap();
}

#[test]
fn save_restore_preserves_front_token_with_multi_cache() {
  use generic_arraydeque::typenum::U2;
  // Fill the cache with two tokens, checkpoint, consume one, then restore.
  // The next token must be the FIRST one again (no silent token loss).
  parse_with("a1", |inp| {
    {
      let peeked = inp.peek::<U2>()?;
      assert_eq!(peeked.len(), 2);
    }
    let ckp = inp.save();
    let first = inp.next()?.expect("first token");
    assert_eq!(first.data, Tok::Word);
    inp.restore(ckp);
    let again = inp.next()?.expect("token after restore");
    assert_eq!(again.data, Tok::Word);
    Ok(())
  })
  .unwrap();
}

#[test]
fn attempt_over_prefilled_cache_preserves_first_token() {
  use generic_arraydeque::typenum::U2;
  // A rollback attempt over a pre-filled cache must not skip a token.
  parse_with("a1", |inp| {
    {
      let peeked = inp.peek::<U2>()?;
      assert_eq!(peeked.len(), 2);
    }
    let outcome = inp.attempt(|inp| {
      // Consume the first token, then decline so the attempt rolls back.
      match inp.next() {
        Ok(Some(_)) => None::<()>,
        _ => None,
      }
    });
    assert!(outcome.is_none());
    let again = inp.next()?.expect("token after rolled-back attempt");
    assert_eq!(again.data, Tok::Word);
    Ok(())
  })
  .unwrap();
}

#[test]
fn spanned_since_under_peek_yields_real_span() {
  use crate::span::SimpleSpan;
  // Peek to fill the cache (as a peek_then_choice branch would), capture the
  // cursor, consume the peeked token, then measure the span from the captured
  // cursor. It must be the token's real span, not an empty span.
  parse_with("a1", |inp| {
    {
      let peeked = inp.peek_one()?;
      assert!(peeked.is_some());
    }
    let start = *inp.cursor();
    let _ = inp.next()?.expect("first token");
    let span = inp.span_since(&start);
    assert_eq!(span, SimpleSpan::new(0, 1));
    Ok(())
  })
  .unwrap();
}

#[test]
fn span_and_slice_report_consumed_token_after_multi_peek() {
  use crate::span::SimpleSpan;
  // With more than one token cached, consuming one must leave `span()`/`slice()`
  // reporting the JUST-CONSUMED token, not the remaining front cached token.
  parse_with("a1", |inp| {
    {
      let peeked = inp.peek::<generic_arraydeque::typenum::U2>()?;
      assert_eq!(peeked.len(), 2);
    }
    let first = inp.next()?.expect("first token");
    assert_eq!(first.data, Tok::Word);
    assert_eq!(*inp.span(), SimpleSpan::new(0, 1));
    assert_eq!(inp.slice(), "a");
    Ok(())
  })
  .unwrap();
}

#[test]
fn token_accessor_reads_ref_arm() {
  use crate::{cache::PeekedTokenExt, span::SimpleSpan};
  use generic_arraydeque::typenum::U2;
  // A U2 window fits the default U3 cache, so both peeked tokens are the
  // borrowed (`Ref`) arm. The accessor reaches token + span without matching.
  parse_with("abc 123", |inp| {
    let peeked = inp.peek::<U2>()?;
    assert_eq!(peeked.len(), 2);
    assert!(peeked[0].is_ref());
    assert!(peeked[1].is_ref());
    assert_eq!(*peeked[0].token(), Tok::Word);
    assert_eq!(*peeked[0].span(), SimpleSpan::new(0, 3));
    assert_eq!(*peeked[1].token(), Tok::Num);
    assert_eq!(*peeked[1].span(), SimpleSpan::new(4, 7));
    Ok(())
  })
  .unwrap();
}

#[test]
fn token_accessor_reads_owned_arm() {
  use crate::{cache::PeekedTokenExt, span::SimpleSpan};
  use generic_arraydeque::typenum::U4;
  // A U4 window exceeds the default U3 cache; the 4th token overflows and is
  // materialized as the owned (`Owned`) arm. The same accessor reaches it.
  parse_with("abc 123 def ghi", |inp| {
    let peeked = inp.peek::<U4>()?;
    assert_eq!(peeked.len(), 4);
    assert!(peeked[3].is_owned());
    assert_eq!(*peeked[3].token(), Tok::Word);
    assert_eq!(*peeked[3].span(), SimpleSpan::new(12, 15));
    Ok(())
  })
  .unwrap();
}

// ── Lexer errors must never be dropped, never double-emitted ──────────────
//
// A counting emitter records exactly how many lexer errors reach the
// emitter. It is non-fatal (always returns `Ok`) and does NOT deduplicate,
// so a double emission of the same malformed region is observable as `2`.

#[derive(Debug, Default)]
struct CountingEmitter {
  lexer_errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NeverFatal;

impl<'inp> crate::emitter::Emitter<'inp, TestLexer<'inp>> for CountingEmitter {
  type Error = NeverFatal;

  fn emit_lexer_error(
    &mut self,
    _: crate::span::Spanned<
      <<TestLexer<'inp> as crate::Lexer<'inp>>::Token as crate::Token<'inp>>::Error,
      <TestLexer<'inp> as crate::Lexer<'inp>>::Span,
    >,
  ) -> Result<(), NeverFatal> {
    self.lexer_errors += 1;
    Ok(())
  }

  fn emit_unexpected_token(
    &mut self,
    _: crate::error::token::UnexpectedTokenOf<'inp, TestLexer<'inp>, ()>,
  ) -> Result<(), NeverFatal> {
    Ok(())
  }

  fn emit_error(
    &mut self,
    _: crate::span::Spanned<NeverFatal, <TestLexer<'inp> as crate::Lexer<'inp>>::Span>,
  ) -> Result<(), NeverFatal> {
    Ok(())
  }

  fn rewind(&mut self, _: &crate::input::Cursor<'inp, '_, TestLexer<'inp>>, _: u64) {}
}

type CountingCtx<'inp> = (
  CountingEmitter,
  crate::cache::DefaultCache<'inp, TestLexer<'inp>>,
);

fn count_lexer_errors<'inp, F>(src: &'inp str, f: F) -> usize
where
  F: FnOnce(
    &mut crate::input::InputRef<'inp, '_, TestLexer<'inp>, CountingCtx<'inp>, ()>,
  ) -> Result<(), NeverFatal>,
{
  let mut emitter = CountingEmitter::default();
  let cache = crate::cache::DefaultCache::<'inp, TestLexer<'inp>>::default();
  let mut input =
    crate::input::Input::<TestLexer<'inp>, CountingCtx<'inp>, ()>::with_state_and_cache(
      src,
      (),
      cache,
    );
  {
    let mut inp = input.as_ref(&mut emitter);
    let _ = f(&mut inp);
  }
  emitter.lexer_errors
}

fn drain<'inp>(
  inp: &mut crate::input::InputRef<'inp, '_, TestLexer<'inp>, CountingCtx<'inp>, ()>,
) -> Result<(), NeverFatal> {
  while inp.next()?.is_some() {}
  Ok(())
}

#[test]
fn consume_direct_single_lexer_error() {
  // No peek at all: a lexer error is emitted once as it is consumed.
  let n = count_lexer_errors("a @ b", |inp| drain(inp));
  assert_eq!(n, 1, "consume-direct");
}

#[test]
fn peek_then_consume_single_lexer_error() {
  // Error precedes a cached token; peek seals it, consume must not re-emit.
  let n = count_lexer_errors("@ a b", |inp| {
    use generic_arraydeque::typenum::U2;
    {
      let _ = inp.peek::<U2>()?;
    }
    drain(inp)
  });
  assert_eq!(n, 1, "peek-then-consume");
}

#[test]
fn peek_trailing_then_consume_single_lexer_error() {
  // Error trails the cached token (no later cached token). Consume re-lexes it.
  let n = count_lexer_errors("a @", |inp| {
    use generic_arraydeque::typenum::U2;
    {
      let _ = inp.peek::<U2>()?;
    }
    drain(inp)
  });
  assert_eq!(n, 1, "peek-trailing-then-consume");
}

#[test]
fn peek_overflow_then_consume_single_lexer_error() {
  // Cache holds 3; window 5. Error sits in the overflow region.
  let n = count_lexer_errors("a b c @ d", |inp| {
    use generic_arraydeque::typenum::U5;
    {
      let _ = inp.peek::<U5>()?;
    }
    drain(inp)
  });
  assert_eq!(n, 1, "peek-overflow-then-consume");
}

#[test]
fn peek_overflow_stop_records_lexer_error() {
  // Peek over the overflow region then STOP without consuming: the error in
  // the overflow region must still have been recorded at peek time.
  let n = count_lexer_errors("a b c @ d", |inp| {
    use generic_arraydeque::typenum::U5;
    let _ = inp.peek::<U5>()?;
    Ok(())
  });
  assert_eq!(n, 1, "peek-overflow-stop must record the error");
}

// ── Fatal overflow peek must not leak staged tokens ───────────────────────────
//
// When a peek window exceeds the cache capacity, tokens past the cache are staged
// in an inline overflow buffer until the cache region is copied out. If a fatal
// lexer error is emitted while tokens are staged, the `?`-return must still drop
// every staged token (and its state) exactly once — never leak them. A
// drop-counting token payload makes any leak observable.

use core::sync::atomic::{AtomicUsize, Ordering};

static STAGED_DROPS: AtomicUsize = AtomicUsize::new(0);

/// A token payload that counts its own drops, so a leaked staged token is
/// observable as a missing drop.
#[derive(Debug, Clone)]
struct DropProbe;

impl Drop for DropProbe {
  fn drop(&mut self) {
    STAGED_DROPS.fetch_add(1, Ordering::SeqCst);
  }
}

#[derive(Debug, Clone, crate::logos::Logos)]
#[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
enum DropTok {
  #[regex(r"[0-9]+", |_| DropProbe)]
  Num(DropProbe),
}

impl core::fmt::Display for DropTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DropKind {
  Num,
}

impl core::fmt::Display for DropKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl crate::Token<'_> for DropTok {
  type Kind = DropKind;
  type Error = ();

  fn kind(&self) -> DropKind {
    DropKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type DropLexer<'a> = LogosLexer<'a, DropTok>;

/// An emitter that treats a lexer error as fatal (returns `Err`), so an invalid
/// lexeme in the overflow region triggers the early-return leak path.
struct FatalOnLexError;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Boom;

impl<'inp> crate::emitter::Emitter<'inp, DropLexer<'inp>> for FatalOnLexError {
  type Error = Boom;

  fn emit_lexer_error(
    &mut self,
    _: crate::span::Spanned<
      <<DropLexer<'inp> as crate::Lexer<'inp>>::Token as crate::Token<'inp>>::Error,
      <DropLexer<'inp> as crate::Lexer<'inp>>::Span,
    >,
  ) -> Result<(), Boom> {
    Err(Boom)
  }

  fn emit_unexpected_token(
    &mut self,
    _: crate::error::token::UnexpectedTokenOf<'inp, DropLexer<'inp>, ()>,
  ) -> Result<(), Boom> {
    Ok(())
  }

  fn emit_error(
    &mut self,
    _: crate::span::Spanned<Boom, <DropLexer<'inp> as crate::Lexer<'inp>>::Span>,
  ) -> Result<(), Boom> {
    Ok(())
  }

  fn rewind(&mut self, _: &crate::input::Cursor<'inp, '_, DropLexer<'inp>>, _: u64) {}
}

type DropCtx<'inp> = (
  FatalOnLexError,
  crate::cache::DefaultCache<'inp, DropLexer<'inp>>,
);

#[test]
fn fatal_overflow_peek_drops_staged_tokens_no_leak() {
  // U6 window over the default U3 cache: tokens 1..=3 fill the cache, 4 and 5
  // overflow into staging, then `@` is an invalid lexeme whose fatal emit
  // `?`-returns while 4 and 5 are still staged.
  use generic_arraydeque::typenum::U6;

  let baseline = STAGED_DROPS.load(Ordering::SeqCst);

  let cache = crate::cache::DefaultCache::<'_, DropLexer<'_>>::default();
  let mut emitter = FatalOnLexError;
  let mut input = crate::input::Input::<DropLexer<'_>, DropCtx<'_>, ()>::with_state_and_cache(
    "1 2 3 4 5 @",
    (),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let result = inp.peek::<U6>().map(|_| ());
  assert_eq!(result, Err(Boom), "the fatal lexer error must propagate");

  let staged_dropped = STAGED_DROPS.load(Ordering::SeqCst) - baseline;
  assert_eq!(
    staged_dropped, 2,
    "both staged overflow tokens must be dropped exactly once on the fatal return (no leak)"
  );
}

// ── A limit trip mid-overflow must truncate the peek at the durability boundary ──
//
// When a peek window exceeds the cache, tokens past the cache are staged in the
// inline overflow buffer. A staged token is durable only because a later
// `next()` re-lexes and regenerates it — but a limit trip mid-overflow latches
// the input, so `next()` will drain the cache-resident prefix and then stop,
// never re-lexing the staged tokens. Returning them would expose PHANTOM
// lookahead the caller can never consume. The peek must therefore truncate its
// result to the cache-resident prefix and drop the staged overflow tokens (freed
// exactly once by the `Overflow` guard — no double-drop with the `drain_into`
// hand-off, which is skipped on the trip).
//
// `LimitProbe` counts its own creations and drops so a leaked or double-dropped
// staged token is observable as `creates != drops`.

static LIMIT_CREATES: AtomicUsize = AtomicUsize::new(0);
static LIMIT_DROPS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct LimitProbe;

impl LimitProbe {
  fn new() -> Self {
    LIMIT_CREATES.fetch_add(1, Ordering::SeqCst);
    LimitProbe
  }
}

impl Clone for LimitProbe {
  fn clone(&self) -> Self {
    // Count a clone as a creation so `creates == drops` stays exact even if the
    // framework ever clones a token payload (it does not on this path).
    LIMIT_CREATES.fetch_add(1, Ordering::SeqCst);
    LimitProbe
  }
}

impl Drop for LimitProbe {
  fn drop(&mut self) {
    LIMIT_DROPS.fetch_add(1, Ordering::SeqCst);
  }
}

/// A limiter whose scan counter is shared across every cloned lexer, so the
/// `check()` trip point is deterministic regardless of `InputRef` rebuilding a
/// fresh lexer per operation.
#[derive(Debug, Clone, Default)]
struct TripLimiter {
  scanned: std::rc::Rc<core::cell::Cell<usize>>,
  limit: usize,
}

impl TripLimiter {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: std::rc::Rc::new(core::cell::Cell::new(0)),
      limit,
    }
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }
}

#[derive(Debug, Clone, PartialEq)]
struct TripLimitExceeded;

impl crate::state::State for TripLimiter {
  type Error = TripLimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.scanned.get() > self.limit {
      Err(TripLimitExceeded)
    } else {
      Ok(())
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
enum TripErr {
  Lex,
  Limit,
}

impl From<()> for TripErr {
  fn from(_: ()) -> Self {
    TripErr::Lex
  }
}

impl From<TripLimitExceeded> for TripErr {
  fn from(_: TripLimitExceeded) -> Self {
    TripErr::Limit
  }
}

#[derive(Debug, Clone, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = TripLimiter, skip r"[ \t\r\n]+")]
enum TripTok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); LimitProbe::new() })]
  Num(LimitProbe),
}

impl core::fmt::Display for TripTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TripKind {
  Num,
}

impl core::fmt::Display for TripKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl crate::Token<'_> for TripTok {
  type Kind = TripKind;
  type Error = TripErr;

  fn kind(&self) -> TripKind {
    TripKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type TripLexer<'a> = LogosLexer<'a, TripTok>;
type TripCtx<'a> = (
  crate::emitter::Silent<TripErr>,
  crate::cache::DefaultCache<'a, TripLexer<'a>>,
);

/// Drives one truncation scenario: a `W`-wide peek over the default U3 cache on
/// `src`, whose limiter (limit `n`) trips after `staged` overflow tokens have
/// been staged. Asserts (a) the peek window equals the 3 cache-resident tokens
/// (staged phantoms excluded); (b) exactly 3 survivors remain live after the
/// peek (every staged token + the trip token dropped); (c) `next()` drains
/// exactly those 3 then `None` (no rescan past the latch).
fn assert_trip_truncates<W>(src: &'static str, limit: usize, staged: usize)
where
  W: crate::Window,
{
  let live = || LIMIT_CREATES.load(Ordering::SeqCst) - LIMIT_DROPS.load(Ordering::SeqCst);

  let cache = crate::cache::DefaultCache::<'_, TripLexer<'_>>::default();
  let mut emitter = crate::emitter::Silent::<TripErr>::new();
  let mut input = crate::input::Input::<TripLexer<'_>, TripCtx<'_>, ()>::with_state_and_cache(
    src,
    TripLimiter::with_limit(limit),
    cache,
  );
  let mut inp = input.as_ref(&mut emitter);

  let alive_before = live();
  {
    // (a) The returned window is truncated to the cache-resident prefix — the
    // `staged` overflow phantoms are excluded.
    let peeked = inp.peek::<W>().unwrap();
    assert_eq!(
      peeked.len(),
      3,
      "peek window must equal the cache-resident count (3), excluding {staged} staged phantom(s)"
    );
  }

  // (c) Exactly 3 tokens survive the peek: the cache prefix. Every staged
  // overflow token AND the trip token have been dropped exactly once — a leak
  // would leave more alive, a double-drop fewer.
  assert_eq!(
    live() - alive_before,
    3,
    "only the 3 cache-resident tokens may survive the truncating peek"
  );

  // (b) `next()` drains exactly the 3 cache tokens, then the latch stops it.
  assert!(inp.next().unwrap().is_some(), "cache token 1");
  assert!(inp.next().unwrap().is_some(), "cache token 2");
  assert!(inp.next().unwrap().is_some(), "cache token 3");
  assert!(inp.next().unwrap().is_none(), "poisoned: no phantom 4");
  assert!(inp.next().unwrap().is_none(), "poisoned: stays None");
}

#[test]
fn overflow_peek_trip_truncates_phantom_tokens() {
  use generic_arraydeque::typenum::{U4, U5, U6};

  let creates_before = LIMIT_CREATES.load(Ordering::SeqCst);
  let drops_before = LIMIT_DROPS.load(Ordering::SeqCst);

  // Trip mid-overflow with SEVERAL staged: 1..=3 cached, 4 & 5 staged, 6 trips.
  assert_trip_truncates::<U6>("1 2 3 4 5 6", 5, 2);
  // Trip on the FIRST overflow token staged: 1..=3 cached, 4 staged, 5 trips.
  assert_trip_truncates::<U5>("1 2 3 4 5", 4, 1);
  // Trip with ZERO staged: 1..=3 fill the cache, the next scan (4) trips.
  assert_trip_truncates::<U4>("1 2 3 4", 3, 0);

  // Every token payload created across all scenarios was dropped exactly once:
  // no leak (drops < creates) and no double-drop (drops > creates).
  let creates = LIMIT_CREATES.load(Ordering::SeqCst) - creates_before;
  let drops = LIMIT_DROPS.load(Ordering::SeqCst) - drops_before;
  assert_eq!(
    creates, drops,
    "every staged/cached/trip token freed exactly once (no leak, no double-drop)"
  );
}

// The discrimination the decision-window combinators (`peek_then`, `peek_then_choice`, the
// `*_while` folds) build on: a peek window truncated by a terminal scanner stop is flagged
// `terminal`, while a genuine end of input and a full window are not. That flag is what lets those
// combinators surface a trip rather than read the short window as a decline.
#[test]
fn peek_with_emitter_terminal_flags_a_trip_but_not_eof() {
  use generic_arraydeque::typenum::U3;

  let peek3 = |src: &'static str, limit: usize| {
    let cache = crate::cache::DefaultCache::<'_, TripLexer<'_>>::default();
    let mut emitter = crate::emitter::Silent::<TripErr>::new();
    let mut input = crate::input::Input::<TripLexer<'_>, TripCtx<'_>, ()>::with_state_and_cache(
      src,
      TripLimiter::with_limit(limit),
      cache,
    );
    let mut inp = input.as_ref(&mut emitter);
    let (peeked, terminal, _e) = inp.peek_with_emitter_terminal::<U3>().unwrap();
    (peeked.len(), terminal)
  };

  // A fresh trip during the fill: a short window, flagged terminal (1 and 2 scan, 3 trips).
  assert_eq!(
    peek3("1 2 3 4 5", 2),
    (2, true),
    "a trip-truncated window is short and flagged terminal"
  );
  // A genuine end of input: a short window, but NOT terminal.
  assert_eq!(
    peek3("1 2", usize::MAX),
    (2, false),
    "a genuine end of input is short but not terminal"
  );
  // A full window under a roomy limit: not terminal.
  assert_eq!(
    peek3("1 2 3 4", usize::MAX),
    (3, false),
    "a full window is not terminal"
  );
}
