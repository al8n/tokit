//! Scanner drain benchmarks for the `InputRef` hot paths.
//!
//! These exercise the real per-operation scanner protocol — build a fresh
//! lexer, lex within the poison boundary, dedup lexer errors, commit — over a
//! synthetic source large enough (~128 KiB) to dominate the fixed per-parse
//! setup cost, so a full drain resolves ~1-2% deltas.
//!
//! Benches:
//!   * `next_drain` — `while let Some = next()` to EOF. THE hot path.
//!   * `skip_trivia_next` — alternating `skip_while(trivia)` + `next()`.
//!   * `try_expect_hits` — `try_expect` with an always-matching predicate
//!     (commit path).
//!   * `try_expect_misses` — `try_expect` with a never-matching predicate
//!     (put-back path), interleaved with `next()` to advance.
//!   * `peek1_then_next` — `peek` + `next`. Control: `peek` is not part of the
//!     scanner-protocol unification, so this must not move; it detects
//!     accidental collateral.
//!
//! The source contains only well-formed idents/ints/puncts/whitespace, so the
//! emitter is never invoked — the measurement is pure lex/cache/commit.
//!
//! Two further groups live below: `input/dispatch` (peek vs fused dispatch, over a
//! light and a heavy lexer state) and `input/peek` — the residency probes that reach
//! the peek fill's cache-hit exit and its staged overflow region, neither of which any
//! other id in this repository enters. See that group's own header.

/// The wall-clock regression gate's measurement window, shared by all five bench binaries.
/// A no-op unless `ci/wallclock/run.sh` set its environment; see the module for what it
/// overrides and why it is not the criterion command line.
mod support;

use core::{fmt::Write as _, time::Duration};
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use generic_arraydeque::typenum::{U3, U8};

use tokora::{
  Cache, Emitter, InputRef, Lexer, Parse, ParseChoice, ParseContext, ParseInput, ParseTokenChoice,
  Parser, State, Token,
  cache::PeekedTokenExt,
  error::{UnexpectedEnd, token::UnexpectedToken},
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::Any,
  span::Spanned,
};

// ── Fixture: a small ident/int/punct/whitespace-trivia token enum ─────────────

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos)]
enum BenchTok {
  /// Whitespace trivia — kept as a token (not `skip`ped) so `skip_while`
  /// and the trivia-skipping paths have something to consume.
  #[regex(r"[ \t\r\n]+")]
  Ws,
  #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
  Ident,
  #[regex(r"[0-9]+")]
  Int,
  #[token("+")]
  #[token("-")]
  #[token("*")]
  #[token("/")]
  #[token("=")]
  #[token(",")]
  #[token(";")]
  Punct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BenchKind {
  Ws,
  Ident,
  Int,
  Punct,
}

impl core::fmt::Display for BenchKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let s = match self {
      BenchKind::Ws => "whitespace",
      BenchKind::Ident => "identifier",
      BenchKind::Int => "integer",
      BenchKind::Punct => "punctuation",
    };
    f.write_str(s)
  }
}

impl core::fmt::Display for BenchTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

impl Token<'_> for BenchTok {
  type Kind = BenchKind;
  type Error = ();

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  fn kind(&self) -> BenchKind {
    match self {
      BenchTok::Ws => BenchKind::Ws,
      BenchTok::Ident => BenchKind::Ident,
      BenchTok::Int => BenchKind::Int,
      BenchTok::Punct => BenchKind::Punct,
    }
  }

  fn is_trivia(&self) -> bool {
    matches!(self, BenchTok::Ws)
  }
}

type BenchLexer<'a> = LogosLexer<'a, BenchTok>;

// A trivial emitter error: the source is well-formed, so these `From`s are only
// needed to satisfy the `FromEmitterError` bound `Parser::new` requires — they
// are never constructed at runtime.
#[derive(Debug, Default, Clone)]
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

// The dispatch benches route the committed-dispatch-failure / end-of-input errors of
// `DispatchOnKind` (and the `Any` arms) through the `Err` channel; the source is
// well-formed so these are only ever the final end-of-input at drain end.
impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for BenchError {
  fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self {
    BenchError
  }
}

