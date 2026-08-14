#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! Collection-driver **absence-exit** terminal-stop regressions.
//!
//! A driver's absence exits — the no-progress stall, the element-decline break, and a condition's
//! [`Action::Stop`] — conclude "there are no more elements" from what the element saw. But an
//! element's *own* lookahead ([`peek`](tokora::InputRef::peek) and friends) discards the terminal
//! flag: a resource-limit trip during the fill emits its diagnostic, latches the poison boundary,
//! and still hands the element `Ok` with a **short window**. The element then accepts consuming
//! nothing, declines, or leaves the pre-trip tokens cached for a condition to read as the end of the
//! construct — and the driver's absence exit returns `Ok`, swallowing the stop, so the caller
//! observes a successful parse of a truncated input.
//!
//! The witness is the *presence* of a latch that differs from the one the attempt started with — not
//! any positional reading, which a rollback over the latch defeats (the restore rewinds the offset
//! behind a boundary that survives). Comparing against a per-attempt snapshot is also what keeps the
//! witness attempt-relative, so a boundary an enclosing lookahead already latched is never
//! mis-charged to this driver.
//!
//! In the delimited drivers the gate sits *inside* the close probe's close-miss arms, never ahead of
//! the probe: the probe is cache-first, so a closer cached before the trip is a real, consumable
//! closer and the construct genuinely closed.
//!
//! Every assertion reads the [`is_terminal`](tokora::error::MaybeTerminal) marker through
//! [`CErr::Eot`]: the stop must surface as a **terminal-marked** end-of-input error, never as a
//! plain recoverable one and never as a clean `Ok`.

use core::cell::Cell;
use std::rc::Rc;
use tokora::EmitterView;

use generic_arraydeque::typenum::{U1, U2, U4};
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  cache::{Peeked, PeekedTokenExt},
  emitter::{
    FullContainerEmitter, SeparatedEmitter, Silent, TooFewEmitter, TooManyEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::Action,
  punct::{CloseParen, Comma, OpenParen, Paren},
  state::State,
  token::PunctuatorToken,
  try_parse_input::ParseAttempt,
};

// ── A scan limiter whose counter is shared across every cloned lexer ──────────
//
// `InputRef` rebuilds a fresh lexer per operation by cloning the state, so only an
// `Rc<Cell<_>>`-shared counter makes every scan observable and the trip sticky.

#[derive(Debug, Clone, Default)]
struct ScanLimiter {
  scanned: Rc<Cell<usize>>,
  limit: usize,
}

impl ScanLimiter {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: Rc::new(Cell::new(0)),
      limit,
    }
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }
}

#[derive(Debug, Clone, PartialEq)]
struct ScanLimitExceeded;

impl State for ScanLimiter {
  type Error = ScanLimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.scanned.get() > self.limit {
      Err(ScanLimitExceeded)
    } else {
      Ok(())
    }
  }
}

// ── Token vocabulary: numbers, commas and parens, every scan counted ─────────

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, extras = ScanLimiter, skip r"[ \t\r\n]+")]
enum Tok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); lex.slice().parse::<i64>().unwrap_or(0) })]
  Num(i64),
  #[token(",", |lex| { lex.extras.increase(); })]
  Comma,
  #[token("(", |lex| { lex.extras.increase(); })]
  LParen,
  #[token(")", |lex| { lex.extras.increase(); })]
  RParen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
  Num,
  Comma,
  LParen,
  RParen,
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

impl core::fmt::Display for Kind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Kind::Num => "number",
      Kind::Comma => ",",
      Kind::LParen => "(",
      Kind::RParen => ")",
    })
  }
}

