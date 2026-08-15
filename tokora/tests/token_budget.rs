#![cfg(all(feature = "std", feature = "combinators"))]

//! The **input-layer token budget** — the durable, driver-enforced bound on items the lexer
//! produces — exercised through the public API only.
//!
//! Three things are pinned here, and each is a property the lexer-side
//! [`TokenLimiter`](tokora::state::token_tracker::TokenLimiter) cannot deliver:
//!
//! 1. **the hostile shape** — one valid token followed by *N* bytes of plain lexer errors is *one
//!    accepted item*, under any bound denominated in accepts, while *N* scans run and *N*
//!    diagnostics land durably in the emitter's log. Section 1 measures that flood with the budget
//!    unbounded — which is the shape as it stands with no budget at all — and then charges it;
//! 2. **durability under rollback** — the same harness carries a by-value `TokenLimiter` in the
//!    lexer's `State`, and a declined `attempt` refunds it to zero while the input-layer budget
//!    keeps every item it charged;
//! 3. **terminality** — exhaustion latches the poison boundary, so a committed consume reports it
//!    terminal and a [`PartialSession`](tokora::input::PartialSession) latches instead of
//!    re-driving forever against a fresh per-`Input` budget;
//! 4. **the refusal is not refundable either** — the durable counter is only half of a bound. The
//!    other half is the stop that acts on it, and the poison boundary a refusal latches *is*
//!    rollbackable. Section 4 re-opens that stop by every route that clears it and counts the
//!    lexer calls an already-exhausted budget funds;
//! 5. **and it is not transplantable** — the same fact pointed the other way. A tally that a safe
//!    caller can install into an `Input` it did not come from fabricates a refusal that never
//!    happened, which is what section 6 pins, through both public `with_token_budget` doors.

use core::cell::Cell;

use tokora::{
  InputRef, Lexer, Parse, ParseContext, Parser, ParserContext, Partial, ScanLookahead, SimpleSpan,
  Token,
  cache::DefaultCache,
  emitter::Verbose,
  error::{Incomplete, MaybeIncomplete, MaybeTerminal, UnexpectedEot, token::UnexpectedToken},
  input::{Budget, InputContext, PartialSession, RedriveFromBase, SessionRefusal, TokenBudget},
  state::token_tracker::{TokenLimitExceeded, TokenLimiter},
};

// ── The instrument: every `lex()` call this process makes ─────────────────────────────────────

thread_local! {
  /// One increment per [`Lexer::lex`] call. The **work** figure the accept count is blind to.
  static SCANS: Cell<usize> = const { Cell::new(0) };
}

fn reset_scans() {
  SCANS.with(|c| c.set(0));
}

fn scans() -> usize {
  SCANS.with(Cell::get)
}

// ── Vocabulary ────────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WordKind;

impl core::fmt::Display for WordKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str("word")
  }
}

#[derive(Clone, Debug, PartialEq)]
struct Word;

/// A plain lexer error and a limit trip, kept apart: the hostile shape needs a byte that lexes as
/// an error **without** failing `Lexer::check`, which is precisely the case the crate's terminal
/// predicate documents as "the caller keeps scanning for the next valid token".
#[derive(Clone, Debug, PartialEq)]
enum LexErr {
  /// A byte in no token's language. `check()` stays `Ok`, so the scan goes on.
  Bad,
  /// The lexer-side `TokenLimiter` tripped — the refundable counter, kept for the contrast
  /// section 2 measures.
  Limit(TokenLimitExceeded),
}

impl Token<'_> for Word {
  type Kind = WordKind;
  type Error = LexErr;

  const SCAN_LOOKAHEAD: ScanLookahead = ScanLookahead::WithinSpan;

  fn kind(&self) -> WordKind {
    WordKind
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

/// One item per byte: `a` is a token, everything else is a one-byte plain lexer error.
struct FloodLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: TokenLimiter,
}

impl<'a> Lexer<'a> for FloodLexer<'a> {
  type State = TokenLimiter;
  type Source = str;
  type Token = Word;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self::with_state(src, TokenLimiter::new())
  }

  fn with_state(src: &'a str, state: TokenLimiter) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), LexErr> {
    self.state.check().map_err(LexErr::Limit)
  }

  fn state(&self) -> &TokenLimiter {
    &self.state
  }

  fn state_mut(&mut self) -> &mut TokenLimiter {
    &mut self.state
  }

  fn into_state(self) -> TokenLimiter {
    self.state
  }

  fn source(&self) -> &'a str {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }

  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }

  fn lex(&mut self) -> Option<Result<Word, LexErr>> {
    SCANS.with(|c| c.set(c.get() + 1));
    let b = self.src.as_bytes();
    self.start = self.end;
    if self.start >= b.len() {
      self.end = self.start;
      return None;
    }
    self.end = self.start + 1;
    if b[self.start] == b'a' {
      // Charged on the lexer side, exactly as smear's handlers do: this is the counter a
      // rollback refunds.
      self.state.increase();
      Some(Ok(Word))
    } else {
      Some(Err(LexErr::Bad))
    }
  }

  fn read_frontier(&self) -> tokora::ReadFrontier<usize> {
    tokora::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
    self.start = self.end;
  }
}

/// A lexer with a **tail it skips**: `a` is a token, a space produces no item at all, anything
/// else is a one-byte plain lexer error. Same vocabulary as [`FloodLexer`], so the same error type
/// and the same context serve both.
///
/// It exists because the end-of-source test alone cannot settle "is there another item?". After
/// the last token of `"aa  "` the lex position is `2` and the source length is `4`, so a positional
/// test says input remains — and no item does. Every lexer that skips whitespace or a comment tail
/// has this shape, which is to say almost every real one.
struct SkipLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: TokenLimiter,
}

impl<'a> Lexer<'a> for SkipLexer<'a> {
  type State = TokenLimiter;
  type Source = str;
  type Token = Word;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self::with_state(src, TokenLimiter::new())
  }

  fn with_state(src: &'a str, state: TokenLimiter) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), LexErr> {
    self.state.check().map_err(LexErr::Limit)
  }

  fn state(&self) -> &TokenLimiter {
    &self.state
  }

  fn state_mut(&mut self) -> &mut TokenLimiter {
    &mut self.state
  }

  fn into_state(self) -> TokenLimiter {
    self.state
  }

  fn source(&self) -> &'a str {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.end)
  }

  fn slice(&self) -> &'a str {
    &self.src[self.start..self.end]
  }

  fn lex(&mut self) -> Option<Result<Word, LexErr>> {
    SCANS.with(|c| c.set(c.get() + 1));
    let b = self.src.as_bytes();
    let mut at = self.end;
    // The skip: bytes consumed by the scan and emitted as no item at all.
    while at < b.len() && b[at] == b' ' {
      at += 1;
    }
    self.start = at;
    if at >= b.len() {
      // The trait's post-exhaustion span: well-formed, ending at the lexer's final position.
      self.end = at;
      return None;
    }
    self.end = at + 1;
    if b[at] == b'a' {
      self.state.increase();
      Some(Ok(Word))
    } else {
      Some(Err(LexErr::Bad))
    }
  }

  fn read_frontier(&self) -> tokora::ReadFrontier<usize> {
    tokora::ReadFrontier::SpanEnd
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
    self.start = self.end;
  }
}

