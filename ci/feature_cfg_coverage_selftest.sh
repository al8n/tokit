#!/bin/bash
#
# Self-test for ci/feature_cfg_coverage.py.
#
# The gate it tests exists because `cargo hack --each-feature` reads as coverage of the feature
# space while covering a slice of it, and the miss it hid downstream survived from the commit
# that introduced it with green CI throughout (#200). Replacing that with a coverage script
# that only ever passes would reproduce the defect one level up — and a coverage script is
# unusually easy to write that way, because its OK path is the path everything already takes.
# The first run of the gate proved the point: it reported "0 multi-feature cfg predicates",
# because the scanner blanked string literals and `feature = "std"` IS a string literal. Only
# the positive control told that apart from a real green.
#
# So the assertions here are that the gate goes RED, and for the stated reason:
#
#   A. A NEW UNCOVERED PREDICATE. A throwaway `#[cfg(all(feature = "peek", feature = "bstr_1"))]`
#      item is planted in a real source file — a combinator family and a string backing, which
#      nothing pulls together except `--all-features`. The gate must red AND print the
#      predicate. (A `peek` + `punct` pair, the obvious plant, does NOT red and must not:
#      `--no-default-features --features combinators` is a real `--each-feature` leg and it
#      enables all thirteen families at once. Case E below pins that, because a gate that reds
#      on a configuration CI genuinely builds is a gate people learn to route around.)
#
#   B. A DELETED LEG. Every declared leg claiming `unique_predicates=True` is removed in turn
#      and the gate must red, naming an uncovered predicate. This is the assertion that the
#      legs CI runs are load-bearing rather than decorative — if removing a leg changes
#      nothing, the leg detects nothing. The list is read out of the gate itself, so a leg
#      added to `EXTRA_LEGS` is tested the day it is added.
#
#   C. A STALE DECLARATION. `unique_predicates` is flipped on the leg that declares False. The
#      gate must red, because the claim and the tree now disagree. This is what stops the leg
#      list accreting entries whose reason has quietly evaporated.
#
#   D. THE GREEN CASE, on the unmodified tree — the false-positive guard. "Everything reds" is
#      not the same as "everything is checked".
#
#   E. A COVERED PLANT. `peek` + `punct`, which `--features combinators` builds. Must stay
#      green.
#
#   F. THE DELETION RECOMMENDATION, REFUSED. For every leg claiming BOTH `unique_predicates`
#      and `unique_suites`, the `src/` sites of the predicates it alone covers are stripped and
#      the gate must red WITHOUT recommending deletion. This is the case the gate was missing:
#      the `std,logos,trace` leg was held by one predicate over three `src/` sites and was the
#      only configuration in CI type-checking eight suite bodies with the combinator families
#      off, so removing those three sites — ordinary test cleanup — made the gate print `delete
#      the leg`, and following its own advice would have dropped the eight with CI green
#      throughout. The assertion is therefore two-sided: the refusal text must be present AND
#      the deletion text must be absent. A guard nobody has watched refuse is the same
#      unfalsifiable claim it replaces.
#
#   G. A STALE unique_suites CLAIM, the same shape as C one level out.
#
#   H. THE SAME REFUSAL, WITH THE ALTERNATIVES SPLIT. F's leg is held because ONE feature is on
#      in every other leg compiling its suites. That is a fact about today's leg list, not about
#      the leg: split the alternatives finely enough and no single feature is common to all of
#      them while the CONFIGURATION the leg protects is still reproduced by none. This case
#      appends two further legs that compile one of the leg's shared suites with a different
#      extra feature each — the per-family split `tokora/Cargo.toml` already discusses — strips
#      the leg's `src/` sites as F does, and demands the same refusal. It is the case that tells
#      "is there a feature they all have" apart from "does another leg reproduce this
#      configuration"; the first answers no here and recommends deletion.
#
#   I. AN ORPHANED SUITE — case A one level out. A throwaway `tokora/tests/*.rs` is planted whose
#      crate-level gate no declared leg satisfies, and the gate must red AND name the file. It
#      is here because the tree itself cannot supply the case: the live count of suites no leg
#      compiles is zero, so without a plant the failure path that enforces that has never been
#      watched to fire, and "reported, not enforced" is what it was until 2026-08-11.
#
# Every case runs against a COPY of the tree, so a failed case cannot leave the checkout
# mutated. Exit codes are captured directly, never through a pipe: `cmd | tail; rc=$?` reads
# `tail`'s status, and that has shipped broken checks in this repository before.
#
# Usage:  bash ci/feature_cfg_coverage_selftest.sh

