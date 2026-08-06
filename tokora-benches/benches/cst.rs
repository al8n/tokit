//! Materialization benchmarks: the wall-clock half of the replay law.
//!
//! The in-suite instrument (`W`, on `cst::sink::finish`) counts replay work in a shape that
//! exists identically before and after a rewrite of the walk. This target is its wall-clock
//! twin, and the two are meant to be read together: `W` says how much work is done, this
//! says what that work costs.
//!
//! Benches (group `cst`):
//!   * `finish_error_dense` — 4 096 alternating (1-byte lexer error, 1-byte token) pairs over
//!     an 8 192-byte source. Every gap is explained by its own diagnostic, so the tree
//!     materializes; this is the shape a from-zero coverage rescan makes quadratic, and the
//!     one the round has to improve.
//!   * `finish_clean` — the same size, tokens only. The no-regression control for ordinary
//!     materialization: nothing about the error channel is exercised.
//!   * `finish_wrap_heavy` — 2 048 retro-wrap targets. The control for how a target's wrap
//!     list is recovered.
//!
//! # The measured region excludes construction
//!
//! Each id is `iter_batched(setup, routine, LargeInput)`: **setup** runs a whole lossless parse
//! over the source and hands back the spent `Cst`, **routine** does only `finish`. Criterion
//! does not time setup, so event construction — which is O(n) and would dilute the ratio — is
//! out of the comparison entirely.
//!
//! # The streams are parsed, not hand-built
//!
//! A sink is minted only by `parse_lossless`, and a token event is born only from a settle, so
//! each shape below is produced by a real drain over a source chosen to make it: `!a!a…` for
//! the error-dense one (`!` is a lexer error, `a` a token), `aaa…` for the clean one. That is
//! the same event stream the old hand-built setup pushed, with the reservation, mapping and
//! validation the real path pays.
//!
//! # Do not add `--all-features` to this
//!
//! `--all-features` turns on `trace`, which is roughly an order of magnitude slower and measures
//! a configuration nobody ships. Nothing needs to be named to avoid it: `tokora-benches` pins the
//! feature point on its `tokora` dependency — `std`, `logos`, `rowan`, `unstable-raw`,
//! `combinators`, and deliberately not `trace` — so the plain form is the correct one:
//!
//! ```text
//! cargo bench -p tokora-benches --bench cst
//! ```

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use tokora::{
  InputRef, Lexer, SimpleSpan, Token,
  cache::DefaultCache,
  cst::{Cst, CstProfile, KindValidator, parse_lossless},
  emitter::Verbose,
};

// ── A byte-per-token lossless lexer, the minimum a Sink will accept ────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BTok(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BErr;

impl Token<'_> for BTok {
  type Kind = u8;
  type Error = BErr;

  // Honest: one byte per token, never skipped.
  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> u8 {
    self.0
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

struct BLexer<'inp> {
  src: &'inp str,
  start: usize,
  pos: usize,
}

