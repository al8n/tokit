//! The `select!` / `try_select!` protocol, exercised at both levels.
//!
//! The runtime (`dispatch_take` / `try_dispatch_take`) owns the semantics, so most cells
//! call it directly; the macro cells exist to pin the *grammar* it expands to. Every
//! outcome the protocol distinguishes has a cell here:
//!
//! | | committed | tentative |
//! |---|---|---|
//! | head in the table | commit + move into the arm | `Accept` |
//! | head outside it | table-shaped unexpected-token error | `Decline`, nothing consumed |
//! | genuine end of input | table-shaped EOT, **not** terminal | `Decline` |
//! | latched boundary | table-shaped EOT, **terminal** | error, never a decline |
//! | kind matched, variant did not | give-back → typed error, no panic | same |
//! | open partial frontier | `Incomplete` | `Incomplete` |
//!
//! The last row is the one the `Cmpl` widening exists for and the one a `is_final = true`
//! fixture cannot see: at an exhausted but unsealed chunk `Partial` and `Complete` differ,
//! and at exhaustion-with-`is_final` they do not.

use core::cell::Cell;
use std::rc::Rc;

use crate::{
  Token,
  cache::DefaultCache,
  emitter::Silent,
  error::{
    Incomplete, MaybeIncomplete, MaybeTerminal, Unclosed, UnexpectedEot, token::UnexpectedToken,
  },
  input::{Input, Partial},
  lexer::LogosLexer,
  state::State,
  try_parse_input::ParseAttempt,
};

// ── A limiter whose scan counter is SHARED across every cloned lexer ──────────
//
// Mirrors `terminal_stop_tests::Limiter`: `InputRef` rebuilds a lexer per operation by
// cloning the state, so a by-value counter would hide re-scans.
#[derive(Debug, Clone, Default)]
struct Limiter {
  scanned: Rc<Cell<usize>>,
  limit: usize,
}

impl Limiter {
  fn with_limit(limit: usize) -> Self {
    Self {
      scanned: Rc::new(Cell::new(0)),
      limit,
    }
  }

  fn counter(&self) -> Rc<Cell<usize>> {
    self.scanned.clone()
  }

  fn increase(&self) {
    self.scanned.set(self.scanned.get() + 1);
  }
}

#[derive(Debug, Clone, PartialEq)]
struct LimitExceeded;

impl State for Limiter {
  type Error = LimitExceeded;

  fn check(&self) -> Result<(), Self::Error> {
    if self.scanned.get() > self.limit {
      Err(LimitExceeded)
    } else {
      Ok(())
    }
  }
}

/// The emitter error, shaped so every outcome the protocol distinguishes is a distinct
/// value — including the **terminal mark**, which a `From` impl that drops the flag
/// would silently erase (that is exactly what makes a cell on "is it an `Err`?"
/// vacuous).
#[derive(Debug, Clone, PartialEq)]
enum SelErr {
  /// A table-shaped end-of-token-stream error that is **not** marked terminal.
  Eot,
  /// The same, marked terminal — a limit trip or a latched poison boundary.
  Terminal,
  /// An unexpected token, carrying what the runtime put in it.
  Unexpected {
    found: Option<SelKind>,
    expected: usize,
  },
  /// The partial-input frontier sentinel.
  Incomplete,
  Other,
}

impl From<()> for SelErr {
  fn from(_: ()) -> Self {
    SelErr::Other
  }
}

impl From<LimitExceeded> for SelErr {
  fn from(_: LimitExceeded) -> Self {
    SelErr::Other
  }
}

impl<O> From<Incomplete<O>> for SelErr {
  fn from(_: Incomplete<O>) -> Self {
    SelErr::Incomplete
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for SelErr {
  fn from(e: UnexpectedEot<O, Lang, Set>) -> Self {
    if e.is_terminal() {
      SelErr::Terminal
    } else {
      SelErr::Eot
    }
  }
}

impl<'a, S, Lang: ?Sized> From<UnexpectedToken<'a, SelTok, SelKind, S, Lang>> for SelErr {
  fn from(err: UnexpectedToken<'a, SelTok, SelKind, S, Lang>) -> Self {
    let (_span, found, expected) = err.into_components();
    SelErr::Unexpected {
      found: found.map(|t| t.kind()),
      expected: match expected {
        Some(crate::utils::Expected::OneOf(set)) => set.as_slice().len(),
        Some(crate::utils::Expected::One(_)) => 1,
        None => 0,
      },
    }
  }
}

impl<'inp, L, Lang: ?Sized> crate::emitter::FromUnclosed<'inp, L, Lang> for SelErr
where
  L: crate::Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    SelErr::Other
  }
}

