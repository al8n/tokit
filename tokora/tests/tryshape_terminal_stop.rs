#![cfg(all(feature = "std", feature = "logos"))]

//! The try-shape terminal-stop law (Codex R1 [high]): an attempt shape declines —
//! `Ok(None)`, zero consumption — **iff the opener is definitely absent** (wrong next
//! token, or genuine end of input). A **terminal scanner stop** at the would-be opener —
//! a fresh resource-limit trip whose diagnostic a recovering emitter accepted, or an
//! already-latched poison boundary — is *not* evidence of absence, so the attempt must
//! surface the committed form's end-of-input error instead of declining into a wrong
//! parse.
//!
//! S1–S5 pin the fresh-trip path for all five shapes, S6 the latched-boundary path
//! (RED at 5bb5afc: each returned `Ok(None)`); S7 pins that a fatal emitter's own
//! rejection path is undisturbed, and S8/S9 guard the decline law against
//! over-correction (wrong opener and genuine EOI still decline).

use core::cell::Cell;
use std::rc::Rc;

use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, Token as TokenT,
  emitter::{Fatal, Silent, UnclosedEmitter, Verbose},
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::{parens, try_angles, try_braces, try_brackets, try_delimited, try_parens},
  punct::{
    CloseAngle, CloseBrace, CloseBracket, CloseParen, OpenAngle, OpenBrace, OpenBracket, OpenParen,
    Paren,
  },
  state::State,
  token::{IdentifierToken, KeywordToken, PunctuatorToken},
  types::{Ident, Keyword},
};

