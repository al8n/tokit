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

# ## Refusal mode, stated rather than inherited
#
# `stack_per_level` prints a bytes-per-nesting-level figure only when `TOKORA_STACK_PROBE=native`
# affirms an uninstrumented build, and `ci/stack_probe.sh` is the job that supplies that
# affirmation. This script is that job's mirror: every leg here IS instrumented, so the affirmation
# must not be in the environment.
#
# It is unset rather than merely left alone because this script is run by hand as well as by CI,
# and a shell that already exports it would otherwise carry it into an ASan process. The
# affirmation cannot override a detection — the probe returns the sanitizer as its reason, not the
# variable — so this is the belt to that brace, and it keeps the skip line naming the sanitizer.
#
# Note what `ASAN_OPTIONS` above does NOT say: `detect_stack_use_after_return`. The fake stack is
# off, so under the `address` leg frames are NOT relocated and `frames_reused()` correctly reports
# that they are reused. ThreadSanitizer never relocates a frame at all. The refusal in both legs
# rests on the compiler's `sanitize=` report, which is why it must not be made to depend on the
# frame observation.
unset TOKORA_STACK_PROBE

# ## Telling the tests they are instrumented — no longer this variable's job alone
#
# Two tests compare the addresses of stack locals: `pratt_limit_unit_sink`'s unwind cell, which
# corroborates the library's recursion-depth accounting against the *machine*, and
# `stack_per_level`, which reports bytes of stack per nesting level. Both readings hold only while
# the addresses are native stack offsets, and under a sanitizer they are not — ASan can relocate a
# frame's locals onto a heap-allocated fake stack, so the recoverer measures DEEPER than the
# descent it had already unwound past. Inverted operands, not a tight margin; no threshold rescues
# it.
#
# This export used to be the ONLY signal, and that was wrong in the direction that matters:
# nothing but this script sets it, so `RUSTFLAGS=-Zsanitizer=address cargo test` — the way anyone
# would actually reach for a sanitizer — left it unset and both tests took the native-stack path
# inside a genuinely instrumented process. Watched: `stack_per_level` printed
# `combinator 4096 B, separated 6144 B` (ASan fake-stack size classes) as frame sizes, and
# `pratt_limit_unit_sink` FAILED on the inverted operands.
#
# `tokora/build.rs` now asks the compiler — `rustc --print cfg` with this build's rustflags — and
# records `sanitize=`, which is the fact `cfg(sanitize = "address")` would give if a
# stable-compiled integration test could carry `feature(cfg_sanitize)`. On top of that,
# `tests/common/native_stack.rs` observes directly whether sequential calls reuse a frame, and
# refuses to print a figure at all unless `TOKORA_STACK_PROBE=native` affirms an uninstrumented
# build.
#
# So this stays as one signal among several — the one that survives a route `build.rs` cannot see,
# such as a flag injected by a `RUSTC_WRAPPER`. Exported INSIDE the loop, and with `${san}` rather
# than a constant, so it covers every sanitizer this script runs and names which one in the skip
# line the test prints.
#
# It gates the two address comparisons and nothing else. The depth-cell assertions stay live under
# every sanitizer, and a skipped one is announced loudly on stderr rather than quietly passing.
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
