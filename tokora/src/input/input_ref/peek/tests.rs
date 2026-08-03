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
  let cache = crate::cache::DefaultCache::<'inp, TestLexer<'inp>>::default();
  let mut input =
    crate::input::Input::<TestLexer<'inp>, CountingCtx<'inp>, ()>::with_state_and_context(
      src,
      (),
      crate::input::InputContext::new(CountingEmitter::default(), cache),
    );
  {
    let mut inp = input.as_ref();
    let _ = f(&mut inp);
  }
  input.emitter().lexer_errors
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
// at the tail of the output buffer until the cache region is copied in ahead of
// them. If a fatal lexer error is emitted while tokens are staged, the `?`-return
// must still drop every staged token (and its state) exactly once — never leak them
// — and hand the buffer back holding nothing it did not arrive with. A
// drop-counting token payload makes any leak observable.

/// The liveness ledger the drop-safety cells below count into.
///
/// One per *scenario*, never a global. Rust runs tests in parallel by default, so a
/// counter shared between cells makes every delta assertion depend on whatever else
/// happens to be running: with these numbers held in global atomics, the ordinary
/// `cargo test overflow_peek` invocation failed 184 runs out of 200, and a flaky
/// leak test is the one people eventually delete. Each cell instead builds its own
/// ledger, hands it to the lexer as state, and reads back only what its own probes
/// wrote.
///
/// `Cell` rather than an atomic on purpose: a ledger never leaves the thread of the
/// test that made it, and choosing the non-`Sync` type is what keeps that true by
/// construction rather than by convention.
#[derive(Debug, Default)]
struct Ledger {
  creates: core::cell::Cell<usize>,
  drops: core::cell::Cell<usize>,
}

impl Ledger {
  /// Payloads created against this ledger and not yet freed.
  fn live(&self) -> usize {
    self.creates.get() - self.drops.get()
  }

  fn created(&self) -> usize {
    self.creates.get()
  }

  fn dropped(&self) -> usize {
    self.drops.get()
  }

  fn record_create(&self) {
    self.creates.set(self.creates.get() + 1);
  }

  fn record_drop(&self) {
    self.drops.set(self.drops.get() + 1);
  }
}

/// A token payload that counts its own drops into its scenario's ledger, so a
/// leaked staged token is observable as a missing drop.
#[derive(Debug, Clone)]
struct DropProbe {
  ledger: std::rc::Rc<Ledger>,
}

impl Drop for DropProbe {
  fn drop(&mut self) {
    self.ledger.record_drop();
  }
}

/// The `DropTok` lexer state: nothing but the scenario's ledger, which is how the
/// payload callback reaches it without a global.
#[derive(Debug, Clone, Default)]
struct DropLedger(std::rc::Rc<Ledger>);

impl DropLedger {
  fn ledger(&self) -> std::rc::Rc<Ledger> {
    std::rc::Rc::clone(&self.0)
  }

  fn probe(&self) -> DropProbe {
    DropProbe {
      ledger: self.ledger(),
    }
  }
}

impl crate::state::State for DropLedger {
  type Error = ();

  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

#[derive(Debug, Clone, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = DropLedger, skip r"[ \t\r\n]+")]
enum DropTok {
  // Never *read*: the payload is observed through its destructor, which is the whole
  // of what this cell measures. (While it was a ZST the dead-code lint exempted it
  // as a positional field; carrying a ledger handle makes it a real value.)
  #[regex(r"[0-9]+", |lex| lex.extras.probe())]
  Num(#[allow(dead_code)] DropProbe),
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

  // Each phase gets its own ledger, threaded in as the lexer state: phase 1's input
  // is still holding its three cache-resident tokens while phase 2 runs, and no
  // concurrently scheduled cell can reach either counter.
  let state = DropLedger::default();
  let ledger = state.ledger();

  let cache = crate::cache::DefaultCache::<'_, DropLexer<'_>>::default();
  let mut input = crate::input::Input::<DropLexer<'_>, DropCtx<'_>, ()>::with_state_and_context(
    "1 2 3 4 5 @",
    state,
    crate::input::InputContext::new(FatalOnLexError, cache),
  );
  let mut inp = input.as_ref();

  let result = inp.peek::<U6>().map(|_| ());
  assert_eq!(result, Err(Boom), "the fatal lexer error must propagate");

  assert_eq!(
    ledger.dropped(),
    2,
    "both staged overflow tokens must be dropped exactly once on the fatal return (no leak)"
  );

  // The same scan again, taken on the fill directly so the buffer it was handed is
  // still readable afterwards. Staging now happens *in* that buffer, so the
  // `?`-return has to unstage before propagating: a caller that reuses its buffer
  // must not find two tokens in it that the peek never promised.
  let state = DropLedger::default();
  let phase2 = state.ledger();

  let cache = crate::cache::DefaultCache::<'_, DropLexer<'_>>::default();
  let mut input = crate::input::Input::<DropLexer<'_>, DropCtx<'_>, ()>::with_state_and_context(
    "1 2 3 4 5 @",
    state,
    crate::input::InputContext::new(FatalOnLexError, cache),
  );
  let mut inp = input.as_ref();

  let mut buf = crate::cache::Peeked::<'_, '_, DropLexer<'_>, U6>::new();
  let outcome = inp
    .peek_with_emitter_inner::<U6>(&mut buf, &mut false)
    .map(|_| ());
  assert_eq!(outcome, Err(Boom), "the fatal lexer error must propagate");
  assert!(
    buf.is_empty(),
    "a failing fill must hand the caller's buffer back exactly as it received it, not \
     holding the tokens it staged in it"
  );
  assert_eq!(
    phase2.dropped(),
    2,
    "unstaging on the fatal return frees each staged token exactly once"
  );
}

// ── A limit trip mid-overflow must truncate the peek at the durability boundary ──
//
// When a peek window exceeds the cache, tokens past the cache are staged at the
// tail of the output buffer. A staged token is durable only because a later
// `next()` re-lexes and regenerates it — but a limit trip mid-overflow latches
// the input, so `next()` will drain the cache-resident prefix and then stop,
// never re-lexing the staged tokens. Returning them would expose PHANTOM
// lookahead the caller can never consume. The peek must therefore truncate its
// result to the cache-resident prefix, dropping the staged region before the
// cache region is copied in — the deque owns those entries, so they are freed
// exactly once and cannot be handed out as well.
//
// `LimitProbe` counts its own creations and drops into its scenario's `Ledger`, so a
// leaked or double-dropped staged token is observable as `creates != drops`.

#[derive(Debug)]
struct LimitProbe {
  ledger: std::rc::Rc<Ledger>,
}

impl LimitProbe {
  fn new(ledger: std::rc::Rc<Ledger>) -> Self {
    ledger.record_create();
    LimitProbe { ledger }
  }
}

impl Clone for LimitProbe {
  fn clone(&self) -> Self {
    // Count a clone as a creation so `creates == drops` stays exact even if the
    // framework ever clones a token payload (it does not on this path).
    Self::new(std::rc::Rc::clone(&self.ledger))
  }
}

impl Drop for LimitProbe {
  fn drop(&mut self) {
    self.ledger.record_drop();
  }
}

/// A limiter whose scan counter and liveness ledger are shared across every cloned
/// lexer, so the `check()` trip point is deterministic regardless of `InputRef`
/// rebuilding a fresh lexer per operation, and so every payload a scenario lexes
/// lands in that scenario's own ledger.
#[derive(Debug, Clone, Default)]
struct TripLimiter {
  scanned: std::rc::Rc<core::cell::Cell<usize>>,
  ledger: std::rc::Rc<Ledger>,
  limit: usize,
}

impl TripLimiter {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: std::rc::Rc::new(core::cell::Cell::new(0)),
      ledger: std::rc::Rc::new(Ledger::default()),
      limit,
    }
  }

