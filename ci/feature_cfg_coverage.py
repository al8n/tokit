#!/usr/bin/env python3
"""Coverage gate for MULTI-FEATURE `cfg` predicates — the hole `--each-feature` cannot see.

`cargo hack --each-feature` runs exactly three kinds of leg: `--no-default-features`, each
feature ALONE, and `--all-features`. It never runs a proper subset of size two or more. So an
`all(feature = "A", feature = "B")` predicate is compiled in exactly one leg — `--all-features`
— where a third feature may supply whatever the pair itself was missing. The pair's own
configuration is never built, and the matrix reads as coverage while covering a slice of it.

That is not hypothetical. In downstream smear a `rowan + graphqlx` pair had been failing since
the commit that introduced it, with green CI on every run in between: `rowan` alone did not
compile the macros, `graphqlx` alone did not compile the substrate, and `--all-features` also
had `graphql`, which supplied the consumers whose absence was the bug.

The reach for `--feature-powerset --depth 2` is the wrong trade at 40 features: ~780
configurations on a CI that already spends hours on the Miri queue, which buys a gate nobody
keeps. So the legs are derived from the `cfg`s THAT ACTUALLY EXIST instead of from the feature
list, and this script is what keeps that derivation honest — a hardcoded leg list is a
snapshot, and the next `all(...)` predicate someone writes would be uncovered again with green
CI.

WHAT IT DOES

  1. Enumerates every `cfg` predicate in `tokora/src/` naming two or more features. The scan is
     ATTRIBUTE-BASED and multi-line aware — a line-oriented grep misses `#[cfg(all(` wrapped
     over five lines by rustfmt, and this crate has 100+ of those. When the enumeration for
     issue #200 was first done with a line grep it reported 25 predicates over 210 sites; the
     attribute scan finds 29 over 317, and one predicate only the attribute scan can see
     (`all(test, trace, any(logos_*), std)`, 3 sites) is reachable from no `--each-feature`
     leg at all.
  2. Expands the leg set: the `--each-feature` legs, resolved through the feature graph from
     `cargo metadata`, plus `EXTRA_LEGS` below.
  3. Evaluates every predicate against every leg and fails, naming the predicate and a site,
     when no leg satisfies one.

`--all-features` IS DELIBERATELY NOT A COVERING LEG. It satisfies every satisfiable predicate
by construction, so counting it would make this script pass unconditionally — it would be the
very defect it exists to detect. Excluding it is what makes the question "does some leg build
this pair ON ITS OWN" instead of "is this pair satisfiable at all".

EVALUATION IS THREE-VALUED, and deliberately generous on everything that is not a feature.
A leaf is TRUE/FALSE when the leg decides it (`feature = "x"`), and UNKNOWN otherwise
(`debug_assertions`, `target_has_atomic`, `docsrs`, `miri`, ...). A predicate counts as covered
by a leg when the leg does not make it definitely FALSE. This gate is about FEATURE
combinations; a predicate that also wants a debug profile or an atomic target is not this
script's business, and treating those leaves as false would manufacture uncoverable predicates
and train everyone to ignore the output. `test` is the one non-feature leaf that is decided,
because it is decided by the leg: FALSE without `--tests`, UNKNOWN with it (a `--tests` leg
builds both the plain lib and the lib test harness, so it reaches `test` and `not(test)` both).

BOUNDS, stated so a green is not read as more than it is:

  * `tokora/src/` only, matching the issue's own enumeration. Integration tests under
    `tokora/tests/` are not scanned.
  * A leg satisfying a predicate means that CONFIGURATION gets compiled. It does not mean the
    gated code is executed, and it does not mean the predicate is the only thing standing
    between the code and a consumer.
  * The scan is textual, over comment- and string-stripped source. A `#[cfg(...)]` produced by
    a macro from tokens that never appear literally is invisible to it. A false positive (a
    predicate that is not real) reds and asks for a leg; a false negative hides one. The
    stripper exists so the first stays rare, and the failure direction of the second is why the
    positive controls below are not optional.

Usage:
    python3 ci/feature_cfg_coverage.py              # the gate; exit 1 names what is uncovered
    python3 ci/feature_cfg_coverage.py --print-legs # the declared extra legs, one per line

`--print-legs` is why `EXTRA_LEGS` is the single source of truth: CI does not repeat the leg
list, it asks this script for it and runs what it prints. A workflow with its own copy of the
list is a workflow that can drift from the script that validates it, and then the gate is
checking a set nobody builds.
"""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys

