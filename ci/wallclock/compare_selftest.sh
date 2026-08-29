#!/usr/bin/env bash
#
# Self-test for `ci/wallclock/compare.py` — the half of the gate that DECIDES.
#
# `measure.py` is watched by its own reading: a criterion binary that ran in test mode writes no
# output files and the harvest refuses an empty one, and a sample with zero iterations or zero
# time is rejected where it is read. The decision logic has no such witness. A `compare.py` that
# returned 0 on everything would leave the `wall clock` job green, the table printed, the numbers
# right — and the gate off. This repository has twice caught a checker passing on already-broken
# input, which is why the changelog and instruction-count jobs run their self-tests first and not
# optionally; the same reasoning applies here, and applies harder to an ADVISORY job, whose green
# nobody is waiting on and whose silence therefore costs nothing to notice.
#
# Runs in about a second and needs neither a build nor a bench: the readings are synthetic.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0

# `readings <file> <base ns> <head ns> [rounds] [extra-json-lines...]`
#
# Two ids in every synthetic reading, because a gate over one row exercises none of the
# per-population reasoning — the id-set comparison, the every-row note, the worst-noise report —
# that decides what a failure MEANS.
readings() {
  python3 - "$@" <<'PY'
import json, sys
path, base_ns, head_ns = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
rounds = int(sys.argv[4]) if len(sys.argv) > 4 else 3
control_ns = float(sys.argv[5]) if len(sys.argv) > 5 else 90_000.0
with open(path, "w") as fh:
    for r in range(1, rounds + 1):
        for side, hot in (("base", base_ns), ("head", head_ns)):
            # A round's reading is never exactly the minimum; the r-th round is 1% slower than
            # the first, so `min` has something to do and the reported own-noise is nonzero.
            drift = 1.0 + 0.01 * (r - 1)
            fh.write(json.dumps({
                "target": "parser_combinators", "side": side, "round": r, "seconds": 4.0,
                "ids": {
                    "parser/repeated_collect": {
                        "ns_per_iter": hot * drift, "samples": 10,
                        "sample_spread_pct": 3.0, "sampling_mode": "Linear",
                    },
                    "cst/finish_clean": {
                        "ns_per_iter": control_ns * drift, "samples": 10,
                        "sample_spread_pct": 3.0, "sampling_mode": "Linear",
                    },
                },
            }, sort_keys=True) + "\n")
PY
}

# `check <name> <expected-exit> <expected-substring> -- <compare.py args...>`
check() {
  local name="$1" want_exit="$2" want_text="$3"; shift 4
  local out rc
  set +e
  out="$(python3 "$here/compare.py" "$@" 2>&1)"; rc=$?
  set -e
  if [ "$rc" -ne "$want_exit" ]; then
    echo "FAIL $name: exit $rc, expected $want_exit"
    echo "$out" | sed 's/^/    /'
    failures=$((failures + 1))
    return
  fi
  if ! printf '%s' "$out" | grep -qF -- "$want_text"; then
    echo "FAIL $name: output does not contain: $want_text"
    echo "$out" | sed 's/^/    /'
    failures=$((failures + 1))
    return
  fi
  echo "ok   $name"
}

# `check_absent <name> <expected-exit> <forbidden-substring> -- <compare.py args...>`
#
# A note is a claim about what the numbers MEAN, and a claim printed under a reading that does not
# support it is worse than no claim. So each is watched from both sides.
check_absent() {
  local name="$1" want_exit="$2" forbidden="$3"; shift 4
  local out rc
  set +e
  out="$(python3 "$here/compare.py" "$@" 2>&1)"; rc=$?
  set -e
  if [ "$rc" -ne "$want_exit" ]; then
    echo "FAIL $name: exit $rc, expected $want_exit"
    echo "$out" | sed 's/^/    /'
    failures=$((failures + 1))
    return
  fi
  if printf '%s' "$out" | grep -qF -- "$forbidden"; then
    echo "FAIL $name: output should NOT contain: $forbidden"
    echo "$out" | sed 's/^/    /'
    failures=$((failures + 1))
    return
  fi
  echo "ok   $name"
}

# ── The verdict ─────────────────────────────────────────────────────────────────────────────

readings "$tmp/flat.jsonl" 1000 1000
check "identical sides pass" 0 "no id regressed" -- \
  "$tmp/flat.jsonl" --threshold 10

readings "$tmp/small.jsonl" 1000 1050
check "a delta under the threshold passes" 0 "no id regressed" -- \
  "$tmp/small.jsonl" --threshold 10

readings "$tmp/big.jsonl" 1000 1300
check "a delta over the threshold fails" 1 "::error::parser/repeated_collect is +30.00% slower" -- \
  "$tmp/big.jsonl" --threshold 10