// ── Error type ────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Err_ {
  Lex(LexErr),
  Unexpected,
  /// End of input, carrying whether the crate marked it terminal.
  Eot(bool),
  Incomplete,
  Refused(SessionRefusal),
}

impl From<LexErr> for Err_ {
  fn from(e: LexErr) -> Self {
    Err_::Lex(e)
  }
}

impl<'a, T, K: Clone, S, Lg: ?Sized> From<UnexpectedToken<'a, T, K, S, Lg>> for Err_ {
  fn from(_: UnexpectedToken<'a, T, K, S, Lg>) -> Self {
    Err_::Unexpected
  }
}

impl From<UnexpectedEot<usize>> for Err_ {
  fn from(e: UnexpectedEot<usize>) -> Self {
    Err_::Eot(e.is_terminal())
  }
}

impl From<Incomplete<usize>> for Err_ {
  fn from(_: Incomplete<usize>) -> Self {
    Err_::Incomplete
  }
}

impl From<SessionRefusal> for Err_ {
  fn from(r: SessionRefusal) -> Self {
    Err_::Refused(r)
  }
}

impl MaybeIncomplete for Err_ {
  fn is_incomplete(&self) -> bool {
    matches!(self, Err_::Incomplete)
  }
}

impl MaybeTerminal for Err_ {
  fn is_terminal(&self) -> bool {
    match self {
      Err_::Eot(terminal) => *terminal,
      Err_::Refused(_) => true,
      Err_::Lex(LexErr::Limit(_)) => true,
      _ => false,
    }
  }
}

type Lex<'a> = FloodLexer<'a>;
type Cache<'a> = DefaultCache<'a, Lex<'a>>;
type Ctx<'a> = ParserContext<'a, Lex<'a>, Verbose<Err_>, Cache<'a>>;

type SkipCache<'a> = DefaultCache<'a, SkipLexer<'a>>;
type SkipCtx<'a> = ParserContext<'a, SkipLexer<'a>, Verbose<Err_>, SkipCache<'a>>;

/// `"a"` then `n` bytes of garbage: one valid token, then a flood of plain lexer errors.
fn hostile(n: usize) -> String {
  let mut s = String::with_capacity(n + 1);
  s.push('a');
  for _ in 0..n {
    s.push('!');
  }
  s
}

/// What one drain of the flood costs.
#[derive(Debug, PartialEq, Eq)]
struct Cost {
  /// Items the **grammar** accepted — the unit a per-accept bound would count.
  accepted: usize,
  /// [`Lexer::lex`] calls — the work.
  scans: usize,
  /// Diagnostics durably retained by the emitter — the memory.
  diagnostics: usize,
  /// What the input-layer budget charged.
  charged: usize,
}

/// Drains to exhaustion with [`InputRef::next`], reporting every figure the emitter and the input
/// can still answer for while the parse is alive.
fn drain<'inp>(
  inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>,
) -> Result<(usize, usize, usize), Err_> {
  let mut accepted = 0usize;
  while inp.next()?.is_some() {
    accepted += 1;
  }
  Ok((
    accepted,
    inp.emitter_ref().diagnostics().len(),
    inp.token_budget().spent(),
  ))
}

fn drain_under(src: &str, budget: TokenBudget) -> Cost {
  reset_scans();
  let (accepted, diagnostics, charged) =
    Parser::with_context(Ctx::new(Verbose::new()).with_token_budget(budget))
      .apply(drain)
      .parse_with_state(src, TokenLimiter::new())
      .expect("a collecting emitter never makes the flood fatal");

  Cost {
    accepted,
    scans: scans(),
    diagnostics,
    charged,
  }
}

const N: usize = 512;

// ── 1. The hostile shape ──────────────────────────────────────────────────────────────────────

/// **The pre-state, reproduced.** With the budget unbounded — the behaviour of every build before
/// this feature, and of every consumer that does not opt in — one accepted item costs `N` scans
/// and `N` durably-retained diagnostics.
///
/// This is the measurement the unit "one charge per accepted item" was rejected on: the accept
/// count is `1` and does not move with `N`, so a ceiling denominated in accepts sees none of it.
/// Measured at `a9ed3de` over `N` in {64, 128, 512, 2048}: `accepted` stayed `1` while `scans` ran
/// {66, 130, 514, 2050} and `diagnostics` {64, 128, 512, 2048}.
#[test]
fn an_error_flood_is_one_accept_and_o_n_work_when_the_budget_is_unbounded() {
  let cost = drain_under(&hostile(N), TokenBudget::unlimited());

  assert_eq!(
    cost,
    Cost {
      // One accept, whatever N is. A per-accept ceiling of 2 would have been satisfied.
      accepted: 1,
      // One scan per byte, plus the exhausting call that returns `None`.
      scans: N + 2,
      // One diagnostic per garbage byte, every one of them retained.
      diagnostics: N,
      // The figure the accept count is blind to: N+1 items produced, the token and every error.
      // Unbounded, so nothing is refused — what changed is that the flood is now *counted*.
      charged: N + 1,
    }
  );
}

/// The same input under a **bounded** budget: the flood is cut at the ceiling. `charged` is the
/// figure that moved with `N` above and does not move now, which is the whole of the fix.
#[test]
fn a_bounded_budget_charges_the_error_flood_and_stops_it() {
  const B: usize = 8;
  let cost = drain_under(&hostile(N), TokenBudget::with_limitation(B));

  assert_eq!(cost.charged, B, "exactly the ceiling, not N + 1");
  assert_eq!(
    cost.scans,
    B + 1,
    "B items lexed, then the ONE at-limit probe — never the B+2-th. The ceiling is tested in \
     front of `Lexer::lex`, so no refusal calls it; the single extra call is the probe that told \
     a met ceiling from an end of input, and it is latched in the budget so it never runs twice. \
     Note the figure it equals: `N + 2` for `N + 1` items is what the UNBOUNDED drain above costs, \
     items plus one exhausting probe. A ceiling now costs the same shape as an end of input, once."
  );
  assert_eq!(
    cost.diagnostics,
    B - 1,
    "the token, then B-1 errors: nothing past the ceiling reaches the log"
  );
  assert_eq!(cost.accepted, 1, "the one valid token still came through");
}

