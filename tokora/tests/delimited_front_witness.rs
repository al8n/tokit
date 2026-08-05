#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]
#![allow(clippy::type_complexity)]
//! The close-miss suppression's witness: it must answer *is the front still that token?*, not
//! *has anything been consumed?*
//!
//! A wrong opening delimiter is reported once, and the close probe's re-sighting of that same
//! cached token is suppressed. The suppression's premise is that zero committed consumption
//! implies the flagged token is still at the front — and that premise is false on its own.
//! `InputRef::set_state` and `state_mut` take the parked token and clear the cache while writing
//! nothing to `span`, so the region ahead re-lexes under a new regime at an unchanged offset.
//! Both are public, documented, and reachable from any element parser, which runs inside exactly
//! that window. Suppressing on the watermark alone there deletes a close-miss naming a token
//! nothing else reported — trading the duplicate this feature removes for a silent loss.
//!
//! So the flag carries a pair: the committed-consumption watermark AND the input's re-key count.
//! These cells are the regression, one per driver family, with the lexer built to defeat a weaker
//! witness three ways:
//!
//! - `?` lexes as `P` or `Q` depending on the state — a **kind** change at a fixed offset, which a
//!   span-only witness misses;
//! - `!` lexes as `Num(1)` or `Num(2)` — a **value** change at a fixed offset with the kind and
//!   span identical, which a span-plus-kind witness also misses;
//! - `?` additionally mutates the lexer's OWN state while scanning, so a witness that trusted
//!   scan-time state would be defeated too. It is not: an uncommitted probe lexes through a clone
//!   of the state, so only *durable* state surgery survives — and durable surgery is the re-key
//!   the counter records.
//!
//! The suite's own positive controls are the `plain` and `rollback` cells: with no re-key the
//! duplicate must still be suppressed. They are what fails if someone later "simplifies" the
//! witness back to the watermark, and equally if someone makes it so conservative that it stops
//! suppressing anything.

use generic_arraydeque::typenum::U1;
use tokora::EmitterView;
use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseContext, ParseInput, Parser, ParserContext,
  SimpleSpan, Token, TryParseInput,
  cache::Peeked,
  emitter::Verbose,
  error::{
    Unclosed,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::Action,
  punct::{Bracket, CloseBracket, Comma, OpenBracket},
  state::State,
  token::PunctuatorToken,
  try_parse_input::ParseAttempt,
  utils::Expected,
};

// ── A state whose value changes what the lexer produces ──────────────────────

#[derive(Debug, Clone, Default, PartialEq)]
struct Flip {
  flipped: bool,
}

impl State for Flip {
  type Error = ();
  fn check(&self) -> Result<(), ()> {
    Ok(())
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FK {
  Open,
  Close,
  Comma,
  Num,
  P,
  Q,
}

impl core::fmt::Display for FK {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      FK::Open => write!(f, "["),
      FK::Close => write!(f, "]"),
      FK::Comma => write!(f, ","),
      FK::Num => write!(f, "number"),
      FK::P => write!(f, "P"),
      FK::Q => write!(f, "Q"),
    }
  }
}

thread_local! {
  /// One-shot: the next `FT::drop` panics, and disarms itself first so the unwind cannot meet a
  /// second panic and abort.
  static DROP_BOMB: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

fn arm_token_drop() {
  DROP_BOMB.with(|c| c.set(true));
}

fn disarm_token_drop() -> bool {
  DROP_BOMB.with(|c| c.replace(false))
}

#[derive(Debug, Clone, PartialEq)]
enum FT {
  Open,
  Close,
  Comma,
  Num(i64),
  P,
  Q,
}

impl core::fmt::Display for FT {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{:?}", self)
  }
}

impl Drop for FT {
  fn drop(&mut self) {
    if DROP_BOMB.with(|c| c.replace(false)) {
      panic!("the armed token drop panics");
    }
  }
}