# The boundary is `<=`, both sides of it, because a threshold whose own edge is untested is a
# threshold that is one comparison operator away from being off by a whole row.
readings "$tmp/edge.jsonl" 1000 1100
check "exactly at the threshold passes" 0 "no id regressed" -- \
  "$tmp/edge.jsonl" --threshold 10
readings "$tmp/over.jsonl" 1000 1100.01
check "a hair over the threshold fails" 1 "::error::parser/repeated_collect" -- \
  "$tmp/over.jsonl" --threshold 10

# An IMPROVEMENT is reported and never failed. It is also the reading the every-row note must
# not fire on.
readings "$tmp/faster.jsonl" 1000 700
check "an improvement passes" 0 "-30.00%" -- \
  "$tmp/faster.jsonl" --threshold 10

# ── The acceptances ─────────────────────────────────────────────────────────────────────────

printf 'Perf-accept: parser/repeated_collect +35%% the CST arena is worth two cache lines\n' \
  > "$tmp/accept.txt"
check "an acceptance wide enough licenses the delta" 0 "accepted (<= +35%)" -- \
  "$tmp/big.jsonl" --threshold 10 --accept-file "$tmp/accept.txt"
check "and its reason is printed" 0 "reason: the CST arena is worth two cache lines" -- \
  "$tmp/big.jsonl" --threshold 10 --accept-file "$tmp/accept.txt"

printf 'Perf-accept: parser/repeated_collect +20%% not enough headroom\n' > "$tmp/narrow.txt"
check "an acceptance narrower than the delta does not" 1 "::error::parser/repeated_collect" -- \
  "$tmp/big.jsonl" --threshold 10 --accept-file "$tmp/narrow.txt"

# The widest ceiling wins: the later commit is the one that knew what the earlier ones cost.
printf 'Perf-accept: parser/repeated_collect +20%% first\nPerf-accept: parser/repeated_collect +35%% second\n' \
  > "$tmp/two.txt"
check "the widest of two ceilings wins" 0 "accepted (<= +35%)" -- \
  "$tmp/big.jsonl" --threshold 10 --accept-file "$tmp/two.txt"

printf 'Perf-accept: parser/repeated_collect +35%%\n' > "$tmp/noreason.txt"
check "an acceptance with no reason is refused" 1 "cannot read this as an acceptance" -- \
  "$tmp/big.jsonl" --threshold 10 --accept-file "$tmp/noreason.txt"

# A criterion id is `parser/repeated_collect`, not `sep_while`. The one widening this gate makes
# to the icount trailer's grammar is the id charset, so the grammar is tested at the character
# that motivated it.
printf 'Perf-accept: sep_while +35%% this one is the instruction-count gate\n' > "$tmp/other.txt"
check "a trailer for the other gate is a note, not a failure" 1 "If it was meant for the instruction-count gate" -- \
  "$tmp/big.jsonl" --threshold 10 --accept-file "$tmp/other.txt"
check_absent "and does not license this gate's row" 1 "accepted" -- \
  "$tmp/big.jsonl" --threshold 10 --accept-file "$tmp/other.txt"

# ── The readings must be comparable at all ──────────────────────────────────────────────────

python3 - "$tmp/mismatch.jsonl" <<'PY'
import json, sys
with open(sys.argv[1], "w") as fh:
    for side, ids in (("base", ["a", "b"]), ("head", ["a", "c"])):
        fh.write(json.dumps({
            "target": "t", "side": side, "round": 1, "seconds": 1.0,
            "ids": {i: {"ns_per_iter": 1000.0, "samples": 10, "sample_spread_pct": 1.0}
                    for i in ids},
        }) + "\n")
PY
check "different id sets are refused" 1 "the two sides measured different ids" -- \
  "$tmp/mismatch.jsonl" --threshold 10

python3 - "$tmp/rounds.jsonl" <<'PY'
import json, sys
with open(sys.argv[1], "w") as fh:
    for side, rounds in (("base", [1, 2]), ("head", [1, 3])):
        for r in rounds:
            fh.write(json.dumps({
                "target": "t", "side": side, "round": r, "seconds": 1.0,
                "ids": {"a": {"ns_per_iter": 1000.0, "samples": 10, "sample_spread_pct": 1.0}},
            }) + "\n")
PY
check "different rounds are refused" 1 "the two sides ran different rounds" -- \
  "$tmp/rounds.jsonl" --threshold 10

python3 - "$tmp/hole.jsonl" <<'PY'
import json, sys
with open(sys.argv[1], "w") as fh:
    for side in ("base", "head"):
        for r in (1, 2):
            ids = {"a": {"ns_per_iter": 1000.0, "samples": 10, "sample_spread_pct": 1.0}}
            # `b` is missing from one round of one side: the id sets and the round sets both
            # still agree, and only the per-id count catches it.
            if not (side == "head" and r == 2):
                ids["b"] = {"ns_per_iter": 2000.0, "samples": 10, "sample_spread_pct": 1.0}
            fh.write(json.dumps({"target": "t", "side": side, "round": r, "seconds": 1.0,
                                 "ids": ids}) + "\n")
