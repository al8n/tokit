//! One knob the wall-clock regression gate turns, shared by all five bench binaries.
//!
//! # Why this exists at all
//!
//! Three of the five files in this directory pin `measurement_time(3s)` and `warm_up_time(1s)`
//! **on the group**, and a group's setting overrides whatever `configure_from_args` read off the
//! command line — so `--measurement-time` is inert in `input_scan`, `parser_combinators` and
//! `backtrack`, and live in `cst` and `pratt_typed`. `pratt_typed`'s module header already
//! records that asymmetry and calls it out as a cost: un-setting a value compiled
//! into the file is not the one-flag operation that setting one from the command line is.
//!
//! `ci/wallclock/run.sh` needs every one of the five to answer to the *same* knob, because it
//! runs the whole population twice per round and its cost is the sum. Reading the pinned files
//! through the command line and the unpinned ones through nothing would leave the gate measuring
//! three targets at one window and two at another, and a per-target window that the gate cannot
//! state is a per-target cost it cannot budget.
//!
//! # What it does and does not change
//!
//! Nothing, unless the environment says otherwise. With no `TOKORA_BENCH_*` variable set,
//! `gate_overrides` returns without touching the group, so `cargo bench` locally, `bench (smoke)`
//! in CI and every recorded figure in these files' headers keep the windows they were taken at.
//! The three pinned files keep their pins; the two unpinned files keep criterion's defaults and
//! keep their command-line knobs live.
//!
//! With a variable set, the override is applied **after** the file's own call, so it wins over a
//! pin and over criterion's default alike. That is the whole point: one setting, five binaries,
//! forty-six ids, one window.
//!
//! # A malformed value is a panic, not a shrug
//!
//! `TOKORA_BENCH_MEASUREMENT_MS=1O0` — capital O — parses as nothing. Ignoring it would leave the
//! gate running its 46 ids at a window it did not ask for, reporting a spread it would then treat
//! as this runner's floor, and nothing anywhere would say so. So a value that does not parse, or
//! that is zero, aborts the binary. `pratt_typed`'s `TOKORA_PRATT_ITERS` is read the same way and
//! for the same reason.

use core::time::Duration;

use criterion::{BenchmarkGroup, measurement::Measurement};

/// Milliseconds of criterion measurement window per id.
const MEASUREMENT_MS: &str = "TOKORA_BENCH_MEASUREMENT_MS";

/// Milliseconds of criterion warm-up per id.
const WARM_UP_MS: &str = "TOKORA_BENCH_WARM_UP_MS";

/// Criterion samples per id.
///
/// Never pinned in any of the five files, so `--sample-size` already reaches all of them — it is
/// here anyway so the gate sets one window through one mechanism rather than two.
const SAMPLE_SIZE: &str = "TOKORA_BENCH_SAMPLE_SIZE";

/// Apply the wall-clock gate's measurement window to `group`, if the environment asks for one.
///
/// Call it as the LAST configuration on a group, after any `measurement_time` / `warm_up_time`
/// the file pins for itself. Absent the environment variables this is a no-op and the group keeps
/// exactly the configuration it had.
pub fn gate_overrides<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
  if let Some(ms) = millis(MEASUREMENT_MS) {
    group.measurement_time(ms);
  }
  if let Some(ms) = millis(WARM_UP_MS) {
    group.warm_up_time(ms);
  }
  if let Some(raw) = read(SAMPLE_SIZE) {
    // Criterion's own floor. Below it criterion panics with a message that names neither this
    // module nor the variable that caused it, so the refusal is made here where it can.
    let n: usize = parse(SAMPLE_SIZE, &raw);
    assert!(
      n >= 10,
      "{SAMPLE_SIZE}={raw}: criterion requires at least 10 samples per id"
    );
    group.sample_size(n);
  }
}

/// Read one variable, treating an unset or empty value as absent.
///
/// Empty counts as absent because `TOKORA_BENCH_MEASUREMENT_MS=` is what a shell writes when the
/// variable it was expanding from was itself unset, and that is the one malformed value that
/// means "no override" rather than "a typo".
fn read(name: &str) -> Option<String> {
  match std::env::var(name) {
    Ok(raw) if raw.trim().is_empty() => None,
    Ok(raw) => Some(raw),
    Err(_) => None,
  }
}

fn millis(name: &str) -> Option<Duration> {
  let raw = read(name)?;
  let ms: u64 = parse(name, &raw);
  assert!(
    ms > 0,
    "{name}={raw}: a zero-length window measures nothing"
  );
  Some(Duration::from_millis(ms))
}

fn parse<T: core::str::FromStr>(name: &str, raw: &str) -> T {
  raw.trim().parse().unwrap_or_else(|_| {
    panic!(
      "{name}={raw:?} is not a non-negative integer. It is read by \
       `tokora-benches/benches/support/mod.rs`, which refuses rather than ignores it: a window \
       silently different from the one asked for is a measurement of something else."
    )
  })
}
