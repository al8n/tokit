#!/bin/bash
#
# The two stack probes, run where they can actually take their reading — and required to have
# taken it.
#
# ## Why this job exists
#
# `tests/stack_per_level.rs` reports bytes of native stack per nesting level, and
# `tests/pratt_limit_unit_sink.rs` corroborates the recursion tracker's frame accounting against
# the machine. Both read the addresses of stack locals, which is a measurement only on a
# contiguous native stack, so both refuse rather than report when the build cannot be shown to
# have one — and `stack_per_level` refuses unless `TOKORA_STACK_PROBE=native` positively affirms
# it, because nothing a stable-compiled integration test can read proves the absence of
# instrumentation.
#
# That refusal is correct and it opened a hole one level up. Nothing in `.github/` set the
# affirmation, so `cargo test` in every job printed a skip line and passed: the fix that stopped
# the probe printing a false number made the true number unreachable in CI. **A skip line in a
# green log is not a measurement.** This script is the one invocation that supplies the
# affirmation, and it fails when the figure does not come back — whether because the affirmation
# went missing, because the build was refused, or because the test did not run at all.
#
# ## What it does NOT do
#
# It asserts no threshold. The figures move with the toolchain, the target and `-Cdebuginfo`, and
# a bound on them would be a flake rather than a contract (`stack_per_level.rs` says the same at
# more length). What is gated is that the reading was taken and printed, so a regression is
# visible in the log and in the job's own output rather than absent from both.
#
# ## Deliberately not run under a sanitizer
#
# The sanitizer matrix runs `ci/sanitizer.sh`, which does the opposite: it leaves the affirmation
# unset so both probes refuse. Under ASan a frame's locals can sit on a heap-backed fake stack and
# every frame carries redzones, so the addresses are not stack offsets and the figure would be
# fiction. Keep the two jobs on opposite sides of that switch.
#
# ## Usage
#
#   ci/stack_probe.sh                      # the default feature point
#   ci/stack_probe.sh --all-features       # extra cargo arguments are passed through
set -euo pipefail

FEATURES=("--features" "logos,std")
[ "$#" -gt 0 ] && FEATURES=("$@")

# The affirmation, and the ONLY signal this script supplies. It deliberately does not clear
# `TOKORA_SANITIZER`: an announcement in this job's environment means something announced a
# sanitizer where a native reading was expected, and the job should red on it rather than strip
# the one detector that survives a route `build.rs` cannot see.
export TOKORA_STACK_PROBE=native

log="${TMPDIR:-/tmp}/tokora-stack-probe.$$.log"
trap 'rm -f "$log"' EXIT

fail() {
  echo "::error::stack probe: $1"
  echo "--- the run's output ---"
  cat "$log"
  exit 1
}

echo "=== stack_per_level (TOKORA_STACK_PROBE=native) ==="
cargo test -p tokora "${FEATURES[@]}" --test stack_per_level -- --nocapture >"$log" 2>&1 \
  || fail "the probe suite did not pass"
cat "$log"

# `NO MEASUREMENT TAKEN` is what the probe prints on the real stderr when it refuses. Checked
# before the positive match so the diagnosis names the refusal rather than the missing line.
if grep -q "NO MEASUREMENT TAKEN" "$log"; then
  fail "the build refused the measurement — read the reason on the line above"
fi
if ! grep -qE "per nesting level: combinator [0-9]+ B, separated [0-9]+ B, hand-written [0-9]+ B" "$log"; then
  fail "no per-nesting-level figure was produced; the reading this job exists to take did not happen"
fi

# The frame-reuse control has three arms; `ci/sanitizer.sh` runs the two instrumented ones, and
# this run is the third. What is gated here is that it RAN and said so. `native` is the only arm
# that asserts frames are reused — the property the figure above was read on — and on a build that
# produced a figure it is the only arm reachable, because every input to the other two also refuses
# the measurement. So this is not a second opinion about the arm; it is the difference between the
# control having run and the control having been filtered out of a green log, which is the same
# thing `--exact` and `1 passed` buy for the witness below. Checked after the refusal and the
# figure so those keep their own diagnoses.
#
# Matched anywhere on the line rather than anchored at its start. The announcement goes straight to
# the real stderr, while libtest's `test <name> ... ` for some *other* test can already have
# reached the same descriptor without its terminating newline — this run has 17 tests and they run
# in parallel. The two then share one physical line, `^` stops matching, and a perfectly good
# `native` run reds as "no arm at all". Watched happening, on the run that planted a skipped
# witness and got this diagnosis instead of its own. The name is written by one `write_str`, so a
# prefix can be displaced onto the line but the word itself is never split.
#
# `|| true` because no match is a *result* here, not an error: `grep` exits 1 on it, and under
# `set -e -o pipefail` that ends the script at this assignment — reporting the failure as a bare
# exit code with the diagnosis below never printed. Watched happening.
arm="$(grep -o 'stack_per_level: CONTROL ARM [a-z][a-z]*' "$log" | tail -1 | sed 's/.* //' || true)"
if [ "$arm" != native ]; then
  fail "the frame-reuse control reported '${arm:-no arm at all}' where it must report 'native'; the figure above was read on the assumption that frames are reused, and that is the arm which asserts it"
fi

echo
echo "=== pratt_limit_unit_sink (the frame-release witness must RUN) ==="
# Named with `--exact`, and the count is required to be 1: the file cfg's itself out entirely at a
# feature point without a lexer, and "the banner did not appear" is satisfied just as well by a
# binary that ran no tests at all.
WITNESS=every_level_and_every_frame_is_released_before_the_trip_surfaces
cargo test -p tokora "${FEATURES[@]}" --test pratt_limit_unit_sink \
  -- --nocapture --exact "$WITNESS" >"$log" 2>&1 \
  || fail "the frame-release witness did not pass"
cat "$log"

if ! grep -q "test result: ok. 1 passed" "$log"; then
  fail "$WITNESS did not run; this feature point does not compile it"
fi
# The witness is skipped on a positive instrumentation finding and announces itself loudly when it
# is. On an ordinary build there is nothing to find, so the banner appearing here means the
# assertion stopped running while the test went on passing.
if grep -q "SKIPPED ASSERTION" "$log"; then
  fail "the frame-release witness was skipped on a build that is not instrumented"
fi

echo
echo "stack probe: both readings were taken."
