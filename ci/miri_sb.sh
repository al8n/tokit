#!/bin/bash
set -euo pipefail

# Miri under Stacked Borrows, ONE SHARD of the suite.
#
# This script and `ci/miri_tb.sh` are deliberate near-duplicates and must stay that way: they
# differ in `MIRIFLAGS` and in nothing else that matters. Everything about WHICH tests a shard
# runs — the enumeration, the partition, the guard, the pinning of the units that cannot be
# split — lives in `ci/miri_shard.py`, so the two scripts cannot drift apart on the part where
# drifting is dangerous. Read that file's header before changing anything here.
#
# Usage: miri_sb.sh <TARGET> <CONFIG_FLAGS> <SHARD>

if [ -z "${1:-}" ]; then
  echo "Error: TARGET is not provided" >&2
  exit 1
fi

if [ -z "${2:-}" ]; then
  echo "Error: CONFIG_FLAGS are not provided" >&2
  exit 1
fi

if [ -z "${3:-}" ]; then
  echo "Error: SHARD is not provided" >&2
  exit 1
fi

TARGET=$1
CONFIG_FLAGS=$2
SHARD=$3

SHARD_PY="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/miri_shard.py"

# ── The shard plan, computed FIRST ──────────────────────────────────────────────────────────
#
# Before `rustup`, before `cargo miri setup`, before anything that costs minutes. A plan that
# cannot be computed is a red job either way; the only question is whether it costs seconds or
# whether it costs the whole toolchain setup first.
#
# `set -e` aborts on a non-zero helper, so a failed plan cannot reach a `cargo` line. The
# `case` statements below check the other half — that what came back is a SELECTOR. An empty
# selector is not "run nothing" to `cargo test`, it is "run everything": `cargo miri test
# --target T --features logos` with no `--test` flag interprets all 102 targets. A plan bug
# that produced an empty slice would therefore not fail and would not be fast; it would
# quietly run the entire suite on every shard.
LOGOS_FLAGS=$(python3 "$SHARD_PY" flags "$SHARD" logos)
RAW_FLAGS=$(python3 "$SHARD_PY" flags "$SHARD" raw)
DOC_FLAGS=$(python3 "$SHARD_PY" flags "$SHARD" doc)

case "$LOGOS_FLAGS" in
  --*) ;;
  *)
    echo "Error: shard plan returned no selector for the logos pass: '$LOGOS_FLAGS'" >&2
    exit 1
    ;;
esac

case "$RAW_FLAGS" in
  --*) ;;
  *)
    echo "Error: shard plan returned no selector for the unstable-raw pass: '$RAW_FLAGS'" >&2
    exit 1
    ;;
esac

# Install cross-compilation toolchain on Linux. The `apt-get update` sits inside the arms that
# need it rather than above the `case`: no target the Miri matrix actually uses needs a
# cross-gcc, and an unconditional index refresh is a fixed cost that sharding now pays once
# per shard instead of once per cell.
if [ "$(uname)" = "Linux" ]; then
  case "$TARGET" in
    aarch64-unknown-linux-gnu)
      sudo apt-get update && sudo apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu
      ;;
    i686-unknown-linux-gnu)
      sudo apt-get update && sudo apt-get install -y --no-install-recommends gcc-multilib
      ;;
    powerpc64-unknown-linux-gnu)
      sudo apt-get update && sudo apt-get install -y --no-install-recommends gcc-powerpc64-linux-gnu
      ;;
    s390x-unknown-linux-gnu)
      sudo apt-get update && sudo apt-get install -y --no-install-recommends gcc-s390x-linux-gnu
      ;;
    riscv64gc-unknown-linux-gnu)
      sudo apt-get update && sudo apt-get install -y --no-install-recommends gcc-riscv64-linux-gnu
      ;;
  esac
fi

rustup toolchain install nightly --component miri
rustup override set nightly
cargo miri setup

