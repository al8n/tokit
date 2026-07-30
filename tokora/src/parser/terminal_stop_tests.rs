//! Terminal-stop regressions for the **committed-consume** layer.
//!
//! A committed leaf consumes with [`next_or_stop`](crate::InputRef::next_or_stop) and turns a
//! `None` outcome into an [`UnexpectedEot`](crate::error::UnexpectedEot). Before that primitive
//! existed the leaves consumed with [`next`](crate::InputRef::next), whose `Scan::Tripped` and
//! `Scan::Eof` both collapse to `Ok(None)`, so a fresh resource-limit trip at an expected token
//! built a *plain* (non-terminal) end-of-input error — one [`Recover`] then swallowed, synthesizing
//! a value and re-entering the scanner to re-trip the same limit.
//!
//! These tests pin, for **every** committed leaf, the two outcomes the fold used to merge:
//!
//! - a **fresh limit trip** at the leaf's expected token surfaces a *terminal* end-of-input error
//!   ([`TsErr::Terminal`]), which [`Recover`] re-raises instead of recovering;
//! - a **genuine end of input** at the same position stays a *plain* end-of-input error
//!   ([`TsErr::Eot`]), recoverable exactly as before.
//!
//! The trip is arranged to fire *inside the leaf's own consume*: a limit-2 lexer is driven two
//! tokens forward first, so the leaf's [`next_or_stop`](crate::InputRef::next_or_stop) is the third
//! scan — the one that trips. The trip preempts the leaf's token-type check, so the same numeric
//! input exercises every leaf regardless of the token type it expects.

use core::cell::Cell;
use std::rc::Rc;

use crate::{
  Check, Emitter, InputRef, ParseContext, ParseInput, Token,
  cache::DefaultCache,
  emitter::Silent,
  error::{MaybeIncomplete, MaybeTerminal, Unclosed, UnexpectedEot, token::UnexpectedToken},
  input::Input,
  lexer::LogosLexer,
  parser::{Any, Recover, RecoverInput, delimited, expect},
  punct::{CloseParen, OpenParen, Paren},
  state::State,
  types::{Ident, Keyword},
  utils::Expected,
};

// ── A limiter whose scan counter is SHARED across every cloned lexer ──────────
//
// `InputRef` rebuilds a fresh lexer per operation by cloning the state, so a by-value counter would
// hide re-scans. Sharing it through an `Rc<Cell<_>>` makes every scanned token observable and lets a
// test prove the leaf's consume — not an earlier one — is the scan that trips. Mirrors the
// `ProbeLimiter` behind the input-layer scan tests.
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

/// The emitter error type, chosen to keep the terminal marker observable. A committed leaf's
/// end-of-input value arrives here through `From<UnexpectedEot>`; the terminal marker it may carry
/// (set by [`next_or_stop`](crate::InputRef::next_or_stop)) is what separates [`Terminal`] from
/// [`Eot`], which is exactly the bit [`Recover`] keys the never-recoverable dual off of.
///
/// [`Terminal`]: TsErr::Terminal
/// [`Eot`]: TsErr::Eot
#[derive(Debug, Clone, PartialEq)]
enum TsErr {
  /// A terminal scanner stop — the committed leaf marked its end-of-input error terminal.
  Terminal,
  /// A genuine end of input — a plain, recoverable end-of-input error.
  Eot,
  /// Any other diagnostic routed through the emitter (a lexer/limit error, an unexpected token, an
  /// unclosed delimiter). None is reached on the trip/EOF arms these tests assert on.
  Other,
}

impl From<()> for TsErr {
  fn from(_: ()) -> Self {
    TsErr::Other
  }
}

impl From<LimitExceeded> for TsErr {
  fn from(_: LimitExceeded) -> Self {
    TsErr::Other
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for TsErr {
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    TsErr::Other
  }
}

impl<'inp, L, Lang: ?Sized> crate::emitter::FromUnclosed<'inp, L, Lang> for TsErr
where
  L: crate::Lexer<'inp>,
{
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    TsErr::Other
  }
}