  /// A handle on this limiter's ledger, for the cell that built it to read back.
  fn ledger(&self) -> std::rc::Rc<Ledger> {
    std::rc::Rc::clone(&self.ledger)
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }

  fn probe(&self) -> LimitProbe {
    LimitProbe::new(self.ledger())
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
  // As with `DropTok`: observed through its destructor, never read.
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); lex.extras.probe() })]
  Num(#[allow(dead_code)] LimitProbe),
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
/// exactly those 3 then `None` (no rescan past the latch); (d) once the input is
/// gone, every payload the scenario lexed was freed exactly once.
///
/// The limiter carries this scenario's own ledger, so the liveness numbers below
/// count nothing but the tokens this call produced.
fn assert_trip_truncates<W>(src: &'static str, limit: usize, staged: usize)
where
  W: crate::Window,
{
  let limiter = TripLimiter::with_limit(limit);
  let ledger = limiter.ledger();

  let cache = crate::cache::DefaultCache::<'_, TripLexer<'_>>::default();
  let mut input = crate::input::Input::<TripLexer<'_>, TripCtx<'_>, ()>::with_state_and_context(
    src,
    limiter,
    crate::input::InputContext::new(crate::emitter::Silent::<TripErr>::new(), cache),
  );
  let mut inp = input.as_ref();

  assert_eq!(ledger.live(), 0, "the scenario starts with nothing lexed");
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
    ledger.live(),
    3,
    "only the 3 cache-resident tokens may survive the truncating peek"
  );

  // (b) `next()` drains exactly the 3 cache tokens, then the latch stops it.
  assert!(inp.next().unwrap().is_some(), "cache token 1");
  assert!(inp.next().unwrap().is_some(), "cache token 2");
  assert!(inp.next().unwrap().is_some(), "cache token 3");
  assert!(inp.next().unwrap().is_none(), "poisoned: no phantom 4");
  assert!(inp.next().unwrap().is_none(), "poisoned: stays None");

  drop(inp);
  drop(input);

  // (d) Every token payload this scenario created was dropped exactly once:
  // no leak (drops < creates) and no double-drop (drops > creates).
  assert_eq!(
    ledger.created(),
    ledger.dropped(),
    "every staged/cached/trip token freed exactly once (no leak, no double-drop)"
  );
}

#[test]
fn overflow_peek_trip_truncates_phantom_tokens() {
  use generic_arraydeque::typenum::{U4, U5, U6};

  // Trip mid-overflow with SEVERAL staged: 1..=3 cached, 4 & 5 staged, 6 trips.
  assert_trip_truncates::<U6>("1 2 3 4 5 6", 5, 2);
  // Trip on the FIRST overflow token staged: 1..=3 cached, 4 staged, 5 trips.
  assert_trip_truncates::<U5>("1 2 3 4 5", 4, 1);
  // Trip with ZERO staged: 1..=3 fill the cache, the next scan (4) trips.
  assert_trip_truncates::<U4>("1 2 3 4", 3, 0);
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
    let mut input = crate::input::Input::<TripLexer<'_>, TripCtx<'_>, ()>::with_state_and_context(
      src,
      TripLimiter::with_limit(limit),
      crate::input::InputContext::new(crate::emitter::Silent::<TripErr>::new(), cache),
    );
    let mut inp = input.as_ref();
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

// ── Stream order across the cache/overflow boundary ───────────────────────────
//
// A peek wider than the cache produces its two regions in the OPPOSITE order from
// the one the window must report: the tokens past the cache are lexed (and staged)
// first, while the cache region is copied out afterwards in one bulk read. The fill
// stages the overflow region at the tail of the output buffer and rotates the
// front-of-stream region ahead of it; these cells pin the resulting order. They are
// new coverage, not a pin of prior behaviour: before this change nothing asserted
// the identity of a peeked token past the cache boundary — the overflow tests
// checked only `len()` — so removing the rotation leaves every pre-existing peek
// test green and turns these red.

/// The spans a `W`-wide peek reports, in window order.
fn peeked_spans<W>(src: &str) -> std::vec::Vec<crate::span::SimpleSpan>
where
  W: crate::Window,
{
  use crate::cache::PeekedTokenExt as _;
  parse_with(src, |inp| {
    let peeked = inp.peek::<W>()?;
    Ok(peeked.iter().map(|t| *t.span()).collect())
  })
  .unwrap()
}

/// Whether each slot of a `W`-wide peek came back owned (staged past the cache)
/// rather than borrowed from the cache, in window order.
fn peeked_arms<W>(src: &str) -> std::vec::Vec<bool>
where
  W: crate::Window,
{
  parse_with(src, |inp| {
    let peeked = inp.peek::<W>()?;
    Ok(peeked.iter().map(|t| t.is_owned()).collect())
  })
  .unwrap()
}

#[test]
fn peek_reports_stream_order_across_the_cache_boundary() {
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U6;

  // Six one-character tokens at known offsets. The default cache holds three, so
  // the window is `[cache: a b c][staged: d e f]` — and the staged half was lexed
  // before the cached half was read out. A window that handed the staged region
  // back first would report 6, 8, 10, 0, 2, 4.
  assert_eq!(
    peeked_spans::<U6>("a b c d e f"),
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
      SimpleSpan::new(8, 9),
      SimpleSpan::new(10, 11),
    ],
    "the window must read in stream order across the cache/overflow boundary"
  );
  // And the split lands exactly at the cache capacity: three borrowed, three owned.
  assert_eq!(
    peeked_arms::<U6>("a b c d e f"),
    std::vec![false, false, false, true, true, true],
    "the borrowed cache region heads the window and the owned staged region tails it"
  );
}

#[test]
fn peek_reports_stream_order_with_a_single_staged_token() {
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U4;

  // The smallest overflow: one staged token behind a full cache. A rotation off by
  // one in either direction is visible here and nowhere else.
  assert_eq!(
    peeked_spans::<U4>("a b c d"),
    std::vec![
      SimpleSpan::new(0, 1),
      SimpleSpan::new(2, 3),
      SimpleSpan::new(4, 5),
      SimpleSpan::new(6, 7),
    ]
  );
  assert_eq!(
    peeked_arms::<U4>("a b c d"),
    std::vec![false, false, false, true]
  );
}

#[test]
fn peek_reports_stream_order_with_a_prefilled_cache() {
  use crate::cache::PeekedTokenExt as _;
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::{U2, U5};

  // A narrow peek first, so the cache is already partly full when the wide peek
  // runs: the wide fill then appends to a non-empty cache before it starts staging,
  // which moves the boundary the rotation has to respect.
  let spans = parse_with("a b c d e", |inp| {
    {
      let warm = inp.peek::<U2>()?;
      assert_eq!(warm.len(), 2, "the warm-up peek seeds the cache");
    }
    let peeked = inp.peek::<U5>()?;
    Ok(
      peeked
        .iter()
        .map(|t| (*t.span(), t.is_owned()))
        .collect::<std::vec::Vec<_>>(),
    )
  })
  .unwrap();

  assert_eq!(
    spans,
    std::vec![
      (SimpleSpan::new(0, 1), false),
      (SimpleSpan::new(2, 3), false),
      (SimpleSpan::new(4, 5), false),
      (SimpleSpan::new(6, 7), true),
      (SimpleSpan::new(8, 9), true),
    ]
  );
}

// ── The same order, over a cache that retains nothing ─────────────────────────
//
// `()` is the zero-capacity cache: every push refuses, so the whole window is
// staged and the parked slot — statically dead under a retaining cache — is live.
// It is the only built-in configuration where the front of the stream is NOT a
// cache entry, so it is the only one that exercises `[staged…][parked]` rotating
// to `[parked][staged…]`.

type UnitCacheCtx = (crate::emitter::Silent<()>, ());

fn with_unit_cache<'inp, F, O>(src: &'inp str, f: F) -> O
where
  F: FnOnce(&mut crate::input::InputRef<'inp, '_, TestLexer<'inp>, UnitCacheCtx, ()>) -> O,
{
  let mut input = crate::input::Input::<TestLexer<'inp>, UnitCacheCtx, ()>::with_state_and_context(
    src,
    (),
    crate::input::InputContext::new(crate::emitter::Silent::<()>::new(), ()),
  );
  let mut inp = input.as_ref();
  f(&mut inp)
}