// The bundle's delimiter half: one generic impl for every pair.
impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for BenchError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    BenchError
  }
}

// ── Scan drivers (generic over the parse context, as external callers write) ──

fn next_drain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  while let Some(tok) = inp.next()? {
    black_box(&tok);
    n += 1;
  }
  Ok(n)
}

fn skip_trivia_next<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    inp.skip_while(|t| t.data.is_trivia())?;
    match inp.next()? {
      Some(tok) => {
        black_box(&tok);
        n += 1;
      }
      None => break,
    }
  }
  Ok(n)
}

fn try_expect_hits<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  // Always-matching predicate: every token commits via the on-input hit path.
  while let Some(tok) = inp.try_expect(|_t| true)? {
    black_box(&tok);
    n += 1;
  }
  Ok(n)
}

fn try_expect_misses<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    // Never-matching predicate: the on-input scan lexes one token, peeks it,
    // and puts it back into the cache (the miss path). `next()` then consumes
    // that cached token (no re-lex) so the scan advances one token per cycle.
    let put_back = inp.try_expect(|_t| false)?;
    debug_assert!(put_back.is_none());
    black_box(&put_back);
    match inp.next()? {
      Some(tok) => {
        black_box(&tok);
        n += 1;
      }
      None => break,
    }
  }
  Ok(n)
}

fn peek1_then_next<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    // Scope the peek borrow so it is released before `next()`.
    let got = {
      let peeked = inp.peek_one()?;
      peeked.is_some()
    };
    black_box(got);
    match inp.next()? {
      Some(tok) => {
        black_box(&tok);
        n += 1;
      }
      None => break,
    }
  }
  Ok(n)
}

// ── Synthetic source ──────────────────────────────────────────────────────────

/// ~128 KiB of well-formed `ident = int + ident ;` lines. Every byte belongs to
/// a token (ident/int/punct/whitespace), so the lexer never errors.
fn synthetic_source() -> String {
  const TARGET: usize = 128 * 1024;
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    let a = i;
    let m = i.wrapping_mul(2654435761) % 100_000;
    let b = i % 4093;
    let _ = writeln!(s, "var{a} = {m} + val{b} ;");
    i = i.wrapping_add(1);
  }
  s
}