impl TokenTrait<'_> for Tok {
  type Kind = Kind;
  type Error = CErr;

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  fn kind(&self) -> Kind {
    match self {
      Tok::Num(_) => Kind::Num,
      Tok::Comma => Kind::Comma,
      Tok::LParen => Kind::LParen,
      Tok::RParen => Kind::RParen,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl PunctuatorToken<'_> for Tok {
  fn comma() -> Option<Self::Kind> {
    Some(Kind::Comma)
  }

  fn open_paren() -> Option<Self::Kind> {
    Some(Kind::LParen)
  }

  fn close_paren() -> Option<Self::Kind> {
    Some(Kind::RParen)
  }
}

impl From<Comma<(), (), ()>> for Kind {
  fn from(_: Comma<(), (), ()>) -> Self {
    Kind::Comma
  }
}

impl From<OpenParen<(), (), ()>> for Kind {
  fn from(_: OpenParen<(), (), ()>) -> Self {
    Kind::LParen
  }
}

impl From<CloseParen<(), (), ()>> for Kind {
  fn from(_: CloseParen<(), (), ()>) -> Self {
    Kind::RParen
  }
}

// ── The fixture error: the `Eot` variant carries the terminal marker ──────────

#[derive(Debug, Clone, PartialEq)]
enum CErr {
  /// An ordinary (recoverable) parse/lexer error — the catch-all for the emitter families.
  Ordinary,
  /// The resource-limit trip's own diagnostic.
  Limit,
  /// The committed form's end-of-input error, carrying its terminal marker — a swallowed stop
  /// surfaces nothing at all, a mis-built one surfaces `Eot(false)`, and only a correctly surfaced
  /// terminal stop is `Eot(true)`.
  Eot(bool),
}

impl From<()> for CErr {
  fn from(_: ()) -> Self {
    CErr::Ordinary
  }
}

impl From<ScanLimitExceeded> for CErr {
  fn from(_: ScanLimitExceeded) -> Self {
    CErr::Limit
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for CErr {
  fn from(e: UnexpectedEot<O, Lang>) -> Self {
    CErr::Eot(e.is_terminal())
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CErr {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for CErr {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for CErr {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for CErr {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for CErr {
  fn from(_: FullContainer<S, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for CErr {
  fn from(_: TooFew<S, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for CErr {
  fn from(_: TooMany<S, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unclosed<Delimiter, S, Lang>> for CErr {
  fn from(_: Unclosed<Delimiter, S, Lang>) -> Self {
    CErr::Ordinary
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CErr
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<Delimiter>(_: Unclosed<Delimiter, L::Span, Lang>) -> Self {
    CErr::Ordinary
  }
}

// ── Harness ──────────────────────────────────────────────────────────────────

type TLexer<'a> = LogosLexer<'a, Tok>;

/// Drive a parser closure over `input` with the scan `limiter` as the initial lexer state, under a
/// recovering emitter: the trip's own diagnostic is accepted, so the *only* way the stop can reach
/// the caller is as a returned terminal error.
fn drive<'inp, O>(
  limiter: ScanLimiter,
  f: impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<CErr>>>,
  ) -> Result<O, CErr>,
  input: &'inp str,
) -> Result<O, CErr> {
  let ctx: ParserContext<'inp, TLexer<'inp>, Silent<CErr>> = ParserContext::new(Silent::new());
  Parser::with_parser_and_context(f, ctx).parse_str_with_state(input, limiter)
}

/// The bounds every fixture's emitter must satisfy — spelled once so the fixtures stay readable.
trait FixtureEmitter<'inp>:
  Emitter<'inp, TLexer<'inp>, Error = CErr>
  + FullContainerEmitter<'inp, TLexer<'inp>>
  + SeparatedEmitter<'inp, TLexer<'inp>>
  + UnclosedEmitter<'inp, TLexer<'inp>>
  + UnexpectedLeadingSeparatorEmitter<'inp, TLexer<'inp>>
  + UnexpectedTrailingSeparatorEmitter<'inp, TLexer<'inp>>
  + TooFewEmitter<'inp, TLexer<'inp>>
  + TooManyEmitter<'inp, TLexer<'inp>>
{
}

impl<'inp, E> FixtureEmitter<'inp> for E where
  E: Emitter<'inp, TLexer<'inp>, Error = CErr>
    + FullContainerEmitter<'inp, TLexer<'inp>>
    + SeparatedEmitter<'inp, TLexer<'inp>>
    + UnclosedEmitter<'inp, TLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TLexer<'inp>>
    + TooFewEmitter<'inp, TLexer<'inp>>
    + TooManyEmitter<'inp, TLexer<'inp>>
{
}

// ── The elements: each latches a terminal stop through its OWN lookahead ──────
//
// `peek::<U2>` asks for two tokens. Arranged so the second slot is the scan that trips, the fill
// emits the trip's diagnostic, latches the poison boundary, and still returns `Ok` with a one-token
// window — no signal the element could consult even if it wanted to.

/// Try-shape element: peek-latches, then **accepts consuming nothing** (the no-progress exit).
fn peek_latch_accept<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  let _ = inp.peek::<U2>()?;
  Ok(ParseAttempt::Accept(0))
}

/// Try-shape element: peek-latches, then **declines** (the element-decline exit).
fn peek_latch_decline<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  let _ = inp.peek::<U2>()?;
  Ok(ParseAttempt::Decline)
}

/// Plain-shape element (for the `*_while` drivers, whose element cannot decline): peek-latches,
/// then succeeds consuming nothing.
fn peek_latch_plain<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  let _ = inp.peek::<U2>()?;
  Ok(0)
}

/// Try-shape element: peek-latches, then opens an `attempt` that drains the cached pre-trip tokens
/// and declines, so the restore rewinds the cursor and the cache but *not* the latch — the
/// checkpoint saved it after the trip had already taken it. The element then declines.
fn peek_latch_rollback_decline<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  let _ = inp.peek::<U4>()?;
  let _: Option<()> = inp.attempt(|t| {
    while matches!(t.next(), Ok(Some(_))) {}
    None
  });
  Ok(ParseAttempt::Decline)
}

/// Try-shape element: opens an `attempt` of its **own**, drains through the committed leaf inside
/// it — which is where the scanner trips — catches that stop, and declines the inner attempt. The
/// restore then puts the poison boundary back to the value the element started with, so nothing
/// about the latch differs from the driver's per-collection snapshot. The element then declines.
///
/// One level deeper than [`peek_latch_rollback_decline`], and that one level is the whole
/// difference: there the trip happens *before* the inner checkpoint is taken, so the checkpoint
/// saved an already-latched boundary and the restore hands it straight back. Here the checkpoint is
/// taken **first** and the trip happens inside it, so the restore erases the boundary outright.
/// Every reading of the latch — positional or presence-plus-change, at any single nesting depth —
/// is clean afterwards, over a stop that is live, already diagnosed, and re-trips on the next scan
/// of the same prefix. Only a cell no rollback reaches sees it.
fn trip_inside_attempt_decline<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  let _: Option<()> = inp.attempt(|t| {
    loop {
      match t.next_or_stop() {
        Ok(Some(_)) => continue,
        // Both exits decline the inner attempt, so the rollback is identical either way and the
        // scan limit is the only thing that decides whether a stop happened inside it. That is
        // what makes the widened control below a control.
        Ok(None) | Err(_) => return None,
      }
    }
  });
  Ok(ParseAttempt::Decline)
}

/// Try-shape element: consumes one `Num`, then peeks a `U2` window (which may trip); declines on
/// anything that is not a `Num`, leaving that token cached and unconsumed.
fn num_then_peek<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
    Some(tok) => {
      let (_span, tok) = tok.into_components();
      let n = match tok {
        Tok::Num(n) => n,
        _ => return Err(CErr::Ordinary),
      };
      let _ = inp.peek::<U2>()?;
      Ok(ParseAttempt::Accept(n))
    }
    None => Ok(ParseAttempt::Decline),
  }
}

/// Plain-shape element (for the `*_while` drivers): consumes one `Num`, then peeks a `U2` window
/// (which may trip), leaving the scanned pre-trip token cached for the driver's next decision
/// window. The condition only ever routes a `Num` here, so anything else is an outright error.
fn num_then_peek_plain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<i64, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  let Some(tok) = inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? else {
    return Err(CErr::Ordinary);
  };
  let (_span, tok) = tok.into_components();
  let n = match tok {
    Tok::Num(n) => n,
    _ => return Err(CErr::Ordinary),
  };
  let _ = inp.peek::<U2>()?;
  Ok(n)
}

/// Try-shape element that only ever accepts a `Comma` and declines on anything else, consuming
/// nothing when it declines — used to reach an absence exit while replaying cached tokens.
fn comma_or_decline<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  match inp.try_expect(|t| matches!(t.data, Tok::Comma))? {
    Some(_) => Ok(ParseAttempt::Accept(0)),
    None => Ok(ParseAttempt::Decline),
  }
}

/// Decision over a `U1` window: continue while the front token is a `Num`. The window is narrow on
/// purpose — the driver's own decision peek must NOT be the scan that trips, or the eager decision
/// gate would surface the stop and the element's latch would go untested.
fn while_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, TLexer<'inp>, Ctx::Emitter>,
) -> Result<Action, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  Ok(match peeked.pop_front() {
    Some(tok) if matches!(tok.token(), Tok::Num(_)) => Action::Continue,
    _ => Action::Stop,
  })
}

/// The one assertion every test makes.
#[track_caller]
fn assert_terminal<O: core::fmt::Debug + PartialEq>(out: Result<O, CErr>, what: &str) {
  assert_eq!(
    out,
    Err(CErr::Eot(true)),
    "{what}: an absence exit reached on a peek-latched terminal stop must surface the \
     terminal-marked end-of-input error, not conclude the construct ended; got {out:?}"
  );
}

// ── `Repeated` ────────────────────────────────────────────────────────────────
//
// `1 2` under a limit of 1: the element's own `peek::<U2>` scans `1` cleanly (1st scan) and trips on
// `2` (2nd scan).

#[test]
fn repeated_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept.repeated().collect().parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "repeated / zero-width accept");
}

#[test]
fn repeated_decline_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_decline.repeated().collect().parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "repeated / decline");
}