#[test]
fn peek_over_a_non_retaining_cache_reports_stream_order() {
  use crate::cache::PeekedTokenExt as _;
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  // No cache at all: all three window slots are staged. The rotation degenerates to
  // a full-length rotate, which must be the identity.
  let got = with_unit_cache("a b c", |inp| {
    let peeked = inp.peek::<U3>().unwrap();
    peeked
      .iter()
      .map(|t| (*t.span(), t.is_owned()))
      .collect::<std::vec::Vec<_>>()
  });
  assert_eq!(
    got,
    std::vec![
      (SimpleSpan::new(0, 1), true),
      (SimpleSpan::new(2, 3), true),
      (SimpleSpan::new(4, 5), true),
    ],
    "a fully staged window is already in stream order and must not be permuted"
  );
}

#[test]
fn peek_over_a_parked_front_reports_the_parked_token_first() {
  use crate::cache::PeekedTokenExt as _;
  use crate::span::SimpleSpan;
  use generic_arraydeque::typenum::U3;

  // `sync_to` stops before the first number and, with nothing able to retain it,
  // parks it at the front of the stream. The peek that follows must head its window
  // with that parked token and stage `2` and `3` behind it — the window is
  // `[parked][staged][staged]`, assembled as `[staged][staged][parked]`.
  let got = with_unit_cache("a b 1 2 3", |inp| {
    use crate::Token as _;
    // One statement, so the window this returns — which borrows `inp` — dies before
    // the peek below reborrows it.
    assert!(
      inp
        .sync_to(|t| t.data.kind() == TokKind::Num, || None)
        .unwrap()
        .is_some(),
      "sync_to must stop on the first number"
    );
    let peeked = inp.peek::<U3>().unwrap();
    peeked
      .iter()
      .map(|t| (*t.span(), t.is_owned()))
      .collect::<std::vec::Vec<_>>()
  });
  assert_eq!(
    got,
    std::vec![
      (SimpleSpan::new(4, 5), false),
      (SimpleSpan::new(6, 7), true),
      (SimpleSpan::new(8, 9), true),
    ],
    "the parked token is the front of the stream and must head the window"
  );
}