/// The ceiling holds independently of `N`: quadrupling the flood changes nothing it bounds.
///
/// This is the falsifier for the pre-state. Run the same comparison against the accept count and
/// it passes vacuously — `1 == 1` for every `N` — which is exactly why the accept count was the
/// wrong unit.
#[test]
fn the_ceiling_does_not_move_with_the_size_of_the_flood() {
  const B: usize = 8;
  let small = drain_under(&hostile(N), TokenBudget::with_limitation(B));
  let large = drain_under(&hostile(N * 4), TokenBudget::with_limitation(B));
  assert_eq!(small, large);
}

/// **Trivia is charged**, in the only sense this harness can state it: the budget's unit is the
/// item the lexer produced, not the item the grammar kept. Here every byte is an item and only the
/// `a`s are ever accepted, so a source of `N` garbage bytes with no accepted token at all still
/// costs `N`.
#[test]
fn an_item_the_grammar_never_keeps_is_still_charged() {
  reset_scans();
  let cost = drain_under("!!!!", TokenBudget::unlimited());
  assert_eq!(cost.accepted, 0, "nothing was kept");
  assert_eq!(cost.charged, 4, "and all four items were charged");
}

/// A **peek-fill is charged at production, not at consumption**: a peek that fills the cache and
/// is then abandoned has already spent, because the lexing already happened.
#[test]
fn a_peek_fill_is_charged_at_production_even_when_nothing_consumes_it() {
  fn peek_only<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    // One peek, filling the cache; nothing is consumed afterwards.
    let _ = inp.peek_one()?;
    Ok(inp.token_budget().spent())
  }

  let spent = Parser::with_context(Ctx::new(Verbose::new()))
    .apply(peek_only)
    .parse_with_state("aaaa", TokenLimiter::new())
    .expect("no error on this path");

  assert_eq!(spent, 1, "the filled token is charged at production");
}

/// **Cache replay is not charged**, and this is the fourth sub-decision measured: consuming a token
/// the cache already holds does no lexing, so it reaches no classification and costs nothing. The
/// budget prices lexing, not consumption.
#[test]
fn consuming_a_cached_token_does_not_charge_it_twice() {
  fn peek_then_take<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>,
  ) -> Result<(usize, usize), Err_> {
    let _ = inp.peek_one()?;
    let after_peek = inp.token_budget().spent();
    // Served from the cache: no `lex`, no classification, no charge.
    assert!(inp.next()?.is_some());
    Ok((after_peek, inp.token_budget().spent()))
  }

  let (after_peek, after_take) = Parser::with_context(Ctx::new(Verbose::new()))
    .apply(peek_then_take)
    .parse_with_state("aaaa", TokenLimiter::new())
    .expect("no error on this path");

  assert_eq!(after_peek, 1);
  assert_eq!(after_take, 1, "the consume re-charged nothing");
}

/// The peek fill has its own arm for the refusal, and it must produce a **short window** rather
/// than a full one: a lookahead the budget refused to fund cannot report tokens it never lexed.
#[test]
fn a_peek_fill_that_the_budget_refuses_returns_a_short_window() {
  fn peek_then_consume<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>,
  ) -> Result<(bool, usize), Err_> {
    // Nothing is authorized, so the fill produces one item, has it refused, and stops empty.
    let empty = inp.peek_one()?.is_none();
    let spent = inp.token_budget().spent();
    // And the refusal latched: the committed consume reports it terminal rather than as the end
    // of a four-byte source that plainly has not ended.
    match inp.next_or_stop() {
      Err(e) => {
        assert_eq!(e, Err_::Eot(true));
        Ok((empty, spent))
      }
      Ok(_) => panic!("a refused peek must leave a terminal stop behind it"),
    }
  }

  let (empty, spent) = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(0)),
  )
  .apply(peek_then_consume)
  .parse_with_state("aaaa", TokenLimiter::new())
  .expect("no error on this path");

  assert!(empty, "the window is short — empty, here");
  assert_eq!(spent, 0, "a refusal is not a charge");
}

// ── 2. Durability: the rollback plant ─────────────────────────────────────────────────────────

/// **Plant.** Spend inside an `attempt`, decline it, and compare the two counters that counted the
/// same items.
///
/// The lexer-side `TokenLimiter` lives in `L::State`, which is a `Checkpoint` field, so the restore
/// puts the saved count back: `0 → 4 → 0`. The input-layer budget is not a `Checkpoint` field, so
/// it does not move back. That contrast is what makes one of them a budget.
///
/// Falsifying output: `after == before` on the budget row. Put the budget inside `Checkpoint` — or
/// on the lexer's `State` — and that is what this prints.
#[test]
fn a_declined_attempt_refunds_the_lexer_tally_and_not_the_input_budget() {
  type Row = (usize, usize, usize);

  fn probe<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<(Row, Row), Err_> {
    let tally_before = inp.state().tokens();
    let budget_before = inp.token_budget().spent();

    let inside = Cell::new((0usize, 0usize));
    let kept: Option<()> = inp.attempt(|txn| {
      while txn.next().ok().flatten().is_some() {}
      inside.set((txn.state().tokens(), txn.token_budget().spent()));
      // Decline: everything this attempt did is rolled back.
      None
    });
    assert!(kept.is_none(), "the attempt declined");

    let (tally_inside, budget_inside) = inside.get();
    Ok((
      (tally_before, tally_inside, inp.state().tokens()),
      (budget_before, budget_inside, inp.token_budget().spent()),
    ))
  }

  let (tally, budget) = Parser::with_context(Ctx::new(Verbose::new()))
    .apply(probe)
    .parse_with_state("aaaa", TokenLimiter::new())
    .expect("no error on this path");

  // The refundable counter: spent, then given back by the restore that reinstalled the state.
  assert_eq!(tally, (0, 4, 0), "the lexer-side tally is refunded");
  // The durable one: spent, and kept.
  assert_eq!(budget, (0, 4, 4), "the input-layer budget is not");
}

/// The same contrast one level up: a declined attempt does not buy back headroom, so a budget of
/// `B` still refuses after `B` items even when every one of them was rolled back.
#[test]
fn a_rolled_back_attempt_still_spends_the_ceiling() {
  fn probe<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    let kept: Option<()> = inp.attempt(|txn| {
      while txn.next().ok().flatten().is_some() {}
      None
    });
    assert!(kept.is_none());
    // The cursor is back at zero and the source is untouched, so a lexer-side tally would let
    // this drain run again in full. Count what it actually yields.
    let mut again = 0usize;
    while inp.next()?.is_some() {
      again += 1;
    }
    Ok(again)
  }

  let again = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(4)),
  )
  .apply(probe)
  .parse_with_state("aaaaaaaa", TokenLimiter::new())
  .expect("no error on this path");

  assert_eq!(
    again, 0,
    "the four items the declined attempt lexed were already paid for"
  );
}

