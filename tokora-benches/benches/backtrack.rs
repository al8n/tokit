//! Backtracking and guard micro-benchmarks for the `InputRef` machinery.
//!
//! The scanner hot paths live in `input_scan.rs`. This target instead
//! quantifies the *absolute per-operation* cost of the backtracking machinery
//! the correctness campaign hangs off — the live-checkpoint lineage stack, the
//! pin set, and the snapshot/restore copies — so an upcoming lineage
//! consolidation has a gate covering these ops and their cost can be read
//! against plain token consumption.
//!
//! Every bench drains a modest (~12 KiB) source token by token so per-op cost
//! dominates but real lexing still happens. Each op is expressed as one real
//! token of progress, so its time divided by the reference below is a clean
//! "how many plain consumes does this machinery cost" multiple.
//!
//! Benches (each does one unit of the named machinery per real token consumed):
//!   * `save_commit_per_token` — `save` + `next` + `commit`. The retry-loop
//!     success shape: lineage push then O(1) forget.
//!   * `save_restore_per_token` — `save` + speculative `next` + `restore` +
//!     real `next`. The rollback shape: push, pop-through, pure-copy restore,
//!     and one re-lex.
//!   * `txn_begin_commit_per_token` — `begin` guard flavour of the first
//!     (pin push/unpin + guard settle on top of save/commit).
//!   * `txn_begin_drop_rollback_per_token` — undecided-drop flavour of the
//!     rollback shape (the guard's `Drop` rewinds).
//!   * `stacked_savepoint_cycle` — one `begin_stacked` per 16 tokens; inside,
//!     per token: `savepoint`, a few speculative consumes, `rollback_to` (with
//!     its re-save), a real consume, `release`.
//!   * `attempt_decline_per_token` — `attempt` whose closure consumes one token
//!     then declines, forcing the rollback arm; then a real consume.
//!   * `failed_sync_through_over_8` — `sync_through` with a never-matching
//!     predicate over an 8-token window: the entry-snapshot + no-trace rewind
//!     cost. Uses a collecting (`Verbose`) emitter so the emissions the failed
//!     scan makes are actually recorded and then unwound (the "no trace" the
//!     rewind restores), and so the pure-rewind no-op is not elided.
//!   * `plain_next_drain_reference` — the same source drained with bare `next`.
//!     The in-target reference: every op above reads as a multiple of it.
//!
//! The per-token benches use the default fail-fast emitter (as in
//! `input_scan.rs`): the source is well-formed, so it is never invoked; the
//! measurement is pure backtracking machinery.

/// The wall-clock regression gate's measurement window, shared by all five bench binaries.
/// A no-op unless `ci/wallclock/run.sh` set its environment; see the module for what it
/// overrides and why it is not the criterion command line.
mod support;

use core::{fmt::Write as _, time::Duration};
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, Token,
  emitter::Verbose,
  error::token::UnexpectedToken,
  lexer::LogosLexer,
  logos::{self, Logos},
};

// ── Fixture: the same ident/int/punct/whitespace-trivia enum as `input_scan` ──

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos)]
enum BenchTok {
  /// Whitespace trivia — kept as a token (not `skip`ped) so every byte of the
  /// source becomes a token to consume.
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

// A trivial emitter error. The per-token benches never emit (well-formed
// source); the `From`s only satisfy the `FromEmitterError` bound `Parser`
// requires. `failed_sync_through_over_8` *does* emit (into a `Verbose`), which
// converts each unexpected token through the second `From`.
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

// The token-level bundle's remaining members: one `Set`-generic end-of-input impl covers both
// the default and the `Kind`-table spellings, and one generic `FromUnclosed` covers every
// delimiter pair. Never constructed here — the sources are well-formed.
impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEnd<H, O, Lang, Set>>
  for BenchError
{
  fn from(_: tokora::error::UnexpectedEnd<H, O, Lang, Set>) -> Self {
    BenchError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for BenchError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    BenchError
  }
}

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Target size of the shared per-token source. ~12 KiB keeps the fixed
/// per-parse setup negligible while total runtime stays moderate.
const SOURCE_TARGET: usize = 12 * 1024;
/// Speculative consumes per savepoint in `stacked_savepoint_cycle`.
const SPECULATIVE_CONSUME: usize = 3;
/// Savepoints taken per `begin_stacked` in `stacked_savepoint_cycle`.
const SAVEPOINTS_PER_TXN: usize = 16;
/// Number of failed `sync_through` calls per `failed_sync_through_over_8`
/// invocation. Each rewinds, so the window is re-scanned every time; looping
/// amortizes the one-time parse setup over many failed syncs.
const SYNC_COUNT: usize = 256;

// ── Backtracking drivers (generic over the parse context, as callers write) ───

/// `save` + `next` + `commit` per token — the retry-loop success shape.
fn save_commit_per_token<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    let ckp = inp.save();
    match inp.next()? {
      Some(tok) => {
        black_box(&tok);
        n += 1;
        inp.commit(ckp);
      }
      None => {
        inp.commit(ckp);
        break;
      }
    }
  }
  Ok(n)
}

