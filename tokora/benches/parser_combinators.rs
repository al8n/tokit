//! Parser-level benchmarks: the combinator assemblies a real grammar is built from.
//!
//! `input_scan.rs` measures the scanner protocol — the per-token cost of `next`,
//! `try_expect`, `peek`, and the dispatch shapes — in isolation. These measure the
//! layer above it: the drivers a grammar actually reaches for, each one driven over a
//! deterministic ~128 KiB source so a full parse dominates the fixed per-parse setup
//! and a run resolves small deltas.
//!
//! Benches (group `parser`):
//!   * `repeated_collect`  — `element.repeated().collect()` over a long run of ints.
//!   * `separated_collect` — `element.separated_by_comma().collect()`: the separator
//!     policy driver, one long list.
//!   * `delimited_list`    — a delimited comma list, `( a , b , c )`, repeated: the
//!     sequencing + separation + repetition stack a real argument list is.
//!   * `dispatch_peek`     — `DispatchOnKind` over an 8-kind stream.
//!   * `dispatch_fused`    — `FusedDispatchOnKind` over the same stream. A small mirror
//!     of `input_scan.rs`'s 8-kind fixture; the CANONICAL dispatch benches (peek vs
//!     fused, light vs heavy lexer state) live there. These two exist so a parser-level
//!     regression in the dispatch drivers shows up in this group too.
//!   * `pratt_expr`        — `InputRef::pratt` over arithmetic expressions, with plain
//!     `i64` binding powers.
//!   * `skip_then_retry`   — the recovery driver over a source with periodic garbage:
//!     every eighth statement is preceded by a parenthesised lump the parser must skip.
//!
//! Every source is generated from a counter — no randomness, no clock — so the fixtures
//! are byte-identical between runs and between machines.

use core::{fmt::Write as _, time::Duration};
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use tokora::{
  Accumulator, Balance, Emitter, InputRef, Parse, ParseChoice, ParseContext, ParseInput,
  ParseTokenChoice, Parser, SimpleSpan, Token, TryParseInput,
  emitter::{
    FullContainerEmitter, PrattEmitter, SeparatedEmitter, UnexpectedLeadingSeparatorEmitter,
    UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    MaybeIncomplete, MaybeTerminal, UnexpectedEnd,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::{Any, PrattInfix, PrattLHS, PrattRHS, Precedenced, expect},
  punct::Comma,
  span::Spanned,
  token::{PrattToken, PunctuatorToken},
  try_parse_input::ParseAttempt,
  utils::Expected,
};

// ── Fixture: a Calc-shaped token enum (the guide's language) ──────────────────
//
// Whitespace is skipped by the lexer rather than tokenized: these benches measure
// the combinator drivers, and `input_scan.rs` already owns the trivia-skipping path.
// The discriminants are dense so a kind match beside a dispatch table compiles to a
// jump table.

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
enum BenchTok {
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
  Int(i64),
  #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
  Ident,
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("^")]
  Caret,
  #[token(",")]
  Comma,
  #[token(";")]
  Semi,
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BenchKind {
  Int,
  Ident,
  Plus,
  Minus,
  Star,
  Slash,
  Caret,
  Comma,
  Semi,
  LParen,
  RParen,
}

impl core::fmt::Display for BenchKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let s = match self {
      BenchKind::Int => "integer",
      BenchKind::Ident => "identifier",
      BenchKind::Plus => "'+'",
      BenchKind::Minus => "'-'",
      BenchKind::Star => "'*'",
      BenchKind::Slash => "'/'",
      BenchKind::Caret => "'^'",
      BenchKind::Comma => "','",
      BenchKind::Semi => "';'",
      BenchKind::LParen => "'('",
      BenchKind::RParen => "')'",
    };
    f.write_str(s)
  }
}

impl core::fmt::Display for BenchTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      BenchTok::Int(n) => write!(f, "{n}"),
      other => core::fmt::Display::fmt(&other.kind(), f),
    }
  }
}