set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
GATE=ci/feature_cfg_coverage.py

[ -f "$root/$GATE" ] || { echo "selftest: $root/$GATE does not exist" >&2; exit 1; }

work=$(mktemp -d "${TMPDIR:-/tmp}/feature-cfg-selftest.XXXXXX") || exit 1
trap 'rm -rf "$work"' EXIT INT TERM

# One pristine copy, re-cloned per case. `cargo metadata --no-deps` needs EVERY workspace
# member's manifest — a `members` entry it cannot load is a hard error, not a skipped package —
# and it needs each explicitly-`path`'d target to exist, so `tokora-benches` comes over whole
# (#216). tokora's own `examples/` do not, because its `[[example]]` sections are the only
# declared targets left whose sources this copy omits and `cargo metadata` is content with a
# missing autodiscovered directory.
#
# `tokora/tests` comes over WHOLE, not filtered to the `*.rs` the suite scan globs. The gate
# reads only the top level today; a fixture trimmed to exactly what a scan reads is a fixture
# that silently under-supplies the day the scan widens, and this file's own history is the
# argument — see the paragraph below.
#
# Copying the workspace root manifest and then NOT copying a member is the failure this comment
# exists to prevent: it makes all six cases die inside `feature_map()` with a `cargo metadata`
# exit 101, which reads like a broken gate rather than a broken fixture. It happened a second
# time on 2026-08-11, from the other end: the gate learned to scan `tokora/tests`, this fixture
# did not copy it, and all six cases died on `tokora/tests does not exist`. Loud, and caught by
# the selftest itself — which is the shape a fixture gap is supposed to have.
base=$work/base
mkdir -p "$base/ci" "$base/tokora" || exit 1
cp "$root/Cargo.toml" "$base/" || exit 1
[ -f "$root/Cargo.lock" ] && cp "$root/Cargo.lock" "$base/"
cp "$root/$GATE" "$base/ci/" || exit 1
cp "$root/tokora/Cargo.toml" "$base/tokora/" || exit 1
cp -R "$root/tokora/src" "$base/tokora/" || exit 1
cp -R "$root/tokora/tests" "$base/tokora/" || exit 1
cp -R "$root/tokora-benches" "$base/" || exit 1

fails=0
total=0

clone() {
  rm -rf "${work:?}/$1"
  cp -R "$base" "$work/$1" || exit 1
}

# The mutation cases F and H share, written once. Both strip the `src/` sites that hold a leg
# through its predicates; H first splits the alternatives that cover the leg's suites. They are
# one script because the stripping is subtle enough that two copies would drift, and a selftest
# whose two halves disagree about what "the sites went away" means is worse than no second half.
cat > "$work/mutate.py" <<'PY'
#!/usr/bin/env python3
"""Mutate one fixture:  mutate.py <fixture root> <leg features> <strip|split>

`strip` (case F) blanks every `tokora/src/` site of the predicates this leg ALONE covers, so its
`unique_predicates` claim evaporates and only its suites can still hold it.

`split` (case H) does the same after appending two legs that compile one of the leg's SHARED
suites — one it does not compile alone — with one extra feature each. Which suite, which
features and how many are all derived; nothing here names a leg, a family or a file.

Sites are blanked IN PLACE rather than deleted with the items they gate: offsets and line
numbers stay put, no neighbouring predicate is caught in the truncation, and the gate is a
textual scan that never compiles the fixture, so what "the sites went away" means to it is
exactly that the attribute text is gone.
"""