// ── CACHE_COPY — a `Cache` that misreports `len()` ───────────────────────────
//
// The fill sizes the window's cache region from `Cache::len()` BEFORE it stages
// anything: `want = room - parked - len()`. Staging then eats the tail of the buffer,
// so by the time `Cache::peek` runs, the room left is exactly what `len()` claimed. On
// the documented contract that is exactly enough. On a cache whose `len` is not its
// resident count it is not, and the two directions fail differently:
//
//   * UNDER-report — `Cache::peek` is clipped mid-run. The window loses the residents
//     the clip cut off but keeps the staged tokens that belong *after* them, so what
//     comes back is not a short window, it is a HOLE: the caller reads a token at a
//     position where a later `next()` serves a different one. THIS ONE IS CREATED BY
//     THE REORDERING, and closing it is why `assert_cache_copy` exists.
//   * OVER-report — the fill reserves slots for residents that are not there and lexes
//     too few tokens to fill them. The window is a correct prefix but SHORT, which a
//     caller is entitled to read as end of input while the input still has more. Not
//     new — that is what an over-reporting cache did before the reordering too — but
//     the count check sees it for free.
//
// The hole is the one no count can see. With residents `[a,b,c]`, `len()` reporting 2
// and a `U4` window, the fill stages `d` and `e`, `Cache::peek` appends `[a,b]` into the
// two slots left, and `copied == in_cache` holds exactly: 2 == 2. The count the fill
// tracks and the count it observes agree *because* the under-report is subtracted from
// one side and added back on the other. The window came back `[a,b,d,e]` for a stream
// that reads `[a,b,c,d]`.
//
// What catches it is the resident run's own endpoints: a copy that is the whole run
// starts at the cache's `front` and ends at its `back`. BOTH halves stay on in EVERY
// build — the count and the endpoints alike — so the cells below are ungated and the
// hole is refused in release, which is the only build where a downstream `Cache` runs
// at speed. The endpoint witness was `debug_assert!` for one release; what changed is
// the measurement, not the reasoning (see CACHE_COPY in `peek/mod.rs`).
//
// A release build is where these cells earn their keep, and it is also where a
// `should_panic` can rot silently: a cell whose panic compiles out passes by never
// running the code it names. `cargo test --release --all-features` runs them, and the
// count cell below is the control — it was already ungated and already release-active,
// so a red there is the harness rather than this change.

/// The three directions the one lie can take. A `bool` covered the first two; the third —
/// under-reporting all the way to **zero** — is not "under by one" with a bigger constant,
/// it is the case where the *magnitude* of the lie determines whether the witness can see
/// it at all, so it gets its own mode rather than a parameter on an existing one.
///
/// Chosen over a `bool` plus a second `bool`, over an enum, and over three separate cache
/// types: const generics on this crate's MSRV take integers but not enums, two `bool`s make
/// an unrepresentable fourth combination, and three types would triple a 90-line impl to
/// vary one `match` arm.
mod lie {
  /// `len()` reports one **below** the resident count. The plausible off-by-one, and the
  /// one that HOLES the window: the copy is clipped mid-run and the staged tokens behind
  /// it close the gap.
  pub(super) const UNDER_BY_ONE: u8 = 0;
  /// `len()` reports one **above**. Shortens the window instead of holing it — the fill
  /// reserves slots for residents that are not there and lexes too few tokens for them.
  pub(super) const OVER_BY_ONE: u8 = 1;
  /// `len()` reports **zero**, whatever is resident. The largest under-report there is,
  /// and the blindest: it satisfies a count check as `0 == 0` and would skip any witness
  /// whose precondition was written over the reported length.
  pub(super) const UNDER_TO_ZERO: u8 = 2;
}

