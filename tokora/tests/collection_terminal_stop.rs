#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Collection-driver terminal-stop regressions.
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
//! * **R1b — and the gate still fires when the cursor *does* reach the boundary.** R1's twin, and
//!   the half that was missing: it says only where the witness must stay quiet, which a witness
//!   deleted outright also satisfies. R1b pins the positive direction, and it is what makes the
//!   `at_committed_boundary()` term in the chokepoint's guard load-bearing under test rather than
//!   merely present in the source. See its own section for what a mutation run measured.
//! * **R2 — the separator-slot gate surfaces a trip** instead of folding it into a clean end.
//! * **R3 — the while-list decision peek surfaces a mid-window trip** instead of reading its short
//!   window as a clean stop.

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

  /// A shared handle on the scan counter, readable after the state has been moved into the parse.
  ///
  /// The non-vacuity control for [`r1c_a_trip_caught_inside_an_elements_own_attempt_is_not_filed`],
  /// which cannot use [`limit_diags`]: the element catches its trip inside an `attempt` it then
  /// declines, and that rollback rewinds the **emissions** along with everything else — the trip's
  /// own diagnostic included. A count above the limit is the fact that survives it, because
  /// [`State::check`] is exactly that comparison and a failing `check` is what the trip is.
  fn counter(&self) -> Rc<Cell<usize>> {
    self.scanned.clone()
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

/// Counts the scan limiter's own trip diagnostics.
///
/// The non-vacuity control for every R1b cell: those cells assert that a collection re-raised
/// instead of filing, and a fixture that quietly stopped tripping — a retuned cache, a scan the
/// lexer no longer repeats — satisfies "filed nothing" by doing nothing. One here on the tight run
/// and none on its widened control is what makes the pair a measurement.
fn limit_diags(emitter: &Verbose<CErr>) -> usize {
  emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == CErr::Limit)
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

// ── R1b: an element failure AT the committed boundary is re-raised, never filed ─
//
// The positive half of R1, and the one the suite was missing.
//
// # Why this section exists
//
// `tokora/src/parser/many/mod.rs::file_element_failure` is the single place a collection element
// failure is filed or re-raised, and it gates on three witnesses. A mutation run over the whole
// `--all-features` suite, one witness neutered at a time, found that two of the three were pinned
// by behaviour and the third was not:
//
// | witness                          | tests that failed when it was neutered |
// |----------------------------------|----------------------------------------|
// | `Cmpl::is_incomplete_error(..)`  | 2 (`input::input_ref::partial_tests`)  |
// | `inp.at_committed_boundary()`    | **0 — nothing, anywhere**              |
// | `inp.tripped_during_attempt(..)` | 12 (`collection_resource_trip.rs`)     |
//
// R1 is negative — it fails when the witness fires where it must not — so deleting the witness
// keeps R1 green. R2 and R3 gate *other* call sites (the separator slot, the decision peek), not
// this one. Nothing asked what happens when an element fails with the committed cursor **on** the
// boundary, which is precisely the case the witness exists for. These cells ask it.
//
// # What "unguarded" looks like, measured
//
// With the term removed from the guard and everything else intact:
//
// * [`r1b_a_boundary_reached_before_the_driver_is_not_filed_as_a_diagnostic`] returns
//   `Ok(vec![1, 2])` — a **silently truncated collection**, plus a spurious "ordinary" syntax error
//   over input the scanner never looked at. Nothing in the result says the parse was cut short.
//   That cell is the sharpest of the set because the boundary is latched *before* the driver
//   starts, so the driver's sibling absence witness (`latched_during_attempt`, attempt-relative)
//   cannot see it either: this one term is the whole defence.
// * the four family cells return `Err(CErr::Eot)` with the element's ordinary failure filed —
//   the stop still surfaces, from a gate further down, but the diagnostic log has gained a syntax
//   error that describes nothing and the re-raised value is no longer the element's own.
//
// # Shape
//
// Every cell runs the identical probe twice with **only the scan limit changed**, and the two runs
// must disagree. The tight run must really have tripped (`limit_diags == 1`) and the control must
// really not have (`limit_diags == 0`), because "filed nothing" is otherwise satisfied by a fixture
// that stopped reaching the collection at all. The control's value is asserted absolutely for the
// same reason.

/// An element that accepts one `Num` and fails **ordinarily** on anything else — a wrong token, a
/// real end of input, or a poison boundary alike. `try_expect` pushes back on decline, so a failure
/// consumes nothing.
fn num_or_ordinary<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  match inp.try_expect(|t| matches!(t.data, Tok::Num(_)))? {
    Some(tok) => match tok.into_data() {
      Tok::Num(n) => Ok(ParseAttempt::Accept(n)),
      _ => unreachable!("the predicate accepted only `Num`"),
    },
    None => Err(CErr::Ordinary),
  }
}

