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
     attribute scan found 29 over 317 on that same tree, and one predicate only the attribute
     scan can see (`all(test, trace, any(logos_*), std)`) is reachable from no `--each-feature`
     leg at all. Those two figures date the comparison; the LIVE counts are printed on the
     green path, which is the only place a count in this file is allowed to live.
  2. Enumerates the crate-level `#![cfg(...)]` gate of every integration suite in
     `tokora/tests/` — the whole-file gate that decides whether a suite's BODY is type-checked
     at all. A different question from (1), asked for a different reason: see WHY THE SUITES
     ARE SCANNED below.
  3. Expands the leg set: the `--each-feature` legs, resolved through the feature graph from
     `cargo metadata`, plus `EXTRA_LEGS` below.
  4. Evaluates every predicate against every leg and fails, naming the predicate and a site,
     when no leg satisfies one — then checks each declared leg's `unique_predicates` and
     `unique_suites` claims against what the tree actually says.

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

WHY THE SUITES ARE SCANNED, when the predicate enumeration is `tokora/src/` only

A leg gets deleted when nothing needs it, so what counts as "needs it" is the whole safety of
the list. Until 2026-08-11 it meant exactly one thing: some `src/` predicate this leg alone
satisfies. The `std,logos,trace` leg had one such predicate over three sites — and something
else entirely riding on it. A group of integration suites carries the crate-level gate
`all(std, any(logos_0_16, logos_0_15, logos_0_14))` with no family and no umbrella — eight of
them on 2026-08-11, and the green path prints the live figure — and that leg is the only
configuration in CI that type-checks their bodies with `combinators` OFF. Nothing derived that.
It was a paragraph in the comment below.

So removing those three `trace` sites — an ordinary bit of test cleanup — would have made this
script print `delete the leg`, and following its own instruction would have dropped the only
combinators-off compile of eight suite bodies with every gate still green. The gate would have
been correct about its stated scope and wrong about what it protected.

`unique_suites` closes it. It is derived from the suite gates the same way `unique_predicates`
is derived from the `src/` ones, checked in both directions the same way, and consulted BEFORE
any deletion advice is printed: a leg still holding a suite is never recommended for deletion,
whatever became of its predicates.

"Holding" is not "sole compile", and the difference is the whole finding. TWO legs compile
those eight suites — `std,logos,combinators` does too — so a sole-compile test says nobody
needs the `trace` leg and recommends deleting it all over again, one layer down. What is
unique is not that the leg compiles them but WITHOUT WHAT: it is the only leg that compiles
them with the thirteen combinator families off, the configuration in which a family-only
helper leaking into a suite would show up. So the derived quantity is, for each suite a leg
compiles, the features every OTHER leg compiling that suite turns on and this one does not.
Non-empty means deleting the leg retires a configuration nothing else provides. Nothing names
`combinators`; it falls out.