fn bench(c: &mut Criterion) {
  let src = synthetic_source();

  let mut group = c.benchmark_group("input/scan");
  group.throughput(Throughput::Bytes(src.len() as u64));
  group.measurement_time(Duration::from_secs(3));
  group.warm_up_time(Duration::from_secs(1));
  support::gate_overrides(&mut group);

  group.bench_function("next_drain", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(next_drain)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("skip_trivia_next", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(skip_trivia_next)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("try_expect_hits", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(try_expect_hits)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("try_expect_misses", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(try_expect_misses)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("peek1_then_next", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(peek1_then_next)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.finish();
}

// ── Fixture: an N-kind dispatch token enum (dense discriminants) ──────────────
//
// Eight distinct kinds so a kind-keyed dispatch table does real linear-scan work
// per token (the `position` lookup `DispatchOnKind` runs). Same ident/int/punct/
// whitespace shape as `BenchTok`, but `+ * = , ;` each get their own kind rather
// than collapsing into one `Punct`. The discriminants are dense (0..8) so a match
// on kind compiles to a jump table.

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos)]
enum DispTok {
  #[regex(r"[ \t\r\n]+")]
  Ws,
  #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
  Ident,
  #[regex(r"[0-9]+")]
  Int,
  #[token("+")]
  Plus,
  #[token("*")]
  Star,
  #[token("=")]
  Eq,
  #[token(",")]
  Comma,
  #[token(";")]
  Semi,
}

/// Deliberately heavy lexer state: a 256-byte array the cache must clone faithfully
/// on every staged token. Legal (`Clone + Default + State`) but an anti-pattern — it
/// quantifies the `CachedToken` state-clone cost the peek→consume round trip pays and
/// the fused lex-once→commit shape avoids.
#[derive(Debug, Clone, Default)]
// `scratch` is never read by bench code — it exists purely to give `State` clones a
// 256-byte copy cost. The clone (a memcpy the analysis ignores) is the whole point.
#[allow(dead_code)]
struct HeavyState {
  scratch: [u64; 32],
}

impl State for HeavyState {
  type Error = ();

  fn check(&self) -> Result<(), ()> {
    Ok(())
  }
}

// Same grammar as `DispTok`, but carrying the heavy state so the cache's per-token
// `State` clone is a 256-byte copy.
#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, extras = HeavyState)]
enum DispTokHeavy {
  #[regex(r"[ \t\r\n]+")]
  Ws,
  #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
  Ident,
  #[regex(r"[0-9]+")]
  Int,
  #[token("+")]
  Plus,
  #[token("*")]
  Star,
  #[token("=")]
  Eq,
  #[token(",")]
  Comma,
  #[token(";")]
  Semi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DispKind {
  Ws,
  Ident,
  Int,
  Plus,
  Star,
  Eq,
  Comma,
  Semi,
}

impl core::fmt::Display for DispKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let s = match self {
      DispKind::Ws => "whitespace",
      DispKind::Ident => "identifier",
      DispKind::Int => "integer",
      DispKind::Plus => "'+'",
      DispKind::Star => "'*'",
      DispKind::Eq => "'='",
      DispKind::Comma => "','",
      DispKind::Semi => "';'",
    };
    f.write_str(s)
  }
}

/// The dispatch table shared by every dispatch driver: `table[i]` is the viable
/// first-token kind for branch `i`, in branch order. Eight arms → `Branch<7>`.
const DISP_TABLE: &[DispKind] = &[
  DispKind::Ws,
  DispKind::Ident,
  DispKind::Int,
  DispKind::Plus,
  DispKind::Star,
  DispKind::Eq,
  DispKind::Comma,
  DispKind::Semi,
];

macro_rules! disp_token_impl {
  ($tok:ident) => {
    impl core::fmt::Display for $tok {
      fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.kind(), f)
      }
    }

    impl Token<'_> for $tok {
      type Kind = DispKind;
      type Error = ();

      const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

      fn kind(&self) -> DispKind {
        match self {
          $tok::Ws => DispKind::Ws,
          $tok::Ident => DispKind::Ident,
          $tok::Int => DispKind::Int,
          $tok::Plus => DispKind::Plus,
          $tok::Star => DispKind::Star,
          $tok::Eq => DispKind::Eq,
          $tok::Comma => DispKind::Comma,
          $tok::Semi => DispKind::Semi,
        }
      }

      fn is_trivia(&self) -> bool {
        matches!(self, $tok::Ws)
      }
    }
  };
}

disp_token_impl!(DispTok);
disp_token_impl!(DispTokHeavy);

type DispLexer<'a> = LogosLexer<'a, DispTok>;
type HeavyLexer<'a> = LogosLexer<'a, DispTokHeavy>;

// ── Dispatch drivers ──────────────────────────────────────────────────────────
//
// Four shapes over the same N-kind stream, stamped for the light and heavy lexers:
//   * `peek_combinator` — the real `DispatchOnKind` combinator surface: peek one,
//     look the kind up in the table, run the winning `Any` arm (which consumes the
//     cache-staged token). THE peek shape.
//   * `peek_inputref` — the underlying InputRef peek path by hand: `peek_one` +
//     match-on-kind + `next`. Same round trip, no combinator wrapper.
//   * `fused_inputref` — the fused try_expect shape: `try_expect_map` lexes once,
//     classifies on kind, and commits directly — no cache round trip. The ceiling
//     a fused dispatch combinator would reach.
//   * `fused_combinator` — the real `FusedDispatchOnKind` combinator surface: the
//     same lex-once → classify → commit path as `fused_inputref`, but reached through
//     the combinator (a `ParseTokenChoice` tuple + table). Proves the combinator layer
//     reaches the raw fused ceiling. THE fused shape the deliverable ships.