// The construct-and-detect pairing that makes the terminal split observable: a terminal-marked
// end-of-input value (the way `next_or_stop` builds one on a trip) maps to `Terminal`, a genuine one
// to `Eot`.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for TsErr {
  fn from(e: UnexpectedEot<O, Lang, Set>) -> Self {
    if e.is_terminal() {
      TsErr::Terminal
    } else {
      TsErr::Eot
    }
  }
}

impl MaybeTerminal for TsErr {
  fn is_terminal(&self) -> bool {
    matches!(self, TsErr::Terminal)
  }
}

impl MaybeIncomplete for TsErr {}

#[derive(Debug, Clone, PartialEq, crate::logos::Logos)]
#[logos(crate = crate::logos, extras = Limiter, skip r"[ \t\r\n]+")]
enum LimTok {
  // Only the numeric token increments the shared counter, so a numeric input of known length trips
  // the limiter at a known scan. The other variants exist purely so `LimTok` can satisfy the
  // identifier/keyword/punctuator trait bounds of the leaves under test; the tests never lex them.
  #[regex(r"[0-9]+", |lex| { lex.extras.increase(); })]
  Num,
  #[token("if")]
  If,
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
  Ident,
  #[token("(")]
  OpenParen,
  #[token(")")]
  CloseParen,
  #[token(",")]
  Comma,
}

impl core::fmt::Display for LimTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LimKind {
  Num,
  If,
  Ident,
  OpenParen,
  CloseParen,
  Comma,
}

impl core::fmt::Display for LimKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

// `Paren`'s `TypedDelimiter` impl maps its zero-sized opener/closer markers into the token's kind
// through these conversions — the delimiter counterpart of the `PunctuatorToken::open_paren`/
// `close_paren` overrides above.
impl From<OpenParen<(), (), ()>> for LimKind {
  fn from(_: OpenParen<(), (), ()>) -> Self {
    LimKind::OpenParen
  }
}

impl From<CloseParen<(), (), ()>> for LimKind {
  fn from(_: CloseParen<(), (), ()>) -> Self {
    LimKind::CloseParen
  }
}

impl Token<'_> for LimTok {
  type Kind = LimKind;
  type Error = TsErr;

  fn kind(&self) -> LimKind {
    match self {
      LimTok::Num => LimKind::Num,
      LimTok::If => LimKind::If,
      LimTok::Ident => LimKind::Ident,
      LimTok::OpenParen => LimKind::OpenParen,
      LimTok::CloseParen => LimKind::CloseParen,
      LimTok::Comma => LimKind::Comma,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl crate::token::IdentifierToken<'_> for LimTok {
  fn is_identifier(&self) -> bool {
    matches!(self, LimTok::Ident)
  }
}

impl crate::token::KeywordToken<'_> for LimTok {
  fn keyword(&self) -> Option<&'static str> {
    match self {
      LimTok::If => Some("if"),
      _ => None,
    }
  }
}

impl crate::token::PunctuatorToken<'_> for LimTok {
  fn open_paren() -> Option<Self::Kind> {
    Some(LimKind::OpenParen)
  }
  fn close_paren() -> Option<Self::Kind> {
    Some(LimKind::CloseParen)
  }
  fn comma() -> Option<Self::Kind> {
    Some(LimKind::Comma)
  }
}

// A local invocation of the `keyword!` macro itself — the regression below exercises the macro
// TEMPLATE's own generated `parse`, distinct from the hand-written `Keyword`
// (any-keyword) leaf already covered above. The trip preempts the leaf's token-type check (see
// the module doc), so the keyword text here is never actually matched against a lexed token.
crate::keyword! {
  (KwIf, "KW_IF", "if"),
}