// ── 3. Terminality ────────────────────────────────────────────────────────────────────────────

/// Exhaustion is a **terminal** stop, not a quiet end of input: a committed consume
/// ([`InputRef::next_or_stop`]) surfaces the end-of-input error already marked terminal, exactly
/// as it does for a lexer resource-limit trip.
#[test]
fn exhaustion_reports_itself_terminal_to_a_committed_consume() {
  fn drive<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    let mut n = 0usize;
    while inp.next_or_stop()?.is_some() {
      n += 1;
    }
    Ok(n)
  }

  let outcome = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
  )
  .apply(drive)
  .parse_with_state("aaaaaaaa", TokenLimiter::new());

  let err = outcome.expect_err("the budget refused before the source ran out");
  assert_eq!(err, Err_::Eot(true), "marked terminal");
  assert!(err.is_terminal());
}

/// The contrast that makes the assertion above mean something: the *same* consume over a source
/// that genuinely runs out reports a **non**-terminal end of input.
#[test]
fn a_genuine_end_of_input_is_not_reported_terminal() {
  fn drive<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    let mut n = 0usize;
    while inp.next_or_stop()?.is_some() {
      n += 1;
    }
    // Exhausted with room to spare: ask once more, and build the plain end-of-input the grammar
    // would build.
    Ok(n)
  }

  let n = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(64)),
  )
  .apply(drive)
  .parse_with_state("aaaa", TokenLimiter::new())
  .expect("a genuine end of input is `Ok(None)`, not an error");
  assert_eq!(n, 4);
}

/// **Plant.** A [`PartialSession`] rebuilds a fresh `Input` — and therefore a fresh budget — for
/// every redrive, so the budget alone cannot bound a session. What stops the redrive is the
/// session's terminal latch, and the latch is fed by the terminality the test above pins.
///
/// Falsifying output: the second attempt returns `Ok`/`Eot(false)` rather than
/// `TerminalLatched` — which is what a non-terminal exhaustion produces, forever, one redrive
/// after another.
#[test]
fn a_session_redrive_cannot_reset_the_refusal() {
  fn drive<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, (), Partial>,
  ) -> Result<usize, Err_> {
    let mut n = 0usize;
    while inp.next_or_stop()?.is_some() {
      n += 1;
    }
    Ok(n)
  }

  let mut session = PartialSession::new(TokenLimiter::new(), Budget::Unbounded, RedriveFromBase);
  let budgeted = || Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2));

  let first = session.parse(budgeted(), "aaaaaaaa", true, drive);
  assert_eq!(
    first,
    Err(Err_::Eot(true)),
    "the first attempt refuses, terminally"
  );
  assert!(session.is_latched(), "and the session latched on it");
  let spent_after_first = session.spent();

  // The redrive would get a fresh `Input` and a fresh budget — and never runs, because the latch
  // is consulted before any work.
  let second = session.parse(budgeted(), "aaaaaaaa", true, drive);
  assert_eq!(
    second,
    Err(Err_::Refused(SessionRefusal::TerminalLatched)),
    "still refused"
  );
  assert_eq!(
    session.spent(),
    spent_after_first,
    "and refused before any work: a refused attempt spends no bytes"
  );
}

// ── 4. The rollback door: an exhausted budget must fund no lexer call ─────────────────────────

/// How the attacker clears the terminal stop between rounds. Every variant is a **public** API,
/// and every one of them restores or drops the poison boundary the refusal latched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reopen {
  /// An [`InputRef::attempt`] that drains to the refusal and then declines. The rollback copies
  /// the checkpoint's saved boundary back — `None`, since the save predates the refusal.
  DeclinedAttempt,
  /// [`InputRef::set_state`], whose re-key drops the boundary outright (the documented
  /// limit-recovery path).
  SetState,
  /// [`InputRef::state_mut`], which runs the identical re-key before handing out the `&mut`.
  StateMut,
  /// [`InputRef::sync_through`], a **fourth** boundary-clearing route the finding did not name: a
  /// rewinding sync snapshots the boundary in its own positional memo (`ThroughEntry`, not a
  /// `Checkpoint`) and `restore_entry` puts it back on the no-match exit — a door the checkpoint
  /// machinery knows nothing about.
  ///
  /// Measured, it is **not** a fourth attack: under the pre-fix ordering it cost a *constant* `1`
  /// lexer call at a zero budget and `5` at `B = 4`, flat across 4, 16 and 256 rounds, where the
  /// three rollback routes grew one per round. A refusal exits through the scan's **trip** exit,
  /// which keeps its progress and never reaches the rewinding restore, so the boundary it latched
  /// survives the call and the next round stops at it. The row is kept because that argument is
  /// about which *exit* a refusal takes — a property of the sync drivers, not of the budget — and
  /// the row is what would notice it changing.
  ///
  /// Those two pre-fix figures are now what **all four** rows read, and the coincidence is worth
  /// not misreading: this row measured `1` and `5` because a refusal kept the boundary it latched,
  /// and every row measures them now because the ceiling funds exactly one at-limit probe. Same
  /// numbers, different mechanism — which is precisely why the row still earns its place.
  SyncThrough,
}

thread_local! {
  /// How many times the probe below re-opens the stop.
  static ROUNDS: Cell<usize> = const { Cell::new(0) };
  /// Which door it re-opens it through.
  static HOW: Cell<Reopen> = const { Cell::new(Reopen::DeclinedAttempt) };
}

/// Drains to the budget's refusal, clears the stop, and repeats. Reports what the budget spent;
/// the [`SCANS`] instrument reports what the sequence cost.
fn reopen_probe<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
  let rounds = ROUNDS.with(Cell::get);
  let how = HOW.with(Cell::get);
  for _ in 0..rounds {
    match how {
      Reopen::DeclinedAttempt => {
        let kept: Option<()> = inp.attempt(|txn| {
          while txn.next().ok().flatten().is_some() {}
          // Decline: the restore puts the pre-refusal boundary — `None` — back.
          None
        });
        assert!(kept.is_none(), "the attempt declined");
      }
      Reopen::SetState => {
        while inp.next().ok().flatten().is_some() {}
        inp.set_state(TokenLimiter::new());
      }
      Reopen::StateMut => {
        while inp.next().ok().flatten().is_some() {}
        let _ = inp.state_mut();
      }
      Reopen::SyncThrough => {
        // A predicate nothing satisfies, so the scan runs to its no-match exit and takes the
        // rewinding restore with it.
        let _ = inp.sync_through(|_| false, || None)?;
      }
    }
  }
  Ok(inp.token_budget().spent())
}