// ── A limiter whose scan counter is SHARED across every cloned lexer ──────────
//
// The `ProbeLimiter` pattern from the input-layer suite: `InputRef` builds a fresh
// lexer per operation by cloning the state, so only an `Rc<Cell<_>>`-shared counter
// makes every scan observable — a frozen count across calls proves the input latched
// and stopped rebuilding lexers.

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

  /// A shared handle to observe the scan counter after moving the state in.
  fn counter(&self) -> Rc<Cell<usize>> {
    self.scanned.clone()
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

// ── The fixture error: distinguishes the three channels the tests assert on ───

#[derive(Debug, Clone, PartialEq)]
enum TErr {
  /// A plain lexer/parse error (and the catch-all for the emitter families).
  Lex,
  /// The resource-limit trip's own diagnostic.
  Limit,
  /// The committed form's end-of-input error — what a terminal stop must surface.
  Eot,
  /// An unclosed-delimiter diagnostic — distinct so a trip-at-close test can assert it is
  /// *not* raised (the Tripped arm adds no `Unclosed`).
  Unclosed,
}

impl From<()> for TErr {
  fn from(_: ()) -> Self {
    TErr::Lex
  }
}

impl<D, S, Lang: ?Sized> From<Unclosed<D, S, Lang>> for TErr {
  fn from(_: Unclosed<D, S, Lang>) -> Self {
    TErr::Unclosed
  }
}

impl From<ScanLimitExceeded> for TErr {
  fn from(_: ScanLimitExceeded) -> Self {
    TErr::Limit
  }
}

impl<O, Lang: ?Sized> From<UnexpectedEot<O, Lang>> for TErr {
  fn from(_: UnexpectedEot<O, Lang>) -> Self {
    TErr::Eot
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for TErr {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    TErr::Lex
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for TErr {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    TErr::Lex
  }
}

impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for TErr {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    TErr::Lex
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for TErr {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    TErr::Lex
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for TErr {
  fn from(_: FullContainer<S, Lang>) -> Self {
    TErr::Lex
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for TErr {
  fn from(_: TooFew<S, Lang>) -> Self {
    TErr::Lex
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for TErr {
  fn from(_: TooMany<S, Lang>) -> Self {
    TErr::Lex
  }
}

// ── The token vocabulary: idents + all four delimiter pairs, every scan counted ─

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, extras = ScanLimiter, skip r"[ \t\r\n]+")]
enum Tok {
  #[regex(r"[a-z]+", |lex| { lex.extras.increase(); })]
  Ident,
  #[token("(", |lex| { lex.extras.increase(); })]
  LParen,
  #[token(")", |lex| { lex.extras.increase(); })]
  RParen,
  #[token("{", |lex| { lex.extras.increase(); })]
  LBrace,
  #[token("}", |lex| { lex.extras.increase(); })]
  RBrace,
  #[token("[", |lex| { lex.extras.increase(); })]
  LBracket,
  #[token("]", |lex| { lex.extras.increase(); })]
  RBracket,
  #[token("<", |lex| { lex.extras.increase(); })]
  LAngle,
  #[token(">", |lex| { lex.extras.increase(); })]
  RAngle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
  Ident,
  LParen,
  RParen,
  LBrace,
  RBrace,
  LBracket,
  RBracket,
  LAngle,
  RAngle,
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

impl core::fmt::Display for Kind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Kind::Ident => "identifier",
      Kind::LParen => "(",
      Kind::RParen => ")",
      Kind::LBrace => "{",
      Kind::RBrace => "}",
      Kind::LBracket => "[",
      Kind::RBracket => "]",
      Kind::LAngle => "<",
      Kind::RAngle => ">",
    })
  }
}

impl TokenT<'_> for Tok {
  type Kind = Kind;
  type Error = TErr;

  fn kind(&self) -> Kind {
    match self {
      Tok::Ident => Kind::Ident,
      Tok::LParen => Kind::LParen,
      Tok::RParen => Kind::RParen,
      Tok::LBrace => Kind::LBrace,
      Tok::RBrace => Kind::RBrace,
      Tok::LBracket => Kind::LBracket,
      Tok::RBracket => Kind::RBracket,
      Tok::LAngle => Kind::LAngle,
      Tok::RAngle => Kind::RAngle,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

impl PunctuatorToken<'_> for Tok {
  fn open_paren() -> Option<Kind> {
    Some(Kind::LParen)
  }
  fn close_paren() -> Option<Kind> {
    Some(Kind::RParen)
  }
  fn open_brace() -> Option<Kind> {
    Some(Kind::LBrace)
  }
  fn close_brace() -> Option<Kind> {
    Some(Kind::RBrace)
  }
  fn open_bracket() -> Option<Kind> {
    Some(Kind::LBracket)
  }
  fn close_bracket() -> Option<Kind> {
    Some(Kind::RBracket)
  }
  fn open_angle() -> Option<Kind> {
    Some(Kind::LAngle)
  }
  fn close_angle() -> Option<Kind> {
    Some(Kind::RAngle)
  }
}

// `Ident` is the identifier; no token is a reserved keyword (the trip tests never reach the
// classifier — the scan trips first — so the vocabulary needs no real keyword).
impl IdentifierToken<'_> for Tok {
  fn is_identifier(&self) -> bool {
    matches!(self, Tok::Ident)
  }
}

impl KeywordToken<'_> for Tok {
  fn keyword(&self) -> Option<&'static str> {
    None
  }
}

// `Kind: From<Open*/Close*<(), (), ()>>` — the `Punctuator` capability the typed pairs
// (`try_delimited::<Paren, …>` and the named twins' `finish_delimited`) classify through.

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

impl From<OpenBrace<(), (), ()>> for Kind {
  fn from(_: OpenBrace<(), (), ()>) -> Self {
    Kind::LBrace
  }
}

impl From<CloseBrace<(), (), ()>> for Kind {
  fn from(_: CloseBrace<(), (), ()>) -> Self {
    Kind::RBrace
  }
}

impl From<OpenBracket<(), (), ()>> for Kind {
  fn from(_: OpenBracket<(), (), ()>) -> Self {
    Kind::LBracket
  }
}

impl From<CloseBracket<(), (), ()>> for Kind {
  fn from(_: CloseBracket<(), (), ()>) -> Self {
    Kind::RBracket
  }
}

impl From<OpenAngle<(), (), ()>> for Kind {
  fn from(_: OpenAngle<(), (), ()>) -> Self {
    Kind::LAngle
  }
}

impl From<CloseAngle<(), (), ()>> for Kind {
  fn from(_: CloseAngle<(), (), ()>) -> Self {
    Kind::RAngle
  }
}

// ── Harness ────────────────────────────────────────────────────────────────────

type TLexer<'a> = LogosLexer<'a, Tok>;

/// The inner sub-parser the shapes wrap; the trip cases never reach it.
fn ident_inner<'inp, Ctx>(inp: &mut InputRef<'inp, '_, TLexer<'inp>, Ctx>) -> Result<(), TErr>
where
  Ctx: ParseContext<'inp, TLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TLexer<'inp>, Error = TErr>,
{
  match inp.try_expect(|t| matches!(t.data, Tok::Ident))? {
    Some(_) => Ok(()),
    None => Err(TErr::Lex),
  }
}

/// The atom-harness drive (the `parser_atoms` pattern plus a lexer state): one concrete
/// lexer, generic over the emitter — `&mut Verbose` for the post-drive diagnostic
/// asserts, `Silent`/`Fatal` by value — through the public `ParserContext` +
/// `parse_str_with_state` surface.
fn drive<'inp, O, Em>(
  emitter: Em,
  state: ScanLimiter,
  f: impl for<'c> FnMut(
    &mut InputRef<'inp, 'c, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Em>>,
  ) -> Result<O, TErr>,
  input: &'inp str,
) -> Result<O, TErr>
where
  Em: Emitter<'inp, TLexer<'inp>, Error = TErr> + UnclosedEmitter<'inp, TLexer<'inp>>,
{
  let ctx: ParserContext<'inp, TLexer<'inp>, Em> = ParserContext::new(emitter);
  Parser::with_parser_and_context(f, ctx).parse_str_with_state(input, state)
}