# ── The declared extra legs ──────────────────────────────────────────────────────────────────
#
# Derived from the predicates that exist, not from the feature list. Every leg runs
# `--no-default-features` so the set under test is exactly the set named; `tests` adds
# `--tests`, which is what puts `cfg(test)` in reach.
#
# `unique_predicates` is CHECKED, not documentation. It states whether some multi-feature
# predicate is covered by this leg and by no other, and `main()` fails when the claim and the
# tree disagree in either direction. That is what stops the list accreting legs whose reason
# has quietly evaporated — a leg that can no longer detect anything is the same defect as a
# gate that cannot fail, one level down.
#
# `--no-default-features --features default` IS one of the `--each-feature` legs, and
# `default = ["std", "combinators"]` pulls `std` plus all thirteen combinator families. That
# single fact disposes of most conjunctions between a family and `std`/`alloc`, and it is the
# reason two of the three legs originally derived for #200 turned out not to be needed.
EXTRA_LEGS = [
    {
        "features": "fold,alloc",
        "tests": False,
        "unique_predicates": False,
        # Kept for the CONFIGURATION, not for a predicate: it is the only leg that compiles the
        # `many`/`fold` family at the alloc-WITHOUT-std tier. `--features many` and
        # `--features fold` have no allocator; `--features alloc`, `conformance` and
        # `smallvec_1` have no family; `default` has the family but brings `std` with it. So
        # `src/parser/many/list.rs` and the `list`/`separated1` sinks — gated
        # `all(many, any(alloc, std))` — are compiled by no other leg without `std`, and a
        # `std`-only path leaking into them would be invisible.
        #
        # `fold = ["many"]` is the crate's ONE inter-family edge (the other twelve families are
        # `[]`), so naming `fold` reaches both families for the price of one leg.
        #
        # It is NOT predicate-load-bearing: the `default` leg already satisfies
        # `all(many, any(alloc, std))` and `all(fold, any(alloc, std))`.
        "why": "only leg building the many/fold family at the alloc-without-std tier",
    },
    {
        "features": "std,logos,combinators",
        "tests": True,
        "unique_predicates": True,
        # The leg that carries the real hole: seven predicates over 31 sites, four of them
        # reachable from no other leg at all and the rest only from the `trace` leg below.
        # Every one wants a logos version AND `std` at once, and no feature supplies both —
        # `logos_0_16` alone has no `std`, `std` alone has no lexer, and `default` has `std`
        # and the families but still no lexer.
        #
        # NAMES `logos`, NOT `logos_0_16`, and the difference is load-bearing.
        # `logos = ["logos_0_16"]`, and `cfg(feature = "...")` matches the LITERAL feature name
        # — so `--features logos_0_16` satisfies `all(std, logos_0_16, combinators)` but NOT
        # `all(test, logos, std, combinators)`. Naming `logos` satisfies both, because it pulls
        # `logos_0_16` transitively. Getting this backwards produces a leg that looks right and
        # covers one of the two.
        "why": "7 logos+std predicates over 31 sites, incl. all(std, logos_0_16, combinators)",
    },
    {
        "features": "std,logos,trace",
        "tests": True,
        "unique_predicates": True,
        # `trace = ["std"]` and nothing pulls `trace`, so `trace` alone has no logos and no leg
        # that has logos has `trace`. This is the leg the derivation for #200 did not have: its
        # predicate is rustfmt-wrapped over five lines in `src/trace.rs` and
        # `src/parser/labelled.rs`, and the line-oriented grep that derivation used cannot see
        # a wrapped attribute. It is also the leg with the most to say — `trace` instruments the
        # crate's own combinators at ~40 `#[cfg(feature = "trace")]` sites, and its own header
        # in `src/trace.rs` calls out the `trace`-without-`logos` test build as the shape to
        # watch.
        #
        # `combinators` is deliberately absent: `src/trace.rs` and `src/parser/labelled.rs` are
        # ungated, so this builds the trace surface without the families masking it.
        "why": "all(test, trace, any(logos_0_16, logos_0_15, logos_0_14), std)",
    },
]