/// `(lexer calls, budget spent)` after `rounds` drain-and-re-open cycles.
fn reopen(src: &str, budget: TokenBudget, how: Reopen, rounds: usize) -> (usize, usize) {
  ROUNDS.with(|c| c.set(rounds));
  HOW.with(|c| c.set(how));
  reset_scans();
  let spent = Parser::with_context(Ctx::new(Verbose::new()).with_token_budget(budget))
    .apply(reopen_probe)
    .parse_with_state(src, TokenLimiter::new())
    .expect("a collecting emitter never makes this fatal");
  (scans(), spent)
}

const ROUTES: [Reopen; 4] = [
  Reopen::DeclinedAttempt,
  Reopen::SetState,
  Reopen::StateMut,
  Reopen::SyncThrough,
];
const ROUNDS_SWEEP: [usize; 3] = [4, 16, 256];

/// The whole route × rounds matrix, so a failure prints every route rather than the first one.
fn sweep(src: &str, budget: TokenBudget) -> [[(usize, usize); 3]; 4] {
  let mut out = [[(0usize, 0usize); 3]; 4];
  for (row, how) in out.iter_mut().zip(ROUTES) {
    for (cell, rounds) in row.iter_mut().zip(ROUNDS_SWEEP) {
      *cell = reopen(src, budget, how, rounds);
    }
  }
  out
}

/// **Plant.** A budget of **zero** authorizes nothing, so it must fund exactly **one** `Lexer::lex`
/// call — the one-shot at-limit probe — however many times, and by whichever door, its stop is
/// re-opened. One is not a weaker claim than zero: what an attack needs is a call *per round*, and
/// the cell is flat in the round count.
///
/// Falsifying output, measured against the pre-fix ordering (the ceiling asked *after* the lex):
/// the scan count tracks the round count exactly — `(4, 0)`, `(16, 0)`, `(256, 0)` on the three
/// rollback rows — while `spent` stays `0`, because a refusal is not a charge. A ceiling that
/// authorizes nothing was funding one full lexer invocation per public call, and the attacker paid
/// for none of them. The `SyncThrough` row read a flat `(1, 0)` instead: see [`Reopen`].
///
/// Falsifying output if the probe's latch moved out of [`TokenBudget`] and into the poison
/// boundary: the same `(4, 0) / (16, 0) / (256, 0)`, because the boundary is what every one of
/// these four doors clears.
#[test]
fn a_zero_budget_funds_one_probe_however_often_the_stop_is_reopened() {
  assert_eq!(
    sweep(&hostile(N), TokenBudget::with_limitation(0)),
    [[(1, 0); 3]; 4],
    "rows are {ROUTES:?}, columns {ROUNDS_SWEEP:?}, cells (lexer calls, spent)"
  );
}

/// **Plant.** The same door, one level up: once a *bounded* budget is exhausted, re-opening its
/// stop must not buy another lexer call. The total work is the ceiling plus the one probe, and it
/// is **invariant in the round count** — which is what "the budget bounds the work" actually
/// consists of.
///
/// Falsifying output, measured against the pre-fix ordering: `B + rounds` scans — `(8, 4)`,
/// `(20, 4)`, `(260, 4)` at `B = 4` on the three rollback rows. The counter was durable and the
/// enforcement was not, so `spent` sat pinned at the ceiling while the lexer ran without bound.
/// The `SyncThrough` row read a flat `(5, 4)`: see [`Reopen`].
#[test]
fn an_exhausted_budget_funds_one_probe_however_often_the_stop_is_reopened() {
  const B: usize = 4;
  assert_eq!(
    sweep("aaaaaaaa", TokenBudget::with_limitation(B)),
    [[(B + 1, B); 3]; 4],
    "rows are {ROUTES:?}, columns {ROUNDS_SWEEP:?}, cells (lexer calls, spent)"
  );
}

/// The re-opened stop is still **terminal** on every round: clearing the boundary buys a fresh
/// *check*, never a fresh lex, and the check refuses.
///
/// Guards the repair against the lazy fix of simply never re-latching: a preflight that refuses
/// without recording the stop would leave a committed consume reporting a plain, non-terminal end
/// of input, and a `PartialSession` would redrive on it forever.
#[test]
fn the_refusal_re_latches_on_every_reopened_round() {
  fn probe<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    for round in 0..8 {
      inp.set_state(TokenLimiter::new());
      match inp.next_or_stop() {
        Err(e) => assert_eq!(e, Err_::Eot(true), "round {round} lost terminality"),
        Ok(_) => panic!("round {round}: a zero budget authorized an item"),
      }
    }
    Ok(inp.token_budget().spent())
  }

  reset_scans();
  let spent = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(0)),
  )
  .apply(probe)
  .parse_with_state("aaaa", TokenLimiter::new())
  .expect("no error on this path");

  assert_eq!(spent, 0);
  assert_eq!(
    scans(),
    1,
    "eight terminal stops, one lexer call: the first round's one-shot probe established that an \
     item was there to refuse, and the seven that follow are answered out of the latch"
  );
}

/// The **unlimited sentinel**, measured rather than asserted: the preflight is a ceiling test, and
/// an unbounded ceiling is never met, so no item is ever refused and the work is exactly the work.
///
/// The figure that would move if the sentinel were mishandled is `scans`: a preflight that refused
/// on `usize::MAX` would cut the drain short, and one that skipped the charge would leave `charged`
/// behind the item count. Both are read here against the same drain the unbounded section measures.
#[test]
fn the_unlimited_sentinel_refuses_nothing_and_costs_no_extra_scan() {
  let unlimited = drain_under(&hostile(N), TokenBudget::unlimited());
  // Every item produced, every item charged, and one scan per item plus the exhausting probe.
  assert_eq!(unlimited.charged, N + 1);
  assert_eq!(unlimited.scans, N + 2);
  assert_eq!(unlimited.diagnostics, N);

  // And the sentinel is not "a ceiling that happens not to be met": re-opening the stop finds
  // nothing to refuse, so the drain runs to the genuine end of the source and every later round
  // pays only the one exhausting probe. A preflight that mistook `usize::MAX` for a met ceiling
  // would show `scans == 0` here.
  let (scans, spent) = reopen("aaaaaaaa", TokenBudget::unlimited(), Reopen::SetState, 4);
  assert_eq!(spent, 8, "all eight items, charged once each");
  assert_eq!(
    scans,
    8 + 4,
    "eight items plus one exhausting probe per round: nothing was refused"
  );
}

