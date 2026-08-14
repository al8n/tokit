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

# Deliberately silent about `detect_stack_use_after_return`, so the main `address` leg runs on
# whatever compiler-rt's default for the runner is — which is what a person typing
# `RUSTFLAGS=-Zsanitizer=address cargo test` gets. The two legs at the bottom of this script pin
# both settings explicitly; this one is the unspecified case, and it is not assumed to mean off.
BASE_ASAN_OPTIONS="detect_odr_violation=0 detect_leaks=0"
export ASAN_OPTIONS="$BASE_ASAN_OPTIONS"

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
# Note what `ASAN_OPTIONS` above does NOT say: `detect_stack_use_after_return`. That is not the
# same as saying it is off. `compiler-rt/lib/asan/asan_flags.inc` gives the flag the default
# `SANITIZER_LINUX && !SANITIZER_ANDROID`, so on this script's own default target the fake stack is
# **on** and the `address` leg relocates frames. ThreadSanitizer never relocates one at all. The
# refusal in both legs rests on the compiler's `sanitize=` report either way, which is why it must
# not be made to depend on the frame observation.
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
fail() {
  echo "::error::sanitizer: $1"
  exit 1
}

# The test that announces which arm the frame-reuse control took, and the line it announces on.
ARM_TEST=refusals::the_frame_reuse_detector_agrees_with_this_build

# ## The fake stack is a RUNTIME choice, so both of its settings are run
#
# `frames_reused()` is the one detector that observes the machine instead of asking the build about
# it, and it can only observe anything when ASan's fake stack is on. The control that consumes it
# has an arm for each case, and the arm for a non-relocating sanitizer deliberately ignores the
# observation — correct, and unfalsifiable on its own, because a run that lands there passes
# whatever `frames_reused()` said.
#
# That is not hypothetical. While an unspecified `detect_stack_use_after_return` was read as *off*,
# every `address` cell here took the ignoring arm on a runner whose frames were in fact being
# relocated, and went green without once putting the detector to the question. A single leg cannot
# show this; two can. Each forces the flag, each reports the arm it reached, and they are required
# to be DIFFERENT arms — a pair that lands in the same one is the same blind spot with more steps.
# Sets ARM to the arm the control announced. Not written to stdout and captured: `fail` has to be
# able to end the script and be read while doing it, and neither survives a command substitution.
ARM=""
asan_arm() {
  value="$1"
  log="$2"
  echo "--- detect_stack_use_after_return=${value} ---"
  ASAN_OPTIONS="${BASE_ASAN_OPTIONS} detect_stack_use_after_return=${value}" \
    RUSTFLAGS="-Z sanitizer=address" \
    cargo test --workspace --target "$TARGET" --all-features --test stack_per_level \
    -- --exact "$ARM_TEST" --nocapture >"$log" 2>&1 \
    || { cat "$log"; fail "detect_stack_use_after_return=${value}: the arm control did not pass"; }
  cat "$log"
  # Required to have RUN, not merely to have not failed: the file cfg's itself out entirely at a
  # feature point without a lexer, and a binary that ran no tests prints no banner either.
  grep -q "test result: ok. 1 passed" "$log" \
    || fail "detect_stack_use_after_return=${value}: ${ARM_TEST} did not run"
  ARM="$(sed -n 's/^stack_per_level: CONTROL ARM \([a-z][a-z]*\)$/\1/p' "$log" | tail -1)"
}

both_fake_stack_settings() {
  echo
  echo "=== the fake stack, forced off and forced on: two legs, two arms ==="
  log_off="${TMPDIR:-/tmp}/tokora-asan-arm-off.$$.log"
  log_on="${TMPDIR:-/tmp}/tokora-asan-arm-on.$$.log"
  trap 'rm -f "$log_off" "$log_on"' EXIT

  asan_arm 0 "$log_off"
  arm_off="$ARM"
  asan_arm 1 "$log_on"
  arm_on="$ARM"

  echo
  echo "detect_stack_use_after_return=0 -> arm ${arm_off:-<none>}"
  echo "detect_stack_use_after_return=1 -> arm ${arm_on:-<none>}"

  # Different first, then named, and in that order because each diagnosis has to have an input
  # that produces it. Difference is what a leg pair is for, and it is what the two ways of getting
  # the flag wrong both destroy — a lookup that cannot turn the fake stack on puts both legs in
  # `declared`, one that cannot turn it off puts both in `relocating`. The names then catch the
  # pair that differs and is still not this pair, which is what a broken declaration produces:
  # `native` against `relocating`.
  [ "$arm_off" != "$arm_on" ] || fail \
    "both fake-stack settings landed in the '${arm_on:-<none>}' arm, so the flag did not decide \
anything and one leg exercised nothing the other did not"
  [ "$arm_off" = declared ] || fail \
    "with the fake stack off the control must take the 'declared' arm — the ASan build that does \
not relocate, where the refusal has to stand on the compiler's report alone — and it took \
'${arm_off:-<none>}'"
  [ "$arm_on" = relocating ] || fail \
    "with the fake stack on the control must take the 'relocating' arm, where the detector is \
required to fire — and it took '${arm_on:-<none>}'"

  echo "the two legs took different arms: ${arm_off} and ${arm_on}."
}

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
  if [ "$san" = address ]; then
    both_fake_stack_settings
  fi
done