/// A **two-token** element: commit one `Num`, then demand a second.
///
/// The four family cells need the element to fail with the committed cursor *on* the boundary, and
/// the separated drivers reach their separator slot before their element slot — a boundary sitting
/// between two elements is therefore claimed by R2's `try_expect_or_stop` gate and never reaches
/// the chokepoint. An element that commits its first token and then fails moves the cursor onto the
/// boundary from *inside* the element attempt, which every family reaches the same way.
fn pair_element<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>,
{
  let ParseAttempt::Accept(first) = num_or_ordinary(inp)? else {
    return Ok(ParseAttempt::Decline);
  };
  let ParseAttempt::Accept(second) = num_or_ordinary(inp)? else {
    return Ok(ParseAttempt::Decline);
  };
  Ok(ParseAttempt::Accept(first + second))
}

#[test]
fn r1b_a_boundary_reached_before_the_driver_is_not_filed_as_a_diagnostic() {
  // R1's fixture with one thing changed: the element. There a `comma_element` fails while still
  // replaying cached tokens, leaving the committed cursor short of the boundary, and the failure
  // must be filed and looped past. Here the element consumes those same cached tokens, so the third
  // cycle fails with the cursor exactly ON the boundary — and the failure must be re-raised.
  //
  // The prefill is what makes this the sharpest cell in the section: the boundary is latched before
  // `repeated` starts, so the driver's `latch_snapshot()` already holds it and the absence-exit
  // witness (`latched_during_attempt`) reads `false` for the rest of the parse. With
  // `at_committed_boundary()` removed from the chokepoint's guard nothing else catches the stop —
  // the run returns `Ok(vec![1, 2])`, a truncated collection reported as a success.
  fn r1b_parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TLexer<'inp>, Error = CErr> + FullContainerEmitter<'inp, TLexer<'inp>>,
  {
    // Prefill + latch: a wide peek scans `1`(1), `2`(2), then the third scan trips.
    let _ = inp.peek_with_emitter::<U4>()?;
    num_or_ordinary.repeated().collect().parse_input(inp)
  }

  let mut tight_log = Verbose::<CErr>::new();
  let tight = drive(
    &mut tight_log,
    ScanLimiter::with_limit(2),
    r1b_parse,
    "1 2 3",
  );
  assert_eq!(
    limit_diags(&tight_log),
    1,
    "the tight run must really have tripped the scan limit — without that this cell compares two \
     untripped parses and asserts nothing"
  );
  assert_eq!(
    (&tight, ordinary_diags(&tight_log)),
    (&Err(CErr::Ordinary), 0),
    "an element failure with the committed cursor ON a latched boundary is re-raised untouched and \
     files nothing. Filed instead, the driver loops, stalls, and returns `Ok(vec![1, 2])` — a \
     collection truncated at the poison and reported as a success, carrying a syntax diagnostic \
     over input the scanner never read"
  );

  // Non-vacuity: same probe, same source, only the scan limit changed. The swallow arm is reachable
  // in this fixture — the control proves it by taking it, at a real end of input.
  let mut roomy_log = Verbose::<CErr>::new();
  let roomy = drive(
    &mut roomy_log,
    ScanLimiter::with_limit(1_000),
    r1b_parse,
    "1 2 3",
  );
  assert_eq!(
    limit_diags(&roomy_log),
    0,
    "the control differs from the run above in one thing only — with room, nothing trips"
  );
  assert_eq!(
    (&roomy, ordinary_diags(&roomy_log)),
    (&Ok(vec![1, 2, 3]), 1),
    "the control pins the fixture absolutely: the collection is reached, the same element fails the \
     same way at the REAL end of input, and there the driver files that one diagnostic and loops \
     past it. The boundary is the only difference"
  );
  assert_ne!(
    tight, roomy,
    "the two runs must disagree — a cell where they agree is measuring the scan limit, not the gate"
  );
}