/// A capacity-3 ring whose `len()` misreports the resident count in one of the three
/// [`lie`] directions, and whose every other operation, `peek` included, works on the real
/// queue.
///
/// An empty cache still reports 0 under every mode, so a lie only shows once the cache is
/// warm: the warm-up peek in each cell below is honest, and the peek under test is the one
/// that reads a misreported length.
///
/// This is the plausible defect rather than a contrived one: an off-by-one — or a
/// forgotten increment, which is the zero case — in a hand-rolled cache's length
/// bookkeeping, in the one method the input layer sizes its window from. The crate's own
/// conformance kit models the same class of lie (`conformance/cache_tests.rs`).
struct MisreportingCache<'a, L, const LIE: u8>
where
  L: crate::Lexer<'a>,
{
  items: std::collections::VecDeque<crate::cache::CachedTokenOf<'a, L>>,
}

impl<'a, L, Lang: ?Sized, const LIE: u8> crate::cache::Cache<'a, L, Lang>
  for MisreportingCache<'a, L, LIE>
where
  L: crate::Lexer<'a>,
{
  type Options = ();

  // Retaining, so the parked slot is statically dead and these cells are about the cache
  // copy alone.
  const RETAINS_FRONT: bool = true;

  fn new() -> Self {
    Self {
      items: std::collections::VecDeque::with_capacity(3),
    }
  }

  fn with_options((): ()) -> Self {
    <Self as crate::cache::Cache<'a, L, Lang>>::new()
  }

  /// THE LIE, and the only one.
  fn len(&self) -> usize {
    match self.items.len() {
      0 => 0,
      _ if LIE == lie::UNDER_TO_ZERO => 0,
      n if LIE == lie::UNDER_BY_ONE => n - 1,
      n => n + 1,
    }
  }

  fn remaining(&self) -> usize {
    3 - self.items.len()
  }

  fn push_front(
    &mut self,
    tok: crate::cache::CachedTokenOf<'a, L>,
  ) -> Result<crate::cache::CachedTokenRefOf<'_, 'a, L>, crate::cache::CachedTokenOf<'a, L>> {
    if self.items.len() == 3 {
      return Err(tok);
    }
    self.items.push_front(tok);
    Ok(self.items.front().expect("just pushed").as_ref())
  }

  fn push_back(
    &mut self,
    tok: crate::cache::CachedTokenOf<'a, L>,
  ) -> Result<crate::cache::CachedTokenRefOf<'_, 'a, L>, crate::cache::CachedTokenOf<'a, L>> {
    if self.items.len() == 3 {
      return Err(tok);
    }
    self.items.push_back(tok);
    Ok(self.items.back().expect("just pushed").as_ref())
  }

  fn pop_front(&mut self) -> Option<crate::cache::CachedTokenOf<'a, L>> {
    self.items.pop_front()
  }

  fn pop_back(&mut self) -> Option<crate::cache::CachedTokenOf<'a, L>> {
    self.items.pop_back()
  }

  fn clear(&mut self) {
    self.items.clear();
  }

  fn peek<'p, W>(
    &'p self,
    buf: &mut generic_arraydeque::GenericArrayDeque<
      crate::cache::MaybeRefCachedTokenOf<'p, 'a, L>,
      W::CAPACITY,
    >,
  ) where
    W: crate::Window,
  {
    // The real queue, oldest first, bounded by the room the buffer has left — the honest
    // implementation of `peek`, which is what leaves the `len()` lie as the whole of the
    // defect.
    let fill = buf.remaining_capacity().min(self.items.len());
    for tok in self.items.iter().take(fill) {
      buf.push_back(mayber::Maybe::Ref(tok.as_ref()));
    }
  }

  fn front(&self) -> Option<crate::cache::CachedTokenRefOf<'_, 'a, L>> {
    self.items.front().map(crate::cache::CachedToken::as_ref)
  }

  fn back(&self) -> Option<crate::cache::CachedTokenRefOf<'_, 'a, L>> {
    self.items.back().map(crate::cache::CachedToken::as_ref)
  }
}

type MisreportingCtx<'a, const LIE: u8> = (
  crate::emitter::Silent<()>,
  MisreportingCache<'a, TestLexer<'a>, LIE>,
);

fn with_misreporting_cache<'inp, const LIE: u8, F, O>(src: &'inp str, f: F) -> O
where
  F: FnOnce(
    &mut crate::input::InputRef<'inp, '_, TestLexer<'inp>, MisreportingCtx<'inp, LIE>, ()>,
  ) -> O,
{
  let cache =
    <MisreportingCache<'_, TestLexer<'_>, LIE> as crate::cache::Cache<'_, TestLexer<'_>, ()>>::new(
    );
  let mut input =
    crate::input::Input::<TestLexer<'inp>, MisreportingCtx<'inp, LIE>, ()>::with_state_and_context(
      src,
      (),
      crate::input::InputContext::new(crate::emitter::Silent::<()>::new(), cache),
    );
  let mut inp = input.as_ref();
  f(&mut inp)
}