PY
check "a round missing for one id is refused" 1 "a round is missing from the readings" -- \
  "$tmp/hole.jsonl" --threshold 10

# ── The self-comparison, which is what the threshold rests on ───────────────────────────────

readings "$tmp/self_ok.jsonl" 1000 1030
check "a self-comparison inside the threshold reports the floor" 0 "SELF-COMPARISON" -- \
  "$tmp/self_ok.jsonl" --threshold 10 --self-comparison
check "and says how much headroom is left" 0 "clears the floor by" -- \
  "$tmp/self_ok.jsonl" --threshold 10 --self-comparison

readings "$tmp/self_bad.jsonl" 1000 1250
check "a self-comparison past the threshold fails the CONFIGURATION" 1 "reds on unchanged code" -- \
  "$tmp/self_bad.jsonl" --threshold 10 --self-comparison
# A negative floor is a floor. Taking the signed maximum instead of the absolute one is the
# single most likely way to write this check so that it passes on half its failures.
readings "$tmp/self_neg.jsonl" 1000 800
check "a self-comparison that is fast in the wrong direction also fails" 1 "reds on unchanged code" -- \
  "$tmp/self_neg.jsonl" --threshold 10 --self-comparison

# ── The notes ───────────────────────────────────────────────────────────────────────────────

# Every row moved, including the control that shares no code path: a runner story, not a branch
# one. Both ids are pushed past the threshold together.
readings "$tmp/all.jsonl" 1000 1300 3 90000
python3 - "$tmp/all.jsonl" <<'PY'
import json, sys
lines = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
for rec in lines:
    if rec["side"] == "head":
        rec["ids"]["cst/finish_clean"]["ns_per_iter"] *= 1.3
with open(sys.argv[1], "w") as fh:
    for rec in lines:
        fh.write(json.dumps(rec, sort_keys=True) + "\n")
PY
check "every row moving is called out as a runner story" 1 "note: EVERY id regressed" -- \
  "$tmp/all.jsonl" --threshold 10
check_absent "and is not claimed when only one row moved" 1 "note: EVERY id regressed" -- \
  "$tmp/big.jsonl" --threshold 10

# ── The estimator is min-of-N, and that is the whole design ─────────────────────────────────
#
# Every case above passes just as well if the round-fold is a MEAN: the synthetic rounds drift
# uniformly on both sides, so the two estimators agree on the ratio. That is the shape of a suite
# that pins its cases and not its property. The readings below are the shape real noise takes —
# one round of one side hit by something, the rest clean — and they separate the two estimators
# by construction: the minimum ignores the spike, the mean does not.

spiked() {
  # spiked <file> <side-to-spike> <spike-ns>
  python3 - "$@" <<'PY'
import json, sys
path, spiked_side, spike = sys.argv[1], sys.argv[2], float(sys.argv[3])
with open(path, "w") as fh:
    for r in (1, 2, 3):
        for side in ("base", "head"):
            ns = spike if (side == spiked_side and r == 3) else 1000.0
            fh.write(json.dumps({
                "target": "t", "side": side, "round": r, "seconds": 4.0,
                "ids": {"parser/repeated_collect": {
                    "ns_per_iter": ns, "samples": 10, "sample_spread_pct": 3.0}},
            }, sort_keys=True) + "\n")
PY
}

# min of (1000, 1000, 1400) is 1000 on both sides: no delta. The mean of the head side would be
# 1133, a +13.3% "regression" invented entirely by one bad round.
spiked "$tmp/spike_head.jsonl" head 1400
check "a one-round spike on the head side is not a regression" 0 "no id regressed" -- \
  "$tmp/spike_head.jsonl" --threshold 10
# And the mirror, which a max-fold or a last-round-wins fold would get wrong in the other
# direction: the same spike on the base side must not read as an improvement either.
spiked "$tmp/spike_base.jsonl" base 1400
check "a one-round spike on the base side is not an improvement" 0 "+0.00%" -- \
  "$tmp/spike_base.jsonl" --threshold 10
# The spike is still REPORTED, as the row's own noise — a reading whose rounds moved 40% is a
# reading a human should see, even when the fold correctly ignored it.
check "and the spike is reported as the row's own noise" 0 "40.00%" -- \
  "$tmp/spike_head.jsonl" --threshold 10