CRATE = "tokora"
SRC = pathlib.Path(CRATE) / "src"

TRUE, UNKNOWN, FALSE = 1, 0, -1


# ── Source scanning ──────────────────────────────────────────────────────────────────────────

def lex(text: str):
    """`(code, in_string)` — comments and char literals blanked, string literals masked.

    Two products because the two hazards pull opposite ways. Comments must be BLANKED out of
    the text, or a `// why` inside a rustfmt-wrapped `#[cfg(all(` lands in the middle of a
    predicate. String literals must be KEPT in the text, because `feature = "std"` is a string
    literal and blanking it erases the very thing being enumerated — but a `#[cfg(...)]` that
    only exists inside a string is not a real gate, so their spans are masked and the attribute
    scanner refuses to start an attribute, or to count a bracket, inside one.

    Offsets are preserved throughout so a reported line number is the real one. Rust's block
    comments nest, so the depth counter is not decoration. Lifetimes (`'a`) and char literals
    (`'x'`) share a sigil, so a `'` is only consumed when it actually closes.
    """
    out: list[str] = []
    mask = bytearray(len(text))
    i, n = 0, len(text)

    def blank(s: str) -> str:
        return "".join(c if c == "\n" else " " for c in s)

    while i < n:
        c = text[i]
        if c == "/" and text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(blank(text[i:j]))
            i = j
        elif c == "/" and text.startswith("/*", i):
            depth, j = 0, i
            while j < n:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                    if depth == 0:
                        break
                else:
                    j += 1
            out.append(blank(text[i:j]))
            i = j
        elif (c in "rb" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_"))
              and (m := re.match(r'(?:br|rb|r|b)(#*)"', text[i:]))):
            # raw / byte string: r"...", r#"..."#, b"...", br#"..."#
            hashes = m.group(1)
            if hashes:
                close = '"' + hashes
                j = text.find(close, i + m.end())
                j = n if j < 0 else j + len(close)
            else:
                j = _end_of_quoted(text, i + m.end())
            out.append(text[i:j])
            mask[i:j] = b"\x01" * (j - i)
            i = j
        elif c == '"':
            j = _end_of_quoted(text, i + 1)
            out.append(text[i:j])
            mask[i:j] = b"\x01" * (j - i)
            i = j
        elif c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                out.append(blank(m.group(0)))
                i += m.end()
            else:  # a lifetime
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out), mask


def _end_of_quoted(text: str, i: int) -> int:
    """Index just past the closing `"` of a non-raw string whose body starts at `i`."""
    n = len(text)
    while i < n:
        if text[i] == "\\":
            i += 2
        elif text[i] == '"':
            return i + 1
        else:
            i += 1
    return n


def attributes(text: str, mask: bytearray):
    """Yield `(offset, body)` for every `#[...]` / `#![...]`, bracket-balanced and multi-line.

    A `#` inside a string literal never opens an attribute, and a bracket inside one never
    counts toward the balance — so `#[doc = "a ] b"]` closes where it really closes, and a
    `#[cfg(...)]` quoted inside a fixture string is not mistaken for a gate.
    """
    i, n = 0, len(text)
    while (i := text.find("#", i)) >= 0:
        if mask[i]:
            i += 1
            continue
        j = i + 1
        if j < n and text[j] == "!":
            j += 1
        if j >= n or text[j] != "[":
            i += 1
            continue
        depth, k = 0, j
        while k < n:
            if not mask[k]:
                if text[k] == "[":
                    depth += 1
                elif text[k] == "]":
                    depth -= 1
                    if depth == 0:
                        break
            k += 1
        if k >= n:
            return
        yield i, text[j + 1:k].strip()
        i = k + 1


def split_top(s: str):
    """Split on top-level commas."""
    parts, depth, start = [], 0, 0
    for idx, ch in enumerate(s):
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif ch == "," and depth == 0:
            parts.append(s[start:idx])
            start = idx + 1
    parts.append(s[start:])
    return [p.strip() for p in parts if p.strip()]