/// Release-active: the endpoint witness runs on the rotation path in every build, so this
/// cell is ungated. The window it refuses is a HOLE, and a hole is exactly what a release
/// build must not hand back.
#[test]
#[should_panic(expected = "did not copy the cache's whole resident run")]
fn peek_over_an_under_reporting_cache_fails_fast_instead_of_holing_the_window() {
  use generic_arraydeque::typenum::{U3, U4};

  with_misreporting_cache::<{ lie::UNDER_BY_ONE }, _, _>("a b c d e", |inp| {
    // Warm-up. The cache is empty, so `len()` tells the truth and the fill's own
    // successful pushes carry the count: a, b and c go in and the window is right. This
    // peek must NOT trip the check — the cell would prove nothing if the fixture failed
    // before the call under test.
    {
      let warm = inp.peek::<U3>().expect("the warm-up peek must succeed");
      assert_eq!(warm.len(), 3, "the warm-up peek seeds the cache");
    }
    // The call under test. `len()` now says 2 against 3 residents, so the fill reserves
    // two slots for the cache and stages `d` and `e` into the other two; `Cache::peek` is
    // then clipped after `[a, b]` and `c` falls out of the window. Without CACHE_COPY this
    // returns `Ok` with the starts 0, 2, 6, 8 where the stream reads 0, 2, 4, 6 — and the
    // count check cannot see it: the under-report is subtracted from `in_cache` and added
    // straight back as `staged`, so `copied == in_cache` holds on both sides of the lie.
    let _ = inp.peek::<U4>();
  });
}

#[test]
#[should_panic(expected = "and `Cache::peek` appended 3")]
fn peek_over_an_over_reporting_cache_fails_fast_instead_of_shortening_the_window() {
  use generic_arraydeque::typenum::{U3, U5};

  with_misreporting_cache::<{ lie::OVER_BY_ONE }, _, _>("a b c d e", |inp| {
    {
      let warm = inp.peek::<U3>().expect("the warm-up peek must succeed");
      assert_eq!(warm.len(), 3, "the warm-up peek seeds the cache");
    }
    // The other direction, and the half of CACHE_COPY a count CAN see — in every build.
    // `len()` now says 4 against 3 residents, so the fill reserves four slots and lexes
    // only one token past them; `Cache::peek` has room for four and appends three.
    // Unchecked this returns a correct but four-long window from a five-slot request — a
    // short window, which the peek contract lets a caller read as end of input.
    let _ = inp.peek::<U5>();
  });
}

/// Release-active for the same reason as the cell above, and this is the one that most
/// needs to be: a count check reads `0 == 0` here and sees nothing at all, so before the
/// promotion a release build had NO witness on the largest under-report there is.
#[test]
#[should_panic(expected = "did not copy the cache's whole resident run")]
fn peek_over_a_cache_under_reporting_to_zero_fails_fast_instead_of_holing_the_window() {
  use generic_arraydeque::typenum::{U2, U3};

  with_misreporting_cache::<{ lie::UNDER_TO_ZERO }, _, _>("a b c d e", |inp| {
    // Warm-up, honest: the cache is empty so 0 IS the resident count, and the fill's own
    // three successful pushes carry `in_cache` up to 3. `a`, `b` and `c` go in.
    {
      let warm = inp.peek::<U3>().expect("the warm-up peek must succeed");
      assert_eq!(warm.len(), 3, "the warm-up peek seeds the cache");
    }
    // The call under test. `len()` now says 0 against three residents, so the fill
    // reserves NOTHING for the cache region and spends the whole two-slot window staging:
    // `d` and `e` are lexed, refused by the full cache, and staged. `Cache::peek` then runs
    // with zero room and appends zero entries.
    //
    // Every count in sight agrees: `copied == in_cache` is `0 == 0`. This is the case that
    // decides the SHAPE of the endpoint witness — gate it on the reported length, as an
    // earlier revision did, and the largest lie there is walks straight past it. It is
    // gated on `staged`, which the fill counted, so it runs here.
    //
    // Unchecked this returns `Ok` with the starts 6, 8 — the window is `[d, e]` while the
    // next consume serves `a` at 0, then `b`, `c`, `d`, `e`. Not a short window: three
    // tokens of HOLE.
    let _ = inp.peek::<U2>();
  });
}

/// The cache-hit exit runs no CACHE_COPY check, and this cell is where that decision is
/// written down.
///
/// The reordering this fix makes is confined to the fill path: the hit exit appends
/// nothing behind its cache copy, so a copy the window's own capacity clipped is a correct
/// short prefix there and cannot hole. Its one weakness — an over-reporting `len()`
/// shortening the window, because `want` saturates to zero before the copy is sized — is
/// **pre-existing and unchanged**, and checking it would put the check's cost on the
/// width-1 head read a grammar runs per token. Both shapes below therefore return `Ok`,
/// as they did before this change; the cell exists so that a future decision to check
/// them is a deliberate one.
#[test]
fn peek_hit_exit_is_unchecked_and_unchanged_by_the_reordering() {
  use crate::cache::PeekedTokenExt as _;
  use generic_arraydeque::typenum::{U3, U4};

  let starts = |peeked: &crate::cache::Peeked<'_, '_, TestLexer<'_>, U3>| {
    peeked
      .iter()
      .map(|t| t.span().start())
      .collect::<std::vec::Vec<_>>()
  };

  // (1) The LEGITIMATE clip: a `U3` request against a four-slot claim, so
  // `min(len(), room)` is `min(4, 3)` and the three residents fill the window exactly. A
  // copy that stops before `back` because the window ran out is correct.
  let clipped = with_misreporting_cache::<{ lie::OVER_BY_ONE }, _, _>("a b c d e", |inp| {
    {
      let warm = inp.peek::<U3>().expect("the warm-up peek must succeed");
      assert_eq!(warm.len(), 3, "the warm-up peek seeds the cache");
    }
    let peeked = inp
      .peek::<U3>()
      .expect("a copy the window's own capacity clipped is not a contract violation");
    starts(&peeked)
  });
  assert_eq!(
    clipped,
    std::vec![0, 2, 4],
    "the clipped copy must still be the window's correct prefix"
  );

  // (2) The over-report SHORTENING the window, unchecked here by design. `len()` says 4
  // against three residents, so `want` saturates to zero at `4 - 4` and the fill returns
  // through the hit exit without lexing at all; `Cache::peek` has room for four and
  // appends the three that exist. A three-long window comes back from a four-slot request
  // over an input that still holds `d` and `e`.
  let shortened = with_misreporting_cache::<{ lie::OVER_BY_ONE }, _, _>("a b c d e", |inp| {
    {
      let warm = inp.peek::<U3>().expect("the warm-up peek must succeed");
      assert_eq!(warm.len(), 3, "the warm-up peek seeds the cache");
    }
    let peeked = inp
      .peek::<U4>()
      .expect("the hit exit is not CACHE_COPY-checked");
    peeked
      .iter()
      .map(|t| t.span().start())
      .collect::<std::vec::Vec<_>>()
  });
  assert_eq!(
    shortened,
    std::vec![0, 2, 4],
    "unchanged pre-existing behaviour: the hit exit hands back what the copy brought"
  );
}