// ── `RepeatedWhile` ───────────────────────────────────────────────────────────
//
// The `U1` decision peek scans `1` cleanly (1st scan) and decides `Continue`; the element's
// `peek::<U2>` then trips on `2` (2nd scan). The element cannot decline, so the no-progress stall is
// its only absence exit.

#[test]
fn repeated_while_zero_width_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "repeated_while / zero-width continue");
}

// ── `DelimitedBy<Repeated>` ───────────────────────────────────────────────────
//
// `(1 2` under a limit of 2: the opener scans (1st), the element's `peek::<U2>` scans `1` (2nd) and
// trips on `2` (3rd). The absence exit runs the close-probe epilogue, which would classify the
// cached `1` as a spurious wrong closer — so the stop must be surfaced ahead of the probe.

#[test]
fn delim_repeated_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept
      .repeated()
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "(1 2");
  assert_terminal(out, "delimited repeated / zero-width accept");
}

#[test]
fn delim_repeated_decline_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_decline
      .repeated()
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "(1 2");
  assert_terminal(out, "delimited repeated / decline");
}

// ── `DelimitedBy<RepeatedWhile>` ──────────────────────────────────────────────

#[test]
fn delim_repeated_while_zero_width_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "(1 2");
  assert_terminal(out, "delimited repeated_while / zero-width continue");
}