type LimLexer<'a> = LogosLexer<'a, LimTok>;
type TsCtx<'a> = (Silent<TsErr>, DefaultCache<'a, LimLexer<'a>>);
// The emitter's error projection, spelled the way the `RecoverInput` trait signature spells it (it
// resolves to `TsErr`, but the trait-compatibility check wants the projection, not the resolved
// type) — the same shape `recover.rs`'s own recoverers use.
type TsEmitErr<'a> = <<TsCtx<'a> as ParseContext<'a, LimLexer<'a>, ()>>::Emitter as Emitter<
  'a,
  LimLexer<'a>,
  (),
>>::Error;

/// Builds an input over `src` behind a limit-2 [`Limiter`]: the third scanned numeric token trips
/// the limiter. Returns the input and a shared handle to observe the scan counter.
fn ts_input(src: &str) -> (Input<'_, LimLexer<'_>, TsCtx<'_>, ()>, Rc<Cell<usize>>) {
  let limiter = Limiter::with_limit(2);
  let scanned = limiter.counter();
  let context = crate::input::InputContext::new(
    Silent::<TsErr>::new(),
    DefaultCache::<'_, LimLexer<'_>>::default(),
  );
  let input = Input::<LimLexer<'_>, TsCtx<'_>, ()>::with_state_and_context(src, limiter, context);
  (input, scanned)
}

/// A classifier for [`expect`] that accepts the numeric token. Its `check` never runs on the
/// trip/EOF arms — the leaf's `next_or_stop` resolves the outcome before the token is classified —
/// so its verdict is immaterial; it exists to satisfy `expect`'s classifier bound.
struct NumClass;

impl<'a> Check<LimTok, Result<(), Expected<'a, LimKind>>> for NumClass {
  fn check(&self, t: &LimTok) -> Result<(), Expected<'a, LimKind>> {
    if matches!(t, LimTok::Num) {
      Ok(())
    } else {
      Err(Expected::one(LimKind::Num))
    }
  }
}

/// Generates the two-outcome regression for one committed leaf.
///
/// `$leaf` is the leaf's committed consume, written as `|inp| <call using inp>` where `inp` is a
/// `&mut InputRef`. Each block drives a limit-2 lexer two tokens forward, so the leaf's own
/// `next_or_stop` is the third scan: over `"1 2 3"` that third scan trips (terminal), and over
/// `"1 2"` it reaches genuine end of input (plain).
macro_rules! committed_leaf_terminal_stop {
  ($name:ident, |$ir:ident| $leaf:expr) => {
    #[test]
    fn $name() {
      // A fresh trip at the leaf's expected token → a TERMINAL end-of-input error.
      {
        let (mut input, scanned) = ts_input("1 2 3");
        let mut $ir = input.as_ref();
        assert!($ir.next().unwrap().is_some(), "first token under the limit");
        assert!(
          $ir.next().unwrap().is_some(),
          "second token under the limit"
        );
        let r = ($leaf).map(|_| ());
        assert_eq!(
          r,
          Err(TsErr::Terminal),
          "a fresh limit trip in the committed leaf must surface a TERMINAL end-of-input error \
           (so recovery re-raises it), got {r:?}"
        );
        assert_eq!(
          scanned.get(),
          3,
          "the leaf's own consume scanned the tripping third token"
        );
      }
      // Genuine end of input at the same position → a PLAIN (recoverable) end-of-input error.
      {
        let (mut input, _scanned) = ts_input("1 2");
        let mut $ir = input.as_ref();
        assert!($ir.next().unwrap().is_some(), "first token");
        assert!($ir.next().unwrap().is_some(), "second token");
        let r = ($leaf).map(|_| ());
        assert_eq!(
          r,
          Err(TsErr::Eot),
          "a genuine end of input stays a PLAIN (recoverable) end-of-input error, got {r:?}"
        );
      }
    }
  };
}