impl Token<'_> for BenchTok {
  type Kind = BenchKind;
  type Error = ();

  fn kind(&self) -> BenchKind {
    match self {
      BenchTok::Int(_) => BenchKind::Int,
      BenchTok::Ident => BenchKind::Ident,
      BenchTok::Plus => BenchKind::Plus,
      BenchTok::Minus => BenchKind::Minus,
      BenchTok::Star => BenchKind::Star,
      BenchTok::Slash => BenchKind::Slash,
      BenchTok::Caret => BenchKind::Caret,
      BenchTok::Comma => BenchKind::Comma,
      BenchTok::Semi => BenchKind::Semi,
      BenchTok::LParen => BenchKind::LParen,
      BenchTok::RParen => BenchKind::RParen,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

// The comma is the separator vocabulary's one wired punctuator (the `separated_by_comma`
// driver looks it up through this pair).
impl PunctuatorToken<'_> for BenchTok {
  fn comma() -> Option<BenchKind> {
    Some(BenchKind::Comma)
  }
}

impl From<Comma<(), (), ()>> for BenchKind {
  fn from(_: Comma<(), (), ()>) -> Self {
    BenchKind::Comma
  }
}

type BenchLexer<'a> = LogosLexer<'a, BenchTok>;

// ── The error absorber ────────────────────────────────────────────────────────
//
// Every source below is well-formed except the recovery fixture's deliberate garbage,
// so most of these `From`s are never constructed at runtime — they exist to satisfy
// the `FromEmitterError` bound the combinator drivers require.

#[derive(Debug, Default, Clone, PartialEq)]
struct BenchError;

impl From<()> for BenchError {
  fn from(_: ()) -> Self {
    BenchError
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for BenchError {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    BenchError
  }
}

// Covers `UnexpectedEot` and the Pratt engine's `UnexpectedEoLhs` / `UnexpectedEoRhs`
// in one impl — they are all aliases of `UnexpectedEnd` with different hints.
impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for BenchError {
  fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self {
    BenchError
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for BenchError {
  fn from(_: FullContainer<S, Lang>) -> Self {
    BenchError
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for BenchError {
  fn from(_: TooFew<S, Lang>) -> Self {
    BenchError
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for BenchError {
  fn from(_: TooMany<S, Lang>) -> Self {
    BenchError
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for BenchError {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    BenchError
  }
}

impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for BenchError {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    BenchError
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for BenchError {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    BenchError
  }
}

// `skip_then_retry` consults these before it skips: an `Incomplete` and a terminal scanner stop
// are never recovered. A `Complete`-mode parse with no resource limit can produce neither, so the
// traits' default answers are right.
impl MaybeIncomplete for BenchError {}
impl MaybeTerminal for BenchError {}

// ── Elements ──────────────────────────────────────────────────────────────────

/// The try-shaped integer element every repetition driver below is built on: a
/// non-integer is put back and the element declines, which is how the drivers stop.
fn try_int<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<i64>, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  Ok(
    match inp.try_expect(|t| matches!(t.data(), BenchTok::Int(_)))? {
      Some(tok) => match tok.into_data() {
        BenchTok::Int(n) => ParseAttempt::Accept(n),
        _ => unreachable!("the predicate admits only integers"),
      },
      None => ParseAttempt::Decline,
    },
  )
}

// ── 1. repeated + collect ─────────────────────────────────────────────────────

fn repeated_collect<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>
    + FullContainerEmitter<'inp, BenchLexer<'inp>>,
{
  let items: Vec<i64> = try_int.repeated().collect().parse_input(inp)?;
  Ok(black_box(items).len())
}

// ── 2. separated + collect ────────────────────────────────────────────────────

fn separated_collect<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>
    + SeparatedEmitter<'inp, BenchLexer<'inp>>
    + FullContainerEmitter<'inp, BenchLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, BenchLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, BenchLexer<'inp>>,
{
  let items: Vec<i64> = try_int.separated_by_comma().collect().parse_input(inp)?;
  Ok(black_box(items).len())
}

// ── 3. delimited comma list, repeated ─────────────────────────────────────────

/// `( int , int , … )` as a *try*-shaped element, so a run of them can be repeated:
/// sequencing for the parentheses, the separated driver for the elements.
fn try_paren_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<Vec<i64>>, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>
    + SeparatedEmitter<'inp, BenchLexer<'inp>>
    + FullContainerEmitter<'inp, BenchLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, BenchLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, BenchLexer<'inp>>,
{
  if inp
    .try_expect(|t| matches!(t.data(), BenchTok::LParen))?
    .is_none()
  {
    return Ok(ParseAttempt::Decline);
  }
  // Committed to the list once the opener is consumed.
  let items: Vec<i64> = try_int
    .separated_by_comma()
    .collect()
    .then_ignore(expect(|t: &BenchTok| {
      if matches!(t, BenchTok::RParen) {
        Ok(())
      } else {
        Err(Expected::one(BenchKind::RParen))
      }
    }))
    .parse_input(inp)?;
  Ok(ParseAttempt::Accept(items))
}

fn delimited_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>
    + SeparatedEmitter<'inp, BenchLexer<'inp>>
    + FullContainerEmitter<'inp, BenchLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, BenchLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, BenchLexer<'inp>>,
{
  let lists: Vec<Vec<i64>> = try_paren_list.repeated().collect().parse_input(inp)?;
  Ok(black_box(lists).len())
}

// ── 4 & 5. dispatch: peeked and fused ─────────────────────────────────────────
//
// A small mirror of `input_scan.rs`'s 8-kind dispatch fixture. The canonical dispatch
// benches — peek vs fused, over both a light and a deliberately heavy lexer state —
// live there; these two exist so the same regression is visible from the parser group.

/// `table[i]` is the viable first-token kind for branch `i`. Eight arms, eight kinds,
/// and the recovery source is the only fixture that contains a kind outside it.
const DISPATCH_TABLE: &[BenchKind] = &[
  BenchKind::Ident,
  BenchKind::Int,
  BenchKind::Plus,
  BenchKind::Star,
  BenchKind::Slash,
  BenchKind::Minus,
  BenchKind::Comma,
  BenchKind::Semi,
];

fn dispatch_peek<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  let mut parser = (
    Any::<BenchLexer<'inp>, Ctx>::new(),
    Any::<BenchLexer<'inp>, Ctx>::new(),
    Any::<BenchLexer<'inp>, Ctx>::new(),
    Any::<BenchLexer<'inp>, Ctx>::new(),
    Any::<BenchLexer<'inp>, Ctx>::new(),
    Any::<BenchLexer<'inp>, Ctx>::new(),
    Any::<BenchLexer<'inp>, Ctx>::new(),
    Any::<BenchLexer<'inp>, Ctx>::new(),
  )
    .dispatch_on_kind(DISPATCH_TABLE);
  // Well-formed source + complete table: the only `Err` is the final end of input.
  while let Ok(tok) = parser.parse_input(inp) {
    black_box(&tok);
    n += 1;
  }
  Ok(n)
}

/// A no-op fused arm: it receives the head token the dispatcher already consumed to
/// classify it — there is no cache round trip to pay for.
fn head_arm<'inp, Ctx>(
  head: Spanned<BenchTok, SimpleSpan>,
  _inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<(), BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  black_box(&head);
  Ok(())
}

fn dispatch_fused<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  let mut parser = (
    head_arm, head_arm, head_arm, head_arm, head_arm, head_arm, head_arm, head_arm,
  )
    .fused_dispatch_on_kind(DISPATCH_TABLE);
  while let Ok(()) = parser.parse_input(inp) {
    n += 1;
  }
  Ok(n)
}

// ── 6. Pratt expressions ──────────────────────────────────────────────────────
//
// Binding powers are plain `i64` — tokora implements `PrattPower` for the integers, so
// no newtype is involved and this measures the engine, not a wrapper. `PREC_PAREN` is
// below `i64::default()` (0), which is what lets `)` be consumed by the recursive call
// the `(` prefix starts and ignored at the top level.

const PREC_PAREN: i64 = -1;
const PREC_SUM: i64 = 1;
const PREC_PROD: i64 = 2;
const PREC_NEG: i64 = 3;
const PREC_EXP: i64 = 4;

impl PrattToken<'_, i64> for BenchTok {
  fn try_pratt_lhs(&self) -> Option<PrattLHS<(), (), i64>> {
    Some(match self {
      BenchTok::Int(_) => PrattLHS::Operand(()),
      BenchTok::Minus => PrattLHS::Prefix(Precedenced::new((), PREC_NEG)),
      BenchTok::LParen => PrattLHS::Prefix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }

  fn try_pratt_rhs(&self) -> Option<PrattRHS<(), (), (), (), i64>> {
    Some(match self {
      BenchTok::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
      BenchTok::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
      BenchTok::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
      BenchTok::Slash => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD)),
      BenchTok::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), PREC_EXP)),
      BenchTok::RParen => PrattRHS::Postfix(Precedenced::new((), PREC_PAREN)),
      _ => return None,
    })
  }
}

/// Named `fn`s, not closures: the fold bounds are higher-ranked over the emitter borrow,
/// which a closure cannot satisfy. Values are wrapped back into `Int`, and every operator
/// is wrapping/saturating so the fixture can never trip an overflow.
fn fold_prefix<E>(
  op: Spanned<BenchTok, SimpleSpan>,
  operand: Spanned<BenchTok, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<BenchTok, SimpleSpan>, BenchError> {
  let (span, op) = op.into_components();
  match op {
    BenchTok::Minus => Ok(Spanned::new(
      span,
      BenchTok::Int(int_of(&operand).wrapping_neg()),
    )),
    BenchTok::LParen => Ok(operand),
    _ => Ok(operand),
  }
}

fn fold_infix<E>(
  left: Spanned<BenchTok, SimpleSpan>,
  right: Spanned<BenchTok, SimpleSpan>,
  infix: Spanned<PrattInfix<BenchTok, BenchTok, BenchTok>, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<BenchTok, SimpleSpan>, BenchError> {
  let span = left.span();
  let (l, r) = (int_of(&left), int_of(&right));
  let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) = infix.into_data();
  let value = match op {
    BenchTok::Plus => l.wrapping_add(r),
    BenchTok::Minus => l.wrapping_sub(r),
    BenchTok::Star => l.wrapping_mul(r),
    BenchTok::Slash => l.checked_div(r).unwrap_or(0),
    BenchTok::Caret => u32::try_from(r.rem_euclid(8))
      .ok()
      .map_or(0, |e| l.wrapping_pow(e)),
    _ => 0,
  };
  Ok(Spanned::new(span, BenchTok::Int(value)))
}

fn fold_postfix<E>(
  operand: Spanned<BenchTok, SimpleSpan>,
  _close: Spanned<BenchTok, SimpleSpan>,
  _: &mut E,
) -> Result<Spanned<BenchTok, SimpleSpan>, BenchError> {
  Ok(operand)
}

fn int_of(tok: &Spanned<BenchTok, SimpleSpan>) -> i64 {
  match tok.data() {
    BenchTok::Int(n) => *n,
    _ => 0,
  }
}

/// One `pratt` call per `;`-terminated expression, to end of input.
fn pratt_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter:
    Emitter<'inp, BenchLexer<'inp>, Error = BenchError> + PrattEmitter<'inp, BenchLexer<'inp>>,
{
  let mut n = 0usize;
  loop {
    // Scope the peek borrow: an empty input ends the drain before `pratt` is asked for
    // an expression that is not there.
    let done = {
      let peeked = inp.peek_one()?;
      peeked.is_none()
    };
    if done {
      break;
    }
    match inp.pratt::<_, _, _, i64, i64>(
      fold_prefix::<Ctx::Emitter>,
      fold_infix::<Ctx::Emitter>,
      fold_postfix::<Ctx::Emitter>,
    )? {
      Some(tok) => {
        black_box(&tok);
        n += 1;
      }
      None => break,
    }
    if inp
      .try_expect(|t| matches!(t.data(), BenchTok::Semi))?
      .is_none()
    {
      break;
    }
  }
  Ok(n)
}

// ── 7. skip_then_retry recovery ───────────────────────────────────────────────

/// Calc's only delimiter pair. Depth is what keeps the skip from stopping on a sync-set
/// token that is *inside* the garbage.
fn parens(kind: &BenchKind) -> Balance<()> {
  match kind {
    BenchKind::LParen => Balance::Open(()),
    BenchKind::RParen => Balance::Close(()),
    _ => Balance::Neutral,
  }
}

/// A statement is `int ;`. Anything else is a hard failure — which is what gives the
/// recovery driver something to do.
fn stmt<'inp, Ctx>(inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>) -> Result<i64, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let value = match inp.try_expect(|t| matches!(t.data(), BenchTok::Int(_)))? {
    Some(tok) => match tok.into_data() {
      BenchTok::Int(n) => n,
      _ => unreachable!("the predicate admits only integers"),
    },
    None => return Err(BenchError),
  };
  if inp
    .try_expect(|t| matches!(t.data(), BenchTok::Semi))?
    .is_none()
  {
    return Err(BenchError);
  }
  Ok(value)
}

