#!/usr/bin/env bash
#
# Wall-clock regression gate: this branch's criterion benches against its merge-base's.
#
# ── WHAT THIS IS FOR, AND WHAT IT IS NOT ────────────────────────────────────────────────────
#
# The `icount` job counts instructions. An instruction count is blind to where those
# instructions read from: a change that keeps `Ir` to the byte can still fall off a cache, lose
# a branch predictor, add an allocation, or move data it used not to move, and cost real time
# for it. That is the gap this fills, and it is the ONLY gap it fills.
#
# **This is the coarse layer.** It cannot see what the instruction-count gate sees, and the
# numbers that say so were measured in this repository:
#
#   * `tokora-benches` under criterion showed a run-to-run spread of **4.3-4.8%** on a dedicated
#     machine, and one full-suite run moved **all nine benches in lockstep by +82%** on code
#     that had not changed.
#   * The repetition engine is **a third to a half** of each bench's work — lexing, allocation
#     and the drain dilute it — so a regression *in the engine* reads here at roughly half its
#     own amplitude.
#
# Together those put the smallest engine regression wall clock can distinguish from noise at
# around **9-10% even on a quiet dedicated machine**, against a project that holds itself to
# ±2%. A shared GitHub runner is worse, and the `--self` run below is what says by how much.
#
# So the claim a caller may rest on is: **this gate sees a cache cliff, an allocation added to a
# hot loop, or a data-movement change large enough to be worth a rollback.** It does not see a
# lost inline, and it must never be read as though it did. When both jobs are green the
# instruction-count one is the tighter statement; when they disagree, this one is measuring the
# thing a user feels and the other is measuring the thing a compiler did.
#
# ── THE DECISIONS, AND WHERE THEY COME FROM ─────────────────────────────────────────────────
#
# **Same-run A/B, no committed baseline.** Same decision as `ci/icount/run.sh`, and here the
# argument is strictly stronger. Absolute times do not travel between runners at all — GitHub
# hands out several CPU generations under one label — so a stored baseline would be comparing
# this run's host to whichever host happened to write the file. Both sides are built and
# measured inside one job, on one runner, with one toolchain.
#
# **The instrument is the constant, the library is the variable.** Same trick as the
# instruction-count gate: the base side is NOT a checkout of the merge-base. It is THIS branch's
# tree with `tokora/` replaced by the merge-base's `tokora/` — the same fixtures, the same ids,
# the same measurement window, compiled against the older library. And here it does one more
# job: the branch may have changed a bench, and a bench that changed is a bench whose two sides
# are not measuring the same work.
#
# A consequence worth stating, the same one `ci/icount/run.sh` states: if this branch changes
# `tokora/Cargo.toml` or a public API in a way `tokora-benches` depends on, the base side fails
# to BUILD and this script reds saying so. That is correct — the two revisions cannot be
# compared by this instrument, and a gate that answered anyway would be answering about
# something else.
#
# **Interleaved, and counterbalanced.** The rounds alternate: the two sides of the SAME bench
# target run back to back, and which of them goes first flips every round (base, head, head,
# base, ...). Running all of one side and then all of the other is what makes a noisy neighbour
# that arrives at minute six into a regression — it would land entirely on the second half. And
# strict alternation alone is not enough either: it puts one side first every time, so any
# drift within a pair (a cache the first run warms for the second, a frequency ramp) is a
# constant bias in one direction. Flipping the order cancels its first-order term.
#
# **Minimum, not mean.** Wall-clock noise is one-directional — nothing makes a sample faster
# than the work it did — so the mean of a sample set is the true cost plus an unknown positive
# noise integral while the minimum converges to the true cost from above. `measure.py` takes the
# minimum over criterion's samples within a round; `compare.py` takes the minimum over rounds.
#
# **The threshold is derived, not chosen.** `--self` builds the same commit twice and measures
# both, so the answer is known to be zero and everything it reports is this runner's floor.
# `WALL_THRESHOLD` sits above the MAXIMUM over ids of that floor, not above the typical id: a
# job that fails when any one of forty-six rows crosses the line fails at about forty-six times
# a single row's rate, and a threshold derived from the median row is a threshold that reds
# roughly every other week.
#
# **Advisory.** The `wall clock` job runs with `continue-on-error: true`. What would promote it
# is written in that job's header; the short form is a false-positive count over a run of PRs,
# not an argument.
#
# ── USAGE ───────────────────────────────────────────────────────────────────────────────────
#
#     ci/wallclock/run.sh <base-ref>            # compare HEAD against its merge-base
#     ci/wallclock/run.sh --self                # compare HEAD against ITSELF: the noise floor
#
# The first form BECOMES the second on its own when the branch's `tokora/` tree is the
# merge-base's, because then the two sides are identical source and the run is a floor
# measurement whatever it was asked to be. See where `self` is re-derived below.
#     ci/wallclock/run.sh --self --plant NAME   # ... with `ci/wallclock/plants/NAME.py` applied
#                                               #     to the head side only
#
# `--plant` exists because the two things a gate must be watched doing — firing on a regression
# and staying silent on a change that is not one — cannot be demonstrated by a commit on the
# branch that carries the gate. A plant committed to `tokora/src/` would have to be reverted
# before merge, and a gate whose demonstration was reverted is a gate nothing has watched. So
# the plants are edit scripts under `ci/wallclock/plants/`, applied to the extracted work tree
# and never to the repository, each asserting its own anchor matched exactly once so that it
# fails loudly rather than silently planting nothing.
#
# Environment: WALL_THRESHOLD, WALL_ROUNDS, WALL_WARMUP_MS, WALL_MEASURE_MS, WALL_SAMPLES,
# WALL_WORK — and WALLGATE_PLANT_RATE, which sizes `alloc_per_token` and is how the gate's edge is
# found rather than asserted: raise it until the plant stops firing.