// ── A successful overflow peek frees each staged token exactly once ────────────
//
// The trip and fatal-emit cells above prove the discarding exits are clean. This is
// the *keeping* exit: the staged tokens are handed to the caller inside the window,
// permuted into place, and freed when the window drops. A permutation that copied
// an entry instead of moving it would show up here as `drops > creates`; one that
// dropped an entry on the floor as `drops < creates`.

#[test]
fn successful_overflow_peek_frees_every_staged_token_exactly_once() {
  use crate::cache::PeekedTokenExt as _;
  use generic_arraydeque::typenum::U6;

  // A limit no scan can reach: this cell is about the keeping exit, not the trip.
  // Its ledger is this cell's own, so the counts below see only its own tokens.
  let limiter = TripLimiter::with_limit(usize::MAX);
  let ledger = limiter.ledger();

  let cache = crate::cache::DefaultCache::<'_, TripLexer<'_>>::default();
  let mut input = crate::input::Input::<TripLexer<'_>, TripCtx<'_>, ()>::with_state_and_context(
    "1 2 3 4 5 6",
    limiter,
    crate::input::InputContext::new(crate::emitter::Silent::<TripErr>::new(), cache),
  );
  let mut inp = input.as_ref();

  assert_eq!(ledger.live(), 0, "the cell starts with nothing lexed");
  {
    let peeked = inp.peek::<U6>().unwrap();
    assert_eq!(peeked.len(), 6, "the full window must come back");
    // Stream order, which for this fixture is also the span order.
    let starts = peeked
      .iter()
      .map(|t| t.span().start())
      .collect::<std::vec::Vec<_>>();
    assert_eq!(starts, std::vec![0, 2, 4, 6, 8, 10]);
    // All six the fill produced are live at once: three in the cache, three owned by
    // the window itself.
    assert_eq!(
      ledger.live(),
      6,
      "three cache-resident and three staged tokens are live while the window is held"
    );
  }
  // Dropping the window frees exactly the staged three; the cache keeps its three.
  assert_eq!(
    ledger.live(),
    3,
    "dropping the window frees the staged tokens once — no leak, no double-drop"
  );

  // The cache still holds its three and hands them back.
  assert!(inp.next().unwrap().is_some(), "cache token 1");
  assert!(inp.next().unwrap().is_some(), "cache token 2");
  assert!(inp.next().unwrap().is_some(), "cache token 3");
  drop(inp);
  drop(input);

  assert_eq!(
    ledger.created(),
    ledger.dropped(),
    "every token payload the peek created was freed exactly once"
  );
}

// ── PEEK_FOOTPRINT — one owned window, whatever the token and state cost ──────
//
// `Token`, `State` and `Span` are unconstrained in size, so every `W`-slot store a
// peek builds costs `W::CAPACITY × (Token + State + Span)` of stack. It must build
// exactly one — the window it hands back. Through 0.7.3 the fill built a second one
// on a cache miss (a `W::CAPACITY`-slot `MaybeUninit` array staging the tokens lexed
// past the cache until the cache region had been copied out), so a large token
// payload or a large lexer state cost twice what the caller asked for. On the widest
// window the crate offers, over the fixture below, that second store was 66,304 bytes
// of stack nobody asked for.
//
// The law is pinned from two sides, because neither side alone can see the whole of
// it:
//
//   * the compile-time assertions below fix the *size* relation over a deliberately
//     oversized token and lexer state — the window is one array of entries and each
//     entry carries one token, one state and one span, so the frame is exactly linear
//     in all three;
//   * `peek_footprint_census_the_fill_owns_no_store` in `census_tests.rs` fixes the
//     *structural* one — the fill body constructs no array, no deque and no
//     `MaybeUninit` of its own. That is the live half: a size assertion cannot see a
//     new local, and reintroducing a staging array trips the census immediately.
//
// A stack measurement was tried in place of the census and rejected. An unoptimized
// build materializes roughly seven window-sized copies around the fill (every
// `Result`, tuple and return slot on the way through `peek` gets one), so the term
// under test is under a seventh of the frame and any threshold over it is a magic
// number that a compiler release or an unrelated refactor moves.

/// Words per oversized payload — 1 KiB at 64 bits, so the window term dominates.
const BIG_WORDS: usize = 128;

/// An oversized token payload: the `Token` half of the per-entry cost.
#[derive(Debug, Clone)]
struct BigPayload([u64; BIG_WORDS]);