/// Counts the limit diagnostics a verbose emitter collected.
fn limit_diags(emitter: &Verbose<TErr>) -> usize {
  emitter
    .errors()
    .values()
    .flatten()
    .filter(|e| **e == TErr::Limit)
    .count()
}

/// The fresh-trip assertion S1–S5 share: not a decline, the committed form's
/// end-of-input error, exactly one scan spent on the tripping opener, and the trip's
/// own diagnostic held by the recovering emitter.
fn assert_trip_surfaces(out: Result<Option<()>, TErr>, scans: usize, diags: usize) {
  assert!(
    !matches!(out, Ok(None)),
    "a terminal stop is not evidence the opener is absent — the attempt must not decline"
  );
  assert!(
    matches!(out, Err(TErr::Eot)),
    "the attempt surfaces the committed form's end-of-input error, got {out:?}"
  );
  assert_eq!(
    scans, 3,
    "scanned exactly the two idents and the tripping opener"
  );
  assert_eq!(
    diags, 1,
    "the trip's own diagnostic reached the recovering emitter"
  );
}

// ── S1–S5: a fresh trip at the would-be opener must not decline ───────────────

#[test]
fn try_parens_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      // The scan at the would-be `(` opener is the third scanned token: it trips.
      try_parens(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a b (x)",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));

  // The one Silent case: the same trip under the other recovering emitter — the
  // attempt still errors (nothing about the law is Verbose-specific).
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    Silent::<TErr>::new(),
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      try_parens(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a b (x)",
  );
  assert!(
    matches!(out, Err(TErr::Eot)),
    "the trip surfaces the same error under Silent, got {out:?}"
  );
  assert_eq!(
    scanned.get(),
    3,
    "scanned exactly a, b, and the tripping opener"
  );
}

