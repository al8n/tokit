#!/usr/bin/env bash
#
# Self-test for `ci/icount/compare.py` — the half of the gate that DECIDES.
#
# `measure.py` is watched by its own reading: a broken callgrind parse produces an absurd number
# or no number at all, and `--linearity` refuses to report on a curve that is not a line. The
# decision logic has no such witness. A `compare.py` that returned 0 on everything would leave
# the `icount` job green, the table printed, the numbers right — and the gate off. This
# repository has twice caught a checker passing on already-broken input, which is why the
# changelog job runs its self-test first and not optionally; the same reasoning applies here.
#
# Runs in about a second and needs neither valgrind nor a build: the readings are synthetic.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0

reading() {
  # reading <file> <sep_while ir_delta> [sep_while_shallow ir_delta] [iters_lo]
  #
  # The pair is in every synthetic reading because the deep/shallow note is DERIVED from the
  # names the binary declared rather than from a table of pairs, and a derivation with no pair
  # in front of it is a derivation nothing exercises.
  python3 - "$1" "$2" "${3:-20000000}" "${4:-8}" <<'PY'
import json, sys
path, sep_while, shallow, lo = (
    sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
)
json.dump(
    {
        "iters_lo": lo,
        "iters_hi": 104,
        "workloads": {
            "sep_while": {"ir_delta": sep_while},
            "sep_while_shallow": {"ir_delta": shallow},
            "repeated": {"ir_delta": 10_000_000},
            "scan_drain": {"ir_delta": 9_000_000},
        },
    },
    open(path, "w"),
)
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
# The pair note is a claim about WHERE a cost is paid, and a claim printed under a reading that
# does not support it is worse than no claim. So each note is watched from both sides: fired
# where it belongs, and silent where it does not.
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
    echo "FAIL $name: output contains what it must not: $forbidden"
    echo "$out" | sed 's/^/    /'
    failures=$((failures + 1))
    return
  fi
  echo "ok   $name"
}

reading "$tmp/base.json" 10000000
reading "$tmp/same.json" 10000000
reading "$tmp/up2.json"  10200000   # +2.000% on sep_while
reading "$tmp/down.json"  9000000   # -10% on sep_while: an improvement
reading "$tmp/lo16.json" 10000000 20000000 16
# The three ways a deep/shallow pair can read. Only the shallow row moves in the first — what a
# per-collection cost produces; only the deep row in the second; both in the third.
reading "$tmp/shallow-only.json" 10000000 20800000   # +4% on sep_while_shallow alone
reading "$tmp/deep-only.json"    10400000 20000000   # +4% on sep_while alone
reading "$tmp/both.json"         10400000 20800000   # +4% on both
python3 - "$tmp/short.json" <<'PY'
import json, sys
json.dump({"iters_lo": 8, "iters_hi": 104,
           "workloads": {"sep_while": {"ir_delta": 10_000_000}}}, open(sys.argv[1], "w"))
PY

echo "Perf-accept: sep_while +1.5% a ceiling below the delta" > "$tmp/accept-low.txt"
echo "Perf-accept: sep_while +3% the trade is worth it"       > "$tmp/accept-ok.txt"
echo "Perf-accept: sep_while +3%"                             > "$tmp/accept-noreason.txt"
echo "Perf-accept: sep_while lots"                            > "$tmp/accept-nopct.txt"

# The positive control comes first: a check that cannot pass is as useless as one that cannot
# fail, and every case below is read against this one.
check "identical readings pass" 0 "no workload regressed" -- \
  "$tmp/base.json" "$tmp/same.json" --threshold 1.0

check "an improvement passes and is reported" 0 "-10.000%" -- \
  "$tmp/base.json" "$tmp/down.json" --threshold 1.0

check "a regression over the threshold fails" 1 "::error::sep_while costs +2.000% more" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 1.0

check "the failure names the absolute delta too" 1 "+2,083" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 1.0

check "a regression under the threshold passes" 0 "no workload regressed" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 3.0

# The one that matters most: an acceptance is a CEILING, not a permission slip. A ceiling below
# the measured delta must still fail, or the mechanism degenerates into "any trailer disables the
# gate for that workload" — which is the shape that gets a gate deleted after it is trusted once.
check "an acceptance ceiling below the delta still fails" 1 "::error::sep_while costs +2.000%" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 1.0 --accept-file "$tmp/accept-low.txt"

check "an acceptance ceiling above the delta passes" 0 "accepted (<= +3%)" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 1.0 --accept-file "$tmp/accept-ok.txt"

check "an accepted regression prints its reason" 0 "reason: the trade is worth it" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 1.0 --accept-file "$tmp/accept-ok.txt"

check "an acceptance with no reason is refused" 1 "cannot read this as an acceptance" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 1.0 --accept-file "$tmp/accept-noreason.txt"

check "an acceptance with no percentage is refused" 1 "cannot read this as an acceptance" -- \
  "$tmp/base.json" "$tmp/up2.json" --threshold 1.0 --accept-file "$tmp/accept-nopct.txt"

# The pair is measured twice so that a SPLIT between the two rows can be read, so the split has
# to be reported. A note nobody sees is a second reading nobody uses.
check "a shallow-only failure is attributed to a per-collection cost" 1 "paid once per COLLECTION" -- \
  "$tmp/base.json" "$tmp/shallow-only.json" --threshold 1.0

check_absent "and is not also attributed to a per-element one" 1 "A per-element cost reads slightly higher" -- \
  "$tmp/base.json" "$tmp/shallow-only.json" --threshold 1.0

check "a deep-only failure is attributed the other way" 1 "A per-element cost reads slightly higher" -- \
  "$tmp/base.json" "$tmp/deep-only.json" --threshold 1.0

check_absent "and not to a per-collection cost" 1 "paid once per COLLECTION" -- \
  "$tmp/base.json" "$tmp/deep-only.json" --threshold 1.0

# The control the two above are read against: a pair that agrees says nothing about where the
# cost is paid, and must therefore say nothing at all.
check_absent "a pair that moves together gets no split note" 1 "paid once per COLLECTION" -- \
  "$tmp/base.json" "$tmp/both.json" --threshold 1.0

check_absent "in neither direction" 1 "A per-element cost reads slightly higher" -- \
  "$tmp/base.json" "$tmp/both.json" --threshold 1.0

# Two ways the two sides can stop being comparable. Both must red rather than answer.
check "a workload set mismatch is refused" 1 "measured different workloads" -- \
  "$tmp/base.json" "$tmp/short.json" --threshold 1.0

check "an iteration-count mismatch is refused" 1 "read at different iters_lo" -- \
  "$tmp/base.json" "$tmp/lo16.json" --threshold 1.0

if [ "$failures" -ne 0 ]; then
  echo "::error::compare.py self-test: $failures case(s) failed"
  exit 1
fi
echo "compare.py self-test: all cases behaved as required"