/// One R1b family cell: a boundary the element itself reaches, and its non-vacuity control.
///
/// `$tight` is chosen so the trip lands inside the *third* element attempt, after two have been
/// accepted — the collection is fully under way, and the run is not a degenerate first-element
/// failure. The scan tally it is tuned against counts re-scans as well as first reads, so the value
/// is empirical; a change to the cache that shifts it reds the `limit_diags` assertions loudly
/// rather than quietly turning the cell into a second copy of its control.
macro_rules! boundary_cell {
  ($name:ident, $probe:ident, $family:literal, $src:literal, $tight:expr, filed = $filed:expr) => {
    /// An element failure at the committed boundary re-raises rather than being filed and looped
    /// past, and the widened-limit control proves the boundary is what did it.
    #[test]
    fn $name() {
      let mut tight_log = Verbose::<CErr>::new();
      let tight = drive(
        &mut tight_log,
        ScanLimiter::with_limit($tight),
        $probe,
        $src,
      );
      assert_eq!(
        limit_diags(&tight_log),
        1,
        concat!(
          $family,
          ": the tight run must really have tripped the scan limit — without that this cell \
           compares two untripped parses and asserts nothing"
        )
      );
      assert_eq!(
        (&tight, ordinary_diags(&tight_log)),
        (&Err(CErr::Ordinary), 0),
        concat!(
          $family,
          ": the element committed its first token, ran onto the poison boundary, and failed \
           there. That failure is re-raised untouched and nothing is filed. Without \
           `at_committed_boundary()` in the chokepoint's guard it is filed as an ordinary syntax \
           error and the loop carries on, and the stop reaches the caller from a later gate as \
           `CErr::Eot` — a diagnostic log describing input the scanner never read"
        )
      );

      // Non-vacuity: same probe, same source, only the scan limit changed.
      let mut roomy_log = Verbose::<CErr>::new();
      let roomy = drive(
        &mut roomy_log,
        ScanLimiter::with_limit(1_000),
        $probe,
        $src,
      );
      assert_eq!(
        limit_diags(&roomy_log),
        0,
        concat!(
          $family,
          ": the control differs from the run above in one thing only — with room, nothing trips"
        )
      );
      assert_eq!(
        (&roomy, ordinary_diags(&roomy_log)),
        (&Ok(vec![23, 43, 63]), $filed),
        concat!(
          $family,
          ": the control pins the fixture absolutely — the collection is reached, every pair is \
           accepted, and the construct completes"
        )
      );
      assert_ne!(
        tight,
        roomy,
        concat!(
          $family,
          ": the two runs must disagree — a cell where they agree is measuring the scan limit, not \
           the gate"
        )
      );
    }
  };
}

fn pair_repeated<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, TLexer<'inp>, Error = CErr> + FullContainerEmitter<'inp, TLexer<'inp>>,
{
  pair_element.repeated().collect().parse_input(inp)
}

fn pair_delim_repeated<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>
    + FullContainerEmitter<'inp, TLexer<'inp>>
    + UnclosedEmitter<'inp, TLexer<'inp>>,
{
  pair_element
    .repeated()
    .delimited::<Paren<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

fn pair_separated<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>
    + FullContainerEmitter<'inp, TLexer<'inp>>
    + SeparatedEmitter<'inp, TLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TLexer<'inp>>,
{
  pair_element.separated_by_comma().collect().parse_input(inp)
}

fn pair_sep_delim<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<Vec<i64>, CErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = CErr>
    + FullContainerEmitter<'inp, TLexer<'inp>>
    + SeparatedEmitter<'inp, TLexer<'inp>>
    + UnclosedEmitter<'inp, TLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, TLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, TLexer<'inp>>,
{
  pair_element
    .separated_by_comma()
    .delimited::<Paren<(), (), ()>>()
    .collect()
    .parse_input(inp)
}

// The four try-driven families — the only drivers that swallow, and so the only ones that call the
// chokepoint. `filed` is the control's ordinary-diagnostic count: the two `repeated` families end
// their list on a failing element at the real end of input and file it, while the separated
// families end theirs on an absent separator and file nothing.

boundary_cell!(
  r1b_repeated_re_raises_a_boundary_element_failure,
  pair_repeated,
  "`repeated()`",
  "11 12 21 22 31 32",
  5,
  filed = 1
);

boundary_cell!(
  r1b_delimited_repeated_re_raises_a_boundary_element_failure,
  pair_delim_repeated,
  "`repeated().delimited()`",
  "( 11 12 21 22 31 32 )",
  7,
  filed = 1
);

boundary_cell!(
  r1b_separated_re_raises_a_boundary_element_failure,
  pair_separated,
  "`separated_by_comma()`",
  "11 12 , 21 22 , 31 32",
  7,
  filed = 0
);

boundary_cell!(
  r1b_delimited_separated_re_raises_a_boundary_element_failure,
  pair_sep_delim,
  "`separated_by_comma().delimited()`",
  "( 11 12 , 21 22 , 31 32 )",
  8,
  filed = 0
);

// ── R1c: a trip caught inside the ELEMENT'S OWN attempt is not filed and looped past ─
//
// R1b's twin one nesting level down, and the sharper of the two: there the element runs onto a
// boundary that is standing, so the positional `at_committed_boundary()` sees it. Here the element
// opens an `attempt` of its own, trips **inside** it, catches the stop and declines the attempt —
// and the restore rewinds the cursor, the cache and the poison boundary together. Every positional
// reading is then clean *and* the latch is back at the value the driver snapshotted, so both of the
// scanner-side readings this tree had answered `false` and the chokepoint filed a spent scanner
// budget as an ordinary syntax diagnostic and carried on looping.
//
// This is the worst of the class the counter closes. An absence exit that reads clean returns a
// silently truncated `Ok`; this one *also* manufactures a diagnostic describing input the scanner
// never read, and does it on the resilient path that exists to keep parsing.

/// An element that trips the scanner inside its **own** inner attempt, catches the stop, declines
/// that attempt, and then fails ordinarily.
///
/// Grammar code is entitled to every one of those moves — speculating, catching an error, and
/// failing for its own reasons. What it must not be able to do is erase the driver's evidence that
/// a resource budget was spent, which is a property of where the evidence lives and not of what the
/// grammar did.
fn trip_inside_attempt_then_fail<'inp, Ctx>(
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
        // Both exits decline the inner attempt, so the rollback is identical either way and only
        // the scan limit decides whether a stop happened inside it.
        Ok(None) | Err(_) => return None,
      }
    }
  });
  Err(CErr::Ordinary)
}