// ── 5. A met ceiling is not an end of input, and an end of input is not a met ceiling ─────────

/// Drains a [`SkipLexer`] source with the committed consume, reporting how many items came out.
fn drive_skip<'inp>(
  inp: &mut InputRef<'inp, '_, SkipLexer<'inp>, SkipCtx<'inp>>,
) -> Result<usize, Err_> {
  let mut n = 0usize;
  while inp.next_or_stop()?.is_some() {
    n += 1;
  }
  Ok(n)
}

/// **Plant.** A budget of **zero** over an **empty** source. There is no item, so there is nothing
/// to refuse: the answer is a genuine end of input.
///
/// Falsifying output, measured against the preflight as it landed in `7a28068`:
/// `Err(Eot(true))` — a ceiling that has refused nothing reporting a *terminal* stop over a source
/// with nothing in it. The preflight asked `spent >= max` before it knew whether an item existed,
/// so its answer was a property of the counter alone.
/// Read through the **committed** consume, deliberately: [`InputRef::next`] folds a terminal stop
/// and a genuine end of input into the same `Ok(None)`, so it cannot see this defect at all.
#[test]
fn a_zero_budget_over_an_empty_source_is_end_of_input_not_exhaustion() {
  fn drive<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    let mut n = 0usize;
    while inp.next_or_stop()?.is_some() {
      n += 1;
    }
    Ok(n)
  }

  reset_scans();
  let n = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(0)),
  )
  .apply(drive)
  .parse_with_state("", TokenLimiter::new())
  .expect("an empty source ends; it does not trip");

  assert_eq!(n, 0);
  assert_eq!(
    scans(),
    0,
    "and the end-of-source test answered it without a lexer call"
  );
}

/// **Plant.** The same shape one level up, and the one a calibrated budget actually meets: a
/// ceiling of exactly the source's item count. After the last item the lex position *is* the end
/// of the source, so the next step is an end of input — not the `max + 1`-th item being refused.
///
/// Falsifying output, measured against `7a28068`: `Err(Eot(true))` after four items on a
/// four-item source. A terminal-aware consumer rejects a document it fully parsed, and a
/// `PartialSession` latches on it permanently.
#[test]
fn a_budget_met_exactly_at_the_last_item_reports_a_genuine_end_of_input() {
  fn drive<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    let mut n = 0usize;
    while inp.next_or_stop()?.is_some() {
      n += 1;
    }
    Ok(n)
  }

  reset_scans();
  let n = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(4)),
  )
  .apply(drive)
  .parse_with_state("aaaa", TokenLimiter::new())
  .expect("four items under a ceiling of four is a complete parse, not a terminal stop");

  assert_eq!(n, 4);
  assert_eq!(
    scans(),
    4,
    "four items, and the end of input settled positionally — the ceiling bought no probe here"
  );
}

/// The smallest ceiling that authorizes anything, in both of its shapes — the boundary between
/// "the first item is authorized" and "the second is refused", which zero and `B = 8` bracket but
/// neither one sits on.
///
/// Exactly-met (`"a"`, one item) must end; met-with-more (`"aa"`) must refuse, terminally, for the
/// price of the one probe. A repair that got the end-of-input case right by weakening the refusal
/// shows up here as the second half returning `Ok`.
#[test]
fn a_ceiling_of_one_ends_on_a_one_item_source_and_refuses_on_a_longer_one() {
  fn drive<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    let mut n = 0usize;
    while inp.next_or_stop()?.is_some() {
      n += 1;
    }
    Ok(n)
  }

  let one = || Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(1));

  reset_scans();
  let exact = Parser::with_context(one())
    .apply(drive)
    .parse_with_state("a", TokenLimiter::new())
    .expect("one item under a ceiling of one is a complete parse");
  assert_eq!(exact, 1);
  assert_eq!(
    scans(),
    1,
    "the end of input settled positionally: no probe"
  );

  reset_scans();
  let refused = Parser::with_context(one())
    .apply(drive)
    .parse_with_state("aa", TokenLimiter::new())
    .expect_err("the second item is over the ceiling");
  assert_eq!(refused, Err_::Eot(true), "and the refusal is terminal");
  assert_eq!(
    scans(),
    2,
    "the authorized item, then the one probe that found the item the ceiling refuses"
  );
}

/// **Plant.** The residue the end-of-source test cannot see: a tail the lexer *skips*. After the
/// last token of `"aa  "` the lex position is `2` and the source length is `4`, so the positional
/// test says input remains — and no item does. Only running the lexer once can tell the two apart.
///
/// Falsifying output, measured against `7a28068`: `Err(Eot(true))` after two items. This is the
/// shape almost every real lexer has, so it is the shape a calibrated ceiling meets in the field.
#[test]
fn a_tail_the_lexer_skips_is_an_end_of_input_not_a_met_ceiling() {
  reset_scans();
  let n = Parser::with_context(
    SkipCtx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
  )
  .apply(drive_skip)
  .parse_with_state("aa  ", TokenLimiter::new())
  .expect("the tail is skipped, so the stream ended; the ceiling refused nothing");

  assert_eq!(n, 2);
  assert_eq!(
    scans(),
    3,
    "two items, then the one at-limit probe that discovered the tail holds none"
  );
}

/// The probe that found **nothing** latches nothing, so the end-of-input answer is *stable*: ask
/// again and the answer is the same, at the same cost an unbudgeted input pays to answer it.
///
/// This is the half of the one-shot that is deliberately **not** one-shot. A probe that produced
/// no item performed no work the ceiling exists to bound, and it costs exactly what the same
/// question costs with no budget configured at all — one `Lexer::lex` per ask, which is the
/// crate's standing end-of-input cost on every input, budgeted or not.
#[test]
fn a_probe_that_found_no_item_leaves_the_end_of_input_answer_stable() {
  fn ask_three_times<'inp>(
    inp: &mut InputRef<'inp, '_, SkipLexer<'inp>, SkipCtx<'inp>>,
  ) -> Result<usize, Err_> {
    let mut n = 0usize;
    while inp.next_or_stop()?.is_some() {
      n += 1;
    }
    for round in 0..2 {
      match inp.next_or_stop() {
        Ok(None) => {}
        other => panic!("round {round}: the end of input moved to {other:?}"),
      }
    }
    Ok(n)
  }

  reset_scans();
  let budgeted = Parser::with_context(
    SkipCtx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
  )
  .apply(ask_three_times)
  .parse_with_state("aa  ", TokenLimiter::new())
  .expect("no error on this path");
  assert_eq!(budgeted, 2);
  let budgeted_scans = scans();

  reset_scans();
  let unbudgeted = Parser::with_context(SkipCtx::new(Verbose::new()))
    .apply(ask_three_times)
    .parse_with_state("aa  ", TokenLimiter::new())
    .expect("no error on this path");
  assert_eq!(unbudgeted, 2);

  assert_eq!(
    budgeted_scans,
    scans(),
    "asking an exhausted budget where the stream ended costs exactly what asking an unbounded one \
     costs: the budget prices items produced, and these probes produce none"
  );
}