set -euo pipefail

# ── The threshold, and the measurement it came from ─────────────────────────────────────────
#
# DERIVED, on the runner, in this configuration. Re-derive it by dispatching this workflow: a
# dispatch runs `--self`, and so does any pull request whose `tokora/` tree is the merge-base's.
#
# **The self-comparison, 2026-08-29, `ubuntu-latest`, run 33225301618.** Same commit both sides,
# 46 ids, 5 interleaved rounds a side, 200 ms warm-up + 500 ms measurement x 10 samples an id.
#
#   | quantity                                        | value   |
#   |-------------------------------------------------|---------|
#   | median id's difference between the two sides     | 0.36%   |
#   | 90th percentile                                  | 1.35%   |
#   | ids over 2%                                      | 2 of 46 |
#   | ids over 5%                                      | 1 of 46 |
#   | **largest**                                      | **16.22%** |
#   | median id's round-to-round spread, same build     | 1.39%   |
#   | worst id's round-to-round spread, same build      | 21.87%  |
#
# Two things in that table matter more than the number this file sets.
#
# **The measurement is far better than the benches' reputation.** Criterion's own run-to-run
# spread on these targets is 4.3-4.8% on a dedicated machine; interleaved min-of-5 on a SHARED
# runner puts the median id at 0.36% and the 90th percentile at 1.35%, while the per-round spread
# it is built from reaches 21.87%. That gap is the estimator working: the minimum is immune to
# the noise the mean would have integrated.
#
# **And the floor is not the measurement.** `input/backtrack/stacked_savepoint_cycle` came back
# **+16.22%** with the two sides' five rounds each stable to under 1% and their two clusters
# COMPLETELY DISJOINT — base 672-677 us across all five rounds, head 781-788 us across all five.
# That is not noise. Noise overlaps; this does not. Eight further ids carried the same signature
# two orders of magnitude smaller (disjoint clusters at 0.5-1.4%).
#
# **What it is NOT is codegen**, and that took a second run and a new diagnostic to establish.
# Run 33226885912 printed what run 33225301618 could not: on a self-comparison the two sides'
# binaries are **byte-identical**, on all five targets. `--remap-path-prefix` maps two
# equal-length source directories to one string, so rustc emits the same bytes twice, and a
# self-comparison therefore runs ONE PROGRAM against itself. Whatever moved that row 16% was
# per-process placement — where the loader mapped the image, where the allocator's arenas landed,
# how the stack aligned — and not a difference between two compilations.
#
# **And it is intermittent.** The second self-comparison, same configuration, same day, put the
# same id at **-0.15%** (base 662-672 us, head 660-666 us) and the whole population at a median of
# 0.19% with a maximum of 5.01%. So the outlier is not a property of that benchmark; it is a
# property of a process, it appeared in one run of two, and no number of rounds WITHIN a run can
# see past it, because within a run it is stable to under 1%.
#
#   | self-comparison | median | p90   | ids over 5% | max     |
#   |-----------------|--------|-------|-------------|---------|
#   | 33225301618     | 0.36%  | 1.35% | 1 of 46     | 16.22%  |
#   | 33226885912     | 0.19%  | 2.40% | 1 of 46     |  5.01%  |
#
# So the resolution of this gate is set by placement, not by the runner's noise, and the honest
# threshold has to clear it. **25%** is about 1.5x the worst of the two observations. It is
# deliberately NOT the mechanical 2x-the-floor rule, which would give 32.4%: the floor is not one
# distribution but a body at ~0.2-0.4% plus an intermittent placement outlier, and with two
# samples of that outlier the right response is to write down what would move the number — see the
# `wall clock` job's promotion criteria — rather than to inflate it until nothing could reach it.
#
# Two consequences worth stating plainly. This gate cannot support a claim about a single id
# moving 20%, because placement alone produced 16% on a single id with nothing changed. And
# 16.22% is a LOWER bound on what a real comparison faces: a self-comparison runs one identical
# binary twice, while a merge-base comparison runs two different ones, which adds the codegen
# placement this measurement was able to exclude.
: "${WALL_THRESHOLD:=25.0}"