/// A no-op fused dispatch arm: receives the already-lexed head token (the token the fused
/// dispatcher consumed to classify it) and does nothing but keep it live for the optimizer.
/// Eight copies of this form the `FusedDispatchOnKind` arm tuple in `fused_combinator`.
fn dispatch_head_arm<'inp, L, Ctx>(
  head: Spanned<L::Token, L::Span>,
  _inp: &mut InputRef<'inp, '_, L, Ctx>,
) -> Result<(), BenchError>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L>,
  Ctx::Emitter: Emitter<'inp, L, Error = BenchError>,
{
  black_box(&head);
  Ok(())
}

macro_rules! dispatch_drivers {
  ($lexer:ident, $peekc:ident, $peekr:ident, $fused:ident, $fusedc:ident) => {
    fn $peekc<'inp, Ctx>(
      inp: &mut InputRef<'inp, '_, $lexer<'inp>, Ctx>,
    ) -> Result<usize, BenchError>
    where
      Ctx: ParseContext<'inp, $lexer<'inp>>,
      Ctx::Emitter: Emitter<'inp, $lexer<'inp>, Error = BenchError>,
    {
      let mut n = 0usize;
      let mut parser = (
        Any::<$lexer<'inp>, Ctx>::new(),
        Any::<$lexer<'inp>, Ctx>::new(),
        Any::<$lexer<'inp>, Ctx>::new(),
        Any::<$lexer<'inp>, Ctx>::new(),
        Any::<$lexer<'inp>, Ctx>::new(),
        Any::<$lexer<'inp>, Ctx>::new(),
        Any::<$lexer<'inp>, Ctx>::new(),
        Any::<$lexer<'inp>, Ctx>::new(),
      )
        .dispatch_on_kind(DISP_TABLE);
      loop {
        match parser.parse_input(inp) {
          Ok(tok) => {
            black_box(&tok);
            n += 1;
          }
          // Well-formed source + complete table: the only Err is the final EOT.
          Err(_) => break,
        }
      }
      Ok(n)
    }

    fn $peekr<'inp, Ctx>(
      inp: &mut InputRef<'inp, '_, $lexer<'inp>, Ctx>,
    ) -> Result<usize, BenchError>
    where
      Ctx: ParseContext<'inp, $lexer<'inp>>,
      Ctx::Emitter: Emitter<'inp, $lexer<'inp>, Error = BenchError>,
    {
      let mut n = 0usize;
      loop {
        let hit = {
          match inp.peek_one()? {
            Some(peeked) => {
              let kind = peeked.token().kind();
              black_box(DISP_TABLE.iter().position(|c| *c == kind));
              true
            }
            None => false,
          }
        };
        if !hit {
          break;
        }
        let tok = inp.next()?;
        black_box(&tok);
        n += 1;
      }
      Ok(n)
    }

    fn $fused<'inp, Ctx>(
      inp: &mut InputRef<'inp, '_, $lexer<'inp>, Ctx>,
    ) -> Result<usize, BenchError>
    where
      Ctx: ParseContext<'inp, $lexer<'inp>>,
      Ctx::Emitter: Emitter<'inp, $lexer<'inp>, Error = BenchError>,
    {
      let mut n = 0usize;
      // `try_expect_map` lexes one token, classifies on kind, and commits the token
      // directly on a match — the fused path, no cache staging. Always matches here.
      while let Some((idx, tok)) =
        inp.try_expect_map(|t| DISP_TABLE.iter().position(|c| *c == t.data.kind()))?
      {
        black_box((idx, &tok));
        n += 1;
      }
      Ok(n)
    }

    fn $fusedc<'inp, Ctx>(
      inp: &mut InputRef<'inp, '_, $lexer<'inp>, Ctx>,
    ) -> Result<usize, BenchError>
    where
      Ctx: ParseContext<'inp, $lexer<'inp>>,
      Ctx::Emitter: Emitter<'inp, $lexer<'inp>, Error = BenchError>,
    {
      let mut n = 0usize;
      let mut parser = (
        dispatch_head_arm,
        dispatch_head_arm,
        dispatch_head_arm,
        dispatch_head_arm,
        dispatch_head_arm,
        dispatch_head_arm,
        dispatch_head_arm,
        dispatch_head_arm,
      )
        .fused_dispatch_on_kind(DISP_TABLE);
      loop {
        match parser.parse_input(inp) {
          Ok(out) => {
            black_box(&out);
            n += 1;
          }
          // Well-formed source + complete table: the only Err is the final EOT.
          Err(_) => break,
        }
      }
      Ok(n)
    }
  };
}