// ── `Separated` ───────────────────────────────────────────────────────────────
//
// The separator slot probes `1`, finds no comma and pushes it back (1st scan); the element's
// `peek::<U2>` then trips on `2` (2nd scan).

#[test]
fn separated_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept
      .separated_by_comma()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "separated / zero-width accept");
}

#[test]
fn separated_decline_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_decline
      .separated_by_comma()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "separated / decline");
}

// ── `DelimitedBy<Separated>` ──────────────────────────────────────────────────

#[test]
fn delim_separated_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept
      .separated_by_comma()
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "(1 2");
  assert_terminal(out, "delimited separated / zero-width accept");
}

#[test]
fn delim_separated_decline_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_decline
      .separated_by_comma()
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "(1 2");
  assert_terminal(out, "delimited separated / decline");
}

// ── `SeparatedWhile` and its delimited variant ────────────────────────────────

#[test]
fn separated_while_zero_width_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "separated_while / zero-width continue");
}

#[test]
fn delim_separated_while_zero_width_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "(1 2");
  assert_terminal(out, "delimited separated_while / zero-width continue");
}

// ── The fold family: try-wing, while-wing, and the buffering wings ────────────

#[test]
fn fold_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept
      .fold(|| 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "fold / zero-width accept");
}

#[test]
fn fold_decline_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_decline
      .fold(|| 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "fold / decline");
}