/// `save` + speculative `next` + `restore` + real `next` per token — the
/// rollback shape (push, pop-through, pure-copy restore, one re-lex).
fn save_restore_per_token<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    let ckp = inp.save();
    match inp.next()? {
      Some(spec) => {
        black_box(&spec);
        // Roll the speculative consume back, then consume it for real: this is
        // the push + pop-through + pure-copy restore + one re-lex the rollback
        // path pays.
        inp.restore(ckp);
        match inp.next()? {
          Some(tok) => {
            black_box(&tok);
            n += 1;
          }
          // Unreachable: the speculative read just returned `Some`.
          None => break,
        }
      }
      None => {
        inp.restore(ckp);
        break;
      }
    }
  }
  Ok(n)
}

/// `begin` guard flavour of `save_commit_per_token` (pin push/unpin + guard
/// settle on top).
fn txn_begin_commit_per_token<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    let mut txn = inp.begin();
    match txn.next()? {
      Some(tok) => {
        black_box(&tok);
        n += 1;
        txn.commit();
      }
      None => {
        txn.commit();
        break;
      }
    }
  }
  Ok(n)
}

/// Undecided-drop flavour of the rollback shape: a `begin` guard consumes one
/// token then drops undecided (rolling back), and the token is consumed for
/// real afterward.
fn txn_begin_drop_rollback_per_token<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    // Speculatively consume one token through an undecided guard; its `Drop`
    // rolls the input back to the begin point.
    {
      let mut txn = inp.begin();
      if let Some(spec) = txn.next()? {
        black_box(&spec);
      }
    }
    // Now consume it for real (re-lexing the rolled-back token).
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

/// One `begin_stacked` per 16 tokens; inside, per token: `savepoint`, a few
/// speculative consumes, `rollback_to` (paying its re-save), a real consume,
/// then `release`. Exercises the savepoint machinery including the re-save.
fn stacked_savepoint_cycle<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    let mut txn = inp.begin_stacked();
    let mut made_progress = false;
    for _ in 0..SAVEPOINTS_PER_TXN {
      let sp = txn.savepoint();
      // Speculatively consume a few tokens...
      for _ in 0..SPECULATIVE_CONSUME {
        match txn.next()? {
          Some(tok) => {
            black_box(&tok);
          }
          None => break,
        }
      }
      // ...then roll back to the savepoint (which re-saves so `sp` survives)...
      txn.rollback_to(sp);
      // ...and consume exactly one token for real.
      let got = match txn.next()? {
        Some(tok) => {
          black_box(&tok);
          n += 1;
          true
        }
        None => false,
      };
      // Release the savepoint, keeping the one token of progress.
      txn.release(sp);
      if !got {
        break;
      }
      made_progress = true;
    }
    txn.commit();
    if !made_progress {
      break;
    }
  }
  Ok(n)
}

/// `attempt` whose closure consumes one token then declines (returns `None`),
/// forcing the rollback arm; then the token is consumed for real.
fn attempt_decline_per_token<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  loop {
    // The attempt consumes one token inside the closure, then declines — the
    // `None` arm unpins and rolls back to the attempt's own checkpoint.
    let declined = inp.attempt(|inp| match inp.next() {
      Ok(Some(spec)) => {
        black_box(&spec);
        None::<()>
      }
      _ => None,
    });
    black_box(&declined);
    // Consume the token for real (re-lexing after the attempt's rollback).
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

/// A never-matching `sync_through` over an 8-token window, repeated. Each call
/// snapshots the entry position, skips-and-diagnoses all 8 tokens, hits end of
/// input, and rewinds the whole thing (position, lexer state, dedup watermark,
/// and the emitted diagnostics) leaving no trace — so the next call re-scans
/// the same window. Returns the number of failed syncs performed.
fn failed_sync_through_over_8<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, BenchLexer<'inp>, Ctx>,
) -> Result<usize, BenchError>
where
  Ctx: ParseContext<'inp, BenchLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, BenchLexer<'inp>, Error = BenchError>,
{
  let mut n = 0usize;
  for _ in 0..SYNC_COUNT {
    let matched = inp.sync_through(|_| false, || None)?;
    debug_assert!(matched.is_none());
    black_box(&matched);
    n += 1;
  }
  Ok(n)
}

