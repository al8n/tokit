#!/bin/bash
#
# Sanitizer legs. Nightly only (`-Z sanitizer`, `-Zbuild-std`).
#
# This script existed, worked, and had **no caller** — no job in `.github/` referenced it, so it
# had never run. That is the same class as a configuration CI compiles and never executes, one
# level up: not a gate that covers nothing, a gate with nothing invoking it. It is wired now.
#
# ## The target is an argument, not a constant
#
# It used to be hard-coded to `x86_64-unknown-linux-gnu`, which is right for the CI runner and
# wrong everywhere else. Which sanitizers exist is a property of the target, so the target
# belongs where the matrix can set it:
#
#     rustc +nightly --print target-spec-json -Z unstable-options --target <t> | grep supported-sanitizers
#
# ## `-Zbuild-std` is not optional for thread/memory, and dropping it looks like a finding
#
# The shipped `std` is uninstrumented. ThreadSanitizer and MemorySanitizer need an instrumented
# one, so those legs rebuild it. Without the flag the build fails with an ABI/interceptor
# mismatch that **reads like a sanitizer report** — it is the shape that gets misdiagnosed as a
# real defect, so it is written down here rather than rediscovered.
#
# ## Usage
#
#   ci/sanitizer.sh                                  # address+thread on the CI target
#   ci/sanitizer.sh aarch64-apple-darwin address     # one leg, one target
set -eu

TARGET="${1:-x86_64-unknown-linux-gnu}"
# `address` and `thread` are the two supported on every target this project builds for. `leak`
# and `memory` are opt-in because they are unsupported on some hosts, and an unsupported leg
# aborting the run would report as a failure rather than as the platform fact it is.
SANITIZERS="${2:-address thread}"

export ASAN_OPTIONS="detect_odr_violation=0 detect_leaks=0"

# ## Telling the tests they are instrumented
#
# One test — `pratt_limit_unit_sink`'s unwind cell — corroborates the library's recursion-depth
# accounting against the *machine*, by comparing the addresses of two stack locals. That
# comparison is only a measurement while the addresses are native stack offsets, and under a
# sanitizer they are not: ASan can relocate a frame's locals onto a heap-allocated fake stack, so
# the recoverer measured DEEPER than the descent it had already unwound past. Inverted operands,
# not a tight margin — no threshold rescues it.
#
# There is no stable `cfg` for "am I instrumented": `cfg(sanitize = "address")` needs
# `feature(cfg_sanitize)`, and an integration test compiled on stable cannot carry a feature gate.
# So the leg announces itself in the environment and the test reads it at run time. Exported
# INSIDE the loop, and with `${san}` rather than a constant, so it covers every sanitizer this
# script runs and names which one in the skip line the test prints.
#
# It gates exactly one assertion. The depth-cell assertions stay live under every sanitizer, and
# the skipped one is announced loudly on stderr rather than quietly passing.
for san in $SANITIZERS; do
  echo "=== sanitizer: ${san} on ${TARGET} ==="
  export TOKORA_SANITIZER="${san}"
  case "$san" in
    memory | thread)
      # instrumented std — see the note above before removing `-Zbuild-std`
      RUSTFLAGS="-Z sanitizer=${san}" \
        cargo -Zbuild-std test --tests --target "$TARGET" --all-features
      ;;
    *)
      RUSTFLAGS="-Z sanitizer=${san}" \
        cargo test --tests --target "$TARGET" --all-features
      ;;
  esac
done