#[test]
fn try_fold_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept
      .try_fold(|| 0i64, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "try_fold / zero-width accept");
}

#[test]
fn try_fold_with_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept
      .try_fold_with(|| 0i64, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "try_fold_with / zero-width accept");
}

#[test]
fn try_fold_with_decline_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_decline
      .try_fold_with(|| 0i64, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "try_fold_with / decline");
}

#[test]
fn rfold_accept_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_accept
      .rfold(|| 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "rfold / zero-width accept");
}

#[test]
fn rfold_decline_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_decline
      .rfold(|| 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "rfold / decline");
}

#[test]
fn fold_while_zero_width_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_plain
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "fold_while / zero-width continue");
}

#[test]
fn rfold_while_zero_width_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_plain
      .rfold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(out, "rfold_while / zero-width continue");
}

// ── The witness is presence-plus-change, not positional ───────────────────────

#[test]
fn rollback_after_a_peek_latch_still_surfaces_terminal() {
  // `1 2 3` under a limit of 2: the element's `peek::<U4>` caches `1 2` and trips on `3`, latching
  // the boundary at the end of `2`. The element then opens an `attempt`, drains both cached tokens,
  // and declines — so the restore rewinds the cursor, the cache and the committed watermark, while
  // the latch survives (the checkpoint saved it *after* the trip took it). The lex offset is now back
  // BEHIND the boundary, so a positional witness reads clean; the stop is nonetheless live and its
  // diagnostic already emitted. Only a presence-plus-change witness sees it.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    peek_latch_rollback_decline
      .repeated()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 2 3");
  assert_terminal(out, "repeated / decline after rolling back over the latch");
}

// ── …and no rollback at any depth can erase it, because the witness is a counter ─
//
// The level below the cell above, and the one a latch comparison cannot reach. There the trip is
// taken *before* the element's inner checkpoint, so the checkpoint saves an already-latched boundary
// and the restore hands it back; the latch survives and the presence-plus-change reading sees it.
// Here the checkpoint is taken **first** and the trip happens inside it, so the restore puts the
// boundary back to what the driver itself snapshotted — and the absence gate reads clean over a
// spent scanner budget, concluding the construct ended.
//
// Relocating a latch read closes one depth and opens the next. A cell inside the rollback set cannot
// witness an event across a rollback at any depth, so the witness is `Input::scanner_trips`: a
// monotone session counter bumped by the crate's sole terminal predicate, outside the rollback set
// by construction. This is the same answer `parser::recovery_gate` reached for the recovery
// combinators, applied to the twelve collection drivers.

