#![cfg(all(feature = "std", feature = "logos"))]

//! Collection-driver terminal-stop regressions (R1 D20, attempt-relative round).
//!
//! Three driver gates must tell a **terminal scanner stop** (a resource-limit trip and the poison
//! boundary it latches) apart from ordinary control flow:
//!
//! * **R1 — the element gate is attempt-relative.** A trip latches the boundary at the cursor it
//!   scans from, and an element that reaches the poison consumes up to it. The gate therefore
//!   witnesses a terminal element failure by the **committed cursor**
//!   ([`InputRef::at_committed_boundary`]), *not* the lex offset: a boundary a prior lookahead
//!   latched ahead of the cursor (a prefilled cache) must not mis-charge an element that failed
//!   *ordinarily* while still short of it. RED with the old lex-offset witness (it re-raised the
//!   ordinary failure instead of recovering it).
//! * **R2 — the separator-slot gate surfaces a trip** instead of folding it into a clean end.
//! * **R3 — the while-list decision peek surfaces a mid-window trip** instead of reading its short
//!   window as a clean stop.

use core::cell::Cell;
use std::rc::Rc;

use generic_arraydeque::typenum::{U1, U2, U4};
use tokora::{
  Accumulator, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext,
  Token as TokenTrait, TryParseInput,
  cache::{Peeked, PeekedTokenExt},
  emitter::{
    FullContainerEmitter, SeparatedEmitter, Silent, TooFewEmitter, TooManyEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter, Verbose,
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

// ── Token vocabulary: numbers and commas, every scan counted ─────────────────

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

// The `Comma` separator and `Paren` delimiter punctuators classify against this token stream by
// kind.
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

// ── The fixture error: distinguishes the channels the assertions read ─────────

#[derive(Debug, Clone, PartialEq)]
enum CErr {
  /// An ordinary (recoverable) parse/lexer error — the catch-all for the emitter families.
  Ordinary,
  /// The resource-limit trip's own diagnostic.
  Limit,
  /// The committed form's end-of-input error — what a terminal stop must surface.
  Eot,
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
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    CErr::Eot
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

// ── Harness ──────────────────────────────────────────────────────────────────

type TLexer<'a> = LogosLexer<'a, Tok>;

/// Drive a parser closure over `input` with the scan `limiter` as the initial lexer state.
fn drive<'inp, O, Em>(
  emitter: Em,
  limiter: ScanLimiter,
  f: impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Em>>,
  ) -> Result<O, CErr>,
  input: &'inp str,
) -> Result<O, CErr>
where
  Em: Emitter<'inp, TLexer<'inp>, Error = CErr>
    + FullContainerEmitter<'inp, TLexer<'inp>>
    + SeparatedEmitter<'inp, TLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TLexer<'inp>>
    + TooFewEmitter<'inp, TLexer<'inp>>
    + TooManyEmitter<'inp, TLexer<'inp>>,
{
  let ctx: ParserContext<'inp, TLexer<'inp>, Em> = ParserContext::new(emitter);
  Parser::with_parser_and_context(f, ctx).parse_str_with_state(input, limiter)
}

/// Counts the ordinary diagnostics a verbose emitter collected.
fn ordinary_diags(emitter: &Verbose<CErr>) -> usize {
  emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == CErr::Ordinary)
    .count()
}

/// Element (try-shape): accept a `Num`, decline is not used — a non-`Num` is an ordinary error.
fn num_element<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  match inp.next()? {
    None => Ok(ParseAttempt::Decline),
    Some(tok) => match tok.into_data() {
      Tok::Num(n) => Ok(ParseAttempt::Accept(n)),
      _ => Err(CErr::Ordinary),
    },
  }
}

/// Element (plain `ParseInput`) for the while-list driver.
fn num_element_plain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<i64, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      Tok::Num(n) => Ok(n),
      _ => Err(CErr::Ordinary),
    },
    None => Err(CErr::Ordinary),
  }
}

/// An element that expects a `Comma` and fails *ordinarily* (without consuming) on anything else —
/// used only by R1 to fail against a prefilled cache of `Num`s.
fn comma_element<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<()>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  match inp.try_expect(|t| matches!(t.data, Tok::Comma))? {
    Some(_) => Ok(ParseAttempt::Accept(())),
    None => Err(CErr::Ordinary),
  }
}

/// While-list decision: continue while the next token is a `Num`.
fn while_num<'inp, Ctx>(
  mut peeked: Peeked<'_, 'inp, TLexer<'inp>, U1>,
  _: &mut Ctx::Emitter,
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

// ── R1: the element gate is attempt-relative (committed cursor, not lex offset) ─