/// Parse statements, recovering over the garbage: on failure, skip (nesting-aware) to
/// the next depth-0 statement start and retry there.
fn skip_then_retry_drain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    let done = {
      let peeked = inp.peek_one()?;
      peeked.is_none()
    };
    if done {
      break;
    }
    let mut parser = stmt.skip_then_retry(parens, |t| matches!(t.data(), BenchTok::Int(_)));
    match parser.parse_input(inp) {
      Ok(value) => {
        black_box(value);
        n += 1;
      }
      Err(_) => break,
    }
  }
  Ok(n)
}

// ── Deterministic sources ─────────────────────────────────────────────────────
//
// All generated from a counter — no randomness, no clock — so a fixture is byte-identical
// between runs and machines. Each targets ~128 KiB so a full parse dwarfs the fixed
// per-parse setup cost.

const TARGET: usize = 128 * 1024;

/// `1 2 3 …` — a long run of whitespace-separated integers for the repetition driver.
fn int_run_source() -> String {
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    let n = i.wrapping_mul(2654435761) % 100_000;
    let _ = write!(s, "{n} ");
    i = i.wrapping_add(1);
  }
  s
}

/// `1 , 2 , 3 , …` — one long comma list (no trailing separator) for the separated driver.
fn comma_list_source() -> String {
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    let n = i.wrapping_mul(2654435761) % 100_000;
    if i > 0 {
      s.push_str(", ");
    }
    let _ = write!(s, "{n}");
    i = i.wrapping_add(1);
  }
  s
}

