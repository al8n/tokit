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

use core::{fmt::Write as _, time::Duration};
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use tokora::{
  Emitter, InputRef, Lexer, Parse, ParseChoice, ParseContext, ParseInput, ParseTokenChoice, Parser,
  State, Token,
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

criterion_group!(benches, bench, dispatch_bench);
criterion_main!(benches);