def predicate_of(body: str) -> str | None:
    """The compile-gating predicate an attribute body carries, if any.

    `cfg(P)` gives `P`. `cfg_attr(P, ...)` gives `P` — it gates whether the inner attributes
    apply, which for `derive(...)` is a real compilation difference. The `doc(cfg(...))` inside
    `cfg_attr(docsrs, doc(cfg(P)))` is NOT returned: it is documentation metadata, inert outside
    docs.rs, and counting it would inflate the predicate set with entries no leg needs to build.
    (Issue #195 is what happens when those are wrong; it is a different gate.)
    """
    if body.startswith("cfg(") and body.endswith(")"):
        return body[4:-1].strip()
    if body.startswith("cfg_attr(") and body.endswith(")"):
        parts = split_top(body[9:-1])
        return parts[0] if parts else None
    return None


def scan(root: pathlib.Path):
    """`{normalised predicate: [site, ...]}` for every predicate naming two or more features."""
    found: dict[str, list[str]] = {}
    files = 0
    for path in sorted(root.rglob("*.rs")):
        files += 1
        text, mask = lex(path.read_text())
        for off, body in attributes(text, mask):
            pred = predicate_of(body)
            if pred is None:
                continue
            norm = re.sub(r"\s+", " ", pred).strip()
            if len(re.findall(r'\bfeature\s*=\s*"', norm)) < 2:
                continue
            line = text.count("\n", 0, off) + 1
            found.setdefault(norm, []).append(f"{path}:{line}")
    return found, files


# ── Predicate parsing and three-valued evaluation ────────────────────────────────────────────

TOKEN = re.compile(r'\s*(?:([A-Za-z_][A-Za-z0-9_-]*)|("(?:[^"\\]|\\.)*")|([(),=]))')


def parse(pred: str):
    pos = 0

    def tok():
        nonlocal pos
        m = TOKEN.match(pred, pos)
        if not m:
            return None
        pos = m.end()
        return m.group(1) or m.group(2) or m.group(3)

    def peek():
        m = TOKEN.match(pred, pos)
        return (m.group(1) or m.group(2) or m.group(3)) if m else None

    def expr():
        name = tok()
        if name is None:
            raise ValueError(f"empty predicate in {pred!r}")
        if name in ("all", "any", "not") and peek() == "(":
            tok()
            args = []
            while peek() != ")":
                args.append(expr())
                if peek() == ",":
                    tok()
            tok()
            if name == "not":
                if len(args) != 1:
                    raise ValueError(f"not() takes one argument in {pred!r}")
                return ("not", args[0])
            return (name, args)
        if peek() == "=":
            tok()
            value = tok()
            return ("leaf", name, json.loads(value) if value and value[0] == '"' else value)
        return ("leaf", name, None)

    node = expr()
    if TOKEN.match(pred, pos):
        raise ValueError(f"trailing tokens in {pred!r}")
    return node


def evaluate(node, features: set[str], tests: bool) -> int:
    kind = node[0]
    if kind == "all":
        return min((evaluate(c, features, tests) for c in node[1]), default=TRUE)
    if kind == "any":
        return max((evaluate(c, features, tests) for c in node[1]), default=FALSE)
    if kind == "not":
        return -evaluate(node[1], features, tests)
    _, name, value = node
    if name == "feature":
        return TRUE if value in features else FALSE
    if name == "test":
        return UNKNOWN if tests else FALSE
    return UNKNOWN


def feature_names(node, acc: set[str]) -> set[str]:
    if node[0] in ("all", "any"):
        for c in node[1]:
            feature_names(c, acc)
    elif node[0] == "not":
        feature_names(node[1], acc)
    elif node[1] == "feature" and node[2] is not None:
        acc.add(node[2])
    return acc


# ── The feature graph ────────────────────────────────────────────────────────────────────────

