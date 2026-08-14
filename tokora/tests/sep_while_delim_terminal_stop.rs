#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! `DelimitedBy<SeparatedWhile<..>>::parse_separated` `Action::Continue` terminal-stop
//! regressions.
//!
//! Mirrors `sep_while_terminal_stop.rs` for the delimited driver. A decision-window peek can come
//! back **trip-truncated**: a real cached front token the condition reads `Continue` from, plus a
//! `terminal` flag latched because a later window slot hit a resource-limit trip. The `Continue`
//! arm must surface that stop at the top of the arm — before the continue-state work and before the
//! close probe — or it is lost two ways: a zero-width element makes no progress, leaving the front
//! token at the close-probe position where `probe_close` reads it as a plain wrong token rather than
//! the trip it is; and a continue-state diagnostic (a missing separator) emitted first would, under
//! an emitter that rejects it, propagate out as an ordinary error ahead of the stop.

use core::cell::Cell;
use std::rc::Rc;
use tokora::EmitterView;

use generic_arraydeque::typenum::U2;
use tokora::{
  Accumulator, Emitter, InputRef, Lexer, Parse, ParseInput, Parser, ParserContext,
  Token as TokenTrait,
  cache::{Peeked, PeekedTokenExt},
  emitter::{
    FullContainerEmitter, SeparatedEmitter, Silent, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, MissingSyntaxOf},
    token::{MissingToken, MissingTokenOf, SeparatedError, UnexpectedToken, UnexpectedTokenOf},
  },
  input::Cursor,
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::Action,
  punct::{CloseParen, Comma, OpenParen, Paren},
  span::Spanned,
  state::State,
  token::PunctuatorToken,
  utils::CowStr,
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

// ── Token vocabulary: numbers, commas, and parens, every scan counted ────────

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

// ── The fixture error: distinguishes the channels the assertion reads ─────────

#[derive(Debug, Clone, PartialEq)]
enum CErr {
  /// An ordinary (recoverable) parse/lexer error — the catch-all for the emitter families.
  Ordinary,
  /// The resource-limit trip's own diagnostic.
  Limit,
  /// The committed form's end-of-input error, carrying its terminal marker — a terminal stop must
  /// surface it marked `true`, not as a plain recoverable end-of-input.
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

// ── An emitter that accepts the limit trip but rejects a missing separator ────
//
// The trip's diagnostic reaches the emitter as a lexer error; accepting it lets the peek report
// `terminal == true` instead of failing there. The missing-separator diagnostic is rejected
// (fatal), so a `Continue` cycle in `State::Element` that emits it would return that ordinary error
// unless the terminal stop is surfaced first — which is exactly what this fixture pins down.

struct SeparatorFatal;

impl<'inp> Emitter<'inp, TLexer<'inp>> for SeparatorFatal {
  type Error = CErr;

  fn emit_lexer_error(
    &mut self,
    _: Spanned<
      <<TLexer<'inp> as Lexer<'inp>>::Token as TokenTrait<'inp>>::Error,
      <TLexer<'inp> as Lexer<'inp>>::Span,
    >,
  ) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Ok(())
  }

  fn emit_unexpected_token(&mut self, _: UnexpectedTokenOf<'inp, TLexer<'inp>>) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Ok(())
  }

  fn emit_error(
    &mut self,
    _: Spanned<CErr, <TLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Ok(())
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, TLexer<'inp>>, _: u64)
  where
    TLexer<'inp>: Lexer<'inp>,
  {
  }
}

impl<'inp> SeparatedEmitter<'inp, TLexer<'inp>> for SeparatorFatal {
  fn emit_missing_separator(
    &mut self,
    _: CowStr,
    _: MissingTokenOf<'inp, TLexer<'inp>>,
  ) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Err(CErr::Ordinary)
  }

  fn emit_missing_element(&mut self, _: MissingSyntaxOf<'inp, TLexer<'inp>>) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Ok(())
  }
}

impl<'inp> FullContainerEmitter<'inp, TLexer<'inp>> for SeparatorFatal {
  fn emit_full_container(
    &mut self,
    _: FullContainer<<TLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Ok(())
  }
}

impl<'inp> UnclosedEmitter<'inp, TLexer<'inp>> for SeparatorFatal {
  fn emit_unclosed<Delimiter>(
    &mut self,
    _: Unclosed<Delimiter, <TLexer<'inp> as Lexer<'inp>>::Span>,
  ) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
    CErr: tokora::emitter::FromUnclosed<'inp, TLexer<'inp>>,
  {
    Ok(())
  }
}

impl<'inp> UnexpectedLeadingSeparatorEmitter<'inp, TLexer<'inp>> for SeparatorFatal {
  fn emit_unexpected_leading_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TLexer<'inp>>,
  ) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Ok(())
  }
}

impl<'inp> UnexpectedTrailingSeparatorEmitter<'inp, TLexer<'inp>> for SeparatorFatal {
  fn emit_unexpected_trailing_separator(
    &mut self,
    _: CowStr,
    _: UnexpectedTokenOf<'inp, TLexer<'inp>>,
  ) -> Result<(), CErr>
  where
    TLexer<'inp>: Lexer<'inp>,
  {
    Ok(())
  }
}