# Five rounds a side. The minimum needs at least one clean round per side to land on, and
# counterbalancing needs an even split of orders; five gives three of one order and two of the
# other, which is the smallest odd count that still leaves a spare of each. Raising it costs
# `WALL_ROUNDS x 2 x (the pass time printed at the end)` linearly and buys a floor that shrinks
# slowly — the minimum of N one-directional-noise samples improves like the tail of the noise
# distribution, not like 1/sqrt(N).
: "${WALL_ROUNDS:=5}"

# The measurement window, per id, applied to all five binaries through the environment rather
# than through criterion's command line. Three of the five pin `measurement_time` and
# `warm_up_time` ON THE GROUP, and a group's setting overrides whatever `configure_from_args`
# read — so `--measurement-time` is inert in `input_scan`, `parser_combinators` and `backtrack`
# and live in `cst` and `pratt_typed`. `tokora-benches/benches/support/mod.rs` is a no-op unless
# these variables are set and applies them after the pins, which is what lets one number reach
# all forty-six ids. The shipped defaults — 1s warm-up, 3s measurement — are untouched by this,
# so a local `cargo bench` and `bench (smoke)` still measure what they always did.
#
# 500ms buys about ten samples of tens to hundreds of iterations each on every id in the
# population; the shortest of them runs in tens of microseconds, four orders of magnitude above
# the clock. `compare.py` re-checks that from the readings rather than trusting this sentence.
: "${WALL_WARMUP_MS:=200}"
: "${WALL_MEASURE_MS:=500}"
: "${WALL_SAMPLES:=10}"

: "${WALL_WORK:=${RUNNER_TEMP:-/tmp}/wallclock}"