#[test]
fn try_braces_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      try_braces(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a b {x}",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

#[test]
fn try_brackets_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      try_brackets(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a b [x]",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

#[test]
fn try_angles_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      try_angles(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a b <x>",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

#[test]
fn try_delimited_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      try_delimited::<Paren, _, _, _, _, _, _>(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a b (x)",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

// ── S6: an already-latched poison boundary at the attempt must not decline ────

#[test]
fn try_parens_does_not_decline_on_latched_boundary() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let observer = scanned.clone();
  let out = drive(
    &mut verbose,
    limiter,
    move |inp| {
      // Latch first: drive `next()` past the trip — the third scan trips, the
      // diagnostic is emitted, and the boundary latches.
      assert!(inp.next()?.is_some(), "first ident");
      assert!(inp.next()?.is_some(), "second ident");
      assert!(inp.next()?.is_none(), "the third scan trips and latches");
      let frozen = observer.get();
      assert_eq!(frozen, 3, "scanned exactly a, b, c before latching");

      // The attempt at the latched boundary: a terminal stop, not proof of absence.
      let out = try_parens(ident_inner)(inp).map(|d| d.map(|_| ()));
      assert_eq!(
        observer.get(),
        frozen,
        "no lexer was rebuilt at the latched boundary — the scan counter is frozen"
      );
      out
    },
    "a b c (x)",
  );
  assert!(
    !matches!(out, Ok(None)),
    "a latched poison boundary is not evidence the opener is absent — the attempt must not decline"
  );
  assert!(
    matches!(out, Err(TErr::Eot)),
    "the attempt surfaces the committed form's end-of-input error, got {out:?}"
  );
  assert_eq!(
    scanned.get(),
    3,
    "the scan counter stays frozen after the drive"
  );
  assert_eq!(
    limit_diags(&verbose),
    1,
    "the latch's diagnostic was emitted when the trip originally latched — the attempt emits nothing new"
  );
}

// ── S7: the fatal path is undisturbed — a trip at the opener already errors ───

#[test]
fn try_parens_trip_under_fatal_stays_err() {
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    Fatal::<TErr>::new(),
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident");
      assert!(inp.next()?.is_some(), "second ident");
      try_parens(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a b (x)",
  );
  assert!(
    matches!(out, Err(TErr::Limit)),
    "the fatal emitter's rejection of the trip diagnostic propagates from the scan itself, got {out:?}"
  );
  assert_eq!(
    scanned.get(),
    3,
    "scanned exactly a, b, and the tripping opener"
  );
}

// ── S8/S9: the decline law is preserved — definite absence still declines ─────

#[test]
fn try_parens_still_declines_on_wrong_opener_under_recovering_emitter() {
  let mut verbose = Verbose::<TErr>::new();
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      let out = try_parens(ident_inner)(inp).map(|d| d.map(|_| ()));
      assert!(
        matches!(out, Ok(None)),
        "a wrong next token is definite absence: the attempt declines, got {out:?}"
      );
      // Zero consumption: the wrong opener is still the next token.
      let next = inp.next()?.expect("the declined token is still next");
      assert!(
        matches!(next.data(), Tok::LBrace),
        "the `{{` the attempt declined on stays unconsumed"
      );
      out
    },
    "{x}",
  );
  assert!(matches!(out, Ok(None)));
  let total: usize = verbose.errors().values().map(|g| g.len()).sum();
  assert_eq!(total, 0, "a legitimate decline emits nothing");
}

#[test]
fn try_parens_still_declines_on_genuine_eoi_under_recovering_emitter() {
  // Empty input.
  let mut verbose = Verbose::<TErr>::new();
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| try_parens(ident_inner)(inp).map(|d| d.map(|_| ())),
    "",
  );
  assert!(
    matches!(out, Ok(None)),
    "genuine end of input is definite absence: the attempt declines, got {out:?}"
  );

  // Fully-consumed input under a roomy limit.
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      assert!(inp.next()?.is_some(), "consume the only token");
      try_parens(ident_inner)(inp).map(|d| d.map(|_| ()))
    },
    "a",
  );
  assert!(
    matches!(out, Ok(None)),
    "end of a fully-consumed input declines, got {out:?}"
  );
  let total: usize = verbose.errors().values().map(|g| g.len()).sum();
  assert_eq!(total, 0, "a genuine end-of-input decline emits nothing");
}

// ── The close-position twin: a terminal trip at the CLOSER ────────────────────
//
// The committed shapes now classify the close position with the same four-way `probe_close`
// as the many-builders. A resource-limit trip there is `Tripped`, not `Eof`: it surfaces the
// committed form's end-of-input error and adds NO `Unclosed`. `(a)` under a limit of 2 trips
// while scanning the closing `)` (the opener + inner ident spend the budget), so the closer
// is never reached as a token.
#[test]
fn committed_shape_trip_at_close_surfaces_eot_not_unclosed() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| parens(ident_inner)(inp).map(|_| ()),
    "(a)",
  );
  assert_eq!(
    out,
    Err(TErr::Eot),
    "a trip at the closer surfaces the committed form's end-of-input error, not Unclosed"
  );
  assert_eq!(
    scanned.get(),
    3,
    "scanned the opener, the inner ident, and the tripping closer"
  );
  assert_eq!(
    limit_diags(&verbose),
    1,
    "the trip's own diagnostic reached the recovering emitter"
  );
  assert!(
    !verbose
      .errors()
      .values()
      .flatten()
      .any(|e| matches!(e, TErr::Unclosed)),
    "no spurious Unclosed on the Tripped path"
  );
}