#[test]
fn r1_prior_poison_does_not_recharge_an_ordinary_element_failure() {
  // A prior lookahead peeks a wide window: it caches `1 2` and, on the third scan, trips and
  // latches the poison boundary — leaving the *lex offset* at the boundary while the *committed
  // cursor* stays at the front. A `repeated` whose element then fails ORDINARILY (it wants a comma,
  // the cache front is `1`) must recover it (emit-and-continue), NOT re-raise it as terminal.
  //
  // RED with the old lex-offset witness (`at_latched_boundary`): the offset sat at the boundary, so
  // the ordinary failure was wrongly re-raised. GREEN with the committed-cursor witness
  // (`at_committed_boundary`): the cursor is short of the boundary, so the failure recovers.
  fn r1_parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<()>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TLexer<'inp>, Error = CErr> + FullContainerEmitter<'inp, TLexer<'inp>>,
  {
    // Prefill + latch: a wide peek scans `1`(1), `2`(2), then the third scan trips.
    let _ = inp.peek_with_emitter::<U4>()?;
    comma_element.repeated().collect().parse_input(inp)
  }

  let mut verbose = Verbose::<CErr>::new();
  let out = drive(&mut verbose, ScanLimiter::with_limit(2), r1_parse, "1 2 3");
  assert!(
    out.is_ok(),
    "an ordinary element failure with the committed cursor short of a prior-latched boundary must \
     recover, not re-raise as terminal; got {out:?}"
  );
  assert!(
    ordinary_diags(&verbose) >= 1,
    "the ordinary failure was emitted-and-continued (recovered), not re-raised"
  );
}

// ── R2: the separator-slot decision gate surfaces a trip ──────────────────────

#[test]
fn r2_separator_slot_trip_surfaces_terminal() {
  // `1 , 2 ,` under a limit of 3: after `1 , 2` the separator-slot peek scans a further `,` that
  // trips. The gate must surface the terminal stop, not fold it into `Ok(None)` and end the list.
  fn r2_parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>
      + SeparatedEmitter<'inp, TLexer<'inp>>
      + FullContainerEmitter<'inp, TLexer<'inp>>
      + UnexpectedLeadingSeparatorEmitter<'inp, TLexer<'inp>>
      + UnexpectedTrailingSeparatorEmitter<'inp, TLexer<'inp>>,
  {
    num_element.separated_by_comma().collect().parse_input(inp)
  }

  let out = drive(
    Silent::<CErr>::new(),
    ScanLimiter::with_limit(3),
    r2_parse,
    "1 , 2 ,",
  );
  assert_eq!(
    out,
    Err(CErr::Eot),
    "the separator-slot trip surfaces the committed end-of-input error, got {out:?}"
  );
}

// ── R3: the while-list decision peek surfaces a mid-window trip ────────────────

#[test]
fn r3_while_decision_trip_surfaces_terminal() {
  // `1 2 3` under a limit of 2: after `1 2`, the `repeated_while` decision peek scans `3`, which
  // trips. Its short window reads as `Action::Stop`; the driver must surface the terminal stop
  // instead of ending the list cleanly. Driven through the public `list_of` constructor.
  fn r3_parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TLexer<'inp>, Error = CErr> + FullContainerEmitter<'inp, TLexer<'inp>>,
  {
    num_element_plain
      .repeated_while::<_, U1>(while_num::<Ctx>)
      .collect()
      .parse_input(inp)
  }

  let out = drive(
    Silent::<CErr>::new(),
    ScanLimiter::with_limit(2),
    r3_parse,
    "1 2 3",
  );
  assert_eq!(
    out,
    Err(CErr::Eot),
    "the mid-window while-list trip surfaces the committed end-of-input error, got {out:?}"
  );
}

// ── The while-list decision gate is eager: the terminal flag outranks a fallible `decide` and a
//    zero-width `Continue` that would otherwise swallow the stop through the no-progress guard ──

#[test]
fn while_condition_error_does_not_preempt_a_terminal_stop() {
  // `1 2` under a limit of 1, window `U2`: the front `1` scans cleanly, the second window slot trips
  // — a one-token window with `terminal == true`. The condition treats a truncated (`len < W`)
  // window as untrustworthy and returns an ordinary error. That fallible `decide` runs before the
  // arm-level terminal check, so only an eager gate surfaces the stop ahead of it. RED today: the
  // ordinary error preempts the terminal stop.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<CErr>>>,
  ) -> Result<Vec<i64>, CErr> {
    let cond = |mut peeked: Peeked<'_, 'inp, TLexer<'inp>, U2>,
                _: &mut Silent<CErr>|
     -> Result<Action, CErr> {
      if peeked.len() < 2 {
        return Err(CErr::Ordinary);
      }
      Ok(match peeked.pop_front() {
        Some(tok) if matches!(tok.token(), Tok::Num(_)) => Action::Continue,
        _ => Action::Stop,
      })
    };
    let elem = |_inp: &mut InputRef<
      'inp,
      '_,
      TLexer<'inp>,
      ParserContext<'inp, TLexer<'inp>, Silent<CErr>>,
    >|
     -> Result<i64, CErr> { Ok(0) };
    elem
      .repeated_while::<_, U2>(cond)
      .collect()
      .parse_input(inp)
  }

  let ctx: ParserContext<'_, TLexer<'_>, Silent<CErr>> = ParserContext::new(Silent::new());
  let out = Parser::with_parser_and_context(parse, ctx)
    .parse_str_with_state("1 2", ScanLimiter::with_limit(1));
  assert_eq!(
    out,
    Err(CErr::Eot),
    "a fallible condition on a trip-truncated window must not preempt the terminal stop, got {out:?}"
  );
}