#[test]
fn a_trip_caught_inside_an_elements_own_attempt_still_surfaces_terminal() {
  // `1 2` under a limit of 1: the element's inner `attempt` drains `1` (1st scan) and trips on `2`
  // (2nd scan) — inside the attempt — then declines it. The restore rewinds the cursor, the cache,
  // the emissions AND the poison boundary; the element declines, and `repeated`'s decline exit is
  // the absence conclusion the gate has to refuse.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    trip_inside_attempt_decline
      .repeated()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(1), parse, "1 2");
  assert_terminal(
    out,
    "repeated / trip caught inside the element's own attempt",
  );

  // Non-vacuity: the identical element and the identical rollback, with only the scan limit
  // widened. Nothing trips, the decline is an ordinary absence, and the construct ends cleanly — so
  // the cell above measures the stop rather than the shape.
  let roomy = drive(ScanLimiter::with_limit(1_000), parse, "1 2");
  assert_eq!(
    roomy,
    Ok(vec![]),
    "the control differs from the run above in one thing only — with room, nothing trips and the \
     same declining element ends the collection cleanly; got {roomy:?}"
  );
}

// ── A cached closer beyond the latch is a real closer: the list still closes ───

#[test]
fn delim_repeated_closes_when_the_closer_is_cached_before_the_trip() {
  // `(1 2) 9` under a limit of 4: the opener and `1` scan (1st, 2nd), then the first element's
  // `peek::<U2>` caches `2` and `)` (3rd, 4th). The second element consumes `2` and its own
  // `peek::<U2>` trips on `9` (5th), latching just PAST the cached `)`. The third element sees `)`,
  // declines, and the close probe — cache-first by contract — finds that real, consumable `)`.
  //
  // The construct genuinely closed, so it must parse: a terminal stop latched beyond a durable
  // cached closer is not evidence the list was truncated. Guarding ahead of the probe instead of
  // inside its close-miss arms would fail this legitimate parse.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek
      .repeated()
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(4), parse, "(1 2) 9");
  assert_eq!(
    out,
    Ok(vec![1, 2]),
    "a closer cached before the trip is a real closer — the delimited list must still close and \
     collect both elements; got {out:?}"
  );
}

// ── Any absence conclusion under a live this-attempt latch errors ──────────────

#[test]
fn decline_on_solid_evidence_under_a_live_latch_surfaces_terminal() {
  // `1 2 , 9` under a limit of 3: the first element consumes `1` and its `peek::<U4>` caches `2` and
  // `,` before tripping on `9`, latching at the end of `,`. The second element consumes the cached
  // `2`. The third sees the cached `,` — solid, single-token, pre-trip evidence — and declines.
  //
  // The absence conclusion is surfaced as terminal anyway. This is a deliberate over-approximation:
  // a driver cannot introspect which evidence its element used, so "no more elements" reached while
  // a stop this attempt latched is live is never taken at face value.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek.repeated().collect().parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(3), parse, "1 2 , 9");
  assert_terminal(
    out,
    "repeated / decline on solid evidence under a live latch",
  );
}

// ── An inherited latch is never charged to this driver ────────────────────────

#[test]
fn an_inherited_latch_does_not_fail_a_legitimate_absence_exit() {
  // An enclosing lookahead caches `1 2` and trips on `3`, latching the boundary BEFORE the list
  // starts. The list's element wants a comma, sees the cached `1`, and declines without consuming —
  // an ordinary, legitimate absence while only replaying pre-trip tokens.
  //
  // The witness compares against the snapshot taken at the driver's entry, so the inherited boundary
  // is equal and the exit stays clean. Charging it here is the misattribution the committed-cursor
  // element gate already had to rule out once.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    let _ = inp.peek_with_emitter::<U4>()?;
    comma_or_decline.repeated().collect().parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 2 3");
  assert_eq!(
    out,
    Ok(vec![]),
    "a boundary an enclosing lookahead latched before the driver started must not be charged to \
     this driver's absence exit; got {out:?}"
  );
}