import ast
import importlib.util
import os
import pathlib
import sys

root, feats, mode = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
gate_path = root / "ci" / "feature_cfg_coverage.py"
os.chdir(root)

_loads = 0


def load():
    """Import the fixture's own copy of the gate. Re-imported after an edit, never reloaded."""
    global _loads
    _loads += 1
    spec = importlib.util.spec_from_file_location(f"gate{_loads}", gate_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def flags_of(g):
    return g.leg_flags(next(l for l in g.EXTRA_LEGS if l["features"] == feats))


def split_alternatives(g):
    """Append two legs that compile a shared suite of `feats`, each adding one feature.

    The construction is what makes the case discriminate, so it is worth stating why it works.
    `base` is the intersection of the features of EVERY leg compiling the chosen suite, so
    `base` is a subset of this leg's own features. The two appended legs are `base` plus one
    distinct feature each, so their intersection is `base` — and the intersection over all the
    other covering legs is therefore inside this leg's features, which is exactly the condition
    under which "is there a feature they all have that I lack" answers no. The leg's protected
    configuration is untouched: neither appended leg has everything it has off.
    """
    fmap = g.feature_map()
    all_legs = g.legs(fmap)
    features_of = {f: ft for f, ft, _t in all_legs}
    test_legs = [(f, ft, t) for f, ft, t in all_legs if t]
    declared = set(fmap)
    flags = flags_of(g)
    mine = features_of[flags]

    shared = []
    for path, gate in sorted(g.scan_suites(g.TESTS).items()):
        if gate is None:
            continue
        node = g.parse(gate)
        if g.feature_names(node, set()) - declared:
            continue
        hits = [f for f, ft, t in test_legs if g.evaluate(node, set(ft), t) >= g.UNKNOWN]
        if flags in hits and len(hits) > 1:
            shared.append((path, hits, node))
    if not shared:
        raise SystemExit(f"selftest: every suite {feats!r} compiles, it compiles alone — there "
                         "are no alternatives to split, so case H is vacuous for this leg")

    path, hits, node = shared[0]
    base = frozenset.intersection(*(features_of[o] for o in hits))
    added = frozenset().union(*(features_of[o] - mine for o in hits if o != flags))
    # Only features that do not pull another candidate: `combinators` implies all thirteen
    # families and `fold` implies `many`, and a leg naming either would not be a one-feature
    # step away from `base`.
    leaves = sorted(f for f in added if not (g.closure([f], fmap) - {f}) & added)

    existing = {ft for _f, ft, t in all_legs if t}
    names = []
    for leaf in leaves:
        want = g.closure(sorted(base | {leaf}), fmap)
        # A synthesised leg that resolves to a leg already declared is not an alternative, it is
        # a duplicate — and two legs with the same features reproduce each other's configuration
        # exactly, which would make the leg it duplicates look deletable for a reason that has
        # nothing to do with what this case is testing.
        if want in existing or g.evaluate(node, want, True) < g.UNKNOWN:
            continue
        names.append(",".join(sorted(want)))
        if len(names) == 2:
            break
    if len(names) < 2:
        raise SystemExit(
            f"selftest: the legs covering {path.name} alongside {feats!r} offer "
            f"{len(names)} usable one-feature alternative(s); two are needed to split them, so "
            "case H is not constructible from this leg"
        )

    text = gate_path.read_text()
    lines = text.splitlines(keepends=True)
    for stmt in ast.parse(text).body:
        if isinstance(stmt, ast.Assign) and any(
            getattr(t, "id", None) == "EXTRA_LEGS" for t in stmt.targets
        ):
            at = stmt.value.elts[-1].end_lineno
            break
    else:
        raise SystemExit("selftest: the fixture gate has no EXTRA_LEGS assignment")
    lines.insert(at, "".join(
        '    {\n'
        f'        "features": "{name}",\n'
        '        "tests": True,\n'
        '        "unique_predicates": False,\n'
        '        "unique_suites": True,\n'
        '        "why": "selftest: one of the split alternatives",\n'
        '    },\n'
        for name in names
    ))
    gate_path.write_text("".join(lines))
    return path.name, names


def strip_sole_sites(g):
    """Blank every `src/` site of the predicates this leg alone covers."""
    fmap = g.feature_map()
    all_legs = g.legs(fmap)
    predicates, _ = g.scan(g.SRC)
    flags = flags_of(g)

    doomed = []
    for pred, sites in predicates.items():
        node = g.parse(pred)
        hits = [f for f, ft, t in all_legs if g.evaluate(node, set(ft), t) >= g.UNKNOWN]
        if hits == [flags]:
            doomed.extend(sites)
    if not doomed:
        raise SystemExit(f"selftest: leg {feats!r} covers no predicate alone; the case is "
                         "vacuous — there is nothing to take away from it")

    by_file = {}
    for site in doomed:
        path, line = site.rsplit(":", 1)
        by_file.setdefault(path, []).append(int(line))

    for path, wanted in by_file.items():
        p = pathlib.Path(path)
        text = p.read_text()
        code, mask = g.lex(text)
        starts = {off for off, _b, _e in g.attributes(code, mask)
                  if code.count("\n", 0, off) + 1 in wanted}
        out = list(text)
        for off, _body, end in g.attributes(code, mask):
            if off in starts:
                for k in range(off, end):
                    if out[k] != "\n":
                        out[k] = " "
        p.write_text("".join(out))
    return len(doomed)


gate = load()
note = ""
if mode == "split":
    suite, added = split_alternatives(gate)
    gate = load()  # EXTRA_LEGS grew; the stripping must see the new leg set
    note = f", after splitting {suite}'s alternatives into {' and '.join(added)}"
elif mode != "strip":
    raise SystemExit(f"selftest: unknown mutation mode {mode!r}")
print(f"{strip_sole_sites(gate)} site(s){note}")
PY

# $1 case name  $2 expected exit (0, or 1 meaning "nonzero")  $3 marker required in the output
# ("" for none)  $4 what to say when the expectation is not met  $5 marker that must NOT appear
# ("" for none)
#
# The forbidden marker is not symmetry for its own sake. Case F's whole subject is a red that
# says the WRONG THING, so an assertion that only demands a red would pass on the very output
# it exists to reject.
run_case() {
  rc_name=$1; rc_want=$2; rc_marker=$3; rc_msg=$4; rc_forbid=${5:-}
  total=$((total + 1))
  ( cd "$work/$rc_name" && python3 "$GATE" ) > "$work/$rc_name.out" 2> "$work/$rc_name.err"
  rc=$?
  cat "$work/$rc_name.out" "$work/$rc_name.err" > "$work/$rc_name.all"
  bad=""
  if [ "$rc_want" = 0 ]; then
    [ "$rc" -eq 0 ] || bad="exit=$rc, expected 0 — $rc_msg"
  else
    [ "$rc" -ne 0 ] || bad="exit=0, expected nonzero — $rc_msg"
  fi
  if [ -n "$rc_marker" ] && ! grep -qF -- "$rc_marker" "$work/$rc_name.all"; then
    bad="$bad; output never mentions '$rc_marker'"
  fi
  if [ -n "$rc_forbid" ] && grep -qF -- "$rc_forbid" "$work/$rc_name.all"; then
    bad="$bad; output says '$rc_forbid', which it must not"
  fi
  if [ -n "$bad" ]; then
    fails=$((fails + 1))
    echo "  FAIL  $rc_name: $bad"
    head -20 "$work/$rc_name.all" | sed 's/^/          /'
  else
    echo "  ok    $rc_name (exit $rc)"
  fi
}

echo "feature-cfg-coverage selftest"
echo "  gate  $root/$GATE"

# ── A. a planted predicate no leg can reach ──────────────────────────────────────────────────
clone plant
cat >> "$work/plant/tokora/src/lib.rs" <<'EOF'

#[cfg(all(feature = "peek", feature = "bstr_1"))]
pub(crate) const SELFTEST_PLANT: () = ();
EOF
run_case plant 1 'feature = "peek"' \
  "a new multi-feature cfg that no leg builds must red and name itself"

# ── B. every load-bearing leg deleted in turn ────────────────────────────────────────────────
legs=$(python3 - "$root/$GATE" <<'PY'
import ast, sys
for node in ast.parse(open(sys.argv[1]).read()).body:
    if isinstance(node, ast.Assign) and any(
        getattr(t, "id", None) == "EXTRA_LEGS" for t in node.targets
    ):
        for elt in node.value.elts:
            d = {k.value: getattr(v, "value", None) for k, v in zip(elt.keys, elt.values)}
            if d.get("unique_predicates"):
                print(d["features"])
PY
)
[ -n "$legs" ] || { echo "  FAIL  drop: no leg declares unique_predicates=True"; fails=$((fails + 1)); }

i=0
for feats in $legs; do
  i=$((i + 1))
  clone "drop$i"
  python3 - "$work/drop$i/$GATE" "$feats" <<'PY'
import ast, sys
path, feats = sys.argv[1], sys.argv[2]
lines = open(path).read().splitlines(keepends=True)
for node in ast.parse("".join(lines)).body:
    if isinstance(node, ast.Assign) and any(
        getattr(t, "id", None) == "EXTRA_LEGS" for t in node.targets
    ):
        for elt in node.value.elts:
            d = {k.value: getattr(v, "value", None) for k, v in zip(elt.keys, elt.values)}
            if d.get("features") == feats:
                del lines[elt.lineno - 1:elt.end_lineno]
                open(path, "w").write("".join(lines))
                raise SystemExit(0)
raise SystemExit(f"selftest: no EXTRA_LEGS entry with features {feats!r}")
PY
  if [ $? -ne 0 ]; then
    echo "  FAIL  drop$i: could not remove leg '$feats'"
    fails=$((fails + 1))
    continue
  fi
  run_case "drop$i" 1 "UNCOVERED" \
    "removing '$feats' must leave a predicate uncovered; if it does not, the leg detects nothing and belongs deleted or marked unique_predicates=False"
done

# ── C. a stale unique_predicates claim ───────────────────────────────────────────────────────
clone stale
python3 - "$work/stale/$GATE" <<'PY'
import re, sys
src = open(sys.argv[1]).read()
out, n = re.subn(r'"unique_predicates": False', '"unique_predicates": True', src, count=1)
if n != 1:
    raise SystemExit(1)
open(sys.argv[1], "w").write(out)
PY
if [ $? -eq 0 ]; then
  run_case stale 1 "unique_predicates=True" \
    "a leg claiming to be the sole cover of a predicate, when it is not, must red"
else
  echo "  skip  stale (no leg declares unique_predicates=False)"
fi

# ── D. the unmodified tree ───────────────────────────────────────────────────────────────────
clone clean
run_case clean 0 "feature-cfg-coverage OK" \
  "the real tree must pass; a gate that reds on everything checks nothing"

# ── E. a plant that IS covered ───────────────────────────────────────────────────────────────
clone covered
cat >> "$work/covered/tokora/src/lib.rs" <<'EOF'

#[cfg(all(feature = "peek", feature = "punct"))]
pub(crate) const SELFTEST_COVERED: () = ();
EOF
run_case covered 0 "feature-cfg-coverage OK" \
  "peek+punct is built by the '--features combinators' leg; reddening on it would be a false positive"

# ── F and H. the predicates that hold a suite-bearing leg, removed ───────────────────────────
#
# Which sites to strip is DERIVED, not listed: the gate is imported and asked which predicates
# each leg alone covers, and every site of those predicates is blanked. A hardcoded line range
# would pin today's `src/trace.rs`, not the property. See `$work/mutate.py` above for the
# mutation itself and for how H's split alternatives are constructed.
suite_legs=$(python3 - "$root/$GATE" <<'PY'
import ast, sys
for node in ast.parse(open(sys.argv[1]).read()).body:
    if isinstance(node, ast.Assign) and any(
        getattr(t, "id", None) == "EXTRA_LEGS" for t in node.targets
    ):
        for elt in node.value.elts:
            d = {k.value: getattr(v, "value", None) for k, v in zip(elt.keys, elt.values)}
            if d.get("unique_predicates") and d.get("unique_suites"):
                print(d["features"])
PY
)
[ -n "$suite_legs" ] || {
  echo "  FAIL  hold: no leg declares both unique_predicates and unique_suites"
  fails=$((fails + 1))
}

i=0
for feats in $suite_legs; do
  i=$((i + 1))
  clone "hold$i"
  stripped=$(python3 "$work/mutate.py" "$work/hold$i" "$feats" strip)
  if [ $? -ne 0 ]; then
    echo "  FAIL  hold$i: could not strip the sites holding '$feats'"
    fails=$((fails + 1))
    continue
  fi
  echo "        hold$i: stripped $stripped holding '$feats'"
  run_case "hold$i" 1 "DO NOT DELETE THE LEG" \
    "'$feats' still compiles integration suites no other leg does; losing its predicates must not produce a deletion recommendation" \
    "Either the predicate it existed for is gone"

  # H, on the same leg. Not every leg can carry it — a leg held because it is the only one
  # compiling its suites at all has no alternatives to split — so `mutate.py` says so and the
  # case is skipped rather than counted, while a leg that CAN carry it and does not refuse is a
  # failure.
  clone "split$i"
  split=$(python3 "$work/mutate.py" "$work/split$i" "$feats" split 2>&1)
  split_rc=$?
  if [ "$split_rc" -ne 0 ]; then
    case "$split" in
      *"is vacuous"*|*"not constructible"*)
        echo "  skip  split$i (${split#selftest: })" ;;
      *)
        echo "  FAIL  split$i: could not split the alternatives covering '$feats' suites"
        echo "$split" | sed 's/^/          /'
        fails=$((fails + 1)) ;;
    esac
  else
    echo "        split$i: stripped $split"
    run_case "split$i" 1 "DO NOT DELETE THE LEG" \
      "with the alternatives split no single feature is common to all of them, but nothing else compiles '$feats' suites with everything it has off; the refusal must survive that" \
      "Either the predicate it existed for is gone"
  fi