/// `( 1 , 2 , 3 , 4 ) ( 5 , 6 , 7 , 8 ) …` — delimited comma lists, repeated.
fn paren_list_source() -> String {
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    let a = i.wrapping_mul(2654435761) % 10_000;
    let b = i.wrapping_mul(40503) % 10_000;
    let c = i % 9973;
    let _ = writeln!(s, "( {a} , {b} , {c} , {i} )");
    i = i.wrapping_add(1);
  }
  s
}

/// `val0 + 123 * val1 / 45 - 6 , 7 ;` — every one of the eight table kinds fires, and
/// every token is a dispatch target.
fn dispatch_source() -> String {
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    let a = i;
    let m = i.wrapping_mul(2654435761) % 100_000;
    let b = i % 4093;
    let _ = writeln!(s, "val{a} + {m} * val{b} / 45 - 6 , 7 ;");
    i = i.wrapping_add(1);
  }
  s
}

/// `2 ^ 3 + 4 * 5 - ( 6 / 2 ) ;` — every precedence rung and the paren pair, so the
/// operator loop recurses rather than running flat.
fn expr_source() -> String {
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    let a = i % 7 + 1;
    let b = i % 5 + 1;
    let c = i % 11 + 1;
    let d = i % 3 + 1;
    let _ = writeln!(s, "{d} ^ 3 + {a} * {b} - ( {c} / 2 ) + -{a} ;");
    i = i.wrapping_add(1);
  }
  s
}