impl Token<'_> for FT {
  type Kind = FK;
  type Error = ();

  fn kind(&self) -> FK {
    match self {
      FT::Open => FK::Open,
      FT::Close => FK::Close,
      FT::Comma => FK::Comma,
      FT::Num(_) => FK::Num,
      FT::P => FK::P,
      FT::Q => FK::Q,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl PunctuatorToken<'_> for FT {
  fn comma() -> Option<FK> {
    Some(FK::Comma)
  }
  fn open_bracket() -> Option<FK> {
    Some(FK::Open)
  }
  fn close_bracket() -> Option<FK> {
    Some(FK::Close)
  }
}

impl From<Comma<(), (), ()>> for FK {
  fn from(_: Comma<(), (), ()>) -> Self {
    FK::Comma
  }
}
impl From<OpenBracket<(), (), ()>> for FK {
  fn from(_: OpenBracket<(), (), ()>) -> Self {
    FK::Open
  }
}
impl From<CloseBracket<(), (), ()>> for FK {
  fn from(_: CloseBracket<(), (), ()>) -> Self {
    FK::Close
  }
}

/// A one-character-per-token lexer. `?` is the state-dependent position: it lexes as `P` while
/// the state is unflipped and as `Q` once it is flipped — same bytes, same offset, different token.
struct FlipLexer<'a> {
  src: &'a str,
  start: usize,
  end: usize,
  state: Flip,
}

impl<'a> Lexer<'a> for FlipLexer<'a> {
  type State = Flip;
  type Source = str;
  type Token = FT;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'a str) -> Self {
    Self::with_state(src, Flip { flipped: false })
  }

  fn with_state(src: &'a str, state: Flip) -> Self {
    Self {
      src,
      start: 0,
      end: 0,
      state,
    }
  }

  fn check(&self) -> Result<(), ()> {
    Ok(())
  }

  fn state(&self) -> &Flip {
    &self.state
  }

  fn state_mut(&mut self) -> &mut Flip {
    &mut self.state
  }

  fn into_state(self) -> Flip {
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

  fn lex(&mut self) -> Option<Result<FT, ()>> {
    let b = self.src.as_bytes();
    let mut i = self.end;
    while i < b.len() && b[i] == b' ' {
      i += 1;
    }
    if i >= b.len() {
      self.start = i;
      self.end = i;
      return None;
    }
    self.start = i;
    let c = b[i];
    self.end = i + 1;
    Some(Ok(match c {
      b'[' => FT::Open,
      b']' => FT::Close,
      b',' => FT::Comma,
      b'?' => {
        let t = if self.state.flipped { FT::Q } else { FT::P };
        // The lexer mutates its OWN state while scanning — a mode stack, a nesting depth, an
        // interpolation flag. No `set_state` is involved and no caller asked for it.
        self.state.flipped = true;
        t
      }
      b'!' => {
        // Same KIND (Num) either way; only the token VALUE changes. This is the shape that
        // defeats a span-plus-kind witness.
        if self.state.flipped {
          FT::Num(2)
        } else {
          FT::Num(1)
        }
      }
      d if d.is_ascii_digit() => FT::Num(i64::from(d - b'0')),
      _ => FT::P,
    }))
  }

  fn bump(&mut self, n: &usize) {
    self.end += *n;
  }
}

// ── Recorded diagnostics ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum D {
  Unexpected {
    expected: Option<FK>,
    found: Option<FT>,
    span: (usize, usize),
  },
  Unclosed,
  Eot,
  Other(&'static str),
}