impl BigPayload {
  /// Reads the payload back, so its bytes are load-bearing rather than dead weight.
  fn probe(&self) -> u64 {
    self.0[0]
  }
}

/// An oversized lexer state: the `State` half of the per-entry cost.
#[derive(Debug, Clone)]
struct BigState([u64; BIG_WORDS]);

impl BigState {
  /// Reads the state back, for the same reason as [`BigPayload::probe`].
  fn probe(&self) -> u64 {
    self.0[0]
  }
}

impl Default for BigState {
  fn default() -> Self {
    Self([0; BIG_WORDS])
  }
}

impl crate::state::State for BigState {
  type Error = ();

  fn check(&self) -> Result<(), Self::Error> {
    Ok(())
  }
}

#[derive(Debug, Clone, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = BigState, skip r"[ \t\r\n]+")]
enum BigTok {
  #[regex(r"[0-9]+", |_| BigPayload([0; BIG_WORDS]))]
  Num(BigPayload),
}

impl core::fmt::Display for BigTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BigKind {
  Num,
}

impl core::fmt::Display for BigKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "number")
  }
}

impl crate::Token<'_> for BigTok {
  type Kind = BigKind;
  type Error = ();

  fn kind(&self) -> BigKind {
    BigKind::Num
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type BigLexer<'a> = LogosLexer<'a, BigTok>;
type BigCtx<'a> = (
  crate::emitter::Silent<()>,
  crate::cache::DefaultCache<'a, BigLexer<'a>>,
);

/// One window entry: the `Maybe` of a borrowed and an owned `CachedToken`.
type BigEntry<'r, 'a> = crate::cache::MaybeRefCachedTokenOf<'r, 'a, BigLexer<'a>>;
/// The widest window the crate offers, over the oversized fixture.
type BigWindow<'r, 'a> =
  crate::cache::Peeked<'r, 'a, BigLexer<'a>, generic_arraydeque::typenum::U32>;
/// The span this fixture's lexer carries, per entry.
type BigSpan = <BigLexer<'static> as crate::Lexer<'static>>::Span;

const BIG_ENTRY: usize = core::mem::size_of::<BigEntry<'static, 'static>>();
const BIG_WINDOW: usize = core::mem::size_of::<BigWindow<'static, 'static>>();

// The size law, stated relationally so it survives a change of word size or of
// `CachedToken`'s layout.
const _: () = {
  // An entry owns one whole token, one whole lexer state and one whole span: that is
  // why the peek footprint is linear in all three, and why a second store of this
  // shape doubles it.
  assert!(
    BIG_ENTRY
      >= core::mem::size_of::<BigTok>()
        + core::mem::size_of::<BigState>()
        + core::mem::size_of::<BigSpan>(),
    "a window entry must be able to own a token together with its lexer state and span"
  );
  // And the window is exactly 32 of them plus the deque's own indices — one array,
  // with no room for a shadow copy hiding inside it.
  assert!(
    32 * BIG_ENTRY <= BIG_WINDOW
      && BIG_WINDOW <= 32 * BIG_ENTRY + 4 * core::mem::size_of::<usize>(),
    "a U32 peek window must be one array of 32 entries plus the deque's own bookkeeping"
  );
};

/// Forty single-digit tokens: more than the widest window, so the fill stops at the
/// window bound rather than at end of input.
const BIG_SRC: &str = "1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 \
                       1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0";

#[test]
fn widest_peek_over_oversized_token_and_state_stays_in_stream_order() {
  use crate::cache::PeekedTokenExt as _;
  use generic_arraydeque::typenum::U32;

  // The widest window the crate allows, over a 1 KiB token payload and a 1 KiB lexer
  // state, against the three-slot default cache: 3 cache-resident entries and 29
  // staged ones, the largest staged region any peek can produce. Running the
  // monomorphization is the point — it is where an accounting slip in the staged
  // region would trip a debug assertion, and where a second window would be paid for.
  let cache = crate::cache::DefaultCache::<'_, BigLexer<'_>>::default();
  let mut input = crate::input::Input::<BigLexer<'_>, BigCtx<'_>, ()>::with_state_and_context(
    BIG_SRC,
    BigState::default(),
    crate::input::InputContext::new(crate::emitter::Silent::<()>::new(), cache),
  );
  let mut inp = input.as_ref();

  let peeked = inp.peek::<U32>().unwrap();
  assert_eq!(peeked.len(), 32, "the widest window must come back full");
  // Token `i` of the fixture starts at offset `2 * i`; stream order is therefore
  // visible as a strictly increasing run of even starts, across the boundary at 3.
  let starts = peeked
    .iter()
    .map(|t| t.span().start())
    .collect::<std::vec::Vec<_>>();
  assert_eq!(
    starts,
    (0..32).map(|i| i * 2).collect::<std::vec::Vec<_>>(),
    "the oversized window must read in stream order across the cache boundary"
  );
  let owned = peeked.iter().filter(|t| t.is_owned()).count();
  assert_eq!(
    owned, 29,
    "3 cache-resident entries, 29 staged past the cache"
  );

  // Both oversized halves really are carried per entry — the premise the size
  // assertions above rest on.
  match peeked[0].token() {
    BigTok::Num(payload) => assert_eq!(payload.probe(), 0),
  }
  let head_state = match &peeked[0] {
    mayber::Maybe::Ref(cached) => cached.state.probe(),
    mayber::Maybe::Owned(cached) => cached.state.probe(),
  };
  assert_eq!(head_state, 0, "every entry carries a whole lexer state");
}