BOUNDS, stated so a green is not read as more than it is:

  * The PREDICATE enumeration is `tokora/src/` only, matching the issue's own enumeration.
    Files under `tokora/tests/` are read for their crate-level `#![cfg(...)]` gate and nothing
    else — the `cfg`s inside a suite body are not enumerated, so "this leg compiles the suite"
    means the body is type-checked in that configuration, not that every branch inside it is.
  * Suite coverage is measured over the same legs as predicate coverage, `--all-features`
    excluded for the same reason. A suite no declared leg compiles is REPORTED by name on the
    green path, not failed: the ones there today are gated on `rowan`, `--all-features` is
    genuinely their only build, and reddening on them would be asking for legs the argument
    above rejects.
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
# `unique_suites` is the same assertion one level out: whether some integration suite under
# `tokora/tests/` is compiled by this leg in a configuration no other leg reproduces — meaning
# with some feature OFF that every other leg compiling that suite has on. It is checked both
# ways too, and it is the reason a leg is never recommended for deletion on the strength of its
# predicates alone — see WHY THE SUITES ARE SCANNED in the module docstring.
#
# NO LIVE COUNT IS WRITTEN IN THIS FILE. Every "N predicates over M sites" this list used to
# state is derived and printed on the green path instead, because the one that was written down
# went stale inside three commits and was read past by everyone, including a release. The
# figures that remain describe a dated observation and are written to say so.
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
        # No `--tests`, so it builds no integration target and can hold no suite.
        "unique_suites": False,
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
        "unique_suites": True,
        # The leg that carries the real hole: the logos+std predicates no `--each-feature` leg
        # reaches, most of them reachable from no other leg at all and the rest only from the
        # `trace` leg below. Every one wants a logos version AND `std` at once, and no feature
        # supplies both — `logos_0_16` alone has no `std`, `std` alone has no lexer, and
        # `default` has `std` and the families but still no lexer.
        #
        # This sentence used to carry the figures — "seven predicates over 31 sites, four of
        # them". They were right when written at `f475643`, became 6/31/3 at `ec337c8` when a
        # predicate stopped being distinct, and rode through `cd5fa11` — a release whose own
        # subject was which configurations compile what — unrecomputed, because nothing
        # recomputes a comment. The `31 sites` half stayed right, which is how the sentence
        # survived being read. The figures are printed by the green path now.
        #
        # NAMES `logos`, NOT `logos_0_16`, and the difference is load-bearing.
        # `logos = ["logos_0_16"]`, and `cfg(feature = "...")` matches the LITERAL feature name
        # — so `--features logos_0_16` satisfies `all(std, logos_0_16, combinators)` but NOT
        # `all(test, logos, std, combinators)`. Naming `logos` satisfies both, because it pulls
        # `logos_0_16` transitively. Getting this backwards produces a leg that looks right and
        # covers one of the two.
        "why": "the logos+std predicates, incl. all(std, logos_0_16, combinators)",
    },
    {
        "features": "std,logos,trace",
        "tests": True,
        "unique_predicates": True,
        "unique_suites": True,
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
        #
        # THAT ABSENCE DOES A SECOND JOB THIS SCRIPT CANNOT SEE, and it is recorded here because
        # the only place it is visible is the leg itself. Eight integration suites carry the
        # crate-level gate `all(std, any(logos_0_16, logos_0_15, logos_0_14))` with no family and
        # no umbrella — `boost`, `error_extra`, `forwarding_discrimination`, `input_ref`,
        # `lexer_error_paths`, `recovery_terminal_stop`, `session_points`, `sync`. No
        # `--each-feature` leg supplies `std` AND a logos version at once, and every other
        # configuration that does has `combinators` too — every `--all-features` job (`test`,
        # `test (release)`, `msrv`, `sanitizer`, `coverage`), `test (valve-off)` and the Miri
        # matrices (which name `--features logos` WITHOUT `--no-default-features`, so `default`
        # brings the umbrella), the logos-parity matrix (which names it outright), and the
        # `std,logos,combinators` leg above. So THIS is the only
        # configuration in CI that type-checks those eight bodies with the families off, which is
        # the configuration in which a family-only helper leaking into them would show up. One of
        # the eight already depends on it: `forwarding_discrimination` gates imports, `From`
        # impls, enum variants and whole test cases on `#[cfg(feature = "pratt")]` and
        # `#[cfg(feature = "map")]`, so this is the only leg that compiles the file with those
        # items ABSENT — and its own line 101 says as much, calling an import gate the thing that
        # "keeps the `std,logos,trace` leg warning-free". Measured on 2026-08-08 at `ec337c8` by
        # planting two `compile_error!`s in `tests/boost.rs`: an unconditional one fired under
        # this leg (the body is compiled) and a `#[cfg(feature = "combinators")]` one did not
        # (the umbrella is off).
        #
        # That coverage USED to be incidental. Nothing derived it — the scan read `tokora/src/`
        # only — and what held the leg in place was its `unique_predicates` claim over the
        # `trace` sites named in `why`. Deleting those sites, which is ordinary test cleanup,
        # made this script print `delete the leg`; doing as it said would have dropped the only
        # combinators-off compile of the eight suites above, green all the way. Reproduced on
        # 2026-08-11 by truncating both files at the gated items and running the drift check.
        #
        # It is derived now: `unique_suites` above is computed from the crate-level gates in
        # `tokora/tests/`, checked in both directions, and the deletion advice is suppressed for
        # any leg that still holds a suite. This paragraph is the history, not the mechanism.
        "why": "all(test, trace, any(logos_0_16, logos_0_15, logos_0_14), std)",
    },
]