repo="$(git rev-parse --show-toplevel)"
cd "$repo"

plant=""
target_ref=""
positional=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --plant)
      [ "$#" -ge 2 ] || { echo "::error::--plant needs a name"; exit 2; }
      plant="$2"; shift 2 ;;
    *) target_ref="$1"; positional=$((positional + 1)); shift ;;
  esac
done

if [ "$positional" -ne 1 ]; then
  # Both patterns are anchored at `^# ` so that THIS line — which contains the start pattern as
  # data — cannot re-open the range and print the rest of the script.
  sed -n '/^# ── USAGE/,/^# Environment/p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

if [ "$target_ref" = "--self" ]; then
  base="$(git rev-parse HEAD)"
  mode="self-comparison"
  self=1
else
  base="$(git merge-base HEAD "$(git rev-parse "$target_ref")")"
  mode="vs merge-base"
  self=0
fi

head_sha="$(git rev-parse HEAD)"

# ── A branch that did not touch `tokora/` is a SELF-COMPARISON, and is told so ──────────────
#
# `tokora-benches` comes from HEAD on both sides — that is the whole instrument-is-the-constant
# trick — so the only source a delta can come from is `tokora/`. When the two sides' `tokora/`
# trees are the same OBJECT, the two builds are byte-identical source and every number below is
# noise by construction: there is no regression available for the gate to find.
#
# Saying so is not a convenience. Most pull requests in this repository touch documentation, CI
# or tests and nothing under `tokora/`, and for every one of them an unlabelled table of
# percentages reads like a verdict while being a measurement of the runner. Worse, an advisory
# job that reds on such a change teaches its readers that its reds mean nothing — which is how a
# gate dies. So the run is relabelled, and what it reports is the floor.
#
# A PLANT is the exception and is excluded: it changes the head side in the work tree, where git
# cannot see it, so the trees agree and the sources do not.
if [ "$self" = 0 ] && [ -z "$plant" ] \
   && [ "$(git rev-parse "$base:tokora")" = "$(git rev-parse "$head_sha:tokora")" ]; then
  self=1
  mode="self-comparison: this branch's \`tokora/\` IS the merge-base's"
fi

echo "wallclock: head $head_sha"
echo "wallclock: base $base ($mode)"
echo "wallclock: $WALL_ROUNDS rounds a side, ${WALL_WARMUP_MS}ms warm-up + ${WALL_MEASURE_MS}ms"
echo "wallclock: measurement x $WALL_SAMPLES samples an id, threshold +${WALL_THRESHOLD}%"

rm -rf "$WALL_WORK"
mkdir -p "$WALL_WORK/head" "$WALL_WORK/base"

# `git archive` rather than `git worktree add`: it materialises a tree with no `.git`, no
# `target/` and no bookkeeping to clean up, and it takes the tree from the OBJECT rather than
# from the working directory — so an uncommitted edit cannot leak into a side.
git archive "$head_sha" | tar -x -C "$WALL_WORK/head"
git archive "$head_sha" | tar -x -C "$WALL_WORK/base"
rm -rf "$WALL_WORK/base/tokora"
git archive "$base" tokora | tar -x -C "$WALL_WORK/base"

if [ -n "$plant" ]; then
  # The name comes from a `workflow_dispatch` input and is pasted into a path, so it is checked
  # to be a name and not a path before it is one.
  case "$plant" in
    *[!a-z0-9_]*) echo "::error::plant names are lowercase, digits and underscores: \`$plant\`"; exit 2 ;;
  esac
  script="$repo/ci/wallclock/plants/$plant.py"
  [ -f "$script" ] || {
    echo "::error::no plant named \`$plant\`. Available:"
    ls "$repo/ci/wallclock/plants/" | sed 's/\.py$//' | sed 's/^/::error::  /'
    exit 2
  }
  echo "wallclock: applying plant \`$plant\` to the HEAD side only"
  python3 "$script" "$WALL_WORK/head"
