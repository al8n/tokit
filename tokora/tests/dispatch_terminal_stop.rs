#![cfg(all(
  feature = "std",
  feature = "combinators",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Dispatch terminal-witness regressions.
//!
//! A prior WIDE peek prefills the cache with a real, non-table token AND latches a poison boundary
//! after it (leaving the *lex offset* at/past the boundary while the *committed cursor* lags at the
//! front). A dispatch whose classifier then MISSES that cached token must NOT be reported as a
//! terminal scanner stop — the cached token is definite evidence of a real (non-table) token, so
//! the dispatch declines (tentative route) / reports an ordinary unexpected token (committed
//! routes), letting an enclosing choice take another alternative. A GENUINE scan trip at the
//! dispatch position must still surface terminal, and a genuine end of input must stay recoverable.
//!
//! Of the three dispatch sites that read the input's terminal state, only the **fused tentative**
//! [`TryParseInput::try_parse_input`] was susceptible: its `None` arm conflated a cache-hit
//! classifier miss with end-of-input and witnessed the terminal state with the lex-offset
//! `at_latched_boundary`, so a miss against a prefilled cache latched ahead of the cursor was
//! mis-charged as terminal and blocked an enclosing choice. It now routes through the
//! attempt-relative `try_expect_map_or_stop`. The two committed routes (fused
//! `ParseInput::parse_input` and non-fused `DispatchOnKind::parse_input`) were already correct: a
//! cache-hit miss surfaces a recoverable unexpected token through their miss branch, and their
//! terminal witness runs only on the empty-cache end-of-input arm, where the lex offset equals the
//! committed cursor. Their cases below hold on the pre-fix tip too and pin that they stay correct.

use core::cell::Cell;
use std::rc::Rc;

use generic_arraydeque::typenum::U4;
use tokora::{
  Emitter, InputRef, Parse, ParseChoice, ParseContext, ParseInput, ParseTokenChoice, Parser,
  ParserContext, SimpleSpan, Token as TokenTrait, TryParseInput,
  error::{MaybeIncomplete, MaybeTerminal, UnexpectedEot, token::UnexpectedToken},
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::Any,
  span::Spanned,
  state::State,
  try_parse_input::ParseAttempt,
};

// ── Scan limiter whose counter is shared across every cloned lexer ────────────

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
  Num,
  Comma,
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
    })
  }
}

impl TokenTrait<'_> for Tok {
  type Kind = Kind;
  type Error = DErr;

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
  fn kind(&self) -> Kind {
    match self {
      Tok::Num(_) => Kind::Num,
      Tok::Comma => Kind::Comma,
    }
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

// ── Error type: distinguishes a TERMINAL eot from a plain eot and an unexpected token ──

#[derive(Debug, Clone, PartialEq)]
enum DErr {
  /// A terminal scanner stop (UnexpectedEot marked terminal).
  Terminal,
  /// A plain, recoverable end of input.
  Eot,
  /// An ordinary unexpected token (the committed dispatch miss diagnostic).
  Token,
  /// Anything else (a lexer/limit error).
  Other,
}

impl From<()> for DErr {
  fn from(_: ()) -> Self {
    DErr::Other
  }
}
impl From<ScanLimitExceeded> for DErr {
  fn from(_: ScanLimitExceeded) -> Self {
    DErr::Other
  }
}
// Generic over `Set` so it covers BOTH the committed routes' `UnexpectedEot<_, _, Kind>` and the
// default-`Set` `UnexpectedEot<_, _, &'static str>` an `_or_stop` primitive raises.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for DErr {
  fn from(e: UnexpectedEot<O, Lang, Set>) -> Self {
    if e.is_terminal() {
      DErr::Terminal
    } else {
      DErr::Eot
    }
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for DErr {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    DErr::Token
  }
}
impl MaybeTerminal for DErr {
  fn is_terminal(&self) -> bool {
    matches!(self, DErr::Terminal)
  }
}
impl MaybeIncomplete for DErr {}

// ── Harness ──────────────────────────────────────────────────────────────────

type TLexer<'a> = LogosLexer<'a, Tok>;

fn drive<'inp, O, Em>(
  emitter: Em,
  limiter: ScanLimiter,
  f: impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Em>>,
  ) -> Result<O, DErr>,
  input: &'inp str,
) -> Result<O, DErr>
where
  Em: Emitter<'inp, TLexer<'inp>, Error = DErr>,
{
  let ctx: ParserContext<'inp, TLexer<'inp>, Em> = ParserContext::new(emitter);
  Parser::with_parser_and_context(f, ctx).parse_str_with_state(input, limiter)
}

/// A fused arm: it never runs on the miss/decline paths under test, so its body is immaterial.
fn fused_arm<'inp, Ctx>(
  _head: Spanned<Tok, SimpleSpan>,
  _inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>,
) -> Result<i64, DErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>, Emitter: Emitter<'inp, TLexer<'inp>, Error = DErr>>,
{
  Ok(0)
}