impl<'inp> Lexer<'inp> for BLexer<'inp> {
  type State = ();
  type Source = str;
  type Token = BTok;
  type Span = SimpleSpan;
  type Offset = usize;

  fn new(src: &'inp str) -> Self {
    Self {
      src,
      start: 0,
      pos: 0,
    }
  }

  fn with_state(src: &'inp str, _: ()) -> Self {
    Self::new(src)
  }

  fn check(&self) -> Result<(), BErr> {
    Ok(())
  }

  fn state(&self) -> &() {
    &()
  }

  fn state_mut(&mut self) -> &mut () {
    unreachable!("the bench never mutates a unit state")
  }

  fn into_state(self) {}

  fn source(&self) -> &'inp str {
    self.src
  }

  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.start, self.pos)
  }

  fn slice(&self) -> &'inp str {
    &self.src[self.start..self.pos]
  }

  fn lex(&mut self) -> Option<Result<BTok, BErr>> {
    let byte = *self.src.as_bytes().get(self.pos)?;
    self.start = self.pos;
    self.pos += 1;
    // `!` is the error-dense shape's refused byte: a recorded lexer error whose span licenses
    // exactly one gap tile at materialization.
    if byte == b'!' {
      Some(Err(BErr))
    } else {
      Some(Ok(BTok(byte)))
    }
  }

  fn bump(&mut self, n: &usize) {
    self.pos += *n;
    self.start = self.pos;
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BenchErr {
  Lex,
}

impl From<BErr> for BenchErr {
  fn from(_: BErr) -> Self {
    Self::Lex
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized>
  From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>> for BenchErr
{
  fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    Self::Lex
  }
}

type BCst<'inp> = Cst<'inp, BLexer<'inp>, Verbose<BenchErr>>;

const K_ROOT: u16 = 1;
const K_NODE: u16 = 2;
const K_WRAP: u16 = 4;
const K_TOK: u16 = 10;
const K_ERR: u16 = 90;
const K_GAP: u16 = 91;

fn map_tok(_: &BTok) -> u16 {
  K_TOK
}

/// The bench dialect's whole kind space — a real range predicate, so the walls the benchmark
/// times are the ones a dialect actually pays for.
fn in_bench_kind_space(kind: u16) -> bool {
  matches!(kind, K_ROOT | K_NODE | K_WRAP | K_TOK | K_ERR | K_GAP)
}

fn profile() -> CstProfile<BTok> {
  CstProfile::new(
    map_tok,
    KindValidator::new(in_bench_kind_space),
    K_ERR,
    K_GAP,
  )
}

/// The pinned context of a lossless parse over this bench's lexer.
type BIr<'inp, 'c> = InputRef<
  'inp,
  'c,
  BLexer<'inp>,
  (
    tokora::cst::Sink<'inp, BLexer<'inp>, Verbose<BenchErr>>,
    DefaultCache<'inp, BLexer<'inp>>,
  ),
>;

/// Drains every token; each settle records a `Token` event through the auto-emission hook.
fn drain(inp: &mut BIr<'_, '_>) -> Result<(), BenchErr> {
  while inp.next()?.is_some() {}
  Ok(())
}

/// Drains every token, retro-wrapping each one in its own `K_WRAP` node.
fn drain_wrapping(inp: &mut BIr<'_, '_>) -> Result<(), BenchErr> {
  loop {
    let mark = inp.cst_mark();
    if inp.next()?.is_none() {
      return Ok(());
    }
    inp.cst_start_at(mark, K_WRAP);
    inp.cst_finish(K_WRAP);
  }
}

fn run(src: &str, f: fn(&mut BIr<'_, '_>) -> Result<(), BenchErr>) -> BCst<'_> {
  let (cst, parsed) = parse_lossless(
    src,
    (),
    Verbose::<BenchErr>::new(),
    profile(),
    DefaultCache::<BLexer<'_>>::default(),
    f,
  );
  parsed.expect("Verbose collects rather than propagates");
  cst
}

/// `n` alternating (1-byte lexer error, 1-byte token) pairs: `2n` events, `n` gap tiles.
fn error_dense_events(src: &str) -> BCst<'_> {
  run(src, drain)
}

/// `2n` committed tokens over a `2n`-byte source: no diagnostics, no gaps.
fn clean_events(src: &str) -> BCst<'_> {
  run(src, drain)
}

/// `m` retro-wrap targets, one wrap each, over an `m`-byte source.
fn wrap_heavy_events(src: &str) -> BCst<'_> {
  let _ = K_NODE;
  run(src, drain_wrapping)
}

fn bench(c: &mut Criterion) {
  const N: usize = 4_096;
  const M: usize = 2_048;

  let error_dense_src = "!a".repeat(N);
  let clean_src = "a".repeat(2 * N);
  let wrap_src = "a".repeat(M);

  let mut group = c.benchmark_group("cst");

  group.throughput(Throughput::Elements(2 * N as u64));
  group.bench_function("finish_error_dense", |b| {
    b.iter_batched(
      || error_dense_events(&error_dense_src),
      |s| {
        let (green, _emitter) = s.finish(K_ROOT);
        green.expect("every gap is explained by its own diagnostic")
      },
      BatchSize::LargeInput,
    );
  });

  group.bench_function("finish_clean", |b| {
    b.iter_batched(
      || clean_events(&clean_src),
      |s| {
        let (green, _emitter) = s.finish(K_ROOT);
        green.expect("a fully covered source materializes")
      },
      BatchSize::LargeInput,
    );
  });

  group.throughput(Throughput::Elements(4 * M as u64));
  group.bench_function("finish_wrap_heavy", |b| {
    b.iter_batched(
      || wrap_heavy_events(&wrap_src),
      |s| {
        let (green, _emitter) = s.finish(K_ROOT);
        green.expect("well-formed single-wrap targets materialize")
      },
      BatchSize::LargeInput,
    );
  });

  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