/// The in-target reference: drain the same source with bare `next`.
fn plain_next_drain_reference<'inp, Ctx>(
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

// ── Synthetic sources ─────────────────────────────────────────────────────────

/// ~12 KiB of well-formed `ident = int + ident ;` lines. Every byte belongs to
/// a token, so the lexer never errors and the per-token benches never emit.
fn synthetic_source() -> String {
  let mut s = String::with_capacity(SOURCE_TARGET + 64);
  let mut i = 0u32;
  while s.len() < SOURCE_TARGET {
    let a = i;
    let m = i.wrapping_mul(2654435761) % 100_000;
    let b = i % 4093;
    let _ = writeln!(s, "var{a} = {m} + val{b} ;");
    i = i.wrapping_add(1);
  }
  s
}

/// Exactly eight single-character tokens (four idents, four puncts), no trivia:
/// the window a failed `sync_through` skips-and-rewinds each call.
fn window_source() -> &'static str {
  "a+b+c+d+"
}

// ── Harness ───────────────────────────────────────────────────────────────────

fn bench(c: &mut Criterion) {
  let src = synthetic_source();
  let window = window_source();

  // Deterministic op counts, for the report's per-op arithmetic. The per-token
  // benches each make exactly `total_tokens` real consumes; `failed_sync` does
  // `SYNC_COUNT` syncs over an 8-token window.
  let total_tokens = Parser::new()
    .apply(plain_next_drain_reference)
    .parse_str(src.as_str())
    .unwrap();
  eprintln!(
    "[backtrack] source_bytes={} tokens={} window_bytes={} window_tokens=8 sync_count={}",
    src.len(),
    total_tokens,
    window.len(),
    SYNC_COUNT,
  );

  let mut group = c.benchmark_group("input/backtrack");
  group.measurement_time(Duration::from_secs(3));
  group.warm_up_time(Duration::from_secs(1));
  support::gate_overrides(&mut group);

  // Per-token benches: one element == one real token of progress, so criterion's
  // per-element throughput is directly the per-op cost.
  group.throughput(Throughput::Elements(total_tokens as u64));

  group.bench_function("plain_next_drain_reference", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(plain_next_drain_reference)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("save_commit_per_token", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(save_commit_per_token)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("save_restore_per_token", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(save_restore_per_token)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("txn_begin_commit_per_token", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(txn_begin_commit_per_token)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("txn_begin_drop_rollback_per_token", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(txn_begin_drop_rollback_per_token)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("stacked_savepoint_cycle", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(stacked_savepoint_cycle)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  group.bench_function("attempt_decline_per_token", |b| {
    b.iter(|| {
      let n = Parser::new()
        .apply(attempt_decline_per_token)
        .parse_str(black_box(src.as_str()))
        .unwrap();
      black_box(n)
    })
  });

  // Failed-sync bench: one element == one failed sync (each scans 8 tokens). A
  // collecting `Verbose` emitter records the skipped-token diagnostics so the
  // no-trace rewind actually has a trace to unwind (and is not elided).
  group.throughput(Throughput::Elements(SYNC_COUNT as u64));
  group.bench_function("failed_sync_through_over_8", |b| {
    b.iter(|| {
      // Explicit context type: `ParserContext`'s cache defaults to `DefaultCache`,
      // but `::new` leaves it a free inference variable, so pin it here.
      let ctx: ParserContext<'_, BenchLexer<'_>, Verbose<BenchError>> =
        ParserContext::new(Verbose::new());
      let n = Parser::with_context(ctx)
        .apply(failed_sync_through_over_8)
        .parse_str(black_box(window))
        .unwrap();
      black_box(n)
    })
  });

  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