// Prefill helper: over "1 2 3" under limit 2 a wide peek caches `1`,`2` and trips on the 3rd scan,
// latching the boundary — leaving the lex offset at/past the boundary while the cursor stays at the
// front and the cache front is `1` (a Num). The dispatch table below is [Comma], so the classifier
// misses that cached Num.

// ══ Fused TENTATIVE (try_parse_input) — the bug: a cache-hit miss must DECLINE, not go terminal ══
#[test]
fn fused_tentative_cache_hit_miss_declines_not_terminal() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(2),
    |inp| {
      let _ = inp.peek::<U4>()?; // prefill + latch
      (fused_arm,)
        .fused_dispatch_on_kind(&[Kind::Comma])
        .try_parse_input(inp)
    },
    "1 2 3",
  );
  assert_eq!(
    out,
    Ok(ParseAttempt::Decline),
    "a cache-hit classifier miss with a prior-latched boundary must DECLINE (definite absence), \
     not be mis-charged as a terminal stop; got {out:?}"
  );
}

// ══ Fused COMMITTED (parse_input) — a cache-hit miss stays a recoverable UnexpectedToken ══
#[test]
fn fused_committed_cache_hit_miss_is_recoverable_token() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(2),
    |inp| {
      let _ = inp.peek::<U4>()?;
      (fused_arm,)
        .fused_dispatch_on_kind(&[Kind::Comma])
        .parse_input(inp)
    },
    "1 2 3",
  );
  assert_eq!(
    out,
    Err(DErr::Token),
    "a cache-hit classifier miss on the committed route is a recoverable unexpected token, \
     never a terminal stop; got {out:?}"
  );
}

// ══ Non-fused COMMITTED (parse_input) — a cache-hit miss stays a recoverable UnexpectedToken ══
#[test]
fn nonfused_committed_cache_hit_miss_is_recoverable_token() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(2),
    |inp| {
      let _ = inp.peek::<U4>()?;
      (Any::of(),)
        .dispatch_on_kind(&[Kind::Comma])
        .parse_input(inp)
        .map(|_| 0i64)
    },
    "1 2 3",
  );
  assert_eq!(
    out,
    Err(DErr::Token),
    "a cache-hit classifier miss on the non-fused route is a recoverable unexpected token, \
     never a terminal stop; got {out:?}"
  );
}

// ── Genuine scan trip at the dispatch position still surfaces terminal ─────────

#[test]
fn fused_tentative_genuine_trip_surfaces_terminal() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(2),
    |inp| {
      assert!(inp.next()?.is_some());
      assert!(inp.next()?.is_some());
      (fused_arm,)
        .fused_dispatch_on_kind(&[Kind::Comma])
        .try_parse_input(inp)
    },
    "1 2 3",
  );
  assert_eq!(
    out,
    Err(DErr::Terminal),
    "a genuine trip at the dispatch position surfaces the terminal end-of-input error; got {out:?}"
  );
}

#[test]
fn fused_committed_genuine_trip_surfaces_terminal() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(2),
    |inp| {
      assert!(inp.next()?.is_some());
      assert!(inp.next()?.is_some());
      (fused_arm,)
        .fused_dispatch_on_kind(&[Kind::Comma])
        .parse_input(inp)
    },
    "1 2 3",
  );
  assert_eq!(out, Err(DErr::Terminal), "got {out:?}");
}

#[test]
fn nonfused_committed_genuine_trip_surfaces_terminal() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(2),
    |inp| {
      assert!(inp.next()?.is_some());
      assert!(inp.next()?.is_some());
      (Any::of(),)
        .dispatch_on_kind(&[Kind::Comma])
        .parse_input(inp)
        .map(|_| 0i64)
    },
    "1 2 3",
  );
  assert_eq!(out, Err(DErr::Terminal), "got {out:?}");
}

// ── Genuine end of input at the dispatch position stays a plain, recoverable eot ───

#[test]
fn fused_committed_genuine_eof_is_recoverable() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      assert!(inp.next()?.is_some());
      assert!(inp.next()?.is_some());
      (fused_arm,)
        .fused_dispatch_on_kind(&[Kind::Comma])
        .parse_input(inp)
    },
    "1 2",
  );
  assert_eq!(out, Err(DErr::Eot), "got {out:?}");
}

#[test]
fn nonfused_committed_genuine_eof_is_recoverable() {
  let out = drive(
    tokora::emitter::Silent::<DErr>::new(),
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      assert!(inp.next()?.is_some());
      assert!(inp.next()?.is_some());
      (Any::of(),)
        .dispatch_on_kind(&[Kind::Comma])
        .parse_input(inp)
        .map(|_| 0i64)
    },
    "1 2",
  );
  assert_eq!(out, Err(DErr::Eot), "got {out:?}");
}