# ── WHAT THE TWO FEATURE SETS BELOW DO NOT COVER: `rowan` ───────────────────────────────────
#
# The passes below run `--features logos` and `--features logos,unstable-raw`. `rowan` is in
# neither and is not in `default`, so no Miri job in this repository has ever COMPILED the
# lossless CST materialization path — the one 0.9.0 ships as its lossless story. Measured on
# `543ce6d`: 66 mentions of `feature = "rowan"` across 8 files under `tokora/src`, of which 41
# are `#[cfg]` gates whose code exists only when the feature is on. Every target, both models:
# that is ZERO coverage of that path, not reduced coverage.
#
# ADDING `rowan` HERE DOES NOT CLOSE THE GAP — IT TURNS THESE CELLS RED. `rowan 0.16.1`, the
# version `Cargo.lock` resolves, executes undefined behaviour on its ordinary public path, and
# the two aliasing models find it in two DIFFERENT places. Neither model is clean, so there is
# no one-model half-measure:
#
#   * Stacked Borrows — THIS model — `src/arc.rs:260`, `<HeaderSlice<H, [T; 0]> as Deref>::deref`
#     forging a `&HeaderSlice<H, [T]>` over the whole slice out of a `&self` retagged for the
#     header only. Reached through `tokora::cst::sink::finish::replay` → `materialize` →
#     `Cst::finish`, i.e. from building any tree at all.
#   * Tree Borrows — `src/cursor.rs:219`, `rowan::cursor::free` dropping a `Box<NodeData>`
#     through a tag an ancestor's `Cell` still holds. Reached from dropping any red-tree
#     `SyntaxNode`.
#
# Reproduced 2026-08-07 on `543ce6d`, each model differing from the line below only in
# `MIRIFLAGS`:
#
#     cargo miri test --target aarch64-apple-darwin --test parser_node --features logos,rowan
#
# Both models red on the FIRST of that target's 24 tests.
#
# The defect is rowan's, not tokora's: `tokora/src/cst` contains no `unsafe` at all. Upstream has
# had it reported since 2021 — rust-analyzer/rowan#108, #163, #192, all three still open — and
# the only fix attempt, PR #211, is a draft whose own description says the immutable path still
# fails. Bumping is not a known answer either: the `arc.rs` construct above is present unchanged
# in `0.17.0`, the newest release, and al8n/smear#77 reproduced both reds against it standalone.
#
# WHAT WOULD FLIP THIS ANSWER. A published rowan whose `arc.rs` `Deref` and whose `cursor.rs`
# `free` are both clean under `-Zmiri-strict-provenance` and under `-Zmiri-tree-borrows`. Then
# add `rowan` to the feature sets below and to `ci/miri_tb.sh`'s — and extend
# `ci/miri_shard.py`'s enumeration if any `rowan`-gated test TARGET arrives with it — rather
# than leaving this note to rot.
#
# Recorded for 0.9.0 by al8n/tokit#235. The same answer is written where the other re-enablers
# arrive from: `ci/miri_tb.sh`, `.github/workflows/miri.yml`, `tokora/Cargo.toml`'s `rowan`
# feature, and the changelog's known-limitation entry.

export MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-disable-isolation -Zmiri-symbolic-alignment-check"
export RUSTFLAGS="--cfg test_$CONFIG_FLAGS"

# `$LOGOS_FLAGS` / `$RAW_FLAGS` are unquoted ON PURPOSE: each is a flag LIST — an optional
# `--lib` followed by `--test <name>` pairs — and word splitting is how it becomes many
# arguments. `ci/miri_shard.py` asserts every name matches `[A-Za-z0-9_-]+` before printing
# them, which is what makes the unquoted expansion safe rather than an injection.
#
# `--lib` appears on exactly one shard per pass, and the two passes pin it to different shards,
# so the two lib runs overlap instead of queueing behind each other.
#
# The old form was `--lib --tests`, which selected the same 102 targets `--tests` selects on its
# own — verified with `cargo test --no-run --message-format=json`, the two sets are equal and
# `--lib` alone is the single lib target. It is gone here because a shard names its targets
# explicitly; there is no longer a selector that could be redundant with another.
# shellcheck disable=SC2086
cargo miri test --target "$TARGET" $LOGOS_FLAGS --features logos

# The doctests are one indivisible unit, pinned to one shard. `SKIP` is a literal sentinel
# rather than an empty string: a shell cannot tell an empty variable from a helper that died,
# and one of those two must never be read as "there were no doctests to run".
case "$DOC_FLAGS" in
  --doc)
    cargo miri test --target "$TARGET" --doc --features logos
    ;;
  SKIP)
    echo "shard $SHARD does not own the doctests; skipping"
    ;;
  *)
    echo "Error: unexpected doctest verdict from the shard plan: '$DOC_FLAGS'" >&2
    exit 1
    ;;
esac

# A second config covering `unstable-raw` — see the note in `miri_tb.sh` for the scope
# reasoning and for what this pass was measured to actually cover. In short: it was written
# `--lib --tests` under a comment claiming "`--lib` only", and `--tests` won, so it has always
# re-interpreted the whole integration suite. The sharded form preserves that exactly, each
# shard running its own slice under `logos,unstable-raw` as well.
# shellcheck disable=SC2086
cargo miri test --target "$TARGET" $RAW_FLAGS --features logos,unstable-raw