impl<Lang: ?Sized> From<UnexpectedToken<'_, FT, FK, SimpleSpan, Lang>> for D {
  fn from(e: UnexpectedToken<'_, FT, FK, SimpleSpan, Lang>) -> Self {
    D::Unexpected {
      expected: match e.expected() {
        Some(Expected::One(k)) => Some(*k),
        _ => None,
      },
      found: e.found().cloned(),
      span: (e.span().start(), e.span().end()),
    }
  }
}
impl<H, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEnd<H, usize, Lang, Set>>
  for D
{
  fn from(_: tokora::error::UnexpectedEnd<H, usize, Lang, Set>) -> Self {
    D::Eot
  }
}
impl From<()> for D {
  fn from(_: ()) -> Self {
    D::Other("unit")
  }
}
impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for D {
  fn from(_: FullContainer<S, Lang>) -> Self {
    D::Other("full")
  }
}
impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for D {
  fn from(_: TooFew<S, Lang>) -> Self {
    D::Other("toofew")
  }
}
impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for D {
  fn from(_: TooMany<S, Lang>) -> Self {
    D::Other("toomany")
  }
}
impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for D {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    D::Other("missing")
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for D {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    D::Other("sep")
  }
}
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for D {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    D::Other("missingsyntax")
  }
}
impl<Dl, Lang: ?Sized> From<Unclosed<Dl, SimpleSpan, Lang>> for D {
  fn from(_: Unclosed<Dl, SimpleSpan, Lang>) -> Self {
    D::Unclosed
  }
}
impl<'inp, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, FlipLexer<'inp>, Lang> for D {
  fn from_unclosed<Dl>(_: Unclosed<Dl, SimpleSpan, Lang>) -> Self {
    D::Unclosed
  }
}

type VCtx<'inp> = ParserContext<'inp, FlipLexer<'inp>, Verbose<D>>;

fn vctx() -> VCtx<'static> {
  ParserContext::new(Verbose::new())
}

// ── The two element parsers: identical except for the state surgery ──────────

fn elem_plain<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, FlipLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, D>
where
  Ctx: ParseContext<'inp, FlipLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, FlipLexer<'inp>, Error = D>,
{
  Ok(ParseAttempt::Decline)
}

fn elem_surgery<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, FlipLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, D>
where
  Ctx: ParseContext<'inp, FlipLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, FlipLexer<'inp>, Error = D>,
{
  // Legal, public, documented: re-key the input so the region ahead lexes under a new state.
  // Consumes nothing, so the driver's committed watermark does not move.
  inp.state_mut().flipped = true;
  Ok(ParseAttempt::Decline)
}

/// The committed element the `_while` families drive: it performs the surgery and then fails
/// **without consuming**, so the driver records the failure and the watermark stays put. A
/// declining element is not available here — these families take a committed parser.
fn elem_surgery_committed<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, FlipLexer<'inp>, Ctx>,
) -> Result<i64, D>
where
  Ctx: ParseContext<'inp, FlipLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, FlipLexer<'inp>, Error = D>,
{
  inp.state_mut().flipped = true;
  Err(D::Other("element declined after surgery"))
}

/// The plain twin of the above: fails identically, touching no state.
fn elem_plain_committed<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, FlipLexer<'inp>, Ctx>,
) -> Result<i64, D>
where
  Ctx: ParseContext<'inp, FlipLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, FlipLexer<'inp>, Error = D>,
{
  Err(D::Other("element declined"))
}

/// Continue exactly once, then stop: the `_while` families need their element to run (that is
/// where the surgery happens) and then the loop to end so the close probe is reached.
fn decide_once<'inp>(
  _peeked: Peeked<'_, 'inp, FlipLexer<'inp>, U1>,
  _: EmitterView<'_, 'inp, FlipLexer<'inp>, Verbose<D>>,
) -> Result<Action, D> {
  Ok(if ONCE.with(|c| c.replace(true)) {
    Action::Stop
  } else {
    Action::Continue
  })
}