committed_leaf_terminal_stop!(any_committed_consume_marks_terminal_on_trip, |inp| Any::of(
)
.parse_input(&mut inp));
committed_leaf_terminal_stop!(expect_committed_consume_marks_terminal_on_trip, |inp| {
  expect::<NumClass, LimLexer<'_>, TsCtx<'_>, ()>(NumClass).parse_input(&mut inp)
});
committed_leaf_terminal_stop!(ident_committed_consume_marks_terminal_on_trip, |inp| {
  Ident::parse(&mut inp)
});
committed_leaf_terminal_stop!(keyword_committed_consume_marks_terminal_on_trip, |inp| {
  Keyword::parse(&mut inp)
});
committed_leaf_terminal_stop!(
  keyword_macro_generated_committed_consume_marks_terminal_on_trip,
  |inp| { KwIf::parse(&mut inp) }
);
committed_leaf_terminal_stop!(delimited_opener_marks_terminal_on_trip, |inp| delimited::<
  Paren,
  _,
  _,
  _,
  _,
  _,
  _,
>(Any::of())(
  &mut inp
));
committed_leaf_terminal_stop!(expect_punct_marks_terminal_on_trip, |inp| inp
  .expect_comma());

// ── End-to-end through `Recover`: a terminal trip is re-raised, not recovered ──
//
// The direct assertions above prove each leaf builds a terminal-marked error on a trip; the
// `recover.rs` terminal-dual suite proves `Recover` re-raises such an error without invoking the
// recoverer. This test wires the two together on a real leaf and a real trip: the recoverer records
// whether it ran, and the trip must leave it untouched while a genuine end of input still admits it.

/// A recoverer that records whether it ran, then re-raises the error it was handed. Re-raising
/// (rather than synthesizing a value) keeps it agnostic to the primary's output type, so the same
/// recoverer serves any leaf; the observable under test is only whether it was invoked. Named rather
/// than a closure so its `'inp`/`'closure` lifetimes are quantified correctly (a closure with a
/// spelled-out `InputRef` type over-constrains them).
struct FlagRecover<'f> {
  invoked: &'f Cell<bool>,
}

impl<'inp, O> RecoverInput<'inp, LimLexer<'inp>, O, TsCtx<'inp>, ()> for FlagRecover<'_> {
  fn recover_input(
    &mut self,
    _inp: &mut InputRef<'inp, '_, LimLexer<'inp>, TsCtx<'inp>, ()>,
    err: TsEmitErr<'inp>,
  ) -> Result<O, TsEmitErr<'inp>> {
    self.invoked.set(true);
    Err(err)
  }
}

#[test]
fn recover_reraises_a_committed_leaf_trip_but_recovers_its_genuine_eof() {
  // Trip: the recoverer must NOT run — the terminal stop is re-raised.
  {
    let (mut input, _scanned) = ts_input("1 2 3");
    let mut inp = input.as_ref();
    assert!(inp.next().unwrap().is_some());
    assert!(inp.next().unwrap().is_some());

    let invoked = Cell::new(false);
    let result = {
      let mut rec = Recover::<_, _, LimTok, LimLexer<'_>, TsCtx<'_>, ()>::new(
        Any::of(),
        FlagRecover { invoked: &invoked },
      );
      rec.parse_input(&mut inp)
    };
    assert_eq!(
      result,
      Err(TsErr::Terminal),
      "a committed-leaf trip reaches Recover as terminal and is re-raised"
    );
    assert!(
      !invoked.get(),
      "the recoverer must NOT run for a terminal trip (it would re-trip the same limit)"
    );
  }
  // Genuine end of input: the recoverer DOES run — the plain end-of-input error stays recoverable.
  {
    let (mut input, _scanned) = ts_input("1 2");
    let mut inp = input.as_ref();
    assert!(inp.next().unwrap().is_some());
    assert!(inp.next().unwrap().is_some());

    let invoked = Cell::new(false);
    let _result = {
      let mut rec = Recover::<_, _, LimTok, LimLexer<'_>, TsCtx<'_>, ()>::new(
        Any::of(),
        FlagRecover { invoked: &invoked },
      );
      rec.parse_input(&mut inp)
    };
    assert!(
      invoked.get(),
      "a genuine end of input stays recoverable — the recoverer runs"
    );
  }
}