// ── A condition's `Action::Stop` is an absence exit too ────────────────────────
//
// The eager decision gate only fires when the driver's OWN decision peek came back short with the
// terminal flag set. It cannot see this shape: the element consumes a token, then its own
// `peek::<U2>` trips one token further on and latches the boundary, leaving the pre-trip token
// **cached**. The next decision peek is served whole from that cache — a FULL window, no fill, no
// terminal flag — and the condition reads the cached boundary token as "the construct ended". That
// conclusion is *absence*, exactly like the no-progress stall, so it carries the same witness.
//
// `1 , 9` under a limit of 2: the `U1` decision peek scans `1` (1st), the element consumes it, and
// its `peek::<U2>` caches `,` (2nd) before tripping on `9` (3rd). The next window is the cached `,`,
// on which `while_num` stops.

#[test]
fn repeated_while_stop_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 , 9");
  assert_terminal(out, "repeated_while / condition stop");
}

#[test]
fn fold_while_stop_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 , 9");
  assert_terminal(out, "fold_while / condition stop");
}

#[test]
fn try_fold_while_stop_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .try_fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| Ok(acc + x))
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 , 9");
  assert_terminal(out, "try_fold_while / condition stop");
}

#[test]
fn try_fold_while_with_stop_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .try_fold_while_with::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x, _state| Ok(acc + x))
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 , 9");
  assert_terminal(out, "try_fold_while_with / condition stop");
}

#[test]
fn rfold_while_stop_on_a_peek_latched_stop_surfaces_terminal() {
  // The buffering wing: the stop breaks out with the outputs still in the buffer, and the reverse
  // fold runs on the single success path afterwards — so the witness has to be consulted before that
  // fold produces a value the caller would read as a complete parse.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .rfold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 , 9");
  assert_terminal(out, "rfold_while / condition stop");
}

// `1 ) 9` for the separated wing: the separator slot would consume a cached `,` before the condition
// ever saw it, so the cached pre-trip token has to be one that is neither a separator nor an element.

#[test]
fn separated_while_stop_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, "1 ) 9");
  assert_terminal(out, "separated_while / condition stop");
}

// The delimited wings reach the stop with a close probe already in hand: `(1 , 9` under a limit of 3
// scans the opener (1st) and `1` (2nd), the element consumes `1` and its `peek::<U2>` caches `,`
// (3rd) before tripping on `9` (4th). The probe then reads the cached `,` as a wrong closer and the
// condition stops — so the close-miss diagnostic, not a genuine close, is what the gate must precede.

#[test]
fn delim_repeated_while_stop_on_a_peek_latched_stop_surfaces_terminal() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(3), parse, "(1 , 9");
  assert_terminal(out, "delimited repeated_while / condition stop");
}

#[test]
fn delim_separated_while_stop_on_a_peek_latched_stop_surfaces_terminal() {
  // `(1 ( 9`, not `(1 , 9`: this driver's loop head accepts a separator *or* the closer, so a cached
  // `,` would be eaten as a separator before the condition ran.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(3), parse, "(1 ( 9");
  assert_terminal(out, "delimited separated_while / condition stop");
}

// ── An ordinary stop, with nothing latched, still ends the construct cleanly ────
//
// The same shapes under a limit no scan reaches: the element consumes `1`, its `peek::<U2>` caches
// the boundary token and hits a plain end of input, and the condition stops on it. Nothing is
// latched, so every one of these must parse.

#[test]
fn repeated_while_stop_without_a_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(100), parse, "1 ,");
  assert_eq!(out, Ok(vec![1]), "repeated_while / plain stop; got {out:?}");
}

#[test]
fn fold_while_stop_without_a_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(100), parse, "1 ,");
  assert_eq!(out, Ok(1), "fold_while / plain stop; got {out:?}");
}

#[test]
fn rfold_while_stop_without_a_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .rfold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(100), parse, "1 2 ,");
  assert_eq!(out, Ok(3), "rfold_while / plain stop; got {out:?}");
}

#[test]
fn separated_while_stop_without_a_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(100), parse, "1 )");
  assert_eq!(
    out,
    Ok(vec![1]),
    "separated_while / plain stop; got {out:?}"
  );
}