thread_local! {
  static ONCE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

fn reset_once() {
  ONCE.with(|c| c.set(false));
}

/// Every close-miss (expected-close) diagnostic in `rows`, by the token it names.
fn close_misses(rows: &[D]) -> std::vec::Vec<FT> {
  rows
    .iter()
    .filter_map(|d| match d {
      D::Unexpected {
        expected: Some(FK::Close),
        found: Some(t),
        ..
      } => Some(t.clone()),
      _ => None,
    })
    .collect()
}

/// Every wrong-opener (expected-open) diagnostic in `rows`, by the token it names.
fn opener_misses(rows: &[D]) -> std::vec::Vec<FT> {
  rows
    .iter()
    .filter_map(|d| match d {
      D::Unexpected {
        expected: Some(FK::Open),
        found: Some(t),
        ..
      } => Some(t.clone()),
      _ => None,
    })
    .collect()
}

/// An element that only rolls back an empty attempt: public API, consumes nothing, changes no
/// durable state.
fn elem_attempt<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, FlipLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, D>
where
  Ctx: ParseContext<'inp, FlipLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, FlipLexer<'inp>, Error = D>,
{
  let _: Option<()> = inp.attempt(|_| None);
  Ok(ParseAttempt::Decline)
}

/// The same driver over a NON-RETAINING cache, where an unconsumed front token is *parked*
/// rather than cached — so a rollback drops it and the close probe must re-lex.
type BhCtx<'inp> = ParserContext<'inp, FlipLexer<'inp>, Verbose<D>, ()>;

fn drive_blackhole(src: &'static str) -> std::vec::Vec<D> {
  let probe = move |inp: &mut InputRef<'static, '_, FlipLexer<'static>, BhCtx<'static>>| -> Result<
    std::vec::Vec<D>,
    D,
  > {
    let r: Result<std::vec::Vec<i64>, D> = elem_attempt
      .repeated()
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp);
    let _ = r;
    Ok(inp.emitter_ref().errors().values().flatten().cloned().collect())
  };
  Parser::with_context(ParserContext::new(Verbose::new()))
    .apply(probe)
    .parse_str(src)
    .expect("the probe converts the outcome into rows")
}

/// An element that re-keys through `set_state` while a **cached** token's `Drop` panics, and
/// catches the unwind itself. `set_state` installs the new state *before* clearing the cache, so
/// a caught panic out of the clear leaves the input carrying the new regime with the old front
/// already destroyed.
fn elem_surgery_catching<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, FlipLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, D>
where
  Ctx: ParseContext<'inp, FlipLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, FlipLexer<'inp>, Error = D>,
{
  arm_token_drop();
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    inp.set_state(Flip { flipped: true });
  }));
  let still_armed = disarm_token_drop();
  assert!(
    caught.is_err(),
    "the fixture needs the armed drop to fire inside the re-key (still armed: {still_armed})"
  );
  Ok(ParseAttempt::Decline)
}

/// The `state_mut` twin of the element above. The distinction matters for how the defect is
/// described: `set_state` installs the new state BEFORE the re-key runs, so a caught panic out of
/// the re-key leaves the new regime live over a destroyed front. `state_mut` returns a borrow only
/// after the re-key returns, so a panic inside it means the caller's mutation never runs and the
/// durable state is unchanged — the re-lex reproduces the same token and there is nothing to lose.
fn elem_surgery_catching_state_mut<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, FlipLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, D>
where
  Ctx: ParseContext<'inp, FlipLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, FlipLexer<'inp>, Error = D>,
{
  arm_token_drop();
  let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    inp.state_mut().flipped = true;
  }));
  let _ = disarm_token_drop();
  assert!(
    caught.is_err(),
    "the armed drop must fire inside the re-key"
  );
  Ok(ParseAttempt::Decline)
}

fn drive_catching_state_mut(src: &'static str) -> std::vec::Vec<D> {
  let probe = move |inp: &mut InputRef<'static, '_, FlipLexer<'static>, VCtx<'static>>| -> Result<
    std::vec::Vec<D>,
    D,
  > {
    let r: Result<std::vec::Vec<i64>, D> = elem_surgery_catching_state_mut
      .repeated()
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp);
    let _ = r;
    Ok(inp.emitter_ref().errors().values().flatten().cloned().collect())
  };
  Parser::with_context(vctx())
    .apply(probe)
    .parse_str(src)
    .expect("the probe converts the outcome into rows")
}