#[test]
fn no_progress_branch_surfaces_a_mid_window_terminal_stop() {
  // Window `U2`: the decision peek asks for two tokens. Under a limit of 2, the opener `(`
  // (1st scan) and the front `Num` (2nd scan) both scan cleanly, but the second window slot
  // trips (3rd scan) — so `peek_with_emitter_terminal` returns a one-token window with
  // `terminal == true`, not an empty one. The condition inspects only that cached front token
  // and decides `Continue`; the element is zero-width (accepts without consuming), so nothing
  // re-lexes the frontier and the trip-truncated front token would otherwise sit at the
  // close-probe position. No closing paren is reachable — the `Continue` arm must surface the
  // trip before any close-position diagnostic is reached.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<CErr>>>,
  ) -> Result<Vec<i64>, CErr> {
    let cond = |mut peeked: Peeked<'_, 'inp, TLexer<'inp>, U2>,
                _emitter: EmitterView<'_, 'inp, TLexer<'inp>, Silent<CErr>>|
     -> Result<Action, CErr> {
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
      .separated_by_comma_while::<_, U2>(cond)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let ctx: ParserContext<'_, TLexer<'_>, Silent<CErr>> = ParserContext::new(Silent::new());
  let out = Parser::with_parser_and_context(parse, ctx)
    .parse_str_with_state("(1 2", ScanLimiter::with_limit(2));
  assert_eq!(
    out,
    Err(CErr::Eot(true)),
    "a mid-window terminal stop behind a real front token must surface as the committed \
     end-of-input error marked terminal, not a clean list end or an ordinary close-position \
     diagnostic; got {out:?}"
  );
}

#[test]
fn separator_diagnostic_does_not_preempt_a_mid_window_terminal_stop() {
  // A consuming first element reaches `State::Element`, so the next `Continue` cycle is a
  // missing-separator case. Window `U2`, limit 3, input `(1 2 3`: the opener `(` (1st scan), `1`
  // (2nd scan) and `2` (3rd scan) all scan cleanly, cycle one consumes `1`; cycle two has `2`
  // cached, and the decision peek scans `3`, which trips (4th scan) — a one-token window `[2]`
  // with `terminal == true`. The condition reads the cached `2` as `Continue`. Because state is
  // `Element`, `handle_continue` would emit a missing-separator diagnostic first; under
  // `SeparatorFatal` that emission returns an ordinary error via `?`. The `Continue` arm must
  // surface the terminal stop ahead of that continue-state work and the close probe.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, SeparatorFatal>>,
  ) -> Result<Vec<i64>, CErr> {
    let cond = |mut peeked: Peeked<'_, 'inp, TLexer<'inp>, U2>,
                _emitter: EmitterView<'_, 'inp, TLexer<'inp>, SeparatorFatal>|
     -> Result<Action, CErr> {
      Ok(match peeked.pop_front() {
        Some(tok) if matches!(tok.token(), Tok::Num(_)) => Action::Continue,
        _ => Action::Stop,
      })
    };
    // A *consuming* element: it takes one `Num` per call, so the first cycle advances the cursor
    // into `State::Element`.
    let elem = |inp: &mut InputRef<
      'inp,
      '_,
      TLexer<'inp>,
      ParserContext<'inp, TLexer<'inp>, SeparatorFatal>,
    >|
     -> Result<i64, CErr> {
      match inp.next()? {
        Some(tok) => match tok.into_data() {
          Tok::Num(n) => Ok(n),
          _ => Err(CErr::Ordinary),
        },
        None => Err(CErr::Ordinary),
      }
    };
    elem
      .separated_by_comma_while::<_, U2>(cond)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let ctx: ParserContext<'_, TLexer<'_>, SeparatorFatal> = ParserContext::new(SeparatorFatal);
  let out = Parser::with_parser_and_context(parse, ctx)
    .parse_str_with_state("(1 2 3", ScanLimiter::with_limit(3));
  assert_eq!(
    out,
    Err(CErr::Eot(true)),
    "a mid-window terminal stop must outrank the missing-separator diagnostic a `State::Element` \
     continue would emit: expected the terminal-marked end-of-input error, got {out:?}"
  );
}

#[test]
fn condition_error_does_not_preempt_a_mid_window_terminal_stop() {
  // Window `U2`, limit 2, input `(1 2`: the opener `(` (1st scan) and the front `Num` (2nd scan)
  // scan cleanly, but the second window slot trips (3rd scan) — `peek_with_emitter_terminal`
  // returns a one-token window with `terminal == true`, not an empty one. The condition treats any
  // truncated (`len < W`) window as untrustworthy input and returns an ordinary error. That fallible
  // `decide` runs before any per-arm terminal check and before the close probe, so an eager terminal
  // gate must surface the stop ahead of `decide`: otherwise the ordinary error preempts it.
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<CErr>>>,
  ) -> Result<Vec<i64>, CErr> {
    let cond = |mut peeked: Peeked<'_, 'inp, TLexer<'inp>, U2>,
                _emitter: EmitterView<'_, 'inp, TLexer<'inp>, Silent<CErr>>|
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
      .separated_by_comma_while::<_, U2>(cond)
      .delimited::<Paren<(), (), ()>>()
      .collect()
      .parse_input(inp)
  }

  let ctx: ParserContext<'_, TLexer<'_>, Silent<CErr>> = ParserContext::new(Silent::new());
  let out = Parser::with_parser_and_context(parse, ctx)
    .parse_str_with_state("(1 2", ScanLimiter::with_limit(2));
  assert_eq!(
    out,
    Err(CErr::Eot(true)),
    "a fallible condition on a trip-truncated window must not preempt the terminal stop inside \
     delimiters: expected the terminal-marked end-of-input error, got {out:?}"
  );
}