dispatch_drivers!(
  DispLexer,
  dispatch_peek_combinator,
  dispatch_peek_inputref,
  dispatch_fused_inputref,
  dispatch_fused_combinator
);
dispatch_drivers!(
  HeavyLexer,
  dispatch_peek_combinator_heavy,
  dispatch_peek_inputref_heavy,
  dispatch_fused_inputref_heavy,
  dispatch_fused_combinator_heavy
);

/// ~128 KiB of well-formed `var = int * ident , int + ident ;` lines. Every token is
/// a dispatch target and all eight kinds fire. Kept separate from `synthetic_source`
/// so the existing scanner benches keep their fixture (and baseline) unchanged.
fn dispatch_source() -> String {
  const TARGET: usize = 128 * 1024;
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  while s.len() < TARGET {
    let a = i;
    let m = i.wrapping_mul(2654435761) % 100_000;
    let b = i % 4093;
    let _ = writeln!(s, "var{a} = {m} * val{b} , {a} + w{b} ;");
    i = i.wrapping_add(1);
  }
  s
}

fn dispatch_bench(c: &mut Criterion) {
  let src = dispatch_source();

  let mut group = c.benchmark_group("input/dispatch");
  group.throughput(Throughput::Bytes(src.len() as u64));
  group.measurement_time(Duration::from_secs(3));
  group.warm_up_time(Duration::from_secs(1));
  support::gate_overrides(&mut group);

  macro_rules! bench_driver {
    ($name:literal, $driver:ident) => {
      group.bench_function($name, |b| {
        b.iter(|| {
          let n = Parser::new()
            .apply($driver)
            .parse_str(black_box(src.as_str()))
            .unwrap();
          black_box(n)
        })
      });
    };
  }

  bench_driver!("peek_combinator", dispatch_peek_combinator);
  bench_driver!("peek_inputref", dispatch_peek_inputref);
  bench_driver!("fused_inputref", dispatch_fused_inputref);
  bench_driver!("fused_combinator", dispatch_fused_combinator);
  bench_driver!("peek_combinator_heavy", dispatch_peek_combinator_heavy);
  bench_driver!("peek_inputref_heavy", dispatch_peek_inputref_heavy);
  bench_driver!("fused_inputref_heavy", dispatch_fused_inputref_heavy);
  bench_driver!("fused_combinator_heavy", dispatch_fused_combinator_heavy);

  group.finish();
}