/// The conservative direction, pinned on the path where getting it wrong is otherwise harmless.
///
/// Unlike its `set_state` sibling, a panic inside `state_mut`'s re-key loses nothing on its own:
/// the caller's mutation never runs, the durable state is unchanged, and the re-lex reproduces the
/// very token already reported — so suppressing there would be *correct*. Measured: with the
/// witness published after the destruction, this path records no close-miss and no diagnostic is
/// lost.
///
/// The witness is published first anyway, and so this path now records a **duplicate**. That is
/// the trade stated plainly: a re-key whose own cleanup unwound is a re-key, and the suppression
/// declines rather than reasoning about whether that particular unwind happened to be harmless.
/// The cell exists because it goes red if the publish is ever moved back after the clear — and it
/// does so on a path where that regression costs nothing, which is exactly where a regression
/// would otherwise sit unnoticed until it met the path where it costs a deleted diagnostic.
#[test]
fn an_interrupted_rekey_declines_to_suppress_rather_than_reasoning_about_it() {
  let rows = drive_catching_state_mut("? 1");
  assert_eq!(
    opener_misses(&rows),
    std::vec![FT::P],
    "the wrong opener is reported once, naming P — rows {rows:?}"
  );
  assert_eq!(
    close_misses(&rows),
    std::vec![FT::P],
    "the re-key's own cleanup unwound part-way through destroying the front. The witness was \
     published before any of that, so the pair reads changed and the suppression declines: a \
     duplicate report, not a deleted one. Publishing after the clear makes this read as empty \
     instead — harmless here, and the same ordering deletes a real diagnostic on the set_state \
     path. Rows {rows:?}"
  );
}

fn drive_catching(src: &'static str) -> std::vec::Vec<D> {
  let probe = move |inp: &mut InputRef<'static, '_, FlipLexer<'static>, VCtx<'static>>| -> Result<
    std::vec::Vec<D>,
    D,
  > {
    let r: Result<std::vec::Vec<i64>, D> = elem_surgery_catching
      .repeated()
      .delimited::<Bracket<(), (), ()>>()
      .collect()
      .parse_input(inp);
    let _ = r;
    Ok(inp.emitter_ref().errors().values().flatten().cloned().collect())
  };
  Parser::with_context(vctx())
    .apply(probe)
    .parse_str(src)
    .expect("the probe converts the outcome into rows")
}

#[test]
fn a_rekey_interrupted_by_caller_code_still_witnesses_itself() {
  let rows = drive_catching("? 1");
  assert_eq!(
    opener_misses(&rows),
    std::vec![FT::P],
    "the wrong opener is reported once, naming P — rows {rows:?}"
  );
  assert_eq!(
    close_misses(&rows),
    std::vec![FT::Q],
    "the re-key installed the new state and then destroyed the front, and a cached token's Drop \
     panicked part-way through that. The element caught the unwind and declined, so the driver \
     reached its close probe with the front gone and the new regime live — and the probe found Q. \
     The witness must already read changed at that point: publishing it only AFTER the clear \
     leaves a window in which the front is destroyed and the pair still says nothing moved, which \
     is the original defect re-entering through the fix's own ordering. Rows {rows:?}"
  );
}

/// Runs one driver family over `src` and returns every recorded diagnostic in span order.
///
/// Each family is spelled out rather than generated: the four builders take different element
/// shapes (declining versus committed) and different decision arguments, and a macro that hid
/// that would be hiding the thing the suite is about.
macro_rules! run_family {
  ($fn_name:ident, $inp:ident, $plain:expr, $surgery:expr) => {
    fn $fn_name(surgery: bool, src: &'static str) -> std::vec::Vec<D> {
      let probe = move |$inp: &mut InputRef<
            'static,
            '_,
            FlipLexer<'static>,
            VCtx<'static>,
          >|
           -> Result<std::vec::Vec<D>, D> {
            let outcome: Result<std::vec::Vec<i64>, D> = if surgery { $surgery } else { $plain };
            let _ = outcome;
            Ok($inp.emitter_ref().errors().values().flatten().cloned().collect())
          };
      Parser::with_context(vctx())
        .apply(probe)
        .parse_str(src)
        .expect("the probe converts the outcome into rows")
    }
  };
}

