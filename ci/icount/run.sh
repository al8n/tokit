#!/usr/bin/env bash
#
# Instruction-count regression gate: this branch's repetition engines against its merge-base's.
#
# ── THE ONE DECISION THIS SCRIPT IS ─────────────────────────────────────────────────────────
#
# Both sides are built and measured HERE, in one invocation, on one machine, with one
# toolchain. There is no committed baseline file, and adding one would break the gate rather
# than speed it up.
#
# Instruction counts are deterministic GIVEN A TOOLCHAIN. They are not stable across one: a
# rustc upgrade, a std change, an LLVM version, a dependency bump all move them, by amounts far
# larger than the regression this gate exists to see. A committed baseline turns every one of
# those into a red gate for a change nobody made — and a gate that reds for reasons nobody
# caused is a gate that gets disabled, which is a worse outcome than never having built it. A
# same-run A/B cancels all of it: whatever the compiler does to the head side it did to the
# base side thirty seconds earlier.
#
# ── AND THE ONE TRICK THAT MAKES IT WORK ────────────────────────────────────────────────────
#
# The merge-base does not contain this instrument — `tokora-icount` is new on this branch, and
# on the next branch the workloads will have been edited. So the base side is NOT a checkout of
# the merge-base. It is THIS branch's tree with `tokora/` replaced by the merge-base's
# `tokora/`: the same harness, the same fixtures, the same iteration counts, compiled against
# the older library. The instrument is the constant and the library is the variable, which is
# the only arrangement in which the difference means what it is read to mean.
#
# A consequence worth stating: if this branch changes `tokora/Cargo.toml` in a way the
# instrument depends on, the base side fails to BUILD, and this script reds saying so. That is
# correct and is not a false positive — it means the two revisions cannot be compared by this
# instrument, and a gate that answered anyway would be answering about something else.
#
# ── USAGE ───────────────────────────────────────────────────────────────────────────────────
#
#     ci/icount/run.sh <base-ref>   # compare HEAD against its merge-base with <base-ref>
#     ci/icount/run.sh --self       # compare HEAD against ITSELF: the gate's own noise floor
#
# Needs a Linux host with `valgrind`. Valgrind is unusable on current macOS/arm64, which is why
# the CI job runs on `ubuntu-latest` and why `--self` is the thing to run first on a new host:
# it says what this machine's floor is before any number from it is trusted.
#
# Environment: ICOUNT_THRESHOLD, ICOUNT_LO, ICOUNT_HI, ICOUNT_WORK.

set -euo pipefail