#[test]
fn delim_repeated_while_stop_without_a_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(100), parse, "(1 ,");
  assert_eq!(
    out,
    Ok(vec![1]),
    "delimited repeated_while / plain stop on a wrong closer; got {out:?}"
  );
}

#[test]
fn delim_separated_while_stop_without_a_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(100), parse, "(1 (");
  assert_eq!(
    out,
    Ok(vec![1]),
    "delimited separated_while / plain stop on a wrong closer; got {out:?}"
  );
}

// ── A real cached closer still closes the construct ────────────────────────────
//
// `(1 ) 9` under a limit of 3: the opener scans (1st) and the probe scans `1` (2nd); the element
// consumes `1` and its `peek::<U2>` caches `)` (3rd) before tripping on `9` (4th), latching PAST that
// closer. The closer is a real, consumable, pre-trip token, so the construct genuinely closed —
// a positive exit, which the absence witness must not touch.

#[test]
fn delim_repeated_while_closes_when_the_closer_is_cached_before_the_trip() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(3), parse, "(1 ) 9");
  assert_eq!(
    out,
    Ok(vec![1]),
    "a closer cached before the trip is a real closer — the delimited list must still close; got \
     {out:?}"
  );
}

#[test]
fn delim_separated_while_closes_when_the_closer_is_cached_before_the_trip() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    num_then_peek_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(3), parse, "(1 ) 9");
  assert_eq!(
    out,
    Ok(vec![1]),
    "a closer cached before the trip is a real closer — the delimited list must still close; got \
     {out:?}"
  );
}

// ── An inherited latch is not charged to a condition's stop either ─────────────
//
// An enclosing lookahead latches the boundary BEFORE the driver starts, and the driver's condition
// then stops on a cached pre-trip token without any element having latched anything. The witness
// compares against the snapshot taken at the driver's entry, so the inherited boundary is equal and
// the stop stays clean.

#[test]
fn repeated_while_stop_under_an_inherited_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    let _ = inp.peek_with_emitter::<U4>()?;
    num_then_peek_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, ", 9 9");
  assert_eq!(
    out,
    Ok(vec![]),
    "repeated_while / inherited latch at a condition stop; got {out:?}"
  );
}

#[test]
fn fold_while_stop_under_an_inherited_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<i64, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    let _ = inp.peek_with_emitter::<U4>()?;
    num_then_peek_plain
      .fold_while::<_, _, _, U1>(while_num::<Ctx>, || 0i64, |acc, x| acc + x)
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, ", 9 9");
  assert_eq!(
    out,
    Ok(0),
    "fold_while / inherited latch at a condition stop; got {out:?}"
  );
}

#[test]
fn separated_while_stop_under_an_inherited_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    let _ = inp.peek_with_emitter::<U4>()?;
    num_then_peek_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(2), parse, ") 9 9");
  assert_eq!(
    out,
    Ok(vec![]),
    "separated_while / inherited latch at a condition stop; got {out:?}"
  );
}

#[test]
fn delim_repeated_while_stop_under_an_inherited_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    let _ = inp.peek_with_emitter::<U4>()?;
    num_then_peek_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(3), parse, "(, 9 9");
  assert_eq!(
    out,
    Ok(vec![]),
    "delimited repeated_while / inherited latch at a condition stop; got {out:?}"
  );
}

#[test]
fn delim_separated_while_stop_under_an_inherited_latch_stays_ok() {
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: FixtureEmitter<'inp>,
  {
    let _ = inp.peek_with_emitter::<U4>()?;
    num_then_peek_plain
      .separated_by_comma_while::<_, U1>(while_num::<Ctx>)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let out = drive(ScanLimiter::with_limit(3), parse, "(( 9 9");
  assert_eq!(
    out,
    Ok(vec![]),
    "delimited separated_while / inherited latch at a condition stop; got {out:?}"
  );
}