// ── Peek-fill residency probes (group `input/peek`) ───────────────────────────
//
// `InputRef::peek_with_emitter_inner` — the one fill every peek in the crate goes
// through — has two paths that, before this group, no shipped bench id in this
// repository entered:
//
//   * the **cache-hit exit**. The fill computes `want = window - (parked + in_cache)`
//     and returns before touching the lexer when that saturates to zero. A driver that
//     consumes what it peeked before peeking again meets that arm never: `want` is 1
//     every time.
//   * the **staged region and the rotation that lands it**. A window wider than the
//     cache can hold makes the fill retain what fits and stage the rest at the tail of
//     the caller's own window, then copy the cache region in and `rotate_left` the
//     staged run behind it. `DefaultCache` is `GenericArrayDeque<_, U3>`, so nothing
//     narrower than a width-4 peek can reach any of it. That path also carries
//     `assert_cache_copy`'s **release-active endpoint witness**, which exists because a
//     `Cache::len` that under-reports clips the copy and the rotation then closes the
//     gap with tokens that belong after it — a hole in the window rather than a short
//     one. The fill's overflow-free exit returns before all three.
//
// That all of this was unreached was measured, not surveyed: on 2026-08-03, against
// `1fbd16e`, with an env-selected `panic!` planted at each of the four sites (hit exit,
// staging push, endpoint witness, `rotate_left`), all 45 shipped ids across
// `input_scan`, `parser_combinators`, `pratt_typed`, `cst` and `backtrack` ran clean
// under `--test` at every site — 180 runs. `r4_peek1_hit8` tripped the hit exit alone;
// `r4_peek8_heavy_staged` tripped the other three and not the hit exit. That is a record
// of one run and nothing here re-derives it — what the committed code does check is the
// weaker and more useful thing: that each probe below still reaches the path its id
// names. Both drivers carry the arithmetic that reaches their exit as a `CHECK`-gated
// assertion rather than as a comment; see `probe_calibration`. `CHECK` is a const
// parameter, so the measured monomorphisation contains none of it.
//
// The fixtures are `dispatch_source()` and the 8-kind `DispTok` / `DispTokHeavy`
// lexers above: the heavy one is what makes the staged path's per-token `L::State`
// clone a visible 256-byte copy rather than a no-op. The group is its own so the
// `input/scan` and `input/dispatch` ids — and their baselines — are untouched.
//
// ── First reading of the staged path, and it cannot be re-derived ─────────────
//
// A DATED RECORD, not a claim this file checks. These ids were first run on
// 2026-08-03 across the merge of #164, which replaced a separate `W::CAPACITY`
// staging array with in-buffer staging plus one `rotate_left` and promoted
// `assert_cache_copy`'s endpoint half to release-active. Same machine
// (aarch64-apple-darwin), same fixture, same `bench` profile, one commit apart —
// and the pre-#164 half can no longer be reproduced, because that shape is gone.
//
//   | build                                  | r4_peek1_hit8 | r4_peek8_heavy_staged |
//   |----------------------------------------|---------------|-----------------------|
//   | d59bb4e, separate staging array        | 596.06 µs     | 2.7728 ms             |
//   | 1fbd16e, in-buffer staging + rotation  | 588.7–595.2 µs| 4.5049–4.5647 ms      |
//   | 1fbd16e, endpoint witness no-oped      | —             | 4.2915 ms             |
//
// So on THIS fixture the hit exit is unmoved (−1.2%, and p = 0.36 between two runs
// of the same build) while the staged path is ~1.63× slower, of which the
// release-active witness is ~1.05× and the staging/rotation restructure the rest.
// The hit id is the control that says the machine did not move between the two
// sessions.
//
// Read that as bounded, because the fixture is deliberately extreme and was built to
// make the path VISIBLE rather than representative: a width-8 window over a 3-slot
// cache means five entries staged and rotated per fill, and a 256-byte `L::State`
// makes each entry a ~280-byte move, so one fill rotates multiple KiB. A grammar
// peeking `U2`/`U3` with a small state stages nothing at all and never reaches this
// code. It is also NOT a verdict on #164, whose subject was the window's stack
// footprint — halved, for every wide peek — and which measured 1.11×–1.31× *gains*
// on the six dispatch ids that take the overflow-free exit. What this records is the
// one arm nothing else measures.

/// Capacity of [`tokora::DefaultCache`], which is what both probes are sized against.
///
/// Not a guess: `probe_calibration` proves the cache really holds this many and refuses
/// to register the group otherwise, so a change to `DefaultCache`'s width fails the
/// bench instead of quietly turning `r4_peek8_heavy_staged` into an ordinary miss and
/// `r4_peek1_hit8` into an ordinary fill.
const DEFAULT_CACHE_SLOTS: usize = 3;

/// The width `r4_peek8_heavy_staged` peeks at, as a `usize` beside the `U8` it is spelled
/// as in the type position. Anything above [`DEFAULT_CACHE_SLOTS`] overflows; eight leaves
/// five tokens in the staged region, enough that the path is not a rounding error against
/// the three the cache keeps.
const STAGED_WINDOW: usize = 8;