def feature_map() -> dict[str, list[str]]:
    """The crate's declared features, from `cargo metadata` rather than a TOML re-parse.

    Authoritative by construction: it is the same resolution `cargo hack` reads to decide which
    legs `--each-feature` expands to, so the leg set here cannot disagree with the leg set CI
    actually runs.
    """
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True, capture_output=True, text=True,
    ).stdout
    for pkg in json.loads(out)["packages"]:
        if pkg["name"] == CRATE:
            return pkg["features"]
    raise SystemExit(f"feature-cfg-coverage: no package named {CRATE!r} in cargo metadata")


def closure(seed, fmap: dict[str, list[str]]) -> set[str]:
    """Transitively enable `seed` through the crate's own feature edges."""
    out: set[str] = set()
    stack = list(seed)
    while stack:
        feat = stack.pop()
        if feat in out:
            continue
        if feat not in fmap:
            raise SystemExit(
                f"feature-cfg-coverage: leg names {feat!r}, which is not a {CRATE} feature"
            )
        out.add(feat)
        for dep in fmap[feat]:
            if dep.startswith("dep:"):
                continue
            if "/" in dep:
                # `pkg/feat` also enables the same-named feature of this crate when one exists;
                # `pkg?/feat` never does.
                pkg = dep.split("/", 1)[0]
                if not pkg.endswith("?") and pkg in fmap:
                    stack.append(pkg)
                continue
            stack.append(dep)
    return out


def leg_flags(leg) -> str:
    flags = f"--no-default-features --features {leg['features']}"
    return flags + " --tests" if leg["tests"] else flags


def legs(fmap: dict[str, list[str]]):
    """Every leg that counts as coverage: the `--each-feature` legs, then `EXTRA_LEGS`.

    `--all-features` is absent on purpose; see the module docstring. `default` is NOT absent —
    `cargo hack --each-feature` really does run `--no-default-features --features default`, and
    pretending otherwise would invent gaps that CI already covers. The `--each-feature` legs
    carry `tests=True` because the workflow's matrix is
    `cargo hack clippy -p tokora --each-feature --no-deps --tests`.
    """
    out = [("--no-default-features", frozenset(), True)]
    for feat in sorted(fmap):
        out.append((f"--no-default-features --features {feat}",
                    frozenset(closure([feat], fmap)), True))
    for leg in EXTRA_LEGS:
        names = [f.strip() for f in leg["features"].split(",") if f.strip()]
        out.append((leg_flags(leg), frozenset(closure(names, fmap)), leg["tests"]))
    return out


# ── Entry points ─────────────────────────────────────────────────────────────────────────────

def print_legs() -> int:
    for leg in EXTRA_LEGS:
        print(leg_flags(leg))
    return 0