fi

# One lock for both sides. `Cargo.lock` is gitignored here, so each side would otherwise resolve
# its own — and two sides that linked different versions of some third crate would report a
# difference this gate would read as a change in tokora. The head side resolves first and the
# base side starts from that lock; cargo keeps every pin the base manifest can still satisfy and
# changes only what it must. Whether it had to is PRINTED below rather than assumed either way.
( cd "$WALL_WORK/head" && cargo generate-lockfile --quiet )
cp "$WALL_WORK/head/Cargo.lock" "$WALL_WORK/base/Cargo.lock"

for side in head base; do
  dir="$WALL_WORK/$side"
  # `--remap-path-prefix` so the two builds embed identical source paths. Panic locations and
  # `file!()` strings are baked into the binary, and two sides whose strings differ in LENGTH are
  # two binaries whose code and data are laid out differently — which for a gate that measures
  # cache behaviour is not a detail but the very quantity being read.
  #
  # `--profile bench` is what `[profile.bench]` in the root manifest configures and what these
  # benches are held to. Measuring another optimisation level would be measuring another program.
  ( cd "$dir" \
    && CARGO_TARGET_DIR="$WALL_WORK/target-$side" \
       RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$dir=/tokora" \
       cargo bench -p tokora-benches --no-run --profile bench \
         --message-format=json-render-diagnostics > "$WALL_WORK/$side.build.json" ) || {
    echo "::error::the $side side did not build."
    if [ "$side" = base ]; then
      echo "::error::That means this branch's benches cannot be compiled against the"
      echo "::error::merge-base's \`tokora\`, so the two revisions are not comparable by them."
      echo "::error::Usually this is a manifest or public-API change under \`tokora/\` that"
      echo "::error::\`tokora-benches\` names; the fix is in the benches, not in this script."
    fi
    exit 1
  }
  python3 ci/wallclock/exe_paths.py "$WALL_WORK/$side.build.json" > "$WALL_WORK/$side.bins.tsv"
done

if cmp -s "$WALL_WORK/head/Cargo.lock" "$WALL_WORK/base/Cargo.lock"; then
  echo "wallclock: lock parity ok — both sides resolved the same dependency graph"
else
  echo "wallclock: the two sides' locks DIFFER; cargo had to re-resolve for the base manifest:"
  diff -u "$WALL_WORK/base/Cargo.lock" "$WALL_WORK/head/Cargo.lock" \
    | grep -E '^[-+](name|version|source)' | sort -u | sed 's/^/wallclock:   /' || true
  echo "wallclock: a delta below may therefore be a dependency's, not this branch's."
fi

# The two sides must be the same instrument. They are built from the same `tokora-benches`, so
# this can only fail if a bench target exists on one side and not the other — which after the
# tree surgery above would mean the surgery went wrong.
if ! cmp -s <(cut -f1 "$WALL_WORK/head.bins.tsv") <(cut -f1 "$WALL_WORK/base.bins.tsv"); then
  echo "::error::the two sides built different bench targets:"
  diff <(cut -f1 "$WALL_WORK/base.bins.tsv") <(cut -f1 "$WALL_WORK/head.bins.tsv") || true
  exit 1
fi
targets="$(cut -f1 "$WALL_WORK/head.bins.tsv" | tr '\n' ' ')"
echo "wallclock: $(wc -l < "$WALL_WORK/head.bins.tsv" | tr -d ' ') bench targets a side: $targets"