impl MaybeTerminal for SelErr {
  fn is_terminal(&self) -> bool {
    matches!(self, SelErr::Terminal)
  }
}

impl MaybeIncomplete for SelErr {
  fn is_incomplete(&self) -> bool {
    matches!(self, SelErr::Incomplete)
  }
}

// ── The token vocabulary ──────────────────────────────────────────────────────
//
// `Int` carries a payload so the **moved** projection is observable (an arm that only
// borrowed could not return it), and `Int` has more than one inhabitant so an arm
// pattern can be *narrower than its kind* — the give-back arm's only reachable shape.
#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = Limiter, skip r"[ \t\r\n]+")]
enum SelTok {
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); lex.slice().parse::<i64>().ok() })]
  Int(i64),
  #[regex(r"[0-9]+\.[0-9]+", |lex| { lex.extras.increase(); lex.slice().to_string() }, priority = 4)]
  Float(String),
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| { lex.extras.increase(); })]
  Word,
}

impl core::fmt::Display for SelTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SelKind {
  Int,
  Float,
  Word,
}

impl core::fmt::Display for SelKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

impl Token<'_> for SelTok {
  type Kind = SelKind;
  type Error = SelErr;

  const SCAN_LOOKAHEAD: crate::ScanLookahead = crate::ScanLookahead::Unbounded;

  fn kind(&self) -> SelKind {
    match self {
      SelTok::Int(_) => SelKind::Int,
      SelTok::Float(_) => SelKind::Float,
      SelTok::Word => SelKind::Word,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type SelLexer<'a> = LogosLexer<'a, SelTok>;
type SelCtx<'a> = (Silent<SelErr>, DefaultCache<'a, SelLexer<'a>>);

/// A complete input behind a limit-2 [`Limiter`]: the third scanned token trips it.
fn sel_input(src: &str) -> (Input<'_, SelLexer<'_>, SelCtx<'_>, ()>, Rc<Cell<usize>>) {
  let limiter = Limiter::with_limit(2);
  let scanned = limiter.counter();
  let context = crate::input::InputContext::new(
    Silent::<SelErr>::new(),
    DefaultCache::<'_, SelLexer<'_>>::default(),
  );
  let input = Input::<SelLexer<'_>, SelCtx<'_>, ()>::with_state_and_context(src, limiter, context);
  (input, scanned)
}

/// A **partial** input over `src`, left OPEN (unsealed) — `is_final` is `false`, which is
/// the only setting where `Partial` and `Complete` disagree at exhaustion.
fn sel_partial(src: &str) -> Input<'_, SelLexer<'_>, SelCtx<'_>, (), Partial> {
  let limiter = Limiter::with_limit(64);
  let context = crate::input::InputContext::new(
    Silent::<SelErr>::new(),
    DefaultCache::<'_, SelLexer<'_>>::default(),
  );
  Input::<SelLexer<'_>, SelCtx<'_>, (), Partial>::with_state_and_context(src, limiter, context)
}

const NUMERIC: &[SelKind] = &[SelKind::Int, SelKind::Float];

// ── The committed runtime ─────────────────────────────────────────────────────

#[test]
fn dispatch_take_hits_and_moves_the_payload_into_the_arm() {
  let (mut input, _scanned) = sel_input("7");
  let mut inp = input.as_ref();
  // The arm receives the token BY VALUE: it returns the payload itself, which an arm
  // that only borrowed the head could not do.
  let out: Result<i64, SelErr> =
    crate::parser::dispatch_take(&mut inp, NUMERIC, |sp| match sp.into_components() {
      (_span, SelTok::Int(v)) => Ok(v),
      (span, other) => Err(crate::span::Spanned::new(span, other)),
    });
  assert_eq!(out, Ok(7));
}

#[test]
fn dispatch_take_miss_errors_with_the_whole_table_and_the_found_token() {
  let (mut input, _scanned) = sel_input("word");
  let mut inp = input.as_ref();
  let out: Result<i64, SelErr> = crate::parser::dispatch_take(&mut inp, NUMERIC, |sp| {
    Err(crate::span::Spanned::new(sp.span, sp.data))
  });
  assert_eq!(
    out,
    Err(SelErr::Unexpected {
      found: Some(SelKind::Word),
      expected: NUMERIC.len(),
    }),
    "a miss carries the found token AND the whole table — a single-kind expected set \
     would be the fused driver's per-arm error, which is the shape this replaces"
  );
}

#[test]
fn dispatch_take_give_back_arm_builds_a_typed_error_and_does_not_panic() {
  // The law-violation arm: the kind is in the table, but the arm pattern is NARROWER
  // than its kind (`Int(0)` under `Kind::Int`). The projection hands the token back and
  // the runtime — where `Lang` is pinned — builds the error. `unreachable!()` here is
  // what the fused-dispatch sites write by hand; this cell is what replaces it.
  let (mut input, _scanned) = sel_input("7");
  let mut inp = input.as_ref();
  let out: Result<i64, SelErr> = crate::select!(&mut inp, {
    SelKind::Int => (_span, SelTok::Int(0)) => 0i64,
    SelKind::Float => (_span, SelTok::Float(_)) => 1i64,
  });
  assert_eq!(
    out,
    Err(SelErr::Unexpected {
      found: Some(SelKind::Int),
      expected: 2,
    }),
    "an arm narrower than its kind gives the token back and gets a typed error, not a \
     panic and not a decline"
  );
}

#[test]
fn dispatch_take_marks_terminal_only_at_a_latch() {
  // Genuine end of input: an EOT error, explicitly NOT terminal.
  {
    let (mut input, _scanned) = sel_input("1");
    let mut inp = input.as_ref();
    assert!(inp.next().unwrap().is_some());
    let out: Result<usize, SelErr> = crate::parser::dispatch_take(&mut inp, NUMERIC, |sp| {
      Ok(crate::span::Span::start(&sp.span))
    });
    assert_eq!(
      out,
      Err(SelErr::Eot),
      "genuine end of input is an EOT error that is NOT terminal"
    );
  }
  // Latched boundary: the same shape, marked terminal.
  {
    let (mut input, _scanned) = sel_input("1 2 3");
    let mut inp = input.as_ref();
    assert!(inp.next().unwrap().is_some());
    assert!(inp.next().unwrap().is_some());
    assert!(
      inp.next().unwrap().is_none(),
      "the third scan trips and latches"
    );
    let out: Result<usize, SelErr> = crate::parser::dispatch_take(&mut inp, NUMERIC, |sp| {
      Ok(crate::span::Span::start(&sp.span))
    });
    assert_eq!(
      out,
      Err(SelErr::Terminal),
      "a latched boundary marks dispatch_take's EOT terminal"
    );
  }
}

// ── The tentative runtime ─────────────────────────────────────────────────────

#[test]
fn try_dispatch_take_declines_a_miss_without_consuming() {
  let (mut input, _scanned) = sel_input("word 7");
  let mut inp = input.as_ref();
  let out: Result<ParseAttempt<i64>, SelErr> =
    crate::parser::try_dispatch_take(&mut inp, NUMERIC, |sp| {
      Err(crate::span::Spanned::new(sp.span, sp.data))
    });
  assert_eq!(out, Ok(ParseAttempt::Decline));
  // Zero consumption: the declined head is still there for the next reader.
  let head = inp
    .next()
    .unwrap()
    .expect("the declined token was not consumed");
  assert_eq!(head.data(), &SelTok::Word);
}

#[test]
fn try_dispatch_take_declines_genuine_eof_but_errs_on_a_terminal_stop() {
  {
    let (mut input, _scanned) = sel_input("1");
    let mut inp = input.as_ref();
    assert!(inp.next().unwrap().is_some());
    let out: Result<ParseAttempt<usize>, SelErr> =
      crate::parser::try_dispatch_take(&mut inp, NUMERIC, |sp| {
        Ok(crate::span::Span::start(&sp.span))
      });
    assert_eq!(
      out,
      Ok(ParseAttempt::Decline),
      "genuine end of input declines"
    );
  }
  {
    let (mut input, _scanned) = sel_input("1 2 3");
    let mut inp = input.as_ref();
    assert!(inp.next().unwrap().is_some());
    assert!(inp.next().unwrap().is_some());
    assert!(inp.next().unwrap().is_none());
    let out: Result<ParseAttempt<usize>, SelErr> =
      crate::parser::try_dispatch_take(&mut inp, NUMERIC, |sp| {
        Ok(crate::span::Span::start(&sp.span))
      });
    assert_eq!(
      out,
      Err(SelErr::Terminal),
      "a terminal stop is an error, never a decline — that IS the or_stop contract"
    );
  }
}

// ── The `Cmpl` widening's own frontier ────────────────────────────────────────
//
// Both cells run with the chunk exhausted and `is_final = false`. Sealing the input
// (`is_final = true`) turns the first into `Err(Eot)` and the second into
// `Ok(Decline)` — which is why a fixture that only ever seals cannot see the widening.
//
// The chunk is deliberately trivia-only. These cells pin **rule 3**, non-final EOF, and
// under 0.10.0 the way to reach it with this fixture is to produce no item at all: `SelTok`
// has `[0-9]+` beside `[0-9]+\.[0-9]+`, so the logos DFA probes into the float arm and
// backtracks, the vocabulary cannot honestly claim
// `ScanLookahead::WithinSpan`, so it declares `Unbounded` and the holdback (rule 1)
// withholds every item while the stream is open. Consuming a token first would therefore
// stop at rule 1 and quietly relocate what these two cells prove. See
// `partial_tests::unbounded_reporter_withholds_every_item_until_sealed` for the cell that
// owns that behaviour.

#[test]
fn dispatch_take_surfaces_incomplete_at_an_open_partial_frontier() {
  let mut input = sel_partial(" ");
  let mut inp = input.as_ref();
  let out: Result<usize, SelErr> = crate::select!(&mut inp, {
    SelKind::Int => (span, SelTok::Int(_)) => crate::span::Span::start(&span),
  });
  assert_eq!(
    out,
    Err(SelErr::Incomplete),
    "an exhausted but UNSEALED chunk is incomplete, not a genuine end of token stream"
  );
}

#[test]
fn try_dispatch_take_surfaces_incomplete_at_an_open_partial_frontier() {
  let mut input = sel_partial(" ");
  let mut inp = input.as_ref();
  let out: Result<ParseAttempt<usize>, SelErr> = crate::try_select!(&mut inp, {
    SelKind::Int => (span, SelTok::Int(_)) => crate::span::Span::start(&span),
  });
  assert_eq!(
    out,
    Err(SelErr::Incomplete),
    "the tentative twin re-raises the frontier instead of declining it — a decline would \
     hand a sibling alternative an input that has not ended"
  );
}

/// The sealed counterpart of the two cells above, which the section comment asserts in prose:
/// with `is_final = true` the frontier rules are inert and the same exhausted chunk is a genuine
/// end of token stream — `Err(Eot)` for the committed form, `Ok(Decline)` for the tentative one.
#[test]
fn a_sealed_chunk_is_eot_and_decline_not_incomplete() {
  {
    let mut input = sel_partial(" ");
    input.seal();
    let mut inp = input.as_ref();
    let out: Result<usize, SelErr> = crate::select!(&mut inp, {
      SelKind::Int => (span, SelTok::Int(_)) => crate::span::Span::start(&span),
    });
    assert_eq!(out, Err(SelErr::Eot), "a sealed exhausted chunk is genuine");
  }
  {
    let mut input = sel_partial(" ");
    input.seal();
    let mut inp = input.as_ref();
    let out: Result<ParseAttempt<usize>, SelErr> = crate::try_select!(&mut inp, {
      SelKind::Int => (span, SelTok::Int(_)) => crate::span::Span::start(&span),
    });
    assert_eq!(
      out,
      Ok(ParseAttempt::Decline),
      "the tentative twin declines a genuine end of token stream"
    );
  }
}

// ── The macro surface ─────────────────────────────────────────────────────────

#[test]
fn select_macro_surface_single_arm_no_trailing_comma() {
  let (mut input, _scanned) = sel_input("7");
  let mut inp = input.as_ref();
  let out: Result<i64, SelErr> = crate::select!(&mut inp, {
    SelKind::Int => (_span, SelTok::Int(v)) => v
  });
  assert_eq!(out, Ok(7));
}

#[test]
fn select_macro_surface_or_pattern_arm() {
  // One arm, two kinds, an or-pattern payload: the table is built from the kind column
  // and the match from the pattern column, so they need not be 1:1.
  for (src, want) in [("7", 1i64), ("7.5", 2i64)] {
    let (mut input, _scanned) = sel_input(src);
    let mut inp = input.as_ref();
    let out: Result<i64, SelErr> = crate::select!(&mut inp, {
      SelKind::Int => (_span, SelTok::Int(_)) => 1i64,
      SelKind::Float => (_span, SelTok::Float(_) | SelTok::Word) => 2i64,
    });
    assert_eq!(out, Ok(want), "source {src:?}");
  }
}

#[test]
fn select_macro_surface_binds_the_span_into_the_user_arm() {
  let (mut input, _scanned) = sel_input("  42");
  let mut inp = input.as_ref();
  let out: Result<(usize, usize, i64), SelErr> = crate::select!(&mut inp, {
    SelKind::Int => (span, SelTok::Int(v)) => (span.start(), span.end(), v),
  });
  assert_eq!(
    out,
    Ok((2, 4, 42)),
    "the arm sees the token's own span, not the input's committed one"
  );
}

#[test]
fn try_select_macro_surface_trailing_comma_and_decline() {
  let (mut input, _scanned) = sel_input("word");
  let mut inp = input.as_ref();
  let out: Result<ParseAttempt<i64>, SelErr> = crate::try_select!(&mut inp, {
    SelKind::Int => (_span, SelTok::Int(v)) => v,
    SelKind::Float => (_span, SelTok::Float(_)) => 0i64,
  });
  assert_eq!(out, Ok(ParseAttempt::Decline));
}