# ── The threshold ───────────────────────────────────────────────────────────────────────────
#
# THE CLAIM A CALLER RELIES ON: **this gate can see a 1% regression in any one workload.** On
# `sep_while`, the most heavily monomorphised axis, 1% is about 7 instructions per parsed
# element.
#
# It is not a round number picked for comfort. Four populations were measured under callgrind
# before it was chosen. They were taken on Linux/aarch64 (valgrind 3.19) and are quoted as
# PERCENTAGES for that reason: the absolute Ir counts are a property of the instruction set, so
# an x86_64 runner's numbers are its own and are not comparable to a developer's. What carries
# across is the shape — a floor of exactly zero and a plant an order of magnitude above the
# threshold — and the `icount` job re-checks the floor on the host it actually runs on every
# time this workflow is dispatched on `main`.
#
#   1. **The gate's own floor.** `--self` builds the identical source twice, in two directories,
#      into two target directories, and measures both. All ten workloads came back at a
#      difference of **EXACTLY ZERO instructions** — not "within noise", zero. There is no
#      run-to-run spread to clear, because callgrind counts what the binary does and the binary
#      does the same thing every time. A machine under load returns the same number as an idle
#      one, which is the property the criterion benches do not have.
#
#   2. **A prose-only change.** A commit adding one `///` line to `SeparatedWhile`'s docs and one
#      `//` comment inside the hottest line of the `sep_while` engine: all ten at +0.000%.
#
#   3. **Real merged commits.** Five recent `main` commits were replayed against their own
#      parents with this instrument on top — the population this gate will actually meet.
#      `a218439` (13 files under `parser/`), `b1faab1` (`feat(pratt)!`) and `73572b5`
#      (`feat(conformance)`) moved every workload by +0.000%. `a35a1d0` (`fix(input)!`, 13 files
#      under `src/`) spanned -0.323% to +0.005%. `abde449` — `fix(many)`, 21 files under
#      `parser/`, a correctness fix in these very engines — moved `sep_while` by **-1.413%** (an
#      improvement) and `repeated_while` by **+0.239%**, and left the other eight at +0.000%.
#      So the largest INCIDENTAL positive drift any of the five produced is a quarter of a
#      percent, and the threshold has four times that in hand. Note where the ONE reading over
#      1% sits: in the axis the commit changed, and in the improving direction, which this gate
#      reports and does not fail.
#
#   4. **A planted lost inline.** `#[inline(never)]` on `SeparatedWhile::handle_continue` — the
#      per-element handler every `sep_while/parse/*` specialisation inlines today, and exactly
#      the boundary a consolidation into shared engines introduces — moved `sep_while` by
#      **+10.797%** and `separated_while` by **+11.403%**, and left the other seven axes and the
#      control inside ±0.5%. The same attribute on `SeparatedWhile::parse`, which contains the
#      element loop rather than sitting inside it, cost **+0.998%**: one call per collection
#      instead of one per element, and the gate would just miss it. That is the honest bound on
#      what a 1% threshold does not see.
#
# So the margin is not for noise; there is none. It is for the changes that legitimately move a
# count without being a performance regression — an added match arm, a widened struct, a
# constant that grows a memcpy — and population 3 says those cost a quarter of a percent, not
# one.
#
# For scale: the criterion benches in `tokora-benches` measure the same engines through wall
# clock, where the run-to-run spread is 4.3-4.8% and the repetition engine is a third to a half
# of each bench — so the smallest engine regression THEY can distinguish from noise is around
# 9-10%, against a project that holds itself to ±2%.
: "${ICOUNT_THRESHOLD:=1.0}"

# The two iteration counts, and the reason they are what they are, are in `measure.py`.
: "${ICOUNT_LO:=8}"
: "${ICOUNT_HI:=104}"

: "${ICOUNT_WORK:=${RUNNER_TEMP:-/tmp}/icount}"

repo="$(git rev-parse --show-toplevel)"
cd "$repo"

if [ "$#" -ne 1 ]; then
  # Both patterns are anchored at `^# ` so that THIS line — which contains the start pattern
  # as data — cannot re-open the range and print the rest of the script. It did.
  sed -n '/^# ── USAGE/,/^# Environment/p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

if [ "$1" = "--self" ]; then
  base="$(git rev-parse HEAD)"
  mode="self-comparison"
else
  base="$(git merge-base HEAD "$(git rev-parse "$1")")"
  mode="vs merge-base"
fi

head_sha="$(git rev-parse HEAD)"
echo "icount: head $head_sha"
echo "icount: base $base ($mode)"

command -v valgrind >/dev/null || {
  echo "::error::valgrind is not installed; this gate cannot run on this host"
  exit 1
}

rm -rf "$ICOUNT_WORK"
mkdir -p "$ICOUNT_WORK/head" "$ICOUNT_WORK/base"

# `git archive` rather than `git worktree add`: it materialises a tree with no `.git`, no
# `target/`, and no bookkeeping to clean up, and it takes the tree from the OBJECT rather than
# from the working directory — so an uncommitted edit cannot leak into a side.
git archive "$head_sha" | tar -x -C "$ICOUNT_WORK/head"
git archive "$head_sha" | tar -x -C "$ICOUNT_WORK/base"
rm -rf "$ICOUNT_WORK/base/tokora"
git archive "$base" tokora | tar -x -C "$ICOUNT_WORK/base"

# One lock for both sides. `Cargo.lock` is gitignored here, so each side would otherwise
# resolve its own — and two sides that linked different versions of some third crate would
# report a difference this gate would read as a regression in tokora. The head side resolves
# first and the base side starts from that lock; cargo keeps every pin the base manifest can
# still satisfy and changes only what it must, so the two graphs are identical unless the
# branch itself moved a dependency requirement. Whether it did is PRINTED below rather than
# assumed either way.
( cd "$ICOUNT_WORK/head" && cargo generate-lockfile --quiet )
cp "$ICOUNT_WORK/head/Cargo.lock" "$ICOUNT_WORK/base/Cargo.lock"