// ── 6. The tally is the input's, and only the ceiling crosses the door ────────────────────────
//
// A budget is two things that were one type until this section existed: the **ceiling** a caller
// configures, and the **tally** one `Input` keeps against it. `TokenBudget` is the first and is
// `Copy`, because both `with_token_budget` doors are public and take it by value.
// `TokenBudgetTally` is the second and is neither `Clone` nor `Copy`, has no public constructor,
// and is reachable only by reference through `InputRef::token_budget`.
//
// While they were one type, `Copy` carried the live cell through the doors. The reviewer's path,
// reproduced at `6aa0b08`: drain a parse under a ceiling of 2 until it refuses, `*inp
// .token_budget()` the value out, hand it to a **fresh** parse over an **empty** source. Through
// `ParserContext::with_token_budget` and through a caller-written `ParseContext` reaching
// `InputContext::with_token_budget`, both read
// `(outcome: Err(terminal), refused_an_item: true, scans: 0)` — the first driver gate counted a
// scanner trip and reported a terminal stop, with `Lexer::lex` never invoked in that input, on an
// empty source. The redrive variant fed the same value to a second `PartialSession` attempt and
// read `items=0 refused_an_item=true scans=0`.
//
// That is the counter's own defect pointed backwards. The rest of this suite exists because a
// rollback must not refund work that happened; this section exists because a transplant must not
// fabricate work that did not.
//
// The cells below carry the reviewer's path as far as the API still allows — a caller can copy the
// ceiling, and there is nothing else left to copy — and verify each of the three bounds written at
// the accessor.

/// A caller-written [`ParseContext`] reaching [`InputContext::with_token_budget`] directly: the
/// second public door, and the one a `ParserContext` cell alone would not cover.
struct RawDoor<'a> {
  budget: TokenBudget,
  _l: core::marker::PhantomData<&'a ()>,
}

impl<'inp> ParseContext<'inp, Lex<'inp>> for RawDoor<'inp> {
  type Emitter = Verbose<Err_>;
  type Cache = Cache<'inp>;

  fn provide(self) -> InputContext<Self::Emitter, Self::Cache> {
    InputContext::new(Verbose::new(), Cache::<'inp>::new()).with_token_budget(self.budget)
  }
}

/// What a fresh parse saw: items reported (or the terminal flag of the error that stopped it),
/// the durable refusal witness, the spend, and the lexer calls it made.
#[derive(Debug, PartialEq, Eq)]
struct Fresh {
  outcome: Result<usize, bool>,
  refused: bool,
  spent: usize,
  scans: usize,
}

macro_rules! fresh_driver {
  ($name:ident, $ctx:ty) => {
    fn $name<'inp>(
      inp: &mut InputRef<'inp, '_, Lex<'inp>, $ctx>,
    ) -> Result<(usize, bool, usize), Err_> {
      let mut n = 0usize;
      while inp.next_or_stop()?.is_some() {
        n += 1;
      }
      Ok((
        n,
        inp.token_budget().refused_an_item(),
        inp.token_budget().spent(),
      ))
    }
  };
}

fresh_driver!(drive_fresh, Ctx<'inp>);
fresh_driver!(drive_fresh_raw, RawDoor<'inp>);

fn fresh(outcome: Result<(usize, bool, usize), Err_>, scans: usize) -> Fresh {
  match outcome {
    Ok((n, refused, spent)) => Fresh {
      outcome: Ok(n),
      refused,
      spent,
      scans,
    },
    Err(e) => Fresh {
      outcome: Err(e.is_terminal()),
      // Unreadable on this arm — the input is gone with the error — so it is reported as the
      // worst case rather than as a `false` that could be mistaken for a passing row.
      refused: true,
      spent: usize::MAX,
      scans,
    },
  }
}

/// Drains a parse to its refusal and hands back **everything a caller can still carry out of it**.
///
/// That is the ceiling and nothing else. `inp.token_budget()` is a `&TokenBudgetTally`, which has
/// no `Clone`, no `Copy` and no public constructor, so `*inp.token_budget()` does not compile:
/// at `6aa0b08` it did, and it is what made the transplant possible.
fn refuse_and_carry_the_ceiling() -> TokenBudget {
  fn drain<'inp>(inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>) -> Result<usize, Err_> {
    while inp.next()?.is_some() {}
    assert!(
      inp.token_budget().refused_an_item(),
      "precondition: the source parse really did refuse an item"
    );
    assert_eq!(inp.token_budget().spent(), 2);
    Ok(inp.token_budget().limitation())
  }

  let limitation = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
  )
  .apply(drain)
  .parse_with_state("aaaaaaaa", TokenLimiter::new())
  .expect("`next` folds the terminal stop into Ok(None)");

  TokenBudget::with_limitation(limitation)
}

/// **The transplant, carried as far as the API allows, through both public doors.**
///
/// A ceiling read out of a parse that refused, installed into a fresh `Input` over an **empty**
/// source. The empty source is the discriminator: it holds no item at any ceiling, so a terminal
/// stop there cannot be the budget correctly refusing something — it can only be a refusal the new
/// input inherited. `scans: 1` is the one `Lexer::lex` that reports the end of input; the
/// transplant's signature was `scans: 0`, a stop decided before the lexer was ever asked.
///
/// Falsifying output, and the pre-repair reading of both rows:
/// `Fresh { outcome: Err(true), refused: true, spent: 18446744073709551615, scans: 0 }`.
#[test]
fn a_ceiling_carried_out_of_a_refused_parse_carries_no_refusal_with_it() {
  let ceiling = refuse_and_carry_the_ceiling();
  assert_eq!(ceiling.limitation(), 2, "the ceiling is what crosses");

  reset_scans();
  let door_a = fresh(
    Parser::with_context(Ctx::new(Verbose::new()).with_token_budget(ceiling))
      .apply(drive_fresh)
      .parse_with_state("", TokenLimiter::new()),
    scans(),
  );

  reset_scans();
  let door_b = fresh(
    Parser::with_context(RawDoor {
      budget: ceiling,
      _l: core::marker::PhantomData,
    })
    .apply(drive_fresh_raw)
    .parse_with_state("", TokenLimiter::new()),
    scans(),
  );

  let clean = Fresh {
    outcome: Ok(0),
    refused: false,
    spent: 0,
    scans: 1,
  };
  assert_eq!(
    door_a, clean,
    "ParserContext::with_token_budget: a fresh Input over an empty source is a plain end of input"
  );
  assert_eq!(
    door_b, clean,
    "InputContext::with_token_budget: and so is the other door"
  );
}