// ── The leaf attempts: an attempt built on a token leaf must not decline on a trip ──
//
// Every try-shaped leaf (`Expect`, `peek_kind`, the keyword/ident attempts, the token-pratt
// LHS/RHS) reads a decline as "the thing is definitely absent". A terminal scanner stop is not
// absence, so the same law S1–S9 pin for the delimited shapes holds one layer down at the leaves:
// a fresh trip at the would-be leaf surfaces the committed end-of-input error, a genuine decline
// (wrong token or real end of input) still declines.

use tokora::{
  TryParseInput, parser::peek_kind, parser::try_expect_of, try_parse_input::ParseAttempt,
};

#[test]
fn expect_try_leaf_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      // The attempt at the would-be `(` opener trips on the third scan.
      let attempt = try_expect_of::<_, TLexer<'_>, _, ()>(|t: &Tok| matches!(t, Tok::LParen))
        .try_parse_input(inp)?;
      Ok(match attempt {
        tokora::try_parse_input::ParseAttempt::Accept(_) => Some(()),
        tokora::try_parse_input::ParseAttempt::Decline => None,
      })
    },
    "a b (x)",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

#[test]
fn expect_try_leaf_still_declines_on_wrong_token_and_eoi() {
  // A wrong next token: definite absence, declines, nothing emitted.
  let mut verbose = Verbose::<TErr>::new();
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      let attempt = try_expect_of::<_, TLexer<'_>, _, ()>(|t: &Tok| matches!(t, Tok::LParen))
        .try_parse_input(inp)?;
      Ok(matches!(
        attempt,
        tokora::try_parse_input::ParseAttempt::Accept(_)
      ))
    },
    "a",
  );
  assert_eq!(out, Ok(false), "a wrong opener is a genuine decline");

  // Genuine end of input: declines.
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      let attempt = try_expect_of::<_, TLexer<'_>, _, ()>(|t: &Tok| matches!(t, Tok::LParen))
        .try_parse_input(inp)?;
      Ok(matches!(
        attempt,
        tokora::try_parse_input::ParseAttempt::Accept(_)
      ))
    },
    "",
  );
  assert_eq!(out, Ok(false), "genuine end of input is a genuine decline");
  let total: usize = verbose.errors().values().map(|g| g.len()).sum();
  assert_eq!(total, 0, "a legitimate decline emits nothing");
}

#[test]
fn peek_kind_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      // Peeking the dispatch kind at the would-be `(` trips on the third scan.
      Ok(peek_kind(inp)?.map(|_| ()))
    },
    "a b (x)",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

#[test]
fn peek_kind_still_reports_none_on_genuine_eoi() {
  let mut verbose = Verbose::<TErr>::new();
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| Ok(peek_kind(inp)?.map(|_| ())),
    "",
  );
  assert_eq!(
    out,
    Ok(None),
    "genuine end of input peeks as None, not an error"
  );
}

// ── A composite alternation on a tripped position must not commit its epsilon fallback ──
//
// A hand-rolled alternation tries each arm's attempt in turn and falls through to an epsilon
// value when every arm declines. On a terminal stop the first arm's attempt is not a decline —
// it surfaces the end-of-input error — so the alternation must propagate it, never reach epsilon.

#[derive(Debug, PartialEq)]
enum Alt {
  Ident,
  Paren,
  Epsilon,
}