run_family!(
  run_repeated,
  inp,
  elem_plain
    .repeated()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp),
  elem_surgery
    .repeated()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
);

run_family!(
  run_separated,
  inp,
  elem_plain
    .separated_by_comma()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp),
  elem_surgery
    .separated_by_comma()
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
);

run_family!(
  run_repeated_while,
  inp,
  elem_plain_committed
    .repeated_while::<_, U1>(decide_once)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp),
  elem_surgery_committed
    .repeated_while::<_, U1>(decide_once)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
);

run_family!(
  run_separated_while,
  inp,
  elem_plain_committed
    .separated_by_comma_while::<_, U1>(decide_once)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp),
  elem_surgery_committed
    .separated_by_comma_while::<_, U1>(decide_once)
    .delimited::<Bracket<(), (), ()>>()
    .collect()
    .parse_input(inp)
);

/// POSITIVE CONTROL for every family: with no re-key the front is provably unmoved, so the close
/// probe is re-seeing the token already reported and its duplicate must still be suppressed.
/// These four cells are what fail if the witness is made so conservative it stops suppressing.
macro_rules! plain_cell {
  ($name:ident, $run:ident, $label:literal) => {
    #[test]
    fn $name() {
      reset_once();
      let rows = $run(false, "? 1");
      assert_eq!(
        opener_misses(&rows),
        std::vec![FT::P],
        "{}: the wrong opener is reported exactly once, naming P — rows {rows:?}",
        $label
      );
      assert_eq!(
        close_misses(&rows),
        std::vec::Vec::<FT>::new(),
        "{}: nothing consumed and nothing re-keyed, so the close probe is re-seeing the very \
         token already reported and must add no second diagnostic — rows {rows:?}",
        $label
      );
    }
  };
}

/// The regression for every family: a re-key between the flag and the close probe re-lexes the
/// front into a DIFFERENT token, so the close-miss is a real diagnostic about a token nothing
/// else reported. Suppressing it would be a silent loss, and a watermark-only witness does.
macro_rules! surgery_cell {
  ($name:ident, $run:ident, $label:literal) => {
    #[test]
    fn $name() {
      reset_once();
      let rows = $run(true, "? 1");
      assert_eq!(
        opener_misses(&rows),
        std::vec![FT::P],
        "{}: the wrong opener is still reported exactly once, naming P — rows {rows:?}",
        $label
      );
      assert_eq!(
        close_misses(&rows),
        std::vec![FT::Q],
        "{}: the re-key cleared the cache and the parked front, so the close probe re-lexed \
         offset 0 under the new state and found Q — a token no other diagnostic names. The \
         committed watermark reads unchanged across that, so a watermark-only witness deletes \
         this report. Rows {rows:?}",
        $label
      );
    }
  };
}

plain_cell!(
  repeated_plain_duplicate_suppressed,
  run_repeated,
  "repeated"
);
surgery_cell!(repeated_surgery_keeps_close_miss, run_repeated, "repeated");

plain_cell!(
  separated_plain_duplicate_suppressed,
  run_separated,
  "separated"
);
surgery_cell!(
  separated_surgery_keeps_close_miss,
  run_separated,
  "separated"
);

plain_cell!(
  repeated_while_plain_duplicate_suppressed,
  run_repeated_while,
  "repeated_while"
);
/// The two `_while` families differ **structurally**, and this pins the difference rather than
/// pretending it away.
///
/// Their loop probes the close position FIRST, before the decision is consulted and before any
/// element runs. With the opener already flagged, that probe re-sees the flagged token with *no
/// caller code in between at all* — so on this path there is no window for a re-key, and the
/// suppression cannot be defeated no matter what an element would have done. The surgery element
/// is wired up here exactly as for the other two families, and its state change never takes
/// effect because it never runs.
///
/// This is a falsifiable pin, not a tautology: reorder either loop to run its element before the
/// first close probe and the surgery lands, a `Q` close-miss appears, and this cell fails —
/// which is the correct alarm, because that reordering opens the window these families do not
/// currently have.
#[test]
fn repeated_while_close_probe_precedes_any_element() {
  reset_once();
  let plain = run_repeated_while(false, "? 1");
  reset_once();
  let surgery = run_repeated_while(true, "? 1");
  assert_eq!(
    plain, surgery,
    "repeated_while: the element never runs before the first close probe, so a surgery element      and a plain one must produce byte-identical diagnostics — plain {plain:?}, surgery {surgery:?}"
  );
  assert_eq!(
    close_misses(&surgery),
    std::vec::Vec::<FT>::new(),
    "repeated_while: the close probe re-sees the flagged token with no caller code in between,      so its duplicate is suppressed — rows {surgery:?}"
  );
}