def main() -> int:
    if not SRC.is_dir():
        print(f"feature-cfg-coverage: {SRC} does not exist — run from the repository root",
              file=sys.stderr)
        return 1

    fmap = feature_map()
    all_legs = legs(fmap)
    predicates, files = scan(SRC)

    # ── Positive controls ────────────────────────────────────────────────────────────────────
    # A check that cannot fail is not a check, and this one's whole subject is gates that pass
    # by not looking. Each of these turns a silently empty extraction — a moved source tree, a
    # regex that stopped matching, a feature table that failed to parse — into a red that names
    # itself, instead of a green that means "found nothing".
    problems = []
    if files == 0:
        problems.append(f"scanned 0 files under {SRC}")
    if not predicates:
        problems.append(f"found 0 multi-feature cfg predicates under {SRC}")
    if len(all_legs) < 2:
        problems.append(f"expanded {len(all_legs)} legs; the feature table did not parse")
    if not EXTRA_LEGS:
        problems.append("EXTRA_LEGS is empty; nothing declares the pair legs CI runs")
    if problems:
        for p in problems:
            print(f"feature-cfg-coverage: {p}", file=sys.stderr)
        return 1

    declared = set(fmap)
    uncovered = []
    unknown_features = []
    rows = []
    covering: dict[str, list[str]] = {}
    for pred in sorted(predicates):
        node = parse(pred)
        named = feature_names(node, set())
        strays = sorted(named - declared)
        if strays:
            unknown_features.append((pred, strays, predicates[pred][0]))
            continue
        hits = [flags for flags, feats, tests in all_legs
                if evaluate(node, set(feats), tests) >= UNKNOWN]
        covering[pred] = hits
        if not hits:
            uncovered.append((pred, predicates[pred]))
        else:
            rows.append((len(predicates[pred]), pred, hits[0]))

    if unknown_features:
        for pred, strays, site in unknown_features:
            print(f"feature-cfg-coverage: {site}: cfg names {', '.join(strays)}, which "
                  f"{'are' if len(strays) > 1 else 'is'} not a {CRATE} feature — the predicate "
                  f"can never be true", file=sys.stderr)
        return 1

    if uncovered:
        print("feature-cfg-coverage: UNCOVERED multi-feature cfg predicates", file=sys.stderr)
        print("", file=sys.stderr)
        for pred, sites in uncovered:
            print(f"  cfg({pred})", file=sys.stderr)
            print(f"    {len(sites)} site(s), e.g. {sites[0]}", file=sys.stderr)
            print("", file=sys.stderr)
        print("No leg of `cargo hack --each-feature` builds a configuration satisfying "
              f"{'these' if len(uncovered) > 1 else 'this'}, and `--all-features` does not "
              "count: it satisfies everything, which is the blind spot this gate exists for "
              "(#200).", file=sys.stderr)
        print("", file=sys.stderr)
        print("Fix: add a leg to EXTRA_LEGS in ci/feature_cfg_coverage.py naming the smallest "
              "feature set that satisfies the predicate (plus `\"tests\": True` when it names "
              "`test`). CI runs whatever that list prints.", file=sys.stderr)
        return 1

    # ── The declaration self-check ───────────────────────────────────────────────────────────
    # `unique_predicates` is an assertion about the tree, so it is verified against the tree. A
    # leg that claims to be the only one reaching a predicate, and is not, is a leg whose reason
    # has evaporated — delete it, or fix the claim. A leg that quietly BECAME the only one
    # reaching a predicate is the more dangerous direction: nothing would say so, and the next
    # person pruning "redundant" legs would delete real coverage.
    sole: dict[str, list[str]] = {}
    for pred, hits in covering.items():
        if len(hits) == 1:
            sole.setdefault(hits[0], []).append(pred)
    drift = []
    for leg in EXTRA_LEGS:
        flags = leg_flags(leg)
        mine = sole.get(flags, [])
        if leg["unique_predicates"] and not mine:
            drift.append(
                f"leg `{flags}` declares unique_predicates=True, but every predicate it "
                f"satisfies is also satisfied by another leg. Either the predicate it existed "
                f"for is gone — delete the leg — or the claim is wrong."
            )
        if not leg["unique_predicates"] and mine:
            drift.append(
                f"leg `{flags}` declares unique_predicates=False, but it is now the only leg "
                f"satisfying: {'; '.join('cfg(' + p + ')' for p in mine)}. Set it to True so "
                f"nobody prunes it as redundant."
            )
    if drift:
        for d in drift:
            print(f"feature-cfg-coverage: {d}", file=sys.stderr)
        return 1

    # A green says what it looked at. A bare "OK" from a coverage gate is indistinguishable
    # from a coverage gate that found nothing to look at.
    print(f"feature-cfg-coverage OK: {len(predicates)} multi-feature cfg predicates over "
          f"{sum(len(v) for v in predicates.values())} sites in {files} files, "
          f"{len(all_legs)} covering legs ({len(EXTRA_LEGS)} declared, --all-features excluded)")
    print("")
    print("declared legs:")
    for leg in EXTRA_LEGS:
        flags = leg_flags(leg)
        mine = sole.get(flags, [])
        tag = (f"sole cover of {len(mine)} predicate{'' if len(mine) == 1 else 's'}"
               if mine else "no predicate needs it alone")
        print(f"  {flags}")
        print(f"      {tag} — {leg['why']}")
    print("")
    for count, pred, hit in sorted(rows, key=lambda r: (-r[0], r[1])):
        print(f"  {count:4d}  cfg({pred})")
        print(f"        by  {hit}")
    return 0


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--print-legs":
        raise SystemExit(print_legs())
    if len(sys.argv) > 1:
        raise SystemExit(f"usage: {sys.argv[0]} [--print-legs]")
    raise SystemExit(main())
