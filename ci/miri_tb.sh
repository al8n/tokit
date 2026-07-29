#!/bin/bash
set -e

# Check if TARGET and CONFIG_FLAGS are provided, otherwise panic
if [ -z "$1" ]; then
  echo "Error: TARGET is not provided"
  exit 1
fi

if [ -z "$2" ]; then
  echo "Error: CONFIG_FLAGS are not provided"
  exit 1
fi

TARGET=$1
CONFIG_FLAGS=$2

# Install cross-compilation toolchain on Linux
if [ "$(uname)" = "Linux" ]; then
  sudo apt-get update
  case "$TARGET" in
    aarch64-unknown-linux-gnu)
      sudo apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu
      ;;
    i686-unknown-linux-gnu)
      sudo apt-get install -y --no-install-recommends gcc-multilib
      ;;
    powerpc64-unknown-linux-gnu)
      sudo apt-get install -y --no-install-recommends gcc-powerpc64-linux-gnu
      ;;
    s390x-unknown-linux-gnu)
      sudo apt-get install -y --no-install-recommends gcc-s390x-linux-gnu
      ;;
    riscv64gc-unknown-linux-gnu)
      sudo apt-get install -y --no-install-recommends gcc-riscv64-linux-gnu
      ;;
  esac
fi

rustup toolchain install nightly --component miri
rustup override set nightly
cargo miri setup

export MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-disable-isolation -Zmiri-symbolic-alignment-check -Zmiri-tree-borrows"
export RUSTFLAGS="--cfg test_$CONFIG_FLAGS"

cargo miri test --tests --target $TARGET --lib --features logos
cargo miri test --target $TARGET --doc --features logos

# A second config covering `unstable-raw`.
#
# The existing run passes no `--no-default-features`, so `std` is already on and the only
# surface miri never saw is the raw/unstable twins — the ones reordered around capture windows, which is precisely the unsafe-adjacent shape miri exists
# to see.
#
# Scope is `--lib` ONLY, deliberately and with the reason recorded rather than left to be
# rediscovered: the tree carries **94** integration targets, and interpreting them a second time
# under miri is hours, not minutes. The raw twins are unit-level, so `--lib` is where their
# coverage actually is; a doubled full run would buy almost nothing for most of the job's wall
# clock. If the raw surface ever grows integration-level tests, this line is where that decision
# gets revisited.
#
# Honest justification, in the same register the rest of this file uses: this would have caught
# nothing in this campaign. No finding in 52 was UB. Miri is kept because it is the only gate
# that can see the class at all, and widened because the already-paid spend should cover the
# surfaces the campaign actually moved.
cargo miri test --tests --target $TARGET --lib --features logos,unstable-raw