done

# ── G. a stale unique_suites claim ───────────────────────────────────────────────────────────
clone stale_suites
python3 - "$work/stale_suites/$GATE" <<'PY'
import re, sys
src = open(sys.argv[1]).read()
out, n = re.subn(r'"unique_suites": False', '"unique_suites": True', src, count=1)
if n != 1:
    raise SystemExit(1)
open(sys.argv[1], "w").write(out)
PY
if [ $? -eq 0 ]; then
  run_case stale_suites 1 "unique_suites=True" \
    "a leg claiming to hold a suite no other leg compiles the same way, when it holds none, must red"
else
  echo "  skip  stale_suites (no leg declares unique_suites=False)"
fi

# ── I. a planted suite no leg compiles ───────────────────────────────────────────────────────
#
# The same `peek` + `bstr_1` pair case A plants as a predicate, planted as a crate-level gate
# instead: a combinator family and a string backing, which nothing pulls together except
# `--all-features`. So the only build of this body would be the one the whole script refuses to
# count, which is what the orphan check is for.
clone orphan
cat > "$work/orphan/tokora/tests/selftest_orphan.rs" <<'EOF'
#![cfg(all(feature = "peek", feature = "bstr_1"))]

//! Planted by ci/feature_cfg_coverage_selftest.sh. Never committed, never compiled.
EOF
run_case orphan 1 "selftest_orphan.rs" \
  "a suite whose crate gate no declared leg satisfies has --all-features as its only build, and must red naming the file"

echo ""
if [ "$fails" -ne 0 ]; then
  echo "feature-cfg-coverage selftest: $fails of $total cases FAILED" >&2
  exit 1
fi
echo "feature-cfg-coverage selftest: $total/$total cases passed"