# ── measure.py's half of the same estimator ─────────────────────────────────────────────────
#
# `compare.py` folds rounds with `min`; `measure.py` folds criterion's samples within a round the
# same way, and a mutation there is just as silent. `harvest` is called directly over a synthetic
# criterion output tree — which also pins the decision to read the id out of `benchmark.json`
# rather than rebuild it from a path, since the group id below contains a slash.
# `PYTHONDONTWRITEBYTECODE`: importing `measure.py` by path would otherwise leave a
# `__pycache__/` beside it, in a directory this repository does not ignore.
if PYTHONDONTWRITEBYTECODE=1 python3 - "$here" "$tmp/crit" <<'PY'
import importlib.util, json, os, sys

here, home = sys.argv[1], sys.argv[2]
spec = importlib.util.spec_from_file_location("wm", os.path.join(here, "measure.py"))
wm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wm)

d = os.path.join(home, "input", "scan", "next_drain", "new")
os.makedirs(d, exist_ok=True)
json.dump({"group_id": "input/scan", "function_id": "next_drain", "value_str": None,
           "throughput": None, "full_id": "input/scan/next_drain",
           "directory_name": "input/scan/next_drain", "title": "input/scan/next_drain"},
          open(os.path.join(d, "benchmark.json"), "w"))
# Per-sample per-iteration costs of 100, 90 and 130 ns: the minimum is 90, the mean is 106.7,
# and criterion's own slope would be something else again.
json.dump({"sampling_mode": "Linear",
           "iters": [10.0, 20.0, 30.0], "times": [1000.0, 1800.0, 3900.0]},
          open(os.path.join(d, "sample.json"), "w"))

got = wm.harvest(home)
assert list(got) == ["input/scan/next_drain"], got
row = got["input/scan/next_drain"]
assert abs(row["ns_per_iter"] - 90.0) < 1e-9, f"expected the MINIMUM 90, got {row['ns_per_iter']}"
assert abs(row["sample_spread_pct"] - (130 / 90 - 1) * 100) < 1e-6, row
PY
then
  echo "ok   measure.py folds criterion's samples with min, and reads the id from benchmark.json"
else
  echo "FAIL measure.py's harvest is not the minimum over samples, or lost the id"
  failures=$((failures + 1))
fi

# An id too fast to price is REPORTED, never dropped. The floor is a parameter so the test does
# not need a synthetic nanosecond-scale bench to reach it.
check "an id under the resolution floor is reported" 0 "below this gate's" -- \
  "$tmp/flat.jsonl" --threshold 10 --short-id-floor-ns 2000
check_absent "and is silent when every id clears it" 0 "below this gate's" -- \
  "$tmp/flat.jsonl" --threshold 10 --short-id-floor-ns 500

# The threshold cannot be trusted below the instrument's own round-to-round spread, and that is
# said whatever the verdict. The synthetic readings drift 1% a round over three rounds.
check "a threshold under the observed same-build spread is called out" 0 "which is more than the" -- \
  "$tmp/flat.jsonl" --threshold 1
check_absent "and is not called out when the threshold clears it" 0 "which is more than the" -- \
  "$tmp/flat.jsonl" --threshold 10

# ── The summary ─────────────────────────────────────────────────────────────────────────────

rm -f "$tmp/summary.md"
python3 "$here/compare.py" "$tmp/big.jsonl" --threshold 10 --summary "$tmp/summary.md" > /dev/null \
  || true
if grep -qF '| `parser/repeated_collect` |' "$tmp/summary.md" \
   && grep -qF 'advisory' "$tmp/summary.md"; then
  echo "ok   the step summary carries the row and says it is advisory"
else
  echo "FAIL the step summary is missing the row or the advisory label"
  sed 's/^/    /' "$tmp/summary.md"
  failures=$((failures + 1))
fi

# A SELF-COMPARISON's summary is the one that matters most and was the one missing: the early
# return skipped the writer entirely, so every run that relabelled itself — which is every pull
# request that does not touch `tokora/` — wrote nothing where a reviewer looks.
for case in self_ok:0 self_bad:1; do
  file="${case%%:*}"
  rm -f "$tmp/summary.md"
  python3 "$here/compare.py" "$tmp/$file.jsonl" --threshold 10 --self-comparison \
    --summary "$tmp/summary.md" > /dev/null 2>&1 || true
  if [ -s "$tmp/summary.md" ] && grep -qiF 'self-comparison' "$tmp/summary.md" \
     && grep -qF '| `parser/repeated_collect` |' "$tmp/summary.md"; then
    echo "ok   a self-comparison writes a labelled step summary ($file)"
  else
    echo "FAIL a self-comparison wrote no usable step summary ($file)"
    sed 's/^/    /' "$tmp/summary.md" 2>/dev/null || echo "    (file absent)"
    failures=$((failures + 1))
  fi
done

if [ "$failures" -ne 0 ]; then
  echo
  echo "::error::compare.py self-test: $failures case(s) failed"
  exit 1
fi
echo
echo "compare.py self-test: all cases passed"