#[test]
fn r1c_a_trip_caught_inside_an_elements_own_attempt_is_not_filed() {
  // `1 2` under a limit of 1: the element's inner attempt drains `1` (1st scan) and trips on `2`
  // (2nd scan), catches it, declines the attempt, and then fails ordinarily with the cursor rolled
  // back to the front and the boundary erased.
  fn parse<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<Vec<i64>, CErr>
  where
    Ctx: ParseContext<'inp, TLexer<'inp>>,
    Ctx::Emitter:
      Emitter<'inp, TLexer<'inp>, Error = CErr> + FullContainerEmitter<'inp, TLexer<'inp>>,
  {
    trip_inside_attempt_then_fail
      .repeated()
      .collect()
      .parse_input(inp)
  }

  let tight_limiter = ScanLimiter::with_limit(1);
  let tight_scans = tight_limiter.counter();
  let mut tight_log = Verbose::<CErr>::new();
  let tight = drive(&mut tight_log, tight_limiter, parse, "1 2");
  assert!(
    tight_scans.get() > 1,
    "the tight run must really have tripped the scan limit — without that this cell compares two \
     untripped parses and asserts nothing. Note what CANNOT be asserted here: `limit_diags` is 0, \
     because the element caught the trip inside an `attempt` it then declined and the rollback \
     rewound the emissions too. The stop is not merely unfiled — after the rollback the diagnostic \
     log does not mention it at all, which is exactly why the witness has to be a cell no rollback \
     reaches"
  );
  assert_eq!(
    limit_diags(&tight_log),
    0,
    "the rollback rewound the trip's own diagnostic — pinned so that a later change which keeps it \
     is noticed here rather than quietly making the assertion above redundant"
  );
  assert_eq!(
    (&tight, ordinary_diags(&tight_log)),
    (&Err(CErr::Ordinary), 0),
    "an element failure that shares its attempt with a scanner trip the element caught inside a \
     speculation of its own is re-raised untouched, and nothing is filed. Filed instead, the loop \
     carries on over a spent scanner budget and the caller gets `Ok(vec![])` plus a syntax \
     diagnostic over input the scanner never read"
  );

  // Non-vacuity: the identical element and the identical rollback, only the scan limit widened. The
  // swallow arm is reachable in this fixture — the control proves it by taking it.
  let roomy_limiter = ScanLimiter::with_limit(1_000);
  let roomy_scans = roomy_limiter.counter();
  let mut roomy_log = Verbose::<CErr>::new();
  let roomy = drive(&mut roomy_log, roomy_limiter, parse, "1 2");
  assert!(
    roomy_scans.get() <= 1_000,
    "the control differs from the run above in one thing only — with room, nothing trips"
  );
  assert_eq!(
    (&roomy, ordinary_diags(&roomy_log)),
    (&Ok(vec![]), 1),
    "the control pins the fixture absolutely: the same element fails the same way, and with no stop \
     to refuse the driver files that one diagnostic, stalls, and ends the collection"
  );
  assert_ne!(
    tight, roomy,
    "the two runs must disagree — a cell where they agree is measuring the scan limit, not the gate"
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
  // instead of ending the list cleanly. Driven through the public `list` constructor.
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
                _: EmitterView<'_, 'inp, TLexer<'inp>, Silent<CErr>>|
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
                _: EmitterView<'_, 'inp, TLexer<'inp>, Silent<CErr>>|
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
                _: EmitterView<'_, 'inp, TLexer<'inp>, Silent<CErr>>|
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
                _: EmitterView<'_, 'inp, TLexer<'inp>, Silent<CErr>>|
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