fn alt3<'inp, Em>(
  inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Em>>,
) -> Result<Alt, TErr>
where
  Em: Emitter<'inp, TLexer<'inp>, Error = TErr> + UnclosedEmitter<'inp, TLexer<'inp>>,
{
  if let ParseAttempt::Accept(_) =
    try_expect_of::<_, TLexer<'_>, _, ()>(|t: &Tok| matches!(t, Tok::Ident)).try_parse_input(inp)?
  {
    return Ok(Alt::Ident);
  }
  if let ParseAttempt::Accept(_) =
    try_expect_of::<_, TLexer<'_>, _, ()>(|t: &Tok| matches!(t, Tok::LParen))
      .try_parse_input(inp)?
  {
    return Ok(Alt::Paren);
  }
  Ok(Alt::Epsilon)
}

#[test]
fn composite_alternation_surfaces_trip_not_epsilon() {
  // A fresh trip at the alternation head must surface, not commit the epsilon fallback.
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      alt3(inp)
    },
    "a b (x)",
  );
  assert_eq!(
    out,
    Err(TErr::Eot),
    "the alternation surfaces the trip instead of committing its epsilon fallback"
  );

  // The or_stop primitive the arms are built on errs directly on a trip.
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident");
      assert!(inp.next()?.is_some(), "second ident");
      Ok(
        inp
          .try_expect_or_stop(|t| matches!(t.data, Tok::LParen))?
          .map(|_| ()),
      )
    },
    "a b (x)",
  );
  assert_eq!(out, Err(TErr::Eot), "try_expect_or_stop errs on a trip");

  // A genuine end of input still declines all the way to the epsilon fallback.
  let mut verbose = Verbose::<TErr>::new();
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      assert!(inp.next()?.is_some(), "consume the only real token");
      alt3(inp)
    },
    "z",
  );
  assert_eq!(
    out,
    Ok(Alt::Epsilon),
    "genuine end of input declines to the epsilon fallback"
  );
}

// ── The keyword and identifier attempts: the same law at the vocabulary leaves ──

#[test]
fn ident_try_leaf_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      Ok(match Ident::try_parse_of(inp)? {
        ParseAttempt::Accept(_) => Some(()),
        ParseAttempt::Decline => None,
      })
    },
    "a b (x)",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

#[test]
fn keyword_try_leaf_does_not_decline_on_trip() {
  let mut verbose = Verbose::<TErr>::new();
  let limiter = ScanLimiter::with_limit(2);
  let scanned = limiter.counter();
  let out = drive(
    &mut verbose,
    limiter,
    |inp| {
      assert!(inp.next()?.is_some(), "first ident under the limit");
      assert!(inp.next()?.is_some(), "second ident under the limit");
      Ok(match Keyword::try_parse_of(inp)? {
        ParseAttempt::Accept(_) => Some(()),
        ParseAttempt::Decline => None,
      })
    },
    "a b (x)",
  );
  assert_trip_surfaces(out, scanned.get(), limit_diags(&verbose));
}

#[test]
fn ident_try_leaf_still_declines_on_genuine_eoi() {
  let mut verbose = Verbose::<TErr>::new();
  let out = drive(
    &mut verbose,
    ScanLimiter::with_limit(usize::MAX),
    |inp| {
      Ok(match Ident::try_parse_of(inp)? {
        ParseAttempt::Accept(_) => Some(()),
        ParseAttempt::Decline => None,
      })
    },
    "",
  );
  assert_eq!(out, Ok(None), "genuine end of input declines, no error");
}

// ── The committed peek combinators: a trip-truncated window must surface, not route to a branch ──
//
// The committed `peek_then` / `peek_then_choice` pass their scrutinee window to the handler and run
// the chosen branch. A window truncated by a terminal scanner stop is not a definite decision, so —
// like the try flavors — the committed impls surface the stop before the handler can route the short
// window to a branch that succeeds without consuming. A genuine short window (no trip) still reaches
// the handler unchanged.

use tokora::{
  Branch, ParseChoice, ParseInput,
  input::Completeness,
  utils::typenum::{U1, U2},
};

// An epsilon parser: succeeds without consuming, producing `()`. Its `ParseInput` impl is pinned to
// the one concrete context the committed-path tests drive over, so `peek_then`/`peek_then_choice`
// resolve a unique context at method resolution — a generic base parser leaves it ambiguous between
// the `ParserContext` and tuple `(E, C)` context impls.
struct Eps;