for side in head base; do
  dir="$ICOUNT_WORK/$side"
  # `--remap-path-prefix` so the two builds embed identical source paths. Panic locations and
  # `file!()` strings are baked into the binary, and two sides whose strings differ in LENGTH
  # are two binaries that could differ in ways nothing here changed. Both map to the same name.
  #
  # `--profile bench` is the profile `[profile.bench]` in the root manifest configures — the
  # one the criterion benches are held to. Measuring a different optimisation level would be
  # measuring a different program. Cargo writes that profile's artefacts into `release/`, so
  # the path is read out of the JSON message stream rather than guessed.
  ( cd "$dir" \
    && CARGO_TARGET_DIR="$ICOUNT_WORK/target-$side" \
       RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$dir=/tokora" \
       cargo build -p tokora-icount --profile bench --message-format=json-render-diagnostics \
         > "$ICOUNT_WORK/$side.build.json" ) || {
    echo "::error::the $side side did not build."
    if [ "$side" = base ]; then
      echo "::error::That means this branch's instrument cannot be compiled against the"
      echo "::error::merge-base's \`tokora\`, so the two revisions are not comparable by it."
      echo "::error::Usually this is a manifest or public-API change under \`tokora/\` that"
      echo "::error::\`tokora-icount\` names; the fix is in the instrument, not in this script."
    fi
    exit 1
  }
  bin="$(python3 ci/icount/exe_path.py "$ICOUNT_WORK/$side.build.json")"
  if [ "$side" = head ]; then bin_head="$bin"; else bin_base="$bin"; fi
done

if cmp -s "$ICOUNT_WORK/head/Cargo.lock" "$ICOUNT_WORK/base/Cargo.lock"; then
  echo "icount: lock parity ok — both sides resolved the same dependency graph"
else
  echo "icount: the two sides' locks DIFFER; cargo had to re-resolve for the base manifest:"
  diff -u "$ICOUNT_WORK/base/Cargo.lock" "$ICOUNT_WORK/head/Cargo.lock" \
    | grep -E '^[-+](name|version|source)' | sort -u | sed 's/^/icount:   /' || true
  echo "icount: a delta below may therefore be a dependency's, not this branch's."
fi

# `--linearity` on the head side only, and it is not decoration: it takes a THIRD reading and
# requires the second difference to vanish. Every number this gate reports is a difference of
# two points on a line the model assumes is straight, and this is the check that the line is
# straight. One side is enough — it is a property of the instrument, and the instrument is the
# same code on both sides.
python3 ci/icount/measure.py --bin "$bin_head" --out "$ICOUNT_WORK/head.json" \
  --label " head" --iters-lo "$ICOUNT_LO" --iters-hi "$ICOUNT_HI" --linearity
python3 ci/icount/measure.py --bin "$bin_base" --out "$ICOUNT_WORK/base.json" \
  --label " base" --iters-lo "$ICOUNT_LO" --iters-hi "$ICOUNT_HI"

# The acceptances, harvested from THIS BRANCH'S OWN COMMITS. That is what makes an acceptance
# one-shot: `$base..HEAD` is this change and nothing else, so a trailer licenses the commits it
# travels with and is gone from the next branch's range. A file checked into the tree would
# instead sit there licensing every future drift in the same workload, which — against a
# merge-base comparison that resets each time — is an unbounded allowance written as a bounded
# one.
git log --format=%B "$base..HEAD" | grep -E '^\s*Perf-accept:' > "$ICOUNT_WORK/accept.txt" || true
if [ -s "$ICOUNT_WORK/accept.txt" ]; then
  echo "icount: this branch carries acceptances:"
  sed 's/^/icount:   /' "$ICOUNT_WORK/accept.txt"
fi

python3 ci/icount/compare.py "$ICOUNT_WORK/base.json" "$ICOUNT_WORK/head.json" \
  --threshold "$ICOUNT_THRESHOLD" \
  --accept-file "$ICOUNT_WORK/accept.txt" \
  ${GITHUB_STEP_SUMMARY:+--summary "$GITHUB_STEP_SUMMARY"}