plain_cell!(
  separated_while_plain_duplicate_suppressed,
  run_separated_while,
  "separated_while"
);
/// The `separated_while` twin of the cell above, and the same pin.
#[test]
fn separated_while_close_probe_precedes_any_element() {
  reset_once();
  let plain = run_separated_while(false, "? 1");
  reset_once();
  let surgery = run_separated_while(true, "? 1");
  assert_eq!(
    plain, surgery,
    "separated_while: the element never runs before the first close probe, so a surgery element      and a plain one must produce byte-identical diagnostics — plain {plain:?}, surgery {surgery:?}"
  );
  assert_eq!(
    close_misses(&surgery),
    std::vec::Vec::<FT>::new(),
    "separated_while: the close probe re-sees the flagged token with no caller code in between,      so its duplicate is suppressed — rows {surgery:?}"
  );
}

/// The **value**-change defeater: `!` lexes as `Num(1)` or `Num(2)` by state, so the flagged and
/// re-lexed tokens share a span AND a kind. A witness carrying "span plus kind" — the obvious
/// sharpening, and the one an outside reviewer proposed — cannot tell these apart and would
/// suppress here exactly as the watermark does, losing a report whose payload differs.
#[test]
fn a_same_kind_relex_is_still_a_distinct_diagnostic() {
  reset_once();
  let plain = run_repeated(false, "! 1");
  reset_once();
  let surgery = run_repeated(true, "! 1");
  assert_eq!(
    opener_misses(&plain),
    std::vec![FT::Num(1)],
    "the wrong opener names Num(1) — rows {plain:?}"
  );
  assert_eq!(
    close_misses(&plain),
    std::vec::Vec::<FT>::new(),
    "no re-key, so the duplicate stays suppressed — rows {plain:?}"
  );
  assert_eq!(
    close_misses(&surgery),
    std::vec![FT::Num(2)],
    "same span, same kind, different value: the close-miss names Num(2) where the opener miss \
     named Num(1), so the two reports are not the same report — rows {surgery:?}"
  );
}

/// POSITIVE CONTROL against over-conservatism, on the path that most looks like a re-key and is
/// not one: an attempt that rolls back destroys the parked front (a checkpoint never captures it,
/// and a restore takes it), so the close probe must re-lex — and the re-lex reproduces the same
/// token, because an uncommitted probe lexes through a CLONE of the state and the lexer's own
/// scan-time mutation dies with it. Only durable state surgery survives, and only that is a
/// re-key. If this cell ever reports a `Q`, the boundary between temporary and durable state has
/// moved and the witness needs re-deriving, not extending.
#[test]
fn a_rollback_without_a_rekey_still_suppresses() {
  let rows = drive_blackhole("? 1");
  assert_eq!(
    opener_misses(&rows),
    std::vec![FT::P],
    "the wrong opener is reported once, naming P — rows {rows:?}"
  );
  assert_eq!(
    close_misses(&rows),
    std::vec::Vec::<FT>::new(),
    "an empty attempt rolled back over a NON-RETAINING cache, where the flagged token is parked \
     rather than cached: the rollback dropped it and the probe re-lexed, but with no durable \
     state change the re-lex reproduces P, so this is still the same token and still one report \
     — rows {rows:?}"
  );
}