impl<'inp, Cmpl>
  ParseInput<'inp, TLexer<'inp>, (), ParserContext<'inp, TLexer<'inp>, Silent<TErr>>, (), Cmpl>
  for Eps
where
  Cmpl: Completeness,
{
  fn parse_input(
    &mut self,
    _inp: &mut InputRef<'inp, '_, TLexer<'inp>, ParserContext<'inp, TLexer<'inp>, Silent<TErr>>, (), Cmpl>,
  ) -> Result<
    (),
    <<ParserContext<'inp, TLexer<'inp>, Silent<TErr>> as ParseContext<'inp, TLexer<'inp>>>::Emitter as Emitter<'inp, TLexer<'inp>>>::Error,
  >{
    Ok(())
  }
}

// Drives a committed parser as the top-level parse over a concrete context, so the parser's context
// is pinned here (avoiding the tuple-vs-`ParserContext` ambiguity that a generic base parser hits at
// method resolution). Limits are chosen so the trip lands at the first/second scan — no pre-parse
// consumption is needed.
fn drive_committed<'inp, P>(parser: P, limit: usize, input: &'inp str) -> Result<(), TErr>
where
  P: ParseInput<'inp, TLexer<'inp>, (), ParserContext<'inp, TLexer<'inp>, Silent<TErr>>, ()>,
{
  let ctx = ParserContext::new(Silent::<TErr>::new());
  Parser::with_parser_and_context(parser, ctx)
    .parse_str_with_state(input, ScanLimiter::with_limit(limit))
}

#[test]
fn committed_peek_then_surfaces_a_trip_not_a_branch() {
  // First-slot trip (W = 1, limit 0): the first scan trips, so the window is empty.
  assert_eq!(
    drive_committed(Eps.peek_then::<_, U1>(|_peeked, _emitter| Ok(())), 0, "(x)"),
    Err(TErr::Eot),
    "a first-slot trip surfaces terminal, not the handler's Ok branch"
  );

  // Mid-window trip (W = 2, limit 1): slot 0 scans, slot 1 trips; the handler would otherwise route
  // the one-token window to a branch that succeeds without consuming.
  assert_eq!(
    drive_committed(Eps.peek_then::<_, U2>(|_peeked, _emitter| Ok(())), 1, "(x)"),
    Err(TErr::Eot),
    "a mid-window trip (W>1) surfaces terminal, not the handler's Ok branch"
  );

  // A genuine short window (no trip) still reaches the handler and runs the branch.
  let reached = Cell::new(false);
  let out = drive_committed(
    Eps.peek_then::<_, U2>(|_peeked, _emitter| {
      reached.set(true);
      Ok(())
    }),
    usize::MAX,
    "a",
  );
  assert_eq!(out, Ok(()), "a genuine short window reaches the handler");
  assert!(reached.get(), "the handler ran on the genuine short window");
}

#[test]
fn committed_peek_then_choice_surfaces_a_trip_not_a_branch() {
  // First-slot trip (W = 1, limit 0).
  assert_eq!(
    drive_committed(
      (Eps, Eps).peek_then_choice::<_, U1>(|_peeked, _emitter| Ok(Branch::B0)),
      0,
      "(x)",
    ),
    Err(TErr::Eot),
    "a first-slot trip surfaces terminal, not branch B0"
  );

  // Mid-window trip (W = 2, limit 1): slot 0 scans, slot 1 trips; B0 would otherwise succeed
  // without consuming.
  assert_eq!(
    drive_committed(
      (Eps, Eps).peek_then_choice::<_, U2>(|_peeked, _emitter| Ok(Branch::B0)),
      1,
      "(x)",
    ),
    Err(TErr::Eot),
    "a mid-window trip surfaces terminal, not branch B0"
  );

  // A genuine short window still reaches the handler and runs the chosen branch.
  let reached = Cell::new(false);
  let out = drive_committed(
    (Eps, Eps).peek_then_choice::<_, U2>(|_peeked, _emitter| {
      reached.set(true);
      Ok(Branch::B0)
    }),
    usize::MAX,
    "a",
  );
  assert_eq!(out, Ok(()), "a genuine short window reaches the handler");
  assert!(reached.get(), "the handler ran on the genuine short window");
}