/// Width-1 peeks over a **primed** cache — the fill's cache-hit exit.
///
/// One width-`U3` peek fills the cache exactly (`want` is 3 on entry, every lexed token
/// fits, nothing overflows); the three width-1 peeks that follow each find the window
/// already met and return before the lexer is touched. Three quarters of the peeks this
/// driver issues take the hit exit; the priming peek is the miss that refills for them.
fn peek1_cache_hit<'inp, Ctx, const CHECK: bool>(
  inp: &mut InputRef<'inp, '_, DispLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, DispLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, DispLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  let mut hits = 0usize;
  'drain: loop {
    // Prime. Scoped so the window borrow is released before anything else runs.
    let primed = { inp.peek::<U3>()?.len() };
    black_box(primed);
    // How many of those the cache actually retained, which is how many width-1 peeks
    // can be answered from it. `probe_calibration` pins that a width-`U3` peek over
    // `DefaultCache` stages nothing, so this is the whole primed window — but reading
    // it rather than assuming `DEFAULT_CACHE_SLOTS` is what keeps the last, short
    // window at the end of the fixture from issuing a peek the cache cannot answer.
    let resident = inp.cache().len();
    if resident == 0 {
      break;
    }
    for _ in 0..resident {
      if CHECK {
        // The reach condition, in the fill's own arithmetic: `peek_one` asks for a
        // width-1 window, so `remaining_cap` is 1 and `want` is
        // `1usize.saturating_sub(parked + in_cache)`. A non-empty cache makes that zero
        // whatever `parked` is, which IS the cache-hit exit — the return that happens
        // before the boundary probe and before a lexer is ever built.
        assert!(
          !inp.cache().is_empty(),
          "input/peek/r4_peek1_hit8: the cache is empty at a width-1 peek, so the fill \
           takes its lexing path and this probe covers nothing"
        );
        hits += 1;
      }
      let got = {
        let peeked = inp.peek_one()?;
        peeked.is_some()
      };
      black_box(got);
      match inp.next()? {
        Some(tok) => {
          black_box(&tok);
          n += 1;
        }
        None => break 'drain,
      }
    }
  }
  if CHECK {
    assert!(
      hits > 0,
      "input/peek/r4_peek1_hit8: not one width-1 peek was issued over a non-empty cache"
    );
  }
  Ok(n)
}

/// Width-8 peeks over a 3-slot cache, under the heavy lexer state — the fill's staged
/// region, its endpoint witness and its rotation.
///
/// `want` is 8 on entry with the cache empty; the first `DEFAULT_CACHE_SLOTS` tokens are
/// retained and the remaining five are staged at the tail of the window, which is what
/// makes this the only shipped id that reaches `assert_cache_copy` — the `Cache`-contract
/// witness that is release-active precisely because the `rotate_left` after it would turn
/// a clipped cache copy into a hole — and the rotation itself. Exactly the cache-resident
/// prefix is then consumed, so the next peek starts empty and overflows by the same five:
/// the staged tokens are not durable and the consume path re-lexes them, which is why this
/// driver costs more per byte than the ids beside it.
fn peek8_heavy_staged<'inp, Ctx, const CHECK: bool>(
  inp: &mut InputRef<'inp, '_, HeavyLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, HeavyLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, HeavyLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  let mut staged = 0usize;
  'drain: loop {
    let window = { inp.peek::<U8>()?.len() };
    black_box(window);
    if window == 0 {
      break;
    }
    if CHECK && window == STAGED_WINDOW {
      // The reach condition, from the fill's accounting and `Cache::peek`'s law. The
      // returned window is `parked + in_cache + staged` entries; `Cache::peek` appends
      // exactly `min(cache.len(), room)` of them and the cache has not moved since the
      // fill wrote it, so `window - cache.len() - 1` is a lower bound on the staged
      // region (the `- 1` charges the parked slot, which `DefaultCache` never uses).
      // Anything above zero is a token the fill had to hold outside the cache.
      //
      // Only a **full** window is judged: the last peek of a fixture comes back short
      // because the input ran out, and a short window that fits in the cache has not
      // failed to overflow — it had nothing to overflow with.
      let floor = window.saturating_sub(inp.cache().len() + 1);
      assert!(
        floor > 0,
        "input/peek/r4_peek8_heavy_staged: a full width-{STAGED_WINDOW} peek came back \
         inside the cache's own residency, so nothing overflowed and this probe covers \
         nothing"
      );
      staged += floor;
    }
    // Drain exactly the cache-resident prefix.
    for _ in 0..DEFAULT_CACHE_SLOTS {
      match inp.next()? {
        Some(tok) => {
          black_box(&tok);
          n += 1;
        }
        None => break 'drain,
      }
    }
  }
  if CHECK {
    assert!(
      staged > 0,
      "input/peek/r4_peek8_heavy_staged: not one peek overflowed the cache"
    );
  }
  Ok(n)
}

