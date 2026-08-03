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