# ── Are the two sides the same object? ──────────────────────────────────────────────────────
#
# Printed because it changes what a delta CAN mean, and because the first self-comparison this
# gate ran needed the answer and did not have it. A row came back +16.22% with each side's five
# rounds stable to under 1% and the two clusters disjoint — a difference between the two sides
# and not between two measurements. If the two binaries are byte-identical, no such row can be
# codegen and the cause is per-process placement: the addresses the loader and the allocator
# happen to hand out. If they differ, it can be either. On a real merge-base comparison they
# always differ and this line says nothing; on a self-comparison it is the whole diagnosis.
for target in $targets; do
  hb="$(awk -F'\t' -v t="$target" '$1 == t { print $2 }' "$WALL_WORK/head.bins.tsv")"
  bb="$(awk -F'\t' -v t="$target" '$1 == t { print $2 }' "$WALL_WORK/base.bins.tsv")"
  if cmp -s "$hb" "$bb"; then verdict="byte-identical"; else verdict="DIFFER"; fi
  echo "wallclock:   $target: the two sides' binaries are $verdict"
done

readings="$WALL_WORK/readings.jsonl"
: > "$readings"

measure_one() {
  # measure_one <round> <side> <target>
  local round="$1" side="$2" target="$3" bin
  bin="$(awk -F'\t' -v t="$target" '$1 == t { print $2 }' "$WALL_WORK/$side.bins.tsv")"
  [ -n "$bin" ] || { echo "::error::no $side binary for target $target"; exit 1; }
  python3 ci/wallclock/measure.py \
    --bin "$bin" --target "$target" --side "$side" --round "$round" \
    --home "$WALL_WORK/crit/$side-$round-$target" --append "$readings" \
    --warm-up-ms "$WALL_WARMUP_MS" --measurement-ms "$WALL_MEASURE_MS" \
    --sample-size "$WALL_SAMPLES"
}

started="$(date +%s)"
for round in $(seq 1 "$WALL_ROUNDS"); do
  # Counterbalanced: the pair of the same target runs back to back, and which side leads flips
  # every round. See the header for why neither property alone is enough.
  if [ $((round % 2)) -eq 1 ]; then order="base head"; else order="head base"; fi
  for target in $targets; do
    for side in $order; do
      measure_one "$round" "$side" "$target"
    done
  done
done
echo "wallclock: measurement took $(( ($(date +%s) - started) / 60 ))m $(( ($(date +%s) - started) % 60 ))s"

# The acceptances, harvested from THIS BRANCH'S OWN COMMITS — the same mechanism, the same
# trailer and the same one-shot property as the instruction-count gate. `$base..HEAD` is this
# change and nothing else, so a trailer licenses the commits it travels with and is gone from
# the next branch's range. A file checked into the tree would instead license every future drift
# in the same id, which against a merge-base comparison that resets each time is an unbounded
# allowance written as a bounded one.
git log --format=%B "$base..HEAD" | grep -E '^\s*Perf-accept:' > "$WALL_WORK/accept.txt" || true
if [ -s "$WALL_WORK/accept.txt" ]; then
  echo "wallclock: this branch carries acceptances:"
  sed 's/^/wallclock:   /' "$WALL_WORK/accept.txt"
fi

# `--self-comparison` only when the two sides really are the same source. A PLANTED self run is
# not one: it has a difference to find, and telling `compare.py` otherwise would have it report a
# floor over a regression.
self_flag=""
if [ "$self" = 1 ] && [ -z "$plant" ]; then self_flag="--self-comparison"; fi

# A plain string rather than an array, because `"${arr[@]}"` on an EMPTY array is an unbound
# variable under `set -u` in bash 3.2 — which is what `/bin/bash` still is on macOS, where the
# by-hand gate set at the top of `.github/workflows/ci.yml` is run. The value is a fixed literal
# from the line above, never anything a caller supplies, so leaving it unquoted splits nothing
# that could carry a space.
# shellcheck disable=SC2086
python3 ci/wallclock/compare.py "$readings" \
  --threshold "$WALL_THRESHOLD" \
  --accept-file "$WALL_WORK/accept.txt" \
  $self_flag \
  ${GITHUB_STEP_SUMMARY:+--summary "$GITHUB_STEP_SUMMARY"}