/// The `CHECK = true` pass, run once per driver before the group is registered.
///
/// It is the same code the measured ids run, one const parameter apart, so it cannot
/// drift from them; and it is outside `b.iter`, so the assertions cost the measurement
/// nothing. What it establishes is that each probe's window arithmetic really lands on
/// the fill exit its id names — not that the input has the right shape.
///
/// It also pins [`DEFAULT_CACHE_SLOTS`] behaviourally: a width-`U3` peek over a fresh
/// input must come back with three entries, all of them cache-resident.
fn probe_calibration(src: &str) {
  fn cache_residency<'inp, Ctx>(
    inp: &mut InputRef<'inp, '_, DispLexer<'inp>, Ctx>,
  ) -> Result<usize, BenchError>
  where
    Ctx: ParseContext<'inp, DispLexer<'inp>>,
    Ctx::Emitter: Emitter<'inp, DispLexer<'inp>, Error = BenchError>,
  {
    let window = { inp.peek::<U3>()?.len() };
    assert_eq!(
      window,
      inp.cache().len(),
      "input/peek: a width-3 peek staged something, so `DefaultCache` is narrower than \
       {DEFAULT_CACHE_SLOTS} slots and both probes are mis-sized"
    );
    Ok(window)
  }

  let residency = Parser::new().apply(cache_residency).parse_str(src).unwrap();
  assert_eq!(
    residency, DEFAULT_CACHE_SLOTS,
    "input/peek: `DefaultCache` no longer retains {DEFAULT_CACHE_SLOTS} tokens, so \
     `r4_peek1_hit8` may no longer reach the cache-hit exit and `r4_peek8_heavy_staged` \
     may no longer overflow"
  );

  // The token counts a plain drain of the same fixture reports, one per lexer. A probe
  // that returns fewer has stopped early over a source criterion still charges
  // `Throughput::Bytes` for in full — the one way these ids could be wrong and look
  // better for it.
  let light = Parser::new()
    .apply(dispatch_peek_inputref)
    .parse_str(src)
    .unwrap();
  let heavy = Parser::new()
    .apply(dispatch_peek_inputref_heavy)
    .parse_str(src)
    .unwrap();

  let n = Parser::new()
    .apply(peek1_cache_hit::<_, true>)
    .parse_str(src)
    .unwrap();
  assert_eq!(
    n, light,
    "input/peek/r4_peek1_hit8: the probe consumed {n} tokens where a plain drain of the \
     same fixture consumes {light}"
  );

  let n = Parser::new()
    .apply(peek8_heavy_staged::<_, true>)
    .parse_str(src)
    .unwrap();
  assert_eq!(
    n, heavy,
    "input/peek/r4_peek8_heavy_staged: the probe consumed {n} tokens where a plain drain \
     of the same fixture consumes {heavy}"
  );
}

fn peek_probe_bench(c: &mut Criterion) {
  let src = dispatch_source();
  probe_calibration(src.as_str());

  let mut group = c.benchmark_group("input/peek");
  group.throughput(Throughput::Bytes(src.len() as u64));
  group.measurement_time(Duration::from_secs(3));
  group.warm_up_time(Duration::from_secs(1));
  support::gate_overrides(&mut group);

  group.bench_function("r4_peek1_hit8", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(peek1_cache_hit::<_, false>)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("r4_peek8_heavy_staged", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(peek8_heavy_staged::<_, false>)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.finish();
}

criterion_group!(benches, bench, dispatch_bench, peek_probe_bench);
criterion_main!(benches);