/// **The negative.** None of the above may be bought by making every budget refuse, or by making
/// the witness unwritable.
///
/// Three rows, and the third is the control that keeps the first two from passing vacuously: a
/// fresh input under a ceiling it does not reach parses normally, a fresh input under a ceiling it
/// *does* reach still refuses and still says so.
#[test]
fn a_fresh_input_refuses_nothing_it_was_not_asked_to_and_still_refuses_what_it_was() {
  reset_scans();
  let empty = fresh(
    Parser::with_context(
      Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
    )
    .apply(drive_fresh)
    .parse_with_state("", TokenLimiter::new()),
    scans(),
  );
  assert_eq!(
    empty,
    Fresh {
      outcome: Ok(0),
      refused: false,
      spent: 0,
      scans: 1
    },
    "an empty source is an end of input, not a refusal"
  );

  reset_scans();
  let inside = fresh(
    Parser::with_context(
      Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(64)),
    )
    .apply(drive_fresh)
    .parse_with_state("aaaa", TokenLimiter::new()),
    scans(),
  );
  assert_eq!(
    inside,
    Fresh {
      outcome: Ok(4),
      refused: false,
      spent: 4,
      scans: 5
    },
    "well inside the ceiling: four items, four charges, no refusal"
  );

  // And the ceiling still bites when it is reached, with the witness written.
  fn drain_witnessed<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>,
  ) -> Result<(usize, bool, usize), Err_> {
    let mut n = 0usize;
    while inp.next()?.is_some() {
      n += 1;
    }
    Ok((
      n,
      inp.token_budget().refused_an_item(),
      inp.token_budget().spent(),
    ))
  }

  let refused = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
  )
  .apply(drain_witnessed)
  .parse_with_state("aaaaaaaa", TokenLimiter::new())
  .expect("`next` folds the terminal stop into Ok(None)");
  assert_eq!(
    refused,
    (2, true, 2),
    "a ceiling that IS reached refuses and writes its witness — the repair removed the transplant, \
     not the refusal"
  );
}

/// **Bound 3, by cell: the witness does not survive a [`PartialSession`] redrive.**
///
/// Attempt 1 refuses under a ceiling of 2 and drains with `next`, which folds the terminal stop
/// into `Ok(None)` — so the session does **not** latch and a second attempt really runs. Attempt 2
/// is then given everything attempt 1 left a caller holding, which is the ceiling.
///
/// Falsifying output at `6aa0b08`, with `*inp.token_budget()` supplied as attempt 2's context:
/// `items=0 refused_an_item=true scans=0`. The bound was written at the accessor while it was
/// false.
#[test]
fn a_redrive_starts_its_tally_at_zero_and_lexes() {
  fn drain<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, (), Partial>,
  ) -> Result<usize, Err_> {
    while inp.next()?.is_some() {}
    assert!(inp.token_budget().refused_an_item(), "attempt 1 refused");
    Ok(inp.token_budget().limitation())
  }

  fn witness<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, (), Partial>,
  ) -> Result<(usize, bool, usize), Err_> {
    let mut n = 0usize;
    while inp.next()?.is_some() {
      n += 1;
    }
    Ok((
      n,
      inp.token_budget().refused_an_item(),
      inp.token_budget().spent(),
    ))
  }

  let mut session = PartialSession::new(TokenLimiter::new(), Budget::Unbounded, RedriveFromBase);

  let carried = session
    .parse(
      Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
      "aaaaaaaa",
      false,
      drain,
    )
    .expect("attempt 1 concludes Ok");
  assert!(!session.is_latched(), "and left the session unlatched");

  reset_scans();
  let second = session
    .parse(
      Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(carried)),
      "aaaaaaaa",
      true,
      witness,
    )
    .expect("attempt 2 concludes Ok");

  assert_eq!(
    second,
    (2, true, 2),
    "attempt 2 gets its own tally at zero, lexes its own two items against its own ceiling, and \
     writes its own refusal — it does not inherit attempt 1's"
  );
  assert_eq!(
    scans(),
    3,
    "two authorized items and the one at-limit probe: attempt 2 did the work, rather than \
     short-circuiting on a refusal it was handed"
  );
}

/// **Bound 2, by cell: the witness is input-absolute, not attempt-relative.**
///
/// A window opened *after* the first refusal reads the same `true` whether or not anything was
/// refused inside it. That is what the bound says the accessor does **not** answer, and the pair
/// of readings below is why a caller judging one `attempt` on its own terms needs a different
/// signal.
#[test]
fn the_witness_is_absolute_and_says_nothing_about_the_window_it_is_read_in() {
  fn probe<'inp>(
    inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>>,
  ) -> Result<(bool, bool, usize), Err_> {
    while inp.next()?.is_some() {}
    let after_the_refusal = inp.token_budget().refused_an_item();

    // A window that refuses nothing of its own: everything it could reach was already refused, so
    // nothing inside it is lexed and nothing inside it is charged.
    let base = inp.token_budget().spent();
    let inside = Cell::new((false, 0usize));
    let kept: Option<()> = inp.attempt(|txn| {
      while txn.next().ok().flatten().is_some() {}
      inside.set((
        txn.token_budget().refused_an_item(),
        txn.token_budget().spent(),
      ));
      None
    });
    assert!(kept.is_none(), "the window declined");

    let (witness_inside, spent_inside) = inside.get();
    Ok((after_the_refusal, witness_inside, spent_inside - base))
  }

  let (before, inside, charged_in_window) = Parser::with_context(
    Ctx::new(Verbose::new()).with_token_budget(TokenBudget::with_limitation(2)),
  )
  .apply(probe)
  .parse_with_state("aaaaaaaa", TokenLimiter::new())
  .expect("`next` folds the terminal stop into Ok(None)");

  assert!(before, "the input refused, somewhere in its life");
  assert!(
    inside,
    "and a window opened afterwards reads the same `true` — it is not a statement about the window"
  );
  assert_eq!(
    charged_in_window, 0,
    "while the window itself refused nothing and lexed nothing: the accessor is a fact about the \
     input's whole life, and it does not become attempt-relative by being read inside an attempt"
  );
}