CRATE = "tokora"
SRC = pathlib.Path(CRATE) / "src"
TESTS = pathlib.Path(CRATE) / "tests"

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
    """Yield `(offset, body, end)` for every `#[...]` / `#![...]`, balanced and multi-line.

    `offset` is the `#`, `end` is just past the closing `]` — `crate_gate` needs the end to
    tell an attribute prologue from an attribute that merely follows something.

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
        yield i, text[j + 1:k].strip(), k + 1
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
        for off, body, _end in attributes(text, mask):
            pred = predicate_of(body)
            if pred is None:
                continue
            norm = re.sub(r"\s+", " ", pred).strip()
            if len(re.findall(r'\bfeature\s*=\s*"', norm)) < 2:
                continue
            line = text.count("\n", 0, off) + 1
            found.setdefault(norm, []).append(f"{path}:{line}")
    return found, files


def crate_gate(text: str, mask: bytearray) -> str | None:
    """The conjunction of a file's crate-level `#![cfg(...)]`s, or `None` when it has none.

    Every `*.rs` directly under `tokora/tests/` is a `cargo` integration-test target, and the
    inner attributes at the top of one decide whether its whole body is type-checked. Only the
    PROLOGUE is read: attributes separated from the start of the file, and from each other, by
    nothing but whitespace. Comments are already blanked to spaces by `lex`, so a doc header
    between two `#![...]` does not end the prologue, while the first item does — which is what
    keeps an inner attribute written inside a `mod m { #![cfg(...)] }` further down the file
    from being mistaken for a gate on the whole target.
    """
    parts, pos = [], 0
    for off, body, end in attributes(text, mask):
        if text[pos:off].strip() or text[off:off + 2] != "#!":
            break
        pred = predicate_of(body)
        if pred is not None:
            parts.append(re.sub(r"\s+", " ", pred).strip())
        pos = end
    if not parts:
        return None
    return parts[0] if len(parts) == 1 else "all(" + ", ".join(parts) + ")"


def scan_suites(root: pathlib.Path):
    """`{suite path: crate-level gate or None}` for every integration-test target under `root`.

    `glob("*.rs")` and not `rglob`: `tests/common/` and `tests/ui/` are a shared module and a
    trybuild fixture tree, compiled as part of a target rather than as one.
    """
    return {path: crate_gate(*lex(path.read_text())) for path in sorted(root.glob("*.rs"))}


def load_bearing_suites(suite_hits, features_of):
    """`{leg flags: {suite name: features only this leg has OFF}}` — what a leg alone protects.

    "Sole compile" is the wrong question and answers itself: every leg builds a suite under a
    feature set no other leg has, so every leg would be sole something. The question that has
    content is what a leg is the only place to see WITHOUT. For each suite and each leg
    compiling it, this is the set of features that every OTHER leg compiling that suite turns
    on and this one does not: delete the leg and the suite's body is never again type-checked
    with those features absent, and a helper from one of them leaking into the suite stops
    being visible anywhere.

    The suites that motivated this are exactly that shape. `std,logos,combinators --tests` and
    `std,logos,trace --tests` both compile them, so neither is a sole compile — but the `trace`
    leg is the only one that compiles them with the thirteen combinator families OFF, which is
    the whole reason it is declared. An empty set means every absence this leg offers is
    offered by another leg too. A suite no other leg compiles at all maps to an empty set as
    well, and is reported separately: nothing needs saying about which features are off when
    the alternative is not compiling the suite.
    """
    out: dict[str, dict[str, frozenset]] = {}
    for path, hits in suite_hits.items():
        for flags in hits:
            others = [features_of[o] for o in hits if o != flags]
            if not others:
                out.setdefault(flags, {})[path.name] = frozenset()
                continue
            missing = frozenset.intersection(*others) - features_of[flags]
            if missing:
                out.setdefault(flags, {})[path.name] = missing
    return out


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
    for path in (SRC, TESTS):
        if not path.is_dir():
            print(f"feature-cfg-coverage: {path} does not exist — run from the repository root",
                  file=sys.stderr)
            return 1

    fmap = feature_map()
    all_legs = legs(fmap)
    predicates, files = scan(SRC)
    suites = scan_suites(TESTS)

    # ── Positive controls ────────────────────────────────────────────────────────────────────
    # A check that cannot fail is not a check, and this one's whole subject is gates that pass
    # by not looking. Each of these turns a silently empty extraction — a moved source tree, a
    # regex that stopped matching, a feature table that failed to parse — into a red that names
    # itself, instead of a green that means "found nothing".
    #
    # The two suite controls are not ceremony. A suite scan that quietly returns nothing makes
    # every `unique_suites=True` claim fail, which reads as "the leg is dead, delete it" — the
    # exact instruction this scan was added to prevent, arrived at from the other side.
    problems = []
    if files == 0:
        problems.append(f"scanned 0 files under {SRC}")
    if not predicates:
        problems.append(f"found 0 multi-feature cfg predicates under {SRC}")
    if not suites:
        problems.append(f"found 0 integration-test targets under {TESTS}")
    if suites and not any(g for g in suites.values()):
        problems.append(f"found 0 crate-level `#![cfg(...)]` gates over {len(suites)} suites "
                        f"under {TESTS}; the prologue scan stopped matching")
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

    # Which legs compile which suite BODY. Only a leg carrying `--tests` builds an integration
    # target at all, so a leg without it can never be a suite's compile, sole or otherwise.
    test_legs = [(flags, feats, t) for flags, feats, t in all_legs if t]
    suite_hits: dict[pathlib.Path, list[str]] = {}
    for path, gate in sorted(suites.items()):
        if gate is None:
            suite_hits[path] = [flags for flags, _f, _t in test_legs]
            continue
        node = parse(gate)
        strays = sorted(feature_names(node, set()) - declared)
        if strays:
            unknown_features.append((gate, strays, f"{path}:1"))
            continue
        suite_hits[path] = [flags for flags, feats, t in test_legs
                            if evaluate(node, set(feats), t) >= UNKNOWN]

    if unknown_features:
        for pred, strays, site in unknown_features:
            print(f"feature-cfg-coverage: {site}: cfg names {', '.join(strays)}, which "
                  f"{'are' if len(strays) > 1 else 'is'} not a {CRATE} feature — the predicate "
                  f"can never be true, so nothing it gates is ever compiled", file=sys.stderr)
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
    #
    # `unique_suites` is the same check over suite BODIES, and it is what makes the deletion
    # advice safe to print. The advice used to be unconditional on the predicate side, so a leg
    # whose predicates evaporated was recommended for deletion even when it was the only
    # configuration compiling eight suite bodies with the combinator families off. Now an
    # evaporated predicate claim on a leg that still holds a suite is a REFUSAL: fix the claim,
    # keep the leg.
    sole: dict[str, list[str]] = {}
    for pred, hits in covering.items():
        if len(hits) == 1:
            sole.setdefault(hits[0], []).append(pred)
    features_of = {flags: feats for flags, feats, _t in all_legs}
    protected = load_bearing_suites(suite_hits, features_of)

    def listing(names, limit: int = 8) -> str:
        names = sorted(names)
        head = ", ".join(names[:limit])
        return head if len(names) <= limit else f"{head}, +{len(names) - limit} more"

    def absences(witness: dict[str, frozenset]) -> str:
        """`X, Y` — what this leg alone has OFF, reduced to the features that imply the rest.

        The raw set is every family the umbrella pulls, which reads as noise and buries the one
        name that carries the argument. `combinators` implies all thirteen, so the thirteen go.
        """
        union = frozenset().union(*witness.values()) if witness else frozenset()
        roots = [f for f in union
                 if not any(g != f and f in closure([g], fmap) for g in union)]
        return listing(roots, 3)

    drift = []
    for leg in EXTRA_LEGS:
        flags = leg_flags(leg)
        mine = sole.get(flags, [])
        witness = protected.get(flags, {})
        held = f"{len(witness)} integration suite{'' if len(witness) == 1 else 's'}"
        if leg["unique_predicates"] and not mine and witness:
            drift.append(
                f"leg `{flags}` declares unique_predicates=True, but every predicate it "
                f"satisfies is also satisfied by another leg. DO NOT DELETE THE LEG: it is "
                f"still the only leg that compiles {held} with {absences(witness)} off "
                f"({listing(witness.keys())}). Set unique_predicates=False; unique_suites=True "
                f"is what holds it now."
            )
        elif leg["unique_predicates"] and not mine:
            drift.append(
                f"leg `{flags}` declares unique_predicates=True, but every predicate it "
                f"satisfies is also satisfied by another leg, and no integration suite needs "
                f"it for a configuration nothing else provides either. Either the predicate it "
                f"existed for is gone — delete the leg — or the claim is wrong."
            )
        if not leg["unique_predicates"] and mine:
            drift.append(
                f"leg `{flags}` declares unique_predicates=False, but it is now the only leg "
                f"satisfying: {'; '.join('cfg(' + p + ')' for p in mine)}. Set it to True so "
                f"nobody prunes it as redundant."
            )
        if leg["unique_suites"] and not witness:
            drift.append(
                f"leg `{flags}` declares unique_suites=True, but every integration suite it "
                f"compiles is compiled by another leg with the same features off. Fix the "
                f"claim — and read what the leg's comment says it protects before concluding "
                f"it protects nothing."
            )
        if not leg["unique_suites"] and witness:
            drift.append(
                f"leg `{flags}` declares unique_suites=False, but it is now the only leg "
                f"compiling {held} with {absences(witness)} off ({listing(witness.keys())}). Set "
                f"it to True so nobody prunes it as redundant."
            )
    if drift:
        for d in drift:
            print(f"feature-cfg-coverage: {d}", file=sys.stderr)
        return 1

    # A green says what it looked at. A bare "OK" from a coverage gate is indistinguishable
    # from a coverage gate that found nothing to look at.
    #
    # THE PER-LEG FIGURES ARE PRINTED, NOT WRITTEN DOWN. The sentence they replace lived in a
    # comment beside the leg, was right when written and wrong three commits later, and was
    # carried through a release unrecomputed — because a comment is the one part of a gate
    # nothing recomputes. Anything a reader needs to know about a leg's size is derived here.
    each_feature = {flags for flags, _f, _t in all_legs} - {leg_flags(l) for l in EXTRA_LEGS}
    orphan_suites = sorted(p.name for p, hits in suite_hits.items() if not hits)
    gated = sum(1 for g in suites.values() if g)
    print(f"feature-cfg-coverage OK: {len(predicates)} multi-feature cfg predicates over "
          f"{sum(len(v) for v in predicates.values())} sites in {files} files, "
          f"{len(all_legs)} covering legs ({len(EXTRA_LEGS)} declared, --all-features excluded)")
    print(f"  and {len(suites)} integration-test targets under {TESTS}, {gated} of them "
          f"crate-gated")
    print("")
    print("declared legs:")
    for leg in EXTRA_LEGS:
        flags = leg_flags(leg)
        mine = sole.get(flags, [])
        witness = protected.get(flags, {})
        # What the leg reaches BEYOND `--each-feature` is the quantity that justifies it: the
        # `--each-feature` legs come free with the matrix, so a predicate they already satisfy
        # is not a reason to declare anything.
        beyond = [p for p, hits in covering.items()
                  if flags in hits and not each_feature.intersection(hits)]
        alone = {s: w for s, w in witness.items() if not w}
        shared = {s: w for s, w in witness.items() if w}
        print(f"  {flags}")
        print(f"      {len(beyond)} predicate{'' if len(beyond) == 1 else 's'} over "
              f"{sum(len(predicates[p]) for p in beyond)} sites that no --each-feature leg "
              f"reaches; sole cover of {len(mine)}")
        print(f"      {len(witness)} integration suite"
              f"{'' if len(witness) == 1 else 's'} no other leg compiles the same way")
        if alone:
            print(f"          {len(alone):3d} compiled by no other leg at all")
        if shared:
            print(f"          {len(shared):3d} compiled elsewhere, but never with "
                  f"{absences(shared)} off")
        print(f"      why: {leg['why']}")
    if orphan_suites:
        print("")
        print(f"{len(orphan_suites)} suite(s) no declared leg compiles — `--all-features` is "
              f"their only build, by design (see BOUNDS):")
        print(f"  {listing(orphan_suites)}")
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