/// `12 ;` statements, with every eighth one preceded by a parenthesised lump of garbage
/// that contains an integer — so a depth-blind skip would stop *inside* it and a
/// depth-aware one skips the whole thing. One hole per lump.
fn garbage_source() -> String {
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    if i.is_multiple_of(8) {
      let g = i % 97;
      let _ = write!(s, "( {g} + * ) ");
    }
    let n = i.wrapping_mul(2654435761) % 100_000;
    let _ = writeln!(s, "{n} ;");
    i = i.wrapping_add(1);
  }
  s
}

// ── The group ─────────────────────────────────────────────────────────────────

macro_rules! bench_parser {
  ($group:expr, $name:literal, $src:expr, $driver:ident) => {{
    let src: &str = $src;
    $group.throughput(Throughput::Bytes(src.len() as u64));
    $group.bench_function($name, |b| {
      b.iter(|| {
        let n = Parser::new()
          .apply($driver)
          .parse_str(black_box(src))
          .unwrap();
        black_box(n)
      })
    });
  }};
}

fn parser_bench(c: &mut Criterion) {
  let ints = int_run_source();
  let commas = comma_list_source();
  let parens_src = paren_list_source();
  let dispatch = dispatch_source();
  let exprs = expr_source();
  let garbage = garbage_source();

  let mut group = c.benchmark_group("parser");
  group.measurement_time(Duration::from_secs(3));
  group.warm_up_time(Duration::from_secs(1));

  bench_parser!(group, "repeated_collect", ints.as_str(), repeated_collect);
  bench_parser!(
    group,
    "separated_collect",
    commas.as_str(),
    separated_collect
  );
  bench_parser!(group, "delimited_list", parens_src.as_str(), delimited_list);
  bench_parser!(group, "dispatch_peek", dispatch.as_str(), dispatch_peek);
  bench_parser!(group, "dispatch_fused", dispatch.as_str(), dispatch_fused);
  bench_parser!(group, "pratt_expr", exprs.as_str(), pratt_expr);
  bench_parser!(
    group,
    "skip_then_retry",
    garbage.as_str(),
    skip_then_retry_drain
  );

  group.finish();
}

criterion_group!(benches, parser_bench);
criterion_main!(benches);