#[test]
fn while_zero_width_continue_does_not_swallow_a_terminal_stop() {
  // `1 2` under a limit of 1, window `U2`: the second window slot trips — a one-token window with
  // `terminal == true`. The condition always continues, and the element consumes nothing, so the
  // no-progress guard fires and ends the list cleanly — swallowing the stop. RED today: `Ok`, not
  // the terminal end-of-input error.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<CErr>>>,
  ) -> Result<Vec<i64>, CErr> {
    let cond = |_peeked: Peeked<'_, 'inp, TLexer<'inp>, U2>,
                _: &mut Silent<CErr>|
     -> Result<Action, CErr> { Ok(Action::Continue) };
    let elem = |_inp: &mut InputRef<
      'inp,
      '_,
      TLexer<'inp>,
      ParserContext<'inp, TLexer<'inp>, Silent<CErr>>,
    >|
     -> Result<i64, CErr> { Ok(0) };
    elem
      .repeated_while::<_, U2>(cond)
      .collect()
      .parse_input(inp)
  }

  let ctx: ParserContext<'_, TLexer<'_>, Silent<CErr>> = ParserContext::new(Silent::new());
  let out = Parser::with_parser_and_context(parse, ctx)
    .parse_str_with_state("1 2", ScanLimiter::with_limit(1));
  assert_eq!(
    out,
    Err(CErr::Eot),
    "a zero-width `Continue` on a trip-truncated window must not swallow the terminal stop through \
     the no-progress guard, got {out:?}"
  );
}

#[test]
fn delim_while_condition_error_does_not_preempt_a_terminal_stop() {
  // The delimited twin of `while_condition_error_does_not_preempt_a_terminal_stop`. `(1 2` under a
  // limit of 2, window `U2`: the opener and the front `1` scan cleanly, the second window slot trips.
  // The fallible condition must not preempt the terminal stop the eager gate surfaces.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<CErr>>>,
  ) -> Result<Vec<i64>, CErr> {
    let cond = |mut peeked: Peeked<'_, 'inp, TLexer<'inp>, U2>,
                _: &mut Silent<CErr>|
     -> Result<Action, CErr> {
      if peeked.len() < 2 {
        return Err(CErr::Ordinary);
      }
      Ok(match peeked.pop_front() {
        Some(tok) if matches!(tok.token(), Tok::Num(_)) => Action::Continue,
        _ => Action::Stop,
      })
    };
    let elem = |_inp: &mut InputRef<
      'inp,
      '_,
      TLexer<'inp>,
      ParserContext<'inp, TLexer<'inp>, Silent<CErr>>,
    >|
     -> Result<i64, CErr> { Ok(0) };
    elem
      .repeated_while::<_, U2>(cond)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let ctx: ParserContext<'_, TLexer<'_>, Silent<CErr>> = ParserContext::new(Silent::new());
  let out = Parser::with_parser_and_context(parse, ctx)
    .parse_str_with_state("(1 2", ScanLimiter::with_limit(2));
  assert_eq!(
    out,
    Err(CErr::Eot),
    "a fallible condition on a trip-truncated window must not preempt the terminal stop inside \
     delimiters, got {out:?}"
  );
}

#[test]
fn delim_while_zero_width_continue_does_not_swallow_a_terminal_stop() {
  // The delimited twin of `while_zero_width_continue_does_not_swallow_a_terminal_stop`. `(1 2` under
  // a limit of 2, window `U2`: the second window slot trips. A zero-width `Continue` makes no
  // progress; without the eager gate the trip-truncated front token would reach the epilogue's close
  // probe and be misread as a spurious wrong closer while the stop is swallowed. Surfacing the
  // terminal end-of-input error before that epilogue is reached also removes that spurious diagnostic.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<CErr>>>,
  ) -> Result<Vec<i64>, CErr> {
    let cond = |_peeked: Peeked<'_, 'inp, TLexer<'inp>, U2>,
                _: &mut Silent<CErr>|
     -> Result<Action, CErr> { Ok(Action::Continue) };
    let elem = |_inp: &mut InputRef<
      'inp,
      '_,
      TLexer<'inp>,
      ParserContext<'inp, TLexer<'inp>, Silent<CErr>>,
    >|
     -> Result<i64, CErr> { Ok(0) };
    elem
      .repeated_while::<_, U2>(cond)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let ctx: ParserContext<'_, TLexer<'_>, Silent<CErr>> = ParserContext::new(Silent::new());
  let out = Parser::with_parser_and_context(parse, ctx)
    .parse_str_with_state("(1 2", ScanLimiter::with_limit(2));
  assert_eq!(
    out,
    Err(CErr::Eot),
    "a zero-width `Continue` on a trip-truncated window inside delimiters must surface the terminal \
     stop, not swallow it and report a spurious wrong closer, got {out:?}"
  );
}
